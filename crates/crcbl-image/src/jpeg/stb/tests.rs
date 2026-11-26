use crate::jpeg::tests::{ORIENTED, first_difference};
use crate::jpeg::{decode_as_stb_image, decode_stored};
use crate::{ImageError, Limits};

/// Each file beside stb_image's decode of it: v2.30, as vendored in
/// llama.cpp at 57291f2 (`llama.cpp/vendor/stb/stb_image.h`), built as
/// llama.cpp's `mtmd-helper.cpp` builds it (`STB_IMAGE_IMPLEMENTATION`
/// and no other option) by g++ 16 at `-O2` on x86-64, so its SSE2
/// kernels ran, and loaded with `stbi_load_from_memory(…, 3)`. The same
/// harness built with `-DSTBI_NO_SIMD` wrote identical bytes for every
/// file, these and all the others they were picked from, except the two
/// `idct-sse2` files whose coefficients overflow 16 bits in the SSE2
/// transform: `sum04` and `q32` (quantiser 32, coefficients of 1000 at
/// natural positions 8 and 56). Every output was checked again by three
/// more builds of the same harness whose stb_image fills its sample and
/// coefficient buffers with `0x00`, `0x37` and `0xc8` straight after
/// allocating them, each file in a process of its own: all three wrote
/// these bytes, so none of them is memory stb_image left unwritten.
///
/// libjpeg-turbo 3.2.1's `cjpeg` wrote most of them, from a seeded
/// gradient with noise, at quality 85 unless named: each chroma
/// sampling stb_image filters (4:2:2, 4:2:0, 4:4:0) and some it takes
/// the nearest sample for (4:1:1, luma 1 × 4 and 2 × 4), sizes that
/// leave chroma one or two samples wide, progressive, restart intervals
/// (one an MCU), grey, and RGB; ImageMagick 7.1 wrote a progressive
/// 4:2:2. A script edited copies for the stream details stb_image reads
/// its own way: a fill byte before a stuffed zero, an Adobe marker
/// (transform 0, no JFIF) and a quantisation table both after the first
/// scan of a progressive file, `RGB` component ids beside a JFIF marker,
/// a progressive grey file's DC refinement scan read twice and its first
/// DC scan read again last, and two by hand: a sequential AC symbol of
/// size 0 and run 1, and an AC coefficient whose dequantised value
/// passes 16 bits (1023 · 64). Those marked
/// `shared` are the libjpeg-turbo fixtures above, decoded by stb_image.
///
/// The rest are damage stb_image still decodes, edits of the files above
/// picked by breaking each behaviour in turn and keeping the smallest
/// file that then failed: SSE2 transforms whose 16-bit sums wrap or
/// whose passes saturate (hand-made by the review of this decoder); a
/// byte flip leaving too few bits for a coefficient at a marker, which
/// stb_image reads as 0; bytes before the frame; a `DNL` marker
/// repeating the height; a baseline file cut short, which reads on as
/// zeros; bytes, an unknown marker, and a Huffman table of more than 256
/// codes (from the review), each after the last scan of a baseline file,
/// where stb_image stops with every block written; and a mutation from the
/// second review's corpus, a 9 × 9 progressive file damaged in more than
/// one scan, each reading the rest of itself from made-up bits, which only
/// the work bound's allowance for each entropy-coded byte lets through.
macro_rules! stb_fixture {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../fixtures/jpeg/stb/", $name, ".jpg")) as &[u8],
            include_bytes!(concat!("../../../fixtures/jpeg/stb/", $name, ".rgb")) as &[u8],
        )
    };
    ($name:literal, shared) => {
        (
            $name,
            include_bytes!(concat!("../../../fixtures/jpeg/", $name, ".jpg")) as &[u8],
            include_bytes!(concat!("../../../fixtures/jpeg/stb/", $name, ".rgb")) as &[u8],
        )
    };
}

