use super::*;
use crate::map::{HALF_DEPTH, HALF_WIDTH, MUZZLE_Y};
use crate::tower::TOWERS;
use crate::wave::GAP_S;

/// The entity entries `blob` frames, as a client's baseline hands them back:
/// eight bytes of entity bits, four of length, then the data.
fn entries(blob: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut rest = blob;
    while !rest.is_empty() {
        let bits = u64::from_le_bytes(rest[..8].try_into().unwrap());
        let len = u32::from_le_bytes(rest[8..12].try_into().unwrap()) as usize;
        out.push((bits, &rest[12..12 + len]));
        rest = &rest[12 + len..];
    }
    out
}

/// A field with a little of everything on it, every position on the wire's
/// grid so it survives quantization exactly.
fn field() -> (RenderState, Stats) {
    let mut render = RenderState::default();
    render.towers[0] = Some(TowerView {
        kind: tower::Kind::Splash,
        tier: Tier::Upgraded,
        working: true,
    });
    render.towers[5] = Some(TowerView {
        kind: tower::Kind::Slow,
        tier: Tier::Base,
        working: false,
    });
    render.creeps[0] = CreepView {
        kind: creep::Kind::Tanky,
        centre: DVec3::new(-12.5, 0.453125, 3.25),
        facing: facing_of(64),
        hurt: true,
        slowed: false,
    };
    render.creeps[1] = CreepView {
        kind: creep::Kind::Swarm,
        centre: DVec3::new(17.0, 0.453125, -11.75),
        facing: facing_of(200),
        hurt: false,
        slowed: true,
    };
    render.creeps_alive = 2;
    render.bolts[0] = DVec3::new(1.5, 1.625, -2.0);
    render.bolts_flying = 1;
    render.bursts[0] = BurstView {
        centre: DVec3::new(-3.0, 0.5, 4.0),
        radius_m: 3.5,
    };
    render.bursts_live = 1;
    let stats = Stats {
        ticks: 12_345,
        gold: 275,
        lives: 9,
        wave: 4,
        creeps: 2,
        towers: 2,
        plots: 8,
        bolts: 1,
        kills: 61,
        leaks: 3,
        shots: 480,
        built: 2,
        built_by_kind: [0, 1, 1],
        upgrades: 1,
        refused: 7,
        outcome: Outcome::Playing,
        runs: 2,
        next_wave_in: Some(1.5),
    };
    render.gold = stats.gold;
    render.lives = stats.lives;
    render.wave = stats.wave;
    render.kills = stats.kills;
    render.leaks = stats.leaks;
    render.outcome = stats.outcome;
    render.next_wave_in = stats.next_wave_in;
    (render, stats)
}

/// **What the host's frame reads is what a client's frame reads.** Every
/// tower, creep, bolt and burst and every number goes through the wire and
/// comes back as it was, one entity apiece.
#[test]
fn a_field_round_trips_through_the_wire() {
    let (render, stats) = field();
    let mut blob = Vec::new();
    let encoded = encode(&render, &stats, &mut blob);
    assert_eq!(
        encoded,
        Encoded {
            entities: 7,
            refused: 0
        },
        "the numbers, two towers, two creeps, a bolt and a burst"
    );

    let decoded = decode(entries(&blob));
    assert_eq!(decoded.undecodable, 0);
    assert_eq!(decoded.stats, stats);
    assert_eq!(decoded.render, render);
}

/// **Entity order on the wire is not list order.** A client's baseline hands
/// its entities back in any order; the lists come back in the host's.
#[test]
fn lists_are_rebuilt_in_index_order_whatever_order_they_arrive_in() {
    let (render, stats) = field();
    let mut blob = Vec::new();
    encode(&render, &stats, &mut blob);
    let mut shuffled = entries(&blob);
    shuffled.reverse();
    assert_eq!(decode(shuffled).render, render);
}

/// **A value the wire cannot carry is refused, not clamped.** A creep off the
/// field's extent is left out and counted; everything else still ships.
#[test]
fn a_position_past_the_extent_is_left_out_and_counted() {
    let (mut render, stats) = field();
    render.creeps[1].centre.x = POSITION_REACH_M + 1.0;
    let mut blob = Vec::new();
    let encoded = encode(&render, &stats, &mut blob);
    assert_eq!(encoded.refused, 1);
    assert_eq!(encoded.entities, 6);
    let decoded = decode(entries(&blob));
    assert_eq!(decoded.render.creeps_alive, 1);
    assert_eq!(decoded.render.creeps[0], render.creeps[0]);
}

/// **An entry no build of this sample wrote is skipped and counted**: a
/// wrong length, an unknown kind of entity, a tower of a kind the table does
/// not have, a plot past the pool. The rest still decode.
#[test]
fn entries_no_build_wrote_are_skipped_and_counted() {
    let (render, stats) = field();
    let mut blob = Vec::new();
    encode(&render, &stats, &mut blob);
    let mut bad_kind = Vec::new();
    quantize::encode_values(TOWER_SCHEMA, &[3.0, 0.0, 0.0], &mut bad_kind).unwrap();
    let mut good_tower = Vec::new();
    quantize::encode_values(TOWER_SCHEMA, &[0.0, 0.0, 0.0], &mut good_tower).unwrap();
    for bad in [
        (CREEP | 7, &[0u8; 3][..]),
        (9 << 32, &good_tower[..]),
        (TOWER | 2, &bad_kind[..]),
        (TOWER | MAX_PLOTS_ON_THE_WIRE, &good_tower[..]),
        (HUD | 1, &good_tower[..]),
    ] {
        let mut all = entries(&blob);
        all.push(bad);
        let decoded = decode(all);
        assert_eq!(decoded.undecodable, 1, "{:#x}", bad.0);
        assert_eq!(decoded.render, render, "{:#x}", bad.0);
    }
}

