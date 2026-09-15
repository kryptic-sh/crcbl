//! The engage widgets that edit a number: a slider and a drag-value.
//!
//! Both engage under the LOCKED rule — see the widgets module docs — and take
//! their value as `&mut f32`. While engaged, a step right adds `step` and a
//! step left takes it away; every other step the engaged widget captures
//! does nothing, so up and down neither adjust nor move focus. Back restores
//! the value the widget engaged with, through [`Ui::snapshot`]'s contract.
//!
//! The pointer adjusts without engaging. A slider's value follows the pointer
//! across its content box from the frame the press lands; a drag-value's
//! moves only once the press is a drag, by `speed` per pixel the pointer has
//! moved from where the press began.

use std::ops::RangeInclusive;
use std::panic::Location;

use super::{Ui, WidgetState, clamp_to, typed};
use crate::style::{Declaration, PseudoClasses};
use crate::tree::{Behavior, Direction, LengthAuto, NavStep, Response};

/// The most decimals a drag-value shows.
const MAX_DECIMALS: usize = 6;

/// How far a scaled step may sit from a whole number, as a share of it, and
/// still be that number: what absorbs the binary rounding of a step like
/// `0.1`, which ten times is not exactly one.
const DECIMAL_TOLERANCE: f32 = 1e-4;

/// The sign a captured step adjusts by: `+1` for right, `-1` for left, and
/// nothing for any other step.
fn horizontal(step: Option<NavStep>) -> Option<f32> {
    match step {
        Some(NavStep::Move(Direction::Right)) => Some(1.0),
        Some(NavStep::Move(Direction::Left)) => Some(-1.0),
        _ => None,
    }
}

/// `value` on the nearest multiple of `step` from `min`, inside the range.
/// A `step` that is not positive snaps nothing.
fn snap(value: f32, min: f32, max: f32, step: f32) -> f32 {
    let snapped = if step > 0.0 {
        min + ((value - min) / step).round() * step
    } else {
        value
    };
    clamp_to(snapped, min, max)
}

/// How many decimals show every multiple of `step`: multiplied by ten until it
/// is a whole number other than zero, at most [`MAX_DECIMALS`] times; none for
/// a step that is zero or not finite. Multiplication and rounding only, so the
/// count does not depend on a platform's libm.
pub(super) fn decimals(step: f32) -> usize {
    let mut scaled = step.abs();
    if scaled == 0.0 || !scaled.is_finite() {
        return 0;
    }
    let mut count = 0;
    while count < MAX_DECIMALS {
        let whole = scaled.round();
        if whole != 0.0 && (scaled - whole).abs() <= DECIMAL_TOLERANCE * whole {
            break;
        }
        scaled *= 10.0;
        count += 1;
    }
    count
}

impl Ui {
    /// A slider editing `value` inside `range` in multiples of `step`: a
    /// `slider` block holding a `.slider-fill` block as wide as the value's
    /// share of the range. [`Response::changed`] is the frame the value moved
    /// — by the pointer, a step, or a cancel.
    ///
    /// A range whose start is past its end holds the value at its end.
    #[track_caller]
    pub fn slider(
        &mut self,
        selector: &str,
        value: &mut f32,
        range: RangeInclusive<f32>,
        step: f32,
    ) -> Response {
        let (min, max) = (*range.start(), *range.end());
        let before = value.to_bits();
        let selector = typed("slider", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let interaction = self.interaction_of(key);
        self.snapshot_for(key, interaction.engagement, value);

        if interaction.pressed
            && !self.building_disabled()
            && let Some(node) = self.store.by_key(key)
        {
            let (start, end) = node.content_box();
            let width = end.x - start.x;
            if width > 0.0 {
                let share = clamp_to((self.pointer.pos.x - start.x) / width, 0.0, 1.0);
                *value = snap(min + share * (max - min), min, max, step);
            }
        }
        if let Some(sign) = horizontal(interaction.captured) {
            *value = snap(*value + sign * step, min, max, step);
        }

        let share = if max > min {
            clamp_to((*value - min) / (max - min), 0.0, 1.0)
        } else {
            0.0
        };
        let fill = [Declaration::Width(LengthAuto::Percent(share))];
        let mut response = self.open_block(
            key,
            parsed,
            &[],
            Behavior::ENGAGE,
            PseudoClasses::NONE,
            |ui| {
                ui.block(".slider-fill", &fill, |_| {});
            },
        );
        response.changed = value.to_bits() != before;
        response
    }

    /// A drag-value editing `value` inside `range`: a `drag-value` block
    /// holding its value as a `.drag-value-text` span, shown with as many
    /// decimals as `step` has. A drag moves it by `speed` per pixel, measured
    /// from where the press began once it passed
    /// [`crate::tree::DRAG_THRESHOLD`], and an engaged step by `step`; it is
    /// held inside `range` either way, not snapped. [`Response::changed`] is
    /// the frame the value moved.
    #[track_caller]
    pub fn drag_value(
        &mut self,
        selector: &str,
        value: &mut f32,
        range: RangeInclusive<f32>,
        speed: f32,
        step: f32,
    ) -> Response {
        let (min, max) = (*range.start(), *range.end());
        let before = value.to_bits();
        let selector = typed("drag-value", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let interaction = self.interaction_of(key);
        self.snapshot_for(key, interaction.engagement, value);

        let held = match self.widget_state(key) {
            WidgetState::Anchor(anchor) => anchor,
            _ => None,
        };
        let anchor = if interaction.pressed && !self.building_disabled() {
            let anchor = held.unwrap_or(*value);
            if self.dragged {
                let moved = self.pointer.pos.x - self.press_origin.x;
                *value = clamp_to(anchor + moved * speed, min, max);
            }
            Some(anchor)
        } else {
            None
        };
        if let Some(sign) = horizontal(interaction.captured) {
            *value = clamp_to(*value + sign * step, min, max);
        }

        let text = format!("{:.*}", decimals(step), *value);
        let mut response = self.open_block(
            key,
            parsed,
            &[],
            Behavior::ENGAGE,
            PseudoClasses::NONE,
            |ui| {
                ui.span(".drag-value-text", text.as_str(), &[]);
            },
        );
        self.set_widget_state(key, WidgetState::Anchor(anchor));
        response.changed = value.to_bits() != before;
        response
    }
}
