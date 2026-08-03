//! Deterministic values from an index, for simulations that have to replay.
//!
//! # Why there is no generator here
//!
//! Every sample that needed random numbers wrote the same function and every
//! one of them argued, independently and at length, for the same shape: a
//! **pure function of an index**, never a running generator.
//!
//! A generator carries a position. Where that position has got to depends on
//! how many values have been drawn, which depends on how far the player got —
//! so it is simulation state that nothing declares, nothing hashes and nothing
//! replicates. Two runs of one recorded script diverge the moment they draw a
//! different *number* of values, even when they draw them for the same reasons.
//! Hashing an index instead means the four-hundredth enemy of a run enters in
//! the same place however the run reached it, and a client and a server agree
//! about a course without sending a byte of it.
//!
//! That is why this module offers no `Rng`, no `next()`, and nothing that takes
//! `&mut self`. If a caller wants a sequence it counts, and the count is
//! ordinary state it already owns and already replicates.
//!
//! # The index space is the caller's problem
//!
//! These functions mix `seed` and `index` thoroughly, so *distinct* indices give
//! independent values — but nothing here stops two draws colliding on one index.
//! A game drawing several values per spawn packs its own index space (a spawn
//! counter times the number of draws, plus which draw this is), and that packing
//! is per-game because the set of draws is.
//!
//! # What it is
//!
//! [splitmix64]'s finaliser. Small, well-mixed, and — the property that matters
//! here — identical on every target, because it is integer arithmetic with no
//! floating point in the mixing and no dependence on word order.
//!
//! [splitmix64]: https://prng.di.unimi.it/splitmix64.c

/// splitmix64's increment: the odd 64-bit approximation of the golden ratio.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// A 64-bit value from `seed` and `index`.
///
/// Distinct indices give independent-looking values; the same `(seed, index)`
/// always gives the same one, on every platform and in every run.
#[must_use]
pub fn hash_u64(seed: u64, index: u64) -> u64 {
    let mut z = seed.wrapping_add(index.wrapping_mul(GAMMA));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A uniform value in `[0, 1)` from `seed` and `index`.
///
/// The top 53 bits of [`hash_u64`], which is exactly an `f64`'s mantissa — so
/// every representable value in the range is reachable and none is reachable
/// twice as often as its neighbour.
#[must_use]
pub fn hash_unit(seed: u64, index: u64) -> f64 {
    // Truncation is the point: 53 bits chosen to fit the mantissa exactly, and
    // the divisor is a power of two, so the division is exact.
    #[allow(clippy::cast_precision_loss)]
    {
        (hash_u64(seed, index) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A seed derived from another, for dealing a different hand from the same one.
///
/// What a restart uses: run *n* of a game seeded with `seed` plays a different
/// course, deterministically, so a recorded script replayed from a fresh game
/// still meets the same course it recorded against.
///
/// Distinct from calling [`hash_u64`] and using the result as a seed only in
/// intent — this is the operation that produces *another seed*, and naming it
/// separately is what stops a reader wondering whether the two index spaces are
/// meant to be related.
#[must_use]
pub fn salt(seed: u64, n: u64) -> u64 {
    seed ^ n.wrapping_mul(0xD1B5_4A32_D192_ED03)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published splitmix64 output for a zero state, so a rewrite that
    /// still "looks random" cannot pass.
    ///
    /// These are the first three outputs of the reference implementation seeded
    /// at zero. Stepping the state by `GAMMA` each call is the same thing as
    /// hashing indices 1, 2, 3 from seed zero, which is the identity the audio
    /// noise generators depend on.
    #[test]
    fn it_matches_the_reference_vectors() {
        assert_eq!(hash_u64(0, 1), 0xE220_A839_7B1D_CDAF);
        assert_eq!(hash_u64(0, 2), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(hash_u64(0, 3), 0x06C4_5D18_8009_454F);
    }

    /// Stepping a state by `GAMMA` equals hashing successive indices.
    ///
    /// The property that lets a noise generator — which wants a *sequence* —
    /// use the same function as a spawn table, which wants an index.
    #[test]
    fn stepping_the_state_is_hashing_the_index() {
        let seed: u64 = 0x1234_5678_9ABC_DEF0;
        let mut state = seed;
        for index in 1..64 {
            state = state.wrapping_add(GAMMA);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            assert_eq!(z ^ (z >> 31), hash_u64(seed, index), "index {index}");
        }
    }

    #[test]
    fn a_unit_value_stays_inside_its_range() {
        for index in 0..10_000 {
            let unit = hash_unit(0xDEAD_BEEF, index);
            assert!((0.0..1.0).contains(&unit), "index {index} gave {unit}");
        }
    }

    /// Uniform enough that a table drawn from it is not lopsided.
    ///
    /// Ten buckets over ten thousand draws: a fair one holds a thousand, and
    /// this allows a fifth either way. Wide, deliberately — the assertion is
    /// "not visibly biased", and a tight bound on a fixed sample would be a
    /// test of this particular seed.
    #[test]
    fn the_unit_values_spread_across_the_range() {
        let mut buckets = [0u32; 10];
        for index in 0..10_000 {
            let unit = hash_unit(1, index);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bucket = (unit * 10.0) as usize;
            buckets[bucket.min(9)] += 1;
        }
        for (bucket, &count) in buckets.iter().enumerate() {
            assert!(
                (800..1200).contains(&count),
                "bucket {bucket} holds {count} of 10000, which is lopsided"
            );
        }
    }

    /// Neighbouring indices do not give neighbouring values.
    ///
    /// The failure a weaker mix would show: `seed + index` without the
    /// finaliser passes every test above and lays every pipe in a slow ramp.
    #[test]
    fn neighbouring_indices_are_not_neighbouring_values() {
        let mut close = 0;
        for index in 0..1_000 {
            let a = hash_unit(7, index);
            let b = hash_unit(7, index + 1);
            if (a - b).abs() < 0.01 {
                close += 1;
            }
        }
        assert!(
            close < 50,
            "{close} of 1000 consecutive pairs landed within 1% of each other — \
             about 20 is what independence looks like, and a ramp would give 1000"
        );
    }

    /// Two seeds are two different games.
    #[test]
    fn a_different_seed_is_a_different_sequence() {
        let same = (0..1_000)
            .filter(|&i| hash_u64(1, i) == hash_u64(2, i))
            .count();
        assert_eq!(same, 0, "{same} indices collided between two seeds");
    }

    /// A salted seed differs from the original, and each run differs from the
    /// last — but run zero is the seed itself, so a first game plays the hand
    /// its seed names.
    #[test]
    fn salting_deals_a_new_hand_each_run() {
        let seed: u64 = 0xABCD_1234_5678_9EF0;
        assert_eq!(salt(seed, 0), seed, "the first run is the seed as given");

        let mut seen = std::collections::HashSet::new();
        for run in 0..1_000 {
            assert!(seen.insert(salt(seed, run)), "run {run} repeated a seed");
        }
    }
}
