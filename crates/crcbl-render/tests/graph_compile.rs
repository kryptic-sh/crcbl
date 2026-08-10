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
    CommandEncoderDesc, Device, DeviceDesc, Format, ImageAspect, ImageSubresourceRange, ImageUsage,
    Instance, QueueHandle, QueueKind, ResourceState,
};
use crcbl_render::graph::{
    GraphBarriers, GraphError, ImageId, ImportedBuffer, ImportedImage, RenderGraph,
};
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
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("graph compile"),
                adapter: adapter.id,
                required_features: crcbl_hal::Features::GPU_DRIVEN,
                // `TIMESTAMP_QUERY` is deliberately *not* in `GPU_DRIVEN` — topic
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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

/// Builds the sandbox frame — forward into an HDR target plus depth, then
/// tonemap into an imported target — against `pool`, and returns what compiled.
///
/// A helper rather than three copies, because the cross-frame tests below are
/// only interesting if the *second* frame is the same frame as the first.
fn sandbox_frame<'a>(
    harness: &Harness,
    pool: &TransientPool,
    target: ImportedImage,
) -> (crcbl_render::CompiledGraph<'a>, ImageId) {
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
    (graph.compile(pool).expect("a legal frame"), swap)
}

/// **The second frame is barriered against the first, not against nothing.**
///
/// A transient is transient in the graph, not in the pool: the second frame gets
/// the same `VkImage` back, while the first frame's writes to it may still be in
/// flight — the frame loop keeps two submissions going and nothing in between
/// them says "wait". So the first barrier the second frame emits against a
/// pooled resource is the *only* thing that can order the two, and a barrier out
/// of [`ResourceState::Undefined`] cannot: on Vulkan it expands to
/// `srcStageMask = NONE, srcAccessMask = NONE`, which is a write-after-write
/// hazard with `write_barriers: 0` and a picture that depends on the driver.
///
/// This is the regression test for exactly that. It needs no GPU, no ICD and no
/// validation layer — which is the point: the bug it covers was found by a
/// driver on one CI leg, and finding it that way is luck.
#[test]
fn a_second_frame_barriers_against_what_the_first_one_left() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();
    let target = harness.target(Format::Bgra8UnormSrgb);

    // Frame one, through a pool that has never been used. Nothing has written
    // these images, so `Undefined` is the truth and not an omission.
    let (first, _) = sandbox_frame(&harness, &pool, target);
    let opening = first.passes()[0].barriers();
    assert_eq!(opening.images[0].from, ResourceState::Undefined);
    assert_eq!(opening.images[1].from, ResourceState::Undefined);
    harness.record(|encoder| {
        first
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    assert_eq!(pool.image_count(), 2, "one colour target, one depth target");

    // Frame two, through the same pool. Same declarations, different barriers —
    // because the resources are not in the same place they were.
    let (second, swap) = sandbox_frame(&harness, &pool, target);
    let opening = second.passes()[0].barriers();
    assert_eq!(opening.images.len(), 2, "{opening:?}");

    // The colour target: the previous frame's tonemap sampled it last, so this
    // frame's colour writes have to be ordered after that read.
    assert_eq!(
        (opening.images[0].from, opening.images[0].to),
        (ResourceState::ShaderRead, ResourceState::ColorAttachment),
        "the scene target's first barrier must cover the previous frame's read \
         of it, or this frame overwrites pixels the last one is still sampling"
    );

    // The depth target: the previous frame's forward pass wrote it and nothing
    // read it afterwards, so this is the write-after-write the driver caught.
    assert_eq!(
        (opening.images[1].from, opening.images[1].to),
        (
            ResourceState::DepthStencilWrite,
            ResourceState::DepthStencilWrite
        ),
        "the depth target's first barrier must cover the previous frame's depth \
         writes; `Undefined` here is `srcStageMask = NONE` and covers nothing"
    );

    // Said once more as a property rather than as two cases, so a third
    // transient added to the frame cannot quietly reintroduce the hole. The
    // imported target is exempt and stays exempt: its owner declares
    // `Undefined`, and for an acquired swapchain image that is correct — the
    // acquire semaphore carries the ordering the barrier does not.
    for batch in second.barrier_batches() {
        for barrier in &batch.images {
            assert!(
                barrier.from != ResourceState::Undefined || barrier.image == swap,
                "a pooled resource the graph has already used cannot re-enter a \
                 frame as `Undefined`: {barrier:?}"
            );
        }
    }

    // And the backend really sees it, rather than the plan merely saying so.
    let expected: Vec<GraphBarriers> = second.barrier_batches().into_iter().cloned().collect();
    harness.recorder.clear();
    harness.record(|encoder| {
        second
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    assert_eq!(
        pool.image_count(),
        2,
        "a steady-state frame reuses; if it allocated, the states above would be \
         describing images this frame never touched"
    );
    let recorded = harness.recorded_barriers();
    assert_eq!(recorded.len(), expected.len());
    for (index, (command, plan)) in recorded.iter().zip(&expected).enumerate() {
        let Command::Barrier { images, .. } = command else {
            unreachable!("filtered to barriers");
        };
        for (recorded, planned) in images.iter().zip(&plan.images) {
            assert_eq!(recorded.from, planned.from, "batch {index}");
            assert_eq!(recorded.to, planned.to, "batch {index}");
        }
    }
    harness.recorder.assert_valid();
    pool.destroy(harness.device.as_ref());
}

/// A frame that is compiled and then dropped must not move the pool on.
///
/// The handover happens in `execute`, after the commands exist, precisely so a
/// graph that was compiled speculatively — or that failed before it ran — cannot
/// leave the next frame ordering itself against writes the GPU never performed.
/// That direction of the mistake is the dangerous one: it produces barriers that
/// *look* right and synchronise nothing.
#[test]
fn compiling_a_frame_that_never_runs_leaves_the_pool_where_it_was() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let (executed, _) = sandbox_frame(&harness, &pool, target);
    harness.record(|encoder| {
        executed
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });

    // Compiled, inspected, dropped.
    let (after_a_dropped_frame, _) = sandbox_frame(&harness, &pool, target);
    let dropped_opening = after_a_dropped_frame.passes()[0].barriers().clone();
    drop(after_a_dropped_frame);

    // The next frame that *does* run sees exactly what the dropped one saw.
    let (next, _) = sandbox_frame(&harness, &pool, target);
    assert_eq!(
        next.passes()[0].barriers(),
        &dropped_opening,
        "a compiled-and-dropped frame moved the pool's idea of where its \
         resources are"
    );
    harness.record(|encoder| {
        next.execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    pool.destroy(harness.device.as_ref());
}

/// A resize gives the pool a resource it has never seen, and the graph must say
/// `Undefined` for it rather than inherit the old size's history.
///
/// The description is the pool's key, so a new extent is a new resource with no
/// past — and carrying a state across that boundary would name a layout the new
/// image is not in, which is a validation error rather than a wrong picture.
#[test]
fn a_resize_starts_the_new_targets_from_undefined_again() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();
    let target = harness.target(Format::Bgra8UnormSrgb);

    let (first, _) = sandbox_frame(&harness, &pool, target);
    harness.record(|encoder| {
        first
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });

    let mut graph = harness.graph();
    let color = graph.create_image(
        "scene-color",
        TransientImageDesc::scene_color((320, 200)), // A different size.
    );
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});
    let resized = graph.compile(&pool).expect("a legal frame");
    assert_eq!(
        resized.passes()[0].barriers().images[0].from,
        ResourceState::Undefined,
        "a target the pool has never handed out has no history to carry"
    );

    harness.record(|encoder| {
        resized
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    harness.recorder.assert_valid();
    pool.destroy(harness.device.as_ref());
}

/// Transient **buffers** carry across frames on the same rule as images.
///
/// A pooled scratch buffer is the same `VkBuffer` next frame, so a frame that
/// opens by writing it has to be ordered after the previous frame's writes. It
/// has no layout, which is exactly why it is worth its own test: the image case
/// would still look right in a capture from the layout alone, and this one would
/// not look like anything at all until it produced wrong numbers.
#[test]
fn a_transient_buffers_state_carries_across_frames_too() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();

    let frame = |pool: &TransientPool| {
        let mut graph = harness.graph();
        let scratch = graph.create_buffer("cull-args", TransientBufferDesc::storage(4096));
        graph
            .add_compute_pass("cull")
            .use_buffer(scratch, ResourceState::ShaderWrite)
            .execute(|ctx| {
                ctx.encoder().dispatch(1, 1, 1);
            });
        graph.compile(pool).expect("a legal frame")
    };

    let first = frame(&pool);
    assert_eq!(
        first.passes()[0].barriers().buffers[0].from,
        ResourceState::Undefined,
        "nothing has written this buffer yet"
    );
    harness.record(|encoder| {
        first
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });

    let second = frame(&pool);
    assert_eq!(
        (
            second.passes()[0].barriers().buffers[0].from,
            second.passes()[0].barriers().buffers[0].to
        ),
        (ResourceState::ShaderWrite, ResourceState::ShaderWrite),
        "the second frame's writes must be ordered after the first frame's"
    );
    harness.record(|encoder| {
        second
            .execute(harness.device.as_ref(), &mut pool, encoder, None)
            .expect("executed");
    });
    assert_eq!(pool.buffer_count(), 1, "one buffer, reused");
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
    let pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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

/// Two transients whose lifetimes do not overlap share one physical image, and
/// the hand-over between them is a **real** barrier rather than a re-acquire
/// from `Undefined`.
#[test]
fn non_overlapping_transients_alias_onto_one_physical_image() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
    assert!(
        compiled.images_alias(first, second),
        "identical descriptions with disjoint lifetimes must share one image"
    );
    assert_eq!(
        compiled.physical_image_count(),
        1,
        "two transients packed onto one physical image"
    );

    // `blur-b` takes the slot from `blur-a`, and its first barrier names what
    // `blur-a` left it in — **not** `Undefined`.
    //
    // `Undefined` would read as the cheaper answer: the pixels in the slot are
    // `blur-a`'s and `blur-b` does not want them, so discard. But aliasing here
    // means *one `VkImage`*, not two images over one allocation — there is no
    // layout to change and nothing to decompress, so the discard buys nothing —
    // and `Undefined` on this seam is not only "discard", it is
    // `srcStageMask = NONE, srcAccessMask = NONE`. It would throw away the only
    // thing this barrier is for: ordering `blur-b`'s colour writes after
    // `blur-a`'s. Two passes writing one image with no dependency between them
    // is a write-after-write hazard whichever names they were declared under.
    let reuse = compiled.passes()[1].barriers();
    assert_eq!(reuse.images.len(), 1);
    assert_eq!(
        reuse.images[0].from,
        ResourceState::ColorAttachment,
        "the slot's incoming barrier must carry a source scope covering the \
         writes the outgoing resource made to the same image"
    );
    assert_eq!(reuse.images[0].to, ResourceState::ColorAttachment);

    assert!(compiled.dump().contains("aliasing saved 1 physical image"));

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
    let pool = TransientPool::new();
    let mut graph = harness.graph();
    let first = graph.create_image("a", scene_color());
    let second = graph.create_image("b", scene_color());

    graph
        .add_render_pass("write-both")
        .clear_color(first, [0.0; 4])
        .clear_color(second, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");

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
    let pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
    let second = compiled.passes()[1].barriers();
    let barrier = second.images[0];
    assert_eq!(barrier.from, ResourceState::ColorAttachment);
    assert_eq!(barrier.to, ResourceState::TransferSrc);
    let ownership = barrier
        .queue_transfer
        .expect("crossing queue families is an ownership transfer, not a plain transition");
    assert_eq!(ownership.from, harness.queue);
    assert_eq!(ownership.to, transfer);

    // A transfer is **two** barriers with identical fields — a release on the
    // source queue and an acquire on the destination, per `QueueTransfer`'s own
    // docs. Emitting only the acquire leaves the source queue never releasing,
    // which is the half that makes the destination's read legal at all. The
    // release belongs after the last pass on the source queue.
    let release = compiled.passes()[0].release_barriers();
    assert_eq!(
        release.images.len(),
        1,
        "the source queue must release what the destination acquires: {release:?}"
    );
    assert_eq!(
        release.images[0], barrier,
        "both halves carry the same fields"
    );
    assert!(compiled.passes()[1].release_barriers().is_empty());

    assert!(compiled.dump().contains("queue "), "{}", compiled.dump());
    assert!(
        compiled.dump().contains("release"),
        "the dump must show both halves: {}",
        compiled.dump()
    );
}

/// Executing a graph must leave the backend holding exactly what it held before,
/// plus the pool's own resources — and destroying the pool must return it to
/// where it started.
#[test]
fn a_frame_leaks_nothing() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();
    let before = harness.recorder.total_live_objects();

    for frame in 0..8 {
        let mut graph = harness.graph();
        let color = graph.create_image("scene", scene_color());
        let depth = graph.create_image("depth", scene_depth());
        graph
            .add_render_pass("forward")
            .clear_color(color, [0.0; 4])
            .clear_depth(depth)
            .execute(|_| {});
        let compiled = graph.compile(&pool).expect("a legal frame");
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let pool = TransientPool::new();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("confused")
        .clear_color(color, [0.0; 4])
        .read_image(color)
        .execute(|_| {});

    let error = graph.compile(&pool).expect_err("one state per pass");
    assert!(
        matches!(error, GraphError::ConflictingAccess { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("scene"), "{error}");
}

#[test]
fn a_render_pass_with_no_attachments_is_refused() {
    let harness = Harness::open();
    let pool = TransientPool::new();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("nowhere")
        .read_image(color)
        .execute(|_| {});

    let error = graph
        .compile(&pool)
        .expect_err("a render pass renders somewhere");
    assert!(matches!(error, GraphError::NoAttachments { .. }), "{error}");
}

#[test]
fn attachments_of_different_sizes_are_refused() {
    let harness = Harness::open();
    let pool = TransientPool::new();
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

    let error = graph.compile(&pool).expect_err("attachments must agree");
    assert!(
        matches!(error, GraphError::AttachmentExtentMismatch { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("scene-depth"), "{error}");
}

#[test]
fn two_depth_attachments_are_refused() {
    let harness = Harness::open();
    let pool = TransientPool::new();
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

    let error = graph.compile(&pool).expect_err("one depth attachment");
    assert!(
        matches!(error, GraphError::DuplicateDepthAttachment { .. }),
        "{error}"
    );
}

#[test]
fn an_attachment_on_a_compute_pass_is_refused() {
    let harness = Harness::open();
    let pool = TransientPool::new();
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_compute_pass("confused")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});

    let error = graph
        .compile(&pool)
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
    let pool = TransientPool::new();
    let mut graph = harness.graph();
    let used = graph.create_image("used", scene_color());
    let _forgotten = graph.create_image("forgotten", scene_color());
    graph
        .add_render_pass("forward")
        .clear_color(used, [0.0; 4])
        .execute(|_| {});

    let error = graph
        .compile(&pool)
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
    let pool = TransientPool::new();
    let target = harness.target(Format::Bgra8UnormSrgb);
    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    let swap = graph.import_image("swapchain", target);
    let _ = swap;
    graph
        .add_render_pass("forward")
        .clear_color(color, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile(&pool).expect("an untouched import is legal");
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
    let pool = TransientPool::new();
    let mut target = harness.target(Format::Bgra8UnormSrgb);
    target.final_state = ResourceState::ColorAttachment;

    let mut graph = harness.graph();
    let swap = graph.import_image("swapchain", target);
    graph
        .add_render_pass("forward")
        .clear_color(swap, [0.0; 4])
        .execute(|_| {});

    let compiled = graph.compile(&pool).expect("a legal frame");
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
    let mut pool = TransientPool::new();
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

    let compiled = graph.compile(&pool).expect("a legal frame");
    let order: Vec<&str> = compiled.passes().iter().map(|pass| pass.label()).collect();
    assert_eq!(order, names);

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

// --- sub-resource range reaches barriers ------------------------------------

/// `read_subresource` and `use_subresource` propagate their declared range
/// into the barrier, so a pass that only touches mip 2 emits a barrier
/// scoped to just mip 2 — not the whole image.
#[test]
fn subresource_range_reaches_the_barrier() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();

    let mut graph = harness.graph();
    let image = graph.create_image(
        "mip-chain",
        TransientImageDesc {
            format: Format::Rgba16Float,
            usage: ImageUsage::STORAGE | ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            extent: EXTENT,
            samples: 1,
            // Two levels, because the passes below address both. The pool used
            // to create every transient with one level regardless, so this
            // graph barriered a level the image did not have.
            mip_levels: 2,
        },
    );

    // Pass 1: write mip 0 as `ShaderReadWrite` (storage-image write).
    graph
        .add_compute_pass("write-mip0")
        .use_subresource(
            image,
            ResourceState::ShaderReadWrite,
            ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: u32::MAX,
            },
        )
        .execute(|_| {});

    // Pass 2: write mip 1.
    graph
        .add_compute_pass("write-mip1")
        .use_subresource(
            image,
            ResourceState::ShaderReadWrite,
            ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 1,
                mip_count: 1,
                base_layer: 0,
                layer_count: u32::MAX,
            },
        )
        .execute(|_| {});

    // Pass 3: read whole image (default range = whole-image, `None`).
    graph
        .add_compute_pass("read-all")
        .read_image(image)
        .execute(|_| {});

    let compiled = graph.compile(&pool).expect("a legal frame");

    // Pass 1: one barrier, from `Undefined` on the declared range.
    let b0 = compiled.passes()[0].barriers();
    assert_eq!(b0.images.len(), 1);
    assert_eq!(b0.images[0].from, ResourceState::Undefined);
    assert_eq!(b0.images[0].to, ResourceState::ShaderReadWrite);
    let r0 = b0.images[0]
        .range
        .expect("pass 1 declares a subresource range");
    assert_eq!(r0.base_mip, 0);
    assert_eq!(r0.mip_count, 1);

    // Pass 2: mip 1 has never been touched, so its barrier leaves `Undefined`.
    // State is tracked per subresource: naming mip 0's `ShaderReadWrite` here —
    // which is what a whole-image tracker did — would be a barrier whose source
    // scope this subresource was never in, and a driver may treat one of those
    // as covering nothing.
    let b1 = compiled.passes()[1].barriers();
    assert_eq!(b1.images.len(), 1);
    assert_eq!(b1.images[0].from, ResourceState::Undefined);
    assert_eq!(b1.images[0].to, ResourceState::ShaderReadWrite);
    let r1 = b1.images[0]
        .range
        .expect("pass 2 declares a subresource range");
    assert_eq!(r1.base_mip, 1);
    assert_eq!(r1.mip_count, 1);

    // Pass 3: both levels are now in the same state, so the whole-image read
    // collapses to one barrier and `read_image` leaves range as `None` —
    // resolved to whole-image at execute time.
    let b2 = compiled.passes()[2].barriers();
    assert_eq!(b2.images.len(), 1);
    assert_eq!(b2.images[0].from, ResourceState::ShaderReadWrite);
    assert_eq!(b2.images[0].to, ResourceState::ShaderRead);
    assert!(
        b2.images[0].range.is_none(),
        "read_image uses whole-image range (None)"
    );

    pool.destroy(harness.device.as_ref());
}

/// The other half of per-subresource tracking: when the levels of one image are
/// in *different* states, a whole-image access cannot be one barrier, because a
/// barrier carries one `from`. It has to be one per state.
#[test]
fn a_whole_image_access_over_mixed_states_emits_a_barrier_per_state() {
    let harness = Harness::open();
    let mut pool = TransientPool::new();

    let mut graph = harness.graph();
    let image = graph.create_image(
        "mip-chain",
        TransientImageDesc {
            format: Format::Rgba16Float,
            usage: ImageUsage::STORAGE | ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            extent: EXTENT,
            samples: 1,
            mip_levels: 3,
        },
    );

    // Only mip 0 is written; mips 1 and 2 stay `Undefined`.
    graph
        .add_compute_pass("write-mip0")
        .use_subresource(
            image,
            ResourceState::ShaderReadWrite,
            ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: u32::MAX,
            },
        )
        .execute(|_| {});
    graph
        .add_compute_pass("read-all")
        .read_image(image)
        .execute(|_| {});

    let compiled = graph.compile(&pool).expect("a legal frame");
    let barriers = compiled.passes()[1].barriers();
    assert_eq!(
        barriers.images.len(),
        2,
        "one barrier per source state: {barriers:?}"
    );
    let written = barriers
        .images
        .iter()
        .find(|barrier| barrier.from == ResourceState::ShaderReadWrite)
        .expect("mip 0 was written");
    assert_eq!(written.range.expect("scoped").base_mip, 0);
    assert_eq!(written.range.expect("scoped").mip_count, 1);
    let untouched = barriers
        .images
        .iter()
        .find(|barrier| barrier.from == ResourceState::Undefined)
        .expect("mips 1 and 2 were never written");
    assert_eq!(untouched.range.expect("scoped").base_mip, 1);
    assert_eq!(untouched.range.expect("scoped").mip_count, 2);

    pool.destroy(harness.device.as_ref());
}

/// A range naming a level the image does not have is a declaration mistake, and
/// the graph says so rather than emitting a barrier that covers nothing.
#[test]
fn a_subresource_the_image_does_not_have_is_rejected() {
    let harness = Harness::open();
    let pool = TransientPool::new();

    let mut graph = harness.graph();
    let image = graph.create_image("single-level", scene_color());
    graph
        .add_compute_pass("write-mip3")
        .use_subresource(
            image,
            ResourceState::ShaderReadWrite,
            ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 3,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
        )
        .execute(|_| {});

    let error = graph
        .compile(&pool)
        .expect_err("mip 3 of a one-level image does not exist");
    let text = error.to_string();
    assert!(text.contains("single-level"), "{text}");
    assert!(text.contains("mip 3"), "{text}");
}

/// Declaring one resource twice in a pass is a mistake with a name: two colour
/// attachments over one view are two shader outputs writing the same pixels in
/// an order nothing defines.
#[test]
fn declaring_one_image_twice_in_a_pass_is_an_error() {
    let harness = Harness::open();
    let pool = TransientPool::new();

    let mut graph = harness.graph();
    let color = graph.create_image("scene", scene_color());
    graph
        .add_render_pass("double")
        .clear_color(color, [0.0; 4])
        .clear_color(color, [1.0; 4])
        .execute(|_| {});

    let error = graph.compile(&pool).expect_err("one view, two attachments");
    let text = error.to_string();
    assert!(text.contains("twice"), "{text}");
    assert!(text.contains("scene"), "{text}");
}

/// Every shader module the engine's own passes create is offered every
/// artifact format that module *has*.
///
/// `crcbl_hal::shader`'s contract is that a caller supplies every format it
/// holds and the backend picks. Nothing about a Vulkan run can check that: a
/// call site that quietly stopped passing its WGSL renders exactly the same on
/// `crcbl-vk`, and only `crcbl-wgpu` — which needs a GPU, or a browser — would
/// notice. That is precisely how the WGSL artifacts sat unused between P5.3 and
/// P5.9. This runs with no ICD, no driver and no GPU.
///
/// # DXIL is a list, so a two-stage module offers it like any other format
///
/// A DXIL container holds exactly one entry point — `dxc` compiles a single
/// `-E`, and a D3D12 pipeline takes one blob per stage — so
/// `ShaderModuleDesc::dxil` carries a container *per entry point* and a
/// graphics module offers both of its own. Every module here therefore offers
/// all four formats, and a `sources` short of that is a call site that dropped
/// one.
///
/// What this cannot see is whether those containers *cover the stages the
/// pipeline names*: the recorder keeps which formats were offered, not which
/// entry points. That half is checked where it can be — the null backend
/// refuses a stage whose entry point has no container, so each `new` below
/// returning at all is the assertion.
#[test]
fn the_engine_passes_offer_every_shader_artifact_they_have() {
    use crcbl_hal::ShaderSources;

    let harness = Harness::open();
    let renderer = crcbl_render::ForwardRenderer::new(
        harness.device.as_ref(),
        harness.queue,
        Format::Rgba8UnormSrgb,
    )
    .expect("the null backend accepts every descriptor");
    renderer.destroy(harness.device.as_ref());
    // The sprite and UI passes are built by a caller rather than by the forward
    // renderer, and they create the other two graphics modules — so they are
    // built here too, or half the engine's call sites would go unchecked.
    let sprites = crcbl_render::SpriteRenderer::new(
        harness.device.as_ref(),
        harness.queue,
        Format::Rgba8UnormSrgb,
    )
    .expect("the null backend accepts every descriptor");
    sprites.destroy(harness.device.as_ref());
    let ui = crcbl_render::UiRenderer::new(
        harness.device.as_ref(),
        harness.queue,
        Format::Rgba8UnormSrgb,
    )
    .expect("the null backend accepts every descriptor");
    ui.destroy(harness.device.as_ref());

    let created = harness.recorder.shader_modules_created();
    assert!(
        !created.is_empty(),
        "the engine's passes created no shader modules at all"
    );
    for (label, sources) in &created {
        let name = label.as_deref().unwrap_or("<unlabelled>");
        assert_eq!(
            *sources,
            ShaderSources::all(),
            "{name}: offered only {sources}"
        );
    }
    // Every shader the engine draws or dispatches with, by the label its
    // creator gives it, so a pass that stopped creating its module at all is a
    // failure rather than a shorter list nothing looks at.
    for expected in [
        "mesh.slang",
        "tonemap.slang",
        "sprite.slang",
        "ui.slang",
        "cull",
        "draw_gen",
    ] {
        assert!(
            created
                .iter()
                .any(|(label, _)| label.as_deref() == Some(expected)),
            "no module labelled `{expected}` was created; got {created:?}"
        );
    }
}
