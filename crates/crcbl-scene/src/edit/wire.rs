//! An edit operation as bytes: what a client sends a server serving a scene,
//! and what that server tells every other client it applied.
//!
//! **The vocabulary is [`EditCommand`], and this is only its spelling.** An
//! [`EditOp`] is a command to apply, or a step of the server's history — an
//! undo or a redo of whatever was done last, by anyone, which is the plan's
//! one global undo log (`docs/plan/08-editor.md`'s 2026-07-27 correction). The
//! server applies a decoded op exactly as the editor applies one a key built,
//! so nothing here is a second set of rules.
//!
//! **Hand-written, not serde.** The commands carry [`Value`]s and rows that are
//! already text, every field has a length the decoder checks against the bytes
//! there are before reading it, and nesting is bounded by
//! [`MAX_BATCH_DEPTH`] — a decoder that reads what a peer chose must never
//! allocate past its input or recurse without end, and this one is held to it
//! by the decoder fuzz target (`crates/crcbl-net/fuzz`).
//!
//! **A variant switch does not travel yet.** [`EditCommand::SetVariant`]
//! carries a [`Snapshot`](crcbl_reflect::Snapshot) whose variant names are
//! `&'static str` — they are the names the type itself answers — and a name
//! read off the wire has no type to borrow one from until it is resolved
//! against the component. [`encode_op`] refuses one by name
//! ([`OpEncodeError::SetVariant`]) and the decoder reserves its kind byte.
//!
//! ```text
//! op:
//!   [0]   version = WIRE_VERSION
//!   [1]   0 = apply, then a command; 1 = undo; 2 = redo
//! command, kind byte first:
//!   0x01  set property: entity u32, system text, path text, value
//!   0x02  (a variant switch: reserved, refused)
//!   0x03  spawn: entity u32, row count u32, rows (system text, row text),
//!         name (0 = none, 1 = text)
//!   0x04  delete: entity u32
//!   0x05  attach: entity u32, system text, row text
//!   0x06  detach: entity u32, system text
//!   0x07  list system: system text, place u64
//!   0x08  unlist system: system text
//!   0x09  rename: entity u32, name (0 = none, 1 = text)
//!   0x0A  batch: count u32, then that many commands
//! value, tag byte first:
//!   0 = bool (one byte, 0 or 1), 1 = int i64, 2 = uint u64,
//!   3 = float (f64 bits), 4 = text
//! text: length u32, then UTF-8; every integer little-endian
//! ```

use crcbl_reflect::Value;

use super::{EditCommand, SystemRow};
use crate::scn::{EntityName, NameError, SceneEntityId};

/// The wire version this build writes and the one it reads.
///
/// Bumped when the spelling changes, so a server meeting an op from another
/// build refuses it by version rather than reading it as something else.
pub const WIRE_VERSION: u8 = 1;

/// How deep batches may nest inside one op.
///
/// Nothing the editor builds nests at all — a paste, a multi-delete and an
/// attach that lists its system are each one flat batch — so this is room, not
/// a measurement; what it bounds is how far a peer's bytes can recurse the
/// decoder.
pub const MAX_BATCH_DEPTH: usize = 8;

const OP_APPLY: u8 = 0;
const OP_UNDO: u8 = 1;
const OP_REDO: u8 = 2;

const SET_PROPERTY: u8 = 0x01;
const SET_VARIANT: u8 = 0x02;
const SPAWN: u8 = 0x03;
const DELETE: u8 = 0x04;
const ATTACH: u8 = 0x05;
const DETACH: u8 = 0x06;
const LIST_SYSTEM: u8 = 0x07;
const UNLIST_SYSTEM: u8 = 0x08;
const RENAME: u8 = 0x09;
const BATCH: u8 = 0x0A;

const VALUE_BOOL: u8 = 0;
const VALUE_INT: u8 = 1;
const VALUE_UINT: u8 = 2;
const VALUE_FLOAT: u8 = 3;
const VALUE_TEXT: u8 = 4;

/// The fewest bytes a command can be: a kind byte and a `u32` — a delete's
/// entity, or an unlisting's empty system name's length.
const MIN_COMMAND_BYTES: usize = 1 + 4;

/// The fewest bytes a spawn's row can be: two empty texts' lengths.
const MIN_ROW_BYTES: usize = 4 + 4;

/// One thing a client asks of a server serving a scene.
#[derive(Clone, Debug, PartialEq)]
pub enum EditOp {
    /// Apply this command, and record it with the inverse it produces.
    Apply(EditCommand),
    /// Step the history back over the last entry applied, whoever made it.
    Undo,
    /// Step the history forward over the last entry undone.
    Redo,
}

