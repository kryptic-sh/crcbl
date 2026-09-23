//! The overlay: the room on screen's counters, and its gaps stated plainly.
//!
//! ```text
//!  ┌ tumble ───────────────────────┐
//!  │ ROOM                     wall │
//!  │ TICK                      612 │
//!  │ AWAKE                     120 │
//!  │ SLEEPING                   37 │
//!  │ ISLANDS            22 + 9 zzz │
//!  │ PAIRS                     431 │
//!  │ CONTACTS                  164 │
//!  │ BEGUN                    5230 │
//!  │ ENDED                    5066 │
//!  │ BROADPHASE              21 us │
//!  │ NARROW                  38 us │
//!  │ SOLVER                 102 us │
//!  │ AT REST                  3 us │
//!  │ PENETRATION           0.42 mm │
//!  │ BOUNCE            0.53 / 0.55 │
//!  │ PTS/MANIFOLD             1.42 │
//!  │ PERSISTED               91.0% │
//!  │ SWEPT              3 / 7 cand │
//!  │ SWEEP HITS                146 │
//!  │ DROPPED TIME         819.5 ms │
//!  │ DROPPED                   232 │
//!  │ STEP                   530 us │
//!  │ HASH         20be321e0066f5de │
//!  └───────────────────────────────┘
//!
//!   balls, pills and cubes bounce down the wall - keys 1 to 6 pick a room
//! ```
//!
//! The Tower room shows the pyramid's contact rows — its solver time is the
//! benchmark — then how far the pyramid's and the column's top boxes have
//! drifted, the column's own solver time and the dominoes down. The Bullets
//! room adds the shots fired and the tunnels its three sensors counted — the
//! plate's, the brick wall's and the plank's. The Bridge room adds the momentum
//! the cradle's first ball arrived with and its last left with, the bridge's
//! sag against the unloaded chain's, its hinges whole and the joints broken,
//! and the worst joint error in the room.
//!
//! `docs/plan/sample/24-tumble.md` asks that a scene the engine cannot produce
//! yet ship labelled as the gap it is, and each room's hint line is that label.

use crcbl::ui::draw_list::DrawList;
use crcbl::ui::readout::{ReadoutPanel, ReadoutRow};
use crcbl::ui::text::FontAtlas;

use crate::scene::{Reading, Tally, View};

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];

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

/// The Spin room's line.
pub const SPIN_HINT: &str =
    "handle flips in zero g - box lands flat on its corners - keys 1 to 6 pick a room";
/// The wall's line.
pub const WALL_HINT: &str = "balls, pills and cubes bounce down the wall - keys 1 to 6 pick a room";
/// The pit's line, and its gaps.
pub const PIT_HINT: &str =
    "1000 balls that sleep once still - no overflow or despawn (rung 6) - keys 1 to 6";
/// The Tower room's line.
pub const TOWER_HINT: &str =
    "counters: the pyramid's; column at 8 substeps, 90 Hz; all sleep once still";
/// The Bullets room's line, and its gap.
pub const BULLETS_HINT: &str =
    "point-blank cannon, spinning plank - dynamic pairs swept only for bullets";
/// The Bridge room's line, and its gap.
pub const BRIDGE_HINT: &str =
    "cradle, plank bridge at 8 substeps an anvil snaps, ragdolls - no 6-DOF joint";

/// The hint for a room.
#[must_use]
pub const fn hint(view: View) -> &'static str {
    match view {
        View::Spin => SPIN_HINT,
        View::Wall => WALL_HINT,
        View::Pit => PIT_HINT,
        View::Tower => TOWER_HINT,
        View::Bullets => BULLETS_HINT,
        View::Bridge => BRIDGE_HINT,
    }
}

/// What the page drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// A stage time, or that there is no clock to take one.
fn micros(seconds: Option<f64>) -> String {
    seconds.map_or_else(
        || "no clock".to_owned(),
        |seconds| format!("{:.0} us", seconds * 1.0e6),
    )
}

/// A share as a percentage, or a dash where there is nothing to share.
fn percent(share: Option<f64>) -> String {
    share.map_or_else(|| "-".to_owned(), |share| format!("{:.1}%", share * 100.0))
}

/// A length in millimetres.
fn millimetres(metres: f64) -> String {
    format!("{:.2} mm", metres * 1.0e3)
}

