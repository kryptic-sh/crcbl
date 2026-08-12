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
use crate::mesh::{MESH_EXTENT, MESH_SECONDS, mesh_camera};
use crcbl_hal::{CommandEncoderDesc, Features, PresentInfo, QueryKind, QuerySetDesc, SubmitInfo};

/// How many timers it takes to see every pass a forward frame records.
///
/// The renderer's own bound rather than a formula spelled out again here: this
/// file had the second copy of that arithmetic, and a second copy is one that
/// stops matching the day a pass is added — which is the drift
/// `ForwardRenderer::MAX_PASSES` exists to end.
const TIMED_PASSES: u32 = crcbl_render::ForwardRenderer::MAX_PASSES;

/// Timestamp queries, if the device has them: the profiler HUD's foundation,
/// and the seam says it degrades rather than breaks without them.
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
    encoder.write_timestamp(set, 0);
    encoder.write_timestamp(set, 1);
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
                crcbl_render::ForwardRenderer::spin(MESH_SECONDS),
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
            let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
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
    // The camera's cull triple, then one per shadow cascade, then the depth-only
    // pass they feed and the two that follow it. Built from
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
    expected.extend(["shadow", "forward", "tonemap"]);
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
    // A loose ceiling on purpose. The failure this guards against is a *unit*
    // mistake — raw ticks reported as nanoseconds, which on radv's 1.0 ns period
    // would be invisible and on another device would be out by orders of
    // magnitude — not slowness. A tight bound would instead be a load-dependent
    // flake, and lavapipe's "GPU" time is CPU time on a machine that may be
    // running thirty other things.
    assert!(
        timings.total_nanos() < 10_000_000_000,
        "a 256x192 frame reporting over ten seconds is a unit mistake, not a slow \
         machine: {}",
        timings.report()
    );

    timers.destroy(device);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
