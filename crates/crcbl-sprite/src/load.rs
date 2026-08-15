//! Reading a baked sheet back: the PNG, and the Aseprite-schema sidecar.
//!
//! [`crate::bake`] writes the two files §7 of `docs/specs/crcbl/pix.md`
//! describes; nothing read them. This is the other direction, and the one a
//! game actually calls — [`load`] takes the PNG bytes and an optional sidecar
//! and hands back a [`Sheet`] plus the pixels in the exact shape
//! `crcbl_render::texture::upload_texture` wants: tightly packed RGBA8.
//!
//! # Why the JSON parser is hand-written
//!
//! For the reason [`crate::bake`]'s writer is. The schema is a dozen fixed
//! fields, this crate's default build has no dependencies at all, and that is a
//! stated design property rather than an accident — a build-side crate is not
//! the place to acquire a derive macro and a general-purpose deserialiser to
//! read sixty lines of JSON. The parser below accepts RFC 8259 (whitespace,
//! escapes and surrogate pairs included) and refuses everything else with the
//! line, column and byte offset of the refusal and what it wanted to see there.
//!
//! # What survives the round trip, and what does not
//!
//! * **Frames, clips and the nine-slice do.** Names, rects, holds, tag ranges,
//!   directions, looping, and `center` back into insets.
//! * **[`SampleMode`] does not.** Aseprite's schema has nowhere to put it, so
//!   [`crate::bake`] does not write it and this reads back
//!   [`SampleMode::default`]. A `.crpix` that says `sample: smooth` bakes to a
//!   sidecar that cannot say so; a caller that needs it must set it itself.
//!   `tests/round_trip.rs` has the test that says so out loud, and it uses a
//!   `smooth` sheet deliberately — a `pixel` one round-trips to the default by
//!   coincidence and proves nothing.
//! * **A repeat count collapses.** Aseprite's `repeat` is a count; [`Clip`] has
//!   only `looping`, so any `repeat` at all reads as "does not loop". `bake`
//!   only ever writes `"1"`, so nothing this crate produces loses anything, but
//!   a hand-exported `"repeat": "3"` plays once here.
//! * **A clip's frame list becomes contiguous.** A frame tag is a range in the
//!   schema, so `from..=to` is all there is to recover.

use crate::{Clip, Direction, Frame, NineSlice, Rect, SampleMode, Sheet, SheetError};
use core::fmt;

/// Tightly packed RGBA8 pixels, and the size they cover.
///
/// Exactly `width * height * 4` bytes, row-major, no row padding — what
/// `crcbl_render::texture::upload_texture` requires of its `pixels`, so a
/// decode hands straight to an upload with no repacking in between.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// One loaded sheet: what it is, and the image it is cut from.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub sheet: Sheet,
    pub image: Rgba8,
}

/// Why a sheet could not be loaded.
///
/// Deliberately one variant per way of being wrong rather than a single
/// "malformed sidecar": the whole value of reading a file back is being told
/// which field of which frame is the problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The PNG decoder refused the bytes.
    Png(String),
    /// The PNG decoded to a layout this loader cannot normalise to RGBA8.
    ///
    /// Defensive: the transformations [`decode_png`] asks for leave only
    /// 8-bit RGBA and 8-bit grayscale+alpha, so nothing is known to reach this.
    UnsupportedPixels { color: String, depth: u8 },
    /// The sidecar is not JSON. `line` and `column` are one-based; `column`
    /// counts bytes, so it lines up with a hex dump as well as an editor.
    Syntax {
        line: usize,
        column: usize,
        offset: usize,
        expected: String,
    },
    /// A required field is absent. `path` locates the object, e.g. `frames[2]`.
    MissingField { path: String, field: &'static str },
    /// A field is present but is the wrong kind of JSON value.
    WrongType {
        path: String,
        expected: &'static str,
        found: &'static str,
    },
    /// A number that is not a whole non-negative one that fits in a `u32`.
    NotAnInteger { path: String, found: String },
    /// A frame's `duration` is zero, which is "skip me" to some readers and
    /// has no tick count at all.
    ZeroDuration { frame: String },
    /// A frame tag names a frame past the end of `frames`.
    TagOutOfRange {
        tag: String,
        from: u32,
        to: u32,
        frames: usize,
    },
    /// A frame tag whose `to` comes before its `from`, which names no frames.
    TagReversed { tag: String, from: u32, to: u32 },
    /// A `direction` that is not `forward`, `reverse` or `pingpong`.
    UnknownDirection { tag: String, direction: String },
    /// A slice's `center` is not inside the frame it is relative to.
    CenterOutsideFrame { center: Rect, frame: Rect },
    /// `meta.size` and the PNG disagree about how big the sheet is.
    SizeMismatch { json: (u32, u32), png: (u32, u32) },
    /// A tick rate of zero, which no duration can be converted against.
    ZeroTickRate,
    /// The sidecar parsed, and describes a sheet that breaks a sheet rule.
    Sheet(SheetError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Png(message) => write!(f, "decoding the sheet PNG failed: {message}"),
            Self::UnsupportedPixels { color, depth } => {
                write!(f, "a {depth}-bit {color} image did not normalise to RGBA8")
            }
            Self::Syntax {
                line,
                column,
                offset,
                expected,
            } => write!(
                f,
                "the sidecar is not JSON: line {line}, column {column} (byte {offset}): \
                 expected {expected}"
            ),
            Self::MissingField { path, field } => {
                write!(f, "`{path}` has no `{field}`")
            }
            Self::WrongType {
                path,
                expected,
                found,
            } => write!(f, "`{path}` should be {expected}, and is {found}"),
            Self::NotAnInteger { path, found } => write!(
                f,
                "`{path}` should be a whole number a sheet can use, and is `{found}`"
            ),
            Self::ZeroDuration { frame } => write!(
                f,
                "frame `{frame}` has a duration of 0 ms, which is no time at all"
            ),
            Self::TagOutOfRange {
                tag,
                from,
                to,
                frames,
            } => write!(
                f,
                "frame tag `{tag}` runs {from}..={to}, but the sidecar has {frames} frames"
            ),
            Self::TagReversed { tag, from, to } => write!(
                f,
                "frame tag `{tag}` runs {from}..={to}, which is backwards and names no frames"
            ),
            Self::UnknownDirection { tag, direction } => write!(
                f,
                "frame tag `{tag}` has direction `{direction}`, not `forward`, `reverse` \
                 or `pingpong`"
            ),
            Self::CenterOutsideFrame { center, frame } => write!(
                f,
                "a nine-slice centre {}x{}+{}+{} does not fit inside a {}x{} frame",
                center.w, center.h, center.x, center.y, frame.w, frame.h
            ),
            Self::SizeMismatch { json, png } => write!(
                f,
                "the sidecar says the sheet is {}x{} and the PNG is {}x{}",
                json.0, json.1, png.0, png.1
            ),
            Self::ZeroTickRate => write!(
                f,
                "a tick rate of zero: frame holds are `duration * tick_hz / 1000`"
            ),
            Self::Sheet(error) => write!(f, "the sidecar describes an unusable sheet: {error}"),
        }
    }
}

