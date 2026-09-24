//! The element tree: blocks and spans rebuilt every frame, with identity that
//! survives the rebuild, laid out by Taffy.
//!
//! UI rungs 2 and 3. A frame is four calls:
//!
//! ```text
//! ui.begin_frame(pointer)      hover and press, from LAST frame's rectangles
//! ui.block(..., |ui| { ... })  the tree, built from nothing, each node styled
//! ui.layout(origin, space, atlas)   flexbox on Taffy; untouched nodes pruned
//! ui.emit(&mut list)           backgrounds, borders, clips, text, images
//! ```
//!
//! ```
//! use crcbl_ui::style::Declaration;
//! use crcbl_ui::tree::{AvailableSpace, LengthAuto, Ui};
//! use crcbl_ui::{DrawList, FontAtlas, PointerInput};
//!
//! let atlas = FontAtlas::built_in();
//! let mut ui = Ui::new();
//! ui.add_stylesheet(
//!     "hud.css",
//!     ".hud { padding: 8px; gap: 4px; background: #1a1a1a; color: gold }",
//! );
//! ui.begin_frame(PointerInput::default());
//! ui.block("#panel.hud", &[Declaration::MinWidth(LengthAuto::Px(120.0))], |ui| {
//!     ui.span("", "SCORE", &[]);
//!     ui.span("", "120", &[]);
//! });
//! ui.layout(glam::Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
//! let mut list = DrawList::new();
//! ui.emit(&mut list);
//! assert_eq!(list.len(), 3, "a background and two strings");
//! ```
//!
//! # Styles
//!
//! A node's style comes from the stylesheets — see [`crate::style`] — matched
//! against the selector its builder was given, `type#id.class`, with the
//! builder's inline [`Declaration`]s over the top. [`NodeStyle::declarations`]
//! turns a whole style into an inline override that leaves the stylesheets no
//! say. A stylesheet changes with [`Ui::add_stylesheet`],
//! [`Ui::replace_stylesheet`] and, for one loaded from a file,
//! [`Ui::poll_stylesheets`]; how resolution is cached is `resolve.rs`'s.
//!
//! # Focus
//!
//! A frame that begins with [`Ui::begin_frame_with`] also takes a
//! [`NavInput`], and a node built with [`Ui::block_with`] declares a
//! [`Behavior`]: whether it is a button, a widget with an engaged state or a
//! focus scope. Focus sets `:focus`, engagement `:engaged` and a disabled
//! behavior `:disabled`; `focus/mod.rs` has the rules, and
//! [`Ui::set_nav_debug`] draws why a move went where it did.
//!
//! # Widgets
//!
//! [`Ui::button`], [`Ui::checkbox`], [`Ui::slider`], [`Ui::drag_value`],
//! [`Ui::collapsing`], [`Ui::tree_node`], [`Ui::split`], [`Ui::list`],
//! [`Ui::text_input`], [`Ui::outliner`], [`Ui::tabs`] and [`Ui::dock`] are
//! builders over blocks and spans, each styled by `default.css`;
//! `widgets/mod.rs` has what each builds and the rules it keeps.
//!
//! # Identity
//!
//! Every node has a [`NodeKey`]: `hash(parent key, id)`, where the id is
//!
//! * the `#id` in the selector passed to [`Ui::block`] or [`Ui::span`]
//!   (`"#health.hud"`), or
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
//! The store keeps, per key, the pointer's hover and press, focus and
//! engagement, a scope's remembered node, a scroll offset, a widget's state,
//! last frame's rectangle, its resolved style, Taffy's layout cache, and three
//! hashes: the node's resolved layout style ([`NodeStyle::layout_hash`], which
//! no paint field reaches), its
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
//! — or on the innermost node around it with a [`Role`], so a click on a
//! button's label clicks the button — through [`UiState`]'s capture, so
//! pressing one node and releasing over another clicks neither. A disabled
//! node takes the press and is never clicked.
//!
//! # Emission
//!
//! A block paints its border box: with square corners a filled rect, then the
//! border — one [`DrawList::rect_outline`] when every side is one width, a rect
//! per side otherwise. With any corner radius it is one
//! [`DrawList::rounded_rect`], whose border is uniform, **drawn at the top
//! side's width**. A `background-image` is stretched over the padding box
//! between the background and the border, and a `border-image` is a
//! [`DrawList::nine_slice_bands`] over the border box in the border's place;
//! both are pictures [`Ui::set_image`] bound by name, and a rounded corner
//! clips neither. A block with `overflow: hidden` clips its children to its
//! padding box, and so does `overflow: scroll`. A text span draws from its content box's top-left: in the
//! bitmap font as one line per newline, or — when its `font-family` names a
//! parsed font, the committed one or one [`Ui::register_font`] registered — as
//! a [`crate::font::layout::TextLayout`] broken at the width layout measured it
//! under, aligned by its `text-align` and pushed as one
//! [`DrawList::glyphs`] run of that font. An image span
//! stretches its picture over its content box. Outlines are drawn after the
//! whole tree, in build order, each under the clip its node was drawn under,
//! so a later sibling never covers a focus ring: a ring `outline-offset`
//! outside the border box, one [`DrawList::rect_outline`] — or a
//! [`DrawList::rounded_rect`], its radii grown by the offset, when the node has
//! any.
//!
//! # Text measures through Taffy
//!
//! A text span is a leaf whose measure callback lays its text out under the
//! width Taffy offers: unbroken for max-content, broken at every space for
//! min-content, and broken to fit for a definite width — so a wrapped label
//! grows its block's height. Measurements are cached by the text, the style
//! fields that size it and the whole-pixel width; see `layout.rs`.
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
pub mod focus;
#[cfg(test)]
mod font_tests;
mod layout;
mod resolve;
mod store;
mod style;
#[cfg(test)]
mod style_tests;
#[cfg(test)]
mod tests;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::Location;

