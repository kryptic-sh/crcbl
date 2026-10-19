//! Shard's zone off a real device, from the fixed camera set, on every
//! [`GeometryPath`] this adapter can be held down to — against the checked-in
//! goldens.
//!
//! # What this is the exit criterion for
//!
//! `docs/plan/sample/15-shard.md`'s milestone 1 asks for "golden frames per
//! `GeometryPath` from a fixed camera set". This sample's whole subject is the
//! *fallback* paths carrying real content — a browser's frame goes through
//! `IndirectPerBatch`, `ArrayPages` and `LightingPath::Rasterised` by
//! construction — so a frame that is only ever checked on the tail a desktop
//! adapter selects for itself is a frame checked on the path nobody ships to.
//!
//! # The camera set is derived, not scripted
//!
//! [`crcbl_shard::Iso`] is a bearing and the bearing it is swinging toward, and
//! `Q`/`E` ask it for a quarter turn. A headless run receives no key, so the
//! binary can only ever present the spawn bearing — but the rig is a pure
//! function of that bearing and of where the character is standing, so the
//! other three bearings are *computed* here rather than pressed for:
//! [`station`] asks the sample's own [`Iso`] for `n` quarter turns and lets it
//! settle. The set is therefore the four bearings the sample can be in — named
//! below for the turn rather than for a compass direction, because the zone has
//! no north — drawn from the state [`Game::render_state`] reports before the
//! first tick has run.
//!
//! # A path is reached by subtracting features from one adapter
//!
//! `apps/quarry/tests/device/harness.rs`'s mechanism, and the reason it works:
//! [`GeometryPath::from_features`] reads `MESH_SHADER`, then
//! `DRAW_INDIRECT_COUNT`, and falls through to `IndirectPerBatch`, so
//! withholding one feature at a time from the device request walks down the
//! three. A device that never had the feature lands on the same path by the
//! same rule, which is what makes a forced path evidence about the path rather
//! than about this adapter.
//!
//! The subtraction starts from [`OffscreenSetup::OPTIONAL_FEATURES`] rather
//! than from a hand-written list, for `apps/lantern/tests/golden.rs`'s reason:
//! two arms opened from different bases are not a comparison, and a list
//! written out here would keep naming the old flags after the request grew one.
//! [`BASE`] is that request with one further flag off, for a reason that is
//! about the two rasterisers rather than about this sample — see it.
//!
//! **An adapter that cannot reach a path says so and does not compare.** No
//! frame is skipped quietly: the test prints which path it asked for, which one
//! the device opened on and what the adapter offers, and
//! `tests/run-shard-golden.sh` refuses a run in which *nothing* matched.
//!
//! # One reference per bearing, not one per path
//!
//! Three paths share each bearing's golden, which is
//! `apps/lantern/tests/golden.rs`'s argument rather than
//! `apps/quarry/tests/device/goldens.rs`': a lesser geometry path is a
//! constraint on how the submission tail is built and not a separate renderer,
//! so a difference between two paths is a **bug**, and a second reference would
//! bless it. Quarry commits one image per path because its paths genuinely
//! disagree — at its mixing budget the mesh path draws two levels per cluster
//! where the others select one per instance — and this zone has no such
//! hierarchy: it is one authored table of greybox pieces, drawn identically by
//! all three tails. Measured, not assumed: every path matches the same
//! reference inside [`Tolerance::RASTERISER`] on radv and on lavapipe.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two black frames compare perfectly. So the golden is the *last* assertion
//! and the ones in front of it are about **where** the frame is bright — the
//! figure standing in the middle of it, the floor the torches light beside it,
//! and how many colours a torch-lit interior has at all.
//!
//! [`the_torches_are_what_lights_the_zone`] carries the claim no single frame
//! can: the same bearing drawn with the torches doused, which is the one switch
//! `L` gives a player and the one the browser gate reads.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e`, lantern's and quarry's suites use. A plain
//! `cargo test --workspace --all-features` on a machine with no GPU must stay
//! green, and `tests/run-shard-golden.sh` is the only thing that turns both off
//! — and it fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use std::path::PathBuf;

