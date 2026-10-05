//! Settings — layered TOML configuration with typed access.
//!
//! # Layer model
//!
//! Settings resolve from a stack of layers, ordered lowest to highest priority:
//!
//! 1. **Engine defaults** — compiled-in defaults shipped with the engine.
//! 2. **Game defaults** — compiled-in defaults shipped with the game.
//! 3. **User settings file** — `settings.toml` on disk, storing only values the
//!    user has explicitly changed (diff vs defaults = small files).
//! 4. **Command-line overrides** — `--set key=value`, for one run.
//!
//! Reading a key searches layers from highest to lowest and returns the first
//! hit. Writing a value stores it in the user settings layer, which can be
//! persisted atomically via [`SettingsStack::save`].
//!
//! # The two layers around the player's file are the launch's
//!
//! [`LaunchLayers`] holds the game's defaults and the command line's
//! overrides, and [`SettingsStack::layered`] puts them below and above the
//! user file. Both are facts about one run of one binary rather than about the
//! machine, which is why neither is ever written: [`SettingsStack::save`]
//! persists the user layer alone, so a `--set` lasts exactly as long as the
//! process that was given it, and a game's default stays out of the file until
//! the player changes it — the "small files" rule above.
//!
//! An override's value is read by the grammar `settings.toml` is: `true`,
//! `0.5`, `"text"`. A bare word is refused rather than taken as text, because
//! the same word in the file would not parse either, and a flag that accepted
//! what the file refuses would be a second dialect.
//!
//! # Namespace convention
//!
//! Keys are namespaced with dot notation:
//!
//! ```toml
//! [engine.video]
//! vsync = true
//! resolution = [1920, 1080]
//!
//! [engine.audio]
//! master_volume = 0.8
//!
//! [game]
//! difficulty = "normal"
//! ```
//!
//! The top-level section names (`engine`, `game`) are not enforced by the API
//! — they are a convention for clarity. Use [`SettingsStack::get`] with dotted
//! keys and [`SettingsStack::get_section`] with a section name to retrieve a
//! whole namespace as a typed struct.

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{StorageError, StorageSource};

/// The file the user settings layer lives in, inside an application's own
/// storage.
///
/// Named here rather than at each call site because it is the one thing a
/// player, a settings screen and a start-up have to agree on: a second spelling
/// is a game that reads one file and writes another.
pub const SETTINGS_FILE: &str = "settings.toml";

// ── Layer types ─────────────────────────────────────────────────────────────

/// A single layer in the settings stack.
#[derive(Debug)]
pub enum SettingsLayer {
    /// Engine default settings (compiled-in TOML).
    EngineDefaults(toml::Table),
    /// Game default settings (compiled-in TOML).
    GameDefaults(toml::Table),
    /// User settings file on disk, loaded from a [`StorageSource`].
    UserFile(StorageSettingsFile),
    /// Command-line overrides: one run's `--set key=value` pairs, see
    /// [`LaunchLayers::set`].
    CliOverrides(toml::Table),
}

/// Which layer of a [`SettingsStack`] a value came from.
///
/// What `crcbl settings list` and the console's `dump` print beside each key,
/// so a value nobody remembers choosing can be traced to whoever chose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerKind {
    /// [`SettingsLayer::EngineDefaults`], or the engine's own reading of a key
    /// no layer holds.
    Engine,
    /// [`SettingsLayer::GameDefaults`].
    Game,
    /// [`SettingsLayer::UserFile`]: the player's [`SETTINGS_FILE`].
    User,
    /// [`SettingsLayer::CliOverrides`].
    CommandLine,
}

impl LayerKind {
    /// The word this layer is reported under, in `--json` and to a person.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Game => "game",
            Self::User => "user",
            Self::CommandLine => "cli",
        }
    }
}

impl SettingsLayer {
    /// Which kind of layer this is.
    #[must_use]
    pub const fn kind(&self) -> LayerKind {
        match self {
            Self::EngineDefaults(_) => LayerKind::Engine,
            Self::GameDefaults(_) => LayerKind::Game,
            Self::UserFile(_) => LayerKind::User,
            Self::CliOverrides(_) => LayerKind::CommandLine,
        }
    }

    /// The inner TOML table, if this layer has one.
    fn table(&self) -> Option<&toml::Table> {
        match self {
            SettingsLayer::EngineDefaults(t)
            | SettingsLayer::GameDefaults(t)
            | SettingsLayer::CliOverrides(t) => Some(t),
            SettingsLayer::UserFile(f) => Some(f.table()),
        }
    }

    /// Mutable table access for the user settings layer.
    fn table_mut(&mut self) -> Option<&mut toml::Table> {
        match self {
            SettingsLayer::UserFile(f) => Some(f.table_mut()),
            _ => None,
        }
    }

    /// Persist this layer to storage, if applicable.
    fn save(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        match self {
            SettingsLayer::UserFile(f) => f.save(storage, path),
            _ => Ok(()),
        }
    }
}

// ── User settings file ─────────────────────────────────────────────────────

/// A user settings file backed by a [`StorageSource`].
///
/// Loads on construction (or starts empty if no file exists). Tracks whether
/// values have been changed since the last load so [`save`](Self::save) can
/// skip unnecessary writes.
#[derive(Debug)]
pub struct StorageSettingsFile {
    data: toml::Table,
    dirty: bool,
}

impl StorageSettingsFile {
    /// A layer holding nothing: what [`load`](Self::load) produces for a path
    /// with no file at it.
    ///
    /// What [`SettingsStack::platform`] falls back to when the file could not
    /// be read at all — a layer that answers `None` for every key, so the
    /// defaults below it decide, rather than no user layer at all, which would
    /// make [`SettingsStack::set`] fail for the rest of the session.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: toml::Table::new(),
            dirty: false,
        }
    }

    /// Load settings from `path` in `storage`. If the file does not exist,
    /// returns an empty (defaults-only) table.
    ///
    /// # Errors
    ///
    /// [`StorageError::Other`] if the file is not UTF-8 or is not valid TOML,
    /// and whatever the backend reports for a read that is neither a hit nor a
    /// [`StorageError::NotFound`]. TOML *is* UTF-8 by definition, so the two
    /// belong together: decoding lossily instead would feed replacement
    /// characters to the parser, and a settings file that came back with a
    /// truncated key and no complaint is one the next save writes back.
    pub fn load(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        let data = match storage.read(path) {
            Ok(bytes) => {
                let text = str::from_utf8(&bytes).map_err(|e| {
                    StorageError::Other(format!(
                        "settings file {} is not UTF-8: invalid byte at offset {}",
                        path.display(),
                        e.valid_up_to()
                    ))
                })?;
                toml::from_str(text)
                    .map_err(|e| StorageError::Other(format!("invalid settings TOML: {e}")))?
            }
            Err(StorageError::NotFound(_)) => toml::Table::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { data, dirty: false })
    }

    /// The inner table.
    pub fn table(&self) -> &toml::Table {
        &self.data
    }

    /// Mutable access to the inner table.
    pub fn table_mut(&mut self) -> &mut toml::Table {
        self.dirty = true;
        &mut self.data
    }

    /// Persist to `path` in `storage` atomically, but only if the data has
    /// changed since the last load or save.
    pub fn save(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        if !self.dirty {
            return Ok(());
        }
        let toml_string = toml::to_string_pretty(&self.data)
            .map_err(|e| StorageError::Other(format!("settings serialization: {e}")))?;
        storage.write(path, toml_string.as_bytes())?;
        Ok(())
    }
}

