//! The other direction: images in, one `.crpix` out.
//!
//! For getting existing art *into* the format — a PNG someone drew in Aseprite,
//! a frame exported from GIMP, or a sheet this repository already bakes and
//! somebody now wants to edit as text. `crcbl crpix` is the command; this is
//! what it calls.
//!
//! # Several images make one sheet
//!
//! Frames are given in playback order, one image each, and become one `.crpix`
//! with one `frame` section apiece. **Every image must be the same size**, and a
//! mismatch is an error rather than a pad or a crop: an animation whose frames
//! are different sizes is not an animation, and silently padding one would put
//! the bird a pixel higher on every third frame with nothing to point at.
//!
//! # Choosing the palette
//!
//! Colours are numbered in the order they are first met, scanning each image
//! left to right and top to bottom, images in the order given. That is stable —
//! the same input always produces the same file — and it puts the colours a
//! reader meets first at the top of the palette, which is roughly the order a
//! human would have written them in.
//!
//! Keys come from [`ALPHABET`], with fully transparent pixels always taking
//! `.`, the convention every hand-written sheet in this repository uses. When
//! there are more colours than characters the keys become two characters wide
//! and the header says so — the same escape hatch XPM has, for the same reason.
//!
//! # This is a conversion, not a quantiser
//!
//! It reproduces exactly the colours it is given and refuses art that is not
//! pixel art. A photograph has tens of thousands of distinct colours and would
//! produce a palette no one can read and a file larger than the PNG it came
//! from; [`MAX_COLOURS`] is where that is called an error instead. Reducing an
//! image's colours is a job for the tool that drew it.

use crate::crpix::Header;
use crate::{NineSlice, SampleMode};
use core::fmt;

/// One frame's pixels, as given to [`trace`].
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8, `width * height * 4` bytes.
    pub rgba: &'a [u8],
}

/// The characters a generated palette draws its keys from.
///
/// Digits and letters first because they read as a palette rather than as line
/// noise, then punctuation. Excluded on purpose: `#`, which opens a comment and
/// so can never be a key; the space, which rows are trimmed of; and `.`, which
/// is reserved for transparency so that a generated file looks like a
/// hand-written one.
pub const ALPHABET: &[u8] =
    b"kwrgbycmoptvzafhijlnqsuxde0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-*/=<>~^&%$@!?:;,'\"()[]{}|`_";

/// The most colours [`trace`] will turn into a palette.
///
/// Two characters per pixel over [`ALPHABET`] would allow thousands, and a
/// sheet with thousands of colours is a photograph rather than pixel art — the
/// file would be unreadable and larger than the PNG. 512 is comfortably above
/// any hand-drawn sprite and far below any photograph.
pub const MAX_COLOURS: usize = 512;

/// Why tracing failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// No images were given.
    NoImages,
    /// An image's buffer is not `width * height * 4` bytes.
    WrongBufferLength {
        name: String,
        expected: usize,
        found: usize,
    },
    /// An image has no area.
    EmptyImage { name: String },
    /// Frames of different sizes, which is not an animation.
    SizeMismatch {
        first: String,
        first_size: (u32, u32),
        offender: String,
        offender_size: (u32, u32),
    },
    /// More distinct colours than a readable palette can hold.
    TooManyColours { found: usize },
    /// Two frames with the same name, which a clip could not tell apart.
    DuplicateName { name: String },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoImages => write!(f, "no images to trace"),
            Self::WrongBufferLength {
                name,
                expected,
                found,
            } => write!(
                f,
                "`{name}` is {found} bytes and its size needs {expected} — RGBA8, four \
                 bytes a pixel"
            ),
            Self::EmptyImage { name } => write!(f, "`{name}` has no pixels"),
            Self::SizeMismatch {
                first,
                first_size,
                offender,
                offender_size,
            } => write!(
                f,
                "`{offender}` is {}x{} and `{first}` is {}x{}; every frame of a sheet is \
                 the same size, and padding one silently would move the art on that frame \
                 alone",
                offender_size.0, offender_size.1, first_size.0, first_size.1
            ),
            Self::TooManyColours { found } => write!(
                f,
                "{found} distinct colours, and the limit is {MAX_COLOURS}. This looks like \
                 a photograph rather than pixel art; reduce its colours in the tool that \
                 drew it"
            ),
            Self::DuplicateName { name } => {
                write!(f, "two frames are both called `{name}`")
            }
        }
    }
}

