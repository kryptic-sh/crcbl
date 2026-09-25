//! Draw list: a sequence of UI draw commands queued for rendering.
//!
//! Each frame the UI code produces a [`DrawList`] — an ordered list of
//! commands (rectangles, pictures, text spans) that a render backend then
//! processes into GPU draw calls. The draw list is the only interface between
//! the immediate-mode UI and the renderer.
//!
//! # Every primitive is one kind of quad
//!
//! Solid rectangles, strokes, bitmap glyphs, [`glyph runs`](DrawList::glyphs)
//! from a parsed font, [`image`](DrawList::image) quads and
//! [`rounded rectangles`](DrawList::rounded_rect) all expand to the same
//! [`Vertex2d`], which says which of them it belongs to in
//! [`Vertex2d::shape`]. `crcbl-render`'s UI pass draws the whole list with one
//! pipeline and one draw a half, so a menu frame, the text on it and a rounded
//! panel beside it never break a batch.
//!
//! # Clip rectangles travel on the vertex
//!
//! [`push_clip`](DrawList::push_clip) narrows everything pushed after it until
//! the matching [`pop_clip`](DrawList::pop_clip), and every vertex a command
//! expands to carries the clip that was current when the command went in. The
//! fragment stage discards outside it. No GPU scissor and no stencil: a scissor
//! is per draw, so a list that clipped two panels differently would be two
//! draws, and the UI's rendering rules (`docs/notes/tooling.md`) keep batching
//! as the reason.

use crate::font::Font;
use crate::font::atlas::{GlyphAtlas, SUBPIXEL_BINS};
use crate::font::layout::PositionedGlyph;
use crate::image::{AtlasImage, NineSliceImage, slice_bands, slice_cuts};
use crate::text::FontAtlas;
use crate::text::GLYPH_HEIGHT;
use crate::widget::SkinInsets;
use core::fmt;
use glam::Vec2;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Vertex
// ---------------------------------------------------------------------------

/// A 2D vertex for UI rendering (screen-space, no Z).
///
/// Six lanes, mirrored field for field by `Vertex` in
/// `crates/crcbl-shaders/shaders/ui.slang` and read out of one storage buffer
/// by `crcbl_render::ui_pass`. The first three are what every primitive has
/// always had; the last three are zero for a primitive that does not use them,
/// except [`clip`](Self::clip), which is [`ClipRect::NONE`] when nothing is
/// clipped.
///
/// # Safety
///
/// The fields are all f32 values with no padding, making the struct safe to
/// transmute to/from byte slices.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vertex2d {
    /// Position in screen-space pixels.
    pub pos: Vec2,
    /// UV into the bitmap font for [`Primitive::Glyph`], into a glyph page for
    /// [`Primitive::FontGlyph`] and into the image atlas for
    /// [`Primitive::Image`]; zero for untextured primitives.
    ///
    /// For [`Primitive::RoundedRect`] it is not a UV at all: it is this vertex's
    /// offset from the rectangle's centre in pixels, which the fragment stage
    /// receives interpolated and measures the rectangle's distance field at.
    pub uv: Vec2,
    /// RGBA colour, each component in `[0, 1]`: the fill, the text colour, or
    /// the tint an image is multiplied by.
    pub color: [f32; 4],
    /// The clip rectangle, `[min.x, min.y, max.x, max.y]` in screen pixels.
    pub clip: [f32; 4],
    /// `[half width, half height, border width, primitive]`: the rounded
    /// rectangle's half extent and border in pixels, and the
    /// [`Primitive`] as a float in the last lane, for every vertex. A
    /// [`Primitive::FontGlyph`] carries its glyph page in the first lane
    /// instead.
    pub shape: [f32; 4],
    /// The rounded rectangle's corner radii in pixels: top-left, top-right,
    /// bottom-right, bottom-left.
    pub radii: [f32; 4],
    /// The rounded rectangle's border colour.
    pub border: [f32; 4],
}

impl Vertex2d {
    /// A vertex of `primitive` with nothing clipped and no rounded-rectangle
    /// lanes — what every primitive but the rounded rectangle is.
    #[must_use]
    pub const fn new(pos: Vec2, uv: Vec2, color: [f32; 4], primitive: Primitive) -> Self {
        Self {
            pos,
            uv,
            color,
            clip: ClipRect::NONE.lane(),
            shape: [0.0, 0.0, 0.0, primitive.lane()],
            radii: [0.0; 4],
            border: [0.0; 4],
        }
    }

    /// Which primitive this vertex belongs to, or `None` for a lane holding no
    /// primitive this crate emits.
    #[must_use]
    pub fn primitive(&self) -> Option<Primitive> {
        Primitive::ALL
            .into_iter()
            .find(|primitive| primitive.lane() == self.shape[3])
    }
}

/// What the fragment stage does with a vertex's lanes.
///
/// Carried as a float in [`Vertex2d::shape`]'s last lane; `ui.slang` spells the
/// same numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    /// The vertex colour, as it is.
    Solid,
    /// The vertex colour, its alpha multiplied by the glyph atlas.
    Glyph,
    /// The image atlas, sampled sharp-bilinear and multiplied by the colour.
    Image,
    /// The analytic rounded rectangle: fill, border and corners evaluated as a
    /// signed distance per fragment.
    RoundedRect,
    /// The vertex colour, its alpha multiplied by a [`GlyphAtlas`] page's
    /// coverage — the page in [`Vertex2d::shape`]'s first lane.
    FontGlyph,
}

