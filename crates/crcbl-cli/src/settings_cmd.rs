//! `crcbl settings` — a game's `settings.toml`, from a terminal.
//!
//! `docs/plan/14-persistence.md` schedules `crcbl settings get|set|list` as
//! "scriptable settings", and `docs/plan/11-cli-headless.md` is the reason it
//! has to exist at all: a capability the settings *screen* has and a script
//! does not is a capability implemented GUI-side. The mechanism is
//! [`crcbl_store::settings`] and was already built — layered TOML, dotted keys,
//! typed reads — so this module is the wiring, and everything interesting about
//! it is a decision about what the wiring must not get wrong.
//!
//! # The type a value lands as is the whole design
//!
//! [`crcbl::settings::video_effects`] reads `engine.video.shadows` with
//! `stack.get::<bool>`, and [`SettingsStack::get`] skips a value it cannot
//! deserialize. So a `set` that wrote the *string* `"false"` there would leave
//! a line in the file that reads back as nothing: the player sees their setting
//! saved, the engine never sees it, and no test in either half would notice.
//! [`Value::parse`] is the answer, and `crate::args::SETTINGS_USAGE` states the
//! rule where a user reads it.
//!
//! # One layer, and it is the player's file
//!
//! A game's engine and game defaults are compiled into the game's own binary,
//! and this CLI is not that binary. So the stack it assembles has exactly one
//! layer — [`SETTINGS_FILE`] — and `list` says so: a key the game defaults and
//! the player has never changed is absent here and still has a value at run
//! time. Reporting a merged view this process cannot actually see would be
//! inventing the half it is missing.
//!
//! # A file that will not parse fails the command
//!
//! [`SettingsStack::platform`] turns an unreadable settings file into an empty
//! layer and a log line, because a start-up must not die over one. That is the
//! wrong answer here: `set` would then write a fresh file over whatever the
//! player had, so the file is loaded through [`StorageSettingsFile::load`]
//! directly and a parse error is [`Failure`].

use std::path::{Path, PathBuf};

use crcbl_store::settings::{SETTINGS_FILE, SettingsLayer, SettingsStack, StorageSettingsFile};
use crcbl_store::{MemoryStorage, NativeStorage, StorageError, StorageSource};

use crate::args::{SettingsAction, SettingsArgs};
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs `crcbl settings`.
///
/// # Errors
///
/// [`Failure`] if the game cannot be named, if the platform names no config
/// directory, if `settings.toml` is not readable TOML, if a `set` cannot be
/// written, or if `get` finds a value it does not render.
pub fn run(args: &SettingsArgs) -> Result<Outcome, Failure> {
    let app = app_name(args)?;
    let root = config_root(args, &app)?;
    let path = root.join(SETTINGS_FILE);
    let storage = NativeStorage::at(root);

    let file_exists = storage.exists(Path::new(SETTINGS_FILE));
    let file = StorageSettingsFile::load(&storage, Path::new(SETTINGS_FILE)).map_err(|error| {
        Failure::new(format!("cannot read {}: {error}", path.display()))
            .with("path", Json::string(path.display().to_string()))
    })?;

    match args.action {
        SettingsAction::List => list(file, &app, &path, file_exists),
        SettingsAction::Set => set(args, file, &storage, &app, &path),
        SettingsAction::Get => {
            let mut stack = SettingsStack::new();
            stack.add(SettingsLayer::UserFile(file));
            get(args, &stack, &app, &path)
        }
    }
}

// ── The three branches ──────────────────────────────────────────────────────

