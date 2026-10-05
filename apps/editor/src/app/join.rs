//! Joining a scene `crcbl edit <DIR> --serve` serves: the editor as a client
//! of the edit protocol, following the served scene and sending its edits.
//!
//! `editor --join <IP:PORT>`, or the address typed on Ctrl+O's path line,
//! connects with [`EDIT_PROTOCOL_ID`](crcbl::scene_edit::serve::EDIT_PROTOCOL_ID)
//! and hand-shakes on
//! [`edit_compatibility`] of this build's vocabulary, so a build with other
//! components is refused before it fetches a scene it could not open; the
//! refusal is said on the status line. The scene is then fetched and
//! followed through a [`SceneFollower`], and the copy it holds **is the
//! document the editor shows**.
//!
//! # Every edit goes to the server (decided 2026-10-05)
//!
//! The copy's edits are routed (`crcbl::scene_edit`'s `route` module): each
//! one the panels, the keys and the gizmo make is held instead of applied,
//! and sent at the end of the frame; the copy changes only when the
//! server's notice comes back, applied by the follower. So the copy is the
//! server's scene and nothing else, a refused edit was never shown, and
//! nothing needs reconciling. **Applied on the notice, not optimistically**:
//! an edit shows a round trip after it is made — a frame or two on one
//! machine, the link's latency across a network — and a drag trails the
//! pointer by as much. Applying first would hide that, at the price of
//! undoing local edits whenever another client's notice landed between
//! them, and of a history that no longer folds as the server's does, which
//! an undo over the protocol needs.
//!
//! * **A drag is one gesture**: each frame of one goes with the same id, of
//!   this editor's numbering, and the frame the button comes up on is marked
//!   last — or, when the release brought no frame of its own, the drag's
//!   last frame is sent again marked last — so the server's history gains
//!   one entry and `crcbl edit --serve` saves it when it ends. The pointer
//!   coming up is the end for the gizmo and a panel's field drag alike: both
//!   are gestures only while it is held.
//! * **Undo and redo step the server's one history**, the most recent entry
//!   whoever made it, as every client's do.
//! * **Edits made faster than the round trip compose**: a nudge goes as an
//!   offset and a spawn asks the server for fresh ids (`crcbl::scene_edit`'s
//!   `route` module), so two nudges move by both and two spawns — or two
//!   clients' at once — both land.
//! * **What this editor spawns is selected once it lands**, as a spawn,
//!   paste or duplicate is selected at once in an editor of its own scene:
//!   the reply to the edit names the revision whose notice spawned it, and
//!   that notice the ids the server gave
//!   ([`SceneFollower::take_spawned`]). Until then nothing new is selected,
//!   since the ids the document picked are only stand-ins.
//! * **A refusal is said on the status line**, the server's own sentence.
//! * **A refetched copy starts the view over**: the follower fetches again
//!   when it misses a notice, and the copy it opens is put in place as an
//!   opened scene is — the panels afresh, nothing selected.
//!
//! # What a joined editor does not do (decided 2026-10-05)
//!
//! * **It holds no lock and writes nothing into the scene's directory.** The
//!   server holds the lock and saves after every update that applied an
//!   edit; the copy has no directory of its own, and joining puts a new,
//!   empty scene in place first, which lets go of any lock the editor held.
//! * **Save and Save as save nothing** and say so: the server has already
//!   saved each edit, and the protocol has no save to ask it for — none is
//!   needed while every applied edit is saved.
//! * **No recovery copy and no autosave**: the server holds every edit
//!   applied, and a copy offered back at the next start would look like
//!   work that was lost. Nothing is unsaved while joined, so a new scene, an
//!   open or the window closing asks nothing either.
//! * **Play mode is refused**: the played world is not the authored scene,
//!   and the server refuses a fetch while its own scene plays for the same
//!   reason. Playing a local copy would need the copy unrouted and put back.
//!
//! # Leaving
//!
//! When the server quits or the link drops, the copy stays open as a
//! document of this editor's own: unrouted, with no directory, and unsaved
//! ([`Document::forget_saved`]), so nothing the person was looking at is
//! lost — Save asks for a directory, and closing asks first. A new scene,
//! an open, or another join leaves without that: the server holds the scene.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use crcbl::assets::DirSource;
use crcbl::client::{Client, Ended};
use crcbl::ecs::World;
use crcbl::net::{EditGesture, EditOutcome, SessionEndReason, Transport};
use crcbl::scene::edit::{EditOp, Gesture, encode_op};
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_edit::serve::{EDIT_TICK_HZ, edit_compatibility};
use crcbl::scene_edit::{RoutedEdit, SceneFollower};
use crcbl::shell::Shell;

