//! The editor's server over `InMemoryTransport`, with real
//! `crcbl::client::Client`s: every command kind applied as the editor's own
//! document applies it, each refusal's code, the notice another client
//! mirrors the scene from, and the one history's undo and redo.

use std::collections::BTreeMap;
use std::path::Path;

use crcbl::client::Client;
use crcbl::net::{InMemoryTransport, ProtocolCompatibility};
use crcbl::reflect::Value;
use crcbl::scene::edit::{EditCommand, SystemRow, encode_op};
use crcbl::scene::scn::{EntityName, SceneEntityId};

use super::*;
use crate::scene::{BLOCKS, vocabulary};

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0000_4544_4954,
    schema_hash: 0x0000_5345_5256,
};

const TICK_HZ: u32 = 60;

/// Puppet's sun, which the shipped vocabulary registers and the compiled-in
/// scene does not list: the system the listing and the attach go through.
const SUN: &str = "sun";

/// An id the compiled-in scene does not hold.
const ABSENT: SceneEntityId = SceneEntityId(999);

/// A server and two clients of it, both in session, stepped together one
/// tick at a time.
struct Rig {
    server: EditServer,
    author: Client<InMemoryTransport>,
    other: Client<InMemoryTransport>,
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
        let mut client = || {
            let (near, far) = InMemoryTransport::pair();
            server.host_mut().add(Box::new(far));
            Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY)
        };
        let author = client();
        let other = client();
        let mut rig = Self {
            server,
            author,
            other,
            now: Duration::ZERO,
        };
        for _ in 0..600 {
            rig.step();
            if rig.author.session_id().is_some() && rig.other.session_id().is_some() {
                // A tick more, so each has opened what it was sent.
                rig.step();
                return rig;
            }
        }
        panic!("the clients never joined the server");
    }

    fn step(&mut self) {
        self.now += Duration::from_secs(1) / TICK_HZ;
        self.server.update(self.now);
        self.author.update(self.now);
        self.other.update(self.now);
    }

    /// Sends `op` from the author and steps until its reply arrives.
    fn send(&mut self, op: &EditOp) -> EditOutcome {
        self.send_bytes(encode_op(op).expect("every op here travels"))
    }

    /// Sends `bytes` as an operation from the author and steps until its
    /// reply arrives.
    fn send_bytes(&mut self, bytes: Vec<u8>) -> EditOutcome {
        let id = self.author.send_edit(bytes).expect("in session");
        for _ in 0..60 {
            self.step();
            let replies: Vec<EditReply> = self.author.edit_replies().collect();
            if let Some(reply) = replies.into_iter().find(|reply| reply.request_id == id) {
                return reply.outcome;
            }
        }
        panic!("no reply to request {id}");
    }
}

fn files(document: &mut Document) -> BTreeMap<String, String> {
    document.files().expect("the scene saves")
}

fn name(text: &str) -> EntityName {
    EntityName::new(text).expect("a name")
}

fn row(system: &str) -> String {
    vocabulary()
        .default_row(system)
        .expect("a registered system")
}

/// One command of every kind that travels, in an order each can apply in:
/// a property, a name, a spawn, a listing, an attach and a detach through
/// it, the unlisting, a delete, and a batch.
fn every_command() -> Vec<EditCommand> {
    let new = SceneEntityId(10);
    vec![
        EditCommand::SetProperty {
            entity: SceneEntityId(1),
            system: BLOCKS.to_owned(),
            path: "position.1".to_owned(),
            value: Value::Float(2.5),
        },
        EditCommand::Rename {
            entity: SceneEntityId(2),
            name: Some(name("Gate")),
        },
        EditCommand::Spawn {
            entity: new,
            rows: vec![SystemRow {
                system: BLOCKS.to_owned(),
                row: row(BLOCKS),
            }],
            name: Some(name("Fresh")),
        },
        EditCommand::ListSystem {
            system: SUN.to_owned(),
            at: 1,
        },
        EditCommand::Attach {
            entity: new,
            system: SUN.to_owned(),
            row: row(SUN),
        },
        EditCommand::Detach {
            entity: new,
            system: SUN.to_owned(),
        },
        EditCommand::UnlistSystem {
            system: SUN.to_owned(),
        },
        EditCommand::Delete { entity: new },
        EditCommand::Batch(vec![
            EditCommand::Delete {
                entity: SceneEntityId(3),
            },
            EditCommand::Rename {
                entity: SceneEntityId(2),
                name: None,
            },
        ]),
    ]
}

/// **Every command kind applies, and the server's scene is the editor's own
/// for the same command**, compared as saved text after each; every reply
/// names the next revision.
#[test]
fn every_command_kind_applies_exactly_as_the_editors_document_applies_it() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let mut reference = Document::built_in().expect("the compiled-in scene");
    for (index, command) in every_command().into_iter().enumerate() {
        reference
            .apply(command.clone())
            .expect("the editor applies it");
        let outcome = rig.send(&EditOp::Apply(command.clone()));
        let revision = u64::try_from(index + 1).expect("a handful of commands");
        assert_eq!(outcome, EditOutcome::Applied { revision }, "{command:?}");
        assert_eq!(
            files(rig.server.document_mut()),
            files(&mut reference),
            "after {command:?}"
        );
    }
    assert_eq!(rig.server.revision(), 9);
}

