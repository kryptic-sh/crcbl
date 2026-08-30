//! **The v2 vertex encodings, decoded by a real device** — the QTangent's
//! normal out of a frame drawn as world-space normals, and a UV lane out of a
//! textured quad whose coordinates lie outside the unit square.
//!
//! # Why a device has to answer this
//!
//! `crcbl_shaders::vertex` builds and checks both encodings on the host, and
//! `crcbl_shaders::mesh::every_shader_decodes_a_vertex_the_same_way` checks
//! that the three Slang copies spell one decode. Neither is evidence that the
//! decode a driver compiled agrees with the encoder: the host test never sees
//! the shader, and the shader test compares text. The two claims that need a
//! frame are exactly the two the encoding is new for —
//!
//! * **a `snorm16x4` quaternion decodes to the normal it was built from**, and
//! * **a `unorm16x2` lane decodes through its mesh's [`UvRange`] rather than
//!   being read as a coordinate in its own right**.
//!
//! # What is still not covered here, and where it is covered instead
//!
//! **Not the handedness lane.** The raster path has nothing that shades through
//! a bitangent — no normal map, no anisotropy — so a frame and its mirror image
//! produce the same picture whatever the sign of `w` does, and a test that drew
//! both and compared them would be asserting that two identical frames are
//! identical. Both handednesses are drawn below because a left-handed frame is a
//! *different quaternion* and its decode is worth exercising, but the sign
//! itself is proven where something reads it: `crcbl-vk`'s
//! `vk_e2e/skinning.rs::assert_vertex` compares the handedness of a skinned
//! frame exactly, off a buffer readback rather than off a picture.
//!
//! **Not `uv1`.** Nothing in the workspace writes a second coordinate set —
//! the glTF importer reads no `TEXCOORD_1` — so the lane is reserved and its
//! decode is checked by the host guard alone.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, render_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{
    CHECKER_LAYER, CHECKER_TEXELS, Capacities, Geometry, MeshDesc, PAGE_EXTENT, PageDesc,
    ProbeGrid, SceneDesc,
};
use crcbl::render::{
    Camera, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc, Projection,
    RenderEffects, TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, MeshVertex, vertex_bytes};
use crcbl_shaders::meshlet::{ClusterBounds, MeshClusters, Meshlet};
use crcbl_shaders::vertex::{TangentFrame, UvRange};
use std::borrow::Cow;

/// Half the visible world height, and therefore the unit every world
/// coordinate below is written in.
///
/// The projection is **orthographic** for one reason: it makes
/// [`pixel_at`] exact arithmetic instead of a perspective divide, so a test
/// that wants the texel over a named point on a quad can name the point rather
/// than hunt for it in the frame.
const ORTHO_HALF_HEIGHT: f32 = 1.0;

/// Where the camera stands, looking down `-Z` at the quads in the `z = 0`
/// plane. Between the near and far planes below with room either side.
const CAMERA_Z: f32 = 3.0;

/// Half a quad's edge. Small enough that four of them at [`QUAD_CENTRE`] do not
/// touch, and that every point this file samples is well inside one.
const QUAD_HALF: f32 = 0.4;

/// How far from the origin each of the four normals-view quads sits, on both
/// axes.
const QUAD_CENTRE: f32 = 0.5;

