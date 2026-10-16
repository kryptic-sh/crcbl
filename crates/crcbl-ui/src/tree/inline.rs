//! Inline layout: a paragraph's runs, each in its own font and size, broken
//! into lines together as one stream of text.
//!
//! # The line breaker's rules
//!
//! [`crate::font::layout`]'s rules, carried across runs and added to:
//!
//! * **A run boundary is not a break.** Words are what lies between spaces,
//!   whichever runs they are in, so `un` in one run and `known` in the next is
//!   one word that never breaks between them.
//! * **Explicit newlines end a line, always**, and **soft breaks are at spaces
//!   and after a hyphen** (`-`, which stays at the end of its line). The spaces
//!   a line breaks at belong to no line, and spaces a line ends with before a
//!   newline or the end of the text are not measured; spaces anywhere else
//!   take their advance.
//! * **A word wider than the wrap width is broken between characters**, at
//!   the last one that fits and never before the first — CSS's
//!   `overflow-wrap: break-word`, so a long link in a notification wraps
//!   rather than overflowing. It does not change the min-content width, which
//!   is the widest word unbroken, as `break-word` specifies.
//! * **The wrap width is whole pixels**, by [`wrap_width`], for the reason
//!   the font module gives.
//!
//! Kerning applies between two neighbours on a line in the same font at the
//! same size, spaces included, and never across a break or between two runs
//! that differ in either.
//!
//! A char in a parsed font is drawn by the face [`map_char`] picks from the
//! run's font and its fallback faces, at that face's advance, and two
//! neighbours drawn by different faces do not kern; a presentation selector
//! or a zero-width joiner takes no width and draws nothing.
//!
//! # Vertical metrics
//!
//! Each run's glyphs sit on the line's one baseline. A run takes its own line
//! height with its font's ascent-to-descent box centred in it, as a span's
//! line does, which puts some of that height above the baseline and the rest
//! below; a line is as tall as the most any of its runs puts above plus the
//! most any puts below — CSS's line box, without the paragraph's own strut. A
//! line with no glyphs — an empty line between two newlines — takes the
//! metrics of the run its newline is in. The bitmap font's box is its fixed
//! line, [`ASCENDER`] of it above the baseline.

use core::ops::Range;

use glam::Vec2;

use crate::font::fallback::{Mapped, map_char};
use crate::font::layout::{PositionedGlyph, TextAlign, advance_pen, wrap_width};
use crate::font::{Font, FontId, GlyphId};
use crate::text::{ASCENDER, FontAtlas, LINE_HEIGHT};

/// What one run is drawn in.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RunFont<'a> {
    /// A parsed font, at the run's size.
    Parsed(&'a Font),
    /// The bitmap font, at this multiple of its natural size.
    Bitmap(f32),
}

/// One run, as the breaker reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineRun<'a> {
    pub text: &'a str,
    pub font: RunFont<'a>,
    /// Pixels per em, for a parsed font.
    pub size: f32,
    /// How far its box reaches above the baseline, leading included.
    pub above: f32,
    /// How far it reaches below.
    pub below: f32,
}

impl<'a> InlineRun<'a> {
    /// `text` in `font` at `size` pixels per em, its lines `line_height`
    /// apart — a parsed font's only: the bitmap font's line is fixed.
    pub fn new(text: &'a str, font: RunFont<'a>, size: f32, line_height: f32) -> Self {
        let (above, below) = match font {
            RunFont::Parsed(font) => {
                let metrics = font.metrics();
                let scale = metrics.scale(size);
                let (ascent, descent) = (metrics.ascent * scale, metrics.descent * scale);
                let above = (line_height - (ascent - descent)) * 0.5 + ascent;
                (above, line_height - above)
            }
            RunFont::Bitmap(scale) => {
                let above = ASCENDER as f32 * scale;
                (above, LINE_HEIGHT * scale - above)
            }
        };
        Self {
            text,
            font,
            size,
            above,
            below,
        }
    }

    /// What two neighbouring glyphs must share to kern: a parsed font and a
    /// size.
    fn kerning_key(&self) -> Option<(FontId, u32)> {
        match self.font {
            RunFont::Parsed(font) => Some((font.id(), self.size.to_bits())),
            RunFont::Bitmap(_) => None,
        }
    }
}

