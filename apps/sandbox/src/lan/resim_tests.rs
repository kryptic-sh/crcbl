//! A sandbox session recorded to a `.crpl` file and re-simulated from it: the
//! sandbox's own [`LanHost`], recording through the engine's
//! `crcbl::lan::LanHost::record`, with two players added over in-process
//! transports — no socket but its listener and announcer, both on loopback —
//! joining, one of them leaving, and an `sv_spin_rate` set from the host's
//! console between; a fresh host built from [`world`] and [`serve`] handed
//! the file (`crcbl::replay_record::resimulate`), its state hash — the
//! players' numbers and the cube's spin — compared at the end of every tick.
//!
//! The sandbox's players send no input, so its recordings carry no frames:
//! what a re-simulation must reproduce is the roster and the set.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use crcbl::client::Client;
use crcbl::core::{FrameClock, TickId};
use crcbl::ecs::World;
use crcbl::net::{ConsoleSet, InMemoryTransport, SessionConfig};
use crcbl::replay_record::{resimulate, tick_inputs};
use crcbl::server::sim_hash::hash_world;
use crcbl::server::{Host, HostConfig, ResimError, RosterChange, TickInputs};
use crcbl::store::NativeStorage;
use crcbl::store::replay::FileTransport;

use super::imp::{COMPATIBILITY, LanHost, MAX_PLAYERS, serve, world};
use super::players::seated;

const TICK_HZ: u32 = 60;

/// How many ticks a session runs: past the leave.
const SESSION_TICKS: u32 = 120;

/// The step on which the host's console sets `sv_spin_rate` to [`RATE`].
const SET_STEP: u32 = 20;

/// The rate set, as the console prints it: not the default, so the cube
/// spins differently from the tick it applies at.
const RATE: &str = "3";

/// The step on which the second player's link drops.
const DROP_STEP: u32 = 40;

/// How long the host keeps a dropped player's place: short, so the session
/// sees it leave.
const GRACE: Duration = Duration::from_millis(250);

