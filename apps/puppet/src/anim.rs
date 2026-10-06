//! The character's animation: a state machine the server steps, and the pose
//! the client draws from the state it was sent.
//!
//! ```text
//!   server, on the tick                           client, on the frame
//!   ───────────────────                           ────────────────────
//!   MoveOutcome ─▶ speed, grounded, jumped
//!                        │
//!   Locomotion::tick ─▶ Machine::step ─▶ MachineState ──copy──▶ Animator::advance
//!                        │                                          │
//!                        └─▶ footstep events, counted     Sampler ─▶ Palette ─▶ skinning
//! ```
//!
//! `docs/plan/sample/09-puppet.md`'s milestone 2: idle, run and jump through
//! [`crcbl::anim::machine`], with the footsteps its run state's event track
//! carries. The asset is `assets/anim/character.ron`, compiled in so a browser
//! build and a test read the same file; the clips it names are [`rig`]'s.
//!
//! # The machine runs on the server, the pose on the client
//!
//! The split `docs/notes/simulation.md` gives animation (_What the deleted
//! 17-animation plan left behind_): state-machine ticks, transition decisions,
//! normalised clip time and events are the server's, and sampling a pose is the
//! client's. [`Locomotion`] is the server's half — it lives on
//! [`crate::game`]'s stage, is stepped once per tick at the fixed timestep, and
//! counts footsteps as the machine reports them, so a footstep lands on the
//! same tick whatever the frame rate. [`Animator`] is the client's half: it is
//! handed the [`MachineState`] the tick left and poses the rig from it, never
//! stepping the machine itself, so the two cannot disagree about where in a
//! clip the character is.
//!
//! **Driven by measured speed, not commanded.** `speed` is
//! [`crate::game::Stats::speed`], measured from the controller's own
//! [`MoveOutcome::motion`](crcbl::phys::MoveOutcome), so a character pushing
//! against a riser it cannot climb stands in idle — which is what it is doing.
//!
//! # Root motion is not used here
//!
//! [`rig`]'s clips are authored in place: nothing drives the root joint, so
//! [`Machine::root_velocity`](crcbl::anim::Machine::root_velocity) would answer
//! zero on every tick, and the controller is moved by the input as it always
//! was. Root motion is proven in `crcbl-anim`'s own tests, not here —
//! `docs/backlog.md` records it.

use crcbl::anim::{
    BoolParam, EventId, FloatParam, Machine, MachineState, Palette, Pose, Sampler, Skeleton,
    StateId, StateMachine, TriggerParam,
};
use crcbl::math::{Mat4, Vec3};

use crate::rig;

/// The character's state machine, as committed.
pub const MACHINE_RON: &str = include_str!("../assets/anim/character.ron");

/// The speed, in metres per second, at which [`rig::walk`] plays at full
/// weight in the run state's blend.
///
/// **The speed the clip is authored for**, one stride of [`rig::STRIDE_M`] over
/// [`rig::WALK_CYCLE_S`], and the asset's first blend stop —
/// `the_run_blend_stops_are_the_rigs_authored_speeds` holds the two together.
pub const WALK_STOP_MPS: f32 = rig::STRIDE_M / rig::WALK_CYCLE_S;

/// The speed at which [`rig::run`] plays at full weight: one stride of
/// [`rig::RUN_STRIDE_M`] over [`rig::RUN_CYCLE_S`], and the asset's second
/// blend stop.
pub const RUN_STOP_MPS: f32 = rig::RUN_STRIDE_M / rig::RUN_CYCLE_S;

/// Every state the asset declares, in its order. Named here so the debug line
/// can carry a state as a `&'static str`, and checked against the asset by
/// `the_committed_asset_binds_to_the_rig`.
pub const STATES: [&str; 3] = ["idle", "run", "jump"];

/// The event the run state's track raises when a foot comes down.
pub const FOOTSTEP: &str = "footstep";

