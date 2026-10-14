//! A synthetic humanoid for the ragdoll tests: twenty joints in centimetres,
//! placed in the world by a model transform that scales them to metres — the
//! shape of an imported character whose skeleton frame carries its scale.

#![allow(
    dead_code,
    reason = "each test binary uses its own part of the fixture"
)]

use crcbl_anim::{Joint, Palette, Pose, Skeleton, Trs};
use crcbl_ecs::Entity;
use crcbl_phys::{ContactSettings, GravityForce, PhysicsSystem, SurfaceMaterial};
use glam::{DVec3, Mat4, Vec3};

/// Each joint's name, parent and offset from its parent at rest, in
/// centimetres. Palette order, parents first.
pub const BONES: [(&str, Option<usize>, [f32; 3]); 20] = [
    ("hips", None, [0.0, 100.0, 0.0]),
    ("spine", Some(0), [0.0, 15.0, 0.0]),
    ("chest", Some(1), [0.0, 20.0, 0.0]),
    ("neck", Some(2), [0.0, 20.0, 0.0]),
    ("head", Some(3), [0.0, 10.0, 0.0]),
    ("head_end", Some(4), [0.0, 20.0, 0.0]),
    ("clavicle_l", Some(2), [5.0, 15.0, 0.0]),
    ("upperarm_l", Some(6), [12.0, 0.0, 0.0]),
    ("forearm_l", Some(7), [28.0, 0.0, 0.0]),
    ("hand_l", Some(8), [25.0, 0.0, 0.0]),
    ("clavicle_r", Some(2), [-5.0, 15.0, 0.0]),
    ("upperarm_r", Some(10), [-12.0, 0.0, 0.0]),
    ("forearm_r", Some(11), [-28.0, 0.0, 0.0]),
    ("hand_r", Some(12), [-25.0, 0.0, 0.0]),
    ("thigh_l", Some(0), [10.0, -5.0, 0.0]),
    ("calf_l", Some(14), [0.0, -45.0, 0.0]),
    ("foot_l", Some(15), [0.0, -45.0, 0.0]),
    ("thigh_r", Some(0), [-10.0, -5.0, 0.0]),
    ("calf_r", Some(17), [0.0, -45.0, 0.0]),
    ("foot_r", Some(18), [0.0, -45.0, 0.0]),
];

/// Centimetres to metres.
pub const SCALE: f32 = 0.01;

/// A leg: a sphere at the hips, a capsule thigh and calf hinged at the knee,
/// and a box foot welded on. The thigh is fat enough to overlap the hips.
pub const LEG: &str = r#"(
    bodies: [
        (bone: "hips", shape: Sphere(radius: 0.12), mass: 12.0),
        (
            bone: "thigh_l",
            shape: Capsule(radius: 0.08, half_height: 0.14),
            mass: 8.0,
            centre: (0.0, -0.225, 0.0),
        ),
        (
            bone: "calf_l",
            shape: Capsule(radius: 0.06, half_height: 0.15),
            mass: 4.0,
            centre: (0.0, -0.225, 0.0),
        ),
        (bone: "foot_l", shape: Box(half_extents: (0.05, 0.04, 0.1)), mass: 1.0),
    ],
    joints: [
        (
            parent: "hips",
            child: "thigh_l",
            kind: Ball(cone: 1.2, twist_lower: -0.4, twist_upper: 0.4),
            axis: (0.0, -1.0, 0.0),
            friction: 0.5,
        ),
        (
            parent: "thigh_l",
            child: "calf_l",
            kind: Hinge(lower: -2.4, upper: 0.0),
            axis: (1.0, 0.0, 0.0),
        ),
        (parent: "calf_l", child: "foot_l", kind: Fixed, axis: (0.0, 0.0, 1.0)),
    ],
    substeps: 6,
)"#;

pub fn names() -> Vec<&'static str> {
    BONES.iter().map(|(name, _, _)| *name).collect()
}

pub fn index(name: &str) -> usize {
    BONES
        .iter()
        .position(|(bone, _, _)| *bone == name)
        .expect("a fixture bone")
}

/// The skeleton, its inverse bind matrices from the rest pose.
pub fn skeleton() -> Skeleton {
    let mut globals: Vec<Mat4> = Vec::with_capacity(BONES.len());
    let joints = BONES
        .iter()
        .map(|&(_, parent, offset)| {
            let rest = Trs {
                translation: Vec3::from(offset),
                ..Trs::IDENTITY
            };
            let global = parent.map_or(Mat4::IDENTITY, |p| globals[p]) * rest.to_mat4();
            globals.push(global);
            Joint {
                parent,
                inverse_bind: global.inverse(),
                rest,
            }
        })
        .collect();
    Skeleton::new(joints).expect("parents come first")
}

/// The model transform: metres, turned a quarter about `y` and standing
/// `lift` metres up at `(2, 0, -1)`.
pub fn model(lift: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(2.0, lift, -1.0))
        * Mat4::from_rotation_y(core::f32::consts::FRAC_PI_2)
        * Mat4::from_scale(Vec3::splat(SCALE))
}

/// A system with contacts, gravity and a floor at `y = 0`.
pub fn world() -> PhysicsSystem {
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    physics.add_force_provider(Box::new(GravityForce::EARTH));
    physics.add_plane(DVec3::Y, 0.0, SurfaceMaterial::new(0.8, 0.0));
    physics
}

/// `count` entities from index `first`.
pub fn entities(first: u32, count: usize) -> Vec<Entity> {
    (first..)
        .take(count)
        .map(|index| {
            Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
        })
        .collect()
}

/// The world position of every joint in `pose` placed by `model`.
pub fn joint_positions(skeleton: &Skeleton, pose: &Pose, model: Mat4) -> Vec<Vec3> {
    let mut palette = Palette::new(skeleton);
    palette.compute(skeleton, pose);
    palette
        .globals()
        .iter()
        .map(|global| (model * *global).w_axis.truncate())
        .collect()
}
