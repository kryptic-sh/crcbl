//! Rich text: runs of text in different fonts laid out together as one
//! paragraph, for a caller that draws the result itself.
//!
//! [`TextLayout`](super::layout::TextLayout) lays out one string in one font.
//! A notification body with bold, italic and plain stretches is several
//! strings in several fonts that must break into lines **together**: a word
//! half in bold and half in plain is still one word, and a line fills with
//! whichever runs come next. [`RichLayout`] is that, over the same inline
//! layout a [`Ui`](crate::tree::Ui) paragraph's runs use, so its rules are
//! that layout's: soft breaks are UAX #14's in the runs' texts joined, a run
//! boundary is never a break by itself, hard breaks end a line, the spaces a
//! line breaks at belong to no line and the spaces a line ends with are not
//! measured, a stretch wider than the wrap width with no soft break in it is
//! broken between clusters, bidi levels are found across runs, and each
//! cluster is drawn by the face the run's font and its fallbacks pick.
//!
//! What comes back is geometry and no glyphs: each line's box and baseline,
//! and its **segments**, each one run's stretch of that line — which run, the
//! bytes of that run's text it holds, and where it sits across the line. A
//! caller keeps whatever a run means to it, a colour, an underline or a link,
//! and draws each segment's text from its run's style, which shapes it the
//! way this layout measured it.

use core::ops::Range;

use glam::Vec2;

use super::Font;
use super::layout::TextAlign;
use crate::text::FontAtlas;
use crate::tree::inline::{InlineLayout, InlineRun, RunFont, Wrap};

/// One run of a [`RichLayout`]: text in one font at one size.
#[derive(Clone, Copy, Debug)]
pub struct RichRun<'a> {
    /// Its text. A newline in it is a hard break.
    pub text: &'a str,
    /// The font it is drawn in, with the fallback faces it carries.
    pub font: &'a Font,
    /// Pixels per em.
    pub size: f32,
}

/// One run's stretch of one line.
#[derive(Clone, Debug, PartialEq)]
pub struct RichSegment {
    /// Which run, by its position in the runs the layout was given.
    pub run: usize,
    /// The bytes of that run's text it holds.
    pub text: Range<usize>,
    /// Its left edge, from the layout's left.
    pub x: f32,
    /// How wide it is.
    pub width: f32,
}

/// One line of a [`RichLayout`].
#[derive(Clone, Debug, PartialEq)]
pub struct RichLine {
    /// Its segments in [`RichLayout::segments`], left to right on the screen.
    pub segments: Range<usize>,
    /// Its top, from the layout's top.
    pub top: f32,
    /// Its baseline, from the layout's top.
    pub baseline: f32,
    /// Its height: as much above the baseline as its tallest run reaches,
    /// and as much below.
    pub height: f32,
    /// Its width, the spaces it ends with left out.
    pub width: f32,
    /// Whether its paragraph runs right to left.
    pub rtl: bool,
}

/// Runs laid out together into lines. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct RichLayout {
    inline: InlineLayout,
    lines: Vec<RichLine>,
    segments: Vec<RichSegment>,
}

impl RichLayout {
    /// Lays `runs` out as one paragraph, each run's lines `line_height`
    /// pixels apart, broken to fit `wrap` pixels — floored to a whole pixel,
    /// as [`wrap_width`](super::layout::wrap_width) says — or only at hard
    /// breaks with `None`.
    #[must_use]
    pub fn new(runs: &[RichRun<'_>], line_height: f32, wrap: Option<f32>) -> Self {
        let runs: Vec<InlineRun<'_>> = runs
            .iter()
            .map(|run| InlineRun::new(run.text, RunFont::Parsed(run.font), run.size, line_height))
            .collect();
        let wrap = wrap.map_or(Wrap::None, Wrap::Width);
        // The atlas is the bitmap font's, which no rich run is in: it is
        // never read here, and building one is a table of advances.
        let inline = InlineLayout::new(&runs, wrap, &FontAtlas::built_in());
        let mut layout = Self {
            inline,
            lines: Vec::new(),
            segments: Vec::new(),
        };
        layout.read();
        layout
    }

    /// Moves every line across a box `width` pixels wide as `align` says; a
    /// line wider than the box starts at its left.
    pub fn align(&mut self, width: f32, align: TextAlign) {
        self.inline.align(width, align);
        self.read();
    }

    /// Every line, top to bottom.
    #[must_use]
    pub fn lines(&self) -> &[RichLine] {
        &self.lines
    }

    /// Every segment, line by line and left to right.
    #[must_use]
    pub fn segments(&self) -> &[RichSegment] {
        &self.segments
    }

    /// `line`'s segments, left to right.
    #[must_use]
    pub fn line_segments(&self, line: &RichLine) -> &[RichSegment] {
        &self.segments[line.segments.clone()]
    }

    /// The box the layout needs: its widest line rounded up to a whole pixel,
    /// and its lines' heights together.
    #[must_use]
    pub fn measure(&self) -> Vec2 {
        self.inline.measure()
    }

    /// The lines and segments, from the inline layout as it now stands.
    fn read(&mut self) {
        self.segments = self
            .inline
            .fragments()
            .iter()
            .map(|fragment| RichSegment {
                run: fragment.run,
                text: fragment.text.clone(),
                x: fragment.x,
                width: fragment.width,
            })
            .collect();
        self.lines = self
            .inline
            .lines()
            .iter()
            .map(|line| RichLine {
                segments: line.fragments.clone(),
                top: line.top,
                baseline: line.baseline,
                height: line.height,
                width: line.width,
                rtl: line.rtl,
            })
            .collect();
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests;
