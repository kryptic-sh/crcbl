//! The render layer, on whichever backend the registry opens — one frame of
//! every [`Scene`] through the engine's own renderers, read back and compared
//! against a checked-in golden.
//!
//! # Why this exists, and why here
//!
//! `docs/backlog.md`'s "The render layer has only ever run on Vulkan and wgpu"
//! is the gap: the frame graph, the cull pass, draw generation, forward and
//! tonemap execute on `crcbl-vk` (`crates/crcbl-vk/tests/vk_e2e/mesh.rs`) and on
//! native wgpu, and on nothing else. `crcbl-mtl`'s own suite proves the *HAL* —
//! dispatch, encoders, bindings, copies — and has never constructed a
//! [`ForwardRenderer`]. A green `mtl e2e` is therefore not evidence about the
//! renderer.
//!
//! This test is in `crcbl` rather than in `crcbl-mtl` because `crcbl` is the
//! only crate that depends on the renderer *and* on the backends: `crcbl-render`
//! is above the seam and names no backend, `crcbl-mtl` is below it and names no
//! renderer. A dev-dependency from `crcbl-mtl` on `crcbl-render` would be
//! acyclic — `crcbl-render`'s manifest takes `crcbl-hal` and nothing below it —
//! but it would have to rebuild the offscreen surface, swapchain, readback and
//! row-unpadding that [`crate::screenshot`](crcbl::screenshot) already owns and
//! that `tests/run-cross-backend-e2e.sh` already drives, and it could not assert
//! the thing a Metal run most needs asserted: that the *registry* picked Metal.
//!
//! # One test per scene, every backend
//!
//! [`OffscreenSetup::open`] opens whatever [`crcbl::backend::open`] selects, so
//! `CRCBL_GPU` decides which backend draws and this file is the same set of
//! tests on all of them. That is deliberate rather than incidental:
//!
//! * The golden is only trustworthy while something keeps re-deriving it, and
//!   the Metal arm cannot (see `run-render-e2e.sh` and the `mtl-e2e` job). The
//!   Vulkan arm is what stops it rotting.
//! * `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5 — a shader
//!   compiles to all four targets and *means something different on each* — has
//!   already cost this repo two real bugs (`SV_InstanceID`, `SV_VertexID`), and
//!   both were caught only by rendering one scene through two targets. MSL is a
//!   target nothing has ever crossed. Comparing Metal's frame against a
//!   Vulkan-blessed reference is the same detector pointed at the third target.
//!
//! # Why all three scenes, and why one test each
//!
//! `docs/backlog.md`'s "Decided: the four-backend compare is more scenes in
//! `render_e2e`, not a new job". This file used to draw [`Scene::Cube`] alone,
//! which exercises `mesh.slang` and `tonemap.slang`; `sprite.slang` and
//! `ui.slang` were compared across targets only by
//! `tests/run-cross-backend-e2e.sh`, which runs Vulkan against wgpu. Those two
//! shaders are precisely the ones that have diverged per target here —
//! `SV_InstanceID` in the sprite pass, `SV_VertexID` in the mesh pool — so MSL
//! and DXIL had no comparison at all for the code with the actual history.
//!
//! One `#[test]` per scene rather than one that loops, because the name is the
//! failure report: a runner nobody can attach a debugger to prints the test
//! name and a diff, and `the_sprite_scene_…` has already said which shader
//! broke before anyone reads the numbers. It also means `-E 'test(sprite)'`
//! reruns the one scene under investigation instead of three device opens.

#![cfg(feature = "render-e2e")]

use crcbl::adapter::{ADAPTER_ENV_VAR, device_type_from_name};
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::hal::Format;
use crcbl::screenshot::{OffscreenSetup, Scene};
use crcbl_golden::{ChannelOrder, Golden, Image};

/// The size the goldens were blessed at.
///
/// The same 256x192 the cross-backend harness and `crcbl-vk`'s mesh suite use,
/// and for the reason those state: the structural metric averages over 8x8
/// blocks, and a smaller frame gives it too few of them to mean anything.
const EXTENT: (u32, u32) = (256, 192);

