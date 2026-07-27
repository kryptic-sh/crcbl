//! The graph-compile suite: what the render graph produces, asserted against
//! `NullBackend`'s recorded command stream.
//!
//! `docs/plan/12-testing.md` lists this among the per-subsystem "non-negotiables"
//! — "`crcbl-hal`/`crcbl-vk`/`crcbl-wgpu`: graph-compile unit suite on
//! NullBackend" — and `crcbl-hal`'s null backend exists to make exactly this
//! assertable. Every test here runs **with no ICD, no driver and no GPU**, which
//! is what lets it run on the macOS and Windows CI legs and under
//! `VK_ICD_FILENAMES=/nonexistent.json`.
//!
//! The suite checks two different things, and the distinction matters:
//!
//! 1. **What `compile` decided** — pass order, barrier placement, layout
//!    transitions, transient aliasing, final states. Pure, no device.
//! 2. **What `execute` actually recorded** — that the stream the backend saw is
//!    the one compilation predicted, in that order, with no barrier the graph
//!    did not compute and none it did missing.
//!
//! The second is what makes the first evidence rather than a self-consistent
//! model of itself.

use crcbl_hal::null::{Command, NullInstance, ObjectKind, Recorder};
use crcbl_hal::{
    CommandEncoderDesc, Device, DeviceDesc, Format, ImageUsage, Instance, QueueHandle, QueueKind,
    ResourceState,
};
use crcbl_render::graph::{GraphBarriers, GraphError, ImportedBuffer, ImportedImage, RenderGraph};
use crcbl_render::transient::{TransientBufferDesc, TransientImageDesc, TransientPool};

const EXTENT: (u32, u32) = (256, 192);

struct Harness {
    recorder: Recorder,
    device: Box<dyn Device>,
    queue: QueueHandle,
}

impl Harness {
    fn open() -> Self {
        let recorder = Recorder::new();
        let instance = NullInstance::tier_a().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("graph compile"),
                adapter: adapter.id,
                required_features: crcbl_hal::Features::TIER_A,
                // `TIMESTAMP_QUERY` is deliberately *not* in `TIER_A` — topic
                // 10's browsers may lack it — so a device that wants per-pass
                // timers has to ask, and `DeviceDesc::for_adapter` does not.
                optional_features: crcbl_hal::Features::TIMESTAMP_QUERY,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        Self {
            recorder,
            device,
            queue,
        }
    }

    fn graph(&self) -> RenderGraph<'static> {
        RenderGraph::new(self.queue)
    }

    /// A stand-in for a swapchain image: created by hand, imported into the
    /// graph exactly as an acquired frame would be.
    fn target(&self, format: Format) -> ImportedImage {
        let image = self
            .device
            .create_image(&crcbl_hal::ImageDesc {
                label: Some("fake swapchain image"),
                image_type: crcbl_hal::ImageType::D2,
                extent: crcbl_hal::Extent3d::d2(EXTENT.0, EXTENT.1),
                format,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::PRESENT,
                memory: crcbl_hal::MemoryLocation::DeviceLocal,
            })
            .expect("an image");
        let view = self
            .device
            .create_image_view(&crcbl_hal::ImageViewDesc {
                label: Some("fake swapchain view"),
                image,
                view_type: crcbl_hal::ImageViewType::D2,
                format,
                range: crcbl_hal::ImageSubresourceRange::all(format),
            })
            .expect("a view");
        ImportedImage {
            image,
            view,
            format,
            extent: EXTENT,
            initial: ResourceState::Undefined,
            final_state: ResourceState::Present,
        }
    }

    fn record(&self, run: impl FnOnce(&mut dyn crcbl_hal::CommandEncoder)) {
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("graph frame"),
            queue: self.queue,
        });
        run(encoder.as_mut());
        let commands = encoder.finish().expect("recording succeeded");
        self.device.destroy_command_buffer(commands);
    }

    /// Every barrier the backend actually saw, in order.
    fn recorded_barriers(&self) -> Vec<Command> {
        self.recorder
            .commands()
            .into_iter()
            .filter(|command| matches!(command, Command::Barrier { .. }))
            .collect()
    }
}

fn scene_color() -> TransientImageDesc {
    TransientImageDesc::scene_color(EXTENT)
}

fn scene_depth() -> TransientImageDesc {
    TransientImageDesc::scene_depth(EXTENT)
}

