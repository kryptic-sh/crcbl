//! `docs/plan/55-water.md` rung 1's price: the two water passes over the still
//! pool, and the same frame with no body.
//!
//! Topic 43 prices a rung before it counts as built,
//! off `crcbl_render::PassStats`; this is that measurement for the surface
//! passes. `tests/render_e2e.rs`'s `still_pool` module holds the picture.

use crate::harness::Headless;
use crcbl::render::{PassStats, PassTimers, TransientPool};

/// The passes the price is read off. The forward pass is beside the water's so
/// the figures have something in the same frame on the same device to be read
/// against — a millisecond on its own is a property of the machine.
const PRICED_PASSES: [&str; 3] = ["forward", "water-copy", "water"];

/// One configuration's measurement.
struct Priced {
    /// Frames that reached [`PassStats`].
    recorded: u64,
    /// Each of [`PRICED_PASSES`]' p50 and p95 in nanoseconds, or [`None`] for a
    /// pass the frame never recorded — which is what a frame with no body is.
    passes: Vec<Option<(u64, u64)>>,
    /// The frame's p50 total over every timed pass, in nanoseconds.
    total: u64,
}

/// The still pool with its body and without it, drawn interleaved a frame each
/// on one device, or [`None`] where the device cannot time a pass.
///
/// `debug_draw.rs`'s `debug_draw_prices` is the shape, down to the warm-up and
/// the interleaving: a software rasteriser's "GPU" time is CPU time, so the two
/// configurations take their turns in the same contention.
fn water_prices(extent: (u32, u32), frames: usize) -> Option<[Priced; 2]> {
    use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let bodies = [vec![crcbl::screenshot::still_pool_body()], Vec::new()];
    let mut priced = bodies.map(|bodies| {
        let scene =
            crcbl::screenshot::still_pool_forward(device, headless.queue, headless.format, &bodies)
                .expect("the still pool builds");
        let timers = timed.then(|| {
            PassTimers::new(
                device,
                crcbl::render::forward::FRAMES_IN_FLIGHT,
                crcbl::render::MAX_TIMED_PASSES,
            )
            .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
        });
        (
            scene,
            TransientPool::new(),
            timers,
            PassStats::new(),
            Vec::new(),
        )
    });

    for index in 0..crate::area_light::PRICE_WARMUP + frames {
        for (scene, pool, timers, stats, recorded) in &mut priced {
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("the ring always has an image");
            scene
                .renderer
                .begin_frame(device, &scene.camera, &scene.sun, extent)
                .expect("the frame's blocks are writable");
            let compiled = {
                let mut graph = crcbl::render::RenderGraph::new(headless.queue);
                let target = graph.import_image(
                    "swapchain",
                    crcbl::render::ImportedImage {
                        image: acquired.image,
                        view: acquired.view,
                        format: headless.format,
                        extent,
                        initial: ResourceState::Undefined,
                        claim: crcbl::render::InitialClaim::Acquired,
                        final_state: ResourceState::Present,
                    },
                );
                scene
                    .renderer
                    .add_passes(&mut graph, &*pool, target, extent);
                graph.compile(&*pool).expect("a legal frame")
            };
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("priced water frame"),
                queue: headless.queue,
            });
            compiled
                .execute(device, pool, encoder.as_mut(), timers.as_mut())
                .expect("the graph executed");
            let commands = encoder.finish().expect("recording succeeded");
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
            if let (true, Some(timers)) =
                (index >= crate::area_light::PRICE_WARMUP, timers.as_ref())
            {
                stats.record(timers.latest());
            }
            recorded.push(commands);
        }
    }

    device.wait_idle().expect("idle");
    let prices = timed.then(|| {
        std::array::from_fn(|index| {
            let (_, _, _, stats, _) = &priced[index];
            eprintln!("{}: {}", crate::SUITE, stats.report());
            Priced {
                recorded: stats.frames(),
                passes: PRICED_PASSES
                    .iter()
                    .map(|pass| stats.percentiles(pass))
                    .collect(),
                total: stats.p50_total_nanos(),
            }
        })
    });
    for (scene, mut pool, timers, _, recorded) in priced {
        if let Some(mut timers) = timers {
            timers.destroy(device);
        }
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        scene.renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
    prices
}

/// **The rung's price**: what the two water passes cost over the still pool,
/// beside the forward pass of the same frames, and what the same frame costs
/// with no body.
///
/// Prints rather than asserts a duration, on `debug_draw.rs`'s terms. What it
/// asserts is the shape: the frame with a body times both passes, and the frame
/// without one records neither, while its forward pass is measured.
///
/// ```text
/// CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
///   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh water
/// ```
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh water"]
fn the_price_of_the_water_passes() {
    let (extent, frames) = crate::area_light::price_frame();
    let Some([wet, dry]) = water_prices(extent, frames) else {
        eprintln!(
            "{}: the still pool drew and this backend reports no TIMESTAMP_QUERY, so the water \
             passes' price went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    let [wet_forward, wet_copy, wet_surface] = wet.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let [dry_forward, dry_copy, dry_surface] = dry.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let wet_forward = wet_forward.expect("the forward pass is in every frame");
    let wet_copy = wet_copy.expect("a frame with a body records and times `water-copy`");
    let wet_surface = wet_surface.expect("a frame with a body records and times `water`");
    let dry_forward = dry_forward.expect("the forward pass is in every frame");
    eprintln!(
        "{}: still pool at {}x{} over {} recorded frames — water-copy {:.3}/{:.3} ms, water \
         {:.3}/{:.3} ms, forward {:.3}/{:.3} ms (p50/p95); frame p50 total {:.3} ms",
        crate::SUITE,
        extent.0,
        extent.1,
        wet.recorded,
        ms(wet_copy.0),
        ms(wet_copy.1),
        ms(wet_surface.0),
        ms(wet_surface.1),
        ms(wet_forward.0),
        ms(wet_forward.1),
        ms(wet.total),
    );
    eprintln!(
        "{}: the same pool with no body at {}x{} over {} recorded frames — no water pass, \
         forward {:.3}/{:.3} ms (p50/p95); frame p50 total {:.3} ms",
        crate::SUITE,
        extent.0,
        extent.1,
        dry.recorded,
        ms(dry_forward.0),
        ms(dry_forward.1),
        ms(dry.total),
    );

    assert!(
        wet_copy.0 > 0 && wet_surface.0 > 0 && wet_forward.0 > 0 && dry_forward.0 > 0,
        "a pass that took no time at all was not measured"
    );
    assert!(dry.recorded > 0, "the dry frame reached no recorded frame");
    assert!(
        dry_copy.is_none() && dry_surface.is_none(),
        "a frame with no body timed a water pass — water-copy {dry_copy:?}, water \
         {dry_surface:?} — so it is not free when there is no water"
    );
}
