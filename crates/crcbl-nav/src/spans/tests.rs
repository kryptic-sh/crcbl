use super::*;

const CELL_M: f64 = 0.25;

/// An adult-sized body: crcbl-phys's default character's limits, spelled out
/// so these tests need no physics.
const AGENT: NavAgent = NavAgent {
    radius_m: 0.3,
    height_m: 1.8,
    max_climb_m: 0.4,
    min_floor_normal_y: std::f64::consts::FRAC_1_SQRT_2,
};

/// A box from `min` to `max`.
#[derive(Clone, Copy)]
struct Solid {
    min: DVec3,
    max: DVec3,
}

fn solid(min: [f64; 3], max: [f64; 3]) -> Solid {
    Solid {
        min: DVec3::from(min),
        max: DVec3::from(max),
    }
}

/// Flat ground at y = 0 and `solids` on it, probed as EW's physics probe does:
/// every box top over the cell's centre and the ground, each with headroom
/// unless a box stands straight above it at the centre, or overlaps the band
/// from a climb above it to the agent's height anywhere across the cell.
fn probe(solids: &[Solid]) -> impl FnMut(DVec2) -> Vec<ProbedFloor> + '_ {
    move |at| {
        let half = CELL_M * 0.5;
        let across = |s: &Solid| {
            s.min.x < at.x + half
                && s.max.x > at.x - half
                && s.min.z < at.y + half
                && s.max.z > at.y - half
        };
        let inside =
            |s: &Solid| (s.min.x..=s.max.x).contains(&at.x) && (s.min.z..=s.max.z).contains(&at.y);
        let mut tops: Vec<f64> = solids
            .iter()
            .filter(|s| inside(s))
            .map(|s| s.max.y)
            .collect();
        tops.push(0.0);
        tops.into_iter()
            .map(|y| ProbedFloor {
                y,
                normal_y: 1.0,
                headroom_clear: !solids.iter().any(|s| {
                    let reaches = |from: f64| s.min.y < y + AGENT.height_m && s.max.y > y + from;
                    (inside(s) && reaches(0.01)) || (across(s) && reaches(AGENT.max_climb_m))
                }),
            })
            .collect()
    }
}

fn config(width_m: f64, depth_m: f64) -> SpanGridConfig {
    SpanGridConfig {
        origin: DVec2::ZERO,
        cell_size_m: CELL_M,
        width: (width_m / CELL_M) as usize,
        height: (depth_m / CELL_M) as usize,
    }
}

fn build(config: SpanGridConfig, solids: &[Solid]) -> SpanGrid {
    SpanGrid::build(config, AGENT, probe(solids)).unwrap()
}

/// A deck 2.4 m up over x 6..10, its underside at 2.2 m, reached by twelve
/// 0.2 m steps of 0.3 m treads rising toward +x from x = 2.4.
fn deck_and_stairs() -> Vec<Solid> {
    let mut solids = vec![solid([6.0, 2.2, 1.0], [10.0, 2.4, 5.0])];
    for step in 0..12 {
        let x = 2.4 + 0.3 * f64::from(step);
        solids.push(solid(
            [x, 0.0, 2.0],
            [x + 0.3, 0.2 * f64::from(step + 1), 4.0],
        ));
    }
    solids
}

fn assert_climbable(route: &GroundRoute, start: DVec3) {
    let mut previous = start;
    for &point in &route.waypoints {
        assert!(
            (point.y - previous.y).abs() <= AGENT.max_climb_m,
            "a step from {previous:?} to {point:?} climbs past the agent's limit"
        );
        previous = point;
    }
}

#[cfg(feature = "phys")]
#[test]
fn a_character_agent_takes_the_controllers_limits() {
    let character = crcbl_phys::character::CharacterConfig::default();
    let agent = NavAgent::from_character(&character);
    assert_eq!(agent.radius_m, character.radius);
    assert_eq!(
        agent.height_m,
        2.0 * (character.half_height + character.radius)
    );
    assert_eq!(agent.max_climb_m, character.step_offset);
    assert_eq!(agent.min_floor_normal_y, character.min_ground_normal_y);
}

