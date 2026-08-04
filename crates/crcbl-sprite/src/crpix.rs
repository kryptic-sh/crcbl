//! `.crpix` — pixel art as text, with XPM's design and frames of its own.
//!
//! A build input. [`bake`](crate::bake) turns one of these into a PNG and,
//! when there is anything to say beyond a single still image, an
//! Aseprite-schema JSON sidecar. The engine loads only those two, so nothing
//! downstream knows this format exists and art exported from Aseprite is
//! indistinguishable by the time it reaches [`Sheet`].
//!
//! # What was taken from XPM, and why
//!
//! XPM (X11's X PixMap) is the open standard for exactly this — a palette
//! mapping characters to colours, then rows of those characters — and its
//! design was adopted rather than improved on:
//!
//! * **The header declares every count.** `crpix: 8 5 2 4 1` is width, height,
//!   frames, colours and characters-per-pixel, mirroring XPM's
//!   `"8 5 4 1"`. This looked at first like redundancy that would rot, and it is
//!   the opposite: it is a checksum. A row one character short is only
//!   detectable *at all* because the width was declared, and it is what lets
//!   this parser reject a frame where two rows are wrong in compensating
//!   directions — which comparing rows against each other cannot see. Real XPM
//!   readers enforce it row by row; so does this one.
//! * **`c None` is transparent**, spelled exactly as XPM spells it.
//! * **Multiple characters per pixel**, so a palette can exceed the ninety-odd
//!   printable characters. `chars: 2` reads rows in two-character cells.
//! * **The `<key> c <colour>` entry form**, including XPM's colour *contexts*,
//!   so a palette line lifted out of an `.xpm` pastes in unchanged.
//!
//! What was deliberately not taken: XPM's `m`, `g` and `s` contexts
//! (monochrome, greyscale and symbolic) are parsed and ignored rather than
//! rejected, because they exist to serve displays this engine will never meet
//! and refusing them would break the paste-an-XPM-palette property for nothing.
//! X11 colour *names* are not accepted — the table is unbounded, hex is
//! unambiguous, and a misspelled name is a colour that silently is not the one
//! you meant.
//!
//! # What XPM does not have, and this does
//!
//! XPM holds one image. A sprite sheet is many, and no open text format covers
//! frames, animation clips or nine-slice — Aseprite's JSON does, and that is a
//! binary tool's export rather than something written by hand. So:
//!
//! ```text
//! crpix: 8 5 2 4 1              # w h frames colours chars-per-pixel
//!
//! palette:
//!   . c None
//!   k c #241c1c
//!   y c #f2c14e
//!   w c #ffffff
//!
//! frame up:
//!   ..kkkk..
//!   .kyyyyk.
//!   kyywwyyk
//!   kyyyyyyk
//!   ..kkkk..
//!
//! frame down:
//!   ...
//!
//! clip flap: up down @ 6 loop    # six ticks a frame
//! nine: 2 2 1 1                  # left right top bottom, in pixels
//! sample: pixel                  # or `smooth`
//! ```
//!
//! # The parser is hostile on purpose
//!
//! Every one of these is a build failure naming a line, not a sprite that
//! renders subtly wrong: a row that is not exactly `width × chars` characters,
//! a frame with the wrong number of rows, a count in the header that disagrees
//! with what follows, a character with no palette entry, a clip naming a frame
//! that does not exist, insets that overlap.
//!
//! The failure being avoided is specific: art whose rows are one character
//! short renders as a sheared sprite, and a sheared sprite is blamed on the
//! renderer for an afternoon before anybody counts the characters.

use crate::{Clip, Direction, Frame, NineSlice, Rect, SampleMode, Sheet};
use core::fmt;

/// One frame's pixels, before they are laid into a sheet image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameArt {
    pub name: String,
    /// Row-major RGBA, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    pub hold: u32,
}

/// The counts a `.crpix` header declares.
///
/// Every one of them is checked against what the file actually contains. See
/// the module docs for why a format that could infer them declares them anyway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub colours: u32,
    /// Characters per pixel, as XPM's fourth field.
    pub chars: u32,
}

/// A parsed `.crpix` file: the art, and the sheet description over it.
#[derive(Clone, Debug, PartialEq)]
pub struct CrpixArt {
    pub header: Header,
    pub frames: Vec<FrameArt>,
    pub clips: Vec<Clip>,
    pub nine: Option<NineSlice>,
    pub sample: SampleMode,
}

/// The RGBA byte size of `frames` frames of `width`×`height` art, or `None`
/// when the strip cannot exist in memory on this platform.
///
/// The header is five `u32`s, so a large-but-parseable file can describe a
/// sheet whose allocation size the u32 arithmetic would wrap; checking here
/// is what turns that from a truncated buffer into a named error.
fn sheet_bytes(width: u32, height: u32, frames: u64) -> Option<usize> {
    let per_frame = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|area| area.checked_mul(4))?;
    let total = per_frame.checked_mul(frames)?;
    usize::try_from(total).ok()
}

