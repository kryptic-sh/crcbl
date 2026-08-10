//! **Diagnostic probes for the Metal draw hang, and nothing else.**
//!
//! `docs/backlog.md`'s "The Metal hang is in the encoder's rasteriser replay,
//! and needs no draw" records where the bisect stands: on GitHub's
//! `macos-26-arm64` runner (an `Apple Paravirtual device`) a render-pass clear
//! reads back correctly and this backend's render pass hangs with
//! `kIOGPUCommandBufferCallbackErrorHang`. The previous round of probes bought
//! three results and they are not re-run here:
//!
//! * **The hang needs no draw at all.** Binding a pipeline in our pass is
//!   enough, so the draw call and its argument forms are out.
//! * **The objects are out.** A hand-encoded pass over this crate's own
//!   `MTLDevice`, queue, `MTLTexture`, `MTLRenderPipelineState` and
//!   [`crate::fault`] command-buffer descriptor passes.
//! * **`setViewport:` and `setScissorRect:` are out**, by the two probes that
//!   dropped them.
//!
//! # What is left, and what this round asks
//!
//! The difference between the passing hand-encoded pass and the failing one is
//! `crate::command`'s `bind_graphics_pipeline`: `setRenderPipelineState:` — which
//! the passing pass also makes — plus a **six-call rasteriser replay** that it
//! does not.
//!
//! So every probe below is that known-good hand-encoded pass **plus exactly one
//! of the six**, which makes a hang name a selector rather than a class. One
//! `--no-fail-fast` run reads straight off the list of failures.
//!
//! | Probe | Emits inside the pass | Reading |
//! |---|---|---|
//! | [`draw_probe_a_bound_pipeline_with_no_draw_call`] | our whole path, no `drawPrimitives:` | **positive control**: expected to hang. Passing means the runner no longer reproduces the bug and nothing below can be read |
//! | [`draw_probe_a_hand_encoded_pass_and_none_of_the_replay`] | `setRenderPipelineState:` and the draw | **negative control**: expected to pass, as it did last round |
//! | [`draw_probe_a_hand_encoded_pass_plus_set_cull_mode`] | …plus `setCullMode:` | a hang names `setCullMode:` |
//! | [`draw_probe_a_hand_encoded_pass_plus_set_front_facing_winding`] | …plus `setFrontFacingWinding:` | a hang names `setFrontFacingWinding:` |
//! | [`draw_probe_a_hand_encoded_pass_plus_set_triangle_fill_mode`] | …plus `setTriangleFillMode:` | a hang names `setTriangleFillMode:` |
//! | [`draw_probe_a_hand_encoded_pass_plus_set_depth_clip_mode`] | …plus `setDepthClipMode:` | a hang names `setDepthClipMode:` |
//! | [`draw_probe_a_hand_encoded_pass_plus_set_depth_bias`] | …plus `setDepthBias:slopeScale:clamp:` | a hang names that call |
//! | [`draw_probe_a_hand_encoded_pass_plus_a_nil_depth_stencil_state`] | …plus `setDepthStencilState:nil` | a hang names *nil* specifically, and the pair below says so |
//! | [`draw_probe_a_hand_encoded_pass_plus_a_default_depth_stencil_state`] | …plus `setDepthStencilState:` with a real object | with the probe above, splits "the call" from "the nil argument" |
//! | [`draw_probe_a_hand_encoded_pass_plus_the_whole_replay`] | …plus all six, in `bind_graphics_pipeline`'s order | the run's backstop; see below |
//!
//! **Every one of the six is image-neutral here**, which is why
//! [`assert_ink_triangle`] stays the assertion for all of them and each probe
//! varies one thing: culling is off, the fill mode is `Fill`, the clip mode is
//! `Clip`, the bias is zero, there is no depth attachment for a depth/stencil
//! state to affect, and the winding cannot matter while nothing is culled.
//!
//! # The run cannot come back with no information
//!
//! [`draw_probe_a_hand_encoded_pass_plus_the_whole_replay`] is what makes that
//! true, because it closes the gap between the two controls:
//!
//! * **It hangs and a single-call probe hangs** — that call is the answer.
//! * **It hangs and every single-call probe passes** — no one call is enough,
//!   so the fault is in a *combination* of the six, and the next run bisects by
//!   halves rather than singles.
//! * **It passes** — then the replay is exonerated entirely, because our pass
//!   hangs with the same seven calls the hand-encoded one now makes. What is
//!   left is the part the hand-encoded probes replace wholesale: the
//!   `MTLRenderPassDescriptor` and `MTLRenderCommandEncoder` that
//!   `crate::command`'s `begin_render_pass` builds, and
//!   `setRenderPipelineState:` in *that* encoder rather than in this one. That
//!   is a real finding and it has nowhere else to hide.
//!
//! **The named suspect is `setFrontFacingWinding:`** — it is the only one of
//! the six asking for anything other than a Metal default, `FrontFace::Ccw`
//! against the encoder's `MTLWinding::Clockwise`, and with `CullMode::None` it
//! cannot change the image, so no picture-based test could ever have caught it.
//! It is a suspect and not a conclusion: the previous round's leading candidate
//! was wrong, which is why all six get a slot.
//!
//! # Running them
//!
//! CI runs the whole set in its own step, so the ordinary suite's result stays
//! readable; `.github/workflows/ci.yml` says why that step is expected to be
//! red. On a real Mac:
//!
//! ```text
//! crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(draw_probe_)'
//! crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(draw_probe_a_hand_encoded_pass_plus_set_front_facing_winding)'
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
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder as _, MTLDepthStencilDescriptor,
    MTLDevice as _, MTLLoadAction, MTLPrimitiveType, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineState, MTLStoreAction,
};

