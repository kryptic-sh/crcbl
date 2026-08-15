//! The reply stream: the answers JS hands back, JS → wasm.
//!
//! Some HAL calls need an answer the caller cannot be handed synchronously —
//! adapter enumeration, the device-request poll,
//! [`poll_readback`](crcbl_hal::Device::poll_readback),
//! [`query_results`](crcbl_hal::Device::query_results). The command stream
//! cannot carry one: it is written during a frame and replayed when the frame
//! ends, and by then the call has long since returned. **This is the channel
//! that carries the answer back**, and it is the same format read the other way:
//! a magic and version header, a tag byte per reply, `u32` length prefixes, the
//! same caps, and the same bounds-checked reader, shared with the command
//! stream rather than written twice.
//!
//! # Every reply names the command it answers
//!
//! By the sequence number [`StreamWriter`](crate::StreamWriter) assigned that
//! command, as a `u64` field after the tag. This is the one place the reply
//! stream deliberately departs from the command stream, which keeps its sequence
//! numbers *off* the wire because they are positional. A reply's position says
//! nothing: JS answers what it can, when it can, so replies need not arrive in
//! order, in one buffer, or at all.
//!
//! A reply for a sequence nothing is waiting on is
//! [`DecodeError::UnexpectedSequence`],
//! reported rather than dropped — see
//! [`StreamChannel::drain_replies`](crate::web::StreamChannel::drain_replies).
//! That check lives with the waiting set in [`web`](crate::web), because it is
//! about *state*; everything in this module is a pure decode.
//!
//! # This is not the production encoder
//!
//! JS is. Replies are written by `web/engine/gpu-reply.js` in a browser, and
//! [`ReplyWriter`] exists for the reason [`reader`](crate::reader) exists in the
//! other direction: so the encoding is testable without a browser, and so the
//! committed fixture the JS writer is held to can be produced by `cargo test`.
//! The two halves are pinned to each other through
//! `crates/crcbl-webgpu/tests/fixtures/canonical-replies.bin`.
//!
//! # What is encoded, and what is not
//!
//! **A partial set**, one reply per *encoding shape* rather than one per HAL
//! method that needs an answer:
//!
//! | Shape | Reply |
//! | --- | --- |
//! | a handle and nothing else | [`Reply::ReadbackPending`] |
//! | a handle and an unbounded byte payload | [`Reply::ReadbackReady`] |
//! | a scalar and a string | [`Reply::Adapter`] |
//! | a counted array of fixed-size elements | [`Reply::QueryResults`] |
//!
//! Not here, and needed before the HAL can be implemented over this channel: the
//! device-request poll (a [`DeviceRequestState`](crcbl_hal::DeviceRequestState)
//! and, on failure, a reason), the rest of
//! [`AdapterInfo`](crcbl_hal::AdapterInfo) — vendor and device ids, driver,
//! backend, and the whole of [`DeviceCaps`](crcbl_hal::DeviceCaps) — and
//! surface capabilities. Each is one of the four shapes above or a composition
//! of them, which is why stopping here was worth doing.

use crcbl_hal::{QuerySetHandle, ReadbackHandle};

use crate::bytes::{ByteReader, ByteWriter, DecodeError};
use crate::tag;

// ── Reply ─────────────────────────────────────────────────────────────────────

