//! Inline layout: a paragraph's runs, each in its own font and size, broken
//! into lines together as one stream of text.
//!
//! # The line breaker's rules
//!
//! [`crate::font::layout`]'s rules, carried across runs and added to:
//!
//! * **A run boundary is not a break.** Soft breaks are found in the runs'
//!   texts joined, and words are the runs between spaces whichever runs they
//!   are in, so `un` in one run and `known` in the next is one word that
//!   never breaks between them. Bidi levels are found in the joined text
//!   too, so a paragraph's direction is its first strong char's whichever
//!   run holds it, and each line's glyphs are put in visual order across
//!   runs; a fragment is one run's stretch of one level, unbroken on the
//!   screen, and a line's fragments run left to right. The bitmap font draws
//!   a fragment's chars left to right whatever its level, having no
//!   right-to-left glyphs to draw.
//! * **A stretch with no soft break wider than the wrap width is broken
//!   between clusters**, at the last one that fits and never before the
//!   first — CSS's `overflow-wrap: break-word`, so a long link in a
//!   notification wraps rather than overflowing — and **never inside a
//!   cluster** ([`crate::font::cluster`]). It does not change the min-content
//!   width, which is the widest stretch unbroken, as `break-word` specifies.
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
//! break at one. A word cut inside — at a soft break or between clusters —
//! has its two sides shaped again apart where `harfrust` says the cut
//! changes the shaping, so nothing kerns across that cut either; the side
//! left on the line was chosen by the width it had shaped whole, so losing a
//! kerning adjustment can leave it a fraction of a pixel past the wrap
//! width.
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

use crate::font::bidi::{Bidi, visual_order};
use crate::font::layout::{PositionedGlyph, TextAlign, wrap_width};
use crate::font::shape::{ShapedGlyph, shape};
use crate::font::{Font, FontId, GlyphId, breaks, cluster};
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
    /// Whether a soft break falls before it.
    pub soft: bool,
    /// Whether cutting before it changes the shaping of either side, as
    /// [`ShapedGlyph::unsafe_to_break`] says; never in the bitmap font.
    pub unsafe_to_break: bool,
    /// Its bidi level on its line, rule L1 applied; zero in a paragraph
    /// with no right-to-left level.
    pub level: u8,
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
    /// Whether its paragraph runs right to left.
    pub rtl: bool,
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

/// One piece of the stream, in order.
enum Token {
    /// A word's parts, in order: a run of anything but spaces, which may hold
    /// soft breaks of its own.
    Word(Vec<Segment>),
    Space(Segment),
    /// A hard break, in this run.
    Newline(usize),
}

/// The paragraph's runs' texts joined, where breaks and bidi levels are
/// found, and where each run's text starts in it.
struct Joined {
    text: String,
    starts: Vec<usize>,
}

impl Joined {
    fn new(runs: &[InlineRun<'_>]) -> Self {
        let mut text = String::new();
        let mut starts = Vec::with_capacity(runs.len());
        for run in runs {
            starts.push(text.len());
            text.push_str(run.text);
        }
        Self { text, starts }
    }

    /// The run holding byte `at`.
    fn run_at(&self, at: usize) -> usize {
        self.starts.partition_point(|&start| start <= at) - 1
    }

    /// The parts of bytes `range` in each run they reach into.
    fn parts(&self, range: Range<usize>) -> Vec<Segment> {
        let mut parts = Vec::new();
        for (run, &start) in self.starts.iter().enumerate() {
            let end = self.starts.get(run + 1).copied().unwrap_or(self.text.len());
            let (from, to) = (range.start.max(start), range.end.min(end));
            if from < to {
                parts.push((run, from - start..to - start));
            }
        }
        parts
    }
}

/// One paragraph of the joined text: its bytes, and its levels when any
/// runs right to left.
struct Paragraph<'j> {
    bytes: Range<usize>,
    bidi: Option<Bidi<'j>>,
}

