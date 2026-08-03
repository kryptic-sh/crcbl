//! Baking a [`CrpixArt`] into the two files the engine actually loads.
//!
//! One `.crpix` in, a PNG out — and beside it an Aseprite-schema JSON sidecar,
//! but only when there is something to say. A single still frame with no clips
//! and no nine-slice is fully described by its image, and a sidecar reading
//! "one frame, the whole picture" is a file to fetch, parse and learn nothing
//! from. [`CrpixArt::needs_metadata`] is the rule.
//!
//! # Why the sidecar is Aseprite's schema and not ours
//!
//! Because then it is not ours. Art exported from Aseprite —
//! `--sheet x.png --data x.json --format json-array` — and art baked from a
//! `.crpix` produce the same two files, so the loader has one path and an
//! artist replacing hand-written art changes nothing downstream.
//!
//! Written by hand rather than through a serialiser: the schema is a dozen
//! fixed fields, this crate has no dependencies to speak of, and a build-side
//! crate pulling in a derive macro to emit sixty lines of JSON is a poor trade.
//!
//! # Where the two models disagree, and what is done about it
//!
//! * **Aseprite counts milliseconds; this engine counts ticks.** A sample's
//!   animation must advance the same way at 20 fps and 240 fps for the reason
//!   its physics must, and the only clock that is true of is the fixed tick. So
//!   [`bake`] converts once, at the tick rate it is told, and the loader
//!   converts back. `hold * 1000 / tick_hz`, rounded to the nearest
//!   millisecond, with a floor of 1 so a fast frame is never a zero-duration
//!   one that a reader treats as "skip".
//! * **A frame tag is a range.** `.crpix` enforces the same rule at parse time
//!   rather than discovering it here, so nothing that parses can fail to bake.
//! * **`repeat`.** A looping clip omits the field, which is Aseprite's
//!   "forever"; a one-shot writes `"repeat": "1"`.
//!
//! [`CrpixArt`]: crate::crpix::CrpixArt
//! [`CrpixArt::needs_metadata`]: crate::crpix::CrpixArt::needs_metadata

use crate::crpix::CrpixArt;
use crate::{Direction, Sheet};
use core::fmt;
use std::path::Path;

/// What one `.crpix` bakes into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baked {
    /// The sheet image, PNG-encoded, RGBA8.
    pub png: Vec<u8>,
    /// The Aseprite-schema sidecar, when the art has anything to say beyond a
    /// single still image.
    pub json: Option<String>,
}

/// Why baking failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BakeError {
    /// The PNG encoder refused the image.
    Png(String),
    /// A tick rate of zero, which no duration can be computed against.
    ZeroTickRate,
}

impl fmt::Display for BakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Png(message) => write!(f, "encoding the sheet as PNG failed: {message}"),
            Self::ZeroTickRate => write!(
                f,
                "a tick rate of zero: frame durations are `hold * 1000 / tick_hz`"
            ),
        }
    }
}

impl core::error::Error for BakeError {}

/// Bakes `art` into a PNG and, if it needs one, a sidecar.
///
/// `image` is the PNG's filename as the sidecar should refer to it — the
/// `meta.image` field, which a loader resolves relative to the JSON.
///
/// # Errors
///
/// [`BakeError`] if the encoder refused the image or `tick_hz` is zero.
pub fn bake(art: &CrpixArt, image: &str, tick_hz: u32) -> Result<Baked, BakeError> {
    if tick_hz == 0 {
        return Err(BakeError::ZeroTickRate);
    }
    let (rgba, sheet) = art.to_sheet();
    Ok(Baked {
        png: encode_png(&rgba, sheet.width, sheet.height)?,
        json: art
            .needs_metadata()
            .then(|| aseprite_json(&sheet, image, tick_hz)),
    })
}

/// Encodes straight RGBA8 as a PNG.
///
/// # Errors
///
/// [`BakeError::Png`] if the encoder refused the image.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, BakeError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Named rather than left to the crate's default, so a version bump cannot
    // silently change the bytes of an artifact a build may be caching or
    // hashing. `Balanced` because these images are a kilobyte and the encode
    // happens once per build.
    encoder.set_compression(png::Compression::Balanced);
    let mut writer = encoder
        .write_header()
        .map_err(|error| BakeError::Png(error.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| BakeError::Png(error.to_string()))?;
    writer
        .finish()
        .map_err(|error| BakeError::Png(error.to_string()))?;
    Ok(out)
}