/// `crcbl settings list`.
fn list(
    file: StorageSettingsFile,
    app: &str,
    path: &Path,
    file_exists: bool,
) -> Result<Outcome, Failure> {
    let (entries, stray) = flatten(&file);
    // `SettingsStack::dump` renders the merged view as TOML, which for a
    // one-layer stack is the file itself — the same text a person would open
    // the file to read, rather than a second rendering of it.
    let mut stack = SettingsStack::new();
    stack.add(SettingsLayer::UserFile(file));

    let header = format!("settings for `{app}` — {}", path.display());
    // Named after the dump rather than beside each line: the dump is the file
    // as a person would open it, and interleaving a verdict into TOML would
    // stop it being that. A stray is worth saying out loud because it is the
    // one thing in the file that will never do anything — see [`status_of`].
    let warning = if stray.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n{} key(s) under `{ENGINE_PREFIX}` that the engine does not \
             define, so nothing reads them: {}",
            stray.len(),
            stray.join(", ")
        )
    };
    let human = if !file_exists {
        format!("{header} (no file yet)")
    } else if entries.is_empty() {
        format!("{header} (empty)")
    } else {
        format!("{header}\n\n{}{warning}", stack.dump().trim_end())
    };

    Ok(Outcome {
        human,
        json: vec![
            ("action", Json::string(SettingsAction::List.name())),
            ("app", Json::string(app)),
            ("path", Json::string(path.display().to_string())),
            ("file_exists", Json::Bool(file_exists)),
            ("count", Json::Number(entries.len() as i64)),
            (
                "unknown",
                Json::Array(stray.into_iter().map(Json::string).collect()),
            ),
            ("settings", Json::Array(entries)),
        ],
    })
}

/// `crcbl settings get <KEY>`.
fn get(
    args: &SettingsArgs,
    stack: &SettingsStack,
    app: &str,
    path: &Path,
) -> Result<Outcome, Failure> {
    let key = args.key.as_deref().expect("`get` parses with a key");
    let common = vec![
        ("action", Json::string(SettingsAction::Get.name())),
        ("app", Json::string(app)),
        ("path", Json::string(path.display().to_string())),
        ("key", Json::string(key)),
    ];

    if let Some(value) = Value::read(stack, key) {
        let mut json = common;
        json.push(("present", Json::Bool(true)));
        json.push(("type", Json::string(value.type_name())));
        json.push(("value", value.json()));
        // The bare value and nothing else, so `v=$(crcbl settings get k)` is
        // the value. Everything a person would want alongside it — the file,
        // the type — is in `--json`, where it cannot corrupt that.
        return Ok(Outcome {
            human: value.bare(),
            json,
        });
    }

    // `SettingsStack::get` answers `None` both for a key no layer defines and
    // for one holding something it could not deserialize, and those are
    // different answers to a person asking what their setting is.
    // `SettingsStack::contains` is the half that tells them apart.
    if stack.contains(key) {
        let mut failure = Failure::new(format!(
            "`{key}` is set to a list or a table, which `get` does not render; \
             `crcbl settings list` shows the whole file"
        ));
        failure.json = common;
        failure.json.push(("present", Json::Bool(true)));
        return Err(failure);
    }

    // Not a failure: the command answered, and the answer is that nothing is
    // set. So stdout carries nothing for a script to mistake for a value, and
    // the sentence a person needs goes to stderr.
    if !args.json {
        eprintln!("crcbl: `{key}` is not set in {}", path.display());
    }
    let mut json = common;
    // No `value` and no `type` field at all rather than a placeholder, which is
    // `lod`'s rule for a number that does not exist: a consumer branches on
    // `present` and never has to decide whether some sentinel meant "unset".
    json.push(("present", Json::Bool(false)));
    Ok(Outcome {
        human: String::new(),
        json,
    })
}