use crcbl::hal::{AdapterInfo, DeviceCaps, Features, Format, GeometryPath};
use crcbl::math::Vec3;
use crcbl::render::{Camera, EffectRequest, ForwardRenderer, OrbitCamera, RenderEffects};
use crcbl::screenshot::{ForwardScene, OffscreenError, OffscreenSetup};
use crcbl_golden::{ChannelOrder, Golden, Image, Tolerance};
use crcbl_sample_test::Block;
use crcbl_shard::{
    DEFAULT_SEED, DEFAULT_TICK_HZ, EXPOSURE, Game, Iso, RenderState, camera, light, zone,
};

/// The extent the checked-in goldens are blessed at.
///
/// The same 4:3 the sample's own default window is and the same size every
/// other golden in the tree is: small enough to read in a diff, large enough
/// that the blocks below are tens of pixels rather than a handful.
const EXTENT: (u32, u32) = (256, 192);

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, for the reason [`Block`] gives.
const BLOCK: (u32, u32) = (4, 4);

/// Whether the torches are alight in every frame but one.
///
/// The state a run starts in — `crcbl_shard::app` lights them and `L` is what
/// puts them out — so this is the zone as a visitor first sees it.
const LIT: bool = true;

/// How far to the camera's right the floor claim reads, in metres.
///
/// **To the camera's right rather than along a world axis**, so it is the same
/// patch of floor relative to the frame at all four bearings and is never the
/// square the figure is standing on top of. Short enough that it stays inside
/// the spawn tile — [`zone::TILE_M`] is three metres, so its own floor reaches
/// 1.5 m in every direction — which is what makes it floor at every bearing
/// rather than whatever the next tile happens to hold.
const FLOOR_OFFSET_M: f32 = 1.2;

/// How high up the character's capsule the figure claim reads, in metres above
/// their feet.
///
/// Mid-body: above the floor the capsule stands on and below its cap, so the
/// block is on the figure at every bearing rather than straddling its
/// silhouette.
const FIGURE_HEIGHT_M: f32 = 0.9;

// ---------------------------------------------------------------------------
// The claims' thresholds
// ---------------------------------------------------------------------------

/// How many distinct colours a frame of this zone has to have.
///
/// A torch-lit interior of stone under a flickering point light per brazier, a
/// spot over the shrine and an irradiance volume: a frame with fewer than this
/// drew the clear colour and very little else. Counted rather than guessed at —
/// `Image::distinct_colors` stops counting at the bound it is given, and the
/// four bearings draw 577, 847, 3084 and 3150 on radv against 567, 857, 3119
/// and 3125 on llvmpipe. Under half of the smallest of those, and the state it
/// exists to catch is a single colour.
const MIN_COLORS: usize = 256;

/// The level a block has to clear to have drawn anything at all, out of 255.
///
/// Not "is this the right shade" — the golden answers that — but "did a pass
/// put anything here". Low because this zone is deliberately dark:
/// [`EXPOSURE`]'s own docs record the doused zone reading a mean of 3.6 over
/// the canvas, and the darkest block either claim reads is the spawn bearing's
/// floor at 6.1 on radv and 6.0 on llvmpipe. Half of that, and the state it
/// exists to catch is the clear colour.
const DREW_AT_ALL: f32 = 3.0;

/// How much brighter the floor beside the character must be with the torches
/// alight than with them out.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the exposure does. **Two and a half against a
/// measured 4.6**: the block reads 6.1/255 lit against 1.3/255 doused on radv
/// and 6.0 against 1.3 on llvmpipe, and what is left with them out is
/// [`zone::house_light`]'s ambient and the shrine's spot reaching nothing this
/// far away. The state this exists to catch — a zone lit by that ambient alone,
/// which is a perfectly plausible dark interior and would match a golden
/// blessed from the same mistake — reads exactly 1.0, which is the whole way
/// below this rather than beside it.
const TORCH_LIFT: f32 = 2.5;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// What one frame of this suite is: the picture, the camera that drew it, and
/// which tail the device actually opened on.
struct Frame {
    image: Image,
    camera: Camera,
    /// The path the device opened on, which is **not** always the one asked
    /// for — see the module docs.
    drew: GeometryPath,
    adapter: AdapterInfo,
    /// What `adapter` offers, which the device may have been opened below.
    offered: DeviceCaps,
    /// What the sample's own simulation said the frame should draw — carried
    /// so a claim reads the state the picture was made from rather than
    /// building a second one and hoping the two agree.
    state: RenderState,
}

