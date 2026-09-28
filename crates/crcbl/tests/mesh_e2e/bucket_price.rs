//! **What recording one call per bucket costs the CPU** — the price of
//! `docs/backlog.md`'s P21, the per-bucket bind and indirect call every geometry
//! pass records for every view.
//!
//! Two renderers over the same instances, the same lights and the same camera,
//! interleaved a frame each per turn: one whose instances are spread over
//! [`MANY`] buckets — the measured `ew` scene's count — and one whose instances
//! are all in [`FEW`]. Every pass, every view and every instance is the same in
//! both, so what the many-bucket row costs over the few-bucket one is what the
//! per-bucket calls cost, and nothing else.
//!
//! The sun turns a degree a frame and the point light steps along its path, so
//! every cascade and every face of the light is redrawn every frame: the shadow
//! views are recorded rather than held, which is the case the item names.
//!
//! Each path is priced twice where the device can draw a range of buckets per
//! call — on a device not asked for `Features::DRAW_INDEX`, which records a
//! call per bucket, and on one that was — so one run prints the price and what
//! the ranged tail took off it.
//!
//! Printed, not asserted, on `area_light.rs`'s terms: a millisecond is a
//! property of the machine. What is asserted is that the many-bucket row
//! recorded more calls than the few-bucket one without a draw index — the
//! difference the price is of — and exactly as many with one. Except on the
//! mesh tail behind a task stage, whose [`FEW`]-bucket run is shorter than the
//! shortest range it draws in one call: there the few row records what it did
//! without a draw index, and the many row fewer calls than without one.
//!
//! ```text
//! CRCBL_GPU=vk CRCBL_VK_VALIDATION=0 CRCBL_PRICE_FRAMES=240 \
//!   cargo nextest run --release -p crcbl --features mesh-e2e --test mesh_e2e \
//!   --run-ignored all --success-output immediate bucket_price
//! ```

use std::time::Instant;

use crate::area_light::{PRICE_WARMUP, price_frame};
use crate::harness::Headless;
use crate::shadow_cache::turning_sun;
use crcbl::hal::{
    CommandEncoderDesc, Features, GeometryPath, PresentInfo, ResourceState, SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::shadow::Cadence;
use crcbl::render::{
    Camera, ForwardRenderer, InstanceDesc, Light, PassStats, PassTimers, PointLight, Projection,
    TransientPool,
};

/// The measured `ew` scene's bucket count, which the backlog's P21 is about.
const MANY: u32 = 938;

/// The comparison row's bucket count: every instance in one of two buckets.
const FEW: u32 = 2;

/// The measured `ew` scene's instance count, drawn on a price run.
const INSTANCES: u32 = 17_219;

/// The bucket count an ordinary suite run spreads its instances over.
///
/// **Why a suite run draws a smaller scene.** The suite runs on CI's software
/// adapters too, where a frame recorded at [`MANY`] buckets — thousands of
/// calls, each validated — outlasted nextest's per-test limit on lavapipe, WARP
/// and the macOS runner, even with the instances cut. What this test asserts
/// is that many buckets record more calls than [`FEW`], and that the ranged
/// tails record the same number either way; any count well above [`FEW`]
/// shows both. The milliseconds only mean something on a price run, which asks
/// for the measured scene with `CRCBL_PRICE_FRAMES`.
const GATE_MANY: u32 = 64;

/// The frames a suite run records per row, after [`gate_warmup`]'s.
///
/// A suite run asserts call counts, which every frame after the warm-up
/// records identically, so it needs a few frames rather than the percentile
/// floor [`price_frame`] enforces. On the macOS runner, under Metal's API and
/// shader validation, two rows of that many frames alone outlasted nextest's
/// per-test limit.
const GATE_FRAMES: usize = 4;

/// The frames a suite run draws and discards first: enough for every frame in
/// flight to have been through the ring once, and the draw counts to settle.
const fn gate_warmup() -> usize {
    crcbl::render::forward::FRAMES_IN_FLIGHT + 2
}

/// Whether this run is a price run, asked for with `CRCBL_PRICE_FRAMES`.
fn priced() -> bool {
    std::env::var_os("CRCBL_PRICE_FRAMES").is_some()
}

/// The many row's bucket count: [`MANY`] on a price run, [`GATE_MANY`]
/// otherwise.
fn many() -> u32 {
    if priced() { MANY } else { GATE_MANY }
}

/// How many instances this run draws: [`INSTANCES`] on a price run, and two per
/// bucket of the many row otherwise, so every bucket holds an instance.
fn instances() -> u32 {
    if priced() { INSTANCES } else { GATE_MANY * 2 }
}

/// Instances a row of the field holds.
const ROW: u32 = 131;

/// The distance between neighbouring instances, in world units.
const SPACING: f32 = 1.0;

/// The passes whose recording the buckets multiply, by the graph's labels.
const PRICED: [&str; 5] = ["depth-prepass", "forward", "shadow", "rsm", "rsm-punctual"];

/// One row's renderer and what it measured.
struct Row {
    buckets: u32,
    renderer: ForwardRenderer,
    pool: TransientPool,
    timers: Option<PassTimers>,
    stats: PassStats,
    /// Each recorded frame's CPU time, in nanoseconds: `begin_frame`, building
    /// and compiling the graph, executing it into the encoder and finishing it,
    /// and the submit.
    begin: Vec<u64>,
    build: Vec<u64>,
    record: Vec<u64>,
    submit: Vec<u64>,
    /// The calls [`ForwardRenderer::counters`] reported, summed.
    draws: u64,
    commands: Vec<crcbl::hal::CommandBufferHandle>,
}

/// The p50 of `samples`, or zero for none.
fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied().unwrap_or(0)
}

