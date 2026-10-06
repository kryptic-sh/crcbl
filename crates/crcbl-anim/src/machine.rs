//! The animation state machine: states that each play a clip or a 1D blend,
//! transitions between them, the events their tracks carry, and the root
//! motion their clips author.
//!
//! ```text
//!  StateMachine ── from_ron(text)      the asset: names resolved, refused by name
//!       │
//!  Machine ─────── new(asset, clips)   bound to the clips its states play
//!       │
//!       ├─ start() ──▶ MachineState    POD: state, time, fade, parameters
//!       │
//!       │  on the tick, on the server:
//!       ├─ step(&mut state, dt, &mut events) ──▶ Advance
//!       ├─ root_velocity(&advance, &state, joint, dt) ──▶ Vec3  → the controller
//!       │
//!       │  on the frame, on the client, from the state it was sent:
//!       └─ Sampler::sample_into(&machine, &state, skeleton, pose)  root stripped
//! ```
//!
//! # The split the design gives it
//!
//! `docs/notes/simulation.md` (_What the deleted 17-animation plan left
//! behind_): **the server runs animation logic and samples no pose curves.**
//! Ticks, transition decisions, normalised clip time, root-motion extraction
//! and events are the server's; turning curves into a pose is the client's.
//! That is the line between [`Machine::step`] and [`Machine::root_velocity`] on
//! one side — which read clip *durations* and the root joint's one translation
//! channel, and nothing else — and [`Sampler`] on the other, which is the only
//! thing here that samples a whole pose.
//!
//! # Deterministic, unlike the rest of this crate
//!
//! The crate docs claim no determinism for pose evaluation, and that stands:
//! sampling goes through a slerp. **The machine's stepping is the exception,
//! and is deterministic by construction**: it is `f32` addition,
//! multiplication, division, comparison and `floor` — every one correctly
//! rounded by IEEE 754, and Rust does not contract `a * b + c` into a fused
//! multiply-add behind the program's back — with no transcendental anywhere on
//! the path. The same asset, the same clip durations and the same parameter
//! values tick for tick give the same [`MachineState`] bit for bit, on every
//! target. Its [`Hash`](std::hash::Hash) impl is field by field, so it folds
//! into a tick hash as the design asks.
//!
//! Root motion inherits the guarantee only as far as its curves do: a
//! `Linear`, `Step` or `CubicSpline` translation channel is the same
//! arithmetic, so [`Machine::root_velocity`] over translation keys is
//! deterministic too.
//!
//! # The tick is the clock
//!
//! [`Machine::step`] takes a `dt` and nothing else that tells time, so a server
//! stepping it once per tick at its fixed timestep fires each event on the same
//! tick however fast the frames that drive those ticks happen to run. Nothing
//! here reads a wall clock or a frame time.
//!
//! # Normalised time
//!
//! Each state has a **cycle**: its clip's duration, or for a 1D blend the
//! durations of the two clips around the parameter mixed by the blend's weight
//! — the clips are sampled at one shared phase, [`BlendSpace1d`]'s rule, so the
//! blend has one cycle and its events one track. Time is measured in cycles: a
//! looping state's time runs `0..1` and wraps, a one-shot's runs `0..=1` and
//! holds. A state's `speed` scales the rate.
//!
//! A state whose cycle is zero long — a one-keyframe stance — is a held pose:
//! its time stays where it is, it crosses no events, and an exit time on it is
//! reached only if its time already stands there.
//!
//! # Transitions
//!
//! Evaluated once per step, after time has advanced, in the asset's order; the
//! first one out of the current state whose conditions all hold — and whose
//! exit time the current state has reached, if it names one — fires. Firing
//! clears every trigger its conditions read, starts the destination at time
//! zero, and fades the outgoing state out over the crossfade (a crossfade of
//! zero is a cut).
//!
//! **No transition is evaluated while a crossfade is in flight.** A fade is
//! finished before the machine decides anything else, which is what keeps a
//! fade from being interrupted half way into a pose that is neither end. A
//! trigger set during a fade waits: triggers persist until consumed. Interrupt
//! rules are recorded as deferred in `docs/backlog.md`.
//!
//! An exit time is reached when the current state's time stands at or past it
//! — or, for a looping state, when this step's advance carried the time across
//! it, which catches the step that wrapped from just before the exit time to
//! just after zero.
//!
//! # Events
//!
//! Each state carries a track of `(normalised time, name)`. A step reports the
//! events its advance **crossed**, over the half-open span `[start, end)` of
//! the current state's time: the spans of consecutive steps tile the timeline
//! exactly, so each crossing is reported once — never twice at a seam, never
//! dropped at a loop wrap. A step that wraps several whole cycles reports an
//! event once per cycle. An event at time 0 fires on a state's first step,
//! because a state is entered at time 0.
//!
//! Only the **current** state's track fires. During a crossfade the outgoing
//! state keeps moving for the pose, but its events are muted — a footstep from
//! a walk that is being faded out, landing on top of the run's own, would be a
//! step the character did not take.
//!
//! # Root motion
//!
//! [`Machine::root_velocity`] measures how far the root joint's translation
//! channel moved across the span a step advanced, crossfaded the way the pose
//! is, and divides by `dt`. **The caller applies it to the character
//! controller as a velocity, never to the transform** — the rule in
//! `docs/notes/simulation.md`, decided before any code to avoid an animation
//! that moved a body the server did not. [`Sampler`] strips the same
//! translation from the pose it draws, so the root stays where the controller
//! puts it.