// ── Launch layers ──────────────────────────────────────────────────────────

/// The layers a run is launched with: the game's compiled-in defaults, which
/// sit below the player's file, and the command line's overrides, which sit
/// above it.
///
/// Held apart from any [`SettingsStack`] because a run opens its settings more
/// than once — the GPU context reads `[engine.video]`, the console reads the
/// whole file, a settings screen opens its own — and every one of those has to
/// be the same four layers. [`SettingsStack::layered`] is how each of them gets
/// these two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchLayers {
    game: Option<toml::Table>,
    cli: toml::Table,
}

/// Why a `--set` was refused.
///
/// The message names the key wherever there is one, so a run started with a
/// dozen overrides is told which of them was wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideError(String);

impl std::fmt::Display for OverrideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OverrideError {}

impl LaunchLayers {
    /// No game defaults and no overrides: a stack layered over this is the
    /// player's file alone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// These layers with `text` — a TOML document — as the game's defaults.
    ///
    /// # Errors
    ///
    /// [`StorageError::Other`] if `text` is not TOML. A game's defaults are
    /// compiled into it, so this is a build that ships a broken table, and the
    /// run is refused rather than started without the values the game assumes.
    pub fn with_game_defaults(mut self, text: &str) -> Result<Self, StorageError> {
        let table = toml::from_str(text)
            .map_err(|e| StorageError::Other(format!("invalid game default settings: {e}")))?;
        self.game = Some(table);
        Ok(self)
    }

    /// Adds one `KEY=VALUE` override, answering the key it set.
    ///
    /// The key is dotted, as in [`SettingsStack::get`]; the value is a TOML
    /// value, read as `settings.toml` reads one — see the module docs. A key
    /// given twice keeps the later value, so a wrapper script can append an
    /// override to a command line that already has one.
    ///
    /// # Errors
    ///
    /// [`OverrideError`] when `arg` has no `=`, when the key is not a dotted
    /// settings key, when the value is not a TOML value, or when the key needs
    /// a table where an earlier override put a value (`a=1` then `a.b=2`).
    pub fn set(&mut self, arg: &str) -> Result<String, OverrideError> {
        let (key, raw) = arg
            .split_once('=')
            .ok_or_else(|| OverrideError(format!("`{arg}` is not <KEY>=<VALUE>")))?;
        if key.is_empty() {
            return Err(OverrideError(format!("`{arg}` names no key")));
        }
        if !key.split('.').all(is_bare_key) {
            return Err(OverrideError(format!(
                "`{key}` is not a settings key: dotted names of letters, digits, `_` and `-`"
            )));
        }
        let value: toml::Value = raw.parse().map_err(|_| {
            OverrideError(format!(
                "{key}: `{raw}` is not a TOML value — quote text, as in {key}=\"{raw}\""
            ))
        })?;
        set_dotted(&mut self.cli, key, value).map_err(|e| OverrideError(format!("{key}: {e}")))?;
        Ok(key.to_owned())
    }

    /// Whether there is nothing here: no game defaults and no override.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.game.is_none() && self.cli.is_empty()
    }

    /// Whether the game supplied defaults at all.
    ///
    /// The half [`game_defines`](Self::game_defines) needs beside it: a game
    /// with no defaults table has said nothing about which keys are its own,
    /// so a key it does not define is not evidence of a typo.
    #[must_use]
    pub const fn has_game_defaults(&self) -> bool {
        self.game.is_some()
    }

    /// Whether the game's defaults hold `key`.
    #[must_use]
    pub fn game_defines(&self, key: &str) -> bool {
        self.game
            .as_ref()
            .is_some_and(|table| get_dotted(table, key).is_some())
    }

    /// Every key an override sets, dotted, in key order.
    #[must_use]
    pub fn overridden_keys(&self) -> Vec<String> {
        leaves(&self.cli).into_iter().map(|(key, _)| key).collect()
    }
}

