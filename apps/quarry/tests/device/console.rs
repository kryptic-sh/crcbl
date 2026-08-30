//! `debug_view ambient occlusion`, in a sample that never had a row for it.
//!
//! `docs/plan/52-debug-console.md`'s slice-6 exit criterion, on this fixture
//! because it is the one with a device suite of its own. **quarry has no
//! ambient-occlusion control anywhere**: its pause panel has a `LOD VIEW` row
//! and a `HEATMAP` row and nothing else, its command line has `--lod-tint` and
//! `--heatmap` and nothing else, and until the console landed the occlusion view
//! was reachable in `apps/lantern` alone. Its render stack does run the pass —
//! [`RenderEffects::DEFAULT_STACK`](crcbl::render::RenderEffects::DEFAULT_STACK)
//! carries it — so there is a channel here to draw, and nothing that could draw
//! it.
//!
//! The two halves are separate on purpose, because they fail for different
//! reasons:
//!
//! * [`the_console_puts_the_occlusion_view_on_a_sample_that_never_had_a_row_for_it`]
//!   is the **wiring**, through the whole loop: a person opens the console with
//!   `` ` ``, types the line, and the renderer this bundle holds is drawing the
//!   occlusion channel on the next frame. It runs on whatever `CRCBL_GPU` names,
//!   `Null` included — the switch is state, not pixels.
//! * [`the_occlusion_view_draws_the_grey_channel_and_not_the_shaded_face`] is
//!   the **picture**, which needs a device: a view that reached the renderer and
//!   drew the shaded frame anyway would satisfy the first test completely.

use crcbl::backend::GpuBackend;
use crcbl::engine::{CONSOLE_KEY, ExitReason};
use crcbl::render::DebugView;
use crcbl::shell::HeadlessShell;
use crcbl_quarry::{Loop, Options, with_shell};

use crate::harness::{DEFAULT_BUDGET, EXTENT, Levels, Quarry, backend};

/// A loop over a concrete [`HeadlessShell`], so this file can play compositor.
///
/// The backend is [`backend`]'s — `Null` unless `CRCBL_GPU` names another —
/// rather than pinned to `Null` the way `crate::app`'s own checks are, because
/// the second test in this file needs pixels and both should be looking at the
/// same fixture.
fn scripted(frames: u64) -> Loop<HeadlessShell> {
    let mut options = Options::default();
    options.common.headless = true;
    options.common.frames = Some(frames);
    options.common.backend = Some(backend());
    with_shell(Box::new(HeadlessShell::new()), &options).expect("headless always starts")
}

/// Presses `key` and releases it, the way a finger does.
fn tap(engine: &mut Loop<HeadlessShell>, key: crcbl::core::input::KeyCode) {
    let window = engine.window();
    engine
        .shell_mut()
        .key_press(window, key)
        .expect("the window is live");
    engine
        .shell_mut()
        .key_release(window, key)
        .expect("the window is live");
}

