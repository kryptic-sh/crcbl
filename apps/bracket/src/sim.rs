//! A synthetic population queueing, matching, playing and being re-rated.
//!
//! This is the whole sample's model. The native front end runs it headless to
//! soak a population of thousands; the demo runs the same type a tick at a time
//! and draws it. Nothing here knows which.
//!
//! # Determinism
//!
//! Every decision — who queues this tick, who wins — comes from
//! [`crcbl::core::rand::hash_unit`] over the seed and a counter, so a
//! run is reproducible from its seed on any platform. There is no clock and no
//! thread-local state; ticking the same seed twice gives the same ladder.

use crcbl::core::rand::{hash_unit, salt};

use crate::queue::{PlayerId, Queue};
use crate::rating::{Outcome, Rating, resolve, settle};

/// The lowest true skill in a generated population.
const SKILL_FLOOR: f64 = 1000.0;

/// The highest.
const SKILL_CEILING: f64 = 2000.0;

/// The chance an idle player joins the queue on any given tick.
///
/// Low enough that the queue holds a mix of fresh and waiting players — which
/// is the state the tolerance widening exists for — rather than everyone
/// arriving at once and pairing off perfectly.
const JOIN_CHANCE: f64 = 0.25;

/// How many finished matches the sim keeps for display.
const RECENT_MATCHES: usize = 8;

/// How often the convergence history takes a reading, in ticks.
///
/// Often enough that the early fall is drawn as a curve rather than a step, and
/// rarely enough that the window below covers a run long enough to show the
/// ratings settling.
const HISTORY_EVERY: u64 = 4;

/// How many readings the convergence history keeps.
///
/// One per horizontal pixel of a reasonably sized plot, so the curve is drawn
/// from its readings rather than resampled.
const HISTORY_LEN: usize = 240;

/// One player in the population.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    /// What they are actually worth, which the rating is trying to discover.
    ///
    /// Never shown to the matchmaker — only [`resolve`] reads it.
    pub skill: f64,
    /// What the system currently believes they are worth.
    pub rating: Rating,
    /// Whether they are queued or in a match right now.
    pub busy: bool,
}

/// A finished match, kept for display.
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// One side.
    pub a: PlayerId,
    /// The other.
    pub b: PlayerId,
    /// Told from `a`'s side.
    pub outcome: Outcome,
    /// How far apart the two ratings were when they were paired.
    pub gap: f64,
    /// How long the longer-waiting of the two had waited, in ticks.
    pub waited: u32,
    /// What the result did to `a`'s rating.
    pub delta_a: f64,
}

/// The population, the queue and the tick counter.
#[derive(Clone, Debug)]
pub struct Sim {
    seed: u64,
    tick: u64,
    players: Vec<Player>,
    queue: Queue,
    matches_played: u64,
    recent: Vec<Report>,
    total_gap: f64,
    total_wait: u64,
    history: Vec<f32>,
}

impl Sim {
    /// A population of `players` drawn evenly across the skill range.
    ///
    /// Evenly rather than randomly, so a small population still covers the range
    /// and a convergence claim is not at the mercy of a clumped draw.
    #[must_use]
    pub fn new(seed: u64, players: usize) -> Self {
        let count = players.max(2);
        let population = (0..count)
            .map(|index| {
                let across = index as f64 / (count - 1) as f64;
                Player {
                    skill: SKILL_FLOOR + across * (SKILL_CEILING - SKILL_FLOOR),
                    rating: Rating::provisional(),
                    busy: false,
                }
            })
            .collect();
        Self {
            seed,
            tick: 0,
            players: population,
            queue: Queue::new(),
            matches_played: 0,
            recent: Vec::new(),
            total_gap: 0.0,
            total_wait: 0,
            history: Vec::new(),
        }
    }

    /// The population.
    #[must_use]
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// Who is waiting right now.
    #[must_use]
    pub const fn queue(&self) -> &Queue {
        &self.queue
    }

    /// How many ticks have run.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    /// How many matches have been played and rated.
    #[must_use]
    pub const fn matches_played(&self) -> u64 {
        self.matches_played
    }

    /// The most recent finished matches, newest first.
    #[must_use]
    pub fn recent(&self) -> &[Report] {
        &self.recent
    }

