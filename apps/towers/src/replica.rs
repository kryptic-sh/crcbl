//! What the server replicates of the field, and how a client reads it back.
//!
//! ```text
//!   Stage ──▶ RenderState + Stats ──encode──▶ "towers" system ──snapshot──▶
//!       ──▶ Client::replicated("towers") ──decode──▶ RenderState + Stats
//! ```
//!
//! # The wire is the frame's own view
//!
//! A remote player's client has no stage — the stage is the host's, and rule
//! 2 has no exemption for a tower defense — so what it draws is what the
//! host's snapshots carry. What they carry is exactly what a frame reads off
//! the stage on the host: a [`RenderState`] and the [`Stats`] beside it. So
//! the host encodes those two, and a client decodes the same two, and the
//! rest of the sample cannot tell which side of the wire a frame came from.
//!
//! # One entity per thing on the field
//!
//! The snapshot is diffed and fitted to a datagram **per entity**
//! (`crcbl::net::budget`), so each tower, creep, bolt and burst is its own
//! entity, keyed by what it is and where it sits in its list, and the numbers
//! are one entity more. A tick that moves the creeps ships the creeps; a tower
//! that did not change ships nothing. Each entity's bytes are a
//! [`crcbl::ecs::quantize`] schema: counters as whole numbers of just the bits
//! they need, positions as fixed point over the field, so what a creep costs a
//! snapshot is a few bytes rather than a `CreepView`'s width.
//!
//! # Refuse, never clamp
//!
//! A value a schema cannot carry — a position off the field's extent, a
//! counter past its width — is not clamped into range: that entity is left
//! out of the snapshot and counted ([`Encoded::refused`]), and whoever
//! replicates says so. [`POSITION_REACH_M`] and [`VERTICAL_REACH_M`] are
//! sized so the committed field and every tower's reach sit well inside them,
//! which `the_extents_hold_the_field_and_every_reach` asserts.

use std::f64::consts::PI;

use crcbl::ecs::quantize::{self, Codec, Field, Fixed, QuantizeError};
use crcbl::math::DVec3;

use crate::creep::{self, CreepView};
use crate::game::{RenderState, Stats};
use crate::tower::{self, BurstView, Tier, TowerView};
use crate::wave::Outcome;

/// The replicated system's name: what the host registers and a client reads
/// back through `crcbl::client::Client::replicated`.
pub const SYSTEM: &str = "towers";

/// How far from the field's centre a position may be along `X` or `Z`, in
/// metres.
pub const POSITION_REACH_M: f64 = 64.0;

/// How far above or below the ground a position may be, in metres.
pub const VERTICAL_REACH_M: f64 = 8.0;

/// The widest splash burst the wire carries, in metres.
pub const BURST_REACH_M: f64 = 8.0;

/// The longest wait until the next wave the wire carries, in seconds.
pub const NEXT_WAVE_REACH_S: f64 = 16.0;

/// How many codes a creep's facing has in a full turn: a step of under a
/// degree and a half, which a creep a few pixels wide does not show.
///
/// A whole-number code rather than [`Fixed`] point over `[-π, π)`, because a
/// heading wraps: an angle within half a step below `π` rounds to the code
/// past the last, which fixed point refuses and a turn folds onto `-π`.
const FACING_CODES: u32 = 256;

/// How many bits a creep's health takes on the wire: a fraction of what it
/// started with, in [`HEALTH_STEPS`] steps.
///
/// Fine enough that every hit moves it — the smallest damage any tower does
/// against the most health any creep has is several steps, which
/// `every_hit_moves_a_creeps_health_on_the_wire` asserts — because a client
/// hears a creep hit by its health falling between two snapshots.
const HEALTH_BITS: u32 = 8;

/// The steps a whole creep's health is divided into: the top code is a whole
/// creep and code zero a dead one, so both ends are exact.
const HEALTH_STEPS: u32 = (1 << HEALTH_BITS) - 1;

/// How many bits a creep's or a burst's tag takes — see
/// [`crate::creep::CreepView::tag`].
const TAG_BITS: u32 = u16::BITS;

/// The entity bits of the numbers: the one entity with no list behind it.
const HUD: u64 = 0;
/// What an entity is, in the high half of its bits; where it sits in its
/// list is the low half.
const TOWER: u64 = 1 << 32;
const CREEP: u64 = 2 << 32;
const BOLT: u64 = 3 << 32;
const BURST: u64 = 4 << 32;
/// The low half: an entity's index in its list.
const INDEX: u64 = 0xFFFF_FFFF;

/// A whole number from zero to `2^bits - 1`, one code apiece.
const fn whole(name: &'static str, bits: u32) -> Field {
    Field {
        name,
        codec: Codec::Fixed(Fixed::new(0.0, (1u64 << bits) as f64, bits)),
    }
}

