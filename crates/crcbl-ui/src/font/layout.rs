//! Text layout: shaped words, greedy line breaking and alignment.
//!
//! # The line breaker's rules
//!
//! * **Explicit newlines** end a line, always; an empty string, and an empty
//!   line between two newlines, is still one line tall.
//! * **Soft breaks are at spaces** (U+0020) and nowhere else. Words are the
//!   runs between spaces; a line takes words greedily while the word's right
//!   edge stays within the wrap width. A space is always a
//!   [`cluster`] of its own, so no break is ever inside one.
//! * **The spaces a line breaks at belong to no line**: they are not drawn,
//!   not measured, and the next line starts at the word after them. Spaces a
//!   line ends with before a newline or the end of the text are not measured
//!   either. Spaces anywhere else — leading a paragraph, or several between two
//!   words — take their advance.
//! * **A word wider than the wrap width is not broken**: it takes a line of its
//!   own and overflows it, which is CSS's `overflow-wrap: normal`.
//! * **The wrap width is whole pixels**: [`wrap_width`] floors it, so a
//!   measurement and a later layout at widths that differ by a fraction of a
//!   pixel — Taffy's unrounded box and the rounded one — break at the same
//!   places, and a measured width, rounded up by [`TextLayout::measure`], always
//!   fits the lines it was measured from.
//!
//! # Shaping
//!
//! **Each word, and each run of spaces, is shaped on its own** by
//! [`shape`](super::shape): kerning, marks, ligatures and every other GSUB and
//! GPOS lookup apply within it and never across a space. A word is placed as
//! one piece wherever the breaker puts it, so its width is the same on every
//! line and nothing kerns across a break. A word's glyphs are advanced by the
//! shaper's advances at their face's scale and drawn at the shaper's offsets
//! from the pen.
//!
//! # Fallback faces
//!
//! Each cluster is drawn by the face
//! [`fallback::cluster_face`](super::fallback::cluster_face) picks from the
//! font and the fallback faces it carries, shaped in that face at its own
//! scale; [`TextLayout::face_runs`] cuts the glyphs into the stretches each
//! face draws. A presentation selector or a zero-width joiner draws nothing
//! and takes no width. The lines' vertical metrics are the font's own: a
//! taller fallback glyph does not make its line taller.
//!
//! # Vertical metrics
//!
//! Lines are [`TextLayout::line_height`] apart. Within one, the baseline is
//! placed the way CSS places it: the font's ascent-to-descent box centred in
//! the line, so half the leading is above it and half below.

use core::ops::Range;

use glam::Vec2;

use super::shape::{ShapedGlyph, shape};
use super::{Font, GlyphId, cluster};

/// How close below a whole pixel a width may be and still count as that pixel.
///
/// Two computations of one box width — Taffy's measurement and a later layout
/// from its stored size — can land an ulp either side of an integer, and
/// flooring those two would disagree by a whole pixel.
pub const WRAP_EPSILON: f32 = 1.0 / 256.0;

/// The width lines are broken at for a box `width` pixels wide: whole pixels.
/// See the module docs.
#[must_use]
pub fn wrap_width(width: f32) -> f32 {
    (width + WRAP_EPSILON).floor()
}

/// `text-align`: where each line sits across its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Against the left edge — `left` and `start`.
    #[default]
    Left,
    /// Centred — `center`.
    Center,
    /// Against the right edge — `right` and `end`.
    Right,
}

/// One glyph as laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    /// Which glyph.
    pub glyph: GlyphId,
    /// Where it is drawn relative to the text's top-left: its pen on the
    /// line's baseline, moved by the shaper's offset.
    pub offset: Vec2,
}

/// One line as laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// Its glyphs in [`TextLayout::glyphs`]. Spaces draw nothing and are not
    /// among them.
    pub glyphs: Range<usize>,
    /// Its width in pixels, trailing spaces excluded.
    pub width: f32,
    /// Its baseline, from the text's top.
    pub baseline: f32,
    /// Its left edge's offset from the text's left, from alignment.
    pub offset: f32,
}

/// A string laid out in one font at one size. See the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    /// The font it was laid out in, fallback faces and all.
    font: Font,
    glyphs: Vec<PositionedGlyph>,
    /// Each glyph's face, as [`Font::face`] numbers it.
    faces: Vec<u8>,
    /// The byte offset in the text of the first char of each glyph's cluster.
    bytes: Vec<usize>,
    /// The text's length in bytes.
    text_len: usize,
    lines: Vec<TextLine>,
    line_height: f32,
}

/// A stretch of a [`TextLayout`]'s glyphs that one face draws: what
/// [`TextLayout::face_runs`] cuts it into.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceRun<'a> {
    /// The face: the layout's font or one of its fallback faces.
    pub font: &'a Font,
    /// Its glyphs.
    pub glyphs: &'a [PositionedGlyph],
    /// The bytes of the text it spells: from its first glyph's cluster, or
    /// the text's start for the first run, to the next run's start or the
    /// text's end.
    pub text: Range<usize>,
}