/// What every arm of this suite opens its device with, before the geometry
/// selector's own flags are taken off it.
///
/// [`OffscreenSetup::OPTIONAL_FEATURES`] **minus
/// [`Features::SAMPLER_ANISOTROPY`]**, and that subtraction is the one thing
/// here that is about the comparison rather than about the sample.
///
/// # Anisotropic filtering is implementation-defined, and this frame is the
/// worst case for it
///
/// Vulkan leaves the sample pattern and the sample count of an anisotropic
/// fetch to the implementation, so two rasterisers filtering the same texture
/// at the same footprint are not required to return the same texel — and
/// almost all of this frame is a tiled floor seen at a grazing angle, which is
/// the footprint where the latitude is widest. Both drivers do implement it:
/// withholding the flag moves radv's own frame by 19.8% of its pixels and
/// llvmpipe's by 21.8%, so this is two implementations disagreeing rather than
/// one of them ignoring the request.
///
/// **Measured 2026-09-07, at a quarter turn, radv (Mesa 26.2.2) against
/// llvmpipe (LLVM 22.1.8):** with the flag asked for, 5.77% of the frame
/// exceeds [`Tolerance::RASTERISER`]'s per-channel delta, against a budget of
/// 1%; without it, 0.25%. Nothing else in the frame accounts for any of it —
/// the same comparison with the antialiasing pass off reads 5.7739% (the same
/// figure to four decimals), with occlusion and reflections off 5.77%, and with
/// every effect but the shadows off 5.79%.
///
/// So the alternative to this line is a suite-local tolerance an order of
/// magnitude looser than the one the rest of the tree is held to, which would
/// widen the gate against every regression to absorb one. `docs/backlog.md`
/// carries what a golden that keeps the anisotropy would need.
const BASE: Features = OffscreenSetup::OPTIONAL_FEATURES.difference(Features::SAMPLER_ANISOTROPY);

/// What to ask the device for so that [`GeometryPath::from_features`] resolves
/// to `path`.
///
/// [`BASE`] minus the flags whose presence would select something above `path`.
/// `TASK_SHADER` goes with `MESH_SHADER`: it is the amplification stage in
/// front of one and has nothing to drive without it.
fn wanted(path: GeometryPath) -> Features {
    let mesh = Features::MESH_SHADER.union(Features::TASK_SHADER);
    match path {
        GeometryPath::MeshShader => BASE,
        GeometryPath::IndirectCount => BASE.difference(mesh),
        GeometryPath::IndirectPerBatch => {
            BASE.difference(mesh.union(Features::DRAW_INDIRECT_COUNT))
        }
    }
}

/// The camera `turns` quarter turns from the spawn bearing, looking at `feet`.
///
/// Through the sample's own [`Iso`], settled: `rotate` records a bearing and
/// `advance` is what closes the gap to it, so a camera read before the swing
/// finished would be a picture of a turn in progress.
///
/// The step handed to `advance` is **twice** the time the rig would take to
/// swing this many quarter turns at [`camera::SWING_RATE`]: it snaps to the
/// target only when the step covers the whole remaining gap, and a step sized
/// to exactly that gap is a float comparison at its own precision.
/// [`Iso::settled`] is what says the swing finished rather than this sentence.
fn station(turns: i32, feet: Vec3) -> Camera {
    let mut iso = Iso::default();
    iso.rotate(turns);
    #[allow(clippy::cast_precision_loss)]
    let settle = 2.0 * turns.unsigned_abs() as f32 * camera::YAW_STEP / camera::SWING_RATE;
    iso.advance(settle);
    assert!(
        iso.settled(),
        "the rig is still swinging at {} of a target {}, so this camera is a turn in progress",
        iso.yaw(),
        iso.target(),
    );
    iso.camera(feet)
}

/// The state the sample's own simulation reports before the first tick.
///
/// From [`Game`] rather than rebuilt out of [`zone::spawn`] and
/// [`crcbl_shard::foe`]: where the character stands, where the foes stand and
/// how much simulated time has passed are the stage's answers, and a test that
/// wrote its own would be blessing a picture of a zone the sample never draws.
fn spawn_state() -> RenderState {
    Game::new(DEFAULT_TICK_HZ, DEFAULT_SEED, None)
        .expect("the loopback session comes up")
        .render_state()
}

