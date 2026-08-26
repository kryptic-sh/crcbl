//! The overlay: a crosshair, a readout panel and the control hint, over the 3D
//! frame.
//!
//! ```text
//!  ┌ breach ──────────┐
//!  │ SHOTS         6  │
//!  │ HITS          5  │
//!  │ ACCURACY    83%  │              ─┼─
//!  │ AIM        near  │
//!  │ NEAR   8 m  DOWN │
//!  │ MID   12 m    UP │
//!  │ FAR   18 m    UP │
//!  └──────────────────┘
//!
//!        W/A/S/D walk   mouse or the arrows look   SPACE fires
//! ```
//!
//! # The crosshair is the one thing on screen that is not optional
//!
//! A first-person demo with no crosshair is one where a visitor cannot tell
//! what they are about to shoot, and the whole of slice 1 is what a trigger
//! pull hits. It sits at the exact centre of the surface — which is where
//! [`crate::camera::forward`] points, and therefore where the ray goes — and it
//! **lights up when a standing plate is under it**, off
//! [`RenderState::crosshair`]. That is the same reading the shot is resolved
//! from, so the picture and the score cannot disagree; see [`crate::game`].
//!
//! # The panel is small on purpose
//!
//! The subject of this sample is what the pistol does to the range, and
//! anything drawn over that is in the way of it. The panel carries the score,
//! what the crosshair is on, and each lane's state; the debug panel (`F3`)
//! carries the position, the paths and the rest.
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

use crate::game::{RenderState, accuracy};
use crate::map::{LANE_LIST, LANES};

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
/// What a lane is drawn in while its plate is lying down — the one piece of
/// state on this panel worth a colour, and the same orange the plate itself is
/// drawn in ([`crate::map::PLATE_DOWN_MATERIAL`]).
const DOWN: [f32; 4] = [0.95, 0.55, 0.36, 1.0];

/// The crosshair over nothing worth shooting.
const CROSSHAIR: [f32; 4] = [0.86, 0.89, 0.95, 0.85];
/// …and over a standing plate.
const CROSSHAIR_ON_TARGET: [f32; 4] = [0.42, 0.92, 0.55, 1.0];

/// How far the crosshair's arms start from the centre, in pixels — the gap that
/// leaves the thing being aimed at visible.
const CROSSHAIR_GAP: f32 = 5.0;
/// How long each arm is, in pixels.
const CROSSHAIR_ARM: f32 = 9.0;
/// How thick each arm is, in pixels.
const CROSSHAIR_WIDTH: f32 = 2.0;

/// The panel's inset from the top-left corner, in pixels.
const PANEL_INSET: f32 = 16.0;
/// The height of one row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// How wide the panel is, in pixels. Wide enough for the longest label and a
/// right-aligned reading beside it.
const PANEL_WIDTH: f32 = 180.0;
/// How thick the panel's border is, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// The scale [`FontAtlas::text_width`] is measured at, which is a multiplier on
/// the baked glyph size rather than a size in pixels. The page draws at the
/// font's natural size, so it measures at the natural scale.
const NATURAL_SCALE: f32 = 1.0;

/// The control hint, which is the whole of what a first-time visitor needs.
const HINT: &str = "W/A/S/D walk   mouse or the arrows look   SPACE fires";

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

    let mut rows: Vec<(&str, String, [f32; 4])> = Vec::with_capacity(4 + LANES);
    rows.push(("SHOTS", format!("{}", state.shots), VALUE));
    rows.push(("HITS", format!("{}", state.hits), VALUE));
    rows.push((
        "ACCURACY",
        match accuracy(state.shots, state.hits) {
            Some(percent) => format!("{percent:.0}%"),
            None => "--".to_string(),
        },
        VALUE,
    ));
    rows.push((
        "AIM",
        state.crosshair.label().to_string(),
        if state.crosshair.scores() {
            CROSSHAIR_ON_TARGET
        } else {
            VALUE
        },
    ));
    for (lane, at) in LANE_LIST.iter().enumerate() {
        let down = state.plates_down[lane];
        rows.push((
            at.label,
            format!(
                "{:.0} m  {}",
                at.distance(),
                if down { "DOWN" } else { "UP" }
            ),
            if down { DOWN } else { VALUE },
        ));
    }

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

    crosshair(list, extent, state.crosshair.scores());

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