use glam::Vec2;
use taffy::{NodeId, compute_root_layout, round_layout};

use crate::draw_list::ClipRect;
#[cfg(doc)]
use crate::draw_list::DrawList;
use crate::font::{Font, ReservedFamilyName, is_reserved_family};
use crate::image::AtlasImage;
use crate::style::{Declaration, InheritedId, NodeSelector, PseudoClasses, Styles};
use crate::text::FontAtlas;
use crate::widget::{ButtonState, PointerInput, UiState};

use focus::FocusState;
use layout::{LayoutTree, MeasureCache};
use store::{Interaction, NodeStore};

pub use crate::font::layout::TextAlign;
pub use crate::font::{FamilyName, FontFamily};
pub use focus::{
    Behavior, Direction, Engagement, FOCUS_HISTORY, InputMode, NavInput, NavScore, NavStep, Role,
    Scope,
};
pub use store::NodeKey;
pub use style::{
    Align, BorderImage, BorderImageWidth, Display, Edges, FlexDirection, FlexWrap, ImageName,
    Justify, Length, LengthAuto, LineHeight, NavId, NavTarget, NavWrap, NodeStyle, Overflow,
    Position,
};
pub use widgets::{
    AXES, ClipboardAnswer, ClipboardReply, ClipboardRequest, DOUBLE_CLICK_TIME, DockLayout,
    DockSide, FieldEdit, FieldRow, INSPECTOR_STEP, Inspection, InspectorOptions, LIST_OVERSCAN,
    MASK, OUTLINER_INDENT, OUTLINER_ROW_HEIGHT, OutlinerBuilder, OutlinerId, OutlinerOptions,
    OutlinerRow, OutlinerState, Overrides, RowBuilder, SPLIT_NAV_STEP, SelectMode, SplitAxis,
    TextInput, TextInputOptions, WHOLE_STEP,
};

/// How far the pointer must move from where a press began, in pixels, before
/// the press is a drag rather than a click: the default of Windows'
/// `SM_CXDRAG`. A drag that ends over an engage widget focuses it without
/// engaging it — dragging a slider is engage, adjust and commit in one
/// gesture — and a drag-value moves only once its press is a drag.
pub const DRAG_THRESHOLD: f32 = 4.0;

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
    /// A string, measured in the font its style names.
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

