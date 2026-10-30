//! `text-overflow: ellipsis`: a text span's lines cut to fit its box.
//!
//! # Where the property applies
//!
//! CSS gives `text-overflow` to a block container that clips its inline
//! content. Here the text lives in a span, a leaf box whose content is its
//! text and nothing else — the block container of that text — so the span's
//! own style decides: **a span is cut when it is `text-overflow: ellipsis`,
//! `white-space: nowrap` and its `overflow` clips** (`hidden` or `scroll`).
//! `text-overflow` is not inherited, as in CSS, so a block setting it cuts none
//! of the spans inside it.
//!
//! # How a line is cut
//!
//! * Each line — the text between newlines — is judged on its own, against the
//!   span's content-box width as layout measured it (the unrounded one, the
//!   width a wrapped span is broken at). A line that fits is left whole.
//! * A line that does not is cut at a **cluster** boundary
//!   ([`crate::font::cluster`]): the longest prefix that, with its trailing
//!   white space dropped and the ellipsis after it, fits. So a mark is never
//!   parted from its base, nor an emoji sequence or a flag cut in two. A
//!   ligature the shaper makes of two clusters can still be cut between them,
//!   and then draws as its two parts.
//! * The ellipsis is `…` (U+2026) in a font that has the glyph, and `...` in
//!   one that would draw `.notdef` for it — the bitmap font always.
//! * **A line too narrow for even the ellipsis shows nothing**, rather than
//!   drawing a partially clipped ellipsis. The span's own overflow clip applies
//!   to its text at the padding box independently of this line-cutting rule.
//!
//! The cut is taken after layout from the width the box was given, so it never
//! moves a box: the box is sized by the whole text, as CSS sizes it.

use std::sync::Arc;

use super::emit::content_width;
use super::style::{NodeStyle, TextOverflow, WhiteSpace};
use super::{Content, Shown, Ui};
use crate::font::layout::{TextLayout, WRAP_EPSILON};
use crate::font::{Font, GlyphId, cluster};
use crate::text::{FontAtlas, NOTDEF_INDEX, glyph_index};
use crate::widget::NATURAL_FONT_SIZE;

/// What a cut line ends with in a font that has the glyph.
pub(super) const ELLIPSIS: &str = "\u{2026}";

/// What a cut line ends with in a font that does not.
pub(super) const ELLIPSIS_FALLBACK: &str = "...";

impl Ui {
    /// Decides what every node shows: a text span under the ellipsis rule its
    /// text cut to the width it was just laid out at, everything else what it
    /// was built with. See the module docs.
    ///
    /// A `nowrap` span in a parsed font is handed the layout its measurement
    /// kept, when it shows its text whole, so neither this nor drawing it
    /// shapes that text again. **Its cut is kept beside that layout** with the
    /// width it was cut at, and handed back while both are unchanged, so a
    /// steady frame cuts none of its spans again.
    pub(super) fn cut_text(&mut self, atlas: &FontAtlas) {
        self.cut.clear();
        for node in &mut self.nodes {
            node.shown = Shown::Whole;
            node.shown_layout = None;
            let Content::Text { start, end } = node.content else {
                continue;
            };
            let style = &node.style;
            let text = &self.text[start..end];
            // A clone, so the measure borrows it rather than the node it is
            // about to cut.
            let font = node.font.clone();
            let nowrap = font
                .as_ref()
                .filter(|_| style.white_space == WhiteSpace::NoWrap)
                .map(|font| (font, style.font_size, style.text_line_height(font)));
            let measured = nowrap.and_then(|(font, size, line_height)| {
                self.measure
                    .nowrap_layout(node.content_hash, font, text, size, line_height)
            });
            if !applies(style) {
                node.shown_layout = measured.cloned();
                continue;
            }
            let width = content_width(&self.store.get(node.slot).unrounded);
            let at = self.cut.len();
            if let Some((font, size, line_height)) = nowrap
                && let Some(kept) =
                    self.measure
                        .kept_cut(node.content_hash, font, text, size, line_height, width)
            {
                node.shown_layout.clone_from(&kept.layout);
                if let Some(shown) = &kept.shown {
                    self.cut.push_str(shown);
                    node.shown = Shown::Cut {
                        start: at,
                        end: self.cut.len(),
                    };
                }
                continue;
            }
            let measure = match &font {
                None => Measure::Bitmap {
                    atlas,
                    scale: style.font_size / NATURAL_FONT_SIZE,
                },
                Some(font) => Measure::Parsed {
                    font,
                    size: style.font_size,
                    line_height: style.text_line_height(font),
                    measured,
                },
            };
            let cut = cut(&measure, text, width, &mut self.cut);
            let shown = if cut.any {
                node.shown = Shown::Cut {
                    start: at,
                    end: self.cut.len(),
                };
                Some(Box::from(&self.cut[at..]))
            } else {
                self.cut.truncate(at);
                None
            };
            if measured.is_some() {
                self.measure.keep_cut(
                    node.content_hash,
                    KeptCut {
                        width: width.to_bits(),
                        shown,
                        layout: cut.layout.clone(),
                    },
                );
            }
            node.shown_layout = cut.layout;
        }
    }
}