use crate::MetalDevice;
use crate::conv;
use crate::device::tests::{
    CANVAS, CANVAS_BYTES, CLEAR, CLEAR_TEXEL, assert_ink_triangle, color_target_of, drain,
    draw_canvas, empty_layout, ink_msl, msl_module, open_device, readback_buffer, texel_in,
    whole_image_copy_of,
};
use crate::pipeline::BoundPipeline;

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

/// **The positive control — our own pass, and it is the one that hangs.**
///
/// Kept from the previous round unchanged, and the only probe of that round
/// kept. Everything else there has answered, and a probe that keeps running
/// after its question is settled is noise in the next run's signal; this one is
/// still doing work, because it is what says the failure is still there. Every
/// other probe below is a *pass* when the fault is elsewhere, so a run where
/// they all pass is unreadable — the runner image being fixed and the replay
/// being innocent look identical — unless something known-red is red beside
/// them.
///
/// It records the whole of this backend's path with an empty vertex range, so
/// the command stream carries `setViewport:`, `setScissorRect:`,
/// `setRenderPipelineState:` and the entire rasteriser replay, and **no
/// `drawPrimitives:` at all**.
///
/// * **Hangs** — as it did last round. The bisect below is readable.
/// * **Passes** — stop reading the rest of the run. Something outside this
///   crate changed, and the next step is to find out what before spending
///   another probe slot.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_bound_pipeline_with_no_draw_call() {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, "crcbl-mtl probe: no draw");

    let bytes = draw_canvas(&device, FORMAT, |encoder| {
        encoder.bind_graphics_pipeline(ink.pipeline);
        // Empty, and `crcbl_mtl::command`'s `draw` documents an empty range as
        // a legitimate "no work this frame" it returns on rather than encodes.
        encoder.draw(0..0, 0..1);
    });
    assert_clear_canvas(&bytes);

    ink.destroy(&device);
}

