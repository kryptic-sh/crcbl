//! A scene edit on the wire: the request a client sends a server serving a
//! scene, the reason-coded answer it gets back, and the notice every client
//! of that server is sent once an edit applies.
//!
//! **The edit itself is opaque here.** A request carries the operation's
//! bytes — `crcbl_scene::edit`'s wire form of an edit command, an undo or a
//! redo — exactly as a console set carries its value as text: this crate
//! frames, bounds and seals it, and needs no scene types to do so. The crate
//! that owns the vocabulary decodes it, and refuses it with
//! [`EditRefusal::MALFORMED`] when it will not.
//!
//! A request is a [`ClientToServer::Command`](crate::ClientToServer::Command)
//! of kind [`EDIT_KIND`], beside the console set. The reply and the notice are
//! messages of their own, sealed on the reliable channel like the console
//! reply, so they arrive once and in the order sent: a client reading the
//! notice of its own edit before the reply that accepts it is the order a
//! server serving edits sends them in.
//!
//! ```text
//! edit request (a Command's data):
//!   [0]       kind = EDIT_KIND
//!   [1..9]    request_id: u64 LE, the client's own numbering
//!             op_len: u32 LE, then op: the operation's bytes
//! edit reply (a whole message):
//!   [0]       tag = EDIT_REPLY_TAG
//!   [1..9]    request_id: u64 LE, echoed
//!   [9]       outcome: 0 = applied, 1 = refused
//!   applied:  revision: u64 LE
//!   refused:  reason: u8; message_len: u16 LE, then message: UTF-8
//! edit notice (a whole message):
//!   [0]       tag = EDIT_NOTICE_TAG
//!   [1..9]    revision: u64 LE
//!   [9..17]   author: u64 LE, the server's number for the peer that sent it
//!             op_len: u32 LE, then op: the operation's bytes
//! ```

use std::fmt;

use crate::codec::{ByteReader, DecodeError, EDIT_NOTICE_TAG, EDIT_REPLY_TAG, MAX_FIELD_BYTES};

/// The kind byte of an edit request, the first thing in a command's `data`
/// after [`CONSOLE_SET_KIND`](crate::command::CONSOLE_SET_KIND).
pub const EDIT_KIND: u8 = 0x02;

/// The longest operation a request or a notice carries, in bytes: what is
/// left of one command's [`MAX_FIELD_BYTES`] once the kind, the request id and
/// the operation's length are written.
pub const MAX_EDIT_OP_BYTES: usize = MAX_FIELD_BYTES - REQUEST_HEADER_BYTES;

/// The longest refusal message a reply carries, in bytes.
///
/// A message names the entity, the system and the field it is about and says
/// why, which is a sentence; this leaves room for long names in it.
pub const MAX_EDIT_MESSAGE_BYTES: usize = 1024;

/// The kind byte, the request id and the operation's length.
const REQUEST_HEADER_BYTES: usize = 1 + 8 + 4;

const OUTCOME_APPLIED: u8 = 0;
const OUTCOME_REFUSED: u8 = 1;

/// One edit a client asks a server to make.
#[derive(Clone, PartialEq, Eq)]
pub struct EditRequest {
    /// The client's number for it, echoed in the reply so the client can tell
    /// which of its requests an answer is about.
    pub request_id: u64,
    /// The operation, in the wire form of the crate that owns the vocabulary.
    pub op: Vec<u8>,
}

/// Prints the operation's length rather than its bytes.
impl fmt::Debug for EditRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditRequest")
            .field("request_id", &self.request_id)
            .field("op_bytes", &self.op.len())
            .finish()
    }
}

/// Why a server refused an edit: a stable number a client can branch on,
/// beside a message for a person.
///
/// **The numbers are the protocol and are never reused.** A code this build
/// does not know is kept as it arrived rather than refused, as
/// [`SessionEndReason`](crate::SessionEndReason) is: the edit was refused
/// whatever the reason, and that is what the client needs; its message still
/// says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EditRefusal(pub u8);

impl EditRefusal {
    /// The operation's bytes are not an operation: cut short, an unknown
    /// kind, a length past what the bytes hold, text that is not UTF-8.
    pub const MALFORMED: Self = Self(0x01);
    /// The operation is in a wire version this server does not read.
    pub const UNSUPPORTED_VERSION: Self = Self(0x02);
    /// Nothing may be edited now: the server serves no scene for editing, or
    /// the scene it serves is playing — a game is running in it, and an edit
    /// made into play would be thrown away when play stops.
    pub const NOT_EDITABLE: Self = Self(0x03);
    /// The edit named an entity the scene does not hold — the stale edit of
    /// one another client has just deleted is the case.
    pub const UNKNOWN_ENTITY: Self = Self(0x04);
    /// The edit named a system the scene does not list, or one the server's
    /// vocabulary cannot read a row of.
    pub const UNKNOWN_SYSTEM: Self = Self(0x05);
    /// The edit named a field the component does not have, or one with fields
    /// of its own rather than a value.
    pub const UNKNOWN_PATH: Self = Self(0x06);
    /// The value is refused: of the wrong kind for its field, out of its
    /// range, refused by the component's rule, or a name that is not one.
    pub const INVALID: Self = Self(0x07);
    /// The edit contradicts the scene as it stands: a spawn under an id in
    /// use, an attach to a system already holding the entity, a detach of one
    /// that does not, a listing of a system already listed.
    pub const CONFLICT: Self = Self(0x08);
    /// An undo with nothing in the history to undo.
    pub const NOTHING_TO_UNDO: Self = Self(0x09);
    /// A redo with nothing undone to redo.
    pub const NOTHING_TO_REDO: Self = Self(0x0A);
    /// The edit was understood and could not be made for a reason none of the
    /// others name; the message says which.
    pub const FAILED: Self = Self(0x0B);

