//! The RGBA image atlas: pictures a [`DrawList`](crate::DrawList) can draw.
//!
//! ```text
//!   caller's RGBA8 pixels ──register──▶ ImageAtlas page ──▶ AtlasImage (id, texel rect)
//!                                          │                    │
//!                                          │ dirty rect         └─▶ DrawList::image / nine_slice
//!                                          ▼
//!                                   crcbl-render's UI pass uploads it
//! ```
//!
//! # Beside the glyph atlas, not inside it
//!
//! [`FontAtlas`](crate::FontAtlas) is a single-channel coverage mask: the UI
//! pass multiplies it into a vertex colour's alpha and nothing else. A picture
//! has colour of its own, so it lives on a second page in a second format, and
//! the shader samples each through its own binding. Folding the two together
//! would make every glyph four bytes a texel for no gain.
//!
//! # One fixed page, packed in shelves
//!
//! [`register`](ImageAtlas::register) places each image on the first shelf with
//! room for it, opening a new shelf below the last one when none has. A shelf
//! is as tall as the first image that opened it. That packs a set of similar
//! sizes — which is what a UI's frames and icons are — well, and is the simplest
//! allocator that never moves an image once placed, so a registered
//! [`AtlasImage`] stays valid until it is unregistered. There is no second page
//! yet (`docs/backlog.md`, _What UI rung 1 shipped without_): an image that does
//! not fit is [`AtlasError::Full`], never a panic.
//!
//! # Giving space back
//!
//! [`unregister`](ImageAtlas::unregister) frees an image's span of its shelf for
//! the next image that fits it, so a launcher whose result icons change every
//! keystroke registers the new ones in the space the old ones held instead of
//! filling the page. Neighbouring free spans join, a free span at a shelf's end
//! returns to the shelf, and an emptied last shelf is closed, so its rows can
//! open a shelf of another height. The atlas does not decide what to evict: the
//! caller knows which pictures it still draws, and nothing here does.
//!
//! The page is [`PAGE_SIZE`] texels square, a power of two so every UV this
//! module hands out is exact in `f32`, and inside the 4096 WebGPU's
//! compatibility mode caps a 2D texture at.
//!
//! # A gutter round every image
//!
//! The UI pass samples images through a **linear** sampler, bent into
//! sharp-bilinear by the shader (the sprite pass's `SampleMode::Pixel`). A
//! linear tap at an image's outermost texel reaches half a texel past it, so
//! each image is surrounded by [`GUTTER`] texels copied outward from its own
//! edge. Whatever such a tap picks up is then the edge colour it was already
//! drawing, rather than a neighbour's picture bleeding in.
//!
//! # sRGB, straight alpha
//!
//! The bytes are stored as given and uploaded as `Rgba8UnormSrgb`, the format
//! the sprite pass uses for its sheets: authored colours are sRGB-encoded, and
//! the alpha is straight rather than premultiplied.

use core::fmt;

use glam::Vec2;

use crate::widget::SkinInsets;

/// The atlas page's width and height in texels.
///
/// 1024 rather than the 2048 the UI's atlas rule (`docs/notes/tooling.md`) allows: the pictures
/// the UI draws today are the menu's five 16-texel frames, and a 2048 page is
/// sixteen megabytes of GPU memory held whether anything is on it or not. Both
/// are powers of two, which is what keeps [`AtlasImage::uv`] exact.
pub const PAGE_SIZE: u32 = 1024;

/// Texels of edge colour copied round every image. See the module docs.
pub const GUTTER: u32 = 1;

/// Bytes per texel of the page: RGBA8.
const TEXEL_BYTES: usize = 4;

/// Names one registered image. Stable for the atlas's whole life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

