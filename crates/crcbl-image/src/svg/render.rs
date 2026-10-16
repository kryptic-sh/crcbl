//! The walk over a parsed document that draws it: elements to geometry,
//! styles to paint, group effects to layers, references followed.
//!
//! # What is drawn, what is skipped, what is refused
//!
//! An element in a rendering position is drawn if it is a group (`g`), a
//! `use` or a basic shape; skipped if it is one SVG never renders in place
//! ([`NOT_RENDERED`]: definitions, paint servers, clip paths, masks and
//! filters, descriptions, style sheets), since those only matter where
//! something references them; skipped too if it is no SVG element at all
//! though in SVG's namespace ([`NOT_SVG`]: Inkscape's flowed text), as
//! browsers and resvg skip it; and refused otherwise. A reference is where
//! the target is checked: a `use` of a `symbol`, a fill of a `pattern`, a
//! filter with a primitive outside the subset, all refuse the file there.
//! An element outside the SVG namespace is an editor's private data and is
//! never rendered by any SVG renderer, so it is skipped with its whole
//! subtree.
//!
//! A reference to an id nothing has is not refused: SVG defines what it
//! means. A `use` of nothing draws nothing, a paint server that is missing
//! paints the fallback colour or none, and a missing clip path or mask is as
//! if none were given, as CSS Masking says of a clip path and as resvg draws
//! both.
//!
//! # Group effects
//!
//! An element with `opacity` below one, a `clip-path`, a `mask` or a
//! `filter` is drawn into a layer of its own. The filter, if any, replaces
//! the layer with its result; then the layer is composited scaled by its
//! opacity, its clip path's coverage and its mask's value at each pixel:
//! SVG's compositing model, in which a group's effects apply to the group as
//! a whole, not to each of its children, filter first.

mod filter;
pub(super) mod image;

use std::collections::HashMap;
use std::rc::Rc;

use super::canvas::Canvas;
use super::paint::{self, Gradient, Shape, Spread, Stop};
use super::path::{self, Path, Rect, Segment};
use super::stroke::{self, Stroke};
use super::style::{self, Cascade, Length, Paint, Specified, Style, length};
use super::transform::{self, Transform};
use super::xml::{Document, Element, Namespace};
use super::{malformed, unsupported};
use crate::raster::{FillRule, Point, Rasterizer};
use crate::{ImageError, Limits};

/// The deepest the walk recurses, counting each element entered and each
/// reference followed: past it, a chain of `use` elements is refused before
/// it can exhaust the stack.
const MAX_RENDER_DEPTH: usize = 256;

/// The most elements drawn in all, counting each instance a `use` makes: a
/// file of a few nested `use` elements can otherwise multiply into billions.
const MAX_INSTANCES: usize = 100_000;

/// Layers open at once, counting each clip path and mask being drawn: each
/// holds a canvas-sized buffer or two, so this bounds the walk's memory to a
/// fixed multiple of the image the caller's limits allowed.
pub(super) const MAX_LAYERS: usize = 32;

/// How many times finer than the pixels, each way, coverage is accumulated
/// before it is averaged into them. The rasteriser's area integral is exact
/// for one edge in a cell, but two edges in one cell whose windings differ
/// (the seam where two halves of a symmetric icon meet, the slivers inside
/// a stroke's corner) read as their difference rather than their union.
/// Accumulating finer confines that error to a sub-cell, which is how
/// supersampling rasterisers such as resvg's avoid it altogether.
const SUBSAMPLES: u32 = 4;

/// Pixel rows rasterised at once: the accumulation grid is a strip this
/// tall, so its memory is a fraction of the canvas's however fine it is.
const STRIP_ROWS: usize = 16;

/// SVG elements never rendered where they stand, only when referenced (or,
/// for descriptions, scripts and style sheets, never).
const NOT_RENDERED: &[&str] = &[
    "clipPath",
    "color-profile",
    "cursor",
    "defs",
    "desc",
    "filter",
    "font",
    "font-face",
    "linearGradient",
    "marker",
    "mask",
    "metadata",
    "pattern",
    "radialGradient",
    "script",
    "solidColor",
    "solidcolor",
    "style",
    "symbol",
    "title",
    "view",
];

/// Elements in SVG's namespace that are no SVG element, which browsers and
/// resvg render nothing of, children included: Inkscape's flowed text,
/// from an SVG 1.2 draft that never became a standard.
const NOT_SVG: &[&str] = &["flowRoot"];

/// Children a shape may have that change nothing: descriptions.
const DESCRIPTIVE: &[&str] = &["desc", "metadata", "title"];

/// The elements drawn where they stand: groups, `use`, the basic shapes and
/// images.
const DRAWN: &[&str] = &[
    "circle", "ellipse", "g", "image", "line", "path", "polygon", "polyline", "rect", "use",
];

/// Attributes that change the picture in ways this renderer does not draw,
/// refused by name beside the properties `style` refuses: conditional
/// processing, which may leave an element out; a path length, which rescales
/// dashes; and a blur's edge mode.
const REFUSED_ATTRIBUTES: &[&str] = &[
    "edgeMode",
    "pathLength",
    "requiredExtensions",
    "systemLanguage",
];

/// Refuses an attribute of `element` whose effect this renderer does not
/// draw: one of [`REFUSED_ATTRIBUTES`], or a property `style` refuses by
/// name. Every other attribute is read by the code drawing its element, or
/// means nothing on it and is ignored, as browsers and resvg ignore it: an
/// editor's in its own namespace, a misspelling, an `href` on a shape, the
/// geometry of another kind of shape an editor left behind.
fn check_attributes(element: &Element) -> Result<(), ImageError> {
    for attribute in &element.attributes {
        let name = attribute.name.as_str();
        if matches!(attribute.namespace, Namespace::None | Namespace::Svg)
            && (REFUSED_ATTRIBUTES.contains(&name) || style::REFUSED_PROPERTIES.contains(&name))
        {
            return Err(unsupported(format_args!(
                "attribute {name} on <{}>",
                element.name
            )));
        }
    }
    Ok(())
}

