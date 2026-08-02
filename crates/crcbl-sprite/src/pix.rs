//! `.pix` — pixel art as text, for art authored in this repository.
//!
//! A build input and nothing else. `build.rs` parses one of these, lays the
//! frames out into an image and writes a PNG plus the Aseprite-schema JSON the
//! engine actually reads, so nothing downstream of the converter knows this
//! format exists. That is the point: replacing it later costs nothing, and art
//! that arrives from Aseprite instead is indistinguishable by the time it
//! reaches [`Sheet`].
//!
//! # The format
//!
//! ```text
//! # comments run to the end of the line
//!
//! palette:
//!   . transparent
//!   k #241c1c
//!   y #f2c14e
//!   w #ffffffcc      # eight digits is RGBA
//!
//! frame up:
//!   ..kkkk..
//!   .kyyyyk.
//!   kyywwyyk
//!   ..kkkk..
//!
//! frame level:
//!   ...
//!
//! clip flap: up level down @ 6      # six ticks a frame
//! clip glide: level @ 1 loop
//! nine: 2 2 1 1                     # left right top bottom, in pixels
//! ```
//!
//! Dimensions are **inferred from the rows**, never declared. A header that
//! restates the size is a second copy of the truth, and the copy is what rots:
//! XPM's `"12 8 5 1"` line is exactly the kind of hand-maintained redundancy
//! this format exists to avoid.
//!
//! # The parser is hostile on purpose
//!
//! Every one of these is a build failure naming a line, not a sprite that
//! renders subtly wrong:
//!
//! * a row longer or shorter than the frame's first row,
//! * a character with no palette entry,
//! * two frames of different sizes in one sheet — an animation of those is not
//!   a thing, and it is usually a typo in one row,
//! * a clip naming a frame that does not exist,
//! * insets that overlap, or a hold of zero.
//!
//! The failure mode being avoided is specific: art whose rows are one character
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

/// A parsed `.pix` file: the art, and the sheet description over it.
#[derive(Clone, Debug, PartialEq)]
pub struct PixArt {
    /// Every frame is this size — enforced, not assumed.
    pub width: u32,
    pub height: u32,
    pub frames: Vec<FrameArt>,
    pub clips: Vec<Clip>,
    pub nine: Option<NineSlice>,
    pub sample: SampleMode,
}

impl PixArt {
    /// Lays the frames left to right into one image and returns the pixels
    /// alongside the [`Sheet`] that describes them.
    ///
    /// A horizontal strip, which is Aseprite's own default and keeps the
    /// arithmetic trivial: frame `i` starts at `x = i * width`. A packed atlas
    /// saves texture memory that a sheet of eight 16×16 frames does not have a
    /// problem with, and it would put a bin-packer between the art and the
    /// screen.
    ///
    /// Returns `(rgba, sheet)` where `rgba` is row-major over the whole strip.
    #[must_use]
    pub fn to_sheet(&self) -> (Vec<u8>, Sheet) {
        let count = self.frames.len() as u32;
        let sheet_w = self.width * count;
        let sheet_h = self.height;
        let mut rgba = vec![0u8; (sheet_w * sheet_h * 4) as usize];

        for (index, frame) in self.frames.iter().enumerate() {
            let x0 = index as u32 * self.width;
            for y in 0..self.height {
                let src = (y * self.width * 4) as usize;
                let dst = ((y * sheet_w + x0) * 4) as usize;
                let run = (self.width * 4) as usize;
                rgba[dst..dst + run].copy_from_slice(&frame.pixels[src..src + run]);
            }
        }

        let sheet = Sheet {
            width: sheet_w,
            height: sheet_h,
            frames: self
                .frames
                .iter()
                .enumerate()
                .map(|(index, frame)| Frame {
                    name: frame.name.clone(),
                    rect: Rect::new(index as u32 * self.width, 0, self.width, self.height),
                    hold: frame.hold,
                })
                .collect(),
            clips: self.clips.clone(),
            nine: self.nine,
            sample: self.sample,
        };
        (rgba, sheet)
    }
}

