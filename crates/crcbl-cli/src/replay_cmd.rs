//! `crcbl replay` — read a `.crpl` file and dump its metadata.
//!
//! P4 exit criterion: a recorded session replays identically.  This command is
//! the first consumer of the replay format from outside `crcbl-store` — it reads
//! a file, validates it, and prints what's inside: the entries' ticks, and from
//! format version 2 the input section's simulation sets and how many state
//! hashes it holds, and from version 3 its peer track — how many ticks have an
//! entry, every roster change, and each peer's frame and dropped counts. The
//! frames themselves are counted, not printed: they are a game's own bytes,
//! and a session holds thousands.
//!
//! It does not re-simulate. `crcbl_server::Host::resimulate` does, but it needs
//! a host built like the recorded one — the game's world, module and registry —
//! and the CLI has no way to build a game's host without the game's code
//! (`docs/backlog.md`, the replay input section's entry).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crcbl_store::NativeStorage;
use crcbl_store::replay::{FileTransport, RecordedPeerTick, RosterChangeKind};

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
    let peers = PeerTrack::of(transport.peer_ticks());
    peers.describe(&mut human);

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
        ("peer_track", peers.json()),
    ];

    Ok(Outcome {
        human,
        json: json_fields,
    })
}

/// What a file's peer track says, counted: the ticks with an entry, every
/// roster change in order, and each peer's frames and dropped frames.
struct PeerTrack {
    /// The ticks with an entry, in order.
    ticks: Vec<u64>,
    /// Every roster change: its tick, what it was, and the peer's number.
    roster: Vec<(u64, RosterChangeKind, u64)>,
    /// Each peer's frames and dropped frames over the whole track, by
    /// number.
    peers: BTreeMap<u64, (u64, u64)>,
}

impl PeerTrack {
    fn of(track: &[RecordedPeerTick]) -> Self {
        let mut peers: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
        for entry in track {
            for frames in &entry.peers {
                let counts = peers.entry(frames.peer).or_default();
                counts.0 += frames.frames.len() as u64;
                counts.1 += u64::from(frames.dropped);
            }
        }
        Self {
            ticks: track.iter().map(|entry| entry.tick.get()).collect(),
            roster: track
                .iter()
                .flat_map(|entry| {
                    entry
                        .roster
                        .iter()
                        .map(|change| (entry.tick.get(), change.kind, change.peer))
                })
                .collect(),
            peers,
        }
    }

    fn frames(&self) -> u64 {
        self.peers.values().map(|(frames, _)| frames).sum()
    }

    fn dropped(&self) -> u64 {
        self.peers.values().map(|(_, dropped)| dropped).sum()
    }

    /// Appends the human report: a line of totals, then each roster change,
    /// then each peer's counts.
    fn describe(&self, human: &mut String) {
        human.push_str(&format!(
            "\npeer track: {} ticks, {} roster changes, {} frames, {} dropped",
            self.ticks.len(),
            self.roster.len(),
            self.frames(),
            self.dropped(),
        ));
        if let (Some(first), Some(last)) = (self.ticks.first(), self.ticks.last()) {
            human.push_str(&format!(" (ticks {first} to {last})"));
        }
        for (tick, kind, peer) in &self.roster {
            human.push_str(&format!(
                "\n  tick {tick}: peer {peer} {}",
                kind_name(*kind)
            ));
        }
        for (peer, (frames, dropped)) in &self.peers {
            human.push_str(&format!(
                "\n  peer {peer}: {frames} frames, {dropped} dropped"
            ));
        }
    }

    fn json(&self) -> Json {
        let mut fields = vec![("tick_count", Json::Number(self.ticks.len() as i64))];
        if let (Some(first), Some(last)) = (self.ticks.first(), self.ticks.last()) {
            fields.push(("first_tick", Json::Number(*first as i64)));
            fields.push(("last_tick", Json::Number(*last as i64)));
        }
        fields.push(("frame_count", Json::Number(self.frames() as i64)));
        fields.push(("dropped_count", Json::Number(self.dropped() as i64)));
        fields.push((
            "roster",
            Json::Array(
                self.roster
                    .iter()
                    .map(|(tick, kind, peer)| {
                        Json::Object(vec![
                            ("tick", Json::Number(*tick as i64)),
                            ("change", Json::string(kind_name(*kind))),
                            ("peer", Json::Number(*peer as i64)),
                        ])
                    })
                    .collect(),
            ),
        ));
        fields.push((
            "peers",
            Json::Array(
                self.peers
                    .iter()
                    .map(|(peer, (frames, dropped))| {
                        Json::Object(vec![
                            ("peer", Json::Number(*peer as i64)),
                            ("frame_count", Json::Number(*frames as i64)),
                            ("dropped_count", Json::Number(*dropped as i64)),
                        ])
                    })
                    .collect(),
            ),
        ));
        Json::Object(fields)
    }
}

