//! The horde horde actually presented, off a real device, against a checked-in
//! golden — and three claims about the picture in front of it.
//!
//! # This is the flag's test as much as the frame's
//!
//! `apps/breakout/tests/golden.rs` is the pattern and its module docs carry the
//! full argument; the short version is that this suite runs the **compiled
//! binary** with `--screenshot` and everything it asserts is about the file that
//! binary left behind. The thing under test is the frame a player would have
//! seen — the arena, the field of enemies, the props and the HUD band, every
//! pass the game hung off the swapchain image — not a scene a test rebuilt to
//! look like it.
//!
//! It is also why there is no second render here and no in-process device: the
//! suite owns no GPU at all, and a failure in it is a failure of the sample.
//!
//! # Why this golden is of a prefilled run
//!
//! **A default headless run of horde never leaves its title screen**, because
//! nothing presses a key, and a title screen over an empty arena has none of the
//! scale sample's subject in it: `field: 0` in the summary's `SceneStats`, and a
//! golden of it would go on passing after every enemy sprite stopped drawing.
//!
//! `--prefill N` is the fixture horde's scale measurement already uses
//! for exactly that. It stages the field and then starts the run through the
//! same action map a player's key goes through, so the frame below is a run in
//! `Playing` with hundreds of enemies on it.
//!
//! It is also safe to pin. The steering pool's thread count is the machine's
//! answer unless `--workers` says otherwise, so a golden of a prefilled run is
//! only reproducible if the simulation does not depend on it — checked rather
//! than assumed, by running this invocation at `--workers 1` and `--workers 8`
//! and comparing the two PNGs byte for byte.
//!
//! # A golden alone cannot say the frame is right
//!
//! Two blank frames compare perfectly, and so do two uniformly dark ones against
//! a uniformly dark reference. So the golden is the *last* assertion and the
//! ones before it say **where** the frame is bright, dark and coloured rather
//! than what any one pixel is.
//!
//! Claim 3 is the one that is horde's rather than any sample's: it counts how
//! much of the frame is enemy-red, over the whole image rather than at one
//! point, because a single block on one enemy would say nothing about whether
//! there were four of them or four hundred. A title-screen frame reads 0.00% and
//! fails it outright, and so does a frame whose channels were written the wrong
//! way round — the enemies stop being the red thing in it. That second job is
//! claim 3's alone here: the arena's grass is too desaturated for a greenness
//! bound to refuse a red/blue swap without also refusing legitimate frames, and
//! [`GROUND_GREENNESS`] says so with the numbers.
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
//! green, and `tests/run-horde-golden.sh` is the only thing that turns both
//! off — and it fails when the suite reports zero tests run.
//!
//! # No darkening test here
//!
//! `apps/breakout/tests/golden.rs` carries
//! `a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`,
//! which pins that a uniform multiply is refused. That is a property of
//! `crcbl_golden::Tolerance::RASTERISER`, which this suite compares under
//! unchanged, so a copy here would be a second thing to keep in step and would
//! prove nothing about horde.

#![cfg(feature = "golden-e2e")]

use crcbl_golden::Image;
use crcbl_sample_test::{Block, SampleRun, required_backend};

/// How many frames the run presents before the one that gets written.
///
/// The budget `.github/workflows/ci.yml`'s **Run horde headless against
/// lavapipe** step already gives this binary, so the golden shares its frame
/// index rather than introducing a second one. One second of the default 60 Hz
/// simulation: far past start-up — the atlas has uploaded, the arena has been
/// laid out and the prefilled field has been through sixty steering ticks — and
/// far past the offscreen ring's frames-in-flight, so the image written has been
/// round the ring several times. Long enough that the enemies have closed on the
/// player and short enough that the run is still `Playing` rather than over.
const FRAMES: u32 = 60;

/// How many enemies the run stages before it starts.
///
/// Enough that the field is unmistakably a horde rather than a handful, and well
/// under `crcbl_horde`'s `DEFAULT_MAX_ENEMIES`, so the cap never truncates it
/// and the count in the summary is the count that was asked for.
const PREFILL: u32 = 400;

/// The extent the checked-in golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless
/// run's offscreen ring renders at when `--size` says nothing — so the golden is
/// the frame the default invocation produces rather than one a flag had to ask
/// for.
const EXTENT: (u32, u32) = (960, 720);

/// How many distinct colours a frame of this arena has to have.
///
/// A tiled and shaded ground, hundreds of enemy sprites in two kinds, the props,
/// the player and two lines of HUD text: a frame with fewer than this drew the
/// clear colour and very little else. Counted rather than guessed at — radv
/// draws 2093 and `Image::distinct_colors` stops counting at the bound it is
/// given.
const MIN_COLORS: usize = 256;

/// Half-extents, in pixels, of the block each claim below averages over.
///
/// A block rather than a pixel, for the reason [`Block`] gives.
const BLOCK: (u32, u32) = (6, 6);

/// A patch of open arena ground, clear of every enemy and prop at [`FRAMES`].
const GROUND_AT: (u32, u32) = (200, 250);

/// The middle of the HUD band along the top of the frame.
const HUD_AT: (u32, u32) = (120, 20);

/// How much brighter the HUD band must be than the arena behind it.
///
/// A ratio rather than a level, because a level is a second golden written in
/// numbers and moves whenever the art does. Measured before it was fixed rather
/// than guessed: radv draws the band at 105.6/255 over ground of 37.1, which is
/// 2.8. Each claim prints what it actually got, so the next person sizing it
/// does not have to re-derive it.
const HUD_OVER_GROUND: f32 = 2.0;

