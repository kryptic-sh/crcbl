//! Continuous collision, run whole: rung 4 of
//! `docs/plan/36-contact-solver.md` — fast bodies swept against statics, the
//! bullet flag, and the time a stopped body loses dropped.
//!
//! Speculative contacts already stop a body whose speed is known when the
//! tick begins: a ball fired at 300 m/s at a centimetre plate stops at it
//! without any sweep. What they cannot stop is speed or a surface the tick's
//! manifold did not see — a body launched by a force within the tick, and a
//! plank's end turning into a pillar the manifold measured from another part
//! of it. Each test here runs its scene with
//! [`ContactSettings::continuous`] off as well as on, and asserts that the
//! scene fails without the sweeps: that is what shows the check can fail.
//!
//! Every bound was measured before it was written down, on 2026-09-23.

use crcbl_core::trig;
use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, CompoundPart, CompoundShape, ContactSettings, GravityForce, MassProperties,
    PhysicsSystem, RigidBody, SurfaceMaterial, Transform, rotation_from_scaled_axis,
};
use glam::{DMat3, DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// The speed a launch gives a body within its first tick, in m/s.
const LAUNCH_SPEED: f64 = 60.0;

/// Half a thin plate's thickness: a centimetre plate.
const PLATE_HALF: f64 = 0.005;

/// A body maker: a system, an entity index and where.
type Make = fn(&mut PhysicsSystem, u32, DVec3) -> Entity;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts, sweeping or not.
fn system(continuous: bool) -> PhysicsSystem {
    PhysicsSystem::with_contacts(ContactSettings {
        continuous,
        ..ContactSettings::DEFAULT
    })
}

fn box_collider(half: DVec3) -> ColliderComponent {
    ColliderComponent::Box {
        offset: DVec3::ZERO,
        half_extents: half,
        is_trigger: false,
    }
}

/// A static box.
fn fixture(phys: &mut PhysicsSystem, index: u32, at: DVec3, half: DVec3) {
    let e = entity(index);
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(e, &box_collider(half), &transform);
}

/// A dynamic body of `collider` and `mass` with `inertia`, at rest.
fn body(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    collider: &ColliderComponent,
    mass: f64,
    inertia: DMat3,
) -> Entity {
    let e = entity(index);
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(e, collider, &transform);
    phys.set_material(e, SurfaceMaterial::new(0.3, 0.3));
    e
}

fn ball(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let radius = 0.05;
    body(
        phys,
        index,
        at,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius,
            is_trigger: false,
        },
        1.0,
        MassProperties::sphere(1.0, radius, DVec3::ZERO).inertia,
    )
}

fn cube(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let half = DVec3::splat(0.05);
    body(
        phys,
        index,
        at,
        &box_collider(half),
        1.0,
        MassProperties::cuboid(1.0, half, DVec3::ZERO).inertia,
    )
}

/// An L of two boxes, one of them turned.
fn compound(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let parts = vec![
        CompoundPart::new(
            DVec3::new(-0.05, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::new(0.08, 0.03, 0.03),
        ),
        CompoundPart::new(
            DVec3::new(0.05, 0.06, 0.0),
            rotation_from_scaled_axis(DVec3::Y * 0.4),
            DVec3::new(0.03, 0.06, 0.03),
        ),
    ];
    let shape = CompoundShape::new(parts).expect("two parts");
    body(
        phys,
        index,
        at,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape,
            is_trigger: false,
        },
        1.0,
        DMat3::from_diagonal(DVec3::splat(0.004)),
    )
}

/// A plank two metres long and ten centimetres wide, at `at`, spinning at
/// 60 rad/s about the vertical, its `+X` end turning towards `+Z`.
fn plank(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let e = body(
        phys,
        index,
        at,
        &box_collider(PLANK_HALF),
        1.0,
        MassProperties::cuboid(1.0, PLANK_HALF, DVec3::ZERO).inertia,
    );
    if let Some(spun) = phys.body_mut(e) {
        spun.angular_velocity = DVec3::NEG_Y * 60.0;
    }
    e
}

/// The plank's half-extents.
const PLANK_HALF: DVec3 = DVec3::new(1.0, 0.02, 0.05);

/// Where a pillar stands from a plank's middle: 0.85 m out, a radian round
/// from the plank's `+X` end, the way it turns.
fn pillar_offset() -> DVec3 {
    let angle = 1.0f64;
    DVec3::new(0.85 * trig::cos(angle), 0.0, 0.85 * trig::sin(angle))
}

/// A pillar's half-extents: two centimetres square.
const PILLAR_HALF: DVec3 = DVec3::new(0.01, 1.0, 0.01);

/// Pushes `e` along `+X` hard enough that it leaves its first tick at
/// [`LAUNCH_SPEED`]: a force held for one tick, which the tick's speculative
/// contacts, sized from the speed it began with, know nothing of.
fn launch(phys: &mut PhysicsSystem, e: Entity) {
    let mass = phys.body(e).expect("a body").mass;
    assert!(phys.apply_force(e, DVec3::X * (mass * LAUNCH_SPEED / DT)));
}

