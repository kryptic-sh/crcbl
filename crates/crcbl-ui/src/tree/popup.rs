//! The pop-up layer: a block anchored to a node, drawn over the whole tree.
//!
//! # Opening one
//!
//! Any widget opens a pop-up by naming the node it hangs from:
//! [`Ui::open_popup`] with that node's key, then — that frame and every frame
//! it stays open — [`Ui::popup`] with the same key and a closure that builds
//! what it holds. [`Ui::popup`] builds nothing and answers `None` while the
//! pop-up is closed, so the call can sit in a widget unconditionally. The
//! open pop-ups are a stack: one opened while another is open (from a node
//! inside it, as a submenu would be) goes on top.
//!
//! **An open pop-up the frame did not build is closed at [`Ui::layout`]**, and
//! so is one whose anchor the frame did not build, with every pop-up above
//! either. A pop-up is the tree's rule applied to a layer: what a frame does
//! not build, it does not keep.
//!
//! # Where it is drawn
//!
//! A pop-up is a **root of its own**: [`Ui::popup`] builds it outside every
//! block that is open, keyed by its anchor ([`Ui::popup_key`]), and laid out
//! in the space the tree was given. Its border box goes below its anchor,
//! lined up with the anchor's left edge; one that would run past the bottom of
//! the viewport where there is more room above flips to sit above the anchor,
//! and it is then shifted the least that keeps it inside the viewport on both
//! axes — its top-left corner wins when it is larger than the viewport.
//! The viewport is the space [`Ui::layout`] was given, from its origin; an
//! axis laid out under content-sized space bounds nothing. A pop-up's own
//! margin and offsets do not move it.
//!
//! **It is clipped to the viewport, and to nothing its anchor is under**: a
//! list hanging out of a scrolled, clipped panel draws whole over it. Each
//! open pop-up is a **layer**, drawn after every lower one — the tree is layer
//! 0 — with its outlines after it, so neither a sibling built later nor a
//! focus ring below shows through. A layer is hit-tested before every layer
//! under it, whatever their build order.
//!
//! # Closing one
//!
//! [`Ui::close_popup`] closes a pop-up and every pop-up above it. The layer
//! also closes itself:
//!
//! * **A press outside it closes it**, and every pop-up above the layer the
//!   press landed in. A press outside every pop-up is **spent closing them**:
//!   it is captured by no node, so nothing under it is pressed, hovered or
//!   clicked until it is released. This is the long-term rule, decided
//!   2026-10-03: a click meant to dismiss a list must not also fire whatever
//!   the list was covering, and a click on the anchor itself closes the list
//!   rather than closing and reopening it.
//! * **Back closes the topmost**, when nothing is engaged — an engaged widget
//!   inside the pop-up cancels first, as back always does — and the back is
//!   spent there: [`Ui::back_requested`] does not report it.
//!
//! # Focus
//!
//! A pop-up's root is a [`Scope::Modal`](super::Scope::Modal), and the topmost
//! modal by layer, so focus moves into it the frame after it is built — onto
//! the node [`Ui::set_focus`] asked for, else by the landing rule in
//! `focus/mod.rs` — and no move or tree-order step leaves it. Inside, the
//! frame's [`NavInput`](super::NavInput) moves focus as anywhere else.
//! **Closing a pop-up, by any of the ways above, gives focus back to its
//! anchor** when the next frame begins, or at once for back.

use glam::Vec2;

use super::store::NodeKey;
use super::widgets::typed;
use super::{Behavior, KeySource, ROOT_KEY, Response, Ui};
use crate::draw_list::ClipRect;
use crate::style::{Declaration, PseudoClasses};
use crate::widget::PointerInput;

/// The layer a node is given when it was built in a pop-up that closed
/// before layout: never hit, focused or drawn.
pub(super) const CLOSED_LAYER: usize = usize::MAX;

/// What a press spent closing pop-ups is captured by: a key no node has, so
/// nothing is pressed, hovered or clicked until the press is released.
const SPENT_PRESS: NodeKey = NodeKey(0x7370_656e_742d_7072);

