//! The reliable channel's send side: messages held until every fragment is
//! acknowledged, and resent on the RTT-derived timeout until it is.
//!
//! Messages sit in id order from the oldest one not yet fully acknowledged.
//! Only the front leaves, so a message acknowledged ahead of an older one keeps
//! its place — and its bytes on the in-flight count — until everything before
//! it has gone too. That is what makes the count an honest bound on what the
//! receiver may be holding: see the `fragment` module's header.

use std::collections::VecDeque;
use std::time::Duration;

use super::endpoint::{MAX_RELIABLE_BYTES_IN_FLIGHT, RELIABLE_WINDOW};
use super::fragment::{fragment_count, piece};
use super::packet::{Fragment, MAX_RELIABLE_MESSAGE_BYTES};
use super::rtt::RttEstimator;
use crate::TransportError;

#[derive(Debug, Clone, Copy, Default)]
struct FragmentState {
    /// When this fragment last went out; `None` until the first time.
    sent_at: Option<Duration>,
    /// Transmissions after the first.
    resends: u32,
    acked: bool,
}

#[derive(Debug)]
struct OutgoingMessage {
    id: u16,
    payload: Vec<u8>,
    fragments: Vec<FragmentState>,
}

impl OutgoingMessage {
    fn is_acked(&self) -> bool {
        self.fragments.iter().all(|fragment| fragment.acked)
    }
}

/// A fragment [`SendBuffer::next_due`] picked to go out now.
#[derive(Debug)]
pub(crate) struct Due<'a> {
    pub(crate) fragment: Fragment<'a>,
    /// Whether this is a resend rather than the first transmission.
    pub(crate) resend: bool,
}

/// The reliable messages not yet fully acknowledged.
#[derive(Debug)]
pub(crate) struct SendBuffer {
    messages: VecDeque<OutgoingMessage>,
    next_id: u16,
    bytes: usize,
}

impl SendBuffer {
    /// An empty buffer whose first message will be `first_id`.
    pub(crate) fn new(first_id: u16) -> Self {
        Self {
            messages: VecDeque::new(),
            next_id: first_id,
            bytes: 0,
        }
    }

    /// Payload bytes held, acknowledged-but-not-yet-front messages included.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Messages held.
    pub(crate) fn len(&self) -> usize {
        self.messages.len()
    }

    /// Drop everything held; the ids carry on from where they were.
    pub(crate) fn clear(&mut self) {
        self.messages.clear();
        self.bytes = 0;
    }

    /// Queue `payload` as the next message.
    ///
    /// # Errors
    ///
    /// [`TransportError::MessageTooLarge`] past
    /// [`MAX_RELIABLE_MESSAGE_BYTES`], and [`TransportError::Backpressure`]
    /// when the buffer already holds [`RELIABLE_WINDOW`] messages or the
    /// payload would take it past [`MAX_RELIABLE_BYTES_IN_FLIGHT`]. Either
    /// way nothing is queued.
    pub(crate) fn push(&mut self, payload: Vec<u8>) -> Result<(), TransportError> {
        if payload.len() > MAX_RELIABLE_MESSAGE_BYTES {
            return Err(TransportError::MessageTooLarge {
                size: payload.len(),
                limit: MAX_RELIABLE_MESSAGE_BYTES,
            });
        }
        if self.messages.len() >= RELIABLE_WINDOW
            || self.bytes + payload.len() > MAX_RELIABLE_BYTES_IN_FLIGHT
        {
            return Err(TransportError::Backpressure);
        }
        self.bytes += payload.len();
        self.messages.push_back(OutgoingMessage {
            id: self.next_id,
            fragments: vec![FragmentState::default(); fragment_count(payload.len())],
            payload,
        });
        self.next_id = self.next_id.wrapping_add(1);
        Ok(())
    }

    /// Mark one fragment acknowledged, and release every fully acknowledged
    /// message at the front. An ack for a message already released, or one
    /// never sent, changes nothing.
    pub(crate) fn ack(&mut self, message_id: u16, index: u8) {
        let Some(front) = self.messages.front() else {
            return;
        };
        let offset = usize::from(message_id.wrapping_sub(front.id));
        let Some(message) = self.messages.get_mut(offset) else {
            return;
        };
        if let Some(fragment) = message.fragments.get_mut(usize::from(index)) {
            fragment.acked = true;
        }
        while self.messages.front().is_some_and(OutgoingMessage::is_acked) {
            if let Some(released) = self.messages.pop_front() {
                self.bytes -= released.payload.len();
            }
        }
    }

