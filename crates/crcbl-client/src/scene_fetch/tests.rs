use std::collections::BTreeMap;
use std::time::Duration;

use crcbl_net::auth::SessionCrypto;
use crcbl_net::{InMemoryTransport, MAX_SCENE_PART_BYTES, MessageKind, ScenePart};

use super::*;
use crate::tests::{TICK, client, connect};

fn files(len: usize) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("scene.ron".to_owned(), "Scene(name: \"one\")".to_owned()),
        ("sys/blocks.ron".to_owned(), "b".repeat(len)),
    ])
}

/// Every part of `files` at `revision`, in order.
fn parts(revision: u64, files: &BTreeMap<String, String>) -> Vec<ScenePart> {
    let scene = crcbl_net::encode_scene_files(files).expect("small enough");
    (0..)
        .map_while(|index| crcbl_net::scene_part(revision, &scene, index))
        .collect()
}

/// Seal `reply` as the server would and put it on the reliable channel.
fn send(peer: &mut InMemoryTransport, crypto: &mut SessionCrypto, reply: &SceneReply) {
    let payload = crcbl_net::encode_scene_reply(reply).expect("encodes");
    send_payload(peer, crypto, &payload);
}

fn send_payload(peer: &mut InMemoryTransport, crypto: &mut SessionCrypto, payload: &[u8]) {
    let sealed = crypto.seal(payload).expect("counter space available");
    peer.send_reliable(Message::reliable(sealed)).unwrap();
}

fn part(fetch_id: u64, part: ScenePart) -> SceneReply {
    SceneReply {
        fetch_id,
        outcome: SceneOutcome::Part(part),
    }
}

#[test]
fn a_fetch_before_the_session_is_refused_and_spends_no_id() {
    let (client_transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(client_transport);
    assert!(matches!(
        client.fetch_scene(),
        Err(EditNotSent::NotInSession)
    ));
    assert!(peer.recv_reliable().unwrap().is_none(), "nothing went out");
    assert_eq!(client.scene_fetch_progress(), None);

    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    for expected in [1, 2] {
        assert_eq!(client.fetch_scene().expect("in session"), expected);
        let msg = peer.recv_reliable().unwrap().expect("the fetch went out");
        assert_eq!(msg.kind, MessageKind::Reliable);
        let opened = crypto
            .open(&msg.payload)
            .expect("sealed with the session key");
        let crcbl_net::ClientToServer::Command { data } =
            crcbl_net::decode_client_to_server(opened).expect("a command")
        else {
            panic!("a fetch travels as a command");
        };
        assert_eq!(
            crcbl_net::decode_scene_fetch(&data).expect("a fetch"),
            expected
        );
    }
    assert_eq!(client.scene_fetch_progress(), Some(0));
}

/// **The parts of a scene larger than one message are joined and handed
/// over once, whole**, with the progress of the fetch moving as they come.
#[test]
fn a_scene_in_many_parts_is_joined_and_taken_once() {
    let (client_transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(client_transport);
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    let id = client.fetch_scene().expect("in session");
    let files = files(3 * MAX_SCENE_PART_BYTES);
    let cut = parts(7, &files);
    assert!(cut.len() > 3);
    let last = cut.len() - 1;
    for (index, scene_part) in cut.into_iter().enumerate() {
        send(&mut peer, &mut crypto, &part(id, scene_part));
        client.update(TICK);
        if index < last {
            assert_eq!(client.scene_fetches().count(), 0, "whole at part {index}");
            assert_eq!(
                client.scene_fetch_progress(),
                Some((index + 1) * MAX_SCENE_PART_BYTES)
            );
        }
    }
    let fetched: Vec<SceneFetch> = client.scene_fetches().collect();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].fetch_id, id);
    assert_eq!(
        fetched[0].outcome.as_ref().expect("a scene"),
        &FetchedScene { revision: 7, files }
    );
    assert_eq!(client.scene_fetches().count(), 0, "taken once");
    assert_eq!(client.scene_fetch_progress(), None, "nothing in flight");
    assert_eq!(client.processing_error_count(), 0);
    assert_eq!(client.edit_notices().count(), 0, "a part is no notice");
    assert_eq!(client.events().count(), 0, "nor a game event");
}

