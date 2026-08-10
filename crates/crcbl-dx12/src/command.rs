//! [`Dx12CommandEncoder`] — an `ID3D12GraphicsCommandList`, the allocator
//! behind it, and the render pass that clears the first pixel.
//!
//! # What records, and what fails the encoder
//!
//! Barriers, buffer↔buffer and buffer↔image copies, render passes with a real
//! `ClearRenderTargetView`/`ClearDepthStencilView`, viewport, scissor and
//! stencil reference — plus pipelines, bind groups, index buffers, every draw
//! the seam has (direct, indexed, indirect and indirect-count), and dispatches
//! both direct and indirect. [`finish`](CommandEncoder::finish) closes the list
//! and hands back a pooled [`CommandBufferHandle`] the device can submit.
//!
//! What still needs a slice — buffer fills, image↔image copies, MSAA resolve
//! attachments, query sets, mesh dispatch, push constants — **fails the encoder** rather than recording
//! nothing, so `finish` returns the refusal instead of a command buffer that
//! submits and draws nothing. That is `crcbl-mtl`'s rule in its command slice,
//! and it is the failure
//! [`Device::take_error`](crcbl_hal::Device::take_error) exists to catch on
//! WebGPU.
//!
//! The indirect path's arithmetic — offsets, strides, spans and the
//! index-buffer view's size — lives in [`crate::draw`], which holds no `windows`
//! type and is therefore the one part of it a non-Windows host can test.
//!
//! # Failures are recorded and reported at `finish`
//!
//! Every recording method returns `()`, so an unresolvable handle or a region
//! D3D12 cannot express has nowhere to go until `finish`. The first failure is
//! kept in [`Dx12CommandEncoder::failed`] and every later command is dropped,
//! exactly as `crcbl-vk` and `crcbl-mtl` do: a command buffer that submits with
//! commands silently missing is far worse than one that refuses to be built.
//!
//! The command list is taken at construction rather than at `finish`, so a
//! queue handle from another device is a failure recorded in
//! [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder)
//! — which returns a bare `Box` and has no way to refuse — and reported at the
//! first call that can carry one.
//!
//! # The encoder holds a reference to everything it records against
//!
//! **A D3D12 command list retains nothing.** So every resource a recorded
//! command names is cloned into [`Dx12CommandEncoder::retained`], moves into the
//! command buffer, and is parked on [`crate::retire`]'s queue for the length of
//! each submission. That is what makes `destroy_buffer` mean "this handle is
//! dead now" without freeing memory the GPU is still reading; see the module
//! docs there for why `crcbl-vk` needs a more elaborate scheme for the same
//! guarantee and Metal needs none.
//!
//! # A render pass, not a blit clear
//!
//! The deliverable of this slice is a *cleared pixel read back*, and the cheap
//! way to fake it would be a copy that fills a texture from a host buffer. A
//! pass opened and immediately closed is the real thing: `OMSetRenderTargets`
//! binds the attachments, `ClearRenderTargetView` clears them, and the output
//! merger is the stage that wrote the bytes.
//!
//! **A clear here honours [`RenderPassDesc::render_area`], which is Vulkan's
//! semantic and not Metal's.** D3D12's clears take a rectangle list, so the
//! area is passed through; `crcbl-mtl` documents the opposite divergence,
//! because a Metal `loadAction` clears the whole attachment.
//!
//! # Store ops are honoured as [`StoreOp::Store`], always
//!
//! `OMSetRenderTargets` has no store op — the render-pass API that does is
//! `ID3D12GraphicsCommandList4::BeginRenderPass`, and reaching for it means
//! requiring that interface and handling its absence. So
//! [`StoreOp::Discard`](crcbl_hal::StoreOp::Discard) writes the attachment out
//! like every other. Storing when the caller said it did not need to is slower
//! and never wrong, which is the direction a slice with no frame loop should
//! err in; `docs/backlog.md` carries it.

use core::mem::ManuallyDrop;
use core::ops::Range;
use std::sync::Arc;

use crcbl_hal::{
    Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePassDesc, ComputePipelineHandle, DrawIndirect,
    DrawIndirectCount, Format, GraphicsPipelineHandle, HalError, ImageAspect, ImageCopy,
    ImageHandle, ImageType, IndexFormat, LoadOp, MemoryLocation, PipelineLayoutHandle,
    QuerySetHandle, QueueTransfer, Rect2d, RenderPassDesc, ShaderStages, Viewport,
};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_BOX, D3D12_CLEAR_FLAG_DEPTH, D3D12_CLEAR_FLAG_STENCIL, D3D12_CLEAR_FLAGS,
    D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_INDEX_BUFFER_VIEW, D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
    D3D12_RESOURCE_BARRIER_FLAG_NONE, D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
    D3D12_RESOURCE_BARRIER_TYPE_UAV, D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER,
    D3D12_RESOURCE_UAV_BARRIER, D3D12_ROOT_PARAMETER_TYPE_CBV, D3D12_ROOT_PARAMETER_TYPE_SRV,
    D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_TEXTURE_DATA_PITCH_ALIGNMENT, D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT, D3D12_VIEWPORT,
    ID3D12CommandAllocator, ID3D12CommandSignature, ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::core::Interface;

use crate::conv;
use crate::device::{
    AttachmentRef, BoundCompute, BoundPipeline, BoundRoot, BufferRef, CommandBufferEntry,
    DeviceInner, ImageRef,
};
use crate::draw::{IndirectKind, check_count, plan_index_binding, plan_indirect};
use crate::instance::not_yet;

/// A batch of resource-state transitions, and the references they borrowed.
///
/// # This type exists because the binding hands over a refcount and never takes
/// it back
///
/// `D3D12_RESOURCE_TRANSITION_BARRIER::pResource` is a
/// `ManuallyDrop<Option<ID3D12Resource>>`, so a barrier built with a cloned
/// interface owns a reference that nothing will release. Making this the *only*
/// way one is built, and releasing in `Drop`, is what turns "remember to
/// release each barrier's resource" from a rule into something the type
/// enforces — including on the path where a later barrier fails to resolve and
/// the batch is abandoned half-built.
///
/// Only `TRANSITION` barriers live here, which is what lets `Drop` name one
/// union member without knowing anything else. The global barrier
/// [`Barriers::global`] asks for is a `UAV` barrier with a null resource, built
/// inline where there is no reference to release.
struct Transitions(Vec<D3D12_RESOURCE_BARRIER>);

impl Transitions {
    const fn new() -> Self {
        Self(Vec::new())
    }

    /// Adds one transition, taking a reference to `resource` for the call.
    fn push(
        &mut self,
        resource: &ID3D12Resource,
        subresource: u32,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) {
        self.0.push(D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            // `BEGIN_ONLY`/`END_ONLY` split a transition across a gap the driver
            // may overlap work in. Nothing here has a gap to split.
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: ManuallyDrop::new(Some(resource.clone())),
                    Subresource: subresource,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        });
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn as_slice(&self) -> &[D3D12_RESOURCE_BARRIER] {
        &self.0
    }
}

impl Drop for Transitions {
    fn drop(&mut self) {
        for barrier in &mut self.0 {
            // SAFETY: `push` above is the only constructor and always writes the
            // `Transition` member, so that is the live one for every entry here.
            // Reading the union field is the whole of what needs the block; the
            // deref through `ManuallyDrop` below is ordinary safe code.
            let transition = unsafe { &mut barrier.Anonymous.Transition };
            // SAFETY: `pResource` is the `ManuallyDrop<Option<ID3D12Resource>>`
            // holding the reference `push` cloned, and this is its matching
            // release. The entry is not read again: the loop visits each once
            // and the `Vec` is dropped immediately afterwards.
            unsafe { ManuallyDrop::drop(&mut transition.pResource) };
        }
    }
}

/// One `CopyTextureRegion` a buffer↔image copy expands into.
///
/// A copy of several array layers is several calls, because a D3D12 texture
/// copy names exactly one subresource — which is also why the buffer offset is
/// per region rather than taken from the descriptor once.
///
/// The box is **not** stored, because the two directions need it in two
/// different coordinate spaces: copying out of an image, it names the region of
/// the image's subresource; copying into one, it names the region of the
/// *footprint*, which is where a `buffer_row_length` wider than the copy puts
/// the padding. Storing one box and using it for both is how a padded upload
/// writes the wrong texels.
#[derive(Clone, Copy, Debug)]
struct CopyRegion {
    /// Subresource index in the image: mip and array layer, plane zero.
    subresource: u32,
    /// Where in the buffer this layer's rows sit, and how they are laid out.
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    /// Where in the image's subresource the region sits, in texels.
    image_offset: (u32, u32, u32),
    /// The region's size, in texels.
    size: (u32, u32, u32),
}