/// The asset, parsed and bound to [`rig`]'s clips.
///
/// # Panics
///
/// Never for the committed asset: it is compiled in, and
/// `the_committed_asset_binds_to_the_rig` parses and binds this exact text.
#[must_use]
pub fn machine() -> Machine {
    let asset = StateMachine::from_ron(MACHINE_RON)
        .unwrap_or_else(|error| panic!("assets/anim/character.ron: {error}"));
    Machine::new(asset, rig::clip)
        .unwrap_or_else(|error| panic!("assets/anim/character.ron against the rig: {error}"))
}

// ---------------------------------------------------------------------------
// The server's half
// ---------------------------------------------------------------------------

/// The character's animation state on the server: the machine, the state it is
/// in, and the handles the stage sets it through.
#[derive(Debug)]
pub struct Locomotion {
    machine: Machine,
    state: MachineState,
    speed: FloatParam,
    grounded: BoolParam,
    jump: TriggerParam,
    footstep: EventId,
    /// What the last step reported, reused so a tick allocates nothing.
    events: Vec<EventId>,
}

impl Default for Locomotion {
    fn default() -> Self {
        Self::new()
    }
}

impl Locomotion {
    /// The machine at its initial state.
    ///
    /// # Panics
    ///
    /// Never for the committed asset — see [`machine`] — which declares every
    /// parameter and event resolved here; `the_committed_asset_binds_to_the_rig`
    /// resolves them too.
    #[must_use]
    pub fn new() -> Self {
        let machine = machine();
        let asset = machine.asset();
        let speed = asset
            .float_parameter("speed")
            .unwrap_or_else(|error| panic!("assets/anim/character.ron: {error}"));
        let grounded = asset
            .bool_parameter("grounded")
            .unwrap_or_else(|error| panic!("assets/anim/character.ron: {error}"));
        let jump = asset
            .trigger_parameter("jump")
            .unwrap_or_else(|error| panic!("assets/anim/character.ron: {error}"));
        let footstep = asset
            .event(FOOTSTEP)
            .unwrap_or_else(|| panic!("assets/anim/character.ron raises no {FOOTSTEP:?}"));
        let state = machine.start();
        Self {
            machine,
            state,
            speed,
            grounded,
            jump,
            footstep,
            events: Vec::new(),
        }
    }

    /// One tick: what the controller did becomes the machine's parameters, and
    /// the machine steps by `dt` seconds. Answers how many footsteps the step
    /// crossed.
    ///
    /// `jumped` is whether the character left the ground on *this* tick; it
    /// sets the trigger, which the machine holds until a transition takes it.
    pub fn tick(&mut self, dt: f32, speed: f32, grounded: bool, jumped: bool) -> u64 {
        self.state.set_float(self.speed, speed);
        self.state.set_bool(self.grounded, grounded);
        if jumped {
            self.state.trigger(self.jump);
        }
        self.machine.step(&mut self.state, dt, &mut self.events);
        self.events
            .iter()
            .filter(|&&event| event == self.footstep)
            .map(|_| 1)
            .sum()
    }

    /// The state as it stands — what the client is sent.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> MachineState {
        self.state
    }

    /// The current state's name, as one of [`STATES`].
    ///
    /// # Panics
    ///
    /// If the asset declares a state [`STATES`] does not list, which
    /// `the_committed_asset_binds_to_the_rig` rules out for the committed one.
    #[must_use]
    pub fn label(&self) -> &'static str {
        let name = self.machine.asset().state_name(self.state.state());
        STATES
            .into_iter()
            .find(|&known| known == name)
            .unwrap_or_else(|| panic!("state {name:?} is not one of {STATES:?}"))
    }
}

// ---------------------------------------------------------------------------
// The client's half
// ---------------------------------------------------------------------------

/// The points a joint's motion is measured at: its own origin and one metre out
/// along each of its axes.
///
/// **The three off-origin probes are what make the measure honest.** A rotation
/// moves no point at its own centre, so a deviation taken at joint origins
/// alone would report nothing at all for an arm swinging from a fixed shoulder
/// — and the arms are half of what the walk does. `apps/viewer`'s
/// `Player::deviation` probes the same four points for the same reason.
const PROBES: [Vec3; 4] = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];

