//! The motion-vector pass, read back through the debug view that encodes it.
//!
//! `docs/plan/43-render-standards.md` §9's subtraction: where a fragment is now,
//! minus where it was last frame, in texture coordinates. Two things feed it and
//! both belong to the renderer rather than to a caller — `GpuInstance`'s
//! previous transform, which `crates/crcbl-render/src/instance_pool.rs` fills,
//! and `FrameUniforms::previous_view_proj`, which
//! `crates/crcbl-render/src/forward.rs` advances — so nothing here writes either
//! and every test below is about what the engine did on its own.
//!
//! # Why this reads the scene target and not the motion target
//!
//! The forward pass writes the vector into a third colour attachment, and
//! **nothing reads that attachment yet**: `docs/plan/49-antialiasing.md`'s TAA
//! is the first pass that will, and it is the attachment's own observer when it
//! arrives. What can be checked today is the arithmetic that fills it, and
//! `ForwardRenderer::set_motion_view` is what puts that arithmetic somewhere a
//! frame can be read out of — the same vector, encoded into the `Rgba16Float`
//! scene target, which `crates/crcbl/tests/mesh_e2e/hdr.rs` already has the
//! readback for.
//!
//! # Telling a covered texel from the background
//!
//! The encoding writes zero into blue and the clear does not, which is the whole
//! of [`covered`]. It needs no second frame and no geometry mask: a texel a
//! fragment wrote is one whose blue channel is exactly zero.
//!
//! # The numbers here were measured
//!
//! Every magnitude and every population floor in this file came from a run on
//! radv and is quoted in the constant or the test that uses it. There is no
//! exact comparison anywhere in the file, and [`REST_TOLERANCE`] is why.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place, render_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    Camera, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc, InstanceHandle,
    Projection, RenderEffects, SCENE_CLEAR, TransientPool,
};
use crcbl_shaders::mesh::MOTION_VIEW_SCALE;

/// What the motion view encodes a fragment that did not move as, in red and
/// green: the shader's `motion * MOTION_VIEW_SCALE + 0.5` with a zero motion.
const REST: f32 = 0.5;

/// The gap between [`REST`] and the half-float immediately below it.
///
/// A half's significand is ten bits and `0.5` sits on a power-of-two boundary,
/// so the values just under it are spaced this far apart — one step, and the
/// smallest disagreement the scene target can carry there.
const HALF_STEP_BELOW_REST: f32 = 1.0 / 4096.0;

/// How far a fragment that did not move may still read from [`REST`], in the
/// texture coordinates [`decode`] answers in.
///
/// **A fragment at rest is not exactly at rest, and the target's rounding is
/// why.** The subtraction is two clip positions the same instructions produced
/// from equal matrices and an equal vertex, so it comes out at zero or within a
/// last-bit of it; the encoding then adds `0.5` and the colour export converts
/// to half. Measured on radv over a still frame's 5851 covered texels: 3681 read
/// exactly [`REST`] and the other 2170 read exactly one
/// [`HALF_STEP_BELOW_REST`] under it, never over — the signature of an export
/// that rounds toward zero, under which a motion of any negative size at all
/// drops a step and a positive one does not.
///
/// So one step is the floor, and it is a tight one: at this frame's width it is
/// [`MESH_EXTENT`]`.0 * REST_TOLERANCE` ≈ 0.008 texels of screen movement, three
/// orders below the smallest thing any test here asserts.
const REST_TOLERANCE: f32 = HALF_STEP_BELOW_REST / MOTION_VIEW_SCALE;

/// Where the instance that moves stands: the point the camera looks at, so its
/// motion is a translation across the screen rather than a magnification about
/// the frame's centre.
const MOVING_AT: Vec3 = Vec3::ZERO;

/// Where the instance that stands still stands, far enough along `-X` to land in
/// its own part of the frame — [`nearer_first`] is what tells the two apart, and
/// two overlapping silhouettes would give it nothing to tell.
///
/// Measured on radv: the two centres project 71 texels apart and the
/// classification splits the frame's covered texels 3737 to 2298 with no
/// crossover — every texel on the moving side read a motion and every texel on
/// the other side read rest.
const STILL_AT: Vec3 = Vec3::new(-2.4, 0.0, 0.0);

