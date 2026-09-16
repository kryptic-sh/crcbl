//! `docs/plan/57-grass.md` rung G1's price: the two generation dispatches and
//! the grass pass over the meadow, and the same frame with no field.
//!
//! `docs/plan/43-render-standards.md` prices a rung before it counts as built,
//! off `crcbl_render::PassStats`; this is that measurement for the grass
//! passes. `tests/render_e2e.rs`'s `grass` module holds the picture and the
//! instance data.
//!
//! **The generation dispatches are priced separately from the draw**, because
//! they answer different questions: what a field costs to *place* is a function
//! of its tiles and its cells, and what it costs to *draw* is a function of how
//! much of the screen the cards cover. A rung that reported one number for both
//! would say nothing about which half a denser field makes more expensive.

use crate::harness::Headless;
use crcbl::render::{PassStats, PassTimers, TransientPool};

/// The passes the price is read off. The forward pass is beside the grass's so
/// the figures have something in the same frame on the same device to be read
/// against — a millisecond on its own is a property of the machine.
const PRICED_PASSES: [&str; 4] = ["forward", "grass-clear", "grass-generate", "grass"];

/// One configuration's measurement.
struct Priced {
    /// Frames that reached [`PassStats`].
    recorded: u64,
    /// Each of [`PRICED_PASSES`]' p50 and p95 in nanoseconds, or [`None`] for a
    /// pass the frame never recorded — which is what a frame with no field is.
    passes: Vec<Option<(u64, u64)>>,
    /// The frame's p50 total over every timed pass, in nanoseconds.
    total: u64,
}

/// The meadow with its field and without it, drawn interleaved a frame each on
/// one device, or [`None`] where the device cannot time a pass.
///
/// `debug_draw.rs`'s `debug_draw_prices` is the shape, down to the warm-up and
/// the interleaving: a software rasteriser's "GPU" time is CPU time, so the two
/// configurations take their turns in the same contention.
fn grass_prices(extent: (u32, u32), frames: usize) -> Option<[Priced; 2]> {
    use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let mut priced = [true, false].map(|grass| {
        let scene =
            crcbl::screenshot::meadow_forward(device, headless.queue, headless.format, grass)
                .expect("the meadow builds");
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
                label: Some("priced grass frame"),
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

/// **The rung's price**: what the three grass passes cost over the meadow,
/// beside the forward pass of the same frames, and what the same frame costs
/// with no field.
///
/// Prints rather than asserts a duration, on `debug_draw.rs`'s terms. What it
/// asserts is the shape: the frame with a field times all three passes, and the
/// frame without one records none of them, while its forward pass is measured.
///
/// ```text
/// CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
///   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh grass
/// ```
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh grass"]
fn the_price_of_the_grass_passes() {
    let (extent, frames) = crate::area_light::price_frame();
    let Some([grown, bare]) = grass_prices(extent, frames) else {
        eprintln!(
            "{}: the meadow drew and this backend reports no TIMESTAMP_QUERY, so the grass \
             passes' price went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    let [grown_forward, grown_clear, grown_generate, grown_draw] = grown.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let [bare_forward, bare_clear, bare_generate, bare_draw] = bare.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let grown_forward = grown_forward.expect("the forward pass is in every frame");
    let grown_clear = grown_clear.expect("a frame with a field records `grass-clear`");
    let grown_generate = grown_generate.expect("a frame with a field records `grass-generate`");
    let grown_draw = grown_draw.expect("a frame with a field records `grass`");
    let bare_forward = bare_forward.expect("the forward pass is in every frame");
    eprintln!(
        "{}: meadow at {}x{} over {} recorded frames — grass-clear {:.3}/{:.3} ms, \
         grass-generate {:.3}/{:.3} ms, grass {:.3}/{:.3} ms, forward {:.3}/{:.3} ms (p50/p95); \
         frame p50 total {:.3} ms",
        crate::SUITE,
        extent.0,
        extent.1,
        grown.recorded,
        ms(grown_clear.0),
        ms(grown_clear.1),
        ms(grown_generate.0),
        ms(grown_generate.1),
        ms(grown_draw.0),
        ms(grown_draw.1),
        ms(grown_forward.0),
        ms(grown_forward.1),
        ms(grown.total),
    );
    eprintln!(
        "{}: the same meadow with no field at {}x{} over {} recorded frames — no grass pass, \
         forward {:.3}/{:.3} ms (p50/p95); frame p50 total {:.3} ms",
        crate::SUITE,
        extent.0,
        extent.1,
        bare.recorded,
        ms(bare_forward.0),
        ms(bare_forward.1),
        ms(bare.total),
    );

    assert!(
        grown_clear.0 > 0
            && grown_generate.0 > 0
            && grown_draw.0 > 0
            && grown_forward.0 > 0
            && bare_forward.0 > 0,
        "a pass that took no time at all was not measured"
    );
    assert!(
        bare.recorded > 0,
        "the bare frame reached no recorded frame"
    );
    assert!(
        bare_clear.is_none() && bare_generate.is_none() && bare_draw.is_none(),
        "a frame with no field timed a grass pass — grass-clear {bare_clear:?}, grass-generate \
         {bare_generate:?}, grass {bare_draw:?} — so it is not free when there is no grass"
    );
}
