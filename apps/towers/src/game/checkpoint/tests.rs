//! A stage saved between waves, written to a scratch directory, read back
//! and resumed — and the resumed run held to the original, tick for tick.

use std::hash::{DefaultHasher, Hasher as _};

use super::super::{DEFAULT_TICK_HZ, Intent, run_tick};
use super::*;
use crate::map::Map;
use crate::save::{PLAYER_FILE, Vault};
use crate::tower::Kind::{Bolt, Slow, Splash};

/// One tick at the default rate.
const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

/// The most ticks any loop here runs before it is a failure: several waves'
/// worth, far past what each needs.
const MAX_TICKS: u64 = 60 * DEFAULT_TICK_HZ as u64;

/// An empty field on the committed map.
fn new_stage() -> Stage {
    Stage::new(Arc::new(Map::built_in()))
}

/// The stage's state hash, as the server's world reads it.
fn hash(stage: &Stage) -> u64 {
    let mut hasher = DefaultHasher::new();
    stage.hash_state(&mut hasher);
    hasher.finish()
}

/// Which plot of the committed field is labelled `label`.
fn plot(label: &str) -> u8 {
    let at = Map::built_in()
        .plots()
        .iter()
        .position(|plot| plot.label == label)
        .unwrap_or_else(|| panic!("the map has no {label} plot"));
    u8::try_from(at).expect("a map's plots fit a byte")
}

/// The command that builds a `kind` tower on the plot labelled `label`.
fn build(label: &str, kind: crate::tower::Kind) -> Intent {
    Intent {
        place: Some(plot(label)),
        kind,
        ..Intent::default()
    }
}

/// What a player asks for on `tick` of the first run, counted from its first
/// tick: a slow tower where the creeps come in, holding the wave's last
/// creeps as the wave ends, and a splash tower at the bend — which is what
/// the opening purse buys.
fn opening(tick: u64) -> Intent {
    match tick {
        1 => build("entry", Slow),
        2 => build("bend", Splash),
        _ => Intent::default(),
    }
}

/// What a player asks for on `tick` after a resume, counted from it: a bolt
/// tower, the splash tower stepped up — each taken or refused by the purse,
/// and either is the same in both runs — and the next wave sent at once.
fn after_the_save(tick: u64) -> Intent {
    match tick {
        1 => build("middle", Bolt),
        2 => Intent {
            upgrade: Some(plot("bend")),
            ..Intent::default()
        },
        3 => Intent {
            start_wave: true,
            ..Intent::default()
        },
        _ => Intent::default(),
    }
}

/// Plays the opening to the first tick of a build phase whose save carries
/// everything a stage can have in it besides the counters: creeps walking,
/// one of them held, a bolt in the air and a tower still reloading. Each is
/// a value only a resume's own ticks can see lost, so a save that dropped
/// one has to be compared here to be caught.
fn played_to_a_full_build_phase() -> Stage {
    let mut stage = new_stage();
    for tick in 1..=MAX_TICKS {
        run_tick(&mut stage, opening(tick), DT);
        let reloading = stage
            .towers
            .iter()
            .any(|tower| tower.ready_at() > stage.elapsed);
        if stage.resting_after().is_some()
            && stage.creeps.iter().any(Creep::is_slowed)
            && !stage.bolts.is_empty()
            && reloading
        {
            return stage;
        }
    }
    panic!("no build phase in {MAX_TICKS} ticks had everything on the field at once");
}

/// `stage` saved to a scratch directory and resumed from it, on a stage of
/// its own — the whole path `--resume` takes, the container included.
fn saved_and_resumed(stage: &Stage) -> Stage {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    let checkpoint = stage.checkpoint().expect("the field is at rest");
    vault
        .store(&checkpoint)
        .expect("the scratch directory is writable");
    let read = vault
        .load(&Map::built_in())
        .expect("the save this test wrote")
        .expect("a save is there");
    let mut resumed = new_stage();
    resumed.restore(&read).expect("a save of this map");
    resumed
}

