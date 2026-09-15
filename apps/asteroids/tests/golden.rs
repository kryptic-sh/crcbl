//! The field asteroids actually presented, off a real device, against a
//! checked-in golden — and three claims about the picture in front of it.
//!
//! # This is the flag's test as much as the frame's
//!
//! `apps/breakout/tests/golden.rs` is the pattern and its module docs carry the
//! full argument; the short version is that this suite runs the **compiled
//! binary** with `--screenshot` and everything it asserts is about the file that
//! binary left behind. The thing under test is the frame a player would have
//! seen — the drifting rocks, the HUD band and the title menu, every pass the
//! game hung off the swapchain image — not a scene a test rebuilt to look like
//! it.
//!
//! It is also why there is no second render here and no in-process device: the
//! suite owns no GPU at all, and a failure in it is a failure of the sample.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two blank frames compare perfectly, and so do two uniformly dark ones against
//! a uniformly dark reference. So the golden is the *last* assertion and the
//! ones before it are ratios between blocks of pixels, which say **where** the
//! frame is bright, dark and coloured rather than what any one pixel is.
//!
//! # Where this one departs from breakout's shape, and why
//!
//! Breakout asserts that its playfield is *dark rather than absent* — a floor
//! under the darker half of its ratio, so that "bright thing over dark thing"
//! cannot be satisfied by a frame that lost the board entirely. **Asteroids has
//! nowhere to make that claim.** Its field is space: radv reads 2.3/255 there,
//! which is the clear colour and not a lit layer, so any floor it could clear
//! would also be cleared by a black frame.
//!
//! So the weight moves onto the bright half instead. [`ROCK_AT`] is an absolute
//! level as well as a ratio, and a sprite pass that never reached the device
//! drops it to the clear and fails the absolute claim first.
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
//! green, and `tests/run-asteroids-golden.sh` is the only thing that turns both
//! off — and it fails when the suite reports zero tests run.
//!
//! # No darkening test here
//!
//! `apps/breakout/tests/golden.rs` carries
//! `a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`,
//! which pins that a uniform multiply is refused. That is a property of
//! `crcbl_golden::Tolerance::RASTERISER`, which this suite compares under
//! unchanged, so a copy here would be a second thing to keep in step and would
//! prove nothing about asteroids.

#![cfg(feature = "golden-e2e")]

use crcbl_golden::Image;
use crcbl_sample_test::{Block, SampleRun, required_backend};

/// How many frames the run presents before the one that gets written.
///
/// The budget `.github/workflows/ci.yml`'s **Run asteroids headless against
/// lavapipe** step already gives this binary, so the golden is a picture of a
/// run that workflow was making anyway rather than a second frame index to keep
/// track of. One second of the default 60 Hz simulation: far past start-up — the
/// atlas has uploaded, the first tick has run and the menu has been laid out —
/// and far past the offscreen ring's frames-in-flight, so the image written has
/// been round the ring several times. No input arrives in a headless run, so
/// nothing has been fired and the run is still `WaitingToStart`, which is the
/// state the summary line names.
const FRAMES: u32 = 60;

/// The extent the checked-in golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless
/// run's offscreen ring renders at when `--size` says nothing — so the golden is
/// the frame the default invocation produces rather than one a flag had to ask
/// for.
const EXTENT: (u32, u32) = (960, 720);

/// How many distinct colours a frame of this field has to have.
///
/// A star field, a wave of shaded rocks, a panel, three buttons, the title and
/// two lines of HUD text: a frame with fewer than this drew the clear colour and
/// very little else. Counted rather than guessed at — radv draws 388 and
/// `Image::distinct_colors` stops counting at the bound it is given.
const MIN_COLORS: usize = 128;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, for the reason [`Block`] gives.
const BLOCK: (u32, u32) = (6, 6);

/// A flat interior of the rock drifting down the left margin, clear of the
/// title panel and of the rock's own shaded rim.
const ROCK_AT: (u32, u32) = (42, 440);

/// Open space in the same margin, well below that rock.
const SPACE_AT: (u32, u32) = (35, 600);