impl core::error::Error for LoadError {}

impl From<SheetError> for LoadError {
    fn from(error: SheetError) -> Self {
        Self::Sheet(error)
    }
}

/// Decodes a PNG to tightly packed RGBA8, whatever it was encoded as.
///
/// 8-bit RGB, palette, grayscale and grayscale+alpha all come out as RGBA8:
/// the decoder is asked for `EXPAND | ALPHA | STRIP_16`, which turns a palette
/// into RGBA, adds an opaque alpha channel to anything without one, and widens
/// a sub-byte grayscale to 8 bits — leaving only RGBA8 and grayscale+alpha8,
/// and the second of those is widened here by writing the grey into all three
/// colour channels.
///
/// **16-bit input is stripped to 8, not refused.** `STRIP_16` keeps the high
/// byte of each sample, which is what libpng's own normalisation does. The
/// alternative — refusing — would reject a file the artist can see on screen
/// over precision this engine's RGBA8 textures cannot carry anyway.
///
/// # Errors
///
/// [`LoadError::Png`] if the bytes are not a readable PNG, the image is
/// larger than this host can address, or the IHDR declares more pixels than
/// the ceiling this function applies to the IHDR before allocating.
pub fn decode_png(bytes: &[u8]) -> Result<Rgba8, LoadError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|error| LoadError::Png(error.to_string()))?;
    // `output_buffer_size` is computed from the IHDR's width and height alone,
    // capped only at `isize::MAX`, so a hundred-byte file can claim gigabytes
    // (65536×65536 is a multi-gigabyte allocation; 2²⁰×2²⁰ aborts the process).
    // Bound the claim before allocating a single byte — the same guard
    // `crcbl-golden`'s `load_png` carries (its module doc calls this "the
    // pattern load.rs should copy").
    const MAX_PIXELS: u64 = 1 << 28;
    let (declared_width, declared_height) = reader.info().size();
    let rgba_bytes = {
        // Two u32s multiplied in u64 cannot overflow, and MAX_PIXELS * 4 is 2^30.
        let pixels = u64::from(declared_width) * u64::from(declared_height);
        if pixels > MAX_PIXELS {
            return Err(LoadError::Png(format!(
                "a {declared_width}x{declared_height} PNG declares more than {MAX_PIXELS} pixels"
            )));
        }
        usize::try_from(pixels * 4).map_err(|_| {
            LoadError::Png("the image does not fit in this host's address space".to_owned())
        })?
    };
    let capacity = reader.output_buffer_size().ok_or_else(|| {
        LoadError::Png("the image does not fit in this host's address space".to_owned())
    })?;
    if capacity > rgba_bytes {
        // More than four bytes per pixel; with `EXPAND | ALPHA | STRIP_16` this
        // cannot happen, and refusing before allocating is cheap either way.
        return Err(LoadError::Png(format!(
            "a {declared_width}x{declared_height} PNG needing {capacity} bytes is deeper than 8-bit RGBA"
        )));
    }
    let mut buffer = vec![0u8; capacity];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| LoadError::Png(error.to_string()))?;
    let (width, height) = (info.width, info.height);
    buffer.truncate(info.buffer_size());

    let pixels = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buffer,
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => {
            let mut rgba = Vec::with_capacity(buffer.len() * 2);
            for pair in buffer.chunks_exact(2) {
                rgba.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
            }
            rgba
        }
        (color, depth) => {
            return Err(LoadError::UnsupportedPixels {
                color: format!("{color:?}"),
                depth: depth as u8,
            });
        }
    };

    let expected = (u64::from(width) * u64::from(height) * 4) as usize;
    if pixels.len() != expected {
        return Err(LoadError::Png(format!(
            "a {width}x{height} RGBA8 image is {expected} bytes, and the decode produced {}",
            pixels.len()
        )));
    }
    Ok(Rgba8 {
        width,
        height,
        pixels,
    })
}

/// Aseprite's milliseconds as a hold in **ticks** — the inverse of
/// [`crate::bake::duration_ms`].
///
/// `duration_ms` is `ceil(hold * 1000 / tick_hz)`, so a written `ms` lies in
/// `[hold * 1000 / tick_hz, hold * 1000 / tick_hz + 1)` and therefore
/// `ms * tick_hz / 1000` lies in `[hold, hold + tick_hz / 1000)`. Flooring
/// that recovers `hold` exactly whenever the window is narrower than one tick,
/// which is **every `tick_hz <= 1000`** — every rate a fixed-step simulation
/// plausibly runs at, and the whole range this is tested over.
///
/// Above 1000 Hz the millisecond encoding is genuinely lossy and no reader can
/// undo it: at 2000 Hz both `hold = 1` and `hold = 2` write `duration: 1`. The
/// first value that fails is `hold = 1000` at `tick_hz = 1001`, which is the
/// boundary the tests assert rather than a range chosen to look clean.
///
/// Floored at 1 for the mirror of the reason `duration_ms` is: a hand-written
/// export can carry a duration shorter than one tick, and the nearest hold
/// this engine can express for it is one tick, not none — and zero would fail
/// [`Sheet::validate`] anyway.
#[must_use]
pub fn hold_ticks(ms: u32, tick_hz: u32) -> u32 {
    let ticks = u64::from(ms) * u64::from(tick_hz) / 1000;
    u32::try_from(ticks).unwrap_or(u32::MAX).max(1)
}

