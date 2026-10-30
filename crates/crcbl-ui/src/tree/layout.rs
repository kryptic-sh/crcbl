//! Taffy's low-level traits, implemented over this frame's nodes and the
//! persistent store.
//!
//! [`LayoutTree`] is a borrow of [`super::Ui`]'s parts for one layout pass:
//! the frame's nodes are the tree Taffy traverses, their [`NodeStyle`]s are
//! the styles it reads, and the store is where each node's cache and unrounded
//! layout live, so a cache hit next frame finds what this frame wrote.
//!
//! The dispatch in [`LayoutPartialTree::compute_child_layout`] is the one
//! `taffy::TaffyTree` makes for the same two layout modes: a hidden run hides,
//! `display: none` hides, a node with children is a flex container and a node
//! without is a leaf measured by its content.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use taffy::{
    AvailableSpace, Cache, CacheTree, Layout, LayoutFlexboxContainer, LayoutInput, LayoutOutput,
    LayoutPartialTree, NodeId, RoundTree, RunMode, Size, TraversePartialTree, TraverseTree,
    compute_cached_layout, compute_flexbox_layout, compute_hidden_layout, compute_leaf_layout,
};

use super::ellipsis::KeptCut;
use super::paragraph::{inline_runs, measure_runs};
use super::store::NodeStore;
use super::style::{Display, NodeStyle, WhiteSpace};
use super::{Content, FrameNode};
use crate::font::Font;
use crate::font::layout::{TextLayout, wrap_width};
use crate::text::{FontAtlas, LINE_HEIGHT};
use crate::widget::NATURAL_FONT_SIZE;

/// Which width a text measurement was asked under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WidthBucket {
    MinContent,
    MaxContent,
    /// The bits of [`wrap_width`] of the width: whole pixels, the widths lines
    /// are broken at, so every width in one bucket measures exactly the same.
    Definite(u32),
    /// Every width at once: text that measures the same under any of them.
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeasureKey {
    /// The span's content hash: its text, and every style field that sizes it.
    text: u64,
    width: WidthBucket,
}

/// Text measurements, keyed by (text and its font, size and line height, width
/// bucket), on Clay's measure-cache pattern.
///
/// A flex pass asks a leaf for its min-content and max-content size and again
/// at its final width, and a text span answers all three from one
/// measurement here. Entries for text no live span holds are dropped at
/// layout.
///
/// Text in a parsed font is laid out by [`TextLayout`]: its max-content size
/// unbroken, its min-content size broken at every space, and at a definite
/// width broken to fit it. **Text that measures the same under every width is
/// measured once**, in one bucket for them all: text in the bitmap font, which
/// never wraps, and `white-space: nowrap` text, laid out unbroken under every
/// width. A paragraph is measured the same way, under the hash of its runs, by
/// its line breaker in `inline.rs`.
///
/// **A `nowrap` span's layout is kept with its size**, so cutting it to fit
/// ([`super::ellipsis`]) and drawing it shape nothing again: one layout per
/// live `nowrap` span in a parsed font, held as long as its text is. **So is
/// the cut** an ellipsis span's text was last cut to fit with, and the width it
/// was cut at, so a steady frame cuts none of its spans again.
#[derive(Clone, Debug, Default)]
pub(crate) struct MeasureCache {
    entries: HashMap<MeasureKey, Measured>,
    /// Measurements answered from `entries`.
    pub hits: u64,
    /// Measurements that went to the font.
    pub misses: u64,
}

impl MeasureCache {
    /// `text`'s natural size in `style`, under a `width` constraint: in
    /// `font`, or in the bitmap font when that is `None`.
    fn text(
        &mut self,
        atlas: &FontAtlas,
        font: Option<&Font>,
        text: &str,
        content_hash: u64,
        style: &NodeStyle,
        width: AvailableSpace,
    ) -> Size<f32> {
        let bucket = match (font, style.white_space) {
            (None, _) | (Some(_), WhiteSpace::NoWrap) => WidthBucket::Any,
            (Some(_), WhiteSpace::Normal) => bucket(width),
        };
        let key = MeasureKey {
            text: content_hash,
            width: bucket,
        };
        self.entry(key, || match font {
            None => {
                let scale = style.font_size / NATURAL_FONT_SIZE;
                Measured::size(Size {
                    width: atlas.text_width(text, scale),
                    height: atlas.line_count(text) as f32 * LINE_HEIGHT * scale,
                })
            }
            Some(font) => {
                let (size, line_height) = (style.font_size, style.text_line_height(font));
                let wrap = match (style.white_space, width) {
                    (WhiteSpace::NoWrap, _) => {
                        let layout = TextLayout::nowrap(font, text, size, line_height);
                        return Measured {
                            size: size_of(&layout),
                            layout: Some(Arc::new(layout)),
                            cut: None,
                        };
                    }
                    (WhiteSpace::Normal, AvailableSpace::MaxContent) => None,
                    (WhiteSpace::Normal, AvailableSpace::MinContent) => Some(0.0),
                    (WhiteSpace::Normal, AvailableSpace::Definite(width)) => Some(width),
                };
                Measured::size(size_of(&TextLayout::new(
                    font,
                    text,
                    size,
                    line_height,
                    wrap,
                )))
            }
        })
        .size
    }

