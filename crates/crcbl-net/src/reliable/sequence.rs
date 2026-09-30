//! 16-bit sequence numbers that compare correctly across wraparound.
//!
//! Every packet and every reliable message is numbered with a `u16` that wraps
//! from `u16::MAX` back to zero, so "newer" cannot be `>`: sequence 2 sent just
//! after 65 535 is the newer of the two. The rule is Glenn Fiedler's, from
//! Gaffer On Games' _Reliability and Congestion Avoidance over UDP_: `s1` is
//! newer than `s2` when it is ahead by at most half the number space, going
//! the short way round. It holds for as long as nothing compared is more than
//! half the space apart, which the endpoint guarantees by bounding every
//! window it keeps far below that.

/// Half of the 16-bit sequence space: the furthest ahead a sequence can be and
/// still read as newer.
pub(crate) const HALF_RANGE: u16 = 1 << 15;

/// Whether `s1` is newer than `s2`, reading both as wrapping 16-bit sequences.
///
/// Transcribed from Gaffer On Games, _Reliability and Congestion Avoidance
/// over UDP_ (`sequence_greater_than`). At exactly half the space apart the
/// answer is asymmetric — `32 768` is newer than `0`, and `0` is not newer than
/// `32 768` — so the relation stays a strict order with no pair both newer
/// than each other.
#[must_use]
pub const fn sequence_greater_than(s1: u16, s2: u16) -> bool {
    (s1 > s2 && s1 - s2 <= HALF_RANGE) || (s1 < s2 && s2 - s1 > HALF_RANGE)
}

/// Whether `s1` is older than `s2`: [`sequence_greater_than`] with its
/// arguments swapped.
#[must_use]
pub const fn sequence_less_than(s1: u16, s2: u16) -> bool {
    sequence_greater_than(s2, s1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked cases, including both sides of the wrap and both sides of
    /// the half-range boundary, where the naive and the wrapping comparisons
    /// disagree.
    #[test]
    fn sequence_comparison_matches_the_worked_values_across_the_wrap() {
        // Ordinary order, nowhere near the wrap.
        assert!(sequence_greater_than(1, 0));
        assert!(!sequence_greater_than(0, 1));
        assert!(!sequence_greater_than(7, 7));

        // Across the wrap: 0 was sent after 65 535, so it is newer.
        assert!(sequence_greater_than(0, u16::MAX));
        assert!(!sequence_greater_than(u16::MAX, 0));
        assert!(sequence_greater_than(2, 65_534));

        // Exactly half the space apart: the larger value wins, one way only.
        assert!(sequence_greater_than(HALF_RANGE, 0));
        assert!(!sequence_greater_than(0, HALF_RANGE));

        // One past half: the short way round now runs the other direction.
        assert!(!sequence_greater_than(HALF_RANGE + 1, 0));
        assert!(sequence_greater_than(0, HALF_RANGE + 1));
    }

    /// Strictness over a sweep straddling the wrap: never both newer, never
    /// newer than itself, and exactly one direction for every distinct pair.
    #[test]
    fn exactly_one_of_two_distinct_sequences_is_newer() {
        let base = u16::MAX - 100;
        for offset in 1..=HALF_RANGE {
            let a = base;
            let b = base.wrapping_add(offset);
            assert!(!sequence_greater_than(a, a));
            assert_ne!(
                sequence_greater_than(a, b),
                sequence_greater_than(b, a),
                "{a} and {b} must be ordered one way",
            );
            assert_eq!(sequence_less_than(a, b), sequence_greater_than(b, a));
        }
    }
}
