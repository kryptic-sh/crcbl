//! Joints, run whole: contact-solver rung 5 (`docs/notes/simulation.md`) — each
//! joint type holding what it holds, its limits and motors reaching their
//! targets, breaking at its threshold, a gapped Newton's cradle passing its
//! momentum along, a plank bridge sagging as a hanging chain says it should,
//! the wake rule a joint brings, determinism, and extra substeps for a group.
//!
//! As in `stacking.rs`, every bound was measured before it was written down,
//! and each says what it was measured at.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, ContactSettings, DistanceJoint, GravityForce, Joint, JointError, JointKind,
    MassProperties, PhysicsSystem, PrismaticJoint, RevoluteJoint, RigidBody, SphericalJoint,
    Spring, SurfaceMaterial, Transform, WeldJoint, rotation_from_scaled_axis,
};
use glam::{DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Standard gravity.
const G: f64 = 9.81;

/// Box2D's default friction, and no bounce.
const CRATE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts at `settings` and Earth gravity, and no floor.
fn system_with(settings: ContactSettings) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(settings);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys
}

/// [`system_with`] at the defaults, sleep off: these tests measure the solver
/// holding joints over a run, and a sleeping joint is not solved.
fn system() -> PhysicsSystem {
    system_with(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    })
}

/// A system at the defaults with no gravity and sleep off.
fn weightless() -> PhysicsSystem {
    PhysicsSystem::with_contacts(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    })
}

/// A fixed anchor: an entity with a transform and nothing else.
fn anchor(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let e = entity(index);
    phys.set_transform(e, Transform::from_position(at));
    e
}

/// A dynamic box of `mass` and half-extents `half` at `at`, turned by
/// `rotation`.
fn box_(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    rotation: DQuat,
    half: DVec3,
    mass: f64,
) -> Entity {
    let e = entity(index);
    let inertia = MassProperties::cuboid(mass, half, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::new(at, rotation);
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

/// A dynamic ball of `mass` and `radius` at `at`, of `material`.
fn ball(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    radius: f64,
    mass: f64,
    material: SurfaceMaterial,
) -> Entity {
    let e = entity(index);
    let inertia = MassProperties::sphere(mass, radius, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, material);
    e
}

fn position(phys: &PhysicsSystem, e: Entity) -> DVec3 {
    phys.transform(e).expect("a registered entity").position
}

fn rotation(phys: &PhysicsSystem, e: Entity) -> DQuat {
    phys.transform(e).expect("a registered entity").rotation
}

fn body(phys: &PhysicsSystem, e: Entity) -> RigidBody {
    *phys.body(e).expect("a body")
}

/// A joint of `kind` between `a` and `b` at `frame` in the world, as they
/// stand now.
fn joint_at(
    phys: &PhysicsSystem,
    a: Entity,
    b: Entity,
    frame: Transform,
    kind: JointKind,
) -> Joint {
    Joint::at(
        a,
        phys.transform(a).expect("a"),
        b,
        phys.transform(b).expect("b"),
        frame,
        kind,
    )
}

/// The rotation about z by `angle`, from its cosine and sine.
fn about_z(angle: f64) -> DQuat {
    rotation_from_scaled_axis(DVec3::Z * angle)
}

// ---------------------------------------------------------------------------
// Each type holds what it holds
// ---------------------------------------------------------------------------

/// **A rigid rod holds its length, and swings with a pendulum's period.** A
/// 1 kg ball on a one-metre rod from a fixed anchor, let go 0.2 rad out: the
/// rod's length never strays past the bound, and the period — from the
/// ball's crossings of the vertical — is the large-amplitude series
/// `2π √(L/g) (1 + θ²/16 + 11θ⁴/3072)`, 2.0111 s.
///
/// Measured on 2026-09-23 over five seconds: the rod strayed 0.08 mm from
/// its length at worst, and the period came out 2.01117 s against 2.01109.
#[test]
fn a_rigid_rod_holds_its_length_and_swings_with_a_pendulums_period() {
    const LENGTH: f64 = 1.0;
    const AMPLITUDE: f64 = 0.2;
    let mut phys = system();
    let top = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let start = DVec3::new(
        LENGTH * crcbl_core::trig::sin(AMPLITUDE),
        3.0 - LENGTH * crcbl_core::trig::cos(AMPLITUDE),
        0.0,
    );
    let bob = ball(&mut phys, 1, start, 0.05, 1.0, CRATE);
    let rod = phys
        .add_joint(Joint::new(
            top,
            bob,
            Transform::IDENTITY,
            Transform::IDENTITY,
            JointKind::Distance(DistanceJoint::rigid(LENGTH)),
        ))
        .expect("a valid joint");

    let mut worst: f64 = 0.0;
    let mut crossings = Vec::new();
    let mut last_x = start.x;
    for tick in 1..=300 {
        phys.step(DT);
        let drift = phys.joint_drift(rod).expect("the rod").linear;
        worst = worst.max(drift);
        let x = position(&phys, bob).x;
        if last_x > 0.0 && x <= 0.0 {
            // Linear interpolation to the crossing's time.
            crossings.push((f64::from(tick) - x / (x - last_x)) * DT);
        }
        last_x = x;
    }
    let period = (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f64;
    let expected = core::f64::consts::TAU
        * (LENGTH / G).sqrt()
        * (1.0
            + AMPLITUDE * AMPLITUDE / 16.0
            + 11.0 * AMPLITUDE * AMPLITUDE * AMPLITUDE * AMPLITUDE / 3072.0);
    assert!(worst < 2.0e-4, "the rod stretched by {worst} m");
    assert!(
        (period - expected).abs() < 1.0e-3 * expected,
        "period {period} s against {expected} s"
    );
}

/// How far a body from rest falls under a steady `acceleration` over `ticks`
/// ticks of four substeps, stepped as the solver steps it — semi-implicit
/// Euler, each substep's velocity before its position — which is
/// `a h² n (n + 1) / 2` over `n` substeps of `h`, a little past `½ a t²`.
fn euler_fall(acceleration: f64, ticks: u32) -> f64 {
    let substeps = f64::from(4 * ticks);
    let h = DT / 4.0;
    acceleration * h * h * substeps * (substeps + 1.0) / 2.0
}

/// **A rope goes slack and catches.** A ball hanging half a metre under its
/// anchor on a one-metre rope falls freely until the rope is taut — exactly
/// as far as [`euler_fall`] says, so the slack rope does nothing — and then
/// never gets further than the rope is long, past the bound.
///
/// Measured on 2026-09-23: the slack fall matched to rounding, and the rope
/// caught the ball at 1.000069 m at its longest.
#[test]
fn a_rope_falls_slack_and_catches_at_its_length() {
    let mut phys = system();
    let top = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bob = ball(&mut phys, 1, DVec3::new(0.0, 2.5, 0.0), 0.05, 1.0, CRATE);
    phys.add_joint(Joint::new(
        top,
        bob,
        Transform::IDENTITY,
        Transform::IDENTITY,
        JointKind::Distance(DistanceJoint::rope(1.0)),
    ))
    .expect("a valid rope");

    // Six ticks fall five centimetres of the half metre of slack.
    for _ in 0..6 {
        phys.step(DT);
    }
    let fall = 2.5 - position(&phys, bob).y;
    let free = euler_fall(G, 6);
    assert!(
        (fall - free).abs() < 1.0e-12,
        "slack, it fell {fall} m where free fall is {free} m"
    );

    let mut longest: f64 = 0.0;
    for _ in 0..180 {
        phys.step(DT);
        longest = longest.max(3.0 - position(&phys, bob).y);
    }
    assert!(longest > 0.999, "the rope never caught: {longest} m");
    assert!(longest < 1.0002, "the rope stretched to {longest} m");
}

/// **A hinge holds its pivot and its axis.** A one-metre bar hinged at one
/// end about z, let go level and kicked about x and y as well: it swings in
/// its plane, its pivot stays put, and the kick off the axis is taken away.
///
/// Measured on 2026-09-23 over three seconds: the pivot drifted 0.57 mm
/// and the axis tipped 0.58 mrad at worst, the bar kept within 0.23 mm of
/// its plane, and the kick off the axis was gone to rounding.
#[test]
fn a_hinge_holds_its_pivot_and_its_axis() {
    let mut phys = system();
    let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bar = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let hinge = phys
        .add_joint(joint_at(
            &phys,
            post,
            bar,
            Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
            JointKind::Revolute(RevoluteJoint::hinge()),
        ))
        .expect("a valid hinge");
    phys.body_mut(bar).expect("the bar").angular_velocity = DVec3::new(2.0, 2.0, 0.0);

    let (mut linear, mut angular, mut off_plane): (f64, f64, f64) = (0.0, 0.0, 0.0);
    for _ in 0..180 {
        phys.step(DT);
        let drift = phys.joint_drift(hinge).expect("the hinge");
        linear = linear.max(drift.linear);
        angular = angular.max(drift.angular);
        off_plane = off_plane.max(position(&phys, bar).z.abs());
    }
    let w = body(&phys, bar).angular_velocity;
    assert!(linear < 1.0e-3, "the pivot drifted {linear} m");
    assert!(angular < 1.0e-3, "the axis tipped {angular} rad");
    assert!(
        off_plane < 1.0e-3,
        "the bar left its plane by {off_plane} m"
    );
    assert!(
        DVec3::new(w.x, w.y, 0.0).length() < 1.0e-9,
        "spin off the axis {w:?}"
    );
}

/// **A slider keeps its body on its axis and lets it slide.** A block on a
/// frictionless slider tilted 30° down slides under gravity with
/// [`euler_fall`] at `g sin 30°`, to rounding, keeps its orientation, and
/// never leaves the line.
///
/// Measured on 2026-09-23 over one second: 2.46272 m, 0.06 mm off the axis
/// at worst, and no turn at all.
#[test]
fn a_slider_keeps_its_body_on_its_axis_and_lets_it_slide() {
    let mut phys = system();
    let rail = anchor(&mut phys, 0, DVec3::new(0.0, 5.0, 0.0));
    let tilt = about_z(-core::f64::consts::FRAC_PI_6);
    let block = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 5.0, 0.0),
        DQuat::IDENTITY,
        DVec3::splat(0.2),
        3.0,
    );
    let slider = phys
        .add_joint(joint_at(
            &phys,
            rail,
            block,
            Transform::new(DVec3::new(0.0, 5.0, 0.0), tilt),
            JointKind::Prismatic(PrismaticJoint::slider()),
        ))
        .expect("a valid slider");

    let mut worst = (0.0_f64, 0.0_f64);
    for _ in 0..60 {
        phys.step(DT);
        let drift = phys.joint_drift(slider).expect("the slider");
        worst = (worst.0.max(drift.linear), worst.1.max(drift.angular));
    }
    let travelled = (position(&phys, block) - DVec3::new(0.0, 5.0, 0.0)).length();
    let expected = euler_fall(0.5 * G, 60);
    assert!(worst.0 < 2.0e-4, "off the axis by {} m", worst.0);
    assert!(worst.1 < 1.0e-9, "turned {} rad", worst.1);
    assert!(
        (travelled - expected).abs() < 1.0e-9,
        "slid {travelled} m where {expected} m was due"
    );
}

/// **A weld holds a cantilever.** A one-metre beam welded by one end to a
/// fixed anchor, under gravity, droops only by the joint's softness.
///
/// Measured on 2026-09-23 over two seconds: the weld opened 0.27 mm and
/// bent 0.41 mrad.
#[test]
fn a_weld_holds_a_cantilever() {
    let mut phys = system();
    let wall = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let beam = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let weld = phys
        .add_joint(joint_at(
            &phys,
            wall,
            beam,
            Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
            JointKind::Weld(WeldJoint::rigid()),
        ))
        .expect("a valid weld");
    for _ in 0..120 {
        phys.step(DT);
    }
    let drift = phys.joint_drift(weld).expect("the weld");
    assert!(drift.linear < 5.0e-4, "the weld opened {} m", drift.linear);
    assert!(
        drift.angular < 1.0e-3,
        "the weld bent {} rad",
        drift.angular
    );
}

/// **A ball joint holds its socket and its cone.** A block hung on a ball
/// joint with a 0.4 rad cone about the vertical, knocked sideways hard:
/// it swings out to its cone and no further.
///
/// Measured on 2026-09-23 over two seconds: the socket opened 2.8 mm and
/// the block swung 5.3 mrad past its cone at worst.
#[test]
fn a_ball_joint_holds_its_socket_and_its_cone() {
    const CONE: f64 = 0.4;
    let mut phys = system();
    let ceiling = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let block = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 2.5, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.5, 0.05),
        2.0,
    );
    // The frames' z-axes point down the block.
    let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
    let socket = phys
        .add_joint(joint_at(
            &phys,
            ceiling,
            block,
            Transform::new(DVec3::new(0.0, 3.0, 0.0), down),
            JointKind::Spherical(SphericalJoint::ball().with_cone(CONE)),
        ))
        .expect("a valid ball joint");
    phys.body_mut(block).expect("the block").velocity = DVec3::new(4.0, 0.0, 2.0);

    let (mut linear, mut cone_excess): (f64, f64) = (0.0, 0.0);
    for _ in 0..120 {
        phys.step(DT);
        let drift = phys.joint_drift(socket).expect("the socket");
        linear = linear.max(drift.linear);
        cone_excess = cone_excess.max(drift.angular);
    }
    assert!(linear < 5.0e-3, "the socket opened {linear} m");
    assert!(cone_excess < 1.0e-2, "past the cone by {cone_excess} rad");
}

