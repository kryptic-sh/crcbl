//! The colour-glyph cache: each colour glyph rasterised once per font, glyph
//! and pixel size, and held in the caller's [`ImageAtlas`].
//!
//! # Sharing the image atlas
//!
//! The atlas is the caller's too: it holds the pictures a stylesheet's
//! `url()` names, a menu's frames, a launcher's icons. The cache registers its
//! glyphs beside them **as evictable images**
//! ([`ImageAtlas::register_evictable`]), so the atlas itself frees glyphs not
//! used this frame, least recently used first, when a registration of either
//! kind finds no room — see [`crate::image`]'s module docs. Emoji never crowd
//! out the caller's pictures, and a picture the caller registered is never
//! evicted for a glyph.
//!
//! A glyph used this frame is never evicted, so every [`AtlasImage`] handed
//! out stays valid until the next [`ColorGlyphCache::begin_frame`]. Before
//! drawing a cached glyph again the cache asks the atlas whether it still
//! holds it ([`ImageAtlas::touch`]) and rasterises it afresh when it does
//! not, so it never draws a rectangle of the page another picture has taken.
//! A glyph that finds no room even after eviction is not drawn in colour this
//! frame, and is asked for again the next.

use std::collections::HashMap;

use crate::font::{Font, FontId, GlyphId};
use crate::image::{AtlasImage, ImageAtlas};

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
    /// In the atlas, unless the atlas has since evicted it.
    Placed(ColorGlyph),
    /// Not a colour glyph at this size: none in the font, or one whose
    /// picture could not be made.
    Outline,
}

/// What the cache did since the last [`ColorGlyphCache::begin_frame`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorGlyphStats {
    /// Glyphs rasterised into the atlas, the ones evicted and drawn again
    /// among them.
    pub rasterized: u32,
    /// Glyphs that found no room even after eviction.
    pub unplaced: u32,
}

/// The colour-glyph cache. See the module docs.
#[derive(Debug, Default)]
pub struct ColorGlyphCache {
    entries: HashMap<ColorGlyphKey, Cached>,
    stats: ColorGlyphStats,
}