/// **A body launched at a thin plate within a tick stops at it** — a ball, a
/// tumbling cube and a tumbling compound, each from rest a hand's width in
/// front of a centimetre plate, launched to 60 m/s in one tick. The sensor
/// behind the plate is its far face: nothing may end a tick past it.
///
/// Without the sweeps all three go straight through, their first tick
/// carrying them well past the plate; with them, each is stopped at the plate
/// on that tick.
#[test]
fn a_body_launched_at_a_thin_plate_within_a_tick_stops_at_it() {
    let run = |continuous: bool, make: Make| {
        let mut phys = system(continuous);
        fixture(&mut phys, 0, DVec3::ZERO, DVec3::new(PLATE_HALF, 1.0, 1.0));
        let e = make(&mut phys, 1, DVec3::new(-0.25, 0.0, 0.0));
        if let Some(spun) = phys.body_mut(e) {
            spun.angular_velocity = DVec3::new(0.0, 5.0, 30.0);
        }
        launch(&mut phys, e);
        let mut tunnelled = 0u32;
        let mut first = None;
        for _ in 0..60 {
            phys.step(DT);
            first.get_or_insert(phys.contact_counters());
            if phys.transform(e).expect("placed").position.x > PLATE_HALF {
                tunnelled += 1;
            }
        }
        (tunnelled, first.expect("stepped"))
    };
    for (name, make) in [
        ("ball", ball as Make),
        ("cube", cube),
        ("compound", compound),
    ] {
        let (through, _) = run(false, make);
        assert!(
            through > 0,
            "the {name} did not tunnel without sweeps, so this scene proves nothing"
        );
        let (through, first) = run(true, make);
        assert_eq!(through, 0, "the {name} tunnelled: {first:?}");
        assert_eq!(
            (first.swept, first.sweep_hits),
            (1, 1),
            "the {name}'s first tick: {first:?}"
        );
        assert!(first.sweep_candidates >= 1, "{first:?}");
        assert!(
            first.dropped_time > 0.0 && first.dropped_time < DT,
            "{first:?}"
        );
    }
}

/// **A fast spinning plank does not sink into a static pillar.** A two-metre
/// plank, ten centimetres wide, spinning at 60 rad/s about its middle, its
/// end sweeping a metre a tick round towards a two-centimetre pillar 0.85 m
/// out.
///
/// The manifold is built once a tick from where the plank is; turning a
/// radian in that tick, the plank meets the pillar with a part of it the
/// manifold did not measure from. Measured on 2026-09-23: without the sweeps
/// the pillar sank 5.9 cm into the plank, past the middle of its width; with
/// them, not at all. Over every pillar from one to six centimetres across,
/// at 0.85 to 0.97 m out, a third of a radian to a radian round and 30 to
/// 120 rad/s, the sweeps kept the worst to 0.38 cm, where without them it
/// was up to 8.7 cm.
#[test]
fn a_fast_spinning_plank_does_not_sink_into_a_static_pillar() {
    let pillar = pillar_offset();
    let run = |continuous: bool| {
        let mut phys = system(continuous);
        fixture(&mut phys, 0, pillar, PILLAR_HALF);
        let plank = plank(&mut phys, 1, DVec3::ZERO);
        let (mut deepest, mut swept, mut crossed) = (0.0f64, 0usize, false);
        let mut side: Option<f64> = None;
        for _ in 0..60 {
            phys.step(DT);
            let counters = phys.contact_counters();
            deepest = deepest.max(counters.worst_penetration);
            swept += counters.swept;
            // The pillar crossing the plank's length from one side to the
            // other, seen from above, is the plank passing through it.
            let t = phys.transform(plank).expect("placed");
            let along = t.rotation * DVec3::X;
            let to = pillar - t.position;
            let within = along.x * to.x + along.z * to.z > 0.0
                && to.x * to.x + to.z * to.z < PLANK_HALF.x * PLANK_HALF.x
                && along.x * along.x + along.z * along.z > 0.25;
            let now = along.x * to.z - along.z * to.x;
            if within {
                crossed |= side.is_some_and(|before| before.signum() != now.signum());
                side = Some(now);
            } else {
                side = None;
            }
        }
        (deepest, swept, crossed)
    };
    let (deepest, _, _) = run(false);
    assert!(
        deepest > PLANK_HALF.z,
        "without sweeps the pillar sank only {deepest} m, so this scene proves nothing"
    );
    let (deepest, swept, crossed) = run(true);
    assert!(!crossed, "the plank passed through the pillar");
    assert!(deepest < 0.01, "the pillar sank {deepest} m into the plank");
    assert!(swept > 0, "the plank was never swept");
}

