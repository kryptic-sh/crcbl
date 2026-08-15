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
//! | a flat record of scalars, strings, an enum code and a bitflags word | [`Reply::Adapter`] |
//! | a string alone | [`Reply::NoAdapter`] |
//! | a counted array of fixed-size elements | [`Reply::QueryResults`] |
//!
//! [`Reply::NoAdapter`] is the one addition this set has taken since it was
//! written, and it is not a new shape but a new *fact*: an enumeration is
//! answered exactly once, so "the browser granted nothing" has nowhere to live
//! inside [`Reply::Adapter`]. Its own docs carry the argument.
//!
//! Not here, and needed before the HAL can be implemented over this channel: the
//! device-request poll (a [`DeviceRequestState`](crcbl_hal::DeviceRequestState)
//! and, on failure, a reason) and surface capabilities. Both are compositions of
//! the shapes above.
//!
//! # What an adapter reply carries, and what the browser cannot tell it
//!
//! [`Reply::Adapter`] is the whole of [`AdapterInfo`]
//! bar one field, in declaration order with
//! [`caps`](crcbl_hal::AdapterInfo::caps) expanded in place: `id`, `name`,
//! `vendor_id`, `device_id`, `device_type`, `driver`, then
//! [`DeviceCaps::features`](crcbl_hal::DeviceCaps::features) as a `u64` of
//! [`Features::bits`](crcbl_hal::Features::bits) and every field of
//! [`Limits`], also in declaration order. Stating the *rule*
//! rather than a list is deliberate: two hand-written codecs agreeing on
//! "declaration order, `backend` omitted" is one fact to check, where a copied
//! list is nineteen.
//!
//! **[`backend`](crcbl_hal::AdapterInfo::backend) is the field that is not on
//! the wire.** It is not a fact about the adapter — it says which crate
//! enumerated it — so it is answered by the half that knows, which is this one:
//! the decoder writes [`BackendKind::WebGpu`]
//! because this crate is what decoded. Carrying it would let a replayer claim to
//! be Vulkan and be believed.
//!
//! **Three fields a browser cannot fill, and what the replayer puts there.**
//! WebGPU's `GPUAdapterInfo` is four strings and nothing else; it has no numeric
//! ids and does not say what class of device it found. So
//! `web/engine/gpu-replay.js` writes the values that *mean absent* rather than
//! values that look real:
//!
//! | Field | Value | Why there is nothing better |
//! | --- | --- | --- |
//! | `vendor_id` | `0`, which [`AdapterInfo`] documents as "unknown" | `GPUAdapterInfo.vendor` is a *string* like `"apple"`; deriving a PCI id from it would be an invention indistinguishable downstream from a real one |
//! | `device_id` | `0`, likewise | there is no numeric device id anywhere in WebGPU |
//! | `device_type` | [`DeviceType::Other`](crcbl_hal::DeviceType::Other) — "the backend declined to say" | WebGPU deliberately does not report discrete-versus-integrated. `GPUAdapter.isFallbackAdapter` is the nearest thing and is not the same claim: it grades *performance*, not device class, so mapping it to `Cpu` would be a guess |
//! | `driver` | the empty string | `GPUAdapterInfo` has no driver name or version. Empty is the absence, not a driver called `""` |
//!
//! The decode side does not know any of that and must not: it decodes whatever
//! the wire says, so the same reply written by something that *can* fill those
//! fields decodes as filled.

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceCaps, Features, Limits, QuerySetHandle,
    ReadbackHandle,
};

use crate::bytes::{ByteReader, ByteWriter, DecodeError};
use crate::tag;

// ── The reply stream's own field writers ──────────────────────────────────────
//
// [`ByteWriter`] and its primitives live in [`crate::bytes`], shared with the
// command direction; these are shaped by `crcbl-hal`'s capability types and
// belong to the reply stream alone — the same split [`crate::writer`] makes for
// the descriptors. Each is the exact counterpart of a `read_*` below.

