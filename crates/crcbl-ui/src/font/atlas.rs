//! The glyph atlas: rasterised glyphs on fixed single-channel pages.
//!
//! ```text
//!  (font, glyph, size, subpixel bin) ──miss──▶ hinted outline ──▶ Rasterizer
//!             │                                                     │ mask
//!             │ hit: touch                                          ▼
//!             ▼                                     shelf slot on a page (+ gutter)
//!        AtlasGlyph ◀───────────────────────────────────────────────┘
//!                                                     │ dirty rectangle
//!                                                     ▼
//!                                   crcbl-render's UI pass copies it up
//! ```
//!
//! # Keys and subpixel bins
//!
//! A glyph is keyed by its font, its glyph id, its pixel size and one of
//! [`SUBPIXEL_BINS`] horizontal offsets. A pen position's fraction of a pixel is
//! rounded to the nearest bin and the glyph is rasterised that far right of the
//! pixel grid, so text laid out at fractional advances keeps its spacing
//! without a mask per position. GPUI uses the same four.
//!
//! # Pages, shelves and slots
//!
//! A page is [`GlyphAtlas::page_size`] texels square; pages open on demand up
//! to [`GlyphAtlas::max_pages`]. Each page is cut into shelves, each shelf as
//! tall as its height class — a glyph's height with its gutter, rounded up to a
//! multiple of [`SHELF_QUANTUM`] — and each shelf into slots left to right. A
//! glyph takes the first free slot wide enough on a shelf of its class (or on
//! any shelf emptied by eviction and tall enough), else the room at a shelf's
//! end, else a new shelf below the last. A slot is split when a narrower glyph
//! takes it, and freed slots merge with free neighbours.
//!
//! Every glyph has [`GLYPH_GUTTER`] texel of zero coverage round it, written
//! with it, so nothing a previous occupant left beside it can bleed into its
//! quad.
//!
//! # LRU eviction per page, and the budget
//!
//! When no page has room and no page can open, the atlas evicts: among the
//! glyphs **not used this frame**, it takes the page whose least recently used
//! glyph is oldest and frees that page's glyphs oldest first until the new one
//! fits, then the next page. A glyph used this frame is never evicted, so every
//! [`AtlasGlyph`] handed out stays valid until the next
//! [`GlyphAtlas::begin_frame`].
//!
//! At most [`GlyphAtlas::raster_budget`] glyphs are rasterised a frame — new
//! ones and ones evicted earlier alike. A miss past the budget returns `None`
//! and is counted as deferred; the caller draws without it, and asks again
//! next frame.
//!
//! # Hinting
//!
//! Outlines come from `skrifa` hinted, by the font's own TrueType instructions
//! where it has them (the committed font does) and by `skrifa`'s automatic
//! hinter where it has none, in its light smooth mode **with linear metrics
//! preserved**: the hinter moves points vertically only. That snaps baselines,
//! x-heights and horizontal stems to whole pixels — what keeps small UI text
//! crisp — while leaving every advance and every horizontal position linear, so
//! subpixel positioning and kerning mean what layout computed.

use std::collections::HashMap;

use glam::Vec2;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{
    DrawSettings, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::{FontRef, MetadataProvider};

use super::raster::Rasterizer;
use super::{Font, FontId, GlyphId};
use crate::image::TexelRect;

/// The page size the UI pass uses: 1024 texels square, a mebibyte of R8 each.
pub const GLYPH_PAGE_SIZE: u32 = 1024;

/// How many pages the UI pass allows.
pub const GLYPH_MAX_PAGES: usize = 2;

/// How many glyphs the UI pass rasterises a frame at most.
pub const GLYPH_RASTER_BUDGET: u32 = 256;

/// How many horizontal offsets a glyph is rasterised at: `0`, `1/4`, `1/2` and
/// `3/4` of a pixel.
pub const SUBPIXEL_BINS: u8 = 4;

/// Texels of zero coverage round every glyph.
pub const GLYPH_GUTTER: u32 = 1;

/// Shelf heights are multiples of this many texels.
pub const SHELF_QUANTUM: u32 = 4;

/// How many sizes' hinting instances the atlas keeps before dropping them all.
const HINTING_INSTANCES: usize = 16;

/// What a glyph is cached under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// Its font.
    pub font: FontId,
    /// Its glyph.
    pub glyph: GlyphId,
    /// Its size in pixels per em, by bits.
    pub size_bits: u32,
    /// Its subpixel bin, `0..SUBPIXEL_BINS`.
    pub bin: u8,
}

/// Where a rasterised glyph is, and where it sits against its pen position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasGlyph {
    /// The page, which is the layer of the UI pass's page array.
    pub page: u32,
    /// Its mask's left column on the page, inside the gutter.
    pub x: u32,
    /// Its mask's top row on the page.
    pub y: u32,
    /// Mask width in texels; zero for a glyph with no ink, like a space.
    pub width: u32,
    /// Mask height in texels.
    pub height: u32,
    /// The mask's left edge relative to the pen's whole-pixel x.
    pub left: i32,
    /// The mask's top edge relative to the baseline, negative above it.
    pub top: i32,
}

