//! The HUD page: the whole of what this sample draws.
//!
//! Everything here is built from the four primitives
//! [`docs/plan/sample/04-hud.md`](https://github.com/kryptic-sh/crcbl/blob/main/docs/plan/sample/04-hud.md)
//! names for milestone 1 — blocks, spans, text and bars — which in
//! [`DrawList`]'s vocabulary are [`DrawList::rect`], [`DrawList::rect_outline`],
//! [`DrawList::text`], and a pair of rects where the inner one is a fraction of
//! the outer's width. There is no widget type in this file and no styling
//! system behind it: the CSS subset, the stylesheets and the two themes are P10
//! work that depends on a layout engine that does not exist yet, and building
//! toward them here would be machinery with no consumer.
//!
//! ```text
//!  ┌ vitals ─────────┐        ┌ WAVE 3 ┐
//!  │ HEALTH ▰▰▰▰▱▱▱▱ │                          145
//!  │ MANA   ▰▰▱▱▱▱▱▱ │                                72
//!  └─────────────────┘                    38
//!
//!                   [STRIKE][CLEAVE][BOLT][NOVA]
//! ```
//!
//! # Laid out against the surface, not against a fixed 960×720
//!
//! Every position below is derived from the extent the swapchain was actually
//! acquired at. The vitals panel is inset from the top-left, the banner and the
//! ability row are centred, and the damage ticker hangs off the right edge — so
//! the page is correct in a resized window and in the headless offscreen ring at
//! whatever `--size` asked for, rather than only at the size it was written at.
//!
//! # No string cache, deliberately
//!
//! `apps/flappy` caches its two HUD lines behind a key, because they change
//! only when the score does. Nothing on this page holds still: the cooldown
//! countdowns and the damage numbers change every tick, so a cache keyed on the
//! state would miss on every frame and cost a comparison for it.
//! [`DrawList::text`] takes an owned `String` either way.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::{DrawCommand, DrawList};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::game::{ABILITIES, ABILITY_COUNT, DAMAGE_LANES, RenderState};

// ---- palette -----------------------------------------------------------------

/// What the frame is cleared to behind the page.
///
/// This sample's whole backdrop, and the plan doc's hard cap says so: "no scene
/// rendering behind the HUD beyond a static backdrop". It is the clear pass's
/// colour rather than a rect, so the page owes no full-screen quad.
pub const BACKDROP: [f32; 4] = [0.05, 0.06, 0.09, 1.0];

const PANEL_BG: [f32; 4] = [0.08, 0.09, 0.13, 0.86];
const TRACK: [f32; 4] = [0.16, 0.17, 0.22, 1.0];
const HEALTH_FILL: [f32; 4] = [0.82, 0.24, 0.28, 1.0];
const MANA_FILL: [f32; 4] = [0.26, 0.48, 0.92, 1.0];
const BORDER: [f32; 4] = [0.42, 0.45, 0.55, 1.0];
const LABEL: [f32; 4] = [0.78, 0.80, 0.86, 1.0];
const VALUE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const BANNER_BG: [f32; 4] = [0.12, 0.09, 0.04, 0.90];
const BANNER_TEXT: [f32; 4] = [1.0, 0.84, 0.36, 1.0];
const SLOT_READY: [f32; 4] = [0.13, 0.20, 0.18, 0.95];
const SLOT_COOLING: [f32; 4] = [0.10, 0.11, 0.14, 0.95];
const SWEEP: [f32; 4] = [0.03, 0.04, 0.06, 0.80];
const READY_TEXT: [f32; 4] = [0.55, 0.95, 0.60, 1.0];
const COOLING_TEXT: [f32; 4] = [0.72, 0.74, 0.82, 1.0];
const DAMAGE_TEXT: [f32; 3] = [1.0, 0.86, 0.36];

// ---- metrics -----------------------------------------------------------------

/// How far the page keeps off every edge of the surface, in pixels.
const MARGIN: f32 = 24.0;

/// Padding between a panel's edge and its content, in pixels.
const PAD: f32 = 10.0;

