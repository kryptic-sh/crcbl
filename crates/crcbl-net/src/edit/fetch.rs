//! Fetching the whole scene: what a client that joins a server serving a
//! scene late, or resumes after a lost link, asks for before it can follow
//! the notices — a notice carries a change, not the state it changes.
//!
//! **The scene travels as its saved text**: the manifest and each system's
//! file, keyed relative to the scene directory, exactly the bytes a save
//! writes. That text is byte-identical for equal scenes, so a client holding
//! it holds the server's scene, and needs no wire form of a world to get it.
//!
//! **It travels in numbered parts.** A scene is larger than one message can
//! be — the transport seam's ceiling is
//! [`MAX_IN_MEMORY_MESSAGE_BYTES`](crate::MAX_IN_MEMORY_MESSAGE_BYTES), and
//! the reliable channel's fragmentation sits beneath that seam, not above it
//! — so the files are written once into one buffer ([`encode_scene_files`])
//! and cut into parts of [`MAX_SCENE_PART_BYTES`] ([`scene_part`]), each a
//! [`SceneReply`] of its own carrying the revision and the whole length.
//! Parts go on the reliable channel, which delivers them once and in order,
//! so a [`SceneAssembly`] takes them strictly in order and refuses any other.
//! Every part but the last is exactly [`MAX_SCENE_PART_BYTES`], which is what
//! lets a decoder check a part's length against the whole before it reads it.
//!
//! ```text
//! scene fetch (a Command's data):
//!   [0]       kind = SCENE_FETCH_KIND
//!   [1..9]    fetch_id: u64 LE, the client's own numbering
//! scene reply (a whole message):
//!   [0]       tag = SCENE_REPLY_TAG
//!   [1..9]    fetch_id: u64 LE, echoed
//!   [9]       outcome: 0 = part, 1 = refused
//!   part:     revision: u64 LE, the server's revision the scene is at
//!             total_len: u32 LE, the whole scene's length, 1..=MAX_SCENE_BYTES
//!             index: u32 LE, below the part count total_len makes
//!             part_len: u32 LE, the length index makes, then the bytes
//!   refused:  reason: u8; message_len: u16 LE, then message: UTF-8
//! scene files (the parts' bytes, joined):
//!   file_count: u32 LE, at most MAX_SCENE_FILES
//!   per file, keys strictly ascending:
//!             path_len: u16 LE, at most MAX_SCENE_PATH_BYTES, then path: UTF-8
//!             text_len: u32 LE, then text: UTF-8
//! ```

use std::collections::BTreeMap;
use std::fmt;

use super::{EditRefusal, EditTooLong, invalid_length, read_refusal, write_refusal};
use crate::auth::AUTH_OVERHEAD;
use crate::codec::{ByteReader, DecodeError, SCENE_REPLY_TAG};

/// The kind byte of a scene fetch, the command kind after
/// [`EDIT_KIND`](super::EDIT_KIND).
pub const SCENE_FETCH_KIND: u8 = 0x03;

/// The longest scene a fetch carries, as [`encode_scene_files`] writes it.
///
/// A scene is RON text, a chunk per system; this is far past any the editor
/// has opened, and small enough that a client's reassembly of one is a
/// bounded allocation.
pub const MAX_SCENE_BYTES: usize = 4 * 1024 * 1024;

/// The bytes of a scene one [`SceneReply`] carries: the length of every part
/// but the last.
///
/// A quarter of the transport seam's ceiling rather than all of it, so a
/// server pacing a fetch can send it a part at a time behind the notices and
/// replies sharing the channel, rather than a burst that holds them back.
pub const MAX_SCENE_PART_BYTES: usize = 16 * 1024;

/// The most parts one scene is cut into.
pub const MAX_SCENE_PARTS: usize = MAX_SCENE_BYTES.div_ceil(MAX_SCENE_PART_BYTES);

