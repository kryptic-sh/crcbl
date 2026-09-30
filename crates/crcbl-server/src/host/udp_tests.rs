//! The host over `crcbl_net::udp`: real `UdpTransport` peers on loopback,
//! handed to [`Host::add`] as a `UdpListener` accepts them — the wiring a
//! LAN host does — with real `crcbl_client::Client`s on the far ends.
//!
//! Native only, as the transport is. Every socket binds `127.0.0.1:0`, so no
//! test needs a free fixed port. Loopback delivery is asynchronous, so the
//! steps that wait for it pause briefly between polls and give up past
//! [`WAIT_LIMIT`], which is reached only when what they wait for never comes;
//! the transport's own timeouts run on a [`ManualClock`] where a test needs
//! one to pass.

use std::thread;
use std::time::Instant;

use crcbl_client::Client;
use crcbl_ecs::{DebugCtx, Entity, System, SystemTrait};
use crcbl_net::reliable::{MAX_UNRELIABLE_PAYLOAD, PEER_TIMEOUT};
use crcbl_net::udp::{ListenerConfig, UdpListener, UdpTransport};
use crcbl_net::{Clock, ManualClock, SystemClock};

use super::*;

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0000_484f_5354,
    schema_hash: 0x0000_5544_5050,
};

/// The endpoint protocol id both ends of every link speak.
const PROTOCOL: u32 = 0x4352_4342;

const TICK_HZ: u32 = 60;
const TICK: Duration = Duration::from_nanos(16_666_667);

/// The longest a test waits for loopback traffic.
const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// The pause between steps while waiting for loopback.
const POLL_PAUSE: Duration = Duration::from_millis(1);

/// How long a test gives loopback to deliver what is already in flight.
const QUIET_TIME: Duration = Duration::from_millis(50);

/// A world with one system holding one entity, so every snapshot carries
/// something.
fn world() -> World {
    let mut world = World::new();
    let entity = world.spawn();
    let mut system = System::<f32>::new("position");
    system.attach(entity, 0.0);
    world.register_system(Box::new(system));
    world
}

/// A system replicating one entity of `bytes` bytes: the knob that makes a
/// snapshot as long as a test needs.
struct Blob {
    bytes: usize,
}

impl SystemTrait for Blob {
    fn name(&self) -> &str {
        "blob"
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        1
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        crcbl_net::encode_entity_entry(out, 1, &vec![0xAB; self.bytes]);
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A host behind a UDP listener, and the clients connecting to it.
struct Rig<C: Clock + Clone> {
    host: Host,
    listener: UdpListener<C>,
    clients: Vec<Client<UdpTransport<C>>>,
    clock: C,
    now: Duration,
}

impl<C: Clock + Clone + 'static> Rig<C> {
    fn new(world: World, clock: C) -> Self {
        let listener =
            UdpListener::bind_with("127.0.0.1:0", ListenerConfig::new(PROTOCOL), clock.clone())
                .expect("loopback UDP must be available to these tests");
        let mut host = Host::new(
            world,
            HostConfig {
                max_peers: 4,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        host.update(Duration::ZERO);
        Self {
            host,
            listener,
            clients: Vec::new(),
            clock,
            now: Duration::ZERO,
        }
    }

    /// Starts one more client's connect.
    fn connect(&mut self) {
        let server = self.listener.local_addr().expect("listener address");
        let transport =
            UdpTransport::connect_with(server, PROTOCOL, self.clock.clone()).expect("connect");
        self.clients.push(Client::new_with_compatibility(
            World::new(),
            transport,
            TICK_HZ,
            COMPATIBILITY,
        ));
    }

    /// Accepts whoever the listener confirmed, then runs the host.
    fn step_host(&mut self) {
        while let Some(peer) = self.listener.accept() {
            self.host.add(Box::new(peer));
        }
        self.host.update(self.now);
    }

    /// One tick: the host, then every client.
    fn step(&mut self) {
        self.now += TICK;
        self.step_host();
        for client in &mut self.clients {
            client.update(self.now);
        }
    }

    /// `ticks` steps, pausing between them for loopback.
    fn run_ticks(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.step();
            thread::sleep(POLL_PAUSE);
        }
    }

    /// Steps until `done` holds, pausing between steps for loopback.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < WAIT_LIMIT, "gave up waiting for {what}");
            self.step();
            thread::sleep(POLL_PAUSE);
        }
    }

    /// Steps until the host admits a peer, and returns it.
    fn until_joined(&mut self) -> PeerId {
        let mut joined = None;
        self.until("a join", |rig| {
            joined = joined.or_else(|| {
                rig.host.events().find_map(|event| match event {
                    PeerEvent::Joined(id) => Some(id),
                    _ => None,
                })
            });
            joined.is_some()
        });
        joined.expect("joined")
    }
}