/// The D3D12 command list implementation of [`CommandEncoder`].
pub(crate) struct Dx12CommandEncoder {
    device: Arc<DeviceInner>,
    /// The allocator the list records into. One per encoder: an allocator's
    /// memory cannot be reset while any list recorded from it is in flight, so
    /// sharing one across encoders needs the reset schedule a frame loop has and
    /// this slice does not.
    allocator: Option<ID3D12CommandAllocator>,
    /// `None` only when construction failed, in which case `failed` says why.
    list: Option<ID3D12GraphicsCommandList>,
    /// Every resource a recorded command names. See the module docs.
    retained: Vec<ID3D12Resource>,
    /// The first failure. Every later command is dropped and `finish` returns
    /// this.
    failed: Option<HalError>,
    /// The graphics pipeline currently bound, if any.
    ///
    /// Held so `draw` can refuse when nothing is bound. Not read for its
    /// contents: the pipeline's own state was replayed onto the command list at
    /// bind time, which is where D3D12 keeps it.
    pipeline: Option<BoundPipeline>,
    /// The compute pipeline currently bound, if any. Held for the reason
    /// [`pipeline`](Self::pipeline) is, and separately from it because a
    /// `DIRECT` command list carries both bind points at once.
    compute: Option<BoundCompute>,
    /// Whether an index-buffer view has been set on this list.
    ///
    /// Held so an indexed draw can refuse when none has, for the reason
    /// [`pipeline`](Self::pipeline) is held. Not cleared at a pass boundary:
    /// `IASetIndexBuffer` is command-list state, so the binding genuinely
    /// survives one — which is Vulkan's rule and the seam's.
    index_buffer: bool,
    in_render_pass: bool,
    in_compute_pass: bool,
}

impl core::fmt::Debug for Dx12CommandEncoder {
    /// The interfaces underneath print as raw pointers and say nothing a reader
    /// wants; what a reader wants is whether this encoder can still produce a
    /// command buffer.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dx12CommandEncoder")
            .field("failed", &self.failed.is_some())
            .field("in_render_pass", &self.in_render_pass)
            .field("retained", &self.retained.len())
            .finish_non_exhaustive()
    }
}

impl Dx12CommandEncoder {
    /// Opens an allocator and a command list, or records why it could not.
    ///
    /// `CreateCommandList` hands back a list already in the *recording* state,
    /// which is why nothing here has to open it.
    pub(crate) fn new(device: Arc<DeviceInner>, desc: &CommandEncoderDesc<'_>) -> Self {
        let mut encoder = Self {
            device,
            allocator: None,
            list: None,
            retained: Vec::new(),
            failed: None,
            pipeline: None,
            compute: None,
            index_buffer: false,
            in_render_pass: false,
            in_compute_pass: false,
        };
        // The queue check comes first and is the one failure here that is a
        // caller bug rather than a driver refusal — a queue from another device
        // is `ForeignObject`, and building the list first would hide it behind
        // whatever the driver said.
        if let Err(error) = encoder.device.check_queue(desc.queue) {
            encoder.fail(error);
            return encoder;
        }
        match encoder.device.open_list(desc.label) {
            Ok((allocator, list)) => {
                encoder.allocator = Some(allocator);
                encoder.list = Some(list);
            }
            Err(error) => encoder.fail(error),
        }
        encoder
    }

    /// The list, or `None` once something has failed.
    ///
    /// Every recording method goes through this, so "the encoder has failed"
    /// and "there is nothing to record into" are one check rather than two that
    /// can drift.
    fn list(&self) -> Option<&ID3D12GraphicsCommandList> {
        if self.failed.is_some() {
            return None;
        }
        self.list.as_ref()
    }

    /// Records the first failure and drops every later command.
    fn fail(&mut self, error: HalError) {
        if self.failed.is_none() {
            log::error!("crcbl-dx12: command recording failed: {error}");
            self.failed = Some(error);
        }
    }

    /// Refuses a command that needs a slice which has not arrived.
    fn refuse(&mut self, what: &'static str) {
        self.fail(not_yet(what));
    }

    /// Takes a reference to a resource a recorded command names.
    ///
    /// Deduplicated by interface pointer, so a buffer copied a hundred times
    /// costs one reference rather than a hundred — and so the set parked at
    /// submission is the set of distinct resources.
    fn retain(&mut self, resource: &ID3D12Resource) {
        let raw = resource.as_raw();
        if !self.retained.iter().any(|held| held.as_raw() == raw) {
            self.retained.push(resource.clone());
        }
    }

    /// Resolves a buffer and takes a reference to it, or records the failure.
    fn buffer(&mut self, handle: BufferHandle) -> Option<BufferRef> {
        match self.device.buffer(handle) {
            Ok(buffer) => {
                self.retain(&buffer.raw);
                Some(buffer)
            }
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Resolves an image and takes a reference to it, or records the failure.
    fn image(&mut self, handle: ImageHandle) -> Option<ImageRef> {
        match self.device.image(handle) {
            Ok(image) => {
                self.retain(&image.raw);
                Some(image)
            }
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Whether a copy or a barrier may be recorded here.
    ///
    /// The seam says copies, barriers and query writes are legal only outside a
    /// pass, and this is where that becomes an error rather than an assumption:
    /// `crcbl-hal`'s `null` backend records a validation error for it, so a
    /// graph that mis-nests fails its own unit suite, and a backend that
    /// accepted it silently would make the two disagree.
    fn outside_a_pass(&mut self, what: &str) -> bool {
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} inside a pass; the seam places copies and barriers between passes"
            )));
            return false;
        }
        true
    }

    /// Whether a compute command may be recorded here.
    ///
    /// The mirror of [`outside_a_pass`](Self::outside_a_pass), and the same
    /// argument: `crcbl-hal`'s `null` recorder makes a dispatch outside a
    /// compute pass a validation error, so a graph that mis-nests fails its own
    /// unit suite — and a backend that accepted it would make the two disagree.
    ///
    /// D3D12 itself would take the call: a `DIRECT` list has both bind points
    /// open at all times. That is exactly why this has to be checked here rather
    /// than left to the runtime, which reports nothing.
    fn inside_a_compute_pass(&mut self, what: &str) -> bool {
        if self.in_compute_pass {
            return true;
        }
        self.fail(HalError::InvalidDescriptor(format!(
            "{what} outside a compute pass; the seam records compute work inside one, and the \
             open scope is the only signal a backend gets about which bind point a group is for"
        )));
        false
    }

    /// Whether a draw may be recorded, or the refusal saying it may not.
    ///
    /// **A draw with no pipeline bound fails the encoder rather than being
    /// dropped.** D3D12 would draw nothing with no pipeline state object set,
    /// and nothing is exactly what a caller reading a blank attachment cannot
    /// tell from a shader that wrote nothing. Shared by every draw entry point
    /// so the five spell one rule rather than five.
    fn drawable(&mut self, what: &str) -> bool {
        if self.pipeline.is_some() {
            return true;
        }
        self.fail(HalError::InvalidDescriptor(format!(
            "{what} with no graphics pipeline bound; D3D12 would rasterise nothing and report \
             nothing"
        )));
        false
    }

    /// Records one batch of transitions, if there is anything to record.
    fn barrier(&mut self, transitions: &Transitions) {
        if transitions.is_empty() {
            return;
        }
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is a live command list in the recording state, and the
        // slice is a live, fully initialised barrier array borrowed for the
        // duration of the call. Every entry is a `TRANSITION` naming a resource
        // `Transitions::push` holds a reference to until after this returns.
        unsafe { list.ResourceBarrier(transitions.as_slice()) };
    }

    /// Expands one [`Barriers`] batch into D3D12 transitions.
    ///
    /// Split out so the rules read as one list: what is skipped, what is
    /// refused, and what becomes a transition.
    fn plan_barriers(&mut self, barriers: &Barriers<'_>) -> Option<Transitions> {
        let mut transitions = Transitions::new();
        for barrier in barriers.buffers {
            if let Err(error) = check_queue_transfer(barrier.queue_transfer) {
                self.fail(error);
                return None;
            }
            let buffer = self.buffer(barrier.buffer)?;
            // **A host-visible buffer has no transitions to make.** D3D12 pins a
            // resource on the upload heap to `GENERIC_READ` and one on the
            // readback heap to `COPY_DEST` for its whole lifetime — see
            // `conv::initial_state`, which is where creation gets the same rule
            // — so a barrier on one is not merely redundant, it is illegal. The
            // seam has no vocabulary for that, so the backend absorbs it.
            if !matches!(buffer.location, MemoryLocation::DeviceLocal) {
                continue;
            }
            let before = conv::resource_state(barrier.from);
            let after = conv::resource_state(barrier.to);
            if before == after {
                continue;
            }
            transitions.push(
                &buffer.raw,
                D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                before,
                after,
            );
        }
        for barrier in barriers.images {
            if let Err(error) = check_queue_transfer(barrier.queue_transfer) {
                self.fail(error);
                return None;
            }
            let image = self.image(barrier.image)?;
            let before = conv::resource_state(barrier.from);
            let after = conv::resource_state(barrier.to);
            // A transition whose two states are the same D3D12 state is not a
            // no-op to the debug layer, it is an error — so the seam pairs that
            // collapse (`Undefined` to `Present`, both `COMMON`) are dropped
            // rather than recorded.
            if before == after {
                continue;
            }
            for subresource in image.subresources(barrier.range) {
                transitions.push(&image.raw, subresource, before, after);
            }
        }
        Some(transitions)
    }
}