/// The most files a fetched scene holds: the manifest and a chunk per system.
pub const MAX_SCENE_FILES: usize = 1024;

/// The longest key one file of a fetched scene has, in bytes.
pub const MAX_SCENE_PATH_BYTES: usize = 256;

/// The tag, the fetch id, the outcome, the revision, the whole length, the
/// index and the part's length.
const PART_HEADER_BYTES: usize = 1 + 8 + 1 + 8 + 4 + 4 + 4;

const OUTCOME_PART: u8 = 0;
const OUTCOME_REFUSED: u8 = 1;

// A part, sealed, is one message on any transport.
const _: () = assert!(
    PART_HEADER_BYTES + MAX_SCENE_PART_BYTES + AUTH_OVERHEAD <= crate::MAX_IN_MEMORY_MESSAGE_BYTES
);
// The whole length and the index travel as `u32`.
const _: () = assert!(MAX_SCENE_BYTES <= u32::MAX as usize);

/// One part of a scene, as [`scene_part`] cuts it or a decoder reads it.
///
/// **Its fields are read, not set**: a part whose length disagrees with its
/// index and the whole cannot be built, so [`encode_scene_reply`] never
/// writes one a decoder would refuse.
#[derive(Clone, PartialEq, Eq)]
pub struct ScenePart {
    revision: u64,
    total_len: u32,
    index: u32,
    bytes: Vec<u8>,
}

impl ScenePart {
    /// The server's revision the scene is at.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The whole scene's length, in bytes.
    #[must_use]
    pub const fn total_len(&self) -> u32 {
        self.total_len
    }

    /// Which part this is, from zero.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// This part's bytes of the scene.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Prints the part's length rather than its bytes.
impl fmt::Debug for ScenePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScenePart")
            .field("revision", &self.revision)
            .field("total_len", &self.total_len)
            .field("index", &self.index)
            .field("part_bytes", &self.bytes.len())
            .finish()
    }
}

/// Part `index` of `scene`, the files [`encode_scene_files`] wrote at
/// `revision`, or [`None`] past the last part — and for a scene that is empty
/// or past [`MAX_SCENE_BYTES`], which has no parts.
#[must_use]
pub fn scene_part(revision: u64, scene: &[u8], index: u32) -> Option<ScenePart> {
    let total_len = u32::try_from(scene.len()).ok()?;
    let len = part_len(total_len, index)?;
    let start = usize::try_from(index).ok()? * MAX_SCENE_PART_BYTES;
    Some(ScenePart {
        revision,
        total_len,
        index,
        bytes: scene.get(start..start + len)?.to_vec(),
    })
}

/// The length part `index` of a scene `total_len` long has, or [`None`] for a
/// whole length outside `1..=MAX_SCENE_BYTES` or an index past the last part.
fn part_len(total_len: u32, index: u32) -> Option<usize> {
    let total = usize::try_from(total_len).ok()?;
    if total == 0 || total > MAX_SCENE_BYTES {
        return None;
    }
    let start = usize::try_from(index)
        .ok()?
        .checked_mul(MAX_SCENE_PART_BYTES)?;
    (start < total).then(|| (total - start).min(MAX_SCENE_PART_BYTES))
}

/// What became of one scene fetch, one message at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneOutcome {
    /// One part of the scene.
    Part(ScenePart),
    /// Not sent, and why: the scene is playing, too large, or this client
    /// already has a fetch in flight.
    Refused {
        /// The code a client branches on.
        reason: EditRefusal,
        /// The sentence a person reads.
        message: String,
    },
}

/// A server's answer to one scene fetch: one of its parts, or its refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneReply {
    /// The fetch it answers, as the client numbered it.
    pub fetch_id: u64,
    /// A part, or why there are none.
    pub outcome: SceneOutcome,
}

