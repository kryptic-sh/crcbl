//! Emission: a laid-out tree into a [`DrawList`], in paint order.
//!
//! See the module docs of [`crate::tree`] for what each kind of node draws.

use std::collections::HashMap;

use glam::Vec2;

use super::style::{Display, ImageName, NodeStyle, TextDecoration, WhiteSpace};
use super::{Content, Ui, padding_box};
use crate::draw_list::{Border, CornerRadii, DrawList};
use crate::font::layout::{PositionedGlyph, TextLayout};
use crate::font::{Font, GlyphId};
use crate::image::{AtlasImage, NineSliceImage};
use crate::text::{ASCENDER, GLYPH_ADVANCE, GLYPH_HEIGHT, LINE_HEIGHT, NOTDEF_INDEX, glyph_index};
use crate::widget::{NATURAL_FONT_SIZE, SkinInsets};

/// How far below the baseline an underline's top is, in ems, for a font whose
/// `post` table does not say and for the bitmap font: about where a text
/// face's own table puts it.
pub(super) const FALLBACK_UNDERLINE_OFFSET_EM: f32 = 0.1;

/// How thick that underline is, in ems.
pub(super) const FALLBACK_UNDERLINE_THICKNESS_EM: f32 = 1.0 / 16.0;

/// The thinnest underline drawn, in pixels: what keeps one at a small size
/// covering a row of pixels rather than falling between two.
pub(super) const MIN_UNDERLINE_THICKNESS: f32 = 1.0;

/// The rectangle underlining `width` pixels of text from `left`, on the
/// baseline at `baseline`, in `font` — the bitmap font for `None` — at `size`
/// pixels per em: where the font's `post` table says, else the fallback above.
pub(super) fn underline(
    font: Option<&Font>,
    size: f32,
    baseline: f32,
    left: f32,
    width: f32,
) -> (Vec2, Vec2) {
    let (top, thickness) = match font.and_then(|font| Some((font, font.underline()?))) {
        Some((font, line)) => {
            let scale = font.metrics().scale(size);
            (baseline - line.offset * scale, line.thickness * scale)
        }
        None => (
            baseline + size * FALLBACK_UNDERLINE_OFFSET_EM,
            size * FALLBACK_UNDERLINE_THICKNESS_EM,
        ),
    };
    let thickness = thickness.max(MIN_UNDERLINE_THICKNESS);
    (
        Vec2::new(left, top),
        Vec2::new(left + width, top + thickness),
    )
}

/// Underlines each of a text span's `lines`, `(left, baseline, width)` from
/// `origin`, when its style says to.
fn underline_lines(
    list: &mut DrawList,
    style: &NodeStyle,
    font: Option<&Font>,
    origin: Vec2,
    lines: impl Iterator<Item = (f32, f32, f32)>,
) {
    if style.text_decoration != TextDecoration::Underline {
        return;
    }
    for (left, baseline, width) in lines {
        if width > 0.0 {
            let (min, max) = underline(font, style.font_size, baseline, left, width);
            list.rect(origin + min, origin + max, style.color);
        }
    }
}

/// Whether a colour draws anything.
fn visible(color: [f32; 4]) -> bool {
    color[3] > 0.0
}

impl Ui {
    /// Draws `text` in the bitmap font at `pos`, `size` pixels high: its own
    /// glyphs, and in the cell of each char it has only `.notdef` for, the
    /// first of [`Ui::set_fallback_fonts`]'s faces that has one — that face's
    /// glyph as wide as the cell allows and no taller than the bitmap's,
    /// centred on the cell and standing on the bitmap's baseline. The grid is
    /// the bitmap's either way: every char is one cell, so measuring is
    /// unchanged. A char no face has stays `.notdef`.
    pub(super) fn bitmap_text(
        &self,
        list: &mut DrawList,
        pos: Vec2,
        text: &str,
        color: [f32; 4],
        size: f32,
    ) {
        let missing = |c: char| c != '\n' && glyph_index(c) == NOTDEF_INDEX;
        if self.fallback.is_empty() || !text.chars().any(missing) {
            list.text(pos, text, color, size);
            return;
        }
        let scale = size / NATURAL_FONT_SIZE;
        let cell = GLYPH_ADVANCE * scale;
        let tallest = GLYPH_HEIGHT as f32 * scale;
        let mut shown = String::with_capacity(text.len());
        let mut borrowed = Vec::new();
        let (mut column, mut line) = (0_u16, 0_u16);
        for c in text.chars() {
            if c == '\n' {
                shown.push(c);
                (column, line) = (0, line + 1);
                continue;
            }
            let found = missing(c)
                .then(|| {
                    self.fallback.iter().find_map(|font| {
                        let glyph = font.glyph_id(c);
                        (glyph != GlyphId::NOTDEF).then_some((font, glyph))
                    })
                })
                .flatten();
            match found {
                Some((font, glyph)) => {
                    // A space keeps the cell and draws nothing over the glyph.
                    shown.push(' ');
                    let wide = font.advance(glyph) / f32::from(font.metrics().units_per_em);
                    let em = if wide > 0.0 {
                        (cell / wide).min(tallest)
                    } else {
                        tallest
                    };
                    let offset = Vec2::new(
                        f32::from(column) * cell + (cell - wide * em) * 0.5,
                        f32::from(line) * LINE_HEIGHT * scale + ASCENDER as f32 * scale,
                    );
                    borrowed.push((font, em, PositionedGlyph { glyph, offset }, c));
                }
                None => shown.push(c),
            }
            column = column.saturating_add(1);
        }
        list.text(pos, shown, color, size);
        let mut buffer = [0; 4];
        for (font, em, glyph, c) in borrowed {
            list.text_glyphs(pos, font, em, color, [glyph], c.encode_utf8(&mut buffer));
        }
    }

