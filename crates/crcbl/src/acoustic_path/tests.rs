//! EW's acoustic path tests, ported with a stand-in for its barrier enum, plus
//! the generic material and determinism cases the engine owes on top.

use crcbl_phys::Aabb;
use glam::DVec3;

use super::{
    AcousticObstacle, AcousticPathBuildError, AcousticPathConfig, AcousticPathConfigError,
    AcousticRoute, SoundPath, build_acoustic_path, validate_acoustic_path_config,
};

const EPSILON: f64 = 1e-12;

/// The materials EW's tests name, standing in for EW's own barrier enum — the
/// `M` a game supplies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcousticBarrier {
    IntactGlass,
    ThinPartition,
    ClosedWood,
    ClosedMetal,
    SolidBrickOrConcrete,
}

fn obstacle<M>(minimum: DVec3, maximum: DVec3, barrier: M) -> AcousticObstacle<M> {
    AcousticObstacle {
        bounds: Aabb::new(minimum, maximum),
        barrier,
    }
}

fn direct(source: DVec3, listener: DVec3) -> AcousticRoute<'static> {
    AcousticRoute {
        source,
        waypoints: &[],
        listener,
    }
}

/// A slab across the X axis from `minimum_x` to `maximum_x`, wide enough that
/// any route along the axis crosses it.
fn slab<M>(minimum_x: f64, maximum_x: f64, barrier: M) -> AcousticObstacle<M> {
    obstacle(
        DVec3::new(minimum_x, -1.0, -1.0),
        DVec3::new(maximum_x, 1.0, 1.0),
        barrier,
    )
}

// ---------------------------------------------------------------------------
// EW's tests
// ---------------------------------------------------------------------------

#[test]
fn clear_direct_path_keeps_its_distance_and_has_no_barriers() {
    let built = build_acoustic_path::<AcousticBarrier>(
        AcousticPathConfig::default(),
        direct(DVec3::ZERO, DVec3::new(3.0, 4.0, 0.0)),
        &[],
    )
    .unwrap();

    assert!((built.total_distance_m - 5.0).abs() < EPSILON);
    assert!(built.barriers.is_empty());
    let path = built.as_sound_path();
    assert_eq!(path.distance_m, built.total_distance_m);
    assert!(path.barriers.is_empty());
}

#[test]
fn one_crossed_obstacle_contributes_its_barrier() {
    let obstacles = [slab(1.0, 2.0, AcousticBarrier::ClosedWood)];

    let built = build_acoustic_path(
        AcousticPathConfig::default(),
        direct(DVec3::ZERO, DVec3::new(3.0, 0.0, 0.0)),
        &obstacles,
    )
    .unwrap();

    assert_eq!(built.barriers, vec![AcousticBarrier::ClosedWood]);
}

#[test]
fn distinct_same_kind_obstacles_are_retained_and_crossings_are_ordered() {
    let obstacles = [
        slab(4.0, 5.0, AcousticBarrier::ClosedMetal),
        slab(1.0, 2.0, AcousticBarrier::ClosedMetal),
        slab(2.5, 3.0, AcousticBarrier::IntactGlass),
    ];

    let built = build_acoustic_path(
        AcousticPathConfig::default(),
        direct(DVec3::ZERO, DVec3::new(6.0, 0.0, 0.0)),
        &obstacles,
    )
    .unwrap();

    assert_eq!(
        built.barriers,
        vec![
            AcousticBarrier::ClosedMetal,
            AcousticBarrier::IntactGlass,
            AcousticBarrier::ClosedMetal,
        ]
    );
}

#[test]
fn repeated_crossing_of_one_obstacle_is_deduplicated() {
    let obstacle = slab(-0.5, 0.5, AcousticBarrier::ThinPartition);
    let waypoints = [DVec3::new(2.0, 0.0, 0.0), DVec3::new(-2.0, 0.0, 0.0)];
    let route = AcousticRoute {
        source: DVec3::new(-2.0, 0.0, 0.0),
        waypoints: &waypoints,
        listener: DVec3::new(2.0, 0.0, 0.0),
    };

    let built = build_acoustic_path(AcousticPathConfig::default(), route, &[obstacle]).unwrap();

    assert_eq!(built.barriers, vec![AcousticBarrier::ThinPartition]);
}

#[test]
fn waypoint_route_through_an_opening_avoids_the_wall_and_uses_polyline_distance() {
    let wall = slab(-0.2, 0.2, AcousticBarrier::SolidBrickOrConcrete);
    let waypoints = [DVec3::new(-2.0, 0.0, 2.0), DVec3::new(2.0, 0.0, 2.0)];
    let route = AcousticRoute {
        source: DVec3::new(-2.0, 0.0, 0.0),
        waypoints: &waypoints,
        listener: DVec3::new(2.0, 0.0, 0.0),
    };

    let built = build_acoustic_path(AcousticPathConfig::default(), route, &[wall]).unwrap();

    assert_eq!(built.barriers, Vec::new());
    assert!((built.total_distance_m - 8.0).abs() < EPSILON);
}

