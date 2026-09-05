//! **The packed metallic-roughness-occlusion page reaches the shading, one
//! channel at a time** — `docs/plan/43-render-standards.md` §2's rung 3,
//! measured on a device.
//!
//! # Why this file exists
//!
//! [`PageKind::MetallicRoughnessOcclusion`] is glTF's own packing: occlusion in
//! `r`, roughness in `g`, metallic in `b`. Each multiplies the material row's
//! own factor rather than replacing it, and each reaches a *different* part of
//! the lighting equation — roughness the lobe's width, metallic the split
//! between the diffuse and specular halves, occlusion the indirect terms and
//! only those. A shader that read the three channels in the wrong order, or
//! applied occlusion to the direct term, would draw a plausible frame and pass
//! every host test in the tree.
//!
//! So each case below is a **split layer**: two texels wide, one value on the
//! left column and another on the right, drawn as one quad under one light. The
//! two readings differ in that one channel and in nothing else — the same
//! material row, the same geometry, the same lobe — which is what makes a
//! difference between them evidence about the channel rather than about the
//! frame.
//!
//! # The two halves are the same fragment but for the texel
//!
//! [`vertex_v2`](crate::vertex_v2)'s flat quad under the orthographic camera,
//! read at the frame texels nearest the page's two column centres. `to_eye` is
//! built from the camera's *position* rather than from a direction, so it is not
//! constant across the quad; the two readings sit symmetrically about the
//! quad's centre on the axis the camera is centred on, so `N·V` and `N·H` are
//! the same at both and the lobe is the same function of the material at each.
//!
//! **A column centre is not exactly a pixel centre**, so each reading carries a
//! little under one per cent of the other column through the bilinear filter —
//! [`bleed`] is that fraction, computed rather than assumed, and it is why the
//! occluded half below is asserted to be *dark* rather than to be exactly zero.
//!
//! Every effect that could put a neighbouring texel or a second term into a
//! reading is forced off, for [`normal_map`](crate::normal_map)'s reasons.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh_lit};
use crate::vertex_v2::{
    ORTHO_HALF_HEIGHT, QUAD_HALF, flat_frame, pixel_at, quad_camera, quad_mesh,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{Capacities, PAGE_EXTENT, PageDesc, PageKind, ProbeGrid, SceneDesc};
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc, RenderEffects,
    TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh};

/// The UVs the quad carries: `u` with world `+X`, in `QUAD_CORNERS`' order.
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The `g` a smooth half is authored with: `0x10`, which decodes to about
/// `0.063` and is above `mesh.slang`'s `MIN_ROUGHNESS` of `0.045` — so what the
/// lobe takes is the texel's own value rather than the clamp's, and the
/// comparison below is about the page.
const SMOOTH_G: u8 = 0x10;

/// The `g` a rough half is authored with: `0xF0`, about `0.941`.
const ROUGH_G: u8 = 0xF0;

/// The `b` a dielectric half is authored with: no metal at all, so the surface
/// keeps its whole Lambert term.
const DIELECTRIC_B: u8 = 0x00;

/// The `b` a conductor half is authored with: `0xFF`, which an `Rgba8Unorm`
/// channel decodes to exactly `1.0`, so the row's factor survives the product
/// untouched and the diffuse albedo goes to zero.
const CONDUCTOR_B: u8 = 0xFF;

/// The `r` an unoccluded half is authored with: `0xFF`, exactly `1.0`.
const OPEN_R: u8 = 0xFF;

/// The `r` a fully occluded half is authored with: none of the room reaches it,
/// so the indirect terms are multiplied by zero.
const CLOSED_R: u8 = 0x00;

/// The `a` every layer here carries. Unread by the shading — glTF packs nothing
/// in the fourth channel of this texture — and opaque so nothing about the
/// fixture depends on that.
const UNUSED_A: u8 = 0xFF;

/// The roughness ramp: smooth on the left column, rough on the right, and a
/// **conductor** on both.
///
/// Conductor because a dielectric's Lambert term is more than ten times its
/// specular lobe here, and the lobe is the only thing roughness moves — the two
/// halves of a dielectric ramp read within seven per cent of each other, which
/// is a test that could pass with the channel unread. A metal has no diffuse
/// term at all, so the whole reading is the lobe.
const RAMP_LAYER: u32 = 0;

/// The metallic split: dielectric on the left column, conductor on the right,
/// fully rough on both so the lobe is small and the diffuse term is what
/// separates them.
const METALLIC_LAYER: u32 = 1;

