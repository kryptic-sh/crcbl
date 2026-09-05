//! **Normal maps on a real device** — `docs/plan/43-render-standards.md` §2's
//! rungs 1 and 2, read back off the shaded frame rather than argued.
//!
//! Every frame here is one flat quad in the `z = 0` plane, facing the camera,
//! lit by a directional light and drawn through
//! [`vertex_v2`](crate::vertex_v2)'s orthographic camera — so a world point maps
//! to a texel by arithmetic and the value read is the one the fragment stage
//! wrote. That file owns the quad, its clusters and its frame helper; this one
//! owns the page and the light.
//!
//! # What each test can actually see
//!
//! A normal map does one thing to a Lambert term: it changes `N·L`. So every
//! claim here is a comparison of two frames that differ in **one** thing, read
//! at the same texel:
//!
//! * a page that tilts every texel towards the sun against the neutral page —
//!   the rung's whole point, and the only test here that would pass on a
//!   renderer that sampled the page and ignored the frame;
//! * the same page under a **left-handed** tangent frame and a mirrored UV, which
//!   is the case `docs/plan/43-render-standards.md` §2 gives as the reason the
//!   vertex route was chosen over the derivative one: the tilt has to land on
//!   the other side, and a renderer that dropped the handedness would draw both
//!   quads identically;
//! * an **unmarked** mesh, whose frame the fragment stage rebuilds out of
//!   `ddx`/`ddy` — which must agree with the marked one to a tolerance this file
//!   measures and states rather than guesses;
//! * a **minified** page whose four texels lean two ways, drawn small enough
//!   that the sampler can only reach the bottom of the page's mip chain — the
//!   one test here about the chain the renderer *builds* rather than about the
//!   frame it applies;
//! * the `Normals` debug view, which is the one reading that is about the vector
//!   itself instead of about what a light did with it.
//!
//! # The sun is the only light, and its ambient is zero
//!
//! [`sun`] points along `+X` with no ambient term, so the value at a texel is
//! `albedo * colour * max(N·L, 0)` and nothing else — see
//! `shaders/mesh.slang`'s accumulation. An ambient term would add a constant to
//! both sides of every comparison below and shrink the ratio the assertions are
//! written in; shadows, occlusion, reflections and antialiasing are forced off
//! for the same reason [`vertex_v2::quad_frame`](crate::vertex_v2::quad_frame)
//! forces two of them off, which is that each puts a neighbouring texel or a
//! second term into the one being read.

