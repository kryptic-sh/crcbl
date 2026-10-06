//! The debug panel's input section: what an [`ActionMap`] hears and what it
//! makes of it, for the "input eaten mysteriously" question the context stack
//! raises.
//!
//! A game adds it in [`HostedGame::debug_sections`] over the map it plays on —
//! `panel.add(&InputInspector::new(&self.actions))` — and the loop does the
//! rest: while the panel is showing it keeps the resolution trace of the map
//! the game hands over through [`HostedGame::actions`] switched on, and records
//! the presses the loop takes before that map hears them, so the section's
//! trace rows say where every press went. Hidden, the panel never asks for the
//! section, and the loop switches the trace off.
//!
//! **Not added by the loop itself**: which sections a sample's panel carries is
//! the sample's to say, and each sample's own tests hold its panel to exactly
//! the modules it has.
//!
//! [`HostedGame::debug_sections`]: crate::engine::HostedGame::debug_sections
//! [`HostedGame::actions`]: crate::engine::HostedGame::actions

use core::fmt::{self, Write as _};

use crate::input::{
    ActionMap, ActionValue, Binding, ButtonState, GLOBAL_CONTEXT, PadAxis, PadButton,
};
use crcbl_ui::{DebugModule, DebugSection};

/// The section's title.
pub const INPUT_SECTION: &str = "input";

/// The label of the row naming the kind of device that last spoke.
pub const LAST_DEVICE_ROW: &str = "last device";

/// The label of the row listing the active contexts, top first.
pub const CONTEXTS_ROW: &str = "contexts";

/// The label of the row standing in for the trace while it is off or empty.
pub const TRACE_ROW: &str = "trace";

/// One [`ActionMap`] as a debug section: its pads, the raw keys, buttons and
/// axes, the context stack top first, each action's resolved value, the last
/// device, and the resolution trace newest first — see the
/// [module docs](self).
#[derive(Clone, Copy, Debug)]
pub struct InputInspector<'a> {
    map: &'a ActionMap,
}

impl<'a> InputInspector<'a> {
    /// The section for `map`.
    #[must_use]
    pub const fn new(map: &'a ActionMap) -> Self {
        Self { map }
    }
}

impl DebugModule for InputInspector<'_> {
    fn debug_section(&self, out: &mut DebugSection) {
        let map = self.map;
        out.set_title(INPUT_SECTION);

        match (map.last_device(), map.last_pad_kind()) {
            (None, _) => out.row_str(LAST_DEVICE_ROW, "none yet"),
            (Some(device), Some(pad)) if device == crate::input::Device::Gamepad => {
                out.row(LAST_DEVICE_ROW, format_args!("{device:?} ({pad:?})"));
            }
            (Some(device), _) => out.row(LAST_DEVICE_ROW, format_args!("{device:?}")),
        }

        let mut pads = 0;
        let mut label = String::new();
        for (id, pad) in map.gamepads() {
            pads += 1;
            label.clear();
            // Writing into a `String` cannot fail; the `Result` is `fmt`'s shape.
            let _ = write!(label, "pad {}", id.0);
            out.row(&label, format_args!("{:?}: {}", pad.kind, PadReading(pad)));
        }
        if pads == 0 {
            out.row_str("pads", "none");
        }

        out.row(
            "keys",
            format_args!("{}", Listed(map.held_keys().iter().map(|key| key.as_str()))),
        );
        let buttons = map.held_mouse_buttons();
        let buttons = buttons.iter().map(|&button| Binding::MouseButton(button));
        match map.pointer() {
            Some((x, y)) => out.row(
                "mouse",
                format_args!("{} at ({x:.2}, {y:.2})", Listed(buttons)),
            ),
            None => out.row("mouse", format_args!("{}", Listed(buttons))),
        }

        // Top first: the global context routes before the whole stack.
        let stack: Vec<&str> = map.active_contexts().collect();
        let mut contexts = String::from(GLOBAL_CONTEXT);
        for context in stack.iter().rev() {
            let _ = write!(contexts, " > {context}");
            if map.is_context_modal(context) {
                contexts.push_str(" (modal)");
            }
        }
        out.row_str(CONTEXTS_ROW, &contexts);

        for name in map.action_names() {
            let enabled = map.is_enabled(name).unwrap_or(false);
            let context = map.context_of(name).unwrap_or(GLOBAL_CONTEXT);
            if !enabled {
                out.row_str(name, "disabled");
            } else if !map.is_context_active(context) {
                out.row(name, format_args!("idle: {context} is off the stack"));
            } else if let Some(value) = map.action(name) {
                out.row(name, format_args!("{}", Value(value)));
            }
        }

        if !map.is_tracing() {
            out.row_str(TRACE_ROW, "off");
        } else if map.trace().next().is_none() {
            out.row_str(TRACE_ROW, "nothing pressed yet");
        }
        for entry in map.trace().rev() {
            label.clear();
            let _ = write!(label, "{}", entry.input);
            if entry.count > 1 {
                out.row(&label, format_args!("{} (x{})", entry.outcome, entry.count));
            } else {
                out.row(&label, format_args!("{}", entry.outcome));
            }
        }
    }
}

/// A list of things, space-separated, or `none`.
struct Listed<I>(I);

impl<I> fmt::Display for Listed<I>
where
    I: Iterator + Clone,
    I::Item: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut items = self.0.clone().peekable();
        if items.peek().is_none() {
            return f.write_str("none");
        }
        for (index, item) in items.enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}

/// One pad's raw state: the buttons it holds, then both sticks and both
/// triggers as the backend reported them, before any dead zone.
struct PadReading<'a>(&'a crate::input::GamepadSnapshot);

impl fmt::Display for PadReading<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pad = self.0;
        let held = PadButton::ALL
            .into_iter()
            .filter(|&button| pad.buttons.contains(button))
            .map(Binding::PadButton);
        write!(
            f,
            "{} L ({:.2}, {:.2}) R ({:.2}, {:.2}) LT {:.2} RT {:.2}",
            Listed(held),
            pad.axis(PadAxis::LeftX),
            pad.axis(PadAxis::LeftY),
            pad.axis(PadAxis::RightX),
            pad.axis(PadAxis::RightY),
            pad.axis(PadAxis::LeftTrigger),
            pad.axis(PadAxis::RightTrigger),
        )
    }
}

/// An action's resolved value: a button's state and how long it has been
/// held, an axis's value.
struct Value<'a>(&'a ActionValue);

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ActionValue::Button(button) => match button.state {
                ButtonState::Pressed => f.write_str("pressed"),
                ButtonState::Held { duration } => write!(f, "held {duration:.2} s"),
                ButtonState::Released => f.write_str("released"),
            },
            ActionValue::Axis1(axis) => write!(f, "{:.2}", axis.value),
            ActionValue::Axis2(axis) => write!(f, "({:.2}, {:.2})", axis.x, axis.y),
        }
    }
}

#[cfg(test)]
mod tests;