/// Reads an Aseprite-schema sidecar back into a [`Sheet`].
///
/// The inverse of [`crate::bake::aseprite_json`], and of an
/// `--format json-array` export from Aseprite itself: `frames[]` become
/// [`Frame`]s, `meta.frameTags` become [`Clip`]s, `meta.slices[0].keys[0]
/// .center` becomes the [`NineSlice`], and `meta.size` is the sheet's size.
/// `tick_hz` is the rate the durations are converted against — see
/// [`hold_ticks`].
///
/// Fields the engine has no use for — `rotated`, `trimmed`,
/// `spriteSourceSize`, `sourceSize`, `meta.app`, `meta.layers` — are ignored
/// rather than refused, so an export carrying more than this needs still
/// loads.
///
/// # Errors
///
/// [`LoadError`] naming the first thing wrong: a syntax error with its line
/// and byte offset, a missing or mistyped field with its path, or the sheet
/// rule the result breaks.
pub fn read_aseprite_json(json: &str, tick_hz: u32) -> Result<Sheet, LoadError> {
    if tick_hz == 0 {
        return Err(LoadError::ZeroTickRate);
    }
    let root = Parser::new(json).parse_document()?;
    let root = object(&root, "the sidecar")?;

    let frames = read_frames(field_required(root, "the sidecar", "frames")?, tick_hz)?;
    let meta = object(field_required(root, "the sidecar", "meta")?, "meta")?;
    let (width, height) = read_size(field_required(meta, "meta", "size")?, "meta.size")?;
    let clips = match field(meta, "frameTags") {
        Some(tags) => read_tags(tags, frames.len())?,
        None => Vec::new(),
    };
    let nine = match field(meta, "slices") {
        Some(slices) => read_nine(slices, &frames)?,
        None => None,
    };

    let sheet = Sheet {
        width,
        height,
        frames,
        clips,
        nine,
        // The schema has nowhere to put it; see this module's header.
        sample: SampleMode::default(),
    };
    sheet.validate()?;
    Ok(sheet)
}

/// Loads a baked sheet: the PNG, and the sidecar when there is one.
///
/// The one call a game makes. `sidecar` is `None` for a still sprite, because
/// §7 of the spec does not write one — a single frame is fully described by
/// its image — so the sheet is synthesised from the image's own size: one
/// frame named `default`, covering the whole picture, held for one tick. The
/// frame's original name is not recoverable, because nothing wrote it down.
///
/// # Errors
///
/// [`LoadError`] from either half, plus [`LoadError::SizeMismatch`] when the
/// sidecar's `meta.size` disagrees with the image it is beside — which means
/// the two files came from different bakes, and every frame rect in the
/// sidecar is suspect.
pub fn load(png: &[u8], sidecar: Option<&str>, tick_hz: u32) -> Result<Loaded, LoadError> {
    let image = decode_png(png)?;
    let sheet = match sidecar {
        Some(json) => {
            let sheet = read_aseprite_json(json, tick_hz)?;
            if (sheet.width, sheet.height) != (image.width, image.height) {
                return Err(LoadError::SizeMismatch {
                    json: (sheet.width, sheet.height),
                    png: (image.width, image.height),
                });
            }
            sheet
        }
        None => {
            let sheet = Sheet {
                width: image.width,
                height: image.height,
                frames: vec![Frame {
                    name: "default".to_owned(),
                    rect: Rect::new(0, 0, image.width, image.height),
                    hold: 1,
                }],
                clips: Vec::new(),
                nine: None,
                sample: SampleMode::default(),
            };
            sheet.validate()?;
            sheet
        }
    };
    Ok(Loaded { sheet, image })
}

/// [`load`] for a sheet the build baked in, which cannot fail at run time.
///
/// The bytes come from `include_bytes!` on a `build.rs` product, so a failure
/// here is a broken build rather than bad input: the baker wrote a PNG this
/// crate's own decoder cannot read, or a sidecar that disagrees with it. There
/// is no run-time recovery to offer and no caller who could take one, so this
/// panics with `name` in the message — a `Result` at every call site would be
/// ceremony around an `unwrap` that can only mean the same thing.
///
/// `#[track_caller]` so the panic names the line that asked for the sheet
/// rather than this function.
///
/// Use [`load`] directly for a sheet that arrives at run time, where a failure
/// is data the caller has to handle.
///
/// # Panics
///
/// When [`load`] returns a [`LoadError`].
#[track_caller]
pub fn load_baked(name: &str, png: &[u8], sidecar: Option<&str>, tick_hz: u32) -> Loaded {
    load(png, sidecar, tick_hz)
        .unwrap_or_else(|error| panic!("the baked {name} sheet did not load: {error}"))
}

// ---------------------------------------------------------------------------
// The schema, field by field
// ---------------------------------------------------------------------------

fn read_frames(value: &Json, tick_hz: u32) -> Result<Vec<Frame>, LoadError> {
    let entries = array(value, "frames")?;
    let mut frames = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let path = format!("frames[{index}]");
        let fields = object(entry, &path)?;
        let name = string(
            field_required(fields, &path, "filename")?,
            &format!("{path}.filename"),
        )?
        .to_owned();
        let rect = read_rect(
            field_required(fields, &path, "frame")?,
            &format!("{path}.frame"),
        )?;
        let ms = integer(
            field_required(fields, &path, "duration")?,
            &format!("{path}.duration"),
        )?;
        if ms == 0 {
            return Err(LoadError::ZeroDuration { frame: name });
        }
        frames.push(Frame {
            name,
            rect,
            hold: hold_ticks(ms, tick_hz),
        });
    }
    Ok(frames)
}

