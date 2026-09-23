//! The behaviour [`Transport`]'s documentation promises, as checks any
//! implementation can be run through.
//!
//! Each check takes a [`Link`] — a way to make a connected pair of the
//! transport under test and to let traffic cross between them — and panics,
//! naming what broke, when the transport does not do what the trait says. The
//! checks cover the promises a caller builds on:
//!
//! - reliable traffic is drained before unreliable
//!   ([`reliable_is_received_before_unreliable`]);
//! - [`Transport::recv_reliable`] never hands back unreliable traffic
//!   ([`recv_reliable_never_returns_unreliable`]);
//! - [`Message::kind`] on a received message is set by the sending call, not
//!   copied from the sender's field ([`the_send_method_sets_the_kind`]);
//! - reliable messages arrive in the order sent, each end receives the
//!   other's traffic and not its own ([`reliable_messages_arrive_in_order`]);
//! - an oversized payload is [`TransportError::MessageTooLarge`] carrying the
//!   size and the limit, and one at the limit is accepted
//!   ([`an_oversized_message_names_its_size_and_the_limit`]);
//! - an end whose peer is gone reports [`TransportError::Disconnected`] and
//!   stops claiming to be connected
//!   ([`a_dropped_peer_is_disconnected`]).
//!
//! [`check_all`] runs every one. Behind the `conformance` feature, for the
//! crates that implement a transport; `crcbl-net`'s own tests run it against
//! [`InMemoryTransport`](crate::InMemoryTransport).

use crate::{Message, MessageKind, Transport, TransportError};

/// How a transport under test is paired up and driven.
pub trait Link {
    /// The transport under test.
    type Transport: Transport;

    /// A fresh pair, connected to each other.
    fn pair(&mut self) -> (Self::Transport, Self::Transport);

    /// Lets what either end has sent — messages, and a closed connection —
    /// reach the other. A transport that delivers on send does nothing here;
    /// one driven by an event pump runs it.
    fn settle(&mut self);

    /// The largest payload the transport accepts.
    fn max_message_bytes(&self) -> usize;
}

/// How many `recv` calls a check makes before deciding a queue will not
/// drain. Far above what any check queues.
const DRAIN_LIMIT: usize = 1024;

/// Receives everything queued on `end`, in order.
fn drain<T: Transport>(end: &mut T) -> Vec<Message> {
    let mut out = Vec::new();
    for _ in 0..DRAIN_LIMIT {
        match end.recv() {
            Ok(Some(message)) => out.push(message),
            Ok(None) => return out,
            Err(error) => panic!("recv failed on a connected pair: {error}"),
        }
    }
    panic!("recv never ran dry after {DRAIN_LIMIT} messages");
}

/// Runs every check against `link`.
pub fn check_all<L: Link>(link: &mut L) {
    reliable_is_received_before_unreliable(link);
    recv_reliable_never_returns_unreliable(link);
    the_send_method_sets_the_kind(link);
    reliable_messages_arrive_in_order(link);
    an_oversized_message_names_its_size_and_the_limit(link);
    a_dropped_peer_is_disconnected(link);
}

/// Unreliable sent first, reliable second: `recv` yields the reliable one
/// first.
pub fn reliable_is_received_before_unreliable<L: Link>(link: &mut L) {
    let (mut a, mut b) = link.pair();
    a.send_unreliable(Message::unreliable(b"state".to_vec()))
        .expect("send unreliable");
    a.send_reliable(Message::reliable(b"control".to_vec()))
        .expect("send reliable");
    link.settle();
    let received = drain(&mut b);
    let payloads: Vec<&[u8]> = received.iter().map(|m| m.payload.as_slice()).collect();
    assert_eq!(
        payloads,
        [b"control".as_slice(), b"state".as_slice()],
        "reliable traffic must be received before unreliable"
    );
}

/// With only unreliable traffic queued, `recv_reliable` finds nothing, and
/// `recv` still delivers it.
pub fn recv_reliable_never_returns_unreliable<L: Link>(link: &mut L) {
    let (mut a, mut b) = link.pair();
    a.send_unreliable(Message::unreliable(b"state".to_vec()))
        .expect("send unreliable");
    link.settle();
    let reliable = b.recv_reliable().expect("recv_reliable");
    assert!(
        reliable.is_none(),
        "recv_reliable returned unreliable traffic: {reliable:?}"
    );
    assert_eq!(drain(&mut b).len(), 1, "the unreliable message was lost");
}

