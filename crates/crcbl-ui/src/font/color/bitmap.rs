//! CBDT/CBLC and sbix bitmap strikes: a glyph's PNG at the strike nearest
//! the size asked for, decoded, scaled to that size and placed by the
//! strike's metrics.
//!
//! # Which strike
//!
//! Of the strikes that hold the glyph, the smallest whose pixels per em are
//! at least the size, since shrinking loses less than growing, else the
//! largest — [`crcbl_image::resize::nearest_size`]'s rule. A CBDT strike is
//! asked before an sbix one, for a font with both.
//!
//! # Where it goes
//!
//! A strike's metrics are in its own pixels, so each is scaled by the size
//! over the strike's pixels per em, as the bitmap is, and rounded to whole
//! pixels. A CBDT bitmap's top-left sits its bearing right of the pen and its
//! bearing above the baseline. An sbix bitmap's bottom-left sits at its
//! origin offset from the glyph's `glyf` left side bearing and lowest point,
//! where it has an outline, and from the pen and the baseline where it has
//! none — CoreText's reading, which `skrifa` takes.
//!
//! # Not drawn
//!
//! A PNG that does not decode, or one larger than [`MAX_COLOR_GLYPH_SIDE`]
//! either way, is not a colour glyph here: the caller draws its outline. So
//! is an uncompressed BGRA CBDT bitmap, which this module does not read.

use crcbl_image::resize::{nearest_size, resize};
use crcbl_image::{Limits, png};
use skrifa::FontRef;
use skrifa::bitmap::{BitmapData, BitmapFormat, BitmapGlyph, BitmapStrikes, Origin};

use super::{ColorBitmap, MAX_COLOR_GLYPH_SIDE};

/// The limits a strike's PNG is decoded and scaled under.
const LIMITS: Limits = Limits {
    max_width: MAX_COLOR_GLYPH_SIDE,
    max_height: MAX_COLOR_GLYPH_SIDE,
    max_bytes: MAX_COLOR_GLYPH_SIDE as usize * MAX_COLOR_GLYPH_SIDE as usize * 4,
};

/// `glyph`'s bitmap at `size` pixels per em, from the strike nearest it, or
/// `None` when no CBDT or sbix strike has one that draws. `units_per_em`
/// scales an sbix glyph's outline bearings.
pub(super) fn rasterize(
    font: &FontRef<'_>,
    glyph: skrifa::GlyphId,
    size: f32,
    units_per_em: u16,
) -> Option<ColorBitmap> {
    let bitmap = [BitmapFormat::Cbdt, BitmapFormat::Sbix]
        .into_iter()
        .filter_map(|format| BitmapStrikes::with_format(font, format))
        .find_map(|strikes| nearest_strike(&strikes, glyph, size))?;
    let BitmapData::Png(bytes) = bitmap.data else {
        return None;
    };
    let decoded = png::decode(bytes, &LIMITS).ok()?;
    let scale = size / bitmap.ppem_y;
    let width = (decoded.width() as f32 * scale).round().max(1.0) as u32;
    let height = (decoded.height() as f32 * scale).round().max(1.0) as u32;
    let pixels = resize(&decoded, width, height, &LIMITS).ok()?.into_pixels();
    let (left, top) = match bitmap.placement_origin {
        Origin::TopLeft => (
            bitmap.inner_bearing_x * scale,
            -bitmap.inner_bearing_y * scale,
        ),
        Origin::BottomLeft => {
            let outline = size / f32::from(units_per_em);
            let bottom = bitmap.bearing_y * outline + bitmap.inner_bearing_y * scale;
            (
                bitmap.bearing_x * outline + bitmap.inner_bearing_x * scale,
                -(bottom.round() + height as f32),
            )
        }
    };
    Some(ColorBitmap {
        width,
        height,
        left: left.round() as i32,
        top: top.round() as i32,
        pixels,
    })
}

/// `glyph` in the strike of `strikes` nearest `size` among those that hold
/// it; see the module docs.
fn nearest_strike<'a>(
    strikes: &BitmapStrikes<'a>,
    glyph: skrifa::GlyphId,
    size: f32,
) -> Option<BitmapGlyph<'a>> {
    let mut held: Vec<BitmapGlyph<'a>> = strikes
        .iter()
        .filter_map(|strike| strike.get(glyph))
        .collect();
    let sides: Vec<(u32, u32)> = held
        .iter()
        .map(|bitmap| {
            let ppem = bitmap.ppem_y as u32;
            (ppem, ppem)
        })
        .collect();
    let at = nearest_size(&sides, size.ceil() as u32)?;
    Some(held.swap_remove(at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::fixtures;

    /// The pixels per em of the strike `c` is drawn from at `size`.
    fn strike_for(font: &crate::font::Font, c: char, size: f32) -> Option<f32> {
        let font_ref = FontRef::new(font.data()).expect("reads");
        let glyph = skrifa::GlyphId::new(font.glyph_id(c).0);
        [BitmapFormat::Cbdt, BitmapFormat::Sbix]
            .into_iter()
            .filter_map(|format| BitmapStrikes::with_format(&font_ref, format))
            .find_map(|strikes| nearest_strike(&strikes, glyph, size))
            .map(|bitmap| bitmap.ppem_y)
    }

    /// **The strike is the smallest at least the size, else the largest**,
    /// among the strikes that hold the glyph: U+2764 is only in the 16 strike,
    /// so even at 8 it comes from there.
    #[test]
    fn the_strike_is_the_smallest_at_least_the_size_among_those_holding_the_glyph() {
        let cbdt = fixtures::cbdt();
        let smile = '\u{1F600}';
        for (size, want) in [
            (4.0, 8.0),
            (8.0, 8.0),
            (8.5, 16.0),
            (12.0, 16.0),
            (16.0, 16.0),
            (40.0, 16.0),
        ] {
            assert_eq!(strike_for(&cbdt, smile, size), Some(want), "at {size}");
        }
        assert_eq!(strike_for(&cbdt, '\u{2764}', 8.0), Some(16.0));
        assert_eq!(strike_for(&cbdt, 'a', 16.0), None);
        let sbix = fixtures::sbix();
        assert_eq!(strike_for(&sbix, smile, 6.0), Some(8.0));
        assert_eq!(strike_for(&sbix, smile, 9.0), Some(16.0));
    }
}
