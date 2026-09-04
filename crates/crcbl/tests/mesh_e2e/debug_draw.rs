//! The debug draw layer, measured in the texels it actually wrote.
//!
//! `docs/plan/07-ui-debug.md` item 5's immediate-mode buffer draws world-space
//! segments into the HDR scene target before the tonemap reads it —
//! `docs/plan/18-render-features.md`'s interaction rule — so what a check here
//! asks is not "did the buffer get longer" but "is the linear value at this
//! texel the colour that was appended, and is the texel beside it still the
//! frame's clear". Every test below reads the `Rgba16Float` target through
//! [`render_mesh`](crate::mesh_scene::render_mesh)'s sink, which is the same
//! path `mesh_e2e/hdr.rs` measures the scene's own values on.
//!
//! # The scene is empty on purpose
//!
//! No instance is placed, no sky is set and every [`RenderEffects`] bit is
//! forced off, so the HDR target is [`SCENE_CLEAR`] everywhere a segment did not
//! land. That is what makes a texel comparison a statement about this layer:
//! with geometry in the frame, a lit surface behind a line would be one more
//! thing the value could have come from, and with the effects on, the bloom
//! chain and the reflection composite are two more.
//!
//! # The camera is orthographic, and off both axes
//!
//! Orthographic because the projection is then affine, so the twelve edges of a
//! box and the six faces of a frustum land where arithmetic on the CPU says they
//! do rather than where a second unprojection says. Off-axis because a box seen
//! down one of its own axes projects its twelve edges onto four lines and four
//! points, and a check that found "the edges" there would be finding four of
//! them.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::MESH_EXTENT;
use crate::mesh_scene::render_mesh;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    Camera, DebugDraw, EffectOverride, EffectRequest, ForwardRenderer, Projection, RenderEffects,
    SCENE_CLEAR, TransientPool,
};

/// The colour every segment in this file is appended in.
///
/// **Red above one**, which is a value only the `Rgba16Float` scene target can
/// hold: a layer that had been folded in after the tonemap, or into an `Rgba8`
/// image, would clamp it and the checks below would read `1.0`. The other two
/// channels are exact in binary16 and are what separates this from the clear.
const SEGMENT: [f32; 4] = [4.0, 0.25, 0.5, 1.0];

/// How far a channel may sit from the value the shader emitted.
///
/// The blend is `src * srcAlpha + dst * (1 - srcAlpha)` at an alpha of one, so
/// the arithmetic is exact and every channel of [`SEGMENT`] is representable in
/// binary16 — this is a floor under rounding rather than a tolerance for a
/// difference anyone expects.
const CHANNEL_EPSILON: f32 = 1.0e-3;

/// Half the world height the orthographic camera sees.
const HALF_HEIGHT: f32 = 2.0;

/// The aspect the renderer computes for a frame at [`MESH_EXTENT`], and the one
/// [`world_texel`] has to project through for its answer to be the renderer's.
fn aspect() -> f32 {
    MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32
}

/// The camera every frame in this file is drawn with: orthographic, off both
/// axes, far enough back that the shapes drawn below are in front of it.
fn oblique_camera() -> Camera {
    Camera {
        eye: Vec3::new(3.0, 2.5, 5.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::Orthographic {
            half_height: HALF_HEIGHT,
            near: 0.1,
            far: 20.0,
        },
    }
}

/// Where `at` lands in the frame, in texels, through the very matrix the
/// renderer draws with.
///
/// The y flip is the engine's own, taken from
/// `mesh_e2e/froxels.rs`'s `ndc.1 = 1.0 - pixel.1 / height * 2.0`: normalised
/// device `y = +1` is the **top** row, which the inverted Vulkan viewport is
/// what makes true.
fn world_texel(camera: &Camera, at: Vec3) -> (f32, f32) {
    let clip = camera.view_projection(aspect()) * at.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    (
        (ndc.x * 0.5 + 0.5) * MESH_EXTENT.0 as f32,
        (1.0 - ndc.y) * 0.5 * MESH_EXTENT.1 as f32,
    )
}

/// One frame of whatever `place` put in the scene and whatever `append` put in
/// the debug buffer, and the `Rgba16Float` target it wrote, read back.
///
/// `place` is [`nothing`] for every check whose subject is the layer alone —
/// see the module docs on why the scene is empty for those — and
/// [`place_cube_at`](crate::mesh_scene::place_cube_at) for the one whose subject
/// is the depth test, which needs a surface to be hidden behind.
///
/// **The console switch is set here rather than left to the caller**, because a
/// frame drawn with it off is the frame this layer does not draw and every check
/// below would then be measuring the clear. `mesh_e2e` runs under `cargo
/// nextest`, which gives each test a process of its own, so the switch is this
/// process's alone — `crates/crcbl-render/src/forward.rs`'s own check carries the
/// same note.
fn debug_frame(
    on: bool,
    place: impl FnOnce(&mut ForwardRenderer),
    append: impl FnOnce(&mut DebugDraw),
) -> (crcbl_golden::Image, HdrTarget) {
    crcbl::render::debug_draw::r_debug_draw
        .set(&crcbl::console::Value::Bool(on))
        .expect("`r_debug_draw` is a writable bool");

    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    // Every effect off, so the HDR target the probe reads is the forward pass's
    // own clear with this layer's segments over it and nothing else in between —
    // see the module docs.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::all(), Some(false)),
        ..EffectRequest::default()
    });
    place(&mut renderer);
    append(renderer.debug_draw());

    let mut hdr = Vec::new();
    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &oblique_camera(),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    assert_eq!(
        hdr.len(),
        (MESH_EXTENT.0 * MESH_EXTENT.1 * 8) as usize,
        "the scene target came back the wrong size, so every value read out of it is at the \
         wrong offset"
    );
    (image, HdrTarget(hdr))
}

