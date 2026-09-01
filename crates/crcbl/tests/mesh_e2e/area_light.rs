//! `docs/plan/44-lighting.md`'s rung 5, drawn: a rectangular area light over a
//! smooth slab, its golden, and the two claims only a rectangle can make.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh area_light
//! ```
//!
//! # Why the frames here rather than in `render_e2e.rs`
//!
//! The three tests below are the whole of the picture evidence for the linearly
//! transformed cosine path, and each is a comparison between two frames this
//! file renders rather than a single frame. `tests/render_e2e.rs` draws
//! `crcbl::screenshot::Scene`s and compares one frame per test, and a scene
//! there also reaches the browser harness — which is where this rung's frame
//! belongs and does not yet go. `docs/backlog.md` carries that gap and what it
//! costs.
//!
//! # The scene
//!
//! A wide flat slab under [`SLAB_MATERIAL`]'s tighter lobe, and one strip light
//! above it. Nothing else: an area light's whole difference from a point light
//! is the *shape* of its highlight, and a scene with a second object in it would
//! put that shape behind something.

use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    DirectionalLight, ForwardRenderer, Light, PointLight, Projection, RectLight, TransientPool,
};

use crate::goldens::mesh_golden;
use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place, render_mesh_lit};

/// The demo scene's tinted row, whose roughness is a quarter where the neutral
/// row's is a half.
///
/// `crcbl_render::scene::PYRAMID_ROUGHNESS` is the number and its doc is the
/// reason: a tighter lobe draws a highlight with an edge, and an area light's
/// claim is entirely about the edge's shape. The neutral row's broad lobe would
/// smear a strip and a point into the same soft blob.
const SLAB_MATERIAL: usize = crcbl::render::scene::DEMO_TINTED;

/// How the demo cube is squashed into a slab: wide in `x` and `z`, thin in `y`.
///
/// Wide enough that the strip's reflection lands well inside it — a highlight
/// running off the edge of its own surface is a frame that would pass these
/// tests while showing nothing.
const SLAB_SCALE: Vec3 = Vec3::new(3.0, 0.06, 3.0);

/// How far above the slab the strip hangs.
const STRIP_HEIGHT: f32 = 1.1;

/// Half the strip's length, along whichever axis [`strip`] is given.
const STRIP_LONG: f32 = 0.85;

/// Half its width, across that axis.
///
/// An order of magnitude under [`STRIP_LONG`], because the claim
/// `turning_the_rectangle_about_its_normal_moves_its_highlight` makes is that
/// the two are told apart — a near-square strip would be its own rotation.
const STRIP_SHORT: f32 = 0.07;

/// The radiance leaving the strip's face.
///
/// Well above one: the scene target is `Rgba16Float` and the tonemap is what
/// brings it down, and a highlight that never reaches the top of the display
/// range is one whose shape the eight-bit golden cannot carry.
const STRIP_COLOR: Vec3 = Vec3::new(22.0, 19.8, 16.9);

/// How far the strip's influence reaches from its own centre.
///
/// Comfortably past the slab's far corner, so the quartic window
/// `crcbl_shaders::light`'s row documents is not what shapes this picture — the
/// polygon integral is.
const STRIP_REACH: f32 = 12.0;

/// The sun, turned right down so the strip is what lights the slab.
///
/// Not off: a frame with a single light in it cannot show that the light list
/// still carries the sun, and every other golden in this suite is drawn under
/// [`DirectionalLight::default`]. The ambient is kept so the slab's unlit half
/// is a surface rather than a hole.
fn dim_sun() -> DirectionalLight {
    let full = DirectionalLight::default();
    DirectionalLight {
        color: full.color * 0.06,
        ..full
    }
}

/// The strip light, lying along `along` and facing straight down.
///
/// `along` is the rectangle's `u` axis and is what
/// `turning_the_rectangle_about_its_normal_moves_its_highlight` varies; the
/// half-extents stay with the axes rather than with the light, so turning it is
/// a turn and not a resize.
fn strip(along: Vec3, fill: bool) -> Light {
    Light::Rect(RectLight {
        position: Vec3::new(0.0, STRIP_HEIGHT, 0.0),
        radius: STRIP_REACH,
        color: STRIP_COLOR,
        direction: Vec3::NEG_Y,
        tangent: along,
        half_width: STRIP_LONG,
        half_height: STRIP_SHORT,
        fill,
    })
}

/// A renderer and a pool on `headless`, with the slab in the frame and `lights`
/// set.
fn slab_scene(headless: &Headless, lights: &[Light]) -> (ForwardRenderer, TransientPool) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        SLAB_MATERIAL,
        Mat4::from_scale(SLAB_SCALE),
    );
    renderer.set_lights(lights);
    (renderer, TransientPool::new())
}

