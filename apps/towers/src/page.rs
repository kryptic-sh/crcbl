//! The overlay: the two shared numbers, the wave, the kind list, the build list,
//! and the banner a finished run wears.
//!
//! ```text
//!  ┌ towers ──────────┐
//!  │ GOLD         80  │
//!  │ LIVES     12/12  │
//!  │ WAVE       1/10  │
//!  │ NEXT      2.4 s  │
//!  │ CREEPS        3  │
//!  │ KILLS         5  │
//!  │ LEAKS         0  │
//!  └──────────────────┘
//!  │>1 BOLT       40g │
//!  │ 2 SPLASH     70g │
//!  │ 3 SLOW       50g │
//!  └──────────────────┘
//!  │ ENTRY   BOLT 50g │
//!  │ BEND   SLOW  MAX │
//!  │>EAST         40g │
//!  │ MIDDLE       40g │
//!  │ GATE         40g │
//!  └──────────────────┘
//!
//!             REFUSED: THAT PLOT IS TAKEN        ← for a few seconds
//!   LEFT/RIGHT pick a plot  1/2/3 a kind  B builds  U upgrades  N sends  R restarts
//! ```
//!
//! # Two lists, because a build is now two choices
//!
//! The **kind list** is what `1`/`2`/`3` pick and the **build list** is what
//! `LEFT`/`RIGHT` walk, and a `PlaceTower` command names one of each. Both are
//! presentation: the marker on either is a local convenience and
//! `crate::game::Stage::place_tower` is what decides whether a tower appears.
//!
//! A build row already built shows **what stands there and what stepping it up
//! costs** rather than a price the server would refuse, and one already stepped
//! up says so. That is the client agreeing with the server rather than replacing
//! it — the refusals still happen in `crate::game::Stage::place_tower` and
//! `Stage::upgrade_tower`, and the debug panel's `refused` row is where a
//! disagreement between the two would show up.
//!
//! **A price a player cannot pay is drawn in the warning colour**, on both
//! lists, which is the whole of why a refused build is never a surprise.
//!
//! # Rule 11 is half met, and not by this panel
//!
//! The build menu a click opens on a plot draws `.crpix` icons
//! (`crate::build_menu`, `crate::art`), but these lists, the creep icons and the
//! wave banner are still untextured rectangles and the engine's built-in font.
//! This sample is not claiming rule 11's exemption — a tower defense is exactly
//! the kind of game that should have pixel art — and
//! `docs/plan/sample/07-towers.md`'s status section says what is left.
//!
//! # One column, because the debug panel owns the other one
//!
//! All three panels stack down the left edge and nothing is drawn against the
//! right.
//! The debug panel is on by default here (sample rule 1) and it is tall and
//! right-aligned, so a build list in the top right is a build list a player
//! reads through a wall of frame timings — which is what the first screenshot
//! of this sample showed. `the_page_keeps_to_its_own_column` is what holds the
//! two apart.
//!
//! # Laid out against the surface
//!
//! Every position is derived from the extent the swapchain was actually
//! acquired at, so the page is correct in a resized window and in the headless
//! offscreen ring at whatever `--size` asked for.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::readout::{NATURAL_SCALE, ReadoutPanel, ReadoutRow};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::NATURAL_FONT_SIZE;

use crate::game::RenderState;
use crate::scene::Plot;
use crate::tower::{self, Tier};
use crate::wave::{Outcome, STARTING_LIVES, WAVES};

const PANEL_BG: [f32; 4] = [0.06, 0.09, 0.07, 0.80];
const BORDER: [f32; 4] = [0.34, 0.44, 0.36, 1.0];
const LABEL: [f32; 4] = [0.68, 0.76, 0.68, 1.0];
const VALUE: [f32; 4] = [0.95, 0.98, 0.94, 1.0];
/// What a reading in trouble is drawn in — the lives once they are down to a
/// third, and a plot nobody can afford.
const WARN: [f32; 4] = [0.95, 0.55, 0.36, 1.0];
/// What the highlighted build row and a won run are drawn in.
const PICKED: [f32; 4] = [0.55, 0.92, 0.62, 1.0];
/// What a lost run is drawn in.
const LOST: [f32; 4] = [0.95, 0.40, 0.36, 1.0];