/// Which width lines are broken at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wrap {
    /// Only at newlines: max-content, or `white-space: nowrap`.
    None,
    /// At every soft break, with no word broken: min-content.
    MinContent,
    /// To fit this many pixels, words broken where one alone overflows.
    Width(f32),
}

/// One character as laid out: spaces too, which draw nothing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineGlyph {
    /// Which run it is from.
    pub run: usize,
    /// Its byte offset in that run's text.
    pub byte: usize,
    pub c: char,
    /// Its glyph in a parsed font; [`GlyphId::NOTDEF`] in the bitmap font,
    /// which draws from the character.
    pub glyph: GlyphId,
    /// Which face of its run's font draws it, as [`Font::face`] numbers
    /// them; zero in the bitmap font.
    pub face: u8,
    /// Whether it draws anything: not a space, and not a char that is
    /// invisible in a parsed font.
    pub drawn: bool,
    /// Its pen position from the paragraph's left.
    pub x: f32,
    pub advance: f32,
}

/// The part of one run that lies on one line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fragment {
    pub run: usize,
    pub line: usize,
    /// Its characters in [`InlineLayout::glyphs`].
    pub glyphs: Range<usize>,
    /// Its bytes in its run's text.
    pub text: Range<usize>,
    /// Its left edge from the paragraph's left.
    pub x: f32,
    pub width: f32,
}

/// One line as laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InlineLine {
    /// Its fragments in [`InlineLayout::fragments`], left to right.
    pub fragments: Range<usize>,
    /// Its top, from the paragraph's top.
    pub top: f32,
    /// Its baseline, from the paragraph's top.
    pub baseline: f32,
    pub height: f32,
    /// Its width, the spaces it ends with excluded.
    pub width: f32,
    /// How far alignment moved it right.
    pub offset: f32,
}

/// A paragraph's runs laid out together. See the module docs.
#[derive(Clone, Debug, Default)]
pub(crate) struct InlineLayout {
    glyphs: Vec<InlineGlyph>,
    fragments: Vec<Fragment>,
    lines: Vec<InlineLine>,
}

/// One piece of the stream between break opportunities.
enum Token {
    /// A word's characters, `(run, byte, char)`, ending at a space, a newline,
    /// a hyphen or the end of the text.
    Word(Vec<(usize, usize, char)>),
    Space(usize, usize),
    Newline(usize),
}

