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
//! | counted arrays of enum codes, plus an optional non-handle field | [`Reply::SurfaceCaps`] |
//!
//! [`Reply::NoAdapter`] was the first addition this set took, and it is not a
//! new shape but a new *fact*: an enumeration is answered exactly once, so "the
//! browser granted nothing" has nowhere to live inside [`Reply::Adapter`]. Its
//! own docs carry the argument. [`Reply::Device`] and [`Reply::DeviceFailed`]
//! are the second, and the same two shapes again — the capability half of an
//! adapter record, and a reason with the machine-readable half of it beside the
//! string.
//!
//! [`Reply::SurfaceCaps`] is the third addition, and the first that brought a
//! shape rather than only a fact: a counted array whose elements are *enum
//! codes* rather than fixed-width scalars, and the seam's first optional field
//! that is neither a handle nor a string. Both are settled in this module's own
//! `put_surface_caps` and in [`tag`]'s three new code tables.
//!
//! Not here, and needed before the HAL can be implemented over this channel:
//! the *failure* half of a surface-capability query.
//! [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps) answers
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) for an adapter
//! that cannot present to the surface, which is
//! [`Reply::DeviceFailed`]'s shape again and is not encoded yet.
//!
//! # There is no "still pending" reply, and there must not be
//!
//! [`DeviceRequestState::Pending`](crcbl_hal::DeviceRequestState::Pending) is
//! the *absence* of a reply, not a reply. `requestDevice` settles once, so a
//! per-frame "not yet" would be one reply per frame naming a sequence that is
//! answered exactly once — which the channel refuses, whole buffer at a time,
//! as [`DecodeError::UnexpectedSequence`]. The waiting frames are the ones with
//! nothing in the buffer for that sequence, and [`crate::device`] is what turns
//! that silence into `Pending`.
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
    ReadbackHandle, SurfaceCaps,
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
        self.put_features(caps.features);
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

    /// A counted list of enum codes: the count, then one byte per element.
    ///
    /// One helper for the three lists [`SurfaceCaps`] carries rather than three
    /// loops, because they are the same knowledge and not merely the same shape
    /// — the count's cap, the element width and the ordering rule are one
    /// decision, and a fix to any of them has to land in all three.
    fn put_enum_list<T: Copy>(&mut self, items: &[T], code: fn(T) -> u8) {
        self.put_count(items.len());
        for item in items {
            self.put_u8(code(*item));
        }
    }

    /// [`SurfaceCaps`], field by field in declaration order.
    ///
    /// Every field crosses. **The three lists keep their order**, which is not
    /// decoration: [`formats`](SurfaceCaps::formats) is documented as best
    /// first and [`preferred_format`](SurfaceCaps::preferred_format) reads it
    /// that way, so a decoder that rebuilt the list in any other order would
    /// change which format a swapchain is created with.
    ///
    /// **[`current_extent`](SurfaceCaps::current_extent) gets a presence byte**,
    /// which is the house rule for every optional field that is not a handle.
    /// The zero-generation niche that spares `Option<Handle>` one does not apply
    /// here — a pair of `u32`s has no niche — and no sentinel could stand in for
    /// it either: `(0, 0)` is a size a window system can report for an
    /// unconfigured or minimised window, and `(0xFFFF_FFFF, 0xFFFF_FFFF)` is
    /// the Vulkan spelling of "no opinion" that
    /// [`SurfaceCaps::current_extent`]'s own docs say a backend must map to
    /// `None` rather than let escape into the seam. So absent is one byte with
    /// nothing behind it and present is one byte with eight, and the reader
    /// refuses any third value of that byte rather than reading it as truthy —
    /// which is what makes the two impossible to confuse in either direction.
    fn put_surface_caps(&mut self, caps: &SurfaceCaps) {
        self.put_enum_list(&caps.formats, tag::format_code);
        self.put_enum_list(&caps.present_modes, tag::present_mode_code);
        self.put_enum_list(&caps.composite_alpha, tag::composite_alpha_code);
        self.put_u32(caps.min_image_count);
        self.put_u32(caps.max_image_count);
        match caps.current_extent {
            None => self.put_u8(tag::ABSENT),
            Some((width, height)) => {
                self.put_u8(tag::PRESENT);
                self.put_u32(width);
                self.put_u32(height);
            }
        }
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
        Ok(DeviceCaps {
            features: self.read_features("DeviceCaps::features")?,
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

    /// The counterpart of [`ByteWriter::put_enum_list`], bounded by
    /// [`read_count`](Self::read_count) — the cap the whole format uses for
    /// element counts, not a second one invented for these three lists.
    ///
    /// A code `from_code` does not claim is [`DecodeError::InvalidEnum`] naming
    /// the list, never the nearest variant: see
    /// [`tag::format_from_code`] for what a plausible neighbour would cost.
    fn read_enum_list<T>(
        &mut self,
        field: &'static str,
        from_code: fn(u8) -> Option<T>,
    ) -> Result<Vec<T>, DecodeError> {
        let count = self.read_count(field)?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let code = self.read_u8()?;
            items.push(from_code(code).ok_or(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            })?);
        }
        Ok(items)
    }

    /// The counterpart of [`ByteWriter::put_surface_caps`].
    fn read_surface_caps(&mut self) -> Result<SurfaceCaps, DecodeError> {
        let formats = self.read_enum_list("SurfaceCaps::formats", tag::format_from_code)?;
        let present_modes =
            self.read_enum_list("SurfaceCaps::present_modes", tag::present_mode_from_code)?;
        let composite_alpha = self.read_enum_list(
            "SurfaceCaps::composite_alpha",
            tag::composite_alpha_from_code,
        )?;
        let min_image_count = self.read_u32()?;
        let max_image_count = self.read_u32()?;
        // Spelled out rather than `Some((self.read_u32()?, self.read_u32()?))`:
        // the two halves are read in source order either way, but relying on
        // that to get width-before-height right reads as a coincidence — the
        // same reason `Draw`'s two ranges are spelled out in `crate::reader`.
        let current_extent = if self.read_present("SurfaceCaps::current_extent")? {
            let width = self.read_u32()?;
            let height = self.read_u32()?;
            Some((width, height))
        } else {
            None
        };
        Ok(SurfaceCaps {
            formats,
            present_modes,
            composite_alpha,
            min_image_count,
            max_image_count,
            current_extent,
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
    /// The browser opened a device, with what *that device* can do.
    ///
    /// **Not the adapter's capabilities**, and the distinction is the whole
    /// point of a separate reply rather than an "ok" flag on
    /// [`Reply::Adapter`]. WebGPU grants a device the features that were asked
    /// for and no others, and its limits are the ones that were requested —
    /// which are the specification's defaults when a request names none — so an
    /// adapter reporting `timestamp-query` and a 16384-pixel texture yields a
    /// device with neither unless the request said so. A backend that reported
    /// the adapter's [`DeviceCaps`] for its device would select render paths
    /// against capabilities the device does not have.
    ///
    /// There is no handle: the [`Device`](crcbl_hal::Device) lives on the far
    /// side for its whole life, and this crate's command set already names one
    /// device implicitly — [`Command::CreateBuffer`](crate::Command::CreateBuffer)
    /// carries no device id either. The side table that stamps object ownership
    /// arrives with the second device, as
    /// `docs/plan/41-webgpu-stream.md` says it must.
    Device {
        /// What the device the browser opened can do.
        caps: DeviceCaps,
    },
    /// The device request failed, with the reason and the gap that caused it.
    ///
    /// **This is the request failing, not a device being lost.** The two are
    /// different events with different lifetimes: this one answers a
    /// [`Command::RequestDevice`](crate::Command::RequestDevice) that never
    /// produced a device, and it is what
    /// [`PendingDevice::poll`](crcbl_hal::PendingDevice::poll) would report as
    /// an `Err`. A device *lost* later — WebGPU's `GPUDevice.lost`, or an
    /// `uncapturederror` on a device that is open and working — belongs to
    /// [`Device::take_error`](crcbl_hal::Device::take_error) and to a reply this
    /// slice does not have, because nothing here holds a device long enough to
    /// lose one.
    DeviceFailed {
        /// What the browser said, or what the replayer refused to ask for. For
        /// a log or a banner; never a code to branch on.
        reason: String,
        /// Which of the requested features could not be satisfied, or empty
        /// when the failure was not about features.
        ///
        /// The machine-readable half of `reason`, and the field an
        /// `impl Instance` would turn into
        /// [`HalError::UnsupportedFeatures`](crcbl_hal::HalError::UnsupportedFeatures).
        /// It is a [`Features`] word rather than a phrase inside the string
        /// because the names belong to this side: `crcbl-hal` spells them, the
        /// replayer only knows bits and `GPUFeatureName`s.
        unsupported: Features,
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
    /// [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps): what the
    /// surface will accept on the adapter the command named.
    ///
    /// **The whole of [`SurfaceCaps`]**, in declaration order — three counted
    /// lists of enum codes, two counts, and an optional extent. Nothing is
    /// dropped on the grounds that a browser cannot fill it, for
    /// [`Reply::Adapter`]'s reason: the decoder decodes what the wire says, so
    /// the same reply written by something that *can* fill a field decodes as
    /// filled.
    ///
    /// The three lists keep the order they were written in.
    /// [`formats`](SurfaceCaps::formats) is documented as best first and
    /// [`preferred_format`](SurfaceCaps::preferred_format) reads it that way, so
    /// the order is a value rather than a presentation detail.
    ///
    /// **There is no failure arm yet**, and the gap is deliberate rather than
    /// overlooked: `surface_caps` answers
    /// [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) for an
    /// adapter that cannot present to the surface, and that is a second reply —
    /// the shape [`Reply::DeviceFailed`] has — which nothing on this side can
    /// wait for until an `impl Instance` exists to ask.
    SurfaceCaps {
        /// What the surface will accept.
        caps: SurfaceCaps,
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
            Self::Device { .. } => "Device",
            Self::DeviceFailed { .. } => "DeviceFailed",
            Self::SurfaceCaps { .. } => "SurfaceCaps",
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

    /// [`Reply::Device`].
    ///
    /// `caps` are the *device's*, which are not the adapter's — see
    /// [`Reply::Device`].
    pub fn device(&mut self, sequence: u64, caps: &DeviceCaps) {
        self.open(tag::DEVICE_REPLY_TAG, sequence);
        self.bytes.put_device_caps(caps);
    }

    /// [`Reply::DeviceFailed`].
    pub fn device_failed(&mut self, sequence: u64, reason: &str, unsupported: Features) {
        self.open(tag::DEVICE_FAILED_REPLY_TAG, sequence);
        self.bytes.put_bytes(reason.as_bytes());
        self.bytes.put_u64(unsupported.bits());
    }

    /// [`Reply::SurfaceCaps`].
    ///
    /// Taken whole rather than field by field, for
    /// [`adapter`](Self::adapter)'s reason: six positional parameters, three of
    /// them lists, is how a caller swaps two of them.
    pub fn surface_caps(&mut self, sequence: u64, caps: &SurfaceCaps) {
        self.open(tag::SURFACE_CAPS_REPLY_TAG, sequence);
        self.bytes.put_surface_caps(caps);
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
            tag::DEVICE_REPLY_TAG => Reply::Device {
                caps: r.read_device_caps()?,
            },
            tag::DEVICE_FAILED_REPLY_TAG => {
                let reason = r.read_string("DeviceFailed::reason")?;
                let unsupported = r.read_features("DeviceFailed::unsupported")?;
                Reply::DeviceFailed {
                    reason,
                    unsupported,
                }
            }
            tag::SURFACE_CAPS_REPLY_TAG => Reply::SurfaceCaps {
                caps: r.read_surface_caps()?,
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
