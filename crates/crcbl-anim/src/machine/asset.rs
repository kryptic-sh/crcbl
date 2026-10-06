//! The state machine asset: a RON document read into file-form structs, then
//! resolved name by name into indices, refusing whatever names nothing.
//!
//! ```text
//! StateMachine(
//!     parameters: [
//!         Float(name: "speed", default: 0.0),
//!         Bool(name: "grounded", default: true),
//!         Trigger(name: "jump"),
//!     ],
//!     initial: "idle",
//!     states: [
//!         State(name: "idle", motion: Clip("idle")),
//!         State(
//!             name: "run",
//!             motion: Blend1d(parameter: "speed", stops: [(2.6, "walk"), (5.5, "run")]),
//!             events: [Event(at: 0.0, name: "footstep"), Event(at: 0.5, name: "footstep")],
//!         ),
//!         State(name: "jump", motion: Clip("jump"), looping: false),
//!     ],
//!     transitions: [
//!         Transition(from: "idle", to: "run", when: [Above("speed", 0.5)], crossfade: 0.2),
//!         Transition(from: "run", to: "idle", when: [Below("speed", 0.5)], crossfade: 0.25),
//!     ],
//! )
//! ```
//!
//! # The file form is not the memory form
//!
//! The file names everything — states, parameters, clips, events — because a
//! person writes it. The memory form holds indices, because the machine is
//! stepped every tick and a string comparison per condition per tick is the
//! cost of leaving them as names. [`StateMachine::from_ron`] is the one place
//! the two meet, and **every name is resolved there or the asset is refused**:
//! a transition to a state that does not exist, a condition on a parameter
//! nobody declared, a condition that reads a trigger as a number. Each of those
//! is an asset that would otherwise misbehave silently at the one moment its
//! condition came true, which is the worst time to find out.
//!
//! Clips are the exception, and on purpose: a clip name is resolved when the
//! asset is bound to the clips it plays ([`Machine::new`](super::Machine::new)),
//! because the asset does not hold the clips and a parse that refused an
//! unknown clip would need them in hand.

use std::fmt;

use serde::Deserialize;

use crate::BlendSpaceError;
use crate::blend::check_positions;

/// How many parameters one machine may declare.
///
/// The runtime state ([`MachineState`](super::MachineState)) holds one value
/// per parameter in a fixed array, so that the whole state is a `Copy` value of
/// known size — what a replicated, saved, hashed component has to be. An asset
/// declaring more is refused by name rather than truncated.
pub const MAX_PARAMETERS: usize = 16;

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// A state of a [`StateMachine`], by index.
///
/// The [`Default`] is the first state, which every machine has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StateId(pub(crate) u16);

impl StateId {
    /// The state's position in the asset's `states` list.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// An animation event name, interned by the asset.
///
/// What [`Machine::step`](super::Machine::step) reports for each event it
/// crosses. Compared as an id rather than a string, so a footstep counter is an
/// integer comparison per event; [`StateMachine::event_name`] turns it back
/// into the name for a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventId(pub(crate) u16);

/// A float parameter, resolved by [`StateMachine::float_parameter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FloatParam(pub(crate) u8);

/// A bool parameter, resolved by [`StateMachine::bool_parameter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoolParam(pub(crate) u8);

/// A trigger parameter, resolved by [`StateMachine::trigger_parameter`].
///
/// A trigger is a bool that a transition **consumes**: it stays set until a
/// transition whose conditions read it fires, and that transition clears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TriggerParam(pub(crate) u8);

/// What kind of value a parameter holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterKind {
    /// A number, compared with `Above` and `Below` and driving a 1D blend.
    Float,
    /// A flag, tested with `IsTrue` and `IsFalse`.
    Bool,
    /// A one-shot flag, tested with `Triggered` and cleared by the transition
    /// that reads it.
    Trigger,
}

impl fmt::Display for ParameterKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Trigger => "trigger",
        })
    }
}

// ---------------------------------------------------------------------------
// The resolved asset
// ---------------------------------------------------------------------------

/// One declared parameter.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Parameter {
    pub(crate) name: String,
    pub(crate) kind: ParameterKind,
    /// The value a fresh state starts with: the float itself, or `0.0`/`1.0`
    /// for a bool. A trigger always starts clear.
    pub(crate) default: f32,
}