/// The demo scene with its cube repeated into `buckets` meshes, and room for
/// [`instances`] of them.
fn scene(buckets: u32) -> crcbl::render::scene::SceneDesc<'static> {
    let mut scene = crcbl::render::scene::demo();
    scene.meshes = vec![scene.meshes[crcbl::render::scene::DEMO_CUBE].clone(); buckets as usize];
    scene.capacities.meshes = buckets;
    scene.capacities.vertices = buckets * crcbl::shaders::mesh::CUBE_VERTEX_COUNT as u32;
    scene.capacities.indices = buckets * crcbl::shaders::mesh::CUBE_INDEX_COUNT as u32;
    scene.capacities.instances = instances();
    scene
}

/// Where instance `index` stands: a square field centred on the origin.
fn placed(index: u32) -> Mat4 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a field of a hundred or so a side"
    )]
    let (x, z) = (
        (f32::from(u16::try_from(index % ROW).expect("a row")) - ROW as f32 / 2.0) * SPACING,
        (f32::from(u16::try_from(index / ROW).expect("a row")) - ROW as f32 / 2.0) * SPACING,
    );
    Mat4::from_translation(Vec3::new(x, 0.0, z)) * Mat4::from_scale(Vec3::splat(0.4 * SPACING))
}

/// The shadowed point light, a step along its path per `index`.
fn lamp(index: usize) -> Light {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a few hundred frames, and the step is what is wanted"
    )]
    let along = (index % 64) as f32 * 0.01;
    Light::Point(PointLight {
        position: Vec3::new(along, 2.0, 0.0),
        radius: 8.0,
        color: Vec3::splat(20.0),
        fill: false,
    })
}

