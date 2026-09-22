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
//! **`render_scale` and every public `VIDEO_KEYS` switch are held below.**

use crcbl::hal::{CommandEncoderDesc, PresentInfo, SubmitInfo};
use crcbl::render::{
    Antialiasing, EffectRequest, ForwardRenderer, RenderEffects, RenderGraph, TransientPool,
};
use crcbl::store::MemoryStorage;
use crcbl::store::settings::SettingsStack;

use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube};

/// The pass labels a rung puts in a frame, transcribed from the two modules
/// that record them — `crcbl_render::fxaa` adds one pass and
/// `crcbl_render::cmaa2` adds three.
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
        Antialiasing::Cmaa2 => &["cmaa2-edges", "cmaa2-shapes", "cmaa2-apply"],
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

/// The passes a settings-derived frame executes, with their render extents.
fn frame_passes(stack: &SettingsStack, label: &str) -> Vec<(String, (u32, u32))> {
    let started = std::time::Instant::now();
    let stage = |phase: &str| {
        eprintln!(
            "{suite}: settings frame {label:?} — {phase} after {elapsed:?}",
            suite = crate::SUITE,
            elapsed = started.elapsed(),
        );
    };
    stage("opening device");
    let video = crcbl::settings::video(stack);
    let headless = Headless::open_for_mesh();
    stage("device opened");
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    stage("renderer built");
    // The default camera stack deliberately excludes lens effects. This frame is
    // the all-on control for the player's clamps, so it must ask for every
    // switch before `apply_video_to` intersects that request with `video`.
    let mut request = renderer.effect_request();
    request.camera = RenderEffects::all();
    renderer.set_effect_request(request);
    crcbl::settings::apply_video_to(&mut renderer, device, &video)
        .expect("the settings video section applies to this device");
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
    stage("frame prepared");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some(label),
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
    let passes = compiled
        .passes()
        .iter()
        .map(|pass| {
            (
                pass.label().to_owned(),
                (pass.render_area().width, pass.render_area().height),
            )
        })
        .collect();

    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    stage("graph recorded");
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    stage("submitted");
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
    stage("presented");
    device.wait_idle().expect("idle");
    stage("idle");

    device.destroy_command_buffer(commands);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    stage("teardown complete");
    passes
}

/// The core scene passes' extents, read from a graph the device executed.
fn frame_extents(stack: &SettingsStack, label: &str) -> Vec<(String, (u32, u32))> {
    frame_passes(stack, label)
        .into_iter()
        .filter(|(label, _)| {
            matches!(
                label.as_str(),
                "depth-prepass" | "forward" | "tonemap" | "upscale"
            )
        })
        .collect()
}

/// The labels a persisted video-effect clamp leaves in the executed frame.
fn frame_labels(stack: &SettingsStack, label: &str) -> Vec<String> {
    frame_passes(stack, label)
        .into_iter()
        .map(|(label, _)| label)
        .collect()
}

/// **The settings video section sizes the frame it opens.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn settings_render_scale_reaches_the_graph_extents() {
    // No file is the documented unrestricted default: the internal target is
    // the caller's extent and an upscale pass would be incorrect.
    let storage = MemoryStorage::new();
    let untouched = SettingsStack::from_storage(&storage);
    let default_video = crcbl::settings::video(&untouched);
    assert_eq!(
        default_video.render_scale, 1.0,
        "an untouched stack must resolve the documented full-scale default"
    );
    let full = frame_extents(&untouched, "default render scale frame");
    assert_eq!(
        full,
        vec![
            ("depth-prepass".to_owned(), MESH_EXTENT),
            ("forward".to_owned(), MESH_EXTENT),
            ("tonemap".to_owned(), MESH_EXTENT),
        ],
        "the full-scale default must record the frame's core passes at its full internal extent"
    );

    // Low's documented, persisted scale. It must enter through the public
    // settings writer and application seam rather than the renderer setter.
    const LOW_RENDER_SCALE: f32 = 0.75;
    let mut written = SettingsStack::from_storage(&storage);
    crcbl::settings::set_render_scale(&mut written, LOW_RENDER_SCALE)
        .expect("the supported low scale enters the user layer");
    written
        .save(
            &storage,
            std::path::Path::new(crcbl::store::settings::SETTINGS_FILE),
        )
        .expect("the supported low scale persists");
    let persisted = SettingsStack::from_storage(&storage);
    let scaled_video = crcbl::settings::video(&persisted);
    assert_eq!(
        scaled_video.render_scale, LOW_RENDER_SCALE,
        "the persisted low scale must be what start-up reads"
    );
    let scaled = frame_extents(&persisted, "persisted render scale frame");
    assert_eq!(
        scaled,
        vec![
            ("depth-prepass".to_owned(), (192, 144)),
            ("forward".to_owned(), (192, 144)),
            ("tonemap".to_owned(), (192, 144)),
            ("upscale".to_owned(), MESH_EXTENT),
        ],
        "the persisted low scale must shrink the graph's internal passes and reconstruct to the target"
    );
}