impl Primitive {
    /// Every primitive, in lane order.
    pub const ALL: [Self; 5] = [
        Self::Solid,
        Self::Glyph,
        Self::Image,
        Self::RoundedRect,
        Self::FontGlyph,
    ];

    /// The value [`Vertex2d::shape`]'s last lane holds for this primitive.
    #[must_use]
    pub const fn lane(self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Glyph => 1.0,
            Self::Image => 2.0,
            Self::RoundedRect => 3.0,
            Self::FontGlyph => 4.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Clip rectangles, corner radii, borders
// ---------------------------------------------------------------------------

/// A screen-space rectangle nothing is drawn outside of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRect {
    /// Top-left corner, in screen pixels.
    pub min: Vec2,
    /// Bottom-right corner, in screen pixels.
    pub max: Vec2,
}

impl ClipRect {
    /// No clip: the whole range of `f32`, which every fragment is inside.
    pub const NONE: Self = Self {
        min: Vec2::splat(f32::MIN),
        max: Vec2::splat(f32::MAX),
    };

    /// The part of `self` that is also inside `other`.
    ///
    /// Two rectangles that do not overlap give an empty one — `max` pulled back
    /// to `min` — which clips away everything rather than turning inside out.
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        let min = self.min.max(other.min);
        let max = self.max.min(other.max).max(min);
        Self { min, max }
    }

    /// The four floats [`Vertex2d::clip`] carries.
    #[must_use]
    pub const fn lane(self) -> [f32; 4] {
        [self.min.x, self.min.y, self.max.x, self.max.y]
    }
}

/// [`DrawList::pop_clip`] was called with no clip pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipUnderflow;

impl fmt::Display for ClipUnderflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pop_clip was called with no clip rectangle pushed")
    }
}

impl std::error::Error for ClipUnderflow {}

/// A rounded rectangle's four corner radii, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadii {
    /// The top-left corner.
    pub top_left: f32,
    /// The top-right corner.
    pub top_right: f32,
    /// The bottom-right corner.
    pub bottom_right: f32,
    /// The bottom-left corner.
    pub bottom_left: f32,
}

impl CornerRadii {
    /// The same radius at every corner.
    #[must_use]
    pub const fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// The radii as [`Vertex2d::radii`] carries them, each clamped into
    /// `0..=limit` — so no corner is rounder than half the shorter side, and a
    /// negative or NaN radius is a square corner.
    #[must_use]
    pub fn lane(self, limit: f32) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .map(|radius| radius.max(0.0).min(limit))
    }
}

/// A rounded rectangle's border: drawn inside its edge, following its corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// Thickness in pixels.
    pub width: f32,
    /// RGBA colour.
    pub color: [f32; 4],
}

impl Border {
    /// No border at all.
    pub const NONE: Self = Self {
        width: 0.0,
        color: [0.0; 4],
    };
}

// ---------------------------------------------------------------------------
// Draw command
// ---------------------------------------------------------------------------

/// A single draw command in a [`DrawList`].
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// A filled rectangle.
    Rect {
        /// Top-left corner in screen-space (Y grows downwards).
        min: Vec2,
        /// Bottom-right corner in screen-space (Y grows downwards).
        max: Vec2,
        /// RGBA fill colour.
        color: [f32; 4],
    },
    /// A rectangle outline (border), drawn inside the declared bounds.
    RectOutline {
        /// Top-left corner in screen-space.
        min: Vec2,
        /// Bottom-right corner in screen-space.
        max: Vec2,
        /// Line thickness in pixels. Clamped to half the smaller extent, so an
        /// over-thick border becomes a filled rect rather than self-intersecting
        /// geometry.
        thickness: f32,
        /// RGBA border colour.
        color: [f32; 4],
    },
    /// A straight segment stroked to a given thickness.
    Line {
        /// One end, in screen-space.
        from: Vec2,
        /// The other end, in screen-space.
        to: Vec2,
        /// Stroke width in pixels, centred on the segment.
        thickness: f32,
        /// RGBA stroke colour.
        color: [f32; 4],
    },
    /// A connected run of segments stroked to a given thickness.
    ///
    /// A point that is not finite **breaks** the run rather than being
    /// dropped: joining its neighbours would draw a chord that is not in the
    /// caller's data, which is exactly the artefact a diverging simulation
    /// would hide behind. A broken run is never closed.
    Polyline {
        /// The vertices, in order and in screen-space.
        points: Vec<Vec2>,
        /// Stroke width in pixels, centred on the run.
        thickness: f32,
        /// Whether to stroke the closing segment from the last point back to
        /// the first, joining the seam like any other corner.
        closed: bool,
        /// RGBA stroke colour.
        color: [f32; 4],
    },
    /// A single line of text rendered from the glyph atlas.
    Text {
        /// Top-left anchor of the text's em box — *not* a baseline. The first
        /// line's glyphs occupy `pos.y ..= pos.y + LINE_HEIGHT * scale`.
        pos: Vec2,
        /// The text content.
        text: String,
        /// Text colour.
        color: [f32; 4],
        /// Font size in pixels (height of the em-square).
        size: f32,
    },
    /// A run of glyphs from a parsed font, already laid out — what
    /// [`crate::font::layout::TextLayout`] produces.
    ///
    /// Each glyph is drawn at `origin + offset × scale` with its pen snapped:
    /// the baseline to a whole pixel, the x to a whole pixel plus one of the
    /// glyph atlas's subpixel bins. Its mask is drawn one texel to one pixel.
    Glyphs {
        /// The top-left every glyph's offset is measured from.
        origin: Vec2,
        /// The font.
        font: &'static Font,
        /// Pixels per em.
        size: f32,
        /// Text colour.
        color: [f32; 4],
        /// The glyphs and their pen positions.
        glyphs: Vec<PositionedGlyph>,
        /// The text the glyphs spell, when they were laid out from text —
        /// what a UI tree span displayed, after any `text-overflow` cut — and
        /// `None` for a run built from glyph ids by hand. The renderer never
        /// reads it: it is how a test of a whole frame reads what a
        /// parsed-font run says, as it reads [`DrawCommand::Text`]'s `text`.
        text: Option<Arc<str>>,
    },
    /// A rectangle of the image atlas, stretched to a screen rectangle.
    ///
    /// The UVs are already the atlas's: [`DrawList::image`] and
    /// [`DrawList::nine_slice`] work them out from an
    /// [`AtlasImage`].
    Image {
        /// Top-left corner in screen-space.
        min: Vec2,
        /// Bottom-right corner in screen-space.
        max: Vec2,
        /// Atlas UV drawn at `min`.
        uv_min: Vec2,
        /// Atlas UV drawn at `max`.
        uv_max: Vec2,
        /// Straight-alpha RGBA the sampled texel is multiplied by.
        tint: [f32; 4],
    },
    /// A filled rectangle with rounded corners and an optional border,
    /// evaluated per fragment as a signed distance.
    RoundedRect {
        /// Top-left corner in screen-space.
        min: Vec2,
        /// Bottom-right corner in screen-space.
        max: Vec2,
        /// Corner radii in pixels, each clamped to half the shorter side.
        radii: CornerRadii,
        /// RGBA fill colour.
        color: [f32; 4],
        /// The border inside the edge. Its width is clamped the same way.
        border: Border,
    },
}

