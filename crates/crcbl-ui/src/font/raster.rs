//! This engine's coverage rasteriser: outlines in, an exact-area coverage mask
//! out.
//!
//! # The algorithm: signed-area accumulation
//!
//! The technique of Raph Levien's `font-rs` ("Inside the fastest font renderer
//! in the world", 2016), which `ab_glyph_rasterizer` also uses. Every outline
//! segment is a line; a line crossing a pixel row deposits, into that row's
//! cells, the **signed area** it leaves to its right, and a running sum along
//! the row then *is* each pixel's coverage. A closed outline's deposits in a
//! row sum to zero, so the sum returns to zero past the shape's right edge
//! without anything tracking inside and outside.
//!
//! For a line spanning `left..right` across one row with height `dy` (signed by
//! its direction), the fraction of column `c`'s strip lying right of it is
//!
//! ```text
//! F(c) = 1/(right - left) · ∫[left, right] clamp(c + 1 - x, 0, 1) dx
//! ```
//!
//! which is `0` left of the line and `1` right of it. Cell `c` receives
//! `dy · (F(c) - F(c - 1))`, and the cell after the last column the line
//! touches receives the remainder, so the running sum at column `c` is
//! `dy · F(c)`. [`Rasterizer::line`] evaluates the integral in closed form per
//! column; `font-rs` writes the same quantity as special cases for one, two
//! and many columns.
//!
//! Coverage is `min(|sum|, 1)`: the non-zero rule, with two overlapping
//! contours of one winding saturating at full coverage.
//!
//! # Curves: Wang's formula
//!
//! A quadratic or cubic is flattened into `n` equal parameter steps, with `n`
//! from Wang's formula (Wang, via Filip, Magedson and Markot, "Surface
//! algorithms using bounds on derivatives", 1986): a degree-`d` Bézier whose
//! largest control-point second difference is `M` stays within `tolerance` of
//! its chords when
//!
//! ```text
//! n ≥ sqrt(d (d - 1) M / (8 · tolerance))
//! ```
//!
//! [`FLATTEN_TOLERANCE`] is the tolerance every glyph is drawn at.
//!
//! # Portable arithmetic
//!
//! Nothing that produces coverage calls a transcendental function. The
//! rasteriser is the four operations, `floor`, `ceil`, `abs`, `min`/`max` and —
//! in Wang's formula only — `sqrt`, which IEEE 754 requires to be correctly
//! rounded. So a mask comes out the same bytes on every platform's libm.
//!
//! # Coverage is stored linear
//!
//! Coverage is an area, and area composites correctly in linear light, which
//! is where the UI pass blends: its target is sRGB-encoded, so the blend runs
//! on decoded values. [`coverage_byte`] therefore quantises coverage as it is —
//! a gamma of one, with no contrast curve — and the shader multiplies it into
//! alpha unchanged.

use glam::Vec2;

/// How far a flattened curve may stray from the true one, in pixels.
pub const FLATTEN_TOLERANCE: f32 = 1.0 / 32.0;

/// The most line segments one curve is flattened into, however large.
pub const MAX_CURVE_SEGMENTS: u32 = 256;

/// Below this horizontal extent, in pixels, a line's column integral is taken
/// at its midpoint: the closed form divides by the extent, and the error of the
/// midpoint rule is at most half of it.
const VERTICAL_EXTENT: f32 = 1.0e-4;

/// How many equal parameter steps flatten a degree-`degree` Bézier whose
/// largest control-point second difference has length
/// `max_second_difference`, to within `tolerance`: Wang's formula, clamped to
/// `1..=MAX_CURVE_SEGMENTS`.
#[must_use]
pub fn wang_segments(degree: u32, max_second_difference: f32, tolerance: f32) -> u32 {
    let degree = degree as f32;
    let steps = (degree * (degree - 1.0) * max_second_difference / (8.0 * tolerance)).sqrt();
    if steps.is_nan() {
        return 1;
    }
    (steps.ceil() as u32).clamp(1, MAX_CURVE_SEGMENTS)
}

