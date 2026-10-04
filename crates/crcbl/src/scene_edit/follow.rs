//! Following a served scene: a copy of the [`EditServer`](super::EditServer)'s
//! document that a client holds, fetched whole and then kept current from the
//! notices — what a client that joins late, or comes back after a lost link,
//! needs before the notices mean anything.
//!
//! # The decisions it carries (2026-10-04)
//!
//! * **A fetch, then the notices after it.** [`Client::fetch_scene`] brings
//!   the scene's saved text at a revision; the copy is a [`Document`] opened
//!   from that text, and each notice past the revision is applied to it in
//!   order — through [`Document::apply`], [`Document::undo`] and
//!   [`Document::redo`], the server's own path.
//! * **What arrives during a fetch waits for it**, held in order up to
//!   [`MAX_BUFFERED_NOTICES`]; once the scene lands, those at or before its
//!   revision are passed over and the rest applied. One held past the cap
//!   is dropped, and the copy refetches once the fetch lands rather than
//!   follow with a hole in it.
//! * **A copy that cannot follow refetches rather than diverge.** A notice
//!   whose revision is not one past the copy's — one was dropped on the way —
//!   and one the copy cannot apply both make it stale, and the next update
//!   fetches again. So does the client dropping anything it could not hold
//!   ([`Client::dropped_event_count`]): a notice may be among it, and the gap
//!   would otherwise show only when the next notice comes, which may be
//!   never.
//! * **An undo is the copy's own history, stepped.** The copy's history is
//!   the server's since the fetch: every entry the server made after it
//!   reached the copy as a notice, in order. So an undo or a redo the copy
//!   can take is the one the server took, and one it cannot — reaching back
//!   past the fetch, where the copy holds no history — is a notice it cannot
//!   apply, and a refetch.
//! * **A drag folds as the server's did** (decided 2026-10-05): a notice
//!   naming a gesture ([`EditNotice::gesture`]) is recorded in that gesture
//!   ([`Document::apply_in`]), so consecutive notices of one gesture are one
//!   entry of the copy's history exactly when they are one of the server's,
//!   and an undo of the drag walks all of it back on both. The server seals
//!   a gesture when it answers a fetch, so a copy never holds the end of a
//!   drag whose beginning only the server's history has.
//! * **A stalled fetch is abandoned**: one that brings no byte for
//!   [`FETCH_STALL_TIMEOUT`] — its parts went with a dropped link, or the
//!   server never answered — is fetched again; and a refused or malformed
//!   one waits [`FETCH_RETRY_DELAY`] before the next, so a server that says
//!   busy, or a scene that is playing, is not asked in a loop.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use crate::client::Client;
use crate::net::{EditNotice, FetchedScene, Transport};
use crate::registry::Registry;
use crate::scene::edit::{EditOp, Gesture, decode_op};

use super::{Document, memory_source};

/// The most notices a [`SceneFollower`] holds while it waits for a fetch;
/// past that the copy refetches once the fetch lands.
///
/// Several seconds of edits from several clients, which a fetch at the
/// server's pace (`crcbl_server::SCENE_FETCH_BYTES_PER_SECOND`) of any scene
/// the editor has opened finishes inside.
pub const MAX_BUFFERED_NOTICES: usize = 1024;

/// How long a fetch may bring nothing before it is abandoned and asked again.
///
/// Many times the gap between two parts at the server's pace, so only a
/// fetch that has stopped trips it.
pub const FETCH_STALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How long after a refused or malformed fetch the next is asked.
pub const FETCH_RETRY_DELAY: Duration = Duration::from_secs(1);

/// A fetch asked and not yet answered.
#[derive(Debug)]
struct InFlight {
    id: u64,
    /// The bytes it had brought when they last moved, and when.
    progress: usize,
    moved_at: Duration,
}