/// **The negative control — the hand-encoded pass, with none of the six.**
///
/// `setRenderPipelineState:`, `drawPrimitives:`, `endEncoding`: two calls
/// inside the pass, where this backend's path makes eight. It passed last round
/// and it is the baseline every probe below adds exactly one call to, so it
/// runs again to establish that the baseline is still a baseline in *this* run
/// rather than in the previous one's log.
///
/// * **Passes** — the additions below are single-variable experiments.
/// * **Hangs** — the objects are back in play after all, and no result below
///   means anything. That would contradict the previous round on the same
///   runner image, so the first thing to check is whether the image moved.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_and_none_of_the_replay() {
    hand_encoded_probe("crcbl-mtl probe: no replay", |_, _, _| {});
}

/// **`setCullMode:`**, and nothing else added to the baseline.
///
/// The value is the one `bind_graphics_pipeline` would pass for this pipeline,
/// read out of its own `RasterState` rather than written here — `CullMode`'s
/// default is `None`, which is also Metal's, so the image is unchanged.
///
/// * **Hangs** — `setCullMode:` is the call, and the fix is to skip it when it
///   restates the encoder's default.
/// * **Passes** — it is not this one.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_set_cull_mode() {
    hand_encoded_probe("crcbl-mtl probe: setCullMode:", |_, bound, encoder| {
        encoder.setCullMode(bound.raster.cull);
    });
}

/// **`setFrontFacingWinding:`**, and nothing else added to the baseline.
///
/// The named suspect, for the reason the module docs give: `FrontFace::Ccw`
/// against an encoder that starts at `MTLWinding::Clockwise` makes this the one
/// call of the six that asks the device to *change* something, and
/// `CullMode::None` makes that change invisible in the image.
///
/// * **Hangs** — the suspicion was right, and the fix is in
///   `bind_graphics_pipeline`.
/// * **Passes** — the suspicion was wrong, which is the outcome the previous
///   round's leading candidate had.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_set_front_facing_winding() {
    hand_encoded_probe(
        "crcbl-mtl probe: setFrontFacingWinding:",
        |_, bound, encoder| {
            encoder.setFrontFacingWinding(bound.raster.winding);
        },
    );
}

/// **`setTriangleFillMode:`**, and nothing else added to the baseline.
///
/// `PolygonMode`'s default maps to `MTLTriangleFillMode::Fill`, which is the
/// encoder's own default, so this restates it.
///
/// * **Hangs** — `setTriangleFillMode:` is the call.
/// * **Passes** — it is not this one.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_set_triangle_fill_mode() {
    hand_encoded_probe(
        "crcbl-mtl probe: setTriangleFillMode:",
        |_, bound, encoder| {
            encoder.setTriangleFillMode(bound.raster.fill);
        },
    );
}

/// **`setDepthClipMode:`**, and nothing else added to the baseline.
///
/// `MTLDepthClipMode::Clip` is the encoder's default, and this pass has no
/// depth attachment for it to act on either way.
///
/// * **Hangs** — `setDepthClipMode:` is the call, and a depth mode set on a
///   pass with no depth attachment would be a plausible thing for a
///   paravirtual device to mishandle.
/// * **Passes** — it is not this one.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_set_depth_clip_mode() {
    hand_encoded_probe("crcbl-mtl probe: setDepthClipMode:", |_, bound, encoder| {
        encoder.setDepthClipMode(bound.raster.clip);
    });
}

/// **`setDepthBias:slopeScale:clamp:`**, and nothing else added to the
/// baseline.
///
/// All three are zero for a pipeline with no depth state, which is Metal's
/// default and this pipeline's value.
///
/// * **Hangs** — that call is it.
/// * **Passes** — it is not this one.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_set_depth_bias() {
    hand_encoded_probe(
        "crcbl-mtl probe: setDepthBias:slopeScale:clamp:",
        |_, bound, encoder| {
            let [constant, slope_scale, clamp] = bound.raster.bias;
            encoder.setDepthBias_slopeScale_clamp(constant, slope_scale, clamp);
        },
    );
}