/// A coverage value as the byte the glyph atlas stores: `round(coverage ×
/// 255)`, clamped into range. Linear — see the module docs.
#[must_use]
pub fn coverage_byte(coverage: f32) -> u8 {
    // `as` saturates and sends NaN to zero.
    (coverage.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// An accumulation buffer one outline is drawn into. See the module docs.
///
/// Coordinates are pixels with `(0, 0)` the top-left corner of the mask and y
/// growing downwards; pixel `(x, y)` covers `x..x + 1` by `y..y + 1`. Geometry
/// outside the mask contributes nothing to it, and is not an error.
#[derive(Clone, Debug, Default)]
pub struct Rasterizer {
    width: usize,
    height: usize,
    /// `width + 1` cells a row: the last is where a line's remainder goes when
    /// it reaches the right edge.
    cells: Vec<f32>,
}

impl Rasterizer {
    /// An empty `width` by `height` mask.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let mut rasterizer = Self::default();
        rasterizer.reset(width, height);
        rasterizer
    }

    /// Empties the mask and resizes it, keeping the allocation.
    pub fn reset(&mut self, width: u32, height: u32) {
        self.width = width as usize;
        self.height = height as usize;
        self.cells.clear();
        self.cells.resize((self.width + 1) * self.height, 0.0);
    }

    /// The mask's width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width as u32
    }

    /// The mask's height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height as u32
    }

    /// Accumulates one edge from `from` to `to`.
    pub fn line(&mut self, from: Vec2, to: Vec2) {
        if !(from.is_finite() && to.is_finite()) || from.y == to.y || self.width == 0 {
            return;
        }
        let (top, bottom, direction) = if from.y < to.y {
            (from, to, 1.0)
        } else {
            (to, from, -1.0)
        };
        let start = top.y.max(0.0);
        let end = bottom.y.min(self.height as f32);
        if start >= end {
            return;
        }
        let slope = (bottom.x - top.x) / (bottom.y - top.y);
        let first_row = start.floor() as usize;
        let last_row = (end.ceil() as usize).min(self.height);
        for row in first_row..last_row {
            let upper = (row as f32).max(top.y);
            let lower = ((row + 1) as f32).min(bottom.y);
            if lower <= upper {
                continue;
            }
            let x_upper = top.x + (upper - top.y) * slope;
            let x_lower = top.x + (lower - top.y) * slope;
            self.deposit(
                row,
                x_upper.min(x_lower),
                x_upper.max(x_lower),
                (lower - upper) * direction,
            );
        }
    }

    /// Deposits one row's piece of an edge spanning `left..=right`, with
    /// signed height `height`. See the module docs for what goes where.
    fn deposit(&mut self, row: usize, left: f32, right: f32, height: f32) {
        let width = self.width;
        let cells = &mut self.cells[row * (width + 1)..(row + 1) * (width + 1)];
        if right <= 0.0 {
            cells[0] += height;
            return;
        }
        if left >= width as f32 {
            return;
        }
        let first = left.floor().max(0.0) as usize;
        let last = (right.ceil() as usize - 1).min(width - 1);
        let mut before = 0.0;
        for (column, cell) in cells.iter_mut().enumerate().take(last + 1).skip(first) {
            let fraction = right_of(left, right, column as f32);
            *cell += height * (fraction - before);
            before = fraction;
        }
        cells[last + 1] += height * (1.0 - before);
    }

    /// Accumulates a quadratic Bézier, flattened by [`wang_segments`].
    pub fn quadratic(&mut self, from: Vec2, control: Vec2, to: Vec2) {
        let second = (from - 2.0 * control + to).length();
        let steps = wang_segments(2, second, FLATTEN_TOLERANCE);
        let mut previous = from;
        for step in 1..=steps {
            let point = if step == steps {
                to
            } else {
                let t = step as f32 / steps as f32;
                let u = 1.0 - t;
                from * (u * u) + control * (2.0 * u * t) + to * (t * t)
            };
            self.line(previous, point);
            previous = point;
        }
    }

    /// Accumulates a cubic Bézier, flattened by [`wang_segments`].
    pub fn cubic(&mut self, from: Vec2, control0: Vec2, control1: Vec2, to: Vec2) {
        let second = (from - 2.0 * control0 + control1)
            .length()
            .max((control0 - 2.0 * control1 + to).length());
        let steps = wang_segments(3, second, FLATTEN_TOLERANCE);
        let mut previous = from;
        for step in 1..=steps {
            let point = if step == steps {
                to
            } else {
                let t = step as f32 / steps as f32;
                let u = 1.0 - t;
                from * (u * u * u)
                    + control0 * (3.0 * u * u * t)
                    + control1 * (3.0 * u * t * t)
                    + to * (t * t * t)
            };
            self.line(previous, point);
            previous = point;
        }
    }

    /// Each pixel's coverage in `0..=1`, rows top to bottom.
    #[must_use]
    pub fn coverage(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.width * self.height);
        for row in self.cells.chunks_exact(self.width + 1) {
            let mut sum = 0.0f32;
            for cell in &row[..self.width] {
                sum += cell;
                out.push(sum.abs().min(1.0));
            }
        }
        out
    }

    /// Each pixel's coverage as [`coverage_byte`], rows top to bottom,
    /// appended to `out`.
    pub fn write_mask(&self, out: &mut Vec<u8>) {
        out.reserve(self.width * self.height);
        for row in self.cells.chunks_exact(self.width + 1) {
            let mut sum = 0.0f32;
            for cell in &row[..self.width] {
                sum += cell;
                out.push(coverage_byte(sum.abs()));
            }
        }
    }
}

