//! Sprite sheets: what is in one, how it animates, and how it stretches.
//!
//! # What this crate is, and is not
//!
//! It is the **description** of a sheet — where each frame sits in an image,
//! which frames make up a named animation, how long each is held, and which
//! parts of a frame may stretch when it is drawn at a size it was not drawn at.
//! It is deliberately none of the rest: no GPU types, no file I/O, no image
//! codec. Turning a description into pixels belongs to `crcbl-render`, and
//! reading a PNG belongs to whoever owns the bytes.
//!
//! That division is what lets a `build.rs` depend on this crate to convert
//! source art without dragging a renderer into the build, and it is what makes
//! every rule here testable with no device and no files.
//!
//! # Two formats, and only one of them reaches the engine
//!
//! ```text
//! .crpix (text, in git) ──bake──▶ PNG + Aseprite-schema JSON ─┐
//! Aseprite ──────────export────▶ PNG + Aseprite-schema JSON ──┴──▶ [`Sheet`]
//! ```
//!
//! **Aseprite is the interchange format**, because it is the one the world
//! already uses and because its JSON export already carries everything a sprite
//! system needs: `frames[]` with per-frame `duration`, `meta.frameTags` for
//! named clips with a direction, and `meta.slices[].keys[].center` — which *is*
//! nine-slice. Reading that schema means art drawn in Aseprite drops in with no
//! engine change.
//!
//! [`crpix`] is the other half: a text format for art authored **in this
//! repository**, baked to a PNG and that same Aseprite schema at build time. It
//! takes XPM's design — a declared header that acts as a checksum, `c None`,
//! multi-character palette keys — and adds the frames XPM has no concept of.
//! `docs/specs/crcbl/pix.md` is its specification; nothing downstream of the
//! converter knows it exists.
//!
//! # Pixels are the unit
//!
//! Every rectangle here is in **texels of the sheet image**, as unsigned
//! integers. Not normalised UVs, and not floats: a sprite sheet is a grid of
//! whole pixels, half a texel is always a bug, and the conversion to UVs needs
//! the image's size, which a description of a sheet does not have to know.

pub mod colours;
pub mod crpix;
pub mod trace;

#[cfg(feature = "bake")]
pub mod bake;

#[cfg(feature = "load")]
pub mod load;

use core::fmt;

/// A rectangle in sheet texels.
///
/// `x`/`y` are the top-left corner, matching image order and Aseprite's own
/// `frame` rects, so a value read out of a JSON export needs no flipping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// Whether the rectangle has any area at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Whether `self` lies entirely within a `width` × `height` image.
    #[must_use]
    pub const fn fits_in(&self, width: u32, height: u32) -> bool {
        // Addition rather than subtraction, and checked: a frame at
        // `x = u32::MAX - 1` with `w = 4` must not wrap into "fits".
        match (self.x.checked_add(self.w), self.y.checked_add(self.h)) {
            (Some(right), Some(bottom)) => right <= width && bottom <= height,
            _ => false,
        }
    }
}

/// One frame of a sheet: where it is, and how long it is held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The frame's name. Aseprite writes one per frame; `.pix` requires one,
    /// because a clip refers to frames by name and an index is a thing that
    /// silently means something else after an edit.
    pub name: String,
    /// Where the frame sits in the sheet image.
    pub rect: Rect,
    /// How long the frame is held, in **simulation ticks**.
    ///
    /// Ticks, not milliseconds. A sample's animation has to advance the same
    /// way at 20 fps and 240 fps for the same reason its physics does, and the
    /// only clock that is true of is the fixed tick. Aseprite records
    /// milliseconds, so the importer converts once, at the tick rate it is
    /// told.
    pub hold: u32,
}

/// Which way a clip runs through its frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// First frame to last, then back to the first.
    #[default]
    Forward,
    /// Last frame to first.
    Reverse,
    /// Forward, then back, without repeating either end.
    ///
    /// The end frames are held once, not twice: a four-frame ping-pong is
    /// `0 1 2 3 2 1` and then `0` again, which is what Aseprite does and what
    /// looks right. Holding them twice reads as a stutter at each extreme.
    PingPong,
}

/// A named animation: an ordered run of frames, and how to walk it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub name: String,
    /// Indices into [`Sheet::frames`], in playback order.
    pub frames: Vec<usize>,
    pub direction: Direction,
    /// Whether the clip returns to the start after the last frame.
    ///
    /// A clip that does not loop holds its final frame forever, which is what a
    /// one-shot — a death, a button press — should do.
    pub looping: bool,
}

impl Clip {
    /// How many frame *showings* one pass of the clip contains.
    ///
    /// Not the same as `frames.len()`: a ping-pong shows the middle frames
    /// twice. This is the number [`Clip::step`] indexes, and the clip's period
    /// in showings when it loops.
    ///
    /// | direction   | looping  | showings |
    /// | ----------- | -------- | -------- |
    /// | `Forward`   | either   | `n`      |
    /// | `Reverse`   | either   | `n`      |
    /// | `PingPong`  | looping  | `2n - 2` |
    /// | `PingPong`  | one-shot | `2n - 1` |
    ///
    /// **A looping ping-pong's period is `2n - 2`, not `2n`,** because neither
    /// end is repeated: four frames run `0 1 2 3 2 1` and then `0` again. A
    /// one-shot has the extra showing precisely because that trailing `0` is no
    /// longer the next cycle's first — an out-and-back that stopped on frame 1
    /// would look truncated, so it plays the return home and parks there.
    ///
    /// Degenerate cases: no frames is zero showings, and a single frame is one
    /// showing in every direction (`2n - 2` would be `0`).
    #[must_use]
    pub fn steps(&self) -> usize {
        let n = self.frames.len();
        match self.direction {
            _ if n <= 1 => n,
            Direction::Forward | Direction::Reverse => n,
            Direction::PingPong if self.looping => 2 * n - 2,
            Direction::PingPong => 2 * n - 1,
        }
    }

