//! Islands and sleep, run whole: rung 3 of `docs/plan/36-contact-solver.md`.
//!
//! A still island sleeps, a sleeping body costs nothing and does not move, and
//! each of decision 4's wake rules wakes what it should and nothing else. As in
//! `stacking.rs`, every bound was measured before it was written down, and
//! each says what it was measured at.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, Ray,
    RigidBody, SurfaceMaterial, Transform,
};
use glam::DVec3;

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Box2D's default friction, and no bounce.
const CRATE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts at the defaults, Earth gravity and a floor at
/// `y = 0`.
fn system() -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, CRATE);
    phys
}

/// A dynamic box of half-extents `half` and 10 kg at `at`.
fn crate_(phys: &mut PhysicsSystem, index: u32, at: DVec3, half: DVec3) -> Entity {
    let e = entity(index);
    let inertia = MassProperties::cuboid(10.0, half, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(10.0).with_inertia(inertia));
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, CRATE);
    e
}

/// A static box.
fn slab(phys: &mut PhysicsSystem, index: u32, centre: DVec3, half: DVec3) -> Entity {
    let e = entity(index);
    let transform = Transform::from_position(centre);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, CRATE);
    e
}

/// A 1 kg ball of radius 0.1 m at `at`, moving at `velocity`.
fn ball(phys: &mut PhysicsSystem, index: u32, at: DVec3, velocity: DVec3) -> Entity {
    const RADIUS: f64 = 0.1;
    let e = entity(index);
    let inertia = MassProperties::sphere(1.0, RADIUS, DVec3::ZERO).inertia;
    let mut body = RigidBody::new_dynamic(1.0).with_inertia(inertia);
    body.velocity = velocity;
    phys.set_body(e, body);
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: RADIUS,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, CRATE);
    e
}

/// A column of `count` half-metre cubes standing on the floor at `x`, bottom
/// first, their entities numbered from `first`.
fn column(phys: &mut PhysicsSystem, first: u32, x: f64, count: u32) -> Vec<Entity> {
    (0..count)
        .map(|i| {
            let y = 0.25 + 0.5 * f64::from(i);
            crate_(phys, first + i, DVec3::new(x, y, 0.0), DVec3::splat(0.25))
        })
        .collect()
}

/// A pyramid with `base` half-metre cubes along its bottom row.
fn pyramid(phys: &mut PhysicsSystem, base: u32) -> Vec<Entity> {
    let mut boxes = Vec::new();
    for row in 0..base {
        let across = base - row;
        for k in 0..across {
            let x = (f64::from(k) - 0.5 * f64::from(across - 1)) * 0.5;
            let y = 0.25 + 0.5 * f64::from(row);
            let index = u32::try_from(boxes.len()).expect("a few hundred");
            boxes.push(crate_(
                phys,
                index,
                DVec3::new(x, y, 0.0),
                DVec3::splat(0.25),
            ));
        }
    }
    boxes
}

/// Steps until no body is awake, and says after how many ticks, or `None` if
/// that took more than `limit`.
fn settle(phys: &mut PhysicsSystem, limit: u32) -> Option<u32> {
    for tick in 1..=limit {
        phys.step(DT);
        if phys.contact_counters().bodies == 0 {
            return Some(tick);
        }
    }
    None
}

/// A half-metre cube resting on the floor, asleep.
fn sleeping_box() -> (PhysicsSystem, Entity) {
    let mut phys = system();
    let e = crate_(&mut phys, 0, DVec3::new(0.0, 0.25, 0.0), DVec3::splat(0.25));
    settle(&mut phys, 120).expect("a box on the floor sleeps");
    assert!(phys.is_sleeping(e));
    (phys, e)
}