/// How far the moving instance moves between two frames, in world units along
/// `+X`.
///
/// Measured on radv: this puts the projected centre 0.0372 of the frame's width
/// to the right, which is three orders above [`REST_TOLERANCE`] and still leaves
/// the instance on its own side of the frame.
const MOVE_BY: Vec3 = Vec3::new(0.25, 0.0, 0.0);

/// How far the moving instance moves between two frames, in world units along
/// `+Y`, for the test that asks about the other component.
///
/// The same distance as [`MOVE_BY`] and up instead of along, so the two tests
/// differ in axis and in nothing else. Measured on radv: this puts the projected
/// centre 0.0557 of the frame's height **upward**, which is `v` decreasing,
/// which is a negative motion under the convention.
const MOVE_UP_BY: Vec3 = Vec3::new(0.0, 0.25, 0.0);

/// How far the moving instance's own texels reach from its projected centre,
/// and [`STILL_SILHOUETTE`] the same for its neighbour.
///
/// **A vertical move needs this where a horizontal one did not.**
/// [`nearer_first`] splits the frame on the bisector of the two centres, and the
/// moving instance's left silhouette edge sits almost exactly on it: moving
/// along `+X` carries that edge away and the split comes out clean, while moving
/// along `+Y` leaves it there. Measured on radv over the `+Y` frame, 13 texels
/// in a two-column sliver at `x = 93..=95` were the moved instance's and fell on
/// the neighbour's side, each within 1.5 texels of being equidistant.
///
/// So the `+Y` test classifies by silhouette instead, and the silhouettes have
/// a gap between them to classify in. Measured on radv: the moved instance's
/// texels reach 40.97 texels from its centre and its neighbour's nearest texel
/// to that centre is 44.09; the neighbour's reach 34.68 from its own centre and
/// the moved instance's nearest is 36.60. These two radii sit in those gaps, and
/// the test asserts the partition they produce is exactly the covered set —
/// every covered texel in one disc, none in both — so a camera or a scene that
/// moved leaves this red rather than quietly reclassifying.
const MOVED_SILHOUETTE: f32 = 42.0;

/// The same for the instance that stands still — see [`MOVED_SILHOUETTE`], which
/// is where both numbers are derived.
const STILL_SILHOUETTE: f32 = 35.5;

/// How much motion across the axis of the move a texel may read, as a fraction
/// of the move's own displacement.
///
/// **Not rest**, and it cannot be: a cube has width, so a vertex off the axis of
/// travel changes its projected `u` slightly as it moves through perspective
/// even when its centre does not. Measured on radv over the `+Y` frame, whose
/// projected centre moves by exactly zero in `u`: the widest texel read 0.00278,
/// which is 5.0% of the same frame's `v` displacement. This is that with room,
/// and it is still an order below the `v` this test asserts — so a `v` leaking
/// into `u` could not pass it.
const CROSS_AXIS_FRACTION: f32 = 0.12;

/// How far the camera pans between two frames, in world units along `+X`.
///
/// Eye and target together, so the frame translates rather than rotating: every
/// covered texel then moves by roughly the same amount, which is what lets
/// [`moving_the_camera_moves_every_covered_pixel`] assert on *every* one of them
/// rather than on a population.
const CAMERA_PAN: Vec3 = Vec3::new(0.2, 0.0, 0.0);

/// How many texels a population has to hold before an assertion over it means
/// anything.
///
/// Measured on radv: a frame of this scene covers 5851 texels and the smaller of
/// the two instances covers 2298 of them. An order of magnitude below the
/// smaller is what turns "the geometry drew" into an assertion rather than an
/// assumption — a scene that drew nothing would otherwise pass every loop in
/// this file by iterating over an empty set.
const COVERED_FLOOR: usize = 200;