/// Where a parse failed, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixError {
    /// One-based, so it matches what an editor shows.
    pub line: usize,
    pub kind: PixErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PixErrorKind {
    /// A line that is not a comment, a section header or a row.
    Unrecognised(String),
    /// A palette entry that is not `<char> <colour>`.
    BadPaletteEntry(String),
    /// A colour that is not `transparent`, `#rgb`, `#rrggbb` or `#rrggbbaa`.
    BadColour(String),
    /// A palette key that is not exactly one character.
    KeyNotOneCharacter(String),
    /// Two palette entries for the same character.
    DuplicateKey(char),
    /// A pixel character with no palette entry.
    UnknownPixel(char),
    /// A row whose length differs from the first row of its frame.
    RaggedRow { expected: usize, found: usize },
    /// A frame whose size differs from the first frame's.
    FrameSizeMismatch {
        first: (u32, u32),
        found: (u32, u32),
    },
    /// Art before any `palette:` section.
    NoPalette,
    /// A frame with no rows.
    EmptyFrame(String),
    /// A clip naming a frame that was never defined.
    UnknownFrame { clip: String, frame: String },
    /// A `@ N` that is not a positive number of ticks.
    BadHold(String),
    /// A `nine:` that is not four numbers.
    BadNine(String),
    /// A `sample:` that is not `pixel` or `smooth`.
    BadSample(String),
    /// The file defines no frames at all.
    NoFrames,
    /// The sheet the file describes broke one of [`Sheet`]'s own rules.
    Sheet(crate::SheetError),
}

