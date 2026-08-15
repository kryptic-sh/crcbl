//! The committed buffers the JavaScript halves are held to.
//!
//! Both directions of this format have two hand-written implementations, and no
//! build anywhere sees both at once:
//!
//! * **Commands** are encoded here and decoded by `web/engine/gpu-stream.js`.
//! * **Replies** are encoded by `web/engine/gpu-reply.js` and decoded here.
//!
//! What holds each pair together is a file. This test freezes the canonical
//! stream and the canonical replies into `tests/fixtures/`; `stream-decode.mjs`
//! decodes the first and asserts every field, and `reply-encode.mjs` re-encodes
//! the second and asserts every byte. So a tag, a field order or a cap changed
//! on the Rust side turns *this* test red, and the same change made only in
//! JavaScript turns the matching node tool red. Neither can be satisfied by
//! editing one half alone, which is the only reason four hand-written codecs are
//! a tolerable arrangement.

mod corpus;
mod replies;

use std::path::{Path, PathBuf};

use crcbl_webgpu::decode_replies;

/// The environment variable that turns the comparison into a regeneration.
///
/// The spelling `crcbl-golden` already uses, and for its reasons: a test binary
/// run under `cargo nextest` has no argument of its own to read, and nobody
/// should have to remember a second lever for the same act.
const BLESS_ENV: &str = "CRCBL_BLESS";

/// Where the committed command stream lives, relative to the crate root.
const STREAM_FIXTURE: &str = "tests/fixtures/canonical-stream.bin";

/// Where the committed reply stream lives, relative to the crate root.
const REPLY_FIXTURE: &str = "tests/fixtures/canonical-replies.bin";

/// The regeneration command, quoted in every failure this file can produce.
const REGENERATE: &str = "CRCBL_BLESS=1 cargo test -p crcbl-webgpu --test fixture";

fn fixture_path(fixture: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture)
}

/// Whether this process was asked to regenerate the fixtures.
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

/// The committed bytes of `fixture`, writing them first if this run was blessed.
///
/// **Blessing is not a pass**, for the reason `crcbl_golden::Outcome::into_result`
/// gives: a run that rewrote the fixture compared nothing, and a gate that any
/// missing file switches off is not a gate. So this panics rather than returning
/// when it writes.
fn committed(fixture: &str, encoded: &[u8], counterpart: &str) -> Vec<u8> {
    let path = fixture_path(fixture);
    let found = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("{} could not be read: {error}", path.display()),
    };

    if blessing() || found.is_none() {
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
            if found.is_none() {
                "created"
            } else {
                "re-blessed"
            },
        );
    }

    let found = found.expect("the blessing branch took every `None`");
    assert!(
        found == encoded,
        "{} no longer matches what this crate encodes.\n  \
         encoded:   {} bytes\n  \
         committed: {} bytes\n  \
         first difference at byte {}\n\n\
         Both halves of this format are hand-written, so a change here is a change \
         {counterpart} has to make too — update it in the same commit. Then regenerate \
         with:\n\n    {REGENERATE}\n",
        path.display(),
        encoded.len(),
        found.len(),
        first_difference(&found, encoded),
    );
    found
}

#[test]
fn the_canonical_stream_still_encodes_to_the_committed_fixture() {
    let (stream, _) = corpus::encode_all();
    committed(
        STREAM_FIXTURE,
        stream.bytes(),
        "web/engine/gpu-stream.js — and the expected commands in web/tools/stream-decode.mjs",
    );
}

#[test]
fn the_canonical_replies_still_encode_to_the_committed_fixture() {
    let (replies, _) = replies::encode_all_replies();
    committed(
        REPLY_FIXTURE,
        replies.bytes(),
        "web/engine/gpu-reply.js — and the expected replies in web/tools/reply-encode.mjs",
    );
}

/// **The committed replies still decode to what they were written from.**
///
/// A second, independent claim about the same file, and the one that matters in
/// this direction: the reply fixture is what the *JavaScript writer* is held to,
/// so the bytes JS produces are only useful if the Rust reader turns them back
/// into the right replies. The test above would stay green if the reader drifted
/// away from the writer, because it never decodes anything.
#[test]
fn the_committed_replies_decode_to_the_replies_they_were_written_from() {
    let (replies, expected) = replies::encode_all_replies();
    let bytes = committed(
        REPLY_FIXTURE,
        replies.bytes(),
        "web/engine/gpu-reply.js — and the expected replies in web/tools/reply-encode.mjs",
    );
    assert_eq!(decode_replies(&bytes), Ok(expected));
}
