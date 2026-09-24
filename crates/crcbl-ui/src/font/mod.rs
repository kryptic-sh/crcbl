//! Real fonts: parsing, the coverage rasteriser, the glyph atlas and text
//! layout — UI rung 5.
//!
//! ```text
//!  TTF bytes ──skrifa──▶ Font (cmap, advances, metrics, GPOS pair kerning)
//!                          │
//!   text ──layout──▶ TextLayout (advanced, kerned, wrapped glyph positions)
//!                          │
//!                          ▼
//!   DrawList::glyphs ──to_triangles_split──▶ GlyphAtlas::glyph
//!                                             │  hinted outline (skrifa)
//!                                             │  ──raster──▶ coverage mask
//!                                             ▼
//!                                   shelf-packed page, dirty rectangle
//! ```
//!
//! * [`Font`] — one parsed font: glyph ids through the cmap, horizontal
//!   advances, line metrics and pair kerning, read through `skrifa` and its
//!   `read-fonts` (`skrifa::raw`).
//! * [`raster`] — this engine's own coverage rasteriser.
//! * [`atlas`] — the glyph atlas: shelf-packed pages, LRU eviction and a
//!   per-frame rasterisation budget.
//! * [`layout`] — advancing, kerning and greedy line breaking.
//!
//! # Two fonts, two families
//!
//! [`FontFamily::Bitmap`] is [`crate::text::FontAtlas`], the built-in 8×13
//! pixel font every panel, menu and console still draws with.
//! [`FontFamily::Sans`] is [`Font::sans`]: Atkinson Hyperlegible Regular, by
//! the Braille Institute of America, under the SIL Open Font License 1.1 —
//! `crates/crcbl-ui/fonts/OFL.txt` is its licence, committed beside
//! `crates/crcbl-ui/fonts/AtkinsonHyperlegible-Regular.ttf` and embedded into
//! every binary that links this crate.
//!
//! # Latin-1 first
//!
//! Every codepoint goes through the font's cmap, and one the font has no glyph
//! for draws `.notdef`. Kerning is read for the pairs of glyphs Latin-1 maps
//! to; there is no shaping, so no ligatures, no marks positioned and no
//! right-to-left text.

pub mod atlas;
mod kern;
pub mod layout;
pub mod raster;

use core::fmt;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

/// The committed UI font's bytes: Atkinson Hyperlegible Regular. See the module
/// docs for its licence.
pub const SANS_TTF: &[u8] = include_bytes!("../../fonts/AtkinsonHyperlegible-Regular.ttf");

/// Which font a text node draws in: `font-family` in a stylesheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// [`crate::text::FontAtlas`]'s built-in 8×13 bitmap font — `bitmap` in a
    /// stylesheet, and the initial value, so every panel that names no family
    /// draws as it always has.
    #[default]
    Bitmap,
    /// [`Font::sans`] — `sans-serif` or `"Atkinson Hyperlegible"` in a
    /// stylesheet.
    Sans,
}

impl FontFamily {
    /// The parsed font this family draws with, or `None` for the bitmap font.
    #[must_use]
    pub fn font(self) -> Option<&'static Font> {
        match self {
            Self::Bitmap => None,
            Self::Sans => Some(Font::sans()),
        }
    }
}

/// Names one parsed [`Font`] for as long as the process runs: what the glyph
/// atlas keys a glyph's font by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(u32);

/// A glyph's index in its font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphId(pub u32);

impl GlyphId {
    /// `.notdef`, glyph zero in every font: what a codepoint with no glyph
    /// draws.
    pub const NOTDEF: Self = Self(0);
}

/// Why [`Font::parse`] refused some bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// `skrifa` could not read the font's table directory.
    Unreadable(String),
    /// The font's `head` table gives no units per em, so nothing in it can be
    /// scaled to a pixel size.
    NoUnitsPerEm,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "the font cannot be read: {why}"),
            Self::NoUnitsPerEm => f.write_str("the font's head table gives no units per em"),
        }
    }
}

impl std::error::Error for FontError {}

/// A font's line metrics, in font units: the
/// ascender, descender and line gap, as `skrifa` selects them from `hhea` or
/// `OS/2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Font units per em: what a pixel size is divided by.
    pub units_per_em: u16,
    /// Baseline to the top of the line's alignment box, positive upwards.
    pub ascent: f32,
    /// Baseline to its bottom, **negative** below the baseline, as the font
    /// stores it.
    pub descent: f32,
    /// Extra space the font recommends between one line and the next.
    pub line_gap: f32,
}

