//! What a joiner draws and hears of the host's field: the bars over its
//! creeps, and the cues read off its snapshots — both from the replicated
//! field alone, since a joiner has no stage.

use std::collections::BTreeMap;

use super::*;
use crate::bars::{self, Bar};
use crate::cue::{Sound, Watcher};

/// The frame every bar here is laid out for: the default window.
const EXTENT: (u32, u32) = (960, 720);

/// The most steps the wave is given to be released and killed.
const A_WAVE: usize = 2_000;

/// How far apart two bars of one creep may be drawn, in pixels: a position
/// crosses the wire to a 2⁻⁹ m step, which the overhead camera makes a small
/// fraction of a pixel.
const BAR_SLACK_PX: f32 = 0.5;

/// The bars of `game`'s field, keyed by the tick it shows.
fn bars_at(game: &Game) -> (u64, Vec<Bar>) {
    (
        game.stats().ticks,
        bars::bars(&game.render_state(), &crate::camera::camera(), EXTENT),
    )
}

/// Whether `a` and `b` are one creep's bar to the wire's precision.
fn same_bar(a: &Bar, b: &Bar) -> bool {
    (a.at - b.at).abs().max_element() <= BAR_SLACK_PX
        && (a.fill - b.fill).abs() <= 1.0 / 255.0
        && a.slowed == b.slowed
}

/// **A joiner's bars are the host's, and it hears the host's wave.** A
/// splash tower and a slow tower, one built by each player, take a wave
/// apart; on every tick both drew, the joiner — from snapshots alone — drew
/// a bar per creep where the host did, filled and tinted as the host's
/// stage says. And the joiner's watcher, reading the same snapshots, heard
/// every build, wave and kill the host's stage counted, each once.
#[test]
fn a_joiners_bars_are_the_hosts_and_it_hears_the_hosts_wave() {
    let mut rig = Rig::playing(1);
    let mut watcher = Watcher::default();
    let mut heard: Vec<Sound> = Vec::new();
    let mut listen = |rig: &Rig<Game>| {
        let joiner = rig.joiners[0].game();
        heard.extend(
            watcher
                .hear(&joiner.replicated(), joiner.map(), TICK_HZ)
                .into_iter()
                .map(|cue| cue.sound),
        );
    };
    // The empty field, heard first, so the builds are heard being built.
    listen(&rig);
    rig.host.set_controls(build(0, tower::Kind::Splash));
    rig.joiners[0]
        .game_mut()
        .set_controls(build(1, tower::Kind::Slow));
    for _ in 0..MAX_FRAMES {
        rig.step();
        listen(&rig);
        if rig.joiners[0].game().stats().built == 2 {
            break;
        }
    }
    assert_eq!(
        rig.joiners[0].game().stats().built,
        2,
        "the towers never went up"
    );
    rig.host.set_controls(send_wave());

    let mut hosts: BTreeMap<u64, Vec<Bar>> = BTreeMap::new();
    let mut joiners: BTreeMap<u64, Vec<Bar>> = BTreeMap::new();
    let first = crate::wave::WAVES[0].creeps();
    for _ in 0..A_WAVE {
        rig.step();
        listen(&rig);
        let (tick, drawn) = bars_at(&rig.host);
        hosts.insert(tick, drawn);
        let joiner = rig.joiners[0].game();
        let (tick, drawn) = bars_at(joiner);
        joiners.insert(tick, drawn);
        let (host, seen) = (rig.host.stats(), joiner.stats());
        if host.kills + host.leaks >= u64::from(first) && seen.ticks == host.ticks {
            break;
        }
    }
    let host = rig.host.stats();
    assert!(
        host.kills + host.leaks >= u64::from(first),
        "the wave was not over in {A_WAVE} steps: {host:?}"
    );

    let (mut compared, mut wounded, mut held) = (0, 0, 0);
    for (tick, drawn) in &joiners {
        let Some(theirs) = hosts.get(tick) else {
            continue;
        };
        assert_eq!(drawn.len(), theirs.len(), "tick {tick}: bars came and went");
        for (a, b) in drawn.iter().zip(theirs) {
            assert!(
                same_bar(a, b),
                "tick {tick}: {a:?} on the joiner, {b:?} on the host"
            );
        }
        compared += 1;
        wounded += drawn.iter().filter(|bar| bar.fill < 1.0).count();
        held += drawn.iter().filter(|bar| bar.slowed).count();
    }
    assert!(
        compared > 100 && wounded > 0 && held > 0,
        "{compared} ticks compared, {wounded} wounded bars, {held} held"
    );

    let count = |sound: Sound| heard.iter().filter(|&&heard| heard == sound).count() as u64;
    assert_eq!(count(Sound::Build), host.built);
    assert_eq!(count(Sound::Wave), host.wave as u64);
    assert_eq!(count(Sound::Kill), host.kills);
    assert_eq!(count(Sound::Leak), host.leaks);
    assert!(count(Sound::Burst) > 0 && count(Sound::Hit) > 0);
}
