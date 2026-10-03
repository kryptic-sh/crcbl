//! `sv_spin_rate` against the sandbox's host world, in one process: the world
//! and the wiring `LanHost` opens with, over in-memory transports — the host's
//! own player and a second client both real `Client`s — and the sandbox's own
//! routing, which the one test that binds a socket, to loopback, reaches.

use std::time::Duration;

use crcbl::client::Client;
use crcbl::core::TickId;
use crcbl::ecs::World;
use crcbl::net::{ConsoleOutcome, ConsoleReply, ConsoleSet, InMemoryTransport};
use crcbl::server::{Host, HostConfig};

use super::imp::{COMPATIBILITY, serve, world};
use crate::spin::{hosted_seconds, replicated_seconds, sv_spin_rate};

const TICK_HZ: u32 = 60;

/// One tick at [`TICK_HZ`], a hair long so every step runs exactly one.
const TICK: Duration = Duration::from_nanos(16_666_667);

/// The sandbox's host, its own player and one other client, in session.
struct Rig {
    host: Host,
    own: Client<InMemoryTransport>,
    other: Client<InMemoryTransport>,
    now: Duration,
}

impl Rig {
    fn new() -> Self {
        let mut host = Host::new(
            world(),
            HostConfig {
                max_peers: 4,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        serve(&mut host);
        host.update(Duration::ZERO);
        let (near, far) = InMemoryTransport::pair();
        host.add_host_player(Box::new(far));
        let own = Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY);
        let (near, far) = InMemoryTransport::pair();
        host.add(Box::new(far));
        let other = Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY);
        let mut rig = Self {
            host,
            own,
            other,
            now: Duration::ZERO,
        };
        for _ in 0..600 {
            rig.step();
            if replicated_seconds(&rig.own).is_some() && replicated_seconds(&rig.other).is_some() {
                return rig;
            }
        }
        panic!("neither client saw the host's cube");
    }

    fn step(&mut self) {
        self.now += TICK;
        assert_eq!(self.host.update(self.now), 1, "one tick a step");
        self.own.update(self.now);
        self.other.update(self.now);
    }

    fn seconds(&mut self) -> f32 {
        hosted_seconds(self.host.world_mut()).expect("the host's world has a cube")
    }

    fn rate(&self) -> f32 {
        self.host.sim_vars().f32(&sv_spin_rate)
    }
}

impl Rig {
    /// One tick's spin at `rate`, as `SpinModule` computes it.
    fn step_at(&self, rate: f32) -> f32 {
        self.host.world().tick_dt() as f32 * rate
    }
}

fn set(value: &str) -> ConsoleSet {
    ConsoleSet {
        name: "sv_spin_rate".to_owned(),
        value: value.to_owned(),
    }
}

#[test]
fn the_hosts_console_set_applies_on_the_next_tick_and_a_client_sees_the_cube_turn_at_it() {
    let mut rig = Rig::new();
    rig.host.submit_console_set(set("2"));
    assert_eq!(rig.rate(), 1.0, "nothing applies before a tick boundary");

    let before = rig.seconds();
    rig.step();
    let tick = rig.host.tick_id();
    assert_eq!(rig.rate(), 2.0);
    assert_eq!(
        rig.seconds(),
        before + rig.step_at(2.0),
        "the boundary's own tick spun at 2"
    );
    assert_eq!(
        rig.host.take_console_replies(),
        [ConsoleReply {
            name: "sv_spin_rate".to_owned(),
            value: "2".to_owned(),
            outcome: ConsoleOutcome::Applied(tick),
        }]
    );

    // The other client's replicated cube turns twice as far a tick now.
    let target = rig.seconds();
    for _ in 0..60 {
        rig.step();
        if replicated_seconds(&rig.other).is_some_and(|seen| seen >= target) {
            break;
        }
    }
    let first = replicated_seconds(&rig.other).expect("replicated");
    assert!(first >= target, "the client never caught up");
    rig.step();
    let second = replicated_seconds(&rig.other).expect("replicated");
    assert!(
        (second - first - rig.step_at(2.0)).abs() < 1e-5,
        "one tick at twice the rate: {first} -> {second}"
    );
}

#[test]
fn the_hosts_own_player_may_set_it_and_another_client_is_refused() {
    let mut rig = Rig::new();
    rig.other.send_console_set(&set("5")).expect("in session");
    rig.step();
    rig.step();
    assert_eq!(
        rig.rate(),
        1.0,
        "a client that is not the host moved nothing"
    );
    let refused: Vec<ConsoleReply> = rig.other.console_replies().collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        matches!(&refused[0].outcome, ConsoleOutcome::Refused(why) if why.contains("not the host")),
        "{refused:?}"
    );

    rig.own.send_console_set(&set("3")).expect("in session");
    rig.step();
    assert_eq!(rig.rate(), 3.0, "the host's own player is the host");
    rig.step();
    let applied: Vec<ConsoleReply> = rig.own.console_replies().collect();
    assert!(
        matches!(applied[..], [ConsoleReply { outcome: ConsoleOutcome::Applied(tick), .. }] if tick > TickId::ZERO),
        "{applied:?}"
    );
}

