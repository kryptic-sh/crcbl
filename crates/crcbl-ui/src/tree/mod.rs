//! The element tree: blocks and spans rebuilt every frame, with identity that
//! survives the rebuild, laid out by Taffy.
//!
//! `docs/plan/07-ui-debug.md` rungs 2 and 3. A frame is four calls:
//!
//! ```text
//! ui.begin_frame(pointer)      hover and press, from LAST frame's rectangles
//! ui.block(..., |ui| { ... })  the tree, built from nothing
//! ui.layout(origin, space, atlas)   flexbox on Taffy; untouched nodes pruned
//! ui.emit(&mut list)           backgrounds, borders, clips, text, images
//! ```
//!
//! ```
//! use crcbl_ui::tree::{AvailableSpace, Length, NodeStyle, Ui};
//! use crcbl_ui::{DrawList, FontAtlas, PointerInput};
//!
//! let atlas = FontAtlas::built_in();
//! let mut ui = Ui::new();
//! let panel = NodeStyle {
//!     padding: crcbl_ui::tree::Edges::all(Length::Px(8.0)),
//!     column_gap: Length::Px(4.0),
//!     background: [0.1, 0.1, 0.1, 1.0],
//!     ..NodeStyle::DEFAULT
//! };
//! ui.begin_frame(PointerInput::default());
//! ui.block(Some("#panel"), &panel, |ui| {
//!     ui.span("SCORE", &NodeStyle::DEFAULT);
//!     ui.span("120", &NodeStyle::DEFAULT);
//! });
//! ui.layout(glam::Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
//! let mut list = DrawList::new();
//! ui.emit(&mut list);
//! assert_eq!(list.len(), 3, "a background and two strings");
//! ```
//!
//! # Identity
//!
//! Every node has a [`NodeKey`]: `hash(parent key, id)`, where the id is
//!
//! * the explicit id passed to [`Ui::block`] (`Some("#health")`), or
//! * the key passed to [`Ui::block_keyed`], for the rows of a loop, or
//! * otherwise the **call site** and how many nodes that call site has already
//!   built under the same parent this frame.
//!
//! A loop's rows need [`Ui::block_keyed`]: by call site alone the third row is
//! "the third node from this line", so reordering the list moves each row's
//! hover, press and scroll onto whatever row lands in its place. Clay's and
//! React's documentation both warn of exactly that.
//!
//! **A duplicate key is a warning, not a panic**: the second node gets a key
//! derived from the first so layout stays correct, and the duplicate is
//! reported once per frame per key through `crcbl_core::warn!` and
//! [`Ui::duplicate_keys`].
//!
//! # What survives a rebuild, and what clears it
//!
//! The store keeps, per key, the pointer's hover and press, a scroll offset,
//! last frame's rectangle, Taffy's layout cache, and three hashes: the node's
//! layout style ([`NodeStyle::layout_hash`], which no paint field reaches), its
//! children's keys in order, and its measured content. **When any of the three
//! moves, that node's cache and every ancestor's is cleared; nothing else is.**
//! A sibling keeps its cache, and so does everything under the changed node,
//! whose own inputs have not changed. A node no frame touched is pruned at
//! [`Ui::layout`].
//!
//! # Hit testing is one frame behind
//!
//! [`Ui::begin_frame`] tests the pointer against the rectangles the previous
//! [`Ui::layout`] produced, before this frame's tree exists — the classic
//! immediate-mode trade the plan takes. The topmost node under the pointer and
//! every node containing it are hovered; a press latches on the topmost node
//! through [`UiState`]'s capture, so pressing one node and releasing over
//! another clicks neither.
//!
//! # Emission
//!
//! A block paints its border box: with square corners a filled rect, then the
//! border — one [`DrawList::rect_outline`] when every side is one width, a rect
//! per side otherwise. With any corner radius it is one
//! [`DrawList::rounded_rect`], whose border is uniform, **drawn at the top
//! side's width**. A block with `overflow: hidden` clips its children to its
//! padding box. A text span draws at its content box's top-left in the
//! built-in font; an image span stretches its picture over its content box.
//!
//! # Layout output is rounded
//!
//! Positions and sizes are snapped to whole pixels by Taffy's `round_layout`,
//! on the cumulative position so that no gap opens between neighbours. That is
//! what Chrome's fixtures were generated with, and what keeps a bitmap glyph on
//! the pixel grid. With the features this crate enables, Taffy's flexbox and
//! rounding are comparisons, the four operations and `floor` — no `sqrt`,
//! `powf` or `mul_add` — so the result does not depend on a platform's libm.

