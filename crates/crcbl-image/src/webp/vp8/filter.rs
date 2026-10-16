//! The loop filter, RFC 6386 section 15, as its reference decoder
//! (`dixie_loopfilter.c`, section 20.6) computes it.
//!
//! It runs over the whole frame once every macroblock is reconstructed,
//! since intra prediction reads unfiltered pixels, and macroblock by
//! macroblock in raster order, since one macroblock's edges overlap the
//! pixels its left and upper neighbours' filtering moved.

/// One macroblock's filter strength, derived from the frame header, its
/// segment and its prediction mode: RFC 6386 section 15.4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Strength {
    /// The filter level, 0 to [`MAX_LEVEL`]; zero skips the macroblock.
    pub level: i32,
    /// The limit on differences between pixels on one side of an edge.
    pub interior: i32,
    /// The difference past which an edge has high variance.
    pub hev_threshold: i32,
}

/// The largest filter level, the range of the header's six-bit field.
pub(super) const MAX_LEVEL: i32 = 63;

impl Strength {
    /// The strength of a level already adjusted for segment and mode, and
    /// clamped: RFC 6386 section 15.4's `interior_limit` and
    /// `hev_threshold`, the latter as a key frame computes it.
    pub(super) fn new(level: i32, sharpness: i32) -> Self {
        let mut interior = level;
        if sharpness > 0 {
            interior >>= if sharpness > 4 { 2 } else { 1 };
            interior = interior.min(9 - sharpness);
        }
        let hev_threshold = match level {
            40.. => 2,
            15.. => 1,
            _ => 0,
        };
        Self {
            level,
            interior: interior.max(1),
            hev_threshold,
        }
    }

    /// The edge limit between macroblocks, for both filters.
    fn macroblock_limit(self) -> i32 {
        (self.level + 2) * 2 + self.interior
    }

    /// The edge limit between subblocks, for both filters.
    fn subblock_limit(self) -> i32 {
        self.level * 2 + self.interior
    }
}

/// Which of the two filters the frame header chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Normal,
    Simple,
}

/// A plane, its rows `stride` apart.
pub(super) struct Plane<'a> {
    pub pixels: &'a mut [u8],
    pub stride: usize,
}

