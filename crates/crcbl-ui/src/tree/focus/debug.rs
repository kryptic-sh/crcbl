//! The navigation debug overlay: the focus path, and every candidate the
//! frame's directional move scored with its score — why focus went where it
//! went, drawn through the same draw list as the tree.
//!
//! Off by default; [`Ui::set_nav_debug`] switches it on, and [`Ui::emit`] then
//! draws it after the tree:
//!
//! * **the focus path** — a one-pixel outline in [`NAV_DEBUG_PATH`] round the
//!   focused node and every node it was built under, and the selectors along
//!   that path, joined by `>`, at the top-left of its root;
//! * **each scored candidate** — an outline in [`NAV_DEBUG_CHOSEN`] for the one
//!   the move landed on, [`NAV_DEBUG_BEAM`] for the others in the beam and
//!   [`NAV_DEBUG_OUTSIDE`] for the rest, with its distance written at its
//!   top-left, `b` before it for a candidate in the beam.
//!
//! Rectangles are this frame's, so a candidate the move scrolled is outlined
//! where it is drawn.

use crate::draw_list::DrawList;
use crate::tree::Ui;
use crate::widget::NATURAL_FONT_SIZE;

/// The focus path's outlines and text: magenta.
pub const NAV_DEBUG_PATH: [f32; 4] = [1.0, 0.0, 1.0, 1.0];

/// The candidate the move landed on: green.
pub const NAV_DEBUG_CHOSEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

/// A candidate in the beam that lost: yellow.
pub const NAV_DEBUG_BEAM: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

/// A candidate outside the beam: grey.
pub const NAV_DEBUG_OUTSIDE: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

/// How far in from a rectangle's top-left its label is written, in pixels.
const LABEL_INSET: f32 = 2.0;

impl Ui {
    /// Switches the navigation debug overlay on or off; see
    /// `tree/focus/debug.rs`.
    pub fn set_nav_debug(&mut self, on: bool) {
        self.focus.debug = on;
    }

    /// Whether the navigation debug overlay is on.
    #[must_use]
    pub fn nav_debug(&self) -> bool {
        self.focus.debug
    }

    /// Draws the overlay into `list`, if it is on.
    pub(crate) fn emit_nav_debug(&self, list: &mut DrawList) {
        if !self.focus.debug {
            return;
        }
        let rect = |key| self.store.by_key(key).map(|node| node.rect);
        let inset = glam::Vec2::splat(LABEL_INSET);

        if let Some(focused) = self.focus.focused
            && let Some(mut at) = self.nodes.iter().position(|node| node.key == focused)
        {
            let mut path = Vec::new();
            loop {
                let node = &self.nodes[at];
                let (min, max) = self.store.get(node.slot).rect;
                list.rect_outline(min, max, 1.0, NAV_DEBUG_PATH);
                let (start, end) = node.selector;
                path.push(&self.selectors[start..end]);
                match node.parent {
                    Some(parent) => at = parent,
                    None => break,
                }
            }
            let root = self.store.get(self.nodes[at].slot).rect.0;
            path.retain(|selector| !selector.is_empty());
            path.reverse();
            list.text(
                root + inset,
                path.join(" > "),
                NAV_DEBUG_PATH,
                NATURAL_FONT_SIZE,
            );
        }

        for scored in &self.focus.scores {
            let Some((min, max)) = rect(scored.key) else {
                continue;
            };
            let colour = if scored.chosen {
                NAV_DEBUG_CHOSEN
            } else if scored.score.in_beam {
                NAV_DEBUG_BEAM
            } else {
                NAV_DEBUG_OUTSIDE
            };
            list.rect_outline(min, max, 1.0, colour);
            let beam = if scored.score.in_beam { "b" } else { "" };
            list.text(
                min + inset,
                format!("{beam}{:.0}", scored.score.distance),
                colour,
                NATURAL_FONT_SIZE,
            );
        }
    }
}