/// A flag: one bit.
const fn flag(name: &'static str) -> Field {
    whole(name, 1)
}

/// One horizontal axis of a position: a 2⁻⁹ m step.
const fn across(name: &'static str) -> Field {
    Field {
        name,
        codec: Codec::Fixed(Fixed::new(-POSITION_REACH_M, POSITION_REACH_M, 16)),
    }
}

/// The vertical axis of a position: a 2⁻⁸ m step.
const fn up(name: &'static str) -> Field {
    Field {
        name,
        codec: Codec::Fixed(Fixed::new(-VERTICAL_REACH_M, VERTICAL_REACH_M, 12)),
    }
}

/// The numbers: [`Stats`], which is also where [`RenderState`]'s own numbers
/// are read from.
const HUD_SCHEMA: &[Field] = &[
    whole("ticks", 32),
    whole("gold", 32),
    whole("lives", 16),
    whole("wave", 8),
    whole("creeps", 16),
    whole("towers", 8),
    whole("plots", 8),
    whole("bolts", 8),
    whole("kills", 32),
    whole("leaks", 32),
    whole("shots", 32),
    whole("built", 32),
    whole("built bolt", 16),
    whole("built splash", 16),
    whole("built slow", 16),
    whole("upgrades", 16),
    whole("refused", 32),
    whole("outcome", 2),
    whole("runs", 32),
    flag("next wave due"),
    Field {
        name: "next wave in",
        codec: Codec::Fixed(Fixed::new(0.0, NEXT_WAVE_REACH_S, 12)),
    },
];

/// A tower: its kind, its tier, and whether it is working this instant.
const TOWER_SCHEMA: &[Field] = &[whole("kind", 2), flag("upgraded"), flag("working")];

/// A creep: its kind, its two flags, where it is and which way it faces, as
/// one of [`FACING_CODES`], what it has left and its tag.
///
/// **The last two are presentation**, and in no state hash: the health is
/// what its bar fills to and the tag is what a client matches it across
/// snapshots by, which is how a joiner hears it hit, killed or leaking — see
/// `crate::cue`.
const CREEP_SCHEMA: &[Field] = &[
    whole("kind", 2),
    flag("hurt"),
    flag("slowed"),
    across("x"),
    up("y"),
    across("z"),
    whole("facing", FACING_CODES.ilog2()),
    whole("health", HEALTH_BITS),
    whole("tag", TAG_BITS),
];

/// A bolt: where it is.
const BOLT_SCHEMA: &[Field] = &[across("x"), up("y"), across("z")];

/// A burst: where it is, how far it reaches, to a 2⁻⁵ m step, and its tag —
/// what a client tells a new burst from one it has heard by.
const BURST_SCHEMA: &[Field] = &[
    across("x"),
    up("y"),
    across("z"),
    Field {
        name: "radius",
        codec: Codec::Fixed(Fixed::new(0.0, BURST_REACH_M, 8)),
    },
    whole("tag", TAG_BITS),
];

/// What [`encode`] wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Encoded {
    /// Entities written.
    pub entities: usize,
    /// Entities left out because a value would not fit its schema — see the
    /// module docs.
    pub refused: usize,
}

/// Appends one entity entry per thing on the field to `out`, in the framing
/// `SystemTrait::replicate` writes.
pub fn encode(render: &RenderState, stats: &Stats, out: &mut Vec<u8>) -> Encoded {
    let mut encoded = Encoded::default();
    let mut data = Vec::new();
    let mut put = |bits: u64, schema: &[Field], values: &[f64]| {
        data.clear();
        match quantize::encode_values(schema, values, &mut data) {
            Ok(()) => {
                crcbl::net::encode_entity_entry(out, bits, &data);
                encoded.entities += 1;
            }
            Err(_) => encoded.refused += 1,
        }
    };

    put(HUD, HUD_SCHEMA, &hud_values(stats));
    for (plot, tower) in render.towers.iter().enumerate() {
        if let Some(tower) = tower {
            put(TOWER | plot as u64, TOWER_SCHEMA, &tower_values(tower));
        }
    }
    for (at, creep) in render.creeps[..render.creeps_alive].iter().enumerate() {
        put(CREEP | at as u64, CREEP_SCHEMA, &creep_values(creep));
    }
    for (at, bolt) in render.bolts[..render.bolts_flying].iter().enumerate() {
        put(BOLT | at as u64, BOLT_SCHEMA, &position_values(*bolt));
    }
    for (at, burst) in render.bursts[..render.bursts_live].iter().enumerate() {
        let [x, y, z] = position_values(burst.centre);
        put(
            BURST | at as u64,
            BURST_SCHEMA,
            &[x, y, z, burst.radius_m, f64::from(burst.tag)],
        );
    }
    encoded
}

