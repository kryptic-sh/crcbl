//! Networking: transport seam and state replication.
//!
//! This crate defines the message-oriented, async-agnostic transport abstraction
//! and the snapshot-based replication protocol. It provides an in-memory
//! transport for testing and local loopback, plus the scaffolding for per-system
//! snapshot serialisation.
//!
//! # Design
//!
//! * [`Transport`] — the trait every network backend implements. Send/recv are
//!   non-blocking so the caller drives the loop.
//! * [`InMemoryTransport`] — an SPSC pair for integration tests and
//!   single-threaded local play.
//! * [`SnapshotWriter`] / [`SnapshotReader`] — encode and decode per-system state
//!   for the server → client snapshot path.
//! * [`budget`] — fits each snapshot to one message of its transport's
//!   unreliable channel, holding the least urgent updates back by priority
//!   rather than sending a snapshot the transport would refuse.
//! * [`command`] — what a `ClientToServer::Command` carries: a console set of
//!   a simulation variable, as text, and the server's sealed answer to it.
//! * [`edit`] — the other thing a command carries: a scene edit, its
//!   reason-coded answer, and the notice every client is sent of an edit
//!   applied.
//! * [`auth`] — the per-session MAC every post-handshake message carries.
//!   Nothing else in the protocol proves who sent a packet.
//! * [`reliable`] — the packet layer the UDP transport runs inside: acks,
//!   resend, ordering and fragmentation over any datagram pipe.
//! * [`seal`] — the AEAD every datagram on a network transport travels under,
//!   with derived nonces, a replay window, and the key schedule behind it.
//! * `udp` (native only) — the engine's own network transport: the packet
//!   layer inside the seal over a UDP socket, a client's connect and a host's
//!   listener, and `udp::discovery` for finding hosts on the LAN. Web builds
//!   have no networking, so it does not exist there.
//! * `conformance` (feature `conformance`) — the checks every [`Transport`]
//!   must pass, for crates that implement one.

pub mod auth;
pub mod budget;
pub mod codec;
pub mod command;
pub mod condition;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod delta;
pub mod edit;
pub mod handshake;
pub mod messages;
pub mod rate_limit;
pub mod reliable;
pub mod seal;
pub mod session;
pub mod transport;
pub mod types;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;

pub use auth::{AuthError, ReplayWindow, SessionCrypto, SessionKey};
pub use budget::{
    BudgetTooSmall, DEFAULT_RELEVANCE, Fitted, OversizedUpdate, PriorityAccumulator,
    snapshot_budget,
};
pub use codec::{
    Ack, DecodeError, decode_ack, decode_client_to_server, decode_handshake_result, decode_hello,
    decode_server_to_client, decode_session_ended, encode_ack, encode_client_to_server,
    encode_handshake_result, encode_hello, encode_server_to_client, encode_session_ended,
};
pub use command::{
    ConsoleOutcome, ConsoleReply, ConsoleSet, ConsoleTextTooLong, decode_console_reply,
    decode_console_set, encode_console_reply, encode_console_set,
};
pub use condition::{Clock, ConditionSimulator, ManualClock, SimConditions, SystemClock};
pub use delta::{
    Baseline, BaselineDecodeError, BaselineStore, Delta, DeltaCodec, DeltaDecodeError,
    MAX_AUTHENTICATED_SYSTEMS, MAX_BASELINE_ENCODED_BYTES, MAX_BASELINE_ENTITIES,
    MAX_BASELINE_SYSTEMS, SystemDelta, Trust, decode_delta, encode_delta, encode_entity_entry,
    hash_encoded,
};
pub use edit::{
    EditNotice, EditOutcome, EditRefusal, EditReply, EditRequest, EditTooLong,
    MAX_EDIT_MESSAGE_BYTES, MAX_EDIT_OP_BYTES, decode_edit_notice, decode_edit_reply,
    decode_edit_request, encode_edit_notice, encode_edit_reply, encode_edit_request,
};
pub use handshake::{HandshakeGate, HandshakeResult, Hello, RejectReason};
pub use messages::{
    ClientToServer, MAX_CLIENT_INPUTS_PER_TICK, ServerToClient, SessionEndReason, SnapshotReader,
    SnapshotWriter, SystemSnapshot, replicated_system_id,
};
pub use rate_limit::{InboundRateLimitConfig, InboundRateLimiter};
pub use session::{SessionConfig, SessionManager, SessionState};
pub use transport::{
    IN_MEMORY_CHANNEL_CAPACITY, InMemoryTransport, MAX_IN_MEMORY_MESSAGE_BYTES, Message,
    MessageKind, Transport, TransportError,
};
pub use types::{EntityBits, EntityData, ProtocolCompatibility, ResumeToken, SectorId, SessionId};