use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh_lit};
use crate::vertex_v2::{
    QUAD_CORNERS, QUAD_HALF, QUAD_INDICES, flat_frame, pixel_at, quad_camera, quad_clusters,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{
    Capacities, Geometry, MeshDesc, PAGE_EXTENT, PageDesc, PageKind, ProbeGrid, SceneDesc,
};
use crcbl::render::{
    Camera, DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc,
    Projection, RenderEffects, TransientPool, mip,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh, MeshVertex, vertex_bytes};
use crcbl_shaders::vertex::{TangentFrame, UvRange};
use std::borrow::Cow;

use crate::harness::Headless;

/// How far the page tilts a texel's normal off vertical, as the `x` of the
/// tangent-space unit vector it encodes.
///
/// Large enough that the brightness it buys is many times the `Rgba16Float`
/// target's step at these values, and small enough that the tilted normal is
/// still comfortably inside the hemisphere the quad faces — so nothing here is
/// measuring a clamp. The angle is `asin` of this, a little over eleven
/// degrees.
const TILT_X: f32 = 0.2;

/// The tangent-space normal every texel of [`tilted_normal_texels`] holds:
/// [`TILT_X`] along `u`, nothing along `v`, and whatever is left over on `z`.
fn tilted_tangent_normal() -> [f32; 3] {
    [TILT_X, 0.0, (1.0 - TILT_X * TILT_X).sqrt()]
}

/// A normal page layer whose every texel tilts towards `+u` by [`TILT_X`].
///
/// `n * 0.5 + 0.5` per channel, rounded to the nearest eight-bit level, which
/// is what a normal map is and what
/// [`PageDesc::push_layer`](crcbl::render::scene::PageDesc::push_layer) takes
/// for [`PageKind::Normal`]. Alpha is opaque and read by nothing: `shaders/mesh.slang` takes the
/// `xyz` of the fetch.
///
/// **Every texel the same**, so nothing here depends on where in the layer a
/// fragment's UV landed and the assertions are about the frame rather than
/// about the filtering. The mip chain the renderer builds over a constant layer
/// is that same constant at every level, so a minified read cannot drift either.
fn tilted_normal_texels() -> Vec<u8> {
    let normal = tilted_tangent_normal();
    let encoded: Vec<u8> = normal
        .iter()
        .map(|axis| (axis * 0.5 + 0.5).clamp(0.0, 1.0).mul_add(255.0, 0.5) as u8)
        .chain(std::iter::once(0xFF))
        .collect();
    encoded.repeat(PAGE_EXTENT as usize * PAGE_EXTENT as usize)
}

/// How far half of [`leaning_normal_texels`]' texels lean towards `+u`, and how
/// far the other half lean towards `-u`.
///
/// **Hard, and unequal, and both halves are load-bearing.** Hard, because the
/// two filters differ by a transfer curve and the gap that opens between them
/// grows with how far the texels sit from the neutral. Unequal, because a page
/// whose leans cancelled would average to the neutral and read black — and a
/// black reading is what a renderer that drew nothing at all also produces, so
/// the expectation this test asserts against would be one it could pass without
/// drawing the quad.
///
/// What they are not is a plausible authored map: no real normal map swings
/// from one hard lean to the opposite one between neighbouring texels. That is
/// the point — this is the input that separates the two filters, and
/// [`the_two_filters_disagree_at_the_bottom_of_the_chain`] measures by how
/// much.
const LEAN_X: f32 = 0.9;

/// [`LEAN_X`]'s other half.
const COUNTER_LEAN_X: f32 = -0.6;

/// A normal page layer whose texels lean **two ways**: a checker of [`LEAN_X`]
/// towards `+u` and [`COUNTER_LEAN_X`] towards `-u`.
///
/// Four texels, because [`PAGE_EXTENT`] is two — so the chain below this layer
/// is one level, and that level is the single texel every cell of the layer
/// averages into. A checker rather than two stripes so the pattern is symmetric
/// under a flipped `u` and a flipped `v` alike: whichever way the fragment
/// stage runs the coordinates, the average is the same average.
fn leaning_normal_texels() -> Vec<u8> {
    let lean = |x: f32| {
        let normal = [x, 0.0, (1.0 - x * x).sqrt()];
        let mut texel = [0xFFu8; 4];
        for (lane, axis) in texel[..3].iter_mut().zip(&normal) {
            *lane = (axis * 0.5 + 0.5).clamp(0.0, 1.0).mul_add(255.0, 0.5) as u8;
        }
        texel
    };
    let (towards, away) = (lean(LEAN_X), lean(COUNTER_LEAN_X));
    [towards, away, away, towards].concat()
}

/// The layer [`normal_scene`] pushes [`tilted_normal_texels`] into.
///
/// **Zero, and layer zero is an ordinary layer.** "No map" is
/// `GpuMaterial::NO_PAGE` on both pages and nothing is burned ahead of this
/// one — see [`PLAIN_ROW`], which is the row that names none.
const TILTED_LAYER: u32 = 0;

/// A layer holding the **neutral** texel a normal map is authored against.
///
/// It exists to make one point that nothing else here can: sampling it is *not*
/// the same as naming no map. `(0.5, 0.5, 1.0)` has no exact eight-bit
/// encoding, so this layer tilts by a fifth of a degree where
/// `GpuMaterial::NO_PAGE` tells the shader to skip the perturbation altogether.
const NEUTRAL_LAYER: u32 = 1;

/// The material row that names no normal map at all.
const PLAIN_ROW: usize = 0;

/// The row that samples [`TILTED_LAYER`] at glTF's default scale.
const TILTED_ROW: usize = 1;

/// The row that samples [`NEUTRAL_LAYER`].
const NEUTRAL_ROW: usize = 2;

/// The row that samples [`TILTED_LAYER`] at half scale — glTF's
/// `normalTexture.scale`, which multiplies the decoded `x` and `y`.
const HALF_SCALE_ROW: usize = 3;

/// The layer [`leaning_normal_texels`] is pushed into.
const LEANING_LAYER: u32 = 2;

/// The row that samples [`LEANING_LAYER`].
const LEANING_ROW: usize = 4;

/// [`HALF_SCALE_ROW`]'s scale.
const HALF_SCALE: f32 = 0.5;

/// The sun every frame here is lit by: straight along `+X`, no ambient.
///
/// **Towards the light**, which is [`DirectionalLight::direction`]'s
/// convention. Along `+X` exactly, because the quad's tangent is `+X` too — so
/// the page's tilt is entirely towards or away from the light and the whole of
/// what it does shows up in one comparison instead of being split between two
/// axes.
///
/// The quad's own normal is `+Z`, perpendicular to this, so an unperturbed
/// surface has `N·L` of exactly zero and is **black**. That is deliberate: it
/// makes the brightening the page buys the entire signal rather than a few per
/// cent on top of a lit surface, and it is why the comparisons below are
/// against a floor rather than a ratio.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::X,
        color: Vec3::splat(1.0),
        ambient: Vec3::ZERO,
    }
}

/// One quad as a resident mesh with a frame, UVs and flags of the caller's
/// choosing.
///
/// [`vertex_v2::quad_mesh`](crate::vertex_v2::quad_mesh) with one difference
/// that matters here: this one takes the frame **per corner**, because the
/// unmarked quad's vertices have to be built through
/// [`MeshVertex::from_normal`] — the constructor whose frame is
/// `orthonormal_basis`' stand-in — rather than through a frame this file chose.
fn quad(
    label: &'static str,
    vertices: Vec<MeshVertex>,
    range: UvRange,
    flags: u32,
) -> MeshDesc<'static> {
    MeshDesc {
        label: Cow::Borrowed(label),
        geometry: Geometry::Flat {
            vertices: Cow::Owned(vertex_bytes(&vertices)),
            uv_range: range,
            indices: Cow::Owned(QUAD_INDICES.to_vec()),
            clusters: quad_clusters(),
            flags,
        },
    }
}

/// The UVs a quad carries when its texture runs left to right the ordinary way:
/// `u` increasing with world `+X`, in [`QUAD_CORNERS`]' order.
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The same quad's UVs **mirrored** in `u`, which is what a mirrored shell of a
/// character carries and what makes its tangent frame left-handed.
const MIRRORED_UVS: [[f32; 2]; 4] = [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

/// The mesh a marked quad is: [`flat_frame`]'s right-handed frame on every
/// corner, ordinary UVs, and the authored-tangent claim.
///
/// The frame is on every corner so the interpolated one is constant across the
/// quad — [`vertex_v2::quad_mesh`](crate::vertex_v2::quad_mesh) gives the
/// argument, and it holds twice as strongly here, where the tangent crosses the
/// stage boundary as its own varying.
fn marked_quad() -> MeshDesc<'static> {
    corner_quad("marked quad", flat_frame(), UVS)
}