/// What [`decode`] reconstructed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decoded {
    /// The field, as a frame draws it.
    pub render: RenderState,
    /// The numbers, as the panel and the `[HUD]` line read them.
    pub stats: Stats,
    /// Entities that did not decode — a length or a code no build of this
    /// sample writes, or an index past its list's pool — and were skipped.
    pub undecodable: usize,
}

/// Rebuilds what the host's frame read off its stage from the entities its
/// `"towers"` system replicated, in any order.
///
/// Lists are packed in their entities' index order, which is the host's
/// list order, and cut at their pools — the same widths [`RenderState`]
/// has. Before the numbers arrive the stats are the default, and the field's
/// numbers with them.
pub fn decode<'a>(entities: impl IntoIterator<Item = (u64, &'a [u8])>) -> Decoded {
    let mut decoded = Decoded::default();
    let mut creeps: Vec<(u64, CreepView)> = Vec::new();
    let mut bolts: Vec<(u64, DVec3)> = Vec::new();
    let mut bursts: Vec<(u64, BurstView)> = Vec::new();

    for (bits, data) in entities {
        let index = bits & INDEX;
        let read = match bits & !INDEX {
            HUD if index == 0 => read_hud(data).map(|stats| decoded.stats = stats),
            TOWER => read_tower(data).and_then(|tower| {
                let slot = decoded
                    .render
                    .towers
                    .get_mut(usize::try_from(index).map_err(|_| QuantizeError::Refused)?)
                    .ok_or(QuantizeError::Refused)?;
                *slot = Some(tower);
                Ok(())
            }),
            CREEP => read_creep(data).map(|creep| creeps.push((index, creep))),
            BOLT => read_position(BOLT_SCHEMA, data).map(|bolt| bolts.push((index, bolt))),
            BURST => read_burst(data).map(|burst| bursts.push((index, burst))),
            _ => Err(QuantizeError::Refused),
        };
        if read.is_err() {
            decoded.undecodable += 1;
        }
    }

    let render = &mut decoded.render;
    render.creeps_alive = pack(creeps, &mut render.creeps, &mut decoded.undecodable);
    render.bolts_flying = pack(bolts, &mut render.bolts, &mut decoded.undecodable);
    render.bursts_live = pack(bursts, &mut render.bursts, &mut decoded.undecodable);
    let stats = &decoded.stats;
    render.gold = stats.gold;
    render.lives = stats.lives;
    render.wave = stats.wave;
    render.kills = stats.kills;
    render.leaks = stats.leaks;
    render.outcome = stats.outcome;
    render.next_wave_in = stats.next_wave_in;
    decoded
}

/// Writes `items` into `pool` in index order, counting what the pool cannot
/// hold as undecodable, and answers how many it holds.
fn pack<T: Copy>(mut items: Vec<(u64, T)>, pool: &mut [T], undecodable: &mut usize) -> usize {
    items.sort_unstable_by_key(|&(index, _)| index);
    *undecodable += items.len().saturating_sub(pool.len());
    for (slot, (_, item)) in pool.iter_mut().zip(&items) {
        *slot = *item;
    }
    items.len().min(pool.len())
}

fn hud_values(stats: &Stats) -> [f64; 21] {
    [
        stats.ticks as f64,
        f64::from(stats.gold),
        f64::from(stats.lives),
        stats.wave as f64,
        stats.creeps as f64,
        stats.towers as f64,
        stats.plots as f64,
        stats.bolts as f64,
        stats.kills as f64,
        stats.leaks as f64,
        stats.shots as f64,
        stats.built as f64,
        stats.built_by_kind[tower::Kind::Bolt.index()] as f64,
        stats.built_by_kind[tower::Kind::Splash.index()] as f64,
        stats.built_by_kind[tower::Kind::Slow.index()] as f64,
        stats.upgrades as f64,
        stats.refused as f64,
        f64::from(outcome_code(stats.outcome)),
        stats.runs as f64,
        f64::from(u8::from(stats.next_wave_in.is_some())),
        stats.next_wave_in.unwrap_or(0.0),
    ]
}

fn read_hud(data: &[u8]) -> Result<Stats, QuantizeError> {
    let mut v = [0.0; 21];
    quantize::decode_values(HUD_SCHEMA, data, &mut v)?;
    // Every field but the last is a whole number of at most 32 bits, so each
    // cast below is exact.
    let mut built_by_kind = [0; tower::KINDS];
    built_by_kind[tower::Kind::Bolt.index()] = v[12] as u64;
    built_by_kind[tower::Kind::Splash.index()] = v[13] as u64;
    built_by_kind[tower::Kind::Slow.index()] = v[14] as u64;
    Ok(Stats {
        ticks: v[0] as u64,
        gold: v[1] as u32,
        lives: v[2] as u32,
        wave: v[3] as usize,
        creeps: v[4] as usize,
        towers: v[5] as usize,
        plots: v[6] as usize,
        bolts: v[7] as usize,
        kills: v[8] as u64,
        leaks: v[9] as u64,
        shots: v[10] as u64,
        built: v[11] as u64,
        built_by_kind,
        upgrades: v[15] as u64,
        refused: v[16] as u64,
        outcome: outcome_of(v[17] as u8).ok_or(QuantizeError::Refused)?,
        runs: v[18] as u64,
        next_wave_in: (v[19] != 0.0).then_some(v[20]),
    })
}

