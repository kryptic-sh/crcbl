//! Transport trait and in-memory implementation.
//!
//! This module defines the message-oriented, async-agnostic transport abstraction
//! and the SPSC in-memory channel pair for testing and single-player loopback.

// ── Message primitives ────────────────────────────────────────────────────────

/// A wire-level message with a reliability hint and a binary payload.
#[derive(Debug, Clone)]
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

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_formatting() {
        let _ = format!("{:?}", MessageKind::Reliable);
        let _ = format!("{:?}", TransportError::Disconnected);
        let _ = format!("{:?}", TransportError::Channel("oops".into()));

        let msg = Message {
            kind: MessageKind::Reliable,
            payload: vec![1, 2, 3],
        };
        let _ = format!("{msg:?}");

        let (a, _b) = InMemoryTransport::pair();
        let _ = format!("{a:?}");
    }
}