impl core::error::Error for TraceError {}

/// Options a caller may set on the generated file.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub nine: Option<NineSlice>,
    pub sample: SampleMode,
    /// A clip over every frame, in order, named this. `None` writes no clip.
    pub clip: Option<&'static str>,
    /// The hold that clip gives each frame, in ticks.
    pub hold: u32,
}

/// Turns `images` into the text of a `.crpix`.
///
/// # Errors
///
/// [`TraceError`] if the images disagree about their size, an image's buffer is
/// the wrong length, or there are too many distinct colours to make a palette
/// anyone can read.
pub fn trace(images: &[Image<'_>], options: &Options) -> Result<String, TraceError> {
    let first = images.first().ok_or(TraceError::NoImages)?;
    if first.width == 0 || first.height == 0 {
        return Err(TraceError::EmptyImage {
            name: first.name.to_string(),
        });
    }

    for (index, image) in images.iter().enumerate() {
        if (image.width, image.height) != (first.width, first.height) {
            return Err(TraceError::SizeMismatch {
                first: first.name.to_string(),
                first_size: (first.width, first.height),
                offender: image.name.to_string(),
                offender_size: (image.width, image.height),
            });
        }
        let expected = (image.width as usize) * (image.height as usize) * 4;
        if image.rgba.len() != expected {
            return Err(TraceError::WrongBufferLength {
                name: image.name.to_string(),
                expected,
                found: image.rgba.len(),
            });
        }
        if images[..index].iter().any(|other| other.name == image.name) {
            return Err(TraceError::DuplicateName {
                name: image.name.to_string(),
            });
        }
    }

    // Colours in first-seen order, so the same input always produces the same
    // file and the palette reads roughly the way a person would have written it.
    let mut palette: Vec<[u8; 4]> = Vec::new();
    for image in images {
        for pixel in image.rgba.chunks_exact(4) {
            let colour = normalise([pixel[0], pixel[1], pixel[2], pixel[3]]);
            if !palette.contains(&colour) {
                if palette.len() == MAX_COLOURS {
                    return Err(TraceError::TooManyColours {
                        found: count_colours(images),
                    });
                }
                palette.push(colour);
            }
        }
    }

    let transparent = palette.iter().position(|c| c[3] == 0);
    // Transparency does not consume a character from the alphabet, so a sheet
    // of N opaque colours needs exactly N of them.
    let opaque = palette.len() - usize::from(transparent.is_some());
    let chars = if opaque <= ALPHABET.len() { 1 } else { 2 };
    if opaque > ALPHABET.len() * ALPHABET.len() {
        return Err(TraceError::TooManyColours {
            found: palette.len(),
        });
    }

    let keys = assign_keys(&palette, transparent, chars);
    let header = Header {
        width: first.width,
        height: first.height,
        frames: images.len() as u32,
        colours: palette.len() as u32,
        chars: chars as u32,
    };
    Ok(render(images, &palette, &keys, header, options))
}

/// Fully transparent pixels are one colour whatever RGB they carry.
///
/// A PNG often holds several — a paint program leaves whatever was underneath —
/// and each would otherwise take a palette entry and a key that no reader can
/// tell apart. Zeroed to match what the parser produces for `None`, which is
/// also what keeps a sampler from haloing.
fn normalise(colour: [u8; 4]) -> [u8; 4] {
    if colour[3] == 0 { [0, 0, 0, 0] } else { colour }
}

/// How many distinct colours there really are, for the error message.
fn count_colours(images: &[Image<'_>]) -> usize {
    let mut seen: Vec<[u8; 4]> = Vec::new();
    for image in images {
        for pixel in image.rgba.chunks_exact(4) {
            let colour = normalise([pixel[0], pixel[1], pixel[2], pixel[3]]);
            if !seen.contains(&colour) {
                seen.push(colour);
            }
        }
    }
    seen.len()
}

/// A key per palette entry: `.` for transparency, then [`ALPHABET`] in order.
fn assign_keys(palette: &[[u8; 4]], transparent: Option<usize>, chars: usize) -> Vec<String> {
    let mut keys = Vec::with_capacity(palette.len());
    let mut next = 0usize;
    for index in 0..palette.len() {
        if Some(index) == transparent {
            keys.push(".".repeat(chars));
            continue;
        }
        keys.push(key_at(next, chars));
        next += 1;
    }
    keys
}

/// The `n`th key of `chars` characters, counting in base [`ALPHABET`].
fn key_at(n: usize, chars: usize) -> String {
    let base = ALPHABET.len();
    if chars == 1 {
        return (ALPHABET[n] as char).to_string();
    }
    let high = ALPHABET[n / base] as char;
    let low = ALPHABET[n % base] as char;
    [high, low].iter().collect()
}

fn render(
    images: &[Image<'_>],
    palette: &[[u8; 4]],
    keys: &[String],
    header: Header,
    options: &Options,
) -> String {
    let mut out = String::with_capacity(1024);
    out.push_str("# generated by `crcbl crpix`; edit freely\n");
    out.push_str(&format!(
        "crpix: {} {} {} {} {}   # w h frames colours chars-per-pixel\n\npalette:\n",
        header.width, header.height, header.frames, header.colours, header.chars
    ));
    for (colour, key) in palette.iter().zip(keys) {
        if colour[3] == 0 {
            out.push_str(&format!("  {key} c None\n"));
        } else if colour[3] == 255 {
            out.push_str(&format!(
                "  {key} c #{:02x}{:02x}{:02x}\n",
                colour[0], colour[1], colour[2]
            ));
        } else {
            out.push_str(&format!(
                "  {key} c #{:02x}{:02x}{:02x}{:02x}\n",
                colour[0], colour[1], colour[2], colour[3]
            ));
        }
    }

    for image in images {
        out.push_str(&format!("\nframe {}:\n", image.name));
        for y in 0..image.height {
            out.push_str("  ");
            for x in 0..image.width {
                let at = ((y * image.width + x) * 4) as usize;
                let colour = normalise([
                    image.rgba[at],
                    image.rgba[at + 1],
                    image.rgba[at + 2],
                    image.rgba[at + 3],
                ]);
                let index = palette
                    .iter()
                    .position(|candidate| *candidate == colour)
                    .expect("every colour was collected in the pass above");
                out.push_str(&keys[index]);
            }
            out.push('\n');
        }
    }

    if let Some(clip) = options.clip {
        let names: Vec<&str> = images.iter().map(|image| image.name).collect();
        out.push_str(&format!("\nclip {clip}: {}", names.join(" ")));
        if options.hold > 1 {
            out.push_str(&format!(" @ {}", options.hold));
        }
        out.push_str(" loop\n");
    }
    if let Some(nine) = options.nine {
        out.push_str(&format!(
            "\nnine: {} {} {} {}\n",
            nine.left, nine.right, nine.top, nine.bottom
        ));
    }
    if options.sample == SampleMode::Smooth {
        out.push_str("\nsample: smooth\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crpix;

    /// Four pixels: transparent, red, green, half-alpha blue.
    const FOUR: [u8; 16] = [
        0, 0, 0, 0, //
        255, 0, 0, 255, //
        0, 255, 0, 255, //
        0, 0, 255, 128,
    ];

    fn image<'a>(name: &'a str, rgba: &'a [u8]) -> Image<'a> {
        Image {
            name,
            width: 2,
            height: 2,
            rgba,
        }
    }

    /// The property that matters: what comes out parses, and parses back to the
    /// pixels that went in. Everything else about the generated text is taste.
    #[test]
    fn a_traced_image_round_trips_through_the_parser() {
        let text = trace(&[image("a", &FOUR)], &Options::default()).expect("this traces");
        let art = match crpix::parse(&text) {
            Ok(art) => art,
            Err(error) => panic!("generated text does not parse: {error}\n{text}"),
        };
        assert_eq!(art.frames.len(), 1);
        assert_eq!(art.frames[0].name, "a");
        assert_eq!(art.frames[0].pixels, FOUR, "the pixels changed:\n{text}");
    }

    /// Several images become one sheet, in the order given.
    #[test]
    fn several_images_become_one_sheet_of_frames() {
        // Reversed by *pixel*, not by byte: reversing the bytes would put an
        // alpha where a red belongs and produce a fixture that is not an image.
        let second: Vec<u8> = FOUR.chunks_exact(4).rev().flatten().copied().collect();
        let text = trace(
            &[image("up", &FOUR), image("down", &second)],
            &Options {
                clip: Some("flap"),
                hold: 6,
                ..Options::default()
            },
        )
        .expect("this traces");

        let art = crpix::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(art.header.frames, 2);
        assert_eq!(art.frames[0].name, "up");
        assert_eq!(art.frames[1].name, "down");
        assert_eq!(art.frames[0].pixels, FOUR);
        assert_eq!(art.frames[1].pixels, second);
        assert_eq!(art.clips.len(), 1);
        assert_eq!(art.clips[0].name, "flap");
        assert_eq!(art.frames[0].hold, 6);
        assert!(art.clips[0].looping);
    }

    /// **Frames of different sizes are an error.** Padding one silently would
    /// move the art on that frame alone, which reads as the animation jittering
    /// and points at nothing.
    #[test]
    fn images_of_different_sizes_are_refused() {
        let tall = [0u8; 2 * 3 * 4];
        let error = trace(
            &[
                image("a", &FOUR),
                Image {
                    name: "b",
                    width: 2,
                    height: 3,
                    rgba: &tall,
                },
            ],
            &Options::default(),
        )
        .expect_err("2x3 is not 2x2");
        assert_eq!(
            error,
            TraceError::SizeMismatch {
                first: "a".into(),
                first_size: (2, 2),
                offender: "b".into(),
                offender_size: (2, 3),
            }
        );
        assert!(error.to_string().contains("same size"), "{error}");
    }

    /// The header the tracer writes has to be the header the parser checks
    /// against, or every generated file is refused by its own format.
    #[test]
    fn the_generated_header_matches_what_was_generated() {
        let text = trace(&[image("a", &FOUR)], &Options::default()).expect("traces");
        let art = crpix::parse(&text).expect("parses");
        assert_eq!(art.header.width, 2);
        assert_eq!(art.header.height, 2);
        assert_eq!(art.header.frames, 1);
        assert_eq!(art.header.colours, 4);
        assert_eq!(art.header.chars, 1);
    }

    /// Transparency takes `.`, as every hand-written sheet here does, and all
    /// transparent pixels are one colour however the source spelled them.
    #[test]
    fn transparency_is_one_colour_and_takes_the_conventional_key() {
        // Two "transparent" pixels with different leftover RGB, and one opaque.
        let messy = [
            0, 0, 0, 0, //
            200, 30, 90, 0, //
            17, 34, 51, 255, //
            17, 34, 51, 255,
        ];
        let text = trace(&[image("a", &messy)], &Options::default()).expect("traces");
        assert!(text.contains("  . c None"), "{text}");

        let art = crpix::parse(&text).expect("parses");
        assert_eq!(art.header.colours, 2, "two colours, not three:\n{text}");
        assert_eq!(&art.frames[0].pixels[0..4], [0, 0, 0, 0]);
        assert_eq!(
            &art.frames[0].pixels[4..8],
            [0, 0, 0, 0],
            "a transparent pixel keeps no colour, or it haloes"
        );
    }

    /// More colours than the alphabet means two characters a pixel, which is
    /// XPM's own escape hatch — and it still has to round-trip.
    #[test]
    fn a_palette_larger_than_the_alphabet_widens_the_keys() {
        let count = ALPHABET.len() + 5;
        let mut rgba = Vec::new();
        for i in 0..count {
            rgba.extend_from_slice(&[(i % 251) as u8, (i / 251) as u8, 7, 255]);
        }
        let wide = Image {
            name: "a",
            width: count as u32,
            height: 1,
            rgba: &rgba,
        };
        let text = trace(&[wide], &Options::default()).expect("traces");
        let art = crpix::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(art.header.chars, 2, "{text}");
        assert_eq!(art.header.colours as usize, count);
        assert_eq!(art.frames[0].pixels, rgba, "the pixels changed");
    }

    /// A photograph is refused with the reason, rather than producing a palette
    /// nobody can read and a file bigger than the PNG.
    #[test]
    fn art_with_too_many_colours_is_refused() {
        let count = MAX_COLOURS + 10;
        let mut rgba = Vec::new();
        for i in 0..count {
            rgba.extend_from_slice(&[(i % 256) as u8, (i / 256) as u8, 11, 255]);
        }
        let photo = Image {
            name: "a",
            width: count as u32,
            height: 1,
            rgba: &rgba,
        };
        let error = trace(&[photo], &Options::default()).expect_err("too many colours");
        assert!(matches!(error, TraceError::TooManyColours { .. }));
        assert!(error.to_string().contains("photograph"), "{error}");
    }

    #[test]
    fn the_remaining_refusals_each_say_what_is_wrong() {
        assert_eq!(
            trace(&[], &Options::default()).expect_err("nothing to do"),
            TraceError::NoImages
        );
        let short = [0u8; 4];
        assert!(matches!(
            trace(&[image("a", &short)], &Options::default()).expect_err("short buffer"),
            TraceError::WrongBufferLength { .. }
        ));
        assert!(matches!(
            trace(
                &[Image {
                    name: "a",
                    width: 0,
                    height: 2,
                    rgba: &[]
                }],
                &Options::default()
            )
            .expect_err("no pixels"),
            TraceError::EmptyImage { .. }
        ));
        assert!(matches!(
            trace(&[image("a", &FOUR), image("a", &FOUR)], &Options::default())
                .expect_err("two frames called `a`"),
            TraceError::DuplicateName { .. }
        ));
    }

    /// Nine-slice and sample mode ride along, so a converted sheet does not
    /// lose what the caller knows about it.
    #[test]
    fn the_options_reach_the_generated_file() {
        let text = trace(
            &[image("a", &FOUR)],
            &Options {
                nine: Some(NineSlice::new(1, 1, 1, 1)),
                sample: SampleMode::Smooth,
                ..Options::default()
            },
        )
        .expect("traces");
        let art = crpix::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(art.nine, Some(NineSlice::new(1, 1, 1, 1)));
        assert_eq!(art.sample, SampleMode::Smooth);
    }

    /// The same input twice gives the same file, so re-running the converter
    /// does not churn a diff.
    #[test]
    fn tracing_the_same_image_twice_gives_a_byte_identical_file() {
        let once = trace(&[image("a", &FOUR)], &Options::default()).expect("traces");
        let twice = trace(&[image("a", &FOUR)], &Options::default()).expect("traces");
        assert_eq!(once, twice);
    }
}