/// The same quad with a **left-handed** frame and a mirrored `u`.
///
/// Both halves are needed and neither is decoration. The mirrored UV is what
/// makes `u` run the other way across the surface, so the page's `+u` tilt
/// points along world `-X`; the flipped bitangent is what a tangent-space
/// reconstruction of that parameterisation actually produces, and it is the
/// sign glTF stores in its tangent's `w`. A renderer that carried the tangent
/// and dropped the sign would draw this quad exactly like [`marked_quad`].
fn mirrored_quad() -> MeshDesc<'static> {
    let frame = TangentFrame {
        // `-X`, because `u` decreases along world `+X` on this quad.
        tangent: [-1.0, 0.0, 0.0],
        bitangent: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    corner_quad("mirrored quad", frame, MIRRORED_UVS)
}

/// A quad built from one frame repeated over [`QUAD_CORNERS`], marked as
/// carrying authored tangents.
fn corner_quad(label: &'static str, frame: TangentFrame, uvs: [[f32; 2]; 4]) -> MeshDesc<'static> {
    let range = UvRange::from_uvs(&uvs);
    let vertices: Vec<MeshVertex> = QUAD_CORNERS
        .iter()
        .zip(&uvs)
        .map(|(corner, uv)| MeshVertex::from_frame(*corner, frame, [1.0; 4], *uv, &range))
        .collect();
    quad(label, vertices, range, GpuMesh::MESH_AUTHORED_TANGENTS)
}

/// The same quad with **no** authored frame: built through
/// [`MeshVertex::from_normal`], and unmarked.
///
/// This is every mesh the engine itself authors and every imported primitive
/// with no `TANGENT` accessor. Its eight bytes of frame are
/// `orthonormal_basis`' arbitrary stand-in, so the fragment stage must ignore
/// them and rebuild a frame from `ddx`/`ddy` of world position and UV — which
/// is the only reason this quad can light like [`marked_quad`] at all.
fn unmarked_quad() -> MeshDesc<'static> {
    let range = UvRange::from_uvs(&UVS);
    // **Not `MeshVertex::from_normal`, and the reason is a coincidence.**
    // `orthonormal_basis` on `+Z` returns `+X`, which is this quad's *true*
    // tangent — see `the_stand_in_frame_for_a_flat_quad_is_its_own_true_tangent`
    // below. So the stand-in an unmarked mesh really ships agrees with this
    // parameterisation by accident, and a quad built through it lights
    // identically whichever frame the fragment stage picks: the derivative test
    // passed with `frame_word` sabotaged to claim every mesh had authored
    // tangents, which is a check that could not fail. The frame here is a legal
    // orthonormal one rotated a quarter turn about the normal, so it agrees
    // with no parameterisation of this quad and only the screen-space frame can
    // recover the tangent the page was authored against.
    let misleading = TangentFrame {
        tangent: [0.0, 1.0, 0.0],
        // `cross(normal, tangent)` — right-handed, so nothing here is testing
        // the handedness bit as well.
        bitangent: [-1.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    let vertices: Vec<MeshVertex> = QUAD_CORNERS
        .iter()
        .zip(&UVS)
        .map(|(corner, uv)| MeshVertex::from_frame(*corner, misleading, [1.0; 4], *uv, &range))
        .collect();
    quad("unmarked quad", vertices, range, 0)
}

/// How many times [`tiled_quad`] repeats the page across its own width.
///
/// The page's sampler repeats rather than clamping, so UVs running past one are
/// tiles of the page rather than its edge texel — and tiling is how a frame this
/// size *minifies* a page only [`PAGE_EXTENT`] texels a side. One tile over a
/// quad this wide magnifies it many times over, which reads level 0 and never
/// fetches the chain at all.
///
/// This many puts several texels under every pixel, so the level the sampler
/// picks is past the bottom of a two-level chain and it clamps to the last one.
/// [`the_tiled_quad_minifies_past_the_bottom_of_its_chain`] is that arithmetic
/// written out, so the premise is asserted rather than described.
const MINIFYING_TILES: f32 = 256.0;

/// [`marked_quad`] with its UVs repeated [`MINIFYING_TILES`] times over.
///
/// The frame, the winding and the flags are the marked quad's exactly — the one
/// difference is the coordinate, so a comparison between the two is a
/// comparison of the mip level the sampler reached.
fn tiled_quad() -> MeshDesc<'static> {
    let uvs = UVS.map(|uv| [uv[0] * MINIFYING_TILES, uv[1] * MINIFYING_TILES]);
    corner_quad("tiled quad", flat_frame(), uvs)
}

/// **`orthonormal_basis` hands a `+Z` normal back its own `+X` tangent**, which
/// is why `unmarked_quad` above cannot use the stand-in an unmarked mesh
/// actually ships.
///
/// Duff et al.'s construction is branchless and arbitrary about which
/// perpendicular it picks, and for this normal the one it picks happens to be
/// the tangent this quad's UVs describe. Nothing is wrong with that; what is
/// wrong is a test built on it, because the two frames the fragment stage
/// chooses between would then be the same frame. This pins the coincidence so
/// that the comment explaining the workaround stays true, and fails if
/// `orthonormal_basis` is ever rewritten — at which point the workaround can go.
#[test]
fn the_stand_in_frame_for_a_flat_quad_is_its_own_true_tangent() {
    let (tangent, bitangent) = crcbl_shaders::vertex::orthonormal_basis([0.0, 0.0, 1.0]);
    assert_eq!(tangent, [1.0, 0.0, 0.0]);
    assert_eq!(bitangent, [0.0, 1.0, 0.0]);
}

/// The description every frame here draws: `meshes`, the four material rows,
/// and a **normal** page carrying the tilted, neutral and leaning layers. It
/// names no base-colour page at all: every row here shades by its factors.
///
/// # The off-by-one this file is the fixture for, and what catches it
///
/// Every layer number here shifted down by one when row (d) stopped burning
/// layer 0, and a producer that shifted its page and not its rows names a layer
/// the page has not got. `ForwardRenderer::with_scene` refuses that before it
/// creates a device object.
///
/// **Sabotage:** [`LEANING_LAYER`] left at its old `3` while the page numbers
/// from zero, with the `assert_eq!` below relaxed so the refusal is what
/// reports. Red on radv on 2026-09-06 at
/// `a_normal_map_tilted_towards_the_sun_lights_a_quad_the_sun_is_edge_on_to`
/// with `"the forward renderer builds this description:
/// InvalidDescriptor(\"material row 4 samples normal page layer 3, and that
/// page has 3\")"`.
fn normal_scene(meshes: Vec<MeshDesc<'static>>) -> SceneDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::Normal, PAGE_EXTENT);
    let tilted = page.push_layer(PageKind::Normal, tilted_normal_texels());
    assert_eq!(tilted, TILTED_LAYER, "the tilted layer is the page's first");
    let neutral = page.push_layer(
        PageKind::Normal,
        PageDesc::NEUTRAL_NORMAL.repeat(PAGE_EXTENT as usize * PAGE_EXTENT as usize),
    );
    assert_eq!(neutral, NEUTRAL_LAYER);
    let leaning = page.push_layer(PageKind::Normal, leaning_normal_texels());
    assert_eq!(leaning, LEANING_LAYER);
    let rows = vec![
        GpuMaterial::UNTINTED,
        GpuMaterial {
            normal_texture: TILTED_LAYER,
            ..GpuMaterial::UNTINTED
        },
        GpuMaterial {
            normal_texture: NEUTRAL_LAYER,
            ..GpuMaterial::UNTINTED
        },
        GpuMaterial {
            normal_texture: TILTED_LAYER,
            normal_scale: HALF_SCALE,
            ..GpuMaterial::UNTINTED
        },
        GpuMaterial {
            normal_texture: LEANING_LAYER,
            ..GpuMaterial::UNTINTED
        },
    ];
    assert_eq!(rows[PLAIN_ROW].normal_texture, GpuMaterial::NO_PAGE);
    SceneDesc {
        meshes,
        materials: rows,
        page,
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

/// One frame of `scene` with `place` having put the instances in it, under
/// [`sun`], answered as the raw `Rgba16Float` target.
///
/// Every effect that could put a second term or a neighbour's texel into a
/// reading is forced off — see the module docs.
fn lit_frame(
    scene: &SceneDesc<'_>,
    normals_view: bool,
    place: impl FnOnce(&mut ForwardRenderer),
) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::with_scene(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        scene,
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
    renderer.set_normals_view(normals_view);
    place(&mut renderer);
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

/// [`quad_camera`], named here so the module reads without the import spelled
/// at every call.
fn camera() -> Camera {
    quad_camera()
}

/// Puts mesh `mesh` of the description at the origin under material row
/// `material`.
fn place_quad(renderer: &mut ForwardRenderer, mesh: usize, material: usize) {
    renderer
        .add_instance(&InstanceDesc {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        })
        .expect("an instance pool of thousands has room for one quad");
}

/// The texel at the middle of a quad standing at the origin.
///
/// Well inside it on both axes — [`QUAD_HALF`](crate::vertex_v2::QUAD_HALF)
/// is the edge — so nothing read here is a partly covered pixel, which under
/// a disabled antialiasing pass would be the clear colour rather than a blend.
fn centre_texel() -> (u32, u32) {
    let (x, y) = pixel_at(0.0, 0.0);
    let (width, height) = MESH_EXTENT;
    assert!(x < width && y < height, "the centre texel is in the frame");
    (x, y)
}

/// The red channel at the quad's centre — which under [`sun`]'s white light is
/// the whole of the shaded value, since all three channels carry it equally.
fn centre(frame: &HdrTarget) -> f32 {
    let (x, y) = centre_texel();
    frame.pixel(x, y)[0]
}

/// **A normal map tilted towards the sun lights a quad the sun is edge-on to.**
///
/// The quad's own normal is `+Z` and the sun comes from `+X`, so `N·L` is
/// exactly zero and an unperturbed surface is black. Tilt every texel by
/// [`TILT_X`] towards `+u` — which is world `+X` on this quad — and `N·L`
/// becomes that tilt, so the surface lights.
///
/// **What each of the two readings rules out.** A renderer that never sampled
/// the page draws the plain row and the tilted row identically, and the
/// comparison fails. A renderer that sampled the page and used the *texel* as a
/// world-space normal would light the plain row too, since the neutral texel is
/// mostly `+Z` — so the plain row being black at all is what says the frame is
/// being applied rather than the texel being used raw.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_normal_map_tilted_towards_the_sun_lights_a_quad_the_sun_is_edge_on_to() {
    let scene = normal_scene(vec![marked_quad()]);
    let plain = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 0, PLAIN_ROW);
    }));
    let tilted = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 0, TILTED_ROW);
    }));

    // The surface is edge-on to the light, so the only thing that can be
    // reaching it is the perturbation.
    assert!(
        plain < LIT_FLOOR,
        "a quad edge-on to the only light read {plain}, and nothing but the normal map \
         should be able to light it"
    );
    // `N·L` is the tilt itself once the frame is applied — the tangent is `+X`
    // and so is the light — so the shaded value is `albedo * colour * TILT_X`,
    // and the sun's colour is one. Read as a band rather than an equality: the
    // page is eight-bit, so the encoded tilt is the nearest level to `TILT_X`
    // and not `TILT_X`, and `PAGE_TILT_TOLERANCE` is that quantisation measured.
    assert!(
        (tilted - TILT_X).abs() <= PAGE_TILT_TOLERANCE,
        "the tilted row read {tilted} where the page tilts by {TILT_X}, which is more than \
         the {PAGE_TILT_TOLERANCE} an eight-bit page can lose"
    );
}