impl FontMetrics {
    /// Pixels per font unit at `size` pixels per em.
    #[must_use]
    pub fn scale(&self, size: f32) -> f32 {
        size / f32::from(self.units_per_em)
    }

    /// `line-height: normal` at `size`: ascent to descent plus the line gap.
    #[must_use]
    pub fn normal_line_height(&self, size: f32) -> f32 {
        (self.ascent - self.descent + self.line_gap) * self.scale(size)
    }
}

/// The first codepoint past Latin-1: everything below it is looked up in a
/// table built at parse time, everything from it on goes through the cmap.
const LATIN1_END: u32 = 0x100;

/// One parsed font. See the module docs.
///
/// Parsing reads everything layout needs per glyph up front — the Latin-1
/// cmap, every advance and the kerning of every Latin-1 pair — so measuring
/// and laying out text touches no table. Outlines are read when the atlas
/// rasterises a glyph.
pub struct Font {
    id: FontId,
    data: &'static [u8],
    metrics: FontMetrics,
    /// Glyph ids of the codepoints below [`LATIN1_END`]; zero where the font
    /// has none.
    latin1: [u32; LATIN1_END as usize],
    /// Every glyph's advance, in font units.
    advances: Box<[f32]>,
    /// `(left, right)` to the first glyph's advance adjustment, in font units.
    /// Only non-zero pairs are held.
    kerning: HashMap<(u32, u32), f32>,
}

impl fmt::Debug for Font {
    /// The id and the metrics; not the tables.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("id", &self.id)
            .field("metrics", &self.metrics)
            .field("glyphs", &self.advances.len())
            .field("kerning_pairs", &self.kerning.len())
            .finish_non_exhaustive()
    }
}

