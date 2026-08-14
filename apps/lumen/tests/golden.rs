//! The room off a real device, from the fixed camera, against a checked-in
//! golden — and six claims about the lighting in front of it.
//!
//! # A golden alone cannot make a claim about lighting
//!
//! A wrong picture is a plausible picture. A shadow map that is never written
//! leaves every surface lit; a window wall built as one quad leaves the floor
//! evenly bright; a material factor that never reached the device leaves a wall
//! grey instead of red; and every one of those produces a frame somebody would
//! bless. So the golden is the *last* of the assertions here, and the ones
//! before it are about **where** the frame is bright and dark, in the shape
//! `crates/crcbl/tests/render_e2e.rs` uses.
//!
//! Each of the six is a ratio between two blocks of pixels rather than an
//! absolute value, because an absolute one is a second golden written in
//! numbers: it moves when the tonemap moves, and it says nothing a reviewer can
//! act on.
//!
//! # Two capability paths, one golden
//!
//! Rule 12 asks a sample's CI run for "the path its runner selects plus one
//! below it", and an adapter reports what it reports — so the lesser path is
//! reached by opening a device *without* the features that select the better
//! one. [`the_room_draws_the_same_on_a_path_below_the_devices_own`] is that
//! second arm, and it is held to the **same** reference: a lesser path is a
//! constraint on data layout rather than a separate renderer, so a difference
//! between the arms is a bug and a second golden would bless it.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-lumen-golden.sh` is the only thing that turns both off — and it
//! fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use crcbl::hal::{AdapterInfo, BindingModel, Features, Format, GeometryPath};
use crcbl::math::Vec3;
use crcbl::render::{Camera, EffectOverride, EffectRequest, ForwardRenderer, RenderEffects};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl::shaders::probe::GpuProbe;
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_lumen::{Forced, room};

/// The extent the checked-in golden is blessed at.
///
/// The same 4:3 the fixed camera frames for, and the same size every other
/// golden in the tree is: small enough to read in a diff, large enough that the
/// blocks below are tens of pixels rather than a handful.
const EXTENT: (u32, u32) = (256, 192);

/// The extent [`the_room_reads_the_same_at_presentation_size`] renders at.
///
/// Twenty-five times the pixels, and the same aspect, so it is the same framing
/// rather than a different picture. Nothing is blessed at this size — what it is
/// for is stated on that test.
const REVIEW_EXTENT: (u32, u32) = (1280, 960);

/// Where a review-size frame is written, relative to the workspace root.
///
/// Under `target/`, which is already ignored, so a reviewer has a path to open
/// and CI has a directory it can upload with no new rule.
const REVIEW_DIR: &str = "target/lumen";

/// How many distinct colours a frame of this room has to have.
///
/// A room lit by a sun through a window, an orange lamp and a cool ambient, on
/// nine objects in five materials: a frame with fewer than this drew the clear
/// colour and very little else. Counted rather than guessed at — see
/// `Image::distinct_colors`, which stops counting at the bound it is given.
const MIN_COLORS: usize = 64;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the lighting: one texel of the floor's check, one
/// step of the occlusion blur, or a triangle edge landing a pixel either way
/// all move it. Small enough at [`EXTENT`] that a block stays on the surface it
/// was aimed at.
const BLOCK: (u32, u32) = (4, 4);

// ---------------------------------------------------------------------------
// The claims, as world positions
// ---------------------------------------------------------------------------

/// A floor point **inside** the shaft of sunlight through the window.
///
/// [`room::SUNLIT_FLOOR`] — derived in that module from the window's opening and
/// the sun's direction, and checked there with no GPU.
const SUNLIT: Vec3 = room::SUNLIT_FLOOR;

/// A floor point on the same surface, **outside** the shaft and out of the
/// lamp's reach, on [`SUNLIT`]'s terms.
const SHADED: Vec3 = room::SHADED_FLOOR;

/// How much brighter the sunlit floor must be than the shaded floor beside it.
///
/// A ratio, not a difference. Well above one because these two points differ in
/// the whole of the sun's direct contribution and in nothing else — same
/// surface, same normal, same depth, same material, same (absent) lamp — so a
/// frame in which they are close is a frame in which the sun is not coming
/// through the window at all. Not higher still: the shaded floor is ambient
/// rather than black, deliberately, and the tonemap compresses the bright end.
const SHAFT_RATIO: f32 = 2.0;

/// How much of nearby plaster's brightness the mirror's SSR-miss control point
/// must retain.
///
/// This is a non-black fallback control, not a claim that the mirror is brighter
/// than plaster or that it rejects whole-frame brightening. The fixed Vulkan run
/// measured a 0.24 ratio, so this leaves margin for rasterisation variation.
const MIRROR_FRACTION_OF_PLASTER: f32 = 0.20;

/// How much a fully screen-space hit may vary when the probe rows are zeroed.
///
/// The fixed Vulkan run measured a 5.1% difference at this pixel; 6% leaves
/// rasterisation margin while rejecting a fallback substituted for the hit.
const SSR_HIT_TOLERANCE: f32 = 0.06;

