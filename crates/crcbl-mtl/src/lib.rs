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
//! **Push constants arrived with `crcbl_mtl::argument`**, which answers the
//! question the binding slice sharpened rather than closed. Metal has no push
//! constants, so Slang lowers the block to an ordinary buffer argument, and
//! *which* index the committed MSL puts it at is now derived instead of guessed:
//! the one after every binding, because `crcbl-shaders`' declaration-order lint
//! requires a source to declare its push constant last —
//! `msl/push_constant_probe.metal` has the block at `[[buffer(1)]]` behind its
//! one storage buffer at `[[buffer(0)]]`. So `create_pipeline_layout` places it,
//! `push_constants` sends it with `setBytes:length:atIndex:`, and the encoder
//! keeps a shadow of the block because that selector replaces the whole argument
//! rather than a range of it.
//!
//! What is still refused, with `what` naming what is missing: **query sets**, a
//! **word-wide buffer fill**, a **variable-count bind group** and a **wait for
//! a timeline value nothing has signalled**. Indirect-count draws left that
//! list when `crcbl_mtl::icb` landed — a compute kernel fills an
//! `MTLIndirectCommandBuffer` before the render encoder opens, which is what
//! `crcbl_mtl::command`'s deferred pass recording exists to allow.
//!
//! Nothing in this crate is a stub that reports success — a refused command
//! recorded into an encoder *fails the encoder*, so `finish` hands back the
//! refusal rather than a command buffer that submits and does nothing.
//!
//! # Which refusal a caller gets, and why the split is worth having
//!
//! Every one of the refusals above is
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported), and the rule is
//! the one [`Capability`](crcbl_hal::Capability) draws: this variant means **the
//! operation is real and this backend cannot perform it**, whether because Metal
//! has not got it (the byte-wide fill) or because a slice here has not built it
//! (mesh pipelines). Both send a caller to the same place — take the fallback —
//! which is why they share a variant, and
//! `crcbl_mtl::instance::MetalInstance::unsupported` and `…::not_yet` are the
//! two sentences that say which.
//!
//! What is deliberately **not** folded in is anything a caller can correct:
//! a stale handle is [`InvalidHandle`](crcbl_hal::HalError::InvalidHandle), one
//! from another device is [`ForeignObject`](crcbl_hal::HalError::ForeignObject),
//! and a descriptor field that is out of range or self-inconsistent is
//! [`InvalidDescriptor`](crcbl_hal::HalError::InvalidDescriptor) naming the
//! field. A fill whose range runs past the buffer is the caller's arithmetic; a
//! fill whose *value* has no Metal encoding is Metal's limit. Same call, two
//! variants, and that is the distinction rather than an inconsistency.
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
//! **Three modules are the exception, and only in a test build.** The present
//! ledger — the count that answers `Device::wait_until_presented`, since Metal
//! numbers no present — is plain Rust with no Objective-C in it, and it is
//! compiled off macOS under `cfg(test)` so that `cargo test` on any host runs
//! its assertions. `crcbl_mtl::quirk` is the other, for the same reason: what a
//! device does that Metal answers no query for is a decision made from a string,
//! and a machine with no Metal can still check the decision.
//! `crcbl_mtl::argument` is the third: how big Metal's argument tables are, and
//! which entry of the buffer one a push-constant block lands in, are arithmetic
//! over a descriptor — and **nothing in Metal reports a wrong index**, so a
//! machine with no Metal checking it is worth more here than in either of the
//! others. Nothing any of them
//! contains is public or reachable from a non-test build, so the paragraph above
//! still holds for anything a caller can see; the reason for the first exception
//! is that the drawable half of that capability is covered by no automated test
//! anywhere, and this converts the half that can be checked into one that is.
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
//! # Paths: the binding model is earned, the geometry one is not
//!
//! `docs/plan/09-backends-metal-dx12.md` specs this backend as fully
//! GPU-driven, and the hardware supports it.
//! [`GeometryPath`](crcbl_hal::GeometryPath) and its siblings are *derived*
//! from [`Features`](crcbl_hal::Features) precisely so that a backend cannot
//! assert a path it has not earned. Which of the six
//! [`GPU_DRIVEN`](crcbl_hal::Features::GPU_DRIVEN) features are off has moved
//! twice, and the direction of each move is the point:
//!
//! * [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) is now
//!   **on**. `crcbl_mtl::draw` explains why a loop over
//!   `drawPrimitives:indirectBuffer:indirectBufferOffset:` is the feature
//!   rather than an approximation of it: the draw count is a CPU value by
//!   definition, so N calls emit exactly the N draws from N GPU-written
//!   argument structures that the flag means.
//! * [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) is now
//!   **on**, on any device that holds an `MTLIndirectCommandBuffer`, and it was
//!   the last Tier A flag missing — so reporting it moves every such Mac onto
//!   [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount).
//!   The count lives in GPU memory and Metal's only execution that reads one is
//!   `executeCommandsInBuffer:` over an ICB, so `crcbl_mtl::icb` is a compute
//!   kernel that fills that buffer *before* the render encoder the call happens
//!   inside is opened, resetting every command past the count so it draws
//!   nothing. `crcbl_mtl::command`'s deferred pass recording is what makes the
//!   ordering reachable, and it landed before this as a pure refactor.
//! * [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) is
//!   **back on**, and the round trip is the part worth knowing about. MTL1
//!   reported it from `argumentBuffersSupport`, which is a true statement about
//!   the *hardware*; the binding slice made it a false one about this
//!   *backend*, because bind groups were flat argument tables with no
//!   runtime-sized array and `crcbl_hal::pipeline` says a backend that refuses
//!   [`BindingFlags`](crcbl_hal::BindingFlags) must not report the feature. It
//!   came back when `crcbl-shaders` grew MSL declaring an argument buffer and
//!   `crcbl_mtl::binding` grew the path that fills one — a table of
//!   `MTLBuffer::gpuAddress` values, kept resident with `useResource:`. The
//!   flag is now a device query rather than a constant: Tier 2 argument buffers
//!   on a Metal 3 device, which is what those addresses need.
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
//! `DRAW_INDIRECT_COUNT`, which was then unwritten here — the case that
//! argument was made from, and one that no longer arises now that the flag is
//! reported. The `the_default_device_desc_opens_and_the_rest_degrades` test is
//! what keeps that from changing quietly, and it pins each flag to the path
//! derived from it rather than to a fixed answer.
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
//! it — `MULTI_DRAW_INDIRECT` plus
//! [`INDIRECT_FIRST_INSTANCE`](crcbl_hal::Features::INDIRECT_FIRST_INSTANCE)
//! with the binding slice's indirect loop, and
//! [`PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) with the
//! `setBytes:length:atIndex:` behind `push_constants` — see
//! `crcbl_mtl::argument` for the argument-table index that call is given, which
//! is read off the committed MSL rather than chosen here. The full list, with a reason against
//! every flag that is absent, is in this crate's `adapter` module.
//!
//! **`DEPTH_CLAMP` carries one exception, and it is the only flag that does.**
//! A call this crate makes is necessary and it turned out not to be sufficient:
//! GitHub's `Apple Paravirtual device` accepts `setDepthClipMode:` and discards
//! the primitive anyway, measured on that device rather than inferred. So the
//! flag is withheld from a device whose name says it is virtual, and
//! `create_graphics_pipeline` refuses a descriptor that asks for clamping
//! without it. `crcbl_mtl::quirk` holds both halves, the evidence, and the
//! reason a name is what the decision is keyed on.