/// **Another client sees each change**: the notices reach it in revision
/// order, authored by the sender, and its own copy of the scene applying them
/// is the server's scene.
#[test]
fn another_client_mirrors_the_scene_from_the_notices() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let mut mirror = Document::built_in().expect("the compiled-in scene");
    let author = rig
        .server
        .host()
        .peers()
        .next()
        .expect("the author joined first");
    for command in every_command() {
        rig.send(&EditOp::Apply(command));
    }
    rig.send(&EditOp::Undo);
    rig.step();
    let notices: Vec<EditNotice> = rig.other.edit_notices().collect();
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.revision)
            .collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    for notice in &notices {
        assert_eq!(notice.author, author.get());
        match decode_op(&notice.op).expect("the server sends what it applied") {
            EditOp::Apply(command) => mirror.apply(command).expect("the mirror applies it"),
            EditOp::Undo => assert!(mirror.undo().expect("the mirror undoes it")),
            EditOp::Redo => assert!(mirror.redo().expect("the mirror redoes it")),
        }
    }
    assert_eq!(files(&mut mirror), files(rig.server.document_mut()));
    // The author hears of its own edits too, so its copy follows the same way.
    assert_eq!(rig.author.edit_notices().count(), notices.len());
}

/// **An undo issued through the server steps its one history**: it restores
/// the scene as it stood before the last edit, a redo puts the edit back,
/// and an undo with nothing left to undo is refused by code.
#[test]
fn an_undo_through_the_server_restores_the_scene_and_a_redo_reapplies() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let before = files(rig.server.document_mut());
    let edit = every_command().remove(0);
    rig.send(&EditOp::Apply(edit));
    let after = files(rig.server.document_mut());
    assert_ne!(after, before, "the edit changed the scene");

    assert_eq!(
        rig.send(&EditOp::Undo),
        EditOutcome::Applied { revision: 2 }
    );
    assert_eq!(files(rig.server.document_mut()), before);
    assert_eq!(
        rig.send(&EditOp::Redo),
        EditOutcome::Applied { revision: 3 }
    );
    assert_eq!(files(rig.server.document_mut()), after);

    assert_eq!(
        rig.send(&EditOp::Undo),
        EditOutcome::Applied { revision: 4 }
    );
    assert!(matches!(
        rig.send(&EditOp::Undo),
        EditOutcome::Refused {
            reason: EditRefusal::NOTHING_TO_UNDO,
            ..
        }
    ));
    assert_eq!(
        rig.send(&EditOp::Redo),
        EditOutcome::Applied { revision: 5 }
    );
    assert!(matches!(
        rig.send(&EditOp::Redo),
        EditOutcome::Refused {
            reason: EditRefusal::NOTHING_TO_REDO,
            ..
        }
    ));
    assert_eq!(rig.server.revision(), 5, "a refusal is no revision");
}

/// The code a refused `op` comes back with, and the scene left as it was.
fn refusal(rig: &mut Rig, op: &EditOp) -> EditRefusal {
    let before = files(rig.server.document_mut());
    let outcome = rig.send(op);
    assert_eq!(
        files(rig.server.document_mut()),
        before,
        "{op:?} changed it"
    );
    match outcome {
        EditOutcome::Refused { reason, message } => {
            assert!(!message.is_empty(), "{op:?} was refused without a word");
            reason
        }
        EditOutcome::Applied { revision } => panic!("{op:?} applied at {revision}"),
    }
}

/// **Each refusal carries its code**: a stale entity, a field the component
/// does not have, a value its rule refuses, a value of the wrong kind, a
/// system the scene does not list, a spawn over an id in use — and nobody
/// hears of a refused edit.
#[test]
fn each_refusal_carries_its_reason_code_and_changes_nothing() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let set = |entity, path: &str, value| {
        EditOp::Apply(EditCommand::SetProperty {
            entity,
            system: BLOCKS.to_owned(),
            path: path.to_owned(),
            value,
        })
    };
    let cases = [
        (
            set(ABSENT, "position.1", Value::Float(1.0)),
            EditRefusal::UNKNOWN_ENTITY,
        ),
        (
            set(SceneEntityId(1), "nowhere", Value::Float(1.0)),
            EditRefusal::UNKNOWN_PATH,
        ),
        (
            set(SceneEntityId(1), "half_extents.0", Value::Float(-1.0)),
            EditRefusal::INVALID,
        ),
        (
            set(SceneEntityId(1), "position.1", Value::Text("up".to_owned())),
            EditRefusal::INVALID,
        ),
        (
            EditOp::Apply(EditCommand::Attach {
                entity: SceneEntityId(1),
                system: SUN.to_owned(),
                row: row(SUN),
            }),
            EditRefusal::UNKNOWN_SYSTEM,
        ),
        (
            EditOp::Apply(EditCommand::Spawn {
                entity: SceneEntityId(1),
                rows: vec![SystemRow {
                    system: BLOCKS.to_owned(),
                    row: row(BLOCKS),
                }],
                name: None,
            }),
            EditRefusal::CONFLICT,
        ),
    ];
    for (op, code) in cases {
        assert_eq!(refusal(&mut rig, &op), code, "{op:?}");
    }
    assert_eq!(rig.server.revision(), 0);
    rig.step();
    assert_eq!(
        rig.other.edit_notices().count(),
        0,
        "a refusal is no notice"
    );
}