/// What a state plays.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Motion {
    /// One clip, by its index in [`StateMachine::clip_names`].
    Clip(usize),
    /// Clips on one axis, mixed by where a float parameter falls between them —
    /// [`BlendSpace1d`](crate::BlendSpace1d)'s rule, sampled at a shared phase.
    Blend1d {
        /// The float parameter, by index.
        parameter: usize,
        /// Each stop's axis position, strictly ascending.
        positions: Vec<f32>,
        /// Each stop's clip, in the same order.
        clips: Vec<usize>,
    },
}

/// One event on a state's track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Event {
    /// Where in the state's cycle it fires, normalised: `0..1`.
    pub(crate) at: f32,
    pub(crate) id: EventId,
}

/// One state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub(crate) name: String,
    pub(crate) motion: Motion,
    /// Whether the normalised time wraps at the end of the cycle (`true`) or
    /// holds at 1 (`false`).
    pub(crate) looping: bool,
    /// A playback-rate multiplier. Non-negative and finite.
    pub(crate) speed: f32,
    /// The event track, in file order.
    pub(crate) events: Vec<Event>,
}

/// One test a transition makes against the parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Condition {
    /// A float parameter strictly above a threshold.
    Above(usize, f32),
    /// A float parameter strictly below a threshold.
    Below(usize, f32),
    /// A bool parameter set.
    IsTrue(usize),
    /// A bool parameter clear.
    IsFalse(usize),
    /// A trigger parameter set — and cleared when the transition fires.
    Triggered(usize),
}

/// One transition.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Transition {
    pub(crate) from: StateId,
    pub(crate) to: StateId,
    /// Every one must hold. An empty list holds always, which with an exit
    /// time is the automatic "when the clip ends" transition.
    pub(crate) conditions: Vec<Condition>,
    /// The normalised time of `from` that must have been reached, `0..=1`.
    pub(crate) exit_time: Option<f32>,
    /// How long the crossfade into `to` lasts, in seconds. Zero is a cut.
    pub(crate) crossfade: f32,
}

/// A parsed, name-checked animation state machine: states that each play a
/// clip or a 1D blend, and transitions between them on conditions over
/// parameters, an exit time and a crossfade.
///
/// Read-only once built. What it describes is stepped by
/// [`Machine`](super::Machine), which binds it to the clips it names; the
/// per-character state that changes every tick is
/// [`MachineState`](super::MachineState).
#[derive(Clone, Debug, PartialEq)]
pub struct StateMachine {
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) states: Vec<State>,
    pub(crate) transitions: Vec<Transition>,
    pub(crate) initial: StateId,
    /// Every clip name a state plays, deduplicated, in first-use order — the
    /// order [`Machine::new`](super::Machine::new) binds them in.
    pub(crate) clips: Vec<String>,
    /// Every event name a state's track carries, deduplicated, in first-use
    /// order. An [`EventId`] indexes this.
    pub(crate) events: Vec<String>,
}

impl StateMachine {
    /// Parses one RON document and resolves every name in it.
    ///
    /// # Errors
    ///
    /// [`MachineError`] naming what is wrong: text that is not RON or not this
    /// shape ([`MachineError::Parse`], with ron's line, column and message), or
    /// a document that parses and does not hold together — an unknown state or
    /// parameter, a condition of the wrong kind, a negative crossfade, an exit
    /// time or event outside the cycle, a duplicate name, a blend whose stops
    /// do not ascend. See [`MachineError`] for each.
    pub fn from_ron(text: &str) -> Result<Self, MachineError> {
        let file: MachineFile = ron::from_str(text).map_err(|error| MachineError::Parse {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        })?;
        Self::resolve(file)
    }

    /// How many states the machine has. Never zero.
    #[inline]
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// The state the machine starts in.
    #[inline]
    #[must_use]
    pub const fn initial(&self) -> StateId {
        self.initial
    }

    /// The state named `name`, if there is one.
    #[must_use]
    pub fn state(&self, name: &str) -> Option<StateId> {
        self.states
            .iter()
            .position(|state| state.name == name)
            .map(state_id)
    }

    /// A state's name.
    ///
    /// # Panics
    ///
    /// If `state` is not a state of this machine.
    #[must_use]
    pub fn state_name(&self, state: StateId) -> &str {
        &self.states[state.index()].name
    }

