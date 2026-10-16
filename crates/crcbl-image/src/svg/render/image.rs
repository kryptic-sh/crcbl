//! Drawing an `<image>`: the picture its `data:` URL embeds, placed in its
//! viewport by `preserveAspectRatio` and resampled to the pixels it covers.
//!
//! # What may be embedded
//!
//! PNG, JPEG and WebP, each decoded by this crate's decoder for it when that
//! format's feature is built and refused by name when it is not, and SVG,
//! drawn by this renderer as vectors, as resvg draws one. A reference to
//! anything outside the file is refused ([`data_url`] says why). An SVG
//! image is a document of its own: nothing of the outer one's style
//! reaches it, and its ids name only its own elements.
//!
//! # Placement
//!
//! As SVG 2 says: `x`, `y`, `width` and `height` are the image's viewport, a
//! missing (or `auto`) side taking the image's own size, or the other side's
//! scaled by the image's aspect; `preserveAspectRatio` fits the image's own
//! size into that viewport, and `slice` cuts what overflows it. A zero or
//! negative side draws nothing. `image-rendering` is ignored: every image is
//! resampled the one way below.
//!
//! # Resampling
//!
//! As resvg draws a raster image: a pattern sampled at each pixel's centre
//! through the inverse of the image's transform, by nearest neighbour when
//! that transform only translates, and otherwise by the bicubic of Mitchell
//! and Netravali with `B = C = 1/3` over the four by four pixels nearest the
//! sample, the edge pixels repeated outwards, in premultiplied colour. A
//! shrunk image is sampled the same way, unfiltered: [`resize`] first, whose
//! Lanczos filter widens with the shrink, would alias less but draws a
//! different picture from resvg's at small sizes.
//!
//! [`resize`]: crate::resize::resize
//!
//! # Hostile images
//!
//! Every embedded image decodes under the caller's [`Limits`], which bound
//! one image, and under what is left of a budget of the same `max_bytes`
//! for the whole document, which bounds them all: each raster image's RGBA
//! is charged once, when it is decoded (each `<image>` element is decoded
//! once however often a `use` draws it), and each SVG image's source
//! whenever it is drawn, since drawing it walks the whole document again.
//! So neither a `use` of one image many times nor SVGs embedding SVGs can
//! multiply the work. SVG images nest at most [`MAX_NESTING`] deep.

use std::rc::Rc;

use super::{MAX_LAYERS, Renderer, Viewport, view_box_transform};
use crate::raster::FillRule;
use crate::svg::canvas::{Canvas, Rgba};
use crate::svg::data_url::{self, DataUrl, MediaType};
use crate::svg::path::{self, Rect};
use crate::svg::style::Style;
use crate::svg::transform::Transform;
use crate::svg::unsupported;
use crate::svg::xml::{self, Document, Namespace};
use crate::{ImageError, Limits, Rgba8Image};

/// The most SVG images that may be inside one another, the outermost
/// document not counted.
pub(in crate::svg) const MAX_NESTING: usize = 4;

/// An `<image>`'s decoded picture.
#[derive(Debug)]
pub(in crate::svg) enum Embedded {
    Raster(Rgba8Image),
    Svg {
        document: Document,
        viewport: Viewport,
        /// The length of the source it was parsed from.
        source: usize,
    },
}

impl Embedded {
    /// Its own size, which `preserveAspectRatio` fits into the viewport:
    /// pixels for a raster image, its outermost viewport for an SVG.
    fn size(&self) -> (f64, f64) {
        match self {
            Self::Raster(image) => (f64::from(image.width()), f64::from(image.height())),
            Self::Svg { viewport, .. } => (viewport.width, viewport.height),
        }
    }
}

/// Where an `<image>` draws.
#[derive(Clone, Copy, Debug)]
pub(in crate::svg) struct Layout {
    /// Its viewport, `x`, `y`, `width` and `height`, in its user space: its
    /// object bounding box.
    pub viewport: Rect,
    /// The map from the image's own size to its user space.
    place: Transform,
    /// Whether what overflows the viewport is cut.
    slice: bool,
}

impl MediaType {
    /// The format's name and the cargo feature that builds its decoder.
    fn name_and_feature(self) -> (&'static str, &'static str) {
        match self {
            Self::Png => ("PNG", "png"),
            Self::Jpeg => ("JPEG", "jpeg"),
            Self::Webp => ("WebP", "webp"),
            Self::Svg => ("SVG", "svg"),
        }
    }
}