#[test]
fn a_route_climbs_stairs_onto_a_deck_and_the_ground_under_it_stays_walkable() {
    let grid = build(config(12.0, 6.0), &deck_and_stairs());

    assert_eq!(
        grid.floors_at(DVec3::new(8.0, 0.0, 3.0)),
        &[0.0, 2.4],
        "the ground under the deck and the deck above it"
    );

    let start = DVec3::new(0.5, 0.0, 3.0);
    let route = grid
        .plan(start, DVec3::new(9.0, 2.4, 3.0))
        .expect("a route up the stairs");
    assert!(
        (route.waypoints.last().unwrap().y - 2.4).abs() < 1e-9,
        "the route ends on the deck"
    );
    assert_climbable(&route, start);
    // And the ground under the deck routes to itself, past the stairs' end.
    let under = grid
        .plan(DVec3::new(0.5, 0.0, 0.5), DVec3::new(8.0, 0.0, 3.0))
        .expect("a route under the deck");
    assert!(under.waypoints.iter().all(|point| point.y == 0.0));
}

#[test]
fn a_route_from_beside_a_deck_goes_round_by_the_stairs_rather_than_up_its_edge() {
    let grid = build(config(12.0, 6.0), &deck_and_stairs());
    // Straight up the deck's side would be shortest.
    let start = DVec3::new(8.0, 0.0, 0.5);
    let route = grid
        .plan(start, DVec3::new(8.0, 2.4, 1.5))
        .expect("a route round to the stairs");
    assert_climbable(&route, start);
    assert!(
        route.waypoints.iter().any(|point| point.x < 6.0),
        "the route goes round by the stairs"
    );
}

#[test]
fn routes_keep_a_radius_from_a_pillar_and_pass_a_doorway_but_not_a_slot() {
    let pillar = solid([2.8, 0.0, 2.8], [3.2, 3.0, 3.2]);
    let grid = build(config(6.0, 6.0), &[pillar]);
    let route = grid
        .plan(DVec3::new(0.5, 0.0, 3.0), DVec3::new(5.5, 0.0, 3.0))
        .expect("a route round the pillar");
    for point in &route.waypoints {
        let on_ground = DVec2::new(point.x, point.z);
        let nearest = on_ground.clamp(DVec2::splat(2.8), DVec2::splat(3.2));
        assert!(
            nearest.distance(on_ground) >= AGENT.radius_m,
            "the route passes {point:?}, inside a body's radius of the pillar"
        );
    }

    // A wall across z = 3 with a gap: a 1.3 m doorway lets a body through, a
    // 0.5 m slot does not.
    for (gap, passes) in [(1.3, true), (0.5, false)] {
        let (left, right) = (3.0 - gap * 0.5, 3.0 + gap * 0.5);
        let wall = [
            solid([0.0, 0.0, 2.9], [left, 3.0, 3.1]),
            solid([right, 0.0, 2.9], [6.0, 3.0, 3.1]),
        ];
        let grid = build(config(6.0, 6.0), &wall);
        assert_eq!(
            grid.plan(DVec3::new(3.0, 0.0, 1.0), DVec3::new(3.0, 0.0, 5.0))
                .is_ok(),
            passes,
            "a {gap} m gap"
        );
    }
}

#[test]
fn the_region_edge_erodes_like_a_drop() {
    let grid = build(config(3.0, 3.0), &[]);
    // At this cell size the radius reaches the next column over and no
    // farther, so only the outermost columns lose their floors.
    assert!(grid.floors_at(DVec3::new(0.1, 0.0, 1.5)).is_empty());
    assert_eq!(grid.floors_at(DVec3::new(0.3, 0.0, 1.5)), &[0.0]);
    assert_eq!(grid.floors_at(DVec3::new(2.7, 0.0, 1.5)), &[0.0]);
    assert!(grid.floors_at(DVec3::new(2.9, 0.0, 1.5)).is_empty());
    assert!(grid.floors_at(DVec3::new(1.5, 0.0, 2.9)).is_empty());
}

