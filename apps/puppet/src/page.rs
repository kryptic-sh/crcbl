//! The overlay: a readout panel and the control hint, over the 3D frame.
//!
//! ```text
//!  ┌ puppet ──────────┐
//!  │ X        0.00 m  │
//!  │ Y        0.30 m  │
//!  │ Z       -2.71 m  │
//!  │ GROUND      YES  │
//!  │ PILOT    PLAYER  │
//!  └──────────────────┘
//!
//!            W/A/S/D walk   Q/E turn the camera   R/F tilt it
//! ```
//!
//! # It is small on purpose
//!
//! The subject of this sample is what the character is doing in the world, and
//! anything drawn over that is in the way of it. The panel carries the three
//! numbers a reader needs to check the frame against — where the feet are, and
//! whether the controller says they are on the ground — and the debug panel
//! (`F3`) carries the rest.
//!
//! # Laid out against the surface
//!
//! Every position is derived from the extent the swapchain was actually
//! acquired at, so the page is correct in a resized window and in the headless
//! offscreen ring at whatever `--size` asked for.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::game::RenderState;

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
/// What a row is drawn in when the controller is refusing the move — the one
/// piece of state on this panel that is worth a colour.
const REFUSED: [f32; 4] = [0.95, 0.55, 0.36, 1.0];

/// The panel's inset from the top-left corner, in pixels.
const PANEL_INSET: f32 = 16.0;
/// The height of one row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// How wide the panel is, in pixels. Wide enough for the longest label and a
/// right-aligned reading beside it.
const PANEL_WIDTH: f32 = 168.0;
/// How thick the panel's border is, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// The scale [`FontAtlas::text_width`] is measured at, which is a multiplier on
/// the baked glyph size rather than a size in pixels. The page draws at the
/// font's natural size, so it measures at the natural scale.
const NATURAL_SCALE: f32 = 1.0;

/// The control hint, which is the whole of what a first-time visitor needs.
const HINT: &str = "W/A/S/D walk   Q/E turn the camera   R/F tilt it";

/// What the page drew, for the loop's own tests and its summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// Draws the overlay into `list`, laid out against a surface of `extent`.
///
/// `atlas` is only measured against — the glyphs themselves are the UI pass's
/// business — and it is what right-aligns the readings against a proportional
/// font rather than against a guess. Its measurements take a scale relative to
/// the baked glyph size, not a pixel size, so everything here is drawn at
/// [`NATURAL_FONT_SIZE`] and measured at the natural scale of `1.0`.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    state: &RenderState,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    let rows: [(&str, String, [f32; 4]); 5] = [
        ("X", format!("{:.2} m", state.position.x), VALUE),
        ("Y", format!("{:.2} m", state.feet), VALUE),
        ("Z", format!("{:.2} m", state.position.z), VALUE),
        (
            "GROUND",
            (if state.grounded { "YES" } else { "NO" }).to_string(),
            if state.grounded { VALUE } else { REFUSED },
        ),
        (
            "PILOT",
            (if state.patrolling {
                "CIRCUIT"
            } else {
                "PLAYER"
            })
            .to_string(),
            if state.blocked { REFUSED } else { VALUE },
        ),
    ];

    let panel_height = 2.0f32.mul_add(PANEL_PAD, rows.len() as f32 * ROW_HEIGHT);
    let min = Vec2::new(PANEL_INSET, PANEL_INSET);
    let max = Vec2::new(PANEL_INSET + PANEL_WIDTH, PANEL_INSET + panel_height);
    list.rect(min, max, PANEL_BG);
    list.rect_outline(min, max, BORDER_WIDTH, BORDER);

    for (index, (label, value, colour)) in rows.iter().enumerate() {
        let y = min.y + PANEL_PAD + index as f32 * ROW_HEIGHT;
        list.text(
            Vec2::new(min.x + PANEL_PAD, y),
            (*label).to_string(),
            LABEL,
            NATURAL_FONT_SIZE,
        );
        let reading_width = atlas.text_width(value, NATURAL_SCALE);
        list.text(
            Vec2::new(max.x - PANEL_PAD - reading_width, y),
            value.clone(),
            *colour,
            NATURAL_FONT_SIZE,
        );
    }

    let hint_width = atlas.text_width(HINT, NATURAL_SCALE);
    list.text(
        Vec2::new(
            (width - hint_width) * 0.5,
            height - PANEL_INSET - ROW_HEIGHT,
        ),
        HINT.to_string(),
        LABEL,
        NATURAL_FONT_SIZE,
    );

    PageStats {
        commands: list.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::DVec3;

    /// **The page draws something, and what it draws says where the character
    /// is.** A frame with an empty draw list is the one failure a headless
    /// smoke test would otherwise report as a pass.
    #[test]
    fn the_panel_carries_the_position_the_frame_was_drawn_at() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let state = RenderState {
            position: DVec3::new(1.25, 0.9, -2.5),
            feet: 0.3,
            grounded: true,
            ..RenderState::default()
        };
        let stats = draw(&mut list, &atlas, (960, 720), &state);
        assert!(stats.commands > 0, "the page drew nothing at all");

        let text: Vec<&str> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            text.contains(&"1.25 m"),
            "the X reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"0.30 m"),
            "the Y reading is the feet, not the capsule centre: {text:?}"
        );
        assert!(
            text.contains(&"-2.50 m"),
            "the Z reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"YES"),
            "the ground reading is missing: {text:?}"
        );
        assert!(
            text.contains(&HINT),
            "the control hint is missing: {text:?}"
        );
    }

    /// **A reading that is measured wrong is drawn off the surface, and the
    /// draw list still contains it.** Asserting the strings reach the list says
    /// nothing about where they land, so this asserts the geometry: every
    /// string the page emits starts inside the surface it was laid out
    /// against, and the right-aligned readings start inside their own panel.
    #[test]
    fn every_reading_is_laid_out_where_it_can_actually_be_seen() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let state = RenderState {
            position: DVec3::new(-12.75, 0.9, -2.5),
            feet: 0.3,
            grounded: false,
            ..RenderState::default()
        };
        let extent = (960u32, 720u32);
        draw(&mut list, &atlas, extent, &state);

        let drawn: Vec<(Vec2, &str)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { pos, text, .. } => {
                    Some((*pos, text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert!(!drawn.is_empty(), "the page drew no text at all");

        for (pos, text) in &drawn {
            assert!(
                pos.x >= 0.0 && pos.y >= 0.0,
                "{text:?} starts off the top-left corner at {pos:?}"
            );
            let end = pos.x + atlas.text_width(text, NATURAL_SCALE);
            assert!(
                end <= extent.0 as f32 && pos.y + NATURAL_FONT_SIZE <= extent.1 as f32,
                "{text:?} runs off a {extent:?} surface: {pos:?}..{end}"
            );
        }

        let panel_right = PANEL_INSET + PANEL_WIDTH;
        for (pos, text) in drawn.iter().filter(|(_, text)| *text != HINT) {
            assert!(
                pos.x >= PANEL_INSET
                    && pos.x + atlas.text_width(text, NATURAL_SCALE) <= panel_right,
                "{text:?} is outside the panel's {PANEL_INSET}..{panel_right} columns: {pos:?}"
            );
        }
    }
}
