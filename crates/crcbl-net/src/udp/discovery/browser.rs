//! A client's view of the LAN: queries out, announces in, one entry per host.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use super::wire::{ANNOUNCE_TAG, Announcement, Refusal, encode_query};
use super::{
    DEFAULT_MAX_HOSTS, DISCOVERY_BUFFER_BYTES, DISCOVERY_PORT, HOST_EXPIRY, QUERY_INTERVAL,
};
use crate::udp::RECEIVE_BUDGET;
use crate::udp::session::send;
use crate::{Clock, ProtocolCompatibility, SystemClock};

/// One host a [`Browser`] has heard from — a hint, not a verified host; see
/// [`super`]'s docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    /// The host's name, as it announced it: at most
    /// [`super::MAX_NAME_BYTES`] of UTF-8 with no control characters.
    pub name: String,
    /// Where to connect: the IP the announce came from, and the game port it
    /// named. Never an address read out of the datagram.
    pub addr: SocketAddr,
    /// Players the host said it has.
    pub players: u16,
    /// Players the host said it takes.
    pub max_players: u16,
    /// What the host said its session handshake gates on.
    pub compatibility: ProtocolCompatibility,
    /// The browser's clock when the host was last heard from.
    pub last_seen: Duration,
}

/// How a [`Browser`] looks for hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserConfig {
    /// The endpoint protocol id this client's transport speaks. Hosts on
    /// another cannot be connected to at all, and are not listed.
    pub protocol_id: u32,
    /// Where the query goes every [`QUERY_INTERVAL`], or `None` to send none
    /// and only listen — for a browser bound to [`DISCOVERY_PORT`] itself.
    pub query_to: Option<SocketAddr>,
    /// Hosts listed at once. An announce from a new host past it is dropped
    /// and counted; hosts already listed are still refreshed.
    pub max_hosts: usize,
}

impl BrowserConfig {
    /// Querying the IPv4 broadcast address on [`DISCOVERY_PORT`], listing up
    /// to [`DEFAULT_MAX_HOSTS`] hosts on `protocol_id`.
    #[must_use]
    pub const fn new(protocol_id: u32) -> Self {
        Self {
            protocol_id,
            query_to: Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::BROADCAST,
                DISCOVERY_PORT,
            ))),
            max_hosts: DEFAULT_MAX_HOSTS,
        }
    }
}

/// What a browser lists and what it dropped. Every drop counted here is a
/// datagram it ignored; none of them touches a listed host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrowserStats {
    /// Hosts listed now.
    pub hosts: usize,
    /// Announces read and recorded, new hosts and refreshes alike.
    pub announces: u64,
    /// Queries sent, periodic and directed.
    pub queries_sent: u64,
    /// Datagrams tagged as an announce that were not one: see
    /// `Announcement::decode` for every reason.
    pub malformed: u64,
    /// Announces in another discovery version.
    pub other_version: u64,
    /// Announces from hosts on another protocol id.
    pub other_protocol: u64,
    /// Datagrams that were not an announce at all: other browsers' queries,
    /// on the discovery port, and anything else.
    pub not_announce: u64,
    /// Announces from a new host dropped because
    /// [`BrowserConfig::max_hosts`] were already listed.
    pub hosts_full: u64,
    /// Hosts dropped after [`HOST_EXPIRY`] unheard.
    pub expired: u64,
    /// Queries the socket would not send.
    pub send_failures: u64,
    /// Reads the socket answered with an error — on Windows, the ICMP
    /// "port unreachable" a query to a silent address provokes, and a
    /// datagram too long for the receive buffer.
    pub receive_errors: u64,
}

/// A client's LAN host list.
///
/// **Call [`poll`](Self::poll) every frame** while the list is on screen:
/// that sends the query when one is due, reads the replies, and ages out
/// hosts that went quiet. Stop polling and nothing is sent. The list lives
/// only as long as the browser — no registry, nothing kept between runs.
pub struct Browser<C: Clock = SystemClock> {
    socket: UdpSocket,
    config: BrowserConfig,
    clock: C,
    hosts: Hosts,
    last_query: Option<Duration>,
    stats: BrowserStats,
}

impl Browser<SystemClock> {
    /// A browser on an ephemeral port of every IPv4 interface, querying the
    /// broadcast address for hosts on `protocol_id` — any number of them run
    /// on one machine; see [`super`]'s docs for why it does not bind
    /// [`DISCOVERY_PORT`].
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be bound, made non-blocking or
    /// allowed to broadcast.
    pub fn open(protocol_id: u32) -> io::Result<Self> {
        Self::bind_with(
            (Ipv4Addr::UNSPECIFIED, 0),
            BrowserConfig::new(protocol_id),
            SystemClock::new(),
        )
    }
}