/// The zone, made resident and placed, on a device the caller opened.
///
/// Every line here is `crcbl_shard::gpu`'s: the same scene description, the
/// same camera-layer effect stack, the same exposure — [`EXPOSURE`] is read
/// from the sample rather than copied, so a stop that moves moves these goldens
/// — the same placement, and the same light list. What is the test's is `lit`.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    state: &RenderState,
    lit: bool,
) -> Result<ForwardRenderer, OffscreenError> {
    let scene = zone::scene();
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // The **camera** layer is the engine's default stack, as `crcbl_shard::gpu`
    // sets it. The player's `[engine.video]` clamp is deliberately absent:
    // `OffscreenSetup` reads no settings file, and a golden that took its
    // effects from whichever home directory ran it would compare two different
    // renderers.
    renderer.set_effect_request(EffectRequest {
        camera: RenderEffects::DEFAULT_STACK,
        ..EffectRequest::default()
    });
    renderer.set_exposure(EXPOSURE);
    let placed = match zone::place(&mut renderer) {
        Ok(placed) => placed,
        Err(error) => {
            renderer.destroy(device);
            return Err(OffscreenError::Hal(
                crcbl::hal::HalError::InvalidDescriptor(format!(
                    "shard's zone does not fit its own pools: {error}"
                )),
            ));
        }
    };
    placed.figure.set_feet(&mut renderer, state.feet);
    for (index, view) in state.foes.iter().enumerate() {
        placed.foes.set(&mut renderer, index, view);
    }
    renderer.set_lights(&light::torches(state.elapsed, lit));
    Ok(renderer)
}