impl ByteWriter {
    /// [`Limits`], field by field in declaration order.
    ///
    /// Every field, not a subset the engine happens to read today: the fields
    /// are read from all over — `crcbl-render` reads
    /// `optimal_buffer_copy_offset_alignment`, `max_image_2d`,
    /// `min_uniform_buffer_offset_alignment` and `timestamp_period_ns`, while
    /// `crcbl-hal`'s own descriptor validation reads `max_sample_count`,
    /// `max_compute_workgroup_size`, `max_bindless_descriptors`,
    /// `max_image_array_layers`, `max_color_attachments`, `max_bind_groups`,
    /// `max_push_constant_size`, `max_sampler_anisotropy` and
    /// `max_compute_invocations_per_workgroup` — and a field left off the wire
    /// is one the decoder would have to invent a ceiling for.
    fn put_limits(&mut self, limits: &Limits) {
        self.put_u32(limits.max_image_2d);
        self.put_u32(limits.max_image_3d);
        self.put_u32(limits.max_image_array_layers);
        self.put_u64(limits.max_storage_buffer_range);
        self.put_u64(limits.max_uniform_buffer_range);
        self.put_u32(limits.max_bind_groups);
        self.put_u32(limits.max_bindless_descriptors);
        self.put_u32(limits.max_push_constant_size);
        self.put_u32(limits.max_color_attachments);
        self.put_u32(limits.max_sample_count);
        self.put_u32(limits.max_draw_indirect_count);
        for axis in limits.max_compute_workgroup_size {
            self.put_u32(axis);
        }
        self.put_u32(limits.max_compute_invocations_per_workgroup);
        self.put_u32(limits.max_compute_workgroups_per_dimension);
        self.put_u64(limits.min_uniform_buffer_offset_alignment);
        self.put_u64(limits.min_storage_buffer_offset_alignment);
        self.put_u64(limits.optimal_buffer_copy_offset_alignment);
        self.put_f32(limits.max_sampler_anisotropy);
        self.put_f32(limits.timestamp_period_ns);
    }

    /// [`DeviceCaps`]: the feature bits, then the limits.
    ///
    /// [`Features`] goes over as `bits()` and comes back through `from_bits`,
    /// which is the house rule for bitflags and the reason they are exempt from
    /// the "tags are ours, not the compiler's" rule: each flag is an explicit
    /// `1 << n`, so the value is already chosen rather than positional.
    /// Truncating would silently drop a bit the other half meant.
    fn put_device_caps(&mut self, caps: &DeviceCaps) {
        self.put_u64(caps.features.bits());
        self.put_limits(&caps.limits);
    }

    /// [`AdapterInfo`] in declaration order, `backend` omitted and `caps`
    /// expanded — see the [module docs](self).
    fn put_adapter_info(&mut self, info: &AdapterInfo) {
        self.put_u32(info.id.0);
        self.put_bytes(info.name.as_bytes());
        self.put_u32(info.vendor_id);
        self.put_u32(info.device_id);
        self.put_u8(tag::device_type_code(info.device_type));
        self.put_bytes(info.driver.as_bytes());
        self.put_device_caps(&info.caps);
    }
}

impl ByteReader<'_> {
    /// The counterpart of [`ByteWriter::put_limits`].
    fn read_limits(&mut self) -> Result<Limits, DecodeError> {
        Ok(Limits {
            max_image_2d: self.read_u32()?,
            max_image_3d: self.read_u32()?,
            max_image_array_layers: self.read_u32()?,
            max_storage_buffer_range: self.read_u64()?,
            max_uniform_buffer_range: self.read_u64()?,
            max_bind_groups: self.read_u32()?,
            max_bindless_descriptors: self.read_u32()?,
            max_push_constant_size: self.read_u32()?,
            max_color_attachments: self.read_u32()?,
            max_sample_count: self.read_u32()?,
            max_draw_indirect_count: self.read_u32()?,
            max_compute_workgroup_size: [self.read_u32()?, self.read_u32()?, self.read_u32()?],
            max_compute_invocations_per_workgroup: self.read_u32()?,
            max_compute_workgroups_per_dimension: self.read_u32()?,
            min_uniform_buffer_offset_alignment: self.read_u64()?,
            min_storage_buffer_offset_alignment: self.read_u64()?,
            optimal_buffer_copy_offset_alignment: self.read_u64()?,
            max_sampler_anisotropy: self.read_f32()?,
            timestamp_period_ns: self.read_f32()?,
        })
    }

    /// The counterpart of [`ByteWriter::put_device_caps`].
    ///
    /// A bit no [`Features`] flag claims is [`DecodeError::InvalidEnum`], never
    /// a truncation: `from_bits_truncate` would accept a word from a newer build
    /// and silently report a lesser device.
    fn read_device_caps(&mut self) -> Result<DeviceCaps, DecodeError> {
        let bits = self.read_u64()?;
        let features = Features::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field: "DeviceCaps::features",
            code: bits,
        })?;
        Ok(DeviceCaps {
            features,
            limits: self.read_limits()?,
        })
    }

    /// The counterpart of [`ByteWriter::put_adapter_info`].
    ///
    /// [`AdapterInfo::backend`] is not read because it is not written: it names
    /// the crate that decoded, which is this one.
    fn read_adapter_info(&mut self) -> Result<AdapterInfo, DecodeError> {
        let id = AdapterId(self.read_u32()?);
        let name = self.read_string("Adapter::name")?;
        let vendor_id = self.read_u32()?;
        let device_id = self.read_u32()?;
        let device_type_code = self.read_u8()?;
        let device_type =
            tag::device_type_from_code(device_type_code).ok_or(DecodeError::InvalidEnum {
                field: "Adapter::device_type",
                code: device_type_code.into(),
            })?;
        let driver = self.read_string("Adapter::driver")?;
        Ok(AdapterInfo {
            id,
            name,
            vendor_id,
            device_id,
            device_type,
            driver,
            backend: BackendKind::WebGpu,
            caps: self.read_device_caps()?,
        })
    }
}

