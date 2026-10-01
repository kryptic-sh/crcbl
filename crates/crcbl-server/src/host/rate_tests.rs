//! The rate drop and the withheld update, end to end over in-memory links:
//! a host whose world changes more than a narrow link carries, and real
//! `crcbl_client::Client`s — or a bare far end, where a test must see every
//! snapshot — on the other side.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crcbl_client::Client;
use crcbl_ecs::{DebugCtx, Entity, SystemTrait};
use crcbl_net::auth::SessionCrypto;
use crcbl_net::{InMemoryTransport, Message, Trust};

use super::tests::{COMPATIBILITY, TICK, TICK_HZ, reply, say_hello};
use super::*;
use crate::cadence::{SNAPSHOT_INTERVAL_STEPS, STEP_DOWN_AFTER, STEP_UP_AFTER};
use crate::{KEYFRAME_RECOVERY_SNAPSHOTS, peer};

/// What a [`Narrow`] link reports its unreliable channel takes: a few of
/// [`Churn`]'s updates, far under one tick of its change.
const NARROW_LIMIT: usize = 400;

/// The slowest snapshot interval the rate drop reaches.
const SLOWEST: u32 = SNAPSHOT_INTERVAL_STEPS[SNAPSHOT_INTERVAL_STEPS.len() - 1];

/// An in-memory link whose unreliable channel reports [`NARROW_LIMIT`], the
/// way a UDP link reports one datagram: the host fits every snapshot to it.
struct Narrow(InMemoryTransport);

impl Transport for Narrow {
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.0.send_reliable(msg)
    }

    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.0.send_unreliable(msg)
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.0.recv_reliable()
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.0.recv()
    }

    fn is_connected(&self) -> bool {
        self.0.is_connected()
    }

    fn max_unreliable_message_bytes(&self) -> usize {
        NARROW_LIMIT
    }
}

/// A system of [`Churn::ENTITIES`] entities whose components all change
/// every tick while its switch is on, and hold while it is off.
struct Churn {
    ticks: u64,
    changing: Arc<AtomicBool>,
}

impl Churn {
    /// Enough to overflow a narrow link many times over, and few enough
    /// that a plain link's client takes a tick of change under its inbound
    /// byte budget.
    const ENTITIES: u64 = 40;
    /// Bytes of each component: the tick it was written at, then padding.
    const COMPONENT_BYTES: usize = 16;
}

impl SystemTrait for Churn {
    fn name(&self) -> &str {
        "churn"
    }

    fn tick(&mut self, _dt: f64) {
        if self.changing.load(Ordering::Relaxed) {
            self.ticks += 1;
        }
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

/// A host whose world churns, with clients on narrow or plain links.
struct Rig {
    host: Host,
    clients: Vec<Client<InMemoryTransport>>,
    ids: Vec<PeerId>,
    changing: Arc<AtomicBool>,
    now: Duration,
}

impl Rig {
    fn new() -> Self {
        let changing = Arc::new(AtomicBool::new(true));
        let mut world = World::new();
        world.register_system(Box::new(Churn {
            ticks: 0,
            changing: Arc::clone(&changing),
        }));
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
            clients: Vec::new(),
            ids: Vec::new(),
            changing,
            now: Duration::ZERO,
        }
    }

    /// Connects one more client, its host end narrow or not, and steps until
    /// it joins.
    fn join(&mut self, narrow: bool) {
        let (near, far) = InMemoryTransport::pair();
        if narrow {
            self.host.add(Box::new(Narrow(far)));
        } else {
            self.host.add(Box::new(far));
        }
        self.clients.push(Client::new_with_compatibility(
            World::new(),
            near,
            TICK_HZ,
            COMPATIBILITY,
        ));
        for _ in 0..60 {
            self.step();
            if let Some(PeerEvent::Joined(id)) = self.host.events().next() {
                self.ids.push(id);
                return;
            }
        }
        panic!("the client never joined");
    }

    /// One tick: the host, then every client.
    fn step(&mut self) {
        self.now += TICK;
        self.host.update(self.now);
        for client in &mut self.clients {
            client.update(self.now);
        }
    }

    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step();
        }
    }

    fn interval(&self, client: usize) -> u32 {
        self.host
            .peer_stats(self.ids[client])
            .expect("the peer is in")
            .snapshot_interval_ticks
    }

    /// The hash of the state the host would snapshot now.
    fn server_state(&self) -> u64 {
        peer::current_baseline(
            self.host.world(),
            SectorId::ZERO,
            self.host.tick_id(),
            &mut Counters::default(),
        )
        .expect("the world serialises")
        .state_hash()
    }
}