/// Rung 1's row, rung 2's, rung 3's and rung 4's, for a room with contacts;
/// rung 5's joint rows are the Bridge room's own.
fn contact_rows(rows: &mut Vec<ReadoutRow>, tally: &Tally) {
    rows.push(ReadoutRow::new("AWAKE", tally.bodies.to_string(), VALUE));
    rows.push(ReadoutRow::new(
        "SLEEPING",
        tally.sleeping.to_string(),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "ISLANDS",
        format!("{} + {} zzz", tally.islands, tally.sleeping_islands),
        VALUE,
    ));
    rows.push(ReadoutRow::new("PAIRS", tally.pairs.to_string(), VALUE));
    rows.push(ReadoutRow::new(
        "CONTACTS",
        tally.touching.to_string(),
        VALUE,
    ));
    rows.push(ReadoutRow::new("BEGUN", tally.begun.to_string(), VALUE));
    rows.push(ReadoutRow::new("ENDED", tally.ended.to_string(), VALUE));
    let stages = tally.stages;
    rows.push(ReadoutRow::new(
        "BROADPHASE",
        micros(stages.map(|s| s.broadphase)),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "NARROW",
        micros(stages.map(|s| s.narrow_phase)),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "SOLVER",
        micros(stages.map(|s| s.solver)),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "AT REST",
        match (stages, tally.solver_at_rest) {
            (None, _) => micros(None),
            (Some(_), None) => "not yet".to_owned(),
            (Some(_), rest) => micros(rest),
        },
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "PENETRATION",
        millimetres(tally.worst_penetration),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "BOUNCE",
        match (tally.bounce_ratio(), tally.restitution()) {
            (Some(ratio), Some(asked)) => format!("{ratio:.2} / {asked:.2}"),
            _ => "none yet".to_owned(),
        },
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "PTS/MANIFOLD",
        tally
            .points_per_manifold()
            .map_or_else(|| "-".to_owned(), |points| format!("{points:.2}")),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "PERSISTED",
        percent(tally.persisted_ratio()),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "SWEPT",
        format!("{} / {} cand", tally.swept, tally.sweep_candidates),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "SWEEP HITS",
        tally.sweep_hits.to_string(),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "DROPPED TIME",
        format!("{:.1} ms", tally.dropped_time * 1.0e3),
        VALUE,
    ));
}

