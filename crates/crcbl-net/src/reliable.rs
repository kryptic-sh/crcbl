//! The reliability layer a UDP transport runs inside: sequenced packets,
//! piggybacked acks, resend, ordering and fragmentation, over any datagram
//! pipe.
//!
//! This is the packet layer of the design in `docs/backlog.md` under _The UDP
//! transport and its crypto_ — the Gaffer On Games and netcode.io lineage. It
//! is pure logic: an [`Endpoint`] takes datagram
//! bytes in through [`Endpoint::receive_datagram`] and hands datagram bytes
//! out through [`Endpoint::poll_outgoing`], and never touches a socket. What
//! carries those bytes is the caller's business, which is what lets the tests
//! run it over a seeded lossy pipe with a hand-driven clock.
//!
//! # Where this sits in the stack
//!
//! ```text
//! game ── Transport ── Endpoint ── seal/open (AEAD) ── UdpSocket
//!                     (this module)
//! ```
//!
//! **Nothing here goes on a network as it is.** The encryption rule in
//! `docs/notes/simulation.md` puts every packet on a network transport under
//! AEAD from the first packet after the hello, with no plaintext mode, so
//! [`crate::seal`]'s `Sealer` belongs between [`Endpoint::poll_outgoing`] and
//! the socket and its `Opener` between the socket and
//! [`Endpoint::receive_datagram`]. The endpoint's whole packet, header
//! included, is the AEAD plaintext, so an ack, a sequence or a channel id can
//! be neither read nor forged by anyone without the key. [`MAX_DATAGRAM_BYTES`]
//! already reserves [`SEAL_RESERVE`] for what the seal adds, so sealing a
//! packet the endpoint emitted never pushes it past the datagram budget.
//!
//! **The nonce cannot be this header's sequence.** It is 16 bits and wraps —
//! at a steady sixty packets a second, in about eighteen minutes — and an AEAD
//! nonce repeated under one key is fatal. The seal carries its own 64-bit
//! counter for the nonce, as netcode.io's packet prefix does beneath
//! reliable.io's 16-bit sequence.
//!
//! # Channels
//!
//! Two of the four in the channel table, and the two the [`crate::Transport`]
//! seam has:
//!
//! * [`Channel::Reliable`] — reliable-ordered, the backend for
//!   [`crate::Transport::send_reliable`] and
//!   [`crate::Transport::recv_reliable`]. Resent on an RTT-derived timeout,
//!   delivered once each and in order, and fragmented when larger than one
//!   datagram, up to [`MAX_RELIABLE_MESSAGE_BYTES`].
//! * [`Channel::UnreliableSequenced`] — the backend for
//!   [`crate::Transport::send_unreliable`]. Never resent, never fragmented: a
//!   payload past [`MAX_UNRELIABLE_PAYLOAD`] is refused rather than split,
//!   which is the one-datagram rule. Latest wins — anything older than what
//!   the channel last delivered is dropped.
//!
//! **The delta tick check stays.** Sequencing here stops a stale snapshot
//! beating a fresh one on this transport; `crcbl_net::delta` refusing a delta
//! whose tick is not newer than its baseline stops it on every transport,
//! including the ones that do not sequence. They catch different things.
//!
//! # Submodules
//!
//! * [`sequence`] — wraparound-correct 16-bit sequence comparison.
//! * [`packet`] — the wire format, and a decoder that is total on hostile
//!   bytes.
//! * `ack` — the received-packet window a packet's ack and bitfield are read
//!   from.
//! * [`rtt`] — the RFC 6298 round-trip estimator the resend timeout comes from.
//! * `outgoing` — reliable messages held and resent until acknowledged.
//! * `fragment` — where a message is cut, and bounded reassembly on receive.
//! * [`window`] — the link's counts over the last second, for rates.
//! * [`endpoint`] — the per-peer state machine that ties them together.

mod ack;
pub mod endpoint;
mod fragment;
mod outgoing;
pub mod packet;
pub mod rtt;
pub mod sequence;
pub mod window;

#[cfg(test)]
mod stats_tests;
#[cfg(test)]
pub(crate) mod tests;

pub use endpoint::{
    Channel, DISCONNECT_REDUNDANCY, Delivery, Endpoint, EndpointState, EndpointStats,
    KEEPALIVE_INTERVAL, MAX_DELIVERED_BYTES, MAX_DELIVERED_MESSAGES, MAX_QUEUED_UNRELIABLE,
    MAX_RELIABLE_BYTES_IN_FLIGHT, PEER_TIMEOUT, RELIABLE_WINDOW, ReceiveError,
};
pub use packet::{
    Fragment, HEADER_BYTES, MAX_DATAGRAM_BYTES, MAX_FRAGMENT_BYTES, MAX_FRAGMENTS_PER_MESSAGE,
    MAX_PACKET_BYTES, MAX_RELIABLE_MESSAGE_BYTES, MAX_UNRELIABLE_PAYLOAD, PacketAcks, PacketBody,
    PacketDecodeError, PacketHeader, SEAL_RESERVE, decode_packet, encode_packet,
};
pub use rtt::RttEstimator;
pub use sequence::{sequence_greater_than, sequence_less_than};
pub use window::{STATS_BUCKET, STATS_BUCKETS, STATS_WINDOW, WindowCounts};