/// `a` and `b`'s overlap, or `None` if it has no area.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let width = (a.x + a.width).min(b.x + b.width) - x;
    let height = (a.y + a.height).min(b.y + b.height) - y;
    (width > 0.0 && height > 0.0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

impl Renderer<'_> {
    /// Draws the `<image>` at `index`, styled `style`, whose user space maps
    /// to pixels by `transform`.
    pub(super) fn image(
        &mut self,
        index: usize,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        self.check_shape_children(self.element(index))?;
        let Some((embedded, layout)) = self.image_layout(index)? else {
            return Ok(());
        };
        if !style.visible {
            return Ok(());
        }
        match &*embedded {
            Embedded::Raster(image) => self.draw_raster(image, &layout, transform, canvas),
            Embedded::Svg {
                document,
                viewport,
                source,
            } => self.draw_svg(document, viewport, *source, &layout, transform, canvas),
        }
    }

    /// The `<image>` at `index`'s picture and where it draws, or `None` for
    /// one that draws nothing: no `href`, or a side that is not positive.
    pub(super) fn image_layout(
        &mut self,
        index: usize,
    ) -> Result<Option<(Rc<Embedded>, Layout)>, ImageError> {
        let Some(embedded) = self.embedded(index)? else {
            return Ok(None);
        };
        let element = self.element(index);
        let (own_width, own_height) = embedded.size();
        let (w, h) = self.viewport;
        let given = |name: &str, reference: f64| -> Result<Option<f64>, ImageError> {
            match element.attribute(Namespace::None, name) {
                None | Some("auto") => Ok(None),
                Some(_) => self.length(element, name, reference, 0.0).map(Some),
            }
        };
        let (width, height) = match (given("width", w)?, given("height", h)?) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, own_height * width / own_width),
            (None, Some(height)) => (own_width * height / own_height, height),
            (None, None) => (own_width, own_height),
        };
        if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
            return Ok(None);
        }
        let viewport = Rect {
            x: self.length(element, "x", w, 0.0)?,
            y: self.length(element, "y", h, 0.0)?,
            width,
            height,
        };
        let aspect = element
            .attribute(Namespace::None, "preserveAspectRatio")
            .unwrap_or("");
        let own = Rect {
            x: 0.0,
            y: 0.0,
            width: own_width,
            height: own_height,
        };
        let place = Transform::translate(viewport.x, viewport.y)
            .then(view_box_transform(own, width, height, aspect)?);
        let slice = aspect.split_ascii_whitespace().any(|word| word == "slice");
        Ok(Some((
            embedded,
            Layout {
                viewport,
                place,
                slice,
            },
        )))
    }

    /// Adds `bytes` to the document's image budget, refusing the file if
    /// that would pass the caller's `max_bytes`.
    fn charge(&mut self, bytes: usize) -> Result<(), ImageError> {
        if bytes > self.remaining_limits().max_bytes {
            return Err(unsupported(format_args!(
                "images processing more than {} bytes together",
                self.limits.max_bytes
            )));
        }
        self.image_bytes += bytes;
        Ok(())
    }

    /// The caller's limits with `max_bytes` cut to what is left of the image
    /// budget, so a decode past it is refused before it allocates.
    fn remaining_limits(&self) -> Limits {
        Limits {
            max_bytes: self.limits.max_bytes.saturating_sub(self.image_bytes),
            ..self.limits
        }
    }

    /// The picture the `<image>` at `index` embeds, decoded the first time
    /// it is asked for; `None` for an `<image>` with no `href`.
    fn embedded(&mut self, index: usize) -> Result<Option<Rc<Embedded>>, ImageError> {
        if let Some(embedded) = self.images.get(&index) {
            return Ok(Some(Rc::clone(embedded)));
        }
        let element = self.element(index);
        let Some(href) = element
            .attribute(Namespace::None, "href")
            .or_else(|| element.attribute(Namespace::Xlink, "href"))
        else {
            return Ok(None);
        };
        let url = data_url::parse(href)?;
        let embedded = if url.media_type == MediaType::Svg {
            if self.nesting >= MAX_NESTING {
                return Err(unsupported(format_args!(
                    "SVG images nested more than {MAX_NESTING} deep"
                )));
            }
            let document = xml::parse(&url.bytes)?;
            let viewport = Viewport::of(&document)?;
            Embedded::Svg {
                document,
                viewport,
                source: url.bytes.len(),
            }
        } else {
            let image = self.decode_raster(&url)?;
            self.charge(image.pixels().len())?;
            Embedded::Raster(image)
        };
        let embedded = Rc::new(embedded);
        self.images.insert(index, Rc::clone(&embedded));
        Ok(Some(embedded))
    }

    /// A raster `url`'s image, decoded under what is left of the budget.
    fn decode_raster(&self, url: &DataUrl) -> Result<Rgba8Image, ImageError> {
        match url.media_type {
            #[cfg(feature = "png")]
            MediaType::Png => crate::png::decode(&url.bytes, &self.remaining_limits()),
            #[cfg(feature = "jpeg")]
            MediaType::Jpeg => crate::jpeg::decode(&url.bytes, &self.remaining_limits()),
            #[cfg(feature = "webp")]
            MediaType::Webp => crate::webp::decode(&url.bytes, &self.remaining_limits()),
            other => {
                let (name, feature) = other.name_and_feature();
                Err(unsupported(format_args!(
                    "<image> of {name}, which this build does not decode \
                     (crcbl-image's {feature} feature is off)"
                )))
            }
        }
    }

    /// Draws a raster image placed by `layout` in a user space that maps to
    /// pixels by `transform`.
    fn draw_raster(
        &mut self,
        image: &Rgba8Image,
        layout: &Layout,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let own = Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(image.width()),
            height: f64::from(image.height()),
        };
        // Placing only scales and translates, so the image's box in user
        // space is a box; meet leaves it inside the viewport and slice cuts
        // it to it.
        let Some(drawn) = intersect(super::map_rect(own, layout.place), layout.viewport) else {
            return Ok(());
        };
        let Some(inverse) = transform.then(layout.place).invert() else {
            return Ok(());
        };
        let frame = path::rect(drawn.x, drawn.y, drawn.width, drawn.height, 0.0, 0.0);
        let coverage = self.fill_coverage(&frame, transform, FillRule::NonZero);
        let sampler = Sampler::new(image, inverse);
        let side = self.side as usize;
        canvas.paint(&coverage, |index| {
            sampler.at((index % side) as f64 + 0.5, (index / side) as f64 + 0.5)
        });
        Ok(())
    }

    /// Draws an SVG image, `document` with its `viewport` parsed from
    /// `source` bytes, placed by `layout` in a user space that maps to pixels
    /// by `transform`.
    fn draw_svg(
        &mut self,
        document: &Document,
        viewport: &Viewport,
        source: usize,
        layout: &Layout,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        self.charge(source)?;
        let clip = if layout.slice {
            let frame = layout.viewport;
            let frame = path::rect(frame.x, frame.y, frame.width, frame.height, 0.0, 0.0);
            self.layers += 1;
            if self.layers > MAX_LAYERS {
                return Err(unsupported(format_args!(
                    "group effects nested more than {MAX_LAYERS} deep"
                )));
            }
            Some(self.fill_coverage(&frame, transform, FillRule::NonZero))
        } else {
            None
        };
        let mut nested = Renderer::new(
            document,
            viewport,
            transform.then(layout.place),
            self.side,
            self.limits,
            self.fonts,
        )?;
        nested.nesting = self.nesting + 1;
        nested.depth = self.depth;
        nested.instances = self.instances;
        nested.layers = self.layers;
        nested.image_bytes = self.image_bytes;
        nested.characters = self.characters;
        nested.rasterizer = std::mem::take(&mut self.rasterizer);
        std::mem::swap(&mut nested.faces, &mut self.faces);
        let result = match &clip {
            Some(clip) => {
                let mut layer = Canvas::new(self.side);
                let result = nested.draw_root(viewport, true, &mut layer);
                canvas.composite(&layer, |index| clip[index]);
                result
            }
            None => nested.draw_root(viewport, true, canvas),
        };
        self.rasterizer = std::mem::take(&mut nested.rasterizer);
        std::mem::swap(&mut nested.faces, &mut self.faces);
        self.characters = nested.characters;
        self.instances = nested.instances;
        self.image_bytes = nested.image_bytes;
        if clip.is_some() {
            self.layers -= 1;
        }
        result
    }
}

