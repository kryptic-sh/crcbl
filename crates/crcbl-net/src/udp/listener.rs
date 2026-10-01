//! A host's socket: hellos answered, datagrams sorted by address, one
//! [`UdpTransport`] per admitted peer.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use hmac::digest::zeroize::Zeroizing;

use super::hello::{Challenge, HELLO_TAG, Hello, REPLY_BYTES, Reply};
use super::session::{Session, send};
use super::token::{TOKEN_KEY_BYTES, TokenError, TokenKey};
use super::{
    DEFAULT_MAX_PEERS, DEFAULT_MAX_PENDING, HANDSHAKE_TIMEOUT, MAX_SPENT_TOKENS, RECEIVE_BUDGET,
    RECEIVE_BUFFER_BYTES, TOKEN_LIFETIME, UdpTransport,
};
use crate::reliable::{Endpoint, MAX_FRAGMENT_BYTES, MAX_RELIABLE_BYTES_IN_FLIGHT};
use crate::seal::{KeyPair, Opener, Role, SEALED_TAG, Sealer, X25519_BYTES, agree_channel};
use crate::{Clock, SystemClock};

/// Datagrams held for one peer between reads of its transport; past it, the
/// newest are dropped and counted, and the reliable channel resends them.
/// Room for the most fragments a sender may have in flight, so an honest
/// peer's burst is never cut by this alone.
pub const MAX_PEER_INBOX: usize = 1024;

const _: () = assert!(MAX_PEER_INBOX >= MAX_RELIABLE_BYTES_IN_FLIGHT / MAX_FRAGMENT_BYTES);

/// How a [`UdpListener`] admits peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerConfig {
    /// The endpoint protocol id every peer must share: a hello carrying
    /// another is not answered, and it is bound into the session keys.
    pub protocol_id: u32,
    /// Peers held at once, accepted or waiting to be. A hello past it goes
    /// unanswered, and the client's connect times out.
    pub max_peers: usize,
    /// Pending handshakes held at once — hellos answered, no sealed datagram
    /// yet. A hello past it goes unanswered.
    pub max_pending: usize,
}

impl ListenerConfig {
    /// [`DEFAULT_MAX_PEERS`] and [`DEFAULT_MAX_PENDING`] for `protocol_id`.
    #[must_use]
    pub const fn new(protocol_id: u32) -> Self {
        Self {
            protocol_id,
            max_peers: DEFAULT_MAX_PEERS,
            max_pending: DEFAULT_MAX_PENDING,
        }
    }
}

/// What a listener holds and what it turned away. Every refusal counted here
/// is a datagram it ignored; none of them touches an admitted peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListenerStats {
    /// Peers held now, accepted or waiting for [`UdpListener::accept`].
    pub peers: usize,
    /// Pending handshakes held now.
    pub pending: usize,
    /// Hellos answered with a reply and a pending handshake.
    pub hellos_answered: u64,
    /// Hellos answered with a [`Challenge`](super::Challenge) instead: no
    /// token, or one counted below as refused. Each held nothing and cost no
    /// key agreement.
    pub challenges: u64,
    /// Hellos whose token was not one this listener minted for that address
    /// and protocol id — forged, altered, from another address, or minted
    /// before a restart — or not a token at all.
    pub tokens_forged: u64,
    /// Hellos whose token was authentic and past
    /// [`TOKEN_LIFETIME`](super::TOKEN_LIFETIME).
    pub tokens_expired: u64,
    /// Hellos whose token had already bought a handshake.
    pub tokens_spent: u64,
    /// Hellos with a valid token turned away because
    /// [`MAX_SPENT_TOKENS`](super::MAX_SPENT_TOKENS) spent tokens were
    /// already remembered.
    pub spent_full: u64,
    /// Hellos turned away because [`ListenerConfig::max_pending`] handshakes
    /// were already pending.
    pub pending_full: u64,
    /// Hellos, and confirmations, turned away because
    /// [`ListenerConfig::max_peers`] peers were already held.
    pub peers_full: u64,
    /// Hellos from an address whose pending handshake has another key: the
    /// first stands until it confirms or expires, so a spoofer cannot swap a
    /// waiting client's key for its own.
    pub conflicting: u64,
    /// Datagrams tagged as a hello that were not one: wrong length, version
    /// or protocol id, or a key no agreement can use.
    pub malformed: u64,
    /// Datagrams from an address with neither a peer nor a pending
    /// handshake, that were not a hello — and every hello once the listener
    /// is dropped.
    pub unknown_source: u64,
    /// Sealed datagrams for a pending handshake that did not open under its
    /// key.
    pub unconfirmed: u64,
    /// Pending handshakes dropped after [`HANDSHAKE_TIMEOUT`] unconfirmed.
    pub expired: u64,
    /// Datagrams dropped because their peer's inbox held
    /// [`MAX_PEER_INBOX`] already.
    pub inbox_overflow: u64,
    /// Hellos with a valid token left unanswered because the operating
    /// system's entropy source failed.
    pub entropy_failures: u64,
    /// Datagrams the socket would not send.
    pub send_failures: u64,
    /// Reads the socket answered with an error.
    pub receive_errors: u64,
}

