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
//! # Why the debug overlay is off
//!
//! It is on by default in a debug build and it draws frame times. A golden of a
//! frame with `0.007 ms` written on it is a golden that fails on the next
//! machine. `--no-debug-overlay` is part of the invocation for that reason and
//! `run-flappy-golden.sh` passes it.
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

use std::path::PathBuf;
use std::process::Command;

use crcbl_golden::{Golden, Image};

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
/// A block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the picture: a glyph edge or a nine-slice seam
/// landing a pixel either way moves it.
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

/// Which backend drew, from the environment.
///
/// **Required, with no default.** Every backend draws this course identically by
/// construction, so a run that fell back to another one produces a frame that
/// passes and proves nothing about the one that was wanted —
/// `crcbl::backend::open` would otherwise answer the question for you. The same
/// argument `tests/run-flappy-golden.sh` makes, made where it can actually be
/// enforced.
fn required_backend() -> String {
    std::env::var(crcbl::backend::BACKEND_ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{} is not set, so nothing would pin the backend and a fallback would pass. \
             Run tests/run-flappy-golden.sh, which names one.",
            crcbl::backend::BACKEND_ENV_VAR
        )
    })
}

/// Runs the real binary with `--screenshot` and hands back the file it wrote.
///
/// The output path is under `CARGO_TARGET_TMPDIR`, which cargo gives an
/// integration test for exactly this and which is already inside the `/target`
/// ignore — so there is no new ignore rule and a reviewer has a path to open.
///
/// **The stale file is removed first**, and its absence is what makes the
/// assertion below mean anything: a `--screenshot` that quietly did nothing
/// would otherwise pass on the previous run's picture forever.
fn screenshot_from_a_real_run(backend: &str) -> (Image, String) {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("course.png");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("could not clear {}: {error}", path.display()),
    }

    let output = Command::new(env!("CARGO_BIN_EXE_flappy"))
        .args([
            "--backend",
            backend,
            "--frames",
            &FRAMES.to_string(),
            // Not because a headless run needs saying — `--screenshot` turns it
            // on — but because saying it is how this suite records that the
            // picture is of the offscreen ring and not of a window.
            "--headless",
            "--no-debug-overlay",
            "--screenshot",
        ])
        .arg(&path)
        .output()
        .expect("the flappy binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "flappy exited {:?} on {backend}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    // The run has to have played the game, not merely started and stopped: a
    // frame written by a run that never reached the simulation is a picture of
    // start-up, and the summary is the only thing that can tell the two apart
    // from out here.
    assert!(
        stdout.contains(&format!("{FRAMES} frames")),
        "the summary does not say the run presented {FRAMES} frames:\n{stdout}"
    );
    assert!(
        stdout.contains("WaitingToStart"),
        "the run left the title screen, so this is not the frame the golden was \
         blessed on:\n{stdout}"
    );

    assert!(
        path.exists(),
        "flappy exited 0 and wrote no {} — `--screenshot` did nothing",
        path.display()
    );
    let image = Image::load_png(&path).expect("the screenshot is a readable PNG");
    assert_eq!(
        (image.width(), image.height()),
        EXTENT,
        "the binary wrote a {}x{} frame, which is not the extent the golden was blessed at",
        image.width(),
        image.height()
    );
    (image, adapter_line(&stderr))
}

/// The adapter the binary opened, read out of its own log.
///
/// From the run rather than from the environment this test exported: a variable
/// that never reached the process and a pin that was honoured look identical
/// from outside. `tests/run-flappy-golden.sh` reads this line back out of the
/// suite for the same reason.
fn adapter_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains(" adapter \""))
        .map(|line| line[line.find("hal: ").map_or(0, |at| at + "hal: ".len())..].to_string())
        .unwrap_or_else(|| panic!("the run never said which adapter it opened:\n{stderr}"))
}

/// The mean brightness of a [`BLOCK`]-sized block around `centre`, out of 255.
fn brightness(image: &Image, centre: (u32, u32)) -> f32 {
    channel_mean(image, centre, None)
}

/// The mean of one channel over the same block, out of 255.
fn channel(image: &Image, centre: (u32, u32), index: usize) -> f32 {
    channel_mean(image, centre, Some(index))
}

/// `index` names a channel, or `None` averages the three colour channels.
fn channel_mean(image: &Image, centre: (u32, u32), index: Option<usize>) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in centre.1.saturating_sub(BLOCK.1)..=(centre.1 + BLOCK.1).min(image.height() - 1) {
        for x in centre.0.saturating_sub(BLOCK.0)..=(centre.0 + BLOCK.0).min(image.width() - 1) {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += match index {
                Some(index) => f32::from(pixel[index]),
                None => (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0,
            };
            count += 1;
        }
    }
    total / count as f32
}

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(image: &Image) {
    let colors = image.distinct_colors(MIN_COLORS);
    assert!(
        colors >= MIN_COLORS,
        "a course with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not \
         evidence — nothing drew, or only the clear did"
    );

    // ---- 1. the title card is a menu on top of the course ------------------
    let button = brightness(image, BUTTON_AT);
    let panel = brightness(image, PANEL_AT);
    eprintln!("flappy golden: FLY button {button:.1}/255, title card {panel:.1}/255");
    assert!(
        panel > DREW_AT_ALL,
        "the title card is at {panel:.1}/255, so the menu pass drew nothing"
    );
    assert!(
        button > panel * BUTTON_OVER_PANEL,
        "the FLY button is {button:.1} and the card behind it is {panel:.1} — the button is \
         not on top of the card, or the whole frame has been flattened"
    );

    // ---- 2. the open sky is blue, in that order ----------------------------
    let sky_blue = channel(image, SKY_AT, 2);
    let sky_red = channel(image, SKY_AT, 0);
    eprintln!("flappy golden: sky blue {sky_blue:.1}, red {sky_red:.1}");
    assert!(
        sky_blue > DREW_AT_ALL && sky_blue > sky_red * SKY_BLUENESS,
        "the open sky reads blue {sky_blue:.1} / red {sky_red:.1} — either the backdrop pass \
         drew nothing, or the readback's channels were written the wrong way round"
    );

    // ---- 3. the ground band is green, in that order ------------------------
    //
    // The claim a lost sprite pass fails: with no band drawn this point reads
    // the sky behind it, whose blue beats its green.
    let ground_green = channel(image, GROUND_AT, 1);
    let ground_blue = channel(image, GROUND_AT, 2);
    eprintln!("flappy golden: ground green {ground_green:.1}, blue {ground_blue:.1}");
    assert!(
        ground_green > DREW_AT_ALL && ground_green > ground_blue * GROUND_GREENNESS,
        "the ground band reads green {ground_green:.1} / blue {ground_blue:.1} — either the \
         sprite pass reached nothing and this is the sky, or the readback's channels were \
         written the wrong way round"
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-flappy-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend();
    let (image, adapter) = screenshot_from_a_real_run(&backend);
    eprintln!("flappy golden: device on {adapter}");
    inspect(&image);

    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/course.png");
    let comparison = Golden::new(reference)
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("on {backend}: {message}"));
    eprintln!(
        "flappy golden: course on {backend} — {}",
        comparison.summary()
    );
}
