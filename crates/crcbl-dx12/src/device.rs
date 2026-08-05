//! [`Dx12Device`]: one `ID3D12Device`, its queue, and the four resource tables
//! this slice fills.
//!
//! # What this slice implements, and what it still refuses
//!
//! Buffers, images, image views and samplers — created, destroyed, and looked up
//! through generational handles — plus [`Device::backend`], [`Device::caps`],
//! [`Device::queue`], [`Device::write_buffer`] and [`Device::wait_idle`].
//! Everything else on the trait refuses with [`HalError::Unsupported`] whose
//! `what` names the slice it arrives in, in the same voice `Dx12Instance`
//! established. Nothing here is a stub that reports success.
//!
//! # One lock over every table
//!
//! The seam takes `&self` everywhere so a device can be shared behind an `Arc`,
//! which means a backend owes its own interior synchronisation. This one uses a
//! single [`Mutex`] over the four pools *and* the descriptor heaps, which is the
//! same call `crcbl-vk` and `crcbl-mtl` made and for the same reason: the
//! traffic is a few dozen operations per frame, and a lock-per-table scheme has
//! a deadlock-ordering problem to design before it has a contention problem to
//! solve. The descriptor allocator has to be inside it either way — creating a
//! view touches the image table and a heap in one step.
//!
//! # No `unsafe` marker impl, and this time the crate docs were right
//!
//! `crcbl-mtl` had to add one in its device slice, because `MTLBuffer` and
//! `MTLTexture` inherit from `MTLResource`, which objc2 leaves unmarked.
//! `windows-rs` declares `Send` **and** `Sync` for every interface this module
//! holds — `ID3D12Device`, `ID3D12CommandQueue`, `ID3D12Resource`,
//! `ID3D12DescriptorHeap` and `ID3D12Fence` — so the markers come from the
//! compiler rather than from an assertion written here, exactly as
//! `crcbl_dx12`'s crate docs predicted for this slice. The one Win32 type that
//! is *not* `Send` is the event `HANDLE` a fence wait uses, and
//! [`Device::wait_idle`] creates and closes it inside the call rather than
//! storing it, so it never becomes this struct's problem.
//!
//! # There is no deletion queue here, and that **is** a gap
//!
//! `crcbl-vk` parks every destroyed object on a timeline-keyed retire queue,
//! because destroying a resource the GPU is still reading is undefined
//! behaviour. D3D12 is Vulkan's model, not Metal's: a command list does **not**
//! retain the resources it references, and releasing the last reference to one
//! with work in flight is a use-after-free in the driver. `destroy_*` therefore
//! drops the interface and is done, which is sound only because nothing in this
//! slice can submit — [`Device::submit`] refuses. **The command slice must
//! bring the retire queue with it**, and this paragraph is the note it is owed.

use std::sync::{Arc, Mutex, MutexGuard};

use crcbl_core::Pool;
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, Device,
    DeviceCaps, DeviceDesc, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc,
    ImageViewHandle, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo,
    QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle,
    ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};
use windows::Win32::Foundation::{CloseHandle, E_OUTOFMEMORY, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE,
    D3D12_COMMAND_QUEUE_PRIORITY_NORMAL, D3D12_COMPARISON_FUNC_ALWAYS,
    D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_DEPTH_STENCIL_VIEW_DESC, D3D12_FENCE_FLAG_NONE,
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_MEMORY_POOL_UNKNOWN, D3D12_RANGE,
    D3D12_RENDER_TARGET_VIEW_DESC, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER,
    D3D12_SAMPLER_DESC, D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
    D3D12_TEXTURE_LAYOUT_UNKNOWN, D3D12_UNORDERED_ACCESS_VIEW_DESC, D3D12CreateDevice,
    ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Object, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
use windows::core::PCWSTR;

use crate::command::Dx12CommandEncoder;
use crate::descriptor::{Descriptors, Kind, Slot};
use crate::handle::{self, Owned, Owner};
use crate::instance::{AdapterRecord, InstanceInner, next_owner_id, not_yet};
use crate::view::Subresource;
use crate::{conv, view};

/// Node mask naming the single adapter node the seam models.
///
/// `crcbl-hal` has no multi-adapter vocabulary at all, so a linked-node rig is
/// described as its first node rather than wrongly. D3D12 wants a *mask* for
/// creation and visibility, not the index `D3D12_FEATURE_DATA_ARCHITECTURE1`
/// takes, which is why this is a bit and not a zero.
const FIRST_NODE: u32 = 1;

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
/// `location` is kept rather than re-derived from the resource: D3D12 will
/// answer `GetHeapProperties`, but that is a call across the ABI to learn
/// something this crate already decided, and `request_readback` will need to
/// tell the two host-visible heaps apart.
#[derive(Debug)]
struct BufferEntry {
    owner: u64,
    raw: ID3D12Resource,
    size: u64,
    location: MemoryLocation,
}

/// An image, plus everything a view of it has to be checked against.
///
/// The seam-side [`Format`] is kept rather than the `DXGI_FORMAT`, because a
/// sampled depth image's resource format is *typeless* — see
/// [`conv::resource_format`] — so comparing DXGI formats would compare a view's
/// concrete format against a storage format that deliberately has no type.
#[derive(Debug)]
struct ImageEntry {
    owner: u64,
    raw: ID3D12Resource,
    format: Format,
    image_type: ImageType,
    usage: ImageUsage,
    mip_levels: u32,
    /// Array layers, or depth slices for a volume — the two readings of
    /// [`Extent3d::depth_or_layers`](crcbl_hal::Extent3d::depth_or_layers), kept
    /// as one number because a view addresses both through the same
    /// `FirstArraySlice`/`FirstWSlice` pair and the seam names both
    /// `base_layer`.
    slices: u32,
    samples: u32,
}

/// The descriptors one image view owns.
///
/// Up to four, because D3D12 has no view object: a texture that is sampled and
/// rendered to needs a shader resource view *and* a render target view, written
/// into two different heaps. See [`crate::view`].
#[derive(Clone, Copy, Debug, Default)]
struct ViewDescriptors {
    shader_resource: Option<Slot>,
    unordered_access: Option<Slot>,
    render_target: Option<Slot>,
    depth_stencil: Option<Slot>,
}

impl ViewDescriptors {
    /// Every slot, so freeing cannot miss one the way four `if let`s can.
    fn slots(&self) -> impl Iterator<Item = Slot> {
        [
            self.shader_resource,
            self.unordered_access,
            self.render_target,
            self.depth_stencil,
        ]
        .into_iter()
        .flatten()
    }
}

/// An image view: the descriptors it wrote, and the resource they point into.
#[derive(Debug)]
struct ViewEntry {
    owner: u64,
    descriptors: ViewDescriptors,
    /// Held so the resource cannot outlive its own descriptors. The seam already
    /// obliges a caller to destroy every view before its image, but a descriptor
    /// is a raw address into a freed resource if it does not, and a refcount is
    /// cheaper than the debugging.
    _image: ID3D12Resource,
}

/// A sampler: one descriptor in the sampler heap.
#[derive(Debug)]
struct SamplerEntry {
    owner: u64,
    slot: Slot,
}

owned!(BufferEntry, ImageEntry, ViewEntry, SamplerEntry);

/// Every table the device owns, behind one lock.
#[derive(Debug)]
struct DeviceState {
    buffers: Pool<BufferEntry>,
    images: Pool<ImageEntry>,
    views: Pool<ViewEntry>,
    samplers: Pool<SamplerEntry>,
    descriptors: Descriptors,
}

/// The device's shared state.
///
/// No `unsafe impl Send`/`Sync`: every field is either plain data or a
/// `windows-rs` interface the bindings already declare both for. See the module
/// docs.
struct DeviceInner {
    /// Obligation 1: a `Device` may outlive its `Instance`, so the instance's
    /// state — the DXGI factory and the enumerated adapters — is kept alive here
    /// rather than borrowed. See [`InstanceInner`].
    _instance: Arc<InstanceInner>,
    raw: ID3D12Device,
    /// The one queue, `D3D12_COMMAND_LIST_TYPE_DIRECT`, which accepts graphics,
    /// compute and copy work. The compute and copy queue *types* are exactly
    /// [`Features::ASYNC_COMPUTE_QUEUE`] and [`Features::TRANSFER_QUEUE`], and
    /// neither is reported, so neither is created.
    queue: ID3D12CommandQueue,
    /// The fence [`Device::wait_idle`] signals through. One per device rather
    /// than one per call: a fence is a monotonic counter and reusing it is what
    /// makes each wait cheaper than the `CreateFence` it would otherwise need.
    idle_fence: ID3D12Fence,
    /// The last value [`Device::wait_idle`] signalled, behind the lock that
    /// keeps the signals ordered.
    ///
    /// **The lock is what makes the fence monotonic, and an atomic counter is
    /// not enough.** Reserving under an atomic gives two concurrent waiters two
    /// distinct values, but nothing then orders the two `Signal` calls: the
    /// waiter holding the higher value can reach the queue first, so the fence
    /// is set to that value and then back down to the lower one. The waiter that
    /// reserved the higher value samples the fence *after* the drop, sees less
    /// than its own value, arms an event for a value nothing will signal again,
    /// and blocks on it forever.
    idle_value: Mutex<u64>,
    caps: DeviceCaps,
    /// Which device this is, and the tag it stamps into every handle it issues.
    /// See [`crate::handle`].
    owner: Owner,
    state: Mutex<DeviceState>,
}

impl core::fmt::Debug for DeviceInner {
    /// The interfaces underneath print as raw pointers and say nothing a reader
    /// wants, so only the device's own identity is shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceInner")
            .field("id", &self.owner.id)
            .field("tier", &self.caps.tier())
            .finish_non_exhaustive()
    }
}

