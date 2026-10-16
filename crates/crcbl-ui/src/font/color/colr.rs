//! COLR glyphs: v0 layer lists and v1 paint graphs, painted with this
//! crate's coverage rasteriser into an RGBA picture.
//!
//! # How a glyph is painted
//!
//! `skrifa` walks the glyph's layers or paint graph and calls a
//! [`ColorPainter`] for every transform, clip, fill and layer. It is walked
//! twice. The first walk only measures: the union of every fill's clip, in
//! pixels, which is the picture's extent. The second paints into a canvas of
//! that extent:
//!
//! * **A clip** is a coverage mask: a glyph's outline, or a clip box, drawn
//!   through the current transform by [`Rasterizer`] under the
//!   non-zero rule, and multiplied into the clip it is pushed inside.
//! * **A fill** paints the current clip's coverage with a solid colour or a
//!   linear, radial or sweep gradient, over what is beneath it.
//! * **A layer** is a canvas of its own, composited onto the one beneath when
//!   it is popped by its composite mode: the Porter-Duff operators, the
//!   separable blend modes and the HSL ones, by the W3C Compositing and
//!   Blending formulas.
//!
//! Colours are composited premultiplied, in sRGB-encoded values as a browser
//! composites them, and stored straight. A gradient's stops are interpolated
//! premultiplied. A palette entry is from the font's first `CPAL` palette;
//! the foreground entry, `0xFFFF`, is opaque black, since the picture is
//! cached without the text colour.
//!
//! # Limits
//!
//! A paint graph is a font's data, trusted less than code. `skrifa` already
//! stops at a cycle and at a depth and node count of its own; within those,
//! a glyph whose transforms, clips and layers nest deeper than
//! [`MAX_PAINT_DEPTH`], that fills more than [`MAX_COLR_LAYERS`] times,
//! whose open clips and layers would hold more than [`MAX_HELD_PIXELS`] or
//! whose fills would touch more than [`MAX_FILLED_PIXELS`] is not drawn in
//! colour at all: its run draws its outline instead.

use skrifa::color::{Brush, ColorPainter, ColorStop, CompositeMode, Extend, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::BoundingBox;
use skrifa::{FontRef, MetadataProvider};

use super::{ColorBitmap, MAX_COLOR_GLYPH_SIDE};
use crate::font::raster::{FillRule, Point, Rasterizer};

/// How deep a COLR glyph's transforms, clips and layers may nest, together.
pub const MAX_PAINT_DEPTH: usize = 32;

/// How many fills — v0 layers, v1 solid and gradient paints — one COLR glyph
/// may make.
pub const MAX_COLR_LAYERS: usize = 4096;

/// The most canvas pixels the clips and layers open at once may hold
/// together.
pub const MAX_HELD_PIXELS: usize = 1 << 20;

/// The most pixels one glyph's fills may touch between them.
pub const MAX_FILLED_PIXELS: usize = 1 << 26;

/// The nesting and fill counts a glyph is painted under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaintLimits {
    /// The deepest transforms, clips and layers may nest together.
    pub depth: usize,
    /// The most fills.
    pub fills: usize,
}

impl PaintLimits {
    /// [`MAX_PAINT_DEPTH`] and [`MAX_COLR_LAYERS`].
    pub const DEFAULT: Self = Self {
        depth: MAX_PAINT_DEPTH,
        fills: MAX_COLR_LAYERS,
    };
}

/// The CPAL entry that means the text's own colour.
const FOREGROUND: u16 = 0xFFFF;

/// `glyph`'s COLR picture at `size` pixels per em, or `None` when it has
/// none, it paints nothing, or it breaks `limits` — see the module docs.
pub(crate) fn rasterize(
    font: &FontRef<'_>,
    glyph: skrifa::GlyphId,
    size: f32,
    units_per_em: u16,
    limits: PaintLimits,
) -> Option<ColorBitmap> {
    let color = font.color_glyphs().get(glyph)?;
    let scale = size / f32::from(units_per_em);
    // Font units, y up, to pixels from the pen and baseline, y down.
    let to_pixels = Transform::from_elements([scale, 0.0, 0.0, -scale, 0.0, 0.0]);

    let mut measure = Measure {
        font,
        transforms: vec![to_pixels],
        clips: Vec::new(),
        extent: None,
        nesting: Nesting::new(limits),
    };
    color.paint(LocationRef::default(), &mut measure).ok()?;
    if measure.nesting.broken {
        return None;
    }
    let extent = measure.extent?;
    let (left, top) = (extent.min.x.floor(), extent.min.y.floor());
    let (width, height) = (extent.max.x.ceil() - left, extent.max.y.ceil() - top);
    let side = MAX_COLOR_GLYPH_SIDE as f32;
    if !(width >= 1.0 && height >= 1.0 && width <= side && height <= side) {
        return None;
    }
    let (width, height) = (width as usize, height as usize);

    let palette = font.color_palettes();
    let colors = palette.get(0);
    let shift = Transform::from_elements([1.0, 0.0, 0.0, 1.0, -left, -top]);
    let mut painter = Painter {
        font,
        palette: colors.as_ref().map_or(&[], |palette| palette.colors()),
        width,
        height,
        transforms: vec![shift * to_pixels],
        clips: Vec::new(),
        layers: vec![Layer {
            pixels: vec![[0.0; 4]; width * height],
            mode: CompositeMode::SrcOver,
        }],
        nesting: Nesting::new(limits),
        filled: 0,
        rasterizer: Rasterizer::default(),
    };
    color.paint(LocationRef::default(), &mut painter).ok()?;
    if painter.nesting.broken {
        return None;
    }
    let pixels = painter.layers[0]
        .pixels
        .iter()
        .flat_map(|&pixel| straight(pixel))
        .collect();
    Some(ColorBitmap {
        width: width as u32,
        height: height as u32,
        left: left as i32,
        top: top as i32,
        pixels,
    })
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    min: Point,
    max: Point,
}