/// **Only the fetch in flight is answered**: asking again drops the parts of
/// the one before as they come, and a refusal of the one in flight is
/// handed over with its code.
#[test]
fn the_parts_of_a_superseded_fetch_are_dropped_and_a_refusal_is_handed_over() {
    let (client_transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(client_transport);
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    let first = client.fetch_scene().expect("in session");
    let second = client.fetch_scene().expect("in session");
    for scene_part in parts(3, &files(8)) {
        send(&mut peer, &mut crypto, &part(first, scene_part));
    }
    send(
        &mut peer,
        &mut crypto,
        &SceneReply {
            fetch_id: second,
            outcome: SceneOutcome::Refused {
                reason: EditRefusal::BUSY,
                message: "still answering the last".to_owned(),
            },
        },
    );
    client.update(TICK);
    let fetched: Vec<SceneFetch> = client.scene_fetches().collect();
    assert_eq!(fetched.len(), 1, "the superseded fetch's scene was dropped");
    assert_eq!(fetched[0].fetch_id, second);
    assert!(matches!(
        &fetched[0].outcome,
        Err(SceneFetchFailed::Refused {
            reason: EditRefusal::BUSY,
            ..
        })
    ));
    assert_eq!(client.processing_error_count(), 0);
}

/// **Garbage is refused without a panic**: a part out of order ends the
/// fetch as malformed, and a reply that will not decode — cut short at every
/// length, or with a lying part length — is a processing error, leaving the
/// fetch in flight.
#[test]
fn a_part_out_of_order_or_a_reply_that_will_not_decode_is_refused() {
    let (client_transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(client_transport);
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

    let id = client.fetch_scene().expect("in session");
    let cut = parts(2, &files(2 * MAX_SCENE_PART_BYTES));
    send(&mut peer, &mut crypto, &part(id, cut[1].clone()));
    client.update(TICK);
    let fetched: Vec<SceneFetch> = client.scene_fetches().collect();
    assert!(matches!(
        &fetched[..],
        [SceneFetch {
            outcome: Err(SceneFetchFailed::Malformed(
                SceneAssemblyError::OutOfOrder {
                    expected: 0,
                    got: 1
                }
            )),
            ..
        }]
    ));
    assert_eq!(client.scene_fetch_progress(), None, "the fetch is spent");

    let id = client.fetch_scene().expect("in session");
    let small = parts(2, &files(8));
    let whole = crcbl_net::encode_scene_reply(&part(id, small[0].clone())).expect("encodes");
    let mut lying = whole.clone();
    // The part length, after the tag, the id, the outcome, the revision, the
    // whole length and the index.
    lying[1 + 8 + 1 + 8 + 4 + 4] ^= 1;
    let garbage = (0..whole.len())
        .map(|len| &whole[..len])
        .chain([&lying[..]]);
    // One a tenth of a second, well inside the client's inbound budget.
    let mut now = Duration::ZERO;
    for bytes in garbage {
        send_payload(&mut peer, &mut crypto, bytes);
        now += Duration::from_millis(100);
        client.update(now);
    }
    assert_eq!(
        client.processing_error_count(),
        u64::try_from(whole.len()).expect("small") + 1
    );
    assert_eq!(client.scene_fetches().count(), 0);
    assert_eq!(client.scene_fetch_progress(), Some(0), "still in flight");
}

/// A reconnect stops waiting for the fetch in flight: its parts went with the
/// link.
#[test]
fn a_reconnect_drops_the_fetch_in_flight() {
    let (client_transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(client_transport);
    let _crypto = connect(&mut client, &mut peer, Duration::ZERO);
    client.fetch_scene().expect("in session");
    assert_eq!(client.scene_fetch_progress(), Some(0));
    let (replacement, _peer) = InMemoryTransport::pair();
    client.reconnect(replacement);
    assert_eq!(client.scene_fetch_progress(), None);
}