    /// The float parameter named `name`.
    ///
    /// # Errors
    ///
    /// [`MachineError::UnknownParameter`] if nothing is declared under that
    /// name, [`MachineError::WrongParameterKind`] if it is not a float.
    pub fn float_parameter(&self, name: &str) -> Result<FloatParam, MachineError> {
        self.parameter_of(name, ParameterKind::Float)
            .map(|index| FloatParam(parameter_byte(index)))
    }

    /// The bool parameter named `name`.
    ///
    /// # Errors
    ///
    /// As [`float_parameter`](Self::float_parameter), for a bool.
    pub fn bool_parameter(&self, name: &str) -> Result<BoolParam, MachineError> {
        self.parameter_of(name, ParameterKind::Bool)
            .map(|index| BoolParam(parameter_byte(index)))
    }

    /// The trigger parameter named `name`.
    ///
    /// # Errors
    ///
    /// As [`float_parameter`](Self::float_parameter), for a trigger.
    pub fn trigger_parameter(&self, name: &str) -> Result<TriggerParam, MachineError> {
        self.parameter_of(name, ParameterKind::Trigger)
            .map(|index| TriggerParam(parameter_byte(index)))
    }

    /// The event named `name`, if any state's track carries it.
    #[must_use]
    pub fn event(&self, name: &str) -> Option<EventId> {
        self.events
            .iter()
            .position(|event| event == name)
            .map(event_id)
    }

    /// An event's name.
    ///
    /// # Panics
    ///
    /// If `event` is not an event of this machine.
    #[must_use]
    pub fn event_name(&self, event: EventId) -> &str {
        &self.events[usize::from(event.0)]
    }

    /// Every clip name a state plays, deduplicated, in first-use order.
    #[must_use]
    pub fn clip_names(&self) -> &[String] {
        &self.clips
    }

    fn parameter_of(&self, name: &str, expected: ParameterKind) -> Result<usize, MachineError> {
        find_parameter(&self.parameters, name, expected)
    }