/// **The console draws the occlusion channel in a sample that has no control
/// for it**, with no per-app code beyond the `GameGpu` forwarder.
///
/// End to end through the loop: the key that opens the panel, the characters a
/// keyboard layout commits, `Enter`, and then the renderer's own answer for what
/// it is drawing — [`ForwardRenderer::debug_view`], read back through
/// `Gpu::debug_view`, which resolves five independent switches by precedence and
/// so cannot be satisfied by a caller that set the wrong one.
///
/// The value has a **space in it**, which is the case a set that took its first
/// argument would lose, and it is the exact line the plan's exit criterion
/// names.
///
/// [`ForwardRenderer::debug_view`]: crcbl::render::ForwardRenderer::debug_view
#[test]
fn the_console_puts_the_occlusion_view_on_a_sample_that_never_had_a_row_for_it() {
    // The view is one process-global value — `crcbl::debug_view` — so this
    // check owns it for its duration and hands it back shaded.
    let _view = crcbl::debug_view::for_test();
    let mut engine = scripted(16);
    let window = engine.window();
    engine.frame().expect("a frame");
    assert_eq!(
        engine.gpu().debug_view(),
        DebugView::Shaded,
        "a run opens on the shaded picture, whatever a later line asks for"
    );

    tap(&mut engine, CONSOLE_KEY);
    engine.frame().expect("a frame");
    assert!(
        engine.console().is_open(),
        "the console key did not open the panel, so nothing below is being typed at it"
    );

    engine
        .shell_mut()
        .commit_text(window, "debug_view ambient occlusion")
        .expect("the window is live");
    tap(&mut engine, crcbl::core::input::KeyCode::Enter);
    engine.frame().expect("a frame");

    assert_eq!(
        engine.gpu().debug_view(),
        DebugView::AmbientOcclusion,
        "the console's line did not reach this sample's renderer"
    );

    // And the same console takes it back, which is the half a reviewer needs
    // more than the first one.
    engine
        .shell_mut()
        .commit_text(window, "debug_view shaded")
        .expect("the window is live");
    tap(&mut engine, crcbl::core::input::KeyCode::Enter);
    engine.frame().expect("a frame");
    assert_eq!(engine.gpu().debug_view(), DebugView::Shaded);

    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// One frame of the face from `at` along the dolly, with `view` in force.
///
/// The view is applied through [`crcbl::settings::set_debug_view_on`] — the
/// body `crcbl::impl_game_gpu!(Gpu, with_renderer)` forwards
/// `GameGpu::set_debug_view` to, and therefore the body the console's line
/// reaches on the test above's frame. A second spelling here would be a picture
/// of something the console does not do.
fn frame_with_at(view: DebugView, at: f32) -> crcbl_golden::Image {
    let mut quarry = Quarry::open(Levels::Dag, DEFAULT_BUDGET);
    crcbl::settings::set_debug_view_on(&mut quarry.renderer, view);
    // `frame_from` rather than `frame`, because that one requires a fifth of the
    // frame to differ from its most common colour and the occlusion view is
    // *deliberately* nearly uniform: an unoccluded surface reads white, and the
    // small share that does not is exactly what this file measures.
    let frame = quarry.frame_from(&crate::harness::dolly(at));
    quarry.finish();
    let (width, height) = EXTENT;
    crcbl_golden::Image::from_rgba8(width, height, frame.pixels_rgba)
        .expect("the readback is one RGBA8 frame of the ring's extent")
}

/// How many pixels of `image` are grey, and how many of those are darker than
/// the white the renderer binds when nothing computed a channel.
///
/// Grey is `r == g == b` on the encoded frame: the occlusion branch writes one
/// value into all three, and both the tonemap and the sRGB encode are per
/// channel, so a value that was equal in the HDR target is equal here.
fn greys(image: &crcbl_golden::Image) -> (usize, usize) {
    let mut grey = 0;
    let mut darkened = 0;
    for pixel in image.pixels().chunks_exact(4) {
        if pixel[0] == pixel[1] && pixel[1] == pixel[2] {
            grey += 1;
            if pixel[0] < OCCLUDED_CEILING {
                darkened += 1;
            }
        }
    }
    (grey, darkened)
}

/// How dark an encoded grey has to be to count as occluded rather than as the
/// white placeholder.
///
/// The channel arrives as `R8Unorm`, so the 1×1 image the renderer binds when no
/// pass ran is 255 and one step of the channel is 1. Five short of it, which is
/// far inside the darkest the pass actually reaches at [`OCCLUSION_POSE`] —
/// measured, see that constant.
const OCCLUDED_CEILING: u8 = 250;

/// Where along the dolly the occlusion is measured from.
///
/// **Swept, not chosen.** The channel over this face is shallow — it is a
/// displaced ridge, not a room with corners — so the pose decides whether there
/// is anything to see at all. Pixels darker than [`OCCLUDED_CEILING`], on radv
/// and on lavapipe: `0.0` → 45 and 45, `0.25` → 335 and 329, `0.5` → 422 and
/// 419, `0.75` → 559 and 557, `1.0` → **0 and 0**, the camera being inside the
/// quarry by then with nothing left to occlude anything. Three quarters down is
/// the deepest of them, and the darkest grey there is 220 on both drivers.
const OCCLUSION_POSE: f32 = 0.75;

/// **The occlusion view draws the grey channel, and the shaded frame does not.**
///
/// Three claims, because each of the three fails on its own:
///
/// * the view's frame is **grey nearly everywhere**, which the shaded frame is
///   not — a view that never reached the renderer draws the lit face and fails
///   here;
/// * some of that grey is **darker than the placeholder**, which says the frame
///   is showing a computed channel rather than the 1×1 white image the renderer
///   binds when no pass ran;
/// * the two frames **differ**, which is the assertion a pair of identical
///   constants would still pass the first two of.
///
/// Not a golden: nothing here is a reference image, and a debug view blessed per
/// backend would be a fifth branch of `mesh.slang` committed as pictures — the
/// argument `docs/backlog.md` already carries for the normals view.
#[test]
fn the_occlusion_view_draws_the_grey_channel_and_not_the_shaded_face() {
    if backend() == GpuBackend::Null {
        eprintln!(
            "quarry console: the Null backend draws nothing, so there are no pixels to compare; \
             run with CRCBL_GPU=vk"
        );
        return;
    }

    let shaded = frame_with_at(DebugView::Shaded, OCCLUSION_POSE);
    let occlusion = frame_with_at(DebugView::AmbientOcclusion, OCCLUSION_POSE);
    let pixels = (EXTENT.0 * EXTENT.1) as usize;
    let (shaded_grey, _) = greys(&shaded);
    let (occlusion_grey, occluded) = greys(&occlusion);
    let differing = shaded
        .pixels()
        .chunks_exact(4)
        .zip(occlusion.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    eprintln!(
        "quarry console: occlusion view — {occlusion_grey}/{pixels} grey against \
         {shaded_grey}/{pixels} shaded, {occluded} below {OCCLUDED_CEILING}, {differing} pixels \
         differ"
    );

    assert!(
        occlusion_grey * 100 >= pixels * GREY_SHARE_PERCENT,
        "the occlusion view is grey by construction and only {occlusion_grey} of {pixels} \
         pixels are"
    );
    assert!(
        shaded_grey * 100 < pixels * GREY_SHARE_PERCENT,
        "the shaded frame is {shaded_grey}/{pixels} grey, so this comparison says nothing"
    );
    assert!(
        occluded > 0,
        "every grey pixel is the white placeholder, so the view is not reading a channel"
    );
    assert!(
        differing * 100 >= pixels * DIFFERING_SHARE_PERCENT,
        "only {differing} of {pixels} pixels moved, which is not a different picture"
    );
}

/// What share of the frame the occlusion view has to be grey over, as a
/// percentage.
///
/// Measured on both drivers at [`OCCLUSION_POSE`]: the occlusion view reads
/// 47 104 of 49 152 pixels grey — everything but the sky above the ridge —
/// against **none at all** in the shaded frame. The bar sits between the two
/// with room for a driver that rounds an edge differently, and the test asserts
/// it from both sides, so a frame that came back grey for some other reason
/// would take the shaded half down with it.
const GREY_SHARE_PERCENT: usize = 90;

/// What share of the frame has to move between the two views.
///
/// Measured the same way: 47 360 of 49 152 pixels differ. Half is the bar
/// because the claim is "a different picture", not a count.
const DIFFERING_SHARE_PERCENT: usize = 50;