/// The Aseprite-schema sidecar for `sheet`, in its `json-array` form.
#[must_use]
pub fn aseprite_json(sheet: &Sheet, image: &str, tick_hz: u32) -> String {
    let tick_hz = tick_hz.max(1);
    let mut out = String::with_capacity(512);
    out.push_str("{\n \"frames\": [\n");

    for (index, frame) in sheet.frames.iter().enumerate() {
        let rect = frame.rect;
        // Untrimmed and unrotated, always: this baker lays frames out as a
        // plain strip, so the source size *is* the frame size and the offset is
        // zero. A reader that honours `spriteSourceSize` gets the right answer
        // either way, and one that ignores it is not broken by us.
        out.push_str("  {\"filename\": ");
        push_json_string(&mut out, &frame.name);
        out.push_str(&format!(
            ", \"frame\": {{\"x\": {}, \"y\": {}, \"w\": {}, \"h\": {}}}, \
             \"rotated\": false, \"trimmed\": false, \
             \"spriteSourceSize\": {{\"x\": 0, \"y\": 0, \"w\": {}, \"h\": {}}}, \
             \"sourceSize\": {{\"w\": {}, \"h\": {}}}, \"duration\": {}}}",
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            rect.w,
            rect.h,
            rect.w,
            rect.h,
            duration_ms(frame.hold, tick_hz),
        ));
        if index + 1 < sheet.frames.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str(" ],\n \"meta\": {\n");
    out.push_str("  \"app\": \"https://github.com/kryptic-sh/crcbl\",\n");
    out.push_str("  \"version\": \"crcbl-sprite bake\",\n");
    out.push_str("  \"image\": ");
    push_json_string(&mut out, image);
    out.push_str(",\n  \"format\": \"RGBA8888\",\n");
    out.push_str(&format!(
        "  \"size\": {{\"w\": {}, \"h\": {}}},\n  \"scale\": \"1\"",
        sheet.width, sheet.height
    ));

    if !sheet.clips.is_empty() {
        out.push_str(",\n  \"frameTags\": [\n");
        for (index, clip) in sheet.clips.iter().enumerate() {
            let from = clip.frames.first().copied().unwrap_or(0);
            let to = clip.frames.last().copied().unwrap_or(from);
            out.push_str("   {\"name\": ");
            push_json_string(&mut out, &clip.name);
            out.push_str(&format!(
                ", \"from\": {from}, \"to\": {to}, \"direction\": \"{}\"",
                match clip.direction {
                    Direction::Forward => "forward",
                    Direction::Reverse => "reverse",
                    Direction::PingPong => "pingpong",
                }
            ));
            // Aseprite's absent `repeat` is "forever"; a one-shot says once.
            if !clip.looping {
                out.push_str(", \"repeat\": \"1\"");
            }
            out.push('}');
            if index + 1 < sheet.clips.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]");
    }

    if let Some(nine) = sheet.nine
        && let Some(first) = sheet.frames.first()
    {
        // Aseprite stores the *centre* of the nine, relative to the slice's own
        // bounds — the inverse of `NineSlice::from_center`, which is what reads
        // it back.
        let rect = first.rect;
        let centre_w = rect.w.saturating_sub(nine.left + nine.right);
        let centre_h = rect.h.saturating_sub(nine.top + nine.bottom);
        out.push_str(",\n  \"slices\": [\n");
        out.push_str(&format!(
            "   {{\"name\": \"nine\", \"color\": \"#0000ffff\", \"keys\": [\
             {{\"frame\": 0, \"bounds\": {{\"x\": {}, \"y\": {}, \"w\": {}, \"h\": {}}}, \
             \"center\": {{\"x\": {}, \"y\": {}, \"w\": {centre_w}, \"h\": {centre_h}}}}}]}}\n",
            rect.x, rect.y, rect.w, rect.h, nine.left, nine.top,
        ));
        out.push_str("  ]");
    }

    out.push_str("\n }\n}\n");
    out
}

/// A hold in ticks as Aseprite's milliseconds.
///
/// Floored at 1: a zero-duration frame is "skip me" to some readers, and a
/// sheet held for one tick at 240 Hz rounds to zero without it.
///
/// Public so the reader's inverse can be tested against it directly rather
/// than against a copy of the formula, which is the one thing a test of an
/// inverse must not do.
#[must_use]
pub fn duration_ms(hold: u32, tick_hz: u32) -> u32 {
    let ms = (u64::from(hold) * 1000).div_ceil(u64::from(tick_hz));
    u32::try_from(ms).unwrap_or(u32::MAX).max(1)
}

/// Appends `text` as a JSON string, escaping what RFC 8259 requires.
///
/// Frame and clip names come from a file somebody wrote, so a quote or a
/// backslash in one must not produce a sidecar that will not parse.
fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// The build-script half
// ---------------------------------------------------------------------------

/// How the generated statics are declared.
///
/// Two values because there are two kinds of consumer, and getting this wrong
/// is a compile error rather than anything subtle: a sample includes the table
/// into its own `art` module and re-exports nothing, while `crcbl-render` keeps
/// its menu art private to the crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// `pub static` — what the samples' `art_data.rs` needs.
    Public,
    /// `pub(crate) static` — what `crcbl-render`'s `menu_art.rs` needs.
    Crate,
}