/// How much brighter the foot of the mirror panel is than its face further up.
///
/// The probe fallback fills both regions; the foot still has its screen-space
/// hit, so this is a modest relation rather than the old black-miss contrast.
const MIRROR_GRADIENT: f32 = 1.05;

/// The floor a lit block's mean brightness has to clear, out of 255.
///
/// What separates "correctly dark" from "nothing drew". Every ratio above is
/// satisfied by two blocks of zero, so each one is paired with this.
const LIT_FLOOR: f32 = 6.0;

/// How much of the coloured wall's brightness has to be red.
///
/// `base_color` is a factor the fragment stage multiplies in, so a material row
/// that never reached the device leaves this wall the same plaster as every
/// other — and the frame is a perfectly plausible picture of a white room. Well
/// above a third, which is what a neutral surface gives.
const BOUNCE_REDNESS: f32 = 0.55;

/// A point on the coloured wall, chosen where the lamp reaches it.
///
/// The lamp rather than the sun: the sun comes in low through a window in the
/// **opposite** wall, so what it reaches on this one is a band along its foot.
/// That is the room being a room, and it is also what makes this wall's absent
/// bounce legible — a wall in shadow beside a floor in full sun is exactly the
/// configuration global illumination would change.
const BOUNCE_AT: Vec3 = Vec3::new(room::HALF_WIDTH, 1.4, -2.0);

/// A point on the mirror-grade panel's `+Z` face, above everything of it that
/// reflects — see `crcbl_lumen::room::MIRROR_MISSES`, which is this point and
/// carries the proof.
const MIRROR_AT: Vec3 = room::MIRROR_MISSES;

/// A point on the plaster back wall, clear of the panel and of the plinth.
///
/// [`room::UNTINTED_PLASTER`] — the far half of the bounce claim below, and the
/// module that owns it is where the proof that nothing but the bounce separates
/// it from [`TINTED_AT`] lives.
const PLASTER_AT: Vec3 = room::UNTINTED_PLASTER;

/// The same plaster **beside the coloured wall**, mirrored across the room's
/// axis — [`room::TINTED_PLASTER`].
const TINTED_AT: Vec3 = room::TINTED_PLASTER;

/// How much redder, in red-to-blue, the plaster beside the coloured wall has to
/// read than the same plaster across the room.
///
/// **The rendered symptom of the coloured wall's isolated CPU contribution.** A
/// ratio of ratios, so it survives the tonemap and exposure the way every other
/// claim here does. `bounce::the_environment_beside_the_coloured_wall_is_the_red_one`
/// suppresses only `Face::Bounce` while preserving every neutral-surface bounce;
/// that control establishes this ratio's coloured source. This assertion verifies
/// the resulting rendered tint, not isolation by pixels alone.
///
/// Ten per cent, against a measured seventeen at [`EXTENT`] and nineteen at
/// [`REVIEW_EXTENT`]. Not higher: the two blocks differ in one term of the
/// shading added to a flat ambient, and the tonemap and the sRGB encode both
/// compress that difference rather than preserving it — the same tint is half as
/// large again in the linear irradiance the shader read, which is what
/// `crcbl_lumen::bounce`'s own CPU claim measures.
const BOUNCE_TINT: f32 = 1.10;

/// Floor in the plinth's contact corner, where ambient occlusion is strongest.
///
/// The plinth's `+Z` face stands at `z = -2.4` and its run in `x` covers this
/// point, so this is floor a few centimetres out from a wall of it — inside the
/// occlusion radius on one whole side. Out of the sun (the shaft is at `+x`) and
/// out of the lamp's reach at `t = 0`, exactly as [`SHADED`] is, so the two are a
/// pair that differs in occlusion and in nothing else.
const AO_CORNER: Vec3 = Vec3::new(-1.2, 0.0, -2.32);

/// How much brighter the shadowed floor must get when the atlas is switched off.
///
/// A ratio rather than a difference, and against the *shadowed* reading, because
/// what that block gains is the sun's whole direct contribution — the same
/// quantity [`SHAFT_RATIO`] measures from the other side. Set below the measured
/// lift so a driver that resolves the shaft's edge differently does not flip it;
/// well above one, because a frame in which the two are close is a frame whose
/// shadow switch did nothing.
const SHADOW_LIFT: f32 = 1.6;

/// How much brighter the contact corner must get when occlusion is switched off.
///
/// Smaller than [`SHADOW_LIFT`] and for a real reason: occlusion scales the
/// *ambient* term alone, so what this block can gain is bounded by the share of
/// it that is ambient — `docs/backlog.md` measures a quarter at a single wall.
/// This is a claim about that term, not about the pixel.
const AO_LIFT: f32 = 1.08;