    /// The frame shown at showing `step`: an index into [`Sheet::frames`].
    ///
    /// `None` past the end of one pass, so `0..clip.steps()` walks it exactly.
    /// [`Direction::Reverse`] reverses the *frames*, and each frame keeps its
    /// own `hold` — a hold belongs to the frame it holds, not to the position
    /// in the list — so a reversed clip's tick pattern is the forward one read
    /// backwards.
    #[must_use]
    pub fn step(&self, step: usize) -> Option<usize> {
        let n = self.frames.len();
        if step >= self.steps() {
            return None;
        }
        let position = match self.direction {
            _ if n <= 1 => step,
            Direction::Forward => step,
            Direction::Reverse => n - 1 - step,
            // The descending leg mirrors the ascending one about `n - 1`, and
            // a one-shot's extra showing lands back on `0`.
            Direction::PingPong if step < n => step,
            Direction::PingPong => 2 * n - 2 - step,
        };
        self.frames.get(position).copied()
    }
}

/// Where a clip has got to, counted in whole simulation ticks.
///
/// **Ticks, not seconds.** The engine is fixed-tick — `crcbl-server`'s
/// accumulator calls the world a whole number of times per frame — and an
/// animation that advanced on a float clock would land on a different frame at
/// 20 fps than at 240 fps for the same reason a float physics step would.
/// There is no `dt` anywhere in here.
///
/// A `Playback` is a bare cursor: it holds neither the sheet nor the clip, so
/// it is `Copy`, costs eight bytes, and can sit in a component next to whatever
/// names the clip. The sheet and clip are passed to the queries, which means
/// one entity can be pointed at a different clip without rebuilding anything.
///
/// ```
/// # use crcbl_sprite::{Clip, Direction, Frame, Playback, Rect, Sheet};
/// # let frame = |name: &str, x, hold| Frame { name: name.into(), rect: Rect::new(x, 0, 8, 8), hold };
/// let sheet = Sheet {
///     width: 16,
///     height: 8,
///     frames: vec![frame("a", 0, 2), frame("b", 8, 3)],
///     clips: vec![Clip {
///         name: "idle".into(),
///         frames: vec![0, 1],
///         direction: Direction::Forward,
///         looping: true,
///     }],
///     ..Sheet::default()
/// };
/// let clip = sheet.clip("idle").expect("the sheet declares it");
///
/// let mut play = Playback::new();
/// assert_eq!(play.frame_index(&sheet, clip), Some(0));
/// play.advance(2);
/// assert_eq!(play.frame_index(&sheet, clip), Some(1), "`a` is held for 2 ticks");
/// play.advance(3);
/// assert_eq!(play.frame_index(&sheet, clip), Some(0), "and it wraps after 5");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Playback {
    elapsed: u64,
}

impl Playback {
    /// A clip about to show its first frame.
    #[must_use]
    pub const fn new() -> Self {
        Self { elapsed: 0 }
    }

    /// Ticks since the clip started.
    ///
    /// Keeps counting past the end of a looping clip rather than wrapping, so
    /// two playbacks started at the same tick stay comparable however long they
    /// run. At 60 Hz a `u64` of ticks is longer than the age of the universe;
    /// it saturates rather than wrapping regardless.
    #[must_use]
    pub const fn elapsed(&self) -> u64 {
        self.elapsed
    }

    /// Moves the clip on by whole ticks.
    ///
    /// Advancing by `n` in one call is exactly advancing by one `n` times —
    /// every query is a pure function of [`Playback::elapsed`], so a client
    /// catching up after a stall lands where a client that never stalled would.
    pub const fn advance(&mut self, ticks: u64) {
        self.elapsed = self.elapsed.saturating_add(ticks);
    }

    /// Back to the first frame, as if the clip had just started.
    pub const fn restart(&mut self) {
        self.elapsed = 0;
    }

    /// Jumps to `tick` ticks after the clip started.
    pub const fn seek(&mut self, tick: u64) {
        self.elapsed = tick;
    }

    /// The frame to draw: an index into [`Sheet::frames`].
    ///
    /// `None` only when there is nothing to draw — a clip with no frames, or
    /// one naming a frame the sheet does not have, both of which
    /// [`Sheet::validate`] refuses.
    ///
    /// A looping clip wraps; a one-shot holds its last showing forever, and
    /// says so through [`Playback::finished`].
    #[must_use]
    pub fn frame_index(&self, sheet: &Sheet, clip: &Clip) -> Option<usize> {
        let steps = clip.steps();
        let total = sheet.clip_duration(clip)?;
        if total == 0 {
            return None;
        }
        // The whole of playback is this one line: a looping clip is its
        // position modulo the pass, a one-shot is clamped to the last tick of
        // the pass. Everything below is finding which showing owns that tick.
        let tick = if clip.looping {
            self.elapsed % total
        } else {
            self.elapsed.min(total - 1)
        };
        let mut end = 0;
        for step in 0..steps {
            let frame = clip.step(step)?;
            end += u64::from(sheet.frames.get(frame)?.hold);
            if tick < end {
                return Some(frame);
            }
        }
        // `tick < total` and the holds sum to `total`, so the loop always
        // returns. Answering with the last showing rather than panicking keeps
        // a malformed sheet from taking the frame down.
        clip.step(steps - 1)
    }

