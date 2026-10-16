//! The colour-glyph cache: each colour glyph rasterised once per font, glyph
//! and pixel size, and held in the caller's [`ImageAtlas`].
//!
//! # Sharing the image atlas
//!
//! The atlas is the caller's: it holds the pictures a stylesheet's `url()`
//! names, a menu's frames, a launcher's icons. The cache registers its
//! glyphs beside them and **only ever unregisters images it registered
//! itself**, so a picture the caller registered is never touched.
//!
//! # Eviction
//!
//! When the atlas has no room for a new glyph, the cache unregisters its
//! glyphs **not used this frame**, least recently used first, until the new
//! one fits. A glyph used this frame is never evicted, so every
//! [`AtlasImage`] handed out stays valid until the next
//! [`ColorGlyphCache::begin_frame`]. A glyph that fits nowhere even then is
//! not drawn in colour this frame, and is asked for again the next; one
//! larger than the atlas could ever hold evicts nothing.

use std::collections::HashMap;

use super::ColorBitmap;
use crate::font::{Font, FontId, GlyphId};
use crate::image::{AtlasImage, GUTTER, ImageAtlas, PAGE_SIZE};

/// What a colour glyph is cached under: like the glyph atlas's key, without
/// a subpixel bin, since a picture is placed on whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorGlyphKey {
    /// Its font.
    pub font: FontId,
    /// Its glyph.
    pub glyph: GlyphId,
    /// Its size in pixels per em, by bits.
    pub size_bits: u32,
}

/// A colour glyph in the image atlas, and where it sits against its pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorGlyph {
    /// Its picture.
    pub image: AtlasImage,
    /// Its left edge's offset from the pen, in pixels.
    pub left: i32,
    /// Its top edge's offset from the baseline, in pixels, y down.
    pub top: i32,
}

/// What the cache knows of one key.
#[derive(Clone, Copy, Debug)]
enum Cached {
    /// In the atlas.
    Placed(ColorGlyph),
    /// Not a colour glyph at this size: none in the font, or one whose
    /// picture could not be made.
    Outline,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    cached: Cached,
    last_used: u64,
}

/// What the cache did since the last [`ColorGlyphCache::begin_frame`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorGlyphStats {
    /// Glyphs rasterised into the atlas.
    pub rasterized: u32,
    /// Glyphs unregistered to make room.
    pub evicted: u32,
    /// Glyphs that found no room even after eviction.
    pub unplaced: u32,
}

/// The colour-glyph cache. See the module docs.
#[derive(Debug, Default)]
pub struct ColorGlyphCache {
    entries: HashMap<ColorGlyphKey, Entry>,
    frame: u64,
    stats: ColorGlyphStats,
}