    /// The file form, resolved. Every check the type docs promise is here.
    fn resolve(file: MachineFile) -> Result<Self, MachineError> {
        if file.states.is_empty() {
            return Err(MachineError::NoStates);
        }
        if file.parameters.len() > MAX_PARAMETERS {
            return Err(MachineError::TooManyParameters {
                count: file.parameters.len(),
            });
        }
        if file.states.len() > usize::from(u16::MAX) {
            return Err(MachineError::TooManyStates {
                count: file.states.len(),
            });
        }

        let mut parameters: Vec<Parameter> = Vec::with_capacity(file.parameters.len());
        for declared in file.parameters {
            let (name, kind, default) = match declared {
                ParameterFile::Float { name, default } => {
                    if !default.is_finite() {
                        return Err(MachineError::NotFinite {
                            what: "default",
                            owner: name,
                        });
                    }
                    (name, ParameterKind::Float, default)
                }
                ParameterFile::Bool { name, default } => {
                    (name, ParameterKind::Bool, f32::from(u8::from(default)))
                }
                ParameterFile::Trigger { name } => (name, ParameterKind::Trigger, 0.0),
            };
            if parameters.iter().any(|parameter| parameter.name == name) {
                return Err(MachineError::DuplicateParameter { name });
            }
            parameters.push(Parameter {
                name,
                kind,
                default,
            });
        }
        let parameter_of =
            |name: &str, expected: ParameterKind| find_parameter(&parameters, name, expected);

        let names: Vec<&str> = file
            .states
            .iter()
            .map(|state| state.name.as_str())
            .collect();
        for (index, name) in names.iter().enumerate() {
            if names[..index].contains(name) {
                return Err(MachineError::DuplicateState {
                    name: (*name).to_owned(),
                });
            }
        }
        let state_of = |name: &str| {
            names
                .iter()
                .position(|candidate| *candidate == name)
                .map(state_id)
                .ok_or_else(|| MachineError::UnknownState {
                    name: name.to_owned(),
                })
        };
        let initial = state_of(&file.initial)?;

        let mut transitions = Vec::with_capacity(file.transitions.len());
        for transition in &file.transitions {
            let from = state_of(&transition.from)?;
            let to = state_of(&transition.to)?;
            if !transition.crossfade.is_finite() {
                return Err(MachineError::NotFinite {
                    what: "crossfade",
                    owner: transition_label(transition),
                });
            }
            if transition.crossfade < 0.0 {
                return Err(MachineError::NegativeCrossfade {
                    from: transition.from.clone(),
                    to: transition.to.clone(),
                    seconds: transition.crossfade,
                });
            }
            if let Some(exit_time) = transition.exit_time
                && !(0.0..=1.0).contains(&exit_time)
            {
                // A NaN fails `contains` as well, so it is refused here too.
                return Err(MachineError::ExitTimeOutOfRange {
                    from: transition.from.clone(),
                    to: transition.to.clone(),
                    exit_time,
                });
            }
            let mut conditions = Vec::with_capacity(transition.when.len());
            for condition in &transition.when {
                conditions.push(match condition {
                    ConditionFile::Above(name, threshold)
                    | ConditionFile::Below(name, threshold) => {
                        if !threshold.is_finite() {
                            return Err(MachineError::NotFinite {
                                what: "threshold",
                                owner: transition_label(transition),
                            });
                        }
                        let index = parameter_of(name, ParameterKind::Float)?;
                        if matches!(condition, ConditionFile::Above(..)) {
                            Condition::Above(index, *threshold)
                        } else {
                            Condition::Below(index, *threshold)
                        }
                    }
                    ConditionFile::IsTrue(name) => {
                        Condition::IsTrue(parameter_of(name, ParameterKind::Bool)?)
                    }
                    ConditionFile::IsFalse(name) => {
                        Condition::IsFalse(parameter_of(name, ParameterKind::Bool)?)
                    }
                    ConditionFile::Triggered(name) => {
                        Condition::Triggered(parameter_of(name, ParameterKind::Trigger)?)
                    }
                });
            }
            transitions.push(Transition {
                from,
                to,
                conditions,
                exit_time: transition.exit_time,
                crossfade: transition.crossfade,
            });
        }

        let mut clips: Vec<String> = Vec::new();
        let mut clip_of = |name: &str| {
            clips
                .iter()
                .position(|clip| clip == name)
                .unwrap_or_else(|| {
                    clips.push(name.to_owned());
                    clips.len() - 1
                })
        };
        let mut events: Vec<String> = Vec::new();
        let mut states = Vec::with_capacity(file.states.len());
        for state in file.states {
            if !state.speed.is_finite() {
                return Err(MachineError::NotFinite {
                    what: "speed",
                    owner: state.name,
                });
            }
            if state.speed < 0.0 {
                return Err(MachineError::NegativeSpeed {
                    state: state.name,
                    speed: state.speed,
                });
            }
            let motion = match state.motion {
                MotionFile::Clip(clip) => Motion::Clip(clip_of(&clip)),
                MotionFile::Blend1d { parameter, stops } => {
                    let parameter = parameter_of(&parameter, ParameterKind::Float)?;
                    if let Err(error) = check_positions(stops.iter().map(|&(position, _)| position))
                    {
                        return Err(MachineError::BadBlend {
                            state: state.name,
                            error,
                        });
                    }
                    let (positions, clips) = stops
                        .into_iter()
                        .map(|(position, clip)| (position, clip_of(&clip)))
                        .unzip();
                    Motion::Blend1d {
                        parameter,
                        positions,
                        clips,
                    }
                }
            };
            let mut track = Vec::with_capacity(state.events.len());
            for event in state.events {
                if !(0.0..1.0).contains(&event.at) {
                    return Err(MachineError::EventOutOfRange {
                        state: state.name,
                        event: event.name,
                        at: event.at,
                    });
                }
                let index = events
                    .iter()
                    .position(|name| *name == event.name)
                    .unwrap_or_else(|| {
                        events.push(event.name);
                        events.len() - 1
                    });
                if index > usize::from(u16::MAX) {
                    return Err(MachineError::TooManyEvents {
                        count: events.len(),
                    });
                }
                track.push(Event {
                    at: event.at,
                    id: event_id(index),
                });
            }
            states.push(State {
                name: state.name,
                motion,
                looping: state.looping,
                speed: state.speed,
                events: track,
            });
        }

        Ok(Self {
            parameters,
            states,
            transitions,
            initial,
            clips,
            events,
        })
    }
}

/// The index of the parameter named `name`, which must be of kind `expected`.
fn find_parameter(
    parameters: &[Parameter],
    name: &str,
    expected: ParameterKind,
) -> Result<usize, MachineError> {
    let index = parameters
        .iter()
        .position(|parameter| parameter.name == name)
        .ok_or_else(|| MachineError::UnknownParameter {
            name: name.to_owned(),
        })?;
    if parameters[index].kind != expected {
        return Err(MachineError::WrongParameterKind {
            name: name.to_owned(),
            expected,
        });
    }
    Ok(index)
}

