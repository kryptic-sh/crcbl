//! The matchmaking queue: who is waiting, and who gets paired with whom.
//!
//! # The trade-off is the feature
//!
//! Pairing by rating alone leaves anyone at the edge of the population waiting
//! forever, because nobody close enough ever arrives. Pairing by wait time alone
//! matches a beginner against an expert as readily as anything else. Neither is
//! a matchmaker, and the interesting behaviour is the exchange rate between
//! them.
//!
//! So a waiting player carries a **tolerance** that starts at
//! [`BASE_TOLERANCE`] and widens by [`WIDEN_PER_TICK`] every tick they are still
//! waiting, up to [`MAX_TOLERANCE`]. A pair is allowed when the rating gap is
//! inside *both* players' tolerance — the stricter of the two decides, so a
//! player who just joined is never dragged into a bad match by someone who has
//! been waiting a long time.
//!
//! # Why adjacent pairs on a sorted queue
//!
//! Sorting by rating and pairing neighbours is the cheap form of the assignment
//! problem, and for this cost function it is a good one: the total gap of a set
//! of pairs drawn from a line is minimised by pairing adjacent points, so no
//! amount of searching would find a materially better set. It is `O(n log n)`
//! for the sort and linear for the sweep, which is what lets a population of
//! thousands run in CI.
//!
//! # What pairing this tightly costs the ratings
//!
//! Matching people to their nearest equal is what a queue is for, and it puts a
//! ceiling on how good the *ratings* can get. Conditioning on a small observed
//! rating gap preferentially selects pairs whose true skill gap is larger,
//! because a rating is a noisy estimate of skill — so the favourite wins more
//! often than the gap predicted and the ladder's spread inflates. Measured over
//! 64 players: converged by 2000 ticks, at a spread of 978 points against a
//! true skill range of 1000, then stretched to 2689 by 30000.
//!
//! Pairing at random instead removes the drift entirely and pairing wider
//! reduces it, which is the same statement from the other side: the drift is
//! what the match quality costs. It is not fixable here — the correction
//! belongs in the rating update, which has to know how uncertain the two
//! ratings are. See `docs/backlog.md`, "narrow matchmaking stretches an Elo
//! ladder".

use crate::rating::Rating;

/// How far apart two ratings may be for a pair that has just joined.
///
/// A fifth of the scale, so an even match at the start rather than any match.
const BASE_TOLERANCE: f64 = 80.0;

/// How much further apart they may be for every tick spent waiting.
const WIDEN_PER_TICK: f64 = 12.0;

/// The widest the tolerance ever grows.
///
/// Past this a match stops being a match; a player at the edge of the
/// population waits rather than being handed something meaningless.
const MAX_TOLERANCE: f64 = 600.0;

/// Who a queue entry belongs to.
///
/// A plain index into whatever the caller's player table is; the queue never
/// looks inside one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlayerId(pub u32);

/// One player waiting to be matched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    /// Who is waiting.
    pub player: PlayerId,
    /// What they are rated.
    pub rating: Rating,
    /// How many ticks they have been in the queue.
    pub waited: u32,
}

impl Entry {
    /// How far from this player's rating a match may currently be.
    #[must_use]
    pub fn tolerance(&self) -> f64 {
        (BASE_TOLERANCE + f64::from(self.waited) * WIDEN_PER_TICK).min(MAX_TOLERANCE)
    }
}

/// A pairing the queue produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pairing {
    /// One side.
    pub a: PlayerId,
    /// The other.
    pub b: PlayerId,
    /// How far apart the two ratings were, in points — the quality half of the
    /// trade-off, recorded so a caller can show it rather than assert it.
    pub gap: f64,
    /// The longer of the two waits, in ticks — the other half.
    pub waited: u32,
}

/// Everyone currently waiting.
#[derive(Clone, Debug, Default)]
pub struct Queue {
    entries: Vec<Entry>,
}