/// What a texel may read and still count as unlit.
///
/// The quad is edge-on to the only light and there is no ambient term, so the
/// honest expectation is zero; this is the room a last-bit `N·L` either side of
/// zero leaves, which four rasterisers may each land on differently.
const LIT_FLOOR: f32 = 1.0e-3;

/// How far a shaded value may sit from the tilt the page was authored with.
///
/// The page is eight-bit and the tilt is stored as `n * 0.5 + 0.5`, so the
/// encoded `x` is the nearest level to [`TILT_X`] — half a level, doubled by the
/// decode, is `1 / 255`. The renormalise after the decode moves it again by less
/// than that. Measured by
/// `the_page_encodes_the_tilt_to_within_the_tolerance_the_frame_tests_allow`,
/// which is a host-side test rather than a claim, and swept on the device: with
/// this at zero the lit quad reads `0.20495605` against a `0.2` tilt on radv and
/// on lavapipe both, a gap of a little over one eight-bit level, so two is the
/// bound with a level of room.
const PAGE_TILT_TOLERANCE: f32 = 2.0 / 255.0;

/// **A left-handed tangent frame puts the tilt on the other side.**
///
/// This is the reading `docs/plan/43-render-standards.md` §2 chose the vertex
/// route for. The mirrored quad carries the same page, the same normal and the
/// same light; what differs is that its `u` runs along world `-X` and its
/// bitangent is flipped, which is exactly what a mirrored UV shell of a
/// character carries. The page's `+u` tilt therefore points **away** from the
/// sun, and the quad stays dark.
///
/// **A renderer that dropped the handedness draws both quads alike**, and that
/// is the whole diagnostic: the two assertions below cannot both hold unless the
/// sign crossed the stage boundary. `FRAME_LEFT_HANDED` in `shaders/mesh.slang`
/// is where it rides.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_left_handed_frame_puts_the_tilt_on_the_other_side() {
    let scene = normal_scene(vec![marked_quad(), mirrored_quad()]);
    let right_handed = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 0, TILTED_ROW);
    }));
    let left_handed = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 1, TILTED_ROW);
    }));

    assert!(
        (right_handed - TILT_X).abs() <= PAGE_TILT_TOLERANCE,
        "the right-handed quad read {right_handed} where the page tilts by {TILT_X}"
    );
    // Tilted away from the light: `N·L` is negative, the accumulation clamps it
    // at zero, and the surface is as dark as the plain row.
    assert!(
        left_handed < LIT_FLOOR,
        "the left-handed quad read {left_handed}; its tilt points away from the sun, so the \
         clamped Lambert term is zero and only a dropped handedness could light it"
    );
}

