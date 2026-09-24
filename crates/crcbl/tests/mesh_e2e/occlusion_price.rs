//! **What the occlusion cull costs, and what it saves** —
//! topic 43's rule that a rung is priced before it is
//! called built.
//!
//! Two scenes, each drawn through a renderer culling by occlusion and one that
//! does not, interleaved a frame each per turn on one device — `area_light.rs`'s
//! reason: a software rasteriser's "GPU" time is CPU time, and contention during
//! one configuration's turn would otherwise read as that configuration's cost.
//! **Two renderers at a time**, never more, because CI's WARP has little device
//! memory (`mesh_e2e/grass.rs`'s price is where that was found).
//!
//! * **`Scene::Occluders`**, walking its camera path, where the cull has most of
//!   the frame to hide — the saving.
//! * **The meadow**, a hillside with nothing standing in front of anything,
//!   where the cull hides nothing — the overhead: the farthest pyramid, the
//!   second phase and the late prepass, paid for no draw removed.
//!
//! The durations are printed and not asserted, for `area_light.rs`'s reason: a
//! millisecond is a property of the machine it was read on. What is asserted is
//! that everything was measured, and that the culling renderer drew fewer
//! instances than the plain one over the occluders' path — asked of the
//! renderers' own counters rather than of the clock.
//!
//! ```text
//! CRCBL_PRICE_SIZE=1920x1080 CRCBL_PRICE_FRAMES=400 \
//!   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh occlusion_price
//! ```

use std::time::Instant;

use crate::area_light::{PRICE_WARMUP, price_frame};
use crate::harness::Headless;
use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};
use crcbl::render::{ForwardRenderer, OcclusionCulling, PassStats, PassTimers, TransientPool};
use crcbl::screenshot::{
    OCCLUDERS_CULLING, OCCLUDERS_PATH_FRAMES, occluders_camera, occluders_walker_desc,
};

/// The passes the occlusion cull adds or changes, by the label the graph gives
/// each. `occlusion-hiz` stands for every level of the farthest pyramid, which
/// are summed.
const PRICED: [&str; 9] = [
    "cull",
    "occlusion-hiz",
    "occlusion-late",
    "draw-late-scatter",
    "draw-late-finish",
    "depth-prepass",
    "depth-prepass-late",
    "forward",
    "shadow",
];

/// Which scene a configuration draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Priced {
    Occluders,
    Meadow,
}

/// One configuration's renderer and what it measured.
struct Configuration {
    renderer: ForwardRenderer,
    pool: TransientPool,
    timers: Option<PassTimers>,
    stats: PassStats,
    /// Each recorded frame's summed pass time, in nanoseconds.
    totals: Vec<u64>,
    /// Each frame's CPU time from `begin_frame` to submit, in nanoseconds.
    cpu: Vec<u64>,
    /// Instances the cull left drawn, summed over the frames the ring reported.
    drawn: u64,
    /// Survivors the second phase kept hidden, summed over the same frames.
    hidden: u64,
    reported: std::collections::BTreeSet<u64>,
    walker: Option<crcbl::render::InstanceHandle>,
    commands: Vec<crcbl::hal::CommandBufferHandle>,
}

/// The p50 of `samples`, or zero for none.
fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied().unwrap_or(0)
}

/// A pass's p50 and p95 in `stats`, the pyramid's levels summed.
fn pass_price(stats: &PassStats, label: &str) -> Option<(u64, u64)> {
    if label == "occlusion-hiz" {
        let levels: Vec<(u64, u64)> = stats
            .labels()
            .filter(|seen| seen.starts_with("occlusion-hiz-"))
            .filter_map(|seen| stats.percentiles(seen))
            .collect();
        return (!levels.is_empty()).then(|| {
            levels
                .iter()
                .fold((0, 0), |(p50, p95), level| (p50 + level.0, p95 + level.1))
        });
    }
    stats.percentiles(label)
}

/// Builds `scene` culling as `culling` asks.
fn configuration(
    headless: &Headless,
    scene: Priced,
    culling: OcclusionCulling,
    timed: bool,
) -> Configuration {
    let device = headless.device.as_ref();
    let (renderer, walker) = match scene {
        Priced::Occluders => {
            let built = crcbl::screenshot::occluders_forward_on_path(
                device,
                headless.queue,
                headless.format,
                device.preferred_geometry_path(),
                culling,
            )
            .expect("the occluders scene builds");
            (*built.scene.renderer, Some(built.walker))
        }
        Priced::Meadow => {
            let mut built =
                crcbl::screenshot::meadow_forward(device, headless.queue, headless.format, true)
                    .expect("the meadow builds");
            built.renderer.set_occlusion_culling(culling);
            (*built.renderer, None)
        }
    };
    Configuration {
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
        totals: Vec::new(),
        cpu: Vec::new(),
        drawn: 0,
        hidden: 0,
        reported: std::collections::BTreeSet::new(),
        walker,
        commands: Vec::new(),
    }
}

