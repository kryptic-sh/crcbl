//! Which mesh pipelines [`ForwardRenderer`]'s build compiles for a scene,
//! counted on the null backend.
//!
//! A bucket table is fixed at build from the modes the description's materials
//! carry, and a probe volume's update mode is fixed with it, so the build
//! knows every pipeline any frame of the renderer will bind — see
//! [`SidedPipelines`]. What this pins is that it compiles exactly those: the
//! single-sided colour and depth shapes and the tile clear for every scene,
//! each `CullMode::None` twin, the cutout shape and the reflective shadow map
//! only for a scene that draws with it, each once and before the first frame,
//! and that frame binding what was built and nothing else.

use super::*;
use crcbl_shaders::mesh::GpuMaterial;

/// Every mesh pipeline the build can compile, by the raster arm's label: the
/// colour shape, the depth shape, the cutout shape and the reflective shadow
/// map, each beside its `CullMode::None` twin, and the tile clear.
const VARIANTS: [&str; 9] = [
    "forward mesh",
    "forward double-sided mesh",
    "shadow cascade",
    "shadow cascade double-sided",
    "shadow cascade masked",
    "shadow cascade masked double-sided",
    "shadow tile clear",
    "rsm",
    "rsm double-sided",
];

/// The variants every scene is built with.
const ALWAYS: [&str; 3] = ["forward mesh", "shadow cascade", "shadow tile clear"];

/// How many graphics pipelines labelled `label` `recorder` has seen created.
fn created(recorder: &Recorder, label: &str) -> usize {
    recorder
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::Created { kind: ObjectKind::GraphicsPipeline, label: Some(created) }
                    if created == label
            )
        })
        .count()
}

/// The demo scene with its untinted row carrying `flags`, and its probes
/// updated every frame where `updated` — the two inputs the build's choice
/// reads.
fn scene_with(flags: u32, updated: bool) -> crate::scene::SceneDesc<'static> {
    let mut scene = crate::scene::demo();
    scene.materials[DEMO_UNTINTED].flags = flags;
    if updated {
        scene.capacities.probes = 1;
        scene.probes = crate::scene::ProbeGrid {
            volume: crcbl_shaders::probe::ProbeVolume {
                origin: [0.0; 3],
                inv_spacing: [1.0; 3],
                counts: [1, 1, 1],
                levels: 1,
                steps: crcbl_shaders::probe::ProbeSteps::default(),
            },
            probes: vec![crcbl_shaders::probe::GpuProbe::ZERO],
            update: ProbeUpdate::EveryFrame,
        };
    }
    scene
}

/// Builds `scene`, checks the build compiled exactly `wanted` of [`VARIANTS`]
/// once each and nothing else of them, then draws two frames with an instance
/// of every material placed and checks they compiled none — so whatever a
/// frame binds existed before it — and that every bind named a live pipeline.
fn builds_exactly(scene: &crate::scene::SceneDesc<'_>, wanted: &[&str]) {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    recorder.clear();
    let mut renderer =
        ForwardRenderer::with_scene(device, queue, Format::Rgba8UnormSrgb, scene).expect("built");
    // The labels above are the raster arm's; the mesh arm's add a suffix.
    assert!(
        !renderer.emit.is_mesh(),
        "this preset built a mesh-shader path, whose labels this does not name"
    );
    for label in VARIANTS {
        let want = usize::from(wanted.contains(&label));
        assert_eq!(
            created(&recorder, label),
            want,
            "the build compiled `{label}` {} time(s) for a scene that wants it {want} time(s)",
            created(&recorder, label)
        );
    }

    // An instance of every material, so each bucket mode the scene holds has
    // draws in it, and a light so the punctual reflective shadow map has a
    // face to draw.
    let mut offset = Vec3::ZERO;
    for material in 0..scene.materials.len() {
        place_demo(
            &mut renderer,
            DEMO_CUBE,
            material,
            Mat4::from_translation(offset),
        );
        offset.x += 1.0;
    }
    renderer.set_lights(&[crate::Light::Spot(crate::SpotLight {
        position: Vec3::new(0.0, 2.0, 0.0),
        radius: 8.0,
        color: Vec3::splat(4.0),
        direction: Vec3::NEG_Y,
        inner_angle: 0.3,
        outer_angle: 0.6,
        fill: false,
    })]);
    recorder.clear();
    frame(device, &mut renderer, queue).release(device);
    frame_moving_the_sun(device, &mut renderer, queue, 1).release(device);
    for label in VARIANTS {
        assert_eq!(
            created(&recorder, label),
            0,
            "a frame compiled `{label}`: the build has to have made every mesh pipeline a \
             frame binds"
        );
    }
    recorder.assert_valid();

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **An opaque, single-sided scene with no updated probes builds no twin, no
/// cutout shape and no reflective shadow map** — the demo, which is every
/// default build.
#[test]
fn an_opaque_single_sided_scene_builds_no_twin() {
    builds_exactly(&crate::scene::demo(), &ALWAYS);
}

/// **A double-sided material builds the colour and depth twins**, once, and
/// still nothing it does not draw with.
#[test]
fn a_double_sided_material_builds_the_twins_it_draws_with() {
    builds_exactly(
        &scene_with(GpuMaterial::DOUBLE_SIDED, false),
        &[
            ALWAYS.as_slice(),
            &["forward double-sided mesh", "shadow cascade double-sided"],
        ]
        .concat(),
    );
}

/// **An alpha-masked material builds the single-sided cutout shape** and no
/// twin of anything.
#[test]
fn a_masked_material_builds_the_cutout_shape_alone() {
    builds_exactly(
        &scene_with(GpuMaterial::ALPHA_MODE_MASK, false),
        &[ALWAYS.as_slice(), &["shadow cascade masked"]].concat(),
    );
}

/// **A masked double-sided material on an updated volume builds the cutout
/// twin, the colour twin and both reflective shadow maps** — and not the
/// opaque depth shape's twin, which only an opaque double-sided bucket draws
/// with.
#[test]
fn a_masked_double_sided_material_on_updated_probes_builds_its_own_set() {
    builds_exactly(
        &scene_with(GpuMaterial::MODE_MASK, true),
        &[
            ALWAYS.as_slice(),
            &[
                "forward double-sided mesh",
                "shadow cascade masked",
                "shadow cascade masked double-sided",
                "rsm",
                "rsm double-sided",
            ],
        ]
        .concat(),
    );
}

/// **A bucket whose pipeline was never built is a panic, not a bind** — the
/// structural half: an opaque scene's renderer handed a double-sided bucket it
/// was not built for has no twin handle to record.
#[test]
#[should_panic(expected = "its pipeline was never built")]
fn a_bucket_without_a_built_pipeline_is_refused() {
    let (_recorder, device, queue) = open();
    let mut renderer =
        ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
    assert_eq!(renderer.mesh_pipeline.double, None);
    let buckets = renderer.bucket_modes.len();
    renderer.bucket_modes = vec![GpuMaterial::DOUBLE_SIDED; buckets];
    frame(device.as_ref(), &mut renderer, queue).release(device.as_ref());
}
