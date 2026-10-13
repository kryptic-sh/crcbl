//! Skeleton joints as rigid frames, and the bodies placed on them.

use crcbl_anim::{Palette, Pose, Skeleton};
use crcbl_phys::Transform;
use crcbl_phys::contact::manifold::collide;
use crcbl_phys::contact::shape::ContactShape;
use glam::{Mat4, Vec3};

use crate::rig::{RigBody, RigJoint, compose};

/// How close two unjointed bodies' rest shapes may come, in metres, before a
/// generated rig, or an authored one with `ignore_rest_overlaps`, keeps them
/// from colliding.
pub const REST_CLEARANCE: f64 = 0.005;

/// Every joint's rigid frame for `pose` placed by `model`: its world rotation
/// and origin, scale taken out. `Err` names the first joint whose world
/// transform has no finite rotation or a degenerate scale.
pub(crate) fn rigid_frames(
    skeleton: &Skeleton,
    pose: &Pose,
    model: Mat4,
) -> Result<Vec<Transform>, usize> {
    let mut palette = Palette::new(skeleton);
    palette.compute(skeleton, pose);
    palette
        .globals()
        .iter()
        .enumerate()
        .map(|(joint, global)| {
            let world = model * *global;
            let (scale, rotation, translation) = world.to_scale_rotation_translation();
            let usable = world.is_finite()
                && scale.cmpgt(Vec3::splat(f32::EPSILON)).all()
                && rotation.is_finite()
                && rotation.length_squared() > 0.0;
            if !usable {
                return Err(joint);
            }
            Ok(Transform::new(
                translation.as_dvec3(),
                rotation.as_dquat().normalize(),
            ))
        })
        .collect()
}

/// Each body's world transform, given its joint's rigid frame.
pub(crate) fn body_worlds(bodies: &[RigBody], frames: &[Transform]) -> Vec<Transform> {
    bodies
        .iter()
        .map(|body| compose(&frames[body.joint], &body.offset))
        .collect()
}

impl crate::RagdollRig {
    /// Where each body of this rig stands, in rig order, for `skeleton` in
    /// `pose` placed by `model` — what an editor draws a rig with before
    /// anything is simulated, or a game checks a spawn against.
    ///
    /// `None` for a skeleton of another size than the rig was made for, or a
    /// pose leaving a joint with no usable frame.
    #[must_use]
    pub fn bodies_at(
        &self,
        skeleton: &Skeleton,
        pose: &Pose,
        model: Mat4,
    ) -> Option<Vec<Transform>> {
        if skeleton.len() != self.joint_count() || pose.len() != skeleton.len() {
            return None;
        }
        let frames = rigid_frames(skeleton, pose, model).ok()?;
        Some(body_worlds(self.bodies(), &frames))
    }

    /// The body pairs, lower index first, that no joint joins and that come
    /// within [`REST_CLEARANCE`] of each other for `skeleton` in `pose`
    /// placed by `model` — what `ignore_rest_overlaps` keeps from colliding,
    /// asked of the rig whether it was resolved with the rule or without.
    /// An editor lists them to offer the pairs worth naming in a file.
    ///
    /// `None` where [`bodies_at`](Self::bodies_at) is.
    #[must_use]
    pub fn rest_overlaps(
        &self,
        skeleton: &Skeleton,
        pose: &Pose,
        model: Mat4,
    ) -> Option<Vec<(usize, usize)>> {
        let worlds = self.bodies_at(skeleton, pose, model)?;
        Some(overlapping_pairs(self.bodies(), self.joints(), &worlds))
    }
}

/// The body pairs, lower index first, that no joint joins and whose shapes
/// at `worlds` come within [`REST_CLEARANCE`] of each other: pairs that
/// would push each other apart the moment they were simulated.
pub(crate) fn overlapping_pairs(
    bodies: &[RigBody],
    joints: &[RigJoint],
    worlds: &[Transform],
) -> Vec<(usize, usize)> {
    let shapes: Vec<Option<ContactShape>> = bodies
        .iter()
        .zip(worlds)
        .map(|(body, world)| ContactShape::placed(&body.shape.collider(), world))
        .collect();
    let jointed = |a: usize, b: usize| {
        joints
            .iter()
            .any(|j| (j.parent, j.child) == (a, b) || (j.parent, j.child) == (b, a))
    };
    let mut pairs = Vec::new();
    for a in 0..bodies.len() {
        for b in a + 1..bodies.len() {
            let (Some(first), Some(second)) = (&shapes[a], &shapes[b]) else {
                continue;
            };
            if jointed(a, b) {
                continue;
            }
            let (low, high) = if first.rank() <= second.rank() {
                (first, second)
            } else {
                (second, first)
            };
            let close = collide(low, high, REST_CLEARANCE)
                .points()
                .iter()
                .any(|point| point.separation < REST_CLEARANCE);
            if close {
                pairs.push((a, b));
            }
        }
    }
    pairs
}