/// How far a texel's own motion may sit either side of the projected centre's,
/// as a factor of it.
///
/// A cube has depth and a perspective camera moves its near corners further
/// across the screen than its far ones, so the population is a band and not a
/// value. Measured on radv over the moved instance: the widest texel was 1.224
/// times the centre's displacement and the narrowest 0.894 times it. The bounds
/// here are those with room either side, and they are still far tighter than the
/// mistakes this test exists to catch — a dropped factor of one half is 2.0, and
/// a subtraction done before the perspective divide is not near 1 at all.
const MOTION_BAND: (f32, f32) = (0.8, 1.35);

/// The smallest `u` displacement a panning camera may produce before this file
/// stops believing the camera moved.
///
/// Measured on radv: the pan moved every covered texel between 0.0163 and 0.0344
/// of the frame's width, all of them leftward. Half the smallest is a floor no
/// rounding reaches and no unadvanced history clears, which is the only pair of
/// values it has to separate.
const CAMERA_MOTION_FLOOR: f32 = 0.008;

/// The camera every frame in this file is drawn with: [`mesh_camera`] pulled
/// back far enough to hold both instances, on `goldens.rs`'s `two_mesh_camera`
/// terms exactly.
fn motion_camera() -> Camera {
    let mut camera = mesh_camera(Projection::default());
    camera.eye *= 1.7;
    camera
}

/// [`motion_camera`] panned by [`CAMERA_PAN`], eye and target together.
fn panned_camera() -> Camera {
    let mut camera = motion_camera();
    camera.eye += CAMERA_PAN;
    camera.target += CAMERA_PAN;
    camera
}

/// Where a world point lands in this frame's texels, by the same mapping
/// `mesh.slang`'s `motion_vector` uses: `u` from NDC `x`, and `v` the other way
/// up because a framebuffer's rows start at the top.
///
/// The CPU side of the convention, written out here rather than imported,
/// because a test that reused the shader's own arithmetic could not disagree
/// with it. `crates/crcbl-shaders/shaders/ssr.slang`'s `ndc_to_pixel` is the
/// same mapping inside the engine, which makes this the third copy rather than a
/// guess.
fn project(camera: &Camera, point: Vec3) -> [f32; 2] {
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    [
        (clip.x / clip.w * 0.5 + 0.5) * MESH_EXTENT.0 as f32,
        (0.5 - clip.y / clip.w * 0.5) * MESH_EXTENT.1 as f32,
    ]
}

/// The two instances and the fixture they are drawn on, so a test can render
/// several frames through **one** renderer.
///
/// That is the whole point of the type: the previous transform and the previous
/// view-projection are both state carried from one frame to the next, so a
/// helper that opened a device per frame would be measuring a renderer that had
/// never drawn anything before.
struct MotionScene {
    headless: Headless,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// The instance the tests move. Placed **first**, so it takes the pool slot
    /// `place`'s docs say every reference in this suite is written against.
    moving: InstanceHandle,
}