impl Rect {
    /// The smallest rectangle holding `points`, or `None` for none or one
    /// that is not finite.
    fn around(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut rect: Option<Self> = None;
        for point in points {
            if !point.is_finite() {
                return None;
            }
            rect = Some(match rect {
                None => Self {
                    min: point,
                    max: point,
                },
                Some(rect) => Self {
                    min: Point::new(rect.min.x.min(point.x), rect.min.y.min(point.y)),
                    max: Point::new(rect.max.x.max(point.x), rect.max.y.max(point.y)),
                },
            });
        }
        rect
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: Point::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Point::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    fn intersect(self, other: Self) -> Self {
        let min = Point::new(self.min.x.max(other.min.x), self.min.y.max(other.min.y));
        let max = Point::new(self.max.x.min(other.max.x), self.max.y.min(other.max.y));
        Self {
            min,
            max: Point::new(max.x.max(min.x), max.y.max(min.y)),
        }
    }

    fn is_empty(self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }
}

/// How deep the paint has nested, and whether it broke its limits.
struct Nesting {
    limits: PaintLimits,
    depth: usize,
    fills: usize,
    broken: bool,
}

impl Nesting {
    const fn new(limits: PaintLimits) -> Self {
        Self {
            limits,
            depth: 0,
            fills: 0,
            broken: false,
        }
    }

    /// One level deeper; false, and broken from now on, past the limit.
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > self.limits.depth {
            self.broken = true;
        }
        !self.broken
    }

    /// One level back out.
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// One more fill; false, and broken from now on, past the limit.
    fn fill(&mut self) -> bool {
        self.fills += 1;
        if self.fills > self.limits.fills {
            self.broken = true;
        }
        !self.broken
    }
}

/// `glyph`'s outline in font units, mapped by `transform`, as closed
/// polygons of line and curve segments: what a clip is drawn from.
fn outline(font: &FontRef<'_>, glyph: skrifa::GlyphId, transform: Transform) -> Vec<Segment> {
    let mut pen = Collect {
        transform,
        segments: Vec::new(),
        start: Point::ZERO,
        current: Point::ZERO,
    };
    if let Some(outline) = font.outline_glyphs().get(glyph) {
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        if outline.draw(settings, &mut pen).is_err() {
            return Vec::new();
        }
        pen.close();
    }
    pen.segments
}

/// A clip box's four corners mapped by `transform`, as a closed polygon.
fn clip_box(rect: BoundingBox<f32>, transform: Transform) -> Vec<Segment> {
    let at = |x: f32, y: f32| {
        let (x, y) = transform.transform(x, y);
        Point::new(x, y)
    };
    let corners = [
        at(rect.x_min, rect.y_min),
        at(rect.x_max, rect.y_min),
        at(rect.x_max, rect.y_max),
        at(rect.x_min, rect.y_max),
    ];
    (0..corners.len())
        .map(|at| Segment::Line(corners[at], corners[(at + 1) % corners.len()]))
        .collect()
}

/// One outline element, in pixels.
#[derive(Clone, Copy, Debug)]
enum Segment {
    Line(Point, Point),
    Quadratic(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

impl Segment {
    /// Its points, the controls among them: a hull holding the curve.
    fn points(&self) -> impl Iterator<Item = Point> {
        let (array, count) = match *self {
            Self::Line(a, b) => ([a, b, b, b], 2),
            Self::Quadratic(a, b, c) => ([a, b, c, c], 3),
            Self::Cubic(a, b, c, d) => ([a, b, c, d], 4),
        };
        array.into_iter().take(count)
    }
}

/// The smallest rectangle holding `segments`.
fn bounds(segments: &[Segment]) -> Option<Rect> {
    Rect::around(segments.iter().flat_map(Segment::points))
}

/// Collects an outline through a transform, closing every contour.
struct Collect {
    transform: Transform,
    segments: Vec<Segment>,
    start: Point,
    current: Point,
}

impl Collect {
    fn point(&self, x: f32, y: f32) -> Point {
        let (x, y) = self.transform.transform(x, y);
        Point::new(x, y)
    }
}

impl OutlinePen for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
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
        let control = self.point(cx0, cy0);
        self.segments
            .push(Segment::Quadratic(self.current, control, to));
        self.current = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        let (c0, c1) = (self.point(cx0, cy0), self.point(cx1, cy1));
        self.segments.push(Segment::Cubic(self.current, c0, c1, to));
        self.current = to;
    }