impl AtlasGlyph {
    /// A glyph with no ink: nothing to draw, and no room taken.
    pub const EMPTY: Self = Self {
        page: 0,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        left: 0,
        top: 0,
    };

    /// Whether it has nothing to draw.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// What the atlas did in the frame since the last
/// [`GlyphAtlas::begin_frame`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphAtlasStats {
    /// Glyphs rasterised.
    pub rasterized: u32,
    /// Misses past the budget, left for a later frame.
    pub deferred: u32,
    /// Glyphs evicted to make room.
    pub evicted: u32,
    /// Rasterised glyphs with no room even after eviction, or larger than a
    /// page.
    pub unplaced: u32,
}

#[derive(Clone, Copy, Debug)]
enum Cached {
    Placed(AtlasGlyph),
    Empty,
    /// Larger than a page: never placeable, so never rasterised again.
    TooLarge,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    cached: Cached,
    last_used: u64,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    x: u32,
    width: u32,
    key: Option<GlyphKey>,
}

#[derive(Clone, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    /// Left to right, covering `0..cursor` with no gaps.
    slots: Vec<Slot>,
    cursor: u32,
}

impl Shelf {
    fn is_empty(&self) -> bool {
        self.slots.iter().all(|slot| slot.key.is_none())
    }
}

#[derive(Clone, Debug)]
struct Page {
    pixels: Vec<u8>,
    shelves: Vec<Shelf>,
    dirty: Option<TexelRect>,
}

impl Page {
    fn new(size: u32) -> Self {
        Self {
            pixels: vec![0; size as usize * size as usize],
            shelves: Vec::new(),
            dirty: None,
        }
    }

    /// Claims a `width` by `height` slot for `key`, returning its top-left.
    fn allocate(
        &mut self,
        size: u32,
        key: GlyphKey,
        width: u32,
        height: u32,
    ) -> Option<(u32, u32)> {
        let class = height.next_multiple_of(SHELF_QUANTUM);
        for shelf in &mut self.shelves {
            if shelf.height < height || !(shelf.height == class || shelf.is_empty()) {
                continue;
            }
            if let Some(at) = shelf
                .slots
                .iter()
                .position(|slot| slot.key.is_none() && slot.width >= width)
            {
                let slot = shelf.slots[at];
                if slot.width > width {
                    shelf.slots.insert(
                        at + 1,
                        Slot {
                            x: slot.x + width,
                            width: slot.width - width,
                            key: None,
                        },
                    );
                }
                shelf.slots[at] = Slot {
                    x: slot.x,
                    width,
                    key: Some(key),
                };
                return Some((slot.x, shelf.y));
            }
            if size - shelf.cursor >= width {
                let x = shelf.cursor;
                shelf.slots.push(Slot {
                    x,
                    width,
                    key: Some(key),
                });
                shelf.cursor += width;
                return Some((x, shelf.y));
            }
        }
        let top = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.y + shelf.height);
        if size - top < height || size < width {
            return None;
        }
        self.shelves.push(Shelf {
            y: top,
            height: class.min(size - top),
            slots: vec![Slot {
                x: 0,
                width,
                key: Some(key),
            }],
            cursor: width,
        });
        Some((0, top))
    }

    /// Frees the slot whose top-left is `(x, y)`.
    fn free(&mut self, x: u32, y: u32) {
        let Some(shelf) = self.shelves.iter_mut().find(|shelf| shelf.y == y) else {
            return;
        };
        let Some(mut at) = shelf.slots.iter().position(|slot| slot.x == x) else {
            return;
        };
        shelf.slots[at].key = None;
        if at + 1 < shelf.slots.len() && shelf.slots[at + 1].key.is_none() {
            shelf.slots[at].width += shelf.slots[at + 1].width;
            shelf.slots.remove(at + 1);
        }
        if at > 0 && shelf.slots[at - 1].key.is_none() {
            shelf.slots[at - 1].width += shelf.slots[at].width;
            shelf.slots.remove(at);
            at -= 1;
        }
        if at + 1 == shelf.slots.len() {
            shelf.cursor = shelf.slots[at].x;
            shelf.slots.pop();
        }
        while self
            .shelves
            .last()
            .is_some_and(|shelf| shelf.slots.is_empty())
        {
            self.shelves.pop();
        }
    }
}

/// One outline element in mask pixels, y down.
#[derive(Clone, Copy, Debug)]
enum Segment {
    Line(Vec2, Vec2),
    Quadratic(Vec2, Vec2, Vec2),
    Cubic(Vec2, Vec2, Vec2, Vec2),
}

/// Collects a drawn outline as [`Segment`]s, flipped to y down and shifted
/// right by a subpixel offset, closing every contour.
struct Collect<'a> {
    segments: &'a mut Vec<Segment>,
    offset: f32,
    start: Vec2,
    current: Vec2,
}

