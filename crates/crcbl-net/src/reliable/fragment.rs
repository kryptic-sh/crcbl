//! The reliable channel's receive side: fragments in, whole messages out, in
//! order, inside a fixed memory budget.
//!
//! Fragments arrive in any order, duplicated, for messages ahead of the one the
//! channel is waiting on. Each is filed under its message's slot in a window of
//! [`RELIABLE_WINDOW`] messages starting at the next id to deliver; a message
//! leaves only when it is complete and every message before it has left.
//!
//! **Refusal is the backpressure.** A fragment the budget cannot hold is
//! refused before the packet carrying it is acknowledged, so the sender keeps
//! it and sends it again. Nothing received is ever dropped after being acked.
//! Against an honest peer the budget is never the thing that refuses: the
//! sender's own cap, [`MAX_RELIABLE_BYTES_IN_FLIGHT`], spans everything from
//! its oldest unacknowledged message onward, and every message this side
//! holds lies in that span — because the endpoint takes each complete message
//! out the moment it is next in order, what stays here always sits behind an
//! incomplete one, whose missing fragment the sender has not seen acked.

use super::endpoint::{MAX_RELIABLE_BYTES_IN_FLIGHT, RELIABLE_WINDOW, ReceiveError};
use super::packet::{Fragment, MAX_FRAGMENT_BYTES};
use super::sequence::HALF_RANGE;

/// The `index`-th piece a payload is sent as: consecutive
/// [`MAX_FRAGMENT_BYTES`] runs, the last one short. An empty payload is one
/// empty piece, so it still has a fragment to be acknowledged by.
pub(crate) fn piece(payload: &[u8], index: usize) -> &[u8] {
    let start = (index * MAX_FRAGMENT_BYTES).min(payload.len());
    let end = (start + MAX_FRAGMENT_BYTES).min(payload.len());
    &payload[start..end]
}

/// How many pieces [`piece`] cuts a payload of `len` bytes into.
pub(crate) fn fragment_count(len: usize) -> usize {
    len.div_ceil(MAX_FRAGMENT_BYTES).max(1)
}

/// One message part-way through reassembly.
#[derive(Debug)]
struct Partial {
    id: u16,
    pieces: Vec<Option<Vec<u8>>>,
    received: usize,
    bytes: usize,
}

impl Partial {
    fn is_complete(&self) -> bool {
        self.received == self.pieces.len()
    }
}

/// What filing a fragment did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filed {
    /// Stored; the packet may be acknowledged.
    Stored,
    /// Already held, or already delivered: nothing to store, and the packet
    /// may be acknowledged again.
    Duplicate,
}

/// The reorder window and the fragments filed in it.
#[derive(Debug)]
pub(crate) struct Reassembly {
    next_id: u16,
    slots: Vec<Option<Partial>>,
    buffered_bytes: usize,
}

impl Reassembly {
    /// An empty window whose first delivery will be message `first_id`.
    pub(crate) fn new(first_id: u16) -> Self {
        Self {
            next_id: first_id,
            slots: (0..RELIABLE_WINDOW).map(|_| None).collect(),
            buffered_bytes: 0,
        }
    }

    /// Payload bytes held, complete messages included.
    #[cfg(test)]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.buffered_bytes
    }

    /// File one fragment.
    ///
    /// # Errors
    ///
    /// Every error leaves the window exactly as it was, and the caller must
    /// not acknowledge the packet: [`ReceiveError::OutOfWindow`] for a
    /// message id no honest sender could have reached,
    /// [`ReceiveError::FragmentCountMismatch`] for a fragment disagreeing with
    /// its siblings about how many there are, and
    /// [`ReceiveError::ReassemblyFull`] when holding it would pass the budget.
    pub(crate) fn file(&mut self, fragment: &Fragment<'_>) -> Result<Filed, ReceiveError> {
        let offset = fragment.message_id.wrapping_sub(self.next_id);
        if offset >= HALF_RANGE {
            // Behind the window: delivered already, and resent because the
            // ack went missing.
            return Ok(Filed::Duplicate);
        }
        if usize::from(offset) >= RELIABLE_WINDOW {
            return Err(ReceiveError::OutOfWindow {
                message_id: fragment.message_id,
                next_expected: self.next_id,
            });
        }
        let slot = &mut self.slots[slot_of(fragment.message_id)];
        let count = usize::from(fragment.count);
        let index = usize::from(fragment.index);
        if let Some(partial) = slot.as_ref() {
            if partial.pieces.len() != count {
                return Err(ReceiveError::FragmentCountMismatch {
                    message_id: fragment.message_id,
                    expected: partial.pieces.len(),
                    found: count,
                });
            }
            if partial.pieces[index].is_some() {
                return Ok(Filed::Duplicate);
            }
        }
        let len = fragment.payload.len();
        if self.buffered_bytes + len > MAX_RELIABLE_BYTES_IN_FLIGHT {
            return Err(ReceiveError::ReassemblyFull {
                buffered: self.buffered_bytes,
                limit: MAX_RELIABLE_BYTES_IN_FLIGHT,
            });
        }
        let partial = slot.get_or_insert_with(|| Partial {
            id: fragment.message_id,
            pieces: vec![None; count],
            received: 0,
            bytes: 0,
        });
        partial.pieces[index] = Some(fragment.payload.to_vec());
        partial.received += 1;
        partial.bytes += len;
        self.buffered_bytes += len;
        Ok(Filed::Stored)
    }

    /// The size of the next message in order, if it is complete.
    fn ready_len(&self) -> Option<usize> {
        self.slots[slot_of(self.next_id)]
            .as_ref()
            .filter(|partial| partial.id == self.next_id && partial.is_complete())
            .map(|partial| partial.bytes)
    }

    /// Take the next message in order if it is complete, and move the window
    /// past it.
    pub(crate) fn pop_ready(&mut self) -> Option<Vec<u8>> {
        self.ready_len()?;
        let partial = self.slots[slot_of(self.next_id)].take()?;
        self.buffered_bytes -= partial.bytes;
        self.next_id = self.next_id.wrapping_add(1);
        let mut message = Vec::with_capacity(partial.bytes);
        for piece in partial.pieces.into_iter().flatten() {
            message.extend_from_slice(&piece);
        }
        Some(message)
    }
}