// ---------------------------------------------------------------------------
// DrawList
// ---------------------------------------------------------------------------

/// An ordered list of draw commands for one frame.
///
/// Create one per frame, push commands into it, then hand it to the renderer.
///
/// # Two layers, one list
///
/// The list is cut in two by [`begin_overlay`](DrawList::begin_overlay): the
/// commands before the cut are the game's HUD and GUI, the ones after it are
/// what has to stay on top of a menu — the menu itself, the debug overlay and
/// the console. The renderer draws the halves as two passes; a list nobody cut
/// is one layer and draws exactly as it used to.
#[derive(Debug, Clone, Default)]
pub struct DrawList {
    commands: Vec<DrawCommand>,
    /// The clip each command was pushed under, one per command.
    clips: Vec<ClipRect>,
    /// The clips pushed and not yet popped, each already intersected with the
    /// one beneath it. Empty means [`ClipRect::NONE`].
    clip_stack: Vec<ClipRect>,
    /// Where [`begin_overlay`](DrawList::begin_overlay) last cut the list.
    ///
    /// `None` on a list nobody cut, which puts every command below the cut.
    overlay_start: Option<usize>,
}

/// A draw list expanded to triangles, with the overlay cut carried through.
///
/// The return of [`DrawList::to_triangles_split`]. One expansion produces both
/// halves' geometry *and* the index the second half starts at, so a renderer
/// that draws them as two passes never triangulates the list twice.
#[derive(Debug, Clone, PartialEq)]
pub struct Triangles {
    /// Every vertex, in command order, shared by both halves.
    pub vertices: Vec<Vertex2d>,
    /// Every index, in command order.
    pub indices: Vec<u32>,
    /// Where the overlay's indices start: `indices[..overlay]` is the HUD half
    /// and `indices[overlay..]` is the overlay half. Equal to `indices.len()`
    /// on a list with no overlay.
    pub overlay: usize,
}

