//! The engage widgets that edit a number: a slider and a drag-value.
//!
//! Both engage under the LOCKED rule — see the widgets module docs. A slider
//! takes its value as `&mut f32`, and a drag-value as `&mut` any
//! [`DragNumber`] — an `f64`, an `i64` or a `u64`, each moved in its own
//! arithmetic. While engaged, a step right adds `step` and a step left takes
//! it away; every other step the engaged widget captures does nothing, so up
//! and down neither adjust nor move focus. Back restores the value the widget
//! engaged with, through [`Ui::snapshot`]'s contract.
//!
//! The pointer adjusts without engaging. A slider's value follows the pointer
//! across its content box from the frame the press lands; a drag-value's
//! moves only once the press is a drag, by `speed` per pixel the pointer has
//! moved from where the press began.
//!
//! A drag-value can also be typed into, opened by a double-click or by accept
//! while it is focused; `number_entry.rs` has that mode.

use std::ops::RangeInclusive;
use std::panic::Location;

use self::sealed::Sealed;
use super::number_entry::NumberEntry;
use super::{Ui, WidgetState, clamp_to, typed};
use crate::style::{Declaration, PseudoClasses};
use crate::tree::{Behavior, Direction, Engagement, LengthAuto, NavStep, Response};

/// The most decimals a drag-value shows.
const MAX_DECIMALS: usize = 6;