/// The readout panel: this page's geometry and palette, over
/// [`crcbl::ui::readout`]'s layout.
const READOUT: ReadoutPanel = ReadoutPanel {
    inset: 16.0,
    width: 176.0,
    row_height: 18.0,
    pad: 8.0,
    border_width: 1.0,
    background: PANEL_BG,
    border: BORDER,
    label: LABEL,
};

/// The kind panel and the build panel, which stand under the readout and are
/// narrower because their rows are a name and a price rather than a reading.
const BUILD: ReadoutPanel = ReadoutPanel {
    width: 168.0,
    ..READOUT
};

/// What marks the highlighted row of the build list.
///
/// ASCII, and every other glyph on this page is too: the built-in atlas has no
/// arrows in it, so `\u{25b6}` and its friends draw as the missing-glyph box —
/// which on a five-row list is a marker a reader cannot tell from a bullet.
const MARKER: &str = ">";

/// What marks a plot whose tower has been stepped up: no price left to show.
const MAXED: &str = "MAX";

/// The control hint, which is the whole of what a first-time visitor needs.
///
/// `concat!` rather than a literal wrapped with a trailing `\`: slice 3a's two
/// new controls take the line past `rustfmt.toml`'s width, and rustfmt joins a
/// continued literal back onto one line **keeping the continuation's
/// indentation inside the string** — which is the hole
/// `tools/check-wrapped-strings.sh` exists to catch, arriving from the formatter
/// rather than from an author. A macro the formatter lays out normally cannot do
/// that.
const HINT: &str = concat!(
    "LEFT/RIGHT a plot   1/2/3 a kind   B builds   U upgrades   ",
    "N sends a wave   R restarts",
);

/// What the page drew, for the loop's own tests and its summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// The readout panel's rows.
fn readout(state: &RenderState) -> Vec<ReadoutRow> {
    vec![
        ReadoutRow::new("GOLD", format!("{}", state.gold), VALUE),
        ReadoutRow::new(
            "LIVES",
            format!("{}/{STARTING_LIVES}", state.lives),
            if state.lives * 3 <= STARTING_LIVES {
                WARN
            } else {
                VALUE
            },
        ),
        ReadoutRow::new("WAVE", format!("{}/{}", state.wave, WAVES.len()), VALUE),
        ReadoutRow::new(
            "NEXT",
            match state.next_wave_in {
                Some(seconds) => format!("{seconds:.1} s"),
                None => "--".into(),
            },
            VALUE,
        ),
        ReadoutRow::new("CREEPS", format!("{}", state.creeps_alive), VALUE),
        ReadoutRow::new("KILLS", format!("{}", state.kills), VALUE),
        ReadoutRow::new(
            "LEAKS",
            format!("{}", state.leaks),
            if state.leaks > 0 { WARN } else { VALUE },
        ),
    ]
}

/// A row's label, with the marker in front of it when it is the picked one.
///
/// The space on an unpicked row is not decoration: every row then starts at the
/// same column, so the marker reads as a marker rather than as the list
/// shifting.
fn marked(label: &str, picked: bool) -> String {
    format!(
        "{}{}",
        if picked { MARKER } else { " " },
        label.to_uppercase(),
    )
}

/// What a price is drawn in: [`WARN`] for one the purse cannot reach.
const fn affordable(gold: u32, cost: u32) -> [f32; 4] {
    if gold < cost { WARN } else { VALUE }
}

/// The kind panel's rows: one per [`tower::Kind`], the picked one marked.
///
/// Numbered, because the number **is** the key that picks it — a list a player
/// has to count to use is a list that needs a hint of its own.
fn kind_list(state: &RenderState, kind: tower::Kind) -> Vec<ReadoutRow> {
    tower::ALL
        .iter()
        .enumerate()
        .map(|(at, row)| {
            let cost = row.spec(Tier::Base).cost;
            ReadoutRow::new(
                marked(&format!("{} {}", at + 1, row.label()), *row == kind),
                format!("{cost}g"),
                affordable(state.gold, cost),
            )
        })
        .collect()
}

