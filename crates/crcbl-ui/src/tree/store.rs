//! The persistent node store: what a node keeps from one rebuild to the next.
//!
//! The tree is rebuilt from nothing every frame, so anything that must outlive
//! a frame — the pointer's hover and press, the layout cache, last frame's
//! rectangle — lives here, keyed by the node's [`NodeKey`], in a slab whose
//! slots a frame's nodes point into. A node no frame touched is pruned at
//! layout, and its slot goes back on the free list.

use std::collections::HashMap;

use glam::Vec2;
use taffy::{Cache, Layout};

use super::focus::{Behavior, Engagement, NavStep};
use super::style::{NavId, NodeStyle};
use super::widgets::WidgetState;
use crate::draw_list::ClipRect;
use crate::style::{Candidates, Declaration, InheritedId, PseudoClasses};

/// A node's identity across rebuilds.
///
/// `hash(parent key, explicit id or call site and occurrence)` — see
/// [`crate::tree`]'s key rules. Opaque apart from equality: two frames that
/// build the same node from the same place under the same parent give it the
/// same key, and nothing else about the number means anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeKey(pub(crate) u64);

/// The pointer's relation to one node, and its focus and engagement, resolved
/// against last frame's rectangles when a frame begins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Interaction {
    /// The pointer is over this node or over a node inside it, and no press
    /// outside it is captured.
    pub hovered: bool,
    /// This node captured the press that is held.
    pub pressed: bool,
    /// This node captured a press that was released over it this frame, or
    /// it is focused and was accepted without engaging.
    pub clicked: bool,
    /// This node holds its context's focus.
    pub focused: bool,
    /// Where this node's engagement is this frame.
    pub engagement: Engagement,
    /// The navigation step this engaged node took from focus this frame.
    pub captured: Option<NavStep>,
}

/// Everything one node keeps between frames.
#[derive(Clone, Debug)]
pub(crate) struct StoredNode {
    pub key: NodeKey,
    /// The key of the node it was built under last, for walking a hover or a
    /// capture up to the nodes that contain it.
    pub parent: Option<NodeKey>,
    /// The frame that last built this node. Anything older is pruned.
    pub last_touched_frame: u64,
    /// [`super::NodeStyle::layout_hash`] as last built.
    pub style_hash: u64,
    /// The keys of its children, in order, as last built.
    pub child_hash: u64,
    /// What a span measures from — its text and size, or its image — as last
    /// built. Zero for a block.
    pub content_hash: u64,
    /// Taffy's per-node layout cache, which survives a rebuild unless one of
    /// the three hashes above moved here or in a descendant.
    pub cache: Cache,
    /// Taffy's layout before rounding, relative to the parent. Kept here rather
    /// than in the frame because a cache hit on an ancestor skips writing it.
    pub unrounded: Layout,
    /// Last layout's border box, in screen pixels.
    pub rect: (Vec2, Vec2),
    /// The clip its ancestors' `overflow: hidden` put it under.
    pub clip: ClipRect,
    /// Its place in last frame's paint order; a higher one is drawn on top.
    pub paint_order: usize,
    /// Whether it had a box last frame: `display: none` here or above is never
    /// hit.
    pub hittable: bool,
    pub interaction: Interaction,
    /// How it takes part in focus, as its builder declared it last.
    pub behavior: Behavior,
    /// The widget state its builder declared last — `:checked`, `:open` —
    /// which the cascade matches beside the interaction's pseudo-classes.
    pub state: PseudoClasses,
    /// What a widget keeps on this node between frames; see
    /// [`super::widgets`].
    pub widget: WidgetState,
    /// Its selector's `#id`, hashed, for a `nav-*` property to find.
    pub id: Option<NavId>,
    /// The node focused inside it last, for a scope root.
    pub remembered: Option<NodeKey>,
    /// How far its children are scrolled, subtracted from where they are laid
    /// out. Set by the caller, and by the tree for an `overflow: scroll` block
    /// that brings the focused node into view.
    pub scroll_offset: Vec2,
    /// How far an `overflow: scroll` block's content lets it scroll, as last
    /// laid out; zero for any other block.
    pub scroll_max: Vec2,
    /// The candidate rules for its selector, and what they were gathered for.
    pub candidates: Option<StoredCandidates>,
    /// What its resolved style was keyed by last; see `resolve.rs`.
    pub style_key: u64,
    /// The inline declarations it was resolved with last.
    pub inline: Vec<Declaration>,
    /// Its resolved style as of `style_key`.
    pub resolved: NodeStyle,
    /// What it passed to its children as of `style_key`.
    pub inherited: InheritedId,
}

/// A node's candidate rules, with the selector and stylesheet generation they
/// were gathered for.
#[derive(Clone, Debug)]
pub(crate) struct StoredCandidates {
    pub selector: String,
    pub span: bool,
    pub generation: u64,
    pub candidates: Candidates,
}

