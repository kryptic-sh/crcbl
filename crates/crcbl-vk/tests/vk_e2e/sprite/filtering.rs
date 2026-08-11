//! Sharp-bilinear: `SampleMode::Pixel` against `SampleMode::Smooth`.
//!
//! Its own module because all three tests are the same discrimination — a
//! `Smooth` control beside every `Pixel` claim, without which a renderer that
//! drew nothing, or one that quietly fell through to plain linear, would pass.
//! And they read straight off sampled pixel values rather than through a
//! reference, because a golden blessed from a broken build would agree with
//! itself forever; `tests/golden/sprite_pixel.png` and `sprite_smooth.png` are
//! reported alongside the arithmetic, not instead of it.
//!
//! The third test covers the half the other two cannot see: the snap. Snapping
//! the quad's corners to the device-pixel grid does not change how many
//! fragments an axis-aligned rectangle covers — `ceil(a - 0.5)` is `round(a)` —
//! so a coverage assertion passes with the snap deleted. What it changes is
//! whether the art's own texel grid slides under sub-pixel motion, which is the
//! crawl, so the assertion is that a fifth-of-a-pixel move leaves a `Pixel`
//! frame byte-identical and a `Smooth` one not.

use crate::harness::Headless;
use crate::sprite::{
    SPRITE_EXTENT, assert_the_camera_maps_a_world_unit_to_a_pixel, close, quad_sheet,
    register_sheet, render_sprites, report_goldens, rgb, sprite_golden, world_to_pixel,
};

/// **`Pixel` and `Smooth` are visibly different pictures at a non-integer
/// scale — and different in the place and by the amount predicted.**
///
/// This is the only real evidence that sharp-bilinear happened. A 2×2 sheet
/// drawn 45 pixels wide is 22.5 pixels per texel, so:
///
/// * plain linear blends between the two texel *centres*, which are 22.5 pixels
///   apart — a gradient across the middle half of the sprite; while
/// * sharp-bilinear is flat inside each texel and crosses over in one fragment
///   at the boundary between them.
///
/// So a scanline through the top row of texels has two colours under `Pixel` and
/// a whole ramp of them under `Smooth`, and the assertion is on *that* rather
/// than on a count of differing pixels — which noise could reach.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn pixel_and_smooth_differ_where_and_by_how_much_sharp_bilinear_predicts() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    // The *same pixels* twice, so the only difference between the two frames is
    // the mode the sheet was registered with.
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // 45 units for 2 texels: 22.5 pixels per texel, and a fractional origin so
    // the quad's edges do not land on the pixel grid by accident either.
    let rect = [-22.3f32, -22.7, 45.0, 45.0];
    let sprite = |sheet| crcbl_render::Sprite::new(sheet, rect, [0.0, 0.0, 1.0, 1.0]);
    let pixel_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp)]);
    let smooth_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth)]);

    // The sprite's screen box, from the world rect. Widened by two pixels
    // because `Pixel` snaps its corners to the grid and `Smooth` does not, so
    // the two outlines legitimately differ by up to one fragment.
    let low = world_to_pixel([rect[0], rect[1]]);
    let high = world_to_pixel([rect[0] + rect[2], rect[1] + rect[3]]);
    let box_left = low[0] as u32 - 2;
    let box_right = high[0] as u32 + 2;
    let box_top = high[1] as u32 - 2;
    let box_bottom = low[1] as u32 + 2;

    // Nothing outside that box may differ at all. A "the two are different"
    // test that counted differing pixels anywhere could be satisfied by driver
    // noise in the clear; this says the difference is the sprite.
    let mut outside = 0u32;
    let mut inside = 0u32;
    for y in 0..SPRITE_EXTENT.1 {
        for x in 0..SPRITE_EXTENT.0 {
            if pixel_image.pixel(x, y) == smooth_image.pixel(x, y) {
                continue;
            }
            if x >= box_left && x <= box_right && y >= box_top && y <= box_bottom {
                inside += 1;
            } else {
                outside += 1;
            }
        }
    }
    assert_eq!(
        outside, 0,
        "the two modes must differ only where the sprite is; {outside} pixels \
         elsewhere disagree"
    );
    assert!(
        inside > 500,
        "at 22.5 pixels per texel the two filters cannot agree over most of a \
         45x45 sprite; only {inside} pixels differ, which means the mode is not \
         reaching the shader"
    );

    // The scanline. Row 84 is inside the sprite's *top* row of texels under
    // either mode (that row spans screen y 73.7..96.2 unsnapped and 74..96.5
    // snapped), and the columns are two pixels in from each edge and two either
    // side of the texel boundary at x = 128.5.
    const ROW: u32 = 84;
    let left_run: Vec<[u8; 3]> = (108..127).map(|x| rgb(&pixel_image, x, ROW)).collect();
    let right_run: Vec<[u8; 3]> = (130..148).map(|x| rgb(&pixel_image, x, ROW)).collect();
    assert!(
        left_run.iter().all(|value| *value == left_run[0]),
        "Pixel must be flat inside the left texel; row {ROW} reads {left_run:?}"
    );
    assert!(
        right_run.iter().all(|value| *value == right_run[0]),
        "Pixel must be flat inside the right texel; row {ROW} reads {right_run:?}"
    );
    assert!(
        close(left_run[0], [255, 0, 0], 2) && close(right_run[0], [0, 255, 0], 2),
        "and flat at the texel's own colours, not at some average: {:?} then {:?}",
        left_run[0],
        right_run[0]
    );

    // The same span under `Smooth` is a ramp. The two texel centres are 22.5
    // pixels apart, so there are about that many distinct values between them.
    let smooth_run: Vec<[u8; 3]> = (108..148).map(|x| rgb(&smooth_image, x, ROW)).collect();
    let mut distinct: Vec<[u8; 3]> = Vec::new();
    for value in &smooth_run {
        if !distinct.contains(value) {
            distinct.push(*value);
        }
    }
    assert!(
        distinct.len() >= 10,
        "Smooth blends across the whole gap between texel centres, so row {ROW} \
         must be a ramp of many values; found {} — which is what it would be if \
         Pixel's bend were being applied to both",
        distinct.len()
    );
    // And the same span under Pixel is not: two flat colours and at most a
    // one-fragment crossover between them.
    let mut sharp_distinct: Vec<[u8; 3]> = Vec::new();
    for x in 108..148 {
        let value = rgb(&pixel_image, x, ROW);
        if !sharp_distinct.contains(&value) {
            sharp_distinct.push(value);
        }
    }
    assert!(
        sharp_distinct.len() <= 3,
        "Pixel's ramp is one fragment wide, so row {ROW} is two colours plus at \
         most one crossover; found {} ({sharp_distinct:?})",
        sharp_distinct.len()
    );

    let verdicts = vec![
        sprite_golden("sprite_pixel", &pixel_image),
        sprite_golden("sprite_smooth", &smooth_image),
    ];
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(verdicts);
}