/// Releases everything in dependency order, then asks the device what it saw.
fn teardown(headless: Headless, renderer: ForwardRenderer, mut pool: TransientPool) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// One frame of the slab under `lights`: the tonemapped image and the linear
/// scene target behind it.
///
/// **Both, because the two questions below want different ones.** A claim about
/// what the *shading* did belongs on the linear target, where nothing has been
/// through a tonemap curve or an exposure yet; a claim about what the frame
/// looks like belongs on the image.
fn slab_frame(headless: &Headless, lights: &[Light]) -> (crcbl_golden::Image, HdrTarget) {
    let (mut renderer, mut pool) = slab_scene(headless, lights);
    let mut hdr = Vec::new();
    let image = render_mesh_lit(
        headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        &dim_sun(),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    (image, HdrTarget(hdr))
}

/// The sum of a pixel's three colour channels, which is what "brighter" means in
/// every comparison below.
fn luma(image: &crcbl_golden::Image, x: u32, y: u32) -> u32 {
    let pixel = image.pixel(x, y).expect("inside the frame");
    u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
}

/// How many pixels of `left` differ from `right` at all.
fn differing(left: &crcbl_golden::Image, right: &crcbl_golden::Image) -> u32 {
    let (width, height) = MESH_EXTENT;
    let mut count = 0;
    for y in 0..height {
        for x in 0..width {
            if left.pixel(x, y) != right.pixel(x, y) {
                count += 1;
            }
        }
    }
    count
}

/// The brightest channel sum anywhere in `image`.
fn brightest(image: &crcbl_golden::Image) -> u32 {
    let (width, height) = MESH_EXTENT;
    let mut peak = 0;
    for y in 0..height {
        for x in 0..width {
            peak = peak.max(luma(image, x, y));
        }
    }
    peak
}

/// How bright the strip's reflection has to get for the eight-bit golden to
/// carry its shape.
///
/// Two thirds of what a saturated pixel sums to, which is a floor rather than a
/// prediction: the frame the golden was blessed from clears it comfortably on
/// radv and on lavapipe alike, and each test prints its own peak beside this so
/// a reader can see the margin. The first [`STRIP_COLOR`] this file tried did
/// not clear it, and drew a highlight whose edge the display encoding had
/// already flattened.
const HIGHLIGHT_FLOOR: u32 = 510;

/// The strip light draws a strip-shaped highlight, against a checked-in
/// reference.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_area_light_over_a_slab_matches_its_golden() {
    let headless = Headless::open_for_mesh();
    let (mut renderer, mut pool) = slab_scene(&headless, &[strip(Vec3::X, false)]);
    let image = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        &dim_sun(),
        None,
    );

    // Something drew, and it is not the whole frame: the clear must still be
    // visible in the corners, or the slab is a full-screen quad.
    let corner = image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    // And the light is actually a light: a highlight the display encoding has
    // already flattened is a golden that cannot tell one lobe from another.
    let peak = brightest(&image);
    eprintln!(
        "{}: the strip's brightest pixel sums to {peak}",
        crate::SUITE
    );
    assert!(
        peak > HIGHLIGHT_FLOOR,
        "the brightest pixel sums to {peak}, under the {HIGHLIGHT_FLOOR} this frame needs for \
         its highlight to have an edge in eight bits"
    );

    let verdict = mesh_golden("mesh_area_light", &image);
    teardown(headless, renderer, pool);
    eprintln!(
        "{}: {}",
        crate::SUITE,
        verdict.unwrap_or_else(|m| panic!("{m}"))
    );
}

/// **A rectangle is a rectangle and not a disc**: turning the strip about its
/// own normal turns its reflection with it.
///
/// The claim the golden cannot make on its own. A shading path that read the
/// light's position and its radius and nothing else — a point light wearing a
/// rectangle's row — draws the identical frame at both orientations, so this is
/// what says the polygon integral is reading the corners.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn turning_the_rectangle_about_its_normal_moves_its_highlight() {
    let headless = Headless::open_for_mesh();
    let (along_x, _) = slab_frame(&headless, &[strip(Vec3::X, false)]);
    let (along_z, _) = slab_frame(&headless, &[strip(Vec3::Z, false)]);
    headless.finish();

    let moved = differing(&along_x, &along_z);
    let (width, height) = MESH_EXTENT;
    let pixels = width * height;
    eprintln!(
        "{}: turning the strip a quarter turn moved {moved} of {pixels} pixels",
        crate::SUITE
    );
    // A twentieth of the frame. The slab covers most of it and the highlight
    // covers a band across the slab, so a working turn moves far more than
    // this — it is a floor against a frame that differs in a few dither steps,
    // not an estimate of the highlight's area.
    assert!(
        moved * 20 > pixels,
        "only {moved} of {pixels} pixels changed when the rectangle turned a quarter turn, so \
         the shading is not reading its corners"
    );
}

