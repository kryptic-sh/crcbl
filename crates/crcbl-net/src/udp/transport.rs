//! One UDP link as a [`crate::Transport`].

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use hmac::digest::zeroize::Zeroizing;

use super::hello::{Challenge, HELLO_NONCE_BYTES, Hello, Reply};
use super::listener::Shared;
use super::session::{Session, send};
use super::token::NO_TOKEN;
use super::{CONNECT_TIMEOUT, HELLO_RESEND_INTERVAL, RECEIVE_BUDGET, RECEIVE_BUFFER_BYTES};
use crate::reliable::{
    Channel, Delivery, Endpoint, EndpointState, EndpointStats, MAX_RELIABLE_MESSAGE_BYTES,
    MAX_UNRELIABLE_PAYLOAD,
};
use crate::seal::{KeyPair, Role, X25519_BYTES, agree_channel};
use crate::{Clock, Message, SystemClock, Transport, TransportError};

/// Why a link ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// No hello reply came within [`CONNECT_TIMEOUT`]: nothing is listening
    /// at the address, it speaks another protocol id or
    /// [`super::TRANSPORT_VERSION`], it is full, or every token it challenged
    /// with was refused.
    ConnectTimedOut,
    /// The peer went silent for [`crate::reliable::PEER_TIMEOUT`] — gone
    /// without saying so.
    TimedOut,
    /// The peer closed the link and said so.
    PeerDisconnected,
    /// This side closed it: [`UdpTransport::disconnect`], or a drop.
    Closed,
    /// The seal ran out of counters under this key. Unreachable in practice
    /// (2^64 datagrams); a reconnect is the rekey.
    KeysExhausted,
}

/// Where a [`UdpTransport`] stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpState {
    /// The hello is out and no reply has come.
    Connecting,
    /// Keyed, and exchanging sealed datagrams.
    Connected,
    /// Over, and why.
    Ended(EndReason),
}

/// A link's health and what it dropped. Every drop counted here is a datagram
/// the link ignored, not one that hurt it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UdpStats {
    /// The packet layer's own figures: round trip, loss, resends, bytes.
    /// Zeros until the link is keyed.
    pub endpoint: EndpointStats,
    /// Datagrams from any address but the peer's. Always zero on a
    /// listener's peer, whose listener sorts datagrams by address first.
    pub foreign: u64,
    /// Datagrams from the peer's address while connecting that were neither
    /// a challenge nor a reply to this hello: malformed, another nonce, or a
    /// key that cannot agree one.
    pub stray: u64,
    /// Challenges to this hello taken while connecting: each carried a
    /// token, and the hello went out again at once presenting it.
    pub challenges: u64,
    /// Datagrams that did not open: forged, tampered, replayed, or not sealed
    /// at all.
    pub unopened: u64,
    /// Datagrams that opened and the packet layer then refused.
    pub refused: u64,
    /// Datagrams the socket would not send; each is lost as a dropped one is.
    pub send_failures: u64,
    /// Reads the socket answered with an error — on Windows, the ICMP
    /// "port unreachable" an earlier send provoked arrives this way.
    pub receive_errors: u64,
}

/// What the client holds between sending its hello and reading the reply.
struct Connecting {
    key_pair: KeyPair,
    hello: Hello,
    started: Duration,
    last_sent: Duration,
}

enum Phase<C: Clock> {
    Connecting(Box<Connecting>),
    Open(Box<Session<C>>),
    Ended(EndReason),
}

/// Whose socket a link sends and reads through.
enum Route<C: Clock + Clone> {
    /// A client's own, read directly.
    Own(UdpSocket),
    /// A listener's, shared by every peer it accepted and read through its
    /// demultiplexer.
    Shared(Arc<Shared<C>>),
}

impl<C: Clock + Clone> Route<C> {
    fn socket(&self) -> &UdpSocket {
        match self {
            Self::Own(socket) => socket,
            Self::Shared(shared) => shared.socket(),
        }
    }
}