/// Ticks of sustained over-budget that take a session at full rate down to
/// [`SLOWEST`]: [`STEP_DOWN_AFTER`] snapshots at each interval but the last.
fn ticks_to_slowest() -> u32 {
    SNAPSHOT_INTERVAL_STEPS[..SNAPSHOT_INTERVAL_STEPS.len() - 1]
        .iter()
        .map(|interval| interval * STEP_DOWN_AFTER)
        .sum()
}

/// **One congested peer is slowed and the other is not, and the slowed one
/// comes back up once the world settles.** The narrow peer's snapshots shed
/// every tick, so it steps down to [`SLOWEST`] and is then sent a snapshot
/// every that many ticks; the plain peer beside it, whose link carries the
/// whole change, is sent one every tick throughout. When the churn stops,
/// sustained headroom steps the narrow peer back to full rate, and its
/// client converges on the server.
#[test]
fn a_congested_peer_steps_down_and_up_alone() {
    let mut rig = Rig::new();
    rig.join(true);
    rig.join(false);
    rig.run(ticks_to_slowest() + 2 * SLOWEST);
    assert_eq!(rig.interval(0), SLOWEST);
    assert_eq!(rig.interval(1), SNAPSHOT_INTERVAL_STEPS[0]);
    rig.run(10 * STEP_DOWN_AFTER * SLOWEST);
    assert_eq!(rig.interval(0), SLOWEST, "it stopped at the slowest step");

    let mut applied: [Vec<u64>; 2] = [Vec::new(), Vec::new()];
    for _ in 0..12 * SLOWEST {
        rig.step();
        for (client, ticks) in applied.iter_mut().enumerate() {
            let tick = rig.clients[client].last_applied_tick().get();
            if ticks.last() != Some(&tick) {
                ticks.push(tick);
            }
        }
    }
    let spacing = |ticks: &[u64]| -> Vec<u64> { ticks.windows(2).map(|w| w[1] - w[0]).collect() };
    assert!(
        spacing(&applied[0])
            .iter()
            .all(|&gap| gap == u64::from(SLOWEST)),
        "the narrow peer applied ticks {:?}",
        applied[0]
    );
    assert!(
        spacing(&applied[1]).iter().all(|&gap| gap == 1) && applied[1].len() >= 12,
        "the plain peer applied ticks {:?}",
        applied[1]
    );

    rig.changing.store(false, Ordering::Relaxed);
    let ticks_to_full: u32 = SNAPSHOT_INTERVAL_STEPS[1..]
        .iter()
        .map(|interval| interval * STEP_UP_AFTER)
        .sum();
    rig.run(ticks_to_full + 4 * SLOWEST);
    assert_eq!(rig.interval(0), SNAPSHOT_INTERVAL_STEPS[0]);
    assert_eq!(
        rig.clients[0].baseline_state_hash_in(SectorId::ZERO),
        Some(rig.server_state())
    );
    assert_eq!(rig.host.processing_error_count(), 0);
}