/// **The fill flag removes the highlight and leaves the light.**
///
/// Measured on the linear scene target rather than on the tonemapped image, and
/// that is the whole design of this test: the two frames carry different total
/// light, so the tonemap maps them differently and a texel with strictly less
/// radiance can still come out one display level higher. The claim is about what
/// the shading did, so it is made where the shading wrote.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_fill_light_keeps_its_diffuse_and_loses_its_highlight() {
    let headless = Headless::open_for_mesh();
    let (_, lit) = slab_frame(&headless, &[strip(Vec3::X, false)]);
    let (_, fill) = slab_frame(&headless, &[strip(Vec3::X, true)]);
    headless.finish();

    let (width, height) = MESH_EXTENT;
    let mut dimmed = 0;
    let mut brightest_gain = 0.0f32;
    for y in 0..height {
        for x in 0..width {
            let before = lit.pixel(x, y);
            let after = fill.pixel(x, y);
            // Alpha is a constant one on both, and would count every texel.
            let lost = (0..3).any(|channel| after[channel] < before[channel]);
            if lost {
                dimmed += 1;
            }
            for channel in 0..3 {
                brightest_gain = brightest_gain.max(after[channel] - before[channel]);
            }
        }
    }
    eprintln!(
        "{}: {dimmed} of {} texels lost radiance to the fill flag, the largest gain anywhere is \
         {brightest_gain}",
        crate::SUITE,
        width * height
    );

    // A fiftieth of the frame: the highlight is a band across a slab that does
    // not fill the frame, and every texel outside the slab is the clear colour
    // on both frames.
    assert!(
        dimmed * 50 > width * height,
        "only {dimmed} of {} texels lost radiance when the light became a fill light, so the \
         flag is not reaching the specular term",
        width * height
    );
    // And nothing anywhere gained any, which is what "the diffuse is untouched"
    // means when the only other term is one that was removed. Exactly zero
    // rather than a tolerance: the diffuse term is the same arithmetic over the
    // same inputs on both frames, so it is the same bits.
    assert_eq!(
        brightest_gain, 0.0,
        "a texel gained {brightest_gain} of radiance when a light lost a term, which is not \
         something removing a lobe can do"
    );
}

/// The frame the price below is measured at, and how many frames it is measured
/// over.
///
/// **The defaults are the guard and not the measurement.** This test runs in
/// every mesh-e2e run, including CI's on lavapipe, where a 1080p frame costs
/// tens of milliseconds and four hundred of them three times over would be
/// minutes; at the suite's own extent and a window just past
/// `crcbl_core::stats::MIN_PERCENTILE_SAMPLES` it costs a fraction of a second
/// and still holds the ordering below. The numbers written into
/// `docs/plan/44-lighting.md` came from the override:
///
/// ```text
/// CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
///   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh price
/// ```
///
/// Both are read from the environment rather than compiled in because the price
/// is a property of the machine, and a machine is not something a constant
/// knows about. The count is of **recorded** frames; [`PRICE_WARMUP`] more are
/// drawn first and thrown away.
pub(crate) fn price_frame() -> ((u32, u32), usize) {
    let extent = match std::env::var("CRCBL_PRICE_SIZE") {
        Ok(size) => {
            let (width, height) = size
                .split_once('x')
                .unwrap_or_else(|| panic!("CRCBL_PRICE_SIZE is WIDTHxHEIGHT, got {size:?}"));
            (
                width.parse().expect("a width"),
                height.parse().expect("a height"),
            )
        }
        Err(_) => MESH_EXTENT,
    };
    // Comfortably past `crcbl_core::stats::MIN_PERCENTILE_SAMPLES`, below which
    // `PassStats::percentiles` returns `None`. Comfortably rather than barely,
    // because the p50 this compares has to survive a burst of contention from
    // the sixty-two tests running beside it.
    let floor = 48;
    let frames = match std::env::var("CRCBL_PRICE_FRAMES") {
        Ok(count) => count.parse().expect("a frame count"),
        Err(_) => floor,
    };
    assert!(
        frames >= floor,
        "{frames} recorded frames is under the {floor} the percentile floor needs, so the \
         measurement would have no percentiles at all"
    );
    (extent, frames)
}