/// `crcbl settings set <KEY> <VALUE>`.
fn set(
    args: &SettingsArgs,
    file: StorageSettingsFile,
    storage: &NativeStorage,
    app: &str,
    path: &Path,
) -> Result<Outcome, Failure> {
    let key = args.key.as_deref().expect("`set` parses with a key");
    let raw = args.value.as_deref().expect("`set` parses with a value");
    let value = Value::parse(raw);

    let mut stack = SettingsStack::new();
    stack.add(SettingsLayer::UserFile(file));
    value.write(&mut stack, key).map_err(|error| {
        Failure::new(format!("cannot set `{key}`: {error}"))
            .with("path", Json::string(path.display().to_string()))
    })?;
    // The one write in this verb, and the one place the config directory is
    // created: `NativeStorage::write` makes the parent of what it writes, and
    // this is a directory the user asked for by asking for the write. A read
    // never reaches here, which is why `list` and `get` leave a machine that
    // has never had one exactly as they found it.
    stack
        .save(storage, Path::new(SETTINGS_FILE))
        .map_err(|error| {
            Failure::new(format!("cannot write {}: {error}", path.display()))
                .with("path", Json::string(path.display().to_string()))
        })?;

    Ok(Outcome {
        // The type is named because it is the thing a person cannot see and
        // would have to guess: `set game.name 42` and `set game.name '"42"'`
        // differ only here.
        human: format!(
            "{key} = {} ({}) in {}",
            value.bare(),
            value.type_name(),
            path.display()
        ),
        json: vec![
            ("action", Json::string(SettingsAction::Set.name())),
            ("app", Json::string(app)),
            ("path", Json::string(path.display().to_string())),
            ("key", Json::string(key)),
            ("type", Json::string(value.type_name())),
            ("value", value.json()),
        ],
    })
}

// ── The value, and the type it lands as ─────────────────────────────────────

/// The one key the probe document in [`Value::parse`] defines.
const PROBE_KEY: &str = "value";

/// A settings value in one of the four kinds this verb reads and writes.
///
/// Deliberately not every kind TOML has: a list and a table are things a
/// settings file may hold and `list` reports, and neither is something to write
/// from a command line one word at a time.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    /// `true` / `false` — the kind `engine.video.*` is read as.
    Bool(bool),
    /// A TOML integer, which is 64-bit and signed.
    Integer(i64),
    /// A TOML float, which is an `f64` by definition.
    Float(f64),
    /// A TOML string.
    Text(String),
}

impl Value {
    /// The name this kind is reported under, in `--json` and to a person.
    fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::Text(_) => "string",
        }
    }

    /// The value with no quoting and no decoration, for stdout.
    fn bare(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            // `{:?}`, not `{}`: the shortest decimal that reads back as the
            // same bits, and it keeps `1.0` a float rather than printing `1`.
            Self::Float(value) => format!("{value:?}"),
            Self::Text(value) => value.clone(),
        }
    }

    /// The value as its natural JSON type.
    fn json(&self) -> Json {
        match self {
            Self::Bool(value) => Json::Bool(*value),
            Self::Integer(value) => Json::Number(*value),
            Self::Float(value) => Json::Double(*value),
            Self::Text(value) => Json::string(value),
        }
    }

    /// The kind `raw` names, decided by TOML's own value grammar.
    ///
    /// The grammar rather than a rule of this CLI's own, because the file the
    /// value lands in is TOML and a person editing it by hand already writes
    /// `true`, `0x2a`, `1_000` and `"quoted"` with those meanings. It also
    /// gives the escape hatch for free: quoting is how a value that looks like
    /// a boolean or a number is stored as text.
    ///
    /// `crcbl-store` exposes a TOML *reader* and not the parser behind it, so
    /// reaching the grammar means loading a one-line document out of memory.
    /// That document is why a `raw` spanning lines is text without being
    /// offered to the parser at all: `"true\n[other]\nx = 1"` parses, and the
    /// value would then be typed by a line that was never about the value.
    fn parse(raw: &str) -> Self {
        if !raw.contains(['\n', '\r']) {
            let probe = MemoryStorage::new();
            let document = format!("{PROBE_KEY} = {raw}");
            let path = Path::new("probe.toml");
            if probe.write(path, document.as_bytes()).is_ok()
                && let Ok(file) = StorageSettingsFile::load(&probe, path)
            {
                let mut stack = SettingsStack::new();
                stack.add(SettingsLayer::UserFile(file));
                if let Some(value) = Self::read(&stack, PROBE_KEY) {
                    return value;
                }
            }
        }
        Self::Text(raw.to_string())
    }

    /// What `key` holds, or `None` for a key that is absent or holds a kind
    /// this verb does not render.
    ///
    /// **The order is the rule.** `SettingsStack::get` asks serde to
    /// deserialize, and a TOML integer deserializes into an `f64` perfectly
    /// happily — so asking for a float first would report every integer in a
    /// settings file as a float and write `1.0` back where the file said `1`.
    /// Each kind is asked for before any kind that would accept it.
    fn read(stack: &SettingsStack, key: &str) -> Option<Self> {
        if let Some(value) = stack.get::<bool>(key) {
            return Some(Self::Bool(value));
        }
        if let Some(value) = stack.get::<i64>(key) {
            return Some(Self::Integer(value));
        }
        if let Some(value) = stack.get::<f64>(key) {
            return Some(Self::Float(value));
        }
        stack.get::<String>(key).map(Self::Text)
    }

    /// Writes this value into the stack's user layer.
    fn write(&self, stack: &mut SettingsStack, key: &str) -> Result<(), StorageError> {
        match self {
            Self::Bool(value) => stack.set(key, value),
            Self::Integer(value) => stack.set(key, value),
            Self::Float(value) => stack.set(key, value),
            Self::Text(value) => stack.set(key, value),
        }
    }
}

