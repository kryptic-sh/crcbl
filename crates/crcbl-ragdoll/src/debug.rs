//! The plan's debug view of a ragdoll, as line segments for a debug draw
//! layer to show: each body's shape, telling awake from asleep, each joint's
//! axis and how far it lets the child turn, and the gap between a client's
//! joints and the server's they follow.
//!
//! The crate draws nothing itself — it depends on no renderer — so each
//! segment goes to a caller's sink with a [`DebugLine`] saying what it is,
//! for the caller to colour; `crcbl_render::debug_draw::DebugDraw::line`
//! takes them as they come.
//!
//! A ball joint's limit is drawn as the rim of its cone about the parent's
//! axis, and a hinge's as the arc its child's `x` sweeps between the limits,
//! from the parent's `x`, as `crcbl_phys`'s revolute angle is measured. A
//! ball joint's twist is drawn the same way, a smaller arc
//! ([`TWIST_RADIUS`]) between its twist limits. The twist is the child's turn
//! about its own axis, but the arc stays in the parent's plane, so it is
//! exact only while the child stands on the parent's axis. A weld has nothing
//! drawn beyond the axis.

use core::f64::consts::TAU;

use crcbl_phys::{PhysicsSystem, Transform};
use glam::DVec3;

use crate::instance::Ragdoll;
use crate::rig::{BodyShape, RagdollRig, RigJointKind, compose};

/// What a segment from [`Ragdoll::debug_lines`] or [`divergence_lines`]
/// shows, for the caller to colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugLine {
    /// A body's outline, and whether the body sleeps.
    Body {
        /// The body is asleep: the ragdoll may be settling.
        sleeping: bool,
    },
    /// A joint's axis, from its origin.
    JointAxis,
    /// The edge of what a joint lets through.
    Limit,
    /// From a client's joint to the server's same joint, and whether the two
    /// stand closer than the width the caller allows.
    Divergence {
        /// Within the width.
        within: bool,
    },
}

/// Segments in a full turn of a drawn circle.
pub const CIRCLE_SEGMENTS: usize = 16;

/// How long a joint's drawn axis and limit are, in metres.
pub const JOINT_AXIS_LENGTH: f64 = 0.1;

/// The radius of a ball joint's drawn twist arc, in metres: inside a hinge's
/// arc, so a twist limit never reads as a hinge's.
pub const TWIST_RADIUS: f64 = JOINT_AXIS_LENGTH / 2.0;
const _: () = assert!(TWIST_RADIUS < JOINT_AXIS_LENGTH);

impl Ragdoll {
    /// Hands `line` every segment of this ragdoll's debug view, as `physics`
    /// holds its bodies now, each tagged with the [`DebugLine`] it is: each
    /// body's outline, marked asleep or awake, and each of `rig`'s joints'
    /// axis and limit — a ball joint's cone rim about the parent's axis and
    /// its twist arc, a hinge's arc from the parent's `x` between its angles,
    /// and nothing more for a weld. `rig` is the one the ragdoll was spawned
    /// from; a body gone from `physics` is left out, with its joints.
    pub fn debug_lines(
        &self,
        physics: &PhysicsSystem,
        rig: &RagdollRig,
        line: &mut impl FnMut(DVec3, DVec3, DebugLine),
    ) {
        let worlds: Vec<Option<Transform>> = self
            .entities()
            .iter()
            .map(|&entity| physics.transform(entity).copied())
            .collect();
        for ((body, world), &entity) in rig.bodies().iter().zip(&worlds).zip(self.entities()) {
            let Some(world) = world else {
                continue;
            };
            let kind = DebugLine::Body {
                sleeping: physics.is_sleeping(entity),
            };
            outline(world, body.shape, &mut |from, to| line(from, to, kind));
        }
        for joint in rig.joints() {
            let (Some(Some(parent)), Some(Some(_))) =
                (worlds.get(joint.parent), worlds.get(joint.child))
            else {
                continue;
            };
            let frame = compose(parent, &joint.frame_parent);
            let origin = frame.position;
            let axis = frame.rotation * DVec3::Z;
            line(
                origin,
                origin + axis * JOINT_AXIS_LENGTH,
                DebugLine::JointAxis,
            );
            let mut limit = |from, to| line(from, to, DebugLine::Limit);
            let (u, v) = (frame.rotation * DVec3::X, frame.rotation * DVec3::Y);
            match joint.kind {
                RigJointKind::Ball {
                    cone,
                    twist_lower,
                    twist_upper,
                } => {
                    let rim = origin + axis * (JOINT_AXIS_LENGTH * cone.cos());
                    let radius = JOINT_AXIS_LENGTH * cone.sin();
                    arc(rim, (u, v), radius, (0.0, TAU), &mut limit);
                    arc(
                        origin,
                        (u, v),
                        TWIST_RADIUS,
                        (twist_lower, twist_upper),
                        &mut limit,
                    );
                }
                RigJointKind::Hinge { lower, upper } => {
                    arc(
                        origin,
                        (u, v),
                        JOINT_AXIS_LENGTH,
                        (lower, upper),
                        &mut limit,
                    );
                }
                RigJointKind::Fixed => {}
            }
        }
    }
}