    /// The first fragment due to go out at `now` — never sent, or unacked past
    /// its backed-off timeout — marked as sent.
    pub(crate) fn next_due(&mut self, now: Duration, rtt: &RttEstimator) -> Option<Due<'_>> {
        for message in &mut self.messages {
            let count = message.fragments.len();
            for (index, state) in message.fragments.iter_mut().enumerate() {
                if state.acked {
                    continue;
                }
                let resend = match state.sent_at {
                    None => false,
                    Some(sent_at) => {
                        if now < sent_at.saturating_add(rtt.backed_off_rto(state.resends)) {
                            continue;
                        }
                        state.resends = state.resends.saturating_add(1);
                        true
                    }
                };
                state.sent_at = Some(now);
                return Some(Due {
                    fragment: Fragment {
                        message_id: message.id,
                        index: u8::try_from(index).ok()?,
                        count: u8::try_from(count).ok()?,
                        payload: piece(&message.payload, index),
                    },
                    resend,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_leaves_only_when_every_fragment_and_every_older_message_is_acked() {
        let mut buffer = SendBuffer::new(u16::MAX);
        buffer
            .push(vec![1; super::super::MAX_FRAGMENT_BYTES + 1])
            .unwrap();
        buffer.push(b"b".to_vec()).unwrap();
        assert_eq!(buffer.len(), 2);

        // The younger message acked first keeps its place and its bytes.
        buffer.ack(0, 0);
        assert_eq!(buffer.len(), 2);
        buffer.ack(u16::MAX, 1);
        assert_eq!(buffer.len(), 2);
        buffer.ack(u16::MAX, 0);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.bytes(), 0);

        // Stale and foreign acks are ignored.
        buffer.ack(u16::MAX, 0);
        buffer.ack(1234, 9);
    }

    #[test]
    fn a_fragment_is_resent_after_its_timeout_and_backs_off() {
        let mut buffer = SendBuffer::new(0);
        let rtt = RttEstimator::default();
        buffer.push(b"x".to_vec()).unwrap();
        let t0 = Duration::ZERO;
        assert!(!buffer.next_due(t0, &rtt).unwrap().resend);
        assert!(buffer.next_due(t0, &rtt).is_none());

        let first = rtt.backed_off_rto(0);
        assert!(
            buffer
                .next_due(first - Duration::from_nanos(1), &rtt)
                .is_none()
        );
        assert!(buffer.next_due(first, &rtt).unwrap().resend);

        // The second wait is the doubled timeout, measured from the resend.
        let second = first + rtt.backed_off_rto(1);
        assert!(
            buffer
                .next_due(second - Duration::from_nanos(1), &rtt)
                .is_none()
        );
        assert!(buffer.next_due(second, &rtt).unwrap().resend);

        buffer.ack(0, 0);
        assert!(buffer.next_due(Duration::from_secs(60), &rtt).is_none());
    }

    #[test]
    fn the_buffer_refuses_past_its_window_its_bytes_and_the_message_cap() {
        let mut buffer = SendBuffer::new(0);
        assert!(matches!(
            buffer.push(vec![0; MAX_RELIABLE_MESSAGE_BYTES + 1]),
            Err(TransportError::MessageTooLarge { .. })
        ));
        for _ in 0..RELIABLE_WINDOW {
            buffer.push(Vec::new()).unwrap();
        }
        assert!(matches!(
            buffer.push(Vec::new()),
            Err(TransportError::Backpressure)
        ));

        let mut buffer = SendBuffer::new(0);
        while buffer.bytes() + MAX_RELIABLE_MESSAGE_BYTES <= MAX_RELIABLE_BYTES_IN_FLIGHT {
            buffer.push(vec![0; MAX_RELIABLE_MESSAGE_BYTES]).unwrap();
        }
        let room = MAX_RELIABLE_BYTES_IN_FLIGHT - buffer.bytes();
        assert!(matches!(
            buffer.push(vec![0; room + 1]),
            Err(TransportError::Backpressure)
        ));
        buffer.push(vec![0; room]).unwrap();
        assert_eq!(buffer.bytes(), MAX_RELIABLE_BYTES_IN_FLIGHT);
    }
}
