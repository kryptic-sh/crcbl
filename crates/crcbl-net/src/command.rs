//! What a [`ClientToServer::Command`](crate::ClientToServer::Command) carries,
//! and the server's answer to it: a console set of a simulation variable.
//!
//! A command's `data` starts with a kind byte, so later kinds of command can
//! share the message; [`CONSOLE_SET_KIND`] is the first. The set travels as
//! the variable's name and its value **as text** — the text the client's
//! console printed it as, which the server parses back through the same
//! console kind — so this crate needs no console types and the value a server
//! applies is the value the client's console checked. The server checks it
//! again; nothing here is trusted past its framing.
//!
//! The answer is a message of its own, [`CONSOLE_REPLY_TAG`], sealed on the
//! reliable channel like the session end, and not a
//! [`ServerToClient::Event`](crate::ServerToClient::Event): an event's bytes
//! are the game's own format, and a reply mixed in among them would be read
//! as one.
//!
//! ```text
//! console set (a Command's data):
//!   [0]       kind = CONSOLE_SET_KIND
//!   [1]       name_len: u8, then name: UTF-8
//!             value_len: u16 LE, then value: UTF-8
//! console reply (a whole message):
//!   [0]       tag = CONSOLE_REPLY_TAG
//!   [1]       outcome: 0 = applied, 1 = refused
//!             name_len: u8, name; value_len: u16 LE, value
//!   applied:  tick: u64 LE
//!   refused:  reason_len: u16 LE, then reason: UTF-8
//! ```

use std::fmt;

use crcbl_core::TickId;

use crate::codec::{ByteReader, CONSOLE_REPLY_TAG, DecodeError};

/// The kind byte of a console set, the first thing in a command's `data`.
pub const CONSOLE_SET_KIND: u8 = 0x01;

/// The longest variable name a set carries, in bytes.
///
/// A console name is an identifier a person types; this is several times the
/// longest one in the workspace, and it is what bounds the reply's echo too.
pub const MAX_CONSOLE_NAME_BYTES: usize = 64;

/// The longest value text a set carries, in bytes.
pub const MAX_CONSOLE_VALUE_BYTES: usize = 256;

/// The longest refusal reason a reply carries, in bytes.
///
/// A reason names the variable and the value, so this leaves room for both at
/// their limits and a sentence around them.
pub const MAX_CONSOLE_REASON_BYTES: usize = 1024;

const OUTCOME_APPLIED: u8 = 0;
const OUTCOME_REFUSED: u8 = 1;

/// A console set of one simulation variable, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleSet {
    /// The variable's name.
    pub name: String,
    /// The value, as the console prints it.
    pub value: String,
}

/// What became of a [`ConsoleSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleOutcome {
    /// Applied at the start of this tick: the first tick whose simulation read
    /// the new value.
    Applied(TickId),
    /// Not applied, and why.
    Refused(String),
}

/// The server's answer to one [`ConsoleSet`], naming the set it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleReply {
    /// The variable's name, as the server spells it once it is known — as the
    /// client sent it otherwise.
    pub name: String,
    /// The value, as the client sent it.
    pub value: String,
    /// What became of it.
    pub outcome: ConsoleOutcome,
}

/// Prints the line a console shows: `sv_spin_rate = 2, applied at tick 120`,
/// or `sv_spin_rate 2 refused: <reason>`.
impl fmt::Display for ConsoleReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.outcome {
            ConsoleOutcome::Applied(tick) => write!(
                f,
                "{} = {}, applied at tick {}",
                self.name,
                self.value,
                tick.get()
            ),
            ConsoleOutcome::Refused(reason) => {
                write!(f, "{} {} refused: {reason}", self.name, self.value)
            }
        }
    }
}

/// A console text past what its field carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleTextTooLong {
    /// Which field: `"name"`, `"value"` or `"reason"`.
    pub field: &'static str,
    /// Its length, in bytes.
    pub len: usize,
    /// The most the field carries, in bytes.
    pub limit: usize,
}