/// Mitchell and Netravali's cubic with `B = C = 1/3` at a tap `t` in
/// `0..=1` from the sample, for the two taps nearest it; the form
/// tiny-skia's pipeline evaluates.
fn near(t: f32) -> f32 {
    ((-21.0 / 18.0 * t + 27.0 / 18.0) * t + 9.0 / 18.0) * t + 1.0 / 18.0
}

/// The same cubic for the two taps a pixel further out, `t` being the
/// fraction a near tap would take.
fn far(t: f32) -> f32 {
    t * t * (7.0 / 18.0 * t - 6.0 / 18.0)
}

/// An image sampled at canvas pixels, as resvg samples a raster image.
struct Sampler<'i> {
    image: &'i Rgba8Image,
    inverse: Transform,
    nearest: bool,
}

impl<'i> Sampler<'i> {
    /// Sampling `image` through `inverse`, the map from canvas pixels to its
    /// own: nearest neighbour where that only translates (in the single
    /// precision resvg computes it in), bicubic otherwise.
    fn new(image: &'i Rgba8Image, inverse: Transform) -> Self {
        let one = |value: f64| value as f32 == 1.0;
        let zero = |value: f64| value as f32 == 0.0;
        let nearest = one(inverse.a) && zero(inverse.b) && zero(inverse.c) && one(inverse.d);
        Self {
            image,
            inverse,
            nearest,
        }
    }