/// What glyphs are pushed from: the paragraph's runs, the bitmap font a
/// bitmap run advances by, the soft breaks and the bidi levels.
struct Source<'s, 'r> {
    runs: &'s [InlineRun<'r>],
    atlas: &'s FontAtlas,
    joined: &'s Joined,
    paragraphs: Vec<Paragraph<'s>>,
    /// Every soft break, as the run it is in and the byte of that run's text
    /// it falls before, in order.
    soft: Vec<(usize, usize)>,
}

impl Source<'_, '_> {
    /// The paragraph holding byte `at` of the joined text.
    fn paragraph(&self, at: usize) -> &Paragraph<'_> {
        let index = self
            .paragraphs
            .partition_point(|paragraph| paragraph.bytes.end < at);
        &self.paragraphs[index.min(self.paragraphs.len() - 1)]
    }

    /// `segments` — parts of one paragraph — cut where the bidi level
    /// changes: each part, and whether it runs right to left.
    fn directed(&self, segments: &[Segment]) -> Vec<(Segment, bool)> {
        let mut out = Vec::with_capacity(segments.len());
        for (run, bytes) in segments {
            let at = self.joined.starts[*run] + bytes.start;
            let paragraph = self.paragraph(at);
            let Some(bidi) = &paragraph.bidi else {
                out.push(((*run, bytes.clone()), false));
                continue;
            };
            let local = at - paragraph.bytes.start;
            for (piece, rtl) in bidi.runs(local..local + bytes.len()) {
                let start = bytes.start + piece.start - local;
                out.push(((*run, start..start + piece.len()), rtl));
            }
        }
        out
    }
}

/// A word token placed in the layout: its glyphs and its parts.
struct PlacedWord {
    glyphs: Range<usize>,
    parts: Vec<Segment>,
}

/// The paragraph's runs as words, spaces and hard breaks, in order, and its
/// soft breaks: breaks are found by [`crate::font::breaks`] in the runs'
/// texts `joined`, cut into `paragraphs`, so a run boundary is never a break
/// of its own.
fn tokens(joined: &Joined, paragraphs: &[Paragraph<'_>]) -> (Vec<Token>, Vec<(usize, usize)>) {
    let (mut tokens, mut soft) = (Vec::new(), Vec::new());
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let at = paragraph.bytes.start;
        let text = &joined.text[paragraph.bytes.clone()];
        for byte in breaks::soft_breaks(text) {
            let run = joined.run_at(at + byte);
            soft.push((run, at + byte - joined.starts[run]));
        }
        let mut chars = text.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            if c == ' ' {
                tokens.extend(
                    joined
                        .parts(at + start..at + start + 1)
                        .into_iter()
                        .map(Token::Space),
                );
                continue;
            }
            let mut end = start + c.len_utf8();
            while let Some((next, c)) = chars.next_if(|&(_, c)| c != ' ') {
                end = next + c.len_utf8();
            }
            tokens.push(Token::Word(joined.parts(at + start..at + end)));
        }
        if index + 1 < paragraphs.len() {
            tokens.push(Token::Newline(joined.run_at(paragraph.bytes.end)));
        }
    }
    (tokens, soft)
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
}