/// Puts a debug name on a D3D12 object, for PIX and the debug layer.
///
/// A failure is logged and not propagated: a name is a diagnostic, and losing
/// the whole resource because the driver would not take one would be absurd.
fn label_object(object: &ID3D12Object, label: &str) {
    // `SetName` takes a NUL-terminated UTF-16 string and copies it, so the
    // buffer only has to outlive the call.
    let wide: Vec<u16> = label.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `object` is a live COM interface this crate owns a reference to,
    // and `wide` is a NUL-terminated UTF-16 buffer that outlives the call — it
    // is dropped at the end of this function, after `SetName` has returned and
    // copied it.
    if let Err(error) = unsafe { object.SetName(PCWSTR::from_raw(wide.as_ptr())) } {
        log::debug!("crcbl-dx12: could not name an object \"{label}\": {error}");
    }
}

/// The heap properties for one of the seam's memory locations.
fn heap_properties(memory: MemoryLocation) -> D3D12_HEAP_PROPERTIES {
    D3D12_HEAP_PROPERTIES {
        Type: conv::heap_type(memory),
        // Both are `UNKNOWN` for every heap type except `CUSTOM`, which this
        // backend never asks for — D3D12 derives them from the heap type and
        // rejects a non-`UNKNOWN` value beside a standard one.
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: FIRST_NODE,
        VisibleNodeMask: FIRST_NODE,
    }
}

/// Turns a resource-creation failure into the seam's word for it.
///
/// Only `E_OUTOFMEMORY` becomes [`HalError::OutOfDeviceMemory`]. Everything else
/// keeps the driver's message, because the other failures here are descriptor
/// bugs — an illegal flag for a heap type, a dimension past the device's
/// ceiling — and reporting them as an allocation failure would send a reader
/// looking for memory pressure that is not there.
fn creation_error(what: &str, error: &windows::core::Error) -> HalError {
    if error.code() == E_OUTOFMEMORY {
        HalError::OutOfDeviceMemory
    } else {
        HalError::Backend(format!("{what} failed: {error}"))
    }
}

/// The D3D12 implementation of [`Device`].
#[derive(Debug)]
pub struct Dx12Device {
    inner: Arc<DeviceInner>,
}

impl Dx12Device {
    /// Opens a device on `record`'s adapter.
    ///
    /// `D3D12CreateDevice` at [`D3D_FEATURE_LEVEL_11_0`], the same floor
    /// enumeration used, for the same reason: which tier a device can run is
    /// decided by [`Features`], never by the feature level it was opened at, and
    /// asking for a higher one would refuse adapters this backend has already
    /// described. Both this call and `CreateCommandQueue` return before this
    /// function does, which is why
    /// [`Instance::request_device`](crcbl_hal::Instance::request_device)
    /// completes on its first poll.
    pub(crate) fn open(
        instance: Arc<InstanceInner>,
        record: &AdapterRecord,
        desc: &DeviceDesc<'_>,
    ) -> Result<Self, HalError> {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: `record.adapter` is a live `IDXGIAdapter1` the instance owns a
        // reference to, and `device` is a live `Option<ID3D12Device>` the call
        // writes through. A failure leaves it `None`, which is why it is read
        // back rather than assumed.
        unsafe { D3D12CreateDevice(&record.adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }
            .map_err(|error| {
                HalError::Backend(format!(
                    "D3D12CreateDevice failed for \"{}\": {error}",
                    record.info.name
                ))
            })?;
        // Enumeration already opened this adapter once, so a `None` here is not
        // a case anyone expects — but the binding's out-parameter is an `Option`
        // and a `None` would be a null pointer to call through.
        let raw = device.ok_or_else(|| {
            HalError::Backend(format!(
                "D3D12CreateDevice reported success and wrote no device for \"{}\"",
                record.info.name
            ))
        })?;

        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0,
            // `DISABLE_GPU_TIMEOUT` is the only other flag and it turns off the
            // driver's watchdog, which is how a hung shader becomes a hung
            // desktop instead of a device removal this backend can report.
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: FIRST_NODE,
        };
        // SAFETY: `queue_desc` is a live, fully initialised descriptor borrowed
        // for the duration of the call, and `ID3D12CommandQueue` is the IID
        // asked for.
        let queue: ID3D12CommandQueue = unsafe { raw.CreateCommandQueue(&queue_desc) }
            .map_err(|error| HalError::Backend(format!("CreateCommandQueue failed: {error}")))?;

        // SAFETY: `raw` is the device just created. `CreateFence` takes only
        // scalars and writes the interface it returns.
        let idle_fence: ID3D12Fence = unsafe { raw.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .map_err(|error| HalError::Backend(format!("CreateFence failed: {error}")))?;

        if let Some(label) = desc.label {
            // The device object names hardware rather than something a program
            // made, so the caller's name goes on the queue — which is where PIX
            // shows it.
            label_object(&queue, label);
        }

        // **D3D12 has no `pEnabledFeatures`.** A `VkDevice` reports only what was
        // switched on at creation, so `crcbl-vk` intersects the adapter's
        // features with what the caller asked for. An `ID3D12Device` can do
        // whatever `CheckFeatureSupport` answered, always, so `caps` is the
        // adapter's caps verbatim. `required_features` was checked in
        // `open_device` before this call; `optional_features` is satisfied by
        // construction, which is exactly what `DeviceDesc::optional_features`
        // documents ("check `Device::caps` afterwards to find out").
        let caps = record.info.caps;
        let owner = Owner::new(next_owner_id());
        let state = DeviceState {
            buffers: Pool::new(),
            images: Pool::new(),
            views: Pool::new(),
            samplers: Pool::new(),
            descriptors: Descriptors::new(&raw),
        };
        let inner = Arc::new(DeviceInner {
            _instance: instance,
            raw,
            queue,
            idle_fence,
            idle_value: Mutex::new(0),
            caps,
            owner,
            state: Mutex::new(state),
        });
        log::info!(
            "crcbl-dx12: opened \"{}\" (tier {:?})",
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

    /// The `ID3D12Device` underneath, for the tests that need to build
    /// something against it directly.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> &ID3D12Device {
        &self.inner.raw
    }

    /// Resolves a queue handle against *this* device.
    ///
    /// Obligation 3 covers queues too, and the three outcomes are kept apart for
    /// the same reason they are everywhere else: a handle carrying another
    /// device's tag is [`HalError::ForeignObject`] — the caller crossed two
    /// objects that never met — while one carrying no tag at all was never
    /// issued by any device and is [`HalError::InvalidHandle`].
    fn check_queue(&self, queue: QueueHandle) -> Result<(), HalError> {
        if queue == handle::queue(self.inner.owner, QueueKind::Graphics) {
            return Ok(());
        }
        Err(if handle::tag_of(queue) == 0 {
            HalError::invalid_handle("queue", queue)
        } else {
            HalError::ForeignObject {
                kind: "queue",
                bits: queue.to_bits(),
            }
        })
    }

    /// Checks an [`ImageDesc`] against this device's limits and D3D12's own
    /// rules, before anything is created.
    ///
    /// Split out of [`Device::create_image`] so the descriptor rules read as one
    /// list rather than as a preamble to a resource descriptor.
    fn check_image(&self, desc: &ImageDesc<'_>) -> Result<(), HalError> {
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
        // A texture cannot live on the upload or readback heap: D3D12 admits
        // only buffers there. The route to a GPU texture is a copy from a
        // host-visible buffer, which is the command slice, and the seam already
        // says a host-visible image is the wrong shape everywhere.
        if !matches!(desc.memory, MemoryLocation::DeviceLocal) {
            return Err(HalError::InvalidDescriptor(format!(
                "an image on {:?} is not creatable: D3D12's upload and readback heaps hold \
                 buffers only, so a texture is reached by copying from one (the DX12 command \
                 slice)",
                desc.memory
            )));
        }
        if !desc.samples.is_power_of_two() || desc.samples > limits.max_sample_count {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::samples is {} but this device supports powers of two up to {}",
                desc.samples, limits.max_sample_count
            )));
        }
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
        Ok(())
    }

    /// Whether a view of this shape can name a subrange of an image of that
    /// shape.
    ///
    /// **A view type D3D12 disagrees with is written, not rejected**: the four
    /// `Create*View` calls return `void`, so a 2D view of a volume produces a
    /// descriptor the debug layer objects to and the runtime ignores — a sample
    /// that reads nothing, with no error at any call. This is the check that
    /// turns it into one.
    ///
    /// The dimensionality rule is D3D12's. The layer rule is this seam's: a
    /// non-array view type has no `FirstArraySlice` field in any of the four
    /// descriptors, so a view of layer 3 through
    /// [`ImageViewType::D2`](crcbl_hal::ImageViewType::D2) would silently be a
    /// view of layer 0.
    fn check_view_type(
        image_type: ImageType,
        layers: u32,
        desc: &ImageViewDesc<'_>,
    ) -> Result<(), HalError> {
        use crcbl_hal::ImageViewType as V;
        let compatible = match image_type {
            ImageType::D1 => matches!(desc.view_type, V::D1),
            // A cube map's storage *is* a layered 2D image, which is why the
            // seam has a cube view type and no cube image type.
            ImageType::D2 => matches!(desc.view_type, V::D2 | V::D2Array | V::Cube | V::CubeArray),
            ImageType::D3 => matches!(desc.view_type, V::D3),
        };
        if !compatible {
            return Err(HalError::InvalidDescriptor(format!(
                "a {:?} view of a {image_type:?} image is not a D3D12 view: the view's \
                 dimensionality must be the image's, and a cube is a layered 2D image",
                desc.view_type
            )));
        }
        let arrayed = matches!(desc.view_type, V::D2Array | V::Cube | V::CubeArray);
        if !arrayed && desc.range.base_layer != 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "a {:?} view cannot start at layer {} of a {layers}-layer image: the descriptor \
                 has no first-slice field, so the layer would be dropped rather than honoured — \
                 use ImageViewType::D2Array",
                desc.view_type, desc.range.base_layer
            )));
        }
        Ok(())
    }

    /// Builds every descriptor an image view needs, or says which one D3D12 has
    /// no member for.
    ///
    /// The four are built *before* any heap slot is taken, so a combination
    /// D3D12 cannot express costs nothing and leaks nothing.
    fn build_views(
        entry_format: Format,
        usage: ImageUsage,
        desc: &ImageViewDesc<'_>,
        sub: Subresource,
    ) -> Result<BuiltViews, HalError> {
        let refuse = |what: &str| {
            HalError::InvalidDescriptor(format!(
                "D3D12 has no {what} for a {:?} view of a {entry_format:?} image with \
                 {} samples and {} layers",
                desc.view_type, sub.samples, sub.layer_count
            ))
        };
        let mut built = BuiltViews::default();
        if usage.contains(ImageUsage::SAMPLED) {
            // A depth image is stored typeless when it is sampled, and the
            // shader view names the depth plane's own format. See
            // `conv::depth_read_format`.
            let format = conv::depth_read_format(desc.format)
                .unwrap_or_else(|| conv::dxgi_format(desc.format));
            built.shader_resource = Some(
                view::shader_resource(format, desc.view_type, sub)
                    .ok_or_else(|| refuse("shader resource view"))?,
            );
        }
        if usage.contains(ImageUsage::STORAGE) {
            built.unordered_access = Some(
                view::unordered_access(conv::dxgi_format(desc.format), desc.view_type, sub)
                    .ok_or_else(|| refuse("unordered access view"))?,
            );
        }
        if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
            built.render_target = Some(
                view::render_target(conv::dxgi_format(desc.format), desc.view_type, sub)
                    .ok_or_else(|| refuse("render target view"))?,
            );
        }
        if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
            built.depth_stencil = Some(
                view::depth_stencil(conv::dxgi_format(desc.format), desc.view_type, sub)
                    .ok_or_else(|| refuse("depth stencil view"))?,
            );
        }
        if built.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "an image with no sampled, storage or attachment usage has no D3D12 view to \
                 create; a transfer-only image is addressed by the copy itself"
                    .to_string(),
            ));
        }
        Ok(built)
    }
}