/// A state index the resolver has already bounded by [`u16::MAX`].
fn state_id(index: usize) -> StateId {
    StateId(u16::try_from(index).expect("the state count is checked against u16::MAX"))
}

/// An event index the resolver has already bounded by [`u16::MAX`].
fn event_id(index: usize) -> EventId {
    EventId(u16::try_from(index).expect("the event count is checked against u16::MAX"))
}

/// A parameter index the resolver has already bounded by [`MAX_PARAMETERS`].
fn parameter_byte(index: usize) -> u8 {
    u8::try_from(index).expect("the parameter count is checked against MAX_PARAMETERS")
}

/// How an error names a transition: its two ends.
fn transition_label(transition: &TransitionFile) -> String {
    format!("{} -> {}", transition.from, transition.to)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a document is not a [`StateMachine`], or why one could not be bound to
/// its clips. Every variant names the thing at fault.
#[derive(Clone, Debug, PartialEq)]
pub enum MachineError {
    /// The text is not RON, or is RON that is not this shape — an unknown
    /// field, a missing one, a misspelt variant.
    Parse {
        /// Where ron stopped: the start of the span, 1-based.
        line: usize,
        /// See `line`.
        column: usize,
        /// What ron said, which names the offending field or variant.
        message: String,
    },
    /// The document declares no states, so there is nothing to start in.
    NoStates,
    /// Two states share a name, so a transition naming it is ambiguous.
    DuplicateState {
        /// The repeated name.
        name: String,
    },
    /// Two parameters share a name.
    DuplicateParameter {
        /// The repeated name.
        name: String,
    },
    /// More parameters than [`MAX_PARAMETERS`].
    TooManyParameters {
        /// How many were declared.
        count: usize,
    },
    /// More states than a [`StateId`] can index.
    TooManyStates {
        /// How many were declared.
        count: usize,
    },
    /// More distinct event names than an [`EventId`] can index.
    TooManyEvents {
        /// How many there were when the limit was passed.
        count: usize,
    },
    /// `initial` or a transition names a state nobody declared.
    UnknownState {
        /// The name that matched nothing.
        name: String,
    },
    /// A condition, a blend or a caller names a parameter nobody declared.
    UnknownParameter {
        /// The name that matched nothing.
        name: String,
    },
    /// A parameter is used as a kind it is not — a trigger compared as a
    /// number, a float tested as a flag.
    WrongParameterKind {
        /// The parameter.
        name: String,
        /// The kind its use needed.
        expected: ParameterKind,
    },
    /// A transition's crossfade is below zero. A fade cannot take negative
    /// time, and clamping it to a cut would hide a typo.
    NegativeCrossfade {
        /// The transition's source state.
        from: String,
        /// Its destination.
        to: String,
        /// The crossfade it asked for.
        seconds: f32,
    },
    /// A transition's exit time is outside `0..=1` of the source's cycle.
    ExitTimeOutOfRange {
        /// The transition's source state.
        from: String,
        /// Its destination.
        to: String,
        /// The exit time it asked for.
        exit_time: f32,
    },
    /// An event sits outside `0..1` of its state's cycle. 1 is excluded
    /// because on a looping state it is the same instant as 0 and would fire
    /// twice there.
    EventOutOfRange {
        /// The state whose track carries it.
        state: String,
        /// The event.
        event: String,
        /// Where it asked to fire.
        at: f32,
    },
    /// A state plays backwards. Not supported: an event track and an exit time
    /// are both defined against time running forward.
    NegativeSpeed {
        /// The state.
        state: String,
        /// The speed it asked for.
        speed: f32,
    },
    /// A number that must be finite is not.
    NotFinite {
        /// Which field.
        what: &'static str,
        /// Whose: a parameter, a state, or a transition as `from -> to`.
        owner: String,
    },
    /// A state's 1D blend has no stops, or stops that do not strictly ascend.
    BadBlend {
        /// The state.
        state: String,
        /// What [`BlendSpace1d`](crate::BlendSpace1d) would have said.
        error: BlendSpaceError,
    },
    /// A state plays a clip the caller binding the machine does not have.
    UnknownClip {
        /// The name that matched nothing.
        name: String,
    },
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                line,
                column,
                message,
            } => write!(f, "line {line}, column {column}: {message}"),
            Self::NoStates => f.write_str("a state machine with no states has nothing to play"),
            Self::DuplicateState { name } => write!(f, "state {name:?} is declared twice"),
            Self::DuplicateParameter { name } => {
                write!(f, "parameter {name:?} is declared twice")
            }
            Self::TooManyParameters { count } => write!(
                f,
                "{count} parameters are declared, and a machine holds at most {MAX_PARAMETERS}"
            ),
            Self::TooManyStates { count } => write!(
                f,
                "{count} states are declared, and a machine holds at most {}",
                u16::MAX
            ),
            Self::TooManyEvents { count } => write!(
                f,
                "{count} distinct event names, and a machine holds at most {}",
                u16::MAX
            ),
            Self::UnknownState { name } => write!(f, "no state is named {name:?}"),
            Self::UnknownParameter { name } => write!(f, "no parameter is named {name:?}"),
            Self::WrongParameterKind { name, expected } => {
                write!(
                    f,
                    "parameter {name:?} is used as a {expected}, which it is not"
                )
            }
            Self::NegativeCrossfade { from, to, seconds } => write!(
                f,
                "the transition {from:?} -> {to:?} has a negative crossfade of {seconds} s"
            ),
            Self::ExitTimeOutOfRange {
                from,
                to,
                exit_time,
            } => write!(
                f,
                "the transition {from:?} -> {to:?} has exit time {exit_time}, outside 0..=1"
            ),
            Self::EventOutOfRange { state, event, at } => write!(
                f,
                "event {event:?} on state {state:?} is at {at}, outside 0..1"
            ),
            Self::NegativeSpeed { state, speed } => {
                write!(f, "state {state:?} has a negative speed of {speed}")
            }
            Self::NotFinite { what, owner } => {
                write!(f, "the {what} of {owner:?} is not a finite number")
            }
            Self::BadBlend { state, error } => write!(f, "state {state:?}'s blend: {error}"),
            Self::UnknownClip { name } => write!(f, "no clip is named {name:?}"),
        }
    }
}