// ---------------------------------------------------------------------------
// Limits and motors
// ---------------------------------------------------------------------------

/// **A hinge's motor reaches its speed, at the rate its torque allows.** A
/// weightless wheel on a hinge with a motor of 2 rad/s and 0.5 N·m: it
/// spins up at `τ / I` until it reaches 2 rad/s, then holds it.
///
/// Measured on 2026-09-23: 0.375 rad/s at half a second, `τ t / I` to
/// rounding, and 2 rad/s held to rounding after ten more.
#[test]
fn a_hinge_motor_spins_up_at_its_torque_and_holds_its_speed() {
    let mut phys = weightless();
    let axle = anchor(&mut phys, 0, DVec3::ZERO);
    let half = DVec3::new(0.5, 0.5, 0.1);
    let wheel = box_(&mut phys, 1, DVec3::ZERO, DQuat::IDENTITY, half, 4.0);
    let inertia = MassProperties::cuboid(4.0, half, DVec3::ZERO)
        .inertia
        .z_axis
        .z;
    let (speed, torque) = (2.0, 0.5);
    phys.add_joint(joint_at(
        &phys,
        axle,
        wheel,
        Transform::IDENTITY,
        JointKind::Revolute(RevoluteJoint::hinge().with_motor(speed, torque)),
    ))
    .expect("a valid motor");

    for _ in 0..30 {
        phys.step(DT);
    }
    let spun = body(&phys, wheel).angular_velocity.z;
    let expected = torque / inertia * 0.5;
    assert!(
        (spun - expected).abs() < 1.0e-12,
        "spun to {spun}, not {expected}"
    );

    for _ in 0..600 {
        phys.step(DT);
    }
    let held = body(&phys, wheel).angular_velocity.z;
    assert!(
        (held - speed).abs() < 1.0e-12,
        "held {held} rad/s, not {speed}"
    );
}

/// **A hinge's limit stops the swing at its angle.** A bar hinged level with
/// a limit from −0.5 to 0.5 rad hangs at −0.5 rad under gravity.
///
/// Measured on 2026-09-23: it rests at −0.50034 rad, and swung 3.2 mrad
/// past its limit at worst on the way.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's atan2 reads the angle the limit is measured against"
)]
fn a_hinge_limit_stops_the_swing_at_its_angle() {
    let mut phys = system();
    let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bar = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let hinge = phys
        .add_joint(joint_at(
            &phys,
            post,
            bar,
            Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
            JointKind::Revolute(RevoluteJoint::hinge().with_limits(-0.5, 0.5)),
        ))
        .expect("a valid hinge");
    let mut past: f64 = 0.0;
    for _ in 0..180 {
        phys.step(DT);
        past = past.max(phys.joint_drift(hinge).expect("the hinge").angular);
    }
    let q = rotation(&phys, bar);
    let angle = 2.0 * q.z.atan2(q.w);
    assert!(
        (angle + 0.5).abs() < 5.0e-4,
        "rests at {angle} rad, not -0.5"
    );
    assert!(past < 5.0e-3, "swung {past} rad past its limit");
}

/// **A slider's motor drives it at its speed, and its limit stops it.** A
/// weightless block on a slider driven at 0.5 m/s with a limit at one metre
/// moves at 0.5 m/s and stops at one metre.
///
/// Measured on 2026-09-23: 0.5 m/s to rounding, and it stopped at
/// 1.00023 m.
#[test]
fn a_slider_motor_drives_to_its_limit_and_stops() {
    let mut phys = weightless();
    let rail = anchor(&mut phys, 0, DVec3::ZERO);
    let block = box_(
        &mut phys,
        1,
        DVec3::ZERO,
        DQuat::IDENTITY,
        DVec3::splat(0.2),
        3.0,
    );
    phys.add_joint(joint_at(
        &phys,
        rail,
        block,
        Transform::IDENTITY,
        JointKind::Prismatic(
            PrismaticJoint::slider()
                .with_motor(0.5, 100.0)
                .with_limits(-1.0, 1.0),
        ),
    ))
    .expect("a valid slider");
    for _ in 0..60 {
        phys.step(DT);
    }
    let speed = body(&phys, block).velocity.x;
    assert!((speed - 0.5).abs() < 1.0e-12, "moving at {speed} m/s");
    for _ in 0..120 {
        phys.step(DT);
    }
    let x = position(&phys, block).x;
    let speed = body(&phys, block).velocity.x;
    assert!((x - 1.0).abs() < 5.0e-4, "stopped at {x} m, not 1");
    assert!(speed.abs() < 1.0e-9, "still moving at {speed} m/s");
}

/// **A distance joint's limits hold a soft spring between them.** A ball on
/// a weak spring of rest length one metre, with limits from 0.9 to 1.1,
/// hangs at 1.1 under a weight the spring alone would let fall further.
///
/// Measured on 2026-09-23: it hangs at 1.10006 m.
#[test]
fn a_distance_limit_holds_a_weak_spring() {
    let mut phys = system();
    let top = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bob = ball(&mut phys, 1, DVec3::new(0.0, 2.0, 0.0), 0.05, 1.0, CRATE);
    phys.add_joint(Joint::new(
        top,
        bob,
        Transform::IDENTITY,
        Transform::IDENTITY,
        JointKind::Distance(DistanceJoint::spring(1.0, 0.5, 1.0).with_limits(0.9, 1.1)),
    ))
    .expect("a valid spring");
    for _ in 0..240 {
        phys.step(DT);
    }
    let length = 3.0 - position(&phys, bob).y;
    assert!(
        (length - 1.1).abs() < 2.0e-4,
        "hangs at {length} m, not 1.1"
    );
}

/// **A ball joint's twist limit stops a twist.** A weightless rod on a ball
/// joint with twist limits of ±0.3 rad, spun about its own axis, stops at
/// 0.3 rad.
///
/// Measured on 2026-09-23: it stopped at 0.29999 rad, the arc tangent's
/// error and no more.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's atan2 reads the angle the limit is measured against"
)]
fn a_ball_joint_twist_limit_stops_a_twist() {
    let mut phys = weightless();
    let base = anchor(&mut phys, 0, DVec3::ZERO);
    let rod = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 0.0, 0.5),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.05, 0.5),
        1.0,
    );
    let socket = phys
        .add_joint(joint_at(
            &phys,
            base,
            rod,
            Transform::IDENTITY,
            JointKind::Spherical(SphericalJoint::ball().with_cone(0.5).with_twist(-0.3, 0.3)),
        ))
        .expect("a valid ball joint");
    phys.body_mut(rod).expect("the rod").angular_velocity = DVec3::new(0.0, 0.0, 3.0);
    for _ in 0..120 {
        phys.step(DT);
    }
    let q = rotation(&phys, rod);
    let twist = 2.0 * q.z.atan2(q.w);
    let drift = phys.joint_drift(socket).expect("the socket");
    assert!(
        drift.angular < 1.0e-3,
        "past the twist limit by {} rad",
        drift.angular
    );
    assert!(
        (twist - 0.3).abs() < 1.0e-4,
        "twisted to {twist} rad, not 0.3"
    );
}

// ---------------------------------------------------------------------------
// Anatomical limits: an elliptical swing, and soft zones before the stops
// ---------------------------------------------------------------------------