use super::Editor;
use crate::document::{Document, EditError, PlayState};
use crate::keys::Action;
use crate::panel::Tone;

/// A served scene this editor has joined: the client, and the follower whose
/// copy is the document the editor shows — see the module docs.
#[derive(Debug)]
pub(super) struct Joined {
    addr: SocketAddr,
    client: Client<Box<dyn Transport>>,
    follower: SceneFollower,
    /// The drag whose frames are going out, until its last has.
    open: Option<OpenGesture>,
    /// The last gesture id handed out: this editor's own numbering.
    gestures: u32,
    /// The follower's [`SceneFollower::landed_count`] when last read.
    landed: u64,
    /// The requests sent that spawn, whose ids are selected once applied.
    selecting: Vec<u64>,
    /// What applied notices spawned, by revision, while a reply to a
    /// spawn of this editor's is awaited.
    spawned: BTreeMap<u64, Vec<SceneEntityId>>,
}

/// A drag being sent: the document's gesture its frames were made in, the id
/// they go with, and the last frame's operation, sent again marked last when
/// the drag ends without a frame of its own.
#[derive(Debug)]
struct OpenGesture {
    local: Gesture,
    id: u32,
    op: Vec<u8>,
}

/// What a frame of following brought that the editor acts on.
#[derive(Debug, Default)]
struct Followed {
    /// A copy was put in place of the one before.
    landed: bool,
    /// The server's refusals of this editor's edits, and the edits that
    /// would not go, in order.
    refusals: Vec<String>,
    /// Why the session is over, once it is.
    ended: Option<String>,
    /// What this editor's latest spawn that applied spawned, to select.
    select: Option<Vec<SceneEntityId>>,
}

impl Joined {
    /// Starts a connect to the edit server at `addr`, as `player`.
    ///
    /// # Errors
    ///
    /// Why no connect could start: no socket, or — in a browser build —
    /// no UDP at all.
    fn connect(addr: SocketAddr, player: crcbl::net::PlayerId) -> Result<Self, String> {
        let vocabulary = crate::scene::vocabulary();
        let compatibility = edit_compatibility(&vocabulary);
        Ok(Self {
            addr,
            client: Client::new_with_compatibility(
                World::new(),
                transport_to(addr)?,
                EDIT_TICK_HZ,
                compatibility,
                player,
            ),
            follower: SceneFollower::new(vocabulary),
            open: None,
            gestures: 0,
            landed: 0,
            selecting: Vec::new(),
            spawned: BTreeMap::new(),
        })
    }

    /// Sends what the copy held this frame, in order, each drag's frames
    /// under one id — the last marked so once the pointer is up (`held`
    /// false) — noting each that spawns, and hands back why any did not go.
    fn send(&mut self, routed: Vec<RoutedEdit>, held: bool) -> Vec<String> {
        let mut failures = Vec::new();
        let ending = if held {
            None
        } else {
            routed.iter().rposition(|edit| edit.gesture.is_some())
        };
        for (index, edit) in routed.into_iter().enumerate() {
            let spawns = matches!(edit.op, EditOp::ApplyFresh(_));
            let op = match encode_op(&edit.op) {
                Ok(op) => op,
                Err(error) => {
                    failures.push(error.to_string());
                    continue;
                }
            };
            let sent = match edit.gesture {
                Some(local) => {
                    let id = match &self.open {
                        Some(open) if open.local == local => open.id,
                        _ => {
                            self.gestures = self.gestures.wrapping_add(1);
                            self.gestures
                        }
                    };
                    let last = ending == Some(index);
                    self.open = (!last).then(|| OpenGesture {
                        local,
                        id,
                        op: op.clone(),
                    });
                    self.client.send_edit_in(op, EditGesture { id, last })
                }
                None => {
                    // Anything else applied seals a drag on the server.
                    self.open = None;
                    self.client.send_edit(op)
                }
            };
            match sent {
                Ok(request) if spawns => self.selecting.push(request),
                Ok(_) => {}
                Err(error) => failures.push(error.to_string()),
            }
        }
        if !held && let Some(open) = self.open.take() {
            let last = EditGesture {
                id: open.id,
                last: true,
            };
            if let Err(error) = self.client.send_edit_in(open.op, last) {
                failures.push(error.to_string());
            }
        }
        failures
    }