/// Why an [`EditOp`] has no wire form.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpEncodeError {
    /// A variant switch, which does not travel yet: its snapshot's variant
    /// names are the type's own `&'static str`s, and a name read off the wire
    /// has no type to borrow one from.
    #[error("a variant switch does not travel over the wire yet")]
    SetVariant,
    /// An environment write, which this wire version has no kind for.
    #[error("an environment write does not travel over the wire yet")]
    SetEnvironment,
    /// A text or a count past what its `u32` length can say.
    #[error("the {what} is {len} long, past what one op can carry")]
    TooLong {
        /// Which field.
        what: &'static str,
        /// Its length.
        len: usize,
    },
    /// Batches nested past [`MAX_BATCH_DEPTH`].
    #[error("batches nest past the {MAX_BATCH_DEPTH} levels one op may hold")]
    TooDeep,
}

/// Why bytes are not an [`EditOp`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpDecodeError {
    /// Another wire version than [`WIRE_VERSION`].
    #[error("the op is wire version {found}, and this build reads version {WIRE_VERSION}")]
    Version {
        /// The version the op said it was.
        found: u8,
    },
    /// The bytes end before the op does.
    #[error("the op is cut short: {needed} bytes wanted at offset {offset}, {remaining} left")]
    Short {
        /// Where the read was.
        offset: usize,
        /// How many bytes it wanted.
        needed: usize,
        /// How many there were.
        remaining: usize,
    },
    /// An op kind this version does not have.
    #[error("unknown op kind {0:#04x}")]
    UnknownOp(u8),
    /// A command kind this version does not have.
    #[error("unknown command kind {0:#04x}")]
    UnknownCommand(u8),
    /// A variant switch's kind byte, which no op carries yet.
    #[error("a variant switch does not travel over the wire yet")]
    SetVariant,
    /// A value tag this version does not have.
    #[error("unknown value tag {0:#04x}")]
    UnknownValue(u8),
    /// A flag or a presence byte that is neither 0 nor 1.
    #[error("byte {found:#04x} at offset {offset} is neither 0 nor 1")]
    NotABool {
        /// Where it was.
        offset: usize,
        /// What it was.
        found: u8,
    },
    /// Text that is not UTF-8.
    #[error("the text at offset {offset} is not UTF-8")]
    NotUtf8 {
        /// Where its bytes began.
        offset: usize,
    },
    /// A name that is not a name.
    #[error(transparent)]
    Name(#[from] NameError),
    /// A count the bytes left could not hold, refused before anything is
    /// allocated for it.
    #[error("a count of {count} at offset {offset} is more than the bytes left can hold")]
    Count {
        /// Where the count was.
        offset: usize,
        /// What it said.
        count: u32,
    },
    /// A manifest place past what this platform's `usize` holds.
    #[error("place {0} is past what this platform can index")]
    Place(u64),
    /// Batches nested past [`MAX_BATCH_DEPTH`].
    #[error("batches nest past the {MAX_BATCH_DEPTH} levels one op may hold")]
    TooDeep,
    /// Bytes after the op.
    #[error("{0} bytes after the op")]
    Trailing(usize),
}

/// The wire form of `op`.
///
/// # Errors
///
/// [`OpEncodeError::SetVariant`] for a variant switch anywhere in it,
/// [`OpEncodeError::TooDeep`] for batches nested past [`MAX_BATCH_DEPTH`], and
/// [`OpEncodeError::TooLong`] for a text or a count no `u32` can say.
pub fn encode_op(op: &EditOp) -> Result<Vec<u8>, OpEncodeError> {
    let mut out = vec![WIRE_VERSION];
    match op {
        EditOp::Apply(command) => {
            out.push(OP_APPLY);
            put_command(&mut out, command, 0)?;
        }
        EditOp::Undo => out.push(OP_UNDO),
        EditOp::Redo => out.push(OP_REDO),
    }
    Ok(out)
}