/// **`setDepthStencilState:` with nil**, and nothing else added to the
/// baseline.
///
/// Exactly what `bind_graphics_pipeline` passes here: this pipeline declares
/// `depth_stencil: None`, so `bound.depth_stencil` is `None` and the selector
/// gets a nil argument. The assertion below states that rather than assuming
/// it, because the whole point of this probe is *which argument* was passed.
///
/// * **Hangs** — with the probe below passing, the nil argument is the fault
///   rather than the selector, and the fix is to skip the call when there is no
///   state to bind.
/// * **Hangs, and the probe below hangs too** — the selector itself is the
///   fault, whatever it is handed.
/// * **Passes** — it is not this one.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_a_nil_depth_stencil_state() {
    hand_encoded_probe(
        "crcbl-mtl probe: setDepthStencilState: nil",
        |_, bound, encoder| {
            assert!(
                bound.depth_stencil.is_none(),
                "this probe exists to pass nil, and the pipeline has a depth/stencil state"
            );
            encoder.setDepthStencilState(bound.depth_stencil.as_deref());
        },
    );
}

/// **`setDepthStencilState:` with a real object**, and nothing else added to
/// the baseline.
///
/// The twin of the probe above, and the only probe here that passes a value
/// this backend would not: a freshly built `MTLDepthStencilState` left at
/// `MTLDepthStencilDescriptor`'s defaults, which Apple documents as compare
/// `Always` and depth writes disabled — *behaviourally* what nil restores. So
/// the image is unchanged and the pair differs only in nil versus an object.
///
/// * **Hangs while the nil probe passes** — the fault is a depth/stencil state
///   object on a pass with no depth attachment, and this backend is at fault
///   only for pipelines that declare one.
/// * **Passes while the nil probe hangs** — nil is the fault; see there.
/// * **Both pass** — `setDepthStencilState:` is out entirely.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_a_default_depth_stencil_state() {
    hand_encoded_probe(
        "crcbl-mtl probe: setDepthStencilState: default",
        |device, _, encoder| {
            let descriptor = MTLDepthStencilDescriptor::new();
            let state = device
                .inner
                .raw
                .newDepthStencilStateWithDescriptor(&descriptor)
                .expect("a default depth/stencil descriptor makes a state on any Metal device");
            encoder.setDepthStencilState(Some(&state));
        },
    );
}

/// **All six, in the order `bind_graphics_pipeline` emits them.**
///
/// The backstop that keeps the run from returning nothing, and the module docs
/// give the three readings in full. In one line each: hanging beside a
/// single-call hang confirms that call; hanging while every single passes means
/// the fault needs a *combination*; and passing exonerates the replay outright
/// and moves the search to `crate::command`'s `begin_render_pass` — the one
/// part of our path a hand-encoded probe replaces wholesale.
///
/// The seventh call `bind_graphics_pipeline` can make,
/// `setStencilReferenceValue:`, is asserted absent rather than omitted
/// silently: this pipeline declares no stencil state, so the replay it emits
/// really is six calls, and the assertion goes red if that ever stops being
/// true instead of leaving this probe quietly reproducing less than it claims.
#[test]
#[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
fn draw_probe_a_hand_encoded_pass_plus_the_whole_replay() {
    hand_encoded_probe("crcbl-mtl probe: whole replay", |_, bound, encoder| {
        assert!(
            bound.raster.stencil_reference.is_none(),
            "the replay this probe reproduces is six calls, and this pipeline wants a seventh"
        );
        encoder.setCullMode(bound.raster.cull);
        encoder.setFrontFacingWinding(bound.raster.winding);
        encoder.setTriangleFillMode(bound.raster.fill);
        encoder.setDepthClipMode(bound.raster.clip);
        let [constant, slope_scale, clamp] = bound.raster.bias;
        encoder.setDepthBias_slopeScale_clamp(constant, slope_scale, clamp);
        encoder.setDepthStencilState(bound.depth_stencil.as_deref());
    });
}

