//! **The shadow atlas's depth pass reads no field its cache record leaves
//! out** — `crcbl_render::forward`'s `depth_pass_reads`, held to the shaders on
//! a device.
//!
//! A shadow group's cache record carries only the fields of its view blocks
//! the depth pass reads, and zeroes the rest, so a change to a zeroed field
//! holds the map. That split was read off the shaders by hand. A shader that
//! started reading one of the zeroed fields would hold a map drawn from a stale
//! value of it — a plausible frame, and one no golden catches.
//!
//! # What is asked here
//!
//! [`crcbl::render::forward::FRAME_UNIFORMS_READERS`] is the split as a table,
//! and `crcbl_render`'s own tests hold `depth_pass_reads` to it. This file holds
//! the table to the shaders: for every entry the table calls colour-only, the
//! atlas is drawn once as the frame built it and once with that field
//! overwritten by garbage in every block the depth pass is fed —
//! [`ForwardRenderer::set_shadow_view_tamper`] — and the two atlases are read
//! back and compared **bit for bit**. Each through a renderer meeting the scene
//! for the first time, so both frames redraw every map.
//!
//! **And the probe can see a read.** The same rewrite of `view_proj`, which the
//! depth pass does read, has to move the atlas — otherwise every equality here
//! would pass through a hook that reached nothing.
//!
//! On every geometry path the device builds, since the depth pass is
//! `mesh_cluster.slang`'s stages on the mesh path and `mesh.slang`'s
//! `depthVertexMain` on the others — with a cutout in the scene, which draws
//! through `vertexMain` and `depthMaskedFragmentMain` instead, and a spot and a
//! point light beside the sun's cascades, so every kind of shadow group is
//! drawn.

use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh_lit};
use crate::occlusion_cull::{OPTIONAL, ReadBack, Readable, paths, read_back};
use crcbl::hal::{Capability, GeometryPath};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::forward::{FRAME_UNIFORMS_READERS, UniformsField, UniformsReader};
use crcbl::render::scene::{DEMO_CUBE, DEMO_PYRAMID, DEMO_UNTINTED, PageKind, SceneDesc};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, Light, PointLight, Projection, SpotLight,
    TransientPool,
};
use crcbl::shaders::mesh::{FrameUniforms, GpuMaterial};

/// The frame this file renders at: the suite's own. The atlas is what is
/// compared, and its size is its own.
const EXTENT: (u32, u32) = crate::mesh_scene::MESH_EXTENT;

/// The floor's side, in world units.
const FLOOR: f32 = 24.0;

/// A 2×2 base-colour layer cut down its first column: `screenshot`'s alpha mask
/// in shape, so the plate wearing it is half hole.
const MASK_TEXELS: [u8; 16] = [
    0xFF, 0xFF, 0xFF, 0x00, // (0, 0) — cut
    0xFF, 0xFF, 0xFF, 0xFF, // (1, 0) — kept
    0xFF, 0xFF, 0xFF, 0x00, // (0, 1) — cut
    0xFF, 0xFF, 0xFF, 0xFF, // (1, 1) — kept
];

/// The engine's demo scene with one masked row appended — the plate's — and
/// that row's index.
fn masked_scene() -> (SceneDesc<'static>, usize) {
    let mut scene = crcbl::render::scene::demo();
    let mask = scene.page.push_layer(PageKind::BaseColor, &MASK_TEXELS[..]);
    scene.materials.push(GpuMaterial {
        base_color_texture: mask,
        flags: GpuMaterial::ALPHA_MODE_MASK,
        ..GpuMaterial::UNTINTED
    });
    let plate = scene.materials.len() - 1;
    (scene, plate)
}

/// Where the lights and the camera stand: over a floor with pyramids on it and
/// the plate hung above it, inside the spot's cone and the point light's reach.
fn lights() -> [Light; 2] {
    let spot_at = Vec3::new(-3.0, 6.0, 3.0);
    [
        Light::Spot(SpotLight {
            position: spot_at,
            color: Vec3::splat(30.0),
            radius: 14.0,
            direction: Vec3::new(-1.0, 0.0, 0.0) - spot_at,
            inner_angle: 0.5,
            outer_angle: 0.8,
            fill: false,
        }),
        Light::Point(PointLight {
            position: Vec3::new(3.0, 2.5, -2.0),
            radius: 8.0,
            color: Vec3::splat(20.0),
            fill: false,
        }),
    ]
}