/// The [`EditOp`] in `bytes`.
///
/// # Errors
///
/// [`OpDecodeError`] naming what is wrong with them: another version, a kind
/// or tag this version does not have, a length or count past the bytes there
/// are, text that is not UTF-8, a name that is not one, batches nested past
/// [`MAX_BATCH_DEPTH`], or bytes after the op. Never a panic, whatever the
/// bytes.
pub fn decode_op(bytes: &[u8]) -> Result<EditOp, OpDecodeError> {
    let mut r = Reader { bytes, offset: 0 };
    let version = r.u8()?;
    if version != WIRE_VERSION {
        return Err(OpDecodeError::Version { found: version });
    }
    let op = match r.u8()? {
        OP_APPLY => EditOp::Apply(r.command(0)?),
        OP_UNDO => EditOp::Undo,
        OP_REDO => EditOp::Redo,
        other => return Err(OpDecodeError::UnknownOp(other)),
    };
    match r.remaining() {
        0 => Ok(op),
        left => Err(OpDecodeError::Trailing(left)),
    }
}

fn put_command(
    out: &mut Vec<u8>,
    command: &EditCommand,
    depth: usize,
) -> Result<(), OpEncodeError> {
    match command {
        EditCommand::SetProperty {
            entity,
            system,
            path,
            value,
        } => {
            out.push(SET_PROPERTY);
            put_entity(out, *entity);
            put_text(out, "system", system)?;
            put_text(out, "path", path)?;
            put_value(out, value)?;
        }
        EditCommand::SetVariant { .. } => return Err(OpEncodeError::SetVariant),
        EditCommand::SetEnvironment { .. } => return Err(OpEncodeError::SetEnvironment),
        EditCommand::Spawn { entity, rows, name } => {
            out.push(SPAWN);
            put_entity(out, *entity);
            put_count(out, "row count", rows.len())?;
            for row in rows {
                put_text(out, "system", &row.system)?;
                put_text(out, "row", &row.row)?;
            }
            put_name(out, name.as_ref())?;
        }
        EditCommand::Delete { entity } => {
            out.push(DELETE);
            put_entity(out, *entity);
        }
        EditCommand::Attach {
            entity,
            system,
            row,
        } => {
            out.push(ATTACH);
            put_entity(out, *entity);
            put_text(out, "system", system)?;
            put_text(out, "row", row)?;
        }
        EditCommand::Detach { entity, system } => {
            out.push(DETACH);
            put_entity(out, *entity);
            put_text(out, "system", system)?;
        }
        EditCommand::ListSystem { system, at } => {
            out.push(LIST_SYSTEM);
            put_text(out, "system", system)?;
            let at = u64::try_from(*at).map_err(|_| OpEncodeError::TooLong {
                what: "place",
                len: *at,
            })?;
            out.extend_from_slice(&at.to_le_bytes());
        }
        EditCommand::UnlistSystem { system } => {
            out.push(UNLIST_SYSTEM);
            put_text(out, "system", system)?;
        }
        EditCommand::Rename { entity, name } => {
            out.push(RENAME);
            put_entity(out, *entity);
            put_name(out, name.as_ref())?;
        }
        EditCommand::Batch(members) => {
            if depth >= MAX_BATCH_DEPTH {
                return Err(OpEncodeError::TooDeep);
            }
            out.push(BATCH);
            put_count(out, "batch", members.len())?;
            for member in members {
                put_command(out, member, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn put_entity(out: &mut Vec<u8>, entity: SceneEntityId) {
    out.extend_from_slice(&entity.0.to_le_bytes());
}

fn put_count(out: &mut Vec<u8>, what: &'static str, count: usize) -> Result<(), OpEncodeError> {
    let count = u32::try_from(count).map_err(|_| OpEncodeError::TooLong { what, len: count })?;
    out.extend_from_slice(&count.to_le_bytes());
    Ok(())
}

fn put_text(out: &mut Vec<u8>, what: &'static str, text: &str) -> Result<(), OpEncodeError> {
    put_count(out, what, text.len())?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn put_name(out: &mut Vec<u8>, name: Option<&EntityName>) -> Result<(), OpEncodeError> {
    match name {
        None => out.push(0),
        Some(name) => {
            out.push(1);
            put_text(out, "name", name.as_str())?;
        }
    }
    Ok(())
}

fn put_value(out: &mut Vec<u8>, value: &Value) -> Result<(), OpEncodeError> {
    match value {
        Value::Bool(flag) => {
            out.push(VALUE_BOOL);
            out.push(u8::from(*flag));
        }
        Value::Int(number) => {
            out.push(VALUE_INT);
            out.extend_from_slice(&number.to_le_bytes());
        }
        Value::UInt(number) => {
            out.push(VALUE_UINT);
            out.extend_from_slice(&number.to_le_bytes());
        }
        // By its bits, so a value arrives as the one sent — a signed zero and
        // a NaN's payload included, which the leaf then judges for itself.
        Value::Float(number) => {
            out.push(VALUE_FLOAT);
            out.extend_from_slice(&number.to_bits().to_le_bytes());
        }
        Value::Text(text) => {
            out.push(VALUE_TEXT);
            put_text(out, "text", text)?;
        }
    }
    Ok(())
}

/// A cursor over an op's bytes that checks every read against what is left.
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, needed: usize) -> Result<&'a [u8], OpDecodeError> {
        if needed > self.remaining() {
            return Err(OpDecodeError::Short {
                offset: self.offset,
                needed,
                remaining: self.remaining(),
            });
        }
        let taken = &self.bytes[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], OpDecodeError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn u8(&mut self) -> Result<u8, OpDecodeError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, OpDecodeError> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, OpDecodeError> {
        self.array().map(u64::from_le_bytes)
    }

    fn bool(&mut self) -> Result<bool, OpDecodeError> {
        let offset = self.offset;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            found => Err(OpDecodeError::NotABool { offset, found }),
        }
    }

    fn entity(&mut self) -> Result<SceneEntityId, OpDecodeError> {
        self.u32().map(SceneEntityId)
    }

    /// A count of things at least `min_bytes` long each, refused when the
    /// bytes left could not hold that many — before anything is allocated
    /// for them.
    fn count(&mut self, min_bytes: usize) -> Result<usize, OpDecodeError> {
        let offset = self.offset;
        let count = self.u32()?;
        let fits = usize::try_from(count)
            .ok()
            .filter(|&count| count <= self.remaining() / min_bytes);
        fits.ok_or(OpDecodeError::Count { offset, count })
    }

    fn text(&mut self) -> Result<String, OpDecodeError> {
        let len = self.count(1)?;
        let offset = self.offset;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| OpDecodeError::NotUtf8 { offset })
    }

    fn name(&mut self) -> Result<Option<EntityName>, OpDecodeError> {
        if !self.bool()? {
            return Ok(None);
        }
        Ok(Some(EntityName::new(&self.text()?)?))
    }

    fn value(&mut self) -> Result<Value, OpDecodeError> {
        Ok(match self.u8()? {
            VALUE_BOOL => Value::Bool(self.bool()?),
            VALUE_INT => Value::Int(i64::from_le_bytes(self.array()?)),
            VALUE_UINT => Value::UInt(self.u64()?),
            VALUE_FLOAT => Value::Float(f64::from_bits(self.u64()?)),
            VALUE_TEXT => Value::Text(self.text()?),
            other => return Err(OpDecodeError::UnknownValue(other)),
        })
    }

    fn command(&mut self, depth: usize) -> Result<EditCommand, OpDecodeError> {
        Ok(match self.u8()? {
            SET_PROPERTY => EditCommand::SetProperty {
                entity: self.entity()?,
                system: self.text()?,
                path: self.text()?,
                value: self.value()?,
            },
            SET_VARIANT => return Err(OpDecodeError::SetVariant),
            SPAWN => {
                let entity = self.entity()?;
                let count = self.count(MIN_ROW_BYTES)?;
                let mut rows = Vec::with_capacity(count);
                for _ in 0..count {
                    rows.push(SystemRow {
                        system: self.text()?,
                        row: self.text()?,
                    });
                }
                EditCommand::Spawn {
                    entity,
                    rows,
                    name: self.name()?,
                }
            }
            DELETE => EditCommand::Delete {
                entity: self.entity()?,
            },
            ATTACH => EditCommand::Attach {
                entity: self.entity()?,
                system: self.text()?,
                row: self.text()?,
            },
            DETACH => EditCommand::Detach {
                entity: self.entity()?,
                system: self.text()?,
            },
            LIST_SYSTEM => {
                let system = self.text()?;
                let at = self.u64()?;
                EditCommand::ListSystem {
                    system,
                    at: usize::try_from(at).map_err(|_| OpDecodeError::Place(at))?,
                }
            }
            UNLIST_SYSTEM => EditCommand::UnlistSystem {
                system: self.text()?,
            },
            RENAME => EditCommand::Rename {
                entity: self.entity()?,
                name: self.name()?,
            },
            BATCH => {
                if depth >= MAX_BATCH_DEPTH {
                    return Err(OpDecodeError::TooDeep);
                }
                let count = self.count(MIN_COMMAND_BYTES)?;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(self.command(depth + 1)?);
                }
                EditCommand::Batch(members)
            }
            other => return Err(OpDecodeError::UnknownCommand(other)),
        })
    }
}

#[cfg(test)]
mod tests;
