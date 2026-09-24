//! Asteroids' balance table: the numbers a designer changes, in a file.
//!
//! `assets/balance.ron` is one RON struct — [`Balance`] — holding every value
//! that decides how this game *plays*. It is read through
//! [`crcbl::assets::AssetSource`], the seam the engine gives content, and
//! the sample plan's third milestone is what asked for it:
//! the first data-driven balance outside a scene.
//!
//! # The rule for what moved and what stayed
//!
//! **A value the art, the protocol or the world's size is baked against stays a
//! constant in `crate::game`. A value that only changes how the game plays
//! lives in the file.** The test of it is what a change would break: turning
//! the fire cooldown down makes the game easier, and nothing else in the
//! process disagrees with it; widening [`crate::game::SHIP_RADIUS`] makes the
//! kill sphere disagree with the sprite `build.rs` baked, which is not a
//! tuning, it is a broken picture.
//!
//! So the file holds the ship's turn rate, thrust and damping; the respawn
//! delay, its ceiling and the room the ship needs to come back into; the
//! starting lives; the bullet's speed, life, cooldown and magazine; the split's
//! child count and the angles it throws them off at; and the first wave's rock
//! count and the cap on it.
//!
//! What stayed in `crate::game`, and why:
//!
//! * [`WORLD_HALF_WIDTH`](crate::game::WORLD_HALF_WIDTH) and
//!   [`WORLD_HALF_HEIGHT`](crate::game::WORLD_HALF_HEIGHT) — the field is 4:3
//!   against the window `crate::app` opens, `crate::gpu` derives the camera
//!   from them and `crate::art` sizes the wrap's copies by them. A field the
//!   viewport does not match hides rocks or shows margin.
//! * [`SHIP_RADIUS`](crate::game::SHIP_RADIUS) and
//!   [`BULLET_RADIUS`](crate::game::BULLET_RADIUS) — `crate::art` draws the
//!   `.crpix` sprites at exactly these, so they are the size of a picture
//!   rather than a difficulty. `MUZZLE_OFFSET` is derived from the pair.
//! * [`FLASH_LIFE`](crate::game::FLASH_LIFE) — a presentation term `crate::art`
//!   fades the hit flash against.
//! * [`RockSize`](crate::game::RockSize)'s radius, speed, score and spin — one
//!   per-size table, whose radius is the size the baked sprite is drawn to and
//!   whose spin is bounded by how many texels that sprite has to show a tumble
//!   in. Half a table in a file and half of it in code is worse than none, so
//!   it moves whole or not at all; it has not moved.
//! * `SHIP_MASS` — one kilogram, so a thrust in newtons reads as an
//!   acceleration in units per second squared. Every number in the file that
//!   touches the ship is priced against that, which makes it a unit and not a
//!   dial.
//! * [`DEFAULT_TICK_HZ`](crate::game::DEFAULT_TICK_HZ) and
//!   [`DEFAULT_SEED`](crate::game::DEFAULT_SEED) — already `--tick-hz` and
//!   `--seed`, and a value with two doors is a value that can be asked for
//!   twice and answered differently.
//! * `COMPATIBILITY` — the protocol. A client and a server that disagree about
//!   it do not hand-shake, which is the opposite of a tuning knob.
//!
//! # Two sources, one loader
//!
//! The committed file is `include_str!`ed and read back through a
//! [`MemorySource`], for the reason `apps/breakout/src/scene.rs` gives about
//! its board: a browser has no filesystem, and a binary that could fail to find
//! its own balance is one whose golden depends on the directory it was run
//! from. `--balance <FILE>` is the run-time door, opened with a [`DirSource`]
//! rooted at the file's directory — the same [`Balance::load`] call either way,
//! because that is what an [`AssetSource`] is for.
//!
//! **The file name has to be a legal asset key**: ASCII letters, digits, `.`,
//! `_` and `-`. That is [`crcbl::assets::DirSource`]'s containment rule and not
//! this game's, and it is what makes a name that loads here also load over HTTP
//! — but it does mean `--balance "my tuning.ron"` is refused as an invalid
//! path rather than read.
//!
//! # What keeps the committed file honest
//!
//! `the_committed_table_is_the_numbers_this_game_shipped_with` asserts every
//! field against the literal it had when it was a `const`. That is the test
//! that catches a drift between the file and the code — and it is why the
//! checked-in goldens under `tests/golden/` did not move when the constants
//! did: the table parses to the same bits the compiler used to fold in.

use std::path::Path;