/// The build panel's rows: one per plot, the highlighted one marked.
///
/// An empty plot is priced at the **picked kind's** cost, because that is what
/// `B` would spend. A built one names what stands on it and what `U` would cost,
/// or [`MAXED`] once there is nothing left to buy.
fn build_list(
    state: &RenderState,
    plots: &[Plot],
    selected: u8,
    kind: tower::Kind,
) -> Vec<ReadoutRow> {
    plots
        .iter()
        .enumerate()
        .map(|(plot, at)| {
            let label = marked(&at.label, plot == usize::from(selected));
            let (reading, colour) = match state.towers[plot] {
                Some(tower) => match tower.tier {
                    Tier::Base => {
                        let cost = tower.kind.spec(Tier::Upgraded).cost;
                        (
                            format!("{} {cost}g", tower.kind.label().to_uppercase()),
                            affordable(state.gold, cost),
                        )
                    }
                    Tier::Upgraded => (
                        format!("{} {MAXED}", tower.kind.label().to_uppercase()),
                        PICKED,
                    ),
                },
                None => {
                    let cost = kind.spec(Tier::Base).cost;
                    (format!("{cost}g"), affordable(state.gold, cost))
                }
            };
            ReadoutRow::new(label, reading, colour)
        })
        .collect()
}

/// Draws `text` centred one row above the control hint, in the warning
/// colour: a line the player reads and then forgets — a command of theirs the
/// server refused, and why. How long it stays is the caller's.
pub fn draw_notice(list: &mut DrawList, atlas: &FontAtlas, extent: (u32, u32), text: &str) {
    let width = atlas.text_width(text, NATURAL_SCALE);
    list.text(
        Vec2::new(
            (extent.0 as f32 - width) * 0.5,
            extent.1 as f32 - READOUT.inset - 2.0 * READOUT.row_height,
        ),
        text.to_string(),
        WARN,
        NATURAL_FONT_SIZE,
    );
}

