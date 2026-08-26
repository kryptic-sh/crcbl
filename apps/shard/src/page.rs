//! The overlay: a small readout panel and the control hint, over the 3D frame.
//!
//! ```text
//!  ┌ shard ─────────────┐
//!  │ POSITION  0.0 18.0 │
//!  │ FOOTING   floor    │
//!  │ HEALTH    100/100  │
//!  │ FOES      3        │
//!  │ TORCHES   LIT      │
//!  └────────────────────┘
//!
//!    W/A/S/D walk   Q/E turn   SPACE strike   L douses the torches
//! ```
//!
//! # Nothing on this panel ticks, and that is load-bearing
//!
//! **Every reading here is a fact about where the character is standing, what
//! they have left and what is switched on** — never a clock, a frame counter, a
//! flame's brightness or an elapsed time. That is not restraint about clutter:
//! `web/tools/browser-e2e.mjs` douses the torches and then asserts the **canvas
//! stops changing**, which is the control that says the change it saw while they
//! were lit came from the light. A tick counter drawn here would change every
//! frame and make that control impossible to pass on a working build.
//!
//! The two fight readings are safe on that test for the same reason the position
//! is: they move when the *world* does and hold still when it does not.
//! [`crate::foe::POSTS`] puts every foe out of notice range of where the gate
//! stands the character to take those samples, and
//! `no_foe_can_reach_the_character_where_the_zone_opens` is what holds it there.
//! They earn their place because a player who cannot see their own health cannot
//! tell a fight they are winning from one they are losing, which is the whole of
//! what this slice added.
//!
//! The moving numbers are all on the debug panel (`F3`) and on the `[HUD]`
//! heartbeat, where a gate reads them as text rather than as pixels.
//!
//! # The panel is small on purpose
//!
//! The subject of this sample is what the torches do to the stone, and anything
//! drawn over that is in the way of it. `docs/plan/sample/15-shard.md`'s
//! milestone 1 eventually wants a grid inventory in front of this frame; there
//! is no item to put in one yet, and topic 34's kit is a later slice's job.
//!
//! **Nothing about the save is drawn here either, and that is the same rule.**
//! An autosave counter would change every `crate::save::SAVE_PERIOD_S` of
//! simulated time and make the still-frame control above impossible to pass on a
//! working build. Whether the session resumed, how many writes it has made and
//! where they go are on the debug panel (`F3`) and the `[HUD]` heartbeat.
//!
//! # Laid out against the surface
//!
//! Every position is derived from the extent the swapchain was actually acquired
//! at, so the page is correct in a resized window and in the headless offscreen
//! ring at whatever `--size` asked for.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::game::RenderState;

const PANEL_BG: [f32; 4] = [0.06, 0.05, 0.04, 0.80];
const BORDER: [f32; 4] = [0.38, 0.32, 0.25, 1.0];
const LABEL: [f32; 4] = [0.72, 0.66, 0.58, 1.0];
const VALUE: [f32; 4] = [0.98, 0.95, 0.90, 1.0];
/// What the torch reading is drawn in while they are burning — the firelight the
/// zone is lit by, so the panel and the room agree about what is on.
const LIT: [f32; 4] = [0.98, 0.66, 0.30, 1.0];
/// …and once they have been put out.
const OUT: [f32; 4] = [0.52, 0.56, 0.66, 1.0];
/// What a reading that is bad news is drawn in: half the character's health
/// gone, or something in the zone still standing.
const HURT: [f32; 4] = [0.92, 0.36, 0.30, 1.0];

/// The panel's inset from the top-left corner, in pixels.
const PANEL_INSET: f32 = 16.0;
/// The height of one row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// How wide the panel is, in pixels. Wide enough for the longest label and a
/// right-aligned reading beside it —
/// `every_reading_is_laid_out_where_it_can_actually_be_seen` is what holds this
/// number to the widest reading rather than to this sentence.
const PANEL_WIDTH: f32 = 190.0;
/// How thick the panel's border is, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// The scale [`FontAtlas::text_width`] is measured at, which is a multiplier on
/// the baked glyph size rather than a size in pixels. The page draws at the
/// font's natural size, so it measures at the natural scale.
const NATURAL_SCALE: f32 = 1.0;