mod asset;
mod root;
mod sampler;
mod state;

pub use asset::{
    BoolParam, EventId, FloatParam, MAX_PARAMETERS, MachineError, ParameterKind, StateId,
    StateMachine, TriggerParam,
};
pub use sampler::Sampler;
pub use state::{Fade, MachineState};

use asset::{Condition, Motion, State};

use crate::Clip;
use crate::blend::locate;

#[cfg(doc)]
use crate::BlendSpace1d;

/// A [`StateMachine`] bound to the clips its states play: what steps a
/// [`MachineState`], extracts root motion, and gives a [`Sampler`] its clips.
///
/// Immutable and shared: one per asset, however many characters play it. All
/// the per-character data is in the [`MachineState`].
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    asset: StateMachine,
    /// One clip per [`StateMachine::clip_names`] entry, in the same order.
    clips: Vec<Clip>,
}

/// How far one state's time moved in one step.
///
/// `end` is **unwrapped**: for a looping state it may pass 1, and how far is
/// how many cycles were completed. A one-shot's `end` is clamped at 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    /// Which state.
    pub state: StateId,
    /// Its time before the step.
    pub start: f32,
    /// Its time after, unwrapped.
    pub end: f32,
}

/// What one [`Machine::step`] did to the clock: the spans the current state
/// and any outgoing state advanced over, and the crossfade weight at the end
/// of the step. What [`Machine::root_velocity`] measures.
///
/// The spans are those of the states that were playing **during** the step: a
/// transition that fires at the end of it changes the state for the next step,
/// not this one's motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Advance {
    /// The state that was current during the step.
    pub current: Span,
    /// The state being faded out of during the step, if a fade was in flight.
    pub source: Option<Span>,
    /// How much of the motion is `current`'s: the fade weight at the end of the
    /// step, and 1 with no fade or a fade that finished on this step.
    pub weight: f32,
}