/// The character's rig, posed from the [`MachineState`] the server sent.
///
/// Built once and advanced in place: sampling, crossfading and composing the
/// palette all write into buffers this owns, so a frame allocates nothing.
#[derive(Debug)]
pub struct Animator {
    machine: Machine,
    skeleton: Skeleton,
    sampler: Sampler,
    pose: Pose,
    palette: Palette,
    idle: StateId,
    /// Where each joint's probes sit in the rest pose, flattened joint-major.
    /// What [`deviation`](Self::deviation) is measured against.
    rest_probes: Vec<Vec3>,
    state: MachineState,
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
    /// The rig, posed at the machine's initial state.
    ///
    /// # Panics
    ///
    /// Never for the committed asset, which has an `idle` state — see
    /// [`machine`].
    #[must_use]
    pub fn new() -> Self {
        let machine = machine();
        let idle = machine
            .asset()
            .state(STATES[0])
            .unwrap_or_else(|| panic!("assets/anim/character.ron has no {:?}", STATES[0]));
        let skeleton = rig::skeleton();
        // No root-motion joint: the rig's clips are in place — see the module
        // docs.
        let sampler = Sampler::new(&skeleton, None);
        let pose = Pose::new(&skeleton);
        let mut palette = Palette::new(&skeleton);
        palette.compute(&skeleton, &pose);
        let rest_probes = probes(&palette);
        let state = machine.start();
        let mut animator = Self {
            machine,
            skeleton,
            sampler,
            pose,
            palette,
            idle,
            rest_probes,
            state,
            blend: 0.0,
            partial: 0,
            deviation: 0.0,
        };
        animator.advance(&state);
        animator
    }

    /// Poses the rig as `state` describes.
    pub fn advance(&mut self, state: &MachineState) {
        self.state = *state;
        self.blend = 1.0 - self.idle_weight();
        if self.blend > 0.0 && self.blend < 1.0 {
            self.partial += 1;
        }
        self.sampler
            .sample_into(&self.machine, state, &self.skeleton, &mut self.pose);
        self.palette.compute(&self.skeleton, &self.pose);

        self.deviation = probes(&self.palette)
            .into_iter()
            .zip(&self.rest_probes)
            .fold(0.0_f32, |worst, (now, &rest)| {
                worst.max((now - rest).length())
            });
    }

    /// How much of the pose is the idle stance: 1 standing in idle, 0 in any
    /// other state, and the crossfade weight in between while one fades into
    /// the other.
    fn idle_weight(&self) -> f32 {
        let Some(fade) = self.state.fade() else {
            return f32::from(u8::from(self.state.state() == self.idle));
        };
        let incoming = fade.weight();
        let mut weight = 0.0;
        if self.state.state() == self.idle {
            weight += incoming;
        }
        if fade.from() == self.idle {
            weight += 1.0 - incoming;
        }
        weight
    }

    /// The skinning matrices this frame, in palette order — what a
    /// [`SkinRange`](crcbl::render::SkinRange) is handed.
    #[inline]
    #[must_use]
    pub fn palette(&self) -> &[Mat4] {
        self.palette.matrices()
    }

    /// How far the character is out of its idle stance: 0 standing in idle, 1
    /// running or jumping, and in between while a crossfade carries it from one
    /// to the other.
    #[inline]
    #[must_use]
    pub const fn blend(&self) -> f32 {
        self.blend
    }

    /// How many advances have found the blend **strictly between** idle and
    /// moving — the frames a crossfade into or out of idle was on screen.
    ///
    /// A counter rather than a reading because the thing it is evidence for is
    /// a transition: the heartbeat the browser gate reads is a second apart,
    /// and a fade a fifth of a second long would show up on it as a snap. This
    /// rises once per frame for as long as the fade takes, so the gate can ask
    /// whether the fade *happened* rather than hoping to sample it.
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
    /// sweeps while it runs.
    #[inline]
    #[must_use]
    pub const fn deviation(&self) -> f32 {
        self.deviation
    }