/// The frame `apps/sandbox` actually draws, compiled and recorded.
///
/// This is the P1 exit criterion "graph dump readable and correct for the
/// sandbox frame", turned into an assertion about the *stream* rather than about
/// the text.
#[test]
fn the_sandbox_frame_compiles_to_the_passes_and_barriers_it_should() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let mut graph = harness.graph();
    let color = graph.create_image("scene-color", scene_color());
    let depth = graph.create_image("scene-depth", scene_depth());
    let swap = graph.import_image("swapchain", target);

    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .clear_depth(depth)
        .execute(|ctx| {
            ctx.encoder().draw(0..36, 0..1);
        });
    graph
        .add_render_pass("tonemap")
        .color(
            swap,
            crcbl_hal::LoadOp::DontCare,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .read_image(color)
        .execute(|ctx| {
            ctx.encoder().draw(0..3, 0..1);
        });

    let compiled = graph.compile().expect("a legal frame");
    assert_eq!(compiled.passes().len(), 2);
    assert_eq!(compiled.passes()[0].label(), "forward");
    assert_eq!(compiled.passes()[1].label(), "tonemap");

    // Pass 0: two fresh transients, both from `Undefined`.
    let first = compiled.passes()[0].barriers();
    assert_eq!(first.images.len(), 2, "{first:?}");
    assert_eq!(first.images[0].from, ResourceState::Undefined);
    assert_eq!(first.images[0].to, ResourceState::ColorAttachment);
    assert_eq!(first.images[1].from, ResourceState::Undefined);
    assert_eq!(
        first.images[1].to,
        ResourceState::DepthStencilWrite,
        "a written depth attachment is a write state, not a read"
    );
    assert!(first.buffers.is_empty());

    // Pass 1: the swapchain image becomes a colour attachment, and the scene
    // target moves from being rendered into to being sampled. **That second
    // transition is the whole point of the graph** — it is the one a hand-written
    // frame forgets, and it is a layout change with a real hazard behind it.
    let second = compiled.passes()[1].barriers();
    assert_eq!(second.images.len(), 2, "{second:?}");
    assert_eq!(second.images[0].from, ResourceState::Undefined);
    assert_eq!(second.images[0].to, ResourceState::ColorAttachment);
    assert_eq!(second.images[1].from, ResourceState::ColorAttachment);
    assert_eq!(second.images[1].to, ResourceState::ShaderRead);

    // And the frame ends by returning the imported image to its owner's state,
    // which is the barrier `apps/sandbox` used to write by hand.
    let last = compiled.final_barriers();
    assert_eq!(last.images.len(), 1, "{last:?}");
    assert_eq!(last.images[0].from, ResourceState::ColorAttachment);
    assert_eq!(last.images[0].to, ResourceState::Present);

    // The two scene targets have different formats, so they cannot alias — and
    // the graph must not pretend otherwise.
    assert!(!compiled.images_alias(color, depth));
    assert_eq!(compiled.physical_image_count(), 2);

    let expected: Vec<GraphBarriers> = compiled
        .barrier_batches()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("execution succeeded");
    });

    // The recorded stream is exactly the compiled plan: three barrier calls, no
    // more and no fewer, each carrying what compilation said it would.
    let recorded = harness.recorded_barriers();
    assert_eq!(
        recorded.len(),
        expected.len(),
        "the backend saw {} barrier call(s) but compilation predicted {}",
        recorded.len(),
        expected.len()
    );
    for (index, (command, plan)) in recorded.iter().zip(&expected).enumerate() {
        let Command::Barrier {
            images,
            buffers,
            global,
        } = command
        else {
            unreachable!("filtered to barriers");
        };
        assert!(!global, "batch {index} used the sledgehammer");
        assert_eq!(images.len(), plan.images.len(), "batch {index}");
        assert_eq!(buffers.len(), plan.buffers.len(), "batch {index}");
        for (recorded, planned) in images.iter().zip(&plan.images) {
            assert_eq!(recorded.from, planned.from, "batch {index}");
            assert_eq!(recorded.to, planned.to, "batch {index}");
        }
    }

    assert_eq!(
        harness.recorder.pass_labels(),
        vec!["forward".to_string(), "tonemap".to_string()],
        "passes execute in declaration order"
    );
    harness.recorder.assert_valid();
    pool.destroy(harness.device.as_ref());
}

