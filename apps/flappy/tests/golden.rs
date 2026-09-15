//! The course flappy actually presented, off a real device, against a
//! checked-in golden — and three claims about the picture in front of it.
//!
//! # This is the flag's test as much as the frame's
//!
//! `apps/breakout/tests/golden.rs` is the pattern and its module docs carry the
//! full argument; the short version is that this suite runs the **compiled
//! binary** with `--screenshot` and everything it asserts is about the file that
//! binary left behind. The thing under test is the frame a player would have
//! seen — sky, course and title menu, every pass the game hung off the
//! swapchain image — not a scene a test rebuilt to look like it.
//!
//! It is also why there is no second render here and no in-process device: the
//! suite owns no GPU at all, and a failure in it is a failure of the sample.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two blank frames compare perfectly, and so do two uniformly dark ones
//! against a uniformly dark reference. So the golden is the *last* assertion and
//! the ones before it are ratios between blocks of pixels, which say **where**
//! the frame is bright, dark and coloured rather than what any one pixel is.
//!
//! Flappy's are picked so that each pass has one that fails without it. The sky
//! and the ground band are the pair that matters most: the sky is blue over red
//! and the ground is green over blue, so a readback whose channels were written
//! the wrong way round fails the first, and a sprite pass that never reached the
//! device fails the second — the ground point then reads the `sky` pass's
//! backdrop, whose blue beats its green.
//!
//! # The invocation is `crcbl-sample-test`'s
//!
//! [`SampleRun`] runs the binary, checks its summary and hands back the frame,
//! because four other samples' suites want the same thing — see that crate.
//! What stays here is what is about *this frame*: the claims below, and the
//! constants they are measured against.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` and breakout's `golden-e2e` use. A plain
//! `cargo test --workspace --all-features` on a machine with no GPU must stay
//! green, and `tests/run-flappy-golden.sh` is the only thing that turns both
//! off — and it fails when the suite reports zero tests run.
//!
//! # No darkening test here
//!
//! `apps/breakout/tests/golden.rs` carries
//! `a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`,
//! which pins that a uniform multiply is refused. That is a property of
//! `crcbl_golden::Tolerance::RASTERISER`, which this suite compares under
//! unchanged, so a copy here would be a second thing to keep in step and would
//! prove nothing about flappy.

#![cfg(feature = "golden-e2e")]

use crcbl_golden::Image;
use crcbl_sample_test::{Block, SampleRun, required_backend};

/// How many frames the run presents before the one that gets written.
///
/// The budget `.github/workflows/ci.yml`'s **Run flappy headless against
/// lavapipe** step already gives this binary, so the golden is a picture of a
/// run that workflow was making anyway rather than a second frame index to keep
/// track of. One second of the default 60 Hz simulation: far past start-up —
/// the atlas has uploaded, the first tick has run and the menu has been laid
/// out — and far past the offscreen ring's frames-in-flight, so the image
/// written has been round the ring several times. No input arrives in a
/// headless run, so the bird has not flapped and the run is still
/// `WaitingToStart`, which is the state the summary line names.
const FRAMES: u32 = 60;

/// The extent the checked-in golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless
/// run's offscreen ring renders at when `--size` says nothing — so the golden
/// is the frame the default invocation produces rather than one a flag had to
/// ask for.
const EXTENT: (u32, u32) = (960, 720);

/// How many distinct colours a frame of this course has to have.
///
/// A three-layer sky, hills, a ground band, pipes, a panel, three buttons, the
/// title and two lines of HUD text: a frame with fewer than this drew the clear
/// colour and very little else. Counted rather than guessed at — radv draws 147
/// and `Image::distinct_colors` stops counting at the bound it is given.
const MIN_COLORS: usize = 96;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, for the reason [`Block`] gives.
const BLOCK: (u32, u32) = (6, 6);

/// Open sky, left of the title panel and above the hills.
const SKY_AT: (u32, u32) = (40, 100);