/// The `data` of a [`ClientToServer::Command`](crate::ClientToServer::Command)
/// asking for the scene, numbered `fetch_id`.
#[must_use]
pub fn encode_scene_fetch(fetch_id: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8);
    buf.push(SCENE_FETCH_KIND);
    buf.extend_from_slice(&fetch_id.to_le_bytes());
    buf
}

/// The fetch id in a scene fetch's `data`.
///
/// # Errors
///
/// [`DecodeError`] for another kind byte, a short buffer or trailing bytes.
pub fn decode_scene_fetch(data: &[u8]) -> Result<u64, DecodeError> {
    let mut r = ByteReader::new(data);
    let kind = r.read_u8()?;
    if kind != SCENE_FETCH_KIND {
        return Err(DecodeError::UnknownTag { tag: kind });
    }
    let fetch_id = r.read_u64()?;
    r.assert_empty()?;
    Ok(fetch_id)
}

/// The whole message carrying `reply`, ready to seal.
///
/// # Errors
///
/// [`EditTooLong`] for a refusal message past
/// [`MAX_EDIT_MESSAGE_BYTES`](super::MAX_EDIT_MESSAGE_BYTES).
pub fn encode_scene_reply(reply: &SceneReply) -> Result<Vec<u8>, EditTooLong> {
    let mut buf = Vec::with_capacity(PART_HEADER_BYTES);
    buf.push(SCENE_REPLY_TAG);
    buf.extend_from_slice(&reply.fetch_id.to_le_bytes());
    match &reply.outcome {
        SceneOutcome::Part(part) => {
            // A part's length is its index's and the whole's — `scene_part`
            // and the decoder are the only places one is built — so it fits.
            let len = u32::try_from(part.bytes.len())
                .expect("a part is at most MAX_SCENE_PART_BYTES long");
            buf.reserve(part.bytes.len());
            buf.push(OUTCOME_PART);
            buf.extend_from_slice(&part.revision.to_le_bytes());
            buf.extend_from_slice(&part.total_len.to_le_bytes());
            buf.extend_from_slice(&part.index.to_le_bytes());
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&part.bytes);
        }
        SceneOutcome::Refused { reason, message } => {
            buf.push(OUTCOME_REFUSED);
            write_refusal(&mut buf, *reason, message)?;
        }
    }
    Ok(buf)
}

/// The [`SceneReply`] in a whole message.
///
/// # Errors
///
/// [`DecodeError`] for another tag or outcome byte; for a part, a whole
/// length outside `1..=MAX_SCENE_BYTES`, an index past the last part, or a
/// part length other than the one its index makes — each refused before the
/// part's bytes are read; for a refusal, a message past
/// [`MAX_EDIT_MESSAGE_BYTES`](super::MAX_EDIT_MESSAGE_BYTES) or not UTF-8;
/// and a short buffer or trailing bytes.
pub fn decode_scene_reply(payload: &[u8]) -> Result<SceneReply, DecodeError> {
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != SCENE_REPLY_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let fetch_id = r.read_u64()?;
    let outcome = match r.read_u8()? {
        OUTCOME_PART => {
            let revision = r.read_u64()?;
            let total_len = r.read_u32()?;
            let index = r.read_u32()?;
            let claimed = r.read_u32()?;
            let len = part_len(total_len, index).ok_or(DecodeError::InvalidLength(total_len))?;
            if usize::try_from(claimed).ok() != Some(len) {
                return Err(DecodeError::InvalidLength(claimed));
            }
            SceneOutcome::Part(ScenePart {
                revision,
                total_len,
                index,
                bytes: r.read_bytes(len)?.to_vec(),
            })
        }
        OUTCOME_REFUSED => {
            let (reason, message) = read_refusal(&mut r)?;
            SceneOutcome::Refused { reason, message }
        }
        other => return Err(DecodeError::UnknownTag { tag: other }),
    };
    r.assert_empty()?;
    Ok(SceneReply { fetch_id, outcome })
}

