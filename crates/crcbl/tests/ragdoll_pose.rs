//! The animation pose bridge driven by the ordinary contact and joint solver.

use crcbl::anim::ragdoll::{BodyJoint, RagdollBinding};
use crcbl::anim::{Joint as Bone, Palette, Pose, Skeleton, Trs};
use crcbl::ecs::Entity;
use crcbl::math::{DVec3, Mat4, Quat, Vec3};
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, Joint, JointKind, MassProperties,
    PhysicsSystem, RigidBody, SphericalJoint, SurfaceMaterial, Transform,
};

const DT: f64 = 1.0 / 60.0;
const POSITION_TOLERANCE: f32 = 1e-4;

fn world(physics: &PhysicsSystem, entity: Entity) -> Mat4 {
    let transform = physics.transform(entity).unwrap();
    Mat4::from_rotation_translation(transform.rotation.as_quat(), transform.position.as_vec3())
}

#[test]
fn articulated_bodies_fall_to_terrain_and_drive_the_skinning_palette() {
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    physics.add_force_provider(Box::new(GravityForce::EARTH));
    physics.add_plane(DVec3::Y, 0.0, SurfaceMaterial::DEFAULT);
    let half = DVec3::new(0.3, 0.1, 0.1);
    let entities = [0_u32, 1].map(|index| {
        let entity = Entity::from_bits((1_u64 << 32) | u64::from(index)).unwrap();
        let body = RigidBody::new_dynamic(1.0)
            .with_inertia(MassProperties::cuboid(1.0, half, DVec3::ZERO).inertia);
        physics.set_body(entity, body);
        let transform = Transform::from_position(DVec3::new(f64::from(index) * 0.6, 3.0, 0.0));
        physics.set_transform(entity, transform);
        physics.set_collider(
            entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: half,
                is_trigger: false,
            },
            &transform,
        );
        entity
    });
    physics
        .add_joint(Joint::new(
            entities[0],
            entities[1],
            Transform::from_position(DVec3::new(0.3, 0.0, 0.0)),
            Transform::from_position(DVec3::new(-0.3, 0.0, 0.0)),
            JointKind::Spherical(SphericalJoint::ball()),
        ))
        .unwrap();
    let skeleton = Skeleton::new(vec![
        Bone {
            parent: None,
            inverse_bind: Mat4::from_translation(Vec3::new(0.0, -300.0, 0.0)),
            rest: Trs {
                translation: Vec3::new(0.0, 300.0, 0.0),
                ..Trs::IDENTITY
            },
        },
        Bone {
            parent: Some(0),
            inverse_bind: Mat4::from_translation(Vec3::new(-60.0, -300.0, 0.0)),
            rest: Trs {
                translation: Vec3::new(60.0, 0.0, 0.0),
                ..Trs::IDENTITY
            },
        },
    ])
    .unwrap();
    let model = Mat4::from_scale(Vec3::splat(0.01));
    let mut pose = Pose::new(&skeleton);
    let bodies = [0, 1].map(|joint| BodyJoint {
        joint,
        world: world(&physics, entities[joint]),
    });
    let mut binding = RagdollBinding::new(&skeleton, &pose, model, &bodies).unwrap();
    let previous_globals = [0, 1].map(|joint| {
        model.inverse()
            * Mat4::from_rotation_translation(
                Quat::from_rotation_z(if joint == 0 { -2.0 } else { 2.0 } * DT as f32),
                Vec3::new(joint as f32 * 0.6 - DT as f32, 3.0, 0.0),
            )
            * model
    });
    let mut previous = pose.clone();
    previous.locals_mut()[0] = Trs::from_mat4(previous_globals[0]);
    previous.locals_mut()[1] = Trs::from_mat4(previous_globals[0].inverse() * previous_globals[1]);
    let motion = binding
        .motion_from_previous_pose(&previous, model, DT)
        .unwrap();
    for (joint, (entity, initial)) in entities.iter().zip(motion).enumerate() {
        assert!(initial.linear_velocity.distance(DVec3::X) < f64::from(POSITION_TOLERANCE));
        let expected_spin = DVec3::Z * if joint == 0 { 2.0 } else { -2.0 };
        assert!(initial.angular_velocity.distance(expected_spin) < f64::from(POSITION_TOLERANCE));
        let body = physics.body_mut(*entity).unwrap();
        body.velocity = initial.linear_velocity;
        body.angular_velocity = initial.angular_velocity;
        physics.set_transform(*entity, Transform::new(initial.position, initial.rotation));
    }
    let mut palette = Palette::new(&skeleton);
    for _ in 0..600 {
        physics.step(DT);
        let current = entities.map(|entity| world(&physics, entity));
        binding.pose_into(model, &current, &mut pose).unwrap();
        palette.compute(&skeleton, &pose);
        for (joint, body_world) in current.iter().enumerate() {
            let bind_vertex = Vec3::new(joint as f32 * 60.0 + 20.0, 300.0, 0.0);
            let skinned = (model * palette.matrices()[joint]).transform_point3(bind_vertex);
            let physical = body_world.transform_point3(Vec3::new(0.2, 0.0, 0.0));
            assert!(
                skinned.distance(physical) < POSITION_TOLERANCE,
                "joint {joint}: skin {skinned:?}, body {physical:?}"
            );
        }
    }
    for entity in entities {
        let transform = physics.transform(entity).unwrap();
        let body = physics.body(entity).unwrap();
        assert!(
            transform.position.y < 0.5 && transform.position.y > 0.0,
            "body did not land on terrain: {transform:?}"
        );
        assert!(
            body.velocity.length() < 0.1 && body.angular_velocity.length() < 0.1,
            "body did not settle: {body:?}"
        );
    }
}