/// The width of a vitals bar's track, in pixels.
const BAR_WIDTH: f32 = 280.0;
/// The height of a vitals bar's track, in pixels.
const BAR_HEIGHT: f32 = 18.0;
/// The gap between a bar's caption and its track, in pixels.
const CAPTION_GAP: f32 = 18.0;
/// The gap between one bar row and the next, in pixels.
const ROW_GAP: f32 = 10.0;

/// The side of one ability slot, in pixels.
const SLOT: f32 = 84.0;
/// The gap between one ability slot and the next, in pixels.
const SLOT_GAP: f32 = 12.0;

/// The banner's size, in pixels.
const BANNER: Vec2 = Vec2::new(240.0, 44.0);

/// How wide one damage lane is, in pixels.
const LANE_WIDTH: f32 = 88.0;
/// How far a damage number climbs over its whole life, in pixels.
const DAMAGE_RISE: f32 = 150.0;

const CAPTION_SIZE: f32 = 14.0;
const VALUE_SIZE: f32 = 13.0;
const SLOT_NAME_SIZE: f32 = 13.0;
const BANNER_SIZE: f32 = 22.0;
const DAMAGE_SIZE: f32 = 18.0;

/// The border every framed element on this page is drawn with, in pixels.
const BORDER_THICKNESS: f32 = 1.0;

// ---- what a frame drew -------------------------------------------------------

/// How many commands of each kind one call to [`draw`] emitted.
///
/// Counted off the draw list itself rather than tallied as the page goes, so the
/// numbers the debug panel reports are the commands the UI pass will actually
/// upload — a page that returned its own idea of what it drew could disagree
/// with the list and nothing would notice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// Filled rectangles — the plan doc's "blocks", and the two halves of every
    /// bar.
    pub rects: usize,
    /// Rectangle outlines — the frames around the panel's bars, the banner and
    /// each ability slot.
    pub outlines: usize,
    /// Text spans.
    pub text: usize,
    /// Stroked lines and polylines.
    pub strokes: usize,
}

impl PageStats {
    /// Tallies a run of commands by kind.
    #[must_use]
    pub fn of(commands: &[DrawCommand]) -> Self {
        let mut stats = Self::default();
        for command in commands {
            match command {
                DrawCommand::Rect { .. } => stats.rects += 1,
                DrawCommand::RectOutline { .. } => stats.outlines += 1,
                DrawCommand::Text { .. } => stats.text += 1,
                DrawCommand::Line { .. } | DrawCommand::Polyline { .. } => stats.strokes += 1,
            }
        }
        stats
    }

    /// Every command the page emitted.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.rects + self.outlines + self.text + self.strokes
    }
}

impl crcbl::ui::DebugModule for PageStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("page");
        section.row("rects", format_args!("{}", self.rects));
        section.row("outlines", format_args!("{}", self.outlines));
        section.row("text", format_args!("{}", self.text));
    }
}

// ---- the page ----------------------------------------------------------------

/// Draws the whole page into `dl`, and reports what it drew.
///
/// `screen` is the extent the swapchain was acquired at, and `atlas` must be the
/// one the UI pass will render with — every centred string on this page is
/// centred by measuring through it, so a page measured with a different atlas
/// would draw its banner off to one side.
pub fn draw(dl: &mut DrawList, screen: Vec2, atlas: &FontAtlas, state: &RenderState) -> PageStats {
    let first = dl.len();
    vitals(dl, atlas, state);
    wave_banner(dl, screen, atlas, state);
    ability_row(dl, screen, atlas, state);
    damage_ticker(dl, screen, atlas, state);
    PageStats::of(&dl.commands()[first..])
}

