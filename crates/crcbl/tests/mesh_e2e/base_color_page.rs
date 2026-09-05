//! **A material naming no base-colour page shades exactly as one naming a
//! white layer** — `docs/plan/43-render-standards.md` §2's row (d), measured on
//! a device rather than argued from the sampler's specification.
//!
//! # Why this test exists
//!
//! Row (d) takes `GpuMaterial::NO_PAGE` out of band and has the fragment stage
//! multiply by the literal `float4(1, 1, 1, 1)` where it used to sample layer 0
//! of the page — a layer every producer in this tree had to remember to fill
//! with `0xFF`. The whole claim that no golden moves rests on those two routes
//! being the *same* number: `PageDesc::WHITE` is `0xFF`, the page is
//! `Rgba8UnormSrgb`, and `0xFF` is the one value that encoding leaves alone, so
//! an all-white layer is supposed to return exactly `1.0` at every tap of every
//! mip level and the product is supposed to be bit-identical.
//!
//! That was prose in `PageDesc::WHITE`'s docs and nothing measured it. Trilinear
//! filtering, anisotropy and an sRGB decode are three places a device could
//! return `0.99999994` instead, and the difference is invisible in a picture and
//! fatal to a byte-compared golden. So this file draws one quad twice — once
//! through a row carrying `NO_PAGE`, once through a row naming a layer of white
//! texels, identical in every other column — and compares the two
//! `Rgba16Float` texels **bit for bit**.
//!
//! It is the one test of row (d) that could be written before any layer index
//! moved, which is why it is a file of its own rather than a case inside
//! [`normal_map`](crate::normal_map): if it goes red, the untextured product was
//! never exactly `1.0`, every untextured golden in the tree has to be
//! re-blessed, and `PageDesc::WHITE`'s paragraph is wrong.
//!
//! # What the frame is
//!
//! [`vertex_v2`](crate::vertex_v2)'s flat quad under an orthographic camera,
//! lit by a sun along `+Z` — straight at the quad's own normal, so `N·L` is one
//! and the quad is fully lit. Every effect that could put a neighbouring texel
//! or a second term into the reading is forced off, for
//! [`normal_map`](crate::normal_map)'s reasons exactly.
//!
//! What the reading is *worth* does not matter here and is nobody's claim: the
//! two frames run the same lobe over the same geometry under the same light, so
//! the only thing that can move between them is the base-colour texel. What the
//! test needs of the value is that it is lit and that the row's factor is in
//! it, or two black texels would compare equal however wrong either route was.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh_lit};
use crate::vertex_v2::{flat_frame, pixel_at, quad_camera, quad_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{Capacities, PAGE_EXTENT, PageDesc, PageKind, ProbeGrid, SceneDesc};
use crcbl::render::{
    Camera, DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc,
    RenderEffects, TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh};

/// The UVs the quad carries: `u` with world `+X`, in `QUAD_CORNERS`' order.
///
/// The whole `0..1` square, so the sampler reads the page across its full
/// extent and a layer that were white in only some of its texels could not pass.
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The layer [`page`] pushes a full square of [`PageDesc::WHITE`] into.
///
/// **Zero, and layer zero is an ordinary layer.** Nothing is burned ahead of it:
/// a caller that wants a white layer writes one, and the claim here is about
/// that layer rather than about a convention. Row (d)'s second step is what
/// removed the burned one, and this test says the same thing either side of it.
const WHITE_LAYER: u32 = 0;

/// The row that names no base-colour page at all.
const NO_PAGE_ROW: usize = 0;

/// The row that names [`WHITE_LAYER`], and is [`NO_PAGE_ROW`] in every other
/// column.
const WHITE_ROW: usize = 1;

/// The factor both rows carry.
///
/// Three different channels and none of them one, so a product that dropped the
/// factor, swapped two channels or multiplied by a texel that is not white
/// reads differently in a way one texel can show. Below one in every channel, so
/// nothing here is measuring the tonemap's shoulder.
const TINT: [f32; 4] = [0.75, 0.5, 0.25, 1.0];

/// The sun: straight along `+Z`, at the quad's own normal, with no ambient term.
///
/// [`DirectionalLight::direction`] points **towards** the light, and the quad
/// faces `+Z`, so `N·L` is exactly one and the surface is as lit as it gets. An
/// ambient term would add a constant to both readings and weaken a comparison
/// that is meant to be exact.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::splat(1.0),
        ambient: Vec3::ZERO,
    }
}

/// The page: one layer, [`WHITE_LAYER`], every texel [`PageDesc::WHITE`].
fn page() -> PageDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::BaseColor, PAGE_EXTENT);
    let texels = PAGE_EXTENT as usize * PAGE_EXTENT as usize * 4;
    let white = page.push_layer(PageKind::BaseColor, vec![PageDesc::WHITE; texels]);
    assert_eq!(white, WHITE_LAYER, "the white layer is the page's only one");
    page
}

