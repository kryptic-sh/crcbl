//! The edit history kept beside a scene on disk, so that an undo in one
//! process walks back an edit another made: `crcbl scene move` and then
//! `crcbl scene undo`, two runs of the CLI with nothing held between them.
//!
//! # Decided 2026-10-04, for the long term
//!
//! * **A sidecar in the scene's own directory**, [`HISTORY`], beside the
//!   recovery copy's `origin.txt` in spirit: the scene loader never reads it,
//!   because it reads only what the manifest names, and a save never removes
//!   it, because a save removes only files the document owns, which are the
//!   manifest's. It travels with the directory it describes, and a hidden name
//!   keeps it out of a listing of the scene's own files.
//! * **Bound to the scene's bytes.** It records a SHA-256 of the files the
//!   scene was saved as when it was written, and is refused on read unless
//!   the scene on disk is still those files. Every inverse in it was read off
//!   that scene; a scene changed since by anything else — a person, the GUI
//!   editor, a checkout — is not one they were made for, and replaying one
//!   blindly would write stale values back.
//! * **Bounded**: at most [`MAX_HISTORY_ENTRIES`] entries, the oldest applied
//!   ones dropped first when it is written, and at most [`MAX_HISTORY_BYTES`];
//!   a file past either is refused when it is read, before it is decoded.
//! * **Checked whole**: a SHA-256 of every byte before it ends the file, as
//!   `crcbl_store::save`'s container ends with one, so a file cut short or
//!   changed is refused rather than half read.
//! * **Its operations are the wire's** ([`encode_op`] and [`decode_op`]), so
//!   the history decodes through the decoder the edit server and its fuzz
//!   target already hold to account, and a variant switch — which the wire
//!   does not carry yet — is refused by name when it is written.
//!
//! # Layout
//!
//! ```text
//! [0..8)    magic:     b"CRCBLHIS"
//! [8..10)   version:   u16 little-endian, HISTORY_VERSION
//! [10..42)  scene:     SHA-256 of the scene's files (see `scene_digest`)
//! [42..46)  position:  u32 little-endian, how many entries are applied
//! [46..50)  count:     u32 little-endian, how many entries follow
//! entries:  done_len u32 LE, done (an encoded EditOp::Apply),
//!           undo_len u32 LE, undo (the same)
//! [..32)    checksum:  SHA-256 of every byte before it
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::registry::Registry;
use crate::scene::edit::{
    EditCommand, EditOp, OpDecodeError, OpEncodeError, UndoLog, decode_op, encode_op,
};
use crate::shaders::sha256::sha256;
use crate::store::{NativeStorage, StorageError, StorageSource};

use super::{Document, EditError};

/// The history's file name in the scene directory.
pub const HISTORY: &str = ".crcbl-history";

/// The most entries a history keeps: a written history drops its oldest
/// applied entries past this, and a read one holding more is refused.
pub const MAX_HISTORY_ENTRIES: usize = 64;

/// The largest history file read, in bytes; a larger one is refused before
/// it is decoded, and a history is trimmed under it when it is written.
pub const MAX_HISTORY_BYTES: usize = 4 * 1024 * 1024;

/// The bytes a history file starts with.
const MAGIC: &[u8; 8] = b"CRCBLHIS";

/// The layout version this build writes and reads.
const HISTORY_VERSION: u16 = 0;

/// The size of a SHA-256 digest.
const DIGEST_BYTES: usize = 32;

/// The magic, the version, the scene's digest, the position and the count.
const HEADER_BYTES: usize = MAGIC.len() + 2 + DIGEST_BYTES + 4 + 4;