/// **Every persisted `VIDEO_KEYS` clamp reaches the frame and removes only its effect.**
///
/// One test per switch rather than one test walking all of them, because every
/// frame here opens a fresh device and a fresh renderer, and on a software
/// rasteriser with no shader cache that frame compiles the forward pipeline's
/// shaders again inside its first submission. Walked in one test, the control
/// and every arm were back-to-back frames of that kind, and on the Windows
/// lavapipe runner the walk used nearly all of the kill that `slow-timeout` in
/// `.config/nextest.toml` sets, on every green run (read off ten CI logs on
/// 2026-09-22), so a slower runner timed it out. Split, each test is the
/// control and its own arm, and nextest schedules the arms beside the rest of
/// the suite.
///
/// The arm list is written once, here, and generates both the tests and
/// `each_persisted_video_effect_switch_reaches_the_frame::ARMS`, which
/// [`every_video_key_has_its_own_frame_test`] holds against `VIDEO_KEYS`. A
/// switch added to the settings layer therefore fails that check until it has a
/// test of its own — the guarantee the single test's `unknown => panic!` arm
/// gave before.
macro_rules! persisted_video_switch_tests {
    ($($key:ident),+ $(,)?) => {
        mod each_persisted_video_effect_switch_reaches_the_frame {
            /// The `VIDEO_KEYS` entries with a test below, in its spelling.
            pub(super) const ARMS: &[&str] = &[$(stringify!($key)),+];

            $(
                #[test]
                #[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
                fn $key() {
                    super::a_persisted_video_switch_reaches_the_frame(stringify!($key));
                }
            )+
        }
    };
}

persisted_video_switch_tests!(
    shadows,
    ambient_occlusion,
    reflections,
    bloom,
    volumetric_fog,
    auto_exposure,
);

/// **No `VIDEO_KEYS` switch is without a frame test.**
///
/// Needs no device: it compares the settings layer's switch list against the
/// arms `persisted_video_switch_tests!` generated a test for.
#[test]
fn every_video_key_has_its_own_frame_test() {
    let keys: Vec<&str> = crcbl::settings::VIDEO_KEYS
        .iter()
        .map(|(key, _)| *key)
        .collect();
    assert_eq!(
        keys,
        each_persisted_video_effect_switch_reaches_the_frame::ARMS,
        "every `VIDEO_KEYS` switch needs an arm in `persisted_video_switch_tests!`, and every arm \
         there must name a `VIDEO_KEYS` switch"
    );
}