/// The health and mana bars, in a panel inset from the top-left.
fn vitals(dl: &mut DrawList, atlas: &FontAtlas, state: &RenderState) {
    let row_height = CAPTION_GAP + BAR_HEIGHT;
    let panel = Vec2::new(
        BAR_WIDTH + PAD * 2.0,
        row_height * 2.0 + ROW_GAP + PAD * 2.0,
    );
    let origin = Vec2::splat(MARGIN);
    dl.rect(origin, origin + panel, PANEL_BG);

    let content = origin + Vec2::splat(PAD);
    bar(
        dl,
        atlas,
        content,
        "HEALTH",
        state.health_fraction(),
        HEALTH_FILL,
        &format!("{} / {}", state.health, crate::game::HEALTH_MAX),
    );
    bar(
        dl,
        atlas,
        content + Vec2::new(0.0, row_height + ROW_GAP),
        "MANA",
        state.mana_fraction(),
        MANA_FILL,
        &format!("{} / {}", state.mana, crate::game::MANA_MAX),
    );
}

/// One bar: a caption, a track, the part of it that is full, and a frame.
///
/// The fill is emitted only when there is some, so an empty pool draws a track
/// and no zero-width quad inside it.
fn bar(
    dl: &mut DrawList,
    atlas: &FontAtlas,
    origin: Vec2,
    caption: &str,
    fraction: f32,
    fill: [f32; 4],
    value: &str,
) {
    dl.text(origin, caption, LABEL, CAPTION_SIZE);

    let track_min = origin + Vec2::new(0.0, CAPTION_GAP);
    let track_max = track_min + Vec2::new(BAR_WIDTH, BAR_HEIGHT);
    dl.rect(track_min, track_max, TRACK);

    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        dl.rect(
            track_min,
            Vec2::new(track_min.x + BAR_WIDTH * fraction, track_max.y),
            fill,
        );
    }
    dl.rect_outline(track_min, track_max, BORDER_THICKNESS, BORDER);

    // Right-aligned inside the track, where a health readout goes.
    let width = atlas.text_width(value, VALUE_SIZE / NATURAL_FONT_SIZE);
    dl.text(
        Vec2::new(track_max.x - width - PAD * 0.5, track_min.y + 3.0),
        value,
        VALUE,
        VALUE_SIZE,
    );
}

/// The wave banner, centred at the top — on screen only while the ticker says
/// so, which is what makes it a banner rather than a heading.
fn wave_banner(dl: &mut DrawList, screen: Vec2, atlas: &FontAtlas, state: &RenderState) {
    if !state.banner {
        return;
    }
    let min = Vec2::new((screen.x - BANNER.x) * 0.5, MARGIN);
    let max = min + BANNER;
    dl.rect(min, max, BANNER_BG);
    dl.rect_outline(min, max, BORDER_THICKNESS, BANNER_TEXT);
    centred(
        dl,
        atlas,
        min,
        max,
        &format!("WAVE {}", state.wave),
        BANNER_TEXT,
        BANNER_SIZE,
    );
}

/// The ability row, centred along the bottom.
///
/// A cooling slot is dimmed, covered from the bottom by a sweep that retreats as
/// the cooldown runs down, and labelled with the seconds it has left; a ready
/// slot is lit and says so. Those are the states the plan doc asks this row to
/// show, and they are the whole of what a cooldown indicator is.
fn ability_row(dl: &mut DrawList, screen: Vec2, atlas: &FontAtlas, state: &RenderState) {
    let row_width = SLOT * ABILITY_COUNT as f32 + SLOT_GAP * (ABILITY_COUNT - 1) as f32;
    let origin = Vec2::new((screen.x - row_width) * 0.5, screen.y - MARGIN - SLOT);

    for (slot, ability) in state.abilities.iter().enumerate() {
        let min = origin + Vec2::new((SLOT + SLOT_GAP) * slot as f32, 0.0);
        let max = min + Vec2::splat(SLOT);
        let ready = ability.is_ready();
        dl.rect(min, max, if ready { SLOT_READY } else { SLOT_COOLING });

        let sweep = ability.sweep(slot);
        if sweep > 0.0 {
            // Up from the bottom edge: the covered part is what is still to
            // run, so a slot that just fired is covered and one about to come
            // back is nearly clear.
            dl.rect(Vec2::new(min.x, max.y - SLOT * sweep), max, SWEEP);
        }
        dl.rect_outline(min, max, BORDER_THICKNESS, BORDER);

        centred(
            dl,
            atlas,
            min,
            Vec2::new(max.x, min.y + SLOT * 0.6),
            ABILITIES[slot].name,
            if ready { VALUE } else { COOLING_TEXT },
            SLOT_NAME_SIZE,
        );
        let (status, colour) = if ready {
            ("READY".to_string(), READY_TEXT)
        } else {
            (format!("{:.1}s", ability.remaining), COOLING_TEXT)
        };
        centred(
            dl,
            atlas,
            Vec2::new(min.x, min.y + SLOT * 0.55),
            max,
            &status,
            colour,
            SLOT_NAME_SIZE,
        );
    }
}