/// The seam's one-queue answer to an ownership transfer.
///
/// This backend creates a single `DIRECT` queue — see `crate::device`'s `queue`
/// — so a transfer naming two different queues names one that does not exist.
/// Recording nothing would be the silent version: the graph believes it
/// released ownership and the acquire it pairs with never arrives.
fn check_queue_transfer(transfer: Option<QueueTransfer>) -> Result<(), HalError> {
    match transfer {
        None => Ok(()),
        Some(transfer) if transfer.from == transfer.to => Ok(()),
        Some(_) => Err(HalError::InvalidDescriptor(
            "a queue-family ownership transfer needs two queues, and this backend creates one \
             DIRECT queue (Features::ASYNC_COMPUTE_QUEUE and Features::TRANSFER_QUEUE are \
             unreported)"
                .to_string(),
        )),
    }
}

/// A scissor or render area as D3D12 spells it.
///
/// The seam's rectangle carries a signed origin and an unsigned size; D3D12's
/// is four signed edges and rejects a negative one. Both failures are real
/// rather than theoretical — a scissor derived from a window that moved
/// off-screen is negative, and one derived from a swapchain extent can overflow
/// when added to an origin.
fn rect(area: &Rect2d) -> Result<RECT, HalError> {
    let refuse = || {
        HalError::InvalidDescriptor(format!(
            "the rectangle {area:?} is not a D3D12 rectangle: its edges must be non-negative and \
             must fit an i32"
        ))
    };
    if area.x < 0 || area.y < 0 {
        return Err(refuse());
    }
    let width = i32::try_from(area.width).map_err(|_| refuse())?;
    let height = i32::try_from(area.height).map_err(|_| refuse())?;
    Ok(RECT {
        left: area.x,
        top: area.y,
        right: area.x.checked_add(width).ok_or_else(refuse)?,
        bottom: area.y.checked_add(height).ok_or_else(refuse)?,
    })
}

/// The texture side of a copy: one subresource of one image.
fn texture_location(image: &ID3D12Resource, subresource: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(image.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: subresource,
        },
    }
}

/// The buffer side of a copy: rows at a stride, at an offset.
fn buffer_location(
    buffer: &ID3D12Resource,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(buffer.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: footprint,
        },
    }
}

/// Releases the reference a copy location borrowed.
///
/// The two constructors above clone into a `ManuallyDrop`, for the same reason
/// [`Transitions`] does; unlike a barrier batch there are exactly two of these
/// per call and they are released on the next line, so a type is more machinery
/// than the problem needs.
fn release_location(location: &mut D3D12_TEXTURE_COPY_LOCATION) {
    // SAFETY: `pResource` is the `ManuallyDrop<Option<ID3D12Resource>>` the
    // constructor above cloned into, and this is its matching release. The
    // location is not used again by any caller.
    unsafe { ManuallyDrop::drop(&mut location.pResource) };
}

/// Turns a seam buffer↔image copy into the `CopyTextureRegion` calls it is.
///
/// # What D3D12 requires that the seam does not say
///
/// A placed footprint's row pitch must be a multiple of
/// [`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`] and its offset a multiple of
/// [`D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT`]. Neither is expressible in
/// [`BufferImageCopy`], and both are refused by name rather than rounded:
/// rounding the pitch would move every row of the caller's data.
///
/// Pure, and separate from the encoder, so the arithmetic is readable on its
/// own — it is the part of a copy that is wrong *silently*, producing an image
/// that is sheared rather than absent.
fn plan_copy(
    image: &ImageRef,
    buffer: &BufferRef,
    copy: &BufferImageCopy,
) -> Result<Vec<CopyRegion>, HalError> {
    let format = image.format;
    if format.is_depth_stencil() {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of a {format:?} image is not available on this backend: a depth format has \
             two planes and a sampled one is stored typeless, so the copy needs a plane slice and \
             a fully typed footprint the seam has no field for (the DX12 depth slice)"
        )));
    }
    if copy.image_subresource.aspect != ImageAspect::COLOR {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of a {format:?} image names {:?}; a colour image has only ImageAspect::COLOR",
            copy.image_subresource.aspect
        )));
    }
    let mip = copy.image_subresource.mip;
    if mip >= image.mip_levels {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy names mip {mip} of an image with {} mips",
            image.mip_levels
        )));
    }
    let is_3d = matches!(image.image_type, ImageType::D3);
    let (mip_width, mip_height, mip_depth) = image.mip_extent(mip);

    let base_layer = copy.image_subresource.base_layer;
    let layers = copy.image_subresource.layer_count;
    if layers == 0 {
        return Err(HalError::InvalidDescriptor(
            "a copy covering no array layers is not a copy".to_string(),
        ));
    }
    if is_3d && (base_layer != 0 || layers != 1) {
        return Err(HalError::InvalidDescriptor(format!(
            "a volume has one array layer, and this copy names {layers} from {base_layer}: a \
             volume's slices are its depth, which belongs in BufferImageCopy::image_extent"
        )));
    }
    if !is_3d {
        let end = base_layer.checked_add(layers).ok_or_else(|| {
            HalError::InvalidDescriptor("a copy's array range overflows".to_string())
        })?;
        if end > image.slices {
            return Err(HalError::InvalidDescriptor(format!(
                "a copy names layers {base_layer}..{end} of a {}-layer image",
                image.slices
            )));
        }
    }

    let offset = copy.image_offset;
    if offset.x < 0 || offset.y < 0 || offset.z < 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy's image offset {offset:?} is negative; D3D12's copy box is unsigned"
        )));
    }
    #[allow(clippy::cast_sign_loss)]
    let (offset_x, offset_y, offset_z) = (offset.x as u32, offset.y as u32, offset.z as u32);
    let extent = copy.image_extent;
    let depth = if is_3d { extent.depth_or_layers } else { 1 };
    if !is_3d && extent.depth_or_layers != 1 {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of a {:?} image has an extent depth of {}; layers belong in \
             ImageSubresourceLayers::layer_count",
            image.image_type, extent.depth_or_layers
        )));
    }
    if extent.width == 0 || extent.height == 0 || depth == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of {extent:?} texels moves nothing"
        )));
    }
    let fits =
        |start: u32, size: u32, limit: u32| start.checked_add(size).is_some_and(|end| end <= limit);
    if !fits(offset_x, extent.width, mip_width)
        || !fits(offset_y, extent.height, mip_height)
        || !fits(offset_z, depth, mip_depth)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of {extent:?} at {offset:?} runs past mip {mip}, which is \
             {mip_width}x{mip_height}x{mip_depth}"
        )));
    }

    // A block is one texel for an uncompressed format, which is what makes this
    // one calculation rather than two.
    let (block_width, block_height) = format.block_extent();
    if offset_x % block_width != 0 || offset_y % block_height != 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of a {format:?} image starts at {offset:?}, which is not on a \
             {block_width}x{block_height} block boundary"
        )));
    }
    // A compressed region ends on a block boundary too — unless it ends at the
    // edge of the mip, which is the case a mip narrower than one block exists
    // for and the one D3D12 carves out. Nothing here is exercised by an
    // uncompressed format, whose block is one texel.
    let ends_on_a_block = |offset: u32, size: u32, block: u32, mip: u32| {
        (offset + size).is_multiple_of(block) || offset + size == mip
    };
    if !ends_on_a_block(offset_x, extent.width, block_width, mip_width)
        || !ends_on_a_block(offset_y, extent.height, block_height, mip_height)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy of a {format:?} image ends {extent:?} from {offset:?}, which is neither on a \
             {block_width}x{block_height} block boundary nor at the edge of mip {mip}"
        )));
    }
    // Zero means tightly packed, which is the copy's own extent.
    let row_texels = if copy.buffer_row_length == 0 {
        extent.width
    } else {
        copy.buffer_row_length
    };
    let column_texels = if copy.buffer_image_height == 0 {
        extent.height
    } else {
        copy.buffer_image_height
    };
    if row_texels < extent.width || column_texels < extent.height {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy's buffer layout is {row_texels}x{column_texels} texels, smaller than the \
             {}x{} region it must hold",
            extent.width, extent.height
        )));
    }
    let row_pitch = row_texels
        .div_ceil(block_width)
        .checked_mul(format.block_size())
        .ok_or_else(|| HalError::InvalidDescriptor("a copy's row pitch overflows".to_string()))?;
    if row_pitch % D3D12_TEXTURE_DATA_PITCH_ALIGNMENT != 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "a copy's row pitch is {row_pitch} bytes, and D3D12 requires a multiple of \
             {D3D12_TEXTURE_DATA_PITCH_ALIGNMENT}: set BufferImageCopy::buffer_row_length to a \
             width whose rows are that wide"
        )));
    }
    let rows = column_texels.div_ceil(block_height);
    let slice_bytes = u64::from(row_pitch)
        .checked_mul(u64::from(rows))
        .and_then(|bytes| bytes.checked_mul(u64::from(depth)))
        .ok_or_else(|| {
            HalError::InvalidDescriptor("a copy's per-layer size overflows".to_string())
        })?;

    let mut regions = Vec::with_capacity(layers as usize);
    for layer in 0..layers {
        let buffer_offset = slice_bytes
            .checked_mul(u64::from(layer))
            .and_then(|skip| copy.buffer_offset.checked_add(skip))
            .ok_or_else(|| {
                HalError::InvalidDescriptor("a copy's buffer offset overflows".to_string())
            })?;
        if buffer_offset % u64::from(D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT) != 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "a copy places layer {layer} at buffer offset {buffer_offset}, and D3D12 requires \
                 a multiple of {D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT}"
            )));
        }
        let end = buffer_offset.checked_add(slice_bytes).ok_or_else(|| {
            HalError::InvalidDescriptor("a copy's buffer range overflows".to_string())
        })?;
        if end > buffer.size {
            return Err(HalError::InvalidDescriptor(format!(
                "a copy needs bytes {buffer_offset}..{end} of a {}-byte buffer",
                buffer.size
            )));
        }
        regions.push(CopyRegion {
            subresource: image.subresource(mip, base_layer + layer),
            footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: buffer_offset,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    // The image's own format, never a view's: a copy moves bytes
                    // and a reinterpretation would move them wrongly.
                    Format: conv::dxgi_format(format),
                    Width: row_texels,
                    Height: column_texels,
                    Depth: depth,
                    RowPitch: row_pitch,
                },
            },
            image_offset: (offset_x, offset_y, offset_z),
            size: (extent.width, extent.height, depth),
        });
    }
    Ok(regions)
}