/// **A client at the slowest interval interpolates without starving.**
/// Between snapshots [`SLOWEST`] ticks apart its playback moves on every
/// frame and never reaches the newest snapshot it holds — which is what a
/// playback delay shorter than the snapshot spacing would do — so it never
/// runs dry. The client's playout delay measures the spacing and covers it.
#[test]
fn a_client_interpolates_smoothly_at_the_slowest_interval() {
    let mut rig = Rig::new();
    rig.join(true);
    rig.run(ticks_to_slowest() + 4 * SLOWEST);
    assert_eq!(rig.interval(0), SLOWEST);
    assert!(
        rig.clients[0].playout_stats().delay >= TICK * SLOWEST,
        "a delay of {:?} does not cover the spacing",
        rig.clients[0].playout_stats().delay
    );

    let underruns = rig.clients[0].playout_stats().underruns;
    let mut previous = rig.clients[0].playback_tick().expect("playing");
    for _ in 0..12 * SLOWEST {
        rig.step();
        let client = &rig.clients[0];
        let alpha = client.interpolation_alpha();
        let playback = client.playback_tick().expect("playing");
        assert!((0.0..=1.0).contains(&alpha), "alpha {alpha} extrapolates");
        assert!(
            playback > previous,
            "playback stood still at tick {playback}"
        );
        assert!(
            playback < client.last_applied_tick().get() as f64,
            "playback reached the newest snapshot, tick {playback}"
        );
        previous = playback;
    }
    assert_eq!(
        rig.clients[0].playout_stats().underruns,
        underruns,
        "playback ran dry"
    );
}

/// A bare far end of a narrow link: the test reads, opens and acknowledges
/// every snapshot itself, so it sees which are keyframes.
struct Watcher {
    transport: InMemoryTransport,
    crypto: SessionCrypto,
}

impl Watcher {
    /// Joins `host` over a [`Narrow`] link.
    fn join(host: &mut Host, now: &mut Duration) -> Self {
        let (mut near, far) = InMemoryTransport::pair();
        host.add(Box::new(Narrow(far)));
        say_hello(&mut near, 1, None);
        *now += TICK;
        host.update(*now);
        let HandshakeResult::Accept { resume_token, .. } = reply(&mut near) else {
            panic!("the host refused the hello");
        };
        Self {
            transport: near,
            crypto: SessionCrypto::from_token(&resume_token),
        }
    }

    /// Every snapshot waiting, decoded: `(tick, is_keyframe)`.
    fn snapshots(&mut self) -> Vec<(TickId, bool)> {
        let mut snapshots = Vec::new();
        while let Some(msg) = self.transport.recv().expect("the link is up") {
            let payload = self.crypto.open(&msg.payload).expect("a sealed snapshot");
            let delta =
                crcbl_net::decode_delta(payload, Trust::Authenticated).expect("a valid delta");
            snapshots.push((delta.tick, delta.is_keyframe));
        }
        snapshots
    }

    fn ack(&mut self, tick: TickId) {
        let sealed = self
            .crypto
            .seal(&crcbl_net::encode_ack(SectorId::ZERO, tick))
            .expect("the key seals");
        self.transport
            .send_unreliable(Message::unreliable(sealed))
            .expect("the link takes the ack");
    }
}

/// **Keyframe recovery counts snapshots, not ticks.** A peer slowed to
/// [`SLOWEST`] whose acks then stop is sent
/// [`KEYFRAME_RECOVERY_SNAPSHOTS`] more deltas before the keyframe that
/// resets it — not a keyframe after that many ticks, which at the slow rate
/// would give it a fraction of the snapshots a full-rate peer gets.
#[test]
fn keyframe_recovery_fires_after_its_snapshots_at_a_reduced_rate() {
    let mut rig = Rig::new();
    let mut watcher = Watcher::join(&mut rig.host, &mut rig.now);
    let peer = match rig.host.events().next() {
        Some(PeerEvent::Joined(id)) => id,
        other => panic!("expected a join, got {other:?}"),
    };
    let interval = |rig: &Rig| {
        rig.host
            .peer_stats(peer)
            .expect("in")
            .snapshot_interval_ticks
    };
    for _ in 0..ticks_to_slowest() + 4 * SLOWEST {
        rig.step();
        if let Some(&(tick, _)) = watcher.snapshots().last() {
            watcher.ack(tick);
        }
    }
    assert_eq!(interval(&rig), SLOWEST);

    // Silence from here. The first snapshot after the last ack lands still
    // sees the ack advance; every one after counts towards recovery.
    let mut deltas = 0;
    let mut ticks = 0;
    let keyframe = loop {
        rig.step();
        ticks += 1;
        assert!(
            ticks < 10 * KEYFRAME_RECOVERY_SNAPSHOTS * SLOWEST,
            "no keyframe came"
        );
        let snapshots = watcher.snapshots();
        if let Some(&(tick, _)) = snapshots.iter().find(|(_, keyframe)| *keyframe) {
            break tick;
        }
        deltas += snapshots.len() as u32;
    };
    assert_eq!(
        interval(&rig),
        SLOWEST,
        "the session stayed slow throughout"
    );
    assert!(
        (KEYFRAME_RECOVERY_SNAPSHOTS..=KEYFRAME_RECOVERY_SNAPSHOTS + 1).contains(&deltas),
        "the keyframe at tick {} came after {deltas} deltas in {ticks} ticks",
        keyframe.get()
    );
}