    /// The mean rating gap across every match played, in points.
    ///
    /// The quality half of the trade-off the matchmaker makes.
    #[must_use]
    pub fn mean_gap(&self) -> f64 {
        if self.matches_played == 0 {
            return 0.0;
        }
        self.total_gap / self.matches_played as f64
    }

    /// The mean wait before a match, in ticks — the other half.
    #[must_use]
    pub fn mean_wait(&self) -> f64 {
        if self.matches_played == 0 {
            return 0.0;
        }
        self.total_wait as f64 / self.matches_played as f64
    }

    /// How far the ratings currently are from the true skills, on average.
    ///
    /// The number the whole sample exists to drive down, and the one a rating
    /// system that could not be falsified would never have to report.
    #[must_use]
    pub fn mean_rating_error(&self) -> f64 {
        if self.players.is_empty() {
            return 0.0;
        }
        let total: f64 = self
            .players
            .iter()
            .map(|player| (player.rating.points() - player.skill).abs())
            .sum();
        total / self.players.len() as f64
    }

    /// How far the ratings have been from the true skills over time, oldest
    /// first, in points.
    ///
    /// The sample's whole claim drawn as a curve: it should fall and then stay
    /// down. It does not stay down forever — see this crate's `queue` module.
    #[must_use]
    pub fn history(&self) -> &[f32] {
        &self.history
    }

    /// The ladder: every player's index, strongest rating first.
    #[must_use]
    pub fn ladder(&self) -> Vec<PlayerId> {
        let mut order: Vec<PlayerId> = (0..self.players.len())
            .map(|index| PlayerId(index as u32))
            .collect();
        order.sort_by(|left, right| {
            let (a, b) = (self.player(*left), self.player(*right));
            b.rating
                .points()
                .partial_cmp(&a.rating.points())
                .unwrap_or(std::cmp::Ordering::Equal)
                // Ties break by id so the ladder does not shuffle between
                // frames that changed nothing.
                .then(left.cmp(right))
        });
        order
    }

    /// One player. Panics if `player` is not in this population, which cannot
    /// happen for an id this type produced.
    #[must_use]
    pub fn player(&self, player: PlayerId) -> &Player {
        &self.players[player.0 as usize]
    }

    /// Put an idle player in the queue, if they are not already busy.
    ///
    /// What the demo's "queue up" button calls.
    pub fn enqueue(&mut self, player: PlayerId) {
        let index = player.0 as usize;
        if index >= self.players.len() || self.players[index].busy {
            return;
        }
        self.players[index].busy = true;
        self.queue.join(player, self.players[index].rating);
    }