/// The transform that fits the `view_box` into a `width` by `height`
/// viewport by `preserveAspectRatio`.
fn view_box_transform(
    view_box: Rect,
    width: f64,
    height: f64,
    aspect: &str,
) -> Result<Transform, ImageError> {
    let words: Vec<&str> = aspect
        .split_ascii_whitespace()
        .filter(|word| *word != "defer")
        .collect();
    let (align, slice) = match words.as_slice() {
        [] => ("xMidYMid", false),
        [align] => (*align, false),
        [align, "meet"] => (*align, false),
        [align, "slice"] => (*align, true),
        _ => return Err(unsupported(format_args!("preserveAspectRatio {aspect:?}"))),
    };
    let (mut sx, mut sy) = (width / view_box.width, height / view_box.height);
    let fraction = |text: &str| match text {
        "Min" => Some(0.0),
        "Mid" => Some(0.5),
        "Max" => Some(1.0),
        _ => None,
    };
    let (ax, ay) = if align == "none" {
        (0.0, 0.0)
    } else {
        let parts = align
            .strip_prefix('x')
            .and_then(|rest| Some((fraction(rest.get(..3)?)?, rest.get(3..)?)))
            .and_then(|(ax, rest)| Some((ax, fraction(rest.strip_prefix('Y')?)?)));
        let Some(parts) = parts else {
            return Err(unsupported(format_args!("preserveAspectRatio {aspect:?}")));
        };
        let s = if slice { sx.max(sy) } else { sx.min(sy) };
        sx = s;
        sy = s;
        parts
    };
    let tx = -view_box.x * sx + ax * (width - view_box.width * sx);
    let ty = -view_box.y * sy + ay * (height - view_box.height * sy);
    Ok(Transform::new(sx, 0.0, 0.0, sy, tx, ty))
}

/// Whether an element styled `style` is drawn through a layer of its own.
fn has_effects(style: &Style) -> bool {
    style.opacity < 1.0
        || style.clip_path.is_some()
        || style.mask.is_some()
        || style.filter.is_some()
}

/// The box mapped by `transform`, as the box round its corners.
fn transform_box(rect: Rect, transform: Transform) -> Rect {
    let corners = [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width, rect.y + rect.height),
    ]
    .map(|corner| transform.apply(corner));
    let (mut min, mut max) = (corners[0], corners[0]);
    for (x, y) in corners {
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    Rect {
        x: min.0,
        y: min.1,
        width: max.0 - min.0,
        height: max.1 - min.1,
    }
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            Some(Rect {
                x,
                y,
                width: (a.x + a.width).max(b.x + b.width) - x,
                height: (a.y + a.height).max(b.y + b.height) - y,
            })
        }
        (a, None) => a,
        (None, b) => b,
    }
}

/// Whether the units attribute `name` of `element` (`maskUnits`,
/// `filterUnits` and their kind) is `objectBoundingBox`, `default_object` if
/// it is absent.
fn units(element: &Element, name: &str, default_object: bool) -> Result<bool, ImageError> {
    match element.attribute(Namespace::None, name) {
        None => Ok(default_object),
        Some("objectBoundingBox") => Ok(true),
        Some("userSpaceOnUse") => Ok(false),
        Some(other) => Err(unsupported(format_args!("{name} {other:?}"))),
    }
}

/// A coordinate attribute in bounding-box units: a number is a fraction of
/// the box and a percentage a hundredth of one; `default` if absent.
fn fraction(element: &Element, name: &str, default: f64) -> Result<f64, ImageError> {
    match element.attribute(Namespace::None, name) {
        None => Ok(default),
        Some(text) => match length(text) {
            Some(Length::Px(value)) => Ok(value),
            Some(Length::Percent(percent)) => Ok(percent / 100.0),
            None => Err(unsupported(format_args!(
                "{name}={text:?} on <{}>",
                element.name
            ))),
        },
    }
}

/// `rect` mapped by `transform`, which only scales and translates.
fn map_rect(rect: Rect, transform: Transform) -> Rect {
    let (x, y) = transform.apply((rect.x, rect.y));
    Rect {
        x,
        y,
        width: rect.width * transform.a,
        height: rect.height * transform.d,
    }
}

/// The map from a bounding box's unit square to the box, for
/// `objectBoundingBox` units; `None` for a box with no area.
fn bounding_box_units(bbox: Option<Rect>) -> Option<Transform> {
    let bbox = bbox?;
    (bbox.width > 0.0 && bbox.height > 0.0)
        .then(|| Transform::new(bbox.width, 0.0, 0.0, bbox.height, bbox.x, bbox.y))
}

/// An SVG's outermost viewport: its size in the units it is placed in, the
/// map from its user space into that size, and the user-space size
/// percentages are of.
#[derive(Clone, Copy, Debug)]
pub(super) struct Viewport {
    pub width: f64,
    pub height: f64,
    inner: Transform,
    user: (f64, f64),
    slice: bool,
}

impl Viewport {
    /// The viewport of `document`, whose root must be an SVG `<svg>`.
    ///
    /// # Errors
    ///
    /// [`ImageError::Malformed`] for a root that is not `<svg>`, or a
    /// `viewBox`, `width` or `height` that is unusable.
    pub(super) fn of(document: &Document) -> Result<Self, ImageError> {
        let root = document.root();
        if !root.is("svg") {
            return Err(malformed(format_args!(
                "the root element is <{}>, not an SVG <svg>",
                root.name
            )));
        }
        check_attributes(root)?;
        let view_box = match root.attribute(Namespace::None, "viewBox") {
            None => None,
            Some(text) => match path::number_list(text).as_deref() {
                Some(&[x, y, width, height]) if width > 0.0 && height > 0.0 => Some(Rect {
                    x,
                    y,
                    width,
                    height,
                }),
                _ => return Err(malformed(format_args!("the viewBox {text:?}"))),
            },
        };
        let side_length = |name: &str, from_box: Option<f64>| -> Result<f64, ImageError> {
            let value = match root.attribute(Namespace::None, name) {
                None => from_box,
                Some(text) => match length(text) {
                    Some(Length::Px(value)) => Some(value),
                    Some(Length::Percent(percent)) => {
                        from_box.map(|reference| percent / 100.0 * reference)
                    }
                    None => return Err(malformed(format_args!("the {name} {text:?}"))),
                },
            };
            match value {
                Some(value) if value > 0.0 && value.is_finite() => Ok(value),
                _ => Err(malformed(format_args!("an SVG with no usable {name}"))),
            }
        };
        let width = side_length("width", view_box.map(|b| b.width))?;
        let height = side_length("height", view_box.map(|b| b.height))?;
        let aspect = root
            .attribute(Namespace::None, "preserveAspectRatio")
            .unwrap_or("");
        let (inner, user) = match view_box {
            Some(view_box) => (
                view_box_transform(view_box, width, height, aspect)?,
                (view_box.width, view_box.height),
            ),
            None => (Transform::IDENTITY, (width, height)),
        };
        Ok(Self {
            width,
            height,
            inner,
            user,
            slice: aspect.split_ascii_whitespace().any(|word| word == "slice"),
        })
    }
}

/// Draws `document` into a `side`-pixel square, decoding the images it
/// embeds under `limits`.
///
/// # Errors
///
/// [`ImageError::Malformed`] for a document that is not an SVG or whose
/// size is unusable, [`ImageError::Unsupported`] for anything it uses
/// that this renderer does not draw, and [`ImageError::TooLarge`] for
/// embedded images past `limits`.
pub(super) fn render(
    document: &Document,
    side: u32,
    limits: &Limits,
) -> Result<Canvas, ImageError> {
    let viewport = Viewport::of(document)?;
    let (width, height) = (viewport.width, viewport.height);
    let side_f = f64::from(side);
    let fit = side_f / width.max(height);
    let placement =
        Transform::translate((side_f - width * fit) / 2.0, (side_f - height * fit) / 2.0)
            .then(Transform::scale(fit, fit));
    let mut renderer = Renderer::new(document, &viewport, placement, side, *limits)?;
    let mut canvas = Canvas::new(side);
    // A square viewport fills the square, whose edge clips a slice already.
    let square = (width - height).abs() < f64::EPSILON * width.max(height);
    renderer.draw_root(&viewport, !square, &mut canvas)?;
    Ok(canvas)
}

