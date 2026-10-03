//! A co-op session recorded to a `.crpl` file and re-simulated from it: two
//! players on one host over in-process transports, building and sending a
//! wave; a fresh host handed the file's roster and frames
//! (`crcbl::server::Host::resimulate`), its state hash — the stage's, through
//! [`FieldReplica`](super::FieldReplica) — compared at the end of every tick.

use std::path::Path;
use std::time::Duration;

use crcbl::client::Client;
use crcbl::core::{FrameClock, TickId};
use crcbl::ecs::World;
use crcbl::net::InMemoryTransport;
use crcbl::server::sim_hash::hash_world;
use crcbl::server::{
    Host, HostConfig, PeerEvent, PeerFrames, PeerId, ResimError, RosterChange, TickInputs,
};
use crcbl::store::MemoryStorage;
use crcbl::store::replay::{
    FileTransport, RecordedPeerFrames, RecordedPeerTick, RecordedRosterChange, ReplayWriter,
    RosterChangeKind,
};

use super::{COMPATIBILITY, Controls, DEFAULT_TICK_HZ, Field, Intent};
use crate::map::Map;
use crate::tower;

const TICK_HZ: u32 = DEFAULT_TICK_HZ;

/// How many ticks a session runs: long enough for the wave sent to walk into
/// both towers' reach and be shot at.
const SESSION_TICKS: u32 = 600;

/// The step, counted from the one both players joined at, on which each
/// player builds — both on the same step, so both commands reach the same
/// host tick and the order the host admitted them is the order they apply.
const BUILD_STEP: u32 = 10;

/// The step on which the second player sends the wave.
const WAVE_STEP: u32 = 20;

/// A host on the committed map, as a LAN host builds one, and its stage.
fn host() -> (Host, Field) {
    let (field, world, module) = Field::open(&Map::built_in(), TICK_HZ);
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 2,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    host.set_module(Box::new(module));
    host.update(Duration::ZERO);
    (host, field)
}

/// What each player asks for on `step`, counted from the join.
fn controls(player: usize, step: u32) -> Controls {
    match (player, step) {
        (0, BUILD_STEP) => Controls {
            place: Some(0),
            kind: tower::Kind::Bolt,
            ..Controls::default()
        },
        (1, BUILD_STEP) => Controls {
            place: Some(1),
            kind: tower::Kind::Splash,
            ..Controls::default()
        },
        (1, WAVE_STEP) => Controls {
            start_wave: true,
            ..Controls::default()
        },
        _ => Controls::default(),
    }
}

/// A recorded session: its file, the tick both players joined at, and the
/// tick the first tower went up — the first a player's frame mattered.
struct Session {
    file: FileTransport,
    joined: TickId,
    built: TickId,
    last: TickId,
    final_hash: u64,
}

/// Plays the session and records it to a `.crpl` file as a recorder would:
/// the state hash at the end of every tick, and the host's input record.
fn record() -> Session {
    let (mut host, field) = host();
    host.record_peer_inputs();
    let mut clients: Vec<Client<InMemoryTransport>> = (0..2)
        .map(|_| {
            let (near, far) = InMemoryTransport::pair();
            host.add(Box::new(far));
            Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY)
        })
        .collect();
    let period = FrameClock::new(TICK_HZ).tick_dt();
    let mut writer = ReplayWriter::new(TICK_HZ);
    let mut now = Duration::ZERO;
    let mut joined: Vec<(TickId, PeerId)> = Vec::new();
    let mut built = None;
    for _ in 0..SESSION_TICKS {
        now += period;
        assert_eq!(host.update(now), 1, "one tick a step");
        let tick = host.tick_id();
        writer.push_state_hash(tick, hash_world(host.world(), tick));
        joined.extend(host.events().filter_map(|event| match event {
            PeerEvent::Joined(peer) => Some((tick, peer)),
            _ => None,
        }));
        if built.is_none() && field.stats().built > 0 {
            built = Some(tick);
        }
        let step = joined.first().map_or(0, |(at, _)| {
            u32::try_from(tick.get() - at.get()).expect("a short run")
        });
        for (player, client) in clients.iter_mut().enumerate() {
            client.set_input(Intent::from(controls(player, step)).to_wire());
            client.update(now);
        }
    }

    let stats = field.stats();
    assert_eq!(stats.built, 2, "both players built");
    assert!(stats.wave >= 1, "the wave was sent");
    assert!(stats.shots > 0, "the towers shot at it");
    let [(joined, _), (second, _)] = joined[..] else {
        panic!("not two joins: {joined:?}");
    };
    assert_eq!(joined, second, "both joined on one tick");
    for entry in host.peer_input_record() {
        writer.push_peer_tick(to_file(entry));
    }
    let storage = MemoryStorage::new();
    let path = Path::new("towers.crpl");
    writer.write(&storage, path).expect("a valid recording");
    let last = host.tick_id();
    Session {
        file: FileTransport::open(&storage, path).expect("it reads back"),
        joined,
        built: built.expect("a tower went up"),
        last,
        final_hash: hash_world(host.world(), last),
    }
}