    /// Steps the client to `now`, and the follower with `document` as its
    /// copy — see [`Editor::step_join`] — and says what came of it.
    fn follow(&mut self, document: &mut Document, now: Duration) -> Followed {
        self.client.update(now);
        // The copy lives in the editor between updates, where it is drawn
        // and edited, and goes back into the follower for each one; what
        // the follower holds in the meantime is the document the editor had
        // before, which nothing reads. A copy the update fetched afresh
        // takes the copy's place on the way out.
        if let Some(copy) = self.follower.document_mut() {
            std::mem::swap(copy, document);
        }
        self.follower.update(&mut self.client, now);
        if let Some(copy) = self.follower.document_mut() {
            std::mem::swap(copy, document);
        }
        let landed = self.follower.landed_count() != self.landed;
        self.landed = self.follower.landed_count();
        if landed {
            // A fetch answered sealed any drag on the server.
            self.open = None;
        }
        for spawned in self.follower.take_spawned() {
            self.spawned.insert(spawned.revision, spawned.ids);
        }
        let mut refusals = Vec::new();
        let mut select = None;
        let mut answered = None;
        for reply in self.client.edit_replies() {
            let spawn = self
                .selecting
                .iter()
                .position(|&request| request == reply.request_id)
                .map(|at| self.selecting.remove(at));
            match reply.outcome {
                EditOutcome::Refused { message, .. } => refusals.push(message),
                EditOutcome::Applied { revision } => {
                    answered = Some(revision);
                    // The notice came before its reply, so its spawns are
                    // here unless a fetch passed it over, which starts the
                    // view over anyway.
                    if spawn.is_some()
                        && let Some(ids) = self.spawned.get(&revision)
                    {
                        select = Some(ids.clone());
                    }
                }
            }
        }
        // Replies come in the order the server applied the edits, so what
        // spawned at or before the last one answered is no spawn awaited.
        match answered {
            _ if self.selecting.is_empty() => self.spawned.clear(),
            Some(answered) => self.spawned.retain(|&revision, _| revision > answered),
            None => {}
        }
        let ended = if let Some(refusal) = self.client.handshake_refusal() {
            Some(format!(
                "{} refused this editor: {}",
                self.addr, refusal.msg
            ))
        } else {
            self.client
                .ended()
                .map(|ended| format!("{} {}", self.addr, how_it_ended(ended)))
        };
        Followed {
            landed,
            refusals,
            ended,
            select,
        }
    }
}

#[cfg(test)]
impl Joined {
    /// The follower, for a test to read where the copy stands.
    pub(super) const fn follower(&self) -> &SceneFollower {
        &self.follower
    }
}

/// A transport connecting to `addr`: UDP, the one the edit server listens
/// on.
///
/// # Errors
///
/// Why no socket could be bound.
#[cfg(not(target_arch = "wasm32"))]
fn transport_to(addr: SocketAddr) -> Result<Box<dyn Transport>, String> {
    crcbl::net::udp::UdpTransport::connect(addr, crcbl::scene_edit::serve::EDIT_PROTOCOL_ID)
        .map(|transport| Box::new(transport) as Box<dyn Transport>)
        .map_err(|error| format!("cannot connect to {addr}: {error}"))
}

/// A browser build has no networking (`crcbl::lan`'s _Native only_), so it
/// joins nothing, and says so.
///
/// # Errors
///
/// Always.
#[cfg(target_arch = "wasm32")]
fn transport_to(addr: SocketAddr) -> Result<Box<dyn Transport>, String> {
    Err(format!(
        "cannot join {addr}: a browser build of the editor has no UDP to join over"
    ))
}

/// How a session ended, in words, after the server's address.
fn how_it_ended(ended: Ended) -> String {
    match ended {
        Ended::ByServer(SessionEndReason::SHUTTING_DOWN) => "shut down".to_owned(),
        // A code this build does not know still ends the session.
        Ended::ByServer(reason) => format!("ended the session: {reason:?}"),
        Ended::Lost => "stopped answering".to_owned(),
    }
}

/// What the status line says while the scene is being fetched.
const JOINING: &str = "fetching the scene it serves";

/// What the status line says once the copy is in place.
const JOINED: &str = "every edit goes to the server, which saves it";

/// What the status line says when Save or Save as is asked for while joined.
const SAVED_BY_THE_SERVER: &str = "the server saves every edit itself, so nothing is saved here";

/// What the status line says when play mode is asked for while joined.
const NO_PLAY_JOINED: &str = "play mode is refused while joined: the server's scene is edited, \
                              not played";

/// What the status line says once the server is gone and the copy kept.
const KEPT_COPY: &str = "the scene stays open here as an unsaved copy with no directory, and \
                         Ctrl+S asks for one";

