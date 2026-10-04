//! The edit server: a [`Document`] served over a [`Host`], so that
//! every client of it — the GUI, the CLI, a script — edits one scene through
//! one history.
//!
//! `docs/plan/08-editor.md`'s architecture: the editor is a client+server
//! pair, and every edit is a command through the transport. This is the
//! server: it takes each [`EditRequest`](crate::net::EditRequest) a peer
//! sends, decodes its operation ([`decode_op`]), and applies it **through
//! [`Document::apply`], [`Document::undo`] and [`Document::redo`]** — the
//! path a key press in the editor takes, with its validation, its refusal in
//! play mode and its undo log. Nothing here is a second set of rules.
//!
//! # The decisions it carries (2026-10-04)
//!
//! * **Any admitted peer may edit.** The plan's 2026-07-27 correction has a GUI
//!   and a CLI editing one scene at once, with one global history and
//!   last-writer-wins; a headless server adds no host player, so restricting
//!   edits to one — as console sets of simulation variables are — would leave
//!   the CLI nothing to edit. Admission is the handshake's: the compatibility
//!   identifiers, and a session key every request is sealed under.
//! * **Undo and redo are part of the protocol**, as [`EditOp::Undo`] and
//!   [`EditOp::Redo`], and they step the server's one history — the most
//!   recent entry, whoever made it, which is the correction's rule. A client
//!   keeps no history of its own to undo.
//! * **Every applied operation is announced to every client, its author
//!   included**, as an [`EditNotice`] carrying the operation's own bytes and
//!   the revision it brought the server to, sent before the author's reply. A
//!   client applying the notices in revision order to a copy of the scene
//!   reaches the server's scene: the command log is the sync point. Snapshot
//!   replication does not carry it — it carries the world's replicated
//!   components, and a rename or a row's field has none — and the document's
//!   world is not the host's.
//! * **A refusal is reason-coded** ([`EditRefusal`]) with the document's own
//!   message beside it, so a script branches on the code and a person reads
//!   the sentence.
//! * **A scene fetch is answered from the document as it stands** — its saved
//!   text, at the current revision, after the edits of the same update — so
//!   a client joining late follows the notices after it
//!   ([`SceneFollower`](super::SceneFollower)). It is refused as not editable
//!   while the scene plays, as an edit is: the played world is not the scene
//!   that was authored, and no notice follows it.

use std::time::Duration;

use crate::ecs::World;
use crate::net::{
    EditNotice, EditOutcome, EditRefusal, EditReply, MAX_EDIT_MESSAGE_BYTES, encode_scene_files,
};
use crate::reflect::PathError;
use crate::scene::edit::{EditOp, OpDecodeError, decode_op};
use crate::scene::scn::ScnError;
use crate::server::{EventNotSent, Host, HostConfig, PeerId};

use crate::scene_edit::{Document, EditError, PlayState};

/// A document, and the host its clients edit it through.
#[derive(Debug)]
pub struct EditServer {
    host: Host,
    document: Document,
    /// How many operations have applied since serving began — the revision
    /// each reply and notice names.
    revision: u64,
    /// Replies, and answers to scene fetches, whose peer could not be sent
    /// one: gone, or its link down.
    unsent_replies: u64,
}

impl EditServer {
    /// Serves `document` over a host built from `config`, with nobody
    /// connected yet; [`host_mut`](Self::host_mut) is where transports are
    /// added.
    ///
    /// The host's own world is empty: the scene lives in the document, and
    /// what reaches the clients of it is the notices, not snapshots.
    ///
    /// # Panics
    ///
    /// As [`Host::new`]: a `config` admitting no peer, ticking at zero, or
    /// with a zero compatibility identifier.
    #[must_use]
    pub fn new(document: Document, config: HostConfig) -> Self {
        let mut host = Host::new(World::new(), config);
        host.serve_edits();
        Self {
            host,
            document,
            revision: 0,
            unsent_replies: 0,
        }
    }

    /// Feeds the host the current time, then answers every edit its peers
    /// sent: each applied or refused, in the order the host read them, and
    /// each applied one announced to every client. Then it answers every
    /// scene fetch with the scene as those edits left it. Returns how many
    /// ticks ran.
    pub fn update(&mut self, now: Duration) -> u32 {
        let ticks = self.host.update(now);
        for (peer, request) in self.host.take_edit_requests() {
            let outcome = self.perform(peer, &request.op);
            let reply = EditReply {
                request_id: request.request_id,
                outcome,
            };
            if self.host.send_edit_reply(peer, &reply).is_err() {
                self.unsent_replies += 1;
            }
        }
        for (peer, fetch_id) in self.host.take_scene_fetches() {
            if self.answer_fetch(peer, fetch_id).is_err() {
                self.unsent_replies += 1;
            }
        }
        ticks
    }

