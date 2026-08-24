//! Elo ratings, and the schedule that decides how fast one moves.
//!
//! # The formula is Elo's, not an approximation of it
//!
//! A player rated `SCALE` points above another is expected to score ten times
//! as often, and the expected score follows from that alone:
//!
//! ```text
//! E = 1 / (1 + 10 ^ ((opponent - rating) / SCALE))
//! ```
//!
//! That relation *is* the definition of the scale, so it is the thing worth
//! asserting rather than a table of values copied from somewhere: at a
//! difference of `SCALE` the expected score is exactly `10/11`, and the tests
//! below pin that.
//!
//! # A rating cannot be built out of range
//!
//! [`Rating`] has no constructor that takes arbitrary points. One starts
//! [`Rating::provisional`] and moves only through [`settle`], whose step is
//! bounded by the K-factor, so a non-finite rating has nowhere to enter from.
//! That is the contract enforced rather than documented — a `f64` parameter
//! would have let a NaN in and it would have spread through every later match.

use crcbl::core::rand::hash_unit;

/// The rating difference at which the stronger player is expected to score ten
/// times as often as the weaker one.
///
/// Elo's defining constant, and the reason the base below is ten.
const SCALE: f64 = 400.0;

/// How many games a player is treated as provisional for.
///
/// Long enough that the fast K below has settled most of the distance to a
/// player's real strength, short enough that it is over well inside a session's
/// worth of matches.
const PROVISIONAL_GAMES: u32 = 30;

/// How far a provisional player's rating moves on a whole win against an equal
/// opponent.
///
/// A new rating starts at [`Rating::START`] and knows nothing, so it should
/// travel; the cost is that it is noisy while it does.
const K_PROVISIONAL: f64 = 40.0;

/// How far an established player's rating moves, in the same terms.
///
/// The trade the schedule exists to make: a settled rating is worth more when it
/// stops chasing the last result, and halving the step halves the noise it
/// carries at rest.
const K_ESTABLISHED: f64 = 20.0;

/// What one player scored in one match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The first player won.
    Win,
    /// Neither did.
    Draw,
    /// The second player won.
    Loss,
}

impl Outcome {
    /// The score the *first* player takes from this outcome.
    #[must_use]
    pub const fn score(self) -> f64 {
        match self {
            Self::Win => 1.0,
            Self::Draw => 0.5,
            Self::Loss => 0.0,
        }
    }

    /// The same outcome told from the second player's side.
    #[must_use]
    pub const fn mirrored(self) -> Self {
        match self {
            Self::Win => Self::Loss,
            Self::Draw => Self::Draw,
            Self::Loss => Self::Win,
        }
    }
}

/// A player's rating, and how many rated games it has behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rating {
    points: f64,
    games: u32,
}

impl Rating {
    /// Where a player with no record starts.
    ///
    /// The middle of the range this sample's synthetic populations are drawn
    /// from, so a new player is equally wrong about a strong opponent and a weak
    /// one.
    pub const START: f64 = 1500.0;

    /// A rating with no games behind it.
    #[must_use]
    pub const fn provisional() -> Self {
        Self {
            points: Self::START,
            games: 0,
        }
    }

    /// The rating itself.
    #[must_use]
    pub const fn points(self) -> f64 {
        self.points
    }

    /// How many rated games it has behind it.
    #[must_use]
    pub const fn games(self) -> u32 {
        self.games
    }

    /// Whether this rating is still moving at the provisional step.
    #[must_use]
    pub const fn is_provisional(self) -> bool {
        self.games < PROVISIONAL_GAMES
    }

    /// How far one match can move this rating.
    #[must_use]
    fn k_factor(self) -> f64 {
        if self.is_provisional() {
            K_PROVISIONAL
        } else {
            K_ESTABLISHED
        }
    }
}

impl Default for Rating {
    fn default() -> Self {
        Self::provisional()
    }
}

/// What `rating` is expected to score against `opponent`, in `0.0..=1.0`.
///
/// Reads as a probability for a match that cannot be drawn, and as an expected
/// score for one that can.
#[must_use]
pub fn expected_score(rating: Rating, opponent: Rating) -> f64 {
    expected_from_points(rating.points, opponent.points)
}

/// [`expected_score`] over bare point values, for callers that hold a true skill
/// rather than a rating — the synthetic population's match stub is the one.
#[must_use]
pub fn expected_from_points(points: f64, opponent: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((opponent - points) / SCALE))
}

