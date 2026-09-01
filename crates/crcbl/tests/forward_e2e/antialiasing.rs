//! What antialiasing a run whose player has configured nothing actually draws.
//!
//! The rung an unconfigured run resolves is answered by
//! `crcbl::settings::antialiasing_or_default`, which the console's
//! `antialiasing` row and `crcbl::settings::presets::selected` both read rather
//! than each spelling the fallback. Nothing above this file could tell you
//! whether that answer is the one a frame is drawn with: an assertion at the
//! settings level can only compare the helper against the constant it is
//! written in terms of, which restates its body and holds however the resolve
//! slot is filled. That is not a hypothetical — the helper was changed to
//! answer [`Antialiasing::None`] and the whole of `crcbl` stayed green.
//!
//! So the question is asked of a frame instead. An untouched settings stack
//! goes in one end, a compiled pass list comes out the other, and the two
//! accounts of the same rung are compared: what the settings layer says an
//! unconfigured player gets, against which resolve passes the renderer actually
//! recorded. A fallback that answered "no antialiasing" while the frame still
//! resolved — or a renderer that stopped recording the resolve while the
//! fallback still named one — is a disagreement between them, and there is no
//! way to satisfy this by restating either side.
//!
//! **`render_scale` and the effect switches have the same gap and this does not
//! close it**; `docs/backlog.md` carries what is left.

use crcbl::hal::{CommandEncoderDesc, PresentInfo, SubmitInfo};
use crcbl::render::{Antialiasing, EffectRequest, ForwardRenderer, RenderGraph, TransientPool};
use crcbl::store::MemoryStorage;
use crcbl::store::settings::SettingsStack;

use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube};

/// The pass labels a rung puts in a frame, transcribed from the two modules
/// that record them — `crcbl_render::fxaa` adds one pass and
/// `crcbl_render::smaa` adds three.
///
/// **Written out here rather than asked of the renderer**, and that is the
/// whole of what makes this test able to fail: a table the renderer handed over
/// would be the renderer agreeing with itself, and would hold whatever ended up
/// in the resolve slot. These are a second, independent account of the same
/// frame, so the two can disagree.
fn resolve_labels(tier: Antialiasing) -> &'static [&'static str] {
    match tier {
        // The rung that records no resolve at all: the tonemap writes the
        // caller's target and there is no second image in the frame.
        Antialiasing::None => &[],
        Antialiasing::Fxaa => &["fxaa"],
        Antialiasing::Smaa => &["smaa-edges", "smaa-weights", "smaa-blend"],
    }
}

/// **An unconfigured run draws the rung its own settings layer resolves.**
///
/// Both halves, and neither is redundant. The frame recorded a resolve at all —
/// which is what a fallback quietly answering [`Antialiasing::None`] would take
/// away, and the regression this module was written for. And the resolve it
/// recorded is the one the settings layer named, which is what a fallback
/// answering the *wrong* rung would break.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn an_unconfigured_run_draws_the_rung_its_settings_resolve() {
    // A player who has configured nothing: no settings file, no keys, nothing
    // written. The same state `SettingsSource::None` puts a run in, and the
    // state every run is in before an options screen is ever opened.
    let storage = MemoryStorage::new();
    let stack = SettingsStack::from_storage(&storage);

    // What the settings layer says this run resolves, read through
    // `presets::current_values` because that is the public reader of the
    // fallback under test — the same call the console's `antialiasing` row and
    // `presets::selected` are built on. Building a `RenderEffects` by hand here
    // would walk around the code this is about and check the frame against a
    // constant.
    let resolved = crcbl::settings::presets::current_values(&stack).antialiasing;
    // And the section a start-up reads, handed to the renderer the way
    // `GpuContext::effect_request` hands it: the player's clamp and the
    // player's rung, with the camera and programmatic layers left at their
    // defaults because neither is the settings file's to answer.
    let video = crcbl::settings::video(&stack);

    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        video: video.effects,
        antialiasing: video.antialiasing,
        ..EffectRequest::default()
    });
    // Geometry in the frame, so the resolve has something with an edge in it to
    // run over and the pass list below is a real frame's rather than an empty
    // one's.
    place_cube(&mut renderer);

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let camera = mesh_camera(crcbl::render::Projection::default());
    renderer
        .begin_frame(
            device,
            &camera,
            &crcbl::render::DirectionalLight::default(),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("antialiasing frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            ForwardRenderer::present_target(
                acquired.image,
                acquired.view,
                headless.format,
                MESH_EXTENT,
            ),
        );
        let _ = renderer.add_passes(&mut graph, &pool, target, MESH_EXTENT);
        graph.compile(&pool).expect("a legal frame")
    };

    // Every label any rung can contribute, so the filter below reads "the
    // resolve passes this frame has" rather than "the passes the expected rung
    // has" — the latter finds nothing whenever the frame drew a different tier,
    // and passes by agreeing that the tier it looked for is absent.
    let vocabulary: Vec<&str> = Antialiasing::ALL
        .into_iter()
        .flat_map(|tier| resolve_labels(tier).iter().copied())
        .collect();
    // Read off the graph and owned, because `execute` consumes it — and the
    // frame is executed rather than only compiled, so what these labels
    // describe is a frame the device actually ran.
    let drawn: Vec<String> = compiled
        .passes()
        .iter()
        .map(|pass| pass.label().to_owned())
        .filter(|label| vocabulary.contains(&label.as_str()))
        .collect();

    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
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
    device.wait_idle().expect("idle");

    let drawn: Vec<&str> = drawn.iter().map(String::as_str).collect();
    eprintln!(
        "{suite}: antialiasing — an unconfigured stack resolves {resolved:?} and the frame \
         recorded {drawn:?}",
        suite = crate::SUITE,
    );

    // **The frame resolved.** First, because the comparison below is satisfied
    // by both sides being empty, and that pair is exactly the regression: a
    // fallback answering `Antialiasing::None` agrees with a frame that recorded
    // no resolve, and an unconfigured run ships without one while every
    // assertion in the tree stays green.
    assert!(
        !drawn.is_empty(),
        "a frame drawn from a settings stack nobody has touched recorded none of {vocabulary:?}, \
         so an unconfigured run resolves nothing at all"
    );
    // **And it is the rung the settings layer named.**
    assert_eq!(
        drawn,
        resolve_labels(resolved),
        "an unconfigured stack resolves {resolved:?}, and the frame it was drawn from recorded \
         {drawn:?} — the fallback and the frame name different rungs"
    );

    device.destroy_command_buffer(commands);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
