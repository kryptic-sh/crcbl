//! The debug view: each body's outline on its surface and marked asleep or
//! awake, each joint's axis and limit, and the server-against-client gap.

mod common;

use common::{LEG, entities, model, names, skeleton, world};
use crcbl_anim::Pose;
use crcbl_phys::{PhysicsSystem, Transform};
use crcbl_ragdoll::{
    BodyShape, DebugLine, Handoff, JOINT_AXIS_LENGTH, Ragdoll, RagdollDesc, RagdollRig,
    RigJointKind, divergence_lines,
};
use glam::DVec3;

/// How near a drawn point must lie to what it marks, in metres.
const NEAR: f64 = 1e-9;

/// The leg spawned at rest, a metre up.
fn leg() -> (RagdollRig, PhysicsSystem, Ragdoll) {
    let rig = RagdollDesc::from_ron(LEG)
        .expect("parses")
        .resolve(&skeleton(), &names(), model(0.0))
        .expect("a rig");
    let skeleton = skeleton();
    let pose = Pose::new(&skeleton);
    let mut physics = world();
    let handoff = Handoff {
        pose: &pose,
        model: model(1.0),
        previous: None,
    };
    let ragdoll = Ragdoll::spawn(
        &mut physics,
        &rig,
        &skeleton,
        &handoff,
        &entities(1, rig.bodies().len()),
    )
    .expect("spawned");
    (rig, physics, ragdoll)
}

/// Every segment of `ragdoll`'s debug view.
fn lines(
    ragdoll: &Ragdoll,
    physics: &PhysicsSystem,
    rig: &RagdollRig,
) -> Vec<(DVec3, DVec3, DebugLine)> {
    let mut lines = Vec::new();
    ragdoll.debug_lines(physics, rig, &mut |from, to, kind| {
        lines.push((from, to, kind));
    });
    lines
}

/// How far `point` lies from the surface of `shape` placed at `world`.
fn off_surface(point: DVec3, shape: BodyShape, world: &Transform) -> f64 {
    let local = world.rotation.inverse() * (point - world.position);
    match shape {
        BodyShape::Sphere { radius } => (local.length() - radius).abs(),
        BodyShape::Capsule {
            radius,
            half_height,
        } => {
            let spine = DVec3::Y * local.y.clamp(-half_height, half_height);
            (local.distance(spine) - radius).abs()
        }
        BodyShape::Box { half_extents } => {
            let outside = local.abs() - half_extents;
            if outside.max_element() > NEAR {
                outside.max_element()
            } else {
                outside.max_element().abs()
            }
        }
    }
}

