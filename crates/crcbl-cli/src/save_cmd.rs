//! `crcbl save` — a game's save files, inspected from outside the game.
//!
//! The persistence plan's exit criterion was "`crcbl save dump/diff` works on
//! any save", and topic 11 (`docs/notes/tooling.md`) is why it must: a save a
//! person can only look at by running the game is state no script, CI job or
//! bug report can read.
//!
//! # The game's reader, and nothing beside it
//!
//! Every file goes through [`SaveReader::open`], the call a game makes — so an
//! older format version is migrated in memory exactly as the game would
//! migrate it, the file on disk is never rewritten (the container's own rule:
//! opening is a read), and a damaged or newer file is refused with the text
//! [`FormatError`](crcbl_store::save::FormatError) gives the game. A reader of
//! this CLI's own would be a second opinion about what a save is, and the day
//! the two disagreed, this would be reporting on a file the game cannot open.
//!
//! # The container, not the payload
//!
//! A sector's bytes are each game's own encoding — towers' `TWRS`, shard's
//! character — with its own magic and version, and the owner deferred a hook
//! for a game to decode them here (2026-10-05). So a sector is shown by its
//! coordinates, its length and a SHA-256 of its bytes, which is enough to tell
//! two saves apart, and `--hex` previews its first bytes; nothing decodes them.

use std::path::{Path, PathBuf};

use crcbl::shaders::sha256::sha256_hex;
use crcbl_store::save::{SAVE_FORMAT_VERSION, SaveData, SaveHeader, SaveReader, SectorSave};
use crcbl_store::{NativeStorage, StorageError};

use crate::json::Json;
use crate::report::{EXIT_OK, Failure, Outcome};
use crate::save_args::{SaveAction, SaveArgs, SaveDir};

mod diff;

/// How many bytes of each sector `dump --hex` shows.
///
/// A preview, not a hex dump: enough to see a payload's magic and version,
/// which is what tells one game's sector from another's, and bounded so a
/// multi-megabyte sector does not bury the table it sits in.
pub const HEX_PREVIEW_BYTES: usize = 64;

/// How many bytes one line of the human hex preview holds.
const HEX_ROW_BYTES: usize = 16;

/// Runs `crcbl save`, with the code a result that worked exits with — which
/// is [`EXIT_OK`] except for a `diff` that found the saves different.
pub fn run(args: &SaveArgs) -> (Result<Outcome, Failure>, u8) {
    let (mut result, success) = match &args.action {
        SaveAction::List(dir) => (list(dir), EXIT_OK),
        SaveAction::Dump { file, hex } => (dump(file, *hex), EXIT_OK),
        SaveAction::Diff { a, b } => diff::run(a, b),
    };
    // First, and on a failure too, so a consumer knows which branch answered
    // before it reads anything else.
    let action = ("action", Json::string(args.action.name()));
    match &mut result {
        Ok(outcome) => outcome.json.insert(0, action),
        Err(failure) => failure.json.insert(0, action),
    }
    (result, success)
}

// ── The three branches ─────────────────────────────────────────────────────

/// `crcbl save list`.
fn list(dir: &SaveDir) -> Result<Outcome, Failure> {
    let (root, app) = match dir {
        SaveDir::Dir(dir) => (dir.clone(), None),
        SaveDir::App(app) => (data_root(app)?, Some(app.clone())),
        SaveDir::Project => {
            let app = crate::settings_cmd::app_name(None, "saves")?;
            (data_root(&app)?, Some(app))
        }
    };
    let common = |dir_exists: bool| {
        vec![
            ("app", app.as_deref().map_or(Json::Null, Json::string)),
            ("dir", Json::string(root.display().to_string())),
            ("dir_exists", Json::Bool(dir_exists)),
        ]
    };
    let heading = match &app {
        Some(app) => format!("saves for `{app}` in {}", root.display()),
        None => format!("saves in {}", root.display()),
    };

    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        // A game that has never saved has no directory, and that is an answer
        // rather than a failure — the same answer `settings list` gives for a
        // file the player never wrote.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut json = common(false);
            json.push(("count", Json::Unsigned(0)));
            json.push(("saves", Json::Array(Vec::new())));
            return Ok(Outcome {
                human: format!("{heading} (no directory yet)"),
                json,
            });
        }
        Err(error) => {
            return Err(Failure::new(format!(
                "cannot read the directory {}: {error}",
                root.display()
            ))
            .with("dir", Json::string(root.display().to_string())));
        }
    };

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            Failure::new(format!(
                "cannot read the directory {}: {error}",
                root.display()
            ))
        })?;
        let is_file = entry
            .file_type()
            .map_err(|error| {
                Failure::new(format!("cannot read {}: {error}", entry.path().display()))
            })?
            .is_file();
        if is_file {
            names.push(entry.file_name());
        }
    }
    names.sort();

    let rows: Vec<(String, Row)> = names
        .iter()
        .map(|name| {
            (
                name.to_string_lossy().into_owned(),
                Row::of(&root.join(name)),
            )
        })
        .collect();
    let width = rows.iter().map(|(name, _)| name.chars().count()).max();
    let human = match width {
        None => format!("{heading} (no files)"),
        Some(width) => {
            let lines: Vec<String> = rows
                .iter()
                .map(|(name, row)| format!("  {name:<width$}  {}", row.human()))
                .collect();
            format!("{heading}\n{}", lines.join("\n"))
        }
    };

    let mut json = common(true);
    json.push(("count", Json::Unsigned(rows.len() as u64)));
    json.push((
        "saves",
        Json::Array(rows.iter().map(|(name, row)| row.json(name)).collect()),
    ));
    Ok(Outcome { human, json })
}