/// Which planes a depth-stencil clear touches, or `None` if it touches neither.
///
/// Keyed on the format as well as the load ops: asking D3D12 to clear the
/// stencil plane of a format that has none is an error, and a caller that
/// spelled `LoadOp::Clear` for both planes of a `D32Float` attachment has done
/// nothing wrong — the seam's `DepthStencilAttachment` carries both pairs of ops
/// whatever the format is.
fn clear_flags(format: Format, depth: LoadOp, stencil: LoadOp) -> Option<D3D12_CLEAR_FLAGS> {
    let mut flags = D3D12_CLEAR_FLAGS(0);
    if format.has_depth() && matches!(depth, LoadOp::Clear) {
        flags |= D3D12_CLEAR_FLAG_DEPTH;
    }
    if format.has_stencil() && matches!(stencil, LoadOp::Clear) {
        flags |= D3D12_CLEAR_FLAG_STENCIL;
    }
    if flags.0 == 0 { None } else { Some(flags) }
}

/// Sets one root descriptor's address, through the call its parameter type
/// names.
///
/// Six calls rather than one because D3D12 has six: a root CBV, SRV and UAV are
/// three different root parameter types, and each has a graphics and a compute
/// form because a `DIRECT` command list carries a root signature of each. All
/// six take a bare `u64` GPU virtual address, which is the property that makes a
/// dynamic offset free here — see `crcbl_dx12::root`.
fn set_root_descriptor(list: &ID3D12GraphicsCommandList, compute: bool, root: BoundRoot) {
    let (parameter, address) = (root.parameter, root.address);
    // SAFETY: `list` is live and recording. `address` is inside a buffer
    // resource the encoder holds a reference to, and `parameter` is a root
    // parameter index the bound root signature declares with exactly this
    // parameter type — both come from `crcbl_dx12::root`, which laid the
    // signature out and resolved the address against it.
    unsafe {
        match (compute, root.parameter_type) {
            (true, D3D12_ROOT_PARAMETER_TYPE_CBV) => {
                list.SetComputeRootConstantBufferView(parameter, address);
            }
            (false, D3D12_ROOT_PARAMETER_TYPE_CBV) => {
                list.SetGraphicsRootConstantBufferView(parameter, address);
            }
            (true, D3D12_ROOT_PARAMETER_TYPE_SRV) => {
                list.SetComputeRootShaderResourceView(parameter, address);
            }
            (false, D3D12_ROOT_PARAMETER_TYPE_SRV) => {
                list.SetGraphicsRootShaderResourceView(parameter, address);
            }
            // `RootPlan::parameter_type` produces exactly those two and the UAV,
            // so this is the writable storage buffer and cannot be anything
            // else.
            (true, _) => list.SetComputeRootUnorderedAccessView(parameter, address),
            (false, _) => list.SetGraphicsRootUnorderedAccessView(parameter, address),
        }
    }
}

impl CommandEncoder for Dx12CommandEncoder {
    // --- debug ---

    /// Accepted and dropped.
    ///
    /// A D3D12 debug region is a PIX event, whose payload format belongs to
    /// `WinPixEventRuntime` rather than to D3D12 — which is exactly why
    /// `crate::adapter` does not report
    /// [`Features::DEBUG_MARKERS`](crcbl_hal::Features::DEBUG_MARKERS), and why
    /// the seam documents a marker as degrading rather than failing when it is
    /// absent.
    fn begin_debug_label(&mut self, _label: &str) {}

    fn end_debug_label(&mut self) {}

    fn insert_debug_marker(&mut self, _label: &str) {}

    // --- sync ---