/// A host built as a sandbox host builds one, at tick zero, without its
/// transports: what re-simulates a sandbox's recording.
fn host() -> Host {
    let mut host = Host::new(
        world(),
        HostConfig {
            max_peers: usize::from(MAX_PLAYERS),
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    serve(&mut host);
    host.update(Duration::ZERO);
    host
}

/// A recorded session: its file, the tick both players joined at, the tick
/// the set applied at and the tick the second player left at.
struct Session {
    file: FileTransport,
    joined: TickId,
    set: TickId,
    left: TickId,
    last: TickId,
    final_hash: u64,
}

/// The first tick of `file`'s peer track whose roster holds a change `is`
/// picks.
fn tick_of(file: &FileTransport, is: impl Fn(&RosterChange) -> bool) -> TickId {
    inputs(file)
        .into_iter()
        .find(|entry| entry.roster.iter().any(&is))
        .map(|entry| entry.tick)
        .expect("the change is recorded")
}

/// Plays the session on the sandbox's [`LanHost`], recorded from before its
/// first frame as `--record` records one, and reads the file back.
fn record() -> Session {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("sandbox.crpl");
    let loopback: SocketAddr = (Ipv4Addr::LOCALHOST, 0).into();
    let mut lan = LanHost::open(loopback, loopback, None, TICK_HZ)
        .expect("loopback UDP must be available to these tests");
    lan.lan_mut().record(&path).expect("it starts");
    let host = lan.lan_mut().host_mut();
    host.set_session_config(SessionConfig {
        reconnect_grace_period: GRACE,
        ..SessionConfig::default()
    });
    let mut clients: Vec<Client<InMemoryTransport>> = (0..2)
        .map(|_| {
            let (near, far) = InMemoryTransport::pair();
            host.add(Box::new(far));
            Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY)
        })
        .collect();
    let period = FrameClock::new(TICK_HZ).tick_dt();
    let mut now = Duration::ZERO;
    let mut most_seated = 0;
    for step in 0..SESSION_TICKS {
        let before = lan.host().tick_id();
        if step == SET_STEP {
            lan.lan_mut().host_mut().submit_console_set(ConsoleSet {
                name: "sv_spin_rate".to_owned(),
                value: RATE.to_owned(),
            });
        }
        if step == DROP_STEP {
            clients.truncate(1);
        }
        now += period;
        lan.frame(now);
        assert_eq!(
            lan.host().tick_id().get(),
            before.get() + 1,
            "one tick a frame"
        );
        most_seated = most_seated.max(seated(lan.lan_mut().host_mut().world_mut()).len());
        for client in &mut clients {
            client.update(now);
        }
    }

    assert_eq!(most_seated, 2, "both players were seated live");
    let host = lan.lan_mut().host_mut();
    assert_eq!(host.peer_count(), 1, "the second player left");
    assert_eq!(
        seated(host.world_mut()).len(),
        1,
        "and its seat went with it"
    );
    let last = host.tick_id();
    let final_hash = hash_world(host.world(), last);
    lan.lan_mut()
        .stop_recording()
        .expect("it was recording")
        .expect("a valid recording");
    let storage = NativeStorage::at(dir.path().to_path_buf());
    let file = FileTransport::open(&storage, Path::new("sandbox.crpl")).expect("it reads back");

    let first = &inputs(&file)[0];
    assert!(
        matches!(
            first.roster[..],
            [RosterChange::Joined(_), RosterChange::Joined(_)]
        ),
        "both joined on one tick: {:?}",
        first.roster
    );
    let [set] = file.sim_sets() else {
        panic!("not one set: {:?}", file.sim_sets());
    };
    Session {
        joined: first.tick,
        set: set.tick,
        left: tick_of(&file, |change| matches!(change, RosterChange::Left(_))),
        file,
        last,
        final_hash,
    }
}

fn hashes(file: &FileTransport) -> Vec<(TickId, u64)> {
    file.state_hashes()
        .iter()
        .map(|recorded| (recorded.tick, recorded.hash))
        .collect()
}

fn sets(file: &FileTransport) -> Vec<(TickId, ConsoleSet)> {
    file.sim_sets()
        .iter()
        .map(|recorded| (recorded.tick, recorded.set.clone()))
        .collect()
}

fn inputs(file: &FileTransport) -> Vec<TickInputs> {
    file.peer_ticks().iter().map(tick_inputs).collect()
}

/// The first tick a fresh host handed `sets` and `inputs` diverges at.
fn diverges_at(
    session: &Session,
    sets: Vec<(TickId, ConsoleSet)>,
    inputs: Vec<TickInputs>,
) -> TickId {
    match host().resimulate(sets, hashes(&session.file), inputs) {
        Err(ResimError::Diverged { tick, .. }) => tick,
        other => panic!("not a divergence: {other:?}"),
    }
}

#[test]
fn a_sandbox_session_resimulated_from_its_file_reproduces_every_tick() {
    let session = record();
    // The tick the recording started on, and every one after.
    assert_eq!(
        session.file.state_hashes().len(),
        SESSION_TICKS as usize + 1
    );
    assert!(session.joined < session.set && session.set < session.left);

    let mut replayed = host();
    assert_eq!(resimulate(&mut replayed, &session.file), Ok(session.last));
    assert_eq!(
        hash_world(replayed.world(), session.last),
        session.final_hash
    );
    assert_eq!(seated(replayed.world_mut()).len(), 1);
}

#[test]
fn the_file_without_its_leave_diverges_where_the_player_left() {
    let session = record();
    let mut kept = inputs(&session.file);
    for entry in &mut kept {
        entry
            .roster
            .retain(|change| !matches!(change, RosterChange::Left(_)));
    }
    // The player stays seated, lost, and only its seat tells.
    assert_eq!(
        diverges_at(&session, sets(&session.file), kept),
        session.left
    );
}

#[test]
fn the_players_admitted_in_the_other_order_diverge_where_they_joined() {
    let session = record();
    let mut swapped = inputs(&session.file);
    swapped[0].roster.swap(0, 1);
    // The same two seats, each holding the other player's number.
    assert_eq!(
        diverges_at(&session, sets(&session.file), swapped),
        session.joined
    );
}

#[test]
fn the_file_without_its_set_diverges_where_the_set_applied() {
    let session = record();
    assert_eq!(
        diverges_at(&session, Vec::new(), inputs(&session.file)),
        session.set
    );
}