/// **An unmarked mesh rebuilds the frame from screen-space derivatives and
/// lights the same way.**
///
/// The unmarked quad's stored frame is `orthonormal_basis`' stand-in, which
/// agrees with no UV parameterisation — so a fragment stage that used it would
/// light this quad by an arbitrary rotation of the page. What it does instead is
/// Schüler's cotangent frame out of `ddx`/`ddy`, and on a flat quad whose `u`
/// runs along `+X` that frame is the marked quad's to within the precision of
/// two finite differences.
///
/// **The tolerance was measured before it was written.** Run with the bound at
/// zero, both quads read `0.20495605` — bit-identical — on radv and on lavapipe
/// alike, so the two arithmetics do not disagree at all on a flat quad. The
/// bound below is therefore slack against the backends this machine cannot run,
/// not a gap anyone observed. See [`DERIVATIVE_TOLERANCE`].
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_unmarked_mesh_takes_the_derivative_frame_and_agrees_with_the_marked_one() {
    let scene = normal_scene(vec![marked_quad(), unmarked_quad()]);
    let marked = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 0, TILTED_ROW);
    }));
    let unmarked = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 1, TILTED_ROW);
    }));

    // Both lit at all, first — otherwise the difference below is a comparison of
    // two blacks and would hold however wrong either frame was.
    for (name, value) in [("marked", marked), ("unmarked", unmarked)] {
        assert!(
            (value - TILT_X).abs() <= PAGE_TILT_TOLERANCE,
            "the {name} quad read {value} where the page tilts by {TILT_X}"
        );
    }
    assert!(
        (marked - unmarked).abs() <= DERIVATIVE_TOLERANCE,
        "the marked quad read {marked} and the unmarked one {unmarked}, a difference of {} \
         against a bound of {DERIVATIVE_TOLERANCE}",
        (marked - unmarked).abs()
    );
}

/// How far the derivative frame's shaded value may sit from the vertex frame's
/// on a flat quad whose UV runs along its tangent.
///
/// **Swept before it was fixed.** With this at zero the two quads read
/// `0.20495605` each, identical to the bit, on both Vulkan drivers this machine
/// has — radv on the Radeon and lavapipe in software. On a flat quad the two
/// arithmetics are the same arithmetic: one interpolates a quaternion-decoded
/// tangent and re-orthogonalises it, the other takes two finite differences of a
/// perspective-correct varying, and there is nothing in between them to round.
///
/// So the bound is not a measured gap; it is slack for the two backends this
/// machine cannot run, and it is one step of the eight-bit page — the smallest
/// difference the normal texture is able to express. A frame that is merely
/// rounded differently stays inside it; a frame that is *wrong* is not nearby,
/// because the tilt then points away from the sun and the clamped Lambert term
/// reads zero against this quad's `0.205`. That is what this failed with before
/// the screen-space frame's winding was corrected.
const DERIVATIVE_TOLERANCE: f32 = 1.0 / 255.0;