// ── Where the file is ───────────────────────────────────────────────────────

/// The game whose settings these are.
///
/// `--app` when it was given, and otherwise the package name of the project
/// `crcbl run` and `crcbl build` would act on — [`crate::cargo::locate_manifest`]
/// is that search, and it is called rather than repeated.
fn app_name(args: &SettingsArgs) -> Result<String, Failure> {
    if let Some(app) = &args.app {
        return Ok(app.clone());
    }

    let manifest = crate::cargo::locate_manifest().map_err(|failure| {
        Failure::new(format!(
            "{}\nhint: settings belong to a game, and this one takes its name from the \
             project here. `--app <NAME>` names it directly.",
            failure.message
        ))
    })?;
    let directory = manifest
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let Some(file_name) = manifest.file_name() else {
        return Err(Failure::new(format!(
            "{} does not name a manifest to read",
            manifest.display()
        )));
    };

    // A manifest is TOML and `StorageSettingsFile` is the TOML reader this
    // workspace exposes; this binary has no TOML dependency of its own and is
    // not the place to acquire one to read a single key. `NativeStorage` is a
    // sandbox rooted at a directory, so the manifest's directory is the root
    // and its name is the key — `replay`'s own rule for a path typed on a
    // command line.
    let storage = NativeStorage::at(directory);
    let file = StorageSettingsFile::load(&storage, Path::new(file_name))
        .map_err(|error| Failure::new(format!("cannot read {}: {error}", manifest.display())))?;
    let mut stack = SettingsStack::new();
    stack.add(SettingsLayer::UserFile(file));

    let Some(name) = stack.get::<String>("package.name") else {
        return Err(Failure::new(format!(
            "{} declares no [package] name, so there is no game to read settings for\n\
             hint: a virtual workspace root is one of these. `--app <NAME>` names the \
             game directly.",
            manifest.display()
        )));
    };
    crate::args::check_app_name(&name).map_err(|why| {
        Failure::new(format!(
            "the package name `{name}` in {} cannot be a config directory: {why}\n\
             hint: `--app <NAME>` names the game directly.",
            manifest.display()
        ))
    })?;
    Ok(name)
}

/// The directory `settings.toml` lives in.
fn config_root(args: &SettingsArgs, app: &str) -> Result<PathBuf, Failure> {
    if let Some(base) = &args.config_dir {
        return Ok(base.join(app));
    }
    NativeStorage::config_root(app).ok_or_else(|| {
        Failure::new(format!(
            "this platform names no config directory, so there is nowhere for `{app}`'s \
             {SETTINGS_FILE} to be\nhint: `--config-dir <DIR>` says where to put it."
        ))
    })
}

// ── Rendering the file ──────────────────────────────────────────────────────