fn read_tags(value: &Json, frames: usize) -> Result<Vec<Clip>, LoadError> {
    let entries = array(value, "meta.frameTags")?;
    let mut clips = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let path = format!("meta.frameTags[{index}]");
        let fields = object(entry, &path)?;
        let name = string(
            field_required(fields, &path, "name")?,
            &format!("{path}.name"),
        )?
        .to_owned();
        let from = integer(
            field_required(fields, &path, "from")?,
            &format!("{path}.from"),
        )?;
        let to = integer(field_required(fields, &path, "to")?, &format!("{path}.to"))?;
        if to < from {
            return Err(LoadError::TagReversed {
                tag: name,
                from,
                to,
            });
        }
        if usize::try_from(to).is_err() || to as usize >= frames {
            return Err(LoadError::TagOutOfRange {
                tag: name,
                from,
                to,
                frames,
            });
        }
        let direction = match string(
            field_required(fields, &path, "direction")?,
            &format!("{path}.direction"),
        )? {
            "forward" => Direction::Forward,
            "reverse" => Direction::Reverse,
            "pingpong" => Direction::PingPong,
            other => {
                return Err(LoadError::UnknownDirection {
                    tag: name,
                    direction: other.to_owned(),
                });
            }
        };
        // Aseprite spells "forever" as no `repeat` field at all; any count it
        // does write is a clip that stops. Type-checked even though the value
        // is discarded, so a malformed export is refused rather than read as
        // "loops".
        let looping = match field(fields, "repeat") {
            Some(repeat) => {
                string(repeat, &format!("{path}.repeat"))?;
                false
            }
            None => true,
        };
        clips.push(Clip {
            name,
            frames: (from as usize..=to as usize).collect(),
            direction,
            looping,
        });
    }
    Ok(clips)
}

fn read_nine(value: &Json, frames: &[Frame]) -> Result<Option<NineSlice>, LoadError> {
    let slices = array(value, "meta.slices")?;
    let Some(slice) = slices.first() else {
        return Ok(None);
    };
    let fields = object(slice, "meta.slices[0]")?;
    let keys = array(
        field_required(fields, "meta.slices[0]", "keys")?,
        "meta.slices[0].keys",
    )?;
    let Some(key) = keys.first() else {
        return Ok(None);
    };
    let key = object(key, "meta.slices[0].keys[0]")?;
    let Some(center) = field(key, "center") else {
        // A slice without a centre is an ordinary Aseprite slice — a named
        // region — and not a nine-slice at all.
        return Ok(None);
    };
    let center = read_rect(center, "meta.slices[0].keys[0].center")?;
    // Relative to the slice's own bounds, which for a baked sheet is frame 0.
    let frame = frames.first().map_or(Rect::default(), |frame| frame.rect);
    NineSlice::from_center(frame, center)
        .map(Some)
        .ok_or(LoadError::CenterOutsideFrame { center, frame })
}

fn read_rect(value: &Json, path: &str) -> Result<Rect, LoadError> {
    let fields = object(value, path)?;
    Ok(Rect {
        x: integer(field_required(fields, path, "x")?, &format!("{path}.x"))?,
        y: integer(field_required(fields, path, "y")?, &format!("{path}.y"))?,
        w: integer(field_required(fields, path, "w")?, &format!("{path}.w"))?,
        h: integer(field_required(fields, path, "h")?, &format!("{path}.h"))?,
    })
}

fn read_size(value: &Json, path: &str) -> Result<(u32, u32), LoadError> {
    let fields = object(value, path)?;
    Ok((
        integer(field_required(fields, path, "w")?, &format!("{path}.w"))?,
        integer(field_required(fields, path, "h")?, &format!("{path}.h"))?,
    ))
}

fn object<'j>(value: &'j Json, path: &str) -> Result<&'j [(String, Json)], LoadError> {
    match value {
        Json::Object(fields) => Ok(fields),
        other => Err(LoadError::WrongType {
            path: path.to_owned(),
            expected: "an object",
            found: other.kind(),
        }),
    }
}

fn array<'j>(value: &'j Json, path: &str) -> Result<&'j [Json], LoadError> {
    match value {
        Json::Array(items) => Ok(items),
        other => Err(LoadError::WrongType {
            path: path.to_owned(),
            expected: "an array",
            found: other.kind(),
        }),
    }
}

fn string<'j>(value: &'j Json, path: &str) -> Result<&'j str, LoadError> {
    match value {
        Json::Str(text) => Ok(text),
        other => Err(LoadError::WrongType {
            path: path.to_owned(),
            expected: "a string",
            found: other.kind(),
        }),
    }
}

fn integer(value: &Json, path: &str) -> Result<u32, LoadError> {
    match value {
        Json::Number(text) => text.parse::<u32>().map_err(|_| LoadError::NotAnInteger {
            path: path.to_owned(),
            found: text.clone(),
        }),
        other => Err(LoadError::WrongType {
            path: path.to_owned(),
            expected: "a number",
            found: other.kind(),
        }),
    }
}

fn field<'j>(fields: &'j [(String, Json)], name: &str) -> Option<&'j Json> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn field_required<'j>(
    fields: &'j [(String, Json)],
    path: &str,
    name: &'static str,
) -> Result<&'j Json, LoadError> {
    field(fields, name).ok_or_else(|| LoadError::MissingField {
        path: path.to_owned(),
        field: name,
    })
}

// ---------------------------------------------------------------------------
// A JSON parser, because the alternative is a dependency
// ---------------------------------------------------------------------------

