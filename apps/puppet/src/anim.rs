//! The locomotion driver: the character's measured speed becomes a pose.
//!
//! ```text
//!   MoveOutcome::motion ──▶ Stage::speed  (smoothed, on the tick)
//!                                 │
//!                                 ▼
//!            BlendSpace1d::locate ──▶ blend weight ──▶ HUD
//!            BlendSpace1d::sample_into(speed, phase)
//!                                 │
//!                            Pose ─┴─▶ Palette::compute ──▶ skinning dispatch
//! ```
//!
//! `docs/plan/sample/09-puppet.md`'s milestone 2, first half: "client blend
//! tree (1D locomotion by speed)". The blending itself is
//! [`crcbl::anim::blend`]'s; what is here is the sample's half — which clips sit
//! where on the axis, how fast the stride runs, and the three readings the
//! browser gate holds the result to.
//!
//! # This runs on the client, and the speed it reads does not
//!
//! `docs/plan/17-animation.md` puts pose evaluation on the client — "pose math
//! is client-side presentation and free to vary" — so nothing in this module is
//! on the tick and nothing here crosses the wire. What crosses is the number it
//! is driven by: [`crate::game::Stats::speed`] is measured from the
//! controller's own [`MoveOutcome::motion`](crcbl::phys::MoveOutcome), on the
//! authoritative side, at the fixed timestep.
//!
//! **Measured, not commanded.** A demo could drive this off the input flag —
//! "a key is down, therefore walk" — and it would look right until the
//! character walked into a wall, where the flag says walk and the body is not
//! moving. Reading what the world actually allowed is what makes the pose track
//! the controller instead of the keyboard, and it is why the browser gate can
//! assert the two against each other.
//!
//! # The phase runs faster the faster the character goes
//!
//! One stride per [`rig::STRIDE_M`] of ground covered, floored at
//! [`REST_CYCLE_HZ`] so the clock never stops. Feet that cycled at a fixed rate
//! would skate whenever the speed was anything but the one the walk was
//! authored at, and the blend would be mixing two clips at unrelated points of
//! their cycles — which [`BlendSpace1d`] is documented as taking a *phase*
//! rather than a time to avoid.

use crcbl::anim::{Blend, BlendSpace1d, Palette, Pose, Skeleton};
use crcbl::math::{Mat4, Vec3};

use crate::rig;

/// The speed, in metres per second, at which [`rig::walk`] plays at full
/// weight.
///
/// **The speed the clip is authored for**, which is one stride of
/// [`rig::STRIDE_M`] over [`rig::WALK_CYCLE_S`] — not the speed the controller
/// is asked for. The two are different numbers on purpose, and both halves
/// matter:
///
/// * The clip has to play at its own cadence somewhere, or the legs skate at
///   every speed.
/// * The stop has to be a speed the reading actually **reaches**. The measured
///   speed is a first-order filter approaching what the world allowed, so it
///   arrives from below and never quite lands on it; a stop at
///   [`crate::game::WALK_SPEED`] would leave a walking character a hair short
///   of the top of the set for ever, and the blend weight would never sit
///   still. `the_walk_stop_is_a_speed_the_controller_passes` is what holds it
///   under the commanded walk.
///
/// A character faster than this plays the walk at full weight, which is what
/// [`BlendSpace1d`] does at either end of its axis.
pub const WALK_STOP_MPS: f32 = rig::STRIDE_M / rig::WALK_CYCLE_S;

/// How many strides a standing character's clock runs through per second.
///
/// The idle clip is a stance and holds one pose whatever the phase, so this
/// changes nothing on screen while the character stands. It matters at the
/// moment it starts moving: a phase frozen at whatever value it stopped on
/// would put the first step of every walk at a different point of the stride.
pub const REST_CYCLE_HZ: f32 = 0.6;

/// The points a joint's motion is measured at: its own origin and one metre out
/// along each of its axes.
///
/// **The three off-origin probes are what make the measure honest.** A rotation
/// moves no point at its own centre, so a deviation taken at joint origins
/// alone would report nothing at all for an arm swinging from a fixed shoulder
/// — and the arms are half of what the walk does. `apps/viewer`'s
/// `Player::deviation` probes the same four points for the same reason.
const PROBES: [Vec3; 4] = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];

/// The character's rig, its locomotion set, and the pose it is currently in.
///
/// Built once and advanced in place: sampling, blending and composing the
/// palette all write into buffers this owns, so a frame allocates nothing.
#[derive(Debug)]
pub struct Animator {
    skeleton: Skeleton,
    space: BlendSpace1d,
    pose: Pose,
    palette: Palette,
    /// Where each joint's probes sit in the rest pose, flattened joint-major.
    /// What [`deviation`](Self::deviation) is measured against.
    rest_probes: Vec<Vec3>,
    phase: f32,
    blend: f32,
    partial: u64,
    deviation: f32,
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}