#[test]
fn floors_too_steep_or_without_headroom_are_left_out() {
    let steep = |at: DVec2| {
        vec![ProbedFloor {
            y: 0.0,
            normal_y: if at.x > 3.0 { 0.5 } else { 1.0 },
            headroom_clear: at.y < 4.0,
        }]
    };
    let grid = SpanGrid::build(config(6.0, 6.0), AGENT, steep).unwrap();
    assert_eq!(grid.floors_at(DVec3::new(1.0, 0.0, 1.0)), &[0.0]);
    assert!(
        grid.floors_at(DVec3::new(5.0, 0.0, 1.0)).is_empty(),
        "a slope past the limit"
    );
    assert!(
        grid.floors_at(DVec3::new(1.0, 0.0, 5.0)).is_empty(),
        "no headroom"
    );
    assert_eq!(
        grid.plan(DVec3::new(1.0, 0.0, 1.0), DVec3::new(5.0, 0.0, 1.0)),
        Err(GroundRouteError::NoRoute)
    );
}

#[test]
fn a_plan_is_the_same_every_time() {
    let pillar = [solid([2.0, 0.0, 2.0], [4.0, 3.0, 4.0])];
    let (start, goal) = (DVec3::new(0.5, 0.0, 3.0), DVec3::new(5.5, 0.0, 3.0));
    let (first, second) = (
        build(config(6.0, 6.0), &pillar),
        build(config(6.0, 6.0), &pillar),
    );
    assert_eq!(first, second);
    assert_eq!(first.plan(start, goal), second.plan(start, goal));
}

#[test]
fn a_route_leaves_out_the_start_and_ends_on_the_goal_as_the_ground_grid_does() {
    let grid = build(config(3.0, 3.0), &[]);
    let start = DVec3::new(1.1, 0.0, 1.1);
    assert_eq!(grid.plan(start, start).unwrap().waypoints, vec![]);
    let route = grid.plan(start, DVec3::new(1.3, 0.1, 1.1)).unwrap();
    assert_eq!(route.waypoints, vec![DVec3::new(1.375, 0.0, 1.125)]);
}

#[test]
fn a_position_picks_the_floor_within_a_climb_of_its_feet() {
    let grid = build(config(12.0, 6.0), &deck_and_stairs());
    // The column at (8, 3) holds the ground and the deck, 2.4 m apart.
    for (feet, floor) in [(0.0, 0.0), (0.4, 0.0), (-0.4, 0.0), (2.0, 2.4), (2.8, 2.4)] {
        let stood_on = grid.reachable_within(DVec3::new(8.0, feet, 3.0), 0);
        assert_eq!(
            stood_on,
            Ok(vec![(DVec3::new(8.125, floor, 3.125), 0)]),
            "feet at {feet}"
        );
    }
    for feet in [1.2, 2.81, -0.41] {
        assert_eq!(
            grid.reachable_within(DVec3::new(8.0, feet, 3.0), 0),
            Err(GroundRouteError::NoRoute),
            "feet at {feet} stand on neither floor"
        );
    }
}

#[test]
fn reachable_spans_spread_by_steps_and_climb_onto_the_deck_by_the_stairs() {
    let grid = build(config(12.0, 6.0), &deck_and_stairs());
    let start = DVec3::new(1.0, 0.0, 3.0);
    let near = grid.reachable_within(start, 4).unwrap();
    assert_eq!(near[0], (DVec3::new(1.125, 0.0, 3.125), 0));
    assert!(
        near.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "steps never decrease"
    );
    // Four diagonal steps, from a start up to half a cell's diagonal off its
    // cell's centre.
    let reach = (4.0 * std::f64::consts::SQRT_2 + std::f64::consts::FRAC_1_SQRT_2) * CELL_M;
    assert!(
        near.iter()
            .all(|&(at, _)| at.distance(start) <= reach + 1e-9)
    );
    // The 9 × 9 block of cells around the start, less the column at the
    // region's edge, which erosion empties.
    assert_eq!(near.len(), 8 * 9);
    let far = grid.reachable_within(start, 200).unwrap();
    assert!(
        far.iter().any(|&(at, _)| (at.y - 2.4).abs() < 1e-9),
        "the deck is reachable by the stairs"
    );
    assert_eq!(far, grid.reachable_within(start, 200).unwrap());
    assert_eq!(
        grid.reachable_within(start, 0).unwrap(),
        vec![(DVec3::new(1.125, 0.0, 3.125), 0)]
    );
}

