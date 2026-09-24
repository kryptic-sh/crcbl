//! `docs/plan/57-grass.md` rungs G1, G2 and G3's price: the two generation
//! dispatches and the grass pass over the meadow, the same frame with no field,
//! the meadow's field drawn as shells at several stack heights, and the blade
//! meadow at each level of detail.
//!
//! Topic 43 prices a rung before it counts as built,
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

/// The meadow with each of `fields` on it, drawn interleaved a frame each on one
/// device, or [`None`] where the device cannot time a pass.
///
/// `debug_draw.rs`'s `debug_draw_prices` is the shape, down to the warm-up and
/// the interleaving: a software rasteriser's "GPU" time is CPU time, so the
/// configurations take their turns in the same contention.
fn grass_prices(
    extent: (u32, u32),
    frames: usize,
    fields: Vec<Option<crcbl::render::grass::GrassField>>,
) -> Option<Vec<Priced>> {
    use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let mut priced: Vec<_> = fields
        .iter()
        .map(|field| {
            let scene = crcbl::screenshot::meadow_forward_with(
                device,
                headless.queue,
                headless.format,
                field.as_ref(),
                crcbl::screenshot::MeadowWind::Windy,
            )
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
        })
        .collect();

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
        priced
            .iter()
            .map(|(_, _, _, stats, _)| {
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
            .collect()
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
    let fields = vec![Some(crcbl::screenshot::meadow_field()), None];
    let Some(prices) = grass_prices(extent, frames, fields) else {
        eprintln!(
            "{}: the meadow drew and this backend reports no TIMESTAMP_QUERY, so the grass \
             passes' price went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let [grown, bare] = <[Priced; 2]>::try_from(prices)
        .unwrap_or_else(|_| unreachable!("one price per configuration"));
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

/// The stack heights the shells are priced at: doubling, so a pass whose cost
/// is proportional to its layers doubles with each step.
const SHELL_COUNTS: [u32; 4] = [4, 8, 16, 32];

/// **Rung G3's price**: the grass pass over the meadow's field drawn as shells,
/// at each of [`SHELL_COUNTS`] with fins and at the default count without —
/// decision 3's "overdraw proportional to shell count", measured.
///
/// Prints every configuration's `grass` pass and a least-squares slope of its
/// p50 against the layer count, which is the cost of one layer over this frame.
/// What it asserts is the relation the plan predicts rather than a duration:
/// the deepest stack's pass costs more than the shallowest's.
///
/// ```text
/// CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
///   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh grass
/// ```
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh grass"]
fn the_price_of_the_shell_passes() {
    use crcbl::render::grass::Shells;

    let (extent, frames) = crate::area_light::price_frame();
    let stacked = |count, fins| {
        crcbl::screenshot::meadow_shells_field()
            .with_shells(Shells { count, fins })
            .expect("every priced stack is a stack")
    };
    // Priced in batches of at most two meadows, the size
    // `the_price_of_the_grass_passes` holds at once: five renderers on one
    // device ran WARP out of device memory on CI. The shallowest and deepest
    // stacks share a batch, so the relation asserted below is measured under
    // the same contention; the slope's other points come from their own.
    let batches = [
        vec![(SHELL_COUNTS[0], true), (SHELL_COUNTS[3], true)],
        vec![(SHELL_COUNTS[1], true), (SHELL_COUNTS[2], true)],
        vec![(crcbl::shaders::grass::DEFAULT_SHELLS, false)],
    ];
    let mut measured = Vec::new();
    for batch in batches {
        let fields = batch
            .iter()
            .map(|(count, fins)| Some(stacked(*count, *fins)))
            .collect();
        let Some(prices) = grass_prices(extent, frames, fields) else {
            eprintln!(
                "{}: the shell meadow drew and this backend reports no TIMESTAMP_QUERY, so \
                 the shells' price went unmeasured here",
                crate::SUITE,
            );
            return;
        };
        measured.extend(batch.into_iter().zip(prices));
    }
    let price_of = |count: u32, fins: bool| {
        measured
            .iter()
            .find(|(configuration, _)| *configuration == (count, fins))
            .map(|(_, price)| price)
            .expect("every configuration was priced")
    };
    let prices: Vec<&Priced> = SHELL_COUNTS
        .iter()
        .map(|count| price_of(*count, true))
        .chain([price_of(crcbl::shaders::grass::DEFAULT_SHELLS, false)])
        .collect();
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    // `PRICED_PASSES`' order: the draw is the fourth.
    let draw = |price: &Priced| price.passes[3].expect("a frame with a field records `grass`");
    let mut points = Vec::new();
    for (count, price) in SHELL_COUNTS.iter().zip(prices.iter().copied()) {
        let (p50, p95) = draw(price);
        let generate = price.passes[2].expect("a frame with a field records `grass-generate`");
        eprintln!(
            "{}: shell meadow at {}x{}, {count} shells with fins, over {} recorded frames — \
             grass-generate {:.3}/{:.3} ms, grass {:.3}/{:.3} ms (p50/p95); frame p50 total \
             {:.3} ms",
            crate::SUITE,
            extent.0,
            extent.1,
            price.recorded,
            ms(generate.0),
            ms(generate.1),
            ms(p50),
            ms(p95),
            ms(price.total),
        );
        points.push((f64::from(*count), ms(p50)));
    }
    let finless = draw(prices[SHELL_COUNTS.len()]);
    eprintln!(
        "{}: shell meadow at {}x{}, {} shells without fins — grass {:.3}/{:.3} ms (p50/p95)",
        crate::SUITE,
        extent.0,
        extent.1,
        crcbl::shaders::grass::DEFAULT_SHELLS,
        ms(finless.0),
        ms(finless.1),
    );
    // Ordinary least squares over the four points: the millisecond one layer
    // adds, and what the pass costs before its first.
    let n = points.len() as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let slope = points
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum::<f64>()
        / points
            .iter()
            .map(|(x, _)| (x - mean_x).powi(2))
            .sum::<f64>();
    eprintln!(
        "{}: shell meadow at {}x{} — {slope:.4} ms per shell, {:.3} ms at none",
        crate::SUITE,
        extent.0,
        extent.1,
        mean_y - slope * mean_x,
    );
    let (shallow, deep) = (draw(prices[0]).0, draw(prices[SHELL_COUNTS.len() - 1]).0);
    assert!(
        deep > shallow,
        "the grass pass costs {} ms over {} shells and {} ms over {}: the stack's depth is not \
         what it is paying for",
        ms(deep),
        SHELL_COUNTS[SHELL_COUNTS.len() - 1],
        ms(shallow),
        SHELL_COUNTS[0],
    );
}

/// **Rung G2's price**: the grass pass over the blade meadow's field with every
/// blade at one level of detail, then the other, then at the meadow's own
/// switch — decision 3's "15 or 7 vertices per blade", measured.
///
/// The near level is the field with its switch past the field's reach, so every
/// blade is drawn with fifteen vertices; the far level is the switch a
/// centimetre from the eye with no band, so the one blade in four the far level
/// keeps is drawn with seven. Priced two meadows at a time, on
/// `the_price_of_the_shell_passes`' terms, with the two levels in one batch so
/// the relation asserted below is measured under one contention.
///
/// Prints every configuration's generation and draw; asserts the relation the
/// plan predicts rather than a duration: the near level's draw costs more than
/// the far level's.
///
/// ```text
/// CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
///   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh grass
/// ```
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh grass"]
fn the_price_of_the_blade_passes() {
    use crcbl::render::grass::BladeLod;

    let (extent, frames) = crate::area_light::price_frame();
    let at = |distance, band| {
        crcbl::screenshot::meadow_blades_field()
            .with_blade_lod(BladeLod { distance, band })
            .expect("every priced switch is a switch")
    };
    let default = crcbl::screenshot::MEADOW_BLADE_LOD;
    let batches = [
        vec![
            ("near", Some(at(crcbl::screenshot::MEADOW_REACH, 0.0))),
            ("far", Some(at(0.01, 0.0))),
        ],
        vec![("meadow", Some(at(default.distance, default.band)))],
    ];
    let mut measured = Vec::new();
    for batch in batches {
        let (names, fields): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
        let Some(prices) = grass_prices(extent, frames, fields) else {
            eprintln!(
                "{}: the blade meadow drew and this backend reports no TIMESTAMP_QUERY, so the \
                 blades' price went unmeasured here",
                crate::SUITE,
            );
            return;
        };
        measured.extend(names.into_iter().zip(prices));
    }
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    let draw = |price: &Priced| price.passes[3].expect("a frame with a field records `grass`");
    for (name, price) in &measured {
        let generate = price.passes[2].expect("a frame with a field records `grass-generate`");
        let (p50, p95) = draw(price);
        eprintln!(
            "{}: blade meadow at {}x{}, {name} level, over {} recorded frames — grass-generate \
             {:.3}/{:.3} ms, grass {:.3}/{:.3} ms (p50/p95); frame p50 total {:.3} ms",
            crate::SUITE,
            extent.0,
            extent.1,
            price.recorded,
            ms(generate.0),
            ms(generate.1),
            ms(p50),
            ms(p95),
            ms(price.total),
        );
    }
    let price_of = |wanted: &str| {
        measured
            .iter()
            .find(|(name, _)| *name == wanted)
            .map(|(_, price)| draw(price).0)
            .expect("every configuration was priced")
    };
    let (near, far) = (price_of("near"), price_of("far"));
    assert!(
        near > far,
        "the grass pass costs {} ms with every blade near and {} ms with every blade far: the \
         near level is not what it pays for",
        ms(near),
        ms(far),
    );
}