/// Why a history beside a scene was refused or would not be written.
#[derive(Debug)]
pub enum HistoryError {
    /// The file would not be read.
    Read(std::io::Error),
    /// The file is larger than [`MAX_HISTORY_BYTES`].
    TooLarge,
    /// The file does not have the layout's shape: no magic, cut short, a
    /// length past its end, or bytes after its checksum.
    Malformed(&'static str),
    /// The file is in a layout version this build does not read.
    Version(u16),
    /// The checksum does not match the bytes before it.
    Checksum,
    /// The scene on disk is not the scene the history was written beside.
    SceneChanged,
    /// The history holds more than [`MAX_HISTORY_ENTRIES`] entries.
    TooMany(usize),
    /// The position is past the entries.
    Position {
        /// How many entries it says are applied.
        position: usize,
        /// How many it holds.
        count: usize,
    },
    /// An entry's operation would not decode.
    Entry {
        /// Which entry, from the oldest.
        index: usize,
        /// What the decoder said.
        error: OpDecodeError,
    },
    /// An entry's operation decoded to an undo or a redo, which a history
    /// does not hold.
    NotACommand(usize),
    /// An entry's command would not encode.
    Encode(OpEncodeError),
    /// The file would not be written.
    Write(StorageError),
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "it would not be read: {error}"),
            Self::TooLarge => write!(f, "it is larger than {MAX_HISTORY_BYTES} bytes"),
            Self::Malformed(why) => write!(f, "it is not a history: {why}"),
            Self::Version(version) => write!(
                f,
                "it is in layout version {version}, and this build reads {HISTORY_VERSION}"
            ),
            Self::Checksum => f.write_str("its checksum does not match its bytes"),
            Self::SceneChanged => f.write_str(
                "the scene has changed since it was written, so its undo would write stale values",
            ),
            Self::TooMany(count) => write!(
                f,
                "it holds {count} entries, more than the {MAX_HISTORY_ENTRIES} a history keeps"
            ),
            Self::Position { position, count } => {
                write!(f, "it stands at entry {position} of {count}, past its end")
            }
            Self::Entry { index, error } => write!(f, "entry {index} will not decode: {error}"),
            Self::NotACommand(index) => {
                write!(f, "entry {index} is an undo or a redo, not an edit")
            }
            Self::Encode(error) => write!(f, "an entry will not encode: {error}"),
            Self::Write(error) => write!(f, "it would not be written: {error}"),
        }
    }
}

impl std::error::Error for HistoryError {}

impl Document {
    /// [`open_dir`](Self::open_dir), with the history [`HISTORY`] beside the
    /// scene read back into the document's log — or a fresh log where there
    /// is none. Either way the document opens clean.
    ///
    /// # Errors
    ///
    /// As [`open_dir`](Self::open_dir), and [`EditError::History`] for a
    /// history that will not be read, is not one, or was written beside a
    /// scene other than the one on disk — see the module docs. Nothing is
    /// replayed from a refused history.
    pub fn open_with_history(
        path: impl Into<PathBuf>,
        registry: Registry,
    ) -> Result<Self, EditError> {
        let path = path.into();
        let mut document = Self::open_dir(path.clone(), registry)?;
        let Some(bytes) = read_bounded(&path.join(HISTORY)).map_err(EditError::History)? else {
            return Ok(document);
        };
        let history = decode(&bytes).map_err(EditError::History)?;
        if history.scene != scene_digest(&document.files()?) {
            return Err(EditError::History(HistoryError::SceneChanged));
        }
        let count = history.entries.len();
        let mut log = UndoLog::restored(history.entries, history.position).ok_or(
            EditError::History(HistoryError::Position {
                position: history.position,
                count,
            }),
        )?;
        log.mark_saved();
        document.log = log;
        Ok(document)
    }

    /// [`save`](Self::save), then the document's history written beside the
    /// scene as [`HISTORY`], bound to the files just saved — trimmed to
    /// [`MAX_HISTORY_ENTRIES`] and [`MAX_HISTORY_BYTES`], the oldest applied
    /// entries first.
    ///
    /// # Errors
    ///
    /// As [`save`](Self::save), and [`EditError::History`] for a history that
    /// will not encode or be written — after the scene itself was saved, so
    /// the history on disk is then the one before, which the next
    /// [`open_with_history`](Self::open_with_history) refuses as written
    /// beside another scene rather than replays.
    pub fn save_with_history(&mut self) -> Result<(), EditError> {
        self.save()?;
        let dir = self.origin.clone().ok_or(EditError::NoOrigin)?;
        let scene = scene_digest(&self.files()?);
        let bytes = encode(&scene, &self.log).map_err(EditError::History)?;
        NativeStorage::at(dir)
            .write(Path::new(HISTORY), &bytes)
            .map_err(|error| EditError::History(HistoryError::Write(error)))
    }
}

/// A history read back: the scene it was written beside, its entries and
/// where it stands.
struct Recorded {
    scene: [u8; DIGEST_BYTES],
    position: usize,
    entries: Vec<(EditCommand, EditCommand)>,
}

/// The SHA-256 the history binds itself to: each file's key and text, in key
/// order, each as its length in eight little-endian bytes and then its bytes —
/// so no two different sets of files run together into the same bytes.
fn scene_digest(files: &BTreeMap<String, String>) -> [u8; DIGEST_BYTES] {
    let mut bytes = Vec::new();
    for (key, text) in files {
        for part in [key.as_bytes(), text.as_bytes()] {
            bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
            bytes.extend_from_slice(part);
        }
    }
    sha256(&bytes)
}

/// The file at `path`, or [`None`] where there is none — read no further than
/// one byte past [`MAX_HISTORY_BYTES`], so a file of any size costs at most
/// that much to refuse.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, HistoryError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(HistoryError::Read(error)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_HISTORY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(HistoryError::Read)?;
    if bytes.len() > MAX_HISTORY_BYTES {
        return Err(HistoryError::TooLarge);
    }
    Ok(Some(bytes))
}