/// One plot past the widest map: the first tower index with no slot.
const MAX_PLOTS_ON_THE_WIRE: u64 = crate::map::MAX_PLOTS as u64;

/// **A heading folds at the half turn.** `Path::heading_at` can answer `π`,
/// which a fixed-point range ending there would refuse; it is the same
/// heading as `-π`, and so is anything within half a step below it.
#[test]
fn a_facing_at_the_half_turn_folds_onto_minus_pi() {
    let step = 2.0 * PI / f64::from(FACING_CODES);
    assert_eq!(facing_code(PI as f32), 0);
    assert_eq!(facing_code((PI - 0.4 * step) as f32), 0);
    assert_eq!(facing_code(-PI as f32), 0);
    assert_eq!(facing_code(0.0), FACING_CODES / 2);
    assert_eq!(facing_code((PI - 0.6 * step) as f32), FACING_CODES - 1);
    for code in 0..FACING_CODES {
        assert_eq!(facing_code(facing_of(code)), code);
    }
}

/// **The extents hold the field, every tower's reach and every burst**, with
/// room: a bolt flies no further from its plot than its tower reaches, and a
/// plot is on the field.
#[test]
fn the_extents_hold_the_field_and_every_reach() {
    let reach = TOWERS
        .iter()
        .flatten()
        .map(|spec| spec.range_m)
        .fold(0.0, f64::max);
    assert!(HALF_WIDTH.max(HALF_DEPTH) + 2.0 * reach < POSITION_REACH_M);
    const { assert!(2.0 * MUZZLE_Y < VERTICAL_REACH_M) };
    let burst = TOWERS
        .iter()
        .flatten()
        .map(|spec| spec.burst_m)
        .fold(0.0, f64::max);
    assert!(burst < BURST_REACH_M);
    const { assert!(GAP_S < NEXT_WAVE_REACH_S) };
}

/// Whether `a` and `b` are the same field to the wire's precision: every
/// count, flag, kind and number exact, every position and heading within a
/// step.
fn same_field(a: &RenderState, b: &RenderState) -> bool {
    let near = |x: DVec3, y: DVec3| (x - y).abs().max_element() <= 1.0 / 256.0;
    let step = 2.0 * PI / f64::from(FACING_CODES);
    a.towers == b.towers
        && a.creeps_alive == b.creeps_alive
        && a.bolts_flying == b.bolts_flying
        && a.bursts_live == b.bursts_live
        && (a.gold, a.lives, a.wave, a.kills, a.leaks, a.outcome)
            == (b.gold, b.lives, b.wave, b.kills, b.leaks, b.outcome)
        && a.next_wave_in.is_some() == b.next_wave_in.is_some()
        && a.creeps[..a.creeps_alive]
            .iter()
            .zip(&b.creeps[..b.creeps_alive])
            .all(|(x, y)| {
                let turn = f64::from(x.facing - y.facing).abs();
                (x.kind, x.hurt, x.slowed) == (y.kind, y.hurt, y.slowed)
                    && near(x.centre, y.centre)
                    && (turn <= step || (2.0 * PI - turn) <= step)
            })
        && a.bolts[..a.bolts_flying]
            .iter()
            .zip(&b.bolts[..b.bolts_flying])
            .all(|(x, y)| near(*x, *y))
        && a.bursts[..a.bursts_live]
            .iter()
            .zip(&b.bursts[..b.bursts_live])
            .all(|(x, y)| near(x.centre, y.centre) && (x.radius_m - y.radius_m).abs() <= 1.0 / 32.0)
}

/// **Solo's own client reconstructs the field the stage holds**, tick after
/// tick, through the same snapshots a LAN joiner reads — so the wire a joiner
/// draws from is exercised by every solo run, and it is the stage.
#[test]
fn solos_client_reconstructs_the_stage_every_tick() {
    use crate::game::{Controls, DEFAULT_TICK_HZ, Game};

    let mut game = Game::new(DEFAULT_TICK_HZ, &crate::map::Map::built_in()).expect("solo");
    let plots = game.map().plots().len();
    let (mut compared, mut crowded) = (0, 0);
    for tick in 0..30 * u64::from(DEFAULT_TICK_HZ) {
        let stats = game.stats();
        let free = (0..plots).find(|&plot| game.render_state().towers[plot].is_none());
        game.set_controls(match free {
            Some(plot) if tick % 2 == 0 => Controls {
                place: Some(plot as u8),
                kind: tower::ALL[plot % tower::KINDS],
                ..Controls::default()
            },
            _ if stats.next_wave_in.is_some() => Controls {
                start_wave: true,
                ..Controls::default()
            },
            _ => Controls::default(),
        });
        game.tick();

        let decoded = game.replicated();
        assert_eq!(decoded.undecodable, 0);
        assert_eq!(decoded.stats.ticks, game.stats().ticks, "tick {tick}");
        assert!(
            same_field(&game.render_state(), &decoded.render),
            "tick {tick}: the client's field is not the stage's"
        );
        compared += 1;
        if decoded.render.creeps_alive > 0 && decoded.render.bolts_flying > 0 {
            crowded += 1;
        }
    }
    assert!(
        compared > 0 && crowded > 0,
        "{crowded} of {compared} ticks had creeps and bolts"
    );
}
