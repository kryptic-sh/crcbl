//! A follower over a real [`EditServer`] and real clients on
//! `InMemoryTransport`: a late join, a scene larger than one message, notices
//! arriving during the fetch, an undo reaching past it, and a notice lost on
//! the way — each compared with the server's scene as saved text. A drag
//! sent as one gesture, from one client or two, is in [`gestures`].

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::client::Client;
use crate::ecs::World;
use crate::net::{InMemoryTransport, ProtocolCompatibility};
use crate::reflect::Value;
use crate::scene::edit::{EditCommand, encode_op};
use crate::scene::scn::{EntityName, SceneEntityId};
use crate::server::HostConfig;

use super::super::tests::{BLOCKS, one_block, vocabulary};
use super::*;
use crate::scene_edit::{EditServer, empty_source};

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0046_4f4c_4c57,
    schema_hash: 0x0053_4345_4e45,
};

const TICK_HZ: u32 = 60;

/// Long enough for any fetch here, at the server's pace, many times over.
const PATIENCE: u32 = 60 * TICK_HZ;

/// A server, a client that edits, and — once [`join`](Self::join)ed — a
/// client that follows, stepped together a tick at a time; and any more
/// clients that edit, once [`add_editor`](Self::add_editor)ed.
struct Rig {
    server: EditServer,
    author: Client<InMemoryTransport>,
    reader: Option<(Client<InMemoryTransport>, SceneFollower)>,
    others: Vec<Client<InMemoryTransport>>,
    now: Duration,
}

impl Rig {
    fn new(document: Document) -> Self {
        let mut server = EditServer::new(
            document,
            HostConfig {
                max_peers: 4,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        let author = client_of(&mut server);
        let mut rig = Self {
            server,
            author,
            reader: None,
            others: Vec::new(),
            now: Duration::ZERO,
        };
        rig.until(|rig| rig.author.session_id().is_some());
        rig
    }

    /// Adds the following client, now — after whatever edits came before.
    fn join(&mut self) {
        let client = client_of(&mut self.server);
        self.reader = Some((client, SceneFollower::new(vocabulary())));
    }

    fn step(&mut self) {
        self.now += Duration::from_secs(1) / TICK_HZ;
        self.server.update(self.now);
        self.author.update(self.now);
        for other in &mut self.others {
            other.update(self.now);
        }
        if let Some((client, follower)) = self.reader.as_mut() {
            client.update(self.now);
            follower.update(client, self.now);
        }
    }

    /// Steps until `done` holds, or panics.
    fn until(&mut self, done: impl Fn(&Self) -> bool) {
        for _ in 0..PATIENCE {
            self.step();
            if done(self) {
                return;
            }
        }
        panic!("never happened");
    }

    fn follower(&self) -> &SceneFollower {
        &self.reader.as_ref().expect("joined").1
    }

    fn follower_mut(&mut self) -> &mut SceneFollower {
        &mut self.reader.as_mut().expect("joined").1
    }

    /// Steps until the follower stands at the server's revision, not stale.
    fn caught_up(&mut self) {
        self.until(|rig| {
            rig.follower().revision() == Some(rig.server.revision()) && !rig.follower().is_stale()
        });
    }

    /// Sends `op` from the author without waiting for its reply.
    fn send_only(&mut self, op: &EditOp) {
        self.author
            .send_edit(encode_op(op).expect("every op here travels"))
            .expect("in session");
    }

    /// Sends `op` from the author and steps until it applies.
    fn send(&mut self, op: &EditOp) {
        let revision = self.server.revision();
        self.send_only(op);
        self.until(|rig| rig.server.revision() > revision);
        let _ = self.author.edit_replies().count();
    }

    /// The server's scene and the follower's copy, as saved text.
    fn both_files(&mut self) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let served = self.server.document_mut().files().expect("saves");
        let copy = self
            .follower_mut()
            .document_mut()
            .expect("a copy")
            .files()
            .expect("saves");
        (served, copy)
    }
}

/// A client of `server`, not yet in session.
fn client_of(server: &mut EditServer) -> Client<InMemoryTransport> {
    let (near, far) = InMemoryTransport::pair();
    server.host_mut().add(Box::new(far));
    Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY)
}

