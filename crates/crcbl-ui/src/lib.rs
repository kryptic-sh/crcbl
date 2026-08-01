//! Immediate-mode GUI toolkit for Crucible.
//!
//! Each frame the UI code produces a [`DrawList`] — a sequence of draw commands
//! (rectangles, text spans) that a render backend processes. Text is rendered
//! from the built-in [`FontAtlas`] which provides a monospace bitmap font.
//!
//! # Architecture
//!
//! ```text
//! Widgets (Label, Button, …)   ←  this slice (P4-b)
//!      │
//!      ▼
//! DrawList + FontAtlas         ←  first slice (P4-a)
//!      │
//!      ▼
//! Render backend (future)
//! ```
//!
//! The draw list is the only interface between the UI and the renderer. The
//! render backend takes a [`DrawList`] and emits GPU draw calls.
//!
//! See `docs/plan/07-ui-debug.md` for the full design.

pub mod draw_list;
pub mod hud;
pub mod text;
pub mod widget;

pub use draw_list::{DrawCommand, DrawList, Vertex2d};
pub use hud::{Anchor, Hud, HudPanel};
pub use text::{
    ASCENDER, ASCII_GLYPH_COUNT, FIRST_CHAR, FontAtlas, GLYPH_ADVANCE, GLYPH_COUNT, GLYPH_HEIGHT,
    GLYPH_WIDTH, GlyphMetrics, LAST_CHAR, LINE_HEIGHT, NOTDEF_INDEX, glyph_index,
};
pub use widget::{
    Button, ButtonState, Label, NATURAL_FONT_SIZE, PointerInput, Style, UiState, WidgetId,
};