impl CrpixArt {
    /// Lays the frames left to right into one image and returns the pixels
    /// alongside the [`Sheet`] that describes them.
    ///
    /// A horizontal strip, which is Aseprite's own default and keeps the
    /// arithmetic trivial: frame `i` starts at `x = i * width`. A packed atlas
    /// would save texture memory that a sheet of eight 16×16 frames does not
    /// have a problem with, at the cost of a bin-packer between the art and the
    /// screen.
    ///
    /// Returns `(rgba, sheet)`, `rgba` being row-major over the whole strip.
    ///
    /// # Panics
    ///
    /// If the art describes a strip that cannot exist in memory — possible
    /// only for a [`CrpixArt`] built by hand, since [`parse`] refuses one.
    #[must_use]
    pub fn to_sheet(&self) -> (Vec<u8>, Sheet) {
        let (fw, fh) = (self.header.width, self.header.height);
        let frame_count = self.frames.len() as u64;
        // Checked arithmetic throughout: `parse` bounds the header so a parsed
        // art always fits, and a `CrpixArt` built by hand past that check gets
        // a loud panic here instead of the truncated allocation the old u32
        // math produced.
        let bytes = u64::from(fw)
            .checked_mul(u64::from(fh))
            .and_then(|area| area.checked_mul(4))
            .and_then(|per_frame| per_frame.checked_mul(frame_count))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .expect("a parsed .crpix is bounded at parse time");
        let sheet_w = u64::from(fw)
            .checked_mul(frame_count)
            .and_then(|w| u32::try_from(w).ok())
            .expect("a parsed .crpix is bounded at parse time");
        let mut rgba = vec![0u8; bytes];

        for (index, frame) in self.frames.iter().enumerate() {
            let x0 = u64::from(index as u32) * u64::from(fw);
            for y in 0..fh {
                let src = usize::try_from(u64::from(y) * u64::from(fw) * 4)
                    .expect("a parsed .crpix is bounded at parse time");
                let dst = usize::try_from((u64::from(y) * u64::from(sheet_w) + x0) * 4)
                    .expect("a parsed .crpix is bounded at parse time");
                let run = usize::try_from(u64::from(fw) * 4)
                    .expect("a parsed .crpix is bounded at parse time");
                rgba[dst..dst + run].copy_from_slice(&frame.pixels[src..src + run]);
            }
        }

        let sheet = Sheet {
            width: sheet_w,
            height: fh,
            frames: self
                .frames
                .iter()
                .enumerate()
                .map(|(index, frame)| Frame {
                    name: frame.name.clone(),
                    rect: Rect::new(index as u32 * fw, 0, fw, fh),
                    hold: frame.hold,
                })
                .collect(),
            clips: self.clips.clone(),
            nine: self.nine,
            sample: self.sample,
        };
        (rgba, sheet)
    }

    /// Whether this art needs a JSON sidecar at all.
    ///
    /// A single still frame with no clips and no nine-slice is fully described
    /// by its PNG, and emitting a sidecar that says only "one frame, the whole
    /// image" is a file for a reader to fetch, parse and learn nothing from.
    #[must_use]
    pub fn needs_metadata(&self) -> bool {
        self.frames.len() > 1 || !self.clips.is_empty() || self.nine.is_some()
    }
}

/// Where a parse failed, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrpixError {
    /// One-based, so it matches what an editor shows.
    pub line: usize,
    pub kind: CrpixErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrpixErrorKind {
    /// The file does not open with a `crpix:` header.
    MissingHeader,
    /// A header that is not five positive numbers.
    BadHeader(String),
    /// A second `crpix:` line.
    RepeatedHeader,
    /// A line that is not a section, a directive or a row.
    Unrecognised(String),
    /// A palette entry that is not `<key> [context] <colour>`.
    BadPaletteEntry(String),
    /// A colour that is not `None`, a hex form, or a known XPM colour name.
    BadColour(String),
    /// A palette key not followed by a space.
    ///
    /// The key is the first `chars` characters of the line, so its *length* is
    /// never wrong — what goes wrong is writing a one-character key under a
    /// `chars: 2` header, which silently takes the following space as part of
    /// the key and leaves every row unmatched. Requiring a separator catches it
    /// where it was written.
    KeyNotSeparated { key: String, chars: u32 },
    /// Two palette entries for the same key.
    DuplicateKey(String),
    /// A cell with no palette entry.
    UnknownPixel(String),
    /// A row whose character count is not `width * chars`.
    RowWrongWidth { expected: usize, found: usize },
    /// A frame with the wrong number of rows.
    FrameWrongHeight { expected: u32, found: u32 },
    /// The header's count and the file's contents disagree.
    CountMismatch {
        what: &'static str,
        declared: u32,
        found: u32,
    },
    /// A clip naming a frame that was never defined.
    UnknownFrame { clip: String, frame: String },
    /// A clip whose frames are not a contiguous ascending run.
    ClipNotContiguous { clip: String },
    /// A `@ N` that is not a positive number of ticks.
    BadHold(String),
    /// A `nine:` that is not four numbers.
    BadNine(String),
    /// A `sample:` that is not `pixel` or `smooth`.
    BadSample(String),
    /// A sheet too large for this platform: `width × height × frames` RGBA
    /// pixels cannot be addressed, or the strip's width cannot fit the
    /// format's `u32`.
    ///
    /// The header is five `u32`s, so a large-but-parseable file can describe
    /// a sheet the allocation arithmetic would wrap — refused where it was
    /// declared rather than silently truncated.
    TooLarge {
        width: u32,
        height: u32,
        frames: u32,
    },
    /// The sheet the file describes broke one of [`Sheet`]'s own rules.
    Sheet(crate::SheetError),
}