impl Animator {
    /// The rig, posed in its idle stance and ready to be advanced.
    ///
    /// # Panics
    ///
    /// Never: the stops below are two finite positions in ascending order,
    /// which is all [`BlendSpace1d::new`] refuses a set for not being.
    #[must_use]
    pub fn new() -> Self {
        let skeleton = rig::skeleton();
        let space = BlendSpace1d::new(
            vec![(0.0, rig::idle()), (WALK_STOP_MPS, rig::walk())],
            &skeleton,
        )
        .expect("idle at zero and walk at the walk speed ascend");
        let pose = Pose::new(&skeleton);
        let mut palette = Palette::new(&skeleton);
        palette.compute(&skeleton, &pose);
        let rest_probes = probes(&palette);
        let mut animator = Self {
            skeleton,
            space,
            pose,
            palette,
            rest_probes,
            phase: 0.0,
            blend: 0.0,
            partial: 0,
            deviation: 0.0,
        };
        animator.advance(0.0, 0.0);
        animator
    }

    /// Advances the phase by `dt` seconds and reposes the character for
    /// `speed`, in metres per second.
    pub fn advance(&mut self, dt: f32, speed: f32) {
        let Blend {
            lower,
            upper,
            weight,
        } = self.space.locate(speed);
        // Where the character sits across the *whole* set rather than within
        // one of its segments, which is the number the overlay and the browser
        // gate both read: 0 at the idle end, 1 at the fastest stop.
        let segments = (self.space.stops() - 1).max(1) as f32;
        self.blend = (lower as f32 + weight) / segments;
        if lower != upper && weight > 0.0 && weight < 1.0 {
            self.partial += 1;
        }

        // One stride per `rig::STRIDE_M` of ground, and never slower than the
        // resting clock. `max` and not a sum, so that at `WALK_STOP_MPS` the
        // clip runs at exactly the cadence it was authored at.
        let cycles = (speed / rig::STRIDE_M).max(REST_CYCLE_HZ);
        self.phase = (self.phase + cycles * dt).rem_euclid(1.0);
        self.space
            .sample_into(speed, self.phase, &self.skeleton, &mut self.pose);
        self.palette.compute(&self.skeleton, &self.pose);

        self.deviation = probes(&self.palette)
            .into_iter()
            .zip(&self.rest_probes)
            .fold(0.0_f32, |worst, (now, &rest)| {
                worst.max((now - rest).length())
            });
    }

    /// The skinning matrices this frame, in palette order — what a
    /// [`SkinRange`](crcbl::render::SkinRange) is handed.
    #[inline]
    #[must_use]
    pub fn palette(&self) -> &[Mat4] {
        self.palette.matrices()
    }

    /// Where the character sits across the locomotion set: 0 standing still, 1
    /// at [`WALK_STOP_MPS`].
    #[inline]
    #[must_use]
    pub const fn blend(&self) -> f32 {
        self.blend
    }

    /// How many advances have found the blend **strictly between** two stops.
    ///
    /// A counter rather than a reading because the thing it is evidence for is
    /// a transition: the heartbeat the browser gate reads is a second apart, and
    /// a weight that swept 0 to 1 in between would show up on it as a snap. This
    /// rises once per frame for as long as the crossing takes, so the gate can
    /// ask whether the crossing *happened* rather than hoping to sample it.
    #[inline]
    #[must_use]
    pub const fn partial(&self) -> u64 {
        self.partial
    }

    /// How far the character's pose has carried a joint from its rest pose, in
    /// metres — the largest distance any probe has moved.
    ///
    /// This is the number that says the rig is being posed at all. It holds
    /// still while the character stands, because [`rig::idle`] is a stance, and
    /// sweeps while it walks.
    #[inline]
    #[must_use]
    pub const fn deviation(&self) -> f32 {
        self.deviation
    }
}

