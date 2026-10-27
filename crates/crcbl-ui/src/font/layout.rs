//! Text layout: shaped words, greedy line breaking and alignment.
//!
//! # The line breaker's rules
//!
//! * **Hard breaks** — a newline, and the rest of UAX #14's mandatory
//!   breaks, which `font::breaks` lists — end a line, always; an
//!   empty string, and an empty line between two newlines, is still one line
//!   tall.
//! * **Soft breaks are UAX #14's**, taken only where a
//!   [`cluster`](super::cluster) starts: after the spaces following a word,
//!   after a hyphen, between two ideographs, and never before closing
//!   punctuation, even after a space.
//!   Words are the runs between spaces (U+0020), so a soft break after a
//!   hyphen or between ideographs falls inside one. A line takes words
//!   greedily; once its last word's right edge passes the wrap width, it is
//!   cut at its last soft break whose words before it fit.
//! * **The spaces a line breaks at belong to no line**: they are not drawn,
//!   not measured, and the next line starts at the word after them. Spaces a
//!   line ends with before a hard break or the end of the text are not
//!   measured either — CSS's `pre-wrap`, where they hang — except in a
//!   [`TextLayout::nowrap`] layout, which measures them as CSS's `pre` does.
//!   Spaces anywhere else — leading a paragraph, or several between two words
//!   — take their advance.
//! * **A stretch with no soft break in it is never broken**: wider than the
//!   wrap width, it takes a line of its own and overflows it, which is CSS's
//!   `overflow-wrap: normal`.
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
//! GPOS lookup apply within it and never across a space, so nothing kerns
//! across a break at a space. A word cut at a soft break inside it keeps its
//! glyphs on both sides where `harfrust` says the cut changes nothing
//! ([`ShapedGlyph::unsafe_to_break`]); where it says otherwise, a kerning pair
//! or a ligature reaching across, each side is shaped again on its own. A
//! word's glyphs are advanced by the shaper's advances at their face's scale
//! and drawn at the shaper's offsets from the pen.
//!
//! # Bidirectional text
//!
//! A paragraph with right-to-left levels (`font::bidi`) is shaped
//! in pieces of one level each, a right-to-left piece right to left; lines are
//! broken in logical order, and each finished line's pieces are then placed
//! from its left edge in the visual order rule L2 gives, a right-to-left
//! piece's glyphs from its last. [`TextAlign::Start`] and [`TextAlign::End`]
//! follow the paragraph's direction, so a Hebrew paragraph sits against the
//! right edge by default. A paragraph with none is laid out exactly as it was
//! before bidi existed.
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

use super::bidi::Bidi;
use super::shape::ShapedGlyph;
use super::{Font, GlyphId};

mod carets;
mod fill;

use carets::line_stops;
pub(crate) use carets::{Carets, caret_stops};

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
    /// Against the edge its paragraph starts at — `start`: the left of a
    /// left-to-right paragraph, the right of a right-to-left one.
    #[default]
    Start,
    /// Against the edge its paragraph ends at — `end`.
    End,
    /// Against the left edge — `left`.
    Left,
    /// Centred — `center`.
    Center,
    /// Against the right edge — `right`.
    Right,
}

impl TextAlign {
    /// How much of the room beside a line goes to its left, for a line of a
    /// paragraph that runs right to left when `rtl` says so.
    pub(crate) fn share(self, rtl: bool) -> f32 {
        match (self, rtl) {
            (Self::Left, _) | (Self::Start, false) | (Self::End, true) => 0.0,
            (Self::Center, _) => 0.5,
            (Self::Right, _) | (Self::Start, true) | (Self::End, false) => 1.0,
        }
    }
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

/// Where a caret stands in a [`TextLayout`]: what [`TextLayout::caret`]
/// answers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// Its line in [`TextLayout::lines`].
    pub line: usize,
    /// Its x from the text's left, alignment included.
    pub x: f32,
    /// Its line's top, from the text's top.
    pub top: f32,
    /// Its line's height: [`TextLayout::line_height`].
    pub height: f32,
}

/// Where one line of a [`TextLayout`] lies in its text.
#[derive(Clone, Debug, PartialEq)]
struct LineBytes {
    /// Its paragraph's bytes, hard break excluded.
    paragraph: Range<usize>,
    /// Its own: from its first char to the next line's first in its
    /// paragraph, or to the paragraph's end, so the spaces it was broken at
    /// are its own.
    line: Range<usize>,
}

/// A string laid out in one font at one size. See the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    /// The font it was laid out in, fallback faces and all.
    font: Font,
    /// The size it was laid out at, in pixels per em.
    size: f32,
    /// The text it was laid out from.
    text: String,
    glyphs: Vec<PositionedGlyph>,
    /// Each glyph's face, as [`Font::face`] numbers it.
    faces: Vec<u8>,
    /// The byte offset in the text of the first char of each glyph's cluster.
    bytes: Vec<usize>,
    lines: Vec<TextLine>,
    /// Where each line lies in the text.
    line_bytes: Vec<LineBytes>,
    /// Whether each line's paragraph runs right to left.
    rtl: Vec<bool>,
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

impl TextLayout {
    /// Lays `text` out in `font` — and the fallback faces it carries — at
    /// `size` pixels per em, lines `line_height` apart, broken to fit `wrap`
    /// pixels when given — see the module docs — and every line against the
    /// left.
    #[must_use]
    pub fn new(font: &Font, text: &str, size: f32, line_height: f32, wrap: Option<f32>) -> Self {
        Self::lay_out(font, text, size, line_height, wrap, false)
    }