/// **A UDP peer that closes is lost, then gone, as any peer is.** Dropping
/// the client's transport sends the transport's disconnect; the host's end
/// reports it, the host raises `Lost` and holds the place for the grace
/// period, then raises `Left` — the same events a `SteamTransport` or an
/// in-memory link produce when it closes.
#[test]
fn a_udp_peer_that_closes_is_lost_then_leaves_after_the_grace_period() {
    const GRACE: Duration = Duration::from_millis(500);
    let mut rig = Rig::new(world(), SystemClock::new());
    rig.host.set_session_config(SessionConfig {
        reconnect_grace_period: GRACE,
        ..SessionConfig::default()
    });
    rig.connect();
    let id = rig.until_joined();
    rig.until("the first snapshot", |rig| {
        rig.clients[0].last_applied_tick() > TickId::ZERO
    });

    rig.clients.clear();
    let mut events = Vec::new();
    rig.until("the loss", |rig| {
        events.extend(rig.host.events());
        events.contains(&PeerEvent::Lost(id))
    });
    assert_eq!(rig.host.peer_state(id), Some(SessionState::Reconnecting));
    assert_eq!(rig.host.peer_count(), 1, "a lost peer keeps its place");

    let lost_at = rig.now;
    while rig.now.saturating_sub(lost_at) <= GRACE {
        rig.step();
    }
    events.extend(rig.host.events());
    assert!(events.contains(&PeerEvent::Left(id)), "{events:?}");
    assert_eq!(rig.host.peer_count(), 0);
    assert_eq!(rig.host.auth_failure_count(), 0);
}

/// **A UDP peer that falls silent times out, and the host reads that as a
/// lost link.** The client stops being driven, so nothing more reaches the
/// host from it; once [`PEER_TIMEOUT`] passes on the transport's clock the
/// host's end times out, and the host raises `Lost` and holds the place.
#[test]
fn a_udp_peer_that_falls_silent_times_out_and_is_lost() {
    let clock = ManualClock::new();
    let mut rig = Rig::new(world(), clock.clone());
    rig.connect();
    let id = rig.until_joined();
    rig.until("the first snapshot", |rig| {
        rig.clients[0].last_applied_tick() > TickId::ZERO
    });

    // From here only the host runs: the client neither sends nor reads. What
    // it sent last is read first — at the frozen clock, so it cannot count as
    // heard from after the jump below.
    thread::sleep(QUIET_TIME);
    rig.now += TICK;
    rig.step_host();
    assert!(
        rig.host.events().next().is_none(),
        "nothing ends before the timeout"
    );
    clock.advance(PEER_TIMEOUT);
    rig.now += TICK;
    rig.step_host();
    let events: Vec<_> = rig.host.events().collect();
    assert_eq!(events, [PeerEvent::Lost(id)]);
    assert_eq!(rig.host.peer_state(id), Some(SessionState::Reconnecting));
    assert_eq!(rig.host.peer_count(), 1, "a lost peer keeps its place");
}

/// **A snapshot that cannot fit one datagram is refused by name, not dropped
/// in silence.** `UdpTransport::send_unreliable` takes at most
/// [`MAX_UNRELIABLE_PAYLOAD`], and here one entity's update alone is that
/// long, so no fitting of the snapshot can hold it back into a datagram (a
/// snapshot that is merely long is fitted — see the test below). The host
/// records the refusal with the limit the transport reports, and the client
/// applies nothing. The same world with the entity under the limit plays, so
/// it is the size that is refused.
#[test]
fn a_snapshot_past_one_datagram_is_refused_by_name_and_one_under_it_plays() {
    for (bytes, fits) in [
        (MAX_UNRELIABLE_PAYLOAD / 4, true),
        (MAX_UNRELIABLE_PAYLOAD, false),
    ] {
        let mut world = world();
        world.register_system(Box::new(Blob { bytes }));
        let mut rig = Rig::new(world, SystemClock::new());
        rig.connect();
        rig.until_joined();
        if fits {
            rig.until("the first snapshot", |rig| {
                rig.clients[0].last_applied_tick() > TickId::ZERO
            });
            assert_eq!(rig.host.oversized_snapshot_count(), 0);
            assert_eq!(rig.host.last_oversized_snapshot(), None);
            let largest = rig.host.largest_snapshot_bytes();
            assert!(
                largest > bytes && largest <= MAX_UNRELIABLE_PAYLOAD,
                "the snapshot sent was {largest} bytes"
            );
            continue;
        }
        rig.until("a refusal", |rig| rig.host.oversized_snapshot_count() > 0);
        let refused = rig
            .host
            .last_oversized_snapshot()
            .expect("the refusal is recorded by name");
        assert_eq!(refused.limit, MAX_UNRELIABLE_PAYLOAD);
        assert!(refused.size > refused.limit, "{refused:?}");
        assert!(
            refused
                .to_string()
                .contains(&MAX_UNRELIABLE_PAYLOAD.to_string()),
            "the message names the limit: {refused}"
        );
        assert_eq!(rig.host.largest_snapshot_bytes(), 0, "nothing was sent");
        assert!(
            rig.host.processing_error_count() >= rig.host.oversized_snapshot_count(),
            "each refusal is a processing error too"
        );
        rig.run_ticks(10);
        assert_eq!(
            rig.clients[0].last_applied_tick(),
            TickId::ZERO,
            "no snapshot reached the client"
        );
    }
}

