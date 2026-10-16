//! Filter effects on pixels: the primitives a [`Plan`] chains, run over the
//! filter region of a layer, as the Filter Effects Module Level 1 defines
//! them. Reading a `filter` element into a plan is the renderer's
//! (`render/filter.rs`); this module only computes.
//!
//! # What is here
//!
//! The drop-shadow set icon themes use: `feGaussianBlur`, `feOffset`,
//! `feFlood`, `feComposite` with every operator, `feColorMatrix` with every
//! type, and `feMerge`.
//!
//! # Pixels
//!
//! A filter runs on the whole pixels its region covers in the canvas's grid.
//! Every image is premultiplied RGBA in `0..=1` held as `f32`, so a chain of
//! primitives rounds to bytes once, at its end. Each primitive works in its
//! `color-interpolation-filters` space, linearRGB or sRGB: an input in the
//! other space is converted first, on unpremultiplied colour as the
//! specification requires, by the sRGB transfer function itself. Outside a
//! primitive's subregion its result is transparent black.
//!
//! # Gaussian blur
//!
//! A deviation of [`BOX_BLUR_FROM`] pixels or more is approximated as the
//! specification describes, by three successive box blurs of the width
//! [`box_width`] derives from it; a smaller one, where three boxes fit a
//! Gaussian poorly, is convolved with the Gaussian kernel itself, much as
//! resvg runs an IIR approximation of it there. Each box sums through a
//! prefix sum, so its cost does not grow with its width, and no deviation
//! makes a blur slower than a narrow one. Each box blurs what the one
//! before it left inside the region, as resvg's do, so what a box carries
//! past the region's edge is lost to the next.

use std::borrow::Cow;

/// A premultiplied RGBA pixel, each channel in `0..=1`.
pub(super) type Pixel = [f32; 4];

const TRANSPARENT: Pixel = [0.0; 4];

/// The most primitives one filter runs, counting each `feMergeNode`: each
/// result holds a buffer the size of the filter region, at most the canvas,
/// so this bounds a filter's memory to a fixed multiple of the image the
/// caller's limits allowed.
pub(super) const MAX_PRIMITIVES: usize = 16;

/// The deviation, in pixels, from which a blur is the specification's three
/// boxes rather than the Gaussian kernel.
pub(super) const BOX_BLUR_FROM: f64 = 2.0;

/// The widest box a blur sums; a wider one is read as this. It keeps the
/// box arithmetic in range for any deviation a file can write, and a box
/// this wide already spans any region a canvas can hold many times over.
const MAX_BOX_WIDTH: u32 = u32::MAX;

/// A rectangle of pixels in the filter region: columns `x0..x1` and rows
/// `y0..y1`, counted from the region's top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Area {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Area {
    fn contains(self, x: usize, y: usize) -> bool {
        (self.x0..self.x1).contains(&x) && (self.y0..self.y1).contains(&y)
    }
}

/// Where a primitive takes an image from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Input {
    /// The element as drawn.
    SourceGraphic,
    /// The element's alpha, its colour black.
    SourceAlpha,
    /// The result of the primitive at this index in the plan.
    Result(usize),
}

/// `feComposite`'s `operator`: Porter and Duff's, of the first input over
/// the second, or the arithmetic combination with `k1` to `k4`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Operator {
    Over,
    In,
    Out,
    Atop,
    Xor,
    Arithmetic([f32; 4]),
}

/// What a primitive computes, its lengths already in pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    /// `feGaussianBlur`, with its horizontal and vertical deviation.
    GaussianBlur { input: Input, deviation: (f64, f64) },
    /// `feOffset`.
    Offset { input: Input, dx: f64, dy: f64 },
    /// `feFlood`, with its colour premultiplied, in sRGB.
    Flood(Pixel),
    /// `feComposite`: `inputs[0]` (`in`) combined with `inputs[1]` (`in2`).
    Composite {
        inputs: [Input; 2],
        operator: Operator,
    },
    /// `feColorMatrix`, its type expressed as the matrix it stands for.
    ColorMatrix { input: Input, matrix: [f32; 20] },
    /// `feMerge`: each input over the ones before it.
    Merge(Vec<Input>),
}

/// One step of a filter.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Primitive {
    pub kind: Kind,
    /// Whether it works in linearRGB rather than sRGB.
    pub linear: bool,
    /// Its subregion, outside which its result is transparent.
    pub area: Area,
}

/// A filter ready to run over a region `width` by `height` pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plan {
    pub width: usize,
    pub height: usize,
    pub primitives: Vec<Primitive>,
}

/// The colour space an image's values are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    Srgb,
    Linear,
}