/// The occlusion split: fully occluded on the left column, open on the right,
/// and a **dielectric** on both — a conductor has no ambient term to occlude,
/// because the ambient scales the diffuse albedo.
const OCCLUSION_LAYER: u32 = 2;

/// [`OCCLUSION_LAYER`]'s **open** value everywhere: the control that isolates
/// the red channel, since it differs from that layer's left column in `r` and
/// in nothing else.
const OPEN_LAYER: u32 = 3;

/// An all-`0xFF` layer, which is the identity of all three products the shading
/// makes out of this page.
const NEUTRAL_LAYER: u32 = 4;

/// The row that names no packed page at all.
const NO_PAGE_ROW: usize = 0;

/// The row that names [`RAMP_LAYER`].
const RAMP_ROW: usize = 1;

/// The row that names [`METALLIC_LAYER`].
const METALLIC_ROW: usize = 2;

/// The row that names [`OCCLUSION_LAYER`].
const OCCLUSION_ROW: usize = 3;

/// The row that names [`OPEN_LAYER`].
const OPEN_ROW: usize = 4;

/// The row that names [`NEUTRAL_LAYER`], and is [`NO_PAGE_ROW`] in every other
/// column.
const NEUTRAL_ROW: usize = 5;

/// The base colour every row here carries.
///
/// Three different channels and none of them one, so a product that dropped a
/// factor or swapped two channels reads differently in a way one texel shows.
const TINT: [f32; 4] = [0.75, 0.5, 0.25, 1.0];

/// The flat ambient [`ambient_only`] lights with. Well below one, so nothing
/// read is near the half-float's coarse range.
const AMBIENT: f32 = 0.5;

/// A two-texel-wide layer: `left` on the `u < 0.5` column, `right` on the
/// other, the same on both rows.
///
/// The rows are equal so the `v` a sample lands at cannot change the reading —
/// the quad's centre row is a blend of both `v` texels, and this is what makes
/// that blend the identity.
fn split_layer(left: [u8; 4], right: [u8; 4]) -> Vec<u8> {
    assert_eq!(
        PAGE_EXTENT, 2,
        "this fixture's halves are the page's two columns"
    );
    let mut texels = Vec::with_capacity(PAGE_EXTENT as usize * PAGE_EXTENT as usize * 4);
    for _ in 0..PAGE_EXTENT {
        texels.extend_from_slice(&left);
        texels.extend_from_slice(&right);
    }
    texels
}

/// One texel repeated over a whole layer.
fn flat_layer(texel: [u8; 4]) -> Vec<u8> {
    texel.repeat(PAGE_EXTENT as usize * PAGE_EXTENT as usize)
}

/// The page: the three split layers, the open control and the neutral one, in
/// the order the `*_LAYER` constants name.
///
/// Every channel a case is not about is held at its identity — `0xFF` — so each
/// case moves one channel and nothing else, bar the deliberate metalness the
/// ramp and the occlusion layers carry for the reasons their constants give.
fn page() -> PageDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::MetallicRoughnessOcclusion, PAGE_EXTENT);
    let mut push = |texels: Vec<u8>| page.push_layer(PageKind::MetallicRoughnessOcclusion, texels);
    assert_eq!(
        push(split_layer(
            [OPEN_R, SMOOTH_G, CONDUCTOR_B, UNUSED_A],
            [OPEN_R, ROUGH_G, CONDUCTOR_B, UNUSED_A],
        )),
        RAMP_LAYER
    );
    assert_eq!(
        push(split_layer(
            [OPEN_R, 0xFF, DIELECTRIC_B, UNUSED_A],
            [OPEN_R, 0xFF, CONDUCTOR_B, UNUSED_A],
        )),
        METALLIC_LAYER
    );
    assert_eq!(
        push(split_layer(
            [CLOSED_R, 0xFF, DIELECTRIC_B, UNUSED_A],
            [OPEN_R, 0xFF, DIELECTRIC_B, UNUSED_A],
        )),
        OCCLUSION_LAYER
    );
    assert_eq!(
        push(flat_layer([OPEN_R, 0xFF, DIELECTRIC_B, UNUSED_A])),
        OPEN_LAYER
    );
    assert_eq!(push(flat_layer([0xFF; 4])), NEUTRAL_LAYER);
    page
}