    fn close(&mut self) {
        if self.current != self.start {
            self.segments.push(Segment::Line(self.current, self.start));
        }
        self.current = self.start;
    }
}

/// The first walk: the union of every fill's clip. See the module docs.
struct Measure<'a, 'f> {
    font: &'a FontRef<'f>,
    transforms: Vec<Transform>,
    /// Each open clip's bounds, inside the one before it.
    clips: Vec<Rect>,
    extent: Option<Rect>,
    nesting: Nesting,
}

impl Measure<'_, '_> {
    fn transform(&self) -> Transform {
        self.transforms.last().copied().unwrap_or_default()
    }

    fn push_clip(&mut self, rect: Option<Rect>) {
        if !self.nesting.enter() {
            return;
        }
        let empty = Rect {
            min: Point::ZERO,
            max: Point::ZERO,
        };
        let rect = rect.unwrap_or(empty);
        self.clips.push(match self.clips.last() {
            Some(outer) => rect.intersect(*outer),
            None => rect,
        });
    }
}

impl ColorPainter for Measure<'_, '_> {
    fn push_transform(&mut self, transform: Transform) {
        if self.nesting.enter() {
            self.transforms.push(self.transform() * transform);
        }
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
            self.nesting.leave();
        }
    }

    fn push_clip_glyph(&mut self, glyph: skrifa::GlyphId) {
        let rect = bounds(&outline(self.font, glyph, self.transform()));
        self.push_clip(rect);
    }

    fn push_clip_box(&mut self, clip: BoundingBox<f32>) {
        let rect = bounds(&clip_box(clip, self.transform()));
        self.push_clip(rect);
    }

    fn pop_clip(&mut self) {
        if self.clips.pop().is_some() {
            self.nesting.leave();
        }
    }

    fn fill(&mut self, _brush: Brush<'_>) {
        if !self.nesting.fill() {
            return;
        }
        // A fill outside every clip covers the whole plane: no extent can
        // hold it, so the glyph is not drawn in colour.
        let Some(&clip) = self.clips.last() else {
            self.nesting.broken = true;
            return;
        };
        if !clip.is_empty() {
            self.extent = Some(self.extent.map_or(clip, |extent| extent.union(clip)));
        }
    }

    fn push_layer(&mut self, _mode: CompositeMode) {
        self.nesting.enter();
    }

    fn pop_layer(&mut self) {
        self.nesting.leave();
    }
}

/// An open clip: each canvas pixel's coverage, and the pixels it can cover.
struct Clip {
    mask: Vec<f32>,
    /// The pixels outside which `mask` is zero: `x0..x1` by `y0..y1`.
    area: (usize, usize, usize, usize),
}

/// A canvas of premultiplied RGBA, and how it joins the one beneath it.
struct Layer {
    pixels: Vec<[f32; 4]>,
    mode: CompositeMode,
}

/// The second walk: the glyph painted. See the module docs.
struct Painter<'a, 'f> {
    font: &'a FontRef<'f>,
    palette: &'a [skrifa::color::Color],
    width: usize,
    height: usize,
    transforms: Vec<Transform>,
    clips: Vec<Clip>,
    layers: Vec<Layer>,
    nesting: Nesting,
    /// Pixels the fills have touched so far.
    filled: usize,
    rasterizer: Rasterizer,
}

impl Painter<'_, '_> {
    fn transform(&self) -> Transform {
        self.transforms.last().copied().unwrap_or_default()
    }

    /// The canvas pixels the open clips and layers hold, and `more` beside
    /// them, within [`MAX_HELD_PIXELS`]; breaks the paint when not.
    fn room_for(&mut self, more: usize) -> bool {
        let held = (self.clips.len() + self.layers.len() + more) * self.width * self.height;
        if held > MAX_HELD_PIXELS {
            self.nesting.broken = true;
        }
        !self.nesting.broken
    }