    /// The name of the state the last advance posed.
    #[must_use]
    pub fn state_name(&self) -> &str {
        self.machine.asset().state_name(self.state.state())
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
    use super::{Animator, FOOTSTEP, Locomotion, RUN_STOP_MPS, STATES, WALK_STOP_MPS, machine};
    use crate::rig;
    use crcbl::anim::{Pose, Sampler};

    /// One tick at the default rate.
    const DT: f32 = 1.0 / crate::game::DEFAULT_TICK_HZ as f32;

    /// Ticks the server half `ticks` times at `speed`, grounded, and hands each
    /// resulting state to the client half.
    fn drive(server: &mut Locomotion, client: &mut Animator, speed: f32, ticks: u32) {
        for _ in 0..ticks {
            server.tick(DT, speed, true, false);
            client.advance(&server.state());
        }
    }

    /// **The committed asset parses and binds to the rig**, and declares
    /// exactly the states, parameters and event the sample drives it by.
    #[test]
    fn the_committed_asset_binds_to_the_rig() {
        let machine = machine();
        let asset = machine.asset();
        assert_eq!(asset.state_count(), STATES.len());
        for name in STATES {
            assert!(asset.state(name).is_some(), "the asset has no {name:?}");
        }
        assert!(
            asset.event(FOOTSTEP).is_some(),
            "the run raises no footsteps"
        );
        assert!(asset.float_parameter("speed").is_ok());
        assert!(asset.bool_parameter("grounded").is_ok());
        assert!(asset.trigger_parameter("jump").is_ok());
        assert_eq!(Locomotion::new().label(), "idle");
    }

    /// **The run's blend stops are the speeds the rig's strides are authored
    /// for**: at [`WALK_STOP_MPS`] the run state draws the walk clip exactly,
    /// at [`RUN_STOP_MPS`] the run clip — and a hair inside either stop, it
    /// draws neither, because the blend has begun. The second half is what
    /// catches a stop the asset moved *outward*, which the clamp at the ends of
    /// a blend would otherwise hide. So the numbers in the asset and the
    /// constants in [`rig`] cannot drift apart unseen.
    #[test]
    fn the_run_blend_stops_are_the_rigs_authored_speeds() {
        /// How far inside a stop the second reading is taken, in metres per
        /// second: far enough that the weight is plainly off the end, near
        /// enough that a stop drifting by a tenth is caught.
        const INSIDE_MPS: f32 = 0.05;

        let machine = machine();
        let skeleton = rig::skeleton();
        let mut sampler = Sampler::new(&skeleton, None);
        let mut drawn = Pose::new(&skeleton);
        let mut expected = Pose::new(&skeleton);
        for (stop, inside, clip, cycle) in [
            (
                WALK_STOP_MPS,
                WALK_STOP_MPS + INSIDE_MPS,
                rig::walk(),
                rig::WALK_CYCLE_S,
            ),
            (
                RUN_STOP_MPS,
                RUN_STOP_MPS - INSIDE_MPS,
                rig::run(),
                rig::RUN_CYCLE_S,
            ),
        ] {
            for (speed, on_the_stop) in [(stop, true), (inside, false)] {
                let mut server = Locomotion::new();
                for _ in 0..60 {
                    server.tick(DT, speed, true, false);
                }
                let state = server.state();
                assert_eq!(server.label(), "run");
                assert!(state.fade().is_none(), "still fading at {speed} m/s");
                sampler.sample_into(&machine, &state, &skeleton, &mut drawn);
                clip.sample_into(state.time() * cycle, &skeleton, &mut expected);
                if on_the_stop {
                    assert_eq!(drawn, expected, "at {speed} m/s the run is not that clip");
                } else {
                    assert_ne!(drawn, expected, "at {speed} m/s the blend has not begun");
                }
            }
        }
    }

    /// Standing still is idle, and the pose does not move. The browser gate's
    /// settle check is this claim, in a browser.
    #[test]
    fn standing_still_holds_one_pose() {
        let mut server = Locomotion::new();
        let mut client = Animator::new();
        drive(&mut server, &mut client, 0.0, 1);
        let held = client.deviation();
        let palette = client.palette().to_vec();
        drive(&mut server, &mut client, 0.0, 120);
        assert_eq!(client.blend(), 0.0);
        assert_eq!(client.state_name(), "idle");
        assert_eq!(client.deviation(), held);
        assert_eq!(client.palette(), palette.as_slice());
    }

    /// And the stance it holds is a posed one, not the rest pose — otherwise
    /// the check above would pass over a character nothing had posed at all.
    #[test]
    fn the_stance_it_holds_is_a_posed_one() {
        let mut server = Locomotion::new();
        let mut client = Animator::new();
        drive(&mut server, &mut client, 0.0, 1);
        assert!(
            client.deviation() > 0.01,
            "the idle stance moved the rig by only {} m",
            client.deviation()
        );
    }

    /// Running moves the pose, and keeps moving it.
    #[test]
    fn running_carries_the_pose() {
        let mut server = Locomotion::new();
        let mut client = Animator::new();
        #[allow(clippy::cast_possible_truncation)]
        let walking = crate::game::WALK_SPEED as f32;
        drive(&mut server, &mut client, walking, 30);
        let mut seen = Vec::new();
        for _ in 0..60 {
            drive(&mut server, &mut client, walking, 1);
            seen.push(client.deviation());
        }
        seen.sort_by(f32::total_cmp);
        seen.dedup();
        assert_eq!(client.blend(), 1.0);
        assert_eq!(client.state_name(), "run");
        assert!(
            seen.len() > 20,
            "a running character should take a new pose nearly every tick; it took {}",
            seen.len()
        );
    }

    /// **The property the browser gate holds the demo to**: the blend sweeps
    /// from idle to moving and back through the crossfades, rather than
    /// snapping — the between-the-ends counter rises on the way up and again on
    /// the way down.
    #[test]
    fn the_blend_sweeps_through_the_crossfade_both_ways() {
        let mut server = Locomotion::new();
        let mut client = Animator::new();
        #[allow(clippy::cast_possible_truncation)]
        let walking = crate::game::WALK_SPEED as f32;

        drive(&mut server, &mut client, walking, 60);
        assert_eq!(
            client.blend(),
            1.0,
            "a second of walking is not out of idle"
        );
        let rising = client.partial();
        assert!(
            rising > 5,
            "the fade into the run spent {rising} frame(s) between the ends"
        );

        drive(&mut server, &mut client, 0.0, 60);
        assert_eq!(
            client.blend(),
            0.0,
            "a second of standing is not back in idle"
        );
        let falling = client.partial() - rising;
        assert!(
            falling > 5,
            "the fade back to idle spent {falling} frame(s) between the ends"
        );
    }

    /// A character held at a steady speed leaves the blend **exactly** at the
    /// top, so the counter that says a fade happened does not tick for ever
    /// while nothing is fading.
    #[test]
    fn a_steady_run_leaves_the_crossing_counter_alone() {
        let mut server = Locomotion::new();
        let mut client = Animator::new();
        #[allow(clippy::cast_possible_truncation)]
        let walking = crate::game::WALK_SPEED as f32;
        drive(&mut server, &mut client, walking, 120);
        let settled = client.partial();
        drive(&mut server, &mut client, walking, 120);
        assert_eq!(client.blend(), 1.0);
        assert_eq!(
            client.partial(),
            settled,
            "the counter rose with nothing fading"
        );
    }

    /// **Both stops are speeds the controller passes**, so a character walking
    /// or running at the commanded speed sits at or past the stop that gait is
    /// authored for rather than a hair short of it.
    #[test]
    fn the_blend_stops_are_speeds_the_controller_passes() {
        assert!(
            f64::from(WALK_STOP_MPS) < crate::game::WALK_SPEED,
            "the walk stop is {WALK_STOP_MPS} m/s and the controller walks at {}",
            crate::game::WALK_SPEED,
        );
        assert!(
            f64::from(RUN_STOP_MPS) < crate::game::RUN_SPEED,
            "the run stop is {RUN_STOP_MPS} m/s and the controller runs at {}",
            crate::game::RUN_SPEED,
        );
    }
}