/// The graph's own dump, checked against the frame it describes.
///
/// `docs/plan/02-vulkan-backend.md` §2.4's debug principle is that "the graph
/// must be able to explain itself", and its exit criteria require the dump to be
/// "readable and correct". Correct is the checkable half: every barrier in the
/// compiled plan appears in the text, with its states.
#[test]
fn the_dump_describes_every_pass_and_every_barrier() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let mut graph = harness.graph();
    let color = graph.create_image("scene-color", scene_color());
    let depth = graph.create_image("scene-depth", scene_depth());
    let swap = graph.import_image("swapchain", target);
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .clear_depth(depth)
        .execute(|_| {});
    graph
        .add_render_pass("tonemap")
        .color(
            swap,
            crcbl_hal::LoadOp::DontCare,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .read_image(color)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    let dump = compiled.dump();
    eprintln!("{dump}");

    for expected in [
        "render graph: 2 pass(es)",
        "[0] render pass \"forward\"",
        "[1] render pass \"tonemap\"",
        "256x192",
        "scene-color",
        "scene-depth",
        "swapchain",
        "Undefined",
        "ColorAttachment",
        "DepthStencilWrite",
        "ShaderRead",
        "Present",
        "[final] barriers",
        "transient #0",
        "imported",
        "color[0]",
        "depth load=Clear",
    ] {
        assert!(
            dump.contains(expected),
            "the dump does not mention {expected:?}:\n{dump}"
        );
    }

    // Every barrier compilation computed must appear as a line, or the dump is
    // describing a different frame from the one that will run.
    let barrier_lines = dump.lines().filter(|line| line.contains(" -> ")).count();
    let planned: usize = compiled
        .barrier_batches()
        .iter()
        .map(|batch| batch.len())
        .sum();
    assert_eq!(
        barrier_lines, planned,
        "the dump shows {barrier_lines} transitions but the plan has {planned}:\n{dump}"
    );
}

/// Two transients whose lifetimes do not overlap share one physical image; the
/// second one's first use starts from `Undefined`, because the pixels in it
/// belong to the first.
#[test]
fn non_overlapping_transients_alias_onto_one_physical_image() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let mut graph = harness.graph();
    let first = graph.create_image("blur-a", scene_color());
    let second = graph.create_image("blur-b", scene_color());
    let swap = graph.import_image("swapchain", target);

    // `blur-a` dies at pass 0; `blur-b` is born at pass 1.
    graph
        .add_render_pass("write-a")
        .clear_color(first, [0.0; 4])
        .execute(|_| {});
    graph
        .add_render_pass("write-b")
        .clear_color(second, [0.0; 4])
        .execute(|_| {});
    graph
        .add_render_pass("present")
        .color(
            swap,
            crcbl_hal::LoadOp::DontCare,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .read_image(second)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    assert!(
        compiled.images_alias(first, second),
        "identical descriptions with disjoint lifetimes must share one image"
    );
    assert_eq!(
        compiled.physical_image_count(),
        1,
        "two transients packed onto one physical image"
    );

    // The aliased slot is re-acquired from `Undefined`: whatever `blur-a` left
    // there is not `blur-b`'s, and discarding it is both correct and free.
    let reuse = compiled.passes()[1].barriers();
    assert_eq!(reuse.images.len(), 1);
    assert_eq!(
        reuse.images[0].from,
        ResourceState::Undefined,
        "an aliased slot must not claim to hold the previous resource's state"
    );
    assert_eq!(reuse.images[0].to, ResourceState::ColorAttachment);

    assert!(compiled.dump().contains("aliasing saved 1 physical image"));

    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    // One physical image, so the pool created exactly one.
    assert_eq!(pool.image_count(), 1);
    harness.recorder.assert_valid();
    pool.destroy(harness.device.as_ref());
}

/// Overlapping lifetimes must **not** alias, however identical the descriptions
/// — that is the difference between an optimisation and a bug.
#[test]
fn overlapping_transients_do_not_alias() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let first = graph.create_image("a", scene_color());
    let second = graph.create_image("b", scene_color());

    graph
        .add_render_pass("write-both")
        .clear_color(first, [0.0; 4])
        .clear_color(second, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    assert!(!compiled.images_alias(first, second));
    assert_eq!(compiled.physical_image_count(), 2);
    assert!(!compiled.dump().contains("aliasing saved"));
}