/// Moves block 0 along x to `x`.
fn shift(x: f64) -> EditOp {
    EditOp::Apply(EditCommand::SetProperty {
        entity: SceneEntityId(0),
        system: BLOCKS.to_owned(),
        path: "position.0".to_owned(),
        value: Value::Float(x),
    })
}

fn rename(id: u32, name: &str) -> EditOp {
    EditOp::Apply(EditCommand::Rename {
        entity: SceneEntityId(id),
        name: Some(EntityName::new(name).expect("a name")),
    })
}

/// A scene of `count` blocks in a row, each named, so its text is long.
fn many_blocks(count: u64) -> Document {
    let mut chunk = String::from("Chunk(\n    system: \"blocks\",\n    entities: [\n");
    for id in 0..count {
        let x = f64::from(u32::try_from(id).expect("a few thousand")) * 3.0;
        writeln!(
            chunk,
            "        ({id}, Block(position: ({x}, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),"
        )
        .expect("a string takes every write");
    }
    chunk.push_str("    ],\n)");
    let mut source = empty_source();
    for (key, text) in [
        (
            "scene.ron",
            "Scene(\n    format: 0,\n    name: \"many\",\n    systems: [\n        \"blocks\",\n    ],\n)"
                .to_owned(),
        ),
        ("sys/blocks.ron", chunk),
    ] {
        source
            .insert(Path::new(key), text.into_bytes())
            .expect("a scene key is a legal asset key");
    }
    Document::open(&source, Path::new(""), vocabulary()).expect("a row of blocks is a scene")
}

/// **A client joining after edits fetches the scene and then follows it**:
/// its copy's saved text is the server's byte for byte at the fetch, and
/// again after edits made since — undo and redo among them — with no second
/// fetch.
#[test]
fn a_late_joiner_fetches_the_scene_and_follows_it_to_the_same_bytes() {
    let mut rig = Rig::new(one_block());
    for x in [1.0, 2.0, 3.0] {
        rig.send(&shift(x));
    }
    rig.send(&rename(0, "First"));
    assert_eq!(rig.server.revision(), 4);

    rig.join();
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().revision(), Some(4));

    for op in [shift(9.0), EditOp::Undo, EditOp::Redo, rename(0, "Again")] {
        rig.send(&op);
    }
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().revision(), Some(8));
    assert_eq!(
        rig.follower().fetch_count(),
        1,
        "it followed, not refetched"
    );
}

/// **A scene larger than one message arrives whole**, paced out over several
/// parts, and is the server's scene byte for byte.
#[test]
fn a_scene_larger_than_one_message_arrives_whole() {
    let mut rig = Rig::new(many_blocks(1500));
    let served = rig.server.document_mut().files().expect("saves");
    let size: usize = served.values().map(String::len).sum();
    assert!(
        size > 2 * crate::net::MAX_IN_MEMORY_MESSAGE_BYTES,
        "the scene is {size} bytes"
    );

    rig.join();
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().fetch_count(), 1);
}

/// **Notices that arrive during a fetch are applied after it, in order**:
/// edits made while a large scene is still on its way are held, and the
/// copy the fetch opens applies them — reaching the server's scene with no
/// second fetch.
#[test]
fn notices_during_the_fetch_are_held_and_applied_after_it() {
    let mut rig = Rig::new(many_blocks(1500));
    rig.join();
    // Until the fetch is under way, its first part come.
    rig.until(|rig| {
        let (client, _) = rig.reader.as_ref().expect("joined");
        client.scene_fetch_progress().is_some_and(|bytes| bytes > 0)
    });
    for (index, name) in ["One", "Two", "Three"].into_iter().enumerate() {
        rig.send(&rename(u32::try_from(index).expect("small"), name));
    }
    rig.step();
    assert!(
        rig.follower().document().is_none(),
        "the fetch is still on its way"
    );
    assert_eq!(rig.follower().held.len(), 3, "the notices wait for it");

    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().revision(), Some(3));
    assert_eq!(
        rig.follower().fetch_count(),
        1,
        "the held notices were applied"
    );
}

/// **An undo reaching back past the fetch refetches**: the copy holds no
/// history from before it, so it cannot take that step itself, and it fetches
/// the scene the undo left rather than drift.
#[test]
fn an_undo_reaching_past_the_fetch_refetches_and_converges() {
    let mut rig = Rig::new(one_block());
    rig.send(&shift(5.0));
    rig.join();
    rig.caught_up();
    assert_eq!(rig.follower().fetch_count(), 1);

    rig.send(&EditOp::Undo);
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().revision(), Some(2));
    assert_eq!(rig.follower().fetch_count(), 2, "the undo refetched");
}