#[test]
fn boundary_tangency_counts_and_zero_length_segments_do_not() {
    let tangent = obstacle(
        DVec3::new(-1.0, -1.0, -1.0),
        DVec3::new(1.0, 1.0, 1.0),
        AcousticBarrier::IntactGlass,
    );
    let tangent_path = build_acoustic_path(
        AcousticPathConfig::default(),
        direct(DVec3::new(-2.0, 1.0, 0.0), DVec3::new(2.0, 1.0, 0.0)),
        &[tangent],
    )
    .unwrap();
    assert_eq!(tangent_path.barriers, vec![AcousticBarrier::IntactGlass]);

    let enclosed = obstacle(
        DVec3::new(-1.0, -1.0, -1.0),
        DVec3::new(1.0, 1.0, 1.0),
        AcousticBarrier::ClosedWood,
    );
    let route = AcousticRoute {
        source: DVec3::ZERO,
        waypoints: &[DVec3::ZERO],
        listener: DVec3::ZERO,
    };
    let zero_length =
        build_acoustic_path(AcousticPathConfig::default(), route, &[enclosed]).unwrap();
    assert_eq!(zero_length.total_distance_m, 0.0);
    assert!(zero_length.barriers.is_empty());
}

#[test]
fn bounds_and_invalid_inputs_are_rejected_without_a_path() {
    assert_eq!(
        validate_acoustic_path_config(AcousticPathConfig {
            maximum_obstacles: 0,
            maximum_waypoints: 1,
        }),
        Err(AcousticPathConfigError::InvalidConfiguration)
    );
    assert_eq!(
        build_acoustic_path::<AcousticBarrier>(
            AcousticPathConfig {
                maximum_obstacles: 0,
                maximum_waypoints: 1,
            },
            direct(DVec3::ZERO, DVec3::X),
            &[],
        ),
        Err(AcousticPathBuildError::InvalidConfiguration)
    );
    let config = AcousticPathConfig {
        maximum_obstacles: 1,
        maximum_waypoints: 1,
    };
    let obstacles = [
        obstacle(DVec3::ZERO, DVec3::ONE, AcousticBarrier::ClosedWood),
        obstacle(
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(3.0, 1.0, 1.0),
            AcousticBarrier::ClosedMetal,
        ),
    ];
    assert_eq!(
        build_acoustic_path(config, direct(DVec3::ZERO, DVec3::X), &obstacles),
        Err(AcousticPathBuildError::TooManyObstacles)
    );
    assert_eq!(
        build_acoustic_path::<AcousticBarrier>(
            config,
            AcousticRoute {
                source: DVec3::ZERO,
                waypoints: &[DVec3::X, DVec3::Y],
                listener: DVec3::Z,
            },
            &[],
        ),
        Err(AcousticPathBuildError::TooManyWaypoints)
    );
    assert_eq!(
        build_acoustic_path::<AcousticBarrier>(
            AcousticPathConfig::default(),
            direct(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X),
            &[],
        ),
        Err(AcousticPathBuildError::InvalidPosition)
    );
    let non_finite_bounds = obstacle(
        DVec3::ZERO,
        DVec3::new(f64::NAN, 1.0, 1.0),
        AcousticBarrier::ClosedWood,
    );
    assert_eq!(
        build_acoustic_path(
            AcousticPathConfig::default(),
            direct(DVec3::ZERO, DVec3::X),
            &[non_finite_bounds],
        ),
        Err(AcousticPathBuildError::InvalidObstacleBounds)
    );
    let empty = obstacle(
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(-1.0, -1.0, -1.0),
        AcousticBarrier::ClosedWood,
    );
    assert_eq!(
        build_acoustic_path(
            AcousticPathConfig::default(),
            direct(DVec3::ZERO, DVec3::X),
            &[empty],
        ),
        Err(AcousticPathBuildError::InvalidObstacleBounds)
    );
}

// ---------------------------------------------------------------------------
// The engine's own: the generic material, and determinism
// ---------------------------------------------------------------------------

