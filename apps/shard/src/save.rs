//! Where the character is kept between sessions.
//!
//! | Target | Where |
//! | --- | --- |
//! | native, windowed | `~/.local/share/shard/character.crb` |
//! | native, `--headless` | nowhere, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System |
//!
//! ```text
//!   Stage ──▶ Game::snapshot ──▶ Character ──▶ encode ──▶ SectorSave
//!                                                            │
//!                              SaveWriter ◀──────────────────┘
//!                                  │
//!                                  ▼
//!                            Vault::store ──▶ StorageSource::write
//!
//!   StorageSource::read ──▶ SaveReader ──▶ decode ──▶ Character ──▶ Game::new
//! ```
//!
//! # The container is `crcbl-store`'s, unchanged
//!
//! [`SaveWriter`] and [`SaveReader`] own the magic, the format version, the
//! SHA-256 over everything before it and the atomic write;
//! the persistence rules in `docs/notes/simulation.md` own the shape — a
//! header, a sector set, and one snapshot per sector. Shard is a single-sector
//! game, so it writes exactly one [`SectorSave`], at [`SectorId::ZERO`], and
//! that is the shape those rules say every MVP sample produces. Nothing in
//! `crcbl-store` changed on this sample's behalf.
//!
//! What is this module's is the **payload** — the bytes inside that one sector —
//! and the platform choice above, which is a fact about where saves live rather
//! than about the container.
//!
//! **The platform arm is not [`Backing::platform`](crcbl::store::record::Backing::platform).**
//! That one answers with the *config* directory, which is where a high score
//! belongs; the persistence rules put saves in the **data** directory,
//! and it hands out a path rather than the [`StorageSource`] a [`SaveWriter`]
//! writes through. `docs/backlog.md` records that a second consumer of *this*
//! rule would be the moment to hoist it into the engine.
//!
//! # What is in the payload, and what is deliberately not
//!
//! Where the character is standing, what they have left, **what they have
//! learned**, how many times they have been put down, how much health each foe
//! has — which is what says who is felled, because [`crate::foe::Foe`] is never
//! alive at zero — and **what they are carrying**.
//!
//! The inventory arrived in `PAYLOAD_VERSION` 2 and it is the reason the
//! payload is no longer a fixed length: a grid holds between nothing and one
//! stack per foe. Experience arrived in 3, and it is one field rather than two:
//! **the level is not written**, because it is [`crate::level::level_for`] of
//! the experience beside it and a second copy is a second copy that can
//! disagree. The same argument keeps two other things out of the payload:
//!
//! * **What is on the floor is not written.** An instance is on the floor
//!   exactly when the foe that left it is down and its stack is not in the
//!   grid, so `crate::game`'s `Stage::restore` derives it. A save whose two
//!   halves disagreed would be one that duplicates an item or loses one.
//! * **A stack's rarity is not written.** A tier is
//!   [`crate::loot::rarity_of`] of the seed and the foe that minted the stack,
//!   exactly as the item and the count are, so a resumed grid answers the same
//!   way a floor does without a byte having to agree with a roll.
//!
//! **A payload from an older version reads as no save**, with the reason logged.
//! There is no migration seam — `docs/backlog.md` owes `crcbl-store` one
//! (_The migration seam_) — so a bump orphans the saves written before it,
//! which for a sample with no players is the honest trade and for the engine is
//! not.
//!
//! Nor is the **clock** restored. [`SaveHeader::playtime_secs`] accumulates
//! across sessions and is read back, but the simulation's own tick counter and
//! `Stage::elapsed` start again at zero, so the torches flicker from the
//! beginning of their cycle and the `[HUD]` heartbeat still opens at `tick: 15`.
//!
//! # The payload's bytes
//!
//! Little-endian throughout, `PAYLOAD_HEAD` bytes and then one block per
//! placement:
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | 0 | 4 | magic `PAYLOAD_MAGIC` |
//! | 4 | 2 | `PAYLOAD_VERSION` |
//! | 6 | 24 | the capsule **centre**, three `f64` |
//! | 30 | 4 | the character's health, `u32` |
//! | 34 | 8 | how many times they have been put down, `u64` |
//! | 42 | 8 | what they have learned, `u64` |
//! | 50 | 4 | how many foes follow, `u32` |
//! | 54 | 4 each | each foe's health, `u32`, in [`crate::foe::POSTS`] order |
//! | `PAYLOAD_HEAD` − 4 | 4 | how many placements follow, `u32` |
//! | then | 13 each | one placement, `PLACEMENT_BYTES` |
//!
//! …and one placement is the item's stable key (`u32`, [`Catalog::key`]), its
//! [`StackId`] (`u32`), how many of it (`u16`), the cell it starts at (two
//! `u8`) and how far it is turned (`u8`).
//!
//! **The count is bounded before anything is allocated.** It is read, checked
//! against `PLACEMENTS_MAX` — one stack per foe, because a stack is only ever
//! minted by a foe falling — and only then is the rest of the payload measured
//! against it. Reading a length and reserving it is how a corrupt four-byte
//! field becomes a four-gigabyte allocation.
//!
//! **Every field is decoded through this module's `decode`, which refuses anything
//! it cannot stand behind** rather than clamping it: a wrong length, a foreign magic, an
//! unknown version, a roster that is not this zone's, an experience past what
//! this zone can ever pay out, a health above what the level that experience
//! buys allows, a foe's health above its archetype's maximum, a position that is
//! not a finite number inside
//! `POSITION_LIMIT_M`, a placement count past the roster, an item key no
//! catalogue holds, a stack no foe could have left, two placements claiming one
//! stack, or a cell the item does not fit in. A refused save reads as *no save*
//! and the zone opens fresh, which is the only safe reading — a `NaN` position
//! would reach [`crcbl::phys::CharacterController::set_position`] and put the
//! character somewhere nothing recovers from.
//!
//! **The grid is rebuilt through [`Grid::place`] rather than deserialised**, and
//! that is what keeps a tampered payload from producing an occupancy map that
//! disagrees with its own placements: the kit paints the map itself and refuses
//! an overlap, so the grid a resumed session holds is one that was actually
//! placeable. `docs/backlog.md` carries the serde path's version of this.