/// A node's key and its interaction state — the pointer's relation to it, its
/// focus and its engagement — as resolved when the frame began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    /// The node's identity; [`Ui::rect`] takes it.
    pub key: NodeKey,
    /// The pointer is over the node or over something inside it, and no press
    /// elsewhere is captured.
    pub hovered: bool,
    /// The node captured the press that is held.
    pub pressed: bool,
    /// The node captured a press that was released over it this frame, or —
    /// focused, and not a [`Role::Engage`] — was accepted: the one event a
    /// widget fires on, whichever device spoke.
    pub clicked: bool,
    /// The node holds the tree's focus, in either [`InputMode`].
    pub focused: bool,
    /// Where the node's engagement is this frame.
    pub engagement: Engagement,
    /// The navigation step the engaged node took instead of focus this frame.
    pub captured: Option<NavStep>,
    /// A widget builder changed the value it edits, or the state it keeps,
    /// this frame. Always false from [`Ui::block`] and its kin.
    pub changed: bool,
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
    /// Its builder's selector: `Ui::selectors[start..end]`, empty when the
    /// builder's was malformed.
    selector: (usize, usize),
    /// Its pseudo-class state this frame.
    pseudo: PseudoClasses,
    /// What it passes to its children.
    inherited: InheritedId,
    /// Its resolved style.
    style: NodeStyle,
    content: Content,
    /// The parsed font a text span measures and draws in — a registered one,
    /// else its built-in family's — or `None` for the bitmap font. Resolved
    /// when the span is built.
    font: Option<&'static Font>,
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
    /// Every node's selector this frame, run together.
    selectors: String,
    /// Its stylesheets and cascade state.
    styles: Styles,
    /// Builder selectors already reported as malformed, so each is reported
    /// once.
    malformed: HashSet<String>,
    measure: MeasureCache,
    capture: UiState,
    focus: FocusState,
    frame: u64,
    /// This frame's pointer, for a widget a press drags.
    pointer: PointerInput,
    /// Where the press held now, or the last one, began.
    press_origin: Vec2,
    /// Whether that press has moved past [`DRAG_THRESHOLD`].
    dragged: bool,
    /// How many [`Ui::enabled`] scopes that disable are open.
    disabled_depth: u32,
    /// The rows of the tree nodes whose children are being built, innermost
    /// last.
    tree_rows: Vec<NodeKey>,
    /// This frame's text input, from [`Ui::set_text_input`].
    text_frame: TextInput,
    /// Every frame's [`TextInput::dt`] together.
    text_clock: std::time::Duration,
    /// Each text input's editing state, pruned with the store.
    edits: HashMap<NodeKey, widgets::EditState>,
    /// This frame's text inputs, for the pass after layout.
    fits: Vec<widgets::TextFit>,
    /// The clipboard requests this frame's text inputs made.
    clipboard_requests: Vec<ClipboardRequest>,
    /// The pictures a stylesheet's `url()` names, from [`Ui::set_image`].
    images: HashMap<ImageName, AtlasImage>,
    /// The fonts a `font-family` list names, from [`Ui::register_font`].
    fonts: HashMap<FamilyName, &'static Font>,
    /// Family names a text span was built in with no font registered under
    /// them, already warned about, so each is reported once.
    unregistered: HashSet<FamilyName>,
    /// The node this frame's pointer clicked, resolved when the frame began.
    clicked: Option<NodeKey>,
    /// The tree row [`Ui::tree_item_step`] opened or closed when this frame
    /// began, if it opened or closed one.
    tree_toggled: Option<NodeKey>,
}

impl Ui {
    /// An empty tree with nothing stored.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a frame with no navigation input from a pointer: drops last
    /// frame's nodes, and resolves `pointer` against last frame's rectangles.
    /// [`Ui::begin_frame_with`] with [`NavInput::default`].
    pub fn begin_frame(&mut self, pointer: PointerInput) {
        self.begin_frame_with(pointer, NavInput::default());
    }

    /// Starts a frame: drops last frame's nodes, resolves `pointer` against
    /// last frame's rectangles, and then focus and engagement from `nav`
    /// against last frame's tree. See `focus/mod.rs`.
    pub fn begin_frame_with(&mut self, pointer: PointerInput, nav: NavInput) {
        self.frame += 1;
        self.nodes.clear();
        self.children.clear();
        self.text.clear();
        self.open.clear();
        self.call_sites.clear();
        self.duplicates.clear();
        self.live_text.clear();
        self.selectors.clear();
        self.styles.begin_frame();
        self.disabled_depth = 0;
        self.tree_rows.clear();
        self.text_frame = TextInput::default();
        self.fits.clear();
        self.clipboard_requests.clear();
        self.pointer = pointer;
        self.tree_toggled = None;
        let clicked = self.resolve_pointer(pointer);
        self.clicked = clicked;
        self.resolve_navigation(nav, clicked, self.dragged);
    }

