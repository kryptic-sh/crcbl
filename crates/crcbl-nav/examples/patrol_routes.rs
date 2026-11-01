//! Patrol routes across a generated span grid, for instruction counts.
//!
//! The shape of EW's generated maps: a square region of quarter-metre cells
//! about 160 m across, over rolling ground, with walls, low platforms and
//! raised decks scattered on a coarse lattice, so a column holds one floor or
//! two. A long plan crosses the whole region, corner to corner or edge to
//! edge; a short one is a hop of a few cells on the same grid.
//!
//! ```text
//! cargo build --release -p crcbl-nav --example patrol_routes
//! valgrind --tool=callgrind target/release/examples/patrol_routes long 0
//! valgrind --tool=callgrind target/release/examples/patrol_routes long 8
//! valgrind --tool=callgrind target/release/examples/patrol_routes short 1000
//! ```
//!
//! The arguments are `long` or `short` and how many plans to run, cycling
//! through [`ROUTES`] or [`HOPS`]. The difference between a run's total and
//! the zero-plan run's, divided by the plans, is what one plan costs, with the
//! grid's construction cancelled out.

use std::hint::black_box;

use crcbl_nav::spans::{NavAgent, ProbedFloor, SpanGrid, SpanGridConfig};
use glam::{DVec2, DVec3};

/// The side of one cell, in metres: EW's.
const CELL_M: f64 = 0.25;
/// Cells along each side of the square region.
const CELLS: usize = 640;
/// The side of one obstacle lattice square, in metres.
const LATTICE_M: f64 = 4.0;
/// Lattice squares this close to the region's edge hold no obstacle, so every
/// route's ends stand on open ground.
const CLEAR_BORDER: usize = 2;

/// crcbl-phys's default character's limits, spelled out so the example needs
/// no physics.
const AGENT: NavAgent = NavAgent {
    radius_m: 0.3,
    height_m: 1.8,
    max_climb_m: 0.4,
    min_floor_normal_y: std::f64::consts::FRAC_1_SQRT_2,
};

/// Each route's start and goal X and Z, in metres.
const ROUTES: [([f64; 2], [f64; 2]); 8] = [
    ([2.0, 2.0], [158.0, 158.0]),
    ([2.0, 158.0], [158.0, 2.0]),
    ([2.0, 80.0], [158.0, 80.0]),
    ([80.0, 2.0], [80.0, 158.0]),
    ([158.0, 158.0], [2.0, 2.0]),
    ([158.0, 2.0], [2.0, 158.0]),
    ([158.0, 80.0], [2.0, 80.0]),
    ([80.0, 158.0], [80.0, 2.0]),
];

/// Short hops' start and goal X and Z, in metres: a few cells each, on the
/// open ground inside the clear border.
const HOPS: [([f64; 2], [f64; 2]); 4] = [
    ([3.0, 3.0], [4.0, 3.5]),
    ([150.0, 5.0], [151.0, 5.0]),
    ([5.0, 150.0], [5.5, 151.0]),
    ([156.0, 156.0], [155.0, 155.0]),
];

/// SplitMix64's output function (Steele, Lea and Flood, 2014): a well-mixed
/// 64-bit value for each input.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The rolling ground's height at `at`, and its slope along X and Z.
fn ground(at: DVec2) -> (f64, DVec2) {
    let (a, b, c) = (at.x / 11.0, at.y / 13.0, (at.x + at.y) / 7.0);
    let height = 1.5 * a.sin() * b.cos() + 0.8 * c.sin();
    let slope = DVec2::new(
        1.5 / 11.0 * a.cos() * b.cos() + 0.8 / 7.0 * c.cos(),
        -1.5 / 13.0 * a.sin() * b.sin() + 0.8 / 7.0 * c.cos(),
    );
    (height, slope)
}

/// What stands in one lattice square.
enum Obstacle {
    None,
    /// A wall 3 m tall over the footprint.
    Wall,
    /// A solid block 0.3 m tall, stepped onto.
    Platform,
    /// A deck 2.5 m up with headroom under it, reached by no ramp.
    Deck,
}

/// The obstacle in lattice square (`i`, `j`), and the footprint it covers
/// there as a minimum and maximum corner, in metres.
fn obstacle(i: usize, j: usize) -> (Obstacle, DVec2, DVec2) {
    let lattice = CELLS as f64 * CELL_M / LATTICE_M;
    let last = lattice as usize - CLEAR_BORDER;
    if i < CLEAR_BORDER || j < CLEAR_BORDER || i >= last || j >= last {
        return (Obstacle::None, DVec2::ZERO, DVec2::ZERO);
    }
    let hash = splitmix64((i * 1000 + j) as u64);
    let corner = DVec2::new(i as f64, j as f64) * LATTICE_M;
    let (kind, size) = match hash % 100 {
        0..30 if hash & (1 << 40) == 0 => (Obstacle::Wall, DVec2::new(3.0, 1.0)),
        0..30 => (Obstacle::Wall, DVec2::new(1.0, 3.0)),
        30..45 => (Obstacle::Platform, DVec2::new(2.0, 2.0)),
        45..55 => (Obstacle::Deck, DVec2::new(3.0, 3.0)),
        _ => (Obstacle::None, DVec2::ZERO),
    };
    let min = corner + 0.5;
    (kind, min, min + size)
}

/// The floors in the column at `at`, in the shape EW's probe reports them.
fn probe(at: DVec2) -> Vec<ProbedFloor> {
    let (height, slope) = ground(at);
    let flat = |y| ProbedFloor {
        y,
        normal_y: 1.0,
        headroom_clear: true,
    };
    let mut floors = vec![ProbedFloor {
        y: height,
        normal_y: 1.0 / (1.0 + slope.length_squared()).sqrt(),
        headroom_clear: true,
    }];
    // In range: the region starts at the origin.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (i, j) = ((at.x / LATTICE_M) as usize, (at.y / LATTICE_M) as usize);
    let (kind, min, max) = obstacle(i, j);
    if at.cmplt(min).any() || at.cmpge(max).any() {
        return floors;
    }
    let base = ground(min).0;
    match kind {
        Obstacle::None => {}
        Obstacle::Wall => {
            floors[0].headroom_clear = false;
            floors.push(flat(base + 3.0));
        }
        Obstacle::Platform => {
            floors[0].headroom_clear = false;
            floors.push(flat(base + 0.3));
        }
        Obstacle::Deck => floors.push(flat(base + 2.5)),
    }
    floors
}

fn main() {
    let mut args = std::env::args().skip(1);
    let routes = match args.next().as_deref() {
        Some("long") => &ROUTES[..],
        Some("short") => &HOPS[..],
        _ => panic!("the first argument is `long` or `short`"),
    };
    let plans: usize = args
        .next()
        .map_or(0, |count| count.parse().expect("a plan count"));
    let config = SpanGridConfig {
        origin: DVec2::ZERO,
        cell_size_m: CELL_M,
        width: CELLS,
        height: CELLS,
    };
    let grid = SpanGrid::build(config, AGENT, probe).expect("a valid grid");
    println!("{} spans", grid.span_count());
    let mut waypoints = 0;
    for &(start, goal) in routes.iter().cycle().take(plans) {
        let feet = |at: [f64; 2]| {
            let at = DVec2::from(at);
            DVec3::new(at.x, ground(at).0, at.y)
        };
        let route = black_box(&grid)
            .plan(black_box(feet(start)), black_box(feet(goal)))
            .expect("every route joins open ground");
        waypoints += route.waypoints.len();
        black_box(route);
    }
    println!("{plans} plans, {waypoints} waypoints");
}
