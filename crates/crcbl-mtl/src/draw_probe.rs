//! **Diagnostic probes for the Metal draw hang, and nothing else.**
//!
//! `docs/backlog.md`'s "Metal draws hang, and it is our bug, not the runner"
//! records the state of the investigation: on GitHub's `macos-26-arm64` runner
//! (an `Apple Paravirtual device`) a render-pass clear reads back correctly and
//! **every draw hangs** with `kIOGPUCommandBufferCallbackErrorHang`, every
//! encoder reporting `completed` and none faulted. The device itself draws —
//! run 31037470086 drew two triangles there from a standalone Swift script — so
//! the difference is between that command stream and this backend's.
//!
//! Three candidates are already dead, and none of them should be retried:
//! the render-target format (the BGRA twin faulted byte-identically), the
//! error-options command-buffer descriptor (a passing clear goes through the
//! same `crate::fault::command_buffer` path), and the draw argument forms (the
//! short selectors were emitted as an experiment and all four quarantined draws
//! hung anyway; that change is reverted).
//!
//! # What a draw does that a clear does not
//!
//! Nine encoder calls, all between `begin_render_pass` and `end_render_pass`:
//!
//! 1. `setViewport:` — `crate::command`'s `set_viewport`.
//! 2. `setScissorRect:` — `set_scissor`.
//! 3. `setRenderPipelineState:`, and then the five-call rasteriser replay
//!    `bind_graphics_pipeline` makes beside it — `setCullMode:`,
//!    `setFrontFacingWinding:`, `setTriangleFillMode:`, `setDepthClipMode:`,
//!    `setDepthBias:slopeScale:clamp:` and `setDepthStencilState:`.
//! 4. `drawPrimitives:…`.
//!
//! **Bind groups are not on that list, and that matters for the plan.**
//! `crate::device`'s `a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`
//! binds no bind group at all — it uses `ink_msl`, whose vertex stage computes
//! its positions from `[[vertex_id]]` and reads no resource — and it hangs
//! exactly like the one that does. So "the bind groups a draw needs" was
//! eliminated before this module was written, by a test that already exists.
//! The probes below spend their slots on what is left.
//!
//! # The ladder
//!
//! Each probe differs from its neighbour by **one** step, so a single
//! `--no-fail-fast` run reads as a bisect rather than as five separate results.
//! Every one of them draws — or deliberately does not draw — the same triangle
//! into the same canvas with the same clear, so a pass is the *same assertion*
//! the quarantined tests make and cannot be a weaker one.
//!
//! | Probe | Removes | A pass means |
//! |---|---|---|
//! | [`draw_probe_a_bound_pipeline_with_no_draw_call`] | the `drawPrimitives:` call | the setup calls are innocent; the hang needs a draw |
//! | [`draw_probe_a_draw_with_no_scissor_call`] | `setScissorRect:` | the scissor call is implicated |
//! | [`draw_probe_a_draw_with_no_viewport_or_scissor_call`] | `setViewport:` too | the viewport call is implicated |
//! | [`draw_probe_a_hand_encoded_pass_over_this_backends_pipeline`] | the rasteriser replay, and this crate's encoder wrapper | the replay is implicated |
//! | [`draw_probe_a_hand_encoded_pass_over_a_hand_built_pipeline`] | this crate's `MTLRenderPipelineDescriptor` | the pipeline descriptor is implicated |
//!
//! **If every one of them faults**, the answer is not in the render encoder at
//! all: the last probe is the Swift script's command stream reproduced in
//! Rust, over a texture and a queue this crate made, and its failing would put
//! the difference in the objects rather than the calls — the `MTLTexture`
//! descriptor `create_image` builds, or the process itself.
//!
//! # Running them
//!
//! CI runs the whole set in its own step, so the ordinary suite's result stays
//! readable; `.github/workflows/ci.yml` says why that step is expected to be
//! red. On a real Mac:
//!
//! ```text
//! crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(draw_probe_)'
//! crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(draw_probe_a_draw_with_no_scissor_call)'
//! ```
//!
//! # Delete this module when it has answered
//!
//! It is a bisect, not coverage. Nothing above the seam depends on it, no
//! behaviour is guarded by it, and a probe kept after its question is settled
//! is a test nobody can say the purpose of.

use std::time::{Duration, Instant};

