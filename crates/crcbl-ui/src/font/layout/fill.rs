//! Filling a [`TextLayout`]'s lines: pieces shaped and placed greedily, and
//! each line cut at a soft break once it overflows — the rules the parent
//! module's docs give.

use core::ops::Range;

use glam::Vec2;

use super::{PositionedGlyph, TextLayout, TextLine, pieces};
use crate::font::shape::{ShapedGlyph, shape};
use crate::font::{Font, breaks};

/// One piece of the line being filled: a word, or a run of spaces.
#[derive(Clone, Debug)]
struct Piece {
    /// Its bytes in the paragraph.
    bytes: Range<usize>,
    /// Its glyphs in the layout; none for spaces, which draw nothing.
    glyphs: Range<usize>,
    spaces: bool,
    /// Where its pen starts on the line.
    pen: f32,
    /// Where its pen ends.
    end: f32,
    /// A run of spaces' advances, summed; zero for a word.
    advance: f32,
}

/// One paragraph being laid into a layout.
struct Filler<'a> {
    layout: &'a mut TextLayout,
    /// Every glyph as shaped, beside the layout's glyphs.
    shaped: &'a mut Vec<ShapedGlyph>,
    font: &'a Font,
    size: f32,
    wrap: Option<f32>,
    paragraph: &'a str,
    /// Where the paragraph starts in the text.
    start: usize,
    /// Its soft breaks, as bytes of the paragraph.
    soft: Vec<usize>,
    /// The line being filled: its first glyph and its pieces, in order.
    first: usize,
    line: Vec<Piece>,
}

/// Lays every paragraph of `text` into `layout` — its glyphs, faces, bytes
/// and lines — in `font` at `size`, broken to fit `wrap` whole pixels when
/// given, each glyph's shaping written beside it to `shaped`.
pub(super) fn fill(
    layout: &mut TextLayout,
    shaped: &mut Vec<ShapedGlyph>,
    font: &Font,
    size: f32,
    wrap: Option<f32>,
    text: &str,
) {
    for bytes in breaks::paragraphs(text) {
        let paragraph = &text[bytes.clone()];
        let mut filler = Filler {
            first: layout.glyphs.len(),
            layout: &mut *layout,
            shaped: &mut *shaped,
            font,
            size,
            wrap,
            paragraph,
            start: bytes.start,
            soft: breaks::soft_breaks(paragraph),
            line: Vec::new(),
        };
        for (piece, spaces) in pieces(paragraph) {
            filler.add(piece, spaces);
        }
        let kept = filler.words_end(filler.line.len());
        filler.finish(kept);
    }
}

/// Places `piece`'s glyphs in `glyphs` from a pen at `pen`, each at its pen
/// plus its shaper's offset in `shaped`, and records where its pen starts
/// and ends.
fn place(piece: &mut Piece, glyphs: &mut [PositionedGlyph], shaped: &[ShapedGlyph], pen: f32) {
    piece.pen = pen;
    if piece.spaces {
        piece.end = pen + piece.advance;
        return;
    }
    let mut pen = pen;
    for at in piece.glyphs.clone() {
        glyphs[at].offset.x = pen + shaped[at].offset.x;
        pen += shaped[at].advance;
    }
    piece.end = pen;
}