/// Frame B's swing from frame A's in `q`, as the angles about frame A's x
/// and y of its quaternion's x and y parts — the parts an elliptical swing
/// limit bounds — and its twist about its own z-axis.
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's asin and atan2 read the angles the limits are measured against"
)]
fn swing_and_twist(q: DQuat) -> (f64, f64, f64) {
    let s = (q.z * q.z + q.w * q.w).sqrt();
    let x = (q.w * q.x - q.z * q.y) / s;
    let y = (q.w * q.y + q.z * q.x) / s;
    let twist = if q.w < 0.0 {
        2.0 * (-q.z).atan2(-q.w)
    } else {
        2.0 * q.z.atan2(q.w)
    };
    (2.0 * x.asin(), 2.0 * y.asin(), twist)
}

/// A soft zone's strength, as the tests below set it.
fn zone(hertz: f64, damping_ratio: f64) -> Spring {
    Spring {
        hertz,
        damping_ratio,
    }
}

/// The half-extents and mass of the block the loaded tests turn, centred on
/// its joint so the joint's rows alone hold its turn.
const LOADED_HALF: DVec3 = DVec3::new(0.3, 0.2, 0.1);
const LOADED_MASS: f64 = 2.0;

/// A weightless block centred on a joint of `kind` to a fixed anchor, both
/// frames at the block's centre, standing at `start` and turning at `spin`,
/// with `torque` applied every tick for `ticks`: the system, the block and
/// the joint.
fn loaded(
    kind: JointKind,
    start: DQuat,
    spin: DVec3,
    torque: DVec3,
    ticks: u32,
) -> (PhysicsSystem, Entity, crcbl_phys::JointId) {
    let mut phys = weightless();
    let base = anchor(&mut phys, 0, DVec3::ZERO);
    let block = box_(&mut phys, 1, DVec3::ZERO, start, LOADED_HALF, LOADED_MASS);
    phys.body_mut(block).expect("the block").angular_velocity = spin;
    let joint = phys
        .add_joint(Joint::new(
            base,
            block,
            Transform::IDENTITY,
            Transform::IDENTITY,
            kind,
        ))
        .expect("a valid joint");
    for _ in 0..ticks {
        phys.apply_torque(block, torque);
        phys.step(DT);
    }
    (phys, block, joint)
}

/// **An elliptical swing holds each axis to its own half-angle.** A
/// weightless rod on a ball joint whose swing is 0.3 rad about x and 0.9 rad
/// about y, with a little friction, pushed by a torque square to it that
/// sweeps round the circle twice: about x it stops at 0.3 rad, about y it
/// goes on to 0.9, and between them it never leaves the ellipse.
///
/// Measured on 2026-11-01: it reached 0.29986 rad about x and 0.90135
/// about y, and left the ellipse by 1.8 mrad at worst, sliding round its
/// tight end — a circular cone of 0.3, 0.6 or 0.9 rad in the same sweep
/// leaves by 0.6 to 3.7 mrad. With the gap inside measured straight ahead
/// rather than to the nearest of the ellipse, it left by 3.2 mrad.
#[test]
fn an_elliptical_swing_holds_each_axis_to_its_own_half_angle() {
    const X: f64 = 0.3;
    const Y: f64 = 0.9;
    let mut phys = weightless();
    let base = anchor(&mut phys, 0, DVec3::ZERO);
    let rod = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 0.0, 0.5),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.05, 0.5),
        1.0,
    );
    let socket = phys
        .add_joint(joint_at(
            &phys,
            base,
            rod,
            Transform::IDENTITY,
            JointKind::Spherical(
                SphericalJoint::ball()
                    .with_swing(X, Y)
                    .with_motor(DVec3::ZERO, 0.2),
            ),
        ))
        .expect("a valid ball joint");
    let (mut most_x, mut most_y, mut past): (f64, f64, f64) = (0.0, 0.0, 0.0);
    for tick in 0..480 {
        let phase = core::f64::consts::TAU * f64::from(tick) / 240.0;
        let push = 3.0
            * DVec3::new(
                crcbl_core::trig::cos(phase),
                crcbl_core::trig::sin(phase),
                0.0,
            );
        // Square to the rod: a swing, not a twist of its thin axis.
        let along = rotation(&phys, rod) * DVec3::Z;
        phys.apply_torque(rod, push - push.dot(along) * along);
        phys.step(DT);
        let (x, y, _) = swing_and_twist(rotation(&phys, rod));
        most_x = most_x.max(x.abs());
        most_y = most_y.max(y.abs());
        past = past.max(phys.joint_drift(socket).expect("the socket").angular);
    }
    assert!(
        (most_x - X).abs() < 5.0e-3,
        "swung {most_x} rad about x, not {X}"
    );
    assert!(
        (most_y - Y).abs() < 5.0e-3,
        "swung {most_y} rad about y, not {Y}"
    );
    assert!(past < 5.0e-3, "past the ellipse by {past} rad");
}

/// The hard and soft bounds the soft-zone tests set, in radians, and the
/// zone's strength at the hard bound.
const HARD: f64 = 1.0;
const SOFT: f64 = 0.5;
const ZONE_HERTZ: f64 = 4.0;

/// The depth past [`SOFT`] at which a zone of [`ZONE_HERTZ`] over the
/// effective inertia `inertia` holds `load`: stiffness `I ω² x` at depth
/// fraction `x = d / (HARD − SOFT)`, so `load = I ω² d² / (HARD − SOFT)`.
fn zone_depth(load: f64, inertia: f64) -> f64 {
    let omega = core::f64::consts::TAU * ZONE_HERTZ;
    (load * (HARD - SOFT) / (inertia * omega * omega)).sqrt()
}

/// The zone's ramp, checked by its shape alone: torque `∝ depth²`, so the
/// depth goes as the root of the load — loads in the ratio 1 : 4 : 9 hold
/// it at depths 1 : 2 : 3, where a spring of one stiffness gives 1 : 4 : 9.
fn assert_depth_goes_as_the_root_of_the_load(name: &str, loads: &[f64], depths: &[f64]) {
    for (load, depth) in loads.iter().zip(depths) {
        let expected = (load / loads[0]).sqrt();
        let ratio = depth / depths[0];
        assert!(
            (ratio / expected - 1.0).abs() < 1.0e-3,
            "{name}: {load} held it {ratio} times as deep as {}, not {expected}",
            loads[0]
        );
    }
}

/// **A soft zone resists more the deeper it is pushed, and never past the
/// hard stop.** A weightless block on a hinge with hard limits of ±1 rad and
/// soft ones of ±0.5, its zone 4 Hz at the stop, held at rest by loads of
/// 1, 4 and 9 N·m in turn: the joint carries each load, the depths go
/// 1 : 2 : 3 as a stiffness rising with depth makes them, and each is the
/// depth the ramp predicts. A load far past what the zone holds stops at the
/// hard limit.
///
/// Measured on 2026-11-01: 1, 4 and 9 N·m held it 0.09555, 0.19113 and
/// 0.28671 rad past the soft limit, the ramp's depth to 2e-4 of itself, and
/// the joint carried each load to 1e-15 of it; 60 N·m, twice what the zone
/// holds at the stop, rested 2.6 mrad past the hard limit, the hard limit's
/// own softness.
#[test]
fn a_soft_hinge_limit_resists_more_the_deeper_it_goes() {
    let inertia = MassProperties::cuboid(LOADED_MASS, LOADED_HALF, DVec3::ZERO)
        .inertia
        .z_axis
        .z;
    let hinge = RevoluteJoint::hinge()
        .with_limits(-HARD, HARD)
        .with_soft_limits(-SOFT, SOFT, zone(ZONE_HERTZ, 1.0));
    let loads = [1.0, 4.0, 9.0];
    let mut depths = Vec::new();
    for load in loads {
        let (phys, block, joint) = loaded(
            JointKind::Revolute(hinge),
            DQuat::IDENTITY,
            DVec3::ZERO,
            DVec3::Z * load,
            300,
        );
        let (_, _, angle) = swing_and_twist(rotation(&phys, block));
        let (_, carried) = phys.joint_reaction(joint).expect("the hinge");
        assert!(
            (carried - load).abs() < 1.0e-3 * load,
            "{load} N·m: carried {carried}"
        );
        depths.push(angle - SOFT);
    }
    assert_depth_goes_as_the_root_of_the_load("hinge", &loads, &depths);
    for (load, depth) in loads.into_iter().zip(depths) {
        let predicted = zone_depth(load, inertia);
        assert!(
            (depth / predicted - 1.0).abs() < 1.0e-3,
            "{load} N·m: {depth} rad past, not {predicted}"
        );
    }

    let (phys, block, joint) = loaded(
        JointKind::Revolute(hinge),
        DQuat::IDENTITY,
        DVec3::ZERO,
        DVec3::Z * 60.0,
        300,
    );
    let (_, _, angle) = swing_and_twist(rotation(&phys, block));
    let past = phys.joint_drift(joint).expect("the hinge").angular;
    assert!(
        past < 5.0e-3 && angle < HARD + 5.0e-3,
        "at {angle} rad, {past} past the stop"
    );
}

/// **A ball joint's soft swing and soft twist resist more the deeper they
/// are pushed.** The loaded block on a ball joint, first with a swing of
/// ±1 rad about x and a soft one of ±0.5 pushed about x, then with a twist
/// of ±1 rad and a soft one of ±0.5 pushed about z, by loads in the ratio
/// 1 : 4 : 9: the depths go 1 : 2 : 3, and each is the ramp's.
///
/// Measured on 2026-11-01: the depths were the ramp's to 3e-4 of
/// themselves, and neither came near its hard limit.
#[test]
fn a_ball_joints_soft_swing_and_twist_resist_more_the_deeper_they_go() {
    let inertia = MassProperties::cuboid(LOADED_MASS, LOADED_HALF, DVec3::ZERO).inertia;
    let swing = SphericalJoint::ball()
        .with_swing(HARD, 1.4)
        .with_soft_swing(SOFT, 0.9, zone(ZONE_HERTZ, 1.0));
    let twist = SphericalJoint::ball()
        .with_cone(1.4)
        .with_twist(-HARD, HARD)
        .with_soft_twist(-SOFT, SOFT, zone(ZONE_HERTZ, 1.0));
    for (name, joint, axis, inertia) in [
        ("swing", swing, DVec3::X, inertia.x_axis.x),
        ("twist", twist, DVec3::Z, inertia.z_axis.z),
    ] {
        let loads = [0.5, 2.0, 4.5];
        let mut depths = Vec::new();
        for load in loads {
            let (phys, block, id) = loaded(
                JointKind::Spherical(joint),
                DQuat::IDENTITY,
                DVec3::ZERO,
                axis * load,
                300,
            );
            let (x, _, t) = swing_and_twist(rotation(&phys, block));
            let past = phys.joint_drift(id).expect("the joint").angular;
            assert!(past == 0.0, "{name} {load}: {past} rad past the hard limit");
            depths.push(if axis == DVec3::X { x } else { t } - SOFT);
        }
        assert_depth_goes_as_the_root_of_the_load(name, &loads, &depths);
        for (load, depth) in loads.into_iter().zip(depths) {
            let predicted = zone_depth(load, inertia);
            assert!(
                (depth / predicted - 1.0).abs() < 1.0e-3,
                "{name} {load}: {depth} rad past, not {predicted}"
            );
        }
    }
}