use crcbl::assets::{AssetSource, DirSource, MemorySource, StorageError};
use crcbl::serde::Deserialize;

/// The key the committed table is filed under, and the name `--balance`
/// defaults to.
const BALANCE: &str = "balance.ron";

/// `assets/balance.ron`, as it is committed.
///
/// `pub(crate)` so `crate::args`' and `crate::game`'s tests can build a table
/// that differs from this one in exactly one value — which is the only way to
/// tell a flag that was read from a flag that was parsed and dropped.
pub(crate) const BUILT_IN_BALANCE_RON: &str = include_str!("../assets/balance.ron");

/// Every number that decides how asteroids plays.
///
/// Flat rather than grouped: a designer opening the file wants one list of
/// names to search, and a nested shape would put two of the four values that
/// tune the gun in different places. See this module's header for the rule that
/// decides what is in here.
///
/// **Not `Eq`.** Most of these are floats, and a float has no total equality;
/// nothing compares two tables for anything but a test's `assert_eq!`, which
/// [`PartialEq`] serves.
///
/// `deny_unknown_fields`, as every one of the engine's own file types is: a
/// misspelled key is a position to go and look at rather than a value silently
/// left at whatever the code had.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(crate = "crcbl::serde", rename = "Balance", deny_unknown_fields)]
pub struct Balance {
    /// How fast the ship turns, in radians per second.
    pub ship_turn_rate: f64,
    /// The engine's thrust, in newtons, against a ship of one kilogram.
    pub ship_thrust: f64,
    /// The coast, as a damping coefficient in kg/s.
    ///
    /// Terminal speed is [`ship_thrust`](Self::ship_thrust) over this, and the
    /// coast's time constant is the ship's mass over it, so the two are one
    /// tuning. There is deliberately no separate speed clamp: a clamp is a
    /// second mechanism that can disagree with this one.
    pub ship_damping: f64,
    /// How long a destroyed ship stays gone, in seconds, before it may return.
    pub respawn_delay: f64,
    /// How long it may wait for a clear centre beyond that, in seconds.
    pub respawn_max_wait: f64,
    /// How much room around the centre must be clear before the ship returns.
    pub respawn_clear_radius: f64,
    /// Lives a game starts with.
    pub starting_lives: u32,
    /// Muzzle speed, in world units per second, added to the ship's own
    /// velocity.
    pub bullet_speed: f64,
    /// How long a bullet lives, in seconds.
    ///
    /// Its reach is this times [`bullet_speed`](Self::bullet_speed), and
    /// `the_reach_of_a_shot_is_less_than_one_lap` asserts that stays under the
    /// height of the wrapping field.
    pub bullet_life: f64,
    /// The gap between shots, in seconds.
    pub fire_cooldown: f64,
    /// How many of the player's bullets may be in the air at once.
    pub max_bullets: usize,
    /// How many children a split produces.
    pub split_children: usize,
    /// The narrowest angle, in radians, between a child's course and its
    /// parent's.
    pub split_angle_min: f64,
    /// How much wider than [`split_angle_min`](Self::split_angle_min) a child's
    /// course may be.
    pub split_angle_range: f64,
    /// How many rocks the first wave puts on the field.
    pub first_wave_rocks: u32,
    /// The ceiling on that count.
    pub max_wave_rocks: u32,
}