#[test]
fn a_value_outside_the_range_is_refused_by_name() {
    let mut rig = Rig::new();
    rig.host.submit_console_set(set("9"));
    rig.step();
    let replies = rig.host.take_console_replies();
    assert_eq!(
        replies[0].outcome,
        ConsoleOutcome::Refused("`sv_spin_rate`: 9 is outside 0..=8".to_owned())
    );
    assert_eq!(rig.rate(), 1.0);
}

/// A checked `sv_spin_rate` set, as the console hands one over.
fn spin_rate(value: &str) -> crcbl::console::SimSet {
    crate::spin::sim_registry()
        .sim_set("sv_spin_rate", value)
        .expect("in range")
}

#[test]
fn with_no_session_a_set_is_given_back_for_the_scene() {
    let mut lan = super::Lan::off();
    let set = spin_rate("2");
    match lan.route_sim_set(set.clone()) {
        Ok(super::SimRoute::Offline(back)) => assert_eq!(back, set),
        other => panic!("an offline sandbox handed the set elsewhere: {other:?}"),
    }
    assert_eq!(
        lan.cube_seconds(),
        None,
        "and has no session's cube to draw"
    );
}

/// **A hosting sandbox records its session when asked**: the file, finished
/// when the sandbox's session is dropped — its window closing — holds every
/// tick's hash and the `sv_spin_rate` set the host applied, as the console
/// prints it.
#[test]
fn a_hosting_sandbox_records_its_session_and_its_sets() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("sandbox.crpl");
    let loopback: std::net::SocketAddr = (std::net::Ipv4Addr::LOCALHOST, 0).into();
    let mut lan = super::Lan::host(
        crcbl::lan::LanBind {
            listen: loopback,
            announce_at: loopback,
            broadcast_to: None,
        },
        TICK_HZ,
        Some(&path),
    )
    .expect("loopback UDP must be available to these tests");
    assert!(matches!(
        lan.route_sim_set(spin_rate("4")),
        Ok(super::SimRoute::Sent)
    ));
    for _ in 0..5 {
        lan.frame(TICK);
    }
    drop(lan);

    let storage = crcbl::store::NativeStorage::at(dir.path().to_path_buf());
    let file =
        crcbl::store::replay::FileTransport::open(&storage, std::path::Path::new("sandbox.crpl"))
            .expect("the recording reads");
    assert_eq!(file.tick_rate(), TICK_HZ);
    assert_eq!(file.state_hashes().len(), 6, "the start and five ticks");
    let sets: Vec<(u64, &str, &str)> = file
        .sim_sets()
        .iter()
        .map(|recorded| {
            (
                recorded.tick.get(),
                recorded.set.name.as_str(),
                recorded.set.value.as_str(),
            )
        })
        .collect();
    assert_eq!(sets, [(1, "sv_spin_rate", "4")]);

    let existing = super::Lan::host(
        crcbl::lan::LanBind {
            listen: loopback,
            announce_at: loopback,
            broadcast_to: None,
        },
        TICK_HZ,
        Some(&path),
    );
    assert!(
        matches!(
            existing,
            Err(crcbl::lan::LanError::Record(
                crcbl::replay_record::RecordError::Exists(_)
            ))
        ),
        "{existing:?}"
    );
}

/// The one test here with a socket: a hosting sandbox's [`super::Lan`],
/// bound to loopback only, which is what routes a set and lends the cube.
#[test]
fn a_hosting_sandbox_hands_a_set_to_its_host_and_lends_the_hosts_cube() {
    let loopback: std::net::SocketAddr = (std::net::Ipv4Addr::LOCALHOST, 0).into();
    let mut lan = super::Lan::host(
        crcbl::lan::LanBind {
            listen: loopback,
            announce_at: loopback,
            broadcast_to: None,
        },
        TICK_HZ,
        None,
    )
    .expect("loopback UDP must be available to these tests");
    assert!(matches!(
        lan.route_sim_set(spin_rate("4")),
        Ok(super::SimRoute::Sent)
    ));
    lan.frame(TICK);
    lan.frame(TICK);
    let first = lan.cube_seconds().expect("the host's cube");
    lan.frame(TICK);
    let second = lan.cube_seconds().expect("the host's cube");
    let step = (1.0_f64 / f64::from(TICK_HZ)) as f32 * 4.0;
    assert!(
        (second - first - step).abs() < 1e-5,
        "the host's cube turned {first} -> {second} in a tick, not at 4"
    );
}
