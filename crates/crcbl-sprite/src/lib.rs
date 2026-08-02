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
}
