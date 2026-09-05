//! **The emissive page reaches the shading, and it is a factor over the row's
//! own radiance** — `docs/plan/43-render-standards.md` §2's rung 3, measured on
//! a device.
//!
//! # Why this file exists
//!
//! [`PageKind::Emissive`] is glTF 2.0 §3.9.4's `emissiveTexture`: an
//! sRGB-encoded colour multiplied by `emissiveFactor`, added to the shaded
//! radiance and scaled by nothing else. The factor half shipped on 2026-08-27
//! and `GpuMaterial::emissive` has carried it since; this is the page half.
//!
//! Two claims, and they fail for different reasons.
//!
//! * **The page selects where a surface emits.** One quad whose emissive layer
//!   is black on the left column and white on the right, with every light
//!   turned off, so the whole frame is the emission and nothing else: the right
//!   half is lit and the left is exactly black. A shader that added the factor
//!   without the texel would light both halves alike.
//! * **And an all-white layer is the identity.** A row naming a layer of
//!   `0xFF` reads bit for bit as a row naming no page at all, at the same
//!   factor — `0xFF` is the one value the sRGB encoding leaves alone, so the
//!   product is exactly the factor. Every golden in the tree is drawn through
//!   rows carrying `NO_PAGE`, and that claim is what says none of them moves.
//!
//! # What the frame is
//!
//! [`vertex_v2`](crate::vertex_v2)'s flat quad under the orthographic camera,
//! read at the frame texels nearest the page's two column centres —
//! [`mro_page`](crate::mro_page)'s arrangement, whose header argues the
//! geometry and the bilinear tap. Every light is off and every effect that
//! could add a term is forced off, so a reading is the emission alone.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh_lit};
use crate::vertex_v2::{QUAD_HALF, flat_frame, pixel_at, quad_camera, quad_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{Capacities, PAGE_EXTENT, PageDesc, PageKind, ProbeGrid, SceneDesc};
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc, RenderEffects,
    TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh};

/// The UVs the quad carries: `u` with world `+X`, in `QUAD_CORNERS`' order.
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The layer that is black on the left column and white on the right.
const SPLIT_LAYER: u32 = 0;

/// The all-white layer, which is the identity of the product this page makes.
const WHITE_LAYER: u32 = 1;

/// The row that names no emissive page.
const NO_PAGE_ROW: usize = 0;

/// The row that names [`SPLIT_LAYER`].
const SPLIT_ROW: usize = 1;

/// The row that names [`WHITE_LAYER`], and is [`NO_PAGE_ROW`] in every other
/// column.
const WHITE_ROW: usize = 2;

/// The linear radiance every row here emits.
///
/// Three different channels and none of them one, so a product that dropped the
/// factor, swapped two channels or multiplied by a texel that is not white
/// reads differently in a way one texel shows. Above one in its first channel,
/// because that is what a lamp is and what the `Rgba16Float` target exists to
/// carry — and the tonemap is not in the reading, since these tests read the
/// scene target directly.
const RADIANCE: [f32; 3] = [1.5, 0.75, 0.25];

/// The page: the split layer, then the white one.
fn page() -> PageDesc<'static> {
    assert_eq!(
        PAGE_EXTENT, 2,
        "this fixture's halves are the page's two columns"
    );
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::Emissive, PAGE_EXTENT);
    let mut split = Vec::new();
    for _ in 0..PAGE_EXTENT {
        split.extend_from_slice(&[0x00, 0x00, 0x00, 0xFF]);
        split.extend_from_slice(&[0xFF; 4]);
    }
    assert_eq!(page.push_layer(PageKind::Emissive, split), SPLIT_LAYER);
    assert_eq!(
        page.push_layer(
            PageKind::Emissive,
            vec![0xFF; PAGE_EXTENT as usize * PAGE_EXTENT as usize * 4]
        ),
        WHITE_LAYER
    );
    page
}