    /// Whether a one-shot has played out.
    ///
    /// **Distinct from being on the last frame.** A one-shot reaches its final
    /// frame and then holds it for that frame's `hold` before this turns true,
    /// so an explosion's last puff is on screen for as long as it was drawn
    /// for. [`Playback::frame_index`] keeps answering with that frame
    /// afterwards, so a caller that ignores this still draws something.
    ///
    /// Always `false` for a looping clip: it never ends. A one-shot that cannot
    /// be drawn at all — no frames, or a frame the sheet does not have — is
    /// finished immediately, because there is nothing left for it to show.
    #[must_use]
    pub fn finished(&self, sheet: &Sheet, clip: &Clip) -> bool {
        !clip.looping
            && sheet
                .clip_duration(clip)
                .is_none_or(|total| self.elapsed >= total)
    }
}

/// The four insets that make a frame stretchable.
///
/// A nine-slice splits a frame into a 3×3 grid: the four corners never scale,
/// the top and bottom edges stretch horizontally only, the left and right edges
/// stretch vertically only, and the centre stretches both ways. That is what
/// keeps a button's rounded corner square at any size, and what lets one pipe
/// image serve a gap of any height.
///
/// Stored as insets rather than as Aseprite's centre rectangle because insets
/// are what the geometry needs and what a human can write; the two are the same
/// information and [`NineSlice::from_center`] converts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NineSlice {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl NineSlice {
    #[must_use]
    pub const fn new(left: u32, right: u32, top: u32, bottom: u32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// The insets implied by Aseprite's `center` rect inside a `frame` rect.
    ///
    /// Aseprite stores the *middle* of the nine, positioned relative to the
    /// slice's own bounds. Returns `None` when the centre is not actually inside
    /// the frame, which is a malformed export rather than a slice with odd
    /// insets.
    #[must_use]
    pub fn from_center(frame: Rect, center: Rect) -> Option<Self> {
        let right_edge = center.x.checked_add(center.w)?;
        let bottom_edge = center.y.checked_add(center.h)?;
        if right_edge > frame.w || bottom_edge > frame.h {
            return None;
        }
        Some(Self {
            left: center.x,
            right: frame.w - right_edge,
            top: center.y,
            bottom: frame.h - bottom_edge,
        })
    }

    /// Whether these insets leave a non-negative centre inside `frame`.
    ///
    /// Insets that overlap would give the centre a negative size, and the
    /// geometry builder would produce inside-out quads that render as nothing —
    /// a sprite that silently disappears at one size and not another.
    #[must_use]
    pub fn fits_in(&self, frame: Rect) -> bool {
        self.left.saturating_add(self.right) <= frame.w
            && self.top.saturating_add(self.bottom) <= frame.h
    }

    /// The smallest size this slice can be drawn at without the fixed parts
    /// overlapping, in pixels.
    #[must_use]
    pub const fn minimum_size(&self) -> (u32, u32) {
        (self.left + self.right, self.top + self.bottom)
    }
}

/// How a sprite is sampled when the screen does not agree with the art.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SampleMode {
    /// Preserve the art's pixels as exactly as the output allows.
    ///
    /// Not simply nearest-neighbour. At a non-integer scale — a 320-wide field
    /// stretched across a 1366-wide canvas — nearest makes some art pixels four
    /// screen pixels across and their neighbours five, and the unevenness
    /// crawls as the sprite moves. This mode samples linearly but bends the UV
    /// so the blend happens only within a one-fragment band at each texel
    /// boundary, which is identical to nearest at whole scales and even-looking
    /// at every other one.
    #[default]
    Pixel,
    /// Ordinary filtered sampling, for art that is not pixel art and for the
    /// normal 2D and 3D path.
    Smooth,
}

/// A sheet: one image, the frames cut out of it, and the clips over those.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    /// The sheet image's size in texels. Every frame must fit inside it.
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
    pub clips: Vec<Clip>,
    /// The nine-slice insets, when this sheet's frames are stretchable.
    pub nine: Option<NineSlice>,
    pub sample: SampleMode,
}

impl Sheet {
    /// The frame with this name.
    #[must_use]
    pub fn frame(&self, name: &str) -> Option<&Frame> {
        self.frames.iter().find(|frame| frame.name == name)
    }

    /// The index of the frame with this name.
    #[must_use]
    pub fn frame_index(&self, name: &str) -> Option<usize> {
        self.frames.iter().position(|frame| frame.name == name)
    }