fn tower_values(tower: &TowerView) -> [f64; 3] {
    [
        tower.kind.index() as f64,
        f64::from(u8::from(tower.tier == Tier::Upgraded)),
        f64::from(u8::from(tower.working)),
    ]
}

fn read_tower(data: &[u8]) -> Result<TowerView, QuantizeError> {
    let mut v = [0.0; 3];
    quantize::decode_values(TOWER_SCHEMA, data, &mut v)?;
    Ok(TowerView {
        kind: tower::Kind::from_index(v[0] as u8).ok_or(QuantizeError::Refused)?,
        tier: if v[1] == 0.0 {
            Tier::Base
        } else {
            Tier::Upgraded
        },
        working: v[2] != 0.0,
    })
}

fn creep_values(creep: &CreepView) -> [f64; 9] {
    let [x, y, z] = position_values(creep.centre);
    [
        creep.kind.index() as f64,
        f64::from(u8::from(creep.hurt)),
        f64::from(u8::from(creep.slowed)),
        x,
        y,
        z,
        f64::from(facing_code(creep.facing)),
        f64::from(health_code(creep.health)),
        f64::from(creep.tag),
    ]
}

fn read_creep(data: &[u8]) -> Result<CreepView, QuantizeError> {
    let mut v = [0.0; 9];
    quantize::decode_values(CREEP_SCHEMA, data, &mut v)?;
    Ok(CreepView {
        kind: *creep::ALL
            .get(v[0] as usize)
            .ok_or(QuantizeError::Refused)?,
        centre: DVec3::new(v[3], v[4], v[5]),
        facing: facing_of(v[6] as u32),
        hurt: v[1] != 0.0,
        slowed: v[2] != 0.0,
        health: health_of(v[7] as u32).ok_or(QuantizeError::Refused)?,
        // A whole number of `TAG_BITS` bits, so the cast is exact.
        tag: v[8] as u16,
    })
}

/// The step nearest `health`, a fraction of a whole creep. Past either end
/// is no health a creep can have, and is answered with a code the schema
/// refuses rather than clamped — see the module docs.
fn health_code(health: f32) -> u32 {
    if (0.0..=1.0).contains(&health) {
        (health * HEALTH_STEPS as f32).round() as u32
    } else {
        u32::MAX
    }
}

/// The fraction `code` stands for, or `None` for a code past a whole creep.
fn health_of(code: u32) -> Option<f32> {
    (code <= HEALTH_STEPS).then(|| code as f32 / HEALTH_STEPS as f32)
}

/// The code nearest `facing`, in radians, a full turn folding onto zero.
fn facing_code(facing: f32) -> u32 {
    let turn = (f64::from(facing) + PI) / (2.0 * PI);
    ((turn * f64::from(FACING_CODES)).round() as i64).rem_euclid(i64::from(FACING_CODES)) as u32
}

/// The heading `code` stands for, in `[-π, π)`.
fn facing_of(code: u32) -> f32 {
    (f64::from(code) / f64::from(FACING_CODES) * 2.0 * PI - PI) as f32
}

fn position_values(at: DVec3) -> [f64; 3] {
    [at.x, at.y, at.z]
}

fn read_position(schema: &[Field], data: &[u8]) -> Result<DVec3, QuantizeError> {
    let mut v = [0.0; 3];
    quantize::decode_values(schema, data, &mut v)?;
    Ok(DVec3::new(v[0], v[1], v[2]))
}

fn read_burst(data: &[u8]) -> Result<BurstView, QuantizeError> {
    let mut v = [0.0; 5];
    quantize::decode_values(BURST_SCHEMA, data, &mut v)?;
    Ok(BurstView {
        centre: DVec3::new(v[0], v[1], v[2]),
        radius_m: v[3],
        // A whole number of `TAG_BITS` bits, so the cast is exact.
        tag: v[4] as u16,
    })
}

const fn outcome_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::Playing => 0,
        Outcome::Won => 1,
        Outcome::Lost => 2,
    }
}

const fn outcome_of(code: u8) -> Option<Outcome> {
    match code {
        0 => Some(Outcome::Playing),
        1 => Some(Outcome::Won),
        2 => Some(Outcome::Lost),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