/// A span's text as [`Ui::cut_text`] last cut it, kept beside the layout of
/// that text in the measure cache: what the cut showed, at what width.
#[derive(Clone, Debug)]
pub(crate) struct KeptCut {
    /// The bits of the content-box width it was cut at.
    pub(super) width: u32,
    /// The text cut to fit, `None` when it was shown whole.
    shown: Option<Box<str>>,
    /// What [`Cut::layout`] answered.
    layout: Option<Arc<TextLayout>>,
}

/// Whether a text span in `style` is cut to fit; see the module docs.
fn applies(style: &NodeStyle) -> bool {
    style.text_overflow == TextOverflow::Ellipsis
        && style.white_space == WhiteSpace::NoWrap
        && style.overflow.clips()
}

/// The font a span is measured in, at its size.
pub(super) enum Measure<'a> {
    /// The built-in bitmap font, `scale` times its natural size.
    Bitmap { atlas: &'a FontAtlas, scale: f32 },
    /// A parsed font, laid out unbroken.
    Parsed {
        font: &'a Font,
        size: f32,
        line_height: f32,
        /// The span's whole text as layout measured it, when that kept it.
        measured: Option<&'a Arc<TextLayout>>,
    },
}

impl Measure<'_> {
    /// `line`'s width in pixels, measured as layout measures it, and in a
    /// parsed font the layout it was measured from: the one layout kept, when
    /// `line` is the span's whole text.
    pub(super) fn width(&self, line: &str) -> (f32, Option<Arc<TextLayout>>) {
        match self {
            Self::Bitmap { atlas, scale } => (atlas.text_width(line, *scale), None),
            Self::Parsed {
                font,
                size,
                line_height,
                measured,
            } => {
                let layout = measured
                    .filter(|layout| layout.lays_out(font, line, *size, *line_height))
                    .map_or_else(
                        || Arc::new(TextLayout::nowrap(font, line, *size, *line_height)),
                        Arc::clone,
                    );
                (layout.width(), Some(layout))
            }
        }
    }

    /// Each prefix of the line `layout` lays out that ends at one of `ends`,
    /// with `ellipsis` after it, settled against `fit` from `layout`'s
    /// shaping where it can be: see [`Estimate`]. `None` in the bitmap font,
    /// which measures without shaping, and for a line a hard break other than
    /// a newline breaks in two.
    fn estimate(
        &self,
        layout: &TextLayout,
        ends: &[usize],
        ellipsis: &str,
        fit: f32,
    ) -> Option<Estimate> {
        let Self::Parsed { font, size, .. } = self else {
            return None;
        };
        if layout.lines().len() != 1 {
            return None;
        }
        let scale = font.metrics().scale(*size);
        let ellipsis: f32 = ellipsis
            .chars()
            .map(|c| font.advance(font.glyph_id(c)) * scale)
            .sum();
        let slack = ELLIPSIS_KERN_EM * size;
        // Every glyph's and run of spaces' start, advance and whether a cut
        // there is safe, in logical order; a run of spaces always is.
        let mut starts: Vec<(usize, f32, bool)> = layout
            .shaped_advances()
            .map(|(byte, advance, unsafe_to_break)| (byte, advance, !unsafe_to_break))
            .chain(
                layout
                    .space_runs()
                    .iter()
                    .map(|&(byte, advance)| (byte, advance, true)),
            )
            .collect();
        starts.sort_by_key(|&(byte, ..)| byte);
        let (mut next, mut before) = (0, 0.0);
        let settled = ends
            .iter()
            .map(|&end| {
                // `ends` never falls, so the advances summed so far stay.
                while let Some(&(byte, advance, _)) = starts.get(next)
                    && byte < end
                {
                    before += advance;
                    next += 1;
                }
                // A prefix ending inside a glyph's cluster — a ligature's —
                // or where a cut could shape the glyphs before it otherwise
                // is not settled.
                let exact = end == 0
                    || starts
                        .get(next)
                        .is_some_and(|&(byte, _, safe)| byte == end && safe);
                let width = before + ellipsis;
                if !exact || (width - fit).abs() <= slack {
                    None
                } else {
                    Some(width < fit)
                }
            })
            .collect();
        Some(Estimate { settled })
    }

    /// The ellipsis this font can draw.
    pub(super) fn ellipsis(&self) -> &'static str {
        let c = '\u{2026}';
        let has = match self {
            Self::Bitmap { .. } => glyph_index(c) != NOTDEF_INDEX,
            Self::Parsed { font, .. } => font.glyph_id(c) != GlyphId::NOTDEF,
        };
        if has { ELLIPSIS } else { ELLIPSIS_FALLBACK }
    }
}

