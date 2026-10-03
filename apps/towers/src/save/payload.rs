//! The bytes inside a towers save's one sector: [`encode`] writes a
//! [`Checkpoint`] out, and [`decode`] reads one back from a stranger's file,
//! refusing by name anything a run between waves could not have written.
//!
//! The layout is the table in `crate::save`'s module docs.

use crcbl::math::DVec3;
use crcbl::net::types::SectorId;
use crcbl::store::save::SaveData;

use super::{Checkpoint, SaveError, SavedBolt, SavedBurst, SavedCreep, SavedTower};
use crate::creep;
use crate::map::Map;
use crate::tower::{self, Kind, TOWERS, Tier};
use crate::wave::{self, MAX_CREEPS, STARTING_GOLD, STARTING_LIVES, TOTAL_BOUNTY, WAVES};

/// What a towers payload starts with, so a file from something else is
/// refused before its bytes are read as numbers. It spells `TWRS`, as the
/// LAN protocol id does.
const PAYLOAD_MAGIC: &[u8; 4] = b"TWRS";

/// The payload's own version, inside the container's. Bump it when a field
/// is added, moved or reinterpreted; a file of any other version is refused
/// as [`SaveError::Version`].
pub(super) const PAYLOAD_VERSION: u16 = 1;

/// One coordinate's or one clock's width: an `f64`'s bits.
const FLOAT_BYTES: usize = size_of::<u64>();

/// One point's width: three coordinates.
const POINT_BYTES: usize = 3 * FLOAT_BYTES;

/// One tower's bytes: plot, kind and tier, then when it may fire again and
/// when it last did.
pub(super) const TOWER_BYTES: usize = 3 + 2 * FLOAT_BYTES;

/// One creep's bytes: kind, how far it has walked, its health and its hold.
const CREEP_BYTES: usize = 1 + FLOAT_BYTES + size_of::<u32>() + FLOAT_BYTES;

/// One bolt's bytes: id, where it is, where it is heading, its target, its
/// damage and its burst.
const BOLT_BYTES: usize = size_of::<u64>() + 2 * POINT_BYTES + 2 * size_of::<u32>() + FLOAT_BYTES;

/// One burst's bytes: id, where it was raised, how far it reached and when.
const BURST_BYTES: usize = size_of::<u64>() + POINT_BYTES + 2 * FLOAT_BYTES;

/// A bolt's target byte for a creep no longer on the field.
const NO_TARGET: u32 = u32::MAX;

/// How far from the origin a saved point may be, in metres. The field is
/// tens of metres across, so a point past this did not come from a run, and
/// bounding it keeps a number no query was built for out of the physics
/// world.
const POSITION_LIMIT_M: f64 = 1.0e4;

/// The payload bytes for `checkpoint`.
pub(super) fn encode(checkpoint: &Checkpoint) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(PAYLOAD_MAGIC);
    bytes.extend_from_slice(&PAYLOAD_VERSION.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.map);
    bytes.extend_from_slice(&checkpoint.runs.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.gold.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.lives.to_le_bytes());
    for count in [
        checkpoint.kills,
        checkpoint.leaks,
        checkpoint.shots,
        checkpoint.built,
    ] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    // Written rather than implied by the length, so a build with another
    // number of kinds refuses the file by name instead of reading the next
    // field's bytes as a kind's count.
    bytes.push(u8::try_from(tower::KINDS).expect("the kind table fits a byte"));
    for count in checkpoint.built_by_kind {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes.extend_from_slice(&checkpoint.upgrades.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.refused.to_le_bytes());
    push_count(&mut bytes, checkpoint.wave);
    push_float(&mut bytes, checkpoint.due_at);

    push_count(&mut bytes, checkpoint.towers.len());
    for saved in &checkpoint.towers {
        bytes.push(u8::try_from(saved.plot).expect("a map's plots fit a byte"));
        bytes.push(u8::try_from(saved.kind.index()).expect("the kind table fits a byte"));
        bytes.push(u8::try_from(saved.tier.index()).expect("the tier table fits a byte"));
        push_float(&mut bytes, saved.ready_at);
        push_float(&mut bytes, saved.fired_at);
    }
    push_count(&mut bytes, checkpoint.creeps.len());
    for saved in &checkpoint.creeps {
        bytes.push(u8::try_from(saved.kind.index()).expect("the creep table fits a byte"));
        push_float(&mut bytes, saved.along);
        bytes.extend_from_slice(&saved.health.to_le_bytes());
        push_float(&mut bytes, saved.slow);
    }
    push_count(&mut bytes, checkpoint.bolts.len());
    for saved in &checkpoint.bolts {
        bytes.extend_from_slice(&saved.id.to_le_bytes());
        push_point(&mut bytes, saved.at);
        push_point(&mut bytes, saved.heading);
        let target = saved.target.map_or(NO_TARGET, |index| {
            u32::try_from(index).expect("the creeps on a field fit a u32")
        });
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&saved.damage.to_le_bytes());
        push_float(&mut bytes, saved.burst_m);
    }
    push_count(&mut bytes, checkpoint.bursts.len());
    for saved in &checkpoint.bursts {
        bytes.extend_from_slice(&saved.id.to_le_bytes());
        push_point(&mut bytes, saved.at);
        push_float(&mut bytes, saved.radius_m);
        push_float(&mut bytes, saved.raised_at);
    }
    bytes
}

