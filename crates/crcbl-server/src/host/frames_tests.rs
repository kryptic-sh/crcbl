//! What a host's module is handed of its peers, recorded and replayed: real
//! `crcbl_client::Client`s over `InMemoryTransport` on the live side, and a
//! fresh host fed the record on the other, with a module that keeps every
//! tick's `PeerInputs` and folds them into the world's state hash.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::{Arc, Mutex};

use crcbl_client::Client;
use crcbl_ecs::{DebugCtx, Entity, SystemTrait};
use crcbl_net::InMemoryTransport;

use super::tests::{COMPATIBILITY, TICK, TICK_HZ};
use super::*;
use crate::sim_hash::hash_world;

/// Every tick's `PeerInputs`, as the module read them, one entry a module
/// call.
type Handed = Arc<Mutex<Vec<Vec<PeerFrames>>>>;

/// A digest of everything the module was handed, so the state hash moves
/// with the input and a re-simulation handed anything else diverges.
struct Seen {
    digest: u64,
}

impl SystemTrait for Seen {
    fn name(&self) -> &str {
        "seen"
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        0
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn hash_state(&self, hasher: &mut dyn Hasher) {
        hasher.write_u64(self.digest);
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Keeps what it is handed and folds it into [`Seen`], in the order handed:
/// a peer's place in the roster matters as much as its frames.
struct Reader {
    handed: Handed,
}

impl HostModule for Reader {
    fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>) {
        let handed: Vec<PeerFrames> = inputs
            .iter()
            .map(|(peer, frames)| PeerFrames {
                peer,
                frames: frames.iter().map(|(t, d)| (t, d.to_vec())).collect(),
                dropped: frames.dropped(),
            })
            .collect();
        let seen = world.system_mut::<Seen>().expect("the world registers it");
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(seen.digest);
        for peer in &handed {
            hasher.write_u64(peer.peer.get());
            hasher.write_u32(peer.dropped);
            hasher.write_usize(peer.frames.len());
            for (tick, data) in &peer.frames {
                hasher.write_u64(tick.get());
                hasher.write_usize(data.len());
                hasher.write(data);
            }
        }
        seen.digest = hasher.finish();
        self.handed.lock().expect("not poisoned").push(handed);
    }
}

/// A host whose module is a [`Reader`], at tick zero, and what it is handed.
fn host() -> (Host, Handed) {
    let mut world = World::new();
    world.register_system(Box::new(Seen { digest: 0 }));
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 4,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    let handed = Handed::default();
    host.set_module(Box::new(Reader {
        handed: Arc::clone(&handed),
    }));
    host.update(Duration::ZERO);
    (host, handed)
}

/// A live host recording its peers' input, its clients, and the state hash
/// at the end of every tick.
struct Live {
    host: Host,
    handed: Handed,
    clients: Vec<Client<InMemoryTransport>>,
    ids: Vec<PeerId>,
    hashes: Vec<(TickId, u64)>,
    now: Duration,
}

impl Live {
    fn new() -> Self {
        let (mut host, handed) = host();
        host.record_peer_inputs();
        Self {
            host,
            handed,
            clients: Vec::new(),
            ids: Vec::new(),
            hashes: Vec::new(),
            now: Duration::ZERO,
        }
    }

    /// One tick: every client says which client it is and which tick its
    /// clock is on, so every frame is distinct; then the host, then the
    /// clients.
    fn step(&mut self) -> Vec<PeerEvent> {
        self.now += TICK;
        assert_eq!(self.host.update(self.now), 1, "one tick a step");
        let tick = self.host.tick_id();
        self.hashes
            .push((tick, hash_world(self.host.world(), tick)));
        for (index, client) in self.clients.iter_mut().enumerate() {
            client.set_input(vec![index as u8, tick.get() as u8]);
            client.update(self.now);
        }
        self.host.events().collect()
    }

    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.step();
        }
    }

    fn until(&mut self, event: impl Fn(PeerEvent) -> bool) -> PeerEvent {
        for _ in 0..600 {
            if let Some(found) = self.step().into_iter().find(|seen| event(*seen)) {
                return found;
            }
        }
        panic!("no such event within 600 ticks");
    }

    fn join(&mut self) {
        let (near, far) = InMemoryTransport::pair();
        self.host.add(Box::new(far));
        self.clients.push(Client::new_with_compatibility(
            World::new(),
            near,
            TICK_HZ,
            COMPATIBILITY,
        ));
        match self.until(|event| matches!(event, PeerEvent::Joined(_))) {
            PeerEvent::Joined(id) => self.ids.push(id),
            other => panic!("not a join: {other:?}"),
        }
    }
}

/// Three peers join, one is lost and resumes, one is kicked, and every one
/// left sends a frame every tick.
fn session() -> Live {
    let mut live = Live::new();
    for _ in 0..3 {
        live.join();
    }
    live.run(5);
    let (near, far) = InMemoryTransport::pair();
    live.clients[1].reconnect(near);
    let lost = live.ids[1];
    live.until(|event| event == PeerEvent::Lost(lost));
    live.run(3);
    live.host.add(Box::new(far));
    live.until(|event| event == PeerEvent::Resumed(lost));
    live.run(5);
    assert!(live.host.kick(live.ids[2]));
    live.clients.truncate(2);
    live.run(10);
    live
}

