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

use crcbl_core::TickId;

// ── Messages ──────────────────────────────────────────────────────────────────

/// A wire-level message with a reliability hint and a binary payload.
#[derive(Debug)]
pub struct Message {
    /// Delivery semantics.
    pub kind: MessageKind,
    /// Opaque payload bytes.
    pub payload: Vec<u8>,
}

/// Delivery guarantee requested for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// Must be delivered in order. Loss is not acceptable.
    Reliable,
    /// May be dropped. Used for input ticks and snapshots where the newest
    /// value supersedes the old.
    Unreliable,
}

// ── Typed protocol messages ───────────────────────────────────────────────────

/// Messages sent from the client to the server.
#[derive(Debug)]
pub enum ClientToServer {
    /// An input tick — the client's input state at a given server tick.
    Input {
        /// The server tick this input is for.
        tick: TickId,
        /// Serialised input data.
        data: Vec<u8>,
    },
    /// A command — chat, ready-up, etc.
    Command {
        /// Serialised command payload.
        data: Vec<u8>,
    },
}

/// Messages sent from the server to the client.
#[derive(Debug)]
pub enum ServerToClient {
    /// A world snapshot for a given tick.
    Snapshot {
        /// The server tick this snapshot represents.
        tick: TickId,
        /// Per-system snapshot data.
        systems: Vec<SystemSnapshot>,
    },
    /// An event — chat message, server notification, etc.
    Event {
        /// Serialised event payload.
        data: Vec<u8>,
    },
}

/// A single system's snapshot data.
#[derive(Debug)]
pub struct SystemSnapshot {
    /// Unique identifier for the system that produced this data.
    pub system_id: u32,
    /// Serialised snapshot payload.
    pub data: Vec<u8>,
}

// ── Transport trait ───────────────────────────────────────────────────────────

/// Errors the transport can produce.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The transport is no longer connected and cannot recover.
    #[error("transport disconnected")]
    Disconnected,
    /// An underlying channel or I/O error.
    #[error("channel error: {0}")]
    Channel(String),
}

/// A message-oriented, async-agnostic network transport.
///
/// Implementations must be `Send` so the event loop can hand them between
/// threads. All methods are non-blocking — the caller is responsible for
/// driving the loop.
pub trait Transport: Send {
    /// Queue a message for reliable, ordered delivery.
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError>;

    /// Queue a message for best-effort delivery. May be dropped or reordered.
    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError>;

    /// Receive the next available message, or `Ok(None)` if nothing is queued.
    fn recv(&mut self) -> Result<Option<Message>, TransportError>;

    /// Whether the transport is still connected.
    fn is_connected(&self) -> bool;
}

// ── In-memory transport ───────────────────────────────────────────────────────

/// An SPSC in-memory transport pair for testing and local loopback.
///
/// Each [`InMemoryTransport`] owns one end of two independent channels:
/// a reliable channel and an unreliable channel. [`recv`](InMemoryTransport::recv)
/// polls the unreliable channel first so stale state is discarded before
/// reliable commands are processed.
///
/// Construct a pair with [`InMemoryTransport::pair`].
pub struct InMemoryTransport {
    reliable_tx: std::sync::mpsc::Sender<Message>,
    unreliable_tx: std::sync::mpsc::Sender<Message>,
    reliable_rx: std::sync::mpsc::Receiver<Message>,
    unreliable_rx: std::sync::mpsc::Receiver<Message>,
    connected: bool,
}

impl InMemoryTransport {
    /// Create a pair of transports connected back-to-back.
    ///
    /// Messages sent on one end are received on the other.
    pub fn pair() -> (Self, Self) {
        let (rtx_a, rrx_a) = std::sync::mpsc::channel();
        let (utx_a, urx_a) = std::sync::mpsc::channel();
        let (rtx_b, rrx_b) = std::sync::mpsc::channel();
        let (utx_b, urx_b) = std::sync::mpsc::channel();

        let a = Self {
            reliable_tx: rtx_a,
            unreliable_tx: utx_a,
            reliable_rx: rrx_b,
            unreliable_rx: urx_b,
            connected: true,
        };

        let b = Self {
            reliable_tx: rtx_b,
            unreliable_tx: utx_b,
            reliable_rx: rrx_a,
            unreliable_rx: urx_a,
            connected: true,
        };

        (a, b)
    }

    /// Disconnect this transport.
    ///
    /// Future send/recv calls will return [`TransportError::Disconnected`].
    pub fn disconnect(&mut self) {
        self.connected = false;
    }
}

