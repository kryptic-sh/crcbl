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
//!   and after a hyphen** (`-`, which stays at the end of its line, with
//!   anything that continues its cluster). The spaces a line breaks at belong
//!   to no line, and spaces a line ends with before a newline or the end of
//!   the text are not measured; spaces anywhere else take their advance.
//! * **A word wider than the wrap width is broken between clusters**, at the
//!   last one that fits and never before the first — CSS's
//!   `overflow-wrap: break-word`, so a long link in a notification wraps
//!   rather than overflowing — and **never inside a cluster**
//!   ([`crate::font::cluster`]). It does not change the min-content width,
//!   which is the widest word unbroken, as `break-word` specifies.
//! * **The wrap width is whole pixels**, by [`wrap_width`], for the reason
//!   the font module gives.
//!
//! # Shaping
//!
//! Each word, and each space, is shaped on its own by
//! [`shape`](crate::font::shape): a word's parts in consecutive runs of one
//! parsed font at one size are shaped together, so kerning and ligatures
//! reach across such a run boundary, and parts that differ in either are
//! shaped apart. Nothing is shaped across a space, so nothing kerns across a
//! break. A word broken between clusters has its two sides shaped again
//! apart, so nothing kerns across that break either; the side left on the
//! line was chosen by the width it had shaped whole, so losing a kerning
//! adjustment can leave it a fraction of a pixel past the wrap width.
//!
//! A cluster in a parsed font is drawn by the face
//! [`cluster_face`](crate::font::fallback::cluster_face) picks from the
//! run's font and its fallback faces, shaped in that face; a presentation
//! selector or a zero-width joiner takes no width and draws nothing.
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

use crate::font::layout::{PositionedGlyph, TextAlign, wrap_width};
use crate::font::shape::{ShapedGlyph, shape};
use crate::font::{Font, FontId, GlyphId, cluster};
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

    /// What two neighbouring parts of a word must share to be shaped
    /// together: a parsed font and a size.
    fn shaping_key(&self) -> Option<(FontId, u32)> {
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

/// One glyph as laid out — a space too, which draws nothing — or, in the
/// bitmap font, one char.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineGlyph {
    /// Which run it is from.
    pub run: usize,
    /// Where the bytes of its run's text its cluster spells start.
    pub start: usize,
    /// Where they end.
    pub end: usize,
    /// Its glyph in a parsed font; [`GlyphId::NOTDEF`] in the bitmap font,
    /// which draws from the character.
    pub glyph: GlyphId,
    /// Which face of its run's font draws it, as [`Font::face`] numbers
    /// them; zero in the bitmap font.
    pub face: u8,
    /// Whether it draws anything: not a space.
    pub drawn: bool,
    /// Whether a word may be broken before it: the first glyph of a cluster.
    pub breakable: bool,
    /// Its pen position from the paragraph's left.
    pub x: f32,
    pub advance: f32,
    /// Where it is drawn from its pen, y down: the shaper's offset.
    pub shift: Vec2,
}

/// The part of one run that lies on one line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fragment {
    pub run: usize,
    pub line: usize,
    /// Its glyphs in [`InlineLayout::glyphs`].
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

/// A word's part in one run: the run and its bytes in that run's text.
type Segment = (usize, Range<usize>);

/// One piece of the stream between break opportunities.
enum Token {
    /// A word's parts, in order, ending at a space, a newline, a hyphen's
    /// cluster or the end of the text.
    Word(Vec<Segment>),
    Space(Segment),
    Newline(usize),
}