impl Balance {
    /// The committed `assets/balance.ron`, parsed.
    ///
    /// # Panics
    ///
    /// If the committed file is not a balance table, naming the line and the
    /// column. It is compiled into this binary, so that is a tree in which
    /// `the_committed_table_is_the_numbers_this_game_shipped_with` is also red
    /// — the panic is what keeps a run from starting on a table nobody could
    /// read.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(&built_in_source(), Path::new(BALANCE))
            .unwrap_or_else(|error| panic!("apps/asteroids/assets/{BALANCE}: {error}"))
    }

    /// The table `key` names, read through `source`.
    ///
    /// # Errors
    ///
    /// [`BalanceError`], which names the key it is about: bytes that are not
    /// there or are not text, or text that is not this struct.
    pub fn load(source: &dyn AssetSource, key: &Path) -> Result<Self, BalanceError> {
        let name = key.display().to_string();
        let bytes = source.read(key).map_err(|source| BalanceError::Read {
            key: name.clone(),
            source,
        })?;
        let text = String::from_utf8(bytes).map_err(|error| BalanceError::Read {
            key: name.clone(),
            source: StorageError::Other(format!("not UTF-8: {error}")),
        })?;
        let table: Self =
            crcbl::ron::from_str(&text).map_err(|error| BalanceError::parse(&name, &error))?;
        table.playable(&name)?;
        Ok(table)
    }

    /// Refuses a table that parses and is not a game.
    ///
    /// **ron stops at the shape, and the shape is not the contract.** Every
    /// field here has a type ron can check and a domain it cannot: a
    /// `max_bullets` of zero is a ship that cannot shoot, a `split_children` of
    /// zero is rocks that vanish instead of splitting, and a `first_wave_rocks`
    /// above `max_wave_rocks` is a ceiling that fires on the opening wave. Each
    /// of those is a table somebody would have to play to diagnose, so the
    /// refusal happens here, naming the field and the bound.
    ///
    /// The float fields are checked for being finite and positive rather than
    /// for a range: what a sensible thrust or damping is belongs to the table,
    /// which is the whole reason it is a file, but a NaN or a zero is not a
    /// tuning choice — a zero `ship_damping` is an infinite terminal speed and a
    /// NaN spreads through every later position.
    ///
    /// # Errors
    ///
    /// [`BalanceError::Range`], naming the field, the bound it broke and the
    /// value it had.
    fn playable(&self, key: &str) -> Result<(), BalanceError> {
        let refuse = |field: &'static str, bound: &'static str, value: String| {
            Err(BalanceError::Range {
                key: key.to_string(),
                field,
                bound,
                value,
            })
        };

        for (field, value) in [
            ("ship_turn_rate", self.ship_turn_rate),
            ("ship_thrust", self.ship_thrust),
            ("ship_damping", self.ship_damping),
            ("respawn_clear_radius", self.respawn_clear_radius),
            ("bullet_speed", self.bullet_speed),
            ("bullet_life", self.bullet_life),
            ("fire_cooldown", self.fire_cooldown),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return refuse(
                    field,
                    "must be a finite number above zero",
                    value.to_string(),
                );
            }
        }

        for (field, value) in [
            ("respawn_delay", self.respawn_delay),
            ("respawn_max_wait", self.respawn_max_wait),
            ("split_angle_min", self.split_angle_min),
            ("split_angle_range", self.split_angle_range),
        ] {
            if !value.is_finite() || value < 0.0 {
                return refuse(
                    field,
                    "must be a finite number, zero or above",
                    value.to_string(),
                );
            }
        }

        for (field, value) in [
            ("starting_lives", self.starting_lives as usize),
            ("max_bullets", self.max_bullets),
            ("split_children", self.split_children),
            ("first_wave_rocks", self.first_wave_rocks as usize),
            ("max_wave_rocks", self.max_wave_rocks as usize),
        ] {
            if value == 0 {
                return refuse(field, "must be at least one", value.to_string());
            }
        }

        if self.first_wave_rocks > self.max_wave_rocks {
            return refuse(
                "first_wave_rocks",
                "must not be above `max_wave_rocks`",
                self.first_wave_rocks.to_string(),
            );
        }
        Ok(())
    }

    /// The table the file at `path` holds, or the message to refuse the run
    /// with.
    ///
    /// Both failures read the same way — the path, then what went wrong with
    /// it — because to a person fixing it "no such file" and "line 3, column 5"
    /// are the same kind of answer about the same argument. The shape
    /// `apps/lantern/src/args.rs`'s `read_stack` and
    /// `apps/breakout/src/scene.rs`'s `read_dir` both have.
    ///
    /// # Errors
    ///
    /// The refusal message, ready to print.
    pub fn read_file(path: &str) -> Result<Self, String> {
        // A `DirSource` is a directory plus a key under it, so the path is
        // split at its last component. Rooting at the parent rather than at the
        // current directory is what lets `--balance` name a file anywhere,
        // while the key stays a single name the source's containment rule can
        // check.
        let file = Path::new(path);
        let Some(name) = file.file_name() else {
            return Err(format!("{path}: not the name of a file"));
        };
        let root = file.parent().unwrap_or_else(|| Path::new(""));
        let source = DirSource::at(root.to_path_buf());
        Self::load(&source, Path::new(name)).map_err(|error| format!("{path}: {error}"))
    }

    /// How many rocks wave `wave` opens with. Wave 0 is the first.
    ///
    /// Saturating rather than wrapping, and that is not defensive arithmetic
    /// about a table [`load`](Self::load) already refused: `wave` is unbounded,
    /// so a long enough run reaches the top of a `u32` whatever the opening
    /// count is. The ceiling below is what the count means anyway.
    #[must_use]
    pub const fn wave_rocks(&self, wave: u32) -> u32 {
        let count = self.first_wave_rocks.saturating_add(wave);
        if count > self.max_wave_rocks {
            self.max_wave_rocks
        } else {
            count
        }
    }
}