    /// The clip with this name.
    #[must_use]
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.iter().find(|clip| clip.name == name)
    }

    /// Frame `index` as normalised UVs: `[u0, v0, u1, v1]`, top-left first.
    ///
    /// The one conversion out of texels this crate does, and it lives here
    /// because the divisor is the sheet's own size — the thing a `Rect` on its
    /// own does not know. `crcbl_render::Sprite::uv` wants exactly this layout,
    /// and every caller that spelled it out by hand was a place the halves
    /// could be swapped.
    ///
    /// `None` for an index the sheet does not have, or a sheet with no size at
    /// all, which would otherwise divide by zero and hand a `NaN` to a vertex
    /// buffer.
    #[must_use]
    pub fn uv(&self, index: usize) -> Option<[f32; 4]> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let rect = self.frames.get(index)?.rect;
        let width = self.width as f32;
        let height = self.height as f32;
        // 64-bit for the far edges: a rect that overflows `u32` here is one
        // `validate` refuses, and wrapping to a small UV would sample the
        // wrong sprite rather than fail.
        let right = u64::from(rect.x) + u64::from(rect.w);
        let bottom = u64::from(rect.y) + u64::from(rect.h);
        Some([
            rect.x as f32 / width,
            rect.y as f32 / height,
            right as f32 / width,
            bottom as f32 / height,
        ])
    }

    /// How many ticks one pass of `clip` lasts.
    ///
    /// The sum of the `hold` of every showing in [`Clip::steps`] order, so a
    /// ping-pong counts its middle frames twice and a reversed clip counts the
    /// same holds as its forward twin. For a one-shot this is when
    /// [`Playback::finished`] turns true; for a looping clip it is the period.
    ///
    /// `None` when the clip has no frames, or names one this sheet does not
    /// have — both refused by [`Sheet::validate`]. Skipping the missing frame
    /// and returning a shorter total would be worse than admitting it: the
    /// clip would then play forever without that frame's tick ever arriving,
    /// which looks like a renderer bug.
    #[must_use]
    pub fn clip_duration(&self, clip: &Clip) -> Option<u64> {
        let steps = clip.steps();
        if steps == 0 {
            return None;
        }
        let mut total = 0u64;
        for step in 0..steps {
            let frame = self.frames.get(clip.step(step)?)?;
            total = total.saturating_add(u64::from(frame.hold));
        }
        Some(total)
    }

    /// Checks every rule a consumer is entitled to assume.
    ///
    /// Called by every constructor in this crate, and worth calling on a sheet
    /// built by hand. The alternative to failing here is a frame rect that
    /// samples whatever is next to it in the atlas, which renders as a sliver of
    /// the neighbouring sprite along one edge — the kind of bug that is looked
    /// at for an hour and blamed on the sampler.
    ///
    /// # Errors
    ///
    /// [`SheetError`] naming the first rule broken.
    pub fn validate(&self) -> Result<(), SheetError> {
        if self.frames.is_empty() {
            return Err(SheetError::NoFrames);
        }
        for frame in &self.frames {
            if frame.rect.is_empty() {
                return Err(SheetError::EmptyFrame {
                    frame: frame.name.clone(),
                });
            }
            if !frame.rect.fits_in(self.width, self.height) {
                return Err(SheetError::FrameOutsideSheet {
                    frame: frame.name.clone(),
                    rect: frame.rect,
                    width: self.width,
                    height: self.height,
                });
            }
            if frame.hold == 0 {
                return Err(SheetError::ZeroHold {
                    frame: frame.name.clone(),
                });
            }
        }
        for (index, frame) in self.frames.iter().enumerate() {
            if self.frames[..index]
                .iter()
                .any(|other| other.name == frame.name)
            {
                return Err(SheetError::DuplicateFrame {
                    frame: frame.name.clone(),
                });
            }
        }
        for clip in &self.clips {
            if clip.frames.is_empty() {
                return Err(SheetError::EmptyClip {
                    clip: clip.name.clone(),
                });
            }
            if let Some(&bad) = clip.frames.iter().find(|&&i| i >= self.frames.len()) {
                return Err(SheetError::ClipFrameOutOfRange {
                    clip: clip.name.clone(),
                    index: bad,
                    frames: self.frames.len(),
                });
            }
        }
        if let Some(nine) = self.nine {
            // Against the *first* frame: every frame in a sheet is the same size
            // (an animation of differently sized frames is not a thing), and
            // `.pix` enforces that, so one check covers them all.
            let frame = &self.frames[0];
            if !nine.fits_in(frame.rect) {
                return Err(SheetError::NineTooBig {
                    nine,
                    frame: frame.rect,
                });
            }
        }
        Ok(())
    }
}

/// Why a sheet is not usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetError {
    NoFrames,
    EmptyFrame {
        frame: String,
    },
    FrameOutsideSheet {
        frame: String,
        rect: Rect,
        width: u32,
        height: u32,
    },
    ZeroHold {
        frame: String,
    },
    DuplicateFrame {
        frame: String,
    },
    EmptyClip {
        clip: String,
    },
    ClipFrameOutOfRange {
        clip: String,
        index: usize,
        frames: usize,
    },
    NineTooBig {
        nine: NineSlice,
        frame: Rect,
    },
}

impl fmt::Display for SheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrames => write!(f, "a sheet with no frames draws nothing"),
            Self::EmptyFrame { frame } => {
                write!(f, "frame `{frame}` has no area")
            }
            Self::FrameOutsideSheet {
                frame,
                rect,
                width,
                height,
            } => write!(
                f,
                "frame `{frame}` at {}x{}+{}+{} runs outside a {width}x{height} sheet; \
                 it would sample whatever is next to it",
                rect.w, rect.h, rect.x, rect.y
            ),
            Self::ZeroHold { frame } => write!(
                f,
                "frame `{frame}` is held for zero ticks, so a clip containing it \
                 would advance forever without drawing"
            ),
            Self::DuplicateFrame { frame } => {
                write!(f, "two frames are both called `{frame}`")
            }
            Self::EmptyClip { clip } => write!(f, "clip `{clip}` names no frames"),
            Self::ClipFrameOutOfRange {
                clip,
                index,
                frames,
            } => write!(
                f,
                "clip `{clip}` names frame {index}, but the sheet has {frames}"
            ),
            Self::NineTooBig { nine, frame } => write!(
                f,
                "nine-slice insets {}+{} wide and {}+{} tall do not fit a {}x{} frame",
                nine.left, nine.right, nine.top, nine.bottom, frame.w, frame.h
            ),
        }
    }
}