impl fmt::Display for PixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: ", self.line)?;
        match &self.kind {
            PixErrorKind::Unrecognised(line) => {
                write!(
                    f,
                    "expected a palette entry, a section or a row of art, got `{line}`"
                )
            }
            PixErrorKind::BadPaletteEntry(line) => {
                write!(f, "a palette entry is `<char> <colour>`, got `{line}`")
            }
            PixErrorKind::BadColour(text) => write!(
                f,
                "`{text}` is not a colour; write `transparent`, `#rgb`, `#rrggbb` or `#rrggbbaa`"
            ),
            PixErrorKind::KeyNotOneCharacter(key) => write!(
                f,
                "the palette key `{key}` is {} characters; one pixel is one character",
                key.chars().count()
            ),
            PixErrorKind::DuplicateKey(key) => {
                write!(f, "`{key}` already has a colour")
            }
            PixErrorKind::UnknownPixel(pixel) => write!(
                f,
                "`{pixel}` is not in the palette; every character in a row must be"
            ),
            PixErrorKind::RaggedRow { expected, found } => write!(
                f,
                "this row is {found} pixels wide and the frame's first row is {expected}; \
                 a ragged frame renders as a sheared sprite"
            ),
            PixErrorKind::FrameSizeMismatch { first, found } => write!(
                f,
                "this frame is {}x{} and the first is {}x{}; every frame in a sheet is \
                 the same size",
                found.0, found.1, first.0, first.1
            ),
            PixErrorKind::NoPalette => {
                write!(f, "art before any `palette:` section")
            }
            PixErrorKind::EmptyFrame(name) => {
                write!(f, "frame `{name}` has no rows")
            }
            PixErrorKind::UnknownFrame { clip, frame } => {
                write!(
                    f,
                    "clip `{clip}` names frame `{frame}`, which is not defined"
                )
            }
            PixErrorKind::BadHold(text) => write!(
                f,
                "`@ {text}` is not a positive number of ticks to hold each frame"
            ),
            PixErrorKind::BadNine(text) => write!(
                f,
                "`nine:` takes four pixel insets — left right top bottom — got `{text}`"
            ),
            PixErrorKind::BadSample(text) => {
                write!(f, "`sample:` is `pixel` or `smooth`, got `{text}`")
            }
            PixErrorKind::NoFrames => write!(f, "the file defines no frames"),
            PixErrorKind::Sheet(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for PixError {}

/// Parses a `.pix` document.
///
/// # Errors
///
/// [`PixError`] naming the line and the rule broken. Parsing stops at the first
/// one: a build failure needs the first cause, not a list downstream of it.
pub fn parse(source: &str) -> Result<PixArt, PixError> {
    Parser::new(source).run()
}

/// A palette entry's colour, as straight (non-premultiplied) RGBA.
type Rgba = [u8; 4];

struct Parser<'a> {
    source: &'a str,
    palette: Vec<(char, Rgba)>,
    frames: Vec<FrameArt>,
    clips: Vec<(String, Vec<String>, u32, Direction, bool)>,
    nine: Option<NineSlice>,
    sample: SampleMode,
    size: Option<(u32, u32)>,
    /// The frame being read: its name, its rows so far, and the line it opened
    /// on so an empty one can be reported where it was written.
    open: Option<(String, Vec<Vec<Rgba>>, usize)>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            palette: Vec::new(),
            frames: Vec::new(),
            clips: Vec::new(),
            nine: None,
            sample: SampleMode::Pixel,
            size: None,
            open: None,
        }
    }

    fn run(mut self) -> Result<PixArt, PixError> {
        let mut in_palette = false;

        for (index, raw) in self.source.lines().enumerate() {
            let line = index + 1;
            let text = strip_comment(raw);
            if text.trim().is_empty() {
                continue;
            }
            let trimmed = text.trim();

            // A section header is unindented and ends in a colon, or is one of
            // the single-line directives. Rows are indented. That is the whole
            // of the grammar's structure, and it is why a row can contain a
            // colon character without ambiguity.
            if let Some(rest) = trimmed.strip_prefix("palette:") {
                self.close_frame(line)?;
                if !rest.trim().is_empty() {
                    return Err(PixError {
                        line,
                        kind: PixErrorKind::Unrecognised(trimmed.to_string()),
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
                    .ok_or_else(|| PixError {
                        line,
                        kind: PixErrorKind::Unrecognised(trimmed.to_string()),
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
                        return Err(PixError {
                            line,
                            kind: PixErrorKind::BadSample(other.to_string()),
                        });
                    }
                };
                continue;
            }

            if in_palette {
                // The raw line, not the comment-stripped one: see
                // `palette_entry`, where the two `#`s in `k #123456 # note` are
                // told apart by position rather than by guesswork.
                self.palette_entry(raw, line)?;
            } else if self.open.is_some() {
                self.row(text, line)?;
            } else {
                return Err(PixError {
                    line,
                    kind: PixErrorKind::Unrecognised(trimmed.to_string()),
                });
            }
        }

        let end = self.source.lines().count().max(1);
        self.close_frame(end)?;
        self.finish(end)
    }

    /// A palette entry is the **first two whitespace-separated tokens** of the
    /// line; anything after them is a comment.
    ///
    /// Token-splitting rather than cutting at the first `#`, because a colour
    /// starts with one: `k #123456  # the outline` has two, and only the second
    /// is a comment. Deciding which by looking at what follows the `#` is how
    /// the first version of this got `k #00` wrong — it read the short colour as
    /// a comment, cut the line to `k `, and reported a malformed *entry* instead
    /// of the malformed *colour* the author had actually typed.
    fn palette_entry(&mut self, text: &str, line: usize) -> Result<(), PixError> {
        let mut parts = text.split_whitespace();
        let key = parts.next().unwrap_or_default();
        let colour = parts.next().unwrap_or_default();
        if key.is_empty() || colour.is_empty() {
            return Err(PixError {
                line,
                kind: PixErrorKind::BadPaletteEntry(text.to_string()),
            });
        }
        let mut chars = key.chars();
        let (Some(key), None) = (chars.next(), chars.next()) else {
            return Err(PixError {
                line,
                kind: PixErrorKind::KeyNotOneCharacter(key.to_string()),
            });
        };
        if self.palette.iter().any(|(existing, _)| *existing == key) {
            return Err(PixError {
                line,
                kind: PixErrorKind::DuplicateKey(key),
            });
        }
        self.palette.push((key, parse_colour(colour, line)?));
        Ok(())
    }

    fn row(&mut self, text: &str, line: usize) -> Result<(), PixError> {
        // Trimmed at **both** ends, which is to say a space is never a pixel.
        //
        // The alternative — leading spaces are art when the palette maps ' ' —
        // makes a row's indentation and its content the same thing, so a file
        // that looks aligned is not, and the parser cannot tell an indent from a
        // transparent column. Pixel art writes empty as `.` by convention
        // anyway; the ambiguity buys nothing and costs the one property this
        // format is for, which is being readable as art.
        let row_text = text.trim();

        let mut row = Vec::with_capacity(row_text.chars().count());
        for pixel in row_text.chars() {
            let colour = self
                .palette
                .iter()
                .find(|(key, _)| *key == pixel)
                .map(|(_, colour)| *colour);
            match colour {
                Some(colour) => row.push(colour),
                None if self.palette.is_empty() => {
                    return Err(PixError {
                        line,
                        kind: PixErrorKind::NoPalette,
                    });
                }
                None => {
                    return Err(PixError {
                        line,
                        kind: PixErrorKind::UnknownPixel(pixel),
                    });
                }
            }
        }

        let (_, rows, _) = self.open.as_mut().expect("checked by the caller");
        if let Some(first) = rows.first()
            && first.len() != row.len()
        {
            return Err(PixError {
                line,
                kind: PixErrorKind::RaggedRow {
                    expected: first.len(),
                    found: row.len(),
                },
            });
        }
        rows.push(row);
        Ok(())
    }

    fn clip(&mut self, text: &str, line: usize) -> Result<(), PixError> {
        let (name, rest) = text.split_once(':').ok_or_else(|| PixError {
            line,
            kind: PixErrorKind::Unrecognised(format!("clip {text}")),
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
                        .ok_or_else(|| PixError {
                            line,
                            kind: PixErrorKind::BadHold(value.to_string()),
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

    /// Turns the open frame's rows into a [`FrameArt`], checking its shape
    /// against the frames already read.
    fn close_frame(&mut self, line: usize) -> Result<(), PixError> {
        let Some((name, rows, opened)) = self.open.take() else {
            return Ok(());
        };
        if rows.is_empty() {
            return Err(PixError {
                line: opened,
                kind: PixErrorKind::EmptyFrame(name),
            });
        }
        let width = rows[0].len() as u32;
        let height = rows.len() as u32;
        match self.size {
            None => self.size = Some((width, height)),
            Some(first) if first != (width, height) => {
                return Err(PixError {
                    line,
                    kind: PixErrorKind::FrameSizeMismatch {
                        first,
                        found: (width, height),
                    },
                });
            }
            Some(_) => {}
        }

        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in &rows {
            for colour in row {
                pixels.extend_from_slice(colour);
            }
        }
        self.frames.push(FrameArt {
            name,
            pixels,
            // Replaced below by whichever clip names it; a frame nothing names
            // is still held for a tick so a still sprite is a legal sheet.
            hold: 1,
        });
        Ok(())
    }

    fn finish(mut self, line: usize) -> Result<PixArt, PixError> {
        let Some((width, height)) = self.size else {
            return Err(PixError {
                line,
                kind: PixErrorKind::NoFrames,
            });
        };

        // Resolve clips by name, and let each clip set the hold of the frames it
        // names. Holding is per frame in Aseprite and per clip here, because
        // hand-written art wants one number for a whole animation; when the same
        // frame appears in two clips at different rates, the last one wins and
        // that is worth knowing, so it is stated rather than silently averaged.
        let mut clips = Vec::with_capacity(self.clips.len());
        for (name, frame_names, hold, direction, looping) in self.clips {
            let mut indices = Vec::with_capacity(frame_names.len());
            for frame in frame_names {
                let index = self
                    .frames
                    .iter()
                    .position(|candidate| candidate.name == frame)
                    .ok_or_else(|| PixError {
                        line,
                        kind: PixErrorKind::UnknownFrame {
                            clip: name.clone(),
                            frame: frame.clone(),
                        },
                    })?;
                self.frames[index].hold = hold;
                indices.push(index);
            }
            clips.push(Clip {
                name,
                frames: indices,
                direction,
                looping,
            });
        }

        let art = PixArt {
            width,
            height,
            frames: self.frames,
            clips,
            nine: self.nine,
            sample: self.sample,
        };

        // The sheet's own rules, checked here so a `.pix` that parses cannot
        // still produce a sheet a consumer would reject.
        let (_, sheet) = art.to_sheet();
        sheet.validate().map_err(|error| PixError {
            line,
            kind: PixErrorKind::Sheet(error),
        })?;
        Ok(art)
    }
}

/// Everything before the first `#`.
///
/// Unambiguous for two reasons that fit together. Palette entries are
/// **tokenised** rather than comment-stripped, so the one place a `#` appears
/// legitimately mid-line — the colour in `k #123456` — never reaches here. And
/// `#` therefore cannot be a palette key, because a line starting with one is
/// stripped to nothing before it could be read as an entry — so no row can
/// contain a `#` either.
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn parse_colour(text: &str, line: usize) -> Result<Rgba, PixError> {
    if text.eq_ignore_ascii_case("transparent") || text == "-" {
        // Zero alpha *and* zero colour: a transparent pixel whose RGB is some
        // leftover shows up as a halo when the sampler blends across it, which
        // is the classic "why is there a dark fringe round my sprite".
        return Ok([0, 0, 0, 0]);
    }
    let hex = text.strip_prefix('#').ok_or_else(|| PixError {
        line,
        kind: PixErrorKind::BadColour(text.to_string()),
    })?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PixError {
            line,
            kind: PixErrorKind::BadColour(text.to_string()),
        });
    }
    let byte = |from: usize, to: usize| u8::from_str_radix(&hex[from..to], 16).unwrap_or(0);
    let nibble = |at: usize| {
        let value = u8::from_str_radix(&hex[at..=at], 16).unwrap_or(0);
        value * 17
    };
    Ok(match hex.len() {
        3 => [nibble(0), nibble(1), nibble(2), 255],
        6 => [byte(0, 2), byte(2, 4), byte(4, 6), 255],
        8 => [byte(0, 2), byte(2, 4), byte(4, 6), byte(6, 8)],
        _ => {
            return Err(PixError {
                line,
                kind: PixErrorKind::BadColour(text.to_string()),
            });
        }
    })
}

fn nine(text: &str, line: usize) -> Result<NineSlice, PixError> {
    let values: Vec<u32> = text
        .split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect();
    let [left, right, top, bottom] = values[..] else {
        return Err(PixError {
            line,
            kind: PixErrorKind::BadNine(text.trim().to_string()),
        });
    };
    Ok(NineSlice::new(left, right, top, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIRD: &str = "\
# a two-frame bird
palette:
  . transparent
  k #000000
  y #f2c14e

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

    fn parsed(source: &str) -> PixArt {
        match parse(source) {
            Ok(art) => art,
            Err(error) => panic!("expected this to parse: {error}"),
        }
    }

    fn rejected(source: &str) -> PixError {
        parse(source).expect_err("expected this to be refused")
    }

    #[test]
    fn a_sheet_is_read_with_its_size_inferred_from_the_rows() {
        let art = parsed(BIRD);
        assert_eq!((art.width, art.height), (5, 3));
        assert_eq!(art.frames.len(), 2);
        assert_eq!(art.frames[0].name, "up");
        assert_eq!(art.frames[0].pixels.len(), 5 * 3 * 4);

        // The palette reached the pixels: the middle of row 1 is the body
        // colour, and the corners are transparent.
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
        assert_eq!(art.clips[0].name, "flap");
        assert_eq!(art.clips[0].frames, vec![0, 1]);
        assert!(art.clips[0].looping);
        assert_eq!(art.clips[0].direction, Direction::Forward);
        assert_eq!(art.frames[0].hold, 6);
        assert_eq!(art.frames[1].hold, 6);
    }

    #[test]
    fn frames_are_laid_out_left_to_right_into_one_strip() {
        let art = parsed(BIRD);
        let (rgba, sheet) = art.to_sheet();
        assert_eq!((sheet.width, sheet.height), (10, 3));
        assert_eq!(rgba.len(), 10 * 3 * 4);
        assert_eq!(sheet.frames[0].rect, Rect::new(0, 0, 5, 3));
        assert_eq!(sheet.frames[1].rect, Rect::new(5, 0, 5, 3));
        sheet.validate().expect("a parsed sheet is always valid");

        // The second frame's pixels really are at x = 5: its bottom row is all
        // transparent where the first frame's is not.
        let at = |x: usize, y: usize| {
            let i = (y * 10 + x) * 4;
            &rgba[i..i + 4]
        };
        assert_eq!(at(1, 2), [0, 0, 0, 0xff], "frame `up` has a bottom edge");
        assert_eq!(at(6, 2), [0, 0, 0, 0], "frame `down` does not");
    }

    /// **The failure this format exists to catch.** A row one character short
    /// renders as a sheared sprite, and a sheared sprite is blamed on the
    /// renderer.
    #[test]
    fn a_ragged_row_is_refused_with_its_line_number() {
        let error = rejected("palette:\n  k #000\n\nframe a:\n  kkkk\n  kkk\n  kkkk\n");
        assert_eq!(error.line, 6);
        assert_eq!(
            error.kind,
            PixErrorKind::RaggedRow {
                expected: 4,
                found: 3
            }
        );
        assert!(error.to_string().contains("sheared"), "{error}");
    }

    #[test]
    fn frames_of_different_sizes_are_refused() {
        let error =
            rejected("palette:\n  k #000\n\nframe a:\n  kk\n  kk\n\nframe b:\n  kkk\n  kkk\n");
        assert!(matches!(error.kind, PixErrorKind::FrameSizeMismatch { .. }));
    }

    #[test]
    fn a_pixel_with_no_palette_entry_is_refused() {
        let error = rejected("palette:\n  k #000\n\nframe a:\n  kzk\n");
        assert_eq!(error.line, 5);
        assert_eq!(error.kind, PixErrorKind::UnknownPixel('z'));
    }

    #[test]
    fn a_clip_naming_a_frame_that_does_not_exist_is_refused() {
        let error = rejected("palette:\n  k #000\n\nframe a:\n  k\n\nclip go: a b\n");
        assert_eq!(
            error.kind,
            PixErrorKind::UnknownFrame {
                clip: "go".into(),
                frame: "b".into()
            }
        );
    }

    #[test]
    fn the_remaining_rules_each_have_a_line_and_a_message() {
        assert_eq!(rejected("frame a:\n  kk\n").kind, PixErrorKind::NoPalette);
        assert!(matches!(
            rejected("palette:\n  k #000\n\nframe a:\n\nframe b:\n  k\n").kind,
            PixErrorKind::EmptyFrame(_)
        ));
        assert!(matches!(
            rejected("palette:\n  kk #000\n").kind,
            PixErrorKind::KeyNotOneCharacter(_)
        ));
        assert_eq!(
            rejected("palette:\n  k #000\n  k #fff\n").kind,
            PixErrorKind::DuplicateKey('k')
        );
        assert!(matches!(
            rejected("palette:\n  k nonsense\n").kind,
            PixErrorKind::BadColour(_)
        ));
        assert!(matches!(
            rejected("palette:\n  k #00\n").kind,
            PixErrorKind::BadColour(_)
        ));
        assert!(matches!(
            rejected("palette:\n  k #000\n\nframe a:\n  k\n\nclip go: a @ 0\n").kind,
            PixErrorKind::BadHold(_)
        ));
        assert!(matches!(
            rejected("palette:\n  k #000\n\nframe a:\n  k\n\nnine: 1 2 3\n").kind,
            PixErrorKind::BadNine(_)
        ));
        assert!(matches!(
            rejected("palette:\n  k #000\n\nframe a:\n  k\n\nsample: fuzzy\n").kind,
            PixErrorKind::BadSample(_)
        ));
        assert_eq!(
            rejected("palette:\n  k #000\n").kind,
            PixErrorKind::NoFrames
        );
        // Outside any section, a stray line has no possible reading.
        assert!(matches!(
            rejected("what is this\n").kind,
            PixErrorKind::Unrecognised(_)
        ));
        // Inside the palette it does have one, and the message is about the
        // reading the parser actually attempted rather than a generic refusal.
        assert!(matches!(
            rejected("palette:\n  what is this\n").kind,
            PixErrorKind::KeyNotOneCharacter(_)
        ));
        // `#` cannot be a palette key, and needs no rule to say so: a line
        // whose first character is one is a comment, so the entry never exists.
        // Which is why rows can be comment-stripped at the first `#` with no
        // ambiguity at all.
        assert_eq!(
            rejected("palette:\n  # #000\n\nframe a:\n  #\n").kind,
            PixErrorKind::EmptyFrame("a".into()),
            "the palette entry and the row were both read as comments"
        );
    }

    /// Insets that overlap would give the centre a negative size. The sheet's
    /// own rule, reached through the parser so a `.pix` cannot produce a sheet
    /// its consumer would reject.
    #[test]
    fn nine_slice_insets_are_checked_against_the_frame() {
        let art = parsed("palette:\n  k #000\n\nframe a:\n  kkkk\n  kkkk\n\nnine: 1 1 1 0\n");
        assert_eq!(art.nine, Some(NineSlice::new(1, 1, 1, 0)));

        let error = rejected("palette:\n  k #000\n\nframe a:\n  kkkk\n  kkkk\n\nnine: 3 3 0 0\n");
        assert!(matches!(error.kind, PixErrorKind::Sheet(_)));
        assert!(error.to_string().contains("do not fit"), "{error}");
    }

    /// A `#` opens a comment, except where it opens a colour — otherwise every
    /// palette entry would be a comment and the file would have no colours at
    /// all.
    #[test]
    fn a_hash_is_a_comment_unless_it_is_a_colour() {
        let art = parsed(
            "palette:\n  k #123456   # the outline\n  w #fff\n\nframe a:  # named a\n  kw\n",
        );
        assert_eq!(art.frames[0].pixels[0..4], [0x12, 0x34, 0x56, 0xff]);
        assert_eq!(art.frames[0].pixels[4..8], [0xff, 0xff, 0xff, 0xff]);
    }

    /// Short hex expands the way CSS does, and eight digits carry alpha.
    #[test]
    fn colours_may_be_three_six_or_eight_digits() {
        let art = parsed(
            "palette:\n  a #f00\n  b #00ff00\n  c #0000ff80\n  d transparent\n\nframe f:\n  abcd\n",
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

    /// **A space is never a pixel.** Rows are trimmed at both ends, so a file
    /// that looks aligned is aligned; the alternative makes indentation and
    /// content the same thing and no reader can tell them apart.
    ///
    /// The cost is one character of vocabulary. Pixel art writes empty as `.`
    /// anyway, and the error says so.
    #[test]
    fn a_space_inside_a_row_is_refused_rather_than_read_as_a_pixel() {
        let error = rejected("palette:\n  k #fff\n  . transparent\n\nframe a:\n  k k\n");
        assert_eq!(error.kind, PixErrorKind::UnknownPixel(' '));

        // And indentation is free: the same art indented four spaces, or none,
        // is the same sprite.
        let flush = parsed("palette:\n  k #fff\n\nframe a:\n  kk\n  kk\n");
        let deep = parsed("palette:\n  k #fff\n\nframe a:\n      kk\n      kk\n");
        assert_eq!(flush, deep);
    }

    /// Directions and one-shots, which is what a death animation needs.
    #[test]
    fn a_clip_can_be_reversed_ping_ponged_or_played_once() {
        let art = parsed(
            "palette:\n  k #000\n\nframe a:\n  k\n\nframe b:\n  k\n\n\
             clip one: a b @ 2\nclip back: b a reverse\nclip bounce: a b pingpong loop\n",
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
}