impl StoredNode {
    fn new(key: NodeKey, frame: u64) -> Self {
        Self {
            key,
            parent: None,
            last_touched_frame: frame,
            style_hash: 0,
            child_hash: 0,
            content_hash: 0,
            cache: Cache::new(),
            unrounded: Layout::new(),
            rect: (Vec2::ZERO, Vec2::ZERO),
            clip: ClipRect::NONE,
            paint_order: 0,
            hittable: false,
            interaction: Interaction::default(),
            behavior: Behavior::NONE,
            state: PseudoClasses::NONE,
            widget: WidgetState::None,
            id: None,
            remembered: None,
            scroll_offset: Vec2::ZERO,
            scroll_max: Vec2::ZERO,
            candidates: None,
            style_key: 0,
            inline: Vec::new(),
            resolved: NodeStyle::DEFAULT,
            inherited: InheritedId::ROOT,
        }
    }

    /// Last layout's content box, in screen pixels: the border box less the
    /// border and the padding the layout resolved.
    pub fn content_box(&self) -> (Vec2, Vec2) {
        let (border, padding) = (self.unrounded.border, self.unrounded.padding);
        (
            self.rect.0 + Vec2::new(border.left + padding.left, border.top + padding.top),
            self.rect.1 - Vec2::new(border.right + padding.right, border.bottom + padding.bottom),
        )
    }

    /// Whether `pos` is inside the part of last frame's box its clip let
    /// through. Half-open, so two boxes that share an edge never both claim
    /// the pixel on it.
    pub fn contains(&self, pos: Vec2) -> bool {
        let visible = self.clip.intersect(ClipRect {
            min: self.rect.0,
            max: self.rect.1,
        });
        self.hittable && pos.cmpge(visible.min).all() && pos.cmplt(visible.max).all()
    }
}

/// The slab of [`StoredNode`]s and the index from key to slot.
#[derive(Clone, Debug, Default)]
pub(crate) struct NodeStore {
    slots: Vec<Option<StoredNode>>,
    index: HashMap<NodeKey, usize>,
    free: Vec<usize>,
}

impl NodeStore {
    /// The slot holding `key`, if it is stored.
    pub fn find(&self, key: NodeKey) -> Option<usize> {
        self.index.get(&key).copied()
    }

    /// A fresh slot for `key`, touched in `frame`. The caller has checked that
    /// `key` is not stored.
    pub fn insert(&mut self, key: NodeKey, frame: u64) -> usize {
        let node = Some(StoredNode::new(key, frame));
        let slot = if let Some(slot) = self.free.pop() {
            self.slots[slot] = node;
            slot
        } else {
            self.slots.push(node);
            self.slots.len() - 1
        };
        self.index.insert(key, slot);
        slot
    }

    /// The node in `slot`.
    ///
    /// # Panics
    ///
    /// If the slot is free: every slot a frame node names was touched this
    /// frame, and nothing prunes a touched slot.
    pub fn get(&self, slot: usize) -> &StoredNode {
        self.slots[slot]
            .as_ref()
            .expect("a frame node's slot is live until the next prune")
    }

    /// See [`NodeStore::get`].
    pub fn get_mut(&mut self, slot: usize) -> &mut StoredNode {
        self.slots[slot]
            .as_mut()
            .expect("a frame node's slot is live until the next prune")
    }

    /// The stored node for `key`.
    pub fn by_key(&self, key: NodeKey) -> Option<&StoredNode> {
        self.find(key).map(|slot| self.get(slot))
    }

    /// Every stored node.
    pub fn iter(&self) -> impl Iterator<Item = &StoredNode> {
        self.slots.iter().flatten()
    }

    /// Every stored node, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut StoredNode> {
        self.slots.iter_mut().flatten()
    }

    /// Drops every node `frame` did not touch, and returns how many went.
    pub fn prune(&mut self, frame: u64) -> usize {
        let mut pruned = 0;
        for (slot, entry) in self.slots.iter_mut().enumerate() {
            if entry
                .as_ref()
                .is_some_and(|node| node.last_touched_frame != frame)
                && let Some(node) = entry.take()
            {
                self.index.remove(&node.key);
                self.free.push(slot);
                pruned += 1;
            }
        }
        pruned
    }

    /// How many nodes are stored.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// The topmost node whose visible box holds `pos`, and the keys of every
    /// node that contains it, innermost first.
    pub fn hit_chain(&self, pos: Vec2) -> Vec<NodeKey> {
        let target = self
            .iter()
            .filter(|node| node.contains(pos))
            .max_by_key(|node| node.paint_order);
        self.ancestry(target.map(|node| node.key))
    }

    /// Whether `key` is `ancestor` or was built under it, walking the same
    /// chain [`NodeStore::ancestry`] does without collecting it.
    pub fn is_within(&self, key: NodeKey, ancestor: NodeKey) -> bool {
        let mut next = Some(key);
        let mut steps = 0;
        while let Some(key) = next {
            if key == ancestor {
                return true;
            }
            // The same bound `ancestry` walks under.
            steps += 1;
            if steps > self.len() {
                break;
            }
            next = self.by_key(key).and_then(|node| node.parent);
        }
        false
    }

    /// `key` and the keys of every node it was built under, innermost first.
    pub fn ancestry(&self, key: Option<NodeKey>) -> Vec<NodeKey> {
        let mut chain = Vec::new();
        let mut next = key;
        while let Some(key) = next {
            // A cycle cannot be built — a parent is always opened before its
            // child in the same frame — but a chain longer than the store would
            // be one, and walking it forever is the worse failure.
            if chain.len() > self.len() {
                break;
            }
            chain.push(key);
            next = self.by_key(key).and_then(|node| node.parent);
        }
        chain
    }
}
