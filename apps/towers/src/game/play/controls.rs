//! What a tool takes part in a played field with: this game's
//! [`PlayControls`] — its four commands as actions, the encoder solo's client
//! goes through, and the run's numbers read off the world.
//!
//! # The bytes are the client's
//!
//! An action becomes a [`Controls`], as a key press does in `crate::app`, and
//! the [`Controls`] becomes the frame through the one conversion
//! [`Game::set_controls`](crate::game::Game::set_controls) makes — so a tool's
//! `Place tower` and a player's are the same four bytes, decoded and
//! validated by the same `Intent::from_wire` and `Stage::place_tower`. A plot
//! is the picked plot's place in the plots chunk, which is how
//! [`Map::load`](crate::map::Map::load) numbers them and so how a command
//! names one.
//!
//! # The readout is a system in the world
//!
//! The stage is behind the module's own lock, and
//! [`PlayControls::status`] is handed a world: so the module registers
//! [`readout`] in the world it plays in — a system holding the same stage,
//! with no entities — and the status and the refusals are read through it.

use std::sync::{Arc, Mutex};

use crcbl::ecs::{DebugCtx, Entity, SystemTrait, World};
use crcbl::registry::{ParamKind, PlayAction, PlayArg, PlayControls};

use crate::game::{Controls, Intent, PLOT_NONE, Stage, lock, stats_of};
use crate::scene::PLOTS;
use crate::tower;
use crate::wave::{STARTING_LIVES, WAVES};

/// This game's controls, registered under its module's system.
pub(super) const CONTROLS: PlayControls = PlayControls {
    actions: &ACTIONS,
    encode,
    status,
    refusals,
};

/// [`ACTIONS`]' index of `PlaceTower`.
const PLACE: usize = 0;
/// [`ACTIONS`]' index of `StartWave`.
const START_WAVE: usize = 1;
/// [`ACTIONS`]' index of `UpgradeTower`.
const UPGRADE: usize = 2;
/// [`ACTIONS`]' index of `Restart`.
const RESTART: usize = 3;

/// Every tower kind's label, in [`tower::ALL`]'s order: the choice a build
/// takes, whose index is the kind.
const KIND_LABELS: [&str; tower::KINDS] = {
    let mut labels = [""; tower::KINDS];
    let mut index = 0;
    while index < tower::KINDS {
        labels[index] = tower::ALL[index].label();
        index += 1;
    }
    labels
};

/// The four commands this game's client sends, as a tool offers them. An
/// upgrade picks the plot its tower stands on: a tower is a runtime entity,
/// which a tool draws and does not pick.
const ACTIONS: [PlayAction; 4] = [
    PlayAction {
        name: "Place tower",
        params: &[ParamKind::Picked(PLOTS), ParamKind::Choice(&KIND_LABELS)],
    },
    PlayAction {
        name: "Start wave",
        params: &[],
    },
    PlayAction {
        name: "Upgrade",
        params: &[ParamKind::Picked(PLOTS)],
    },
    PlayAction {
        name: "Restart",
        params: &[],
    },
];

/// The frame solo's client sends for the action at `action` taking `args`.
///
/// # Errors
///
/// A plot whose place no command frame can carry, a kind past the table, or
/// an action this game does not have — none of which arrives through
/// [`crcbl::registry::Registry::encode_play`], which checks the arguments
/// against [`ACTIONS`] first, and each refused here rather than trusted.
fn encode(action: usize, args: &[PlayArg]) -> Result<Vec<u8>, String> {
    let controls = match (action, args) {
        (PLACE, &[PlayArg::Picked(plot), PlayArg::Choice(kind)]) => Controls {
            place: Some(plot_byte(plot)?),
            kind: *tower::ALL
                .get(kind)
                .ok_or_else(|| format!("towers has no tower kind {kind}"))?,
            ..Controls::default()
        },
        (START_WAVE, []) => Controls {
            start_wave: true,
            ..Controls::default()
        },
        (UPGRADE, &[PlayArg::Picked(plot)]) => Controls {
            upgrade: Some(plot_byte(plot)?),
            ..Controls::default()
        },
        (RESTART, []) => Controls {
            restart: true,
            ..Controls::default()
        },
        _ => return Err(format!("towers has no action {action} taking {args:?}")),
    };
    Ok(Intent::from(controls).to_wire())
}

/// `plot` as a command frame's plot byte: any place but the one the frame
/// reserves for "no plot". Whether the map has that plot is the stage's to
/// refuse, as it refuses any client's.
fn plot_byte(plot: usize) -> Result<u8, String> {
    u8::try_from(plot)
        .ok()
        .filter(|&byte| byte != PLOT_NONE)
        .ok_or_else(|| format!("plot {plot} is past the plots a command frame can name"))
}

/// The run's numbers, read off the stage the world's [`readout`] holds —
/// empty for a world this game's module is not playing in.
fn status(world: &mut World) -> Vec<(&'static str, String)> {
    let Some(readout) = world.system_mut::<FieldReadout>() else {
        return Vec::new();
    };
    let stats = stats_of(&lock(&readout.shared));
    vec![
        ("Lives", format!("{}/{STARTING_LIVES}", stats.lives)),
        ("Gold", stats.gold.to_string()),
        ("Wave", format!("{}/{}", stats.wave, WAVES.len())),
        ("Outcome", stats.outcome.label().to_owned()),
    ]
}

/// Takes the stage's refusals not yet told, oldest first, each as the label a
/// player is shown. A tool's commands are the one local player's, so every
/// refusal is theirs, as solo's are.
fn refusals(world: &mut World) -> Vec<String> {
    let Some(readout) = world.system_mut::<FieldReadout>() else {
        return Vec::new();
    };
    std::mem::take(&mut lock(&readout.shared).refusals)
        .into_iter()
        .map(|(_, refusal)| refusal.label().to_owned())
        .collect()
}