impl Machine {
    /// Binds `asset` to its clips, asking `lookup` for each name in
    /// [`StateMachine::clip_names`] once.
    ///
    /// # Errors
    ///
    /// [`MachineError::UnknownClip`] for the first name `lookup` has no clip
    /// for.
    pub fn new(
        asset: StateMachine,
        mut lookup: impl FnMut(&str) -> Option<Clip>,
    ) -> Result<Self, MachineError> {
        let clips = asset
            .clips
            .iter()
            .map(|name| {
                lookup(name).ok_or_else(|| MachineError::UnknownClip { name: name.clone() })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { asset, clips })
    }

    /// The asset this machine plays.
    #[inline]
    #[must_use]
    pub const fn asset(&self) -> &StateMachine {
        &self.asset
    }

    /// A fresh state: the initial state at time zero, every parameter at its
    /// declared default, every trigger clear, no fade.
    #[must_use]
    pub fn start(&self) -> MachineState {
        let mut values = [0.0; MAX_PARAMETERS];
        for (value, parameter) in values.iter_mut().zip(&self.asset.parameters) {
            *value = parameter.default;
        }
        MachineState {
            state: self.asset.initial,
            time: 0.0,
            fade: None,
            values,
        }
    }

    /// Advances `state` by `dt` seconds, writes the events crossed into
    /// `events` (cleared first), and takes the first transition that holds.
    ///
    /// The order inside one step, which the module docs give the reasons for:
    /// the current state's time advances and its events are reported; a fade in
    /// flight advances, and ends if its duration is reached; then, if no fade
    /// is left in flight, the transitions out of the current state are tried.
    ///
    /// A `dt` that is negative, zero or not finite advances nothing — the
    /// transitions are still tried, against the parameters as they stand.
    ///
    /// # Panics
    ///
    /// If `state` names a state this machine has not got — a state started from
    /// a different machine.
    pub fn step(&self, state: &mut MachineState, dt: f32, events: &mut Vec<EventId>) -> Advance {
        events.clear();
        let dt = if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };

        let current = self.advance(state.state, &mut state.time, &state.values, dt);
        self.report(current, events);

        let mut source = None;
        let mut weight = 1.0;
        if let Some(mut fade) = state.fade {
            source = Some(self.advance(fade.from, &mut fade.from_time, &state.values, dt));
            fade.elapsed += dt;
            if fade.elapsed >= fade.duration {
                state.fade = None;
            } else {
                weight = fade.weight();
                state.fade = Some(fade);
            }
        }

        if state.fade.is_none() {
            self.take_transition(state, current);
        }
        Advance {
            current,
            source,
            weight,
        }
    }

    /// The clips, in [`StateMachine::clip_names`] order.
    pub(crate) fn clip(&self, index: usize) -> &Clip {
        &self.clips[index]
    }

    /// A state of the asset.
    pub(crate) fn state_def(&self, state: StateId) -> &State {
        &self.asset.states[state.index()]
    }

    /// How long one cycle of `motion` lasts at these parameter values, in
    /// seconds. See the module docs for a blend's cycle.
    fn cycle_seconds(&self, motion: &Motion, values: &[f32; MAX_PARAMETERS]) -> f32 {
        match motion {
            Motion::Clip(clip) => self.clips[*clip].duration(),
            Motion::Blend1d {
                parameter,
                positions,
                clips,
            } => {
                let blend = locate(positions, values[*parameter]);
                let lower = self.clips[clips[blend.lower]].duration();
                let upper = self.clips[clips[blend.upper]].duration();
                // The ends exact, for the reason `blend_into` gives: a blend
                // sitting on a stop is that clip, cycle and all.
                if blend.weight <= 0.0 {
                    lower
                } else if blend.weight >= 1.0 {
                    upper
                } else {
                    lower + (upper - lower) * blend.weight
                }
            }
        }
    }

    /// Advances one state's normalised time by `dt` seconds.
    fn advance(
        &self,
        state: StateId,
        time: &mut f32,
        values: &[f32; MAX_PARAMETERS],
        dt: f32,
    ) -> Span {
        let def = self.state_def(state);
        let start = *time;
        let seconds = self.cycle_seconds(&def.motion, values);
        // A held pose has no cycle to advance through — see the module docs.
        // A NaN cycle holds too, rather than poisoning the time.
        if seconds.is_nan() || seconds <= 0.0 {
            return Span {
                state,
                start,
                end: start,
            };
        }
        let cycles = dt * def.speed / seconds;
        if def.looping {
            let end = start + cycles;
            // Exact: `end - floor(end)` is representable for any non-negative
            // `end`, so the next step starts precisely where this one's span
            // stopped and the two spans tile.
            *time = end - end.floor();
            Span { state, start, end }
        } else {
            let end = (start + cycles).min(1.0);
            *time = end;
            Span { state, start, end }
        }
    }

    /// Writes every event the current state's span crossed into `events`.
    fn report(&self, span: Span, events: &mut Vec<EventId>) {
        let def = self.state_def(span.state);
        for event in &def.events {
            for _ in 0..crossings(span, event.at, def.looping) {
                events.push(event.id);
            }
        }
    }