    /// Draws the tree [`Ui::layout`] last laid out into `list`: every root in
    /// build order, each parent before its children, then every node's
    /// outline; then each open pop-up the same way, lowest first, and the
    /// tooltip last, under the viewport's clip (`popup.rs`, `tooltip.rs`).
    ///
    /// Clips are pushed and popped in pairs, so `list`'s own clip is what it
    /// was when this returns. The navigation debug overlay follows the tree
    /// when [`Ui::set_nav_debug`] switched it on.
    pub fn emit(&self, list: &mut DrawList) {
        self.emit_layer(0, list);
        if !self.popups.is_empty() || self.tooltip.built.is_some() {
            let viewport = self.viewport();
            list.push_clip(viewport.min, viewport.max);
            for layer in 1..=self.popups.len() {
                self.emit_layer(layer, list);
            }
            self.emit_layer(super::tooltip::TOOLTIP_LAYER, list);
            list.pop_clip()
                .expect("the clip pushed above is still on the stack");
        }
        self.emit_nav_debug(list);
    }

    /// Draws the roots of `layer` and their subtrees, then their outlines.
    fn emit_layer(&self, layer: usize, list: &mut DrawList) {
        let in_layer = |node: &super::FrameNode| {
            let stored = self.store.get(node.slot);
            stored.layer == layer && stored.hittable
        };
        // A root that is not hittable is one `display: none` hides, which
        // draws nothing, or a pop-up's that has closed since layout.
        for (index, node) in self.nodes.iter().enumerate() {
            if node.parent.is_none() && in_layer(node) {
                self.emit_node(index, list);
            }
        }
        // Outlines after the whole layer, as a browser paints them in a later
        // phase: a focus ring a later sibling covered would show focus on
        // nothing. Each is still clipped as its node was.
        for node in &self.nodes {
            if !in_layer(node) || !has_outline(&node.style) {
                continue;
            }
            let stored = self.store.get(node.slot);
            let (min, max) = stored.rect;
            // Painted outside every opacity group, so faded by them here: the
            // ring is drawn without overlapping itself, and its alpha times
            // the opacity is what the group would have drawn.
            let mut style = node.style;
            style.outline_color[3] *= self.drawn_opacity(node);
            list.push_clip(stored.clip.min, stored.clip.max);
            paint_outline(list, &style, min, max);
            list.pop_clip()
                .expect("the clip pushed above is still on the stack");
        }
    }

    /// The opacity `node` is drawn at: its own times every ancestor's, the
    /// groups it is drawn inside.
    fn drawn_opacity(&self, node: &super::FrameNode) -> f32 {
        let mut opacity = node.style.opacity;
        let mut parent = node.parent;
        while let Some(index) = parent {
            let ancestor = &self.nodes[index];
            opacity *= ancestor.style.opacity;
            parent = ancestor.parent;
        }
        opacity
    }

