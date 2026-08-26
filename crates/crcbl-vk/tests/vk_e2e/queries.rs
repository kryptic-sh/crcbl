//! Timestamp queries, and the per-pass GPU timers built on them.
//!
//! `docs/plan/02-vulkan-backend.md` §2.4 asks for a GPU timestamp per pass,
//! exposed as a frame-timing report. `crcbl-render`'s own tests cover the
//! report's *shape* with no device in the room; this module is the half that
//! needs a driver — that the numbers are non-zero, ordered, and attached to the
//! right pass names — which is the whole reason it is here rather than there.
//!
//! It renders `mesh`'s scene, importing that module's extent, camera and spin,
//! because a timer needs real work to measure and a scene of its own would be a
//! second thing to keep in step. The seam says timestamps degrade rather than
//! break where a device has none, so the first test asserts both arms and the
//! run says which one it took.

use crate::harness::Headless;
use crate::mesh::{MESH_EXTENT, mesh_camera, place_cube};
use crcbl_hal::{
    CommandEncoderDesc, ComputePassDesc, Features, PassTimestampWrites, PresentInfo, QueryKind,
    QuerySetDesc, SubmitInfo,
};

/// How many timers it takes to see every pass a forward frame records.
///
/// The renderer's own bound rather than a formula spelled out again here: this
/// file had the second copy of that arithmetic, and a second copy is one that
/// stops matching the day a pass is added — which is the drift
/// `ForwardRenderer::MAX_PASSES` exists to end.
const TIMED_PASSES: u32 = crcbl_render::ForwardRenderer::MAX_PASSES;

/// Timestamp queries, if the device has them: the profiler HUD's foundation,
/// and the seam says it degrades rather than breaks without them.
///
/// Both halves of the name run. The working half comes from whatever this
/// device reports; the refusal comes from a device opened without
/// `TIMESTAMP_QUERY`, because every adapter this suite can reach has it and
/// "or are refused cleanly" would otherwise describe a branch no run takes.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn timestamps_either_work_or_are_refused_cleanly() {
    let headless = Headless::open();
    let device = &headless.device;
    let has_timestamps = device.caps().features.contains(Features::TIMESTAMP_QUERY);

    let set = device.create_query_set(&QuerySetDesc {
        label: Some("frame timers"),
        kind: QueryKind::Timestamp,
        count: 2,
    });
    let Ok(set) = set else {
        assert!(
            !has_timestamps,
            "a device reporting TIMESTAMP_QUERY must create a timestamp set"
        );
        headless.finish();
        return;
    };
    assert!(
        has_timestamps,
        "a set was created on a device claiming none"
    );

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("timers"),
        queue: headless.queue,
    });
    encoder.reset_query_set(set, 0..2);
    // A pass, because the seam has no other place a timestamp can go: the two
    // queries are named by the descriptor and `crcbl-vk` writes them either
    // side of the scope. A compute pass opens no Vulkan object, so this is the
    // smallest thing that can carry a pair.
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("timed"),
        timestamp_writes: Some(PassTimestampWrites {
            set,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
    });
    encoder.end_compute_pass();
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");

    let mut results = [0u64; 2];
    device
        .query_results(set, 0, &mut results)
        .expect("timestamps read back");
    assert!(
        results[1] >= results[0],
        "the GPU clock does not run backwards: {results:?}"
    );

    // **The refusal arm, on a device manufactured to have it.** The `Err`
    // branch above is what a device without `TIMESTAMP_QUERY` takes, and no
    // adapter this suite can reach is one — radv and lavapipe both report the
    // feature, so "or are refused cleanly" was a claim about the source rather
    // than about any run. Opening a device without it is what reaches the
    // refusal.
    let lesser = Headless::open_pinning_format(
        "vk e2e timestamps unsupported",
        Features::DEBUG_MARKERS,
        crate::harness::EXTENT,
    );
    let lesser_device = lesser.device.as_ref();
    assert!(
        !lesser_device
            .caps()
            .features
            .contains(Features::TIMESTAMP_QUERY),
        "this device is opened without TIMESTAMP_QUERY; if it reports the \
         feature anyway the subtraction is not happening and the refusal below \
         is a timestamped device's answer wearing this one's name"
    );
    let refused = lesser_device.create_query_set(&QuerySetDesc {
        label: Some("frame timers on a device without them"),
        kind: QueryKind::Timestamp,
        count: 2,
    });
    let error = refused.expect_err("a device without TIMESTAMP_QUERY must refuse a timestamp set");
    assert!(
        matches!(error, crcbl_hal::HalError::Unsupported { .. }),
        "the refusal must be loud and typed, not an InvalidDescriptor: {error}"
    );
    lesser.finish();

    device.destroy_command_buffer(commands);
    device.destroy_query_set(set);
    headless.finish();
}

