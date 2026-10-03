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
//! names one; an upgrade picks the built tower itself, a [`Turret`] the
//! module mirrored, and reads the plot off its row — the client's command
//! names a tower by its plot too.
//!
//! # The readout is a system in the world
//!
//! The stage is behind the module's own lock, and
//! [`PlayControls::status`] is handed a world: so the module registers
//! [`readout`] in the world it plays in — a system holding the same stage,
//! with no entities — and the status and the refusals are read through it.

use std::sync::{Arc, Mutex};

use crcbl::ecs::{DebugCtx, Entity, System, SystemTrait, World};
use crcbl::registry::{ParamKind, PlayAction, PlayArg, PlayControls};

use super::{TURRETS, Turret};
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

/// The four commands this game's client sends, as a tool offers them. A
/// build picks a plot of the scene; an upgrade picks a tower the run built,
/// where it is drawn.
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
        params: &[ParamKind::PickedRuntime(TURRETS)],
    },
    PlayAction {
        name: "Restart",
        params: &[],
    },
];

/// The frame solo's client sends for the action at `action` taking `args`,
/// a picked tower's plot read off its [`Turret`] in `world`.
///
/// # Errors
///
/// A plot whose place no command frame can carry, a kind past the table, a
/// picked entity that is no built tower, or an action this game does not
/// have — none of which arrives through
/// [`crcbl::registry::Registry::encode_play`], which checks the arguments
/// against [`ACTIONS`] and the world first, and each refused here rather
/// than trusted.
fn encode(world: &mut World, action: usize, args: &[PlayArg]) -> Result<Vec<u8>, String> {
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
        (UPGRADE, &[PlayArg::PickedRuntime(tower)]) => Controls {
            upgrade: Some(plot_byte(plot_of(world, tower)?)?),
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

/// The plot the tower `turret` mirrors stands on, read off its row in
/// `world`.
fn plot_of(world: &mut World, turret: Entity) -> Result<usize, String> {
    world
        .system_mut::<System<Turret>>()
        .and_then(|turrets| turrets.get(turret))
        .map(|row| row.plot)
        .ok_or_else(|| format!("{turret:?} is not a tower on this field"))
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
/// refusal is theirs, as solo's are. Refusals [`bound_untold`] dropped since
/// the last call come first, as one line counting them: they were older.
fn refusals(world: &mut World) -> Vec<String> {
    let Some(readout) = world.system_mut::<FieldReadout>() else {
        return Vec::new();
    };
    let dropped = std::mem::take(&mut readout.dropped);
    let told = std::mem::take(&mut lock(&readout.shared).refusals)
        .into_iter()
        .map(|(_, refusal)| refusal.label().to_owned());
    (dropped > 0)
        .then(|| dropped_label(dropped))
        .into_iter()
        .chain(told)
        .collect()
}

/// How many refusals a played field keeps for a tool that has not taken
/// them. Far more than one tick of one player's commands can be refused, so
/// a tool taking them every frame — the editor — never loses one; a tool
/// ticking without taking them holds this many and no more.
const UNTOLD_KEPT: usize = 64;

/// Drops the oldest of the stage's untold refusals past [`UNTOLD_KEPT`],
/// counting them on the readout for [`refusals`] to tell. Nothing in a world
/// this game's module is not playing in.
///
/// The played field's own bound, called after every tick: solo, a host and a
/// dedicated server take the stage's refusals every tick themselves, and
/// tell each to whoever sent it, so their path keeps no cap. The untold
/// refusals are no part of [`Stage::hash_state`], so dropping one changes no
/// hash.
pub(super) fn bound_untold(world: &mut World) {
    let Some(readout) = world.system_mut::<FieldReadout>() else {
        return;
    };
    let excess = {
        let mut stage = lock(&readout.shared);
        let excess = stage.refusals.len().saturating_sub(UNTOLD_KEPT);
        stage.refusals.drain(..excess);
        excess
    };
    readout.dropped += excess;
}

/// The line [`refusals`] tells `dropped` refusals [`bound_untold`] dropped
/// by, in the labels' own capitals.
fn dropped_label(dropped: usize) -> String {
    format!("{dropped} OLDER REFUSALS WENT UNTOLD")
}

/// The name [`readout`]'s system goes by in the world's schedule.
const READOUT: &str = "towers-readout";

/// The stage a played field runs, as a system of the world it plays in: what
/// [`status`] and [`refusals`] read. It holds no entities and does nothing on
/// a tick.
struct FieldReadout {
    shared: Arc<Mutex<Stage>>,
    /// How many refusals [`bound_untold`] dropped since [`refusals`] last
    /// took them.
    dropped: usize,
}

impl std::fmt::Debug for FieldReadout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldReadout").finish_non_exhaustive()
    }
}

/// The system a played field registers so a tool's controls can read
/// `shared`.
pub(super) fn readout(shared: Arc<Mutex<Stage>>) -> Box<dyn SystemTrait> {
    Box::new(FieldReadout { shared, dropped: 0 })
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

    /// A world holding one mirrored tower, on `plot`, and that tower.
    fn with_a_turret(plot: usize) -> (World, Entity) {
        let mut world = World::new();
        world.register_system(Box::new(System::<Turret>::new(TURRETS)));
        let turret = world.spawn();
        let feet = Map::built_in().plots()[plot].at();
        world
            .system_mut::<System<Turret>>()
            .expect("registered")
            .attach(
                turret,
                Turret {
                    plot,
                    feet,
                    scale: 1.0,
                },
            );
        (world, turret)
    }

    /// **Every action's bytes are the bytes solo's client sends for the same
    /// command**, each kind of tower included — and they decode back to that
    /// command on the server's side of the wire.
    #[test]
    fn every_action_encodes_what_the_client_sends() {
        let plot = 2;
        let (mut world, turret) = with_a_turret(plot);
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
                vec![PlayArg::PickedRuntime(turret)],
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
            let encoded = encode(&mut world, action, &args).expect("an action towers has");
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

    /// **A plot no frame can name, a kind past the table and an upgrade of
    /// something that is no tower are refused**, not wrapped or clamped into
    /// some other command.
    #[test]
    fn what_a_frame_cannot_carry_is_refused() {
        let (mut world, turret) = with_a_turret(0);
        let mut place = |plot, kind| {
            encode(
                &mut world,
                PLACE,
                &[PlayArg::Picked(plot), PlayArg::Choice(kind)],
            )
        };
        assert!(place(usize::from(PLOT_NONE) - 1, 0).is_ok());
        assert!(place(usize::from(PLOT_NONE), 0).is_err(), "the sentinel");
        assert!(
            place(usize::from(u8::MAX) + 1, 0).is_err(),
            "a wrap to plot 0"
        );
        assert!(place(0, tower::KINDS).is_err());
        assert!(encode(&mut world, ACTIONS.len(), &[]).is_err());
        assert!(encode(&mut world, START_WAVE, &[PlayArg::Choice(0)]).is_err());
        assert!(encode(&mut world, UPGRADE, &[PlayArg::PickedRuntime(turret)]).is_ok());
        let stray = world.spawn();
        let refused = encode(&mut world, UPGRADE, &[PlayArg::PickedRuntime(stray)])
            .expect_err("an entity that is no tower");
        assert!(refused.contains("not a tower"), "{refused}");
        assert!(
            encode(
                &mut World::new(),
                UPGRADE,
                &[PlayArg::PickedRuntime(turret)]
            )
            .is_err(),
            "a world with no field played in it named a tower"
        );
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

    /// **A tool that never takes the refusals leaves [`UNTOLD_KEPT`] of them
    /// on the stage and no more**, the newest, every one still counted; the
    /// take that follows tells how many older ones were dropped, then the
    /// kept ones, and the count starts again.
    #[test]
    fn untold_refusals_stay_bounded_when_nobody_takes_them() {
        use crcbl::core::TickId;
        use crcbl::ecs::{ClientInputs, GameModule};

        let mut module = super::super::FieldPlay::new(Map::built_in());
        let mut world = World::new();
        module.register(&mut world);
        // Refused every tick each is sent: a build on a plot the field does
        // not have, first — the refusals that should be dropped — then an
        // upgrade on a plot with no tower.
        let frame = |controls: Controls| [(TickId::from_raw(0), Intent::from(controls).to_wire())];
        let past_the_plots = u8::try_from(Map::built_in().plots().len()).expect("a plot byte");
        let oldest = frame(Controls {
            place: Some(past_the_plots),
            ..Controls::default()
        });
        let newest = frame(Controls {
            upgrade: Some(0),
            ..Controls::default()
        });
        let dropped = 5;
        let sent = UNTOLD_KEPT + dropped;
        for index in 0..sent {
            let inputs = if index < dropped { &oldest } else { &newest };
            world.tick();
            module.tick(&mut world, ClientInputs::new(inputs, 0));
            world.sweep();
            let untold = lock(&module.towers.shared).refusals.len();
            assert!(untold <= UNTOLD_KEPT, "{untold} refusals left untold");
        }
        let stage_refused = lock(&module.towers.shared).refused;
        assert_eq!(stage_refused, u64::try_from(sent).expect("a count"));

        let told = refusals(&mut world);
        assert_eq!(told.len(), 1 + UNTOLD_KEPT, "{told:?}");
        assert_eq!(told[0], dropped_label(dropped));
        assert!(
            told[1..]
                .iter()
                .all(|label| label == Refusal::NoTower.label()),
            "{told:?}"
        );
        assert!(refusals(&mut world).is_empty(), "the drop was told twice");
    }
}