/// The four arms of the crosshair, about the exact centre of the surface.
///
/// The centre and nothing near it: the ray [`crate::game`] casts goes along
/// [`crate::camera::forward`], which is the view's own axis, so a crosshair
/// drawn anywhere else would be pointing at something the pistol does not hit.
fn crosshair(list: &mut DrawList, extent: (u32, u32), on_target: bool) {
    let centre = Vec2::new(extent.0 as f32 * 0.5, extent.1 as f32 * 0.5);
    let colour = if on_target {
        CROSSHAIR_ON_TARGET
    } else {
        CROSSHAIR
    };
    let half = CROSSHAIR_WIDTH * 0.5;
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        let near = centre + Vec2::new(dx, dy) * CROSSHAIR_GAP;
        let far = centre + Vec2::new(dx, dy) * (CROSSHAIR_GAP + CROSSHAIR_ARM);
        let across = Vec2::new(dy.abs() * half, dx.abs() * half);
        list.rect(
            Vec2::new(near.x.min(far.x), near.y.min(far.y)) - across,
            Vec2::new(near.x.max(far.x), near.y.max(far.y)) + across,
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Aim;
    use crcbl::ui::draw_list::DrawCommand;

    /// The readings a run part way through a string would show.
    fn shooting() -> RenderState {
        RenderState {
            shots: 6,
            hits: 5,
            crosshair: Aim::Plate(0),
            plates_down: [true, false, false],
            ..RenderState::default()
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

    /// **The page draws something, and what it draws is the score.** A frame
    /// with an empty draw list is the one failure a headless smoke test would
    /// otherwise report as a pass.
    #[test]
    fn the_panel_carries_the_score_the_frame_was_drawn_at() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(&mut list, &atlas, (960, 720), &shooting());
        assert!(stats.commands > 0, "the page drew nothing at all");

        let drawn = text(&list);
        for reading in ["6", "5", "83%", "near"] {
            assert!(
                drawn.contains(&reading),
                "the {reading} reading is missing: {drawn:?}",
            );
        }
        assert!(
            drawn.iter().any(|t| t.contains("DOWN")),
            "no lane is shown down: {drawn:?}",
        );
        assert!(drawn.contains(&HINT), "the control hint is missing");
    }

    /// **A run that has not fired shows a dash rather than a zero**, which is
    /// [`accuracy`]'s whole distinction reaching the screen.
    #[test]
    fn accuracy_reads_as_nothing_yet_before_the_first_shot() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        draw(&mut list, &atlas, (960, 720), &RenderState::default());
        assert!(
            text(&list).contains(&"--"),
            "an unfired run showed a number"
        );

        let mut list = DrawList::new();
        draw(
            &mut list,
            &atlas,
            (960, 720),
            &RenderState {
                shots: 3,
                hits: 0,
                ..RenderState::default()
            },
        );
        assert!(
            text(&list).contains(&"0%"),
            "a run that has missed everything is 0%, not nothing yet",
        );
    }

    /// **The crosshair is at the centre of the surface and changes on a
    /// target.** Two claims one test can make because the second is a
    /// difference: a page that drew the same four rectangles whatever the
    /// simulation said would pass a check that only looked at one frame.
    #[test]
    fn the_crosshair_sits_where_the_ray_goes_and_lights_up_on_a_plate() {
        let atlas = FontAtlas::built_in();
        let extent = (960u32, 720u32);
        let centre = Vec2::new(480.0, 360.0);

        let arms = |state: &RenderState| {
            let mut list = DrawList::new();
            draw(&mut list, &atlas, extent, state);
            list.commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Rect { min, max, color } => Some((*min, *max, *color)),
                    _ => None,
                })
                // The panel's own background is the only other rectangle, and
                // it is nowhere near the middle of the surface.
                .filter(|(min, max, _)| {
                    min.x < centre.x + 32.0
                        && max.x > centre.x - 32.0
                        && min.y < centre.y + 32.0
                        && max.y > centre.y - 32.0
                })
                .collect::<Vec<_>>()
        };

        let idle = arms(&RenderState::default());
        assert_eq!(idle.len(), 4, "the crosshair is not four arms: {idle:?}");
        for (min, max, colour) in &idle {
            assert_eq!(*colour, CROSSHAIR);
            // Every arm straddles one of the centre's two axes and clears the
            // other by the gap, which is what leaves the target visible.
            let straddles_x = min.x <= centre.x && max.x >= centre.x;
            let straddles_y = min.y <= centre.y && max.y >= centre.y;
            assert!(
                straddles_x ^ straddles_y,
                "an arm at {min:?}..{max:?} is not one of the four",
            );
        }

        let on_target = arms(&shooting());
        assert_eq!(on_target.len(), 4);
        for (_, _, colour) in &on_target {
            assert_eq!(
                *colour, CROSSHAIR_ON_TARGET,
                "the crosshair did not change over a standing plate",
            );
        }
    }

    /// **A reading that is measured wrong is drawn off the surface, and the
    /// draw list still contains it.** Asserting the strings reach the list says
    /// nothing about where they land, so this asserts the geometry: every
    /// string the page emits starts inside the surface it was laid out against,
    /// and the right-aligned readings start inside their own panel.
    #[test]
    fn every_reading_is_laid_out_where_it_can_actually_be_seen() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let extent = (960u32, 720u32);
        draw(&mut list, &atlas, extent, &shooting());

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
