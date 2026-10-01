//! Play mode: the ticks, the pause, the restore, and every edit refused in
//! between — and the test-only module and vocabulary the editor's other tests
//! play with.
//!
//! **The module is a test fixture, not a shipped one.** The editor's own
//! [`Block`](crate::scene::Block) has no module, and the one the shipped
//! vocabulary carries is towers', which plays only towers' field — its tests
//! are `towers_play_tests`. A module here that moves every block is what lets
//! a test see a tick land on the compiled-in scene.

use std::time::Duration;

use super::*;

use crcbl::ecs::{ClientInputs, DebugCtx, GameModule, System, SystemTrait};

use crate::scene::{BLOCKS, Block, GREYBOX};

/// How far [`Drift`] moves every block along X each tick, in metres.
pub(crate) const DRIFT_M: f64 = 0.5;

/// The tick period [`Drift`] sets on the world it is registered on: a quarter
/// second, so a test's frame times add up to whole ticks exactly, and not the
/// world's default — which is what shows play stepping at the world's rate
/// rather than at a rate of its own.
pub(crate) const TICK: Duration = Duration::from_millis(250);

/// A test module: sets the world's tick period to [`TICK`] and moves every
/// block [`DRIFT_M`] along X each tick.
struct Drift;

impl GameModule for Drift {
    fn name(&self) -> &str {
        "drift"
    }

    fn register(&self, world: &mut World) {
        world.set_tick_dt(TICK.as_secs_f64());
    }

    fn tick(&mut self, world: &mut World, _inputs: ClientInputs<'_>) {
        if let Some(blocks) = world.system_mut::<System<Block>>() {
            for block in blocks.iter_mut() {
                block.position[0] += DRIFT_M;
            }
        }
    }
}

/// The editor's vocabulary with [`Drift`] registered for its blocks.
pub(crate) fn drifting() -> Registry {
    let mut registry = crate::scene::vocabulary();
    registry.module(BLOCKS, |_, _| Ok(Box::new(Drift)));
    registry
}

/// The compiled-in greybox scene, opened with [`drifting`]'s vocabulary.
pub(crate) fn drifting_document() -> Document {
    Document::open(
        &crate::scene::built_in_source(),
        Path::new(GREYBOX),
        drifting(),
    )
    .expect("the compiled-in scene is a scene")
}

/// The centre of `id`'s bounds along X.
fn centre_x(document: &mut Document, id: SceneEntityId) -> f64 {
    let (min, max) = document.bounds(id).expect("a block in this document");
    f64::from((min.x + max.x) * 0.5)
}

/// The first step, which stands clear of its neighbours on a 3 m pitch.
const STEP: SceneEntityId = SceneEntityId(1);

/// **Play ticks the registered module, and the change is what the picture
/// and a click read**: the bounds the renderer draws from, and the collider a
/// ray picks.
#[test]
fn play_ticks_the_registered_module_and_the_bounds_follow() {
    let mut document = drifting_document();
    let was = centre_x(&mut document, STEP);
    let y = {
        let (min, max) = document.bounds(STEP).expect("the first step");
        f64::from((min.y + max.y) * 0.5)
    };
    let ray = |x: f64| Ray::new(DVec3::new(x, y, 20.0), DVec3::NEG_Z);

    document.play().expect("the scene saves, so it plays");
    assert_eq!(document.play_state(), PlayState::Playing);
    assert_eq!(document.playing_modules(), ["drift"]);
    assert_eq!(document.advance(TICK * 2), 2);

    let now = centre_x(&mut document, STEP);
    assert!(
        (now - (was + 2.0 * DRIFT_M)).abs() < 1e-5,
        "two ticks moved the step from {was} to {now}",
    );
    // Just inside the step's new right edge: outside the box it stood in, and
    // inside where its neighbour stood before that one drifted too — so a
    // collider left behind picks the neighbour here.
    let (_, max) = document.bounds(STEP).expect("the first step");
    assert_eq!(
        document.pick(&ray(f64::from(max.x) - 0.2)),
        Some(STEP),
        "the collider did not follow what the module moved",
    );
}

/// **Pause stops the ticks, and the paused time is not owed afterwards**: a
/// resumed scene ticks for the time it is handed from then on, not for the
/// second it spent paused.
#[test]
fn pause_stops_the_ticks_and_play_resumes_them() {
    let mut document = drifting_document();
    document.play().expect("plays");
    assert_eq!(document.advance(TICK), 1);
    let held = centre_x(&mut document, STEP);

    assert!(document.pause());
    assert_eq!(document.play_state(), PlayState::Paused);
    assert!(!document.pause(), "a paused scene paused again");
    assert_eq!(document.advance(Duration::from_secs(1)), 0);
    assert_eq!(centre_x(&mut document, STEP), held, "a paused scene moved");

    document.play().expect("resumes");
    assert_eq!(document.play_state(), PlayState::Playing);
    assert_eq!(
        document.advance(Duration::ZERO),
        0,
        "the time spent paused was banked as ticks",
    );
    assert_eq!(document.advance(TICK), 1);
}