/// The view descriptors an image view will write, before any heap slot exists.
///
/// No `Debug`: the four D3D12 structs are unions, and a derived formatter would
/// have to pick a member to print without knowing which one is live.
#[derive(Default)]
struct BuiltViews {
    shader_resource: Option<D3D12_SHADER_RESOURCE_VIEW_DESC>,
    unordered_access: Option<D3D12_UNORDERED_ACCESS_VIEW_DESC>,
    render_target: Option<D3D12_RENDER_TARGET_VIEW_DESC>,
    depth_stencil: Option<D3D12_DEPTH_STENCIL_VIEW_DESC>,
}

impl BuiltViews {
    fn is_empty(&self) -> bool {
        self.shader_resource.is_none()
            && self.unordered_access.is_none()
            && self.render_target.is_none()
            && self.depth_stencil.is_none()
    }
}

impl Device for Dx12Device {
    fn backend(&self) -> BackendKind {
        BackendKind::Dx12
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
    }

    /// The graphics queue, and only ever the graphics queue.
    ///
    /// D3D12's command list types are exactly this enum's three variants —
    /// which is what [`QueueKind`] says is why it is not named `QueueFamily` —
    /// so [`QueueKind::Compute`] and [`QueueKind::Transfer`] are creatable and
    /// deliberately not created. The seam ties each to a feature
    /// ([`Features::ASYNC_COMPUTE_QUEUE`], [`Features::TRANSFER_QUEUE`]), this
    /// backend reports neither, and a queue handed out for a feature that is not
    /// reported is the flag-without-a-call mistake in reverse.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        match kind {
            QueueKind::Graphics => Some(handle::queue(self.inner.owner, kind)),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    // --- resources ---