/// One decoded reply, with every borrowed field owned.
///
/// The counterpart of [`Command`](crate::Command), and owned for the same
/// reason: the answer outlives the buffer it arrived in — which here is not a
/// figure of speech but the detached-view rule, since that buffer is wasm memory
/// JS wrote into and the next allocation may move it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// One entry of an adapter enumeration.
    ///
    /// A partial [`AdapterInfo`](crcbl_hal::AdapterInfo): the id the engine
    /// passes back in a [`DeviceDesc`](crcbl_hal::DeviceDesc), and the name a
    /// log or a device-selection screen shows.
    Adapter {
        /// Position in the enumeration —
        /// [`AdapterId`](crcbl_hal::AdapterId)'s `u32`, unwrapped, because the
        /// newtype is a compile-time distinction and has no wire form.
        id: u32,
        /// Human-readable device name, as the browser reports it.
        name: String,
    },
    /// [`poll_readback`](crcbl_hal::Device::poll_readback) answering
    /// [`ReadbackState::Pending`](crcbl_hal::ReadbackState::Pending): the bytes
    /// are not there yet, and the caller's `out` is left untouched.
    ReadbackPending {
        /// Which readback.
        readback: ReadbackHandle,
    },
    /// [`poll_readback`](crcbl_hal::Device::poll_readback) answering
    /// [`ReadbackState::Ready`](crcbl_hal::ReadbackState::Ready), with the bytes.
    ///
    /// The length is **the payload's own**, not a promise about the descriptor:
    /// `poll_readback`'s contract is exactly
    /// [`ReadbackDesc::size`](crcbl_hal::ReadbackDesc::size) bytes and a wrong
    /// length is [`HalError::InvalidDescriptor`](crcbl_hal::HalError), so the
    /// implementation over this channel checks the length it got against the
    /// descriptor it kept. Decoding cannot: nothing in the buffer says what the
    /// descriptor asked for.
    ReadbackReady {
        /// Which readback.
        readback: ReadbackHandle,
        /// The bytes read back.
        data: Vec<u8>,
    },
    /// [`query_results`](crcbl_hal::Device::query_results).
    QueryResults {
        /// Which query set.
        set: QuerySetHandle,
        /// The first query the values start at.
        first_query: u32,
        /// Raw values, one per query, in query order.
        values: Vec<u64>,
    },
}

impl Reply {
    /// A stable variant name.
    ///
    /// What a dump prints, and what lets a test assert the *shape* of a buffer
    /// without spelling out every handle. Same role as
    /// [`Command::name`](crate::Command::name).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Adapter { .. } => "Adapter",
            Self::ReadbackPending { .. } => "ReadbackPending",
            Self::ReadbackReady { .. } => "ReadbackReady",
            Self::QueryResults { .. } => "QueryResults",
        }
    }
}

// ── ReplyWriter ───────────────────────────────────────────────────────────────

/// Encodes replies into a buffer. **The reference encoder, not the production
/// one** — see the [module docs](self).
///
/// Every method takes the sequence number of the command it answers, because
/// nothing about a reply's position in the buffer says which command that is.
///
/// # Panics
///
/// Past [`tag::MAX_FIELD_BYTES`] or [`tag::MAX_ELEMENT_COUNT`], which are the
/// caps the reader enforces, so nothing this crate encodes is something it
/// would refuse to decode.
#[derive(Debug)]
pub struct ReplyWriter {
    bytes: ByteWriter,
}

impl Default for ReplyWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplyWriter {
    /// A fresh writer holding nothing but a header.
    #[must_use]
    pub fn new() -> Self {
        let mut writer = Self {
            bytes: ByteWriter::with_capacity(tag::REPLY_HEADER_BYTES),
        };
        writer
            .bytes
            .put_format_header(tag::REPLY_MAGIC, tag::REPLY_VERSION);
        writer
    }

    /// The encoded replies, header included.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }

    /// Drops the encoded replies, keeping the allocation.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.bytes
            .put_format_header(tag::REPLY_MAGIC, tag::REPLY_VERSION);
    }

    /// [`Reply::Adapter`].
    pub fn adapter(&mut self, sequence: u64, id: u32, name: &str) {
        self.open(tag::ADAPTER_REPLY_TAG, sequence);
        self.bytes.put_u32(id);
        self.bytes.put_bytes(name.as_bytes());
    }

    /// [`Reply::ReadbackPending`].
    pub fn readback_pending(&mut self, sequence: u64, readback: ReadbackHandle) {
        self.open(tag::READBACK_PENDING_REPLY_TAG, sequence);
        self.bytes.put_handle(readback);
    }

    /// [`Reply::ReadbackReady`].
    pub fn readback_ready(&mut self, sequence: u64, readback: ReadbackHandle, data: &[u8]) {
        self.open(tag::READBACK_READY_REPLY_TAG, sequence);
        self.bytes.put_handle(readback);
        self.bytes.put_bytes(data);
    }

    /// [`Reply::QueryResults`].
    pub fn query_results(
        &mut self,
        sequence: u64,
        set: QuerySetHandle,
        first_query: u32,
        values: &[u64],
    ) {
        self.open(tag::QUERY_RESULTS_REPLY_TAG, sequence);
        self.bytes.put_handle(set);
        self.bytes.put_u32(first_query);
        self.bytes.put_count(values.len());
        for value in values {
            self.bytes.put_u64(*value);
        }
    }

    /// Opens a reply: its tag, then the sequence it answers.
    fn open(&mut self, tag: u8, sequence: u64) {
        self.bytes.put_u8(tag);
        self.bytes.put_u64(sequence);
    }
}

