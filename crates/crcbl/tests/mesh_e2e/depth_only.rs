//! **What the depth-only passes cost**, now that they fetch stream 0 of the
//! vertex pool and nothing else.
//!
//! `docs/plan/43-render-standards.md` §2 split a vertex into a twelve-byte
//! position region and a twenty-byte attribute region so that the passes which
//! want a clip position and no more could stop reading the whole record.
//! `mesh.slang`'s `depthVertexMain` is what spends that split, and
//! `crcbl_render::forward`'s depth pipeline — the shadow atlas's and the depth
//! prepass's, one pipeline driven with two frame blocks — is what names it.
//!
//! # What is asked here, and what is asked elsewhere
//!
//! **That the entry point reads the position stream alone** is a property of
//! the compiled artifact and is asked without a device, in
//! `crcbl_shaders::mesh`'s
//! `the_depth_entry_point_fetches_the_position_stream_alone`. **That it draws
//! the same depth as the full stage** is what every golden in this suite and in
//! `tests/forward_e2e/` already asserts: the prepass writes the depth the
//! occlusion pair samples and the cascades write the depth the forward pass
//! compares against, so a transform that landed a fragment elsewhere would move
//! a picture rather than hide.
//!
//! What is left for a device is the **price**, which is the reason the split
//! exists at all: a depth pass over a field of dunes patches against the shaded
//! pass over the same field, timed by the GPU timestamps the render graph
//! already takes around every pass.
//!
//! # The mesh-shader path is not priced here and does not take this rung
//!
//! A device with a mesh stage draws these same tiles through
//! `mesh_cluster.slang`'s `meshMain`, which reads a whole vertex; the depth-only
//! mesh stage is its own rung. The fixture below opens a device without
//! [`Features::MESH_SHADER`](crcbl::hal::Features), which is the path this
//! suite runs everywhere anyway — see `mesh_e2e/main.rs`.

use crate::area_light::{PRICE_WARMUP, price_frame};
use crate::harness::Headless;
use crate::mesh_scene::place;
use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{Camera, ForwardRenderer, Projection, TransientPool};
use crcbl::shaders::dunes::DUNES_EXTENT;

/// Patches along one side of the field the price is measured over.
///
/// **The field is what makes the measurement about vertices rather than about
/// pass overhead**, and it is this wide because a narrower one is not. One patch
/// is `crcbl::shaders::dunes::DUNES_VERTEX_COUNT` vertices and this many squared
/// of them stand in front of the camera; measured on lavapipe on 2026-08-31, a
/// field a third as wide put the depth prepass inside the run-to-run spread of
/// its own repeats, and at this width two repeats of the same build agreed to
/// about a percent. A price nothing can be read out of is a price not taken.
const FIELD_SIDE: usize = 12;

/// The passes this file prices, by the label the render graph gives each — the
/// same strings the debug overlay shows.
///
/// The two depth-only passes first and the shaded pass last, because the
/// ordering asserted below is between them.
const PRICED_PASSES: [&str; 3] = ["shadow", "depth-prepass", "forward"];

/// Where the camera stands: back from the near edge of the field and a little
/// way up, looking at its centre, so every patch is in front of it.
fn field_camera() -> Camera {
    let reach = FIELD_SIDE as f32 * DUNES_EXTENT;
    Camera {
        eye: Vec3::new(0.0, 12.0, -reach - DUNES_EXTENT),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// A renderer and a pool on `headless`, with the field of dunes patches in the
/// frame.
///
/// The patches tile without overlapping — a patch spans `2 * DUNES_EXTENT` on
/// both axes and the step is that — so the vertex count in front of the camera
/// is [`FIELD_SIDE`] squared patches and not one patch drawn many times into the
/// same pixels. What each is drawn at is the level-of-detail cut's decision, as
/// it would be in a frame of a real scene.
fn dunes_field(headless: &Headless) -> (ForwardRenderer, TransientPool) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    let step = 2.0 * DUNES_EXTENT;
    let first = -(FIELD_SIDE as f32 - 1.0) / 2.0;
    for row in 0..FIELD_SIDE {
        for column in 0..FIELD_SIDE {
            place(
                &mut renderer,
                crcbl::render::scene::DEMO_DUNES,
                crcbl::render::scene::DEMO_UNTINTED,
                Mat4::from_translation(Vec3::new(
                    (first + column as f32) * step,
                    0.0,
                    (first + row as f32) * step,
                )),
            );
        }
    }
    (renderer, TransientPool::new())
}

/// Each of [`PRICED_PASSES`]' p50 and p95 in nanoseconds, in that order, or
/// [`None`] where the device reports no way to time a pass.
///
/// `area_light.rs`'s helper is the shape this follows, down to the warm-up and
/// the percentile floor — both are that file's constants rather than a second
/// copy — and the difference is that one scene is drawn rather than three
/// interleaved: the comparison here is between passes **of the same frame**, so
/// contention that lands on one lands on all three and no interleaving can
/// separate them.
fn depth_pass_prices(extent: (u32, u32), frames: usize) -> Option<Vec<(u64, u64)>> {
    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    // Asked for rather than required, on `area_light.rs`'s terms: a backend
    // that cannot time a pass cannot price one, and the frames are drawn
    // either way.
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let camera = field_camera();
    let sun = crcbl::render::DirectionalLight::default();
    let (mut renderer, mut pool) = dunes_field(&headless);
    assert!(
        renderer.selects_levels(),
        "this device chooses no level of a DAG, so the field in front of the camera would be \
         empty and the price would be of an empty pass"
    );
    // **What is being priced, asked of the renderer rather than assumed.** A
    // mesh-shader path draws these same tiles through `mesh_cluster.slang`,
    // which reads a whole vertex, so a number measured there would be a number
    // about a rung this one has not reached. The fixture asks for no mesh stage,
    // and this is what says the device honoured that.
    assert_ne!(
        renderer.geometry_path(),
        crcbl::hal::GeometryPath::MeshShader,
        "the depth pipeline's geometry came from a mesh stage, so `depthVertexMain` drew none \
         of the frames this priced"
    );
    let mut timers = timed.then(|| {
        crcbl::render::PassTimers::new(
            device,
            crcbl::render::forward::FRAMES_IN_FLIGHT,
            crcbl::render::MAX_TIMED_PASSES,
        )
        .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
    });
    let mut stats = crcbl::render::PassStats::new();
    let mut recorded = Vec::new();

    for index in 0..PRICE_WARMUP + frames {
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
            renderer.add_passes(&mut graph, &pool, target, extent);
            graph.compile(&pool).expect("a legal frame")
        };
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("priced frame"),
            queue: headless.queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), timers.as_mut())
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

    device.wait_idle().expect("idle");
    let prices = timed.then(|| {
        eprintln!("{}: {}", crate::SUITE, stats.report());
        PRICED_PASSES
            .iter()
            .map(|pass| {
                stats.percentiles(pass).unwrap_or_else(|| {
                    panic!("the {pass} pass is timed and the window is past its floor")
                })
            })
            .collect()
    });
    if let Some(mut timers) = timers.take() {
        timers.destroy(device);
    }
    for commands in recorded {
        device.destroy_command_buffer(commands);
    }
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    prices
}