impl MotionScene {
    /// Opens the fixture and puts both instances in the frame, unspun so their
    /// silhouettes are square-on and land either side of the frame's centre.
    fn open() -> Self {
        let headless = Headless::open_for_mesh();
        let mut renderer =
            ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
                .expect("the forward renderer builds");
        // **Every effect that writes into or composites over the scene target,
        // off.** This file reads that target as data rather than as a picture:
        // the reflection composite, the bloom chain, the froxel composite and
        // either antialiasing tier each add a term to the very texels the
        // encoding is read out of, and a blend of two neighbouring motion
        // vectors is a motion no fragment wrote. Shadows and ambient occlusion
        // are left alone, because the motion view's branch returns before
        // anything they feed is touched.
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none().force(
                RenderEffects::REFLECTIONS
                    .union(RenderEffects::BLOOM)
                    .union(RenderEffects::VOLUMETRIC_FOG)
                    .union(RenderEffects::ANTIALIASING)
                    .union(RenderEffects::SMAA),
                Some(false),
            ),
            ..EffectRequest::default()
        });
        renderer.set_motion_view(true);
        let moving = place_cube_at(&mut renderer, MOVING_AT);
        let _still = place_cube_at(&mut renderer, STILL_AT);
        Self {
            headless,
            renderer,
            pool: TransientPool::new(),
            moving,
        }
    }

    /// Draws one frame and answers with the scene target it wrote.
    fn frame(&mut self, camera: &Camera) -> HdrTarget {
        let mut hdr = Vec::new();
        let _ = render_mesh(
            &self.headless,
            &mut self.renderer,
            &mut self.pool,
            camera,
            Some(&mut hdr),
        );
        assert_eq!(
            hdr.len(),
            (MESH_EXTENT.0 * MESH_EXTENT.1 * 8) as usize,
            "the scene target came back the wrong size, so every value read out of it is at \
             the wrong offset"
        );
        HdrTarget(hdr)
    }

    /// Puts the moving instance at `at`, which the next [`frame`](Self::frame)
    /// uploads.
    ///
    /// The instance pool is what fills the previous transform out of what the
    /// record already held — this passes a position and nothing else, which is
    /// the whole of what a caller ever does.
    fn move_to(&mut self, at: Vec3) {
        self.renderer.set_instance(
            self.moving,
            &InstanceDesc {
                mesh: crcbl::render::scene::DEMO_CUBE,
                material: crcbl::render::scene::DEMO_UNTINTED,
                transform: Mat4::from_translation(at),
            },
        );
    }

    /// Releases everything in dependency order and asks the fixture what the
    /// device saw.
    fn finish(self) {
        let device = self.headless.device.as_ref();
        device.wait_idle().expect("the queue drains");
        self.renderer.destroy(device);
        let mut pool = self.pool;
        pool.destroy(device);
        self.headless.finish();
    }
}

/// The demo scene's cube at `at`, unspun.
fn place_cube_at(renderer: &mut ForwardRenderer, at: Vec3) -> InstanceHandle {
    place(
        renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(at),
    )
}

/// The encoded value at every texel a fragment covered, and [`None`] where the
/// frame is still the clear.
///
/// **Blue is the discriminator**: the motion view writes exactly zero there and
/// [`SCENE_CLEAR`] does not, so no second frame and no geometry mask is needed
/// to say which texels a fragment wrote. The assertion below is what keeps that
/// true — a clear colour that gained a zero blue channel would turn this into a
/// mask of the whole frame, and every test here would then be passing on the
/// background.
fn covered(frame: &HdrTarget) -> Vec<Option<[f32; 4]>> {
    const {
        assert!(
            SCENE_CLEAR[2] != 0.0,
            "the clear colour's blue channel is zero, so it is no longer distinguishable from \
             a texel the motion view wrote and every population below is the whole frame"
        );
    }
    let mut out = Vec::with_capacity((MESH_EXTENT.0 * MESH_EXTENT.1) as usize);
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let pixel = frame.pixel(x, y);
            out.push((pixel[2] == 0.0).then_some(pixel));
        }
    }
    out
}

/// The motion the encoding carries, in texture coordinates.
fn decode(encoded: [f32; 4]) -> [f32; 2] {
    [
        (encoded[0] - REST) / MOTION_VIEW_SCALE,
        (encoded[1] - REST) / MOTION_VIEW_SCALE,
    ]
}

/// Whether this texel's motion is rest, within what the target's rounding leaves
/// of it — see [`REST_TOLERANCE`].
fn at_rest(motion: [f32; 2]) -> bool {
    motion[0].abs() <= REST_TOLERANCE && motion[1].abs() <= REST_TOLERANCE
}

/// Where in the frame the texel at `index` sits.
fn texel_at(index: usize) -> (u32, u32) {
    (index as u32 % MESH_EXTENT.0, index as u32 / MESH_EXTENT.0)
}

/// Which of two projected centres the texel at `index` is nearer, `true` for the
/// first.
///
/// The two instances are told apart by where they are rather than by what they
/// are, because under this view they have no colour of their own — the picture
/// *is* the motion field. Projecting both centres on the CPU is what makes the
/// classification a statement about this camera rather than a hard-coded
/// half-frame.
fn nearer_first(index: usize, first: [f32; 2], second: [f32; 2]) -> bool {
    let (x, y) = texel_at(index);
    let to = |centre: [f32; 2]| (x as f32 - centre[0]).powi(2) + (y as f32 - centre[1]).powi(2);
    to(first) < to(second)
}