    /// Hover, press capture and click for every stored node, from last frame's
    /// rectangles, recording where the press began and whether it is a drag.
    /// Returns the node clicked, unless it is disabled.
    fn resolve_pointer(&mut self, pointer: PointerInput) -> Option<NodeKey> {
        let held_before = self.capture.active().is_some();
        let over = self.store.hit_chain(pointer.pos);
        // A press goes to the innermost node with a role around the topmost
        // one, so a button's label does not take its button's click.
        let target = over
            .iter()
            .copied()
            .find(|&key| {
                self.store
                    .by_key(key)
                    .is_some_and(|node| node.behavior.role != Role::None)
            })
            .or_else(|| over.first().copied());
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
            && let Some(target) = target
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

        // A press that began this frame starts a new gesture; one held since an
        // earlier frame becomes a drag once it has moved far enough, and stays
        // one until the next press.
        if !held_before && (pressed.is_some() || clicked.is_some()) {
            self.press_origin = pointer.pos;
            self.dragged = false;
        } else if held_before
            && (pointer.pos - self.press_origin).length_squared() > DRAG_THRESHOLD * DRAG_THRESHOLD
        {
            self.dragged = true;
        }

        let clicked = clicked.filter(|&key| {
            self.store
                .by_key(key)
                .is_some_and(|node| !node.behavior.disabled)
        });
        let captured = self.store.ancestry(self.capture.active().map(NodeKey));
        for node in self.store.iter_mut() {
            node.interaction = Interaction {
                hovered: over.contains(&node.key)
                    && (captured.is_empty() || captured.contains(&node.key)),
                pressed: pressed == Some(node.key),
                clicked: clicked == Some(node.key),
                ..Interaction::default()
            };
        }
        clicked
    }

    /// A block: a flex container whose children `build` adds.
    ///
    /// `selector` is the node's own `type#id.class` — every part optional,
    /// `""` for a plain block — which stylesheet rules match against; its
    /// `#id` is also the node's explicit identity, and with none the block is
    /// keyed by this call site. See the module docs. `inline` overrides every
    /// rule; `&[]` for none.
    #[track_caller]
    pub fn block(
        &mut self,
        selector: &str,
        inline: &[Declaration],
        build: impl FnOnce(&mut Self),
    ) -> Response {
        self.block_with(selector, inline, Behavior::NONE, build)
    }

    /// A block that takes part in focus as `behavior` says: [`Ui::block`]
    /// otherwise.
    #[track_caller]
    pub fn block_with(
        &mut self,
        selector: &str,
        inline: &[Declaration],
        behavior: Behavior,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let parsed = self.node_selector(selector);
        let key = self.selector_key(parsed, Location::caller());
        self.open_block(key, parsed, inline, behavior, PseudoClasses::NONE, build)
    }

    /// A block keyed by `key`, for one row of a loop: its identity follows the
    /// item rather than its position. `selector` and `inline` are
    /// [`Ui::block`]'s, and an `#id` in the selector does not key it.
    pub fn block_keyed(
        &mut self,
        key: impl Hash,
        selector: &str,
        inline: &[Declaration],
        build: impl FnOnce(&mut Self),
    ) -> Response {
        self.block_keyed_with(key, selector, inline, Behavior::NONE, build)
    }

    /// [`Ui::block_keyed`] with a [`Behavior`], as [`Ui::block_with`].
    pub fn block_keyed_with(
        &mut self,
        key: impl Hash,
        selector: &str,
        inline: &[Declaration],
        behavior: Behavior,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let parsed = self.node_selector(selector);
        let key = self.key(KeySource::Keyed(hash_of(key)));
        self.open_block(key, parsed, inline, behavior, PseudoClasses::NONE, build)
    }

