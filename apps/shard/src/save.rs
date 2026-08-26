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
//! `docs/plan/14-persistence.md`'s correction of 2026-07-27 owns the shape — a
//! header, a sector set, and one snapshot per sector. Shard is a single-sector
//! game, so it writes exactly one [`SectorSave`], at [`SectorId::ZERO`], and
//! that is the shape that plan says every MVP sample produces. Nothing in
//! `crcbl-store` changed on this sample's behalf.
//!
//! What is this module's is the **payload** — the bytes inside that one sector —
//! and the platform choice above, which is a fact about where saves live rather
//! than about the container.
//!
//! **The platform arm is not [`Backing::platform`](crcbl::store::record::Backing::platform).**
//! That one answers with the *config* directory, which is where a high score
//! belongs; `docs/plan/14-persistence.md` puts saves in the **data** directory,
//! and it hands out a path rather than the [`StorageSource`] a [`SaveWriter`]
//! writes through. `docs/backlog.md` records that a second consumer of *this*
//! rule would be the moment to hoist it into the engine.
//!
//! # What is in the payload, and what is deliberately not
//!
//! Where the character is standing, what they have left, how many times they
//! have been put down, and how much health each foe has — which is what says
//! who is felled, because [`crate::foe::Foe`] is never alive at zero.
//!
//! **There is no inventory field, and its absence is a decision not yet taken
//! rather than an oversight.** `docs/plan/sample/15-shard.md`'s milestone 1
//! wants loot, rarity and a grid inventory through `docs/plan/34-inventory.md`'s
//! kit; `docs/backlog.md` carries an open question about who forces that kit,
//! and reserving a field here would answer it by accident. The container absorbs
//! an added field the way any versioned format does — `PAYLOAD_VERSION` is
//! what a reader checks — so the cost of adding one later is a version bump and
//! nothing else.
//!
//! Nor is the **clock** restored. [`SaveHeader::playtime_secs`] accumulates
//! across sessions and is read back, but the simulation's own tick counter and
//! `Stage::elapsed` start again at zero, so the torches flicker from the
//! beginning of their cycle and the `[HUD]` heartbeat still opens at `tick: 15`.
//!
//! # The payload's bytes
//!
//! Little-endian throughout, `PAYLOAD_BYTES` of them:
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | 0 | 4 | magic `PAYLOAD_MAGIC` |
//! | 4 | 2 | `PAYLOAD_VERSION` |
//! | 6 | 24 | the capsule **centre**, three `f64` |
//! | 30 | 4 | the character's health, `u32` |
//! | 34 | 8 | how many times they have been put down, `u64` |
//! | 42 | 4 | how many foes follow, `u32` |
//! | 46 | 4 each | each foe's health, `u32`, in [`crate::foe::POSTS`] order |
//!
//! **Every field is decoded through this module's `decode`, which refuses anything
//! it cannot stand behind** rather than clamping it: a wrong length, a foreign magic, an
//! unknown version, a roster that is not this zone's, a health above the
//! archetype's maximum, or a position that is not a finite number inside
//! `POSITION_LIMIT_M`. A refused save reads as *no save* and the zone opens
//! fresh, which is the only safe reading — a `NaN` position would reach
//! [`crcbl::phys::CharacterController::set_position`] and put the character
//! somewhere nothing recovers from.

use std::path::Path;

use crcbl::core::TickId;
use crcbl::math::DVec3;
use crcbl::net::types::SectorId;
use crcbl::store::StorageSource;
use crcbl::store::save::{SaveData, SaveHeader, SaveReader, SaveWriter, SectorSave};

use crate::foe::{self, FOES, HEALTH_MAX};

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
/// per-system version `docs/plan/14-persistence.md` asks the header to carry and
/// it does not. Bump it when a field is added, moved or reinterpreted.
const PAYLOAD_VERSION: u16 = 1;

/// The fixed part of the payload: magic, version, centre, health, downs and the
/// foe count.
const PAYLOAD_FIXED: usize = 4 + 2 + 3 * 8 + 4 + 8 + 4;