use crcbl_hal::{
    Barriers, ColorTargetState, CommandEncoderDesc, Device, Format, GraphicsPipelineDesc,
    GraphicsPipelineHandle, ImageBarrier, ImageSubresourceRange, MultisampleState,
    PipelineLayoutHandle, PrimitiveState, QueueKind, ReadbackDesc, ResourceState, ShaderEntry,
    ShaderModuleHandle, SubmitInfo,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder as _, MTLDevice as _, MTLLibrary,
    MTLLoadAction, MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLStoreAction,
};

use crate::MetalDevice;
use crate::conv;
use crate::device::lookup;
use crate::device::tests::{
    CANVAS, CANVAS_BYTES, CLEAR, CLEAR_TEXEL, Framing, assert_ink_triangle, color_target_of, drain,
    draw_canvas_framed, empty_layout, ink_msl, msl_module, open_device, readback_buffer, texel_in,
    whole_image_copy_of,
};

/// The attachment format every probe uses.
///
/// The same one the quarantined draws use, so a probe's result is comparable
/// with theirs. The BGRA twin already settled that the format is not the
/// variable.
const FORMAT: Format = Format::Rgba8Unorm;

/// How long a probe waits for a hand-committed command buffer before calling it
/// hung.
///
/// The same shape and the same figure as `crate::device`'s `drain`: a deadline
/// rather than a sleep, so a completion point that is never reached fails the
/// test instead of blocking the job until its timeout. Metal's own hang
/// detection fails a command buffer well inside this, which is how the
/// quarantined draws report at all.
const DEADLINE: Duration = Duration::from_secs(10);

/// The ink triangle's pipeline, and the two objects it needs kept alive.
///
/// Every probe below builds the identical one — same MSL from
/// [`ink_msl`], same empty layout, same single colour target, same default
/// [`PrimitiveState`] and [`MultisampleState`] — because a probe that also
/// varied its pipeline would not be varying one thing.
struct InkPipeline {
    module: ShaderModuleHandle,
    layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
}

impl InkPipeline {
    /// Builds it through this crate's own `Device` calls.
    fn new(device: &MetalDevice, label: &str) -> Self {
        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(device);
        let targets = [ColorTargetState::opaque(FORMAT)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some(label),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline over an Rgba8Unorm target");
        Self {
            module,
            layout,
            pipeline,
        }
    }

    /// The `MTLLibrary` behind [`module`](Self::module).
    ///
    /// Read out of the device's own table rather than compiled a second time,
    /// so [`draw_probe_a_hand_encoded_pass_over_a_hand_built_pipeline`] varies
    /// the *pipeline descriptor* and not the library beside it.
    fn library(&self, device: &MetalDevice) -> Retained<ProtocolObject<dyn MTLLibrary>> {
        let state = device.inner.state();
        lookup(
            &state.shader_modules,
            "shader module",
            self.module,
            &*device.inner,
        )
        .expect("the module was created on this device a moment ago")
        .raw
        .clone()
    }

    fn destroy(self, device: &MetalDevice) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.layout);
        device.destroy_shader_module(self.module);
    }
}

/// Asserts a readback is the clear colour and nothing else.
///
/// The counterpart of [`assert_ink_triangle`] for the one probe that records no
/// draw. It is a real assertion rather than "not the ink": the readback buffer
/// starts full of `crate::device`'s poison byte, which is neither colour, so a
/// copy that never ran fails here instead of passing as "nothing was drawn".
fn assert_clear_canvas(bytes: &[u8]) {
    assert_eq!(bytes.len(), CANVAS_BYTES, "the readback is the wrong size");
    let clear = texel_in(FORMAT, CLEAR_TEXEL);
    for (index, texel) in bytes.chunks_exact(4).enumerate() {
        assert_eq!(
            texel, clear,
            "texel {index} is not the clear colour, so this pass painted something"
        );
    }
}

