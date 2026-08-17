//! The mirrored wizard, rasterised: a reversed `u` range draws the exact
//! column-reversed image.
//!
//! `apps/horde/src/art.rs` turns the player left and right by swapping a
//! frame's `u` ends — `art::mirrored` — and the CPU test for it asserts the
//! exact reversal and that the reversed range stays inside the frame's own
//! interval. What it cannot check is that the *shader* honours the reversal:
//! the argument that it does is read off `sprite.slang`, where `u` is an
//! unconditional `lerp(uv.x, uv.z, corner.x)` with no clamp and no `saturate`,
//! and `sharpen` is written in terms of `fwidth`, which is symmetric. Read off
//! the source is not the same as seen in a picture.
//!
//! It is separate from its siblings because it is a **self-comparison** rather
//! than a golden: one frame draws frame A of the asymmetric sheet, the next
//! draws the same sprite mirrored, and the assertion is that the second image
//! is the first reversed left to right at every pixel. No reference PNG is
//! checked in — the two frames are each other's reference, which is what makes
//! the sheet's asymmetry load-bearing here. A sprite that ignored its `uv`
//! entirely would draw the same picture twice and pass; and frame B sits
//! immediately to the right of frame A, so a reversed range that strayed
//! outside frame A's interval bleeds frame B's colours.
//!
//! Frame A's internal texel boundary runs down the middle of the sprite, so the
//! sharp-bilinear ramp at it is part of what is compared — a filter that needed
//! its `u` axis oriented a particular way would come apart there.

use crate::harness::Headless;
use crate::sprite::{
    FRAME_A, SPRITE_EXTENT, assert_the_camera_maps_a_world_unit_to_a_pixel, asymmetric_sheet,
    background_rgb, close, register_sheet, render_sprites, rgb,
};

/// The same frame, mirrored left to right — `art::mirrored`'s swap, reproduced
/// here because the horde sample is not a dependency of this crate.
/// `Sprite::uv` is `[u0, v0, u1, v1]` top-left first, so swapping the two ends
/// runs the frame backwards across the quad.
const fn mirrored(uv: [f32; 4]) -> [f32; 4] {
    [uv[2], uv[1], uv[0], uv[3]]
}

/// The largest per-channel difference between two RGB triples.
fn channel_diff(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter()
        .zip(&b)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .expect("three channels")
}

