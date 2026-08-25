//! The emitter shapes: that a cone points where it is aimed, and that both
//! shapes spread the way their doc comments say they do.
//!
//! The distribution is the part worth a test. A cone sampled uniformly in its
//! polar *angle* rather than in the cosine of that angle looks almost right —
//! it fills the cone, it respects the axis, every direction is unit length —
//! and it is dense down the middle with a thin skirt. Nothing but a count
//! against the analytic fraction tells the two apart.

use crcbl_vfx::{EffectDesc, Modifiers, ParticleSystem, Shape, Spawn};
use glam::Vec3;

/// Enough particles that a fraction is worth two decimal places.
const N: u32 = 40_000;

/// How far a measured fraction may sit from the analytic one.
///
/// The sampling error over [`N`] draws is a few parts in a thousand; the gap
/// this has to stay inside is the one between a solid-angle sample and an angle
/// sample, which is over a tenth for the cone below.
const TOLERANCE: f64 = 0.01;

/// Every particle's direction, from a burst emitted at unit speed.
fn directions(shape: Shape) -> Vec<Vec3> {
    let mut vfx = ParticleSystem::new(N);
    vfx.add(
        &EffectDesc {
            spawn: Spawn::Burst { count: N },
            shape,
            lifetime: (10.0, 10.0),
            speed: (1.0, 1.0),
            size: (1.0, 1.0),
            spin: (0.0, 0.0),
            modifiers: Modifiers::default(),
            max_particles: N,
        },
        Vec3::ZERO,
        0xD1_2EC7,
    )
    .expect("the burst fits its own pool");
    vfx.step(1.0 / 64.0);
    assert_eq!(vfx.live_count(), N, "the burst did not land whole");
    // One step of travel at unit speed and no modifiers, so the position is the
    // direction times the step; the velocity is the direction itself.
    vfx.pool().velocities()[..N as usize].to_vec()
}

fn fraction(directions: &[Vec3], predicate: impl Fn(Vec3) -> bool) -> f64 {
    directions.iter().copied().filter(|d| predicate(*d)).count() as f64 / directions.len() as f64
}

fn assert_close(what: &str, measured: f64, expected: f64) {
    assert!(
        (measured - expected).abs() < TOLERANCE,
        "{what}: measured {measured:.4}, expected {expected:.4}"
    );
}

#[test]
fn every_direction_is_unit_length() {
    for shape in [
        Shape::Point,
        Shape::Cone {
            axis: Vec3::new(1.0, 2.0, -3.0),
            half_angle: 0.7,
        },
    ] {
        for (at, direction) in directions(shape).iter().enumerate() {
            assert!(
                (direction.length() - 1.0).abs() < 1.0e-5,
                "{shape:?} gave particle {at} a direction {direction:?} of length {}",
                direction.length()
            );
        }
    }
}

/// A cone points along its axis, and nothing leaves through the side.
#[test]
fn a_cone_contains_every_direction_it_emits() {
    let axis = Vec3::new(1.0, 2.0, -3.0).normalize();
    let half_angle = 0.4_f32;
    let bound = half_angle.cos();
    for (at, direction) in directions(Shape::Cone { axis, half_angle })
        .iter()
        .enumerate()
    {
        let cosine = direction.dot(axis);
        assert!(
            cosine >= bound - 1.0e-5,
            "particle {at} left the cone: its direction is {:.4} radians off the axis, \
             and the cone is {half_angle} wide",
            cosine.clamp(-1.0, 1.0).acos()
        );
    }
}

/// A cone of zero width is a beam down its axis.
#[test]
fn a_cone_of_no_width_is_a_beam() {
    let axis = Vec3::new(0.0, 0.0, -1.0);
    for (at, direction) in directions(Shape::Cone {
        axis,
        half_angle: 0.0,
    })
    .iter()
    .enumerate()
    {
        assert!(
            direction.distance(axis) < 1.0e-5,
            "particle {at} of a beam left along {direction:?} rather than {axis:?}"
        );
    }
}

/// **The distribution, which is the whole point.**
///
/// For a cone of half-angle θ, the fraction of a uniform-in-solid-angle sample
/// that falls within a narrower half-angle φ is `(1 - cos φ) / (1 - cos θ)`.
/// For a hemisphere (θ = π/2) and φ = π/3 that is exactly one half. Sampling
/// the angle uniformly instead would put φ/θ = two thirds of them there, which
/// is a sixth of the emitter in the wrong place and nowhere near the tolerance.
#[test]
fn a_cone_is_uniform_per_unit_of_solid_angle() {
    let axis = Vec3::Y;
    let half_angle = std::f32::consts::FRAC_PI_2;
    let directions = directions(Shape::Cone { axis, half_angle });

    for narrow in [
        std::f32::consts::FRAC_PI_6,
        std::f32::consts::FRAC_PI_3,
        1.2,
    ] {
        let expected = (1.0 - f64::from(narrow.cos())) / (1.0 - f64::from(half_angle.cos()));
        let measured = fraction(&directions, |d| d.dot(axis) >= narrow.cos());
        assert_close(
            &format!("within {narrow} radians of a hemisphere's axis"),
            measured,
            expected,
        );
    }
}

/// And the same question of the sphere: a band of the sphere holds a share of
/// the directions equal to its share of the surface, which is Archimedes' — it
/// depends on the band's height alone.
#[test]
fn a_point_emitter_is_uniform_over_the_sphere() {
    let directions = directions(Shape::Point);
    for (low, high) in [(-1.0_f32, -0.5_f32), (-0.25, 0.25), (0.5, 1.0)] {
        let expected = f64::from(high - low) / 2.0;
        let measured = fraction(&directions, |d| {
            let z = d.dot(Vec3::Z);
            z >= low && z < high
        });
        assert_close(
            &format!("the band of the sphere from {low} to {high}"),
            measured,
            expected,
        );
    }
    let mean = directions.iter().copied().sum::<Vec3>() / directions.len() as f32;
    assert!(
        mean.length() < 0.02,
        "the directions average to {mean:?}, so the sphere is not evenly covered"
    );
}