/// The middle of the `FLY` button's panel, below its text.
const BUTTON_AT: (u32, u32) = (480, 300);

/// The title panel's own backdrop, between the title text and the first button.
const PANEL_AT: (u32, u32) = (480, 200);

/// The level the rock has to reach on its own, out of 255.
///
/// **The claim that carries this suite's "the sprite pass ran".** Space around
/// it is the clear colour, so the ratio below would be satisfied by any speck;
/// this is what a frame that drew no rock fails. Measured before it was fixed
/// rather than guessed: radv draws the rock's flat interior at 46.3/255, so this
/// leaves a factor of nearly four. Each claim prints what it actually got, so
/// the next person sizing it does not have to re-derive it.
const ROCK_DREW: f32 = 12.0;

/// How much brighter the rock must be than the space beside it.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the art does. radv reads 46.3 against 2.3, which
/// is 20 — this bound is deliberately far below that, because what it is for is
/// the case where the rock is drawn *into* a field that is no longer empty.
const ROCK_OVER_SPACE: f32 = 6.0;

/// How much brighter the `FLY` button must be than the card it sits on.
///
/// radv draws the button at 101.3/255 over a card of 24.3, which is 4.2, so this
/// leaves a factor of one and two thirds.
const BUTTON_OVER_PANEL: f32 = 2.5;

/// How much bluer than red the `FLY` button's panel must be.
///
/// The claim a channel-order mistake fails, and close to the only one that can:
/// this frame is otherwise greyscale, so a BGRA readback written as RGBA leaves
/// every other assertion here happy — including the structural half of the
/// golden comparison, which is computed on luma and barely moves for a swap.
/// radv draws it at blue 144 / red 77, which is 1.9.
const BUTTON_BLUENESS: f32 = 1.4;

/// The floor a block has to clear to have drawn anything at all.
///
/// Out of 255, and low because that is the question it asks: not "is this the
/// right shade" — the golden answers that — but "did a pass put anything here".
/// The darkest block it is applied to is the title card's 24.3, so this leaves a
/// factor of four. It is deliberately **not** applied to [`SPACE_AT`], which is
/// the clear colour and would fail it — see the module docs.
const DREW_AT_ALL: f32 = 6.0;

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(image: &Image) {
    let block = Block::new(image, BLOCK, "asteroids");
    block.distinct_enough("a field", MIN_COLORS);

    // ---- 1. a rock is drawn into the space beside it -----------------------
    block.drew(
        "rock",
        ROCK_AT,
        ROCK_DREW,
        "the sprite pass reached nothing and this is space",
    );
    block.over(
        ("rock", ROCK_AT),
        ("space beside it", SPACE_AT),
        ROCK_OVER_SPACE,
        "the rock is not on top of an empty field",
    );

    // ---- 2. the title card is a menu on top of the field -------------------
    block.drew("title card", PANEL_AT, DREW_AT_ALL, "the menu drew nothing");
    block.over(
        ("FLY button", BUTTON_AT),
        ("card behind it", PANEL_AT),
        BUTTON_OVER_PANEL,
        "the button is not on top of the card",
    );

    // ---- 3. the button's panel is blue, in that order ----------------------
    block.channel_beats(
        "FLY button",
        BUTTON_AT,
        &[("blue", 2), ("red", 0)],
        BUTTON_BLUENESS,
        None,
        "the readback's channels were written the wrong way round",
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-asteroids-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend("tests/run-asteroids-golden.sh");
    let run = SampleRun {
        name: "asteroids",
        binary: env!("CARGO_BIN_EXE_asteroids"),
        tmp_dir: env!("CARGO_TARGET_TMPDIR"),
        file: "field.png",
        frames: FRAMES,
        extent: EXTENT,
        args: &[],
        stdout_contains: &["WaitingToStart"],
        simulation_advanced: true,
    };
    let (image, adapter) = run.screenshot(&backend);
    eprintln!("asteroids golden: device on {adapter}");
    inspect(&image);
    run.compare_to_golden(&image, &backend, env!("CARGO_MANIFEST_DIR"), "field");
}