    fn pipeline_barrier(&mut self, barriers: &Barriers<'_>) {
        if self.list().is_none() || !self.outside_a_pass("a barrier") {
            return;
        }
        let Some(transitions) = self.plan_barriers(barriers) else {
            return;
        };
        self.barrier(&transitions);
        if barriers.global {
            let Some(list) = self.list() else { return };
            // A UAV barrier with a null resource is D3D12's "every unordered
            // access finishes before any starts", which is the whole of what a
            // global barrier can mean on an API with no execution barrier of its
            // own. No reference is taken, so there is none to release.
            let global = [D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                        pResource: ManuallyDrop::new(None),
                    }),
                },
            }];
            // SAFETY: `list` is a live command list in the recording state and
            // `global` is a live, fully initialised barrier array borrowed for
            // the call. Its one entry is a `UAV` barrier whose resource is
            // `None`, which is the legal spelling of "all resources".
            unsafe { list.ResourceBarrier(&global) };
        }
    }

    // --- copies ---

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        if self.list().is_none() || !self.outside_a_pass("a buffer copy") {
            return;
        }
        let Some(src) = self.buffer(copy.src) else {
            return;
        };
        let Some(dst) = self.buffer(copy.dst) else {
            return;
        };
        if copy.size == 0 {
            return;
        }
        if src.raw.as_raw() == dst.raw.as_raw() {
            self.fail(HalError::InvalidDescriptor(
                "a buffer copy whose source and destination are the same resource is not a D3D12 \
                 copy; CopyBufferRegion requires two resources"
                    .to_string(),
            ));
            return;
        }
        let ends = |offset: u64, size: u64| offset.checked_add(size);
        let (Some(src_end), Some(dst_end)) = (
            ends(copy.src_offset, copy.size),
            ends(copy.dst_offset, copy.size),
        ) else {
            self.fail(HalError::InvalidDescriptor(
                "a buffer copy's range overflows".to_string(),
            ));
            return;
        };
        if src_end > src.size || dst_end > dst.size {
            self.fail(HalError::InvalidDescriptor(format!(
                "a copy of {} bytes reads {}..{src_end} of a {}-byte buffer and writes \
                 {}..{dst_end} of a {}-byte one",
                copy.size, copy.src_offset, src.size, copy.dst_offset, dst.size
            )));
            return;
        }
        let Some(list) = self.list() else { return };
        // SAFETY: both resources are live buffers this device created, held by
        // `self.retained` for the encoder's lifetime and by the retire queue for
        // the submission's. Every byte of both ranges was bounds-checked above.
        unsafe {
            list.CopyBufferRegion(
                &dst.raw,
                copy.dst_offset,
                &src.raw,
                copy.src_offset,
                copy.size,
            );
        }
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        self.record_texture_copy(copy, true);
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        self.record_texture_copy(copy, false);
    }

    fn copy_image_to_image(&mut self, _copy: &ImageCopy) {
        // Not the same call with two arguments swapped: both sides are texture
        // locations with their own subresource and box, and neither is the
        // placed footprint `plan_copy` exists to build. Nothing in this slice
        // needs one.
        self.refuse("image-to-image copies (the DX12 pipeline slice)");
    }

    fn fill_buffer(&mut self, _buffer: BufferHandle, _offset: u64, _size: u64, _value: u32) {
        // `ClearUnorderedAccessViewUint` is D3D12's fill, and it takes a
        // descriptor from a **shader-visible** heap — which is the one
        // `crate::descriptor` deliberately never creates, because a
        // shader-visible heap is a frame resource belonging to the slice that
        // has a root signature to bind it against.
        // **Deliberately not implemented, and nothing needs it.** D3D12's fill
        // is `ClearUnorderedAccessViewUint`, which needs a descriptor from a
        // shader-visible heap that `crcbl_dx12::descriptor` does not create.
        // `crcbl-render` zeroes its draw-generation counters with a clear
        // dispatch instead — chosen over a graph-level fill precisely because
        // `fill_buffer` is four separate backend promises (Metal repeats a
        // byte, wgpu clears only to zero, this refuses) where a dispatch runs
        // anywhere that can dispatch. So this is a non-fix on purpose rather
        // than an oversight, and a caller that wants it should say why a
        // dispatch will not do.
        self.refuse("buffer fills (the DX12 binding slice)");
    }

    // --- render scope ---

    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) {
        if self.list().is_none() {
            return;
        }
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_render_pass inside a pass; passes do not nest".to_string(),
            ));
            return;
        }
        if desc.color_attachments.is_empty() && desc.depth_stencil_attachment.is_none() {
            self.fail(HalError::InvalidDescriptor(
                "a render pass with no colour and no depth/stencil attachment renders nowhere"
                    .to_string(),
            ));
            return;
        }
        let ceiling = self.device.caps.limits.max_color_attachments as usize;
        if desc.color_attachments.len() > ceiling {
            self.fail(HalError::InvalidDescriptor(format!(
                "{} colour attachments exceed this device's limit of {ceiling}",
                desc.color_attachments.len()
            )));
            return;
        }
        let area = match rect(&desc.render_area) {
            Ok(area) => area,
            Err(error) => {
                self.fail(error);
                return;
            }
        };

        let mut colors: Vec<AttachmentRef> = Vec::with_capacity(desc.color_attachments.len());
        for attachment in desc.color_attachments {
            if attachment.resolve.is_some() {
                // An MSAA resolve is `ResolveSubresource` after the pass, not a
                // field of the attachment — and running it needs the resolve
                // target's state transitioned, which the graph did not ask for.
                self.refuse("MSAA resolve attachments (the DX12 pipeline slice)");
                return;
            }
            match self.device.color_attachment(attachment.view) {
                Ok(reference) => {
                    self.retain(&reference.image);
                    colors.push(reference);
                }
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        let depth_stencil = match desc.depth_stencil_attachment {
            None => None,
            Some(attachment) => {
                if attachment.read_only {
                    // A read-only depth attachment is a DSV created with
                    // `D3D12_DSV_FLAG_READ_ONLY_DEPTH`/`_STENCIL`, and
                    // `crate::device`'s `create_image_view` creates neither —
                    // the seam has no field to ask for one, so a view cannot
                    // know which pass will read it. Binding the writable
                    // descriptor instead would be the silent version: the pass
                    // reads correctly and the image is in the wrong state.
                    self.fail(HalError::InvalidDescriptor(
                        "a read-only depth/stencil attachment needs a DSV created with \
                         D3D12_DSV_FLAG_READ_ONLY_DEPTH, and this backend creates one writable \
                         descriptor per view (the DX12 pipeline slice)"
                            .to_string(),
                    ));
                    return;
                }
                match self.device.depth_attachment(attachment.view) {
                    Ok(reference) => {
                        self.retain(&reference.image);
                        Some(reference)
                    }
                    Err(error) => {
                        self.fail(error);
                        return;
                    }
                }
            }
        };

        let descriptors: Vec<D3D12_CPU_DESCRIPTOR_HANDLE> =
            colors.iter().map(|color| color.descriptor).collect();
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording. `descriptors` is a live array of
        // exactly `len` render target descriptors, each allocated out of this
        // device's own RTV heap and still live because the view that owns it is;
        // the pointer is borrowed for the duration of the call only. `false`
        // says the handles are separate rather than a contiguous range, which is
        // what a `Vec` of independently allocated slots is. The depth pointer is
        // `None` when there is no depth attachment.
        unsafe {
            list.OMSetRenderTargets(
                descriptors.len() as u32,
                if descriptors.is_empty() {
                    None
                } else {
                    Some(descriptors.as_ptr())
                },
                false,
                depth_stencil
                    .as_ref()
                    .map(|attachment| core::ptr::from_ref(&attachment.descriptor)),
            );
        }

        for (attachment, reference) in desc.color_attachments.iter().zip(&colors) {
            if !matches!(attachment.load, LoadOp::Clear) {
                continue;
            }
            let color = attachment.clear.color;
            // SAFETY: `reference.descriptor` is a render target view this device
            // wrote into its own non-shader-visible RTV heap, which is the heap
            // this call requires; the resource behind it is retained by this
            // encoder. `color` and the rectangle are live locals borrowed for the
            // call.
            unsafe { list.ClearRenderTargetView(reference.descriptor, &color, Some(&[area])) };
        }
        if let (Some(attachment), Some(reference)) = (desc.depth_stencil_attachment, &depth_stencil)
            && let Some(flags) = clear_flags(
                reference.format,
                attachment.depth_load,
                attachment.stencil_load,
            )
        {
            let Ok(stencil) = u8::try_from(attachment.clear.stencil) else {
                self.fail(HalError::InvalidDescriptor(format!(
                    "a stencil clear value of {} does not fit D3D12's 8-bit stencil plane",
                    attachment.clear.stencil
                )));
                return;
            };
            // SAFETY: as the colour clear above, into this device's own DSV
            // heap. `flags` names only planes `reference.format` has.
            unsafe {
                list.ClearDepthStencilView(
                    reference.descriptor,
                    flags,
                    attachment.clear.depth,
                    stencil,
                    Some(&[area]),
                );
            }
        }

        // A D3D12 command list starts with no viewport and no scissor at all, so
        // a pass that never calls `set_viewport` would rasterise nothing. The
        // render area is the only defensible default and is what the caller
        // already said the pass covers.
        #[allow(clippy::cast_precision_loss)]
        let viewport = D3D12_VIEWPORT {
            TopLeftX: desc.render_area.x as f32,
            TopLeftY: desc.render_area.y as f32,
            Width: desc.render_area.width as f32,
            Height: desc.render_area.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        // SAFETY: `list` is live and recording; both arrays are live locals
        // borrowed for the duration of their calls.
        unsafe {
            list.RSSetViewports(&[viewport]);
            list.RSSetScissorRects(&[area]);
        }
        self.in_render_pass = true;
    }

    fn end_render_pass(&mut self) {
        if !self.in_render_pass {
            return;
        }
        self.in_render_pass = false;
        // Deliberately not gated on the failure flag: the attachments stay bound
        // otherwise, and a copy recorded after a failed pass would then be a
        // copy of a resource D3D12 still believes is a render target.
        let Some(list) = self.list.as_ref() else {
            return;
        };
        // SAFETY: `list` is live and recording. Unbinding takes a count of zero
        // and three null pointers, which is what `None` compiles to here.
        unsafe { list.OMSetRenderTargets(0, None, false, None) };
    }

    /// Sets the viewport, one to one.
    ///
    /// **No Y flip and no depth inversion.** D3D12's clip space is the seam's,
    /// and the engine's reversed-Z lives in the projection matrix rather than in
    /// the viewport's depth range — see [`Viewport`]'s own docs, and
    /// `crcbl-mtl`, which says the same about Metal for the same reason.
    fn set_viewport(&mut self, viewport: &Viewport) {
        let Some(list) = self.list() else { return };
        let raw = D3D12_VIEWPORT {
            TopLeftX: viewport.x,
            TopLeftY: viewport.y,
            Width: viewport.width,
            Height: viewport.height,
            MinDepth: viewport.depth_min,
            MaxDepth: viewport.depth_max,
        };
        // SAFETY: `list` is live and recording, and `raw` is a live local
        // borrowed for the duration of the call.
        unsafe { list.RSSetViewports(&[raw]) };
    }

    fn set_scissor(&mut self, area: &Rect2d) {
        if self.list().is_none() {
            return;
        }
        let area = match rect(area) {
            Ok(area) => area,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(list) = self.list() else { return };
        // SAFETY: as `set_viewport`.
        unsafe { list.RSSetScissorRects(&[area]) };
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording; the call takes one scalar.
        unsafe { list.OMSetStencilRef(reference) };
    }

    /// Binds a pipeline state object, its root signature, and the two pieces of
    /// state D3D12 keeps on the command list.
    ///
    /// **`SetPipelineState` does not set a root signature.** A command list
    /// carries one of its own, and a draw whose PSO and root signature disagree
    /// is undefined behaviour the debug layer reports and a release runtime does
    /// not — so the pipeline entry carries the signature it was built against
    /// and both are set here.
    ///
    /// Setting the root signature also **resets every root argument**, so a
    /// caller must bind its groups after its pipeline. That is the order
    /// `crcbl_hal::CommandEncoder` already documents, and it is Vulkan's rule
    /// too.
    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        if self.list().is_none() {
            return;
        }
        let bound = match self.device.graphics_pipeline(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is a live command list in the recording state, and each
        // interface is one this encoder holds a reference to for the duration of
        // the call. `IASetPrimitiveTopology` and `OMSetStencilRef` take scalars.
        unsafe {
            list.SetGraphicsRootSignature(&bound.root_signature);
            list.SetPipelineState(&bound.raw);
            list.IASetPrimitiveTopology(bound.topology);
            if let Some(reference) = bound.stencil_reference {
                list.OMSetStencilRef(reference);
            }
        }
        self.pipeline = Some(bound);
        // **A command list has one pipeline state object, not one per bind
        // point.** Setting this one replaced whatever a compute pass left, so a
        // later dispatch that did not rebind would run *this* object as a
        // compute shader. Forgetting here is what turns that into the refusal
        // `dispatch` already writes.
        self.compute = None;
    }

    /// Sets the `D3D12_INDEX_BUFFER_VIEW` a later indexed draw reads.
    ///
    /// D3D12's view is an address, a byte count and a format rather than a
    /// buffer and an offset, so the offset is folded into
    /// `GetGPUVirtualAddress` and the size is what is left of the allocation —
    /// see [`crate::draw::plan_index_binding`] for the two rules that makes
    /// checkable, and for why `SizeInBytes` is the only bound an indexed draw
    /// needs.
    ///
    /// **The binding is command-list state and survives a pass boundary**, which
    /// is Vulkan's rule and not Metal's: `IASetIndexBuffer` is recorded here
    /// rather than replayed at the draw, so this is legal outside a render pass
    /// too. `crcbl-mtl` carries the binding across to the draw instead, because
    /// `drawIndexedPrimitives:` takes the buffer as an argument.
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        if self.list().is_none() {
            return;
        }
        let Some(resolved) = self.buffer(buffer) else {
            return;
        };
        let size = match plan_index_binding(offset, format, resolved.size) {
            Ok(size) => size,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        // SAFETY: `resolved.raw` is a live buffer resource this encoder just
        // retained, and `GetGPUVirtualAddress` takes nothing and reads no state.
        let base = unsafe { resolved.raw.GetGPUVirtualAddress() };
        let view = D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: base + offset,
            SizeInBytes: size,
            Format: conv::index_format(format),
        };
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, and `view` is a live, fully
        // initialised local borrowed for the duration of the call. Its address
        // is inside an allocation this encoder holds a reference to, aligned to
        // the index width, and its size was bounded by the allocation's own.
        unsafe { list.IASetIndexBuffer(Some(&raw const view)) };
        self.index_buffer = true;
    }

    /// Binds a bind group's descriptor tables and root descriptors at the root
    /// parameters the pipeline layout put them at.
    ///
    /// **A dynamic offset reaches a root descriptor, not a table.** A descriptor
    /// table has no offset to apply, so `crcbl_dx12::binding` plans a `dynamic`
    /// binding as a root CBV/SRV/UAV — which takes a GPU virtual address rather
    /// than a handle — and the offset is added on the way to
    /// `SetGraphicsRootConstantBufferView` here. `crcbl_dx12::device::bind_group`
    /// has already checked the count, the alignment and the bounds.
    ///
    /// **Which bind point anything lands on is decided by the open scope**, and
    /// nothing else: the seam gives a backend no other signal, which is why it
    /// asks a caller to bind its groups inside the pass they are for. That is
    /// `crcbl-vk`'s rule verbatim, and here it is the difference between
    /// `SetComputeRootDescriptorTable` and its graphics twin — a `DIRECT`
    /// command list has both, and the wrong one leaves the dispatch reading the
    /// last draw's arguments.
    fn bind_group(
        &mut self,
        index: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) {
        if self.list().is_none() {
            return;
        }
        let bound = match self
            .device
            .bind_group(index, group, dynamic_offsets, layout)
        {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        for resource in &bound.retained {
            self.retain(resource);
        }
        let compute = self.in_compute_pass;
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording. `bound.heaps` is a live slice of
        // shader-visible heaps this device owns, borrowed for the call and
        // containing no null entry — `VisibleHeaps::bound` drops a heap that was
        // never created rather than passing one. Each table base is an address
        // inside one of exactly those heaps, and each root descriptor's address
        // is inside a buffer this encoder now holds a reference to; both are at
        // a root parameter index the pipeline layout's own root signature
        // declares, of the type declared there.
        unsafe {
            if !bound.heaps.is_empty() {
                list.SetDescriptorHeaps(&bound.heaps);
            }
            for (root, base) in bound.views.into_iter().chain(bound.samplers) {
                if compute {
                    list.SetComputeRootDescriptorTable(root, base);
                } else {
                    list.SetGraphicsRootDescriptorTable(root, base);
                }
            }
            for root in bound.roots {
                set_root_descriptor(list, compute, root);
            }
        }
    }

    /// Fails the encoder, because no layout on this device can declare a range.
    ///
    /// Not [`not_yet`]: `create_pipeline_layout` refuses a
    /// [`PushConstantRange`](crcbl_hal::PushConstantRange) outright — this
    /// device does not report
    /// [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) — so a
    /// caller reaching here is holding a layout that does not exist, and "the
    /// push-constant slice has not landed" would send it looking for a feature
    /// rather than at the layout it built.
    fn push_constants(
        &mut self,
        _stages: ShaderStages,
        offset: u32,
        data: &[u8],
        _layout: PipelineLayoutHandle,
    ) {
        self.fail(HalError::InvalidDescriptor(format!(
            "{} byte(s) of push constants at offset {offset}, on a device that does not report \
             Features::PUSH_CONSTANTS: create_pipeline_layout refuses a push-constant range, so \
             no layout here declares one",
            data.len()
        )));
    }

    // --- draws ---

    /// `DrawInstanced`, with the seam's two ranges as D3D12's four scalars.
    ///
    /// **A draw with no pipeline bound fails the encoder rather than being
    /// dropped.** D3D12 would draw nothing with no pipeline state object set,
    /// and nothing is exactly what a caller reading a blank attachment cannot
    /// tell from a shader that wrote nothing.
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        if self.list().is_none() || !self.drawable("a draw") {
            return;
        }
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, and the call takes four scalars.
        // `saturating_sub` is what makes an empty or inverted range a zero
        // count rather than a wrap to four billion vertices.
        unsafe {
            list.DrawInstanced(
                vertices.end.saturating_sub(vertices.start),
                instances.end.saturating_sub(instances.start),
                vertices.start,
                instances.start,
            );
        }
    }

    /// `DrawIndexedInstanced`, with the seam's two ranges and its base vertex as
    /// D3D12's five scalars.
    ///
    /// **Both bases are passed through exactly as they arrive**, and D3D12
    /// excludes both from the ids the shader sees: `SV_VertexID` is the value
    /// read out of the index buffer and `SV_InstanceID` counts from zero,
    /// neither picking up `BaseVertexLocation` or `StartInstanceLocation`. That
    /// is the lowering `crates/crcbl-shaders/shaders/mesh.slang`'s header
    /// measured, and it is why the engine's own draws pass zero for both: WGSL
    /// and MSL *include* the bases, so zero is the only value all four targets
    /// agree on. Nothing here enforces that — it is the caller's rule, and a
    /// backend that quietly zeroed a base it was handed would be lying to the
    /// one target that reads it correctly.
    ///
    /// A draw with no pipeline or no index buffer bound fails the encoder, for
    /// the reason [`draw`](Self::draw) gives: D3D12 rasterises nothing and
    /// reports nothing, and nothing is what a caller reading a blank attachment
    /// cannot tell from a shader that wrote nothing.
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        if self.list().is_none() || !self.drawable("an indexed draw") {
            return;
        }
        if !self.index_buffer {
            self.fail(HalError::InvalidDescriptor(
                "an indexed draw with no index buffer bound; D3D12 would read indices through no \
                 view and rasterise nothing"
                    .to_string(),
            ));
            return;
        }
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, and the call takes five scalars.
        // `saturating_sub` is what makes an empty or inverted range a zero count
        // rather than a wrap to four billion indices.
        unsafe {
            list.DrawIndexedInstanced(
                indices.end.saturating_sub(indices.start),
                instances.end.saturating_sub(instances.start),
                indices.start,
                base_vertex,
                instances.start,
            );
        }
    }

    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, IndirectKind::Draw);
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, IndirectKind::DrawIndexed);
    }

    fn draw_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, IndirectKind::Draw);
    }

    fn draw_indexed_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, IndirectKind::DrawIndexed);
    }

    /// Refused for the same reason `create_mesh_pipeline` is: there is no mesh
    /// pipeline for this to have been recorded against.
    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        self.refuse("DispatchMesh (the DX12 mesh slice)");
    }

    // --- compute scope ---

    /// Opens a compute *scope*, and records nothing.
    ///
    /// D3D12 has no compute encoder to create — a `DIRECT` list takes both — so
    /// the only thing a compute pass is here is the bookkeeping that makes
    /// nesting an error. Every command that would go inside one refuses.
    fn begin_compute_pass(&mut self, _desc: &ComputePassDesc<'_>) {
        if self.list().is_none() {
            return;
        }
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_compute_pass inside a pass; passes do not nest".to_string(),
            ));
            return;
        }
        self.in_compute_pass = true;
    }

    /// Closes the scope, and forgets the pipeline it was bound in.
    ///
    /// Forgetting matters: the state object stays set on the command list, so a
    /// dispatch in a *later* pass with nothing bound would run the previous
    /// pass's shader against the previous pass's root arguments. Clearing here
    /// makes that the refusal [`dispatch`](Self::dispatch) already writes.
    fn end_compute_pass(&mut self) {
        self.in_compute_pass = false;
        self.compute = None;
    }

    /// Binds a compute pipeline state object and its root signature.
    ///
    /// The compute twin of
    /// [`bind_graphics_pipeline`](Self::bind_graphics_pipeline), and everything
    /// that method's docs say about `SetPipelineState` not setting a root
    /// signature holds here — with the extra trap that a `DIRECT` command list
    /// carries a **graphics and a compute** root signature at once, so binding
    /// the graphics one would leave the dispatch reading whatever the last draw
    /// bound.
    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        if self.list().is_none() || !self.inside_a_compute_pass("bind_compute_pipeline") {
            return;
        }
        let bound = match self.device.compute_pipeline(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is a live command list in the recording state, and each
        // interface is one this encoder holds a reference to for the duration of
        // the call.
        unsafe {
            list.SetComputeRootSignature(&bound.root_signature);
            list.SetPipelineState(&bound.raw);
        }
        self.compute = Some(bound);
        // The mirror of `bind_graphics_pipeline`'s last line, and the same
        // reason: one command list, one pipeline state object.
        self.pipeline = None;
    }

    /// `Dispatch`, in workgroups.
    ///
    /// **A dispatch with no compute pipeline bound fails the encoder**, for the
    /// reason [`draw`](Self::draw) gives: D3D12 runs nothing and reports
    /// nothing, and nothing is what a caller reading an unwritten buffer cannot
    /// tell from a shader that wrote nothing.
    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        if self.list().is_none() || !self.inside_a_compute_pass("dispatch") {
            return;
        }
        if self.compute.is_none() {
            self.fail(HalError::InvalidDescriptor(
                "a dispatch with no compute pipeline bound; D3D12 would run nothing and report \
                 nothing"
                    .to_string(),
            ));
            return;
        }
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, and the call takes three
        // scalars.
        unsafe { list.Dispatch(x, y, z) };
    }

    /// `ExecuteIndirect` with a `DISPATCH` command signature and one command.
    ///
    /// D3D12 has no `vkCmdDispatchIndirect`. What it has is `ExecuteIndirect`,
    /// which reads *any* argument layout a command signature describes — so the
    /// seam's call is the degenerate case: one command, whose arguments are a
    /// `D3D12_DISPATCH_ARGUMENTS`, and no count buffer. See
    /// [`DeviceInner::dispatch_signature`](crate::device::DeviceInner::dispatch_signature)
    /// for the object, which is created once per device.
    ///
    /// The span is checked against the buffer's own size before the call.
    /// `ExecuteIndirect` reads twelve bytes at the offset and bounds-checks
    /// neither, so a short buffer is a GPU fault where this is an error a caller
    /// can catch — the same trade `crcbl-mtl` makes for the same reason.
    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        if self.list().is_none() || !self.inside_a_compute_pass("dispatch_indirect") {
            return;
        }
        if self.compute.is_none() {
            self.fail(HalError::InvalidDescriptor(
                "an indirect dispatch with no compute pipeline bound; D3D12 would run nothing and \
                 report nothing"
                    .to_string(),
            ));
            return;
        }
        let Some(buffer) = self.buffer(args) else {
            return;
        };
        let plan = match plan_indirect(IndirectKind::Dispatch, offset, 1, 0, buffer.size) {
            Ok(Some(plan)) => plan,
            // Unreachable through a count of one, and dropped rather than
            // refused if it ever becomes reachable: a call of no commands is not
            // an error anywhere else in this module either.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let signature = match self
            .device
            .indirect_signature(IndirectKind::Dispatch, plan.stride)
        {
            Ok(signature) => signature,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, `signature` is a live command
        // signature this device owns for exactly this layout and stride, and
        // `buffer.raw` is a live resource this encoder retained above. The
        // argument span was checked against the buffer's own size, and the count
        // buffer is `None` — legal for a signature whose command count is a
        // constant.
        unsafe {
            list.ExecuteIndirect(&signature, plan.count, &buffer.raw, offset, None, 0);
        }
    }

    // --- queries ---

    /// Accepted and dropped.
    ///
    /// The seam asks every caller to reset unconditionally so the Vulkan path is
    /// never the special case; D3D12 query heaps need no reset, and failing here
    /// would fail a call a correct caller always makes.
    fn reset_query_set(&mut self, _set: QuerySetHandle, _range: Range<u32>) {}

    /// Accepted and dropped, which is what the seam documents for a device
    /// without [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY)
    /// — and `crate::adapter` reports none.
    fn write_timestamp(&mut self, _set: QuerySetHandle, _index: u32) {}

    /// Fails the encoder, because this one has a destination.
    ///
    /// Unlike the two above, dropping a resolve leaves `dst` holding whatever
    /// was there and a caller reading it as timings. That is not degradation, it
    /// is wrong data — so this refuses, and as a stale handle rather than a
    /// missing slice: no query set exists to have issued one.
    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        _range: Range<u32>,
        _dst: BufferHandle,
        _dst_offset: u64,
    ) {
        self.fail(HalError::invalid_handle("query set", set));
    }

    // --- finish ---

    fn finish(mut self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a render pass still open".to_string(),
            ));
        }
        if self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a compute pass still open".to_string(),
            ));
        }
        if let Some(error) = self.failed.take() {
            return Err(error);
        }
        let (Some(list), Some(allocator)) = (self.list.take(), self.allocator.take()) else {
            return Err(HalError::DeviceLost(
                "this encoder never had a command list".to_string(),
            ));
        };
        // SAFETY: `list` is a live command list this encoder opened and has been
        // recording into, and it is closed exactly once — `self.list` is now
        // `None`, so no later call can reach it.
        unsafe { list.Close() }.map_err(|error| {
            HalError::Backend(format!("ID3D12GraphicsCommandList::Close failed: {error}"))
        })?;

        Ok(self.device.register_command_buffer(CommandBufferEntry {
            owner: self.device.owner.id,
            allocator,
            list,
            retained: core::mem::take(&mut self.retained),
        }))
    }
}