use std::path::Path;

use crcbl::core::TickId;
use crcbl::inventory::{Catalog, Cell, Grid, Rotation, Stack, StackId};
use crcbl::math::DVec3;
use crcbl::net::types::SectorId;
use crcbl::store::StorageSource;
use crcbl::store::save::{SaveData, SaveHeader, SaveReader, SaveWriter, SectorSave};

use crate::foe::{self, FOES};
use crate::level;
use crate::loot;

// ---------------------------------------------------------------------------
// Where, and how often
// ---------------------------------------------------------------------------

/// The application directory. `~/.local/share/shard/` on Linux.
///
/// Names the data directory natively. A browser has no directory to name and
/// ignores it — the origin is already the namespace.
const APP: &str = "shard";

/// The file inside it.
///
/// One component and no directory, deliberately: the browser shim reaches OPFS
/// entries by name off the root — `restoreOpfs` in `web/engine/storage.js` —
/// so a key with a `/` in it would name a file that restore never delivers.
const SAVE_FILE: &str = "character.crb";

/// How much **simulated** time passes between autosaves, in seconds.
///
/// Simulated rather than wall-clock on purpose: it is the same clock the tick
/// counter runs on, so a machine drawing at a fifth of real time saves exactly
/// as often per second *of play* as one that keeps up, and nothing that waits
/// for a save is waiting on a frame rate.
///
/// It is also the bound on what a closed tab loses: a browser write returns when
/// it is *queued*, and the page's `pagehide` drain is the last chance it gets, so
/// the honest claim this sample can make is "at most one second of play".
pub const SAVE_PERIOD_S: f64 = 1.0;

/// How many ticks that is at `tick_hz`, and never zero.
///
/// **Counted in ticks rather than compared against an accumulated `f64`**, and
/// the difference is one the browser gate reads: sixty additions of `1.0 / 60.0`
/// come to `0.999…`, so a threshold test fires on tick 61 and every later one
/// drifts further, while `ticks % 60 == 0` lands on 60, 120, 180 exactly. At the
/// default rate that is a whole number of [`crate::game::HEARTBEAT_TICKS`], so
/// the write and the `[HUD]` line reporting it happen on the **same tick** off
/// the same `Stats` — which is what lets a reader compare a resumed session
/// against the state that was actually written rather than against a line near
/// it. `a_save_lands_on_a_heartbeat_at_the_default_rate` is what holds it.
#[must_use]
pub fn save_ticks(tick_hz: u32) -> u64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (SAVE_PERIOD_S * f64::from(tick_hz)).round() as u64;
    ticks.max(1)
}

// ---------------------------------------------------------------------------
// The payload
// ---------------------------------------------------------------------------

/// What a shard payload starts with, so a file from something else is refused
/// before its bytes are read as numbers.
const PAYLOAD_MAGIC: &[u8; 4] = b"SHRD";

/// The payload's own version, inside the container's.
///
/// The container's `format_version` says how the header and the sector table are
/// laid out; this says how *this* sample's sector bytes are, which is the
/// per-system version the persistence rules ask the header to carry and it
/// does not. Bump it when a field is added, moved or reinterpreted.
///
/// **1 → 2** added what the character is carrying. **2 → 3** added what they
/// have learned, which is also what decides the ceiling their health is checked
/// against. A file from an older version reads as no
/// save: there is no migration seam anywhere in `crcbl-store` yet, and inventing
/// one here would be an engine decision taken in a sample.
const PAYLOAD_VERSION: u16 = 3;

/// The fixed part of the payload: magic, version, centre, health, downs,
/// experience and the foe count.
const PAYLOAD_FIXED: usize = 4 + 2 + 3 * 8 + 4 + 8 + 8 + 4;

/// Everything before the first placement: the fixed part, this zone's roster,
/// and the count of placements that follow.
const PAYLOAD_HEAD: usize = PAYLOAD_FIXED + FOES * 4 + 4;

/// One placement: the item's stable key, the stack's id, its count, the cell it
/// starts at and how far it is turned.
const PLACEMENT_BYTES: usize = 4 + 4 + 2 + 1 + 1 + 1;

/// The most placements a payload may claim.
///
/// **One per foe, and it is a bound rather than a guess:** the only thing that
/// mints a stack in this zone is a foe falling, and there is no revive. It is
/// checked before the placements are read, so a corrupt count is refused rather
/// than reserved.
const PLACEMENTS_MAX: usize = FOES;

/// How long a payload carrying `placements` of them is, in bytes.
const fn payload_bytes(placements: usize) -> usize {
    PAYLOAD_HEAD + placements * PLACEMENT_BYTES
}

/// How far from the origin a restored position may be, in metres.
///
/// `crate::zone` is tens of metres across, so anything past this is a number
/// that did not come from a walk. Bounds `set_position` against a file that
/// parsed but is not this game's.
const POSITION_LIMIT_M: f64 = 1.0e4;

