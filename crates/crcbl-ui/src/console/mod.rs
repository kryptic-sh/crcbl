//! The debug console's widgets: an editable line, a view of the log, and the
//! panel that lays the two out.
//!
//! `docs/plan/52-debug-console.md` decision 6 is the design. Everything here is
//! **data in, draw list out**: the panel is handed the log records it should
//! show and the completion candidates it should offer, and it reports the line
//! that was submitted. It reads no ring, no registry and no keyboard — the
//! engine's slice owns all three — so the whole console panel is testable with
//! nothing running.
//!
//! ```text
//! ConsolePanel ── layout(extent, atlas) ─→ ConsoleLayout ─→ render(&mut DrawList)
//!    ├── LogView    the records, wrapped, coloured by level, newest at the bottom
//!    └── TextField  the line being typed, and its caret
//! ```
//!
//! The panel is the top [`CONSOLE_HEIGHT_FRACTION`] of the frame, Source's
//! drop-down, and is laid out at [`MenuStyle::pixel_art`]'s whole-number scale
//! so the glyphs land on pixel boundaries at every window size.
//!
//! [`MenuStyle::pixel_art`]: crate::menu::MenuStyle::pixel_art

mod field;
mod log_view;
mod panel;

use core::time::Duration;

use glam::Vec2;

use crcbl_core::log::Level;

use crate::text::{FontAtlas, LINE_HEIGHT};
use crate::widget::{NATURAL_FONT_SIZE, Style};

pub use field::{TextField, TextFieldStyle};
pub use log_view::{LogLine, LogView};
pub use panel::{
    COMPLETION_ROWS, CONSOLE_HEIGHT_FRACTION, ConsoleLayout, ConsolePanel, MINIMUM_FIELD_COLUMNS,
    MINIMUM_LOG_ROWS, PROMPT, SEND_ID, SEND_LABEL,
};

/// How long the caret spends shown, and then hidden, before it repeats.
///
/// The interval a terminal caret has blinked at since the DEC VT100, and slow
/// enough that a still frame of a screenshot usually catches the caret rather
/// than the gap.
pub const CARET_BLINK: Duration = Duration::from_millis(530);

/// Whether the caret is in its shown half of the blink at `elapsed` into the
/// run.
///
/// The console's own clock is the caller's — the panel draws no frames of its
/// own and holds no `Instant`, which is also what makes a test able to draw
/// both halves of the blink without waiting for one.
#[must_use]
pub fn caret_shown(elapsed: Duration) -> bool {
    let period = CARET_BLINK.as_nanos().max(1);
    (elapsed.as_nanos() / period).is_multiple_of(2)
}

/// Every colour and length the console panel is drawn with, in **pixels**.
///
/// Built from a whole-number `scale` by [`ConsoleStyle::pixel_art`], for the
/// reason [`MenuStyle::pixel_art`] gives: the glyphs are an 8x13 bitmap and a
/// fractional scale draws them through a filter.
///
/// [`MenuStyle::pixel_art`]: crate::menu::MenuStyle::pixel_art
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsoleStyle {
    /// Device pixels per texel of the console's own metrics.
    pub scale: f32,
    /// The font size every line in the panel is drawn at.
    ///
    /// One size for the log, the prompt, the typed line and the candidates: a
    /// console is a terminal, and a terminal's columns line up.
    pub text_size: f32,
    /// Space between the panel's edge and its contents.
    pub padding: Vec2,
    /// How wide the caret bar is drawn.
    pub caret_width: f32,
    /// The panel's own fill — the scrim the frame behind it is dimmed with.
    pub panel_color: [f32; 4],
    /// The line along the panel's bottom edge, and the field's outline.
    pub border_color: [f32; 4],
    /// The input row's fill, behind the prompt and the typed line.
    pub field_color: [f32; 4],
    /// The `]` prompt's colour.
    pub prompt_color: [f32; 4],
    /// The typed line's colour.
    pub text_color: [f32; 4],
    /// The caret's colour.
    pub caret_color: [f32; 4],
    /// A record logged at [`Level::Error`].
    pub error_color: [f32; 4],
    /// A record logged at [`Level::Warn`].
    pub warn_color: [f32; 4],
    /// A record logged at [`Level::Info`], which is most of them.
    pub info_color: [f32; 4],
    /// A record logged at [`Level::Debug`].
    pub debug_color: [f32; 4],
    /// A record logged at [`Level::Trace`].
    pub trace_color: [f32; 4],
    /// The completion list's fill.
    pub completion_color: [f32; 4],
    /// The part of a candidate past the prefix that was matched.
    pub candidate_color: [f32; 4],
    /// The matched prefix at the head of every candidate, which is what says
    /// why the list holds what it holds.
    pub match_color: [f32; 4],
    /// The **Send** button's palette, which is [`crate::widget::Button`]'s own.
    pub button: Style,
}

