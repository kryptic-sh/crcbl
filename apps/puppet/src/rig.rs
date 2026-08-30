//! The character's rig: a greybox humanoid, its skeleton, and its two clips —
//! authored here in code, with no asset on disk.
//!
//! ```text
//!                 ┌──────┐  1.80        joints, in palette order
//!                 │      │
//!         ┌─┐     │chest │     ┌─┐      0 root    1 hips     2 chest
//!         │ │     │      │     │ │      3 l.thigh 4 l.shin
//!         │a│     ├──────┤  1.35        5 r.thigh 6 r.shin
//!         │r│     │      │     │a│      7 l.arm   8 r.arm
//!         │m│     │ hips │     │r│
//!         └─┘     ├──┬───┤  0.90 └─┘
//!                 │  │   │
//!                 │  │   │  0.45  ← knees
//!                 │  │   │
//!                 └──┴───┘  0.00  ← feet, and the character's own origin
//! ```
//!
//! # Why there is no `.glb` here
//!
//! `docs/plan/sample/09-puppet.md` names a stock glTF character as the
//! asset-pipeline honesty check, and that is milestone 5's job — a real file,
//! imported with zero manual fixup. What milestone 2 needs is a rig that
//! **exercises the blend layer and the skinning dispatch**, and for that a
//! binary asset would buy nothing and cost the licence question, the size in
//! every wasm bundle, and a test that cannot say what it is looking at. Every
//! number below is a constant a test can assert against.
//!
//! It is also why this module does not go through glTF *at all*, which is where
//! it differs from `apps/viewer/src/demo_model.rs`. That file authors a glTF
//! document because the thing it is testing is the **importer**; here the
//! importer is not in the picture, `crcbl::anim`'s types are constructible
//! directly, and routing through a parse would mean turning on the `scene`
//! feature — which this sample's manifest turns down by name, because it drags
//! a glTF parser into a browser build that has nothing to parse.
//!
//! # One mesh per limb, and each limb spans two joints
//!
//! The character is five boxes: a torso, two legs and two arms. Each is its own
//! resident mesh and its own skinned range, and each is bound across **two**
//! joints — the leg between its thigh and its shin, the arm between the chest
//! and the arm, the torso between the hips and the chest.
//!
//! Spanning two joints is the whole point of the arrangement. A box bound
//! wholly to one joint is a rigid body, and a rigid body is something an
//! instance transform can draw with no skinning in the frame at all — so a
//! character built that way would light up every check while proving nothing.
//! Bound across two, the box shears as the joints part, and no single transform
//! produces that picture.
//!
//! A cuboid has vertices only at its two ends, so every vertex sits wholly on
//! the nearer of its limb's two joints and the weights here are all `1`. The
//! **shear between the ends** is what the dispatch produces; intermediate
//! weights would need rings up the limb, which greybox's primitives do not cut
//! and which would change none of what this milestone is for.
//!
//! # The clips
//!
//! [`idle`] is a **stance and not a motion**: one keyframe, a relaxed stand.
//! That is deliberate and the browser gate depends on it — "the character
//! settles when it stands" is only a claim a pose can be held to if standing
//! still is a fixed pose. [`walk`] is a one-second stride: legs swinging,
//! knees bending, arms counter-swinging and the hips dropping twice.
//!
//! Both are authored so that their first and last keyframes agree, which is
//! what lets [`crate::anim`] wrap the phase without a jump at the seam.

use std::borrow::Cow;

use crcbl::anim::{Channel, Clip, Interpolation, Joint, Skeleton, Track, Trs};
use crcbl::greybox::platform;
use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::scene::Geometry;
use crcbl::shaders::mesh::VERTEX_STRIDE;
use crcbl::shaders::skinning::SkinBinding;

// ---------------------------------------------------------------------------
// The proportions
// ---------------------------------------------------------------------------

