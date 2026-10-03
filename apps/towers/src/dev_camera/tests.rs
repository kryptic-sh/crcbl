//! The dev camera's modes, its look, and the fly camera passing through what
//! the walker cannot.

use super::*;
use crate::game::DEFAULT_TICK_HZ;
use crate::tower::{Kind, Tier};

/// One tick at the default rate.
const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

/// A dev camera over the committed field.
fn dev_camera() -> DevCamera {
    DevCamera::new(&Map::built_in())
}

/// A bolt tower on every plot of the committed field.
fn every_plot_built() -> [Option<TowerView>; MAX_PLOTS] {
    let mut towers = [None; MAX_PLOTS];
    for tower in towers.iter_mut().take(Map::built_in().plots().len()) {
        *tower = Some(TowerView {
            kind: Kind::Bolt,
            tier: Tier::Base,
            working: false,
        });
    }
    towers
}

/// Steps `camera` by `steer` for `count` ticks.
fn hold(camera: &mut DevCamera, steer: Steer, count: usize) {
    for _ in 0..count {
        camera.step(steer, DT, &[None; MAX_PLOTS]);
    }
}

/// Where a camera looks, flattened onto the ground and made unit.
fn flat_look(camera: &Camera) -> Vec3 {
    let ahead = camera.target - camera.eye;
    Vec3::new(ahead.x, 0.0, ahead.z).normalize()
}

/// **The modes go overhead, fly, walk and back, and the overhead camera is
/// the fixed one exactly** — and leaving for it puts the fly camera back at
/// its pose, so a fly flown away from and come back to starts over the field
/// again rather than wherever the last one was left.
#[test]
fn the_modes_cycle_and_come_back_to_the_overhead_camera_exactly() {
    let mut camera = dev_camera();
    assert_eq!(camera.mode(), Mode::Overhead);
    assert_eq!(camera.camera(), crate::camera::camera());

    assert_eq!(camera.cycle(), Mode::Fly);
    let start = camera.camera().eye;
    assert_eq!(
        start,
        crate::camera::EYE,
        "the fly camera does not start overhead"
    );
    hold(
        &mut camera,
        Steer {
            ahead: 1.0,
            turn: 1.0,
            ..Steer::default()
        },
        30,
    );
    assert_ne!(camera.camera().eye, start, "the fly camera did not fly");

    assert_eq!(camera.cycle(), Mode::Walk);
    assert_eq!(camera.cycle(), Mode::Overhead);
    assert_eq!(camera.camera(), crate::camera::camera());

    assert_eq!(camera.cycle(), Mode::Fly);
    let again = camera.camera();
    assert_eq!(
        again.eye, start,
        "the next fly starts where the last was left"
    );
    assert!(
        flat_look(&again).dot(flat_look(&crate::camera::camera())) > 0.9999,
        "the next fly does not look where the overhead camera looks",
    );
}

/// **The fly and the walk both go where the camera looks**: one tick of
/// "ahead" moves the eye along the view flattened onto the ground, and one of
/// "strafe" along the view's right — read off the drawn camera's own matrix,
/// at several turns, so a sign in the yaw or in the conversion is a red test.
#[test]
fn the_walk_is_where_the_camera_looks() {
    // A fly camera turned right for `turns` ticks.
    let turned = |turns: usize| {
        let mut fly = dev_camera();
        fly.cycle();
        hold(
            &mut fly,
            Steer {
                turn: 1.0,
                ..Steer::default()
            },
            turns,
        );
        fly
    };
    for turns in [0, 7, 19, 40] {
        let drawn = turned(turns).camera();
        let right = drawn.view().row(0).truncate();
        for (steer, want) in [
            (
                Steer {
                    ahead: 1.0,
                    ..Steer::default()
                },
                flat_look(&drawn),
            ),
            (
                Steer {
                    strafe: 1.0,
                    ..Steer::default()
                },
                Vec3::new(right.x, 0.0, right.z).normalize(),
            ),
        ] {
            let mut moved = turned(turns);
            let before = moved.camera().eye;
            hold(&mut moved, steer, 1);
            let went = (moved.camera().eye - before).normalize();
            assert!(
                went.dot(want) > 0.9999,
                "after {turns} turns {steer:?} flew {went:?}, not {want:?}",
            );

            let mut walk = turned(turns);
            walk.cycle();
            // Landed first, so the step below is a walk and not a fall.
            hold(&mut walk, Steer::default(), 200);
            assert!(walk.walker().is_grounded(), "the walker never landed");
            let before = walk.walker().feet();
            hold(&mut walk, steer, 1);
            let went = walk.walker().feet() - before;
            let went = went.as_vec3().with_y(0.0).normalize();
            assert!(
                went.dot(want) > 0.9999,
                "after {turns} turns {steer:?} walked {went:?}, not {want:?}",
            );
        }
    }
}

