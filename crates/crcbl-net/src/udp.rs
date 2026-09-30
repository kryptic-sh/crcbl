//! The engine's own network transport: [`crate::reliable`]'s packet layer
//! inside [`crate::seal`]'s AEAD, over a [`std::net::UdpSocket`].
//!
//! - [`UdpTransport`] is one link, and implements [`crate::Transport`]. A
//!   client makes one with [`UdpTransport::connect`]; a host gets one per
//!   peer from its listener.
//! - [`UdpListener`] binds one socket, answers hellos, and hands out a
//!   `UdpTransport` per admitted peer, every one sharing that socket.
//! - [`EndReason`] says why a link ended: a connect nobody answered, a peer
//!   that went silent, one that said goodbye, or this side closing it.
//!
//! ```text
//! Transport ── Endpoint ── Sealer / Opener ── UdpSocket
//! ```
//!
//! # Native only, by design
//!
//! The whole module is `#[cfg(not(target_arch = "wasm32"))]`. That is the
//! LOCKED rule in `docs/notes/simulation.md` (_Sessions are LAN, and web builds
//! have no networking_), not a missing arm: a browser cannot open a UDP socket
//! at all, and the design gives web builds no network transport rather than a
//! second-class one. The rest of the crate builds for `wasm32-unknown-unknown`
//! unchanged.
//!
//! # The hello
//!
//! Before any key exists the socket carries one plaintext exchange, laid out
//! in `hello`: the client sends [`HELLO_TAG`], the [`TRANSPORT_VERSION`], the
//! protocol id, a fresh random nonce and its X25519 public key; the server
//! answers [`HELLO_REPLY_TAG`] with the same fields, echoing the client's
//! nonce and carrying its own public key. Each side then calls
//! [`crate::seal::agree_channel`] with its [`crate::seal::Role`], and from
//! there on **every datagram is sealed** — the encryption rule in
//! `docs/notes/simulation.md`, with no plaintext mode and no switch. The tag
//! bytes are distinct from [`crate::seal::SEALED_TAG`], so one socket carries
//! both and tells them apart by the first byte.
//!
//! Each side draws its secret bytes from the operating system (`getrandom`)
//! for every connection and never reuses them: a reconnect is a new hello and
//! new keys, which is the rekey the seal's docs ask for.
//!
//! - **The echo** is what an off-path attacker cannot forge. A reply whose
//!   nonce is not the one this hello carried is ignored, so someone who did
//!   not see the hello cannot hand the client a key of their choosing.
//! - **No amplification.** A reply is exactly as long as the hello that asked
//!   for it ([`HELLO_BYTES`] both ways), so a spoofed hello cannot turn the
//!   server into a multiplier aimed at someone else's address.
//! - **Bounded state.** A hello costs the server one pending entry, capped at
//!   [`ListenerConfig::max_pending`]; the entry becomes a peer only when a
//!   sealed datagram opens under its key — proof the client both holds the
//!   private key and received the reply at the address it claims — and
//!   expires after [`HANDSHAKE_TIMEOUT`] otherwise. A flood of hellos from
//!   spoofed addresses fills the pending table and then goes unanswered; it
//!   never grows memory. It does still cost an X25519 agreement per answered
//!   hello: connection tokens (slice D in `docs/backlog.md`) are what move
//!   that cost onto the client.
//! - **Honest trust.** Nothing authenticates either public key. The exchange
//!   defeats a passive observer and is open to anyone who can sit in the
//!   middle of the hello and substitute keys both ways — the seal's docs say
//!   the same. A token minted by a trusted source, or an operator-configured
//!   pre-shared key, is what would close that; nothing here claims to.
//!
//! This is the transport's own key exchange. The session handshake in
//! [`crate::handshake`] — protocol, build and schema gate — rides on the
//! transport after it is up, sealed like everything else.
//!
//! # Hostile datagrams are dropped, never obeyed
//!
//! A datagram from any address but the peer's, a plaintext one after the
//! hello, or one that fails to open is dropped and counted in [`UdpStats`] or
//! [`ListenerStats`] — **never a disconnect**. Anyone can put bytes on a UDP
//! socket; if garbage could end a link, anyone could end every link. The only
//! things that end one are the peer's sealed disconnect, silence for
//! [`crate::reliable::PEER_TIMEOUT`], and this side closing it.
//!
//! # Driving it
//!
//! Nothing here runs a thread. Each [`crate::Transport`] call on a
//! `UdpTransport` reads what the socket holds, runs the timers — hello
//! resends, keepalives, reliable resends, timeouts — and sends what is due, so
//! a game that calls `recv` every frame keeps its links alive. A host calls
//! [`UdpListener::accept`] every frame too: that is what answers hellos while
//! no peer's transport is being read.
//!
//! # Submodules
//!
//! * `hello` — the plaintext hello and its reply.
//! * `session` — a keyed link: sealer, opener and endpoint together.
//! * `transport` — [`UdpTransport`].
//! * `listener` — [`UdpListener`] and the demultiplexer behind it.

