//! [`MetalDevice`]: one `MTLDevice`, its queue, and the four resource tables
//! this slice fills.
//!
//! # What this slice implements, and what it still refuses
//!
//! Buffers, images, image views and samplers — created, destroyed, and looked
//! up through generational handles — plus [`Device::backend`],
//! [`Device::caps`], [`Device::queue`], [`Device::write_buffer`] and
//! [`Device::wait_idle`]. Everything else on the trait refuses with
//! [`HalError::Unsupported`] whose `what` names the slice it arrives in, in the
//! same voice `MetalInstance` established. Nothing here is a stub that reports
//! success.
//!
//! # One lock over every table
//!
//! The seam takes `&self` everywhere so a device can be shared behind an `Arc`,
//! which means a backend owes its own interior synchronisation. This one uses a
//! single [`Mutex`] over all four pools, which is the same call the Vulkan
//! backend made and for the same reason: the traffic is a few dozen operations
//! per frame, and a lock-per-table scheme has a deadlock-ordering problem to
//! design before it has a contention problem to solve.
//!
//! # There is no deletion queue here, and that is not an oversight
//!
//! `crcbl-vk` parks every destroyed object on a timeline-keyed retire queue,
//! because `vkDestroyBuffer` while the GPU is reading is undefined behaviour.
//! Metal does not have that hazard: an `MTLCommandBuffer` retains every
//! resource it references, so releasing the last handle to a buffer the GPU is
//! still reading frees it *after* the command buffer completes, not during.
//! `destroy_*` therefore drops the `Retained` and is done. When the command
//! slice adds `commandBufferWithUnretainedReferences` — the opt-out — it
//! reintroduces the hazard and must reintroduce the queue with it.

use std::sync::{Arc, Mutex, MutexGuard};

use crcbl_core::{Handle, Pool};
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, BufferUsage,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePipelineDesc,
    ComputePipelineHandle, Device, DeviceCaps, DeviceDesc, Features, Format, GraphicsPipelineDesc,
    GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageType, ImageViewDesc,
    ImageViewHandle, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo,
    QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle,
    ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString, NSUInteger};
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue, MTLDevice, MTLResource,
    MTLSamplerDescriptor, MTLSamplerState, MTLTexture, MTLTextureDescriptor,
};

use crate::command::MetalCommandEncoder;
use crate::conv;
use crate::instance::{AdapterRecord, InstanceInner, next_owner_id};

/// Largest anisotropy `MTLSamplerDescriptor::maxAnisotropy` accepts.
///
/// Fixed by the Metal API rather than by any device — the property is
/// documented as taking a value in `1...16` and Metal raises outside it — which
/// is why it is a constant here and why
/// [`Limits::max_sampler_anisotropy`](crcbl_hal::Limits::max_sampler_anisotropy)
/// reports it unconditionally.
pub(crate) const MAX_SAMPLER_ANISOTROPY: f32 = 16.0;

/// Anything the object tables hold, so one lookup helper serves them all.
pub(crate) trait Owned {
    /// Id of the device that created it, per `crcbl-hal`'s obligation 3.
    fn owner(&self) -> u64;
}

macro_rules! owned {
    ($($ty:ty),+ $(,)?) => {
        $(impl Owned for $ty {
            fn owner(&self) -> u64 {
                self.owner
            }
        })+
    };
}

/// A buffer, its size and where its memory lives.
///
/// `location` is kept rather than re-derived from `raw.storageMode()` because
/// the seam's three locations do not round-trip through Metal's two storage
/// modes — `HostUpload` and `HostReadback` are both `Shared` — and
/// `request_readback` will need to tell them apart.
#[derive(Debug)]
struct BufferEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    size: u64,
    location: MemoryLocation,
}

/// A texture, plus the seam-side format it was created with.
///
/// The format is kept because `create_image_view` needs to compare against it,
/// and comparing `MTLPixelFormat`s would answer a subtly different question:
/// two seam formats never share a Metal format (`conv`'s injectivity test), but
/// the reverse direction is what the view check is about.
#[derive(Debug)]
struct ImageEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLTexture>>,
    format: Format,
}

/// A texture view.
#[derive(Debug)]
struct ViewEntry {
    owner: u64,
    /// Held to keep the view alive for as long as its handle resolves. Nothing
    /// in this slice reads it back: the first reader is the bind-group slice,
    /// which binds it, and the render-pass slice, which attaches it.
    #[allow(dead_code)]
    raw: Retained<ProtocolObject<dyn MTLTexture>>,
}

/// A sampler state.
#[derive(Debug)]
struct SamplerEntry {
    owner: u64,
    /// Held to keep the sampler alive; see [`ViewEntry::raw`].
    #[allow(dead_code)]
    raw: Retained<ProtocolObject<dyn MTLSamplerState>>,
}

owned!(BufferEntry, ImageEntry, ViewEntry, SamplerEntry);

/// Every table the device owns, behind one lock.
#[derive(Debug, Default)]
struct DeviceState {
    buffers: Pool<BufferEntry>,
    images: Pool<ImageEntry>,
    views: Pool<ViewEntry>,
    samplers: Pool<SamplerEntry>,
}

/// The device's shared state.
struct DeviceInner {
    /// Obligation 1: a `Device` may outlive its `Instance`, so the instance's
    /// state — on Metal, the enumerated `MTLDevice` objects — is kept alive
    /// here rather than borrowed. See [`InstanceInner`].
    _instance: Arc<InstanceInner>,
    raw: Retained<ProtocolObject<dyn MTLDevice>>,
    /// The one queue. Metal has a single `MTLCommandQueue` type and no queue
    /// families, which is exactly why the seam's enum is named
    /// [`QueueKind`] rather than `QueueFamily`.
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    caps: DeviceCaps,
    id: u64,
    /// This device's stamp on every handle it issues; see the handle-tagging
    /// section below. Never zero.
    tag: u32,
    state: Mutex<DeviceState>,
}

// SAFETY: this is the marker impl the crate docs used to say a device slice
// would not need, and the reason it does is narrower than "Objective-C is not
// thread-safe".
//
// `MTLDevice`, `MTLCommandQueue` and `MTLSamplerState` are all declared
// `NSObjectProtocol + Send + Sync` in `objc2-metal`, so those three fields
// carry the markers themselves and are not why this impl exists. `MTLBuffer`
// and `MTLTexture` are not: they inherit from `MTLResource`, which objc2 leaves
// unmarked because `MTLBuffer::contents` hands out a raw pointer into the
// allocation and a binding cannot know what a user will do with it.
//
// This backend can answer that, which is what makes the assertion sound rather
// than optimistic:
//
// * Every `MTLBuffer` and `MTLTexture` lives inside `state`, so every access to
//   one is already under the `Mutex`.
// * The only use of `contents` outside the tests is `write_buffer`, which copies
//   into the pointer while holding that same lock and never lets it escape.
//   (`tests::read_back` is the other caller and takes the same lock; it exists
//   because reading the bytes is the only way to observe that `write_buffer`
//   wrote anything.) There is no persistent mapping handed across the seam — the
//   seam has no shape for one, which is the argument `Device::write_buffer`
//   makes for being a copy.
// * Retain and release are atomic in the Objective-C runtime, so moving a
//   `Retained` between threads and dropping it on another is sound on its own.
//
// Apple documents `MTLDevice` and the objects created from it as safe to use
// from multiple threads; the two impls below narrow that to the accesses this
// crate actually performs.
unsafe impl Send for DeviceInner {}
// SAFETY: as above.
unsafe impl Sync for DeviceInner {}

