use crate::harness::Headless;
use crate::mesh::{MESH_EXTENT, MESH_SECONDS, mesh_camera};
use crcbl_hal::{CommandEncoderDesc, Features, PresentInfo, QueryKind, QuerySetDesc, SubmitInfo};

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
        crcbl_render::PassTimers::new(device, 2, 8).expect("the device reports timestamps");
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
    assert_eq!(
        timings
            .passes
            .iter()
            .map(|pass| pass.label.as_str())
            .collect::<Vec<_>>(),
        vec!["forward", "tonemap"],
        "the report must name the passes the graph ran, in order"
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