/// One primitive's result.
struct Image {
    pixels: Vec<Pixel>,
    space: Space,
}

/// Runs `plan` over `source`, the region's pixels of the element as drawn,
/// row by row, and returns the last primitive's result in sRGB: the
/// filtered element.
pub(super) fn run(plan: &Plan, source: &[Pixel]) -> Vec<Pixel> {
    let (width, height) = (plan.width, plan.height);
    // Each result is dropped after the last primitive that reads it.
    let mut last_use: Vec<usize> = (0..plan.primitives.len()).collect();
    for (index, primitive) in plan.primitives.iter().enumerate() {
        for input in primitive.kind.inputs() {
            if let Input::Result(from) = input {
                last_use[from] = index;
            }
        }
    }
    let mut results: Vec<Option<Image>> = Vec::with_capacity(plan.primitives.len());
    for (index, primitive) in plan.primitives.iter().enumerate() {
        let space = if primitive.linear {
            Space::Linear
        } else {
            Space::Srgb
        };
        let get = |input: Input| fetch(input, source, &results, space);
        let (mut pixels, space) = match &primitive.kind {
            Kind::GaussianBlur { input, deviation } => {
                let mut pixels = get(*input).into_owned();
                blur(&mut pixels, width, height, *deviation);
                (pixels, space)
            }
            Kind::Offset { input, dx, dy } => {
                (offset(&get(*input), width, height, *dx, *dy), space)
            }
            Kind::Flood(color) => (vec![*color; width * height], Space::Srgb),
            Kind::Composite { inputs, operator } => {
                let (a, b) = (get(inputs[0]), get(inputs[1]));
                let pixels = a
                    .iter()
                    .zip(b.iter())
                    .map(|(a, b)| composite(*operator, *a, *b))
                    .collect();
                (pixels, space)
            }
            Kind::ColorMatrix { input, matrix } => {
                let pixels = get(*input)
                    .iter()
                    .map(|pixel| color_matrix(matrix, *pixel))
                    .collect();
                (pixels, space)
            }
            Kind::Merge(inputs) => {
                let mut pixels = vec![TRANSPARENT; width * height];
                for input in inputs {
                    for (below, above) in pixels.iter_mut().zip(get(*input).iter()) {
                        *below = composite(Operator::Over, *above, *below);
                    }
                }
                (pixels, space)
            }
        };
        for (at, pixel) in pixels.iter_mut().enumerate() {
            if !primitive
                .area
                .contains(at % width.max(1), at / width.max(1))
            {
                *pixel = TRANSPARENT;
            }
        }
        for (from, last) in last_use.iter().enumerate().take(index) {
            if *last == index {
                results[from] = None;
            }
        }
        results.push(Some(Image { pixels, space }));
    }
    match results.pop().flatten() {
        Some(image) => convert(image.pixels, image.space, Space::Srgb),
        None => vec![TRANSPARENT; width * height],
    }
}

impl Kind {
    /// The inputs it reads.
    pub fn inputs(&self) -> Vec<Input> {
        match self {
            Self::GaussianBlur { input, .. }
            | Self::Offset { input, .. }
            | Self::ColorMatrix { input, .. } => vec![*input],
            Self::Flood(_) => Vec::new(),
            Self::Composite { inputs, .. } => inputs.to_vec(),
            Self::Merge(inputs) => inputs.clone(),
        }
    }
}

/// An input's pixels in `space`.
fn fetch<'a>(
    input: Input,
    source: &'a [Pixel],
    results: &'a [Option<Image>],
    space: Space,
) -> Cow<'a, [Pixel]> {
    let (pixels, from) = match input {
        Input::SourceGraphic => (Cow::Borrowed(source), Space::Srgb),
        Input::SourceAlpha => (
            Cow::Owned(source.iter().map(|p| [0.0, 0.0, 0.0, p[3]]).collect()),
            Space::Srgb,
        ),
        Input::Result(index) => match results.get(index).and_then(Option::as_ref) {
            Some(image) => (Cow::Borrowed(image.pixels.as_slice()), image.space),
            // A plan only names earlier results, and none is dropped before
            // its last reader; transparency is what a missing one would be.
            None => (Cow::Owned(vec![TRANSPARENT; source.len()]), space),
        },
    };
    if from == space {
        pixels
    } else {
        Cow::Owned(convert(pixels.into_owned(), from, space))
    }
}