/// The ground band along the bottom of the course, below the panel.
const GROUND_AT: (u32, u32) = (480, 700);

/// The middle of the `FLY` button's panel, below its text.
const BUTTON_AT: (u32, u32) = (480, 300);

/// The title panel's own backdrop, between the title text and the first button.
///
/// Dark by design — flappy's menu is a dark card over a bright sky, which is the
/// opposite of breakout's bright panel over a dark field, so the ratio below
/// runs the other way.
const PANEL_AT: (u32, u32) = (480, 200);

/// How much brighter the `FLY` button must be than the card it sits on.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the art does. Measured before it was fixed rather
/// than guessed: radv draws the button at 101.3/255 over a card of 24.3, which
/// is 4.2, so this leaves a factor of one and two thirds. Each claim prints what
/// it actually got, so the next person sizing it does not have to re-derive it.
const BUTTON_OVER_PANEL: f32 = 2.5;

/// How much bluer than red the open sky must be.
///
/// Half of the claim a channel-order mistake fails: a BGRA readback written as
/// RGBA turns the sky orange and leaves the brightness ratios happy — including
/// the structural half of the golden comparison, which is computed on luma and
/// barely moves for a swap. radv draws it at blue 141 / red 63, which is 2.2.
const SKY_BLUENESS: f32 = 1.6;

/// How much greener than blue the ground band must be.
///
/// The other half, and the one a lost sprite pass fails: with no ground drawn
/// the point reads the `sky` pass's backdrop, whose blue (141) beats its green
/// (105) and so cannot clear this. radv draws the band at green 119 / blue 50,
/// which is 2.4.
const GROUND_GREENNESS: f32 = 1.6;

/// The floor a block has to clear to have drawn anything at all.
///
/// Out of 255, and low because that is the question it asks: not "is this the
/// right shade" — the golden answers that — but "did a pass put anything here".
/// The darkest block any claim below reads is the title card's 24.3, so this
/// leaves a factor of four.
const DREW_AT_ALL: f32 = 6.0;

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(image: &Image) {
    let block = Block::new(image, BLOCK, "flappy");
    block.distinct_enough("a course", MIN_COLORS);

    // ---- 1. the title card is a menu on top of the course ------------------
    block.drew("title card", PANEL_AT, DREW_AT_ALL, "the menu drew nothing");
    block.over(
        ("FLY button", BUTTON_AT),
        ("card behind it", PANEL_AT),
        BUTTON_OVER_PANEL,
        "the button is not on top of the card",
    );

    // ---- 2. the open sky is blue, in that order ----------------------------
    block.channel_beats(
        "open sky",
        SKY_AT,
        &[("blue", 2), ("red", 0)],
        SKY_BLUENESS,
        Some(DREW_AT_ALL),
        "either the backdrop pass drew nothing, or the readback's channels were written the \
         wrong way round",
    );

    // ---- 3. the ground band is green, in that order ------------------------
    //
    // The claim a lost sprite pass fails: with no band drawn this point reads
    // the sky behind it, whose blue beats its green.
    block.channel_beats(
        "ground band",
        GROUND_AT,
        &[("green", 1), ("blue", 2)],
        GROUND_GREENNESS,
        Some(DREW_AT_ALL),
        "either the sprite pass reached nothing and this is the sky, or the readback's \
         channels were written the wrong way round",
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-flappy-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend("tests/run-flappy-golden.sh");
    let run = SampleRun {
        name: "flappy",
        binary: env!("CARGO_BIN_EXE_flappy"),
        tmp_dir: env!("CARGO_TARGET_TMPDIR"),
        file: "course.png",
        frames: FRAMES,
        extent: EXTENT,
        args: &[],
        stdout_contains: &["WaitingToStart"],
        simulation_advanced: true,
    };
    let (image, adapter) = run.screenshot(&backend);
    eprintln!("flappy golden: device on {adapter}");
    inspect(&image);
    run.compare_to_golden(&image, &backend, env!("CARGO_MANIFEST_DIR"), "course");
}