    /// Fires the first transition out of the current state that holds.
    fn take_transition(&self, state: &mut MachineState, span: Span) {
        let looping = self.state_def(state.state).looping;
        let Some(transition) = self.asset.transitions.iter().find(|transition| {
            transition.from == state.state
                && transition
                    .conditions
                    .iter()
                    .all(|condition| holds(*condition, &state.values))
                && transition
                    .exit_time
                    .is_none_or(|exit| reached(span, state.time, exit, looping))
        }) else {
            return;
        };
        for condition in &transition.conditions {
            if let Condition::Triggered(parameter) = *condition {
                state.values[parameter] = 0.0;
            }
        }
        state.fade = (transition.crossfade > 0.0).then_some(Fade {
            from: state.state,
            from_time: state.time,
            elapsed: 0.0,
            duration: transition.crossfade,
        });
        state.state = transition.to;
        state.time = 0.0;
    }
}

/// How many times a span crossed normalised time `at`: the count of instants
/// congruent to `at` (for a looping state) inside the half-open `[start, end)`.
///
/// Counted against the wrapped tail `end - floor(end)` — the exact value the
/// state's time is left at — rather than by subtracting `at` from the unwrapped
/// end, whose rounding could put an event a hair either side of a seam and
/// count it on both steps or neither.
fn crossings(span: Span, at: f32, looping: bool) -> u64 {
    if !looping {
        return u64::from(span.start <= at && at < span.end);
    }
    let wraps = span.end.floor();
    let tail = span.end - wraps;
    if wraps < 1.0 {
        return u64::from(span.start <= at && at < tail);
    }
    // A float to an integer count: `as` saturates rather than wrapping, and
    // `wraps` is a whole number of at least one, so nothing is truncated.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = wraps as u64 - 1;
    u64::from(at >= span.start) + whole + u64::from(at < tail)
}

/// Whether the current state has reached an exit time: its time stands at or
/// past it, or — looping — this step's span crossed it.
fn reached(span: Span, time: f32, exit: f32, looping: bool) -> bool {
    time >= exit || (looping && crossings(span, exit, true) > 0)
}

/// Whether one condition holds against the parameter values.
fn holds(condition: Condition, values: &[f32; MAX_PARAMETERS]) -> bool {
    match condition {
        Condition::Above(parameter, threshold) => values[parameter] > threshold,
        Condition::Below(parameter, threshold) => values[parameter] < threshold,
        Condition::IsTrue(parameter) | Condition::Triggered(parameter) => values[parameter] != 0.0,
        Condition::IsFalse(parameter) => values[parameter] == 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::{Span, StateId, crossings};

    fn span(start: f32, end: f32) -> Span {
        Span {
            state: StateId(0),
            start,
            end,
        }
    }

    #[test]
    fn a_span_inside_one_cycle_crosses_what_it_covers() {
        assert_eq!(crossings(span(0.1, 0.4), 0.3, true), 1);
        assert_eq!(crossings(span(0.1, 0.4), 0.5, true), 0);
    }

    /// The span is half-open: its start is in, its end is out, so the next
    /// span — which starts at this one's end — owns that instant.
    #[test]
    fn the_start_is_in_and_the_end_is_out() {
        assert_eq!(crossings(span(0.3, 0.6), 0.3, true), 1);
        assert_eq!(crossings(span(0.0, 0.3), 0.3, true), 0);
    }

    #[test]
    fn a_wrap_counts_the_event_on_the_far_side_of_the_seam() {
        // 0.9 → 1.2 wraps to 0.2: an event at 0.1 is crossed, one at 0.5 is not.
        assert_eq!(crossings(span(0.9, 1.2), 0.1, true), 1);
        assert_eq!(crossings(span(0.9, 1.2), 0.5, true), 0);
        assert_eq!(crossings(span(0.9, 1.2), 0.95, true), 1);
    }

    #[test]
    fn several_whole_cycles_count_once_each() {
        assert_eq!(crossings(span(0.5, 3.75), 0.25, true), 3);
        assert_eq!(crossings(span(0.5, 3.75), 0.6, true), 4);
    }

    #[test]
    fn a_one_shot_never_wraps() {
        assert_eq!(crossings(span(0.8, 1.0), 0.9, false), 1);
        assert_eq!(crossings(span(1.0, 1.0), 0.9, false), 0);
    }

    #[test]
    fn a_span_that_did_not_move_crosses_nothing() {
        assert_eq!(crossings(span(0.0, 0.0), 0.0, true), 0);
    }
}