impl Visibility {
    /// The keyword, as it is written into the generated file.
    ///
    /// **Nothing catches getting this wrong**, which is why it is a parameter
    /// rather than a constant. Widening a visibility is never a compile error,
    /// and `crcbl-render` includes its table into `pub mod menu` — so emitting
    /// `pub` there does not fail a build, it silently adds `MENU_PNG` and
    /// `MENU_JSON` to that crate's public API. Verified by doing it: the two
    /// statics appear in `cargo doc`'s output for `crcbl_render::menu`.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Public => "pub",
            Self::Crate => "pub(crate)",
        }
    }
}

/// What [`bake_dir`] is being asked to do.
///
/// A struct rather than six positional arguments, because four of them are
/// strings and a caller that swapped two would get a build that works and puts
/// the files somewhere surprising.
#[derive(Clone, Copy, Debug)]
pub struct BakeDir<'a> {
    /// The crate root — `CARGO_MANIFEST_DIR`. Assets are read from
    /// `<manifest_dir>/assets/<stem>.crpix`.
    pub manifest_dir: &'a Path,
    /// Where the baked files go — `OUT_DIR`.
    pub out_dir: &'a Path,
    /// The sheets, by file stem.
    pub stems: &'a [&'a str],
    /// The tick rate holds are converted against. Must match the loader's; see
    /// the module docs on the two clocks.
    pub tick_hz: u32,
    /// How the generated statics are declared.
    pub visibility: Visibility,
    /// The generated file's name inside `OUT_DIR` — `art_data.rs` for a sample,
    /// `menu_art.rs` for `crcbl-render`.
    pub table_name: &'a str,
    /// The asset directory as it is written into the `@generated` header, for a
    /// reader who has the generated file and wants the source.
    pub source_label: &'a str,
}

/// Bakes every sheet in a directory and writes the `include_bytes!` table.
///
/// The whole body of a build script, which five of them were writing out: the
/// `rerun-if-changed` line, the read, the parse, the bake, the two writes, and
/// the table of statics that `include!`s back into the crate.
///
/// # This is build-script code in a library
///
/// It reads files, writes files, prints `cargo::` directives and **exits the
/// process** on failure, which is not how a library behaves. That is
/// deliberate: a build script cannot report an error any other way that cargo
/// renders legibly, and the alternative — returning a `Result` every caller
/// unwraps identically — is the copy this exists to delete, minus two lines.
/// It lives behind the `bake` feature, which only a `[build-dependencies]`
/// entry turns on.
///
/// # Panics
///
/// Does not panic; it exits the process with a `cargo::error=` line, so the
/// failure appears as a build error naming the file rather than as a backtrace.
pub fn bake_dir(request: &BakeDir<'_>) {
    let mut table = format!(
        "// @generated by build.rs from {}/*.crpix — do not edit.\n\n",
        request.source_label
    );

    for stem in request.stems {
        let relative = format!("assets/{stem}.crpix");
        // **The line that makes editing art rebuild the game.** Without it the
        // baked PNG in OUT_DIR is a cached artifact of a source cargo has never
        // been told about, and a redrawn frame shows up on the next
        // `cargo clean` and not before.
        println!("cargo::rerun-if-changed={relative}");

        let source = request.manifest_dir.join(&relative);
        let text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| fail(&format!("{relative}: {error}")));
        let art =
            crate::crpix::parse(&text).unwrap_or_else(|error| fail(&format!("{relative}:{error}")));

        let image = format!("{stem}.png");
        let baked = bake(&art, &image, request.tick_hz)
            .unwrap_or_else(|error| fail(&format!("{relative}: {error}")));

        let png = request.out_dir.join(&image);
        write(&png, &baked.png);

        let vis = request.visibility.keyword();
        let upper = stem.to_uppercase();
        table.push_str(&format!(
            "{vis} static {upper}_PNG: &[u8] = include_bytes!({:?});\n",
            png.display().to_string()
        ));

        match &baked.json {
            Some(json) => {
                let sidecar = request.out_dir.join(format!("{stem}.json"));
                write(&sidecar, json.as_bytes());
                table.push_str(&format!(
                    "{vis} static {upper}_JSON: Option<&str> = Some(include_str!({:?}));\n",
                    sidecar.display().to_string()
                ));
            }
            // A single still frame is fully described by its image; the loader
            // takes `None` and reads one frame covering the whole picture.
            None => {
                table.push_str(&format!(
                    "{vis} static {upper}_JSON: Option<&str> = None;\n"
                ));
            }
        }
    }

    write(&request.out_dir.join(request.table_name), table.as_bytes());
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes)
        .unwrap_or_else(|error| fail(&format!("writing {}: {error}", path.display())));
}