/// An empty scene, for the frames whose subject is the debug layer alone.
///
/// Named rather than written as `|_| {}` at each call site, so the one frame
/// that *does* place something reads as the exception it is.
fn nothing(_: &mut ForwardRenderer) {}

/// Whether the texel at `(x, y)` carries [`SEGMENT`].
fn is_segment(frame: &HdrTarget, x: u32, y: u32) -> bool {
    let pixel = frame.pixel(x, y);
    pixel
        .iter()
        .zip(SEGMENT)
        .all(|(value, wanted)| (value - wanted).abs() <= CHANNEL_EPSILON)
}

/// Whether the texel at `(x, y)` is the frame's clear — no segment landed on it.
fn is_clear(frame: &HdrTarget, x: u32, y: u32) -> bool {
    let pixel = frame.pixel(x, y);
    pixel
        .iter()
        .zip(SCENE_CLEAR)
        .all(|(value, wanted)| (value - wanted).abs() <= CHANNEL_EPSILON)
}

/// Whether a segment landed within one texel of where `at` projects.
///
/// **One texel, and that is the rasteriser's freedom rather than a tolerance
/// for being wrong.** A line between two projected endpoints covers the texels
/// whose centres it passes through, and the four rasterisers this suite runs on
/// resolve a centre on a boundary differently; a point *two* texels from where
/// the projection puts it is a segment somewhere else.
fn segment_near(frame: &HdrTarget, camera: &Camera, at: Vec3) -> bool {
    let (u, v) = world_texel(camera, at);
    let (u, v) = (u as i32, v as i32);
    for y in (v - 1)..=(v + 1) {
        for x in (u - 1)..=(u + 1) {
            if (0..MESH_EXTENT.0 as i32).contains(&x)
                && (0..MESH_EXTENT.1 as i32).contains(&y)
                && is_segment(frame, x as u32, y as u32)
            {
                return true;
            }
        }
    }
    false
}

/// The eight corners of the axis-aligned box the checks below draw, in
/// [`crcbl::render::frustum_corners`]' own corner order — bit 0 is the x end,
/// bit 1 the y end, bit 2 the z end.
fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    let mut corners = [Vec3::ZERO; 8];
    for (corner, out) in corners.iter_mut().enumerate() {
        *out = Vec3::new(
            if corner & 1 != 0 { max.x } else { min.x },
            if corner & 2 != 0 { max.y } else { min.y },
            if corner & 4 != 0 { max.z } else { min.z },
        );
    }
    corners
}

/// A box's twelve edges as pairs of [`box_corners`] indices: two corners share
/// an edge exactly when their indices differ in one bit.
fn box_edges() -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for from in 0usize..8 {
        for to in (from + 1)..8 {
            if (from ^ to).count_ones() == 1 {
                edges.push((from, to));
            }
        }
    }
    edges
}