/// **Stop puts the scene back byte for byte** — after play changed it, so
/// the comparison is not of a scene that never moved.
#[test]
fn stop_restores_the_exact_pre_play_scene() {
    let mut document = drifting_document();
    let before = document.files().expect("every block has an id");

    document.play().expect("plays");
    document.advance(TICK * 3);
    assert_ne!(
        document.files().expect("ids"),
        before,
        "play changed nothing, so the restore below proves nothing",
    );

    assert!(document.stop().expect("the snapshot loads again"));
    assert_eq!(document.play_state(), PlayState::Editing);
    assert_eq!(document.files().expect("ids"), before);
    assert!(document.playing_modules().is_empty());
    assert!(!document.stop().expect("nothing to stop"), "stopped twice");
}

/// **An edit made before play survives play and stop, still unsaved, and an
/// undo after stop still reverts it** — the log addresses the restored
/// entities because they came back under the ids it names. And the id map's
/// high-water mark comes back too: a duplicate after stop does not hand out
/// the id of a copy the log deleted before play.
#[test]
fn edits_before_play_survive_it_and_undo_still_reverts_them() {
    let mut document = drifting_document();
    let original = document.files().expect("ids");

    document
        .apply(EditCommand::SetProperty {
            entity: STEP,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.1".to_owned(),
            value: Value::Float(2.5),
        })
        .expect("a block has a y");
    let copy = document.duplicate(STEP).expect("in the scene");
    document.delete(copy).expect("the copy is in the scene");
    document.select(Some(STEP));
    let edited = document.files().expect("ids");
    assert!(document.is_dirty());

    document.play().expect("plays");
    document.advance(TICK * 4);
    document.stop().expect("restores");

    assert_eq!(document.files().expect("ids"), edited, "the edit was lost");
    assert!(document.is_dirty(), "the dirty marker forgot the edit");
    assert_eq!(document.selected(), Some(STEP), "the selection was dropped");
    assert_eq!(document.log().position(), 3, "the log lost entries");

    let next = document.duplicate(STEP).expect("in the scene");
    assert_ne!(
        next, copy,
        "a deleted copy's id was handed out again after the restore",
    );
    assert!(document.undo().expect("the duplicate"));

    while document.undo().expect("every entry names a live entity") {}
    assert_eq!(
        document.files().expect("ids"),
        original,
        "walking the log back after stop did not reach the scene as opened",
    );
    assert!(!document.is_dirty());
}

/// **Every edit is refused in play mode**, playing or paused — the scene, the
/// log and the dirty marker all as they were — and a panel's own write is
/// taken back by the refusal rather than left standing.
#[test]
fn every_edit_is_refused_in_play_mode() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut document = drifting_document();
    document
        .apply(EditCommand::SetProperty {
            entity: STEP,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.2".to_owned(),
            value: Value::Float(1.0),
        })
        .expect("a block has a z");
    document.undo().expect("one entry");
    let clipping = document.copy(STEP).expect("in the scene");

    document.play().expect("plays");
    for paused in [false, true] {
        if paused {
            assert!(document.pause());
        }
        let played = document.files().expect("ids");
        let refusals: Vec<(&str, Result<(), EditError>)> = vec![
            (
                "apply",
                document.apply(EditCommand::SetProperty {
                    entity: STEP,
                    system: crate::scene::BLOCKS.to_owned(),
                    path: "position.0".to_owned(),
                    value: Value::Float(9.0),
                }),
            ),
            ("apply_in", {
                let gesture = document.begin_gesture();
                document.apply_in(
                    EditCommand::SetProperty {
                        entity: STEP,
                        system: crate::scene::BLOCKS.to_owned(),
                        path: "position.0".to_owned(),
                        value: Value::Float(9.0),
                    },
                    gesture,
                )
            }),
            ("record_edit", {
                // What a panel does: write the field, then report it.
                let before = document
                    .read(STEP, crate::scene::BLOCKS, "position.0")
                    .expect("an x");
                let component = document
                    .component(STEP, crate::scene::BLOCKS)
                    .expect("a block");
                set_path(component, "position.0", &Value::Float(9.0)).expect("an x");
                document.record_edit(
                    STEP,
                    crate::scene::BLOCKS,
                    "position.0",
                    &before,
                    &Value::Float(9.0),
                    None,
                )
            }),
            ("delete", document.delete(STEP)),
            ("duplicate", document.duplicate(STEP).map(drop)),
            ("paste", document.paste(&clipping).map(drop)),
            // Refused for play mode before the text is read, not for the text.
            ("paste of no clipping", document.paste("hello").map(drop)),
            ("undo", document.undo().map(drop)),
            ("redo", document.redo().map(drop)),
            ("save_to", document.save_to(dir.path())),
            ("save", document.save()),
        ];
        for (path, outcome) in refusals {
            assert!(
                matches!(outcome, Err(EditError::Playing)),
                "{path} while {} was not refused for play mode: {outcome:?}",
                if paused { "paused" } else { "playing" },
            );
        }
        assert_eq!(
            document.files().expect("ids"),
            played,
            "a refused edit changed the scene"
        );
        assert_eq!(document.log().position(), 0, "a refused edit moved the log");
        assert_eq!(document.log().len(), 1, "a refused edit was recorded");
        assert!(!document.is_dirty());
    }
    assert!(
        std::fs::read_dir(dir.path())
            .expect("the directory is there")
            .next()
            .is_none(),
        "a refused save wrote a file",
    );
    assert!(
        EditError::Playing.to_string().contains("play mode"),
        "the refusal does not name play mode",
    );
}