/// **Probe 1 — the setup calls with no draw behind them.**
///
/// Identical to the quarantined
/// `a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` in every
/// respect but one: the vertex range is empty, and `crate::command`'s `draw`
/// returns before emitting anything for an empty range. So the command stream
/// carries `setViewport:`, `setScissorRect:`, `setRenderPipelineState:` and the
/// whole rasteriser replay, and **no `drawPrimitives:` at all**.
///
/// **It isolates** the draw call — and, with it, shader execution — from
/// everything a draw sets up.
///
/// * **Passes** — the setup calls are innocent and the hang needs the draw
///   itself. That kills the viewport/scissor candidate and the
///   `setRenderPipelineState:` half of the pipeline candidate in one result,
///   and points at `drawPrimitives:` or at the shaders the pipeline runs.
/// * **Faults** — the hang does not need a draw at all, which is a much bigger
///   finding than any of the standing candidates: it would mean one of the
///   eight state calls hangs this device, and probes 2 to 5 then say which.
///
/// **What turns it red for ordinary reasons.** A `draw` that stopped
/// short-circuiting an empty range would paint the triangle and trip
/// [`assert_clear_canvas`]; a copy that never ran leaves the poison pattern,
/// which is not the clear colour either.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_bound_pipeline_with_no_draw_call() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: no draw");

    let bytes = draw_canvas_framed(&device, FORMAT, Framing::Both, |encoder| {
        encoder.bind_graphics_pipeline(ink.pipeline);
        // Empty, and `crcbl_mtl::command`'s `draw` documents an empty range as
        // a legitimate "no work this frame" it returns on rather than encodes.
        encoder.draw(0..0, 0..1);
    });
    assert_clear_canvas(&bytes);

    ink.destroy(&device);
}

/// **Probe 2 — the same draw, without `setScissorRect:`.**
///
/// One call fewer than the quarantined triangle. Metal's default scissor is
/// already the whole render target and the canvas pass covers the whole
/// attachment, so the image must be **identical** — which is what lets
/// [`assert_ink_triangle`] stay the assertion and makes this a controlled
/// experiment rather than a different test.
///
/// **It isolates** `crate::command`'s `set_scissor`.
///
/// * **Passes** — `setScissorRect:` is implicated, and the fix is in
///   `set_scissor` (its `Rect2d` arithmetic, or making the whole-target case
///   emit no call the way `begin_render_pass` already does for its render
///   area).
/// * **Faults** — the scissor call is eliminated; read probe 3.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_draw_with_no_scissor_call() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: no scissor");

    let bytes = draw_canvas_framed(&device, FORMAT, Framing::ViewportOnly, |encoder| {
        encoder.bind_graphics_pipeline(ink.pipeline);
        encoder.draw(0..3, 0..1);
    });
    assert_ink_triangle(&bytes, FORMAT);

    ink.destroy(&device);
}

/// **Probe 3 — the same draw again, without `setViewport:` either.**
///
/// One call fewer than probe 2, and for the same reason the image is
/// unchanged: Metal's default viewport is the whole render target at depth
/// `0..1`, which is exactly what `Viewport::from_size` asks for here.
///
/// **It isolates** `crate::command`'s `set_viewport` — *given* probe 2's
/// result. Read on its own it removes two calls, which is why both rungs exist:
/// probe 2 faulting and this one passing names the viewport, and both faulting
/// clears the pair together.
///
/// * **Passes** (with probe 2 faulting) — `setViewport:` is implicated. The
///   `MTLViewport` this backend builds is the first thing to read: it passes
///   the seam's `depth_min`/`depth_max` through unchanged, and a paravirtual
///   device is exactly the kind to object to a degenerate depth range.
/// * **Faults** — the viewport and the scissor are both eliminated, and what is
///   left inside the pass is `setRenderPipelineState:`, the rasteriser replay
///   and the draw. Probes 4 and 5 split those.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_draw_with_no_viewport_or_scissor_call() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: no framing");

    let bytes = draw_canvas_framed(&device, FORMAT, Framing::Neither, |encoder| {
        encoder.bind_graphics_pipeline(ink.pipeline);
        encoder.draw(0..3, 0..1);
    });
    assert_ink_triangle(&bytes, FORMAT);

    ink.destroy(&device);
}