/// The control hint, which is the whole of what a first-time visitor needs.
const HINT: &str = "W/A/S/D walk   Q/E turn the camera   SPACE strikes   L douses the torches";

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
/// font rather than against a guess.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    state: &RenderState,
    torches_lit: bool,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    let rows: Vec<(&str, String, [f32; 4])> = vec![
        (
            "POSITION",
            format!("{:.0} {:.0}", state.feet.x, state.feet.z),
            VALUE,
        ),
        (
            "FOOTING",
            if state.grounded {
                if state.feet.y > 0.05 { "dais" } else { "floor" }
            } else {
                "falling"
            }
            .to_string(),
            VALUE,
        ),
        (
            "HEALTH",
            format!("{}/{}", state.health, crate::foe::HEALTH_MAX),
            if state.health * 2 <= crate::foe::HEALTH_MAX {
                HURT
            } else {
                VALUE
            },
        ),
        (
            "FOES",
            format!("{}", state.alive),
            if state.alive > 0 { HURT } else { VALUE },
        ),
        (
            "TORCHES",
            if torches_lit { "LIT" } else { "OUT" }.to_string(),
            if torches_lit { LIT } else { OUT },
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
    use crcbl::ui::draw_list::DrawCommand;

    /// A character standing on the dais with the torches lit.
    fn on_the_dais() -> RenderState {
        RenderState {
            position: DVec3::new(1.0, 1.35, -3.0),
            feet: DVec3::new(1.0, crate::zone::DAIS_HEIGHT, -3.0),
            grounded: true,
            blocked: false,
            elapsed: 12.5,
            foes: [crate::foe::FoeView::default(); crate::foe::FOES],
            health: crate::foe::HEALTH_MAX,
            alive: crate::foe::FOES,
        }
    }

    /// Every `Text` command the page produced.
    fn text(list: &DrawList) -> Vec<&str> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// **The page draws something, and what it draws is where the character is
    /// standing.** A frame with an empty draw list is the one failure a headless
    /// smoke test would otherwise report as a pass.
    #[test]
    fn the_panel_carries_the_footing_the_frame_was_drawn_at() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(&mut list, &atlas, (960, 720), &on_the_dais(), true);
        assert!(stats.commands > 0, "the page drew nothing at all");

        let drawn = text(&list);
        for reading in ["POSITION", "FOOTING", "dais", "TORCHES", "LIT"] {
            assert!(
                drawn.contains(&reading),
                "the {reading} reading is missing: {drawn:?}",
            );
        }
        assert!(drawn.contains(&HINT), "the control hint is missing");
    }

    /// **Dousing the torches changes exactly one reading**, which is what says
    /// the panel is reporting the switch rather than restating the frame.
    #[test]
    fn the_torch_reading_follows_the_switch() {
        let atlas = FontAtlas::built_in();
        let readings = |lit: bool| {
            let mut list = DrawList::new();
            draw(&mut list, &atlas, (960, 720), &on_the_dais(), lit);
            list.commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let lit = readings(true);
        let out = readings(false);
        assert!(lit.contains(&"LIT".to_string()));
        assert!(out.contains(&"OUT".to_string()));
        assert_eq!(
            lit.len(),
            out.len(),
            "the two states draw different numbers of strings",
        );
        assert_eq!(
            lit.iter().filter(|word| !out.contains(word)).count(),
            1,
            "dousing the torches changed more than the torch reading: {lit:?} vs {out:?}",
        );
    }

    /// **Nothing on this panel moves when only the clock does.** The check the
    /// browser gate's still-frame control depends on: with the character standing
    /// still and the torches out, two draws seconds apart have to be identical
    /// commands, or the canvas changes for a reason that is not the lighting.
    #[test]
    fn the_panel_is_identical_between_two_frames_the_character_stood_still_for() {
        let atlas = FontAtlas::built_in();
        let commands = |elapsed: f64| {
            let mut list = DrawList::new();
            draw(
                &mut list,
                &atlas,
                (960, 720),
                &RenderState {
                    elapsed,
                    ..on_the_dais()
                },
                false,
            );
            format!("{:?}", list.commands())
        };
        assert_eq!(
            commands(0.0),
            commands(97.5),
            "the overlay draws something that ticks",
        );
    }

    /// **A reading that is measured wrong is drawn off the surface, and the draw
    /// list still contains it.** Asserting the strings reach the list says
    /// nothing about where they land, so this asserts the geometry: every string
    /// the page emits starts inside the surface it was laid out against, and the
    /// right-aligned readings start inside their own panel.
    #[test]
    fn every_reading_is_laid_out_where_it_can_actually_be_seen() {
        let atlas = FontAtlas::built_in();
        let extent = (960u32, 720u32);
        // The longest strings this panel ever right-aligns, which is a negative
        // pair of coordinates and the word `falling`.
        let far = RenderState {
            feet: DVec3::new(-19.5, 0.0, -22.5),
            grounded: false,
            ..on_the_dais()
        };
        let mut list = DrawList::new();
        draw(&mut list, &atlas, extent, &far, true);

        let drawn: Vec<(Vec2, &str)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { pos, text, .. } => Some((*pos, text.as_str())),
                _ => None,
            })
            .collect();
        assert!(!drawn.is_empty(), "the page drew no text at all");

        for (pos, text) in &drawn {
            assert!(
                pos.x >= 0.0 && pos.y >= 0.0,
                "{text:?} starts off the top-left corner at {pos:?}",
            );
            let end = pos.x + atlas.text_width(text, NATURAL_SCALE);
            assert!(
                end <= extent.0 as f32 && pos.y + NATURAL_FONT_SIZE <= extent.1 as f32,
                "{text:?} runs off a {extent:?} surface: {pos:?}..{end}",
            );
        }

        let panel_right = PANEL_INSET + PANEL_WIDTH;
        for (pos, text) in drawn.iter().filter(|(_, text)| *text != HINT) {
            assert!(
                pos.x >= PANEL_INSET
                    && pos.x + atlas.text_width(text, NATURAL_SCALE) <= panel_right,
                "{text:?} is outside the panel's {PANEL_INSET}..{panel_right} columns: {pos:?}",
            );
        }
    }
}
