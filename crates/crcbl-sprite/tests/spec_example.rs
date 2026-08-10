//! `tests/spec_example.crpix` — the worked example from
//! `docs/specs/crcbl/pix.md` — held to what §8 claims about it.
//!
//! A specification whose example does not parse is worse than none: it is the
//! first thing anyone copies. The example is a file rather than a string
//! literal here so it can be diffed against the document, and the assertions
//! below are the document's own sentences turned into code — "bakes to a 24×6
//! PNG and a sidecar whose `frameTags` holds one `flap` tag from 0 to 2,
//! forward, repeating forever, with each frame at 100 ms".
//!
//! When this test and the spec disagree, one of them is wrong and the disagree-
//! ment is the point of having it.
//!
//! # Why it is not part of `round_trip.rs`, which loads the same fixture
//!
//! Different subjects, and they fail for different reasons. This one is about
//! the **document**: it re-reads §8's prose and goes red when the spec and the
//! implementation drift apart. `round_trip.rs` is about `bake` and `load` being
//! inverses and would go on passing while both halves agreed on something the
//! spec never said. Naming this file after the fixture rather than after
//! "spec" is what keeps that split visible, since both files test the spec.
//!
//! It is a target of its own rather than a `#[cfg(test)]` module because the
//! spec describes what a *consumer* gets: it reaches `crpix::parse` and
//! `bake::bake` through the crate's public surface, and a claim the document
//! makes that only a `pub(crate)` item can check is a claim the document should
//! not be making.

#![cfg(feature = "bake")]

use crcbl_sprite::{SampleMode, bake, crpix};

/// Kept byte-for-byte in step with §8 of the spec.
const EXAMPLE: &str = include_str!("spec_example.crpix");

#[test]
fn the_specs_worked_example_parses_and_bakes_as_documented() {
    let art = match crpix::parse(EXAMPLE) {
        Ok(art) => art,
        Err(error) => panic!("the spec's own example does not parse: {error}"),
    };

    // §8: three 8x6 frames, five colours, one character per pixel.
    assert_eq!(art.header.width, 8);
    assert_eq!(art.header.height, 6);
    assert_eq!(art.header.frames, 3);
    assert_eq!(art.header.colours, 5);
    assert_eq!(art.header.chars, 1);
    assert_eq!(art.frames.len(), 3);
    assert_eq!(art.sample, SampleMode::Pixel);

    let baked = bake::bake(&art, "bird.png", 60).expect("this bakes");

    // "bakes to a 24x6 PNG"
    let decoder = png::Decoder::new(std::io::Cursor::new(&baked.png));
    let mut reader = decoder.read_info().expect("a readable PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a known size")];
    let info = reader.next_frame(&mut buffer).expect("one image");
    assert_eq!(
        (info.width, info.height),
        (24, 6),
        "three 8-wide frames in a strip"
    );

    // "…and a sidecar whose frameTags holds one `flap` tag from 0 to 2,
    //  forward, repeating forever, with each frame at 100 ms."
    let json = baked.json.expect("three frames and a clip need a sidecar");
    assert!(
        json.contains("\"name\": \"flap\", \"from\": 0, \"to\": 2, \"direction\": \"forward\""),
        "{json}"
    );
    assert!(
        !json.contains("\"repeat\""),
        "a looping clip repeats forever, which Aseprite spells as no field at all"
    );
    assert_eq!(
        json.matches("\"duration\": 100").count(),
        3,
        "six ticks at 60 Hz is 100 ms, on every frame: {json}"
    );

    // §7: a still sheet gets no sidecar, so this one having one is a claim
    // about the rule and not an accident of the fixture.
    assert!(art.needs_metadata());
}