impl Transport for InMemoryTransport {
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        self.reliable_tx
            .send(msg)
            .map_err(|e| TransportError::Channel(e.to_string()))
    }

    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        self.unreliable_tx
            .send(msg)
            .map_err(|e| TransportError::Channel(e.to_string()))
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }

        // Poll unreliable first — newest state supersedes old.
        match self.unreliable_rx.try_recv() {
            Ok(msg) => return Ok(Some(msg)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Treat a disconnected unreliable channel as "no message now"
                // rather than killing the transport — the reliable channel may
                // still be alive.
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        match self.reliable_rx.try_recv() {
            Ok(msg) => return Ok(Some(msg)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // The peer dropped its sender — we are disconnected.
                self.connected = false;
                return Err(TransportError::Disconnected);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        Ok(None)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

impl std::fmt::Debug for InMemoryTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryTransport")
            .field("connected", &self.connected)
            .finish_non_exhaustive()
    }
}

// ── Snapshot helpers ──────────────────────────────────────────────────────────

/// Builds a [`ServerToClient::Snapshot`] incrementally, system by system.
pub struct SnapshotWriter {
    systems: Vec<SystemSnapshot>,
    tick: TickId,
}

impl SnapshotWriter {
    /// Create a writer for the given server tick.
    pub fn new(tick: TickId) -> Self {
        Self {
            systems: Vec::new(),
            tick,
        }
    }

    /// Append a system's snapshot data.
    pub fn write_system(&mut self, system_id: u32, data: Vec<u8>) {
        self.systems.push(SystemSnapshot { system_id, data });
    }

    /// Consume the writer and produce the finished [`ServerToClient`] message.
    pub fn finish(self) -> ServerToClient {
        ServerToClient::Snapshot {
            tick: self.tick,
            systems: self.systems,
        }
    }
}

impl std::fmt::Debug for SnapshotWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotWriter")
            .field("tick", &self.tick)
            .field("system_count", &self.systems.len())
            .finish()
    }
}

/// Reads per-system data out of a [`ServerToClient::Snapshot`].
pub struct SnapshotReader<'a> {
    tick: TickId,
    systems: &'a [SystemSnapshot],
}

impl<'a> SnapshotReader<'a> {
    /// Extract a reader from a server-to-client message.
    ///
    /// Returns `None` if the message is not a snapshot.
    pub fn from_snapshot(msg: &'a ServerToClient) -> Option<Self> {
        match msg {
            ServerToClient::Snapshot { tick, systems } => Some(Self {
                tick: *tick,
                systems,
            }),
            ServerToClient::Event { .. } => None,
        }
    }

    /// The server tick this snapshot represents.
    pub fn tick(&self) -> TickId {
        self.tick
    }

    /// Look up a system's data by its id.
    pub fn system_data(&self, system_id: u32) -> Option<&[u8]> {
        self.systems
            .iter()
            .find(|s| s.system_id == system_id)
            .map(|s| s.data.as_slice())
    }

    /// Iterate over every system snapshot, yielding `(system_id, data)`.
    pub fn iter_systems(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.systems
            .iter()
            .map(|s| (s.system_id, s.data.as_slice()))
    }
}