/// Why some text is not a [`Balance`].
///
/// The shape `crcbl::scene::scn::ScnError` has, and for its reason: a caller
/// reporting one needs the key, the position and what the parser said, and
/// should not have to depend on ron's types to get them.
#[derive(Debug)]
pub enum BalanceError {
    /// The key would not read, or its bytes are not text.
    Read {
        /// The asset key that failed.
        key: String,
        /// What the source said.
        source: StorageError,
    },
    /// The text is not RON, or is RON that is not this struct — an unknown
    /// field included, since [`Balance`] sets `deny_unknown_fields`.
    Parse {
        /// The asset key the text came from.
        key: String,
        /// The line ron stopped at, 1-based.
        line: usize,
        /// The column of that line, 1-based as ron counts it.
        column: usize,
        /// What ron said, without the position it said it at.
        message: String,
    },
    /// The text is this struct and one of its fields is outside the domain the
    /// game can play.
    Range {
        /// The asset key the text came from.
        key: String,
        /// The field that is out of range.
        field: &'static str,
        /// What that field has to be.
        bound: &'static str,
        /// What it was instead.
        value: String,
    },
}

impl BalanceError {
    /// Reduce ron's `SpannedError` to the key, the position and the message.
    fn parse(key: &str, error: &crcbl::ron::error::SpannedError) -> Self {
        Self::Parse {
            key: key.to_string(),
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        }
    }
}

impl std::fmt::Display for BalanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { key, source } => write!(f, "reading `{key}`: {source}"),
            Self::Parse {
                key,
                line,
                column,
                message,
            } => write!(f, "`{key}` line {line}, column {column}: {message}"),
            Self::Range {
                key,
                field,
                bound,
                value,
            } => write!(f, "`{key}`: `{field}` {bound}, not {value}"),
        }
    }
}

impl std::error::Error for BalanceError {}

