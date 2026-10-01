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
//! - [`discovery`] finds hosts on the LAN: a host's announcer and a client's
//!   browser, a convenience over connecting by address.
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
//! in `hello`. The client sends a [`Hello`]: [`HELLO_TAG`], the
//! [`TRANSPORT_VERSION`], the protocol id, a fresh random nonce, its X25519
//! public key and a token field, empty at first. The server answers a hello
//! without a valid token with a [`Challenge`] carrying one (see [`token`]);
//! the client puts it in its hello and sends again at once. A hello whose
//! token verifies is answered with a [`Reply`] echoing the client's nonce and
//! carrying the server's public key. Each side then calls
//! [`crate::seal::agree_channel`] with its [`crate::seal::Role`], and from
//! there on **every datagram is sealed** — the encryption rule in
//! `docs/notes/simulation.md`, with no plaintext mode and no switch. The tag
//! bytes are distinct from [`crate::seal::SEALED_TAG`], so one socket carries
//! both and tells them apart by the first byte.
//!
//! ```text
//! client                                  listener
//!   Hello { nonce, key, NO_TOKEN }   ──►    mints a token: one HMAC, no state
//!                                    ◄──  Challenge { nonce, token }
//!   Hello { nonce, key, token }      ──►    verifies it, X25519, pending entry
//!                                    ◄──  Reply { nonce, key }
//!   sealed datagrams                 ◄─►  sealed datagrams
//! ```
//!
//! Each side draws its secret bytes from the operating system (`getrandom`)
//! for every connection and never reuses them: a reconnect is a new hello and
//! new keys, which is the rekey the seal's docs ask for.
//!
//! - **The echo** is what an off-path attacker cannot forge. A challenge or
//!   reply whose nonce is not the one this hello carried is ignored, so
//!   someone who did not see the hello can neither hand the client a key of
//!   their choosing nor a token.
//! - **No amplification.** The hello is the longest handshake datagram
//!   ([`HELLO_BYTES`]; a [`CHALLENGE_BYTES`] challenge and a [`REPLY_BYTES`]
//!   reply are shorter), so a spoofed hello cannot turn the server into a
//!   multiplier aimed at someone else's address.
//! - **Work only for a returning address.** A hello without a token valid for
//!   its source address costs the server one HMAC and a challenge, and holds
//!   nothing. Only a token presented back from the address it was minted for,
//!   within [`TOKEN_LIFETIME`] and on the run of the listener that minted it,
//!   buys the X25519 agreement and a pending entry — so a spoofer, who never
//!   sees the challenges, never gets that far. Each token buys it once: the
//!   listener remembers the tokens it has spent until they expire.
//! - **Bounded state.** A valid hello costs the server one pending entry,
//!   capped at [`ListenerConfig::max_pending`]; the entry becomes a peer only
//!   when a sealed datagram opens under its key — proof the client holds the
//!   private key — and expires after [`HANDSHAKE_TIMEOUT`] otherwise. Spent
//!   tokens are capped at [`MAX_SPENT_TOKENS`]. Neither table grows past its
//!   cap, however many hellos arrive.
//! - **Honest trust.** Nothing authenticates either public key. The exchange
//!   defeats a passive observer and is open to anyone who can sit in the
//!   middle of the hello and substitute keys both ways — the seal's docs say
//!   the same. A token minted by a trusted backend carrying key material, or
//!   an operator-configured pre-shared key, is what would close that; the
//!   listener's own tokens prove an address, not an identity.
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
//! * `hello` — the plaintext hello, challenge and reply.
//! * [`token`] — the connection tokens a challenge carries.
//! * `session` — a keyed link: sealer, opener and endpoint together.
//! * `transport` — [`UdpTransport`].
//! * `listener` — [`UdpListener`] and the demultiplexer behind it.
//! * [`discovery`] — LAN host discovery, on its own socket.

pub mod discovery;
mod hello;
mod listener;
mod session;
pub mod token;
mod transport;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub use hello::{
    CHALLENGE_BYTES, CHALLENGE_TAG, Challenge, HELLO_BYTES, HELLO_NONCE_BYTES, HELLO_REPLY_TAG,
    HELLO_TAG, Hello, REPLY_BYTES, Reply, TRANSPORT_VERSION,
};
pub use listener::{ListenerConfig, ListenerStats, MAX_PEER_INBOX, UdpListener};
pub use token::{NO_TOKEN, TOKEN_BYTES, TokenError, TokenKey, VerifiedToken};
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
/// round trip; this is what clears the entries of clients that went away
/// mid-handshake, or presented a token they had seen and hold no key for.
pub const HANDSHAKE_TIMEOUT: Duration = CONNECT_TIMEOUT;

/// How long a token a [`Challenge`] carries stays valid. A client presents
/// it the moment the challenge arrives, so it needs a round trip; it is as
/// long as a connect may take, so no token outlives the connect it was
/// minted for. One that expires anyway costs the client one more challenge.
pub const TOKEN_LIFETIME: Duration = CONNECT_TIMEOUT;

/// Spent tokens a [`UdpListener`] remembers at once. Each is kept only until
/// it expires — after [`TOKEN_LIFETIME`] its expiry refuses it anyway — and a
/// token is spent only when a hello presenting it takes a pending entry, so
/// filling this takes that many valid handshakes, each a round trip from a
/// real address, within one lifetime. When it is full, valid tokens are
/// turned away rather than accepted unremembered: a replayed token must never
/// buy a second agreement.
pub const MAX_SPENT_TOKENS: usize = 4096;

/// Peers a [`UdpListener`] admits at once unless its [`ListenerConfig`] says
/// otherwise. A LAN session's worth, with room.
pub const DEFAULT_MAX_PEERS: usize = 64;

/// Pending handshakes a [`UdpListener`] holds at once unless its
/// [`ListenerConfig`] says otherwise. Each is one entry of fixed size, so this
/// is the memory a flood of hellos can take — and each takes a token valid
/// for its source address, so a spoofer cannot fill it.
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