    /// One tick: idle players decide whether to queue, then the queue pairs,
    /// then every pair plays and is re-rated.
    pub fn step(&mut self) {
        self.tick = self.tick.saturating_add(1);
        if self.tick.is_multiple_of(HISTORY_EVERY) {
            if self.history.len() == HISTORY_LEN {
                self.history.remove(0);
            }
            self.history.push(self.mean_rating_error() as f32);
        }

        for index in 0..self.players.len() {
            if self.players[index].busy {
                continue;
            }
            let roll = hash_unit(salt(self.seed, self.tick), index as u64);
            if roll < JOIN_CHANCE {
                self.enqueue(PlayerId(index as u32));
            }
        }

        for pairing in self.queue.tick() {
            let (a, b) = (pairing.a.0 as usize, pairing.b.0 as usize);
            let outcome = resolve(
                self.players[a].skill,
                self.players[b].skill,
                salt(self.seed, self.tick),
                self.matches_played,
            );
            let before = self.players[a].rating.points();
            let (next_a, next_b) = settle(self.players[a].rating, self.players[b].rating, outcome);
            self.players[a].rating = next_a;
            self.players[b].rating = next_b;
            self.players[a].busy = false;
            self.players[b].busy = false;

            self.matches_played = self.matches_played.saturating_add(1);
            self.total_gap += pairing.gap;
            self.total_wait = self.total_wait.saturating_add(u64::from(pairing.waited));

            self.recent.insert(
                0,
                Report {
                    a: pairing.a,
                    b: pairing.b,
                    outcome,
                    gap: pairing.gap,
                    waited: pairing.waited,
                    delta_a: next_a.points() - before,
                },
            );
            self.recent.truncate(RECENT_MATCHES);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: u64, players: usize, ticks: u64) -> Sim {
        let mut sim = Sim::new(seed, players);
        for _ in 0..ticks {
            sim.step();
        }
        sim
    }

    #[test]
    fn the_same_seed_runs_the_same_population() {
        let left = run(0x5EED, 32, 500);
        let right = run(0x5EED, 32, 500);
        assert_eq!(left.ladder(), right.ladder());
        assert_eq!(left.matches_played(), right.matches_played());
        for (a, b) in left.players().iter().zip(right.players()) {
            assert_eq!(a.rating, b.rating);
        }
    }

    #[test]
    fn a_different_seed_runs_a_different_one() {
        let left = run(1, 32, 500);
        let right = run(2, 32, 500);
        let same = left
            .players()
            .iter()
            .zip(right.players())
            .filter(|(a, b)| a.rating == b.rating)
            .count();
        assert!(
            same < left.players().len(),
            "two seeds produced identical ratings for every player"
        );
    }

    #[test]
    fn nobody_is_in_two_places_at_once() {
        let mut sim = Sim::new(0xB0B, 40);
        for tick in 0..300 {
            sim.step();
            // A queued player is busy, and a busy player is either queued or
            // was just matched and released within the same step.
            for entry in sim.queue().entries() {
                assert!(
                    sim.player(entry.player).busy,
                    "tick {tick}: {:?} is queued without being busy",
                    entry.player
                );
            }
            let queued = sim.queue().len();
            let busy = sim.players().iter().filter(|p| p.busy).count();
            assert_eq!(busy, queued, "tick {tick}: {busy} busy but {queued} queued");
        }
    }

    #[test]
    fn queueing_a_busy_player_again_changes_nothing() {
        let mut sim = Sim::new(7, 8);
        sim.enqueue(PlayerId(0));
        let before = sim.queue().len();
        sim.enqueue(PlayerId(0));
        assert_eq!(sim.queue().len(), before);
        // And an id outside the population is ignored rather than panicking.
        sim.enqueue(PlayerId(9_999));
        assert_eq!(sim.queue().len(), before);
    }

    #[test]
    fn a_fresh_population_has_played_nothing() {
        let sim = Sim::new(1, 16);
        assert_eq!(sim.matches_played(), 0);
        assert_eq!(sim.mean_gap(), 0.0);
        assert_eq!(sim.mean_wait(), 0.0);
        assert!(sim.recent().is_empty());
        assert_eq!(sim.tick_count(), 0);
    }

    /// How many ticks a population is given to find its true skills.
    ///
    /// Measured across five seeds at 64 players: mean error 54.4..57.7 points
    /// against a starting error of 254, so this window is where convergence is
    /// claimed and checked. It does **not** hold indefinitely — see
    /// `docs/backlog.md`, "narrow matchmaking stretches an Elo ladder".
    const CONVERGENCE_TICKS: u64 = 2_000;

    #[test]
    fn a_population_finds_its_true_skills() {
        let players = 64;
        let start_error = Sim::new(1, players).mean_rating_error();
        assert!(
            start_error > 200.0,
            "a population that starts this close ({start_error:.1}) would prove nothing"
        );

        for seed in [0xB2ACu64, 1, 7, 99, 12_345] {
            let sim = run(seed, players, CONVERGENCE_TICKS);
            assert!(
                sim.matches_played() > 10_000,
                "seed {seed}: only {} matches were played",
                sim.matches_played()
            );
            let error = sim.mean_rating_error();
            assert!(
                error < 90.0,
                "seed {seed}: ratings sat {error:.1} points from the true skills"
            );
            assert!(
                error < start_error / 2.0,
                "seed {seed}: error {error:.1} barely improved on {start_error:.1}"
            );
        }
    }

    #[test]
    fn the_matchmaker_reports_the_trade_off_it_made() {
        let sim = run(0xB2AC, 64, CONVERGENCE_TICKS);
        // Both halves have to be real numbers, or the panel showing them is
        // showing a placeholder.
        assert!(sim.mean_gap() > 0.0, "every match was a perfect pairing?");
        assert!(
            sim.mean_gap() < 100.0,
            "mean gap {:.1} is wider than the queue should ever allow at rest",
            sim.mean_gap()
        );
        assert!(
            sim.mean_wait() >= 1.0,
            "matches happened before anyone waited"
        );
        assert!(
            !sim.recent().is_empty(),
            "no match was recorded for display"
        );
        assert_eq!(sim.recent().len(), RECENT_MATCHES);
    }
}
