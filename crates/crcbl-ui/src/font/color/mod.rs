//! Colour glyphs: emoji drawn from a font's own pictures rather than its
//! outlines, as RGBA pictures in the [`ImageAtlas`](crate::image::ImageAtlas).
//!
//! ```text
//!   DrawList glyph run ──resolve_color_glyphs──▶ ColorGlyphCache::glyph
//!                                                 │ miss
//!                                                 ▼
//!                      CBDT / sbix strike PNG ──decode, resize──▶ RGBA
//!                                                 │
//!                                                 ▼
//!                          ImageAtlas::register ──▶ DrawCommand::Image quad
//! ```
//!
//! * `bitmap` — CBDT/CBLC and sbix strikes: the strike nearest the size, its
//!   PNG decoded by `crcbl_image::png`, scaled by [`crcbl_image::resize`]
//!   and placed by the strike's metrics.
//! * [`ColorGlyphCache`] — the rasterised glyphs, held in the caller's image
//!   atlas and evicted when it fills.
//!
//! # Why the image atlas
//!
//! The glyph atlas's pages are one coverage channel, multiplied into a text
//! colour; a colour glyph has colours of its own, and the UI pass already
//! draws RGBA pictures from the image atlas through its own binding. So a
//! colour glyph is an image quad:
//! [`DrawList::resolve_color_glyphs`](crate::draw_list::DrawList::resolve_color_glyphs)
//! swaps each one in a glyph run for one before the list goes to the
//! renderer, which needs nothing new.
//!
//! # Trust
//!
//! A font is trusted less than code. A colour glyph whose picture cannot be
//! made — a PNG that does not decode, a picture larger than
//! [`MAX_COLOR_GLYPH_SIDE`] — is not a colour glyph here, and its glyph run
//! draws its outline instead, which for a bitmap-only font is nothing.

#[cfg(feature = "parsed-font")]
mod bitmap;
mod cache;

pub use cache::{ColorGlyph, ColorGlyphCache, ColorGlyphKey, ColorGlyphStats};
#[cfg(feature = "parsed-font")]
use skrifa::bitmap::{BitmapFormat, BitmapStrikes};
#[cfg(feature = "parsed-font")]
use skrifa::{FontRef, MetadataProvider};

use super::{Font, GlyphId};

/// The widest and tallest colour glyph drawn, in pixels: half the image
/// atlas's page, so one glyph never fills it.
pub const MAX_COLOR_GLYPH_SIDE: u32 = crate::image::PAGE_SIZE / 2;

/// A colour glyph's picture at one size, and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorBitmap {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its left edge's offset from the pen, in pixels.
    pub left: i32,
    /// Its top edge's offset from the baseline, in pixels, y down: negative
    /// above the baseline.
    pub top: i32,
    /// Straight-alpha sRGB RGBA8, rows top to bottom.
    pub pixels: Vec<u8>,
}

/// Whether `glyph` of `font` has a `COLR` layer list or paint graph, or a
/// bitmap in a `CBDT` or `sbix` strike. A monochrome `EBDT` strike is not
/// colour.
#[cfg(feature = "parsed-font")]
pub(crate) fn has_color_glyph(font: &FontRef<'_>, glyph: skrifa::GlyphId) -> bool {
    font.color_glyphs().get(glyph).is_some()
        || [BitmapFormat::Cbdt, BitmapFormat::Sbix]
            .into_iter()
            .filter_map(|format| BitmapStrikes::with_format(font, format))
            .any(|strikes| strikes.iter().any(|strike| strike.get(glyph).is_some()))
}

/// Without the `parsed-font` feature there is no font to draw from.
#[cfg(not(feature = "parsed-font"))]
pub(crate) fn rasterize(font: &Font, _glyph: GlyphId, _size: f32) -> Option<ColorBitmap> {
    match font.parsed.never {}
}