/// **A reversed `u` range draws the exact column-reversed image.**
///
/// The same 128×64 sprite at the same rect, rendered twice — once sampling
/// frame A normally, once with [`mirrored`] — so the two frames differ in
/// exactly one way: the direction `u` runs across the quad. The sprite is
/// centred on the frame's horizontal middle, so its column range is its own
/// mirror and the whole-frame comparison below is the within-sprite one:
/// outside the sprite both sides are the clear colour, inside it the assertion
/// is exactly "the reversed range drew the mirror image".
///
/// Both frames land on whole pixels, so `Pixel` mode's rigid snap moves the
/// quad by nothing and the comparison lines up column for column. Frame A's
/// internal texel boundary runs straight down the middle of the sprite, so the
/// sharp-bilinear ramp at it is part of what is compared — `sharpen` gets its
/// ramp width from `fwidth`, which is symmetric, and this is the picture that
/// says so.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_mirrored_frame_is_the_column_reversed_image() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "mirror sheet",
        4,
        2,
        crcbl::render::SampleMode::Pixel,
        &asymmetric_sheet(),
    );

    // 128 world units wide and 64 tall — one world unit is one pixel under the
    // sprite camera — centred on the frame's horizontal middle (world x 0,
    // screen column 128) so the sprite's column range is its own mirror and the
    // whole-frame comparison below is the within-sprite one. The centre is a
    // whole-pixel centre, so `Pixel` mode's rigid snap moves it by nothing.
    let rect = [-64.0f32, -32.0, 128.0, 64.0];
    let plain = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[crcbl::render::Sprite::new(sheet, rect, FRAME_A)],
    );
    let mirror = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[crcbl::render::Sprite::new(sheet, rect, mirrored(FRAME_A))],
    );

    // --- the golden: the mirrored image is the plain image, column-reversed ---
    //
    // Every pixel of the frame, compared against its mirror about the frame's
    // vertical centre line. Outside the sprite both sides are the clear colour,
    // so the comparison's substance is the sprite interior — including the
    // sharp-bilinear ramp at frame A's internal texel boundary, which `sharpen`
    // widens with the symmetric `fwidth` rather than with anything oriented.
    let (width, height) = SPRITE_EXTENT;
    let mut failing = 0u32;
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);
    for y in 0..height {
        for x in 0..width {
            let actual = rgb(&mirror, x, y);
            let expected = rgb(&plain, width - 1 - x, y);
            let distance = channel_diff(actual, expected);
            if distance > worst {
                worst = distance;
                worst_at = (x, y);
            }
            if !close(actual, expected, 2) {
                failing += 1;
                if failing == 1 {
                    eprintln!(
                        "{}: mirror — first mismatch at ({x}, {y}): the mirrored \
                         sprite reads {actual:?}, the plain one column-reversed reads \
                         {expected:?} (diff {distance})",
                        crate::SUITE
                    );
                }
            }
        }
    }
    eprintln!(
        "{}: mirror — column-reversed comparison: {failing} mismatching \
         pixel(s) out of {} against a slack of 2, worst diff {worst} at {worst_at:?}",
        crate::SUITE,
        width * height
    );
    assert_eq!(
        failing, 0,
        "the mirrored sprite must be the plain one reversed left to right, to \
         within the 2-value slack; {failing} pixels disagreed, worst {worst} at \
         {worst_at:?}. A reversed `u` range is only correct if the quad really \
         draws the column-reversed image, and the ramp at frame A's internal \
         texel boundary is where a `fwidth` asymmetry would show up."
    );

    // --- anti-vacuity ---------------------------------------------------------
    //
    // A symmetric picture passes a column-reversed comparison for the wrong
    // reason — a sprite that ignored its `uv` entirely would pass it too,
    // because frame A would then be compared against itself. Three checks make
    // the asymmetry load-bearing:
    //
    // 1. The plain frame is genuinely **not** column-symmetric, so the golden
    //    above is not comparing a picture to itself.
    // 2. The mirrored frame genuinely differs from the plain one, so the golden
    //    is not two identical frames agreeing by accident.
    // 3. The sprite is really there: the plain frame's non-background pixels
    //    number more than the sprite's own 128×64 area, and a frame with no
    //    sprite passes everything above by comparing the clear colour to itself.
    let mut non_background = 0u32;
    let mut plain_asymmetry = 0i32;
    let mut mirror_difference = 0i32;
    for y in 0..height {
        for x in 0..width {
            let plain_pixel = rgb(&plain, x, y);
            if !close(plain_pixel, background_rgb(), 2) {
                non_background += 1;
            }
            plain_asymmetry =
                plain_asymmetry.max(channel_diff(plain_pixel, rgb(&plain, width - 1 - x, y)));
            mirror_difference =
                mirror_difference.max(channel_diff(rgb(&mirror, x, y), plain_pixel));
        }
    }
    eprintln!(
        "{}: mirror — {non_background} non-background pixel(s) in the plain \
         frame (the 128×64 sprite is 8192); the plain frame is column-asymmetric \
         by {plain_asymmetry}; the two frames differ by {mirror_difference}",
        crate::SUITE
    );
    assert!(
        non_background > 64 * 32,
        "the plain frame must contain the sprite: {non_background} pixels differ \
         from the clear colour, and a frame with no sprite passes a \
         self-comparison by comparing the background to itself"
    );
    assert!(
        plain_asymmetry > 2,
        "the plain frame must not be column-symmetric: frame A is red/green over \
         blue/yellow, so a sprite pixel and its mirror must differ by more than \
         the slack; the largest difference found is {plain_asymmetry}"
    );
    assert!(
        mirror_difference > 2,
        "the mirrored frame must differ from the plain one: the uv was reversed \
         over an asymmetric frame, and the largest difference found is \
         {mirror_difference} — identical frames would mean the shader ignored \
         the uv"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
