//! Intra prediction, RFC 6386 section 12, into a macroblock's workspace.
//!
//! A workspace is a block with one row above it and one column to its left,
//! holding the already reconstructed (not yet loop-filtered) pixels a
//! prediction reads, or the RFC's stand-ins where the frame has none: 127
//! above the top row (and above-left of it), 129 left of the left column.
//! The luma workspace has four columns more on the right for the pixels
//! above and to the right of the rightmost subblocks.

use super::{BMode, Mode};

/// The values the RFC assumes above the frame's top row and left of its left
/// column, RFC 6386 section 12.
pub(super) const ABOVE_EDGE: u8 = 127;
pub(super) const LEFT_EDGE: u8 = 129;

/// RFC 6386 section 12.3's `avg3`: a weighted average centred on `y`.
const fn avg3(x: u8, y: u8, z: u8) -> u8 {
    ((x as u16 + 2 * y as u16 + z as u16 + 2) >> 2) as u8
}

/// RFC 6386 section 12.3's `avg2`.
const fn avg2(x: u8, y: u8) -> u8 {
    ((x as u16 + y as u16 + 1) >> 1) as u8
}

fn clamp255(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// Predicts the `size` × `size` block at `at` in `buffer` (rows `stride`
/// apart) by one of the whole-block modes, RFC 6386 section 12.2: DC, V, H
/// or TM. `above` and `left` say whether the frame has those neighbours,
/// which only DC prediction asks; the others read the edge values.
pub(super) fn whole_block(
    buffer: &mut [u8],
    stride: usize,
    at: usize,
    size: usize,
    mode: Mode,
    above: bool,
    left: bool,
) {
    match mode {
        Mode::Dc => {
            let above_sum: u32 = (0..size).map(|x| u32::from(buffer[at - stride + x])).sum();
            let left_sum: u32 = (0..size)
                .map(|y| u32::from(buffer[at + y * stride - 1]))
                .sum();
            let log2 = size.trailing_zeros();
            // The average of the 16 or 32 genuinely visible pixels, of 8 or
            // 16 with one edge missing, and 128 with both missing.
            let value = match (above, left) {
                (true, true) => (above_sum + left_sum + (1 << log2)) >> (log2 + 1),
                (true, false) => (above_sum + (1 << (log2 - 1))) >> log2,
                (false, true) => (left_sum + (1 << (log2 - 1))) >> log2,
                (false, false) => 128,
            };
            for y in 0..size {
                buffer[at + y * stride..][..size].fill(value as u8);
            }
        }
        Mode::V => {
            for y in 0..size {
                buffer.copy_within(at - stride..at - stride + size, at + y * stride);
            }
        }
        Mode::H => {
            for y in 0..size {
                let row = at + y * stride;
                let left = buffer[row - 1];
                buffer[row..row + size].fill(left);
            }
        }
        Mode::Tm => {
            let corner = i32::from(buffer[at - stride - 1]);
            for y in 0..size {
                let row = at + y * stride;
                let left = i32::from(buffer[row - 1]);
                for x in 0..size {
                    let above = i32::from(buffer[at - stride + x]);
                    buffer[row + x] = clamp255(left + above - corner);
                }
            }
        }
        // B_PRED is predicted subblock by subblock.
        Mode::B => {}
    }
}

/// Predicts the 4x4 subblock at `at` by `mode`: RFC 6386 section 12.3's
/// `subblock_intra_predict`, transcribed. `A` is the eight pixels above
/// (four above, four above-right), `L` the four to the left, `P` the corner,
/// and `E` the RFC's edge array running up the left column, through the
/// corner and along the top.
#[allow(clippy::many_single_char_names)]
pub(super) fn subblock(buffer: &mut [u8], stride: usize, at: usize, mode: BMode) {
    let a: [u8; 8] = std::array::from_fn(|i| buffer[at - stride + i]);
    let l: [u8; 4] = std::array::from_fn(|i| buffer[at + i * stride - 1]);
    let p = buffer[at - stride - 1];
    let e = [l[3], l[2], l[1], l[0], p, a[0], a[1], a[2], a[3]];
    // `avg3p(E + k)` and `avg2p(E + k)` of the RFC.
    let e3 = |k: usize| avg3(e[k - 1], e[k], e[k + 1]);
    let e2 = |k: usize| avg2(e[k], e[k + 1]);
    let mut b = [[0u8; 4]; 4];
    match mode {
        BMode::Dc => {
            let sum: u32 = (0..4).map(|i| u32::from(a[i]) + u32::from(l[i])).sum();
            b = [[((sum + 4) >> 3) as u8; 4]; 4];
        }
        BMode::Tm => {
            for (r, row) in b.iter_mut().enumerate() {
                for (c, pixel) in row.iter_mut().enumerate() {
                    *pixel = clamp255(i32::from(l[r]) + i32::from(a[c]) - i32::from(p));
                }
            }
        }
        BMode::Ve => {
            // All four rows are the smoothed top row; its first pixel is
            // smoothed with the corner.
            let above = [p, a[0], a[1], a[2], a[3], a[4]];
            let row: [u8; 4] = std::array::from_fn(|c| avg3(above[c], above[c + 1], above[c + 2]));
            b = [row; 4];
        }
        BMode::He => {
            // Each row is its smoothed left pixel; the bottom one has no
            // pixel below it and repeats its own.
            let left = [p, l[0], l[1], l[2], l[3], l[3]];
            for (r, row) in b.iter_mut().enumerate() {
                *row = [avg3(left[r], left[r + 1], left[r + 2]); 4];
            }
        }
        BMode::Ld => {
            let smoothed = |k: usize| avg3(a[k - 1], a[k], a[k + 1]);
            b[0][0] = smoothed(1);
            [b[0][1], b[1][0]] = [smoothed(2); 2];
            [b[0][2], b[1][1], b[2][0]] = [smoothed(3); 3];
            [b[0][3], b[1][2], b[2][1], b[3][0]] = [smoothed(4); 4];
            [b[1][3], b[2][2], b[3][1]] = [smoothed(5); 3];
            [b[2][3], b[3][2]] = [smoothed(6); 2];
            b[3][3] = avg3(a[6], a[7], a[7]);
        }
        BMode::Rd => {
            b[3][0] = e3(1);
            [b[3][1], b[2][0]] = [e3(2); 2];
            [b[3][2], b[2][1], b[1][0]] = [e3(3); 3];
            [b[3][3], b[2][2], b[1][1], b[0][0]] = [e3(4); 4];
            [b[2][3], b[1][2], b[0][1]] = [e3(5); 3];
            [b[1][3], b[0][2]] = [e3(6); 2];
            b[0][3] = e3(7);
        }
        BMode::Vr => {
            b[3][0] = e3(2);
            b[2][0] = e3(3);
            [b[3][1], b[1][0]] = [e3(4); 2];
            [b[2][1], b[0][0]] = [e2(4); 2];
            [b[3][2], b[1][1]] = [e3(5); 2];
            [b[2][2], b[0][1]] = [e2(5); 2];
            [b[3][3], b[1][2]] = [e3(6); 2];
            [b[2][3], b[0][2]] = [e2(6); 2];
            b[1][3] = e3(7);
            b[0][3] = e2(7);
        }
        BMode::Vl => {
            let smoothed = |k: usize| avg3(a[k - 1], a[k], a[k + 1]);
            let half = |k: usize| avg2(a[k], a[k + 1]);
            b[0][0] = half(0);
            b[1][0] = smoothed(1);
            [b[2][0], b[0][1]] = [half(1); 2];
            [b[1][1], b[3][0]] = [smoothed(2); 2];
            [b[2][1], b[0][2]] = [half(2); 2];
            [b[3][1], b[1][2]] = [smoothed(3); 2];
            [b[2][2], b[0][3]] = [half(3); 2];
            [b[3][2], b[1][3]] = [smoothed(4); 2];
            // The last two do not follow the pattern, as the RFC notes.
            b[2][3] = smoothed(5);
            b[3][3] = smoothed(6);
        }
        BMode::Hd => {
            b[3][0] = e2(0);
            b[3][1] = e3(1);
            [b[2][0], b[3][2]] = [e2(1); 2];
            [b[2][1], b[3][3]] = [e3(2); 2];
            [b[2][2], b[1][0]] = [e2(2); 2];
            [b[2][3], b[1][1]] = [e3(3); 2];
            [b[1][2], b[0][0]] = [e2(3); 2];
            [b[1][3], b[0][1]] = [e3(4); 2];
            b[0][2] = e3(5);
            b[0][3] = e3(6);
        }
        BMode::Hu => {
            b[0][0] = avg2(l[0], l[1]);
            b[0][1] = avg3(l[0], l[1], l[2]);
            [b[0][2], b[1][0]] = [avg2(l[1], l[2]); 2];
            [b[0][3], b[1][1]] = [avg3(l[1], l[2], l[3]); 2];
            [b[1][2], b[2][0]] = [avg2(l[2], l[3]); 2];
            [b[1][3], b[2][1]] = [avg3(l[2], l[3], l[3]); 2];
            // No nearby pixel lies on the rest of the diagonals.
            [b[2][2], b[2][3], b[3][0], b[3][1], b[3][2], b[3][3]] = [l[3]; 6];
        }
    }
    for (r, row) in b.iter().enumerate() {
        buffer[at + r * stride..][..4].copy_from_slice(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 subblock workspace with its edges: stride 9, the subblock at row
    /// 1, column 1, the eight pixels above from the corner's right.
    const STRIDE: usize = 9;
    const AT: usize = STRIDE + 1;

    fn workspace(corner: u8, above: [u8; 8], left: [u8; 4]) -> [u8; STRIDE * 5] {
        let mut buffer = [0; STRIDE * 5];
        buffer[0] = corner;
        buffer[1..9].copy_from_slice(&above);
        for (row, &pixel) in left.iter().enumerate() {
            buffer[(row + 1) * STRIDE] = pixel;
        }
        buffer
    }

    fn block(buffer: &[u8]) -> [[u8; 4]; 4] {
        std::array::from_fn(|r| std::array::from_fn(|c| buffer[AT + r * STRIDE + c]))
    }

    /// Each subblock mode on one set of edges, every output worked by hand
    /// from section 12.3's code: corner 50, above 10..80 in steps of 10, left
    /// 100, 90, 60, 20. The edge array E is then 20, 60, 90, 100, 50, 10, 20,
    /// 30, 40.
    #[test]
    fn every_subblock_mode_predicts_as_section_12_3_computes() {
        let above = [10, 20, 30, 40, 50, 60, 70, 80];
        let left = [100, 90, 60, 20];
        let cases: [(BMode, [[u8; 4]; 4]); 10] = [
            // (10 + 20 + 30 + 40 + 100 + 90 + 60 + 20 + 4) >> 3 = 46.
            (BMode::Dc, [[46; 4]; 4]),
            // L[r] + A[c] - 50, clamped.
            (
                BMode::Tm,
                [
                    [60, 70, 80, 90],
                    [50, 60, 70, 80],
                    [20, 30, 40, 50],
                    [0, 0, 0, 10],
                ],
            ),
            // avg3(50, 10, 20) = 23, avg3(10, 20, 30) = 20, 30, 40.
            (BMode::Ve, [[23, 20, 30, 40]; 4]),
            // avg3(50, 100, 90) = 85, avg3(100, 90, 60) = 85,
            // avg3(90, 60, 20) = 58, avg3(60, 20, 20) = 30.
            (BMode::He, [[85; 4], [85; 4], [58; 4], [30; 4]]),
            // Down-left along the smoothed top row; the last repeats A[7].
            (
                BMode::Ld,
                [
                    [20, 30, 40, 50],
                    [30, 40, 50, 60],
                    [40, 50, 60, 70],
                    [50, 60, 70, 78],
                ],
            ),
            // avg3p(E + k): k=1: (20+120+90+2)>>2 = 58, k=2: 85, k=3: 85,
            // k=4: (100+100+10+2)>>2 = 53, k=5: (50+20+20+2)>>2 = 23,
            // k=6: 20, k=7: 30.
            (
                BMode::Rd,
                [
                    [53, 23, 20, 30],
                    [85, 53, 23, 20],
                    [85, 85, 53, 23],
                    [58, 85, 85, 53],
                ],
            ),
            // avg2p(E + 4) = 30, avg2p(E + 5) = 15, avg2p(E + 6) = 25,
            // avg2p(E + 7) = 35.
            (
                BMode::Vr,
                [
                    [30, 15, 25, 35],
                    [53, 23, 20, 30],
                    [85, 30, 15, 25],
                    [85, 53, 23, 20],
                ],
            ),
            (
                BMode::Vl,
                [
                    [15, 25, 35, 45],
                    [20, 30, 40, 50],
                    [25, 35, 45, 60],
                    [30, 40, 50, 70],
                ],
            ),
            // avg2p(E) = 40, avg2p(E + 1) = 75, avg2p(E + 2) = 95,
            // avg2p(E + 3) = 75.
            (
                BMode::Hd,
                [
                    [75, 53, 23, 20],
                    [95, 85, 75, 53],
                    [75, 85, 95, 85],
                    [40, 58, 75, 85],
                ],
            ),
            // avg2(100, 90) = 95, avg3(100, 90, 60) = 85, avg2(90, 60) = 75,
            // avg3(90, 60, 20) = 58, avg2(60, 20) = 40, avg3(60, 20, 20) = 30.
            (
                BMode::Hu,
                [
                    [95, 85, 75, 58],
                    [75, 58, 40, 30],
                    [40, 30, 20, 20],
                    [20; 4],
                ],
            ),
        ];
        for (mode, expected) in cases {
            let mut buffer = workspace(50, above, left);
            subblock(&mut buffer, STRIDE, AT, mode);
            assert_eq!(block(&buffer), expected, "{mode:?}");
        }
    }

    /// The whole-block modes on an 8x8 block, and DC's three edge cases.
    #[test]
    fn whole_block_modes_predict_as_section_12_2_computes() {
        const STRIDE: usize = 9;
        let mut buffer = [0u8; STRIDE * 9];
        buffer[0] = 100;
        for x in 0..8 {
            buffer[1 + x] = 10 * x as u8 + 10;
        }
        for y in 0..8 {
            buffer[(y + 1) * STRIDE] = 200 - 20 * y as u8;
        }
        let at = STRIDE + 1;
        let row = |buffer: &[u8], y: usize| buffer[at + y * STRIDE..][..8].to_vec();

        let mut v = buffer;
        whole_block(&mut v, STRIDE, at, 8, Mode::V, true, true);
        assert_eq!(row(&v, 7), [10, 20, 30, 40, 50, 60, 70, 80]);

        let mut h = buffer;
        whole_block(&mut h, STRIDE, at, 8, Mode::H, true, true);
        assert_eq!(row(&h, 3), [140; 8]);

        // L + A - P: row 7 has L = 60, so 60 + A - 100, clamped at zero.
        let mut tm = buffer;
        whole_block(&mut tm, STRIDE, at, 8, Mode::Tm, true, true);
        assert_eq!(row(&tm, 7), [0, 0, 0, 0, 10, 20, 30, 40]);
        assert_eq!(row(&tm, 0), [110, 120, 130, 140, 150, 160, 170, 180]);

        // Above sums to 360 and left to 1040: (1400 + 8) >> 4 = 88 with
        // both, (360 + 4) >> 3 = 45 above only, (1040 + 4) >> 3 = 130 left
        // only, and 128 with neither.
        for (above, left, value) in [
            (true, true, 88),
            (true, false, 45),
            (false, true, 130),
            (false, false, 128),
        ] {
            let mut dc = buffer;
            whole_block(&mut dc, STRIDE, at, 8, Mode::Dc, above, left);
            assert_eq!(row(&dc, 5), [value; 8], "above {above}, left {left}");
        }
    }
}