impl Dx12CommandEncoder {
    /// `ExecuteIndirect` with a `DRAW` or `DRAW_INDEXED` command signature and a
    /// count the CPU knows.
    ///
    /// The body of [`draw_indirect`](CommandEncoder::draw_indirect) and
    /// [`draw_indexed_indirect`](CommandEncoder::draw_indexed_indirect), which
    /// differ only in which argument structure the signature describes.
    ///
    /// **`MaxCommandCount` is the whole of `MULTI_DRAW_INDIRECT` here.** One
    /// call emits `draw_count` draws from `draw_count` argument structures, so
    /// this is D3D12's native multi-draw rather than a loop standing in for one
    /// — which is what `crcbl-mtl` has to do, and why its module docs argue that
    /// the loop *is* the feature there.
    fn indirect(&mut self, draw: &DrawIndirect, kind: IndirectKind) {
        if self.list().is_none() || !self.drawable(kind.what()) || !self.indexed_for(kind) {
            return;
        }
        let Some(args) = self.buffer(draw.args) else {
            return;
        };
        let plan = match plan_indirect(kind, draw.offset, draw.draw_count, draw.stride, args.size) {
            Ok(Some(plan)) => plan,
            // A draw of nothing, which the seam allows and D3D12 has no call
            // for: `MaxCommandCount` of zero is not a legal `ExecuteIndirect`.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(signature) = self.signature(kind, plan.stride) else {
            return;
        };
        let Some(list) = self.list() else { return };
        // SAFETY: `list` is live and recording, `signature` is a live command
        // signature this device owns for exactly this layout and stride, and
        // `args.raw` is a live resource this encoder retained above. The span
        // every structure lies in was checked against the buffer's own size, and
        // the count buffer is `None` — legal for a signature whose command count
        // is a constant.
        unsafe {
            list.ExecuteIndirect(&signature, plan.count, &args.raw, draw.offset, None, 0);
        }
    }

