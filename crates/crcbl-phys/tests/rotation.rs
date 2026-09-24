//! Rotation: torque, the inertia tensor, the gyroscopic term and the quaternion,
//! over runs long enough for their errors to show.
//!
//! Contact-solver rung 0 (`docs/notes/simulation.md`). The claims are the ones
//! a tumbling body makes visible: angular momentum and energy are conserved in
//! zero g and energy never grows, a T-handle spun about its intermediate axis
//! flips while one spun about its major axis does not, the quaternion stays a
//! unit, and two runs agree to the bit.

use std::hash::Hasher;

use crcbl_ecs::SystemTrait as _;
use crcbl_phys::{
    MAX_ROTATION_LENGTH_ERROR, MassProperties, PhysicsSystem, RigidBody, Transform,
    rotation_from_scaled_axis,
};
use glam::{DQuat, DVec3};

/// Build an entity from a raw index; see `dynamics.rs`'s twin.
fn test_entity(index: u32) -> crcbl_ecs::Entity {
    crcbl_ecs::Entity::from_bits((1u64 << 32) | index as u64).expect("generation 1 is never zero")
}

/// The substep the T-handle runs take: 240 Hz.
const DT: f64 = 1.0 / 240.0;

/// A T-handle: a bar across the top of a stem, both solid boxes of one density,
/// in the XY plane with the bar along X.
///
/// Returns the body, with its inertia about the combined centre of mass, and
/// its principal axes in ascending order of moment. The T is symmetric under
/// `x → −x` and `z → −z`, so its products of inertia vanish and the body axes
/// are its principal axes — asserted here rather than trusted.
fn t_handle() -> (RigidBody, [DVec3; 3]) {
    const DENSITY: f64 = 1000.0;
    let bar = DVec3::new(0.4, 0.05, 0.05);
    let stem = DVec3::new(0.05, 0.3, 0.05);
    let volume = |half: DVec3| 8.0 * half.x * half.y * half.z;
    let props = MassProperties::combine(&[
        MassProperties::cuboid(DENSITY * volume(bar), bar, DVec3::new(0.0, 0.35, 0.0)),
        MassProperties::cuboid(DENSITY * volume(stem), stem, DVec3::ZERO),
    ]);
    let inertia = props.inertia;
    assert_eq!(
        [inertia.x_axis.y, inertia.x_axis.z, inertia.y_axis.z],
        [0.0; 3],
        "the T's body axes are not its principal axes"
    );
    let mut axes = [
        (inertia.x_axis.x, DVec3::X),
        (inertia.y_axis.y, DVec3::Y),
        (inertia.z_axis.z, DVec3::Z),
    ];
    axes.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        axes[0].0 < axes[1].0 && axes[1].0 < axes[2].0,
        "the T's principal moments are not distinct: {axes:?}"
    );
    (
        RigidBody::new_dynamic(props.mass).with_inertia(inertia),
        [axes[0].1, axes[1].1, axes[2].1],
    )
}

/// A system holding one `body`, at the origin and unrotated.
fn alone(body: RigidBody) -> PhysicsSystem {
    let mut phys = PhysicsSystem::new();
    phys.set_body(test_entity(0), body);
    phys.set_transform(test_entity(0), Transform::IDENTITY);
    phys
}

fn state(phys: &PhysicsSystem) -> (RigidBody, Transform) {
    let e = test_entity(0);
    (
        *phys.body(e).expect("a body"),
        *phys.transform(e).expect("a transform"),
    )
}