/// The camera: above and behind the field's middle, looking into it.
fn camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 14.0, 24.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// Builds the `buckets` row on `path`.
fn row(headless: &Headless, buckets: u32, path: GeometryPath, timed: bool) -> Row {
    let device = headless.device.as_ref();
    let mut renderer = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &scene(buckets),
        path,
    )
    .expect("the priced renderer builds");
    renderer.set_shadow_cadence(Some(Cadence::EVERY_FRAME));
    for index in 0..instances() {
        renderer
            .add_instance(&InstanceDesc {
                mesh: (index % buckets) as usize,
                material: crcbl::render::scene::DEMO_UNTINTED,
                transform: placed(index),
            })
            .expect("the instance capacity");
    }
    Row {
        buckets,
        renderer,
        pool: TransientPool::new(),
        timers: timed.then(|| {
            PassTimers::new(
                device,
                crcbl::render::forward::FRAMES_IN_FLIGHT,
                crcbl::render::MAX_TIMED_PASSES,
            )
            .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
        }),
        stats: PassStats::new(),
        begin: Vec::new(),
        build: Vec::new(),
        record: Vec::new(),
        submit: Vec::new(),
        draws: 0,
        commands: Vec::new(),
    }
}

/// Nanoseconds since `started`, and restarts it.
fn lap(started: &mut Instant) -> u64 {
    let now = Instant::now();
    let nanos = u64::try_from((now - *started).as_nanos()).unwrap_or(u64::MAX);
    *started = now;
    nanos
}

/// Draws both rows on `path`, interleaved, and prints them — on a device asked
/// for [`Features::DRAW_INDEX`] where `ranged`, and on one that was not
/// otherwise. Returns the calls each row recorded a frame, many first.
fn price(path: GeometryPath, ranged: bool, extent: (u32, u32), frames: usize) -> (u64, u64) {
    let asked = crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES
        .union(Features::TIMESTAMP_QUERY)
        .union(Features::DEBUG_MARKERS);
    let asked = if ranged {
        asked
    } else {
        asked.difference(Features::DRAW_INDEX)
    };
    let headless = Headless::open_at(extent, asked);
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let warmup = if priced() {
        PRICE_WARMUP
    } else {
        gate_warmup()
    };
    let mut rows = [many(), FEW].map(|buckets| row(&headless, buckets, path, timed));
    let camera = camera();

    for index in 0..warmup + frames {
        for row in &mut rows {
            row.renderer.set_lights(&[lamp(index)]);
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("the ring always has an image");
            let mut started = Instant::now();
            row.renderer
                .begin_frame(device, &camera, &turning_sun(index), extent)
                .expect("the uniform buffer is writable");
            let begin = lap(&mut started);
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
                row.renderer
                    .add_passes(&mut graph, &row.pool, target, extent);
                graph.compile(&row.pool).expect("a legal frame")
            };
            let build = lap(&mut started);
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("bucket priced frame"),
                queue: headless.queue,
            });
            compiled
                .execute(device, &mut row.pool, encoder.as_mut(), row.timers.as_mut())
                .expect("the graph executed");
            let commands = encoder.finish().expect("recording succeeded");
            let record = lap(&mut started);
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            let submit = lap(&mut started);
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
            row.commands.push(commands);
            if index >= warmup {
                row.begin.push(begin);
                row.build.push(build);
                row.record.push(record);
                row.submit.push(submit);
                row.draws += row.renderer.counters().draws;
                if let Some(timers) = row.timers.as_ref() {
                    row.stats.record(timers.latest());
                }
            }
        }
    }
    device.wait_idle().expect("idle");

    #[expect(
        clippy::cast_precision_loss,
        reason = "nanoseconds printed as milliseconds"
    )]
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    for row in &rows {
        let passes: Vec<String> = PRICED
            .iter()
            .map(|label| match row.stats.percentiles(label) {
                Some((p50, p95)) => format!("{label} {:.3}/{:.3}", ms(p50), ms(p95)),
                None => format!("{label} -"),
            })
            .collect();
        eprintln!(
            "{}: {path:?}{}, {} buckets, {} instances at {}x{} over {frames} frames: {} \
             calls a frame; cpu p50 begin {:.3} ms, build {:.3} ms, record {:.3} ms, submit \
             {:.3} ms; gpu passes (p50/p95 ms): {}",
            crate::SUITE,
            if ranged { " ranged" } else { "" },
            row.buckets,
            instances(),
            extent.0,
            extent.1,
            row.draws / frames as u64,
            ms(median(&row.begin)),
            ms(median(&row.build)),
            ms(median(&row.record)),
            ms(median(&row.submit)),
            passes.join(", "),
        );
    }
    let calls = (rows[0].draws / frames as u64, rows[1].draws / frames as u64);
    for row in rows {
        if let Some(mut timers) = row.timers {
            timers.destroy(device);
        }
        for commands in row.commands {
            device.destroy_command_buffer(commands);
        }
        row.renderer.destroy(device);
        let mut pool = row.pool;
        pool.destroy(device);
    }
    headless.finish();
    calls
}