/// How far the texel at `index` is from a projected centre, in texels.
fn distance_to(index: usize, centre: [f32; 2]) -> f32 {
    let (x, y) = texel_at(index);
    ((x as f32 - centre[0]).powi(2) + (y as f32 - centre[1]).powi(2)).sqrt()
}

/// **A still scene under a still camera reads rest**, on the second frame as
/// much as the first.
///
/// The first frame is the one with no history at all: the renderer carries no
/// previous view-projection and hands the block its own, which is the branch
/// that would otherwise reproject every pixel of a fresh frame through a zero or
/// an identity matrix. The second is the one where both halves of the history
/// are real — the pool has settled the instances the first frame wrote, and the
/// camera's previous matrix is the first frame's — so a subtraction that was
/// merely *absent* would pass on frame one, and this asks for both.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_still_scene_under_a_still_camera_reads_rest() {
    let mut scene = MotionScene::open();
    let camera = motion_camera();
    let first = scene.frame(&camera);
    let second = scene.frame(&camera);
    scene.finish();

    for (name, frame) in [("the first frame", &first), ("the second frame", &second)] {
        let field = covered(frame);
        let mut drawn = 0usize;
        for (index, texel) in field.iter().enumerate() {
            let Some(encoded) = texel else {
                continue;
            };
            drawn += 1;
            let motion = decode(*encoded);
            assert!(
                at_rest(motion),
                "{name}: texel {:?} reads {motion:?}, and nothing in this scene moved",
                texel_at(index)
            );
        }
        assert!(
            drawn > COVERED_FLOOR,
            "{name} covered only {drawn} texel(s), so the loop above checked almost nothing"
        );
    }
}

/// **The instance that moved reads motion, and the one beside it reads rest.**
///
/// Both halves are the claim. A pass that wrote *something* everywhere would
/// satisfy the first; a pass that wrote the camera's motion into every pixel —
/// which is what a previous transform nothing read would produce — would satisfy
/// it too, and fail the second. Two instances in one frame, one moved and one
/// not, is what separates "this fragment moved" from "this frame moved".
///
/// # The sign
///
/// `+X` in world is to the right of this camera, and the moving instance sits at
/// the point the camera looks at, so a translation there is a translation across
/// the screen with no magnification to fight and the screen position increases
/// in `u`. The convention is current minus previous, so the motion is positive —
/// and [`project`] is asked to confirm that before the band below is applied, so
/// a camera someone later moves cannot leave this test asserting a sign the
/// arithmetic no longer produces.
///
/// Measured on radv: every one of the moved instance's 3737 texels read between
/// 0.0333 and 0.0455 in `u` against a projected centre displacement of 0.0372,
/// and all 2298 of its neighbour's read rest.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_instance_that_moved_reads_motion_and_its_neighbour_reads_rest() {
    let camera = motion_camera();
    let mut scene = MotionScene::open();
    // Two frames of nothing, so the history the third subtracts against is a
    // real one rather than the first frame's own matrices.
    let _ = scene.frame(&camera);
    let _ = scene.frame(&camera);
    scene.move_to(MOVING_AT + MOVE_BY);
    let moved = scene.frame(&camera);
    scene.finish();

    let before = project(&camera, MOVING_AT);
    let after = project(&camera, MOVING_AT + MOVE_BY);
    let still = project(&camera, STILL_AT);
    let predicted = (after[0] - before[0]) / MESH_EXTENT.0 as f32;
    assert!(
        predicted > CAMERA_MOTION_FLOOR,
        "this camera moves the instance's centre by {predicted} in u, so the positive sign and \
         the band below are not what its motion works out to"
    );

    let field = covered(&moved);
    let (mut moving_texels, mut still_texels) = (0usize, 0usize);
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        let motion = decode(*encoded);
        if nearer_first(index, after, still) {
            moving_texels += 1;
            assert!(
                motion[0] > predicted * MOTION_BAND.0 && motion[0] < predicted * MOTION_BAND.1,
                "texel {:?} belongs to the instance that moved and reads {motion:?} in u, \
                 against a projected centre displacement of {predicted}",
                texel_at(index)
            );
        } else {
            still_texels += 1;
            assert!(
                at_rest(motion),
                "texel {:?} belongs to the instance that did not move and reads {motion:?}, so \
                 the motion is the frame's rather than the object's",
                texel_at(index)
            );
        }
    }
    assert!(
        moving_texels > COVERED_FLOOR && still_texels > COVERED_FLOOR,
        "the two instances covered {moving_texels} and {still_texels} texel(s), so one of the \
         two assertions above ran over almost nothing"
    );
}