    /// Creates a buffer as a committed resource on the heap its memory location
    /// names.
    ///
    /// Committed rather than placed: a placed resource needs a suballocator over
    /// `ID3D12Heap`, which is what `gpu-allocator` is to `crcbl-vk`, and picking
    /// one is a decision with a lifetime and defragmentation story attached.
    /// **Committed is correct and slower**, which is the right direction for a
    /// slice with no frame loop to be slow in.
    ///
    /// There is no [`BufferUsage::DEVICE_ADDRESS`](crcbl_hal::BufferUsage::DEVICE_ADDRESS)
    /// check, and that is not an omission: every D3D12 buffer answers
    /// `GetGPUVirtualAddress`, which is why `crcbl_dx12::adapter` reports
    /// [`Features::BUFFER_DEVICE_ADDRESS`] unconditionally. A check against a
    /// feature that is always present is a check that cannot fail.
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "BufferDesc::size must be non-zero".to_string(),
            ));
        }
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            // Zero means "the default for this resource", which for a buffer is
            // the 64 KiB D3D12 requires.
            Alignment: 0,
            Width: desc.size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            // A buffer is untyped in D3D12; the format belongs to the view.
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            // The only layout a buffer may have.
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: conv::buffer_flags(desc.usage, desc.memory),
        };
        let properties = heap_properties(desc.memory);
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: both descriptors are live, fully initialised locals borrowed
        // for the duration of the call, and `resource` is a live `Option` the
        // call writes through. The initial state is the one D3D12 requires for
        // this heap type — see `conv::initial_state`, which is the only source
        // of it — and no optimised clear value is passed, which is legal for a
        // buffer and required for one.
        unsafe {
            self.inner.raw.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                conv::initial_state(desc.memory),
                None,
                &mut resource,
            )
        }
        .map_err(|error| creation_error("CreateCommittedResource (buffer)", &error))?;
        let raw = resource.ok_or_else(|| {
            HalError::Backend(
                "CreateCommittedResource reported success and wrote no buffer".to_string(),
            )
        })?;
        if let Some(label) = desc.label {
            label_object(&raw, label);
        }

        let handle = self.state().buffers.insert(BufferEntry {
            owner: self.inner.owner.id,
            raw,
            size: desc.size,
            location: desc.memory,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        let mut state = self.state();
        drop(handle::take_owned(
            &mut state.buffers,
            buffer,
            self.inner.owner,
        ));
    }

    /// Copies `data` into a host-visible buffer.
    ///
    /// # `DeviceLocal` is refused, never silently dropped
    ///
    /// A [`MemoryLocation::DeviceLocal`] buffer lives on the default heap, which
    /// `ID3D12Resource::Map` rejects — D3D12's only route into one is a copy
    /// from a buffer on the upload heap, and the copy command list is the DX12
    /// command slice. So this refuses with [`HalError::InvalidDescriptor`]
    /// naming the location, which is both what the seam documents ("
    /// `InvalidDescriptor` … if the buffer is not host-visible") and what
    /// `crcbl-vk` and `crcbl-mtl` answer for the same call, so the backends
    /// disagree about nothing.
    ///
    /// # The map ranges are not decoration
    ///
    /// `Map` is given an empty read range and `Unmap` the exact range written.
    /// Both matter on a discrete GPU: an empty read range tells the runtime the
    /// CPU will not read what is there, and a precise written range is what
    /// keeps a write-combined upload buffer from flushing bytes nobody touched.
    /// Passing `None` to either is the "assume everything" answer and is
    /// correct, slowly — which for a staging ring written every frame is the
    /// wrong default to start from.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.buffers, "buffer", buffer, self.inner.owner)?;
        if !entry.location.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a host-visible buffer; this one is {:?}, which D3D12 can only \
                 reach through a copy from an upload buffer (the DX12 command slice)",
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
        // A `D3D12_RANGE` is expressed in `usize`, so a 64-bit offset that does
        // not fit one is refused rather than truncated. Only reachable on a
        // 32-bit host, where the allocation could not have existed either.
        let begin = usize::try_from(offset).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "write_buffer offset {offset} does not fit this host's address space"
            ))
        })?;
        let written = D3D12_RANGE {
            Begin: begin,
            End: begin + data.len(),
        };
        // Begin == End says "the CPU read nothing", which is what this call
        // does.
        let read_nothing = D3D12_RANGE { Begin: 0, End: 0 };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on a host-visible heap — checked
        // above — and subresource 0 is the only one a buffer has. Both range
        // pointers name live locals, and `mapped` is a live pointer the call
        // writes through.
        unsafe { entry.raw.Map(0, Some(&read_nothing), Some(&mut mapped)) }
            .map_err(|error| HalError::Backend(format!("ID3D12Resource::Map failed: {error}")))?;
        if mapped.is_null() {
            return Err(HalError::Backend(
                "ID3D12Resource::Map reported success and wrote no pointer".to_string(),
            ));
        }
        // SAFETY: `mapped` points at `entry.size` bytes of this buffer's
        // storage, the range `begin..begin + data.len()` was bounds-checked
        // against that size immediately above, and the two regions cannot
        // overlap because `data` is a caller-owned slice and the destination is
        // the buffer's own mapping. The pointer does not escape this block, and
        // the device lock is held across the whole map/copy/unmap.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                mapped.cast::<u8>().add(begin),
                data.len(),
            );
        }
        // SAFETY: the matching `Unmap` for the `Map` above, on the same
        // subresource of the same live resource, with a range inside the
        // allocation.
        unsafe { entry.raw.Unmap(0, Some(&written)) };
        Ok(())
    }

    fn request_readback(&self, _desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        // Deliberately refused before the buffer handle is checked. A readback
        // is not a buffer read: the seam's contract is that it covers work
        // already submitted, which means waiting on a completion point and, on a
        // default-heap source, copying into a readback buffer. Both need a
        // command list.
        Err(not_yet("GPU readback (the DX12 command slice)"))
    }

    fn poll_readback(
        &self,
        _readback: ReadbackHandle,
        _out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        Err(not_yet("GPU readback (the DX12 command slice)"))
    }

    fn destroy_readback(&self, _readback: ReadbackHandle) {
        // Unreachable with a live handle: `request_readback` above issues none.
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        self.check_image(desc)?;
        let extent = desc.extent;
        let mip_levels = desc.mip_levels.max(1);
        let samples = desc.samples.max(1);
        // One field for two meanings, which is exactly what `depth_or_layers`
        // already is: D3D12 spells a volume's depth and an array's layer count
        // in the same 16-bit `DepthOrArraySize`, so there is nothing to choose
        // between here.
        //
        // `DepthOrArraySize` and `MipLevels` are 16-bit in the resource
        // descriptor. The limit checks above bound both, so this is the
        // conversion those checks make safe rather than a second policy.
        let depth_or_array = u16::try_from(extent.depth_or_layers).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "{} layers or slices do not fit D3D12's 16-bit DepthOrArraySize",
                extent.depth_or_layers
            ))
        })?;
        let mips = u16::try_from(mip_levels).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "{mip_levels} mip levels do not fit D3D12's 16-bit MipLevels"
            ))
        })?;

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: conv::resource_dimension(desc.image_type),
            Alignment: 0,
            Width: u64::from(extent.width),
            Height: extent.height,
            DepthOrArraySize: depth_or_array,
            MipLevels: mips,
            Format: conv::resource_format(desc.format, desc.usage),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: samples,
                // The default pattern. A non-zero quality names a vendor
                // pattern from `CheckFeatureSupport`, which is a per-format
                // query this backend does not make — and asking for one that
                // does not exist fails resource creation.
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: conv::image_flags(desc.usage),
        };
        let properties = heap_properties(desc.memory);
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: as `create_buffer` — live descriptors borrowed for the call,
        // a live out-parameter, and the initial state D3D12 requires for the
        // default heap. No optimised clear value is passed: it is a hint, and a
        // wrong one costs more than none at all.
        unsafe {
            self.inner.raw.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                conv::initial_state(desc.memory),
                None,
                &mut resource,
            )
        }
        .map_err(|error| creation_error("CreateCommittedResource (image)", &error))?;
        let raw = resource.ok_or_else(|| {
            HalError::Backend(
                "CreateCommittedResource reported success and wrote no image".to_string(),
            )
        })?;
        if let Some(label) = desc.label {
            label_object(&raw, label);
        }

        let handle = self.state().images.insert(ImageEntry {
            owner: self.inner.owner.id,
            raw,
            format: desc.format,
            image_type: desc.image_type,
            usage: desc.usage,
            mip_levels,
            slices: extent.depth_or_layers,
            samples,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_image(&self, image: ImageHandle) {
        let mut state = self.state();
        drop(handle::take_owned(
            &mut state.images,
            image,
            self.inner.owner,
        ));
    }

    /// Creates the descriptors a view of `desc.image` needs.
    ///
    /// # The view format must equal the image's, and that is a real gap
    ///
    /// [`ImageViewDesc::format`](crcbl_hal::ImageViewDesc::format) is documented
    /// as free to differ from its image's "for sRGB reinterpretation", and
    /// `crcbl-mtl` honours that. **This backend refuses it**, because D3D12
    /// offers two ways to allow the cast and neither is free:
    ///
    /// * Create every colour image with a **typeless** format. That works
    ///   everywhere and gives up the driver's format-specific compression on
    ///   every render target in the engine, for a capability almost none of them
    ///   use.
    /// * Query `D3D12_FEATURE_DATA_D3D12_OPTIONS3::CastingFullyTypedFormatSupported`
    ///   and allow the cast where it is reported. That makes the seam's promise
    ///   hold on some machines and not others, which is the "correct on one
    ///   class of machine" failure `crcbl-mtl`'s storage-mode decision was
    ///   written to avoid.
    ///
    /// Refusing uniformly is the only one of the three that is honest on every
    /// machine, so that is what this does — with the reason in the error. The
    /// depth spelling is *not* a reinterpretation and is handled underneath: a
    /// sampled depth image is stored typeless and its shader view names the
    /// depth plane's format, both derived from the one seam format the caller
    /// passed.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let mut state = self.state();
        // Everything the view needs is copied out of the entry here, so the
        // borrow of the image table ends before the descriptor heaps — in the
        // same lock — are borrowed mutably below.
        let (image, image_format, image_type, usage, levels, slices, samples) = {
            let entry = handle::lookup(&state.images, "image", desc.image, self.inner.owner)?;
            (
                entry.raw.clone(),
                entry.format,
                entry.image_type,
                entry.usage,
                entry.mip_levels,
                entry.slices,
                entry.samples,
            )
        };
        if desc.format != image_format {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of a {image_format:?} image cannot be created as {:?}: D3D12 permits a \
                 differing view format only on a typeless resource, and this backend stores \
                 colour images with their own format so the driver can compress them",
                desc.format
            )));
        }

        Self::check_view_type(image_type, slices, desc)?;

        let range = desc.range;
        if range.base_mip >= levels || range.base_layer >= slices {
            return Err(HalError::InvalidDescriptor(format!(
                "view starts at mip {} layer {}, and the image has {levels} mips and {slices} \
                 layers",
                range.base_mip, range.base_layer
            )));
        }
        let sub = Subresource {
            base_mip: range.base_mip,
            mip_count: resolve_count(range.mip_count, range.base_mip, levels),
            base_layer: range.base_layer,
            layer_count: resolve_count(range.layer_count, range.base_layer, slices),
            samples,
        };
        if sub.mip_count == 0 || sub.layer_count == 0 {
            return Err(HalError::InvalidDescriptor(
                "an image view covering no mip levels or no layers is not a view".to_string(),
            ));
        }
        let built = Self::build_views(image_format, usage, desc, sub)?;

        // Every slot is taken before any descriptor is written, so a heap that
        // will not grow leaves nothing half-created — and the ones already taken
        // go back rather than leaking.
        let mut descriptors = ViewDescriptors::default();
        let mut failure = None;
        for (wanted, kind, out) in [
            (
                built.shader_resource.is_some(),
                Kind::ShaderResource,
                &mut descriptors.shader_resource,
            ),
            (
                built.unordered_access.is_some(),
                Kind::ShaderResource,
                &mut descriptors.unordered_access,
            ),
            (
                built.render_target.is_some(),
                Kind::RenderTarget,
                &mut descriptors.render_target,
            ),
            (
                built.depth_stencil.is_some(),
                Kind::DepthStencil,
                &mut descriptors.depth_stencil,
            ),
        ] {
            if !wanted || failure.is_some() {
                continue;
            }
            match state.descriptors.allocate(&self.inner.raw, kind) {
                Ok(slot) => *out = Some(slot),
                Err(error) => failure = Some(error),
            }
        }
        if let Some(error) = failure {
            for slot in descriptors.slots() {
                state.descriptors.free(slot);
            }
            return Err(error);
        }

        // Writing a descriptor cannot fail and cannot report: every one of these
        // returns `void`. That is why the whole descriptor was validated in
        // `build_views` above.
        if let (Some(view_desc), Some(slot)) = (built.shader_resource, descriptors.shader_resource)
        {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: `image` is a live resource this device created, `view_desc`
            // is a fully initialised descriptor borrowed for the call, and `at`
            // is a descriptor this device just allocated out of a
            // `CBV_SRV_UAV` heap — which is the heap type
            // `CreateShaderResourceView` writes into.
            unsafe {
                self.inner
                    .raw
                    .CreateShaderResourceView(&image, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) =
            (built.unordered_access, descriptors.unordered_access)
        {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above. The counter resource is `None`, which is the
            // only legal value for a texture UAV — append/consume counters are
            // a buffer feature.
            unsafe {
                self.inner
                    .raw
                    .CreateUnorderedAccessView(&image, None, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) = (built.render_target, descriptors.render_target) {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above, into an `RTV` heap.
            unsafe {
                self.inner
                    .raw
                    .CreateRenderTargetView(&image, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) = (built.depth_stencil, descriptors.depth_stencil) {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above, into a `DSV` heap.
            unsafe {
                self.inner
                    .raw
                    .CreateDepthStencilView(&image, Some(&view_desc), at);
            }
        }

        let handle = state.views.insert(ViewEntry {
            owner: self.inner.owner.id,
            descriptors,
            _image: image,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.views, view, self.inner.owner) else {
            return;
        };
        for slot in entry.descriptors.slots() {
            state.descriptors.free(slot);
        }
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
        let Some(filter) = conv::filter(desc) else {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} with min {:?} / mag {:?} / mip {:?} is not a D3D12 filter: \
                 D3D12_FILTER_ANISOTROPIC is linear on all three, and leaving MaxAnisotropy set \
                 beside a point filter would sample one tap while claiming {}",
                desc.anisotropy, desc.min_filter, desc.mag_filter, desc.mip_filter, desc.anisotropy
            )));
        };

        // D3D12 takes an integer here; the seam's `f32` is a Vulkan-shaped
        // spelling of the same knob. Truncating rather than rounding keeps the
        // promise a limit makes: never sample with more taps than the caller
        // asked for.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let anisotropy = desc.anisotropy.min(cap) as u32;
        let sampler_desc = D3D12_SAMPLER_DESC {
            Filter: filter,
            AddressU: conv::address_mode(desc.address_mode[0]),
            AddressV: conv::address_mode(desc.address_mode[1]),
            AddressW: conv::address_mode(desc.address_mode[2]),
            // The seam has no LOD bias, and zero is "no bias" rather than a
            // value picked here.
            MipLODBias: 0.0,
            MaxAnisotropy: anisotropy.max(1),
            // A non-comparison sampler ignores this field, but the debug layer
            // reads a zero as `D3D12_COMPARISON_FUNC_NONE`, which is a sampler
            // feedback value and not a comparison at all. `ALWAYS` is the
            // neutral one.
            ComparisonFunc: desc
                .compare
                .map_or(D3D12_COMPARISON_FUNC_ALWAYS, conv::comparison_func),
            // Transparent black, which is what the seam's `ClampToBorder`
            // documents and what a shadow atlas needs; opaque black would put a
            // black frame around every clamped sample.
            BorderColor: [0.0, 0.0, 0.0, 0.0],
            MinLOD: desc.lod_min,
            MaxLOD: desc.lod_max,
        };

        let mut state = self.state();
        let slot = state.descriptors.allocate(&self.inner.raw, Kind::Sampler)?;
        let at = state.descriptors.cpu_handle(slot);
        // SAFETY: `sampler_desc` is a fully initialised descriptor borrowed for
        // the call, and `at` is a descriptor this device just allocated out of a
        // `SAMPLER` heap — the only heap type `CreateSampler` writes into.
        unsafe { self.inner.raw.CreateSampler(&sampler_desc, at) };

        let handle = state.samplers.insert(SamplerEntry {
            owner: self.inner.owner.id,
            slot,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.samplers, sampler, self.inner.owner) else {
            return;
        };
        state.descriptors.free(entry.slot);
    }

    // --- shaders and pipelines ---

    fn create_shader_module(
        &self,
        _desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // Not `ShaderCompilation`, which the seam reserves for an artifact this
        // backend tried and failed to compile. Nothing was tried: DXIL is not a
        // `crcbl-shaders` output yet, and `ShaderModuleDesc` carries no field
        // this backend reads.
        Err(not_yet("shader modules (the DX12 pipeline slice)"))
    }

    fn destroy_shader_module(&self, _module: ShaderModuleHandle) {}

    fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        Err(not_yet("bind group layouts (the DX12 binding slice)"))
    }

    fn destroy_bind_group_layout(&self, _layout: BindGroupLayoutHandle) {}

    fn create_bind_group(&self, _desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        Err(not_yet("bind groups (the DX12 binding slice)"))
    }

    fn update_bind_group(
        &self,
        _group: BindGroupHandle,
        _entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        Err(not_yet("bind groups (the DX12 binding slice)"))
    }

    fn destroy_bind_group(&self, _group: BindGroupHandle) {}

    fn create_pipeline_layout(
        &self,
        _desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        Err(not_yet("pipeline layouts (the DX12 binding slice)"))
    }

    fn destroy_pipeline_layout(&self, _layout: PipelineLayoutHandle) {}

    fn create_graphics_pipeline(
        &self,
        _desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Err(not_yet("graphics pipelines (the DX12 pipeline slice)"))
    }

    fn destroy_graphics_pipeline(&self, _pipeline: GraphicsPipelineHandle) {}

    fn create_compute_pipeline(
        &self,
        _desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        Err(not_yet("compute pipelines (the DX12 pipeline slice)"))
    }

    fn destroy_compute_pipeline(&self, _pipeline: ComputePipelineHandle) {}

    // --- queries ---

    fn create_query_set(&self, _desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        Err(not_yet("query sets (the DX12 query slice)"))
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
        // `ID3D12Fence` is the seam's timeline semaphore almost verbatim, and
        // `wait_idle` below already drives one. What is missing is the other
        // half — `ID3D12CommandQueue::Wait`, and a submission to attach it to —
        // so a semaphore handed out now would be a counter nothing can signal
        // from the GPU.
        Err(not_yet("semaphores (the DX12 command slice)"))
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
        Err(not_yet("semaphores (the DX12 command slice)"))
    }

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// D3D12 has no `vkDeviceWaitIdle`. What it has is the fence idiom, and this
    /// is it: signal the queue's fence with a value nobody else took, then wait
    /// for the fence to reach it. A queue executes in submission order, so the
    /// signal lands after everything submitted before it.
    ///
    /// # The wait uses a real event, and checks that it waited
    ///
    /// `SetEventOnCompletion` accepts a null handle and is documented to block
    /// until the value is reached, which would be less code. The event is used
    /// anyway because it is the version that can be *checked*:
    /// `WaitForSingleObject` reports which way it returned, so a wait that did
    /// not happen is an `Err` here rather than a `wait_idle` that silently does
    /// not wait — and a silent one is worse than none, because it would be
    /// trusted at shutdown.
    ///
    /// The event is created and closed inside the call rather than kept on the
    /// device. Two reasons, and the first is enough: an auto-reset event shared
    /// between two concurrent waiters lets one consume the other's signal, and a
    /// Win32 `HANDLE` is a raw pointer that `windows-rs` declares neither `Send`
    /// nor `Sync`, so storing one would cost this module the marker impl it
    /// otherwise does not need.
    ///
    /// It is a real wait today even though [`Device::submit`] still refuses: the
    /// queue is real and the fence is real, and this is the call that proves
    /// both work. When submission lands, nothing here changes.
    fn wait_idle(&self) -> Result<(), HalError> {
        // Reserving the value and signalling it happen together, under one lock,
        // so the queue receives the signals in increasing order. See
        // [`DeviceInner::idle_value`] for the deadlock the lock rules out. The
        // wait itself is outside it: two waiters block concurrently, they just
        // do not race to signal.
        let value = {
            let mut next = self
                .inner
                .idle_value
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *next += 1;
            // SAFETY: `queue` and `idle_fence` are live interfaces this device
            // owns, created together in `open`. `Signal` takes the fence by
            // reference and a scalar.
            unsafe { self.inner.queue.Signal(&self.inner.idle_fence, *next) }.map_err(|error| {
                HalError::DeviceLost(format!("ID3D12CommandQueue::Signal failed: {error}"))
            })?;
            *next
        };

        // SAFETY: `idle_fence` is live; `GetCompletedValue` reads no pointer of
        // ours and returns a `u64` by value.
        if unsafe { self.inner.idle_fence.GetCompletedValue() } < value {
            // SAFETY: no security attributes, auto-reset, initially unsignalled,
            // unnamed. Every argument is a scalar or a null pointer the API
            // documents as optional.
            let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
                .map_err(|error| HalError::DeviceLost(format!("CreateEventW failed: {error}")))?;
            // SAFETY: `event` is the handle just created and `value` is the one
            // this call signalled. The runtime signals the event when the fence
            // reaches the value, including immediately if it already has.
            let armed = unsafe { self.inner.idle_fence.SetEventOnCompletion(value, event) };
            let waited = if armed.is_ok() {
                // SAFETY: `event` is a live event handle owned by this call.
                Some(unsafe { WaitForSingleObject(event, INFINITE) })
            } else {
                None
            };
            // SAFETY: `event` is this call's handle and is not used again. Closed
            // on both paths, so a failed `SetEventOnCompletion` leaks nothing.
            if let Err(error) = unsafe { CloseHandle(event) } {
                log::debug!("crcbl-dx12: could not close a wait_idle event: {error}");
            }
            armed.map_err(|error| {
                HalError::DeviceLost(format!("SetEventOnCompletion failed: {error}"))
            })?;
            if waited != Some(WAIT_OBJECT_0) {
                return Err(HalError::DeviceLost(format!(
                    "waiting for fence value {value} returned {waited:?} rather than \
                     WAIT_OBJECT_0"
                )));
            }
        }

        // SAFETY: as above.
        let completed = unsafe { self.inner.idle_fence.GetCompletedValue() };
        if completed < value {
            return Err(HalError::DeviceLost(format!(
                "the wait returned with the fence at {completed}, short of {value}"
            )));
        }
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, _desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(Dx12CommandEncoder::new())
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
        Err(not_yet("submission (the DX12 command slice)"))
    }

    // --- presentation ---

    fn create_swapchain(&self, _desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the DX12 swapchain slice)",
        )))
    }

    fn reconfigure_swapchain(
        &self,
        _swapchain: SwapchainHandle,
        _desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the DX12 swapchain slice)",
        )))
    }

    fn destroy_swapchain(&self, _swapchain: SwapchainHandle) {}

    fn acquire_next_frame(
        &self,
        _swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "swapchains (the DX12 swapchain slice)",
        )))
    }

    fn present(&self, _queue: QueueHandle, _present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        Err(SurfaceError::Hal(not_yet(
            "presentation (the DX12 swapchain slice)",
        )))
    }
}