#[test]
fn a_query_refuses_what_the_ground_grid_refuses() {
    let grid = build(config(3.0, 3.0), &[]);
    let inside = DVec3::new(1.5, 0.0, 1.5);
    assert_eq!(
        grid.plan(DVec3::new(f64::NAN, 0.0, 1.0), inside),
        Err(GroundRouteError::InvalidWorldPosition)
    );
    assert_eq!(
        grid.plan(inside, DVec3::new(-0.1, 0.0, 1.0)),
        Err(GroundRouteError::OutsideGrid)
    );
    assert_eq!(
        grid.reachable_within(DVec3::new(3.0, 0.0, 1.0), 1),
        Err(GroundRouteError::OutsideGrid)
    );
    assert_eq!(
        grid.reachable_within(DVec3::new(1.5, 5.0, 1.5), 1),
        Err(GroundRouteError::NoRoute)
    );
}

#[test]
fn a_build_refuses_a_bad_config_agent_or_floor() {
    let ground = |_: DVec2| {
        vec![ProbedFloor {
            y: 0.0,
            normal_y: 1.0,
            headroom_clear: true,
        }]
    };
    let good = config(3.0, 3.0);
    let refuse = |config: SpanGridConfig, agent: NavAgent| {
        SpanGrid::build(config, agent, ground).unwrap_err()
    };
    assert_eq!(
        refuse(
            SpanGridConfig {
                origin: DVec2::new(f64::INFINITY, 0.0),
                ..good
            },
            AGENT
        ),
        SpanGridError::InvalidOrigin
    );
    assert_eq!(
        refuse(
            SpanGridConfig {
                cell_size_m: 0.0,
                ..good
            },
            AGENT
        ),
        SpanGridError::InvalidCellSize
    );
    assert_eq!(
        refuse(SpanGridConfig { width: 0, ..good }, AGENT),
        SpanGridError::InvalidDimensions
    );
    assert_eq!(
        refuse(
            SpanGridConfig {
                width: MAX_SPAN_COLUMN_COUNT,
                height: 2,
                ..good
            },
            AGENT
        ),
        SpanGridError::TooManyColumns
    );
    assert_eq!(
        refuse(
            SpanGridConfig {
                width: usize::MAX,
                height: 2,
                ..good
            },
            AGENT
        ),
        SpanGridError::TooManyColumns
    );
    for agent in [
        NavAgent {
            radius_m: -0.1,
            ..AGENT
        },
        NavAgent {
            height_m: 0.0,
            ..AGENT
        },
        NavAgent {
            max_climb_m: f64::NAN,
            ..AGENT
        },
        NavAgent {
            min_floor_normal_y: 1.5,
            ..AGENT
        },
    ] {
        assert_eq!(
            refuse(good, agent),
            SpanGridError::InvalidAgent,
            "{agent:?}"
        );
    }
    assert_eq!(
        refuse(
            good,
            NavAgent {
                radius_m: CELL_M * (MAX_EROSION_REACH_CELLS as f64 + 1.0),
                ..AGENT
            }
        ),
        SpanGridError::RadiusTooWide
    );
    assert!(
        SpanGrid::build(
            good,
            NavAgent {
                radius_m: CELL_M * MAX_EROSION_REACH_CELLS as f64,
                ..AGENT
            },
            ground
        )
        .is_ok()
    );
    let bad_floor = |_: DVec2| {
        vec![ProbedFloor {
            y: f64::NAN,
            normal_y: 1.0,
            headroom_clear: false,
        }]
    };
    assert_eq!(
        SpanGrid::build(good, AGENT, bad_floor),
        Err(SpanGridError::InvalidFloor)
    );
}

/// A probe answering each column's floors from `floors`, row by row, flat and
/// with headroom.
fn columns<'a>(
    width: usize,
    floors: &'a [&'a [f64]],
) -> impl FnMut(DVec2) -> Vec<ProbedFloor> + 'a {
    move |at| {
        let (column, row) = ((at.x / CELL_M) as usize, (at.y / CELL_M) as usize);
        floors[row * width + column]
            .iter()
            .map(|&y| ProbedFloor {
                y,
                normal_y: 1.0,
                headroom_clear: true,
            })
            .collect()
    }
}

/// [`AGENT`] with no radius, so nothing erodes.
const POINT: NavAgent = NavAgent {
    radius_m: 0.0,
    ..AGENT
};