/// Why the connect could not start.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// The local socket could not be bound or configured.
    #[error("socket: {0}")]
    Io(#[from] io::Error),
    /// The operating system's entropy source failed, so there is no secret
    /// to key the link with.
    #[error("the operating system's entropy source failed: {0}")]
    Entropy(#[from] getrandom::Error),
}

/// One UDP link, implementing [`Transport`]. See [`super`]'s docs.
///
/// Made by [`connect`](Self::connect) on a client, which binds an ephemeral
/// socket of its own, and by [`UdpListener::accept`](super::UdpListener::accept)
/// on a host, where every peer shares the listener's socket.
///
/// `send_reliable` rides [`Channel::Reliable`] — resent, ordered, fragmented
/// up to [`MAX_RELIABLE_MESSAGE_BYTES`] — and `send_unreliable`
/// [`Channel::UnreliableSequenced`], one datagram of at most
/// [`MAX_UNRELIABLE_PAYLOAD`], latest wins. A received [`Message`]'s `kind` is
/// the channel it came on.
///
/// **While connecting**, [`is_connected`](Transport::is_connected) is `true`
/// — up or coming up, as `crcbl-steam`'s `SteamTransport` answers — a send is
/// [`TransportError::Backpressure`], to try again on a later frame, and a
/// receive finds nothing. A connect nobody answers ends with
/// [`EndReason::ConnectTimedOut`]. However the link ends, a receive then
/// answers [`TransportError::Disconnected`], after the messages that arrived
/// before the end, and [`end_reason`](Self::end_reason) says why.
///
/// Dropping it closes the link: what is due goes out, then the disconnect
/// packets, so the peer reads [`EndReason::PeerDisconnected`] rather than
/// waiting out a timeout.
pub struct UdpTransport<C: Clock + Clone = SystemClock> {
    route: Route<C>,
    peer: SocketAddr,
    protocol_id: u32,
    clock: C,
    phase: Phase<C>,
    /// This link's counters; a session's own are folded in when it ends.
    stats: UdpStats,
}

impl UdpTransport<SystemClock> {
    /// Starts a connect to the listener at `server`, speaking `protocol_id`,
    /// timed by the wall clock. Returns at once; the link comes up over the
    /// following receives.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] when no socket could be bound or no secret drawn.
    pub fn connect(server: SocketAddr, protocol_id: u32) -> Result<Self, ConnectError> {
        Self::connect_with(server, protocol_id, SystemClock::new())
    }
}

impl<C: Clock + Clone> UdpTransport<C> {
    /// [`connect`](UdpTransport::connect), timed by `clock` — a
    /// [`crate::ManualClock`] makes the hello resends and every timeout exact.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] when no socket could be bound or no secret drawn.
    pub fn connect_with(
        server: SocketAddr,
        protocol_id: u32,
        clock: C,
    ) -> Result<Self, ConnectError> {
        let local: SocketAddr = if server.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(local)?;
        socket.set_nonblocking(true)?;
        // Fresh for every connection and never reused: this is the rekey.
        // `KeyPair` wipes its own copy on drop; `Zeroizing` wipes this one.
        let mut secret = Zeroizing::new([0u8; X25519_BYTES]);
        getrandom::fill(&mut secret[..])?;
        let mut nonce = [0u8; HELLO_NONCE_BYTES];
        getrandom::fill(&mut nonce)?;
        let key_pair = KeyPair::from_secret_bytes(*secret);
        let hello = Hello {
            nonce,
            public_key: key_pair.public_key(),
            token: NO_TOKEN,
        };
        let now = clock.now();
        let mut transport = Self {
            route: Route::Own(socket),
            peer: server,
            protocol_id,
            clock,
            phase: Phase::Connecting(Box::new(Connecting {
                key_pair,
                hello,
                started: now,
                last_sent: now,
            })),
            stats: UdpStats::default(),
        };
        transport.send_hello(now);
        Ok(transport)
    }

    /// A peer a listener has just confirmed.
    pub(crate) fn accepted(
        shared: Arc<Shared<C>>,
        peer: SocketAddr,
        protocol_id: u32,
        clock: C,
        session: Session<C>,
    ) -> Self {
        Self {
            route: Route::Shared(shared),
            peer,
            protocol_id,
            clock,
            phase: Phase::Open(Box::new(session)),
            stats: UdpStats::default(),
        }
    }

    /// Who is on the other end. Nothing authenticates it beyond the hello:
    /// see [`super`]'s docs on trust.
    #[must_use]
    pub const fn peer_addr(&self) -> SocketAddr {
        self.peer
    }

    /// The local address the link's socket is bound to — on a listener's
    /// peer, the listener's.
    ///
    /// # Errors
    ///
    /// The socket's own error when it cannot say.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.route.socket().local_addr()
    }

    /// Where the link stands, as of the last call that drove it.
    #[must_use]
    pub fn state(&self) -> UdpState {
        match &self.phase {
            Phase::Connecting(_) => UdpState::Connecting,
            Phase::Open(session) => match end_of(session.endpoint.state()) {
                None => UdpState::Connected,
                Some(reason) => UdpState::Ended(reason),
            },
            Phase::Ended(reason) => UdpState::Ended(*reason),
        }
    }

    /// Why the link ended, once it has.
    #[must_use]
    pub fn end_reason(&self) -> Option<EndReason> {
        match self.state() {
            UdpState::Ended(reason) => Some(reason),
            UdpState::Connecting | UdpState::Connected => None,
        }
    }

    /// The link's counters: what the netgraph reads, and what it dropped.
    #[must_use]
    pub fn stats(&self) -> UdpStats {
        let mut stats = self.stats;
        if let Phase::Open(session) = &self.phase {
            fold(&mut stats, session);
        }
        stats
    }

    /// Closes the link from this side: what is due goes out, then the
    /// disconnect packets. Later sends and receives answer
    /// [`TransportError::Disconnected`]. A link that had already ended keeps
    /// the reason it ended with.
    pub fn disconnect(&mut self) {
        match &mut self.phase {
            Phase::Connecting(_) => self.end(EndReason::Closed),
            Phase::Open(session) => {
                let reason = match end_of(session.endpoint.state()) {
                    Some(reason) => reason,
                    None => match session.close(
                        self.route.socket(),
                        self.peer,
                        &mut self.stats.send_failures,
                    ) {
                        Ok(()) => EndReason::Closed,
                        Err(_) => EndReason::KeysExhausted,
                    },
                };
                self.end(reason);
            }
            Phase::Ended(_) => {}
        }
    }

    // ── private ──────────────────────────────────────────────────────────

    /// Read what has arrived, then run the timers and send what is due.
    fn pump(&mut self) {
        if matches!(self.phase, Phase::Ended(_)) {
            return;
        }
        for datagram in self.read() {
            self.take(&datagram);
        }
        self.drive();
    }

    /// The datagrams waiting for this link.
    fn read(&mut self) -> Vec<Vec<u8>> {
        match &self.route {
            Route::Own(socket) => read_from(socket, self.peer, &mut self.stats),
            Route::Shared(shared) => shared.inbox(self.peer),
        }
    }

    /// One datagram from the peer's address.
    fn take(&mut self, datagram: &[u8]) {
        match &mut self.phase {
            Phase::Connecting(connecting) => {
                let nonce = connecting.hello.nonce;
                if let Some(challenge) = Challenge::decode(datagram, self.protocol_id)
                    .filter(|challenge| challenge.nonce == nonce)
                {
                    // The newest token wins: a listener that restarted, or
                    // whose last token expired, challenges again.
                    connecting.hello.token = challenge.token;
                    self.stats.challenges += 1;
                    let now = self.clock.now();
                    self.send_hello(now);
                    return;
                }
                let reply =
                    Reply::decode(datagram, self.protocol_id).filter(|reply| reply.nonce == nonce);
                let agreed = reply.and_then(|reply| {
                    agree_channel(
                        Role::Client,
                        &connecting.key_pair,
                        &reply.public_key,
                        self.protocol_id,
                    )
                    .ok()
                });
                let Some((sealer, opener)) = agreed else {
                    self.stats.stray += 1;
                    return;
                };
                let endpoint = Endpoint::new(self.protocol_id, self.clock.clone());
                let mut session = Session::new(sealer, opener, endpoint);
                // The listener admits the peer on its first sealed datagram;
                // send one now rather than a keepalive interval from now.
                session.endpoint.request_keepalive();
                self.phase = Phase::Open(Box::new(session));
            }
            Phase::Open(session) => session.receive(datagram),
            Phase::Ended(_) => {}
        }
    }

    /// Run the timers and send what is due.
    fn drive(&mut self) {
        let now = self.clock.now();
        match &mut self.phase {
            Phase::Connecting(connecting) => {
                if now.saturating_sub(connecting.started) >= CONNECT_TIMEOUT {
                    self.end(EndReason::ConnectTimedOut);
                } else if now.saturating_sub(connecting.last_sent) >= HELLO_RESEND_INTERVAL {
                    self.send_hello(now);
                }
            }
            Phase::Open(session) => {
                let flushed = session.flush(
                    self.route.socket(),
                    self.peer,
                    &mut self.stats.send_failures,
                );
                if flushed.is_err() {
                    self.end(EndReason::KeysExhausted);
                }
            }
            Phase::Ended(_) => {}
        }
    }

    fn send_hello(&mut self, now: Duration) {
        if let Phase::Connecting(connecting) = &mut self.phase {
            let datagram = connecting.hello.encode(self.protocol_id);
            send(
                self.route.socket(),
                &datagram,
                self.peer,
                &mut self.stats.send_failures,
            );
            connecting.last_sent = now;
        }
    }

    /// End the link, keeping the session's counters, and free the listener's
    /// slot for this peer.
    fn end(&mut self, reason: EndReason) {
        if let Phase::Open(session) = &self.phase {
            fold(&mut self.stats, session);
        }
        self.phase = Phase::Ended(reason);
        if let Route::Shared(shared) = &self.route {
            shared.forget(self.peer);
        }
    }

    fn send(&mut self, channel: Channel, payload: Vec<u8>) -> Result<(), TransportError> {
        let limit = match channel {
            Channel::Reliable => MAX_RELIABLE_MESSAGE_BYTES,
            Channel::UnreliableSequenced => MAX_UNRELIABLE_PAYLOAD,
        };
        if payload.len() > limit {
            return Err(TransportError::MessageTooLarge {
                size: payload.len(),
                limit,
            });
        }
        match &mut self.phase {
            Phase::Connecting(_) => return Err(TransportError::Backpressure),
            Phase::Open(session) => session.endpoint.send(channel, payload)?,
            Phase::Ended(_) => return Err(TransportError::Disconnected),
        }
        self.drive();
        Ok(())
    }

    /// The next delivery `next` picks, or [`TransportError::Disconnected`]
    /// once the link has ended and `next` finds nothing more.
    ///
    /// `drains_everything` says `next` looks at every delivery: only then
    /// is an empty answer proof that nothing is left, and the link is ended
    /// for good. `recv_reliable` finding no reliable message must not throw
    /// away unreliable ones `recv` would still hand over.
    fn deliver(
        &mut self,
        next: impl FnOnce(&mut Endpoint<C>) -> Option<Delivery>,
        drains_everything: bool,
    ) -> Result<Option<Message>, TransportError> {
        self.pump();
        let reason = match &mut self.phase {
            Phase::Connecting(_) => return Ok(None),
            Phase::Ended(_) => return Err(TransportError::Disconnected),
            Phase::Open(session) => {
                if let Some(delivery) = next(&mut session.endpoint) {
                    return Ok(Some(Message {
                        kind: delivery.channel.into(),
                        payload: delivery.payload,
                    }));
                }
                match end_of(session.endpoint.state()) {
                    None => return Ok(None),
                    Some(reason) => reason,
                }
            }
        };
        if drains_everything {
            self.end(reason);
        }
        Err(TransportError::Disconnected)
    }
}