impl fmt::Display for ConsoleTextTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} is {} bytes, past the {} a console message carries",
            self.field, self.len, self.limit
        )
    }
}

impl std::error::Error for ConsoleTextTooLong {}

/// The `data` of a [`ClientToServer::Command`](crate::ClientToServer::Command)
/// carrying `set`.
///
/// # Errors
///
/// [`ConsoleTextTooLong`] when the name is empty or past
/// [`MAX_CONSOLE_NAME_BYTES`], or the value past [`MAX_CONSOLE_VALUE_BYTES`].
pub fn encode_console_set(set: &ConsoleSet) -> Result<Vec<u8>, ConsoleTextTooLong> {
    let mut buf = Vec::with_capacity(1 + 1 + set.name.len() + 2 + set.value.len());
    buf.push(CONSOLE_SET_KIND);
    put_name_and_value(&mut buf, &set.name, &set.value)?;
    Ok(buf)
}

/// The [`ConsoleSet`] in a command's `data`.
///
/// # Errors
///
/// [`DecodeError`] for another kind byte, a field past its limit, an empty
/// name, text that is not UTF-8, a short buffer or trailing bytes.
pub fn decode_console_set(data: &[u8]) -> Result<ConsoleSet, DecodeError> {
    let mut r = ByteReader::new(data);
    let kind = r.read_u8()?;
    if kind != CONSOLE_SET_KIND {
        return Err(DecodeError::UnknownTag { tag: kind });
    }
    let (name, value) = read_name_and_value(&mut r)?;
    r.assert_empty()?;
    Ok(ConsoleSet { name, value })
}

/// The whole message carrying `reply`, ready to seal.
///
/// # Errors
///
/// [`ConsoleTextTooLong`] on [`encode_console_set`]'s terms, or for a reason
/// past [`MAX_CONSOLE_REASON_BYTES`].
pub fn encode_console_reply(reply: &ConsoleReply) -> Result<Vec<u8>, ConsoleTextTooLong> {
    let mut buf = Vec::with_capacity(2 + 1 + reply.name.len() + 2 + reply.value.len() + 8);
    buf.push(CONSOLE_REPLY_TAG);
    let outcome = match reply.outcome {
        ConsoleOutcome::Applied(_) => OUTCOME_APPLIED,
        ConsoleOutcome::Refused(_) => OUTCOME_REFUSED,
    };
    buf.push(outcome);
    put_name_and_value(&mut buf, &reply.name, &reply.value)?;
    match &reply.outcome {
        ConsoleOutcome::Applied(tick) => buf.extend_from_slice(&tick.get().to_le_bytes()),
        ConsoleOutcome::Refused(reason) => {
            let len = checked_len("reason", reason, MAX_CONSOLE_REASON_BYTES)?;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(reason.as_bytes());
        }
    }
    Ok(buf)
}

/// The [`ConsoleReply`] in a whole message.
///
/// # Errors
///
/// [`DecodeError`] for another tag or outcome byte, and on
/// [`decode_console_set`]'s terms for the fields.
pub fn decode_console_reply(payload: &[u8]) -> Result<ConsoleReply, DecodeError> {
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != CONSOLE_REPLY_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let outcome = r.read_u8()?;
    let (name, value) = read_name_and_value(&mut r)?;
    let outcome = match outcome {
        OUTCOME_APPLIED => ConsoleOutcome::Applied(TickId::from_raw(r.read_u64()?)),
        OUTCOME_REFUSED => {
            let len = usize::from(r.read_u16()?);
            ConsoleOutcome::Refused(read_text(&mut r, len, MAX_CONSOLE_REASON_BYTES)?)
        }
        other => return Err(DecodeError::UnknownTag { tag: other }),
    };
    r.assert_empty()?;
    Ok(ConsoleReply {
        name,
        value,
        outcome,
    })
}