/// Filters one macroblock's edges in the RFC's order — its left edge, the
/// edges between its subblock columns, its top edge, the edges between its
/// subblock rows — on luma, and on chroma for the normal filter.
/// `(x, y)` is the macroblock's position in macroblocks; `inner` is whether
/// its subblock edges are filtered (it has coefficients, or is predicted
/// subblock by subblock).
pub(super) fn macroblock(
    kind: Kind,
    planes: [Plane<'_>; 3],
    x: usize,
    y: usize,
    strength: Strength,
    inner: bool,
) {
    if strength.level == 0 {
        return;
    }
    let [luma, u, v] = planes;
    match kind {
        Kind::Simple => {
            let at = (y * luma.stride + x) * 16;
            let edge = |pixels: &mut [u8], at: usize, step: usize, along: usize, limit: i32| {
                for i in 0..16 {
                    simple_segment(pixels, at + i * along, step, limit);
                }
            };
            let stride = luma.stride;
            if x > 0 {
                edge(luma.pixels, at, 1, stride, strength.macroblock_limit());
            }
            if inner {
                for column in [4, 8, 12] {
                    edge(
                        luma.pixels,
                        at + column,
                        1,
                        stride,
                        strength.subblock_limit(),
                    );
                }
            }
            if y > 0 {
                edge(luma.pixels, at, stride, 1, strength.macroblock_limit());
            }
            if inner {
                for row in [4, 8, 12] {
                    edge(
                        luma.pixels,
                        at + row * stride,
                        stride,
                        1,
                        strength.subblock_limit(),
                    );
                }
            }
        }
        Kind::Normal => {
            for (plane, size) in [(luma, 16), (u, 8), (v, 8)] {
                normal(plane, x, y, size, strength, inner);
            }
        }
    }
}

/// The normal filter over one plane's part of a macroblock, `size` pixels
/// square.
fn normal(plane: Plane<'_>, x: usize, y: usize, size: usize, strength: Strength, inner: bool) {
    let stride = plane.stride;
    let pixels = plane.pixels;
    let at = (y * stride + x) * size;
    let subblocks = (4..size).step_by(4);
    if x > 0 {
        for i in 0..size {
            macroblock_segment(pixels, at + i * stride, 1, strength);
        }
    }
    if inner {
        for column in subblocks.clone() {
            for i in 0..size {
                subblock_segment(pixels, at + column + i * stride, 1, strength);
            }
        }
    }
    if y > 0 {
        for i in 0..size {
            macroblock_segment(pixels, at + i, stride, strength);
        }
    }
    if inner {
        for row in subblocks {
            for i in 0..size {
                subblock_segment(pixels, at + row * stride + i, stride, strength);
            }
        }
    }
}

/// The eight pixels of a segment across an edge, `p3 p2 p1 p0 | q0 q1 q2
/// q3`, read from `pixels` with `q0` at `at` and neighbours `step` apart.
struct Segment {
    p: [i32; 4],
    q: [i32; 4],
}

impl Segment {
    fn read(pixels: &[u8], at: usize, step: usize) -> Self {
        Self {
            p: std::array::from_fn(|i| i32::from(pixels[at - (i + 1) * step])),
            q: std::array::from_fn(|i| i32::from(pixels[at + i * step])),
        }
    }

    /// The reference decoder's `simple_threshold`: the edge's own step,
    /// weighted against its outer neighbours.
    fn edge_below(&self, limit: i32) -> bool {
        (self.p[0] - self.q[0]).abs() * 2 + ((self.p[1] - self.q[1]).abs() >> 1) <= limit
    }

    /// The reference decoder's `normal_threshold`: the edge test and every
    /// step within each side at most `interior`.
    fn normal_below(&self, limit: i32, interior: i32) -> bool {
        self.edge_below(limit)
            && (self.p[3] - self.p[2]).abs() <= interior
            && (self.p[2] - self.p[1]).abs() <= interior
            && (self.p[1] - self.p[0]).abs() <= interior
            && (self.q[3] - self.q[2]).abs() <= interior
            && (self.q[2] - self.q[1]).abs() <= interior
            && (self.q[1] - self.q[0]).abs() <= interior
    }

    /// The reference decoder's `high_edge_variance`.
    fn high_variance(&self, threshold: i32) -> bool {
        (self.p[1] - self.p[0]).abs() > threshold || (self.q[1] - self.q[0]).abs() > threshold
    }
}

fn saturate_i8(value: i32) -> i32 {
    value.clamp(-128, 127)
}

fn saturate_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// The reference decoder's `filter_common` (the RFC's `common_adjust`, and
/// for `use_outer_taps` false the rest of its `subblock_filter`).
fn common(pixels: &mut [u8], at: usize, step: usize, segment: &Segment, use_outer_taps: bool) {
    let (p1, p0, q0, q1) = (segment.p[1], segment.p[0], segment.q[0], segment.q[1]);
    let mut a = 3 * (q0 - p0);
    if use_outer_taps {
        a += saturate_i8(p1 - q1);
    }
    let a = saturate_i8(a);
    // Rounding a / 8 up for p0 and down for q0 when it falls on a half.
    let f1 = (a + 4).min(127) >> 3;
    let f2 = (a + 3).min(127) >> 3;
    pixels[at - step] = saturate_u8(p0 + f2);
    pixels[at] = saturate_u8(q0 - f1);
    if !use_outer_taps {
        let a = (f1 + 1) >> 1;
        pixels[at - 2 * step] = saturate_u8(p1 + a);
        pixels[at + step] = saturate_u8(q1 - a);
    }
}

/// The simple filter across one segment, RFC 6386 section 15.2.
fn simple_segment(pixels: &mut [u8], at: usize, step: usize, limit: i32) {
    let segment = Segment::read(pixels, at, step);
    if segment.edge_below(limit) {
        common(pixels, at, step, &segment, true);
    }
}

/// The normal filter across one segment of an edge between subblocks, RFC
/// 6386 section 15.3's `subblock_filter`.
fn subblock_segment(pixels: &mut [u8], at: usize, step: usize, strength: Strength) {
    let segment = Segment::read(pixels, at, step);
    if segment.normal_below(strength.subblock_limit(), strength.interior) {
        let high_variance = segment.high_variance(strength.hev_threshold);
        common(pixels, at, step, &segment, high_variance);
    }
}

/// The normal filter across one segment of an edge between macroblocks, RFC
/// 6386 section 15.3's `MBfilter`: the simple adjustment where the edge has
/// high variance, and otherwise three pixels each side moved by 27, 18 and 9
/// sixty-fourths (roughly) of the edge's step.
fn macroblock_segment(pixels: &mut [u8], at: usize, step: usize, strength: Strength) {
    let segment = Segment::read(pixels, at, step);
    if !segment.normal_below(strength.macroblock_limit(), strength.interior) {
        return;
    }
    if segment.high_variance(strength.hev_threshold) {
        common(pixels, at, step, &segment, true);
        return;
    }
    let (p, q) = (segment.p, segment.q);
    let w = saturate_i8(saturate_i8(p[1] - q[1]) + 3 * (q[0] - p[0]));
    for (i, weight) in [27, 18, 9].into_iter().enumerate() {
        let a = (weight * w + 63) >> 7;
        pixels[at - (i + 1) * step] = saturate_u8(p[i] + a);
        pixels[at + i * step] = saturate_u8(q[i] - a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A segment laid out horizontally, `q0` at index 4.
    fn run(values: [u8; 8], filter: impl Fn(&mut [u8], usize, usize)) -> [u8; 8] {
        let mut pixels = values;
        filter(&mut pixels, 4, 1);
        pixels
    }

    /// Section 15.4's limits worked by hand: level 32 at sharpness 0 has an
    /// interior limit of 32; at sharpness 3, 32 >> 1 capped at 9 - 3 = 6; at
    /// sharpness 5, 32 >> 2 capped at 4; and level 1 at sharpness 6 rounds to
    /// zero, raised to one. The variance threshold steps at 15 and 40.
    #[test]
    fn strengths_follow_section_15_4() {
        assert_eq!(Strength::new(32, 0).interior, 32);
        assert_eq!(Strength::new(32, 3).interior, 6);
        assert_eq!(Strength::new(32, 5).interior, 4);
        assert_eq!(Strength::new(1, 6).interior, 1);
        assert_eq!(Strength::new(10, 1).interior, 5);
        // Sharpness 5 shifts by two before the cap: 10 >> 2 = 2, where a
        // shift by one would reach the cap of 4.
        assert_eq!(Strength::new(10, 5).interior, 2);
        let thresholds: Vec<i32> = [14, 15, 39, 40]
            .iter()
            .map(|&l| Strength::new(l, 0).hev_threshold)
            .collect();
        assert_eq!(thresholds, [0, 1, 1, 2]);
        let strength = Strength::new(20, 0);
        assert_eq!(
            (strength.macroblock_limit(), strength.subblock_limit()),
            (64, 60)
        );
    }

    /// The simple filter on a step of 20 between flat sides, worked from
    /// section 15.2: the test is `20 * 2 + 20 / 2 = 50 <= limit`, and
    /// `a = 3 * 20 + (100 - 120) = 40`, so `p0 += (40 + 3) >> 3 = 5` and
    /// `q0 -= (40 + 4) >> 3 = 5`.
    #[test]
    fn the_simple_filter_moves_the_edge_pixels_by_section_15_2() {
        let step = [100, 100, 100, 100, 120, 120, 120, 120];
        assert_eq!(
            run(step, |p, at, s| simple_segment(p, at, s, 50)),
            [100, 100, 100, 105, 115, 120, 120, 120]
        );
        assert_eq!(run(step, |p, at, s| simple_segment(p, at, s, 49)), step);
    }

    /// The normal filters on the same step, at level 20 (limits 64 and 60,
    /// interior 20, threshold 1): flat sides have no variance, so the
    /// macroblock filter takes `w = (100 - 120) + 3 * 20 = 40` and moves three
    /// pixels each side by `(27 * 40 + 63) >> 7 = 8`,
    /// `(18 * 40 + 63) >> 7 = 6` and `(9 * 40 + 63) >> 7 = 3`; the subblock
    /// filter, without outer taps, has `a = 60`, so it moves p0 by
    /// `(60 + 3) >> 3 = 7`, q0 by `(60 + 4) >> 3 = 8`, and p1 and q1 by
    /// `(8 + 1) >> 1 = 4`.
    #[test]
    fn the_normal_filters_move_the_edge_pixels_by_section_15_3() {
        let step = [100, 100, 100, 100, 120, 120, 120, 120];
        let strength = Strength::new(20, 0);
        assert_eq!(
            run(step, |p, at, s| macroblock_segment(p, at, s, strength)),
            [100, 103, 106, 108, 112, 114, 117, 120]
        );
        assert_eq!(
            run(step, |p, at, s| subblock_segment(p, at, s, strength)),
            [100, 100, 104, 107, 112, 116, 120, 120]
        );
        // A step of 3 inside one side past the interior limit of 2 (level 2)
        // leaves the segment alone.
        let rough = [100, 103, 100, 100, 104, 104, 104, 104];
        assert_eq!(
            run(rough, |p, at, s| subblock_segment(
                p,
                at,
                s,
                Strength::new(2, 0)
            )),
            rough
        );
        // High variance (p1 to p0 is 4, past the threshold of 1) takes the
        // simple adjustment with outer taps: a = 3 * 10 + (96 - 110) = 16,
        // so p0 += (16 + 3) >> 3 = 2 and q0 -= (16 + 4) >> 3 = 2.
        let varied = [100, 100, 96, 100, 110, 110, 110, 110];
        assert_eq!(
            run(varied, |p, at, s| macroblock_segment(p, at, s, strength)),
            [100, 100, 96, 102, 108, 110, 110, 110]
        );
    }
}