/// What [`cut`] did to a span's text.
pub(super) struct Cut {
    /// Whether any line was cut.
    pub(super) any: bool,
    /// What the text came out as, laid out unbroken in a parsed font, when it
    /// is one line and was laid out whole: the layout of the line that fit, or
    /// of the cut prefix and its ellipsis that did.
    pub(super) layout: Option<Arc<TextLayout>>,
}

/// Appends `text` to `out` with every line too wide for `width` cut to fit, as
/// the module docs say.
///
/// A line is cut by a binary search over its cluster boundaries. In a parsed
/// font each probe is first asked of the line's [`Estimate`], from its
/// shaping as a whole, and only the probes it cannot settle — and the prefix
/// the search lands on, whose layout is kept — are shaped with their
/// ellipsis. In a joining script the estimate settles only the probes that
/// end at a space, and every probe inside a word is shaped; in the bitmap
/// font every probe is measured.
///
/// **The cut lands where shaping every probe would only as far as the
/// estimate's margin, [`ELLIPSIS_KERN_EM`], holds**: that margin was
/// measured, not proven. Should the prefix landed on not fit after all, the
/// line is searched again with every probe shaped — but that catches only an
/// estimate that wrongly accepted the landing prefix. One that wrongly
/// rejected a prefix goes unnoticed, and the line is cut shorter than it
/// could be, though still to a prefix that fits.
pub(super) fn cut(measure: &Measure<'_>, text: &str, width: f32, out: &mut String) -> Cut {
    let fit = width.max(0.0) + WRAP_EPSILON;
    let mut scratch = String::new();
    let mut cut_any = false;
    // What the last line came out as, laid out.
    let mut shown = None;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let (whole, layout) = measure.width(line);
        if whole <= fit {
            out.push_str(line);
            shown = layout;
            continue;
        }
        cut_any = true;
        let ellipsis = measure.ellipsis();
        // Every cluster boundary short of the whole line, which is known not
        // to fit, as the end of the prefix a cut there leaves once its
        // trailing white space is dropped: the first is the empty prefix.
        let ends: Vec<usize> = cluster::clusters(line)
            .map(|range| line[..range.start].trim_end().len())
            .collect();
        let estimate = layout
            .as_deref()
            .and_then(|layout| measure.estimate(layout, &ends, ellipsis, fit));
        let mut probe = Probe {
            measure,
            line,
            ellipsis,
            fit,
            scratch: &mut scratch,
            fitted: None,
        };
        let settled = |index: usize| {
            estimate
                .as_ref()
                .and_then(|estimate| estimate.settles(index))
        };
        let mut at = search(ends.len(), |index| {
            settled(index).unwrap_or_else(|| probe.fits(ends[index]))
        });
        // The prefix landed on was settled by its estimate, so it is shaped
        // now, for its layout; should it not fit after all, the estimate was
        // wrong, and the line is searched again shaping every probe. A prefix
        // the estimate wrongly rejected is not caught here.
        if let Some(index) = at
            && probe
                .fitted
                .as_ref()
                .is_none_or(|(end, _)| *end != ends[index])
            && !probe.fits(ends[index])
        {
            at = search(ends.len(), |index| probe.fits(ends[index]));
        }
        let Some(at) = at else {
            continue;
        };
        out.push_str(&line[..ends[at]]);
        out.push_str(ellipsis);
        shown = probe.fitted.and_then(|(_, layout)| layout);
    }
    Cut {
        any: cut_any,
        // A layout of the last line is not one of the whole text.
        layout: shown.filter(|_| !text.contains('\n')),
    }
}