/// A message whose `kind` field disagrees with the send method arrives
/// labelled by the method.
pub fn the_send_method_sets_the_kind<L: Link>(link: &mut L) {
    let (mut a, mut b) = link.pair();
    a.send_unreliable(Message {
        kind: MessageKind::Reliable,
        payload: b"said reliable".to_vec(),
    })
    .expect("send unreliable");
    a.send_reliable(Message {
        kind: MessageKind::Unreliable,
        payload: b"said unreliable".to_vec(),
    })
    .expect("send reliable");
    link.settle();
    let received = drain(&mut b);
    let kinds: Vec<(MessageKind, &[u8])> = received
        .iter()
        .map(|m| (m.kind, m.payload.as_slice()))
        .collect();
    assert_eq!(
        kinds,
        [
            (MessageKind::Reliable, b"said unreliable".as_slice()),
            (MessageKind::Unreliable, b"said reliable".as_slice()),
        ],
        "the kind must be the sending call's"
    );
}

/// Reliable messages both ways: each end receives the other's, in order,
/// and never its own.
pub fn reliable_messages_arrive_in_order<L: Link>(link: &mut L) {
    let (mut a, mut b) = link.pair();
    for i in 0..5_u8 {
        a.send_reliable(Message::reliable(vec![b'a', i]))
            .expect("send from a");
        b.send_reliable(Message::reliable(vec![b'b', i]))
            .expect("send from b");
    }
    link.settle();
    let at_b: Vec<Vec<u8>> = drain(&mut b).into_iter().map(|m| m.payload).collect();
    let at_a: Vec<Vec<u8>> = drain(&mut a).into_iter().map(|m| m.payload).collect();
    let expected = |from: u8| (0..5_u8).map(|i| vec![from, i]).collect::<Vec<_>>();
    assert_eq!(at_b, expected(b'a'), "b must receive a's messages in order");
    assert_eq!(at_a, expected(b'b'), "a must receive b's messages in order");
}

/// One byte over the limit is refused, naming both numbers; the limit
/// itself is accepted, on both channels.
pub fn an_oversized_message_names_its_size_and_the_limit<L: Link>(link: &mut L) {
    let limit = link.max_message_bytes();
    let (mut a, _b) = link.pair();
    for reliable in [true, false] {
        let send = |a: &mut L::Transport, size: usize| {
            let payload = vec![0; size];
            if reliable {
                a.send_reliable(Message::reliable(payload))
            } else {
                a.send_unreliable(Message::unreliable(payload))
            }
        };
        match send(&mut a, limit + 1) {
            Err(TransportError::MessageTooLarge { size, limit: said }) => {
                assert_eq!((size, said), (limit + 1, limit), "reliable: {reliable}");
            }
            other => panic!("{} bytes over a {limit}-byte limit: {other:?}", limit + 1),
        }
        send(&mut a, limit).expect("a payload at the limit must be accepted");
    }
}

/// Once the peer is dropped and that has settled, `recv` reports
/// [`TransportError::Disconnected`] and `is_connected` is false.
pub fn a_dropped_peer_is_disconnected<L: Link>(link: &mut L) {
    let (a, mut b) = link.pair();
    assert!(b.is_connected(), "a fresh pair must be connected");
    drop(a);
    link.settle();
    let mut outcome = None;
    for _ in 0..DRAIN_LIMIT {
        match b.recv() {
            Ok(Some(_)) => {}
            Ok(None) => {
                outcome = Some(Ok(()));
                break;
            }
            Err(error) => {
                outcome = Some(Err(error));
                break;
            }
        }
    }
    assert!(
        matches!(outcome, Some(Err(TransportError::Disconnected))),
        "recv after the peer dropped: {outcome:?}"
    );
    assert!(!b.is_connected(), "is_connected after the peer dropped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryTransport, transport::MAX_IN_MEMORY_MESSAGE_BYTES};

    /// `InMemoryTransport` delivers on send, so settling is nothing.
    struct InMemory;

    impl Link for InMemory {
        type Transport = InMemoryTransport;

        fn pair(&mut self) -> (InMemoryTransport, InMemoryTransport) {
            InMemoryTransport::pair()
        }

        fn settle(&mut self) {}

        fn max_message_bytes(&self) -> usize {
            MAX_IN_MEMORY_MESSAGE_BYTES
        }
    }

    // One test per check, so a transport that breaks one says which; the
    // same six are what `check_all` runs for other crates.
    #[test]
    fn in_memory_reliable_is_received_before_unreliable() {
        reliable_is_received_before_unreliable(&mut InMemory);
    }

    #[test]
    fn in_memory_recv_reliable_never_returns_unreliable() {
        recv_reliable_never_returns_unreliable(&mut InMemory);
    }

    #[test]
    fn in_memory_the_send_method_sets_the_kind() {
        the_send_method_sets_the_kind(&mut InMemory);
    }

    #[test]
    fn in_memory_reliable_messages_arrive_in_order() {
        reliable_messages_arrive_in_order(&mut InMemory);
    }

    #[test]
    fn in_memory_an_oversized_message_names_its_size_and_the_limit() {
        an_oversized_message_names_its_size_and_the_limit(&mut InMemory);
    }

    #[test]
    fn in_memory_a_dropped_peer_is_disconnected() {
        a_dropped_peer_is_disconnected(&mut InMemory);
    }
}