impl DrawList {
    /// Create an empty draw list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            clips: Vec::new(),
            clip_stack: Vec::new(),
            overlay_start: None,
        }
    }

    /// Appends one command under the current clip and overlay state.
    ///
    /// This supports adapters that transform commands from another list without
    /// losing primitive parameters. The source command's clip and overlay are
    /// not part of the command; restore them with [`Self::push_clip`] and
    /// [`Self::begin_overlay`] before appending it.
    pub fn push_command(&mut self, command: DrawCommand) {
        self.push(command);
    }

    fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
        self.clips.push(self.clip());
    }

    /// Push a filled rectangle command.
    pub fn rect(&mut self, min: Vec2, max: Vec2, color: [f32; 4]) {
        self.push(DrawCommand::Rect { min, max, color });
    }

    /// Push a rectangle outline command.
    pub fn rect_outline(&mut self, min: Vec2, max: Vec2, thickness: f32, color: [f32; 4]) {
        self.push(DrawCommand::RectOutline {
            min,
            max,
            thickness,
            color,
        });
    }

    /// Push a straight line segment, stroked centred on the segment.
    pub fn line(&mut self, from: Vec2, to: Vec2, thickness: f32, color: [f32; 4]) {
        self.push(DrawCommand::Line {
            from,
            to,
            thickness,
            color,
        });
    }

    /// Push a connected run of segments, stroked centred on the run.
    ///
    /// Corners are bevelled. Set `closed` to stroke the segment from the last
    /// point back to the first as well; see [`DrawCommand::Polyline`] for what
    /// a non-finite point does.
    pub fn polyline(
        &mut self,
        points: impl Into<Vec<Vec2>>,
        thickness: f32,
        closed: bool,
        color: [f32; 4],
    ) {
        self.push(DrawCommand::Polyline {
            points: points.into(),
            thickness,
            closed,
            color,
        });
    }

    /// Push a text command.
    pub fn text(&mut self, pos: Vec2, text: impl Into<String>, color: [f32; 4], size: f32) {
        self.push(DrawCommand::Text {
            pos,
            text: text.into(),
            color,
            size,
        });
    }

    /// Push a run of laid-out glyphs from `font`; see [`DrawCommand::Glyphs`].
    pub fn glyphs(
        &mut self,
        origin: Vec2,
        font: &'static Font,
        size: f32,
        color: [f32; 4],
        glyphs: impl Into<Vec<PositionedGlyph>>,
    ) {
        self.push(DrawCommand::Glyphs {
            origin,
            font,
            size,
            color,
            glyphs: glyphs.into(),
            text: None,
        });
    }

    /// [`DrawList::glyphs`] for a run laid out from `text`, which the command
    /// carries alongside its glyphs so the frame can be read back as text.
    pub fn text_glyphs(
        &mut self,
        origin: Vec2,
        font: &'static Font,
        size: f32,
        color: [f32; 4],
        glyphs: impl Into<Vec<PositionedGlyph>>,
        text: &str,
    ) {
        self.push(DrawCommand::Glyphs {
            origin,
            font,
            size,
            color,
            glyphs: glyphs.into(),
            text: Some(Arc::from(text)),
        });
    }

    /// Push a whole registered image, stretched to `min..max` and multiplied by
    /// `tint`.
    pub fn image(&mut self, min: Vec2, max: Vec2, image: &AtlasImage, tint: [f32; 4]) {
        self.push(DrawCommand::Image {
            min,
            max,
            uv_min: image.uv_min(),
            uv_max: image.uv_max(),
            tint,
        });
    }

    /// Push `sliced` drawn into `min..max` as a nine-slice: up to nine
    /// [`DrawCommand::Image`]s, corners fixed, edges and centre stretched.
    ///
    /// `scale` is screen pixels per texel of the fixed bands, so a four-texel
    /// corner at a scale of three is twelve pixels across. The stretched bands
    /// take whatever is left.
    ///
    /// # What comes out
    ///
    /// The quads in image order — top row left to right, then the middle, then
    /// the bottom — sharing every cut line as the same `f32`, so no seam can
    /// open between two bands. **An empty band emits nothing**: a three-slice
    /// with no top or bottom inset is three quads, not nine with six of them
    /// zero-sized.
    ///
    /// A target smaller than its corners squashes the corners in proportion
    /// rather than overlapping them; see [`slice_bands`]. A non-finite
    /// rectangle, or a `scale` that is not a positive finite number, draws
    /// nothing.
    pub fn nine_slice(
        &mut self,
        min: Vec2,
        max: Vec2,
        sliced: &NineSliceImage,
        scale: f32,
        tint: [f32; 4],
    ) {
        if !(scale.is_finite() && scale > 0.0) {
            return;
        }
        let insets = sliced.clamped_insets();
        let bands = SkinInsets::new(
            insets.left * scale,
            insets.right * scale,
            insets.top * scale,
            insets.bottom * scale,
        );
        self.nine_slice_bands(min, max, sliced, bands, true, tint);
    }

    /// Push `sliced` drawn into `min..max` with each fixed band `bands` pixels
    /// wide, rather than its insets at one scale: what a stylesheet's
    /// `border-image-width` asks for, whose sides need not agree.
    ///
    /// Cut as [`nine_slice`](Self::nine_slice) cuts, which is this with the
    /// insets times the scale; `fill` false leaves out the middle quad, as
    /// CSS's `border-image-slice` does without its `fill` keyword. A band whose
    /// inset is zero texels draws nothing whatever its width, and a non-finite
    /// rectangle or a band that is negative or not finite draws nothing at all.
    pub fn nine_slice_bands(
        &mut self,
        min: Vec2,
        max: Vec2,
        sliced: &NineSliceImage,
        bands: SkinInsets,
        fill: bool,
        tint: [f32; 4],
    ) {
        let widths = [bands.left, bands.right, bands.top, bands.bottom];
        if !(min.is_finite()
            && max.is_finite()
            && widths
                .iter()
                .all(|width| width.is_finite() && *width >= 0.0))
        {
            return;
        }
        let insets = sliced.clamped_insets();
        let size = sliced.image.size();
        let texel_x = [0.0, insets.left, size.x - insets.right, size.x];
        let texel_y = [0.0, insets.top, size.y - insets.bottom, size.y];
        let extent = max - min;
        let xs = slice_cuts(
            min.x,
            slice_bands(bands.left, bands.right, extent.x),
            extent.x,
        );
        let ys = slice_cuts(
            min.y,
            slice_bands(bands.top, bands.bottom, extent.y),
            extent.y,
        );
        for row in 0..3 {
            if texel_y[row] == texel_y[row + 1] || ys[row + 1] <= ys[row] {
                continue;
            }
            for column in 0..3 {
                if texel_x[column] == texel_x[column + 1]
                    || xs[column + 1] <= xs[column]
                    || (!fill && row == 1 && column == 1)
                {
                    continue;
                }
                self.push(DrawCommand::Image {
                    min: Vec2::new(xs[column], ys[row]),
                    max: Vec2::new(xs[column + 1], ys[row + 1]),
                    uv_min: sliced.image.uv(Vec2::new(texel_x[column], texel_y[row])),
                    uv_max: sliced
                        .image
                        .uv(Vec2::new(texel_x[column + 1], texel_y[row + 1])),
                    tint,
                });
            }
        }
    }

    /// Push a filled rectangle with rounded corners and an optional border.
    ///
    /// The corners are a signed distance evaluated per fragment, so they are
    /// smooth with multisampling off and cost no geometry: the command is one
    /// quad whatever its radii. See [`DrawCommand::RoundedRect`].
    pub fn rounded_rect(
        &mut self,
        min: Vec2,
        max: Vec2,
        radii: CornerRadii,
        color: [f32; 4],
        border: Border,
    ) {
        self.push(DrawCommand::RoundedRect {
            min,
            max,
            radii,
            color,
            border,
        });
    }

    /// Narrows the clip to its intersection with `min..max` until the matching
    /// [`pop_clip`](Self::pop_clip).
    ///
    /// Nested clips intersect: a panel inside a scroll view is clipped by both.
    pub fn push_clip(&mut self, min: Vec2, max: Vec2) {
        let clip = self.clip().intersect(ClipRect { min, max });
        self.clip_stack.push(clip);
    }

    /// Restores the clip that was current before the last
    /// [`push_clip`](Self::push_clip).
    ///
    /// # Errors
    ///
    /// [`ClipUnderflow`] when no clip is pushed, and nothing changes. An error
    /// rather than a panic, as everything else here that a mismatched caller
    /// can reach degrades rather than taking the frame down with it.
    pub fn pop_clip(&mut self) -> Result<(), ClipUnderflow> {
        self.clip_stack.pop().map(|_| ()).ok_or(ClipUnderflow)
    }

    /// The clip a command pushed now would be drawn under.
    #[must_use]
    pub fn clip(&self) -> ClipRect {
        self.clip_stack.last().copied().unwrap_or(ClipRect::NONE)
    }

    /// The clip each command was pushed under, one per
    /// [`commands`](Self::commands) entry.
    #[must_use]
    pub fn clips(&self) -> &[ClipRect] {
        &self.clips
    }

    /// Consume the draw list and return its commands.
    #[must_use]
    pub fn into_commands(self) -> Vec<DrawCommand> {
        self.commands
    }

    /// Borrow the commands.
    #[must_use]
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Cut the list here: everything pushed after this call is **overlay**.
    ///
    /// `crcbl-render`'s UI pass draws the two halves as two render passes, so a
    /// command pushed after this call paints over a pause menu's scrim and one
    /// pushed before it goes under. The engine calls this once a frame — after
    /// the game has drawn its HUD, before the menu, the debug overlay and the
    /// console go in.
    ///
    /// **The last call wins.** A second call moves the cut down to the new
    /// length rather than being refused, which is the only answer that keeps
    /// the engine's overlays on top: a game that marked a boundary of its own
    /// during `draw` marked an earlier one, and the engine's comes after it.
    ///
    /// **The cut also drops every pushed clip.** The overlay is the engine's,
    /// and a clip a game pushed and forgot to pop would otherwise cut the debug
    /// panel and the console down to that game's scroll view.
    ///
    /// [`clear`](Self::clear) drops the cut along with the commands.
    pub fn begin_overlay(&mut self) {
        self.overlay_start = Some(self.commands.len());
        self.clip_stack.clear();
    }

    /// The commands below the cut — the game's HUD and GUI.
    ///
    /// The whole list on a frame where
    /// [`begin_overlay`](Self::begin_overlay) was never called.
    #[must_use]
    pub fn base_commands(&self) -> &[DrawCommand] {
        &self.commands[..self.overlay_start()]
    }

    /// The commands above the cut — what must stay on top of a menu.
    ///
    /// Empty on a frame where [`begin_overlay`](Self::begin_overlay) was never
    /// called.
    #[must_use]
    pub fn overlay_commands(&self) -> &[DrawCommand] {
        &self.commands[self.overlay_start()..]
    }

    /// Where the overlay starts, clamped into the list.
    ///
    /// The clamp is what makes the two slicings above infallible rather than
    /// merely unreachable: `begin_overlay` records a length and the list only
    /// grows until it is cleared, and a cut that somehow outran the commands
    /// would lose the split rather than panic the frame.
    fn overlay_start(&self) -> usize {
        self.overlay_start
            .unwrap_or(self.commands.len())
            .min(self.commands.len())
    }

    /// Number of commands in the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clear all commands, every pushed clip and the overlay cut (reuse the
    /// allocation across frames).
    ///
    /// The cut goes with them: a frame that kept the previous frame's cut would
    /// put its first few commands above a menu for no reason anyone wrote down.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.clips.clear();
        self.clip_stack.clear();
        self.overlay_start = None;
    }

    /// Expand every draw command into screen-space triangles.
    ///
    /// [`to_triangles_split`](Self::to_triangles_split) without the overlay cut,
    /// for a caller that draws the whole list as one thing.
    #[must_use]
    pub fn to_triangles(
        &self,
        atlas: Option<&FontAtlas>,
        glyphs: Option<&mut GlyphAtlas>,
        scale: f32,
    ) -> (Vec<Vertex2d>, Vec<u32>) {
        let triangles = self.to_triangles_split(atlas, glyphs, scale);
        (triangles.vertices, triangles.indices)
    }

    /// Expand every draw command into screen-space triangles, both halves in
    /// one pass over the list.
    ///
    /// [`Triangles::overlay`] is where the indices of the commands pushed after
    /// [`begin_overlay`](Self::begin_overlay) start, so a renderer drawing the
    /// halves as two passes gets both ranges out of **one** expansion — running
    /// the tessellation twice would double the work and could disagree with
    /// itself about the glyph layout.
    ///
    /// The vertices and indices are in a format a render backend can upload
    /// directly. Each `Rect`, `Image` and `RoundedRect` becomes one quad (4
    /// vertices, 6 indices). `RectOutline` becomes 4 thin quads — one per side
    /// — forming a hollow border. `Line` and `Polyline` become one quad per
    /// segment plus one bevel triangle per corner. Every vertex carries the clip
    /// its command was pushed under.
    ///
    /// `Text` commands are expanded when `atlas` is `Some`: each glyph becomes
    /// one textured quad with UV coordinates into the atlas. When `atlas` is
    /// `None`, text commands are skipped (the `to_triangles` return from S6).
    ///
    /// `Glyphs` commands are expanded when `glyphs` is `Some`, each glyph
    /// looked up in — and on a miss rasterised into — that atlas, which is why
    /// it is taken mutably. A glyph the atlas defers this frame is left out,
    /// and a list with glyph runs should be expanded once a frame, after
    /// [`GlyphAtlas::begin_frame`]. When `glyphs` is `None` they are skipped.
    ///
    /// `scale` is a multiplier on the font size (1.0 = baked-in 8×13 px) and
    /// on a glyph run's size and offsets.
    ///
    /// The index buffer uses `u32` indices; callers that need `u16` must adapt.
    ///
    /// # Coordinate convention
    ///
    /// Vertex positions are in screen-space pixels, **Y-down**: `(0, 0)` is the
    /// top-left of the framebuffer and Y grows towards the bottom. This is the
    /// convention `shaders/ui.slang` implements
    /// (`ndc.y = 1.0 - (y / viewport.y) * 2.0`) and the one every widget in this
    /// crate lays out in, so `min` really is the visually-upper corner.
    ///
    /// Glyph UVs follow the same convention: `v = 0` is the atlas's top row and
    /// is emitted at the quad's `min.y` vertex, so glyphs render upright.
    #[must_use]
    pub fn to_triangles_split(
        &self,
        atlas: Option<&FontAtlas>,
        mut glyphs: Option<&mut GlyphAtlas>,
        scale: f32,
    ) -> Triangles {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let cut = self.overlay_start();
        let mut overlay = 0;
        for (index, (cmd, clip)) in self.commands.iter().zip(&self.clips).enumerate() {
            if index == cut {
                overlay = indices.len();
            }
            let first = vertices.len();
            expand(
                cmd,
                atlas,
                glyphs.as_deref_mut(),
                scale,
                &mut vertices,
                &mut indices,
            );
            for vertex in &mut vertices[first..] {
                vertex.clip = clip.lane();
            }
        }
        if cut == self.commands.len() {
            overlay = indices.len();
        }
        Triangles {
            vertices,
            indices,
            overlay,
        }
    }
}

