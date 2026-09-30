//! Which packets have arrived, in the form a packet header acks them.
//!
//! Every outgoing packet carries the newest sequence received from the peer and
//! a bitfield for the [`ACK_BITS`] sequences before it, so one packet
//! acknowledges a whole window and an ack lost on the wire is repeated by every
//! packet after it (Gaffer On Games, _Reliability and Congestion Avoidance over
//! UDP_). [`ReceivedWindow`] is that state, kept in exactly the shape it goes
//! out in.

use super::packet::PacketAcks;
use super::sequence::sequence_greater_than;

/// Sequences behind the newest one a packet can acknowledge: the width of
/// [`PacketAcks::bits`].
pub(crate) const ACK_BITS: u16 = u64::BITS as u16;

/// What recording a received sequence found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// First sighting, now recorded and acknowledged by the next packet out.
    New,
    /// Already recorded: the wire duplicated it, or the peer resent it.
    Duplicate,
    /// Further behind the newest than the bitfield reaches. It cannot be
    /// acknowledged, and whether it was seen before cannot be told.
    TooOld,
}

/// The newest received sequence and the bitfield of the ones before it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReceivedWindow {
    newest: Option<u16>,
    /// Bit `i` set means `newest - 1 - i` has been received.
    bits: u64,
}

impl ReceivedWindow {
    /// What recording `sequence` would find, without recording it.
    pub(crate) fn classify(&self, sequence: u16) -> Arrival {
        let Some(newest) = self.newest else {
            return Arrival::New;
        };
        if sequence == newest {
            return Arrival::Duplicate;
        }
        if sequence_greater_than(sequence, newest) {
            return Arrival::New;
        }
        let behind = newest.wrapping_sub(sequence);
        if behind > ACK_BITS {
            Arrival::TooOld
        } else if self.bits & (1u64 << (behind - 1)) != 0 {
            Arrival::Duplicate
        } else {
            Arrival::New
        }
    }

    /// Record `sequence` as received, and say what it was.
    pub(crate) fn record(&mut self, sequence: u16) -> Arrival {
        let arrival = self.classify(sequence);
        if arrival != Arrival::New {
            return arrival;
        }
        match self.newest {
            Some(newest) if !sequence_greater_than(sequence, newest) => {
                self.bits |= 1u64 << (newest.wrapping_sub(sequence) - 1);
            }
            Some(newest) => {
                // The old newest slides into the bitfield at `shift - 1`, and
                // everything already there slides with it. A jump wider than
                // the bitfield leaves nothing of the old window worth keeping.
                let shift = sequence.wrapping_sub(newest);
                self.bits = if shift > ACK_BITS {
                    0
                } else {
                    let shift = u32::from(shift);
                    self.bits.checked_shl(shift).unwrap_or(0) | (1 << (shift - 1))
                };
                self.newest = Some(sequence);
            }
            None => {
                self.newest = Some(sequence);
                self.bits = 0;
            }
        }
        arrival
    }

    /// The ack field for the next packet out, or `None` before anything has
    /// arrived — a zeroed field would acknowledge sequence 0.
    pub(crate) fn acks(&self) -> Option<PacketAcks> {
        self.newest.map(|latest| PacketAcks {
            latest,
            bits: self.bits,
        })
    }

    /// The newest sequence received, if any.
    pub(crate) fn newest(&self) -> Option<u16> {
        self.newest
    }
}

impl PacketAcks {
    /// Every sequence this field acknowledges: [`Self::latest`] and each one
    /// its bitfield marks.
    pub fn acknowledged(&self) -> impl Iterator<Item = u16> + '_ {
        let latest = self.latest;
        std::iter::once(latest).chain(
            (0..ACK_BITS)
                .filter(|&i| self.bits & (1u64 << i) != 0)
                .map(move |i| latest.wrapping_sub(i + 1)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acknowledged(window: &ReceivedWindow) -> Vec<u16> {
        let mut seen: Vec<u16> = window
            .acks()
            .map(|acks| acks.acknowledged().collect())
            .unwrap_or_default();
        seen.sort_unstable();
        seen
    }

    #[test]
    fn nothing_is_acknowledged_before_the_first_packet() {
        assert!(ReceivedWindow::default().acks().is_none());
    }

    /// The window marks the right sequences as it crosses the wrap, with a
    /// gap in the middle that must stay unacknowledged.
    #[test]
    fn the_bitfield_marks_received_sequences_across_the_wrap() {
        let mut window = ReceivedWindow::default();
        for sequence in [65_533, 65_534, 0, 1] {
            assert_eq!(window.record(sequence), Arrival::New);
        }
        let acks = window.acks().unwrap();
        assert_eq!(acks.latest, 1);
        // 1 - 1 = 0 is bit 0; 1 - 3 = 65 534 is bit 2; 1 - 4 = 65 533 is bit
        // 3. 65 535 never arrived, so bit 1 is clear.
        assert_eq!(acks.bits, 0b1101);
        assert_eq!(acknowledged(&window), vec![0, 1, 65_533, 65_534]);

        // The straggler fills its gap, and a second copy is a duplicate.
        assert_eq!(window.record(65_535), Arrival::New);
        assert_eq!(window.acks().unwrap().bits, 0b1111);
        assert_eq!(window.record(65_535), Arrival::Duplicate);
        assert_eq!(window.record(1), Arrival::Duplicate);
    }

    #[test]
    fn a_sequence_behind_the_bitfield_is_too_old_to_acknowledge() {
        let mut window = ReceivedWindow::default();
        window.record(10);
        assert_eq!(window.record(10u16.wrapping_sub(ACK_BITS)), Arrival::New);
        assert_eq!(
            window.record(10u16.wrapping_sub(ACK_BITS + 1)),
            Arrival::TooOld
        );
    }

    /// A jump of exactly the bitfield's width keeps the old newest in the top
    /// bit; one further drops it, because it is out of reach.
    #[test]
    fn a_jump_keeps_the_old_newest_only_while_the_bitfield_reaches_it() {
        let mut window = ReceivedWindow::default();
        window.record(65_500);
        window.record(65_500u16.wrapping_add(ACK_BITS));
        assert_eq!(window.acks().unwrap().bits, 1 << (ACK_BITS - 1));

        let mut window = ReceivedWindow::default();
        window.record(65_500);
        window.record(65_500u16.wrapping_add(ACK_BITS + 1));
        assert_eq!(window.acks().unwrap().bits, 0);
    }
}
