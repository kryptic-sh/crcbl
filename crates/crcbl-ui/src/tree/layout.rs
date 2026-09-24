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

use taffy::{
    AvailableSpace, Cache, CacheTree, Layout, LayoutFlexboxContainer, LayoutInput, LayoutOutput,
    LayoutPartialTree, NodeId, RoundTree, RunMode, Size, TraversePartialTree, TraverseTree,
    compute_cached_layout, compute_flexbox_layout, compute_hidden_layout, compute_leaf_layout,
};

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
/// Text in the bitmap font never wraps, so it measures the same in every
/// bucket. Text in a parsed font is laid out by [`TextLayout`]: its max-content
/// size unbroken, its min-content size broken at every space, and at a definite
/// width broken to fit it — unless it is `white-space: nowrap`, which measures
/// unbroken under every width.
#[derive(Clone, Debug, Default)]
pub(crate) struct MeasureCache {
    entries: HashMap<MeasureKey, Size<f32>>,
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
        let key = MeasureKey {
            text: content_hash,
            width: match width {
                AvailableSpace::MinContent => WidthBucket::MinContent,
                AvailableSpace::MaxContent => WidthBucket::MaxContent,
                AvailableSpace::Definite(width) => {
                    WidthBucket::Definite(wrap_width(width).to_bits())
                }
            },
        };
        if let Some(size) = self.entries.get(&key) {
            self.hits += 1;
            return *size;
        }
        self.misses += 1;
        let size = match font {
            None => {
                let scale = style.font_size / NATURAL_FONT_SIZE;
                Size {
                    width: atlas.text_width(text, scale),
                    height: atlas.line_count(text) as f32 * LINE_HEIGHT * scale,
                }
            }
            Some(font) => {
                let wrap = match (style.white_space, width) {
                    (WhiteSpace::NoWrap, _) | (_, AvailableSpace::MaxContent) => None,
                    (WhiteSpace::Normal, AvailableSpace::MinContent) => Some(0.0),
                    (WhiteSpace::Normal, AvailableSpace::Definite(width)) => Some(width),
                };
                let measured = TextLayout::new(
                    font,
                    text,
                    style.font_size,
                    style.text_line_height(font),
                    wrap,
                )
                .measure();
                Size {
                    width: measured.x,
                    height: measured.y,
                }
            }
        };
        self.entries.insert(key, size);
        size
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
                            let natural = match frame.content {
                                Content::Block => Size::ZERO,
                                // The width the span is given, else the space
                                // it may take: what a wrapped line breaks at.
                                Content::Text { start, end } => measure.text(
                                    atlas,
                                    frame.font,
                                    &text[start..end],
                                    frame.content_hash,
                                    &frame.style,
                                    known
                                        .width
                                        .map_or(available.width, AvailableSpace::Definite),
                                ),
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