/// How tall the character is, feet to crown, in metres.
///
/// [`crate::map::CHARACTER_HEIGHT`] — the height of the capsule the controller
/// actually sweeps. The drawn character is built to it rather than to a number
/// of its own, and `the_rig_fits_the_capsule_that_moves_it` is what holds the
/// two together.
pub const HEIGHT: f32 = crate::map::CHARACTER_HEIGHT as f32;

/// How far the drawn character may reach from its own axis, in metres — the
/// radius of that same capsule. Nothing here may stick out past it, or the
/// picture says the character is somewhere the collider is not.
pub const REACH: f32 = crate::map::CHARACTER_RADIUS as f32;

/// The height of the hip joint, and of the knees and the chest, in metres above
/// the feet.
const HIP_Y: f32 = 0.5 * HEIGHT;
/// See [`HIP_Y`].
const KNEE_Y: f32 = 0.25 * HEIGHT;
/// See [`HIP_Y`].
const CHEST_Y: f32 = 0.75 * HEIGHT;
/// The height of the shoulder joints, in metres above the feet.
const SHOULDER_Y: f32 = 1.55;
/// The height of the hand end of an arm, in metres above the feet.
const HAND_Y: f32 = 0.93;

/// How far to the side of the character's axis a leg hangs, in metres.
const LEG_X: f32 = 0.08;
/// How far to the side of it an arm hangs.
const ARM_X: f32 = 0.22;

/// The torso box, across `X` and through `Z`, in metres.
const TORSO_WIDTH: f32 = 0.30;
/// See [`TORSO_WIDTH`].
const TORSO_DEPTH: f32 = 0.22;
/// The side of a leg box, in metres — square in plan.
const LEG_SIDE: f32 = 0.14;
/// The side of an arm box, in metres — square in plan.
const ARM_SIDE: f32 = 0.14;

// ---------------------------------------------------------------------------
// The skeleton
// ---------------------------------------------------------------------------

/// The joint every other one hangs from, at the character's own origin.
pub const ROOT: usize = 0;
/// The pelvis. Where the legs and the torso meet, and the joint the walk bobs.
pub const HIPS: usize = 1;
/// The upper body, which the arms hang from.
pub const CHEST: usize = 2;
/// The left thigh, hinging at the hip.
pub const LEFT_THIGH: usize = 3;
/// The left shin, hinging at the knee.
pub const LEFT_SHIN: usize = 4;
/// The right thigh.
pub const RIGHT_THIGH: usize = 5;
/// The right shin.
pub const RIGHT_SHIN: usize = 6;
/// The left arm, hinging at the shoulder.
pub const LEFT_ARM: usize = 7;
/// The right arm.
pub const RIGHT_ARM: usize = 8;
/// How many joints the rig has.
pub const JOINTS: usize = 9;

/// Each joint's parent and its rest transform, in palette order.
///
/// Parents come before their children, which is what [`Skeleton::new`] refuses a
/// rig for not doing, and what lets the palette be composed in one forward pass.
const REST: [(Option<usize>, Vec3); JOINTS] = [
    (None, Vec3::ZERO),
    (Some(ROOT), Vec3::new(0.0, HIP_Y, 0.0)),
    (Some(HIPS), Vec3::new(0.0, CHEST_Y - HIP_Y, 0.0)),
    (Some(HIPS), Vec3::new(LEG_X, 0.0, 0.0)),
    (Some(LEFT_THIGH), Vec3::new(0.0, KNEE_Y - HIP_Y, 0.0)),
    (Some(HIPS), Vec3::new(-LEG_X, 0.0, 0.0)),
    (Some(RIGHT_THIGH), Vec3::new(0.0, KNEE_Y - HIP_Y, 0.0)),
    (Some(CHEST), Vec3::new(ARM_X, SHOULDER_Y - CHEST_Y, 0.0)),
    (Some(CHEST), Vec3::new(-ARM_X, SHOULDER_Y - CHEST_Y, 0.0)),
];

