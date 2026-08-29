//! The panels hud actually presented, off a real device, against a checked-in
//! golden — and four claims about the picture in front of it.
//!
//! # This is the flag's test as much as the frame's
//!
//! `apps/breakout/tests/golden.rs` is the pattern and its module docs carry the
//! full argument; the short version is that this suite runs the **compiled
//! binary** with `--screenshot` and everything it asserts is about the file that
//! binary left behind. The thing under test is the frame a player would have
//! seen, not a draw list a test rebuilt to look like it.
//!
//! It is also why there is no second render here and no in-process device: the
//! suite owns no GPU at all, and a failure in it is a failure of the sample.
//!
//! # The one sample whose frame is a clear plus `ui-composite`
//!
//! hud is exempt from sample rule 11 — no `.crpix` art, no sprite pass — so its
//! frame is a `backdrop` pass and the UI compositor and nothing else. That makes
//! it the only golden in the tree that would catch a UI-pass regression with no
//! sprite pass in front of it to have tripped first, and it is why the claims
//! below are about the widgets themselves: the health bar's fill, the mana
//! bar's, and the empty page they sit on.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two blank frames compare perfectly, and so do two uniformly dark ones against
//! a uniformly dark reference. So the golden is the *last* assertion and the
//! ones before it are ratios between blocks of pixels, which say **where** the
//! frame is bright, dark and coloured rather than what any one pixel is.
//!
//! The two bar fills are the pair that matters most: health is red over blue and
//! mana is blue over red, so a readback whose channels were written the wrong
//! way round fails both — and the structural half of the golden comparison,
//! computed on luma, barely moves for a swap.
//!
//! # Why the debug overlay is off
//!
//! It is on by default in a debug build and it draws frame times. A golden of a
//! frame with `0.007 ms` written on it is a golden that fails on the next
//! machine. `--no-debug-overlay` is part of the invocation for that reason and
//! `run-hud-golden.sh` passes it.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` and breakout's `golden-e2e` use. A plain
//! `cargo test --workspace --all-features` on a machine with no GPU must stay
//! green, and `tests/run-hud-golden.sh` is the only thing that turns both off —
//! and it fails when the suite reports zero tests run.
//!
//! # No darkening test here
//!
//! `apps/breakout/tests/golden.rs` carries
//! `a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`,
//! which pins that a uniform multiply is refused. That is a property of
//! `crcbl_golden::Tolerance::RASTERISER`, which this suite compares under
//! unchanged, so a copy here would be a second thing to keep in step and would
//! prove nothing about hud.

#![cfg(feature = "golden-e2e")]

use std::path::PathBuf;
use std::process::Command;

use crcbl_golden::{Golden, Image};

/// How many frames the run presents before the one that gets written.
///
/// The budget `.github/workflows/ci.yml`'s **Run hud headless against lavapipe**
/// step already gives this binary, so the golden is a picture of a run that
/// workflow was making anyway rather than a second frame index to keep track of.
/// One second of the default 60 Hz simulation: far past start-up — the atlas has
/// uploaded, the first tick has run and the page has been laid out — and far
/// past the offscreen ring's frames-in-flight, so the image written has been
/// round the ring several times. The ticker is seeded, so the floating damage
/// numbers and the ability cooldowns are in the same place every run.
const FRAMES: u32 = 60;

/// The extent the checked-in golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless
/// run's offscreen ring renders at when `--size` says nothing — so the golden is
/// the frame the default invocation produces rather than one a flag had to ask
/// for.
const EXTENT: (u32, u32) = (960, 720);

/// How many distinct colours a frame of this page has to have.
///
/// **Far lower than any other sample's, and that is what hud is.** There is no
/// sprite art here at all: a flat backdrop, two bars over their tracks, a stat
/// panel, the wave banner, four ability tiles and one text colour. Every one of
/// those is a solid fill, so the whole page is a page of a dozen-odd colours by
/// construction — a frame with fewer than this drew the clear and very little
/// else. Counted rather than guessed at — radv draws 17 and
/// `Image::distinct_colors` stops counting at the bound it is given.
const MIN_COLORS: usize = 12;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// **Smaller than breakout's**, because hud's subjects are bars a dozen pixels
/// tall and a mana fill a dozen wide at wave one: a six-pixel half straddles the
/// fill's right edge and drags the track's colour into the mean. It is still a
/// block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the picture.
///
/// A smaller block is not a looser comparison — the golden below is compared
/// under the same `crcbl_golden::Tolerance::RASTERISER` every other sample uses,
/// which is why no darkening test is repeated here.
const BLOCK: (u32, u32) = (4, 4);

/// Inside the health bar's fill, left of the `200 / 200` label.
const HEALTH_AT: (u32, u32) = (120, 60);

/// Inside the mana bar's fill, which at wave one is the leftmost sliver of its
/// track.
const MANA_AT: (u32, u32) = (42, 106);

/// The empty middle of the page, which no widget reaches.
const BACKDROP_AT: (u32, u32) = (600, 300);

/// How much brighter the health bar's fill must be than the page behind it.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the art does. Measured before it was fixed rather
/// than guessed: radv draws the fill at 170.7/255 over a backdrop of 72.3, which
/// is 2.4. Each claim prints what it actually got, so the next person sizing it
/// does not have to re-derive it.
const FILL_OVER_BACKDROP: f32 = 1.8;

/// How much redder than its other channels the health bar's fill must be.
///
/// Half of the claim a channel-order mistake fails: a BGRA readback written as
/// RGBA turns this bar cyan and leaves every brightness ratio here happy. radv
/// draws it at red 234 / green 134 / blue 144, so 1.7 against the nearer of the
/// two.
const HEALTH_REDNESS: f32 = 1.3;

