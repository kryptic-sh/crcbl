//! The overlay: rung 0's counters, and the gap stated plainly.
//!
//! ```text
//!  ┌ tumble ───────────────────────┐
//!  │ TICK                      612 │
//!  │ FLIPS                       4 │
//!  │ L DRIFT              3.1e-12  │
//!  │ E DRIFT              6.2e-12  │
//!  │ SPIN               6.00 rad/s │
//!  │ BOX Y                 -2.41 m │
//!  │ BOX SPIN                    0 │
//!  │ BOX LEVEL               exact │
//!  │ DROPS                       3 │
//!  │ STEP                    41 us │
//!  │ HASH         20be321e0066f5de │
//!  └───────────────────────────────┘
//!
//!   handle flips on its middle axis - box falls flat, through the floor: no
//!   contact solver yet
//! ```
//!
//! `docs/plan/sample/24-tumble.md` asks that a scene the engine cannot produce
//! yet ship labelled as the gap it is, and the hint line is that label for the
//! box: it drops without turning, which rung 0 can show, and then does not
//! land, which rung 1 closes.

use crcbl::ui::draw_list::DrawList;
use crcbl::ui::readout::{ReadoutPanel, ReadoutRow};
use crcbl::ui::text::FontAtlas;

use crate::scene::Reading;

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
/// A reading that says something went wrong: spin on a box nothing turned.
const WRONG: [f32; 4] = [0.95, 0.40, 0.30, 1.0];

/// The panel the readings are drawn in.
const PANEL: ReadoutPanel = ReadoutPanel {
    inset: 16.0,
    // Wide enough for the label and a sixteen-digit hash beside it.
    width: 280.0,
    row_height: 18.0,
    pad: 8.0,
    border_width: 1.0,
    background: PANEL_BG,
    border: BORDER,
    label: LABEL,
};

/// The two scenes, and the gap, in one line.
pub const HINT: &str =
    "handle flips on its middle axis - box falls flat, through the floor: no contact solver yet";

/// What the page drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// Draws the overlay into `list`, laid out against a surface of `extent`.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    reading: &Reading,
) -> PageStats {
    let rows = [
        ReadoutRow::new("TICK", reading.tick.to_string(), VALUE),
        ReadoutRow::new("FLIPS", reading.flips.to_string(), VALUE),
        ReadoutRow::new("L DRIFT", format!("{:.1e}", reading.momentum_drift), VALUE),
        ReadoutRow::new("E DRIFT", format!("{:.1e}", reading.energy_drift), VALUE),
        ReadoutRow::new("SPIN", format!("{:.2} rad/s", reading.handle_spin), VALUE),
        ReadoutRow::new("BOX Y", format!("{:.2} m", reading.box_height), VALUE),
        ReadoutRow::new(
            "BOX SPIN",
            format!("{}", reading.box_spin),
            if reading.box_spin == 0.0 {
                VALUE
            } else {
                WRONG
            },
        ),
        ReadoutRow::new(
            "BOX LEVEL",
            if reading.box_level { "exact" } else { "tilted" },
            if reading.box_level { VALUE } else { WRONG },
        ),
        ReadoutRow::new("DROPS", reading.drops.to_string(), VALUE),
        ReadoutRow::new(
            "STEP",
            reading
                .step_micros
                .map_or_else(|| "no clock".to_owned(), |micros| format!("{micros:.0} us")),
            VALUE,
        ),
        ReadoutRow::new("HASH", format!("{:016x}", reading.hash), VALUE),
    ];

    PANEL.draw(list, atlas, &rows);
    PANEL.hint(list, atlas, extent, HINT);

    PageStats {
        commands: list.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> Reading {
        Reading {
            tick: 612,
            flips: 4,
            momentum_drift: 3.1e-12,
            energy_drift: 6.2e-12,
            handle_spin: 6.0,
            box_height: -2.41,
            box_spin: 0.0,
            box_level: true,
            drops: 3,
            step_micros: Some(41.0),
            hash: 0x20be_321e_0066_f5de,
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

    /// **The page carries every counter it was handed, and the gap.**
    #[test]
    fn the_panel_carries_the_counters_and_the_gap() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(&mut list, &atlas, (960, 720), &reading());
        assert!(stats.commands > 0, "the page drew nothing");
        let text = text(&list);
        for want in [
            "612",
            "4",
            "3.1e-12",
            "6.2e-12",
            "exact",
            "41 us",
            "20be321e0066f5de",
        ] {
            assert!(
                text.iter().any(|t| t == want),
                "{want:?} is missing: {text:?}"
            );
        }
        assert!(
            text.iter().any(|t| t == HINT) && HINT.contains("no contact solver yet"),
            "the gap is not labelled: {text:?}"
        );
    }

    /// The hint fits across the default window, so the gap is not cut off.
    #[test]
    fn the_hint_fits_the_default_window() {
        let atlas = FontAtlas::built_in();
        let width = atlas.text_width(HINT, crcbl::ui::readout::NATURAL_SCALE);
        assert!(
            width < 960.0 - 2.0 * PANEL.inset,
            "the hint is {width} px wide"
        );
    }

    /// A build with no clock says so rather than reading zero.
    #[test]
    fn an_untimed_step_says_there_is_no_clock() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        draw(
            &mut list,
            &atlas,
            (960, 720),
            &Reading {
                step_micros: None,
                ..reading()
            },
        );
        assert!(text(&list).iter().any(|t| t == "no clock"));
    }
}