/// How far a control block is allowed to move, as a share of its own reading.
///
/// Every claim above is paired with a block the same switch must **not** move,
/// which is what separates "this effect stopped darkening its own corner" from
/// "the whole frame got brighter". Not zero: the two frames are separate runs of
/// a rasteriser, and a block on a lit floor carries the texture's check.
const UNCHANGED: f32 = 0.02;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The selectors [`the_room_draws_the_same_on_a_path_below_the_devices_own`]
/// holds its lesser arm at.
///
/// The floor of both axes — the browser's shape, which
/// `docs/plan/sample/13-lumen.md` already names as the combination this desktop
/// can be made to run. The floor rather than one named step down because an
/// adapter reports what it reports: from here, *any* adapter offering anything
/// above the floor gives the two arms a real difference to compare, and the one
/// that offers nothing above it says so through the assertion rather than
/// quietly.
const BELOW: Forced = Forced {
    geometry: Some(GeometryPath::IndirectPerBatch),
    binding: Some(BindingModel::ArrayPages),
};

/// What to ask the lesser arm's device for: [`draw`]'s own set, minus the flags
/// whose presence would select something above [`BELOW`].
///
/// **The subtraction is `crcbl_lumen`'s, not this file's.** `Forced` is what the
/// binary's `--force-geometry` and `--force-binding` go through, so taking the
/// difference there and applying it here means a selector that grows a flag
/// moves this arm too, instead of leaving a second table behind still naming the
/// old ones.
///
/// It has to be a difference rather than `BELOW.optional_features()` outright,
/// because the two sets do **not** share a base:
/// `Forced::optional_features` starts from `GpuContextDesc::default`'s optional
/// set plus `TASK_SHADER`, which also carries the timestamp, present-feedback
/// and present-timing flags [`OffscreenSetup::OPTIONAL_FEATURES`] never asks
/// for. An arm opened from a different base than the one it is compared against
/// is not a comparison.
fn below_features() -> Features {
    let selecting = Forced::default()
        .optional_features()
        .difference(BELOW.optional_features());
    OffscreenSetup::OPTIONAL_FEATURES.difference(selecting)
}

/// Opens a device on the best path this adapter offers, builds the room on it,
/// and reads one frame back.
///
/// [`draw_with`] is this asking for something less, which is the whole of what
/// separates them: every claim below is about a frame, and a frame is the one
/// thing that does not say which tail drew it.
fn draw(extent: (u32, u32), effects: RenderEffects) -> (Image, String) {
    let (image, paths, _) =
        draw_with_probes(extent, effects, OffscreenSetup::OPTIONAL_FEATURES, false);
    (image, paths)
}

/// [`draw`] opening the device with `optional_features` instead of
/// [`OffscreenSetup::OPTIONAL_FEATURES`], and naming the adapter it opened.
///
/// Everything below the renderer — the offscreen surface, the adapter pin, the
/// ring, the barriers around the readback and the row unpadding — is
/// [`OffscreenSetup`]'s, reached through
/// [`OffscreenSetup::open_forward_with`](crcbl::screenshot::OffscreenSetup::open_forward_with).
/// A sample rebuilding that for itself is exactly what
/// `docs/plan/sample/00-samples-overview.md` rule 1 forbids.
///
/// The adapter comes back because two arms are only a comparison if they opened
/// the same one, and that is a claim the caller has to make.
fn draw_with(
    extent: (u32, u32),
    effects: RenderEffects,
    optional_features: Features,
) -> (Image, String, AdapterInfo) {
    draw_with_probes(extent, effects, optional_features, false)
}