/// **Each body's outline lies on its surface, marked awake and then asleep
/// once settled; each joint has an axis, and the ball's cone and the hinge's
/// arc are drawn where they let the child turn.** A body taken out of the
/// system takes its outline and its joints with it.
#[test]
fn the_debug_view_draws_bodies_joints_and_limits() {
    let (rig, mut physics, ragdoll) = leg();
    let drawn = lines(&ragdoll, &physics, &rig);
    let worlds: Vec<Transform> = ragdoll.body_transforms(&physics).expect("every body");
    let mut outlined = vec![false; worlds.len()];
    for &(from, to, kind) in &drawn {
        if kind != (DebugLine::Body { sleeping: false }) {
            continue;
        }
        for point in [from, to] {
            let body = rig
                .bodies()
                .iter()
                .zip(&worlds)
                .position(|(body, world)| off_surface(point, body.shape, world) < NEAR);
            let body = body.unwrap_or_else(|| panic!("{point} is on no body's surface"));
            outlined[body] = true;
        }
    }
    assert!(outlined.iter().all(|&drawn| drawn), "outlined {outlined:?}");

    let axes: Vec<(DVec3, DVec3)> = drawn
        .iter()
        .filter(|(_, _, kind)| *kind == DebugLine::JointAxis)
        .map(|&(from, to, _)| (from, to))
        .collect();
    assert_eq!(axes.len(), rig.joints().len(), "one axis per joint");
    for &(from, to) in &axes {
        assert!((from.distance(to) - JOINT_AXIS_LENGTH).abs() < NEAR);
    }
    let mut on_cone = false;
    // The least and greatest hinge angle an arc point was drawn at.
    let mut swept = (f64::INFINITY, f64::NEG_INFINITY);
    for &(from, to, kind) in &drawn {
        if kind != DebugLine::Limit {
            continue;
        }
        for point in [from, to] {
            let marks = rig
                .joints()
                .iter()
                .zip(&axes)
                .any(|(joint, &(origin, tip))| {
                    let axis = (tip - origin).normalize();
                    let reach = point - origin;
                    let at_length = (reach.length() - JOINT_AXIS_LENGTH).abs() < NEAR;
                    let angle = axis.angle_between(reach);
                    match joint.kind {
                        RigJointKind::Ball { cone, .. }
                            if at_length && (angle - cone).abs() < NEAR =>
                        {
                            on_cone = true;
                            true
                        }
                        RigJointKind::Hinge { .. }
                            if at_length && (angle - core::f64::consts::FRAC_PI_2).abs() < NEAR =>
                        {
                            // Measured from the parent's `x`, as the revolute
                            // angle is.
                            let zero = worlds[joint.parent].rotation
                                * joint.frame_parent.rotation
                                * DVec3::X;
                            let turned = zero.cross(reach).dot(axis).atan2(zero.dot(reach));
                            swept = (swept.0.min(turned), swept.1.max(turned));
                            true
                        }
                        _ => false,
                    }
                });
            assert!(marks, "{point} marks no joint's limit");
        }
    }
    assert!(on_cone, "no cone drawn");
    let Some(RigJointKind::Hinge { lower, upper }) = rig
        .joints()
        .iter()
        .map(|joint| joint.kind)
        .find(|kind| matches!(kind, RigJointKind::Hinge { .. }))
    else {
        panic!("the leg has a hinge");
    };
    assert!(
        (swept.0 - lower).abs() < NEAR && (swept.1 - upper).abs() < NEAR,
        "the arc swept {swept:?}, not the limits {lower} to {upper}"
    );

    assert!(ragdoll.force_settle(&mut physics), "settled");
    let settled = lines(&ragdoll, &physics, &rig);
    assert!(
        settled
            .iter()
            .all(|(_, _, kind)| *kind != (DebugLine::Body { sleeping: false })),
        "a settled body drawn awake"
    );
    let (from, to, _) = drawn[0];
    assert!(
        settled.contains(&(from, to, DebugLine::Body { sleeping: true })),
        "a settled body not drawn asleep"
    );

    let foot = *ragdoll.entities().last().expect("a foot");
    physics.remove_entity(foot);
    let footless = lines(&ragdoll, &physics, &rig);
    let axes = footless
        .iter()
        .filter(|(_, _, kind)| *kind == DebugLine::JointAxis)
        .count();
    assert_eq!(axes, rig.joints().len() - 1, "the foot's weld still drawn");
    let foot_world = worlds.last().expect("a foot");
    let foot_shape = rig.bodies().last().expect("a foot").shape;
    assert!(
        footless
            .iter()
            .filter(|(_, _, kind)| matches!(kind, DebugLine::Body { .. }))
            .all(|&(from, _, _)| off_surface(from, foot_shape, foot_world) > NEAR),
        "the foot's outline still drawn"
    );
}

/// **The divergence view joins each joint the two share, marked within the
/// width or not**, and leaves out a joint only one has.
#[test]
fn the_divergence_view_joins_shared_joints() {
    let server = [(1, DVec3::ZERO), (2, DVec3::X)];
    let client = [(1, DVec3::Y * 0.05), (2, DVec3::X * 2.0), (3, DVec3::Z)];
    let mut drawn = Vec::new();
    divergence_lines(&server, &client, 0.1, &mut |from, to, kind| {
        drawn.push((from, to, kind));
    });
    assert_eq!(
        drawn,
        [
            (
                DVec3::Y * 0.05,
                DVec3::ZERO,
                DebugLine::Divergence { within: true }
            ),
            (
                DVec3::X * 2.0,
                DVec3::X,
                DebugLine::Divergence { within: false }
            ),
        ]
    );
}
