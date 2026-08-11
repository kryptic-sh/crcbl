//! Slice 16b: sprites that turn.
//!
//! `Sprite::rotation` is a number on an instance lane and the arithmetic that
//! consumes it lives entirely in `sprite.slang`'s vertex stage, so nothing on
//! the CPU side can check it: a recorder assertion can say the angle reached
//! the buffer and cannot say the quad turned, let alone that it turned about
//! its own centre, by that angle, in that direction. The device test asserts
//! all three from the rotation matrix rather than off the picture, plus that
//! sharp-bilinear still collapses to flat when the quad is not axis-aligned,
//! against `tests/golden/sprite_rotation.png`.
//!
//! # Why one test here is not `#[ignore]`d
//!
//! `the_rotation_frame_of_reference_agrees_with_the_shaders` is pure — no
//! device, no loader, no Vulkan — and could live in `crcbl-vk`'s `src/` tests
//! under the rule that puts it there. It stays in the e2e binary on purpose.
//! [`sprite_local`] is the inverse of the shader's rotation, and an inverted
//! sign in it would relabel every expected colour *consistently*, so the sweep
//! beside it would pass while asserting the mirror image. It pins the frame of
//! reference its neighbours' pixel assertions are written in, and it belongs
//! next to what it protects; `docs/plan/12-testing.md` cites it as the exemplar
//! of placement following what a test needs rather than which directory looks
//! tidier.

use crate::harness::Headless;
use crate::sprite::{
    assert_background, assert_the_camera_maps_a_world_unit_to_a_pixel, close, quad_sheet,
    register_sheet, render_sprites, report_goldens, rgb, sprite_golden, world_to_pixel,
};

/// The angles the rotation frame draws, in **degrees**, in the order their
/// centres are listed.
///
/// 0, 45 and 90 because the brief for this slice names them; 180 and 270 because
/// they are the other two exactly-expressible quarter turns and a sign error in
/// the rotation matrix survives 90 alone; 30 because a general angle is not a
/// multiple of 45 and a shader that snapped the angle to one would pass
/// everything else here.
const ROTATION_DEGREES: [f32; 6] = [0.0, 45.0, 90.0, 180.0, 270.0, 30.0];

/// Where each of those sprites is centred, in world units — a 3 by 2 grid.
///
/// Every centre is a whole number of pixels away from the origin under
/// [`world_to_pixel`], so the rigid snap a rotated `Pixel` sprite gets moves it
/// by nothing and the corner arithmetic below is exact rather than exact to
/// within the snap.
const ROTATION_CENTRES: [[f32; 2]; 6] = [
    [-80.0, 44.0],
    [0.0, 44.0],
    [80.0, 44.0],
    [-80.0, -44.0],
    [0.0, -44.0],
    [80.0, -44.0],
];

/// The rotation suite's sprite before it is turned: **64 by 32, deliberately not
/// square**, so a quarter turn changes the outline and not only the colours.
///
/// Both halves are whole numbers of texels of [`quad_sheet`] — 32 pixels per
/// texel across and 16 down — so an unrotated one is flat blocks and the turned
/// ones have a texel grid coarse enough to read off the picture.
const ROTATION_SIZE: [f32; 2] = [64.0, 32.0];

/// Half the diagonal of [`ROTATION_SIZE`], plus room to probe outside it.
///
/// The columns are 80 world units apart and this is 41.8, so a sweep around one
/// sprite cannot reach the next one — which matters, because every pixel it
/// finds outside a sprite is asserted to be the clear colour.
fn rotation_reach() -> f32 {
    (ROTATION_SIZE[0] * ROTATION_SIZE[0] + ROTATION_SIZE[1] * ROTATION_SIZE[1]).sqrt() / 2.0 + 6.0
}

