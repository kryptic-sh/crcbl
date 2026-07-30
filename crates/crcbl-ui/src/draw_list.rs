//! Draw list: a sequence of UI draw commands queued for rendering.
//!
//! Each frame the UI code produces a [`DrawList`] — an ordered list of
//! commands (rectangles, text spans) that a render backend then processes
//! into GPU draw calls. The draw list is the only interface between the
//! immediate-mode UI and the renderer.

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
}