/// `glyph` of `font` drawn in colour at `size` pixels per em, or `None` when
/// it has no colour representation that draws — see the module docs.
#[cfg(feature = "parsed-font")]
pub(crate) fn rasterize(font: &Font, glyph: GlyphId, size: f32) -> Option<ColorBitmap> {
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let font_ref = FontRef::new(font.data()).ok()?;
    let glyph = skrifa::GlyphId::new(glyph.0);
    bitmap::rasterize(&font_ref, glyph, size, font.metrics().units_per_em)
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use crcbl_image::resize::resize;
    use crcbl_image::{Limits, Rgba8Image, png};

    use super::*;
    use crate::font::fixtures;

    /// `c` of `font` in colour at `size`.
    fn drawn(font: &Font, c: char, size: f32) -> Option<ColorBitmap> {
        rasterize(font, font.glyph_id(c), size)
    }

    /// The PNG the `ppem` CBDT strike holds for `c`, decoded.
    fn strike_png(font: &Font, c: char, ppem: f32) -> Rgba8Image {
        let font_ref = FontRef::new(font.data()).expect("reads");
        let strikes = BitmapStrikes::with_format(&font_ref, BitmapFormat::Cbdt).expect("CBDT");
        let strike = strikes
            .iter()
            .find(|strike| strike.ppem() == ppem)
            .expect("the strike");
        let bitmap = strike
            .get(skrifa::GlyphId::new(font.glyph_id(c).0))
            .expect("in the strike");
        let skrifa::bitmap::BitmapData::Png(bytes) = bitmap.data else {
            panic!("a PNG");
        };
        png::decode(bytes, &Limits::DEFAULT).expect("decodes")
    }

    fn pixel(bitmap: &ColorBitmap, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * bitmap.width + x) * 4) as usize;
        bitmap.pixels[at..at + 4].try_into().expect("four bytes")
    }

    /// **A CBDT glyph at its strike's size is the strike's PNG, texel for
    /// texel**, its top-left its bearing above the baseline; at a smaller
    /// strike's size it is that strike's picture.
    #[test]
    fn a_cbdt_glyph_at_a_strikes_size_is_its_png_placed_by_its_bearing() {
        let cbdt = fixtures::cbdt();
        let at16 = drawn(&cbdt, '\u{1F600}', 16.0).expect("in colour");
        assert_eq!(
            (at16.width, at16.height, at16.left, at16.top),
            (16, 16, 0, -12)
        );
        for y in 0..16 {
            for x in 0..16 {
                let want = if x < 8 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                assert_eq!(pixel(&at16, x, y), want, "({x}, {y})");
            }
        }
        let at8 = drawn(&cbdt, '\u{1F600}', 8.0).expect("in colour");
        assert_eq!((at8.width, at8.height, at8.left, at8.top), (8, 8, 0, -6));
        assert!(at8.pixels.chunks(4).all(|p| p == [0, 255, 0, 255]));
    }

    /// **Between strikes the nearest is scaled**, by `crcbl_image::resize`,
    /// and so are its metrics: twice the 16 strike is the 16 PNG resized to
    /// 32, its top twice as far above the baseline.
    #[test]
    fn a_cbdt_glyph_between_strikes_is_the_nearest_resized() {
        let cbdt = fixtures::cbdt();
        let at32 = drawn(&cbdt, '\u{1F600}', 32.0).expect("in colour");
        let want = resize(
            &strike_png(&cbdt, '\u{1F600}', 16.0),
            32,
            32,
            &Limits::DEFAULT,
        )
        .expect("resizes");
        assert_eq!(
            (at32.width, at32.height, at32.left, at32.top),
            (32, 32, 0, -24)
        );
        assert_eq!(at32.pixels, want.pixels());
        // Shrunk from 16 to 12: three quarters, bearing and all.
        let at12 = drawn(&cbdt, '\u{1F600}', 12.0).expect("in colour");
        let want = resize(
            &strike_png(&cbdt, '\u{1F600}', 16.0),
            12,
            12,
            &Limits::DEFAULT,
        )
        .expect("resizes");
        assert_eq!((at12.width, at12.top), (12, -9));
        assert_eq!(at12.pixels, want.pixels());
    }

    /// **An sbix glyph's bottom-left is its origin offset from the pen**:
    /// the fixture's is a quarter of the strike below the baseline, so its
    /// top is three quarters above.
    #[test]
    fn an_sbix_glyph_sits_on_its_origin_offset() {
        let sbix = fixtures::sbix();
        let at16 = drawn(&sbix, '\u{1F600}', 16.0).expect("in colour");
        assert_eq!(
            (at16.width, at16.height, at16.left, at16.top),
            (16, 16, 0, -12)
        );
        assert!(at16.pixels.chunks(4).all(|p| p == [255, 255, 0, 255]));
        let at8 = drawn(&sbix, '\u{1F600}', 8.0).expect("in colour");
        assert_eq!((at8.width, at8.top), (8, -6));
        assert!(at8.pixels.chunks(4).all(|p| p == [0, 255, 255, 255]));
    }

    /// **A strike PNG that does not decode is no colour glyph**, though the
    /// font says it has one; and neither is a glyph at no size at all.
    #[test]
    fn a_corrupt_png_or_a_size_of_nothing_draws_no_colour_glyph() {
        let cbdt = fixtures::cbdt();
        let broken = cbdt.glyph_id('\u{1F4A5}');
        assert!(cbdt.has_color_glyph(broken));
        assert_eq!(rasterize(&cbdt, broken, 16.0), None);
        for size in [0.0, -16.0, f32::NAN, f32::INFINITY] {
            assert_eq!(drawn(&cbdt, '\u{1F600}', size), None, "{size}");
        }
        assert_eq!(drawn(&fixtures::text(), '\u{2764}', 16.0), None);
    }
}