    /// Sends `peer` the scene as it stands, at the current revision, or
    /// refuses its fetch: while the scene plays, and for a scene that will
    /// not save or is past what a fetch carries.
    fn answer_fetch(&mut self, peer: PeerId, fetch_id: u64) -> Result<(), EventNotSent> {
        let refusal = if self.document.play_state() == PlayState::Editing {
            match self.document.files() {
                Ok(files) => match encode_scene_files(&files) {
                    Ok(scene) => return self.host.send_scene(peer, fetch_id, self.revision, scene),
                    Err(too_large) => (EditRefusal::TOO_LARGE, too_large.to_string()),
                },
                Err(error) => (EditRefusal::FAILED, error.to_string()),
            }
        } else {
            (
                EditRefusal::NOT_EDITABLE,
                "the scene is playing; fetch it again once play stops".to_owned(),
            )
        };
        let (reason, message) = refusal;
        self.host
            .refuse_scene_fetch(peer, fetch_id, reason, cut_to_a_reply(message))
    }

    /// Applies one operation for `peer` and says what became of it,
    /// announcing it to every client when it applied.
    fn perform(&mut self, peer: PeerId, op: &[u8]) -> EditOutcome {
        let decoded = match decode_op(op) {
            Ok(decoded) => decoded,
            Err(error) => return refused(refusal_of_decode(&error), error.to_string()),
        };
        let stepped = match &decoded {
            EditOp::Apply(command) => self.document.apply(command.clone()).map(|()| true),
            EditOp::Undo => self.document.undo(),
            EditOp::Redo => self.document.redo(),
        };
        match stepped {
            Ok(true) => {
                self.revision += 1;
                self.host
                    .broadcast_edit_notice(&EditNotice {
                        revision: self.revision,
                        author: peer.get(),
                        op: op.to_vec(),
                    })
                    .expect("an op read off a request fits a notice: the two share a limit");
                EditOutcome::Applied {
                    revision: self.revision,
                }
            }
            Ok(false) if matches!(decoded, EditOp::Redo) => refused(
                EditRefusal::NOTHING_TO_REDO,
                "there is nothing undone to redo".to_owned(),
            ),
            // Only a step of the history finds nothing to step over.
            Ok(false) => refused(
                EditRefusal::NOTHING_TO_UNDO,
                "there is nothing in the history to undo".to_owned(),
            ),
            Err(error) => refused(refusal_of(&error), error.to_string()),
        }
    }

    /// The document being served.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// The document being served, for what the serving editor does itself —
    /// play, stop and save. An edit made here reaches no client: edits go
    /// through the clients, so each hears of every one.
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// The host, for its peers and counters.
    #[must_use]
    pub const fn host(&self) -> &Host {
        &self.host
    }

    /// The host, to add a client's transport to.
    pub fn host_mut(&mut self) -> &mut Host {
        &mut self.host
    }

    /// How many operations have applied since serving began.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Replies, and answers to scene fetches, that could not be sent because
    /// their peer had gone or its link was down. The operation applied, or
    /// was refused, all the same.
    #[must_use]
    pub const fn unsent_reply_count(&self) -> u64 {
        self.unsent_replies
    }
}

/// The code a document's refusal travels as — to a client of the server, and
/// as the exit code of the `crcbl` CLI's edits, so both answer one refusal
/// with one reason.
#[must_use]
pub fn refusal_of(error: &EditError) -> EditRefusal {
    match error {
        EditError::Playing => EditRefusal::NOT_EDITABLE,
        EditError::NoEntity(_) => EditRefusal::UNKNOWN_ENTITY,
        EditError::NoSystem(_) | EditError::Scene(ScnError::NoCodec { .. }) => {
            EditRefusal::UNKNOWN_SYSTEM
        }
        EditError::Path(PathError::Set(_)) => EditRefusal::INVALID,
        EditError::Path(_) => EditRefusal::UNKNOWN_PATH,
        // A row that will not read is a value refused, as a leaf's is.
        EditError::Invalid { .. }
        | EditError::FieldPaste { .. }
        | EditError::Name(_)
        | EditError::NoComponent(_)
        | EditError::Asset(_)
        | EditError::Scene(_) => EditRefusal::INVALID,
        EditError::IdInUse(_)
        | EditError::Listed(_)
        | EditError::PastManifest { .. }
        | EditError::Populated(_)
        | EditError::NotAttached { .. }
        | EditError::Attached { .. } => EditRefusal::CONFLICT,
        _ => EditRefusal::FAILED,
    }
}

/// The code bytes that are not an operation travel as.
fn refusal_of_decode(error: &OpDecodeError) -> EditRefusal {
    match error {
        OpDecodeError::Version { .. } => EditRefusal::UNSUPPORTED_VERSION,
        _ => EditRefusal::MALFORMED,
    }
}

/// A refusal carrying `message`, cut to what a reply carries.
fn refused(reason: EditRefusal, message: String) -> EditOutcome {
    EditOutcome::Refused {
        reason,
        message: cut_to_a_reply(message),
    }
}

/// `message` cut to what a reply carries, on a character boundary.
fn cut_to_a_reply(mut message: String) -> String {
    if message.len() > MAX_EDIT_MESSAGE_BYTES {
        let cut = (0..=MAX_EDIT_MESSAGE_BYTES)
            .rev()
            .find(|&at| message.is_char_boundary(at))
            .unwrap_or(0);
        message.truncate(cut);
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
