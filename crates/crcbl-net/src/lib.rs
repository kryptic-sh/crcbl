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

pub mod codec;
pub mod condition;
pub mod delta;
pub mod handshake;
pub mod messages;
pub mod session;
pub mod transport;
pub mod types;

pub use codec::{
    DecodeError, decode_ack, decode_client_to_server, decode_handshake_result, decode_hello,
    decode_server_to_client, encode_ack, encode_client_to_server, encode_handshake_result,
    encode_hello, encode_server_to_client,
};
pub use condition::{ConditionSimulator, SimConditions};
pub use delta::{
    Baseline, BaselineStore, Delta, DeltaCodec, DeltaDecodeError, SystemDelta, decode_delta,
    encode_delta, hash_encoded,
};
pub use handshake::{HandshakeGate, HandshakeResult, Hello, RejectReason};
pub use messages::{
    ClientToServer, ServerToClient, SnapshotReader, SnapshotWriter, SystemSnapshot,
};
pub use session::{SessionConfig, SessionManager, SessionState};
pub use transport::{InMemoryTransport, Message, MessageKind, Transport, TransportError};
pub use types::{EntityBits, EntityData, SectorId, SessionId};