impl Ui {
    /// Opens the pop-up hanging from `anchor` on top of every open one, for
    /// [`Ui::popup`] to build. Does nothing when it is already open.
    pub fn open_popup(&mut self, anchor: NodeKey) {
        if !self.popups.contains(&anchor) {
            self.popups.push(anchor);
        }
    }

    /// Closes the pop-up hanging from `anchor`, and every pop-up above it, and
    /// gives focus back to `anchor` when the next frame begins. Does nothing
    /// when it is not open.
    pub fn close_popup(&mut self, anchor: NodeKey) {
        if let Some(depth) = self.popups.iter().position(|&open| open == anchor) {
            self.close_popups_from(depth);
        }
    }

    /// Whether the pop-up hanging from `anchor` is open.
    #[must_use]
    pub fn is_popup_open(&self, anchor: NodeKey) -> bool {
        self.popups.contains(&anchor)
    }

    /// The key of the root [`Ui::popup`] builds for `anchor`, which is the
    /// same wherever the call is made: what a caller or a test walks a
    /// pop-up's nodes from with [`Ui::child_keys`].
    #[must_use]
    pub fn popup_key(anchor: NodeKey) -> NodeKey {
        Self::key_under(ROOT_KEY, KeySource::Popup(anchor.0))
    }

    /// The pop-up hanging from `anchor`, while it is open: a `popup` block —
    /// `selector` is its `#id.class`, as a widget's is, though an `#id` does
    /// not key it: its anchor does — whose children `build` adds, placed and
    /// drawn as the module docs say. `inline` overrides every rule, as
    /// [`Ui::block`]'s does.
    ///
    /// Returns the root's [`Response`], or `None` without building anything
    /// while the pop-up is closed.
    pub fn popup(
        &mut self,
        anchor: NodeKey,
        selector: &str,
        inline: &[Declaration],
        build: impl FnOnce(&mut Self),
    ) -> Option<Response> {
        if !self.is_popup_open(anchor) {
            return None;
        }
        let selector = typed("popup", selector);
        let parsed = self.node_selector(&selector);
        // Built outside every open block, so it is a root of its own and
        // nothing it hangs out of clips or scrolls it; a tree row's children
        // are not its.
        let open = std::mem::take(&mut self.open);
        let rows = std::mem::take(&mut self.tree_rows);
        let index = self.nodes.len();
        let response = self.open_block(
            Self::popup_key(anchor),
            parsed,
            inline,
            Behavior::MODAL,
            PseudoClasses::NONE,
            build,
        );
        self.open = open;
        self.tree_rows = rows;
        self.popup_roots.push((index, anchor));
        Some(response)
    }

    /// Closes every pop-up from `depth` up the stack: what last frame drew of
    /// them stops being hit, focused or trapping focus at once, rather than at
    /// the next layout, and focus goes back to the lowest one's anchor.
    fn close_popups_from(&mut self, depth: usize) {
        let Some(&anchor) = self.popups.get(depth) else {
            return;
        };
        self.popups.truncate(depth);
        for node in self.store.iter_mut() {
            if node.layer > depth {
                node.hittable = false;
            }
        }
        self.set_focus(anchor);
    }

    /// A press that began outside the topmost pop-up closes it, and every
    /// pop-up above the layer it landed in; see the module docs. `over` is
    /// the press's hit chain, and what is returned is the chain the rest of
    /// the frame sees: empty when the press landed outside every pop-up and
    /// was spent closing them.
    pub(super) fn press_outside_popups(
        &mut self,
        pointer: PointerInput,
        over: Vec<NodeKey>,
    ) -> Vec<NodeKey> {
        if self.popups.is_empty() || !pointer.down || self.capture.active().is_some() {
            return over;
        }
        let layer = over
            .first()
            .and_then(|&key| self.store.by_key(key))
            .map_or(0, |node| node.layer);
        if layer >= self.popups.len() {
            return over;
        }
        self.close_popups_from(layer);
        if layer > 0 {
            // Inside a lower pop-up: the press is that pop-up's.
            return over;
        }
        self.capture
            .interact(SPENT_PRESS.0, true, pointer.down, pointer.released);
        Vec::new()
    }