    /// Lays `text` out as `white-space: nowrap` text, which this crate draws
    /// as CSS's `pre`: broken only at hard breaks, as [`TextLayout::new`] with
    /// no wrap width is, but **a line keeps the spaces it ends with** — they
    /// take their advance in its width and in [`TextLayout::measure`]. A span
    /// of one plain space is a space wide, as a text field's block caret over
    /// one needs.
    #[must_use]
    pub fn nowrap(font: &Font, text: &str, size: f32, line_height: f32) -> Self {
        Self::lay_out(font, text, size, line_height, None, true)
    }

    /// [`TextLayout::new`], keeping each line's trailing spaces when
    /// `keep_spaces`.
    fn lay_out(
        font: &Font,
        text: &str,
        size: f32,
        line_height: f32,
        wrap: Option<f32>,
        keep_spaces: bool,
    ) -> Self {
        let metrics = font.metrics();
        let scale = metrics.scale(size);
        let wrap = wrap.map(wrap_width);
        let mut layout = Self {
            font: font.clone(),
            size,
            text: text.to_owned(),
            glyphs: Vec::with_capacity(text.len()),
            faces: Vec::with_capacity(text.len()),
            bytes: Vec::with_capacity(text.len()),
            lines: Vec::new(),
            line_bytes: Vec::new(),
            rtl: Vec::new(),
            line_height,
        };
        // Every glyph as shaped, beside `glyphs`: its advance and offset.
        let mut shaped: Vec<ShapedGlyph> = Vec::with_capacity(text.len());
        fill::fill(
            &mut layout,
            &mut shaped,
            font,
            size,
            wrap,
            keep_spaces,
            text,
        );

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

    /// Moves every line across a box `width` pixels wide as `align` says. A
    /// line wider than the box starts at its left edge, as CSS starts an
    /// overflowing line.
    pub fn align(&mut self, width: f32, align: TextAlign) {
        for (line, &rtl) in self.lines.iter_mut().zip(&self.rtl) {
            let room = (width - line.width).max(0.0);
            let offset = room * align.share(rtl);
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
            let text_end = self.bytes.get(end).copied().unwrap_or(self.text.len());
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
                text: 0..self.text.len(),
            });
        }
        runs
    }

    /// Every line, top to bottom.
    #[must_use]
    pub fn lines(&self) -> &[TextLine] {
        &self.lines
    }

    /// Where a caret stands before the byte `byte` of the text, measured by
    /// the code a text input places its caret with — the same stops, from
    /// the same shaping — so the two never disagree. A text input rounds the
    /// x to a whole pixel when it draws; this does not.
    ///
    /// * **Before a char, at its cluster's leading edge**: the left of a
    ///   left-to-right cluster, the right of a right-to-left one, in the
    ///   visual order the line's bidi levels give. A byte inside a cluster —
    ///   or inside a char — stands where the cluster starts, the stop
    ///   [`cluster::caret_stops`](super::cluster::caret_stops) names.
    /// * **At a line's first byte, at that line's start**; a byte in the
    ///   spaces a wrapped line was broken at, on the line they end, past its
    ///   last word; a byte in a hard break, at the end of the line it ends.
    /// * **At the end of the text**, and past it, after the last line's last
    ///   cluster: its trailing edge.
    /// * **Empty text** has one line, and the caret stands at its left edge.
    ///
    /// It shapes the line again, so a caller drawing every frame keeps the
    /// answer while the text and caret stay the same.
    ///
    /// ```
    /// # #[cfg(feature = "parsed-font")] {
    /// use crcbl_ui::font::Font;
    /// use crcbl_ui::font::layout::TextLayout;
    ///
    /// let layout = TextLayout::new(Font::sans(), "ab\ncd", 16.0, 20.0, None);
    /// let start = layout.caret(0);
    /// assert_eq!((start.line, start.x, start.top), (0, 0.0, 0.0));
    /// // After "ab", on the first line, where "b" ends.
    /// let end = layout.caret(2);
    /// assert_eq!(end.line, 0);
    /// assert_eq!(end.x, layout.lines()[0].width);
    /// // "c" starts the second line.
    /// let next = layout.caret(3);
    /// assert_eq!((next.line, next.x, next.top), (1, 0.0, 20.0));
    /// # }
    /// ```
    #[must_use]
    pub fn caret(&self, byte: usize) -> Caret {
        let index = self
            .line_bytes
            .partition_point(|bytes| bytes.line.start <= byte)
            .saturating_sub(1);
        let LineBytes { paragraph, line } = &self.line_bytes[index];
        let at = self
            .text
            .floor_char_boundary(byte.clamp(line.start, line.end));
        let text = &self.text[paragraph.clone()];
        let mut carets = Carets::default();
        line_stops(
            &self.font,
            text,
            Bidi::new(text).as_ref(),
            line.start - paragraph.start..line.end - paragraph.start,
            self.size,
            &mut carets,
        );
        let chars = self.text[line.start..at].chars().count();
        Caret {
            line: index,
            x: self.lines[index].offset + carets.stops[chars],
            top: index as f32 * self.line_height,
            height: self.line_height,
        }
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
