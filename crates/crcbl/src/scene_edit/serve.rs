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
//!
//! # A client's drag is one entry (decided 2026-10-05)
//!
//! A client sends a drag as one edit a frame, each naming the same
//! [`EditGesture`] of its own numbering, and the server records them through
//! [`Document::apply_in`] in one [`Gesture`] of the document's — the fold a
//! drag in the editor takes, not a second one — so the drag is one entry of
//! the history and one undo walks all of it back.
//!
//! * **A gesture's edits fold only while nothing comes between them.** The
//!   entry stops taking them — the gesture is _sealed_ — at its last edit
//!   ([`EditGesture::last`], applied or refused), at any other operation
//!   applied for anyone (an edit outside the gesture, another gesture, an
//!   undo or a redo, the same client's or another's), when its client's link
//!   is found down, when a fetch is answered, and when the document's own
//!   log seals the entry (a save through
//!   [`document_mut`](EditServer::document_mut)). An edit of the gesture
//!   after that starts an entry of its own. Interleaving seals because the
//!   history is one list and only the entry on top can fold: two clients'
//!   drags at once are entries in the order their frames came, as two
//!   people's edits are.
//! * **A refused edit seals nothing but its own gesture's last**: it changed
//!   nothing, so what comes after it folds as if it had not been sent.
//! * **The notice names the document's gesture, not the client's**
//!   ([`EditNotice::gesture`](crate::net::EditNotice::gesture)), so a copy
//!   records each edit in the same gesture the server did and its history
//!   folds exactly as the server's: the same entries, so an undo over the
//!   protocol walks back the same drag on the server and on every copy. A
//!   seal needs no notice of its own — the gesture's next edit simply names
//!   another number, and the copy pushes where the server pushed.
//! * **Answering a fetch seals**: the copy it opens holds no history, so a
//!   drag carried on into the entry it began before the fetch would be the
//!   whole drag on the server and only its end on the copy, and their undos
//!   would differ.
//! * **An undo or a redo in a gesture is refused as malformed**: a gesture
//!   is the writes of one drag, and a step of the history is none of them.
//!
//! # What a client and its server agree on (decided 2026-10-05)
//!
//! A client of a served scene connects with [`EDIT_PROTOCOL_ID`], ticks at
//! [`EDIT_TICK_HZ`] and hand-shakes with [`edit_compatibility`] of its own
//! vocabulary. The schema identifier is a digest of that vocabulary — every
//! system's name and its component's type — so a client built with another
//! set of components is refused by the handshake, before it fetches a scene
//! it could not open or sends a command naming a system the server lacks.

use std::time::Duration;

use crate::ecs::World;
use crate::net::{
    EditGesture, EditNotice, EditOutcome, EditRefusal, EditReply, MAX_EDIT_MESSAGE_BYTES,
    ProtocolCompatibility, SessionState, encode_scene_files,
};
use crate::reflect::PathError;
use crate::registry::Registry;
use crate::scene::edit::{EditOp, Gesture, OpDecodeError, decode_op};
use crate::scene::scn::ScnError;
use crate::server::{EventNotSent, Host, HostConfig, PeerId};
use crate::shaders::sha256::sha256;

use crate::scene_edit::{Document, EditError, PlayState};

/// The endpoint protocol id an edit server's links speak: it spells `CRED`.
/// A listener or a client of another — a game's — answers nothing.
pub const EDIT_PROTOCOL_ID: u32 = u32::from_be_bytes(*b"CRED");

/// The rate an edit server's host ticks at, and its clients with it. Its
/// world is empty, so a tick carries only the links' own traffic; the rate
/// sets how soon an edit or a fetch's next part goes out after it is read.
pub const EDIT_TICK_HZ: u32 = 60;

/// The build identifier an edit session hand-shakes on: it spells `CRCBL`,
/// as the samples' sessions do. What can differ between two builds of the
/// edit protocol is the vocabulary, which [`edit_compatibility`] digests.
const EDIT_BUILD_ID: u64 = 0x0043_5243_424C;

/// The identifiers a client and the edit server serving it must share: the
/// engine's wire version, `EDIT_BUILD_ID`, and a digest of `vocabulary` —
/// each system's name and its component's type, in name order, each part
/// as its length in eight little-endian bytes and then its bytes, so no two
/// vocabularies run together into the same bytes. See the module docs.
#[must_use]
pub fn edit_compatibility(vocabulary: &Registry) -> ProtocolCompatibility {
    let mut bytes = Vec::new();
    for system in vocabulary.systems() {
        let component = vocabulary.component_type(system).unwrap_or_default();
        for part in [system, component] {
            bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
            bytes.extend_from_slice(part.as_bytes());
        }
    }
    let digest = sha256(&bytes);
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: EDIT_BUILD_ID,
        // The handshake refuses a zero identifier, which a digest can be.
        schema_hash: u64::from_le_bytes(first).max(1),
    }
}

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
    /// The client's gesture the entry on top was recorded in, until it is
    /// sealed — see the module docs.
    open: Option<OpenGesture>,
}

