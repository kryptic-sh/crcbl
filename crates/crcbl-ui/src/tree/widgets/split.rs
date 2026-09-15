//! A split pane: two panes and a divider between them that a drag or an
//! engaged step moves.
//!
//! The divider's position is the first pane's length along the split, kept in
//! the store. Until anything moves it there is none, and both panes share the
//! space equally. Each pane has its minimum length as an inline `min-width`
//! (or `min-height`), so flex layout holds them to it at any size, and a
//! position is clamped so the second pane keeps its minimum — **the first
//! pane's minimum wins** when the split is too short for both.

use std::panic::Location;

use glam::Vec2;

use super::{Ui, WidgetState, typed};
use crate::style::{Declaration, PseudoClasses};
use crate::tree::{Behavior, Direction, FlexDirection, LengthAuto, NavStep, Response};

/// How far one engaged step moves a divider, in pixels.
pub const SPLIT_NAV_STEP: f32 = 8.0;

/// Which way a split lays its panes out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplitAxis {
    /// Side by side, the divider upright between them: left and right move
    /// it.
    #[default]
    Row,
    /// One above the other, the divider across: up and down move it.
    Column,
}

impl SplitAxis {
    /// `vector`'s component along the split.
    const fn along(self, vector: Vec2) -> f32 {
        match self {
            Self::Row => vector.x,
            Self::Column => vector.y,
        }
    }

    /// The sign a captured step moves the divider by, if it moves it at all.
    const fn step(self, step: Option<NavStep>) -> Option<f32> {
        match (self, step) {
            (Self::Row, Some(NavStep::Move(Direction::Right)))
            | (Self::Column, Some(NavStep::Move(Direction::Down))) => Some(1.0),
            (Self::Row, Some(NavStep::Move(Direction::Left)))
            | (Self::Column, Some(NavStep::Move(Direction::Up))) => Some(-1.0),
            _ => None,
        }
    }

    /// A length along the split, as a size declaration.
    const fn size(self, length: LengthAuto) -> Declaration {
        match self {
            Self::Row => Declaration::Width(length),
            Self::Column => Declaration::Height(length),
        }
    }

    /// A minimum length along the split, as a declaration.
    const fn min_size(self, length: f32) -> Declaration {
        match self {
            Self::Row => Declaration::MinWidth(LengthAuto::Px(length)),
            Self::Column => Declaration::MinHeight(LengthAuto::Px(length)),
        }
    }
}

impl Ui {
    /// A split pane: a `split` block laid out along `axis`, holding a
    /// `.split-pane.split-first` block `first` builds, a `.split-divider`
    /// block — with `.split-row` or `.split-column` for its axis, which the
    /// stylesheet sizes it by — and a `.split-pane.split-second` block `second`
    /// builds. `min` is each pane's minimum length along the split, in pixels.
    ///
    /// Pressing the divider drags it; a click or accept engages it, and then a
    /// step along the axis moves it by [`SPLIT_NAV_STEP`] and back puts it
    /// where it was. Returns the divider's [`Response`], with
    /// [`Response::changed`] set the frame it moved.
    #[track_caller]
    pub fn split(
        &mut self,
        selector: &str,
        axis: SplitAxis,
        min: [f32; 2],
        first: impl FnOnce(&mut Self),
        second: impl FnOnce(&mut Self),
    ) -> Response {
        let selector = typed("split", selector);
        let direction = match axis {
            SplitAxis::Row => FlexDirection::Row,
            SplitAxis::Column => FlexDirection::Column,
        };
        let mut divider = None;
        let response = self.block_with(
            &selector,
            &[Declaration::FlexDirection(direction)],
            Behavior::NONE,
            |ui| divider = Some(ui.split_panes(axis, min, first, second)),
        );
        let (divider, position) = divider.expect("the panes are built inside the block");
        self.set_widget_state(response.key, WidgetState::Split(position));
        divider
    }