/// **A line puts its colour on the row it projects to, and on neither row
/// beside it.**
///
/// The sharpest claim in this file and the one every other check rests on: a
/// segment whose two endpoints share a world `y` and a world `z` under this
/// camera projects to a horizontal run of texels, so the row is arithmetic and
/// the two rows around it are the control. A layer that drew nothing leaves the
/// row clear; one that drew into the wrong image leaves it clear too; one whose
/// vertex stage lost the camera puts the colour on some other row, which the
/// controls catch.
///
/// The world `y` is derived from the row rather than chosen, so the line's
/// centre is a texel centre and every rasteriser resolves it the same way. The
/// ends are left out of the span that is asserted, because the diamond-exit rule
/// D3D12 uses and the parallelogram rule Vulkan uses legitimately disagree about
/// a line's last texel.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_line_lands_on_the_row_it_projects_to_and_not_beside_it() {
    /// The row the segment is aimed at: the middle of the frame.
    const ROW: u32 = MESH_EXTENT.1 / 2;

    let camera = oblique_camera();
    // The camera is off-axis, so a world segment along one axis is not
    // horizontal on screen. What is horizontal is a segment along the camera's
    // own right vector at a fixed height in *its* frame, so the two endpoints
    // are built out of the view basis rather than out of world axes.
    let inverse_view = camera.view().inverse();
    // Where the row sits in the view plane: the projection is orthographic, so
    // normalised device `y` scales straight to view-space `y`.
    let ndc_y = 1.0 - 2.0 * (ROW as f32 + 0.5) / MESH_EXTENT.1 as f32;
    let view_y = ndc_y * HALF_HEIGHT;
    let at = |x: f32| inverse_view.transform_point3(Vec3::new(x, view_y, -5.0));
    let half_width = HALF_HEIGHT * aspect();
    let (from, to) = (at(-0.6 * half_width), at(0.6 * half_width));

    let (_, frame) = debug_frame(true, nothing, |draw| draw.line(from, to, SEGMENT));

    let (start, _) = world_texel(&camera, from);
    let (end, _) = world_texel(&camera, to);
    let (first, last) = (start.min(end).ceil() as u32 + 2, start.max(end) as u32 - 2);
    assert!(
        last > first + 8,
        "the segment covers {first}..={last}, which is too short a run to say anything about"
    );
    for x in first..=last {
        assert!(
            is_segment(&frame, x, ROW),
            "texel ({x}, {ROW}) is {:?}, not the segment's {SEGMENT:?}",
            frame.pixel(x, ROW),
        );
        for row in [ROW - 1, ROW + 1] {
            assert!(
                is_clear(&frame, x, row),
                "texel ({x}, {row}) is {:?}, and the row beside the segment must still be the \
                 frame's clear {SCENE_CLEAR:?}",
                frame.pixel(x, row),
            );
        }
    }
}

/// **A box is twelve edges and no face**: every one of the twelve lands where it
/// projects, and the middle of the box is still the frame's clear.
///
/// Twelve *distinct* texels are asserted first, because a camera that projected
/// two edges onto one place would make this pass with eleven of them drawn. The
/// hollow centre is the other half: a helper that emitted triangles rather than
/// segments would light every one of the twelve midpoints and fill the middle
/// too.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_box_draws_its_twelve_edges_and_leaves_its_middle_clear() {
    let (min, max) = (Vec3::new(-1.0, -0.8, -0.6), Vec3::new(1.0, 0.8, 0.6));
    let camera = oblique_camera();
    let corners = box_corners(min, max);
    let edges = box_edges();
    assert_eq!(edges.len(), 12, "a box has twelve edges to look for");

    // Anti-vacuity: twelve midpoints that landed on fewer than twelve places
    // would make the assertions below a claim about however many places there
    // were.
    let mut places: Vec<(i32, i32)> = edges
        .iter()
        .map(|(from, to)| {
            let (u, v) = world_texel(&camera, (corners[*from] + corners[*to]) * 0.5);
            (u as i32, v as i32)
        })
        .collect();
    places.sort_unstable();
    let distinct = places.len();
    places.dedup();
    assert_eq!(
        places.len(),
        distinct,
        "two of the box's edges project to one texel under this camera, so this would find \
         fewer than twelve edges and pass"
    );

    let (_, frame) = debug_frame(true, nothing, |draw| draw.aabb(min, max, SEGMENT));

    for (from, to) in edges {
        let middle = (corners[from] + corners[to]) * 0.5;
        assert!(
            segment_near(&frame, &camera, middle),
            "the edge from corner {from} to corner {to} is not drawn: its midpoint {middle:?} \
             projects to {:?} and no texel within one of it carries {SEGMENT:?}",
            world_texel(&camera, middle),
        );
    }

    let (u, v) = world_texel(&camera, (min + max) * 0.5);
    assert!(
        is_clear(&frame, u as u32, v as u32),
        "the middle of the box is {:?}; a box is an outline and its faces are not filled",
        frame.pixel(u as u32, v as u32),
    );
}

