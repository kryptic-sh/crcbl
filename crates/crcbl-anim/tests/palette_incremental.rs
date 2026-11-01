//! [`Palette::compute`] recomposes only the joints that changed and their
//! descendants; these hold it to the full composition, bit for bit.
//!
//! The reference is the full walk as it was before the palette learned to skip
//! joints, kept here verbatim. Every comparison is of bit patterns, not of
//! values: `-0.0` against `0.0` and a `NaN` against itself are both cases the
//! skip has to get right, and `==` would excuse the first and fail the second.

use crcbl_anim::{
    Joint, Palette, Pose, Skeleton, Trs, rotate_joint, rotate_joints, solve_two_bone,
};
use crcbl_rand::Rng;
use glam::{Mat4, Quat, Vec3};

/// The palette's full composition as it stood before it skipped anything:
/// every joint, forwards, `global = parent · local`, `skin = global · bind⁻¹`.
fn reference(skeleton: &Skeleton, pose: &Pose) -> (Vec<Mat4>, Vec<Mat4>) {
    let mut globals = vec![Mat4::IDENTITY; skeleton.len()];
    let mut skinning = vec![Mat4::IDENTITY; skeleton.len()];
    for (index, joint) in skeleton.joints().iter().enumerate() {
        let local = pose.locals()[index].to_mat4();
        let global = match joint.parent {
            Some(parent) => globals[parent] * local,
            None => local,
        };
        globals[index] = global;
        skinning[index] = global * joint.inverse_bind;
    }
    (globals, skinning)
}

fn bits(matrices: &[Mat4]) -> Vec<[u32; 16]> {
    matrices
        .iter()
        .map(|matrix| matrix.to_cols_array().map(f32::to_bits))
        .collect()
}

/// The first joint whose globals or skinning differ from the reference in
/// any bit, or `None`.
fn first_difference(palette: &Palette, skeleton: &Skeleton, pose: &Pose) -> Option<usize> {
    let (globals, skinning) = reference(skeleton, pose);
    let (want_globals, want_skinning) = (bits(&globals), bits(&skinning));
    let (got_globals, got_skinning) = (bits(palette.globals()), bits(palette.matrices()));
    (0..skeleton.len()).find(|&joint| {
        got_globals[joint] != want_globals[joint] || got_skinning[joint] != want_skinning[joint]
    })
}

fn assert_matches_reference(palette: &Palette, skeleton: &Skeleton, pose: &Pose, what: &str) {
    if let Some(joint) = first_difference(palette, skeleton, pose) {
        panic!(
            "{what}: joint {joint} differs from the full composition\n  palette global {:?}\n  pose local     {:?}",
            palette.globals()[joint],
            pose.locals()[joint]
        );
    }
}

/// A float in `[-range, range)`.
fn uniform(rng: &mut Rng, range: f32) -> f32 {
    let unit = (rng.next_u32() >> 8) as f32 / (1 << 24) as f32;
    (unit * 2.0 - 1.0) * range
}

fn vector(rng: &mut Rng, range: f32) -> Vec3 {
    Vec3::new(
        uniform(rng, range),
        uniform(rng, range),
        uniform(rng, range),
    )
}

/// A random local transform, now and then spelled with the values whose bits
/// a value comparison would get wrong: signed zeros and a `NaN`.
fn local(rng: &mut Rng) -> Trs {
    let mut trs = Trs {
        translation: vector(rng, 2.0),
        rotation: Quat::from_scaled_axis(vector(rng, 3.0)),
        scale: Vec3::splat(0.5) + vector(rng, 0.25).abs(),
    };
    match rng.next_u32() % 8 {
        0 => trs.translation = Vec3::new(0.0, -0.0, 0.0),
        1 => trs.rotation = Quat::from_xyzw(-0.0, 0.0, -0.0, 1.0),
        2 => trs.scale.x = -trs.scale.x,
        3 if rng.next_u32().is_multiple_of(4) => trs.translation.y = f32::NAN,
        _ => {}
    }
    trs
}

/// Changes `trs` in one way a palette must notice: a new value, or only the
/// sign of a zero, or only the payload of a `NaN`.
fn nudge(rng: &mut Rng, trs: &mut Trs) {
    match rng.next_u32() % 4 {
        0 => *trs = local(rng),
        1 => {
            trs.translation.x = if trs.translation.x.to_bits() == 0 {
                -0.0
            } else {
                0.0
            }
        }
        2 => trs.translation.z = f32::from_bits(0x7fc0_0000 | (rng.next_u32() & 0xff)),
        _ => trs.rotation = Quat::from_scaled_axis(vector(rng, 3.0)),
    }
}

/// A random skeleton of `joints` joints: each parent drawn from the joints
/// before it, or none, and each inverse bind an arbitrary invertible-looking
/// matrix with a non-trivial bottom row, so nothing about it is assumed affine.
fn skeleton(rng: &mut Rng, joints: usize) -> Skeleton {
    let joints = (0..joints)
        .map(|index| {
            let parent = (index > 0 && !rng.next_u32().is_multiple_of(7))
                .then(|| rng.next_u32() as usize % index);
            let mut inverse_bind =
                Mat4::from_scale_rotation_translation(Vec3::ONE, Quat::IDENTITY, vector(rng, 1.0));
            inverse_bind.x_axis.w = uniform(rng, 0.01);
            Joint {
                parent,
                inverse_bind,
                rest: local(rng),
            }
        })
        .collect();
    Skeleton::new(joints).expect("every parent precedes its child")
}