/// Every joint's [`PROBES`], in the pose this palette holds, joint-major.
///
/// [`Palette::globals`] and not [`Palette::matrices`]: the skinning matrices
/// have the inverse binds folded in and are the identity in the bind pose, so a
/// measure taken from them would be measuring the deformation of a mesh rather
/// than the motion of a bone.
fn probes(palette: &Palette) -> Vec<Vec3> {
    palette
        .globals()
        .iter()
        .flat_map(|global| PROBES.map(|probe| global.transform_point3(probe)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Animator, REST_CYCLE_HZ, WALK_STOP_MPS};

    /// A tick of the frame clock, for the tests below. Not the simulation's:
    /// this runs on the frame.
    const DT: f32 = 1.0 / 60.0;

    /// Standing still is the idle end of the set, and the pose does not move.
    /// The browser gate's settle check is this claim, in a browser.
    #[test]
    fn standing_still_holds_one_pose() {
        let mut animator = Animator::new();
        animator.advance(DT, 0.0);
        let held = animator.deviation();
        let palette = animator.palette().to_vec();
        for _ in 0..120 {
            animator.advance(DT, 0.0);
        }
        assert_eq!(animator.blend(), 0.0);
        assert_eq!(animator.deviation(), held);
        assert_eq!(animator.palette(), palette.as_slice());
    }

    /// And the stance it holds is a posed one, not the rest pose — otherwise
    /// the check above would pass over a character nothing had posed at all.
    #[test]
    fn the_stance_it_holds_is_a_posed_one() {
        let mut animator = Animator::new();
        animator.advance(DT, 0.0);
        assert!(
            animator.deviation() > 0.01,
            "the idle stance moved the rig by only {} m",
            animator.deviation()
        );
    }

    /// Walking moves it, and keeps moving it.
    #[test]
    fn walking_carries_the_pose() {
        let mut animator = Animator::new();
        animator.advance(DT, WALK_STOP_MPS);
        let mut seen = Vec::new();
        for _ in 0..60 {
            animator.advance(DT, WALK_STOP_MPS);
            seen.push(animator.deviation());
        }
        let distinct = {
            let mut sorted = seen.clone();
            sorted.sort_by(f32::total_cmp);
            sorted.dedup();
            sorted.len()
        };
        assert_eq!(animator.blend(), 1.0);
        assert!(
            distinct > 20,
            "a walking character should take a new pose nearly every frame; it took {distinct} \
             across {} frames",
            seen.len()
        );
    }

    /// **The property the demo exists to show**: the blend follows the speed
    /// continuously, so an accelerating character passes through the middle of
    /// the set rather than snapping across it.
    #[test]
    fn the_blend_follows_the_speed_through_the_middle() {
        let mut animator = Animator::new();
        let mut weights = Vec::new();
        for step in 0..=40 {
            let speed = WALK_STOP_MPS * step as f32 / 40.0;
            animator.advance(DT, speed);
            weights.push(animator.blend());
        }
        assert_eq!(weights.first().copied(), Some(0.0));
        assert_eq!(weights.last().copied(), Some(1.0));
        for pair in weights.windows(2) {
            assert!(
                pair[1] >= pair[0],
                "the blend went backwards as the speed rose: {pair:?}"
            );
        }
        let inside = weights.iter().filter(|&&w| w > 0.0 && w < 1.0).count();
        assert!(
            inside > 30,
            "the blend should sit between the stops for most of a sweep; it did for {inside} \
             of {} steps",
            weights.len()
        );
        assert!(
            animator.partial() >= inside as u64,
            "every advance that found the blend inside the set should have been counted"
        );
    }

    /// A speed past the top of the set saturates rather than running past it.
    #[test]
    fn a_speed_past_the_set_holds_its_top() {
        let mut animator = Animator::new();
        animator.advance(DT, 100.0 * WALK_STOP_MPS);
        assert_eq!(animator.blend(), 1.0);
    }

    /// **The stop is a speed the controller passes**, so a character actually
    /// walking sits at the top of the set rather than a hair short of it for
    /// ever — see [`WALK_STOP_MPS`].
    #[test]
    fn the_walk_stop_is_a_speed_the_controller_passes() {
        assert!(
            f64::from(WALK_STOP_MPS) < crate::game::WALK_SPEED,
            "the walk stop is {WALK_STOP_MPS} m/s and the controller only ever reaches {}",
            crate::game::WALK_SPEED,
        );
    }

    /// And a character held at the commanded walk speed leaves the blend
    /// **exactly** at the top, so the counter that says a crossing happened does
    /// not tick for ever while nothing is crossing.
    #[test]
    fn a_steady_walk_leaves_the_crossing_counter_alone() {
        let mut animator = Animator::new();
        #[allow(clippy::cast_possible_truncation)]
        let commanded = crate::game::WALK_SPEED as f32;
        for _ in 0..120 {
            animator.advance(DT, commanded);
        }
        let settled = animator.partial();
        for _ in 0..120 {
            animator.advance(DT, commanded);
        }
        assert_eq!(animator.blend(), 1.0);
        assert_eq!(
            animator.partial(),
            settled,
            "the blend counted {} crossing frame(s) while the character walked at a constant \
             speed",
            animator.partial() - settled,
        );
    }

    /// The clock runs while the character stands, so the first step of a walk
    /// does not start from wherever the last one stopped.
    #[test]
    fn the_phase_runs_while_the_character_stands() {
        let mut animator = Animator::new();
        let start = animator.phase;
        animator.advance(1.0, 0.0);
        assert!(
            (animator.phase - (start + REST_CYCLE_HZ).rem_euclid(1.0)).abs() < 1e-5,
            "a second of standing should carry the phase by {REST_CYCLE_HZ}, and it reached {}",
            animator.phase
        );
    }
}