/// Expand one draw command onto the end of `vertices` and `indices`.
///
/// The clip is not this function's: [`DrawList::to_triangles_split`] stamps it
/// onto whatever this pushed, so no primitive below can forget to carry it.
fn expand(
    cmd: &DrawCommand,
    atlas: Option<&FontAtlas>,
    glyph_atlas: Option<&mut GlyphAtlas>,
    scale: f32,
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    match cmd {
        DrawCommand::Rect { min, max, color } => {
            push_quad(
                *min,
                *max,
                (Vec2::ZERO, Vec2::ZERO),
                *color,
                Primitive::Solid,
                vertices,
                indices,
            );
        }
        DrawCommand::RectOutline {
            min,
            max,
            thickness,
            color,
        } => {
            // Four quads: top and bottom span the full width, left and
            // right fill the gap between them. Every corner is covered
            // exactly once, and both ends of each edge are built the
            // same way.
            //
            // The thickness is clamped to half the smaller extent: an
            // unclamped border would invert the inner rect and emit
            // self-intersecting bowties that paint over the whole box.
            let half = (*max - *min) * 0.5;
            let t = thickness.max(0.0).min(half.x.max(0.0)).min(half.y.max(0.0));
            let c = *color;

            let inner_min = Vec2::new(min.x + t, min.y + t);
            let inner_max = Vec2::new(max.x - t, max.y - t);

            let mut edge = |q_min: Vec2, q_max: Vec2| {
                push_quad(
                    q_min,
                    q_max,
                    (Vec2::ZERO, Vec2::ZERO),
                    c,
                    Primitive::Solid,
                    vertices,
                    indices,
                );
            };
            // top (full width, including both corners)
            edge(*min, Vec2::new(max.x, inner_min.y));
            // bottom (full width, including both corners)
            edge(Vec2::new(min.x, inner_max.y), *max);
            // left (between the two horizontal edges)
            edge(
                Vec2::new(min.x, inner_min.y),
                Vec2::new(inner_min.x, inner_max.y),
            );
            // right (between the two horizontal edges)
            edge(
                Vec2::new(inner_max.x, inner_min.y),
                Vec2::new(max.x, inner_max.y),
            );
        }
        DrawCommand::Line {
            from,
            to,
            thickness,
            color,
        } => {
            push_stroke(&[*from, *to], false, *thickness, *color, vertices, indices);
        }
        DrawCommand::Polyline {
            points,
            thickness,
            closed,
            color,
        } => {
            push_stroke(points, *closed, *thickness, *color, vertices, indices);
        }
        DrawCommand::Text {
            pos,
            text,
            color,
            size,
        } => {
            if let Some(atlas) = atlas {
                let layout_scale = (*size / GLYPH_HEIGHT as f32) * scale;
                let glyphs = atlas.glyph_positions(text, *pos, layout_scale);
                for (c, min, max) in glyphs {
                    // v = 0 is the atlas's top row, and `min.y` is the
                    // quad's top edge in the Y-down screen convention.
                    let uv_min = Vec2::new(atlas.glyph_u_min(c), 0.0);
                    let uv_max = Vec2::new(atlas.glyph_u_max(c), 1.0);
                    push_quad(
                        min,
                        max,
                        (uv_min, uv_max),
                        *color,
                        Primitive::Glyph,
                        vertices,
                        indices,
                    );
                }
            }
        }
        DrawCommand::Glyphs {
            origin,
            font,
            size,
            color,
            glyphs,
            text: _,
        } => {
            if let Some(glyph_atlas) = glyph_atlas {
                push_glyphs(
                    glyph_atlas,
                    (*origin, font, *size * scale, scale),
                    glyphs,
                    *color,
                    vertices,
                    indices,
                );
            }
        }
        DrawCommand::Image {
            min,
            max,
            uv_min,
            uv_max,
            tint,
        } => {
            push_quad(
                *min,
                *max,
                (*uv_min, *uv_max),
                *tint,
                Primitive::Image,
                vertices,
                indices,
            );
        }
        DrawCommand::RoundedRect {
            min,
            max,
            radii,
            color,
            border,
        } => push_rounded_rect(*min, *max, *radii, *color, *border, vertices, indices),
    }
}