mod emit;
mod layout;
mod store;
mod style;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::Location;

use glam::Vec2;
use taffy::{NodeId, compute_root_layout, round_layout};

use crate::draw_list::ClipRect;
#[cfg(doc)]
use crate::draw_list::DrawList;
use crate::image::AtlasImage;
use crate::text::FontAtlas;
use crate::widget::{ButtonState, PointerInput, UiState};

use layout::{LayoutTree, MeasureCache};
use store::{Interaction, NodeStore};

pub use layout::MEASURE_WIDTH_BUCKET;
pub use store::NodeKey;
pub use style::{
    Align, Display, Edges, FlexDirection, FlexWrap, Justify, Length, LengthAuto, NodeStyle,
    Overflow, Position,
};

/// The space a root is laid out in, on one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Available {
    /// This many pixels.
    Definite(f32),
    /// As little as the content can take.
    MinContent,
    /// As much as the content wants.
    MaxContent,
}

/// The space every root is laid out in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvailableSpace {
    /// Horizontally.
    pub width: Available,
    /// Vertically.
    pub height: Available,
}

impl AvailableSpace {
    /// Unbounded on both axes: a root sized by its content and its own style.
    pub const MAX_CONTENT: Self = Self {
        width: Available::MaxContent,
        height: Available::MaxContent,
    };

    /// A surface of `size` pixels.
    #[must_use]
    pub const fn definite(size: Vec2) -> Self {
        Self {
            width: Available::Definite(size.x),
            height: Available::Definite(size.y),
        }
    }
}

impl Available {
    const fn to_taffy(self) -> taffy::AvailableSpace {
        match self {
            Self::Definite(px) => taffy::AvailableSpace::Definite(px),
            Self::MinContent => taffy::AvailableSpace::MinContent,
            Self::MaxContent => taffy::AvailableSpace::MaxContent,
        }
    }
}

/// What a span holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span<'a> {
    /// A string in the built-in font, measured by it.
    Text(&'a str),
    /// A registered picture, measured by its size in texels.
    Image(AtlasImage),
}

impl<'a> From<&'a str> for Span<'a> {
    fn from(text: &'a str) -> Self {
        Self::Text(text)
    }
}

impl From<AtlasImage> for Span<'_> {
    fn from(image: AtlasImage) -> Self {
        Self::Image(image)
    }
}

/// A node's key and the pointer's relation to it, as resolved when the frame
/// began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    /// The node's identity; [`Ui::rect`] takes it.
    pub key: NodeKey,
    /// The pointer is over the node or over something inside it, and no press
    /// elsewhere is captured.
    pub hovered: bool,
    /// The node captured the press that is held.
    pub pressed: bool,
    /// The node captured a press that was released over it this frame.
    pub clicked: bool,
}

/// What a frame node is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Content {
    /// A container.
    Block,
    /// A string: `Ui::text[start..end]`.
    Text { start: usize, end: usize },
    /// A picture.
    Image(AtlasImage),
}

/// One node as this frame built it.
#[derive(Clone, Debug)]
struct FrameNode {
    key: NodeKey,
    /// Its slot in the store.
    slot: usize,
    parent: Option<usize>,
    style: NodeStyle,
    content: Content,
    content_hash: u64,
    /// Folded over each child's key as the child is built.
    child_hash: u64,
    /// Whether the store had no node of this key before this frame.
    fresh: bool,
    first_child: Option<usize>,
    last_child: Option<usize>,
    next_sibling: Option<usize>,
    /// Where its children start in `Ui::children`, once flattened at layout.
    child_start: usize,
    child_count: usize,
    /// The rounded layout, relative to the parent.
    layout: taffy::Layout,
}

/// The seed a root's key is derived from, in place of a parent's key.
const ROOT_KEY: NodeKey = NodeKey(0x6372_6362_6c2d_7569);