/// How long one payload is, in bytes, for this zone's roster.
const PAYLOAD_BYTES: usize = PAYLOAD_FIXED + FOES * 4;

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    /// The centre of the character's capsule, in metres.
    pub centre: DVec3,
    /// What they have left, out of [`crate::foe::HEALTH_MAX`].
    pub health: u32,
    /// How many times they have been put down and returned to the spawn.
    pub downs: u64,
    /// Each foe's health, in [`crate::foe::POSTS`] order. Zero is felled.
    pub foes: [u32; FOES],
    /// Seconds of simulated time across every session so far.
    pub playtime_secs: f64,
    /// The tick the writing session was on. Provenance rather than state: a
    /// resumed session's own counter starts again at zero.
    pub tick: u64,
}

/// The payload bytes for one sector.
fn encode(character: &Character) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PAYLOAD_BYTES);
    bytes.extend_from_slice(PAYLOAD_MAGIC);
    bytes.extend_from_slice(&PAYLOAD_VERSION.to_le_bytes());
    for axis in [character.centre.x, character.centre.y, character.centre.z] {
        bytes.extend_from_slice(&axis.to_le_bytes());
    }
    bytes.extend_from_slice(&character.health.to_le_bytes());
    bytes.extend_from_slice(&character.downs.to_le_bytes());
    // Written rather than implied by the length, so a roster that changed size
    // is refused by name in `decode` instead of being read off a payload that
    // happens to be the right length for a different zone.
    let foes = u32::try_from(FOES).expect("this zone's roster fits a u32");
    bytes.extend_from_slice(&foes.to_le_bytes());
    for health in character.foes {
        bytes.extend_from_slice(&health.to_le_bytes());
    }
    debug_assert_eq!(bytes.len(), PAYLOAD_BYTES, "the payload changed size");
    bytes
}

/// Reads eight bytes at `at` as an `f64`, which the caller has bounds-checked.
fn read_f64(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// Reads four bytes at `at` as a `u32`, which the caller has bounds-checked.
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
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
    if bytes.len() != PAYLOAD_BYTES {
        crcbl::log::warn!(
            "save: {} payload bytes, not {PAYLOAD_BYTES}; starting fresh",
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
    if health == 0 || health > HEALTH_MAX {
        crcbl::log::warn!("save: {health} health is not a live character; starting fresh");
        return None;
    }
    let downs = u64::from_le_bytes(bytes[34..42].try_into().expect("eight bytes"));
    let roster = read_u32(bytes, 42) as usize;
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

    Some(Character {
        centre,
        health,
        downs,
        foes,
        playtime_secs,
        tick: data.header.tick.get(),
    })
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
            downs: 3,
            // A felled husk, a wounded adept, an untouched warden — each
            // inside its own archetype's ceiling, which `decode` checks
            // separately.
            foes: [0, 20, 100],
            playtime_secs: 62.5,
            tick: 3750,
        }
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
        assert_eq!(read.downs, character.downs);
        assert_eq!(read.foes, character.foes);
        assert_eq!(read.playtime_secs, character.playtime_secs);
        assert_eq!(read.tick, character.tick);
    }

    /// **The payload is exactly as long as the table says**, so a field added
    /// without a version bump fails here rather than reading the next field's
    /// bytes.
    #[test]
    fn the_payload_is_the_length_the_format_documents() {
        assert_eq!(encode(&walked()).len(), PAYLOAD_BYTES);
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
        overfull[30..34].copy_from_slice(&(HEALTH_MAX + 1).to_le_bytes());
        assert!(
            decode(&saved(overfull, 1.0, 1)).is_none(),
            "over the maximum"
        );

        let mut roster = good.clone();
        roster[42..46].copy_from_slice(&(FOES as u32 + 1).to_le_bytes());
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
        let last = bytes.len() - 1 - PAYLOAD_BYTES;
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