    /// The premultiplied pixel at column `x` and row `y`, each clamped to
    /// the image so its edge repeats outwards.
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let width = i64::from(self.image.width());
        let height = i64::from(self.image.height());
        let (x, y) = (x.clamp(0, width - 1), y.clamp(0, height - 1));
        // Both are inside the image, whose size passed the limits.
        let at = ((y * width + x) * 4) as usize;
        let pixel = &self.image.pixels()[at..at + 4];
        let alpha = f32::from(pixel[3]) / 255.0;
        [
            f32::from(pixel[0]) / 255.0 * alpha,
            f32::from(pixel[1]) / 255.0 * alpha,
            f32::from(pixel[2]) / 255.0 * alpha,
            alpha,
        ]
    }

    /// The straight-alpha colour at the canvas point `(x, y)`, or `None`
    /// where it is transparent.
    fn at(&self, x: f64, y: f64) -> Option<Rgba> {
        let (u, v) = self.inverse.apply((x, y));
        // `as` saturates, and sends NaN to zero.
        let [r, g, b, a] = if self.nearest {
            self.texel(u.floor() as i64, v.floor() as i64)
        } else {
            // The taps are the pixels whose centres lie within two of the
            // sample: centres at u - 1.5 + i, i from 0 to 3.
            let (u, v) = (u + 0.5, v + 0.5);
            let (fx, fy) = ((u - u.floor()) as f32, (v - v.floor()) as f32);
            let (left, top) = (u.floor() as i64 - 2, v.floor() as i64 - 2);
            let across = [far(1.0 - fx), near(1.0 - fx), near(fx), far(fx)];
            let down = [far(1.0 - fy), near(1.0 - fy), near(fy), far(fy)];
            let mut sum = [0.0f32; 4];
            for (j, wy) in (0..).zip(down) {
                for (i, wx) in (0..).zip(across) {
                    let texel = self.texel(left + i, top + j);
                    for (total, channel) in sum.iter_mut().zip(texel) {
                        *total += wx * wy * channel;
                    }
                }
            }
            sum
        };
        // The cubic's negative lobes can overshoot: alpha is clamped to its
        // range, and colour to its alpha.
        let a = a.clamp(0.0, 1.0);
        if a <= 0.0 {
            return None;
        }
        let straight = |c: f32| c.clamp(0.0, a) / a;
        Some([straight(r), straight(g), straight(b), a])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The cubic's weights are Mitchell and Netravali's**: at a pixel
    /// centre 1/18, 16/18, 1/18 and 0, and halfway between centres the two
    /// near taps 77/144 and the far ones −5/144 (the kernel's values at a
    /// half and one and a half), summing to one at every fraction.
    #[test]
    fn the_cubic_weights_are_mitchells() {
        let weights = |t: f32| [far(1.0 - t), near(1.0 - t), near(t), far(t)];
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(close(
            weights(0.0),
            [1.0 / 18.0, 16.0 / 18.0, 1.0 / 18.0, 0.0]
        ));
        assert!(close(
            weights(0.5),
            [-5.0 / 144.0, 77.0 / 144.0, 77.0 / 144.0, -5.0 / 144.0]
        ));
        for step in 0..=16 {
            let total: f32 = weights(step as f32 / 16.0).iter().sum();
            assert!((total - 1.0).abs() < 1e-6, "{step}");
        }
    }
}
