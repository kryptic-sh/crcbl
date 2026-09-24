//! Beam-first spatial scoring: which of a set of rectangles a directional move
//! from one rectangle lands on.
//!
//! The UI's spatial-navigation rule (`docs/notes/tooling.md`), revised 2026-09-15. Three
//! steps, for a move from `from` in a direction:
//!
//! 1. **A candidate lies in the direction** when, measured along the move's
//!    axis, its leading edge is past `from`'s leading edge (or at or past its
//!    trailing edge) and its trailing edge is past `from`'s trailing edge — the
//!    test Android's `FocusFinder.isCandidate` makes, so a rectangle that
//!    overlaps `from` still counts when it reaches further.
//! 2. **A candidate in the beam beats every candidate outside it.** The beam is
//!    the band `from` projects in the direction: a candidate is in it when its
//!    extent across the move overlaps `from`'s. Godot 4.4's regression — a plain
//!    gap-plus-misalignment score let a taller neighbour take `ui_down` from the
//!    aligned one below — is what the rule exists for.
//! 3. **Within each group the lower distance wins**, and on a tie the earlier
//!    candidate in the order given, which the tree passes in tree order:
//!
//!    ```text
//!    distance = 13 · major² + minor²
//!    major    = the gap from from's trailing edge to the candidate's leading
//!               edge along the move, floored at zero
//!    minor    = the distance between the two centres across the move
//!    ```
//!
//!    The weighting is Android's `FocusFinder.getWeightedDistanceFor`.

use glam::Vec2;

use super::Direction;

/// A rectangle as `(min, max)`, in screen pixels.
pub(crate) type Rect = (Vec2, Vec2);

/// How much more a pixel along the move counts than a pixel across it:
/// Android's `FocusFinder` weighting.
pub const MAJOR_AXIS_WEIGHT: f32 = 13.0;

/// A candidate's standing for one move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    /// Whether it overlaps the band the current rectangle projects.
    pub in_beam: bool,
    /// `13 · major² + minor²`; see the module docs.
    pub distance: f32,
}

impl Score {
    /// Whether `self` wins over `other`: the beam first, then the distance.
    /// A tie is not a win, so the first of two equal candidates keeps its
    /// place.
    #[must_use]
    pub fn beats(self, other: Self) -> bool {
        match (self.in_beam, other.in_beam) {
            (true, false) => true,
            (false, true) => false,
            _ => self.distance < other.distance,
        }
    }
}

/// `rect` measured along a move in `direction`, turned so the move runs toward
/// positive major coordinates: `(major min, major max, cross min, cross max)`.
fn project((min, max): Rect, direction: Direction) -> (f32, f32, f32, f32) {
    match direction {
        Direction::Right => (min.x, max.x, min.y, max.y),
        Direction::Left => (-max.x, -min.x, min.y, max.y),
        Direction::Down => (min.y, max.y, min.x, max.x),
        Direction::Up => (-max.y, -min.y, min.x, max.x),
    }
}

/// How `to` stands for a move from `from` in `direction`, or `None` when it
/// does not lie in that direction at all.
#[must_use]
pub fn score(from: Rect, to: Rect, direction: Direction) -> Option<Score> {
    let (from_lead, from_trail, from_cross_min, from_cross_max) = project(from, direction);
    let (to_lead, to_trail, to_cross_min, to_cross_max) = project(to, direction);
    let ahead = (from_lead < to_lead || from_trail <= to_lead) && from_trail < to_trail;
    if !ahead {
        return None;
    }
    let in_beam = to_cross_min < from_cross_max && to_cross_max > from_cross_min;
    let major = (to_lead - from_trail).max(0.0);
    let minor = ((from_cross_min + from_cross_max) - (to_cross_min + to_cross_max)).abs() * 0.5;
    Some(Score {
        in_beam,
        distance: MAJOR_AXIS_WEIGHT * major * major + minor * minor,
    })
}

