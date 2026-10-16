//! SVG icons rendered to RGBA: the subset of SVG that desktop icon themes
//! are drawn in, rasterised by this crate's own coverage rasteriser.
//!
//! # A subset that refuses what it does not draw
//!
//! Icon themes use a small part of SVG, and a renderer of a part has to
//! choose what to do with the rest. Drawing what it understands and dropping
//! the rest gives an icon with pieces missing — a background plate with no
//! glyph on it — which is worse than no icon, because the caller cannot tell
//! it went wrong and so never falls back to its generic one. So here, **an
//! element, attribute, property value or feature this renderer does not
//! draw is a decode error**, [`ImageError::Unsupported`] naming it, and
//! never a partial image.
//!
//! What has no effect on the picture is not unsupported, and is ignored by
//! design:
//!
//! * elements and attributes outside the SVG namespace — Inkscape's,
//!   Sodipodi's, RDF, Creative Commons and Dublin Core metadata, any
//!   editor's — which no SVG renderer draws;
//! * `<title>`, `<desc>` and `<metadata>`, comments, processing instructions
//!   and the DOCTYPE, `<script>` (a still image runs none), event-handler,
//!   `data-` and `aria-` attributes;
//! * definitions nothing references: an unsupported element inside `<defs>`
//!   refuses the file only when something uses it;
//! * the properties `style`'s module lists as having no effect here.
//!
//! What is drawn: `svg` with `viewBox`, `width`, `height` and
//! `preserveAspectRatio`; `g`, `defs` and `use`; `path` (all of SVG 2's path
//! grammar), `rect`, `circle`, `ellipse`, `line`, `polyline` and `polygon`;
//! `transform`; presentation attributes, the `style` attribute and a
//! `<style>` sheet of type, class and id selectors; fill and stroke with
//! their opacities, rules, joins, caps, miter limits and dashes; colours,
//! `currentColor`, linear and radial gradients; `clipPath` and `mask`;
//! group opacity; and filters of the primitives drop shadows are made of
//! (`feGaussianBlur`, `feOffset`, `feFlood`, `feComposite`, `feColorMatrix`
//! and `feMerge`). What is refused, among other things: text, images, other
//! filter primitives and filter functions, patterns, markers, `symbol`,
//! nested `svg`, `switch`, blend modes, animation, and CSS beyond simple
//! selectors.
//!
//! # Fitting the square
//!
//! [`decode`] draws into a transparent square of `size × scale` pixels, the
//! SVG's viewport scaled to fit its longer side and centred, so an icon
//! that is not square keeps its aspect.
//!
//! # Hostile files
//!
//! The XML parser expands only a DTD's plain-text entities, which reference
//! nothing, within caps, and fetches nothing; the tree is capped in depth and
//! size; `use` instances, layers, dashes and filter
//! primitives are capped where a small file could multiply them; a filter
//! region is cut to the canvas; and the canvas passes the caller's
//! [`Limits`] before it is allocated.

mod canvas;
mod color;
mod css;
mod filter;
mod paint;
mod path;
mod render;
mod stroke;
mod style;
mod transform;
mod xml;

#[cfg(test)]
mod tests;

use crate::{ImageError, Limits, Rgba8Image};

fn unsupported(what: impl core::fmt::Display) -> ImageError {
    ImageError::Unsupported(format!("SVG {what}"))
}

fn malformed(what: impl core::fmt::Display) -> ImageError {
    ImageError::Malformed(format!("SVG: {what}"))
}

/// Renders an SVG into a transparent square of `size × scale` pixels, its
/// viewport scaled to fit and centred.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the square breaks
/// `limits` (checked before anything is parsed or allocated);
/// [`ImageError::Malformed`] for bytes that are not a well-formed SVG,
/// including a zero, negative or non-finite `viewBox`, `width` or `height`;
/// and [`ImageError::Unsupported`] for anything the file uses that this
/// renderer does not draw, as the module docs describe.
pub fn decode(
    bytes: &[u8],
    size: u32,
    scale: u32,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    let side = size.saturating_mul(scale);
    limits.rgba_bytes(side, side)?;
    let document = xml::parse(bytes)?;
    Ok(render::render(&document, side)?.into_image())
}