/// FNV-1a over a system's hash.
fn hash(phys: &PhysicsSystem) -> u64 {
    use std::hash::Hasher;
    struct Fnv(u64);
    impl Hasher for Fnv {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// Every float of a body and its transform, as bits.
fn bits(phys: &PhysicsSystem, e: Entity) -> Vec<u64> {
    let t = phys.transform(e).expect("a transform");
    let b = phys.body(e).expect("a body");
    t.position
        .to_array()
        .into_iter()
        .chain(t.rotation.to_array())
        .chain(b.velocity.to_array())
        .chain(b.angular_velocity.to_array())
        .chain(b.force_accum.to_array())
        .map(f64::to_bits)
        .collect()
}

// ---------------------------------------------------------------------------
// Sleeping
// ---------------------------------------------------------------------------

/// **A base-20 pyramid settles to zero awake bodies**, as one island, without
/// leaving its place — the rung's row for the Tower room's pyramid, at the
/// defaults.
///
/// Measured on 2026-09-23: every one of the 210 cubes asleep at tick 59, and
/// none moved more than 2.73 cm getting there — the contact springs'
/// squeeze, which `stacking.rs` measures the same pyramid sinking by awake.
#[test]
fn a_pyramid_settles_to_sleep() {
    let mut phys = system();
    let boxes = pyramid(&mut phys, 20);
    let starts: Vec<DVec3> = boxes
        .iter()
        .map(|&e| phys.transform(e).expect("placed").position)
        .collect();
    let ticks = settle(&mut phys, 240).expect("the pyramid never slept");
    let counters = phys.contact_counters();
    assert_eq!(counters.sleeping, 210, "{counters:?}");
    assert_eq!(
        (counters.islands, counters.sleeping_islands),
        (0, 1),
        "{counters:?}"
    );
    assert!(boxes.iter().all(|&e| phys.is_sleeping(e)));
    let worst = boxes
        .iter()
        .zip(&starts)
        .map(|(&e, start)| (phys.transform(e).expect("placed").position - *start).length())
        .fold(0.0, f64::max);
    assert!(
        worst < 0.04,
        "a cube moved {worst} m settling, by tick {ticks}"
    );
    assert!(ticks <= 90, "slept at tick {ticks}");
}

/// **A sleeping body is not integrated**: every float of its body and
/// transform stays bit for bit what it was when it fell asleep, tick after
/// tick, and its contact is not collided.
#[test]
fn a_sleeping_body_is_not_integrated() {
    let (mut phys, e) = sleeping_box();
    let asleep = bits(&phys, e);
    let hash_asleep = hash(&phys);
    for tick in 0..120 {
        phys.step(DT);
        assert!(phys.is_sleeping(e), "woke at tick {tick}");
        assert_eq!(bits(&phys, e), asleep, "tick {tick}");
        let counters = phys.contact_counters();
        assert_eq!(counters.bodies, 0, "tick {tick}: {counters:?}");
        assert_eq!(counters.touching, 0, "tick {tick}: {counters:?}");
    }
    assert_eq!(hash(&phys), hash_asleep, "the state moved while asleep");
}

/// **An island that loses a contact is split, so its still part sleeps while
/// the rest moves**: two boxes side by side are one island; one is pushed
/// away and kept moving, and the other sleeps alone.
///
/// Without the split the island could not sleep while any of it moved.
/// Measured on 2026-09-23: the left box asleep on the thirtieth step after the
/// push — as soon as its half second was up — with the right one sliding at
/// 1.0 m/s and still awake.
#[test]
fn an_island_that_parts_is_split_and_its_still_half_sleeps() {
    let mut phys = system();
    let half = DVec3::splat(0.25);
    let left = crate_(&mut phys, 0, DVec3::new(0.0, 0.25, 0.0), half);
    let right = crate_(&mut phys, 1, DVec3::new(0.5, 0.25, 0.0), half);
    phys.step(DT);
    assert_eq!(
        phys.contact_counters().islands,
        1,
        "touching boxes share an island"
    );
    // Slid away at 1 m/s, held there against friction every tick.
    let friction = 0.6 * 10.0 * 9.81;
    phys.body_mut(right).expect("a body").velocity = DVec3::new(1.0, 0.0, 0.0);
    let mut left_slept = None;
    for tick in 0..120 {
        phys.apply_force(right, DVec3::new(friction, 0.0, 0.0));
        phys.step(DT);
        if left_slept.is_none() && phys.is_sleeping(left) {
            left_slept = Some(tick);
        }
    }
    let tick = left_slept.expect("the left box never slept");
    assert!(!phys.is_sleeping(right), "the pushed box slept");
    let counters = phys.contact_counters();
    assert_eq!(
        (counters.islands, counters.sleeping_islands),
        (1, 1),
        "{counters:?}"
    );
    assert!(tick < 90, "the left box slept only at tick {tick}");
}

// ---------------------------------------------------------------------------
// Waking
// ---------------------------------------------------------------------------

/// **A ball dropped on a sleeping stack wakes that stack and no other**, and
/// when it has settled, everything sleeps again.
///
/// Measured on 2026-09-23: both columns asleep at tick 31; the ball, dropped
/// from 1.4 m over the first, woke it on the thirty-second tick of its fall
/// while the second slept on; everything asleep again 49 ticks later.
#[test]
fn a_ball_wakes_the_stack_it_hits_and_only_that_one() {
    let mut phys = system();
    let hit = column(&mut phys, 0, 0.0, 3);
    let spared = column(&mut phys, 10, 3.0, 3);
    let first = settle(&mut phys, 120).expect("the columns sleep");
    assert!(first <= 60, "the columns slept only at tick {first}");
    assert_eq!(phys.contact_counters().sleeping_islands, 2);

    let b = ball(&mut phys, 20, DVec3::new(0.05, 3.0, 0.0), DVec3::ZERO);
    let mut woke = None;
    for tick in 0..60 {
        phys.step(DT);
        assert!(
            spared.iter().all(|&e| phys.is_sleeping(e)),
            "tick {tick}: the other column woke"
        );
        if !phys.is_sleeping(hit[2]) {
            woke = Some(tick);
            break;
        }
        assert!(
            hit.iter().all(|&e| phys.is_sleeping(e)),
            "tick {tick}: the column woke from the bottom"
        );
    }
    let woke = woke.expect("the ball never woke the column");
    // A 1.4 m fall takes 32 ticks: waking much before that is not the ball.
    assert!(
        woke >= 28,
        "the column woke at tick {woke}, before the ball got there"
    );
    assert!(hit.iter().all(|&e| !phys.is_sleeping(e)), "woken whole");
    assert!(!phys.is_sleeping(b));

    let again = settle(&mut phys, 240).expect("everything sleeps again");
    assert!(again <= 120, "asleep again only after {again} ticks");
    let counters = phys.contact_counters();
    assert_eq!(counters.sleeping, 7, "{counters:?}");
}

/// **A body wakes when its support is removed** — decision 4's trap, which
/// Jolt falls into — whether the support is a static slab taken out, a
/// dynamic box under it taken out, or a collider taken off: each time the box
/// wakes at once and falls.
#[test]
fn a_body_wakes_when_its_support_is_removed() {
    enum Removal {
        StaticEntity,
        DynamicEntity,
        StaticCollider,
    }
    for removal in [
        Removal::StaticEntity,
        Removal::DynamicEntity,
        Removal::StaticCollider,
    ] {
        let mut phys = system();
        let half = DVec3::splat(0.25);
        let support = match removal {
            Removal::StaticEntity | Removal::StaticCollider => slab(
                &mut phys,
                0,
                DVec3::new(0.0, 0.5, 0.0),
                DVec3::new(1.0, 0.5, 1.0),
            ),
            Removal::DynamicEntity => crate_(
                &mut phys,
                0,
                DVec3::new(0.0, 0.5, 0.0),
                DVec3::new(1.0, 0.5, 1.0),
            ),
        };
        let top = crate_(&mut phys, 1, DVec3::new(0.0, 1.25, 0.0), half);
        settle(&mut phys, 120).expect("the box on its support sleeps");
        assert!(phys.is_sleeping(top));

        match removal {
            Removal::StaticEntity | Removal::DynamicEntity => phys.remove_entity(support),
            Removal::StaticCollider => phys.remove_collider(support),
        }
        assert!(!phys.is_sleeping(top), "the box slept on in mid-air");
        for _ in 0..30 {
            phys.step(DT);
        }
        let y = phys.transform(top).expect("the box").position.y;
        assert!(y < 0.3, "the box did not fall: it is at {y} m");
    }
}

/// **Every other wake rule wakes a sleeping box**: a force, a torque, a
/// velocity written, a new body, a teleport of it, a teleport of the slab it
/// sits on, a new material under it, a new collider on it — and **nothing
/// that only looks at it does**: reading its body and transform, and a ray,
/// a sweep and an overlap through it.
#[test]
fn each_wake_rule_wakes_and_a_query_does_not() {
    type Rule = fn(&mut PhysicsSystem, Entity, Entity);
    let rules: [(&str, Rule); 8] = [
        ("apply_force", |phys, e, _| {
            phys.apply_force(e, DVec3::X);
        }),
        ("apply_torque", |phys, e, _| {
            phys.apply_torque(e, DVec3::Y);
        }),
        ("body_mut", |phys, e, _| {
            let _ = phys.body_mut(e);
        }),
        ("set_body", |phys, e, _| {
            let body = *phys.body(e).expect("a body");
            phys.set_body(e, body);
        }),
        ("set_transform", |phys, e, _| {
            let at = *phys.transform(e).expect("placed");
            phys.set_transform(e, at);
        }),
        ("set_transform of its support", |phys, _, slab| {
            let at = *phys.transform(slab).expect("placed");
            phys.set_transform(slab, at);
        }),
        ("set_material of its support", |phys, _, slab| {
            phys.set_material(slab, SurfaceMaterial::new(0.0, 0.0));
        }),
        ("set_collider", |phys, e, _| {
            let at = *phys.transform(e).expect("placed");
            phys.set_collider(
                e,
                &ColliderComponent::Box {
                    offset: DVec3::ZERO,
                    half_extents: DVec3::splat(0.25),
                    is_trigger: false,
                },
                &at,
            );
        }),
    ];
    let on_slab = || {
        let mut phys = system();
        let support = slab(
            &mut phys,
            0,
            DVec3::new(0.0, 0.5, 0.0),
            DVec3::new(1.0, 0.5, 1.0),
        );
        let e = crate_(&mut phys, 1, DVec3::new(0.0, 1.25, 0.0), DVec3::splat(0.25));
        settle(&mut phys, 120).expect("the box sleeps");
        (phys, e, support)
    };
    for (name, rule) in rules {
        let (mut phys, e, support) = on_slab();
        rule(&mut phys, e, support);
        assert!(!phys.is_sleeping(e), "{name} left the box asleep");
    }

    let (mut phys, e, _) = on_slab();
    let before = bits(&phys, e);
    let _ = phys.body(e);
    let _ = phys.transform(e);
    let down = Ray::new(DVec3::new(0.0, 3.0, 0.0), DVec3::NEG_Y);
    assert_eq!(phys.cast_ray(&down).map(|(hit, _)| hit), Some(e));
    let _ = phys.sweep_sphere(
        &crcbl_phys::Segment {
            start: DVec3::new(0.0, 3.0, 0.0),
            end: DVec3::new(0.0, 0.0, 0.0),
        },
        0.1,
    );
    assert!(
        phys.overlap_sphere(DVec3::new(0.0, 1.25, 0.0), 0.1)
            .contains(&e)
    );
    assert!(phys.is_sleeping(e), "a query woke the box");
    phys.step(DT);
    assert!(phys.is_sleeping(e), "a query woke the box by the next step");
    assert_eq!(bits(&phys, e), before);
}

/// **A body touching a moving kinematic body never counts as still**, even
/// when it is: a box on a frictionless platform sliding out from under it
/// stays awake for as long as the platform moves under it.
#[test]
fn a_body_on_a_moving_kinematic_platform_stays_awake() {
    let mut phys = system();
    let ice = SurfaceMaterial::new(0.0, 0.0);
    let platform = entity(10);
    let mut body = RigidBody::new_kinematic();
    body.velocity = DVec3::new(0.2, 0.0, 0.0);
    phys.set_body(platform, body);
    let at = Transform::from_position(DVec3::new(0.0, 0.1, 0.0));
    phys.set_transform(platform, at);
    phys.set_collider(
        platform,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(2.0, 0.1, 1.0),
            is_trigger: false,
        },
        &at,
    );
    phys.set_material(platform, ice);
    let e = crate_(&mut phys, 0, DVec3::new(0.0, 0.45, 0.0), DVec3::splat(0.25));
    phys.set_material(e, ice);
    // Five seconds: the platform, four metres long, slides a metre.
    for tick in 0..300 {
        phys.step(DT);
        assert!(
            !phys.is_sleeping(e),
            "slept on the moving platform at tick {tick}"
        );
    }
    let slid = phys.transform(e).expect("placed").position.x;
    assert!(
        slid.abs() < 0.01,
        "the frictionless box was carried {slid} m"
    );
}

/// **Turning sleep off wakes everything on the next step**, and keeps it
/// awake.
#[test]
fn turning_sleep_off_wakes_everything() {
    let (mut phys, e) = sleeping_box();
    phys.contact_settings_mut().expect("contacts").sleep = false;
    for tick in 0..60 {
        phys.step(DT);
        assert!(!phys.is_sleeping(e), "asleep at tick {tick}");
    }
    assert_eq!(phys.contact_counters().sleeping, 0);
}

/// **A static body placed on a sleeping one wakes it on the next step**, and
/// a kinematic body touching one wakes it only while it moves: placed on the
/// box it wakes it once, as anything placed there does, and at rest it lets
/// the box sleep again under it.
#[test]
fn a_static_dropped_in_and_a_moving_kinematic_wake_a_sleeper() {
    let (mut phys, e) = sleeping_box();
    slab(&mut phys, 10, DVec3::new(0.3, 0.25, 0.0), DVec3::splat(0.1));
    assert!(phys.is_sleeping(e), "placing it is not yet a contact");
    phys.step(DT);
    assert!(
        !phys.is_sleeping(e),
        "a static placed into it left it asleep"
    );

    let (mut phys, e) = sleeping_box();
    let k = entity(10);
    phys.set_body(k, RigidBody::new_kinematic());
    let at = Transform::from_position(DVec3::new(0.0, 0.75, 0.0));
    phys.set_transform(k, at);
    phys.set_collider(
        k,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(0.5, 0.25, 0.5),
            is_trigger: false,
        },
        &at,
    );
    phys.step(DT);
    assert!(
        !phys.is_sleeping(e),
        "a kinematic body placed on it left it asleep"
    );
    let mut slept = false;
    for _ in 0..120 {
        phys.step(DT);
        slept |= phys.is_sleeping(e);
        if slept {
            assert!(
                phys.is_sleeping(e),
                "a kinematic body at rest on it woke it"
            );
        }
    }
    assert!(
        slept,
        "the box never slept again under a kinematic body at rest"
    );
    phys.body_mut(k).expect("a body").velocity = DVec3::new(0.0, -0.2, 0.0);
    phys.step(DT);
    assert!(
        !phys.is_sleeping(e),
        "a kinematic body pressing on it left it asleep"
    );
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

/// **A sleeping pyramid costs the solver next to nothing**: the base-20
/// pyramid's mean solver time over thirty ticks once asleep is under a tenth
/// of its mean over its first thirty ticks, awake. Run with `--nocapture` to
/// see the figures; a release build gives the ones worth quoting.
#[test]
fn a_sleeping_pyramid_costs_the_solver_next_to_nothing() {
    const TICKS: u32 = 30;
    let mut phys = system();
    pyramid(&mut phys, 20);
    let epoch = std::time::Instant::now();
    let mut clock = || epoch.elapsed().as_secs_f64();
    let mut mean = |phys: &mut PhysicsSystem| {
        let mut sum = [0.0; 4];
        for _ in 0..TICKS {
            phys.step_timed(DT, &mut clock);
            let stages = phys.contact_counters().stages.expect("timed");
            for (total, value) in sum.iter_mut().zip([
                stages.broadphase,
                stages.narrow_phase,
                stages.solver,
                stages.islands,
            ]) {
                *total += value;
            }
        }
        sum.map(|total| total / f64::from(TICKS))
    };
    let awake = mean(&mut phys);
    settle(&mut phys, 240).expect("the pyramid sleeps");
    let asleep = mean(&mut phys);
    assert_eq!(phys.contact_counters().bodies, 0, "it woke while measured");
    for (name, awake, asleep) in [
        ("broadphase", awake[0], asleep[0]),
        ("narrow phase", awake[1], asleep[1]),
        ("solver", awake[2], asleep[2]),
        ("islands", awake[3], asleep[3]),
    ] {
        println!(
            "pyramid {name}: {:.1} us awake, {:.1} us asleep",
            awake * 1e6,
            asleep * 1e6
        );
    }
    assert!(
        asleep[2] < awake[2] / 10.0,
        "asleep the solver took {} s a tick against {} s awake",
        asleep[2],
        awake[2]
    );
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// A pyramid that settles, is hit by a ball and settles again, hashed after
/// `ticks`; and whether by then it had slept, woken, and slept again.
fn sleep_and_wake(ticks: u32) -> (u64, bool) {
    let mut phys = system();
    let boxes = pyramid(&mut phys, 5);
    let (mut slept, mut woke, mut slept_again) = (false, false, false);
    for tick in 0..ticks {
        if tick == 90 {
            ball(
                &mut phys,
                100,
                DVec3::new(0.1, 3.5, 0.0),
                DVec3::new(0.0, -2.0, 0.0),
            );
        }
        phys.step(DT);
        let asleep = boxes.iter().all(|&e| phys.is_sleeping(e));
        slept |= asleep;
        woke |= slept && !asleep;
        slept_again |= woke && asleep;
    }
    (hash(&phys), slept && woke && slept_again)
}

/// **Sleep is deterministic**: a pyramid that sleeps, is woken by a ball and
/// sleeps again hashes the same on two runs, at the end and in the middle of
/// it, and the hash tells the ticks apart.
#[test]
fn sleep_and_wake_hash_the_same_on_two_runs() {
    let (end, cycled) = sleep_and_wake(300);
    assert!(cycled, "the pyramid did not sleep, wake and sleep again");
    assert_eq!(sleep_and_wake(300).0, end);
    assert_eq!(sleep_and_wake(100).0, sleep_and_wake(100).0);
    assert_ne!(sleep_and_wake(100).0, sleep_and_wake(101).0);
}

/// **A column woken by a teleport of its top box to where it already was
/// picks up where it slept**, its warm-started contacts holding it, and sleeps
/// again within the time to sleep and a tick.
///
/// It does move a little: an island sleeps once it is slower than
/// [`ContactSettings::sleep_speed`], not once it is still, so this one went
/// to sleep with its springs not quite done squeezing and finishes when
/// woken. Measured on 2026-09-23: of five half-metre cubes, woken, the most
/// any moved was 0.38 mm before they slept again 31 ticks later.
#[test]
fn a_woken_column_holds_and_sleeps_again() {
    let mut phys = system();
    let boxes = column(&mut phys, 0, 0.0, 5);
    settle(&mut phys, 120).expect("the column sleeps");
    let before: Vec<DVec3> = boxes
        .iter()
        .map(|&e| phys.transform(e).expect("placed").position)
        .collect();
    let top = *boxes.last().expect("a top");
    let at = *phys.transform(top).expect("placed");
    phys.set_transform(top, at);
    assert!(boxes.iter().all(|&e| !phys.is_sleeping(e)));
    let ticks = settle(&mut phys, 120).expect("the column sleeps again");
    let moved = boxes
        .iter()
        .zip(&before)
        .map(|(&e, start)| (phys.transform(e).expect("placed").position - *start).length())
        .fold(0.0, f64::max);
    assert!(moved < 1e-3, "a box moved {moved} m while awake");
    assert!(ticks <= 31, "slept again only after {ticks} ticks");
}

// ---------------------------------------------------------------------------
// Restoring asleep
// ---------------------------------------------------------------------------

/// A settled, sleeping column of `count` cubes: the system, the boxes, and
/// each box's transform as it slept — what a snapshot saves.
fn saved_column(count: u32) -> (PhysicsSystem, Vec<Entity>, Vec<Transform>) {
    let mut phys = system();
    let boxes = column(&mut phys, 0, 0.0, count);
    settle(&mut phys, 120).expect("the column sleeps");
    let saved = boxes
        .iter()
        .map(|&e| *phys.transform(e).expect("placed"))
        .collect();
    (phys, boxes, saved)
}

/// A fresh system holding the column `saved` describes, each box registered at
/// its saved transform and put to sleep there, without a step.
fn restored_column(saved: &[Transform]) -> (PhysicsSystem, Vec<Entity>) {
    let mut phys = system();
    let boxes: Vec<Entity> = saved
        .iter()
        .enumerate()
        .map(|(i, at)| {
            let index = u32::try_from(i).expect("a few boxes");
            let e = crate_(&mut phys, index, at.position, DVec3::splat(0.25));
            phys.set_transform(e, *at);
            e
        })
        .collect();
    for &e in &boxes {
        assert!(phys.put_to_sleep(e), "{e:?} did not sleep");
    }
    (phys, boxes)
}

/// **A sleeping column restored asleep is the column that was saved, and stays
/// it**: every float of every box is bit for bit what the saved system holds,
/// straight after the restore and after two seconds of steps, and nothing
/// wakes — the round trip a save game makes, which restoring awake misses by
/// the re-settle before the island sleeps again.
#[test]
fn a_column_restored_asleep_stays_exactly_where_it_slept() {
    let (original, saved_boxes, saved) = saved_column(3);
    let (mut phys, boxes) = restored_column(&saved);
    let expected: Vec<Vec<u64>> = saved_boxes.iter().map(|&e| bits(&original, e)).collect();
    let now = |phys: &PhysicsSystem| boxes.iter().map(|&e| bits(phys, e)).collect::<Vec<_>>();
    assert_eq!(now(&phys), expected, "restored");
    for tick in 0..120 {
        phys.step(DT);
        assert!(
            boxes.iter().all(|&e| phys.is_sleeping(e)),
            "woke at tick {tick}"
        );
        assert_eq!(now(&phys), expected, "tick {tick}");
    }
    let counters = phys.contact_counters();
    assert_eq!(counters.bodies, 0, "{counters:?}");
    assert_eq!(counters.sleeping, 3, "{counters:?}");
}

/// **A body restored asleep wakes by the rules a sleeping body wakes by**, and
/// a ball dropped on a restored column wakes the whole column once it lands on
/// it: the islands it was restored as join on contact, as any others do.
#[test]
fn a_body_restored_asleep_wakes_as_a_sleeping_one_does() {
    let (_, _, saved) = saved_column(1);
    type Rule = fn(&mut PhysicsSystem, Entity);
    let rules: [(&str, Rule); 3] = [
        ("apply_force", |phys, e| {
            phys.apply_force(e, DVec3::X);
        }),
        ("body_mut", |phys, e| {
            let _ = phys.body_mut(e);
        }),
        ("set_transform", |phys, e| {
            let at = *phys.transform(e).expect("placed");
            phys.set_transform(e, at);
        }),
    ];
    for (name, rule) in rules {
        let (mut phys, boxes) = restored_column(&saved);
        rule(&mut phys, boxes[0]);
        assert!(!phys.is_sleeping(boxes[0]), "{name} left the box asleep");
    }

    let (_, _, saved) = saved_column(3);
    let (mut phys, boxes) = restored_column(&saved);
    let top = saved[2].position.y + 0.25;
    ball(&mut phys, 99, DVec3::new(0.0, top + 0.3, 0.0), DVec3::ZERO);
    let mut woke = None;
    for tick in 1..=60 {
        phys.step(DT);
        if boxes.iter().all(|&e| !phys.is_sleeping(e)) {
            woke = Some(tick);
            break;
        }
    }
    assert!(woke.is_some(), "the ball left the column asleep");
}

/// **Nothing is put to sleep that cannot sleep**: an entity with no body, a
/// system without contacts, and one with sleep turned off all say `false`
/// and leave the body awake.
#[test]
fn put_to_sleep_refuses_what_cannot_sleep() {
    let mut phys = system();
    assert!(!phys.put_to_sleep(entity(7)), "an entity with no body");

    let mut plain = PhysicsSystem::new();
    plain.set_body(entity(0), RigidBody::new_dynamic(1.0));
    assert!(!plain.put_to_sleep(entity(0)), "a system without contacts");
    assert!(!plain.is_sleeping(entity(0)));

    let mut off = PhysicsSystem::with_contacts(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    });
    off.set_body(entity(0), RigidBody::new_dynamic(1.0));
    assert!(!off.put_to_sleep(entity(0)), "sleep off");
    assert!(!off.is_sleeping(entity(0)));
}
