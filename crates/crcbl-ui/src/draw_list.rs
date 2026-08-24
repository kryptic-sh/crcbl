//! Draw list: a sequence of UI draw commands queued for rendering.
//!
//! Each frame the UI code produces a [`DrawList`] — an ordered list of
//! commands (rectangles, text spans) that a render backend then processes
//! into GPU draw calls. The draw list is the only interface between the
//! immediate-mode UI and the renderer.

use crate::text::FontAtlas;
use crate::text::GLYPH_HEIGHT;
use glam::Vec2;

// ---------------------------------------------------------------------------
// Vertex
// ---------------------------------------------------------------------------

/// A 2D vertex for UI rendering (screen-space, no Z).
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
    /// UV coordinates into the glyph / atlas texture (0-1 range). Zero for
    /// untextured primitives.
    pub uv: Vec2,
    /// RGBA colour, each component in `[0, 1]`.
    pub color: [f32; 4],
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
}

// ---------------------------------------------------------------------------
// DrawList
// ---------------------------------------------------------------------------

/// An ordered list of draw commands for one frame.
///
/// Create one per frame, push commands into it, then hand it to the renderer.
#[derive(Debug, Clone, Default)]
pub struct DrawList {
    commands: Vec<DrawCommand>,
}

impl DrawList {
    /// Create an empty draw list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Push a filled rectangle command.
    pub fn rect(&mut self, min: Vec2, max: Vec2, color: [f32; 4]) {
        self.commands.push(DrawCommand::Rect { min, max, color });
    }

    /// Push a rectangle outline command.
    pub fn rect_outline(&mut self, min: Vec2, max: Vec2, thickness: f32, color: [f32; 4]) {
        self.commands.push(DrawCommand::RectOutline {
            min,
            max,
            thickness,
            color,
        });
    }

    /// Push a straight line segment, stroked centred on the segment.
    pub fn line(&mut self, from: Vec2, to: Vec2, thickness: f32, color: [f32; 4]) {
        self.commands.push(DrawCommand::Line {
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
        self.commands.push(DrawCommand::Polyline {
            points: points.into(),
            thickness,
            closed,
            color,
        });
    }

    /// Push a text command.
    pub fn text(&mut self, pos: Vec2, text: impl Into<String>, color: [f32; 4], size: f32) {
        self.commands.push(DrawCommand::Text {
            pos,
            text: text.into(),
            color,
            size,
        });
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

    /// Clear all commands (reuse the allocation across frames).
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Expand every draw command into screen-space triangles.
    ///
    /// Returns `(vertices, indices)` in a format a render backend can upload
    /// directly. Each `Rect` becomes one quad (4 vertices, 6 indices).
    /// `RectOutline` becomes 4 thin quads — one per side — forming a hollow
    /// border. `Line` and `Polyline` become one quad per segment plus one
    /// bevel triangle per corner.
    ///
    /// `Text` commands are expanded when `atlas` is `Some`: each glyph becomes
    /// one textured quad with UV coordinates into the atlas. When `atlas` is
    /// `None`, text commands are skipped (the `to_triangles` return from S6).
    ///
    /// `scale` is a multiplier on the font size (1.0 = baked-in 8×13 px).
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
    pub fn to_triangles(&self, atlas: Option<&FontAtlas>, scale: f32) -> (Vec<Vertex2d>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for cmd in &self.commands {
            match cmd {
                DrawCommand::Rect { min, max, color } => {
                    push_quad(
                        *min,
                        *max,
                        Vec2::ZERO,
                        Vec2::ZERO,
                        *color,
                        &mut vertices,
                        &mut indices,
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
                            Vec2::ZERO,
                            Vec2::ZERO,
                            c,
                            &mut vertices,
                            &mut indices,
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
                    push_stroke(
                        &[*from, *to],
                        false,
                        *thickness,
                        *color,
                        &mut vertices,
                        &mut indices,
                    );
                }
                DrawCommand::Polyline {
                    points,
                    thickness,
                    closed,
                    color,
                } => {
                    push_stroke(
                        points,
                        *closed,
                        *thickness,
                        *color,
                        &mut vertices,
                        &mut indices,
                    );
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
                                uv_min,
                                uv_max,
                                *color,
                                &mut vertices,
                                &mut indices,
                            );
                        }
                    }
                }
            }
        }
        (vertices, indices)
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
        vertices.push(Vertex2d {
            pos,
            uv: Vec2::ZERO,
            color,
        });
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
        vertices.push(Vertex2d {
            pos,
            uv: Vec2::ZERO,
            color,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Push one axis-aligned quad — 4 vertices, 6 indices — for use by
/// [`DrawList::to_triangles`].
///
/// `min`/`max` are the top-left and bottom-right corners in the Y-down screen
/// convention, and `uv_min`/`uv_max` the matching atlas corners (both
/// [`Vec2::ZERO`] for untextured primitives, which the fragment shader reads as
/// "do not sample").
///
/// Vertices are emitted bottom-left, bottom-right, top-right, top-left, which
/// after the shader's Y flip is counter-clockwise in NDC. Nothing depends on
/// that today — `crcbl-render`'s UI pass uses `PrimitiveState::default()` with
/// no face culling — but the order is fixed here rather than per-call-site so
/// there is only one thing to change if it ever does.
fn push_quad(
    min: Vec2,
    max: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
    color: [f32; 4],
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
        vertices.push(Vertex2d { pos, uv, color });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

// SAFETY: Vertex2d is `#[repr(C)]` with only f32 fields (Vec2 is 2×f32,
// [f32; 4] is 4×f32). No padding, all bit patterns valid.
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
        let (verts, indices) = dl.to_triangles(None, 1.0);
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
        let (verts, indices) = dl.to_triangles(None, 1.0);

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
        let (verts, indices) = dl.to_triangles(None, 1.0);

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
        let (verts, indices) = dl.to_triangles(None, 1.0);
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
        let (verts, indices) = dl.to_triangles(None, 1.0);

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
        let (verts, indices) = dl.to_triangles(Some(&atlas), 1.0);

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
        let (verts, _) = dl.to_triangles(Some(&atlas), 1.0);
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
        let (verts, _) = dl.to_triangles(None, 1.0);

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
        let (verts, _) = dl.to_triangles(None, 1.0);

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
        let (verts, indices) = dl.to_triangles(Some(&atlas), 1.0);
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

        let (verts, indices) = dl.to_triangles(Some(&atlas), 1.0);

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
        let (verts2, indices2) = dl2.to_triangles(Some(&atlas), 1.0);
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
        let (verts, indices) = dl.to_triangles(None, 1.0);
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
        let (vertices, indices) = dl.to_triangles(None, 1.0);

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
        let (vertices, _) = dl.to_triangles(None, 1.0);

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
        let (vertices, indices) = dl.to_triangles(None, 1.0);

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

        let (_, open_indices) = open.to_triangles(None, 1.0);
        let (closed_vertices, closed_indices) = closed.to_triangles(None, 1.0);
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
        let (vertices, indices) = dl.to_triangles(None, 1.0);

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
            let (vertices, indices) = list.to_triangles(None, 1.0);
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
        let (vertices, indices) = dl.to_triangles(None, 1.0);

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
}