impl core::fmt::Debug for DeviceInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceInner")
            .field("id", &self.id)
            .field("tier", &self.caps.tier())
            .finish_non_exhaustive()
    }
}

// --- handle tagging --------------------------------------------------------
//
// **Every object table here is per-device**, and every insert stamps
// `owner: self.id` — which on its own makes `HalError::ForeignObject`
// unproducible rather than satisfied. Two devices allocating in step reach the
// same slot index at the same generation immediately, so device A's first
// buffer handle resolves inside device B's pool to *B's* first buffer, whose
// owner is B: the check passes and B writes into the wrong object. That is
// obligation 3 met by accident and only while the two pools happen to differ.
//
// So the handle carries the issuing device. The top byte of the index half is
// the device's tag; the rest is the pool's own index, restored before any
// lookup. The generation half is untouched, so `Pool`'s generation-exhaustion
// rule is unaffected. `crcbl-vk` uses the same scheme, arrived at from the same
// bug.
//
// The residual hole is stated rather than hidden: tags repeat every
// `DEVICE_TAG_COUNT` devices, so the 256th device in a process shares a tag
// with the first and falls back to the owner check, which cannot separate them.
// A process that opens hundreds of Metal devices has a different problem.

/// Bits of a handle's index half given over to the owning device's tag.
const DEVICE_TAG_SHIFT: u32 = 24;
/// The part of a handle's index half that is the pool's own index.
const POOL_INDEX_MASK: u32 = (1 << DEVICE_TAG_SHIFT) - 1;
/// How many distinct device tags exist. Tag `0` is reserved for "nobody", so a
/// hand-made or un-stamped handle is foreign to every device.
const DEVICE_TAG_COUNT: u64 = (u32::MAX >> DEVICE_TAG_SHIFT) as u64;

/// The tag a device with this owner id stamps into its handles. Never zero.
fn device_tag(id: u64) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        1 + (id % DEVICE_TAG_COUNT) as u32
    }
}

/// The device tag a handle carries, or `0` if it carries none.
const fn handle_tag<M>(handle: Handle<M>) -> u32 {
    handle.index() >> DEVICE_TAG_SHIFT
}

/// Strips the device tag, recovering the pool's own handle.
fn untag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from(handle.index() & POOL_INDEX_MASK),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Deterministic queue handles.
///
/// Queues are not pooled: there is exactly one for the device's lifetime and it
/// carries no state a caller can hold, so [`Device::queue`] stays a pure
/// function. The device's tag rides in the same place it does on a pooled
/// handle, because obligation 3 covers queues too — a `QueueHandle` synthesised
/// from the kind alone would carry no device identity at all, and every device
/// would accept every other device's.
fn queue_handle(tag: u32, kind: QueueKind) -> QueueHandle {
    Handle::from_bits((1u64 << 32) | u64::from((tag << DEVICE_TAG_SHIFT) | queue_index(kind)))
        .unwrap_or_else(|| unreachable!("generation 1 is non-zero"))
}

const fn queue_index(kind: QueueKind) -> u32 {
    match kind {
        QueueKind::Graphics => 0,
        QueueKind::Compute => 1,
        QueueKind::Transfer => 2,
    }
}

/// Decodes a handle for `inner`'s pools, or says why it is not one.
fn local_handle<E, M>(
    kind: &'static str,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Result<Handle<E>, HalError> {
    let tag = handle_tag(handle);
    if tag == inner.tag {
        return Ok(untag(handle));
    }
    // Tag zero was never issued by any device — a hand-made handle, or one
    // whose pool index overflowed the tagged range. Anything else is a real
    // handle belonging to a real, different device, which is the case
    // obligation 3 exists for.
    Err(if tag == 0 {
        HalError::invalid_handle(kind, handle)
    } else {
        HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }
    })
}