mod hello;
mod listener;
mod session;
mod transport;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub use hello::{HELLO_BYTES, HELLO_NONCE_BYTES, HELLO_REPLY_TAG, HELLO_TAG, TRANSPORT_VERSION};
pub use listener::{ListenerConfig, ListenerStats, MAX_PEER_INBOX, UdpListener};
pub use transport::{ConnectError, EndReason, UdpState, UdpStats, UdpTransport};

use crate::reliable::{KEEPALIVE_INTERVAL, MAX_DATAGRAM_BYTES, PEER_TIMEOUT};

/// How often a client resends its hello while no reply has come: the same
/// rate as [`KEEPALIVE_INTERVAL`], ten a second, which is netcode.io's send
/// rate for its connection request packets too.
pub const HELLO_RESEND_INTERVAL: Duration = KEEPALIVE_INTERVAL;

/// How long a client waits for a hello reply before the connect fails with
/// [`EndReason::ConnectTimedOut`]. Dozens of resends on a LAN, so loss cannot
/// reach it; a host that is not there, or not listening, does.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a server holds a pending handshake — a hello answered, no sealed
/// datagram yet — before dropping it. The client's first sealed datagram
/// leaves the moment it reads the reply, so an honest one arrives within a
/// round trip; this is what clears the entries a spoofed hello leaves behind.
pub const HANDSHAKE_TIMEOUT: Duration = CONNECT_TIMEOUT;

/// Peers a [`UdpListener`] admits at once unless its [`ListenerConfig`] says
/// otherwise. A LAN session's worth, with room.
pub const DEFAULT_MAX_PEERS: usize = 64;

/// Pending handshakes a [`UdpListener`] holds at once unless its
/// [`ListenerConfig`] says otherwise. Each is one entry of fixed size, so this
/// is the memory a flood of spoofed hellos can take.
pub const DEFAULT_MAX_PENDING: usize = 256;

/// Datagrams one read of a socket takes before handing back to the caller,
/// so a flood cannot hold a frame hostage; the rest wait for the next read.
/// It also bounds the loop when the socket keeps reporting an error.
pub const RECEIVE_BUDGET: usize = 1024;

/// The receive buffer: one byte past the datagram budget, so a datagram that
/// was too long shows as too long rather than being silently truncated to
/// one that fits.
const RECEIVE_BUFFER_BYTES: usize = MAX_DATAGRAM_BYTES + 1;

// A connect that could outlast the peer timeout would let a half-open link
// look healthier than a dead one.
const _: () = assert!(CONNECT_TIMEOUT.as_millis() < PEER_TIMEOUT.as_millis());
// Many resends fit a connect, and many keepalives a pending handshake.
const _: () = assert!(CONNECT_TIMEOUT.as_millis() >= 10 * HELLO_RESEND_INTERVAL.as_millis());
const _: () = assert!(HANDSHAKE_TIMEOUT.as_millis() >= 10 * KEEPALIVE_INTERVAL.as_millis());