/// **The property.** Across seeded skeletons, one palette kept across a run
/// of edits — each a random set of joints changed in a random way — equals
/// the full composition of the pose after every compute.
#[test]
fn an_incremental_compute_is_the_full_composition() {
    for seed in 0..64 {
        let mut rng = Rng::from_u64(seed);
        let joints = 1 + rng.next_u32() as usize % 60;
        let skeleton = skeleton(&mut rng, joints);
        let mut pose = Pose::new(&skeleton);
        for trs in pose.locals_mut() {
            *trs = local(&mut rng);
        }
        let mut palette = Palette::new(&skeleton);
        palette.compute(&skeleton, &pose);
        assert_matches_reference(&palette, &skeleton, &pose, "first compute");
        for step in 0..40 {
            let changes = rng.next_u32() % 4;
            for _ in 0..changes {
                let joint = rng.next_u32() as usize % joints;
                nudge(&mut rng, &mut pose.locals_mut()[joint]);
            }
            palette.compute(&skeleton, &pose);
            assert_matches_reference(
                &palette,
                &skeleton,
                &pose,
                &format!("seed {seed}, step {step}, {changes} change(s)"),
            );
        }
    }
}

/// A palette handed a different skeleton of the same size recomposes all of
/// it, even for a pose whose locals it already holds.
#[test]
fn another_skeleton_recomposes_everything() {
    let mut rng = Rng::from_u64(7);
    let first = skeleton(&mut rng, 20);
    let second = skeleton(&mut rng, 20);
    let pose = Pose::new(&first);
    let mut palette = Palette::new(&first);
    palette.compute(&first, &pose);
    palette.compute(&second, &pose);
    assert_matches_reference(&palette, &second, &pose, "the second skeleton");
    // A clone is the same joints, so the palette may keep what it holds.
    let clone = second.clone();
    palette.compute(&clone, &pose);
    assert_matches_reference(&palette, &clone, &pose, "a clone of it");
}

/// A palette's first compute writes every joint, even one whose local is the
/// identity that [`Palette::new`] filled it with.
#[test]
fn the_first_compute_writes_every_joint() {
    let skeleton = Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::from_translation(Vec3::X),
            rest: Trs::IDENTITY,
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        },
    ])
    .expect("a chain");
    let pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    assert_matches_reference(&palette, &skeleton, &pose, "identity locals");
}

/// The IK calls leave the palette at the full composition of the pose they
/// wrote, with the palette kept across a sequence of them.
#[test]
fn ik_leaves_the_full_composition() {
    // A conformal chain with a branch: the IK refuses non-uniform parent
    // scales, so this skeleton's locals are rotations and offsets only.
    let offsets = [
        (None, Vec3::new(0.0, 1.0, 0.0)),
        (Some(0), Vec3::new(0.0, 0.3, 0.0)),
        (Some(1), Vec3::new(0.2, 0.1, 0.0)),
        (Some(2), Vec3::new(0.3, 0.0, 0.0)),
        (Some(3), Vec3::new(0.3, 0.0, 0.0)),
        (Some(4), Vec3::new(0.05, 0.0, 0.0)),
        (Some(1), Vec3::new(-0.2, 0.1, 0.0)),
        (Some(6), Vec3::new(-0.3, 0.0, 0.0)),
        (Some(0), Vec3::new(0.1, -0.1, 0.0)),
    ];
    let skeleton = Skeleton::new(
        offsets
            .iter()
            .map(|&(parent, translation)| Joint {
                parent,
                inverse_bind: Mat4::from_translation(-translation),
                rest: Trs {
                    translation,
                    ..Trs::IDENTITY
                },
            })
            .collect(),
    )
    .expect("parents first");
    let model = Mat4::from_translation(Vec3::new(1.0, 0.0, -2.0));
    let mut rng = Rng::from_u64(11);
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    for step in 0..50 {
        let turn = Quat::from_scaled_axis(vector(&mut rng, 0.3));
        let joint = rng.next_u32() as usize % skeleton.len();
        match step % 3 {
            0 => rotate_joint(&skeleton, model, &mut pose, &mut palette, joint, turn)
                .expect("a conformal frame"),
            1 => rotate_joints(
                &skeleton,
                model,
                &mut pose,
                &mut palette,
                &[(joint, turn), (1, turn.inverse())],
            )
            .expect("conformal frames"),
            _ => solve_two_bone(
                &skeleton,
                model,
                &mut pose,
                &mut palette,
                [2, 3, 5],
                Vec3::new(1.5, 1.4, -1.8) + vector(&mut rng, 0.2),
                Vec3::NEG_Z,
            )
            .expect("an arm"),
        }
        assert_matches_reference(&palette, &skeleton, &pose, &format!("IK step {step}"));
    }
}