/// **Inside its soft limits a joint is free.** The loaded block turning
/// slowly from its neutral pose inside soft limits — a hinge, then a ball
/// joint with a soft elliptical swing and a soft twist — turns as it would
/// with no limits at all, a hinge's spin held and a ball's tumbling as a
/// free block's does, and the joint carries no torque; set down at rest
/// inside them, it stays where it was put. Nothing pulls it towards
/// neutral.
///
/// Measured on 2026-11-01: both turned exactly as with no limits, to the
/// bit, and carried exactly nothing.
#[test]
fn inside_its_soft_limits_a_joint_is_free() {
    let hinge = JointKind::Revolute(
        RevoluteJoint::hinge()
            .with_limits(-HARD, HARD)
            .with_soft_limits(-SOFT, SOFT, zone(ZONE_HERTZ, 1.0)),
    );
    let ball = JointKind::Spherical(
        SphericalJoint::ball()
            .with_swing(0.8, 1.2)
            .with_soft_swing(0.5, 0.7, zone(ZONE_HERTZ, 1.0))
            .with_twist(-HARD, HARD)
            .with_soft_twist(-SOFT, SOFT, zone(ZONE_HERTZ, 1.0)),
    );
    for (name, kind, free, spin) in [
        (
            "hinge",
            hinge,
            JointKind::Revolute(RevoluteJoint::hinge()),
            DVec3::Z * 0.2,
        ),
        (
            "ball",
            ball,
            JointKind::Spherical(SphericalJoint::ball()),
            DVec3::new(0.1, 0.15, 0.2),
        ),
    ] {
        let (phys, block, joint) = loaded(kind, DQuat::IDENTITY, spin, DVec3::ZERO, 90);
        let (unlimited, free_block, _) = loaded(free, DQuat::IDENTITY, spin, DVec3::ZERO, 90);
        let w = body(&phys, block).angular_velocity;
        let free_w = body(&unlimited, free_block).angular_velocity;
        let apart = (rotation(&phys, block) * rotation(&unlimited, free_block).inverse())
            .xyz()
            .length();
        let (_, carried) = phys.joint_reaction(joint).expect("the joint");
        assert!(
            (w - free_w).length() < 1.0e-12 && apart < 1.0e-12,
            "{name}: spinning {w:?}, not {free_w:?}, {apart} apart"
        );
        assert!(
            name != "hinge" || (w - spin).length() < 1.0e-12,
            "{name}: spinning {w:?}, not {spin:?}"
        );
        assert!(carried == 0.0, "{name}: carried {carried} N·m");

        let put = rotation_from_scaled_axis(spin.normalize() * 0.4);
        let (phys, block, _) = loaded(kind, put, DVec3::ZERO, DVec3::ZERO, 120);
        let moved = (rotation(&phys, block) * put.inverse()).xyz().length();
        assert!(
            moved < 1.0e-12,
            "{name}: moved {moved} from where it was put"
        );
    }
}

/// The kinetic energy of `bodies` plus their weight's potential.
fn energy(phys: &PhysicsSystem, bodies: &[Entity]) -> f64 {
    bodies
        .iter()
        .map(|&e| {
            let b = body(phys, e);
            let t = phys.transform(e).expect("a body");
            b.kinetic_energy(t.rotation) + G * t.position.y / b.inverse_mass
        })
        .sum()
}

/// **Soft zones and an elliptical swing never gain energy.** Each run
/// undamped in its zone, so only the solver could add energy: a bar on a
/// hinge falling from level through its soft limit into its hard stop; a
/// block hung on a ball joint with an elliptical swing and a soft one,
/// knocked sideways; and a weightless rod spun into its soft and hard
/// twist. None ever rises above where it started.
///
/// Measured on 2026-11-01 over six seconds: the hinge never came within
/// 5 mJ of its start and the swing within 2.5 J, and the twist rose 3e-16 J
/// on its 0.03 J — the energy's own rounding, on its free spin.
#[test]
fn soft_zones_and_an_elliptical_swing_never_gain_energy() {
    let mut runs: Vec<(&str, PhysicsSystem, Entity)> = Vec::new();

    let mut phys = system();
    let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bar = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let hinge = joint_at(
        &phys,
        post,
        bar,
        Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
        JointKind::Revolute(
            RevoluteJoint::hinge()
                .with_limits(-1.2, 1.2)
                .with_soft_limits(-0.7, 0.7, zone(8.0, 0.0)),
        ),
    );
    phys.add_joint(hinge).expect("a valid hinge");
    runs.push(("hinge", phys, bar));

    let mut phys = system();
    let ceiling = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let block = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 2.5, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.5, 0.05),
        2.0,
    );
    let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
    let socket = joint_at(
        &phys,
        ceiling,
        block,
        Transform::new(DVec3::new(0.0, 3.0, 0.0), down),
        JointKind::Spherical(SphericalJoint::ball().with_swing(0.5, 1.0).with_soft_swing(
            0.3,
            0.6,
            zone(8.0, 0.0),
        )),
    );
    phys.add_joint(socket).expect("a valid ball joint");
    phys.body_mut(block).expect("the block").velocity = DVec3::new(3.0, 0.0, 1.5);
    runs.push(("swing", phys, block));

    let mut phys = weightless();
    let base = anchor(&mut phys, 0, DVec3::ZERO);
    let rod = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 0.0, 0.5),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.05, 0.5),
        1.0,
    );
    let twist = joint_at(
        &phys,
        base,
        rod,
        Transform::IDENTITY,
        JointKind::Spherical(
            SphericalJoint::ball()
                .with_cone(0.5)
                .with_twist(-0.6, 0.6)
                .with_soft_twist(-0.3, 0.3, zone(8.0, 0.0)),
        ),
    );
    phys.add_joint(twist).expect("a valid ball joint");
    phys.body_mut(rod).expect("the rod").angular_velocity = DVec3::new(0.0, 0.0, 6.0);
    runs.push(("twist", phys, rod));

    for (name, mut phys, e) in runs {
        let start = energy(&phys, &[e]);
        let mut highest = f64::NEG_INFINITY;
        for _ in 0..360 {
            phys.step(DT);
            highest = highest.max(energy(&phys, &[e]));
        }
        // The energy's own rounding, on the free spin before any zone.
        assert!(
            highest <= start * (1.0 + 1.0e-12),
            "{name}: rose {} J over its start",
            highest - start
        );
    }
}

/// A run of [`a_soft_zone_alone_never_gains_energy`]: its name, its system,
/// the body it weighs, and how far into its zone that body is.
type ZoneRun = (
    &'static str,
    PhysicsSystem,
    Entity,
    Box<dyn Fn(&PhysicsSystem) -> f64>,
);

/// **A soft zone alone never gains energy**, so the hard stop's losses
/// cannot hide a gain in it: undamped zones strong enough that the joint
/// never reaches its stop — a bar on a hinge falling from level into its
/// soft limit, and a block hung on a ball joint with an elliptical swing,
/// knocked into its soft ellipse. Each goes into its zone, never reaches its
/// hard limit, and never rises above where it started.
///
/// Measured on 2026-11-01 over six seconds: the hinge went 0.46 of the way
/// to its stop and never came within 5 mJ of its start, the swing 0.49 and
/// within 1.1 J.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's sine reads the swing against the ellipse it is held to"
)]
fn a_soft_zone_alone_never_gains_energy() {
    const HINGE: (f64, f64) = (0.7, 1.4);
    const SWING: [(f64, f64); 2] = [(0.3, 0.5), (1.2, 1.5)];

    let mut phys = system();
    let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bar = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let hinge = joint_at(
        &phys,
        post,
        bar,
        Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
        JointKind::Revolute(
            RevoluteJoint::hinge()
                .with_limits(-HINGE.1, HINGE.1)
                .with_soft_limits(-HINGE.0, HINGE.0, zone(8.0, 0.0)),
        ),
    );
    phys.add_joint(hinge).expect("a valid hinge");
    // How deep into the zone, as a fraction of the way to the stop.
    let into_hinge = move |phys: &PhysicsSystem| {
        let (_, _, angle) = swing_and_twist(rotation(phys, bar));
        (angle.abs() - HINGE.0) / (HINGE.1 - HINGE.0)
    };
    let mut runs: Vec<ZoneRun> = vec![("hinge", phys, bar, Box::new(into_hinge))];

    let mut phys = system();
    let ceiling = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let block = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 2.5, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.5, 0.05),
        2.0,
    );
    let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
    let socket = joint_at(
        &phys,
        ceiling,
        block,
        Transform::new(DVec3::new(0.0, 3.0, 0.0), down),
        JointKind::Spherical(
            SphericalJoint::ball()
                .with_swing(SWING[1].0, SWING[1].1)
                .with_soft_swing(SWING[0].0, SWING[0].1, zone(8.0, 0.0)),
        ),
    );
    phys.add_joint(socket).expect("a valid ball joint");
    phys.body_mut(block).expect("the block").velocity = DVec3::new(2.0, 0.0, 1.0);
    // The ellipse's function in the swing's own parts, at each bound: one
    // on the bound, and the depth fraction from the soft bound to the hard.
    let into_swing = move |phys: &PhysicsSystem| {
        let (x, y, _) = swing_and_twist(down.inverse() * rotation(phys, block) * down);
        let at = |(hx, hy): (f64, f64)| {
            let (px, py) = (
                (0.5 * x).sin() / (0.5 * hx).sin(),
                (0.5 * y).sin() / (0.5 * hy).sin(),
            );
            (px * px + py * py).sqrt()
        };
        (at(SWING[0]) - 1.0) / (at(SWING[0]) - at(SWING[1]))
    };
    runs.push(("swing", phys, block, Box::new(into_swing)));

    for (name, mut phys, e, into) in runs {
        let start = energy(&phys, &[e]);
        let (mut highest, mut deepest) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for _ in 0..360 {
            phys.step(DT);
            highest = highest.max(energy(&phys, &[e]));
            deepest = deepest.max(into(&phys));
        }
        assert!(
            deepest > 0.1,
            "{name}: only {deepest} of the way into its zone"
        );
        assert!(deepest < 0.9, "{name}: {deepest} of the way to its stop");
        assert!(
            highest <= start,
            "{name}: rose {} J over its start",
            highest - start
        );
    }
}

