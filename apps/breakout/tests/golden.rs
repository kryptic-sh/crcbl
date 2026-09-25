//! The board breakout actually presented, off a real device, against a
//! checked-in golden — and three claims about the picture in front of it.
//!
//! # This is the flag's test as much as the frame's
//!
//! `apps/lantern/tests/golden.rs` builds its scene in-process and renders it
//! through [`OffscreenSetup`](crcbl::screenshot::OffscreenSetup). This suite
//! does the opposite on purpose: it runs the **compiled binary** with
//! `--screenshot`, and everything it asserts is about the file that binary
//! left behind. That is the whole point of putting the capture in
//! `crcbl::args::Common` rather than in each sample — the thing under test is
//! the frame a player would have seen, including the menu, the HUD and every
//! pass the game hung off the swapchain image, not a scene a test rebuilt to
//! look like it.
//!
//! It is also why there is no second render here and no in-process device: the
//! suite owns no GPU at all, and a failure in it is a failure of the sample.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two blank frames compare perfectly, and so do two uniformly dark ones
//! against a uniformly dark reference. So the golden is the *last* assertion,
//! and the ones before it are the shape
//! `crates/crcbl/tests/render_e2e.rs` uses: a distinct-colour floor, then
//! ratios between blocks of pixels — bright menu against dark backdrop, a
//! saturated brick against both — which say **where** the frame is bright and
//! dark rather than what any one pixel is.
//!
//! # The invocation is `crcbl-sample-test`'s
//!
//! [`SampleRun`] runs the binary, checks its summary and hands back the frame,
//! because four other samples' suites want the same thing — see that crate.
//! What stays here is what is about *this board*: the claims below, and the
//! constants they are measured against.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` and lantern's `golden-e2e` use. A plain
//! `cargo test --workspace --all-features` on a machine with no GPU must stay
//! green, and `tests/run-breakout-golden.sh` is the only thing that turns both
//! off — and it fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use std::path::PathBuf;

use crcbl_golden::{Image, Tolerance, compare};
use crcbl_sample_test::{Block, SampleRun, required_backend};

/// How many frames the run presents before the one that gets written.
///
/// The same 24 `tests/headless.rs` uses, so the state this frame is a picture
/// of is the state that suite already pins by name: no input has arrived, the
/// ball has not launched, and the launch menu is up.
const FRAMES: u32 = 24;

/// The extent the checked-in golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless
/// run's offscreen ring renders at when `--size` says nothing — so the golden
/// is the frame the default invocation produces rather than one a flag had to
/// ask for.
const EXTENT: (u32, u32) = (960, 720);

/// How many distinct colours a frame of this board has to have.
///
/// A backdrop in several layers, forty bricks in four hues, a panel, three
/// buttons and two lines of HUD text: a frame with fewer than this drew the
/// clear colour and very little else. Counted rather than guessed at — see
/// `Image::distinct_colors`, which stops counting at the bound it is given.
const MIN_COLORS: usize = 64;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, for the reason [`Block`] gives.
const BLOCK: (u32, u32) = (6, 6);

/// The middle of the `PLAY` button's panel, below its text.
const MENU_AT: (u32, u32) = (480, 340);

/// The playfield backdrop beside the brick column, which nothing draws over.
///
/// Left of the panel and above the bottom band, so it is the sprite pass's
/// darkest *lit* layer rather than the unpainted margin — a point in the margin
/// would read near zero and the ratio below would say nothing.
const FIELD_AT: (u32, u32) = (60, 400);

/// The top brick of the left-hand column — the red one.
const BRICK_AT: (u32, u32) = (82, 125);

/// How much brighter the menu panel must be than the field behind it.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the art does. Measured before it was fixed rather
/// than guessed: radv draws the panel at 101.3/255 over a field of 14.0, which
/// is 7.2, so this leaves a factor of two and a half. Each claim prints what it
/// actually got, so the next person sizing it does not have to re-derive it.
const MENU_OVER_FIELD: f32 = 3.0;

/// How much redder than blue the top-left brick must be.
///
/// The claim a channel-order mistake fails, and close to the only one that
/// can: a BGRA readback written as RGBA turns this brick blue and leaves every
/// other assertion here happy — including the structural half of the golden
/// comparison, which is computed on luma and barely moves for a swap. radv
/// draws it at red 142 / blue 44, which is 3.2.
const BRICK_REDNESS: f32 = 2.0;