/// The camera: up and back over the middle of the floor.
fn camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 12.0, 10.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// What one fresh renderer's first frame left in the atlas.
struct Drawn {
    /// Every texel of the atlas, as bits.
    atlas: Vec<u32>,
    /// Whether the frame redrew every cascade and every occupied light slot —
    /// the comparison is only about the depth pass if it ran for all of them.
    redrew_everything: bool,
    /// How many light slots a light held.
    occupied: usize,
}

/// Draws one frame on `path` through a renderer meeting the scene for the
/// first time, with the plate wearing `plate` and every shadow view's block
/// rewritten by `tamper`, and reads the atlas back.
fn draw_atlas(
    headless: &Headless,
    path: GeometryPath,
    plate: Option<usize>,
    tamper: Option<fn(&mut FrameUniforms)>,
) -> Drawn {
    let device = headless.device.as_ref();
    let (scene, masked) = masked_scene();
    let mut renderer =
        ForwardRenderer::with_scene_on_path(device, headless.queue, headless.format, &scene, path)
            .unwrap_or_else(|why| panic!("the forward renderer builds on {path:?}: {why}"));
    let mut pool = TransientPool::new();
    place(
        &mut renderer,
        DEMO_CUBE,
        DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -0.5 * FLOOR, 0.0))
            * Mat4::from_scale(Vec3::splat(FLOOR)),
    );
    for at in [
        Vec3::new(-1.5, 0.6, 1.0),
        Vec3::new(2.0, 0.6, -2.5),
        Vec3::new(3.5, 0.6, -0.5),
    ] {
        place(
            &mut renderer,
            DEMO_PYRAMID,
            DEMO_UNTINTED,
            Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(1.5)),
        );
    }
    place(
        &mut renderer,
        DEMO_CUBE,
        plate.unwrap_or(masked),
        Mat4::from_translation(Vec3::new(-1.0, 1.2, 0.0))
            * Mat4::from_scale(Vec3::new(2.0, 0.05, 1.0)),
    );
    renderer.set_lights(&lights());
    renderer.set_shadow_view_tamper(tamper);
    render_mesh_lit(
        headless,
        &mut renderer,
        &mut pool,
        &camera(),
        &DirectionalLight::default(),
        None,
    );
    let occupied: Vec<usize> = (0..crcbl::render::shadow::LIGHT_SLOTS)
        .filter(|slot| renderer.shadow_lights().base_of(*slot).is_some())
        .collect();
    let redrew_everything = !renderer.shadow_atlas_cached()
        && (0..crcbl::render::shadow::CASCADES)
            .all(|cascade| renderer.shadow_cascade_redrawn(cascade))
        && occupied
            .iter()
            .all(|slot| renderer.shadow_slot_redrawn(*slot));
    let (width, height) = crcbl::render::shadow::atlas_extent();
    let atlas = read_back(
        headless,
        &mut pool,
        &[Readable::Depth {
            image: renderer.shadow_atlas(),
            view: renderer.shadow_atlas_view(),
            extent: (width, height),
        }],
    )
    .into_iter()
    .next()
    .map(ReadBack::depth)
    .expect("one image")
    .into_iter()
    .map(f32::to_bits)
    .collect();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    Drawn {
        atlas,
        redrew_everything,
        occupied: occupied.len(),
    }
}

/// How many texels of two atlases differ in any bit.
fn differing(left: &[u32], right: &[u32]) -> usize {
    left.iter().zip(right).filter(|(a, b)| a != b).count()
}

/// The depth-read entry the positive control perturbs: a light's and a
/// cascade's matrix, which every depth path reads.
fn view_proj() -> &'static UniformsField {
    FRAME_UNIFORMS_READERS
        .iter()
        .find(|field| field.name == "view_proj")
        .expect("the table lists view_proj")
}

