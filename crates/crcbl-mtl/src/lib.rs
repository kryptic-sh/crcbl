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
//! `a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` submits a
//! draw, copies the image into a host-readable buffer and asserts that the
//! centre carries the triangle's colour while every corner still carries the
//! clear's. `crcbl_mtl::pipeline` is where that all lives, and where the split
//! of Metal's pipeline state across the object and the encoder is argued.
//!
//! What is still refused, with `what` naming the slice it will arrive in:
//! surfaces and swapchains, **bind groups and everything that needs one** —
//! bind group layouts, a pipeline layout that names any, push constants — query
//! sets, indexed and indirect draws, and compute dispatches. The bind-group gap
//! is the one with a visible consequence: a pipeline can be built here only
//! from shaders that read no resources, so the engine's own `triangle.slang`,
//! which pulls its vertices from a storage buffer, **compiles and builds a
//! pipeline but cannot yet be drawn with**. Both halves of that are asserted —
//! see `the_engines_own_triangle_artifact_builds_a_real_pipeline`.
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
//! public `MetalInstance` while a device is open releases nothing — which the
//! `a_device_outlives_the_instance_that_made_it` test exercises rather than
//! asserts on paper.
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
//! `MetalInstance` — which holds `MTLDevice` objects and owned `AdapterInfo`
//! and nothing else — needs no `unsafe` of its own.
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
//! # Tier: this slice still reports **Tier B**, and that is not a claim about
//! Metal
//!
//! `docs/plan/09-backends-metal-dx12.md` specs this backend as Tier A, and the
//! hardware supports it. [`DeviceCaps::tier`](crcbl_hal::DeviceCaps::tier) is
//! *derived* from [`Features`](crcbl_hal::Features) precisely so that a backend
//! cannot assert a tier it has not earned, and two of the six Tier A features
//! are still off. [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT)
//! and [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) depend
//! on which indirect path this backend takes — indirect command buffers, per
//! the plan's mapping table — and no slice so far has chosen one, because a
//! plain `drawPrimitives:indirectBuffer:` emits one draw and reads no count.
//! [`COMPUTE`](crcbl_hal::Features::COMPUTE) is the one the pipeline slice
//! turned on, and it is worth being precise about what it now claims: compute
//! *pipelines* build, and `dispatch` still refuses, because that needs an
//! `MTLComputeCommandEncoder` nothing opens yet.
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE) arrived with
//! the command slice, on `MTLSharedEvent`.
//!
//! That has a visible consequence now that devices open:
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) asks for
//! [`Features::TIER_A`](crcbl_hal::Features::TIER_A), so the seam's own
//! convenience constructor is **refused** by this backend with
//! [`UnsupportedFeatures`](crcbl_hal::HalError::UnsupportedFeatures). A caller
//! that wants a device today asks for the features it actually needs. The
//! `the_default_device_desc_is_refused_for_the_tier_a_gap` test is what keeps
//! that from changing quietly.
//!
//! The Tier A features that *are* adapter questions —
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) and
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS) — are
//! reported from real queries. Every other flag this backend reports is
//! reported because a call in *this* crate now makes it true, never because
//! Metal has the capability in the abstract:
//! [`SAMPLER_ANISOTROPY`](crcbl_hal::Features::SAMPLER_ANISOTROPY) arrived with
//! sampler creation, `TIMELINE_SEMAPHORE` and
//! [`DEBUG_MARKERS`](crcbl_hal::Features::DEBUG_MARKERS) with the command
//! slice, and `COMPUTE`,
//! [`DEPTH_CLAMP`](crcbl_hal::Features::DEPTH_CLAMP),
//! [`DEPTH_BIAS_CLAMP`](crcbl_hal::Features::DEPTH_BIAS_CLAMP) and
//! [`POLYGON_MODE_LINE`](crcbl_hal::Features::POLYGON_MODE_LINE) with this one
//! — each because `bind_graphics_pipeline` now makes the call behind it. The
//! full list, with a reason against every flag that is absent, is in this
//! crate's `adapter` module.

#[cfg(target_os = "macos")]
mod adapter;
#[cfg(target_os = "macos")]
mod command;
#[cfg(target_os = "macos")]
mod conv;
#[cfg(target_os = "macos")]
mod device;
#[cfg(target_os = "macos")]
mod fault;
#[cfg(target_os = "macos")]
mod instance;
#[cfg(target_os = "macos")]
mod pipeline;

#[cfg(target_os = "macos")]
pub use device::MetalDevice;
#[cfg(target_os = "macos")]
pub use instance::MetalInstance;