/// The winner among `candidates` for a move from `from` in `direction`, and
/// every candidate that lies in the direction with its score, in the order
/// given.
pub fn pick<K: Copy>(
    from: Rect,
    candidates: impl IntoIterator<Item = (K, Rect)>,
    direction: Direction,
) -> (Option<K>, Vec<(K, Score)>) {
    let mut best: Option<(K, Score)> = None;
    let mut scored = Vec::new();
    for (key, rect) in candidates {
        let Some(score) = score(from, rect, direction) else {
            continue;
        };
        scored.push((key, score));
        if best.is_none_or(|(_, held)| score.beats(held)) {
            best = Some((key, score));
        }
    }
    (best.map(|(key, _)| key), scored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        (Vec2::new(x, y), Vec2::new(x + width, y + height))
    }

    fn pick_named(
        from: Rect,
        layout: &[(&'static str, Rect)],
        direction: Direction,
    ) -> Option<&'static str> {
        pick(from, layout.iter().copied(), direction).0
    }

    /// A 3 × 3 grid of 40 × 20 cells, 10 apart, named by row and column.
    fn grid() -> Vec<(&'static str, Rect)> {
        const NAMES: [[&str; 3]; 3] = [["a0", "a1", "a2"], ["b0", "b1", "b2"], ["c0", "c1", "c2"]];
        let mut cells = Vec::new();
        for (row, names) in NAMES.iter().enumerate() {
            for (column, name) in names.iter().enumerate() {
                cells.push((
                    *name,
                    rect(column as f32 * 50.0, row as f32 * 30.0, 40.0, 20.0),
                ));
            }
        }
        cells
    }

    /// **In a regular grid every move lands on the adjacent cell, and a move
    /// off an edge finds nothing** — every cell in every direction, against a
    /// table.
    #[test]
    fn a_grid_moves_to_the_adjacent_cell_and_stops_at_its_edges() {
        use Direction::{Down, Left, Right, Up};
        let cells = grid();
        let table: &[(&str, [Option<&str>; 4])] = &[
            //        up          right       down        left
            ("a0", [None, Some("a1"), Some("b0"), None]),
            ("a1", [None, Some("a2"), Some("b1"), Some("a0")]),
            ("a2", [None, None, Some("b2"), Some("a1")]),
            ("b0", [Some("a0"), Some("b1"), Some("c0"), None]),
            ("b1", [Some("a1"), Some("b2"), Some("c1"), Some("b0")]),
            ("b2", [Some("a2"), None, Some("c2"), Some("b1")]),
            ("c0", [Some("b0"), Some("c1"), None, None]),
            ("c1", [Some("b1"), Some("c2"), None, Some("c0")]),
            ("c2", [Some("b2"), None, None, Some("c1")]),
        ];
        for (from, want) in table {
            let origin = cells
                .iter()
                .find(|(name, _)| name == from)
                .expect("named")
                .1;
            let others: Vec<_> = cells
                .iter()
                .copied()
                .filter(|(name, _)| name != from)
                .collect();
            for (direction, want) in [Up, Right, Down, Left].into_iter().zip(want) {
                assert_eq!(
                    pick_named(origin, &others, direction),
                    *want,
                    "{from} moving {direction:?}"
                );
            }
        }
    }

    /// **Godot 4.4's regression shape: a tall neighbour must not steal a
    /// vertical move from an aligned one.** The tall column beside the current
    /// button starts below its top, so it lies downward with no gap at all,
    /// while the aligned button sits 60 pixels below — the gap-first score
    /// picks the column, the beam picks the button.
    #[test]
    fn a_tall_neighbour_does_not_steal_a_vertical_move_from_an_aligned_one() {
        let current = rect(0.0, 0.0, 100.0, 40.0);
        let layout = [
            ("tall", rect(110.0, 20.0, 100.0, 300.0)),
            ("aligned", rect(0.0, 100.0, 100.0, 40.0)),
        ];
        let (_, scored) = pick(current, layout, Direction::Down);
        let tall = scored[0].1;
        let aligned = scored[1].1;
        assert!(
            tall.distance < aligned.distance,
            "the fixture no longer has the shape: the tall column must be the nearer by distance \
             ({tall:?} against {aligned:?})"
        );
        assert!(!tall.in_beam && aligned.in_beam);
        assert_eq!(
            pick_named(current, &layout, Direction::Down),
            Some("aligned")
        );
    }

    /// **The beam beats distance on both axes**: a candidate far along the move
    /// but overlapping the band wins over a near one outside it, and among two
    /// in the beam the nearer wins.
    #[test]
    fn a_candidate_in_the_beam_beats_a_nearer_one_outside_it() {
        let current = rect(100.0, 100.0, 40.0, 20.0);
        type Case = (Direction, [(&'static str, Rect); 3], &'static str);
        let cases: [Case; 4] = [
            (
                Direction::Right,
                [
                    ("near-off", rect(150.0, 125.0, 40.0, 20.0)),
                    ("far-beam", rect(400.0, 110.0, 40.0, 20.0)),
                    ("farther-beam", rect(500.0, 100.0, 40.0, 20.0)),
                ],
                "far-beam",
            ),
            (
                Direction::Left,
                [
                    ("near-off", rect(50.0, 80.0, 40.0, 19.0)),
                    ("farther-beam", rect(-300.0, 100.0, 40.0, 20.0)),
                    ("far-beam", rect(-100.0, 119.0, 40.0, 20.0)),
                ],
                "far-beam",
            ),
            (
                Direction::Down,
                [
                    ("near-off", rect(141.0, 121.0, 40.0, 20.0)),
                    ("far-beam", rect(139.0, 300.0, 40.0, 20.0)),
                    ("farther-beam", rect(100.0, 301.0, 40.0, 20.0)),
                ],
                "far-beam",
            ),
            (
                Direction::Up,
                [
                    ("near-off", rect(40.0, 70.0, 59.0, 20.0)),
                    ("farther-beam", rect(100.0, -200.0, 40.0, 20.0)),
                    ("far-beam", rect(120.0, -100.0, 40.0, 20.0)),
                ],
                "far-beam",
            ),
        ];
        for (direction, layout, want) in cases {
            assert_eq!(
                pick_named(current, &layout, direction),
                Some(want),
                "moving {direction:?}"
            );
        }
    }

    /// **What lies in a direction is decided by both edges**: a rectangle that
    /// contains the current one lies in no direction, one that overlaps it and
    /// reaches further lies in that direction, one level with it lies in none
    /// along the move — and two candidates scoring the same keep the order they
    /// were given in.
    #[test]
    fn overlap_containment_and_ties_are_decided_by_the_edges_and_the_order() {
        let current = rect(100.0, 100.0, 40.0, 20.0);
        let container = rect(0.0, 0.0, 400.0, 400.0);
        for direction in Direction::ALL {
            assert_eq!(score(current, container, direction), None, "{direction:?}");
        }
        let reaching = rect(120.0, 100.0, 40.0, 20.0);
        let overlap = score(current, reaching, Direction::Right).expect("reaches further right");
        assert_eq!(
            overlap,
            Score {
                in_beam: true,
                distance: 0.0
            }
        );
        assert_eq!(score(current, reaching, Direction::Left), None);
        let level = rect(100.0, 200.0, 40.0, 20.0);
        assert_eq!(score(current, level, Direction::Right), None);

        // Mirror images of each other across the move's axis: equal scores.
        let layout = [
            ("above", rect(200.0, 90.0, 40.0, 20.0)),
            ("below", rect(200.0, 110.0, 40.0, 20.0)),
        ];
        assert_eq!(
            pick_named(current, &layout, Direction::Right),
            Some("above")
        );
        let reversed = [layout[1], layout[0]];
        assert_eq!(
            pick_named(current, &reversed, Direction::Right),
            Some("below")
        );
    }

    /// **The distance is the formula the module documents**, at known values:
    /// the major gap weighted by thirteen and squared, plus the squared offset
    /// between the centres.
    #[test]
    fn the_distance_is_thirteen_major_squared_plus_minor_squared() {
        let current = rect(0.0, 0.0, 10.0, 10.0);
        // Gap 20 along x, centres 3 apart along y: 13·400 + 9.
        let to = rect(30.0, 3.0, 10.0, 10.0);
        assert_eq!(
            score(current, to, Direction::Right),
            Some(Score {
                in_beam: true,
                distance: 5209.0
            })
        );
        // Gap 5 along −y, centres 12 apart along x, outside the beam: 13·25 + 144.
        let to = rect(12.0, -15.0, 10.0, 10.0);
        assert_eq!(
            score(current, to, Direction::Up),
            Some(Score {
                in_beam: false,
                distance: 469.0
            })
        );
    }
}
