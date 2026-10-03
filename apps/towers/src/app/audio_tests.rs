//! The field's sounds and its health bars through the running game, headless:
//! what a played wave is heard as, that hearing it leaves the stage alone,
//! that a headless run hears nothing — and the bars over the creeps.

use crcbl::core::input::KeyCode;
use crcbl::engine::{ExitReason, HostedGame};
use crcbl::shell::HeadlessShell;

use super::tests::{frames, headless, scripted, tap};
use super::{CAMERA_KEY, KIND_KEYS, Loop};
use crate::audio::Audio;
use crate::cue::Sound;
use crate::dev_camera::Mode;
use crate::tower;

/// How many frames a wave is given to be released and walked or killed.
const A_WAVE: usize = 600;

/// The frames [`play_two_waves`] runs, with room for its taps.
const TWO_WAVES: u64 = 2 * A_WAVE as u64 + 200;

/// Builds a slow tower on the entry plot and a splash tower on the bend,
/// asks for the bend again — refused — sends the first wave, then builds a
/// bolt tower on the east plot out of its bounties and sends the second:
/// every sound the field makes but the run's end, from keys, as a player
/// plays. The bolt tower stands on the second leg, so it shoots only what
/// gets past the other two.
fn play_two_waves(engine: &mut Loop<HeadlessShell>) {
    frames(engine, 4);
    tap(engine, KIND_KEYS[tower::Kind::Slow.index()]);
    tap(engine, KeyCode::KeyB);
    tap(engine, KeyCode::ArrowRight);
    tap(engine, KIND_KEYS[tower::Kind::Splash.index()]);
    tap(engine, KeyCode::KeyB);
    tap(engine, KeyCode::KeyB);
    tap(engine, KeyCode::KeyN);
    frames(engine, A_WAVE);
    tap(engine, KeyCode::ArrowRight);
    tap(engine, KIND_KEYS[tower::Kind::Bolt.index()]);
    tap(engine, KeyCode::KeyB);
    tap(engine, KeyCode::KeyN);
    frames(engine, A_WAVE);
}

/// A headless loop with an [`Audio`] that opens no device — what a windowed
/// run has, with the mixer left for the test to read.
fn hearing(frames: u64) -> Loop<HeadlessShell> {
    let mut engine = scripted(&headless(frames));
    engine.game_mut().audio = Some(Audio::offline());
    engine
}