impl<'a> std::fmt::Debug for SnapshotReader<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotReader")
            .field("tick", &self.tick)
            .field("system_count", &self.systems.len())
            .finish()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── InMemoryTransport roundtrip ────────────────────────────────────────

    #[test]
    fn reliable_roundtrip() {
        let (mut a, mut b) = InMemoryTransport::pair();
        assert!(a.is_connected());
        assert!(b.is_connected());

        let msg = Message {
            kind: MessageKind::Reliable,
            payload: b"hello".to_vec(),
        };

        a.send_reliable(msg).unwrap();

        let received = b.recv().unwrap().unwrap();
        assert_eq!(received.kind, MessageKind::Reliable);
        assert_eq!(received.payload, b"hello");
    }

    #[test]
    fn unreliable_roundtrip() {
        let (mut a, mut b) = InMemoryTransport::pair();

        let msg = Message {
            kind: MessageKind::Unreliable,
            payload: b"snapshot".to_vec(),
        };

        a.send_unreliable(msg).unwrap();

        let received = b.recv().unwrap().unwrap();
        assert_eq!(received.kind, MessageKind::Unreliable);
        assert_eq!(received.payload, b"snapshot");
    }

    #[test]
    fn unreliable_supersedes_reliable_in_recv() {
        // When both channels have data, recv() returns the unreliable message
        // first so the caller processes the newest state before commands.
        let (mut a, mut b) = InMemoryTransport::pair();

        a.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: b"cmd".to_vec(),
        })
        .unwrap();

        a.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: b"snap".to_vec(),
        })
        .unwrap();

        let first = b.recv().unwrap().unwrap();
        assert_eq!(first.kind, MessageKind::Unreliable);
        assert_eq!(first.payload, b"snap");

        let second = b.recv().unwrap().unwrap();
        assert_eq!(second.kind, MessageKind::Reliable);
        assert_eq!(second.payload, b"cmd");
    }

    #[test]
    fn recv_returns_none_when_empty() {
        let (a, mut b) = InMemoryTransport::pair();
        assert!(b.recv().unwrap().is_none());
        let _ = a; // keep sender alive
    }

    #[test]
    fn disconnect_detected_on_recv() {
        let (a, mut b) = InMemoryTransport::pair();

        // Drop a — its senders go away.
        drop(a);

        // The reliable channel's sender is now gone; recv should detect it.
        let result = b.recv();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransportError::Disconnected));
        assert!(!b.is_connected());
    }

    #[test]
    fn send_after_disconnect() {
        let (mut a, _b) = InMemoryTransport::pair();
        a.disconnect();

        let result = a.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: b"x".to_vec(),
        });
        assert!(matches!(result, Err(TransportError::Disconnected)));
    }

    #[test]
    fn recv_after_disconnect() {
        let (mut a, _b) = InMemoryTransport::pair();
        a.disconnect();

        let result = a.recv();
        assert!(matches!(result, Err(TransportError::Disconnected)));
    }

    #[test]
    fn is_connected_after_disconnect() {
        let (mut a, _b) = InMemoryTransport::pair();
        assert!(a.is_connected());
        a.disconnect();
        assert!(!a.is_connected());
    }

    #[test]
    fn bidirectional_traffic() {
        let (mut a, mut b) = InMemoryTransport::pair();

        a.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: b"a->b".to_vec(),
        })
        .unwrap();
        b.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: b"b->a".to_vec(),
        })
        .unwrap();

        let a_recv = a.recv().unwrap().unwrap();
        let b_recv = b.recv().unwrap().unwrap();

        assert_eq!(a_recv.payload, b"b->a");
        assert_eq!(b_recv.payload, b"a->b");
    }

    #[test]
    fn multiple_messages() {
        let (mut a, mut b) = InMemoryTransport::pair();

        for i in 0..5 {
            a.send_reliable(Message {
                kind: MessageKind::Reliable,
                payload: vec![i],
            })
            .unwrap();
        }

        for i in 0..5 {
            let msg = b.recv().unwrap().unwrap();
            assert_eq!(msg.payload, vec![i]);
        }

        assert!(b.recv().unwrap().is_none());
    }

    // ── SnapshotWriter / SnapshotReader roundtrip ─────────────────────────

    #[test]
    fn snapshot_writer_reader_roundtrip() {
        let tick = TickId::from_raw(42);
        let mut writer = SnapshotWriter::new(tick);

        writer.write_system(1, b"physics".to_vec());
        writer.write_system(2, b"render".to_vec());
        writer.write_system(3, b"audio".to_vec());

        let msg = writer.finish();
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();

        assert_eq!(reader.tick(), tick);
        assert_eq!(reader.system_data(1), Some(b"physics".as_slice()));
        assert_eq!(reader.system_data(2), Some(b"render".as_slice()));
        assert_eq!(reader.system_data(3), Some(b"audio".as_slice()));
        assert_eq!(reader.system_data(99), None);
    }

    #[test]
    fn snapshot_reader_rejects_event() {
        let event = ServerToClient::Event {
            data: b"chat".to_vec(),
        };
        assert!(SnapshotReader::from_snapshot(&event).is_none());
    }

    #[test]
    fn snapshot_writer_empty() {
        let writer = SnapshotWriter::new(TickId::from_raw(1));
        let msg = writer.finish();

        let reader = SnapshotReader::from_snapshot(&msg).unwrap();
        assert_eq!(reader.tick(), TickId::from_raw(1));
        assert_eq!(reader.iter_systems().count(), 0);
    }

    #[test]
    fn snapshot_reader_iter_systems() {
        let mut writer = SnapshotWriter::new(TickId::from_raw(7));
        writer.write_system(10, vec![0]);
        writer.write_system(20, vec![1, 2]);

        let msg = writer.finish();
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();

        let systems: Vec<_> = reader.iter_systems().collect();
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0], (10, &[0][..]));
        assert_eq!(systems[1], (20, &[1, 2][..]));
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_formatting() {
        // Ensure all public types format without panicking.
        let msg = Message {
            kind: MessageKind::Reliable,
            payload: vec![1, 2, 3],
        };
        let _ = format!("{msg:?}");

        let _ = format!("{:?}", MessageKind::Reliable);
        let _ = format!(
            "{:?}",
            ClientToServer::Input {
                tick: TickId::from_raw(1),
                data: vec![4],
            }
        );
        let _ = format!("{:?}", ClientToServer::Command { data: vec![5] });
        let _ = format!(
            "{:?}",
            ServerToClient::Snapshot {
                tick: TickId::from_raw(2),
                systems: vec![SystemSnapshot {
                    system_id: 1,
                    data: vec![6],
                }],
            }
        );
        let _ = format!("{:?}", ServerToClient::Event { data: vec![7] });
        let _ = format!(
            "{:?}",
            SystemSnapshot {
                system_id: 42,
                data: vec![8],
            }
        );
        let _ = format!("{:?}", TransportError::Disconnected);
        let _ = format!("{:?}", TransportError::Channel("oops".into()));

        let (a, _b) = InMemoryTransport::pair();
        let _ = format!("{a:?}");

        let writer = SnapshotWriter::new(TickId::from_raw(3));
        let _ = format!("{writer:?}");

        let msg = ServerToClient::Snapshot {
            tick: TickId::from_raw(3),
            systems: vec![],
        };
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();
        let _ = format!("{reader:?}");
    }
}
