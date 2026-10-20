//! Caret stops: where a caret stands in one line of text, and where each
//! char's cluster lies, placed as [`TextLayout`](super::TextLayout) places
//! the glyphs.

use core::ops::Range;

use super::pieces;
use crate::font::bidi::{Bidi, visual_order};
use crate::font::shape::shape;
use crate::font::{Font, cluster};

/// Where a caret stands in one line of text, and where each char's cluster
/// lies, from the line's left edge: what [`caret_stops`] measures.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Carets {
    /// Before each char, then at the end, where a caret stands: before a
    /// char, its cluster's leading edge — the left of a left-to-right
    /// cluster, the right of a right-to-left one — and at the end the last
    /// cluster's trailing edge.
    pub stops: Vec<f32>,
    /// Each char's cluster's trailing edge.
    pub trailing: Vec<f32>,
    /// How far right the line reaches.
    pub width: f32,
}

impl Carets {
    /// The stop nearest `x`: of two as near, the one further right, and of
    /// two at the same place, the first.
    #[must_use]
    pub fn nearest(&self, x: f32) -> usize {
        let mut best = 0;
        for (at, &stop) in self.stops.iter().enumerate().skip(1) {
            let (distance, best_distance) = ((stop - x).abs(), (self.stops[best] - x).abs());
            if distance < best_distance || (distance == best_distance && stop > self.stops[best]) {
                best = at;
            }
        }
        best
    }

    /// The char whose cluster `x` falls in: of the chars whose cluster's left
    /// edge is at or before `x`, the last of those with that edge furthest
    /// right — or the first char when there is none.
    #[must_use]
    pub fn char_at(&self, x: f32) -> usize {
        let mut best: Option<(usize, f32)> = None;
        for (at, (&stop, &trailing)) in self.stops.iter().zip(&self.trailing).enumerate() {
            let left = stop.min(trailing);
            if left <= x && best.is_none_or(|(_, edge)| left >= edge) {
                best = Some((at, left));
            }
        }
        best.map_or(0, |(at, _)| at)
    }

    /// The stretch of the line the chars `range` cover — chars of one bidi
    /// level, which lie unbroken on the screen: from the leftmost edge of
    /// their clusters to the rightmost, or nothing at zero for no chars.
    #[must_use]
    pub fn span(&self, range: Range<usize>) -> (f32, f32) {
        range
            .map(|at| {
                let (stop, trailing) = (self.stops[at], self.trailing[at]);
                (stop.min(trailing), stop.max(trailing))
            })
            .reduce(|(left, right), (l, r)| (left.min(l), right.max(r)))
            .unwrap_or((0.0, 0.0))
    }
}

/// Measures `text` — one line, unbroken — in `font` at `size` into `out`,
/// placed as [`TextLayout`](super::TextLayout) places the glyphs: bidi levels
/// and all. Spaces the line ends with, which a layout does not place, go past
/// its end on the side its paragraph ends at — to the left of a right-to-left
/// line.
///
/// A cluster's edges are where the shaper put its glyphs. The shaper may join
/// several clusters into one glyph — a ligature — and they then share its
/// width evenly, from its leading edge. A char inside a cluster, where no
/// caret stands, takes its cluster's edges.
pub(crate) fn caret_stops(font: &Font, text: &str, size: f32, out: &mut Carets) {
    line_stops(
        font,
        text,
        Bidi::new(text).as_ref(),
        0..text.len(),
        size,
        out,
    );
}