impl Filler<'_> {
    /// Shapes the piece `bytes` of the paragraph — spaces or a word — onto
    /// the line, and cuts the line until it fits.
    fn add(&mut self, bytes: Range<usize>, spaces: bool) {
        let pen = self.line.last().map_or(0.0, |piece| piece.end);
        let first = self.shaped.len();
        shape(
            self.font,
            self.size,
            self.paragraph,
            bytes.clone(),
            self.shaped,
        );
        let mut piece = Piece {
            bytes,
            glyphs: first..first,
            spaces,
            pen,
            end: pen,
            advance: 0.0,
        };
        if spaces {
            piece.advance = self.shaped[first..]
                .iter()
                .map(|glyph| glyph.advance)
                .sum::<f32>();
            self.shaped.truncate(first);
        } else {
            self.push_glyphs(first);
            piece.glyphs = first..self.shaped.len();
        }
        place(&mut piece, &mut self.layout.glyphs, self.shaped, pen);
        self.line.push(piece);
        if spaces {
            return;
        }
        while let Some(wrap) = self.wrap
            && self.right() > wrap
            && let Some((index, glyph)) = self.cut_at(wrap)
        {
            self.cut(index, glyph);
        }
    }

    /// Writes the layout's glyph, face and byte for each shaped glyph from
    /// `first` on, unplaced.
    fn push_glyphs(&mut self, first: usize) {
        for glyph in &self.shaped[first..] {
            self.layout.glyphs.push(PositionedGlyph {
                glyph: glyph.glyph,
                offset: Vec2::ZERO,
            });
            self.layout.faces.push(glyph.face);
            self.layout.bytes.push(self.start + glyph.cluster);
        }
    }

    /// How many of the line's first `count` pieces it keeps once the spaces
    /// they end with go.
    fn words_end(&self, count: usize) -> usize {
        self.line[..count]
            .iter()
            .rposition(|piece| !piece.spaces)
            .map_or(0, |at| at + 1)
    }

    /// The right edge of the line's last word.
    fn right(&self) -> f32 {
        let kept = self.words_end(self.line.len());
        kept.checked_sub(1).map_or(0.0, |last| self.line[last].end)
    }

    /// Where the line is cut to fit `wrap`: before the glyph — as the piece
    /// it is in and its index — at the last soft break with a word before it
    /// whose words before it fit, or else at the first such soft break, the
    /// words before it overflowing as no break can help. `None` when the
    /// line has no such soft break.
    fn cut_at(&self, wrap: f32) -> Option<(usize, usize)> {
        let (mut fitting, mut first) = (None, None);
        // The right edge of the last word before the glyph looked at.
        let mut before: Option<f32> = None;
        for (index, piece) in self.line.iter().enumerate() {
            if piece.spaces {
                continue;
            }
            let mut pen = piece.pen;
            for at in piece.glyphs.clone() {
                let glyph = &self.shaped[at];
                let (byte, right, starts) = if at == piece.glyphs.start {
                    (piece.bytes.start, before, true)
                } else {
                    let starts = self.shaped[at - 1].cluster != glyph.cluster;
                    (glyph.cluster, Some(pen), starts)
                };
                if starts
                    && let Some(right) = right
                    && self.soft.binary_search(&byte).is_ok()
                {
                    first.get_or_insert((index, at));
                    if right <= wrap {
                        fitting = Some((index, at));
                    }
                }
                pen += glyph.advance;
            }
            before = Some(piece.end);
        }
        fitting.or(first)
    }

    /// Ends the line before glyph `glyph` of its piece `index`, and starts the
    /// next with what follows, placed from its left edge.
    fn cut(&mut self, index: usize, glyph: usize) {
        let mut index = index;
        if glyph > self.line[index].glyphs.start {
            self.split(index, glyph);
            index += 1;
        }
        let kept = self.words_end(index);
        self.finish(kept);
        let mut rest: Vec<Piece> = self.line.drain(index..).collect();
        self.first = rest[0].glyphs.start;
        let mut pen = 0.0;
        for piece in &mut rest {
            place(piece, &mut self.layout.glyphs, self.shaped, pen);
            pen = piece.end;
        }
        self.line = rest;
    }

    /// Splits the line's word `index` in two before its glyph `glyph`, each
    /// side shaped again on its own where `harfrust` says cutting there
    /// changes the shaping.
    fn split(&mut self, index: usize, glyph: usize) {
        let piece = self.line[index].clone();
        let byte = self.shaped[glyph].cluster;
        let (mut head, mut tail) = (piece.clone(), piece.clone());
        head.bytes.end = byte;
        tail.bytes.start = byte;
        if self.shaped[glyph].unsafe_to_break {
            let mut halves = Vec::new();
            shape(
                self.font,
                self.size,
                self.paragraph,
                head.bytes.clone(),
                &mut halves,
            );
            let middle = piece.glyphs.start + halves.len();
            shape(
                self.font,
                self.size,
                self.paragraph,
                tail.bytes.clone(),
                &mut halves,
            );
            let end = piece.glyphs.start + halves.len();
            let start = self.start;
            self.layout.glyphs.splice(
                piece.glyphs.clone(),
                halves.iter().map(|glyph| PositionedGlyph {
                    glyph: glyph.glyph,
                    offset: Vec2::ZERO,
                }),
            );
            self.layout
                .faces
                .splice(piece.glyphs.clone(), halves.iter().map(|glyph| glyph.face));
            self.layout.bytes.splice(
                piece.glyphs.clone(),
                halves.iter().map(|glyph| start + glyph.cluster),
            );
            self.shaped.splice(piece.glyphs.clone(), halves);
            for later in &mut self.line[index + 1..] {
                later.glyphs = later.glyphs.start + end - piece.glyphs.end
                    ..later.glyphs.end + end - piece.glyphs.end;
            }
            head.glyphs = piece.glyphs.start..middle;
            tail.glyphs = middle..end;
        } else {
            head.glyphs.end = glyph;
            tail.glyphs.start = glyph;
        }
        place(&mut head, &mut self.layout.glyphs, self.shaped, piece.pen);
        place(&mut tail, &mut self.layout.glyphs, self.shaped, head.end);
        self.line.splice(index..=index, [head, tail]);
    }

    /// Finishes the line with its first `kept` pieces: its glyphs to the end
    /// of the last, and its width to that one's right edge.
    fn finish(&mut self, kept: usize) {
        let last = kept.checked_sub(1).map(|last| &self.line[last]);
        self.layout.lines.push(TextLine {
            glyphs: self.first..last.map_or(self.first, |piece| piece.glyphs.end),
            width: last.map_or(0.0, |piece| piece.end),
            baseline: 0.0,
            offset: 0.0,
        });
    }
}