    /// Pushes the clip `segments` cover, inside the current one.
    fn push_clip(&mut self, segments: &[Segment]) {
        if !(self.nesting.enter() && self.room_for(1)) {
            return;
        }
        let (width, height) = (self.width, self.height);
        let canvas = Rect {
            min: Point::ZERO,
            max: Point::new(width as f32, height as f32),
        };
        let mut area = (0, 0, 0, 0);
        let mut mask = vec![0.0; width * height];
        if let Some(rect) = bounds(segments).map(|rect| rect.intersect(canvas))
            && !rect.is_empty()
        {
            self.rasterizer.reset(width as u32, height as u32);
            for segment in segments {
                match *segment {
                    Segment::Line(a, b) => self.rasterizer.line(a, b),
                    Segment::Quadratic(a, b, c) => self.rasterizer.quadratic(a, b, c),
                    Segment::Cubic(a, b, c, d) => self.rasterizer.cubic(a, b, c, d),
                }
            }
            mask = self.rasterizer.coverage_by(FillRule::NonZero);
            area = (
                rect.min.x.floor() as usize,
                rect.min.y.floor() as usize,
                (rect.max.x.ceil() as usize).min(width),
                (rect.max.y.ceil() as usize).min(height),
            );
        }
        if let Some(outer) = self.clips.last() {
            for (coverage, outside) in mask.iter_mut().zip(&outer.mask) {
                *coverage *= outside;
            }
            area = (
                area.0.max(outer.area.0),
                area.1.max(outer.area.1),
                area.2.min(outer.area.2),
                area.3.min(outer.area.3),
            );
        }
        self.clips.push(Clip { mask, area });
    }

    /// The colour of palette entry `index`, its alpha times `alpha`, as
    /// straight sRGB in `0..=1`.
    fn palette_color(&self, index: u16, alpha: f32) -> [f32; 4] {
        let rgba = if index == FOREGROUND {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            self.palette
                .get(usize::from(index))
                .map_or([0.0; 4], |color| {
                    [color.red, color.green, color.blue, color.alpha].map(|c| f32::from(c) / 255.0)
                })
        };
        [rgba[0], rgba[1], rgba[2], rgba[3] * alpha.clamp(0.0, 1.0)]
    }

    /// `stops` at `t`, `extend` applied, premultiplied.
    fn color_line(&self, stops: &[ColorStop], extend: Extend, t: f32) -> [f32; 4] {
        let t = match extend {
            Extend::Repeat => t - t.floor(),
            Extend::Reflect => {
                let folded = t.rem_euclid(2.0);
                if folded > 1.0 { 2.0 - folded } else { folded }
            }
            _ => t.clamp(0.0, 1.0),
        };
        let stop =
            |stop: &ColorStop| premultiplied(self.palette_color(stop.palette_index, stop.alpha));
        let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
            return [0.0; 4];
        };
        if t <= first.offset {
            return stop(first);
        }
        if t >= last.offset {
            return stop(last);
        }
        let after = stops
            .partition_point(|stop| stop.offset <= t)
            .min(stops.len() - 1);
        let (a, b) = (&stops[after.saturating_sub(1)], &stops[after]);
        let span = b.offset - a.offset;
        let along = if span > 0.0 {
            (t - a.offset) / span
        } else {
            0.0
        };
        let (from, to) = (stop(a), stop(b));
        core::array::from_fn(|c| from[c] + (to[c] - from[c]) * along)
    }
}

/// `color`, straight, premultiplied.
fn premultiplied([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [r * a, g * a, b * a, a]
}

/// A premultiplied pixel as straight RGBA8.
fn straight([r, g, b, a]: [f32; 4]) -> [u8; 4] {
    let alpha = a.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return [0; 4];
    }
    let channel = |c: f32| ((c / alpha).clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        channel(r),
        channel(g),
        channel(b),
        (alpha * 255.0).round() as u8,
    ]
}

/// The inverse of `transform`, or `None` for one that flattens the plane.
fn inverse(transform: Transform) -> Option<Transform> {
    let [xx, yx, xy, yy, dx, dy] = transform.elements();
    let det = xx * yy - xy * yx;
    if det.abs() < f32::EPSILON || !det.is_finite() {
        return None;
    }
    let (ixx, iyx, ixy, iyy) = (yy / det, -yx / det, -xy / det, xx / det);
    Some(Transform::from_elements([
        ixx,
        iyx,
        ixy,
        iyy,
        -(ixx * dx + ixy * dy),
        -(iyx * dx + iyy * dy),
    ]))
}