impl<C: Clock> Browser<C> {
    /// A browser bound to `addr`, looking for hosts as `config` says, timed
    /// by `clock`. Binding [`DISCOVERY_PORT`] hears hosts' broadcasts
    /// without asking, and fails when anything else on the machine holds it.
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be bound, made non-blocking or
    /// allowed to broadcast.
    pub fn bind_with(
        addr: impl ToSocketAddrs,
        config: BrowserConfig,
        clock: C,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            config,
            clock,
            hosts: Hosts::default(),
            last_query: None,
            stats: BrowserStats::default(),
        })
    }

    /// The address the socket is bound to.
    ///
    /// # Errors
    ///
    /// The socket's own error when it cannot say.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Sends the query if [`QUERY_INTERVAL`] has passed since the last (the
    /// first poll always sends), reads up to [`RECEIVE_BUDGET`] datagrams,
    /// then drops hosts unheard for [`HOST_EXPIRY`]. Never blocks.
    pub fn poll(&mut self) {
        let now = self.clock.now();
        if let Some(to) = self.config.query_to
            && self
                .last_query
                .is_none_or(|last| now.saturating_sub(last) >= QUERY_INTERVAL)
        {
            self.query(to);
            self.last_query = Some(now);
        }
        let mut buffer = [0u8; DISCOVERY_BUFFER_BYTES];
        for _ in 0..RECEIVE_BUDGET {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, from)) => {
                    self.hosts
                        .receive(&buffer[..len], from, now, &self.config, &mut self.stats)
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // An error on this read says nothing about the datagrams
                // still queued, so the read carries on — as the listener's.
                Err(_) => self.stats.receive_errors += 1,
            }
        }
        self.hosts.expire(now, &mut self.stats);
    }

    /// Sends one query to `to` now: a host's address to ask it directly, or a
    /// broadcast address. Its reply is read by a later
    /// [`poll`](Self::poll).
    pub fn query(&mut self, to: SocketAddr) {
        send(
            &self.socket,
            &encode_query(self.config.protocol_id),
            to,
            &mut self.stats.send_failures,
        );
        self.stats.queries_sent += 1;
    }

    /// Every host heard from within [`HOST_EXPIRY`] as of the last poll, by
    /// name and then address, so a list redrawn every frame does not
    /// shuffle.
    #[must_use]
    pub fn hosts(&self) -> Vec<HostEntry> {
        self.hosts.sorted()
    }

    /// What the browser lists and has dropped, as of the last poll.
    #[must_use]
    pub fn stats(&self) -> BrowserStats {
        BrowserStats {
            hosts: self.hosts.entries.len(),
            ..self.stats
        }
    }

    /// Whether the socket may broadcast, as the operating system reports it.
    #[cfg(test)]
    pub(super) fn may_broadcast(&self) -> io::Result<bool> {
        self.socket.broadcast()
    }
}

impl<C: Clock> std::fmt::Debug for Browser<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("local_addr", &self.socket.local_addr().ok())
            .field("config", &self.config)
            .field("stats", &self.stats())
            .finish()
    }
}

/// The host list itself, apart from any socket, so its rules can be tested
/// from any source address.
#[derive(Debug, Default)]
pub(super) struct Hosts {
    /// Keyed by where a client would connect — the source IP and announced
    /// port — so a host's broadcasts and its replies to queries are one row.
    entries: HashMap<SocketAddr, HostEntry>,
}

impl Hosts {
    /// Records the announce `datagram` from `from`, or counts why not.
    pub(super) fn receive(
        &mut self,
        datagram: &[u8],
        from: SocketAddr,
        now: Duration,
        config: &BrowserConfig,
        stats: &mut BrowserStats,
    ) {
        if datagram.first() != Some(&ANNOUNCE_TAG) {
            stats.not_announce += 1;
            return;
        }
        let announcement = match Announcement::decode(datagram) {
            Ok(announcement) => announcement,
            Err(Refusal::OtherVersion) => {
                stats.other_version += 1;
                return;
            }
            Err(Refusal::Malformed) => {
                stats.malformed += 1;
                return;
            }
        };
        if announcement.protocol_id != config.protocol_id {
            stats.other_protocol += 1;
            return;
        }
        // The source IP, never anything the payload says: see `super`.
        let addr = SocketAddr::new(from.ip(), announcement.game_port.get());
        if !self.entries.contains_key(&addr) && self.entries.len() >= config.max_hosts {
            stats.hosts_full += 1;
            return;
        }
        let entry = HostEntry {
            name: announcement.name().to_owned(),
            addr,
            players: announcement.players,
            max_players: announcement.max_players,
            compatibility: announcement.compatibility,
            last_seen: now,
        };
        self.entries.insert(addr, entry);
        stats.announces += 1;
    }

    /// Drops every host unheard for [`HOST_EXPIRY`] as of `now`.
    pub(super) fn expire(&mut self, now: Duration, stats: &mut BrowserStats) {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| now.saturating_sub(entry.last_seen) < HOST_EXPIRY);
        stats.expired += (before - self.entries.len()) as u64;
    }

    /// Every entry, by name and then address.
    pub(super) fn sorted(&self) -> Vec<HostEntry> {
        let mut hosts: Vec<HostEntry> = self.entries.values().cloned().collect();
        hosts.sort_by(|a, b| a.name.cmp(&b.name).then(a.addr.cmp(&b.addr)));
        hosts
    }
}