/// **A joint with everything on still sleeps.** On a floorless system with
/// sleep on: a bar on a hinge with its spring pulling to a target, its soft
/// zones and its hard limits, let go from level; and a block on a ball
/// joint with an elliptical swing, a soft swing, a twist, a soft twist and
/// a little friction, its neutral tipped from hanging, knocked sideways.
/// Both come to rest in a soft zone, held there by gravity, and sleep.
///
/// Measured on 2026-11-01: the bar rested 0.068 rad into its zone and slept
/// at tick 83, the block 0.053 rad into its and slept at tick 313. With the
/// zones' damping taken 1.2 below what they ask, the block never slept.
#[test]
fn a_joint_with_everything_on_still_sleeps() {
    let mut phys = system_with(ContactSettings::DEFAULT);
    let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let bar = box_(
        &mut phys,
        1,
        DVec3::new(0.5, 3.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.05, 0.05),
        2.0,
    );
    let hinge = joint_at(
        &phys,
        post,
        bar,
        Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
        JointKind::Revolute(
            RevoluteJoint::hinge()
                .with_spring(-0.3, zone(1.0, 0.5))
                .with_limits(-1.2, 1.2)
                .with_soft_limits(-0.7, 0.7, zone(8.0, 1.0)),
        ),
    );
    phys.add_joint(hinge).expect("a valid hinge");

    let ceiling = anchor(&mut phys, 2, DVec3::new(5.0, 3.0, 0.0));
    let block = box_(
        &mut phys,
        3,
        DVec3::new(5.0, 2.5, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.05, 0.5, 0.05),
        2.0,
    );
    // Frame B's z-axis down the block; frame A's tipped 0.6 rad from it, so
    // hanging is 0.6 rad of swing from neutral and the block rests in its
    // soft swing.
    let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
    let tipped = rotation_from_scaled_axis(DVec3::X * (core::f64::consts::FRAC_PI_2 + 0.6));
    let socket = Joint::new(
        ceiling,
        block,
        Transform::new(DVec3::ZERO, tipped),
        Transform::new(DVec3::new(0.0, 0.5, 0.0), down),
        JointKind::Spherical(
            SphericalJoint::ball()
                .with_swing(0.8, 1.0)
                .with_soft_swing(0.3, 0.6, zone(8.0, 1.0))
                .with_twist(-0.6, 0.6)
                .with_soft_twist(-0.3, 0.3, zone(8.0, 1.0))
                .with_motor(DVec3::ZERO, 0.5),
        ),
    );
    phys.add_joint(socket).expect("a valid ball joint");
    phys.body_mut(block).expect("the block").velocity = DVec3::new(3.0, 0.0, 1.5);

    let mut asleep = [None, None];
    for tick in 1..=900 {
        phys.step(DT);
        for (k, e) in [bar, block].into_iter().enumerate() {
            if asleep[k].is_none() && phys.is_sleeping(e) {
                asleep[k] = Some(tick);
            }
        }
    }
    assert!(asleep[0].is_some(), "the hinged bar never slept");
    assert!(asleep[1].is_some(), "the ball-jointed block never slept");
}

// ---------------------------------------------------------------------------
// Breaking
// ---------------------------------------------------------------------------

/// A 10 kg weight hung on a rigid rod whose joint breaks at `threshold`
/// newtons, stepped `ticks`: the system and the weight.
fn hung_weight(threshold: f64, ticks: u32) -> (PhysicsSystem, Entity, crcbl_phys::JointId, usize) {
    let mut phys = system();
    let hook = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let weight = ball(&mut phys, 1, DVec3::new(0.0, 2.0, 0.0), 0.1, 10.0, CRATE);
    let rod = phys
        .add_joint(
            Joint::new(
                hook,
                weight,
                Transform::IDENTITY,
                Transform::IDENTITY,
                JointKind::Distance(DistanceJoint::rigid(1.0)),
            )
            .breaking_at(threshold, f64::INFINITY),
        )
        .expect("a valid rod");
    let mut broke = 0;
    for _ in 0..ticks {
        phys.step(DT);
        broke += phys.broken_joints().len();
        assert_eq!(
            phys.contact_counters().broken_joints,
            phys.broken_joints().len()
        );
    }
    (phys, weight, rod, broke)
}

/// **A joint breaks at its threshold and not below it.** A 10 kg weight on
/// a rod pulls 98.1 N: a rod that breaks at 103 N holds it for two seconds,
/// carrying 98.1 N, and one that breaks at 93 N lets go at once, is
/// reported, is gone, and the weight falls.
///
/// Measured on 2026-09-23: the holding rod carried 98.100 N; the other
/// broke on its first tick, carrying 93.9 N.
#[test]
fn a_joint_breaks_at_its_threshold_and_not_below_it() {
    let weight_force = 10.0 * G;
    let (phys, weight, rod, broke) = hung_weight(1.05 * weight_force, 120);
    let (force, _) = phys.joint_reaction(rod).expect("the rod holds");
    assert_eq!(broke, 0, "a rod below its threshold broke");
    assert!(
        (force - weight_force).abs() < 1.0e-3 * weight_force,
        "carries {force} N"
    );
    assert!((position(&phys, weight).y - 2.0).abs() < 1.0e-3);

    let mut phys = system();
    let hook = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
    let weight = ball(&mut phys, 1, DVec3::new(0.0, 2.0, 0.0), 0.1, 10.0, CRATE);
    let rod = phys
        .add_joint(
            Joint::new(
                hook,
                weight,
                Transform::IDENTITY,
                Transform::IDENTITY,
                JointKind::Distance(DistanceJoint::rigid(1.0)),
            )
            .breaking_at(0.95 * weight_force, f64::INFINITY),
        )
        .expect("a valid rod");
    phys.step(DT);
    let breaks = phys.broken_joints().to_vec();
    assert_eq!(breaks.len(), 1, "one break");
    assert_eq!(breaks[0].joint, rod);
    assert_eq!((breaks[0].body_a, breaks[0].body_b), (hook, weight));
    assert!(
        breaks[0].force >= 0.95 * weight_force,
        "broke at {} N",
        breaks[0].force
    );
    assert!(phys.joint(rod).is_none(), "a broken joint is gone");
    assert_eq!(phys.joint_count(), 0);
    for _ in 0..30 {
        phys.step(DT);
    }
    assert!(phys.broken_joints().is_empty(), "a break is reported once");
    assert!(position(&phys, weight).y < 1.0, "the weight did not fall");
}

// ---------------------------------------------------------------------------
// The cradle and the bridge
// ---------------------------------------------------------------------------

/// Steel on steel for the cradle: no friction to spin the balls, and a
/// perfect bounce.
const STEEL: SurfaceMaterial = SurfaceMaterial::new(0.0, 1.0);

/// The cradle's balls' radius, and the gap between two.
const CRADLE_RADIUS: f64 = 0.1;
const CRADLE_GAP: f64 = 0.025;

/// A gapped Newton's cradle of `count` 1 kg balls, each hung on two rigid
/// rods from a bar 1.5 m above it, the first drawn back to `height`: the
/// system and the balls.
fn cradle(count: u32, height: f64) -> (PhysicsSystem, Vec<Entity>) {
    const DROP: f64 = 1.0;
    const TOP: f64 = 3.0;
    const SPREAD: f64 = 0.3;
    let mut phys = system();
    let pitch = 2.0 * CRADLE_RADIUS + CRADLE_GAP;
    let mut balls = Vec::new();
    for k in 0..count {
        let x = pitch * f64::from(k);
        let rest = DVec3::new(x, TOP - DROP, 0.0);
        let bob = ball(&mut phys, 100 + k, rest, CRADLE_RADIUS, 1.0, STEEL);
        for (side, z) in [(0, -SPREAD), (1, SPREAD)] {
            let hook = anchor(&mut phys, 2 * k + side, DVec3::new(x, TOP, z));
            let length = (DVec3::new(x, TOP, z) - rest).length();
            phys.add_joint(Joint::new(
                hook,
                bob,
                Transform::IDENTITY,
                Transform::IDENTITY,
                JointKind::Distance(DistanceJoint::rigid(length)),
            ))
            .expect("a valid rod");
        }
        balls.push(bob);
    }
    // The first ball drawn back along its circle to `height`.
    let angle_cos = 1.0 - height / DROP;
    let angle_sin = (1.0 - angle_cos * angle_cos).sqrt();
    phys.set_transform(
        balls[0],
        Transform::from_position(DVec3::new(-DROP * angle_sin, TOP - DROP * angle_cos, 0.0)),
    );
    (phys, balls)
}

/// **A gapped Newton's cradle passes its momentum along.** Five 1 kg steel
/// balls 2.5 cm apart, the first let go from 20 cm: the momentum the last
/// ball leaves with is all but what the first arrived with, and when it
/// leaves the balls between are all but still.
///
/// The gap is what makes it a cradle this solver can pass. Wider than
/// [`ContactSettings::speculative_distance`], it leaves each pair without a
/// contact until the ball before is moving at it, so each impact is its own
/// two-ball collision a tick after the last, which the restitution pass
/// makes elastic. Two millimetres apart, the next pair's contact was already
/// in the solve when the ball between was struck, and closed inelastically
/// within the tick: measured on 2026-09-23, only 41% of the momentum came
/// out.
///
/// Measured on 2026-09-23: 1.9808 kg·m/s in against the 1.9809 a free swing
/// arrives with, 1.9734 out (99.6%), and 0.033 m/s the fastest middle ball
/// as the last one left.
#[test]
fn a_gapped_newtons_cradle_passes_its_momentum() {
    let (mut phys, balls) = cradle(5, 0.2);
    let first = balls[0];
    let last = *balls.last().expect("a last ball");
    let (mut into, mut out, mut middle): (f64, f64, f64) = (0.0, 0.0, 0.0);
    let arrival = (2.0 * G * 0.2).sqrt();
    for _ in 0..60 {
        phys.step(DT);
        into = into.max(body(&phys, first).velocity.x);
        let leaving = body(&phys, last).velocity.x;
        if leaving > out {
            out = leaving;
            middle = balls[1..balls.len() - 1]
                .iter()
                .map(|&ball| body(&phys, ball).velocity.length())
                .fold(0.0, f64::max);
        }
    }
    assert!(into > 0.99 * arrival, "the first ball arrived at {into}");
    assert!(out > 0.99 * into, "only {out} of {into} came out");
    assert!(middle < 0.05 * into, "a middle ball moved at {middle}");
}