/// Whether `part` is one segment of a dotted key: a TOML bare key, which is
/// what a section header and a `key = value` line in `settings.toml` spell.
fn is_bare_key(part: &str) -> bool {
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ── Stack ──────────────────────────────────────────────────────────────────

/// A stack of settings layers, resolved highest-to-lowest priority.
///
/// # Example
///
/// ```ignore
/// use crcbl_store::settings::*;
///
/// let mut stack = SettingsStack::new();
/// stack.add(SettingsLayer::EngineDefaults(defaults_table));
/// stack.add(SettingsLayer::UserFile(StorageSettingsFile::load(&storage, "settings.toml")?));
///
/// let vsync: bool = stack.get("engine.video.vsync").unwrap_or(true);
/// ```
#[derive(Debug)]
pub struct SettingsStack {
    layers: Vec<SettingsLayer>,
}

impl Default for SettingsStack {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsStack {
    /// Create an empty settings stack.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// The stack a game starts with: the player's own [`SETTINGS_FILE`] where
    /// this platform keeps it, and no other layer.
    ///
    /// Natively that is [`SETTINGS_FILE`] under
    /// [`NativeStorage::config_root`](crate::NativeStorage::config_root) —
    /// **read without creating the directory**, so a start-up that finds no
    /// settings file leaves the machine as it found it. In a browser it is the
    /// Origin Private File System store the shim installed, which is the same
    /// rule [`Backing::platform`](crate::record::Backing::platform) applies to
    /// a record, in the same two arms.
    ///
    /// # Nothing here is a start-up failure
    ///
    /// A player who has never opened a settings screen has no file; a platform
    /// may name no settings directory; a page may have no store installed yet;
    /// and a hand-edited file can be unreadable or not be TOML. Every one of
    /// those produces an **empty user layer** — so every key reads as absent
    /// and whatever the caller layers underneath decides — and the last two
    /// also log, because they are a machine's problem rather than a new
    /// player's.
    ///
    /// # One layer, and it is the top one
    ///
    /// [`add`](Self::add) appends, and a later layer wins, so anything added to
    /// the stack this returns would sit **above** the player's file and beat
    /// it. A caller that wants engine or game defaults underneath assembles the
    /// stack itself instead — [`new`](Self::new), the default tables, then
    /// `SettingsLayer::UserFile(StorageSettingsFile::load(..))` last — or asks
    /// [`platform_with`](Self::platform_with), which does exactly that.
    #[must_use]
    pub fn platform(app_name: &str) -> Self {
        Self::platform_with(app_name, &LaunchLayers::new())
    }

    /// [`platform`](Self::platform)'s file, [`layered`](Self::layered) between
    /// `launch`'s game defaults and its overrides.
    #[must_use]
    pub fn platform_with(app_name: &str, launch: &LaunchLayers) -> Self {
        let file = Self::with_platform_storage(app_name, Self::user_file).unwrap_or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            crcbl_core::log::warn!(
                "settings: this platform names no config directory; \
                 {SETTINGS_FILE} will not be read"
            );
            #[cfg(target_arch = "wasm32")]
            crcbl_core::log::info!(
                "settings: no OPFS store installed; {SETTINGS_FILE} will not be read"
            );
            StorageSettingsFile::empty()
        });
        Self::layered(launch, file)
    }

    /// The player's `file` between `launch`'s two layers: the game's defaults
    /// below it and the command line's overrides above it.
    ///
    /// A layer `launch` does not have is not added, so a stack layered over
    /// [`LaunchLayers::new`] is the one-layer stack
    /// [`from_storage`](Self::from_storage) has always been.
    ///
    /// **Neither launch layer is ever saved.** [`set`](Self::set) writes the
    /// user layer and [`save`](Self::save) persists it alone, so an override is
    /// read for this run and gone at the next, and a game default reaches the
    /// file only once the player changes the key.
    #[must_use]
    pub fn layered(launch: &LaunchLayers, file: StorageSettingsFile) -> Self {
        let mut stack = Self::new();
        if let Some(game) = &launch.game {
            stack.add(SettingsLayer::GameDefaults(game.clone()));
        }
        stack.add(SettingsLayer::UserFile(file));
        if !launch.cli.is_empty() {
            stack.add(SettingsLayer::CliOverrides(launch.cli.clone()));
        }
        stack
    }

    /// Run `f` against the storage [`platform`](Self::platform) reads out of,
    /// or answer `None` where that platform has none.
    ///
    /// **A borrow rather than a value, and that is the whole reason this shape
    /// exists.** The two backends do not have one owned type between them: the
    /// native arm builds a [`NativeStorage`](crate::NativeStorage) on the spot
    /// and the browser arm hands out an `Rc` to the store the shim installed,
    /// which is not `Send` and so cannot be a `Box<dyn StorageSource>`. Both
    /// can be *lent* for the length of a call, and a settings screen only ever
    /// needs it for the length of one.
    ///
    /// # This is what makes a setting writable
    ///
    /// [`platform`](Self::platform) resolves this storage, reads
    /// [`SETTINGS_FILE`] out of it and drops it, so a caller holding the stack
    /// it returned had nothing to hand [`save`](Self::save) — which is why no
    /// application in this workspace could write a setting until this existed.
    /// [`save_platform`](Self::save_platform) is the pairing that closes it.
    pub fn with_platform_storage<R>(
        app_name: &str,
        f: impl FnOnce(&dyn StorageSource) -> R,
    ) -> Option<R> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root = crate::NativeStorage::config_root(app_name)?;
            Some(f(&crate::NativeStorage::at(root)))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = app_name;
            Some(f(crate::web::opfs::installed()?.as_ref()))
        }
    }

    /// Write the user layer back to the file [`platform`](Self::platform) read
    /// it from.
    ///
    /// The mirror of [`platform`](Self::platform), and here rather than at the
    /// call site so that [`SETTINGS_FILE`] keeps the one spelling its own
    /// documentation asks for: a settings screen that named the file itself
    /// could save to a path the next start-up does not read.
    ///
    /// # A platform with nowhere to write is an error, where one with nothing
    /// to read is not
    ///
    /// [`platform`](Self::platform) answers an empty stack for a machine that
    /// names no config directory, because a first run has no file and that is
    /// not a failure. The opposite direction is: a player who moved a slider
    /// and pressed Save has been told their choice was kept, so a save that
    /// went nowhere has to reach them rather than a log line.
    pub fn save_platform(&self, app_name: &str) -> Result<(), StorageError> {
        Self::with_platform_storage(app_name, |storage| {
            self.save(storage, Path::new(SETTINGS_FILE))
        })
        .unwrap_or_else(|| {
            Err(StorageError::Other(format!(
                "no settings directory for `{app_name}` on this platform, so \
                 {SETTINGS_FILE} cannot be written"
            )))
        })
    }

    /// The same one-layer stack over a [`StorageSource`] the caller already
    /// has.
    ///
    /// What [`platform`](Self::platform) is once the platform question has been
    /// answered, and what a test — or a game that keeps its settings somewhere
    /// of its own — asks for directly. One layer, and the top one, on
    /// [`platform`](Self::platform)'s terms.
    #[must_use]
    pub fn from_storage(storage: &dyn StorageSource) -> Self {
        Self::from_storage_with(storage, &LaunchLayers::new())
    }

    /// [`from_storage`](Self::from_storage)'s file, [`layered`](Self::layered)
    /// between `launch`'s game defaults and its overrides.
    #[must_use]
    pub fn from_storage_with(storage: &dyn StorageSource, launch: &LaunchLayers) -> Self {
        Self::layered(launch, Self::user_file(storage))
    }

    /// [`SETTINGS_FILE`] out of `storage`, or an empty layer and a log line.
    fn user_file(storage: &dyn StorageSource) -> StorageSettingsFile {
        match StorageSettingsFile::load(storage, Path::new(SETTINGS_FILE)) {
            Ok(file) => file,
            Err(error) => {
                crcbl_core::log::warn!(
                    "settings: {SETTINGS_FILE} could not be read ({error}); \
                     starting from defaults"
                );
                StorageSettingsFile::empty()
            }
        }
    }

    /// Add a layer. Layers added later have higher priority.
    pub fn add(&mut self, layer: SettingsLayer) {
        self.layers.push(layer);
    }

    /// The number of layers in the stack.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether the stack has no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    // ── Reading ─────────────────────────────────────────────────────────

    /// Get a typed value by dotted key path, searching layers from highest
    /// priority to lowest. `None` if no layer defines the key.
    ///
    /// Key path examples: `"engine.video.vsync"`, `"game.difficulty"`.
    ///
    /// # A value of the wrong type is skipped, not fatal, and not the end
    ///
    /// A layer whose value will not deserialize into `T` — `vsync = "on"` in a
    /// hand-edited file — is passed over and the search **continues
    /// downward**, so the layer beneath still answers. That is the useful
    /// direction for a user file sitting over a game's defaults: a typo costs
    /// the player their override rather than the key. It also means a `None`
    /// here does not tell a caller that the key is absent; use
    /// [`contains`](Self::contains) for that, which is the pair a caller needs
    /// to warn about a value it could not read.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.find(key, |_: &T| true).map(|(value, _)| value)
    }

    /// [`get`](Self::get), passing over a value `accept` refuses as well as one
    /// of the wrong type, and naming the layer that answered.
    ///
    /// The same downward search with the same reason for it: a player's
    /// `grid = 0` is an override the game cannot use, and the layer beneath —
    /// the game's own default — is the right answer for it, where stopping at
    /// the top would leave the caller holding a zero. A caller that wants to
    /// tell the player their line did nothing compares the layer answered here
    /// with [`layer_of`](Self::layer_of).
    pub fn find<T: DeserializeOwned>(
        &self,
        key: &str,
        accept: impl Fn(&T) -> bool,
    ) -> Option<(T, LayerKind)> {
        for layer in self.layers.iter().rev() {
            // A layer without a table is skipped, not treated as the end of
            // the search.
            let Some(table) = layer.table() else { continue };
            if let Some(value) = get_dotted(table, key)
                && let Ok(v) = value.clone().try_into::<T>()
                && accept(&v)
            {
                return Some((v, layer.kind()));
            }
        }
        None
    }

    /// The highest-priority layer holding `key` at all, whatever type it
    /// holds — the layer [`contains`](Self::contains) found it in.
    #[must_use]
    pub fn layer_of(&self, key: &str) -> Option<LayerKind> {
        self.layers.iter().rev().find_map(|layer| {
            layer
                .table()
                .and_then(|table| get_dotted(table, key))
                .map(|_| layer.kind())
        })
    }

    /// Every key the stack holds a value for, with the value that wins and the
    /// layer it came from, in key order.
    ///
    /// A key is a dotted path to anything but a table — a list is one value,
    /// as it is one line in the file. What `crcbl settings list` and the
    /// console's `dump` print, so the two cannot disagree about which layer
    /// answered.
    #[must_use]
    pub fn entries(&self) -> Vec<SettingsEntry> {
        leaves(&self.merge_all())
            .into_iter()
            .map(|(key, value)| SettingsEntry {
                layer: self
                    .layer_of(&key)
                    .expect("a key in the merged view is in some layer"),
                key,
                value,
            })
            .collect()
    }

    /// Whether any layer defines `key` at all, whatever type it holds.
    ///
    /// The half [`get`](Self::get) cannot answer: a `None` from it means "no
    /// layer had a value of this type", which a key that is missing and a key
    /// holding `"off"` where a `bool` belongs produce alike. A caller that
    /// wants to tell a player their settings file has a line in it that does
    /// nothing needs both — and it is here rather than at the call site
    /// because the merged tables are private and a caller outside this crate
    /// cannot reach a `toml::Value` without naming the parser this crate
    /// exists to keep to itself.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.layers
            .iter()
            .filter_map(SettingsLayer::table)
            .any(|table| get_dotted(table, key).is_some())
    }

    /// Get a whole section (namespace) as a typed struct.
    ///
    /// For example, `stack.get_section::<VideoSettings>("engine.video")` would
    /// deserialize the `[engine.video]` section.
    ///
    /// A section is just a dotted key whose value happens to be a table, so
    /// this is [`get`](Self::get) under another name.
    pub fn get_section<T: DeserializeOwned>(&self, namespace: &str) -> Option<T> {
        self.get(namespace)
    }

    // ── Writing ─────────────────────────────────────────────────────────

    /// Set a value in the user settings layer (the first
    /// [`SettingsLayer::UserFile`] in the stack).
    ///
    /// Returns an error if no user settings layer exists, or if an ancestor of
    /// `key` already holds a scalar in the user's `settings.toml` (e.g.
    /// `engine = "x"` with a `"engine.video.vsync"` write) — a hand-edited file
    /// can put anything there, so this is reported rather than clobbered.
    pub fn set<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), StorageError> {
        let toml_value = toml::Value::try_from(value)
            .map_err(|e| StorageError::Other(format!("settings value serialization: {e}")))?;

        for layer in self.layers.iter_mut() {
            if let Some(table) = layer.table_mut() {
                return set_dotted(table, key, toml_value.clone());
            }
        }

        Err(StorageError::Other(
            "no user settings layer in the stack".into(),
        ))
    }

    // ── Persistence ─────────────────────────────────────────────────────

    /// Persist the user settings layer to its storage backend.
    ///
    /// Does nothing if no user settings layer is present or if it has not
    /// been modified since the last save.
    pub fn save(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        for layer in &self.layers {
            layer.save(storage, path)?;
        }
        Ok(())
    }

    /// Dump the merged view of all layers as a TOML string.
    ///
    /// Every key reads back the same value [`get`](Self::get) would return.
    /// Useful for debugging and for the `crcbl settings list` CLI command.
    pub fn dump(&self) -> String {
        let merge = self.merge_all();
        toml::to_string_pretty(&merge).unwrap_or_else(|_| "<merge error>".into())
    }

    // ── Internal ────────────────────────────────────────────────────────

    /// Merge all layers into one table.
    ///
    /// [`deep_merge`] is first-write-wins, so layers are visited **highest
    /// priority first** — the same order [`get`](Self::get) searches in.
    /// Visiting them lowest-first would make the dump report engine defaults
    /// where `get` returns the user's override.
    fn merge_all(&self) -> toml::Table {
        let mut merged = toml::Table::new();
        for layer in self.layers.iter().rev() {
            if let Some(table) = layer.table() {
                deep_merge(&mut merged, table);
            }
        }
        merged
    }
}