/// **The accumulator runs whole periods of the frame time it is handed** at
/// the world's tick rate — [`TICK`], which the module set — carrying the
/// remainder from one frame to the next, and dropping what passes the
/// clock's catch-up cap rather than owing it.
#[test]
fn the_accumulator_ticks_whole_periods_of_the_frame_time() {
    let mut document = drifting_document();
    document.play().expect("plays");
    let frames = [100, 100, 100, 300, 1000].map(Duration::from_millis);
    let ticks = frames.map(|dt| document.advance(dt));
    // 0.1, 0.2, 0.3, 0.6 and 1.6 s in all, at four ticks a second.
    assert_eq!(ticks, [0, 0, 1, 1, 4]);

    let stalled = document.advance(Duration::from_secs(10));
    assert_eq!(
        stalled,
        crcbl::core::time::DEFAULT_MAX_CATCH_UP_TICKS,
        "a stalled frame came back as a burst of every tick it missed",
    );
}

/// A system that counts the ticks the world's schedule runs.
#[derive(Debug, Default)]
struct Ticks(u32);

impl SystemTrait for Ticks {
    fn name(&self) -> &str {
        "ticks"
    }
    fn tick(&mut self, _dt: f64) {
        self.0 += 1;
    }
    fn entity_count(&self) -> usize {
        0
    }
    fn sweep(&mut self, _dead: &[Entity]) {}
    fn debug_draw(&mut self, _ctx: &DebugCtx) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A vocabulary with no modules plays**, and the world's own schedule
/// still ticks, at the world's default rate.
#[test]
fn a_vocabulary_with_no_modules_plays_and_the_world_still_ticks() {
    let mut document = Document::built_in().expect("the compiled-in scene is a scene");
    document.world.register_system(Box::new(Ticks::default()));
    let before = document.files().expect("ids");

    document.play().expect("a scene no game plays still plays");
    assert!(document.playing_modules().is_empty());
    let period = Duration::from_secs_f64(World::DEFAULT_TICK_DT);
    assert_eq!(document.advance(period * 3), 3);
    assert_eq!(
        document.world.system_mut::<Ticks>().map(|ticks| ticks.0),
        Some(3),
        "the world's schedule did not run",
    );

    document.stop().expect("restores");
    assert_eq!(document.files().expect("ids"), before);
}

/// A module whose `register` leaves the world a tick period play cannot step.
struct Stopped;

impl GameModule for Stopped {
    fn name(&self) -> &str {
        "stopped"
    }
    fn register(&self, world: &mut World) {
        world.set_tick_dt(0.0);
    }
}

/// **A module that leaves the world no tick period is refused, and the
/// document is back to editing exactly as it was** — not left holding the
/// module's systems or its rate.
#[test]
fn a_world_with_no_tick_period_does_not_play() {
    let mut registry = crate::scene::vocabulary();
    registry.module(BLOCKS, |_, _| Ok(Box::new(Stopped)));
    let mut document = Document::open(
        &crate::scene::built_in_source(),
        Path::new(GREYBOX),
        registry,
    )
    .expect("the compiled-in scene is a scene");
    let before = document.files().expect("ids");

    let error = document
        .play()
        .expect_err("a zero period cannot be stepped");
    assert!(
        matches!(error, EditError::TickRate(dt) if dt == 0.0),
        "{error}"
    );
    assert_eq!(document.play_state(), PlayState::Editing);
    assert_eq!(document.world.tick_dt(), World::DEFAULT_TICK_DT);
    assert_eq!(document.files().expect("ids"), before);
}