/// The static equilibrium of a hanging chain of rigid links of `length`,
/// hinged end to end between two anchors `span` apart at one height, with
/// each link's weight `weights[i]` acting at its middle: the height of each
/// link's middle below the anchors, and each link's slope as `tan θ`.
///
/// Every link carries the horizontal tension `H`; its moment about its left
/// hinge sets `tan θᵢ = (Vᵢ + wᵢ / 2) / H`, `Vᵢ` the vertical force at that
/// hinge, which starts at minus half the whole weight and gains each link's
/// weight in turn. `H` is found by bisection so the links span the gap.
/// Algebraic throughout — nothing here is a transcendental.
fn hanging_chain(weights: &[f64], length: f64, span: f64) -> (Vec<f64>, Vec<f64>) {
    let total: f64 = weights.iter().sum();
    let slopes = |tension: f64| -> Vec<f64> {
        let mut vertical = -0.5 * total;
        weights
            .iter()
            .map(|&w| {
                let slope = (vertical + 0.5 * w) / tension;
                vertical += w;
                slope
            })
            .collect()
    };
    let reach = |tension: f64| -> f64 {
        slopes(tension)
            .iter()
            .map(|t| length / (1.0 + t * t).sqrt())
            .sum()
    };
    let (mut low, mut high) = (1.0e-6 * total, 1.0e6 * total);
    for _ in 0..200 {
        let mid = 0.5 * (low + high);
        if reach(mid) < span {
            low = mid;
        } else {
            high = mid;
        }
    }
    let slopes = slopes(0.5 * (low + high));
    let mut y = 0.0;
    let mut middles = Vec::new();
    for t in &slopes {
        let dy = length * t / (1.0 + t * t).sqrt();
        middles.push(-(y + 0.5 * dy));
        y += dy;
    }
    (middles, slopes)
}

/// A plank bridge: `planks` planks of 5 kg, half a metre long, hinged end to
/// end between two fixed anchors `span` apart at `height`, laid in the
/// hanging chain's equilibrium: the system, the planks and the joints.
fn bridge(
    phys: &mut PhysicsSystem,
    planks: u32,
    span: f64,
    height: f64,
) -> (Vec<Entity>, Vec<crcbl_phys::JointId>) {
    const HALF: DVec3 = DVec3::new(0.25, 0.05, 0.5);
    const MASS: f64 = 5.0;
    let weights = vec![MASS * G; planks as usize];
    let (_, slopes) = hanging_chain(&weights, 2.0 * HALF.x, span);
    let left = anchor(phys, 0, DVec3::new(-0.5 * span, height, 0.0));
    let right = anchor(phys, 1, DVec3::new(0.5 * span, height, 0.0));
    let mut hinge = DVec3::new(-0.5 * span, height, 0.0);
    let mut entities = Vec::new();
    for (k, slope) in slopes.iter().enumerate() {
        let cos = 1.0 / (1.0 + slope * slope).sqrt();
        let sin = slope * cos;
        let direction = DVec3::new(cos, sin, 0.0);
        // The turn about z whose cosine and sine these are.
        let half_cos = (0.5 * (1.0 + cos)).sqrt();
        let half_sin = (0.5 * (1.0 - cos)).sqrt().copysign(sin);
        let turn = DQuat::from_xyzw(0.0, 0.0, half_sin, half_cos);
        let centre = hinge + direction * HALF.x;
        entities.push(box_(
            phys,
            10 + u32::try_from(k).expect("a few planks"),
            centre,
            turn,
            HALF,
            MASS,
        ));
        hinge += direction * 2.0 * HALF.x;
    }
    let mut joints = Vec::new();
    let mut previous = left;
    let mut at = DVec3::new(-0.5 * span, height, 0.0);
    for &plank in &entities {
        joints.push(
            phys.add_joint(joint_at(
                phys,
                previous,
                plank,
                Transform::from_position(at),
                JointKind::Revolute(RevoluteJoint::hinge()),
            ))
            .expect("a valid hinge"),
        );
        let t = phys.transform(plank).expect("a plank");
        at = t.position + t.rotation * DVec3::new(HALF.x, 0.0, 0.0);
        previous = plank;
    }
    joints.push(
        phys.add_joint(joint_at(
            phys,
            previous,
            right,
            Transform::from_position(DVec3::new(0.5 * span, height, 0.0)),
            JointKind::Revolute(RevoluteJoint::hinge()),
        ))
        .expect("a valid hinge"),
    );
    (entities, joints)
}

/// The loaded bridge's planks: twenty-one across 9.5 m, their anchors 4 m up.
const LOADED_PLANKS: u32 = 21;
const LOADED_SPAN: f64 = 9.5;
const LOADED_HEIGHT: f64 = 4.0;
/// The crate on its middle plank, in kilograms.
const LOAD: f64 = 40.0;

/// The loaded bridge, its planks asking for `substeps`: the system, the
/// planks and the crate resting on the middle one.
fn loaded_bridge(substeps: u32) -> (PhysicsSystem, Vec<Entity>, Entity) {
    let mut phys = system();
    let (planks, _) = bridge(&mut phys, LOADED_PLANKS, LOADED_SPAN, LOADED_HEIGHT);
    for &plank in &planks {
        phys.set_substeps(plank, substeps);
    }
    let middle = planks[planks.len() / 2];
    let deck = position(&phys, middle) + DVec3::new(0.0, 0.05 + 0.2, 0.0);
    let load = box_(
        &mut phys,
        200,
        deck,
        DQuat::IDENTITY,
        DVec3::splat(0.2),
        LOAD,
    );
    (phys, planks, load)
}

/// How far a loaded bridge's middle plank sags below its anchors after
/// three seconds, against the hanging chain's, with its planks asking for
/// `substeps`, and the worst joint error.
fn bridge_sag(substeps: u32) -> (f64, f64, f64) {
    const PLANKS: u32 = LOADED_PLANKS;
    const SPAN: f64 = LOADED_SPAN;
    const HEIGHT: f64 = LOADED_HEIGHT;
    let (mut phys, planks, _) = loaded_bridge(substeps);
    let middle = planks[planks.len() / 2];
    let mut worst: f64 = 0.0;
    for _ in 0..180 {
        phys.step(DT);
        worst = worst.max(phys.contact_counters().joint_error);
    }
    let sag = HEIGHT - position(&phys, middle).y;
    let mut weights = vec![5.0 * G; PLANKS as usize];
    weights[PLANKS as usize / 2] += LOAD * G;
    let (middles, _) = hanging_chain(&weights, 0.5, SPAN);
    let expected = middles[PLANKS as usize / 2];
    (sag, expected, worst)
}

/// **A loaded plank bridge sags as the hanging chain says.** Twenty-one
/// hinged planks across 9.5 m, a 40 kg crate on the middle one: after three
/// seconds the middle plank hangs within the bound of the depth a chain of
/// rigid links would, which is algebra.
///
/// Measured on 2026-09-23, and again on 2026-11-01 once a jointed body's
/// contacts began cancelling part of its joints' change in the step: 2.188 m
/// against 2.100 — the joints stretched 5.1 mm at worst, and twenty-two of
/// them lengthen the chain — at the defaults. The chain is right: with no load it hangs 1.953 m deep, where a
/// catenary of the same length and span hangs 1.96 m.
#[test]
fn a_loaded_bridge_sags_as_a_hanging_chain_says() {
    let (sag, expected, worst) = bridge_sag(0);
    assert!(
        (sag - expected).abs() < 0.1,
        "sags {sag} m, not {expected} m"
    );
    assert!(worst < 1.0e-2, "a joint drifted {worst} m");
}

/// **A loaded bridge never gains energy.** The loaded bridge above, awake:
/// its planks' and the crate's kinetic energy plus their weight's potential,
/// tick by tick over five seconds, never rises above where it started and
/// ends lower. A joint correction the step takes only half of, put back by
/// the joint's spring, adds energy — measured on 2026-09-23, the unloaded
/// bridge gained 3.4 kJ in three seconds that way — and the crate is the
/// contact that shares the middle plank's load with its joints.
///
/// Measured on 2026-11-01: it never came within 0.56 J of its start and ended
/// 94.5 J lower. With every joint's change taken at the midpoint instead, it
/// rose 5.6 kJ over its start.
#[test]
fn a_loaded_bridge_never_gains_energy() {
    let (mut phys, planks, load) = loaded_bridge(0);
    let bodies: Vec<Entity> = planks.iter().copied().chain([load]).collect();
    let energy = |phys: &PhysicsSystem| -> f64 {
        bodies
            .iter()
            .map(|&e| {
                let b = body(phys, e);
                let t = phys.transform(e).expect("a body");
                b.kinetic_energy(t.rotation) + G * t.position.y / b.inverse_mass
            })
            .sum()
    };
    let start = energy(&phys);
    let mut highest = f64::NEG_INFINITY;
    for _ in 0..300 {
        phys.step(DT);
        highest = highest.max(energy(&phys));
    }
    let end = energy(&phys);
    assert!(
        highest <= start,
        "it rose {} J over its start",
        highest - start
    );
    assert!(end < start, "it ended {} J over its start", end - start);
}

/// **More substeps for the bridge's group hold it closer to the chain.**
/// The same bridge with its planks asking for twelve substeps sags nearer
/// the chain's depth, its joints stretched less.
///
/// Measured on 2026-09-23 after three seconds: 2.116 m against the chain's
/// 2.100, the worst joint error 0.58 mm, where four substeps sagged 2.188 m
/// with 5.1 mm.
#[test]
fn more_substeps_for_the_bridge_hold_it_closer() {
    let (sag4, expected, worst4) = bridge_sag(0);
    let (sag12, _, worst12) = bridge_sag(12);
    assert!(
        (sag12 - expected).abs() < 0.02,
        "twelve substeps sagged {sag12} m against {expected} m, four {sag4} m"
    );
    assert!(
        worst12 < 1.0e-3 && worst12 < worst4,
        "joint error {worst12} at twelve against {worst4}"
    );
}