/// The description every frame here draws: one quad and the six rows.
///
/// **Every factor a page channel multiplies is `1.0`**, so a reading is the
/// texel's own value and the arithmetic under test is one multiply rather than
/// two.
fn scene() -> SceneDesc<'static> {
    let row = |layer: u32| GpuMaterial {
        base_color: TINT,
        metallic: 1.0,
        roughness: 1.0,
        metallic_roughness_occlusion_texture: layer,
        ..GpuMaterial::UNTINTED
    };
    let rows = vec![
        row(GpuMaterial::NO_PAGE),
        row(RAMP_LAYER),
        row(METALLIC_LAYER),
        row(OCCLUSION_LAYER),
        row(OPEN_LAYER),
        row(NEUTRAL_LAYER),
    ];
    // The unnamed row and the neutral row differ in the page column and in
    // nothing else, which is what makes the bit comparison below evidence about
    // that column.
    assert_eq!(
        GpuMaterial {
            metallic_roughness_occlusion_texture: rows[NEUTRAL_ROW]
                .metallic_roughness_occlusion_texture,
            ..rows[NO_PAGE_ROW]
        },
        rows[NEUTRAL_ROW],
        "the two control rows must differ in the packed page column alone"
    );
    assert_eq!(
        rows[NO_PAGE_ROW].metallic_roughness_occlusion_texture,
        GpuMaterial::NO_PAGE
    );
    SceneDesc {
        meshes: vec![quad_mesh(
            "packed quad",
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

/// The sun straight along `+Z`, at the quad's own normal, with no ambient term.
///
/// `N·L` is exactly one and the half-vector is within a fraction of a degree of
/// the normal, which is where a change of roughness moves the lobe most. No
/// ambient, so the direct terms are the whole of what is read.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::splat(1.0),
        ambient: Vec3::ZERO,
    }
}

/// The **ambient** light: no sun at all, and a flat environment term.
///
/// `color` zero, so the loop over this froxel's lights adds exactly nothing and
/// the whole reading is `diffuse_albedo * irradiance * ambient_visibility` —
/// the one product the material's occlusion channel is allowed to touch.
fn ambient_only() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::ZERO,
        ambient: Vec3::splat(AMBIENT),
    }
}