/// **The per-bucket calls' price on every geometry path this device has**:
/// the many-bucket row against the few-bucket one, per path.
#[test]
#[ignore = "needs a real GPU; see this file's header for the release command"]
fn the_price_of_one_call_per_bucket() {
    let buckets = many();
    let (extent, frames) = if priced() {
        price_frame()
    } else {
        (price_frame().0, GATE_FRAMES)
    };
    let features = {
        let headless =
            Headless::open_at(extent, crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES);
        let features = headless.device.caps().features;
        headless.finish();
        features
    };
    // A suite run prices one path, the first this device has in the order
    // below: the ranged tails first, because they are what this test is about.
    // Every path builds its own renderers, and on the macOS runner, under
    // Metal's API and shader validation, each one costs seconds, so three
    // paths outlasted nextest's per-test limit even at `GATE_MANY` buckets.
    // A price run prices them all.
    let mut priced_paths = 0;
    for (path, needs) in [
        (GeometryPath::IndirectCount, Features::DRAW_INDIRECT_COUNT),
        (GeometryPath::IndirectPerBatch, Features::empty()),
        (GeometryPath::MeshShader, Features::MESH_SHADER),
    ] {
        if !features.contains(needs) {
            eprintln!(
                "{}: {path:?} is not on this device, so it went unpriced",
                crate::SUITE
            );
            continue;
        }
        if priced_paths > 0 && !priced() {
            eprintln!(
                "{}: {path:?} is priced on a price run only; this is a suite run",
                crate::SUITE
            );
            continue;
        }
        priced_paths += 1;
        let (many, few) = price(path, false, extent, frames);
        assert!(
            many > few,
            "{path:?}: {buckets} buckets recorded {many} calls a frame and {FEW} recorded {few}, so \
             the rows do not differ in what this prices"
        );
        // A device that can draw a range of buckets per call does, on every
        // tail — and then the bucket count stops deciding how many calls a
        // frame records.
        let ranges = features.contains(Features::MULTI_DRAW_INDIRECT | Features::DRAW_INDEX);
        if ranges {
            let (ranged_many, ranged_few) = price(path, true, extent, frames);
            if path == GeometryPath::MeshShader && features.contains(Features::TASK_SHADER) {
                assert_eq!(
                    ranged_few, few,
                    "{path:?} behind a task stage: {FEW} buckets are a run too short to draw in \
                     one call, so they record a call per bucket either way"
                );
                assert!(
                    ranged_many < many,
                    "{path:?} behind a task stage: {buckets} buckets recorded {ranged_many} calls \
                     a frame with a draw index and {many} without"
                );
            } else {
                assert_eq!(
                    ranged_many, ranged_few,
                    "{path:?} with a draw index: {buckets} buckets recorded {ranged_many} calls a \
                     frame and {FEW} recorded {ranged_few}, so the calls still grow with the \
                     buckets"
                );
            }
        } else {
            eprintln!(
                "{}: {path:?} records a call per bucket on this device, so it has no ranged row",
                crate::SUITE
            );
        }
    }
    assert!(
        priced_paths > 0,
        "no geometry path was priced on this device"
    );
}
