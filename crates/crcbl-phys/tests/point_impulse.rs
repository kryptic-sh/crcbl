use crcbl_ecs::Entity;
use crcbl_phys::{
    ContactSettings, ImpulseError, PhysicsSystem, RigidBody, Transform, rotation_from_scaled_axis,
};
use glam::{DMat3, DQuat, DVec3};

const TOLERANCE: f64 = 1e-12;

fn body() -> RigidBody {
    RigidBody::new_dynamic(2.0).with_inertia(DMat3::from_diagonal(DVec3::new(2.0, 4.0, 8.0)))
}

#[test]
fn off_centre_impulse_transfers_linear_and_world_angular_momentum() {
    let mut body = body();
    body.velocity = DVec3::new(1.0, 2.0, 3.0);
    body.angular_velocity = DVec3::new(0.5, -0.2, 0.1);
    body.force_accum = DVec3::new(3.0, 2.0, 1.0);
    body.torque_accum = DVec3::new(1.0, 3.0, 2.0);
    let before = body;
    let transform = Transform::new(
        DVec3::new(10.0, 20.0, 30.0),
        rotation_from_scaled_axis(DVec3::Z * std::f64::consts::FRAC_PI_2),
    );
    let impulse = DVec3::new(0.0, 0.0, 8.0);
    body.apply_impulse_at(impulse, transform.position + DVec3::Y, &transform)
        .unwrap();
    assert_eq!(body.velocity, before.velocity + DVec3::new(0.0, 0.0, 4.0));
    assert!(body.angular_velocity.abs_diff_eq(
        before.angular_velocity + DVec3::new(2.0, 0.0, 0.0),
        TOLERANCE
    ));
    assert!(
        (body.angular_momentum(transform.rotation) - before.angular_momentum(transform.rotation))
            .abs_diff_eq(DVec3::new(8.0, 0.0, 0.0), TOLERANCE)
    );
    assert_eq!(body.force_accum, before.force_accum);
    assert_eq!(body.torque_accum, before.torque_accum);
}

#[test]
fn centre_hits_and_bodies_without_inertia_do_not_invent_spin() {
    for mut body in [
        body(),
        RigidBody::new_dynamic(2.0),
        RigidBody::new_kinematic(),
    ] {
        body.velocity = DVec3::Z * 5.0;
        body.angular_velocity = DVec3::ONE;
        let before = body;
        body.apply_impulse_at(DVec3::X * 6.0, DVec3::ZERO, &Transform::IDENTITY)
            .unwrap();
        assert_eq!(body.angular_velocity, before.angular_velocity);
        assert_eq!(
            body.velocity,
            before.velocity + DVec3::X * (6.0 * before.inverse_mass)
        );
    }
    for mut body in [RigidBody::new_dynamic(2.0), RigidBody::new_kinematic()] {
        body.angular_velocity = DVec3::ONE;
        body.apply_impulse_at(DVec3::X * 6.0, DVec3::Y, &Transform::IDENTITY)
            .unwrap();
        assert_eq!(body.angular_velocity, DVec3::ONE);
        assert_eq!(body.velocity, DVec3::X * (6.0 * body.inverse_mass));
    }
}

#[test]
fn invalid_inputs_and_overflow_leave_the_body_unchanged() {
    let mut body = body();
    let before = body;
    assert_eq!(
        body.apply_impulse_at(DVec3::splat(f64::NAN), DVec3::ZERO, &Transform::IDENTITY),
        Err(ImpulseError::NonFiniteImpulse)
    );
    assert_eq!(body, before);
    assert_eq!(
        body.apply_impulse_at(DVec3::X, DVec3::splat(f64::INFINITY), &Transform::IDENTITY),
        Err(ImpulseError::NonFinitePoint)
    );
    assert_eq!(body, before);
    let invalid = Transform::new(DVec3::ZERO, DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0));
    assert_eq!(
        body.apply_impulse_at(DVec3::X, DVec3::ZERO, &invalid),
        Err(ImpulseError::InvalidTransform)
    );
    assert_eq!(body, before);
    body.velocity.x = f64::MAX;
    let before = body;
    assert_eq!(
        body.apply_impulse_at(DVec3::X * f64::MAX, DVec3::ZERO, &Transform::IDENTITY),
        Err(ImpulseError::NonFiniteVelocity)
    );
    assert_eq!(body, before);
    body.velocity = DVec3::ZERO;
    let before = body;
    assert_eq!(
        body.apply_impulse_at(
            DVec3::Z * f64::MAX,
            DVec3::Y * f64::MAX,
            &Transform::IDENTITY
        ),
        Err(ImpulseError::NonFiniteVelocity)
    );
    assert_eq!(body, before);
}

#[test]
fn system_impulses_wake_sleeping_bodies_only_after_validation() {
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    let entity = Entity::from_bits(1_u64 << 32).unwrap();
    assert_eq!(
        physics.apply_impulse_at(entity, DVec3::X, DVec3::ZERO),
        Err(ImpulseError::UnknownBody)
    );
    physics.set_body(entity, body());
    physics.set_transform(entity, Transform::IDENTITY);
    assert!(physics.put_to_sleep(entity));
    assert_eq!(
        physics.apply_impulse_at(entity, DVec3::X, DVec3::splat(f64::NAN)),
        Err(ImpulseError::NonFinitePoint)
    );
    assert!(physics.is_sleeping(entity));
    assert_eq!(physics.body(entity).unwrap().velocity, DVec3::ZERO);
    physics
        .apply_impulse_at(entity, DVec3::Z * 8.0, DVec3::Y)
        .unwrap();
    assert!(!physics.is_sleeping(entity));
    let body = physics.body(entity).unwrap();
    assert_eq!(body.velocity, DVec3::Z * 4.0);
    assert_eq!(body.angular_velocity, DVec3::X * 4.0);
}