struct Renderer<'a> {
    document: &'a Document,
    /// The map from the coordinates the SVG is placed in, its outermost
    /// viewport's before any `viewBox`, to pixels: where a non-scaling
    /// stroke is measured.
    host: Transform,
    cascade: Cascade,
    parents: Vec<Option<usize>>,
    /// Each id's last element. Ids are meant to be unique; where a file
    /// repeats one, the last wins, as in resvg (and in the icons that do
    /// it, where the last is the one the drawing means).
    ids: HashMap<&'a str, usize>,
    /// Styles computed in document position, for elements reached by
    /// reference (gradients, stops, clip paths, masks) and the root.
    styles: Vec<Option<Style>>,
    side: u32,
    /// The viewport in user units, which percentages are of.
    viewport: (f64, f64),
    rasterizer: Rasterizer,
    /// The elements whose references are being followed, innermost last.
    following: Vec<usize>,
    depth: usize,
    instances: usize,
    layers: usize,
    /// The caller's limits, which every embedded image decodes under.
    limits: Limits,
    /// How many `<image>` documents this one is inside.
    nesting: usize,
    /// The image budget spent so far, in this document and the ones it
    /// embeds, as `image`'s module docs describe.
    image_bytes: usize,
    /// Each `<image>` element's decoded image, decoded once however many
    /// times a `use` draws it.
    images: HashMap<usize, Rc<image::Embedded>>,
}

impl<'a> Renderer<'a> {
    /// A renderer of `document`, whose viewport is `viewport` and whose
    /// outermost viewport maps to pixels by `host`, into a `side`-square.
    fn new(
        document: &'a Document,
        viewport: &Viewport,
        host: Transform,
        side: u32,
        limits: Limits,
    ) -> Result<Self, ImageError> {
        let mut ids = HashMap::new();
        let mut parents = vec![None; document.elements.len()];
        for (index, element) in document.elements.iter().enumerate() {
            for &child in &element.children {
                parents[child] = Some(index);
            }
            if let Some(id) = element.attribute(Namespace::None, "id") {
                ids.insert(id, index);
            }
        }
        Ok(Renderer {
            document,
            host,
            cascade: Cascade::new(document)?,
            parents,
            ids,
            styles: vec![None; document.elements.len()],
            side,
            viewport: viewport.user,
            rasterizer: Rasterizer::default(),
            following: Vec::new(),
            depth: 0,
            instances: 0,
            layers: 0,
            limits,
            nesting: 0,
            image_bytes: 0,
            images: HashMap::new(),
        })
    }