/// Opens a device held to `path`, draws the zone from `turns` quarter turns,
/// and reads the frame back.
fn draw(turns: i32, path: GeometryPath, lit: bool) -> Frame {
    // A logger before anything opens: without one, every line a backend emits
    // on the way to a device goes nowhere, and a failure inside `open` names
    // the call that noticed rather than the one that caused it.
    crcbl::core::log::init_logging();

    let state = spawn_state();
    #[allow(clippy::cast_possible_truncation)]
    let feet = Vec3::new(
        state.feet.x as f32,
        state.feet.y as f32,
        state.feet.z as f32,
    );
    let camera = station(turns, feet);
    let asked = wanted(path);

    let mut setup =
        OffscreenSetup::open_forward_with(EXTENT.0, EXTENT.1, asked, |device, queue, format| {
            Ok(ForwardScene {
                camera,
                sun: zone::house_light(),
                renderer: Box::new(build(device, queue, format, &state, lit)?),
            })
        })
        .unwrap_or_else(|why| panic!("a GPU backend opens for shard's zone: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    let offered = setup
        .adapter_caps()
        .expect("the adapter the device opened on describes itself");
    // Printed unconditionally and read with `--success-output immediate`: on a
    // green run — the run where the path that drew is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "shard golden: device on adapter {id} {name:?} type={kind:?}",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
    );
    let drew = caps.geometry_path();
    eprintln!(
        "shard golden: {backend} {drew:?} / {:?} / {:?} at {}x{}, asked for {path:?}",
        caps.binding_model(),
        caps.lighting_path(),
        EXTENT.0,
        EXTENT.1,
    );
    // **The device landed on exactly the path its request names**, which the
    // frame alone cannot say. Met against the adapter rather than against
    // `path`: an adapter that never had the feature lands lower, and that is
    // the case the caller reports rather than the one that fails here.
    let granted = asked.intersection(offered.features);
    assert_eq!(
        drew,
        GeometryPath::from_features(granted),
        "adapter {} offers {:?}, this run asked for {asked:?}, and the device opened on {drew:?}",
        adapter.name,
        offered.features,
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else.
    setup.finish().expect("the device reaches idle");

    assert_eq!(
        (width, height),
        EXTENT,
        "the swapchain handed back an extent nothing was measured at"
    );
    let order = if format == Format::Bgra8UnormSrgb || format == Format::Bgra8Unorm {
        ChannelOrder::Bgra
    } else {
        ChannelOrder::Rgba
    };
    let image = Image::from_readback(width, height, &pixels, order)
        .expect("the readback is exactly one image");
    Frame {
        image,
        camera,
        drew,
        adapter,
        offered,
        state,
    }
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in the frame, in pixels.
///
/// Through the very same [`Camera::view_projection`] the frame was drawn with,
/// so a claim about a surface is a claim about the pixels that surface actually
/// covers rather than about a hand-derived mapping a change of camera would
/// silently invalidate.
///
/// The projection produces **Y-up** normalised device coordinates; the
/// framebuffer's rows run the other way, which is the flip below.
fn project(camera: &Camera, point: Vec3) -> (u32, u32) {
    #[allow(clippy::cast_precision_loss)]
    let aspect = EXTENT.0 as f32 / EXTENT.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    assert!(
        clip.w > 0.0,
        "{point:?} is behind the camera, so nothing in the frame is about it"
    );
    let ndc = clip.truncate() / clip.w;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (EXTENT.0 as f32, EXTENT.1 as f32);
    let x = (ndc.x + 1.0) * 0.5 * width;
    let y = (1.0 - ndc.y) * 0.5 * height;
    assert!(
        x >= 0.0 && x < width && y >= 0.0 && y < height,
        "{point:?} projects to ({x:.1}, {y:.1}), outside a {width}x{height} frame — the claim \
         about it would be about a pixel that is not there"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x as u32, y as u32)
}

/// Where the two blocks every claim reads sit, in the world.
///
/// The figure is the capsule the physics moves, so it is read at the feet the
/// stage reported; the floor is [`FLOOR_OFFSET_M`] to the **camera's** right,
/// which is [`OrbitCamera::walk_direction`]'s "strafe" axis for this bearing —
/// the sample's own conversion rather than a second one written here.
fn read_points(state: &RenderState, turns: i32) -> (Vec3, Vec3) {
    #[allow(clippy::cast_possible_truncation)]
    let feet = Vec3::new(
        state.feet.x as f32,
        state.feet.y as f32,
        state.feet.z as f32,
    );
    let yaw = f64::from(turns) * f64::from(camera::YAW_STEP);
    let right = OrbitCamera::walk_direction(yaw, 0.0, 1.0);
    #[allow(clippy::cast_possible_truncation)]
    let right = Vec3::new(right.x as f32, right.y as f32, right.z as f32);
    (
        feet + Vec3::Y * FIGURE_HEIGHT_M,
        feet + right * FLOOR_OFFSET_M,
    )
}

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(frame: &Frame, turns: i32, name: &str) {
    let block = Block::new(&frame.image, BLOCK, "shard");
    let colors = frame.image.distinct_colors(MIN_COLORS);
    let (figure_at, floor_at) = read_points(&frame.state, turns);
    let figure = block.brightness(project(&frame.camera, figure_at));
    let floor = block.brightness(project(&frame.camera, floor_at));
    // Every claim prints what it actually got before any of them is made, so
    // the next person sizing one of these thresholds does not have to re-derive
    // it from a run that stopped at the first failure.
    eprintln!(
        "shard golden: {name} — {colors} colours, figure {figure:.1}/255, floor {floor:.1}/255"
    );
    assert!(
        colors >= MIN_COLORS,
        "a zone with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not evidence — \
         nothing drew, or only the clear did"
    );
    assert!(
        figure > DREW_AT_ALL,
        "the block on the capsule reads {figure:.1}/255, so no pass drew into the middle of \
         this frame — whether what stands there is the capsule is the golden's claim, not this \
         one"
    );
    assert!(
        floor > DREW_AT_ALL,
        "the floor beside the character reads {floor:.1}/255, so no light reached it"
    );
}

/// The reference this bearing is held to, whichever path drew it.
fn reference(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"))
}

/// Renders one bearing on one path and compares it against that bearing's
/// committed reference.
fn check(path: GeometryPath, turns: i32, name: &str) {
    let frame = draw(turns, path, LIT);
    if frame.drew != path {
        eprintln!(
            "shard golden: {name} cannot be drawn on {path:?} — adapter {:?} offers {:?}, so the \
             device opened on {:?} instead and this bearing is compared by the test that asked \
             for that path",
            frame.adapter.name, frame.offered.features, frame.drew,
        );
        return;
    }
    inspect(&frame, turns, name);

    let comparison = Golden::new(reference(name))
        .with_tolerance(Tolerance::RASTERISER)
        .check(&frame.image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{name} on {path:?}: {message}"));
    // Printed on success too: the numbers are how the tolerance stays honest
    // across two drivers, and a run that quietly passes teaches nothing.
    eprintln!(
        "shard golden: {name} drew on {path:?} and matched — {}",
        comparison.summary()
    );
}

/// One test per image rather than one over twelve.
///
/// `crcbl-golden` **fails a blessing run on purpose**, so that re-blessing can
/// never be mistaken for comparing. A single test would therefore write one
/// image and stop, and blessing the set would take as many runs as there are
/// images. One test per frame blesses them in one pass with `--no-fail-fast`,
/// and names in the failure which frame moved and which path drew it.
macro_rules! golden_test {
    ($fn_name:ident, $path:expr, $turns:expr, $name:literal) => {
        #[test]
        #[ignore = "needs a real GPU and a backend pin; run tests/run-shard-golden.sh"]
        fn $fn_name() {
            check($path, $turns, $name);
        }
    };
}

golden_test!(
    the_mesh_shader_path_draws_the_spawn_bearing,
    GeometryPath::MeshShader,
    0,
    "spawn"
);
golden_test!(
    the_mesh_shader_path_draws_a_quarter_turn,
    GeometryPath::MeshShader,
    1,
    "quarter"
);
golden_test!(
    the_mesh_shader_path_draws_a_half_turn,
    GeometryPath::MeshShader,
    2,
    "half"
);
golden_test!(
    the_mesh_shader_path_draws_three_quarters_of_a_turn,
    GeometryPath::MeshShader,
    3,
    "three-quarters"
);
golden_test!(
    the_indirect_count_path_draws_the_spawn_bearing,
    GeometryPath::IndirectCount,
    0,
    "spawn"
);
golden_test!(
    the_indirect_count_path_draws_a_quarter_turn,
    GeometryPath::IndirectCount,
    1,
    "quarter"
);
golden_test!(
    the_indirect_count_path_draws_a_half_turn,
    GeometryPath::IndirectCount,
    2,
    "half"
);
golden_test!(
    the_indirect_count_path_draws_three_quarters_of_a_turn,
    GeometryPath::IndirectCount,
    3,
    "three-quarters"
);
golden_test!(
    the_indirect_per_batch_path_draws_the_spawn_bearing,
    GeometryPath::IndirectPerBatch,
    0,
    "spawn"
);
golden_test!(
    the_indirect_per_batch_path_draws_a_quarter_turn,
    GeometryPath::IndirectPerBatch,
    1,
    "quarter"
);
golden_test!(
    the_indirect_per_batch_path_draws_a_half_turn,
    GeometryPath::IndirectPerBatch,
    2,
    "half"
);
golden_test!(
    the_indirect_per_batch_path_draws_three_quarters_of_a_turn,
    GeometryPath::IndirectPerBatch,
    3,
    "three-quarters"
);

/// **The torches are what lights this zone**, which no single frame can say.
///
/// Every golden above is drawn with them alight, and a frame lit by nothing but
/// the house ambient is a perfectly plausible picture of a dark interior — it
/// would pass every claim in [`inspect`] and match a golden blessed from the
/// same mistake. So this draws the spawn bearing twice, differing in the one
/// switch `L` gives a player, and reads the pair.
///
/// On whichever path the adapter selects for itself: the switch is
/// `crcbl_shard::light`'s and has nothing to do with the submission tail.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-shard-golden.sh"]
fn the_torches_are_what_lights_the_zone() {
    let lit = draw(0, GeometryPath::MeshShader, true);
    let doused = draw(0, GeometryPath::MeshShader, false);
    let (_, floor_at) = read_points(&lit.state, 0);

    let at = project(&lit.camera, floor_at);
    let lit_floor = Block::new(&lit.image, BLOCK, "shard").brightness(at);
    let doused_floor = Block::new(&doused.image, BLOCK, "shard").brightness(at);
    eprintln!(
        "shard golden: the floor reads {lit_floor:.1}/255 lit and {doused_floor:.1}/255 doused"
    );
    assert!(
        lit_floor > doused_floor * TORCH_LIFT,
        "the floor reads {lit_floor:.1} with the torches lit and {doused_floor:.1} with them out \
         — the braziers are not what is lighting this zone"
    );
}
