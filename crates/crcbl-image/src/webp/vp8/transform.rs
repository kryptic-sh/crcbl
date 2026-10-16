//! The inverse transforms, RFC 6386 sections 14.3 and 14.4.
//!
//! Both are the RFC's C, which the RFC makes normative ("a conforming
//! decoder should implement the inverse transform using exactly the same
//! rounding"), including its 16-bit store between the passes. libwebp keeps
//! that intermediate in 32 bits; the two agree whenever it fits in 16, which
//! it does for any coefficients an encoder produces.

/// `sqrt(2) * cos(pi / 8) - 1` in 16-bit fixed point, RFC 6386 section 14.4:
/// the constant above one is applied as `x + x * (c - 1)` to keep 16 bits of
/// precision.
const COS_PI8_SQRT2_MINUS_1: i32 = 20091;

/// `sqrt(2) * sin(pi / 8)` in 16-bit fixed point, RFC 6386 section 14.4.
const SIN_PI8_SQRT2: i32 = 35468;

/// The odd half of the RFC's one-dimensional inverse DCT: the pair it adds
/// to and subtracts from the even half, from the inputs at positions 1 and 3.
fn odd(one: i32, three: i32) -> (i32, i32) {
    let c = ((one * SIN_PI8_SQRT2) >> 16) - (three + ((three * COS_PI8_SQRT2_MINUS_1) >> 16));
    let d = (one + ((one * COS_PI8_SQRT2_MINUS_1) >> 16)) + ((three * SIN_PI8_SQRT2) >> 16);
    (c, d)
}

/// RFC 6386 section 14.4's `short_idct4x4llm_c`: a 4x4 block's residue from
/// its dequantised coefficients, both row by row. The columns are
/// transformed first, then the rows with rounding.
pub(super) fn idct(input: &[i16; 16]) -> [i16; 16] {
    let mut columns = [0i16; 16];
    for i in 0..4 {
        let at = |row: usize| i32::from(input[row * 4 + i]);
        let (a, b) = (at(0) + at(2), at(0) - at(2));
        let (c, d) = odd(at(1), at(3));
        columns[i] = (a + d) as i16;
        columns[4 + i] = (b + c) as i16;
        columns[8 + i] = (b - c) as i16;
        columns[12 + i] = (a - d) as i16;
    }
    let mut out = [0i16; 16];
    for (row, out) in columns
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0)
    {
        let at = |column: usize| i32::from(row[column]);
        let (a, b) = (at(0) + at(2), at(0) - at(2));
        let (c, d) = odd(at(1), at(3));
        out[0] = ((a + d + 4) >> 3) as i16;
        out[1] = ((b + c + 4) >> 3) as i16;
        out[2] = ((b - c + 4) >> 3) as i16;
        out[3] = ((a - d + 4) >> 3) as i16;
    }
    out
}

