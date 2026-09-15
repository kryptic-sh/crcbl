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
//! draws, and `docs/plan/07-ui-debug.md` keeps batching as the reason.

use crate::font::Font;
use crate::font::atlas::{GlyphAtlas, SUBPIXEL_BINS};
use crate::font::layout::PositionedGlyph;
use crate::image::{AtlasImage, NineSliceImage, slice_bands, slice_cuts};
use crate::text::FontAtlas;
use crate::text::GLYPH_HEIGHT;
use core::fmt;
use glam::Vec2;

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

    /// Appends one command under the current clip.
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
        if !(min.is_finite() && max.is_finite() && scale.is_finite() && scale > 0.0) {
            return;
        }
        let insets = sliced.clamped_insets();
        let size = sliced.image.size();
        let texel_x = [0.0, insets.left, size.x - insets.right, size.x];
        let texel_y = [0.0, insets.top, size.y - insets.bottom, size.y];
        let extent = max - min;
        let xs = slice_cuts(
            min.x,
            slice_bands(insets.left * scale, insets.right * scale, extent.x),
            extent.x,
        );
        let ys = slice_cuts(
            min.y,
            slice_bands(insets.top * scale, insets.bottom * scale, extent.y),
            extent.y,
        );
        for row in 0..3 {
            if texel_y[row] == texel_y[row + 1] || ys[row + 1] <= ys[row] {
                continue;
            }
            for column in 0..3 {
                if texel_x[column] == texel_x[column + 1] || xs[column + 1] <= xs[column] {
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
                let glyphs = atlas.layout_line(text, *pos, layout_scale);
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
mod tests {
    use super::*;

    /// An opaque colour for tests that only care about geometry.
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

    /// **A glyph run becomes one whole-pixel quad per inked glyph**, its pen
    /// split into a pixel and the nearest subpixel bin, its UVs exactly the
    /// glyph's texels on its page, and its page in the first shape lane; a
    /// space draws nothing, and with no glyph atlas nothing is drawn.
    #[test]
    fn a_glyph_run_expands_to_whole_pixel_quads_on_its_glyphs_texels() {
        use crate::font::Font;
        use crate::font::layout::PositionedGlyph;

        let font = Font::sans();
        let run = |c: char, x: f32, y: f32| PositionedGlyph {
            glyph: font.glyph_id(c),
            offset: Vec2::new(x, y),
        };
        let mut dl = DrawList::new();
        dl.glyphs(
            Vec2::new(20.0, 30.0),
            font,
            18.0,
            RED,
            [
                run('H', 0.3, 14.6),
                run(' ', 12.0, 14.6),
                run('g', 14.9, 14.6),
            ],
        );
        assert!(dl.to_triangles(None, None, 1.0).0.is_empty());

        for scale in [1.0, 2.0] {
            let mut atlas = GlyphAtlas::new(256, 1, 100);
            atlas.begin_frame();
            let (vertices, indices) = dl.to_triangles(None, Some(&mut atlas), scale);
            assert_eq!(indices.len(), 12, "H and g, and nothing for the space");
            for (quad, (c, x)) in vertices.chunks_exact(4).zip([('H', 0.3), ('g', 14.9)]) {
                let pen = Vec2::new(20.0 + x * scale, 30.0 + 14.6 * scale);
                let steps = (pen.x * 4.0).round();
                let whole = (steps / 4.0).floor();
                let bin = (steps - whole * 4.0) as u8;
                let placed = atlas
                    .cached(GlyphAtlas::key(font, font.glyph_id(c), 18.0 * scale, bin))
                    .expect("rasterised during expansion");
                let min = Vec2::new(
                    whole + placed.left as f32,
                    pen.y.round() + placed.top as f32,
                );
                let max = min + Vec2::new(placed.width as f32, placed.height as f32);
                let top_left = quad[3];
                let bottom_right = quad[1];
                assert_eq!(
                    (top_left.pos, bottom_right.pos),
                    (min, max),
                    "{c:?} at {scale}"
                );
                assert_eq!(min, min.round(), "{c:?} is off the pixel grid");
                assert_eq!(
                    top_left.uv * 256.0,
                    Vec2::new(placed.x as f32, placed.y as f32)
                );
                assert_eq!(
                    bottom_right.uv * 256.0,
                    Vec2::new(
                        (placed.x + placed.width) as f32,
                        (placed.y + placed.height) as f32
                    )
                );
                for vertex in quad {
                    assert_eq!(vertex.primitive(), Some(Primitive::FontGlyph));
                    assert_eq!(vertex.shape[0], placed.page as f32);
                    assert_eq!(vertex.color, RED);
                }
            }
        }
    }

    #[test]
    fn new_draw_list_is_empty() {
        let dl = DrawList::new();
        assert!(dl.is_empty());
        assert_eq!(dl.len(), 0);
    }

    #[test]
    fn rect_command_is_stored() {
        let mut dl = DrawList::new();
        dl.rect(
            Vec2::new(10.0, 10.0),
            Vec2::new(100.0, 50.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(dl.len(), 1);
        match &dl.commands()[0] {
            DrawCommand::Rect { min, max, color } => {
                assert_eq!(*min, Vec2::new(10.0, 10.0));
                assert_eq!(*max, Vec2::new(100.0, 50.0));
                assert_eq!(*color, [1.0, 0.0, 0.0, 1.0]);
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn rect_outline_command_is_stored() {
        let mut dl = DrawList::new();
        dl.rect_outline(Vec2::ZERO, Vec2::splat(50.0), 2.0, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(dl.len(), 1);
    }

    #[test]
    fn text_command_is_stored() {
        let mut dl = DrawList::new();
        dl.text(Vec2::new(5.0, 5.0), "hello", [1.0, 1.0, 1.0, 1.0], 16.0);
        assert_eq!(dl.len(), 1);
        match &dl.commands()[0] {
            DrawCommand::Text { text, size, .. } => {
                assert_eq!(text, "hello");
                assert_eq!(*size, 16.0);
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn clear_empties_the_list() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), [1.0; 4]);
        dl.clear();
        assert!(dl.is_empty());
    }

    #[test]
    fn into_commands_hands_back_the_commands_that_were_recorded() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), [1.0; 4]);
        let cmds = dl.into_commands();
        assert_eq!(cmds.len(), 1);
        // `dl` is consumed; can't use it after this.
    }

    #[test]
    fn multiple_commands_are_ordered() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), [1.0; 4]);
        dl.text(Vec2::new(5.0, 5.0), "hi", [1.0; 4], 12.0);
        assert_eq!(dl.len(), 2);
        assert!(matches!(dl.commands()[0], DrawCommand::Rect { .. }));
        assert!(matches!(dl.commands()[1], DrawCommand::Text { .. }));
    }

    // ── triangulation ─────────────────────────────────────────────────

    #[test]
    fn to_triangles_from_empty_list() {
        let dl = DrawList::new();
        let (verts, indices) = dl.to_triangles(None, None, 1.0);
        assert!(verts.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn rect_becomes_one_quad() {
        let mut dl = DrawList::new();
        dl.rect(
            Vec2::new(10.0, 20.0),
            Vec2::new(110.0, 120.0),
            [1.0, 0.5, 0.0, 1.0],
        );
        let (verts, indices) = dl.to_triangles(None, None, 1.0);

        // One quad = 4 vertices, 6 indices (2 triangles).
        assert_eq!(verts.len(), 4);
        assert_eq!(indices.len(), 6);

        // Emission order, in the Y-down screen convention: bottom-left,
        // bottom-right, top-right, top-left.
        assert_eq!(verts[0].pos, Vec2::new(10.0, 120.0)); // bottom-left
        assert_eq!(verts[1].pos, Vec2::new(110.0, 120.0)); // bottom-right
        assert_eq!(verts[2].pos, Vec2::new(110.0, 20.0)); // top-right
        assert_eq!(verts[3].pos, Vec2::new(10.0, 20.0)); // top-left

        // All vertices share the command's color.
        for v in &verts {
            assert_eq!(v.color, [1.0, 0.5, 0.0, 1.0]);
        }

        // Indices form two triangles: (0,1,2) and (0,2,3).
        assert_eq!(&indices[..3], &[0, 1, 2]);
        assert_eq!(&indices[3..], &[0, 2, 3]);
    }

    #[test]
    fn rect_outline_becomes_four_quads() {
        let mut dl = DrawList::new();
        dl.rect_outline(
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 80.0),
            3.0,
            [0.0, 1.0, 0.0, 1.0],
        );
        let (verts, indices) = dl.to_triangles(None, None, 1.0);

        // 4 quads = 16 vertices, 24 indices.
        assert_eq!(verts.len(), 16);
        assert_eq!(indices.len(), 24);

        // All verts have the outline colour.
        for v in &verts {
            assert_eq!(v.color, [0.0, 1.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn text_commands_are_skipped_in_triangulation() {
        let mut dl = DrawList::new();
        dl.text(Vec2::new(5.0, 5.0), "hello", [1.0; 4], 16.0);
        let (verts, indices) = dl.to_triangles(None, None, 1.0);
        assert!(verts.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn mixed_commands_expand_correctly() {
        let mut dl = DrawList::new();
        dl.rect(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        dl.rect(
            Vec2::new(20.0, 0.0),
            Vec2::new(30.0, 10.0),
            [0.0, 1.0, 0.0, 1.0],
        );
        dl.text(Vec2::ZERO, "skipped", [0.0; 4], 12.0);
        let (verts, indices) = dl.to_triangles(None, None, 1.0);

        // 2 rects → 8 verts, 12 indices (text skipped).
        assert_eq!(verts.len(), 8);
        assert_eq!(indices.len(), 12);

        // First rect's verts are red, second's are green.
        assert_eq!(verts[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(verts[4].color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(indices[0], 0);
        assert_eq!(indices[6], 4);
    }

    #[test]
    fn text_with_atlas_generates_vertex_uvs() {
        use crate::text::{FontAtlas, GLYPH_WIDTH};

        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(Vec2::new(100.0, 200.0), "A", [1.0, 0.0, 0.0, 1.0], 13.0);
        let (verts, indices) = dl.to_triangles(Some(&atlas), None, 1.0);

        // One glyph 'A' → one quad.
        assert_eq!(verts.len(), 4);
        assert_eq!(indices.len(), 6);

        // 'A' is codepoint 65, atlas column = 65 - 32 = 33.
        let atlas_w = atlas.texture_size.0 as f32;
        let expected_u_min = (33.0 * GLYPH_WIDTH as f32) / atlas_w;
        let expected_u_max = (34.0 * GLYPH_WIDTH as f32) / atlas_w;

        assert!(
            (verts[0].uv.x - expected_u_min).abs() < 0.001,
            "u_min mismatch"
        );
        assert!(
            (verts[1].uv.x - expected_u_max).abs() < 0.001,
            "u_max mismatch"
        );

        // All verts share the text colour.
        for v in &verts {
            assert_eq!(v.color, [1.0, 0.0, 0.0, 1.0]);
        }
    }

    /// Pins the UV/position relationship the shader depends on. `ui.slang`
    /// computes `ndc.y = 1.0 - (y / viewport.y) * 2.0`, so screen y = 0 is the
    /// **top** of the framebuffer; the atlas's top row (`v = 0`) must therefore
    /// be emitted at the quad's smallest y. Emitting it at the largest y — as
    /// `to_triangles` used to — renders every glyph vertically mirrored.
    ///
    /// There is no golden image for the UI pass, so this is the check that
    /// keeps the two ends of the convention from drifting apart again.
    #[test]
    fn glyph_atlas_top_row_maps_to_the_quads_top_edge() {
        use crate::text::GLYPH_WIDTH;

        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(Vec2::new(100.0, 200.0), "A", [1.0; 4], 13.0);
        let (verts, _) = dl.to_triangles(Some(&atlas), None, 1.0);
        assert_eq!(verts.len(), 4);

        for v in &verts {
            let top = v.pos.y < 200.0 + GLYPH_HEIGHT as f32 * 0.5;
            let expected_v = if top { 0.0 } else { 1.0 };
            assert!(
                (v.uv.y - expected_v).abs() < 0.001,
                "vertex at y={} has v={}, expected {expected_v} — the glyph is mirrored",
                v.pos.y,
                v.uv.y
            );
        }

        // And the same for u: the atlas's left column is at the quad's left.
        let u_left = atlas.glyph_u_min('A');
        for v in &verts {
            let left = v.pos.x < 100.0 + GLYPH_WIDTH as f32 * 0.5 + 1.0;
            let expected_u = if left { u_left } else { atlas.glyph_u_max('A') };
            assert!(
                (v.uv.x - expected_u).abs() < 0.001,
                "u mismatch at {:?}",
                v.pos
            );
        }
    }

    /// The bug: the top edge tapered to the *inner* width while the bottom was
    /// emitted full-width and the verticals stopped short, leaving each top
    /// corner half covered. Every pixel of the border ring must be painted.
    #[test]
    fn rect_outline_covers_every_corner() {
        let mut dl = DrawList::new();
        dl.rect_outline(Vec2::ZERO, Vec2::new(100.0, 80.0), 3.0, [1.0; 4]);
        let (verts, _) = dl.to_triangles(None, None, 1.0);

        // Every point in the border ring must lie inside one of the four quads.
        let quads: Vec<(Vec2, Vec2)> = verts
            .chunks_exact(4)
            .map(|q| {
                let xs = q.iter().map(|v| v.pos.x);
                let ys = q.iter().map(|v| v.pos.y);
                (
                    Vec2::new(
                        xs.clone().fold(f32::MAX, f32::min),
                        ys.clone().fold(f32::MAX, f32::min),
                    ),
                    Vec2::new(xs.fold(f32::MIN, f32::max), ys.fold(f32::MIN, f32::max)),
                )
            })
            .collect();
        assert_eq!(quads.len(), 4);

        let covered = |p: Vec2| {
            quads
                .iter()
                .any(|(lo, hi)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y)
        };
        // Sample the four corner squares, which is where the miter left holes.
        for p in [
            Vec2::new(1.5, 1.5),
            Vec2::new(98.5, 1.5),
            Vec2::new(1.5, 78.5),
            Vec2::new(98.5, 78.5),
        ] {
            assert!(covered(p), "corner pixel {p:?} is not covered by any quad");
        }
        // And the interior must stay hollow.
        assert!(!covered(Vec2::new(50.0, 40.0)), "the border filled the box");
    }

    /// `rect_outline((0,0), (10,10), 8.0)` used to invert the inner rect and
    /// emit self-intersecting bowties. Clamping turns it into a filled box.
    #[test]
    fn rect_outline_thickness_is_clamped_to_half_the_extent() {
        let mut dl = DrawList::new();
        dl.rect_outline(Vec2::ZERO, Vec2::splat(10.0), 8.0, [1.0; 4]);
        let (verts, _) = dl.to_triangles(None, None, 1.0);

        for q in verts.chunks_exact(4) {
            let (x0, x1) = (q[3].pos.x, q[1].pos.x);
            let (y0, y1) = (q[3].pos.y, q[1].pos.y);
            assert!(
                x1 >= x0 && y1 >= y0,
                "quad is inverted: ({x0},{y0})..({x1},{y1})"
            );
            assert!(
                (0.0..=10.0).contains(&x0) && (0.0..=10.0).contains(&x1),
                "quad escapes the declared bounds"
            );
        }
    }

    #[test]
    fn space_generates_no_vertices() {
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(Vec2::ZERO, " ", [1.0; 4], 13.0);
        let (verts, indices) = dl.to_triangles(Some(&atlas), None, 1.0);
        // Space glyph has width=0 → no quad.
        assert!(verts.is_empty());
        assert!(indices.is_empty());
    }

    // ── Snapshot (golden-hash) tests ─────────────────────────────────────

    /// Hash the combined vertex and index data into a single `u64` using
    /// [`std::collections::hash_map::DefaultHasher`]. Same purpose as the
    /// state-hash functions in `crcbl-phys` and `crcbl-net`: a
    /// deterministic, lightweight identity check that catches regressions
    /// in triangulation output.
    fn hash_triangles(verts: &[Vertex2d], indices: &[u32]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        for v in verts {
            v.pos.x.to_bits().hash(&mut h);
            v.pos.y.to_bits().hash(&mut h);
            v.uv.x.to_bits().hash(&mut h);
            v.uv.y.to_bits().hash(&mut h);
            v.color[0].to_bits().hash(&mut h);
            v.color[1].to_bits().hash(&mut h);
            v.color[2].to_bits().hash(&mut h);
            v.color[3].to_bits().hash(&mut h);
        }
        for &idx in indices {
            idx.hash(&mut h);
        }
        h.finish()
    }

    /// A full HUD scene — score label, health bar, background panels,
    /// pause button — triangulated with atlas. The hash is a regression
    /// gate: any change to triangulation vertex order, colour, or UV must
    /// be intentional and update the golden value.
    #[test]
    fn snapshot_hud_scene_hash() {
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();

        // Top-left info panel background.
        dl.rect(
            Vec2::new(4.0, 4.0),
            Vec2::new(204.0, 54.0),
            [0.15, 0.15, 0.15, 0.9],
        );

        // Score text.
        dl.text(
            Vec2::new(10.0, 10.0),
            "Score: 1,250",
            [1.0, 1.0, 0.3, 1.0],
            14.0,
        );

        // FPS text.
        dl.text(Vec2::new(10.0, 28.0), "FPS: 60", [0.5, 0.8, 0.5, 1.0], 12.0);

        // Bottom-centre health bar background.
        dl.rect(
            Vec2::new(300.0, 560.0),
            Vec2::new(500.0, 576.0),
            [0.1, 0.1, 0.1, 0.8],
        );
        // Health bar fill.
        dl.rect(
            Vec2::new(302.0, 562.0),
            Vec2::new(450.0, 574.0),
            [0.8, 0.2, 0.2, 1.0],
        );
        // Health bar outline.
        dl.rect_outline(
            Vec2::new(300.0, 560.0),
            Vec2::new(500.0, 576.0),
            1.0,
            [0.4, 0.4, 0.4, 1.0],
        );

        // Pause button in top-right corner.
        dl.rect(
            Vec2::new(720.0, 4.0),
            Vec2::new(796.0, 32.0),
            [0.15, 0.15, 0.15, 0.9],
        );
        dl.text(Vec2::new(732.0, 10.0), "Pause", [1.0, 1.0, 1.0, 1.0], 14.0);
        dl.rect_outline(
            Vec2::new(720.0, 4.0),
            Vec2::new(796.0, 32.0),
            1.0,
            [0.4, 0.4, 0.4, 1.0],
        );

        let (verts, indices) = dl.to_triangles(Some(&atlas), None, 1.0);

        // Structural assertions: exact vertex/index counts from known scene.
        assert_eq!(verts.len(), 136, "vertex count changed");
        assert_eq!(indices.len(), 204, "index count changed");

        let hash = hash_triangles(&verts, &indices);

        // Golden hash. Update only when triangulation output intentionally
        // changes. Last re-blessed when text layout stopped double-applying
        // `scale`, the `Text` anchor became a true top-left, glyph V flipped to
        // match `ui.slang`'s Y-down screen space, and `RectOutline` stopped
        // mitering its top edge.
        assert_eq!(
            hash, 16_849_584_058_741_182_623,
            "draw-list snapshot hash mismatch — triangulation output changed"
        );

        // Determinism: same scene produces the same hash.
        let mut dl2 = DrawList::new();
        dl2.rect(
            Vec2::new(4.0, 4.0),
            Vec2::new(204.0, 54.0),
            [0.15, 0.15, 0.15, 0.9],
        );
        dl2.text(
            Vec2::new(10.0, 10.0),
            "Score: 1,250",
            [1.0, 1.0, 0.3, 1.0],
            14.0,
        );
        dl2.text(Vec2::new(10.0, 28.0), "FPS: 60", [0.5, 0.8, 0.5, 1.0], 12.0);
        dl2.rect(
            Vec2::new(300.0, 560.0),
            Vec2::new(500.0, 576.0),
            [0.1, 0.1, 0.1, 0.8],
        );
        dl2.rect(
            Vec2::new(302.0, 562.0),
            Vec2::new(450.0, 574.0),
            [0.8, 0.2, 0.2, 1.0],
        );
        dl2.rect_outline(
            Vec2::new(300.0, 560.0),
            Vec2::new(500.0, 576.0),
            1.0,
            [0.4, 0.4, 0.4, 1.0],
        );
        dl2.rect(
            Vec2::new(720.0, 4.0),
            Vec2::new(796.0, 32.0),
            [0.15, 0.15, 0.15, 0.9],
        );
        dl2.text(Vec2::new(732.0, 10.0), "Pause", [1.0, 1.0, 1.0, 1.0], 14.0);
        dl2.rect_outline(
            Vec2::new(720.0, 4.0),
            Vec2::new(796.0, 32.0),
            1.0,
            [0.4, 0.4, 0.4, 1.0],
        );
        let (verts2, indices2) = dl2.to_triangles(Some(&atlas), None, 1.0);
        let hash2 = hash_triangles(&verts2, &indices2);
        assert_eq!(hash, hash2, "same scene → same hash");

        // Sanity: a non-trivial scene produces non-trivial geometry.
        assert!(verts.len() > 20);
        assert!(indices.len() > 30);
    }

    /// Snapshot hash for an empty draw list — the zero-input baseline.
    #[test]
    fn snapshot_empty_list_hash() {
        let dl = DrawList::new();
        let (verts, indices) = dl.to_triangles(None, None, 1.0);
        let hash = hash_triangles(&verts, &indices);
        assert_eq!(
            hash, 15_130_871_412_783_076_140,
            "empty draw-list hash changed"
        );
    }

    // -----------------------------------------------------------------------
    // Stroked lines
    // -----------------------------------------------------------------------

    /// Whether `point` lies inside the triangle `a`,`b`,`c`, edges included.
    ///
    /// Sign-of-cross-product test against each edge; a point is inside when it
    /// is on the same side of all three, which for a degenerate triangle is
    /// never true away from the line itself.
    fn inside(point: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
        let edge = |p: Vec2, q: Vec2| (q - p).perp_dot(point - p);
        let (x, y, z) = (edge(a, b), edge(b, c), edge(c, a));
        let negative = x < 0.0 || y < 0.0 || z < 0.0;
        let positive = x > 0.0 || y > 0.0 || z > 0.0;
        !(negative && positive)
    }

    /// Whether any emitted triangle covers `point`.
    fn covered(point: Vec2, vertices: &[Vertex2d], indices: &[u32]) -> bool {
        indices.chunks_exact(3).any(|t| {
            inside(
                point,
                vertices[t[0] as usize].pos,
                vertices[t[1] as usize].pos,
                vertices[t[2] as usize].pos,
            )
        })
    }

    #[test]
    fn a_line_is_stroked_centred_on_the_segment() {
        let mut dl = DrawList::new();
        dl.line(Vec2::new(0.0, 10.0), Vec2::new(100.0, 10.0), 4.0, RED);
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        assert_eq!(vertices.len(), 4, "one quad");
        assert_eq!(indices.len(), 6);
        let ys: Vec<f32> = vertices.iter().map(|v| v.pos.y).collect();
        let lo = ys.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        // Centred, not offset to one side: half the thickness each way.
        assert_eq!((lo, hi), (8.0, 12.0), "{ys:?}");
    }

    #[test]
    fn a_diagonal_stroke_is_as_wide_as_its_thickness() {
        let (from, to) = (Vec2::new(10.0, 10.0), Vec2::new(50.0, 90.0));
        let mut dl = DrawList::new();
        dl.line(from, to, 6.0, RED);
        let (vertices, _) = dl.to_triangles(None, None, 1.0);

        // Measured across the segment, not along an axis: an implementation
        // that offset by `half` in x and y would pass an axis-aligned check
        // and be too wide here by root two.
        let direction = (to - from).normalize();
        let across = Vec2::new(-direction.y, direction.x);
        let offsets: Vec<f32> = vertices.iter().map(|v| across.dot(v.pos - from)).collect();
        let lo = offsets.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = offsets.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (hi - lo - 6.0).abs() < 1.0e-4,
            "width {} from {offsets:?}",
            hi - lo
        );
        assert!((lo + hi).abs() < 1.0e-4, "not centred: {lo} .. {hi}");
    }

    #[test]
    fn a_corner_is_filled_rather_than_left_notched() {
        // A right-angle turn. Without a join the two quads meet at the inside
        // corner only and leave a square notch on the outside, which is the
        // artefact this whole helper exists to avoid.
        let corner = Vec2::new(50.0, 50.0);
        let thickness = 10.0;
        let mut dl = DrawList::new();
        dl.polyline(
            vec![Vec2::new(0.0, 50.0), corner, Vec2::new(50.0, 0.0)],
            thickness,
            false,
            RED,
        );
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        // The elbow opens down-and-right: the two segment quads reach only to
        // `corner` along their own axis, so the square just outside it belongs
        // to neither and the bevel is the only thing that can fill it. The
        // up-and-right side is *inside* the second quad and stays covered even
        // with no join at all, which is why it is the wrong place to look.
        let half = thickness * 0.5;
        let outer = corner + Vec2::new(half, half) * 0.3;
        let inner = corner + Vec2::new(half, -half) * 0.3;
        assert!(
            covered(outer, &vertices, &indices),
            "the notch outside the turn at {outer:?} is not covered"
        );
        assert!(
            covered(inner, &vertices, &indices),
            "the inside of the turn at {inner:?} came uncovered"
        );
        // A bevel cuts the corner rather than extending to the miter point, so
        // the far tip of that square stays open. Pinning it keeps a future
        // switch to miters from passing this test silently.
        let tip = corner + Vec2::new(half, half) * 0.95;
        assert!(
            !covered(tip, &vertices, &indices),
            "the bevel reached {tip:?}, which is past where a bevel ends"
        );
    }

    #[test]
    fn a_closed_polyline_strokes_the_seam() {
        let square = vec![
            Vec2::new(10.0, 10.0),
            Vec2::new(90.0, 10.0),
            Vec2::new(90.0, 90.0),
            Vec2::new(10.0, 90.0),
        ];
        let mut open = DrawList::new();
        open.polyline(square.clone(), 4.0, false, RED);
        let mut closed = DrawList::new();
        closed.polyline(square, 4.0, true, RED);

        let (_, open_indices) = open.to_triangles(None, None, 1.0);
        let (closed_vertices, closed_indices) = closed.to_triangles(None, None, 1.0);
        assert!(
            closed_indices.len() > open_indices.len(),
            "closing added nothing: {} vs {}",
            closed_indices.len(),
            open_indices.len()
        );
        // The closing edge runs down the left side; nothing on the open run
        // draws there.
        assert!(
            covered(Vec2::new(10.0, 50.0), &closed_vertices, &closed_indices),
            "the seam segment was not stroked"
        );
    }

    #[test]
    fn a_non_finite_point_breaks_the_run_instead_of_bridging_it() {
        let mut dl = DrawList::new();
        dl.polyline(
            vec![
                Vec2::new(0.0, 50.0),
                Vec2::new(20.0, 50.0),
                Vec2::new(f32::NAN, f32::NAN),
                Vec2::new(80.0, 50.0),
                Vec2::new(100.0, 50.0),
            ],
            4.0,
            false,
            RED,
        );
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        assert!(
            covered(Vec2::new(10.0, 50.0), &vertices, &indices),
            "the run before the break was dropped"
        );
        assert!(
            covered(Vec2::new(90.0, 50.0), &vertices, &indices),
            "the run after the break was dropped"
        );
        // The gap is the point: a chord across it is data the caller never
        // supplied.
        assert!(
            !covered(Vec2::new(50.0, 50.0), &vertices, &indices),
            "the break was bridged"
        );
    }

    #[test]
    fn strokes_with_no_extent_draw_nothing() {
        let point = Vec2::new(5.0, 5.0);
        for (name, list) in [
            ("zero thickness", {
                let mut dl = DrawList::new();
                dl.line(Vec2::ZERO, Vec2::new(10.0, 0.0), 0.0, RED);
                dl
            }),
            ("negative thickness", {
                let mut dl = DrawList::new();
                dl.line(Vec2::ZERO, Vec2::new(10.0, 0.0), -4.0, RED);
                dl
            }),
            ("nan thickness", {
                let mut dl = DrawList::new();
                dl.line(Vec2::ZERO, Vec2::new(10.0, 0.0), f32::NAN, RED);
                dl
            }),
            ("infinite thickness", {
                let mut dl = DrawList::new();
                dl.line(Vec2::ZERO, Vec2::new(10.0, 0.0), f32::INFINITY, RED);
                dl
            }),
            ("a segment of zero length", {
                let mut dl = DrawList::new();
                dl.line(point, point, 4.0, RED);
                dl
            }),
            ("a single point", {
                let mut dl = DrawList::new();
                dl.polyline(vec![point], 4.0, true, RED);
                dl
            }),
            ("no points at all", {
                let mut dl = DrawList::new();
                dl.polyline(Vec::new(), 4.0, true, RED);
                dl
            }),
            ("every point the same", {
                let mut dl = DrawList::new();
                dl.polyline(vec![point, point, point], 4.0, true, RED);
                dl
            }),
        ] {
            let (vertices, indices) = list.to_triangles(None, None, 1.0);
            assert!(
                vertices.is_empty() && indices.is_empty(),
                "{name} emitted {} vertices",
                vertices.len()
            );
        }
    }

    #[test]
    fn every_stroke_triangle_indexes_a_vertex_that_exists() {
        // The stroke helpers push vertices and indices from two places, so a
        // base-offset slip would go unnoticed by the coverage checks above.
        let mut dl = DrawList::new();
        dl.line(Vec2::ZERO, Vec2::new(10.0, 10.0), 3.0, RED);
        dl.polyline(
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(30.0, 5.0),
                Vec2::new(10.0, 40.0),
            ],
            3.0,
            true,
            RED,
        );
        dl.rect(Vec2::ZERO, Vec2::new(4.0, 4.0), RED);
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        assert_eq!(indices.len() % 3, 0);
        assert!(!indices.is_empty());
        for &index in &indices {
            assert!(
                (index as usize) < vertices.len(),
                "index {index} of {} vertices",
                vertices.len()
            );
        }
    }

    // -----------------------------------------------------------------------
    // The overlay cut
    // -----------------------------------------------------------------------

    /// **The cut lands where `begin_overlay` was called**, in the commands and
    /// in the triangles both.
    ///
    /// The two halves are asserted against each other rather than against a
    /// written-down count: `base` plus `overlay` must be the whole list and the
    /// whole index buffer, so a cut recorded at zero — which is what an
    /// uninitialised marker gives — puts every command in the overlay and fails
    /// the first pair, and a cut that never moved off `len()` fails the second.
    #[test]
    fn the_overlay_cut_splits_the_list_where_it_was_taken() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);
        dl.rect(Vec2::splat(20.0), Vec2::splat(30.0), RED);
        dl.begin_overlay();
        dl.rect(Vec2::splat(40.0), Vec2::splat(50.0), RED);

        assert_eq!(dl.base_commands().len(), 2, "two commands went in first");
        assert_eq!(dl.overlay_commands().len(), 1, "one went in after the cut");
        assert_eq!(
            dl.base_commands().len() + dl.overlay_commands().len(),
            dl.len()
        );

        let triangles = dl.to_triangles_split(None, None, 1.0);
        // Every rect here is one quad, so the cut falls on a quad boundary and
        // the two halves are countable without re-tessellating anything.
        let per_quad = triangles.indices.len() / 3;
        assert_eq!(per_quad, 6, "three quads, two triangles each");
        assert_eq!(
            triangles.overlay,
            triangles.indices.len() * 2 / 3,
            "the first two of three quads are below the cut"
        );
        assert_eq!(
            triangles.indices,
            dl.to_triangles(None, None, 1.0).1,
            "splitting the list must not change the geometry it expands to"
        );
    }

    /// A list nobody cut is **one** layer: the whole thing below, nothing above,
    /// and an overlay index at the end of the buffer so a renderer's second
    /// range is empty.
    #[test]
    fn a_list_with_no_cut_is_all_below_it() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);

        assert_eq!(dl.base_commands().len(), 1);
        assert!(dl.overlay_commands().is_empty());
        let triangles = dl.to_triangles_split(None, None, 1.0);
        assert!(!triangles.indices.is_empty(), "the rect tessellated");
        assert_eq!(triangles.overlay, triangles.indices.len());
    }

    /// **`clear` drops the cut with the commands.** A frame that inherited the
    /// previous frame's cut would put its opening commands above the menu, which
    /// is the bug with the widest blast radius here: the draw list is reused
    /// every frame and cleared exactly once.
    #[test]
    fn clear_drops_the_overlay_cut() {
        let mut dl = DrawList::new();
        dl.begin_overlay();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);
        assert_eq!(dl.overlay_commands().len(), 1, "the cut was taken at zero");

        dl.clear();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);
        assert_eq!(
            dl.base_commands().len(),
            1,
            "the cleared list starts below the cut again"
        );
        assert!(dl.overlay_commands().is_empty());
    }

    /// The **last** cut wins, which is what keeps the engine's overlays on top
    /// of a game that marked a boundary of its own during `draw`.
    #[test]
    fn a_second_cut_moves_the_boundary_down() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);
        dl.begin_overlay();
        dl.rect(Vec2::splat(20.0), Vec2::splat(30.0), RED);
        dl.begin_overlay();
        dl.rect(Vec2::splat(40.0), Vec2::splat(50.0), RED);

        assert_eq!(
            dl.base_commands().len(),
            2,
            "the later cut is the one taken"
        );
        assert_eq!(dl.overlay_commands().len(), 1);
    }

    // -----------------------------------------------------------------------
    // The vertex layout
    // -----------------------------------------------------------------------

    /// **The layout `ui.slang`'s `Vertex` mirrors**: six lanes of floats, 96
    /// bytes, each lane where the shader reads it. A field added, dropped or
    /// reordered here moves an offset the storage buffer is read at, and the
    /// fragment stage would then read a colour as a clip.
    #[test]
    fn the_vertex_is_six_lanes_at_the_offsets_the_shader_reads() {
        assert_eq!(size_of::<Vertex2d>(), 96);
        assert_eq!(core::mem::offset_of!(Vertex2d, pos), 0);
        assert_eq!(core::mem::offset_of!(Vertex2d, uv), 8);
        assert_eq!(core::mem::offset_of!(Vertex2d, color), 16);
        assert_eq!(core::mem::offset_of!(Vertex2d, clip), 32);
        assert_eq!(core::mem::offset_of!(Vertex2d, shape), 48);
        assert_eq!(core::mem::offset_of!(Vertex2d, radii), 64);
        assert_eq!(core::mem::offset_of!(Vertex2d, border), 80);
    }

    /// The pre-existing primitives say which they are, and nothing else about
    /// them moved: no clip, no shape, no radii, no border.
    #[test]
    fn the_old_primitives_are_solid_or_glyph_and_unclipped() {
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(10.0), RED);
        dl.text(Vec2::new(20.0, 0.0), "A", RED, 13.0);
        let (vertices, _) = dl.to_triangles(Some(&atlas), None, 1.0);
        assert_eq!(vertices.len(), 8);
        for (index, vertex) in vertices.iter().enumerate() {
            let expected = if index < 4 {
                Primitive::Solid
            } else {
                Primitive::Glyph
            };
            assert_eq!(vertex.primitive(), Some(expected), "vertex {index}");
            assert_eq!(vertex.clip, ClipRect::NONE.lane());
            assert_eq!(vertex.shape[..3], [0.0; 3]);
            assert_eq!((vertex.radii, vertex.border), ([0.0; 4], [0.0; 4]));
        }
    }

    // -----------------------------------------------------------------------
    // Images and nine-slices
    // -----------------------------------------------------------------------

    use crate::image::{ImageAtlas, NineSliceImage};
    use crate::widget::SkinInsets;

    /// An atlas holding one `width` by `height` opaque image, registered after a
    /// throwaway one so the image under test is not at the page origin.
    fn atlas_with(width: u32, height: u32) -> (ImageAtlas, AtlasImage) {
        let mut images = ImageAtlas::new();
        images
            .register(3, 3, &[9; 36])
            .expect("a 3x3 image fits an empty page");
        let image = images
            .register(width, height, &vec![255; (width * height * 4) as usize])
            .expect("fits");
        assert_ne!((image.x, image.y), (0, 0));
        (images, image)
    }

    #[test]
    fn an_image_is_one_quad_sampling_exactly_its_own_rectangle() {
        let (_, image) = atlas_with(16, 8);
        let tint = [0.5, 0.25, 1.0, 0.75];
        let mut dl = DrawList::new();
        dl.image(Vec2::new(10.0, 20.0), Vec2::new(42.0, 36.0), &image, tint);
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        assert_eq!((vertices.len(), indices.len()), (4, 6));
        let page = crate::image::PAGE_SIZE as f32;
        for vertex in &vertices {
            assert_eq!(vertex.primitive(), Some(Primitive::Image));
            assert_eq!(vertex.color, tint);
            // The image's top-left texel corner at the quad's top-left, and its
            // bottom-right at the bottom-right — upright, not mirrored.
            let texel = if vertex.pos.x < 26.0 {
                image.x
            } else {
                image.x + 16
            };
            let row = if vertex.pos.y < 28.0 {
                image.y
            } else {
                image.y + 8
            };
            assert_eq!(
                vertex.uv * page,
                Vec2::new(texel as f32, row as f32),
                "{vertex:?}"
            );
        }
    }

    /// The four cut lines of a nine-slice as the quads it emitted report them,
    /// one axis at a time, deduplicated.
    fn cut_lines(dl: &DrawList, axis: fn(Vec2) -> f32) -> Vec<f32> {
        let mut cuts: Vec<f32> = dl
            .commands()
            .iter()
            .flat_map(|command| match command {
                DrawCommand::Image { min, max, .. } => [axis(*min), axis(*max)],
                other => panic!("a nine-slice emitted {other:?}"),
            })
            .collect();
        cuts.sort_by(f32::total_cmp);
        cuts.dedup();
        cuts
    }

    /// **The corners stay fixed as the target grows, the bands between take the
    /// growth, and every quad samples its own band of the image.**
    #[test]
    fn a_nine_slice_keeps_its_corners_and_stretches_the_bands_between() {
        let (_, image) = atlas_with(16, 12);
        // All four insets different, so a mirrored or transposed cut shows up.
        let sliced = NineSliceImage {
            image,
            insets: SkinInsets::new(3.0, 5.0, 2.0, 4.0),
        };
        let scale = 2.0;
        for (min, max) in [
            (Vec2::new(10.0, 10.0), Vec2::new(60.0, 40.0)),
            (Vec2::new(0.0, 100.0), Vec2::new(300.0, 190.0)),
        ] {
            let mut dl = DrawList::new();
            dl.nine_slice(min, max, &sliced, scale, [1.0; 4]);
            assert_eq!(dl.len(), 9, "every band is non-empty at this size");

            assert_eq!(
                cut_lines(&dl, |v| v.x),
                [min.x, min.x + 6.0, max.x - 10.0, max.x],
                "the columns: 3 and 5 texels at two pixels each"
            );
            assert_eq!(
                cut_lines(&dl, |v| v.y),
                [min.y, min.y + 4.0, max.y - 8.0, max.y],
                "the rows: 2 and 4 texels at two pixels each"
            );

            // Image order, and each quad's UVs are the matching texel band.
            let page = crate::image::PAGE_SIZE as f32;
            let texel_x = [0.0, 3.0, 11.0, 16.0];
            let texel_y = [0.0, 2.0, 8.0, 12.0];
            for (index, command) in dl.commands().iter().enumerate() {
                let DrawCommand::Image { uv_min, uv_max, .. } = command else {
                    unreachable!("cut_lines checked every command");
                };
                let (row, column) = (index / 3, index % 3);
                assert_eq!(
                    *uv_min * page,
                    Vec2::new(
                        image.x as f32 + texel_x[column],
                        image.y as f32 + texel_y[row]
                    ),
                    "quad {index}"
                );
                assert_eq!(
                    *uv_max * page,
                    Vec2::new(
                        image.x as f32 + texel_x[column + 1],
                        image.y as f32 + texel_y[row + 1]
                    ),
                    "quad {index}"
                );
            }
        }
    }

    #[test]
    fn an_empty_band_emits_no_quad() {
        let (_, image) = atlas_with(16, 8);
        // A three-slice: caps left and right, nothing fixed top or bottom.
        let bar = NineSliceImage {
            image,
            insets: SkinInsets::new(4.0, 4.0, 0.0, 0.0),
        };
        let mut dl = DrawList::new();
        dl.nine_slice(Vec2::ZERO, Vec2::new(100.0, 20.0), &bar, 1.0, [1.0; 4]);
        assert_eq!(dl.len(), 3, "a three-slice is three quads");

        // No insets at all is the whole image once.
        let plain = NineSliceImage {
            image,
            insets: SkinInsets::NONE,
        };
        let mut dl = DrawList::new();
        dl.nine_slice(Vec2::ZERO, Vec2::new(100.0, 20.0), &plain, 1.0, [1.0; 4]);
        assert_eq!(dl.len(), 1);
    }

    /// Below its corners the slice squashes them in proportion and still fills
    /// exactly the target — nothing outside it and nothing inverted.
    #[test]
    fn a_nine_slice_smaller_than_its_corners_squashes_inside_the_target() {
        let (_, image) = atlas_with(16, 16);
        let sliced = NineSliceImage {
            image,
            insets: SkinInsets::new(4.0, 8.0, 4.0, 4.0),
        };
        let mut dl = DrawList::new();
        let (min, max) = (Vec2::new(5.0, 5.0), Vec2::new(11.0, 45.0));
        dl.nine_slice(min, max, &sliced, 1.0, [1.0; 4]);

        let columns = cut_lines(&dl, |v| v.x);
        assert_eq!(columns, [5.0, 7.0, 11.0], "4:8 of six pixels, no centre");
        for command in dl.commands() {
            let DrawCommand::Image { min: a, max: b, .. } = command else {
                unreachable!()
            };
            assert!(a.x < b.x && a.y < b.y, "inverted quad {command:?}");
            assert!(a.cmpge(min).all() && b.cmple(max).all(), "{command:?}");
        }

        // And nonsense draws nothing rather than NaN geometry.
        for (scale, max) in [
            (0.0, max),
            (f32::NAN, max),
            (1.0, Vec2::new(f32::INFINITY, 45.0)),
        ] {
            let mut dl = DrawList::new();
            dl.nine_slice(min, max, &sliced, scale, [1.0; 4]);
            assert!(dl.is_empty(), "scale {scale}, max {max:?}");
        }
    }

    // -----------------------------------------------------------------------
    // Rounded rectangles
    // -----------------------------------------------------------------------

    #[test]
    fn a_rounded_rect_is_one_quad_carrying_its_shape() {
        let border = Border {
            width: 3.0,
            color: [0.0, 1.0, 0.0, 1.0],
        };
        let radii = CornerRadii {
            top_left: 2.0,
            top_right: 4.0,
            bottom_right: 6.0,
            bottom_left: 8.0,
        };
        let mut dl = DrawList::new();
        dl.rounded_rect(
            Vec2::new(10.0, 20.0),
            Vec2::new(50.0, 40.0),
            radii,
            RED,
            border,
        );
        let (vertices, indices) = dl.to_triangles(None, None, 1.0);

        assert_eq!((vertices.len(), indices.len()), (4, 6));
        for vertex in &vertices {
            assert_eq!(vertex.primitive(), Some(Primitive::RoundedRect));
            assert_eq!(vertex.color, RED);
            assert_eq!(
                vertex.shape,
                [20.0, 10.0, 3.0, Primitive::RoundedRect.lane()]
            );
            assert_eq!(vertex.radii, [2.0, 4.0, 6.0, 8.0]);
            assert_eq!(vertex.border, border.color);
            // The UV lane is the corner's offset from the centre (30, 30), so
            // the fragment stage's distance field is centred on the rectangle.
            assert_eq!(vertex.uv, vertex.pos - Vec2::new(30.0, 30.0), "{vertex:?}");
        }
    }

    /// No corner rounder than half the shorter side, no border thicker, and a
    /// radius that is negative or NaN is a square corner — the shader never
    /// sees a number that bends two corners into each other.
    #[test]
    fn radii_and_border_are_clamped_to_half_the_shorter_side() {
        let mut dl = DrawList::new();
        dl.rounded_rect(
            Vec2::ZERO,
            Vec2::new(100.0, 12.0),
            CornerRadii {
                top_left: 50.0,
                top_right: -1.0,
                bottom_right: f32::NAN,
                bottom_left: 3.0,
            },
            RED,
            Border {
                width: 40.0,
                color: RED,
            },
        );
        let (vertices, _) = dl.to_triangles(None, None, 1.0);
        assert_eq!(vertices[0].radii, [6.0, 0.0, 0.0, 3.0]);
        assert_eq!(vertices[0].shape[2], 6.0);
    }

    #[test]
    fn a_rounded_rect_with_no_area_draws_nothing() {
        for (min, max) in [
            (Vec2::ZERO, Vec2::new(0.0, 10.0)),
            (Vec2::splat(10.0), Vec2::ZERO),
            (Vec2::ZERO, Vec2::new(f32::NAN, 10.0)),
        ] {
            let mut dl = DrawList::new();
            dl.rounded_rect(min, max, CornerRadii::uniform(2.0), RED, Border::NONE);
            let (vertices, indices) = dl.to_triangles(None, None, 1.0);
            assert!(
                vertices.is_empty() && indices.is_empty(),
                "{min:?}..{max:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Clip rectangles
    // -----------------------------------------------------------------------

    fn clips_of(dl: &DrawList) -> Vec<[f32; 4]> {
        let (vertices, _) = dl.to_triangles(None, None, 1.0);
        vertices.chunks_exact(4).map(|quad| quad[0].clip).collect()
    }

    /// **Every vertex carries the clip its command went in under**, nested
    /// clips intersect, and a pop restores the one beneath — asserted on the
    /// vertices the renderer uploads, not on the stack.
    #[test]
    fn clips_nest_by_intersection_and_pop_back_to_the_one_beneath() {
        let mut dl = DrawList::new();
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        dl.push_clip(Vec2::new(10.0, 10.0), Vec2::new(100.0, 80.0));
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        dl.push_clip(Vec2::new(50.0, 0.0), Vec2::new(200.0, 60.0));
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        dl.pop_clip().expect("two were pushed");
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        dl.pop_clip().expect("one is left");
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);

        assert_eq!(
            clips_of(&dl),
            [
                ClipRect::NONE.lane(),
                [10.0, 10.0, 100.0, 80.0],
                [50.0, 10.0, 100.0, 60.0],
                [10.0, 10.0, 100.0, 80.0],
                ClipRect::NONE.lane(),
            ]
        );
        assert_eq!(dl.clips().len(), dl.len());
    }

    #[test]
    fn a_pop_with_nothing_pushed_is_an_error_and_changes_nothing() {
        let mut dl = DrawList::new();
        assert_eq!(dl.pop_clip(), Err(ClipUnderflow));
        dl.push_clip(Vec2::ZERO, Vec2::splat(10.0));
        dl.pop_clip().expect("one was pushed");
        assert_eq!(dl.pop_clip(), Err(ClipUnderflow), "and only one");
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        assert_eq!(clips_of(&dl), [ClipRect::NONE.lane()]);
    }

    /// Two clips that do not overlap leave an empty one — which clips away
    /// everything — rather than an inside-out rectangle a fragment test would
    /// read as "no clip".
    #[test]
    fn disjoint_clips_intersect_to_nothing_rather_than_inside_out() {
        let mut dl = DrawList::new();
        dl.push_clip(Vec2::ZERO, Vec2::splat(10.0));
        dl.push_clip(Vec2::splat(20.0), Vec2::splat(30.0));
        let clip = dl.clip();
        assert!(
            clip.max.x <= clip.min.x && clip.max.y <= clip.min.y,
            "{clip:?}"
        );
        assert!(clip.max.cmpge(clip.min).all(), "inside out: {clip:?}");
    }

    /// The overlay cut and `clear` both drop the stack: the engine's overlay is
    /// never clipped by a clip the game forgot to pop, and neither is next
    /// frame.
    #[test]
    fn the_overlay_cut_and_clear_drop_every_pushed_clip() {
        let mut dl = DrawList::new();
        dl.push_clip(Vec2::ZERO, Vec2::splat(10.0));
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        dl.begin_overlay();
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        assert_eq!(
            clips_of(&dl),
            [[0.0, 0.0, 10.0, 10.0], ClipRect::NONE.lane()]
        );

        dl.push_clip(Vec2::ZERO, Vec2::splat(10.0));
        dl.clear();
        dl.rect(Vec2::ZERO, Vec2::splat(4.0), RED);
        assert_eq!(clips_of(&dl), [ClipRect::NONE.lane()]);
    }
}