impl fmt::Display for CrpixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: ", self.line)?;
        match &self.kind {
            CrpixErrorKind::MissingHeader => write!(
                f,
                "a .crpix opens with `crpix: <width> <height> <frames> <colours> <chars>`"
            ),
            CrpixErrorKind::BadHeader(text) => write!(
                f,
                "`crpix: {text}` is not five positive numbers — width, height, frames, \
                 colours, characters-per-pixel"
            ),
            CrpixErrorKind::RepeatedHeader => write!(f, "the header is already set"),
            CrpixErrorKind::Unrecognised(line) => write!(
                f,
                "expected a palette entry, a section or a row of art, got `{line}`"
            ),
            CrpixErrorKind::BadPaletteEntry(line) => write!(
                f,
                "a palette entry is `<key> c <colour>`, as in XPM; got `{line}`"
            ),
            CrpixErrorKind::BadColour(text) => write!(
                f,
                "`{text}` is not a colour; write `None`, a hex form (`#rgb`, `#rrggbb`, \
                 `#rrggbbaa`, `#rrrrggggbbbb`) or one of the XPM colour names"
            ),
            CrpixErrorKind::KeyNotSeparated { key, chars } => write!(
                f,
                "the header says {chars} characters per pixel, so the key here is `{key}` \
                 — which is not followed by a space. Check the key's width"
            ),
            CrpixErrorKind::DuplicateKey(key) => write!(f, "`{key}` already has a colour"),
            CrpixErrorKind::UnknownPixel(cell) => write!(
                f,
                "`{cell}` is not in the palette; every cell in a row must be"
            ),
            CrpixErrorKind::RowWrongWidth { expected, found } => write!(
                f,
                "this row is {found} characters and the header declares {expected}; \
                 a row of the wrong length renders as a sheared sprite"
            ),
            CrpixErrorKind::FrameWrongHeight { expected, found } => write!(
                f,
                "this frame has {found} rows and the header declares {expected}"
            ),
            CrpixErrorKind::CountMismatch {
                what,
                declared,
                found,
            } => write!(
                f,
                "the header declares {declared} {what} and the file has {found}"
            ),
            CrpixErrorKind::UnknownFrame { clip, frame } => write!(
                f,
                "clip `{clip}` names frame `{frame}`, which is not defined"
            ),
            CrpixErrorKind::ClipNotContiguous { clip } => write!(
                f,
                "clip `{clip}` names frames that are not a contiguous run in sheet order. \
                 An Aseprite frame tag is a range, so list the frames in the order they \
                 appear and use `reverse` or `pingpong` to change how they play"
            ),
            CrpixErrorKind::BadHold(text) => write!(
                f,
                "`@ {text}` is not a positive number of ticks to hold each frame"
            ),
            CrpixErrorKind::BadNine(text) => write!(
                f,
                "`nine:` takes four pixel insets — left right top bottom — got `{text}`"
            ),
            CrpixErrorKind::BadSample(text) => {
                write!(f, "`sample:` is `pixel` or `smooth`, got `{text}`")
            }
            CrpixErrorKind::TooLarge {
                width,
                height,
                frames,
            } => write!(
                f,
                "a {width}×{height} strip of {frames} frame(s) is too large for this platform"
            ),
            CrpixErrorKind::Sheet(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for CrpixError {}

/// Parses a `.crpix` document.
///
/// # Errors
///
/// [`CrpixError`] naming the line and the rule broken. Parsing stops at the
/// first one: a build failure needs the first cause, not the noise downstream
/// of it.
pub fn parse(source: &str) -> Result<CrpixArt, CrpixError> {
    Parser::new(source).run()
}

/// A palette entry's colour, as straight (non-premultiplied) RGBA.
type Rgba = [u8; 4];

struct Parser<'a> {
    source: &'a str,
    header: Option<Header>,
    palette: Vec<(String, Rgba)>,
    frames: Vec<FrameArt>,
    clips: Vec<(String, Vec<String>, u32, Direction, bool)>,
    nine: Option<NineSlice>,
    sample: SampleMode,
    /// The frame being read: its name, its rows so far, and the line it opened
    /// on, so a frame of the wrong height is reported where it was written.
    open: Option<(String, Vec<Vec<Rgba>>, usize)>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            header: None,
            palette: Vec::new(),
            frames: Vec::new(),
            clips: Vec::new(),
            nine: None,
            sample: SampleMode::Pixel,
            open: None,
        }
    }

    fn run(mut self) -> Result<CrpixArt, CrpixError> {
        let mut in_palette = false;
        let mut last = 0;

        for (index, raw) in self.source.lines().enumerate() {
            let line = index + 1;
            last = line;
            let text = strip_comment(raw);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("crpix:") {
                if self.header.is_some() {
                    return Err(CrpixError {
                        line,
                        kind: CrpixErrorKind::RepeatedHeader,
                    });
                }
                self.header = Some(header(rest, line)?);
                continue;
            }
            let Some(header) = self.header else {
                return Err(CrpixError {
                    line,
                    kind: CrpixErrorKind::MissingHeader,
                });
            };

            if let Some(rest) = trimmed.strip_prefix("palette:") {
                self.close_frame(line)?;
                if !rest.trim().is_empty() {
                    return Err(CrpixError {
                        line,
                        kind: CrpixErrorKind::Unrecognised(trimmed.to_string()),
                    });
                }
                in_palette = true;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("frame ") {
                self.close_frame(line)?;
                in_palette = false;
                let name = rest
                    .trim()
                    .strip_suffix(':')
                    .ok_or_else(|| CrpixError {
                        line,
                        kind: CrpixErrorKind::Unrecognised(trimmed.to_string()),
                    })?
                    .trim()
                    .to_string();
                self.open = Some((name, Vec::new(), line));
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("clip ") {
                self.close_frame(line)?;
                in_palette = false;
                self.clip(rest, line)?;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("nine:") {
                self.close_frame(line)?;
                in_palette = false;
                self.nine = Some(nine(rest, line)?);
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("sample:") {
                self.close_frame(line)?;
                in_palette = false;
                self.sample = match rest.trim() {
                    "pixel" => SampleMode::Pixel,
                    "smooth" => SampleMode::Smooth,
                    other => {
                        return Err(CrpixError {
                            line,
                            kind: CrpixErrorKind::BadSample(other.to_string()),
                        });
                    }
                };
                continue;
            }

            if in_palette {
                // The raw line, not the comment-stripped one: a colour opens
                // with `#`, and the two `#`s in `k c #123456 # note` are told
                // apart by position rather than by guesswork.
                self.palette_entry(raw, line)?;
            } else if self.open.is_some() {
                self.row(trimmed, header, line)?;
            } else {
                return Err(CrpixError {
                    line,
                    kind: CrpixErrorKind::Unrecognised(trimmed.to_string()),
                });
            }
        }

        self.close_frame(last.max(1))?;
        self.finish(last.max(1))
    }

    /// A palette entry is `<key> [context] <colour>`, XPM's own form.
    ///
    /// The key is the header's characters-per-pixel wide and is taken by
    /// character count rather than by splitting on whitespace, because with
    /// `chars: 2` a key may legitimately contain a space. Contexts other than
    /// `c` are skipped rather than refused, so an XPM palette line pastes in.
    fn palette_entry(&mut self, raw: &str, line: usize) -> Result<(), CrpixError> {
        let header = self.header.expect("checked by the caller");
        let chars = header.chars as usize;

        let indent: usize = raw.len() - raw.trim_start().len();
        let body = &raw[indent..];
        if body.chars().count() < chars + 1 {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::BadPaletteEntry(body.trim_end().to_string()),
            });
        }
        let split = body
            .char_indices()
            .nth(chars)
            .map_or(body.len(), |(at, _)| at);
        let (key, rest) = body.split_at(split);

        // XPM writes contexts as `c <colour>`, and may carry several. Take the
        // colour after the first `c`, or — for a file that omits the context
        // entirely — the first token.
        let mut tokens = rest.split_whitespace();
        let colour = match tokens.next() {
            Some("c") => tokens.next(),
            Some("m" | "g" | "s") => {
                // A context this engine does not serve: skip its value and look
                // for a `c` after it.
                let mut skip = tokens;
                loop {
                    match skip.next() {
                        Some("c") => break skip.next(),
                        Some(_) => {}
                        None => break None,
                    }
                }
            }
            other => other,
        };
        let Some(colour) = colour else {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::BadPaletteEntry(body.trim_end().to_string()),
            });
        };

        if !rest.starts_with(char::is_whitespace) {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::KeyNotSeparated {
                    key: key.to_string(),
                    chars: header.chars,
                },
            });
        }
        if self.palette.iter().any(|(existing, _)| existing == key) {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::DuplicateKey(key.to_string()),
            });
        }
        self.palette
            .push((key.to_string(), parse_colour(colour, line)?));
        Ok(())
    }

    fn row(&mut self, text: &str, header: Header, line: usize) -> Result<(), CrpixError> {
        let cells: Vec<char> = text.chars().collect();
        let expected = (header.width * header.chars) as usize;
        if cells.len() != expected {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::RowWrongWidth {
                    expected,
                    found: cells.len(),
                },
            });
        }

        let mut row = Vec::with_capacity(header.width as usize);
        for cell in cells.chunks(header.chars as usize) {
            let key: String = cell.iter().collect();
            let colour = self
                .palette
                .iter()
                .find(|(existing, _)| *existing == key)
                .map(|(_, colour)| *colour);
            match colour {
                Some(colour) => row.push(colour),
                None => {
                    return Err(CrpixError {
                        line,
                        kind: CrpixErrorKind::UnknownPixel(key),
                    });
                }
            }
        }

        let (_, rows, _) = self.open.as_mut().expect("checked by the caller");
        rows.push(row);
        // Complete the moment it has the rows the header declared. Without
        // this, a stray line after a frame is read as a row and reported as a
        // very wide one, which sends the reader looking at their art instead of
        // at the line they mistyped.
        if rows.len() as u32 == header.height {
            self.close_frame(line)?;
        }
        Ok(())
    }

    fn clip(&mut self, text: &str, line: usize) -> Result<(), CrpixError> {
        let (name, rest) = text.split_once(':').ok_or_else(|| CrpixError {
            line,
            kind: CrpixErrorKind::Unrecognised(format!("clip {text}")),
        })?;
        let name = name.trim().to_string();

        let mut hold = 1;
        let mut looping = false;
        let mut direction = Direction::Forward;
        let mut frames = Vec::new();

        let mut tokens = rest.split_whitespace();
        while let Some(token) = tokens.next() {
            match token {
                "@" => {
                    let value = tokens.next().unwrap_or_default();
                    hold = value
                        .parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| CrpixError {
                            line,
                            kind: CrpixErrorKind::BadHold(value.to_string()),
                        })?;
                }
                "loop" => looping = true,
                "reverse" => direction = Direction::Reverse,
                "pingpong" => direction = Direction::PingPong,
                frame => frames.push(frame.to_string()),
            }
        }
        self.clips.push((name, frames, hold, direction, looping));
        Ok(())
    }

    /// Turns the open frame's rows into a [`FrameArt`], against the header's
    /// declared height.
    fn close_frame(&mut self, line: usize) -> Result<(), CrpixError> {
        let Some((name, rows, opened)) = self.open.take() else {
            return Ok(());
        };
        let header = self.header.expect("a frame cannot open before the header");
        if rows.len() as u32 != header.height {
            return Err(CrpixError {
                line: if rows.is_empty() { opened } else { line },
                kind: CrpixErrorKind::FrameWrongHeight {
                    expected: header.height,
                    found: rows.len() as u32,
                },
            });
        }

        let mut pixels = Vec::with_capacity(sheet_bytes(header.width, header.height, 1).ok_or(
            CrpixError {
                line: opened,
                kind: CrpixErrorKind::TooLarge {
                    width: header.width,
                    height: header.height,
                    frames: 1,
                },
            },
        )?);
        for row in &rows {
            for colour in row {
                pixels.extend_from_slice(colour);
            }
        }
        self.frames.push(FrameArt {
            name,
            pixels,
            // Replaced by whichever clip names it. A frame nothing names is
            // still held for a tick, so a still sprite is a legal sheet.
            hold: 1,
        });
        Ok(())
    }

    fn finish(mut self, line: usize) -> Result<CrpixArt, CrpixError> {
        let Some(header) = self.header else {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::MissingHeader,
            });
        };

        // The header's counts against what the file actually holds. This is the
        // whole reason they are declared.
        for (what, declared, found) in [
            ("frames", header.frames, self.frames.len() as u32),
            ("colours", header.colours, self.palette.len() as u32),
        ] {
            if declared != found {
                return Err(CrpixError {
                    line,
                    kind: CrpixErrorKind::CountMismatch {
                        what,
                        declared,
                        found,
                    },
                });
            }
        }

        // Resolve clips by name, and let each clip set the hold of the frames it
        // names. Aseprite holds per frame; a hand-written sheet wants one number
        // for a whole animation. Where the same frame is in two clips at
        // different rates the last one wins, which is worth knowing and is why
        // it is written down rather than silently averaged.
        let mut clips = Vec::with_capacity(self.clips.len());
        for (name, frame_names, hold, direction, looping) in self.clips {
            let mut indices = Vec::with_capacity(frame_names.len());
            for frame in frame_names {
                let index = self
                    .frames
                    .iter()
                    .position(|candidate| candidate.name == frame)
                    .ok_or_else(|| CrpixError {
                        line,
                        kind: CrpixErrorKind::UnknownFrame {
                            clip: name.clone(),
                            frame: frame.clone(),
                        },
                    })?;
                self.frames[index].hold = hold;
                indices.push(index);
            }
            // Contiguous and ascending, because that is what an Aseprite frame
            // tag is — a `from`/`to` range over the sheet. Playback order is the
            // direction's job, not the list's, so this costs nothing except
            // writing `a b reverse` where one might have written `b a`.
            if indices.windows(2).any(|pair| pair[1] != pair[0] + 1) {
                return Err(CrpixError {
                    line,
                    kind: CrpixErrorKind::ClipNotContiguous { clip: name },
                });
            }
            clips.push(Clip {
                name,
                frames: indices,
                direction,
                looping,
            });
        }

        // The strip's frame count, captured before `self.frames` is moved into
        // the art below: the size check that guards `to_sheet` needs it.
        let frame_count = self.frames.len() as u64;

        let art = CrpixArt {
            header,
            frames: self.frames,
            clips,
            nine: self.nine,
            sample: self.sample,
        };

        // Refuse a strip that cannot exist in memory before building it: the
        // sheet's width is a `u32` and its pixels a `Vec`, and the header is
        // five `u32`s, so a large-but-parseable file would otherwise wrap the
        // allocation arithmetic and produce a truncated sheet.
        let fits = sheet_bytes(header.width, header.height, frame_count).is_some()
            && u64::from(header.width)
                .checked_mul(frame_count)
                .and_then(|w| u32::try_from(w).ok())
                .is_some();
        if !fits {
            return Err(CrpixError {
                line,
                kind: CrpixErrorKind::TooLarge {
                    width: header.width,
                    height: header.height,
                    frames: u32::try_from(frame_count).unwrap_or(u32::MAX),
                },
            });
        }

        // The sheet's own rules, so a `.crpix` that parses cannot still produce
        // a sheet its consumer would reject.
        let (_, sheet) = art.to_sheet();
        sheet.validate().map_err(|error| CrpixError {
            line,
            kind: CrpixErrorKind::Sheet(error),
        })?;
        Ok(art)
    }
}