/// **Overwriting any field the shadow cache record zeroes, in every block the
/// atlas's depth pass is fed, leaves the atlas unchanged to the bit — and
/// nudging `view_proj`, which the record keeps, does not.**
///
/// On every geometry path this device builds, with a cutout and both kinds of
/// light slot in the scene. Each frame is a fresh renderer's first, so every
/// map is drawn, and that is asserted rather than assumed; the untampered frame
/// is drawn twice and the two held equal, so a difference below is the
/// tampering's and not the device's.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_depth_pass_reads_no_field_the_shadow_cache_record_zeroes() {
    let headless = Headless::open_at(EXTENT, OPTIONAL);
    if !headless
        .device
        .supports(Capability::DepthImageCopy)
        .is_yes()
    {
        eprintln!(
            "{}: this device cannot copy a depth image out, so the atlas cannot be read back \
             and the depth pass's reads are not checked here",
            crate::SUITE
        );
        headless.finish();
        return;
    }
    let colour_only: Vec<&UniformsField> = FRAME_UNIFORMS_READERS
        .iter()
        .filter(|field| field.reader == UniformsReader::ColourOnly)
        .collect();
    let paths = paths(&headless);
    let mut failures = Vec::new();
    if colour_only.is_empty() {
        failures.push("the table calls no field colour-only, so nothing is checked".to_owned());
    }
    for &path in &paths {
        let baseline = draw_atlas(&headless, path, None, None);
        let written = baseline.atlas.iter().filter(|bits| **bits != 0).count();
        eprintln!(
            "{}: {path:?}: the atlas holds {written} written texels under {} light slot(s)",
            crate::SUITE,
            baseline.occupied
        );
        if written == 0 {
            failures.push(format!("{path:?}: the atlas holds nothing to compare"));
        }
        if baseline.occupied != lights().len() {
            failures.push(format!(
                "{path:?}: {} of the {} lights held a slot, so not every kind of group is drawn",
                baseline.occupied,
                lights().len()
            ));
        }

        let again = draw_atlas(&headless, path, None, None);
        let unsteady = differing(&baseline.atlas, &again.atlas);
        if unsteady != 0 {
            failures.push(format!(
                "{path:?}: two untampered renderers drew atlases {unsteady} texels apart, so \
                 no equality below means anything"
            ));
        }

        // The cutout drew through the masked pipeline: the same plate made
        // opaque casts a different atlas.
        let opaque = draw_atlas(&headless, path, Some(DEMO_UNTINTED), None);
        let cut = differing(&baseline.atlas, &opaque.atlas);
        eprintln!(
            "{}: {path:?}: the cutout's hole is {cut} texels of the atlas",
            crate::SUITE
        );
        if cut == 0 {
            failures.push(format!(
                "{path:?}: the masked plate and an opaque one drew the same atlas, so no \
                 cutout's depth pass is in this comparison"
            ));
        }

        let control = draw_atlas(&headless, path, None, Some(view_proj().perturb));
        let moved = differing(&baseline.atlas, &control.atlas);
        eprintln!(
            "{}: {path:?}: nudging view_proj moved {moved} texels",
            crate::SUITE
        );
        if moved == 0 {
            failures.push(format!(
                "{path:?}: nudging `view_proj` in every shadow block left the atlas as it was, \
                 so the hook reaches nothing the depth pass reads"
            ));
        }

        for field in &colour_only {
            let tampered = draw_atlas(&headless, path, None, Some(field.perturb));
            if !tampered.redrew_everything {
                failures.push(format!(
                    "{path:?}: the frame with `{}` overwritten held a map",
                    field.name
                ));
            }
            let changed = differing(&baseline.atlas, &tampered.atlas);
            if changed != 0 {
                failures.push(format!(
                    "{path:?}: overwriting `{}` in every shadow block moved {changed} texels of \
                     the atlas: the depth pass reads it, so `depth_pass_reads` must keep it and \
                     `FRAME_UNIFORMS_READERS` must call it a depth-pass field",
                    field.name
                ));
            }
        }
        for (name, drawn) in [
            ("untampered", &baseline),
            ("repeated", &again),
            ("opaque", &opaque),
            ("view_proj", &control),
        ] {
            if !drawn.redrew_everything {
                failures.push(format!("{path:?}: the {name} frame held a map"));
            }
        }
    }
    eprintln!(
        "{}: {} colour-only field(s) held to the atlas on {paths:?}",
        crate::SUITE,
        colour_only.len()
    );
    headless.finish();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