/// A parsed JSON value.
///
/// Numbers are kept as the text that spelled them rather than as an `f64`:
/// every number in this schema is a whole pixel count or a millisecond, and
/// routing those through a binary float to get them back out as integers is a
/// rounding question nobody should have to think about.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "a boolean",
            Self::Number(_) => "a number",
            Self::Str(_) => "a string",
            Self::Array(_) => "an array",
            Self::Object(_) => "an object",
        }
    }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            src: text.as_bytes(),
            pos: 0,
        }
    }

    fn error(&self, at: usize, expected: impl Into<String>) -> LoadError {
        let (line, column) = line_column(self.src, at);
        LoadError::Syntax {
            line,
            column,
            offset: at,
            expected: expected.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// The bytes `start..end` as text. Cannot fail: the parser only ever cuts
    /// at ASCII bytes, and the input arrived as a `&str`.
    fn text(&self, start: usize, end: usize) -> Result<&'a str, LoadError> {
        core::str::from_utf8(&self.src[start..end])
            .map_err(|_| self.error(start, "valid UTF-8 text"))
    }

    fn parse_document(mut self) -> Result<Json, LoadError> {
        self.skip_whitespace();
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos != self.src.len() {
            return Err(self.error(self.pos, "the end of the file after the top-level value"));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<Json, LoadError> {
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Json::Str),
            Some(b't') => self.parse_keyword("true", Json::Bool(true)),
            Some(b'f') => self.parse_keyword("false", Json::Bool(false)),
            Some(b'n') => self.parse_keyword("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(self.error(self.pos, "a JSON value")),
        }
    }

    fn parse_keyword(&mut self, word: &str, value: Json) -> Result<Json, LoadError> {
        if self.src[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error(self.pos, format!("`{word}`")))
        }
    }

    fn parse_object(&mut self) -> Result<Json, LoadError> {
        let open = self.pos;
        self.pos += 1; // `{`
        let mut fields = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error(self.pos, "`:` after a member name"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.parse_value()?;
            fields.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(fields));
                }
                Some(_) => return Err(self.error(self.pos, "`,` or `}`")),
                None => return Err(self.error(open, "a `}` closing this object")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Json, LoadError> {
        let open = self.pos;
        self.pos += 1; // `[`
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                Some(_) => return Err(self.error(self.pos, "`,` or `]`")),
                None => return Err(self.error(open, "a `]` closing this array")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, LoadError> {
        let open = self.pos;
        if self.peek() != Some(b'"') {
            return Err(self.error(self.pos, "a string"));
        }
        self.pos += 1;
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error(open, "a `\"` closing this string"));
            };
            match byte {
                b'"' => {
                    out.push_str(self.text(run, self.pos)?);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(self.text(run, self.pos)?);
                    self.pos += 1;
                    let ch = self.parse_escape()?;
                    out.push(ch);
                    run = self.pos;
                }
                0x00..=0x1f => {
                    return Err(self.error(
                        self.pos,
                        "an escape: a raw control character cannot appear in a JSON string",
                    ));
                }
                _ => self.pos += 1,
            }
        }
    }

    fn parse_escape(&mut self) -> Result<char, LoadError> {
        let at = self.pos;
        let Some(byte) = self.peek() else {
            return Err(self.error(at, "an escape character after `\\`"));
        };
        self.pos += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let first = self.parse_hex4()?;
                if (0xd800..0xdc00).contains(&first) {
                    // A high surrogate is only half a character; the other half
                    // must follow, or the string does not name a scalar value.
                    if self.peek() != Some(b'\\') || self.src.get(self.pos + 1) != Some(&b'u') {
                        return Err(self.error(at, "a `\\u` low surrogate after a high surrogate"));
                    }
                    self.pos += 2;
                    let low = self.parse_hex4()?;
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(self.error(
                            at,
                            "a low surrogate in `\\udc00`..`\\udfff` after a high one",
                        ));
                    }
                    let code =
                        0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(low) - 0xdc00);
                    char::from_u32(code).ok_or_else(|| self.error(at, "a Unicode scalar value"))?
                } else {
                    char::from_u32(u32::from(first)).ok_or_else(|| {
                        self.error(at, "a Unicode scalar value, not a lone low surrogate")
                    })?
                }
            }
            _ => return Err(self.error(at, "one of `\"`, `\\`, `/`, `b`, `f`, `n`, `r`, `t`, `u`")),
        })
    }

    fn parse_hex4(&mut self) -> Result<u16, LoadError> {
        let at = self.pos;
        let mut value: u16 = 0;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err(self.error(at, "four hex digits after `\\u`"));
            };
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a') + 10,
                b'A'..=b'F' => u16::from(byte - b'A') + 10,
                _ => return Err(self.error(at, "four hex digits after `\\u`")),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Json, LoadError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.error(self.pos, "a digit")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error(self.pos, "a digit after the decimal point"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error(self.pos, "a digit in the exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text = self.text(start, self.pos)?;
        Ok(Json::Number(text.to_owned()))
    }
}

