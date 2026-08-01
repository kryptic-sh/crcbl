//! `crcbl replay` — read a `.crpl` file and dump its metadata.
//!
//! P4 exit criterion: a recorded session replays identically.  This command is
//! the first consumer of the replay format from outside `crcbl-store` — it reads
//! a file, validates it, and prints what's inside.

use std::path::{Path, PathBuf};

use crcbl_store::NativeStorage;
use crcbl_store::replay::FileTransport;

use crate::args::ReplayArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs `crcbl replay`.
pub fn run(args: &ReplayArgs) -> Result<Outcome, Failure> {
    // `NativeStorage` is a sandbox: a key may not escape its root, so an
    // absolute path is not a valid key. A CLI argument names a file anywhere on
    // disk, so the root is the file's own directory and the key is its name.
    let path = Path::new(&args.file);
    let file_name = path
        .file_name()
        .ok_or_else(|| Failure::new(format!("not a replay file: {}", path.display())))?;
    let root = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let storage = NativeStorage::at(root);

    let transport = FileTransport::open(&storage, Path::new(file_name))
        .map_err(|e| Failure::new(format!("cannot open replay: {e}")))?;

    let tick_ids: Vec<i64> = (0..transport.len())
        .map(|i| transport.tick_at(i).get() as i64)
        .collect();

    let human = format!(
        "replay: {} ticks at {} Hz\n{}",
        transport.len(),
        transport.tick_rate(),
        tick_ids
            .iter()
            .enumerate()
            .map(|(i, tick)| format!("  [{i}] tick {tick}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let json_fields = vec![
        ("tick_rate", Json::Number(transport.tick_rate() as i64)),
        ("tick_count", Json::Number(transport.len() as i64)),
        (
            "tick_ids",
            Json::Array(tick_ids.into_iter().map(Json::Number).collect()),
        ),
    ];

    Ok(Outcome {
        human,
        json: json_fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::core::TickId;
    use crcbl_store::replay::ReplayWriter;
    use crcbl_store::{MemoryStorage, StorageSource};
    use std::path::PathBuf;

    #[test]
    fn replay_command_reads_native_file() {
        let storage = MemoryStorage::new();
        let mut writer = ReplayWriter::new(60);
        writer.push_tick(TickId::from_raw(1), b"data");
        writer.push_tick(TickId::from_raw(2), b"data");
        let path = Path::new("test.crpl");
        writer.write(&storage, path).unwrap();

        let dir = std::env::temp_dir().join("crcbl-replay-test");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test.crpl");
        std::fs::write(&file_path, storage.read(path).unwrap()).unwrap();

        let args = ReplayArgs {
            file: file_path,
            json: false,
        };
        let outcome = run(&args).unwrap();
        assert!(outcome.human.contains("2 ticks"));
        assert!(outcome.human.contains("tick 1"));
        assert!(outcome.human.contains("tick 2"));

        let json_args = ReplayArgs {
            file: args.file,
            json: true,
        };
        let outcome2 = run(&json_args).unwrap();
        assert_eq!(outcome2.json[0], ("tick_rate", Json::Number(60)));
        assert_eq!(outcome2.json[1], ("tick_count", Json::Number(2)));
    }

    #[test]
    fn replay_command_rejects_bad_file() {
        let args = ReplayArgs {
            file: PathBuf::from("/nonexistent.crpl"),
            json: false,
        };
        assert!(run(&args).is_err());
    }
}