/// One key of [`SettingsStack::entries`]: the dotted key, the value that wins,
/// and the layer it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsEntry {
    /// The dotted key, as [`SettingsStack::get`] takes it.
    pub key: String,
    /// The layer the value came from.
    pub layer: LayerKind,
    /// The value, as the file would spell it.
    pub value: toml::Value,
}

// ── Dotted key helpers ─────────────────────────────────────────────────────

/// Every non-table value under `table`, under its dotted key, in key order.
fn leaves(table: &toml::Table) -> Vec<(String, toml::Value)> {
    let mut found = Vec::new();
    // A worklist in reverse so that popping visits keys in the table's order.
    let mut pending: Vec<(String, &toml::Value)> = table
        .iter()
        .rev()
        .map(|(name, value)| (name.clone(), value))
        .collect();
    while let Some((key, value)) = pending.pop() {
        match value.as_table() {
            Some(section) => pending.extend(
                section
                    .iter()
                    .rev()
                    .map(|(name, child)| (format!("{key}.{name}"), child)),
            ),
            None => found.push((key, value.clone())),
        }
    }
    found
}

/// Navigate a dotted key into a TOML table, returning the value if found.
///
/// `"engine.video.vsync"` → `table["engine"]["video"]["vsync"]`.
fn get_dotted<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    // `str::split` always yields at least one element, so `parts` is never
    // empty — not even for `""`.
    let parts: Vec<&str> = key.split('.').collect();

    // Navigate through intermediate tables.
    let mut current: &toml::Table = table;
    for &part in &parts[..parts.len() - 1] {
        match current.get(part) {
            Some(toml::Value::Table(t)) => current = t,
            _ => return None,
        }
    }

    // Get the final value from the last table.
    current.get(parts[parts.len() - 1])
}