/// What one session leaves for the next.
///
/// The **centre** of the capsule rather than the feet, because that is what
/// [`crcbl::phys::CharacterController::set_position`] takes and what
/// `Stage::snapshot` reads — a save that stored the feet would have to add the
/// lift back on, in a second place, from a config it did not store.
#[derive(Clone, Debug, PartialEq)]
pub struct Character {
    /// The centre of the character's capsule, in metres.
    pub centre: DVec3,
    /// What they have left, out of the pool their level allows —
    /// [`crate::level::health_max`].
    pub health: u32,
    /// What they have learned. The **level is not a field**: it is
    /// [`crate::level::level_for`] of this, so a save cannot carry a level that
    /// disagrees with the experience beside it.
    pub experience: u64,
    /// How many times they have been put down and returned to the spawn.
    pub downs: u64,
    /// Each foe's health, in [`crate::foe::POSTS`] order. Zero is felled.
    pub foes: [u32; FOES],
    /// Seconds of simulated time across every session so far.
    pub playtime_secs: f64,
    /// The tick the writing session was on. Provenance rather than state: a
    /// resumed session's own counter starts again at zero.
    pub tick: u64,
    /// What they are carrying: the kit's one container, cells, rotations,
    /// counts and [`StackId`]s.
    ///
    /// This is what costs [`Character`] its `Copy` — a [`Grid`] owns two
    /// `Vec`s — and the clones that fell out of that are all at session
    /// boundaries: a snapshot, a restore, a decode.
    pub grid: Grid,
}

/// The payload bytes for one sector.
fn encode(character: &Character) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(payload_bytes(character.grid.len()));
    bytes.extend_from_slice(PAYLOAD_MAGIC);
    bytes.extend_from_slice(&PAYLOAD_VERSION.to_le_bytes());
    for axis in [character.centre.x, character.centre.y, character.centre.z] {
        bytes.extend_from_slice(&axis.to_le_bytes());
    }
    bytes.extend_from_slice(&character.health.to_le_bytes());
    bytes.extend_from_slice(&character.downs.to_le_bytes());
    bytes.extend_from_slice(&character.experience.to_le_bytes());
    // Written rather than implied by the length, so a roster that changed size
    // is refused by name in `decode` instead of being read off a payload that
    // happens to be the right length for a different zone.
    let foes = u32::try_from(FOES).expect("this zone's roster fits a u32");
    bytes.extend_from_slice(&foes.to_le_bytes());
    for health in character.foes {
        bytes.extend_from_slice(&health.to_le_bytes());
    }

    // The variable half. The count is written before the placements for the
    // reason `decode` reads it that way: a reader must know how many are coming
    // before it commits to anything, and deriving the number from the length
    // would make a truncated file look like a shorter grid.
    let placements: Vec<_> = character.grid.slots().collect();
    let count = u32::try_from(placements.len()).unwrap_or(u32::MAX);
    bytes.extend_from_slice(&count.to_le_bytes());
    for (_, placement) in &placements {
        let stack = placement.stack();
        // The stable key rather than the `ItemId`, which is a position in the
        // file and moves the moment `data/items.ron` is edited. `Catalog::key`
        // is what the kit provides for exactly this.
        let key = loot::catalog().key(stack.item()).unwrap_or_default();
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&stack.id().0.to_le_bytes());
        bytes.extend_from_slice(&stack.count().to_le_bytes());
        bytes.push(placement.at().x);
        bytes.push(placement.at().y);
        bytes.push(rotation_byte(placement.rotation()));
    }
    debug_assert_eq!(
        bytes.len(),
        payload_bytes(placements.len()),
        "the payload changed size",
    );
    bytes
}

/// How a rotation is spelled on disk: quarter turns clockwise, `0` to `3`.
///
/// Written out rather than taken from the enum's discriminant, because a
/// discriminant is not something the kit promises to keep — it is a memory
/// layout, and this is a file format.
const fn rotation_byte(rotation: Rotation) -> u8 {
    match rotation {
        Rotation::Deg0 => 0,
        Rotation::Deg90 => 1,
        Rotation::Deg180 => 2,
        Rotation::Deg270 => 3,
    }
}

/// [`rotation_byte`] undone, or `None` for a byte this build never wrote.
const fn rotation_of(byte: u8) -> Option<Rotation> {
    match byte {
        0 => Some(Rotation::Deg0),
        1 => Some(Rotation::Deg90),
        2 => Some(Rotation::Deg180),
        3 => Some(Rotation::Deg270),
        _ => None,
    }
}

