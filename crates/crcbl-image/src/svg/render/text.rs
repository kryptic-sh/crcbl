//! Drawing a `<text>` element: its characters gathered from it and its
//! `<tspan>` descendants in document order, positioned, and drawn glyph by
//! glyph as the outlines of a path, filled and stroked as a shape is.
//!
//! # What is drawn
//!
//! `text`, and inside it `tspan` (and `a`, as a `tspan` that links), nested
//! as deep as the file likes: `x`, `y`, `dx` and `dy` lists, `xml:space`,
//! each element's own font, size, weight, style, anchor and spacing, fill
//! and stroke with every paint and their properties, and on the `text`
//! element its transform and group effects. The module [`text`] says how a
//! line is laid out and shaped, its submodule `shape` how it is shaped, and
//! its submodule `faces` where the faces come from.
//!
//! # What is refused
//!
//! `textPath`, `tref` and any other element inside the text; `rotate` and
//! `textLength`; opacity, a clip path, a mask or a filter on a `tspan`
//! (SVG 2 draws them on the whole text element only); any text property at
//! a value that changes text in a way this renderer does not lay out
//! (vertical writing, baselines, decorations, small caps and the rest the
//! style module's `text` lists); and, where a character would be drawn,
//! text with no face to draw it in, which includes every text in a build
//! without the `svg-text` feature. Text inside a clip path is refused where
//! the clip path is read.
//!
//! # Hostile text
//!
//! [`MAX_CHARACTERS`] bounds the characters drawn in all, `use` instances
//! counted, and the font lookups and faces read are capped where they are
//! made.

use std::collections::HashMap;

use super::{DESCRIPTIVE, Renderer, check_attributes, has_effects};
use crate::ImageError;
use crate::svg::canvas::Canvas;
use crate::svg::path::Rect;
#[cfg(feature = "svg-text")]
use crate::svg::style::TextStyle;
use crate::svg::style::{Style, length};
#[cfg(feature = "svg-text")]
use crate::svg::text::shape;
use crate::svg::text::{self, Axis, Character, Position};
use crate::svg::transform::Transform;
use crate::svg::unsupported;
use crate::svg::xml::Namespace;

/// The most characters drawn in all, counting each instance a `use` makes
/// of a text: each is a glyph outline to look up and rasterise, and a
/// small file of nested `use` elements could otherwise ask for millions.
pub(in crate::svg) const MAX_CHARACTERS: usize = 10_000;

/// Elements inside a text that hold text of their own.
const TEXT_CHILDREN: &[&str] = &["a", "tspan"];

/// Attributes of a text element that change its layout in ways this
/// renderer does not draw: a rotation for each character, and a length to
/// stretch the text to.
const REFUSED_TEXT_ATTRIBUTES: &[&str] = &["rotate", "textLength"];

/// A text's characters, gathered but not yet in glyphs.
pub(super) struct Content {
    pub characters: Vec<Character>,
    pub positions: Vec<Position>,
    /// The computed style of each element holding characters, by its index
    /// in the document.
    pub styles: HashMap<usize, Style>,
}