/// Push one quad per inked glyph of a run, each mask one texel to one pixel.
///
/// `(origin, font, pixel_size, scale)` is the run: its top-left, its font, its
/// size already scaled, and the scale its offsets take. Each pen's x is
/// rounded to the nearest [`SUBPIXEL_BINS`]th of a pixel, split into a whole
/// pixel and a bin, and its baseline to a whole pixel, so the quad's corners
/// are whole pixels and a nearest sample at every fragment centre reads
/// exactly one texel.
fn push_glyphs(
    atlas: &mut GlyphAtlas,
    (origin, font, pixel_size, scale): (Vec2, &Font, f32, f32),
    glyphs: &[PositionedGlyph],
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let bins = f32::from(SUBPIXEL_BINS);
    let page = atlas.page_size() as f32;
    for glyph in glyphs {
        let pen = origin + glyph.offset * scale;
        if !pen.is_finite() {
            continue;
        }
        let steps = (pen.x * bins).round();
        let whole = (steps / bins).floor();
        let bin = (steps - whole * bins) as u8;
        let Some(placed) = atlas.glyph(font, glyph.glyph, pixel_size, bin) else {
            continue;
        };
        if placed.is_empty() {
            continue;
        }
        let min = Vec2::new(
            whole + placed.left as f32,
            pen.y.round() + placed.top as f32,
        );
        let extent = Vec2::new(placed.width as f32, placed.height as f32);
        let texel = Vec2::new(placed.x as f32, placed.y as f32);
        let first = vertices.len();
        push_quad(
            min,
            min + extent,
            (texel / page, (texel + extent) / page),
            color,
            Primitive::FontGlyph,
            vertices,
            indices,
        );
        for vertex in &mut vertices[first..] {
            vertex.shape[0] = placed.page as f32;
        }
    }
}