/// Appends the name (`u8` length) and the value (`u16` length).
fn put_name_and_value(
    buf: &mut Vec<u8>,
    name: &str,
    value: &str,
) -> Result<(), ConsoleTextTooLong> {
    if name.is_empty() {
        return Err(ConsoleTextTooLong {
            field: "name",
            len: 0,
            limit: MAX_CONSOLE_NAME_BYTES,
        });
    }
    let name_len = u8::try_from(checked_len("name", name, MAX_CONSOLE_NAME_BYTES)?)
        .expect("the name limit fits a u8");
    let value_len = checked_len("value", value, MAX_CONSOLE_VALUE_BYTES)?;
    buf.push(name_len);
    buf.extend_from_slice(name.as_bytes());
    buf.extend_from_slice(&value_len.to_le_bytes());
    buf.extend_from_slice(value.as_bytes());
    Ok(())
}

/// `text`'s length as the `u16` the wire writes, refused past `limit`.
fn checked_len(field: &'static str, text: &str, limit: usize) -> Result<u16, ConsoleTextTooLong> {
    let too_long = ConsoleTextTooLong {
        field,
        len: text.len(),
        limit,
    };
    if text.len() > limit {
        return Err(too_long);
    }
    u16::try_from(text.len()).map_err(|_| too_long)
}

fn read_name_and_value(r: &mut ByteReader<'_>) -> Result<(String, String), DecodeError> {
    let name_len = usize::from(r.read_u8()?);
    if name_len == 0 {
        return Err(DecodeError::InvalidLength(0));
    }
    let name = read_text(r, name_len, MAX_CONSOLE_NAME_BYTES)?;
    let value_len = usize::from(r.read_u16()?);
    let value = read_text(r, value_len, MAX_CONSOLE_VALUE_BYTES)?;
    Ok((name, value))
}