impl Collect<'_> {
    fn point(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x + self.offset, -y)
    }

    fn close_contour(&mut self) {
        if self.current != self.start {
            self.segments.push(Segment::Line(self.current, self.start));
        }
        self.current = self.start;
    }
}

impl OutlinePen for Collect<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_contour();
        self.start = self.point(x, y);
        self.current = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments.push(Segment::Line(self.current, to));
        self.current = to;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments
            .push(Segment::Quadratic(self.current, self.point(cx0, cy0), to));
        self.current = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        self.segments.push(Segment::Cubic(
            self.current,
            self.point(cx0, cy0),
            self.point(cx1, cy1),
            to,
        ));
        self.current = to;
    }

    fn close(&mut self) {
        self.close_contour();
    }
}

/// The hinting every glyph is drawn with; see the module docs.
fn hinting_options() -> HintingOptions {
    HintingOptions::from(Target::Smooth {
        mode: SmoothMode::Light,
        symmetric_rendering: true,
        preserve_linear_metrics: true,
    })
}

/// `glyph`'s outline at `size`, shifted `offset` pixels right, into
/// `segments`: hinted by `hinting` when given. Returns whether it drew.
fn draw_outline(
    font: &Font,
    glyph: GlyphId,
    size: f32,
    offset: f32,
    hinting: Option<&HintingInstance>,
    segments: &mut Vec<Segment>,
) -> bool {
    segments.clear();
    let Ok(font_ref) = FontRef::new(font.data()) else {
        return false;
    };
    let outlines = font_ref.outline_glyphs();
    let Some(outline) = outlines.get(skrifa::GlyphId::new(glyph.0)) else {
        return false;
    };
    let mut pen = Collect {
        segments,
        offset,
        start: Vec2::ZERO,
        current: Vec2::ZERO,
    };
    let drawn = match hinting {
        Some(instance) => outline.draw(DrawSettings::hinted(instance, false), &mut pen),
        None => outline.draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::default()),
            &mut pen,
        ),
    };
    pen.close_contour();
    drawn.is_ok()
}

/// The glyph atlas. See the module docs.
pub struct GlyphAtlas {
    page_size: u32,
    max_pages: usize,
    raster_budget: u32,
    pages: Vec<Page>,
    entries: HashMap<GlyphKey, Entry>,
    frame: u64,
    stats: GlyphAtlasStats,
    /// Per font and size; `None` where the font could not be given one.
    hinting: HashMap<(FontId, u32), Option<HintingInstance>>,
    rasterizer: Rasterizer,
    segments: Vec<Segment>,
    mask: Vec<u8>,
}

impl core::fmt::Debug for GlyphAtlas {
    /// Everything but the pages' texels and the scratch buffers.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GlyphAtlas")
            .field("page_size", &self.page_size)
            .field("pages", &self.pages.len())
            .field("max_pages", &self.max_pages)
            .field("glyphs", &self.entries.len())
            .field("frame", &self.frame)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new(GLYPH_PAGE_SIZE, GLYPH_MAX_PAGES, GLYPH_RASTER_BUDGET)
    }
}

impl GlyphAtlas {
    /// An empty atlas of pages `page_size` texels square, at most `max_pages`
    /// of them, rasterising at most `raster_budget` glyphs a frame. Each is at
    /// least one.
    #[must_use]
    pub fn new(page_size: u32, max_pages: usize, raster_budget: u32) -> Self {
        Self {
            page_size: page_size.max(1),
            max_pages: max_pages.max(1),
            raster_budget: raster_budget.max(1),
            pages: Vec::new(),
            entries: HashMap::new(),
            frame: 0,
            stats: GlyphAtlasStats::default(),
            hinting: HashMap::new(),
            rasterizer: Rasterizer::default(),
            segments: Vec::new(),
            mask: Vec::new(),
        }
    }

    /// A page's width and height in texels.
    #[must_use]
    pub const fn page_size(&self) -> u32 {
        self.page_size
    }

    /// The most pages it opens.
    #[must_use]
    pub const fn max_pages(&self) -> usize {
        self.max_pages
    }

    /// The most glyphs it rasterises a frame.
    #[must_use]
    pub const fn raster_budget(&self) -> u32 {
        self.raster_budget
    }

    /// How many pages are open.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// How many glyphs are cached, empty ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What it did this frame.
    #[must_use]
    pub const fn stats(&self) -> GlyphAtlasStats {
        self.stats
    }