/// How many frames are drawn and thrown away before anything is recorded.
///
/// **The measurement is worthless without this, and it fails loudly rather than
/// quietly**: a software rasteriser compiles its pipelines on first use, so the
/// opening frames of a fixture cost several times what the steady state does. A
/// short run that recorded them read the sun alone as dearer than sixteen area
/// lights on lavapipe — a p50 of 6.44 ms against 3.10 ms — which is what the
/// ordering assertions below then reported. Long enough to cover the ring's
/// latency and the first draws with every pipeline the frame uses.
pub(crate) const PRICE_WARMUP: usize = crcbl::render::forward::FRAMES_IN_FLIGHT + 2 + 16;

/// How many lights of each kind the price is measured with.
///
/// `crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY` is the number and the
/// reason: every light here reaches every froxel over the slab, so a full list
/// is the worst case the grid allows and one more light would be dropped rather
/// than shaded. One light would be a measurement of one light against the rest
/// of the frame rather than of a per-light cost.
const PRICE_LIGHTS: usize = crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY as usize;

/// Each light set's forward-pass p50 and p95 in nanoseconds, in nanoseconds and
/// in the order given.
///
/// **The sets are rendered interleaved on one device, a frame each per turn, and
/// that is the whole reason this takes a slice rather than being called three
/// times.** The suite runs sixty-three tests at once and a software rasteriser's
/// "GPU" time is CPU time, so a run measured set after set reads whatever else
/// was on the machine during that set's turn: measured that way on lavapipe the
/// sun alone came out at a 2.547 ms p50 with an 8.590 ms p95, dearer than the
/// sixteen area lights that followed it. Interleaved, a burst of contention
/// lands on every set alike and the comparison survives it.
///
/// [`PRICE_WARMUP`] turns are drawn before the first is recorded.
///
/// Reads the GPU timestamps the render graph already takes around every pass —
/// the same numbers the debug overlay shows and the same accumulator
/// `apps/lantern`'s headless report uses — so this measures what a frame costs
/// rather than what a benchmark harness costs.
fn forward_pass_prices(
    sets: &[&[Light]],
    extent: (u32, u32),
    frames: usize,
) -> Option<Vec<(u64, u64)>> {
    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    // **The features above are asked for, not required** — `Headless::open_at`
    // opens the best device it can — and a backend that cannot time a pass
    // cannot price one. CI's Apple Paravirtual device is the case: it reports
    // no `TIMESTAMP_QUERY`, so this answers `None` and the caller says the
    // price went unmeasured rather than asserting an ordering between three
    // zeroes. The frames are still drawn either way, which is the half of this
    // helper every backend can run.
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let camera = mesh_camera(Projection::default());
    let sun = dim_sun();
    let mut priced: Vec<_> = sets
        .iter()
        .map(|lights| {
            let (renderer, pool) = slab_scene(&headless, lights);
            let timers = timed.then(|| {
                crcbl::render::PassTimers::new(
                    device,
                    crcbl::render::forward::FRAMES_IN_FLIGHT,
                    crcbl::render::MAX_TIMED_PASSES,
                )
                .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
            });
            (
                renderer,
                pool,
                timers,
                crcbl::render::PassStats::new(),
                Vec::new(),
            )
        })
        .collect();

    for index in 0..PRICE_WARMUP + frames {
        for (renderer, pool, timers, stats, recorded) in &mut priced {
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("the ring always has an image");
            renderer
                .begin_frame(device, &camera, &sun, extent)
                .expect("the uniform buffer is writable");
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
                renderer.add_passes(&mut graph, &*pool, target, extent);
                graph.compile(&*pool).expect("a legal frame")
            };
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("priced frame"),
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
            if let (true, Some(timers)) = (index >= PRICE_WARMUP, timers.as_ref()) {
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
                stats
                    .percentiles("forward")
                    .expect("the forward pass is timed and the window is past its floor")
            })
            .collect()
    });
    for (renderer, mut pool, timers, _, recorded) in priced {
        if let Some(mut timers) = timers {
            timers.destroy(device);
        }
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
    prices
}