/// The name [`readout`]'s system goes by in the world's schedule.
const READOUT: &str = "towers-readout";

/// The stage a played field runs, as a system of the world it plays in: what
/// [`status`] and [`refusals`] read. It holds no entities and does nothing on
/// a tick.
struct FieldReadout {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for FieldReadout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldReadout").finish_non_exhaustive()
    }
}

/// The system a played field registers so a tool's controls can read
/// `shared`.
pub(super) fn readout(shared: Arc<Mutex<Stage>>) -> Box<dyn SystemTrait> {
    Box::new(FieldReadout { shared })
}

impl SystemTrait for FieldReadout {
    fn name(&self) -> &str {
        READOUT
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        0
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::game::{DEFAULT_TICK_HZ, Game, Refusal};
    use crate::map::Map;

    /// What solo's client sends on its next tick for `controls`: the frame
    /// [`Game::tick`] seals out of what [`Game::set_controls`] recorded.
    fn client_frame(controls: Controls) -> Vec<u8> {
        let mut game = Game::new(DEFAULT_TICK_HZ, &Map::built_in()).expect("solo");
        game.set_controls(controls);
        game.pending.to_wire()
    }

    /// **Every action's bytes are the bytes solo's client sends for the same
    /// command**, each kind of tower included — and they decode back to that
    /// command on the server's side of the wire.
    #[test]
    fn every_action_encodes_what_the_client_sends() {
        let plot = 2;
        let mut cases = vec![
            (
                START_WAVE,
                vec![],
                Controls {
                    start_wave: true,
                    ..Controls::default()
                },
            ),
            (
                UPGRADE,
                vec![PlayArg::Picked(plot)],
                Controls {
                    upgrade: Some(2),
                    ..Controls::default()
                },
            ),
            (
                RESTART,
                vec![],
                Controls {
                    restart: true,
                    ..Controls::default()
                },
            ),
        ];
        for (index, kind) in tower::ALL.into_iter().enumerate() {
            cases.push((
                PLACE,
                vec![PlayArg::Picked(plot), PlayArg::Choice(index)],
                Controls {
                    place: Some(2),
                    kind,
                    ..Controls::default()
                },
            ));
        }
        for (action, args, controls) in cases {
            let encoded = encode(action, &args).expect("an action towers has");
            assert_eq!(
                encoded,
                client_frame(controls),
                "{} {args:?} is not the client's frame",
                ACTIONS[action].name,
            );
            // Field by field against what was asked, rather than through the
            // conversion both sides above share.
            let decoded = Intent::from_wire(&encoded).expect("a frame this build reads");
            assert_eq!(
                (
                    decoded.place,
                    decoded.kind,
                    decoded.upgrade,
                    decoded.start_wave,
                    decoded.restart
                ),
                (
                    controls.place,
                    controls.kind,
                    controls.upgrade,
                    controls.start_wave,
                    controls.restart
                ),
            );
        }
    }

    /// **Each action index names the action it is used for**, and a build's
    /// choices are the tower kinds in the table's order — so a reordered list
    /// is a red test rather than a tool whose `Restart` starts a wave.
    #[test]
    fn each_action_index_names_its_action() {
        assert_eq!(ACTIONS[PLACE].name, "Place tower");
        assert_eq!(ACTIONS[START_WAVE].name, "Start wave");
        assert_eq!(ACTIONS[UPGRADE].name, "Upgrade");
        assert_eq!(ACTIONS[RESTART].name, "Restart");
        assert_eq!(KIND_LABELS, tower::ALL.map(tower::Kind::label));
    }

    /// **A plot no frame can name and a kind past the table are refused**,
    /// not wrapped or clamped into some other command.
    #[test]
    fn what_a_frame_cannot_carry_is_refused() {
        let place = |plot, kind| encode(PLACE, &[PlayArg::Picked(plot), PlayArg::Choice(kind)]);
        assert!(place(usize::from(PLOT_NONE) - 1, 0).is_ok());
        assert!(place(usize::from(PLOT_NONE), 0).is_err(), "the sentinel");
        assert!(
            place(usize::from(u8::MAX) + 1, 0).is_err(),
            "a wrap to plot 0"
        );
        assert!(place(0, tower::KINDS).is_err());
        assert!(encode(ACTIONS.len(), &[]).is_err());
        assert!(encode(START_WAVE, &[PlayArg::Choice(0)]).is_err());
    }

    /// **The readout reads the stage it was handed, and nothing in a world
    /// without one**: the four numbers, and each refusal once.
    #[test]
    fn the_readout_reads_the_stage_and_takes_each_refusal_once() {
        let mut world = World::new();
        assert!(status(&mut world).is_empty());
        assert!(refusals(&mut world).is_empty());

        let shared = Arc::new(Mutex::new(Stage::new(Arc::new(Map::built_in()))));
        world.register_system(readout(Arc::clone(&shared)));
        {
            let mut stage = lock(&shared);
            stage.gold = 7;
            stage.lives = 3;
            stage.refuse(None, Refusal::PlotTaken);
        }
        assert_eq!(
            status(&mut world),
            [
                ("Lives", format!("3/{STARTING_LIVES}")),
                ("Gold", "7".to_owned()),
                ("Wave", format!("0/{}", WAVES.len())),
                ("Outcome", "playing".to_owned()),
            ],
        );
        assert_eq!(refusals(&mut world), [Refusal::PlotTaken.label()]);
        assert!(refusals(&mut world).is_empty(), "a refusal was told twice");
        assert_eq!(world.entity_count(), 0, "the readout spawned something");
    }
}