/// [`draw_with`] with every authored probe row replaced by [`GpuProbe::ZERO`]
/// while retaining the same probe-grid volume and table capacity.
fn draw_with_probes(
    extent: (u32, u32),
    effects: RenderEffects,
    optional_features: Features,
    zero_probes: bool,
) -> (Image, String, AdapterInfo) {
    // A logger before anything opens: without one, every line a backend emits on
    // the way to a device goes nowhere, and a failure inside `open` names the
    // call that noticed rather than the one that caused it.
    crcbl::core::log::init_logging();

    let mut setup = OffscreenSetup::open_forward_with(
        extent.0,
        extent.1,
        optional_features,
        |device, queue, format| {
            Ok(ForwardScene {
                camera: room::fixed_camera(),
                sun: room::sun(),
                renderer: Box::new(build(device, queue, format, effects, zero_probes)?),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for lumen's room: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    // Printed unconditionally and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "lumen golden: device on adapter {id} {name:?} type={kind:?}",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
    );
    let paths = format!(
        "{backend} {:?} / {:?} / {:?}",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );
    eprintln!(
        "lumen golden: {paths} at {}x{}, asked for {optional_features:?}",
        extent.0, extent.1,
    );
    // **The device landed on exactly the path its request names.** The frame
    // alone cannot say — every path draws this room identically by construction
    // — so an arm on a tail other than the one it asked for would leave every
    // assertion below still passing. Met against the adapter, so on the default
    // request this is the claim it has always made, "the best path the adapter
    // offers"; on a forced one it is the lesser path, and an arm that got the
    // better tail anyway is a self-comparison wearing a cross-path label.
    let granted = optional_features.intersection(adapter.caps.features);
    assert_eq!(
        (caps.geometry_path(), caps.binding_model()),
        (
            GeometryPath::from_features(granted),
            BindingModel::from_features(granted),
        ),
        "adapter {} offers {:?}, this run asked for {optional_features:?}, and the device \
         opened on {:?} / {:?}",
        adapter.name,
        adapter.caps.features,
        caps.geometry_path(),
        caps.binding_model(),
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else.
    setup.finish().expect("the device reaches idle");

    assert_eq!(
        (width, height),
        extent,
        "the swapchain handed back an extent nothing was measured at"
    );
    let order = if format == Format::Bgra8UnormSrgb || format == Format::Bgra8Unorm {
        ChannelOrder::Bgra
    } else {
        ChannelOrder::Rgba
    };
    let image = Image::from_readback(width, height, &pixels, order)
        .expect("the readback is exactly one image");
    (image, paths, adapter)
}

/// The room, made resident and placed, on a device the caller opened, drawing
/// `effects`.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    effects: RenderEffects,
    zero_probes: bool,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let mut scene = room::room();
    if zero_probes {
        scene.probes.probes.fill(GpuProbe::ZERO);
    }
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // The **programmatic** layer of topic 39's resolution order, which is the
    // one a test has any business driving — see `crcbl::render::effects`.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(effects), Some(false)),
        ..EffectRequest::default()
    });
    if let Err(error) = room::place(&mut renderer) {
        renderer.destroy(device);
        return Err(crcbl::screenshot::OffscreenError::Hal(
            crcbl::hal::HalError::InvalidDescriptor(format!(
                "lumen's room does not fit its own instance pool: {error}"
            )),
        ));
    }
    // **`t = 0`, like every other screenshot in the tree.** The lamp's orbit is
    // a pure function of the time, so a golden is only worth comparing at a time
    // both runs agree on — and zero is the one the app's own first frame draws.
    renderer.set_lights(&[room::lamp(0.0)]);
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in the frame, in pixels.
///
/// Through the very same [`Camera::view_projection`] the frame was drawn with,
/// so a claim about a surface is a claim about the pixels that surface actually
/// covers — rather than about a hand-derived mapping that a change of camera
/// would silently invalidate.
///
/// The projection produces **Y-up** normalised device coordinates with depth in
/// `0..1`; the framebuffer's rows run the other way, which is the flip below.
fn project(camera: &Camera, extent: (u32, u32), point: Vec3) -> (u32, u32) {
    #[allow(clippy::cast_precision_loss)]
    let aspect = extent.0 as f32 / extent.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    assert!(
        clip.w > 0.0,
        "{point:?} is behind the camera, so nothing in the frame is about it"
    );
    let ndc = clip.truncate() / clip.w;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let x = (ndc.x + 1.0) * 0.5 * width;
    let y = (1.0 - ndc.y) * 0.5 * height;
    assert!(
        x >= 0.0 && x < width && y >= 0.0 && y < height,
        "{point:?} projects to ({x:.1}, {y:.1}), outside a {width}x{height} frame — \
         the claim about it would be about a pixel that is not there"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x as u32, y as u32)
}

/// Mean luminance of a block of pixels around `centre`, out of 255.
///
/// Unweighted across the three channels: what every claim here is about is how
/// much light reached a surface, and a perceptual weighting would make the
/// coloured wall's red count for a third of what the plaster's white does.
fn brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image
                .pixel(x, y)
                .unwrap_or_else(|| panic!("({x}, {y}) is inside the frame"));
            total += (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// The mean of one channel over the same block.
fn channel(image: &Image, centre: (u32, u32), half: (u32, u32), index: usize) -> f32 {
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image
                .pixel(x, y)
                .unwrap_or_else(|| panic!("({x}, {y}) is inside the frame"));
            total += f32::from(pixel[index]);
            count += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// Every claim this suite makes about the lighting, at whatever extent.
///
/// One function so the golden's extent and the review extent assert the *same*
/// things: a structural claim that only held at 256×192 would be a claim about
/// the rasteriser's sampling rather than about the room.
fn inspect(image: &Image, extent: (u32, u32), block: (u32, u32)) {
    let camera = room::fixed_camera();
    let colors = image.distinct_colors(MIN_COLORS);
    assert!(
        colors >= MIN_COLORS,
        "a room with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not \
         evidence — nothing drew, or only the clear did"
    );

    // ---- 1. the sun comes through the window --------------------------------
    let sunlit = brightness(image, project(&camera, extent, SUNLIT), block);
    let shaded = brightness(image, project(&camera, extent, SHADED), block);
    assert!(
        sunlit > LIT_FLOOR,
        "the sunlit floor is at {sunlit:.1}/255, which is not a lit surface at all"
    );
    assert!(
        sunlit > shaded * SHAFT_RATIO,
        "the floor inside the window's shaft is {sunlit:.1} and the floor outside it is \
         {shaded:.1} — the sun is not coming through the opening, or the wall casts no \
         shadow at all"
    );

    // ---- 2. the shaded floor is ambient rather than black -------------------
    //
    // The other half of claim 1, and the one that stops it being satisfied by a
    // frame that is simply dark: `sunlit > shaded * SHAFT_RATIO` holds for
    // `shaded == 0`, which is what a lost ambient term looks like. Shown red by
    // zeroing `room::sun`'s ambient, which takes this block to exactly nothing.
    assert!(
        shaded > LIT_FLOOR,
        "the shaded floor is at {shaded:.1}/255, so the ambient term reached nothing"
    );

    // ---- 3. a conductor reflects the probe environment on an SSR miss --------
    let mirror = brightness(image, project(&camera, extent, MIRROR_AT), block);
    // Read again by claim 6 below, which is the far half of the bounce pair.
    let at_plaster = project(&camera, extent, PLASTER_AT);
    let plaster = brightness(image, at_plaster, block);
    assert!(
        plaster > LIT_FLOOR,
        "the plaster wall is at {plaster:.1}/255, so there is nothing to compare the \
         mirror against"
    );
    assert!(
        mirror > LIT_FLOOR && mirror > plaster * MIRROR_FRACTION_OF_PLASTER,
        "the SSR-miss point is {mirror:.1} and nearby plaster is {plaster:.1} — the probe \
         fallback is absent or the probe buffer was not bound"
    );

    // ---- 4. what a conductor owes the room instead is a reflection ----------
    //
    // The block hangs directly **above** the panel's bottom edge rather than
    // being centred on a point: the band that reflects is the lowest eighth of
    // the face, so a block centred in it would have to be a different size from
    // every other claim's, and one centred on the edge would take in the plinth
    // below. Sitting it on the edge keeps the caller's own block and puts every
    // reflecting row of the face inside it.
    let foot = project(&camera, extent, room::MIRROR_FOOT);
    let reflecting = brightness(image, (foot.0, foot.1 - block.1), block);
    assert!(
        reflecting > LIT_FLOOR,
        "the foot of the mirror panel is at {reflecting:.1}/255 — a conductor with no \
         ambient and no reflection is black, and this one still is"
    );
    assert!(
        reflecting > mirror * MIRROR_GRADIENT,
        "the mirror panel reads {reflecting:.1} at its foot and {mirror:.1} further up the \
         same face — same row, same normal, same F0, same roughness and no direct light on \
         either, so this says the reflection is not following the geometry"
    );

    // ---- 5. the material table's colour factor reached the device -----------
    let at = project(&camera, extent, BOUNCE_AT);
    let red = channel(image, at, block, 0);
    let green = channel(image, at, block, 1);
    let blue = channel(image, at, block, 2);
    let sum = red + green + blue;
    assert!(
        sum > 3.0 * LIT_FLOOR,
        "the coloured wall is at {:.1}/255, so nothing lit it",
        sum / 3.0
    );
    assert!(
        red / sum > BOUNCE_REDNESS,
        "the coloured wall reads {red:.1} / {green:.1} / {blue:.1} — its material row's \
         base-colour factor did not reach the fragment stage"
    );

    // ---- 6. and the coloured wall tints the plaster beside it ---------------
    //
    // Two blocks of the back wall's inner face at one height, mirrored across
    // the room's axis: same material row, same normal, same depth, neither in
    // the sun and neither in the lamp's reach —
    // `room::the_two_back_wall_samples_differ_in_the_bounce_and_in_nothing_else`
    // is what proves each has matching direct-light terms with no GPU. The CPU
    // bounce control suppresses only Face::Bounce while retaining neutral rows;
    // this assertion observes that isolated source's rendered tint rather than
    // claiming the two pixel blocks isolate it on their own.
    let tinted = project(&camera, extent, TINTED_AT);
    // The block has to stay off the coloured wall itself, or this measures the
    // material row rather than its bounce — and how many pixels that is depends
    // on the extent, so it is asked of the projection rather than written down.
    let corner = project(
        &camera,
        extent,
        Vec3::new(room::HALF_WIDTH, TINTED_AT.y, TINTED_AT.z),
    );
    assert!(
        tinted.0 + block.0 < corner.0,
        "the block beside the coloured wall reaches to {} and the wall's own corner is at \
         {} — this block has the wall in it",
        tinted.0 + block.0,
        corner.0
    );
    let tinted_brightness = brightness(image, tinted, block);
    assert!(
        tinted_brightness > LIT_FLOOR,
        "the plaster beside the coloured wall is at {tinted_brightness:.1}/255, so there is \
         nothing to take a ratio of"
    );
    let tinted_redness = channel(image, tinted, block, 0) / channel(image, tinted, block, 2);
    let plain_redness = channel(image, at_plaster, block, 0) / channel(image, at_plaster, block, 2);
    assert!(
        tinted_redness > plain_redness * BOUNCE_TINT,
        "the plaster beside the coloured wall reads a red-to-blue of {tinted_redness:.3} and \
         the same plaster across the room {plain_redness:.3} — the CPU control isolates the \
         coloured wall's contribution, and its rendered tint is missing"
    );
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// **The room, from the fixed camera, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn the_fixed_camera_draws_the_room_and_matches_its_golden() {
    let (image, paths) = draw(EXTENT, RenderEffects::all());
    inspect(&image, EXTENT, BLOCK);
    check_golden(&image, &paths);
}

/// **Zeroing authored probe rows darkens an SSR miss but not an SSR hit.**
///
/// The zero control retains the authored volume and row count, so it exercises
/// the same binding and lookup shape while removing only the fallback radiance.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn zero_probes_only_remove_the_ssr_miss_fallback() {
    let effects = RenderEffects::all();
    let (authored, _, authored_adapter) =
        draw_with(EXTENT, effects, OffscreenSetup::OPTIONAL_FEATURES);
    let (zeroed, _, zeroed_adapter) =
        draw_with_probes(EXTENT, effects, OffscreenSetup::OPTIONAL_FEATURES, true);
    assert_eq!(
        authored_adapter, zeroed_adapter,
        "the authored and zero-probe frames opened different adapters, so they are not a control"
    );

    let camera = room::fixed_camera();
    let miss = project(&camera, EXTENT, MIRROR_AT);
    let foot = project(&camera, EXTENT, room::MIRROR_FOOT);
    let hit = (foot.0, foot.1.saturating_sub(1));
    let authored_miss = brightness(&authored, miss, BLOCK);
    let zeroed_miss = brightness(&zeroed, miss, BLOCK);
    let authored_hit = brightness(&authored, hit, (1, 1));
    let zeroed_hit = brightness(&zeroed, hit, (1, 1));
    eprintln!(
        "lumen probes: SSR miss {authored_miss:.1} -> {zeroed_miss:.1}, \
         SSR hit {authored_hit:.1} -> {zeroed_hit:.1}"
    );
    assert!(
        authored_miss > LIT_FLOOR && zeroed_miss <= 1.0,
        "the SSR miss reads {authored_miss:.1} with authored probes and {zeroed_miss:.1} with \
         zero rows — the fallback must be the only light removed"
    );
    let hit_tolerance_percent = SSR_HIT_TOLERANCE * 100.0;
    assert!(
        (authored_hit - zeroed_hit).abs() < authored_hit * SSR_HIT_TOLERANCE,
        "the real SSR hit moved from {authored_hit:.1} to {zeroed_hit:.1}; the measured \
         {hit_tolerance_percent:.0}% tolerance covers the remaining probe blend but not \
         replacing a valid screen hit with the fallback"
    );
}

/// **The same room on the path below this device's own, against the same
/// golden.**
///
/// `docs/plan/sample/00-samples-overview.md` rule 12 asks each sample's CI run
/// to exercise "the path its runner selects plus one below it". Every other test
/// here opens through [`draw`], which asks for
/// [`OffscreenSetup::OPTIONAL_FEATURES`] — so without this one every frame the
/// suite draws comes off the best tail the adapter reports, and the lesser ones,
/// which is what browsers and Apple devices run, are code no run here executes.
/// The sample already *said* which path it took; this is what makes it take more
/// than one.
///
/// # One golden, both arms
///
/// Both frames are held to `tests/golden/room.png` rather than each to a
/// reference of its own. `docs/plan/03-gpu-driven-rendering.md` §3.5's design
/// rule is that a lesser path is a constraint on data layout and not a separate
/// renderer, so a difference between the arms is a **bug in the better path** —
/// and a second reference is exactly what would bless it.
///
/// # It cannot pass vacuously
///
/// Two frames drawn by the same code match perfectly, so the arms have to have
/// actually differed. Both are asserted to open the same adapter, and the
/// selectors they resolve to are asserted to differ **exactly when that adapter
/// offers one of the flags [`BELOW`] withholds**. A device already at the floor
/// of both axes — a software rasteriser, `crcbl-wgpu` — is a legitimate run of
/// this test, and the printed line and that assertion are what keep it from
/// being a silent one.
///
/// That pair is each other's alibi, though: a [`below_features`] withholding
/// *nothing* leaves both halves false on every machine, and the comparison then
/// holds trivially — which is what it did the first time it was broken on
/// purpose to watch it fail. So the withheld set is asserted non-empty first,
/// which is the claim that the second arm is a second arm at all.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn the_room_draws_the_same_on_a_path_below_the_devices_own() {
    let below = below_features();
    // Only the flags a *selector* reads decide whether the arms can differ:
    // `TASK_SHADER` comes out beside `MESH_SHADER` because it is an
    // amplification stage in front of one, and no selector reads it — see
    // `GeometryPath::INPUTS`, which is the table `downgrades` answers from.
    let withheld = OffscreenSetup::OPTIONAL_FEATURES
        .difference(below)
        .intersection(GeometryPath::INPUTS.union(BindingModel::INPUTS));
    // **Before either device opens**, and before the two claims below, which are
    // each other's alibi otherwise: an empty set here makes "the arms differ" and
    // "the adapter offers something better" both false on every machine, and the
    // test would then compare a frame against itself and report a lesser path
    // exercised.
    assert!(
        !withheld.is_empty(),
        "the lesser arm asks for {below:?}, which withholds no selector input at all — it is \
         the same request as the arm it is compared against"
    );

    let (best, best_paths, adapter) = draw_with(
        EXTENT,
        RenderEffects::all(),
        OffscreenSetup::OPTIONAL_FEATURES,
    );
    let (lesser, lesser_paths, lesser_adapter) = draw_with(EXTENT, RenderEffects::all(), below);
    assert_eq!(
        adapter, lesser_adapter,
        "the two arms opened different adapters, so they are not a comparison"
    );

    let offers_better = adapter.caps.features.intersects(withheld);
    eprintln!(
        "lumen golden: {best_paths} against {lesser_paths} — withheld {withheld:?}, \
         adapter {name} has {held:?}",
        name = adapter.name,
        held = adapter.caps.features.intersection(withheld),
    );
    assert_eq!(
        best_paths != lesser_paths,
        offers_better,
        "the adapter {} one of {withheld:?} and the two arms resolved to {best_paths} and \
         {lesser_paths} — one of those two facts is wrong, and a self-comparison that reads \
         as a cross-path one is worse than no test",
        if offers_better {
            "offers"
        } else {
            "offers none of"
        },
    );

    // Every claim, on every arm: the golden is a comparison of pixels and the
    // six in front of it are what say the frame holds the room at all, so an
    // arm that lost a mesh or a material row on the way down its lesser tail
    // fails on the claim rather than on a diff nobody can read.
    for (image, paths) in [(&best, &best_paths), (&lesser, &lesser_paths)] {
        inspect(image, EXTENT, BLOCK);
        check_golden(image, paths);
    }
}

/// Holds one frame to the checked-in reference, and says what it found.
///
/// `label` is the arm's [`draw_with`] path row, so a failure names the tail the
/// frame came off rather than only the frame.
fn check_golden(image: &Image, label: &str) {
    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/room.png");
    let comparison = Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("on {label}: {message}"));
    eprintln!("lumen golden: room on {label} — {}", comparison.summary());
}

/// **The same claims at twenty-five times the pixels**, written where a human
/// can look at it.
///
/// Two jobs, and the first is the one that makes this a test. Every ratio in
/// [`inspect`] is measured over a block a few pixels across, and at 256×192 a
/// block is a large fraction of the surface it sits on — so a claim that
/// happened to hold because of where one triangle edge landed would hold at that
/// extent and nowhere else. Re-running them at [`REVIEW_EXTENT`], where the same
/// world positions cover twenty-five times the area, is what distinguishes a
/// measurement of the room from a measurement of the sampling.
///
/// The second is that the frame is saved: a lighting fixture is a thing people
/// look at, and 256×192 is not a size anybody can judge a shadow's edge at.
/// Nothing is blessed — there is no reference at this extent — so the picture is
/// an artefact of the run rather than a gate.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn the_room_reads_the_same_at_presentation_size() {
    let image = review(RenderEffects::all(), "fixed-camera");

    // The blocks grow with the frame, so each covers the same patch of the room
    // rather than a twenty-fifth of it.
    inspect(&image, REVIEW_EXTENT, review_block());
}

