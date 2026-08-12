//! Immediate-mode GUI toolkit for Crucible.
//!
//! Each frame the UI code produces a [`DrawList`] — a sequence of draw commands
//! (rectangles, text spans) that a render backend processes. Text is rendered
//! from the built-in [`FontAtlas`] which provides a monospace bitmap font.
//!
//! # Architecture
//!
//! ```text
//! Debug overlay ([`debug`])     ←  the modular profiler panel
//!      │
//! Widgets (Label, Button, …)   ←  this slice (P4-b)
//!      │
//!      ▼
//! DrawList + FontAtlas         ←  first slice (P4-a)
//!      │
//!      ▼
//! Render backend (future)
//! ```
//!
//! [`debug`] is the one panel every sample switches on: frame timing and FPS
//! always, and a section per system that has one to contribute. It names no
//! system — see that module's docs. [`budget`] is the profiler row that sits
//! beside it once there is anything to put in it: CPU against GPU frame time,
//! and which of the two the frame is costing.
//!
//! The draw list is the only interface between the UI and the renderer. The
//! render backend takes a [`DrawList`] and emits GPU draw calls.
//!
//! See `docs/plan/07-ui-debug.md` for the full design.

pub mod budget;
pub mod debug;
pub mod draw_list;
pub mod hud;
pub mod menu;
pub mod text;
pub mod widget;

pub use budget::{Bound, BudgetStats, MIN_PERCENTILE_SAMPLES};
pub use debug::{
    DEFAULT_FRAME_WINDOW, DebugModule, DebugOverlay, DebugPanel, DebugRow, DebugSection,
    DebugStyle, FrameStats,
};
pub use draw_list::{DrawCommand, DrawList, Vertex2d};
pub use hud::{Anchor, Hud, HudPanel};
pub use menu::{
    BUTTON_INSETS, FIT_FRACTION, Menu, MenuItem, MenuItemLayout, MenuLayout, MenuSet, MenuStyle,
    PANEL_INSETS,
};
pub use text::{
    ASCENDER, ASCII_GLYPH_COUNT, FIRST_CHAR, FontAtlas, GLYPH_ADVANCE, GLYPH_COUNT, GLYPH_HEIGHT,
    GLYPH_WIDTH, GlyphMetrics, LAST_CHAR, LINE_HEIGHT, NOTDEF_INDEX, glyph_index,
};
pub use widget::{
    Button, ButtonState, Label, NATURAL_FONT_SIZE, PointerInput, SkinInsets, Style, UiState,
    WidgetId,
};