/// The rig's joint hierarchy, with each joint's inverse bind matrix computed
/// from the rest pose above.
///
/// **The bind pose is the rest pose**, which is not something a rig has to be
/// but is what this one is: the boxes in [`parts`] are authored in the same
/// space the rest hierarchy composes to, so a joint's inverse bind is exactly
/// the inverse of where it rests. An imported rig carries the two separately
/// because an exporter can disagree about them; here nothing can, and computing
/// one from the other is what makes that true rather than merely intended.
///
/// # Panics
///
/// Never: `REST` names parents before children and no joint is its own
/// parent, which is the only thing [`Skeleton::new`] refuses, and
/// `the_rig_is_a_well_ordered_skeleton` is the check that it stays that way.
#[must_use]
pub fn skeleton() -> Skeleton {
    let mut globals = [Mat4::IDENTITY; JOINTS];
    let joints = REST
        .iter()
        .enumerate()
        .map(|(index, &(parent, translation))| {
            let rest = Trs {
                translation,
                ..Trs::IDENTITY
            };
            globals[index] = match parent {
                Some(parent) => globals[parent] * rest.to_mat4(),
                None => rest.to_mat4(),
            };
            Joint {
                parent,
                inverse_bind: globals[index].inverse(),
                rest,
            }
        })
        .collect();
    Skeleton::new(joints).expect("the rig names every parent before its child")
}

// ---------------------------------------------------------------------------
// The drawn body
// ---------------------------------------------------------------------------

/// One drawn piece of the character: a box in the rig's own space, and which
/// joint each of its vertices follows.
#[derive(Clone, Debug)]
pub struct Part {
    /// What the mesh is called in the scene description.
    pub label: &'static str,
    /// The box, positioned in bind space.
    pub geometry: Geometry<'static>,
    /// One entry per vertex of `geometry`, in the same order.
    pub bindings: Vec<SkinBinding>,
}

/// How many pieces the character is drawn as.
pub const PARTS: usize = 5;

/// The character's boxes, in the order they become meshes.
///
/// Each spans two joints, split at the height where the two meet — see the
/// module docs for why a limb bound to one joint would prove nothing.
#[must_use]
pub fn parts() -> [Part; PARTS] {
    [
        // The torso runs from the hips to the crown, and hands over at the
        // chest joint, so it bends at the waist.
        box_part(
            "torso",
            (TORSO_WIDTH, TORSO_DEPTH),
            Vec3::new(0.0, HIP_Y, 0.0),
            HEIGHT - HIP_Y,
            (HIPS, CHEST),
            CHEST_Y,
        ),
        box_part(
            "left leg",
            (LEG_SIDE, LEG_SIDE),
            Vec3::new(LEG_X, 0.0, 0.0),
            HIP_Y,
            (LEFT_SHIN, LEFT_THIGH),
            KNEE_Y,
        ),
        box_part(
            "right leg",
            (LEG_SIDE, LEG_SIDE),
            Vec3::new(-LEG_X, 0.0, 0.0),
            HIP_Y,
            (RIGHT_SHIN, RIGHT_THIGH),
            KNEE_Y,
        ),
        box_part(
            "left arm",
            (ARM_SIDE, ARM_SIDE),
            Vec3::new(ARM_X, HAND_Y, 0.0),
            SHOULDER_Y - HAND_Y,
            (LEFT_ARM, CHEST),
            SHOULDER_Y,
        ),
        box_part(
            "right arm",
            (ARM_SIDE, ARM_SIDE),
            Vec3::new(-ARM_X, HAND_Y, 0.0),
            SHOULDER_Y - HAND_Y,
            (RIGHT_ARM, CHEST),
            SHOULDER_Y,
        ),
    ]
}

/// A box `plan.0` × `plan.1` across and through, standing `height` tall with
/// its base at `base`, whose vertices below `split` follow `across.0` and whose
/// vertices at or above it follow `across.1`.
fn box_part(
    label: &'static str,
    plan: (f32, f32),
    base: Vec3,
    height: f32,
    across: (usize, usize),
    split: f32,
) -> Part {
    let (lower, upper) = across;
    let geometry = translated(platform(plan.0, plan.1, height), base);
    let bindings = positions(&geometry)
        .map(|position| SkinBinding {
            joints: [
                u32::try_from(if position.y < split { lower } else { upper })
                    .expect("a rig with nine joints indexes them in a u32"),
                0,
                0,
                0,
            ],
            // A cuboid's vertices are all at one end or the other, so each one
            // follows a single joint whole; see the module docs.
            weights: [1.0, 0.0, 0.0, 0.0],
        })
        .collect();
    Part {
        label,
        geometry,
        bindings,
    }
}

