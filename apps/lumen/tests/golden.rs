//! The room off a real device, from the fixed camera, against a checked-in
//! golden — and five claims about the lighting in front of it.
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
//! Each of the five is a ratio between two blocks of pixels rather than an
//! absolute value, because an absolute one is a second golden written in
//! numbers: it moves when the tonemap moves, and it says nothing a reviewer can
//! act on.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-lumen-golden.sh` is the only thing that turns both off — and it
//! fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use crcbl::hal::Format;
use crcbl::math::Vec3;
use crcbl::render::{Camera, ForwardRenderer};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_lumen::room;

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
// The five claims, as world positions
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

/// How much darker the mirror-grade panel is than the plaster behind it.
///
/// **What was the sample's central honest claim, and is now half of it.** Both
/// points face `+Z`, both are out of the lamp's reach and both are out of the
/// sun's, so the direct term is zero on each and the only thing left is the
/// ambient — which scales the *diffuse* albedo, and a conductor has none. That
/// is the model `docs/plan/18-render-features.md` documents, and this is what
/// says the frame still shows it: a build in which metals had picked up an
/// ambient term would fail here rather than quietly getting prettier.
///
/// The model has since gained the other half — a conductor with no ambient
/// **reflects** instead — so this number is read at [`MIRROR_AT`], which is above
/// the band of the panel that reflects anything, and [`MIRROR_GRADIENT`] is the
/// claim about the band. The number itself has not moved: lowering it to keep it
/// green would be making the test pass the code.
const METAL_DARKNESS: f32 = 3.0;

/// How much brighter the foot of the mirror panel is than its face further up.
///
/// **The sample's central honest claim now.** The panel looks straight at the
/// camera, which is close to screen-space reflection's worst case: a ray leaving
/// it goes back past the eye, so only the lowest part of the face sends one that
/// reaches the floor while still inside the frame —
/// `crcbl_lumen::room::MIRROR_FOOT` carries the geometry and
/// `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head` derives the band
/// with no GPU. The two blocks are one material row, one face, one normal, one
/// `F0`, one roughness and no direct light on either; the only thing that differs
/// is whether the ray finds anything.
///
/// Three, and it is the ratio [`METAL_DARKNESS`] used to make about the whole
/// panel — because the control here is a *conductor with no reflection*, which
/// this model says is black, and the frame this was set against measures it at
/// exactly zero against a foot near twenty. A build that reflected everywhere,
/// or one that reflected nothing, closes that gap from one end or the other.
const MIRROR_GRADIENT: f32 = 3.0;

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
const PLASTER_AT: Vec3 = Vec3::new(-2.6, 1.5, -room::HALF_DEPTH);

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Opens a device, builds the room on it, and reads one frame back.
///
/// Everything below the renderer — the offscreen surface, the adapter pin, the
/// ring, the barriers around the readback and the row unpadding — is
/// [`OffscreenSetup`]'s, reached through
/// [`OffscreenSetup::open_forward`](crcbl::screenshot::OffscreenSetup::open_forward).
/// A sample rebuilding that for itself is exactly what
/// `docs/plan/sample/00-samples-overview.md` rule 1 forbids.
fn draw(extent: (u32, u32)) -> (Image, String) {
    // A logger before anything opens: without one, every line a backend emits on
    // the way to a device goes nowhere, and a failure inside `open` names the
    // call that noticed rather than the one that caused it.
    crcbl::core::log::init_logging();

    let mut setup = OffscreenSetup::open_forward(extent.0, extent.1, |device, queue, format| {
        Ok(ForwardScene {
            camera: room::fixed_camera(),
            sun: room::sun(),
            renderer: Box::new(build(device, queue, format)?),
        })
    })
    .unwrap_or_else(|why| panic!("a GPU backend opens for lumen's room: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter();
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
    eprintln!("lumen golden: {paths} at {}x{}", extent.0, extent.1);
    // The device took the best path its adapter offers. The frame alone cannot
    // say — every path draws this room identically by construction — so a
    // request that omitted a selector's flag would leave the renderer on a
    // lesser tail with every assertion below still passing.
    assert_eq!(
        caps.geometry_path(),
        adapter.caps.geometry_path(),
        "adapter {} offers {:?} and the device opened on {:?}",
        adapter.name,
        adapter.caps.geometry_path(),
        caps.geometry_path(),
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
    (image, paths)
}

/// The room, made resident and placed, on a device the caller opened.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &room::room())?;
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

    // ---- 3. a conductor has no ambient, and it shows ------------------------
    let mirror = brightness(image, project(&camera, extent, MIRROR_AT), block);
    let plaster = brightness(image, project(&camera, extent, PLASTER_AT), block);
    assert!(
        plaster > LIT_FLOOR,
        "the plaster wall is at {plaster:.1}/255, so there is nothing to compare the \
         metal against"
    );
    assert!(
        mirror * METAL_DARKNESS < plaster,
        "the mirror panel is at {mirror:.1} and the plaster behind it at {plaster:.1} — a \
         fully metallic surface has no diffuse albedo for the ambient to scale, so this \
         says metals picked one up"
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
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// **The room, from the fixed camera, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lumen-golden.sh"]
fn the_fixed_camera_draws_the_room_and_matches_its_golden() {
    let (image, paths) = draw(EXTENT);
    inspect(&image, EXTENT, BLOCK);

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/room.png");
    let comparison = Golden::new(reference)
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    eprintln!("lumen golden: room on {paths} — {}", comparison.summary());
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
    let (image, _) = draw(REVIEW_EXTENT);

    // **Written before the claims are checked**, on `Golden::check`'s terms: a
    // run that is about to fail is exactly the run somebody wants the picture
    // from, and a save after the assertions is a save a failure skips.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!(
        "fixed-camera-{}x{}.png",
        REVIEW_EXTENT.0, REVIEW_EXTENT.1
    ));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("lumen golden: review frame at {}", path.display());

    // The blocks grow with the frame, so each covers the same patch of the room
    // rather than a twenty-fifth of it.
    let scale = REVIEW_EXTENT.0 / EXTENT.0;
    inspect(&image, REVIEW_EXTENT, (BLOCK.0 * scale, BLOCK.1 * scale));
}
