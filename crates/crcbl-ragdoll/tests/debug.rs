//! The debug view: each body's outline on its surface and marked asleep or
//! awake, each joint's axis and limit, and the server-against-client gap.

mod common;

use common::{LEG, entities, model, names, skeleton, world};
use crcbl_anim::Pose;
use crcbl_phys::{PhysicsSystem, Transform};
use crcbl_ragdoll::{
    BodyShape, DebugLine, Handoff, JOINT_AXIS_LENGTH, Ragdoll, RagdollDesc, RagdollRig,
    RigJointKind, TWIST_RADIUS, divergence_lines,
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
    // The least and greatest hinge angle an arc point was drawn at, and the
    // same of the ball's twist arc.
    let mut swept = (f64::INFINITY, f64::NEG_INFINITY);
    let mut twisted = (f64::INFINITY, f64::NEG_INFINITY);
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
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "measuring a drawn line, not simulating"
                    )]
                    let angle = axis.angle_between(reach);
                    let square = (angle - core::f64::consts::FRAC_PI_2).abs() < NEAR;
                    // Measured from the parent's `x`, as the revolute and the
                    // twist angle are.
                    let zero =
                        worlds[joint.parent].rotation * joint.frame_parent.rotation * DVec3::X;
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "measuring a drawn line, not simulating"
                    )]
                    let turned = zero.cross(reach).dot(axis).atan2(zero.dot(reach));
                    match joint.kind {
                        // The leg's swing is circular, so its rim is a cone.
                        RigJointKind::Ball { swing_x, .. }
                            if at_length && (angle - swing_x).abs() < NEAR =>
                        {
                            on_cone = true;
                            true
                        }
                        RigJointKind::Ball { .. }
                            if square && (reach.length() - TWIST_RADIUS).abs() < NEAR =>
                        {
                            twisted = (twisted.0.min(turned), twisted.1.max(turned));
                            true
                        }
                        RigJointKind::Hinge { .. } if at_length && square => {
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
    let Some(RigJointKind::Hinge { lower, upper, .. }) = rig
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
    let Some(RigJointKind::Ball {
        twist_lower,
        twist_upper,
        ..
    }) = rig
        .joints()
        .iter()
        .map(|joint| joint.kind)
        .find(|kind| matches!(kind, RigJointKind::Ball { .. }))
    else {
        panic!("the leg has a ball joint");
    };
    assert!(
        (twisted.0 - twist_lower).abs() < NEAR && (twisted.1 - twist_upper).abs() < NEAR,
        "the twist arc swept {twisted:?}, not {twist_lower} to {twist_upper}"
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

/// **An elliptical swing is drawn as its ellipse, soft bounds apart from
/// hard ones, and every limit about the joint's neutral**: each point of the
/// ball's hard rim, and of its smaller soft one, stands where `crcbl_phys`'s
/// swing ellipse of those half-angles puts the child's axis; the twist and
/// the hinge arcs sweep their limits, and soft spokes stand at the soft
/// bounds — the hinge's measured from its rest frame, turned by its neutral.
#[test]
fn the_debug_view_draws_ellipses_soft_bounds_and_the_neutral() {
    let rig = RagdollDesc::from_ron(common::EVERYTHING)
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
    let drawn = lines(&ragdoll, &physics, &rig);
    let worlds = ragdoll.body_transforms(&physics).expect("every body");
    // Each joint's frame in the world, as the physics joint holds it, and as
    // it stood at rest.
    let place = |joint: usize, frame: &Transform| {
        let parent = worlds[rig.joints()[joint].parent];
        Transform::new(
            parent.position + parent.rotation * frame.position,
            parent.rotation * frame.rotation,
        )
    };
    let hip = place(0, &rig.joints()[0].physics_frames().0);
    let knee = place(1, &rig.joints()[1].physics_frames().0);
    let knee_rest = place(1, &rig.joints()[1].frame_parent);
    #[expect(
        clippy::disallowed_methods,
        reason = "measuring a drawn line, not simulating"
    )]
    let (sin, atan2) = (f64::sin, f64::atan2);
    // How far a point at the axis length from the hip lies from the swing
    // ellipse of half-angles `x` and `y`, in its own function: zero on it.
    let off_ellipse = |point: DVec3, (x, y): (f64, f64)| {
        let reach = hip.rotation.inverse() * (point - hip.position) / JOINT_AXIS_LENGTH;
        let swing = glam::DQuat::from_rotation_arc(DVec3::Z, reach.normalize());
        let (u, v) = (swing.x / sin(0.5 * x), swing.y / sin(0.5 * y));
        (u * u + v * v - 1.0).abs()
    };
    let turned = |frame: &Transform, point: DVec3| {
        let (zero, axis) = (frame.rotation * DVec3::X, frame.rotation * DVec3::Z);
        let reach = point - frame.position;
        atan2(zero.cross(reach).dot(axis), zero.dot(reach))
    };
    let widen = |(lo, hi): (f64, f64), value: f64| (lo.min(value), hi.max(value));
    let empty = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut hard_rim, mut soft_rim) = (0, 0);
    let (mut twisted, mut swept) = (empty, empty);
    let (mut twist_spokes, mut hinge_spokes) = (Vec::new(), Vec::new());
    for &(from, to, kind) in &drawn {
        let at = |frame: &Transform, point: DVec3, radius: f64| {
            (point.distance(frame.position) - radius).abs() < NEAR
        };
        match kind {
            DebugLine::Limit => {
                for point in [from, to] {
                    if at(&hip, point, JOINT_AXIS_LENGTH) {
                        assert!(off_ellipse(point, (1.0, 0.5)) < NEAR, "{point} off the rim");
                        hard_rim += 1;
                    } else if at(&hip, point, TWIST_RADIUS) {
                        twisted = widen(twisted, turned(&hip, point));
                    } else if at(&knee, point, JOINT_AXIS_LENGTH) {
                        swept = widen(swept, turned(&knee_rest, point));
                    } else {
                        panic!("{point} marks no hard limit");
                    }
                }
            }
            DebugLine::SoftLimit if at(&hip, from, 0.0) && at(&hip, to, TWIST_RADIUS) => {
                twist_spokes.push(turned(&hip, to));
            }
            DebugLine::SoftLimit if at(&knee, from, 0.0) && at(&knee, to, JOINT_AXIS_LENGTH) => {
                hinge_spokes.push(turned(&knee_rest, to));
            }
            DebugLine::SoftLimit => {
                for point in [from, to] {
                    assert!(
                        at(&hip, point, JOINT_AXIS_LENGTH) && off_ellipse(point, (0.6, 0.3)) < NEAR,
                        "{point} is on no soft bound"
                    );
                    soft_rim += 1;
                }
            }
            _ => {}
        }
    }
    assert!(
        hard_rim > 0 && soft_rim > 0,
        "rims {hard_rim} hard, {soft_rim} soft"
    );
    let near = |(lo, hi): (f64, f64), (want_lo, want_hi): (f64, f64)| {
        (lo - want_lo).abs() < NEAR && (hi - want_hi).abs() < NEAR
    };
    assert!(
        near(twisted, (-0.4, 0.4)),
        "the twist arc swept {twisted:?}"
    );
    // The knee's limits, −1.0 to 0.5 about a neutral of 0.4, from rest.
    assert!(near(swept, (-0.6, 0.9)), "the hinge arc swept {swept:?}");
    let sorted = |mut spokes: Vec<f64>| {
        spokes.sort_by(f64::total_cmp);
        spokes
    };
    let spokes = |spokes: Vec<f64>, want: [f64; 2]| {
        let spokes = sorted(spokes);
        spokes.len() == 2 && spokes.iter().zip(want).all(|(a, b)| (a - b).abs() < NEAR)
    };
    assert!(
        spokes(twist_spokes.clone(), [-0.2, 0.1]),
        "twist spokes {twist_spokes:?}"
    );
    assert!(
        spokes(hinge_spokes.clone(), [-0.1, 0.6]),
        "hinge spokes {hinge_spokes:?}"
    );
}