/// **Switching an effect off changes the frame where that effect darkens it, and
/// leaves the rest of the room alone.**
///
/// The charter's "every effect toggles independently", as the one thing a
/// picture can actually say about it. `crcbl-render`'s own tests already show the
/// toggle reaches the recorded pass list; what they cannot show is that the
/// passes it removed were the ones doing the darkening — a switch wired to the
/// wrong effect, or an occlusion placeholder that is not white, produces a frame
/// with the right passes in it and the wrong picture.
///
/// Each claim is therefore **a pair of blocks over a pair of frames**: one block
/// where the effect works and one where it does not, so a frame that simply got
/// brighter — a lost tonemap, a different exposure — fails the control half.
///
/// Nothing here is blessed. One frame per state is written for a reviewer, at
/// the size a shadow's edge can be judged at.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn every_effect_toggles_and_the_frame_says_so() {
    let block = review_block();
    let camera = room::fixed_camera();
    let at = |point: Vec3| project(&camera, REVIEW_EXTENT, point);

    let all_on = review(RenderEffects::all(), "all-effects");
    let no_shadows = review(
        RenderEffects::all().difference(RenderEffects::SHADOWS),
        "no-shadows",
    );
    let no_ao = review(
        RenderEffects::all().difference(RenderEffects::AMBIENT_OCCLUSION),
        "no-ao",
    );
    let no_reflections = review(
        RenderEffects::all().difference(RenderEffects::REFLECTIONS),
        "no-reflections",
    );

    // ---- shadows -----------------------------------------------------------
    //
    // `SHADED_FLOOR` is floor in the window wall's own shadow and `SUNLIT_FLOOR`
    // is floor inside the shaft: with no shadow atlas the first one takes the
    // sun it was being denied and the second one, already lit, does not move.
    let shaded_on = brightness(&all_on, at(SHADED), block);
    let shaded_off = brightness(&no_shadows, at(SHADED), block);
    let sunlit_on = brightness(&all_on, at(SUNLIT), block);
    let sunlit_off = brightness(&no_shadows, at(SUNLIT), block);
    eprintln!(
        "lumen toggles: shadowed floor {shaded_on:.1} -> {shaded_off:.1}, \
         sunlit floor {sunlit_on:.1} -> {sunlit_off:.1}"
    );
    assert!(
        shaded_off > shaded_on * SHADOW_LIFT,
        "the shadowed floor reads {shaded_on:.1} with shadows and {shaded_off:.1} without — \
         switching the atlas off did not stop the wall occluding the sun"
    );
    assert!(
        (sunlit_off - sunlit_on).abs() < sunlit_on * UNCHANGED,
        "the sunlit floor moved from {sunlit_on:.1} to {sunlit_off:.1} — a floor already in \
         full sun has no shadow to lose, so this is the whole frame changing rather than the \
         shadows"
    );

    // ---- ambient occlusion -------------------------------------------------
    //
    // Two blocks of the same floor, same material, same normal, both out of the
    // sun and out of the lamp's reach: one in the plinth's contact corner and
    // one out in the open. Occlusion is the only term that separates them, so
    // switching it off has to lift the first and leave the second where it is.
    let corner_on = brightness(&all_on, at(AO_CORNER), block);
    let corner_off = brightness(&no_ao, at(AO_CORNER), block);
    let open_on = brightness(&all_on, at(SHADED), block);
    let open_off = brightness(&no_ao, at(SHADED), block);
    eprintln!(
        "lumen toggles: contact corner {corner_on:.1} -> {corner_off:.1}, \
         open floor {open_on:.1} -> {open_off:.1}"
    );
    assert!(
        corner_off > corner_on * AO_LIFT,
        "the plinth's contact corner reads {corner_on:.1} with occlusion and {corner_off:.1} \
         without — the occlusion pass is not darkening the corner, or the placeholder bound in \
         its place is not white"
    );
    assert!(
        (open_off - open_on).abs() < open_on * UNCHANGED,
        "open floor moved from {open_on:.1} to {open_off:.1} — a surface with nothing within \
         the occlusion radius has no occlusion to lose"
    );

    // ---- reflections -------------------------------------------------------
    //
    // The two blocks the [`MIRROR_GRADIENT`] claim already reads: the foot of the
    // panel, whose reflected ray finds the floor while still on screen, and a
    // point further up the same face whose ray finds nothing. With the march off
    // the first one has to lose what it was gaining and the second one, which was
    // gaining nothing, cannot move at all.
    //
    // The control is the strong half here. A conductor with no ambient is near
    // black, so "the foot got darker" is satisfied by a frame that went dark
    // everywhere; a face that reflected nothing either way and stayed exactly put
    // is what says the pass came out and nothing else did.
    let foot = at(room::MIRROR_FOOT);
    let foot_block = (foot.0, foot.1 - block.1);
    let foot_on = brightness(&all_on, foot_block, block);
    let foot_off = brightness(&no_reflections, foot_block, block);
    let missing_on = brightness(&all_on, at(MIRROR_AT), block);
    let missing_off = brightness(&no_reflections, at(MIRROR_AT), block);
    eprintln!(
        "lumen toggles: mirror foot {foot_on:.1} -> {foot_off:.1}, \
         mirror face {missing_on:.1} -> {missing_off:.1}"
    );
    assert!(
        foot_off * MIRROR_GRADIENT < foot_on,
        "the mirror panel's foot reads {foot_on:.1} with the march and {foot_off:.1} without — \
         a conductor with no ambient and no reflection is black, and this one is not"
    );
    assert!(
        missing_on > LIT_FLOOR && missing_off <= 1.0,
        "the SSR-miss point reads {missing_on:.1} with reflections and {missing_off:.1} without — \
         the probe fallback was not supplied by the reflection pass"
    );
}

/// One frame at [`REVIEW_EXTENT`], written where a reviewer can open it.
///
/// **Written before any claim is checked**, on `Golden::check`'s terms: a run
/// that is about to fail is exactly the run somebody wants the picture from, and
/// a save after the assertions is a save a failure skips.
fn review(effects: RenderEffects, name: &str) -> Image {
    let (image, paths) = draw(REVIEW_EXTENT, effects);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!(
        "{name}-{}x{}.png",
        REVIEW_EXTENT.0, REVIEW_EXTENT.1
    ));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("lumen golden: {paths} review frame at {}", path.display());
    image
}

/// The block every claim at [`REVIEW_EXTENT`] averages over.
///
/// [`BLOCK`] scaled by the extent ratio, so a block covers the same patch of the
/// room rather than a twenty-fifth of it.
fn review_block() -> (u32, u32) {
    let scale = REVIEW_EXTENT.0 / EXTENT.0;
    (BLOCK.0 * scale, BLOCK.1 * scale)
}