/// **Probe 4 — the Swift script's command stream, over this backend's own
/// objects.**
///
/// The render pass is encoded by hand: an `MTLRenderPassDescriptor` with one
/// colour attachment, `setRenderPipelineState:`, `drawPrimitives:` in its short
/// form, `endEncoding`. **Two calls inside the pass, where the seam's path
/// makes nine.** Everything the pass touches is still this crate's: the
/// `MTLDevice` and `MTLCommandQueue` `MetalDevice::open` made, the `MTLTexture`
/// `create_image` made, the `MTLRenderPipelineState`
/// `create_graphics_pipeline` made, the command-buffer descriptor
/// `crate::fault::command_buffer` sets, and the barrier, copy, submit and
/// readback that follow.
///
/// **It isolates** the rasteriser replay — `setCullMode:`,
/// `setFrontFacingWinding:`, `setTriangleFillMode:`, `setDepthClipMode:`,
/// `setDepthBias:slopeScale:clamp:` and `setDepthStencilState:` — given probe
/// 3. It also drops this crate's encoder wrapper, but that is not a second
/// variable of any weight: the wrapper's pass, blit, submit and readback are
/// the ones `a_render_pass_clear_reads_back_the_exact_texels` exercises and
/// passes with.
///
/// * **Passes** (with probe 3 faulting) — the replay is implicated, and
///   `crate::command`'s `bind_graphics_pipeline` is where to look.
///   `setFrontFacingWinding:` is the call to suspect first: it is the **only**
///   one of the six that asks for something other than Metal's own default
///   here, because `FrontFace`'s default is `Ccw` while the encoder starts at
///   `MTLWinding::Clockwise`. With `CullMode::None` — also the default — it
///   cannot change the image, which makes it exactly the sort of redundant
///   state a virtualised driver could mishandle unnoticed. The rest
///   (`setCullMode:` `None`, `setTriangleFillMode:` `Fill`,
///   `setDepthClipMode:` `Clip`, a zero depth bias and a nil depth/stencil
///   state) each restate a default, so the fix would be to skip them.
/// * **Faults** — no encoder call this backend makes is the cause, because the
///   two that remain are the two the working Swift probe made. What is left is
///   the objects: the pipeline state (probe 5) and the texture.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_over_this_backends_pipeline() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: hand-encoded");
    let bound = device
        .inner
        .graphics_pipeline_raw(ink.pipeline)
        .expect("the pipeline was created on this device a moment ago");

    let bytes = hand_encoded_canvas(&device, &bound.raw);
    assert_ink_triangle(&bytes, FORMAT);

    ink.destroy(&device);
}

/// **Probe 5 — probe 4 with the pipeline descriptor built by hand too.**
///
/// The only difference from probe 4 is which
/// `MTLRenderPipelineDescriptor` reached
/// `newRenderPipelineStateWithDescriptor:error:`. This one sets the three
/// things the Swift script set — vertex function, fragment function, colour
/// attachment 0's pixel format — and nothing else, where
/// `crate::pipeline`'s `create_graphics_pipeline_impl` additionally sets a
/// label, `rasterSampleCount`, `alphaToCoverageEnabled` and the attachment's
/// `writeMask`. The `MTLLibrary` and both `MTLFunction`s are the same objects
/// probe 4 used, taken from the device's own table, so the shader is not a
/// variable here.
///
/// **It isolates** the render pipeline state object — the candidate
/// `docs/backlog.md` names first.
///
/// * **Passes** (with probe 4 faulting) — the pipeline descriptor is
///   implicated, and the four extra setters are a short enough list to bisect
///   by hand on the next run. All four ask for what the descriptor already
///   defaults to — Apple documents `rasterSampleCount` as 1,
///   `alphaToCoverageEnabled` as `NO` and a colour attachment's `writeMask` as
///   all channels — so this outcome would say a *redundant* declaration is
///   enough to hang this device. That is a strange thing to be true, and it is
///   the same shape as the replay probe 4 removes; both are what is left after
///   the ordinary explanations died.
/// * **Faults** — nothing in this crate is implicated at all. This probe is the
///   Swift script, in Rust, on the same device, and its hanging would mean the
///   difference is in the texture `create_image` builds or in the process
///   rather than in any command this backend records. The next experiment would
///   be to build the `MTLTexture` here as well.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_over_a_hand_built_pipeline() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: hand-built pipeline");
    let library = ink.library(&device);

    let vertex = library
        .newFunctionWithName(&NSString::from_str("vertexMain"))
        .expect("ink_msl defines vertexMain");
    let fragment = library
        .newFunctionWithName(&NSString::from_str("fragmentMain"))
        .expect("ink_msl defines fragmentMain");
    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setVertexFunction(Some(&vertex));
    descriptor.setFragmentFunction(Some(&fragment));
    // SAFETY: `objc2` marks the subscript unsafe because Metal does not
    // bounds-check the attachment index, and `crate::pipeline` makes the same
    // call for the same reason. Index 0 is below the array's length on every
    // Metal device — `MTLRenderPipelineDescriptor` always vends at least one
    // colour attachment slot.
    let slot = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    slot.setPixelFormat(conv::pixel_format(FORMAT));
    let state = device
        .inner
        .raw
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .expect("a vertex function, a fragment function and one colour format");

    let bytes = hand_encoded_canvas(&device, &state);
    assert_ink_triangle(&bytes, FORMAT);

    ink.destroy(&device);
}