/// A client's gesture whose entry may still fold: whose it is, the client's
/// number for it, and the document's gesture its edits are recorded in.
#[derive(Clone, Copy, Debug)]
struct OpenGesture {
    peer: PeerId,
    id: u32,
    gesture: Gesture,
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
            open: None,
        }
    }

    /// Feeds the host the current time, then answers every edit its peers
    /// sent: each applied or refused, in the order the host read them, and
    /// each applied one announced to every client. Then it answers every
    /// scene fetch with the scene as those edits left it. Returns how many
    /// ticks ran.
    pub fn update(&mut self, now: Duration) -> u32 {
        let ticks = self.host.update(now);
        // A link that went down took whatever frames of the drag it still
        // held; what follows from that client, resumed, is a drag of its own.
        if let Some(open) = self.open
            && self.host.peer_state(open.peer) != Some(SessionState::Connected)
        {
            self.open = None;
        }
        for (peer, request) in self.host.take_edit_requests() {
            let outcome = self.perform(peer, request.gesture, &request.op);
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
                    Ok(scene) => {
                        self.host.send_scene(peer, fetch_id, self.revision, scene)?;
                        // The copy opened from it holds no history: see the
                        // module docs.
                        self.open = None;
                        return Ok(());
                    }
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

    /// Applies one operation for `peer`, in `gesture` when it is part of one,
    /// and says what became of it, announcing it to every client when it
    /// applied.
    fn perform(&mut self, peer: PeerId, gesture: Option<EditGesture>, op: &[u8]) -> EditOutcome {
        let decoded = match decode_op(op) {
            Ok(decoded) => decoded,
            Err(error) => return refused(refusal_of_decode(&error), error.to_string()),
        };
        let recorded = match (&decoded, gesture) {
            (EditOp::Apply(_), Some(wire)) => Some(
                self.carried_on(peer, wire.id)
                    .unwrap_or_else(|| self.document.begin_gesture()),
            ),
            (EditOp::Undo | EditOp::Redo, Some(_)) => {
                return refused(
                    EditRefusal::MALFORMED,
                    "an undo or a redo is no part of a gesture".to_owned(),
                );
            }
            (_, None) => None,
        };
        let stepped = match &decoded {
            EditOp::Apply(command) => match recorded {
                Some(recorded) => self.document.apply_in(command.clone(), recorded),
                None => self.document.apply(command.clone()),
            }
            .map(|()| true),
            EditOp::Undo => self.document.undo(),
            EditOp::Redo => self.document.redo(),
        };
        if matches!(stepped, Ok(true)) {
            self.open =
                gesture
                    .zip(recorded)
                    .filter(|(wire, _)| !wire.last)
                    .map(|(wire, gesture)| OpenGesture {
                        peer,
                        id: wire.id,
                        gesture,
                    });
        } else if let Some(wire) = gesture.filter(|wire| wire.last)
            && self.carried_on(peer, wire.id).is_some()
        {
            self.open = None;
        }
        match stepped {
            Ok(true) => {
                self.revision += 1;
                self.host
                    .broadcast_edit_notice(&EditNotice {
                        revision: self.revision,
                        author: peer.get(),
                        gesture: recorded.map(|recorded| recorded.0),
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

    /// The document's gesture `peer`'s gesture `id` is recorded in, while its
    /// entry may still fold: on top of the history and unsealed.
    fn carried_on(&self, peer: PeerId, id: u32) -> Option<Gesture> {
        self.open
            .filter(|open| {
                open.peer == peer
                    && open.id == id
                    && self.document.log().open_gesture() == Some(open.gesture)
            })
            .map(|open| open.gesture)
    }

    /// Whether a client's gesture is open: its edits so far one entry of the
    /// history that its next edit may still fold into — see the module docs.
    /// A save of the document seals that entry, so a caller saving after
    /// every edit waits while this holds, or a drag is one entry per save.
    #[must_use]
    pub fn gesture_open(&self) -> bool {
        self.open
            .is_some_and(|open| self.document.log().open_gesture() == Some(open.gesture))
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

    /// **The schema identifier is the vocabulary's**: the same components
    /// give the same identifier, and one more system, the same component
    /// under another name, or another component under the same name, each
    /// give another.
    #[test]
    fn the_handshake_identifies_the_vocabulary() {
        use super::super::tests::{BLOCKS, vocabulary};
        use crate::scene_physics::Body;

        let same = edit_compatibility(&vocabulary());
        assert_eq!(edit_compatibility(&vocabulary()), same);
        same.assert_explicit();

        let mut more = vocabulary();
        crate::scene_physics::register(&mut more);
        let mut renamed = Registry::new();
        renamed.register::<Body>(crate::scene_physics::BODIES);
        let mut retyped = Registry::new();
        retyped.register::<Body>(BLOCKS);
        let mut moved = Registry::new();
        moved.register::<Body>("blocks2");
        for other in [more, retyped, Registry::new()] {
            assert_ne!(edit_compatibility(&other), same);
        }
        assert_ne!(edit_compatibility(&renamed), edit_compatibility(&moved));
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
}