impl ImageId {
    /// The order the image was registered in, from zero. Never reused, so an
    /// id kept past [`ImageAtlas::unregister`] names nothing rather than the
    /// image that took its space.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Names a texture the **renderer** owns rather than this atlas: a picture
/// the GPU drew this frame, such as a second camera's view, that a
/// [`DrawList::texture`](crate::DrawList::texture) rectangle samples.
///
/// # A name, resolved when the frame is recorded
///
/// The list cannot hold the texture itself: this crate knows no GPU, and what a
/// view renders into may be a render-graph transient that does not exist until
/// the frame is built. So the caller picks a number, draws with it, and hands
/// the renderer the same number beside the image it stands for when the frame's
/// passes are added — `crcbl-render`'s `UiRenderer::add_passes_with_textures`.
/// The number means nothing else: two lists may use the same one for different
/// pictures, and one list may name as many as it likes.
///
/// A texture the frame names and the renderer is not handed is drawn
/// transparent rather than with some other picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(u32);

impl TextureId {
    /// The texture the caller numbered `index`.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The number the caller chose.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Where one registered image sits on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasImage {
    /// Which image this is.
    pub id: ImageId,
    /// The image's left column on the page, in texels. Its gutter is outside
    /// this.
    pub x: u32,
    /// The image's top row on the page, in texels.
    pub y: u32,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
}

impl AtlasImage {
    /// The page UV of a point in this image's own texel space.
    ///
    /// `(0, 0)` is the image's top-left corner and `(width, height)` its
    /// bottom-right, so an integer argument lands on a texel **boundary** and
    /// `n + 0.5` on a texel centre. Exact for every whole or half texel,
    /// because the page size is a power of two.
    #[must_use]
    pub fn uv(&self, texel: Vec2) -> Vec2 {
        let page = PAGE_SIZE as f32;
        Vec2::new(
            (self.x as f32 + texel.x) / page,
            (self.y as f32 + texel.y) / page,
        )
    }

    /// The UV of the image's top-left corner.
    #[must_use]
    pub fn uv_min(&self) -> Vec2 {
        self.uv(Vec2::ZERO)
    }

    /// The UV of the image's bottom-right corner.
    #[must_use]
    pub fn uv_max(&self) -> Vec2 {
        self.uv(self.size())
    }

    /// `(width, height)` as floats.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width as f32, self.height as f32)
    }
}

/// A registered image and the insets a nine-slice leaves unstretched.
///
/// What [`DrawList::nine_slice`](crate::DrawList::nine_slice) draws: the
/// corners stay `insets` texels across at whatever scale they are drawn, the
/// edges stretch along one axis and the centre along both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NineSliceImage {
    /// The picture.
    pub image: AtlasImage,
    /// The fixed bands, in **texels** of [`image`](Self::image).
    pub insets: SkinInsets,
}

impl NineSliceImage {
    /// The insets actually drawn: each clamped to the image and to what the
    /// opposite side left, so the cut lines can never run backwards.
    #[must_use]
    pub fn clamped_insets(&self) -> SkinInsets {
        let (width, height) = (self.image.width as f32, self.image.height as f32);
        // `max` before `min`, so a NaN inset comes out as zero rather than as a
        // NaN cut line.
        let fit = |inset: f32, room: f32| inset.max(0.0).min(room);
        let left = fit(self.insets.left, width);
        let top = fit(self.insets.top, height);
        SkinInsets::new(
            left,
            fit(self.insets.right, width - left),
            top,
            fit(self.insets.bottom, height - top),
        )
    }
}

/// The three lengths a nine-slice cuts one axis into: the low fixed band, the
/// stretched band, and the high fixed band.
///
/// `low` and `high` are the fixed bands in the caller's units. **Below
/// `low + high` the two fixed bands shrink in proportion and the stretched band
/// is zero**, so the bands still tile `extent` exactly, nothing lands outside it,
/// and the picture is continuous through the minimum size rather than jumping.
/// The sprite pass's `crcbl_render::NineSliceSource::expand` and
/// [`DrawList::nine_slice`](crate::DrawList::nine_slice) both cut with this, so
/// the two cannot disagree about what a squashed frame looks like.
#[must_use]
pub fn slice_bands(low: f32, high: f32, extent: f32) -> [f32; 3] {
    if extent.is_nan() || extent <= 0.0 {
        return [0.0; 3];
    }
    let fixed = low + high;
    if extent < fixed {
        // `fixed > extent > 0`, so the division is safe.
        let low = low * (extent / fixed);
        return [low, 0.0, extent - low];
    }
    [low, extent - fixed, high]
}