/// **A notice the client dropped refetches**: edits past what the client
/// holds between two reads are dropped by it, and the copy fetches the scene
/// again — even with no later notice to show the gap — and converges.
#[test]
fn a_dropped_notice_refetches_and_converges() {
    let mut rig = Rig::new(one_block());
    rig.join();
    rig.caught_up();

    // More edits than the client holds, all answered in one server update
    // and read in one client update.
    let edits = u32::try_from(crate::client::MAX_QUEUED_EVENTS).expect("small") + 8;
    for n in 0..edits {
        rig.send_only(&shift(f64::from(n)));
    }
    rig.caught_up();
    let (client, _) = rig.reader.as_ref().expect("joined");
    assert!(
        client.dropped_event_count() > 0,
        "the client dropped notices"
    );
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
    assert_eq!(rig.follower().revision(), Some(u64::from(edits)));
    assert_eq!(rig.follower().fetch_count(), 2, "the drop refetched");
}

/// The follower's copy of `one_block` at `revision`, as a fetch would bring
/// it.
fn following_at(revision: u64) -> SceneFollower {
    let mut follower = SceneFollower::new(vocabulary());
    let files = one_block().files().expect("saves");
    follower.adopt(FetchedScene { revision, files }, Duration::ZERO);
    assert_eq!(follower.revision(), Some(revision));
    follower
}

fn notice(revision: u64, op: &EditOp) -> EditNotice {
    EditNotice {
        revision,
        author: 1,
        gesture: None,
        op: encode_op(op).expect("travels"),
    }
}

/// **A gap in the revisions makes the copy stale rather than diverge**: the
/// notice after a missing one is held, not applied, and the copy waits for a
/// fetch; one at or before the copy's revision is passed over.
#[test]
fn a_revision_gap_makes_the_copy_stale_and_holds_the_notice() {
    let mut follower = following_at(3);
    follower.take(notice(2, &shift(1.0)));
    follower.take(notice(4, &shift(2.0)));
    assert_eq!(follower.revision(), Some(4));
    assert!(!follower.is_stale());

    follower.take(notice(6, &shift(3.0)));
    assert!(follower.is_stale());
    assert_eq!(
        follower.revision(),
        Some(4),
        "the notice past the gap waits"
    );
    assert_eq!(follower.held.len(), 1);
    assert!(
        follower
            .last_failure()
            .is_some_and(|why| why.contains("revision 5 to 5")),
        "{:?}",
        follower.last_failure()
    );
}

/// **Notices held past the cap make the copy refetch once the fetch lands**,
/// rather than follow with a hole in it.
#[test]
fn notices_held_past_the_cap_make_the_landed_copy_stale() {
    let mut follower = SceneFollower::new(vocabulary());
    let op = shift(1.0);
    let held = u64::try_from(MAX_BUFFERED_NOTICES).expect("fits");
    for revision in 1..=held + 1 {
        follower.take(notice(revision, &op));
    }
    assert_eq!(follower.held.len(), MAX_BUFFERED_NOTICES);
    let files = one_block().files().expect("saves");
    follower.adopt(FetchedScene { revision: 0, files }, Duration::ZERO);
    assert!(follower.is_stale());
    assert_eq!(follower.revision(), Some(held), "what was held applied");
}

/// A refused fetch holds the next one back by [`FETCH_RETRY_DELAY`].
#[test]
fn a_refused_fetch_waits_before_the_next() {
    let mut rig = Rig::new(one_block());
    rig.server
        .document_mut()
        .play()
        .expect("the block scene plays");
    rig.join();
    rig.until(|rig| rig.follower().last_failure().is_some());
    let refused_at = rig.now;
    let failure = rig.follower().last_failure().expect("refused").to_owned();
    assert!(failure.contains("not editable"), "{failure}");
    assert_eq!(rig.follower().fetch_count(), 1);
    rig.until(|rig| rig.follower().fetch_count() == 2);
    assert!(rig.now >= refused_at + FETCH_RETRY_DELAY);

    rig.server.document_mut().stop().expect("play stops");
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served);
}

mod gestures;