fn grid_of(width: usize, height: usize, agent: NavAgent, floors: &[&[f64]]) -> SpanGrid {
    let config = SpanGridConfig {
        origin: DVec2::ZERO,
        cell_size_m: CELL_M,
        width,
        height,
    };
    SpanGrid::build(config, agent, columns(width, floors)).unwrap()
}

#[test]
fn a_floor_links_to_every_floor_within_a_climb_not_only_the_nearest() {
    // The middle column holds a landing and a step; only the step reaches the
    // last column's floor.
    let grid = grid_of(3, 1, POINT, &[&[0.0], &[0.0, 0.3], &[0.6]]);
    let route = grid
        .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(0.6, 0.6, 0.1))
        .unwrap();
    assert_eq!(
        route.waypoints,
        vec![DVec3::new(0.375, 0.3, 0.125), DVec3::new(0.625, 0.6, 0.125)]
    );
}

#[test]
fn a_diagonal_step_needs_both_columns_it_cuts_past() {
    // (1, 0) has no floor, so (0, 0) to (1, 1) goes round by (0, 1).
    let grid = grid_of(2, 2, POINT, &[&[0.0], &[], &[0.0], &[0.0]]);
    let route = grid
        .plan(DVec3::new(0.1, 0.0, 0.1), DVec3::new(0.4, 0.0, 0.4))
        .unwrap();
    assert_eq!(
        route.waypoints,
        vec![DVec3::new(0.125, 0.0, 0.375), DVec3::new(0.375, 0.0, 0.375)]
    );
}

#[test]
fn erosion_reaches_every_column_whose_cell_comes_within_the_radius() {
    // One column with no floor in the middle of a 7 × 7 field. The radius is
    // 1.2 cells: the nearest point of a side or diagonal neighbour's cell is
    // 0.5 or 0.71 cells from the centre, so those erode, and of a column two
    // over 1.5 cells or more, so those are kept.
    let mut floors: Vec<&[f64]> = vec![&[0.0]; 49];
    floors[3 * 7 + 3] = &[];
    let grid = grid_of(7, 7, AGENT, &floors);
    let at = |column: usize, row: usize| {
        grid.floors_at(DVec3::new(
            (column as f64 + 0.5) * CELL_M,
            0.0,
            (row as f64 + 0.5) * CELL_M,
        ))
        .to_vec()
    };
    assert!(at(3, 2).is_empty(), "beside the hole");
    assert!(at(2, 2).is_empty(), "diagonal to the hole");
    assert_eq!(at(1, 3), vec![0.0], "two columns from the hole");
    assert_eq!(at(1, 2), vec![0.0], "two over and one along");
}

/// **A position inside a wall snaps to the nearest kept floor on its own side,
/// and a plan from there succeeds where one from the position did not.**
///
/// The wall's column keeps no ground floor (no headroom) and its top is no
/// floor within a climb of the feet, so the position stands on nothing; the
/// floors beside it are eroded by the agent's radius, so the nearest kept one
/// is a few columns off — which is why the rings are a parameter.
#[test]
fn nearest_walkable_snaps_a_position_in_a_wall_to_the_nearest_kept_floor() {
    let grid = build(config(4.0, 4.0), &[solid([1.9, 0.0, 0.0], [2.6, 3.0, 4.0])]);
    let goal = DVec3::new(0.5, 0.0, 2.0);
    let near_minus_x = DVec3::new(2.0, 0.0, 2.0);
    assert_eq!(
        grid.plan(near_minus_x, goal),
        Err(GroundRouteError::NoRoute)
    );
    assert_eq!(grid.nearest_walkable(near_minus_x, 0), Ok(None));

    let snapped = grid
        .nearest_walkable(near_minus_x, 4)
        .unwrap()
        .expect("a kept floor lies within four rings");
    assert!(
        snapped.y.abs() < 1e-9,
        "the ground, not the wall top: {snapped}"
    );
    assert!(
        snapped.x < 1.9 - AGENT.radius_m,
        "the -X side, clear of the wall by the radius: {snapped}"
    );
    assert!((snapped.z - near_minus_x.z).abs() <= CELL_M, "{snapped}");
    assert!(grid.plan(snapped, goal).is_ok());

    let near_plus_x = DVec3::new(2.5, 0.0, 2.0);
    let other = grid.nearest_walkable(near_plus_x, 4).unwrap().unwrap();
    assert!(other.x > 2.6 + AGENT.radius_m, "the +X side: {other}");

    let open = DVec3::new(0.6, 0.1, 2.1);
    assert_eq!(
        grid.nearest_walkable(open, 4).unwrap(),
        grid.reachable_within(open, 0)
            .unwrap()
            .first()
            .map(|&(at, _)| at),
        "a position on a kept floor answers that floor"
    );
    assert_eq!(
        grid.nearest_walkable(DVec3::new(f64::NAN, 0.0, 2.0), 1),
        Err(GroundRouteError::InvalidWorldPosition)
    );
    assert_eq!(
        grid.nearest_walkable(DVec3::new(9.0, 0.0, 2.0), 1),
        Err(GroundRouteError::OutsideGrid)
    );
}