/// **A material that names no normal map gets the surface normal back
/// exactly**, and the neutral texel is not the same thing.
///
/// The `Normals` view writes `n * 0.5 + 0.5` straight into the `Rgba16Float`
/// target, so a quad facing `+Z` reads exactly `(0.5, 0.5, 1.0)` — every one of
/// those three is exact in half precision. That exactness is the claim: it is
/// what says the goldens of every scene in this tree with no normal map cannot
/// have moved, and it is a stronger statement than any tolerance.
///
/// **The neutral layer is the control**, and it is what says the exactness comes
/// from the `NO_PAGE` test in `shading_normal_of` rather than from the page
/// happening to be neutral. `(0.5, 0.5, 1.0)` has no eight-bit encoding: `0x80`
/// decodes to `128 / 255`, which is a fifth of a degree off vertical, and that
/// shows up here as a red channel several half-float steps away from `0.5`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn no_normal_map_returns_the_surface_normal_exactly_and_the_neutral_texel_does_not() {
    let scene = normal_scene(vec![marked_quad()]);
    let (x, y) = centre_texel();

    let plain = lit_frame(&scene, true, |renderer| {
        place_quad(renderer, 0, PLAIN_ROW);
    });
    assert_eq!(
        [
            plain.pixel(x, y)[0],
            plain.pixel(x, y)[1],
            plain.pixel(x, y)[2]
        ],
        [0.5, 0.5, 1.0],
        "a quad facing +Z whose material names no normal map must encode to exactly the \
         surface normal, or every golden in the tree drawn without a normal map moves"
    );

    let neutral = lit_frame(&scene, true, |renderer| {
        place_quad(renderer, 0, NEUTRAL_ROW);
    });
    let red = neutral.pixel(x, y)[0];
    assert_ne!(
        red, 0.5,
        "sampling the neutral texel must not be the same as naming no map, or the \
         NO_PAGE test in the shader is doing nothing"
    );
    // And it is off by about the eight-bit step the encoding cannot represent,
    // rather than by something structural. Half of `1 / 255`, because the view
    // halves the normal on the way to the target.
    let step = 0.5 / 255.0;
    assert!(
        (red - 0.5 - step).abs() < step * 0.5,
        "the neutral texel moved the encoded normal's red channel to {red}, and the \
         eight-bit step it should cost is {step}"
    );
}

/// **The `Normals` debug view draws the perturbed normal, and glTF's
/// `normalTexture.scale` moves it.**
///
/// The view is the only reading in this file about the vector itself rather than
/// about what a light did with it, and it is what says the perturbation reaches
/// *every* consumer of the shading normal rather than only the Lambert term —
/// the view, the lighting and `ssr.slang` all read the one variable.
///
/// The scale is checked in the same frame because it is the one part of the
/// decode a brightness comparison reads only indirectly: glTF 2.0 §3.9.3
/// multiplies the decoded `x` and `y` and leaves `z`, so halving it does **not**
/// halve the tilt — the vector is renormalised afterwards. The assertion is
/// therefore written against the arithmetic rather than against half of
/// [`TILT_X`].
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_normals_view_draws_the_perturbed_normal_and_the_scale_moves_it() {
    let scene = normal_scene(vec![marked_quad()]);
    let (x, y) = centre_texel();

    let tilted = lit_frame(&scene, true, |renderer| {
        place_quad(renderer, 0, TILTED_ROW);
    });
    // `n * 0.5 + 0.5`, so the tilt reaches the target halved.
    let expected = TILT_X * 0.5 + 0.5;
    let red = tilted.pixel(x, y)[0];
    assert!(
        (red - expected).abs() <= PAGE_TILT_TOLERANCE,
        "the normals view read {red} where the page's tilt encodes to {expected}"
    );

    let halved = lit_frame(&scene, true, |renderer| {
        place_quad(renderer, 0, HALF_SCALE_ROW);
    });
    let scaled = scaled_tilt(HALF_SCALE);
    let red = halved.pixel(x, y)[0];
    assert!(
        (red - (scaled * 0.5 + 0.5)).abs() <= PAGE_TILT_TOLERANCE,
        "the half-scale row read {red} where scaling the decoded xy by {HALF_SCALE} and \
         renormalising gives {scaled}"
    );
    // And the two really differ, so the assertion above is not one a renderer
    // ignoring the scale would also pass.
    assert!(
        (scaled - TILT_X).abs() > PAGE_TILT_TOLERANCE * 2.0,
        "the two scales must produce tilts this test can tell apart"
    );
}

/// The `x` of the tangent-space normal after glTF's `normalTexture.scale`:
/// `normalize((x, y, z) * (scale, scale, 1)).x`.
///
/// Not `scale * x`. The specification scales `x` and `y` and leaves `z`, and the
/// vector is renormalised — so a half scale tilts by rather more than half the
/// angle. Written out here because the test that reads it would otherwise be
/// asserting against a number nobody could check.
fn scaled_tilt(scale: f32) -> f32 {
    let normal = tilted_tangent_normal();
    let scaled = [normal[0] * scale, normal[1] * scale, normal[2]];
    let length = (scaled[0] * scaled[0] + scaled[1] * scaled[1] + scaled[2] * scaled[2]).sqrt();
    scaled[0] / length
}