/// Which of the three identity rules made a key, folded into it so that an
/// explicit id can never collide with a loop key of the same hash.
#[derive(Hash)]
enum KeySource<'a> {
    CallSite(&'a str, u32, u32, u32),
    Id(&'a str),
    Keyed(u64),
    Duplicate(u64, u32),
}

fn hash_of(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The element tree and the store it keeps between frames. See the module
/// docs for the frame's shape.
#[derive(Debug, Default)]
pub struct Ui {
    store: NodeStore,
    nodes: Vec<FrameNode>,
    children: Vec<NodeId>,
    text: String,
    /// The blocks whose closures are running, innermost last.
    open: Vec<usize>,
    /// How many nodes each call site has built under each parent this frame.
    call_sites: HashMap<(Option<usize>, u64), u32>,
    duplicates: Vec<NodeKey>,
    /// The content hash of every text span built this frame.
    live_text: HashSet<u64>,
    measure: MeasureCache,
    capture: UiState,
    frame: u64,
}

impl Ui {
    /// An empty tree with nothing stored.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a frame: drops last frame's nodes, and resolves `pointer`
    /// against last frame's rectangles.
    pub fn begin_frame(&mut self, pointer: PointerInput) {
        self.frame += 1;
        self.nodes.clear();
        self.children.clear();
        self.text.clear();
        self.open.clear();
        self.call_sites.clear();
        self.duplicates.clear();
        self.live_text.clear();
        self.resolve_pointer(pointer);
    }

    /// Hover, press capture and click for every stored node, from last frame's
    /// rectangles.
    fn resolve_pointer(&mut self, pointer: PointerInput) {
        let over = self.store.hit_chain(pointer.pos);
        let mut pressed = None;
        let mut clicked = None;

        // The captured node first, so a release frees the capture before
        // anything else can be offered the press — `UiState::interact`'s rule.
        if let Some(active) = self.capture.active() {
            let (state, click) = self.capture.interact(
                active,
                over.contains(&NodeKey(active)),
                pointer.down,
                pointer.released,
            );
            pressed = (state == ButtonState::Pressed).then_some(NodeKey(active));
            clicked = click.then_some(NodeKey(active));
        }
        if self.capture.active().is_none()
            && let Some(&target) = over.first()
        {
            let (state, click) =
                self.capture
                    .interact(target.0, true, pointer.down, pointer.released);
            if state == ButtonState::Pressed {
                pressed = Some(target);
            }
            if click {
                clicked = Some(target);
            }
        }

        let captured = self.store.ancestry(self.capture.active().map(NodeKey));
        for node in self.store.iter_mut() {
            node.interaction = Interaction {
                hovered: over.contains(&node.key)
                    && (captured.is_empty() || captured.contains(&node.key)),
                pressed: pressed == Some(node.key),
                clicked: clicked == Some(node.key),
            };
        }
    }

    /// A block: a flex container whose children `build` adds.
    ///
    /// `id` is an explicit identity (`Some("#health")`); `None` keys the block
    /// by this call site. See the module docs.
    #[track_caller]
    pub fn block(
        &mut self,
        id: Option<&str>,
        style: &NodeStyle,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let source = match id {
            Some(id) => self.key(KeySource::Id(id)),
            None => self.call_site_key(Location::caller()),
        };
        self.open_block(source, style, build)
    }

    /// A block keyed by `key`, for one row of a loop: its identity follows the
    /// item rather than its position.
    pub fn block_keyed(
        &mut self,
        key: impl Hash,
        style: &NodeStyle,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let source = self.key(KeySource::Keyed(hash_of(key)));
        self.open_block(source, style, build)
    }

    /// A span: text or a picture, keyed by this call site.
    #[track_caller]
    pub fn span<'a>(&mut self, content: impl Into<Span<'a>>, style: &NodeStyle) -> Response {
        let key = self.call_site_key(Location::caller());
        let (content, content_hash) = match content.into() {
            Span::Text(text) => {
                let start = self.text.len();
                self.text.push_str(text);
                let content_hash = hash_of((text, style.font_size.to_bits()));
                self.live_text.insert(content_hash);
                (
                    Content::Text {
                        start,
                        end: self.text.len(),
                    },
                    content_hash,
                )
            }
            Span::Image(image) => (Content::Image(image), hash_of((image.width, image.height))),
        };
        let index = self.push(key, style, content, content_hash);
        self.close(index);
        self.response(index)
    }

    fn open_block(
        &mut self,
        key: NodeKey,
        style: &NodeStyle,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let index = self.push(key, style, Content::Block, 0);
        let response = self.response(index);
        self.open.push(index);
        build(self);
        self.open.pop();
        self.close(index);
        response
    }

    /// The current parent's key folded with `source`.
    fn key(&self, source: KeySource<'_>) -> NodeKey {
        let parent = self
            .open
            .last()
            .map_or(ROOT_KEY, |&parent| self.nodes[parent].key);
        NodeKey(hash_of((parent, source)))
    }

    /// The key for the next node built at `location` under the current parent.
    fn call_site_key(&mut self, location: &Location<'_>) -> NodeKey {
        let site = hash_of((location.file(), location.line(), location.column()));
        let occurrence = self
            .call_sites
            .entry((self.open.last().copied(), site))
            .or_insert(0);
        let nth = *occurrence;
        *occurrence += 1;
        self.key(KeySource::CallSite(
            location.file(),
            location.line(),
            location.column(),
            nth,
        ))
    }

    /// Appends a node under the current parent and touches its slot.
    fn push(
        &mut self,
        key: NodeKey,
        style: &NodeStyle,
        content: Content,
        content_hash: u64,
    ) -> usize {
        let key = self.unique(key);
        let (slot, fresh) = match self.store.find(key) {
            Some(slot) => (slot, false),
            None => (self.store.insert(key, self.frame), true),
        };
        let parent = self.open.last().copied();
        let stored = self.store.get_mut(slot);
        stored.last_touched_frame = self.frame;
        stored.parent = parent.map(|parent| self.nodes[parent].key);

        let index = self.nodes.len();
        self.nodes.push(FrameNode {
            key,
            slot,
            parent,
            style: *style,
            content,
            content_hash,
            child_hash: 0,
            fresh,
            first_child: None,
            last_child: None,
            next_sibling: None,
            child_start: 0,
            child_count: 0,
            layout: taffy::Layout::new(),
        });
        if let Some(parent) = parent {
            if let Some(last) = self.nodes[parent].last_child {
                self.nodes[last].next_sibling = Some(index);
            } else {
                self.nodes[parent].first_child = Some(index);
            }
            let node = &mut self.nodes[parent];
            node.last_child = Some(index);
            node.child_hash = hash_of((node.child_hash, key));
        }
        index
    }

    /// `key`, or — if a node already took it this frame — a key derived from
    /// it, after reporting the duplicate once for this frame.
    fn unique(&mut self, key: NodeKey) -> NodeKey {
        let frame = self.frame;
        let taken = |store: &NodeStore, key| {
            store
                .find(key)
                .is_some_and(|slot| store.get(slot).last_touched_frame == frame)
        };
        if !taken(&self.store, key) {
            return key;
        }
        if !self.duplicates.contains(&key) {
            crcbl_core::warn!(
                "ui tree: two nodes under one parent share the key {key:?}; give a loop's rows \
                 `block_keyed` or distinct ids"
            );
            self.duplicates.push(key);
        }
        let mut nth = 0;
        loop {
            let derived = NodeKey(hash_of(KeySource::Duplicate(key.0, nth)));
            if !taken(&self.store, derived) {
                return derived;
            }
            nth += 1;
        }
    }

    /// Diffs a finished node against the store, and clears its cache and every
    /// ancestor's when its style, children or content changed.
    fn close(&mut self, index: usize) {
        let node = &self.nodes[index];
        let mut style_hash = DefaultHasher::new();
        node.style.layout_hash(&mut style_hash);
        let style_hash = style_hash.finish();
        let stored = self.store.get_mut(node.slot);
        let changed = node.fresh
            || stored.style_hash != style_hash
            || stored.child_hash != node.child_hash
            || stored.content_hash != node.content_hash;
        if !changed {
            return;
        }
        stored.style_hash = style_hash;
        stored.child_hash = node.child_hash;
        stored.content_hash = node.content_hash;
        let mut next = Some(index);
        while let Some(index) = next {
            let node = &self.nodes[index];
            self.store.get_mut(node.slot).cache.clear();
            next = node.parent;
        }
    }

    fn response(&self, index: usize) -> Response {
        let node = &self.nodes[index];
        let Interaction {
            hovered,
            pressed,
            clicked,
        } = self.store.get(node.slot).interaction;
        Response {
            key: node.key,
            hovered,
            pressed,
            clicked,
        }
    }

    /// The innermost open block's stored state, if a block is open.
    fn current(&self) -> Option<&store::StoredNode> {
        self.open
            .last()
            .map(|&index| self.store.get(self.nodes[index].slot))
    }

    /// Whether the pointer is over the innermost open block; see
    /// [`Response::hovered`]. False outside every block.
    #[must_use]
    pub fn hovered(&self) -> bool {
        self.current().is_some_and(|node| node.interaction.hovered)
    }

    /// Whether the innermost open block holds the press; see
    /// [`Response::pressed`].
    #[must_use]
    pub fn pressed(&self) -> bool {
        self.current().is_some_and(|node| node.interaction.pressed)
    }

    /// Whether the innermost open block was clicked this frame; see
    /// [`Response::clicked`].
    #[must_use]
    pub fn clicked(&self) -> bool {
        self.current().is_some_and(|node| node.interaction.clicked)
    }

    /// How far the innermost open block's children are scrolled. Zero outside
    /// every block.
    #[must_use]
    pub fn scroll_offset(&self) -> Vec2 {
        self.current().map_or(Vec2::ZERO, |node| node.scroll_offset)
    }

    /// Scrolls the innermost open block's children by `offset`: they are drawn
    /// and hit that far up and left of where they are laid out. **Not clamped**
    /// — the tree does not yet measure how far its content reaches. Does
    /// nothing outside every block.
    pub fn set_scroll_offset(&mut self, offset: Vec2) {
        if let Some(&index) = self.open.last() {
            self.store.get_mut(self.nodes[index].slot).scroll_offset = offset;
        }
    }

    /// Lays out this frame's tree and prunes every stored node it did not
    /// build.
    ///
    /// Every top-level block is a root, laid out in `available` and placed at
    /// `origin`; a page with more than one panel puts them under one root and
    /// positions them there.
    pub fn layout(&mut self, origin: Vec2, available: AvailableSpace, atlas: &FontAtlas) {
        self.flatten_children();
        self.store.prune(self.frame);
        self.measure.retain(&self.live_text);

        let space = taffy::Size {
            width: available.width.to_taffy(),
            height: available.height.to_taffy(),
        };
        let roots: Vec<usize> = (0..self.nodes.len())
            .filter(|&index| self.nodes[index].parent.is_none())
            .collect();
        let mut tree = LayoutTree {
            nodes: &mut self.nodes,
            children: &self.children,
            store: &mut self.store,
            text: &self.text,
            measure: &mut self.measure,
            atlas,
        };
        for &root in &roots {
            compute_root_layout(&mut tree, NodeId::from(root), space);
            round_layout(&mut tree, NodeId::from(root));
        }
        self.place(origin);
    }

    /// Writes each node's children into `children` in build order.
    fn flatten_children(&mut self) {
        self.children.clear();
        for index in 0..self.nodes.len() {
            let start = self.children.len();
            let mut next = self.nodes[index].first_child;
            while let Some(child) = next {
                self.children.push(NodeId::from(child));
                next = self.nodes[child].next_sibling;
            }
            let node = &mut self.nodes[index];
            node.child_start = start;
            node.child_count = self.children.len() - start;
        }
    }

    /// Resolves every node's screen rectangle and clip into the store, for
    /// emission and for next frame's hit test.
    fn place(&mut self, origin: Vec2) {
        // Nodes are in build order, so a parent is always placed before its
        // children and these three are filled in by the time a child reads them.
        let mut placed: Vec<(Vec2, ClipRect, bool)> = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let (base, clip, hidden) = match node.parent {
                Some(parent) => {
                    let (parent_min, parent_clip, parent_hidden) = placed[parent];
                    let parent_node = &self.nodes[parent];
                    let scroll = self.store.get(parent_node.slot).scroll_offset;
                    let clip = if parent_node.style.overflow == Overflow::Hidden {
                        let (min, max) = padding_box(parent_min, &parent_node.layout);
                        parent_clip.intersect(ClipRect { min, max })
                    } else {
                        parent_clip
                    };
                    (parent_min - scroll, clip, parent_hidden)
                }
                None => (origin, ClipRect::NONE, false),
            };
            let min = base + Vec2::new(node.layout.location.x, node.layout.location.y);
            let max = min + Vec2::new(node.layout.size.width, node.layout.size.height);
            let hidden = hidden || node.style.display == Display::None;
            placed.push((min, clip, hidden));

            let stored = self.store.get_mut(node.slot);
            stored.rect = (min, max);
            stored.clip = clip;
            stored.paint_order = index;
            stored.hittable = !hidden;
        }
    }

    /// Last layout's border box for `key`, in screen pixels.
    #[must_use]
    pub fn rect(&self, key: NodeKey) -> Option<(Vec2, Vec2)> {
        self.store.by_key(key).map(|node| node.rect)
    }

    /// How many nodes the store holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the store holds no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.len() == 0
    }

    /// The keys two nodes shared this frame, each once.
    #[must_use]
    pub fn duplicate_keys(&self) -> &[NodeKey] {
        &self.duplicates
    }
}

/// A node's padding box, given its border box's top-left.
fn padding_box(min: Vec2, layout: &taffy::Layout) -> (Vec2, Vec2) {
    let border = layout.border;
    (
        min + Vec2::new(border.left, border.top),
        min + Vec2::new(
            layout.size.width - border.right,
            layout.size.height - border.bottom,
        ),
    )
}