/// A roster change's kind, as the report words it.
const fn kind_name(kind: RosterChangeKind) -> &'static str {
    match kind {
        RosterChangeKind::Joined => "joined",
        RosterChangeKind::Lost => "lost",
        RosterChangeKind::Resumed => "resumed",
        RosterChangeKind::Left => "left",
        RosterChangeKind::Ended => "ended",
    }
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
            outcome.human.ends_with(
                "sim sets: 1\n  tick 1: sv_spin_rate 2.5\nstate hashes: 1\npeer track: 0 ticks, \
                 0 roster changes, 0 frames, 0 dropped"
            ),
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

    /// **The peer track is reported**: the ticks with an entry, every roster
    /// change by tick, kind and peer, and each peer's frames and dropped
    /// frames — in the human lines and in `--json`.
    #[test]
    fn replay_command_reports_the_peer_track() {
        use crcbl_store::replay::{RecordedPeerFrames, RecordedRosterChange};

        let frames = |peer, count: u64, dropped| RecordedPeerFrames {
            peer,
            dropped,
            frames: (0..count)
                .map(|n| (TickId::from_raw(n), vec![1, 2]))
                .collect(),
        };
        let mut writer = ReplayWriter::new(30);
        writer.push_peer_tick(RecordedPeerTick {
            tick: TickId::from_raw(3),
            roster: vec![
                RecordedRosterChange {
                    kind: RosterChangeKind::Joined,
                    peer: 1,
                },
                RecordedRosterChange {
                    kind: RosterChangeKind::Joined,
                    peer: 2,
                },
            ],
            peers: vec![frames(1, 2, 0)],
        });
        writer.push_peer_tick(RecordedPeerTick {
            tick: TickId::from_raw(7),
            roster: vec![RecordedRosterChange {
                kind: RosterChangeKind::Lost,
                peer: 2,
            }],
            peers: vec![frames(1, 1, 3)],
        });
        let storage = MemoryStorage::new();
        let path = Path::new("peers.crpl");
        writer.write(&storage, path).unwrap();
        let dir = std::env::temp_dir().join("crcbl-replay-peer-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("peers.crpl");
        std::fs::write(&file_path, storage.read(path).unwrap()).unwrap();

        let outcome = run(&ReplayArgs {
            file: file_path,
            json: true,
        })
        .unwrap();
        assert!(
            outcome.human.ends_with(
                "peer track: 2 ticks, 3 roster changes, 3 frames, 3 dropped (ticks 3 to 7)\n  \
                 tick 3: peer 1 joined\n  tick 3: peer 2 joined\n  tick 7: peer 2 lost\n  peer \
                 1: 3 frames, 3 dropped"
            ),
            "{}",
            outcome.human
        );
        let change = |tick, change, peer| {
            Json::Object(vec![
                ("tick", Json::Number(tick)),
                ("change", Json::string(change)),
                ("peer", Json::Number(peer)),
            ])
        };
        assert_eq!(
            outcome.json[6],
            (
                "peer_track",
                Json::Object(vec![
                    ("tick_count", Json::Number(2)),
                    ("first_tick", Json::Number(3)),
                    ("last_tick", Json::Number(7)),
                    ("frame_count", Json::Number(3)),
                    ("dropped_count", Json::Number(3)),
                    (
                        "roster",
                        Json::Array(vec![
                            change(3, "joined", 1),
                            change(3, "joined", 2),
                            change(7, "lost", 2),
                        ])
                    ),
                    (
                        "peers",
                        Json::Array(vec![Json::Object(vec![
                            ("peer", Json::Number(1)),
                            ("frame_count", Json::Number(3)),
                            ("dropped_count", Json::Number(3)),
                        ])])
                    ),
                ])
            )
        );
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