/// **The rung's price**: what the shadow atlas and the depth prepass cost
/// beside the shaded pass over the same geometry, with all three fetching their
/// vertices out of the split pool.
///
/// Prints rather than asserts a duration, for `area_light.rs`'s reason — a
/// millisecond figure is a property of the machine it was measured on. What it
/// asserts is the shape: **the depth prepass is cheaper than the forward pass**
/// it precedes. The two draw the same buckets with the same instances through
/// the same graph, and differ in that one reads the position stream and runs no
/// fragment stage while the other reads the whole record, the previous frame's
/// position with it, and shades every covered pixel.
///
/// **What that ordering does not catch, said plainly.** Pointing the depth
/// pipeline back at `vertexMain` leaves it true: measured on 2026-08-31 the
/// prepass then cost about a tenth more on lavapipe and the same to three
/// figures on an RX 7900 XTX, both still far under the forward pass. A
/// threshold on that difference would be a threshold on a duration, which is
/// the thing this file refuses to assert — so what guards the fetch is
/// `crcbl_shaders::mesh`'s
/// `the_depth_entry_point_fetches_the_position_stream_alone`, over the
/// committed artifacts, and what this asserts is that the depth pass has not
/// taken on the colour pass's work altogether.
///
/// The shadow pass is priced beside them and asserted only for having been
/// measured. It draws into atlas tiles of a size this frame's extent does not
/// set, and its cascades reject most of a field this wide — measured on
/// lavapipe on 2026-08-31, widening the field threefold left the shadow pass
/// where it was and tripled the prepass — so an ordering against a camera pass
/// would be an assertion about what the cascades cover rather than about a
/// vertex fetch.
///
/// # A backend that cannot time a pass cannot price one
///
/// CI's Apple Paravirtual device reports no `TIMESTAMP_QUERY`. The frames are
/// still drawn there — a depth pipeline that failed to build, or an entry point
/// the committed manifest did not carry, would fail inside the helper on every
/// backend alike — and the price is reported as unmeasured rather than passing
/// quietly.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh price"]
fn the_price_of_the_depth_only_passes() {
    let (extent, frames) = price_frame();
    let Some(prices) = depth_pass_prices(extent, frames) else {
        eprintln!(
            "{}: a field of {} dunes patches drew through the depth prepass, the shadow atlas \
             and the forward pass, and this backend reports no TIMESTAMP_QUERY, so the rung's \
             price went unmeasured here",
            crate::SUITE,
            FIELD_SIDE * FIELD_SIDE,
        );
        return;
    };
    let [shadow, prepass, forward] = prices[..].try_into().expect("one price per priced pass");
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    eprintln!(
        "{}: a field of {} dunes patches at {}x{} over {frames} recorded frames — shadow \
         {:.3}/{:.3} ms, depth prepass {:.3}/{:.3} ms, forward {:.3}/{:.3} ms (p50/p95)",
        crate::SUITE,
        FIELD_SIDE * FIELD_SIDE,
        extent.0,
        extent.1,
        ms(shadow.0),
        ms(shadow.1),
        ms(prepass.0),
        ms(prepass.1),
        ms(forward.0),
        ms(forward.1),
    );

    // Anti-vacuity first: timestamps that came back as zeroes would satisfy the
    // ordering below without having measured anything.
    assert!(
        shadow.0 > 0 && prepass.0 > 0 && forward.0 > 0,
        "a pass that took no time at all was not measured"
    );
    assert!(
        prepass.0 < forward.0,
        "the depth prepass cost {:.3} ms against the forward pass's {:.3} ms over the same \
         draws; a depth-only pass that is not the cheaper of the two is doing work it has no \
         fragment stage to use",
        ms(prepass.0),
        ms(forward.0),
    );
}
