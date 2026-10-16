//! Colour glyphs: which glyphs a font draws in colour.

use skrifa::bitmap::{BitmapFormat, BitmapStrikes};
use skrifa::{FontRef, MetadataProvider};

/// Whether `glyph` of `font` has a `COLR` layer list or paint graph, or a
/// bitmap in a `CBDT` or `sbix` strike. A monochrome `EBDT` strike is not
/// colour.
pub(crate) fn has_color_glyph(font: &FontRef<'_>, glyph: skrifa::GlyphId) -> bool {
    font.color_glyphs().get(glyph).is_some()
        || [BitmapFormat::Cbdt, BitmapFormat::Sbix]
            .into_iter()
            .filter_map(|format| BitmapStrikes::with_format(font, format))
            .any(|strikes| strikes.iter().any(|strike| strike.get(glyph).is_some()))
}