/// A host's UDP socket, admitting peers by the hello.
///
/// **Call [`accept`](Self::accept) every frame.** Nothing runs a thread:
/// hellos are answered, pending handshakes expire and datagrams reach their
/// peers only when this socket is read, which `accept` and every accepted
/// peer's receive do.
///
/// A hello without a token valid for its address is answered with a
/// challenge carrying one, and nothing is held for it. A hello presenting a
/// valid token is answered with the reply only while fewer than
/// [`ListenerConfig::max_peers`] peers and [`ListenerConfig::max_pending`]
/// handshakes are held; its sender becomes a peer only when a sealed datagram
/// opens under the new key — see [`super`]'s docs for why that is the proof —
/// and `accept` hands it out then. A peer's slot is freed when its transport
/// reports the link's end from a receive, closes, or is dropped.
///
/// Every accepted [`UdpTransport`] shares this socket, and keeps it open
/// while it lives. Dropping the listener stops admitting — later hellos go
/// unanswered, peers confirmed but not yet accepted are sent a disconnect —
/// and peers already accepted carry on.
pub struct UdpListener<C: Clock + Clone = SystemClock> {
    shared: Arc<Shared<C>>,
}

impl UdpListener<SystemClock> {
    /// Binds `addr` for peers speaking `protocol_id`, with the default caps,
    /// timed by the wall clock. Port 0 picks a free one; see
    /// [`local_addr`](Self::local_addr).
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be bound or made non-blocking, and
    /// an [`io::ErrorKind::Other`] carrying the entropy source's error when
    /// no token key could be drawn.
    pub fn bind(addr: impl ToSocketAddrs, protocol_id: u32) -> io::Result<Self> {
        Self::bind_with(addr, ListenerConfig::new(protocol_id), SystemClock::new())
    }
}

impl<C: Clock + Clone> UdpListener<C> {
    /// [`bind`](UdpListener::bind) with `config`'s caps, timed by `clock`.
    ///
    /// Every bind draws a new token key, so tokens minted by an earlier
    /// listener — this process's or one before a restart — do not verify.
    ///
    /// # Errors
    ///
    /// As [`bind`](UdpListener::bind).
    pub fn bind_with(
        addr: impl ToSocketAddrs,
        config: ListenerConfig,
        clock: C,
    ) -> io::Result<Self> {
        let mut secret = Zeroizing::new([0u8; TOKEN_KEY_BYTES]);
        getrandom::fill(&mut secret[..]).map_err(io::Error::other)?;
        let tokens = TokenKey::from_secret_bytes(*secret);
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            shared: Arc::new(Shared {
                socket,
                demux: Mutex::new(Demux {
                    config,
                    clock,
                    listening: true,
                    tokens,
                    next_serial: 0,
                    spent: HashMap::new(),
                    pending: HashMap::new(),
                    inboxes: HashMap::new(),
                    ready: VecDeque::new(),
                    stats: ListenerStats::default(),
                }),
            }),
        })
    }

    /// The address the socket is bound to.
    ///
    /// # Errors
    ///
    /// The socket's own error when it cannot say.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.shared.socket.local_addr()
    }

    /// Reads the socket, then hands out the next confirmed peer, or `None`
    /// when none is waiting.
    pub fn accept(&mut self) -> Option<UdpTransport<C>> {
        let mut demux = self.shared.lock();
        demux.pump(&self.shared.socket);
        let (peer, session) = demux.ready.pop_front()?;
        let clock = demux.clock.clone();
        let protocol_id = demux.config.protocol_id;
        drop(demux);
        Some(UdpTransport::accepted(
            Arc::clone(&self.shared),
            peer,
            protocol_id,
            clock,
            session,
        ))
    }

    /// What the listener holds and has turned away, as of the last read.
    #[must_use]
    pub fn stats(&self) -> ListenerStats {
        let demux = self.shared.lock();
        ListenerStats {
            peers: demux.inboxes.len(),
            pending: demux.pending.len(),
            ..demux.stats
        }
    }
}