/// How much bluer than red the mana bar's fill must be.
///
/// The other half, and pointing the opposite way, so the pair cannot both be
/// satisfied by a frame whose channels were rotated rather than swapped. radv
/// draws it at blue 246 / red 139, which is 1.8.
const MANA_BLUENESS: f32 = 1.3;

/// The floor a block has to clear to have drawn anything at all.
///
/// Out of 255, and low because that is the question it asks: not "is this the
/// right shade" — the golden answers that — but "did a pass put anything here".
/// The darkest block any claim below reads is the backdrop's 72.3, so this
/// leaves an order of magnitude.
const DREW_AT_ALL: f32 = 6.0;

/// Which backend drew, from the environment.
///
/// **Required, with no default.** Every backend draws this page identically by
/// construction, so a run that fell back to another one produces a frame that
/// passes and proves nothing about the one that was wanted —
/// `crcbl::backend::open` would otherwise answer the question for you. The same
/// argument `tests/run-hud-golden.sh` makes, made where it can actually be
/// enforced.
fn required_backend() -> String {
    std::env::var(crcbl::backend::BACKEND_ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{} is not set, so nothing would pin the backend and a fallback would pass. \
             Run tests/run-hud-golden.sh, which names one.",
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("panels.png");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("could not clear {}: {error}", path.display()),
    }

    let output = Command::new(env!("CARGO_BIN_EXE_hud"))
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
        .expect("the hud binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "hud exited {:?} on {backend}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    // The binary's log, re-emitted whether or not it passed. `.output()` keeps
    // the child's stderr, and until now a green run showed none of it — so a
    // `vk validation:` line the layer wrote there reached nothing.
    // `tests/run-hud-golden.sh` reads this suite's log for exactly that line
    // and for the messenger's own announcement, and both live in the child's
    // stderr, not this process's.
    eprint!("{stderr}");
    // The run has to have driven the ticker, not merely started and stopped: a
    // frame written by a run that never reached the simulation is a picture of
    // start-up, and the summary is the only thing that can tell the two apart
    // from out here.
    assert!(
        stdout.contains(&format!("{FRAMES} frames")),
        "the summary does not say the run presented {FRAMES} frames:\n{stdout}"
    );
    assert!(
        stdout.contains("wave 1"),
        "the ticker moved off wave one, so this is not the frame the golden was \
         blessed on:\n{stdout}"
    );

    assert!(
        path.exists(),
        "hud exited 0 and wrote no {} — `--screenshot` did nothing",
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
/// from outside. `tests/run-hud-golden.sh` reads this line back out of the suite
/// for the same reason.
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
        "a page with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not \
         evidence — nothing drew, or only the clear did"
    );

    // ---- 1. the bars are widgets on top of a page --------------------------
    let health = brightness(image, HEALTH_AT);
    let backdrop = brightness(image, BACKDROP_AT);
    eprintln!("hud golden: health fill {health:.1}/255, backdrop {backdrop:.1}/255");
    // No separate "the fill drew at all" floor: the page behind it is already at
    // 72/255, so any floor low enough to be a drew-at-all check is one the page
    // itself would clear. The ratio is the claim, and a UI pass that never ran
    // leaves both points reading the same backdrop and fails it at 1.0.
    assert!(
        health > backdrop * FILL_OVER_BACKDROP,
        "the health bar's fill is {health:.1} and the page behind it is {backdrop:.1} — the \
         bar is not on top of the page, or the whole frame has been flattened"
    );

    // ---- 2. the page is dark rather than absent ----------------------------
    //
    // The other half of claim 1, and the one that stops it being satisfied by a
    // frame that lost its backdrop: `health > backdrop * ratio` holds for
    // `backdrop == 0`, which is what a pass that never ran looks like.
    assert!(
        backdrop > DREW_AT_ALL,
        "the page is at {backdrop:.1}/255, so the backdrop pass reached nothing"
    );

    // ---- 3. the health bar is red, in that order ---------------------------
    let health_red = channel(image, HEALTH_AT, 0);
    let health_green = channel(image, HEALTH_AT, 1);
    let health_blue = channel(image, HEALTH_AT, 2);
    eprintln!(
        "hud golden: health red {health_red:.1}, green {health_green:.1}, blue {health_blue:.1}"
    );
    assert!(
        health_red > health_green * HEALTH_REDNESS && health_red > health_blue * HEALTH_REDNESS,
        "the health bar reads red {health_red:.1} / green {health_green:.1} / blue \
         {health_blue:.1} — either no bar drew there, or the readback's channels were \
         written the wrong way round"
    );

    // ---- 4. the mana bar is blue, in that order ----------------------------
    //
    // Pointing the opposite way to claim 3, so the pair cannot both be satisfied
    // by a frame whose channels were rotated rather than swapped.
    let mana_blue = channel(image, MANA_AT, 2);
    let mana_red = channel(image, MANA_AT, 0);
    eprintln!("hud golden: mana blue {mana_blue:.1}, red {mana_red:.1}");
    assert!(
        mana_blue > DREW_AT_ALL && mana_blue > mana_red * MANA_BLUENESS,
        "the mana bar reads blue {mana_blue:.1} / red {mana_red:.1} — either no fill drew \
         there, or the readback's channels were written the wrong way round"
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hud-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend();
    let (image, adapter) = screenshot_from_a_real_run(&backend);
    eprintln!("hud golden: device on {adapter}");
    inspect(&image);

    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/panels.png");
    let comparison = Golden::new(reference)
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("on {backend}: {message}"));
    eprintln!("hud golden: panels on {backend} — {}", comparison.summary());
}