/// The description every frame here draws: one quad and the three rows.
///
/// `base_color` is black, so nothing a light could do to this surface reaches
/// the reading even if one were on — the emission is the whole of it, which is
/// what makes the left half's "exactly zero" a claim about the page rather than
/// about how dark the lighting happened to be.
fn scene() -> SceneDesc<'static> {
    let row = |layer: u32| GpuMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        emissive: RADIANCE,
        emissive_texture: layer,
        ..GpuMaterial::UNTINTED
    };
    let rows = vec![
        row(GpuMaterial::NO_PAGE),
        row(SPLIT_LAYER),
        row(WHITE_LAYER),
    ];
    assert_eq!(
        GpuMaterial {
            emissive_texture: rows[WHITE_ROW].emissive_texture,
            ..rows[NO_PAGE_ROW]
        },
        rows[WHITE_ROW],
        "the two control rows must differ in the emissive page column alone"
    );
    assert_eq!(rows[NO_PAGE_ROW].emissive_texture, GpuMaterial::NO_PAGE);
    SceneDesc {
        meshes: vec![quad_mesh(
            "emissive quad",
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

/// No light at all: a black sun and no ambient, so every term but the emission
/// is exactly zero and a reading is the page's own product.
fn unlit() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::ZERO,
        ambient: Vec3::ZERO,
    }
}

/// One frame of `description` with the quad drawn under material row
/// `material`, answered as the raw `Rgba16Float` target.
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
        &quad_camera(),
        &unlit(),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The frame texel nearest the centre of page column `column`, on the quad's
/// centre row — [`mro_page`](crate::mro_page)'s arrangement.
fn column_texel(column: u32) -> (u32, u32) {
    let u = (column as f32 + 0.5) / PAGE_EXTENT as f32;
    let (x, y) = pixel_at((u * 2.0 - 1.0) * QUAD_HALF, 0.0);
    let (width, height) = MESH_EXTENT;
    assert!(x < width && y < height, "column {column} is in the frame");
    (x, y)
}

/// The eight bytes one `Rgba16Float` texel occupies in the copied target.
fn texel_bytes(frame: &HdrTarget, at: (u32, u32)) -> [u8; 8] {
    let index = ((at.1 * MESH_EXTENT.0 + at.0) * 4) as usize * 2;
    frame.0[index..index + 8]
        .try_into()
        .expect("four half channels")
}

/// **A black emissive texel emits nothing and a white one emits the row's whole
/// radiance**, with every light off.
///
/// The two halves of one quad under one material row, so the only thing that
/// differs between the readings is the texel. The dark half is asserted to be
/// *dark* rather than exactly black, because the bilinear tap at a pixel centre
/// carries a per cent of the white column — see
/// [`mro_page`](crate::mro_page)'s header, which measures that fraction.
///
/// **And the lit half carries the factor**, channel for channel: a product that
/// had lost the row's radiance would read white where this reads
/// [`RADIANCE`]'s own descent.
///
/// # Sweep
///
/// Measured 2026-09-06 before [`DARK_CEILING`] and [`BRIGHT_FLOOR`] were fixed.
/// On radv the black half reads `[0.01171875, 0.005859375, 0.001953125]` and
/// the white half `[1.4882813, 0.7441406, 0.24804688]` against a factor of
/// [`RADIANCE`]; on lavapipe `[0.011711121, 0.0058555603, 0.0019521713]` and
/// `[1.4873047, 0.74365234, 0.2479248]`. That is a share of `0.0078` and
/// `0.992` of the factor either way — the bilinear tap the
/// [`mro_page`](crate::mro_page) header measures, and nothing else.
///
/// # Sabotage
///
/// `lit += emissive_of(material) * emitted.rgb` in `shaders/mesh.slang` cut
/// back to `lit += emissive_of(material)`, artifacts regenerated. Red on radv on
/// 2026-09-06 with `"channel 0 of the black half read 1.5 against a factor of
/// 1.5; a shader that added the factor without the texel would light both
/// halves alike"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_black_emissive_texel_emits_nothing_and_a_white_one_emits_the_factor() {
    let description = scene();
    let split = lit_frame(&description, SPLIT_ROW);
    let dark = split.pixel(column_texel(0).0, column_texel(0).1);
    let bright = split.pixel(column_texel(1).0, column_texel(1).1);
    eprintln!(
        "{}: the emissive split reads dark {dark:?} at {:?} and bright {bright:?} at {:?}",
        crate::SUITE,
        column_texel(0),
        column_texel(1)
    );

    for (channel, emitted) in RADIANCE.iter().enumerate() {
        assert!(
            dark[channel] < *emitted * DARK_CEILING,
            "channel {channel} of the black half read {} against a factor of {emitted}; a \
             shader that added the factor without the texel would light both halves alike",
            dark[channel]
        );
        assert!(
            bright[channel] > *emitted * BRIGHT_FLOOR,
            "channel {channel} of the white half read {} against a factor of {emitted}; an \
             all-white texel is exactly 1.0 through the sRGB encoding, so the product is the \
             factor",
            bright[channel]
        );
    }
    assert!(
        bright[0] > bright[1] && bright[1] > bright[2],
        "the lit half read {bright:?}, which does not descend the way its factor {RADIANCE:?} \
         does — the row's radiance has to be in the product"
    );
}

