//! [`SpanGrid::plan`]'s and [`SpanGrid::reachable_within`]'s searches against
//! the versions they replaced, kept here verbatim but for taking spans rather
//! than positions: ordered maps for the search state, a `Vec` of neighbours
//! per step, and every feet position found by a binary search for its column.
//! Over seeded grids and span pairs, the routes must match exactly and the
//! spans expanded must come in the same order at the same costs.

use std::collections::BTreeSet;

use super::*;

const CELL_M: f64 = 0.25;

/// SplitMix64 (Steele, Lea and Flood, 2014): the next value of a seeded
/// sequence.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A value below `bound`.
fn below(state: &mut u64, bound: u64) -> u64 {
    splitmix64(state) % bound
}

/// A grid of `width` by `height` columns whose floors are drawn from `seed`:
/// up to three per column, on a 0.2 m lattice when `lattice` (so equal
/// estimates are common and the tie-break decides) and anywhere otherwise,
/// with one column in eight left empty.
fn seeded_grid(seed: u64, width: usize, height: usize, lattice: bool, agent: NavAgent) -> SpanGrid {
    let mut state = seed;
    let columns: Vec<Vec<f64>> = (0..width * height)
        .map(|_| {
            if below(&mut state, 8) == 0 {
                return Vec::new();
            }
            let count = 1 + below(&mut state, 3);
            (0..count)
                .map(|storey| {
                    let rise = if lattice {
                        0.2 * below(&mut state, 4) as f64
                    } else {
                        0.6 * (below(&mut state, 1 << 20) as f64 / f64::from(1 << 20))
                    };
                    2.5 * storey as f64 + rise
                })
                .collect()
        })
        .collect();
    let config = SpanGridConfig {
        origin: DVec2::new(-3.0, 7.0),
        cell_size_m: CELL_M,
        width,
        height,
    };
    SpanGrid::build(config, agent, |at| {
        let local = (at - config.origin) / CELL_M;
        columns[local.y as usize * width + local.x as usize]
            .iter()
            .map(|&y| ProbedFloor {
                y,
                normal_y: 1.0,
                headroom_clear: true,
            })
            .collect()
    })
    .unwrap()
}

/// The open-set entry the old search used.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OldOpen {
    estimate: f64,
    span: SpanId,
}

impl Eq for OldOpen {}

impl Ord for OldOpen {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.span.cmp(&self.span))
    }
}

impl PartialOrd for OldOpen {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The old column lookup: the last column whose range starts at or before
/// `span`.
fn old_column_of(grid: &SpanGrid, span: SpanId) -> usize {
    grid.column_start.partition_point(|&start| start <= span) - 1
}

/// The old feet position: `span`'s cell centre at its floor's height.
fn old_feet_of(grid: &SpanGrid, span: SpanId) -> DVec3 {
    let column = old_column_of(grid, span);
    let at = grid
        .config
        .cell_center(column % grid.config.width, column / grid.config.width);
    DVec3::new(at.x, grid.floors[span], at.y)
}

/// The old neighbour list.
fn old_neighbours(grid: &SpanGrid, span: SpanId) -> Vec<SpanId> {
    let column = old_column_of(grid, span);
    let (x, z) = (column % grid.config.width, column / grid.config.width);
    let y = grid.floors[span];
    let step = |dx: isize, dz: isize| -> Vec<SpanId> {
        let (Some(nx), Some(nz)) = (
            x.checked_add_signed(dx).filter(|&v| v < grid.config.width),
            z.checked_add_signed(dz).filter(|&v| v < grid.config.height),
        ) else {
            return Vec::new();
        };
        grid.spans_of(nz * grid.config.width + nx)
            .filter(|&other| (grid.floors[other] - y).abs() <= grid.agent.max_climb_m)
            .collect()
    };
    let mut out = Vec::new();
    for (dx, dz) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        out.extend(step(dx, dz));
    }
    for (dx, dz) in [(1, 1), (1, -1), (-1, -1), (-1, 1)] {
        if !step(dx, 0).is_empty() && !step(0, dz).is_empty() {
            out.extend(step(dx, dz));
        }
    }
    out
}

