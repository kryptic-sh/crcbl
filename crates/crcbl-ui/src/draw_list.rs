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
#[derive(Debug, Clone, Copy, PartialEq)]
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
        /// Top-left corner in screen-space.
        min: Vec2,
        /// Bottom-right corner in screen-space.
        max: Vec2,
        /// RGBA fill colour.
        color: [f32; 4],
    },
    /// A rectangle outline (border).
    RectOutline {
        min: Vec2,
        max: Vec2,
        /// Line thickness in pixels.
        thickness: f32,
        color: [f32; 4],
    },
    /// A single line of text rendered from the glyph atlas.
    Text {
        /// Top-left anchor of the text.
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
    /// border.
    ///
    /// `Text` commands are expanded when `atlas` is `Some`: each glyph becomes
    /// one textured quad with UV coordinates into the atlas. When `atlas` is
    /// `None`, text commands are skipped (the `to_triangles` return from S6).
    ///
    /// `scale` is a multiplier on the font size (1.0 = baked-in 8×13 px); also
    /// multiplied into position for text commands.
    ///
    /// The index buffer uses `u32` indices; callers that need `u16` must
    /// adapt. Vertex positions are in screen-space pixels with Y-up (the UI
    /// convention); the renderer applies an orthographic projection.
    #[must_use]
    pub fn to_triangles(&self, atlas: Option<&FontAtlas>, scale: f32) -> (Vec<Vertex2d>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for cmd in &self.commands {
            match cmd {
                DrawCommand::Rect { min, max, color } => {
                    let base = vertices.len() as u32;
                    // clockwise winding: top-left, top-right, bottom-right, bottom-left
                    vertices.push(Vertex2d {
                        pos: Vec2::new(min.x, max.y),
                        uv: Vec2::ZERO,
                        color: *color,
                    });
                    vertices.push(Vertex2d {
                        pos: Vec2::new(max.x, max.y),
                        uv: Vec2::ZERO,
                        color: *color,
                    });
                    vertices.push(Vertex2d {
                        pos: Vec2::new(max.x, min.y),
                        uv: Vec2::ZERO,
                        color: *color,
                    });
                    vertices.push(Vertex2d {
                        pos: Vec2::new(min.x, min.y),
                        uv: Vec2::ZERO,
                        color: *color,
                    });
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
                DrawCommand::RectOutline {
                    min,
                    max,
                    thickness,
                    color,
                } => {
                    // Four narrow quads: top, bottom, left, right. Each quad is
                    // inset so the border sits *inside* the declared bounds.
                    let t = *thickness;
                    let inner_min = Vec2::new(min.x + t, min.y + t);
                    let inner_max = Vec2::new(max.x - t, max.y - t);
                    let c = *color;

                    // top edge
                    push_quad(
                        Vec2::new(min.x, max.y),
                        Vec2::new(max.x, max.y),
                        Vec2::new(inner_max.x, inner_max.y),
                        Vec2::new(inner_min.x, inner_max.y),
                        c,
                        &mut vertices,
                        &mut indices,
                    );
                    // bottom edge
                    push_quad(
                        Vec2::new(min.x, inner_min.y),
                        Vec2::new(max.x, inner_min.y),
                        Vec2::new(max.x, min.y),
                        Vec2::new(min.x, min.y),
                        c,
                        &mut vertices,
                        &mut indices,
                    );
                    // left edge
                    push_quad(
                        Vec2::new(min.x, inner_max.y),
                        Vec2::new(inner_min.x, inner_max.y),
                        Vec2::new(inner_min.x, inner_min.y),
                        Vec2::new(min.x, inner_min.y),
                        c,
                        &mut vertices,
                        &mut indices,
                    );
                    // right edge
                    push_quad(
                        Vec2::new(inner_max.x, inner_max.y),
                        Vec2::new(max.x, inner_max.y),
                        Vec2::new(max.x, inner_min.y),
                        Vec2::new(inner_max.x, inner_min.y),
                        c,
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
                            let u_min = atlas.glyph_u_min(c);
                            let u_max = atlas.glyph_u_max(c);
                            let v_min = 0.0;
                            let v_max = 1.0;
                            let base = vertices.len() as u32;
                            vertices.push(Vertex2d {
                                pos: Vec2::new(min.x, max.y),
                                uv: Vec2::new(u_min, v_min),
                                color: *color,
                            });
                            vertices.push(Vertex2d {
                                pos: Vec2::new(max.x, max.y),
                                uv: Vec2::new(u_max, v_min),
                                color: *color,
                            });
                            vertices.push(Vertex2d {
                                pos: Vec2::new(max.x, min.y),
                                uv: Vec2::new(u_max, v_max),
                                color: *color,
                            });
                            vertices.push(Vertex2d {
                                pos: Vec2::new(min.x, min.y),
                                uv: Vec2::new(u_min, v_max),
                                color: *color,
                            });
                            indices.extend_from_slice(&[
                                base,
                                base + 1,
                                base + 2,
                                base,
                                base + 2,
                                base + 3,
                            ]);
                        }
                    }
                }
            }
        }
        (vertices, indices)
    }
}

/// Push a quad (4 vertices, 6 indices) for use by [`DrawList::to_triangles`].
fn push_quad(
    a: Vec2,
    b: Vec2,
    c: Vec2,
    d: Vec2,
    color: [f32; 4],
    vertices: &mut Vec<Vertex2d>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    vertices.push(Vertex2d {
        pos: a,
        uv: Vec2::ZERO,
        color,
    });
    vertices.push(Vertex2d {
        pos: b,
        uv: Vec2::ZERO,
        color,
    });
    vertices.push(Vertex2d {
        pos: c,
        uv: Vec2::ZERO,
        color,
    });
    vertices.push(Vertex2d {
        pos: d,
        uv: Vec2::ZERO,
        color,
    });
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn into_commands_consumes() {
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

        // Vertices are CCW from top-left.
        assert_eq!(verts[0].pos, Vec2::new(10.0, 120.0)); // top-left
        assert_eq!(verts[1].pos, Vec2::new(110.0, 120.0)); // top-right
        assert_eq!(verts[2].pos, Vec2::new(110.0, 20.0)); // bottom-right
        assert_eq!(verts[3].pos, Vec2::new(10.0, 20.0)); // bottom-left

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
        // v goes from 0 (top of atlas) to 1 (bottom).
        assert!((verts[0].uv.y - 0.0).abs() < 0.001);
        assert!((verts[2].uv.y - 1.0).abs() < 0.001);

        // All verts share the text colour.
        for v in &verts {
            assert_eq!(v.color, [1.0, 0.0, 0.0, 1.0]);
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
}