/// A scene's files past what a fetch carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTooLarge {
    /// What is past its limit: `"scene"`, `"file count"` or `"path"`.
    pub what: &'static str,
    /// Its length or count.
    pub len: usize,
    /// The most a fetch carries.
    pub limit: usize,
}

impl fmt::Display for SceneTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} is {}, past the {} a scene fetch carries",
            self.what, self.len, self.limit
        )
    }
}

impl std::error::Error for SceneTooLarge {}

/// `files`, a scene's saved text keyed relative to its directory, as the one
/// buffer [`scene_part`] cuts.
///
/// # Errors
///
/// [`SceneTooLarge`] for more than [`MAX_SCENE_FILES`] files, a key past
/// [`MAX_SCENE_PATH_BYTES`], or a whole past [`MAX_SCENE_BYTES`].
pub fn encode_scene_files(files: &BTreeMap<String, String>) -> Result<Vec<u8>, SceneTooLarge> {
    let too_many = SceneTooLarge {
        what: "file count",
        len: files.len(),
        limit: MAX_SCENE_FILES,
    };
    if files.len() > MAX_SCENE_FILES {
        return Err(too_many);
    }
    let count = u32::try_from(files.len()).map_err(|_| too_many)?;
    let mut buf = Vec::new();
    buf.extend_from_slice(&count.to_le_bytes());
    for (path, text) in files {
        let long_path = SceneTooLarge {
            what: "path",
            len: path.len(),
            limit: MAX_SCENE_PATH_BYTES,
        };
        if path.len() > MAX_SCENE_PATH_BYTES {
            return Err(long_path);
        }
        let path_len = u16::try_from(path.len()).map_err(|_| long_path)?;
        let too_large = SceneTooLarge {
            what: "scene",
            len: buf.len() + 2 + path.len() + 4 + text.len(),
            limit: MAX_SCENE_BYTES,
        };
        if too_large.len > MAX_SCENE_BYTES {
            return Err(too_large);
        }
        let text_len = u32::try_from(text.len()).map_err(|_| too_large)?;
        buf.extend_from_slice(&path_len.to_le_bytes());
        buf.extend_from_slice(path.as_bytes());
        buf.extend_from_slice(&text_len.to_le_bytes());
        buf.extend_from_slice(text.as_bytes());
    }
    Ok(buf)
}

/// Why the joined parts of a scene are not a scene's files.
#[derive(Debug, thiserror::Error)]
pub enum SceneFilesError {
    /// Cut short, a count or length past its limit or past the bytes there
    /// are, text that is not UTF-8, or trailing bytes.
    #[error(transparent)]
    Wire(#[from] DecodeError),
    /// A key not after the one before it: repeated, or out of the order
    /// [`encode_scene_files`] writes them in.
    #[error("the scene's file {path:?} is out of order or repeated")]
    Unordered {
        /// The key that came out of order.
        path: String,
    },
}

/// The files in `bytes`, as [`encode_scene_files`] wrote them.
///
/// # Errors
///
/// [`SceneFilesError`] for anything [`encode_scene_files`] would not have
/// written: every count and length is checked against its limit, and against
/// the bytes there are, before anything is read for it.
pub fn decode_scene_files(bytes: &[u8]) -> Result<BTreeMap<String, String>, SceneFilesError> {
    if bytes.len() > MAX_SCENE_BYTES {
        return Err(invalid_length(bytes.len()).into());
    }
    let mut r = ByteReader::new(bytes);
    let count = r.read_u32()?;
    let count = usize::try_from(count).map_err(|_| DecodeError::InvalidLength(count))?;
    if count > MAX_SCENE_FILES {
        return Err(invalid_length(count).into());
    }
    let mut files = BTreeMap::new();
    for _ in 0..count {
        let path_len = usize::from(r.read_u16()?);
        if path_len > MAX_SCENE_PATH_BYTES {
            return Err(invalid_length(path_len).into());
        }
        let path = utf8(r.read_bytes(path_len)?)?;
        let text_len = r.read_u32()?;
        let text_len =
            usize::try_from(text_len).map_err(|_| DecodeError::InvalidLength(text_len))?;
        let text = utf8(r.read_bytes(text_len)?)?;
        if files
            .last_key_value()
            .is_some_and(|(last, _)| *last >= path)
        {
            return Err(SceneFilesError::Unordered { path });
        }
        files.insert(path, text);
    }
    r.assert_empty()?;
    Ok(files)
}

fn utf8(bytes: &[u8]) -> Result<String, DecodeError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid_length(bytes.len()))
}

