//! Why an input did what it did: the resolution trace an input inspector
//! shows, which is the answer to "input eaten mysteriously" by the context
//! stack.
//!
//! # Opt in, and bounded
//!
//! Nothing is recorded until [`ActionMap::set_tracing`] turns it on, and an
//! input then costs the map nothing more than the check of that switch. On,
//! the map keeps the most recent [`RESOLUTION_TRACE_CAP`] entries and drops
//! the oldest — a ring of recent inputs rather than one frame's, because a
//! frame's list is empty again before anyone has read it. An input that
//! repeats the newest entry exactly, input and outcome both, counts on that
//! entry ([`TraceEntry::count`]) instead of pushing another: a touchpad's
//! scroll is dozens of wheel events a second, and would otherwise push every
//! key press out of the ring before it was read.
//!
//! # What is traced
//!
//! **Each press**: a key going down (an OS auto-repeat of a key already down
//! is not a new press), a pointer button going down, a wheel turn, an
//! on-screen button pressed or an on-screen stick leaving centre, a pad
//! button going down, and a pad stick or trigger pushed out past
//! [`PAD_ACTIVITY_THRESHOLD`](crate::PAD_ACTIVITY_THRESHOLD) — the same
//! travel that makes the pad the [last device](ActionMap::last_device).
//!
//! **Not traced**: releases, whose route is the press's; pointer motion and
//! the pointer's position, which arrive every frame the mouse moves and would
//! fill the ring with themselves; and a stick or trigger moving while already
//! out, which is a held level, not a new press.
//!
//! # What an entry says
//!
//! Each [`TraceEntry`] names the input and its [`Outcome`], read off the same
//! routes resolution reads (`context.rs`): the context that owns the input and
//! every binding in it that read the input, with the action it drives
//! ([`Outcome::Read`]) — an empty list there is an input the context owns and
//! no binding of a live action read, a chord's key pressed without its
//! modifier, say; that the owner withholds it until it is released
//! ([`Outcome::Withheld`]); that a modal context stopped it on its way down
//! to a context that binds it ([`Outcome::Blocked`]); or that no active
//! context binds it ([`Outcome::Unbound`]). Something that takes an input
//! before it reaches the map — the engine loop's reserved keys, a menu over
//! the game — says so with [`ActionMap::trace_claimed`]
//! ([`Outcome::Claimed`]).
//!
//! # It never changes resolution
//!
//! An entry is explained after the input has resolved, by reading the routes,
//! the withheld inputs and the raw state, and nothing in this module writes
//! any of them. `tests::resolution_is_identical_with_tracing_on_and_off`
//! drives two maps through one script, one tracing and one not, and compares
//! every action after every step.

use core::fmt;
use std::collections::VecDeque;

use crcbl_core::input::{KeyCode, PointerButton};

use crate::binding_text::{stick_name, trigger_name};
use crate::context::View;
use crate::gamepad::{pad_trigger, radial_deadzone};
use crate::{ActionKind, ActionMap, ActionSlot, Binding, PadButton, Stick, Trigger};

/// The most entries [`ActionMap::trace`] holds: the inputs of the last few
/// seconds of play, and as many rows as an inspector panel can show beside
/// the rest of the input section.
pub const RESOLUTION_TRACE_CAP: usize = 16;

/// One input the trace records — see the [module docs](self) for which.
#[derive(Clone, Debug, PartialEq)]
pub enum TracedInput {
    /// A key went down.
    Key(KeyCode),
    /// A pointer button went down.
    MouseButton(PointerButton),
    /// The wheel turned.
    Wheel,
    /// An on-screen button was pressed — [`ActionMap::virtual_button`].
    Control(String),
    /// An on-screen stick left centre — [`ActionMap::virtual_stick`].
    ControlStick(String),
    /// A pad button went down, on any pad.
    PadButton(PadButton),
    /// A pad stick was pushed out past the activity threshold.
    PadStick(Stick),
    /// A pad trigger was pulled past the activity threshold.
    PadTrigger(Trigger),
}