/// **The other component, and its sign.** An instance moved along `+Y` reads a
/// negative `v`, and almost nothing in `u`.
///
/// The move test above holds only `u`, so the `-0.5` in `mesh.slang`'s
/// `motion_vector` — the half of the NDC-to-texture mapping that flips the
/// vertical axis — is unconstrained by it: turn it into `+0.5` and every
/// assertion in this file that predates this one still passes. **The convention
/// has a `+y`-down half and this is what holds it**, because a consumer reading
/// history at `uv - motion` gets a vertical smear rather than a wrong picture if
/// the sign is inverted, which is the kind of wrong that survives review.
///
/// # Deriving the sign
///
/// `+Y` in world is up for this camera, a texture's `v` grows downward, so a
/// screen position that rises is a `v` that falls. [`project`] is asked for the
/// two centres and their difference is the prediction, and the assertion below
/// is a *ratio* against it — so the sign is asserted by the arithmetic rather
/// than written down here, and a camera someone later re-aims cannot leave this
/// test demanding a sign the projection no longer produces.
///
/// Measured on radv: the centre rises 0.0557 of the frame's height, all 3663 of
/// the moved instance's texels read `v` between −0.0682 and −0.0510 — 0.915 to
/// 1.223 times the centre's — with `u` no wider than 0.00278, and all 2298 of
/// its neighbour's read rest.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_instance_that_moved_up_reads_a_negative_v_and_no_u() {
    let camera = motion_camera();
    let mut scene = MotionScene::open();
    let _ = scene.frame(&camera);
    let _ = scene.frame(&camera);
    scene.move_to(MOVING_AT + MOVE_UP_BY);
    let moved = scene.frame(&camera);
    scene.finish();

    let before = project(&camera, MOVING_AT);
    let after = project(&camera, MOVING_AT + MOVE_UP_BY);
    let still = project(&camera, STILL_AT);
    let predicted = (after[1] - before[1]) / MESH_EXTENT.1 as f32;
    assert!(
        predicted < -CAMERA_MOTION_FLOOR,
        "this camera moves the instance's centre by {predicted} in v, so it is not rising far \
         enough for the ratio below to say anything about the sign"
    );

    let field = covered(&moved);
    let (mut moving_texels, mut still_texels) = (0usize, 0usize);
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        let motion = decode(*encoded);
        // Silhouettes rather than the bisector — see `MOVED_SILHOUETTE`, which
        // is also where these two radii were measured. The partition is asserted
        // rather than assumed: a texel in both discs or in neither is a scene
        // this classification no longer describes.
        let in_moved = distance_to(index, after) <= MOVED_SILHOUETTE;
        let in_still = distance_to(index, still) <= STILL_SILHOUETTE;
        assert!(
            in_moved != in_still,
            "texel {:?} is {} of the two silhouettes, so the radii no longer separate the \
             instances and every assertion below is about the wrong one",
            texel_at(index),
            if in_moved {
                "inside both"
            } else {
                "outside both"
            }
        );
        if in_moved {
            moving_texels += 1;
            let ratio = motion[1] / predicted;
            assert!(
                ratio > MOTION_BAND.0 && ratio < MOTION_BAND.1,
                "texel {:?} belongs to the instance that rose and reads {motion:?}, which is \
                 {ratio} times the projected centre's {predicted} — a ratio at or below zero \
                 is the vertical axis flipped",
                texel_at(index)
            );
            assert!(
                motion[0].abs() < predicted.abs() * CROSS_AXIS_FRACTION,
                "texel {:?} rose straight up and reads {motion:?}, whose u is more than \
                 perspective across the silhouette accounts for",
                texel_at(index)
            );
        } else {
            still_texels += 1;
            assert!(
                at_rest(motion),
                "texel {:?} belongs to the instance that did not move and reads {motion:?}, so \
                 the motion is the frame's rather than the object's",
                texel_at(index)
            );
        }
    }
    assert!(
        moving_texels > COVERED_FLOOR && still_texels > COVERED_FLOOR,
        "the two instances covered {moving_texels} and {still_texels} texel(s), so one of the \
         three assertions above ran over almost nothing"
    );
}