/// One frame of `description` with the quad drawn under material row
/// `material`, answered as the raw `Rgba16Float` target.
fn lit_frame(description: &SceneDesc<'_>, material: usize, light: &DirectionalLight) -> HdrTarget {
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
        light,
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
/// centre row.
fn column_texel(column: u32) -> (u32, u32) {
    let u = (column as f32 + 0.5) / PAGE_EXTENT as f32;
    let (x, y) = pixel_at((u * 2.0 - 1.0) * QUAD_HALF, 0.0);
    let (width, height) = MESH_EXTENT;
    assert!(x < width && y < height, "column {column} is in the frame");
    (x, y)
}

/// The texel over the quad's left half, which samples column 0.
fn left_texel() -> (u32, u32) {
    column_texel(0)
}

/// The texel over the quad's right half, which samples column 1.
fn right_texel() -> (u32, u32) {
    column_texel(1)
}

/// How much of the *other* column a reading at [`column_texel`] carries.
///
/// A pixel centre is not a texel centre, so the bilinear tap at the frame texel
/// nearest a column's centre is a blend. Computed here rather than asserted to
/// be zero, because it is what makes "fully occluded" read as nearly black
/// rather than as exactly black — and a fixture that assumed otherwise would be
/// a test that fails for a reason that is not the shader's.
fn bleed(column: u32) -> f32 {
    let (x, _) = column_texel(column);
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    // The pixel's centre, back through `pixel_at`'s mapping into the quad's own
    // `u`.
    let ndc_x = ((x as f32 + 0.5) / MESH_EXTENT.0 as f32) * 2.0 - 1.0;
    let world_x = ndc_x * ORTHO_HALF_HEIGHT * aspect;
    let u = (world_x + QUAD_HALF) / (2.0 * QUAD_HALF);
    let wanted = (column as f32 + 0.5) / PAGE_EXTENT as f32;
    ((u - wanted) * PAGE_EXTENT as f32).abs()
}

/// The red channel at a texel — under a white light with a tinted base colour,
/// the channel the tint is largest in.
fn red_at(frame: &HdrTarget, at: (u32, u32)) -> f32 {
    frame.pixel(at.0, at.1)[0]
}

/// The eight bytes one `Rgba16Float` texel occupies in the copied target.
fn texel_bytes(frame: &HdrTarget, at: (u32, u32)) -> [u8; 8] {
    let index = ((at.1 * MESH_EXTENT.0 + at.0) * 4) as usize * 2;
    frame.0[index..index + 8]
        .try_into()
        .expect("four half channels")
}

/// **A column centre is within a per cent of a pixel centre**, which is the
/// premise every reading in this file rests on.
///
/// Asserted rather than described: a frame extent or a quad half that moved
/// would slide the two readings towards the middle of the quad, and every
/// comparison below would weaken silently rather than fail.
#[test]
fn each_column_is_read_within_a_per_cent_of_its_own_centre() {
    for column in 0..PAGE_EXTENT {
        let carried = bleed(column);
        assert!(
            carried < MAX_BLEED,
            "column {column} is read at {:?}, which carries {carried} of the other column — \
             the fixture's halves are no longer separated",
            column_texel(column)
        );
    }
    assert_ne!(left_texel(), right_texel());
}

/// The most of the other column a reading may carry.
///
/// **Measured before it was fixed**: at [`MESH_EXTENT`] and [`QUAD_HALF`] both
/// columns carry `0.0078` of the other, which is where the occluded half's
/// `0.0029` against the open half's `0.372` comes from. This bound is a little
/// over it, so a frame extent or a quad that moved the readings towards the
/// middle of the quad fails here rather than weakening every comparison in the
/// file silently.
const MAX_BLEED: f32 = 0.01;

/// **The page's green channel is the roughness, and the two halves of one quad
/// light differently because of it.**
///
/// One quad, one material row, one light: the left half's texel carries
/// [`SMOOTH_G`] and the right's [`ROUGH_G`], so the only thing that differs
/// between the two readings is the width of the GGX lobe. A smooth surface
/// concentrates the sun's reflection and reads far brighter near the peak; a
/// rough one spreads the same energy out.
///
/// **And a row naming no page reads bit for bit as one naming an all-`0xFF`
/// layer**, which is the claim that this page adds nothing to a scene that does
/// not use it: `0xFF` on an `Rgba8Unorm` channel is exactly `1.0`, so all three
/// products are the identity — and every golden in the tree is drawn through
/// rows carrying `NO_PAGE`.
///
/// # Sweep
///
/// Measured 2026-09-06 before [`ROUGHNESS_RATIO`] was fixed. On an RX 7900 XTX
/// under radv the smooth half reads `3.2421875` and the rough half `0.5473633`,
/// a ratio of `5.92`; on lavapipe `3.4199219` and `0.54785156`, a ratio of
/// `6.24`. A page whose green channel never reached the lobe would read the
/// same on both halves, which is a ratio of one.
///
/// # Sabotage
///
/// `mro_layer` in `shaders/mesh.slang` returning `NO_PAGE`, artifacts
/// regenerated. Red on radv on 2026-09-06 with `"the half whose texel carries
/// g=0x10 read 0.49121094 and the half carrying g=0xf0 read 0.49121094; a page
/// whose green channel never reached the lobe would light the two halves
/// alike"`. Red the same day and the same way with the `r` and `g` channels
/// swapped at their two consumers — the roughness taking `mro.r` and the
/// occlusion `mro.g` — which is the mistake the packing invites.
///
/// **And the bit comparison has its own.** `mro_texel` returning
/// `float4(0.999, 0.999, 0.999, 1.0)` for the unnamed case: red on radv the
/// same day with `"a material naming no packed page must shade exactly as one
/// naming an all-0xFF layer, or every golden in the tree moves: no page read
/// [0.49316406, 0.26123047, 0.09680176, 1.0] and the neutral layer read
/// [0.49121094, 0.26000977, 0.09631348, 1.0] at (108, 96)"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_packed_pages_green_channel_widens_the_lobe() {
    let description = scene();
    let ramp = lit_frame(&description, RAMP_ROW, &sun());
    let smooth = red_at(&ramp, left_texel());
    let rough = red_at(&ramp, right_texel());
    eprintln!(
        "{}: roughness ramp reads smooth {smooth} at {:?} and rough {rough} at {:?}",
        crate::SUITE,
        left_texel(),
        right_texel()
    );

    assert!(
        rough > 0.0,
        "the rough half read {rough}, and two dark halves would make the ratio below vacuous"
    );
    assert!(
        smooth > rough * ROUGHNESS_RATIO,
        "the half whose texel carries g={SMOOTH_G:#04x} read {smooth} and the half carrying \
         g={ROUGH_G:#04x} read {rough}; a page whose green channel never reached the lobe \
         would light the two halves alike"
    );

    // The control: no page at all against an all-`0xFF` layer, bit for bit and
    // at both readings.
    let none = lit_frame(&description, NO_PAGE_ROW, &sun());
    let neutral = lit_frame(&description, NEUTRAL_ROW, &sun());
    for at in [left_texel(), right_texel()] {
        assert_eq!(
            texel_bytes(&none, at),
            texel_bytes(&neutral, at),
            "a row naming no packed page must shade exactly as one naming an all-0xFF layer, \
             or every golden in the tree moves: no page read {:?} and the neutral layer read \
             {:?} at {at:?}",
            none.pixel(at.0, at.1),
            neutral.pixel(at.0, at.1)
        );
    }
    assert!(
        red_at(&none, left_texel()) > 0.0,
        "the untextured quad is black, which makes the comparison above vacuous"
    );
}