/// Spelled as [`Binding`]'s text form spells the binding that reads it —
/// `Space`, `Mouse:Left`, `Pad:South`, `Virtual:jump` — so a trace row and the
/// binding beside it read alike. A stick or a trigger has no dead zone of its
/// own, so it is spelled without one: `PadStick:Left`.
impl fmt::Display for TracedInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => f.write_str(key.as_str()),
            Self::MouseButton(button) => Binding::MouseButton(*button).fmt(f),
            Self::Wheel => Binding::MouseScroll.fmt(f),
            Self::Control(id) => write!(f, "Virtual:{id}"),
            Self::ControlStick(id) => write!(f, "Virtual:{id} (stick)"),
            Self::PadButton(button) => Binding::PadButton(*button).fmt(f),
            Self::PadStick(stick) => write!(f, "PadStick:{}", stick_name(*stick)),
            Self::PadTrigger(trigger) => write!(f, "PadTrigger:{}", trigger_name(*trigger)),
        }
    }
}

/// One binding that read a traced input, and the action it drives.
#[derive(Clone, Debug, PartialEq)]
pub struct TracedRead {
    /// The action.
    pub action: String,
    /// Its binding that read the input.
    pub binding: Binding,
}

/// Where a traced input went — see the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// `context` owns the input, and each of `reads` read it. Empty when the
    /// context owns it and no binding of a live action in it read it.
    Read {
        /// The context that consumed the input.
        context: String,
        /// Every binding that read it, in declaration order.
        reads: Vec<TracedRead>,
    },
    /// `context` owns the input and withholds it until it is released, since
    /// the stack changed under it or a suppress asked — see `context.rs`.
    Withheld {
        /// The context that would read it.
        context: String,
    },
    /// The modal context `modal` stopped the input before it reached a
    /// context beneath that binds it.
    Blocked {
        /// The modal context.
        modal: String,
    },
    /// No active context binds the input.
    Unbound,
    /// Something took the input before it reached the map, by its own account
    /// — see [`ActionMap::trace_claimed`].
    Claimed {
        /// What took it, as the claimant names itself.
        by: String,
    },
}

/// `ui: ui_accept (Space)`, `gameplay: nothing read it`, `withheld by
/// gameplay until released`, `blocked by modal inventory`, `unbound`,
/// `claimed by the loop: pause`.
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { context, reads } => {
                write!(f, "{context}: ")?;
                if reads.is_empty() {
                    return f.write_str("nothing read it");
                }
                for (index, read) in reads.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{} ({})", read.action, read.binding)?;
                }
                Ok(())
            }
            Self::Withheld { context } => write!(f, "withheld by {context} until released"),
            Self::Blocked { modal } => write!(f, "blocked by modal {modal}"),
            Self::Unbound => f.write_str("unbound"),
            Self::Claimed { by } => write!(f, "claimed by {by}"),
        }
    }
}

/// One row of the trace: an input, where it went, and how many times in a
/// row it went there.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceEntry {
    /// The input.
    pub input: TracedInput,
    /// Where it went.
    pub outcome: Outcome,
    /// How many consecutive inputs this entry stands for — see the
    /// [module docs](self). At least one.
    pub count: u32,
}

impl ActionMap {
    /// Start or stop recording the resolution trace — see the
    /// [module docs](self). Stopping drops what was recorded.
    pub fn set_tracing(&mut self, on: bool) {
        if !on {
            self.trace = None;
        } else if self.trace.is_none() {
            self.trace = Some(VecDeque::with_capacity(RESOLUTION_TRACE_CAP));
        }
    }

    /// Whether the resolution trace is being recorded.
    #[must_use]
    pub const fn is_tracing(&self) -> bool {
        self.trace.is_some()
    }

    /// The recorded trace, oldest first; empty while tracing is off.
    pub fn trace(&self) -> impl DoubleEndedIterator<Item = &TraceEntry> {
        self.trace.iter().flatten()
    }