/// Resolves a handle against a pool and its owning device.
fn lookup<'p, E: Owned, M>(
    pool: &'p Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Result<&'p E, HalError> {
    let local = local_handle(kind, handle, inner)?;
    match pool.get(local) {
        Some(entry) if entry.owner() == inner.id => Ok(entry),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// Removes a handle from `pool`, but **only** if this device owns it.
///
/// The order is the point: removing first and checking the owner afterwards
/// would already have dropped the entry, so a foreign handle that happened to
/// resolve would destroy this device's own unrelated object.
fn take_owned<E: Owned, M>(pool: &mut Pool<E>, handle: Handle<M>, inner: &DeviceInner) -> bool {
    let Ok(local) = local_handle::<E, M>("object", handle, inner) else {
        return false;
    };
    if !pool
        .get(local)
        .is_some_and(|entry| entry.owner() == inner.id)
    {
        return false;
    }
    pool.remove(local).is_some()
}

/// The Metal implementation of [`Device`].
#[derive(Debug)]
pub struct MetalDevice {
    inner: Arc<DeviceInner>,
}

impl MetalDevice {
    /// Opens a device on `record`'s adapter.
    ///
    /// There is no `vkCreateDevice` equivalent: the `MTLDevice` already exists
    /// from enumeration, so opening a device means taking a reference to it and
    /// creating the queue every submission will go through. Both steps are
    /// synchronous, which is why [`crate::MetalInstance::request_device`]
    /// completes on its first poll.
    pub(crate) fn open(
        instance: Arc<InstanceInner>,
        record: &AdapterRecord,
        desc: &DeviceDesc<'_>,
    ) -> Result<Self, HalError> {
        let raw = record.raw.clone();
        let Some(queue) = raw.newCommandQueue() else {
            return Err(HalError::Backend(format!(
                "MTLDevice::newCommandQueue returned nil for adapter {:?}",
                record.info.name
            )));
        };
        // `MTLDevice` has no settable label — it names hardware, not an object
        // a program made — so the device's debug name goes on the queue, which
        // is where Xcode's GPU capture shows it.
        if let Some(label) = desc.label {
            queue.setLabel(Some(&NSString::from_str(label)));
        }

        // **Metal has no `pEnabledFeatures`.** A `VkDevice` reports only what
        // was switched on at creation, so `crcbl-vk` intersects the adapter's
        // features with what the caller asked for. An `MTLDevice` can do
        // whatever the adapter answered, always — there is nothing to enable
        // and nothing to leave off — so `caps` is the adapter's caps verbatim.
        // `required_features` was checked in `open_device` before this call;
        // `optional_features` is satisfied by construction, which is exactly
        // what `DeviceDesc::optional_features` documents ("check `Device::caps`
        // afterwards to find out").
        let caps = record.info.caps;
        let id = next_owner_id();
        let inner = Arc::new(DeviceInner {
            _instance: instance,
            raw,
            queue,
            caps,
            id,
            tag: device_tag(id),
            state: Mutex::new(DeviceState::default()),
        });
        log::info!(
            "crcbl-mtl: opened {:?} (tier {:?})",
            record.info.name,
            caps.tier()
        );
        Ok(Self { inner })
    }

    fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Resolves a queue handle against *this* device.
    ///
    /// Obligation 3 covers queues too, and the three outcomes are kept apart
    /// for the same reason they are everywhere else: a handle carrying another
    /// device's tag is [`HalError::ForeignObject`] — the caller crossed two
    /// objects that never met — while one carrying no tag at all was never
    /// issued by any device and is [`HalError::InvalidHandle`].
    fn check_queue(&self, queue: QueueHandle) -> Result<(), HalError> {
        if queue == queue_handle(self.inner.tag, QueueKind::Graphics) {
            return Ok(());
        }
        Err(if handle_tag(queue) == 0 {
            HalError::invalid_handle("queue", queue)
        } else {
            HalError::ForeignObject {
                kind: "queue",
                bits: queue.to_bits(),
            }
        })
    }

    /// Stamps this device's tag into a handle its pools just issued.
    ///
    /// Every handle that crosses the seam goes through here; every handle that
    /// comes back goes through [`local_handle`]. A pool index too large to
    /// carry the tag gets tag `0`, which resolves nowhere — the object leaks
    /// until the device is dropped, which is far better than a handle that
    /// might resolve to another device's object. It takes more live objects of
    /// one kind than [`POOL_INDEX_MASK`] admits to reach.
    fn stamp<A, B>(&self, handle: Handle<A>) -> Handle<B> {
        let index = handle.index();
        let tag = if index > POOL_INDEX_MASK {
            log::error!(
                "crcbl-mtl: pool index {index} is too large to carry a device tag; issuing a \
                 handle that resolves nowhere rather than one that might resolve to another \
                 device's object"
            );
            0
        } else {
            self.inner.tag
        };
        Handle::from_bits(
            (u64::from(handle.generation()) << 32) | u64::from((tag << DEVICE_TAG_SHIFT) | index),
        )
        .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
    }
}

/// The "this slice has not arrived" answer, in one place so the voice is
/// uniform across the whole trait.
fn not_yet(what: &'static str) -> HalError {
    crate::MetalInstance::not_yet(what)
}

/// Saturating `u64` → `NSUInteger`, for a length already bounds-checked against
/// a device limit.
fn to_ns(value: u64) -> NSUInteger {
    NSUInteger::try_from(value).unwrap_or(NSUInteger::MAX)
}

impl Device for MetalDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Metal
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
    }

    /// The graphics queue, and only ever the graphics queue.
    ///
    /// Metal has one `MTLCommandQueue` type that accepts render, compute and
    /// blit work, and no queue families at all — so there is nothing an
    /// [`QueueKind::Compute`] or [`QueueKind::Transfer`] handle could name that
    /// the graphics one does not. `crcbl-mtl`'s adapter module reports neither
    /// [`Features::ASYNC_COMPUTE_QUEUE`] nor [`Features::TRANSFER_QUEUE`] for
    /// the same reason, and the seam's contract — "check the feature first, or
    /// just fall back to `Graphics`, which always exists" — is then answered
    /// consistently from both ends.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        match kind {
            QueueKind::Graphics => Some(queue_handle(self.inner.tag, kind)),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    // --- resources ---

    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "BufferDesc::size must be non-zero".to_string(),
            ));
        }
        let ceiling = self.inner.caps.limits.max_storage_buffer_range;
        if desc.size > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "BufferDesc::size {} exceeds this device's maxBufferLength of {ceiling}",
                desc.size
            )));
        }
        // Metal has no buffer usage flags — every `MTLBuffer` can be bound
        // anywhere — with one exception the seam already models: taking a
        // buffer's GPU address is `MTLBuffer::gpuAddress`, a Metal 3 call, and
        // `BufferUsage::DEVICE_ADDRESS` is documented as requiring the feature
        // that reports it. Refusing here is what stops a Tier B caller getting
        // a buffer whose address it cannot take.
        if desc.usage.contains(BufferUsage::DEVICE_ADDRESS)
            && !self
                .inner
                .caps
                .features
                .contains(Features::BUFFER_DEVICE_ADDRESS)
        {
            return Err(HalError::InvalidDescriptor(
                "BufferUsage::DEVICE_ADDRESS needs Features::BUFFER_DEVICE_ADDRESS, which this \
                 device does not report"
                    .to_string(),
            ));
        }

        let Some(raw) = self
            .inner
            .raw
            .newBufferWithLength_options(to_ns(desc.size), conv::resource_options(desc.memory))
        else {
            // `newBufferWithLength:options:` returns nil when the allocation
            // fails, which is the one failure the seam has a name for here.
            return Err(HalError::OutOfDeviceMemory);
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().buffers.insert(BufferEntry {
            owner: self.inner.id,
            raw,
            size: desc.size,
            location: desc.memory,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        let mut state = self.state();
        take_owned(&mut state.buffers, buffer, &self.inner);
    }

    /// Copies `data` into a host-visible buffer.
    ///
    /// # `DeviceLocal` is refused, never silently dropped
    ///
    /// A [`MemoryLocation::DeviceLocal`] buffer is `MTLStorageMode::Private`
    /// and has no `contents` pointer at all — Metal's only route into one is a
    /// blit from a staging buffer, and the blit encoder is the MTL3 slice. So
    /// this refuses with [`HalError::InvalidDescriptor`] naming the location,
    /// which is both what the seam documents ("`InvalidDescriptor` … if the
    /// buffer is not host-visible") and what `crcbl-vk` answers for the same
    /// call, so the two backends disagree about nothing.
    ///
    /// The alternative — accepting the call and writing nothing — is the shape
    /// this workspace treats as a defect: a caller would upload a mesh, draw
    /// it, and see an empty screen with no error anywhere.
    ///
    /// # No `didModifyRange:`
    ///
    /// That call exists for `MTLStorageMode::Managed` and, per Metal's own
    /// header, "is not valid to invoke … on buffers of other storage modes".
    /// Both host-visible locations here are `Shared` — see
    /// `conv::storage_mode` for that decision — where the write is coherent
    /// at the next command-buffer boundary with no call at all.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let state = self.state();
        let entry = lookup(&state.buffers, "buffer", buffer, &self.inner)?;
        if !entry.location.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a host-visible buffer; this one is {:?}, which Metal can \
                 only reach through a blit from a staging buffer (the Metal command slice)",
                entry.location
            )));
        }
        let end = offset.checked_add(data.len() as u64).ok_or_else(|| {
            HalError::InvalidDescriptor("write_buffer range overflows".to_string())
        })?;
        if end > entry.size {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer range {offset}..{end} exceeds the buffer's {} bytes",
                entry.size
            )));
        }
        if data.is_empty() {
            return Ok(());
        }
        let contents = entry.raw.contents();
        // SAFETY: `contents` points at `entry.size` bytes of a `Shared`
        // allocation this device made, the range was bounds-checked against
        // that size immediately above, and the two regions cannot overlap
        // because `data` is a caller-owned slice and the destination is the
        // buffer's own storage. The pointer does not escape this block, and the
        // device lock is held for the whole copy.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                contents.as_ptr().cast::<u8>().add(offset as usize),
                data.len(),
            );
        }
        Ok(())
    }

    fn request_readback(&self, _desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        // Deliberately refused before the buffer handle is checked. A readback
        // is not a buffer read: the seam's contract is that it covers work
        // already submitted, which means waiting on a completion point and, on
        // a `Private` source, blitting into a host-visible one. Both need the
        // blit encoder.
        Err(not_yet("GPU readback (the Metal command slice)"))
    }

    fn poll_readback(
        &self,
        _readback: ReadbackHandle,
        _out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        Err(not_yet("GPU readback (the Metal command slice)"))
    }

    fn destroy_readback(&self, _readback: ReadbackHandle) {
        // Unreachable with a live handle: `request_readback` above issues none.
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let limits = self.inner.caps.limits;
        let extent = desc.extent;
        if extent.width == 0 || extent.height == 0 || extent.depth_or_layers == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::extent has a zero dimension: {extent:?}"
            )));
        }
        if desc.usage.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "ImageDesc::usage is empty, so the image could never be used".to_string(),
            ));
        }
        if !desc.samples.is_power_of_two() || desc.samples > limits.max_sample_count {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::samples is {} but this device supports powers of two up to {}",
                desc.samples, limits.max_sample_count
            )));
        }
        // Two rules `MTLTextureDescriptor` states in its own headers, checked
        // here because Metal enforces them by *raising* rather than by
        // returning nil — and an Objective-C exception crossing back into Rust
        // aborts the process, which is a far worse answer than an `Err` for
        // what is a caller's descriptor bug either way.
        if desc.samples > 1 && desc.mip_levels > 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "a multisampled image has one mip level, not {}",
                desc.mip_levels
            )));
        }
        if matches!(desc.image_type, ImageType::D1) && extent.height != 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageType::D1 requires a height of 1, not {}",
                extent.height
            )));
        }
        let is_3d = matches!(desc.image_type, ImageType::D3);
        let ceiling = if is_3d {
            limits.max_image_3d
        } else {
            limits.max_image_2d
        };
        if extent.width > ceiling || extent.height > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::extent {extent:?} exceeds this device's {ceiling}-texel limit"
            )));
        }
        if is_3d && extent.depth_or_layers > limits.max_image_3d {
            return Err(HalError::InvalidDescriptor(format!(
                "a volume's depth {} exceeds this device's {} limit",
                extent.depth_or_layers, limits.max_image_3d
            )));
        }
        if !is_3d && extent.depth_or_layers > limits.max_image_array_layers {
            return Err(HalError::InvalidDescriptor(format!(
                "{} array layers exceed this device's {} limit",
                extent.depth_or_layers, limits.max_image_array_layers
            )));
        }
        // Two format questions Metal answers per device rather than per API,
        // and both would otherwise raise rather than return nil.
        if desc.format == Format::D24UnormS8Uint
            && !self.inner.raw.isDepth24Stencil8PixelFormatSupported()
        {
            return Err(HalError::InvalidDescriptor(
                "Format::D24UnormS8Uint is not supported by this device — Apple silicon reports \
                 no; use Format::D32Float, which the seam already prefers"
                    .to_string(),
            ));
        }
        if desc.format.is_compressed()
            && !self
                .inner
                .caps
                .features
                .contains(Features::TEXTURE_COMPRESSION_BC)
        {
            return Err(HalError::InvalidDescriptor(format!(
                "{:?} needs Features::TEXTURE_COMPRESSION_BC, which this device does not report",
                desc.format
            )));
        }

        let mip_levels = desc.mip_levels.max(1);
        let layers = if is_3d { 1 } else { extent.depth_or_layers };
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(conv::texture_type(desc.image_type, layers, desc.samples));
        descriptor.setPixelFormat(conv::pixel_format(desc.format));
        descriptor.setUsage(conv::texture_usage(desc.usage, desc.format));
        descriptor.setStorageMode(conv::storage_mode(desc.memory));
        descriptor.setCpuCacheMode(conv::cpu_cache_mode(desc.memory));
        // SAFETY: `objc2` marks these setters unsafe because Metal does not
        // bounds-check them and raises on an out-of-range value. Every argument
        // below was checked against this device's `Limits` above — the extent
        // against the dimension ceilings, the layer count against
        // `max_image_array_layers`, the sample count against
        // `max_sample_count` — and `mip_levels` is clamped to at least one, so
        // none of them can be the value Metal objects to.
        unsafe {
            descriptor.setWidth(extent.width as NSUInteger);
            descriptor.setHeight(extent.height as NSUInteger);
            descriptor.setDepth(if is_3d {
                extent.depth_or_layers as NSUInteger
            } else {
                1
            });
            descriptor.setArrayLength(layers as NSUInteger);
            descriptor.setMipmapLevelCount(mip_levels as NSUInteger);
            descriptor.setSampleCount(desc.samples.max(1) as NSUInteger);
        }

        let Some(raw) = self.inner.raw.newTextureWithDescriptor(&descriptor) else {
            return Err(HalError::OutOfDeviceMemory);
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().images.insert(ImageEntry {
            owner: self.inner.id,
            raw,
            format: desc.format,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_image(&self, image: ImageHandle) {
        let mut state = self.state();
        take_owned(&mut state.images, image, &self.inner);
    }

    /// Creates a view onto a subrange of an image.
    ///
    /// Metal's `newTextureViewWithPixelFormat:textureType:levels:slices:` takes
    /// absolute ranges, so the seam's
    /// [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL)
    /// sentinel is resolved here against the texture's own level and slice
    /// counts, read back off the object rather than remembered.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let mut state = self.state();
        let entry = lookup(&state.images, "image", desc.image, &self.inner)?;
        // A depth or stencil format has no compatible reinterpretation in
        // Metal — the depth formats are their own class — so the texture was
        // not created with `MTLTextureUsagePixelFormatView` and asking for a
        // different format here would raise. Refuse it while it is still an
        // error a caller can catch.
        if desc.format != entry.format
            && (desc.format.is_depth_stencil() || entry.format.is_depth_stencil())
        {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of a {:?} image cannot reinterpret it as {:?}: Metal permits no \
                 reinterpretation involving a depth or stencil format",
                entry.format, desc.format
            )));
        }

        let texture = entry.raw.clone();
        let levels = texture.mipmapLevelCount();
        let slices = texture.arrayLength();
        let range = desc.range;
        let base_mip = range.base_mip as NSUInteger;
        let base_layer = range.base_layer as NSUInteger;
        if base_mip >= levels || base_layer >= slices {
            return Err(HalError::InvalidDescriptor(format!(
                "view starts at mip {base_mip} layer {base_layer}, and the image has {levels} \
                 mips and {slices} layers"
            )));
        }
        let mip_count = resolve_count(range.mip_count, base_mip, levels);
        let layer_count = resolve_count(range.layer_count, base_layer, slices);
        if mip_count == 0 || layer_count == 0 {
            return Err(HalError::InvalidDescriptor(
                "an image view covering no mip levels or no layers is not a view".to_string(),
            ));
        }

        // SAFETY: `objc2` marks this unsafe because Metal does not bounds-check
        // the two ranges. Both were just clamped to the texture's own
        // `mipmapLevelCount` and `arrayLength`, read off the texture above, and
        // both bases were checked to be inside them.
        let view = unsafe {
            texture.newTextureViewWithPixelFormat_textureType_levels_slices(
                conv::pixel_format(desc.format),
                conv::view_texture_type(desc.view_type),
                NSRange::new(base_mip, mip_count),
                NSRange::new(base_layer, layer_count),
            )
        };
        let Some(view) = view else {
            return Err(HalError::Backend(
                "MTLTexture::newTextureViewWithPixelFormat:textureType:levels:slices: returned \
                 nil"
                .to_string(),
            ));
        };
        if let Some(label) = desc.label {
            view.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = state.views.insert(ViewEntry {
            owner: self.inner.id,
            raw: view,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        let mut state = self.state();
        take_owned(&mut state.views, view, &self.inner);
    }

    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        let cap = self.inner.caps.limits.max_sampler_anisotropy;
        if desc.anisotropy < 1.0 {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} is below 1.0, which is the value that disables it",
                desc.anisotropy
            )));
        }
        if desc.anisotropy > cap {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} exceeds max_sampler_anisotropy {cap}",
                desc.anisotropy
            )));
        }

        let descriptor = MTLSamplerDescriptor::new();
        descriptor.setMinFilter(conv::min_mag_filter(desc.min_filter));
        descriptor.setMagFilter(conv::min_mag_filter(desc.mag_filter));
        descriptor.setMipFilter(conv::mip_filter(desc.mip_filter));
        descriptor.setSAddressMode(conv::address_mode(desc.address_mode[0]));
        descriptor.setTAddressMode(conv::address_mode(desc.address_mode[1]));
        descriptor.setRAddressMode(conv::address_mode(desc.address_mode[2]));
        // Transparent black, which is what the seam's `ClampToBorder`
        // documents and what a shadow atlas needs; opaque black would put a
        // black frame around every clamped sample.
        descriptor.setBorderColor(objc2_metal::MTLSamplerBorderColor::TransparentBlack);
        descriptor.setLodMinClamp(desc.lod_min);
        descriptor.setLodMaxClamp(desc.lod_max);
        // Metal takes an integer here and rejects anything outside 1...16; the
        // seam's `f32` is a Vulkan-shaped spelling of the same knob. Truncating
        // rather than rounding keeps the promise a limit makes: never sample
        // with more taps than the caller asked for.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let anisotropy = desc.anisotropy.min(MAX_SAMPLER_ANISOTROPY) as NSUInteger;
        descriptor.setMaxAnisotropy(anisotropy.max(1));
        if let Some(compare) = desc.compare {
            descriptor.setCompareFunction(conv::compare_function(compare));
        }
        // A sampler's argument-buffer support is fixed at creation and cannot
        // be retrofitted, and this backend's headline capability is Tier 2
        // argument buffers — so a sampler made now that could not be written
        // into one would be a sampler the bind-group slice has to re-create.
        // Asked for only where the device reports the feature, because the
        // property is meaningless without it.
        if self
            .inner
            .caps
            .features
            .contains(Features::DESCRIPTOR_INDEXING)
        {
            descriptor.setSupportArgumentBuffers(true);
        }
        if let Some(label) = desc.label {
            descriptor.setLabel(Some(&NSString::from_str(label)));
        }

        let Some(raw) = self.inner.raw.newSamplerStateWithDescriptor(&descriptor) else {
            return Err(HalError::Backend(
                "MTLDevice::newSamplerStateWithDescriptor: returned nil".to_string(),
            ));
        };
        let handle = self.state().samplers.insert(SamplerEntry {
            owner: self.inner.id,
            raw,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        let mut state = self.state();
        take_owned(&mut state.samplers, sampler, &self.inner);
    }

    // --- shaders and pipelines ---

    fn create_shader_module(
        &self,
        _desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // Not `ShaderCompilation`, which the seam reserves for an artifact this
        // backend tried and failed to compile. Nothing was tried: MSL is a
        // `crcbl-shaders` output that does not exist yet, and
        // `ShaderModuleDesc` carries no field this backend reads.
        Err(not_yet("shader modules (the Metal pipeline slice)"))
    }

    fn destroy_shader_module(&self, _module: ShaderModuleHandle) {}

    fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        Err(not_yet("bind group layouts (the Metal binding slice)"))
    }

    fn destroy_bind_group_layout(&self, _layout: BindGroupLayoutHandle) {}

    fn create_bind_group(&self, _desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        Err(not_yet("bind groups (the Metal binding slice)"))
    }

    fn update_bind_group(
        &self,
        _group: BindGroupHandle,
        _entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        Err(not_yet("bind groups (the Metal binding slice)"))
    }

    fn destroy_bind_group(&self, _group: BindGroupHandle) {}

    fn create_pipeline_layout(
        &self,
        _desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        Err(not_yet("pipeline layouts (the Metal binding slice)"))
    }

    fn destroy_pipeline_layout(&self, _layout: PipelineLayoutHandle) {}

    fn create_graphics_pipeline(
        &self,
        _desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Err(not_yet("graphics pipelines (the Metal pipeline slice)"))
    }

    fn destroy_graphics_pipeline(&self, _pipeline: GraphicsPipelineHandle) {}

    fn create_compute_pipeline(
        &self,
        _desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        Err(not_yet("compute pipelines (the Metal pipeline slice)"))
    }

    fn destroy_compute_pipeline(&self, _pipeline: ComputePipelineHandle) {}

    // --- queries ---

    fn create_query_set(&self, _desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        Err(not_yet("query sets (the Metal query slice)"))
    }

    fn destroy_query_set(&self, _set: QuerySetHandle) {}

    fn query_results(
        &self,
        set: QuerySetHandle,
        _first_query: u32,
        _out: &mut [u64],
    ) -> Result<(), HalError> {
        // The seam says a device without `TIMESTAMP_QUERY` returns zeros rather
        // than failing, so the profiler HUD degrades instead of breaking — but
        // that applies to a query set that exists. None can, so the handle
        // cannot resolve, and `InvalidHandle` is the honest answer.
        Err(HalError::invalid_handle("query set", set))
    }

    // --- synchronisation ---

    fn create_semaphore(&self, _desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        Err(not_yet("semaphores (the Metal command slice)"))
    }

    fn destroy_semaphore(&self, _semaphore: SemaphoreHandle) {}

    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        Err(HalError::invalid_handle("semaphore", semaphore))
    }

    fn wait_semaphores(
        &self,
        _waits: &[crcbl_hal::SemaphoreWait],
        _timeout_ns: u64,
    ) -> Result<bool, HalError> {
        Err(not_yet("semaphores (the Metal command slice)"))
    }

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// Metal has no `vkDeviceWaitIdle`. What it has is ordering: an
    /// `MTLCommandQueue` runs its command buffers in commit order, so an empty
    /// command buffer committed now completes only after everything committed
    /// before it, and waiting on that one waits for all of them. That is the
    /// standard Metal idiom for a device idle and it is what this does.
    ///
    /// It is a real wait today even though [`Device::submit`] still refuses:
    /// the queue is real, the command buffer is real, and this is the call that
    /// proves both work. When submission lands, nothing here changes.
    fn wait_idle(&self) -> Result<(), HalError> {
        let Some(command_buffer) = self.inner.queue.commandBuffer() else {
            return Err(HalError::DeviceLost(
                "MTLCommandQueue::commandBuffer returned nil".to_string(),
            ));
        };
        command_buffer.setLabel(Some(&NSString::from_str("crcbl wait_idle")));
        command_buffer.commit();
        command_buffer.waitUntilCompleted();
        if command_buffer.status() == MTLCommandBufferStatus::Error {
            let reason = command_buffer
                .error()
                .map_or_else(|| "no reason given".to_string(), |error| error.to_string());
            return Err(HalError::DeviceLost(format!(
                "the wait_idle command buffer failed: {reason}"
            )));
        }
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, _desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(MetalCommandEncoder::new())
    }

    fn destroy_command_buffer(&self, _buffer: CommandBufferHandle) {
        // Unreachable with a live handle: no encoder finishes, so this device
        // has issued no command buffer for a caller to release.
    }

    fn submit(&self, queue: QueueHandle, _submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        // The queue check is real and comes first, because it is the one thing
        // here this slice can genuinely diagnose: a handle from another device
        // is a caller bug with its own contract, and hiding it behind the
        // refusal below would lose it.
        self.check_queue(queue)?;
        Err(not_yet("submission (the Metal command slice)"))
    }

    // --- presentation ---

    fn create_swapchain(&self, _desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the Metal surface slice)",
        )))
    }

    fn reconfigure_swapchain(
        &self,
        _swapchain: SwapchainHandle,
        _desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the Metal surface slice)",
        )))
    }

    fn destroy_swapchain(&self, _swapchain: SwapchainHandle) {}

    fn acquire_next_frame(
        &self,
        _swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the Metal surface slice)",
        )))
    }

    fn present(&self, _queue: QueueHandle, _present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "presentation (the Metal surface slice)",
        )))
    }
}