impl Font {
    /// The committed UI font, parsed once per process.
    ///
    /// # Panics
    ///
    /// If [`SANS_TTF`] does not parse, which the crate's own tests rule out:
    /// the bytes are compiled in, so they are the same bytes the tests read.
    #[must_use]
    pub fn sans() -> &'static Self {
        static SANS: OnceLock<Font> = OnceLock::new();
        SANS.get_or_init(|| Self::parse(SANS_TTF).expect("the committed UI font parses"))
    }

    /// Parses `data`, which lives as long as the process.
    ///
    /// A font whose GPOS table cannot be read parses with no kerning, and says
    /// so through `crcbl_core::warn!`: a font that draws unkerned is still a
    /// font.
    ///
    /// # Errors
    ///
    /// [`FontError`] when the table directory cannot be read or the font has
    /// no units per em.
    pub fn parse(data: &'static [u8]) -> Result<Self, FontError> {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);

        let font = FontRef::new(data).map_err(|error| FontError::Unreadable(error.to_string()))?;
        let unscaled = font.metrics(Size::unscaled(), LocationRef::default());
        if unscaled.units_per_em == 0 {
            return Err(FontError::NoUnitsPerEm);
        }
        let metrics = FontMetrics {
            units_per_em: unscaled.units_per_em,
            ascent: unscaled.ascent,
            descent: unscaled.descent,
            line_gap: unscaled.leading,
        };

        let charmap = font.charmap();
        let mut latin1 = [0; LATIN1_END as usize];
        for (codepoint, glyph) in latin1.iter_mut().enumerate() {
            *glyph = charmap
                .map(codepoint as u32)
                .map_or(0, skrifa::GlyphId::to_u32);
        }

        let glyph_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
        let advances = (0..glyph_metrics.glyph_count())
            .map(|glyph| {
                glyph_metrics
                    .advance_width(skrifa::GlyphId::new(glyph))
                    .unwrap_or(0.0)
            })
            .collect();

        let mut pair_glyphs: Vec<u32> = latin1.iter().copied().filter(|&g| g != 0).collect();
        pair_glyphs.sort_unstable();
        pair_glyphs.dedup();
        let kerning = match kern::pair_kerning(&font, &pair_glyphs) {
            Ok(kerning) => kerning,
            Err(error) => {
                crcbl_core::warn!(
                    "font: the GPOS table cannot be read ({error}); the font draws unkerned"
                );
                HashMap::new()
            }
        };

        Ok(Self {
            id: FontId(NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            data,
            metrics,
            latin1,
            advances,
            kerning,
        })
    }

    /// This font's identity.
    #[must_use]
    pub const fn id(&self) -> FontId {
        self.id
    }

    /// The bytes it was parsed from.
    #[must_use]
    pub const fn data(&self) -> &'static [u8] {
        self.data
    }

    /// Its line metrics.
    #[must_use]
    pub const fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// How many glyphs it holds.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.advances.len()
    }

    /// The glyph `c` draws as: through the cmap, or [`GlyphId::NOTDEF`] when
    /// the font maps it to nothing.
    #[must_use]
    pub fn glyph_id(&self, c: char) -> GlyphId {
        let codepoint = c as u32;
        if codepoint < LATIN1_END {
            return GlyphId(self.latin1[codepoint as usize]);
        }
        FontRef::new(self.data)
            .ok()
            .and_then(|font| font.charmap().map(codepoint))
            .map_or(GlyphId::NOTDEF, |glyph| GlyphId(glyph.to_u32()))
    }

    /// `glyph`'s advance in font units; zero for a glyph the font does not
    /// have.
    #[must_use]
    pub fn advance(&self, glyph: GlyphId) -> f32 {
        self.advances.get(glyph.0 as usize).copied().unwrap_or(0.0)
    }

    /// How far `right` moves when it follows `left`, in font units: the pair
    /// adjustment of the GPOS `kern` feature, zero for a pair it does not
    /// kern. Only pairs of glyphs Latin-1 maps to are read.
    #[must_use]
    pub fn kerning(&self, left: GlyphId, right: GlyphId) -> f32 {
        self.kerning.get(&(left.0, right.0)).copied().unwrap_or(0.0)
    }

    /// How many glyph pairs the font kerns.
    #[must_use]
    pub fn kerning_pairs(&self) -> usize {
        self.kerning.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The committed font parses into what layout reads**: its units per em
    /// and line metrics as `skrifa` reports them, a glyph for every printable
    /// Latin-1 codepoint, and kerning.
    #[test]
    fn the_committed_font_parses_with_latin1_and_kerning() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let metrics = reference.metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(font.metrics().units_per_em, metrics.units_per_em);
        assert_eq!(font.metrics().ascent, metrics.ascent);
        assert_eq!(font.metrics().descent, metrics.descent);
        assert!(font.metrics().ascent > 0.0 && font.metrics().descent < 0.0);

        let printable = (0x20u32..0x7f).chain(0xa0..0x100);
        for codepoint in printable {
            let c = char::from_u32(codepoint).expect("Latin-1 is valid");
            assert_ne!(
                font.glyph_id(c),
                GlyphId::NOTDEF,
                "U+{codepoint:04X} has no glyph"
            );
        }
        assert!(
            font.kerning_pairs() > 0,
            "the committed font's GPOS kerning was not read"
        );
    }

    /// **A codepoint the cmap does not map draws `.notdef`**, inside Latin-1
    /// and past it, and one it does map is read through the cmap past Latin-1
    /// too.
    #[test]
    fn an_unmapped_codepoint_is_notdef_and_a_mapped_one_is_not() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let charmap = reference.charmap();
        for c in ['\u{1}', '\u{7f}', '\u{4e2d}', '\u{1f600}'] {
            assert_eq!(charmap.map(c as u32), None, "{c:?} is mapped after all");
            assert_eq!(font.glyph_id(c), GlyphId::NOTDEF, "{c:?}");
        }
        // Past Latin-1 and mapped: the Euro sign and the en dash.
        for c in ['€', '–'] {
            let want = charmap.map(c as u32).expect("the font maps it");
            assert_eq!(font.glyph_id(c), GlyphId(want.to_u32()), "{c:?}");
            assert_ne!(font.glyph_id(c), GlyphId::NOTDEF);
        }
    }

    /// **Bytes that are not a font are refused**, not panicked on.
    #[test]
    fn bytes_that_are_not_a_font_are_refused() {
        assert!(matches!(
            Font::parse(b"not a font at all"),
            Err(FontError::Unreadable(_))
        ));
    }

    /// Advances are the font's `hmtx` widths, one per glyph.
    #[test]
    fn advances_are_the_fonts_own() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let metrics = reference.glyph_metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(font.glyph_count(), metrics.glyph_count() as usize);
        for c in ['A', 'i', 'W', ' ', 'é'] {
            let glyph = font.glyph_id(c);
            assert_eq!(
                Some(font.advance(glyph)),
                metrics.advance_width(skrifa::GlyphId::new(glyph.0)),
                "{c:?}"
            );
        }
        assert_eq!(font.advance(GlyphId(u32::MAX)), 0.0);
    }

    /// Two parses are two identities, so an atlas never confuses their glyphs.
    #[test]
    fn every_parse_is_its_own_id() {
        let a = Font::parse(SANS_TTF).expect("parses");
        let b = Font::parse(SANS_TTF).expect("parses");
        assert_ne!(a.id(), b.id());
    }
}