#[cfg(target_os = "macos")]
mod adapter;
// The third module that is not macOS-only, and for the reason the other two
// give: how big Metal's argument tables are and where a push-constant block
// lands in the buffer one are decided in plain Rust, and nothing in Metal
// reports a wrong index — so `cargo test` on any host runs the arithmetic.
#[cfg(any(target_os = "macos", test))]
mod argument;
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
mod icb;
#[cfg(target_os = "macos")]
mod instance;
// The fifth, and for the reason the other four give: where a render pass's area
// lands as a scissor rectangle and which of Metal's two debug-group stacks a
// label was pushed onto are decided in plain Rust — and neither fails loudly.
// A scissor that leaves the render target raises, which aborts the process
// rather than returning an error, and a group popped on the wrong stack is
// visible only inside a capture tool. So `cargo test` on any host runs both.
#[cfg(any(target_os = "macos", test))]
mod pass;
#[cfg(target_os = "macos")]
mod pipeline;
// The one module that is not macOS-only, and the crate docs say why: it holds
// no Objective-C, and off macOS it exists in the test build alone so that
// `cargo test` on any host runs the present-wait assertions.
#[cfg(any(target_os = "macos", test))]
mod present;
// The fourth, and for the reason the other three give: where a query lands in a
// visibility-result buffer and how far a read of one may reach are `u64`
// arithmetic, and Metal reports neither an out-of-range read nor an overrunning
// resolve — it raises and the process dies — so `cargo test` on any host runs
// the bounds.
#[cfg(any(target_os = "macos", test))]
mod query;
// The second, and for the same reason: what a device does that Metal answers no
// query for is decided in plain Rust, so `cargo test` on any host runs it.
#[cfg(any(target_os = "macos", test))]
mod quirk;
#[cfg(target_os = "macos")]
mod swapchain;

#[cfg(target_os = "macos")]
pub use device::MetalDevice;
#[cfg(target_os = "macos")]
pub use instance::MetalInstance;