/// Every scalar in the file, as `{key, type, value}` records under its dotted
/// key.
///
/// # User keys are values here, never JSON keys
///
/// `docs/plan/11-cli-headless.md` asks for stable JSON schemas, and a settings
/// file's keys are whatever the player and the game put there. Rendering them
/// as the object's own keys would make the schema a function of the file, so
/// the record shape is fixed and the key travels inside it.
///
/// # Why this is a stack and not a recursive function
///
/// The values being walked are `toml`'s, and `toml` is `crcbl-store`'s
/// dependency rather than this binary's — so nothing here can name the type,
/// and a `fn` taking one would have to. An explicit worklist takes every type
/// from inference instead. `toml::Table` iterates in key order, so the records
/// come out in a stable order for a diff.
/// The engine namespace, whose keys the engine's own catalogue is the authority
/// on.
///
/// Anything outside it belongs to whichever game wrote it, so an unrecognised
/// `[game]` key is not a mistake this command can see.
const ENGINE_PREFIX: &str = "engine.";

/// What [`crcbl::settings::catalogue`] says about a key in a player's file.
///
/// The four answers, and the one that matters is the third: **a key under
/// `engine.` that the catalogue does not name is a typo**, and until this
/// existed it was invisible. The readers warn about a key they know holding the
/// wrong type; nothing warned about `engine.video.shadow`, which parses, saves,
/// lists and is read by nothing for ever.
fn status_of(key: &str) -> &'static str {
    match crcbl::settings::catalogued(key) {
        Some(entry) => match entry.status {
            crcbl::settings::KeyStatus::Read => "read",
            // Named by the catalogue and read by nothing yet — a control a
            // settings screen must label rather than offer silently.
            crcbl::settings::KeyStatus::Named => "named",
        },
        None if key.starts_with(ENGINE_PREFIX) => "unknown",
        None => "game",
    }
}