/// The paragraph's runs as words, spaces and newlines, in order, cut at
/// cluster boundaries.
fn tokens(runs: &[InlineRun<'_>]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<Segment> = Vec::new();
    for (run, input) in runs.iter().enumerate() {
        for bytes in cluster::clusters(input.text) {
            match input.text[bytes.start..].chars().next() {
                Some(c @ (' ' | '\n')) => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(core::mem::take(&mut word)));
                    }
                    tokens.push(if c == ' ' {
                        Token::Space((run, bytes))
                    } else {
                        Token::Newline(run)
                    });
                }
                first => {
                    match word.last_mut() {
                        Some((last, range)) if *last == run && range.end == bytes.start => {
                            range.end = bytes.end;
                        }
                        _ => word.push((run, bytes)),
                    }
                    if first == Some('-') {
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

/// `segments` cut at byte `byte` of run `run`: what lies before it, and what
/// lies from it on.
fn split(segments: &[Segment], run: usize, byte: usize) -> (Vec<Segment>, Vec<Segment>) {
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for (at, range) in segments {
        if (*at, range.end) <= (run, byte) {
            before.push((*at, range.clone()));
        } else if (*at, range.start) >= (run, byte) {
            after.push((*at, range.clone()));
        } else {
            before.push((*at, range.start..byte));
            after.push((*at, byte..range.end));
        }
    }
    (before, after)
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
                Token::Space(segment) => {
                    last_run = segment.0;
                    let start = layout.glyphs.len();
                    layout.push(runs, atlas, core::slice::from_ref(&segment), false);
                    line.pen = layout.place_from(line.pen, start);
                }
                Token::Word(segments) => {
                    if let Some(&(run, _)) = segments.last() {
                        last_run = run;
                    }
                    let start = layout.glyphs.len();
                    layout.push(runs, atlas, &segments, true);
                    let mut right = layout.place_from(line.pen, start);
                    if line.has_word && limit.is_some_and(|limit| right > limit) {
                        finished.push((line.first..line.end, last_run));
                        line = Line::new(start);
                        right = layout.place_from(0.0, start);
                    }
                    // A word alone too wide for the line is broken between
                    // clusters, at the last that fits, until what is left
                    // fits, each side shaped again on its own.
                    let mut word_start = start;
                    let mut rest = segments;
                    while let Some(limit) = word_limit
                        && right > limit
                        && let Some(fits) = layout.break_before(word_start, limit)
                    {
                        let pen = layout.glyphs[word_start].x;
                        let at = layout.glyphs[fits];
                        let (before, after) = split(&rest, at.run, at.start);
                        layout.glyphs.truncate(word_start);
                        layout.push(runs, atlas, &before, true);
                        let fits = layout.glyphs.len();
                        layout.place_from(pen, word_start);
                        finished.push((line.first..fits, last_run));
                        layout.push(runs, atlas, &after, true);
                        line = Line::new(fits);
                        word_start = fits;
                        rest = after;
                        right = layout.place_from(0.0, fits);
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

    /// Where a word whose glyphs start at `word_start` breaks to fit `limit`:
    /// before the last cluster whose glyphs before it all fit, and never
    /// before the first cluster; `None` when the word is one cluster.
    fn break_before(&self, word_start: usize, limit: f32) -> Option<usize> {
        let mut breaks =
            (word_start + 1..self.glyphs.len()).filter(|&at| self.glyphs[at].breakable);
        let first = breaks.next()?;
        let fits = |at: usize| {
            let before = &self.glyphs[at - 1];
            before.x + before.advance <= limit
        };
        if !fits(first) {
            return Some(first);
        }
        Some(breaks.take_while(|&at| fits(at)).last().unwrap_or(first))
    }

    /// Appends the glyphs `segments` shape into, unplaced, drawn or — for a
    /// space — not. See the module docs for which parts are shaped together.
    fn push(
        &mut self,
        runs: &[InlineRun<'_>],
        atlas: &FontAtlas,
        segments: &[Segment],
        drawn: bool,
    ) {
        let mut at = 0;
        while at < segments.len() {
            let (run, bytes) = &segments[at];
            let input = &runs[*run];
            let RunFont::Parsed(font) = input.font else {
                if let RunFont::Bitmap(scale) = input.font {
                    self.push_bitmap(input, *run, bytes.clone(), scale, atlas, drawn);
                }
                at += 1;
                continue;
            };
            let key = input.shaping_key();
            let together = segments[at..]
                .iter()
                .take_while(|(run, _)| runs[*run].shaping_key() == key)
                .count();
            self.push_shaped(runs, font, input.size, &segments[at..at + together], drawn);
            at += together;
        }
    }

    /// Appends `segments`, each in a run of `font` at `size`, shaped
    /// together.
    fn push_shaped(
        &mut self,
        runs: &[InlineRun<'_>],
        font: &Font,
        size: f32,
        segments: &[Segment],
        drawn: bool,
    ) {
        let mut shaped: Vec<ShapedGlyph> = Vec::new();
        // Where each segment starts in the text shaped, when that is the
        // segments joined; one segment is shaped in its run's own text.
        let mut starts = Vec::with_capacity(segments.len());
        if let [(run, bytes)] = segments {
            shape(font, size, runs[*run].text, bytes.clone(), &mut shaped);
        } else {
            let mut joined = String::new();
            for (run, bytes) in segments {
                starts.push(joined.len());
                joined.push_str(&runs[*run].text[bytes.clone()]);
            }
            shape(font, size, &joined, 0..joined.len(), &mut shaped);
        }
        // A byte of the text shaped, as its run, its byte in that run's text,
        // and the end of the segment it is in.
        let locate = |byte: usize| {
            if starts.is_empty() {
                let (run, bytes) = &segments[0];
                return (*run, byte, bytes.end);
            }
            let index = starts.partition_point(|&start| start <= byte) - 1;
            let (run, bytes) = &segments[index];
            (*run, bytes.start + byte - starts[index], bytes.end)
        };
        for (index, glyph) in shaped.iter().enumerate() {
            let (run, start, segment_end) = locate(glyph.cluster);
            let next = shaped[index + 1..]
                .iter()
                .find(|next| next.cluster != glyph.cluster)
                .map(|next| locate(next.cluster));
            let end = match next {
                Some((next_run, next_start, _)) if next_run == run => next_start,
                _ => segment_end,
            };
            let breakable = index == 0 || shaped[index - 1].cluster != glyph.cluster;
            self.glyphs.push(InlineGlyph {
                run,
                start,
                end,
                glyph: glyph.glyph,
                face: glyph.face,
                drawn,
                breakable,
                x: 0.0,
                advance: glyph.advance,
                shift: glyph.offset,
            });
        }
    }

    /// Appends `bytes` of the bitmap run `input`, one glyph a char.
    fn push_bitmap(
        &mut self,
        input: &InlineRun<'_>,
        run: usize,
        bytes: Range<usize>,
        scale: f32,
        atlas: &FontAtlas,
        drawn: bool,
    ) {
        for cluster in cluster::clusters(&input.text[bytes.clone()]) {
            let cluster = bytes.start + cluster.start..bytes.start + cluster.end;
            for (at, c) in input.text[cluster.clone()].char_indices() {
                let start = cluster.start + at;
                self.glyphs.push(InlineGlyph {
                    run,
                    start,
                    end: start + c.len_utf8(),
                    glyph: GlyphId::NOTDEF,
                    face: 0,
                    drawn: drawn && c != ' ',
                    breakable: at == 0,
                    x: 0.0,
                    advance: atlas.glyph(c).advance * scale,
                    shift: Vec2::ZERO,
                });
            }
        }
    }

    /// Places every glyph from `start` on, the first with its pen at `pen`,
    /// and returns the pen after the last.
    fn place_from(&mut self, pen: f32, start: usize) -> f32 {
        let mut pen = pen;
        for glyph in &mut self.glyphs[start..] {
            glyph.x = pen;
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
                    text: head.start..tail.end,
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

    /// `fragment`'s glyphs that draw — its spaces left out — at their pens on
    /// their baseline moved by the shaper's offsets, from the paragraph's
    /// top-left, cut where the face drawing them changes: each stretch's
    /// face, its glyphs, and the bytes of its run's text it spells, from its
    /// first glyph's cluster to the next stretch or the fragment's end.
    pub fn face_runs(&self, fragment: &Fragment) -> Vec<(u8, Vec<PositionedGlyph>, Range<usize>)> {
        let baseline = self.lines[fragment.line].baseline;
        let mut runs: Vec<(u8, Vec<PositionedGlyph>, Range<usize>)> = Vec::new();
        for glyph in &self.glyphs[fragment.glyphs.clone()] {
            if !glyph.drawn {
                continue;
            }
            let positioned = PositionedGlyph {
                glyph: glyph.glyph,
                offset: Vec2::new(glyph.x, baseline) + glyph.shift,
            };
            match runs.last_mut() {
                Some((face, glyphs, _)) if *face == glyph.face => glyphs.push(positioned),
                _ => {
                    if let Some((_, _, text)) = runs.last_mut() {
                        text.end = glyph.start;
                    }
                    runs.push((glyph.face, vec![positioned], glyph.start..fragment.text.end));
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

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;

    const SIZE: f32 = 16.0;

    /// Each line's text, its fragments' texts joined, and its width.
    fn lines(layout: &InlineLayout, runs: &[InlineRun<'_>]) -> Vec<(String, f32)> {
        layout
            .lines()
            .iter()
            .map(|line| {
                let text = layout.fragments()[line.fragments.clone()]
                    .iter()
                    .map(|fragment| &runs[fragment.run].text[fragment.text.clone()])
                    .collect();
                (text, line.width)
            })
            .collect()
    }

    /// **A word too wide is broken between clusters and never inside one**:
    /// at a width nothing fits, each `h` with its accent — two glyphs, one
    /// cluster — keeps a line of its own.
    #[test]
    fn an_overlong_word_never_breaks_inside_a_cluster() {
        let run = InlineRun::new(
            "hh\u{301}h\u{301}",
            RunFont::Parsed(Font::sans()),
            SIZE,
            20.0,
        );
        let runs = [run];
        let layout = InlineLayout::new(&runs, Wrap::Width(1.0), &FontAtlas::built_in());
        let texts: Vec<String> = lines(&layout, &runs)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(texts, ["h", "h\u{301}", "h\u{301}"]);
    }

    /// **A paragraph draws a mark at the shaper's offset from its pen**, on
    /// its line's baseline, and the mark takes no width.
    #[test]
    fn a_mark_is_drawn_at_the_shapers_offset() {
        let font = Font::sans();
        let runs = [InlineRun::new(
            "xh\u{301}",
            RunFont::Parsed(font),
            SIZE,
            20.0,
        )];
        let layout = InlineLayout::new(&runs, Wrap::None, &FontAtlas::built_in());
        let mut shaped = Vec::new();
        shape(font, SIZE, "xh\u{301}", 0..4, &mut shaped);
        let fragment = &layout.fragments()[0];
        let (_, glyphs, _) = &layout.face_runs(fragment)[0];
        let baseline = layout.lines()[0].baseline;
        let pen = shaped[0].advance + shaped[1].advance;
        assert_ne!(
            shaped[2].offset,
            Vec2::ZERO,
            "the font does not move the mark"
        );
        assert_eq!(
            glyphs[2].offset,
            Vec2::new(pen, baseline) + shaped[2].offset
        );
        assert_eq!(fragment.width, pen);
    }

    /// **A word broken between clusters has its sides shaped again apart**:
    /// three hearts kerned together are each a heart's own width once each
    /// takes a line, with nothing of the kerning towards the next.
    #[test]
    fn a_broken_word_is_shaped_again_on_each_side() {
        let hearts = fixtures::shaped();
        let run = InlineRun::new(
            "\u{2764}\u{2764}\u{2764}",
            RunFont::Parsed(&hearts),
            SIZE,
            20.0,
        );
        let runs = [run];
        let whole = InlineLayout::new(&runs, Wrap::None, &FontAtlas::built_in());
        assert_eq!(
            lines(&whole, &runs)[0].1,
            13.0 + 13.0 + 16.0,
            "the hearts kern"
        );
        let broken = InlineLayout::new(&runs, Wrap::Width(20.0), &FontAtlas::built_in());
        let widths: Vec<f32> = lines(&broken, &runs)
            .into_iter()
            .map(|(_, width)| width)
            .collect();
        assert_eq!(widths, [16.0, 16.0, 16.0]);
    }
}