/// The record laid out as the module was handed it: every recorded tick
/// from the first, each with every admitted peer in admission order.
fn as_handed(record: &[TickInputs], last: TickId) -> Vec<Vec<PeerFrames>> {
    let mut roster: Vec<PeerId> = Vec::new();
    let mut ticks = Vec::new();
    let mut entries = record.iter().peekable();
    let first = record.first().expect("something was recorded").tick.get();
    for tick in first..=last.get() {
        let entry = entries.next_if(|entry| entry.tick.get() == tick);
        for change in entry.iter().flat_map(|entry| &entry.roster) {
            match *change {
                RosterChange::Joined(peer) => roster.push(peer),
                RosterChange::Left(peer) | RosterChange::Ended(peer) => {
                    roster.retain(|id| *id != peer);
                }
                RosterChange::Lost(_) | RosterChange::Resumed(_) => {}
            }
        }
        ticks.push(
            roster
                .iter()
                .map(|id| {
                    entry
                        .and_then(|entry| entry.peers.iter().find(|frames| frames.peer == *id))
                        .cloned()
                        .unwrap_or_else(|| PeerFrames::none(*id))
                })
                .collect(),
        );
    }
    ticks
}

#[test]
fn the_record_is_what_the_module_read_with_the_roster_in_the_order_applied() {
    let live = session();
    let record = live.host.peer_input_record();
    let handed = live.handed.lock().expect("not poisoned").clone();

    // The record starts at the first join; every module call from there is
    // in it, peer by peer and frame by frame.
    let first = record[0].tick;
    let from_first = &handed[(first.get() - 1) as usize..];
    assert_eq!(as_handed(record, live.host.tick_id()), from_first);
    assert!(
        from_first
            .iter()
            .flatten()
            .any(|peer| peer.frames.len() == 1),
        "frames were handed, so the comparison compared some"
    );

    let changes: Vec<RosterChange> = record
        .iter()
        .flat_map(|entry| entry.roster.iter().copied())
        .collect();
    let [a, b, c] = live.ids[..] else {
        panic!("three peers");
    };
    assert_eq!(
        changes,
        [
            RosterChange::Joined(a),
            RosterChange::Joined(b),
            RosterChange::Joined(c),
            RosterChange::Lost(b),
            RosterChange::Resumed(b),
            RosterChange::Ended(c),
        ]
    );
    // A kick between two ticks is the next tick's: its module is the first
    // that no longer lists the peer.
    let kicked = record
        .iter()
        .find(|entry| entry.roster.contains(&RosterChange::Ended(c)))
        .expect("recorded")
        .tick;
    let before = &handed[(kicked.get() - 2) as usize];
    let at = &handed[(kicked.get() - 1) as usize];
    assert!(before.iter().any(|peer| peer.peer == c));
    assert!(at.iter().all(|peer| peer.peer != c));
}

#[test]
fn a_resimulated_host_hands_its_module_what_the_live_one_read() {
    let live = session();
    let record = live.host.peer_input_record().to_vec();

    let (mut replayed, handed) = host();
    replayed.record_peer_inputs();
    assert_eq!(
        replayed.resimulate([], live.hashes.clone(), record.clone()),
        Ok(live.host.tick_id())
    );
    assert_eq!(
        *handed.lock().expect("not poisoned"),
        *live.handed.lock().expect("not poisoned"),
        "every tick, peer by peer"
    );
    assert_eq!(
        replayed.peer_input_record(),
        record,
        "the replay recorded the same"
    );
}

#[test]
fn a_record_without_its_frames_diverges_at_the_first_tick_one_was_handed() {
    let live = session();
    let mut record = live.host.peer_input_record().to_vec();
    let first_frame = record
        .iter()
        .find(|entry| !entry.peers.is_empty())
        .expect("frames were recorded")
        .tick;
    for entry in &mut record {
        entry.peers.clear();
    }
    let (mut replayed, _) = host();
    let error = replayed
        .resimulate([], live.hashes.clone(), record)
        .expect_err("the frames are what the module read");
    assert!(
        matches!(error, ResimError::Diverged { tick, .. } if tick == first_frame),
        "{error}"
    );
}

#[test]
fn recording_begun_mid_session_opens_with_the_peers_already_in_it() {
    let (host, _) = host();
    let mut live = Live {
        host,
        handed: Handed::default(),
        clients: Vec::new(),
        ids: Vec::new(),
        hashes: Vec::new(),
        now: Duration::ZERO,
    };
    live.join();
    live.join();
    let (spare, _unheard) = InMemoryTransport::pair();
    live.clients[1].reconnect(spare);
    let lost = live.ids[1];
    live.until(|event| event == PeerEvent::Lost(lost));
    assert!(live.host.peer_input_record().is_empty(), "not recording");

    live.host.record_peer_inputs();
    live.step();
    let record = live.host.peer_input_record();
    assert_eq!(record[0].tick, live.host.tick_id());
    assert_eq!(
        record[0].roster,
        [
            RosterChange::Joined(live.ids[0]),
            RosterChange::Joined(lost),
            RosterChange::Lost(lost),
        ]
    );
}

