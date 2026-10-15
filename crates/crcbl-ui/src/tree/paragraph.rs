//! Paragraphs: one node whose text is a sequence of runs, each styled as a
//! node of its own, broken into lines together — rich text that wraps.
//!
//! ```
//! use crcbl_ui::tree::{AvailableSpace, Ui};
//! use crcbl_ui::{FontAtlas, PointerInput};
//!
//! let atlas = FontAtlas::built_in();
//! let mut ui = Ui::new();
//! ui.add_stylesheet("note.css", "b { color: gold } a:hover { color: white }");
//! ui.begin_frame(PointerInput::default());
//! ui.block("", &[], |ui| {
//!     ui.paragraph(".body", &[], |p| {
//!         p.run("", "Build finished in ", &[]);
//!         p.run("b", "4 minutes", &[]);
//!         p.run("", ". ", &[]);
//!         if p.run("a#log", "Open the log", &[]).clicked {
//!             // Open it.
//!         }
//!     });
//! });
//! ui.layout(glam::Vec2::ZERO, AvailableSpace::MAX_CONTENT, &atlas);
//! ```
//!
//! # Runs are nodes
//!
//! [`ParagraphBuilder::run`] builds a node under the paragraph, keyed as a
//! span is — by its selector's `#id`, else its call site and how many runs
//! that call site has built — or by [`ParagraphBuilder::run_keyed`]'s key. Its
//! selector's type is `span` unless it names one, so markup maps onto the
//! stylesheet directly: `p.run("b", …)` for `<b>`, matched by `b { … }`. A run
//! is resolved as a span is, inheriting from the paragraph, which is a block;
//! **what a run takes from its style is its text's**: `color`, `font-*`,
//! `line-height`, `text-decoration` and `cursor`, and `display: none` leaves
//! it out. Its box properties — padding, margin, border, background — lay out
//! and draw nothing. The paragraph's own `text-align` aligns its lines and its
//! `white-space: nowrap` breaks them only at newlines.
//!
//! # Layout
//!
//! The paragraph is one Taffy leaf, measured like a span: unbroken for
//! max-content, broken at every break for min-content, and broken to fit a
//! definite width. `inline.rs` has the line breaker and the vertical metrics.
//! **`text-overflow: ellipsis` does not apply to a paragraph**: a cut that
//! respects every run's font is not built.
//!
//! # Interaction
//!
//! Each run is hit on its **fragments** — one rectangle per line it lies on,
//! from its line's top to its bottom — so a link that wraps onto two lines is
//! one node hit on both, and nowhere between them. A run is the topmost node
//! under the pointer over its text, so its [`Response`] reports the click and
//! the hover, `:hover` and `:active` style it, and its `cursor` is the one
//! [`Ui::cursor`] reports; the paragraph and everything around it are hovered
//! with it, as a span's ancestors are. [`Ui::rect`] of a run is the box round
//! all its fragments.

use std::hash::Hash;
use std::panic::Location;

use glam::Vec2;
use taffy::{AvailableSpace, Size};

use super::emit::content_width;
use super::inline::{InlineLayout, InlineRun, RunFont, Wrap};
use super::style::{Display, WhiteSpace};
use super::{Behavior, Content, FrameNode, KeySource, Response, Ui, hash_of};
use crate::draw_list::DrawList;
use crate::style::{Declaration, NodeSelector, PseudoClasses};
use crate::text::{ASCENDER, FontAtlas};
use crate::widget::NATURAL_FONT_SIZE;

/// Builds a paragraph's runs; [`Ui::paragraph`] hands one to its closure.
#[derive(Debug)]
pub struct ParagraphBuilder<'a> {
    ui: &'a mut Ui,
}

