//! Fetching the scene a server serves for editing: the request, the parts
//! joined as they arrive, and the scene — or why there is none — handed over
//! once, as edit replies are.
//!
//! The wire form and its checks are `crcbl_net::edit::fetch`'s; this half
//! numbers the fetch, keeps the one assembly in flight, and drops the parts
//! of any fetch it has stopped waiting for.

use std::fmt;

use crcbl_net::{
    EditRefusal, FetchedScene, Message, SceneAssembly, SceneAssemblyError, SceneOutcome,
    SceneReply, Transport,
};

use crate::{Client, EditNotSent, hold};

/// Why a scene fetch brought no scene.
#[derive(Debug)]
pub enum SceneFetchFailed {
    /// The server refused it: it serves no scene, the scene is playing or
    /// too large, or this client had a fetch in flight already.
    Refused {
        /// The code a client branches on.
        reason: EditRefusal,
        /// The sentence a person reads.
        message: String,
    },
    /// The parts came out of order, disagreed with each other, or joined into
    /// something that is not a scene's files.
    Malformed(SceneAssemblyError),
}

impl fmt::Display for SceneFetchFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { reason, message } => {
                write!(f, "the server refused the fetch ({reason}): {message}")
            }
            Self::Malformed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SceneFetchFailed {}

/// What became of one [`Client::fetch_scene`].
#[derive(Debug)]
pub struct SceneFetch {
    /// The fetch, as [`Client::fetch_scene`] numbered it.
    pub fetch_id: u64,
    /// The scene and its revision, or why there is none.
    pub outcome: Result<FetchedScene, SceneFetchFailed>,
}

impl<T: Transport> Client<T> {
    /// Ask the server for the whole scene it serves for editing, sealed and
    /// on the reliable channel, and return the id the answer will name.
    ///
    /// The scene arrives in parts, joined here, and is handed over whole —
    /// its files and the revision they are at — through
    /// [`Client::scene_fetches`]; the notices to apply to it are the ones
    /// after that revision. **One fetch is in flight at a time**: asking
    /// again stops waiting for the one before, whose parts are then dropped
    /// as they come, and a server refuses a fetch from a client it is still
    /// answering, as busy. A reconnect stops waiting too: the parts went
    /// with the link.
    ///
    /// # Errors
    ///
    /// [`EditNotSent`], naming why nothing was sent: no session, or the key
    /// or the transport refused it. No id is spent on a fetch that was not
    /// sent, and the fetch in flight before, if any, is still awaited.
    pub fn fetch_scene(&mut self) -> Result<u64, EditNotSent> {
        if !self.handshake_complete {
            return Err(EditNotSent::NotInSession);
        }
        let Some(crypto) = self.session_crypto.as_mut() else {
            return Err(EditNotSent::NotInSession);
        };
        let fetch_id = self.next_scene_fetch;
        let data = crcbl_net::encode_scene_fetch(fetch_id);
        let payload =
            crcbl_net::encode_client_to_server(&crcbl_net::ClientToServer::Command { data });
        let sealed = crypto.seal(&payload).map_err(EditNotSent::Seal)?;
        self.transport
            .send_reliable(Message::reliable(sealed))
            .map_err(EditNotSent::Transport)?;
        self.next_scene_fetch = self.next_scene_fetch.wrapping_add(1);
        self.scene_assembly = Some(SceneAssembly::new(fetch_id));
        Ok(fetch_id)
    }

    /// Take the fetches that finished since the last call, oldest first —
    /// each with the scene it brought or why it brought none. Held until
    /// taken, up to [`MAX_QUEUED_EVENTS`](crate::MAX_QUEUED_EVENTS), as edit
    /// replies are.
    pub fn scene_fetches(&mut self) -> impl Iterator<Item = SceneFetch> + '_ {
        self.scene_fetches.drain(..)
    }

    /// How many bytes of the scene the fetch in flight has brought so far, or
    /// [`None`] with no fetch in flight — what tells a fetch that is moving
    /// from one that stalled.
    #[must_use]
    pub fn scene_fetch_progress(&self) -> Option<usize> {
        self.scene_assembly.as_ref().map(SceneAssembly::received)
    }

    /// Take one opened scene reply: a part of the fetch in flight, or its
    /// refusal. A reply naming any other fetch is one this client stopped
    /// waiting for, and is dropped without a word. Returns whether a
    /// finished fetch was held, or `false` when the queue was full.
    pub(crate) fn take_scene_reply(&mut self, reply: SceneReply) -> bool {
        let Some(assembly) = self.scene_assembly.as_mut() else {
            return true;
        };
        if assembly.fetch_id() != reply.fetch_id {
            return true;
        }
        let outcome = match reply.outcome {
            SceneOutcome::Part(part) => match assembly.push(part) {
                Ok(None) => return true,
                Ok(Some(scene)) => Ok(scene),
                Err(error) => Err(SceneFetchFailed::Malformed(error)),
            },
            SceneOutcome::Refused { reason, message } => {
                Err(SceneFetchFailed::Refused { reason, message })
            }
        };
        self.scene_assembly = None;
        hold(
            &mut self.scene_fetches,
            SceneFetch {
                fetch_id: reply.fetch_id,
                outcome,
            },
        )
    }
}

#[cfg(test)]
mod tests;
