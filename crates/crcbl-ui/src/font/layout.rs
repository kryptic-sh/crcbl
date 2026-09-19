//! Text layout: advancing, kerning, greedy line breaking and alignment.
//!
//! # The line breaker's rules
//!
//! * **Explicit newlines** end a line, always; an empty string, and an empty
//!   line between two newlines, is still one line tall.
//! * **Soft breaks are at spaces** (U+0020) and nowhere else. Words are the
//!   runs between spaces; a line takes words greedily while the word's right
//!   edge stays within the wrap width.
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
//! Kerning applies between every two consecutive glyphs on a line, spaces
//! included, and never across a break.
//!
//! # Vertical metrics
//!
//! Lines are [`TextLayout::line_height`] apart. Within one, the baseline is
//! placed the way CSS places it: the font's ascent-to-descent box centred in
//! the line, so half the leading is above it and half below.

use core::ops::Range;

use glam::Vec2;

use super::{Font, GlyphId};

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
    /// Its pen position relative to the text's top-left: x along the line, y
    /// the line's baseline.
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
    glyphs: Vec<PositionedGlyph>,
    lines: Vec<TextLine>,
    line_height: f32,
}

/// The line being filled.
struct Line {
    first: usize,
    pen: f32,
    previous: Option<GlyphId>,
    /// The right edge of its last word.
    width: f32,
    has_word: bool,
}

impl Line {
    const fn new(first: usize) -> Self {
        Self {
            first,
            pen: 0.0,
            previous: None,
            width: 0.0,
            has_word: false,
        }
    }
}

/// Positions `word` from `line`'s pen, writing each glyph's x beside it, and
/// returns its right edge.
fn place_word(font: &Font, scale: f32, word: &mut [PositionedGlyph], line: &Line) -> f32 {
    let mut pen = line.pen;
    let mut previous = line.previous;
    for positioned in word {
        let glyph = positioned.glyph;
        if let Some(previous) = previous {
            pen += font.kerning(previous, glyph) * scale;
        }
        positioned.offset.x = pen;
        pen += font.advance(glyph) * scale;
        previous = Some(glyph);
    }
    pen
}

