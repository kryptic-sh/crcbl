//! Bake a `.crpix`, load it back, and hold the two halves to being inverses.
//!
//! `bake` and `load` are the only pair in this crate that has to agree byte for
//! byte across a file boundary, and each half has tests that pass with the
//! other half wrong: the baker's tests read the JSON it wrote as text, and the
//! loader's read a sidecar written by hand in its own test module. Neither
//! notices a field one of them renamed. This does.
//!
//! The fixture is the specification's own worked example, so a disagreement
//! here is a disagreement with `docs/specs/crcbl/pix.md` §7 and §8.

#![cfg(all(feature = "bake", feature = "load"))]

use crcbl_sprite::{Direction, Rect, SampleMode, bake, crpix, load};

/// Kept byte-for-byte in step with §8 of the spec.
const EXAMPLE: &str = include_str!("spec_example.crpix");

const TICK_HZ: u32 = 60;

/// Every field the two formats share survives the trip: rects, names, holds,
/// the clip's range and direction and looping, and the sheet's size.
#[test]
fn the_specs_worked_example_bakes_and_loads_back_to_the_sheet_it_started_as() {
    let art = crpix::parse(EXAMPLE).expect("the spec's own example parses");
    let (_, before) = art.to_sheet();
    let baked = bake::bake(&art, "bird.png", TICK_HZ).expect("this bakes");
    let json = baked.json.as_deref().expect("three frames need a sidecar");

    let loaded = load::load(&baked.png, Some(json), TICK_HZ).expect("what was baked loads");
    let after = &loaded.sheet;

    assert_eq!((after.width, after.height), (24, 6), "§8: a 24x6 sheet");
    assert_eq!(after.frames.len(), 3, "§8: three frames");
    assert_eq!(after.frames, before.frames, "every rect, name and hold");
    // Spelled out as well as compared, so a `Frame` that lost a field to a
    // refactor cannot make both sides equally empty.
    assert_eq!(after.frames[0].name, "up");
    assert_eq!(after.frames[1].name, "level");
    assert_eq!(after.frames[2].name, "down");
    assert_eq!(after.frames[2].rect, Rect::new(16, 0, 8, 6));
    assert_eq!(after.frames[0].hold, 6, "§8: six ticks");

    assert_eq!(after.clips.len(), 1);
    assert_eq!(after.clips, before.clips);
    assert_eq!(after.clips[0].name, "flap");
    assert_eq!(after.clips[0].frames, vec![0, 1, 2], "§8: from 0 to 2");
    assert_eq!(after.clips[0].direction, Direction::Forward);
    assert!(after.clips[0].looping, "§8: repeating forever");

    assert_eq!(after.nine, before.nine, "this example has none");
    assert_eq!(after.nine, None);
}

/// **The one field that does not survive.** Aseprite's schema has no place for
/// a sample mode, so `bake` writes nothing and `load` reads the default back.
/// The spec's example says `sample: pixel`, which *is* the default — so a
/// round trip of that file alone would pass whether this worked or not. A
/// `smooth` sheet is the case that tells the truth.
#[test]
fn the_sample_mode_is_lost_in_the_schema_and_the_spec_example_hides_it() {
    let art = crpix::parse(
        "crpix: 2 2 2 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kk\n  kk\n\n\
         frame b:\n  kk\n  kk\n\nsample: smooth\n",
    )
    .expect("this parses");
    let (_, before) = art.to_sheet();
    assert_eq!(before.sample, SampleMode::Smooth);

    let baked = bake::bake(&art, "a.png", TICK_HZ).expect("this bakes");
    let json = baked.json.as_deref().expect("two frames need a sidecar");
    assert!(
        !json.contains("sample") && !json.contains("smooth"),
        "there is no field for it: {json}"
    );

    let after = load::load(&baked.png, Some(json), TICK_HZ).expect("loads");
    assert_eq!(after.sheet.frames.len(), 2, "the rest did survive");
    assert_eq!(
        after.sheet.sample,
        SampleMode::Pixel,
        "the mode came back as the default, not as `smooth`"
    );
}

/// A nine-slice is written as Aseprite's centre rect and read back as insets,
/// and the example has none — so it needs a fixture of its own or the
/// conversion is untested end to end.
#[test]
fn a_nine_slice_survives_the_centre_rect_it_is_written_as() {
    let art = crpix::parse(
        "crpix: 8 6 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkkkkkk\n  kkkkkkkk\n  \
         kkkkkkkk\n  kkkkkkkk\n  kkkkkkkk\n  kkkkkkkk\n\nnine: 2 3 1 4\n",
    )
    .expect("this parses");
    let (_, before) = art.to_sheet();
    let baked = bake::bake(&art, "a.png", TICK_HZ).expect("this bakes");
    let json = baked.json.as_deref().expect("a nine-slice needs a sidecar");

    let after = load::load(&baked.png, Some(json), TICK_HZ).expect("loads");
    assert_eq!(
        after.sheet.nine,
        Some(crcbl_sprite::NineSlice::new(2, 3, 1, 4)),
        "insets, not a transposition of them"
    );
    assert_eq!(after.sheet.nine, before.nine);
}