/// The old plan's search from `from` to `to`, and every span it expanded with
/// the cost it expanded it at, in order.
fn old_route(
    grid: &SpanGrid,
    from: SpanId,
    to: SpanId,
) -> (Option<Vec<DVec3>>, Vec<(SpanId, f64)>) {
    let mut expanded = Vec::new();
    let target = old_feet_of(grid, to);
    let mut reached: BTreeMap<SpanId, (f64, SpanId)> = BTreeMap::from([(from, (0.0, from))]);
    let mut open = BinaryHeap::from([OldOpen {
        estimate: old_feet_of(grid, from).distance(target),
        span: from,
    }]);
    while let Some(OldOpen { span, .. }) = open.pop() {
        if span == to {
            let mut waypoints = Vec::new();
            let mut at = to;
            while at != from {
                waypoints.push(old_feet_of(grid, at));
                at = reached[&at].1;
            }
            waypoints.reverse();
            return (Some(waypoints), expanded);
        }
        let here = old_feet_of(grid, span);
        let so_far = reached[&span].0;
        expanded.push((span, so_far));
        for next in old_neighbours(grid, span) {
            let there = old_feet_of(grid, next);
            let through = so_far + here.distance(there);
            if reached.get(&next).is_none_or(|&(cost, _)| through < cost) {
                reached.insert(next, (through, span));
                open.push(OldOpen {
                    estimate: through + there.distance(target),
                    span: next,
                });
            }
        }
    }
    (None, expanded)
}

/// The old `reachable_within` from span `from`.
fn old_reachable(grid: &SpanGrid, from: SpanId, max_steps: usize) -> Vec<(DVec3, usize)> {
    let mut steps = BTreeMap::from([(from, 0)]);
    let mut frontier = VecDeque::from([from]);
    let mut reached = Vec::new();
    while let Some(span) = frontier.pop_front() {
        let here = steps[&span];
        reached.push((old_feet_of(grid, span), here));
        if here == max_steps {
            continue;
        }
        for next in old_neighbours(grid, span) {
            steps.entry(next).or_insert_with(|| {
                frontier.push_back(next);
                here + 1
            });
        }
    }
    reached
}

/// The grids the comparison runs over: lattice and free heights, a point
/// agent and an eroding one, small to long and thin.
fn seeded_grids() -> Vec<(u64, SpanGrid)> {
    let eroding = NavAgent {
        radius_m: 0.3,
        height_m: 1.8,
        max_climb_m: 0.4,
        min_floor_normal_y: 0.7,
    };
    let point = NavAgent {
        radius_m: 0.0,
        ..eroding
    };
    let mut grids = Vec::new();
    for seed in 0..48_u64 {
        let mut state = seed ^ 0x5eed;
        let width = 4 + below(&mut state, 28) as usize;
        let height = 1 + below(&mut state, 24) as usize;
        let agent = if seed % 3 == 0 { eroding } else { point };
        grids.push((seed, seeded_grid(seed, width, height, seed % 2 == 0, agent)));
    }
    grids
}

