//! A host's voice on the LAN: queries answered, and a periodic broadcast.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use super::wire::{ANNOUNCE_BYTES, Announcement, QUERY_TAG, Refusal, decode_query};
use super::{ANNOUNCE_INTERVAL, DISCOVERY_BUFFER_BYTES, DISCOVERY_PORT};
use crate::udp::RECEIVE_BUDGET;
use crate::udp::session::send;
use crate::{Clock, SystemClock};

/// What an announcer sent and what it ignored. Every drop counted here is a
/// datagram it left unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnnouncerStats {
    /// Queries answered with the announce.
    pub queries_answered: u64,
    /// Periodic broadcasts sent.
    pub broadcasts: u64,
    /// Datagrams tagged as a query that were not one — a short, unpadded
    /// query among them, which is the amplification attempt the padding
    /// rule refuses; see [`super`]'s docs.
    pub malformed: u64,
    /// Queries in another discovery version.
    pub other_version: u64,
    /// Queries for another protocol id.
    pub other_protocol: u64,
    /// Datagrams that were not a query at all: other hosts' broadcasts, and
    /// this one's own where the network loops it back.
    pub not_query: u64,
    /// Announces the socket would not send.
    pub send_failures: u64,
    /// Reads the socket answered with an error.
    pub receive_errors: u64,
}

/// A host's announcer.
///
/// **Call [`poll`](Self::poll) every frame** while the session is open to
/// joiners: that answers the queries waiting and broadcasts when one is due.
/// Keep the announcement current with
/// [`set_announcement`](Self::set_announcement) as players come and go.
/// Dropping the announcer stops both; browsers age the host out after
/// [`super::HOST_EXPIRY`].
pub struct Announcer<C: Clock = SystemClock> {
    socket: UdpSocket,
    announcement: Announcement,
    /// The announcement encoded once, not per reply.
    encoded: [u8; ANNOUNCE_BYTES],
    broadcast_to: Option<SocketAddr>,
    clock: C,
    last_broadcast: Option<Duration>,
    stats: AnnouncerStats,
}

impl Announcer<SystemClock> {
    /// An announcer on [`DISCOVERY_PORT`] of every IPv4 interface,
    /// broadcasting `announcement` to that port at the IPv4 broadcast
    /// address.
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be bound — another host on this
    /// machine holding the port, among other reasons — made non-blocking, or
    /// allowed to broadcast.
    pub fn open(announcement: Announcement) -> io::Result<Self> {
        Self::bind_with(
            (Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT),
            announcement,
            Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::BROADCAST,
                DISCOVERY_PORT,
            ))),
            SystemClock::new(),
        )
    }
}

impl<C: Clock> Announcer<C> {
    /// An announcer bound to `addr`, answering queries with `announcement`,
    /// broadcasting it to `broadcast_to` every [`ANNOUNCE_INTERVAL`] (or
    /// never, for `None`), timed by `clock`.
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be bound, made non-blocking or
    /// allowed to broadcast.
    pub fn bind_with(
        addr: impl ToSocketAddrs,
        announcement: Announcement,
        broadcast_to: Option<SocketAddr>,
        clock: C,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            encoded: announcement.encode(),
            announcement,
            broadcast_to,
            clock,
            last_broadcast: None,
            stats: AnnouncerStats::default(),
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

    /// What this host announces.
    #[must_use]
    pub fn announcement(&self) -> &Announcement {
        &self.announcement
    }

    /// Announces `announcement` from now on: the next reply and broadcast
    /// carry it.
    pub fn set_announcement(&mut self, announcement: Announcement) {
        self.encoded = announcement.encode();
        self.announcement = announcement;
    }

    /// Answers up to [`RECEIVE_BUDGET`] waiting datagrams, then broadcasts
    /// if [`ANNOUNCE_INTERVAL`] has passed since the last (the first poll
    /// always does). Never blocks.
    pub fn poll(&mut self) {
        let mut buffer = [0u8; DISCOVERY_BUFFER_BYTES];
        for _ in 0..RECEIVE_BUDGET {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, from)) => self.answer(&buffer[..len], from),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // An error on this read says nothing about the datagrams
                // still queued, so the read carries on — as the listener's.
                Err(_) => self.stats.receive_errors += 1,
            }
        }
        let now = self.clock.now();
        if let Some(to) = self.broadcast_to
            && self
                .last_broadcast
                .is_none_or(|last| now.saturating_sub(last) >= ANNOUNCE_INTERVAL)
        {
            send(
                &self.socket,
                &self.encoded,
                to,
                &mut self.stats.send_failures,
            );
            self.stats.broadcasts += 1;
            self.last_broadcast = Some(now);
        }
    }

    /// What the announcer has sent and ignored.
    #[must_use]
    pub fn stats(&self) -> AnnouncerStats {
        self.stats
    }

    /// Answers `datagram` from `from` with the announce if it is a query for
    /// this host's protocol id, or counts why not.
    fn answer(&mut self, datagram: &[u8], from: SocketAddr) {
        if datagram.first() != Some(&QUERY_TAG) {
            self.stats.not_query += 1;
            return;
        }
        match decode_query(datagram) {
            Ok(protocol_id) if protocol_id == self.announcement.protocol_id => {
                send(
                    &self.socket,
                    &self.encoded,
                    from,
                    &mut self.stats.send_failures,
                );
                self.stats.queries_answered += 1;
            }
            Ok(_) => self.stats.other_protocol += 1,
            Err(Refusal::OtherVersion) => self.stats.other_version += 1,
            Err(Refusal::Malformed) => self.stats.malformed += 1,
        }
    }

    /// Whether the socket may broadcast, as the operating system reports it.
    #[cfg(test)]
    pub(super) fn may_broadcast(&self) -> io::Result<bool> {
        self.socket.broadcast()
    }
}

impl<C: Clock> std::fmt::Debug for Announcer<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Announcer")
            .field("local_addr", &self.socket.local_addr().ok())
            .field("announcement", &self.announcement)
            .field("broadcast_to", &self.broadcast_to)
            .field("stats", &self.stats)
            .finish()
    }
}
