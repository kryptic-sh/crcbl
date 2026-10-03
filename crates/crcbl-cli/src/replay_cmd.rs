//! `crcbl replay` — read a `.crpl` file and dump its metadata.
//!
//! P4 exit criterion: a recorded session replays identically.  This command is
//! the first consumer of the replay format from outside `crcbl-store` — it reads
//! a file, validates it, and prints what's inside: the entries' ticks, and from
//! format version 2 the input section's simulation sets and how many state
//! hashes it holds. Version 3's peer track is read and checked like the rest of
//! the file, and not reported.
//!
//! It does not re-simulate. `crcbl_server::Host::resimulate` does, but it needs
//! a host built like the recorded one — the game's world, module and registry —
//! and the CLI has no way to build a game's host without the game's code
//! (`docs/backlog.md`, the replay input section's entry).

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

    let mut human = format!(
        "replay: {} ticks at {} Hz, format version {}",
        transport.len(),
        transport.tick_rate(),
        transport.format_version(),
    );
    for (i, tick) in tick_ids.iter().enumerate() {
        human.push_str(&format!("\n  [{i}] tick {tick}"));
    }
    human.push_str(&format!("\nsim sets: {}", transport.sim_sets().len()));
    for recorded in transport.sim_sets() {
        human.push_str(&format!(
            "\n  tick {}: {} {}",
            recorded.tick.get(),
            recorded.set.name,
            recorded.set.value
        ));
    }
    human.push_str(&format!(
        "\nstate hashes: {}",
        transport.state_hashes().len()
    ));

    let sim_sets = transport
        .sim_sets()
        .iter()
        .map(|recorded| {
            Json::Object(vec![
                ("tick", Json::Number(recorded.tick.get() as i64)),
                ("name", Json::string(recorded.set.name.as_str())),
                ("value", Json::string(recorded.set.value.as_str())),
            ])
        })
        .collect();

    let json_fields = vec![
        ("tick_rate", Json::Number(transport.tick_rate() as i64)),
        ("tick_count", Json::Number(transport.len() as i64)),
        (
            "tick_ids",
            Json::Array(tick_ids.into_iter().map(Json::Number).collect()),
        ),
        (
            "format_version",
            Json::Number(i64::from(transport.format_version())),
        ),
        ("sim_sets", Json::Array(sim_sets)),
        (
            "state_hash_count",
            Json::Number(transport.state_hashes().len() as i64),
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
    use crcbl::net::ConsoleSet;
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
    fn replay_command_reports_the_input_section() {
        let storage = MemoryStorage::new();
        let mut writer = ReplayWriter::new(60);
        writer.push_tick(TickId::from_raw(1), b"data");
        writer.push_sim_set(
            TickId::from_raw(1),
            ConsoleSet {
                name: "sv_spin_rate".to_owned(),
                value: "2.5".to_owned(),
            },
        );
        writer.push_state_hash(TickId::from_raw(1), 7);
        let path = Path::new("inputs.crpl");
        writer.write(&storage, path).unwrap();

        let dir = std::env::temp_dir().join("crcbl-replay-input-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("inputs.crpl");
        std::fs::write(&file_path, storage.read(path).unwrap()).unwrap();

        let outcome = run(&ReplayArgs {
            file: file_path,
            json: true,
        })
        .unwrap();
        assert!(
            outcome.human.contains("format version 3"),
            "{}",
            outcome.human
        );
        assert!(
            outcome
                .human
                .ends_with("sim sets: 1\n  tick 1: sv_spin_rate 2.5\nstate hashes: 1"),
            "{}",
            outcome.human
        );
        assert_eq!(outcome.json[3], ("format_version", Json::Number(3)));
        assert_eq!(
            outcome.json[4],
            (
                "sim_sets",
                Json::Array(vec![Json::Object(vec![
                    ("tick", Json::Number(1)),
                    ("name", Json::string("sv_spin_rate")),
                    ("value", Json::string("2.5")),
                ])])
            )
        );
        assert_eq!(outcome.json[5], ("state_hash_count", Json::Number(1)));
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