const STB_FIXTURES: [(&str, &[u8], &[u8]); 43] = [
    stb_fixture!("baseline-444-17x9"),
    stb_fixture!("baseline-422-33x7"),
    stb_fixture!("baseline-420-17x9"),
    stb_fixture!("baseline-440-9x33"),
    stb_fixture!("baseline-411-33x7"),
    stb_fixture!("baseline-luma-1x4-9x33"),
    stb_fixture!("baseline-luma-2x4-17x9"),
    stb_fixture!("baseline-420-2x2"),
    stb_fixture!("baseline-422-2x2"),
    stb_fixture!("baseline-422-3x5"),
    stb_fixture!("baseline-420-3x5"),
    stb_fixture!("progressive-420-33x7"),
    stb_fixture!("progressive-restart-420-17x9"),
    stb_fixture!("restart-422-17x9"),
    stb_fixture!("grey-17x9"),
    stb_fixture!("progressive-grey-9x33"),
    stb_fixture!("rgb-17x9"),
    stb_fixture!("quality-30-422-17x9"),
    stb_fixture!("magick-progressive-422-17x9"),
    stb_fixture!("fill-stuffed-420"),
    stb_fixture!("late-adobe-progressive"),
    stb_fixture!("late-dqt-progressive"),
    stb_fixture!("rgb-ids-jfif"),
    stb_fixture!("sequential-run-symbol"),
    stb_fixture!("dequantised-wrap"),
    stb_fixture!("dc-refined-twice"),
    stb_fixture!("dc-scan-last"),
    stb_fixture!("idct-sse2-sum04"),
    stb_fixture!("idct-sse2-q32"),
    stb_fixture!("idct-sse2-saturate"),
    stb_fixture!("short-bits-at-marker"),
    stb_fixture!("junk-before-frame"),
    stb_fixture!("dnl-height"),
    stb_fixture!("truncated-baseline"),
    stb_fixture!("junk-after-last-scan"),
    stb_fixture!("marker-after-last-scan"),
    stb_fixture!("huffman-past-256-after-scans"),
    stb_fixture!("damaged-in-several-scans"),
    stb_fixture!("narrow-420", shared),
    stb_fixture!("baseline-422", shared),
    stb_fixture!("cmyk-adobe", shared),
    stb_fixture!("cmyk-plain", shared),
    stb_fixture!("ycck-420", shared),
];

/// The stb_image fixture of that name, its JPEG and stb_image's RGB.
pub(in crate::jpeg) fn stb_fixture(wanted: &str) -> (&'static [u8], &'static [u8]) {
    let (_, jpeg, rgb) = STB_FIXTURES
        .into_iter()
        .find(|&(name, ..)| name == wanted)
        .expect(wanted);
    (jpeg, rgb)
}