/// The same geometry with every vertex, and every cluster's bounding sphere,
/// moved by `offset`.
///
/// `crcbl::greybox`'s primitives are all centred on `X`/`Z` with their base on
/// `y = 0`, and a limb needs to sit where the limb is. The **bounds move too**:
/// a cluster's centre is in the positions' own space, so shifting the positions
/// and leaving the sphere behind would hand the culler a bound that does not
/// contain the geometry it stands for.
///
/// # Panics
///
/// If handed a level-of-detail hierarchy rather than a flat mesh. Every
/// `crcbl::greybox` primitive is flat, and a DAG passed through here silently
/// untranslated would be a limb drawn at the origin.
fn translated(geometry: Geometry<'static>, offset: Vec3) -> Geometry<'static> {
    let Geometry::Flat {
        vertices,
        uv_range,
        indices,
        mut clusters,
        flags,
    } = geometry
    else {
        panic!("crcbl::greybox's primitives are flat meshes, and this one was not")
    };
    let mut bytes = vertices.into_owned();
    for vertex in bytes.chunks_exact_mut(VERTEX_STRIDE) {
        for (lane, delta) in [offset.x, offset.y, offset.z].into_iter().enumerate() {
            let at = lane * size_of::<f32>();
            let moved = read_f32(&bytes_at(vertex, at)) + delta;
            vertex[at..at + size_of::<f32>()].copy_from_slice(&moved.to_le_bytes());
        }
    }
    for meshlet in &mut clusters.clusters {
        meshlet.bounds.center[0] += offset.x;
        meshlet.bounds.center[1] += offset.y;
        meshlet.bounds.center[2] += offset.z;
    }
    Geometry::Flat {
        vertices: Cow::Owned(bytes),
        // Untouched: a translation moves positions, and the UV lanes and the
        // range they decode through are the same either side of it.
        uv_range,
        indices,
        clusters,
        // Carried through rather than re-decided: a translation moves
        // positions and leaves the tangent frame exactly as the primitive
        // built it, so whatever it claimed it still claims.
        flags,
    }
}

/// The four bytes at `at`, which the caller has already sized to a vertex.
#[inline]
fn bytes_at(vertex: &[u8], at: usize) -> [u8; size_of::<f32>()] {
    vertex[at..at + size_of::<f32>()]
        .try_into()
        .expect("a vertex is a whole number of f32 lanes")
}

/// One little-endian `f32`, which is how every vertex lane reaches the device.
#[inline]
fn read_f32(bytes: &[u8; size_of::<f32>()]) -> f32 {
    f32::from_le_bytes(*bytes)
}

/// Every vertex position of a flat mesh, in vertex order.
///
/// # Panics
///
/// If handed a level-of-detail hierarchy — see [`translated`].
fn positions<'a>(geometry: &'a Geometry<'static>) -> impl Iterator<Item = Vec3> + 'a {
    let Geometry::Flat { vertices, .. } = geometry else {
        panic!("crcbl::greybox's primitives are flat meshes, and this one was not")
    };
    vertices.chunks_exact(VERTEX_STRIDE).map(|vertex| {
        Vec3::new(
            read_f32(&bytes_at(vertex, 0)),
            read_f32(&bytes_at(vertex, size_of::<f32>())),
            read_f32(&bytes_at(vertex, 2 * size_of::<f32>())),
        )
    })
}

// ---------------------------------------------------------------------------
// The clips
// ---------------------------------------------------------------------------

/// How long one stride of [`walk`] lasts, in seconds.
///
/// A *stride* and not a step: both feet come forward once inside it, which is
/// why the hips drop twice.
pub const WALK_CYCLE_S: f32 = 1.0;

/// How much ground one stride of [`walk`] covers, in metres.
///
/// The boxes have no feet to plant, so this is authored rather than measured —
/// but it is the number the clip *means*, and two things read it as one:
/// [`crate::anim`] turns the phase over once per this much ground so the legs
/// do not skate, and it divides by [`WALK_CYCLE_S`] to give the speed the clip
/// is authored for, which is where the top of the locomotion set sits.
pub const STRIDE_M: f32 = 2.6;

/// How far a thigh swings from vertical at the extremes of the stride, in
/// radians. A positive rotation about `+X` carries the leg toward `-Z`, which
/// is the direction this engine calls forward.
const THIGH_SWING: f32 = 0.55;
/// How far an arm swings, in radians — less than the legs, and out of phase
/// with the leg on its own side.
const ARM_SWING: f32 = 0.45;
/// How far the arms hang out from the body, in radians about `+Z`.
const ARM_SPLAY: f32 = 0.12;
/// How far the hips drop at the bottom of each step, in metres.
const HIP_BOB: f32 = 0.04;
/// How far the chest twists against the stride, in radians about `+Y`.
const CHEST_TWIST: f32 = 0.08;

/// The stance the character holds when it is standing still.
///
/// **One keyframe, so the pose does not move.** See the module docs: the
/// browser gate's "and settles when it stands" is a claim about the pose
/// itself, and it can only be made against an idle that is a stance.
///
/// It is not the rest pose either — the knees are bent and the arms hang out —
/// so a character standing still is visibly *posed by a clip* rather than
/// merely un-posed, which is what makes the reading the gate settles on a
/// non-zero one.
#[must_use]
pub fn idle() -> Clip {
    let held = |joint: usize, rotation: Quat| {
        Channel::new(
            joint,
            vec![0.0],
            Interpolation::Linear,
            Track::Rotation(vec![rotation]),
        )
        .expect("one keyframe and one value")
    };
    Clip::new(vec![
        held(LEFT_THIGH, Quat::from_rotation_x(0.06)),
        held(RIGHT_THIGH, Quat::from_rotation_x(0.06)),
        held(LEFT_SHIN, Quat::from_rotation_x(-0.12)),
        held(RIGHT_SHIN, Quat::from_rotation_x(-0.12)),
        held(LEFT_ARM, Quat::from_rotation_z(ARM_SPLAY)),
        held(RIGHT_ARM, Quat::from_rotation_z(-ARM_SPLAY)),
    ])
}

/// One stride, over [`WALK_CYCLE_S`].
///
/// The first and last keyframes of every channel agree, so the phase wraps
/// without a jump.
#[must_use]
pub fn walk() -> Clip {
    // Quarters of the stride: contact, mid-swing, contact again, mid-swing.
    let beats = vec![
        0.0,
        0.25 * WALK_CYCLE_S,
        0.5 * WALK_CYCLE_S,
        0.75 * WALK_CYCLE_S,
        WALK_CYCLE_S,
    ];
    let turning = |joint: usize, angles: [f32; 5], axis: fn(f32) -> Quat| {
        Channel::new(
            joint,
            beats.clone(),
            Interpolation::Linear,
            Track::Rotation(angles.into_iter().map(axis).collect()),
        )
        .expect("five keyframes and five values")
    };
    let swinging = |joint: usize, angles: [f32; 5], splay: f32| {
        Channel::new(
            joint,
            beats.clone(),
            Interpolation::Linear,
            Track::Rotation(
                angles
                    .into_iter()
                    .map(|angle| Quat::from_rotation_z(splay) * Quat::from_rotation_x(angle))
                    .collect(),
            ),
        )
        .expect("five keyframes and five values")
    };
    let s = THIGH_SWING;
    let a = ARM_SWING;
    Clip::new(vec![
        turning(
            LEFT_THIGH,
            [s, 0.0, -s, 0.0, s],
            Quat::from_rotation_x as fn(f32) -> Quat,
        ),
        turning(RIGHT_THIGH, [-s, 0.0, s, 0.0, -s], Quat::from_rotation_x),
        // A knee only bends one way, so every value is negative: the shin comes
        // up behind, never through the front of the leg.
        turning(
            LEFT_SHIN,
            [-0.15, -0.70, -0.10, -0.25, -0.15],
            Quat::from_rotation_x,
        ),
        turning(
            RIGHT_SHIN,
            [-0.10, -0.25, -0.15, -0.70, -0.10],
            Quat::from_rotation_x,
        ),
        // Arms counter-swing: the left arm goes back as the left leg comes
        // forward, which is what stops a walk reading as a march.
        swinging(LEFT_ARM, [-a, 0.0, a, 0.0, -a], ARM_SPLAY),
        swinging(RIGHT_ARM, [a, 0.0, -a, 0.0, a], -ARM_SPLAY),
        turning(
            CHEST,
            [0.0, CHEST_TWIST, 0.0, -CHEST_TWIST, 0.0],
            Quat::from_rotation_y,
        ),
        // The hips drop at the bottom of each step — twice per stride, which is
        // why this channel has the beats the legs do and half their period.
        Channel::new(
            HIPS,
            beats.clone(),
            Interpolation::Linear,
            Track::Translation(
                [0.0, -HIP_BOB, 0.0, -HIP_BOB, 0.0]
                    .into_iter()
                    .map(|drop| Vec3::new(0.0, HIP_Y + drop, 0.0))
                    .collect(),
            ),
        )
        .expect("five keyframes and five values"),
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        HEIGHT, JOINTS, PARTS, REACH, WALK_CYCLE_S, idle, parts, positions, skeleton, walk,
    };
    use crcbl::anim::{Palette, Pose};
    use crcbl::math::Vec3;

    #[test]
    fn the_rig_is_a_well_ordered_skeleton() {
        let skeleton = skeleton();
        assert_eq!(skeleton.len(), JOINTS);
        for (index, joint) in skeleton.joints().iter().enumerate() {
            if let Some(parent) = joint.parent {
                assert!(parent < index, "joint {index} hangs from a later joint");
            }
        }
    }

    /// The bind pose composes to the identity palette: every joint's global
    /// rest transform times its own inverse bind is the identity, so a
    /// character nothing has posed is drawn exactly as its boxes were authored.
    #[test]
    fn the_bind_pose_skins_to_the_geometry_as_authored() {
        let skeleton = skeleton();
        let pose = Pose::new(&skeleton);
        let mut palette = Palette::new(&skeleton);
        palette.compute(&skeleton, &pose);
        for (index, matrix) in palette.matrices().iter().enumerate() {
            let error = (*matrix - crcbl::math::Mat4::IDENTITY).to_cols_array();
            let worst = error
                .iter()
                .fold(0.0_f32, |worst, term| worst.max(term.abs()));
            assert!(
                worst < 1e-5,
                "joint {index}'s bind palette is off the identity by {worst}"
            );
        }
    }

    /// **The drawn character fits the capsule that moves it.** A mesh larger
    /// than the collider is a picture that lies about where the character is,
    /// and this is the check `apps/puppet` has always made about its body —
    /// carried over from the capsule the rig replaced.
    #[test]
    fn the_rig_fits_the_capsule_that_moves_it() {
        let (mut low, mut high, mut reach) = (f32::MAX, f32::MIN, 0.0_f32);
        for part in &parts() {
            for position in positions(&part.geometry) {
                low = low.min(position.y);
                high = high.max(position.y);
                reach = reach.max(Vec3::new(position.x, 0.0, position.z).length());
            }
        }
        assert!(
            low >= -1e-5,
            "the character reaches below its own feet: {low}"
        );
        assert!(
            high <= HEIGHT + 1e-5,
            "the character is {high} m tall and the capsule is {HEIGHT} m"
        );
        assert!(
            reach <= REACH + 1e-5,
            "the character reaches {reach} m from its axis and the capsule's radius is {REACH} m"
        );
    }

    /// Every part is bound across **two** joints. A part on one joint is a
    /// rigid body an instance transform could draw, and a rig made of those
    /// would light up the skinning checks while proving nothing — see the
    /// module docs.
    #[test]
    fn every_part_spans_two_joints() {
        for part in &parts() {
            let mut seen: Vec<u32> = part
                .bindings
                .iter()
                .map(|binding| binding.joints[0])
                .collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(
                seen.len(),
                2,
                "{} is bound to {seen:?}, which is not two joints",
                part.label
            );
        }
    }

    #[test]
    fn every_part_has_one_binding_per_vertex() {
        for part in &parts() {
            assert_eq!(
                part.bindings.len(),
                positions(&part.geometry).count(),
                "{}'s bindings and vertices disagree",
                part.label
            );
            for binding in &part.bindings {
                assert!(
                    (binding.weights.iter().sum::<f32>() - 1.0).abs() < 1e-6,
                    "{}'s weights do not sum to one",
                    part.label
                );
                assert!(
                    binding
                        .joints
                        .iter()
                        .all(|&joint| (joint as usize) < JOINTS),
                    "{} names a joint the rig has not got",
                    part.label
                );
            }
        }
    }

    #[test]
    fn there_are_as_many_parts_as_the_constant_says() {
        assert_eq!(parts().len(), PARTS);
    }

    /// The idle clip is a stance: sampling it at any phase gives one pose. The
    /// browser gate's settle check is a claim about exactly this.
    #[test]
    fn the_idle_clip_holds_one_pose() {
        let skeleton = skeleton();
        let clip = idle();
        assert_eq!(clip.duration(), 0.0);
        let mut first = Pose::new(&skeleton);
        let mut later = Pose::new(&skeleton);
        clip.sample_into(0.0, &skeleton, &mut first);
        clip.sample_into(37.5, &skeleton, &mut later);
        assert_eq!(first, later);
    }

    /// And it is not the rest pose, so a standing character is visibly posed by
    /// a clip rather than merely un-posed.
    #[test]
    fn the_idle_stance_is_not_the_rest_pose() {
        let skeleton = skeleton();
        let rest = Pose::new(&skeleton);
        let mut stance = Pose::new(&skeleton);
        idle().sample_into(0.0, &skeleton, &mut stance);
        assert_ne!(stance, rest);
    }

    /// The walk loops: its first and last keyframes agree, so wrapping the
    /// phase does not jump.
    #[test]
    fn the_walk_clip_meets_itself_at_the_seam() {
        let skeleton = skeleton();
        let clip = walk();
        assert_eq!(clip.duration(), WALK_CYCLE_S);
        let mut start = Pose::new(&skeleton);
        let mut end = Pose::new(&skeleton);
        clip.sample_into(0.0, &skeleton, &mut start);
        clip.sample_into(WALK_CYCLE_S, &skeleton, &mut end);
        assert_eq!(start, end);
    }

    /// And it moves: the feet are somewhere different a quarter of the way
    /// through than they are at the contact pose.
    #[test]
    fn the_walk_clip_carries_the_feet() {
        let skeleton = skeleton();
        let clip = walk();
        let mut pose = Pose::new(&skeleton);
        let mut palette = Palette::new(&skeleton);
        let foot = |palette: &Palette, joint: usize| {
            palette.globals()[joint].transform_point3(Vec3::new(0.0, -0.45, 0.0))
        };

        clip.sample_into(0.0, &skeleton, &mut pose);
        palette.compute(&skeleton, &pose);
        let contact = foot(&palette, super::LEFT_SHIN);

        clip.sample_into(0.25 * WALK_CYCLE_S, &skeleton, &mut pose);
        palette.compute(&skeleton, &pose);
        let swing = foot(&palette, super::LEFT_SHIN);

        assert!(
            (contact - swing).length() > 0.1,
            "the left foot barely moved across a quarter stride: {contact:?} then {swing:?}"
        );
    }
}
