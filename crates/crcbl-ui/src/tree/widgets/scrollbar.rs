//! A vertical scrollbar: a track, and a handle on it that shows where a scroll
//! container's view sits in its content and that a drag moves.
//!
//! The track is a `scrollbar` block the stylesheet sizes and places — beside
//! the container in a row, or absolutely over its edge — and the handle a
//! `.scroll-handle` block inside it, absolutely positioned: as long as the
//! view's share of the content, never shorter than [`MIN_SCROLL_HANDLE`], and
//! as far down the track's spare length as the offset is down its reach. Both
//! the container's numbers and the track's length are read from last frame's
//! layout, as every scroll offset is, so the first frame a scrollbar is built
//! it has no handle. Nor does a container with nothing to scroll.
//!
//! Pressing the handle and moving the pointer scrolls the container by the
//! same share of its reach: the handle stays under the pointer. Where the
//! offset was when the press began is kept on the handle, as a divider keeps
//! its position (`split.rs`).

use std::panic::Location;

use glam::Vec2;

use super::{Ui, WidgetKind, WidgetState, typed};
use crate::style::{Declaration, PseudoClasses, Sides};
use crate::tree::{Behavior, LengthAuto, NodeKey, Position, Response};

/// The shortest a handle is drawn, in pixels, so that a long list's handle is
/// still something to take hold of.
pub const MIN_SCROLL_HANDLE: f32 = 16.0;

impl Ui {
    /// A vertical scrollbar for the scroll container `target`, a `scrollbar`
    /// block holding a `.scroll-handle` block; see the module docs. Returns the
    /// handle's [`Response`] — the track's when there is no handle — with
    /// [`Response::changed`] set the frame a drag scrolled the container.
    #[track_caller]
    pub fn scrollbar(&mut self, selector: &str, target: NodeKey) -> Response {
        let selector = typed("scrollbar", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let mut handle = None;
        let track = self.open_block(
            key,
            parsed,
            &[],
            Behavior::NONE,
            PseudoClasses::NONE,
            |ui| handle = ui.scroll_handle(key, target),
        );
        self.mark(key, WidgetKind::ScrollBar);
        handle.unwrap_or(track)
    }

    /// The handle inside the open track `track`, if `target` has anything to
    /// scroll and the track has been laid out.
    fn scroll_handle(&mut self, track: NodeKey, target: NodeKey) -> Option<Response> {
        let length = self.store.by_key(track).map(|node| {
            let (top, bottom) = node.content_box();
            bottom.y - top.y
        })?;
        let (view, offset, reach) = self.store.by_key(target).map(|node| {
            let (top, bottom) = node.content_box();
            (bottom.y - top.y, node.scroll_offset, node.scroll_max.y)
        })?;
        if reach <= 0.0 || length <= 0.0 {
            return None;
        }
        let handle_length = (length * view / (view + reach))
            .max(MIN_SCROLL_HANDLE)
            .min(length);
        let travel = length - handle_length;

        let selector = self.node_selector(".scroll-handle");
        let handle_key = self.widget_key(selector, Location::caller());
        let interaction = self.interaction_of(handle_key);
        let anchored = match self.widget_state(handle_key) {
            WidgetState::Anchor(anchor) => anchor,
            _ => None,
        };
        let mut scrolled = offset.y;
        let anchor = if interaction.pressed && !self.building_disabled() {
            let anchor = anchored.unwrap_or(offset.y);
            if travel > 0.0 {
                let moved = (self.pointer.pos.y - self.press_origin.y) * reach / travel;
                scrolled = (anchor + moved).clamp(0.0, reach);
            }
            Some(anchor)
        } else {
            None
        };
        let changed = scrolled.to_bits() != offset.y.to_bits();
        if changed {
            self.set_scroll_offset_of(target, Vec2::new(offset.x, scrolled));
        }

        let inline = [
            Declaration::Position(Position::Absolute),
            Declaration::Inset(Sides::Top, LengthAuto::Px(travel * scrolled / reach)),
            Declaration::Inset(Sides::Left, LengthAuto::Px(0.0)),
            Declaration::Inset(Sides::Right, LengthAuto::Px(0.0)),
            Declaration::Height(LengthAuto::Px(handle_length)),
        ];
        let mut response = self.open_block(
            handle_key,
            selector,
            &inline,
            Behavior::NONE,
            PseudoClasses::NONE,
            |_| {},
        );
        self.set_widget_state(handle_key, WidgetState::Anchor(anchor));
        response.changed = changed;
        Some(response)
    }
}