impl<C: Clock + Clone> Drop for UdpListener<C> {
    fn drop(&mut self) {
        let mut demux = self.shared.lock();
        demux.listening = false;
        demux.pending.clear();
        let Demux {
            ready,
            inboxes,
            stats,
            ..
        } = &mut *demux;
        for (peer, mut session) in ready.drain(..) {
            // A fresh session has every seal counter left, so this cannot
            // fail; if it ever did, the disconnect did not go out, which is
            // a send failure, and the peer times out instead.
            if session
                .close(&self.shared.socket, peer, &mut stats.send_failures)
                .is_err()
            {
                stats.send_failures += 1;
            }
            inboxes.remove(&peer);
        }
    }
}

impl<C: Clock + Clone> std::fmt::Debug for UdpListener<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpListener")
            .field("local_addr", &self.shared.socket.local_addr().ok())
            .field("stats", &self.stats())
            .finish()
    }
}

/// The socket and the demultiplexer, shared by a listener and its peers.
pub(crate) struct Shared<C: Clock + Clone> {
    socket: UdpSocket,
    demux: Mutex<Demux<C>>,
}

impl<C: Clock + Clone> Shared<C> {
    pub(crate) fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    /// Reads the socket, then takes everything waiting for `peer`.
    pub(crate) fn inbox(&self, peer: SocketAddr) -> Vec<Vec<u8>> {
        let mut demux = self.lock();
        demux.pump(&self.socket);
        demux
            .inboxes
            .get_mut(&peer)
            .map(|inbox| inbox.drain(..).collect())
            .unwrap_or_default()
    }

    /// Frees `peer`'s slot: its link has ended.
    pub(crate) fn forget(&self, peer: SocketAddr) {
        self.lock().inboxes.remove(&peer);
    }

    fn lock(&self) -> MutexGuard<'_, Demux<C>> {
        // Every update to the table is a handful of map operations with no
        // panic between them, so a thread that panicked elsewhere while
        // holding the lock left it consistent.
        self.demux.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A hello answered, waiting for the client's first sealed datagram.
struct Pending {
    sealer: Sealer,
    opener: Opener,
    /// The hello it answered: a repeat of it — the same nonce and key,
    /// whatever token — is answered again.
    hello: Hello,
    /// The reply, kept to resend when the client repeats its hello because
    /// the first reply was lost.
    reply: [u8; REPLY_BYTES],
    since: Duration,
}

/// Who each datagram is for.
struct Demux<C: Clock + Clone> {
    config: ListenerConfig,
    clock: C,
    /// `false` once the listener is dropped: nobody will accept a new peer.
    listening: bool,
    /// What this run's tokens are minted and verified under.
    tokens: TokenKey,
    /// The serial the next token is minted with.
    next_serial: u64,
    /// The serials of tokens that have bought a handshake, each with its
    /// expiry: kept until then, after which the expiry refuses it anyway.
    spent: HashMap<u64, Duration>,
    pending: HashMap<SocketAddr, Pending>,
    /// Every peer, accepted or ready, and the datagrams waiting for it.
    inboxes: HashMap<SocketAddr, VecDeque<Vec<u8>>>,
    /// Peers confirmed and not yet accepted.
    ready: VecDeque<(SocketAddr, Session<C>)>,
    stats: ListenerStats,
}

impl<C: Clock + Clone> Demux<C> {
    /// Read up to [`RECEIVE_BUDGET`] datagrams and sort each, then expire
    /// the pending handshakes that have waited too long.
    fn pump(&mut self, socket: &UdpSocket) {
        let now = self.clock.now();
        let mut buffer = [0u8; RECEIVE_BUFFER_BYTES];
        for _ in 0..RECEIVE_BUDGET {
            match socket.recv_from(&mut buffer) {
                Ok((len, from)) => self.sort(socket, &buffer[..len], from, now),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // Windows reports an ICMP "port unreachable" for an earlier
                // send as an error on this read. It says nothing about the
                // datagrams still queued, so the read carries on.
                Err(_) => self.stats.receive_errors += 1,
            }
        }
        let before = self.pending.len();
        self.pending
            .retain(|_, pending| now.saturating_sub(pending.since) < HANDSHAKE_TIMEOUT);
        self.stats.expired += (before - self.pending.len()) as u64;
        self.spent.retain(|_, expires| now < *expires);
    }

    fn sort(&mut self, socket: &UdpSocket, datagram: &[u8], from: SocketAddr, now: Duration) {
        if let Some(inbox) = self.inboxes.get_mut(&from) {
            // The peer's own opener judges it, and counts what it refuses.
            if inbox.len() >= MAX_PEER_INBOX {
                self.stats.inbox_overflow += 1;
            } else {
                inbox.push_back(datagram.to_vec());
            }
            return;
        }
        match datagram.first() {
            Some(&HELLO_TAG) if self.listening => self.hello(socket, datagram, from, now),
            Some(&SEALED_TAG) if self.pending.contains_key(&from) => self.confirm(datagram, from),
            _ => self.stats.unknown_source += 1,
        }
    }