/// A transient that is written, read, and then written again by a *third*
/// resource is the interesting aliasing case: the middle read extends the first
/// resource's lifetime, so the third cannot take its slot until after it.
#[test]
fn a_read_extends_a_transients_lifetime_and_delays_reuse() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let first = graph.create_image("a", scene_color());
    let second = graph.create_image("b", scene_color());

    graph
        .add_render_pass("write-a")
        .clear_color(first, [0.0; 4])
        .execute(|_| {});
    graph
        .add_render_pass("write-b-read-a")
        .clear_color(second, [0.0; 4])
        .read_image(first)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    assert!(
        !compiled.images_alias(first, second),
        "`b` is written while `a` is still being read; sharing would corrupt both"
    );
    assert_eq!(compiled.physical_image_count(), 2);
}

/// Two consecutive passes that read the same resource in the same state need
/// **no** barrier between them — the seam's `needs_barrier` says so and the
/// graph must not emit one anyway.
#[test]
fn read_after_read_emits_nothing() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    let swap = graph.import_image("swapchain", target);

    graph
        .add_render_pass("write")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});
    graph
        .add_render_pass("read-once")
        .color(
            swap,
            crcbl_hal::LoadOp::Clear,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .read_image(color)
        .execute(|_| {});
    graph
        .add_render_pass("read-again")
        .color(
            swap,
            crcbl_hal::LoadOp::Load,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .read_image(color)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    let third = compiled.passes()[2].barriers();
    assert!(
        !third.images.iter().any(|barrier| barrier.image == color),
        "a second read of an already-readable resource needs no barrier: {third:?}"
    );
    // The swapchain image *does* still get one: two passes writing the same
    // colour attachment is a write-after-write hazard, however unchanged its
    // layout. That is `ResourceState::needs_barrier`'s rule, not the graph
    // inventing work, and conflating the two cases is how a read-after-read
    // optimisation quietly deletes a real dependency.
    assert_eq!(
        third
            .images
            .iter()
            .filter(|barrier| barrier.image == swap)
            .count(),
        1,
        "{third:?}"
    );

    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    assert_eq!(
        harness.recorded_barriers().len(),
        4,
        "one per pass plus the final present transition — never an empty call"
    );
    pool.destroy(harness.device.as_ref());
}

/// A read-only depth attachment is `DepthStencilRead`, not `…Write` — the
/// difference between a prepass consumer and a pass that clobbers it.
#[test]
fn a_read_only_depth_attachment_uses_the_read_state() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("color", scene_color());
    let depth = graph.create_image("depth", scene_depth());

    graph
        .add_render_pass("prepass")
        .clear_color(color, [0.0; 4])
        .depth(
            depth,
            crcbl_hal::LoadOp::Clear,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .execute(|_| {});
    graph
        .add_render_pass("shade")
        .color(
            color,
            crcbl_hal::LoadOp::Load,
            crcbl_hal::StoreOp::Store,
            crcbl_hal::ClearValue::default(),
        )
        .depth_read(depth)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    let second = compiled.passes()[1].barriers();
    let depth_barrier = second
        .images
        .iter()
        .find(|barrier| barrier.to == ResourceState::DepthStencilRead)
        .expect("the depth attachment must transition to the read state");
    assert_eq!(depth_barrier.from, ResourceState::DepthStencilWrite);
}

/// Buffers are tracked exactly as images are, including the imported final
/// state — which is what a P7 indirect-argument buffer will depend on.
#[test]
fn buffers_transition_and_return_to_their_final_state() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);
    let uniform = harness
        .device
        .create_buffer(&crcbl_hal::BufferDesc {
            label: Some("camera"),
            size: 256,
            usage: crcbl_hal::BufferUsage::UNIFORM,
            memory: crcbl_hal::MemoryLocation::HostUpload,
        })
        .expect("a buffer");

    let mut graph = harness.graph();
    let swap = graph.import_image("swapchain", target);
    let camera = graph.import_buffer(
        "camera",
        ImportedBuffer {
            buffer: uniform,
            initial: ResourceState::HostRead,
            final_state: ResourceState::HostRead,
        },
    );
    let scratch = graph.create_buffer("cull-args", TransientBufferDesc::storage(4096));

    graph
        .add_compute_pass("cull")
        .use_buffer(scratch, ResourceState::ShaderWrite)
        .read_buffer(camera)
        .execute(|ctx| {
            ctx.encoder().dispatch(1, 1, 1);
        });
    graph
        .add_render_pass("draw")
        .clear_color(swap, [0.0; 4])
        .use_buffer(scratch, ResourceState::IndirectArgument)
        .read_buffer(camera)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");

    // The single most important barrier in a GPU-driven frame, per the seam's
    // own docs: the culling output reaching `IndirectArgument` before the draws
    // that read it.
    let draw = compiled.passes()[1].barriers();
    let args = draw
        .buffers
        .iter()
        .find(|barrier| barrier.to == ResourceState::IndirectArgument)
        .expect("the compute output must be made readable as indirect arguments");
    assert_eq!(args.from, ResourceState::ShaderWrite);

    // The camera buffer was read in both passes in the same state, so exactly
    // one transition happened — into `ShaderRead` at the first pass.
    let cull = compiled.passes()[0].barriers();
    assert_eq!(
        cull.buffers
            .iter()
            .filter(|barrier| barrier.to == ResourceState::ShaderRead)
            .count(),
        1
    );
    assert!(
        !draw
            .buffers
            .iter()
            .any(|barrier| barrier.to == ResourceState::ShaderRead),
        "a second read in the same state needs no barrier: {draw:?}"
    );

    // And it goes home: the CPU maps it again next frame.
    let last = compiled.final_barriers();
    assert!(
        last.buffers
            .iter()
            .any(|barrier| barrier.to == ResourceState::HostRead),
        "an imported buffer must be returned to its owner's state: {last:?}"
    );

    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    // A compute pass and a render pass, correctly nested and correctly scoped —
    // the null backend records a validation error if either is wrong.
    harness.recorder.assert_valid();
    assert_eq!(
        harness.recorder.pass_labels(),
        vec!["cull".to_string(), "draw".to_string()]
    );
    pool.destroy(harness.device.as_ref());
    harness.device.destroy_buffer(uniform);
}