    /// `ExecuteIndirect` with a count buffer, which is the whole of
    /// [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT).
    ///
    /// `pCountBuffer` is a parameter of the same call the CPU-count path makes,
    /// so the signature is shared and nothing about this is emulated: D3D12
    /// reads the `u32`, clamps it to `MaxCommandCount`, and executes that many.
    /// `crcbl-mtl` refuses the same two entry points because Metal has no such
    /// parameter — see `crcbl_mtl::draw` — which is the difference that puts
    /// D3D12 on [`IndirectCount`](crcbl_hal::GeometryPath::IndirectCount) and
    /// Metal on the per-batch arm.
    fn indirect_count(&mut self, draw: &DrawIndirectCount, kind: IndirectKind) {
        if self.list().is_none() || !self.drawable(kind.what()) || !self.indexed_for(kind) {
            return;
        }
        let Some(args) = self.buffer(draw.args) else {
            return;
        };
        let Some(count) = self.buffer(draw.count_buffer) else {
            return;
        };
        let plan = match plan_indirect(
            kind,
            draw.args_offset,
            draw.max_draw_count,
            draw.stride,
            args.size,
        ) {
            Ok(Some(plan)) => plan,
            // A ceiling of zero reads neither buffer, so neither is checked and
            // there is nothing to record. See `crate::draw::plan_indirect`.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        if let Err(error) = check_count(kind, draw.count_offset, count.size) {
            self.fail(error);
            return;
        }
        let Some(signature) = self.signature(kind, plan.stride) else {
            return;
        };
        let Some(list) = self.list() else { return };
        // SAFETY: as `indirect`, plus `count.raw`, a live resource this encoder
        // also retained, whose `u32` at `count_offset` was just checked to be
        // inside it.
        unsafe {
            list.ExecuteIndirect(
                &signature,
                plan.count,
                &args.raw,
                draw.args_offset,
                &count.raw,
                draw.count_offset,
            );
        }
    }