/// The all-on control, then the frame with `key`'s persisted switch off, and
/// the assertion that the switch removed its own pass family and nothing else.
fn a_persisted_video_switch_reaches_the_frame(key: &str) {
    // An untouched stack is the all-on control. `frame_passes` explicitly asks
    // for `RenderEffects::all()` in the camera layer before applying this video
    // section, because the default stack excludes lens effects.
    let control_storage = MemoryStorage::new();
    let control = SettingsStack::from_storage(&control_storage);
    let control_video = crcbl::settings::video(&control);
    assert_eq!(
        control_video.effects,
        RenderEffects::all(),
        "an untouched settings stack must allow every public video effect"
    );
    let full = frame_labels(&control, "all video effects frame");

    // These labels are independently transcribed from the graph producers. They
    // make the control prove it asks for every lens effect before an off arm can
    // prove that it removed one.
    for label in [
        "ssao",
        "ssao-blur",
        "ssao-upsample",
        "hiz-1",
        "hiz-2",
        "ssr",
        "ssr-blur",
        "bloom-down-1",
        "bloom-down-2",
        "bloom-up-1",
        "bloom-composite",
        "volumetric-scatter",
        "volumetric-integrate",
        "volumetric-composite",
        "exposure-clear",
        "exposure-histogram",
        "exposure-reduce",
    ] {
        assert!(
            full.iter().any(|recorded| recorded == label),
            "the all-on control recorded no `{label}` pass: {full:?}"
        );
    }

    let effect = crcbl::settings::VIDEO_KEYS
        .iter()
        .find_map(|&(candidate, effect)| (candidate == key).then_some(effect))
        .unwrap_or_else(|| panic!("`{key}` is not a `VIDEO_KEYS` switch"));
    let witness_labels: &[&str] = match key {
        "shadows" => &[],
        "ambient_occlusion" => &["ssao", "ssao-blur", "ssao-upsample"],
        "reflections" => &["hiz-1", "hiz-2", "ssr", "ssr-blur"],
        "bloom" => &[
            "bloom-down-1",
            "bloom-down-2",
            "bloom-up-1",
            "bloom-composite",
        ],
        "volumetric_fog" => &[
            "volumetric-scatter",
            "volumetric-integrate",
            "volumetric-composite",
        ],
        "auto_exposure" => &["exposure-clear", "exposure-histogram", "exposure-reduce"],
        unknown => panic!("VIDEO_KEYS gained an untested effect switch `{unknown}`"),
    };
    let storage = MemoryStorage::new();
    let mut written = SettingsStack::from_storage(&storage);
    let expected_effects = RenderEffects::all().difference(effect);
    crcbl::settings::set_video_effects(&mut written, expected_effects)
        .expect("every public video switch writes to the user layer");
    written
        .save(
            &storage,
            std::path::Path::new(crcbl::store::settings::SETTINGS_FILE),
        )
        .expect("the video-effect switches persist");
    let reopened = SettingsStack::from_storage(&storage);
    let video = crcbl::settings::video(&reopened);
    assert_eq!(
        video.effects, expected_effects,
        "reopening the saved `{key}` arm must resolve exactly all effects but its bit"
    );
    let off = frame_labels(&reopened, &format!("{key} disabled frame"));

    if effect == RenderEffects::SHADOWS {
        let shadow = full
            .iter()
            .position(|label| label == "shadow")
            .expect("the all-on control records the shadow atlas pass");
        assert!(
            shadow > 0,
            "the all-on frame must have shadow culls before `shadow`: {full:?}"
        );
        let off_shadow = off
            .iter()
            .position(|label| label == "shadow")
            .expect("the shadow atlas pass remains when its rendering is disabled");
        assert_eq!(
            shadow.checked_sub(off_shadow),
            Some(crcbl::render::DrawGen::MAX_PASSES as usize * crcbl::render::shadow::CASCADES,),
            "disabling persisted shadows must remove every cascade's cull passes: \
             full {full:?}, off {off:?}"
        );
        assert!(
            off[..off_shadow].iter().any(|label| label == "cull"),
            "the shadow-off frame must retain camera culls before `shadow`: {off:?}"
        );
        assert_eq!(
            &off[off_shadow..],
            &full[shadow..],
            "the full suffix beginning at `shadow` must be unchanged when persisted shadows are disabled"
        );
    } else {
        let expected: Vec<String> = full
            .iter()
            .filter(|label| {
                let label = label.as_str();
                let removed = match key {
                    "ambient_occlusion" => {
                        label == "ssao"
                            || label.starts_with("ssao-blur")
                            || label == "ssao-upsample"
                    }
                    "reflections" => {
                        label.starts_with("hiz-") || label == "ssr" || label == "ssr-blur"
                    }
                    "bloom" => {
                        label.starts_with("bloom-down-")
                            || label.starts_with("bloom-up-")
                            || label == "bloom-composite"
                    }
                    "volumetric_fog" => witness_labels.contains(&label),
                    "auto_exposure" => witness_labels.contains(&label),
                    "shadows" => false,
                    unknown => {
                        panic!("VIDEO_KEYS gained an untested effect switch `{unknown}`")
                    }
                };
                !removed
            })
            .cloned()
            .collect();
        assert_eq!(
            off, expected,
            "disabling persisted `{key}` must remove only its pass family; the control witnesses {witness_labels:?}"
        );
    }
}