/// **A frustum's six faces are each closed**, which is the claim twelve
/// unattributed edges cannot make.
///
/// Every face is asked for by name: its four edges are the four pairs of its
/// corners that differ in one bit, and all four have to be drawn. A layer that
/// dropped the four edges along one axis would still draw eight segments and
/// would leave two faces open, which is what this finds and a count does not.
///
/// The view volume is a finite one — an orthographic box — because the engine's
/// own camera has no far plane to draw; `crcbl_render::frustum_corners` refuses
/// that one and `crcbl-render`'s unit tests are where the refusal is asserted.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_frustums_six_faces_are_each_drawn_closed() {
    // A view volume of its own, not the camera's: `world_texel` projects with
    // the camera, and a frustum drawn through the camera's own matrix would be
    // the frame's own edges.
    let volume = Camera {
        eye: Vec3::new(0.0, 0.0, 1.2),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::Orthographic {
            half_height: 0.7,
            near: 0.2,
            far: 1.6,
        },
    };
    let view_proj = volume.view_projection(1.4);
    let corners = crcbl::render::frustum_corners(view_proj).expect("a finite view volume");

    let camera = oblique_camera();
    let (_, frame) = debug_frame(true, nothing, |draw| draw.frustum(view_proj, SEGMENT));

    let mut faces = 0;
    for axis in 0..3 {
        for end in [0usize, 1] {
            let bit = 1 << axis;
            let on_face: Vec<usize> = (0..8)
                .filter(|corner| usize::from(corner & bit != 0) == end)
                .collect();
            assert_eq!(on_face.len(), 4, "a face of a box has four corners");
            let mut drawn = 0;
            for &from in &on_face {
                for &to in &on_face {
                    if from >= to || (from ^ to).count_ones() != 1 {
                        continue;
                    }
                    let middle = (corners[from] + corners[to]) * 0.5;
                    assert!(
                        segment_near(&frame, &camera, middle),
                        "the face at axis {axis} end {end} is open: its edge {from}–{to} has no \
                         segment within one texel of {:?}",
                        world_texel(&camera, middle),
                    );
                    drawn += 1;
                }
            }
            assert_eq!(drawn, 4, "a face is closed by four edges");
            faces += 1;
        }
    }
    assert_eq!(faces, 6, "a view volume has six faces");
}

/// **A sphere is drawn on its three great circles and is hollow.**
///
/// Sampled at angles between the vertices rather than at them, so what is
/// checked is the segment and not the endpoint a vertex would have put there
/// anyway. Each ring is asked for separately: three rings that had collapsed
/// into one would light a third of these points.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_sphere_is_drawn_on_three_rings_and_is_hollow() {
    let (centre, radius) = (Vec3::new(0.0, 0.0, 0.0), 1.2);
    let camera = oblique_camera();
    let (_, frame) = debug_frame(true, nothing, |draw| draw.sphere(centre, radius, SEGMENT));

    // Between two vertices of the ring, so the texel asked about belongs to a
    // segment rather than to an endpoint.
    let segments = crcbl::render::debug_draw::SPHERE_SEGMENTS;
    for ring in 0..3 {
        for step in 0..8 {
            let turn = (step as f32 + 0.5) / segments as f32;
            let angle = std::f32::consts::TAU * turn;
            let (sin, cos) = angle.sin_cos();
            let offset = match ring {
                0 => Vec3::new(cos, sin, 0.0),
                1 => Vec3::new(0.0, cos, sin),
                _ => Vec3::new(sin, 0.0, cos),
            };
            let at = centre + offset * radius;
            assert!(
                segment_near(&frame, &camera, at),
                "ring {ring} has nothing at {at:?}, which projects to {:?}",
                world_texel(&camera, at),
            );
        }
    }

    let (u, v) = world_texel(&camera, centre);
    assert!(
        is_clear(&frame, u as u32, v as u32),
        "the middle of the sphere is {:?}; three rings are an outline and do not fill it",
        frame.pixel(u as u32, v as u32),
    );
}

/// **A frame that appends nothing is the frame the layer's absence draws** —
/// byte for byte, in both the HDR target and the tonemapped image.
///
/// This is the "costs nothing when empty" claim, and it is the reason every
/// golden image in the workspace was left alone by this rung. The comparison is
/// of raw bytes rather than of a tolerance, because there is nothing here for a
/// tolerance to absorb: the two frames are meant to be the *same* commands.
///
/// The third frame is the anti-vacuity half: a comparison of two frames that
/// were both empty would pass identically if the layer could never draw at all,
/// so a frame that did append is compared against them and has to differ.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_empty_buffer_draws_the_frame_the_layer_absent_draws() {
    let (absent_image, absent) = debug_frame(false, nothing, |draw| {
        // Appended with the switch off, which is the case a system that appends
        // unconditionally produces every frame.
        draw.aabb(-Vec3::ONE, Vec3::ONE, SEGMENT);
    });
    let (empty_image, empty) = debug_frame(true, nothing, |_| {});
    assert_eq!(
        empty.0, absent.0,
        "the switch on with an empty buffer wrote a different HDR frame from the one the layer \
         switched off wrote"
    );
    assert_eq!(
        empty_image.pixels(),
        absent_image.pixels(),
        "the switch on with an empty buffer wrote a different tonemapped frame"
    );

    let (drawn_image, drawn) = debug_frame(true, nothing, |draw| {
        draw.aabb(-Vec3::ONE, Vec3::ONE, SEGMENT);
    });
    assert_ne!(
        drawn.0, empty.0,
        "a frame that did append a box is identical to one that appended nothing, so the two \
         above agree about a layer that never draws"
    );
    assert_ne!(
        drawn_image.pixels(),
        empty_image.pixels(),
        "the appended box reached the HDR target and not the tonemapped frame, so the pass is \
         after the tonemap rather than before it"
    );
}