/// RFC 6386 section 14.3's `vp8_short_inv_walsh4x4_c`: the second-order
/// block's inverse Walsh-Hadamard transform, whose sixteen outputs are the
/// DC coefficients of the luma subblocks in raster order.
pub(super) fn iwht(input: &[i16; 16]) -> [i16; 16] {
    let mut columns = [0i16; 16];
    for i in 0..4 {
        let at = |row: usize| i32::from(input[row * 4 + i]);
        let (a, b) = (at(0) + at(3), at(1) + at(2));
        let (c, d) = (at(1) - at(2), at(0) - at(3));
        columns[i] = (a + b) as i16;
        columns[4 + i] = (c + d) as i16;
        columns[8 + i] = (a - b) as i16;
        columns[12 + i] = (d - c) as i16;
    }
    let mut out = [0i16; 16];
    for (row, out) in columns
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0)
    {
        let at = |column: usize| i32::from(row[column]);
        let (a, b) = (at(0) + at(3), at(1) + at(2));
        let (c, d) = (at(1) - at(2), at(0) - at(3));
        out[0] = ((a + b + 3) >> 3) as i16;
        out[1] = ((c + d + 3) >> 3) as i16;
        out[2] = ((a - b + 3) >> 3) as i16;
        out[3] = ((d - c + 3) >> 3) as i16;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_coefficient(position: usize, value: i16) -> [i16; 16] {
        let mut block = [0; 16];
        block[position] = value;
        block
    }

    /// A DC coefficient alone is the same residue everywhere: the column pass
    /// passes it through, and the row pass rounds it, `(dc + 4) >> 3`.
    #[test]
    fn the_idct_of_a_dc_alone_is_flat() {
        assert_eq!(idct(&one_coefficient(0, 100)), [13; 16]);
        assert_eq!(idct(&one_coefficient(0, -100)), [-12; 16]);
    }

    /// The first horizontal AC coefficient, worked from section 14.4 by hand:
    /// with x = 100, `c = (100 * 35468) >> 16 = 54` and
    /// `d = 100 + ((100 * 20091) >> 16) = 130`, so each row is
    /// `(d + 4) >> 3, (c + 4) >> 3, (4 - c) >> 3, (4 - d) >> 3`, the last two
    /// shifts rounding toward minus infinity.
    #[test]
    fn the_idct_of_the_first_ac_coefficient_matches_the_rfcs_arithmetic() {
        let row = [16, 7, -7, -16];
        let expected: Vec<i16> = row.iter().cycle().take(16).copied().collect();
        assert_eq!(idct(&one_coefficient(1, 100)).to_vec(), expected);
        // The same coefficient one row down transforms the columns instead.
        let transposed: Vec<i16> = (0..16).map(|i| row[i / 4]).collect();
        assert_eq!(idct(&one_coefficient(4, 100)).to_vec(), transposed);
    }

    /// The last AC coefficient of the first row, at values where each
    /// constant's last digit moves a result: 610 has
    /// `c = -(610 + ((610 * 20091) >> 16)) = -797` (20090 would give -796) and
    /// `d = (610 * 35468) >> 16 = 330`, so each row is `(d + 4) >> 3 = 41`,
    /// `(c + 4) >> 3 = -100`, `(4 - c) >> 3 = 100`, `(4 - d) >> 3 = -41`; 704
    /// has `c = -919` and `d = (704 * 35468) >> 16 = 381` (35467 would give
    /// 380), so 48, -115, 115, -48.
    #[test]
    fn the_idct_constants_are_the_rfcs_to_the_last_digit() {
        for (value, row) in [(610, [41, -100, 100, -41]), (704, [48, -115, 115, -48])] {
            let expected: Vec<i16> = row.iter().cycle().take(16).copied().collect();
            assert_eq!(
                idct(&one_coefficient(3, value)).to_vec(),
                expected,
                "{value}"
            );
        }
    }

    /// The WHT's basis, from section 14.3 by hand: a lone coefficient of 8 at
    /// row 0, column k gives every row the k-th Walsh function (rounded
    /// `(±8 + 3) >> 3`), and a DC of 100 gives `(100 + 3) >> 3` everywhere.
    #[test]
    fn the_wht_of_one_coefficient_is_its_walsh_function() {
        assert_eq!(iwht(&one_coefficient(0, 100)), [12; 16]);
        for (position, row) in [
            (1, [1, 1, -1, -1]),
            (2, [1, -1, -1, 1]),
            (3, [1, -1, 1, -1]),
        ] {
            let expected: Vec<i16> = row.iter().cycle().take(16).copied().collect();
            assert_eq!(
                iwht(&one_coefficient(position, 8)).to_vec(),
                expected,
                "{position}"
            );
        }
        let column: Vec<i16> = (0..16).map(|i| [1, 1, -1, -1][i / 4]).collect();
        assert_eq!(iwht(&one_coefficient(4, 8)).to_vec(), column);
    }
}