    /// [`Ui::block_keyed`] drawn in pseudo-class `state` whatever the pointer
    /// and focus say: for a view of a model that keeps its own interaction
    /// state, as [`crate::menu::Menu`] keeps its selection and press.
    pub(crate) fn block_keyed_in_state(
        &mut self,
        key: impl Hash,
        selector: &str,
        inline: &[Declaration],
        state: PseudoClasses,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let parsed = self.node_selector(selector);
        let key = self.key(KeySource::Keyed(hash_of(key)));
        self.open_block(key, parsed, inline, Behavior::NONE, state, build)
    }

    /// A span: text or a picture, keyed by its selector's `#id` or else by
    /// this call site. `selector` and `inline` are [`Ui::block`]'s; a span's
    /// type is `span` unless the selector names one.
    #[track_caller]
    pub fn span<'a>(
        &mut self,
        selector: &str,
        content: impl Into<Span<'a>>,
        inline: &[Declaration],
    ) -> Response {
        let parsed = self.node_selector(selector);
        let key = self.selector_key(parsed, Location::caller());
        let content = match content.into() {
            Span::Text(text) => {
                let start = self.text.len();
                self.text.push_str(text);
                Content::Text {
                    start,
                    end: self.text.len(),
                }
            }
            Span::Image(image) => Content::Image(image),
        };
        let index = self.push(
            key,
            parsed,
            inline,
            Behavior::NONE,
            PseudoClasses::NONE,
            content,
        );
        self.close(index);
        self.response(index)
    }

    /// `selector` split into its parts, or — reported once — no parts at all.
    fn node_selector<'s>(&mut self, selector: &'s str) -> NodeSelector<'s> {
        NodeSelector::parse(selector).unwrap_or_else(|()| {
            if self.malformed.insert(selector.to_owned()) {
                crcbl_core::warn!(
                    "ui tree: `{selector}` is not a node selector (`type#id.class`, names of \
                     letters, digits, `-` and `_`); the node is built with none"
                );
            }
            NodeSelector::EMPTY
        })
    }

    fn open_block(
        &mut self,
        key: NodeKey,
        selector: NodeSelector<'_>,
        inline: &[Declaration],
        behavior: Behavior,
        state: PseudoClasses,
        build: impl FnOnce(&mut Self),
    ) -> Response {
        let index = self.push(key, selector, inline, behavior, state, Content::Block);
        let response = self.response(index);
        self.open.push(index);
        build(self);
        self.open.pop();
        self.close(index);
        response
    }

    /// The key a node built from `selector` at `location` gets: its `#id`, or
    /// else the call site.
    fn selector_key(&mut self, selector: NodeSelector<'_>, location: &Location<'_>) -> NodeKey {
        match selector.id {
            Some(id) => self.key(KeySource::Id(id)),
            None => self.call_site_key(location),
        }
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

    /// Appends a node under the current parent, touches its slot and resolves
    /// its style.
    fn push(
        &mut self,
        key: NodeKey,
        selector: NodeSelector<'_>,
        inline: &[Declaration],
        behavior: Behavior,
        state: PseudoClasses,
        content: Content,
    ) -> usize {
        let key = self.unique(key);
        let behavior = Behavior {
            disabled: behavior.disabled || self.disabled_depth > 0,
            ..behavior
        };
        let (slot, fresh) = match self.store.find(key) {
            Some(slot) => (slot, false),
            None => (self.store.insert(key, self.frame), true),
        };
        let parent = self.open.last().copied();
        let stored = self.store.get_mut(slot);
        stored.last_touched_frame = self.frame;
        stored.parent = parent.map(|parent| self.nodes[parent].key);
        stored.behavior = behavior;
        stored.state = state;
        stored.id = selector.id.map(NavId::new);

        let span = !matches!(content, Content::Block);
        let start = self.selectors.len();
        self.selectors.push_str(selector.text());
        let text = (start, self.selectors.len());
        let resolved = self.resolve_style(slot, fresh, parent, selector, span, inline);

        let font = match content {
            Content::Text { .. } => self.font_for(&resolved.style, selector.text()),
            Content::Block | Content::Image(_) => None,
        };
        let content_hash = match content {
            Content::Block => 0,
            Content::Text { start, end } => {
                let style = &resolved.style;
                // The resolved font's identity too: registering a font under
                // the span's family name, or replacing it, re-measures it.
                let hash = hash_of((
                    &self.text[start..end],
                    style.font_size.to_bits(),
                    style.font_family,
                    font.map(Font::id),
                    style.line_height.bits(),
                ));
                self.live_text.insert(hash);
                hash
            }
            Content::Image(image) => hash_of((image.width, image.height)),
        };

        let index = self.nodes.len();
        self.nodes.push(FrameNode {
            key,
            slot,
            parent,
            selector: text,
            pseudo: resolved.pseudo,
            inherited: resolved.inherited,
            style: resolved.style,
            content,
            font,
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
            focused,
            engagement,
            captured,
        } = self.store.get(node.slot).interaction;
        Response {
            key: node.key,
            hovered,
            pressed,
            clicked,
            focused,
            engagement,
            captured,
            changed: false,
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
    /// and hit that far up and left of where they are laid out. For an
    /// `overflow: scroll` block the offset is clamped at layout to how far its
    /// content reaches; any other block takes it as given. Does nothing
    /// outside every block.
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
        let store = &self.store;
        self.edits.retain(|key, _| store.find(*key).is_some());
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
        self.fit_text_inputs(atlas);
        self.clamp_scroll();
        self.place(origin);
    }

    /// Clamps every `overflow: scroll` block's offset to the reach its content
    /// was just laid out with, and records that reach for focus to scroll by.
    fn clamp_scroll(&mut self) {
        for node in &self.nodes {
            let stored = self.store.get_mut(node.slot);
            if node.style.overflow == Overflow::Scroll {
                let reach = Vec2::new(node.layout.scroll_width(), node.layout.scroll_height());
                stored.scroll_max = reach;
                stored.scroll_offset = stored.scroll_offset.clamp(Vec2::ZERO, reach);
            } else {
                stored.scroll_max = Vec2::ZERO;
            }
        }
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
                    let clip = if parent_node.style.overflow.clips() {
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

    /// How far `key`'s children are scrolled, as last frame left it; zero for a
    /// node the store does not hold.
    #[must_use]
    pub fn scroll_offset_of(&self, key: NodeKey) -> Vec2 {
        self.store
            .by_key(key)
            .map_or(Vec2::ZERO, |node| node.scroll_offset)
    }

    /// Scrolls `key`'s children to `offset`, whether or not that node is the
    /// one being built: [`Ui::set_scroll_offset`] by key, and the mirror of
    /// [`Ui::scroll_offset_of`]. Does nothing for a node the store does not
    /// hold.
    ///
    /// **This is how a caller scrolls a virtualized row into view.** A list or
    /// an outliner builds only the rows in its window, so a row the offset is
    /// nowhere near has no node and no rectangle — [`Ui::set_focus`]'s
    /// scroll-into-view, which works from a node's rectangle, cannot reach it.
    /// The caller knows the row's index and height and so knows where it is; it
    /// writes the offset here, and the next [`Ui::layout`] clamps it to the
    /// content's reach, as it clamps every offset it is given.
    pub fn set_scroll_offset_of(&mut self, key: NodeKey, offset: Vec2) {
        if let Some(slot) = self.store.find(key) {
            self.store.get_mut(slot).scroll_offset = offset;
        }
    }

    /// Scrolls the `overflow: scroll` blocks under this frame's pointer by a
    /// wheel's `delta`, in pixels, and says whether anything moved. Call it
    /// after [`Ui::begin_frame`], with the frame's wheel movement; positive `y`
    /// moves toward the end of the content, as a wheel turned toward the
    /// player does in a browser.
    ///
    /// **Each axis goes to the innermost block that can still move along it.**
    /// The pointer is hit-tested against last frame's rectangles, as
    /// [`Ui::begin_frame`] resolves hover, and the chain of blocks holding it is
    /// walked innermost first: a list scrolls until it is at its end, and only
    /// then does the wheel reach the panel around it — a browser's scroll
    /// chaining. Each block moves no further than the reach its last layout
    /// gave it.
    ///
    /// `false` when nothing under the pointer could move — so the caller can
    /// give the wheel to something else, such as a zoom bound to it.
    pub fn scroll_wheel(&mut self, delta: Vec2) -> bool {
        let chain = self.store.hit_chain(self.pointer.pos);
        let mut moved = false;
        for axis in 0..2 {
            let step = delta[axis];
            if step == 0.0 {
                continue;
            }
            for &key in &chain {
                let Some(slot) = self.store.find(key) else {
                    continue;
                };
                let node = self.store.get_mut(slot);
                let offset = node.scroll_offset[axis];
                let next = (offset + step).clamp(0.0, node.scroll_max[axis]);
                if next != offset {
                    node.scroll_offset[axis] = next;
                    moved = true;
                    break;
                }
            }
        }
        moved
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

    /// Binds `name` — what a stylesheet's `url(name)` says — to `image`, a
    /// picture registered in an [`ImageAtlas`](crate::image::ImageAtlas),
    /// replacing whatever the name was bound to.
    ///
    /// Read when the tree is emitted, so a binding changes what the next
    /// [`Ui::emit`] draws and never moves a box. A name no call bound draws
    /// nothing; see [`crate::style`]'s image notes.
    pub fn set_image(&mut self, name: &str, image: AtlasImage) {
        self.images.insert(ImageName::new(name), image);
    }

    /// Registers `font` under the family `name`, so a `font-family` list that
    /// names it ahead of its first built-in family measures and draws in it.
    ///
    /// `name` is matched as [`FamilyName`] says: ASCII case ignored, and an
    /// unquoted name's words joined by single spaces. **Registering a name
    /// again replaces its font**, as [`Ui::set_image`] replaces a picture, and
    /// every span in it is measured again. The registry is read when a span is
    /// built, so a span built this frame before the call keeps the font it was
    /// built in. See `crate::style`'s property notes for what a name nothing is
    /// registered under does.
    ///
    /// # Errors
    ///
    /// [`ReservedFamilyName`] for an empty name, a built-in family's (`bitmap`,
    /// `Atkinson Hyperlegible`), a generic family (`serif`, `sans-serif`,
    /// `monospace`, …) or a CSS-wide keyword: no `font-family` list selects a
    /// registered font by one of those, so the font would never draw.
    pub fn register_font(
        &mut self,
        name: &str,
        font: &'static Font,
    ) -> Result<(), ReservedFamilyName> {
        if is_reserved_family(name) {
            return Err(ReservedFamilyName(name.to_owned()));
        }
        self.fonts.insert(FamilyName::new(name), font);
        Ok(())
    }

    /// The parsed font a text span in `style` draws in: the one registered
    /// under its family name, else its built-in family's, `None` for the
    /// bitmap font. Warns once per name when the span names a family nothing is
    /// registered under; `selector` says which span.
    fn font_for(&mut self, style: &NodeStyle, selector: &str) -> Option<&'static Font> {
        if let Some(name) = style.family_name {
            if let Some(font) = self.fonts.get(&name) {
                return Some(*font);
            }
            if self.unregistered.insert(name) {
                crcbl_core::warn!(
                    "tree: the text span `{selector}` names a font family no font is registered \
                     under (Ui::register_font); it draws in {:?}",
                    style.font_family
                );
            }
        }
        style.font_family.font()
    }

    /// The keys two nodes shared this frame, each once.
    #[must_use]
    pub fn duplicate_keys(&self) -> &[NodeKey] {
        &self.duplicates
    }

    /// The key of the block whose builder is running: what a closure a widget
    /// calls back — a list's row, an outliner's row, a dock's pane — reads to
    /// learn which node it is filling. None outside every block.
    #[must_use]
    pub fn current_key(&self) -> Option<NodeKey> {
        self.open.last().map(|&index| self.nodes[index].key)
    }

    /// The keys of every node built inside `key` this frame, in build order;
    /// empty for a node this frame did not build. What a test or a UI
    /// inspector walks the tree with.
    #[must_use]
    pub fn child_keys(&self, key: NodeKey) -> Vec<NodeKey> {
        let Some(parent) = self.nodes.iter().position(|node| node.key == key) else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        let mut next = self.nodes[parent].first_child;
        while let Some(child) = next {
            keys.push(self.nodes[child].key);
            next = self.nodes[child].next_sibling;
        }
        keys
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