/// [`caret_stops`] of the bytes `line` of `paragraph` — one line of it as a
/// [`TextLayout`](super::TextLayout) broke it, the spaces it was broken at
/// its own — shaped with the rest of the paragraph as context and at the
/// levels `bidi` gives the paragraph, as the layout shaped and placed it:
/// each stop, from the line's left edge, for each char of the line.
pub(super) fn line_stops(
    font: &Font,
    paragraph: &str,
    bidi: Option<&Bidi<'_>>,
    line: Range<usize>,
    size: f32,
    out: &mut Carets,
) {
    out.stops.clear();
    out.trailing.clear();
    let text = &paragraph[line.clone()];
    // Each piece in logical order: its bytes in the paragraph, its glyphs in
    // `shaped` and whether it runs right to left.
    let mut pieces_shaped: Vec<(Range<usize>, Range<usize>, bool)> = Vec::new();
    let mut shaped = Vec::new();
    let mut words_end = 0;
    for (piece, spaces) in pieces(text) {
        let piece = line.start + piece.start..line.start + piece.end;
        let runs = match bidi {
            Some(bidi) => bidi.runs(piece),
            None => vec![(piece, false)],
        };
        for (run, rtl) in runs {
            let first = shaped.len();
            shape(font, size, paragraph, run.clone(), rtl, &mut shaped);
            pieces_shaped.push((run, first..shaped.len(), rtl));
        }
        if !spaces {
            words_end = pieces_shaped.len();
        }
    }
    // Every glyph's pen: the pieces before the trailing spaces in visual
    // order from zero, then those spaces outward on the line's end side.
    let mut pens = vec![0.0; shaped.len()];
    let mut place = |index: usize, pen: f32| -> f32 {
        let (_, glyphs, rtl) = &pieces_shaped[index];
        let mut pen = pen;
        let mut put = |at: usize| {
            pens[at] = pen;
            pen += shaped[at].advance;
        };
        if *rtl {
            glyphs.clone().rev().for_each(&mut put);
        } else {
            glyphs.clone().for_each(&mut put);
        }
        pen
    };
    let order: Vec<usize> = match bidi {
        Some(bidi) => {
            let levels = bidi.line_levels(line.clone());
            visual_order(
                &pieces_shaped[..words_end]
                    .iter()
                    .map(|(bytes, _, _)| levels[bytes.start])
                    .collect::<Vec<_>>(),
            )
        }
        None => (0..words_end).collect(),
    };
    let mut pen = 0.0;
    for index in order {
        pen = place(index, pen);
    }
    let rtl = bidi.is_some_and(Bidi::rtl);
    let mut left = 0.0;
    for (index, (_, glyphs, _)) in pieces_shaped.iter().enumerate().skip(words_end) {
        if rtl {
            left -= shaped[glyphs.clone()]
                .iter()
                .map(|glyph| glyph.advance)
                .sum::<f32>();
            place(index, left);
        } else {
            pen = place(index, pen);
        }
    }
    out.width = pen;

    // Each shaped cluster: its first byte, its left and right edges, its
    // advances summed and whether it runs right to left.
    let mut shaped_clusters: Vec<(usize, f32, f32, f32, bool)> = Vec::new();
    for (_, glyphs, rtl) in &pieces_shaped {
        for at in glyphs.clone() {
            let glyph = &shaped[at];
            let (left, right) = (pens[at], pens[at] + glyph.advance);
            match shaped_clusters.last_mut() {
                Some((start, low, high, width, _)) if *start == glyph.cluster => {
                    *low = low.min(left);
                    *high = high.max(right);
                    *width += glyph.advance;
                }
                _ => shaped_clusters.push((glyph.cluster, left, right, glyph.advance, *rtl)),
            }
        }
    }
    let starts: Vec<usize> = cluster::clusters(text)
        .map(|range| line.start + range.start)
        .collect();
    let mut edges = starts.iter().map(|&start| {
        // The shaped cluster it lies in, and the cluster starts that share it.
        let at = shaped_clusters.partition_point(|&(first, ..)| first <= start);
        let Some(&(first, left, right, width, rtl)) =
            at.checked_sub(1).map(|at| &shaped_clusters[at])
        else {
            return (0.0, 0.0);
        };
        let end = shaped_clusters.get(at).map_or(line.end, |&(next, ..)| next);
        let sharing =
            &starts[starts.partition_point(|&s| s < first)..starts.partition_point(|&s| s < end)];
        // A shaped cluster that starts inside a rule cluster counts its own
        // start as one more share before the first.
        let lead = usize::from(sharing.first() != Some(&first));
        let index = sharing.partition_point(|&s| s < start) + lead;
        let shares = sharing.len() + lead;
        let edge = |share: usize| match (share == shares, rtl) {
            (true, false) => right,
            (true, true) => left,
            (false, false) => left + width * share as f32 / shares as f32,
            (false, true) => right - width * share as f32 / shares as f32,
        };
        (edge(index), edge(index + 1))
    });
    let mut current = (0.0, 0.0);
    let mut next_start = starts.iter().peekable();
    for (byte, _) in text.char_indices() {
        if next_start
            .next_if(|&&start| start == line.start + byte)
            .is_some()
        {
            current = edges.next().unwrap_or(current);
        }
        out.stops.push(current.0);
        out.trailing.push(current.1);
    }
    out.stops.push(out.trailing.last().copied().unwrap_or(0.0));
}
