//! LAN host discovery: hosts announce themselves on the local network, and
//! clients list what they hear.
//!
//! - [`Announcer`] is the host's side. It answers every [`Browser`]'s query
//!   with an [`Announcement`], unicast, and broadcasts the same announce every
//!   [`ANNOUNCE_INTERVAL`].
//! - [`Browser`] is the client's side. It queries every [`QUERY_INTERVAL`] —
//!   the IPv4 broadcast address on [`DISCOVERY_PORT`] unless told otherwise —
//!   and keeps a [`HostEntry`] per host it hears from, until the host has
//!   been silent for [`HOST_EXPIRY`].
//!
//! Both are driven like the rest of [`super`]: nothing runs a thread, and
//! each `poll` reads what the socket holds, answers or records it, and sends
//! what is due.
//!
//! # A hint, never trusted
//!
//! Discovery is a convenience over connecting by address, which stays
//! first-class (`docs/notes/simulation.md`, _Sessions are LAN, and web builds
//! have no networking_). **An announcement is a hint.** Nothing signs it and
//! anyone on the LAN can send one; a browser shows what it heard and nothing
//! more. Connecting to a listed host runs the transport's hello, the session
//! handshake's protocol, build and schema-hash gate, and the seal, all
//! unchanged — so a forged announce can put a false row in a list and no
//! further. What keeps even that row from being aimed at someone else:
//!
//! - **The address to connect to is the announce's source IP**, with the port
//!   it names ([`HostEntry::addr`]). No address travels in the datagram, so a
//!   forger can point clients only at its own machine — unless it spoofs its
//!   source address, which on a LAN nothing here can stop, and which then
//!   ends at a hello nobody answers.
//! - **Everything shown is bounded and checked**: a fixed-size datagram, a
//!   name capped at [`MAX_NAME_BYTES`] of valid UTF-8 without control
//!   characters, at most [`BrowserConfig::max_hosts`] entries. Anything else
//!   is dropped and counted in [`BrowserStats`], never a panic.
//! - **Players and compatibility are the host's claims.** The browser can
//!   grey out a host whose [`HostEntry::compatibility`] differs from its own
//!   before anyone tries; the handshake is still what refuses it.
//!
//! # No amplification
//!
//! A query from a spoofed source address would make a host send its reply
//! to the victim that address names. The rule QUIC applies before an address
//! is validated (RFC 9000 §8.1: at most three times the bytes received) is
//! taken here at a factor of one: **a query is padded to [`QUERY_BYTES`], at
//! least [`ANNOUNCE_BYTES`]**, and a query of any other length is dropped
//! unanswered. A spoofer gets out exactly what it puts in, no gain — the same
//! reason [`super::HELLO_BYTES`] is one length both ways.
//!
//! # Why the browser does not listen on the discovery port
//!
//! Only a socket bound to [`DISCOVERY_PORT`] hears a broadcast sent to it,
//! and a second process binding that port fails unless the sockets opt into
//! address reuse — `SO_REUSEADDR` or `SO_REUSEPORT`, with rules that differ
//! by platform — which [`std::net::UdpSocket`] has no setter for, and no
//! dependency is taken for it. So a [`Browser`] binds an
//! ephemeral port by default and **asks**: its query goes to the broadcast
//! address on the discovery port, where each host's [`Announcer`] is bound,
//! and the replies come back to it directly. Any number of browsers on one
//! machine work that way. The announcer's own periodic broadcast is heard by
//! a browser bound to [`DISCOVERY_PORT`] itself — possible with
//! [`Browser::bind_with`] when nothing else on the machine holds the port —
//! and it is what keeps such a list fresh without asking; a browser on an
//! ephemeral port relies on its queries alone. The price of the default is
//! one host per machine on the discovery port: a second announcer's bind
//! fails, loudly. It can bind another port instead, and then only a browser
//! that queries that port finds it.
//!
//! # Where broadcast goes
//!
//! [`Ipv4Addr::BROADCAST`] is the limited broadcast address: the operating
//! system sends it out one interface, which on Linux is the one its routes
//! pick. A machine on several networks is found on that one. Reaching each
//! would take a subnet-directed broadcast per interface, which needs the
//! interface list `std` does not give; link-local multicast has the same
//! bind problem as above. Neither is built.
//!
//! # Native only
//!
//! Part of [`super`], and native only with it: a browser cannot open a UDP
//! socket, and web builds have no networking by the LOCKED rule in
//! `docs/notes/simulation.md`.
//!
//! [`Ipv4Addr::BROADCAST`]: std::net::Ipv4Addr::BROADCAST

mod announcer;
mod browser;
mod wire;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub use announcer::{Announcer, AnnouncerStats};
pub use browser::{Browser, BrowserConfig, BrowserStats, HostEntry};
pub use wire::{
    ANNOUNCE_BYTES, ANNOUNCE_TAG, Announcement, DISCOVERY_VERSION, MAX_NAME_BYTES, QUERY_BYTES,
    QUERY_TAG,
};

/// The port hosts' [`Announcer`]s bind and browsers query.
///
/// In the dynamic and private range (49152–65535, RFC 6335 §6), which IANA
/// never assigns, so no registered service expects it; and above Linux's
/// default ephemeral range (`net.ipv4.ip_local_port_range`, 32768–60999), so
/// a Linux machine never hands it to some other program's port-0 socket.
/// Windows and macOS draw ephemeral ports from the whole dynamic range and
/// might; a bind then fails, and says so.
pub const DISCOVERY_PORT: u16 = 61_917;

/// How often an [`Announcer`] broadcasts its announce: fresh enough for a
/// lobby list, and one [`ANNOUNCE_BYTES`] datagram per interval is nothing
/// on a LAN.
pub const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(1);

/// How often a [`Browser`] sends its query while it is polled: the announce
/// rate, so a browser on an ephemeral port sees a list as fresh as one
/// hearing the broadcasts.
pub const QUERY_INTERVAL: Duration = ANNOUNCE_INTERVAL;

/// How long a [`Browser`] keeps a host it has not heard from. Several
/// announce and query intervals' worth (asserted below), so a lost datagram
/// or two never blinks a row out, and a host that stopped leaves the list
/// soon after.
pub const HOST_EXPIRY: Duration = Duration::from_secs(5);

/// Hosts a [`Browser`] lists at once unless its [`BrowserConfig`] says
/// otherwise. Far past any LAN, and the bound on what a flood of forged
/// announces from spoofed addresses can take.
pub const DEFAULT_MAX_HOSTS: usize = 256;

/// The receive buffer: one byte past the longest discovery datagram, so one
/// that was too long shows as too long rather than being cut to one that
/// fits.
const DISCOVERY_BUFFER_BYTES: usize = if ANNOUNCE_BYTES > QUERY_BYTES {
    ANNOUNCE_BYTES
} else {
    QUERY_BYTES
} + 1;

// A single lost announce or query must never expire a host.
const _: () = assert!(HOST_EXPIRY.as_millis() >= 3 * ANNOUNCE_INTERVAL.as_millis());
const _: () = assert!(HOST_EXPIRY.as_millis() >= 3 * QUERY_INTERVAL.as_millis());
const _: () = assert!(DISCOVERY_PORT >= 49_152 && DISCOVERY_PORT > 60_999);