#[test]
fn a_roster_that_cannot_happen_is_refused_before_any_tick_runs() {
    let at = TickId::from_raw;
    let (a, b) = (PeerId::from_raw(1), PeerId::from_raw(2));
    let tick = |tick, roster: Vec<RosterChange>, peers: Vec<PeerFrames>| TickInputs {
        tick: at(tick),
        roster,
        peers,
    };
    let sent = |peer| PeerFrames {
        peer,
        frames: vec![(at(1), vec![1])],
        dropped: 0,
    };
    let roster = |tick, change, fault| ResimError::RosterRefused {
        tick: at(tick),
        change,
        fault,
    };
    let frames = |tick, peer, fault| ResimError::FramesRefused {
        tick: at(tick),
        peer,
        fault,
    };
    for (record, expected) in [
        (
            vec![tick(1, vec![RosterChange::Left(a)], vec![])],
            roster(1, RosterChange::Left(a), RosterFault::NotAdmitted),
        ),
        (
            vec![tick(
                1,
                vec![RosterChange::Joined(a), RosterChange::Joined(a)],
                vec![],
            )],
            roster(1, RosterChange::Joined(a), RosterFault::AlreadyAdmitted),
        ),
        (
            vec![
                tick(
                    1,
                    vec![RosterChange::Joined(a), RosterChange::Left(a)],
                    vec![],
                ),
                tick(2, vec![RosterChange::Joined(a)], vec![]),
            ],
            roster(2, RosterChange::Joined(a), RosterFault::Reused),
        ),
        (
            vec![tick(
                1,
                vec![
                    RosterChange::Joined(a),
                    RosterChange::Lost(a),
                    RosterChange::Lost(a),
                ],
                vec![],
            )],
            roster(1, RosterChange::Lost(a), RosterFault::AlreadyLost),
        ),
        (
            vec![tick(
                1,
                vec![RosterChange::Joined(a), RosterChange::Resumed(a)],
                vec![],
            )],
            roster(1, RosterChange::Resumed(a), RosterFault::NotLost),
        ),
        // Swapped, a join and a kick of the same peer cannot happen.
        (
            vec![tick(
                1,
                vec![RosterChange::Ended(a), RosterChange::Joined(a)],
                vec![],
            )],
            roster(1, RosterChange::Ended(a), RosterFault::NotAdmitted),
        ),
        (
            vec![tick(1, vec![RosterChange::Joined(a)], vec![sent(b)])],
            frames(1, b, FramesFault::NotAdmitted),
        ),
        (
            vec![tick(
                1,
                vec![RosterChange::Joined(a), RosterChange::Lost(a)],
                vec![sent(a)],
            )],
            frames(1, a, FramesFault::Lost),
        ),
        (
            vec![tick(
                1,
                vec![RosterChange::Joined(a)],
                vec![sent(a), sent(a)],
            )],
            frames(1, a, FramesFault::Twice),
        ),
        (
            vec![
                tick(3, vec![RosterChange::Joined(a)], vec![]),
                tick(3, vec![RosterChange::Joined(b)], vec![]),
            ],
            ResimError::TickPassed {
                tick: at(3),
                host_tick: at(3),
            },
        ),
    ] {
        let (mut replayed, handed) = host();
        assert_eq!(
            replayed.resimulate([], [(at(5), 0)], record.clone()),
            Err(expected),
            "{record:?}"
        );
        assert_eq!(replayed.tick_id(), TickId::ZERO, "no tick ran");
        assert!(handed.lock().expect("not poisoned").is_empty());
    }
}

#[test]
fn a_recorded_dropped_count_reaches_the_module() {
    let at = TickId::from_raw;
    let peer = PeerId::from_raw(9);
    let record = vec![TickInputs {
        tick: at(2),
        roster: vec![RosterChange::Joined(peer)],
        peers: vec![PeerFrames {
            peer,
            frames: vec![(at(1), vec![4, 2])],
            dropped: 3,
        }],
    }];
    let (mut replayed, handed) = host();
    replayed.record_peer_inputs();
    let hash = {
        let (mut probe, _) = host();
        probe
            .resimulate([], [(at(3), 0)], record.clone())
            .expect_err("a zero hash is not reproduced");
        hash_world(probe.world(), at(3))
    };
    assert_eq!(
        replayed.resimulate([], [(at(3), hash)], record.clone()),
        Ok(at(3))
    );
    let handed = handed.lock().expect("not poisoned");
    assert_eq!(handed[0], [], "no peer before its join");
    assert_eq!(handed[1], record[0].peers);
    assert_eq!(
        handed[2],
        [PeerFrames::none(peer)],
        "listed after, with nothing"
    );
    assert_eq!(replayed.peer_input_record(), record);
}