/// The paragraph's runs as words, spaces and newlines, in order.
fn tokens(runs: &[InlineRun<'_>]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = Vec::new();
    for (run, input) in runs.iter().enumerate() {
        for (byte, c) in input.text.char_indices() {
            match c {
                ' ' | '\n' => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(core::mem::take(&mut word)));
                    }
                    tokens.push(if c == ' ' {
                        Token::Space(run, byte)
                    } else {
                        Token::Newline(run)
                    });
                }
                _ => {
                    word.push((run, byte, c));
                    if c == '-' {
                        tokens.push(Token::Word(core::mem::take(&mut word)));
                    }
                }
            }
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

/// The line being filled.
struct Line {
    /// Its first glyph.
    first: usize,
    /// The end of its last word: what it holds once its trailing spaces go.
    end: usize,
    pen: f32,
    has_word: bool,
}

impl Line {
    const fn new(first: usize) -> Self {
        Self {
            first,
            end: first,
            pen: 0.0,
            has_word: false,
        }
    }
}

impl InlineLayout {
    /// Lays `runs` out together, broken as `wrap` says; `atlas` is the bitmap
    /// font a bitmap run advances by. Every line against the left.
    pub fn new(runs: &[InlineRun<'_>], wrap: Wrap, atlas: &FontAtlas) -> Self {
        let mut layout = Self::default();
        if runs.is_empty() {
            return layout;
        }
        let limit = match wrap {
            Wrap::None => None,
            Wrap::MinContent => Some(0.0),
            Wrap::Width(width) => Some(wrap_width(width)),
        };
        // Only a definite width breaks inside a word.
        let word_limit = limit.filter(|_| matches!(wrap, Wrap::Width(_)));
        // Each finished line's glyphs and the run its metrics fall back to.
        let mut finished: Vec<(Range<usize>, usize)> = Vec::new();
        let mut line = Line::new(0);
        let mut last_run = 0;
        for token in tokens(runs) {
            match token {
                Token::Newline(run) => {
                    finished.push((line.first..line.end, run));
                    last_run = run;
                    line = Line::new(layout.glyphs.len());
                }
                Token::Space(run, byte) => {
                    layout.push(runs, atlas, run, byte, ' ');
                    line.pen =
                        layout.place_from(runs, line.pen, layout.glyphs.len() - 1, line.first);
                    last_run = run;
                }
                Token::Word(chars) => {
                    let start = layout.glyphs.len();
                    for &(run, byte, c) in &chars {
                        layout.push(runs, atlas, run, byte, c);
                        last_run = run;
                    }
                    let mut right = layout.place_from(runs, line.pen, start, line.first);
                    if line.has_word && limit.is_some_and(|limit| right > limit) {
                        finished.push((line.first..line.end, last_run));
                        line = Line::new(start);
                        right = layout.place_from(runs, 0.0, start, start);
                    }
                    // A word alone too wide for the line is broken between
                    // characters, at the last that fits, until what is left
                    // fits.
                    let mut word_start = start;
                    while let Some(limit) = word_limit
                        && right > limit
                        && layout.glyphs.len() - word_start > 1
                    {
                        let fits = (word_start + 1..layout.glyphs.len())
                            .take_while(|&at| {
                                let glyph = &layout.glyphs[at];
                                glyph.x + glyph.advance <= limit
                            })
                            .last()
                            .map_or(word_start + 1, |at| at + 1);
                        finished.push((line.first..fits, last_run));
                        line = Line::new(fits);
                        word_start = fits;
                        right = layout.place_from(runs, 0.0, fits, fits);
                    }
                    line.pen = right;
                    line.end = layout.glyphs.len();
                    line.has_word = true;
                }
            }
        }
        finished.push((line.first..line.end, last_run));
        layout.finish(runs, &finished);
        layout
    }

    /// Appends `c` of `run`, at `byte` in its text, unplaced.
    fn push(
        &mut self,
        runs: &[InlineRun<'_>],
        atlas: &FontAtlas,
        run: usize,
        byte: usize,
        c: char,
    ) {
        let input = &runs[run];
        let (mapped, advance) = match input.font {
            RunFont::Parsed(font) => {
                let next = input.text[byte + c.len_utf8()..].chars().next();
                match map_char(font, c, next) {
                    Some(mapped) => (
                        Some(mapped),
                        advance_pen(font, input.size, 0.0, None, mapped).1,
                    ),
                    None => (None, 0.0),
                }
            }
            RunFont::Bitmap(scale) => (
                Some(Mapped {
                    face: 0,
                    glyph: GlyphId::NOTDEF,
                }),
                atlas.glyph(c).advance * scale,
            ),
        };
        self.glyphs.push(InlineGlyph {
            run,
            byte,
            c,
            glyph: mapped.map_or(GlyphId::NOTDEF, |mapped| mapped.glyph),
            face: mapped.map_or(0, |mapped| mapped.face),
            drawn: c != ' ' && mapped.is_some(),
            x: 0.0,
            advance,
        });
    }

    /// Places every glyph from `start` on, the first with its pen at `pen`,
    /// each kerned against the glyph before it unless that is not on its line
    /// — the line starting at glyph `line_first` — and returns the pen after
    /// the last.
    fn place_from(
        &mut self,
        runs: &[InlineRun<'_>],
        pen: f32,
        start: usize,
        line_first: usize,
    ) -> f32 {
        let mut pen = pen;
        for at in start..self.glyphs.len() {
            let glyph = self.glyphs[at];
            if at > line_first {
                let previous = self.glyphs[at - 1];
                let (before, after) = (&runs[previous.run], &runs[glyph.run]);
                if let (Some(key), RunFont::Parsed(font)) = (after.kerning_key(), after.font)
                    && before.kerning_key() == Some(key)
                    && previous.face == glyph.face
                {
                    let face = font.face(glyph.face);
                    pen += face.kerning(previous.glyph, glyph.glyph)
                        * face.metrics().scale(after.size);
                }
            }
            self.glyphs[at].x = pen;
            pen += glyph.advance;
        }
        pen
    }

    /// Cuts each line in `finished` into fragments by run and stacks the
    /// lines; see the module docs for the vertical metrics.
    fn finish(&mut self, runs: &[InlineRun<'_>], finished: &[(Range<usize>, usize)]) {
        let mut top = 0.0;
        for (index, (glyphs, fallback)) in finished.iter().enumerate() {
            let first = self.fragments.len();
            let mut at = glyphs.start;
            while at < glyphs.end {
                let run = self.glyphs[at].run;
                let start = at;
                while at < glyphs.end && self.glyphs[at].run == run {
                    at += 1;
                }
                let (head, tail) = (self.glyphs[start], self.glyphs[at - 1]);
                self.fragments.push(Fragment {
                    run,
                    line: index,
                    glyphs: start..at,
                    text: head.byte..tail.byte + tail.c.len_utf8(),
                    x: head.x,
                    width: tail.x + tail.advance - head.x,
                });
            }
            let fragments = first..self.fragments.len();
            let mut on_line: Vec<&InlineRun<'_>> = self.fragments[fragments.clone()]
                .iter()
                .map(|fragment| &runs[fragment.run])
                .collect();
            if on_line.is_empty() {
                on_line.push(&runs[*fallback]);
            }
            let above = on_line.iter().fold(0.0f32, |most, run| most.max(run.above));
            let below = on_line.iter().fold(0.0f32, |most, run| most.max(run.below));
            let width = self.fragments[fragments.clone()]
                .last()
                .map_or(0.0, |last| last.x + last.width);
            self.lines.push(InlineLine {
                fragments,
                top,
                baseline: top + above,
                height: above + below,
                width,
                offset: 0.0,
            });
            top += above + below;
        }
    }

    /// Moves every line across a box `width` pixels wide as `align` says; a
    /// line wider than the box starts at its left, as
    /// [`crate::font::layout::TextLayout::align`] places one.
    pub fn align(&mut self, width: f32, align: TextAlign) {
        for line in &mut self.lines {
            let room = (width - line.width).max(0.0);
            let offset = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => room * 0.5,
                TextAlign::Right => room,
            };
            let shift = offset - line.offset;
            for fragment in &mut self.fragments[line.fragments.clone()] {
                fragment.x += shift;
                for glyph in &mut self.glyphs[fragment.glyphs.clone()] {
                    glyph.x += shift;
                }
            }
            line.offset = offset;
        }
    }

    /// Every fragment, line by line and left to right.
    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    /// Every line, top to bottom.
    pub fn lines(&self) -> &[InlineLine] {
        &self.lines
    }

    /// `fragment`'s glyphs that draw — its spaces and invisible chars left
    /// out — pen at their baseline, from the paragraph's top-left, cut where
    /// the face drawing them changes: each stretch's face, its glyphs, and
    /// the bytes of its run's text it spells, from its first glyph to the
    /// next stretch or the fragment's end.
    pub fn face_runs(&self, fragment: &Fragment) -> Vec<(u8, Vec<PositionedGlyph>, Range<usize>)> {
        let baseline = self.lines[fragment.line].baseline;
        let mut runs: Vec<(u8, Vec<PositionedGlyph>, Range<usize>)> = Vec::new();
        for glyph in &self.glyphs[fragment.glyphs.clone()] {
            if !glyph.drawn {
                continue;
            }
            let positioned = PositionedGlyph {
                glyph: glyph.glyph,
                offset: Vec2::new(glyph.x, baseline),
            };
            match runs.last_mut() {
                Some((face, glyphs, _)) if *face == glyph.face => glyphs.push(positioned),
                _ => {
                    if let Some((_, _, text)) = runs.last_mut() {
                        text.end = glyph.byte;
                    }
                    runs.push((glyph.face, vec![positioned], glyph.byte..fragment.text.end));
                }
            }
        }
        if let Some((_, _, text)) = runs.first_mut() {
            text.start = fragment.text.start;
        }
        runs
    }

    /// The box the paragraph needs: its widest line rounded up to a whole
    /// pixel, and its lines' heights together.
    pub fn measure(&self) -> Vec2 {
        let widest = self
            .lines
            .iter()
            .fold(0.0f32, |widest, line| widest.max(line.width));
        Vec2::new(
            widest.ceil(),
            self.lines.iter().map(|line| line.height).sum(),
        )
    }
}