impl Renderer<'_> {
    /// Draws the text element at `index`, styled `style`, whose user space
    /// maps to pixels by `transform`.
    pub(super) fn text(
        &mut self,
        index: usize,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let content = self.text_content(index, style)?;
        if content.characters.is_empty() {
            return Ok(());
        }
        self.characters += content.characters.len();
        if self.characters > MAX_CHARACTERS {
            return Err(unsupported(format_args!(
                "more than {MAX_CHARACTERS} characters of text drawn"
            )));
        }
        self.draw_glyphs(&content, transform, canvas)
    }

    /// The object bounding box of the text element at `index`, as its
    /// glyphs' cells: each glyph's advance wide, and its face's ascent to
    /// its descent high, as resvg measures text.
    pub(super) fn text_box(&mut self, index: usize) -> Result<Option<Rect>, ImageError> {
        let style = self.document_style(index)?;
        let content = self.text_content(index, &style)?;
        if content.characters.is_empty() {
            return Ok(None);
        }
        self.text_bounds(&content)
    }

    /// Gathers the characters of the text element at `index`, styled
    /// `style`, and their positions.
    fn text_content(&mut self, index: usize, style: &Style) -> Result<Content, ImageError> {
        let preserve = self.preserves_space(index);
        let mut content = Content {
            characters: Vec::new(),
            positions: Vec::new(),
            styles: HashMap::new(),
        };
        content.styles.insert(index, style.clone());
        let mut ranges = Vec::new();
        self.gather(index, style, preserve, &mut content, &mut ranges)?;
        text::finish(&mut content.characters);
        let count = content.characters.len();
        content.positions = vec![Position::default(); count];
        let (w, h) = self.viewport;
        // Ancestors first, so an element's lists win for its characters.
        for (element, start, end) in ranges {
            let (start, end) = (start.min(count), end.min(count));
            for (axis, name, reference) in [
                (Axis::X, "x", w),
                (Axis::Y, "y", h),
                (Axis::Dx, "dx", w),
                (Axis::Dy, "dy", h),
            ] {
                let values = self.length_list(element, name, reference)?;
                text::assign(&mut content.positions[start..end], axis, &values);
            }
        }
        for character in &content.characters {
            if let Some((name, value)) = content
                .styles
                .get(&character.owner)
                .and_then(|style| style.text.undrawn.first())
            {
                return Err(unsupported(format_args!(
                    "text under {name}: {value} (<{}>)",
                    self.element(character.owner).name
                )));
            }
        }
        Ok(content)
    }

    /// Whether the text element at `index` keeps its white space: the
    /// nearest `xml:space` on it or an ancestor says `preserve`.
    fn preserves_space(&self, index: usize) -> bool {
        let mut at = Some(index);
        while let Some(element) = at {
            if let Some(space) = self.element(element).attribute(Namespace::Xml, "space") {
                return space == "preserve";
            }
            at = self.parents[element];
        }
        false
    }

    /// Appends the characters of the element at `index`, styled `style`, to
    /// `content`, and to `ranges` the span of them each element holds, in
    /// document order.
    fn gather(
        &mut self,
        index: usize,
        style: &Style,
        preserve: bool,
        content: &mut Content,
        ranges: &mut Vec<(usize, usize, usize)>,
    ) -> Result<(), ImageError> {
        let element = self.element(index);
        for name in REFUSED_TEXT_ATTRIBUTES {
            if element.attribute(Namespace::None, name).is_some() {
                return Err(unsupported(format_args!(
                    "attribute {name} on <{}>",
                    element.name
                )));
            }
        }
        let slot = ranges.len();
        ranges.push((index, content.characters.len(), 0));
        let mut offset = 0;
        for (&child, &at) in element.children.iter().zip(&element.child_offsets) {
            text::push_characters(
                &mut content.characters,
                &element.text[offset..at],
                index,
                preserve,
            );
            offset = at;
            let child_element = self.element(child);
            if child_element.namespace != Namespace::Svg
                || DESCRIPTIVE.contains(&child_element.name.as_str())
            {
                continue;
            }
            if !TEXT_CHILDREN.contains(&child_element.name.as_str()) {
                return Err(unsupported(format_args!(
                    "element <{}> inside <text>",
                    child_element.name
                )));
            }
            check_attributes(child_element)?;
            let child_style = Style::compute(&self.specified(child)?, style, &child_element.name)?;
            if !child_style.display {
                continue;
            }
            if has_effects(&child_style) {
                return Err(unsupported(format_args!(
                    "opacity, a clip-path, a mask or a filter on <{}>",
                    child_element.name
                )));
            }
            let child_preserve = match child_element.attribute(Namespace::Xml, "space") {
                Some(space) => space == "preserve",
                None => preserve,
            };
            content.styles.insert(child, child_style.clone());
            self.enter()?;
            let result = self.gather(child, &child_style, child_preserve, content, ranges);
            self.depth -= 1;
            result?;
        }
        text::push_characters(
            &mut content.characters,
            &element.text[offset..],
            index,
            preserve,
        );
        ranges[slot].2 = content.characters.len();
        Ok(())
    }

    /// A list of lengths, the attribute `name` of the element at `index`:
    /// empty if absent, a percentage taken of `reference`.
    fn length_list(
        &self,
        index: usize,
        name: &str,
        reference: f64,
    ) -> Result<Vec<f64>, ImageError> {
        let element = self.element(index);
        let Some(text) = element.attribute(Namespace::None, name) else {
            return Ok(Vec::new());
        };
        text.split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| match length(part) {
                Some(value) => Ok(value.resolve(reference)),
                None => Err(unsupported(format_args!(
                    "{name}={text:?} on <{}>",
                    element.name
                ))),
            })
            .collect()
    }
}

#[cfg(not(feature = "svg-text"))]
impl Renderer<'_> {
    fn draw_glyphs(
        &mut self,
        _content: &Content,
        _transform: Transform,
        _canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        Err(no_glyphs())
    }

    fn text_bounds(&mut self, _content: &Content) -> Result<Option<Rect>, ImageError> {
        Err(no_glyphs())
    }
}

/// Text drawn in a build with no fonts to draw it in.
#[cfg(not(feature = "svg-text"))]
fn no_glyphs() -> ImageError {
    unsupported("<text> (crcbl-image is built without its svg-text feature)")
}