/// Resolves a seam subresource count against the object's real extent.
///
/// [`ImageSubresourceRange::ALL`] means "every remaining one", and a count that
/// runs past the end is clamped rather than refused — the request is
/// satisfiable, just wider than the object, and D3D12 would read the raw number
/// as a real count of levels that do not exist.
fn resolve_count(requested: u32, base: u32, total: u32) -> u32 {
    let remaining = total.saturating_sub(base);
    if requested == ImageSubresourceRange::ALL {
        return remaining;
    }
    requested.min(remaining)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crcbl_core::Handle;
    use crcbl_hal::{
        BindingResource, BufferUsage, CompareOp, CompositeAlpha, Extent3d, FilterMode, ImageAspect,
        ImageViewType, Instance, MultisampleState, PresentMode, PrimitiveState, QueryKind,
        SemaphoreKind, SemaphoreWait, ShaderEntry,
    };

    use crate::Dx12Instance;
    use crate::instance::tests::{desc as device_desc, open as open_instance};

    /// Every [`MemoryLocation`] the seam has, so the buffer tests cover all
    /// three rather than the one that was convenient.
    const LOCATIONS: &[MemoryLocation] = &[
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ];

    /// A device, opened through this crate's own type so a test can reach the
    /// pools and heaps underneath it.
    pub(crate) fn open_device() -> (Dx12Instance, Dx12Device) {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "a Windows machine has at least WARP");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a D3D12 device opens with no required features");
        (instance, device)
    }

    /// A handle no device ever issued.
    ///
    /// Generation 1 — the lowest a `Handle` admits — and index 0, so the device
    /// tag `crcbl_dx12::handle` reads out of the index half is `0`, which that
    /// module reserves for "nobody". Every entry point that takes a handle and
    /// still refuses is offered one of these, and it is the same claim each
    /// time, so it is written once.
    fn unissued<T>() -> Handle<T> {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    fn buffer(size: u64, memory: MemoryLocation) -> BufferDesc<'static> {
        BufferDesc {
            label: Some("crcbl-dx12 test buffer"),
            size,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory,
        }
    }

    fn image(format: Format, usage: ImageUsage, extent: Extent3d) -> ImageDesc<'static> {
        ImageDesc {
            label: Some("crcbl-dx12 test image"),
            image_type: ImageType::D2,
            extent,
            format,
            mip_levels: 1,
            samples: 1,
            usage,
            memory: MemoryLocation::DeviceLocal,
        }
    }

    fn whole(image: ImageHandle, format: Format) -> ImageViewDesc<'static> {
        ImageViewDesc {
            label: Some("crcbl-dx12 test view"),
            image,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        }
    }

    /// Reads a host-visible buffer's bytes straight out of D3D12, bypassing the
    /// seam — which has no read path yet, and which is exactly why this is the
    /// only way to observe that `write_buffer` wrote anything.
    fn read_back(device: &Dx12Device, handle: BufferHandle, len: usize) -> Vec<u8> {
        let state = device.state();
        let entry = handle::lookup(&state.buffers, "buffer", handle, device.inner.owner)
            .expect("the buffer is live and this device's");
        assert!(entry.location.is_mappable(), "not a readable buffer");
        assert!(len as u64 <= entry.size, "reading past the buffer");
        let read = D3D12_RANGE { Begin: 0, End: len };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on a host-visible heap, checked
        // just above, and subresource 0 is the only one a buffer has. The read
        // range is inside the allocation, and `mapped` is a live local the call
        // writes through.
        unsafe { entry.raw.Map(0, Some(&read), Some(&mut mapped)) }.expect("mapping for a read");
        assert!(!mapped.is_null(), "Map wrote no pointer");
        // SAFETY: `mapped` covers `entry.size` bytes of a live host-visible
        // allocation, `len` was just asserted to be within it, and the read
        // happens under the device lock with no GPU work in flight.
        let bytes = unsafe { core::slice::from_raw_parts(mapped.cast::<u8>(), len) }.to_vec();
        // SAFETY: the matching `Unmap`. Nothing was written, which is what the
        // empty written range says.
        unsafe {
            entry.raw.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        }
        bytes
    }

    /// The device opens, says which backend it is, and has exactly the queue
    /// this backend creates.
    #[test]
    fn a_device_reports_dx12_and_one_graphics_queue() {
        let (_instance, device) = open_device();
        assert_eq!(device.backend(), BackendKind::Dx12);
        assert!(
            device.queue(QueueKind::Graphics).is_some(),
            "every device creates a DIRECT queue"
        );
        assert!(
            device.queue(QueueKind::Compute).is_none(),
            "no COMPUTE queue is created while ASYNC_COMPUTE_QUEUE is unreported"
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

    /// `caps` is the adapter's caps: D3D12 enables nothing and disables nothing,
    /// so a device that reported less than its adapter would be lying about
    /// hardware it can use.
    #[test]
    fn device_caps_match_the_adapter_they_came_from() {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a D3D12 device opens with no required features");
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
        let error = device
            .create_buffer(&buffer(0, MemoryLocation::HostUpload))
            .expect_err("a zero-byte buffer is not a buffer");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
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
    /// location D3D12 cannot map.
    #[test]
    fn write_buffer_writes_host_visible_memory_and_refuses_device_local() {
        let (_instance, device) = open_device();
        let readback = device
            .create_buffer(&buffer(16, MemoryLocation::HostReadback))
            .expect("a readback buffer");

        // Two writes, so the result is fully determined whatever D3D12 left in
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
            .expect_err("a default-heap buffer cannot be mapped");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("DeviceLocal"), "{text}");
        assert!(
            text.contains("copy"),
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
    /// Device B is given a buffer of its own first, and that is the whole design
    /// of this test: without the device tag in the handle, A's first handle and
    /// B's first handle are bit-identical, so B would resolve A's handle to B's
    /// own buffer, find the owner matching, and write into the wrong object with
    /// no error anywhere.
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
        let handle = device
            .create_image(&ImageDesc {
                mip_levels: extent.full_mip_levels(ImageType::D2),
                ..image(
                    Format::Rgba8Unorm,
                    ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                    extent,
                )
            })
            .expect("a 2D colour image");

        let view = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect("a whole-image view");

        // One mip of the chain, which is the case the ALL sentinel resolution
        // has to get right.
        let one_mip = device
            .create_image_view(&ImageViewDesc {
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 1,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
                ..whole(handle, Format::Rgba8Unorm)
            })
            .expect("a single-mip view");
        assert_ne!(view, one_mip);

        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("crcbl-dx12 test sampler"),
                ..SamplerDesc::default()
            })
            .expect("the seam's default sampler");

        device.destroy_image_view(one_mip);
        device.destroy_image_view(view);
        device.destroy_sampler(sampler);
        device.destroy_image(handle);

        let error = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect_err("the image was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "image"),
            "{error:?}"
        );
    }

    /// **Every descriptor a view took goes back when the view is destroyed.**
    ///
    /// The key is the descriptor *address*, which is what a leak moves and a
    /// correct free does not: creating and destroying the same view repeatedly
    /// must keep handing back the same slot. Counting views would pass whether
    /// or not the heap slots were ever returned, and the heap grows a chunk at a
    /// time, so a leak of a handful would not even show up as a failure to
    /// allocate.
    #[test]
    fn destroying_a_view_returns_every_descriptor_it_took() {
        let (_instance, device) = open_device();
        // Sampled *and* rendered to, so the view really owns two descriptors in
        // two different heaps — the case a free that only released one leaves
        // behind.
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                Extent3d::d2(16, 16),
            ))
            .expect("a colour image");

        let mut addresses = Vec::new();
        for round in 0..4 {
            let view = device
                .create_image_view(&whole(handle, Format::Rgba8Unorm))
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));
            {
                let mut state = device.state();
                let local = handle::local::<ViewEntry, _>("view", view, device.inner.owner)
                    .expect("this device's own handle");
                let entry = state
                    .views
                    .get(local)
                    .expect("the view is live")
                    .descriptors;
                let srv = entry
                    .shader_resource
                    .expect("a SAMPLED image view owns a shader resource view");
                let rtv = entry
                    .render_target
                    .expect("a COLOR_ATTACHMENT image view owns a render target view");
                addresses.push((
                    state.descriptors.cpu_handle(srv).ptr,
                    state.descriptors.cpu_handle(rtv).ptr,
                ));
            }
            device.destroy_image_view(view);
        }
        assert_eq!(addresses.len(), 4, "the loop did not run");
        let first = addresses[0];
        for (round, pair) in addresses.iter().enumerate() {
            assert_eq!(
                *pair, first,
                "round {round} took a different descriptor pair, so the previous round's was \
                 never freed"
            );
        }
        device.destroy_image(handle);
    }

    /// A sampler's descriptor comes back too, by the same test.
    #[test]
    fn destroying_a_sampler_returns_its_descriptor() {
        let (_instance, device) = open_device();
        let mut addresses = Vec::new();
        for round in 0..4 {
            let sampler = device
                .create_sampler(&SamplerDesc::default())
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));
            {
                let mut state = device.state();
                let local =
                    handle::local::<SamplerEntry, _>("sampler", sampler, device.inner.owner)
                        .expect("this device's own handle");
                let slot = state.samplers.get(local).expect("the sampler is live").slot;
                addresses.push(state.descriptors.cpu_handle(slot).ptr);
            }
            device.destroy_sampler(sampler);
        }
        assert_eq!(addresses.len(), 4, "the loop did not run");
        assert!(
            addresses.iter().all(|&ptr| ptr == addresses[0]),
            "a sampler descriptor was not freed: {addresses:?}"
        );
    }

    /// A sampled depth image is stored typeless and read through the depth
    /// plane's own format, which is what makes a shadow map creatable at all.
    ///
    /// The falsifying value is a backend that stored `D32_FLOAT`: D3D12 refuses
    /// a shader resource view on a fully-typed depth resource, and refuses it by
    /// writing nothing and returning `void`, so this would pass creation and
    /// sample black.
    #[test]
    fn a_sampled_depth_image_gets_both_of_its_views() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::D32Float,
                ImageUsage::SAMPLED | ImageUsage::DEPTH_STENCIL_ATTACHMENT,
                Extent3d::d2(32, 32),
            ))
            .expect("a sampled depth image");

        let view = device
            .create_image_view(&whole(handle, Format::D32Float))
            .expect("a depth view of a depth image");
        {
            let state = device.state();
            let local = handle::local::<ViewEntry, _>("view", view, device.inner.owner)
                .expect("this device's own handle");
            let descriptors = state
                .views
                .get(local)
                .expect("the view is live")
                .descriptors;
            assert!(
                descriptors.shader_resource.is_some(),
                "a SAMPLED depth image must get a shader resource view"
            );
            assert!(
                descriptors.depth_stencil.is_some(),
                "a DEPTH_STENCIL_ATTACHMENT image must get a depth stencil view"
            );
            assert!(descriptors.render_target.is_none());
            assert!(descriptors.unordered_access.is_none());
        }
        device.destroy_image_view(view);
        device.destroy_image(handle);
    }

    /// The image descriptors D3D12 cannot satisfy are refused before anything is
    /// created.
    #[test]
    fn images_d3d12_cannot_create_are_refused_by_name() {
        let (_instance, device) = open_device();
        let base = image(
            Format::Rgba8Unorm,
            ImageUsage::SAMPLED,
            Extent3d::d2(16, 16),
        );

        let refusals: Vec<(&str, ImageDesc<'_>)> = vec![
            (
                "a zero extent",
                ImageDesc {
                    extent: Extent3d::d2(0, 16),
                    ..base
                },
            ),
            (
                "no usage at all",
                ImageDesc {
                    usage: ImageUsage::empty(),
                    ..base
                },
            ),
            (
                "a host-visible image",
                ImageDesc {
                    memory: MemoryLocation::HostUpload,
                    ..base
                },
            ),
            (
                "a sample count that is not a power of two",
                ImageDesc { samples: 3, ..base },
            ),
            (
                "a multisampled mip chain",
                ImageDesc {
                    samples: 4,
                    mip_levels: 4,
                    ..base
                },
            ),
            (
                "an extent past the device's ceiling",
                ImageDesc {
                    extent: Extent3d::d2(device.caps().limits.max_image_2d + 1, 16),
                    ..base
                },
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, desc) in refusals {
            let error = device
                .create_image(&desc)
                .err()
                .unwrap_or_else(|| panic!("{what} was accepted"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }

    /// A view format that differs from its image's is refused, with the reason
    /// in the message — this backend's one documented divergence from
    /// `crcbl-mtl`.
    #[test]
    fn a_differing_view_format_is_refused_and_says_why() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED,
                Extent3d::d2(32, 32),
            ))
            .expect("a linear image");

        let error = device
            .create_image_view(&ImageViewDesc {
                format: Format::Rgba8UnormSrgb,
                ..whole(handle, Format::Rgba8Unorm)
            })
            .expect_err("D3D12 needs a typeless resource to cast between fully typed formats");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("typeless"), "{text}");

        // The same-format view is still fine, so the refusal is about the cast
        // and not about the image.
        let view = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect("a same-format view");
        device.destroy_image_view(view);
        device.destroy_image(handle);
    }

    /// An image with only transfer usage has no D3D12 view, and says so rather
    /// than handing back a view that names nothing.
    #[test]
    fn a_transfer_only_image_has_no_view_to_create() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
                Extent3d::d2(16, 16),
            ))
            .expect("a transfer-only image");
        let error = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect_err("there is no descriptor kind for a transfer-only image");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        device.destroy_image(handle);
    }

    /// A view whose shape does not match its image's is refused, because D3D12
    /// would write it and say nothing.
    ///
    /// The two cases are the two silent losses: a 3D view of a 2D image is a
    /// descriptor the runtime ignores, and a non-array view starting at a
    /// non-zero layer has no field to carry the layer, so it would quietly view
    /// layer zero.
    #[test]
    fn a_view_whose_shape_disagrees_with_its_image_is_refused() {
        let (_instance, device) = open_device();
        let layered = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED,
                Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_layers: 6,
                },
            ))
            .expect("a six-layer 2D image");

        let wrong_shape = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D3,
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect_err("a volume view of a layered 2D image is not a D3D12 view");
        assert!(
            matches!(wrong_shape, HalError::InvalidDescriptor(_)),
            "{wrong_shape:?}"
        );

        let dropped_layer = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D2,
                range: ImageSubresourceRange {
                    base_layer: 3,
                    layer_count: 1,
                    ..ImageSubresourceRange::all(Format::Rgba8Unorm)
                },
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect_err("a D2 view has no first-slice field to carry layer 3");
        let HalError::InvalidDescriptor(text) = dropped_layer else {
            panic!("expected InvalidDescriptor, got {dropped_layer:?}");
        };
        assert!(
            text.contains("D2Array"),
            "the refusal must say the fix: {text}"
        );

        // The array view of the same layer really is accepted, so the two
        // refusals above are about the shape and not about the image.
        let view = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D2Array,
                range: ImageSubresourceRange {
                    base_layer: 3,
                    layer_count: 1,
                    ..ImageSubresourceRange::all(Format::Rgba8Unorm)
                },
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect("an array view can start where it likes");
        device.destroy_image_view(view);

        // And a cube view of the same six layers, which is the case the seam has
        // a view type for and no image type.
        let cube = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::Cube,
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect("six layers are a cube");
        device.destroy_image_view(cube);
        device.destroy_image(layered);
    }

    /// Anisotropy is bounded on both sides, and a point filter beside it is
    /// refused rather than silently dropped.
    #[test]
    fn samplers_reject_anisotropy_outside_the_reported_cap() {
        let (_instance, device) = open_device();
        let cap = device.caps().limits.max_sampler_anisotropy;
        assert!(
            cap > 1.0,
            "this backend reports SAMPLER_ANISOTROPY, so the cap must exceed the value that \
             disables it"
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

        let point_with_anisotropy = device.create_sampler(&SamplerDesc {
            anisotropy: cap,
            mip_filter: FilterMode::Nearest,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(point_with_anisotropy, Err(HalError::InvalidDescriptor(_))),
            "D3D12 has no point-filtered anisotropic sampler: {point_with_anisotropy:?}"
        );

        // And a comparison sampler at the cap is accepted — the shadow-map
        // shape, with the reversed-Z comparison the seam documents.
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("shadow"),
                anisotropy: cap,
                compare: Some(CompareOp::Greater),
                ..SamplerDesc::default()
            })
            .expect("a comparison sampler at the reported cap");
        device.destroy_sampler(sampler);
    }

    /// `wait_idle` really waits: it signals the queue's fence and blocks on it.
    /// A queue or fence that could not do either fails here.
    #[test]
    fn wait_idle_signals_the_queue_fence_and_waits_for_it() {
        let (_instance, device) = open_device();
        device
            .wait_idle()
            .expect("an empty queue reaches the value");
        // Twice, because a fence that only works once is a fence whose value
        // never advanced.
        device.wait_idle().expect("and again");
        // The value really moved, which is what makes the second wait a wait
        // rather than a repeat of the first.
        // SAFETY: `idle_fence` is live and `GetCompletedValue` returns a `u64`
        // by value.
        let completed = unsafe { device.inner.idle_fence.GetCompletedValue() };
        assert!(
            completed >= 2,
            "two waits left the fence at {completed}, so at least one did not signal"
        );
    }

    /// Concurrent `wait_idle`s all return, and the fence ends where the count
    /// says it should.
    ///
    /// **What this does and does not prove.** The final value pins the
    /// mechanism: every call signalled exactly once, so a reservation that
    /// skipped or repeated a value fails here deterministically. The *ordering*
    /// half — that the signals reach the queue in increasing order, which is
    /// what stops a waiter arming an event for a value the fence has already
    /// passed and dropped below — is a race, and a passing run is not evidence
    /// the race cannot happen. Its failure mode is a hang rather than an
    /// assertion, caught by `slow-timeout` in `.config/nextest.toml`.
    #[test]
    fn concurrent_waits_each_signal_once_and_all_return() {
        let (_instance, device) = open_device();
        let before = {
            // SAFETY: `idle_fence` is live and `GetCompletedValue` returns a
            // `u64` by value.
            unsafe { device.inner.idle_fence.GetCompletedValue() }
        };

        let waiters: u64 = 8;
        std::thread::scope(|scope| {
            for index in 0..waiters {
                let device = &device;
                scope.spawn(move || {
                    device
                        .wait_idle()
                        .unwrap_or_else(|error| panic!("waiter {index}: {error:?}"));
                });
            }
        });

        // SAFETY: as above.
        let after = unsafe { device.inner.idle_fence.GetCompletedValue() };
        assert_eq!(
            after,
            before + waiters,
            "the fence moved by {} across {waiters} waits, so a value was skipped or reused",
            after - before
        );
    }

    /// Every slice that has not arrived still refuses, by name — so none of them
    /// can be half-implemented without this saying so.
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
                        msl: None,
                    })
                    .expect_err("no DXIL path yet"),
            ),
            (
                "bind group layouts",
                device
                    .create_bind_group_layout(&BindGroupLayoutDesc {
                        label: None,
                        entries: &[],
                    })
                    .expect_err("no root signatures yet"),
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
                        kind: QueryKind::Timestamp,
                        count: 1,
                    })
                    .expect_err("no query heaps yet"),
            ),
            (
                "semaphores",
                device
                    .create_semaphore(&SemaphoreDesc {
                        label: None,
                        kind: SemaphoreKind::Timeline { initial_value: 0 },
                    })
                    .expect_err("no shared fence yet"),
            ),
            (
                "readback",
                device
                    .request_readback(&ReadbackDesc {
                        label: None,
                        buffer: unissued(),
                        offset: 0,
                        size: 4,
                        after: None,
                    })
                    .expect_err("no command list yet"),
            ),
            (
                "bind groups",
                device
                    .create_bind_group(&BindGroupDesc {
                        label: None,
                        layout: unissued(),
                        entries: &[],
                        variable_count: None,
                    })
                    .expect_err("no descriptor tables yet"),
            ),
            (
                "bind group updates",
                device
                    .update_bind_group(
                        unissued(),
                        &[BindGroupEntry {
                            binding: 0,
                            array_index: 0,
                            resource: BindingResource::Sampler(unissued()),
                        }],
                    )
                    .expect_err("no descriptor tables yet"),
            ),
            (
                "graphics pipelines",
                device
                    .create_graphics_pipeline(&GraphicsPipelineDesc {
                        label: None,
                        layout: unissued(),
                        vertex: ShaderEntry {
                            module: unissued(),
                            entry_point: "vs_main",
                        },
                        fragment: None,
                        primitive: PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: MultisampleState::default(),
                        color_targets: &[],
                    })
                    .expect_err("no pipeline state objects yet"),
            ),
            (
                "compute pipelines",
                device
                    .create_compute_pipeline(&ComputePipelineDesc {
                        label: None,
                        layout: unissued(),
                        compute: ShaderEntry {
                            module: unissued(),
                            entry_point: "cs_main",
                        },
                    })
                    .expect_err("no pipeline state objects yet"),
            ),
            (
                "semaphore waits",
                device
                    .wait_semaphores(
                        &[SemaphoreWait {
                            semaphore: unissued(),
                            value: 1,
                        }],
                        0,
                    )
                    .expect_err("no fence a caller can hold yet"),
            ),
            (
                "readback polls",
                device
                    .poll_readback(unissued(), &mut [0u8; 4])
                    .expect_err("no readback was ever issued"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, error) in &refusals {
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if *backend == BackendKind::Dx12),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(text.contains("dx12"), "{what}: {text}");
            assert!(
                text.contains("DX12") && text.contains("slice"),
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
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );

        // Submission checks the queue handle first and refuses second.
        let error = device
            .submit(queue, &SubmitInfo::new(&[]))
            .expect_err("nothing can be submitted yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
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

        let untagged = unissued();
        let error = device
            .submit(untagged, &SubmitInfo::new(&[]))
            .expect_err("no device ever issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }

    /// **The presentation half refuses through [`SurfaceError`], and the words
    /// survive the conversion.**
    ///
    /// These four are the entry points whose refusal takes a different route
    /// out: [`not_yet`] builds a [`HalError`] and `SurfaceError::Hal` wraps it,
    /// so a caller matching on `SurfaceError` sees a `Hal` arm rather than
    /// [`SurfaceError::Lost`] or [`SurfaceError::OutOfDate`]. Nothing asserted
    /// that before — and the two failures it admits are the ones a caller cannot
    /// recover from: an unwritten call that *panicked* instead of returning
    /// looks the same as one nobody tested, and a refusal miscast as `OutOfDate`
    /// would put a render loop into an unending reconfigure.
    ///
    /// Every handle offered is one no device issued, because none can be: both
    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface) and
    /// `create_swapchain` refuse. So the refusal has to arrive before any handle
    /// is resolved, which is what makes these reachable at all.
    #[test]
    fn the_presentation_slice_refuses_through_surface_error_and_names_dx12() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let swapchain = SwapchainDesc {
            label: Some("crcbl-dx12 test swapchain"),
            surface: unissued(),
            format: Format::Bgra8UnormSrgb,
            extent: (320, 200),
            image_count: 3,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        };

        let refusals: Vec<(&str, SurfaceError)> = vec![
            (
                "swapchain creation",
                device
                    .create_swapchain(&swapchain)
                    .expect_err("no DXGI swapchain yet"),
            ),
            (
                "swapchain reconfiguration",
                device
                    .reconfigure_swapchain(unissued(), &swapchain)
                    .expect_err("there is no swapchain to reconfigure"),
            ),
            (
                "acquire",
                device
                    .acquire_next_frame(unissued())
                    .expect_err("no swapchain hands out a frame"),
            ),
            (
                "present",
                device
                    .present(
                        queue,
                        &PresentInfo {
                            swapchain: unissued(),
                            waits: &[],
                        },
                    )
                    .expect_err("there is nothing to present"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, error) in &refusals {
            let SurfaceError::Hal(hal) = error else {
                panic!("{what}: a slice that has not landed is not a surface condition: {error:?}");
            };
            assert!(
                matches!(hal, HalError::Unsupported { backend, .. } if *backend == BackendKind::Dx12),
                "{what}: {hal:?}"
            );
            // `SurfaceError::Hal` is transparent, so the refusal's own words are
            // what a caller prints. A wrapper that swallowed them would leave
            // the caller with a message naming no backend and no slice.
            let text = error.to_string();
            assert!(text.contains("dx12"), "{what}: {text}");
            assert!(
                text.contains("DX12") && text.contains("slice"),
                "{what}: {text}"
            );
        }
    }

    /// **A handle that cannot resolve is not the same answer as a slice that has
    /// not landed**, and these are the two entry points that say so.
    ///
    /// The crate docs draw the line: `Unsupported` for a call this backend has
    /// not written, and [`HalError::InvalidHandle`] for anything it can genuinely
    /// diagnose. `query_results` and `semaphore_value` are on the far side of it
    /// — they take a handle, no query set or semaphore exists to have issued
    /// one, so "you handed me something that never resolved" is both true and
    /// more useful than "the query slice is not here".
    ///
    /// Asserted beside the *creation* calls, which refuse the other way, because
    /// the claim is about the difference: a backend that answered `Unsupported`
    /// everywhere would pass either assertion alone.
    #[test]
    fn a_query_set_and_a_semaphore_handle_refuse_as_unresolvable_not_as_unimplemented() {
        let (_instance, device) = open_device();

        // Poisoned, so "left untouched" is distinguishable from "written with
        // the zeros a timestamp-less device is allowed to report".
        let mut results = [u64::MAX; 2];
        let error = device
            .query_results(unissued(), 0, &mut results)
            .expect_err("no query set was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "query set"),
            "{error:?}"
        );
        assert_eq!(
            results,
            [u64::MAX; 2],
            "a refused query wrote results anyway: {results:?}"
        );

        let error = device
            .semaphore_value(unissued())
            .expect_err("no semaphore was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );

        // The other side of the line, on the calls that create the very objects
        // those handles would have named.
        let error = device
            .create_query_set(&QuerySetDesc {
                label: None,
                kind: QueryKind::Timestamp,
                count: 1,
            })
            .expect_err("no query heaps yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
        let error = device
            .create_semaphore(&SemaphoreDesc {
                label: None,
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect_err("no shared fence yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
    }
}