// ---------------------------------------------------------------------------
// Islands, sleep and collisions
// ---------------------------------------------------------------------------

/// **A new joint wakes a sleeping island.** A box asleep on the floor is
/// jointed to a fixed anchor a metre above it: the joint wakes it, and the
/// rope pulls it up.
#[test]
fn a_new_joint_wakes_a_sleeping_island() {
    let mut phys = system_with(ContactSettings::DEFAULT);
    phys.add_plane(DVec3::Y, 0.0, CRATE);
    let crate_ = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 0.25, 0.0),
        DQuat::IDENTITY,
        DVec3::splat(0.25),
        10.0,
    );
    for _ in 0..120 {
        phys.step(DT);
    }
    assert!(phys.is_sleeping(crate_), "the box should be asleep");
    let hook = anchor(&mut phys, 0, DVec3::new(0.0, 1.5, 0.0));
    // A spring of rest length half a metre, hung from 1.5 m, lifts it.
    phys.add_joint(Joint::new(
        hook,
        crate_,
        Transform::IDENTITY,
        Transform::from_position(DVec3::new(0.0, 0.25, 0.0)),
        JointKind::Distance(DistanceJoint::spring(0.5, 2.0, 0.5)),
    ))
    .expect("a valid spring");
    assert!(!phys.is_sleeping(crate_), "the new joint did not wake it");
    for _ in 0..60 {
        phys.step(DT);
    }
    assert!(
        position(&phys, crate_).y > 0.35,
        "the spring did not lift it"
    );
}

/// **A jointed pair is one island**: it sleeps together, and touching one
/// wakes the other, though the two never touch.
#[test]
fn a_jointed_pair_sleeps_and_wakes_as_one_island() {
    let mut phys = system_with(ContactSettings::DEFAULT);
    phys.add_plane(DVec3::Y, 0.0, CRATE);
    let a = box_(
        &mut phys,
        1,
        DVec3::new(0.0, 0.25, 0.0),
        DQuat::IDENTITY,
        DVec3::splat(0.25),
        10.0,
    );
    let b = box_(
        &mut phys,
        2,
        DVec3::new(2.0, 0.25, 0.0),
        DQuat::IDENTITY,
        DVec3::splat(0.25),
        10.0,
    );
    phys.add_joint(joint_at(
        &phys,
        a,
        b,
        Transform::from_position(DVec3::new(1.0, 0.25, 0.0)),
        JointKind::Distance(DistanceJoint::rope(2.5)),
    ))
    .expect("a valid rope");
    for _ in 0..120 {
        phys.step(DT);
    }
    assert!(
        phys.is_sleeping(a) && phys.is_sleeping(b),
        "the pair should sleep"
    );
    assert_eq!(phys.contact_counters().sleeping_islands, 1, "as one island");
    assert!(phys.apply_force(a, DVec3::new(-1.0, 0.0, 0.0)));
    assert!(!phys.is_sleeping(b), "waking one did not wake its partner");
}