/// The damage ticker: one number per hit, climbing its lane and fading out.
fn damage_ticker(dl: &mut DrawList, screen: Vec2, atlas: &FontAtlas, state: &RenderState) {
    let lanes_left = screen.x - MARGIN - LANE_WIDTH * DAMAGE_LANES as f32;
    let base = screen.y * 0.55;
    for number in &state.damage {
        let age = number.age.clamp(0.0, 1.0);
        let text = format!("{}", number.amount);
        let width = atlas.text_width(&text, DAMAGE_SIZE / NATURAL_FONT_SIZE);
        // Centred in its lane, so two lanes' numbers cannot read as one.
        let x = lanes_left + LANE_WIDTH * number.lane as f32 + (LANE_WIDTH - width) * 0.5;
        let colour = [DAMAGE_TEXT[0], DAMAGE_TEXT[1], DAMAGE_TEXT[2], 1.0 - age];
        dl.text(
            Vec2::new(x, base - DAMAGE_RISE * age),
            text,
            colour,
            DAMAGE_SIZE,
        );
    }
}

/// One line of text centred in the box `min..max`.
fn centred(
    dl: &mut DrawList,
    atlas: &FontAtlas,
    min: Vec2,
    max: Vec2,
    text: &str,
    colour: [f32; 4],
    size: f32,
) {
    let scale = size / NATURAL_FONT_SIZE;
    let extent = Vec2::new(
        atlas.text_width(text, scale),
        crcbl::ui::text::LINE_HEIGHT * scale,
    );
    dl.text(min + (max - min - extent) * 0.5, text, colour, size);
}

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{AbilityView, DamageView, HEALTH_MAX, MANA_MAX};

    const SCREEN: Vec2 = Vec2::new(960.0, 720.0);

    fn drawn(state: &RenderState) -> (DrawList, PageStats) {
        let mut dl = DrawList::new();
        let stats = draw(&mut dl, SCREEN, &FontAtlas::built_in(), state);
        (dl, stats)
    }

    /// A page with every element on it: both pools part-full, the banner up, two
    /// slots cooling and three numbers in the air.
    fn busy() -> RenderState {
        RenderState {
            tick: 601,
            wave: 2,
            banner: true,
            health: HEALTH_MAX / 2,
            mana: MANA_MAX / 4,
            abilities: [
                AbilityView {
                    cooldown: 0,
                    remaining: 0.0,
                },
                AbilityView {
                    cooldown: 105,
                    remaining: 1.75,
                },
                AbilityView {
                    cooldown: 0,
                    remaining: 0.0,
                },
                AbilityView {
                    cooldown: 255,
                    remaining: 4.25,
                },
            ],
            damage: vec![
                DamageView {
                    amount: 24,
                    age: 0.0,
                    lane: 0,
                },
                DamageView {
                    amount: 155,
                    age: 0.5,
                    lane: 1,
                },
                DamageView {
                    amount: 61,
                    age: 0.9,
                    lane: 2,
                },
            ],
        }
    }

    /// **The exact element count**, spelled out from what the page claims to
    /// show. A test that walked the commands and asserted inside a match arm
    /// would pass a page that drew nothing at all, which this codebase has been
    /// caught by twice.
    #[test]
    fn the_page_draws_exactly_one_element_for_every_thing_it_shows() {
        let state = busy();
        let (dl, stats) = drawn(&state);

        // Rects: the vitals panel, plus a track and a fill for each of the two
        // bars; the banner's block; one per ability slot, plus a sweep for each
        // of the two that are cooling.
        assert_eq!(stats.rects, 1 + 2 * 2 + 1 + ABILITY_COUNT + 2);
        // Outlines: one per bar, one for the banner, one per slot.
        assert_eq!(stats.outlines, 2 + 1 + ABILITY_COUNT);
        // Text: a caption and a readout per bar, the banner's line, a name and a
        // status per slot, and one number per damage entry.
        assert_eq!(stats.text, 2 * 2 + 1 + 2 * ABILITY_COUNT + 3);

        assert_eq!(stats.total(), dl.len(), "the page counted its own commands");
        assert_eq!(stats, PageStats::of(dl.commands()));
        assert_eq!(dl.len(), 35, "the whole page, on one screen");
    }

    /// An empty pool draws its track and nothing inside it, and a full one fills
    /// the track exactly — the two ends of the one arithmetic a bar is.
    #[test]
    fn a_bars_fill_spans_exactly_its_fraction_of_the_track() {
        let full = RenderState {
            health: HEALTH_MAX,
            ..RenderState::default()
        };
        let (dl, stats) = drawn(&full);
        // Health is full and mana is empty, so exactly one of the two bars has a
        // fill: the panel, two tracks, one fill.
        assert_eq!(stats.rects, 1 + 2 + 1 + ABILITY_COUNT);

        let widths = fill_widths(&dl);
        assert_eq!(widths, vec![BAR_WIDTH], "a full pool fills its whole track");

        let half = RenderState {
            health: HEALTH_MAX / 2,
            mana: MANA_MAX,
            ..RenderState::default()
        };
        let (dl, _) = drawn(&half);
        assert_eq!(fill_widths(&dl), vec![BAR_WIDTH * 0.5, BAR_WIDTH]);
    }

    /// The width of every bar fill on the page, in draw order.
    ///
    /// Found by colour rather than by position: a fill and the track under it
    /// share a left edge, and the fill is the only rect on the page painted in
    /// a pool's own colour.
    fn fill_widths(dl: &DrawList) -> Vec<f32> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color }
                    if *color == HEALTH_FILL || *color == MANA_FILL =>
                {
                    Some(max.x - min.x)
                }
                _ => None,
            })
            .collect()
    }

    /// The banner is on the page only while the ticker raises it.
    #[test]
    fn the_wave_banner_is_on_the_page_only_while_the_ticker_raises_it() {
        let up = busy();
        let (dl, with) = drawn(&up);
        assert!(
            text_of(&dl).iter().any(|line| line == "WAVE 2"),
            "the banner names its wave: {:?}",
            text_of(&dl),
        );

        let down = RenderState {
            banner: false,
            ..up
        };
        let (dl, without) = drawn(&down);
        assert!(!text_of(&dl).iter().any(|line| line.starts_with("WAVE")));
        assert_eq!(with.rects - without.rects, 1, "the banner's block");
        assert_eq!(with.outlines - without.outlines, 1);
        assert_eq!(with.text - without.text, 1);
    }

    /// A cooling slot is covered by a sweep and says how long it has left; a
    /// ready one is clear and says READY. Both states are on one page here, so
    /// neither can be the only one the row can draw.
    #[test]
    fn a_slot_shows_its_countdown_while_cooling_and_reads_ready_when_it_is_not() {
        let state = busy();
        let (dl, _) = drawn(&state);
        let text = text_of(&dl);

        assert_eq!(
            text.iter().filter(|line| *line == "READY").count(),
            2,
            "two slots are ready: {text:?}",
        );
        assert!(text.iter().any(|line| line == "1.8s"), "{text:?}");
        assert!(text.iter().any(|line| line == "4.2s"), "{text:?}");
        for ability in ABILITIES {
            assert!(text.iter().any(|line| line == ability.name), "{text:?}");
        }

        // The sweep covers the fraction of the slot still to run, **up from the
        // slot's bottom edge**. Slot 3 is on 255 of NOVA's 510 ticks, so
        // exactly half its height is covered. Both halves are asserted: a sweep
        // of the right height anchored to the top would retreat upwards, which
        // is the opposite of a cooldown emptying.
        let slot_bottom = SCREEN.y - MARGIN;
        let sweeps = sweeps(&dl);
        assert_eq!(sweeps.len(), 2, "one sweep per cooling slot");
        assert_eq!(sweeps[0].1 - sweeps[0].0, SLOT * (105.0 / 210.0));
        assert_eq!(sweeps[1].1 - sweeps[1].0, SLOT * 0.5);
        for (_, bottom) in sweeps {
            assert_eq!(bottom, slot_bottom, "a sweep hangs off the slot's floor");
        }
    }

    /// The top and bottom edge of every rect that is exactly one slot wide and
    /// shorter than a slot — the sweeps, in draw order.
    fn sweeps(dl: &DrawList) -> Vec<(f32, f32)> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, .. }
                    if max.x - min.x == SLOT && max.y - min.y < SLOT =>
                {
                    Some((min.y, max.y))
                }
                _ => None,
            })
            .collect()
    }

    /// Every damage number reaches the page, fading and climbing as it ages.
    #[test]
    fn a_damage_number_climbs_its_lane_and_fades_as_it_ages() {
        let state = busy();
        let (dl, _) = drawn(&state);
        let numbers: Vec<(String, Vec2, f32)> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text {
                    pos, text, color, ..
                } if text.chars().all(|c| c.is_ascii_digit()) => {
                    Some((text.clone(), *pos, color[3]))
                }
                _ => None,
            })
            .collect();
        assert_eq!(numbers.len(), 3, "one span per live number: {numbers:?}");
        assert_eq!(numbers[0].0, "24");
        assert_eq!(numbers[0].2, 1.0, "a fresh number is opaque");
        assert!(
            (numbers[2].2 - 0.1).abs() < 1e-6,
            "an old one has nearly faded out: {}",
            numbers[2].2,
        );
        assert!(
            numbers[2].1.y < numbers[0].1.y,
            "an older number sits higher up the lane",
        );
        assert!(
            numbers[0].1.x < numbers[1].1.x && numbers[1].1.x < numbers[2].1.x,
            "successive lanes run left to right",
        );
    }

    /// The page is laid out against the surface, so nothing runs off a window
    /// that is not the one it was written at.
    #[test]
    fn the_page_stays_inside_the_surface_at_every_extent() {
        for screen in [
            Vec2::new(960.0, 720.0),
            Vec2::new(1920.0, 1080.0),
            Vec2::new(800.0, 600.0),
            Vec2::new(1440.0, 400.0),
        ] {
            let mut dl = DrawList::new();
            let stats = draw(&mut dl, screen, &FontAtlas::built_in(), &busy());
            assert!(stats.total() > 0, "{screen:?} drew nothing");
            for command in dl.commands() {
                let (min, max) = match command {
                    DrawCommand::Rect { min, max, .. }
                    | DrawCommand::RectOutline { min, max, .. } => (*min, *max),
                    DrawCommand::Text { pos, .. } => (*pos, *pos),
                    DrawCommand::Line { from, to, .. } => (from.min(*to), from.max(*to)),
                    DrawCommand::Polyline { points, .. } => points.iter().fold(
                        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                        |(min, max), point| (min.min(*point), max.max(*point)),
                    ),
                };
                assert!(
                    min.x >= 0.0 && min.y >= 0.0 && max.x <= screen.x && max.y <= screen.y,
                    "{command:?} escapes a {screen:?} surface",
                );
            }
        }
    }

    /// The panel section reports the page that produced it, so a frame that drew
    /// nothing cannot report a page full of elements.
    #[test]
    fn the_page_section_reports_the_commands_the_frame_actually_emitted() {
        let (_, stats) = drawn(&busy());
        let mut section = crcbl::ui::DebugSection::default();
        crcbl::ui::DebugModule::debug_section(&stats, &mut section);
        assert_eq!(section.title(), "page");
        let rows: Vec<(&str, &str)> = section
            .rows()
            .iter()
            .map(|row| (row.label.as_str(), row.value.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![("rects", "12"), ("outlines", "7"), ("text", "16")],
        );
    }

    /// Every `Text` command on the page, in order.
    fn text_of(dl: &DrawList) -> Vec<String> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}