/// Resolves a seam subresource count against the object's real extent.
///
/// [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL) means
/// "every remaining one", and a count that runs past the end is clamped rather
/// than refused — the request is satisfiable, just wider than the object, and
/// Metal would raise on the raw number.
fn resolve_count(requested: u32, base: NSUInteger, total: NSUInteger) -> NSUInteger {
    let remaining = total.saturating_sub(base);
    if requested == crcbl_hal::ImageSubresourceRange::ALL {
        return remaining;
    }
    (requested as NSUInteger).min(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{
        Extent3d, ImageAspect, ImageSubresourceRange, ImageUsage, ImageViewType, Instance,
    };

    use crate::MetalInstance;
    use crate::instance::tests::{desc as device_desc, open as open_instance};

    /// Every [`MemoryLocation`] the seam has, so the buffer tests cover all
    /// three rather than the one that was convenient.
    const LOCATIONS: &[MemoryLocation] = &[
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ];

    /// A device, opened through this crate's own type so a test can reach the
    /// pools underneath it.
    fn open_device() -> (MetalInstance, MetalDevice) {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "a Mac has at least one adapter");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        (instance, device)
    }

    fn buffer(size: u64, memory: MemoryLocation) -> BufferDesc<'static> {
        BufferDesc {
            label: Some("crcbl-mtl test buffer"),
            size,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory,
        }
    }

    /// Reads a host-visible buffer's bytes straight out of Metal, bypassing the
    /// seam — which has no read path yet, and which is exactly why this is the
    /// only way to observe that `write_buffer` wrote anything.
    fn read_back(device: &MetalDevice, handle: BufferHandle, len: usize) -> Vec<u8> {
        let state = device.state();
        let entry = lookup(&state.buffers, "buffer", handle, &device.inner)
            .expect("the buffer is live and this device's");
        assert!(entry.location.is_mappable(), "not a readable buffer");
        assert!(len as u64 <= entry.size, "reading past the buffer");
        let contents = entry.raw.contents();
        // SAFETY: `contents` covers `entry.size` bytes of a live `Shared`
        // allocation, `len` was just asserted to be within it, and the read
        // happens under the device lock with no GPU work in flight.
        unsafe { core::slice::from_raw_parts(contents.as_ptr().cast::<u8>(), len) }.to_vec()
    }

    /// The device opens, says which backend it is, and has exactly the queue
    /// Metal has.
    #[test]
    fn a_device_reports_metal_and_one_graphics_queue() {
        let (_instance, device) = open_device();
        assert_eq!(device.backend(), BackendKind::Metal);
        assert!(
            device.queue(QueueKind::Graphics).is_some(),
            "every MTLDevice has a command queue"
        );
        assert!(
            device.queue(QueueKind::Compute).is_none(),
            "Metal has no queue families, so there is no separate compute queue"
        );
        assert!(device.queue(QueueKind::Transfer).is_none());
        // A queue handle is per device, not per process.
        let (_second_instance, other) = open_device();
        assert_ne!(
            device.queue(QueueKind::Graphics),
            other.queue(QueueKind::Graphics),
            "two devices must not issue the same queue handle"
        );
    }

    /// `caps` is the adapter's caps: Metal enables nothing and disables
    /// nothing, so a device that reported less than its adapter would be
    /// lying about hardware it can use.
    #[test]
    fn device_caps_match_the_adapter_they_came_from() {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        assert_eq!(device.caps().features, adapters[0].caps.features);
        assert_eq!(device.caps().limits, adapters[0].caps.limits);
    }

    /// A buffer of every memory location creates and destroys, and a destroyed
    /// handle fails **as a stale handle** rather than as anything else.
    ///
    /// The variant matters and is the reason this asserts on it: a
    /// `DeviceLocal` buffer that is still alive fails `write_buffer` with
    /// `InvalidDescriptor`, so "it returned an error" would pass whether or not
    /// the handle was ever invalidated.
    #[test]
    fn buffers_of_every_memory_location_create_and_then_stop_resolving() {
        let (_instance, device) = open_device();
        assert!(!LOCATIONS.is_empty(), "nothing to check");
        for &location in LOCATIONS {
            let handle = device
                .create_buffer(&buffer(4096, location))
                .unwrap_or_else(|error| panic!("{location:?}: {error:?}"));

            device.destroy_buffer(handle);

            let error = device
                .write_buffer(handle, 0, &[0u8; 4])
                .expect_err("the handle was destroyed");
            assert!(
                matches!(error, HalError::InvalidHandle { kind, .. } if kind == "buffer"),
                "{location:?}: expected a stale-handle failure, got {error:?}"
            );
        }
    }

    /// A recycled slot must not resurrect the handle that used to name it.
    #[test]
    fn a_destroyed_handle_does_not_alias_the_buffer_that_replaces_it() {
        let (_instance, device) = open_device();
        let first = device
            .create_buffer(&buffer(256, MemoryLocation::HostUpload))
            .expect("first buffer");
        device.destroy_buffer(first);
        let second = device
            .create_buffer(&buffer(256, MemoryLocation::HostUpload))
            .expect("second buffer");

        assert_ne!(
            first, second,
            "the pool reissued the identical handle, so the generation never moved"
        );
        assert_eq!(
            first.index(),
            second.index(),
            "the free list should have handed back the same slot; if not, this test is not \
             exercising recycling at all"
        );
        device
            .write_buffer(second, 0, &[1u8; 4])
            .expect("the live handle resolves");
        let error = device
            .write_buffer(first, 0, &[1u8; 4])
            .expect_err("the dead handle must not name its replacement");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    }

    /// `write_buffer` writes, at the offset it was given, and refuses the
    /// location Metal cannot reach without a blit.
    #[test]
    fn write_buffer_writes_host_visible_memory_and_refuses_device_local() {
        let (_instance, device) = open_device();
        let readback = device
            .create_buffer(&buffer(16, MemoryLocation::HostReadback))
            .expect("a readback buffer");

        // Two writes, so the result is fully determined whatever Metal left in
        // the fresh allocation: fill, then overwrite a window at an offset.
        device
            .write_buffer(readback, 0, &[0xAA; 16])
            .expect("the whole range");
        device
            .write_buffer(readback, 4, &[0x01, 0x02, 0x03, 0x04])
            .expect("a window at an offset");
        let mut expected = [0xAA_u8; 16];
        expected[4..8].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            read_back(&device, readback, 16),
            expected,
            "write_buffer either wrote nothing or ignored the offset"
        );

        // An upload buffer is write-combined, so it is written and not read
        // here — the acceptance is the assertion.
        let upload = device
            .create_buffer(&buffer(16, MemoryLocation::HostUpload))
            .expect("an upload buffer");
        device
            .write_buffer(upload, 0, &[0x7F; 16])
            .expect("HostUpload is what write_buffer is for");

        let private = device
            .create_buffer(&buffer(16, MemoryLocation::DeviceLocal))
            .expect("a device-local buffer");
        let error = device
            .write_buffer(private, 0, &[0u8; 4])
            .expect_err("a Private buffer has no contents pointer");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("DeviceLocal"), "{text}");
        assert!(
            text.contains("blit"),
            "the refusal must say what would make it work: {text}"
        );

        // Out of range is the other refusal the seam names.
        let error = device
            .write_buffer(readback, 13, &[0u8; 4])
            .expect_err("13..17 does not fit in 16 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        for handle in [readback, upload, private] {
            device.destroy_buffer(handle);
        }
    }

    /// **Obligation 3.** A handle from device A used on device B is a caller
    /// bug that must be *detected*.
    ///
    /// Device B is given a buffer of its own first, and that is the whole
    /// design of this test: without the device tag in the handle, A's first
    /// handle and B's first handle are bit-identical, so B would resolve A's
    /// handle to B's own buffer, find the owner matching, and write into the
    /// wrong object with no error anywhere.
    #[test]
    fn a_handle_from_another_device_is_foreign_not_merely_unresolvable() {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let a = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("device A");
        let b = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("device B");

        let on_a = a
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("a buffer on A");
        let on_b = b
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("a buffer on B, occupying the slot A's handle would land in");
        assert_eq!(
            on_a.generation(),
            on_b.generation(),
            "both pools are fresh, so only the tag can tell these apart"
        );

        let error = b
            .write_buffer(on_a, 0, &[0xFF; 4])
            .expect_err("A's buffer is not B's to write");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "buffer"),
            "expected ForeignObject, got {error:?}"
        );

        // And B's own handle still works, so the check is not simply refusing
        // everything.
        b.write_buffer(on_b, 0, &[0xFF; 4])
            .expect("B's own buffer resolves");

        // A destroy with a foreign handle must not destroy the local object
        // that shares its bits.
        b.destroy_buffer(on_a);
        b.write_buffer(on_b, 0, &[0xEE; 4])
            .expect("B's buffer survived a foreign destroy");
    }

    /// Images, views and samplers all the way through, and the handles stop
    /// resolving when they are destroyed.
    #[test]
    fn images_views_and_samplers_create_and_destroy() {
        let (_instance, device) = open_device();
        let extent = Extent3d::d2(64, 64);
        let image = device
            .create_image(&ImageDesc {
                label: Some("crcbl-mtl test image"),
                image_type: ImageType::D2,
                extent,
                format: Format::Rgba8Unorm,
                mip_levels: extent.full_mip_levels(ImageType::D2),
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a 2D colour image");

        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("crcbl-mtl test view"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("a whole-image view");

        // One mip of the chain, which is the case the ALL sentinel resolution
        // has to get right.
        let one_mip = device
            .create_image_view(&ImageViewDesc {
                label: Some("crcbl-mtl one mip"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 1,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            })
            .expect("a single-mip view");
        assert_ne!(view, one_mip);

        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("crcbl-mtl test sampler"),
                ..SamplerDesc::default()
            })
            .expect("the seam's default sampler");

        device.destroy_image_view(one_mip);
        device.destroy_image_view(view);
        device.destroy_sampler(sampler);
        device.destroy_image(image);

        let error = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect_err("the image was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "image"),
            "{error:?}"
        );
    }

    /// The sRGB reinterpretation the seam names, exercised end to end.
    ///
    /// This is what `MTLTextureUsagePixelFormatView` is set for. Without it
    /// Metal refuses the view, so this is the assertion that keeps the flag
    /// from being "optimised away" later.
    #[test]
    fn a_linear_image_can_be_viewed_as_its_srgb_partner() {
        let (_instance, device) = open_device();
        let image = device
            .create_image(&ImageDesc {
                label: Some("linear albedo"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(32, 32),
                format: Format::Rgba8Unorm,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::SAMPLED,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a linear image");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("sRGB view"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8UnormSrgb,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("an sRGB view of a linear image");
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    /// A depth image cannot be reinterpreted, and says so instead of letting
    /// Metal raise.
    #[test]
    fn a_depth_image_refuses_a_differing_view_format() {
        let (_instance, device) = open_device();
        let image = device
            .create_image(&ImageDesc {
                label: Some("depth"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(32, 32),
                format: Format::D32Float,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a depth image");
        let error = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::R32Float,
                range: ImageSubresourceRange::all(Format::D32Float),
            })
            .expect_err("depth formats have no compatible reinterpretation");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // The same-format view is still fine, so the refusal is about the
        // reinterpretation and not about depth images.
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::D32Float,
                range: ImageSubresourceRange::all(Format::D32Float),
            })
            .expect("a depth view of a depth image");
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    /// Anisotropy is bounded on both sides, and the bound is the one the
    /// adapter reports.
    #[test]
    fn samplers_reject_anisotropy_outside_the_reported_cap() {
        let (_instance, device) = open_device();
        let cap = device.caps().limits.max_sampler_anisotropy;
        assert!(
            cap >= 1.0,
            "an anisotropy cap below 1.0 would disable a sampler that asked for none"
        );

        let too_much = device.create_sampler(&SamplerDesc {
            anisotropy: cap + 1.0,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(too_much, Err(HalError::InvalidDescriptor(_))),
            "{too_much:?}"
        );

        let too_little = device.create_sampler(&SamplerDesc {
            anisotropy: 0.5,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(too_little, Err(HalError::InvalidDescriptor(_))),
            "{too_little:?}"
        );

        // And a comparison sampler at the cap is accepted — the shadow-map
        // shape, with the reversed-Z comparison the seam documents.
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("shadow"),
                anisotropy: cap,
                compare: Some(crcbl_hal::CompareOp::Greater),
                ..SamplerDesc::default()
            })
            .expect("a comparison sampler at the reported cap");
        device.destroy_sampler(sampler);
    }

    /// `wait_idle` really waits: it commits a command buffer to the real queue
    /// and blocks on it. A queue that could not produce or run one fails here.
    #[test]
    fn wait_idle_runs_a_command_buffer_to_completion() {
        let (_instance, device) = open_device();
        device
            .wait_idle()
            .expect("an empty command buffer completes");
        // Twice, because a queue that only works once is a queue that leaked
        // something.
        device.wait_idle().expect("and again");
    }

    /// Every slice that has not arrived still refuses, by name — so none of
    /// them can be half-implemented without this saying so.
    #[test]
    fn the_slices_that_have_not_arrived_still_refuse_and_name_themselves() {
        let (_instance, device) = open_device();

        let refusals: Vec<(&str, HalError)> = vec![
            (
                "shader modules",
                device
                    .create_shader_module(&ShaderModuleDesc {
                        label: None,
                        spirv: &[],
                        wgsl: None,
                    })
                    .expect_err("no MSL path yet"),
            ),
            (
                "bind group layouts",
                device
                    .create_bind_group_layout(&BindGroupLayoutDesc {
                        label: None,
                        entries: &[],
                    })
                    .expect_err("no argument buffers yet"),
            ),
            (
                "pipeline layouts",
                device
                    .create_pipeline_layout(&PipelineLayoutDesc {
                        label: None,
                        bind_group_layouts: &[],
                        push_constants: None,
                    })
                    .expect_err("no pipeline layouts yet"),
            ),
            (
                "query sets",
                device
                    .create_query_set(&QuerySetDesc {
                        label: None,
                        kind: crcbl_hal::QueryKind::Timestamp,
                        count: 1,
                    })
                    .expect_err("no counter sampling yet"),
            ),
            (
                "semaphores",
                device
                    .create_semaphore(&SemaphoreDesc {
                        label: None,
                        kind: crcbl_hal::SemaphoreKind::Timeline { initial_value: 0 },
                    })
                    .expect_err("no MTLSharedEvent yet"),
            ),
            (
                "readback",
                device
                    .request_readback(&ReadbackDesc {
                        label: None,
                        buffer: Handle::from_bits(1 << 32).expect("generation 1"),
                        offset: 0,
                        size: 4,
                        after: None,
                    })
                    .expect_err("no blit encoder yet"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, error) in &refusals {
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if *backend == BackendKind::Metal),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(text.contains("metal"), "{what}: {text}");
            assert!(
                text.contains("Metal") && text.contains("slice"),
                "{what}: {text}"
            );
        }

        // Recording is refused where the seam gives it a `Result` to say so:
        // `create_command_encoder` returns a bare `Box`, so the encoder accepts
        // the recording calls and `finish` is the refusal.
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        let error = encoder.finish().expect_err("nothing was recorded");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
            "{error:?}"
        );

        // Submission checks the queue handle first and refuses second.
        let error = device
            .submit(queue, &SubmitInfo::new(&[]))
            .expect_err("nothing can be submitted yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
            "{error:?}"
        );
        // A queue handle really belonging to another device is foreign; a
        // hand-made one carries no device tag at all and was never issued.
        let (_other_instance, other) = open_device();
        let other_queue = other
            .queue(QueueKind::Graphics)
            .expect("the other device has a queue too");
        assert_ne!(queue, other_queue);
        let error = device
            .submit(other_queue, &SubmitInfo::new(&[]))
            .expect_err("that queue belongs to the other device");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "queue"),
            "{error:?}"
        );

        let untagged = Handle::from_bits(1 << 32).expect("generation 1");
        let error = device
            .submit(untagged, &SubmitInfo::new(&[]))
            .expect_err("no device ever issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }
}