// ── Reply ─────────────────────────────────────────────────────────────────────

/// One decoded reply, with every borrowed field owned.
///
/// The counterpart of [`Command`](crate::Command), and owned for the same
/// reason: the answer outlives the buffer it arrived in — which here is not a
/// figure of speech but the detached-view rule, since that buffer is wasm memory
/// JS wrote into and the next allocation may move it.
/// **Not [`Eq`]**, and it stopped being so when [`Reply::Adapter`] grew its
/// [`Limits`], two of whose fields are `f32`. Nothing in this crate needs total
/// equality, and deriving it would have meant either an ordering on floats that
/// does not exist or a hand-written `Eq` asserting one.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// One entry of an adapter enumeration: the whole of
    /// [`AdapterInfo`].
    ///
    /// Everything a caller needs to pick an adapter without paying for a device
    /// — the id it passes back in a [`DeviceDesc`](crcbl_hal::DeviceDesc), the
    /// name a log or a selection screen shows, and the
    /// [`DeviceCaps`] every renderer path is selected
    /// from.
    ///
    /// **[`backend`](crcbl_hal::AdapterInfo::backend) did not cross the wire**
    /// and is always [`BackendKind::WebGpu`]
    /// here; the three fields WebGPU cannot supply arrive as the values that
    /// mean absent. The [module docs](self) carry both lists.
    Adapter {
        /// What the browser said about the adapter it granted.
        info: AdapterInfo,
    },
    /// The enumeration found nothing, with the reason the browser gave.
    ///
    /// **The terminator [`Reply::Adapter`] cannot be**, and the one place this
    /// slice extended the reply set rather than reusing it. One command is
    /// answered exactly once — a second reply naming a sequence already answered
    /// is [`DecodeError::UnexpectedSequence`], the same as a reply for a
    /// sequence nobody asked — so an enumeration is *one* reply, and there is no
    /// value of `id` or `name` that means "no adapters": an empty name is a
    /// browser that declined to name a real adapter, which the canonical corpus
    /// carries.
    ///
    /// Reachable on a machine that has WebGPU and no GPU to run it on, which is
    /// not a corner: `navigator.gpu.requestAdapter()` resolves `null` for a
    /// blocklisted driver or a session with no GPU, and the demo shim already
    /// has a sentence for it.
    NoAdapter {
        /// What the browser said, for a log or a banner. Never a code to branch
        /// on: it is a message from another vendor's runtime.
        reason: String,
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
            Self::NoAdapter { .. } => "NoAdapter",
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
    ///
    /// **[`info.backend`](crcbl_hal::AdapterInfo::backend) is not written**, and
    /// the argument is taken whole rather than field by field because eight
    /// positional parameters is how a caller swaps two of them. A reply decoded
    /// from these bytes always names
    /// [`BackendKind::WebGpu`], whatever was
    /// passed here — see the [module docs](self) for why that field belongs to
    /// the decoder.
    pub fn adapter(&mut self, sequence: u64, info: &AdapterInfo) {
        self.open(tag::ADAPTER_REPLY_TAG, sequence);
        self.bytes.put_adapter_info(info);
    }

    /// [`Reply::NoAdapter`].
    pub fn no_adapter(&mut self, sequence: u64, reason: &str) {
        self.open(tag::NO_ADAPTER_REPLY_TAG, sequence);
        self.bytes.put_bytes(reason.as_bytes());
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
                info: r.read_adapter_info()?,
            },
            tag::NO_ADAPTER_REPLY_TAG => Reply::NoAdapter {
                reason: r.read_string("NoAdapter::reason")?,
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