    /// The code's name as a person reads it, or [`None`] for one this build
    /// does not know.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::MALFORMED => "malformed",
            Self::UNSUPPORTED_VERSION => "unsupported version",
            Self::NOT_EDITABLE => "not editable",
            Self::UNKNOWN_ENTITY => "unknown entity",
            Self::UNKNOWN_SYSTEM => "unknown system",
            Self::UNKNOWN_PATH => "unknown path",
            Self::INVALID => "invalid",
            Self::CONFLICT => "conflict",
            Self::NOTHING_TO_UNDO => "nothing to undo",
            Self::NOTHING_TO_REDO => "nothing to redo",
            Self::FAILED => "failed",
            _ => return None,
        })
    }
}

/// Prints the code's name, or its number for one this build does not know.
impl fmt::Display for EditRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "refusal {:#04x}", self.0),
        }
    }
}

/// What became of one [`EditRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// Applied, and recorded in the server's history.
    Applied {
        /// How many operations the server has applied since it began serving
        /// the scene, this one included — the revision the
        /// [`EditNotice`] that carried it names.
        revision: u64,
    },
    /// Not applied, and why.
    Refused {
        /// The code a client branches on.
        reason: EditRefusal,
        /// The sentence a person reads.
        message: String,
    },
}

/// A server's answer to one [`EditRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditReply {
    /// The request it answers, as the client numbered it.
    pub request_id: u64,
    /// What became of it.
    pub outcome: EditOutcome,
}

/// An edit a server applied, sent to every client of the scene it serves —
/// its author included — so each can apply the same operation to its own copy
/// in the order the server did.
#[derive(Clone, PartialEq, Eq)]
pub struct EditNotice {
    /// The server's revision once this operation applied: one past the
    /// notice before it, so a client can tell it missed one.
    pub revision: u64,
    /// Which peer sent it, as the server numbers its peers.
    pub author: u64,
    /// The operation, as the request carried it.
    pub op: Vec<u8>,
}

/// Prints the operation's length rather than its bytes.
impl fmt::Debug for EditNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditNotice")
            .field("revision", &self.revision)
            .field("author", &self.author)
            .field("op_bytes", &self.op.len())
            .finish()
    }
}

/// A field past what an edit message carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditTooLong {
    /// Which field: `"op"` or `"message"`.
    pub field: &'static str,
    /// Its length, in bytes.
    pub len: usize,
    /// The most the field carries, in bytes.
    pub limit: usize,
}

impl fmt::Display for EditTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} is {} bytes, past the {} an edit message carries",
            self.field, self.len, self.limit
        )
    }
}

impl std::error::Error for EditTooLong {}

/// The `data` of a [`ClientToServer::Command`](crate::ClientToServer::Command)
/// carrying `request`.
///
/// # Errors
///
/// [`EditTooLong`] for an operation past [`MAX_EDIT_OP_BYTES`].
pub fn encode_edit_request(request: &EditRequest) -> Result<Vec<u8>, EditTooLong> {
    let op_len = checked_op_len(&request.op)?;
    let mut buf = Vec::with_capacity(REQUEST_HEADER_BYTES + request.op.len());
    buf.push(EDIT_KIND);
    buf.extend_from_slice(&request.request_id.to_le_bytes());
    buf.extend_from_slice(&op_len.to_le_bytes());
    buf.extend_from_slice(&request.op);
    Ok(buf)
}

/// The [`EditRequest`] in a command's `data`.
///
/// # Errors
///
/// [`DecodeError`] for another kind byte, an operation length past
/// [`MAX_EDIT_OP_BYTES`] or past the bytes there are, a short buffer or
/// trailing bytes.
pub fn decode_edit_request(data: &[u8]) -> Result<EditRequest, DecodeError> {
    let mut r = ByteReader::new(data);
    let kind = r.read_u8()?;
    if kind != EDIT_KIND {
        return Err(DecodeError::UnknownTag { tag: kind });
    }
    let request_id = r.read_u64()?;
    let op = read_op(&mut r)?;
    r.assert_empty()?;
    Ok(EditRequest { request_id, op })
}