/// Draws the overlay into `list`, laid out against a surface of `extent`, with
/// one build row per entry of `plots`.
///
/// `atlas` is only measured against — the glyphs themselves are the UI pass's
/// business — and it is what right-aligns the readings against a proportional
/// font rather than against a guess; see [`ReadoutPanel::draw_at`].
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    state: &RenderState,
    plots: &[Plot],
    selected: u8,
    kind: tower::Kind,
) -> PageStats {
    let width = extent.0 as f32;
    let height = extent.1 as f32;

    let readout = readout(state);
    let kinds = kind_list(state, kind);
    let build = build_list(state, plots, selected, kind);
    READOUT.draw(list, atlas, &readout);
    // Stacked, each panel under the one before it with an inset between — so a
    // row added to any of them moves the ones below rather than drawing over
    // them.
    let mut below = READOUT.inset + READOUT.height(readout.len());
    for rows in [&kinds, &build] {
        below += READOUT.inset;
        BUILD.draw_at(list, atlas, Vec2::new(READOUT.inset, below), rows);
        below += BUILD.height(rows.len());
    }

    // The banner, which is the one thing on screen a player who has stopped
    // watching the field still reads.
    if let Some((text, colour)) = match state.outcome {
        Outcome::Playing => None,
        Outcome::Won => Some(("EVERY WAVE HELD", PICKED)),
        Outcome::Lost => Some(("OVERRUN", LOST)),
    } {
        let banner = atlas.text_width(text, NATURAL_SCALE);
        list.text(
            Vec2::new((width - banner) * 0.5, height * 0.5 - READOUT.row_height),
            text.to_string(),
            colour,
            NATURAL_FONT_SIZE,
        );
    }

    READOUT.hint(list, atlas, extent, HINT);

    PageStats {
        commands: list.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::draw_list::DrawCommand;

    use crate::map::{MAX_PLOTS, Map};
    use crate::tower::{Kind, TowerView};

    /// A window the page is laid out against.
    const EXTENT: (u32, u32) = (960, 720);

    /// The committed field's plots, which is what every build list here lists.
    fn plots() -> Vec<Plot> {
        Map::built_in().plots().to_vec()
    }

    /// Every `Text` command the page produced.
    fn text(list: &DrawList) -> Vec<String> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A built tower of `kind` at `tier`, as the frame sees one.
    const fn built(kind: Kind, tier: Tier) -> Option<TowerView> {
        Some(TowerView {
            kind,
            tier,
            working: false,
        })
    }

    /// A run part way through the fourth wave with three towers up: a base bolt
    /// tower, an upgraded one, and a slow tower.
    fn playing() -> RenderState {
        RenderState {
            gold: 64,
            lives: 9,
            wave: 4,
            kills: 5,
            leaks: 3,
            creeps_alive: 3,
            next_wave_in: None,
            towers: {
                let mut towers = [None; MAX_PLOTS];
                towers[..3].copy_from_slice(&[
                    built(Kind::Bolt, Tier::Base),
                    built(Kind::Bolt, Tier::Upgraded),
                    built(Kind::Slow, Tier::Base),
                ]);
                towers
            },
            ..RenderState::default()
        }
    }

    /// **Every number the game turns on is on the page**, and every plot has a
    /// row. A readout that dropped one is a player who cannot see why a build
    /// was refused.
    #[test]
    fn the_readout_carries_every_number_a_player_plays_on() {
        let mut list = DrawList::new();
        let state = playing();
        let stats = draw(
            &mut list,
            &FontAtlas::built_in(),
            EXTENT,
            &state,
            &plots(),
            2,
            Kind::Bolt,
        );
        assert_eq!(stats.commands, list.len());
        assert!(stats.commands > 0, "the page drew nothing");

        let text = text(&list);
        for expected in [
            "GOLD".to_string(),
            "64".to_string(),
            format!("{}/{STARTING_LIVES}", state.lives),
            format!("{}/{}", state.wave, WAVES.len()),
            "5".to_string(),
            "3".to_string(),
        ] {
            assert!(text.contains(&expected), "{expected} is not on the page");
        }
        for plot in plots() {
            assert!(
                text.iter()
                    .any(|drawn| drawn.contains(&plot.label.to_uppercase())),
                "{} has no row on the build list",
                plot.label,
            );
        }
        assert!(
            text.contains(&HINT.to_string()),
            "the control hint is missing"
        );
    }

    /// **Every kind a player can build has a row, numbered by the key that picks
    /// it, and priced.**
    ///
    /// A kind with no row is a kind nobody discovers: the key works and nothing
    /// on screen says it exists.
    #[test]
    fn every_kind_has_a_numbered_priced_row() {
        let mut list = DrawList::new();
        draw(
            &mut list,
            &FontAtlas::built_in(),
            EXTENT,
            &playing(),
            &plots(),
            0,
            Kind::Splash,
        );
        let text = text(&list);
        for (at, kind) in tower::ALL.iter().enumerate() {
            let row = text
                .iter()
                .find(|drawn| drawn.contains(&kind.label().to_uppercase()))
                .unwrap_or_else(|| panic!("the {} kind has no row", kind.label()));
            assert!(
                row.contains(&format!("{}", at + 1)),
                "the {} row is not numbered {}: {row}",
                kind.label(),
                at + 1,
            );
            assert!(
                text.contains(&format!("{}g", kind.spec(Tier::Base).cost)),
                "the {} kind is not priced",
                kind.label(),
            );
        }
    }

    /// **A built plot says what stands on it and what stepping it up costs**, and
    /// an upgraded one says there is nothing left to buy.
    ///
    /// The third plot is the control: an empty one is priced at the picked
    /// kind's cost, which is what `B` would actually spend — a page that priced
    /// every row the same would be telling a player the wrong number for two of
    /// the three kinds.
    #[test]
    fn a_built_plot_is_priced_for_its_upgrade_and_a_maxed_one_is_not() {
        let state = playing();
        let rows = build_list(&state, &plots(), 0, Kind::Splash);

        let base = &rows[0];
        assert!(
            base.value.contains("BOLT")
                && base
                    .value
                    .contains(&format!("{}g", Kind::Bolt.spec(Tier::Upgraded).cost)),
            "a base bolt tower's row does not price its upgrade: {}",
            base.value,
        );
        assert!(
            rows[1].value.contains(MAXED),
            "an upgraded tower's row still shows a price: {}",
            rows[1].value,
        );
        assert!(
            rows[2].value.contains("SLOW"),
            "the slow tower's row does not name its kind: {}",
            rows[2].value,
        );
        assert_eq!(
            rows[3].value,
            format!("{}g", Kind::Splash.spec(Tier::Base).cost),
            "an empty plot is not priced at the picked kind's cost",
        );

        // …and with a different kind picked, the empty rows move with it.
        let other = build_list(&state, &plots(), 0, Kind::Bolt);
        assert_eq!(
            other[3].value,
            format!("{}g", Kind::Bolt.spec(Tier::Base).cost),
            "the empty rows do not follow the picked kind",
        );
        assert_ne!(
            rows[3].value, other[3].value,
            "the two kinds are priced the same, so this proves nothing",
        );
    }

    /// **A price the purse cannot reach is drawn in [`WARN`]**, and one it can is
    /// not. The whole of why a refused build is never a surprise.
    #[test]
    fn a_price_the_purse_cannot_reach_is_drawn_as_a_warning() {
        let dear = Kind::Splash.spec(Tier::Base).cost;
        let flush = RenderState {
            gold: dear,
            ..RenderState::default()
        };
        let broke = RenderState {
            gold: dear - 1,
            ..RenderState::default()
        };
        let row_of =
            |state: &RenderState| kind_list(state, Kind::Splash)[Kind::Splash.index()].colour;
        assert_eq!(
            row_of(&flush),
            VALUE,
            "a price the purse covers is warned about"
        );
        assert_eq!(row_of(&broke), WARN, "a price the purse is short of is not");
    }

    /// **The highlighted plot and the picked kind are the ones marked**, and only
    /// one of each — the markers are the whole of what says where `B` builds and
    /// what it builds there.
    #[test]
    fn exactly_the_selected_plot_and_kind_are_marked() {
        let plots = plots();
        for selected in 0..plots.len() as u8 {
            for kind in tower::ALL {
                let mut list = DrawList::new();
                draw(
                    &mut list,
                    &FontAtlas::built_in(),
                    EXTENT,
                    &RenderState::default(),
                    &plots,
                    selected,
                    kind,
                );
                let marked: Vec<String> = text(&list)
                    .into_iter()
                    .filter(|drawn| drawn.starts_with(MARKER))
                    .collect();
                assert_eq!(
                    marked.len(),
                    2,
                    "{marked:?} rows are marked rather than one plot and one kind",
                );
                assert!(
                    marked
                        .iter()
                        .any(|row| row.contains(&plots[usize::from(selected)]
                            .label
                            .to_uppercase())),
                    "no marker on {}: {marked:?}",
                    plots[usize::from(selected)].label,
                );
                assert!(
                    marked
                        .iter()
                        .any(|row| row.contains(&kind.label().to_uppercase())),
                    "no marker on the {} kind: {marked:?}",
                    kind.label(),
                );
            }
        }
    }

    /// **The player's panels keep to the left column**, which is the half of
    /// the surface the debug panel does not take, and they do not draw over each
    /// other.
    ///
    /// A build list drawn in the top right is legible in a test and unreadable on
    /// screen, and only a position assertion can tell the two apart. The
    /// overlap check is the same argument vertically: three stacked panels whose
    /// offsets were worked out by hand would read as one panel with the wrong
    /// rows in it.
    #[test]
    fn the_page_keeps_to_its_own_column() {
        let mut list = DrawList::new();
        draw(
            &mut list,
            &FontAtlas::built_in(),
            EXTENT,
            &playing(),
            &plots(),
            2,
            Kind::Bolt,
        );
        let middle = EXTENT.0 as f32 * 0.5;
        let mut panels: Vec<(f32, f32)> = Vec::new();
        for command in list.commands() {
            let DrawCommand::Rect { min, max, .. } = command else {
                continue;
            };
            assert!(
                max.x < middle,
                "a panel runs from {} to {}, across the middle at {middle}",
                min.x,
                max.x,
            );
            panels.push((min.y, max.y));
        }
        assert_eq!(
            panels.len(),
            3,
            "the page drew {} panels rather than three",
            panels.len()
        );
        panels.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in panels.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "a panel ending at {} overlaps the next, which starts at {}",
                pair[0].1,
                pair[1].0,
            );
        }
        assert!(
            panels[panels.len() - 1].1 < EXTENT.1 as f32,
            "the last panel runs off the bottom of the window at {}",
            panels[panels.len() - 1].1,
        );
    }

    /// **The control hint fits the window it is drawn in.**
    ///
    /// [`ReadoutPanel::hint`] centres one line and neither wraps nor scales it,
    /// so a hint that outgrew the surface is drawn with both ends off the
    /// screen — which is what slice 3a's two extra controls did, and what only a
    /// screenshot caught. Measured with the atlas the UI pass draws with, at the
    /// default `--size`, against the same [`ReadoutPanel::inset`] the panels keep.
    #[test]
    fn the_control_hint_fits_the_window_it_is_drawn_in() {
        let atlas = FontAtlas::built_in();
        let measured = atlas.text_width(HINT, NATURAL_SCALE);
        let room = EXTENT.0 as f32 - 2.0 * READOUT.inset;
        assert!(
            measured <= room,
            "the hint is {measured:.0} px wide in a {room:.0} px window: {HINT}",
        );
        // …and the banner, which is centred the same way, and every refusal's
        // notice: the other strings on this page nothing bounds.
        for banner in ["EVERY WAVE HELD", "OVERRUN"] {
            assert!(
                atlas.text_width(banner, NATURAL_SCALE) <= room,
                "the {banner} banner does not fit the window",
            );
        }
        for refusal in crate::game::Refusal::ALL {
            let notice = format!("REFUSED: {}", refusal.label());
            assert!(
                atlas.text_width(&notice, NATURAL_SCALE) <= room,
                "the {notice} notice does not fit the window",
            );
        }
    }

    /// **A notice is drawn centred above the hint, in the warning colour**,
    /// clear of the hint's row and inside the surface.
    #[test]
    fn a_notice_is_drawn_centred_above_the_hint_in_the_warning_colour() {
        let atlas = FontAtlas::built_in();
        let notice = "REFUSED: THAT PLOT IS TAKEN";
        let mut list = DrawList::new();
        draw_notice(&mut list, &atlas, EXTENT, notice);
        let [
            DrawCommand::Text {
                pos: position,
                text,
                color,
                ..
            },
        ] = list.commands()
        else {
            panic!("not one line: {:?}", list.commands());
        };
        assert_eq!(text, notice);
        assert_eq!(*color, WARN);
        let width = atlas.text_width(notice, NATURAL_SCALE);
        assert!(
            (position.x + width * 0.5 - EXTENT.0 as f32 * 0.5).abs() < 1.0,
            "not centred: {position:?}"
        );
        let hint_top = EXTENT.1 as f32 - READOUT.inset - READOUT.row_height;
        assert!(
            position.y >= 0.0 && position.y + READOUT.row_height <= hint_top,
            "not above the hint: {position:?}"
        );
    }

    /// **A finished run wears its banner, and a running one does not.** The
    /// second half is the control: a page that always drew one would say
    /// `OVERRUN` over a field that is doing fine.
    #[test]
    fn a_finished_run_says_so_and_a_running_one_does_not() {
        let banner_of = |outcome: Outcome| {
            let mut list = DrawList::new();
            draw(
                &mut list,
                &FontAtlas::built_in(),
                EXTENT,
                &RenderState {
                    outcome,
                    ..playing()
                },
                &plots(),
                0,
                Kind::Bolt,
            );
            text(&list)
        };
        assert!(
            banner_of(Outcome::Won)
                .iter()
                .any(|drawn| drawn == "EVERY WAVE HELD")
        );
        assert!(
            banner_of(Outcome::Lost)
                .iter()
                .any(|drawn| drawn == "OVERRUN")
        );
        for drawn in banner_of(Outcome::Playing) {
            assert_ne!(drawn, "EVERY WAVE HELD");
            assert_ne!(drawn, "OVERRUN");
        }
    }
}
