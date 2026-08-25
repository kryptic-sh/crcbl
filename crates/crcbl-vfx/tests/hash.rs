//! The per-particle hash: that it is the published function, and that it
//! behaves like one.
//!
//! # There are no published test vectors, so this does three other things
//!
//! *Hash Functions for GPU Rendering* gives `pcg3d` as a listing and no table
//! of values, so there is nothing external to compare against. What the pins
//! below are is a **transcription guard**: they were taken from an independent
//! second transcription of the published listing, and they are what goes red
//! when a constant, a shift or the order of the two mixing rounds moves. They
//! are also worth pinning on their own terms — the hash decides what every
//! effect in the engine looks like, so changing it is a change to every golden
//! frame and should not be possible by accident.
//!
//! Beside them are the statistical properties a hash has and a mistyped one
//! does not: an avalanche, a flat distribution, and independence between the
//! streams a particle draws from.

use crcbl_vfx::hash::{pcg3d, range, unit};

/// Six inputs and what the published algorithm makes of them. Chosen for the
/// edges: all zero (where an LCG's increment is the only thing that saves it),
/// each input alone, a realistic (seed, index, stream), and all bits set.
const PINS: &[([u32; 3], [u32; 3])] = &[
    ([0, 0, 0], [0x9baf_d7c6, 0xa8e8_8a6b, 0x3f15_482c]),
    ([1, 0, 0], [0xa1fe_c616, 0x54e4_e5e7, 0x7b1d_d140]),
    ([0, 1, 0], [0x1ad3_129c, 0x4ed8_1c0f, 0x7473_b11a]),
    ([0, 0, 1], [0x7f5d_b8d2, 0x68e0_a5ac, 0x3b10_c274]),
    ([0x5EED, 7, 1], [0x57db_1a0a, 0x6bf2_4cf9, 0x9d99_fc1c]),
    (
        [u32::MAX, u32::MAX, u32::MAX],
        [0xa5f4_8f40, 0xa453_3e83, 0x515b_8a62],
    ),
];

#[test]
fn the_hash_is_the_published_function() {
    for (input, expected) in PINS {
        assert_eq!(
            pcg3d(*input),
            *expected,
            "pcg3d{input:?} is not what the published listing makes of it"
        );
    }
}

#[test]
fn the_same_input_always_gives_the_same_words() {
    for (input, _) in PINS {
        assert_eq!(pcg3d(*input), pcg3d(*input), "the hash is not a function");
    }
}

/// No two of a long run of particle indices collide, and no particle's three
/// words repeat within itself.
#[test]
fn consecutive_particle_indices_do_not_collide() {
    const N: u32 = 200_000;
    let mut seen = std::collections::HashSet::with_capacity(N as usize);
    for index in 0..N {
        let words = pcg3d([0x5EED, index, 0]);
        assert!(
            seen.insert(words),
            "particles {index} and an earlier one hash to the same three words"
        );
        assert!(
            words[0] != words[1] && words[1] != words[2] && words[0] != words[2],
            "particle {index} draws the same word twice: {words:?}"
        );
    }
    assert_eq!(seen.len(), N as usize, "the sweep did not run");
}

/// The two streams a particle draws from are independent: no particle gets the
/// same word out of both.
#[test]
fn the_streams_of_one_particle_are_independent() {
    const N: u32 = 50_000;
    let mut shared = 0;
    for index in 0..N {
        let motion = pcg3d([0x5EED, index, 0]);
        let life = pcg3d([0x5EED, index, 1]);
        shared += motion.iter().filter(|w| life.contains(w)).count();
    }
    // Three words against three is nine chances of a 2⁻³² coincidence per
    // particle, so the expected count over this sweep is far below one. A
    // stream index the hash ignored would give exactly three per particle.
    assert_eq!(
        shared, 0,
        "the two streams of a particle share {shared} words across {N} particles, so the \
         stream index is not reaching the hash"
    );
}