    /// Records that `by` took `input` before it reached this map — the engine
    /// loop's reserved keys, a menu drawn over the game — so the trace shows
    /// where an input that never arrived went. Nothing happens while tracing
    /// is off, and the map's state is not touched either way.
    pub fn trace_claimed(&mut self, input: TracedInput, by: &str) {
        if self.trace.is_some() {
            self.push_trace(input, Outcome::Claimed { by: by.to_owned() });
        }
    }

    /// Records where `input` went, if tracing is on. Called after the input
    /// resolved; `input` is built only when it is recorded.
    pub(crate) fn traced(&mut self, input: impl FnOnce() -> TracedInput) {
        if self.trace.is_none() {
            return;
        }
        let input = input();
        let outcome = self.explain(&input);
        self.push_trace(input, outcome);
    }

    /// Appends an entry, or counts it on the newest one it repeats, and drops
    /// the oldest past [`RESOLUTION_TRACE_CAP`].
    fn push_trace(&mut self, input: TracedInput, outcome: Outcome) {
        let Some(trace) = &mut self.trace else {
            return;
        };
        if let Some(newest) = trace.back_mut()
            && newest.input == input
            && newest.outcome == outcome
        {
            newest.count = newest.count.saturating_add(1);
            return;
        }
        if trace.len() == RESOLUTION_TRACE_CAP {
            trace.pop_front();
        }
        trace.push_back(TraceEntry {
            input,
            outcome,
            count: 1,
        });
    }

    /// Where `input` goes as the map stands now.
    fn explain(&self, input: &TracedInput) -> Outcome {
        let Some(context) = self.routes.owner(input, self.held_pad_buttons) else {
            return match self.blocking_modal(|binding| claims(binding, input)) {
                Some(modal) => Outcome::Blocked {
                    modal: self.contexts[modal].clone(),
                },
                None => Outcome::Unbound,
            };
        };
        let name = self.contexts[context].clone();
        if self.suppressed.holds(input) {
            return Outcome::Withheld { context: name };
        }
        let view = &self.view(context);
        let reads = self
            .slots
            .iter()
            .enumerate()
            .filter(|&(idx, slot)| slot.context == context && self.is_live(idx))
            .flat_map(|(_, slot)| {
                slot.decl
                    .bindings
                    .iter()
                    .filter(move |binding| self.reads(view, slot, binding, input))
                    .map(|binding| TracedRead {
                        action: slot.decl.name.clone(),
                        binding: binding.clone(),
                    })
            })
            .collect();
        Outcome::Read {
            context: name,
            reads,
        }
    }