/// The page really does encode [`TILT_X`] to within
/// [`PAGE_TILT_TOLERANCE`] — a host-side check, so the device tests above are
/// not the first place an encoding mistake shows up.
///
/// Without it, an encoder that wrote the wrong channel or forgot the `* 0.5 +
/// 0.5` would fail every frame test above with a message about a shaded value,
/// and the page would be the last place anyone looked.
#[test]
fn the_page_encodes_the_tilt_to_within_the_tolerance_the_frame_tests_allow() {
    let texels = tilted_normal_texels();
    assert_eq!(
        texels.len(),
        PAGE_EXTENT as usize * PAGE_EXTENT as usize * 4,
        "a layer is the page's extent squared in RGBA8"
    );
    let want = tilted_tangent_normal();
    for (index, texel) in texels.chunks_exact(4).enumerate() {
        let decoded: Vec<f32> = texel[..3]
            .iter()
            .map(|lane| f32::from(*lane) / 255.0 * 2.0 - 1.0)
            .collect();
        for (axis, (got, expected)) in decoded.iter().zip(&want).enumerate() {
            assert!(
                (got - expected).abs() <= PAGE_TILT_TOLERANCE,
                "texel {index} axis {axis} decodes to {got} where it was authored {expected}"
            );
        }
        assert_eq!(texel[3], 0xFF, "texel {index} is opaque");
    }
}

/// The two UV sets this file uses really are mirrors of each other, and the
/// frames really do differ in handedness.
///
/// Both are premises of
/// [`a_left_handed_frame_puts_the_tilt_on_the_other_side`], and neither is
/// visible in the frame it reads: a mirrored quad whose UVs were *not* mirrored
/// would simply be a quad with a different tangent, and the test would pass for
/// the wrong reason.
#[test]
fn the_mirrored_quad_is_a_mirror_and_its_frame_is_left_handed() {
    for (left, right) in UVS.iter().zip(&MIRRORED_UVS) {
        assert_eq!(left[0], 1.0 - right[0], "u is mirrored");
        assert_eq!(left[1], right[1], "and v is not");
    }
    let Geometry::Flat { vertices, .. } = &mirrored_quad().geometry else {
        unreachable!("the quad is flat geometry")
    };
    for record in vertices.chunks_exact(crcbl_shaders::mesh::VERTEX_STRIDE) {
        let vertex = MeshVertex::from_bytes(record.try_into().expect("one record"));
        assert_eq!(
            vertex.qtangent.handedness(),
            -1.0,
            "the mirrored quad's frame must be the left-handed one, or the test that reads \
             it is comparing two right-handed quads"
        );
    }
    let Geometry::Flat { vertices, .. } = &marked_quad().geometry else {
        unreachable!("the quad is flat geometry")
    };
    for record in vertices.chunks_exact(crcbl_shaders::mesh::VERTEX_STRIDE) {
        let vertex = MeshVertex::from_bytes(record.try_into().expect("one record"));
        assert_eq!(vertex.qtangent.handedness(), 1.0);
    }
}

/// The unmarked quad really is unmarked and the marked ones really are marked.
///
/// [`an_unmarked_mesh_takes_the_derivative_frame_and_agrees_with_the_marked_one`]
/// is a comparison of two frames, and it would pass just as well if both of them
/// took the same path — which is exactly what a flag written the wrong way round
/// would produce.
#[test]
fn the_flags_say_which_frame_each_quad_asks_for() {
    for (mesh, want) in [
        (marked_quad(), GpuMesh::MESH_AUTHORED_TANGENTS),
        (mirrored_quad(), GpuMesh::MESH_AUTHORED_TANGENTS),
        (unmarked_quad(), 0),
    ] {
        let Geometry::Flat { flags, .. } = mesh.geometry else {
            unreachable!("the quads are flat geometry")
        };
        assert_eq!(flags, want, "{} claims the wrong frame", mesh.label);
    }
}

/// The camera and projection this file draws through are the orthographic ones
/// [`pixel_at`] is written against.
///
/// A perspective camera would make every texel coordinate in this file wrong by
/// a divide, and the failure would look like a lighting bug rather than a
/// projection one.
#[test]
fn the_frames_are_drawn_through_the_orthographic_camera() {
    assert!(matches!(
        camera().projection,
        Projection::Orthographic { .. }
    ));
    assert_eq!(camera().eye.z, crate::vertex_v2::CAMERA_Z);
    // And the quad the tests read the middle of really does cover that texel.
    let _ = centre_texel();
}

/// The tangent-space `u` a page texel decodes to, normalised the way
/// `shading_normal_of` normalises it.
///
/// On a quad whose tangent is world `+X` and under [`sun`], that number **is**
/// `N·L` — so it is what [`centre`] reads out of the frame, and the two can be
/// compared directly.
fn tilt_of(texel: &[u8]) -> f32 {
    let decoded: Vec<f32> = texel[..3]
        .iter()
        .map(|lane| f32::from(*lane) / 255.0 * 2.0 - 1.0)
        .collect();
    let length = decoded.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    decoded[0] / length
}

/// The tilt at the **bottom** of [`leaning_normal_texels`]' chain, built through
/// `filter` — [`mip::normal_chain`] for the one the renderer is meant to use
/// and [`mip::chain`] for the colour one.
///
/// The bottom level is the whole of what a minified frame can read, and on a
/// [`PAGE_EXTENT`]-wide page it is one texel: the average of all four, which is
/// where the two filters part company.
fn bottom_tilt(filter: fn(&[u8], u32) -> Vec<Vec<u8>>) -> f32 {
    let levels = filter(&leaning_normal_texels(), PAGE_EXTENT);
    let bottom = levels.last().expect("a 2² layer has one level below it");
    assert_eq!(bottom.len(), 4, "the bottom of the chain is a single texel");
    tilt_of(bottom)
}