/// A one-shot clip is `repeat: "1"`, and must not come back looping — a death
/// animation that loops is a bug the player sees.
#[test]
fn a_one_shot_clip_comes_back_a_one_shot() {
    let art = crpix::parse(
        "crpix: 1 1 2 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\nframe b:\n  k\n\n\
         clip die: a b @ 3\n",
    )
    .expect("this parses");
    let baked = bake::bake(&art, "a.png", TICK_HZ).expect("this bakes");
    let json = baked.json.as_deref().expect("a clip needs a sidecar");
    let after = load::load(&baked.png, Some(json), TICK_HZ).expect("loads");
    assert_eq!(after.sheet.clips.len(), 1);
    assert!(!after.sheet.clips[0].looping, "`repeat: \"1\"` plays once");
}

/// **The pixels.** Every byte of the baked PNG is the RGBA the `.crpix`
/// describes, decoded back — the first test in this crate that checks an image
/// rather than a text round trip.
#[test]
fn the_baked_png_decodes_to_exactly_the_pixels_the_crpix_describes() {
    let art = crpix::parse(EXAMPLE).expect("the spec's own example parses");
    let (rgba, sheet) = art.to_sheet();
    let baked = bake::bake(&art, "bird.png", TICK_HZ).expect("this bakes");

    let image = load::decode_png(&baked.png).expect("what was baked decodes");
    assert_eq!((image.width, image.height), (sheet.width, sheet.height));
    assert_eq!(image.pixels.len(), 24 * 6 * 4, "24x6 RGBA8");
    assert_eq!(image.pixels, rgba, "byte for byte");

    // And the bytes are the palette the file spells out, not a uniform block
    // that would compare equal to any other uniform block. `#241c1c` is `k`,
    // the outline; the top-left corner is `.`, which is `None`.
    let at = |x: usize, y: usize| &image.pixels[(y * 24 + x) * 4..(y * 24 + x) * 4 + 4];
    assert_eq!(at(0, 0), [0, 0, 0, 0], "`.` is fully transparent");
    assert_eq!(at(2, 0), [0x24, 0x1c, 0x1c, 0xff], "`k`");
    assert_eq!(at(3, 2), [0xff, 0xff, 0xff, 0xff], "`w`, the eye");
    assert_eq!(at(1, 1), [0x24, 0x1c, 0x1c, 0xff], "`k`");
    assert_eq!(at(2, 1), [0xf2, 0xc1, 0x4e, 0xff], "`y`, the body");
    assert_eq!(at(3, 4), [0xe0, 0x8a, 0x3c, 0xff], "`o`, the beak");
    // Frame 2 starts at x = 8 and is the same drawing.
    assert_eq!(at(10, 1), at(2, 1), "frame `level` repeats frame `up`");
}

/// **The lossy conversion, proved rather than sampled.** `duration_ms` is
/// `ceil(hold * 1000 / tick_hz)` and `hold_ticks` is `floor(ms * tick_hz /
/// 1000)`; the second undoes the first exactly for every `tick_hz <= 1000`.
#[test]
fn every_hold_survives_the_millisecond_round_trip_up_to_a_thousand_hertz() {
    for tick_hz in [30, 60, 120, 1000] {
        for hold in 1..=600u32 {
            let ms = bake::duration_ms(hold, tick_hz);
            assert_eq!(
                load::hold_ticks(ms, tick_hz),
                hold,
                "{hold} ticks at {tick_hz} Hz wrote {ms} ms and came back wrong"
            );
        }
    }
}

/// And the boundary, stated as a fact rather than avoided: 1000 Hz is the last
/// rate that survives, and above it the encoding loses information no reader
/// can recover.
#[test]
fn above_a_thousand_hertz_the_millisecond_encoding_is_lossy() {
    // The first failure at the first rate that has one.
    assert_eq!(bake::duration_ms(1000, 1001), 1000);
    assert_eq!(
        load::hold_ticks(1000, 1001),
        1001,
        "1001 Hz is past the boundary, and this is where it first shows"
    );

    // Two different holds that write the same millisecond count: no inverse
    // exists, so this is a property of the format and not of the reader.
    assert_eq!(bake::duration_ms(1, 2000), 1);
    assert_eq!(bake::duration_ms(2, 2000), 1);

    // 1000 Hz itself is exact, which is what makes 1001 the boundary.
    for hold in 1..=600u32 {
        assert_eq!(load::hold_ticks(bake::duration_ms(hold, 1000), 1000), hold);
    }
}

/// A still sprite bakes to a PNG and nothing else, and loading it back is the
/// path with no sidecar at all.
#[test]
fn a_still_sprite_bakes_to_no_sidecar_and_loads_as_one_frame() {
    let art = crpix::parse("crpix: 3 2 1 1 1\n\npalette:\n  k c #0f0\n\nframe a:\n  kkk\n  kkk\n")
        .expect("this parses");
    let baked = bake::bake(&art, "a.png", TICK_HZ).expect("this bakes");
    assert_eq!(baked.json, None, "§7: a still sheet gets no sidecar");

    let loaded = load::load(&baked.png, baked.json.as_deref(), TICK_HZ).expect("it loads anyway");
    assert_eq!(loaded.sheet.frames.len(), 1);
    assert_eq!(loaded.sheet.frames[0].rect, Rect::new(0, 0, 3, 2));
    assert_eq!(loaded.image.pixels.len(), 3 * 2 * 4);
    assert_eq!(&loaded.image.pixels[..4], [0x00, 0xff, 0x00, 0xff]);
}
