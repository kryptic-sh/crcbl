//! Transport trait and in-memory implementation.
//!
//! This module defines the message-oriented, async-agnostic transport abstraction
//! and the SPSC in-memory channel pair for testing and single-player loopback.

// ── Message primitives ────────────────────────────────────────────────────────

/// A wire-level message with a reliability hint and a binary payload.
///
/// `kind` is **not** an instruction: which channel a message travels on is
/// decided entirely by whether the sender called [`Transport::send_reliable`]
/// or [`Transport::send_unreliable`], and every implementation overwrites
/// `kind` to match on the way in. It is therefore a truthful label on a
/// received message and an ignored field on a sent one — a mismatch cannot
/// silently reroute anything.
#[derive(Debug, Clone)]
pub struct Message {
    /// Delivery semantics. Set by the sending call; see the type docs.
    pub kind: MessageKind,
    /// Opaque payload bytes.
    pub payload: Vec<u8>,
}

impl Message {
    /// A message for the reliable channel.
    #[must_use]
    pub fn reliable(payload: Vec<u8>) -> Self {
        Self {
            kind: MessageKind::Reliable,
            payload,
        }
    }

    /// A message for the unreliable channel.
    #[must_use]
    pub fn unreliable(payload: Vec<u8>) -> Self {
        Self {
            kind: MessageKind::Unreliable,
            payload,
        }
    }
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

/// Maximum payload retained by an in-memory transport queue.
pub const MAX_IN_MEMORY_MESSAGE_BYTES: usize = 64 * 1024;
/// Maximum queued messages per in-memory reliability channel.
pub const IN_MEMORY_CHANNEL_CAPACITY: usize = 256;

/// Errors the transport can produce.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The transport is no longer connected and cannot recover.
    #[error("transport disconnected")]
    Disconnected,
    /// An underlying channel or I/O error.
    #[error("channel error: {0}")]
    Channel(String),
    /// The message exceeds this transport's payload limit.
    #[error("message payload exceeds transport limit: {size} > {limit}")]
    MessageTooLarge { size: usize, limit: usize },
    /// The bounded channel cannot accept another message now.
    #[error("transport channel is full")]
    Backpressure,
}

/// A message-oriented, async-agnostic network transport.
///
/// Implementations must be `Send` so the event loop can hand them between
/// threads. All methods are non-blocking — the caller is responsible for
/// driving the loop.
pub trait Transport: Send {
    /// Queue a message for reliable, ordered delivery.
    ///
    /// Implementations must set [`Message::kind`] to
    /// [`MessageKind::Reliable`] so the field cannot disagree with the channel.
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError>;

    /// Queue a message for best-effort delivery. May be dropped or reordered.
    ///
    /// Implementations must set [`Message::kind`] to
    /// [`MessageKind::Unreliable`].
    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError>;

    /// Receive the next available reliable control message without polling
    /// unreliable traffic.
    ///
    /// The default defers to [`Transport::recv`], which is correct for a
    /// backend with a single delivery queue and merely unprioritised for one
    /// that forgot to override it. It deliberately does not return `Ok(None)`:
    /// that made a missing override look like an idle link, and a backend
    /// author who forgot would lose every handshake with no error anywhere.
    /// Backends with separate channels must override this so control traffic
    /// cannot be starved by lossy state.
    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.recv()
    }

    /// Receive the next available message, or `Ok(None)` if nothing is queued.
    ///
    /// Reliable traffic is drained first, so a caller that only ever calls
    /// `recv` still cannot have its control messages starved by a flood of
    /// unreliable state.
    fn recv(&mut self) -> Result<Option<Message>, TransportError>;

    /// Whether the transport is still connected.
    fn is_connected(&self) -> bool;
}

// ── In-memory transport ───────────────────────────────────────────────────────

/// An SPSC in-memory transport pair for testing and local loopback.
///
/// Each [`InMemoryTransport`] owns one end of two independent channels:
/// a reliable channel and an unreliable channel. The separate
/// [`Transport::recv_reliable`] path lets servers prioritize control traffic so
/// lossy state cannot starve handshakes. Both channels are bounded and reject
/// oversized payloads before queueing.
///
/// Construct a pair with [`InMemoryTransport::pair`].
pub struct InMemoryTransport {
    reliable_tx: std::sync::mpsc::SyncSender<Message>,
    unreliable_tx: std::sync::mpsc::SyncSender<Message>,
    reliable_rx: std::sync::mpsc::Receiver<Message>,
    unreliable_rx: std::sync::mpsc::Receiver<Message>,
    connected: bool,
}