/// Push one rounded rectangle: a single quad whose vertices carry the shape.
///
/// The UV lane is each corner's offset from the centre — `-half` at `min`,
/// `+half` at `max` — so the fragment stage receives the offset interpolated
/// and evaluates the distance field there. The radii and the border width are
/// clamped to half the shorter side here rather than in the shader, where a
/// radius past it would bend the corners of a pill into each other.
///
/// A rectangle with no area, or one that is not finite, pushes nothing.
fn push_rounded_rect(
    min: Vec2,
    max: Vec2,
    radii: CornerRadii,
    color: [f32; 4],
    border: Border,
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    if !(min.is_finite() && max.is_finite()) || max.x <= min.x || max.y <= min.y {
        return;
    }
    let half = (max - min) * 0.5;
    let limit = half.x.min(half.y);
    let first = vertices.len();
    push_quad(
        min,
        max,
        (-half, half),
        color,
        Primitive::RoundedRect,
        vertices,
        indices,
    );
    for vertex in &mut vertices[first..] {
        vertex.shape = [
            half.x,
            half.y,
            border.width.max(0.0).min(limit),
            Primitive::RoundedRect.lane(),
        ];
        vertex.radii = radii.lane(limit);
        vertex.border = border.color;
    }
}

/// Stroke a run of points, centred on the path, and push the triangles.
///
/// A point that is not finite splits the run in two rather than being dropped:
/// dropping it would connect its neighbours with a chord the caller never
/// asked for, and a straight line across a discontinuity reads as data. A run
/// that was split is never closed, because its two ends are no longer known to
/// belong together.
///
/// Consecutive points closer together than one representable step are merged,
/// so every segment handed to [`push_run`] has a direction.
fn push_stroke(
    points: &[Vec2],
    closed: bool,
    thickness: f32,
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let half = thickness * 0.5;
    if half.is_nan() || half <= 0.0 || half.is_infinite() {
        return;
    }

    let mut run: Vec<Vec2> = Vec::with_capacity(points.len());
    let mut split = false;
    for &point in points {
        if !point.is_finite() {
            split = true;
            push_run(&run, false, half, color, vertices, indices);
            run.clear();
            continue;
        }
        if run.last().is_none_or(|&last| (point - last).length() > 0.0) {
            run.push(point);
        }
    }
    push_run(&run, closed && !split, half, color, vertices, indices);
}