/// **Jointed bodies do not collide unless the joint says so.** Two boxes
/// overlapping at a ball joint stay where they are; asked to collide, they
/// push apart.
#[test]
fn jointed_bodies_collide_only_when_asked() {
    for collide in [false, true] {
        let mut phys = weightless();
        let a = box_(
            &mut phys,
            1,
            DVec3::ZERO,
            DQuat::IDENTITY,
            DVec3::splat(0.5),
            1.0,
        );
        let b = box_(
            &mut phys,
            2,
            DVec3::new(0.6, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::splat(0.5),
            1.0,
        );
        phys.add_joint(
            joint_at(
                &phys,
                a,
                b,
                Transform::from_position(DVec3::new(0.3, 0.0, 0.0)),
                JointKind::Spherical(SphericalJoint::ball()),
            )
            .colliding(collide),
        )
        .expect("a valid joint");
        for _ in 0..30 {
            phys.step(DT);
        }
        let pushed =
            body(&phys, a).angular_velocity.length() + body(&phys, b).angular_velocity.length();
        let touching = phys.contact_counters().touching;
        if collide {
            assert!(touching > 0, "asked to collide, they did not");
        } else {
            assert_eq!(touching, 0, "a joint's bodies collided");
            assert!(pushed < 1.0e-9, "something pushed them: {pushed}");
        }
    }
}

/// A joint on an unregistered body, or on one body twice, is refused.
#[test]
fn a_joint_needs_two_registered_bodies() {
    let mut phys = weightless();
    let a = box_(
        &mut phys,
        1,
        DVec3::ZERO,
        DQuat::IDENTITY,
        DVec3::splat(0.5),
        1.0,
    );
    let hinge = JointKind::Revolute(RevoluteJoint::hinge());
    let refused = phys.add_joint(Joint::new(
        a,
        entity(9),
        Transform::IDENTITY,
        Transform::IDENTITY,
        hinge,
    ));
    assert_eq!(refused, Err(JointError::UnknownBody(entity(9))));
    let refused = phys.add_joint(Joint::new(
        a,
        a,
        Transform::IDENTITY,
        Transform::IDENTITY,
        hinge,
    ));
    assert_eq!(refused, Err(JointError::SameBody));
    let backwards = JointKind::Revolute(RevoluteJoint::hinge().with_limits(0.5, -0.5));
    let other = anchor(&mut phys, 2, DVec3::ZERO);
    assert!(matches!(
        phys.add_joint(Joint::new(
            a,
            other,
            Transform::IDENTITY,
            Transform::IDENTITY,
            backwards
        )),
        Err(JointError::Invalid(_))
    ));
}

/// **A swing or soft zone out of range is refused**, each by the rule it
/// breaks, and the same settings in range are accepted: a soft limit
/// outside its hard one, backwards, or without the hard limit on; a swing
/// half-angle below zero or past π, or zero about one axis only; a soft
/// swing wider than its swing; a zone whose strength is negative or not
/// finite.
#[test]
fn a_swing_or_soft_zone_out_of_range_is_refused() {
    let firm = zone(4.0, 1.0);
    let hinge = RevoluteJoint::hinge().with_limits(-1.0, 1.0);
    let ball = SphericalJoint::ball()
        .with_swing(0.8, 1.2)
        .with_twist(-1.0, 1.0);
    let cases = [
        (
            JointKind::Revolute(hinge.with_soft_limits(-0.5, 0.5, firm)),
            None,
        ),
        (
            JointKind::Spherical(ball.with_soft_swing(0.4, 1.2, firm)),
            None,
        ),
        (
            JointKind::Spherical(ball.with_soft_twist(-1.0, 0.2, firm)),
            None,
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_swing(0.1, core::f64::consts::PI)),
            None,
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_swing(0.0, 0.0)),
            None,
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_swing(0.0, core::f64::consts::PI)),
            Some("both zero or neither"),
        ),
        (
            JointKind::Spherical(ball.with_soft_swing(0.4, 0.0, firm)),
            Some("both zero or neither"),
        ),
        (
            JointKind::Revolute(hinge.with_soft_limits(-1.5, 0.5, firm)),
            Some("inside its hard"),
        ),
        (
            JointKind::Revolute(hinge.with_soft_limits(0.5, -0.5, firm)),
            Some("inside its hard"),
        ),
        (
            JointKind::Revolute(RevoluteJoint::hinge().with_soft_limits(-0.5, 0.5, firm)),
            Some("need its hard"),
        ),
        (
            JointKind::Revolute(hinge.with_soft_limits(-0.5, 0.5, zone(-1.0, 1.0))),
            Some("soft zone"),
        ),
        (
            JointKind::Revolute(hinge.with_soft_limits(-0.5, 0.5, zone(f64::INFINITY, 1.0))),
            Some("soft zone"),
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_swing(-0.1, 0.5)),
            Some("swing lies"),
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_swing(0.5, 3.2)),
            Some("swing lies"),
        ),
        (
            JointKind::Spherical(ball.with_soft_swing(0.9, 0.5, firm)),
            Some("soft swing"),
        ),
        (
            JointKind::Spherical(SphericalJoint::ball().with_soft_swing(0.1, 0.1, firm)),
            Some("soft swing"),
        ),
        (
            JointKind::Spherical(ball.with_soft_twist(0.3, -0.3, firm)),
            Some("soft twist"),
        ),
        (
            JointKind::Spherical(ball.with_soft_twist(-1.1, 0.3, firm)),
            Some("soft twist"),
        ),
        (
            JointKind::Spherical(ball.with_soft_twist(-0.3, 0.3, zone(1.0, f64::NAN))),
            Some("soft zones"),
        ),
    ];
    let mut phys = weightless();
    let a = anchor(&mut phys, 0, DVec3::ZERO);
    let b = box_(
        &mut phys,
        1,
        DVec3::ZERO,
        DQuat::IDENTITY,
        DVec3::splat(0.5),
        1.0,
    );
    for (kind, refusal) in cases {
        let joint = Joint::new(a, b, Transform::IDENTITY, Transform::IDENTITY, kind);
        match (phys.add_joint(joint), refusal) {
            (Ok(id), None) => {
                phys.remove_joint(id);
            }
            (Err(JointError::Invalid(why)), Some(rule)) => {
                assert!(
                    why.contains(rule),
                    "{kind:?}: refused for {why:?}, not {rule:?}"
                );
            }
            (got, _) => panic!("{kind:?}: {got:?}, expected {refusal:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Determinism and groups
// ---------------------------------------------------------------------------

/// Everything jointed at once — a bridge with its group's extra substeps, a
/// cradle's worth of balls and a motor — stepped `ticks`, hashed.
fn jointed_scene_hash(ticks: u32, motor_speed: f64) -> u64 {
    let mut phys = system();
    let (planks, _) = bridge(&mut phys, 11, 5.0, 4.0);
    phys.set_substeps(planks[0], 8);
    let deck = position(&phys, planks[5]) + DVec3::new(0.0, 0.3, 0.0);
    box_(
        &mut phys,
        300,
        deck,
        DQuat::IDENTITY,
        DVec3::splat(0.2),
        20.0,
    );
    let axle = anchor(&mut phys, 400, DVec3::new(10.0, 2.0, 0.0));
    let wheel = box_(
        &mut phys,
        401,
        DVec3::new(10.0, 2.0, 0.0),
        DQuat::IDENTITY,
        DVec3::new(0.5, 0.5, 0.1),
        4.0,
    );
    phys.add_joint(joint_at(
        &phys,
        axle,
        wheel,
        Transform::from_position(DVec3::new(10.0, 2.0, 0.0)),
        JointKind::Revolute(RevoluteJoint::hinge().with_motor(motor_speed, 5.0)),
    ))
    .expect("a valid motor");
    for _ in 0..ticks {
        phys.step(DT);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    phys.hash_state(&mut hasher);
    std::hash::Hasher::finish(&hasher)
}

/// **A jointed scene hashes the same on two runs**, and differently a tick
/// later, so the hash sees it move; and before any step, two scenes that
/// differ only in one joint's motor speed hash differently, so the hash
/// sees the joints.
#[test]
fn a_jointed_scene_hashes_the_same_on_two_runs() {
    let first = jointed_scene_hash(240, 3.0);
    assert_eq!(first, jointed_scene_hash(240, 3.0), "two runs disagreed");
    assert_ne!(
        first,
        jointed_scene_hash(241, 3.0),
        "the hash did not see a tick"
    );
    // Before a single step, the only difference is a joint's setting.
    assert_ne!(
        jointed_scene_hash(0, 3.0),
        jointed_scene_hash(0, 3.5),
        "the hash did not see a joint's motor speed"
    );
}

/// FNV-1a, 64-bit: a hasher fixed by its definition, so a digest of it can be
/// pinned across targets, where the standard library's may change.
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

/// **Soft zones and an elliptical swing step to the same bits everywhere.**
/// A bar on a hinge with a spring, soft zones and hard limits, let go from
/// level, and a block on a ball joint with an elliptical swing, a soft
/// swing, a twist and a soft twist, knocked sideways, stepped three seconds:
/// two runs hash alike, and to the pinned digest, which every target's CI
/// run checks.
#[test]
fn soft_zones_and_an_elliptical_swing_hash_the_same_everywhere() {
    /// The digest on x86-64 Linux, 2026-11-01.
    const PINNED: u64 = 0xf305_d87e_545a_16c9;

    let run = || {
        let mut phys = system();
        let post = anchor(&mut phys, 0, DVec3::new(0.0, 3.0, 0.0));
        let bar = box_(
            &mut phys,
            1,
            DVec3::new(0.5, 3.0, 0.0),
            DQuat::IDENTITY,
            DVec3::new(0.5, 0.05, 0.05),
            2.0,
        );
        let hinge = joint_at(
            &phys,
            post,
            bar,
            Transform::from_position(DVec3::new(0.0, 3.0, 0.0)),
            JointKind::Revolute(
                RevoluteJoint::hinge()
                    .with_spring(-0.3, zone(1.0, 0.5))
                    .with_limits(-1.2, 1.2)
                    .with_soft_limits(-0.7, 0.7, zone(8.0, 1.0)),
            ),
        );
        phys.add_joint(hinge).expect("a valid hinge");
        let ceiling = anchor(&mut phys, 2, DVec3::new(5.0, 3.0, 0.0));
        let block = box_(
            &mut phys,
            3,
            DVec3::new(5.0, 2.5, 0.0),
            DQuat::IDENTITY,
            DVec3::new(0.05, 0.5, 0.05),
            2.0,
        );
        let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
        let socket = joint_at(
            &phys,
            ceiling,
            block,
            Transform::new(DVec3::new(5.0, 3.0, 0.0), down),
            JointKind::Spherical(
                SphericalJoint::ball()
                    .with_swing(0.5, 1.0)
                    .with_soft_swing(0.3, 0.6, zone(8.0, 0.5))
                    .with_twist(-0.6, 0.6)
                    .with_soft_twist(-0.3, 0.3, zone(8.0, 0.5)),
            ),
        );
        phys.add_joint(socket).expect("a valid ball joint");
        phys.body_mut(block).expect("the block").velocity = DVec3::new(3.0, 0.0, 1.5);
        phys.body_mut(block).expect("the block").angular_velocity = DVec3::new(0.0, 4.0, 0.0);
        for _ in 0..180 {
            phys.step(DT);
        }
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        phys.hash_state(&mut hasher);
        std::hash::Hasher::finish(&hasher)
    };
    let first = run();
    assert_eq!(first, run(), "two runs hashed differently");
    assert_eq!(
        first, PINNED,
        "the scene hashes to {first:#018x}, not the pinned {PINNED:#018x}"
    );
}

/// **A group's extra substeps leave the rest of the system alone**: a ball
/// falling far from a column whose cubes ask for twelve substeps falls
/// exactly as it does with no column there at all.
#[test]
fn a_groups_substeps_leave_the_rest_alone() {
    let fall = |with_column: bool| {
        let mut phys = system();
        phys.add_plane(DVec3::Y, 0.0, CRATE);
        if with_column {
            for i in 0..5 {
                let cube = box_(
                    &mut phys,
                    10 + i,
                    DVec3::new(0.0, 0.5 + f64::from(i), 0.0),
                    DQuat::IDENTITY,
                    DVec3::splat(0.5),
                    10.0,
                );
                phys.set_substeps(cube, 12);
            }
        }
        let falling = ball(&mut phys, 1, DVec3::new(20.0, 10.0, 0.0), 0.1, 1.0, CRATE);
        for _ in 0..60 {
            phys.step(DT);
        }
        (position(&phys, falling), body(&phys, falling).velocity)
    };
    assert_eq!(fall(false), fall(true));
}

/// A column of `count` one-metre cubes whose cubes ask for `substeps`,
/// stepped ten seconds at the defaults: how far its top moved sideways.
fn column_lean(count: u32, substeps: u32) -> f64 {
    let mut phys = system();
    phys.add_plane(DVec3::Y, 0.0, CRATE);
    let cubes: Vec<Entity> = (0..count)
        .map(|i| {
            let cube = box_(
                &mut phys,
                i,
                DVec3::new(0.0, 0.5 + f64::from(i), 0.0),
                DQuat::IDENTITY,
                DVec3::splat(0.5),
                10.0,
            );
            phys.set_substeps(cube, substeps);
            cube
        })
        .collect();
    let top = *cubes.last().expect("a top");
    let start = position(&phys, top);
    for _ in 0..600 {
        phys.step(DT);
    }
    let moved = position(&phys, top) - start;
    DVec3::new(moved.x, 0.0, moved.z).length()
}

/// **A group with extra substeps stands a column the defaults cannot.**
/// Twenty one-metre cubes at the default settings fall (see `stacking.rs`:
/// past Greenhill's height of fifteen at 30 Hz); the same column asking for
/// twelve substeps — its contacts at 90 Hz, as a whole system's were at
/// eight substeps and 90 Hz before groups — stands, in a system whose
/// settings are the defaults.
///
/// Measured on 2026-09-23 over ten seconds, sleep off: at the defaults the
/// top cube was 3.32 m out; asking for twelve substeps it moved 0.22 mm and
/// sank 1.18 cm — the whole 90 Hz system's own measured sink, since the sink
/// is the stiffness's — and asking for eight (60 Hz, under Greenhill's 24
/// cubes) 2.3 mm.
#[test]
fn a_group_with_more_substeps_stands_a_tall_column() {
    let fallen = column_lean(20, 0);
    let standing = column_lean(20, 12);
    let eight = column_lean(20, 8);
    assert!(fallen > 1.0, "the default column stood: {fallen} m");
    assert!(standing < 1.0e-3, "the grouped column leaned {standing} m");
    assert!(
        eight < 5.0e-3,
        "the column at eight substeps leaned {eight} m"
    );
}

// ---------------------------------------------------------------------------
// A joint and a contact holding one body
// ---------------------------------------------------------------------------

/// **A bar hung from a ball joint with its free end on the floor reports the
/// spin it makes, and sleeps.** A 5 cm, 100 g bar — a fingertip's size —
/// leans 0.3 rad, its upper end jointed to a fixed anchor and a corner of its
/// lower end on the floor, so the joint and the floor share its weight and
/// their impulses cancel about its centre.
///
/// While the solver turned a body by its joints' change to its spin whole and
/// its contacts' at half, they cancelled in the velocity and not in the turn:
/// the bar turned by half the joint's warm-started impulse each substep, its
/// spring pulled it back, and it reported a spin it was not making. Measured
/// on 2026-11-01 at the default four substeps: from tick 15 to tick 60 it read
/// 0.089 rising to 0.1006 rad/s while turning at 0.029 down to 0.004 —
/// hovering at [`ContactSettings::sleep_angular_speed`], over it from tick 29
/// — and slept at tick 424. Since the part of the joints' change a contact
/// opposes is taken at the midpoint too, the two agree to 2e-4 rad/s from
/// tick 15, and it sleeps at tick 31.
#[test]
fn a_bar_held_by_a_joint_and_the_floor_reports_its_spin_and_sleeps() {
    const LENGTH: f64 = 0.05;
    let mut phys = system_with(ContactSettings::DEFAULT);
    phys.add_plane(DVec3::Y, 0.0, CRATE);
    let half = DVec3::new(0.5 * LENGTH, 0.05 * LENGTH, 0.05 * LENGTH);
    let tilt = about_z(-0.3);
    let corner = tilt * DVec3::new(half.x, -half.y, 0.0);
    let centre = DVec3::new(0.0, 0.001 - corner.y, 0.0);
    let bar = box_(&mut phys, 1, centre, tilt, half, 0.1);
    let pivot = centre + tilt * DVec3::new(-half.x, 0.0, 0.0);
    let hook = anchor(&mut phys, 0, pivot);
    let joint = joint_at(
        &phys,
        hook,
        bar,
        Transform::from_position(pivot),
        JointKind::Spherical(SphericalJoint::ball()),
    );
    phys.add_joint(joint).expect("a valid ball joint");

    let mut asleep_at = None;
    for tick in 1..=60 {
        let before = rotation(&phys, bar);
        phys.step(DT);
        if phys.is_sleeping(bar) {
            asleep_at = Some(tick);
            break;
        }
        // Past the landing, what it reports is what it turns.
        let reported = body(&phys, bar).angular_velocity.length();
        // Twice the turn's half-angle sine: its angle to third order, which
        // at the few milliradians a tick turns here is far inside the bound.
        let turn = rotation(&phys, bar) * before.inverse();
        let turning = 2.0 * DVec3::new(turn.x, turn.y, turn.z).length() / DT;
        assert!(
            tick < 15 || (reported - turning).abs() < 0.01,
            "tick {tick}: it reports {reported} rad/s while turning at {turning}"
        );
    }
    assert!(asleep_at.is_some(), "still awake after a second");
}
