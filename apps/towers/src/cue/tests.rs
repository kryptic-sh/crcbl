use super::*;
use crate::creep::CreepView;
use crate::tower::{BurstView, TowerView};

/// The ticks the first snapshot of every test is at: well into a run.
const AT: u64 = 1_000;

fn map() -> Map {
    Map::built_in()
}

/// A field in play, at [`AT`], on its first run.
fn field() -> Decoded {
    let mut decoded = Decoded::default();
    decoded.stats.ticks = AT;
    decoded.stats.runs = 1;
    decoded
}

/// `decoded`, one tick on.
fn next(decoded: &Decoded) -> Decoded {
    let mut next = *decoded;
    next.stats.ticks += 1;
    next
}

/// A creep tagged `tag` at `centre`, whole.
fn creep(tag: u16, centre: DVec3) -> CreepView {
    CreepView {
        centre,
        tag,
        ..CreepView::default()
    }
}

/// Puts `creeps` on `decoded`'s field, in order.
fn with_creeps(decoded: &mut Decoded, creeps: &[CreepView]) {
    decoded.render.creeps[..creeps.len()].copy_from_slice(creeps);
    decoded.render.creeps_alive = creeps.len();
}

/// A watcher that has heard `first`, and what it hears of `second`.
fn heard(first: &Decoded, second: &Decoded) -> Vec<Cue> {
    let mut watcher = Watcher::default();
    assert!(
        watcher
            .hear(first, &map(), crate::game::DEFAULT_TICK_HZ)
            .is_empty(),
        "the first snapshot is a baseline"
    );
    watcher.hear(second, &map(), crate::game::DEFAULT_TICK_HZ)
}

fn muzzle_of(plot: usize) -> DVec3 {
    muzzle(&map(), plot).expect("a plot of the committed field")
}

fn tower(kind: tower::Kind, tier: Tier, working: bool) -> Option<TowerView> {
    Some(TowerView {
        kind,
        tier,
        working,
    })
}

/// **Every index is its own, and in [`Sound::ALL`]'s order** — what the
/// audio bank and its counters are keyed by.
#[test]
fn every_sound_has_its_own_index_in_order() {
    for (at, sound) in Sound::ALL.into_iter().enumerate() {
        assert_eq!(sound.index(), at, "{}", sound.label());
    }
}

/// **A tower built, stepped up or set working is heard once, at its
/// muzzle** — each kind's shot as that kind's — and a tower that stays
/// working or idle, or stays built, is not heard again.
#[test]
fn a_tower_is_heard_built_upgraded_and_firing_each_once_at_its_plot() {
    let empty = field();
    let mut built = next(&empty);
    built.render.towers[2] = tower(tower::Kind::Bolt, Tier::Base, false);
    assert_eq!(
        heard(&empty, &built),
        [Cue {
            sound: Sound::Build,
            at: muzzle_of(2)
        }]
    );

    let mut upgraded = next(&built);
    upgraded.render.towers[2] = tower(tower::Kind::Bolt, Tier::Upgraded, false);
    assert_eq!(
        heard(&built, &upgraded),
        [Cue {
            sound: Sound::Upgrade,
            at: muzzle_of(2)
        }]
    );

    for kind in tower::ALL {
        let mut idle = field();
        idle.render.towers[4] = tower(kind, Tier::Base, false);
        let mut working = next(&idle);
        working.render.towers[4] = tower(kind, Tier::Base, true);
        assert_eq!(
            heard(&idle, &working),
            [Cue {
                sound: Sound::Fire(kind),
                at: muzzle_of(4)
            }],
            "{}",
            kind.label()
        );
        assert!(
            heard(&working, &next(&working)).is_empty(),
            "a {} tower still working was heard again",
            kind.label()
        );
        assert!(heard(&working, &next(&idle)).is_empty(), "nor going idle");
    }
    assert!(heard(&built, &next(&built)).is_empty());
}

/// **A creep whose health fell is heard hit, where it is now** — matched by
/// its tag, so a creep that moved into a killed creep's place in the list is
/// not heard as that creep wounded.
#[test]
fn a_creep_is_heard_hit_by_its_tag_not_its_place() {
    let (a, b) = (DVec3::new(-4.0, 0.45, 8.0), DVec3::new(-9.0, 0.45, 8.0));
    let moved = DVec3::new(-8.5, 0.45, 8.0);
    let mut before = field();
    with_creeps(&mut before, &[creep(1, a), creep(2, b)]);

    let mut hit = next(&before);
    with_creeps(
        &mut hit,
        &[
            creep(1, a),
            CreepView {
                health: 0.75,
                ..creep(2, moved)
            },
        ],
    );
    assert_eq!(
        heard(&before, &hit),
        [Cue {
            sound: Sound::Hit,
            at: moved
        }]
    );

    // The whole first creep killed, and the wounded second swap-removed into
    // its place: by place, the first slot went from whole to a quarter, which
    // a match by place would hear as a hit.
    let mut wounded = before;
    with_creeps(
        &mut wounded,
        &[
            creep(1, a),
            CreepView {
                health: 0.25,
                ..creep(2, b)
            },
        ],
    );
    let mut killed = next(&wounded);
    with_creeps(
        &mut killed,
        &[CreepView {
            health: 0.25,
            ..creep(2, moved)
        }],
    );
    killed.stats.kills = wounded.stats.kills + 1;
    assert_eq!(
        heard(&wounded, &killed),
        [Cue {
            sound: Sound::Kill,
            at: a
        }]
    );
}

