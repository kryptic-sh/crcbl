//! Extra substeps per group: decision 1's "a long chain or bridge gets more
//! substeps for its group, not more iterations".
//!
//! ```text
//!   each awake dynamic body asks for its substeps: the system's, or more
//!   (`PhysicsSystem::set_substeps`)
//!   a group is a connected set of them under this tick's constraints —
//!   contacts and joints; static and kinematic bodies join none, as they
//!   join no island
//!   a group runs the most substeps any of its bodies asked for
//!   the solve runs one pass per substep count: the groups asking for more
//!   first, each alone over the whole tick, then everything else
//! ```
//!
//! Nothing is grouped until a body asks, so a system where none has solves
//! exactly as it did before groups. Groups are built afresh each tick by
//! union-find over the prepared constraints, in their order, rather than read
//! off the islands: a pair of bodies can touch before their islands learn of
//! it (a body turned dynamic while touching begins nothing), and a group must
//! never split a constraint between two passes.
//!
//! **A group's constraints are stiffer in proportion.** A group of `n`
//! substeps where the system runs `N` runs its contacts at
//! [`super::ContactSettings::contact_hertz`] `× n / N` and its joints' rigid
//! rows at their stiffness `× n / N`, each still capped at a quarter of the
//! substep rate — so every constraint keeps its stiffness per substep, and a
//! group with more substeps both converges further and holds harder. More
//! substeps at the same stiffness would not stand a tall column: a soft
//! contact's stiffness, not its iterations, is what buckles one (see
//! [`super::ContactSettings::TALL_STACK`]).
//!
//! **A kinematic body touching a group** steps in the last pass, with
//! everything else, so while an earlier pass solves the group it is taken to
//! move at its velocity — which is exactly how a kinematic body moves.

use std::ops::Range;

use super::solver::NONE;

/// One pass of the solve.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Pass {
    /// Its substeps over the tick.
    pub(super) substeps: u32,
    /// The awake bodies it integrates, by awake index.
    pub(super) bodies: Vec<usize>,
    /// Its contacts, as a range of the reordered constraints.
    pub(super) contacts: Range<usize>,
    /// Its joints, likewise.
    pub(super) joints: Range<usize>,
    /// Awake bodies its constraints touch that another pass integrates: the
    /// kinematic bodies it must take to be coasting.
    pub(super) outside: Vec<usize>,
}

/// Each awake body's substeps this tick, once the groups are joined.
#[derive(Debug)]
pub(super) struct Plan {
    counts: Vec<u32>,
    default: u32,
}

/// Union-find with path halving: the root of `k`.
fn find(parent: &mut [usize], mut k: usize) -> usize {
    while parent[k] != k {
        parent[k] = parent[parent[k]];
        k = parent[k];
    }
    k
}