impl ParagraphBuilder<'_> {
    /// A run of `text`, keyed by its selector's `#id` or else by this call
    /// site. `selector` and `inline` are [`Ui::span`]'s. See the module docs.
    #[track_caller]
    pub fn run(&mut self, selector: &str, text: &str, inline: &[Declaration]) -> Response {
        let parsed = self.ui.node_selector(selector);
        let key = self.ui.selector_key(parsed, Location::caller());
        self.ui.push_run(key, parsed, text, inline)
    }

    /// A run keyed by `key`, for a run a loop builds — a link per item — so
    /// its hover and press follow the item rather than its position.
    /// `selector` and `inline` are [`ParagraphBuilder::run`]'s, and an `#id`
    /// in the selector does not key it.
    pub fn run_keyed(
        &mut self,
        key: impl Hash,
        selector: &str,
        text: &str,
        inline: &[Declaration],
    ) -> Response {
        let parsed = self.ui.node_selector(selector);
        let key = self.ui.key(KeySource::Keyed(hash_of(key)));
        self.ui.push_run(key, parsed, text, inline)
    }
}

/// A paragraph as the last layout broke it.
#[derive(Clone, Debug)]
pub(super) struct LaidOut {
    /// The paragraph's index in this frame's nodes.
    pub paragraph: usize,
    /// Each laid-out run's index in this frame's nodes, by its place in the
    /// layout.
    pub runs: Vec<usize>,
    pub layout: InlineLayout,
}

