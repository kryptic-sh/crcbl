//! Immediate-mode GUI toolkit for Crucible.
//!
//! Each frame the UI code produces a [`DrawList`] — a sequence of draw commands
//! (rectangles, pictures, text spans) that a render backend processes. Text is
//! rendered from the built-in [`FontAtlas`] which provides a monospace bitmap
//! font, or from a parsed font through [`font`]'s glyph atlas, and pictures from
//! an [`ImageAtlas`] the caller registers RGBA8 images into.
//!
//! # Architecture
//!
//! ```text
//! Debug overlay ([`debug`])     ←  the modular profiler panel
//!      │
//! On-screen controls ([`touch`]) ←  the widgets a finger drives
//!      │
//! Debug console ([`console`])  ←  the panel the `` ` `` key drops down
//!      │
//! Widgets (Label, Button, …)   ←  this slice (P4-b)
//!      │
//!      ▼
//! DrawList + FontAtlas         ←  first slice (P4-a)
//!   + ImageAtlas                ←  textured quads, rounded rects, clips
//!      │
//!      ▼
//! Render backend            ← crcbl-render's ui_pass
//! ```
//!
//! [`debug`] is the one panel every sample switches on: frame timing and FPS
//! always, and a section per system that has one to contribute. It names no
//! system — see that module's docs. [`budget`] is the profiler row that sits
//! beside it once there is anything to put in it: CPU against GPU frame time,
//! and which of the two the frame is costing. [`readout`] is the panel the 3D
//! samples put their own numbers in — a label and a right-aligned reading a
//! row, sized and coloured by the page rather than by its contents.
//!
//! [`tree`] is the element tree `docs/plan/07-ui-debug.md` builds the rest of
//! the toolkit on: blocks and spans rebuilt every frame with identity that
//! survives the rebuild, laid out by Taffy's flexbox and emitted into the same
//! [`DrawList`]. [`readout`] is its first consumer.
//!
//! The draw list is the only interface between the UI and the renderer. The
//! render backend takes a [`DrawList`] and emits GPU draw calls.
//!
//! See `docs/plan/07-ui-debug.md` for the full design.

pub mod budget;
pub mod console;
pub mod debug;
pub mod draw_list;
pub mod font;
pub mod hud;
pub mod image;
pub mod menu;
pub mod readout;
pub mod style;
pub mod text;
pub mod touch;
pub mod tree;
pub mod widget;

pub use budget::{Bound, BudgetStats, MIN_PERCENTILE_SAMPLES};
pub use console::{
    CARET_BLINK, COMPLETION_ROWS, CONSOLE_HEIGHT_FRACTION, ConsoleInput, ConsoleLayout,
    ConsolePanel, ConsoleStyle, KEY_ID_BASE, KEY_ID_SPAN, KEYBOARD_HEIGHT_FRACTION, KeyBox, KeyCap,
    KeyboardLayout, Layer, LogLine, LogView, MINIMUM_FIELD_COLUMNS, MINIMUM_LOG_ROWS, PROMPT,
    SEND_ID, SEND_LABEL, TextField, TextFieldStyle, TouchKeyboard, caret_shown,
};
pub use debug::{
    DEFAULT_FRAME_WINDOW, DebugModule, DebugOverlay, DebugPanel, DebugRow, DebugSection,
    DebugStyle, FrameStats,
};
pub use draw_list::{
    Border, ClipRect, ClipUnderflow, CornerRadii, DrawCommand, DrawList, Primitive, Triangles,
    Vertex2d,
};
pub use hud::Anchor;
pub use image::{AtlasError, AtlasImage, ImageAtlas, ImageId, NineSliceImage, TexelRect};
pub use menu::{
    BUTTON_INSETS, Cycler, FIT_FRACTION, Menu, MenuItem, MenuItemKind, MenuItemLayout, MenuLayout,
    MenuSet, MenuSkin, MenuStyle, PANEL_INSETS, Slider,
};
pub use readout::{NATURAL_SCALE, ReadoutPanel, ReadoutRow};
pub use text::{
    ASCENDER, ASCII_GLYPH_COUNT, FIRST_CHAR, FontAtlas, GLYPH_ADVANCE, GLYPH_COUNT, GLYPH_HEIGHT,
    GLYPH_WIDTH, GlyphMetrics, LAST_CHAR, LINE_HEIGHT, NOTDEF_INDEX, glyph_index,
};
pub use touch::{TouchButton, TouchStick};
pub use widget::{
    Button, ButtonSkin, ButtonState, Label, NATURAL_FONT_SIZE, PointerInput, SkinInsets, Style,
    UiState, WidgetId,
};