/// Stroke one unbroken run whose consecutive points are all distinct.
///
/// Each segment becomes a quad offset by `half` along the segment's normal.
/// Corners are bevelled: a single triangle fills the wedge that opens on the
/// outside of the turn, which is where the two quads leave a gap. The inside
/// of the turn needs nothing — the quads already overlap there, and painting
/// it again would blend a translucent stroke against itself.
///
/// Bevel rather than miter deliberately. A miter joint's length grows without
/// bound as the turn sharpens, so it needs a limit and a fallback to this same
/// bevel anyway; going straight to the bevel is one triangle, no division, and
/// no angle at which it degenerates.
fn push_run(
    points: &[Vec2],
    closed: bool,
    half: f32,
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    // A closing segment needs a third point to be anything but a retrace of
    // the run, and it needs the two ends to actually differ — a caller that
    // repeated the first point to close the shape has already drawn it.
    let closed =
        closed && points.len() >= 3 && (points[points.len() - 1] - points[0]).length() > 0.0;

    let count = points.len();
    if count < 2 {
        return;
    }
    let segments = if closed { count } else { count - 1 };

    let mut directions: Vec<Vec2> = Vec::with_capacity(segments);
    for index in 0..segments {
        let delta = points[(index + 1) % count] - points[index];
        directions.push(delta / delta.length());
    }

    let normal = |direction: Vec2| Vec2::new(-direction.y, direction.x) * half;

    for index in 0..segments {
        let (a, b) = (points[index], points[(index + 1) % count]);
        let offset = normal(directions[index]);
        push_quad_free(
            a + offset,
            b + offset,
            b - offset,
            a - offset,
            color,
            vertices,
            indices,
        );
    }

    // One joint per interior corner, plus the seam when the run is closed.
    let joints = if closed { segments } else { segments - 1 };
    for index in 0..joints {
        let incoming = directions[index];
        let outgoing = directions[(index + 1) % segments];
        // `perp_dot` is positive when the path turns towards the +normal side,
        // which is the inside of the bend; the gap is on the other one.
        let turn = incoming.perp_dot(outgoing);
        if turn == 0.0 {
            continue;
        }
        let side = if turn > 0.0 { -1.0 } else { 1.0 };
        let corner = points[(index + 1) % count];
        push_triangle(
            corner,
            corner + normal(incoming) * side,
            corner + normal(outgoing) * side,
            color,
            vertices,
            indices,
        );
    }
}

/// Push one quad from four corners in order, for primitives that are not
/// axis-aligned. `p0`..`p3` must wind consistently; the stroke code emits them
/// as (start left, end left, end right, start right).
fn push_quad_free(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    for pos in [p0, p1, p2, p3] {
        vertices.push(Vertex2d::new(pos, Vec2::ZERO, color, Primitive::Solid));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Push one untextured triangle, used for stroke corners.
fn push_triangle(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    for pos in [p0, p1, p2] {
        vertices.push(Vertex2d::new(pos, Vec2::ZERO, color, Primitive::Solid));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Push one axis-aligned quad — 4 vertices, 6 indices — for use by
/// [`DrawList::to_triangles`].
///
/// `min`/`max` are the top-left and bottom-right corners in the Y-down screen
/// convention, and `(uv_min, uv_max)` the matching UV corners (both
/// [`Vec2::ZERO`] for untextured primitives). `primitive` is what the fragment
/// stage does with them.
///
/// Vertices are emitted bottom-left, bottom-right, top-right, top-left, which
/// after the shader's Y flip is counter-clockwise in NDC. Nothing depends on
/// that today — `crcbl-render`'s UI pass uses `PrimitiveState::default()` with
/// no face culling — but the order is fixed here rather than per-call-site so
/// there is only one thing to change if it ever does.
fn push_quad(
    min: Vec2,
    max: Vec2,
    (uv_min, uv_max): (Vec2, Vec2),
    color: [f32; 4],
    primitive: Primitive,
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    for (pos, uv) in [
        (Vec2::new(min.x, max.y), Vec2::new(uv_min.x, uv_max.y)),
        (Vec2::new(max.x, max.y), Vec2::new(uv_max.x, uv_max.y)),
        (Vec2::new(max.x, min.y), Vec2::new(uv_max.x, uv_min.y)),
        (Vec2::new(min.x, min.y), Vec2::new(uv_min.x, uv_min.y)),
    ] {
        vertices.push(Vertex2d::new(pos, uv, color, primitive));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

// SAFETY: Vertex2d is `#[repr(C)]` with only f32 fields (Vec2 is 2×f32,
// [f32; 4] is 4×f32), every field's alignment is 4, and the fields add to a
// multiple of it. No padding, all bit patterns valid.
unsafe impl bytemuck::Pod for Vertex2d {}
unsafe impl bytemuck::Zeroable for Vertex2d {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
