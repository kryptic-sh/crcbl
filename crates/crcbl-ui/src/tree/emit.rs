//! Emission: a laid-out tree into a [`DrawList`], in paint order.
//!
//! See the module docs of [`crate::tree`] for what each kind of node draws.

use glam::Vec2;

use super::style::{Display, NodeStyle};
use super::{Content, Ui, padding_box};
use crate::draw_list::{Border, CornerRadii, DrawList};
use crate::font::layout::TextLayout;

/// Whether a colour draws anything.
fn visible(color: [f32; 4]) -> bool {
    color[3] > 0.0
}

impl Ui {
    /// Draws the tree [`Ui::layout`] last laid out into `list`: every root in
    /// build order, each parent before its children.
    ///
    /// Clips are pushed and popped in pairs, so `list`'s own clip is what it
    /// was when this returns. The navigation debug overlay follows the tree
    /// when [`Ui::set_nav_debug`] switched it on.
    pub fn emit(&self, list: &mut DrawList) {
        for (index, node) in self.nodes.iter().enumerate() {
            if node.parent.is_none() {
                self.emit_node(index, list);
            }
        }
        // Outlines after the whole tree, as a browser paints them in a later
        // phase: a focus ring a later sibling covered would show focus on
        // nothing. Each is still clipped as its node was.
        for node in &self.nodes {
            let stored = self.store.get(node.slot);
            if !stored.hittable || !has_outline(&node.style) {
                continue;
            }
            let (min, max) = stored.rect;
            list.push_clip(stored.clip.min, stored.clip.max);
            paint_outline(list, &node.style, min, max);
            list.pop_clip()
                .expect("the clip pushed above is still on the stack");
        }
        self.emit_nav_debug(list);
    }

    fn emit_node(&self, index: usize, list: &mut DrawList) {
        let node = &self.nodes[index];
        if node.style.display == Display::None {
            return;
        }
        let (min, max) = self.store.get(node.slot).rect;
        match node.content {
            Content::Block => paint_box(list, &node.style, min, max),
            Content::Text { start, end } => {
                let (content_min, content_max) = content_box(min, &node.layout);
                let text = &self.text[start..end];
                let style = &node.style;
                match style.font_family.font() {
                    None => list.text(content_min, text, style.color, style.font_size),
                    Some(font) => {
                        // Broken at the width the layout measured it under —
                        // the unrounded one — and aligned in the box it is
                        // drawn in, the rounded one.
                        let unrounded = &self.store.get(node.slot).unrounded;
                        let mut layout = TextLayout::new(
                            font,
                            text,
                            style.font_size,
                            style.text_line_height(font),
                            Some(content_width(unrounded)),
                        );
                        layout.align(content_max.x - content_min.x, style.text_align);
                        list.glyphs(
                            content_min,
                            font,
                            style.font_size,
                            style.color,
                            layout.glyphs(),
                        );
                    }
                }
            }
            Content::Image(image) => {
                let (content_min, content_max) = content_box(min, &node.layout);
                list.image(content_min, content_max, &image, node.style.color);
            }
        }

        let clipped = node.style.overflow.clips();
        if clipped {
            let (clip_min, clip_max) = padding_box(min, &node.layout);
            list.push_clip(clip_min, clip_max);
        }
        for child in &self.children[node.child_start..node.child_start + node.child_count] {
            self.emit_node(usize::from(*child), list);
        }
        if clipped {
            list.pop_clip()
                .expect("the clip pushed above is still on the stack");
        }
    }
}

/// Whether a node's outline draws anything.
fn has_outline(style: &NodeStyle) -> bool {
    style.outline_width > 0.0 && visible(style.outline_color)
}

/// A node's outline: a ring `outline-offset` outside its border box `min..max`,
/// `outline-width` wide, with its corners' radii grown by the offset.
fn paint_outline(list: &mut DrawList, style: &NodeStyle, min: Vec2, max: Vec2) {
    let width = style.outline_width;
    let grow = Vec2::splat(style.outline_offset + width);
    let (min, max) = (min - grow, max + grow);
    if style.radii == CornerRadii::uniform(0.0) {
        list.rect_outline(min, max, width, style.outline_color);
        return;
    }
    let spread = |radius: f32| {
        if radius > 0.0 {
            (radius + grow.x).max(0.0)
        } else {
            0.0
        }
    };
    let radii = style.radii;
    list.rounded_rect(
        min,
        max,
        CornerRadii {
            top_left: spread(radii.top_left),
            top_right: spread(radii.top_right),
            bottom_right: spread(radii.bottom_right),
            bottom_left: spread(radii.bottom_left),
        },
        [0.0; 4],
        Border {
            width,
            color: style.outline_color,
        },
    );
}

/// A block's background and border over its border box `min..max`.
fn paint_box(list: &mut DrawList, style: &NodeStyle, min: Vec2, max: Vec2) {
    let border = style.border;
    let has_border = visible(style.border_color)
        && [border.top, border.right, border.bottom, border.left]
            .iter()
            .any(|width| *width > 0.0);

    if style.radii != CornerRadii::uniform(0.0) {
        if visible(style.background) || has_border {
            list.rounded_rect(
                min,
                max,
                style.radii,
                style.background,
                Border {
                    width: if has_border { border.top } else { 0.0 },
                    color: style.border_color,
                },
            );
        }
        return;
    }

    if visible(style.background) {
        list.rect(min, max, style.background);
    }
    if !has_border {
        return;
    }
    let uniform =
        border.top == border.right && border.top == border.bottom && border.top == border.left;
    if uniform {
        list.rect_outline(min, max, border.top, style.border_color);
        return;
    }
    // Top and bottom across the whole width, left and right between them — the
    // same four bands `DrawCommand::RectOutline` expands to.
    let bands = [
        (min, Vec2::new(max.x, min.y + border.top)),
        (Vec2::new(min.x, max.y - border.bottom), max),
        (
            Vec2::new(min.x, min.y + border.top),
            Vec2::new(min.x + border.left, max.y - border.bottom),
        ),
        (
            Vec2::new(max.x - border.right, min.y + border.top),
            Vec2::new(max.x, max.y - border.bottom),
        ),
    ];
    for (band_min, band_max) in bands {
        if band_max.x > band_min.x && band_max.y > band_min.y {
            list.rect(band_min, band_max, style.border_color);
        }
    }
}

/// A layout's content-box width.
pub(super) fn content_width(layout: &taffy::Layout) -> f32 {
    layout.size.width
        - layout.padding.left
        - layout.padding.right
        - layout.border.left
        - layout.border.right
}

/// A node's content box, given its border box's top-left.
fn content_box(min: Vec2, layout: &taffy::Layout) -> (Vec2, Vec2) {
    let (padding_min, padding_max) = padding_box(min, layout);
    let padding = layout.padding;
    (
        padding_min + Vec2::new(padding.left, padding.top),
        padding_max - Vec2::new(padding.right, padding.bottom),
    )
}