/// The barrier model represents queue-family acquire/release from the start,
/// which `docs/plan/02-vulkan-backend.md`'s corrections require so a dedicated
/// transfer queue is additive later rather than a rewrite.
///
/// Nothing in the MVP uses a second queue. This test does, to prove the model
/// can express it.
#[test]
fn a_resource_crossing_queues_gets_an_ownership_transfer() {
    let harness = Harness::open();
    let transfer = harness
        .device
        .queue(QueueKind::Transfer)
        .expect("the tier A null adapter models a transfer queue");
    assert_ne!(transfer, harness.queue);

    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());

    graph
        .add_render_pass("draw")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});
    graph
        .add_compute_pass("readback")
        .on_queue(transfer)
        .use_image(color, ResourceState::TransferSrc)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    let second = compiled.passes()[1].barriers();
    let barrier = second.images[0];
    assert_eq!(barrier.from, ResourceState::ColorAttachment);
    assert_eq!(barrier.to, ResourceState::TransferSrc);
    let ownership = barrier
        .queue_transfer
        .expect("crossing queue families is an ownership transfer, not a plain transition");
    assert_eq!(ownership.from, harness.queue);
    assert_eq!(ownership.to, transfer);
    assert!(compiled.dump().contains("queue "), "{}", compiled.dump());
}

/// Executing a graph must leave the backend holding exactly what it held before,
/// plus the pool's own resources — and destroying the pool must return it to
/// where it started.
#[test]
fn a_frame_leaks_nothing() {
    let harness = Harness::open();
    let before = harness.recorder.total_live_objects();
    let mut pool = TransientPool::new();

    for frame in 0..8 {
        let mut graph = harness.graph();
        let color = graph.create_image("scene", scene_color());
        let depth = graph.create_image("depth", scene_depth());
        graph
            .add_render_pass("forward")
            .clear_color(color, [0.0; 4])
            .clear_depth(depth)
            .execute(|_| {});
        let compiled = graph.compile().expect("a legal frame");
        harness.record(|encoder| {
            compiled
                .execute(harness.device.as_ref(), &mut pool, encoder, None)
                .expect("executed");
        });
        pool.retire_unused(harness.device.as_ref());
        assert_eq!(
            pool.image_count(),
            2,
            "frame {frame}: a steady-state frame must reuse its transients"
        );
    }

    // Two images and two views live in the pool; nothing else accumulated.
    assert_eq!(
        harness.recorder.live_objects(ObjectKind::Image),
        2,
        "the pool holds one image per transient, and no frame added another"
    );
    assert_eq!(harness.recorder.live_objects(ObjectKind::CommandBuffer), 0);
    pool.destroy(harness.device.as_ref());
    assert_eq!(harness.recorder.total_live_objects(), before);
    harness.recorder.assert_valid();
}