/// **The material really is the caller's type**, and the builder asks nothing
/// of it beyond `Clone`: a `u8` material id and an owned `String` — which is
/// not `Copy` — both come back exactly as the obstacles carried them, in route
/// order, and lend themselves as a [`SoundPath`] of the same type.
#[test]
fn any_clone_material_is_carried_through_unchanged() {
    let ids = [slab(3.0, 4.0, 7_u8), slab(1.0, 2.0, 200_u8)];
    let built = build_acoustic_path(
        AcousticPathConfig::default(),
        direct(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0)),
        &ids,
    )
    .unwrap();
    assert_eq!(built.barriers, vec![200_u8, 7]);
    let path: SoundPath<'_, u8> = built.as_sound_path();
    assert_eq!(path.barriers, &[200_u8, 7]);
    assert_eq!(path.distance_m, 5.0);

    let named = [
        slab(3.0, 4.0, String::from("far door")),
        slab(1.0, 2.0, String::from("near door")),
    ];
    let built = build_acoustic_path(
        AcousticPathConfig::default(),
        direct(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0)),
        &named,
    )
    .unwrap();
    assert_eq!(built.barriers, ["near door", "far door"]);
    // A borrowed path copies whatever its material is.
    let path = built.as_sound_path();
    let copied = path;
    assert_eq!(copied, path);
}

/// **Obstacles entered at the same point keep their input order**, which is
/// the tie-break the module docs promise. Two coincident slabs are entered at
/// the same distance; swapping them in the input swaps them in the output, so
/// the order is the input's and not an artefact of the sort.
#[test]
fn equal_entries_keep_input_order() {
    let route = direct(DVec3::ZERO, DVec3::new(3.0, 0.0, 0.0));
    let forward = [
        slab(1.0, 2.0, AcousticBarrier::ClosedWood),
        slab(1.0, 2.0, AcousticBarrier::ClosedMetal),
    ];
    let reversed = [forward[1], forward[0]];

    let built = build_acoustic_path(AcousticPathConfig::default(), route, &forward).unwrap();
    assert_eq!(
        built.barriers,
        [AcousticBarrier::ClosedWood, AcousticBarrier::ClosedMetal]
    );
    let built = build_acoustic_path(AcousticPathConfig::default(), route, &reversed).unwrap();
    assert_eq!(
        built.barriers,
        [AcousticBarrier::ClosedMetal, AcousticBarrier::ClosedWood]
    );
}

/// **Same inputs, same output, barrier order included.** A route with
/// waypoints doubling back, obstacles out of route order, and a tie — every
/// ordering rule at once — built repeatedly, each result compared in full to
/// the first. The expected order is written out too, so a build that was
/// consistently wrong would not pass by agreeing with itself.
#[test]
fn identical_inputs_build_identical_paths() {
    let waypoints = [DVec3::new(6.0, 0.0, 0.0), DVec3::new(0.5, 0.0, 0.0)];
    let route = AcousticRoute {
        source: DVec3::ZERO,
        waypoints: &waypoints,
        listener: DVec3::new(0.5, 0.0, 3.0),
    };
    let obstacles = [
        slab(4.0, 5.0, AcousticBarrier::ClosedMetal),
        slab(2.0, 3.0, AcousticBarrier::IntactGlass),
        slab(2.0, 3.0, AcousticBarrier::ThinPartition),
        slab(1.0, 1.5, AcousticBarrier::ClosedWood),
        // Reached only by the last leg, which runs up +Z from x = 0.5.
        obstacle(
            DVec3::new(0.0, -1.0, 2.0),
            DVec3::new(1.0, 1.0, 2.5),
            AcousticBarrier::SolidBrickOrConcrete,
        ),
    ];

    let first = build_acoustic_path(AcousticPathConfig::default(), route, &obstacles).unwrap();
    assert_eq!(
        first.barriers,
        [
            AcousticBarrier::ClosedWood,
            AcousticBarrier::IntactGlass,
            AcousticBarrier::ThinPartition,
            AcousticBarrier::ClosedMetal,
            AcousticBarrier::SolidBrickOrConcrete,
        ]
    );
    assert!((first.total_distance_m - (6.0 + 5.5 + 3.0)).abs() < EPSILON);

    for _ in 0..16 {
        let again = build_acoustic_path(AcousticPathConfig::default(), route, &obstacles).unwrap();
        assert_eq!(again, first);
        assert_eq!(
            again.total_distance_m.to_bits(),
            first.total_distance_m.to_bits()
        );
    }
}

/// The errors say what went wrong, for a caller that reports rather than
/// matches.
#[test]
fn errors_display_and_are_std_errors() {
    fn is_error(_: &dyn std::error::Error) {}
    is_error(&AcousticPathConfigError::InvalidConfiguration);
    is_error(&AcousticPathBuildError::TooManyObstacles);
    for error in [
        AcousticPathBuildError::InvalidConfiguration,
        AcousticPathBuildError::TooManyObstacles,
        AcousticPathBuildError::TooManyWaypoints,
        AcousticPathBuildError::InvalidPosition,
        AcousticPathBuildError::InvalidObstacleBounds,
        AcousticPathBuildError::InvalidSegmentLength,
    ] {
        assert!(!error.to_string().is_empty(), "{error:?} displays nothing");
    }
    assert!(
        !AcousticPathConfigError::InvalidConfiguration
            .to_string()
            .is_empty()
    );
}