/// One glyph placed: whose it is (its cluster's first character's), which,
/// where the pen is for it in the text's user space, and the font units to
/// user units scale it is drawn at.
#[cfg(feature = "svg-text")]
struct Placed {
    owner: usize,
    glyph: text::faces::Glyph,
    origin: (f64, f64),
    /// Where it is drawn from the pen, in user units, `y` down.
    offset: (f64, f64),
    scale: f64,
    weight: u16,
    /// How far it moves the pen, spacing included, in user units.
    advance: f64,
}

#[cfg(feature = "svg-text")]
impl Renderer<'_> {
    /// Each glyph the text shapes into, placed, chunk by chunk and each
    /// chunk's in visual order; [`text`] says how.
    fn lay_out(&mut self, content: &Content) -> Result<Vec<Placed>, ImageError> {
        let count = content.characters.len();
        let chars: Vec<char> = content.characters.iter().map(|c| c.ch).collect();
        let styles: Vec<&TextStyle> = content
            .characters
            .iter()
            .map(|character| &content.styles[&character.owner].text)
            .collect();
        let mut placed = Vec::with_capacity(count);
        let mut advances = Vec::with_capacity(count);
        let mut start = 0;
        while start < count {
            let mut end = start + 1;
            while end < count {
                let position = content.positions[end];
                if (position.x.is_some() || position.y.is_some())
                    && !shape::continues(&chars, &styles, end)
                {
                    break;
                }
                end += 1;
            }
            let shaped = shape::shape_chunk(
                &mut self.faces,
                self.fonts,
                &chars[start..end],
                &styles[start..end],
            )?;
            for (index, glyph) in shaped.iter().enumerate() {
                let character = start + glyph.cluster;
                let style = styles[character];
                let scale = style.size
                    / self
                        .faces
                        .metrics(glyph.glyph.face, style.weight)
                        .units_per_em;
                let mut advance = glyph.advance * scale;
                if shaped
                    .get(index + 1)
                    .is_none_or(|next| next.cluster != glyph.cluster)
                {
                    advance += style.letter_spacing.resolve(style.size);
                    if chars[character] == ' ' {
                        advance += style.word_spacing.resolve(style.size);
                    }
                }
                let mut position = Position::default();
                if index == 0 || shaped[index - 1].cluster != glyph.cluster {
                    position.dx = content.positions[character].dx;
                    position.dy = content.positions[character].dy;
                }
                if index == 0 {
                    position.x = content.positions[start].x;
                    position.y = content.positions[start].y;
                }
                advances.push(text::Advance {
                    position,
                    advance,
                    anchor: styles[start].anchor,
                });
                placed.push(Placed {
                    owner: content.characters[character].owner,
                    glyph: glyph.glyph,
                    origin: (0.0, 0.0),
                    offset: (glyph.offset.0 * scale, -glyph.offset.1 * scale),
                    scale,
                    weight: style.weight,
                    advance,
                });
            }
            start = end;
        }
        for (glyph, origin) in placed.iter_mut().zip(text::place(&advances)) {
            glyph.origin = origin;
        }
        Ok(placed)
    }

    /// The glyphs' cells joined; see [`Renderer::text_box`].
    fn text_bounds(&mut self, content: &Content) -> Result<Option<Rect>, ImageError> {
        let placed = self.lay_out(content)?;
        Ok(self.cells(&placed))
    }

    fn cells(&self, placed: &[Placed]) -> Option<Rect> {
        placed.iter().fold(None, |total, glyph| {
            let metrics = self.faces.metrics(glyph.glyph.face, glyph.weight);
            let cell = Rect {
                x: glyph.origin.0,
                y: glyph.origin.1 - metrics.ascent * glyph.scale,
                width: glyph.advance,
                height: (metrics.ascent - metrics.descent) * glyph.scale,
            };
            super::union(total, Some(cell))
        })
    }

    /// Draws each run of glyphs of one element as one path, filled and
    /// stroked by that element's style, chunk by chunk and each chunk's in
    /// visual order.
    fn draw_glyphs(
        &mut self,
        content: &Content,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let placed = self.lay_out(content)?;
        let bbox = self.cells(&placed);
        for run in placed.chunk_by(|a, b| a.owner == b.owner) {
            let style = &content.styles[&run[0].owner];
            if !style.visible {
                continue;
            }
            let mut segments = Vec::new();
            for glyph in run {
                let (x, y) = (
                    glyph.origin.0 + glyph.offset.0,
                    glyph.origin.1 + glyph.offset.1,
                );
                let scale = glyph.scale;
                self.faces.outline(
                    glyph.glyph,
                    glyph.weight,
                    |gx, gy| (x + gx * scale, y - gy * scale),
                    &mut segments,
                );
            }
            if segments.is_empty() {
                continue;
            }
            let geometry = crate::svg::path::Path { segments };
            self.paint_geometry(&geometry, style, bbox, transform, canvas)?;
        }
        Ok(())
    }
}