// ── ReplyReader ───────────────────────────────────────────────────────────────

/// Decodes a reply buffer — in production, one `web/engine/gpu-reply.js` wrote.
///
/// Replies come out one at a time, each with the sequence it answers, so a
/// caller can match them against what it is waiting for as it goes rather than
/// after the fact.
#[derive(Debug)]
pub struct ReplyReader<'a> {
    reader: ByteReader<'a>,
    /// Set once a decode fails. Nothing is resumable after that: the cursor is
    /// somewhere inside a reply body and the next byte is not a tag.
    failed: bool,
}

impl<'a> ReplyReader<'a> {
    /// Opens a reply buffer, checking its magic and version.
    ///
    /// # Errors
    ///
    /// [`DecodeError::BadMagic`] — which is also what a *command* buffer handed
    /// to this reader produces — or [`DecodeError::UnsupportedVersion`], or
    /// [`DecodeError::TooShort`] if there is not a whole header.
    pub fn new(replies: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(replies);
        reader.read_format_header(tag::REPLY_MAGIC, tag::REPLY_VERSION)?;
        Ok(Self {
            reader,
            failed: false,
        })
    }

    /// The next reply and the sequence it answers, or `None` at the end.
    ///
    /// Returns `None` forever after an error, for [`StreamReader`]'s reason: the
    /// cursor is then somewhere inside a body, so the next byte is not a tag and
    /// resuming would invent replies out of a payload.
    ///
    /// [`StreamReader`]: crate::StreamReader
    ///
    /// # Errors
    ///
    /// Any [`DecodeError`] the reply body produces.
    pub fn next_reply(&mut self) -> Option<Result<(u64, Reply), DecodeError>> {
        if self.failed || self.reader.is_empty() {
            return None;
        }
        match self.decode_reply() {
            Ok(pair) => Some(Ok(pair)),
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }

    fn decode_reply(&mut self) -> Result<(u64, Reply), DecodeError> {
        let r = &mut self.reader;
        let opcode = r.read_u8()?;
        let sequence = r.read_u64()?;
        let reply = match opcode {
            tag::ADAPTER_REPLY_TAG => Reply::Adapter {
                id: r.read_u32()?,
                name: r.read_string("Adapter::name")?,
            },
            tag::READBACK_PENDING_REPLY_TAG => Reply::ReadbackPending {
                readback: r.read_handle("ReadbackPending::readback")?,
            },
            tag::READBACK_READY_REPLY_TAG => {
                let readback = r.read_handle("ReadbackReady::readback")?;
                let data = r.read_field("ReadbackReady::data")?.to_vec();
                Reply::ReadbackReady { readback, data }
            }
            tag::QUERY_RESULTS_REPLY_TAG => {
                let set = r.read_handle("QueryResults::set")?;
                let first_query = r.read_u32()?;
                let count = r.read_count("QueryResults::values")?;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(r.read_u64()?);
                }
                Reply::QueryResults {
                    set,
                    first_query,
                    values,
                }
            }
            unknown => return Err(DecodeError::UnknownTag { tag: unknown }),
        };
        Ok((sequence, reply))
    }
}

/// Every reply in a buffer, in order, each with the sequence it answers.
///
/// The convenience half of [`ReplyReader`], for a test or a dump that wants the
/// whole buffer at once. **It does not check that anything was waiting** on
/// those sequences; that is
/// [`StreamChannel::drain_replies`](crate::web::StreamChannel::drain_replies),
/// which is where the waiting set lives.
///
/// # Errors
///
/// The first [`DecodeError`] the buffer produces; nothing after it is decoded.
pub fn decode_replies(replies: &[u8]) -> Result<Vec<(u64, Reply)>, DecodeError> {
    let mut reader = ReplyReader::new(replies)?;
    let mut decoded = Vec::new();
    while let Some(next) = reader.next_reply() {
        decoded.push(next?);
    }
    Ok(decoded)
}