    /// The layout a `white-space: nowrap` span of content `content_hash` was
    /// measured from — `text` laid out by [`TextLayout::nowrap`] in `font` at
    /// `size` and `line_height` — when one is held.
    ///
    /// The layout is checked against all four rather than trusted to the hash,
    /// so a held one is never another text's, nor the same text's in another
    /// font, fallback faces and all, or at another size or line height.
    pub fn nowrap_layout(
        &self,
        content_hash: u64,
        font: &Font,
        text: &str,
        size: f32,
        line_height: f32,
    ) -> Option<&Arc<TextLayout>> {
        self.nowrap(content_hash, font, text, size, line_height)?
            .layout
            .as_ref()
    }

    /// The cut kept for a `white-space: nowrap` span of content
    /// `content_hash` cut at `width`, when one is: only beside a layout of
    /// `text` in `font` at `size` and `line_height`, checked as
    /// [`MeasureCache::nowrap_layout`] checks it, and only at the same width,
    /// to the bit.
    pub fn kept_cut(
        &self,
        content_hash: u64,
        font: &Font,
        text: &str,
        size: f32,
        line_height: f32,
        width: f32,
    ) -> Option<&KeptCut> {
        self.nowrap(content_hash, font, text, size, line_height)?
            .cut
            .as_ref()
            .filter(|cut| cut.width == width.to_bits())
    }

    /// Keeps `cut` beside the layout of the `nowrap` span of content
    /// `content_hash`, in place of any cut kept before. Does nothing when no
    /// layout is held for it, so a cut is never kept that
    /// [`MeasureCache::kept_cut`] could not check.
    pub fn keep_cut(&mut self, content_hash: u64, cut: KeptCut) {
        let key = MeasureKey {
            text: content_hash,
            width: WidthBucket::Any,
        };
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.layout.is_some()
        {
            entry.cut = Some(cut);
        }
    }

    /// The entry of the `nowrap` span of content `content_hash`, when it
    /// holds a layout of `text` in `font` — the same fallback faces and all —
    /// at `size` and `line_height`: checked against all four rather than
    /// trusted to the hash, so it is never another text's.
    fn nowrap(
        &self,
        content_hash: u64,
        font: &Font,
        text: &str,
        size: f32,
        line_height: f32,
    ) -> Option<&Measured> {
        let key = MeasureKey {
            text: content_hash,
            width: WidthBucket::Any,
        };
        self.entries.get(&key).filter(|entry| {
            entry
                .layout
                .as_ref()
                .is_some_and(|layout| layout.lays_out(font, text, size, line_height))
        })
    }

    /// The measurement held for content `content_hash` under `width`, or
    /// `measure`'s, held from now on.
    pub fn cached(
        &mut self,
        content_hash: u64,
        width: AvailableSpace,
        measure: impl FnOnce() -> Size<f32>,
    ) -> Size<f32> {
        let key = MeasureKey {
            text: content_hash,
            width: bucket(width),
        };
        self.entry(key, || Measured::size(measure())).size
    }

    /// The entry held under `key`, or `measure`'s, held from now on.
    fn entry(&mut self, key: MeasureKey, measure: impl FnOnce() -> Measured) -> &Measured {
        // Counted before the entry is borrowed, which ends the borrow `get`
        // would hold across the insert.
        if self.entries.contains_key(&key) {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        self.entries.entry(key).or_insert_with(measure)
    }

    /// Drops every entry for text not in `live`.
    pub fn retain(&mut self, live: &HashSet<u64>) {
        self.entries.retain(|key, _| live.contains(&key.text));
    }

    /// How many different strings have measurements held.
    #[cfg(test)]
    pub fn texts(&self) -> usize {
        self.entries
            .keys()
            .map(|key| key.text)
            .collect::<HashSet<_>>()
            .len()
    }
}

/// One measurement held.
#[derive(Clone, Debug)]
struct Measured {
    size: Size<f32>,
    /// What a `nowrap` span in a parsed font was measured from: its text laid
    /// out by [`TextLayout::nowrap`]. `None` for every other measurement.
    layout: Option<Arc<TextLayout>>,
    /// What `layout`'s text was last cut to under `text-overflow: ellipsis`;
    /// `None` until it is cut, and for every measurement without a layout.
    cut: Option<KeptCut>,
}

impl Measured {
    /// A measurement with no layout kept.
    const fn size(size: Size<f32>) -> Self {
        Self {
            size,
            layout: None,
            cut: None,
        }
    }
}

/// The bucket a measurement under `width` is held in, for content that
/// measures differently under different widths.
fn bucket(width: AvailableSpace) -> WidthBucket {
    match width {
        AvailableSpace::MinContent => WidthBucket::MinContent,
        AvailableSpace::MaxContent => WidthBucket::MaxContent,
        AvailableSpace::Definite(width) => WidthBucket::Definite(wrap_width(width).to_bits()),
    }
}

/// The box `layout` needs, as Taffy takes a size.
fn size_of(layout: &TextLayout) -> Size<f32> {
    let measured = layout.measure();
    Size {
        width: measured.x,
        height: measured.y,
    }
}

/// One layout pass's view of a [`super::Ui`].
pub(crate) struct LayoutTree<'a> {
    pub nodes: &'a mut [FrameNode],
    /// Every node's children, flattened: a node's are
    /// `children[child_start..child_start + child_count]`.
    pub children: &'a [NodeId],
    pub store: &'a mut NodeStore,
    pub text: &'a str,
    pub measure: &'a mut MeasureCache,
    pub atlas: &'a FontAtlas,
}