fn flatten(file: &StorageSettingsFile) -> (Vec<Json>, Vec<String>) {
    let mut entries = Vec::new();
    let mut stray = Vec::new();
    let mut pending: Vec<_> = file
        .table()
        .iter()
        .rev()
        .map(|(name, value)| (name.clone(), value))
        .collect();

    while let Some((key, value)) = pending.pop() {
        if let Some(section) = value.as_table() {
            for (name, child) in section.iter().rev() {
                pending.push((format!("{key}.{name}"), child));
            }
            continue;
        }
        let (kind, rendered) = if let Some(value) = value.as_bool() {
            ("boolean", Json::Bool(value))
        } else if let Some(value) = value.as_integer() {
            ("integer", Json::Number(value))
        } else if let Some(value) = value.as_float() {
            ("float", Json::Double(value))
        } else if let Some(value) = value.as_str() {
            ("string", Json::string(value))
        } else {
            // A list or a date. `set` writes neither and `get` renders
            // neither, so it is reported as the text TOML spells it with —
            // which is what the human dump above shows too.
            ("other", Json::string(value.to_string()))
        };
        let status = status_of(&key);
        if status == "unknown" {
            stray.push(key.clone());
        }
        entries.push(Json::Object(vec![
            ("status", Json::string(status)),
            ("key", Json::string(key)),
            ("type", Json::string(kind)),
            ("value", rendered),
        ]));
    }
    (entries, stray)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The typing rule, spelled out.**
    ///
    /// The pairs are written here rather than derived from anything
    /// [`Value::parse`] consults, because a table used as its own oracle cannot
    /// fail. Each row is also a promise to whoever has already typed it: the
    /// day `false` starts landing as the string `"false"` is the day every
    /// `[engine.video]` row a player wrote stops doing anything, and nothing
    /// else in either crate would notice.
    #[test]
    fn a_value_is_typed_by_the_toml_spelling_it_was_written_in() {
        for (raw, expected) in [
            ("true", Value::Bool(true)),
            ("false", Value::Bool(false)),
            ("0", Value::Integer(0)),
            ("42", Value::Integer(42)),
            ("-7", Value::Integer(-7)),
            ("+7", Value::Integer(7)),
            ("1_000", Value::Integer(1000)),
            ("0x2a", Value::Integer(42)),
            ("0o17", Value::Integer(15)),
            ("0b101", Value::Integer(5)),
            ("1.5", Value::Float(1.5)),
            ("-0.25", Value::Float(-0.25)),
            ("2e3", Value::Float(2000.0)),
            ("1.0", Value::Float(1.0)),
            ("\"true\"", Value::Text("true".into())),
            ("\"42\"", Value::Text("42".into())),
            ("\"\"", Value::Text(String::new())),
            ("", Value::Text(String::new())),
            ("hello", Value::Text("hello".into())),
            ("hello world", Value::Text("hello world".into())),
            ("Ada", Value::Text("Ada".into())),
            ("2026-08-23", Value::Text("2026-08-23".into())),
            ("[1, 2]", Value::Text("[1, 2]".into())),
            ("{ a = 1 }", Value::Text("{ a = 1 }".into())),
            ("True", Value::Text("True".into())),
            ("TRUE", Value::Text("TRUE".into())),
        ] {
            assert_eq!(Value::parse(raw), expected, "`{raw}`");
        }

        // `inf` and `nan` are TOML floats and one of them is not equal to
        // itself, so they are asserted on their kind rather than their value.
        for raw in ["inf", "-inf", "nan"] {
            assert_eq!(Value::parse(raw).type_name(), "float", "`{raw}`");
        }
    }

    /// **Quoting is the escape hatch**, and it is the only one: a value that
    /// would otherwise be a boolean or a number is text when it is quoted, and
    /// the quotes are not part of it.
    #[test]
    fn a_quoted_value_is_text_and_keeps_none_of_its_quotes() {
        for (raw, text) in [
            ("\"true\"", "true"),
            ("\"false\"", "false"),
            ("\"1.5\"", "1.5"),
            ("'42'", "42"),
            ("\"a b\"", "a b"),
        ] {
            let value = Value::parse(raw);
            assert_eq!(value, Value::Text(text.into()), "`{raw}`");
            assert_eq!(value.bare(), text, "`{raw}`");
        }
    }

    /// **A value spanning lines is text, and cannot type itself from a line
    /// that was never about it.**
    ///
    /// [`Value::parse`] reaches TOML's grammar through a one-line document, and
    /// a raw value carrying a newline could otherwise finish that line early
    /// and put a whole second table after it — so the value would be typed by
    /// `true` while the user typed something else entirely.
    #[test]
    fn a_value_carrying_a_newline_is_stored_as_the_text_it_is() {
        for raw in ["true\n[other]\nx = 1", "42\n", "\rtrue"] {
            assert_eq!(Value::parse(raw), Value::Text(raw.into()), "{raw:?}");
        }
    }

    /// **An integer is never reported as a float.**
    ///
    /// A TOML integer deserializes into an `f64` without complaint, so the
    /// order [`Value::read`] asks in is the whole of this: ask for a float
    /// first and `vsync_delay = 1` reads back as `1.0`, and `set` would write
    /// that back into the player's file.
    #[test]
    fn each_kind_reads_back_as_itself_and_not_as_a_kind_that_would_accept_it() {
        for (toml, expected) in [
            ("k = true", Value::Bool(true)),
            ("k = 1", Value::Integer(1)),
            ("k = 0", Value::Integer(0)),
            ("k = 1.0", Value::Float(1.0)),
            ("k = \"1\"", Value::Text("1".into())),
        ] {
            let storage = MemoryStorage::new();
            storage
                .write(Path::new(SETTINGS_FILE), toml.as_bytes())
                .expect("memory storage accepts every write");
            let stack = SettingsStack::from_storage(&storage);
            assert_eq!(Value::read(&stack, "k"), Some(expected), "{toml}");
        }
    }

    /// **A key that is absent and a key holding something unreadable are
    /// different answers**, and `contains` is the only thing that tells them
    /// apart — which is what `get` branches on.
    #[test]
    fn a_list_reads_as_absent_and_still_answers_contains() {
        let storage = MemoryStorage::new();
        storage
            .write(Path::new(SETTINGS_FILE), b"size = [1920, 1080]\n")
            .expect("memory storage accepts every write");
        let stack = SettingsStack::from_storage(&storage);

        assert_eq!(Value::read(&stack, "size"), None);
        assert!(stack.contains("size"), "the key is in the file");
        assert!(!stack.contains("missing"), "and this one is not");
    }

    /// **Every kind survives being written to a file and read back**, through
    /// the real serializer and the real parser rather than a table in memory:
    /// the type is only useful if it is still that type after the round trip.
    #[test]
    fn every_kind_round_trips_through_a_real_settings_file() {
        for raw in ["true", "false", "42", "-7", "1.5", "Ada", "\"42\""] {
            let value = Value::parse(raw);
            let storage = MemoryStorage::new();

            let mut stack = SettingsStack::from_storage(&storage);
            value
                .write(&mut stack, "engine.video.probe")
                .expect("a fresh stack has a user layer");
            stack
                .save(&storage, Path::new(SETTINGS_FILE))
                .expect("memory storage accepts every write");

            let reloaded = SettingsStack::from_storage(&storage);
            assert_eq!(
                Value::read(&reloaded, "engine.video.probe"),
                Some(value.clone()),
                "`{raw}` did not survive the file"
            );
            assert_eq!(
                Value::read(&reloaded, "engine.video.probe")
                    .expect("just asserted")
                    .type_name(),
                value.type_name(),
                "`{raw}`"
            );
        }
    }

    /// The four renderings agree with each other: what a person sees, what a
    /// script parses, and what the value is called.
    #[test]
    fn the_human_and_json_renderings_of_a_value_are_the_same_value() {
        for (value, kind, bare, json) in [
            (Value::Bool(false), "boolean", "false", Json::Bool(false)),
            (Value::Integer(-7), "integer", "-7", Json::Number(-7)),
            (Value::Float(1.0), "float", "1.0", Json::Double(1.0)),
            (Value::Text("42".into()), "string", "42", Json::string("42")),
        ] {
            assert_eq!(value.type_name(), kind);
            assert_eq!(value.bare(), bare);
            assert_eq!(value.json(), json);
        }
    }

    /// `list`'s machine-readable half reports a dotted key per scalar, in a
    /// stable order, and reports a list rather than dropping it.
    #[test]
    fn flatten_reports_one_record_per_scalar_under_its_dotted_key() {
        let storage = MemoryStorage::new();
        storage
            .write(
                Path::new(SETTINGS_FILE),
                b"[game]\nname = \"Ada\"\n\n[engine.video]\nshadows = false\nsize = [1920, 1080]\n",
            )
            .expect("memory storage accepts every write");
        let file = StorageSettingsFile::load(&storage, Path::new(SETTINGS_FILE))
            .expect("a file this test wrote");

        let (records, stray) = flatten(&file);
        let rendered: Vec<String> = records.iter().map(Json::to_string).collect();
        assert_eq!(
            rendered,
            vec![
                r#"{"status":"read","key":"engine.video.shadows","type":"boolean","value":false}"#
                    .to_string(),
                r#"{"status":"unknown","key":"engine.video.size","type":"other","value":"[1920, 1080]"}"#
                    .to_string(),
                r#"{"status":"game","key":"game.name","type":"string","value":"Ada"}"#.to_string(),
            ]
        );
        // `size` is the typo shape this exists for: the catalogue's row is
        // `resolution`, so nothing will ever read what was written here, and
        // before the status column said so nothing in the tree could tell a
        // person that.
        assert_eq!(stray, vec!["engine.video.size".to_string()]);
    }

    /// **Each of the four statuses, against a key that really has it.**
    ///
    /// Written as the pairs rather than derived from the catalogue: deriving it
    /// would make the test agree with whatever `status_of` did, which is the
    /// one thing it is here to check.
    #[test]
    fn a_key_is_read_named_unknown_or_the_game_own() {
        for (key, wanted) in [
            ("engine.video.shadows", "read"),
            ("engine.video.render_scale", "read"),
            ("engine.video.anisotropic_filtering", "read"),
            ("engine.audio.music_volume", "read"),
            ("engine.video.display_mode", "named"),
            ("engine.video.shadow", "unknown"),
            ("engine.audio.music", "unknown"),
            ("game.difficulty", "game"),
        ] {
            assert_eq!(status_of(key), wanted, "`{key}`");
        }
    }
}