/// The pass body really does run inside the pass scope, with a viewport and
/// scissor already set to the render area — so a body is a bind and a draw
/// rather than four lines of boilerplate every pass repeats.
#[test]
fn the_graph_opens_the_pass_and_sets_the_dynamic_state() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .execute(|ctx| {
            assert_eq!(ctx.render_area().width, EXTENT.0);
            assert_eq!(ctx.render_area().height, EXTENT.1);
            ctx.encoder().draw(0..3, 0..1);
        });

    let compiled = graph.compile().expect("a legal frame");
    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });

    assert_eq!(
        harness.recorder.command_names(),
        vec![
            "Barrier",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "Draw",
            "EndRenderPass",
        ]
    );
    let viewport = harness
        .recorder
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::SetViewport(viewport) => Some(viewport),
            _ => None,
        })
        .expect("the graph sets one");
    assert_eq!(viewport.width, EXTENT.0 as f32);
    assert_eq!(viewport.height, EXTENT.1 as f32);
    // Reversed-Z lives in the projection matrix, never in the viewport range.
    assert_eq!(viewport.depth_min, 0.0);
    assert_eq!(viewport.depth_max, 1.0);
    harness.recorder.assert_valid();
    pool.destroy(harness.device.as_ref());
}

/// Per-pass GPU timers bracket each pass **outside** its scope, which is where
/// the seam's rules put query writes.
#[test]
fn timers_bracket_every_pass_outside_its_scope() {
    let harness = Harness::open();
    let mut timers = crcbl_render::PassTimers::new(harness.device.as_ref(), 2, 8)
        .expect("the tier A null adapter has timestamp queries");

    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});
    graph
        .add_compute_pass("post")
        .use_image(color, ResourceState::ShaderReadWrite)
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(
                harness.device.as_ref(),
                &mut pool,
                encoder,
                Some(&mut timers),
            )
            .expect("executed");
    });

    let names = harness.recorder.command_names();
    assert_eq!(
        names,
        vec![
            "ResetQuerySet",
            "WriteTimestamp",
            "Barrier",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "EndRenderPass",
            "WriteTimestamp",
            "WriteTimestamp",
            "Barrier",
            "BeginComputePass",
            "EndComputePass",
            "WriteTimestamp",
        ],
        "timestamps must sit outside the passes they measure — the seam forbids \
         query writes inside a pass"
    );
    // Which the null backend agrees with, or it would have recorded a violation.
    harness.recorder.assert_valid();

    timers.destroy(harness.device.as_ref());
    pool.destroy(harness.device.as_ref());
}

// --- the graph refuses what it cannot execute -------------------------------

#[test]
fn a_pass_that_wants_one_resource_in_two_states_is_refused() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("confused")
        .clear_color(color, [0.0; 4])
        .read_image(color)
        .execute(|_| {});

    let error = graph.compile().expect_err("one state per pass");
    assert!(
        matches!(error, GraphError::ConflictingAccess { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("scene"), "{error}");
}

#[test]
fn a_render_pass_with_no_attachments_is_refused() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("nowhere")
        .read_image(color)
        .execute(|_| {});

    let error = graph
        .compile()
        .expect_err("a render pass renders somewhere");
    assert!(matches!(error, GraphError::NoAttachments { .. }), "{error}");
}