/// The worst relative momentum drift, the worst relative energy above the
/// start, the worst relative energy below it, and the worst single-step
/// relative rise, over `steps` steps of `dt`.
fn run_free(phys: &mut PhysicsSystem, steps: usize, dt: f64) -> (f64, f64, f64, f64) {
    let (body, transform) = state(phys);
    let momentum = body.angular_momentum(transform.rotation);
    let start = body.kinetic_energy(transform.rotation);
    assert!(
        start > 0.0 && momentum.length() > 0.0,
        "nothing is spinning"
    );
    let (mut drift, mut above, mut below, mut rise) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut previous = start;
    for _ in 0..steps {
        phys.step(dt);
        let (body, transform) = state(phys);
        let energy = body.kinetic_energy(transform.rotation);
        above = above.max((energy - start) / start);
        below = below.max((start - energy) / start);
        rise = rise.max((energy - previous) / start);
        previous = energy;
        let now = body.angular_momentum(transform.rotation);
        drift = drift.max((now - momentum).length() / momentum.length());
    }
    (drift, above, below, rise)
}

/// **Ten minutes of a tumbling T-handle keep its angular momentum and its
/// energy to ten digits.**
///
/// The bounds are measured, with headroom: on 2026-09-17 the run drifted
/// `3.7·10^-11` in momentum and `7.5·10^-11` in energy, the energy creeping up
/// by rounding at a few `10^-15` a step. What fails them is a scheme that does
/// not conserve: Box3D's implicit Euler lost half this body's energy in two
/// minutes, and reading the angular velocity back through the wrong
/// orientation lost a fifth.
#[test]
fn a_zero_g_tumble_conserves_momentum_and_energy() {
    /// Relative drift in angular momentum over the run.
    const MOMENTUM_BOUND: f64 = 1e-10;
    /// Relative departure of the energy from its start, either way.
    const ENERGY_BOUND: f64 = 2e-10;
    /// Relative rise of the energy in any one step: rounding, and no more.
    const STEP_RISE_BOUND: f64 = 1e-14;

    let (mut body, axes) = t_handle();
    body.angular_velocity = axes[1] * 8.0 + axes[0] * 0.08;
    let mut phys = alone(body);
    let (drift, above, below, rise) = run_free(&mut phys, (600.0 / DT) as usize, DT);
    assert!(
        drift <= MOMENTUM_BOUND,
        "angular momentum drifted {drift:e}"
    );
    assert!(
        above <= ENERGY_BOUND,
        "energy rose {above:e} above its start"
    );
    assert!(
        below <= ENERGY_BOUND,
        "energy fell {below:e} below its start"
    );
    assert!(rise <= STEP_RISE_BOUND, "energy rose {rise:e} in one step");
}

/// **A thin bar spun off-axis at two radians a step still conserves, and at
/// five it still never gains energy.**
///
/// Past what the contact solver's quarter-turn cap will allow, which is the
/// point: this is where the Newton iterations stop converging, and where the
/// energy scaling in `gyroscopic_step` is what stands between the body and an
/// explosion — with two iterations and no scaling, the 120 rad/s run gained
/// eleven orders of magnitude of energy.
#[test]
fn a_fast_thin_bar_never_gains_energy() {
    const HZ: f64 = 60.0;
    for (rate, momentum_bound) in [(120.0, 1e-10), (300.0, 1e-5)] {
        let props = MassProperties::cuboid(1.0, DVec3::new(1.0, 0.02, 0.05), DVec3::ZERO);
        let mut body = RigidBody::new_dynamic(1.0).with_inertia(props.inertia);
        body.angular_velocity = DVec3::new(0.3, 1.0, 0.2).normalize() * rate;
        let mut phys = alone(body);
        let (drift, above, _, _) = run_free(&mut phys, (600.0 * HZ) as usize, 1.0 / HZ);
        assert!(
            above <= 1e-10,
            "at {rate} rad/s the energy rose {above:e} above its start"
        );
        assert!(
            drift <= momentum_bound,
            "at {rate} rad/s angular momentum drifted {drift:e}"
        );
    }
}

