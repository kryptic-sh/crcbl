//! The overlay: the budget readout, over the 3D frame.
//!
//! ```text
//!  ┌ sparks ──────────────┐
//!  │ LIVE            412  │
//!  │ SPARKS           96  │
//!  │ PUFF            252  │
//!  │ SPAM          64/64  │
//!  │ CLAMPED     251 003  │
//!  │ POOL       704/2048  │
//!  └──────────────────────┘
//!
//!         sparks off the anvil · smoke at the vent · the spam is clamped
//! ```
//!
//! # The panel is the claim
//!
//! `docs/plan/sample/10-sparks.md` asks that a hostile effect "clamps to its
//! pool share **and the panel shows it**". The `SPAM` row is that: the count
//! and the share side by side, so a reader sees the cap being held rather than
//! inferring it from a frame rate. `CLAMPED` is the other half — how many
//! spawns the budget has refused — because a count sitting at its share could
//! equally be an emitter that happens to ask for exactly that many, and a
//! refusal counter cannot be mistaken for one.
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

use crate::show::Reading;

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
/// What a reading is drawn in when it is up against a limit — the hostile
/// effect at its share, which is the one piece of state on this panel worth a
/// colour.
const CLAMPED: [f32; 4] = [0.95, 0.72, 0.30, 1.0];

/// The panel's inset from the top-left corner, in pixels.
const PANEL_INSET: f32 = 16.0;
/// The height of one row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// How wide the panel is, in pixels. Wide enough for the longest label and a
/// right-aligned reading beside it.
const PANEL_WIDTH: f32 = 196.0;
/// How thick the panel's border is, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// The scale [`FontAtlas::text_width`] is measured at, which is a multiplier on
/// the baked glyph size rather than a size in pixels. The page draws at the
/// font's natural size, so it measures at the natural scale.
const NATURAL_SCALE: f32 = 1.0;

/// What a first-time visitor needs, given there is nothing to press.
const HINT: &str = "sparks off the anvil  ·  smoke at the vent  ·  the spam is clamped";

/// What the page drew, for the loop's own tests and its summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// A count with a thin space every three digits, because the refusal counter
/// reaches seven figures inside a minute and an unbroken run of them is a
/// number nobody reads.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(digit);
    }
    out
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
    reading: &Reading,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    let at_share = reading.spam >= reading.spam_share;
    let rows: [(&str, String, [f32; 4]); 6] = [
        ("LIVE", grouped(u64::from(reading.live)), VALUE),
        ("SPARKS", grouped(u64::from(reading.sparks)), VALUE),
        (
            "PUFF",
            format!(
                "{} {}",
                grouped(u64::from(reading.puff)),
                if reading.puff_emitting { "ON" } else { "OFF" }
            ),
            VALUE,
        ),
        // The budget claim, in one row: what the hostile effect holds against
        // what it is allowed.
        (
            "SPAM",
            format!("{}/{}", reading.spam, reading.spam_share),
            if at_share { CLAMPED } else { VALUE },
        ),
        (
            "CLAMPED",
            grouped(reading.spam_clamped),
            if reading.spam_clamped > 0 {
                CLAMPED
            } else {
                VALUE
            },
        ),
        (
            "POOL",
            format!("{}/{}", reading.reserved, reading.capacity),
            VALUE,
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

    fn reading() -> Reading {
        Reading {
            live: 412,
            sparks: 96,
            puff: 252,
            spam: 64,
            spam_share: 64,
            spam_clamped: 251_003,
            puff_emitting: true,
            reserved: 704,
            capacity: 2048,
            effects: 3,
        }
    }

    fn text(list: &DrawList) -> Vec<String> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The page draws something, and what it draws is the budget.** A frame
    /// with an empty draw list is the one failure a headless smoke test would
    /// otherwise report as a pass.
    #[test]
    fn the_panel_carries_the_budget_it_was_handed() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(&mut list, &atlas, (960, 720), &reading());
        assert!(stats.commands > 0, "the page drew nothing at all");

        let text = text(&list);
        assert!(text.iter().any(|t| t == "412"), "the live count: {text:?}");
        assert!(
            text.iter().any(|t| t == "64/64"),
            "the hostile effect's count against its share: {text:?}"
        );
        assert!(
            text.iter().any(|t| t == "251 003"),
            "the refusal counter: {text:?}"
        );
        assert!(
            text.iter().any(|t| t == "704/2048"),
            "the pool occupancy: {text:?}"
        );
        assert!(text.iter().any(|t| t == HINT), "the hint: {text:?}");
    }

    /// The puff row says which way round it is, because the whole browser gate
    /// is a count read on both sides of a stop.
    #[test]
    fn the_puff_row_says_whether_its_emitter_is_running() {
        let atlas = FontAtlas::built_in();
        for (emitting, want) in [(true, "252 ON"), (false, "252 OFF")] {
            let mut list = DrawList::new();
            draw(
                &mut list,
                &atlas,
                (960, 720),
                &Reading {
                    puff_emitting: emitting,
                    ..reading()
                },
            );
            let text = text(&list);
            assert!(
                text.iter().any(|t| t == want),
                "the puff row does not say {want:?}: {text:?}"
            );
        }
    }

    /// **A reading that is measured wrong is drawn off the surface, and the
    /// draw list still contains it.** Asserting the strings reach the list says
    /// nothing about where they land, so this asserts the geometry.
    #[test]
    fn every_reading_is_laid_out_where_it_can_actually_be_seen() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let extent = (960u32, 720u32);
        draw(
            &mut list,
            &atlas,
            extent,
            &Reading {
                live: 2048,
                spam_clamped: u64::from(u32::MAX),
                ..reading()
            },
        );

        let drawn: Vec<(Vec2, String)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { pos, text, .. } => {
                    Some((*pos, text.clone()))
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
        for (pos, text) in drawn.iter().filter(|(_, text)| text != HINT) {
            assert!(
                pos.x >= PANEL_INSET
                    && pos.x + atlas.text_width(text, NATURAL_SCALE) <= panel_right,
                "{text:?} is outside the panel's {PANEL_INSET}..{panel_right} columns: {pos:?}"
            );
        }
    }

    #[test]
    fn a_long_count_is_grouped_into_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(7), "7");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1 000");
        assert_eq!(grouped(251_003), "251 003");
        assert_eq!(grouped(12_345_678), "12 345 678");
    }
}
