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
//! 4. **CLI / env overrides** — single-key overrides passed at launch.
//!
//! Reading a key searches layers from highest to lowest and returns the first
//! hit. Writing a value stores it in the user settings layer, which can be
//! persisted atomically via [`SettingsStack::save`].
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
    /// CLI / env overrides (parsed as TOML key = value pairs).
    CliOverrides(toml::Table),
}

impl SettingsLayer {
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
    /// Load settings from `path` in `storage`. If the file does not exist,
    /// returns an empty (defaults-only) table.
    pub fn load(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        let data = match storage.read(path) {
            Ok(bytes) => toml::from_str(&String::from_utf8_lossy(&bytes))
                .map_err(|e| StorageError::Other(format!("invalid settings TOML: {e}")))?,
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
    /// priority to lowest. Returns `None` if no layer defines the key or if
    /// the value cannot be deserialized into `T`.
    ///
    /// Key path examples: `"engine.video.vsync"`, `"game.difficulty"`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        for layer in self.layers.iter().rev() {
            // A layer without a table is skipped, not treated as the end of
            // the search.
            let Some(table) = layer.table() else { continue };
            if let Some(value) = get_dotted(table, key)
                && let Ok(v) = value.clone().try_into::<T>()
            {
                return Some(v);
            }
        }
        None
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

// ── Dotted key helpers ─────────────────────────────────────────────────────

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