/// `F(column)` from the module docs: the fraction of the strip
/// `column..column + 1` lying right of an edge running across `left..=right`.
fn right_of(left: f32, right: f32, column: f32) -> f32 {
    let next = column + 1.0;
    if right - left < VERTICAL_EXTENT {
        return (next - 0.5 * (left + right)).clamp(0.0, 1.0);
    }
    // Where the edge is left of `column`, the whole strip is right of it.
    let whole = (right.min(column) - left).max(0.0);
    // Where it crosses the strip, the part right of `x` is `next - x`.
    let a = left.max(column);
    let b = right.min(next);
    let crossing = if b > a {
        (b - a) * (next - 0.5 * (a + b))
    } else {
        0.0
    };
    (whole + crossing) / (right - left)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws a closed polygon.
    fn polygon(rasterizer: &mut Rasterizer, points: &[Vec2]) {
        for (index, &point) in points.iter().enumerate() {
            rasterizer.line(point, points[(index + 1) % points.len()]);
        }
    }

    /// A polygon's area by the shoelace formula.
    fn area(points: &[Vec2]) -> f32 {
        let mut twice = 0.0f64;
        for (index, a) in points.iter().enumerate() {
            let b = points[(index + 1) % points.len()];
            twice += f64::from(a.x) * f64::from(b.y) - f64::from(b.x) * f64::from(a.y);
        }
        (twice.abs() * 0.5) as f32
    }

    /// `points` clipped to the half-plane `keep`, by Sutherland–Hodgman.
    fn clip(points: &[Vec2], keep: impl Fn(Vec2) -> f32) -> Vec<Vec2> {
        let mut out = Vec::new();
        for (index, &a) in points.iter().enumerate() {
            let b = points[(index + 1) % points.len()];
            let (da, db) = (keep(a), keep(b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
        out
    }

    /// The exact coverage of every pixel of a `width` by `height` mask by a
    /// convex-or-not polygon: the polygon clipped to each pixel square, its
    /// area by the shoelace formula. Nothing in common with the rasteriser.
    fn clipped_coverage(points: &[Vec2], width: u32, height: u32) -> Vec<f32> {
        let mut out = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (x, y) = (x as f32, y as f32);
                let mut piece = clip(points, |p| p.x - x);
                piece = clip(&piece, |p| x + 1.0 - p.x);
                piece = clip(&piece, |p| p.y - y);
                piece = clip(&piece, |p| y + 1.0 - p.y);
                out.push(if piece.len() < 3 { 0.0 } else { area(&piece) });
            }
        }
        out
    }

    fn assert_close(got: &[f32], want: &[f32], within: f32, what: &str) {
        assert_eq!(got.len(), want.len(), "{what}");
        for (index, (got, want)) in got.iter().zip(want).enumerate() {
            assert!(
                (got - want).abs() <= within,
                "{what}: pixel {index} has coverage {got}, want {want} (within {within})"
            );
        }
    }

    /// **A unit square on a pixel covers that pixel fully and nothing else**,
    /// wound either way.
    #[test]
    fn a_unit_square_covers_exactly_its_pixel() {
        let square = [
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(1.0, 2.0),
        ];
        let mut want = vec![0.0; 16];
        want[5] = 1.0;
        for points in [square.to_vec(), square.iter().rev().copied().collect()] {
            let mut rasterizer = Rasterizer::new(4, 4);
            polygon(&mut rasterizer, &points);
            assert_close(&rasterizer.coverage(), &want, 1e-6, "unit square");
        }
    }

    /// **A square offset half a pixel covers each pixel it straddles by half.**
    #[test]
    fn a_half_covered_pixel_is_half_covered() {
        let mut rasterizer = Rasterizer::new(3, 1);
        polygon(
            &mut rasterizer,
            &[
                Vec2::new(0.5, 0.0),
                Vec2::new(1.5, 0.0),
                Vec2::new(1.5, 1.0),
                Vec2::new(0.5, 1.0),
            ],
        );
        assert_close(&rasterizer.coverage(), &[0.5, 0.5, 0.0], 1e-6, "half");
        let mut bytes = Vec::new();
        rasterizer.write_mask(&mut bytes);
        assert_eq!(bytes, [128, 128, 0]);
    }

    /// **A triangle's coverage is its clipped area in every pixel**: a right
    /// triangle whose hypotenuse halves the diagonal pixels, and a skewed one
    /// with no edge on the grid.
    #[test]
    fn triangles_cover_their_exact_area_per_pixel() {
        let right = [Vec2::ZERO, Vec2::new(3.0, 0.0), Vec2::new(0.0, 3.0)];
        let mut rasterizer = Rasterizer::new(3, 3);
        polygon(&mut rasterizer, &right);
        let want = clipped_coverage(&right, 3, 3);
        assert_eq!(want[0], 1.0);
        assert!((want[2] - 0.5).abs() < 1e-6, "{want:?}");
        assert_close(&rasterizer.coverage(), &want, 1e-5, "right triangle");

        let skewed = [
            Vec2::new(0.3, 0.2),
            Vec2::new(3.7, 1.1),
            Vec2::new(1.4, 3.6),
        ];
        let mut rasterizer = Rasterizer::new(4, 4);
        polygon(&mut rasterizer, &skewed);
        let want = clipped_coverage(&skewed, 4, 4);
        assert_close(&rasterizer.coverage(), &want, 1e-5, "skewed triangle");
        let total: f32 = rasterizer.coverage().iter().sum();
        assert!((total - area(&skewed)).abs() < 1e-4, "{total}");
    }

    /// A circle's outline as a 256-gon of **exactly rational points on it**:
    /// `((1 - t²)/(1 + t²), 2t/(1 + t²))` is on the unit circle for every `t`,
    /// so no sine or cosine goes into the test either.
    fn circle(centre: Vec2, radius: f32) -> Vec<Vec2> {
        const QUARTER: usize = 64;
        let mut first = Vec::new();
        // The tangent half-angle substitution: `t` in `0..1` sweeps the first
        // quadrant, and each further quadrant is the first turned a right angle.
        for step in 0..QUARTER {
            let t = step as f64 / QUARTER as f64;
            let d = 1.0 + t * t;
            first.push(((1.0 - t * t) / d, 2.0 * t / d));
        }
        let mut points = Vec::new();
        for (sx, sy, swap) in [
            (1.0, 1.0, false),
            (-1.0, 1.0, true),
            (-1.0, -1.0, false),
            (1.0, -1.0, true),
        ] {
            for &(x, y) in &first {
                let (x, y) = if swap { (y, x) } else { (x, y) };
                points.push(Vec2::new(
                    centre.x + radius * (sx * x) as f32,
                    centre.y + radius * (sy * y) as f32,
                ));
            }
        }
        points
    }

    /// **A circle drawn as its outline covers each pixel by that outline's
    /// clipped area**, and its whole area is πr² to within what the polygon
    /// cuts off.
    #[test]
    fn a_circle_outline_covers_its_exact_area_per_pixel() {
        let (centre, radius) = (Vec2::new(6.1, 5.7), 5.3);
        let points = circle(centre, radius);
        for point in &points {
            assert!(((*point - centre).length() - radius).abs() < 1e-4);
        }
        let mut rasterizer = Rasterizer::new(13, 12);
        polygon(&mut rasterizer, &points);
        let coverage = rasterizer.coverage();
        assert_close(
            &coverage,
            &clipped_coverage(&points, 13, 12),
            1e-4,
            "circle",
        );
        let total: f32 = coverage.iter().sum();
        let disc = core::f32::consts::PI * radius * radius;
        // A 256-gon inscribed in the circle falls short of it by well under a
        // thousandth of its area.
        assert!(
            total < disc && disc - total < disc * 1e-3,
            "{total} vs {disc}"
        );
    }

    /// Coverage of each pixel by `inside`, from `samples × samples` point
    /// tests a pixel.
    fn sampled(
        width: u32,
        height: u32,
        samples: u32,
        inside: impl Fn(f32, f32) -> bool,
    ) -> Vec<f32> {
        let mut out = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let mut hits = 0;
                for sy in 0..samples {
                    for sx in 0..samples {
                        let px = x as f32 + (sx as f32 + 0.5) / samples as f32;
                        let py = y as f32 + (sy as f32 + 0.5) / samples as f32;
                        hits += u32::from(inside(px, py));
                    }
                }
                out.push(hits as f32 / (samples * samples) as f32);
            }
        }
        out
    }

    /// **A quadratic and a cubic cover what their implicit curves bound**, to
    /// within the flattening tolerance and the sampling of the expectation.
    #[test]
    fn curves_cover_the_area_their_equations_bound() {
        const SAMPLES: u32 = 256;
        let within = FLATTEN_TOLERANCE + 2.0 / SAMPLES as f32;

        // (0,4) → control (0,0) → (4,0) is x = 4t², y = 4(1 - t)², the curve
        // √x + √y = 2. Closed through (4,4), the region is √x + √y ≥ 2.
        let mut rasterizer = Rasterizer::new(4, 4);
        rasterizer.quadratic(Vec2::new(0.0, 4.0), Vec2::ZERO, Vec2::new(4.0, 0.0));
        rasterizer.line(Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0));
        rasterizer.line(Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0));
        let want = sampled(4, 4, SAMPLES, |x, y| x.sqrt() + y.sqrt() >= 2.0);
        assert_close(&rasterizer.coverage(), &want, within, "quadratic");

        // Controls at x = 0, 1, 2, 3 make x = 3t, and y controls 0, 0, 0, 3
        // make y = 3t³: the curve y = x³/9. Closed along the bottom, the
        // region is y ≤ x³/9.
        let mut rasterizer = Rasterizer::new(3, 3);
        rasterizer.cubic(
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(3.0, 3.0),
        );
        rasterizer.line(Vec2::new(3.0, 3.0), Vec2::new(3.0, 0.0));
        rasterizer.line(Vec2::new(3.0, 0.0), Vec2::ZERO);
        let want = sampled(3, 3, SAMPLES, |x, y| y <= x * x * x / 9.0);
        assert_close(&rasterizer.coverage(), &want, within, "cubic");
    }

    /// **Wang's formula gives the step counts its bound says**, and the chords
    /// it produces really do stay within the tolerance of the curve.
    #[test]
    fn wangs_formula_gives_known_counts_and_keeps_its_bound() {
        // d = 2, M = 8, tolerance 0.5: sqrt(2 · 8 / 4) = 2.
        assert_eq!(wang_segments(2, 8.0, 0.5), 2);
        // d = 3, M = 8, tolerance 0.25: sqrt(6 · 8 / 2) = 4.899 → 5.
        assert_eq!(wang_segments(3, 8.0, 0.25), 5);
        assert_eq!(
            wang_segments(2, 0.0, 0.5),
            1,
            "a straight curve is one line"
        );
        assert_eq!(wang_segments(3, 1.0e9, 0.001), MAX_CURVE_SEGMENTS);
        assert_eq!(wang_segments(2, f32::NAN, 0.5), 1);

        let (p0, p1, p2) = (Vec2::ZERO, Vec2::new(10.0, 30.0), Vec2::new(40.0, 0.0));
        let steps = wang_segments(2, (p0 - 2.0 * p1 + p2).length(), FLATTEN_TOLERANCE);
        let at = |t: f32| p0 * ((1.0 - t) * (1.0 - t)) + p1 * (2.0 * t * (1.0 - t)) + p2 * (t * t);
        let mut worst = 0.0f32;
        for step in 0..steps {
            let (t0, t1) = (step as f32 / steps as f32, (step + 1) as f32 / steps as f32);
            let (a, b) = (at(t0), at(t1));
            for sample in 1..32 {
                let t = t0 + (t1 - t0) * sample as f32 / 32.0;
                let point = at(t);
                let along = (b - a).normalize();
                let off = (point - a) - along * (point - a).dot(along);
                worst = worst.max(off.length());
            }
        }
        assert!(worst <= FLATTEN_TOLERANCE, "{worst} over {steps} steps");
    }

    /// **Two overlapping contours of one winding saturate; opposite windings
    /// cut a hole.**
    #[test]
    fn overlaps_saturate_and_opposite_windings_cut_holes() {
        let outer = [
            Vec2::ZERO,
            Vec2::new(3.0, 0.0),
            Vec2::new(3.0, 3.0),
            Vec2::new(0.0, 3.0),
        ];
        let inner = [
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(1.0, 2.0),
        ];
        let mut same = Rasterizer::new(3, 3);
        polygon(&mut same, &outer);
        polygon(&mut same, &inner);
        assert_close(&same.coverage(), &[1.0; 9], 1e-6, "same winding");

        let mut hole = Rasterizer::new(3, 3);
        polygon(&mut hole, &outer);
        let reversed: Vec<Vec2> = inner.iter().rev().copied().collect();
        polygon(&mut hole, &reversed);
        let mut want = [1.0; 9];
        want[4] = 0.0;
        assert_close(&hole.coverage(), &want, 1e-6, "hole");
    }

    /// **Geometry past the mask's edges clips cleanly**: a square hanging off
    /// every side covers the mask fully, and one wholly outside covers nothing.
    #[test]
    fn geometry_outside_the_mask_is_clipped() {
        let mut rasterizer = Rasterizer::new(2, 2);
        polygon(
            &mut rasterizer,
            &[
                Vec2::new(-3.0, -1.5),
                Vec2::new(4.5, -2.0),
                Vec2::new(5.0, 6.0),
                Vec2::new(-2.0, 4.0),
            ],
        );
        assert_close(&rasterizer.coverage(), &[1.0; 4], 1e-5, "overhang");

        let mut rasterizer = Rasterizer::new(2, 2);
        polygon(
            &mut rasterizer,
            &[
                Vec2::new(5.0, 0.0),
                Vec2::new(6.0, 0.0),
                Vec2::new(6.0, 1.0),
                Vec2::new(5.0, 1.0),
            ],
        );
        assert_close(&rasterizer.coverage(), &[0.0; 4], 0.0, "outside");
    }

    /// **Coverage is stored linear**: the byte is `round(coverage × 255)` at
    /// every level, with no curve bending the middle, and it is monotone.
    #[test]
    fn coverage_bytes_are_linear_and_monotone() {
        assert_eq!(coverage_byte(0.0), 0);
        assert_eq!(coverage_byte(0.5), 128);
        assert_eq!(coverage_byte(1.0), 255);
        assert_eq!(coverage_byte(-0.25), 0);
        assert_eq!(coverage_byte(1.5), 255);
        assert_eq!(coverage_byte(f32::NAN), 0);
        let mut last = 0;
        for level in 0..=1024 {
            let coverage = level as f32 / 1024.0;
            let byte = coverage_byte(coverage);
            assert!(byte >= last);
            assert!(
                (f32::from(byte) / 255.0 - coverage).abs() <= 0.5 / 255.0 + 1e-6,
                "{coverage} stored as {byte}"
            );
            last = byte;
        }
    }
}