/// How many times the body axis `axis` reverses against the angular momentum
/// over `seconds`: crossings from above `0.9` to below `-0.9` and back.
fn reversals(body: RigidBody, axis: DVec3, seconds: f64) -> usize {
    let mut phys = alone(body);
    let (start, transform) = state(&phys);
    let momentum = start.angular_momentum(transform.rotation).normalize();
    let mut aligned = true;
    let mut count = 0;
    for _ in 0..(seconds / DT) as usize {
        phys.step(DT);
        let (_, transform) = state(&phys);
        let along = (transform.rotation * axis).dot(momentum);
        if aligned && along < -0.9 {
            aligned = false;
            count += 1;
        } else if !aligned && along > 0.9 {
            aligned = true;
            count += 1;
        }
    }
    count
}

/// **The Dzhanibekov effect: spun about its intermediate axis a T-handle flips
/// over and back, and spun about its major axis it does not.**
///
/// The contrast is what makes the flip a test of the gyroscopic term rather
/// than of anything that merely turns a body: with the term removed, neither
/// spin changes axis at all. The nudge is the same for both.
#[test]
fn a_t_handle_flips_about_its_intermediate_axis_and_only_that_one() {
    let (body, axes) = t_handle();
    let spun = |axis: DVec3| RigidBody {
        angular_velocity: axis * 8.0 + axes[0] * 0.08,
        ..body
    };

    let flips = reversals(spun(axes[1]), axes[1], 20.0);
    assert!(
        flips >= 4,
        "about its intermediate axis the handle reversed {flips} times in 20 s"
    );
    let steady = reversals(spun(axes[2]), axes[2], 20.0);
    assert_eq!(
        steady, 0,
        "about its major axis the handle reversed {steady} times"
    );
}

#[test]
fn a_spinning_quaternion_stays_unit_length() {
    const STEPS: usize = 1_000_000;
    // One body through the exponential map — no inertia — and one through the
    // Cayley rotation the gyroscopic step turns with.
    let (tumbling, axes) = t_handle();
    for body in [
        RigidBody {
            angular_velocity: DVec3::new(3.1, -7.4, 11.9),
            ..RigidBody::new_kinematic()
        },
        RigidBody {
            angular_velocity: axes[1] * 8.0 + axes[0] * 0.08,
            ..tumbling
        },
    ] {
        let mut phys = alone(body);
        let mut worst = 0.0f64;
        for _ in 0..STEPS {
            phys.step(DT);
            worst = worst.max((state(&phys).1.rotation.length() - 1.0).abs());
        }
        assert!(
            worst <= MAX_ROTATION_LENGTH_ERROR,
            "a quaternion drifted {worst:e} from unit length, past {MAX_ROTATION_LENGTH_ERROR:e}"
        );
    }
}

/// **A body with no inertia turns at exactly the rate it was given.**
///
/// Sixty steps of `π` rad/s about `+Z` at 60 Hz: the exponential map lands on
/// the single rotation by the whole half turn, to rounding. The first-order
/// update would fall short by a relative `θ²/12` a step.
#[test]
fn a_body_with_no_inertia_turns_by_exactly_its_rate_times_the_time() {
    let rate = core::f64::consts::PI;
    let body = RigidBody {
        angular_velocity: DVec3::Z * rate,
        ..RigidBody::new_kinematic()
    };
    let mut phys = alone(body);
    for _ in 0..60 {
        phys.step(1.0 / 60.0);
    }
    let got = state(&phys).1.rotation;
    let want = rotation_from_scaled_axis(DVec3::Z * rate);
    assert!(
        got.dot(want) > 1.0 - 1e-14,
        "turned to {got:?}, not the half turn {want:?}"
    );
}