/// The anti-vacuity floor for [`Scene::Cube`]: distinct RGBA colours the frame
/// must contain.
///
/// Two blank frames compare perfectly, so a tolerance alone cannot tell "the
/// same picture" from "no picture". Measured by `run-cross-backend-e2e.sh` on
/// both ICDs at both of its sizes: the cube scene has 44-49 distinct colours and
/// a cleared frame has one. This floor is that harness's own
/// `CRCBL_CROSS_MIN_COLORS_CUBE`, so losing the cube, the pyramid or the
/// tonemap trips it.
const MIN_COLORS_CUBE: usize = 16;

/// The same, for [`Scene::Sprite`]: `CRCBL_CROSS_MIN_COLORS_SPRITE`.
///
/// That harness measured 17-24 distinct colours for this scene on both ICDs at
/// both sizes, and its floor sits just under the minimum — so losing one of the
/// three batches trips it.
const MIN_COLORS_SPRITE: usize = 16;

/// The same, for [`Scene::Ui`]: `CRCBL_CROSS_MIN_COLORS_UI`.
///
/// The UI frame is the least colourful of the three — the clear, the panel, the
/// translucent bar over each of those, the outline and two text colours, which
/// that harness measured as 7 on both ICDs at both sizes. A floor of 16 would
/// fail a correct frame; this one fails a frame that lost an element.
const MIN_COLORS_UI: usize = 6;

/// How far apart a channel must be before two pixels count as different colours
/// rather than the same one off two rasterisers.
///
/// The comparison below runs at `crcbl_golden::Tolerance::RASTERISER`, whose
/// `max_channel_delta` is the largest per-channel difference two rasterisers are
/// allowed to have on the *same* picture. This threshold is several times that,
/// because the question it answers is "did anything draw here", not "is this the
/// right colour" — the golden is what answers the second one.
const PAINTED_DELTA: u8 = 8;

/// What channel order [`OffscreenSetup::draw_and_readback`]'s bytes are in.
///
/// The same three lines as `crcbl-cli`'s `screenshot::channel_order` and
/// `vk_e2e/mesh.rs`'s, and not shared with either: `crcbl-golden` is a
/// **dev**-dependency here precisely so `png` reaches no shipped binary, so
/// `crcbl::screenshot` cannot name [`ChannelOrder`] to hand one out.
fn channel_order(format: Format) -> ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    }
}

/// Whether two pixels differ by more than driver noise, in RGB.
///
/// Alpha is ignored: the readback's alpha is the attachment's rather than
/// anything the scene chose, and both sprite and UI content is opaque by the
/// time it reaches the target.
fn differ(pixel: [u8; 4], other: [u8; 4]) -> bool {
    pixel[..3]
        .iter()
        .zip(&other[..3])
        .any(|(left, right)| left.abs_diff(*right) >= PAINTED_DELTA)
}

/// A lit cube and a pyramid, drawn by the engine's own renderer on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`draw_scene_and_match_its_golden`] for what each assertion group is for.
/// This scene's anti-vacuity claim is [`the_cube_is_lit_against_an_unpainted_corner`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_through_the_forward_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Cube,
        "cube",
        MIN_COLORS_CUBE,
        the_cube_is_lit_against_an_unpainted_corner,
    );
}

/// Four sprites over three batches and two sheets — `sprite.slang` — on the
/// backend `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// This is the scene `SV_InstanceID` broke: the third batch starts at instance
/// 3, and a backend that reads the wrong lowering draws the last rectangle in
/// the first's place. [`every_sprite_slot_is_painted_and_the_gaps_are_not`] is
/// what makes that a failure here rather than only in the golden's numbers.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_through_the_sprite_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Sprite,
        "sprite",
        MIN_COLORS_SPRITE,
        every_sprite_slot_is_painted_and_the_gaps_are_not,
    );
}

/// Rectangles, an outline and glyph-atlas text — `ui.slang` — on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds`] for
/// this scene's anti-vacuity claim, which is a blend rather than a lit centre:
/// the UI pass loads its target rather than clearing it, so "it composited"
/// is the thing worth asserting.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_through_the_ui_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ui,
        "ui",
        MIN_COLORS_UI,
        the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds,
    );
}