/// Draws `state`'s triangle into a [`CANVAS`]-sized target with a **hand-built
/// render encoder**, and reads the texels back.
///
/// The render pass is the whole of what is hand-built. The copy that follows it
/// goes through this crate's `CommandEncoder`, `submit` and `poll_readback`,
/// because those are the calls
/// `a_render_pass_clear_reads_back_the_exact_texels` already passes with on the
/// faulting runner — rewriting them here would add a variable to a probe whose
/// point is having only one.
///
/// The two command buffers are ordered by the queue itself: Metal executes them
/// in commit order, and the render one is waited on here anyway so a hang is
/// reported with its per-encoder status rather than as a wrong image.
fn hand_encoded_canvas(
    device: &MetalDevice,
    state: &ProtocolObject<dyn MTLRenderPipelineState>,
) -> Vec<u8> {
    let (image, view) = color_target_of(device, CANVAS, FORMAT);
    let readback = readback_buffer(device, CANVAS_BYTES as u64);
    let (texture, _) = device
        .inner
        .view_raw(view)
        .expect("the view was created on this device a moment ago");

    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: as in `draw_probe_a_hand_encoded_pass_over_a_hand_built_pipeline`
    // and in `crate::command`'s `begin_render_pass` — index 0 is below the
    // fixed length of Metal's colour-attachment array.
    let slot = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    slot.setTexture(Some(&texture));
    slot.setLoadAction(MTLLoadAction::Clear);
    slot.setStoreAction(MTLStoreAction::Store);
    slot.setClearColor(conv::clear_color(CLEAR));

    let command_buffer = crate::fault::command_buffer(&device.inner.queue, "crcbl-mtl probe")
        .expect("the queue vends a command buffer");
    let encoder = command_buffer
        .renderCommandEncoderWithDescriptor(&descriptor)
        .expect("a descriptor with one colour attachment opens a render encoder");
    // Labelled for the same reason every encoder in this crate is: an unlabelled
    // encoder reports as an empty string in a fault report, which turns the
    // diagnostic back into "one of them faulted".
    encoder.setLabel(Some(&NSString::from_str("probe-canvas")));
    encoder.setRenderPipelineState(state);
    // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither the
    // vertex start nor the count. Neither indexes an object — `ink_msl`'s
    // vertex stage reads a compile-time array by `[[vertex_id]]` and no buffer
    // is bound at all — and three is the length of that array. The encoder is
    // kept alive by the `Retained` held across the call.
    unsafe {
        encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
    }
    encoder.endEncoding();
    command_buffer.commit();
    await_completion(&command_buffer);

    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue exists");
    let mut copy = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("crcbl-mtl probe copy"),
        queue,
    });
    copy.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            ImageSubresourceRange::all(FORMAT),
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    copy.copy_image_to_buffer(&whole_image_copy_of(image, readback, CANVAS));
    let commands = copy.finish().expect("the recording is complete");
    device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .expect("the queue accepts it");
    let request = device
        .request_readback(&ReadbackDesc {
            label: Some("the probe canvas"),
            buffer: readback,
            offset: 0,
            size: CANVAS_BYTES as u64,
            after: None,
        })
        .expect("a HostReadback buffer, in range");
    let bytes = drain(device, request, CANVAS_BYTES);

    device.destroy_readback(request);
    device.destroy_command_buffer(commands);
    device.destroy_image_view(view);
    device.destroy_image(image);
    device.destroy_buffer(readback);
    bytes
}

/// Waits for a hand-committed command buffer, and reports a fault the way the
/// rest of this backend does.
///
/// A poll with a deadline rather than `waitUntilCompleted`, for the reason
/// `crate::device`'s `drain` gives: a completion point that is never reached
/// must fail the test rather than block the job. The failure text comes from
/// [`crate::fault::describe`], so a hang here reads identically to the ones in
/// the CI log this module exists to explain — same domain, same code, same
/// per-encoder status list.
fn await_completion(command_buffer: &ProtocolObject<dyn MTLCommandBuffer>) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        match command_buffer.status() {
            MTLCommandBufferStatus::Completed => return,
            MTLCommandBufferStatus::Error => panic!(
                "the hand-encoded render pass failed: {}",
                crate::fault::describe(command_buffer)
            ),
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "the hand-encoded render pass neither completed nor failed within {DEADLINE:?}; \
             Metal's own hang detection did not fire, which is not a failure mode any \
             quarantined draw has shown"
        );
        std::thread::yield_now();
    }
}
