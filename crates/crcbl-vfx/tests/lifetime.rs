//! When a particle goes, and what happens to the slot it leaves.

use crcbl_vfx::{EffectDesc, Modifiers, ParticleSystem, Shape, Spawn};
use glam::Vec3;

/// A quarter second. Exact in binary, so `age` after four steps is exactly one
/// second and the retirement below is a question about the comparison rather
/// than about accumulated error.
const DT: f32 = 0.25;

fn one_second_burst(count: u32) -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Burst { count },
        shape: Shape::Point,
        lifetime: (1.0, 1.0),
        speed: (0.0, 0.0),
        size: (1.0, 1.0),
        spin: (0.0, 0.0),
        modifiers: Modifiers::default(),
        max_particles: count,
    }
}

#[test]
fn a_particle_retires_on_the_step_its_age_reaches_its_lifetime() {
    let mut vfx = ParticleSystem::new(16);
    let id = vfx
        .add(&one_second_burst(3), Vec3::ZERO, 1)
        .expect("the burst fits");

    // The first step emits; the particles are born on it with age zero.
    vfx.step(DT);
    assert_eq!(vfx.live_count(), 3, "the burst did not emit");

    for step in 1..=3 {
        vfx.step(DT);
        let ages = vfx.pool().ages();
        assert_eq!(
            vfx.live_count(),
            3,
            "a particle retired on step {step}, at age {}, before its lifetime of 1.0",
            ages[0]
        );
        assert_eq!(
            ages[0].to_bits(),
            (step as f32 * DT).to_bits(),
            "the age after {step} steps is not {step} times the step"
        );
    }

    vfx.step(DT);
    assert_eq!(
        vfx.live_count(),
        0,
        "a particle outlived its lifetime: age reached 1.0 and it is still alive"
    );
    assert!(
        vfx.effect_stats(id).is_none(),
        "the burst has no particles left and no more to emit, and is still in the pool"
    );
}

#[test]
fn every_particle_of_a_burst_retires_and_the_pool_empties() {
    let mut vfx = ParticleSystem::new(64);
    vfx.add(&one_second_burst(48), Vec3::ZERO, 7)
        .expect("the burst fits");
    for _ in 0..5 {
        vfx.step(DT);
    }
    let stats = vfx.stats();
    assert_eq!(stats.live, 0, "particles outlived a burst that is over");
    assert_eq!(
        stats.reserved, 0,
        "the finished effect kept its range, so the pool leaks a share per burst"
    );
    assert_eq!(
        stats.free_spans, 1,
        "the freed range did not merge back, so the free list fragments per burst"
    );
    assert_eq!(
        stats.granted, 48,
        "the burst did not emit what it asked for"
    );
}

/// **A retired slot is reused.**
///
/// The observable is the spawn index sitting in the slots: an effect capped at
/// four particles that has granted far more than four has necessarily put later
/// particles into the slots earlier ones vacated, and the indices are what say
/// so. A simulation that leaked a slot per retirement would clamp at four
/// grants and never spawn again.
#[test]
fn a_retired_slot_is_reused_by_the_next_particle() {
    const CAP: u32 = 4;
    let mut vfx = ParticleSystem::new(32);
    let id = vfx
        .add(
            &EffectDesc {
                spawn: Spawn::Rate { per_second: 16.0 },
                lifetime: (0.5, 0.5),
                max_particles: CAP,
                ..one_second_burst(CAP)
            },
            Vec3::ZERO,
            11,
        )
        .expect("the stream fits");

    for _ in 0..24 {
        vfx.step(DT);
        assert!(
            vfx.live_count() <= CAP,
            "the effect holds {} particles, past its share of {CAP}",
            vfx.live_count()
        );
    }

    let stats = vfx.effect_stats(id).expect("the stream is still running");
    assert_eq!(stats.reserved, CAP, "the effect's range changed size");
    assert!(
        stats.granted > u64::from(CAP),
        "only {} particles were ever granted, so no slot was ever recycled",
        stats.granted
    );

    let indices = &vfx.pool().indices()[..CAP as usize];
    let oldest = indices
        .iter()
        .copied()
        .min()
        .expect("the range is not empty");
    assert!(
        oldest >= CAP,
        "slot {} still holds particle {oldest}, one of the first {CAP} ever spawned, \
         so the early particles never gave their slots up",
        indices.iter().position(|i| *i == oldest).unwrap_or(0),
    );
}

/// **Exactly the particles whose lifetime has run out are the ones that go.**
///
/// The oracle is the pool's own lifetimes, read on the step the burst lands
/// while every particle is still alive. From then on the live count is
/// arithmetic: how many of those lifetimes are longer than the age. Nothing
/// about the *order* of the slots is assumed, which is the point — a simulation
/// that dropped the tail of its range whenever a particle in the middle died
/// would keep every invariant the tests above check and fail this one, because
/// the count would run ahead of the oracle.
#[test]
fn the_live_count_is_the_particles_whose_lifetime_has_not_run_out() {
    const N: u32 = 64;
    const STEP: f32 = 1.0 / 32.0;
    let mut vfx = ParticleSystem::new(128);
    vfx.add(
        &EffectDesc {
            spawn: Spawn::Burst { count: N },
            lifetime: (0.5, 1.5),
            ..one_second_burst(N)
        },
        Vec3::ZERO,
        23,
    )
    .expect("the burst fits");

    vfx.step(STEP);
    assert_eq!(vfx.live_count(), N, "the burst did not land whole");
    let lifetimes: Vec<f32> = vfx.pool().lifetimes()[..N as usize].to_vec();
    let spread = lifetimes.iter().copied().fold(f32::MAX, f32::min)
        < lifetimes.iter().copied().fold(f32::MIN, f32::max);
    assert!(
        spread,
        "every particle drew the same lifetime, so they all retire together and \
         this test cannot tell one retirement order from another"
    );

    let mut seen_partial = false;
    for step in 1..64 {
        vfx.step(STEP);
        let age = step as f32 * STEP;
        let expected = lifetimes.iter().filter(|life| **life > age).count() as u32;
        assert_eq!(
            vfx.live_count(),
            expected,
            "at age {age}, {} particles are alive and {expected} have lifetime left",
            vfx.live_count()
        );
        if expected > 0 && expected < N {
            seen_partial = true;
        }
    }
    assert!(
        seen_partial,
        "the burst went from whole to empty in one step, so no partial retirement \
         was ever compared"
    );
}