/// **A creep gone is heard as the counters say it went**: every leak at the
/// exit, every kill where its creep was last seen — the leaked ones being
/// those nearest the exit — and nothing for a creep gone with no counter
/// moving.
#[test]
fn a_creep_gone_is_heard_killed_or_leaking_as_the_counters_say() {
    let map = map();
    let exit = map.exit_centre();
    let at_the_gate = exit + DVec3::new(0.5, -0.55, 0.0);
    let far = DVec3::new(-13.0, 0.45, 8.0);
    let mut before = field();
    with_creeps(&mut before, &[creep(7, far), creep(8, at_the_gate)]);

    let mut after = next(&before);
    with_creeps(&mut after, &[]);
    after.stats.kills = 1;
    after.stats.leaks = 1;
    assert_eq!(
        heard(&before, &after),
        [
            Cue {
                sound: Sound::Leak,
                at: exit
            },
            Cue {
                sound: Sound::Kill,
                at: far
            },
        ]
    );

    let mut vanished = next(&before);
    with_creeps(&mut vanished, &[creep(7, far)]);
    assert!(
        heard(&before, &vanished).is_empty(),
        "a creep went with no counter saying how, and was heard"
    );
}

/// **A burst is heard once, where it landed**, and not again for as long as
/// it is drawn.
#[test]
fn a_burst_is_heard_once_where_it_landed() {
    let before = field();
    let centre = DVec3::new(2.0, 0.5, -6.0);
    let mut burst = next(&before);
    burst.render.bursts[0] = BurstView {
        centre,
        radius_m: 2.5,
        tag: 41,
    };
    burst.render.bursts_live = 1;
    assert_eq!(
        heard(&before, &burst),
        [Cue {
            sound: Sound::Burst,
            at: centre
        }]
    );
    assert!(heard(&burst, &next(&burst)).is_empty());
}

/// **A wave starting is heard at the spawn, once a wave**, and the run's
/// end once — a win in the middle of the field, a loss at the exit.
#[test]
fn a_wave_and_the_runs_end_are_heard_once_each() {
    let map = map();
    let before = field();
    let mut wave = next(&before);
    wave.stats.wave = 1;
    assert_eq!(
        heard(&before, &wave),
        [Cue {
            sound: Sound::Wave,
            at: creep::centre_at(map.path(), 0.0)
        }]
    );
    assert!(heard(&wave, &next(&wave)).is_empty());

    for (outcome, sound, at) in [
        (Outcome::Won, Sound::Won, middle()),
        (Outcome::Lost, Sound::Lost, map.exit_centre()),
    ] {
        let mut over = next(&before);
        over.stats.outcome = outcome;
        assert_eq!(heard(&before, &over), [Cue { sound, at }]);
        assert!(heard(&over, &next(&over)).is_empty());
    }
}

/// **The same snapshot again is heard as nothing**, which is what a frame
/// that ran no tick hands the watcher — and an event is not heard twice by
/// the frames after it.
#[test]
fn an_event_is_heard_once_however_many_frames_see_it() {
    let map = map();
    let mut watcher = Watcher::default();
    let hz = crate::game::DEFAULT_TICK_HZ;
    let before = field();
    let mut built = next(&before);
    built.render.towers[0] = tower(tower::Kind::Splash, Tier::Base, false);
    assert!(watcher.hear(&before, &map, hz).is_empty());
    assert_eq!(watcher.hear(&built, &map, hz).len(), 1);
    assert!(watcher.hear(&built, &map, hz).is_empty());
    assert!(watcher.hear(&next(&built), &map, hz).is_empty());
}

/// **A field that jumped is a new baseline, not a burst of events**: the
/// clock going back, a new run, a gap past [`LONGEST_HEARD_GAP_S`] — and
/// after [`Watcher::forget`]. Each is followed by a tick that is heard, so
/// the baseline really moved.
#[test]
fn a_field_that_jumped_is_heard_as_nothing_and_becomes_the_baseline() {
    let map = map();
    let hz = crate::game::DEFAULT_TICK_HZ;
    let longest = (LONGEST_HEARD_GAP_S * f64::from(hz)) as u64;
    let before = field();
    let restored = |decoded: &mut Decoded| {
        decoded.render.towers[1] = tower(tower::Kind::Slow, Tier::Upgraded, false);
        decoded.stats.wave = 3;
    };
    let mut back = before;
    back.stats.ticks = AT - 10;
    let mut new_run = next(&before);
    new_run.stats.runs = 2;
    let mut far_on = before;
    far_on.stats.ticks = AT + longest + 1;
    for (what, mut jumped) in [("back", back), ("a new run", new_run), ("far on", far_on)] {
        restored(&mut jumped);
        let mut watcher = Watcher::default();
        assert!(watcher.hear(&before, &map, hz).is_empty());
        assert!(watcher.hear(&jumped, &map, hz).is_empty(), "{what}");
        let mut then = next(&jumped);
        then.stats.wave = 4;
        assert_eq!(watcher.hear(&then, &map, hz).len(), 1, "{what}");
    }

    // The longest gap itself is still play.
    let mut just = before;
    just.stats.ticks = AT + longest;
    just.stats.wave = 1;
    assert_eq!(heard(&before, &just).len(), 1);

    let mut watcher = Watcher::default();
    assert!(watcher.hear(&before, &map, hz).is_empty());
    watcher.forget();
    let mut built = next(&before);
    restored(&mut built);
    assert!(watcher.hear(&built, &map, hz).is_empty(), "forgotten");
}

/// **A refusal is heard at the plot the cursor is on.**
#[test]
fn a_refusal_is_heard_at_the_cursors_plot() {
    assert_eq!(
        refused_at(&map(), 3),
        Cue {
            sound: Sound::Refused,
            at: muzzle_of(3)
        }
    );
}