/// **A minified normal page lights by the renormalised average and not by the
/// colour one.**
///
/// **No other frame here reaches [`mip::normal_chain`]'s output at all.** Every
/// one of them draws a page whose every texel is the same, and a constant layer
/// survives *any* box filter unchanged — so swapping the normal page's chain for
/// the base-colour page's at the call site in `crcbl_render::forward` moved no
/// golden, no e2e value and no assertion in this suite. The layer this one
/// draws leans two ways, which is exactly the input the two filters answer
/// differently for: one averages the decoded vectors and renormalises, the
/// other decodes an sRGB curve, averages, and re-encodes with no length to put
/// back.
///
/// **What the tiling is for.** The chain is only reachable by a fragment whose
/// footprint covers more than a texel, and one tile of a two-texel page over a
/// quad this size is a heavy magnification. [`MINIFYING_TILES`] tiles put the
/// LOD past the bottom of the chain, so the sampler clamps there and the whole
/// quad reads that one texel — which is why the centre reading is the whole
/// frame's value rather than a point on a ramp.
///
/// **Sabotaged before it was believed.** With the call site in
/// `crcbl_render::forward::ForwardRenderer::with_scene` swapped to build the
/// normal layers through `crate::mip::chain`, this reads:
///
/// ```text
/// the minified quad read 0.63134766 where the bottom of the normal page's chain
/// tilts by 0.23232852, a gap of 0.39901912 against a bound of 0.007843138; the
/// colour chain's bottom tilts by 0.5534985
/// ```
///
/// The reading sits above the colour chain's own tilt for the reason
/// [`MINIFIED_TOLERANCE`] gives: what rides on top of the Lambert term is
/// `mesh.slang`'s GGX lobe, and a normal leaning that much further towards the
/// sun is a normal that much closer to the half vector between the sun and the
/// camera.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_minified_normal_page_lights_by_the_renormalised_average() {
    let scene = normal_scene(vec![marked_quad(), tiled_quad()]);
    let read = centre(&lit_frame(&scene, false, |renderer| {
        place_quad(renderer, 1, LEANING_ROW);
    }));
    let expected = bottom_tilt(mip::normal_chain);
    assert!(
        (read - expected).abs() <= MINIFIED_TOLERANCE,
        "the minified quad read {read} where the bottom of the normal page's chain tilts \
         by {expected}, a gap of {} against a bound of {MINIFIED_TOLERANCE}; the colour \
         chain's bottom tilts by {}",
        (read - expected).abs(),
        bottom_tilt(mip::chain)
    );
}

/// How far the minified reading may sit from the texel at the bottom of the
/// chain.
///
/// **Swept before it was fixed.** With this at zero the tiled quad reads
/// `0.2376709` on radv and on lavapipe alike — identical to the bit, as the
/// other readings in this file are — against a bottom level that tilts by
/// `0.23232852`. The `0.0053` between them is neither the chain nor the
/// sampler: it is `mesh.slang`'s specular lobe, which rides on top of the
/// clamped Lambert term in every reading in this file —
/// [`a_normal_map_tilted_towards_the_sun_lights_a_quad_the_sun_is_edge_on_to`]
/// carries it too, reading `0.20495605` against a `0.2` tilt. It grows as the
/// shading normal turns towards the half vector between the sun and the camera,
/// so a bound written here does not transfer to a page that leans further.
///
/// So the bound is [`PAGE_TILT_TOLERANCE`]'s — one eight-bit page step on the
/// decoded normal — which has room over the measured gap and is still far under
/// the distance to the colour filter's answer that
/// [`the_two_filters_disagree_at_the_bottom_of_the_chain`] holds this test to.
const MINIFIED_TOLERANCE: f32 = PAGE_TILT_TOLERANCE;

/// **The two filters disagree at the bottom of the chain**, by far more than
/// the frame above can absorb.
///
/// Without this the device test is a comparison whose two sides might be the
/// same number, and it would pass just as well on a renderer that mipped the
/// normal page through the colour filter. Host-side, so the disagreement is a
/// property of the two functions rather than of a frame.
#[test]
fn the_two_filters_disagree_at_the_bottom_of_the_chain() {
    let renormalised = bottom_tilt(mip::normal_chain);
    let colour = bottom_tilt(mip::chain);
    assert!(
        renormalised > LIT_FLOOR * 10.0,
        "the renormalised average must light the quad at all, or the device test asserts \
         against a black frame: it tilts by {renormalised}"
    );
    assert!(
        (renormalised - colour).abs() > MINIFIED_TOLERANCE * 10.0,
        "the renormalised bottom level tilts by {renormalised} and the colour one by \
         {colour}, a gap of {} — and the device test can only tell them apart if that gap \
         is well past its {MINIFIED_TOLERANCE} bound",
        (renormalised - colour).abs()
    );
}

/// **The tiled quad really is minified past the bottom of its chain.**
///
/// The premise of the device test above, and the one part of it that is not
/// visible in the value it reads: a quad that magnified the page would sample
/// level 0 — one authored lean or the other, or a blend across their boundary —
/// and would fail with a number that looks like a filtering bug rather than
/// like a coordinate one. Stated as the LOD arithmetic the sampler does, at
/// least one level past the last the chain has, so nothing of level 0 is
/// blended in.
#[test]
fn the_tiled_quad_minifies_past_the_bottom_of_its_chain() {
    let (left, _) = pixel_at(-QUAD_HALF, 0.0);
    let (right, _) = pixel_at(QUAD_HALF, 0.0);
    let across = (right - left) as f32;
    assert!(across > 1.0, "the quad covers {across} pixels");
    let texels_per_pixel = MINIFYING_TILES * PAGE_EXTENT as f32 / across;
    let lod = texels_per_pixel.log2();
    let bottom = mip::normal_chain(&leaning_normal_texels(), PAGE_EXTENT).len() as f32;
    assert!(
        lod >= bottom + 1.0,
        "the tiled quad puts {texels_per_pixel} texels under a pixel, which is a LOD of \
         {lod}; the chain's bottom level is {bottom} and the sampler must land a whole \
         level past it for level 0 to be out of the blend"
    );
}