/// **Moving the camera moves every covered pixel**, with nothing in the scene
/// moving at all.
///
/// This is the half of the subtraction the previous view-projection owns, and it
/// is the half a renderer can get wrong silently: feed the block this frame's
/// own matrix instead of last frame's and every object test above still passes,
/// because an object's previous transform is unaffected. What fails is this — a
/// panning camera over a still scene then reads rest everywhere.
///
/// Eye and target pan together, so there is no centre of expansion for a texel
/// to sit at and read zero legitimately; every covered texel moves, and the
/// camera moving right means the scene moving left, so the sign is negative.
///
/// Measured on radv: the pan moved all 5637 covered texels between −0.0344 and
/// −0.0163 in `u`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn moving_the_camera_moves_every_covered_pixel() {
    let mut scene = MotionScene::open();
    let _ = scene.frame(&motion_camera());
    let panned = scene.frame(&panned_camera());
    scene.finish();

    let field = covered(&panned);
    let mut drawn = 0usize;
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        drawn += 1;
        let motion = decode(*encoded);
        assert!(
            motion[0] < -CAMERA_MOTION_FLOOR,
            "texel {:?} reads {motion:?} under a camera panning to the right, which is rest or \
             the wrong way — the frame block is carrying this frame's own view-projection \
             rather than the previous frame's",
            texel_at(index)
        );
    }
    assert!(
        drawn > COVERED_FLOOR,
        "the pan covered only {drawn} texel(s), so the loop above checked almost nothing"
    );
}

/// **A frame in which nothing moved is back at rest**, after one in which
/// something did.
///
/// This is the test that catches the previous view-projection being advanced in
/// the wrong place. Advance it somewhere a frame can reach twice, or not at all,
/// and the camera half of the subtraction never returns to zero; leave the
/// instance pool's carry-forward out and the object half does not either. Both
/// failures look exactly like a working pass on the frame the move happens and
/// show up only on the frame after it, which is why the move above and this are
/// separate claims rather than one.
///
/// Measured on radv: 3737 texels moved in the third frame and every one of the
/// fourth frame's 6035 covered texels was back at rest.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_frame_after_a_move_is_back_at_rest() {
    let camera = motion_camera();
    let mut scene = MotionScene::open();
    let _ = scene.frame(&camera);
    let _ = scene.frame(&camera);
    scene.move_to(MOVING_AT + MOVE_BY);
    let moved = scene.frame(&camera);
    // Nothing written and nothing panned: the pool settles the instance the
    // frame before wrote, and the renderer advances the camera onto the matrix
    // it just drew with.
    let settled = scene.frame(&camera);
    scene.finish();

    // The move happened, or a fourth frame at rest says nothing at all. Read off
    // the same frame rather than asserted a second time — the move's own test is
    // where the magnitude and the sign are checked.
    let moving = covered(&moved)
        .into_iter()
        .flatten()
        .filter(|encoded| !at_rest(decode(*encoded)))
        .count();
    assert!(
        moving > COVERED_FLOOR,
        "only {moving} texel(s) moved in the frame that moved, so a fourth frame at rest \
         proves nothing"
    );

    let field = covered(&settled);
    let mut drawn = 0usize;
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        drawn += 1;
        let motion = decode(*encoded);
        assert!(
            at_rest(motion),
            "texel {:?} reads {motion:?} a frame after the move finished",
            texel_at(index)
        );
    }
    assert!(
        drawn > COVERED_FLOOR,
        "the settled frame covered only {drawn} texel(s), so the loop above checked almost \
         nothing"
    );
}
