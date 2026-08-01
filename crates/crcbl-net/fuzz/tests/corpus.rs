use crcbl_net::{DeltaDecodeError, Trust, decode_delta};

/// The corpus is decoded at the hostile-input trust level, which is what the
/// seeds were written to exercise.
fn decode_delta_untrusted(payload: &[u8]) -> Result<crcbl_net::Delta, DeltaDecodeError> {
    decode_delta(payload, Trust::Untrusted)
}

#[test]
fn named_delta_seeds_reach_their_intended_paths() {
    let minimal = include_bytes!("../corpus/decoder/delta-keyframe-minimal");
    let decoded =
        decode_delta(minimal, Trust::Untrusted).expect("minimal keyframe seed must decode");
    assert!(decoded.is_keyframe);
    assert!(decoded.systems.is_empty());

    assert!(matches!(
        decode_delta_untrusted(include_bytes!("../corpus/decoder/delta-truncated")),
        Err(DeltaDecodeError::TooShort)
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!("../corpus/decoder/delta-trailing-byte")),
        Err(DeltaDecodeError::TrailingBytes(1))
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!(
            "../corpus/decoder/delta-hostile-system-count"
        )),
        Err(DeltaDecodeError::InvalidLength(u32::MAX))
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!(
            "../corpus/decoder/delta-hostile-entity-count"
        )),
        Err(DeltaDecodeError::InvalidLength(4_294_967_284))
    ));
}

#[test]
fn oversized_seed_crosses_the_decoder_limit() {
    let oversized = include_bytes!("../corpus/decoder/oversized-payload");
    assert_eq!(oversized.len(), 65_537);
    assert!(matches!(
        decode_delta(oversized, Trust::Untrusted),
        Err(DeltaDecodeError::InvalidLength(65_537))
    ));
}