/// **A stage resumed between waves is the stage that saved, and stays it.**
/// Saved in a build phase with the last wave's creeps still walking — one
/// held, a bolt in the air, a tower reloading — written, read back and
/// restored onto a fresh stage, it hashes as the original; then both are
/// handed the same commands — a build, an upgrade and a wave sent at once —
/// and hash alike on every tick through that wave and the build phase after
/// it.
///
/// The second half is what the first cannot see: a value left out of the
/// state hash but read by the simulation — a tower's reload — hashes alike at
/// the resume and diverges the tick it decides a shot.
#[test]
fn a_stage_resumed_between_waves_is_the_same_run_tick_for_tick() {
    let mut original = played_to_a_full_build_phase();
    assert!(original.shots > 0, "nothing was fired before the save");
    let mut resumed = saved_and_resumed(&original);
    assert_eq!(
        hash(&resumed),
        hash(&original),
        "the resumed stage does not hash as the one that saved",
    );
    let saved_at = original.waves.started();

    // Through the next wave and the build phase after it: until the wave
    // after that starts.
    let mut through = false;
    for tick in 1..=MAX_TICKS {
        run_tick(&mut original, after_the_save(tick), DT);
        run_tick(&mut resumed, after_the_save(tick), DT);
        assert_eq!(
            hash(&resumed),
            hash(&original),
            "the runs diverged {tick} tick(s) after the resume, at wave {}",
            original.waves.started(),
        );
        if original.waves.started() > saved_at + 1 {
            through = true;
            break;
        }
    }
    assert!(
        through,
        "the wave after next never started, so the runs were not compared through the next"
    );
    // …and the wave was really fought: shots fired and creeps met.
    assert!(original.shots > 0 && original.kills + original.leaks > 0);
}

/// **A save is refused mid-wave**, named, for as long as the wave is
/// releasing — and taken the first tick it is not, with what it sent still
/// walking, which is saved with it. A finished run is refused as one.
#[test]
fn a_save_is_refused_while_a_wave_is_coming_in() {
    let mut stage = new_stage();
    assert!(
        stage.checkpoint().is_ok(),
        "the control: a fresh field is between waves"
    );
    run_tick(
        &mut stage,
        Intent {
            start_wave: true,
            ..Intent::default()
        },
        DT,
    );
    let mut releasing = 0;
    while stage.waves.is_releasing() {
        assert_eq!(
            stage.checkpoint(),
            Err(NotSaved::WaveReleasing),
            "a save was taken while the wave was coming in",
        );
        releasing += 1;
        assert!(releasing < MAX_TICKS, "the wave never finished releasing");
        run_tick(&mut stage, Intent::default(), DT);
    }
    assert!(releasing > 1, "the wave released in a single tick");
    let checkpoint = stage
        .checkpoint()
        .expect("the build phase after the wave saves");
    assert!(
        !checkpoint.creeps.is_empty(),
        "nothing the wave sent was on the field, so this checked no creep"
    );
    assert_eq!(checkpoint.creeps.len(), stage.creeps.len());

    stage.outcome = crate::wave::Outcome::Lost;
    assert_eq!(stage.checkpoint(), Err(NotSaved::RunOver));
}

/// **A save of another map is not restored**, whatever it holds.
#[test]
fn a_checkpoint_of_another_map_is_not_restored() {
    let mut checkpoint = new_stage().checkpoint().expect("a fresh field saves");
    checkpoint.map[0] ^= 0xFF;
    let mut stage = new_stage();
    assert!(matches!(
        stage.restore(&checkpoint),
        Err(SaveError::OtherMap)
    ));
}

/// **The autosave is due once at each wave's end**: never before the first
/// wave, once on the first tick of the build phase after it, and not again
/// for the same wave — and a restored stage's own wave is the save it came
/// from, so it is not written again.
#[test]
fn the_autosave_is_due_once_at_each_waves_end() {
    let mut stage = new_stage();
    let mut autosave = Autosave::default();
    let mut saves = Vec::new();
    for tick in 1..=MAX_TICKS {
        run_tick(&mut stage, opening(tick), DT);
        if let Some(checkpoint) = autosave.due(&stage) {
            assert!(
                stage.checkpoint().is_ok() && stage.waves.started() > 0,
                "an autosave at tick {tick} with the field not at rest after a wave",
            );
            saves.push((checkpoint.wave, stage.ticks));
        }
        if saves.len() == 2 {
            break;
        }
    }
    assert_eq!(saves.len(), 2, "two waves' ends were not autosaved");
    assert!(
        saves[0].0 < saves[1].0,
        "one wave was autosaved twice: {saves:?}",
    );

    let checkpoint = stage.checkpoint().expect("at rest after a wave");
    let mut resumed = new_stage();
    resumed.restore(&checkpoint).expect("a save of this map");
    let mut after = Autosave::default();
    after.restored(&checkpoint);
    assert!(
        after.due(&resumed).is_none(),
        "the wave a stage was restored to was autosaved again",
    );
}