/// Where along a gradient the brush-space point `p` is, or `None` where the
/// gradient paints nothing.
fn gradient_t(brush: &Brush<'_>, p: Point) -> Option<f32> {
    match *brush {
        Brush::Solid { .. } => Some(0.0),
        Brush::LinearGradient { p0, p1, .. } => {
            let axis = Point::new(p1.x - p0.x, p1.y - p0.y);
            let length = axis.dot(axis);
            (length > 0.0).then(|| (p - Point::new(p0.x, p0.y)).dot(axis) / length)
        }
        Brush::RadialGradient { c0, r0, c1, r1, .. } => {
            // The largest t whose circle, c0 + t(c1 - c0) at radius
            // r0 + t(r1 - r0), passes through p with a radius of at least
            // zero: the two-point conical gradient.
            let dc = Point::new(c1.x - c0.x, c1.y - c0.y);
            let dr = r1 - r0;
            let pc = p - Point::new(c0.x, c0.y);
            let a = dc.dot(dc) - dr * dr;
            let b = pc.dot(dc) + r0 * dr;
            let c = pc.dot(pc) - r0 * r0;
            let radius_ok = |t: f32| r0 + t * dr >= 0.0;
            if a.abs() < f32::EPSILON {
                if b.abs() < f32::EPSILON {
                    return None;
                }
                let t = c / (2.0 * b);
                return radius_ok(t).then_some(t);
            }
            let discriminant = b * b - a * c;
            if discriminant < 0.0 {
                return None;
            }
            let root = discriminant.sqrt();
            let (high, low) = {
                let (t1, t2) = ((b + root) / a, (b - root) / a);
                (t1.max(t2), t1.min(t2))
            };
            [high, low].into_iter().find(|&t| radius_ok(t))
        }
        Brush::SweepGradient {
            c0,
            start_angle,
            end_angle,
            ..
        } => {
            // Clockwise in the brush's own y-up space, as `skrifa` hands the
            // angles over, and only from 0° to 360°.
            let mut angle = (-(p.y - c0.y)).atan2(p.x - c0.x).to_degrees();
            if angle < 0.0 {
                angle += 360.0;
            }
            let sector = end_angle - start_angle;
            (sector != 0.0).then(|| (angle - start_angle) / sector)
        }
    }
}

impl ColorPainter for Painter<'_, '_> {
    fn push_transform(&mut self, transform: Transform) {
        if self.nesting.enter() {
            self.transforms.push(self.transform() * transform);
        }
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
            self.nesting.leave();
        }
    }

    fn push_clip_glyph(&mut self, glyph: skrifa::GlyphId) {
        let segments = outline(self.font, glyph, self.transform());
        self.push_clip(&segments);
    }

    fn push_clip_box(&mut self, clip: BoundingBox<f32>) {
        let segments = clip_box(clip, self.transform());
        self.push_clip(&segments);
    }

    fn pop_clip(&mut self) {
        if self.clips.pop().is_some() {
            self.nesting.leave();
        }
    }

    fn fill(&mut self, brush: Brush<'_>) {
        if !self.nesting.fill() {
            return;
        }
        let (x0, y0, x1, y1) = self
            .clips
            .last()
            .map_or((0, 0, self.width, self.height), |clip| clip.area);
        self.filled += x1.saturating_sub(x0) * y1.saturating_sub(y0);
        if self.filled > MAX_FILLED_PIXELS {
            self.nesting.broken = true;
            return;
        }
        let solid = match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => Some(premultiplied(self.palette_color(palette_index, alpha))),
            _ => None,
        };
        let to_brush = match solid {
            Some(_) => None,
            None => match inverse(self.transform()) {
                Some(inverse) => Some(inverse),
                None => return,
            },
        };
        let (stops, extend) = match brush {
            Brush::LinearGradient {
                color_stops,
                extend,
                ..
            }
            | Brush::RadialGradient {
                color_stops,
                extend,
                ..
            }
            | Brush::SweepGradient {
                color_stops,
                extend,
                ..
            } => (color_stops, extend),
            Brush::Solid { .. } => (&[][..], Extend::Pad),
        };
        let mut layer =
            core::mem::take(&mut self.layers.last_mut().expect("the base layer").pixels);
        for y in y0..y1 {
            for x in x0..x1 {
                let at = y * self.width + x;
                let coverage = self.clips.last().map_or(1.0, |clip| clip.mask[at]);
                if coverage <= 0.0 {
                    continue;
                }
                let color = match (solid, to_brush) {
                    (Some(color), _) => color,
                    (None, Some(to_brush)) => {
                        let (bx, by) = to_brush.transform(x as f32 + 0.5, y as f32 + 0.5);
                        match gradient_t(&brush, Point::new(bx, by)) {
                            Some(t) => self.color_line(stops, extend, t),
                            None => continue,
                        }
                    }
                    (None, None) => continue,
                };
                let source = color.map(|c| c * coverage);
                let target = &mut layer[at];
                *target = core::array::from_fn(|c| source[c] + target[c] * (1.0 - source[3]));
            }
        }
        self.layers.last_mut().expect("the base layer").pixels = layer;
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        if !(self.nesting.enter() && self.room_for(1)) {
            return;
        }
        self.layers.push(Layer {
            pixels: vec![[0.0; 4]; self.width * self.height],
            mode,
        });
    }

    fn pop_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        self.nesting.leave();
        let top = self.layers.pop().expect("two layers");
        let below = self.layers.last_mut().expect("one layer");
        for (target, source) in below.pixels.iter_mut().zip(&top.pixels) {
            *target = composite(*source, *target, top.mode);
        }
    }
}