/// **The fly camera passes through everything**: down through the ground
/// slab, then straight across a field with a tower on every plot and out
/// through the walker's edge wall, covering exactly what [`FLY_SPEED`] says
/// in a straight line each way.
#[test]
fn the_fly_camera_passes_through_everything() {
    let towers = every_plot_built();
    let (_, bend) = {
        let map = Map::built_in();
        let at = map
            .plots()
            .iter()
            .position(|plot| plot.label == "bend")
            .expect("the committed field has a bend plot");
        (at, map.plots()[at].at())
    };
    let mut camera = dev_camera();
    camera.cycle();
    let start = camera.camera().eye;
    let per_tick = FLY_SPEED / DEFAULT_TICK_HZ as f32;
    let fly = |camera: &mut DevCamera, steer: Steer, count: usize| {
        for _ in 0..count {
            camera.step(steer, DT, &towers);
        }
    };

    // Down to a metre off the ground: inside every tower's height.
    let down = ((start.y - 1.0) / per_tick).round() as usize;
    fly(
        &mut camera,
        Steer {
            rise: -1.0,
            ..Steer::default()
        },
        down,
    );
    // Across to the bend plot's line, then straight through its tower.
    let across = ((bend.x as f32 - start.x) / per_tick).round() as usize;
    fly(
        &mut camera,
        Steer {
            strafe: 1.0,
            ..Steer::default()
        },
        across,
    );
    // From the overhead eye's side of the field out past the far edge.
    let through = ((start.z + crate::map::HALF_DEPTH as f32 + 1.0) / per_tick).round() as usize;
    let before = camera.camera().eye;
    fly(
        &mut camera,
        Steer {
            ahead: 1.0,
            ..Steer::default()
        },
        through,
    );
    let eye = camera.camera().eye;
    let want = before - Vec3::Z * per_tick * through as f32;
    assert!(
        (eye - want).length() < 1e-3,
        "the fly camera ended at {eye:?}, not {want:?}"
    );
    assert!(
        (before.x - bend.x as f32).abs() < 1e-3,
        "it did not line up on the tower"
    );
    assert!(
        eye.z < -(crate::map::HALF_DEPTH as f32),
        "it did not leave the field"
    );

    fly(
        &mut camera,
        Steer {
            rise: -1.0,
            ..Steer::default()
        },
        DEFAULT_TICK_HZ as usize,
    );
    assert!(
        camera.camera().eye.y < -(crate::map::SLAB_THICKNESS as f32),
        "the fly camera stopped at the ground",
    );
}

/// **A walk starts under the fly camera**, dropped onto the field and kept
/// inside its edge — the overhead pose stands off the field's near edge, so
/// switching straight to the walk from it lands at that edge — and the walk
/// camera's eye is the walker's.
#[test]
fn a_walk_drops_from_under_the_fly_camera_onto_the_field() {
    let mut camera = dev_camera();
    camera.cycle();
    camera.cycle();
    assert_eq!(camera.mode(), Mode::Walk);
    hold(&mut camera, Steer::default(), 4 * DEFAULT_TICK_HZ as usize);

    let walker = camera.walker();
    assert!(walker.is_grounded(), "the walker did not land");
    let feet = walker.feet();
    assert!(
        feet.z.abs() < crate::map::HALF_DEPTH && feet.x.abs() < crate::map::HALF_WIDTH,
        "the walker landed off the field at {feet:?}",
    );
    assert!(
        (f64::from(crate::camera::EYE.x) - feet.x).abs() < 1e-9,
        "it was not dropped under the fly camera",
    );
    let eye = walker.eye();
    assert!(
        (camera.camera().eye - eye.as_vec3()).length() < 1e-6,
        "the walk camera is not at the walker's eye",
    );
}