    /// What `context` can see of the raw input — the [`View`] resolution
    /// reads through.
    fn view(&self, context: usize) -> View<'_> {
        View {
            context,
            routes: &self.routes,
            suppressed: &self.suppressed,
            held_keys: &self.held_keys,
            held_buttons: &self.held_buttons,
            held_controls: &self.held_controls,
            held_pad_buttons: self.held_pad_buttons,
        }
    }

    /// Whether `binding`, on `slot`'s action, read `input` — the questions
    /// `ActionMap::resolve_slot` asks of each binding, narrowed to the one
    /// input. A binding an action of its kind never reads (a
    /// [`Binding::Key`] on an [`ActionKind::Axis2`]) reads nothing, as it
    /// resolves to nothing; neither does any binding on a 1-D axis whose
    /// [`Binding::PointerPosition`] has a position to replace them with; and
    /// a level reads only once it is past the binding's own dead zone or
    /// threshold.
    /// `tests::a_binding_is_traced_as_reading_exactly_when_its_action_moves`
    /// holds the two to the same answers.
    fn reads(
        &self,
        view: &View<'_>,
        slot: &ActionSlot,
        binding: &Binding,
        input: &TracedInput,
    ) -> bool {
        use ActionKind::{Axis1, Axis2, Button};
        let kind = slot.kind();
        let absolute = kind == Axis1
            && self.pointer.is_some()
            && view.pointer()
            && slot
                .decl
                .bindings
                .iter()
                .any(|bound| matches!(bound, Binding::PointerPosition { .. }));
        if absolute {
            return false;
        }
        match (input, binding) {
            (TracedInput::Key(key), Binding::Key(bound)) => {
                kind != Axis2 && bound == key && view.key(*key)
            }
            (
                TracedInput::Key(key),
                Binding::Chord {
                    modifier,
                    key: bound,
                },
            ) => kind != Axis2 && bound == key && view.chord(*modifier, *key),
            (TracedInput::Key(key), Binding::KeyAxis { .. }) => {
                kind != Axis2 && binding.owns_key(*key) && view.key(*key)
            }
            (TracedInput::Key(key), Binding::Wasd { .. }) => {
                kind != Axis1 && binding.owns_key(*key) && view.key(*key)
            }
            (TracedInput::MouseButton(button), Binding::MouseButton(bound)) => {
                kind == Button && bound == button && view.button(*button)
            }
            (
                TracedInput::MouseButton(button),
                Binding::ButtonChord {
                    modifier,
                    button: bound,
                },
            ) => kind == Button && bound == button && view.button_chord(*modifier, *button),
            (TracedInput::Wheel, Binding::MouseScroll) => kind == Axis1 && view.scroll(),
            (TracedInput::Wheel, Binding::ScrollChord { held }) => {
                kind == Axis1 && view.scroll_chord(*held)
            }
            (TracedInput::Control(id), Binding::Virtual(bound)) => {
                kind == Button && bound == id && view.control(id)
            }
            (TracedInput::ControlStick(id), Binding::Virtual(bound)) => {
                kind == Axis2
                    && bound == id
                    && view.stick(id)
                    && self
                        .control_sticks
                        .get(id.as_str())
                        .is_some_and(|&deflection| deflection != (0.0, 0.0))
            }
            (TracedInput::PadButton(button), Binding::PadButton(bound)) => {
                kind != Axis2 && bound == button && view.pad_button(*button)
            }
            (
                TracedInput::PadButton(button),
                Binding::PadChord {
                    modifier,
                    button: bound,
                },
            ) => kind != Axis2 && bound == button && view.pad_chord(*modifier, *button),
            (TracedInput::PadButton(button), Binding::PadDpad) => {
                kind != Axis1 && PadButton::DPAD.contains(button) && view.pad_button(*button)
            }
            (
                TracedInput::PadStick(stick),
                Binding::PadStick {
                    stick: bound,
                    deadzone,
                },
            ) => {
                kind == Axis2
                    && bound == stick
                    && view.pad_stick(*stick)
                    && self
                        .pads
                        .values()
                        .any(|pad| radial_deadzone(pad.stick(*stick), *deadzone) != (0.0, 0.0))
            }
            (
                TracedInput::PadTrigger(trigger),
                Binding::PadTrigger {
                    trigger: bound,
                    threshold,
                },
            ) => {
                kind != Axis2
                    && bound == trigger
                    && view.trigger(*trigger)
                    && pad_trigger(&self.pads, *trigger) > *threshold
            }
            _ => false,
        }
    }
}

/// Whether `binding` claims `input` for its context, whether or not it reads
/// it now — what makes a context the owner in `Routes::build`, so a context
/// binding the input beneath a modal one is what [`Outcome::Blocked`] means.
fn claims(binding: &Binding, input: &TracedInput) -> bool {
    match input {
        TracedInput::Key(key) => binding.owns_key(*key),
        TracedInput::MouseButton(button) => binding.mouse_button() == Some(*button),
        TracedInput::Wheel => binding.reads_wheel(),
        TracedInput::Control(id) | TracedInput::ControlStick(id) => {
            matches!(binding, Binding::Virtual(bound) if bound == id)
        }
        TracedInput::PadButton(button) => match binding {
            Binding::PadButton(bound) | Binding::PadChord { button: bound, .. } => bound == button,
            Binding::PadDpad => PadButton::DPAD.contains(button),
            _ => false,
        },
        TracedInput::PadStick(stick) => {
            matches!(binding, Binding::PadStick { stick: bound, .. } if bound == stick)
        }
        TracedInput::PadTrigger(trigger) => {
            matches!(binding, Binding::PadTrigger { trigger: bound, .. } if bound == trigger)
        }
    }
}

#[cfg(test)]
mod tests;