/// The four sprites of the rotation frame, at the six angles.
fn rotation_sprites(sheet: crcbl_render::SheetId) -> Vec<crcbl_render::Sprite> {
    ROTATION_DEGREES
        .iter()
        .zip(ROTATION_CENTRES)
        .map(|(degrees, centre)| {
            crcbl_render::Sprite::new(
                sheet,
                [
                    centre[0] - ROTATION_SIZE[0] / 2.0,
                    centre[1] - ROTATION_SIZE[1] / 2.0,
                    ROTATION_SIZE[0],
                    ROTATION_SIZE[1],
                ],
                [0.0, 0.0, 1.0, 1.0],
            )
            .with_rotation(degrees.to_radians())
        })
        .collect()
}

/// The colour [`quad_sheet`] shows at a point in the **sprite's own** frame:
/// `local` in world units from the centre, x right and y up.
///
/// `quad_sheet` is written in image order — red, green over blue, yellow — so
/// the top row is the one with the larger `v`, which is where a flipped V would
/// be caught if the earlier tests had not already caught it.
fn quad_sheet_texel(local: [f32; 2]) -> [u8; 3] {
    match (local[0] < 0.0, local[1] > 0.0) {
        (true, true) => [255, 0, 0],     // top-left
        (false, true) => [0, 255, 0],    // top-right
        (true, false) => [0, 0, 255],    // bottom-left
        (false, false) => [255, 255, 0], // bottom-right
    }
}

/// A screen pixel's centre, expressed in the frame of the sprite centred at
/// `centre_pixel` and turned by `radians`.
///
/// The inverse of what the shader does: screen Y runs down and world Y runs up,
/// and `R(-angle)` undoes the turn. Everything below classifies pixels with this
/// and then asserts the colour, so a sign error here would move every assertion
/// together rather than fail one — which is why
/// [`the_rotation_frame_of_reference_agrees_with_the_shaders`] checks it against
/// a hand-worked quarter turn first.
fn sprite_local(centre_pixel: [f32; 2], radians: f32, x: u32, y: u32) -> [f32; 2] {
    let (sin, cos) = radians.sin_cos();
    let dx = (x as f32 + 0.5) - centre_pixel[0];
    // Screen rows run down, world Y runs up.
    let dy = -((y as f32 + 0.5) - centre_pixel[1]);
    [dx * cos + dy * sin, -dx * sin + dy * cos]
}

/// **The frame of reference the pixel assertions are written in, checked by
/// hand.**
///
/// [`sprite_local`] is the inverse of the shader's rotation, and an inverted
/// sign in it would relabel every expected colour consistently — so the sweep
/// below would still pass while asserting the mirror image. This pins it on one
/// worked case that needs no shader at all: a quarter turn counter-clockwise
/// takes the sprite's own top-left corner to the screen's bottom-left.
#[test]
fn the_rotation_frame_of_reference_agrees_with_the_shaders() {
    let centre = [128.0f32, 96.0];
    let quarter = std::f32::consts::FRAC_PI_2;

    // A pixel 8 to the left of the centre and 16 below it, after a quarter turn,
    // came from the sprite's own upper-left: local x negative, local y positive.
    let local = sprite_local(centre, quarter, 120, 112);
    assert!(
        local[0] < 0.0 && local[1] > 0.0,
        "a quarter turn CCW must put the sprite's top-left at the screen's \
         bottom-left; ({}, {}) maps to local {local:?}",
        120,
        112
    );
    assert_eq!(
        quad_sheet_texel(local),
        [255, 0, 0],
        "which is the red texel"
    );

    // And the identity case is the identity: screen up is sprite up.
    let above = sprite_local(centre, 0.0, 128, 80);
    assert!(
        above[1] > 0.0 && above[0].abs() < 1.0,
        "with no rotation a pixel above the centre is above it, got {above:?}"
    );
}

