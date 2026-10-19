//! The sRGB transfer function, in both directions, on one channel in `0..=1`.
//!
//! IEC 61966-2-1's piecewise curve, which is what Vulkan's `*_SRGB` formats,
//! Metal's `*_sRGB` ones and D3D12's `*_SRGB` ones apply on a write and undo on
//! a read. The engine draws in linear light and lets an sRGB attachment encode,
//! so a colour authored the usual way — `#rrggbb`, a picker's bytes — is
//! [`decode`]d once on its way in.
//!
//! It lives here because every crate that turns authored colour into linear
//! light already depends on this one: `crcbl-ui`'s stylesheet colours and
//! `crcbl-frame`'s mip generation call it, rather than each carrying a copy.

/// The slope of the straight segment both arms start from.
const LINEAR_SLOPE: f32 = 12.92;

/// The encoded value the two arms change over at, as the specification writes
/// it.
const KNEE_ENCODED: f32 = 0.040_45;

/// The same knee in linear light, as the specification writes it.
const KNEE_LINEAR: f32 = 0.003_130_8;

/// The curved arm's offset.
const OFFSET: f32 = 0.055;

/// The curved arm's exponent.
const GAMMA: f32 = 2.4;

/// One sRGB-encoded channel as linear light.
#[must_use]
pub fn decode(encoded: f32) -> f32 {
    if encoded <= KNEE_ENCODED {
        encoded / LINEAR_SLOPE
    } else {
        ((encoded + OFFSET) / (1.0 + OFFSET)).powf(GAMMA)
    }
}

/// One linear channel as sRGB-encoded; the inverse of [`decode`].
#[must_use]
pub fn encode(linear: f32) -> f32 {
    if linear <= KNEE_LINEAR {
        linear * LINEAR_SLOPE
    } else {
        (1.0 + OFFSET) * linear.powf(1.0 / GAMMA) - OFFSET
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic is `f32`; the references are Python's `f64` from the
    /// specification's formulas (2026-10-19).
    const TOLERANCE: f32 = 1e-6;

    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= TOLERANCE,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn decode_matches_the_specification_on_both_arms() {
        close(decode(0.0), 0.0);
        close(decode(1.0), 1.0);
        close(decode(0.5), 0.214_041_14);
        close(decode(128.0 / 255.0), 0.215_860_5);
        close(decode(0.2), 0.033_104_766);
        close(decode(0.02), 0.02 / 12.92);
    }

    #[test]
    fn encode_matches_the_specification_on_both_arms() {
        close(encode(0.0), 0.0);
        close(encode(1.0), 1.0);
        close(encode(0.214_041_14), 0.5);
        close(encode(0.5), 0.735_357);
        close(encode(0.001), 0.012_92);
    }

    #[test]
    fn every_byte_survives_a_round_trip() {
        for byte in 0..=u8::MAX {
            let encoded = f32::from(byte) / 255.0;
            let back = (encode(decode(encoded)) * 255.0).round();
            assert_eq!(back, f32::from(byte), "byte {byte}");
        }
    }
}