/// The line being filled.
struct Line {
    first: usize,
    pen: f32,
    /// The right edge of its last word.
    width: f32,
    has_word: bool,
}

impl Line {
    const fn new(first: usize) -> Self {
        Self {
            first,
            pen: 0.0,
            width: 0.0,
            has_word: false,
        }
    }
}

/// The pieces `paragraph` is shaped in, in order: each word and each run of
/// spaces, as its bytes and whether it is spaces. See the module docs.
fn pieces(paragraph: &str) -> impl Iterator<Item = (Range<usize>, bool)> + '_ {
    let mut chars = paragraph.char_indices().peekable();
    core::iter::from_fn(move || {
        let (start, c) = chars.next()?;
        let spaces = c == ' ';
        let mut end = start + c.len_utf8();
        while let Some(&(at, c)) = chars.peek() {
            if (c == ' ') != spaces {
                break;
            }
            end = at + c.len_utf8();
            chars.next();
        }
        Some((start..end, spaces))
    })
}

/// Places `glyphs` from a pen at `pen`, each at its pen plus its shaper's
/// offset, and returns the pen after the last.
fn place(glyphs: &mut [PositionedGlyph], shaped: &[ShapedGlyph], pen: f32) -> f32 {
    let mut pen = pen;
    for (positioned, shaped) in glyphs.iter_mut().zip(shaped) {
        positioned.offset.x = pen + shaped.offset.x;
        pen += shaped.advance;
    }
    pen
}

/// Each caret stop of `text` — one line, unbroken — in `font` at `size`,
/// written to `out` from its left edge: one before each char and one at the
/// end, placed as [`TextLayout`] places the glyphs.
///
/// A stop before the first char of a [`cluster`] is where the shaper put that
/// cluster's pen. The shaper may join several clusters into one glyph — a
/// ligature — and their stops then share its width evenly. A char inside a
/// cluster, where no caret stands, takes its cluster's stop.
pub(crate) fn caret_stops(font: &Font, text: &str, size: f32, out: &mut Vec<f32>) {
    out.clear();
    // Each shaped cluster: its first byte, its pen and its width.
    let mut shaped_clusters: Vec<(usize, f32, f32)> = Vec::new();
    let mut shaped = Vec::new();
    let mut pen = 0.0;
    for (piece, _) in pieces(text) {
        shaped.clear();
        shape(font, size, text, piece, &mut shaped);
        for glyph in &shaped {
            match shaped_clusters.last_mut() {
                Some((start, _, width)) if *start == glyph.cluster => *width += glyph.advance,
                _ => shaped_clusters.push((glyph.cluster, pen, glyph.advance)),
            }
            pen += glyph.advance;
        }
    }
    let starts: Vec<usize> = cluster::clusters(text).map(|range| range.start).collect();
    let mut stops = starts.iter().map(|&start| {
        // The shaped cluster it lies in, and the cluster starts that share it.
        let at = shaped_clusters.partition_point(|&(first, _, _)| first <= start);
        let Some(&(first, left, width)) = at.checked_sub(1).map(|at| &shaped_clusters[at]) else {
            return 0.0;
        };
        let end = shaped_clusters
            .get(at)
            .map_or(text.len(), |&(next, _, _)| next);
        let sharing =
            &starts[starts.partition_point(|&s| s < first)..starts.partition_point(|&s| s < end)];
        // A shaped cluster that starts inside a rule cluster counts its own
        // start as one more share before the first.
        let lead = usize::from(sharing.first() != Some(&first));
        let index = sharing.partition_point(|&s| s < start) + lead;
        left + width * index as f32 / (sharing.len() + lead) as f32
    });
    let mut current = 0.0;
    let mut next_start = starts.iter().peekable();
    for (byte, _) in text.char_indices() {
        if next_start.next_if(|&&start| start == byte).is_some() {
            current = stops.next().unwrap_or(current);
        }
        out.push(current);
    }
    out.push(pen);
}