/// How much greener than blue the arena ground must be.
///
/// **This is a claim about the arena, not about channel order**, and the
/// distinction was measured rather than assumed. Shaded grass is not a saturated
/// colour: radv draws it at green 46.4 / blue 29.4, which is 1.6, and swapping
/// red for blue leaves green 46.4 over blue 35.3 — still 1.3, so a BGRA readback
/// written as RGBA slips past this bound by a hair. Tightening it to catch that
/// would leave under a tenth of headroom over a legitimate frame, which is not a
/// bound worth having. **Claim 3 is what refuses a swap here**, and decisively:
/// the enemies stop being red at all and the fraction goes to zero.
///
/// What this one does catch is an arena that drew something other than grass, or
/// did not draw at all — the pair with [`HUD_OVER_GROUND`], which reads the same
/// block for brightness.
const GROUND_GREENNESS: f32 = 1.25;

/// What counts as an enemy pixel: this much red against the other two channels.
///
/// The enemies are the one saturated thing in the frame — the ground is desatu-
/// rated green, the props grey-green, the HUD grey-blue and the player cyan — so
/// "red beats both others by half again" selects them and nothing else.
const ENEMY_RED_RATIO: f32 = 1.5;

/// …and this much red on its own, out of 255.
///
/// Without it the ratio alone would also select the darkest corners of the
/// ground, where a couple of levels of noise can put red over green.
const ENEMY_RED_LEVEL: u8 = 60;

/// How much of the frame the horde has to cover, as a fraction of all pixels.
///
/// **The claim that is horde's rather than any sample's.** A block on one enemy
/// would pass on a frame with one enemy on it, which is the regression this
/// sample would actually have: a field that silently stopped reaching the sprite
/// pass, or a cull that ate it. Measured before it was fixed rather than
/// guessed: radv reads 2.2522% and lavapipe 2.2617%, so this leaves a factor of
/// two — and a title-screen frame, which is what a run that failed to start
/// produces, reads 0.0000%.
const ENEMY_RED_FRACTION: f32 = 0.01;

/// The floor a block has to clear to have drawn anything at all.
///
/// Out of 255, and low because that is the question it asks: not "is this the
/// right shade" — the golden answers that — but "did a pass put anything here".
/// The darkest block any claim below reads is the ground's 37.1, so this leaves
/// a factor of six.
const DREW_AT_ALL: f32 = 6.0;

/// What fraction of the frame is enemy-red, by [`ENEMY_RED_RATIO`] and
/// [`ENEMY_RED_LEVEL`].
fn enemy_red_fraction(image: &Image) -> f32 {
    let mut red = 0u32;
    for pixel in image.pixels().chunks_exact(4) {
        let (r, g, b) = (
            f32::from(pixel[0]),
            f32::from(pixel[1]),
            f32::from(pixel[2]),
        );
        if pixel[0] > ENEMY_RED_LEVEL && r > g * ENEMY_RED_RATIO && r > b * ENEMY_RED_RATIO {
            red += 1;
        }
    }
    red as f32 / (image.width() * image.height()) as f32
}

/// The claims in front of the golden: it drew, and it drew in the right places.
fn inspect(image: &Image) {
    let block = Block::new(image, BLOCK, "horde");
    block.distinct_enough("an arena", MIN_COLORS);

    // ---- 1. the HUD band is a band on top of an arena ----------------------
    block.drew(
        "arena ground",
        GROUND_AT,
        DREW_AT_ALL,
        "the arena pass reached nothing",
    );
    block.over(
        ("HUD band", HUD_AT),
        ("arena behind it", GROUND_AT),
        HUD_OVER_GROUND,
        "the band is not on top of the arena",
    );

    // ---- 2. the arena drew grass ------------------------------------------
    //
    // Not the channel-order claim — see [`GROUND_GREENNESS`], which a red/blue
    // swap clears by a hair. Claim 3 is the one that refuses a swap.
    block.channel_beats(
        "arena ground",
        GROUND_AT,
        &[("green", 1), ("blue", 2)],
        GROUND_GREENNESS,
        None,
        "the arena pass drew something that is not grass",
    );

    // ---- 3. there is a horde on the field ----------------------------------
    //
    // The claim that is this sample's, and the one a block on a single enemy
    // could not make: how much of the frame the enemies cover, so a field that
    // arrived at the sprite pass with four of them instead of four hundred fails
    // it. A frame still on the title screen reads zero.
    let horde = enemy_red_fraction(image);
    eprintln!(
        "horde golden: {:.4}% of the frame is enemy-red (floor {:.4}%)",
        horde * 100.0,
        ENEMY_RED_FRACTION * 100.0
    );
    assert!(
        horde > ENEMY_RED_FRACTION,
        "{:.4}% of the frame is enemy-red — the field never reached the sprite pass, the \
         run never started, or the readback's channels were written the wrong way round",
        horde * 100.0
    );
}

/// **The frame the binary presented, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-horde-golden.sh"]
fn the_frame_the_binary_wrote_matches_its_golden() {
    let backend = required_backend("tests/run-horde-golden.sh");
    let prefill = PREFILL.to_string();
    let run = SampleRun {
        name: "horde",
        binary: env!("CARGO_BIN_EXE_horde"),
        tmp_dir: env!("CARGO_TARGET_TMPDIR"),
        file: "horde.png",
        frames: FRAMES,
        extent: EXTENT,
        args: &["--prefill", &prefill],
        // A horde still on its title screen has no field on it at all, so the
        // prefill having reached `Playing` is a precondition for every claim
        // below rather than a nicety.
        stdout_contains: &["Playing"],
        simulation_advanced: true,
    };
    let (image, adapter) = run.screenshot(&backend);
    eprintln!("horde golden: device on {adapter}");
    inspect(&image);
    run.compare_to_golden(&image, &backend, env!("CARGO_MANIFEST_DIR"), "field");
}