/// The whole message carrying `reply`, ready to seal.
///
/// # Errors
///
/// [`EditTooLong`] for a refusal message past [`MAX_EDIT_MESSAGE_BYTES`].
pub fn encode_edit_reply(reply: &EditReply) -> Result<Vec<u8>, EditTooLong> {
    let mut buf = Vec::with_capacity(1 + 8 + 1 + 8);
    buf.push(EDIT_REPLY_TAG);
    buf.extend_from_slice(&reply.request_id.to_le_bytes());
    match &reply.outcome {
        EditOutcome::Applied { revision } => {
            buf.push(OUTCOME_APPLIED);
            buf.extend_from_slice(&revision.to_le_bytes());
        }
        EditOutcome::Refused { reason, message } => {
            let too_long = EditTooLong {
                field: "message",
                len: message.len(),
                limit: MAX_EDIT_MESSAGE_BYTES,
            };
            if message.len() > MAX_EDIT_MESSAGE_BYTES {
                return Err(too_long);
            }
            let len = u16::try_from(message.len()).map_err(|_| too_long)?;
            buf.push(OUTCOME_REFUSED);
            buf.push(reason.0);
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(message.as_bytes());
        }
    }
    Ok(buf)
}

/// The [`EditReply`] in a whole message.
///
/// # Errors
///
/// [`DecodeError`] for another tag or outcome byte, a message past
/// [`MAX_EDIT_MESSAGE_BYTES`] or not UTF-8, a short buffer or trailing bytes.
pub fn decode_edit_reply(payload: &[u8]) -> Result<EditReply, DecodeError> {
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != EDIT_REPLY_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let request_id = r.read_u64()?;
    let outcome = match r.read_u8()? {
        OUTCOME_APPLIED => EditOutcome::Applied {
            revision: r.read_u64()?,
        },
        OUTCOME_REFUSED => {
            let reason = EditRefusal(r.read_u8()?);
            let len = usize::from(r.read_u16()?);
            if len > MAX_EDIT_MESSAGE_BYTES {
                return Err(invalid_length(len));
            }
            let message =
                String::from_utf8(r.read_bytes(len)?.to_vec()).map_err(|_| invalid_length(len))?;
            EditOutcome::Refused { reason, message }
        }
        other => return Err(DecodeError::UnknownTag { tag: other }),
    };
    r.assert_empty()?;
    Ok(EditReply {
        request_id,
        outcome,
    })
}

/// The whole message carrying `notice`, ready to seal.
///
/// # Errors
///
/// [`EditTooLong`] for an operation past [`MAX_EDIT_OP_BYTES`].
pub fn encode_edit_notice(notice: &EditNotice) -> Result<Vec<u8>, EditTooLong> {
    let op_len = checked_op_len(&notice.op)?;
    let mut buf = Vec::with_capacity(1 + 8 + 8 + 4 + notice.op.len());
    buf.push(EDIT_NOTICE_TAG);
    buf.extend_from_slice(&notice.revision.to_le_bytes());
    buf.extend_from_slice(&notice.author.to_le_bytes());
    buf.extend_from_slice(&op_len.to_le_bytes());
    buf.extend_from_slice(&notice.op);
    Ok(buf)
}

/// The [`EditNotice`] in a whole message.
///
/// # Errors
///
/// [`DecodeError`] for another tag, and on [`decode_edit_request`]'s terms
/// for the operation.
pub fn decode_edit_notice(payload: &[u8]) -> Result<EditNotice, DecodeError> {
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != EDIT_NOTICE_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let revision = r.read_u64()?;
    let author = r.read_u64()?;
    let op = read_op(&mut r)?;
    r.assert_empty()?;
    Ok(EditNotice {
        revision,
        author,
        op,
    })
}

/// `op`'s length as the `u32` the wire writes, refused past
/// [`MAX_EDIT_OP_BYTES`].
fn checked_op_len(op: &[u8]) -> Result<u32, EditTooLong> {
    let too_long = EditTooLong {
        field: "op",
        len: op.len(),
        limit: MAX_EDIT_OP_BYTES,
    };
    if op.len() > MAX_EDIT_OP_BYTES {
        return Err(too_long);
    }
    u32::try_from(op.len()).map_err(|_| too_long)
}

/// An operation's length and its bytes, the length checked against
/// [`MAX_EDIT_OP_BYTES`] before anything is read for it.
fn read_op(r: &mut ByteReader<'_>) -> Result<Vec<u8>, DecodeError> {
    let len = r.read_u32()?;
    let len = usize::try_from(len).map_err(|_| DecodeError::InvalidLength(len))?;
    if len > MAX_EDIT_OP_BYTES {
        return Err(invalid_length(len));
    }
    Ok(r.read_bytes(len)?.to_vec())
}

fn invalid_length(len: usize) -> DecodeError {
    DecodeError::InvalidLength(u32::try_from(len).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests;