/// Draws `scene` through a culling and a plain renderer, interleaved, and
/// prints both rows. Returns each row's instances drawn per reported frame,
/// culling first, and the survivors the culling row kept hidden.
fn price(scene: Priced, extent: (u32, u32), frames: usize) -> (f64, f64, u64) {
    let headless = Headless::open_at(
        extent,
        crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES
            .union(Features::TIMESTAMP_QUERY)
            .union(Features::DEBUG_MARKERS),
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let mut rows = [OCCLUDERS_CULLING, OcclusionCulling::OFF]
        .map(|culling| configuration(&headless, scene, culling, timed));
    let meadow_camera = crcbl::screenshot::meadow_camera();

    for index in 0..PRICE_WARMUP + frames {
        for row in &mut rows {
            let camera = match scene {
                Priced::Occluders => occluders_camera(index % OCCLUDERS_PATH_FRAMES),
                Priced::Meadow => meadow_camera,
            };
            if let Some(walker) = row.walker {
                row.renderer.set_instance(
                    walker,
                    &occluders_walker_desc(index % OCCLUDERS_PATH_FRAMES),
                );
            }
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("the ring always has an image");
            let started = Instant::now();
            row.renderer
                .begin_frame(
                    device,
                    &camera,
                    &crcbl::render::DirectionalLight::default(),
                    extent,
                )
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
                row.renderer
                    .add_passes(&mut graph, &row.pool, target, extent);
                graph.compile(&row.pool).expect("a legal frame")
            };
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("occlusion priced frame"),
                queue: headless.queue,
            });
            compiled
                .execute(device, &mut row.pool, encoder.as_mut(), row.timers.as_mut())
                .expect("the graph executed");
            let commands = encoder.finish().expect("recording succeeded");
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            let cpu = started.elapsed();
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
            if index >= PRICE_WARMUP {
                row.cpu
                    .push(u64::try_from(cpu.as_nanos()).unwrap_or(u64::MAX));
                if let Some(timers) = row.timers.as_ref() {
                    let latest = timers.latest();
                    if row.stats.record(latest) {
                        row.totals
                            .push(latest.passes.iter().map(|pass| pass.gpu_nanos).sum());
                    }
                }
                if let Some(stats) = row.renderer.cull_stats()
                    && row.reported.insert(stats.frame)
                {
                    row.drawn += stats.instances - stats.occlusion.late_rejects;
                    row.hidden += stats.occlusion.late_rejects;
                }
            }
        }
    }
    device.wait_idle().expect("idle");

    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    for (row, name) in rows.iter().zip(["culling", "plain"]) {
        let passes: Vec<String> = PRICED
            .iter()
            .map(|label| match pass_price(&row.stats, label) {
                Some((p50, p95)) => format!("{label} {:.3}/{:.3}", ms(p50), ms(p95)),
                None => format!("{label} -"),
            })
            .collect();
        eprintln!(
            "{}: {scene:?} {name} at {}x{} over {} recorded frames: gpu frame p50 {:.3} ms, cpu \
             record+submit p50 {:.3} ms, {} instances drawn over {} reported frames; passes \
             (p50/p95 ms): {}",
            crate::SUITE,
            extent.0,
            extent.1,
            row.stats.frames(),
            ms(median(&row.totals)),
            ms(median(&row.cpu)),
            row.drawn,
            row.reported.len(),
            passes.join(", "),
        );
    }
    let measured = rows.iter().all(|row| !timed || median(&row.totals) > 0);
    let per_frame = |row: &Configuration| row.drawn as f64 / row.reported.len().max(1) as f64;
    let drawn = (per_frame(&rows[0]), per_frame(&rows[1]), rows[0].hidden);
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
    assert!(measured, "a row recorded frames whose passes took no time");
    if !timed {
        eprintln!(
            "{}: this backend reports no TIMESTAMP_QUERY, so the {scene:?} price went unmeasured \
             here and only the draw counts above were read",
            crate::SUITE
        );
    }
    drawn
}

/// **What the cull saves where there is something to hide**: `Scene::Occluders`
/// along its path, and the culling renderer draws fewer instances than the
/// plain one.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh occlusion_price"]
fn the_price_of_occlusion_culling_behind_walls() {
    let (extent, frames) = price_frame();
    let (culled, plain, hidden) = price(Priced::Occluders, extent, frames);
    assert!(
        hidden > 0 && culled < plain,
        "the culling renderer drew {culled:.1} instances a frame and the plain one {plain:.1} \
         over the same path, hiding {hidden}, so the cull hid nothing behind the walls it was \
         priced against"
    );
}

/// **What the cull costs where there is nothing to hide**: the meadow, drawn
/// with the cull on and off.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh occlusion_price"]
fn the_price_of_occlusion_culling_in_the_open() {
    let (extent, frames) = price_frame();
    let (culled, plain, hidden) = price(Priced::Meadow, extent, frames);
    assert_eq!(
        hidden, 0,
        "the meadow has nothing standing in front of anything, and the cull still hid {hidden} \
         instances ({culled:.1} a frame drawn against {plain:.1})"
    );
}