    /// Refuses an *indexed* indirect draw with no index buffer bound.
    ///
    /// The indirect twin of the check [`draw_indexed`](CommandEncoder::draw_indexed)
    /// makes, and it has to be a separate one because the argument structure is
    /// what says how many indices to read: with no view bound D3D12 reads them
    /// through nothing and rasterises nothing, which is indistinguishable from a
    /// cull pass that emitted no work.
    fn indexed_for(&mut self, kind: IndirectKind) -> bool {
        if kind != IndirectKind::DrawIndexed || self.index_buffer {
            return true;
        }
        self.fail(HalError::InvalidDescriptor(
            "an indexed indirect draw with no index buffer bound; D3D12 would read indices \
             through no view and rasterise nothing"
                .to_string(),
        ));
        false
    }

    /// The command signature for one layout and stride, or the failure recorded.
    fn signature(&mut self, kind: IndirectKind, stride: u32) -> Option<ID3D12CommandSignature> {
        match self.device.indirect_signature(kind, stride) {
            Ok(signature) => Some(signature),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// The body of both buffer↔image copies, which differ only in which side is
    /// the placed footprint.
    fn record_texture_copy(&mut self, copy: &BufferImageCopy, buffer_is_source: bool) {
        let what = if buffer_is_source {
            "a buffer-to-image copy"
        } else {
            "an image-to-buffer copy"
        };
        if self.list().is_none() || !self.outside_a_pass(what) {
            return;
        }
        let Some(buffer) = self.buffer(copy.buffer) else {
            return;
        };
        let Some(image) = self.image(copy.image) else {
            return;
        };
        let regions = match plan_copy(&image, &buffer, copy) {
            Ok(regions) => regions,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(list) = self.list() else { return };
        for region in regions {
            let (offset_x, offset_y, offset_z) = region.image_offset;
            let (width, height, depth) = region.size;
            // The box is always the *source's* region, in the source's own
            // coordinates: texels of the image's subresource when the image is
            // the source, and texels of the footprint — which starts at its own
            // origin, because the buffer offset is inside the footprint — when
            // the buffer is. The destination origin is the other side's.
            let (source_box, (x, y, z)) = if buffer_is_source {
                (
                    D3D12_BOX {
                        left: 0,
                        top: 0,
                        front: 0,
                        right: width,
                        bottom: height,
                        back: depth,
                    },
                    (offset_x, offset_y, offset_z),
                )
            } else {
                (
                    D3D12_BOX {
                        left: offset_x,
                        top: offset_y,
                        front: offset_z,
                        right: offset_x + width,
                        bottom: offset_y + height,
                        back: offset_z + depth,
                    },
                    (0, 0, 0),
                )
            };
            let mut texture = texture_location(&image.raw, region.subresource);
            let mut placed = buffer_location(&buffer.raw, region.footprint);
            let (dst, src) = if buffer_is_source {
                (core::ptr::from_ref(&texture), core::ptr::from_ref(&placed))
            } else {
                (core::ptr::from_ref(&placed), core::ptr::from_ref(&texture))
            };
            // SAFETY: both locations are live, fully initialised structs holding
            // references to resources this encoder retains, borrowed for the
            // duration of the call, and `source_box` is a live local. `plan_copy`
            // bounds-checked the region against the image's mip extent and the
            // footprint against the buffer's size, and checked D3D12's pitch and
            // placement alignments; the footprint-space box is inside the
            // footprint because the layout was checked to be at least the
            // region's size.
            unsafe {
                list.CopyTextureRegion(dst, x, y, z, src, Some(core::ptr::from_ref(&source_box)));
            }
            release_location(&mut texture);
            release_location(&mut placed);
        }
    }
}