#[test]
fn plan_matches_the_old_search_route_for_route_and_expansion_for_expansion() {
    let (mut routes, mut no_routes, mut stale, mut early_stale) = (0, 0, 0, 0);
    for (seed, grid) in seeded_grids() {
        let spans = grid.span_count() as u64;
        if spans == 0 {
            continue;
        }
        let mut state = seed.wrapping_mul(31);
        for pair in 0..24 {
            let (from, to) = (
                below(&mut state, spans) as SpanId,
                below(&mut state, spans) as SpanId,
            );
            let (old, old_expanded) = old_route(&grid, from, to);
            let mut pops = Vec::new();
            let new = grid.route(from, to, |span, cost, so_far| {
                pops.push((span, cost, so_far))
            });
            assert_eq!(
                new.map(|route| route.waypoints),
                old,
                "seed {seed}, pair {pair}: {from} to {to} took another route"
            );
            // The old search re-expanded a span at a cost it had already
            // expanded it at, to no effect; the new one passes those over.
            let mut seen = BTreeSet::new();
            let useful: Vec<(SpanId, f64)> = old_expanded
                .iter()
                .copied()
                .filter(|&(span, cost)| seen.insert((span, cost.to_bits())))
                .collect();
            stale += old_expanded.len() - useful.len();
            let mut expanded = BTreeSet::new();
            let mut new_expanded = Vec::new();
            for &(span, cost, so_far) in &pops {
                let key = (span, so_far.to_bits());
                if cost <= so_far {
                    new_expanded.push((span, so_far));
                    expanded.insert(key);
                } else if !expanded.contains(&key) {
                    // A stale entry popping before its span's latest one, at
                    // an equal estimate: passed over while the span was still
                    // to be expanded at its current cost.
                    early_stale += 1;
                }
            }
            assert_eq!(
                new_expanded, useful,
                "seed {seed}, pair {pair}: {from} to {to} expanded other spans"
            );
            if old.is_some() {
                routes += 1;
            } else {
                no_routes += 1;
            }
        }
    }
    // The comparison ran over both answers, over the stale entries it filters
    // out, and over a stale entry popping first, not over nothing.
    assert!(
        routes > 100 && no_routes > 10 && stale > 0 && early_stale > 0,
        "{routes} routes, {no_routes} without, {stale} stale, {early_stale} stale before fresh"
    );
}

#[test]
fn reachable_within_matches_the_old_search() {
    let mut compared = 0;
    for (seed, grid) in seeded_grids() {
        let spans = grid.span_count() as u64;
        let mut state = seed.wrapping_mul(17);
        for _ in 0..spans.min(8) {
            let from = below(&mut state, spans) as SpanId;
            let max_steps = below(&mut state, 12) as usize;
            let feet = grid.feet_of(from);
            // Through the public query, so the start is read as a caller's
            // would be; it can stand on a higher floor within a climb.
            let start = grid.span_at(feet).unwrap().unwrap();
            assert_eq!(
                grid.reachable_within(feet, max_steps).unwrap(),
                old_reachable(&grid, start, max_steps),
                "seed {seed}: from {from} within {max_steps}"
            );
            compared += 1;
        }
    }
    assert!(compared > 100, "{compared} compared");
}

#[test]
fn a_plan_after_the_generation_wraps_ignores_slots_from_before() {
    let grid = seeded_grid(
        5,
        24,
        18,
        true,
        NavAgent {
            radius_m: 0.0,
            height_m: 1.8,
            max_climb_m: 0.4,
            min_floor_normal_y: 0.7,
        },
    );
    let spans = grid.span_count() as u64;
    let mut state = 0x77a9;
    let pairs: Vec<(SpanId, SpanId)> = (0..64)
        .map(|_| {
            (
                below(&mut state, spans) as SpanId,
                below(&mut state, spans) as SpanId,
            )
        })
        .filter(|&(from, to)| old_route(&grid, from, to).0.is_some_and(|w| w.len() > 4))
        .collect();
    assert!(pairs.len() > 4, "{} long pairs", pairs.len());
    // The thread's first plan stamps its slots with generation one, which
    // the wrap hands out again two plans on, to the same pair: a slot left
    // from the first would read as reached at its final cost.
    let (from, to) = pairs[0];
    grid.route(from, to, |_, _, _| {});
    SCRATCH.with(|scratch| scratch.borrow_mut().generation = u32::MAX - 1);
    let order = [pairs[1], pairs[0]]
        .into_iter()
        .chain(pairs[2..].iter().copied());
    for (n, (from, to)) in order.enumerate() {
        assert_eq!(
            grid.route(from, to, |_, _, _| {})
                .map(|route| route.waypoints),
            old_route(&grid, from, to).0,
            "plan {n}: {from} to {to}"
        );
    }
    let generation = SCRATCH.with(|scratch| scratch.borrow().generation);
    assert!(generation < 64, "the generation wrapped, to {generation}");
}