/// **The bullet flag sweeps a slow body, and sweeps it against moving
/// bodies.** A ball rolling at half a metre a second goes a sixth of its
/// inner radius a tick, under the fast threshold, and is swept only as a
/// bullet. And a ball launched within a tick at a thin plate that is itself
/// dynamic — a tonne, floating — goes through it unless it is a bullet,
/// sweeps or no sweeps, because only a bullet sweeps against a body that is
/// not static.
#[test]
fn the_bullet_flag_sweeps_a_slow_body_and_against_moving_ones() {
    for bullet in [false, true] {
        let mut phys = system(true);
        let e = ball(&mut phys, 1, DVec3::ZERO);
        if let Some(b) = phys.body_mut(e) {
            b.velocity = DVec3::X * 0.5;
            b.bullet = bullet;
        }
        phys.step(DT);
        assert_eq!(
            phys.contact_counters().swept,
            usize::from(bullet),
            "a slow ball, bullet {bullet}"
        );
    }

    let run = |bullet: bool| {
        let mut phys = system(true);
        let plate_half = DVec3::new(0.01, 1.0, 1.0);
        let plate = body(
            &mut phys,
            0,
            DVec3::ZERO,
            &box_collider(plate_half),
            1000.0,
            MassProperties::cuboid(1000.0, plate_half, DVec3::ZERO).inertia,
        );
        let shot = ball(&mut phys, 1, DVec3::new(-0.25, 0.0, 0.0));
        let flagged = phys.body(shot).expect("a body").with_bullet(bullet);
        phys.set_body(shot, flagged);
        launch(&mut phys, shot);
        let mut through = 0u32;
        for _ in 0..60 {
            phys.step(DT);
            let plate_x = phys.transform(plate).expect("placed").position.x;
            if phys.transform(shot).expect("placed").position.x > plate_x {
                through += 1;
            }
        }
        through
    };
    assert!(
        run(false) > 0,
        "a plain ball did not pass the dynamic plate, so this proves nothing"
    );
    assert_eq!(run(true), 0, "the bullet passed the dynamic plate");
}

/// **A sleeping body is not swept.** A bullet dropped on the floor is swept
/// as it falls; once its island sleeps nothing sweeps it and it does not
/// move; given a push, it wakes and is swept again.
#[test]
fn a_sleeping_body_is_not_swept() {
    let mut phys = system(true);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, SurfaceMaterial::new(0.6, 0.0));
    let e = ball(&mut phys, 1, DVec3::new(0.0, 1.0, 0.0));
    let flagged = phys.body(e).expect("a body").with_bullet(true);
    phys.set_body(e, flagged);

    let mut swept_awake = 0;
    let mut asleep = false;
    for _ in 0..300 {
        phys.step(DT);
        if phys.is_sleeping(e) {
            asleep = true;
            break;
        }
        swept_awake += phys.contact_counters().swept;
    }
    assert!(asleep, "the ball never slept");
    assert!(swept_awake > 0, "the falling bullet was never swept");

    let rest = *phys.transform(e).expect("placed");
    for _ in 0..60 {
        phys.step(DT);
        let counters = phys.contact_counters();
        assert!(phys.is_sleeping(e), "{counters:?}");
        assert_eq!(counters.swept, 0, "a sleeping bullet was swept");
        assert_eq!(counters.sweep_candidates, 0, "{counters:?}");
    }
    assert_eq!(*phys.transform(e).expect("placed"), rest);

    assert!(phys.apply_force(e, DVec3::new(300.0, 0.0, 0.0)));
    phys.step(DT);
    assert!(!phys.is_sleeping(e));
    assert_eq!(phys.contact_counters().swept, 1, "the woken bullet");
}

/// **The sweeps are deterministic**: a scene of launched balls, cubes and a
/// compound at plates, and a spinning plank at a pillar, under gravity,
/// hashes the same on two runs, and the sweeps did stop things in it.
#[test]
fn a_scene_with_sweeps_hashes_the_same_twice() {
    let run = || {
        let mut phys = system(true);
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_plane(DVec3::Y, 0.0, SurfaceMaterial::new(0.6, 0.2));
        let mut launched = Vec::new();
        for (index, make) in (0u32..).zip([ball as Make, cube, compound]) {
            let z = 3.0 * f64::from(index);
            fixture(
                &mut phys,
                index,
                DVec3::new(0.0, 1.0, z),
                DVec3::new(PLATE_HALF, 1.0, 1.0),
            );
            launched.push(make(&mut phys, 10 + index, DVec3::new(-0.25, 1.0, z)));
        }
        let middle = DVec3::new(0.0, 1.0, -3.0);
        fixture(&mut phys, 3, middle + pillar_offset(), PILLAR_HALF);
        plank(&mut phys, 20, middle);
        for &e in &launched {
            launch(&mut phys, e);
        }
        let mut hits = 0;
        for _ in 0..120 {
            phys.step(DT);
            hits += phys.contact_counters().sweep_hits;
        }
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        phys.hash_state(&mut hasher);
        (hasher.0, hits)
    };
    let (first, hits) = run();
    assert!(hits >= 3, "the sweeps stopped {hits} bodies");
    assert_eq!(run(), (first, hits), "two runs disagreed");
}

/// FNV-1a, for a digest that means the same in every build.
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