/// **A headless run opens no device and hears nothing** — not a cue, though
/// the run builds, is refused, and kills — and a volume typed at its console
/// is refused as one with no mixer.
#[test]
fn a_headless_run_hears_nothing() {
    let mut engine = scripted(&headless(TWO_WAVES));
    play_two_waves(&mut engine);
    let stats = engine.game().game().stats();
    assert!(
        stats.kills > 0 && stats.refused > 0 && stats.built > 0,
        "the run did nothing to hear: {stats:?}"
    );
    assert!(engine.game().audio().is_none(), "a headless run has audio");
    assert!(
        engine
            .game_mut()
            .set_bus_gain(crcbl::audio::mixer::Bus::Sfx, 0.5)
            .is_err(),
        "a headless run took a volume it has no mixer for"
    );
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Every event of two played waves is heard, each once**: as many builds,
/// waves, kills, leaks and refusals as the stage counted, a shot heard for
/// every bolt but those fired on the tick their tower went up, and the slow
/// tower's hold, the splash bursts and the hits all heard.
#[test]
fn every_event_of_a_played_wave_is_heard_once() {
    let mut engine = hearing(TWO_WAVES);
    play_two_waves(&mut engine);
    let stats = engine.game().game().stats();
    let audio = engine.game().audio().expect("the test gave it audio");
    let plays = |sound: Sound| audio.plays(sound);

    assert_eq!(plays(Sound::Build), stats.built);
    assert_eq!(plays(Sound::Upgrade), stats.upgrades);
    assert_eq!(plays(Sound::Wave), stats.wave as u64);
    assert_eq!(plays(Sound::Kill), stats.kills);
    assert_eq!(plays(Sound::Leak), stats.leaks);
    assert_eq!(plays(Sound::Refused), stats.refused);
    assert!(
        stats.kills > 0 && stats.refused > 0 && stats.wave >= 2,
        "the run did not play what this test hears: {stats:?}"
    );

    let shots = plays(Sound::Fire(tower::Kind::Bolt)) + plays(Sound::Fire(tower::Kind::Splash));
    assert!(
        shots <= stats.shots && shots + stats.towers as u64 >= stats.shots,
        "{shots} shots heard of {} fired by {} towers",
        stats.shots,
        stats.towers
    );
    for sound in [
        Sound::Fire(tower::Kind::Bolt),
        Sound::Fire(tower::Kind::Splash),
        Sound::Fire(tower::Kind::Slow),
        Sound::Burst,
        Sound::Hit,
    ] {
        assert!(plays(sound) > 0, "no {} was heard", sound.label());
    }
    assert_eq!(
        (plays(Sound::Won), plays(Sound::Lost)),
        (0, 0),
        "a run still being played was heard ending"
    );
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The most frames [`a_field_left_open_is_heard_leaking_and_lost`] waits for
/// the run to be lost.
const TO_LOSE: usize = 6_000;

/// **A field left open is heard leaking, every life, and then lost** — once,
/// at the exit — with no tower on it to hear anything else.
#[test]
fn a_field_left_open_is_heard_leaking_and_lost() {
    let mut engine = hearing(TO_LOSE as u64 + 8);
    frames(&mut engine, 2);
    let mut lost = false;
    for _ in 0..TO_LOSE / 2 {
        // Every wave as soon as the table will take it; a refusal while one
        // is releasing is heard too, and counted below.
        tap(&mut engine, KeyCode::KeyN);
        if engine.game().game().stats().outcome == crate::wave::Outcome::Lost {
            lost = true;
            break;
        }
    }
    assert!(lost, "the open field was never overrun");
    let stats = engine.game().game().stats();
    let audio = engine.game().audio().expect("the test gave it audio");
    assert_eq!(audio.plays(Sound::Leak), stats.leaks);
    assert!(stats.leaks >= u64::from(crate::wave::STARTING_LIVES));
    assert_eq!(audio.plays(Sound::Lost), 1);
    assert_eq!(audio.plays(Sound::Refused), stats.refused);
    assert_eq!(
        audio
            .played()
            .iter()
            .filter(|cue| cue.sound == Sound::Leak)
            .map(|cue| cue.at)
            .collect::<Vec<_>>(),
        vec![engine.game().game().map().exit_centre(); stats.leaks as usize],
        "a leak was heard somewhere other than the exit"
    );
    for sound in [Sound::Kill, Sound::Hit, Sound::Build, Sound::Won] {
        assert_eq!(
            audio.plays(sound),
            0,
            "{} heard on an open field",
            sound.label()
        );
    }
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// After one frame: how many ticks the game has run, and the stage's
/// fingerprint — `Game::stage_fingerprint`.
type Seen = (u64, Option<(u64, usize)>);

/// [`play_two_waves`] with the stage's fingerprint taken after every frame
/// from the second wave's on, on a run with audio or without.
fn fingerprints(with_audio: bool) -> (Vec<Seen>, Loop<HeadlessShell>) {
    let mut engine = if with_audio {
        hearing(TWO_WAVES + A_WAVE as u64)
    } else {
        scripted(&headless(TWO_WAVES + A_WAVE as u64))
    };
    play_two_waves(&mut engine);
    let mut seen = Vec::with_capacity(A_WAVE);
    for _ in 0..A_WAVE {
        engine.frame().expect("a frame");
        seen.push((
            engine.game().game().ticks_run(),
            engine.game().game().stage_fingerprint(),
        ));
    }
    (seen, engine)
}

/// **Hearing the field leaves the stage exactly as it would have been**: two
/// played waves and the one after heard through a mixer hash as the same
/// waves played silent, frame for frame — and the heard run did hear them,
/// or the two agreeing would say nothing.
#[test]
fn hearing_the_field_leaves_the_stage_hash_alone() {
    let (silent, silent_engine) = fingerprints(false);
    let (heard, heard_engine) = fingerprints(true);
    let audio = heard_engine.game().audio().expect("the test gave it audio");
    assert!(
        audio.plays(Sound::Kill) > 0 && audio.plays(Sound::Burst) > 0,
        "the heard run heard no kill or burst"
    );
    assert_eq!(silent.len(), heard.len());
    for (frame, (a, b)) in silent.iter().zip(&heard).enumerate() {
        assert_eq!(a, b, "the stage diverged on frame {frame} of the last wave");
    }
    assert_eq!(
        silent_engine.game().game().stats(),
        heard_engine.game().game().stats()
    );
    silent_engine
        .finish(ExitReason::FrameBudget)
        .expect("teardown");
    heard_engine
        .finish(ExitReason::FrameBudget)
        .expect("teardown");
}

/// How many health bars the last frame drew: the rectangles in the bars'
/// own track colour.
fn bars_drawn(engine: &Loop<HeadlessShell>) -> usize {
    engine
        .gpu()
        .draw_list()
        .commands()
        .iter()
        .filter(|command| {
            matches!(
                command,
                crcbl::ui::draw_list::DrawCommand::Rect { color, .. }
                    if *color == crate::bars::TRACK
            )
        })
        .count()
}

/// **A bar over every creep on the field, and none in the walk**: as many
/// as the frame draws creeps once a wave is walking, none in the dev camera's
/// walk, and back with the overhead view.
#[test]
fn a_bar_is_drawn_over_every_creep_and_none_in_the_walk() {
    let mut engine = scripted(&headless(800));
    frames(&mut engine, 4);
    assert_eq!(bars_drawn(&engine), 0, "bars over an empty field");
    tap(&mut engine, KeyCode::KeyN);
    for _ in 0..A_WAVE {
        frames(&mut engine, 1);
        if engine.game().render_state.creeps_alive > 2 {
            break;
        }
    }
    let alive = engine.game().render_state.creeps_alive;
    assert!(alive > 2, "the wave never walked onto the field");
    assert_eq!(bars_drawn(&engine), alive);

    tap(&mut engine, CAMERA_KEY);
    assert_eq!(engine.game().dev_camera().mode(), Mode::Fly);
    assert!(bars_drawn(&engine) > 0, "the fly camera hides the bars");
    tap(&mut engine, CAMERA_KEY);
    assert_eq!(engine.game().dev_camera().mode(), Mode::Walk);
    assert_eq!(bars_drawn(&engine), 0, "bars drawn in the walk");
    tap(&mut engine, CAMERA_KEY);
    assert_eq!(engine.game().dev_camera().mode(), Mode::Overhead);
    assert_eq!(
        bars_drawn(&engine),
        engine.game().render_state.creeps_alive,
        "the bars did not come back with the overhead view"
    );
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}