impl InMemoryTransport {
    /// Create a pair of transports connected back-to-back.
    ///
    /// Messages sent on one end are received on the other.
    pub fn pair() -> (Self, Self) {
        let (rtx_a, rrx_a) = std::sync::mpsc::sync_channel(IN_MEMORY_CHANNEL_CAPACITY);
        let (utx_a, urx_a) = std::sync::mpsc::sync_channel(IN_MEMORY_CHANNEL_CAPACITY);
        let (rtx_b, rrx_b) = std::sync::mpsc::sync_channel(IN_MEMORY_CHANNEL_CAPACITY);
        let (utx_b, urx_b) = std::sync::mpsc::sync_channel(IN_MEMORY_CHANNEL_CAPACITY);

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

fn validate_message_size(msg: &Message) -> Result<(), TransportError> {
    if msg.payload.len() > MAX_IN_MEMORY_MESSAGE_BYTES {
        return Err(TransportError::MessageTooLarge {
            size: msg.payload.len(),
            limit: MAX_IN_MEMORY_MESSAGE_BYTES,
        });
    }
    Ok(())
}

fn map_try_send_error(error: std::sync::mpsc::TrySendError<Message>) -> TransportError {
    match error {
        std::sync::mpsc::TrySendError::Full(_) => TransportError::Backpressure,
        std::sync::mpsc::TrySendError::Disconnected(_) => TransportError::Disconnected,
    }
}

impl Transport for InMemoryTransport {
    fn send_reliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        validate_message_size(&msg)?;
        msg.kind = MessageKind::Reliable;
        self.reliable_tx.try_send(msg).map_err(map_try_send_error)
    }

    fn send_unreliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        validate_message_size(&msg)?;
        msg.kind = MessageKind::Unreliable;
        self.unreliable_tx.try_send(msg).map_err(map_try_send_error)
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        match self.reliable_rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.connected = false;
                Err(TransportError::Disconnected)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
        }
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }

        // Reliable first: control traffic must not queue behind a backlog of
        // lossy state, and a peer that floods the unreliable channel must not
        // be able to delay a handshake or a session-key rotation.
        let reliable_gone = match self.reliable_rx.try_recv() {
            Ok(msg) => return Ok(Some(msg)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => true,
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
        };

        // Messages already queued outlive the peer that sent them; the
        // transport only reports the disconnect once both channels are drained.
        match self.unreliable_rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) if reliable_gone => {
                self.connected = false;
                Err(TransportError::Disconnected)
            }
            Err(_) => Ok(None),
        }
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
    fn a_reliable_message_reaches_the_far_end_with_its_kind_and_payload_intact() {
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
    fn an_unreliable_message_reaches_the_far_end_with_its_kind_and_payload_intact() {
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
    fn reliable_control_can_bypass_unreliable_backlog() {
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

        let first = b.recv_reliable().unwrap().unwrap();
        assert_eq!(first.kind, MessageKind::Reliable);
        assert_eq!(first.payload, b"cmd");

        let second = b.recv().unwrap().unwrap();
        assert_eq!(second.kind, MessageKind::Unreliable);
        assert_eq!(second.payload, b"snap");
    }

    #[test]
    fn recv_drains_reliable_before_unreliable() {
        let (mut a, mut b) = InMemoryTransport::pair();

        a.send_unreliable(Message::unreliable(b"snap".to_vec()))
            .unwrap();
        a.send_reliable(Message::reliable(b"cmd".to_vec())).unwrap();

        // Sent second, received first — control traffic is never starved.
        let first = b.recv().unwrap().unwrap();
        assert_eq!(first.kind, MessageKind::Reliable);
        assert_eq!(first.payload, b"cmd");
        assert_eq!(b.recv().unwrap().unwrap().payload, b"snap");
    }

    #[test]
    fn the_sending_call_decides_the_channel_not_the_kind_field() {
        let (mut a, mut b) = InMemoryTransport::pair();

        // A message mislabelled `Reliable` sent on the unreliable channel
        // stays unreliable, and arrives labelled as such.
        a.send_unreliable(Message {
            kind: MessageKind::Reliable,
            payload: b"lie".to_vec(),
        })
        .unwrap();
        assert!(b.recv_reliable().unwrap().is_none());
        let received = b.recv().unwrap().unwrap();
        assert_eq!(received.kind, MessageKind::Unreliable);
        assert_eq!(received.payload, b"lie");
    }

    #[test]
    fn buffered_unreliable_messages_survive_the_peer_dropping() {
        let (mut a, mut b) = InMemoryTransport::pair();
        a.send_unreliable(Message::unreliable(b"last".to_vec()))
            .unwrap();
        drop(a);

        assert_eq!(b.recv().unwrap().unwrap().payload, b"last");
        assert!(matches!(b.recv(), Err(TransportError::Disconnected)));
    }

    #[test]
    fn queues_are_bounded_and_payloads_are_size_gated() {
        let (mut sender, _receiver) = InMemoryTransport::pair();
        let message = || Message {
            kind: MessageKind::Unreliable,
            payload: vec![0; 1],
        };
        for _ in 0..IN_MEMORY_CHANNEL_CAPACITY {
            sender.send_unreliable(message()).unwrap();
        }
        assert!(matches!(
            sender.send_unreliable(message()),
            Err(TransportError::Backpressure)
        ));
        assert!(matches!(
            sender.send_reliable(Message {
                kind: MessageKind::Reliable,
                payload: vec![0; MAX_IN_MEMORY_MESSAGE_BYTES + 1],
            }),
            Err(TransportError::MessageTooLarge { .. })
        ));
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
    fn sending_after_a_local_disconnect_reports_disconnected_rather_than_queueing() {
        let (mut a, _b) = InMemoryTransport::pair();
        a.disconnect();

        let result = a.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: b"x".to_vec(),
        });
        assert!(matches!(result, Err(TransportError::Disconnected)));
    }

    #[test]
    fn receiving_after_a_local_disconnect_reports_disconnected() {
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
    fn each_end_of_a_pair_receives_what_the_other_sent_and_not_its_own() {
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
    fn queued_reliable_messages_arrive_in_send_order_and_then_the_queue_is_empty() {
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
    fn every_transport_type_debug_prints_without_panicking() {
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