/// Reads eight bytes at `at` as an `f64`, which the caller has bounds-checked.
fn read_f64(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// Reads four bytes at `at` as a `u32`, which the caller has bounds-checked.
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

/// Reads eight bytes at `at` as a `u64`, which the caller has bounds-checked.
fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The character a save holds, or `None` for a save this build will not stand
/// behind.
///
/// Every refusal is logged with what was wrong, because each of them means a
/// player's zone opened fresh and they are entitled to know why.
fn decode(data: &SaveData) -> Option<Character> {
    let [sector] = data.sectors.as_slice() else {
        crcbl::log::warn!(
            "save: {} sector(s) in a single-sector game; starting fresh",
            data.sectors.len(),
        );
        return None;
    };
    if sector.sector_id != SectorId::ZERO {
        crcbl::log::warn!(
            "save: sector {:?} is not this zone's; starting fresh",
            sector.sector_id,
        );
        return None;
    }
    let bytes = sector.snapshot_data.as_slice();
    if bytes.len() < PAYLOAD_HEAD {
        crcbl::log::warn!(
            "save: {} payload bytes, short of the {PAYLOAD_HEAD} every save has; \
             starting fresh",
            bytes.len(),
        );
        return None;
    }
    if &bytes[0..4] != PAYLOAD_MAGIC {
        crcbl::log::warn!("save: the payload is not shard's; starting fresh");
        return None;
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().expect("two bytes"));
    if version != PAYLOAD_VERSION {
        crcbl::log::warn!("save: payload version {version}, not {PAYLOAD_VERSION}; starting fresh",);
        return None;
    }

    let centre = DVec3::new(read_f64(bytes, 6), read_f64(bytes, 14), read_f64(bytes, 22));
    if !centre.is_finite() || centre.abs().max_element() > POSITION_LIMIT_M {
        crcbl::log::warn!("save: {centre:?} is not a place in this zone; starting fresh");
        return None;
    }
    let health = read_u32(bytes, 30);
    let downs = read_u64(bytes, 34);
    // **Read before the health, because it is what the health is measured
    // against.** The zone pays out one kill per post and one find per drop and
    // nothing respawns, so anything past `level::EXPERIENCE_MAX` is a total no
    // session could have reached — and letting one through would let a payload
    // buy a pool this game does not have.
    let experience = read_u64(bytes, 42);
    if experience > level::EXPERIENCE_MAX {
        crcbl::log::warn!(
            "save: {experience} experience, past the {} this zone can pay out; starting fresh",
            level::EXPERIENCE_MAX,
        );
        return None;
    }
    let pool = level::health_max(level::level_for(experience));
    if health == 0 || health > pool {
        crcbl::log::warn!(
            "save: {health} health is not a live character with a {pool} pool; starting fresh",
        );
        return None;
    }
    let roster = read_u32(bytes, 50) as usize;
    if roster != FOES {
        crcbl::log::warn!("save: {roster} foes, not {FOES}; starting fresh");
        return None;
    }

    let mut foes = [0; FOES];
    for (index, health) in foes.iter_mut().enumerate() {
        *health = read_u32(bytes, PAYLOAD_FIXED + index * 4);
        // The archetype's own ceiling, not one shared maximum: a warden holds
        // more than a husk, and a payload claiming a husk has a warden's health
        // is one this zone did not write.
        let ceiling = foe::POSTS[index].kind.health();
        if *health > ceiling {
            crcbl::log::warn!(
                "save: foe {index} at {health} health, over the {}'s {ceiling}; starting fresh",
                foe::POSTS[index].kind.label(),
            );
            return None;
        }
    }

    let playtime_secs = data.header.playtime_secs;
    if !playtime_secs.is_finite() || playtime_secs < 0.0 {
        crcbl::log::warn!("save: {playtime_secs} is not a playtime; starting fresh");
        return None;
    }

    let grid = decode_grid(bytes, &foes)?;

    Some(Character {
        centre,
        health,
        experience,
        downs,
        foes,
        playtime_secs,
        tick: data.header.tick.get(),
        grid,
    })
}

/// The grid the variable half of `bytes` holds, or `None` for one this build
/// will not stand behind.
///
/// `foes` is this zone's roster as the fixed half gave it, and it is what makes
/// a stack checkable: an instance exists because a foe fell, so a placement
/// naming a foe that is still standing is one no session could have written.
///
/// **The count is bounded before the placements are measured**, which is the
/// whole reason this is a function of its own: a corrupt `u32` here is four
/// gigabytes reserved by a `Vec::with_capacity` a line later, and refusing it
/// costs one comparison.
fn decode_grid(bytes: &[u8], foes: &[u32; FOES]) -> Option<Grid> {
    let count = read_u32(bytes, PAYLOAD_HEAD - 4) as usize;
    if count > PLACEMENTS_MAX {
        crcbl::log::warn!(
            "save: {count} placements in a zone that can drop {PLACEMENTS_MAX}; starting fresh",
        );
        return None;
    }
    let expected = payload_bytes(count);
    if bytes.len() != expected {
        crcbl::log::warn!(
            "save: {} payload bytes for {count} placement(s), not {expected}; starting fresh",
            bytes.len(),
        );
        return None;
    }

    let catalog: &Catalog = loot::catalog();
    let mut grid = loot::carried();
    let mut seen: Vec<StackId> = Vec::with_capacity(count);
    for index in 0..count {
        let at = PAYLOAD_HEAD + index * PLACEMENT_BYTES;
        let key = read_u32(bytes, at);
        let Some(item) = catalog.by_key(key) else {
            crcbl::log::warn!("save: no item with key {key:#010x} in this build; starting fresh");
            return None;
        };
        let stack_id = StackId(read_u32(bytes, at + 4));
        // Which foe minted it, and whether that foe is actually down. Together
        // they are what says an instance came from somewhere: the roster bounds
        // the ids, and a felled foe is the only thing that produces one.
        let Some(foe) = loot::foe_of(stack_id, FOES) else {
            crcbl::log::warn!(
                "save: stack {} is not one a {FOES}-foe zone mints; starting fresh",
                stack_id.0,
            );
            return None;
        };
        if foes[foe] != 0 {
            crcbl::log::warn!(
                "save: stack {} came from a {} that is still standing; starting fresh",
                stack_id.0,
                foe::POSTS[foe].kind.label(),
            );
            return None;
        }
        if seen.contains(&stack_id) {
            crcbl::log::warn!(
                "save: stack {} is in the grid twice; starting fresh",
                stack_id.0,
            );
            return None;
        }
        seen.push(stack_id);

        let count_held = u16::from_le_bytes(bytes[at + 8..at + 10].try_into().expect("two bytes"));
        let ceiling = catalog
            .get(item)
            .map_or(0, crcbl::inventory::ItemDef::stack_max);
        if count_held == 0 || count_held > ceiling {
            crcbl::log::warn!(
                "save: a stack of {count_held} is not one to {ceiling} of it; starting fresh",
            );
            return None;
        }
        let Some(rotation) = rotation_of(bytes[at + 12]) else {
            crcbl::log::warn!(
                "save: {} is not a quarter turn; starting fresh",
                bytes[at + 12],
            );
            return None;
        };

        // The placement itself is the kit's to accept or refuse: out of bounds,
        // overlapping something already placed, or an item this grid's filter
        // does not take all read as no save. That is also what re-derives the
        // occupancy map, which is why nothing here writes one.
        let cell = Cell::new(bytes[at + 10], bytes[at + 11]);
        if let Err(error) = grid.place(
            catalog,
            Stack::new(item, stack_id, count_held),
            cell,
            rotation,
        ) {
            crcbl::log::warn!("save: a placement this grid refuses ({error}); starting fresh");
            return None;
        }
    }
    Some(grid)
}

// ---------------------------------------------------------------------------
// Where a save is kept
// ---------------------------------------------------------------------------

/// Where this run's saves go, if anywhere.
///
/// The three arms are the table at the top of this module. `None` is a state a
/// caller *chooses* rather than a failure: a headless run must leave nothing
/// behind, so the test suite and CI cannot write into whoever's data directory.
#[derive(Debug)]
pub enum Vault {
    /// Kept nowhere. A headless run saves in name only.
    None,
    /// A directory on a real filesystem.
    #[cfg(not(target_arch = "wasm32"))]
    Native(crcbl::store::NativeStorage),
    /// The store the page's shim restored the Origin Private File System into.
    #[cfg(target_arch = "wasm32")]
    Browser(std::rc::Rc<crcbl::store::web::OpfsStorage>),
}

impl Vault {
    /// Opens the place this platform keeps saves, or [`Vault::None`].
    ///
    /// A headless run is always `None`. Everything else is the platform's own
    /// answer, and a platform that will not give one — no data directory, no
    /// OPFS store installed — is `None` too, with a warning: it is the ordinary
    /// no-shim case rather than something a caller can do anything about.
    #[must_use]
    pub fn open(headless: bool) -> Self {
        if headless {
            return Self::None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match crcbl::store::NativeStorage::data(APP) {
                Ok(store) => Self::Native(store),
                Err(error) => {
                    crcbl::log::warn!(
                        "save: no data dir ({error}); the character will not persist"
                    );
                    Self::None
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = APP;
            match crcbl::store::web::opfs::installed() {
                Some(store) => Self::Browser(store),
                None => {
                    crcbl::log::warn!(
                        "save: no OPFS store installed; the character will not persist"
                    );
                    Self::None
                }
            }
        }
    }

    /// The backend a [`SaveWriter`] writes through, or `None` for a run that
    /// keeps nothing.
    fn source(&self) -> Option<&dyn StorageSource> {
        match self {
            Self::None => None,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(store) => Some(store),
            #[cfg(target_arch = "wasm32")]
            Self::Browser(store) => Some(&**store),
        }
    }

    /// Where this run's saves go, in the words the debug panel uses.
    #[must_use]
    pub const fn where_it_goes(&self) -> &'static str {
        match self {
            Self::None => "nowhere",
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(_) => "data dir",
            #[cfg(target_arch = "wasm32")]
            Self::Browser(_) => "opfs",
        }
    }

    /// The character a previous session left, if there is one this build will
    /// stand behind.
    ///
    /// Absence is the ordinary first-run case and is silent. Everything else is
    /// logged: a checksum mismatch, a truncated file, a payload this build
    /// cannot read. All of them read as "no save", because the alternative is
    /// opening a zone from numbers nobody verified.
    #[must_use]
    pub fn load(&self) -> Option<Character> {
        let source = self.source()?;
        match SaveReader::open(source, Path::new(SAVE_FILE)) {
            Ok(reader) => decode(reader.data()),
            Err(crcbl::store::StorageError::NotFound(_)) => None,
            Err(error) => {
                crcbl::log::warn!("save: {error}; starting fresh");
                None
            }
        }
    }

    /// Writes `character` out, reporting whether it reached the backend.
    ///
    /// **In a browser "reached the backend" is not "reached the disk."** The
    /// write returns as soon as the record is *queued*; the page's shim performs
    /// it on a later frame and on `pagehide`, and
    /// [`OpfsStats`](crcbl::store::web::OpfsStats)`::queued` is what answers "is
    /// it on the disk yet". `crcbl-store`'s `web` module carries the whole of
    /// which half of the atomic-write guarantee survives a browser.
    pub fn store(&self, character: &Character) -> bool {
        let Some(source) = self.source() else {
            return false;
        };
        let mut writer = SaveWriter::new(SaveHeader {
            tick: TickId::from_raw(character.tick),
            playtime_secs: character.playtime_secs,
        });
        writer.add_sector(SectorSave {
            sector_id: SectorId::ZERO,
            snapshot_data: encode(character),
        });
        match writer.write(source, Path::new(SAVE_FILE)) {
            Ok(()) => true,
            Err(error) => {
                crcbl::log::warn!("save: could not write the character ({error})");
                false
            }
        }
    }
}

/// What the debug panel says about this run's persistence.
///
/// Its own section rather than rows on `crate::game::Stats`, because none of it
/// is the simulation's: where a save goes is the platform's answer and how many
/// have been written is the frame loop's count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaveStats {
    /// Whether this session opened from a save.
    pub resumed: bool,
    /// How many times the character has been written out.
    pub writes: u64,
    /// Seconds of simulated time across every session, including this one.
    pub playtime: f64,
    /// Where the writes go — [`Vault::where_it_goes`].
    pub vault: &'static str,
}

impl Default for SaveStats {
    fn default() -> Self {
        Self {
            resumed: false,
            writes: 0,
            playtime: 0.0,
            vault: "nowhere",
        }
    }
}

impl crcbl::ui::DebugModule for SaveStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("save");
        section.row_str("state", if self.resumed { "resumed" } else { "fresh" });
        section.row("writes", format_args!("{}", self.writes));
        section.row("playtime", format_args!("{:.1} s", self.playtime));
        section.row_str("where", self.vault);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character that is nothing like a fresh zone's.
    fn walked() -> Character {
        Character {
            centre: DVec3::new(-2.5, 0.9, -7.25),
            health: 41,
            // Past the second level's row, so the pool `decode` measures the
            // health against is a *levelled* one rather than the starting pool
            // — which is what makes `a_health_no_level_pays_for_reads_as_no_save`
            // a check of the level rather than of one constant.
            experience: 45,
            downs: 3,
            // A felled husk, a wounded adept, an untouched warden — each
            // inside its own archetype's ceiling, which `decode` checks
            // separately.
            foes: [0, 20, 100],
            playtime_secs: 62.5,
            tick: 3750,
            // …and carrying what the husk left, which is the one drop this
            // roster can have produced.
            grid: carrying(),
        }
    }

    /// A grid holding the husk's drop.
    ///
    /// Foe 0 is the one `walked` has felled, and `decode` refuses a stack from a
    /// foe that is still standing — so this is not an arbitrary grid, it is the
    /// only one that payload could honestly carry.
    fn carrying() -> Grid {
        let mut grid = loot::carried();
        loot::stow(&mut grid, loot::drop_of(loot::DEFAULT_SEED, 0))
            .expect("an empty grid takes one drop");
        grid
    }

    /// Where placement `index` starts in a payload.
    fn placement_at(index: usize) -> usize {
        PAYLOAD_HEAD + index * PLACEMENT_BYTES
    }

    /// A [`SaveData`] holding `bytes` as this zone's one sector.
    fn saved(bytes: Vec<u8>, playtime_secs: f64, tick: u64) -> SaveData {
        SaveData {
            header: SaveHeader {
                tick: TickId::from_raw(tick),
                playtime_secs,
            },
            sectors: vec![SectorSave {
                sector_id: SectorId::ZERO,
                snapshot_data: bytes,
            }],
            checksum_valid: true,
        }
    }

    /// **Every field survives the round trip**, and it is asserted field by
    /// field rather than by comparing two structs the same code built: the
    /// point is that each one is written and read at the offset the table in
    /// the module docs gives it, and a pair of offsets swapped in both
    /// directions would pass a whole-struct comparison.
    #[test]
    fn a_walked_character_comes_back_exactly_as_it_went_in() {
        let character = walked();
        let data = saved(encode(&character), character.playtime_secs, character.tick);
        let read = decode(&data).expect("the payload this build just wrote");
        assert_eq!(read.centre, character.centre);
        assert_eq!(read.health, character.health);
        assert_eq!(read.experience, character.experience);
        assert_eq!(read.downs, character.downs);
        assert_eq!(read.foes, character.foes);
        assert_eq!(read.playtime_secs, character.playtime_secs);
        assert_eq!(read.tick, character.tick);
        assert_eq!(read.grid, character.grid);
        // …and the stack came back as the same instance rather than as another
        // one of the same item, which is what `docs/plan/34-inventory.md` means
        // by ids that survive persistence.
        let (_, placement) = read.grid.slots().next().expect("the one placement");
        assert_eq!(placement.stack().id(), loot::stack_id(0));
        assert_eq!(placement.stack(), loot::drop_of(loot::DEFAULT_SEED, 0));
    }

    /// **The payload is exactly as long as the table says**, so a field added
    /// without a version bump fails here rather than reading the next field's
    /// bytes.
    #[test]
    fn the_payload_is_the_length_the_format_documents() {
        let character = walked();
        assert_eq!(character.grid.len(), 1, "this fixture carries one stack");
        assert_eq!(encode(&character).len(), payload_bytes(1));

        // …and an empty grid is the head and nothing after it, which is what
        // says the variable half is genuinely variable rather than padded.
        let empty = Character {
            grid: loot::carried(),
            ..character
        };
        assert_eq!(encode(&empty).len(), PAYLOAD_HEAD);
        assert_eq!(payload_bytes(0), PAYLOAD_HEAD);
    }

    /// **Every refusal is a refusal.** Each of these is a byte a corrupt or
    /// foreign file could hold, and each must read as "no save" rather than as
    /// a character — the control being the untouched payload above, which is
    /// accepted.
    #[test]
    fn a_payload_this_build_cannot_stand_behind_reads_as_no_save() {
        let good = encode(&walked());
        assert!(
            decode(&saved(good.clone(), 1.0, 1)).is_some(),
            "the control"
        );

        let mut foreign = good.clone();
        foreign[0] = b'X';
        assert!(decode(&saved(foreign, 1.0, 1)).is_none(), "foreign magic");

        let mut future = good.clone();
        future[4] = PAYLOAD_VERSION.to_le_bytes()[0].wrapping_add(1);
        assert!(decode(&saved(future, 1.0, 1)).is_none(), "another version");

        let mut short = good.clone();
        short.pop();
        assert!(decode(&saved(short, 1.0, 1)).is_none(), "one byte short");

        let mut nan = good.clone();
        nan[6..14].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&saved(nan, 1.0, 1)).is_none(), "a NaN position");

        let mut far = good.clone();
        far[6..14].copy_from_slice(&(POSITION_LIMIT_M * 2.0).to_le_bytes());
        assert!(decode(&saved(far, 1.0, 1)).is_none(), "outside the zone");

        let mut dead = good.clone();
        dead[30..34].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&saved(dead, 1.0, 1)).is_none(), "no health left");

        let mut overfull = good.clone();
        let pool = level::health_max(level::level_for(walked().experience));
        overfull[30..34].copy_from_slice(&(pool + 1).to_le_bytes());
        assert!(
            decode(&saved(overfull, 1.0, 1)).is_none(),
            "over the maximum"
        );

        let mut learned = good.clone();
        learned[42..50].copy_from_slice(&(level::EXPERIENCE_MAX + 1).to_le_bytes());
        assert!(
            decode(&saved(learned, 1.0, 1)).is_none(),
            "more experience than this zone pays out",
        );

        let mut roster = good.clone();
        roster[50..54].copy_from_slice(&(FOES as u32 + 1).to_le_bytes());
        assert!(decode(&saved(roster, 1.0, 1)).is_none(), "another roster");

        let mut mighty = good.clone();
        mighty[PAYLOAD_FIXED..PAYLOAD_FIXED + 4]
            .copy_from_slice(&(foe::POSTS[0].kind.health() + 1).to_le_bytes());
        assert!(decode(&saved(mighty, 1.0, 1)).is_none(), "over its ceiling");

        assert!(
            decode(&saved(good.clone(), f64::NAN, 1)).is_none(),
            "a NaN playtime",
        );

        let mut two = saved(good.clone(), 1.0, 1);
        two.sectors.push(two.sectors[0].clone());
        assert!(decode(&two).is_none(), "two sectors in a one-sector game");

        let mut elsewhere = saved(good, 1.0, 1);
        elsewhere.sectors[0].sector_id = SectorId { x: 1, y: 0, z: 0 };
        assert!(decode(&elsewhere).is_none(), "another sector");
    }

    /// **A payload from every version before this one reads as no save.** There
    /// is no migration seam, so the honest answer to a file this build cannot
    /// read is a fresh zone and a logged reason — not a field guessed from a
    /// format that had none.
    ///
    /// The bytes are **this build's own**, with only the version stamped over:
    /// that makes the length, the magic, the roster and every field correct, so
    /// the only thing that can refuse them is the version check itself. A build
    /// that leant on the length would pass this and read a genuinely older file
    /// as a character with whatever the new fields' bytes happened to be.
    #[test]
    fn a_payload_from_an_older_version_reads_as_no_save() {
        for older in 1..PAYLOAD_VERSION {
            let mut old = encode(&walked());
            assert!(
                decode(&saved(old.clone(), 1.0, 1)).is_some(),
                "the control: these bytes are this build's own",
            );
            old[4..6].copy_from_slice(&older.to_le_bytes());
            assert!(
                decode(&saved(old, 1.0, 1)).is_none(),
                "version {older} was read anyway",
            );
        }
    }

    /// **A health no level pays for reads as no save.** The ceiling is the pool
    /// the payload's *own* experience buys, so a file claiming the top level's
    /// health with nothing learned is refused.
    ///
    /// The control is the pair: the same health is accepted the moment the
    /// experience beside it is enough for it, which is what says this refuses a
    /// level rather than refusing a large number.
    #[test]
    fn a_health_no_level_pays_for_reads_as_no_save() {
        let deep = level::health_max(level::MAX_LEVEL);
        let top = level::THRESHOLDS[level::THRESHOLDS.len() - 1];
        assert!(
            deep > level::health_max(1),
            "every level has the same pool, so this test asserts nothing",
        );

        let unearned = encode(&Character {
            health: deep,
            experience: 0,
            ..walked()
        });
        assert!(
            decode(&saved(unearned, 1.0, 1)).is_none(),
            "a first-level character carrying the top level's pool",
        );

        let earned = encode(&Character {
            health: deep,
            experience: top,
            ..walked()
        });
        assert!(
            decode(&saved(earned, 1.0, 1)).is_some(),
            "a top-level character was refused their own pool",
        );
    }

    /// **Every way a payload can lie about what is in the grid is refused**, and
    /// the count is refused *before* it is believed.
    ///
    /// Each of these is a byte a corrupt or hand-made file could hold, and each
    /// would otherwise reach the kit — or an allocator — as a number nobody
    /// checked.
    #[test]
    fn a_grid_this_zone_could_not_have_produced_reads_as_no_save() {
        let good = encode(&walked());
        assert!(
            decode(&saved(good.clone(), 1.0, 1)).is_some(),
            "the control"
        );

        // A count past one stack per foe, refused before the length is measured
        // against it — this is the four-byte field that would otherwise become
        // a reservation.
        let mut many = good.clone();
        many[PAYLOAD_HEAD - 4..PAYLOAD_HEAD].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            decode(&saved(many, 1.0, 1)).is_none(),
            "a count past the roster"
        );

        // A count this zone could produce, on a payload that does not carry
        // that many.
        let mut lying = good.clone();
        lying[PAYLOAD_HEAD - 4..PAYLOAD_HEAD]
            .copy_from_slice(&(PLACEMENTS_MAX as u32).to_le_bytes());
        assert!(decode(&saved(lying, 1.0, 1)).is_none(), "a count that lies");

        let mut unknown = good.clone();
        let at = placement_at(0);
        unknown[at..at + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert!(decode(&saved(unknown, 1.0, 1)).is_none(), "no such item");

        let mut stranger = good.clone();
        stranger[at + 4..at + 8].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            decode(&saved(stranger, 1.0, 1)).is_none(),
            "no foe mints that"
        );

        // A stack from the warden, which this payload says is untouched: an
        // instance no session could have produced, because nothing but a foe
        // falling mints one.
        let mut standing = good.clone();
        standing[at + 4..at + 8].copy_from_slice(&loot::stack_id(2).0.to_le_bytes());
        assert!(
            decode(&saved(standing, 1.0, 1)).is_none(),
            "a drop from a foe that never fell",
        );

        let mut none_of_it = good.clone();
        none_of_it[at + 8..at + 10].copy_from_slice(&0u16.to_le_bytes());
        assert!(
            decode(&saved(none_of_it, 1.0, 1)).is_none(),
            "a stack of none"
        );

        let mut hoard = good.clone();
        hoard[at + 8..at + 10].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(
            decode(&saved(hoard, 1.0, 1)).is_none(),
            "over the stack max"
        );

        let mut off_the_grid = good.clone();
        off_the_grid[at + 10] = crate::loot::GRID_W;
        assert!(
            decode(&saved(off_the_grid, 1.0, 1)).is_none(),
            "off the grid"
        );

        let mut turned = good.clone();
        turned[at + 12] = 4;
        assert!(
            decode(&saved(turned, 1.0, 1)).is_none(),
            "not a quarter turn"
        );
    }

    /// **Two placements cannot claim one stack, and two cannot claim one cell.**
    ///
    /// The duplication check, from the side a save can reach: a payload holding
    /// the same id twice is the file form of an item that was copied, and the
    /// kit's own placement rule is what refuses the overlap.
    #[test]
    fn a_payload_cannot_hold_one_stack_twice() {
        // Two foes down, and the character carrying both drops.
        let mut grid = loot::carried();
        loot::stow(&mut grid, loot::drop_of(loot::DEFAULT_SEED, 0)).expect("the first fits");
        loot::stow(&mut grid, loot::drop_of(loot::DEFAULT_SEED, 1)).expect("and the second");
        let character = Character {
            foes: [0, 0, 100],
            grid,
            ..walked()
        };
        let good = encode(&character);
        assert!(
            decode(&saved(good.clone(), 1.0, 1)).is_some(),
            "the control"
        );

        // The second placement re-labelled with the first's id: same item, same
        // cell as itself, but now two placements claiming one instance.
        let mut twice = good.clone();
        let first = placement_at(0);
        let second = placement_at(1);
        let id: [u8; 4] = twice[first + 4..first + 8].try_into().expect("four bytes");
        twice[second + 4..second + 8].copy_from_slice(&id);
        assert!(
            decode(&saved(twice, 1.0, 1)).is_none(),
            "one stack was in the grid twice",
        );

        // …and the second placement moved onto the first's cell, which the kit
        // refuses rather than painting over.
        let mut stacked = good;
        stacked[second + 10] = stacked[first + 10];
        stacked[second + 11] = stacked[first + 11];
        assert!(
            decode(&saved(stacked, 1.0, 1)).is_none(),
            "two items were placed in one cell",
        );
    }

    /// **A save lands on a heartbeat**, at the rate every run that is not asked
    /// for another one uses.
    ///
    /// The property `web/tools/browser-e2e.mjs`'s save block rests on: the write
    /// and the line that reports it are the same tick's, so the `[HUD]` beat
    /// carrying a raised `saves` carries the state that was written. Nothing
    /// enforces it but this test — change either constant and it goes red rather
    /// than the gate going quietly approximate.
    #[test]
    fn a_save_lands_on_a_heartbeat_at_the_default_rate() {
        let ticks = save_ticks(crate::game::DEFAULT_TICK_HZ);
        assert_eq!(ticks, u64::from(crate::game::DEFAULT_TICK_HZ));
        assert_eq!(
            ticks % crate::game::HEARTBEAT_TICKS,
            0,
            "{ticks} ticks between saves is not a whole number of the \
             {} between heartbeats",
            crate::game::HEARTBEAT_TICKS,
        );
        assert_eq!(
            save_ticks(0),
            1,
            "a rate that rounds to nothing still saves"
        );
    }

    /// **A headless run keeps nothing and writes nothing.** The rule that lets
    /// the test suite and CI run this sample without touching a real data
    /// directory, and it is this module's rather than `crcbl-store`'s — a
    /// [`SaveWriter`] writes through whatever backend it is handed.
    #[test]
    fn a_headless_run_has_nowhere_to_save_and_leaves_nothing() {
        let vault = Vault::open(true);
        assert_eq!(vault.where_it_goes(), "nowhere");
        assert!(vault.load().is_none(), "a headless run found a save");
        assert!(!vault.store(&walked()), "a headless run wrote one");
        assert!(vault.load().is_none(), "and then read it back");
    }

    /// **A character written out comes back on the next open**, through the
    /// real container and a real directory. The one check that says the writer,
    /// the reader, the checksum and the payload agree end to end rather than
    /// pairwise.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_character_written_to_a_directory_is_there_on_the_next_open() {
        let dir = std::env::temp_dir().join("crcbl-shard-save-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory is writable");

        let vault = Vault::Native(crcbl::store::NativeStorage::at(dir.clone()));
        assert!(vault.load().is_none(), "nothing has been written yet");
        assert!(vault.store(&walked()), "the write was refused");

        let reopened = Vault::Native(crcbl::store::NativeStorage::at(dir.clone()));
        assert_eq!(reopened.load(), Some(walked()), "it did not reach the disk");

        // …and a file whose bytes were tampered with is refused by the
        // container's own checksum, which is the half `decode` cannot see.
        let file = dir.join(SAVE_FILE);
        let mut bytes = std::fs::read(&file).expect("the save this test wrote");
        let last = bytes.len() - 1 - payload_bytes(walked().grid.len());
        bytes[last] ^= 0xFF;
        std::fs::write(&file, &bytes).expect("the scratch directory is writable");
        assert!(
            Vault::Native(crcbl::store::NativeStorage::at(dir.clone()))
                .load()
                .is_none(),
            "a corrupted save was read as a character",
        );

        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }
}