/// Flipping one bit of the input changes about half of the top sixteen output
/// bits — and measurably fewer of the bottom eight.
///
/// The avalanche is what separates a hash from an arithmetic dressing-up: a
/// wrong multiplier, a dropped mixing round or a mistyped shift moves it. It is
/// measured over the *top* half of each word because `pcg3d`'s last stage is an
/// add rather than a permute, and a product's low bits depend only on its
/// operands' low bits — so the bottom byte carries about a third of a bit
/// change instead of half. Which is why [`unit`] reads from bit 8 upwards —
/// that it still does is `unit_reads_the_top_bits_and_discards_the_weak_ones`
/// below, because this test measures the hash and would not notice.
#[test]
fn one_input_bit_changes_about_half_the_top_output_bits() {
    const N: u32 = 2_000;
    let mut top = 0u64;
    let mut low = 0u64;
    let mut trials = 0u64;
    for index in 0..N {
        let base = pcg3d([0x5EED, index, 0]);
        for bit in 0..32 {
            let other = pcg3d([0x5EED, index ^ (1 << bit), 0]);
            for lane in 0..3 {
                let changed = base[lane] ^ other[lane];
                top += u64::from((changed >> 16).count_ones());
                low += u64::from((changed & 0xFF).count_ones());
            }
            trials += 1;
        }
    }
    let top = top as f64 / (trials * 3 * 16) as f64;
    let low = low as f64 / (trials * 3 * 8) as f64;
    assert!(
        (0.49..=0.51).contains(&top),
        "flipping one input bit changes {top:.4} of the top sixteen output bits, \
         not about half"
    );
    assert!(
        (0.30..=0.36).contains(&low),
        "the bottom byte's avalanche is {low:.4}; `unit` discards those bits on the \
         strength of it being about a third, so this needs re-reading"
    );
}

/// `unit` throws the bottom byte away, which is the byte the avalanche test
/// above measures as carrying about a third of a bit change instead of half.
///
/// Two directions, because one alone is half a check: that a value made only of
/// low bits reads as zero, and that two values differing only in low bits read
/// the same.
#[test]
fn unit_reads_the_top_bits_and_discards_the_weak_ones() {
    assert_eq!(
        unit(0x0000_00FF),
        0.0,
        "the bottom byte reached the result, so `unit` is reading the weakest bits \
         of the hash"
    );
    assert_eq!(
        unit(0xABCD_EF00),
        unit(0xABCD_EFFF),
        "two words differing only in their bottom byte read differently"
    );
    assert!(
        unit(0x0000_0100) > 0.0,
        "bit 8 did not reach the result, so `unit` is discarding more than a byte"
    );
}

/// `unit` covers `[0, 1)` evenly and never reaches either side of it.
#[test]
fn unit_is_flat_over_the_half_open_interval() {
    const N: u32 = 400_000;
    const BUCKETS: usize = 16;
    let mut counts = [0u32; BUCKETS];
    for index in 0..N {
        let value = unit(pcg3d([0x5EED, index, 0])[0]);
        assert!(
            (0.0..1.0).contains(&value),
            "unit produced {value}, which is outside [0, 1)"
        );
        counts[(value * BUCKETS as f32) as usize] += 1;
    }
    let expected = N as f64 / BUCKETS as f64;
    for (bucket, count) in counts.iter().enumerate() {
        let error = (f64::from(*count) - expected).abs() / expected;
        assert!(
            error < 0.02,
            "bucket {bucket} of {BUCKETS} holds {count} of {N} draws, {:.1}% off flat",
            error * 100.0
        );
    }
}

#[test]
fn range_stays_inside_its_endpoints() {
    for index in 0..10_000u32 {
        let word = pcg3d([1, index, 0])[0];
        let up = range(word, -2.0, 5.0);
        assert!(
            (-2.0..5.0).contains(&up),
            "range walked outside [-2, 5) with {up}"
        );
        let down = range(word, 5.0, -2.0);
        assert!(
            (-2.0..=5.0).contains(&down),
            "a descending range walked outside [-2, 5] with {down}"
        );
    }
    assert_eq!(
        range(0, 3.0, 3.0),
        3.0,
        "a range with equal endpoints is not that value"
    );
}