/// A client's copy of a served scene, fetched and then kept current. See the
/// [module docs](self).
#[derive(Debug)]
pub struct SceneFollower {
    /// The vocabulary the copy opens with: the server's, or the copy refuses
    /// the scene the way the server would refuse its files.
    registry: Registry,
    /// The copy, and the revision it stands at.
    copy: Option<(Document, u64)>,
    /// Whether the copy has missed a notice, or met one it cannot apply, and
    /// waits for a fetch.
    stale: bool,
    in_flight: Option<InFlight>,
    /// No fetch is asked before this.
    retry_at: Duration,
    /// Notices waiting for a fetch to land, in the order they came.
    held: VecDeque<EditNotice>,
    /// Whether a notice was dropped from [`held`](Self::held) since the
    /// last fetch landed.
    overflowed: bool,
    fetches: u64,
    /// The client's [`Client::dropped_event_count`] when last read.
    dropped_seen: u64,
    last_failure: Option<String>,
}

impl SceneFollower {
    /// A follower holding no copy yet, which opens the scene it fetches with
    /// `registry`.
    #[must_use]
    pub const fn new(registry: Registry) -> Self {
        Self {
            registry,
            copy: None,
            stale: false,
            in_flight: None,
            retry_at: Duration::ZERO,
            held: VecDeque::new(),
            overflowed: false,
            fetches: 0,
            dropped_seen: 0,
            last_failure: None,
        }
    }

    /// Takes what `client` has brought since the last call — fetched scenes
    /// and notices — and asks for the scene when the copy needs it: at the
    /// start, after a gap, and after a fetch that stalled or failed. `now`
    /// is the time `client` was last updated with.
    ///
    /// It takes every one of the client's notices
    /// ([`Client::edit_notices`]) and fetches ([`Client::scene_fetches`]); a
    /// caller that wants to hear of changes reads
    /// [`revision`](Self::revision).
    pub fn update<T: Transport>(&mut self, client: &mut Client<T>, now: Duration) {
        let dropped = client.dropped_event_count();
        let missed = dropped > self.dropped_seen;
        self.dropped_seen = dropped;
        let waiting = self.in_flight.is_some();
        let fetched: Vec<_> = client.scene_fetches().collect();
        for fetch in fetched {
            if self.in_flight.as_ref().map(|f| f.id) != Some(fetch.fetch_id) {
                continue;
            }
            self.in_flight = None;
            match fetch.outcome {
                Ok(scene) => self.adopt(scene, now),
                Err(error) => self.failed(error.to_string(), now),
            }
        }
        let notices: Vec<_> = client.edit_notices().collect();
        for notice in notices {
            self.take(notice);
        }
        // What the client dropped came with what it held this update, so it
        // may be a notice before or after a scene that landed with it.
        if missed && (waiting || self.copy.is_some()) {
            self.last_failure = Some(
                "the client dropped a message it could not hold; a notice may be among them"
                    .to_owned(),
            );
            if self.in_flight.is_some() {
                self.overflowed = true;
            } else {
                self.stale = true;
            }
        }
        self.watch_fetch(client.scene_fetch_progress(), now);
        if self.in_flight.is_none() && (self.copy.is_none() || self.stale) && now >= self.retry_at {
            match client.fetch_scene() {
                Ok(id) => {
                    self.fetches += 1;
                    self.in_flight = Some(InFlight {
                        id,
                        progress: 0,
                        moved_at: now,
                    });
                }
                // Not yet in session, or no longer: the fetch goes once it is.
                Err(crate::client::EditNotSent::NotInSession) => {}
                Err(error) => self.failed(error.to_string(), now),
            }
        }
    }

    /// Abandons the fetch in flight when the client stopped waiting for it
    /// — a reconnect — or it has brought nothing for [`FETCH_STALL_TIMEOUT`].
    fn watch_fetch(&mut self, progress: Option<usize>, now: Duration) {
        let Some(in_flight) = self.in_flight.as_mut() else {
            return;
        };
        match progress {
            None => self.in_flight = None,
            Some(bytes) if bytes != in_flight.progress => {
                in_flight.progress = bytes;
                in_flight.moved_at = now;
            }
            Some(_) if now.saturating_sub(in_flight.moved_at) >= FETCH_STALL_TIMEOUT => {
                self.last_failure = Some("the scene fetch stalled".to_owned());
                self.in_flight = None;
            }
            Some(_) => {}
        }
    }