impl std::error::Error for MachineError {}

// ---------------------------------------------------------------------------
// The file form
// ---------------------------------------------------------------------------

/// The document, as written.
#[derive(Deserialize)]
#[serde(rename = "StateMachine", deny_unknown_fields)]
struct MachineFile {
    #[serde(default)]
    parameters: Vec<ParameterFile>,
    initial: String,
    states: Vec<StateFile>,
    #[serde(default)]
    transitions: Vec<TransitionFile>,
}

/// One parameter declaration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum ParameterFile {
    Float {
        name: String,
        #[serde(default)]
        default: f32,
    },
    Bool {
        name: String,
        #[serde(default)]
        default: bool,
    },
    Trigger {
        name: String,
    },
}

/// One state, as written.
#[derive(Deserialize)]
#[serde(rename = "State", deny_unknown_fields)]
struct StateFile {
    name: String,
    motion: MotionFile,
    #[serde(default = "looping_by_default")]
    looping: bool,
    #[serde(default = "unit_speed")]
    speed: f32,
    #[serde(default)]
    events: Vec<EventFile>,
}

/// A state loops unless it says otherwise: locomotion is the common case, and
/// a one-shot is the one that has to be marked.
const fn looping_by_default() -> bool {
    true
}

/// A state plays at its clips' own rate unless it says otherwise.
const fn unit_speed() -> f32 {
    1.0
}

/// What a state plays, as written: clip names rather than indices.
#[derive(Deserialize)]
enum MotionFile {
    Clip(String),
    Blend1d {
        parameter: String,
        stops: Vec<(f32, String)>,
    },
}

/// One event, as written.
#[derive(Deserialize)]
#[serde(rename = "Event", deny_unknown_fields)]
struct EventFile {
    at: f32,
    name: String,
}

/// One transition, as written.
#[derive(Deserialize)]
#[serde(rename = "Transition", deny_unknown_fields)]
struct TransitionFile {
    from: String,
    to: String,
    #[serde(default)]
    when: Vec<ConditionFile>,
    #[serde(default)]
    exit_time: Option<f32>,
    crossfade: f32,
}

/// One condition, as written: a parameter name and, for a comparison, the
/// threshold.
#[derive(Deserialize)]
enum ConditionFile {
    Above(String, f32),
    Below(String, f32),
    IsTrue(String),
    IsFalse(String),
    Triggered(String),
}