    /// The inside of an open split block: resolves the divider's position,
    /// builds both panes and the divider, and returns the divider's response
    /// with the position to keep.
    fn split_panes(
        &mut self,
        axis: SplitAxis,
        min: [f32; 2],
        first: impl FnOnce(&mut Self),
        second: impl FnOnce(&mut Self),
    ) -> (Response, Option<f32>) {
        let min = min.map(|length| length.max(0.0));
        let split = &self.nodes[*self.open.last().expect("called inside the split block")];
        let fresh = split.fresh;
        let stored = self.store.get(split.slot);
        let room = (!fresh).then(|| axis.along(stored.content_box().1 - stored.content_box().0));
        let held = match stored.widget {
            WidgetState::Split(position) => position,
            _ => None,
        };

        let first_selector = self.node_selector(".split-pane.split-first");
        let first_key = self.widget_key(first_selector, Location::caller());
        let divider_selector = self.node_selector(match axis {
            SplitAxis::Row => ".split-divider.split-row",
            SplitAxis::Column => ".split-divider.split-column",
        });
        let divider_key = self.widget_key(divider_selector, Location::caller());
        let second_selector = self.node_selector(".split-pane.split-second");
        let second_key = self.widget_key(second_selector, Location::caller());

        // Where the divider is: the position kept, else where last frame's
        // equal share put the first pane.
        let laid_out = self
            .store
            .by_key(first_key)
            .map(|pane| axis.along(pane.rect.1 - pane.rect.0));
        let was = held.or(laid_out);
        let interaction = self.interaction_of(divider_key);
        let mut position = held;
        self.snapshot_for(divider_key, interaction.engagement, &mut position);
        let current = position.or(laid_out);
        let anchored = match self.widget_state(divider_key) {
            WidgetState::Anchor(anchor) => anchor,
            _ => None,
        };
        let anchor = if interaction.pressed && !self.building_disabled() {
            let anchor = anchored.or(current).unwrap_or(min[0]);
            position = Some(anchor + axis.along(self.pointer.pos - self.press_origin));
            Some(anchor)
        } else {
            None
        };
        if let Some(sign) = axis.step(interaction.captured) {
            position = Some(current.unwrap_or(min[0]) + sign * SPLIT_NAV_STEP);
        }
        let divider_length = self
            .store
            .by_key(divider_key)
            .map_or(0.0, |node| axis.along(node.rect.1 - node.rect.0));
        let most = room.map_or(f32::INFINITY, |room| room - divider_length - min[1]);
        let position = position.map(|position| position.min(most).max(min[0]));

        let first_inline = match position {
            Some(length) => [
                axis.size(LengthAuto::Px(length)),
                Declaration::FlexGrow(0.0),
                Declaration::FlexShrink(0.0),
                axis.min_size(min[0]),
            ],
            None => [
                Declaration::FlexBasis(LengthAuto::Px(0.0)),
                Declaration::FlexGrow(1.0),
                Declaration::FlexShrink(1.0),
                axis.min_size(min[0]),
            ],
        };
        let second_inline = [
            Declaration::FlexBasis(LengthAuto::Px(0.0)),
            Declaration::FlexGrow(1.0),
            Declaration::FlexShrink(1.0),
            axis.min_size(min[1]),
        ];
        let none = PseudoClasses::NONE;
        self.open_block(
            first_key,
            first_selector,
            &first_inline,
            Behavior::NONE,
            none,
            first,
        );
        let mut divider = self.open_block(
            divider_key,
            divider_selector,
            &[Declaration::FlexShrink(0.0)],
            Behavior::ENGAGE,
            none,
            |_| {},
        );
        self.open_block(
            second_key,
            second_selector,
            &second_inline,
            Behavior::NONE,
            none,
            second,
        );
        self.set_widget_state(divider_key, WidgetState::Anchor(anchor));
        divider.changed = position.is_some() && position.map(f32::to_bits) != was.map(f32::to_bits);
        (divider, position)
    }
}