/// **A segment behind an opaque surface is hidden by it, and the same segment in
/// front of that surface is not.**
///
/// The layer's pipeline is depth-tested and does not write depth —
/// `crcbl_render::debug_draw` says so in as many words, and the changelog
/// repeats it — and until this check nothing anywhere asked. Changing
/// `depth_compare` to `CompareOp::Always` left the whole suite green, which is
/// the shape this repo calls a check that cannot fail.
///
/// **Both halves, because either alone passes against something broken.** A
/// check that only looked at the hidden segment would pass against a layer that
/// draws nothing at all — which is the failure mode next door. A check that only
/// looked at the visible one would pass against a layer that ignores depth
/// entirely, which is the failure this exists for. The same texels are read in
/// both frames and the segment is at the same place on screen in both, so the
/// only difference between them is which side of the surface it is on.
///
/// # Where "behind" comes from
///
/// **Off the scene rather than off a depth value.** The engine's clip space is
/// reversed-Z and this pipeline's pass condition is `Greater`, so "behind" is
/// the *smaller* depth — and a number computed here with that sign backwards
/// would give a check that passed for the wrong reason. So the two positions are
/// the cube's own: the camera looks at the origin and the cube is placed there,
/// so the surface sits at the eye's own distance along the view axis, and the
/// two segments are that distance `DEPTH_CLEARANCE` nearer the eye and the same
/// again further from it. The clearance is far more than the cube's
/// half-diagonal, so neither segment is ever inside it.
///
/// The segment is horizontal in screen space, for the reason
/// `a_line_lands_on_the_row_it_projects_to_and_not_beside_it` gives, and its
/// span is short enough to stay well inside the cube's silhouette — which the
/// surface frame is asked about texel by texel rather than assumed.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_segment_behind_a_surface_is_hidden_and_one_in_front_is_not() {
    /// Half the segment's length along the camera's right vector, in world
    /// units.
    ///
    /// The cube spans half a metre either side of the origin, so this is a run
    /// well inside its silhouette from any angle — and the surface frame below
    /// asserts that texel by texel rather than trusting it.
    const HALF_SPAN: f32 = 0.25;
    /// How far along the view axis each segment sits from the cube's centre.
    ///
    /// Comfortably past the cube's half-diagonal, so a segment is wholly in
    /// front of the surface or wholly behind it and never inside.
    const DEPTH_CLEARANCE: f32 = 2.0;

    let camera = oblique_camera();
    let inverse_view = camera.view().inverse();
    // Where the cube's centre sits along the camera's view axis: the camera
    // looks at the origin and the cube is placed there, so this is the distance
    // between the two.
    let cube_view_depth = -(camera.eye - camera.target).length();

    // The row through the cube's centre, and the view-space height that puts
    // the segment on that row's centre — the derivation
    // `a_line_lands_on_the_row_it_projects_to_and_not_beside_it` uses, exactly.
    let (_, centre_v) = world_texel(&camera, Vec3::ZERO);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a texel coordinate inside a 256x192 frame"
    )]
    let row = centre_v as u32;
    let ndc_y = 1.0 - 2.0 * (row as f32 + 0.5) / MESH_EXTENT.1 as f32;
    let view_y = ndc_y * HALF_HEIGHT;
    // Both segments at the same screen position and different depths: the
    // projection is orthographic, so moving along the view axis moves nothing on
    // screen, and the two ends of each segment differ only in the camera's own
    // right direction.
    let at = |x: f32, depth: f32| inverse_view.transform_point3(Vec3::new(x, view_y, depth));
    let behind = (
        at(-HALF_SPAN, cube_view_depth - DEPTH_CLEARANCE),
        at(HALF_SPAN, cube_view_depth - DEPTH_CLEARANCE),
    );
    let front = (
        at(-HALF_SPAN, cube_view_depth + DEPTH_CLEARANCE),
        at(HALF_SPAN, cube_view_depth + DEPTH_CLEARANCE),
    );
    // Said rather than assumed: if the two landed on different texels, the two
    // halves below would be reading different pixels and the comparison would
    // be about the projection rather than about the depth test.
    for (behind_end, front_end) in [(behind.0, front.0), (behind.1, front.1)] {
        let (bu, bv) = world_texel(&camera, behind_end);
        let (fu, fv) = world_texel(&camera, front_end);
        assert!(
            (bu - fu).abs() < 1.0e-3 && (bv - fv).abs() < 1.0e-3,
            "the two segments project to {:?} and {:?}, so they differ on screen as well as in \
             depth",
            (bu, bv),
            (fu, fv),
        );
    }

    let cube = |renderer: &mut ForwardRenderer| {
        crate::mesh_scene::place_cube_at(renderer, Mat4::IDENTITY);
    };
    let (_, surface) = debug_frame(false, cube, |_| {});
    let (_, hidden) = debug_frame(true, cube, |draw| {
        draw.line(behind.0, behind.1, SEGMENT);
    });
    let (_, shown) = debug_frame(true, cube, |draw| {
        draw.line(front.0, front.1, SEGMENT);
    });

    let (start, _) = world_texel(&camera, front.0);
    let (end, _) = world_texel(&camera, front.1);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "texel coordinates inside a 256x192 frame"
    )]
    let (first, last) = (start.min(end).ceil() as u32 + 1, start.max(end) as u32 - 1);
    assert!(
        last > first + 8,
        "the segment covers {first}..={last}, which is too short a run to say anything about"
    );

    for x in first..=last {
        // The surface has to be there, or "hidden" would mean "hidden behind
        // the frame's clear" and the check would be about nothing.
        assert!(
            !is_clear(&surface, x, row),
            "texel ({x}, {row}) is the frame's clear, so the cube does not cover the run this \
             asks about and neither half below is a claim about occlusion"
        );
        assert!(
            !is_segment(&surface, x, row),
            "the cube shades to the segment's own colour at texel ({x}, {row}), so the two \
             halves below cannot be told apart"
        );
        assert_eq!(
            hidden.pixel(x, row),
            surface.pixel(x, row),
            "texel ({x}, {row}) moved when a segment was appended *behind* the cube: the layer \
             is drawing through solid geometry, so its depth test is not running"
        );
        assert!(
            is_segment(&shown, x, row),
            "texel ({x}, {row}) is {:?} with the segment in front of the cube, not the \
             segment's {SEGMENT:?} — the depth test is rejecting what it should pass",
            shown.pixel(x, row),
        );
    }
}