/// The four cut lines [`slice_bands`] implies along one axis, from `origin`.
///
/// The far cut is `origin + extent` rather than the sum of the bands: a float
/// sum that lands a half-ulp short would leave the last quad a sliver narrower
/// than the target it was asked to fill. Every quad of a slice indexes into
/// these, so two neighbours share an edge as the *same* `f32`.
#[must_use]
pub fn slice_cuts(origin: f32, bands: [f32; 3], extent: f32) -> [f32; 4] {
    [
        origin,
        origin + bands[0],
        origin + bands[0] + bands[1],
        origin + extent,
    ]
}

/// A rectangle of the page, in texels: what changed and needs uploading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexelRect {
    /// Left column.
    pub x: u32,
    /// Top row.
    pub y: u32,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
}

impl TexelRect {
    /// The smallest rectangle covering both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

/// Why [`ImageAtlas::register`] refused an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasError {
    /// The image has no texels at all.
    Empty,
    /// `pixels` is not `width * height * 4` bytes.
    PixelCount {
        /// The byte count the extent implies.
        expected: usize,
        /// The byte count that arrived.
        actual: usize,
    },
    /// No shelf has room for the image and its gutter, and there is no room
    /// below the last shelf to open one.
    Full {
        /// The refused image's width in texels.
        width: u32,
        /// The refused image's height in texels.
        height: u32,
    },
}

impl fmt::Display for AtlasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("an image with no texels cannot be registered"),
            Self::PixelCount { expected, actual } => write!(
                f,
                "the image's extent implies {expected} bytes of RGBA8 and {actual} arrived"
            ),
            Self::Full { width, height } => write!(
                f,
                "the {PAGE_SIZE}x{PAGE_SIZE} image atlas has no room for a {width}x{height} \
                 image and its gutter"
            ),
        }
    }
}

impl std::error::Error for AtlasError {}

/// One row of the page that images are placed along, left to right.
#[derive(Debug, Clone)]
struct Shelf {
    /// The shelf's top row, gutter included.
    y: u32,
    /// Its height, gutters included: the first image's, which is what opened
    /// it.
    height: u32,
    /// The first column past every image on it.
    cursor: u32,
    /// Spans left of `cursor` that unregistered images gave back, as `(x,
    /// width)` gutters included: sorted, and never touching each other or
    /// `cursor`, because [`Shelf::free`] joins them.
    free: Vec<(u32, u32)>,
}

impl Shelf {
    /// Claims `width` columns, from a free span if one is wide enough and else
    /// at the cursor, and returns the first.
    fn claim(&mut self, width: u32) -> Option<u32> {
        if let Some(at) = self.free.iter().position(|&(_, span)| span >= width) {
            let (x, span) = self.free[at];
            if span == width {
                self.free.remove(at);
            } else {
                self.free[at] = (x + width, span - width);
            }
            return Some(x);
        }
        (PAGE_SIZE - self.cursor >= width).then(|| {
            let x = self.cursor;
            self.cursor += width;
            x
        })
    }

    /// Gives `width` columns from `x` back, joined to the free spans beside
    /// them, or to the cursor when they end there.
    fn free(&mut self, x: u32, width: u32) {
        let at = self.free.partition_point(|&(start, _)| start < x);
        let (mut x, mut width) = (x, width);
        if at < self.free.len() && x + width == self.free[at].0 {
            width += self.free.remove(at).1;
        }
        if at > 0 && self.free[at - 1].0 + self.free[at - 1].1 == x {
            let (start, span) = self.free.remove(at - 1);
            x = start;
            width += span;
        }
        if x + width == self.cursor {
            self.cursor = x;
        } else {
            let at = self.free.partition_point(|&(start, _)| start < x);
            self.free.insert(at, (x, width));
        }
    }
}

