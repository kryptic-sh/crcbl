//! The committed stream the JavaScript decoder is held to.
//!
//! `web/engine/gpu-stream.js` is the production decoder, hand-written in another
//! language, so nothing in either build ever sees both halves of this format at
//! once. What holds them together is this file and its counterpart
//! `web/tools/stream-decode.mjs`: this one freezes the canonical stream into
//! `tests/fixtures/`, that one decodes the same bytes and asserts every field.
//!
//! So a tag, a field order or a cap changed on the Rust side turns *this* test
//! red, and the same change made only in JavaScript turns the node test red.
//! Neither can be satisfied by editing one half alone, which is the only reason
//! two hand-written decoders are a tolerable arrangement.

mod corpus;

use std::path::{Path, PathBuf};

/// The environment variable that turns the comparison into a regeneration.
///
/// The spelling `crcbl-golden` already uses, and for its reasons: a test binary
/// run under `cargo nextest` has no argument of its own to read, and nobody
/// should have to remember a second lever for the same act.
const BLESS_ENV: &str = "CRCBL_BLESS";

/// Where the committed stream lives, relative to the crate root.
const FIXTURE: &str = "tests/fixtures/canonical-stream.bin";

/// The regeneration command, quoted in every failure this file can produce.
const REGENERATE: &str = "CRCBL_BLESS=1 cargo test -p crcbl-webgpu --test fixture";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE)
}

/// Whether this process was asked to regenerate the fixture.
///
/// Anything other than unset, empty or `0` counts, so `CRCBL_BLESS=1` and
/// `CRCBL_BLESS=true` both work and `CRCBL_BLESS=0` does not — the rule
/// `crcbl_golden::blessing` documents.
fn blessing() -> bool {
    match std::env::var(BLESS_ENV) {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

/// The offset of the first byte the two disagree on, or the shorter length when
/// one is a prefix of the other.
fn first_difference(committed: &[u8], encoded: &[u8]) -> usize {
    committed
        .iter()
        .zip(encoded)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| committed.len().min(encoded.len()))
}

/// **Blessing is not a pass**, for the reason `crcbl_golden::Outcome::into_result`
/// gives: a run that rewrote the fixture compared nothing, and a gate that any
/// missing file switches off is not a gate.
#[test]
fn the_canonical_stream_still_encodes_to_the_committed_fixture() {
    let (stream, _) = corpus::encode_all();
    let encoded = stream.bytes();
    let path = fixture_path();

    let committed = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("{} could not be read: {error}", path.display()),
    };

    if blessing() || committed.is_none() {
        let directory = path.parent().expect("the fixture path names a directory");
        std::fs::create_dir_all(directory).unwrap_or_else(|error| {
            panic!("{} could not be created: {error}", directory.display())
        });
        std::fs::write(&path, encoded)
            .unwrap_or_else(|error| panic!("{} could not be written: {error}", path.display()));
        panic!(
            "{} was {} rather than compared, so this run proved nothing. Review the \
             bytes, commit the file, and re-run without {BLESS_ENV}.",
            path.display(),
            if committed.is_none() {
                "created"
            } else {
                "re-blessed"
            },
        );
    }

    let committed = committed.expect("the blessing branch took every `None`");
    assert!(
        committed == encoded,
        "the canonical stream no longer matches {}.\n  \
         encoded:   {} bytes\n  \
         committed: {} bytes\n  \
         first difference at byte {}\n\n\
         Both halves of this format are hand-written, so a change here is a change \
         web/engine/gpu-stream.js has to make too — update it and the expected commands \
         in web/tools/stream-decode.mjs in the same commit. Then regenerate with:\n\n    \
         {REGENERATE}\n",
        path.display(),
        encoded.len(),
        committed.len(),
        first_difference(&committed, encoded),
    );
}