#[test]
fn attachments_of_different_sizes_are_refused() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene-color", scene_color());
    let depth = graph.create_image(
        "scene-depth",
        TransientImageDesc::scene_depth((EXTENT.0 / 2, EXTENT.1)),
    );
    graph
        .add_render_pass("mismatched")
        .clear_color(color, [0.0; 4])
        .clear_depth(depth)
        .execute(|_| {});

    let error = graph.compile().expect_err("attachments must agree");
    assert!(
        matches!(error, GraphError::AttachmentExtentMismatch { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("scene-depth"), "{error}");
}

#[test]
fn two_depth_attachments_are_refused() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("color", scene_color());
    let first = graph.create_image("depth-a", scene_depth());
    let second = graph.create_image("depth-b", scene_depth());
    graph
        .add_render_pass("greedy")
        .clear_color(color, [0.0; 4])
        .clear_depth(first)
        .clear_depth(second)
        .execute(|_| {});

    let error = graph.compile().expect_err("one depth attachment");
    assert!(
        matches!(error, GraphError::DuplicateDepthAttachment { .. }),
        "{error}"
    );
}

#[test]
fn an_attachment_on_a_compute_pass_is_refused() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_compute_pass("confused")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});

    let error = graph
        .compile()
        .expect_err("compute passes have no attachments");
    assert!(
        matches!(error, GraphError::AttachmentInComputePass { .. }),
        "{error}"
    );
}

/// A transient nothing uses has no lifetime, so it cannot be allocated or
/// aliased — and it is nearly always a pass that forgot to declare a read.
#[test]
fn an_unused_transient_is_refused_rather_than_silently_allocated() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let used = graph.create_image("used", scene_color());
    let _forgotten = graph.create_image("forgotten", scene_color());
    graph
        .add_render_pass("forward")
        .clear_color(used, [0.0; 4])
        .execute(|_| {});

    let error = graph
        .compile()
        .expect_err("an unused transient is a mistake");
    assert!(
        matches!(error, GraphError::UnusedTransient { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("forgotten"), "{error}");
}

/// An imported resource that no pass touches is *not* an error: importing one
/// purely to transition it is a legitimate thing to ask for, and the graph does
/// exactly that.
#[test]
fn an_untouched_import_is_transitioned_rather_than_refused() {
    let harness = Harness::open();
    let target = harness.target(Format::Bgra8UnormSrgb);
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    let swap = graph.import_image("swapchain", target);
    let _ = swap;
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile().expect("an untouched import is legal");
    let last = compiled.final_barriers();
    assert_eq!(last.images.len(), 1);
    assert_eq!(last.images[0].from, ResourceState::Undefined);
    assert_eq!(last.images[0].to, ResourceState::Present);
}

/// An import already in its final state needs no trailing barrier. The graph is
/// finished with it, so the transition would guard nothing and change no layout
/// — unlike a write-after-write *between two passes*, which is a real hazard and
/// does get one.
#[test]
fn an_import_left_in_its_final_state_gets_no_trailing_barrier() {
    let harness = Harness::open();
    let mut target = harness.target(Format::Bgra8UnormSrgb);
    target.final_state = ResourceState::ColorAttachment;

    let mut graph = harness.graph();
    let swap = graph.import_image("swapchain", target);
    graph
        .add_render_pass("forward")
        .clear_color(swap, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile().expect("a legal frame");
    assert!(
        compiled.final_barriers().is_empty(),
        "{:?}",
        compiled.final_barriers()
    );
}

/// Declaration order is execution order, with no sorting and no surprises —
/// which is the whole of `docs/plan/02-vulkan-backend.md`'s "no reordering".
#[test]
fn passes_run_in_the_order_they_were_declared() {
    let harness = Harness::open();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());

    // Deliberately declared so a topological sort would have latitude: pass
    // "second" depends on nothing "first" produces.
    let names = ["zulu", "alpha", "mike"];
    for (index, name) in names.iter().enumerate() {
        let load = if index == 0 {
            crcbl_hal::LoadOp::Clear
        } else {
            crcbl_hal::LoadOp::Load
        };
        graph
            .add_render_pass(*name)
            .color(
                color,
                load,
                crcbl_hal::StoreOp::Store,
                crcbl_hal::ClearValue::default(),
            )
            .execute(|_| {});
    }

    let compiled = graph.compile().expect("a legal frame");
    let order: Vec<&str> = compiled.passes().iter().map(|pass| pass.label()).collect();
    assert_eq!(order, names);

    let mut pool = TransientPool::new();
    harness.record(|encoder| {
        compiled
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    assert_eq!(
        harness.recorder.pass_labels(),
        names.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    // Successive writes to the same attachment are still hazards, so each of
    // the later two gets its own barrier.
    assert_eq!(harness.recorded_barriers().len(), 3);
    pool.destroy(harness.device.as_ref());
}
