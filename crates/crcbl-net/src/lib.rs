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

pub mod condition;
pub mod messages;
pub mod transport;
pub mod types;

pub use condition::{ConditionSimulator, SimConditions};
pub use messages::{
    ClientToServer, ServerToClient, SnapshotReader, SnapshotWriter, SystemSnapshot,
};
pub use transport::{InMemoryTransport, Message, MessageKind, Transport, TransportError};
pub use types::{EntityBits, EntityData, SectorId, SessionId};