/// `source` composited onto `backdrop` by `mode`, both premultiplied: the
/// W3C Compositing and Blending Level 1 formulas.
fn composite(source: [f32; 4], backdrop: [f32; 4], mode: CompositeMode) -> [f32; 4] {
    let (sa, da) = (source[3], backdrop[3]);
    let porter_duff = |fs: f32, fd: f32| -> [f32; 4] {
        core::array::from_fn(|c| source[c] * fs + backdrop[c] * fd)
    };
    match mode {
        CompositeMode::Clear => [0.0; 4],
        CompositeMode::Src => source,
        CompositeMode::Dest => backdrop,
        CompositeMode::DestOver => porter_duff(1.0 - da, 1.0),
        CompositeMode::SrcIn => porter_duff(da, 0.0),
        CompositeMode::DestIn => porter_duff(0.0, sa),
        CompositeMode::SrcOut => porter_duff(1.0 - da, 0.0),
        CompositeMode::DestOut => porter_duff(0.0, 1.0 - sa),
        CompositeMode::SrcAtop => porter_duff(da, 1.0 - sa),
        CompositeMode::DestAtop => porter_duff(1.0 - da, sa),
        CompositeMode::Xor => porter_duff(1.0 - da, 1.0 - sa),
        CompositeMode::Plus => core::array::from_fn(|c| (source[c] + backdrop[c]).min(1.0)),
        CompositeMode::Screen
        | CompositeMode::Overlay
        | CompositeMode::Darken
        | CompositeMode::Lighten
        | CompositeMode::ColorDodge
        | CompositeMode::ColorBurn
        | CompositeMode::HardLight
        | CompositeMode::SoftLight
        | CompositeMode::Difference
        | CompositeMode::Exclusion
        | CompositeMode::Multiply
        | CompositeMode::HslHue
        | CompositeMode::HslSaturation
        | CompositeMode::HslColor
        | CompositeMode::HslLuminosity => {
            let unpremultiply = |pixel: [f32; 4]| -> [f32; 3] {
                if pixel[3] > 0.0 {
                    [pixel[0], pixel[1], pixel[2]].map(|c| (c / pixel[3]).clamp(0.0, 1.0))
                } else {
                    [0.0; 3]
                }
            };
            let blended = blend(unpremultiply(backdrop), unpremultiply(source), mode);
            let alpha = sa + da - sa * da;
            let mut out = [0.0, 0.0, 0.0, alpha];
            for c in 0..3 {
                out[c] = (1.0 - da) * source[c] + (1.0 - sa) * backdrop[c] + sa * da * blended[c];
            }
            out
        }
        // Source-over, the default, and what an unknown mode reads as.
        _ => porter_duff(1.0, 1.0 - sa),
    }
}