/// The page, the images on it, and what has changed since it was last taken.
///
/// See the [module docs](self).
#[derive(Clone)]
pub struct ImageAtlas {
    pixels: Vec<u8>,
    /// The registered images, in id order: unregistering removes without
    /// reordering, so [`ImageAtlas::image`] can binary-search it.
    images: Vec<AtlasImage>,
    /// The id the next image gets.
    next_id: u32,
    shelves: Vec<Shelf>,
    dirty: Option<TexelRect>,
}

impl fmt::Debug for ImageAtlas {
    /// Everything but the four megabytes of page.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageAtlas")
            .field("images", &self.images)
            .field("shelves", &self.shelves.len())
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

impl Default for ImageAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageAtlas {
    /// An empty page: every texel transparent black, nothing dirty.
    ///
    /// Nothing dirty because a renderer uploads the whole page when it first
    /// takes one, so a fresh atlas owes it no region.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pixels: vec![0; PAGE_SIZE as usize * PAGE_SIZE as usize * TEXEL_BYTES],
            images: Vec::new(),
            next_id: 0,
            shelves: Vec::new(),
            dirty: None,
        }
    }

    /// Places a `width` by `height` image of straight-alpha RGBA8 `pixels`,
    /// rows top to bottom, and returns where it went.
    ///
    /// The page texels it covers, gutter included, join the
    /// [`dirty`](Self::dirty) rectangle.
    ///
    /// # Errors
    ///
    /// [`AtlasError::Empty`] for a zero extent, [`AtlasError::PixelCount`] when
    /// `pixels` is not `width * height * 4` bytes, and [`AtlasError::Full`]
    /// when there is no room for it. A refused image changes nothing.
    pub fn register(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<AtlasImage, AtlasError> {
        if width == 0 || height == 0 {
            return Err(AtlasError::Empty);
        }
        let expected = width as usize * height as usize * TEXEL_BYTES;
        if pixels.len() != expected {
            return Err(AtlasError::PixelCount {
                expected,
                actual: pixels.len(),
            });
        }
        let full = AtlasError::Full { width, height };
        let padded_width = width.checked_add(2 * GUTTER).ok_or(full)?;
        let padded_height = height.checked_add(2 * GUTTER).ok_or(full)?;
        let id = ImageId(self.next_id);
        let next_id = self.next_id.checked_add(1).ok_or(full)?;
        let (shelf_x, shelf_y) = self.place(padded_width, padded_height).ok_or(full)?;
        self.next_id = next_id;

        let image = AtlasImage {
            id,
            x: shelf_x + GUTTER,
            y: shelf_y + GUTTER,
            width,
            height,
        };
        self.blit(&image, pixels);
        self.images.push(image);
        let covered = TexelRect {
            x: shelf_x,
            y: shelf_y,
            width: padded_width,
            height: padded_height,
        };
        self.dirty = Some(self.dirty.map_or(covered, |dirty| dirty.union(covered)));
        Ok(image)
    }

    /// Finds the top-left of a free `width` by `height` block and claims it.
    fn place(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width > PAGE_SIZE || height > PAGE_SIZE {
            return None;
        }
        for shelf in self
            .shelves
            .iter_mut()
            .filter(|shelf| shelf.height >= height)
        {
            if let Some(x) = shelf.claim(width) {
                return Some((x, shelf.y));
            }
        }
        let top = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.y + shelf.height);
        if PAGE_SIZE - top < height {
            return None;
        }
        self.shelves.push(Shelf {
            y: top,
            height,
            cursor: width,
            free: Vec::new(),
        });
        Some((0, top))
    }

    /// Removes the image registered as `id` and frees its space, gutter
    /// included, for later images. Returns where it was, or `None` for an id
    /// this atlas does not hold — another atlas's, or one already
    /// unregistered — which changes nothing.
    ///
    /// The page's texels are left as they were and nothing turns dirty: a
    /// draw that still holds the removed [`AtlasImage`] samples the old picture
    /// until a new image is registered over it, and then the new one.
    pub fn unregister(&mut self, id: ImageId) -> Option<AtlasImage> {
        let at = self
            .images
            .binary_search_by_key(&id.0, |image| image.id.0)
            .ok()?;
        let image = self.images.remove(at);
        let shelf = self
            .shelves
            .iter_mut()
            .find(|shelf| shelf.y == image.y - GUTTER)
            .expect("every image sits on the shelf it was placed on");
        shelf.free(image.x - GUTTER, image.width + 2 * GUTTER);
        while self.shelves.last().is_some_and(|shelf| shelf.cursor == 0) {
            self.shelves.pop();
        }
        Some(image)
    }

    /// Copies `pixels` onto the page at `image`, then extends its edges into
    /// the gutter.
    fn blit(&mut self, image: &AtlasImage, pixels: &[u8]) {
        let page = PAGE_SIZE as usize;
        let (x0, y0) = (image.x as usize, image.y as usize);
        let (width, height) = (image.width as usize, image.height as usize);
        let gutter = GUTTER as usize;
        // Every page row from the top gutter to the bottom one, each filled from
        // the nearest image row, and every column likewise from the nearest
        // image column — which extends the edges and fills the corners with the
        // corner texels in one pass.
        for row in 0..height + 2 * gutter {
            let source_row = row.saturating_sub(gutter).min(height - 1);
            for column in 0..width + 2 * gutter {
                let source_column = column.saturating_sub(gutter).min(width - 1);
                let source = (source_row * width + source_column) * TEXEL_BYTES;
                let target = ((y0 - gutter + row) * page + (x0 - gutter + column)) * TEXEL_BYTES;
                self.pixels[target..target + TEXEL_BYTES]
                    .copy_from_slice(&pixels[source..source + TEXEL_BYTES]);
            }
        }
    }

    /// The image registered as `id`, or `None` for an id from another atlas
    /// or one unregistered.
    #[must_use]
    pub fn image(&self, id: ImageId) -> Option<AtlasImage> {
        self.images
            .binary_search_by_key(&id.0, |image| image.id.0)
            .ok()
            .map(|at| self.images[at])
    }

    /// Every registered image still held, in registration order.
    #[must_use]
    pub fn images(&self) -> &[AtlasImage] {
        &self.images
    }

    /// The whole page: [`PAGE_SIZE`] squared texels of RGBA8, rows top to
    /// bottom.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The page texels changed since the last [`take_dirty`](Self::take_dirty),
    /// or `None` when nothing has.
    #[must_use]
    pub const fn dirty(&self) -> Option<TexelRect> {
        self.dirty
    }

    /// Hands back the changed rectangle and forgets it.
    ///
    /// What a renderer calls once it has taken the region into an upload, so
    /// the next frame uploads only what changes after this.
    pub fn take_dirty(&mut self) -> Option<TexelRect> {
        self.dirty.take()
    }

    /// Marks `rect` as changed again, for an upload that was taken and then not
    /// recorded.
    pub fn mark_dirty(&mut self, rect: TexelRect) {
        self.dirty = Some(self.dirty.map_or(rect, |dirty| dirty.union(rect)));
    }

    /// The bytes of `rect`, rows top to bottom, tightly packed.
    #[must_use]
    pub fn region(&self, rect: TexelRect) -> Vec<u8> {
        let page = PAGE_SIZE as usize;
        let row_bytes = rect.width as usize * TEXEL_BYTES;
        let mut out = Vec::with_capacity(row_bytes * rect.height as usize);
        for row in rect.y as usize..(rect.y + rect.height) as usize {
            let start = (row * page + rect.x as usize) * TEXEL_BYTES;
            out.extend_from_slice(&self.pixels[start..start + row_bytes]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `width` by `height` texels, each one's colour its own position — so a
    /// texel copied from the wrong place is a different value.
    fn numbered(width: u32, height: u32) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as u8, y as u8, 200, 255]);
            }
        }
        pixels
    }

    fn texel(atlas: &ImageAtlas, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * PAGE_SIZE + x) * 4) as usize;
        atlas.pixels()[at..at + 4]
            .try_into()
            .expect("four channels")
    }

    #[test]
    fn a_registered_image_is_on_the_page_where_it_says() {
        let mut atlas = ImageAtlas::new();
        let first = atlas.register(3, 2, &numbered(3, 2)).expect("fits");
        let second = atlas.register(5, 4, &numbered(5, 4)).expect("fits");

        assert_eq!(first.id.index(), 0);
        assert_eq!(second.id.index(), 1);
        assert_eq!(atlas.image(second.id), Some(second));
        for image in [first, second] {
            for y in 0..image.height {
                for x in 0..image.width {
                    assert_eq!(
                        texel(&atlas, image.x + x, image.y + y),
                        [x as u8, y as u8, 200, 255],
                        "image {:?} texel ({x}, {y})",
                        image.id
                    );
                }
            }
        }
        // The two do not overlap, gutters included: the second is past the
        // first's right gutter or below its bottom one.
        assert!(
            second.x >= first.x + first.width + 2 * GUTTER
                || second.y >= first.y + first.height + 2 * GUTTER,
            "{first:?} {second:?}"
        );
    }

    #[test]
    fn the_gutter_repeats_the_images_own_edge() {
        let mut atlas = ImageAtlas::new();
        let image = atlas.register(4, 3, &numbered(4, 3)).expect("fits");
        let (x0, y0) = (image.x, image.y);
        let (x1, y1) = (image.x + image.width - 1, image.y + image.height - 1);
        // Left and right columns, top and bottom rows, and the four corners.
        assert_eq!(texel(&atlas, x0 - 1, y0 + 1), texel(&atlas, x0, y0 + 1));
        assert_eq!(texel(&atlas, x1 + 1, y0 + 2), texel(&atlas, x1, y0 + 2));
        assert_eq!(texel(&atlas, x0 + 2, y0 - 1), texel(&atlas, x0 + 2, y0));
        assert_eq!(texel(&atlas, x0 + 3, y1 + 1), texel(&atlas, x0 + 3, y1));
        assert_eq!(texel(&atlas, x0 - 1, y0 - 1), texel(&atlas, x0, y0));
        assert_eq!(texel(&atlas, x1 + 1, y1 + 1), texel(&atlas, x1, y1));
        // And the gutter really was written: the page is transparent elsewhere.
        assert_eq!(texel(&atlas, x0 - 1, y0 - 1)[3], 255);
        assert_eq!(texel(&atlas, x1 + 3, y1 + 3), [0; 4]);
    }

    /// **A texel centre in image space is a texel centre on the page, exactly.**
    /// The shader's sharp-bilinear bend reads a UV back into texels, so a UV an
    /// ulp off a centre is a UV that blends with a neighbour.
    #[test]
    fn uvs_land_exactly_on_texel_centres_and_boundaries() {
        let mut atlas = ImageAtlas::new();
        atlas.register(7, 9, &numbered(7, 9)).expect("fits");
        let image = atlas.register(13, 5, &numbered(13, 5)).expect("fits");
        let page = PAGE_SIZE as f32;
        for y in 0..image.height {
            for x in 0..image.width {
                let centre = image.uv(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                assert_eq!(
                    centre * page,
                    Vec2::new((image.x + x) as f32 + 0.5, (image.y + y) as f32 + 0.5),
                    "texel ({x}, {y})"
                );
            }
        }
        assert_eq!(
            image.uv_min() * page,
            Vec2::new(image.x as f32, image.y as f32)
        );
        assert_eq!(
            image.uv_max() * page,
            Vec2::new((image.x + 13) as f32, (image.y + 5) as f32)
        );
    }

    #[test]
    fn a_taller_image_opens_a_shelf_below_rather_than_overlapping() {
        let mut atlas = ImageAtlas::new();
        let short = atlas.register(10, 4, &numbered(10, 4)).expect("fits");
        let tall = atlas.register(10, 20, &numbered(10, 20)).expect("fits");
        assert!(
            tall.y >= short.y + short.height + GUTTER,
            "a 20-tall image cannot sit on a 6-tall shelf: {short:?} {tall:?}"
        );
        // And a short one after it still goes back on the first shelf.
        let again = atlas.register(10, 4, &numbered(10, 4)).expect("fits");
        assert_eq!(again.y, short.y);
    }

    #[test]
    fn a_full_page_refuses_rather_than_panicking_and_changes_nothing() {
        let mut atlas = ImageAtlas::new();
        let side = PAGE_SIZE - 2 * GUTTER;
        atlas
            .register(side, side, &vec![255; (side * side * 4) as usize])
            .expect("exactly the page, gutters included");
        let before = (atlas.images().len(), atlas.dirty());
        assert_eq!(
            atlas.register(1, 1, &[1, 2, 3, 4]),
            Err(AtlasError::Full {
                width: 1,
                height: 1
            })
        );
        assert_eq!((atlas.images().len(), atlas.dirty()), before);

        // Larger than the page at all, on a fresh atlas.
        let mut fresh = ImageAtlas::new();
        let over = PAGE_SIZE - 1;
        assert!(matches!(
            fresh.register(over, 1, &vec![0; (over * 4) as usize]),
            Err(AtlasError::Full { .. })
        ));
        assert!(fresh.images().is_empty());
    }

    #[test]
    fn a_malformed_image_is_refused() {
        let mut atlas = ImageAtlas::new();
        assert_eq!(atlas.register(0, 4, &[]), Err(AtlasError::Empty));
        assert_eq!(
            atlas.register(2, 2, &[0; 15]),
            Err(AtlasError::PixelCount {
                expected: 16,
                actual: 15
            })
        );
        assert!(atlas.images().is_empty());
        assert_eq!(atlas.dirty(), None);
    }

    /// A page filled with `side`-square images, `side` plus the gutters
    /// dividing the page exactly, so not one more fits.
    fn filled(side: u32) -> (ImageAtlas, Vec<AtlasImage>) {
        let padded = side + 2 * GUTTER;
        assert_eq!(PAGE_SIZE % padded, 0, "the images tile the page");
        let mut atlas = ImageAtlas::new();
        let count = (PAGE_SIZE / padded).pow(2);
        let images = (0..count)
            .map(|_| {
                atlas
                    .register(side, side, &numbered(side, side))
                    .expect("fits")
            })
            .collect();
        assert!(matches!(
            atlas.register(1, 1, &[0; 4]),
            Err(AtlasError::Full { .. })
        ));
        (atlas, images)
    }

    /// **An unregistered image's space takes the next image that fits, and its
    /// id names nothing after.** The launcher case: a full page of icons, one
    /// replaced.
    #[test]
    fn an_unregistered_image_gives_its_space_to_the_next_and_its_id_to_none() {
        let side = 62;
        let (mut atlas, images) = filled(side);
        let gone = images[images.len() / 2];
        let kept = images[images.len() / 2 + 1];

        assert_eq!(atlas.unregister(gone.id), Some(gone));
        assert_eq!(atlas.image(gone.id), None);
        assert_eq!(atlas.unregister(gone.id), None, "only once");
        assert_eq!(atlas.image(kept.id), Some(kept), "the rest are still found");
        assert_eq!(atlas.images().len(), images.len() - 1);
        assert!(!atlas.images().contains(&gone));

        let new = atlas
            .register(side, side, &numbered(side, side))
            .expect("the freed space");
        assert_eq!((new.x, new.y), (gone.x, gone.y));
        assert!(new.id.index() > images.last().expect("filled").id.index());
        assert_eq!(atlas.image(new.id), Some(new));
        assert_eq!(
            atlas.image(gone.id),
            None,
            "the new image took the space, not the id"
        );
    }

    /// **Free spans beside each other join**, so two small images given back
    /// make room for one twice as wide; and the join reaches back to the
    /// shelf's cursor when the span ends there.
    #[test]
    fn neighbouring_free_spans_join_into_room_for_a_wider_image() {
        let side = 62;
        let padded = side + 2 * GUTTER;
        let (mut atlas, images) = filled(side);
        // The first and second of the first shelf.
        atlas.unregister(images[1].id).expect("held");
        atlas.unregister(images[0].id).expect("held");
        let wide = 2 * padded - 2 * GUTTER;
        let joined = atlas
            .register(wide, side, &numbered(wide, side))
            .expect("the two spans as one");
        assert_eq!((joined.x, joined.y), (images[0].x, images[0].y));

        // Given back again, the span holds two narrow images side by side: the
        // first takes its left end and the second what remains.
        atlas.unregister(joined.id).expect("held");
        let left = atlas
            .register(side, side, &numbered(side, side))
            .expect("fits");
        let right = atlas
            .register(side, side, &numbered(side, side))
            .expect("fits");
        assert_eq!(
            [(left.x, left.y), (right.x, right.y)],
            [(images[0].x, images[0].y), (images[1].x, images[1].y)]
        );

        // The last two of the first shelf: the second one freed joins the first
        // and both go back to the cursor, which the next wide image uses.
        let per_shelf = (PAGE_SIZE / padded) as usize;
        atlas.unregister(images[per_shelf - 2].id).expect("held");
        atlas.unregister(images[per_shelf - 1].id).expect("held");
        let at_end = atlas
            .register(wide, side, &numbered(wide, side))
            .expect("the shelf's end");
        assert_eq!((at_end.x, at_end.y), (images[per_shelf - 2].x, images[0].y));
    }

    /// **An emptied last shelf closes**, so its rows can take an image taller
    /// than the shelf was.
    #[test]
    fn emptying_the_last_shelf_frees_its_rows_for_a_taller_image() {
        let mut atlas = ImageAtlas::new();
        let rest = PAGE_SIZE - 2 * GUTTER;
        let top = atlas
            .register(rest, rest - 10, &vec![9; (rest * (rest - 10) * 4) as usize])
            .expect("fits");
        let low = atlas.register(4, 4, &numbered(4, 4)).expect("fits below");
        assert!(
            matches!(
                atlas.register(4, 7, &numbered(4, 7)),
                Err(AtlasError::Full { .. })
            ),
            "taller than the low shelf, and no rows left under it"
        );

        atlas.unregister(low.id).expect("held");
        let tall = atlas
            .register(4, 7, &numbered(4, 7))
            .expect("the closed shelf's rows");
        assert_eq!(tall.y, top.y + top.height + 2 * GUTTER);
    }

    /// The dirty rectangle covers exactly what registration wrote, and taking
    /// it leaves nothing behind — the renderer's "upload only when it changed".
    #[test]
    fn the_dirty_rectangle_is_what_changed_and_taking_it_clears_it() {
        let mut atlas = ImageAtlas::new();
        assert_eq!(atlas.dirty(), None, "a fresh page owes no upload");

        let a = atlas.register(6, 3, &numbered(6, 3)).expect("fits");
        let b = atlas.register(2, 8, &numbered(2, 8)).expect("fits");
        let dirty = atlas.take_dirty().expect("two images changed the page");
        for image in [a, b] {
            assert!(dirty.x + GUTTER <= image.x && dirty.y + GUTTER <= image.y);
            assert!(image.x + image.width + GUTTER <= dirty.x + dirty.width);
            assert!(image.y + image.height + GUTTER <= dirty.y + dirty.height);
        }
        assert_eq!(atlas.dirty(), None);

        let region = atlas.region(dirty);
        assert_eq!(region.len(), (dirty.width * dirty.height * 4) as usize);
        let row = (b.y - dirty.y) as usize;
        let column = (b.x - dirty.x) as usize;
        let at = (row * dirty.width as usize + column) * 4;
        assert_eq!(&region[at..at + 4], &[0, 0, 200, 255]);

        atlas.mark_dirty(dirty);
        assert_eq!(
            atlas.dirty(),
            Some(dirty),
            "an unrecorded upload comes back"
        );
    }
}