/// The share of the factor the black half must stay under. Swept on both
/// drivers before it was fixed; the bilinear tap is what keeps it off zero.
const DARK_CEILING: f32 = 0.05;

/// The share of the factor the white half must reach. Swept on both drivers
/// before it was fixed; it is under one because of the same tap, which carries a
/// per cent of the black column into this reading.
const BRIGHT_FLOOR: f32 = 0.95;

/// **A row naming an all-white emissive layer emits exactly what a row naming
/// no page emits**, at the same factor.
///
/// `0xFF` on an `Rgba8UnormSrgb` channel decodes to exactly `1.0`, so
/// `emissive_texel`'s literal `float4(1, 1, 1, 1)` for the unnamed case and a
/// tap of an all-white layer are supposed to be one number — and every golden in
/// the tree is drawn through rows carrying `NO_PAGE`. Compared bit for bit at
/// both readings, on
/// [`base_color_page`](crate::base_color_page)'s terms exactly.
///
/// # Sabotage
///
/// `emissive_texel` in `shaders/mesh.slang` returning
/// `float4(0.999, 0.999, 0.999, 1.0)` for the unnamed case, artifacts
/// regenerated. Red on radv on 2026-09-06 with `"a row naming no emissive page
/// must emit exactly what one naming an all-white layer emits, or every golden
/// in the tree moves: no page read [1.4980469, 0.74902344, 0.24963379, 1.0] and
/// the white layer read [1.5, 0.75, 0.25, 1.0] at (108, 96)"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_all_white_emissive_layer_emits_exactly_what_no_page_does() {
    let description = scene();
    let none = lit_frame(&description, NO_PAGE_ROW);
    let white = lit_frame(&description, WHITE_ROW);
    for column in 0..PAGE_EXTENT {
        let at = column_texel(column);
        assert_eq!(
            texel_bytes(&none, at),
            texel_bytes(&white, at),
            "a row naming no emissive page must emit exactly what one naming an all-white \
             layer emits, or every golden in the tree moves: no page read {:?} and the white \
             layer read {:?} at {at:?}",
            none.pixel(at.0, at.1),
            white.pixel(at.0, at.1)
        );
    }
    let read = none.pixel(column_texel(0).0, column_texel(0).1);
    eprintln!(
        "{}: no page and an all-white layer both read {read:?}",
        crate::SUITE
    );
    assert!(
        read[0] > 0.0 && read[1] > 0.0 && read[2] > 0.0,
        "the unlit quad read {read:?}, and three black channels would make the comparison \
         above vacuous"
    );
}