/// How many boxes a priced frame appends.
///
/// **A busy buffer rather than a token one**: at twelve segments a box this is
/// the overlay a cluster-bounds or light-reach view would put on screen at once,
/// and it is what makes the measurement about the vertices rather than about
/// pass overhead. Measured on lavapipe on 2026-08-31, a tenth as many put the
/// pass inside the run-to-run spread of its own repeats.
const PRICE_BOXES: usize = 1024;

/// The passes the price is read off, by the label the render graph gives each.
///
/// The forward pass is beside the layer's own so the number has something on
/// the same device and the same frame to be read against — a millisecond figure
/// on its own is a property of the machine.
const PRICED_PASSES: [&str; 2] = ["forward", "debug-draw"];

/// The configurations priced, as boxes each frame appends.
///
/// The busy buffer first and the empty one second, because what is asked below
/// is between them. The second row is the layer's off-switch — a buffer with
/// nothing in it records no pass for the graph to time — and it is at the same
/// time the zero-geometry row `forward`'s fused attachment clears are read off,
/// at the same extent through the same effect stack with nothing appended over
/// them.
const PRICED_BUFFERS: [usize; 2] = [PRICE_BOXES, 0];

/// One configuration's measurement.
struct Priced {
    /// How many frames reached [`crcbl::render::PassStats`], which is not how
    /// many were drawn: the timer ring hands the same report back until a new
    /// slot resolves, and a repeat is not a second sample.
    recorded: u64,
    /// Each of [`PRICED_PASSES`]' p50 and p95 in nanoseconds, in that order.
    ///
    /// A pass that was **never recorded** — which is what an empty buffer
    /// produces — has no percentiles, so its slot is [`None`] while the forward
    /// pass beside it is measured. That is the shape the empty-buffer claim is
    /// read out of, and it is why this is an option per pass rather than a
    /// refusal of the whole row.
    passes: Vec<Option<(u64, u64)>>,
}