impl ConsoleStyle {
    /// The shipped look, at `scale` device pixels per texel.
    ///
    /// `scale` is clamped to at least one, for the reason
    /// [`MenuStyle::pixel_art`] gives: a panel drawn at zero is not a smaller
    /// panel, it is an invisible one.
    ///
    /// [`MenuStyle::pixel_art`]: crate::menu::MenuStyle::pixel_art
    #[must_use]
    pub fn pixel_art(scale: u32) -> Self {
        let scale = scale.max(1) as f32;
        Self {
            scale,
            text_size: NATURAL_FONT_SIZE * scale,
            padding: Vec2::new(6.0, 4.0) * scale,
            caret_width: 2.0 * scale,
            // Nearly opaque, unlike the menu's two-thirds scrim: the console is
            // read while the game runs behind it, and a log line over a moving
            // frame is a log line nobody can read.
            panel_color: [0.04, 0.05, 0.08, 0.92],
            border_color: [0.42, 0.46, 0.72, 1.0],
            field_color: [0.10, 0.11, 0.18, 1.0],
            prompt_color: [0.62, 0.64, 0.82, 1.0],
            text_color: [0.94, 0.95, 1.0, 1.0],
            caret_color: [0.94, 0.95, 1.0, 1.0],
            // The five levels read as a temperature: red, amber, plain, then two
            // greys the eye passes over, so a warning in a wall of debug lines
            // is found without reading it.
            error_color: [1.0, 0.44, 0.40, 1.0],
            warn_color: [1.0, 0.78, 0.35, 1.0],
            info_color: [0.90, 0.92, 1.0, 1.0],
            debug_color: [0.66, 0.70, 0.82, 1.0],
            trace_color: [0.50, 0.54, 0.66, 1.0],
            completion_color: [0.08, 0.09, 0.14, 0.98],
            candidate_color: [0.80, 0.83, 0.95, 1.0],
            match_color: [1.0, 0.94, 0.55, 1.0],
            button: Style {
                text: [0.94, 0.95, 1.0, 1.0],
                bg: [0.16, 0.18, 0.30, 1.0],
                bg_hover: [0.24, 0.27, 0.44, 1.0],
                bg_active: [0.34, 0.38, 0.60, 1.0],
                border: [0.42, 0.46, 0.72, 1.0],
            },
        }
    }

    /// The colour a record of `level` is drawn in.
    #[must_use]
    pub fn level_color(&self, level: Level) -> [f32; 4] {
        match level {
            Level::Error => self.error_color,
            Level::Warn => self.warn_color,
            Level::Info => self.info_color,
            Level::Debug => self.debug_color,
            Level::Trace => self.trace_color,
        }
    }

    /// How tall one line of text is, and so how far apart two rows sit.
    #[must_use]
    pub fn row_height(&self) -> f32 {
        LINE_HEIGHT * self.glyph_scale()
    }

    /// How wide one column is.
    ///
    /// One number for every glyph because the built-in atlas is **monospace**:
    /// that is what lets the log wrap by counting characters instead of
    /// measuring each line, and it is measured off the atlas rather than
    /// assumed so a proportional atlas would move this figure rather than
    /// silently make the wrap wrong.
    #[must_use]
    pub fn advance(&self, atlas: &FontAtlas) -> f32 {
        atlas.text_width("M", self.glyph_scale())
    }

    /// How a [`TextField`] in this panel draws itself.
    #[must_use]
    pub fn field_style(&self) -> TextFieldStyle {
        TextFieldStyle {
            size: self.text_size,
            text_color: self.text_color,
            caret_color: self.caret_color,
            caret_width: self.caret_width,
        }
    }

    /// The multiplier from the atlas's baked-in size to this style's.
    fn glyph_scale(&self) -> f32 {
        self.text_size / NATURAL_FONT_SIZE
    }
}

impl Default for ConsoleStyle {
    fn default() -> Self {
        Self::pixel_art(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caret is shown for exactly half of each blink and hidden for the
    /// other half, from the first frame of the run onwards.
    #[test]
    fn the_caret_blinks_on_and_off_at_the_declared_interval() {
        assert!(caret_shown(Duration::ZERO), "the caret starts shown");
        assert!(caret_shown(CARET_BLINK - Duration::from_millis(1)));
        assert!(!caret_shown(CARET_BLINK));
        assert!(!caret_shown(CARET_BLINK * 2 - Duration::from_millis(1)));
        assert!(caret_shown(CARET_BLINK * 2), "the blink did not repeat");
    }

    /// Every length grows with the scale and every colour stays put, which is
    /// what "the same panel, drawn bigger" means.
    #[test]
    fn a_bigger_scale_is_a_bigger_panel_and_not_a_different_one() {
        let one = ConsoleStyle::pixel_art(1);
        let three = ConsoleStyle::pixel_art(3);
        assert_eq!(three.text_size, one.text_size * 3.0);
        assert_eq!(three.row_height(), one.row_height() * 3.0);
        assert_eq!(three.padding, one.padding * 3.0);
        assert_eq!(three.caret_width, one.caret_width * 3.0);
        assert_eq!(three.panel_color, one.panel_color);
        assert_eq!(three.button, one.button);
        assert_eq!(
            ConsoleStyle::pixel_art(0),
            one,
            "a zero scale is an invisible panel, not a smaller one",
        );
    }

    /// The five levels are five different colours, so a level is readable from
    /// the line alone — the panel draws no level name.
    #[test]
    fn every_level_has_a_colour_of_its_own() {
        let style = ConsoleStyle::default();
        let mut seen: Vec<[f32; 4]> = Vec::new();
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            let color = style.level_color(level);
            assert!(
                !seen.contains(&color),
                "{level} shares its colour with another level",
            );
            seen.push(color);
        }
    }

    /// A column is one glyph advance wide, at every scale, because that is what
    /// the log's wrap counts in.
    #[test]
    fn a_column_is_one_glyph_advance_at_every_scale() {
        let atlas = FontAtlas::built_in();
        for scale in 1..=4 {
            let style = ConsoleStyle::pixel_art(scale);
            let advance = style.advance(&atlas);
            assert!(
                (advance - crate::text::GLYPH_ADVANCE * scale as f32).abs() < 1e-3,
                "scale {scale} makes a column {advance} pixels wide",
            );
            for glyph in ["i", " ", "W"] {
                assert!(
                    (atlas.text_width(glyph, style.text_size / NATURAL_FONT_SIZE) - advance).abs()
                        < 1e-3,
                    "{glyph:?} is not one column wide, so the wrap cannot count",
                );
            }
        }
    }
}