/// Set a value at a dotted key path, creating intermediate tables as needed.
///
/// `"engine.video.vsync" = false` → sets `table["engine"]["video"]["vsync"]`.
///
/// Returns an error if an ancestor key already exists as a non-table value —
/// a user-edited `settings.toml` can contain `engine = "not a table"`, and
/// overwriting it would silently discard whatever else they wrote.
fn set_dotted(table: &mut toml::Table, key: &str, value: toml::Value) -> Result<(), StorageError> {
    // `str::split` always yields at least one element.
    let parts: Vec<&str> = key.split('.').collect();
    let (last, ancestors) = parts.split_last().expect("split yields one element");

    let mut current = table;
    for &part in ancestors {
        current = current
            .entry(part)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                StorageError::Other(format!(
                    "settings key {key:?} needs a table at {part:?}, but that key holds a value"
                ))
            })?;
    }
    current.insert((*last).to_string(), value);
    Ok(())
}

/// Deep-merge `source` into `target`. Keys in `source` that already exist in
/// `target` are skipped (first-write-wins).
fn deep_merge(target: &mut toml::Table, source: &toml::Table) {
    for (key, value) in source {
        if !target.contains_key(key) {
            target.insert(key.clone(), value.clone());
        } else if let (Some(t_target), Some(t_source)) = (
            target.get_mut(key).and_then(toml::Value::as_table_mut),
            value.as_table(),
        ) {
            // Both are tables: recurse.
            deep_merge(t_target, t_source);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(kv: &[(&str, &str)]) -> toml::Table {
        let mut table = toml::Table::new();
        for (k, v) in kv {
            set_dotted(&mut table, k, toml::Value::String(v.to_string())).unwrap();
        }
        table
    }

    // ── Dotted helpers ────────────────────────────────────────────────

    #[test]
    fn get_dotted_single_key() {
        let mut t = toml::Table::new();
        t.insert("name".into(), "engine".into());
        assert_eq!(
            get_dotted(&t, "name"),
            Some(&toml::Value::String("engine".into()))
        );
    }

    #[test]
    fn get_dotted_nested_key() {
        let mut t = toml::Table::new();
        let mut inner = toml::Table::new();
        inner.insert("vsync".into(), true.into());
        t.insert("video".into(), toml::Value::Table(inner));

        assert!(get_dotted(&t, "video").unwrap().as_table().is_some());
        assert_eq!(
            get_dotted(&t, "video.vsync"),
            Some(&toml::Value::Boolean(true))
        );
    }

    #[test]
    fn get_dotted_missing_key() {
        let t = toml::Table::new();
        assert_eq!(get_dotted(&t, "nope"), None);
        assert_eq!(get_dotted(&t, "deeply.nested.key"), None);
    }

    #[test]
    fn set_dotted_creates_intermediates() {
        let mut t = toml::Table::new();
        set_dotted(&mut t, "engine.video.vsync", true.into()).unwrap();

        assert_eq!(get_dotted(&t, "engine.video.vsync"), Some(&true.into()));
    }

    #[test]
    fn set_dotted_overwrites_existing() {
        let mut t = toml::Table::new();
        set_dotted(&mut t, "key", "old".into()).unwrap();
        set_dotted(&mut t, "key", "new".into()).unwrap();

        assert_eq!(get_dotted(&t, "key"), Some(&"new".into()));
    }

    #[test]
    fn set_over_scalar_ancestor_errors_instead_of_panicking() {
        // A hand-edited settings.toml containing `engine = "not a table"`.
        let mut user = toml::Table::new();
        user.insert("engine".into(), "not a table".into());

        let mut stack = SettingsStack::new();
        stack.add(SettingsLayer::UserFile(StorageSettingsFile {
            data: user,
            dirty: false,
        }));

        let err = stack.set("engine.video.vsync", &true).unwrap_err();
        assert!(err.to_string().contains("holds a value"), "{err}");
        // The user's value is left alone.
        assert_eq!(
            stack.get::<String>("engine").as_deref(),
            Some("not a table")
        );
    }

    // ── Deep merge ────────────────────────────────────────────────────

    #[test]
    fn deep_merge_first_writer_wins() {
        let mut target = make_table(&[("a", "1"), ("b", "2")]);
        let source = make_table(&[("b", "overwrite"), ("c", "3")]);
        deep_merge(&mut target, &source);

        // b keeps original value (first-write-wins)
        assert_eq!(get_dotted(&target, "a"), Some(&"1".into()));
        assert_eq!(get_dotted(&target, "b"), Some(&"2".into()));
        assert_eq!(get_dotted(&target, "c"), Some(&"3".into()));
    }

    #[test]
    fn deep_merge_recursive_tables() {
        let mut target = {
            let mut t = toml::Table::new();
            let mut inner = toml::Table::new();
            inner.insert("own".to_string(), "target_val".into());
            t.insert("section".to_string(), toml::Value::Table(inner));
            t
        };
        let source = {
            let mut t = toml::Table::new();
            let mut inner = toml::Table::new();
            inner.insert("own".to_string(), "source_val".into()); // exists in target → skip
            inner.insert("extra".to_string(), "source_extra".into()); // new → added
            t.insert("section".to_string(), toml::Value::Table(inner));
            t
        };

        deep_merge(&mut target, &source);

        assert_eq!(
            get_dotted(&target, "section.own"),
            Some(&"target_val".into())
        );
        assert_eq!(
            get_dotted(&target, "section.extra"),
            Some(&"source_extra".into())
        );
    }

    // ── SettingsStack ─────────────────────────────────────────────────

    #[test]
    fn stack_get_highest_priority_wins() {
        let mut stack = SettingsStack::new();
        let mut defaults = toml::Table::new();
        defaults.insert("volume".to_string(), toml::Value::Integer(100));
        let mut user = toml::Table::new();
        user.insert("volume".to_string(), toml::Value::Integer(50));

        stack.add(SettingsLayer::EngineDefaults(defaults));
        stack.add(SettingsLayer::CliOverrides(user));

        let vol: i64 = stack.get("volume").unwrap();
        assert_eq!(vol, 50);
    }

    #[test]
    fn stack_get_falls_through_missing_keys() {
        let mut stack = SettingsStack::new();
        stack.add(SettingsLayer::EngineDefaults(make_table(&[("a", "1")])));

        assert_eq!(stack.get::<String>("a").as_deref(), Some("1"));
        assert_eq!(stack.get::<String>("b"), None);
    }

    /// **`contains` answers for a key `get` cannot read**, which is the whole
    /// reason it exists: a value of the wrong type and a key that was never
    /// written both make `get::<bool>` return `None`, and only one of them is
    /// something to tell a player about.
    #[test]
    fn contains_separates_a_wrong_typed_key_from_a_missing_one() {
        let mut stack = SettingsStack::new();
        let mut table = toml::Table::new();
        set_dotted(
            &mut table,
            "engine.video.shadows",
            toml::Value::String("off".into()),
        )
        .unwrap();
        stack.add(SettingsLayer::UserFile(StorageSettingsFile {
            data: table,
            dirty: false,
        }));

        assert_eq!(stack.get::<bool>("engine.video.shadows"), None);
        assert!(
            stack.contains("engine.video.shadows"),
            "the key is in the file, it just does not hold a bool"
        );
        assert!(
            !stack.contains("engine.video.reflections"),
            "a key nobody wrote is not present"
        );
        assert!(
            !stack.contains("engine.video.shadows.deeper"),
            "a path through a scalar is not a key"
        );
    }

    /// **A key is present if *any* layer has it**, not only the top one.
    ///
    /// The arm that fails if the search stops at the highest-priority layer:
    /// an engine default the user's file never mentions is still a key this
    /// stack defines, and reporting it as absent would have a caller warn
    /// about a line it can read perfectly well.
    #[test]
    fn contains_searches_every_layer() {
        let mut stack = SettingsStack::new();
        stack.add(SettingsLayer::EngineDefaults(make_table(&[("a", "1")])));
        stack.add(SettingsLayer::UserFile(StorageSettingsFile {
            data: make_table(&[("b", "2")]),
            dirty: false,
        }));

        assert!(stack.contains("a"), "a key only the bottom layer defines");
        assert!(stack.contains("b"), "a key only the top layer defines");
        assert!(!stack.contains("c"));
    }

    #[test]
    fn get_section_returns_the_table_the_layer_stored_under_that_key() {
        let mut stack = SettingsStack::new();
        let mut t = toml::Table::new();
        let mut video = toml::Table::new();
        video.insert("vsync".to_string(), true.into());
        t.insert("video".to_string(), toml::Value::Table(video));

        stack.add(SettingsLayer::EngineDefaults(t));

        let section: toml::Table = stack.get_section("video").unwrap();
        assert_eq!(section.get("vsync"), Some(&toml::Value::Boolean(true)));
    }

    #[test]
    fn stack_set_stores_in_user_layer() {
        let mut stack = SettingsStack::new();
        let user_file = StorageSettingsFile {
            data: toml::Table::new(),
            dirty: true,
        };
        stack.add(SettingsLayer::UserFile(user_file));

        stack.set("volume", &42).unwrap();
        let vol: i64 = stack.get("volume").unwrap();
        assert_eq!(vol, 42);
    }

    #[test]
    fn stack_set_errors_without_user_layer() {
        let mut stack = SettingsStack::new();
        stack.add(SettingsLayer::EngineDefaults(toml::Table::new()));

        let result = stack.set("key", &"val");
        assert!(result.is_err());
    }

    #[test]
    fn stack_dump_produces_toml() {
        let mut stack = SettingsStack::new();
        let mut t = toml::Table::new();
        t.insert("name".to_string(), "engine".into());
        stack.add(SettingsLayer::EngineDefaults(t));

        let dump = stack.dump();
        assert!(dump.contains("name"));
    }

    #[test]
    fn stack_dump_agrees_with_get() {
        let mut stack = SettingsStack::new();

        let mut defaults = toml::Table::new();
        let mut video = toml::Table::new();
        video.insert("vsync".into(), true.into());
        video.insert("fov".into(), toml::Value::Integer(90));
        defaults.insert("video".into(), toml::Value::Table(video));
        defaults.insert("volume".into(), toml::Value::Integer(100));
        stack.add(SettingsLayer::EngineDefaults(defaults));

        let mut overrides = toml::Table::new();
        let mut video = toml::Table::new();
        video.insert("vsync".into(), false.into());
        overrides.insert("video".into(), toml::Value::Table(video));
        overrides.insert("volume".into(), toml::Value::Integer(50));
        stack.add(SettingsLayer::CliOverrides(overrides));

        let dumped: toml::Table = toml::from_str(&stack.dump()).unwrap();
        for key in ["volume", "video.vsync", "video.fov"] {
            assert_eq!(
                get_dotted(&dumped, key),
                stack.get::<toml::Value>(key).as_ref(),
                "dump disagrees with get for {key}"
            );
        }
        // Specifically: the higher-priority layer wins in both.
        assert_eq!(stack.get::<i64>("volume"), Some(50));
    }

    // ── StorageSettingsFile ───────────────────────────────────────────

    #[test]
    fn load_missing_file_returns_empty_table() {
        let storage = crate::MemoryStorage::new();
        let file = StorageSettingsFile::load(&storage, Path::new("settings.toml")).unwrap();
        assert!(file.table().is_empty());
        assert!(!file.dirty);
    }

    #[test]
    fn load_valid_toml_file() {
        // Write a TOML file, then load it
        let storage = crate::MemoryStorage::new();
        let path = Path::new("settings.toml");
        storage.write(path, br#"volume = 75"#).unwrap();

        let file = StorageSettingsFile::load(&storage, path).unwrap();
        assert_eq!(file.table().get("volume"), Some(&toml::Value::Integer(75)));
    }

    /// A settings file that is not UTF-8 is reported, not silently repaired.
    ///
    /// The bytes are a valid TOML document with one invalid byte inside a
    /// string, which is the shape a partial write or a foreign encoding leaves
    /// behind. Decoded lossily it parses — `volume` survives, the string becomes
    /// a replacement character — so the load used to succeed with a value the
    /// file does not contain, and the next `save` wrote that back.
    #[test]
    fn load_rejects_a_file_that_is_not_utf8() {
        let storage = crate::MemoryStorage::new();
        let path = Path::new("settings.toml");
        let mut bytes = b"volume = 75\nname = \"".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"\"\n");
        storage.write(path, &bytes).unwrap();

        // The premise: lossy decoding would have parsed this.
        assert!(
            toml::from_str::<toml::Table>(&String::from_utf8_lossy(&bytes)).is_ok(),
            "the fixture must be a document only the encoding refuses"
        );

        let error = StorageSettingsFile::load(&storage, path)
            .expect_err("a settings file that is not UTF-8");
        let text = error.to_string();
        assert!(text.contains("not UTF-8"), "{text}");
        assert!(text.contains("settings.toml"), "{text}");
        assert!(text.contains("offset 20"), "{text}");
    }

    /// **A file that is not TOML is an empty layer, not a refused start-up.**
    ///
    /// The arm a hand-edited `settings.toml` reaches. [`StorageSettingsFile::load`]
    /// reports it — that is what its own test asserts — and this is the layer
    /// above deciding the game still starts, with every key reading as absent.
    #[test]
    fn a_settings_file_that_is_not_toml_leaves_an_empty_layer_behind() {
        let storage = crate::MemoryStorage::new();
        storage
            .write(Path::new(SETTINGS_FILE), b"this is not [ toml")
            .expect("memory storage accepts every write");
        // The premise: the loader really does refuse this one.
        assert!(
            StorageSettingsFile::load(&storage, Path::new(SETTINGS_FILE)).is_err(),
            "the fixture has to be a file the loader rejects"
        );

        let stack = SettingsStack::from_storage(&storage);
        assert_eq!(
            stack.len(),
            1,
            "the user layer is still there to be written to"
        );
        assert_eq!(
            stack.get::<bool>("engine.video.shadows"),
            None,
            "a broken file must read as absent, not as a value"
        );
    }

    /// **A start-up that finds no settings file leaves the machine as it found
    /// it.**
    ///
    /// [`SettingsStack::platform`] runs on every start-up of every game, so a
    /// `mkdir` in it would create a config directory on every machine that has
    /// never had one — including every CI runner and every test process. The
    /// check is the directory's absence *after* the read, which is what
    /// [`NativeStorage::config_root`](crate::NativeStorage::config_root) exists
    /// for.
    #[test]
    fn a_platform_stack_for_an_app_with_no_file_is_empty_and_creates_nothing() {
        // A name nothing owns, so the answer cannot be a real game's settings.
        let app = "crcbl-settings-platform-test-no-such-app";
        let root = crate::NativeStorage::config_root(app);

        let stack = SettingsStack::platform(app);
        assert_eq!(
            stack.len(),
            1,
            "the user layer is present whether or not its file was"
        );
        assert_eq!(
            stack.get::<bool>("engine.video.shadows"),
            None,
            "a player with no settings file has said nothing about any key"
        );

        // `None` is a platform that names no config directory at all, which is
        // an arm this suite can reach on a machine with no HOME — and there is
        // then nothing that could have been created.
        if let Some(root) = root {
            assert!(
                !root.exists(),
                "reading settings created {}",
                root.display()
            );
        }
    }

    // ── Launch layers ─────────────────────────────────────────────────

    /// A user layer holding `toml`, as the loader would read it.
    fn user_file(toml: &str) -> StorageSettingsFile {
        StorageSettingsFile {
            data: toml::from_str(toml).expect("a test's own TOML"),
            dirty: false,
        }
    }

    /// Launch layers with `game` as the game's defaults and `sets` as the
    /// command line's overrides.
    fn launch(game: &str, sets: &[&str]) -> LaunchLayers {
        let mut launch = LaunchLayers::new()
            .with_game_defaults(game)
            .expect("a test's own TOML");
        for arg in sets {
            launch.set(arg).expect("a well-formed override");
        }
        launch
    }

    /// **One key through all four layers: engine < game < user < command
    /// line.**
    ///
    /// Each layer is peeled off in turn, so every one of the three orderings
    /// is asserted on its own — a stack that put the overrides beneath the
    /// player's file answers `3` in the first assertion, one that put the
    /// game's defaults above it answers `2` in the second.
    #[test]
    fn one_key_resolves_engine_then_game_then_user_then_command_line() {
        let stack_of = |launch: &LaunchLayers, file: &str| {
            let mut stack = SettingsStack::new();
            stack.add(SettingsLayer::EngineDefaults(
                toml::from_str("[game]\nlives = 1").expect("a test's own TOML"),
            ));
            for layer in SettingsStack::layered(launch, user_file(file)).layers {
                stack.add(layer);
            }
            stack
        };
        let key = "game.lives";

        let all = stack_of(
            &launch("[game]\nlives = 2", &["game.lives=4"]),
            "[game]\nlives = 3",
        );
        assert_eq!(all.get::<i64>(key), Some(4), "the command line wins");
        assert_eq!(all.layer_of(key), Some(LayerKind::CommandLine));

        let no_cli = stack_of(&launch("[game]\nlives = 2", &[]), "[game]\nlives = 3");
        assert_eq!(no_cli.get::<i64>(key), Some(3), "then the player's file");
        assert_eq!(no_cli.layer_of(key), Some(LayerKind::User));

        let no_user = stack_of(&launch("[game]\nlives = 2", &[]), "");
        assert_eq!(no_user.get::<i64>(key), Some(2), "then the game's defaults");
        assert_eq!(no_user.layer_of(key), Some(LayerKind::Game));

        let engine_only = stack_of(&LaunchLayers::new(), "");
        assert_eq!(engine_only.get::<i64>(key), Some(1), "then the engine's");
        assert_eq!(engine_only.layer_of(key), Some(LayerKind::Engine));
    }

    /// **An override is read for this run and never written to the file.**
    ///
    /// The player changes a different key and saves, and the file that comes
    /// back holds their two keys and nothing the command line said — not the
    /// override of a key the file already had, and not a key it did not.
    #[test]
    fn an_override_is_never_written_back_when_the_user_file_saves() {
        let storage = crate::MemoryStorage::new();
        let path = Path::new(SETTINGS_FILE);
        storage
            .write(path, b"volume = 3\n")
            .expect("memory storage accepts every write");
        let launch = launch(
            "difficulty = \"normal\"",
            &["volume=9", "engine.video.shadows=false"],
        );

        let mut stack = SettingsStack::from_storage_with(&storage, &launch);
        assert_eq!(stack.get::<i64>("volume"), Some(9), "the override is read");
        stack.set("speed", &2).expect("the user layer is writable");
        stack.save(&storage, path).expect("memory storage saves");

        let written: toml::Table = toml::from_str(
            str::from_utf8(&storage.read(path).expect("the save wrote a file"))
                .expect("the writer emits UTF-8"),
        )
        .expect("the writer emits TOML");
        let mut expected = toml::Table::new();
        expected.insert("volume".into(), 3.into());
        expected.insert("speed".into(), 2.into());
        assert_eq!(
            written, expected,
            "the file holds what the player wrote, and no launch layer"
        );
    }

    /// **An override's value is a TOML value**, read as `settings.toml` reads
    /// one, and the key it lands under is the dotted key it named.
    #[test]
    fn an_override_is_typed_by_the_grammar_the_file_uses() {
        let mut layers = LaunchLayers::new();
        for (arg, key) in [
            ("engine.video.shadows=false", "engine.video.shadows"),
            ("engine.video.render_scale=0.5", "engine.video.render_scale"),
            ("game.lives=3", "game.lives"),
            ("game.name=\"Ada\"", "game.name"),
            ("game.empty=''", "game.empty"),
            ("game.equation=\"a=b\"", "game.equation"),
            ("game.list=[1, 2]", "game.list"),
        ] {
            assert_eq!(layers.set(arg).as_deref(), Ok(key), "`{arg}`");
        }
        let stack = SettingsStack::layered(&layers, StorageSettingsFile::empty());
        assert_eq!(stack.get::<bool>("engine.video.shadows"), Some(false));
        assert_eq!(stack.get::<f64>("engine.video.render_scale"), Some(0.5));
        assert_eq!(stack.get::<i64>("game.lives"), Some(3));
        assert_eq!(stack.get::<String>("game.name").as_deref(), Some("Ada"));
        assert_eq!(stack.get::<String>("game.empty").as_deref(), Some(""));
        assert_eq!(
            stack.get::<String>("game.equation").as_deref(),
            Some("a=b"),
            "the key ends at the first `=`"
        );
        assert_eq!(stack.get::<Vec<i64>>("game.list"), Some(vec![1, 2]));
    }

    /// **A malformed override is refused, and the refusal names its key.**
    ///
    /// A bare word is the case worth pinning: `crcbl settings set` takes one
    /// as text, but the same word on a line of `settings.toml` does not parse,
    /// and an override follows the file.
    #[test]
    fn a_malformed_override_is_refused_by_name() {
        for (arg, names) in [
            (
                "engine.video.render_scale=fast",
                "engine.video.render_scale",
            ),
            ("game.name=Ada", "game.name"),
            ("game.lives=", "game.lives"),
            ("game.lives=1\nother = 2", "game.lives"),
            ("game.lives=1 2", "game.lives"),
        ] {
            let refused = LaunchLayers::new()
                .set(arg)
                .expect_err(&format!("`{arg}` was accepted"))
                .to_string();
            assert!(refused.contains(names), "`{arg}`: {refused}");
            assert!(refused.contains("not a TOML value"), "`{arg}`: {refused}");
        }

        for (arg, says) in [
            ("game.lives", "is not <KEY>=<VALUE>"),
            ("=3", "names no key"),
            ("game..lives=3", "is not a settings key"),
            ("game lives=3", "is not a settings key"),
            (".lives=3", "is not a settings key"),
        ] {
            let refused = LaunchLayers::new()
                .set(arg)
                .expect_err(&format!("`{arg}` was accepted"))
                .to_string();
            assert!(refused.contains(says), "`{arg}`: {refused}");
        }

        let mut layers = LaunchLayers::new();
        layers.set("game=1").expect("a scalar key");
        let refused = layers
            .set("game.lives=3")
            .expect_err("a key through a scalar override")
            .to_string();
        assert!(refused.starts_with("game.lives: "), "{refused}");
        assert_eq!(
            layers.overridden_keys(),
            ["game"],
            "the refused one left no trace"
        );
    }

    /// **A key given twice keeps the later value**, so a wrapper script can
    /// append an override to a command line that already has one.
    #[test]
    fn the_later_of_two_overrides_of_one_key_wins() {
        let layers = launch("", &["game.lives=3", "game.lives=5"]);
        let stack = SettingsStack::layered(&layers, StorageSettingsFile::empty());
        assert_eq!(stack.get::<i64>("game.lives"), Some(5));
        assert_eq!(layers.overridden_keys(), ["game.lives"]);
    }

    /// A game's defaults that are not TOML refuse the run; ones that are say
    /// which keys they hold.
    #[test]
    fn game_defaults_are_a_toml_document_and_answer_for_their_keys() {
        assert!(
            LaunchLayers::new()
                .with_game_defaults("this is not [ toml")
                .is_err()
        );
        let layers = launch("[editor.snap]\ngrid = 0.25", &[]);
        assert!(layers.has_game_defaults());
        assert!(layers.game_defines("editor.snap.grid"));
        assert!(!layers.game_defines("editor.snap.angle"));
        assert!(!LaunchLayers::new().has_game_defaults());
        assert!(LaunchLayers::new().is_empty());
        assert!(!layers.is_empty());
    }

    /// **A launch with nothing in it adds no layer**, so every stack this
    /// crate built before launch layers existed is still the stack it builds.
    #[test]
    fn an_empty_launch_layers_nothing_around_the_file() {
        let stack = SettingsStack::layered(&LaunchLayers::new(), StorageSettingsFile::empty());
        assert_eq!(stack.len(), 1);
        let stack =
            SettingsStack::layered(&launch("a = 1", &["b=2"]), StorageSettingsFile::empty());
        assert_eq!(stack.len(), 3);
    }

    /// **`find` passes a refused value over and the layer beneath answers**,
    /// naming itself — which is how a caller tells a player their line did
    /// nothing.
    #[test]
    fn find_passes_over_a_refused_value_to_the_layer_beneath() {
        let stack = SettingsStack::layered(
            &launch("[editor.snap]\ngrid = 0.25", &[]),
            user_file("[editor.snap]\ngrid = 0.0"),
        );
        let positive = |step: &f64| *step > 0.0;
        assert_eq!(
            stack.find("editor.snap.grid", positive),
            Some((0.25, LayerKind::Game))
        );
        assert_eq!(stack.layer_of("editor.snap.grid"), Some(LayerKind::User));
        assert_eq!(
            stack.find("editor.snap.grid", |_: &f64| true),
            Some((0.0, LayerKind::User))
        );
        assert_eq!(stack.find("editor.snap.angle", positive), None);
    }

    /// **`entries` names the layer each key's value came from**, and the value
    /// is the one that wins.
    #[test]
    fn entries_name_the_layer_each_value_came_from() {
        let stack = SettingsStack::layered(
            &launch("[game]\nlives = 2\nspeed = 1", &["game.lives=4"]),
            user_file("[game]\nspeed = 3\nname = \"Ada\""),
        );
        let entries: Vec<(String, LayerKind, toml::Value)> = stack
            .entries()
            .into_iter()
            .map(|entry| (entry.key, entry.layer, entry.value))
            .collect();
        assert_eq!(
            entries,
            [
                ("game.lives".to_owned(), LayerKind::CommandLine, 4.into()),
                ("game.name".to_owned(), LayerKind::User, "Ada".into()),
                ("game.speed".to_owned(), LayerKind::User, 3.into()),
            ]
        );
        assert_eq!(
            [
                LayerKind::Engine,
                LayerKind::Game,
                LayerKind::User,
                LayerKind::CommandLine,
            ]
            .map(LayerKind::name),
            ["engine", "game", "user", "cli"]
        );
    }

    #[test]
    fn mark_dirty_on_mutate() {
        let mut file = StorageSettingsFile {
            data: toml::Table::new(),
            dirty: false,
        };
        file.table_mut().insert("key".into(), "val".into());
        assert!(file.dirty);
    }
}