/// sRGB's transfer function inverted: an encoded value to a linear one.
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB's transfer function: a linear value to an encoded one.
fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// `pixels` from the space `from` to `to`, each converted on its
/// unpremultiplied colour.
fn convert(mut pixels: Vec<Pixel>, from: Space, to: Space) -> Vec<Pixel> {
    let transfer = match (from, to) {
        (Space::Srgb, Space::Linear) => srgb_to_linear,
        (Space::Linear, Space::Srgb) => linear_to_srgb,
        _ => return pixels,
    };
    for pixel in &mut pixels {
        let [r, g, b, a] = unpremultiply(*pixel);
        *pixel = premultiply([transfer(r), transfer(g), transfer(b), a]);
    }
    pixels
}

/// A premultiplied pixel's straight colour, each channel in `0..=1`; a
/// transparent pixel's is black.
fn unpremultiply(pixel: Pixel) -> Pixel {
    let a = pixel[3];
    if a <= 0.0 {
        return TRANSPARENT;
    }
    [
        (pixel[0] / a).clamp(0.0, 1.0),
        (pixel[1] / a).clamp(0.0, 1.0),
        (pixel[2] / a).clamp(0.0, 1.0),
        a,
    ]
}

fn premultiply([r, g, b, a]: Pixel) -> Pixel {
    [r * a, g * a, b * a, a]
}

/// `feComposite` of `a` (`in`) with `b` (`in2`) at one pixel.
fn composite(operator: Operator, a: Pixel, b: Pixel) -> Pixel {
    let (fa, fb) = match operator {
        Operator::Over => (1.0, 1.0 - a[3]),
        Operator::In => (b[3], 0.0),
        Operator::Out => (1.0 - b[3], 0.0),
        Operator::Atop => (b[3], 1.0 - a[3]),
        Operator::Xor => (1.0 - b[3], 1.0 - a[3]),
        Operator::Arithmetic([k1, k2, k3, k4]) => {
            let value = |c: usize| k1 * a[c] * b[c] + k2 * a[c] + k3 * b[c] + k4;
            // Clamped to `0..=1`, and the colour to the alpha so the result
            // stays a premultiplied colour.
            let alpha = value(3).clamp(0.0, 1.0);
            return [
                value(0).clamp(0.0, alpha),
                value(1).clamp(0.0, alpha),
                value(2).clamp(0.0, alpha),
                alpha,
            ];
        }
    };
    [
        a[0] * fa + b[0] * fb,
        a[1] * fa + b[1] * fb,
        a[2] * fa + b[2] * fb,
        a[3] * fa + b[3] * fb,
    ]
}

/// The matrix that leaves every pixel as it is: `feColorMatrix`'s default.
pub(super) const IDENTITY_MATRIX: [f32; 20] = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0,
];

/// `type="luminanceToAlpha"`: the alpha is the luminance, by the
/// specification's coefficients, and the colour black.
pub(super) const LUMINANCE_TO_ALPHA: [f32; 20] = [
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.2125, 0.7154, 0.0721, 0.0, 0.0,
];