/// Each of [`PRICED_BUFFERS`]' measurement, in that order, or [`None`] where the
/// device reports no way to time a pass.
///
/// `depth_only.rs`'s helper is the shape this follows, down to the warm-up, the
/// percentile floor and the interleaving — the first two are `area_light.rs`'s
/// constants rather than a second copy. **The configurations are drawn
/// interleaved on one device, a frame each per turn**, for that file's reason:
/// the suite runs its tests at once and a software rasteriser's "GPU" time is
/// CPU time, so a run measured configuration after configuration reads whatever
/// else was on the machine during that configuration's turn. The comparison
/// between the passes of one frame — the layer against the forward pass beside
/// it — needs none of that, because contention that lands on one lands on both;
/// anything read between the two rows is read between frames, and does.
fn debug_draw_prices(extent: (u32, u32), frames: usize) -> Option<[Priced; PRICED_BUFFERS.len()]> {
    use crcbl::hal::{CommandEncoderDesc, Features, PresentInfo, ResourceState, SubmitInfo};

    crcbl::render::debug_draw::r_debug_draw
        .set(&crcbl::console::Value::Bool(true))
        .expect("`r_debug_draw` is a writable bool");

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    // Asked for rather than required, on `depth_only.rs`'s terms: a backend that
    // cannot time a pass cannot price one, and the frames are drawn either way.
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let camera = oblique_camera();
    let sun = crcbl::render::DirectionalLight::default();
    let mut priced = PRICED_BUFFERS.map(|_| {
        let renderer = ForwardRenderer::new(device, headless.queue, headless.format)
            .expect("the forward renderer builds");
        let timers = timed.then(|| {
            crcbl::render::PassTimers::new(
                device,
                crcbl::render::forward::FRAMES_IN_FLIGHT,
                crcbl::render::MAX_TIMED_PASSES,
            )
            .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
        });
        (
            renderer,
            TransientPool::new(),
            timers,
            crcbl::render::PassStats::new(),
            Vec::new(),
        )
    });

    for index in 0..crate::area_light::PRICE_WARMUP + frames {
        for (boxes, (renderer, pool, timers, stats, recorded)) in
            PRICED_BUFFERS.iter().zip(&mut priced)
        {
            // Appended before the frame opens, because `begin_frame` is what
            // uploads and clears the buffer — the whole of what "immediate
            // mode" means here. The empty row appends nothing and is the same
            // code with nothing to iterate.
            let draw = renderer.debug_draw();
            for step in 0..*boxes {
                let at = Vec3::splat(step as f32 * 0.01);
                draw.aabb(at - Vec3::ONE, at + Vec3::ONE, SEGMENT);
            }
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("the ring always has an image");
            renderer
                .begin_frame(device, &camera, &sun, extent)
                .expect("the uniform buffer is writable");
            let compiled = {
                let mut graph = crcbl::render::RenderGraph::new(headless.queue);
                let target = graph.import_image(
                    "swapchain",
                    crcbl::render::ImportedImage {
                        image: acquired.image,
                        view: acquired.view,
                        format: headless.format,
                        extent,
                        initial: ResourceState::Undefined,
                        claim: crcbl::render::InitialClaim::Acquired,
                        final_state: ResourceState::Present,
                    },
                );
                renderer.add_passes(&mut graph, &*pool, target, extent);
                graph.compile(&*pool).expect("a legal frame")
            };
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("priced frame"),
                queue: headless.queue,
            });
            compiled
                .execute(device, pool, encoder.as_mut(), timers.as_mut())
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
            if let (true, Some(timers)) =
                (index >= crate::area_light::PRICE_WARMUP, timers.as_ref())
            {
                stats.record(timers.latest());
            }
            recorded.push(commands);
        }
    }

    device.wait_idle().expect("idle");
    // **What makes the second configuration a floor rather than a second copy of
    // the first**, asked of the renderers and not of the clock — a duration read
    // off a busy machine cannot tell the two apart, and this can.
    // `FrameCounters::instances` is the live instances the cull dispatches were
    // handed plus the frame's direct draws, and neither row places an instance,
    // so what is left is the direct draws: the full-screen passes' triangles,
    // which both configurations record alike through the same effect stack, and
    // the layer's own line list, which only a frame with a segment in it
    // records. So a busy row that appended nothing, or a floor row that appended
    // anything, fails here instead of reporting a floor that is not one.
    let submitted = std::array::from_fn::<_, { PRICED_BUFFERS.len() }, _>(|index| {
        let (renderer, ..) = &priced[index];
        renderer.counters().instances
    });
    let [busy, empty] = submitted;
    assert_eq!(
        busy,
        empty + 1,
        "the busy buffer submitted {busy} instances and the empty one {empty}, which do not \
         differ by the one line-list draw that is the only thing between them"
    );
    let prices = timed.then(|| {
        std::array::from_fn(|index| {
            let (_, _, _, stats, _) = &priced[index];
            eprintln!("{}: {}", crate::SUITE, stats.report());
            Priced {
                recorded: stats.frames(),
                passes: PRICED_PASSES
                    .iter()
                    .map(|pass| stats.percentiles(pass))
                    .collect(),
            }
        })
    });
    for (renderer, mut pool, timers, _, recorded) in priced {
        if let Some(mut timers) = timers {
            timers.destroy(device);
        }
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
    prices
}