impl ColorGlyphCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a frame of `images`, the atlas it holds its glyphs in — which
    /// starts the atlas's own frame, [`ImageAtlas::begin_frame`], so glyphs
    /// used before now may be evicted — and forgets the glyphs the atlas has
    /// evicted since the last.
    pub fn begin_frame(&mut self, images: &mut ImageAtlas) {
        images.begin_frame();
        self.stats = ColorGlyphStats::default();
        self.entries.retain(|_, cached| match cached {
            Cached::Placed(placed) => images.image(placed.image.id) == Some(placed.image),
            Cached::Outline => true,
        });
    }

    /// What it did this frame.
    #[must_use]
    pub const fn stats(&self) -> ColorGlyphStats {
        self.stats
    }

    /// How many glyphs it holds in the atlas, as of its last
    /// [`ColorGlyphCache::begin_frame`] and since.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .values()
            .filter(|cached| matches!(cached, Cached::Placed(_)))
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

    /// The glyph held under `key`, if the cache holds it in `images` and
    /// the atlas has not evicted it, without marking it used.
    #[must_use]
    pub fn cached(&self, images: &ImageAtlas, key: ColorGlyphKey) -> Option<ColorGlyph> {
        match self.entries.get(&key)? {
            Cached::Placed(glyph) => {
                (images.image(glyph.image.id) == Some(glyph.image)).then_some(*glyph)
            }
            Cached::Outline => None,
        }
    }

    /// `glyph` of `font` at `size` pixels per em in colour, rasterising it
    /// into `images` on a miss or when the atlas evicted it, and marks it used
    /// this frame. `None` when it is not a colour glyph at this size, or when
    /// the atlas has no room for it this frame: its run draws its outline
    /// instead.
    pub fn glyph(
        &mut self,
        images: &mut ImageAtlas,
        font: &Font,
        glyph: GlyphId,
        size: f32,
    ) -> Option<ColorGlyph> {
        let key = Self::key(font, glyph, size);
        match self.entries.get(&key) {
            Some(Cached::Outline) => return None,
            Some(Cached::Placed(placed)) => {
                if images.touch(placed.image.id) == Some(placed.image) {
                    return Some(*placed);
                }
                self.entries.remove(&key);
            }
            None => {}
        }
        let Some(bitmap) = font
            .has_color_glyph(glyph)
            .then(|| super::rasterize(font, glyph, size))
            .flatten()
        else {
            self.entries.insert(key, Cached::Outline);
            return None;
        };
        let Ok(image) = images.register_evictable(bitmap.width, bitmap.height, &bitmap.pixels)
        else {
            self.stats.unplaced += 1;
            return None;
        };
        let placed = ColorGlyph {
            image,
            left: bitmap.left,
            top: bitmap.top,
        };
        self.stats.rasterized += 1;
        self.entries.insert(key, Cached::Placed(placed));
        Some(placed)
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;
    use crate::image::{AtlasError, GUTTER, PAGE_SIZE};

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

        cache.begin_frame(&mut images);
        let first = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        let second = cache.glyph(&mut images, &cbdt, heart, SIZE).expect("room");
        assert_eq!(cache.len(), 2);

        cache.begin_frame(&mut images);
        // The heart is used this frame; the smile is not, and is older.
        assert_eq!(cache.glyph(&mut images, &cbdt, heart, SIZE), Some(second));
        let third = cache.glyph(&mut images, &sbix, yellow, SIZE).expect("room");
        assert_eq!(
            cache.cached(&images, ColorGlyphCache::key(&cbdt, smile, SIZE)),
            None
        );
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
        cache.begin_frame(&mut images);
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

    /// **Of the glyphs not used this frame, the least recently used goes
    /// first**: with two to choose from, the older is evicted and the newer
    /// stays.
    #[test]
    fn eviction_takes_the_least_recently_used_first() {
        let (mut images, _) = nearly_full();
        let (cbdt, sbix) = (fixtures::cbdt(), fixtures::sbix());
        let smile = cbdt.glyph_id('\u{1F600}');
        let heart = cbdt.glyph_id('\u{2764}');
        let yellow = sbix.glyph_id('\u{1F600}');
        let mut cache = ColorGlyphCache::new();
        cache.begin_frame(&mut images);
        let older = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        cache.begin_frame(&mut images);
        let newer = cache.glyph(&mut images, &cbdt, heart, SIZE).expect("room");
        cache.begin_frame(&mut images);
        let third = cache.glyph(&mut images, &sbix, yellow, SIZE).expect("room");
        assert_eq!(
            (third.image.x, third.image.y),
            (older.image.x, older.image.y)
        );
        assert_eq!(
            cache.cached(&images, ColorGlyphCache::key(&cbdt, smile, SIZE)),
            None
        );
        assert_eq!(
            cache.cached(&images, ColorGlyphCache::key(&cbdt, heart, SIZE)),
            Some(newer)
        );
    }

    /// **A caller's picture evicts emoji unused this frame, never one used
    /// in it, and an evicted emoji is rasterised afresh**: the page full of
    /// two glyphs refuses a picture while both are drawn this frame; the next
    /// frame, drawing only the heart, the picture takes the smile's place;
    /// the smile, asked for again that frame, is not drawn from its stale
    /// rectangle; and a frame later it is rasterised again in the room the
    /// heart, now unused, gives up.
    #[test]
    fn a_callers_picture_evicts_unused_emoji_and_the_cache_draws_them_again() {
        let (mut images, _) = nearly_full();
        let cbdt = fixtures::cbdt();
        let smile = cbdt.glyph_id('\u{1F600}');
        let heart = cbdt.glyph_id('\u{2764}');
        let icon = vec![3; 16 * 16 * 4];
        let mut cache = ColorGlyphCache::new();

        cache.begin_frame(&mut images);
        let first = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        let kept = cache.glyph(&mut images, &cbdt, heart, SIZE).expect("room");
        assert!(
            matches!(images.register(16, 16, &icon), Err(AtlasError::Full { .. })),
            "both emoji are drawn this frame"
        );

        cache.begin_frame(&mut images);
        assert_eq!(cache.glyph(&mut images, &cbdt, heart, SIZE), Some(kept));
        let picture = images.register(16, 16, &icon).expect("the smile gives way");
        assert_eq!((picture.x, picture.y), (first.image.x, first.image.y));
        assert_eq!(images.image(first.image.id), None);
        assert_eq!(images.image(kept.image.id), Some(kept.image));
        // Asked for in the same frame, the smile is not its stale rectangle,
        // and there is no room to draw it afresh.
        assert_eq!(cache.glyph(&mut images, &cbdt, smile, SIZE), None);
        assert_eq!(cache.stats().unplaced, 1);

        cache.begin_frame(&mut images);
        let again = cache.glyph(&mut images, &cbdt, smile, SIZE).expect("room");
        assert_eq!(cache.stats().rasterized, 1);
        assert_ne!(again.image.id, first.image.id);
        assert_eq!((again.image.x, again.image.y), (kept.image.x, kept.image.y));
        assert_eq!(images.image(picture.id), Some(picture), "never evicted");
    }

    /// **A glyph is rasterised once per size**, and one with no colour
    /// representation is remembered as such rather than asked again.
    #[test]
    fn a_glyph_is_rasterised_once_per_size_and_a_plain_one_never() {
        let mut images = ImageAtlas::new();
        let cbdt = fixtures::cbdt();
        let smile = cbdt.glyph_id('\u{1F600}');
        let mut cache = ColorGlyphCache::new();
        cache.begin_frame(&mut images);
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