/// Writes `count` as the `u32` every count is.
fn push_count(bytes: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count).expect("every count a stage holds fits a u32");
    bytes.extend_from_slice(&count.to_le_bytes());
}

/// Writes `value` as its bits: a resumed run is held to bits, not to a
/// rounding of them.
fn push_float(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

/// Writes `point` as its three coordinates' bits.
fn push_point(bytes: &mut Vec<u8>, point: DVec3) {
    for axis in point.to_array() {
        push_float(bytes, axis);
    }
}

/// Reads a payload a field at a time, answering [`SaveError::Truncated`] for
/// a field the bytes stop partway through.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], SaveError> {
        let end = self.at.checked_add(count).ok_or(SaveError::Truncated)?;
        let field = self.bytes.get(self.at..end).ok_or(SaveError::Truncated)?;
        self.at = end;
        Ok(field)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SaveError> {
        self.take(N)?.try_into().map_err(|_| SaveError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, SaveError> {
        self.array::<1>().map(|[byte]| byte)
    }

    fn u16(&mut self) -> Result<u16, SaveError> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, SaveError> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, SaveError> {
        self.array().map(u64::from_le_bytes)
    }

    fn f64(&mut self) -> Result<f64, SaveError> {
        self.u64().map(f64::from_bits)
    }

    fn point(&mut self) -> Result<DVec3, SaveError> {
        Ok(DVec3::new(self.f64()?, self.f64()?, self.f64()?))
    }

    /// Whether the bytes left hold `count` items `width` bytes each — asked
    /// before anything is reserved for them, so a count the file cannot
    /// carry is refused as truncated rather than allocated.
    fn holds(&self, count: usize, width: usize) -> Result<(), SaveError> {
        match count.checked_mul(width) {
            Some(needed) if needed <= self.left() => Ok(()),
            _ => Err(SaveError::Truncated),
        }
    }

    const fn left(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// A refusal of `what`, as [`SaveError::Invalid`].
fn invalid(what: impl Into<String>) -> SaveError {
    SaveError::Invalid(what.into())
}

/// Whether `point` is a finite place no further than [`POSITION_LIMIT_M`]
/// from the origin on any axis.
fn on_the_field(point: DVec3) -> bool {
    point.is_finite() && point.abs().max_element() <= POSITION_LIMIT_M
}

/// The run `data` holds, played on `map`, or the [`SaveError`] naming why it
/// will not be resumed — see `crate::save`'s module docs for every rule it is
/// held to.
///
/// # Errors
///
/// See [`SaveError`]: every variant but [`SaveError::Nowhere`],
/// [`SaveError::Unwritten`], [`SaveError::Unreadable`], [`SaveError::NoStage`],
/// [`SaveError::NoSave`] and [`SaveError::Recording`] is one of this
/// function's.
pub fn decode(data: &SaveData, map: &Map) -> Result<Checkpoint, SaveError> {
    let [sector] = data.sectors.as_slice() else {
        return Err(SaveError::NotOneSector);
    };
    if sector.sector_id != SectorId::ZERO {
        return Err(SaveError::NotOneSector);
    }
    let mut reader = Reader {
        bytes: &sector.snapshot_data,
        at: 0,
    };

    if reader.array::<4>()? != *PAYLOAD_MAGIC {
        return Err(SaveError::NotTowers);
    }
    let version = reader.u16()?;
    if version != PAYLOAD_VERSION {
        return Err(SaveError::Version { found: version });
    }
    if reader.array::<32>()? != map.fingerprint() {
        return Err(SaveError::OtherMap);
    }

    let ticks = data.header.tick.get();
    let elapsed = data.header.playtime_secs;
    if !elapsed.is_finite() || elapsed < 0.0 {
        return Err(invalid(format!("clock, {elapsed} s, is not a time")));
    }

    let runs = reader.u64()?;
    if runs == 0 {
        return Err(invalid("run count is zero, and the first run is run 1"));
    }
    let gold = reader.u32()?;
    let lives = reader.u32()?;
    let kills = reader.u64()?;
    let leaks = reader.u64()?;
    let shots = reader.u64()?;
    let built = reader.u64()?;
    let kinds = usize::from(reader.u8()?);
    if kinds != tower::KINDS {
        return Err(invalid(format!(
            "towers come in {kinds} kinds, and this build has {}",
            tower::KINDS
        )));
    }
    let mut built_by_kind = [0; tower::KINDS];
    for count in &mut built_by_kind {
        *count = reader.u64()?;
    }
    let upgrades = reader.u64()?;
    let refused = reader.u64()?;
    let wave = reader.u32()? as usize;
    let due_at = reader.f64()?;

    // **Every row started and nothing releasing is a won run** once the field
    // clears, and a run with lives left and every row out has nothing to wait
    // for — so a run between waves has a wave still to come.
    if wave >= WAVES.len() {
        return Err(invalid(format!(
            "wave count, {wave}, leaves no wave of the {} to come",
            WAVES.len()
        )));
    }
    // No later than a whole build phase away, which is the furthest
    // `Waves::step` ever puts the next wave.
    if !due_at.is_finite() || due_at < 0.0 || due_at > elapsed + wave::GAP_S {
        return Err(invalid(format!(
            "next wave, due at {due_at} s, is not one the clock at {elapsed} s could wait for"
        )));
    }
    // A life goes for each leak and nothing gives one back, and a run at none
    // has ended.
    if lives == 0 || u64::from(lives) + leaks != u64::from(STARTING_LIVES) {
        return Err(invalid(format!(
            "{lives} lives after {leaks} leaks is not what a run of {STARTING_LIVES} keeps"
        )));
    }

    let towers = read_towers(&mut reader, map, elapsed)?;
    let creeps = read_creeps(&mut reader, map)?;
    let bolts = read_bolts(&mut reader, shots, creeps.len())?;
    let bursts = read_bursts(&mut reader, shots, elapsed)?;
    if reader.left() > 0 {
        return Err(SaveError::TrailingBytes {
            count: reader.left(),
        });
    }

    // Every creep the started waves let out has been killed, has leaked, or
    // is still on the field — nothing is releasing, so each started row went
    // out whole.
    let released: u64 = WAVES[..wave]
        .iter()
        .map(|row| u64::from(row.creeps()))
        .sum();
    let accounted = kills
        .checked_add(leaks)
        .and_then(|gone| gone.checked_add(creeps.len() as u64));
    if accounted != Some(released) {
        return Err(invalid(format!(
            "{kills} kills, {leaks} leaks and {} creeps on the field are not the {released} \
             creeps {wave} wave(s) released",
            creeps.len()
        )));
    }
    check_the_towers_counted(&towers, built, &built_by_kind, upgrades)?;
    // What the towers cost and what is left cannot be more than the opening
    // purse and every bounty the table pays.
    let spent: u64 = towers
        .iter()
        .map(|saved| {
            let base = u64::from(saved.kind.spec(Tier::Base).cost);
            match saved.tier {
                Tier::Base => base,
                Tier::Upgraded => base + u64::from(saved.kind.spec(Tier::Upgraded).cost),
            }
        })
        .sum();
    let most = u64::from(STARTING_GOLD) + u64::from(TOTAL_BOUNTY);
    if u64::from(gold) + spent > most {
        return Err(invalid(format!(
            "{gold} gold beside {spent} spent is more than the {most} a run can be paid"
        )));
    }

    Ok(Checkpoint {
        map: map.fingerprint(),
        runs,
        ticks,
        elapsed,
        gold,
        lives,
        kills,
        leaks,
        shots,
        built,
        built_by_kind,
        upgrades,
        refused,
        wave,
        due_at,
        towers,
        creeps,
        bolts,
        bursts,
    })
}

/// The towers, held to `map`'s plots — one to a plot — and to the clock at
/// `elapsed`.
fn read_towers(
    reader: &mut Reader<'_>,
    map: &Map,
    elapsed: f64,
) -> Result<Vec<SavedTower>, SaveError> {
    let count = reader.u32()? as usize;
    let plots = map.plots().len();
    if count > plots {
        return Err(invalid(format!(
            "{count} towers do not fit the map's {plots} plots"
        )));
    }
    reader.holds(count, TOWER_BYTES)?;
    let mut towers: Vec<SavedTower> = Vec::with_capacity(count);
    for index in 0..count {
        let plot = usize::from(reader.u8()?);
        let kind_byte = reader.u8()?;
        let tier_byte = reader.u8()?;
        let ready_at = reader.f64()?;
        let fired_at = reader.f64()?;
        if plot >= plots {
            return Err(invalid(format!(
                "tower {index} stands on plot {plot}, and the map has {plots}"
            )));
        }
        if towers.iter().any(|other| other.plot == plot) {
            return Err(invalid(format!("plot {plot} holds two towers")));
        }
        let Some(kind) = Kind::from_index(kind_byte) else {
            return Err(invalid(format!(
                "tower {index} is of kind {kind_byte}, which no tower is"
            )));
        };
        let Some(tier) = Tier::from_index(tier_byte) else {
            return Err(invalid(format!(
                "tower {index} is at tier {tier_byte}, which no tower is"
            )));
        };
        // A shot is taken at a time the clock has reached, and the reload
        // runs from it; a tower that never fired is ready from the start.
        let reload = kind.spec(tier).reload_s;
        if !ready_at.is_finite() || ready_at < 0.0 || ready_at > elapsed + reload {
            return Err(invalid(format!(
                "tower {index} is ready at {ready_at} s, which a shot by {elapsed} s does not \
                 reach"
            )));
        }
        if fired_at != f64::NEG_INFINITY && !(fired_at.is_finite() && fired_at <= elapsed) {
            return Err(invalid(format!(
                "tower {index} last fired at {fired_at} s, which the clock at {elapsed} s has \
                 not reached"
            )));
        }
        towers.push(SavedTower {
            plot,
            kind,
            tier,
            ready_at,
            fired_at,
        });
    }
    Ok(towers)
}

/// Nothing takes a tower down, so the counters count the towers standing.
fn check_the_towers_counted(
    towers: &[SavedTower],
    built: u64,
    built_by_kind: &[u64; tower::KINDS],
    upgrades: u64,
) -> Result<(), SaveError> {
    if built != towers.len() as u64 {
        return Err(invalid(format!(
            "{built} towers built is not the {} standing",
            towers.len()
        )));
    }
    for kind in tower::ALL {
        let standing = towers.iter().filter(|saved| saved.kind == kind).count() as u64;
        if built_by_kind[kind.index()] != standing {
            return Err(invalid(format!(
                "{} {} towers built is not the {standing} standing",
                built_by_kind[kind.index()],
                kind.label()
            )));
        }
    }
    let stepped = towers
        .iter()
        .filter(|saved| saved.tier == Tier::Upgraded)
        .count() as u64;
    if upgrades != stepped {
        return Err(invalid(format!(
            "{upgrades} upgrades is not the {stepped} towers stepped up"
        )));
    }
    Ok(())
}

/// The creeps on the field, each on `map`'s path, alive, and held — if at
/// all — by a hold some slow tower has.
fn read_creeps(reader: &mut Reader<'_>, map: &Map) -> Result<Vec<SavedCreep>, SaveError> {
    let count = reader.u32()? as usize;
    if count > MAX_CREEPS {
        return Err(invalid(format!(
            "{count} creeps are more than the {MAX_CREEPS} the whole table sends"
        )));
    }
    reader.holds(count, CREEP_BYTES)?;
    let length = map.path().length();
    let mut creeps = Vec::with_capacity(count);
    for index in 0..count {
        let kind_byte = reader.u8()?;
        let along = reader.f64()?;
        let health = reader.u32()?;
        let slow = reader.f64()?;
        let Some(kind) = creep::ALL.get(usize::from(kind_byte)).copied() else {
            return Err(invalid(format!(
                "creep {index} is of kind {kind_byte}, which no creep is"
            )));
        };
        // The exit takes a creep before the path runs out — see
        // `crate::path` — so a creep is somewhere on it.
        if !(along.is_finite() && (0.0..=length).contains(&along)) {
            return Err(invalid(format!(
                "creep {index} is {along} m along a path {length} m long"
            )));
        }
        if health == 0 || health > kind.spec().health {
            return Err(invalid(format!(
                "creep {index} has {health} health, and a {} has 1 to {}",
                kind.spec().label,
                kind.spec().health
            )));
        }
        // Free, or held by exactly some slow tower's factor: the strongest
        // hold wins, so a hold is always one of the table's.
        let held_by_a_tower = TOWERS
            .iter()
            .flatten()
            .any(|spec| spec.slows() && spec.slow_factor.to_bits() == slow.to_bits());
        if slow.to_bits() != 1.0_f64.to_bits() && !held_by_a_tower {
            return Err(invalid(format!(
                "creep {index} is held at {slow} of its speed, which no slow tower holds"
            )));
        }
        creeps.push(SavedCreep {
            kind,
            along,
            health,
            slow,
        });
    }
    Ok(creeps)
}

/// The bolts in the air, each one of the `shots` fired, at one of the
/// `creeps` or at none, carrying what some firing tower's row fires.
fn read_bolts(
    reader: &mut Reader<'_>,
    shots: u64,
    creeps: usize,
) -> Result<Vec<SavedBolt>, SaveError> {
    let count = reader.u32()? as usize;
    if count as u64 > shots {
        return Err(invalid(format!(
            "{count} bolts in the air are more than the {shots} shots fired"
        )));
    }
    reader.holds(count, BOLT_BYTES)?;
    let mut bolts = Vec::with_capacity(count);
    for index in 0..count {
        let id = reader.u64()?;
        let at = reader.point()?;
        let heading = reader.point()?;
        let target = reader.u32()?;
        let damage = reader.u32()?;
        let burst_m = reader.f64()?;
        if id >= shots {
            return Err(invalid(format!(
                "bolt {index} is shot {id}, and {shots} have been fired"
            )));
        }
        // A heading is a unit vector, or zero for a bolt fired from where its
        // target stood — `Bolt::fire`'s `normalize_or_zero`.
        let unit = (heading.length() - 1.0).abs() <= HEADING_SLACK;
        if !on_the_field(at) || !heading.is_finite() || !(unit || heading == DVec3::ZERO) {
            return Err(invalid(format!(
                "bolt {index} is at {at} heading {heading}, which no shot flies"
            )));
        }
        let target = if target == NO_TARGET {
            None
        } else if (target as usize) < creeps {
            Some(target as usize)
        } else {
            return Err(invalid(format!(
                "bolt {index} is aimed at creep {target}, and {creeps} are on the field"
            )));
        };
        let fired_by_a_tower = TOWERS.iter().flatten().any(|spec| {
            spec.fires() && spec.damage == damage && spec.burst_m.to_bits() == burst_m.to_bits()
        });
        if !fired_by_a_tower {
            return Err(invalid(format!(
                "bolt {index} does {damage} damage in a {burst_m} m burst, which no tower fires"
            )));
        }
        bolts.push(SavedBolt {
            id,
            at,
            heading,
            target,
            damage,
            burst_m,
        });
    }
    Ok(bolts)
}

/// How far a saved heading's length may stray from one: a normalised
/// vector's rounding, many times over, and far short of any other length.
const HEADING_SLACK: f64 = 1.0e-9;

/// The bursts still drawn, each raised by one of the `shots` fired, at a
/// radius some bursting row has, at a time the clock at `elapsed` reached.
fn read_bursts(
    reader: &mut Reader<'_>,
    shots: u64,
    elapsed: f64,
) -> Result<Vec<SavedBurst>, SaveError> {
    let count = reader.u32()? as usize;
    if count as u64 > shots {
        return Err(invalid(format!(
            "{count} bursts are more than the {shots} shots fired"
        )));
    }
    reader.holds(count, BURST_BYTES)?;
    let mut bursts = Vec::with_capacity(count);
    for index in 0..count {
        let id = reader.u64()?;
        let at = reader.point()?;
        let radius_m = reader.f64()?;
        let raised_at = reader.f64()?;
        if id >= shots {
            return Err(invalid(format!(
                "burst {index} is shot {id}'s, and {shots} have been fired"
            )));
        }
        let a_rows_burst = TOWERS
            .iter()
            .flatten()
            .any(|spec| spec.bursts() && spec.burst_m.to_bits() == radius_m.to_bits());
        if !on_the_field(at) || !a_rows_burst {
            return Err(invalid(format!(
                "burst {index} is {radius_m} m at {at}, which no shot raises"
            )));
        }
        if !(raised_at.is_finite() && (0.0..=elapsed).contains(&raised_at)) {
            return Err(invalid(format!(
                "burst {index} was raised at {raised_at} s, which the clock at {elapsed} s has \
                 not reached"
            )));
        }
        bursts.push(SavedBurst {
            id,
            at,
            radius_m,
            raised_at,
        });
    }
    Ok(bursts)
}