impl<C: Clock + Clone> Transport for UdpTransport<C> {
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.send(Channel::Reliable, msg.payload)
    }

    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.send(Channel::UnreliableSequenced, msg.payload)
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.deliver(Endpoint::recv_reliable, false)
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.deliver(Endpoint::recv, true)
    }

    /// Up, or coming up: `true` while connecting, and until the link ends.
    fn is_connected(&self) -> bool {
        matches!(self.state(), UdpState::Connecting | UdpState::Connected)
    }

    /// One datagram's payload: the unreliable channel never fragments.
    fn max_unreliable_message_bytes(&self) -> usize {
        MAX_UNRELIABLE_PAYLOAD
    }

    /// [`UdpStats::endpoint`] once the link is keyed; `None` while
    /// connecting, when there is no packet layer yet to measure anything.
    fn link_stats(&self) -> Option<EndpointStats> {
        match self.state() {
            UdpState::Connecting => None,
            UdpState::Connected | UdpState::Ended(_) => Some(self.stats().endpoint),
        }
    }
}

impl<C: Clock + Clone> Drop for UdpTransport<C> {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl<C: Clock + Clone> std::fmt::Debug for UdpTransport<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpTransport")
            .field("peer", &self.peer)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

/// How an endpoint that is no longer connected ended, or `None` while it is.
fn end_of(state: EndpointState) -> Option<EndReason> {
    match state {
        EndpointState::Connected => None,
        EndpointState::Disconnecting | EndpointState::Disconnected => Some(EndReason::Closed),
        EndpointState::PeerDisconnected => Some(EndReason::PeerDisconnected),
        EndpointState::TimedOut => Some(EndReason::TimedOut),
    }
}

/// Add a session's figures to a link's own.
fn fold<C: Clock>(stats: &mut UdpStats, session: &Session<C>) {
    stats.endpoint = session.endpoint.stats();
    stats.unopened += session.refusals.unopened;
    stats.refused += session.refusals.refused;
}

/// Read up to [`RECEIVE_BUDGET`] datagrams from a client's own socket,
/// keeping those from `peer` and counting the rest.
fn read_from(socket: &UdpSocket, peer: SocketAddr, stats: &mut UdpStats) -> Vec<Vec<u8>> {
    let mut buffer = [0u8; RECEIVE_BUFFER_BYTES];
    let mut datagrams = Vec::new();
    for _ in 0..RECEIVE_BUDGET {
        match socket.recv_from(&mut buffer) {
            Ok((_, from)) if from != peer => stats.foreign += 1,
            Ok((len, _)) => datagrams.push(buffer[..len].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(_) => stats.receive_errors += 1,
        }
    }
    datagrams
}