/// One line's prefixes measured with the ellipsis after them, for [`cut`].
struct Probe<'a> {
    measure: &'a Measure<'a>,
    line: &'a str,
    ellipsis: &'static str,
    /// The width a prefix must fit.
    fit: f32,
    scratch: &'a mut String,
    /// The end of the last prefix measured that fit, and its layout in a
    /// parsed font.
    fitted: Option<(usize, Option<Arc<TextLayout>>)>,
}

impl Probe<'_> {
    /// Whether the line's prefix ending at byte `end`, with the ellipsis
    /// after it, fits.
    fn fits(&mut self, end: usize) -> bool {
        self.scratch.clear();
        self.scratch.push_str(&self.line[..end]);
        self.scratch.push_str(self.ellipsis);
        let (width, layout) = self.measure.width(self.scratch);
        let fits = width <= self.fit;
        if fits {
            self.fitted = Some((end, layout));
        }
        fits
    }
}

/// The index of the prefix, of `count`, a line is cut to, binary searched with
/// `fits`; `None` when not even the empty prefix, the first, fits.
///
/// Kerning can make a longer prefix narrower, so the search keeps the
/// invariant that its `low` fits rather than relying on monotony: it lands on
/// a prefix that fits with the next one not fitting.
fn search(count: usize, mut fits: impl FnMut(usize) -> bool) -> Option<usize> {
    if !fits(0) {
        return None;
    }
    let (mut low, mut high) = (0, count);
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if fits(mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    Some(low)
}

/// How far from its [`Estimate`] a prefix and its ellipsis may shape, in ems,
/// where the estimate is otherwise exact: what the ellipsis kerns against the
/// prefix's last glyph by, and the ellipsis' own shaping against its nominal
/// advance, and the error sources [`Estimate`] lists. A sweep of Roboto,
/// FreeSans, FreeSerif, Liberation Serif, Adwaita Sans and the committed font
/// on 2026-10-31 found at most 0.11 em, Roboto's `T…`, and a review's sweep of
/// 18 system fonts the same; the margin is over twice that, for kerning in
/// fonts no sweep has seen, at the cost of shaping the probes it leaves
/// unsettled.
const ELLIPSIS_KERN_EM: f32 = 0.25;

/// A parsed-font line's cut prefixes measured from the line's shaping as a
/// whole, without shaping any of them.
///
/// A prefix's width is estimated as the advances of the glyphs and spaces the
/// whole line shaped before its end, plus the ellipsis' nominal advance. It
/// settles a probe only where the prefix ends at a space or where `harfrust`
/// says cutting before the glyph there shapes neither side differently, and
/// only when it is further from the width than [`ELLIPSIS_KERN_EM`]. Inside a
/// ligature, across a kerning pair or inside a word of a joining script it
/// settles nothing.
///
/// **Where it settles, it is not proven exact**; the margin has to cover:
///
/// * what the ellipsis kerns against the prefix's last glyph by, and its own
///   shaping against its nominal advance;
/// * a contextual rule — `calt`, a ligature, a word-final form — that fires
///   only when the ellipsis follows the prefix's last glyph, which the whole
///   line's shaping never flagged unsafe to break, as the ellipsis was not in
///   it;
/// * joining across a change of face: [`crate::font::shape::shape`] makes the
///   first glyph of each face's run safe to break, though Arabic joining reads
///   the neighbouring run as context;
/// * a mirrored bracket's advance, should cutting the line change the
///   bracket's direction.
struct Estimate {
    /// For each prefix: whether it fits, where its estimate is exact and
    /// further from the width than [`ELLIPSIS_KERN_EM`].
    settled: Vec<Option<bool>>,
}

impl Estimate {
    /// Whether prefix `index` fits, when its estimate settles it.
    fn settles(&self, index: usize) -> Option<bool> {
        self.settled[index]
    }
}
