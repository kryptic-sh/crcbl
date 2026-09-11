//! Material page sampling must select the uploaded mip chain under minification.
//!
//! A balanced black/white checker becomes uniform from mip 3 onwards. The
//! minified quad covers about 24 source texels per framebuffer pixel, selecting
//! those uniform levels. Compare its HDR output with an independently authored
//! uniform layer, then magnify the same checker to prove the page was sampled
//! and its level zero really contains contrast. White-only page tests cannot
//! distinguish correct gradients from accidentally sampling level zero.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh_lit};
use crate::vertex_v2::{
    ORTHO_HALF_HEIGHT, QUAD_HALF, flat_frame, pixel_at, quad_camera, quad_mesh,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{Capacities, PageDesc, PageKind, ProbeGrid, SceneDesc};
use crcbl::render::{
    Antialiasing, DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc,
    RenderEffects, TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh};

const EXTENT: u32 = 256;
const CELL: u32 = 4;
const MINIFIED_SPAN: f32 = 7.3;
const MAGNIFIED_SPAN: f32 = 1.0 / 16.0;

fn texel(kind: PageKind, value: u8) -> [u8; 4] {
    match kind {
        // Only occlusion varies: ambient-only lighting makes its contribution
        // linear and leaves roughness and metallic out of this comparison.
        PageKind::MetallicRoughnessOcclusion => [value, 255, 0, 255],
        _ => [value, value, value, 255],
    }
}

fn scene(kind: PageKind, checker: bool, span: f32) -> SceneDesc<'static> {
    // Average decoded black and white in linear space: (0 + 1) / 2.
    // Encoding 0.5 gives round(187.516) = 188 in sRGB, or round(127.5)
    // = 128 in UNORM. Do not average the encoded bytes for colour pages.
    let average = match kind {
        PageKind::MetallicRoughnessOcclusion => 128,
        _ => 188,
    };
    let mut bytes = Vec::with_capacity((EXTENT * EXTENT * 4) as usize);
    for y in 0..EXTENT {
        for x in 0..EXTENT {
            let value = if checker {
                if (x / CELL + y / CELL).is_multiple_of(2) {
                    0
                } else {
                    255
                }
            } else {
                average
            };
            bytes.extend_from_slice(&texel(kind, value));
        }
    }
    let mut page = PageDesc::empty();
    page.set_extent(kind, EXTENT);
    let layer = page.push_layer(kind, bytes);
    let mut material = GpuMaterial {
        base_color: [0.75, 0.5, 0.25, 1.0],
        metallic: 0.0,
        roughness: 1.0,
        ..GpuMaterial::UNTINTED
    };
    match kind {
        PageKind::BaseColor => material.base_color_texture = layer,
        PageKind::MetallicRoughnessOcclusion => {
            material.metallic_roughness_occlusion_texture = layer;
        }
        PageKind::Emissive => {
            material.emissive_texture = layer;
            material.emissive = [0.75, 0.5, 0.25];
        }
        _ => unreachable!("only scalar material pages are tested here"),
    }
    SceneDesc {
        meshes: vec![quad_mesh(
            "material mip quad",
            flat_frame(),
            [[0.0, 0.0], [span, 0.0], [span, span], [0.0, span]],
            GpuMesh::MESH_AUTHORED_TANGENTS,
        )],
        materials: vec![material],
        page,
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

fn frame(kind: PageKind, checker: bool, span: f32) -> HdrTarget {
    let description = scene(kind, checker, span);
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::with_scene(device, headless.queue, headless.format, &description)
            .expect("material mip scene");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(Antialiasing::SLOT, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::SHADOWS, Some(false))
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(false)),
        ..EffectRequest::default()
    });
    renderer
        .add_instance(&InstanceDesc {
            mesh: 0,
            material: 0,
            transform: Mat4::IDENTITY,
        })
        .expect("one quad");
    let light = DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::ZERO,
        ambient: if kind == PageKind::Emissive {
            Vec3::ZERO
        } else {
            Vec3::splat(0.5)
        },
    };
    let mut hdr = Vec::new();
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &quad_camera(),
        &light,
        Some(&mut hdr),
    );
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn minified_material_pages_match_their_linear_average() {
    let quad_pixels = MESH_EXTENT.1 as f32 * QUAD_HALF / ORTHO_HALF_HEIGHT;
    let texels_per_pixel = EXTENT as f32 * MINIFIED_SPAN / quad_pixels;
    assert!(
        texels_per_pixel >= (CELL * 2) as f32,
        "minified footprint {texels_per_pixel} must select an already uniform mip"
    );
    assert!(
        EXTENT as f32 * MAGNIFIED_SPAN / quad_pixels < 1.0,
        "the negative control must magnify level zero"
    );
    for kind in [
        PageKind::BaseColor,
        PageKind::MetallicRoughnessOcclusion,
        PageKind::Emissive,
    ] {
        let minified = frame(kind, true, MINIFIED_SPAN);
        let reference = frame(kind, false, MINIFIED_SPAN);
        let magnified = frame(kind, true, MAGNIFIED_SPAN);
        let mut max_error = 0.0_f32;
        let mut magnified_low = f32::INFINITY;
        let mut magnified_high = f32::NEG_INFINITY;
        let mut control_difference = 0.0_f32;
        // All points are well inside the quad; compare individual samples,
        // never a spatial average that could conceal a level-zero checker.
        for y in -4..=4 {
            for x in -4..=4 {
                let (px, py) = pixel_at(x as f32 * 0.07, y as f32 * 0.07);
                let got = minified.pixel(px, py);
                let want = reference.pixel(px, py);
                let control = magnified.pixel(px, py);
                for channel in 0..3 {
                    assert!(want[channel] > 0.02, "{kind:?}: blank reference {want:?}");
                    // 1% permits texture decode/filter and half-float rounding;
                    // a wrongly selected level zero has order-one contrast.
                    let error = (got[channel] - want[channel]).abs() / want[channel];
                    assert!(
                        error <= 0.01,
                        "{kind:?}: minified ({px}, {py}) channel {channel}: {got:?}, \
                         reference {want:?}, relative error {error}"
                    );
                    max_error = max_error.max(error);
                    assert!(control[channel].is_finite(), "{kind:?}: {control:?}");
                }
                magnified_low = magnified_low.min(control[0]);
                magnified_high = magnified_high.max(control[0]);
                control_difference = control_difference.max((control[0] - want[0]).abs() / want[0]);
            }
        }
        assert!(
            magnified_high > 0.1 && magnified_low < magnified_high * 0.25,
            "{kind:?}: level-zero control lacks checker contrast: {magnified_low}..{magnified_high}"
        );
        assert!(
            control_difference > 0.5,
            "{kind:?}: magnified checker did not differ from the flat reference: {control_difference}"
        );
        eprintln!(
            "{}: {kind:?} minified max relative error {max_error}; magnified red \
             {magnified_low}..{magnified_high}, reference separation {control_difference}",
            crate::SUITE
        );
    }
}