/// The committed table, as a source with no filesystem under it.
fn built_in_source() -> MemorySource {
    let mut source = MemorySource::new();
    source
        .insert(Path::new(BALANCE), BUILT_IN_BALANCE_RON.as_bytes().to_vec())
        .expect("`balance.ron` is a legal asset key");
    source
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in the temp directory holding `text`, and the path to it.
    fn written(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("asteroids-balance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp dir is writable");
        let path = dir.join(name);
        std::fs::write(&path, text).expect("the temp dir is writable");
        path.to_str().expect("utf-8").to_string()
    }

    /// **The committed table is the numbers this game shipped with**, field by
    /// field, against the literals they had while they were `const`s in
    /// `crate::game`.
    ///
    /// The literals are written out here rather than read from anywhere,
    /// because that is the whole point: this is the one test that goes red if
    /// the file and the game drift apart. It is also why `tests/golden/` did
    /// not move when the constants did — ron parses `3.4` to the same `f64` the
    /// compiler folded in, so the simulation is bit-for-bit the one the
    /// pictures were taken of.
    #[test]
    fn the_committed_table_is_the_numbers_this_game_shipped_with() {
        let balance = Balance::built_in();
        assert_eq!(balance.ship_turn_rate, 3.4);
        assert_eq!(balance.ship_thrust, 14.0);
        assert_eq!(balance.ship_damping, 1.0);
        assert_eq!(balance.respawn_delay, 1.0);
        assert_eq!(balance.respawn_max_wait, 5.0);
        assert_eq!(balance.respawn_clear_radius, 3.5);
        assert_eq!(balance.starting_lives, 3);
        assert_eq!(balance.bullet_speed, 24.0);
        assert_eq!(balance.bullet_life, 0.8);
        assert_eq!(balance.fire_cooldown, 0.16);
        assert_eq!(balance.max_bullets, 4);
        assert_eq!(balance.split_children, 2);
        assert_eq!(balance.split_angle_min, 0.35);
        assert_eq!(balance.split_angle_range, 0.6);
        assert_eq!(balance.first_wave_rocks, 4);
        assert_eq!(balance.max_wave_rocks, 11);
    }

    /// Waves grow, and then stop growing — the rule
    /// `crate::game::deal_wave` deals by, now that it reads the pair out of the
    /// table.
    #[test]
    fn waves_grow_to_a_ceiling_and_no_further() {
        let balance = Balance::built_in();
        assert_eq!(balance.wave_rocks(0), balance.first_wave_rocks);
        assert_eq!(balance.wave_rocks(1), balance.first_wave_rocks + 1);
        assert!(balance.wave_rocks(3) > balance.wave_rocks(2));
        for wave in 0..500 {
            assert!(balance.wave_rocks(wave) <= balance.max_wave_rocks);
            assert!(
                balance.wave_rocks(wave + 1) >= balance.wave_rocks(wave),
                "wave {wave} shrank"
            );
        }
        assert_eq!(balance.wave_rocks(500), balance.max_wave_rocks);
        // `wave` is a `u32` and nothing bounds it, so the top of the type is a
        // wave number this has to answer rather than overflow on.
        assert_eq!(balance.wave_rocks(u32::MAX), balance.max_wave_rocks);
    }

    /// **A file that is not a balance table is refused by key, line and
    /// column** — the same answer `--scene` gives about a directory that is not
    /// a scene, so a person fixing either is told where to look.
    #[test]
    fn a_file_that_is_not_a_balance_table_is_refused_by_key_and_position() {
        let path = written(
            "typo.ron",
            &BUILT_IN_BALANCE_RON.replace("starting_lives", "startng_lives"),
        );
        let message =
            Balance::read_file(&path).expect_err("a misspelled field is not a balance table");
        assert!(message.contains(&path), "{message}");
        assert!(message.contains("typo.ron"), "{message}");
        assert!(message.contains("line"), "{message}");
        assert!(message.contains("column"), "{message}");
        assert!(
            message.contains("startng_lives"),
            "ron names the field: {message}"
        );
    }

    /// **A table can parse and still not be a game**, and each of those is
    /// refused by the field that is wrong rather than by a crash later.
    ///
    /// One case per rule in [`Balance::playable`]: a count of zero, a float that
    /// must be positive and is not, a float that may be zero and is negative,
    /// and the one rule about a pair of fields. A zero `max_bullets` is the case
    /// that reads most like a game and plays least like one — the ship fires and
    /// nothing leaves it.
    #[test]
    fn a_table_that_parses_and_cannot_be_played_is_refused_by_field() {
        for (field, from, to, bound) in [
            (
                "max_bullets",
                "max_bullets: 4",
                "max_bullets: 0",
                "at least one",
            ),
            (
                "split_children",
                "split_children: 2",
                "split_children: 0",
                "at least one",
            ),
            (
                "ship_damping",
                "ship_damping: 1.0",
                "ship_damping: 0.0",
                "above zero",
            ),
            (
                "respawn_delay",
                "respawn_delay: 1.0",
                "respawn_delay: -1.0",
                "zero or above",
            ),
            (
                "first_wave_rocks",
                "first_wave_rocks: 4",
                "first_wave_rocks: 99",
                "max_wave_rocks",
            ),
        ] {
            let text = BUILT_IN_BALANCE_RON.replace(from, to);
            assert_ne!(
                text, BUILT_IN_BALANCE_RON,
                "the committed table still says `{from}`"
            );
            let path = written(&format!("{field}.ron"), &text);
            let message = Balance::read_file(&path)
                .expect_err("a table outside the domain is not a game this can run");
            assert!(message.contains(field), "the field is named: {message}");
            assert!(message.contains(bound), "the bound is named: {message}");
        }
    }

    /// A file that is not there is refused by the path the caller named, not by
    /// the built-in table's key — a loader looking under the compiled-in name
    /// would report a file nobody asked for.
    #[test]
    fn a_file_that_is_not_there_is_refused_by_the_name_the_caller_gave() {
        let dir = std::env::temp_dir().join(format!("asteroids-missing-{}", std::process::id()));
        let path = dir.join("nowhere.ron");
        let path = path.to_str().expect("utf-8");
        let message = Balance::read_file(path).expect_err("a missing file is not a balance table");
        assert!(message.contains(path), "{message}");
        assert!(message.contains("path not found"), "{message}");
        assert!(
            !message.contains(BALANCE),
            "the key must be the caller's, not the built-in table's: {message}"
        );
    }

    /// A table read off disk is the same value as the compiled-in one when it
    /// holds the same text, which is what makes `--balance` a door onto the
    /// *same* loader rather than a second one.
    #[test]
    fn the_same_text_on_disk_parses_to_the_built_in_table() {
        let path = written("same.ron", BUILT_IN_BALANCE_RON);
        assert_eq!(
            Balance::read_file(&path).expect("the committed text is a balance table"),
            Balance::built_in()
        );
    }
}