impl TextLayout {
    /// Lays `text` out in `font` — and the fallback faces it carries — at
    /// `size` pixels per em, lines `line_height` apart, broken to fit `wrap`
    /// pixels when given — see the module docs — and every line against the
    /// left.
    #[must_use]
    pub fn new(font: &Font, text: &str, size: f32, line_height: f32, wrap: Option<f32>) -> Self {
        let metrics = font.metrics();
        let scale = metrics.scale(size);
        let wrap = wrap.map(wrap_width);
        let mut layout = Self {
            font: font.clone(),
            glyphs: Vec::with_capacity(text.len()),
            faces: Vec::with_capacity(text.len()),
            bytes: Vec::with_capacity(text.len()),
            text_len: text.len(),
            lines: Vec::new(),
            line_height,
        };
        // Every glyph as shaped, beside `glyphs`: its advance and offset.
        let mut shaped: Vec<ShapedGlyph> = Vec::with_capacity(text.len());
        let mut paragraph_start = 0;
        for paragraph in text.split('\n') {
            let mut line = Line::new(layout.glyphs.len());
            for (piece, spaces) in pieces(paragraph) {
                let word_start = shaped.len();
                shape(font, size, paragraph, piece, &mut shaped);
                if spaces {
                    line.pen += shaped[word_start..]
                        .iter()
                        .map(|glyph| glyph.advance)
                        .sum::<f32>();
                    shaped.truncate(word_start);
                    continue;
                }
                for glyph in &shaped[word_start..] {
                    layout.glyphs.push(PositionedGlyph {
                        glyph: glyph.glyph,
                        offset: Vec2::ZERO,
                    });
                    layout.faces.push(glyph.face);
                    layout.bytes.push(paragraph_start + glyph.cluster);
                }
                let word = word_start..shaped.len();
                let mut right = place(
                    &mut layout.glyphs[word.clone()],
                    &shaped[word.clone()],
                    line.pen,
                );
                if line.has_word && wrap.is_some_and(|wrap| right > wrap) {
                    layout.finish_line(&line, word_start);
                    line = Line::new(word_start);
                    right = place(&mut layout.glyphs[word.clone()], &shaped[word], 0.0);
                }
                line.pen = right;
                line.width = right;
                line.has_word = true;
            }
            let end = layout.glyphs.len();
            layout.finish_line(&line, end);
            paragraph_start += paragraph.len() + 1;
        }

        let ascent = metrics.ascent * scale;
        let descent = metrics.descent * scale;
        let above = (line_height - (ascent - descent)) * 0.5 + ascent;
        for (index, line) in layout.lines.iter_mut().enumerate() {
            line.baseline = index as f32 * line_height + above;
            for at in line.glyphs.clone() {
                layout.glyphs[at].offset.y = line.baseline + shaped[at].offset.y;
            }
        }
        layout
    }

    fn finish_line(&mut self, line: &Line, end: usize) {
        self.lines.push(TextLine {
            glyphs: line.first..end,
            width: line.width,
            baseline: 0.0,
            offset: 0.0,
        });
    }

    /// Moves every line across a box `width` pixels wide as `align` says. A
    /// line wider than the box starts at its left edge, as CSS starts an
    /// overflowing line.
    pub fn align(&mut self, width: f32, align: TextAlign) {
        for line in &mut self.lines {
            let room = (width - line.width).max(0.0);
            let offset = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => room * 0.5,
                TextAlign::Right => room,
            };
            for glyph in &mut self.glyphs[line.glyphs.clone()] {
                glyph.offset.x += offset - line.offset;
            }
            line.offset = offset;
        }
    }

    /// Every drawn glyph, line by line.
    #[must_use]
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// Each of [`TextLayout::glyphs`]' faces, as [`Font::face`] numbers them
    /// for the font it was laid out in.
    #[must_use]
    pub fn faces(&self) -> &[u8] {
        &self.faces
    }

    /// The glyphs cut where the face drawing them changes, in order: what a
    /// caller draws one
    /// [`DrawList::text_glyphs`](crate::draw_list::DrawList::text_glyphs)
    /// run per, since a run is in one font. Text with no glyphs at all is one
    /// empty run in the font itself.
    #[must_use]
    pub fn face_runs(&self) -> Vec<FaceRun<'_>> {
        let mut runs = Vec::new();
        let mut start = 0;
        while start < self.glyphs.len() {
            let face = self.faces[start];
            let end = (start..self.glyphs.len())
                .find(|&at| self.faces[at] != face)
                .unwrap_or(self.glyphs.len());
            let text_start = if start == 0 { 0 } else { self.bytes[start] };
            let text_end = self.bytes.get(end).copied().unwrap_or(self.text_len);
            runs.push(FaceRun {
                font: self.font.face(face),
                glyphs: &self.glyphs[start..end],
                text: text_start..text_end,
            });
            start = end;
        }
        if runs.is_empty() {
            runs.push(FaceRun {
                font: &self.font,
                glyphs: &[],
                text: 0..self.text_len,
            });
        }
        runs
    }

    /// Every line, top to bottom.
    #[must_use]
    pub fn lines(&self) -> &[TextLine] {
        &self.lines
    }

    /// The pitch between baselines.
    #[must_use]
    pub const fn line_height(&self) -> f32 {
        self.line_height
    }

    /// The widest line's width.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.lines
            .iter()
            .fold(0.0, |widest, line| widest.max(line.width))
    }

    /// Every line's height together.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.lines.len() as f32 * self.line_height
    }

    /// The box the text needs: its widest line rounded up to a whole pixel, and
    /// its lines' height.
    #[must_use]
    pub fn measure(&self) -> Vec2 {
        Vec2::new(self.width().ceil(), self.height())
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests;
