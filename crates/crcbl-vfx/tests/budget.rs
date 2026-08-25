//! Budgets behave: an effect clamps to its pool share and the clamp is visible.
//!
//! `docs/plan/sample/10-sparks.md` states the requirement as a demo: "a
//! deliberately hostile effect (max spam) clamps to its pool share and the
//! panel shows it — never a frame-rate cliff". The frame rate is not something
//! a unit test can see; what it can see is that the count stops climbing, that
//! the pool is not overrun, and that the refusals are counted rather than
//! silently dropped.

use crcbl_vfx::{AddError, EffectDesc, EffectError, Modifiers, ParticleSystem, Shape, Spawn};
use glam::Vec3;

const DT: f32 = 1.0 / 64.0;

/// An emitter asking for a million particles a second out of a share of
/// [`SHARE`]. It gets its share and not one more.
const HOSTILE_RATE: f32 = 1.0e6;

/// The pool share the hostile effect is given.
const SHARE: u32 = 64;

fn hostile() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Rate {
            per_second: HOSTILE_RATE,
        },
        shape: Shape::Point,
        lifetime: (0.2, 0.4),
        speed: (1.0, 3.0),
        size: (0.05, 0.05),
        spin: (0.0, 0.0),
        modifiers: Modifiers::default(),
        max_particles: SHARE,
    }
}

fn modest() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Rate { per_second: 30.0 },
        max_particles: 32,
        ..hostile()
    }
}

#[test]
fn a_hostile_emitter_clamps_to_its_share_and_stays_there() {
    let mut vfx = ParticleSystem::new(1024);
    let id = vfx.add(&hostile(), Vec3::ZERO, 3).expect("the share fits");

    let mut peak = 0;
    for step in 0..256 {
        vfx.step(DT);
        peak = peak.max(vfx.live_count());
        assert!(
            vfx.live_count() <= SHARE,
            "on step {step} the hostile effect holds {} particles, past its share of {SHARE}",
            vfx.live_count()
        );
    }

    assert_eq!(
        peak, SHARE,
        "the hostile effect never even filled its share, so the clamp was never tested"
    );
    let stats = vfx.effect_stats(id).expect("the hostile effect is running");
    assert_eq!(
        stats.live, SHARE,
        "the hostile effect settled at {} rather than at its share",
        stats.live
    );
    assert_eq!(stats.reserved, SHARE, "the effect's range is not its share");
    assert!(
        stats.clamped() > 0,
        "the emitter asked for {} and was granted {}, so nothing was refused and the \
         count is capped by something other than the budget",
        stats.requested,
        stats.granted
    );
    assert_eq!(
        vfx.stats().live,
        SHARE,
        "the pool's own count disagrees with the effect's"
    );
}

/// The control that says the clamp is per effect rather than a dead pool.
///
/// Without it, a simulation that stopped spawning entirely once the hostile
/// effect saturated would pass the test above.
#[test]
fn a_modest_effect_keeps_emitting_beside_a_hostile_one() {
    let mut vfx = ParticleSystem::new(1024);
    let loud = vfx.add(&hostile(), Vec3::ZERO, 3).expect("the share fits");
    let quiet = vfx
        .add(&modest(), Vec3::X, 4)
        .expect("the modest share fits beside it");

    for _ in 0..256 {
        vfx.step(DT);
    }

    let loud = vfx
        .effect_stats(loud)
        .expect("the hostile effect is running");
    let quiet = vfx
        .effect_stats(quiet)
        .expect("the modest effect is running");
    assert_eq!(loud.live, SHARE, "the hostile effect left its share");
    assert!(
        quiet.live > 0,
        "the modest effect was starved by the hostile one, which is the pool share \
         failing to be a share"
    );
    assert_eq!(
        quiet.clamped(),
        0,
        "the modest effect was refused {} spawns, so it is being clamped too",
        quiet.clamped()
    );
}

/// Asking for more of the pool than is left is a clamp, not a refusal.
#[test]
fn an_oversized_share_is_clamped_to_what_the_pool_has() {
    let mut vfx = ParticleSystem::new(100);
    let greedy = EffectDesc {
        max_particles: 1000,
        ..hostile()
    };
    let id = vfx
        .add(&greedy, Vec3::ZERO, 5)
        .expect("a clamp is not an error");
    let stats = vfx.effect_stats(id).expect("the effect was added");
    assert_eq!(stats.reserved, 100, "the range was not clamped to the pool");
    assert_eq!(
        stats.budget, 1000,
        "the panel lost what the effect asked for"
    );
    assert!(
        stats.short_of_budget(),
        "an effect given a tenth of what it asked for does not report being short"
    );

    vfx.step(DT);
    assert_eq!(vfx.live_count(), 100, "the clamped range did not fill");
    assert_eq!(
        vfx.add(&greedy, Vec3::ZERO, 6),
        Err(AddError::PoolFull),
        "a second effect was given slots out of a pool with none left"
    );
}

#[test]
fn an_effect_with_no_budget_is_refused() {
    let mut vfx = ParticleSystem::new(64);
    let starved = EffectDesc {
        max_particles: 0,
        ..hostile()
    };
    assert_eq!(
        vfx.add(&starved, Vec3::ZERO, 1),
        Err(AddError::Effect(EffectError::NoBudget)),
        "an effect that can never hold a particle was accepted"
    );
}

#[test]
fn a_lifetime_of_zero_is_refused() {
    let mut vfx = ParticleSystem::new(64);
    let instant = EffectDesc {
        lifetime: (0.0, 1.0),
        ..hostile()
    };
    assert_eq!(
        vfx.add(&instant, Vec3::ZERO, 1),
        Err(AddError::Effect(EffectError::OutOfRange {
            field: "lifetime"
        })),
        "a lifetime of zero was accepted, and it is a division by zero per particle"
    );
}

/// Stopping an emitter empties it, and the pool takes the share back.
#[test]
fn a_stopped_emitter_drains_and_returns_its_share() {
    let mut vfx = ParticleSystem::new(1024);
    let id = vfx.add(&hostile(), Vec3::ZERO, 3).expect("the share fits");
    for _ in 0..64 {
        vfx.step(DT);
    }
    assert_eq!(vfx.live_count(), SHARE, "the effect never filled its share");

    assert!(vfx.stop(id), "the effect was not there to stop");
    let mut drained = None;
    for step in 0..64 {
        vfx.step(DT);
        if vfx.live_count() == 0 {
            drained = Some(step);
            break;
        }
    }
    let drained = drained.expect("a stopped emitter never emptied");
    assert!(
        drained > 0,
        "the effect emptied on the step it was stopped, so nothing outlived the stop"
    );
    assert_eq!(
        vfx.stats().reserved,
        0,
        "the drained effect kept its share of the pool"
    );
}