impl ColorGlyphCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a frame: glyphs used before now may be evicted.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.stats = ColorGlyphStats::default();
    }

    /// What it did this frame.
    #[must_use]
    pub const fn stats(&self) -> ColorGlyphStats {
        self.stats
    }

    /// How many glyphs it holds in the atlas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry.cached, Cached::Placed(_)))
            .count()
    }

    /// Whether it holds no glyph in the atlas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The key `glyph` of `font` at `size` pixels per em is cached under.
    #[must_use]
    pub fn key(font: &Font, glyph: GlyphId, size: f32) -> ColorGlyphKey {
        ColorGlyphKey {
            font: font.id(),
            glyph,
            size_bits: size.to_bits(),
        }
    }

    /// The glyph held under `key`, if it is in the atlas, without marking it
    /// used.
    #[must_use]
    pub fn cached(&self, key: ColorGlyphKey) -> Option<ColorGlyph> {
        match self.entries.get(&key)?.cached {
            Cached::Placed(glyph) => Some(glyph),
            Cached::Outline => None,
        }
    }

    /// `glyph` of `font` at `size` pixels per em in colour, rasterising it
    /// into `images` on a miss, and marks it used this frame. `None` when it
    /// is not a colour glyph at this size, or when the atlas has no room for
    /// it this frame: its run draws its outline instead.
    pub fn glyph(
        &mut self,
        images: &mut ImageAtlas,
        font: &Font,
        glyph: GlyphId,
        size: f32,
    ) -> Option<ColorGlyph> {
        let key = Self::key(font, glyph, size);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = self.frame;
            return match entry.cached {
                Cached::Placed(placed) => Some(placed),
                Cached::Outline => None,
            };
        }
        let Some(bitmap) = font
            .has_color_glyph(glyph)
            .then(|| super::rasterize(font, glyph, size))
            .flatten()
        else {
            self.insert(key, Cached::Outline);
            return None;
        };
        let placed = self.place(images, &bitmap)?;
        self.stats.rasterized += 1;
        self.insert(key, Cached::Placed(placed));
        Some(placed)
    }

    fn insert(&mut self, key: ColorGlyphKey, cached: Cached) {
        self.entries.insert(
            key,
            Entry {
                cached,
                last_used: self.frame,
            },
        );
    }

    /// Registers `bitmap` in `images`, evicting glyphs not used this frame,
    /// least recently used first, until it fits; see the module docs.
    fn place(&mut self, images: &mut ImageAtlas, bitmap: &ColorBitmap) -> Option<ColorGlyph> {
        let fits_at_all =
            bitmap.width + 2 * GUTTER <= PAGE_SIZE && bitmap.height + 2 * GUTTER <= PAGE_SIZE;
        let mut evictable: Vec<(u64, ColorGlyphKey)> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.last_used < self.frame && matches!(entry.cached, Cached::Placed(_))
            })
            .map(|(key, entry)| (entry.last_used, *key))
            .collect();
        // Oldest last, so `pop` takes the least recently used.
        evictable.sort_unstable_by_key(|&(last_used, _)| core::cmp::Reverse(last_used));
        loop {
            if let Ok(image) = images.register(bitmap.width, bitmap.height, &bitmap.pixels) {
                return Some(ColorGlyph {
                    image,
                    left: bitmap.left,
                    top: bitmap.top,
                });
            }
            let oldest = evictable.pop().filter(|_| fits_at_all);
            let Some((_, key)) = oldest else {
                self.stats.unplaced += 1;
                return None;
            };
            if let Some(Entry {
                cached: Cached::Placed(evicted),
                ..
            }) = self.entries.remove(&key)
            {
                images.unregister(evicted.image.id);
                self.stats.evicted += 1;
            }
        }
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;

    const SIZE: f32 = 16.0;

    /// An atlas holding the caller's pictures with room left for exactly
    /// two 16-pixel glyphs on one shelf, and those pictures.
    fn nearly_full() -> (ImageAtlas, Vec<AtlasImage>) {
        let mut images = ImageAtlas::new();
        let side = PAGE_SIZE - 2 * GUTTER;
        let top_height = side - 20;
        let top = images
            .register(side, top_height, &vec![7; (side * top_height * 4) as usize])
            .expect("fits");
        // Opens the last shelf at a glyph's height, leaving two glyphs' room.
        let glyph = 16 + 2 * GUTTER;
        let rest = PAGE_SIZE - 2 * glyph - 2 * GUTTER;
        let shelf = images
            .register(rest, 16, &vec![9; (rest * 16 * 4) as usize])
            .expect("fits");
        (images, vec![top, shelf])
    }

    /// **When the atlas fills, glyphs not used this frame go, oldest first,
    /// and nothing else**: a glyph used this frame stays, a glyph that finds
    /// no room is not drawn in colour, and the caller's pictures are where
    /// they were with the bytes they had.
    #[test]
    fn eviction_takes_glyphs_unused_this_frame_and_never_the_callers_images() {
        let (mut images, callers) = nearly_full();
        let before: Vec<Vec<u8>> = callers.iter().map(|image| region(&images, image)).collect();
        let (cbdt, sbix) = (fixtures::cbdt(), fixtures::sbix());
        let smile = cbdt.glyph_id('\u{1F600}');
        let heart = cbdt.glyph_id('\u{2764}');
        let yellow = sbix.glyph_id('\u{1F600}');
        let mut cache = ColorGlyphCache::new();

        cache.begin_frame();
        let first = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        let second = cache.glyph(&mut images, &cbdt, heart, SIZE).expect("room");
        assert_eq!(cache.len(), 2);

        cache.begin_frame();
        // The heart is used this frame; the smile is not, and is older.
        assert_eq!(cache.glyph(&mut images, &cbdt, heart, SIZE), Some(second));
        let third = cache.glyph(&mut images, &sbix, yellow, SIZE).expect("room");
        assert_eq!(cache.stats().evicted, 1);
        assert_eq!(cache.cached(ColorGlyphCache::key(&cbdt, smile, SIZE)), None);
        assert_eq!(images.image(first.image.id), None, "the smile was evicted");
        assert_eq!(
            (third.image.x, third.image.y),
            (first.image.x, first.image.y)
        );
        assert_eq!(images.image(second.image.id), Some(second.image));

        // Both glyphs are used this frame, so a third finds no room.
        assert_eq!(cache.glyph(&mut images, &cbdt, smile, SIZE), None);
        assert_eq!(cache.stats().unplaced, 1);
        assert_eq!(cache.len(), 2);

        // Next frame it is drawn again, in the room the older of the two
        // leaves.
        cache.begin_frame();
        assert_eq!(cache.glyph(&mut images, &sbix, yellow, SIZE), Some(third));
        let again = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        assert_eq!(
            (again.image.x, again.image.y),
            (second.image.x, second.image.y)
        );

        for (image, bytes) in callers.iter().zip(&before) {
            assert_eq!(images.image(image.id), Some(*image));
            assert_eq!(&region(&images, image), bytes);
        }
    }

    /// **A glyph is rasterised once per size**, and one with no colour
    /// representation is remembered as such rather than asked again.
    #[test]
    fn a_glyph_is_rasterised_once_per_size_and_a_plain_one_never() {
        let mut images = ImageAtlas::new();
        let cbdt = fixtures::cbdt();
        let smile = cbdt.glyph_id('\u{1F600}');
        let mut cache = ColorGlyphCache::new();
        cache.begin_frame();
        let at16 = cache
            .glyph(&mut images, &cbdt, smile, SIZE)
            .expect("colour");
        assert_eq!(cache.glyph(&mut images, &cbdt, smile, SIZE), Some(at16));
        let at8 = cache.glyph(&mut images, &cbdt, smile, 8.0).expect("colour");
        assert_ne!(at8.image, at16.image);
        assert_eq!(cache.stats().rasterized, 2);
        assert_eq!(images.images().len(), 2);

        let text = fixtures::text();
        assert_eq!(
            cache.glyph(&mut images, &text, text.glyph_id('\u{2764}'), SIZE),
            None
        );
        assert_eq!(cache.len(), 2);
        assert_eq!(images.images().len(), 2);
    }

    fn region(images: &ImageAtlas, image: &AtlasImage) -> Vec<u8> {
        images.region(crate::image::TexelRect {
            x: image.x,
            y: image.y,
            width: image.width,
            height: image.height,
        })
    }
}