/// Fails the build with a message cargo shows, rather than a panic backtrace.
fn fail(message: &str) -> ! {
    println!("cargo::error={message}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crpix;

    /// The two keywords, pinned — because the compiler will not pin them.
    ///
    /// A wrong answer here widens a crate's public API instead of failing a
    /// build, so this is the only thing standing between `Visibility::Crate`
    /// and `crcbl_render::menu::MENU_PNG` becoming part of that crate's
    /// documented surface.
    #[test]
    fn a_crate_visible_static_is_not_a_public_one() {
        assert_eq!(Visibility::Public.keyword(), "pub");
        assert_eq!(Visibility::Crate.keyword(), "pub(crate)");
        assert_ne!(
            Visibility::Crate.keyword(),
            Visibility::Public.keyword(),
            "the whole point of the parameter"
        );
    }

    const BIRD: &str = "\
crpix: 2 2 2 2 1

palette:
  . c None
  k c #112233

frame up:
  .k
  k.

frame down:
  k.
  .k

clip flap: up down @ 6 loop
nine: 1 0 1 0
";

    fn bird() -> crpix::CrpixArt {
        crpix::parse(BIRD).expect("the fixture parses")
    }

    /// The PNG is a real PNG, the right size, and carries the art's pixels —
    /// checked by decoding it again rather than by trusting the encoder.
    #[test]
    fn the_sheet_bakes_to_a_png_that_decodes_to_the_art() {
        let baked = bake(&bird(), "bird.png", 60).expect("this bakes");
        assert_eq!(&baked.png[1..4], b"PNG", "not a PNG at all");

        let decoder = png::Decoder::new(std::io::Cursor::new(&baked.png));
        let mut reader = decoder.read_info().expect("the PNG is readable");
        let mut buffer = vec![0; reader.output_buffer_size().expect("a known size")];
        let info = reader.next_frame(&mut buffer).expect("one frame");
        assert_eq!(
            (info.width, info.height),
            (4, 2),
            "two 2x2 frames in a strip"
        );
        assert_eq!(info.color_type, png::ColorType::Rgba);

        let at = |x: usize, y: usize| &buffer[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(at(0, 0), [0, 0, 0, 0], "frame `up` starts transparent");
        assert_eq!(at(1, 0), [0x11, 0x22, 0x33, 0xff]);
        assert_eq!(at(2, 0), [0x11, 0x22, 0x33, 0xff], "frame `down` is next");
        assert_eq!(at(3, 0), [0, 0, 0, 0]);
    }

    /// The same input twice gives the same bytes, so a rebuild that changed
    /// nothing does not churn a committed artifact.
    #[test]
    fn baking_is_deterministic() {
        let first = bake(&bird(), "bird.png", 60).expect("this bakes");
        let second = bake(&bird(), "bird.png", 60).expect("this bakes");
        assert_eq!(first, second);
    }

    /// The sidecar carries what Aseprite's does, in the fields Aseprite uses.
    #[test]
    fn the_sidecar_is_the_aseprite_schema() {
        let baked = bake(&bird(), "bird.png", 60).expect("this bakes");
        let json = baked.json.expect("two frames need a sidecar");

        // Frames, in order, with their rects and durations.
        assert!(json.contains("\"filename\": \"up\""), "{json}");
        assert!(
            json.contains("\"frame\": {\"x\": 2, \"y\": 0, \"w\": 2, \"h\": 2}"),
            "the second frame is at x=2: {json}"
        );
        // Six ticks at 60 Hz is 100 ms.
        assert!(json.contains("\"duration\": 100"), "{json}");

        // The clip as a frame tag: a range, a direction, and no `repeat`
        // because it loops.
        assert!(
            json.contains("\"name\": \"flap\", \"from\": 0, \"to\": 1, \"direction\": \"forward\""),
            "{json}"
        );
        assert!(
            !json.contains("\"repeat\""),
            "a looping clip repeats forever"
        );

        // The nine-slice as Aseprite's centre rect: insets 1/0/1/0 on a 2x2
        // frame leave a 1x1 centre at (1, 1).
        assert!(
            json.contains("\"center\": {\"x\": 1, \"y\": 1, \"w\": 1, \"h\": 1}"),
            "{json}"
        );

        assert!(json.contains("\"image\": \"bird.png\""));
        assert!(json.contains("\"format\": \"RGBA8888\""));
        assert!(json.contains("\"size\": {\"w\": 4, \"h\": 2}"));
    }

    /// The centre rect the sidecar writes is the one `NineSlice::from_center`
    /// reads back — the two are inverses, and a transposition would survive any
    /// test that only looked at one of them.
    #[test]
    fn the_nine_slice_survives_a_round_trip_through_the_schema() {
        let art = crpix::parse(
            "crpix: 8 6 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkkkkkk\n  kkkkkkkk\n  \
             kkkkkkkk\n  kkkkkkkk\n  kkkkkkkk\n  kkkkkkkk\n\nnine: 2 3 1 4\n",
        )
        .expect("this parses");
        let (_, sheet) = art.to_sheet();
        let json = aseprite_json(&sheet, "a.png", 60);

        // Insets 2/3/1/4 on an 8x6 frame leave a 3x1 centre at (2, 1).
        assert!(
            json.contains("\"center\": {\"x\": 2, \"y\": 1, \"w\": 3, \"h\": 1}"),
            "{json}"
        );
        let read_back = crate::NineSlice::from_center(
            crate::Rect::new(0, 0, 8, 6),
            crate::Rect::new(2, 1, 3, 1),
        )
        .expect("the centre is inside the frame");
        assert_eq!(read_back, sheet.nine.expect("the sheet has one"));
    }

    /// A still sprite is fully described by its image, so it gets no sidecar.
    #[test]
    fn a_still_sprite_bakes_to_a_png_and_nothing_else() {
        let art = crpix::parse("crpix: 1 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n")
            .expect("parses");
        let baked = bake(&art, "a.png", 60).expect("this bakes");
        assert_eq!(baked.json, None);
        assert!(!baked.png.is_empty());
    }

    /// A one-shot clip says so, because Aseprite's absent `repeat` means
    /// forever and a death animation that loops is a bug the player sees.
    #[test]
    fn a_one_shot_clip_writes_its_repeat() {
        let art = crpix::parse(
            "crpix: 1 1 2 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\nframe b:\n  k\n\n\
             clip die: a b\n",
        )
        .expect("parses");
        let json = bake(&art, "a.png", 60)
            .expect("bakes")
            .json
            .expect("a clip");
        assert!(json.contains("\"repeat\": \"1\""), "{json}");
    }

    /// Durations convert at the tick rate they are told, and never round to
    /// zero — a zero-duration frame is "skip me" to some readers.
    #[test]
    fn a_hold_in_ticks_becomes_milliseconds_at_the_rate_it_is_given() {
        assert_eq!(duration_ms(6, 60), 100);
        assert_eq!(duration_ms(1, 60), 17, "rounded up, not truncated to 16");
        assert_eq!(duration_ms(30, 30), 1000);
        assert_eq!(duration_ms(1, 240), 5);
        assert_eq!(duration_ms(1, 100_000), 1, "never zero");
        assert_eq!(duration_ms(u32::MAX, 1), u32::MAX);
    }

    /// A tick rate of zero has no answer, and is refused rather than divided by.
    #[test]
    fn a_zero_tick_rate_is_refused() {
        assert_eq!(bake(&bird(), "bird.png", 0), Err(BakeError::ZeroTickRate));
    }

    /// Names come out of a file somebody wrote, so a quote in one must not
    /// produce a sidecar that will not parse.
    #[test]
    fn names_are_escaped_into_the_json() {
        let mut out = String::new();
        push_json_string(&mut out, "a \"quoted\\name\"\twith\ncontrol\u{1}chars");
        assert_eq!(
            out,
            "\"a \\\"quoted\\\\name\\\"\\twith\\ncontrol\\u0001chars\""
        );
    }
}