/// Runs one probe: the known-good hand-encoded pass, plus whatever `replay`
/// adds to it.
///
/// Every probe in this round is this function with a different `replay`, which
/// is what makes the set a bisect: the device, the pipeline, the pass, the
/// draw and the assertion are written once, so the emitted calls are the only
/// difference between two results. `label` names the pipeline, the command
/// buffer and the render encoder, so a fault report says which probe produced
/// it rather than "one of them faulted".
///
/// `replay` is handed the device — for the one probe that must build an object
/// this backend would not — and the resolved [`BoundPipeline`], so each probe
/// passes the value `bind_graphics_pipeline` would pass rather than a constant
/// written out here that could drift from it.
fn hand_encoded_probe(
    label: &str,
    replay: impl FnOnce(&MetalDevice, &BoundPipeline, &ProtocolObject<dyn MTLRenderCommandEncoder>),
) {
    let (_instance, device) = open_device();
    let ink = InkPipeline::new(&device, label);
    let bound = device
        .inner
        .graphics_pipeline_raw(ink.pipeline)
        .expect("the pipeline was created on this device a moment ago");

    let bytes = hand_encoded_canvas(&device, label, &bound.raw, |encoder| {
        replay(&device, &bound, encoder);
    });
    assert_ink_triangle(&bytes, FORMAT);

    ink.destroy(&device);
}

/// Draws `state`'s triangle into a [`CANVAS`]-sized target with a **hand-built
/// render encoder**, and reads the texels back.
///
/// The render pass is the whole of what is hand-built, and inside it the order
/// is `crate::command`'s: `setRenderPipelineState:`, then whatever `replay`
/// emits, then the draw. The copy that follows goes through this crate's
/// `CommandEncoder`, `submit` and `poll_readback`, because those are the calls
/// `a_render_pass_clear_reads_back_the_exact_texels` already passes with on the
/// faulting runner — rewriting them here would add a variable to a probe whose
/// point is having only one.
///
/// The two command buffers are ordered by the queue itself: Metal executes them
/// in commit order, and the render one is waited on here anyway so a hang is
/// reported with its per-encoder status rather than as a wrong image.
fn hand_encoded_canvas(
    device: &MetalDevice,
    label: &str,
    state: &ProtocolObject<dyn MTLRenderPipelineState>,
    replay: impl FnOnce(&ProtocolObject<dyn MTLRenderCommandEncoder>),
) -> Vec<u8> {
    let (image, view) = color_target_of(device, CANVAS, FORMAT);
    let readback = readback_buffer(device, CANVAS_BYTES as u64);
    let (texture, _) = device
        .inner
        .view_raw(view)
        .expect("the view was created on this device a moment ago");

    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: `objc2` marks the subscript unsafe because Metal does not
    // bounds-check the attachment index, and `crate::command`'s
    // `begin_render_pass` makes the same call for the same reason. Index 0 is
    // below the fixed length of Metal's colour-attachment array.
    let slot = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    slot.setTexture(Some(&texture));
    slot.setLoadAction(MTLLoadAction::Clear);
    slot.setStoreAction(MTLStoreAction::Store);
    slot.setClearColor(conv::clear_color(CLEAR));

    let command_buffer = crate::fault::command_buffer(&device.inner.queue, label)
        .expect("the queue vends a command buffer");
    let encoder = command_buffer
        .renderCommandEncoderWithDescriptor(&descriptor)
        .expect("a descriptor with one colour attachment opens a render encoder");
    // Labelled for the same reason every encoder in this crate is: an unlabelled
    // encoder reports as an empty string in a fault report, which turns the
    // diagnostic back into "one of them faulted".
    encoder.setLabel(Some(&NSString::from_str(label)));
    encoder.setRenderPipelineState(state);
    replay(&encoder);
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