/// The camera every frame here is drawn with.
fn quad_camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 0.0, CAMERA_Z),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::Orthographic {
            half_height: ORTHO_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// The texel a world-space point in the `z = 0` plane lands on, under
/// [`quad_camera`].
///
/// The orthographic projection maps `y` onto `±ORTHO_HALF_HEIGHT` and `x` onto
/// that times the aspect ratio; NDC is Y-up and the readback is row-major from
/// the top, which is the one flip in here.
fn pixel_at(x: f32, y: f32) -> (u32, u32) {
    let (width, height) = MESH_EXTENT;
    let aspect = width as f32 / height as f32;
    let ndc_x = x / (ORTHO_HALF_HEIGHT * aspect);
    let ndc_y = y / ORTHO_HALF_HEIGHT;
    assert!(
        ndc_x.abs() < 1.0 && ndc_y.abs() < 1.0,
        "({x}, {y}) is outside the frame this camera draws"
    );
    (
        ((ndc_x * 0.5 + 0.5) * width as f32) as u32,
        ((0.5 - ndc_y * 0.5) * height as f32) as u32,
    )
}

/// A quad in the `z = 0` plane, wound the way `crcbl_shaders::mesh::FACES`
/// winds its `+Z` face so this one faces the camera too.
const QUAD_CORNERS: [[f32; 3]; 4] = [
    [-QUAD_HALF, -QUAD_HALF, 0.0],
    [QUAD_HALF, -QUAD_HALF, 0.0],
    [QUAD_HALF, QUAD_HALF, 0.0],
    [-QUAD_HALF, QUAD_HALF, 0.0],
];

/// Its two triangles, `cube_indices`' pattern for one face.
const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// One quad as a resident mesh: `frame` on all four corners, `uvs` corner by
/// corner in [`QUAD_CORNERS`]' order.
///
/// The frame is on every corner so the interpolated normal is constant over the
/// quad — a varying one would make the reading below a statement about the
/// rasteriser's interpolation rather than about the decode.
fn quad_mesh(label: &'static str, frame: TangentFrame, uvs: [[f32; 2]; 4]) -> MeshDesc<'static> {
    let range = UvRange::from_uvs(&uvs);
    let vertices: Vec<MeshVertex> = QUAD_CORNERS
        .iter()
        .zip(&uvs)
        .map(|(corner, uv)| MeshVertex::from_frame(*corner, frame, [1.0; 4], *uv, &range))
        .collect();
    MeshDesc {
        label: Cow::Borrowed(label),
        geometry: Geometry::Flat {
            vertices: Cow::Owned(vertex_bytes(&vertices)),
            uv_range: range,
            indices: Cow::Owned(QUAD_INDICES.to_vec()),
            clusters: quad_clusters(),
        },
    }
}

/// The one cluster a quad is, for the mesh path to have something to read.
///
/// Omnidirectional rather than a cone about `+Z`: a cone is a cull rule, and a
/// test whose geometry can be rejected by one has a way to pass by drawing
/// nothing.
fn quad_clusters() -> MeshClusters {
    MeshClusters {
        clusters: vec![
            Meshlet::new(
                0,
                QUAD_CORNERS.len(),
                0,
                QUAD_INDICES.len() / 3,
                ClusterBounds {
                    center: [0.0, 0.0, 0.0],
                    radius: QUAD_HALF * std::f32::consts::SQRT_2,
                    cone_axis: ClusterBounds::OMNIDIRECTIONAL_AXIS,
                    cone_cutoff: ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
                },
            )
            .expect("a four-vertex cluster fits a u32"),
        ],
        vertices: (0..QUAD_CORNERS.len() as u32).collect(),
        corners: QUAD_INDICES.iter().map(|&i| i as u8).collect(),
    }
}

/// [`demo`](crcbl::render::scene::demo)'s page and material rows, with
/// `meshes` in place of its geometry.
///
/// Row 0 is the untextured white one every unassigned instance shades through;
/// row [`TEXTURED_ROW`] samples the checker. Both are `demo`'s, spelled out
/// here because the mesh ids have to be this file's own — a description that
/// appended to `demo` would put every quad past four meshes and one DAG.
fn quad_scene(meshes: Vec<MeshDesc<'static>>) -> SceneDesc<'static> {
    let mut page = PageDesc::opaque_white(PAGE_EXTENT);
    let checker = page.push_layer(&CHECKER_TEXELS[..]);
    assert_eq!(
        checker, CHECKER_LAYER,
        "the checker is the layer past white"
    );
    SceneDesc {
        meshes,
        materials: vec![
            GpuMaterial {
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color_texture: CHECKER_LAYER,
                ..GpuMaterial::UNTINTED
            },
        ],
        page,
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

/// The material row that samples the checker.
const TEXTURED_ROW: usize = 1;

/// Renders one frame of `scene` with `place` having put the instances in it,
/// and answers the raw `Rgba16Float` target.
///
/// The HDR target rather than the swapchain image, for the reason
/// `mesh_e2e/hdr.rs` gives: it is linear and untonemapped, so a value read out
/// of it is the value the fragment stage wrote.
///
/// Antialiasing and reflections are forced off. Both would put a neighbour's
/// texel into the one being read, which is the whole measurement here.
fn quad_frame(
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
            .force(RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_normals_view(normals_view);
    place(&mut renderer);
    let mut hdr = Vec::new();
    let _ = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &quad_camera(),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The four frames the normals-view quads carry, and where each one's quad
/// stands.
///
/// The first two are `crcbl_shaders::vertex`'s
/// `a_frame_extracts_the_quaternion_the_paper_writes_for_it` pairs — the
/// rotations whose quaternions that test writes out by hand — and the second
/// two are those with the bitangent mirrored, which is a **different**
/// quaternion — the encoder negates all four lanes — around the same normal.
///
/// Two normals and two quaternions each, so a decode that swapped two lanes,
/// read the pair at the wrong word, or rotated the wrong canonical axis has
/// four places here to go wrong. What it does **not** separate is the
/// handedness: `q` and `-q` describe one rotation, so the mirrored pair's
/// normal is the same vector as its twin's, which is the module header's point.
///
/// Both normals point somewhere no default has ever pointed: `(0, 0, 1)` faces
/// the camera and `(0, -1, 0)` is straight down.
const NORMAL_QUADS: [NormalQuad; 4] = [
    NormalQuad {
        name: "quarter turn about +Z",
        frame: TangentFrame {
            tangent: [0.0, 1.0, 0.0],
            bitangent: [-1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
        at: (-QUAD_CENTRE, QUAD_CENTRE),
    },
    NormalQuad {
        name: "quarter turn about +Z, mirrored",
        frame: TangentFrame {
            tangent: [0.0, 1.0, 0.0],
            bitangent: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
        at: (QUAD_CENTRE, QUAD_CENTRE),
    },
    NormalQuad {
        name: "quarter turn about +X",
        frame: TangentFrame {
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
            normal: [0.0, -1.0, 0.0],
        },
        at: (-QUAD_CENTRE, -QUAD_CENTRE),
    },
    NormalQuad {
        name: "quarter turn about +X, mirrored",
        frame: TangentFrame {
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, -1.0],
            normal: [0.0, -1.0, 0.0],
        },
        at: (QUAD_CENTRE, -QUAD_CENTRE),
    },
];

/// One of [`NORMAL_QUADS`]: a frame, and where the quad carrying it stands.
///
/// The expected normal is [`TangentFrame::normal`] itself rather than a fourth
/// field — every instance transform here is a translation, which leaves a
/// normal alone, so a second copy of the vector would only be a second place to
/// mistype it.
struct NormalQuad {
    /// What the failure message calls this frame.
    name: &'static str,
    /// The frame its four corners carry.
    frame: TangentFrame,
    /// Where its quad's centre sits in the `z = 0` plane.
    at: (f32, f32),
}

/// How far a decoded normal component may sit from the one encoded.
///
/// **Not the encoding's error**, and deliberately looser than it. Every frame
/// in [`NORMAL_QUADS`] is a quarter turn, whose quaternion is two equal lanes
/// and two zeroes, and both of those survive `snorm16` and the decode's
/// renormalisation unchanged — measured, every lane of all four came back
/// exact. What the bound has to leave room for is the path *around* the
/// encoding: the debug view writes `n * 0.5 + 0.5` into an `Rgba16Float`
/// target, whose step near `0.5` is `2^-11`, undoing that doubles it, and a
/// driver whose `normalize` rounds the other way spends one of those steps. So
/// this is the target's resolution rather than the quaternion's — a bound taken
/// from `QTangent::MAX_COMPONENT_ERROR` would be a claim about the frame buffer
/// that is not true of it.
const NORMAL_TOLERANCE: f32 = 4.0 / 2048.0;

/// **A QTangent decodes on the device to the normal it was encoded from**, for
/// four frames, two of them left-handed.
///
/// Four quads side by side under
/// [`ForwardRenderer::set_normals_view`](crcbl::render::ForwardRenderer::set_normals_view),
/// which writes `normal * 0.5 + 0.5` and shades nothing — so the texel at each
/// quad's centre *is* the decoded normal, and the reading needs no light, no
/// material and no tonemap to be undone first.
///
/// The encoded normal is the only thing that differs between the four; every
/// position, index and instance transform is identical. That is what makes the
/// four readings four measurements of the decode rather than four pictures of
/// four different quads.
///
/// See this module's header for why the handedness itself is not what this
/// asserts.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_qtangent_decodes_to_its_own_normal_on_the_device() {
    let scene = quad_scene(
        NORMAL_QUADS
            .iter()
            .map(|quad| quad_mesh(quad.name, quad.frame, [[0.0, 0.0]; 4]))
            .collect(),
    );
    let frame = quad_frame(&scene, true, |renderer| {
        for (mesh, quad) in NORMAL_QUADS.iter().enumerate() {
            let (x, y) = quad.at;
            renderer
                .add_instance(&InstanceDesc {
                    mesh,
                    material: 0,
                    transform: Mat4::from_translation(Vec3::new(x, y, 0.0)),
                })
                .expect("an instance pool of thousands has room for four quads");
        }
    });

    for quad in &NORMAL_QUADS {
        let (name, expected) = (quad.name, quad.frame.normal);
        let (x, y) = quad.at;
        let (px, py) = pixel_at(x, y);
        let encoded = frame.pixel(px, py);
        let decoded = [
            encoded[0] * 2.0 - 1.0,
            encoded[1] * 2.0 - 1.0,
            encoded[2] * 2.0 - 1.0,
        ];
        for (lane, (&got, &want)) in decoded.iter().zip(&expected).enumerate() {
            let error = (got - want).abs();
            assert!(
                error <= NORMAL_TOLERANCE,
                "{name}: lane {lane} of the decoded normal at texel ({px}, {py}) is {got}, not \
                 {want} — off by {error}, over a bound of {NORMAL_TOLERANCE}; the whole normal \
                 read {decoded:?}"
            );
        }
    }
}

/// The texture coordinates the UV quad's corners carry, in [`QUAD_CORNERS`]'
/// order.
///
/// **A whole tile out and half a tile over.** The span is one unit, so the quad
/// covers the layer exactly once; the `2.5` offset is what makes the reading
/// below an answer about the range rather than about the lane. A decode that
/// dropped the offset would sample `0.0..1.0` — the same four texels, in the
/// opposite corners, because half a tile of shift exchanges them — and a decode
/// that clamped instead of tiling would sample one texel four times. An integer
/// offset would have been invisible to both.
const TILED_UVS: [[f32; 2]; 4] = [[2.5, 2.5], [3.5, 2.5], [3.5, 3.5], [2.5, 3.5]];

/// How far into the quad each sample sits, as a fraction of its edge.
///
/// A quarter and three quarters, which under [`TILED_UVS`] are the two texel
/// *centres* — `2.5 + 0.25` and `2.5 + 0.75` wrap to `0.75` and `0.25`. Centres
/// rather than anywhere else because the sampler is bilinear: half a texel from
/// an edge is the one place a reading is one texel and not a blend of two.
const SAMPLE_FRACTIONS: [f32; 2] = [0.25, 0.75];

/// The four samples of the UV quad, brightest texel first: where each one is in
/// world space and which texel of
/// [`CHECKER_TEXELS`](crcbl::render::scene::CHECKER_TEXELS) it must read.
///
/// The world coordinates are [`SAMPLE_FRACTIONS`] across a quad of
/// [`QUAD_HALF`] at the origin. Which texel each lands on is worked out in
/// [`TILED_UVS`]' doc: `v` grows with world `y` and the layer's rows are
/// row-major from `v = 0`, so the top of the quad reads row 0.
const UV_SAMPLES: [(&str, f32, f32, u8); 4] = [
    ("texel (0, 0)", 0.2, 0.2, 0xFF),
    ("texel (1, 0)", -0.2, 0.2, 0xB0),
    ("texel (0, 1)", 0.2, -0.2, 0x70),
    ("texel (1, 1)", -0.2, -0.2, 0x30),
];

/// The smallest ratio between two consecutive samples of [`UV_SAMPLES`] that
/// counts as having read two different texels.
///
/// The four texels' linear values fall by more than a factor of two at every
/// step once decoded from sRGB, and the shading multiplies all four by one
/// number — the quad is flat, unspun and lit by one directional light, so
/// nothing about the light varies between the samples. What the shading also
/// *adds* is a term that does not scale with albedo, which compresses the
/// ratios a little; measured on this scene the tightest of the three steps
/// still clears a factor of two. This is that factor: below every true step and
/// far above the `1.0` that two readings of one texel would produce.
const TEXEL_STEP: f32 = 2.0;

/// **A UV lane decodes through its mesh's range, and the sampler tiles**: a
/// quad whose coordinates run from `2.5` to `3.5` reads the four texels of the
/// layer, in the corners the offset puts them.
///
/// One quad, one draw, four texels read out of it — so nothing here can pass by
/// drawing a different quad for each reading. The three ways the decode can go
/// wrong all land on the same assertion, that the four samples fall in the
/// order [`UV_SAMPLES`] names:
///
/// * **A clamp instead of a tile** reads `(1, 1)` four times, so no two samples
///   differ at all.
/// * **A dropped offset** reads the same four texels with both axes shifted by
///   half a tile, which exchanges every sample with its diagonal opposite and
///   reverses the order exactly.
/// * **A dropped scale** puts every sample on the texel boundary, where
///   bilinear filtering returns the mean of all four and the samples are again
///   equal.
///
/// The reading is the raw `Rgba16Float` target's green channel — the layer is
/// grey, so all three channels carry the same number, and green is the one that
/// is not first or last in memory.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_uv_lane_outside_the_unit_square_tiles_through_its_range() {
    // The sample points have to be the fractions this test says they are, or
    // every expectation above is about a quad that is not the one drawn.
    for (index, fraction) in SAMPLE_FRACTIONS.iter().enumerate() {
        let inside = -QUAD_HALF + fraction * 2.0 * QUAD_HALF;
        let named = UV_SAMPLES
            .iter()
            .map(|&(_, x, _, _)| x)
            .find(|x| (x - inside).abs() < 1.0e-6);
        assert!(
            named.is_some(),
            "fraction {index} of the quad is at {inside}, which no sample in UV_SAMPLES names"
        );
    }

    let scene = quad_scene(vec![quad_mesh("tiled quad", flat_frame(), TILED_UVS)]);
    let frame = quad_frame(&scene, false, |renderer| {
        renderer
            .add_instance(&InstanceDesc {
                mesh: 0,
                material: TEXTURED_ROW,
                transform: Mat4::IDENTITY,
            })
            .expect("an instance pool of thousands has room for one quad");
    });

    let mut read = Vec::with_capacity(UV_SAMPLES.len());
    for &(name, x, y, texel) in &UV_SAMPLES {
        let (px, py) = pixel_at(x, y);
        let green = frame.pixel(px, py)[1];
        assert!(
            green > 0.0,
            "{name} read {green} at texel ({px}, {py}); nothing was drawn there"
        );
        read.push((name, texel, green));
    }

    for pair in read.windows(2) {
        let [(brighter, above, high), (darker, below, low)] = pair else {
            unreachable!("windows(2) yields pairs")
        };
        assert!(
            *high >= *low * TEXEL_STEP,
            "{brighter} read {high} and {darker} read {low}, a ratio of {} under a bound of \
             {TEXEL_STEP}; the layer's texels are {above:#04x} and {below:#04x}, which differ by \
             more than a factor of two once decoded from sRGB",
            high / low
        );
    }
}

/// A right-handed frame whose normal faces the camera, for a quad that is being
/// shaded rather than being read as a normal.
///
/// `crcbl_shaders::vertex::orthonormal_basis`' tangent rather than a written
/// one, because which tangent it is does not matter here and inventing a second
/// convention for it would.
fn flat_frame() -> TangentFrame {
    let normal = [0.0, 0.0, 1.0];
    let (tangent, bitangent) = crcbl_shaders::vertex::orthonormal_basis(normal);
    TangentFrame {
        tangent,
        bitangent,
        normal,
    }
}