/// `crcbl save dump`.
fn dump(file: &Path, hex: bool) -> Result<Outcome, Failure> {
    let data = open(file).map_err(|error| refused(file, &error))?;
    let size = size_of(file).map_err(|error| refused(file, &error))?;
    let header = &data.header;

    let mut lines = vec![
        format!("{}: {size} bytes", file.display()),
        format!("format version  {}", version_text(&data)),
        format!("tick            {}", header.tick.get()),
        format!("playtime        {}", playtime(header).1),
        format!("engine version  {}", engine(header).1),
        format!("scene           {}", scene(header).1),
        format!("sectors         {}", data.sectors.len()),
    ];
    let mut sectors = Vec::with_capacity(data.sectors.len());
    for sector in &data.sectors {
        let bytes = &sector.snapshot_data;
        let digest = sha256_hex(bytes);
        lines.push(format!(
            "  {}  {} bytes  sha256 {digest}",
            sector_name(sector),
            bytes.len()
        ));
        let mut record = vec![
            ("sector", sector_coordinates(sector)),
            ("length", Json::Unsigned(bytes.len() as u64)),
            ("sha256", Json::string(digest)),
        ];
        if hex {
            let shown = &bytes[..bytes.len().min(HEX_PREVIEW_BYTES)];
            for row in shown.chunks(HEX_ROW_BYTES) {
                let pairs: Vec<String> = row.iter().map(|byte| format!("{byte:02x}")).collect();
                lines.push(format!("      {}", pairs.join(" ")));
            }
            let hidden = bytes.len() - shown.len();
            if hidden > 0 {
                lines.push(format!("      … and {hidden} more bytes"));
            }
            record.push(("hex", Json::string(hex_text(shown))));
            record.push(("hex_truncated", Json::Bool(hidden > 0)));
        }
        sectors.push(Json::Object(record));
    }

    Ok(Outcome {
        human: lines.join("\n"),
        json: vec![
            ("file", Json::string(file.display().to_string())),
            ("size", Json::Unsigned(size)),
            ("format_version", Json::Number(data.format_version.into())),
            (
                "current_format_version",
                Json::Number(SAVE_FORMAT_VERSION.into()),
            ),
            ("migrated", Json::Bool(migrated(&data))),
            ("tick", Json::Unsigned(header.tick.get())),
            ("playtime_secs", playtime(header).0),
            ("engine_version", engine(header).0),
            ("scene", scene(header).0),
            ("sector_count", Json::Unsigned(data.sectors.len() as u64)),
            ("sectors", Json::Array(sectors)),
        ],
    })
}

// ── Opening a file ─────────────────────────────────────────────────────────

/// The save at `path`, read through the game's own reader.
///
/// [`NativeStorage`] is rooted at a directory, so the file's directory is the
/// root and its name the key — `replay`'s rule for a path typed on a command
/// line.
fn open(path: &Path) -> Result<SaveData, StorageError> {
    let Some(name) = path.file_name() else {
        return Err(StorageError::Other(format!(
            "{} names no file",
            path.display()
        )));
    };
    let root = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    SaveReader::open(&NativeStorage::at(root), Path::new(name)).map(SaveReader::into_data)
}

/// The size of the file at `path`, in bytes.
fn size_of(path: &Path) -> Result<u64, StorageError> {
    Ok(std::fs::metadata(path)?.len())
}

/// A file that did not open, named, with the reason the reader gave.
fn refused(file: &Path, error: &StorageError) -> Failure {
    Failure::new(format!("cannot open {}: {error}", file.display()))
        .with("file", Json::string(file.display().to_string()))
}

/// The platform's data directory for `app`, where its saves are.
fn data_root(app: &str) -> Result<PathBuf, Failure> {
    NativeStorage::data_root(app).ok_or_else(|| {
        Failure::new(format!(
            "this platform names no data directory, so there is nowhere for `{app}`'s saves \
             to be\nhint: `--dir <DIR>` names the directory."
        ))
    })
}