/// **The rung's price**, on `docs/plan/43-render-standards.md`'s terms: what the
/// forward pass costs with a froxel full of area lights against the same froxel
/// full of point lights, and against the sun alone.
///
/// Prints rather than asserts a millisecond figure, deliberately: a duration is
/// a property of the machine it was measured on and an assertion on one is red
/// on every other machine. What it asserts is the *shape* the pricing rule cares
/// about — that a rectangle costs more than a point light and by a bounded
/// factor — so a polygon integral that grew an order of magnitude fails here
/// rather than being noticed in a frame.
///
/// # A backend that cannot time a pass cannot price one
///
/// The ordering below needs GPU timestamps, and CI's Apple Paravirtual device
/// reports none. So this draws its three light sets on every backend — which
/// is a real claim, and the one that caught the missing `Rgba16Float` blend on
/// a backend that had it and the polygon integral on one that did not — and
/// prices them only where the device says it can measure. **It says which**,
/// rather than passing quietly: a price that went unmeasured is reported as
/// unmeasured, which is the difference `mesh_e2e/main.rs`'s header draws
/// between splitting a test and gating one. `docs/backlog.md` carries which
/// backends have actually priced this rung.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh price"]
fn the_price_of_a_froxel_full_of_area_lights() {
    let (extent, frames) = price_frame();
    let ring = |index: usize| {
        let turn = index as f32 * std::f32::consts::TAU / PRICE_LIGHTS as f32;
        Vec3::new(turn.cos() * 0.6, STRIP_HEIGHT, turn.sin() * 0.6)
    };
    let points: Vec<Light> = (0..PRICE_LIGHTS)
        .map(|index| {
            Light::Point(PointLight {
                position: ring(index),
                radius: STRIP_REACH,
                color: STRIP_COLOR / PRICE_LIGHTS as f32,
                fill: false,
            })
        })
        .collect();
    let rects: Vec<Light> = (0..PRICE_LIGHTS)
        .map(|index| {
            Light::Rect(RectLight {
                position: ring(index),
                radius: STRIP_REACH,
                color: STRIP_COLOR / PRICE_LIGHTS as f32,
                direction: Vec3::NEG_Y,
                tangent: Vec3::X,
                half_width: STRIP_LONG,
                half_height: STRIP_SHORT,
                fill: false,
            })
        })
        .collect();

    let Some(prices) = forward_pass_prices(&[&[], &points, &rects], extent, frames) else {
        // Drawn, not priced. The frames above went through the whole forward
        // pass with sixteen rectangles in a froxel — a shading path that
        // trapped or refused a pipeline would have failed inside the helper —
        // and this backend reports no way to time them.
        eprintln!(
            "{}: the forward pass drew {PRICE_LIGHTS} area lights and this backend reports no \
             TIMESTAMP_QUERY, so the rung's price went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let [none, punctual, area] = prices[..].try_into().expect("one price per set");
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    eprintln!(
        "{}: forward at {}x{} over {frames} recorded frames — sun only {:.3}/{:.3} ms, \
         {PRICE_LIGHTS} point {:.3}/{:.3} ms, {PRICE_LIGHTS} rect {:.3}/{:.3} ms (p50/p95)",
        crate::SUITE,
        extent.0,
        extent.1,
        ms(none.0),
        ms(none.1),
        ms(punctual.0),
        ms(punctual.1),
        ms(area.0),
        ms(area.1),
    );

    // Anti-vacuity first: a run whose timestamps came back as zeroes would
    // satisfy every ordering below without having measured anything.
    assert!(
        none.0 > 0 && punctual.0 > 0 && area.0 > 0,
        "a forward pass that took no time at all was not measured"
    );
    assert!(
        area.0 > none.0,
        "a froxel full of area lights cost {} ns where the sun alone cost {} ns, so the light \
         loop is not shading them",
        area.0,
        none.0
    );
    // **Bounded on both sides, and the lower bound is the load-bearing one.**
    // A rectangle's shading is two polygon integrals against a point light's one
    // half-vector and one lobe, so it must cost *more*: a run where the two came
    // out level is one where the light loop reached the same arithmetic for
    // both, which is what a rectangle silently shaded as a point light looks
    // like from here. The measured ratio is 3.7 on radv and 2.3 on lavapipe, so
    // there is room above one. Sixteen is the ceiling, well above either tier
    // and well below what a per-fragment fit or a per-light table read costs.
    let punctual_cost = punctual.0.saturating_sub(none.0).max(1);
    let area_cost = area.0.saturating_sub(none.0).max(1);
    eprintln!(
        "{}: over the sun-only frame, {PRICE_LIGHTS} point lights cost {punctual_cost} ns and \
         {PRICE_LIGHTS} rectangles cost {area_cost} ns",
        crate::SUITE
    );
    assert!(
        area_cost > punctual_cost,
        "a froxel of rectangles cost {area_cost} ns where the same froxel of point lights cost \
         {punctual_cost} ns, so the rectangles are not being integrated"
    );
    assert!(
        area_cost < punctual_cost * 16,
        "an area light costs {area_cost} ns where a point light costs {punctual_cost} ns, which \
         is more than the polygon integral can account for"
    );
}