/// The description both frames are drawn from: one quad and the two rows.
fn scene() -> SceneDesc<'static> {
    let rows = vec![
        GpuMaterial {
            base_color: TINT,
            base_color_texture: GpuMaterial::NO_PAGE,
            ..GpuMaterial::UNTINTED
        },
        GpuMaterial {
            base_color: TINT,
            base_color_texture: WHITE_LAYER,
            ..GpuMaterial::UNTINTED
        },
    ];
    // The two rows differ in the page column and in nothing else, which is what
    // makes the comparison below evidence about that column. A `..` spread that
    // had picked up a second difference would make it evidence about two.
    assert_eq!(
        GpuMaterial {
            base_color_texture: rows[WHITE_ROW].base_color_texture,
            ..rows[NO_PAGE_ROW]
        },
        rows[WHITE_ROW],
        "the two rows must differ in the base-colour page column alone"
    );
    SceneDesc {
        meshes: vec![quad_mesh(
            "white quad",
            flat_frame(),
            UVS,
            GpuMesh::MESH_AUTHORED_TANGENTS,
        )],
        materials: rows,
        page: page(),
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

/// One frame of `scene` with the quad drawn under material row `material`,
/// answered as the raw `Rgba16Float` target.
fn lit_frame(description: &SceneDesc<'_>, material: usize) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::with_scene(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        description,
    )
    .expect("the forward renderer builds this description");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::SHADOWS, Some(false))
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(false)),
        ..EffectRequest::default()
    });
    renderer
        .add_instance(&InstanceDesc {
            mesh: 0,
            material,
            transform: Mat4::IDENTITY,
        })
        .expect("an instance pool of thousands has room for one quad");
    let mut hdr = Vec::new();
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &camera(),
        &sun(),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// [`quad_camera`], named here so the module reads without the import spelled at
/// the call site.
fn camera() -> Camera {
    quad_camera()
}

/// The texel at the middle of the quad, well inside it on both axes so nothing
/// read is a partly covered pixel.
fn centre_texel() -> (u32, u32) {
    let (x, y) = pixel_at(0.0, 0.0);
    let (width, height) = MESH_EXTENT;
    assert!(x < width && y < height, "the centre texel is in the frame");
    (x, y)
}

/// The eight bytes one `Rgba16Float` texel occupies in the copied target.
fn texel_bytes(frame: &HdrTarget, x: u32, y: u32) -> [u8; 8] {
    let index = ((y * MESH_EXTENT.0 + x) * 4) as usize * 2;
    frame.0[index..index + 8]
        .try_into()
        .expect("four half channels")
}

/// **A row naming no page shades exactly as a row naming a white layer.**
///
/// The two frames differ in one material column and nothing else, and the texel
/// at the quad's centre has to be the *same bits* in both: the shader's
/// `base_color_texel` returns the literal `1.0` for the first and an all-`0xFF`
/// sRGB tap for the second, and row (d)'s whole "no golden moves" argument is
/// that those are one number.
///
/// The reading is also asserted to be lit and to carry the factor, because two
/// black texels would compare equal however wrong either route was.
///
/// # Sabotage
///
/// `base_color_texel` in `shaders/mesh.slang` returning
/// `float4(0.999, 0.999, 0.999, 1.0)` for the unnamed case, artifacts
/// regenerated. Red on radv on 2026-09-06 with
/// `"a material naming no page must shade exactly as one naming an all-white
/// layer, or every golden in the tree drawn without a texture moves: no page
/// read [0.90966797, 0.65966797, 0.41015625, 1.0] and the white layer read
/// [0.91015625, 0.66015625, 0.4104004, 1.0]"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_row_naming_no_page_shades_exactly_as_a_white_layer_does() {
    let description = scene();
    let (x, y) = centre_texel();

    let no_page = lit_frame(&description, NO_PAGE_ROW);
    let white = lit_frame(&description, WHITE_ROW);

    // Lit at all, and carrying the row's tint: two black texels would compare
    // equal below however wrong either route was, and a product that had lost
    // the factor would read the same in all three channels. Not an equality
    // against `TINT` — the lobe adds a specular term on top of the diffuse
    // product, and this file is not the place that pins the shading model.
    let read = no_page.pixel(x, y);
    for (channel, value) in read.iter().enumerate().take(3) {
        assert!(
            *value > 0.0,
            "channel {channel} of the untextured quad read {value}, and a black quad makes \
             the comparison below vacuous"
        );
    }
    assert!(
        read[0] > read[1] && read[1] > read[2],
        "the untextured quad read {read:?}, which does not descend the way its factor \
         {TINT:?} does — the row's colour has to be in the product, or the comparison \
         below is about a white surface"
    );

    assert_eq!(
        texel_bytes(&no_page, x, y),
        texel_bytes(&white, x, y),
        "a material naming no page must shade exactly as one naming an all-white layer, or \
         every golden in the tree drawn without a texture moves: no page read {:?} and the \
         white layer read {:?}",
        no_page.pixel(x, y),
        white.pixel(x, y)
    );

    eprintln!(
        "{}: no page and an all-white layer both read {:?} at ({x}, {y})",
        crate::SUITE,
        no_page.pixel(x, y)
    );
}