/// A system replicating [`Churn::ENTITIES`] entities whose components all
/// change every tick for the first [`Churn::TICKS`] ticks and then hold: far
/// more change per tick than one datagram carries, and then none.
struct Churn {
    ticks: u64,
}

impl Churn {
    /// Entities in the system: several datagrams' worth of updates a tick.
    const ENTITIES: u64 = 200;
    /// Ticks the components change for.
    const TICKS: u64 = 60;
    /// Bytes of each component: the tick it was written at, then padding.
    const COMPONENT_BYTES: usize = 24;
}

impl SystemTrait for Churn {
    fn name(&self) -> &str {
        "churn"
    }

    fn tick(&mut self, _dt: f64) {
        self.ticks = (self.ticks + 1).min(Self::TICKS);
    }

    fn entity_count(&self) -> usize {
        Self::ENTITIES as usize
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        let mut component = self.ticks.to_le_bytes().to_vec();
        component.resize(Self::COMPONENT_BYTES, 0x5A);
        for entity in 1..=Self::ENTITIES {
            crcbl_net::encode_entity_entry(out, entity, &component);
        }
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A session changing more than one datagram a tick plays, and converges.**
/// Every one of [`Churn::ENTITIES`] components changes every tick, several
/// datagrams' worth, so each snapshot is fitted to one datagram and the rest
/// held back by priority rather than the snapshot being refused. Once the
/// changes stop, the held-back updates rotate through and the client's state
/// matches the server's exactly.
#[test]
fn a_session_changing_more_than_a_datagram_holds_updates_back_and_converges() {
    let mut world = world();
    world.register_system(Box::new(Churn { ticks: 0 }));
    let full = peer::current_baseline(
        &world,
        SectorId::ZERO,
        TickId::from_raw(1),
        &mut Counters::default(),
    )
    .expect("the world serialises");
    let full_len = crcbl_net::encode_delta(&crcbl_net::DeltaCodec::encode_from_baseline(
        SectorId::ZERO,
        &full,
        None,
    ))
    .expect("a whole snapshot encodes in memory")
    .len();
    assert!(
        full_len > 4 * MAX_UNRELIABLE_PAYLOAD,
        "a {full_len}-byte snapshot must be several datagrams"
    );

    let mut rig = Rig::new(world, SystemClock::new());
    rig.connect();
    rig.until_joined();
    rig.until("the first snapshot", |rig| {
        rig.clients[0].last_applied_tick() > TickId::ZERO
    });
    let server_state = |rig: &Rig<SystemClock>| {
        peer::current_baseline(
            rig.host.world(),
            SectorId::ZERO,
            rig.host.tick_id(),
            &mut Counters::default(),
        )
        .expect("the world serialises")
        .state_hash()
    };
    rig.until("the client to converge once the changes stop", |rig| {
        rig.host.tick_id().get() > Churn::TICKS
            && rig.clients[0].baseline_state_hash_in(SectorId::ZERO) == Some(server_state(rig))
    });

    assert!(
        rig.host.held_back_update_count() > 0,
        "the snapshots were fitted by holding updates back"
    );
    assert_eq!(
        rig.host.oversized_snapshot_count(),
        0,
        "nothing was refused"
    );
    let largest = rig.host.largest_snapshot_bytes();
    assert!(
        largest <= MAX_UNRELIABLE_PAYLOAD,
        "the longest snapshot sent was {largest} bytes"
    );
    assert_eq!(
        rig.clients[0].baseline_entity_count(),
        full.entity_count(),
        "the client holds every entity"
    );
    assert_eq!(rig.host.processing_error_count(), 0);
}