impl Plan {
    /// Groups the awake bodies under `edges` — each constraint's awake
    /// indices — and gives each the most substeps any of its group asked
    /// for. `requested` is each body's own ask, never below `default`, and
    /// only a `dynamic` body joins a group. `None` if no body asked for more
    /// than `default`: there is nothing to group.
    pub(super) fn new(
        requested: &[u32],
        dynamic: &[bool],
        default: u32,
        edges: impl Iterator<Item = (usize, usize)>,
    ) -> Option<Self> {
        if requested.iter().all(|&n| n <= default) {
            return None;
        }
        let mut parent: Vec<usize> = (0..requested.len()).collect();
        let joins = |k: usize| k != NONE && dynamic[k];
        for (a, b) in edges {
            if joins(a) && joins(b) {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let mut most = vec![default; requested.len()];
        for k in 0..requested.len() {
            if dynamic[k] {
                let root = find(&mut parent, k);
                most[root] = most[root].max(requested[k]);
            }
        }
        let counts = (0..requested.len())
            .map(|k| {
                if dynamic[k] {
                    most[find(&mut parent, k)]
                } else {
                    default
                }
            })
            .collect();
        Some(Self { counts, default })
    }

    /// The substeps of the constraint between awake indices `a` and `b`:
    /// its dynamic side's group's.
    fn count_of(&self, a: usize, b: usize, dynamic: &[bool]) -> u32 {
        if a != NONE && dynamic[a] {
            self.counts[a]
        } else if b != NONE {
            self.counts[b]
        } else {
            self.default
        }
    }

    /// The order passes run in, as a sort key: the groups asking for more
    /// first, fewest substeps first, and the rest last.
    pub(super) fn rank(&self, a: usize, b: usize, dynamic: &[bool]) -> u32 {
        match self.count_of(a, b, dynamic) {
            n if n == self.default => u32::MAX,
            n => n,
        }
    }

    /// The passes, given the constraints' and joints' sides **already
    /// sorted by [`rank`](Self::rank)**.
    pub(super) fn passes(
        &self,
        contacts: &[(usize, usize)],
        joints: &[(usize, usize)],
        dynamic: &[bool],
    ) -> Vec<Pass> {
        let mut counts: Vec<u32> = self.counts.clone();
        counts.push(self.default);
        counts.sort_unstable();
        counts.dedup();
        // The default last.
        counts.retain(|&n| n != self.default);
        counts.push(self.default);

        let range = |sides: &[(usize, usize)], n: u32| {
            let start = sides
                .iter()
                .position(|&(a, b)| self.count_of(a, b, dynamic) == n)
                .unwrap_or(sides.len());
            let len = sides[start..]
                .iter()
                .take_while(|&&(a, b)| self.count_of(a, b, dynamic) == n)
                .count();
            start..start + len
        };
        counts
            .into_iter()
            .map(|n| {
                let contacts_range = range(contacts, n);
                let joints_range = range(joints, n);
                let bodies: Vec<usize> = (0..self.counts.len())
                    .filter(|&k| self.counts[k] == n)
                    .collect();
                let mut outside: Vec<usize> = contacts[contacts_range.clone()]
                    .iter()
                    .chain(&joints[joints_range.clone()])
                    .flat_map(|&(a, b)| [a, b])
                    .filter(|&k| k != NONE && self.counts[k] != n)
                    .collect();
                outside.sort_unstable();
                outside.dedup();
                Pass {
                    substeps: n,
                    bodies,
                    contacts: contacts_range,
                    joints: joints_range,
                    outside,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing asks for more, so nothing is grouped.
    #[test]
    fn no_asks_is_no_plan() {
        let plan = Plan::new(&[4, 4, 4], &[true, true, false], 4, [(0, 1)].into_iter());
        assert!(plan.is_none());
    }

    /// One body asking for eight takes the bodies it is joined to with it,
    /// but not across a kinematic body, and the kinematic body it touches
    /// coasts through the group's pass.
    #[test]
    fn a_group_takes_its_neighbours_and_stops_at_a_kinematic_body() {
        // 0 — 1 — 2 (kinematic) — 3, and 4 alone.
        let dynamic = [true, true, false, true, true];
        let edges = [(0, 1), (1, 2), (2, 3)];
        let plan = Plan::new(&[4, 8, 4, 4, 4], &dynamic, 4, edges.into_iter()).expect("an ask");
        let mut sides = edges.to_vec();
        sides.sort_by_key(|&(a, b)| plan.rank(a, b, &dynamic));
        assert_eq!(sides, [(0, 1), (1, 2), (2, 3)]);
        let passes = plan.passes(&sides, &[], &dynamic);
        assert_eq!(passes.len(), 2);
        assert_eq!(passes[0].substeps, 8);
        assert_eq!(passes[0].bodies, [0, 1]);
        assert_eq!(passes[0].contacts, 0..2);
        assert_eq!(passes[0].outside, [2]);
        assert_eq!(passes[1].substeps, 4);
        assert_eq!(passes[1].bodies, [2, 3, 4]);
        assert_eq!(passes[1].contacts, 2..3);
        assert!(passes[1].outside.is_empty());
    }
}