/// One-based line and byte column of `offset`, for an error a human reads.
fn line_column(src: &[u8], offset: usize) -> (usize, usize) {
    let upto = &src[..offset.min(src.len())];
    let line = 1 + upto.iter().filter(|&&byte| byte == b'\n').count();
    let start = upto
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |index| index + 1);
    (line, upto.len() - start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sidecar with everything in it: two frames, a looping clip and a
    /// nine-slice, written the way `bake` writes one.
    const SIDECAR: &str = r##"{
 "frames": [
  {"filename": "up", "frame": {"x": 0, "y": 0, "w": 2, "h": 3}, "rotated": false, "trimmed": false, "spriteSourceSize": {"x": 0, "y": 0, "w": 2, "h": 3}, "sourceSize": {"w": 2, "h": 3}, "duration": 100},
  {"filename": "down", "frame": {"x": 2, "y": 0, "w": 2, "h": 3}, "rotated": false, "trimmed": false, "spriteSourceSize": {"x": 0, "y": 0, "w": 2, "h": 3}, "sourceSize": {"w": 2, "h": 3}, "duration": 100}
 ],
 "meta": {
  "app": "https://github.com/kryptic-sh/crcbl",
  "version": "crcbl-sprite bake",
  "image": "bird.png",
  "format": "RGBA8888",
  "size": {"w": 4, "h": 3},
  "scale": "1",
  "frameTags": [
   {"name": "flap", "from": 0, "to": 1, "direction": "forward"}
  ],
  "slices": [
   {"name": "nine", "color": "#0000ffff", "keys": [{"frame": 0, "bounds": {"x": 0, "y": 0, "w": 2, "h": 3}, "center": {"x": 1, "y": 1, "w": 1, "h": 1}}]}
  ]
 }
}
"##;

    fn sheet() -> Sheet {
        read_aseprite_json(SIDECAR, 60).expect("this sidecar is well formed")
    }

    /// Every field the schema carries comes back, with counts asserted so a
    /// reader that gave up and returned nothing could not pass.
    #[test]
    fn a_sidecar_reads_back_into_the_sheet_it_describes() {
        let sheet = sheet();
        assert_eq!((sheet.width, sheet.height), (4, 3));
        assert_eq!(sheet.frames.len(), 2);
        assert_eq!(sheet.frames[0].name, "up");
        // Deliberately not square: a reader that transposed `w` and `h` would
        // survive any fixture whose frames happened to be.
        assert_eq!(sheet.frames[0].rect, Rect::new(0, 0, 2, 3));
        assert_eq!(sheet.frames[1].name, "down");
        assert_eq!(sheet.frames[1].rect, Rect::new(2, 0, 2, 3));
        // 100 ms at 60 Hz is six ticks.
        assert_eq!(sheet.frames[0].hold, 6);
        assert_eq!(sheet.frames[1].hold, 6);

        assert_eq!(sheet.clips.len(), 1);
        let clip = &sheet.clips[0];
        assert_eq!(clip.name, "flap");
        assert_eq!(clip.frames, vec![0, 1]);
        assert_eq!(clip.direction, Direction::Forward);
        assert!(clip.looping, "no `repeat` field is Aseprite's forever");

        // A 1x1 centre at (1, 1) of a 2x3 frame leaves 1 on the left, 0 on
        // the right, 1 on the top and 1 on the bottom.
        assert_eq!(sheet.nine, Some(NineSlice::new(1, 0, 1, 1)));
    }

    /// The rate is what the durations are read against, so the same file at a
    /// different rate is a different number of ticks — and that is the point.
    #[test]
    fn durations_are_read_at_the_tick_rate_they_are_given() {
        assert_eq!(read_aseprite_json(SIDECAR, 30).unwrap().frames[0].hold, 3);
        assert_eq!(read_aseprite_json(SIDECAR, 120).unwrap().frames[0].hold, 12);
        assert_eq!(read_aseprite_json(SIDECAR, 0), Err(LoadError::ZeroTickRate));
    }

    /// A duration shorter than a tick is one tick, not none: zero would fail
    /// [`Sheet::validate`], and "as short as this engine can hold it" is the
    /// only honest answer.
    #[test]
    fn a_hold_is_never_rounded_down_to_nothing() {
        assert_eq!(hold_ticks(1, 30), 1, "1 ms at 30 Hz is under a tick");
        assert_eq!(hold_ticks(17, 60), 1);
        assert_eq!(hold_ticks(100, 60), 6);
        assert_eq!(hold_ticks(1000, 30), 30);
        assert_eq!(hold_ticks(u32::MAX, u32::MAX), u32::MAX, "and saturates");
    }

    /// Aseprite's absent `repeat` is forever; any count at all is a one-shot,
    /// because a [`Clip`] has nowhere to put a number of repeats.
    #[test]
    fn a_repeat_field_turns_a_clip_into_a_one_shot() {
        let json = SIDECAR.replace(
            r#""direction": "forward""#,
            r#""direction": "forward", "repeat": "1""#,
        );
        let sheet = read_aseprite_json(&json, 60).expect("this parses");
        assert_eq!(sheet.clips.len(), 1);
        assert!(!sheet.clips[0].looping);
    }

    #[test]
    fn every_direction_the_schema_spells_is_understood() {
        for (text, expected) in [
            ("forward", Direction::Forward),
            ("reverse", Direction::Reverse),
            ("pingpong", Direction::PingPong),
        ] {
            let json = SIDECAR.replace(
                r#""direction": "forward""#,
                &format!(r#""direction": "{text}""#),
            );
            let sheet = read_aseprite_json(&json, 60).expect("this parses");
            assert_eq!(sheet.clips.len(), 1);
            assert_eq!(sheet.clips[0].direction, expected);
        }
    }

    // The sample mode not surviving the schema is asserted in
    // `tests/round_trip.rs`, not here: a check against this fixture could only
    // compare the default to the default, which is a green light wired to
    // nothing. Baking a `sample: smooth` sheet and watching it come back
    // `Pixel` is the version that can fail.

    // -- the ways a sidecar can be wrong, each with its own error ------------

    /// A file that stops in the middle is refused, and says what was left
    /// open. Four cuts, because each unterminated construct runs out its own
    /// way and a reader can get one right and the rest wrong.
    #[test]
    fn a_truncated_sidecar_names_what_was_left_unclosed() {
        let cases: [(&str, u8, &str); 4] = [
            ("", 0, "a JSON value"),
            (r#"{"frames": [1"#, b'[', "]"),
            (r#"{"frames": [{"filename": "up""#, b'{', "}"),
            (r#"{"frames": [{"filename": "up"#, b'"', "\""),
        ];
        for (json, opener, wanted) in cases {
            let error = match read_aseprite_json(json, 60) {
                Err(error) => error,
                Ok(sheet) => panic!("`{json}` is truncated, and parsed to {sheet:?}"),
            };
            let LoadError::Syntax {
                line,
                column,
                offset,
                expected,
            } = error
            else {
                panic!("`{json}` is a syntax error, and gave {error:?}");
            };
            assert!(
                expected.contains(wanted),
                "`{json}`: wanted {wanted} in the message, got `{expected}`"
            );
            assert_eq!((line, column), (1, offset + 1), "all on one line");
            if opener != 0 {
                assert_eq!(
                    json.as_bytes()[offset],
                    opener,
                    "`{json}`: the offset should point at the unclosed `{}`, and points at byte \
                     {offset}",
                    opener as char
                );
            }
        }

        // And the real thing, cut in half, is refused rather than half read.
        let half = &SIDECAR[..SIDECAR.len() / 2];
        let error = read_aseprite_json(half, 60).expect_err("this is cut in half");
        let LoadError::Syntax { line, .. } = error else {
            panic!("a truncated sidecar is a syntax error, got {error:?}");
        };
        assert!(line > 1, "the cut is past the first line, and says so");
    }

    #[test]
    fn a_frame_missing_a_field_names_the_frame_and_the_field() {
        let json = SIDECAR.replace(r#""filename": "down", "#, "");
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::MissingField {
                path: "frames[1]".to_owned(),
                field: "filename",
            })
        );

        let json = SIDECAR.replace(
            r#", "duration": 100}
 ]"#,
            "}\n ]",
        );
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::MissingField {
                path: "frames[1]".to_owned(),
                field: "duration",
            })
        );
    }

    #[test]
    fn a_tag_that_runs_past_the_frames_is_refused() {
        let json = SIDECAR.replace(r#""from": 0, "to": 1"#, r#""from": 7, "to": 9"#);
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::TagOutOfRange {
                tag: "flap".to_owned(),
                from: 7,
                to: 9,
                frames: 2,
            })
        );

        let json = SIDECAR.replace(r#""from": 0, "to": 1"#, r#""from": 1, "to": 0"#);
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::TagReversed {
                tag: "flap".to_owned(),
                from: 1,
                to: 0,
            })
        );
    }

    #[test]
    fn a_zero_duration_is_refused_rather_than_clamped() {
        let json = SIDECAR.replacen(r#""duration": 100"#, r#""duration": 0"#, 1);
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::ZeroDuration {
                frame: "up".to_owned()
            })
        );
    }

    #[test]
    fn a_centre_that_does_not_fit_its_frame_is_refused() {
        let json = SIDECAR.replace(
            r#""center": {"x": 1, "y": 1, "w": 1, "h": 1}"#,
            r#""center": {"x": 1, "y": 1, "w": 4, "h": 1}"#,
        );
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::CenterOutsideFrame {
                center: Rect::new(1, 1, 4, 1),
                frame: Rect::new(0, 0, 2, 3),
            })
        );
    }

    #[test]
    fn a_field_of_the_wrong_kind_says_which_and_what_it_wanted() {
        let json = SIDECAR.replace(r#""filename": "up""#, r#""filename": 7"#);
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::WrongType {
                path: "frames[0].filename".to_owned(),
                expected: "a string",
                found: "a number",
            })
        );

        let json = SIDECAR
            .replace(r#""frames": ["#, r#""frames": {"0": ["#)
            .replace(
                r#" ],
 "meta""#,
                r#" ]},
 "meta""#,
            );
        assert!(matches!(
            read_aseprite_json(&json, 60),
            Err(LoadError::WrongType {
                expected: "an array",
                ..
            })
        ));
    }

    #[test]
    fn a_number_that_is_not_a_pixel_count_is_refused() {
        let json = SIDECAR.replace(r#""w": 4, "h": 3"#, r#""w": 4.5, "h": 3"#);
        assert_eq!(
            read_aseprite_json(&json, 60),
            Err(LoadError::NotAnInteger {
                path: "meta.size.w".to_owned(),
                found: "4.5".to_owned(),
            })
        );
    }

    #[test]
    fn a_sheet_the_schema_allows_but_a_sheet_rule_forbids_is_refused() {
        // A frame rect that runs off the right of the image: legal JSON, and
        // the thing `Sheet::validate` exists to catch.
        let json = SIDECAR.replace(
            r#""frame": {"x": 2, "y": 0, "w": 2, "h": 3}"#,
            r#""frame": {"x": 3, "y": 0, "w": 2, "h": 3}"#,
        );
        assert!(matches!(
            read_aseprite_json(&json, 60),
            Err(LoadError::Sheet(SheetError::FrameOutsideSheet { .. }))
        ));
    }

    #[test]
    fn the_escapes_the_baker_can_emit_are_all_understood() {
        let json = SIDECAR.replace(
            r#""filename": "up""#,
            r#""filename": "a \"quoted\\name\"\twith\ncontrol\u0001chars\u00e9\ud83d\ude00""#,
        );
        let sheet = read_aseprite_json(&json, 60).expect("this parses");
        assert_eq!(sheet.frames.len(), 2);
        assert_eq!(
            sheet.frames[0].name,
            "a \"quoted\\name\"\twith\ncontrol\u{1}chars\u{e9}\u{1f600}"
        );
    }

    #[test]
    fn arbitrary_whitespace_between_tokens_is_accepted() {
        let json = "  {\n\t\"frames\"\r\n:\n[ {  \"filename\" : \"a\" , \"frame\" : \
                    { \"x\":0 , \"y\":0 , \"w\":1 , \"h\":1 } , \"duration\" : 100 } ] , \
                    \"meta\" : { \"size\" : { \"w\" : 1 , \"h\" : 1 } } }  \n";
        let sheet = read_aseprite_json(json, 60).expect("whitespace is not an error");
        assert_eq!(sheet.frames.len(), 1);
        assert_eq!(sheet.frames[0].rect, Rect::new(0, 0, 1, 1));
    }

    // -- the image ----------------------------------------------------------

    /// The same 2x2 picture written five ways must decode to one answer.
    #[test]
    fn every_png_colour_type_normalises_to_the_same_rgba8() {
        const GREYS: [u8; 4] = [0x00, 0x40, 0x80, 0xff];
        let expected: Vec<u8> = GREYS.iter().flat_map(|&g| [g, g, g, 0xff]).collect();

        let rgba: Vec<u8> = expected.clone();
        let rgb: Vec<u8> = GREYS.iter().flat_map(|&g| [g, g, g]).collect();
        let gray: Vec<u8> = GREYS.to_vec();
        let gray_alpha: Vec<u8> = GREYS.iter().flat_map(|&g| [g, 0xff]).collect();
        let palette: Vec<u8> = GREYS.iter().flat_map(|&g| [g, g, g]).collect();
        let indices: Vec<u8> = vec![0, 1, 2, 3];

        /// A name, the colour type to write, the samples, and a palette.
        type Case<'a> = (&'a str, png::ColorType, &'a [u8], Option<&'a [u8]>);

        let cases: [Case<'_>; 5] = [
            ("rgba", png::ColorType::Rgba, &rgba, None),
            ("rgb", png::ColorType::Rgb, &rgb, None),
            ("grayscale", png::ColorType::Grayscale, &gray, None),
            (
                "grayscale+alpha",
                png::ColorType::GrayscaleAlpha,
                &gray_alpha,
                None,
            ),
            ("palette", png::ColorType::Indexed, &indices, Some(&palette)),
        ];

        for (name, color, data, plte) in cases {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
                encoder.set_color(color);
                encoder.set_depth(png::BitDepth::Eight);
                if let Some(plte) = plte {
                    encoder.set_palette(plte.to_vec());
                }
                let mut writer = encoder.write_header().expect("a header");
                writer.write_image_data(data).expect("the rows");
                writer.finish().expect("the end");
            }
            let image = decode_png(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((image.width, image.height), (2, 2), "{name}");
            assert_eq!(image.pixels, expected, "{name} did not normalise");
        }
    }

    /// The grayscale widening has to carry the alpha through, and the case
    /// above cannot see that: every pixel in it is opaque, so a widening that
    /// dropped the alpha and wrote `0xff` would pass it. This one varies both.
    #[test]
    fn a_grayscale_alpha_image_keeps_its_alpha_and_greys_all_three_channels() {
        let samples: [u8; 8] = [0x00, 0xff, 0x40, 0x00, 0x80, 0x33, 0xff, 0x7f];
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::GrayscaleAlpha);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("a header");
            writer.write_image_data(&samples).expect("the rows");
            writer.finish().expect("the end");
        }
        let image = decode_png(&bytes).expect("grayscale+alpha decodes");
        assert_eq!(
            image.pixels,
            vec![
                0x00, 0x00, 0x00, 0xff, //
                0x40, 0x40, 0x40, 0x00, //
                0x80, 0x80, 0x80, 0x33, //
                0xff, 0xff, 0xff, 0x7f,
            ]
        );
    }

    /// 16-bit samples are stripped to their high byte rather than refused.
    #[test]
    fn sixteen_bit_input_is_stripped_to_eight() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Sixteen);
            let mut writer = encoder.write_header().expect("a header");
            // 0x1234, 0x5678, 0x9abc big-endian.
            writer
                .write_image_data(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc])
                .expect("the row");
            writer.finish().expect("the end");
        }
        let image = decode_png(&bytes).expect("16-bit is stripped, not refused");
        assert_eq!(image.pixels, vec![0x12, 0x56, 0x9a, 0xff]);
    }

    #[test]
    fn bytes_that_are_not_a_png_are_refused() {
        let error = decode_png(b"not a png at all").expect_err("this is not a PNG");
        assert!(matches!(error, LoadError::Png(_)), "{error:?}");
    }

    // -- the whole call -----------------------------------------------------

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("a header");
            writer
                .write_image_data(&vec![0x7f; (width * height * 4) as usize])
                .expect("the rows");
            writer.finish().expect("the end");
        }
        bytes
    }

    /// CRC-32, the PNG chunk CRC, in the bitwise reflected form.
    ///
    /// Pinned by the standard check value below so a transcription slip cannot
    /// silently produce a CRC the decoder rejects.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = 0u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    /// `png_bytes`'s output with the IHDR's declared size rewritten and the
    /// chunk's CRC fixed up, so the decoder believes the hostile claim.
    fn png_with_declared_size(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = png_bytes(1, 1);
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        bytes
    }

    /// A hostile IHDR must be refused before `vec![0u8; …]` is sized from it —
    /// `output_buffer_size` trusts the file's own claim, and 65536×65536 is a
    /// multi-gigabyte allocation (2²⁰×2²⁰ aborts the process).
    #[test]
    fn a_png_that_declares_a_huge_size_is_refused_before_allocating() {
        // Pins the CRC implementation: the standard check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let hostile = png_with_declared_size(65_536, 65_536);
        let error = decode_png(&hostile).expect_err("a multi-gigabyte claim must be refused");
        assert!(
            matches!(&error, LoadError::Png(message) if message.contains("declares more than")),
            "the refusal must name the pixel-count guard: {error:?}"
        );
        // The same file at a sane size still decodes — the refusal is the claim,
        // not the file's shape.
        let fine = decode_png(&png_bytes(6, 3)).expect("a sane PNG still decodes");
        assert_eq!((fine.width, fine.height), (6, 3));
    }

    /// §7 writes no sidecar for a still sprite, so the loader has to invent the
    /// sheet from the image: one frame, the whole picture.
    #[test]
    fn a_sheet_with_no_sidecar_is_one_frame_covering_the_image() {
        let loaded = load(&png_bytes(6, 3), None, 60).expect("a still sprite loads");
        assert_eq!(loaded.sheet.frames.len(), 1);
        assert_eq!(loaded.sheet.frames[0].rect, Rect::new(0, 0, 6, 3));
        assert_eq!(loaded.sheet.frames[0].hold, 1);
        assert_eq!(loaded.sheet.frames[0].name, "default");
        assert!(loaded.sheet.clips.is_empty());
        assert_eq!(loaded.sheet.nine, None);
        assert_eq!((loaded.image.width, loaded.image.height), (6, 3));
        assert_eq!(loaded.image.pixels.len(), 6 * 3 * 4);
    }

    /// A sidecar beside the wrong PNG means every rect in it is suspect, and
    /// the sizes are the one place that shows.
    #[test]
    fn a_sidecar_that_does_not_match_its_png_is_refused() {
        assert_eq!(
            load(&png_bytes(8, 8), Some(SIDECAR), 60),
            Err(LoadError::SizeMismatch {
                json: (4, 3),
                png: (8, 8),
            })
        );
        let loaded = load(&png_bytes(4, 3), Some(SIDECAR), 60).expect("these agree");
        assert_eq!(loaded.sheet.frames.len(), 2);
    }
}