/// **A sprite turns about its own centre, by the angle it was given, in the
/// direction the angle means.**
///
/// Six sprites at 0°, 45°, 90°, 180°, 270° and 30°, all from the same 2×2
/// four-colour sheet at the same 64×32 rect, differing only in
/// [`crcbl_render::Sprite::rotation`].
///
/// Every assertion is computed from the rotation matrix, not read off the
/// picture:
///
/// * the four **corners** of each turned quad, checked two pixels inside (which
///   must be that corner's texel) and three pixels outside (which must be the
///   clear colour) — so a quad that turned by the wrong angle, about the wrong
///   point, or the wrong way round fails on a named pixel;
/// * every pixel whose centre is at least two pixels inside a texel of the
///   turned quad, which must be **exactly** that texel's colour — the evidence
///   that sharp-bilinear still collapses to flat when the quad is not
///   axis-aligned; and
/// * every pixel at least two pixels outside the turned quad, which must be the
///   clear colour — the evidence that the outline turned rather than the sprite
///   merely being repainted inside its old box.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_rotated_sprite_turns_about_its_own_centre_by_the_angle_it_was_given() {
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
    // The control for the flatness claim below, registered up front so both
    // frames come out of one renderer.
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    let sprites = rotation_sprites(sharp);
    let image = render_sprites(&headless, &mut renderer, &mut pool, &sprites);

    let (half_w, half_h) = (ROTATION_SIZE[0] / 2.0, ROTATION_SIZE[1] / 2.0);
    // Two pixels of slack on every classification. One fragment is the widest a
    // sharp-bilinear ramp is at 45° — `fwidth` is an L1 norm, so it reports root
    // two times the scale on the diagonal — and the rasteriser's own edge is
    // another, so anything nearer than two pixels to a boundary is legitimately
    // in transition and is not asserted about at all.
    const MARGIN: f32 = 2.0;

    let mut corner_checks = 0u32;
    let mut interior = 0u32;
    let mut exterior = 0u32;

    for (index, degrees) in ROTATION_DEGREES.into_iter().enumerate() {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        let centre = world_to_pixel(ROTATION_CENTRES[index]);

        // --- the corners, from the matrix -----------------------------------
        //
        // `local` is in the sprite's own frame; `R(angle)` turns it into world
        // and the screen flips Y. At 90°, 180° and 270° `sin` and `cos` are
        // exactly 0 and ±1, so these land on whole pixels.
        let to_screen = |local: [f32; 2]| {
            let wx = local[0] * cos - local[1] * sin;
            let wy = local[0] * sin + local[1] * cos;
            [centre[0] + wx, centre[1] - wy]
        };
        for (sign_x, sign_y) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let expected = quad_sheet_texel([sign_x, sign_y]);
            let inside = to_screen([sign_x * (half_w - MARGIN), sign_y * (half_h - MARGIN)]);
            let actual = rgb(&image, inside[0] as u32, inside[1] as u32);
            assert!(
                close(actual, expected, 2),
                "sprite {index} at {degrees}°: the corner two pixels inside \
                 ({sign_x}, {sign_y}) is pixel ({}, {}), which should be \
                 {expected:?} and is {actual:?}. A wrong colour here is the quad \
                 turned the other way or by the wrong angle; the clear colour is \
                 a quad that is not there at all.",
                inside[0] as u32,
                inside[1] as u32
            );
            let outside = to_screen([sign_x * (half_w + 3.0), sign_y * (half_h + 3.0)]);
            assert_background(&image, outside[0] as u32, outside[1] as u32);
            corner_checks += 2;
        }

        // --- every pixel around the sprite, classified by the same matrix ----
        let reach = rotation_reach();
        let x0 = (centre[0] - reach) as u32;
        let x1 = (centre[0] + reach) as u32;
        let y0 = (centre[1] - reach) as u32;
        let y1 = (centre[1] + reach) as u32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let local = sprite_local(centre, radians, x, y);
                let (u, v) = (local[0], local[1]);
                let deep_inside = u.abs() <= half_w - MARGIN
                    && v.abs() <= half_h - MARGIN
                    // and clear of the texel boundary through the middle, which
                    // is the one place a filter is allowed to blend.
                    && u.abs() >= MARGIN
                    && v.abs() >= MARGIN;
                let well_outside = u.abs() >= half_w + MARGIN || v.abs() >= half_h + MARGIN;
                if deep_inside {
                    let expected = quad_sheet_texel(local);
                    let actual = rgb(&image, x, y);
                    assert!(
                        close(actual, expected, 2),
                        "sprite {index} at {degrees}°: pixel ({x}, {y}) is at \
                         sprite-local {local:?}, which is the {expected:?} texel, \
                         and reads {actual:?}. A gradient here is sharp-bilinear \
                         failing on a turned quad; another texel's colour is the \
                         wrong angle."
                    );
                    interior += 1;
                } else if well_outside {
                    assert_background(&image, x, y);
                    exterior += 1;
                }
            }
        }
    }

    // Anti-vacuity. A sweep that classified nothing passes silently, and so does
    // one that only ever found background.
    assert_eq!(corner_checks, 6 * 8, "every corner of every sprite");
    assert!(
        interior > 6 * 1000,
        "each 64x32 sprite has about 1500 pixels two or more inside a texel; \
         only {interior} were checked across six, so the sweep is not finding \
         the sprites"
    );
    assert!(
        exterior > 6 * 1000,
        "and each sprite is surrounded by more than that much clear space; \
         {exterior} pixels were checked"
    );
    let colors = image.distinct_colors(16);
    assert!(
        colors >= 5,
        "four texel colours and the clear is five; found {colors}"
    );

    // --- the control for the flatness claim ------------------------------
    //
    // The interior sweep says a turned `Pixel` sprite is flat inside each texel.
    // On its own that is not evidence of sharp-bilinear: it is also what a
    // renderer that ignored the sample mode would produce if linear filtering
    // happened to be flat here. The same sprite at the same angle, from a sheet
    // registered `Smooth`, is the frame where it is not flat.
    // `rotation_sprites` is a function of the sheet alone, so index 1 of the
    // smooth set is `sprites[1]` with its sheet swapped and nothing else moved.
    let smooth_sprite = rotation_sprites(smooth)[1];
    let smooth_image = render_sprites(&headless, &mut renderer, &mut pool, &[smooth_sprite]);

    let radians = ROTATION_DEGREES[1].to_radians();
    let centre = world_to_pixel(ROTATION_CENTRES[1]);
    let reach = rotation_reach();
    let texels = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    let blended = |image: &crcbl_golden::Image| {
        let mut count = 0u32;
        for y in (centre[1] - reach) as u32..=(centre[1] + reach) as u32 {
            for x in (centre[0] - reach) as u32..=(centre[0] + reach) as u32 {
                let local = sprite_local(centre, radians, x, y);
                if local[0].abs() > half_w - MARGIN || local[1].abs() > half_h - MARGIN {
                    continue;
                }
                let actual = rgb(image, x, y);
                if !texels.iter().any(|texel| close(actual, *texel, 2)) {
                    count += 1;
                }
            }
        }
        count
    };
    let sharp_blend = blended(&image);
    let smooth_blend = blended(&smooth_image);
    eprintln!(
        "vk e2e: at 45°, {sharp_blend} pixel(s) inside the Pixel sprite are not one \
         of its four texel colours, against {smooth_blend} inside the Smooth one"
    );
    assert!(
        smooth_blend > 500,
        "the control: plain linear at 45° must blend across most of the sprite, \
         and only {smooth_blend} pixels did — so the flatness asserted above is \
         not evidence of anything"
    );
    assert!(
        sharp_blend * 4 < smooth_blend,
        "sharp-bilinear's crossover is a band a fragment or two wide along two \
         internal boundaries; {sharp_blend} blended pixels against the control's \
         {smooth_blend} is a ramp, which means `fwidth` collapsed once the quad \
         was turned"
    );

    let verdict = sprite_golden("sprite_rotation", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}
