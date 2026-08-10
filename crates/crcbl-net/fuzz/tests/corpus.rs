//! Named seeds from the fuzzer's corpus, replayed through the delta decoder.
//!
//! `fuzz_targets/decoder.rs` runs the decoders against bytes libFuzzer invents.
//! This target runs one of them against bytes somebody named, and the two are
//! not the same job. A fuzzer finds a crashing or wrongly-accepted input once
//! and then moves on; nothing makes it generate that input again, so a fix that
//! regresses is a fix nobody notices. Pinning the seed here — with the exact
//! [`DeltaDecodeError`] it must produce, not merely "it did not panic" — is what
//! keeps that specific input failing forever if the decoder stops rejecting it.
//!
//! It is a `#[test]` rather than a second fuzz target because it needs no
//! fuzzing runtime at all: the seeds are `include_bytes!`d out of
//! `../corpus/decoder/`, so an ordinary `cargo test` replays them in
//! milliseconds with no nightly toolchain and no `cargo fuzz`. That matters
//! because `crates/crcbl-net/fuzz/Cargo.toml` declares its own `[workspace]`:
//! the repository's `cargo nextest run --workspace` sweep does not reach this
//! directory, so these two tests have to be cheap enough to run on their own.
//!
//! Every seed is decoded at [`Trust::Untrusted`], which is the level they were
//! written to exercise.

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