/// Draws one frame of `scene` and compares it against `tests/golden/{golden}.png`.
///
/// **What the assertions are for, in the order a failure would hit them:**
///
/// 1. The backend that opened is the one that was asked for. Every backend
///    draws these scenes identically by construction, so a Metal run that fell
///    back to wgpu would produce a passing frame and prove nothing about Metal.
/// 2. The adapter it opened is the class
///    [`ADAPTER_ENV_VAR`](crcbl::adapter::ADAPTER_ENV_VAR) named, and both the
///    adapter and the pin are printed. The same argument one layer down: this
///    frame died on `windows-latest` with `DXGI_ERROR_DEVICE_REMOVED` because
///    the first enumerated adapter is not a usable device there.
/// 3. The device's [`GeometryPath`](crcbl::hal::GeometryPath) is reported, and
///    the frame is drawn through whichever indirect tail it selects. Metal
///    reports no `DRAW_INDIRECT_COUNT` — the flag is absent from the API rather
///    than unimplemented — so it selects
///    [`IndirectPerBatch`](crcbl::hal::GeometryPath::IndirectPerBatch), the arm
///    that until now had only ever run on Vulkan behind a forced selector.
/// 4. Something drew: at least `min_colors` colours, plus `inspect`'s
///    scene-specific claim about *where* it drew. A full-screen quad and a blank
///    frame both fail these and neither is distinguishable by a tolerance.
/// 5. The picture is the one that was reviewed.
///
/// Both halves of 4 are per scene, because the scenes are not equally colourful
/// and they are not the same shape: the cube has a lit centre against a dark
/// clear, the sprite scene has four rectangles with gaps between them, and the
/// UI frame is a composite over a background it never clears.
fn draw_scene_and_match_its_golden(
    scene: Scene,
    golden: &str,
    min_colors: usize,
    inspect: fn(&Image),
) {
    // **Install a logger before opening anything.** Without one, every
    // `log::info!` a backend emits on the way to a device — the adapter it
    // chose, the surface it built, whether a validation layer loaded — goes
    // nowhere. Two sessions were spent diagnosing a D3D12 failure inside
    // `open` with no backend output at all in the run, for exactly this
    // reason: the panic message was the only evidence, and it named the call
    // that noticed rather than the one that caused it.
    crcbl_core::log::init_logging();

    // `unwrap_or_else` rather than `expect`, which would format the error with
    // `Debug` and escape the newlines out of the adapter listing a pin miss
    // carries — on a runner nobody can log into, that listing is the whole
    // diagnosis.
    let mut setup = OffscreenSetup::open(EXTENT.0, EXTENT.1, scene)
        .unwrap_or_else(|why| panic!("a GPU backend opens for the {golden} scene: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    // Printed unconditionally, and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "crcbl render e2e: {backend} selected {:?} / {:?} / {:?} for the {golden} scene",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );

    // The adapter line, and the raw pin beside it. `run-render-e2e.sh` matches
    // the pin back against what it exported, because the one failure this test
    // cannot see is the variable never reaching this process: the pin would be
    // `None` here, `select` would take the first adapter, and every assertion
    // below would pass on a device nobody asked for.
    let adapter = setup.adapter();
    let requested_adapter = crcbl::adapter::pin();
    eprintln!(
        "crcbl render e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = requested_adapter.as_deref().unwrap_or("<unset>"),
    );

    // A pin the loader ignored is the failure this catches, and it is the same
    // class as a suite that runs no tests. Both names go through the mappings
    // that already exist rather than a third table.
    if let Ok(requested) = std::env::var(BACKEND_ENV_VAR) {
        let opened = GpuBackend::from_name(&backend.to_string())
            .expect("every backend the registry can open has a GpuBackend spelling");
        assert_eq!(
            Some(opened),
            GpuBackend::from_name(&requested),
            "{BACKEND_ENV_VAR}={requested} was asked for and {backend} drew the frame"
        );
    }
    // Same shape one layer down: `select` refuses a class it cannot find, so
    // this can only fire if it resolved to something else — but it is the
    // assertion that makes the pin's arrival observable rather than assumed.
    if let Some(requested) = requested_adapter.as_deref() {
        let want = device_type_from_name(requested)
            .unwrap_or_else(|| panic!("{ADAPTER_ENV_VAR}={requested} is not a device class"));
        assert_eq!(
            adapter.device_type, want,
            "{ADAPTER_ENV_VAR}={requested} was asked for and adapter {} ({:?}) drew the frame",
            adapter.name, adapter.device_type
        );
    }

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else — so a run that panicked
    // on the pixels first would report a wrong picture where the real answer is
    // that the GPU never finished drawing it.
    setup.finish().expect("the device reaches idle");

    assert_eq!(
        (width, height),
        EXTENT,
        "the swapchain handed back an extent the golden was not blessed at"
    );
    let image = Image::from_readback(width, height, &pixels, channel_order(format))
        .expect("the readback is exactly one image");

    let colors = image.distinct_colors(min_colors);
    assert!(
        colors >= min_colors,
        "a {golden} frame with {colors} distinct colour(s) (counted to {min_colors}) is not \
         evidence — nothing drew, or only the clear did"
    );
    inspect(&image);

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{golden}.png"));
    let golden_image = Golden::new(reference);
    let comparison = golden_image
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    eprintln!(
        "crcbl render e2e: golden {golden} on {backend} — {}",
        comparison.summary()
    );
}

/// [`Scene::Cube`]'s anti-vacuity claim: a corner still holding the clear, and a
/// centre that is not it.
///
/// The bounds are absolute rather than relative to the corner because this
/// scene's clear is near-black and its cube is lit — "dark here, bright there"
/// is the claim, and it is the one a full-screen quad and a blank frame both
/// fail.
fn the_cube_is_lit_against_an_unpainted_corner(image: &Image) {
    let corner = image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = image.pixel(EXTENT.0 / 2, EXTENT.1 / 2).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );
}

/// Where a point in [`Scene::Sprite`]'s world lands in this frame.
///
/// That scene is drawn with an orthographic camera down −Z with Y up and a fixed
/// half height in world units, so the half width is that times the frame's
/// aspect — `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT` and `sprite_camera`,
/// both private to that module. This is therefore the one place the projection
/// is written down twice; a change to it moves the picture, and re-blessing the
/// golden is the step that will notice.
fn sprite_pixel(world_x: f32, world_y: f32) -> (u32, u32) {
    /// `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT`.
    const HALF_HEIGHT: f32 = 100.0;

    let half_width = HALF_HEIGHT * EXTENT.0 as f32 / EXTENT.1 as f32;
    let x = (world_x / half_width + 1.0) / 2.0 * EXTENT.0 as f32;
    let y = (1.0 - world_y / HALF_HEIGHT) / 2.0 * EXTENT.1 as f32;
    (x as u32, y as u32)
}

/// [`Scene::Sprite`]'s anti-vacuity claim: all four rectangles drew, and the
/// background between them did not.
///
/// The clear is read off the frame's corner rather than written down here — the
/// sprite pass loads its target instead of clearing it, so the corner is the
/// scene's clear by construction and comparing against it needs no assumption
/// about sRGB encoding or channel order.
///
/// The four samples are a quarter of a rectangle in from each centre rather than
/// on it: every sprite is a 2x2 or 4x2 texel sheet, so the exact centre lands on
/// a texel boundary and which of the four colours it resolves to is the
/// sampler's business. A quarter in is squarely inside one texel whichever way
/// V runs.
///
/// **This is the `SV_InstanceID` detector.** Three batches over two sheets, the
/// third starting at instance 3: a backend that reads the wrong lowering draws
/// the last rectangle over the first and leaves its own slot at the clear, which
/// is a slot that fails here by name rather than a number in a summary line.
fn every_sprite_slot_is_painted_and_the_gaps_are_not(image: &Image) {
    /// The world-space centres of `crcbl::screenshot`'s `SPRITE_RECTS`: four
    /// 40x40 rectangles on `y = 0`, ten units apart so no two of them touch.
    const SLOT_CENTRES: [f32; 4] = [-75.0, -25.0, 25.0, 75.0];
    /// Two of the three gaps between those rectangles, in world x. A quarter of
    /// the frame apart, and neither is the centre of a rectangle under any
    /// rotation of the batch order.
    const GAPS: [f32; 2] = [-50.0, 0.0];
    /// A quarter of a rectangle, in world units — see the sampling note above.
    const QUARTER: f32 = 10.0;

    let clear = image.pixel(1, 1).expect("inside");

    for (slot, centre) in SLOT_CENTRES.iter().enumerate() {
        let (x, y) = sprite_pixel(centre - QUARTER, QUARTER);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            differ(pixel, clear),
            "sprite slot {slot} at ({x}, {y}) is still the clear {clear:?} (got {pixel:?}) — \
             that batch did not draw, or it drew somewhere else"
        );
    }

    for gap in GAPS {
        let (x, y) = sprite_pixel(gap, 0.0);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            !differ(pixel, clear),
            "the gap between two sprites at ({x}, {y}) is painted {pixel:?} against a clear of \
             {clear:?} — the sprites are not rectangles where the scene puts them"
        );
    }
}