/// Hands `line` a segment from each of `client`'s joints to the same joint in
/// `server`, both as [`Ragdoll::joint_positions`] gives them, marked within
/// `width` metres or not: the plan's server-against-client divergence view.
/// A joint only one of them has is left out.
pub fn divergence_lines(
    server: &[(usize, DVec3)],
    client: &[(usize, DVec3)],
    width: f64,
    line: &mut impl FnMut(DVec3, DVec3, DebugLine),
) {
    for &(joint, mine) in client {
        if let Some(&(_, theirs)) = server.iter().find(|(other, _)| *other == joint) {
            let within = mine.distance(theirs) < width;
            line(mine, theirs, DebugLine::Divergence { within });
        }
    }
}

/// `shape`'s outline placed at `world`: three rings for a sphere, the rings
/// and side lines of a capsule's cylinder with a ring at each cap's widest,
/// and a box's twelve edges.
fn outline(world: &Transform, shape: BodyShape, line: &mut impl FnMut(DVec3, DVec3)) {
    let at = |local: DVec3| world.position + world.rotation * local;
    let axes = [DVec3::X, DVec3::Y, DVec3::Z].map(|axis| world.rotation * axis);
    match shape {
        BodyShape::Sphere { radius } => {
            for (u, v) in [(0, 1), (1, 2), (2, 0)] {
                arc(world.position, (axes[u], axes[v]), radius, (0.0, TAU), line);
            }
        }
        BodyShape::Capsule {
            radius,
            half_height,
        } => {
            for end in [-half_height, half_height] {
                let centre = at(DVec3::Y * end);
                arc(centre, (axes[2], axes[0]), radius, (0.0, TAU), line);
                // The cap's half ring across each of the other two axes.
                let outward = axes[1] * end.signum();
                for side in [axes[0], axes[2]] {
                    arc(centre, (side, outward), radius, (0.0, TAU / 2.0), line);
                }
            }
            for side in [axes[0], -axes[0], axes[2], -axes[2]] {
                let offset = side * radius;
                line(
                    at(-DVec3::Y * half_height) + offset,
                    at(DVec3::Y * half_height) + offset,
                );
            }
        }
        BodyShape::Box { half_extents } => {
            let corner = |index: usize| {
                let sign = |bit: usize| if index & bit == 0 { -1.0 } else { 1.0 };
                at(half_extents * DVec3::new(sign(1), sign(2), sign(4)))
            };
            for index in 0..8 {
                for bit in [1, 2, 4] {
                    if index & bit == 0 {
                        line(corner(index), corner(index | bit));
                    }
                }
            }
        }
    }
}

/// The arc of radius `radius` about `centre` in the plane of `u` and `v`,
/// from angle `from` to `to` measured from `u` towards `v`, as segments of
/// at most a [`CIRCLE_SEGMENTS`]th of a turn — at least one.
fn arc(
    centre: DVec3,
    (u, v): (DVec3, DVec3),
    radius: f64,
    (from, to): (f64, f64),
    line: &mut impl FnMut(DVec3, DVec3),
) {
    let point = |angle: f64| centre + (u * angle.cos() + v * angle.sin()) * radius;
    let segments = (((to - from).abs() / TAU) * CIRCLE_SEGMENTS as f64)
        .ceil()
        .max(1.0) as usize;
    let step = (to - from) / segments as f64;
    for index in 0..segments {
        let (a, b) = (from + step * index as f64, from + step * (index + 1) as f64);
        line(point(a), point(b));
    }
}
