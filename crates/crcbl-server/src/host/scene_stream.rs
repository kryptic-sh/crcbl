//! A scene fetch being answered: the scene's files, written once when the
//! fetch was answered, sent to one peer a part at a time.
//!
//! **Paced, not sent in one burst.** The parts share the reliable channel with
//! the notices and replies of everyone's edits, and a client charges every
//! message it reads against its inbound budget and drops what is past it —
//! a dropped part is a fetch that cannot finish. So a stream sends its parts
//! at [`SCENE_FETCH_BYTES_PER_SECOND`], well inside a client's default budget
//! on that channel, and a part the transport has no room for waits for the
//! next update rather than failing the fetch.
//!
//! **One stream a peer.** A fetch from a peer already being sent one, or with
//! one waiting to be answered, is refused as busy by the host, before the
//! caller is asked to write the scene again: writing it is the expensive
//! half, and a client asking in a loop would otherwise choose how often the
//! server does it.

use std::time::Duration;

use crcbl_net::ScenePart;

/// The rate a stream sends a scene's bytes at.
///
/// Half of [`InboundRateLimitConfig`](crcbl_net::rate_limit::InboundRateLimitConfig)'s
/// default byte budget, which a client applies to the reliable channel
/// alone: the other half is the notices, replies and events sent beside it.
/// `crcbl_server`'s tests hold it under that default.
pub const SCENE_FETCH_BYTES_PER_SECOND: u64 = 64 * 1024;

/// One peer's fetch, part way sent.
#[derive(Debug)]
pub(super) struct SceneStream {
    fetch_id: u64,
    revision: u64,
    scene: Vec<u8>,
    next_index: u32,
    /// When the next part may go: a part's bytes at
    /// [`SCENE_FETCH_BYTES_PER_SECOND`] after the one before it.
    next_at: Duration,
}

impl SceneStream {
    /// A stream of `scene` at `revision` answering fetch `fetch_id`, its
    /// first part due at `now`.
    pub(super) const fn new(fetch_id: u64, revision: u64, scene: Vec<u8>, now: Duration) -> Self {
        Self {
            fetch_id,
            revision,
            scene,
            next_index: 0,
            next_at: now,
        }
    }

    /// The fetch this answers.
    pub(super) const fn fetch_id(&self) -> u64 {
        self.fetch_id
    }

    /// The next part, if it is due at `now` and there is one left.
    pub(super) fn due(&self, now: Duration) -> Option<ScenePart> {
        if now < self.next_at {
            return None;
        }
        crcbl_net::scene_part(self.revision, &self.scene, self.next_index)
    }

    /// Records that the part [`due`](Self::due) returned at `now` went, `len`
    /// bytes of it.
    ///
    /// The next is due that many bytes' time after this one or after `now`,
    /// whichever is later — so a host updated late sends one part, not the
    /// burst its absence saved up.
    pub(super) fn sent(&mut self, len: usize, now: Duration) {
        self.next_index = self.next_index.saturating_add(1);
        self.next_at = self.next_at.max(now) + time_to_send(len);
    }

    /// Whether every part has gone.
    pub(super) fn is_done(&self) -> bool {
        crcbl_net::scene_part(self.revision, &self.scene, self.next_index).is_none()
    }
}

/// How long `len` bytes take at [`SCENE_FETCH_BYTES_PER_SECOND`].
fn time_to_send(len: usize) -> Duration {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    let len = u128::try_from(len).unwrap_or(u128::MAX);
    let nanos = len.saturating_mul(NANOS_PER_SECOND) / u128::from(SCENE_FETCH_BYTES_PER_SECOND);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}