    fn emit_node(&self, index: usize, list: &mut DrawList) {
        let node = &self.nodes[index];
        if node.style.display == Display::None {
            return;
        }
        // The node and its subtree as one group, faded together: CSS's
        // `opacity`, which an opacity of one leaves out.
        let faded = node.style.opacity < 1.0;
        if faded {
            list.push_opacity(node.style.opacity);
        }
        let (min, max) = self.store.get(node.slot).rect;
        match node.content {
            // A text span is a box like a block, painted behind its text.
            Content::Block | Content::Paragraph | Content::Text { .. } => {
                let padding = padding_box(min, &node.layout);
                paint_box(list, &node.style, (min, max), padding, &self.images);
            }
            // A run is drawn by its paragraph, fragment by fragment, and paints
            // no box: one would be a rectangle per fragment, not its own.
            Content::Run { .. } => {}
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
        if let Content::Text { .. } = node.content {
            let (content_min, content_max) = content_box(min, &node.layout);
            // What layout decided it shows: cut by `text-overflow`, or whole.
            let text = self.shown_text(node).expect("a text span shows text");
            let style = &node.style;
            match &node.font {
                None => {
                    // The bitmap font's lines: one per newline, its fixed
                    // pitch apart, every glyph its fixed advance.
                    let scale = style.font_size / NATURAL_FONT_SIZE;
                    let lines = text.split('\n').enumerate().map(|(at, line)| {
                        let top = at as f32 * LINE_HEIGHT * scale;
                        let width = line.chars().count() as f32 * GLYPH_ADVANCE * scale;
                        (0.0, top + ASCENDER as f32 * scale, width)
                    });
                    underline_lines(list, style, None, content_min, lines);
                    self.bitmap_text(list, content_min, text, style.color, style.font_size);
                }
                Some(font) => {
                    // Broken at the width the layout measured it under —
                    // the unrounded one — unless it is `nowrap`, and
                    // aligned in the box it is drawn in, the rounded one.
                    let unrounded = &self.store.get(node.slot).unrounded;
                    let (size, line_height) = (style.font_size, style.text_line_height(font));
                    let mut layout = match style.white_space {
                        WhiteSpace::Normal => TextLayout::new(
                            font,
                            text,
                            size,
                            line_height,
                            Some(content_width(unrounded)),
                        ),
                        // Measuring or cutting it laid out what it shows
                        // already; a copy, as aligning moves its glyphs.
                        WhiteSpace::NoWrap => node.shown_layout.as_deref().map_or_else(
                            || TextLayout::nowrap(font, text, size, line_height),
                            TextLayout::clone,
                        ),
                    };
                    layout.align(content_max.x - content_min.x, style.text_align);
                    let lines = layout
                        .lines()
                        .iter()
                        .map(|line| (line.offset, line.baseline, line.width));
                    underline_lines(list, style, Some(font), content_min, lines);
                    // One run per face, since a run is in one font: a span
                    // whose every char its own font has is one run.
                    for run in layout.face_runs() {
                        list.text_glyphs(
                            content_min,
                            run.font,
                            style.font_size,
                            style.color,
                            run.glyphs,
                            &text[run.text],
                        );
                    }
                }
            }
        }
        if node.content == Content::Paragraph {
            self.emit_paragraph(index, list);
        }
        for child in &self.children[node.child_start..node.child_start + node.child_count] {
            self.emit_node(usize::from(*child), list);
        }
        if clipped {
            list.pop_clip()
                .expect("the clip pushed above is still on the stack");
        }
        if faded {
            list.pop_opacity()
                .expect("the group pushed above is still open");
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

/// A block's or text span's background, its background image and its border — or its
/// border image in the border's place — over its border box `min..max`, in
/// that order. `padding` is its padding box, which the background image is
/// stretched over; `images` is what [`Ui::set_image`] bound.
fn paint_box(
    list: &mut DrawList,
    style: &NodeStyle,
    (min, max): (Vec2, Vec2),
    padding: (Vec2, Vec2),
    images: &HashMap<ImageName, AtlasImage>,
) {
    let resolve = |name: Option<ImageName>| name.and_then(|name| images.get(&name));
    let background_image = resolve(style.background_image);
    let border_image = resolve(style.border_image.source);
    let border = style.border;
    let has_border = border_image.is_none()
        && visible(style.border_color)
        && [border.top, border.right, border.bottom, border.left]
            .iter()
            .any(|width| *width > 0.0);

    if style.radii != CornerRadii::uniform(0.0) {
        // One command for the background and the border, unless a background
        // image has to go between them.
        let joined = background_image.is_none() && has_border;
        if visible(style.background) || joined {
            list.rounded_rect(
                min,
                max,
                style.radii,
                style.background,
                Border {
                    width: if joined { border.top } else { 0.0 },
                    color: style.border_color,
                },
            );
        }
        paint_images(
            list,
            style,
            (min, max),
            padding,
            background_image,
            border_image,
        );
        if has_border && !joined {
            list.rounded_rect(
                min,
                max,
                style.radii,
                [0.0; 4],
                Border {
                    width: border.top,
                    color: style.border_color,
                },
            );
        }
        return;
    }

    if visible(style.background) {
        list.rect(min, max, style.background);
    }
    paint_images(
        list,
        style,
        (min, max),
        padding,
        background_image,
        border_image,
    );
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

/// A block's background image over its padding box, then its border image
/// over its border box: stretched and untinted, as CSS draws both. A rounded
/// box's corners do not clip either.
fn paint_images(
    list: &mut DrawList,
    style: &NodeStyle,
    (min, max): (Vec2, Vec2),
    padding: (Vec2, Vec2),
    background_image: Option<&AtlasImage>,
    border_image: Option<&AtlasImage>,
) {
    if let Some(image) = background_image {
        list.image(padding.0, padding.1, image, [1.0; 4]);
    }
    if let Some(image) = border_image {
        let frame = style.border_image;
        let slice = frame.slice;
        let sliced = NineSliceImage {
            image: *image,
            insets: SkinInsets::new(slice.left, slice.right, slice.top, slice.bottom),
        };
        let (width, border) = (frame.width, style.border);
        let bands = SkinInsets::new(
            width.left.resolve(border.left),
            width.right.resolve(border.right),
            width.top.resolve(border.top),
            width.bottom.resolve(border.bottom),
        );
        list.nine_slice_bands(min, max, &sliced, bands, frame.fill, [1.0; 4]);
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