/// The multiply the reported defect amounted to.
///
/// The browser demos' transfer function came out uniformly dim, and this is the
/// factor it was reported at. Applied to the reference by
/// [`a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`],
/// which is what pins that the bound this suite compares under would have
/// caught it.
const DARKENED_BY: f32 = 0.61;

/// The floor a block has to clear to have drawn anything at all.
///
/// Out of 255, and low because that is the question it asks: not "is this the
/// right shade" — the golden answers that — but "did a pass put anything here".
/// The darkest block any claim below reads is the field's 14.0, so this leaves
/// a factor of two.
const DREW_AT_ALL: f32 = 6.0;

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(image: &Image) {
    let block = Block::new(image, BLOCK, "breakout");
    block.distinct_enough("a board", MIN_COLORS);

    // ---- 1. the menu panel is a bright thing on a dark field ---------------
    block.drew("menu panel", MENU_AT, DREW_AT_ALL, "the menu drew nothing");
    block.over(
        ("menu panel", MENU_AT),
        ("field behind it", FIELD_AT),
        MENU_OVER_FIELD,
        "the panel is not on top of the board",
    );

    // ---- 2. the field is dark rather than absent ---------------------------
    //
    // The other half of claim 1, and the one that stops it being satisfied by a
    // frame that simply lost the board: `menu > field * ratio` holds for
    // `field == 0`, which is what a sprite pass that never ran looks like.
    block.drew(
        "field",
        FIELD_AT,
        DREW_AT_ALL,
        "the sprite pass reached nothing",
    );

    // ---- 3. the top-left brick is red, in that order ------------------------
    block.channel_beats(
        "top-left brick",
        BRICK_AT,
        &[("red", 0), ("blue", 2)],
        BRICK_REDNESS,
        Some(DREW_AT_ALL),
        "either no brick drew there, or the readback's channels were written the wrong way \
         round",
    );
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

/// **The defect this whole suite exists for: output uniformly too dark.**
///
/// Every browser demo rendered its transfer function too dark for several
/// commits and every gate stayed green, because what the samples compared —
/// simulation tuples and state hashes — contains no pixel. This is the check
/// that the *golden's tolerance* would have refused it, which is a different
/// question from whether the golden exists: a bound loose enough to admit two
/// rasterisers could easily be loose enough to admit a uniform multiply, and
/// then the picture would be checked and the bug still ship.
///
/// [`DARKENED_BY`] is applied to the checked-in reference and the result is
/// offered to the same [`Tolerance::RASTERISER`] the golden uses. No GPU is
/// involved and none is needed — this asks about the comparator, not about a
/// device — so it is the one test here that is not `#[ignore]`d.
#[test]
fn a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses() {
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/board.png");
    let good = Image::load_png(&reference).expect("the reference is readable");

    let darkened = Image::from_rgba8(
        good.width(),
        good.height(),
        good.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| {
                let dim = |channel: u8| (f32::from(channel) * DARKENED_BY).round() as u8;
                [dim(pixel[0]), dim(pixel[1]), dim(pixel[2]), pixel[3]]
            })
            .collect(),
    )
    .expect("the same frame, dimmer");

    let comparison = compare(&good, &darkened, &Tolerance::RASTERISER);
    eprintln!(
        "breakout golden: a {DARKENED_BY} multiply reads {}",
        comparison.summary()
    );
    assert!(
        !comparison.is_match(),
        "a uniform {DARKENED_BY} multiply passed Tolerance::RASTERISER, so the golden would \
         have shipped the too-dark frame the way every other gate did: {}",
        comparison.summary()
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-breakout-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend("tests/run-breakout-golden.sh");
    let run = SampleRun {
        name: "breakout",
        binary: env!("CARGO_BIN_EXE_breakout"),
        tmp_dir: env!("CARGO_TARGET_TMPDIR"),
        file: "board.png",
        frames: FRAMES,
        extent: EXTENT,
        args: &[],
        stdout_contains: &[],
        simulation_advanced: true,
    };
    let (image, adapter) = run.screenshot(&backend);
    eprintln!("breakout golden: device on {adapter}");
    inspect(&image);
    run.compare_to_golden(&image, &backend, env!("CARGO_MANIFEST_DIR"), "board");
}