/// Per-pass GPU timers, against a real clock.
///
/// `docs/plan/02-vulkan-backend.md` §2.4 asks for "GPU timestamp per pass,
/// exposed as a frame-timing report". `crcbl-render`'s own tests cover the
/// report's shape; this is the half that needs a driver — that the numbers are
/// non-zero, ordered, and attached to the right pass names.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn per_pass_gpu_timers_report_real_numbers() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    if !device.caps().features.contains(Features::TIMESTAMP_QUERY) {
        eprintln!("vk e2e: no timestamp queries on this device; the report degrades to empty");
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let mut timers =
        // Room for every pass the forward frame records: the camera's cull
        // triple, one per shadow cascade, and the three render passes. A
        // capacity short of that is not an error — `PassTimers` warns and times
        // the ones that fit — so a literal here would have turned this
        // assertion into a check on a truncated prefix.
        crcbl_render::PassTimers::new(device, 2, TIMED_PASSES).expect("the device reports timestamps");
    let camera = mesh_camera(crcbl_render::Projection::default());

    // Enough frames for the timer ring to come round and resolve a slot.
    for _ in 0..6 {
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("an image");
        renderer
            .begin_frame(
                device,
                &camera,
                &crcbl_render::DirectionalLight::default(),
                MESH_EXTENT,
            )
            .expect("uniforms");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("timed frame"),
            queue: headless.queue,
        });
        let compiled = {
            let mut graph = crcbl_render::RenderGraph::new(headless.queue);
            let target = graph.import_image(
                "swapchain",
                crcbl_render::ForwardRenderer::present_target(
                    acquired.image,
                    acquired.view,
                    headless.format,
                    MESH_EXTENT,
                ),
            );
            let _ = renderer.add_passes(&mut graph, &pool, target, MESH_EXTENT);
            graph.compile(&pool).expect("a legal frame")
        };
        compiled
            .execute(device, &mut pool, encoder.as_mut(), Some(&mut timers))
            .expect("executed");
        let commands = encoder.finish().expect("recorded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        // The timers resolve a slot only when it comes back round, and this
        // suite submits without pipelining, so an idle here is what stands in
        // for the frame loop's timeline wait.
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
    }

    let timings = timers.latest();
    eprintln!("vk e2e: {}", timings.report());
    // The camera's cull triple, then one per shadow cascade, then the shadow
    // atlas's depth-only pass, `docs/plan/18-render-features.md`'s depth prepass
    // and occlusion pair, and the two colour passes that follow them. Built from
    // `crcbl_render::shadow::CASCADES` rather than written out, so a cascade
    // whose passes stopped being recorded is a failure here and not a shorter
    // HUD nobody counted.
    let mut expected: Vec<&str> = Vec::new();
    for cascade in 0..=crcbl_render::shadow::CASCADES {
        expected.extend(["clear-counters", "cull", "draw-args"]);
        if cascade == 0 {
            // The camera's alone: a cascade shades nothing, so one froxel grid
            // per camera is the whole of what the light list costs a frame.
            expected.push("light-cluster");
        }
    }
    // The culling-statistics copy is **not** here, and its absence is the
    // report's shape rather than an omission: the seam takes a timestamp only
    // where a pass opens and closes, and a `PassKind::Copy` opens no scope at
    // all. `PassTimers` gives it no query pair and no row, rather than a row
    // reading 0.000 ms that a reader would take for a measurement.
    expected.extend([
        "shadow",
        "depth-prepass",
        "ssao",
        "ssao-blur",
        "forward",
        "ssr",
        "ssr-blur",
        "tonemap",
        // The antialiasing resolve, which every frame draws — see
        // `RenderEffects::DEFAULT_STACK`.
        "fxaa",
    ]);
    assert_eq!(
        timings
            .passes
            .iter()
            .map(|pass| pass.label.as_str())
            .collect::<Vec<_>>(),
        expected,
        "the report must name the passes the graph ran, in order — the three \
         compute dispatches that generate the draws included, which is what the \
         per-pass HUD is for"
    );
    assert!(
        timings.total_nanos() > 0,
        "a real GPU took a measurable amount of time: {}",
        timings.report()
    );
    // **The bracket is a unit check, not a performance one**, and both ends are
    // an order of magnitude clear of anything a device could produce. What it
    // guards is `Device::query_results` reporting nanoseconds: this backend
    // multiplies a raw tick by `VkPhysicalDeviceLimits::timestampPeriod` in
    // `conv::timestamp_nanos`, and a read that skipped that step under-reports by
    // exactly the period — a factor of ten on the radv Navi 31 this was measured
    // on, and *invisible* on a device whose period is 1.0, which is why the
    // conversion also has unit tests that do not need a GPU.
    //
    // That card reported this frame at 0.112 ms run alone and as little as
    // 0.058 ms with the rest of the suite running beside it, and it is the
    // fastest consumer GPU this suite has been run on; lavapipe, the driver CI
    // uses, is orders of magnitude slower and clears the floor without trying.
    // The floor sits near the geometric mean of that smallest measurement and
    // what an unconverted read of it would have been, so it has about the same
    // room on both sides — a third either way. Most of the time is per-pass
    // overhead across the frame's passes rather than shading, so it does not
    // shrink with the next card either. A tighter bound would be a
    // load-dependent flake, and lavapipe's "GPU" time is CPU time on a machine
    // that may be running thirty other things.
    assert!(
        (20_000..10_000_000_000).contains(&timings.total_nanos()),
        "a 256x192 frame reporting {} ns is a unit mistake rather than a fast or \
         a slow machine — the seam's timestamps are nanoseconds: {}",
        timings.total_nanos(),
        timings.report()
    );

    timers.destroy(device);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