/// **A torque about a principal axis accelerates the spin by `τ / I`, and a
/// body with no inertia ignores it.**
#[test]
fn a_torque_accelerates_by_its_inertia_and_nothing_else() {
    const TORQUE: f64 = 3.0;
    let props = MassProperties::cuboid(12.0, DVec3::new(0.5, 1.0, 1.5), DVec3::ZERO);
    let moment = props.inertia.y_axis.y;
    let mut phys = alone(RigidBody::new_dynamic(12.0).with_inertia(props.inertia));
    let free = test_entity(1);
    phys.set_body(free, RigidBody::new_dynamic(12.0));
    phys.set_transform(free, Transform::IDENTITY);

    for _ in 0..240 {
        assert!(phys.apply_torque(test_entity(0), DVec3::Y * TORQUE));
        assert!(phys.apply_torque(free, DVec3::Y * TORQUE));
        phys.step(DT);
    }
    let spin = state(&phys).0.angular_velocity;
    let want = TORQUE / moment * 240.0 * DT;
    assert!(
        (spin.y - want).abs() < 1e-12 && spin.x.abs() < 1e-15 && spin.z.abs() < 1e-15,
        "spun to {spin:?}, want {want} about +Y"
    );
    assert_eq!(
        phys.body(free).expect("a body").angular_velocity,
        DVec3::ZERO,
        "a body with no inertia turned under torque"
    );
    assert!(!phys.apply_torque(test_entity(9), DVec3::Y), "no such body");
}

/// **An orientation set by hand survives a step bit for bit** when nothing
/// spins it — the promise every caller that turns a body itself relies on.
#[test]
fn an_orientation_nothing_spins_survives_a_step_bit_for_bit() {
    let rotation = rotation_from_scaled_axis(DVec3::new(0.3, -1.1, 0.7));
    let (body, _) = t_handle();
    for body in [
        body,
        RigidBody::new_dynamic(1.0),
        RigidBody::new_kinematic(),
    ] {
        let mut phys = PhysicsSystem::new();
        phys.set_body(test_entity(0), body);
        phys.set_transform(test_entity(0), Transform::new(DVec3::ZERO, rotation));
        phys.step(DT);
        let after = state(&phys).1.rotation;
        assert_eq!(
            after.to_array().map(f64::to_bits),
            rotation.to_array().map(f64::to_bits),
            "a body with no spin was turned"
        );
    }
}

/// `rotation_from_scaled_axis` is the rotation `glam` builds from the
/// platform's sine and cosine, to rounding.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the platform's rotation is the oracle here, not simulation"
)]
fn the_constructed_rotation_is_glams_to_rounding() {
    for v in [
        DVec3::new(0.3, -1.1, 0.7),
        DVec3::X * core::f64::consts::FRAC_PI_2,
        DVec3::new(-40.0, 3.0, 12.5),
        DVec3::new(1e-9, 0.0, 0.0),
    ] {
        let ours = rotation_from_scaled_axis(v);
        let glams = DQuat::from_scaled_axis(v);
        let error = (ours - glams).length();
        assert!(error < 1e-15, "{v:?}: {ours:?} against {glams:?}");
    }
}

/// FNV-1a, so a digest does not depend on `std`'s hasher, which promises
/// nothing across Rust releases.
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

/// **A tumbling T-handle hashes the same on two runs, and to the pinned
/// digest.**
///
/// The pinned value was taken on x86-64 Linux; CI runs this test on aarch64
/// macOS and x86-64 Windows as well, so a target whose arithmetic differs in
/// one bit of one body fails here.
#[test]
fn a_tumbling_t_handle_hashes_the_same_everywhere() {
    /// The digest on x86-64 Linux, 2026-09-17.
    const PINNED: u64 = 0x10f3_3b16_2db2_0b04;

    let run = || {
        let (body, axes) = t_handle();
        let mut phys = alone(RigidBody {
            angular_velocity: axes[1] * 8.0 + axes[0] * 0.08,
            ..body
        });
        for _ in 0..(10.0 / DT) as usize {
            phys.step(DT);
        }
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        phys.hash_state(&mut hasher);
        hasher.finish()
    };
    let first = run();
    assert_eq!(first, run(), "two runs of one tumble hashed differently");
    assert_eq!(
        first, PINNED,
        "the tumble hashes to {first:#018x}, not the pinned {PINNED:#018x}"
    );
}