/// The blend function `B(backdrop, source)` of a blend `mode`, on straight
/// colours.
fn blend(backdrop: [f32; 3], source: [f32; 3], mode: CompositeMode) -> [f32; 3] {
    let separable = |f: fn(f32, f32) -> f32| -> [f32; 3] {
        core::array::from_fn(|c| f(backdrop[c], source[c]))
    };
    match mode {
        CompositeMode::Multiply => separable(|b, s| b * s),
        CompositeMode::Screen => separable(screen),
        CompositeMode::Overlay => separable(|b, s| hard_light(s, b)),
        CompositeMode::Darken => separable(f32::min),
        CompositeMode::Lighten => separable(f32::max),
        CompositeMode::ColorDodge => separable(|b, s| {
            if b == 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        CompositeMode::ColorBurn => separable(|b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        CompositeMode::HardLight => separable(hard_light),
        CompositeMode::SoftLight => separable(|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        CompositeMode::Difference => separable(|b, s| (b - s).abs()),
        CompositeMode::Exclusion => separable(|b, s| b + s - 2.0 * b * s),
        CompositeMode::HslHue => set_lum(set_sat(source, sat(backdrop)), lum(backdrop)),
        CompositeMode::HslSaturation => set_lum(set_sat(backdrop, sat(source)), lum(backdrop)),
        CompositeMode::HslColor => set_lum(source, lum(backdrop)),
        CompositeMode::HslLuminosity => set_lum(backdrop, lum(source)),
        _ => source,
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    c.map(|channel| {
        let mut channel = channel;
        if n < 0.0 && l - n != 0.0 {
            channel = l + (channel - l) * l / (l - n);
        }
        if x > 1.0 && x - l != 0.0 {
            channel = l + (channel - l) * (1.0 - l) / (x - l);
        }
        channel
    })
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|channel| channel + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max > min {
        c.map(|channel| (channel - min) * s / (max - min))
    } else {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{Font, fixtures};

    /// The fixtures' em: at this size a font unit is a pixel.
    const SIZE: f32 = 16.0;
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const CLEAR: [u8; 4] = [0; 4];

    fn paint(font: &Font, c: char, limits: PaintLimits) -> Option<ColorBitmap> {
        let font_ref = FontRef::new(font.data()).expect("reads");
        let glyph = skrifa::GlyphId::new(font.glyph_id(c).0);
        rasterize(&font_ref, glyph, SIZE, font.metrics().units_per_em, limits)
    }

    /// The pixel covering the font-unit square from `(x, y)` to
    /// `(x + 1, y + 1)`, y up from the baseline.
    fn unit(bitmap: &ColorBitmap, x: i32, y: i32) -> [u8; 4] {
        let column = (x - bitmap.left) as u32;
        let row = (-(y + 1) - bitmap.top) as u32;
        let at = ((row * bitmap.width + column) * 4) as usize;
        bitmap.pixels[at..at + 4].try_into().expect("four bytes")
    }

    fn extent(bitmap: &ColorBitmap) -> (u32, u32, i32, i32) {
        (bitmap.width, bitmap.height, bitmap.left, bitmap.top)
    }

    /// Each channel of `got` within one of `want`: a gradient's values pass
    /// through a transcendental function, so the last bit may differ by
    /// platform.
    fn near(got: [u8; 4], want: [u8; 4]) -> bool {
        got.iter().zip(want).all(|(&g, w)| g.abs_diff(w) <= 1)
    }

    /// **COLR v0 layers paint in order, each in its palette colour**: red,
    /// then blue over it, then half-transparent green over both, composited
    /// premultiplied — worked out by hand from the fixture's squares.
    #[test]
    fn v0_layers_paint_in_order_in_their_palette_colours() {
        let smile = paint(&fixtures::colrv0(), '\u{1F600}', PaintLimits::DEFAULT).expect("paints");
        assert_eq!(extent(&smile), (12, 12, 0, -12));
        assert_eq!(unit(&smile, 1, 1), RED, "the first layer alone");
        assert_eq!(unit(&smile, 10, 10), BLUE, "the second alone");
        // 128/255 green over red: red keeps 127/255, green is 128/255.
        assert_eq!(unit(&smile, 3, 3), [127, 128, 0, 255], "green over red");
        assert_eq!(unit(&smile, 5, 5), [0, 128, 127, 255], "green over blue");
        assert_eq!(unit(&smile, 6, 6), BLUE, "blue over red, the green ended");
        assert_eq!(unit(&smile, 10, 1), CLEAR);
        assert_eq!(unit(&smile, 1, 10), CLEAR);
        // The module's own entry asks COLR first, so it is this picture.
        let font = fixtures::colrv0();
        let glyph = font.glyph_id('\u{1F600}');
        assert_eq!(super::super::rasterize(&font, glyph, SIZE), Some(smile));
    }

    /// **A COLR v1 solid, a translation, a composite and a nested clip** each
    /// paint the pixels worked out from the fixture's paint graphs, and the
    /// picture is exactly as large as what was filled.
    #[test]
    fn v1_solids_transforms_composites_and_clips_paint_their_pixels() {
        let font = fixtures::colrv1();
        let solid = paint(&font, '\u{1F7E5}', PaintLimits::DEFAULT).expect("paints");
        assert_eq!(extent(&solid), (12, 12, 0, -12));
        assert!(solid.pixels.chunks(4).all(|p| p == RED));

        // A 4-unit square moved 8 right and 4 up.
        let moved = paint(&font, '\u{1F7E9}', PaintLimits::DEFAULT).expect("paints");
        assert_eq!(extent(&moved), (4, 4, 8, -8));
        assert!(moved.pixels.chunks(4).all(|p| p == BLUE));

        // Blue source-in over a red backdrop: blue where both are, nothing
        // where either is alone.
        let composite = paint(&font, '\u{1F7EA}', PaintLimits::DEFAULT).expect("paints");
        assert_eq!(extent(&composite), (12, 12, 0, -12));
        assert_eq!(unit(&composite, 5, 5), BLUE);
        assert_eq!(unit(&composite, 1, 1), CLEAR, "the backdrop alone");
        assert_eq!(unit(&composite, 10, 10), CLEAR, "the source alone");

        // Green filled through two nested glyph clips: their overlap only.
        let clipped = paint(&font, '\u{1F7EB}', PaintLimits::DEFAULT).expect("paints");
        assert_eq!(extent(&clipped), (4, 4, 4, -8));
        assert!(clipped.pixels.chunks(4).all(|p| p == GREEN));
    }

    /// **Linear, radial and sweep gradients** from red to blue, at pixels
    /// whose positions along each were worked out by hand: a pixel's centre
    /// is half a unit into it.
    #[test]
    fn v1_gradients_follow_their_geometry() {
        let font = fixtures::colrv1();
        // Along x from 0 to 12: t is the centre's x over 12.
        let linear = paint(&font, '\u{1F7E6}', PaintLimits::DEFAULT).expect("paints");
        for (x, want) in [
            (0, [244, 0, 11, 255]),
            (5, [138, 0, 117, 255]),
            (11, [11, 0, 244, 255]),
        ] {
            let got = unit(&linear, x, 7);
            assert!(near(got, want), "x {x}: {got:?} is not {want:?}");
        }
        // Out from (6, 6) to a radius of 6, padded past it.
        let radial = paint(&font, '\u{1F7E7}', PaintLimits::DEFAULT).expect("paints");
        let got = unit(&radial, 6, 6);
        assert!(near(got, [225, 0, 30, 255]), "{got:?}: t is sqrt(0.5) / 6");
        assert_eq!(unit(&radial, 0, 11), BLUE, "past the radius");
        // Counter-clockwise from 0° to 90° round (6, 6), padded past 90°.
        let sweep = paint(&font, '\u{1F7E8}', PaintLimits::DEFAULT).expect("paints");
        let got = unit(&sweep, 8, 8);
        assert!(near(got, [128, 0, 128, 255]), "{got:?}: 45° is halfway");
        let got = unit(&sweep, 11, 6);
        // atan(0.5 / 5.5) is 5.19°, t 0.0577.
        assert!(near(got, [240, 0, 15, 255]), "{got:?}: just past 0°");
        assert_eq!(unit(&sweep, 8, 3), BLUE, "315° is past the end");
    }

    /// **A paint graph past the depth limit, or a glyph past the fill
    /// limit, is not drawn in colour**, and within them it is: the limits,
    /// not `skrifa`'s own, refuse it.
    #[test]
    fn a_glyph_past_the_depth_or_fill_limit_paints_nothing() {
        let deep = '\u{1F7EC}';
        let font = fixtures::colrv1();
        assert_eq!(paint(&font, deep, PaintLimits::DEFAULT), None);
        let deeper = PaintLimits {
            depth: 2 * MAX_PAINT_DEPTH,
            ..PaintLimits::DEFAULT
        };
        assert!(paint(&font, deep, deeper).is_some(), "skrifa paints it");

        let many = '\u{1F601}';
        let colrv0 = fixtures::colrv0();
        let fills = |fills| PaintLimits {
            fills,
            ..PaintLimits::DEFAULT
        };
        assert_eq!(paint(&colrv0, many, fills(2)), None, "three layers");
        assert!(paint(&colrv0, many, fills(3)).is_some());
        // Refused, the glyph has no colour picture at all, through the
        // module's own entry too.
        assert_eq!(
            super::super::rasterize(&font, font.glyph_id(deep), SIZE),
            None
        );
    }

    /// **The composite modes are the W3C formulas**, checked on opaque and
    /// transparent pixels whose results are worked out by hand.
    #[test]
    fn composite_modes_follow_the_w3c_formulas() {
        let backdrop = [0.5, 0.25, 1.0, 1.0];
        let source = [0.5, 0.5, 0.0, 1.0];
        let cases = [
            (CompositeMode::SrcOver, source),
            (CompositeMode::DestOver, backdrop),
            (CompositeMode::Multiply, [0.25, 0.125, 0.0, 1.0]),
            (CompositeMode::Screen, [0.75, 0.625, 1.0, 1.0]),
            (CompositeMode::Darken, [0.5, 0.25, 0.0, 1.0]),
            (CompositeMode::Lighten, [0.5, 0.5, 1.0, 1.0]),
            (CompositeMode::Difference, [0.0, 0.25, 1.0, 1.0]),
            (CompositeMode::Exclusion, [0.5, 0.5, 1.0, 1.0]),
            (CompositeMode::Xor, [0.0; 4]),
            (CompositeMode::DestOut, [0.0; 4]),
            (CompositeMode::SrcAtop, source),
            (CompositeMode::Clear, [0.0; 4]),
        ];
        for (mode, want) in cases {
            assert_eq!(composite(source, backdrop, mode), want, "{mode:?}");
        }
        // A half-transparent source over nothing keeps itself, whatever it
        // blends by: there is no backdrop to blend with.
        let half = [0.25, 0.0, 0.0, 0.5];
        assert_eq!(composite(half, [0.0; 4], CompositeMode::Multiply), half);
        assert_eq!(composite(half, [0.0; 4], CompositeMode::SrcIn), [0.0; 4]);
        // Greys: luminosity takes the source's lightness, colour the
        // backdrop's.
        let (dark, light) = ([0.2, 0.2, 0.2, 1.0], [0.6, 0.6, 0.6, 1.0]);
        let close =
            |got: [f32; 4], want: [f32; 4]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5);
        assert!(close(
            composite(light, dark, CompositeMode::HslLuminosity),
            light
        ));
        assert!(close(composite(light, dark, CompositeMode::HslColor), dark));
    }
}