/// `len` bytes of UTF-8, refused past `limit`. A text that is not UTF-8 is
/// reported as [`DecodeError::InvalidLength`], as the handshake's reject
/// message is.
fn read_text(r: &mut ByteReader<'_>, len: usize, limit: usize) -> Result<String, DecodeError> {
    let invalid = || DecodeError::InvalidLength(u32::try_from(len).unwrap_or(u32::MAX));
    if len > limit {
        return Err(invalid());
    }
    let bytes = r.read_bytes(len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, value: &str) -> ConsoleSet {
        ConsoleSet {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn a_set_is_the_kind_byte_then_the_name_then_the_value() {
        let encoded = encode_console_set(&set("sv_spin_rate", "2")).expect("short enough");
        let mut expected = vec![CONSOLE_SET_KIND, 12];
        expected.extend_from_slice(b"sv_spin_rate");
        expected.extend_from_slice(&[1, 0, b'2']);
        assert_eq!(encoded, expected);
        assert_eq!(
            decode_console_set(&encoded).expect("well formed"),
            set("sv_spin_rate", "2")
        );
    }

    #[test]
    fn both_outcomes_of_a_reply_round_trip() {
        for outcome in [
            ConsoleOutcome::Applied(TickId::from_raw(0x0102_0304_0506)),
            ConsoleOutcome::Refused("`sv_spin_rate`: 99 is outside 0..=8".to_owned()),
        ] {
            let reply = ConsoleReply {
                name: "sv_spin_rate".to_owned(),
                value: "99".to_owned(),
                outcome,
            };
            let encoded = encode_console_reply(&reply).expect("short enough");
            assert_eq!(encoded[0], CONSOLE_REPLY_TAG);
            assert_eq!(decode_console_reply(&encoded).expect("well formed"), reply);
        }
    }

    #[test]
    fn a_reply_prints_the_line_a_console_shows() {
        let applied = ConsoleReply {
            name: "sv_spin_rate".to_owned(),
            value: "2".to_owned(),
            outcome: ConsoleOutcome::Applied(TickId::from_raw(120)),
        };
        assert_eq!(applied.to_string(), "sv_spin_rate = 2, applied at tick 120");
        let refused = ConsoleReply {
            outcome: ConsoleOutcome::Refused("only the host may".to_owned()),
            ..applied
        };
        assert_eq!(
            refused.to_string(),
            "sv_spin_rate 2 refused: only the host may"
        );
    }

    #[test]
    fn an_encoder_refuses_what_its_decoder_would() {
        let long_name = "n".repeat(MAX_CONSOLE_NAME_BYTES + 1);
        assert_eq!(
            encode_console_set(&set(&long_name, "1")).expect_err("too long"),
            ConsoleTextTooLong {
                field: "name",
                len: MAX_CONSOLE_NAME_BYTES + 1,
                limit: MAX_CONSOLE_NAME_BYTES,
            }
        );
        assert_eq!(
            encode_console_set(&set("", "1")).expect_err("empty").field,
            "name"
        );
        let long_value = "1".repeat(MAX_CONSOLE_VALUE_BYTES + 1);
        assert_eq!(
            encode_console_set(&set("a", &long_value))
                .expect_err("too long")
                .field,
            "value"
        );
        let reply = ConsoleReply {
            name: "a".to_owned(),
            value: "1".to_owned(),
            outcome: ConsoleOutcome::Refused("r".repeat(MAX_CONSOLE_REASON_BYTES + 1)),
        };
        assert_eq!(
            encode_console_reply(&reply).expect_err("too long").field,
            "reason"
        );
        // The limits themselves are carried.
        let at_limit = set(
            &"n".repeat(MAX_CONSOLE_NAME_BYTES),
            &"1".repeat(MAX_CONSOLE_VALUE_BYTES),
        );
        let encoded = encode_console_set(&at_limit).expect("at the limits");
        assert_eq!(
            decode_console_set(&encoded).expect("at the limits"),
            at_limit
        );
    }

    #[test]
    fn a_decoder_refuses_every_malformed_set() {
        let good = encode_console_set(&set("ab", "1")).expect("short enough");
        // Every truncation.
        for len in 0..good.len() {
            assert!(decode_console_set(&good[..len]).is_err(), "{len} bytes");
        }
        // Trailing bytes.
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(matches!(
            decode_console_set(&trailing),
            Err(DecodeError::TrailingBytes(1))
        ));
        // Another kind.
        let mut other = good.clone();
        other[0] = 0x7F;
        assert!(matches!(
            decode_console_set(&other),
            Err(DecodeError::UnknownTag { tag: 0x7F })
        ));
        // An empty name.
        assert!(matches!(
            decode_console_set(&[CONSOLE_SET_KIND, 0, 0, 0]),
            Err(DecodeError::InvalidLength(0))
        ));
        // A name past its limit, claimed by the length byte alone.
        let mut long = vec![
            CONSOLE_SET_KIND,
            u8::try_from(MAX_CONSOLE_NAME_BYTES + 1).unwrap(),
        ];
        long.extend(std::iter::repeat_n(b'n', MAX_CONSOLE_NAME_BYTES + 1));
        long.extend_from_slice(&[0, 0]);
        assert!(decode_console_set(&long).is_err());
        // A value length past its limit, before any value bytes are read.
        let mut huge = vec![CONSOLE_SET_KIND, 1, b'a'];
        huge.extend_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(
            decode_console_set(&huge),
            Err(DecodeError::InvalidLength(_))
        ));
        // Text that is not UTF-8.
        assert!(decode_console_set(&[CONSOLE_SET_KIND, 1, 0xFF, 0, 0]).is_err());
    }

    #[test]
    fn a_decoder_refuses_every_malformed_reply() {
        let reply = ConsoleReply {
            name: "a".to_owned(),
            value: "1".to_owned(),
            outcome: ConsoleOutcome::Refused("no".to_owned()),
        };
        let good = encode_console_reply(&reply).expect("short enough");
        for len in 0..good.len() {
            assert!(decode_console_reply(&good[..len]).is_err(), "{len} bytes");
        }
        let mut outcome = good.clone();
        outcome[1] = 7;
        assert!(matches!(
            decode_console_reply(&outcome),
            Err(DecodeError::UnknownTag { tag: 7 })
        ));
        let mut tag = good;
        tag[0] = 0x11;
        assert!(matches!(
            decode_console_reply(&tag),
            Err(DecodeError::UnknownTag { tag: 0x11 })
        ));
    }
}