// ── One file in a listing ──────────────────────────────────────────────────

/// What `list` says about one file: its size, and its header or why it did
/// not open.
enum Row {
    /// A save that opened.
    Save { size: u64, data: SaveData },
    /// A file that did not, and the reason — the reader's, or the
    /// filesystem's.
    Refused { size: Option<u64>, reason: String },
}

impl Row {
    fn of(path: &Path) -> Self {
        let size = size_of(path);
        match (open(path), size) {
            (Ok(data), Ok(size)) => Self::Save { size, data },
            (Err(error), size) => Self::Refused {
                size: size.ok(),
                reason: error.to_string(),
            },
            (Ok(_), Err(error)) => Self::Refused {
                size: None,
                reason: error.to_string(),
            },
        }
    }

    fn human(&self) -> String {
        match self {
            Self::Save { size, data } => {
                // `dump`'s sentence about the file on disk is too long for a
                // row, and `list` only reads too.
                let migrated = if migrated(data) {
                    format!(" (migrated to {SAVE_FORMAT_VERSION})")
                } else {
                    String::new()
                };
                format!(
                    "{size} bytes  tick {}  playtime {}  format {}{migrated}  engine {}",
                    data.header.tick.get(),
                    playtime(&data.header).1,
                    data.format_version,
                    engine(&data.header).1
                )
            }
            Self::Refused { size, reason } => match size {
                Some(size) => format!("{size} bytes  refused: {reason}"),
                None => format!("refused: {reason}"),
            },
        }
    }

    /// The record `--json` lists, keyed on `ok`: a save's header fields when
    /// it opened, and the refusal in `error` when it did not.
    fn json(&self, name: &str) -> Json {
        match self {
            Self::Save { size, data } => Json::Object(vec![
                ("file", Json::string(name)),
                ("size", Json::Unsigned(*size)),
                ("ok", Json::Bool(true)),
                ("format_version", Json::Number(data.format_version.into())),
                ("migrated", Json::Bool(migrated(data))),
                ("tick", Json::Unsigned(data.header.tick.get())),
                ("playtime_secs", playtime(&data.header).0),
                ("engine_version", engine(&data.header).0),
            ]),
            Self::Refused { size, reason } => Json::Object(vec![
                ("file", Json::string(name)),
                ("size", size.map_or(Json::Null, Json::Unsigned)),
                ("ok", Json::Bool(false)),
                ("error", Json::string(reason)),
            ]),
        }
    }
}

// ── The header, rendered ───────────────────────────────────────────────────

/// Whether the file was written at an older format version and migrated on
/// open.
fn migrated(data: &SaveData) -> bool {
    data.format_version != SAVE_FORMAT_VERSION
}

/// The file's format version, saying so when it was migrated — which is in
/// memory only: the reader never writes.
fn version_text(data: &SaveData) -> String {
    if migrated(data) {
        format!(
            "{}, migrated in memory to {SAVE_FORMAT_VERSION}; the file is unchanged",
            data.format_version
        )
    } else {
        data.format_version.to_string()
    }
}

/// The playtime, as JSON and as text.
fn playtime(header: &SaveHeader) -> (Json, String) {
    // `{:?}`: the shortest decimal that reads back as the same bits, which is
    // `Json::Double`'s spelling too.
    (
        Json::Double(header.playtime_secs),
        format!("{:?} s", header.playtime_secs),
    )
}

/// The engine version, as JSON and as text.
fn engine(header: &SaveHeader) -> (Json, String) {
    match &header.engine_version {
        Some(version) => (Json::string(version), version.clone()),
        None => (Json::Null, "not recorded".to_owned()),
    }
}

/// The scene reference, as JSON and as text.
fn scene(header: &SaveHeader) -> (Json, String) {
    match &header.scene {
        Some(scene) => {
            let hash = hex_text(&scene.content_hash);
            (
                Json::Object(vec![
                    ("name", Json::string(&scene.name)),
                    ("content_hash", Json::string(&hash)),
                ]),
                format!("{}, content sha256 {hash}", scene.name),
            )
        }
        None => (Json::Null, "none".to_owned()),
    }
}

/// `bytes` as lower-case hex, two digits a byte and nothing between them.
fn hex_text(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sector_name(sector: &SectorSave) -> String {
    let id = sector.sector_id;
    format!("({}, {}, {})", id.x, id.y, id.z)
}

fn sector_coordinates(sector: &SectorSave) -> Json {
    let id = sector.sector_id;
    Json::Array(vec![
        Json::Number(id.x),
        Json::Number(id.y),
        Json::Number(id.z),
    ])
}
