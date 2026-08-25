//! The headline property: same seeds, same `dt`, same steps, same pool.
//!
//! `docs/plan/20-particles.md` puts it as "fixed seed + fixed time step =
//! identical frames", and hangs golden-frame testing of effects on it. These
//! tests are what says the claim is true of the simulation rather than of the
//! sentence.

mod support;

use crcbl_vfx::{EffectDesc, Modifiers, ParticleSystem, Shape, Spawn};
use glam::{Vec3, Vec4};

/// The step the whole suite uses. A power of two over one, so `age` advances by
/// an exactly representable amount and the arithmetic under test is the
/// simulation's rather than the accumulator's.
const DT: f32 = 1.0 / 64.0;

/// Long enough that every effect below has spawned, saturated and started
/// recycling slots.
const STEPS: u32 = 240;

/// A cone burst with gravity and drag — the shape of impact sparks.
fn sparks() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Burst { count: 96 },
        shape: Shape::Cone {
            axis: Vec3::new(0.0, 1.0, 0.3),
            half_angle: 0.6,
        },
        lifetime: (0.25, 0.75),
        speed: (4.0, 9.0),
        size: (0.02, 0.05),
        spin: (-12.0, 12.0),
        modifiers: Modifiers {
            gravity: Vec3::new(0.0, -9.8, 0.0),
            drag: 1.2,
            size: crcbl_vfx::Curve::new(vec![(0.0, 1.0), (1.0, 0.2)]).unwrap(),
            color: crcbl_vfx::Gradient::new(vec![
                (0.0, Vec4::new(1.0, 0.95, 0.75, 1.0)),
                (1.0, Vec4::new(1.0, 0.35, 0.0, 0.0)),
            ])
            .unwrap(),
        },
        max_particles: 128,
    }
}

/// A slow omnidirectional stream — the shape of a smoke puff.
fn smoke() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Rate { per_second: 40.0 },
        shape: Shape::Point,
        lifetime: (1.0, 2.0),
        speed: (0.2, 0.8),
        size: (0.15, 0.3),
        spin: (-0.5, 0.5),
        modifiers: Modifiers {
            gravity: Vec3::new(0.0, 0.6, 0.0),
            drag: 0.9,
            ..Modifiers::default()
        },
        max_particles: 96,
    }
}

/// Build the same three-effect scene every time, with the same seeds.
fn scene(smoke_seed: u32) -> ParticleSystem {
    let mut vfx = ParticleSystem::new(512);
    vfx.add(&sparks(), Vec3::new(1.0, 0.0, -2.0), 0x5EED_0001)
        .expect("the sparks fit an empty pool");
    vfx.add(&smoke(), Vec3::new(-1.0, 0.5, 0.0), smoke_seed)
        .expect("the smoke fits beside them");
    vfx
}

fn run(smoke_seed: u32) -> Vec<u32> {
    let mut vfx = scene(smoke_seed);
    for _ in 0..STEPS {
        vfx.step(DT);
    }
    support::bits(&vfx)
}

#[test]
fn two_runs_of_one_scene_leave_identical_pools() {
    let first = run(0x5EED_0002);
    let second = run(0x5EED_0002);
    support::assert_same("a replayed scene", &first, &second);
}

/// The control for the test above. Without it, a simulation that never wrote
/// anything to the pool would pass it — two empty pools are identical.
#[test]
fn a_different_seed_leaves_a_different_pool() {
    let first = run(0x5EED_0002);
    let other = run(0x5EED_0003);
    support::assert_differs("one seed against another", &first, &other);
}

/// **The property a running generator cannot have.**
///
/// The sparks effect is added first in both scenes, so it owns the same slots
/// in both. In one it runs alone; in the other a second effect spawns and
/// retires particles around it on every step. A stream shared between effects
/// would hand the sparks different numbers in the two runs — that is the whole
/// difference between a stream and a hash — so this comparison is what says the
/// randomness is keyed on the particle and not on the order of the frame.
///
/// Compared on every step rather than at the end, because the sparks are a
/// burst: by the time the scene above has finished they have all retired, and
/// two empty effects agree about everything.
#[test]
fn an_effect_is_unaffected_by_what_is_simulated_beside_it() {
    let mut alone = ParticleSystem::new(512);
    let solo = alone
        .add(&sparks(), Vec3::new(1.0, 0.0, -2.0), 0x5EED_0001)
        .expect("the sparks fit an empty pool");

    let mut crowded = scene(0x5EED_0002);
    let together = crowded
        .effects()
        .next()
        .expect("the sparks are the first effect of the crowded scene")
        .id;

    let mut compared = 0usize;
    for step in 0..STEPS {
        alone.step(DT);
        crowded.step(DT);
        let (Some(solo), Some(together)) = (alone.live(solo), crowded.live(together)) else {
            break;
        };
        assert_eq!(
            solo.len(),
            together.len(),
            "on step {step} the same effect kept a different number of particles \
             depending on its company"
        );
        for at in 0..solo.len() {
            assert_eq!(
                solo.position[at].to_array().map(f32::to_bits),
                together.position[at].to_array().map(f32::to_bits),
                "on step {step}, particle {at} of the sparks is somewhere else when \
                 another effect runs beside it"
            );
            assert_eq!(
                solo.size[at].to_bits(),
                together.size[at].to_bits(),
                "on step {step}, particle {at} of the sparks is a different size when \
                 another effect runs beside it"
            );
            assert_eq!(
                solo.color[at].to_array().map(f32::to_bits),
                together.color[at].to_array().map(f32::to_bits),
                "on step {step}, particle {at} of the sparks is a different colour when \
                 another effect runs beside it"
            );
            compared += 1;
        }
    }
    assert!(
        compared > 0,
        "no live particle was ever compared, so this test cannot fail"
    );
}

/// A step of zero moves nothing, so a paused frame is not a frame of drift.
#[test]
fn a_zero_step_changes_nothing() {
    let mut vfx = scene(0x5EED_0002);
    for _ in 0..STEPS {
        vfx.step(DT);
    }
    let before = support::bits(&vfx);
    vfx.step(0.0);
    let after = support::bits(&vfx);
    support::assert_same("a zero-length step", &before, &after);
}