impl LayoutTree<'_> {
    fn node(&self, id: NodeId) -> &FrameNode {
        &self.nodes[usize::from(id)]
    }

    fn cache(&mut self, id: NodeId) -> &mut Cache {
        let slot = self.nodes[usize::from(id)].slot;
        &mut self.store.get_mut(slot).cache
    }
}

impl TraversePartialTree for LayoutTree<'_> {
    type ChildIter<'a>
        = core::iter::Copied<core::slice::Iter<'a, NodeId>>
    where
        Self: 'a;

    fn child_ids(&self, parent: NodeId) -> Self::ChildIter<'_> {
        let node = self.node(parent);
        self.children[node.child_start..node.child_start + node.child_count]
            .iter()
            .copied()
    }

    fn child_count(&self, parent: NodeId) -> usize {
        self.node(parent).child_count
    }

    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        self.children[self.node(parent).child_start + index]
    }
}

impl TraverseTree for LayoutTree<'_> {}

impl LayoutPartialTree for LayoutTree<'_> {
    type CoreContainerStyle<'a>
        = &'a NodeStyle
    where
        Self: 'a;

    type CustomIdent = String;

    fn get_core_container_style(&self, node: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.node(node).style
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        let slot = self.node(node).slot;
        self.store.get_mut(slot).unrounded = *layout;
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node);
        }
        compute_cached_layout(self, node, inputs, |tree, node, inputs| {
            let index = usize::from(node);
            match (
                tree.nodes[index].style.display,
                tree.nodes[index].child_count,
            ) {
                (Display::None, _) => compute_hidden_layout(tree, node),
                (Display::Flex, 1..) => compute_flexbox_layout(tree, node, inputs),
                (Display::Flex, 0) => {
                    let LayoutTree {
                        nodes,
                        text,
                        measure,
                        atlas,
                        ..
                    } = tree;
                    let frame = &nodes[index];
                    compute_leaf_layout(
                        inputs,
                        &frame.style,
                        // Nothing in the subset is a `calc()`.
                        |_, _| 0.0,
                        |known, available| {
                            // The width the node is given, else the space it
                            // may take: what a wrapped line breaks at.
                            let width = known
                                .width
                                .map_or(available.width, AvailableSpace::Definite);
                            let natural = match frame.content {
                                // A run is laid out by its paragraph.
                                Content::Block | Content::Run { .. } => Size::ZERO,
                                Content::Text { start, end } => measure.text(
                                    atlas,
                                    frame.font.as_ref(),
                                    &text[start..end],
                                    frame.content_hash,
                                    &frame.style,
                                    width,
                                ),
                                Content::Paragraph => {
                                    measure.cached(frame.content_hash, width, || {
                                        let (_, runs) = inline_runs(nodes, text, index);
                                        measure_runs(&runs, frame.style.white_space, width, atlas)
                                    })
                                }
                                Content::Image(image) => Size {
                                    width: image.width as f32,
                                    height: image.height as f32,
                                },
                            };
                            Size {
                                width: known.width.unwrap_or(natural.width),
                                height: known.height.unwrap_or(natural.height),
                            }
                        },
                    )
                }
            }
        })
    }
}

impl CacheTree for LayoutTree<'_> {
    fn cache_get(&mut self, node: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.cache(node).get(input)
    }

    fn cache_store(&mut self, node: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.cache(node).store(input, output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.cache(node).clear();
    }
}

impl LayoutFlexboxContainer for LayoutTree<'_> {
    type FlexboxContainerStyle<'a>
        = &'a NodeStyle
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a NodeStyle
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.node(node).style
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.node(child).style
    }
}

impl RoundTree for LayoutTree<'_> {
    fn get_unrounded_layout(&self, node: NodeId) -> Layout {
        self.store.get(self.node(node).slot).unrounded
    }

    fn set_final_layout(&mut self, node: NodeId, layout: &Layout) {
        self.nodes[usize::from(node)].layout = *layout;
    }
}