impl<S: Shell + ?Sized> Editor<S> {
    /// Joins the scene served at `addr` in place of the scene being edited,
    /// which goes as a new scene's predecessor does — see the module docs.
    /// Refused in play mode, as an open is. What became of it is on the
    /// status line.
    pub(super) fn join(&mut self, addr: SocketAddr) -> Result<(), EditError> {
        if self.document.play_state() != PlayState::Editing {
            return Err(EditError::Playing);
        }
        self.leave_join();
        self.new_scene()?;
        // The id this machine keeps for the editor, or a headless run's: who
        // the server's denylist and its one-session-a-player rule know.
        let joined = crcbl::store::identity::for_app(crate::layout::APP_NAME, !self.windowed)
            .map_err(|error| format!("cannot join {addr}: no player id: {error}"))
            .and_then(|player| Joined::connect(addr, player));
        match joined {
            Ok(joined) => {
                self.document.route_edits();
                self.joined = Some(Box::new(joined));
                self.panels
                    .set_status(format!("Joining {addr}: {JOINING}"), Tone::Info);
            }
            Err(why) => {
                crcbl::log::warn!("editor: {why}");
                self.panels.set_status(why, Tone::Warning);
            }
        }
        Ok(())
    }

    /// The address of the served scene this editor has joined, if it has.
    #[must_use]
    pub(super) fn joined_addr(&self) -> Option<SocketAddr> {
        self.joined.as_ref().map(|joined| joined.addr)
    }

    /// Lets go of the served scene without keeping the copy as this
    /// editor's own: what a new scene, an open or another join does, each
    /// putting a document of its own in place.
    pub(super) fn leave_join(&mut self) {
        if let Some(joined) = self.joined.take() {
            crcbl::log::info!("editor: left the scene served at {}", joined.addr);
            self.document.stop_routing();
        }
    }

    /// The status line's answer to `action` while joined, when it is one a
    /// joined editor refuses — see the module docs.
    pub(super) fn refused_while_joined(&self, action: &Action) -> Option<String> {
        let addr = self.joined_addr()?;
        let why = match action {
            Action::Save | Action::SaveAs => SAVED_BY_THE_SERVER,
            Action::PlayStop => NO_PLAY_JOINED,
            _ => return None,
        };
        Some(format!("Joined {addr}: {why}"))
    }

    /// Sends what the copy held this frame, steps the client and follows
    /// the server's notices — once a frame, after every action — then puts
    /// a copy fetched afresh in place, says what the server refused, and
    /// keeps the copy as this editor's own once the session is over. `held`
    /// is whether the pointer's button is down, which a drag's end is read
    /// from.
    pub(super) fn step_join(&mut self, held: bool) {
        let Some(joined) = self.joined.as_mut() else {
            return;
        };
        let mut refusals = joined.send(self.document.take_routed(), held);
        // Unrouted while the follower holds it, so a notice applies.
        self.document.stop_routing();
        let followed = joined.follow(&mut self.document, self.elapsed);
        self.document.route_edits();
        let addr = joined.addr;
        if followed.landed {
            self.put_copy_in_place(addr);
        }
        if let Some(spawned) = followed.select {
            self.document.set_selection(spawned);
        }
        refusals.extend(followed.refusals);
        if !refusals.is_empty() {
            for refusal in &refusals {
                crcbl::log::warn!("editor: {addr} refused an edit — {refusal}");
            }
            self.panels
                .set_status(super::refused_status(&refusals), Tone::Warning);
        }
        if let Some(why) = followed.ended {
            self.end_join(&why);
        }
    }

    /// Builds the view afresh around a copy the follower fetched, now the
    /// document, and says the scene is joined.
    fn put_copy_in_place(&mut self, addr: SocketAddr) {
        if let Some(root) = &self.assets {
            self.document
                .set_assets(Box::new(DirSource::at(root.clone())));
        }
        self.settle_document();
        crcbl::log::info!("editor: joined `{}` served at {addr}", self.document.name());
        self.panels.set_status(
            format!("Joined `{}` at {addr}: {JOINED}", self.document.name()),
            Tone::Info,
        );
    }

    /// Ends the session `why` says is over, keeping the copy — when one had
    /// landed — as an unsaved document of this editor's own.
    fn end_join(&mut self, why: &str) {
        let Some(joined) = self.joined.take() else {
            return;
        };
        self.document.stop_routing();
        let status = if joined.follower.document().is_some() {
            self.document.forget_saved();
            format!("{why}: {KEPT_COPY}")
        } else {
            why.to_owned()
        };
        crcbl::log::warn!("editor: {status}");
        self.panels.set_status(status, Tone::Warning);
    }
}