    /// Opens a fetched scene as the copy, then applies the notices held for
    /// it.
    fn adopt(&mut self, scene: FetchedScene, now: Duration) {
        let opened = memory_source(scene.files)
            .and_then(|source| Document::open(&source, Path::new(""), self.registry.clone()));
        let document = match opened {
            Ok(document) => document,
            Err(error) => return self.failed(error.to_string(), now),
        };
        self.copy = Some((document, scene.revision));
        self.stale = false;
        self.last_failure = None;
        for notice in std::mem::take(&mut self.held) {
            self.take(notice);
        }
        // What was held still applies, so the copy is as current as it can
        // be while the fetch that fills the hole is on its way.
        if std::mem::take(&mut self.overflowed) {
            self.last_failure = Some("more notices came during the fetch than are held".to_owned());
            self.stale = true;
        }
    }

    /// Records why a fetch brought no copy, and holds the next one back.
    fn failed(&mut self, why: String, now: Duration) {
        self.last_failure = Some(why);
        self.retry_at = now + FETCH_RETRY_DELAY;
    }

    /// Applies `notice` to a copy that is following, or holds it for the
    /// fetch the copy waits on.
    fn take(&mut self, notice: EditNotice) {
        let following = !self.stale && self.in_flight.is_none();
        let Some((document, revision)) = self.copy.as_mut().filter(|_| following) else {
            if self.held.len() < MAX_BUFFERED_NOTICES {
                self.held.push_back(notice);
            } else {
                self.overflowed = true;
            }
            return;
        };
        if notice.revision <= *revision {
            return;
        }
        let applied = if notice.revision == *revision + 1 {
            apply(document, &notice.op, notice.gesture)
        } else {
            Err(format!(
                "missed the notices from revision {} to {}",
                *revision + 1,
                notice.revision - 1
            ))
        };
        match applied {
            Ok(()) => *revision = notice.revision,
            // It waits, with what follows it, for the fetch that replaces
            // the copy.
            Err(why) => {
                self.last_failure = Some(why);
                self.stale = true;
                self.held.push_back(notice);
            }
        }
    }

    /// The copy, once a fetch has brought one. It may be
    /// [stale](Self::is_stale) while a fetch to replace it is in flight.
    #[must_use]
    pub fn document(&self) -> Option<&Document> {
        self.copy.as_ref().map(|(document, _)| document)
    }

    /// The copy, for what reads it through `&mut` — [`Document::files`]
    /// among them. An edit made here is the copy's alone, and the next
    /// notice it contradicts makes the copy stale: edits go to the server.
    pub fn document_mut(&mut self) -> Option<&mut Document> {
        self.copy.as_mut().map(|(document, _)| document)
    }

    /// The server's revision the copy stands at, once a fetch has brought one.
    #[must_use]
    pub fn revision(&self) -> Option<u64> {
        self.copy.as_ref().map(|(_, revision)| *revision)
    }

    /// Whether the copy has missed a notice, or met one it cannot apply, and
    /// waits for a fetch to replace it.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// How many fetches this follower has asked for: one to start, and one
    /// more for each gap, stall or failure.
    #[must_use]
    pub const fn fetch_count(&self) -> u64 {
        self.fetches
    }

    /// Why the copy last went stale or a fetch brought none, until a fetch
    /// brings one.
    #[must_use]
    pub fn last_failure(&self) -> Option<&str> {
        self.last_failure.as_deref()
    }
}

/// Applies one notice's operation to `document`, in the server's `gesture`
/// when it names one, or says why it did not take: bytes that are no
/// operation, a command the copy refuses, a step of the history with nothing
/// to step over — an undo reaching back past the fetch — or a step named part
/// of a gesture, which no server sends.
fn apply(document: &mut Document, op: &[u8], gesture: Option<u64>) -> Result<(), String> {
    let decoded = decode_op(op).map_err(|error| error.to_string())?;
    let stepped = match (decoded, gesture) {
        (EditOp::Apply(command), None) => {
            return document.apply(command).map_err(|e| e.to_string());
        }
        (EditOp::Apply(command), Some(gesture)) => {
            return document
                .apply_in(command, Gesture(gesture))
                .map_err(|e| e.to_string());
        }
        (EditOp::Undo | EditOp::Redo, Some(_)) => {
            return Err("a notice put a step of the history in a gesture".to_owned());
        }
        (EditOp::Undo, None) => document.undo(),
        (EditOp::Redo, None) => document.redo(),
    };
    match stepped {
        Ok(true) => Ok(()),
        Ok(false) => Err("the copy's history does not reach the step a notice took".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