    /// Answer a hello: with a challenge unless it presents a token valid for
    /// its address, and otherwise with a reply and a pending handshake.
    fn hello(&mut self, socket: &UdpSocket, datagram: &[u8], from: SocketAddr, now: Duration) {
        let protocol_id = self.config.protocol_id;
        let Some(hello) = Hello::decode(datagram, protocol_id) else {
            self.stats.malformed += 1;
            return;
        };
        // A repeat of the hello a pending handshake answered — its reply was
        // lost — is answered again before its token is looked at: it is the
        // same handshake, not a second use of a token. The token is left out
        // of the match because a client whose hello was resent before the
        // first challenge came back holds a second challenge's token by the
        // time it repeats itself, and must still get its reply.
        if let Some(pending) = self.pending.get(&from) {
            if (pending.hello.nonce, pending.hello.public_key) == (hello.nonce, hello.public_key) {
                send(socket, &pending.reply, from, &mut self.stats.send_failures);
            } else {
                self.stats.conflicting += 1;
            }
            return;
        }
        let token = match self.tokens.verify(&hello.token, from, protocol_id, now) {
            Ok(token) if !self.spent.contains_key(&token.serial) => token,
            refused => {
                match refused {
                    Ok(_) => self.stats.tokens_spent += 1,
                    Err(TokenError::Absent) => {}
                    Err(TokenError::Malformed | TokenError::Forged) => {
                        self.stats.tokens_forged += 1;
                    }
                    Err(TokenError::Expired) => self.stats.tokens_expired += 1,
                }
                self.challenge(socket, &hello, from, now);
                return;
            }
        };
        if self.inboxes.len() >= self.config.max_peers {
            self.stats.peers_full += 1;
            return;
        }
        if self.pending.len() >= self.config.max_pending {
            self.stats.pending_full += 1;
            return;
        }
        if self.spent.len() >= MAX_SPENT_TOKENS {
            self.stats.spent_full += 1;
            return;
        }
        // Fresh for every connection and never reused: this is the rekey.
        let mut secret = Zeroizing::new([0u8; X25519_BYTES]);
        if getrandom::fill(&mut secret[..]).is_err() {
            self.stats.entropy_failures += 1;
            return;
        }
        let key_pair = KeyPair::from_secret_bytes(*secret);
        let Ok((sealer, opener)) =
            agree_channel(Role::Server, &key_pair, &hello.public_key, protocol_id)
        else {
            self.stats.malformed += 1;
            return;
        };
        let reply = Reply {
            nonce: hello.nonce,
            public_key: key_pair.public_key(),
        }
        .encode(protocol_id);
        send(socket, &reply, from, &mut self.stats.send_failures);
        self.spent.insert(token.serial, token.expires);
        self.pending.insert(
            from,
            Pending {
                sealer,
                opener,
                hello,
                reply,
                since: now,
            },
        );
        self.stats.hellos_answered += 1;
    }

    /// Answer `hello` with a fresh token for `from`. Stateless: nothing is
    /// held for it, so a flood of these costs one HMAC and one datagram no
    /// longer than the hello, each.
    fn challenge(&mut self, socket: &UdpSocket, hello: &Hello, from: SocketAddr, now: Duration) {
        let serial = self.next_serial;
        // Unreachable in practice (2^64 challenges). If it ever wrapped, a
        // repeated serial could only be refused as spent, never accepted.
        self.next_serial = serial.wrapping_add(1);
        let protocol_id = self.config.protocol_id;
        let token = self.tokens.mint(
            from,
            protocol_id,
            now.saturating_add(TOKEN_LIFETIME),
            serial,
        );
        let challenge = Challenge {
            nonce: hello.nonce,
            token,
        }
        .encode(protocol_id);
        send(socket, &challenge, from, &mut self.stats.send_failures);
        self.stats.challenges += 1;
    }

    /// A sealed datagram for a pending handshake: if it opens, the client
    /// holds the key, and becomes a peer.
    fn confirm(&mut self, datagram: &[u8], from: SocketAddr) {
        let Some(mut pending) = self.pending.remove(&from) else {
            self.stats.unknown_source += 1;
            return;
        };
        let packet = match pending.opener.open(datagram) {
            Ok(packet) => packet,
            Err(_) => {
                self.stats.unconfirmed += 1;
                self.pending.insert(from, pending);
                return;
            }
        };
        if self.inboxes.len() >= self.config.max_peers {
            self.stats.peers_full += 1;
            return;
        }
        let endpoint = Endpoint::new(self.config.protocol_id, self.clock.clone());
        let mut session = Session::new(pending.sealer, pending.opener, endpoint);
        session.receive_packet(&packet);
        self.inboxes.insert(from, VecDeque::new());
        self.ready.push_back((from, session));
    }
}