/// **Bytes that are no operation are refused as malformed**, and an operation
/// in another wire version as unsupported — each answered by the request's
/// own id, since the envelope around them was whole.
#[test]
fn bytes_that_are_no_operation_are_refused_by_code() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let garbage = [
        Vec::new(),
        vec![crcbl::scene::edit::WIRE_VERSION, 0, 0x7F],
        vec![crcbl::scene::edit::WIRE_VERSION, 0, 0x04, 1],
    ];
    for bytes in garbage {
        let outcome = rig.send_bytes(bytes.clone());
        assert!(
            matches!(
                outcome,
                EditOutcome::Refused {
                    reason: EditRefusal::MALFORMED,
                    ..
                }
            ),
            "{bytes:?}: {outcome:?}"
        );
    }
    let outcome = rig.send_bytes(vec![crcbl::scene::edit::WIRE_VERSION + 1, 1]);
    assert!(
        matches!(
            outcome,
            EditOutcome::Refused {
                reason: EditRefusal::UNSUPPORTED_VERSION,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(rig.server.revision(), 0);
}

/// **A scene that is playing refuses every edit as not editable** — towers'
/// committed field, with towers' game running in it — and takes edits again
/// once play stops.
#[test]
fn a_playing_scene_refuses_edits_as_not_editable() {
    let field = Document::open(
        &crcbl_towers::built_in_source(),
        Path::new(crcbl_towers::FIELD),
        vocabulary(),
    )
    .expect("the shipped vocabulary opens towers' field");
    let mut rig = Rig::new(field);
    rig.server
        .document_mut()
        .play()
        .expect("towers plays its committed field");
    let rename = EditOp::Apply(EditCommand::Rename {
        entity: SceneEntityId(0),
        name: Some(name("Start")),
    });
    assert_eq!(refusal(&mut rig, &rename), EditRefusal::NOT_EDITABLE);
    assert_eq!(refusal(&mut rig, &EditOp::Undo), EditRefusal::NOT_EDITABLE);

    rig.server.document_mut().stop().expect("play stops");
    assert_eq!(rig.send(&rename), EditOutcome::Applied { revision: 1 });
}

/// A refusal message past what a reply carries is cut on a character
/// boundary rather than refused itself, so the author still hears why.
#[test]
fn a_long_refusal_message_is_cut_to_what_a_reply_carries() {
    let long = "é".repeat(MAX_EDIT_MESSAGE_BYTES);
    let EditOutcome::Refused { reason, message } = refused(EditRefusal::FAILED, long) else {
        panic!("a refusal");
    };
    assert_eq!(reason, EditRefusal::FAILED);
    assert!(message.len() <= MAX_EDIT_MESSAGE_BYTES);
    assert!(message.len() > MAX_EDIT_MESSAGE_BYTES - 'é'.len_utf8());
    assert!(message.chars().all(|c| c == 'é'));
}

/// Each request has its own id, and each reply echoes the id of the request
/// it answers, in the order they were sent.
#[test]
fn a_reply_echoes_the_request_id_it_answers() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let first = rig
        .author
        .send_edit(encode_op(&EditOp::Undo).expect("an undo"))
        .expect("in session");
    let second = rig
        .author
        .send_edit(encode_op(&EditOp::Redo).expect("a redo"))
        .expect("in session");
    assert_ne!(first, second, "each request has its own id");
    let mut ids = Vec::new();
    for _ in 0..60 {
        rig.step();
        ids.extend(rig.author.edit_replies().map(|reply| reply.request_id));
        if ids.len() == 2 {
            break;
        }
    }
    assert_eq!(ids, [first, second]);
}

/// Longer than the host holds a session whose client has sent nothing under
/// its key (`crcbl_server`'s `AUTHENTICATION_DEADLINE`).
const IDLE: Duration = Duration::from_secs(15);

/// **A client that never edits stays a client.** The host's world is empty —
/// the scene is the document's — but a client's own sealed traffic each tick
/// still proves it holds its key, so a mirror that only listens is not ended
/// as one that never took its session up.
#[test]
fn a_client_that_only_listens_stays_in_session() {
    let mut rig = Rig::new(Document::built_in().expect("the compiled-in scene"));
    let ticks = IDLE.as_secs() * u64::from(TICK_HZ);
    for _ in 0..ticks {
        rig.step();
    }
    assert_eq!(rig.server.host().peer_count(), 2);
    assert_eq!(rig.other.ended(), None);
}
