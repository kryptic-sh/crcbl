//! `crcbl-mtl` — the engine's Metal backend, and macOS's only path to a GPU.
//!
//! `docs/plan/09-backends-metal-dx12.md` puts Metal first of the two P14
//! backends because it is the mandatory one — there is no Vulkan on macOS
//! without MoltenVK — and because the API distance from the Vulkan-flavoured
//! seam is the larger of the two, so it is the backend that finds the seam's
//! leaks.
//!
//! # What this slice is
//!
//! **The first pixel.** `MetalInstance` enumerates adapters and opens devices,
//! `MetalDevice` owns the resource tables, and — as of the command slice — it
//! records, submits and reads back real GPU work: an `MTLCommandBuffer` behind
//! every [`CommandEncoder`](crcbl_hal::CommandEncoder), a real
//! `MTLRenderCommandEncoder` behind every render pass, an
//! `MTLBlitCommandEncoder` behind every copy, `MTLSharedEvent` behind every
//! timeline semaphore, and `MTLCommandBuffer::status` behind
//! `poll_readback`. (The types are named rather than linked, for the reason the
//! next section gives: they do not exist in the build these docs are generated
//! from.)
//!
//! That was the plan's **clear** rung. **The pipeline slice reaches the
//! triangle**: `create_shader_module` compiles the MSL `crcbl-shaders` now
//! commits, `create_pipeline_layout` makes the empty layout, and
//! `create_graphics_pipeline` and `create_compute_pipeline` build real
//! `MTLRenderPipelineState` and `MTLComputePipelineState` objects — so
//! `bind_graphics_pipeline` and `draw` are calls rather than refusals, and
//! `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` submits a
//! draw, copies the image into a host-readable buffer and asserts that the
//! centre carries the triangle's colour while every corner still carries the
//! clear's. `crcbl_mtl::pipeline` is where that all lives, and where the split
//! of Metal's pipeline state across the object and the encoder is argued.
//!
//! **The surface slice is what makes macOS usable at all**, since
//! `docs/backlog.md`'s 2026-08-05 decision leaves Apple platforms with no other
//! GPU path. `Instance::create_surface` takes the shell's `CAMetalLayer`
//! straight from [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit),
//! `surface_caps` answers from the layer, and `create_swapchain` /
//! `acquire_next_frame` / `present` map onto `setDrawableSize:`,
//! `nextDrawable` and `MTLCommandBuffer::presentDrawable:`.
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) is a ring
//! of plain textures through the *same* acquire/present path, which is what
//! `crcbl screenshot` uses and — needing neither a window server nor a shader —
//! is the half CI can execute end to end. `crcbl_mtl::swapchain` is where all of
//! it lives, along with the sRGB argument, the extent rule and the reason a
//! Metal acquire hands back no semaphores.
//!
//! **The binding slice is what lets the engine's own shaders run.** Bind group
//! layouts, bind groups and pipeline layouts naming them are real, indexed
//! draws are real, and the indirect path is real — so `triangle.slang`, which
//! pulls its vertices from a `StructuredBuffer` and could previously be
//! compiled but not drawn with, **now records and executes a complete draw**.
//! `crcbl_mtl::binding` is where the model lives, together with the decision
//! that shapes the whole backend: bind groups map onto Metal's **flat argument
//! tables** rather than argument buffers, because every MSL artifact
//! `crcbl-shaders` commits declares plain `[[buffer(n)]]` arguments that an
//! argument buffer cannot feed. `crcbl_mtl::draw` owns the index-buffer state
//! and the indirect loop.
//!
//! **The dispatch slice makes compute run.** A compute pass opens a real
//! `MTLComputeCommandEncoder` whose lifetime is the pass's, bind groups reach
//! its argument tables, and `dispatch`/`dispatch_indirect` are
//! `dispatchThreadgroups:threadsPerThreadgroup:` and its indirect sibling. The
//! threads-per-threadgroup Metal takes at the *call* comes from
//! [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size),
//! which the seam carries for this backend's sake and `crcbl-vk` checks against
//! the SPIR-V it compiles — so a number disagreeing with `[numthreads(…)]`
//! fails there rather than launching the wrong thread count here.
//!
//! What is still refused, with `what` naming what is missing: **query sets**,
//! **push constants** and **indirect-count draws**. The last is not simply
//! unwritten: an indirect-count draw needs an `MTLIndirectCommandBuffer` filled
//! by a compute pass that would have to run before the render encoder the call
//! happens inside. See `crcbl_mtl::command`'s `indirect_count`.
//!
//! Nothing in this crate is a stub that reports success — a refused command
//! recorded into an encoder *fails the encoder*, so `finish` hands back the
//! refusal rather than a command buffer that submits and does nothing.
//!
//! # Barriers are encoder boundaries, not calls
//!
//! Metal has no `vkCmdPipelineBarrier`, and this backend does not invent one.
//! Every resource it allocates is hazard-tracked by the driver — the default
//! for anything created straight from an `MTLDevice` — and Metal inserts the
//! dependency itself **between encoders**. So
//! [`pipeline_barrier`](crcbl_hal::CommandEncoder::pipeline_barrier) ends the
//! open blit encoder and records nothing else: the encoder split *is* the
//! barrier. The argument in full, including the three changes that would
//! invalidate it, is on that method in this crate's `command` module, and
//! `every_resource_is_hazard_tracked` is what checks the premise on real
//! objects rather than on paper.
//!
//! # `MetalInstance` exists only on macOS, and is unlinked here on purpose
//!
//! The whole backend is behind `#[cfg(target_os = "macos")]`: Metal is an Apple
//! framework and `objc2-metal` is pinned to `cfg(target_os = "macos")` in this
//! crate's manifest, so on Linux, Windows and `wasm32` this crate is these docs
//! and no code at all. That is the same shape `crcbl_jobs::Threads` takes on
//! `wasm32`, and it is written the same way: the type is named in backticks
//! rather than linked, because a link to it is unresolvable in exactly the
//! builds that do not have it, and rustdoc is a CI gate in this workspace.
//!
//! **One module is the exception, and only in a test build.** The present
//! ledger — the count that answers `Device::wait_until_presented`, since Metal
//! numbers no present — is plain Rust with no Objective-C in it, and it is
//! compiled off macOS under `cfg(test)` so that `cargo test` on any host runs
//! its assertions. Nothing it contains is public or reachable from a non-test
//! build, so the paragraph above still holds for anything a caller can see; the
//! reason for the exception is that the drawable half of that capability is
//! covered by no automated test anywhere, and this converts the half that can
//! be checked into one that is.
//!
//! No `#[cfg(target_os = …)]` appears *above* the seam as a result of any of
//! this — `crcbl-hal`'s rule — because the absence is expressed by the crate
//! having no public items, not by a caller testing the platform.
//!
//! # The instance now holds its `MTLDevice` objects, and a device holds the
//! instance
//!
//! MTL1 read every answer the seam wanted off each `MTLDevice` in
//! `MetalInstance::open` and dropped the objects, because nothing needed them.
//! Device creation needs them: on Metal there is no `vkCreateDevice`, so the
//! `MTLDevice` enumeration produced **is** the device a caller opens. They are
//! kept.
//!
//! Keeping them is also how this backend discharges the seam's **obligation 1**
//! (`crcbl_hal::device`): a `Device` may outlive its `Instance`, and the
//! backend must keep the instance's state alive internally. The instance's
//! state is an `Arc` and every device holds a clone of it, so dropping the
//! public `MetalInstance` while a device is open releases nothing — which
//! `crates/crcbl/tests/hal_seam_e2e.rs`'s
//! `a_device_outlives_the_instance_that_made_it` exercises on this backend and
//! every other, rather than asserting on paper.
//!
//! # The `Send + Sync` question — and the marker impl MTL1 said would not be
//! needed
//!
//! [`Instance`](crcbl_hal::Instance) and [`Device`](crcbl_hal::Device) require
//! [`HalThreadSafe`](crcbl_hal::threading::HalThreadSafe), which is
//! `Send + Sync` on native, and MTL1 established that an Objective-C `Retained`
//! pointer can be both: `objc2-metal` declares
//! `pub unsafe trait MTLDevice: NSObjectProtocol + Send + Sync`, so
//! `Retained<ProtocolObject<dyn MTLDevice>>` picks the markers up upstream
//! rather than each user asserting them. That is still true, and it is why
//! `MetalInstance` — which held `MTLDevice` objects and owned `AdapterInfo` and
//! nothing else — needed no `unsafe` of its own until the surface slice put a
//! `CAMetalLayer` in its table. See below for that one, which is deliberately
//! the narrowest of the three.
//!
//! **It does not extend to resources, and MTL1's guess that the device slice
//! "will not need a marker impl" was wrong.** `MTLBuffer` and `MTLTexture` are
//! declared `pub unsafe trait MTLBuffer: MTLResource` — `MTLResource` inherits
//! from `MTLAllocation`, which inherits from `NSObjectProtocol` alone — so
//! neither carries `Send` or `Sync`. The binding is right to withhold them:
//! `MTLBuffer::contents` returns a raw pointer into the allocation, and a
//! binding cannot know what a caller will do with it.
//!
//! So this crate does contain `unsafe` marker assertions, and each SAFETY
//! comment is narrow on purpose. `device.rs` asserts them for the object
//! tables: every buffer, texture and command buffer lives behind the device's
//! `Mutex`, the only `contents` calls are `write_buffer`'s and
//! `poll_readback`'s and both copy under that lock without letting the pointer
//! escape, and Objective-C reference counting is atomic. `MTLCommandQueue`,
//! `MTLSamplerState` and `MTLEvent` are declared `Send + Sync` upstream and are
//! not why that impl exists.
//!
//! `command.rs` asserts them for the encoder, and there the argument is the
//! *trait's own shape* rather than a lock: `CommandEncoder` takes `&mut self`
//! on every recording method, so the borrow checker enforces Metal's "one
//! thread at a time" rule for a command buffer, and the only `&self` method is
//! `Debug::fmt`, which touches no Objective-C object at all.
//!
//! ## The Core Animation one, which is narrower again
//!
//! The surface slice put a `CAMetalLayer` in the instance's surface table and a
//! `CAMetalDrawable` in the device's swapchain table, so `instance.rs` now
//! carries a marker impl too. Exclusion is the same lock argument as above; the
//! part that is *not* about exclusion is **thread affinity**, and it is
//! discharged by what this crate does not do rather than by a claim about Core
//! Animation. The only selectors it ever sends a layer are the Metal-facing ones
//! — `setDevice:`, `setPixelFormat:`, `setDrawableSize:`, `nextDrawable` and
//! their neighbours — and **no `NSView`, `NSWindow` or `NSScreen` is reached
//! from anywhere in this crate**. Those are the genuinely main-thread-only
//! objects that `crcbl_core::surface`'s thread-safety note is about, and nothing
//! here walks a layer's `superlayer` or `delegate` to find one.
//!
//! The claim is not widened past that. `Instance::create_surface` still requires
//! the window's own thread, exactly as the seam says, and this crate does not
//! pretend otherwise.
//!
//! # Handles are generational, and they carry their device
//!
//! Resources are [`Pool`](crcbl_core::Pool) entries and the seam's
//! [`Handle`](crcbl_core::Handle)s name them, so destroying a resource makes
//! every outstanding handle to it fail lookup instead of aliasing whatever took
//! the slot. Obligation 3 — a handle from another device must produce
//! [`ForeignObject`](crcbl_hal::HalError::ForeignObject), never undefined
//! behaviour — needs more than the per-entry owner id it looks like it needs:
//! two devices allocating in step issue bit-identical handles, so device B
//! would resolve device A's handle to *B's own* object and find the owner
//! matching. The issuing device's tag therefore rides in the top byte of the
//! handle's index, exactly as it does in `crcbl-vk`, which arrived at the same
//! scheme from the same bug.
//!
//! The surface slice generalised that rather than copying it: obligation 3
//! checks a surface against the *instance* and everything else against the
//! *device*, so `MetalInstance` now carries an id and a tag of its own and both
//! owners share one lookup path. Two instances issue bit-identical surface
//! handles for the same reason two devices do, and the tag is what separates
//! them.
//!
//! # Paths: this backend selects the **floor** of every axis, and the binding
//! slice moved the reason rather than removing it
//!
//! `docs/plan/09-backends-metal-dx12.md` specs this backend as fully
//! GPU-driven, and the hardware supports it.
//! [`GeometryPath`](crcbl_hal::GeometryPath) and its siblings are *derived*
//! from [`Features`](crcbl_hal::Features) precisely so that a backend cannot
//! assert a path it has not earned. Two of the six
//! [`GPU_DRIVEN`](crcbl_hal::Features::GPU_DRIVEN) features are off, and they
//! are **not the two that were off before**:
//!
//! * [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) is now
//!   **on**. `crcbl_mtl::draw` explains why a loop over
//!   `drawPrimitives:indirectBuffer:indirectBufferOffset:` is the feature
//!   rather than an approximation of it: the draw count is a CPU value by
//!   definition, so N calls emit exactly the N draws from N GPU-written
//!   argument structures that the flag means.
//! * [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) is still
//!   **off**, and it is the one Metal cannot express through this seam's shape
//!   at all. The count lives in GPU memory, Metal's only execution that reads
//!   one is `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over
//!   an `MTLIndirectCommandBuffer`, and filling that buffer from the seam's
//!   argument structures needs a compute kernel running *before* the render
//!   encoder the call happens inside was opened.
//! * [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) has been
//!   **withdrawn**, and that is the reversal worth knowing about. MTL1 reported
//!   it from `argumentBuffersSupport`, which is a true statement about the
//!   *hardware*; the binding slice makes it a false one about this *backend*,
//!   because bind groups here are flat argument tables with no runtime-sized
//!   array, so `create_bind_group_layout` refuses every
//!   [`BindingFlags`](crcbl_hal::BindingFlags) — and `crcbl_hal::pipeline` says
//!   a backend that refuses them must not report the feature. It comes back
//!   with argument buffers, which need `crcbl-shaders` to emit MSL declaring
//!   them.
//!
//! [`COMPUTE`](crcbl_hal::Features::COMPUTE) stays on and now means the whole
//! of it: pipelines build, a pass opens an encoder, and both dispatches are
//! real calls.
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE) arrived with
//! the command slice, on `MTLSharedEvent`.
//!
//! Those two are also exactly what
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) requires, so
//! the seam's own convenience constructor now **opens** here and asks for
//! [`Features::GPU_DRIVEN`](crcbl_hal::Features::GPU_DRIVEN) optionally on top.
//! Until topic 39 it demanded the whole bundle and this backend was refused
//! with [`UnsupportedFeatures`](crcbl_hal::HalError::UnsupportedFeatures) over
//! `DRAW_INDIRECT_COUNT`, a flag absent from Metal's API rather than
//! unimplemented here — the case that argument was made from. The
//! `the_default_device_desc_opens_and_the_rest_degrades` test is what keeps
//! that from changing quietly, and it asserts all three flags above rather than
//! only the absent ones.
//!
//! The one bundled feature that is still purely an adapter question —
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS) — is
//! reported from a real query. Every other flag this backend reports is
//! reported because a call in *this* crate now makes it true, never because
//! Metal has the capability in the abstract:
//! [`SAMPLER_ANISOTROPY`](crcbl_hal::Features::SAMPLER_ANISOTROPY) arrived with
//! sampler creation, `TIMELINE_SEMAPHORE` and
//! [`DEBUG_MARKERS`](crcbl_hal::Features::DEBUG_MARKERS) with the command
//! slice, and `COMPUTE`,
//! [`DEPTH_CLAMP`](crcbl_hal::Features::DEPTH_CLAMP),
//! [`DEPTH_BIAS_CLAMP`](crcbl_hal::Features::DEPTH_BIAS_CLAMP) and
//! [`POLYGON_MODE_LINE`](crcbl_hal::Features::POLYGON_MODE_LINE) with the
//! pipeline slice — each because `bind_graphics_pipeline` makes the call behind
//! it — and `MULTI_DRAW_INDIRECT` plus
//! [`INDIRECT_FIRST_INSTANCE`](crcbl_hal::Features::INDIRECT_FIRST_INSTANCE)
//! with the binding slice's indirect loop. The full list, with a reason against
//! every flag that is absent, is in this crate's `adapter` module.

#[cfg(target_os = "macos")]
mod adapter;
#[cfg(target_os = "macos")]
mod binding;
#[cfg(target_os = "macos")]
mod command;
#[cfg(target_os = "macos")]
mod conv;
#[cfg(target_os = "macos")]
mod device;
#[cfg(target_os = "macos")]
mod draw;
#[cfg(target_os = "macos")]
mod fault;
#[cfg(target_os = "macos")]
mod instance;
#[cfg(target_os = "macos")]
mod pipeline;
// The one module that is not macOS-only, and the crate docs say why: it holds
// no Objective-C, and off macOS it exists in the test build alone so that
// `cargo test` on any host runs the present-wait assertions.
#[cfg(any(target_os = "macos", test))]
mod present;
#[cfg(target_os = "macos")]
mod swapchain;

#[cfg(target_os = "macos")]
pub use device::MetalDevice;
#[cfg(target_os = "macos")]
pub use instance::MetalInstance;