/// A system replicating one entity whose bytes are the tick count: what a
/// client can read the host's progress off.
struct Ticker {
    ticks: u64,
}

impl SystemTrait for Ticker {
    fn name(&self) -> &str {
        "ticker"
    }

    fn tick(&mut self, _dt: f64) {
        self.ticks += 1;
    }

    fn entity_count(&self) -> usize {
        1
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        crcbl_net::encode_entity_entry(out, 1, &self.ticks.to_le_bytes());
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A system replicating one entity longer than [`NARROW_LIMIT`].
struct Boulder;

impl SystemTrait for Boulder {
    fn name(&self) -> &str {
        "boulder"
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        1
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        crcbl_net::encode_entity_entry(out, 7, &[0xB0; NARROW_LIMIT]);
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **An entity too long for the link does not stop the others.** The
/// boulder's one update is longer than the narrow link carries; every
/// snapshot still reaches the client with the ticker's latest value in it,
/// the boulder is withheld from each — one count a snapshot, against the
/// peer and the host alike, recorded by its system's name — and nothing is
/// refused or slowed.
#[test]
fn an_oversized_entity_does_not_stop_the_others() {
    let mut world = World::new();
    world.register_system(Box::new(Ticker { ticks: 0 }));
    world.register_system(Box::new(Boulder));
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 1,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    let (near, far) = InMemoryTransport::pair();
    host.add(Box::new(Narrow(far)));
    let mut client = Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY);
    let mut now = Duration::ZERO;
    let mut peer = None;
    while client.last_applied_tick() == TickId::ZERO {
        now += TICK;
        host.update(now);
        client.update(now);
        peer = peer.or_else(|| host.events().next());
        assert!(now < Duration::from_secs(1), "no snapshot arrived");
    }
    let Some(PeerEvent::Joined(peer)) = peer else {
        panic!("expected a join, got {peer:?}");
    };

    let before = host.oversized_update_count();
    for _ in 0..30 {
        now += TICK;
        host.update(now);
        client.update(now);
        assert_eq!(
            client.last_applied_tick(),
            host.tick_id(),
            "a snapshot every tick"
        );
        let ticker: Vec<u64> = client
            .replicated("ticker")
            .map(|(_, data)| u64::from_le_bytes(data.try_into().expect("eight bytes")))
            .collect();
        assert_eq!(ticker, [host.tick_id().get()], "the ticker is current");
        assert_eq!(client.replicated("boulder").count(), 0);
    }
    assert_eq!(host.oversized_update_count() - before, 30, "one a snapshot");
    let stats = host.peer_stats(peer).expect("in");
    assert_eq!(stats.oversized_updates, host.oversized_update_count());
    assert_eq!(stats.snapshot_interval_ticks, SNAPSHOT_INTERVAL_STEPS[0]);
    let withheld = host.last_oversized_update().expect("recorded");
    assert_eq!(
        (withheld.system.as_str(), withheld.entity_bits),
        ("boulder", 7)
    );
    assert_eq!(withheld.limit, NARROW_LIMIT);
    assert_eq!(host.oversized_snapshot_count(), 0);
    assert_eq!(host.processing_error_count(), 0);
}