#[test]
fn every_stb_fixture_decodes_to_exactly_stb_images_pixels() {
    for (name, jpeg, rgb) in STB_FIXTURES {
        let image = decode_as_stb_image(jpeg, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(image.pixels().len() / 4, rgb.len() / 3, "{name}");
        assert_eq!(first_difference(&image, rgb), None, "{name}");
    }
}

/// stb_image ignores EXIF, so an oriented file decodes as stored.
#[test]
fn stb_images_pixels_are_stored_not_oriented() {
    let (_, _, six, _) = ORIENTED[5];
    let stored = decode_as_stb_image(six, &Limits::DEFAULT).expect("decodes");
    assert_eq!((stored.width(), stored.height()), (5, 3));
}

/// Files stb_image refuses, refused as malformed, and files where it
/// stops with memory it never wrote in its output, refused as
/// unsupported: a DC that overflows 16 bits once dequantised (from the
/// review), and a Huffman code a marker cuts short (an inserted `ff 01`);
/// an unknown marker after the last scan of a progressive file, and a
/// progressive file cut short, where stb_image stops before its inverse
/// DCT runs; a Huffman table of more than 256 codes between scans of a
/// progressive file (from the review); and a baseline file with restart
/// markers cut short, whose scan ends at the first interval without
/// its marker; and three from the second review where an AC refinement
/// reads blocks no DC first pass cleared, so the coefficients stb_image
/// finds nonzero, whatever memory held, steer the bits it reads: it
/// stops early on some memory and not on other, fails except on zeroed
/// memory, and (a mutated file) fails on zeroed memory but decodes on
/// other. On each of the undetermined ones the builds of the harness
/// that fill stb_image's buffers with `0x00`, `0x37` and `0xc8`
/// disagreed, in success or in pixels.
const STB_REFUSED: [(&str, &[u8], bool); 9] = [
    (
        "refused-dc-overflow",
        include_bytes!("../../../fixtures/jpeg/stb/refused-dc-overflow.jpg"),
        true,
    ),
    (
        "refused-code-cut-by-marker",
        include_bytes!("../../../fixtures/jpeg/stb/refused-code-cut-by-marker.jpg"),
        true,
    ),
    (
        "refused-marker-after-progressive",
        include_bytes!("../../../fixtures/jpeg/stb/refused-marker-after-progressive.jpg"),
        false,
    ),
    (
        "refused-truncated-progressive",
        include_bytes!("../../../fixtures/jpeg/stb/refused-truncated-progressive.jpg"),
        false,
    ),
    (
        "refused-progressive-huffman-past-256",
        include_bytes!("../../../fixtures/jpeg/stb/refused-progressive-huffman-past-256.jpg"),
        false,
    ),
    (
        "refused-restart-cut",
        include_bytes!("../../../fixtures/jpeg/stb/refused-restart-cut.jpg"),
        false,
    ),
    (
        "refused-ac-refine-stop",
        include_bytes!("../../../fixtures/jpeg/stb/refused-ac-refine-stop.jpg"),
        false,
    ),
    (
        "refused-ac-refine-before-dc",
        include_bytes!("../../../fixtures/jpeg/stb/refused-ac-refine-before-dc.jpg"),
        false,
    ),
    (
        "refused-garbage-decides-failure",
        include_bytes!("../../../fixtures/jpeg/stb/refused-garbage-decides-failure.jpg"),
        false,
    ),
];

#[test]
fn what_stb_image_refuses_or_leaves_unwritten_is_refused() {
    for (name, jpeg, malformed) in STB_REFUSED {
        let result = decode_as_stb_image(jpeg, &Limits::DEFAULT);
        if malformed {
            assert!(
                matches!(result, Err(ImageError::Malformed(_))),
                "{name}: {result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(ImageError::Unsupported(_))),
                "{name}: {result:?}"
            );
        }
    }
}

/// After the last scan of a baseline file every block is written, so
/// whatever stb_image stops at there, or skips, leaves the pixels as
/// they were: an unknown marker, `TEM`, a restart marker that does not
/// follow a scan, and a restart interval segment of the wrong length.
#[test]
fn a_stop_after_the_last_baseline_scan_keeps_the_pixels() {
    let (jpeg, rgb) = stb_fixture("baseline-420-17x9");
    let end = jpeg.len() - 2;
    let with = |inserted: &[u8]| {
        let mut bytes = jpeg[..end].to_vec();
        bytes.extend_from_slice(inserted);
        bytes.extend_from_slice(&jpeg[end..]);
        bytes
    };
    for (what, bytes) in [
        ("a restart marker after the scan", with(b"\xff\xd0")),
        ("a comment", with(b"\xff\xfe\x00\x04hi")),
        (
            "a restart marker after a comment",
            with(b"\xff\xfe\x00\x04hi\xff\xd0"),
        ),
        ("an unknown marker", with(b"\xff\xf0\x00\x04hi")),
        ("TEM", with(b"\xff\x01")),
        (
            "a long restart interval segment",
            with(b"\xff\xdd\x00\x05\x00\x01\x00"),
        ),
        ("no end-of-image marker", jpeg[..end].to_vec()),
    ] {
        let image = decode_as_stb_image(&bytes, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(first_difference(&image, rgb), None, "{what}");
    }
}

/// A progressive component no DC first pass reached is never cleared by
/// stb_image: the progressive grey file without its first scan, the DC
/// one, up to the Huffman table the next scan reads.
#[test]
fn a_progressive_component_without_a_dc_scan_is_refused() {
    let (progressive, _) = stb_fixture("progressive-grey-9x33");
    let scan = progressive
        .windows(2)
        .position(|pair| pair == [0xff, 0xda])
        .expect("a scan");
    let next_table = scan
        + progressive[scan..]
            .windows(2)
            .position(|pair| pair == [0xff, 0xc4])
            .expect("a table after it");
    let mut no_dc = progressive[..scan].to_vec();
    no_dc.extend_from_slice(&progressive[next_table..]);
    assert!(decode_stored(&no_dc, &Limits::DEFAULT).is_ok());
    assert!(matches!(
        decode_as_stb_image(&no_dc, &Limits::DEFAULT),
        Err(ImageError::Unsupported(_))
    ));
}