/// Everything before the first `#`.
///
/// Unambiguous for two reasons that fit together. Palette entries are read from
/// the raw line, so the one place a `#` appears legitimately mid-line — the
/// colour in `k c #123456` — never reaches here. And `#` therefore cannot be a
/// palette key, because a line starting with one is stripped to nothing before
/// it could be read as an entry, so no row can contain one either.
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn header(text: &str, line: usize) -> Result<Header, CrpixError> {
    let bad = || CrpixError {
        line,
        kind: CrpixErrorKind::BadHeader(text.trim().to_string()),
    };
    let values: Vec<u32> = text
        .split_whitespace()
        .map(|token| token.parse::<u32>().unwrap_or(0))
        .collect();
    let [width, height, frames, colours, chars] = values[..] else {
        return Err(bad());
    };
    if [width, height, frames, colours, chars].contains(&0) {
        return Err(bad());
    }
    Ok(Header {
        width,
        height,
        frames,
        colours,
        chars,
    })
}

fn parse_colour(text: &str, line: usize) -> Result<Rgba, CrpixError> {
    // `None` is XPM's spelling and the one an `.xpm` palette line will carry;
    // `transparent` is accepted because it is what anyone would try first.
    if text.eq_ignore_ascii_case("none") || text.eq_ignore_ascii_case("transparent") {
        // Zero alpha *and* zero colour. A transparent texel with some leftover
        // RGB shows as a halo when the sampler blends across it, which is the
        // classic "why is there a dark fringe round my sprite".
        return Ok([0, 0, 0, 0]);
    }
    let bad = || CrpixError {
        line,
        kind: CrpixErrorKind::BadColour(text.to_string()),
    };
    let Some(hex) = text.strip_prefix('#') else {
        // Not hex, so it is either an XPM colour name or a mistake. The table
        // is the one `magick -list color` marks XPM-compliant, so a palette
        // block pasted out of a real `.xpm` resolves.
        return crate::colours::lookup(text).ok_or_else(bad);
    };
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let byte = |from: usize, to: usize| u8::from_str_radix(&hex[from..to], 16).unwrap_or(0);
    // `0xf` becomes `0xff`, as CSS expands short hex — not `0xf0`, which would
    // make `#fff` a slightly grey white.
    let nibble = |at: usize| u8::from_str_radix(&hex[at..=at], 16).unwrap_or(0) * 17;
    Ok(match hex.len() {
        3 => [nibble(0), nibble(1), nibble(2), 255],
        6 => [byte(0, 2), byte(2, 4), byte(4, 6), 255],
        8 => [byte(0, 2), byte(2, 4), byte(4, 6), byte(6, 8)],
        // XPM's 16-bits-per-channel form, which is what ImageMagick writes.
        // Truncated to the high byte rather than rounded: `#ffffffffffff` must
        // land on `0xff` exactly, and rounding the low byte in would carry.
        12 => [byte(0, 2), byte(4, 6), byte(8, 10), 255],
        _ => return Err(bad()),
    })
}