/// Draws the overlay into `list`, laid out against a surface of `extent`.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    reading: &Reading,
) -> PageStats {
    let mut rows = vec![
        ReadoutRow::new("ROOM", reading.view.name(), VALUE),
        ReadoutRow::new("TICK", reading.tick.to_string(), VALUE),
    ];
    match reading.view {
        View::Spin => {
            let spin = &reading.spin;
            rows.push(ReadoutRow::new("FLIPS", spin.flips.to_string(), VALUE));
            rows.push(ReadoutRow::new(
                "L DRIFT",
                format!("{:.1e}", spin.momentum_drift),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "E DRIFT",
                format!("{:.1e}", spin.energy_drift),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "SPIN",
                format!("{:.2} rad/s", spin.handle_spin),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "BOX Y",
                format!("{:.3} m", spin.box_height),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "BOX TILT",
                format!("{:.1e}", spin.box_tilt),
                VALUE,
            ));
            rows.push(ReadoutRow::new("DROPS", spin.drops.to_string(), VALUE));
            rows.push(ReadoutRow::new(
                "PENETRATION",
                millimetres(spin.contacts.worst_penetration),
                VALUE,
            ));
        }
        View::Tower => {
            let tower = &reading.tower;
            contact_rows(&mut rows, &tower.pyramid);
            rows.push(ReadoutRow::new(
                "PYRAMID TOP",
                millimetres(tower.pyramid_drift),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "COLUMN TOP",
                millimetres(tower.column_drift),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "COLUMN SOLVER",
                micros(tower.column.stages.map(|s| s.solver)),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "DOMINOES",
                format!("{} / {}", tower.dominoes_down, crate::tower::DOMINOES),
                VALUE,
            ));
        }
        View::Wall => {
            contact_rows(&mut rows, &reading.wall.contacts);
            rows.push(ReadoutRow::new(
                "DROPPED",
                reading.wall.dropped.to_string(),
                VALUE,
            ));
        }
        View::Pit => {
            contact_rows(&mut rows, &reading.pit.contacts);
            rows.push(ReadoutRow::new(
                "BALLS",
                reading.pit.balls.to_string(),
                VALUE,
            ));
        }
        View::Bullets => {
            let bullets = &reading.bullets;
            contact_rows(&mut rows, &bullets.contacts);
            rows.push(ReadoutRow::new("SHOTS", bullets.shots.to_string(), VALUE));
            rows.push(ReadoutRow::new(
                "TUNNELS",
                format!(
                    "{} / {} / {}",
                    bullets.plate_tunnels, bullets.wall_tunnels, bullets.plank_tunnels
                ),
                VALUE,
            ));
        }
        View::Bridge => {
            let bridge = &reading.bridge;
            contact_rows(&mut rows, &bridge.contacts);
            rows.push(ReadoutRow::new(
                "CRADLE IN/OUT",
                format!("{:.3} / {:.3}", bridge.cradle_in, bridge.cradle_out),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "SAG",
                format!("{:.3} m / {:.3}", bridge.sag, bridge.chain_sag),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "HINGES",
                format!("{} / {}", bridge.hinges, crate::bridge::PLANKS + 1),
                VALUE,
            ));
            rows.push(ReadoutRow::new("BROKEN", bridge.broken.to_string(), VALUE));
            rows.push(ReadoutRow::new(
                "JOINTS",
                bridge.contacts.joints.to_string(),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "JOINT ERROR",
                millimetres(bridge.contacts.joint_error),
                VALUE,
            ));
            rows.push(ReadoutRow::new(
                "JOINT ANGLE",
                format!("{:.2} mrad", bridge.contacts.joint_angle_error * 1.0e3),
                VALUE,
            ));
        }
    }
    rows.push(ReadoutRow::new(
        "STEP",
        micros(reading.step_micros.map(|m| m * 1.0e-6)),
        VALUE,
    ));
    rows.push(ReadoutRow::new(
        "HASH",
        format!("{:016x}", reading.hash),
        VALUE,
    ));

    PANEL.draw(list, atlas, &rows);
    PANEL.hint(list, atlas, extent, hint(reading.view));

    PageStats {
        commands: list.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::BridgeReading;
    use crate::bullets::BulletsReading;
    use crate::pit::PitReading;
    use crate::scene::View;
    use crate::spin::SpinReading;
    use crate::tower::TowerReading;
    use crate::wall::WallReading;
    use crcbl::phys::StageTimes;

    fn reading(view: View) -> Reading {
        let tally = Tally {
            bodies: 120,
            sleeping: 37,
            islands: 22,
            sleeping_islands: 9,
            solver_at_rest: Some(3e-6),
            pairs: 431,
            touching: 164,
            points: 233,
            persisted: 212,
            begun: 5230,
            ended: 5066,
            worst_penetration: 4.2e-4,
            swept: 3,
            sweep_candidates: 7,
            sweep_hits: 146,
            dropped_time: 0.8195,
            stages: Some(StageTimes {
                broadphase: 21e-6,
                narrow_phase: 38e-6,
                solver: 102e-6,
                islands: 5e-6,
                continuous: 4e-6,
            }),
            ..Tally::default()
        };
        Reading {
            tick: 612,
            view,
            spin: SpinReading {
                flips: 4,
                momentum_drift: 3.1e-12,
                energy_drift: 6.2e-12,
                handle_spin: 6.0,
                box_height: 0.3,
                box_tilt: 0.0,
                drops: 3,
                contacts: tally,
            },
            wall: WallReading {
                dropped: 232,
                live: 120,
                contacts: tally,
            },
            pit: PitReading {
                balls: 1000,
                contacts: tally,
            },
            tower: TowerReading {
                column_drift: 0.0118,
                column_sway: 8.5e-4,
                pyramid_drift: 0.0151,
                pyramid_sway: 5.0e-5,
                dominoes_down: 9,
                runs: 2,
                pyramid: tally,
                column: tally,
            },
            bullets: BulletsReading {
                shots: 40,
                plate_tunnels: 0,
                wall_tunnels: 1,
                plank_tunnels: 2,
                contacts: tally,
            },
            bridge: BridgeReading {
                cradle_in: 1.981,
                cradle_out: 1.973,
                cradle_swings: 2,
                sag: 2.211,
                chain_sag: 1.953,
                crates: 3,
                hinges: 20,
                builds: 1,
                broken: 2,
                ragdoll_drops: 2,
                contacts: Tally {
                    joints: 64,
                    joint_error: 3.4e-3,
                    joint_angle_error: 1.25e-3,
                    ..tally
                },
            },
            step_micros: Some(530.0),
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

    /// **Each room's page carries its rung's counters and its gap**: rung 0's
    /// in the Spin room, rungs 1 to 3's wherever there are contacts, and the
    /// Tower room's drifts and dominoes.
    #[test]
    fn each_room_carries_its_counters_and_its_gap() {
        let atlas = FontAtlas::built_in();
        for (view, wants) in [
            (View::Spin, &["spin", "4", "3.1e-12", "0.300 m", "3"][..]),
            (
                View::Wall,
                &[
                    "wall",
                    "120",
                    "37",
                    "22 + 9 zzz",
                    "431",
                    "164",
                    "5230",
                    "5066",
                    "21 us",
                    "38 us",
                    "102 us",
                    "3 us",
                    "0.42 mm",
                    "1.42",
                    "91.0%",
                    "232",
                    "3 / 7 cand",
                    "819.5 ms",
                ][..],
            ),
            (
                View::Pit,
                &[
                    "pit",
                    "1000",
                    "37",
                    "22 + 9 zzz",
                    "431",
                    "102 us",
                    "3 us",
                    "1.42",
                ][..],
            ),
            (
                View::Tower,
                &[
                    "tower",
                    "37",
                    "22 + 9 zzz",
                    "431",
                    "3 us",
                    "1.42",
                    "91.0%",
                    "15.10 mm",
                    "11.80 mm",
                    "102 us",
                    "9 / 15",
                ][..],
            ),
            (
                View::Bullets,
                &[
                    "bullets",
                    "37",
                    "3 / 7 cand",
                    "146",
                    "819.5 ms",
                    "40",
                    "0 / 1 / 2",
                ][..],
            ),
            (
                View::Bridge,
                &[
                    "bridge",
                    "37",
                    "1.981 / 1.973",
                    "2.211 m / 1.953",
                    "20 / 22",
                    "2",
                    "64",
                    "3.40 mm",
                    "1.25 mrad",
                ][..],
            ),
        ] {
            let mut list = DrawList::new();
            let stats = draw(&mut list, &atlas, (960, 720), &reading(view));
            assert!(stats.commands > 0, "the page drew nothing");
            let text = text(&list);
            for want in wants.iter().chain(&["612", "530 us", "20be321e0066f5de"]) {
                assert!(
                    text.iter().any(|t| t == want),
                    "{view:?}: {want:?} is missing: {text:?}"
                );
            }
            assert!(
                text.iter().any(|t| t == hint(view)),
                "{view:?}: the gap is not labelled: {text:?}"
            );
        }
        assert!(PIT_HINT.contains("rung 6"));
        assert!(BULLETS_HINT.contains("bullets"));
        assert!(BRIDGE_HINT.contains("6-DOF"));
        assert!(PIT_HINT.contains("sleep") && TOWER_HINT.contains("sleep"));
    }

    /// Before anything has been at rest the solver's time at rest says so,
    /// rather than showing a time.
    #[test]
    fn no_time_at_rest_until_everything_has_slept() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let mut restless = reading(View::Pit);
        restless.pit.contacts.solver_at_rest = None;
        draw(&mut list, &atlas, (960, 720), &restless);
        assert!(text(&list).iter().any(|t| t == "not yet"));
    }

    /// Every hint fits across the default window, so no gap is cut off.
    #[test]
    fn every_hint_fits_the_default_window() {
        let atlas = FontAtlas::built_in();
        for view in [
            View::Spin,
            View::Wall,
            View::Pit,
            View::Tower,
            View::Bullets,
            View::Bridge,
        ] {
            let width = atlas.text_width(hint(view), crcbl::ui::readout::NATURAL_SCALE);
            assert!(
                width < 960.0 - 2.0 * PANEL.inset,
                "{view:?}'s hint is {width} px wide"
            );
        }
    }

    /// A build with no clock says so rather than reading zero.
    #[test]
    fn an_untimed_step_says_there_is_no_clock() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let mut untimed = reading(View::Pit);
        untimed.step_micros = None;
        untimed.pit.contacts.stages = None;
        draw(&mut list, &atlas, (960, 720), &untimed);
        assert!(text(&list).iter().filter(|t| *t == "no clock").count() >= 4);
    }
}