/// A scene a fetch brought whole: its files, and the revision they are at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedScene {
    /// The server's revision the files are at: the notices to apply to them
    /// are the ones after it.
    pub revision: u64,
    /// The scene's saved text, keyed relative to its directory.
    pub files: BTreeMap<String, String>,
}

/// Why a [`SceneAssembly`] refused a part, or the scene its parts made.
#[derive(Debug, thiserror::Error)]
pub enum SceneAssemblyError {
    /// A part other than the next: the reliable channel delivers in order, so
    /// one missing or repeated means the fetch lost a part.
    #[error("scene part {got} arrived where part {expected} was due")]
    OutOfOrder {
        /// The index due next.
        expected: u32,
        /// The index that came.
        got: u32,
    },
    /// A part naming another revision or another whole length than the first
    /// part did.
    #[error("a scene part disagrees with the first on the revision or the length")]
    Changed,
    /// The parts joined are not a scene's files.
    #[error("the fetched scene is malformed: {0}")]
    Files(#[from] SceneFilesError),
}

/// One fetch's parts, joined as they arrive.
#[derive(Debug, Clone)]
pub struct SceneAssembly {
    fetch_id: u64,
    /// The revision and the whole length the first part named.
    shape: Option<(u64, u32)>,
    bytes: Vec<u8>,
    next: u32,
}

impl SceneAssembly {
    /// An assembly of fetch `fetch_id`, holding no part yet.
    #[must_use]
    pub const fn new(fetch_id: u64) -> Self {
        Self {
            fetch_id,
            shape: None,
            bytes: Vec::new(),
            next: 0,
        }
    }

    /// The fetch whose parts this joins.
    #[must_use]
    pub const fn fetch_id(&self) -> u64 {
        self.fetch_id
    }

    /// How many of the scene's bytes have arrived.
    #[must_use]
    pub fn received(&self) -> usize {
        self.bytes.len()
    }

    /// Takes the next part, returning the scene once its last part is in.
    ///
    /// The allocation grows with the parts that arrive, never with what the
    /// first claims, and is bounded by [`MAX_SCENE_BYTES`] through the
    /// decoder's check on every part's whole length.
    ///
    /// # Errors
    ///
    /// [`SceneAssemblyError`] for a part out of order or disagreeing with the
    /// first, and for parts that join into something that is not a scene's
    /// files. The assembly is spent either way: a caller fetches again.
    pub fn push(&mut self, part: ScenePart) -> Result<Option<FetchedScene>, SceneAssemblyError> {
        if part.index != self.next {
            return Err(SceneAssemblyError::OutOfOrder {
                expected: self.next,
                got: part.index,
            });
        }
        let shape = *self.shape.get_or_insert((part.revision, part.total_len));
        if shape != (part.revision, part.total_len) {
            return Err(SceneAssemblyError::Changed);
        }
        self.bytes.extend_from_slice(&part.bytes);
        self.next = self.next.saturating_add(1);
        if u32::try_from(self.bytes.len()).ok() != Some(part.total_len) {
            return Ok(None);
        }
        let files = decode_scene_files(&self.bytes)?;
        Ok(Some(FetchedScene {
            revision: part.revision,
            files,
        }))
    }
}

#[cfg(test)]
mod tests;