fn nine(text: &str, line: usize) -> Result<NineSlice, CrpixError> {
    let values: Vec<u32> = text
        .split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect();
    let [left, right, top, bottom] = values[..] else {
        return Err(CrpixError {
            line,
            kind: CrpixErrorKind::BadNine(text.trim().to_string()),
        });
    };
    Ok(NineSlice::new(left, right, top, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIRD: &str = "\
# a two-frame bird
crpix: 5 3 2 3 1

palette:
  . c None
  k c #000000
  y c #f2c14e

frame up:
  .kkk.
  kyyyk
  .kkk.

frame down:
  .kkk.
  kyyyk
  .....

clip flap: up down @ 6 loop
";

    fn parsed(source: &str) -> CrpixArt {
        match parse(source) {
            Ok(art) => art,
            Err(error) => panic!("expected this to parse: {error}"),
        }
    }

    fn rejected(source: &str) -> CrpixError {
        parse(source).expect_err("expected this to be refused")
    }

    #[test]
    fn a_sheet_is_read_with_its_header_describing_it() {
        let art = parsed(BIRD);
        assert_eq!(
            art.header,
            Header {
                width: 5,
                height: 3,
                frames: 2,
                colours: 3,
                chars: 1
            }
        );
        assert_eq!(art.frames.len(), 2);
        assert_eq!(art.frames[0].name, "up");
        assert_eq!(art.frames[0].pixels.len(), 5 * 3 * 4);

        let at = |x: usize, y: usize| {
            let i = (y * 5 + x) * 4;
            &art.frames[0].pixels[i..i + 4]
        };
        assert_eq!(at(2, 1), [0xf2, 0xc1, 0x4e, 0xff]);
        assert_eq!(at(0, 0), [0, 0, 0, 0]);
        assert_eq!(at(1, 0), [0, 0, 0, 0xff]);
    }

    #[test]
    fn clips_resolve_to_frame_indices_and_set_the_hold() {
        let art = parsed(BIRD);
        assert_eq!(art.clips.len(), 1);
        assert_eq!(art.clips[0].frames, vec![0, 1]);
        assert!(art.clips[0].looping);
        assert_eq!(art.frames[0].hold, 6);
        assert_eq!(art.frames[1].hold, 6);
    }

    #[test]
    fn frames_are_laid_out_left_to_right_into_one_strip() {
        let art = parsed(BIRD);
        let (rgba, sheet) = art.to_sheet();
        assert_eq!((sheet.width, sheet.height), (10, 3));
        assert_eq!(rgba.len(), 10 * 3 * 4);
        assert_eq!(sheet.frames[1].rect, Rect::new(5, 0, 5, 3));
        sheet.validate().expect("a parsed sheet is always valid");

        let at = |x: usize, y: usize| {
            let i = (y * 10 + x) * 4;
            &rgba[i..i + 4]
        };
        assert_eq!(at(1, 2), [0, 0, 0, 0xff], "frame `up` has a bottom edge");
        assert_eq!(at(6, 2), [0, 0, 0, 0], "frame `down` does not");
    }

    /// The checked arithmetic behind the parse-time size bounds: a header's
    /// `width × height × frames × 4` never wraps, so a large-but-parseable
    /// header becomes an error instead of a truncated allocation.
    ///
    /// A real 32768×32768 fixture is four gigabytes of text, so the numbers
    /// are exercised directly: the review's canonical case is reported
    /// exactly where the platform can address it and refused where it cannot,
    /// and everything past that comes back `None` rather than a wrapped small
    /// size — at every step of the multiplication.
    #[test]
    fn sheet_bytes_is_exact_for_large_but_parseable_headers() {
        // 32768×32768×4 is exactly 2³², which the old u32 math wrapped to 0.
        let exact = u64::from(32768u32) * 32768 * 4;
        assert_eq!(
            sheet_bytes(32768, 32768, 1),
            usize::try_from(exact).ok(),
            "a 32768×32768 frame must be reported exactly, never wrapped",
        );
        assert_eq!(sheet_bytes(5, 3, 2), Some(5 * 3 * 2 * 4));
        // Products no platform can address: `None`, not a truncated size.
        assert_eq!(sheet_bytes(u32::MAX, u32::MAX, 1), None);
        assert_eq!(sheet_bytes(u32::MAX, u32::MAX, u64::from(u32::MAX)), None);
        assert_eq!(sheet_bytes(u32::MAX, 1, u64::from(u32::MAX)), None);
    }

    /// **The rule the declared header buys.** A row one character short renders
    /// as a sheared sprite, and it is only detectable at all because the width
    /// was stated.
    #[test]
    fn a_row_of_the_wrong_width_is_refused_against_the_header() {
        let error = rejected(
            "crpix: 4 3 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkk\n  kkk\n  kkkk\n",
        );
        assert_eq!(error.line, 8);
        assert_eq!(
            error.kind,
            CrpixErrorKind::RowWrongWidth {
                expected: 4,
                found: 3
            }
        );
        assert!(error.to_string().contains("sheared"), "{error}");
    }

    /// The case comparing rows against each other cannot see: two rows wrong in
    /// compensating directions. XPM readers catch this because they check every
    /// row against the header, and so does this.
    #[test]
    fn two_rows_wrong_in_opposite_directions_are_still_refused() {
        let error = rejected(
            "crpix: 4 3 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkkk\n  kkk\n  kkkk\n",
        );
        assert_eq!(error.line, 7, "the first bad row, not the pair");
        assert!(matches!(
            error.kind,
            CrpixErrorKind::RowWrongWidth {
                expected: 4,
                found: 5
            }
        ));
    }

    #[test]
    fn a_frame_with_the_wrong_number_of_rows_is_refused() {
        let error = rejected("crpix: 2 3 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kk\n  kk\n");
        assert_eq!(
            error.kind,
            CrpixErrorKind::FrameWrongHeight {
                expected: 3,
                found: 2
            }
        );
    }

    /// The header's counts are checked against the file, which is what makes
    /// them a checksum rather than decoration.
    #[test]
    fn the_declared_counts_must_match_what_the_file_holds() {
        let three_frames = rejected("crpix: 1 1 3 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n");
        assert_eq!(
            three_frames.kind,
            CrpixErrorKind::CountMismatch {
                what: "frames",
                declared: 3,
                found: 1
            }
        );

        let two_colours = rejected("crpix: 1 1 1 2 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n");
        assert_eq!(
            two_colours.kind,
            CrpixErrorKind::CountMismatch {
                what: "colours",
                declared: 2,
                found: 1
            }
        );
    }

    /// XPM's multi-character keys, so a palette can exceed the printable
    /// characters — and so a two-character cell is read as one pixel.
    #[test]
    fn two_characters_can_make_one_pixel() {
        let art = parsed(
            "crpix: 3 2 1 2 2\n\npalette:\n  .. c None\n  KK c #112233\n\n\
             frame a:\n  ..KK..\n  KK..KK\n",
        );
        assert_eq!(art.header.chars, 2);
        let px = |i: usize| &art.frames[0].pixels[i * 4..i * 4 + 4];
        assert_eq!(px(0), [0, 0, 0, 0]);
        assert_eq!(px(1), [0x11, 0x22, 0x33, 0xff]);
        assert_eq!(px(3), [0x11, 0x22, 0x33, 0xff]);

        // A one-character key under a two-character header would silently
        // swallow the space after it and leave every row unmatched. Caught
        // where it was written, not where it failed to match.
        let error = rejected("crpix: 1 1 1 1 2\n\npalette:\n  K c #000\n\nframe a:\n  KK\n");
        assert!(
            matches!(error.kind, CrpixErrorKind::KeyNotSeparated { .. }),
            "{error}"
        );
        assert_eq!(error.line, 4);
    }

    /// A palette line lifted straight out of an `.xpm` pastes in: `c None`, and
    /// the contexts this engine does not serve are skipped rather than refused.
    #[test]
    fn an_xpm_palette_line_pastes_in_unchanged() {
        let art = parsed(
            "crpix: 2 1 1 2 1\n\npalette:\n  . c None\n  k m white c #241C1C\n\n\
             frame a:\n  .k\n",
        );
        let px = |i: usize| &art.frames[0].pixels[i * 4..i * 4 + 4];
        assert_eq!(px(0), [0, 0, 0, 0]);
        assert_eq!(px(1), [0x24, 0x1c, 0x1c, 0xff]);
    }

    /// XPM colour names resolve, so a palette block pasted out of a real
    /// `.xpm` — where names are ordinary — works without being rewritten.
    #[test]
    fn xpm_colour_names_resolve() {
        let art = parsed(
            "crpix: 3 1 1 3 1\n\npalette:\n  o c white\n  b c CornflowerBlue\n  \
             g c gold\n\nframe a:\n  obg\n",
        );
        let px = |i: usize| &art.frames[0].pixels[i * 4..i * 4 + 4];
        assert_eq!(px(0), [255, 255, 255, 255]);
        assert_eq!(px(1), [100, 149, 237, 255]);
        assert_eq!(px(2), [255, 215, 0, 255]);
    }

    /// A name that is not one is refused rather than guessed at, and the
    /// message lists what a colour may be.
    #[test]
    fn a_name_that_is_not_a_colour_is_refused() {
        let error =
            rejected("crpix: 1 1 1 1 1\n\npalette:\n  k c cornflowerbloo\n\nframe a:\n  k\n");
        assert!(
            matches!(error.kind, CrpixErrorKind::BadColour(_)),
            "{error}"
        );
        assert!(error.to_string().contains("#rrggbb"), "{error}");
    }

    /// XPM's 16-bit-per-channel colours, which is what ImageMagick writes.
    ///
    /// The channels below differ in their low byte from their high one, so a
    /// reader taking the wrong half gets a visibly different colour — a fixture
    /// of `#f2f2c1c14e4e` would pass either way and prove nothing.
    #[test]
    fn twelve_digit_hex_is_truncated_to_its_high_bytes() {
        let art = parsed(
            "crpix: 2 1 1 2 1\n\npalette:\n  a c #f200c1114e22\n  b c #ffffffffffff\n\n\
             frame f:\n  ab\n",
        );
        let px = |i: usize| &art.frames[0].pixels[i * 4..i * 4 + 4];
        assert_eq!(px(0), [0xf2, 0xc1, 0x4e, 255]);
        assert_eq!(
            px(1),
            [255, 255, 255, 255],
            "white must land exactly on white, which rounding would carry past"
        );
    }

    #[test]
    fn a_pixel_with_no_palette_entry_is_refused() {
        let error = rejected("crpix: 3 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kzk\n");
        assert_eq!(error.kind, CrpixErrorKind::UnknownPixel("z".into()));
    }

    /// A clip is a range in sheet order, because an Aseprite frame tag is.
    /// Playback order is what `reverse` and `pingpong` are for.
    #[test]
    fn a_clip_that_is_not_a_contiguous_run_is_refused() {
        let three = "crpix: 1 1 3 1 1\n\npalette:\n  k c #000\n\n                     frame a:\n  k\n\nframe b:\n  k\n\nframe c:\n  k\n\n";
        parsed(&format!("{three}clip run: a b c\n"));
        assert!(matches!(
            rejected(&format!("{three}clip skip: a c\n")).kind,
            CrpixErrorKind::ClipNotContiguous { .. }
        ));
        assert!(
            matches!(
                rejected(&format!("{three}clip back: c b a\n")).kind,
                CrpixErrorKind::ClipNotContiguous { .. }
            ),
            "descending is `reverse`, not a backwards list"
        );
    }

    #[test]
    fn a_clip_naming_a_frame_that_does_not_exist_is_refused() {
        let error =
            rejected("crpix: 1 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\nclip go: a b\n");
        assert_eq!(
            error.kind,
            CrpixErrorKind::UnknownFrame {
                clip: "go".into(),
                frame: "b".into()
            }
        );
    }

    #[test]
    fn a_file_without_a_header_is_refused_before_anything_else() {
        assert_eq!(
            rejected("palette:\n  k c #000\n").kind,
            CrpixErrorKind::MissingHeader
        );
        assert!(matches!(
            rejected("crpix: 8 5 2 4\n").kind,
            CrpixErrorKind::BadHeader(_)
        ));
        assert!(matches!(
            rejected("crpix: 8 0 2 4 1\n").kind,
            CrpixErrorKind::BadHeader(_)
        ));
        assert_eq!(
            rejected("crpix: 1 1 1 1 1\ncrpix: 2 2 1 1 1\n").kind,
            CrpixErrorKind::RepeatedHeader
        );
    }

    #[test]
    fn the_remaining_rules_each_have_a_line_and_a_message() {
        let prefix = "crpix: 1 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\n";
        assert!(matches!(
            rejected(&format!("{prefix}clip go: a @ 0\n")).kind,
            CrpixErrorKind::BadHold(_)
        ));
        assert!(matches!(
            rejected(&format!("{prefix}nine: 1 2 3\n")).kind,
            CrpixErrorKind::BadNine(_)
        ));
        assert!(matches!(
            rejected(&format!("{prefix}sample: fuzzy\n")).kind,
            CrpixErrorKind::BadSample(_)
        ));
        assert!(matches!(
            rejected(&format!("{prefix}what is this\n")).kind,
            CrpixErrorKind::Unrecognised(_)
        ));
        assert!(matches!(
            rejected("crpix: 1 1 1 1 1\n\npalette:\n  k\n").kind,
            CrpixErrorKind::BadPaletteEntry(_)
        ));
        assert_eq!(
            rejected("crpix: 1 1 1 2 1\n\npalette:\n  k c #000\n  k c #fff\n").kind,
            CrpixErrorKind::DuplicateKey("k".into())
        );
    }

    #[test]
    fn nine_slice_insets_are_checked_against_the_frame() {
        let art = parsed(
            "crpix: 4 2 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkk\n  kkkk\n\nnine: 1 1 1 0\n",
        );
        assert_eq!(art.nine, Some(NineSlice::new(1, 1, 1, 0)));

        let error = rejected(
            "crpix: 4 2 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kkkk\n  kkkk\n\nnine: 3 3 0 0\n",
        );
        assert!(matches!(error.kind, CrpixErrorKind::Sheet(_)));
        assert!(error.to_string().contains("do not fit"), "{error}");
    }

    #[test]
    fn colours_may_be_three_six_or_eight_digits() {
        let art = parsed(
            "crpix: 4 1 1 4 1\n\npalette:\n  a c #f00\n  b c #00ff00\n  c c #0000ff80\n  \
             d c None\n\nframe f:\n  abcd\n",
        );
        let px = |i: usize| &art.frames[0].pixels[i * 4..i * 4 + 4];
        assert_eq!(px(0), [0xff, 0, 0, 0xff]);
        assert_eq!(px(1), [0, 0xff, 0, 0xff]);
        assert_eq!(px(2), [0, 0, 0xff, 0x80]);
        assert_eq!(
            px(3),
            [0, 0, 0, 0],
            "a transparent pixel must have no colour left in it, or it haloes"
        );
    }

    #[test]
    fn a_hash_is_a_comment_unless_it_is_a_colour() {
        let art = parsed(
            "crpix: 2 1 1 2 1  # a strip\n\npalette:\n  k c #123456   # the outline\n  \
             w c #fff\n\nframe a:  # named a\n  kw\n",
        );
        assert_eq!(art.frames[0].pixels[0..4], [0x12, 0x34, 0x56, 0xff]);
        assert_eq!(art.frames[0].pixels[4..8], [0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn a_clip_can_be_reversed_ping_ponged_or_played_once() {
        let art = parsed(
            "crpix: 1 1 2 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\nframe b:\n  k\n\n\
             clip one: a b @ 2\nclip back: a b reverse\nclip bounce: a b pingpong loop\n",
        );
        assert_eq!(art.clips[0].direction, Direction::Forward);
        assert!(
            !art.clips[0].looping,
            "a clip is one-shot unless it says loop"
        );
        assert_eq!(art.clips[1].direction, Direction::Reverse);
        assert_eq!(art.clips[2].direction, Direction::PingPong);
        assert!(art.clips[2].looping);
    }

    /// A still sprite is fully described by its PNG; anything more needs the
    /// sidecar.
    #[test]
    fn only_art_with_something_to_say_needs_a_sidecar() {
        let still = parsed("crpix: 1 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n");
        assert!(!still.needs_metadata());

        assert!(parsed(BIRD).needs_metadata(), "two frames");
        assert!(
            parsed("crpix: 2 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  kk\n\nnine: 1 1 0 0\n")
                .needs_metadata(),
            "a nine-slice"
        );
        assert!(
            parsed("crpix: 1 1 1 1 1\n\npalette:\n  k c #000\n\nframe a:\n  k\n\nclip go: a\n")
                .needs_metadata(),
            "a clip"
        );
    }
}