impl core::error::Error for SheetError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Sheet {
        Sheet {
            width: 32,
            height: 16,
            frames: vec![
                Frame {
                    name: "a".into(),
                    rect: Rect::new(0, 0, 16, 16),
                    hold: 4,
                },
                Frame {
                    name: "b".into(),
                    rect: Rect::new(16, 0, 16, 16),
                    hold: 4,
                },
            ],
            clips: vec![Clip {
                name: "walk".into(),
                frames: vec![0, 1],
                direction: Direction::Forward,
                looping: true,
            }],
            nine: None,
            sample: SampleMode::Pixel,
        }
    }

    #[test]
    fn a_well_formed_sheet_validates_and_can_be_looked_up_by_name() {
        let sheet = sheet();
        sheet.validate().expect("this sheet is well formed");
        assert_eq!(
            sheet.frame("b").map(|f| f.rect),
            Some(Rect::new(16, 0, 16, 16))
        );
        assert_eq!(sheet.frame_index("b"), Some(1));
        assert_eq!(sheet.frame("nope"), None);
        assert_eq!(sheet.clip("walk").map(|c| c.frames.len()), Some(2));
    }

    /// **The rule this crate exists to enforce.** A frame that runs past the
    /// edge of its image samples whatever is next to it, which renders as a
    /// sliver of the neighbouring sprite along one edge.
    #[test]
    fn a_frame_that_runs_off_the_sheet_is_refused() {
        let mut sheet = sheet();
        sheet.frames[1].rect = Rect::new(20, 0, 16, 16);
        let error = sheet.validate().expect_err("this frame runs off the right");
        assert!(matches!(error, SheetError::FrameOutsideSheet { .. }));
        assert!(error.to_string().contains("outside"), "{error}");
    }

    /// And it must not be possible to arrive at "fits" by wrapping.
    #[test]
    fn a_frame_rect_cannot_wrap_its_way_inside_the_sheet() {
        assert!(!Rect::new(u32::MAX - 1, 0, 4, 4).fits_in(64, 64));
        assert!(!Rect::new(0, u32::MAX, 4, 4).fits_in(64, 64));
        // The case the obvious `x <= width - w` formulation gets wrong: a frame
        // wider than the whole sheet underflows the subtraction, which panics in
        // debug and answers "fits" in release.
        assert!(!Rect::new(0, 0, 128, 4).fits_in(64, 64));
        assert!(!Rect::new(0, 0, 4, 128).fits_in(64, 64));
        assert!(
            Rect::new(60, 60, 4, 4).fits_in(64, 64),
            "flush to the edge fits"
        );
    }

    #[test]
    fn the_other_rules_each_have_a_message() {
        let mut empty = sheet();
        empty.frames.clear();
        assert_eq!(empty.validate(), Err(SheetError::NoFrames));

        let mut zero = sheet();
        zero.frames[0].hold = 0;
        assert!(matches!(zero.validate(), Err(SheetError::ZeroHold { .. })));

        let mut duplicate = sheet();
        duplicate.frames[1].name = "a".into();
        assert!(matches!(
            duplicate.validate(),
            Err(SheetError::DuplicateFrame { .. })
        ));

        let mut dangling = sheet();
        dangling.clips[0].frames = vec![0, 7];
        assert!(matches!(
            dangling.validate(),
            Err(SheetError::ClipFrameOutOfRange { .. })
        ));

        let mut hollow = sheet();
        hollow.clips[0].frames.clear();
        assert!(matches!(
            hollow.validate(),
            Err(SheetError::EmptyClip { .. })
        ));

        let mut flat = sheet();
        flat.frames[0].rect = Rect::new(0, 0, 0, 16);
        assert!(matches!(
            flat.validate(),
            Err(SheetError::EmptyFrame { .. })
        ));
    }

    /// Overlapping insets would give the centre a negative size, and the
    /// geometry builder would emit inside-out quads that draw nothing — a
    /// sprite that vanishes at one size and not another.
    #[test]
    fn nine_slice_insets_that_overlap_are_refused() {
        let frame = Rect::new(0, 0, 16, 16);
        assert!(
            NineSlice::new(8, 8, 8, 8).fits_in(frame),
            "exactly touching is fine"
        );
        assert!(!NineSlice::new(9, 8, 0, 0).fits_in(frame));
        assert!(!NineSlice::new(0, 0, 12, 12).fits_in(frame));
        assert!(
            !NineSlice::new(u32::MAX, 1, 0, 0).fits_in(frame),
            "and must not wrap"
        );

        let mut sheet = sheet();
        sheet.nine = Some(NineSlice::new(9, 9, 0, 0));
        assert!(matches!(
            sheet.validate(),
            Err(SheetError::NineTooBig { .. })
        ));
    }

    /// Aseprite stores the middle of the nine; we store the four insets. The
    /// conversion is the one place that could silently transpose them.
    #[test]
    fn asepite_centre_rects_convert_to_the_insets_they_describe() {
        let frame = Rect::new(0, 0, 32, 16);
        // A centre 4 in from the left, 6 from the top, 20 wide and 4 tall
        // leaves 8 on the right and 6 on the bottom.
        let nine = NineSlice::from_center(frame, Rect::new(4, 6, 20, 4))
            .expect("the centre is inside the frame");
        assert_eq!(nine, NineSlice::new(4, 8, 6, 6));
        assert_eq!(nine.minimum_size(), (12, 12));

        // A centre that runs outside the frame is a malformed export.
        assert_eq!(NineSlice::from_center(frame, Rect::new(30, 0, 8, 4)), None);
        assert_eq!(NineSlice::from_center(frame, Rect::new(0, 14, 4, 8)), None);
    }

    // -----------------------------------------------------------------------
    // Clip playback
    // -----------------------------------------------------------------------

    /// A four-frame strip. The holds are **unequal on purpose**: with every
    /// hold the same, "showing 3" and "tick 3" are the same number and every
    /// confusion between them passes.
    fn strip(holds: [u32; 4]) -> Sheet {
        let frame = |name: &str, x: u32, hold: u32| Frame {
            name: name.into(),
            rect: Rect::new(x, 0, 16, 16),
            hold,
        };
        Sheet {
            width: 64,
            height: 16,
            frames: vec![
                frame("a", 0, holds[0]),
                frame("b", 16, holds[1]),
                frame("c", 32, holds[2]),
                frame("d", 48, holds[3]),
            ],
            ..Sheet::default()
        }
    }

    fn clip(frames: Vec<usize>, direction: Direction, looping: bool) -> Clip {
        Clip {
            name: "clip".into(),
            frames,
            direction,
            looping,
        }
    }

    /// The frame drawn on each of the first `ticks` ticks, in order.
    fn drawn(sheet: &Sheet, clip: &Clip, ticks: u64) -> Vec<usize> {
        let mut play = Playback::new();
        (0..ticks)
            .map(|_| {
                let frame = play.frame_index(sheet, clip).expect("the clip draws");
                play.advance(1);
                frame
            })
            .collect()
    }

    /// The showings of one pass, ignoring how long each is held.
    fn showings(clip: &Clip) -> Vec<usize> {
        (0..clip.steps())
            .map(|step| clip.step(step).expect("inside the pass"))
            .collect()
    }

    /// An independent model of the same rule: a cursor and a countdown, walked
    /// one tick at a time with no modular arithmetic anywhere. This is what
    /// [`Playback::frame_index`]'s closed form is checked against — the two
    /// agreeing is the thing that catches an off-by-one in either.
    fn naive(sheet: &Sheet, clip: &Clip, ticks: u64) -> usize {
        let steps = clip.steps();
        let mut step = 0;
        let mut held = 0;
        for _ in 0..ticks {
            let frame = clip.step(step).expect("inside the pass");
            let hold = sheet.frames[frame].hold;
            held += 1;
            if held < hold {
                continue;
            }
            held = 0;
            if step + 1 < steps {
                step += 1;
            } else if clip.looping {
                step = 0;
            } else {
                held = hold; // parked on the last showing, forever
            }
        }
        clip.step(step).expect("inside the pass")
    }

    /// Every shape of clip worth running a property over.
    fn cases() -> Vec<(Sheet, Clip)> {
        let uneven = strip([2, 1, 3, 1]);
        let flat = strip([1, 1, 1, 1]);
        let mut out = Vec::new();
        for direction in [Direction::Forward, Direction::Reverse, Direction::PingPong] {
            for looping in [true, false] {
                for sheet in [uneven.clone(), flat.clone()] {
                    out.push((sheet.clone(), clip(vec![0, 1, 2, 3], direction, looping)));
                    out.push((sheet.clone(), clip(vec![2], direction, looping)));
                    out.push((sheet.clone(), clip(vec![1, 3], direction, looping)));
                    // A clip may name a frame more than once.
                    out.push((sheet, clip(vec![0, 2, 0, 3], direction, looping)));
                }
            }
        }
        out
    }

    /// Forward is the plain case, and it is still worth writing the whole
    /// sequence out: an implementation that never advances satisfies every
    /// invariant an "is it in range" assertion can express.
    #[test]
    fn forward_holds_each_frame_for_its_own_ticks_and_wraps() {
        let sheet = strip([2, 1, 3, 1]);
        let walk = clip(vec![0, 1, 2, 3], Direction::Forward, true);
        assert_eq!(showings(&walk), vec![0, 1, 2, 3]);
        assert_eq!(sheet.clip_duration(&walk), Some(7), "2 + 1 + 3 + 1");
        assert_eq!(
            drawn(&sheet, &walk, 16),
            vec![0, 0, 1, 2, 2, 2, 3, 0, 0, 1, 2, 2, 2, 3, 0, 0],
            "two whole cycles and a bit"
        );

        // Equal holds are the case that hides index/tick confusion, so pin it
        // separately rather than trusting the one above to cover it.
        let flat = strip([1, 1, 1, 1]);
        assert_eq!(flat.clip_duration(&walk), Some(4));
        assert_eq!(drawn(&flat, &walk, 9), vec![0, 1, 2, 3, 0, 1, 2, 3, 0]);
    }

    /// **Holds belong to frames, not to positions.** Reversing the clip
    /// reverses the tick sequence exactly — which is only visible at all when
    /// the holds differ.
    #[test]
    fn reverse_runs_the_frames_backwards_and_takes_their_holds_with_them() {
        let sheet = strip([2, 1, 3, 1]);
        let back = clip(vec![0, 1, 2, 3], Direction::Reverse, true);
        assert_eq!(showings(&back), vec![3, 2, 1, 0]);
        assert_eq!(
            sheet.clip_duration(&back),
            Some(7),
            "the same holds, so the same total"
        );
        assert_eq!(
            drawn(&sheet, &back, 14),
            vec![3, 2, 2, 2, 1, 0, 0, 3, 2, 2, 2, 1, 0, 0],
            "`d` is held 1 tick and `c` 3, exactly as forward holds them"
        );

        // The same claim said the other way: one reversed cycle is one forward
        // cycle read backwards. It would fail if holds stayed with positions.
        let forward = clip(vec![0, 1, 2, 3], Direction::Forward, true);
        let mut mirrored = drawn(&sheet, &forward, 7);
        mirrored.reverse();
        assert_eq!(drawn(&sheet, &back, 7), mirrored);
    }

    /// **The choice: each end is shown once, not twice.** A wing does not pause
    /// at the top of its stroke. Four frames run `0 1 2 3 2 1` — six showings,
    /// `2n - 2`, not the eight a repeated-ends implementation would give.
    #[test]
    fn ping_pong_shows_each_end_once_so_its_period_is_two_n_minus_two() {
        let sheet = strip([2, 1, 3, 1]);
        let flap = clip(vec![0, 1, 2, 3], Direction::PingPong, true);

        assert_eq!(flap.steps(), 6, "2n - 2 for n = 4, not 2n");
        assert_eq!(showings(&flap), vec![0, 1, 2, 3, 2, 1]);
        assert_eq!(
            sheet.clip_duration(&flap),
            Some(11),
            "2 + 1 + 3 + 1 + 3 + 1; repeating the ends would be 14"
        );

        let cycle = vec![0, 0, 1, 2, 2, 2, 3, 2, 2, 2, 1];
        let mut twice = cycle.clone();
        twice.extend_from_slice(&cycle);
        assert_eq!(drawn(&sheet, &flap, 22), twice);

        // And the period really is the period, out where drift would show.
        let mut here = Playback::new();
        let mut later = Playback::new();
        later.advance(11 * 9);
        for _ in 0..40 {
            assert_eq!(
                here.frame_index(&sheet, &flap),
                later.frame_index(&sheet, &flap),
                "at tick {}",
                here.elapsed()
            );
            here.advance(1);
            later.advance(1);
        }

        // Anti-vacuity: the sequence has to actually move.
        assert!(
            cycle.windows(2).any(|pair| pair[0] != pair[1]),
            "a playback that never advances would satisfy a weaker assertion"
        );
    }

    /// A one-shot ping-pong plays the return home. The trailing `0` is the next
    /// cycle's first showing when the clip loops, so it is not counted twice —
    /// but a clip that stops has to show it, or it ends on frame 1.
    #[test]
    fn a_one_shot_ping_pong_returns_to_its_first_frame_and_parks() {
        let sheet = strip([2, 1, 3, 1]);
        let once = clip(vec![0, 1, 2, 3], Direction::PingPong, false);
        assert_eq!(once.steps(), 7, "2n - 1: the walk home is not a repeat");
        assert_eq!(showings(&once), vec![0, 1, 2, 3, 2, 1, 0]);
        assert_eq!(sheet.clip_duration(&once), Some(13), "11 + `a`'s two ticks");
        assert_eq!(
            drawn(&sheet, &once, 17),
            vec![0, 0, 1, 2, 2, 2, 3, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0]
        );
    }

    /// A one-shot has to say it is **over**, distinguishably from being on its
    /// last frame — a game acts when the explosion ends, not when it starts its
    /// final puff.
    #[test]
    fn a_one_shot_holds_its_last_frame_and_reports_finished_after_it() {
        let sheet = strip([2, 1, 3, 1]);
        let once = clip(vec![0, 1, 2, 3], Direction::Forward, false);
        assert_eq!(sheet.clip_duration(&once), Some(7));
        assert_eq!(
            drawn(&sheet, &once, 12),
            vec![0, 0, 1, 2, 2, 2, 3, 3, 3, 3, 3, 3],
            "and then `d` forever"
        );

        let mut play = Playback::new();
        play.advance(6);
        assert_eq!(
            play.frame_index(&sheet, &once),
            Some(3),
            "the last frame, on the first tick it is drawn"
        );
        assert!(
            !play.finished(&sheet, &once),
            "on the last frame is not the same as done with it"
        );
        play.advance(1);
        assert_eq!(play.frame_index(&sheet, &once), Some(3));
        assert!(play.finished(&sheet, &once), "after its hold, it is over");
        play.advance(1_000);
        assert!(play.finished(&sheet, &once));
        assert_eq!(
            play.frame_index(&sheet, &once),
            Some(3),
            "a caller ignoring `finished` still draws something"
        );

        // A looping clip never ends, however far it runs.
        let forever = clip(vec![0, 1, 2, 3], Direction::Forward, true);
        assert!(!play.finished(&sheet, &forever));
    }

    /// One frame is where `2n - 2` is zero. It must be a period of one, not a
    /// clip that draws nothing or divides by zero.
    #[test]
    fn a_single_frame_clip_is_one_showing_in_every_direction() {
        let sheet = strip([2, 1, 3, 1]);
        for direction in [Direction::Forward, Direction::Reverse, Direction::PingPong] {
            for looping in [true, false] {
                let lone = clip(vec![2], direction, looping);
                assert_eq!(lone.steps(), 1, "{direction:?} looping={looping}");
                assert_eq!(showings(&lone), vec![2]);
                assert_eq!(lone.step(1), None);
                assert_eq!(
                    sheet.clip_duration(&lone),
                    Some(3),
                    "`c`'s hold, counted once: {direction:?}"
                );
                assert_eq!(
                    drawn(&sheet, &lone, 8),
                    vec![2; 8],
                    "{direction:?} looping={looping}"
                );
                let mut play = Playback::new();
                play.advance(3);
                assert_eq!(play.finished(&sheet, &lone), !looping);
            }
        }
    }

    /// **The property that catches every off-by-one in the modular
    /// arithmetic.** A closed form over `elapsed` and a cursor walked one tick
    /// at a time have to agree at every tick, for every shape of clip.
    #[test]
    fn the_closed_form_agrees_with_a_tick_by_tick_walk() {
        for (sheet, clip) in cases() {
            for ticks in 0..200 {
                let mut play = Playback::new();
                play.advance(ticks);
                assert_eq!(
                    play.frame_index(&sheet, &clip),
                    Some(naive(&sheet, &clip, ticks)),
                    "{:?} looping={} frames={:?} at tick {ticks}",
                    clip.direction,
                    clip.looping,
                    clip.frames,
                );
            }
        }
    }

    /// Catching up after a stall must land where never stalling would.
    #[test]
    fn advancing_many_ticks_at_once_lands_where_one_at_a_time_does() {
        for (sheet, clip) in cases() {
            let mut stepped = Playback::new();
            for ticks in 0..300u64 {
                let mut jumped = Playback::new();
                jumped.advance(ticks);
                assert_eq!(
                    jumped.frame_index(&sheet, &clip),
                    stepped.frame_index(&sheet, &clip),
                    "{:?} looping={} frames={:?} at tick {ticks}",
                    clip.direction,
                    clip.looping,
                    clip.frames,
                );
                assert_eq!(jumped.elapsed(), stepped.elapsed());
                // And in two hops, which is where a wrap-then-wrap would show.
                let mut split = Playback::new();
                split.advance(ticks / 3);
                split.advance(ticks - ticks / 3);
                assert_eq!(
                    split.frame_index(&sheet, &clip),
                    stepped.frame_index(&sheet, &clip)
                );
                stepped.advance(1);
            }
        }
    }

    #[test]
    fn restarting_and_seeking_put_playback_where_asked() {
        let sheet = strip([2, 1, 3, 1]);
        let walk = clip(vec![0, 1, 2, 3], Direction::Forward, false);
        let mut play = Playback::new();
        play.advance(5);
        assert_eq!(play.elapsed(), 5);
        assert_eq!(play.frame_index(&sheet, &walk), Some(2));
        play.restart();
        assert_eq!(play.elapsed(), 0);
        assert_eq!(play.frame_index(&sheet, &walk), Some(0));
        play.seek(6);
        assert_eq!(play.frame_index(&sheet, &walk), Some(3));
        assert!(!play.finished(&sheet, &walk));
        play.seek(u64::MAX);
        play.advance(9);
        assert_eq!(play.elapsed(), u64::MAX, "saturates rather than wrapping");
        assert!(play.finished(&sheet, &walk));
    }

    /// A clip with nothing in it, or one naming a frame that is not there,
    /// draws nothing rather than panicking. `validate` refuses both.
    #[test]
    fn a_clip_that_cannot_draw_answers_none() {
        let sheet = strip([2, 1, 3, 1]);
        let empty = clip(vec![], Direction::PingPong, true);
        assert_eq!(empty.steps(), 0);
        assert_eq!(empty.step(0), None);
        assert_eq!(sheet.clip_duration(&empty), None);
        assert_eq!(Playback::new().frame_index(&sheet, &empty), None);

        let dangling = clip(vec![0, 9], Direction::Forward, true);
        assert_eq!(Playback::new().frame_index(&sheet, &dangling), None);
    }

    /// The renderer wants `[u0, v0, u1, v1]` with the top-left corner first,
    /// and every caller was computing it by hand from a rect and the sheet
    /// size. Halves swapped here would mirror every sprite in the game.
    #[test]
    fn frame_uvs_are_normalised_with_the_top_left_corner_first() {
        let sheet = Sheet {
            width: 64,
            height: 32,
            frames: vec![
                Frame {
                    name: "left".into(),
                    rect: Rect::new(0, 0, 16, 8),
                    hold: 1,
                },
                Frame {
                    name: "far".into(),
                    rect: Rect::new(48, 24, 16, 8),
                    hold: 1,
                },
            ],
            ..Sheet::default()
        };
        sheet.validate().expect("well formed");
        assert_eq!(sheet.uv(0), Some([0.0, 0.0, 0.25, 0.25]));
        assert_eq!(sheet.uv(1), Some([0.75, 0.75, 1.0, 1.0]));
        assert_eq!(sheet.uv(2), None, "no such frame");

        // A sheet with no size would divide by zero and hand a NaN to a vertex
        // buffer, which draws nothing and blames the shader.
        let sizeless = Sheet {
            width: 0,
            ..sheet.clone()
        };
        assert_eq!(sizeless.uv(0), None);
    }
}