/// A host's input record as the file carries it.
fn to_file(entry: &TickInputs) -> RecordedPeerTick {
    RecordedPeerTick {
        tick: entry.tick,
        roster: entry
            .roster
            .iter()
            .map(|change| RecordedRosterChange {
                kind: match change {
                    RosterChange::Joined(_) => RosterChangeKind::Joined,
                    RosterChange::Lost(_) => RosterChangeKind::Lost,
                    RosterChange::Resumed(_) => RosterChangeKind::Resumed,
                    RosterChange::Left(_) => RosterChangeKind::Left,
                    RosterChange::Ended(_) => RosterChangeKind::Ended,
                },
                peer: change.peer().get(),
            })
            .collect(),
        peers: entry
            .peers
            .iter()
            .map(|frames| RecordedPeerFrames {
                peer: frames.peer.get(),
                dropped: frames.dropped,
                frames: frames.frames.clone(),
            })
            .collect(),
    }
}

/// …and back, as a re-simulation takes it.
fn from_file(entry: &RecordedPeerTick) -> TickInputs {
    TickInputs {
        tick: entry.tick,
        roster: entry
            .roster
            .iter()
            .map(|change| {
                let peer = PeerId::from_raw(change.peer);
                match change.kind {
                    RosterChangeKind::Joined => RosterChange::Joined(peer),
                    RosterChangeKind::Lost => RosterChange::Lost(peer),
                    RosterChangeKind::Resumed => RosterChange::Resumed(peer),
                    RosterChangeKind::Left => RosterChange::Left(peer),
                    RosterChangeKind::Ended => RosterChange::Ended(peer),
                }
            })
            .collect(),
        peers: entry
            .peers
            .iter()
            .map(|frames| PeerFrames {
                peer: PeerId::from_raw(frames.peer),
                frames: frames.frames.clone(),
                dropped: frames.dropped,
            })
            .collect(),
    }
}

fn hashes(file: &FileTransport) -> Vec<(TickId, u64)> {
    file.state_hashes()
        .iter()
        .map(|recorded| (recorded.tick, recorded.hash))
        .collect()
}

fn inputs(file: &FileTransport) -> Vec<TickInputs> {
    file.peer_ticks().iter().map(from_file).collect()
}

/// The first tick `inputs` makes a fresh host diverge at.
fn diverges_at(session: &Session, inputs: Vec<TickInputs>) -> TickId {
    let (mut replayed, _) = host();
    match replayed.resimulate([], hashes(&session.file), inputs) {
        Err(ResimError::Diverged { tick, .. }) => tick,
        other => panic!("not a divergence: {other:?}"),
    }
}

#[test]
fn a_two_player_session_resimulated_from_its_file_reproduces_every_tick() {
    let session = record();
    assert_eq!(session.file.state_hashes().len(), SESSION_TICKS as usize);
    assert!(!session.file.peer_ticks().is_empty());

    let (mut replayed, field) = host();
    assert_eq!(
        replayed.resimulate([], hashes(&session.file), inputs(&session.file)),
        Ok(session.last)
    );
    assert_eq!(
        hash_world(replayed.world(), session.last),
        session.final_hash
    );
    let stats = field.stats();
    assert_eq!(stats.built, 2);
    assert!(stats.shots > 0);
}

#[test]
fn the_file_without_its_frames_diverges_at_the_first_build() {
    let session = record();
    let mut stripped = inputs(&session.file);
    for entry in &mut stripped {
        entry.peers.clear();
    }
    // The roster stays, so both players are still in the run and it ticks
    // as it did: only their commands are gone, and the first one that did
    // anything built a tower.
    assert!(session.built > session.joined);
    assert_eq!(diverges_at(&session, stripped), session.built);
}

#[test]
fn the_players_admitted_in_the_other_order_diverge_at_the_first_build() {
    let session = record();
    let mut swapped = inputs(&session.file);
    let first = &mut swapped[0];
    assert_eq!(first.tick, session.joined);
    assert!(
        matches!(
            first.roster[..],
            [RosterChange::Joined(_), RosterChange::Joined(_)]
        ),
        "{:?}",
        first.roster
    );
    first.roster.swap(0, 1);
    // Each player keeps its own commands; the two builds just apply in the
    // other order, and the stage's towers stand in it.
    assert_eq!(diverges_at(&session, swapped), session.built);
}