/// **The rung's price**: what a busy overlay costs beside the forward pass over
/// the same frame, and what an empty buffer costs.
///
/// Prints rather than asserts a duration, for `depth_only.rs`'s reason — a
/// millisecond figure is a property of the machine it was measured on. What it
/// asserts is the shape, and the shape is the claim this rung was scheduled on:
///
/// * **A busy buffer is measured**, so the number printed beside it is about
///   work that happened.
/// * **An empty buffer has no pass to time at all.** Not a small number: the
///   graph records nothing, so `PassStats` has never seen the label and answers
///   [`None`] — while the forward pass in the very same frames is measured,
///   which is what says the run happened and the timers worked.
///
/// # The empty row is the clears, and so is the row above it
///
/// `forward` opens by clearing the scene colour, the reflectivity target and the
/// motion target over the whole extent. Each is a `LoadOp::Clear` fused into the
/// pass's begin, so no timestamp can be put around them without giving them a
/// pass and a second full-target write of their own, and every millisecond
/// printed for `forward` has them in it. `depth_only.rs` separates them from a
/// draw with a second **configuration** rather than a second timestamp, and
/// [`PRICED_BUFFERS`]' empty row is that shape here: the same extent, the same
/// effect stack, nothing appended.
///
/// **What it shows is that the row above it is already the same measurement**,
/// which is the reason it is printed rather than folded into the busy row's
/// sentence. Neither configuration places an instance — every frame this file
/// draws is the layer over an empty scene — so `forward` records the clears and
/// the pass's begin and no draw at all in both rows, and the layer, which
/// records a pass of its own further down the frame, adds nothing to it.
/// Measured at 640x480 over 48 recorded frames on 2026-09-05, the two rows'
/// `forward` p50s landed within 80 ns of each other on an RX 7900 XTX and
/// within a tenth of a millisecond on lavapipe, in *both* directions over three
/// runs each. So the empty row is asked only for having been **measured** —
/// frames reached the accumulator and the forward pass came back with a
/// duration, where a row of zeroes would report a floor nothing observed — and
/// an ordering between the two figures would be an assertion about which way a
/// coin landed. What says the row is really empty is the instance counts the
/// helper compares, which need no timestamps at all.
///
/// # A backend that cannot time a pass cannot price one
///
/// CI's Apple Paravirtual device reports no `TIMESTAMP_QUERY`. The frames are
/// still drawn there and the price is reported as unmeasured rather than passing
/// quietly.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh price"]
fn the_price_of_the_debug_draw_layer() {
    let (extent, frames) = crate::area_light::price_frame();
    let Some([busy, empty]) = debug_draw_prices(extent, frames) else {
        eprintln!(
            "{}: {PRICE_BOXES} boxes drew through the debug draw layer and this backend reports \
             no TIMESTAMP_QUERY, so the rung's price went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let ms = |nanos: u64| nanos as f64 / 1.0e6;

    let [busy_forward, busy_layer] = busy.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let [empty_forward, empty_layer] = empty.passes[..]
        .try_into()
        .expect("one price per priced pass");
    let busy_forward = busy_forward.expect("the forward pass is in every frame");
    let busy_layer =
        busy_layer.expect("a frame that appended boxes records the layer's pass and times it");
    let empty_forward = empty_forward.expect("the forward pass is in every frame");
    eprintln!(
        "{}: {PRICE_BOXES} boxes ({} segments) at {}x{} over {} recorded frames — debug draw \
         {:.3}/{:.3} ms against the forward pass's {:.3}/{:.3} ms (p50/p95)",
        crate::SUITE,
        PRICE_BOXES * 12,
        extent.0,
        extent.1,
        busy.recorded,
        ms(busy_layer.0),
        ms(busy_layer.1),
        ms(busy_forward.0),
        ms(busy_forward.1),
    );
    eprintln!(
        "{}: an empty buffer at {}x{} over {} recorded frames — the layer records no pass at all \
         and forward costs {:.3}/{:.3} ms (p50/p95), which is the clear-plus-pass-begin floor \
         the row above is standing on and, with no instance placed in either row, is also all \
         that row's forward figure was",
        crate::SUITE,
        extent.0,
        extent.1,
        empty.recorded,
        ms(empty_forward.0),
        ms(empty_forward.1),
    );

    // Anti-vacuity first: timestamps that came back as zeroes would satisfy
    // everything below without having measured anything, and so would an empty
    // row no frame ever reached.
    assert!(
        busy_layer.0 > 0 && busy_forward.0 > 0 && empty_forward.0 > 0,
        "a pass that took no time at all was not measured"
    );
    assert!(
        empty.recorded > 0,
        "the empty buffer reached {} recorded frames, so the figure it reports was not measured",
        empty.recorded,
    );
    assert!(
        empty_layer.is_none(),
        "an empty buffer was timed at {:?}, so the frame recorded a pass for it — the layer is \
         not free when nothing is appended",
        empty_layer,
    );
}