/// **The nearest floor is nearest in height too**: a position at the foot of a
/// pillar under the deck snaps to the ground beside the pillar, not to the deck
/// straight above it, and one at the deck's height answers the deck.
#[test]
fn nearest_walkable_prefers_the_floor_on_the_positions_own_storey() {
    // A pillar under the middle of the deck, from the ground to its underside.
    let mut solids = deck_and_stairs();
    solids.push(solid([7.9, 0.0, 2.9], [8.1, 2.2, 3.1]));
    let grid = build(config(10.0, 6.0), &solids);
    let low = DVec3::new(8.0, 0.0, 3.0);
    let high = DVec3::new(8.0, 2.4, 3.0);
    assert_eq!(
        grid.plan(low, DVec3::new(9.0, 0.0, 3.0)),
        Err(GroundRouteError::NoRoute)
    );

    let ground = grid.nearest_walkable(low, 4).unwrap().unwrap();
    assert!(
        ground.y.abs() < 1e-9,
        "beside the pillar on the ground: {ground}"
    );
    let deck = grid.nearest_walkable(high, 4).unwrap().unwrap();
    assert!((deck.y - 2.4).abs() < 1e-9, "the deck itself: {deck}");
}

/// **Ties go to the lower span, and a position on a kept floor answers the one
/// `plan` reads, not merely the nearest.**
///
/// A hand-written probe and a body narrower than half a cell, so nothing is
/// eroded and every floor is exactly where the probe put it: three columns in
/// a row, the outer two holding a floor at the ground and a ledge a step above
/// it, the middle one holding none.
#[test]
fn nearest_walkable_breaks_ties_by_span_and_reads_the_floor_plan_reads() {
    let narrow = NavAgent {
        radius_m: CELL_M * 0.4,
        ..AGENT
    };
    let ledge = AGENT.max_climb_m * 0.75;
    let config = SpanGridConfig {
        origin: DVec2::ZERO,
        cell_size_m: CELL_M,
        width: 3,
        height: 1,
    };
    let grid = SpanGrid::build(config, narrow, |at: DVec2| {
        if at.x > CELL_M && at.x < 2.0 * CELL_M {
            return Vec::new();
        }
        [0.0, ledge]
            .into_iter()
            .map(|y| ProbedFloor {
                y,
                normal_y: 1.0,
                headroom_clear: true,
            })
            .collect()
    })
    .unwrap();
    assert_eq!(grid.span_count(), 4, "nothing eroded");

    // Midway between the outer columns at the ground: the ground floors tie.
    let middle = DVec3::new(1.5 * CELL_M, 0.0, 0.5 * CELL_M);
    assert_eq!(
        grid.nearest_walkable(middle, 0),
        Ok(None),
        "the floors are one ring out"
    );
    assert_eq!(
        grid.nearest_walkable(middle, 1),
        Ok(Some(DVec3::new(0.5 * CELL_M, 0.0, 0.5 * CELL_M))),
        "the -X column's ground is the lower span"
    );

    // On the first column, nearer the ground than the ledge but within a climb
    // of both: `plan` stands it on the ledge, the highest within a climb.
    let feet = DVec3::new(0.5 * CELL_M, ledge * 0.25, 0.5 * CELL_M);
    assert_eq!(
        grid.nearest_walkable(feet, 1),
        Ok(Some(DVec3::new(0.5 * CELL_M, ledge, 0.5 * CELL_M)))
    );
}