/// Apply one result, returning both players' new ratings.
///
/// `outcome` is told from `a`'s side. Each player moves by their own K-factor,
/// so a provisional player meeting an established one takes more from the
/// result than they give — which is the point of the schedule and the reason
/// this is not zero-sum in general.
#[must_use]
pub fn settle(a: Rating, b: Rating, outcome: Outcome) -> (Rating, Rating) {
    let expected = expected_score(a, b);
    let a_next = Rating {
        // `expected` is in `0.0..=1.0` and the K-factors are finite constants,
        // so the step is bounded and a finite rating stays finite.
        points: a.points + a.k_factor() * (outcome.score() - expected),
        games: a.games.saturating_add(1),
    };
    let b_next = Rating {
        points: b.points + b.k_factor() * (outcome.mirrored().score() - (1.0 - expected)),
        games: b.games.saturating_add(1),
    };
    (a_next, b_next)
}

/// Resolve one match between players of known true skill.
///
/// The outcome is a seeded roll weighted by the same logistic curve the ratings
/// are built on, so a population's ratings converge on its true skills if and
/// only if the arithmetic in this module is right. It is deliberately *not*
/// decided by whoever is stronger: an outcome that always favoured the higher
/// skill would make convergence trivial and prove nothing.
#[must_use]
pub fn resolve(skill_a: f64, skill_b: f64, seed: u64, index: u64) -> Outcome {
    if hash_unit(seed, index) < expected_from_points(skill_a, skill_b) {
        Outcome::Win
    } else {
        Outcome::Loss
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::core::rand::salt;

    /// A rating at a chosen number of points, for tests that need a spread.
    ///
    /// Deliberately not public: outside this module a rating is only reachable
    /// through [`Rating::provisional`] and [`settle`], which is what keeps a
    /// non-finite one out.
    fn at(points: f64, games: u32) -> Rating {
        Rating { points, games }
    }

    #[test]
    fn equal_ratings_expect_half_a_point() {
        let player = Rating::provisional();
        assert_eq!(expected_score(player, player), 0.5);
    }

    #[test]
    fn the_scale_is_the_ten_to_one_difference() {
        // Elo's defining calibration, written as the literal difference it is
        // defined at rather than as `SCALE`: a player *four hundred* points
        // ahead scores ten times as often, so ten of every eleven points.
        // Phrasing it in terms of the constant would only prove the module
        // agrees with itself, and would pass with the scale set to anything.
        let strong = at(Rating::START + 400.0, 0);
        let weak = at(Rating::START, 0);
        let expected = expected_score(strong, weak);
        assert!(
            (expected - 10.0 / 11.0).abs() < 1.0e-12,
            "expected {expected}, want {}",
            10.0 / 11.0
        );
    }

    #[test]
    fn the_two_sides_of_a_match_expect_a_whole_point_between_them() {
        for delta in [0.0, 1.0, 37.5, 400.0, 1200.0, -250.0] {
            let a = at(Rating::START, 0);
            let b = at(Rating::START + delta, 0);
            let total = expected_score(a, b) + expected_score(b, a);
            assert!(
                (total - 1.0).abs() < 1.0e-12,
                "delta {delta}: total {total}"
            );
        }
    }

    #[test]
    fn a_win_raises_and_a_loss_lowers() {
        let start = Rating::provisional();
        let (winner, loser) = settle(start, start, Outcome::Win);
        assert!(winner.points() > start.points(), "{winner:?}");
        assert!(loser.points() < start.points(), "{loser:?}");
    }

    #[test]
    fn a_draw_between_equals_moves_nothing() {
        let start = Rating::provisional();
        let (a, b) = settle(start, start, Outcome::Draw);
        assert_eq!(a.points(), start.points());
        assert_eq!(b.points(), start.points());
        // The game still counted, which is what moves them off provisional.
        assert_eq!((a.games(), b.games()), (1, 1));
    }

    #[test]
    fn an_upset_moves_more_than_the_expected_result() {
        let favourite = at(Rating::START + SCALE, PROVISIONAL_GAMES);
        let underdog = at(Rating::START, PROVISIONAL_GAMES);

        let (_, upset_winner) = settle(favourite, underdog, Outcome::Loss);
        let (expected_winner, _) = settle(favourite, underdog, Outcome::Win);

        let upset_gain = upset_winner.points() - underdog.points();
        let expected_gain = expected_winner.points() - favourite.points();
        assert!(
            upset_gain > expected_gain,
            "upset gained {upset_gain}, the expected result gained {expected_gain}"
        );
    }

    #[test]
    fn a_provisional_rating_moves_faster_than_a_settled_one() {
        let new = Rating::provisional();
        assert!(new.is_provisional());
        let settled = at(Rating::START, PROVISIONAL_GAMES);
        assert!(!settled.is_provisional());

        let (new_after, _) = settle(new, new, Outcome::Win);
        let (settled_after, _) = settle(settled, settled, Outcome::Win);
        assert!(
            new_after.points() - new.points() > settled_after.points() - settled.points(),
            "provisional {new_after:?} did not outrun settled {settled_after:?}"
        );
    }

    #[test]
    fn a_rating_leaves_provisional_after_its_scheduled_games() {
        let mut player = Rating::provisional();
        let filler = Rating::provisional();
        for _ in 0..PROVISIONAL_GAMES {
            assert!(player.is_provisional(), "left early at {}", player.games());
            (player, _) = settle(player, filler, Outcome::Draw);
        }
        assert!(
            !player.is_provisional(),
            "still provisional at {}",
            player.games()
        );
    }

    /// The population the convergence test runs, and the error it ends with.
    ///
    /// Returns the mean and worst absolute distance between a player's rating
    /// and their true skill, in points.
    fn converge(players: usize, matches: u64, seed: u64) -> (f64, f64) {
        // True skills spread evenly across the range a rating starts in the
        // middle of, so nobody's starting rating is already right.
        let skill: Vec<f64> = (0..players)
            .map(|index| {
                let across = index as f64 / (players - 1) as f64;
                Rating::START - 500.0 + across * 1000.0
            })
            .collect();
        let mut ratings = vec![Rating::provisional(); players];

        for round in 0..matches {
            // Pair at random rather than by rating: this test is about the
            // arithmetic converging, and skill-based pairing would let a good
            // matchmaker hide a bad update rule.
            let a = (hash_unit(seed, round * 2) * players as f64) as usize % players;
            let b = (hash_unit(seed, round * 2 + 1) * players as f64) as usize % players;
            if a == b {
                continue;
            }
            let outcome = resolve(skill[a], skill[b], salt(seed, round), round);
            let (next_a, next_b) = settle(ratings[a], ratings[b], outcome);
            ratings[a] = next_a;
            ratings[b] = next_b;
        }

        let errors: Vec<f64> = ratings
            .iter()
            .zip(&skill)
            .map(|(rating, truth)| (rating.points() - truth).abs())
            .collect();
        let mean = errors.iter().sum::<f64>() / errors.len() as f64;
        let worst = errors.iter().copied().fold(0.0, f64::max);
        (mean, worst)
    }

    /// The mean absolute error a population has before a single match is
    /// played, which is what convergence has to beat.
    fn starting_error(players: usize) -> f64 {
        let total: f64 = (0..players)
            .map(|index| {
                let across = index as f64 / (players - 1) as f64;
                (Rating::START - (Rating::START - 500.0 + across * 1000.0)).abs()
            })
            .sum();
        total / players as f64
    }

    #[test]
    fn ratings_converge_on_true_skill() {
        // Every player starts at `Rating::START` while true skills are spread
        // evenly 500 either side of it, so the error to be closed is a known
        // quantity — and asserting it first is what stops the bounds below
        // passing on a run that never moved a rating at all.
        let start_error = starting_error(64);
        assert!(
            start_error > 200.0,
            "the population barely spreads at all ({start_error:.1}), so closing \
             on it would prove nothing"
        );

        // Measured across populations of 16, 64 and 256 and five seeds each:
        // mean 24.5..39.9, worst 67.8..172.9. The bounds sit above that range
        // and far below the error being closed, so they red on a broken update
        // rule without flaking on the seed.
        const MEAN_ERROR: f64 = 60.0;
        const WORST_ERROR: f64 = 200.0;
        for seed in [0xB2ACu64, 1, 7, 99, 123_456] {
            let (mean, worst) = converge(64, 40_000, seed);
            assert!(
                mean < MEAN_ERROR,
                "seed {seed}: mean error {mean:.1} did not close on the true skills"
            );
            assert!(worst < WORST_ERROR, "seed {seed}: worst error {worst:.1}");
            assert!(
                mean < start_error / 3.0,
                "seed {seed}: mean error {mean:.1} barely improved on {start_error:.1}"
            );
        }
    }
}