impl Line {
    const fn new(first: usize) -> Self {
        Self {
            first,
            end: first,
            pen: 0.0,
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
        // Only a definite width breaks a word between clusters.
        let break_words = matches!(wrap, Wrap::Width(_));
        let joined = Joined::new(runs);
        let paragraphs: Vec<Paragraph<'_>> = breaks::paragraphs(&joined.text)
            .map(|bytes| Paragraph {
                bidi: Bidi::new(&joined.text[bytes.clone()]),
                bytes,
            })
            .collect();
        let (tokens, soft) = tokens(&joined, &paragraphs);
        let source = Source {
            runs,
            atlas,
            joined: &joined,
            paragraphs,
            soft,
        };
        // Each finished line's glyphs and the run its metrics fall back to.
        let mut finished: Vec<(Range<usize>, usize)> = Vec::new();
        let mut words: Vec<PlacedWord> = Vec::new();
        let mut line = Line::new(0);
        let mut last_run = 0;
        for token in tokens {
            match token {
                Token::Newline(run) => {
                    finished.push((line.first..line.end, run));
                    last_run = run;
                    line = Line::new(layout.glyphs.len());
                }
                Token::Space(segment) => {
                    last_run = segment.0;
                    let start = layout.glyphs.len();
                    layout.push(&source, core::slice::from_ref(&segment), false);
                    line.pen = layout.place_from(line.pen, start);
                }
                Token::Word(parts) => {
                    if let Some(&(run, _)) = parts.last() {
                        last_run = run;
                    }
                    let start = layout.glyphs.len();
                    layout.push(&source, &parts, true);
                    words.push(PlacedWord {
                        glyphs: start..layout.glyphs.len(),
                        parts,
                    });
                    let mut right = layout.place_from(line.pen, start);
                    // Cut the line until what is left of it fits.
                    while let Some(limit) = limit
                        && right > limit
                        && let Some(cut) = layout.cut_at(line.first, limit, break_words)
                    {
                        let cut = layout.split_word(&source, &mut words, cut);
                        let kept = layout.glyphs[line.first..cut]
                            .iter()
                            .rposition(|glyph| glyph.drawn)
                            .map_or(line.first, |last| line.first + last + 1);
                        finished.push((line.first..kept, last_run));
                        line = Line::new(cut);
                        right = layout.place_from(0.0, cut);
                    }
                    line.pen = right;
                    line.end = layout.glyphs.len();
                }
            }
        }
        finished.push((line.first..line.end, last_run));
        layout.finish(&source, &finished);
        layout
    }

    /// Where the line whose glyphs start at `first` is cut to fit `limit`:
    /// before the glyph at its last soft break whose drawn glyphs before it
    /// fit; else, when `break_words`, between two clusters of the line's
    /// first unbreakable stretch, at the last whose glyphs before it fit and
    /// never before its first cluster; else at its first soft break, the
    /// stretch before it overflowing. Never before the line's first drawn
    /// glyph; `None` when nothing can be cut.
    fn cut_at(&self, first: usize, limit: f32, break_words: bool) -> Option<usize> {
        let (mut fitting, mut first_soft) = (None, None);
        // The right edge of the last drawn glyph before the one looked at.
        let mut before: Option<f32> = None;
        for (at, glyph) in self.glyphs.iter().enumerate().skip(first) {
            if glyph.soft
                && let Some(right) = before
            {
                first_soft.get_or_insert(at);
                if right <= limit {
                    fitting = Some(at);
                }
            }
            if glyph.drawn {
                before = Some(glyph.x + glyph.advance);
            }
        }
        if fitting.is_some() || !break_words {
            return fitting.or(first_soft);
        }
        let end = first_soft.unwrap_or(self.glyphs.len());
        let mut breaks = (first + 1..end).filter(|&at| {
            let (glyph, before) = (&self.glyphs[at], &self.glyphs[at - 1]);
            glyph.breakable && glyph.drawn && before.drawn
        });
        let Some(earliest) = breaks.next() else {
            return first_soft;
        };
        let fits = |at: usize| {
            let before = &self.glyphs[at - 1];
            before.x + before.advance <= limit
        };
        if !fits(earliest) {
            return Some(earliest);
        }
        Some(breaks.take_while(|&at| fits(at)).last().unwrap_or(earliest))
    }

    /// Readies a cut before glyph `cut`: when it falls inside a word where
    /// `harfrust` says cutting changes the shaping, that word's two sides
    /// are shaped again apart, the side before it placed where the word was.
    /// Returns the glyph the cut now falls before.
    fn split_word(
        &mut self,
        source: &Source<'_, '_>,
        words: &mut Vec<PlacedWord>,
        cut: usize,
    ) -> usize {
        let index = words.partition_point(|word| word.glyphs.start <= cut) - 1;
        let word = &words[index];
        if cut == word.glyphs.start || !self.glyphs[cut].unsafe_to_break {
            return cut;
        }
        let at = self.glyphs[cut];
        let (before, after) = split(&word.parts, at.run, at.start);
        let (start, old_end) = (word.glyphs.start, word.glyphs.end);
        let pen = self.glyphs[start].x;
        let rest = self.glyphs.split_off(old_end);
        self.glyphs.truncate(start);
        self.push(source, &before, true);
        let middle = self.glyphs.len();
        self.push(source, &after, true);
        let end = self.glyphs.len();
        self.glyphs.extend(rest);
        self.place_from(pen, start);
        for later in &mut words[index + 1..] {
            later.glyphs = later.glyphs.start + end - old_end..later.glyphs.end + end - old_end;
        }
        words.splice(
            index..=index,
            [
                PlacedWord {
                    glyphs: start..middle,
                    parts: before,
                },
                PlacedWord {
                    glyphs: middle..end,
                    parts: after,
                },
            ],
        );
        middle
    }

    /// Appends the glyphs `segments` shape into, unplaced, drawn or — for a
    /// space — not. See the module docs for which parts are shaped together.
    fn push(&mut self, source: &Source<'_, '_>, segments: &[Segment], drawn: bool) {
        let runs = source.runs;
        let parts = source.directed(segments);
        let mut at = 0;
        while at < parts.len() {
            let ((run, bytes), rtl) = &parts[at];
            let input = &runs[*run];
            let RunFont::Parsed(font) = input.font else {
                if let RunFont::Bitmap(scale) = input.font {
                    self.push_bitmap(source, *run, bytes.clone(), scale, drawn);
                }
                at += 1;
                continue;
            };
            let key = input.shaping_key();
            let together: Vec<Segment> = parts[at..]
                .iter()
                .take_while(|((run, _), next)| runs[*run].shaping_key() == key && next == rtl)
                .map(|(part, _)| part.clone())
                .collect();
            at += together.len();
            self.push_shaped(source, font, input.size, &together, *rtl, drawn);
        }
    }

    /// Appends `segments`, each in a run of `font` at `size`, shaped
    /// together, right to left when `rtl` says so.
    fn push_shaped(
        &mut self,
        source: &Source<'_, '_>,
        font: &Font,
        size: f32,
        segments: &[Segment],
        rtl: bool,
        drawn: bool,
    ) {
        let runs = source.runs;
        let mut shaped: Vec<ShapedGlyph> = Vec::new();
        // Where each segment starts in the text shaped, when that is the
        // segments joined; one segment is shaped in its run's own text.
        let mut starts = Vec::with_capacity(segments.len());
        if let [(run, bytes)] = segments {
            shape(font, size, runs[*run].text, bytes.clone(), rtl, &mut shaped);
        } else {
            let mut joined = String::new();
            for (run, bytes) in segments {
                starts.push(joined.len());
                joined.push_str(&runs[*run].text[bytes.clone()]);
            }
            shape(font, size, &joined, 0..joined.len(), rtl, &mut shaped);
        }
        // Where each cluster of the segments starts, as its run and byte: a
        // glyph that starts a cluster the shaper split, such as a flag's
        // second regional indicator, is no place to break.
        let cluster_starts: Vec<(usize, usize)> = segments
            .iter()
            .flat_map(|(run, bytes)| {
                cluster::clusters(&runs[*run].text[bytes.clone()])
                    .map(move |cluster| (*run, bytes.start + cluster.start))
            })
            .collect();
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
            let breakable = (index == 0 || shaped[index - 1].cluster != glyph.cluster)
                && cluster_starts.binary_search(&(run, start)).is_ok();
            self.glyphs.push(InlineGlyph {
                run,
                start,
                end,
                glyph: glyph.glyph,
                face: glyph.face,
                drawn,
                breakable,
                soft: breakable && source.soft.binary_search(&(run, start)).is_ok(),
                unsafe_to_break: glyph.unsafe_to_break,
                level: 0,
                x: 0.0,
                advance: glyph.advance,
                shift: glyph.offset,
            });
        }
    }