/// `log`'s entries as the history's bytes, bound to `scene`, trimmed to the
/// bounds: the oldest applied entry goes first, and only a history with
/// nothing applied loses its newest undone one instead.
fn encode(scene: &[u8; DIGEST_BYTES], log: &UndoLog) -> Result<Vec<u8>, HistoryError> {
    let mut entries = Vec::with_capacity(log.len());
    for (done, undo) in log.entries() {
        let done = encode_op(&EditOp::Apply(done.clone())).map_err(HistoryError::Encode)?;
        let undo = encode_op(&EditOp::Apply(undo.clone())).map_err(HistoryError::Encode)?;
        entries.push((done, undo));
    }
    let mut position = log.position();
    let size = |entries: &[(Vec<u8>, Vec<u8>)]| {
        HEADER_BYTES
            + DIGEST_BYTES
            + entries
                .iter()
                .map(|(done, undo)| 8 + done.len() + undo.len())
                .sum::<usize>()
    };
    while entries.len() > MAX_HISTORY_ENTRIES || size(&entries) > MAX_HISTORY_BYTES {
        if position > 0 {
            entries.remove(0);
            position -= 1;
        } else {
            entries.pop();
        }
    }

    let mut bytes = Vec::with_capacity(size(&entries));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&HISTORY_VERSION.to_le_bytes());
    bytes.extend_from_slice(scene);
    bytes.extend_from_slice(&length(position)?.to_le_bytes());
    bytes.extend_from_slice(&length(entries.len())?.to_le_bytes());
    for (done, undo) in &entries {
        for op in [done, undo] {
            bytes.extend_from_slice(&length(op.len())?.to_le_bytes());
            bytes.extend_from_slice(op);
        }
    }
    let checksum = sha256(&bytes);
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

/// `value` as the layout's `u32`. Every value written is held under
/// [`MAX_HISTORY_BYTES`] first, so a refusal here is a bound that moved past
/// what the layout can spell.
fn length(value: usize) -> Result<u32, HistoryError> {
    u32::try_from(value).map_err(|_| HistoryError::Malformed("a length past what it can spell"))
}

/// The history `bytes` spell, checked: the checksum before anything else is
/// read, then the header, then every entry decoded as a command.
fn decode(bytes: &[u8]) -> Result<Recorded, HistoryError> {
    if bytes.len() < HEADER_BYTES + DIGEST_BYTES || &bytes[..MAGIC.len()] != MAGIC {
        return Err(HistoryError::Malformed("it does not start as one"));
    }
    let (body, checksum) = bytes.split_at(bytes.len() - DIGEST_BYTES);
    if sha256(body) != checksum {
        return Err(HistoryError::Checksum);
    }
    let mut reader = Reader {
        bytes: body,
        at: MAGIC.len(),
    };
    let version = u16::from_le_bytes(reader.array()?);
    if version != HISTORY_VERSION {
        return Err(HistoryError::Version(version));
    }
    let scene = reader.array()?;
    let position = reader.length()?;
    let count = reader.length()?;
    if count > MAX_HISTORY_ENTRIES {
        return Err(HistoryError::TooMany(count));
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let done = command(index, reader.op()?)?;
        let undo = command(index, reader.op()?)?;
        entries.push((done, undo));
    }
    if reader.at != body.len() {
        return Err(HistoryError::Malformed("bytes after its last entry"));
    }
    Ok(Recorded {
        scene,
        position,
        entries,
    })
}

/// The command entry `index`'s operation `bytes` encode.
fn command(index: usize, bytes: &[u8]) -> Result<EditCommand, HistoryError> {
    match decode_op(bytes) {
        Ok(EditOp::Apply(command)) => Ok(command),
        Ok(EditOp::Undo | EditOp::Redo) => Err(HistoryError::NotACommand(index)),
        Err(error) => Err(HistoryError::Entry { index, error }),
    }
}

/// A cursor over a history's checked body.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// The next `N` bytes.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], HistoryError> {
        let taken = self.take(N)?;
        Ok(taken.try_into().expect("`take` hands back exactly N bytes"))
    }

    /// The next `u32` length, as a `usize`.
    fn length(&mut self) -> Result<usize, HistoryError> {
        usize::try_from(u32::from_le_bytes(self.array()?))
            .map_err(|_| HistoryError::Malformed("a length past what this host can hold"))
    }

    /// The next length-prefixed operation.
    fn op(&mut self) -> Result<&'a [u8], HistoryError> {
        let len = self.length()?;
        self.take(len)
    }

    /// The next `len` bytes, or a refusal where the body ends first.
    fn take(&mut self, len: usize) -> Result<&'a [u8], HistoryError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(HistoryError::Malformed("a length past its end"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }
}