/// Four creeps of one hit point each on a fresh field, at `along` metres —
/// first put down at `from`, if given, and the physics world's tree built
/// over them there before they walk on to `along`. A world refits its tree
/// as spheres move and rebuilds it only when one is added or taken away, so
/// that is the world of a run that has played since its last spawn, where a
/// resume builds its tree over the creeps where they stand.
fn field_walked(from: Option<[f64; 4]>, along: [f64; 4]) -> Stage {
    let mut stage = new_stage();
    let kind = crate::creep::Kind::Swarm;
    for (index, &metres) in from.unwrap_or(along).iter().enumerate() {
        let creep = Creep::restored(&mut stage.world, stage.map.path(), kind, metres, 1, 1.0);
        stage.creeps.push(creep);
        assert_eq!(stage.creeps[index].along(), metres);
    }
    // The query is what builds the tree over the creeps where they are now.
    let _ = stage.world.overlap_sphere(stage.creeps[0].centre(), 1.0);
    let speed = kind.spec().speed;
    for (creep, &metres) in stage.creeps.iter_mut().zip(&along) {
        let dt = (metres - creep.along()) / speed;
        creep.advance(&mut stage.world, stage.map.path(), dt);
    }
    stage
}

/// **A burst wounds in the field's order, whatever the physics world's
/// history.** Two stages with the same creeps in the same list — one on a
/// fresh world, one on a world whose freed slots hand the spheres out in
/// another order, as the world of a run that has played does against the
/// fresh one a resume builds — take the same burst, which kills two of
/// four. A kill swap-removes its creep, so the order the two die in is the
/// order the list is left in, and the two stages must be left alike.
#[test]
fn a_burst_wounds_in_the_fields_order_whatever_the_worlds_history() {
    // The near two swap places on the lane between the tree being built and
    // the burst; the far two are out of its reach and only fill the list.
    let along = [2.6, 2.0, 30.0, 31.0];
    let mut resumed = field_walked(None, along);
    let mut played = field_walked(Some([1.0, 3.6, 30.0, 31.0]), along);
    assert_eq!(
        hash(&resumed),
        hash(&played),
        "the two fields differ to begin with"
    );
    let spec = Splash.spec(crate::tower::Tier::Upgraded);
    for stage in [&mut resumed, &mut played] {
        let at = stage.creeps[0].centre();
        let bolt = crate::tower::Bolt::restored(
            0,
            at,
            crcbl::math::DVec3::NEG_Y,
            stage.exit,
            spec.damage,
            spec.burst_m,
        );
        stage.splash(&bolt, None);
        assert_eq!(stage.creeps.len(), 2, "the burst did not kill the near two");
    }
    assert_eq!(
        hash(&played),
        hash(&resumed),
        "the burst left the list in the physics world's order"
    );
}

/// Two creeps level on the lane, on a fresh field — after `history` creeps
/// were spawned and taken away first, so the world hands the two the slots
/// that history freed, last freed first.
fn level_pair(history: usize) -> Stage {
    let mut stage = new_stage();
    let kind = crate::creep::Kind::Fast;
    let ghosts: Vec<Creep> = (0..history)
        .map(|_| Creep::spawn(&mut stage.world, stage.map.path(), kind))
        .collect();
    for ghost in ghosts {
        ghost.despawn(&mut stage.world);
    }
    for _ in 0..2 {
        let creep = Creep::restored(&mut stage.world, stage.map.path(), kind, 4.0, 1, 1.0);
        stage.creeps.push(creep);
    }
    stage
}

/// **A tower picks between creeps level on the lane in the field's order.**
/// Nearest the exit is the rule, and two creeps the same distance in are
/// the same distance from it; the one picked is the one earlier in the
/// list, on a fresh world and on one whose freed slots put the two in the
/// other order — which is the order an overlap answers in when two spheres
/// stand in one place.
#[test]
fn a_tower_picks_between_creeps_level_on_the_lane_in_the_fields_order() {
    for history in [0, 2] {
        let mut stage = level_pair(history);
        let muzzle = stage.creeps[0].centre();
        let Stage {
            world,
            creeps,
            scratch,
            ..
        } = &mut stage;
        assert_eq!(
            crate::tower::acquire(world, creeps, muzzle, 1.0, scratch),
            Some(0),
            "after {history} creeps came and went, the tower picked another of the two",
        );
    }
}