impl Queue {
    /// An empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Put a player in, or refresh their rating if they are already in.
    ///
    /// Joining twice does not queue twice — a double-click must not be able to
    /// put one player into two matches.
    pub fn join(&mut self, player: PlayerId, rating: Rating) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.player == player) {
            existing.rating = rating;
            return;
        }
        self.entries.push(Entry {
            player,
            rating,
            waited: 0,
        });
    }

    /// Take a player out. Returns whether they were in.
    pub fn leave(&mut self, player: PlayerId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.player != player);
        self.entries.len() != before
    }

    /// Whether a player is waiting.
    #[must_use]
    pub fn contains(&self, player: PlayerId) -> bool {
        self.entries.iter().any(|e| e.player == player)
    }

    /// How many are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nobody is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Everyone waiting, in join order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Age every waiting player by one tick, then pair whoever can be paired.
    ///
    /// Matched players are removed; everyone else stays in with a wider
    /// tolerance for next time.
    pub fn tick(&mut self) -> Vec<Pairing> {
        for entry in &mut self.entries {
            entry.waited = entry.waited.saturating_add(1);
        }

        // Sorted by rating, with the player id breaking ties so the result does
        // not depend on the order players happened to join in — two runs of the
        // same population must pair the same way.
        let mut order: Vec<Entry> = self.entries.clone();
        order.sort_by(|left, right| {
            left.rating
                .points()
                .partial_cmp(&right.rating.points())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.player.cmp(&right.player))
        });

        let mut pairings = Vec::new();
        let mut matched = Vec::new();
        let mut index = 0;
        while index + 1 < order.len() {
            let (left, right) = (order[index], order[index + 1]);
            let gap = (right.rating.points() - left.rating.points()).abs();
            // The stricter tolerance decides, so nobody is pulled into a match
            // wider than they themselves have waited for.
            if gap <= left.tolerance().min(right.tolerance()) {
                pairings.push(Pairing {
                    a: left.player,
                    b: right.player,
                    gap,
                    waited: left.waited.max(right.waited),
                });
                matched.push(left.player);
                matched.push(right.player);
                // Both are spoken for, so the next candidate pair starts past
                // them rather than reusing `right`.
                index += 2;
            } else {
                index += 1;
            }
        }

        self.entries
            .retain(|entry| !matched.contains(&entry.player));
        pairings
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::{Outcome, settle};
    use crcbl::core::rand::hash_unit;

    /// A rating near `points`, reached by playing the result that gets there.
    ///
    /// The rating type has no constructor taking arbitrary points — see
    /// `rating.rs` — so a queue test that wants a spread has to earn one. Two
    /// hundred settled wins or losses against the starting rating is plenty to
    /// cover the range these tests use.
    fn near(points: f64) -> Rating {
        let mut rating = Rating::provisional();
        for _ in 0..200 {
            let outcome = if rating.points() < points {
                Outcome::Win
            } else {
                Outcome::Loss
            };
            (rating, _) = settle(rating, Rating::provisional(), outcome);
        }
        rating
    }

    #[test]
    fn a_player_joins_leaves_and_is_not_queued_twice() {
        let mut queue = Queue::new();
        let player = PlayerId(7);
        assert!(!queue.contains(player));

        queue.join(player, Rating::provisional());
        queue.join(player, Rating::provisional());
        assert_eq!(queue.len(), 1, "joining twice queued twice");
        assert!(queue.contains(player));

        assert!(queue.leave(player));
        assert!(queue.is_empty());
        assert!(
            !queue.leave(player),
            "leaving twice reported a second removal"
        );
    }

    #[test]
    fn two_evenly_matched_players_pair_on_the_first_tick() {
        let mut queue = Queue::new();
        queue.join(PlayerId(1), Rating::provisional());
        queue.join(PlayerId(2), Rating::provisional());

        let pairings = queue.tick();
        assert_eq!(pairings.len(), 1, "{pairings:?}");
        assert_eq!(pairings[0].gap, 0.0);
        assert!(queue.is_empty(), "a matched player stayed in the queue");
    }

    #[test]
    fn a_distant_pair_waits_and_then_matches() {
        let mut queue = Queue::new();
        queue.join(PlayerId(1), near(1200.0));
        queue.join(PlayerId(2), near(1800.0));

        // Far enough apart that the opening tolerance refuses them.
        assert!(queue.tick().is_empty(), "matched immediately");

        let mut ticks = 1;
        while queue.tick().is_empty() {
            ticks += 1;
            assert!(ticks < 1_000, "never matched, even at the widest tolerance");
        }
        assert_eq!(queue.len(), 0);
        // The point of the widening: it took time, and it happened.
        assert!(ticks > 1, "the wait bought nothing");
    }

    #[test]
    fn a_gap_wider_than_the_cap_is_never_matched() {
        let (low, high) = (near(500.0), near(2500.0));
        let gap = high.points() - low.points();
        assert!(
            gap > MAX_TOLERANCE,
            "this test needs a pair the cap actually refuses, and {gap} is not one"
        );

        let mut queue = Queue::new();
        queue.join(PlayerId(1), low);
        queue.join(PlayerId(2), high);

        // Long past the point where the tolerance has stopped widening.
        for tick in 0..500 {
            assert!(
                queue.tick().is_empty(),
                "tick {tick} matched a pair {gap} apart, wider than the cap"
            );
        }
        assert_eq!(queue.len(), 2, "both should still be waiting");

        // And the cap really is what stopped it, rather than the widening
        // simply not having got there yet.
        let entry = Entry {
            player: PlayerId(1),
            rating: low,
            waited: u32::MAX,
        };
        assert_eq!(entry.tolerance(), MAX_TOLERANCE);
    }

    #[test]
    fn a_fresh_player_is_not_dragged_into_a_long_waiter_s_wide_match() {
        // The stricter tolerance has to decide. Someone who has waited a long
        // time will accept a wide gap; someone who just joined will not, and
        // pairing on the *wider* of the two would hand the newcomer a match
        // they never waited for.
        let (low, high) = (near(1400.0), near(1650.0));
        let gap = high.points() - low.points();

        let mut queue = Queue::new();
        queue.join(PlayerId(1), low);

        // Wait alone until this gap is inside the waiter's tolerance.
        let mut waited = 0;
        while queue.entries()[0].tolerance() < gap {
            assert!(queue.tick().is_empty(), "matched with nobody to match");
            waited += 1;
            assert!(waited < 1_000, "the tolerance never reached {gap}");
        }
        assert!(
            waited > 0,
            "the gap was inside the opening tolerance already"
        );

        // The newcomer's own tolerance is nowhere near it yet.
        queue.join(PlayerId(2), high);
        assert!(
            queue.tick().is_empty(),
            "a player on their first tick was matched {gap} away"
        );

        // And once they have waited for it themselves, it goes through — so the
        // refusal above was the newcomer's tolerance, not a queue that never
        // matches this pair at all.
        let mut more = 0;
        while queue.tick().is_empty() {
            more += 1;
            assert!(more < 1_000, "never matched even once both had waited");
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn tolerance_widens_with_waiting() {
        let fresh = Entry {
            player: PlayerId(1),
            rating: Rating::provisional(),
            waited: 0,
        };
        let waited = Entry { waited: 5, ..fresh };
        assert_eq!(fresh.tolerance(), BASE_TOLERANCE);
        assert!(
            waited.tolerance() > fresh.tolerance(),
            "waiting bought nothing: {} vs {}",
            waited.tolerance(),
            fresh.tolerance()
        );
    }

    #[test]
    fn nobody_is_matched_twice_or_against_themselves() {
        let mut queue = Queue::new();
        for id in 0..64u32 {
            queue.join(PlayerId(id), near(1000.0 + f64::from(id) * 15.0));
        }

        let mut seen: Vec<PlayerId> = Vec::new();
        for _ in 0..40 {
            for pairing in queue.tick() {
                assert_ne!(pairing.a, pairing.b, "a player matched themselves");
                assert!(
                    !seen.contains(&pairing.a) && !seen.contains(&pairing.b),
                    "{pairing:?} reused a player already matched"
                );
                seen.push(pairing.a);
                seen.push(pairing.b);
            }
        }
        assert!(
            !seen.is_empty(),
            "nothing matched at all, so nothing was checked"
        );
    }

    #[test]
    fn every_pairing_is_inside_both_players_tolerance() {
        let mut queue = Queue::new();
        let mut ratings = std::collections::HashMap::new();
        for id in 0..48u32 {
            // A spread with gaps in it, so some pairs have to wait.
            let rating = near(1000.0 + hash_unit(0x9E3D, u64::from(id)) * 900.0);
            ratings.insert(PlayerId(id), rating);
            queue.join(PlayerId(id), rating);
        }

        let mut checked = 0;
        for _ in 0..60 {
            // Read the waits before `tick` consumes them.
            let waits: std::collections::HashMap<PlayerId, u32> = queue
                .entries()
                .iter()
                .map(|entry| (entry.player, entry.waited + 1))
                .collect();
            for pairing in queue.tick() {
                let gap = (ratings[&pairing.a].points() - ratings[&pairing.b].points()).abs();
                assert!(
                    (gap - pairing.gap).abs() < 1.0e-9,
                    "{pairing:?} reported a gap of {} but the ratings differ by {gap}",
                    pairing.gap
                );
                for side in [pairing.a, pairing.b] {
                    let allowed = Entry {
                        player: side,
                        rating: ratings[&side],
                        waited: waits[&side],
                    }
                    .tolerance();
                    assert!(
                        gap <= allowed,
                        "{pairing:?}: gap {gap} exceeds {side:?}'s tolerance {allowed}"
                    );
                }
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no pairing was produced, so nothing was checked"
        );
    }

    #[test]
    fn the_same_population_pairs_the_same_way_whatever_order_it_joined_in() {
        let ratings: Vec<(PlayerId, Rating)> = (0..32u32)
            .map(|id| {
                (
                    PlayerId(id),
                    near(1000.0 + hash_unit(0x51D5, u64::from(id)) * 800.0),
                )
            })
            .collect();

        let mut forwards = Queue::new();
        for (player, rating) in &ratings {
            forwards.join(*player, *rating);
        }
        let mut backwards = Queue::new();
        for (player, rating) in ratings.iter().rev() {
            backwards.join(*player, *rating);
        }

        for round in 0..20 {
            let mut left = forwards.tick();
            let mut right = backwards.tick();
            left.sort_by_key(|p| (p.a, p.b));
            right.sort_by_key(|p| (p.a, p.b));
            assert_eq!(left, right, "round {round} disagreed on the pairings");
        }
    }

    #[test]
    fn an_odd_queue_leaves_exactly_one_waiting() {
        let mut queue = Queue::new();
        for id in 0..5u32 {
            queue.join(PlayerId(id), Rating::provisional());
        }
        let pairings = queue.tick();
        assert_eq!(pairings.len(), 2, "{pairings:?}");
        assert_eq!(queue.len(), 1, "{:?}", queue.entries());
    }
}