impl Ui {
    /// A paragraph: a block whose content is the runs `build` adds, broken
    /// into lines together at its width. `selector` and `inline` are
    /// [`Ui::block`]'s, and its type is `block` unless the selector names one.
    /// See `paragraph.rs` for the rules.
    #[track_caller]
    pub fn paragraph(
        &mut self,
        selector: &str,
        inline: &[Declaration],
        build: impl FnOnce(&mut ParagraphBuilder<'_>),
    ) -> Response {
        let parsed = self.node_selector(selector);
        let key = self.selector_key(parsed, Location::caller());
        let index = self.push(
            key,
            parsed,
            inline,
            Behavior::NONE,
            PseudoClasses::NONE,
            Content::Paragraph,
        );
        let response = self.response(index);
        self.open.push(index);
        build(&mut ParagraphBuilder { ui: self });
        self.open.pop();

        // What the paragraph measures from: each run's text and the style
        // that sizes it, whether it is laid out at all, and its own
        // `white-space`.
        let mut hash = hash_of(self.nodes[index].style.white_space);
        let mut next = self.nodes[index].first_child;
        while let Some(run) = next {
            let node = &self.nodes[run];
            hash = hash_of((hash, node.content_hash, node.style.display));
            next = node.next_sibling;
        }
        self.nodes[index].content_hash = hash;
        self.live_text.insert(hash);
        self.close(index);
        response
    }

    fn push_run(
        &mut self,
        key: super::NodeKey,
        selector: NodeSelector<'_>,
        text: &str,
        inline: &[Declaration],
    ) -> Response {
        let start = self.text.len();
        self.text.push_str(text);
        let content = Content::Run {
            start,
            end: self.text.len(),
        };
        let index = self.push(
            key,
            selector,
            inline,
            Behavior::NONE,
            PseudoClasses::NONE,
            content,
        );
        self.close(index);
        self.response(index)
    }

    /// Breaks every paragraph into lines at the width layout gave its content
    /// box — the unrounded one, as a span is broken — and aligns them in the
    /// rounded one, for placement and emission.
    pub(super) fn lay_out_paragraphs(&mut self, atlas: &FontAtlas) {
        self.paragraphs.clear();
        for (paragraph, node) in self.nodes.iter().enumerate() {
            if node.content != Content::Paragraph {
                continue;
            }
            let (runs, inputs) = inline_runs(&self.nodes, &self.text, paragraph);
            let wrap = match node.style.white_space {
                WhiteSpace::Normal => {
                    Wrap::Width(content_width(&self.store.get(node.slot).unrounded))
                }
                WhiteSpace::NoWrap => Wrap::None,
            };
            let mut layout = InlineLayout::new(&inputs, wrap, atlas);
            layout.align(content_width(&node.layout), node.style.text_align);
            self.paragraphs.push(LaidOut {
                paragraph,
                runs,
                layout,
            });
        }
    }

    /// Writes each run's fragments, in screen pixels, into the store for the
    /// next frame's hit test, and its rectangle as the box round them.
    pub(super) fn place_runs(&mut self) {
        for laid in &self.paragraphs {
            for &run in &laid.runs {
                self.store.get_mut(self.nodes[run].slot).fragments.clear();
            }
            let origin = self.content_origin(laid.paragraph);
            for fragment in laid.layout.fragments() {
                let line = &laid.layout.lines()[fragment.line];
                let min = origin + Vec2::new(fragment.x, line.top);
                let max = min + Vec2::new(fragment.width, line.height);
                let stored = self.store.get_mut(self.nodes[laid.runs[fragment.run]].slot);
                stored.rect = if stored.fragments.is_empty() {
                    (min, max)
                } else {
                    (stored.rect.0.min(min), stored.rect.1.max(max))
                };
                stored.fragments.push((min, max));
            }
        }
    }

    /// The top-left of `index`'s content box, as last placed.
    fn content_origin(&self, index: usize) -> Vec2 {
        let node = &self.nodes[index];
        let (border, padding) = (node.layout.border, node.layout.padding);
        self.store.get(node.slot).rect.0
            + Vec2::new(border.left + padding.left, border.top + padding.top)
    }

    /// Draws the paragraph `index`'s runs, fragment by fragment, each in its
    /// own run's style.
    pub(super) fn emit_paragraph(&self, index: usize, list: &mut DrawList) {
        let Some(laid) = self.paragraphs.iter().find(|laid| laid.paragraph == index) else {
            return;
        };
        let origin = self.content_origin(index);
        for fragment in laid.layout.fragments() {
            let run = &self.nodes[laid.runs[fragment.run]];
            let Content::Run { start, end } = run.content else {
                continue;
            };
            let text = &self.text[start..end][fragment.text.clone()];
            let style = &run.style;
            match &run.font {
                Some(font) => {
                    let glyphs = laid.layout.positioned(fragment);
                    if !glyphs.is_empty() {
                        list.text_glyphs(origin, font, style.font_size, style.color, glyphs, text);
                    }
                }
                None => {
                    let line = &laid.layout.lines()[fragment.line];
                    let scale = style.font_size / NATURAL_FONT_SIZE;
                    let top = line.baseline - ASCENDER as f32 * scale;
                    list.text(
                        origin + Vec2::new(fragment.x, top),
                        text,
                        style.color,
                        style.font_size,
                    );
                }
            }
        }
    }
}

/// The paragraph `paragraph`'s runs that are laid out — every run under it
/// but those `display: none` hides — as their node indices and as the line
/// breaker reads them.
pub(super) fn inline_runs<'a>(
    nodes: &'a [FrameNode],
    text: &'a str,
    paragraph: usize,
) -> (Vec<usize>, Vec<InlineRun<'a>>) {
    let mut indices = Vec::new();
    let mut runs = Vec::new();
    let mut next = nodes[paragraph].first_child;
    while let Some(index) = next {
        let node = &nodes[index];
        next = node.next_sibling;
        let Content::Run { start, end } = node.content else {
            continue;
        };
        if node.style.display == Display::None {
            continue;
        }
        let style = &node.style;
        let run = match &node.font {
            Some(font) => InlineRun::new(
                &text[start..end],
                RunFont::Parsed(font),
                style.font_size,
                style.text_line_height(font),
            ),
            None => InlineRun::new(
                &text[start..end],
                RunFont::Bitmap(style.font_size / NATURAL_FONT_SIZE),
                style.font_size,
                0.0,
            ),
        };
        indices.push(index);
        runs.push(run);
    }
    (indices, runs)
}

/// The size `runs` take under a `width` constraint, for a paragraph whose
/// `white-space` is `white_space`.
pub(super) fn measure_runs(
    runs: &[InlineRun<'_>],
    white_space: WhiteSpace,
    width: AvailableSpace,
    atlas: &FontAtlas,
) -> Size<f32> {
    let wrap = match (white_space, width) {
        (WhiteSpace::NoWrap, _) | (_, AvailableSpace::MaxContent) => Wrap::None,
        (WhiteSpace::Normal, AvailableSpace::MinContent) => Wrap::MinContent,
        (WhiteSpace::Normal, AvailableSpace::Definite(width)) => Wrap::Width(width),
    };
    let measured = InlineLayout::new(runs, wrap, atlas).measure();
    Size {
        width: measured.x,
        height: measured.y,
    }
}
