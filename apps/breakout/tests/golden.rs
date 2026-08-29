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
//! # Why the debug overlay is off
//!
//! It is on by default in a debug build, and it draws frame times. A golden of
//! a frame with `0.007 ms` written on it is a golden that fails on the next
//! machine. `--no-debug-overlay` is part of the invocation for that reason and
//! `run-breakout-golden.sh` passes it.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` and lantern's `golden-e2e` use. A plain
//! `cargo test --workspace --all-features` on a machine with no GPU must stay
//! green, and `tests/run-breakout-golden.sh` is the only thing that turns both
//! off — and it fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use std::path::PathBuf;
use std::process::Command;

use crcbl_golden::{Golden, Image, Tolerance, compare};

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
/// A block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the picture: a glyph edge or a nine-slice seam
/// landing a pixel either way moves it.
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

/// Which backend drew, from the environment.
///
/// **Required, with no default.** Every backend draws this board identically by
/// construction, so a run that fell back to another one produces a frame that
/// passes and proves nothing about the one that was wanted —
/// `crcbl::backend::open` would otherwise answer the question for you. The same
/// argument `tests/run-breakout-golden.sh` makes, made where it can actually be
/// enforced.
fn required_backend() -> String {
    std::env::var(crcbl::backend::BACKEND_ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{} is not set, so nothing would pin the backend and a fallback would pass. \
             Run tests/run-breakout-golden.sh, which names one.",
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("board.png");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("could not clear {}: {error}", path.display()),
    }

    let output = Command::new(env!("CARGO_BIN_EXE_breakout"))
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
        .expect("the breakout binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "breakout exited {:?} on {backend}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    // The binary's log, re-emitted whether or not it passed. `.output()` keeps
    // the child's stderr, and until now a green run showed none of it — so a
    // `vk validation:` line the layer wrote there reached nothing.
    // `tests/run-breakout-golden.sh` reads this suite's log for exactly that line
    // and for the messenger's own announcement, and both live in the child's
    // stderr, not this process's.
    eprint!("{stderr}");
    // The run has to have played the game, not merely started and stopped:
    // `tests/headless.rs` pins what this line says, and a frame written by a
    // run that never reached the simulation is a picture of start-up.
    assert!(
        stdout.contains(&format!("{FRAMES} frames")),
        "the summary does not say the run presented {FRAMES} frames:\n{stdout}"
    );
    // **And that the simulation advanced, which the frames do not say.** The
    // picture this guards is of a menu over a still field, so a build whose
    // `Game::tick` did nothing presented its frames, wrote a byte-identical
    // image and passed every claim below. Measured by emptying `tick`.
    //
    // It has to be the *simulated* count, not the loop's: the loop counts the
    // times it called `tick` and reads the same either way, while `sim_ticks`
    // comes from `Game::ticks_run` and goes to zero. flappy's golden made
    // exactly that mistake first and passed the frozen build twice.
    //
    // Half of `FRAMES` rather than the exact figure, because the exact one is
    // the accumulator's business. Zero is the case that matters.
    let simulated: u32 = stdout
        .split_once(" simulated)")
        .and_then(|(before, _)| before.rsplit('(').next())
        .and_then(|word| word.parse().ok())
        .unwrap_or_else(|| panic!("the summary names no simulated tick count:\n{stdout}"));
    assert!(
        simulated >= FRAMES / 2,
        "the simulation advanced {simulated} times over {FRAMES} frames, so it was not \
         running and this image is of a game that never started:\n{stdout}"
    );

    assert!(
        path.exists(),
        "breakout exited 0 and wrote no {} — `--screenshot` did nothing",
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
/// from outside. `tests/run-breakout-golden.sh` reads this line back out of the
/// suite for the same reason.
fn adapter_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains(" adapter \""))
        .map(|line| line[line.find("hal: ").map_or(0, |at| at + "hal: ".len())..].to_string())
        .unwrap_or_else(|| panic!("the run never said which adapter it opened:\n{stderr}"))
}

/// The mean brightness of a `half`-sized block around `centre`, out of 255.
fn brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    channel_mean(image, centre, half, None)
}

/// The mean of one channel over the same block, out of 255.
fn channel(image: &Image, centre: (u32, u32), half: (u32, u32), index: usize) -> f32 {
    channel_mean(image, centre, half, Some(index))
}

/// `index` names a channel, or `None` averages the three colour channels.
fn channel_mean(image: &Image, centre: (u32, u32), half: (u32, u32), index: Option<usize>) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in centre.1.saturating_sub(half.1)..=(centre.1 + half.1).min(image.height() - 1) {
        for x in centre.0.saturating_sub(half.0)..=(centre.0 + half.0).min(image.width() - 1) {
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
        "a board with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not \
         evidence — nothing drew, or only the clear did"
    );

    // ---- 1. the menu panel is a bright thing on a dark field ---------------
    let menu = brightness(image, MENU_AT, BLOCK);
    let field = brightness(image, FIELD_AT, BLOCK);
    eprintln!("breakout golden: menu {menu:.1}/255, field {field:.1}/255");
    assert!(
        menu > DREW_AT_ALL,
        "the menu panel is at {menu:.1}/255, so the menu pass drew nothing"
    );
    assert!(
        menu > field * MENU_OVER_FIELD,
        "the menu panel is {menu:.1} and the field behind it is {field:.1} — the panel is \
         not on top of the board, or the whole frame has been flattened"
    );

    // ---- 2. the field is dark rather than absent ---------------------------
    //
    // The other half of claim 1, and the one that stops it being satisfied by a
    // frame that simply lost the board: `menu > field * ratio` holds for
    // `field == 0`, which is what a sprite pass that never ran looks like.
    assert!(
        field > DREW_AT_ALL,
        "the field is at {field:.1}/255, so the sprite pass reached nothing"
    );

    // ---- 3. the top-left brick is red, in that order ------------------------
    let red = channel(image, BRICK_AT, BLOCK, 0);
    let blue = channel(image, BRICK_AT, BLOCK, 2);
    eprintln!("breakout golden: brick red {red:.1}, blue {blue:.1}");
    assert!(
        red > DREW_AT_ALL && red > blue * BRICK_REDNESS,
        "the top-left brick reads red {red:.1} / blue {blue:.1} — either no brick drew there, \
         or the readback's channels were written the wrong way round"
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
            .chunks_exact(4)
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
    let backend = required_backend();
    let (image, adapter) = screenshot_from_a_real_run(&backend);
    eprintln!("breakout golden: device on {adapter}");
    inspect(&image);

    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/board.png");
    let comparison = Golden::new(reference)
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("on {backend}: {message}"));
    eprintln!(
        "breakout golden: board on {backend} — {}",
        comparison.summary()
    );
}