    /// Starts a frame: glyphs used before now may be evicted, and the budget
    /// is full again.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.stats = GlyphAtlasStats::default();
    }

    /// The key `glyph` of `font` at `size` pixels per em and subpixel `bin`
    /// is cached under.
    #[must_use]
    pub fn key(font: &Font, glyph: GlyphId, size: f32, bin: u8) -> GlyphKey {
        GlyphKey {
            font: font.id(),
            glyph,
            size_bits: size.to_bits(),
            bin: bin % SUBPIXEL_BINS,
        }
    }

    /// Where `glyph` of `font` at `size` and subpixel `bin` is, rasterising
    /// it on a miss, and marks it used this frame.
    ///
    /// [`AtlasGlyph::EMPTY`]-sized for a glyph with no ink. `None` when the
    /// miss is past this frame's budget, or when there is no room even after
    /// eviction — see [`GlyphAtlas::stats`] for which.
    pub fn glyph(&mut self, font: &Font, glyph: GlyphId, size: f32, bin: u8) -> Option<AtlasGlyph> {
        let key = Self::key(font, glyph, size, bin);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = self.frame;
            return match entry.cached {
                Cached::Placed(placed) => Some(placed),
                Cached::Empty => Some(AtlasGlyph::EMPTY),
                Cached::TooLarge => None,
            };
        }
        if !(size.is_finite() && size > 0.0) {
            return Some(AtlasGlyph::EMPTY);
        }
        if self.stats.rasterized >= self.raster_budget {
            self.stats.deferred += 1;
            return None;
        }
        self.stats.rasterized += 1;

        let Some((width, height, left, top)) = self.rasterize(font, key) else {
            self.insert(key, Cached::Empty);
            return Some(AtlasGlyph::EMPTY);
        };
        let slot_width = width + 2 * GLYPH_GUTTER;
        let slot_height = height + 2 * GLYPH_GUTTER;
        if slot_width > self.page_size || slot_height > self.page_size {
            self.stats.unplaced += 1;
            self.insert(key, Cached::TooLarge);
            return None;
        }
        let Some((page, x, y)) = self.place(key, slot_width, slot_height) else {
            self.stats.unplaced += 1;
            return None;
        };
        let placed = AtlasGlyph {
            page: page as u32,
            x: x + GLYPH_GUTTER,
            y: y + GLYPH_GUTTER,
            width,
            height,
            left,
            top,
        };
        self.blit(&placed, slot_width, slot_height);
        self.insert(key, Cached::Placed(placed));
        Some(placed)
    }

    fn insert(&mut self, key: GlyphKey, cached: Cached) {
        self.entries.insert(
            key,
            Entry {
                cached,
                last_used: self.frame,
            },
        );
    }

    /// Draws `key`'s outline into the mask scratch, returning its extent and
    /// its offset from the pen, or `None` for a glyph with no ink.
    fn rasterize(&mut self, font: &Font, key: GlyphKey) -> Option<(u32, u32, i32, i32)> {
        let size = f32::from_bits(key.size_bits);
        let hinting_key = (key.font, key.size_bits);
        if !self.hinting.contains_key(&hinting_key) {
            if self.hinting.len() >= HINTING_INSTANCES {
                self.hinting.clear();
            }
            let instance = FontRef::new(font.data()).ok().and_then(|font_ref| {
                HintingInstance::new(
                    &font_ref.outline_glyphs(),
                    Size::new(size),
                    LocationRef::default(),
                    hinting_options(),
                )
                .ok()
            });
            self.hinting.insert(hinting_key, instance);
        }
        let offset = f32::from(key.bin) / f32::from(SUBPIXEL_BINS);
        let hinting = self.hinting.get(&hinting_key).and_then(Option::as_ref);
        let drawn = draw_outline(font, key.glyph, size, offset, hinting, &mut self.segments)
            || draw_outline(font, key.glyph, size, offset, None, &mut self.segments);
        if !drawn || self.segments.is_empty() {
            return None;
        }

        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for segment in &self.segments {
            let points: &[Vec2] = match segment {
                Segment::Line(a, b) => &[*a, *b],
                Segment::Quadratic(a, b, c) => &[*a, *b, *c],
                Segment::Cubic(a, b, c, d) => &[*a, *b, *c, *d],
            };
            for point in points {
                min = min.min(*point);
                max = max.max(*point);
            }
        }
        if !(min.is_finite() && max.is_finite()) {
            return None;
        }
        let origin = min.floor();
        let extent = max.ceil() - origin;
        let (width, height) = (extent.x as u32, extent.y as u32);
        if width == 0 || height == 0 {
            return None;
        }

        self.rasterizer.reset(width, height);
        for segment in &self.segments {
            match *segment {
                Segment::Line(a, b) => self.rasterizer.line(a - origin, b - origin),
                Segment::Quadratic(a, b, c) => {
                    self.rasterizer
                        .quadratic(a - origin, b - origin, c - origin);
                }
                Segment::Cubic(a, b, c, d) => {
                    self.rasterizer
                        .cubic(a - origin, b - origin, c - origin, d - origin);
                }
            }
        }
        self.mask.clear();
        self.rasterizer.write_mask(&mut self.mask);
        Some((width, height, origin.x as i32, origin.y as i32))
    }

    /// Finds a slot on an open page, a new page, or — evicting — an old one.
    fn place(&mut self, key: GlyphKey, width: u32, height: u32) -> Option<(usize, u32, u32)> {
        let size = self.page_size;
        for (index, page) in self.pages.iter_mut().enumerate() {
            if let Some((x, y)) = page.allocate(size, key, width, height) {
                return Some((index, x, y));
            }
        }
        if self.pages.len() < self.max_pages {
            let mut page = Page::new(size);
            let placed = page.allocate(size, key, width, height);
            self.pages.push(page);
            return placed.map(|(x, y)| (self.pages.len() - 1, x, y));
        }
        self.evict_for(key, width, height)
    }

    /// Evicts least recently used glyphs a page at a time until `key` fits.
    /// See the module docs.
    fn evict_for(&mut self, key: GlyphKey, width: u32, height: u32) -> Option<(usize, u32, u32)> {
        let mut by_page: Vec<Vec<(u64, GlyphKey, AtlasGlyph)>> = vec![Vec::new(); self.pages.len()];
        for (candidate, entry) in &self.entries {
            if let Cached::Placed(placed) = entry.cached
                && entry.last_used != self.frame
            {
                by_page[placed.page as usize].push((entry.last_used, *candidate, placed));
            }
        }
        for glyphs in &mut by_page {
            glyphs.sort_unstable_by_key(|(last_used, ..)| *last_used);
        }
        let mut order: Vec<usize> = (0..by_page.len())
            .filter(|&page| !by_page[page].is_empty())
            .collect();
        order.sort_unstable_by_key(|&page| by_page[page][0].0);

        let size = self.page_size;
        for page in order {
            for &(_, victim, placed) in &by_page[page] {
                self.entries.remove(&victim);
                self.pages[page].free(placed.x - GLYPH_GUTTER, placed.y - GLYPH_GUTTER);
                self.stats.evicted += 1;
                if let Some((x, y)) = self.pages[page].allocate(size, key, width, height) {
                    return Some((page, x, y));
                }
            }
        }
        None
    }

    /// Writes the mask scratch into `placed`'s slot, zeroing its gutter, and
    /// marks the slot dirty.
    fn blit(&mut self, placed: &AtlasGlyph, slot_width: u32, slot_height: u32) {
        let size = self.page_size as usize;
        let page = &mut self.pages[placed.page as usize];
        let (slot_x, slot_y) = (
            (placed.x - GLYPH_GUTTER) as usize,
            (placed.y - GLYPH_GUTTER) as usize,
        );
        for row in 0..slot_height as usize {
            let start = (slot_y + row) * size + slot_x;
            page.pixels[start..start + slot_width as usize].fill(0);
        }
        let width = placed.width as usize;
        for (row, source) in self.mask.chunks_exact(width).enumerate() {
            let start = (placed.y as usize + row) * size + placed.x as usize;
            page.pixels[start..start + width].copy_from_slice(source);
        }
        let rect = TexelRect {
            x: slot_x as u32,
            y: slot_y as u32,
            width: slot_width,
            height: slot_height,
        };
        page.dirty = Some(page.dirty.map_or(rect, |dirty| dirty.union(rect)));
    }

    /// Page `page`'s texels, rows top to bottom, one byte of coverage each.
    ///
    /// # Panics
    ///
    /// If `page` is not open.
    #[must_use]
    pub fn page_pixels(&self, page: usize) -> &[u8] {
        &self.pages[page].pixels
    }

    /// The texels of `page` changed since it was last taken, or `None`.
    #[must_use]
    pub fn dirty(&self, page: usize) -> Option<TexelRect> {
        self.pages.get(page).and_then(|page| page.dirty)
    }

    /// Hands back `page`'s changed rectangle and forgets it.
    pub fn take_dirty(&mut self, page: usize) -> Option<TexelRect> {
        self.pages.get_mut(page).and_then(|page| page.dirty.take())
    }

    /// Marks `rect` of `page` changed again, for an upload that was taken and
    /// then not recorded.
    pub fn mark_dirty(&mut self, page: usize, rect: TexelRect) {
        if let Some(page) = self.pages.get_mut(page) {
            page.dirty = Some(page.dirty.map_or(rect, |dirty| dirty.union(rect)));
        }
    }

    /// The bytes of `rect` on `page`, rows top to bottom.
    ///
    /// # Panics
    ///
    /// If `page` is not open or `rect` is not inside it.
    #[must_use]
    pub fn region(&self, page: usize, rect: TexelRect) -> Vec<u8> {
        let size = self.page_size as usize;
        let pixels = &self.pages[page].pixels;
        let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize);
        for row in rect.y as usize..(rect.y + rect.height) as usize {
            let start = row * size + rect.x as usize;
            out.extend_from_slice(&pixels[start..start + rect.width as usize]);
        }
        out
    }

    /// Where `key` is cached, without marking it used; `None` when it is not
    /// placed.
    #[must_use]
    pub fn cached(&self, key: GlyphKey) -> Option<AtlasGlyph> {
        match self.entries.get(&key)?.cached {
            Cached::Placed(placed) => Some(placed),
            Cached::Empty => Some(AtlasGlyph::EMPTY),
            Cached::TooLarge => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::raster::coverage_byte;

    fn sans() -> &'static Font {
        Font::sans()
    }

    fn glyph(c: char) -> GlyphId {
        sans().glyph_id(c)
    }

    fn overlaps(a: &AtlasGlyph, b: &AtlasGlyph) -> bool {
        let g = GLYPH_GUTTER;
        a.page == b.page
            && a.x - g < b.x + b.width + g
            && b.x - g < a.x + a.width + g
            && a.y - g < b.y + b.height + g
            && b.y - g < a.y + a.height + g
    }

    /// **Every glyph is on a page, inside it, overlapping no other glyph or
    /// gutter, and its texels are its own rasterised coverage with a zero
    /// gutter round them.**
    #[test]
    fn glyphs_pack_without_overlap_and_hold_their_own_coverage() {
        let mut atlas = GlyphAtlas::new(256, 4, 10_000);
        atlas.begin_frame();
        let mut placed = Vec::new();
        for size in [11.0, 23.0] {
            for c in ('A'..='Z').chain('a'..='z').chain(['é', 'Ç', '@']) {
                for bin in 0..SUBPIXEL_BINS {
                    let at = atlas.glyph(sans(), glyph(c), size, bin).expect("room");
                    assert!(!at.is_empty(), "{c:?} has ink");
                    placed.push((c, size, bin, at));
                }
            }
        }
        assert!(
            atlas.page_count() > 1,
            "the test is meant to open a second page"
        );
        assert_eq!(atlas.stats().evicted, 0);
        for (i, (_, _, _, a)) in placed.iter().enumerate() {
            assert!(a.x >= GLYPH_GUTTER && a.y >= GLYPH_GUTTER);
            assert!(a.x + a.width + GLYPH_GUTTER <= 256 && a.y + a.height + GLYPH_GUTTER <= 256);
            for (_, _, _, b) in &placed[i + 1..] {
                assert!(!overlaps(a, b), "{a:?} overlaps {b:?}");
            }
        }

        // The texels, against an independent rasterisation of the same outline.
        let (c, size, bin, at) = placed[37];
        let mut segments = Vec::new();
        let instance = HintingInstance::new(
            &FontRef::new(sans().data()).expect("reads").outline_glyphs(),
            Size::new(size),
            LocationRef::default(),
            hinting_options(),
        )
        .expect("hints");
        assert!(draw_outline(
            sans(),
            glyph(c),
            size,
            f32::from(bin) / 4.0,
            Some(&instance),
            &mut segments
        ));
        let origin = Vec2::new(at.left as f32, at.top as f32);
        let mut rasterizer = Rasterizer::new(at.width, at.height);
        for segment in segments {
            match segment {
                Segment::Line(a, b) => rasterizer.line(a - origin, b - origin),
                Segment::Quadratic(a, b, c) => {
                    rasterizer.quadratic(a - origin, b - origin, c - origin)
                }
                Segment::Cubic(a, b, c, d) => {
                    rasterizer.cubic(a - origin, b - origin, c - origin, d - origin);
                }
            }
        }
        let want: Vec<u8> = rasterizer
            .coverage()
            .into_iter()
            .map(coverage_byte)
            .collect();
        let page = atlas.page_pixels(at.page as usize);
        let texel = |x: u32, y: u32| page[(y * 256 + x) as usize];
        for y in 0..at.height {
            for x in 0..at.width {
                assert_eq!(texel(at.x + x, at.y + y), want[(y * at.width + x) as usize]);
            }
        }
        for x in at.x - 1..=at.x + at.width {
            assert_eq!(texel(x, at.y - 1), 0, "top gutter");
            assert_eq!(texel(x, at.y + at.height), 0, "bottom gutter");
        }
        assert!(want.contains(&255), "{c:?} has a solid stem");
    }

    /// **A hit rasterises nothing and returns the same place; a glyph with no
    /// ink takes no room.**
    #[test]
    fn a_hit_is_free_and_a_space_takes_no_room() {
        let mut atlas = GlyphAtlas::new(256, 1, 100);
        atlas.begin_frame();
        let first = atlas.glyph(sans(), glyph('Q'), 20.0, 1).expect("room");
        let second = atlas.glyph(sans(), glyph('Q'), 20.0, 1).expect("cached");
        assert_eq!(first, second);
        assert_eq!(atlas.stats().rasterized, 1);
        // A bin past the range is the same bin, not a new key.
        assert_eq!(atlas.glyph(sans(), glyph('Q'), 20.0, 5), Some(first));
        assert_eq!(atlas.stats().rasterized, 1);

        let space = atlas
            .glyph(sans(), glyph(' '), 20.0, 0)
            .expect("empty, not deferred");
        assert!(space.is_empty());
        assert_eq!(atlas.dirty(0).map(|rect| rect.width), Some(first.width + 2));
    }

    /// **The budget caps rasterisations a frame and defers the rest**, which
    /// arrive on the frames after.
    #[test]
    fn the_budget_defers_misses_to_later_frames() {
        let mut atlas = GlyphAtlas::new(256, 1, 3);
        let letters = ['a', 'b', 'c', 'd', 'e', 'f', 'g'];
        let mut frames = 0;
        loop {
            atlas.begin_frame();
            frames += 1;
            let drawn = letters
                .iter()
                .filter(|&&c| atlas.glyph(sans(), glyph(c), 16.0, 0).is_some())
                .count();
            let stats = atlas.stats();
            assert!(stats.rasterized <= 3, "{stats:?}");
            assert_eq!(drawn + stats.deferred as usize, letters.len(), "{stats:?}");
            if drawn == letters.len() {
                break;
            }
        }
        assert_eq!(frames, 3, "seven glyphs at three a frame take three frames");
    }

    /// **With one tiny page and nothing evictable, a glyph is refused; with
    /// glyphs of different ages, a newcomer evicts the least recently used and
    /// never one used this frame; and the evicted come back within the budget,
    /// frame by frame.**
    ///
    /// The glyphs are one `H` from each of many parses of the same font: every
    /// parse is its own [`FontId`], so each is its own key, and every mask is
    /// the same size — which keeps shelf fragmentation out of what is counted.
    #[test]
    fn eviction_takes_the_least_recently_used_and_respects_the_budget() {
        const PAGE: u32 = 48;
        const SIZE: f32 = 14.0;
        let fonts: Vec<Font> = (0..40)
            .map(|_| Font::parse(crate::font::SANS_TTF).expect("parses"))
            .collect();
        let h = sans().glyph_id('H');
        let draw = |atlas: &mut GlyphAtlas, font: &Font| atlas.glyph(font, h, SIZE, 0);
        let cached = |atlas: &GlyphAtlas, font: &Font| {
            atlas.cached(GlyphAtlas::key(font, h, SIZE, 0)).is_some()
        };

        // Everything drawn in one frame: nothing is evictable, so the page
        // simply fills and the next glyph is refused.
        let mut one_frame = GlyphAtlas::new(PAGE, 1, 1000);
        one_frame.begin_frame();
        let capacity = fonts
            .iter()
            .take_while(|font| draw(&mut one_frame, font).is_some())
            .count();
        assert!(capacity >= 6 && capacity * 2 < fonts.len(), "{capacity}");
        assert_eq!(one_frame.stats().evicted, 0);
        assert_eq!(one_frame.stats().unplaced, 1);

        // One glyph a frame, so each has its own age; then touch the two
        // oldest and draw one more.
        let mut atlas = GlyphAtlas::new(PAGE, 1, 1000);
        for font in &fonts[..capacity] {
            atlas.begin_frame();
            draw(&mut atlas, font).expect("fits");
        }
        atlas.begin_frame();
        for font in &fonts[..2] {
            draw(&mut atlas, font).expect("cached");
        }
        assert_eq!(atlas.stats().rasterized, 0, "a hit rasterised");
        draw(&mut atlas, &fonts[capacity]).expect("room after eviction");
        assert_eq!(
            atlas.stats().evicted,
            1,
            "one slot frees room for one glyph"
        );
        let gone: Vec<usize> = (0..capacity)
            .filter(|&index| !cached(&atlas, &fonts[index]))
            .collect();
        assert_eq!(
            gone,
            [2],
            "the evicted glyph is not the least recently used"
        );

        // Evict a full page with a second set in one frame, then ask for the
        // first set back under a budget of two.
        let mut returning = GlyphAtlas::new(PAGE, 1, 1000);
        returning.begin_frame();
        for font in &fonts[..capacity] {
            draw(&mut returning, font).expect("fits");
        }
        returning.begin_frame();
        for font in &fonts[capacity..2 * capacity] {
            draw(&mut returning, font).expect("room after eviction");
        }
        assert_eq!(returning.stats().evicted as usize, capacity);
        returning.raster_budget = 2;
        let mut frames = 0;
        let mut back = 0;
        while back < capacity {
            returning.begin_frame();
            frames += 1;
            back = fonts[..capacity]
                .iter()
                .filter(|font| draw(&mut returning, font).is_some())
                .count();
            let stats = returning.stats();
            assert_eq!(
                stats.rasterized as usize,
                2.min(capacity - (back - stats.rasterized as usize)),
                "frame {frames}: {stats:?}"
            );
            assert_eq!(stats.unplaced, 0, "frame {frames}: {stats:?}");
        }
        assert_eq!(frames, capacity.div_ceil(2));
    }

    /// **Each subpixel bin moves the ink a quarter pixel right**: the coverage
    /// centroid of a vertical stem, in page-independent pixels, steps by 1/4.
    #[test]
    fn subpixel_bins_shift_the_ink_by_quarter_pixels() {
        let mut atlas = GlyphAtlas::new(256, 1, 100);
        atlas.begin_frame();
        let centroid = |atlas: &mut GlyphAtlas, bin: u8| {
            let at = atlas.glyph(sans(), glyph('l'), 24.0, bin).expect("room");
            let page = atlas.page_pixels(0);
            let (mut sum, mut weight) = (0.0f64, 0.0f64);
            for y in 0..at.height {
                for x in 0..at.width {
                    let c = f64::from(page[((at.y + y) * 256 + at.x + x) as usize]);
                    sum += c * (f64::from(at.left) + f64::from(x) + 0.5);
                    weight += c;
                }
            }
            sum / weight
        };
        let base = centroid(&mut atlas, 0);
        for bin in 1..SUBPIXEL_BINS {
            let shift = centroid(&mut atlas, bin) - base;
            let want = f64::from(bin) / 4.0;
            // Quantising coverage to bytes moves a centroid by far less than this.
            assert!(
                (shift - want).abs() < 0.02,
                "bin {bin} shifted {shift}, not {want}"
            );
        }
    }

    /// Every point of a drawn outline, in order.
    fn points(segments: &[Segment]) -> Vec<Vec2> {
        segments
            .iter()
            .flat_map(|segment| match *segment {
                Segment::Line(a, b) => vec![a, b],
                Segment::Quadratic(a, b, c) => vec![a, b, c],
                Segment::Cubic(a, b, c, d) => vec![a, b, c, d],
            })
            .collect()
    }

    /// **Hinting is on, and vertical only**: the committed font's `H` has its
    /// cap height on a whole pixel when hinted and off it when not, and no
    /// point of any letter moves horizontally at any of several sizes.
    #[test]
    fn hinting_snaps_vertical_metrics_and_moves_nothing_horizontally() {
        let outlines = || FontRef::new(sans().data()).expect("reads").outline_glyphs();
        for size in [11.0, 13.0, 16.0, 24.0] {
            let instance = HintingInstance::new(
                &outlines(),
                Size::new(size),
                LocationRef::default(),
                hinting_options(),
            )
            .expect("hints");
            for c in ('A'..='Z').chain('a'..='z') {
                let (mut hinted, mut plain) = (Vec::new(), Vec::new());
                assert!(draw_outline(
                    sans(),
                    glyph(c),
                    size,
                    0.0,
                    Some(&instance),
                    &mut hinted
                ));
                assert!(draw_outline(sans(), glyph(c), size, 0.0, None, &mut plain));
                let (hinted, plain) = (points(&hinted), points(&plain));
                assert_eq!(hinted.len(), plain.len(), "{c:?} at {size}");
                for (a, b) in hinted.iter().zip(&plain) {
                    assert_eq!(a.x, b.x, "{c:?} at {size}px moved horizontally");
                }
            }
        }

        let size = 13.0;
        let instance = HintingInstance::new(
            &outlines(),
            Size::new(size),
            LocationRef::default(),
            hinting_options(),
        )
        .expect("hints");
        let top = |hinting: Option<&HintingInstance>| {
            let mut segments = Vec::new();
            assert!(draw_outline(
                sans(),
                glyph('H'),
                size,
                0.0,
                hinting,
                &mut segments
            ));
            points(&segments)
                .iter()
                .fold(f32::INFINITY, |top, point| top.min(point.y))
        };
        let (hinted, plain) = (top(Some(&instance)), top(None));
        assert_eq!(hinted, hinted.round(), "the hinted cap height is {hinted}");
        assert_ne!(
            plain,
            plain.round(),
            "the unhinted cap height is already whole"
        );
    }

    /// **The dirty rectangle covers every glyph placed since it was taken**,
    /// and taking it clears it.
    #[test]
    fn the_dirty_rectangle_covers_what_was_placed() {
        let mut atlas = GlyphAtlas::new(128, 1, 100);
        atlas.begin_frame();
        assert_eq!(atlas.dirty(0), None);
        let a = atlas.glyph(sans(), glyph('W'), 18.0, 0).expect("room");
        let b = atlas.glyph(sans(), glyph('g'), 30.0, 2).expect("room");
        let dirty = atlas.take_dirty(0).expect("two glyphs changed the page");
        for at in [a, b] {
            assert!(dirty.x + GLYPH_GUTTER <= at.x && dirty.y + GLYPH_GUTTER <= at.y);
            assert!(at.x + at.width + GLYPH_GUTTER <= dirty.x + dirty.width);
            assert!(at.y + at.height + GLYPH_GUTTER <= dirty.y + dirty.height);
        }
        assert_eq!(atlas.dirty(0), None);
        assert_eq!(
            atlas.region(0, dirty).len(),
            (dirty.width * dirty.height) as usize
        );
        atlas.mark_dirty(0, dirty);
        assert_eq!(atlas.dirty(0), Some(dirty));
    }

    /// **A glyph larger than a page is refused once and never rasterised
    /// again.**
    #[test]
    fn a_glyph_larger_than_a_page_is_refused_once() {
        let mut atlas = GlyphAtlas::new(32, 1, 100);
        atlas.begin_frame();
        assert_eq!(atlas.glyph(sans(), glyph('M'), 200.0, 0), None);
        assert_eq!(atlas.stats().unplaced, 1);
        assert_eq!(atlas.glyph(sans(), glyph('M'), 200.0, 0), None);
        assert_eq!(atlas.stats().rasterized, 1);
        assert_eq!(atlas.page_count(), 0);
    }
}