/// How much brighter the smooth half must be than the rough one.
///
/// **Swept before it was fixed**, on radv and on lavapipe — see the sweep in
/// [`the_packed_pages_green_channel_widens_the_lobe`]'s own docs. This bound is
/// far below the measured ratio, because what the test claims is that the
/// channel *reached* the lobe rather than that the lobe has one exact shape.
const ROUGHNESS_RATIO: f32 = 4.0;

/// **The page's blue channel is the metalness, and the diffuse term vanishes
/// where it is `0xFF`.**
///
/// A conductor scatters nothing: `mesh.slang` scales the diffuse albedo by
/// `1 - metallic`, so the half whose texel carries [`CONDUCTOR_B`] keeps only
/// its specular lobe while the half carrying [`DIELECTRIC_B`] keeps the whole
/// Lambert term. The row's own `metallic` factor is `1.0` in both, so the page
/// is the only thing that separates them.
///
/// # Sweep
///
/// Measured 2026-09-06 before [`LAMBERT_SHARE`] was fixed, and identical on the
/// two drivers: the dielectric half reads `[0.75634766, 0.5078125, 0.25927734]`
/// and the conductor half `[0.49121094, 0.26098633, 0.09741211]`, against a
/// Lambert term of [`TINT`]. So the wall at `0.675` sits twelve per cent under
/// the one and twenty-seven per cent over the other.
///
/// # Sabotage
///
/// `mro_layer` in `shaders/mesh.slang` returning `NO_PAGE`, artifacts
/// regenerated — which leaves the row's own `metallic` factor of `1.0`
/// unmultiplied, so *both* halves are conductors. Red on radv on 2026-09-06
/// with `"the dielectric half read 0.49121094, and its Lambert term alone is
/// 0.75 — the page's blue channel has removed a diffuse lobe that should be
/// there"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_packed_pages_blue_channel_removes_the_diffuse_lobe() {
    let description = scene();
    let split = lit_frame(&description, METALLIC_ROW, &sun());
    let dielectric = red_at(&split, left_texel());
    let conductor = red_at(&split, right_texel());
    eprintln!(
        "{}: metallic split reads dielectric {:?} at {:?} and conductor {:?} at {:?}",
        crate::SUITE,
        split.pixel(left_texel().0, left_texel().1),
        left_texel(),
        split.pixel(right_texel().0, right_texel().1),
        right_texel()
    );

    // **The Lambert term is the wall, and the two halves are on opposite sides
    // of it.** The sun is along the quad's own normal, so `N·L` is one and the
    // diffuse product is the tint's red channel exactly. A surface that kept its
    // diffuse lobe reads *above* that — the specular lobe only adds — and one
    // that lost it reads below, whatever the lobe is worth. So the same number
    // is a floor for one half and a ceiling for the other, and neither
    // assertion depends on what the specular term happens to be.
    let lambert = TINT[0] * LAMBERT_SHARE;
    assert!(
        dielectric >= lambert,
        "the dielectric half read {dielectric}, and its Lambert term alone is {} — the page's \
         blue channel has removed a diffuse lobe that should be there",
        TINT[0]
    );
    assert!(
        conductor < lambert,
        "the half whose texel carries b={CONDUCTOR_B:#04x} read {conductor}, which is above \
         the Lambert term of {} the dielectric half beside it keeps; a conductor has no \
         diffuse lobe, so a page whose blue channel never reached the metalness would light \
         the two alike",
        TINT[0]
    );
}

/// The share of `base_color * N·L` that separates a surface which kept its
/// diffuse lobe from one that did not.
///
/// **Swept before it was fixed** — see the sweep in
/// [`the_packed_pages_blue_channel_removes_the_diffuse_lobe`]'s own docs. Under
/// one is what makes the floor reachable at all, since the bilinear tap carries
/// a per cent of the conductor column into the dielectric reading.
const LAMBERT_SHARE: f32 = 0.9;