impl TextLayout {
    /// Lays `text` out in `font` at `size` pixels per em, lines `line_height`
    /// apart, broken to fit `wrap` pixels when given — see the module docs —
    /// and every line against the left.
    #[must_use]
    pub fn new(font: &Font, text: &str, size: f32, line_height: f32, wrap: Option<f32>) -> Self {
        let metrics = font.metrics();
        let scale = metrics.scale(size);
        let wrap = wrap.map(wrap_width);
        let mut layout = Self {
            glyphs: Vec::with_capacity(text.len()),
            lines: Vec::new(),
            line_height,
        };
        for paragraph in text.split('\n') {
            let mut line = Line::new(layout.glyphs.len());
            let mut chars = paragraph.chars().peekable();
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    chars.next();
                    let glyph = font.glyph_id(c);
                    if let Some(previous) = line.previous {
                        line.pen += font.kerning(previous, glyph) * scale;
                    }
                    line.pen += font.advance(glyph) * scale;
                    line.previous = Some(glyph);
                    continue;
                }
                let word_start = layout.glyphs.len();
                while let Some(&c) = chars.peek() {
                    if c == ' ' {
                        break;
                    }
                    chars.next();
                    layout.glyphs.push(PositionedGlyph {
                        glyph: font.glyph_id(c),
                        offset: Vec2::ZERO,
                    });
                }
                let mut right = place_word(font, scale, &mut layout.glyphs[word_start..], &line);
                if line.has_word && wrap.is_some_and(|wrap| right > wrap) {
                    layout.finish_line(&line, word_start);
                    line = Line::new(word_start);
                    right = place_word(font, scale, &mut layout.glyphs[word_start..], &line);
                }
                line.previous = layout.glyphs.last().map(|glyph| glyph.glyph);
                line.pen = right;
                line.width = right;
                line.has_word = true;
            }
            let end = layout.glyphs.len();
            layout.finish_line(&line, end);
        }

        let ascent = metrics.ascent * scale;
        let descent = metrics.descent * scale;
        let above = (line_height - (ascent - descent)) * 0.5 + ascent;
        for (index, line) in layout.lines.iter_mut().enumerate() {
            line.baseline = index as f32 * line_height + above;
            for glyph in &mut layout.glyphs[line.glyphs.clone()] {
                glyph.offset.y = line.baseline;
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
#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: f32 = 16.0;
    const LINE: f32 = 20.0;
    const TEXT: &str = "The quick brown fox jumps";

    /// `text`'s width at [`SIZE`], summed here glyph by glyph: every advance,
    /// and the kerning between every two neighbours.
    fn width(text: &str) -> f32 {
        let font = Font::sans();
        let scale = font.metrics().scale(SIZE);
        let mut pen = 0.0;
        let mut previous = None;
        for c in text.chars() {
            let glyph = font.glyph_id(c);
            if let Some(previous) = previous {
                pen += font.kerning(previous, glyph) * scale;
            }
            pen += font.advance(glyph) * scale;
            previous = Some(glyph);
        }
        pen
    }

    /// Each line's glyphs, as the non-space glyphs of the string it should be.
    fn assert_lines(layout: &TextLayout, want: &[&str], what: &str) {
        let font = Font::sans();
        let got: Vec<Vec<GlyphId>> = layout
            .lines()
            .iter()
            .map(|line| {
                layout.glyphs()[line.glyphs.clone()]
                    .iter()
                    .map(|g| g.glyph)
                    .collect()
            })
            .collect();
        let expected: Vec<Vec<GlyphId>> = want
            .iter()
            .map(|line| {
                line.chars()
                    .filter(|&c| c != ' ')
                    .map(|c| font.glyph_id(c))
                    .collect()
            })
            .collect();
        assert_eq!(got, expected, "{what}: lines are not {want:?}");
        for (line, text) in layout.lines().iter().zip(want) {
            assert!(
                (line.width - width(text.trim_end())).abs() < 1e-4,
                "{what}: {text:?} measured {} rather than {}",
                line.width,
                width(text.trim_end())
            );
        }
    }

    /// **Greedy breaks at exact places for fixed widths**, the widths chosen
    /// either side of where each line's last word ends.
    #[test]
    fn lines_break_at_the_exact_places_for_fixed_widths() {
        let font = Font::sans();
        // The table's premise, from the independent sum.
        assert!(width("The quick brown fox jumps") > 188.0 && width(TEXT) < 189.0);
        assert!(width("The quick brown fox") > 141.0 && width("The quick brown fox") < 142.0);
        assert!(width("brown fox") < 68.0 && width("fox jumps") > 68.0);

        let cases: [(Option<f32>, &[&str]); 11] = [
            (None, &["The quick brown fox jumps"]),
            (Some(189.0), &["The quick brown fox jumps"]),
            (Some(188.0), &["The quick brown fox", "jumps"]),
            (Some(142.0), &["The quick brown fox", "jumps"]),
            (Some(141.9), &["The quick brown", "fox jumps"]),
            (Some(116.0), &["The quick brown", "fox jumps"]),
            (Some(69.0), &["The quick", "brown fox", "jumps"]),
            (Some(68.0), &["The", "quick", "brown fox", "jumps"]),
            (Some(67.0), &["The", "quick", "brown", "fox", "jumps"]),
            (Some(0.0), &["The", "quick", "brown", "fox", "jumps"]),
            // A word wider than the width overflows its own line unbroken.
            (Some(20.0), &["The", "quick", "brown", "fox", "jumps"]),
        ];
        for (wrap, want) in cases {
            let layout = TextLayout::new(font, TEXT, SIZE, LINE, wrap);
            assert_lines(&layout, want, &format!("wrap {wrap:?}"));
        }
    }

    /// **Newlines always break, spaces are measured only inside a line, and an
    /// empty line is still a line.**
    #[test]
    fn newlines_and_spaces_follow_the_rules() {
        let font = Font::sans();
        let layout = TextLayout::new(font, "ab\n\ncd", SIZE, LINE, None);
        assert_lines(&layout, &["ab", "", "cd"], "blank line");
        assert_eq!(layout.height(), 3.0 * LINE);

        let empty = TextLayout::new(font, "", SIZE, LINE, None);
        assert_eq!(empty.lines().len(), 1);
        assert_eq!(empty.measure(), Vec2::new(0.0, LINE));

        let spaced = TextLayout::new(font, "  The  quick   \nfox ", SIZE, LINE, None);
        assert_lines(&spaced, &["  The  quick", "fox"], "spaces");
        assert!(spaced.lines()[0].width > width("The quick"));

        // The spaces at a soft break vanish: the next line starts at zero.
        let broken = TextLayout::new(font, "The     quick", SIZE, LINE, Some(40.0));
        assert_lines(&broken, &["The", "quick"], "break in a run of spaces");
        assert_eq!(
            broken.glyphs()[broken.lines()[1].glyphs.start].offset.x,
            0.0
        );
    }

    /// **A kerned pair is advanced by the font's pair adjustment**: `AV`'s
    /// second glyph sits at `A`'s advance plus the kerning the font gives.
    #[test]
    fn a_kerned_pair_is_advanced_by_the_fonts_adjustment() {
        let font = Font::sans();
        let (a, v) = (font.glyph_id('A'), font.glyph_id('V'));
        let adjustment = font.kerning(a, v);
        assert!(adjustment < 0.0, "the committed font does not kern AV");
        let scale = font.metrics().scale(SIZE);
        let layout = TextLayout::new(font, "AV", SIZE, LINE, None);
        let step = layout.glyphs()[1].offset.x - layout.glyphs()[0].offset.x;
        assert_eq!(step, font.advance(a) * scale + adjustment * scale);
        assert!(step < font.advance(a) * scale);
    }

    /// **Baselines are one line height apart**, with the font's box centred in
    /// each line.
    #[test]
    fn baselines_are_a_line_height_apart_with_the_box_centred() {
        let font = Font::sans();
        let metrics = font.metrics();
        let scale = metrics.scale(SIZE);
        let layout = TextLayout::new(font, TEXT, SIZE, LINE, Some(0.0));
        let first =
            (LINE - (metrics.ascent - metrics.descent) * scale) * 0.5 + metrics.ascent * scale;
        for (index, line) in layout.lines().iter().enumerate() {
            assert_eq!(line.baseline, index as f32 * LINE + first);
            for glyph in &layout.glyphs()[line.glyphs.clone()] {
                assert_eq!(glyph.offset.y, line.baseline);
            }
        }
    }

    /// **Alignment moves each line across its box**: left at zero, centred at
    /// half the room, right at all of it, and an overflowing line at zero.
    #[test]
    fn alignment_places_each_line_in_its_box() {
        let font = Font::sans();
        let base = TextLayout::new(font, "The quick\nfox", SIZE, LINE, None);
        for (align, share) in [
            (TextAlign::Left, 0.0),
            (TextAlign::Center, 0.5),
            (TextAlign::Right, 1.0),
        ] {
            let mut layout = base.clone();
            layout.align(100.0, align);
            // Aligning twice is aligning once.
            layout.align(100.0, align);
            for (line, before) in layout.lines().iter().zip(base.lines()) {
                let want = (100.0 - line.width) * share;
                assert_eq!(line.offset, want, "{align:?}");
                let first = line.glyphs.start;
                assert_eq!(
                    layout.glyphs()[first].offset.x,
                    base.glyphs()[first].offset.x + want,
                    "{align:?}"
                );
                assert_eq!(line.width, before.width);
            }
        }
        let mut wide = TextLayout::new(font, TEXT, SIZE, LINE, None);
        wide.align(50.0, TextAlign::Center);
        assert_eq!(wide.lines()[0].offset, 0.0);
    }

    /// **The wrap width snaps to whole pixels with a sliver of slack**, so two
    /// computations of one width an ulp apart agree.
    #[test]
    fn the_wrap_width_is_whole_pixels() {
        assert_eq!(wrap_width(100.0), 100.0);
        assert_eq!(wrap_width(99.999_99), 100.0);
        assert_eq!(wrap_width(100.000_01), 100.0);
        assert_eq!(wrap_width(100.9), 100.0);
        let measured = TextLayout::new(Font::sans(), TEXT, SIZE, LINE, None).measure();
        let again = TextLayout::new(Font::sans(), TEXT, SIZE, LINE, Some(measured.x));
        assert_eq!(
            again.lines().len(),
            1,
            "a measured width does not fit its own line"
        );
    }
}