    /// Back closes the topmost pop-up when nothing is engaged to cancel, and is
    /// spent doing it: returns `nav` without its back when it was.
    pub(super) fn back_out_of_popup(&mut self, nav: super::NavInput) -> super::NavInput {
        if !nav.back || self.engaged().is_some() || self.popups.is_empty() {
            return nav;
        }
        self.close_popups_from(self.popups.len() - 1);
        super::NavInput { back: false, ..nav }
    }

    /// Closes what the frame did not build — see the module docs — then gives
    /// each of this frame's nodes its layer: 0 for the tree, `n` under the
    /// `n`th open pop-up, and [`CLOSED_LAYER`] under a pop-up that closed
    /// while the frame was being built.
    pub(super) fn layers(&mut self) -> Vec<usize> {
        let unbuilt = self.popups.iter().position(|&anchor| {
            !self.popup_roots.iter().any(|&(_, built)| built == anchor)
                || self.store.find(anchor).is_none()
        });
        if let Some(depth) = unbuilt {
            self.close_popups_from(depth);
        }
        let mut layers = vec![0; self.nodes.len()];
        for &(root, anchor) in &self.popup_roots {
            layers[root] = self
                .popups
                .iter()
                .position(|&open| open == anchor)
                .map_or(CLOSED_LAYER, |depth| depth + 1);
        }
        // A parent is always built before its children.
        for index in 0..self.nodes.len() {
            if let Some(parent) = self.nodes[index].parent {
                layers[index] = layers[parent];
            }
        }
        layers
    }

    /// The viewport [`Ui::layout`] was last given: what a pop-up is kept
    /// inside and clipped to.
    pub(super) fn viewport(&self) -> ClipRect {
        self.viewport.unwrap_or(ClipRect::NONE)
    }

    /// The border-box top-left of the pop-up root `index` of the open pop-up
    /// hanging from `anchor`, which is placed by now.
    pub(super) fn popup_origin(&self, index: usize, anchor: NodeKey) -> Vec2 {
        let anchor = self
            .store
            .by_key(anchor)
            .map_or((Vec2::ZERO, Vec2::ZERO), |node| node.rect);
        let size = self.nodes[index].layout.size;
        hang(anchor, Vec2::new(size.width, size.height), self.viewport())
    }
}

/// The viewport of a tree laid out at `origin` in `available`: the space
/// itself on a definite axis, and no bound on a content-sized one.
pub(super) fn viewport_of(origin: Vec2, available: super::AvailableSpace) -> ClipRect {
    let axis = |start: f32, extent: super::Available| match extent {
        super::Available::Definite(length) => (start, start + length),
        super::Available::MinContent | super::Available::MaxContent => (f32::MIN, f32::MAX),
    };
    let (min_x, max_x) = axis(origin.x, available.width);
    let (min_y, max_y) = axis(origin.y, available.height);
    ClipRect {
        min: Vec2::new(min_x, min_y),
        max: Vec2::new(max_x, max_y),
    }
}

/// Where a pop-up of `size` hangs from the border box `anchor` inside
/// `viewport`: below it, flipped above it when it would run past the bottom
/// and there is more room above, then shifted the least that keeps it inside
/// — its top-left corner kept when it is larger than the viewport.
pub(super) fn hang(anchor: (Vec2, Vec2), size: Vec2, viewport: ClipRect) -> Vec2 {
    let (anchor_min, anchor_max) = anchor;
    let mut min = Vec2::new(anchor_min.x, anchor_max.y);
    let below = viewport.max.y - anchor_max.y;
    let above = anchor_min.y - viewport.min.y;
    if size.y > below && above > below {
        min.y = anchor_min.y - size.y;
    }
    // The far edge first, so the near one wins when both cannot hold.
    min.min(viewport.max - size).max(viewport.min)
}
