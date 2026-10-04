//! The host answering scene fetches, with real `crcbl_client::Client`s over
//! `InMemoryTransport`: the one fetch a peer may have in flight, the pace the
//! parts go at, and the refusals.

use std::collections::BTreeMap;

use crcbl_client::{Client, SceneFetch, SceneFetchFailed};
use crcbl_net::rate_limit::InboundRateLimitConfig;
use crcbl_net::{InMemoryTransport, MAX_SCENE_BYTES, MAX_SCENE_PART_BYTES};

use super::tests::{COMPATIBILITY, TICK, TICK_HZ};
use super::*;

/// A host serving edits, or not, and clients in session with it.
struct Rig {
    host: Host,
    clients: Vec<Client<InMemoryTransport>>,
    ids: Vec<PeerId>,
    now: Duration,
}

impl Rig {
    fn new(serving: bool, clients: usize) -> Self {
        let mut host = Host::new(
            World::new(),
            HostConfig {
                max_peers: 4,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        if serving {
            host.serve_edits();
        }
        let mut rig = Self {
            host,
            clients: Vec::new(),
            ids: Vec::new(),
            now: Duration::ZERO,
        };
        for _ in 0..clients {
            let (near, far) = InMemoryTransport::pair();
            rig.host.add(Box::new(far));
            rig.clients.push(Client::new_with_compatibility(
                World::new(),
                near,
                TICK_HZ,
                COMPATIBILITY,
            ));
        }
        for _ in 0..600 {
            rig.step();
            if rig
                .clients
                .iter()
                .all(|client| client.session_id().is_some())
            {
                rig.step();
                rig.ids = rig.host.peers().collect();
                return rig;
            }
        }
        panic!("the clients never joined");
    }

    fn step(&mut self) {
        self.now += TICK;
        self.host.update(self.now);
        for client in &mut self.clients {
            client.update(self.now);
        }
    }

    /// Steps until the host holds a fetch for the caller, and takes it.
    fn take_fetches(&mut self) -> Vec<(PeerId, u64)> {
        for _ in 0..60 {
            self.step();
            let fetches = self.host.take_scene_fetches();
            if !fetches.is_empty() {
                return fetches;
            }
        }
        panic!("no fetch reached the caller");
    }

    /// Steps until client `index` has a finished fetch, and returns them.
    fn finished(&mut self, index: usize) -> Vec<SceneFetch> {
        for _ in 0..600 {
            self.step();
            let fetched: Vec<_> = self.clients[index].scene_fetches().collect();
            if !fetched.is_empty() {
                return fetched;
            }
        }
        panic!("client {index}'s fetch never finished");
    }
}

/// A scene's files encoded, `parts` parts long with a short last part.
fn scene(parts: usize) -> (BTreeMap<String, String>, Vec<u8>) {
    let files = BTreeMap::from([(
        "sys/blocks.ron".to_owned(),
        "b".repeat(parts * MAX_SCENE_PART_BYTES - MAX_SCENE_PART_BYTES / 2),
    )]);
    let encoded = crcbl_net::encode_scene_files(&files).expect("small enough");
    (files, encoded)
}

fn refusal(fetch: &SceneFetch) -> Option<EditRefusal> {
    match &fetch.outcome {
        Err(SceneFetchFailed::Refused { reason, .. }) => Some(*reason),
        _ => None,
    }
}

/// **One fetch a peer is in flight at a time**: a second while the first
/// waits for the caller, and a third while its scene is being sent, are each
/// refused as busy by the host without reaching the caller — while another
/// peer's fetch goes through, and the first peer's next one does once its
/// scene has arrived.
#[test]
fn a_peer_has_one_fetch_in_flight_and_another_is_refused_as_busy() {
    let mut rig = Rig::new(true, 2);
    let first = rig.clients[0].fetch_scene().expect("in session");
    assert_eq!(rig.take_fetches(), [(rig.ids[0], first)]);

    let waiting = rig.clients[0].fetch_scene().expect("in session");
    let fetched = rig.finished(0);
    assert_eq!(fetched[0].fetch_id, waiting);
    assert_eq!(refusal(&fetched[0]), Some(EditRefusal::BUSY));
    assert!(
        rig.host.take_scene_fetches().is_empty(),
        "the caller never saw it"
    );

    let other = rig.clients[1].fetch_scene().expect("in session");
    assert_eq!(rig.take_fetches(), [(rig.ids[1], other)]);

    // The first peer's scene, being sent: a fetch now is busy too.
    let (files, encoded) = scene(3);
    rig.host
        .send_scene(rig.ids[0], first, 4, encoded.clone())
        .expect("connected");
    rig.step();
    let sending = rig.clients[0].fetch_scene().expect("in session");
    let fetched = rig.finished(0);
    assert_eq!(fetched[0].fetch_id, sending);
    assert_eq!(refusal(&fetched[0]), Some(EditRefusal::BUSY));

    // Once the stream is done, the next is the caller's again.
    for _ in 0..TICK_HZ * 2 {
        rig.step();
    }
    let next = rig.clients[0].fetch_scene().expect("in session");
    assert_eq!(rig.take_fetches(), [(rig.ids[0], next)]);
    rig.host
        .send_scene(rig.ids[0], next, 4, encoded)
        .expect("connected");
    let fetched = rig.finished(0);
    let scene = fetched[0].outcome.as_ref().expect("the scene");
    assert_eq!((scene.revision, &scene.files), (4, &files));
}

/// **A scene's parts are paced** at [`SCENE_FETCH_BYTES_PER_SECOND`], half a
/// client's default budget on the channel they share with the notices: one
/// part goes at once, and the scene takes its bytes' time to arrive.
#[test]
fn a_scenes_parts_go_at_the_fetch_pace() {
    assert!(
        2 * SCENE_FETCH_BYTES_PER_SECOND <= InboundRateLimitConfig::default().bytes_per_second,
        "the pace leaves half a client's reliable budget to everything else"
    );
    let mut rig = Rig::new(true, 1);
    let id = rig.clients[0].fetch_scene().expect("in session");
    rig.take_fetches();
    let (files, encoded) = scene(4);
    let sent_at = rig.now;
    rig.host
        .send_scene(rig.ids[0], id, 1, encoded.clone())
        .expect("connected");
    rig.step();
    assert_eq!(
        rig.clients[0].scene_fetch_progress(),
        Some(MAX_SCENE_PART_BYTES),
        "one part at once"
    );
    let fetched = rig.finished(0);
    assert_eq!(fetched[0].outcome.as_ref().expect("the scene").files, files);
    let before_last = u64::try_from(encoded.len() - MAX_SCENE_PART_BYTES / 2).expect("small");
    let took = rig.now - sent_at;
    assert!(
        took.as_secs_f64() >= before_last as f64 / SCENE_FETCH_BYTES_PER_SECOND as f64 - 0.05,
        "the scene arrived in {took:?}"
    );
}

/// A host serving no scene refuses a fetch as not editable, as it refuses an
/// edit, and nothing reaches the caller.
#[test]
fn a_host_serving_no_scene_refuses_a_fetch() {
    let mut rig = Rig::new(false, 1);
    rig.clients[0].fetch_scene().expect("in session");
    let fetched = rig.finished(0);
    assert_eq!(refusal(&fetched[0]), Some(EditRefusal::NOT_EDITABLE));
    assert!(rig.host.take_scene_fetches().is_empty());
}

/// A scene with no bytes, or past what a fetch carries, is refused by name
/// rather than sent; so is a fetch answered for a peer that is not there.
#[test]
fn a_scene_that_cannot_be_sent_is_refused_by_name() {
    let mut rig = Rig::new(true, 1);
    let id = rig.clients[0].fetch_scene().expect("in session");
    rig.take_fetches();
    for scene in [Vec::new(), vec![0; MAX_SCENE_BYTES + 1]] {
        assert!(matches!(
            rig.host.send_scene(rig.ids[0], id, 1, scene),
            Err(EventNotSent::TooLarge { limit, .. }) if limit == MAX_SCENE_BYTES
        ));
    }
    let absent = PeerId(99);
    assert!(matches!(
        rig.host.send_scene(absent, id, 1, vec![1]),
        Err(EventNotSent::NoSuchPeer(peer)) if peer == absent
    ));
    rig.host
        .refuse_scene_fetch(rig.ids[0], id, EditRefusal::FAILED, "no".to_owned())
        .expect("connected");
    let fetched = rig.finished(0);
    assert_eq!(refusal(&fetched[0]), Some(EditRefusal::FAILED));
}