/// **The page's red channel is the occlusion, and it scales the indirect terms
/// and only those.**
///
/// Two lights over the same quad and the same split layer.
///
/// * Under [`ambient_only`] — no sun at all — the whole reading is the ambient
///   term, and the half whose texel carries [`CLOSED_R`] is dark where the open
///   half is not. That is glTF 2.0 §3.9.5's rule doing what it says. Dark rather
///   than exactly black because of the bilinear tap the module header describes.
/// * Under [`sun`] — no ambient at all — the occluded half is **bit for bit**
///   what the same pixel reads through [`OPEN_LAYER`], which differs from it in
///   `r` and in nothing else. That is the other half of the same rule: an
///   occlusion channel that leaked into the direct term would darken light the
///   shadow maps have already measured a path for.
///
/// The comparison is between two frames at *one* pixel rather than between the
/// two halves of one frame, and that is what makes it exact: the halves sit at
/// different world positions, so `to_eye` is a different vector at each and only
/// a tolerance could compare them.
///
/// # Sweep
///
/// Measured 2026-09-06 before [`OCCLUSION_CEILING`] was fixed. Under the
/// ambient alone the occluded half reads `0.0029296875` against the open half's
/// `0.3720703` on radv, and `0.0029411316` against `0.37182617` on lavapipe — a
/// share of `0.0079` either way, which is the bilinear tap the module header
/// measures and nothing else. Under the sun alone both frames read
/// `[0.7607422, 0.5107422, 0.2607422, 1.0]` on both drivers.
///
/// # Sabotage
///
/// The `r` and `g` channels swapped at their two consumers in
/// `shaders/mesh.slang` — the occlusion taking `mro.g`, which is `0xFF` on both
/// halves of this layer — artifacts regenerated. Red on radv on 2026-09-06 with
/// `"the half whose texel carries r=0x00 read 0.375 under an ambient-only light
/// against the open half's 0.375; a fully occluded surface takes none of the
/// environment"`.
///
/// `mro_layer` returning `NO_PAGE` reddens it too, one assertion earlier: the
/// row's `metallic` factor of `1.0` then goes unmultiplied, the surface has no
/// diffuse albedo for the ambient to scale, and the guard fires with `"the open
/// half read 0 under the ambient, and two black halves would make the
/// comparison below vacuous"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_packed_pages_red_channel_occludes_the_indirect_terms_alone() {
    let description = scene();

    let ambient = lit_frame(&description, OCCLUSION_ROW, &ambient_only());
    let closed = red_at(&ambient, left_texel());
    let open = red_at(&ambient, right_texel());
    eprintln!(
        "{}: occlusion split under ambient alone reads closed {closed} at {:?} and open \
         {open} at {:?}",
        crate::SUITE,
        left_texel(),
        right_texel()
    );
    assert!(
        open > 0.0,
        "the open half read {open} under the ambient, and two black halves would make the \
         comparison below vacuous"
    );
    assert!(
        closed < open * OCCLUSION_CEILING,
        "the half whose texel carries r={CLOSED_R:#04x} read {closed} under an ambient-only \
         light against the open half's {open}; a fully occluded surface takes none of the \
         environment"
    );

    let occluded_frame = lit_frame(&description, OCCLUSION_ROW, &sun());
    let open_frame = lit_frame(&description, OPEN_ROW, &sun());
    eprintln!(
        "{}: under the sun alone the occluded half reads {:?} and the open control {:?}",
        crate::SUITE,
        occluded_frame.pixel(left_texel().0, left_texel().1),
        open_frame.pixel(left_texel().0, left_texel().1)
    );
    assert_eq!(
        texel_bytes(&occluded_frame, left_texel()),
        texel_bytes(&open_frame, left_texel()),
        "an occluded surface and an open one must shade identically under a light with no \
         ambient term: the occluded half read {:?} and the open control {:?}",
        occluded_frame.pixel(left_texel().0, left_texel().1),
        open_frame.pixel(left_texel().0, left_texel().1)
    );
    assert!(
        red_at(&open_frame, left_texel()) > 0.0,
        "the sun-lit quad is black, which makes the equality above vacuous"
    );
}

/// The share of the open half's ambient reading the occluded half must stay
/// under. Swept on both drivers before it was fixed — the bilinear tap the
/// module header describes is what keeps it off zero.
const OCCLUSION_CEILING: f32 = 0.05;
