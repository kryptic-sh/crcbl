//! The `ALPH` chunk, RFC 9649 section 2.7.1.2: a lossy image's alpha plane.
//!
//! The plane is stored raw or as a headerless lossless image whose green
//! channel is the alpha, and either way may be filtered: each value stored
//! as its difference from a prediction by its left, upper or both
//! neighbours, undone here. The header's pre-processing field is
//! informative ("decoders are not required to use this information"): level
//! reduction quantised the values before they were coded, and the values
//! decoded are the image's alpha as they stand. libwebp's default decode
//! does the same, smoothing them only when asked to dither alpha.

use super::{lossless, malformed, unsupported};
use crate::ImageError;

/// The alpha header's compression methods.
const RAW: u8 = 0;
const LOSSLESS: u8 = 1;

/// The alpha header's filtering methods.
const NO_FILTER: u8 = 0;
const HORIZONTAL: u8 = 1;
const VERTICAL: u8 = 2;
const GRADIENT: u8 = 3;

/// Decodes an `ALPH` chunk's data to `width` × `height` alpha values, row by
/// row. The caller has checked the size against its limits.
pub(super) fn decode(chunk: &[u8], width: usize, height: usize) -> Result<Vec<u8>, ImageError> {
    let (&header, data) = chunk
        .split_first()
        .ok_or_else(|| malformed("an empty alpha chunk"))?;
    let compression = header & 3;
    let filter = (header >> 2) & 3;
    let count = width * height;
    let mut alpha = match compression {
        RAW => data
            .get(..count)
            .ok_or_else(|| malformed("raw alpha shorter than its image"))?
            .to_vec(),
        LOSSLESS => lossless::decode_headerless(data, width, height)?
            .into_iter()
            .map(|pixel| (pixel >> 8) as u8)
            .collect(),
        _ => {
            return Err(unsupported(format!(
                "alpha compression method {compression}"
            )));
        }
    };
    unfilter(&mut alpha, width, filter);
    Ok(alpha)
}

/// Undoes the filter the header names, in place: each value has the
/// prediction added back, modulo 256, in scan order, so every prediction
/// reads values already restored.
fn unfilter(alpha: &mut [u8], width: usize, filter: u8) {
    if filter == NO_FILTER {
        return;
    }
    for at in 0..alpha.len() {
        let (x, y) = (at % width, at / width);
        let left = || alpha[at - 1];
        let above = || alpha[at - width];
        let prediction = if x == 0 && y == 0 {
            0
        } else if y == 0 {
            // The top row predicts from the left whatever the method.
            left()
        } else if x == 0 {
            // The left column predicts from above whatever the method.
            above()
        } else {
            match filter {
                HORIZONTAL => left(),
                VERTICAL => above(),
                _ => {
                    debug_assert_eq!(filter, GRADIENT);
                    let estimate =
                        i32::from(left()) + i32::from(above()) - i32::from(alpha[at - width - 1]);
                    estimate.clamp(0, 255) as u8
                }
            }
        };
        alpha[at] = alpha[at].wrapping_add(prediction);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each filter undone on a 3x3 plane of stored differences, worked from
    /// the RFC's rules: the first value is predicted by 0, the rest of the
    /// top row by the left, the rest of the left column by the value above,
    /// and the interior by the method; the gradient clips `A + B - C` to a
    /// byte, and the sum wraps.
    #[test]
    fn each_filter_is_undone_as_the_rfc_predicts() {
        let stored = [10, 5, 5, 20, 1, 2, 250, 3, 4];
        let cases = [
            (NO_FILTER, stored),
            // Rows: 10 15 20; 30 31 33; 24 27 31 (250 + 30 wraps to 24).
            (HORIZONTAL, [10, 15, 20, 30, 31, 33, 24, 27, 31]),
            // Rows: 10 15 20; 30 16 22; 24 19 26.
            (VERTICAL, [10, 15, 20, 30, 16, 22, 24, 19, 26]),
            // (1,1): 30 + 15 - 10 = 35, + 1 = 36; (2,1): 36 + 20 - 15 = 41,
            // + 2 = 43; (1,2): 24 + 36 - 30 = 30, + 3 = 33; (2,2):
            // 33 + 43 - 36 = 40, + 4 = 44.
            (GRADIENT, [10, 15, 20, 30, 36, 43, 24, 33, 44]),
        ];
        for (filter, expected) in cases {
            let mut alpha = stored;
            unfilter(&mut alpha, 3, filter);
            assert_eq!(alpha, expected, "filter {filter}");
        }
        // The gradient's estimate clips: 250 + 250 - 0 is 255, not 244.
        let mut alpha = [0, 250, 250, 0];
        unfilter(&mut alpha, 2, GRADIENT);
        assert_eq!(alpha, [0, 250, 250, 255]);
    }

    #[test]
    fn raw_alpha_is_read_as_it_stands_and_must_cover_its_image() {
        assert_eq!(decode(&[RAW, 1, 2, 3, 4, 9], 2, 2), Ok(vec![1, 2, 3, 4]));
        assert!(matches!(
            decode(&[RAW, 1, 2, 3], 2, 2),
            Err(ImageError::Malformed(_))
        ));
        assert!(matches!(decode(&[], 2, 2), Err(ImageError::Malformed(_))));
        assert!(matches!(
            decode(&[2, 0, 0, 0, 0], 2, 2),
            Err(ImageError::Unsupported(_))
        ));
        // The pre-processing and reserved bits change nothing.
        assert_eq!(
            decode(&[0xf0 | RAW, 1, 2, 3, 4], 2, 2),
            Ok(vec![1, 2, 3, 4])
        );
    }
}