/// `type="saturate"` with the value `s`, the specification's matrix.
pub(super) fn saturate(s: f32) -> [f32; 20] {
    [
        0.213 + 0.787 * s,
        0.715 - 0.715 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 + 0.285 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 - 0.715 * s,
        0.072 + 0.928 * s,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// `type="hueRotate"` by `degrees`, the specification's matrix.
pub(super) fn hue_rotate(degrees: f32) -> [f32; 20] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        0.213 + 0.787 * cos - 0.213 * sin,
        0.715 - 0.715 * cos - 0.715 * sin,
        0.072 - 0.072 * cos + 0.928 * sin,
        0.0,
        0.0,
        0.213 - 0.213 * cos + 0.143 * sin,
        0.715 + 0.285 * cos + 0.140 * sin,
        0.072 - 0.072 * cos - 0.283 * sin,
        0.0,
        0.0,
        0.213 - 0.213 * cos - 0.787 * sin,
        0.715 - 0.715 * cos + 0.715 * sin,
        0.072 + 0.928 * cos + 0.072 * sin,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// `feColorMatrix` at one pixel: the matrix applied to its unpremultiplied
/// colour, each result clamped to `0..=1`.
fn color_matrix(matrix: &[f32; 20], pixel: Pixel) -> Pixel {
    let [r, g, b, a] = unpremultiply(pixel);
    let row = |i: usize| {
        let m = &matrix[i * 5..i * 5 + 5];
        (m[0] * r + m[1] * g + m[2] * b + m[3] * a + m[4]).clamp(0.0, 1.0)
    };
    premultiply([row(0), row(1), row(2), row(3)])
}

/// `feOffset`: `pixels` moved `dx` right and `dy` down, a fractional
/// distance interpolated linearly between the pixels either side.
fn offset(pixels: &[Pixel], width: usize, height: usize, dx: f64, dy: f64) -> Vec<Pixel> {
    let (whole_x, whole_y) = (dx.floor(), dy.floor());
    let (fx, fy) = ((dx - whole_x) as f32, (dy - whole_y) as f32);
    // `as` saturates a distance too far to matter, which then reads nothing.
    let (whole_x, whole_y) = (whole_x as i64, whole_y as i64);
    let at = |x: i64, y: i64| -> Pixel {
        match (usize::try_from(x), usize::try_from(y)) {
            (Ok(x), Ok(y)) if x < width && y < height => pixels[y * width + x],
            _ => TRANSPARENT,
        }
    };
    let mut out = vec![TRANSPARENT; width * height];
    for (index, pixel) in out.iter_mut().enumerate() {
        let x = (index % width) as i64 - whole_x;
        let y = (index / width) as i64 - whole_y;
        let taps = [
            (at(x, y), (1.0 - fx) * (1.0 - fy)),
            (at(x.saturating_sub(1), y), fx * (1.0 - fy)),
            (at(x, y.saturating_sub(1)), (1.0 - fx) * fy),
            (at(x.saturating_sub(1), y.saturating_sub(1)), fx * fy),
        ];
        for (tap, weight) in taps {
            for (channel, value) in pixel.iter_mut().zip(tap) {
                *channel += value * weight;
            }
        }
    }
    out
}

/// The box width the specification derives from a deviation `s` in pixels,
/// `d = ⌊s · 3√(2π) / 4 + 0.5⌋`, at most [`MAX_BOX_WIDTH`].
pub(super) fn box_width(s: f64) -> usize {
    let d = (s * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor();
    // `as` sends NaN to zero, and the cap fits a 32-bit `usize` too.
    d.min(f64::from(MAX_BOX_WIDTH)) as usize
}

/// The `(left, right, width)` of the three box blurs the specification
/// makes of a width `d`: three boxes of `d` centred on the output pixel if
/// `d` is odd; otherwise one of `d` centred on its left edge, one on its
/// right edge, and one of `d + 1` centred on it. `left` and `right` count
/// the pixels each box reaches either side.
pub(super) fn boxes(d: usize) -> [(usize, usize, usize); 3] {
    let half = d / 2;
    if d % 2 == 1 {
        [(half, half, d); 3]
    } else {
        [
            (half, half.saturating_sub(1), d),
            (half.saturating_sub(1), half, d),
            (half, half, d.saturating_add(1)),
        ]
    }
}

/// `feGaussianBlur` of `pixels`, `width` by `height`, by a horizontal and a
/// vertical deviation in pixels; one of zero or less leaves that direction
/// unblurred.
fn blur(pixels: &mut [Pixel], width: usize, height: usize, (sx, sy): (f64, f64)) {
    blur_lines(pixels, height, width, 1, width, sx);
    blur_lines(pixels, width, height, width, 1, sy);
}

/// Blurs `lines` lines of `length` pixels each, the pixels `step` apart and
/// the lines `stride` apart, by `deviation`.
fn blur_lines(
    pixels: &mut [Pixel],
    lines: usize,
    length: usize,
    step: usize,
    stride: usize,
    deviation: f64,
) {
    if deviation <= 0.0 || length == 0 {
        return;
    }
    let kernel = (deviation < BOX_BLUR_FROM).then(|| gaussian_kernel(deviation));
    let boxes = boxes(box_width(deviation));
    let mut line = vec![TRANSPARENT; length];
    let mut out = vec![TRANSPARENT; length];
    let mut prefix = vec![[0.0f64; 4]; length + 1];
    for index in 0..lines {
        let start = index * stride;
        for (at, pixel) in line.iter_mut().enumerate() {
            *pixel = pixels[start + at * step];
        }
        match &kernel {
            Some(kernel) => convolve(&line, &mut out, kernel),
            None => {
                box_pass(&line, &mut out, &mut prefix, boxes[0]);
                box_pass(&out, &mut line, &mut prefix, boxes[1]);
                box_pass(&line, &mut out, &mut prefix, boxes[2]);
            }
        }
        for (at, pixel) in out.iter().enumerate() {
            pixels[start + at * step] = *pixel;
        }
    }
}

/// One box blur of `src` into `dst`: each output the sum of the inputs
/// from `left` before it to `right` after it, transparent past the ends,
/// over `width`.
fn box_pass(
    src: &[Pixel],
    dst: &mut [Pixel],
    prefix: &mut [[f64; 4]],
    (left, right, width): (usize, usize, usize),
) {
    let mut sum = [0.0f64; 4];
    prefix[0] = sum;
    for (at, pixel) in src.iter().enumerate() {
        for (total, value) in sum.iter_mut().zip(pixel) {
            *total += f64::from(*value);
        }
        prefix[at + 1] = sum;
    }
    let width = width as f64;
    for (at, pixel) in dst.iter_mut().enumerate() {
        let low = at.saturating_sub(left);
        let high = at.saturating_add(right).saturating_add(1).min(src.len());
        for (channel, (high, low)) in pixel.iter_mut().zip(prefix[high].iter().zip(prefix[low])) {
            *channel = ((high - low) / width) as f32;
        }
    }
}

/// The Gaussian of `deviation`, sampled at whole pixels out to three
/// deviations each side and normalised to sum to one.
fn gaussian_kernel(deviation: f64) -> Vec<f32> {
    let radius = (3.0 * deviation).ceil() as i64;
    let weights: Vec<f64> = (-radius..=radius)
        .map(|x| (-((x * x) as f64) / (2.0 * deviation * deviation)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    weights
        .iter()
        .map(|weight| (weight / total) as f32)
        .collect()
}

/// `src` convolved with the centred `kernel` into `dst`, transparent past
/// the ends.
fn convolve(src: &[Pixel], dst: &mut [Pixel], kernel: &[f32]) {
    let radius = kernel.len() / 2;
    for (at, pixel) in dst.iter_mut().enumerate() {
        *pixel = TRANSPARENT;
        for (tap, weight) in kernel.iter().enumerate() {
            let Some(from) = (at + tap).checked_sub(radius) else {
                continue;
            };
            let Some(value) = src.get(from) else {
                break;
            };
            for (channel, value) in pixel.iter_mut().zip(value) {
                *channel += value * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: f32, want: f32, why: &str) {
        assert!((got - want).abs() < 1e-5, "{why}: {got} want {want}");
    }

    fn close_pixel(got: Pixel, want: Pixel, why: &str) {
        for (got, want) in got.into_iter().zip(want) {
            close(got, want, why);
        }
    }

    /// A row `length` long, transparent but for opaque white at `at`.
    fn impulse(length: usize, at: usize) -> Vec<Pixel> {
        let mut row = vec![TRANSPARENT; length];
        row[at] = [1.0; 4];
        row
    }

    /// **The box widths are the specification's formula**: `d = ⌊s · 3√(2π)/4
    /// + 0.5⌋`, which is 4 at a deviation of 2, 5 at 2.6 and 6 at 3; and an
    /// even width splits into boxes either side of the pixel and one of
    /// `d + 1`.
    #[test]
    fn box_widths_follow_the_formula() {
        // 3√(2π)/4 = 1.879971…
        assert_eq!(box_width(2.0), 4, "2 · 1.88 + 0.5 = 4.26");
        assert_eq!(box_width(2.6), 5, "2.6 · 1.88 + 0.5 = 5.39");
        assert_eq!(box_width(3.0), 6, "3 · 1.88 + 0.5 = 6.14");
        assert_eq!(box_width(2.392), 4, "2.392 · 1.88 + 0.5 = 4.997");
        assert_eq!(box_width(2.394), 5, "2.394 · 1.88 + 0.5 = 5.0006");
        assert_eq!(box_width(1e300), MAX_BOX_WIDTH as usize);
        assert_eq!(boxes(5), [(2, 2, 5); 3]);
        assert_eq!(boxes(4), [(2, 1, 4), (1, 2, 4), (2, 2, 5)]);
    }

    /// **Three boxes of an odd width blur an impulse into their
    /// convolution**: for three boxes of 5 the centre takes the 19 ways
    /// three offsets in `-2..=2` sum to zero, of 125, and the farthest
    /// pixel, six away, the one way.
    #[test]
    fn three_odd_boxes_convolve_an_impulse() {
        let mut row = impulse(21, 10);
        // A deviation of 2.6 is a width of 5.
        blur(&mut row, 21, 1, (2.6, 0.0));
        close(row[10][3], 19.0 / 125.0, "centre");
        close(row[4][3], 1.0 / 125.0, "six left");
        close(row[16][3], 1.0 / 125.0, "six right");
        close(row[3][3], 0.0, "seven left");
        close(row.iter().map(|p| p[3]).sum(), 1.0, "the total is kept");
    }

    /// **An even width's three boxes are offset as the specification
    /// says**: for a width of 4 the boxes reach `-2..=1`, `-1..=2` and
    /// `-2..=2`, so the centre takes the 14 of their 80 combinations that
    /// sum to zero, the reach is five each side, and the kernel is
    /// symmetric.
    #[test]
    fn three_even_boxes_convolve_an_impulse() {
        let mut row = impulse(21, 10);
        blur(&mut row, 21, 1, (2.0, 0.0));
        close(row[10][3], 14.0 / 80.0, "centre");
        close(row[5][3], 1.0 / 80.0, "five left");
        close(row[15][3], 1.0 / 80.0, "five right");
        close(row[4][3], 0.0, "six left");
        // One away: a + b + c = ±1 has 13 combinations each way.
        close(row[9][3], 13.0 / 80.0, "one left");
        close(row[11][3], 13.0 / 80.0, "one right");
    }

    /// **A deviation under two is the Gaussian kernel itself**: at one, the
    /// centre takes `1 / Σ e^(−x²/2)` over `x` in `-3..=3`.
    #[test]
    fn small_deviations_are_gaussian() {
        let mut row = impulse(15, 7);
        blur(&mut row, 15, 1, (1.0, 0.0));
        let total = 1.0 + 2.0 * ((-0.5f64).exp() + (-2.0f64).exp() + (-4.5f64).exp());
        close(row[7][3], (1.0 / total) as f32, "centre");
        close(row[8][3], ((-0.5f64).exp() / total) as f32, "one away");
        close(row[10][3], ((-4.5f64).exp() / total) as f32, "three away");
        close(row[11][3], 0.0, "four away");
    }

    /// **Each direction blurs by its own deviation**, and a zero leaves its
    /// direction alone: a column impulse blurred by `0 2.6` spreads down the
    /// column only.
    #[test]
    fn each_direction_has_its_own_deviation() {
        let (width, height) = (3, 21);
        let mut image = vec![TRANSPARENT; width * height];
        image[10 * width + 1] = [1.0; 4];
        blur(&mut image, width, height, (0.0, 2.6));
        close(image[10 * width + 1][3], 19.0 / 125.0, "centre");
        close(image[4 * width + 1][3], 1.0 / 125.0, "six up");
        close(image[10 * width][3], 0.0, "nothing sideways");
        let mut wide = vec![TRANSPARENT; height * width];
        wide[width * 10 + 1] = [1.0; 4];
        let mut still = wide.clone();
        blur(&mut wide, width, height, (2.6, 0.0));
        // Past the ends of the three-pixel row is transparent, neither
        // wrapped round nor the edge repeated, and each box blurs what the
        // one before left in the row: the first gives each pixel a fifth of
        // the impulse, and the next two a fifth of the row's sum each.
        for pixel in &wide[10 * width..11 * width] {
            close(pixel[3], 9.0 / 125.0, "what the row keeps");
        }
        close(wide[9 * width + 1][3], 0.0, "nothing up");
        blur(&mut still, width, height, (0.0, 0.0));
        close(still[10 * width + 1][3], 1.0, "no deviation, no blur");
    }

    /// **A huge deviation spreads an impulse to nothing quickly**, the box
    /// width capped and every box summed from a prefix sum.
    #[test]
    fn a_huge_deviation_neither_hangs_nor_overflows() {
        let mut row = impulse(64, 32);
        blur(&mut row, 64, 1, (1e300, 0.0));
        assert!(row.iter().all(|p| p[3] >= 0.0 && p[3] < 1e-6));
    }

    /// **Each Porter–Duff operator is its formula**, half-opaque red (`in`)
    /// with half-opaque blue (`in2`), and the arithmetic operator combines by
    /// its four coefficients, clamped to `0..=1` and the colour to the alpha.
    #[test]
    fn composite_operators_follow_their_formulas() {
        let a = [0.5, 0.0, 0.0, 0.5];
        let b = [0.0, 0.0, 0.5, 0.5];
        close_pixel(
            composite(Operator::Over, a, b),
            [0.5, 0.0, 0.25, 0.75],
            "over: A + B(1 - αA)",
        );
        close_pixel(
            composite(Operator::In, a, b),
            [0.25, 0.0, 0.0, 0.25],
            "in: A αB",
        );
        close_pixel(
            composite(Operator::Over, [1.0, 0.0, 0.0, 1.0], b),
            [1.0, 0.0, 0.0, 1.0],
            "over: opaque A hides B",
        );
        close_pixel(
            composite(Operator::Out, a, b),
            [0.25, 0.0, 0.0, 0.25],
            "out: A (1 - αB)",
        );
        let opaque_b = [0.0, 0.0, 1.0, 1.0];
        close_pixel(
            composite(Operator::Out, a, opaque_b),
            TRANSPARENT,
            "out of opaque",
        );
        close_pixel(
            composite(Operator::In, a, opaque_b),
            a,
            "in opaque keeps A whole",
        );
        close_pixel(
            composite(Operator::Atop, a, b),
            [0.25, 0.0, 0.25, 0.5],
            "atop: A αB + B (1 - αA)",
        );
        close_pixel(
            composite(Operator::Atop, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.25, 0.25]),
            [0.25, 0.0, 0.0, 0.25],
            "atop takes B's alpha",
        );
        close_pixel(
            composite(Operator::Xor, a, b),
            [0.25, 0.0, 0.25, 0.5],
            "xor: A (1 - αB) + B (1 - αA)",
        );
        close_pixel(
            composite(Operator::Xor, [1.0, 0.0, 0.0, 1.0], b),
            [0.5, 0.0, 0.0, 0.5],
            "xor of opaque A leaves A outside B",
        );
        // k1 A B + k2 A + k3 B + k4, channel by channel.
        close_pixel(
            composite(Operator::Arithmetic([1.0, 0.5, 0.25, 0.1]), a, b),
            [0.35, 0.1, 0.225, 0.725],
            "arithmetic",
        );
        close_pixel(
            composite(Operator::Arithmetic([0.0, 0.0, 0.0, 2.0]), a, b),
            [1.0; 4],
            "arithmetic clamps to one",
        );
        close_pixel(
            composite(Operator::Arithmetic([0.0, 1.0, 0.0, -0.25]), a, b),
            [0.25, 0.0, 0.0, 0.25],
            "arithmetic clamps the colour to the alpha and to zero",
        );
        close_pixel(
            composite(
                Operator::Arithmetic([0.0, 1.0, -0.5, 0.0]),
                a,
                [0.0, 0.0, 0.0, 1.0],
            ),
            TRANSPARENT,
            "arithmetic clamps a colour above the alpha to it",
        );
    }

    /// **The colour matrix applies to unpremultiplied colour and clamps**:
    /// halving red on half-opaque red gives a quarter premultiplied; each
    /// type's matrix is the specification's.
    #[test]
    fn colour_matrices_follow_the_specification() {
        let mut halve_red = IDENTITY_MATRIX;
        halve_red[0] = 0.5;
        close_pixel(
            color_matrix(&halve_red, [0.5, 0.0, 0.0, 0.5]),
            [0.25, 0.0, 0.0, 0.5],
            "on straight colour",
        );
        let mut offset_alpha = IDENTITY_MATRIX;
        offset_alpha[19] = 1.0;
        close_pixel(
            color_matrix(&offset_alpha, TRANSPARENT),
            [0.0, 0.0, 0.0, 1.0],
            "transparent black made opaque stays black",
        );
        let mut overflow = IDENTITY_MATRIX;
        overflow[4] = 2.0;
        close_pixel(
            color_matrix(&overflow, [0.0, 0.0, 0.0, 1.0]),
            [1.0, 0.0, 0.0, 1.0],
            "clamped to one",
        );
        let grey = color_matrix(&saturate(0.0), [1.0, 0.0, 0.0, 1.0]);
        close_pixel(
            grey,
            [0.213, 0.213, 0.213, 1.0],
            "saturate 0 is red's luminance",
        );
        close_pixel(
            color_matrix(&saturate(1.0), [0.2, 0.4, 0.6, 1.0]),
            [0.2, 0.4, 0.6, 1.0],
            "saturate 1 is the identity",
        );
        close_pixel(
            color_matrix(&saturate(0.5), [1.0, 0.0, 0.0, 1.0]),
            [0.6065, 0.1065, 0.1065, 1.0],
            "saturate ½",
        );
        close_pixel(
            color_matrix(&hue_rotate(0.0), [0.1, 0.2, 0.3, 0.5]),
            [0.1, 0.2, 0.3, 0.5],
            "a rotation of zero is the identity",
        );
        // cos 90° = 0 and sin 90° = 1: red's column is 0.213 - 0.213,
        // 0.213 + 0.143 and 0.213 - 0.787, the last clamped.
        close_pixel(
            color_matrix(&hue_rotate(90.0), [1.0, 0.0, 0.0, 1.0]),
            [0.0, 0.356, 0.0, 1.0],
            "hueRotate 90",
        );
        close_pixel(
            color_matrix(&LUMINANCE_TO_ALPHA, [0.0, 0.5, 0.0, 0.5]),
            [0.0, 0.0, 0.0, 0.7154],
            "luminanceToAlpha of half-opaque green",
        );
    }

    /// **sRGB and linearRGB convert by the transfer function on
    /// unpremultiplied colour**: half-opaque mid-grey, 0.5 encoded, is
    /// 0.214041 linear, so 0.107020 premultiplied; and back.
    #[test]
    fn colour_spaces_convert_unpremultiplied() {
        // ((0.5 + 0.055) / 1.055)^2.4
        let linear = 0.214_041_14;
        let pixels = convert(vec![[0.25, 0.25, 0.25, 0.5]], Space::Srgb, Space::Linear);
        close_pixel(
            pixels[0],
            [linear * 0.5, linear * 0.5, linear * 0.5, 0.5],
            "to linear",
        );
        let back = convert(pixels, Space::Linear, Space::Srgb);
        close_pixel(back[0], [0.25, 0.25, 0.25, 0.5], "and back");
        close(srgb_to_linear(0.04), 0.04 / 12.92, "the linear toe");
        close(linear_to_srgb(0.002), 0.002 * 12.92, "the linear toe back");
        close(
            linear_to_srgb(0.5),
            0.735_356_6,
            "1.055 · 0.5^(1/2.4) - 0.055",
        );
        assert_eq!(
            convert(vec![TRANSPARENT], Space::Srgb, Space::Linear),
            [TRANSPARENT]
        );
    }

    /// **An offset moves by whole pixels exactly and splits a fraction
    /// between neighbours.**
    #[test]
    fn offsets_move_and_interpolate() {
        let row = impulse(6, 1);
        let moved = offset(&row, 6, 1, 2.0, 0.0);
        assert_eq!(moved[3], [1.0; 4]);
        assert_eq!(moved.iter().map(|p| p[3]).sum::<f32>(), 1.0);
        let half = offset(&row, 6, 1, 0.5, 0.0);
        close_pixel(half[1], [0.5; 4], "half stays");
        close_pixel(half[2], [0.5; 4], "half moves on");
        let back = offset(&row, 6, 1, -1.25, 0.0);
        close_pixel(back[0], [0.75; 4], "three quarters a whole pixel back");
        close_pixel(
            back[5],
            TRANSPARENT,
            "the rest off the left edge wraps nowhere",
        );
        let column = offset(&impulse(6, 1), 1, 6, 0.0, 3.0);
        assert_eq!(column[4], [1.0; 4]);
        assert!(
            offset(&row, 6, 1, 1e300, 0.0)
                .iter()
                .all(|p| *p == TRANSPARENT)
        );
    }

    /// **A plan chains results by index, converts between spaces only where
    /// a primitive's differs, clears outside each subregion, and ends in
    /// sRGB**: a flood (sRGB) composited in a linear primitive over the
    /// source comes back unchanged where nothing blends.
    #[test]
    fn a_plan_chains_its_primitives() {
        let whole = Area {
            x0: 0,
            y0: 0,
            x1: 2,
            y1: 1,
        };
        let source = vec![[0.25, 0.0, 0.0, 0.5], TRANSPARENT];
        let plan = Plan {
            width: 2,
            height: 1,
            primitives: vec![
                Primitive {
                    kind: Kind::Flood([0.0, 0.0, 0.5, 0.5]),
                    linear: true,
                    area: Area { x0: 1, ..whole },
                },
                Primitive {
                    kind: Kind::Merge(vec![Input::Result(0), Input::SourceGraphic]),
                    linear: true,
                    area: whole,
                },
            ],
        };
        let out = run(&plan, &source);
        close_pixel(out[0], [0.25, 0.0, 0.0, 0.5], "the source, round trip");
        close_pixel(out[1], [0.0, 0.0, 0.5, 0.5], "the flood, in its subregion");
        // SourceAlpha is the source's alpha in black.
        let alpha = Plan {
            primitives: vec![Primitive {
                kind: Kind::Offset {
                    input: Input::SourceAlpha,
                    dx: 1.0,
                    dy: 0.0,
                },
                linear: false,
                area: whole,
            }],
            ..plan.clone()
        };
        close_pixel(run(&alpha, &source)[1], [0.0, 0.0, 0.0, 0.5], "SourceAlpha");
        // A linear primitive over a colour converts it: halving red in
        // linearRGB from a straight 0.5 is 0.214 / 2 linear, 0.3608 encoded.
        let mut halve_red = IDENTITY_MATRIX;
        halve_red[0] = 0.5;
        let matrix = |linear| Plan {
            primitives: vec![Primitive {
                kind: Kind::ColorMatrix {
                    input: Input::SourceGraphic,
                    matrix: halve_red,
                },
                linear,
                area: whole,
            }],
            ..plan.clone()
        };
        let encoded = linear_to_srgb(srgb_to_linear(0.5) / 2.0);
        close(encoded, 0.360_780_2, "0.5 halved in linear light");
        close_pixel(
            run(&matrix(true), &source)[0],
            [encoded * 0.5, 0.0, 0.0, 0.5],
            "linearRGB",
        );
        close_pixel(
            run(&matrix(false), &source)[0],
            [0.125, 0.0, 0.0, 0.5],
            "sRGB",
        );
    }
}