/// **The assertion that pins the arithmetic: `Pixel` at a whole scale is exactly
/// flat inside every texel.**
///
/// Sharp-bilinear's ramp is one *fragment* wide, so at `n` device pixels per
/// texel the fragment centres nearest a boundary sit `0.5/n` texels either side
/// of it — exactly the ramp's half-width — and evaluate to a clean 0 and 1. The
/// picture is therefore bit-identical to nearest, with no intermediate pixel
/// anywhere. Plain linear at the same scale is a gradient across the middle half
/// of each texel pair, which is what the `Smooth` control at the bottom shows.
///
/// Read straight off sampled pixel values, not through a reference: a golden
/// blessed from a broken build would agree with itself forever.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn pixel_mode_is_exactly_flat_inside_each_texel_at_a_whole_scale() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // 64 pixels for 2 texels: 32 device pixels per texel, on the grid.
    let rect = [-32.0f32, -32.0, 64.0, 64.0];
    let sprite = |sheet| crcbl_render::Sprite::new(sheet, rect, [0.0, 0.0, 1.0, 1.0]);
    let pixel_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp)]);
    let smooth_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth)]);

    // Screen x 96..160, y 64..128. Each quadrant is inset by one pixel, so the
    // assertion is about a texel's *interior* and cannot be defeated or
    // satisfied by whatever happens on the boundary itself.
    let quadrants = [
        (97u32, 65u32, [255u8, 0, 0]),
        (129, 65, [0, 255, 0]),
        (97, 97, [0, 0, 255]),
        (129, 97, [255, 255, 0]),
    ];
    let mut flat_under_smooth = 0;
    for (left, top, expected) in quadrants {
        let first = rgb(&pixel_image, left, top);
        assert!(
            close(first, expected, 2),
            "the texel at ({left}, {top}) should be {expected:?}, got {first:?}"
        );
        let mut uniform_smooth = true;
        for y in top..top + 31 {
            for x in left..left + 31 {
                let value = rgb(&pixel_image, x, y);
                assert_eq!(
                    value, first,
                    "Pixel must be exactly flat inside a texel: ({x}, {y}) reads \
                     {value:?} where ({left}, {top}) reads {first:?}. A gradient here \
                     is plain linear sampling, which is the thing sharp-bilinear \
                     replaces."
                );
                uniform_smooth &= rgb(&smooth_image, x, y) == rgb(&smooth_image, left, top);
            }
        }
        flat_under_smooth += u32::from(uniform_smooth);
    }

    // **The control.** If plain linear also came out flat, the assertion above
    // would be measuring nothing — a 2×2 sheet where both filters agree, or a
    // mode that never reached the shader.
    assert_eq!(
        flat_under_smooth, 0,
        "every one of the four texels must be a gradient under Smooth, or the \
         flatness asserted above is not evidence of anything"
    );

    // No intermediate value anywhere in the sprite: at a whole scale the ramp
    // has no fragment to land in, so the picture is exactly four colours.
    let mut inside: Vec<[u8; 3]> = Vec::new();
    for y in 64..128 {
        for x in 96..160 {
            let value = rgb(&pixel_image, x, y);
            if !inside.contains(&value) {
                inside.push(value);
            }
        }
    }
    assert_eq!(
        inside.len(),
        4,
        "a whole scale leaves the ramp with no fragment to land in, so the sprite \
         is exactly its four texels; found {inside:?}"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The snap: a `Pixel` sprite's picture is piecewise-constant in position, and
/// a `Smooth` one is not.**
///
/// This is the half of `SampleMode::Pixel` the filtering tests cannot see.
/// Snapping the quad's corners to the device-pixel grid does not change how many
/// fragments an axis-aligned rectangle covers — `ceil(a - 0.5)` is `round(a)` —
/// so a coverage assertion would pass with the snap deleted. What it changes is
/// where the art's own texel grid lands: with the snap, the sheet's texels start
/// on whole device pixels and stay there while the sprite drifts by fractions of
/// one; without it they slide, and the boundary between two art pixels crosses
/// the fragment grid at a different place every frame. That is the crawl.
///
/// So: move the sprite by a fifth of a pixel, within one rounding bucket. Under
/// `Pixel` the frame must come back **byte for byte identical**. Under `Smooth`
/// — the control, without which this would pass on a renderer that drew nothing
/// — it must not, because a fifth of a pixel genuinely reached the GPU.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_sub_pixel_move_leaves_a_pixel_sprite_alone_and_moves_a_smooth_one() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // Screen x 105.6 and 105.8, which round to the same 106; the width is whole
    // so the far edge rounds together with it. A fractional height too, so the
    // test is not accidentally about one axis.
    let here = [-22.4f32, -22.7, 45.0, 45.0];
    let nudged = [-22.2f32, -22.7, 45.0, 45.0];
    let sprite = |sheet, rect| crcbl_render::Sprite::new(sheet, rect, [0.0, 0.0, 1.0, 1.0]);

    let sharp_here = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp, here)]);
    let sharp_there = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[sprite(sharp, nudged)],
    );
    let smooth_here = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth, here)]);
    let smooth_there = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[sprite(smooth, nudged)],
    );

    let differing = |a: &crcbl_golden::Image, b: &crcbl_golden::Image| {
        (0..SPRITE_EXTENT.1)
            .flat_map(|y| (0..SPRITE_EXTENT.0).map(move |x| (x, y)))
            .filter(|(x, y)| a.pixel(*x, *y) != b.pixel(*x, *y))
            .count()
    };

    let moved = differing(&smooth_here, &smooth_there);
    assert!(
        moved > 100,
        "the control: a fifth of a pixel must reach the GPU and move a Smooth \
         sprite's gradient. Only {moved} pixels changed, so this test is not \
         measuring anything"
    );

    let crawled = differing(&sharp_here, &sharp_there);
    assert_eq!(
        crawled, 0,
        "a Pixel sprite nudged within one rounding bucket must render \
         identically; {crawled} pixels moved, which is the texel grid sliding \
         against the fragment grid — the crawl the snap exists to remove"
    );

    // And it is a picture, not two blank frames agreeing.
    let colors = sharp_here.distinct_colors(16);
    assert!(
        colors >= 5,
        "four texels and a background is five colours; found {colors}"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