    fn element(&self, index: usize) -> &'a Element {
        &self.document.elements[index]
    }

    /// Draws the whole document into `canvas` through its `viewport`, whose
    /// overflow a `slice` hides only where `clip_slice` says the canvas's
    /// edge does not already.
    fn draw_root(
        &mut self,
        viewport: &Viewport,
        clip_slice: bool,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let root_style = self.document_style(0)?;
        let transform = self.host.then(viewport.inner);
        if !(viewport.slice && clip_slice) {
            // Drawing that strays outside a viewport narrower than the square
            // shows in the margin, as it does where resvg draws an SVG as an
            // image: clipping to the viewport would also square the coverage
            // of every edge lying on it, and icons draw to their edges.
            return self.root(&root_style, transform, canvas);
        }
        // A sliced viewBox overflows its viewport by design, and the
        // overflow stays hidden.
        let mut layer = Canvas::new(self.side);
        self.root(&root_style, transform, &mut layer)?;
        let frame = path::rect(0.0, 0.0, viewport.width, viewport.height, 0.0, 0.0);
        let coverage = self.fill_coverage(&frame, self.host, FillRule::NonZero);
        canvas.composite(&layer, |index| coverage[index]);
        Ok(())
    }

    /// Draws the root `svg` element, which has a style and group effects of
    /// its own like any group.
    fn root(
        &mut self,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        if !style.display {
            return Ok(());
        }
        if !has_effects(style) {
            return self.content(0, style, transform, canvas);
        }
        let mut layer = Canvas::new(self.side);
        self.layers += 1;
        let result = self.effects(0, style, transform, canvas, &mut layer);
        self.layers -= 1;
        result
    }

    fn specified(&self, index: usize) -> Result<Specified, ImageError> {
        self.cascade.specified(self.element(index))
    }

    /// The style of the element at `index` where it stands in the document.
    fn document_style(&mut self, index: usize) -> Result<Style, ImageError> {
        if let Some(style) = &self.styles[index] {
            return Ok(style.clone());
        }
        let parent = match self.parents[index] {
            Some(parent) => self.document_style(parent)?,
            None => Style::initial(),
        };
        let style = Style::compute(&self.specified(index)?, &parent, &self.element(index).name)?;
        self.styles[index] = Some(style.clone());
        Ok(style)
    }

    /// The normalised diagonal percentages of a radius or a width are of.
    fn diagonal(&self) -> f64 {
        let (w, h) = self.viewport;
        ((w * w + h * h) / 2.0).sqrt()
    }

    /// A length attribute of `element`: `default` if absent.
    fn length(
        &self,
        element: &Element,
        name: &str,
        reference: f64,
        default: f64,
    ) -> Result<f64, ImageError> {
        match element.attribute(Namespace::None, name) {
            None => Ok(default),
            Some(text) => length(text)
                .map(|length| length.resolve(reference))
                .ok_or_else(|| unsupported(format_args!("{name}={text:?} on <{}>", element.name))),
        }
    }

    fn local_transform(element: &Element) -> Result<Transform, ImageError> {
        match element.attribute(Namespace::None, "transform") {
            None => Ok(Transform::IDENTITY),
            Some(text) => transform::parse(text).ok_or_else(|| {
                malformed(format_args!("the transform {text:?} on <{}>", element.name))
            }),
        }
    }

    /// Enters one level of the walk, refusing what is too deep.
    fn enter(&mut self) -> Result<(), ImageError> {
        self.depth += 1;
        if self.depth > MAX_RENDER_DEPTH {
            return Err(unsupported(format_args!(
                "nesting and references more than {MAX_RENDER_DEPTH} deep"
            )));
        }
        Ok(())
    }

    /// Follows a reference to `target`, refusing a cycle.
    fn follow(&mut self, target: usize) -> Result<(), ImageError> {
        if self.following.contains(&target) {
            return Err(malformed(format_args!(
                "<{}> references itself",
                self.element(target).name
            )));
        }
        self.following.push(target);
        Ok(())
    }

    /// Draws the element at `index`, a child of something styled `parent`,
    /// whose user space maps to pixels by `transform`.
    fn node(
        &mut self,
        index: usize,
        parent: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let element = self.element(index);
        if element.namespace != Namespace::Svg
            || NOT_RENDERED.contains(&element.name.as_str())
            || NOT_SVG.contains(&element.name.as_str())
        {
            return Ok(());
        }
        if !DRAWN.contains(&element.name.as_str()) {
            return Err(unsupported(format_args!("element <{}>", element.name)));
        }
        check_attributes(element)?;
        self.instances += 1;
        if self.instances > MAX_INSTANCES {
            return Err(unsupported(format_args!(
                "more than {MAX_INSTANCES} elements drawn"
            )));
        }
        self.enter()?;
        let result = self.styled_node(index, parent, transform, canvas);
        self.depth -= 1;
        result
    }

    fn styled_node(
        &mut self,
        index: usize,
        parent: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let element = self.element(index);
        let style = Style::compute(&self.specified(index)?, parent, &element.name)?;
        if !style.display {
            return Ok(());
        }
        let transform = transform.then(Self::local_transform(element)?);
        if transform.invert().is_none() {
            return Ok(());
        }
        if !has_effects(&style) {
            return self.content(index, &style, transform, canvas);
        }
        if style.opacity <= 0.0 {
            return Ok(());
        }
        self.layers += 1;
        if self.layers > MAX_LAYERS {
            return Err(unsupported(format_args!(
                "group effects nested more than {MAX_LAYERS} deep"
            )));
        }
        let mut layer = Canvas::new(self.side);
        let result = self.effects(index, &style, transform, canvas, &mut layer);
        self.layers -= 1;
        result
    }

    /// Draws an element with group effects through `layer`.
    fn effects(
        &mut self,
        index: usize,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
        layer: &mut Canvas,
    ) -> Result<(), ImageError> {
        let bbox = if style.clip_path.is_some() || style.mask.is_some() || style.filter.is_some() {
            self.object_box(index)?
        } else {
            None
        };
        let filter = match &style.filter {
            Some(id) => match self.filter_plan(id, transform, bbox)? {
                Some(filter) => Some(filter),
                // SVG 1.1: an element whose filter cannot apply is not
                // rendered.
                None => return Ok(()),
            },
            None => None,
        };
        self.content(index, style, transform, layer)?;
        if let Some((region, plan)) = filter {
            self.apply_filter(region, &plan, layer);
        }
        let clip = match &style.clip_path {
            Some(id) => Some(self.clip(id, transform, bbox)?),
            None => None,
        };
        let mask = match &style.mask {
            Some(id) => Some(self.mask(id, transform, bbox)?),
            None => None,
        };
        let opacity = style.opacity;
        canvas.composite(layer, |pixel| {
            opacity
                * clip.as_ref().map_or(1.0, |clip| clip[pixel])
                * mask.as_ref().map_or(1.0, |mask| mask[pixel])
        });
        Ok(())
    }

    /// Draws what an element is, styled already and in its own user space.
    fn content(
        &mut self,
        index: usize,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let element = self.element(index);
        match element.name.as_str() {
            "svg" | "g" => {
                for child in element.children.clone() {
                    self.node(child, style, transform, canvas)?;
                }
                Ok(())
            }
            "use" => {
                let Some(target) = self.use_target(index)? else {
                    return Ok(());
                };
                let element = self.element(index);
                let (w, h) = self.viewport;
                let offset = Transform::translate(
                    self.length(element, "x", w, 0.0)?,
                    self.length(element, "y", h, 0.0)?,
                );
                self.follow(target)?;
                self.enter()?;
                let result = self.node(target, style, transform.then(offset), canvas);
                self.depth -= 1;
                self.following.pop();
                result
            }
            "image" => self.image(index, style, transform, canvas),
            _ => self.shape(index, style, transform, canvas),
        }
    }

    /// The element a `use` draws, or `None` for a reference to nothing.
    fn use_target(&self, index: usize) -> Result<Option<usize>, ImageError> {
        let element = self.element(index);
        let Some(href) = element
            .attribute(Namespace::None, "href")
            .or_else(|| element.attribute(Namespace::Xlink, "href"))
        else {
            return Ok(None);
        };
        let Some(id) = style::iri(href) else {
            return Err(unsupported(format_args!(
                "a <use> of another file ({href})"
            )));
        };
        let Some(&target) = self.ids.get(id.as_str()) else {
            return Ok(None);
        };
        let target_element = self.element(target);
        if target_element.namespace == Namespace::Svg
            && matches!(target_element.name.as_str(), "symbol" | "svg")
        {
            return Err(unsupported(format_args!(
                "a <use> of <{}>",
                target_element.name
            )));
        }
        Ok(Some(target))
    }

    /// A basic shape's geometry in its user space, or `None` if it draws
    /// nothing (a zero size, no path data).
    fn geometry(&self, element: &Element) -> Result<Option<Path>, ImageError> {
        let (w, h) = self.viewport;
        let diagonal = self.diagonal();
        let path = match element.name.as_str() {
            "path" => element
                .attribute(Namespace::None, "d")
                .map(path::parse_data),
            "rect" => {
                let width = self.length(element, "width", w, 0.0)?;
                let height = self.length(element, "height", h, 0.0)?;
                if width <= 0.0 || height <= 0.0 {
                    return Ok(None);
                }
                let radius = |name: &str, reference| -> Result<Option<f64>, ImageError> {
                    Ok(match element.attribute(Namespace::None, name) {
                        None | Some("auto") => None,
                        Some(_) => {
                            Some(self.length(element, name, reference, 0.0)?).filter(|r| *r >= 0.0)
                        }
                    })
                };
                let (rx, ry) = match (radius("rx", w)?, radius("ry", h)?) {
                    (None, None) => (0.0, 0.0),
                    (Some(rx), None) => (rx, rx),
                    (None, Some(ry)) => (ry, ry),
                    (Some(rx), Some(ry)) => (rx, ry),
                };
                Some(path::rect(
                    self.length(element, "x", w, 0.0)?,
                    self.length(element, "y", h, 0.0)?,
                    width,
                    height,
                    rx.min(width / 2.0),
                    ry.min(height / 2.0),
                ))
            }
            "circle" => {
                let r = self.length(element, "r", diagonal, 0.0)?;
                (r > 0.0)
                    .then(|| -> Result<Path, ImageError> {
                        Ok(path::ellipse(
                            self.length(element, "cx", w, 0.0)?,
                            self.length(element, "cy", h, 0.0)?,
                            r,
                            r,
                        ))
                    })
                    .transpose()?
            }
            "ellipse" => {
                let radius = |name: &str, reference| -> Result<Option<f64>, ImageError> {
                    Ok(match element.attribute(Namespace::None, name) {
                        None | Some("auto") => None,
                        Some(_) => Some(self.length(element, name, reference, 0.0)?),
                    })
                };
                let (rx, ry) = match (radius("rx", w)?, radius("ry", h)?) {
                    (Some(rx), Some(ry)) => (rx, ry),
                    (Some(r), None) | (None, Some(r)) => (r, r),
                    (None, None) => return Ok(None),
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return Ok(None);
                }
                Some(path::ellipse(
                    self.length(element, "cx", w, 0.0)?,
                    self.length(element, "cy", h, 0.0)?,
                    rx,
                    ry,
                ))
            }
            "line" => Some(path::polyline(
                &[
                    (
                        self.length(element, "x1", w, 0.0)?,
                        self.length(element, "y1", h, 0.0)?,
                    ),
                    (
                        self.length(element, "x2", w, 0.0)?,
                        self.length(element, "y2", h, 0.0)?,
                    ),
                ],
                false,
            )),
            "polyline" | "polygon" => {
                let points =
                    path::points(element.attribute(Namespace::None, "points").unwrap_or(""));
                (points.len() >= 2).then(|| path::polyline(&points, element.name == "polygon"))
            }
            other => return Err(unsupported(format_args!("element <{other}>"))),
        };
        Ok(path.filter(|path| !path.segments.is_empty()))
    }

    /// Refuses a child of a shape that would change it: only descriptions
    /// and foreign elements may be there.
    fn check_shape_children(&self, element: &Element) -> Result<(), ImageError> {
        for &child in &element.children {
            let child = self.element(child);
            if child.namespace == Namespace::Svg && !DESCRIPTIVE.contains(&child.name.as_str()) {
                return Err(unsupported(format_args!(
                    "element <{}> inside <{}>",
                    child.name, element.name
                )));
            }
        }
        Ok(())
    }

    fn shape(
        &mut self,
        index: usize,
        style: &Style,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let element = self.element(index);
        self.check_shape_children(element)?;
        let Some(geometry) = self.geometry(element)? else {
            return Ok(());
        };
        if !style.visible {
            return Ok(());
        }
        let bbox = path::bounding_box(&geometry);
        let order: [bool; 2] = if style.stroke_first {
            [true, false]
        } else {
            [false, true]
        };
        for stroke in order {
            if stroke {
                self.stroke(&geometry, style, bbox, transform, canvas)?;
            } else if let Some(paint) =
                self.paint(&style.fill, style.fill_opacity, style, bbox, transform)?
            {
                let coverage = self.fill_coverage(&geometry, transform, style.fill_rule);
                self.apply_paint(&paint, &coverage, canvas);
            }
        }
        Ok(())
    }

    fn stroke(
        &mut self,
        geometry: &Path,
        style: &Style,
        bbox: Option<Rect>,
        transform: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), ImageError> {
        let diagonal = self.diagonal();
        let width = style.stroke_width.resolve(diagonal);
        if width <= 0.0 {
            return Ok(());
        }
        let Some(paint) =
            self.paint(&style.stroke, style.stroke_opacity, style, bbox, transform)?
        else {
            return Ok(());
        };
        let mut dashes: Vec<f64> = style
            .stroke_dasharray
            .iter()
            .map(|length| length.resolve(diagonal))
            .collect();
        // A negative dash is an error that leaves the stroke solid, and so is
        // a pattern with no length.
        if dashes.iter().any(|dash| *dash < 0.0) || dashes.iter().sum::<f64>() <= 0.0 {
            dashes.clear();
        }
        if dashes.len() % 2 == 1 {
            dashes.extend_from_within(..);
        }
        let stroke = Stroke {
            width,
            cap: style.stroke_linecap,
            join: style.stroke_linejoin,
            miter_limit: style.stroke_miterlimit,
            dashes,
            dash_offset: style.stroke_dashoffset.resolve(diagonal),
        };
        let tolerance = path::user_tolerance(transform.max_scale());
        let mut polylines = path::flatten(geometry, tolerance);
        // A non-scaling stroke is outlined in the host's coordinates, where
        // its width and dashes are measured, and only the placement maps it
        // to pixels.
        let (outline_tolerance, to_pixels) = if style.non_scaling_stroke {
            let Some(to_host) = self.host.invert().map(|inverse| inverse.then(transform)) else {
                return Ok(());
            };
            for polyline in &mut polylines {
                for (point, _) in &mut polyline.points {
                    *point = to_host.apply(*point);
                }
            }
            (path::user_tolerance(self.host.max_scale()), self.host)
        } else {
            (tolerance, transform)
        };
        let contours = stroke::outline(&polylines, &stroke, outline_tolerance)?;
        let mut edges = Vec::new();
        for contour in &contours {
            for (index, &from) in contour.iter().enumerate() {
                let to = contour[(index + 1) % contour.len()];
                edges.push(Edge::Line(device(to_pixels, from), device(to_pixels, to)));
            }
        }
        let coverage = self.coverage(&edges, FillRule::NonZero);
        self.apply_paint(&paint, &coverage, canvas);
        Ok(())
    }

    fn apply_paint(&self, paint: &paint::Paint, coverage: &[f32], canvas: &mut Canvas) {
        let side = self.side as usize;
        canvas.paint(coverage, |index| {
            paint.at((index % side) as u32, (index / side) as u32)
        });
    }

    /// Each pixel's coverage by `geometry` filled by `rule`, every open
    /// subpath closed as filling does.
    fn fill_coverage(&mut self, geometry: &Path, transform: Transform, rule: FillRule) -> Vec<f32> {
        let at = |point| device(transform, point);
        let mut edges = Vec::with_capacity(geometry.segments.len() + 1);
        let (mut start, mut current) = ((0.0, 0.0), (0.0, 0.0));
        for segment in &geometry.segments {
            match *segment {
                Segment::MoveTo(to) => {
                    edges.push(Edge::Line(at(current), at(start)));
                    start = to;
                    current = to;
                }
                Segment::LineTo(to) => {
                    edges.push(Edge::Line(at(current), at(to)));
                    current = to;
                }
                Segment::QuadTo(control, to) => {
                    edges.push(Edge::Quadratic(at(current), at(control), at(to)));
                    current = to;
                }
                Segment::CubicTo(c1, c2, to) => {
                    edges.push(Edge::Cubic(at(current), at(c1), at(c2), at(to)));
                    current = to;
                }
                Segment::Close => {
                    edges.push(Edge::Line(at(current), at(start)));
                    current = start;
                }
            }
        }
        edges.push(Edge::Line(at(current), at(start)));
        self.coverage(&edges, rule)
    }

    /// Each pixel's coverage by closed outlines made of `edges`, in
    /// subsample units, under `rule`: accumulated a strip of
    /// [`STRIP_ROWS`] rows at a time on a grid [`SUBSAMPLES`] times finer
    /// than the pixels, the rule applied to each sub-cell, and the sub-cells
    /// averaged.
    fn coverage(&mut self, edges: &[Edge], rule: FillRule) -> Vec<f32> {
        let side = self.side as usize;
        let s = SUBSAMPLES as usize;
        let mut out = vec![0.0f32; side * side];
        let per_pixel = 1.0 / (s * s) as f32;
        for top in (0..side).step_by(STRIP_ROWS) {
            let rows = STRIP_ROWS.min(side - top);
            let top_y = (top * s) as f32;
            let bottom_y = ((top + rows) * s) as f32;
            self.rasterizer
                .reset(self.side * SUBSAMPLES, (rows * s) as u32);
            let shift = Point::new(0.0, top_y);
            for edge in edges {
                let (low, high) = edge.vertical_extent();
                if high <= top_y || low >= bottom_y {
                    continue;
                }
                match *edge {
                    Edge::Line(a, b) => self.rasterizer.line(a - shift, b - shift),
                    Edge::Quadratic(a, b, c) => {
                        self.rasterizer.quadratic(a - shift, b - shift, c - shift)
                    }
                    Edge::Cubic(a, b, c, d) => {
                        self.rasterizer
                            .cubic(a - shift, b - shift, c - shift, d - shift)
                    }
                }
            }
            let cells = self.rasterizer.coverage_by(rule);
            let width = side * s;
            for (sub_row, row) in cells.chunks_exact(width).enumerate() {
                let pixels = &mut out[(top + sub_row / s) * side..][..side];
                for (pixel, block) in pixels.iter_mut().zip(row.chunks_exact(s)) {
                    *pixel += block.iter().sum::<f32>() * per_pixel;
                }
            }
        }
        out
    }

    /// What a `fill` or `stroke` paints with, or `None` for nothing.
    fn paint(
        &mut self,
        value: &Paint,
        opacity: f32,
        style: &Style,
        bbox: Option<Rect>,
        transform: Transform,
    ) -> Result<Option<paint::Paint>, ImageError> {
        match value {
            Paint::None => Ok(None),
            Paint::Color(color) => {
                let color = style.resolve(*color).with_opacity(opacity);
                Ok((color.a > 0.0)
                    .then_some(paint::Paint::Solid([color.r, color.g, color.b, color.a])))
            }
            Paint::Server { id, fallback } => {
                let Some(&target) = self.ids.get(id.as_str()) else {
                    return match fallback {
                        Some(fallback) => self.paint(fallback, opacity, style, bbox, transform),
                        None => Ok(None),
                    };
                };
                let element = self.element(target);
                let name = if element.namespace == Namespace::Svg {
                    element.name.as_str()
                } else {
                    ""
                };
                match name {
                    "linearGradient" | "radialGradient" => {
                        self.gradient(target, opacity, bbox, transform)
                    }
                    "pattern" | "solidColor" | "solidcolor" | "meshgradient" | "hatch" => {
                        Err(unsupported(format_args!("paint server <{name}>")))
                    }
                    // Not a paint server at all: SVG 2 paints the fallback,
                    // or nothing.
                    _ => match fallback {
                        Some(fallback) => self.paint(fallback, opacity, style, bbox, transform),
                        None => Ok(None),
                    },
                }
            }
        }
    }

    /// A gradient's paint, its attributes and stops gathered through its
    /// `href` chain.
    fn gradient(
        &mut self,
        index: usize,
        opacity: f32,
        bbox: Option<Rect>,
        transform: Transform,
    ) -> Result<Option<paint::Paint>, ImageError> {
        let mut chain = vec![index];
        loop {
            let element = self.element(*chain.last().unwrap_or(&index));
            check_attributes(element)?;
            for &child in &element.children {
                let child = self.element(child);
                if child.namespace == Namespace::Svg
                    && child.name != "stop"
                    && !DESCRIPTIVE.contains(&child.name.as_str())
                {
                    return Err(unsupported(format_args!(
                        "element <{}> inside a gradient",
                        child.name
                    )));
                }
            }
            let href = element
                .attribute(Namespace::None, "href")
                .or_else(|| element.attribute(Namespace::Xlink, "href"));
            let Some(next) = href
                .and_then(style::iri)
                .and_then(|id| self.ids.get(id.as_str()).copied())
            else {
                break;
            };
            let next_element = self.element(next);
            if next_element.namespace != Namespace::Svg
                || !matches!(
                    next_element.name.as_str(),
                    "linearGradient" | "radialGradient"
                )
            {
                break;
            }
            if chain.contains(&next) {
                return Err(malformed("a gradient's href chain loops"));
            }
            chain.push(next);
        }
        let first = self.element(index);
        let document = self.document;
        let attribute = |name: &str, same_kind: bool| {
            chain.iter().find_map(|&link| {
                let element = &document.elements[link];
                if same_kind && element.name != first.name {
                    return None;
                }
                element.attribute(Namespace::None, name)
            })
        };
        let object_units = match attribute("gradientUnits", false) {
            None | Some("objectBoundingBox") => true,
            Some("userSpaceOnUse") => false,
            Some(other) => return Err(unsupported(format_args!("gradientUnits {other:?}"))),
        };
        let spread = match attribute("spreadMethod", false) {
            None | Some("pad") => Spread::Pad,
            Some("reflect") => Spread::Reflect,
            Some("repeat") => Spread::Repeat,
            Some(other) => return Err(unsupported(format_args!("spreadMethod {other:?}"))),
        };
        let gradient_transform = match attribute("gradientTransform", false) {
            None => Transform::IDENTITY,
            Some(text) => transform::parse(text)
                .ok_or_else(|| malformed(format_args!("the gradientTransform {text:?}")))?,
        };
        let (w, h) = self.viewport;
        let diagonal = self.diagonal();
        // A coordinate: in bounding-box units a number is a fraction of the
        // box and a percentage a hundredth of one; in user space a percentage
        // is of the viewport.
        let coordinate = |name: &str, reference: f64, default: &str| -> Result<f64, ImageError> {
            let text = attribute(name, true).unwrap_or(default);
            let value = length(text)
                .ok_or_else(|| unsupported(format_args!("{name}={text:?} on a gradient")))?;
            Ok(match (value, object_units) {
                (Length::Percent(percent), true) => percent / 100.0,
                (Length::Px(value), true) => value,
                (value, false) => value.resolve(reference),
            })
        };
        let stops_owner = chain.iter().copied().find(|&link| {
            self.element(link)
                .children
                .iter()
                .any(|&child| self.element(child).is("stop"))
        });
        let mut stops = Vec::new();
        if let Some(owner) = stops_owner {
            for child in self.element(owner).children.clone() {
                if !self.element(child).is("stop") {
                    continue;
                }
                check_attributes(self.element(child))?;
                let stop_style = self.document_style(child)?;
                let text = self
                    .element(child)
                    .attribute(Namespace::None, "offset")
                    .unwrap_or("0");
                let offset = style::number_or_percent(text)
                    .ok_or_else(|| malformed(format_args!("the stop offset {text:?}")))?
                    .clamp(0.0, 1.0);
                let previous = stops.last().map_or(0.0, |stop: &Stop| stop.offset);
                let color = stop_style.resolve(stop_style.stop_color);
                stops.push(Stop {
                    offset: offset.max(previous),
                    color: [color.r, color.g, color.b, color.a * stop_style.stop_opacity],
                });
            }
        }
        let Some(last) = stops.last().copied() else {
            return Ok(None);
        };
        let solid = |color: [f32; 4]| {
            Some(paint::Paint::Solid([
                color[0],
                color[1],
                color[2],
                color[3] * opacity,
            ]))
        };
        if stops.len() == 1 {
            return Ok(solid(last.color));
        }
        let units = if object_units {
            match bounding_box_units(bbox) {
                Some(units) => units,
                None => return Ok(None),
            }
        } else {
            Transform::IDENTITY
        };
        let shape = if first.name == "linearGradient" {
            let (x1, y1) = (coordinate("x1", w, "0%")?, coordinate("y1", h, "0%")?);
            let (x2, y2) = (coordinate("x2", w, "100%")?, coordinate("y2", h, "0%")?);
            if x1 == x2 && y1 == y2 {
                return Ok(solid(last.color));
            }
            Shape::Linear { x1, y1, x2, y2 }
        } else {
            let (cx, cy) = (coordinate("cx", w, "50%")?, coordinate("cy", h, "50%")?);
            let r = coordinate("r", diagonal, "50%")?;
            let fr = coordinate("fr", diagonal, "0%")?;
            if r < 0.0 || fr < 0.0 {
                return Err(malformed("a negative gradient radius"));
            }
            if r == 0.0 {
                return Ok(solid(last.color));
            }
            let fx = if attribute("fx", true).is_some() {
                coordinate("fx", w, "0")?
            } else {
                cx
            };
            let fy = if attribute("fy", true).is_some() {
                coordinate("fy", h, "0")?
            } else {
                cy
            };
            Shape::Radial {
                cx,
                cy,
                r,
                fx,
                fy,
                fr,
            }
        };
        let Some(inverse) = transform.then(units).then(gradient_transform).invert() else {
            return Ok(None);
        };
        Ok(Some(paint::Paint::Gradient(Gradient {
            shape,
            spread,
            stops,
            inverse,
            opacity,
        })))
    }

    /// The object bounding box of the element at `index` in its own user
    /// space (its transform applied): a shape's geometry, a group's
    /// children's boxes joined.
    fn object_box(&mut self, index: usize) -> Result<Option<Rect>, ImageError> {
        self.enter()?;
        let result = self.object_box_inner(index);
        self.depth -= 1;
        result
    }

    fn object_box_inner(&mut self, index: usize) -> Result<Option<Rect>, ImageError> {
        let element = self.element(index);
        if element.namespace != Namespace::Svg {
            return Ok(None);
        }
        match element.name.as_str() {
            "g" | "svg" => {
                let mut total = None;
                for child in element.children.clone() {
                    let child_element = self.element(child);
                    if child_element.namespace != Namespace::Svg
                        || NOT_RENDERED.contains(&child_element.name.as_str())
                    {
                        continue;
                    }
                    let local = Self::local_transform(child_element)?;
                    let child_box = self
                        .object_box(child)?
                        .map(|rect| transform_box(rect, local));
                    total = union(total, child_box);
                }
                Ok(total)
            }
            "use" => {
                let Some(target) = self.use_target(index)? else {
                    return Ok(None);
                };
                let element = self.element(index);
                let (w, h) = self.viewport;
                let offset = Transform::translate(
                    self.length(element, "x", w, 0.0)?,
                    self.length(element, "y", h, 0.0)?,
                )
                .then(Self::local_transform(self.element(target))?);
                self.follow(target)?;
                let result = self.object_box(target);
                self.following.pop();
                Ok(result?.map(|rect| transform_box(rect, offset)))
            }
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => Ok(self
                .geometry(element)?
                .as_ref()
                .and_then(path::bounding_box)),
            "image" => Ok(self.image_layout(index)?.map(|(_, layout)| layout.viewport)),
            _ => Ok(None),
        }
    }

    /// Each pixel's coverage by the clip path `id`, applied to an element
    /// whose user space maps to pixels by `transform`.
    fn clip(
        &mut self,
        id: &str,
        transform: Transform,
        bbox: Option<Rect>,
    ) -> Result<Vec<f32>, ImageError> {
        let pixels = self.side as usize * self.side as usize;
        let Some(&index) = self.ids.get(id) else {
            return Ok(vec![1.0; pixels]);
        };
        let element = self.element(index);
        if !element.is("clipPath") {
            return Err(unsupported(format_args!(
                "a clip-path of <{}>",
                element.name
            )));
        }
        check_attributes(element)?;
        self.follow(index)?;
        self.enter()?;
        self.layers += 1;
        let result = if self.layers > MAX_LAYERS {
            Err(unsupported(format_args!(
                "group effects nested more than {MAX_LAYERS} deep"
            )))
        } else {
            self.clip_inner(index, transform, bbox, pixels)
        };
        self.layers -= 1;
        self.depth -= 1;
        self.following.pop();
        result
    }

    fn clip_inner(
        &mut self,
        index: usize,
        transform: Transform,
        bbox: Option<Rect>,
        pixels: usize,
    ) -> Result<Vec<f32>, ImageError> {
        let element = self.element(index);
        let style = self.document_style(index)?;
        let mut local = transform.then(Self::local_transform(element)?);
        match element.attribute(Namespace::None, "clipPathUnits") {
            None | Some("userSpaceOnUse") => {}
            Some("objectBoundingBox") => match bounding_box_units(bbox) {
                Some(units) => local = local.then(units),
                None => return Ok(vec![0.0; pixels]),
            },
            Some(other) => return Err(unsupported(format_args!("clipPathUnits {other:?}"))),
        }
        let mut coverage = vec![0.0f32; pixels];
        for child in element.children.clone() {
            let child_element = self.element(child);
            if child_element.namespace != Namespace::Svg
                || DESCRIPTIVE.contains(&child_element.name.as_str())
            {
                continue;
            }
            // A clip path's children are shapes, or `use` elements that
            // reference shapes directly.
            let (shape, shape_style, shape_transform) = match child_element.name.as_str() {
                "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                    self.check_clip_child(child)?;
                    let child_style =
                        Style::compute(&self.specified(child)?, &style, &child_element.name)?;
                    (
                        child,
                        child_style,
                        local.then(Self::local_transform(child_element)?),
                    )
                }
                "use" => {
                    self.check_clip_child(child)?;
                    let use_style = Style::compute(&self.specified(child)?, &style, "use")?;
                    let Some(target) = self.use_target(child)? else {
                        continue;
                    };
                    let target_element = self.element(target);
                    if !matches!(
                        target_element.name.as_str(),
                        "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon"
                    ) {
                        return Err(unsupported(format_args!(
                            "a clip path's <use> of <{}>",
                            target_element.name
                        )));
                    }
                    self.check_clip_child(target)?;
                    if use_style.clip_path.is_some() {
                        return Err(unsupported("a clip-path inside a clip path"));
                    }
                    if !use_style.display {
                        continue;
                    }
                    let (w, h) = self.viewport;
                    let child_element = self.element(child);
                    let offset = Transform::translate(
                        self.length(child_element, "x", w, 0.0)?,
                        self.length(child_element, "y", h, 0.0)?,
                    );
                    let target_style =
                        Style::compute(&self.specified(target)?, &use_style, &target_element.name)?;
                    let shape_transform = local
                        .then(Self::local_transform(self.element(child))?)
                        .then(offset)
                        .then(Self::local_transform(target_element)?);
                    (target, target_style, shape_transform)
                }
                other => {
                    return Err(unsupported(format_args!(
                        "element <{other}> inside a clip path"
                    )));
                }
            };
            if shape_style.clip_path.is_some() {
                return Err(unsupported("a clip-path inside a clip path"));
            }
            if !shape_style.display || !shape_style.visible {
                continue;
            }
            let Some(geometry) = self.geometry(self.element(shape))? else {
                continue;
            };
            let piece = self.fill_coverage(&geometry, shape_transform, shape_style.clip_rule);
            for (total, piece) in coverage.iter_mut().zip(piece) {
                *total += piece - *total * piece;
            }
        }
        if let Some(other) = &style.clip_path {
            let other = self.clip(other, transform, bbox)?;
            for (total, other) in coverage.iter_mut().zip(other) {
                *total *= other;
            }
        }
        if style.mask.is_some() {
            return Err(unsupported("a mask on a clip path"));
        }
        Ok(coverage)
    }

    /// Checks a shape (or `use`) inside a clip path: its attributes and its
    /// children.
    fn check_clip_child(&self, index: usize) -> Result<(), ImageError> {
        let element = self.element(index);
        check_attributes(element)?;
        if element.name != "use" {
            self.check_shape_children(element)?;
        }
        Ok(())
    }

    /// The region of a `mask` or `filter` element applied to an element whose
    /// object bounding box is `bbox`, in that element's user space: its `x`,
    /// `y`, `width` and `height`, fractions of the box in bounding-box units
    /// and lengths otherwise, by default the box (or the viewport) grown by a
    /// tenth each way. `None` in bounding-box units for an element with no
    /// box.
    fn effect_region(
        &self,
        element: &Element,
        object_units: bool,
        bbox: Option<Rect>,
    ) -> Result<Option<Rect>, ImageError> {
        if object_units {
            let Some(units) = bounding_box_units(bbox) else {
                return Ok(None);
            };
            let rect = Rect {
                x: fraction(element, "x", -0.1)?,
                y: fraction(element, "y", -0.1)?,
                width: fraction(element, "width", 1.2)?,
                height: fraction(element, "height", 1.2)?,
            };
            return Ok(Some(map_rect(rect, units)));
        }
        let (w, h) = self.viewport;
        Ok(Some(Rect {
            x: self.length(element, "x", w, -0.1 * w)?,
            y: self.length(element, "y", h, -0.1 * h)?,
            width: self.length(element, "width", w, 1.2 * w)?,
            height: self.length(element, "height", h, 1.2 * h)?,
        }))
    }

    /// Each pixel's value of the mask `id`, applied to an element whose user
    /// space maps to pixels by `transform`.
    fn mask(
        &mut self,
        id: &str,
        transform: Transform,
        bbox: Option<Rect>,
    ) -> Result<Vec<f32>, ImageError> {
        let pixels = self.side as usize * self.side as usize;
        let Some(&index) = self.ids.get(id) else {
            return Ok(vec![1.0; pixels]);
        };
        let element = self.element(index);
        if !element.is("mask") {
            return Err(unsupported(format_args!("a mask of <{}>", element.name)));
        }
        check_attributes(element)?;
        self.follow(index)?;
        self.enter()?;
        self.layers += 1;
        let result = if self.layers > MAX_LAYERS {
            Err(unsupported(format_args!(
                "group effects nested more than {MAX_LAYERS} deep"
            )))
        } else {
            self.mask_inner(index, transform, bbox, pixels)
        };
        self.layers -= 1;
        self.depth -= 1;
        self.following.pop();
        result
    }

    fn mask_inner(
        &mut self,
        index: usize,
        transform: Transform,
        bbox: Option<Rect>,
        pixels: usize,
    ) -> Result<Vec<f32>, ImageError> {
        let element = self.element(index);
        let style = self.document_style(index)?;
        if style.clip_path.is_some() {
            return Err(unsupported("a clip-path on a mask"));
        }
        let object_units = units(element, "maskUnits", true)?;
        let object_content = units(element, "maskContentUnits", false)?;
        let bbox_units = bounding_box_units(bbox);
        let Some(rect) = self.effect_region(element, object_units, bbox)? else {
            return Ok(vec![0.0; pixels]);
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Ok(vec![0.0; pixels]);
        }
        let content_transform = if object_content {
            let Some(bbox_units) = bbox_units else {
                return Ok(vec![0.0; pixels]);
            };
            transform.then(bbox_units)
        } else {
            transform
        };
        let frame = path::rect(rect.x, rect.y, rect.width, rect.height, 0.0, 0.0);
        let region_coverage = self.fill_coverage(&frame, transform, FillRule::NonZero);
        let mut layer = Canvas::new(self.side);
        for child in self.element(index).children.clone() {
            self.node(child, &style, content_transform, &mut layer)?;
        }
        let mut values: Vec<f32> = (0..pixels)
            .map(|pixel| {
                let value = if style.mask_alpha {
                    layer.alpha(pixel)
                } else {
                    layer.luminance(pixel)
                };
                value * region_coverage[pixel]
            })
            .collect();
        if let Some(other) = &style.mask {
            let other = self.mask(other, transform, bbox)?;
            for (value, other) in values.iter_mut().zip(other) {
                *value *= other;
            }
        }
        Ok(values)
    }
}

/// A user-space point in subsample units: pixels times [`SUBSAMPLES`].
fn device(transform: Transform, point: (f64, f64)) -> Point {
    let (x, y) = transform.apply(point);
    let scale = f64::from(SUBSAMPLES);
    Point::new((x * scale) as f32, (y * scale) as f32)
}

/// One outline element in subsample units.
enum Edge {
    Line(Point, Point),
    Quadratic(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

impl Edge {
    /// The lowest and highest `y` of its control points, which bound it.
    fn vertical_extent(&self) -> (f32, f32) {
        let points: &[Point] = match self {
            Self::Line(a, b) => &[*a, *b],
            Self::Quadratic(a, b, c) => &[*a, *b, *c],
            Self::Cubic(a, b, c, d) => &[*a, *b, *c, *d],
        };
        points
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), point| {
                (low.min(point.y), high.max(point.y))
            })
    }
}