/// A message id's slot. The window is a power of two, so the ids inside it map
/// to distinct slots even as they wrap.
fn slot_of(message_id: u16) -> usize {
    usize::from(message_id) % RELIABLE_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(message_id: u16, index: u8, count: u8, payload: &[u8]) -> Fragment<'_> {
        Fragment {
            message_id,
            index,
            count,
            payload,
        }
    }

    #[test]
    fn pieces_are_full_fragments_and_an_empty_payload_is_one_empty_piece() {
        assert_eq!(fragment_count(0), 1);
        assert!(piece(&[], 0).is_empty());

        let payload: Vec<u8> = (0..MAX_FRAGMENT_BYTES * 2 + 1).map(|i| i as u8).collect();
        assert_eq!(fragment_count(payload.len()), 3);
        let pieces: Vec<&[u8]> = (0..3).map(|i| piece(&payload, i)).collect();
        assert_eq!(pieces[0].len(), MAX_FRAGMENT_BYTES);
        assert_eq!(pieces[1].len(), MAX_FRAGMENT_BYTES);
        assert_eq!(pieces[2].len(), 1);
        assert_eq!(pieces.concat(), payload);
        assert_eq!(fragment_count(MAX_FRAGMENT_BYTES), 1);
        assert_eq!(fragment_count(MAX_FRAGMENT_BYTES + 1), 2);
    }

    /// Fragments out of order and messages out of order still come out whole
    /// and in id order, across the id wrap.
    #[test]
    fn messages_leave_whole_and_in_order_whatever_order_fragments_arrive_in() {
        let mut window = Reassembly::new(u16::MAX);
        let tail = [2u8; 5];
        let full = [1u8; MAX_FRAGMENT_BYTES];
        // Message 0 (after the wrap) arrives complete before 65 535.
        assert_eq!(
            window.file(&fragment(0, 0, 1, b"second")).unwrap(),
            Filed::Stored
        );
        assert_eq!(window.pop_ready(), None);
        assert_eq!(
            window.file(&fragment(u16::MAX, 1, 2, &tail)).unwrap(),
            Filed::Stored
        );
        assert_eq!(window.pop_ready(), None);
        assert_eq!(
            window.file(&fragment(u16::MAX, 1, 2, &tail)).unwrap(),
            Filed::Duplicate
        );
        assert_eq!(
            window.file(&fragment(u16::MAX, 0, 2, &full)).unwrap(),
            Filed::Stored
        );

        let first = window.pop_ready().unwrap();
        assert_eq!(first.len(), MAX_FRAGMENT_BYTES + tail.len());
        assert_eq!(&first[MAX_FRAGMENT_BYTES..], &tail);
        assert_eq!(window.pop_ready().unwrap(), b"second");
        assert_eq!(window.buffered_bytes(), 0);

        // Both are behind the window now: a resend is a duplicate.
        assert_eq!(
            window.file(&fragment(0, 0, 1, b"second")).unwrap(),
            Filed::Duplicate
        );
    }

    #[test]
    fn a_fragment_that_contradicts_its_siblings_or_the_window_is_refused_untouched() {
        let mut window = Reassembly::new(10);
        window
            .file(&fragment(10, 0, 2, &[0; MAX_FRAGMENT_BYTES]))
            .unwrap();
        let before = window.buffered_bytes();

        assert!(matches!(
            window.file(&fragment(10, 1, 3, b"x")),
            Err(ReceiveError::FragmentCountMismatch { .. })
        ));
        let beyond = 10 + u16::try_from(RELIABLE_WINDOW).unwrap();
        assert!(matches!(
            window.file(&fragment(beyond, 0, 1, b"x")),
            Err(ReceiveError::OutOfWindow { .. })
        ));
        assert_eq!(window.buffered_bytes(), before);
        assert_eq!(window.ready_len(), None);
    }

    /// A peer filling every slot with incomplete messages reaches the budget
    /// and is refused there; the held bytes never pass it.
    #[test]
    fn a_flood_of_partial_messages_stops_at_the_budget() {
        let mut window = Reassembly::new(0);
        let full = [0u8; MAX_FRAGMENT_BYTES];
        let count = u8::try_from(super::super::MAX_FRAGMENTS_PER_MESSAGE).unwrap();
        let mut refused = 0;
        for id in 0..u16::try_from(RELIABLE_WINDOW).unwrap() {
            // Every piece but the last, so nothing ever completes.
            for index in 0..count - 1 {
                match window.file(&fragment(id, index, count, &full)) {
                    Ok(_) => {}
                    Err(ReceiveError::ReassemblyFull { .. }) => refused += 1,
                    Err(other) => panic!("unexpected refusal: {other}"),
                }
                assert!(window.buffered_bytes() <= MAX_RELIABLE_BYTES_IN_FLIGHT);
            }
        }
        assert!(refused > 0, "the flood must have reached the budget");
        assert!(window.buffered_bytes() + MAX_FRAGMENT_BYTES > MAX_RELIABLE_BYTES_IN_FLIGHT);
    }
}