    /// Appends `bytes` of the bitmap run `run`, one glyph a char.
    fn push_bitmap(
        &mut self,
        source: &Source<'_, '_>,
        run: usize,
        bytes: Range<usize>,
        scale: f32,
        drawn: bool,
    ) {
        let input = &source.runs[run];
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
                    soft: at == 0 && source.soft.binary_search(&(run, start)).is_ok(),
                    unsafe_to_break: false,
                    level: 0,
                    x: 0.0,
                    advance: source.atlas.glyph(c).advance * scale,
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

    /// Puts the glyphs of the line `glyphs` in visual order — rule L2 — and
    /// places them again from its left edge, when its paragraph has
    /// right-to-left levels. Returns whether its paragraph runs right to
    /// left, and whether it was reordered.
    fn reorder(&mut self, source: &Source<'_, '_>, glyphs: Range<usize>) -> (bool, bool) {
        let Some(first) = self.glyphs.get(glyphs.start) else {
            return (false, false);
        };
        let joined = |glyph: &InlineGlyph| source.joined.starts[glyph.run] + glyph.start;
        let start = joined(first);
        let paragraph = source.paragraph(start);
        let Some(bidi) = &paragraph.bidi else {
            return (false, false);
        };
        let at = paragraph.bytes.start;
        let last = &self.glyphs[glyphs.end - 1];
        let end = source.joined.starts[last.run] + last.end;
        let levels = bidi.line_levels(start - at..end - at);
        let line = &mut self.glyphs[glyphs];
        let line_levels: Vec<_> = line
            .iter()
            .map(|glyph| levels[joined(glyph) - at])
            .collect();
        let mut pen = 0.0;
        for index in visual_order(&line_levels) {
            line[index].level = line_levels[index].number();
            line[index].x = pen;
            pen += line[index].advance;
        }
        (bidi.rtl(), true)
    }

    /// Puts each line in `finished` in visual order, cuts it into fragments —
    /// each one run's stretch of one level, so one piece on the screen — and
    /// stacks the lines; see the module docs for the vertical metrics.
    fn finish(&mut self, source: &Source<'_, '_>, finished: &[(Range<usize>, usize)]) {
        let runs = source.runs;
        let mut top = 0.0;
        for (index, (glyphs, fallback)) in finished.iter().enumerate() {
            let (rtl, reordered) = self.reorder(source, glyphs.clone());
            let first = self.fragments.len();
            let mut at = glyphs.start;
            while at < glyphs.end {
                let (run, level) = (self.glyphs[at].run, self.glyphs[at].level);
                let start = at;
                while at < glyphs.end
                    && (self.glyphs[at].run, self.glyphs[at].level) == (run, level)
                {
                    at += 1;
                }
                let (head, tail) = (self.glyphs[start], self.glyphs[at - 1]);
                let left = self.glyphs[start..at]
                    .iter()
                    .fold(f32::INFINITY, |left, glyph| left.min(glyph.x));
                let right = self.glyphs[start..at]
                    .iter()
                    .fold(f32::NEG_INFINITY, |right, glyph| {
                        right.max(glyph.x + glyph.advance)
                    });
                self.fragments.push(Fragment {
                    run,
                    line: index,
                    glyphs: start..at,
                    text: head.start..tail.end,
                    x: left,
                    width: right - left,
                });
            }
            let fragments = first..self.fragments.len();
            if reordered {
                self.fragments[fragments.clone()].sort_by(|a, b| a.x.total_cmp(&b.x));
            }
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
                rtl,
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
            let offset = room * align.share(line.rtl);
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
mod tests;