/// How far a scaled step may sit from a whole number, as a share of it, and
/// still be that number: what absorbs the binary rounding of a step like
/// `0.1`, which ten times is not exactly one.
const DECIMAL_TOLERANCE: f64 = 1e-4;

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
pub(super) fn decimals(step: f64) -> usize {
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

/// A number [`Ui::drag_value`] edits: an `f64`, an `i64` or a `u64`, the three
/// a reflected [`Value`](crcbl_reflect::Value) widens to.
///
/// Each moves in its own arithmetic, so a drag keeps every digit its type
/// holds: an `f64` is never rounded through an `f32`, and a whole number moves
/// by whole numbers — one at a time at any magnitude — and saturates at its
/// type's ends rather than wrapping. A caller holding an `f32` widens it with
/// `f64::from` and narrows the result back itself, so the one rounding there
/// is happens where that caller can see it.
///
/// Sealed: the arithmetic is the widget's own, and these three are the kinds
/// a [`Value`](crcbl_reflect::Value) has.
pub trait DragNumber: Sealed {}

impl DragNumber for f64 {}
impl DragNumber for i64 {}
impl DragNumber for u64 {}

/// What a [`DragNumber`] does inside the widget, out of its callers' reach.
pub(super) mod sealed {
    /// A drag-value's value when its press began, in the number's own kind,
    /// so a whole number past what an `f64` holds exactly is held exactly.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum DragAnchor {
        /// An `f64`'s.
        Float(f64),
        /// An `i64`'s.
        Int(i64),
        /// A `u64`'s.
        UInt(u64),
    }

    /// [`super::DragNumber`]'s arithmetic.
    pub trait Sealed: Copy + Send + Sync + 'static {
        /// `self` moved by `by`: a whole number by `by` rounded to the nearest
        /// whole, saturating at the type's ends.
        fn offset(self, by: f64) -> Self;
        /// `self` moved one `step` forward, or back, saturating.
        fn stepped(self, step: Self, forward: bool) -> Self;
        /// `self` held inside `min..=max` without panicking: a NaN is `min`,
        /// and a range whose start is past its end holds every value at its
        /// end.
        fn clamped(self, min: Self, max: Self) -> Self;
        /// `self` as its drag-value shows it: with as many decimals as `step`
        /// has, and a whole number whole.
        fn text(self, step: Self) -> String;
        /// `self` with every digit it holds, as typing into its drag-value
        /// starts from: text that [`parse_text`](Self::parse_text) reads back
        /// as `self`, bit for bit.
        fn exact_text(self) -> String;
        /// The number `text` spells in this kind, read straight to it — a
        /// float never through an `f32` — with white space around it
        /// ignored; `None` for text that spells no number of this kind: a
        /// fraction or an exponent for a whole number, one past the type's
        /// ends, and a float that is not finite.
        fn parse_text(text: &str) -> Option<Self>;
        /// Whether `self` is `other` — bit for bit for a float, so a move from
        /// `0.0` to `-0.0` is a change.
        fn same(self, other: Self) -> bool;
        /// `self` as the anchor a press keeps.
        fn anchor(self) -> DragAnchor;
        /// The number `anchor` holds, if it is of this kind.
        fn from_anchor(anchor: DragAnchor) -> Option<Self>;
    }

    impl Sealed for f64 {
        fn offset(self, by: f64) -> Self {
            self + by
        }

        fn stepped(self, step: Self, forward: bool) -> Self {
            if forward { self + step } else { self - step }
        }

        fn clamped(self, min: Self, max: Self) -> Self {
            self.max(min).min(max)
        }

        fn text(self, step: Self) -> String {
            format!("{:.*}", super::decimals(step), self)
        }

        fn exact_text(self) -> String {
            // `Display` with no precision writes the fewest digits that read
            // back to the same `f64`, so the text round-trips exactly.
            self.to_string()
        }

        fn parse_text(text: &str) -> Option<Self> {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
        }

        fn same(self, other: Self) -> bool {
            self.to_bits() == other.to_bits()
        }

        fn anchor(self) -> DragAnchor {
            DragAnchor::Float(self)
        }

        fn from_anchor(anchor: DragAnchor) -> Option<Self> {
            match anchor {
                DragAnchor::Float(value) => Some(value),
                _ => None,
            }
        }
    }

    /// [`Sealed`] for a whole number, which adds its rounded offset's
    /// magnitude with `$add` and takes it away with `$sub` — both saturating,
    /// and both taking a `u64`, so an offset as wide as the type's whole span
    /// still lands on its end.
    macro_rules! whole_number {
        ($type:ty, $variant:ident, $add:ident, $sub:ident) => {
            impl Sealed for $type {
                fn offset(self, by: f64) -> Self {
                    // A float-to-integer `as` saturates and takes a NaN to
                    // zero, so no offset can wrap or panic.
                    let whole = by.round();
                    if whole >= 0.0 {
                        self.$add(whole as u64)
                    } else {
                        self.$sub((-whole) as u64)
                    }
                }

                fn stepped(self, step: Self, forward: bool) -> Self {
                    if forward {
                        self.saturating_add(step)
                    } else {
                        self.saturating_sub(step)
                    }
                }

                fn clamped(self, min: Self, max: Self) -> Self {
                    self.max(min).min(max)
                }

                fn text(self, _step: Self) -> String {
                    self.to_string()
                }

                fn exact_text(self) -> String {
                    self.to_string()
                }

                fn parse_text(text: &str) -> Option<Self> {
                    text.trim().parse::<$type>().ok()
                }

                fn same(self, other: Self) -> bool {
                    self == other
                }

                fn anchor(self) -> DragAnchor {
                    DragAnchor::$variant(self)
                }

                fn from_anchor(anchor: DragAnchor) -> Option<Self> {
                    match anchor {
                        DragAnchor::$variant(value) => Some(value),
                        _ => None,
                    }
                }
            }
        };
    }

    whole_number!(i64, Int, saturating_add_unsigned, saturating_sub_unsigned);
    whole_number!(u64, UInt, saturating_add, saturating_sub);
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
    /// decimals as `step` has — a whole number with none. A drag moves it by
    /// `speed` per pixel, measured from where the press began once it passed
    /// [`crate::tree::DRAG_THRESHOLD`], and an engaged step by `step`; it is
    /// held inside `range` either way, not snapped. A whole number moves by
    /// the drag's distance rounded to the nearest whole — so by one at any
    /// magnitude — and saturates at its type's ends; [`DragNumber`] has the
    /// rest. [`Response::changed`] is the frame the value moved.
    ///
    /// A double-click, or accept while it is focused, opens it for typing: it
    /// becomes a text input holding every digit of its value, all selected,
    /// and accept or a click elsewhere puts in the number typed — see
    /// `number_entry.rs`.
    #[track_caller]
    pub fn drag_value<N: DragNumber>(
        &mut self,
        selector: &str,
        value: &mut N,
        range: RangeInclusive<N>,
        speed: f64,
        step: N,
    ) -> Response {
        let (min, max) = (*range.start(), *range.end());
        let before = *value;
        let selector = typed("drag-value", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let interaction = self.interaction_of(key);
        self.snapshot_for(key, interaction.engagement, value);
        let entry = NumberEntry {
            key,
            selector: parsed,
            interaction,
            min,
            max,
            step,
        };

        let (held, last_press) = match self.widget_state(key) {
            WidgetState::Drag { anchor, last_press } => {
                (anchor.and_then(N::from_anchor), last_press)
            }
            WidgetState::TypedNumber(original) => {
                if let Some(original) = N::from_anchor(original)
                    && let Some(mut response) = self.typed_number(entry, value, original)
                {
                    response.changed = !value.same(before);
                    return response;
                }
                (None, None)
            }
            _ => (None, None),
        };

        let pressed = interaction.pressed && !self.building_disabled();
        let starts = pressed && held.is_none();
        let double = starts && self.second_press(last_press);
        // Engaged by accept, not by a click: the keyboard's way in to typing.
        let accepted = interaction.engagement == Engagement::Began
            && self.clicked_key().is_none()
            && !self.building_disabled();
        if double || accepted {
            let mut response = self.begin_typing(entry, value, double);
            response.changed = !value.same(before);
            return response;
        }

        let last_press = if starts {
            Some((self.text_clock, self.pointer.pos))
        } else if pressed && self.dragged {
            // A drag is not a click, so no press after it is a double-click.
            None
        } else {
            last_press
        };
        let anchor = if pressed {
            let anchor = held.unwrap_or(*value);
            if self.dragged {
                let moved = f64::from(self.pointer.pos.x - self.press_origin.x);
                *value = anchor.offset(moved * speed).clamped(min, max);
            }
            Some(anchor)
        } else {
            None
        };
        if let Some(sign) = horizontal(interaction.captured) {
            *value = value.stepped(step, sign > 0.0).clamped(min, max);
        }

        let text = value.text(step);
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
        // A captured press still reports pressed on the frame it is released,
        // so only a button still down carries the anchor into the next frame —
        // and a press after it is one that starts.
        let anchor = anchor.filter(|_| self.pointer.down);
        self.set_widget_state(
            key,
            WidgetState::Drag {
                anchor: anchor.map(N::anchor),
                last_press,
            },
        );
        response.changed = !value.same(before);
        response
    }
}
