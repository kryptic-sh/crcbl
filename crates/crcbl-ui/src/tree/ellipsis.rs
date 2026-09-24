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
//! * A line that does not is cut at a **char** boundary: the longest prefix
//!   that, with its trailing white space dropped and the ellipsis after it,
//!   fits. The crate has no grapheme segmenter and draws one glyph per char, so
//!   no glyph is ever cut in half, but a combining mark can be parted from its
//!   base.
//! * The ellipsis is `…` (U+2026) in a font that has the glyph, and `...` in
//!   one that would draw `.notdef` for it — the bitmap font always.
//! * **A line too narrow for even the ellipsis shows nothing**: the span does
//!   not clip its own text, so a lone `…` would overflow the box it was cut to
//!   fit.
//!
//! The cut is taken after layout from the width the box was given, so it never
//! moves a box: the box is sized by the whole text, as CSS sizes it.

use super::emit::content_width;
use super::style::{NodeStyle, TextOverflow, WhiteSpace};
use super::{Content, Shown, Ui};
use crate::font::layout::{TextLayout, WRAP_EPSILON};
use crate::font::{Font, GlyphId};
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
    pub(super) fn cut_text(&mut self, atlas: &FontAtlas) {
        self.cut.clear();
        for node in &mut self.nodes {
            node.shown = Shown::Whole;
            let Content::Text { start, end } = node.content else {
                continue;
            };
            let style = &node.style;
            if !applies(style) {
                continue;
            }
            let measure = match node.font {
                None => Measure::Bitmap {
                    atlas,
                    scale: style.font_size / NATURAL_FONT_SIZE,
                },
                Some(font) => Measure::Parsed {
                    font,
                    size: style.font_size,
                    line_height: style.text_line_height(font),
                },
            };
            let width = content_width(&self.store.get(node.slot).unrounded);
            let at = self.cut.len();
            if cut(&measure, &self.text[start..end], width, &mut self.cut) {
                node.shown = Shown::Cut {
                    start: at,
                    end: self.cut.len(),
                };
            } else {
                self.cut.truncate(at);
            }
        }
    }
}

/// Whether a text span in `style` is cut to fit; see the module docs.
fn applies(style: &NodeStyle) -> bool {
    style.text_overflow == TextOverflow::Ellipsis
        && style.white_space == WhiteSpace::NoWrap
        && style.overflow.clips()
}

/// The font a span is measured in, at its size.
enum Measure<'a> {
    /// The built-in bitmap font, `scale` times its natural size.
    Bitmap { atlas: &'a FontAtlas, scale: f32 },
    /// A parsed font, laid out unbroken.
    Parsed {
        font: &'a Font,
        size: f32,
        line_height: f32,
    },
}

impl Measure<'_> {
    /// `line`'s width in pixels, measured as layout measures it.
    fn width(&self, line: &str) -> f32 {
        match self {
            Self::Bitmap { atlas, scale } => atlas.text_width(line, *scale),
            Self::Parsed {
                font,
                size,
                line_height,
            } => TextLayout::new(font, line, *size, *line_height, None).width(),
        }
    }

    /// The ellipsis this font can draw.
    fn ellipsis(&self) -> &'static str {
        let c = '\u{2026}';
        let has = match self {
            Self::Bitmap { .. } => glyph_index(c) != NOTDEF_INDEX,
            Self::Parsed { font, .. } => font.glyph_id(c) != GlyphId::NOTDEF,
        };
        if has { ELLIPSIS } else { ELLIPSIS_FALLBACK }
    }
}

/// Appends `text` to `out` with every line too wide for `width` cut to fit, as
/// the module docs say, and returns whether any line was cut.
fn cut(measure: &Measure<'_>, text: &str, width: f32, out: &mut String) -> bool {
    let fit = width.max(0.0) + WRAP_EPSILON;
    let mut scratch = String::new();
    let mut cut_any = false;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if measure.width(line) <= fit {
            out.push_str(line);
            continue;
        }
        cut_any = true;
        let ellipsis = measure.ellipsis();
        let mut fits = |end: usize| {
            scratch.clear();
            scratch.push_str(line[..end].trim_end());
            scratch.push_str(ellipsis);
            measure.width(&scratch) <= fit
        };
        // Every char boundary short of the whole line, which is known not to
        // fit: the first is the empty prefix.
        let ends: Vec<usize> = line.char_indices().map(|(at, _)| at).collect();
        if !fits(0) {
            continue;
        }
        // Kerning can make a longer prefix narrower, so the search keeps the
        // invariant that `ends[low]` fits rather than relying on monotony.
        let (mut low, mut high) = (0, ends.len());
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if fits(ends[mid]) {
                low = mid;
            } else {
                high = mid;
            }
        }
        out.push_str(line[..ends[low]].trim_end());
        out.push_str(ellipsis);
    }
    cut_any
}