/// Where a fraction of the frame lands in pixels.
///
/// `crcbl::screenshot`'s `ui_draw_list` lays [`Scene::Ui`] out as fractions of
/// the extent, in the pass's Y-down screen pixels, so that the same picture
/// arrives at every `--size`. The samples below are written in the same
/// fractions its rectangles are.
fn ui_pixel(fraction_x: f32, fraction_y: f32) -> (u32, u32) {
    (
        (fraction_x * EXTENT.0 as f32) as u32,
        (fraction_y * EXTENT.1 as f32) as u32,
    )
}

/// [`Scene::Ui`]'s anti-vacuity claim: the panel drew, and the translucent bar
/// blended rather than replaced.
///
/// There is no lit centre to assert here and no useful "the middle is not the
/// clear" — the UI pass **loads** its target rather than clearing it, so what is
/// worth proving is that it composited onto what was already there. The bar
/// straddles the panel's bottom edge, so it covers two different backgrounds:
/// blending gives two different results and replacing gives one. That is a claim
/// a blank frame fails, a frame that lost the panel fails, and a frame whose
/// blend state came up as `Replace` also fails.
///
/// The three samples avoid the two lines of text by construction: both start at
/// 12% of the width, and the panel sample is far to the right of where the
/// longest of them can reach while the bar samples sit below the second line.
fn the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds(image: &Image) {
    let clear = image.pixel(1, 1).expect("inside");

    // Inside the panel (8%-92% across, 10%-62% down), outside the bar and
    // clear of the outline's band at the frame's edge.
    let (x, y) = ui_pixel(0.88, 0.25);
    let panel = image.pixel(x, y).expect("inside");
    assert!(
        differ(panel, clear),
        "the UI panel at ({x}, {y}) is still the clear {clear:?} (got {panel:?}) — nothing \
         filled it"
    );

    // The bar (30%-70% across, 45%-85% down) over the panel, then over the
    // clear below the panel's bottom edge.
    let (x, y) = ui_pixel(0.50, 0.50);
    let over_panel = image.pixel(x, y).expect("inside");
    let (x, y) = ui_pixel(0.62, 0.80);
    let over_clear = image.pixel(x, y).expect("inside");
    assert!(
        differ(over_panel, clear) && differ(over_clear, clear),
        "the translucent bar drew nothing: over the panel {over_panel:?}, over the clear \
         {over_clear:?}, against a clear of {clear:?}"
    );
    assert!(
        differ(over_panel, over_clear),
        "the bar is {over_panel:?} over the panel and {over_clear:?} over the clear — the same \
         colour on two backgrounds means the pass replaced rather than blended"
    );
    assert!(
        differ(over_panel, panel),
        "the bar over the panel is {over_panel:?} and the bare panel is {panel:?} — the bar did \
         not reach the panel at all"
    );
}
