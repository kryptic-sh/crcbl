//! The sprite pass drawing: the right sprite, the right way up, in the right
//! place, from the right frame — and compositing rather than replacing.
//!
//! This is the module the whole slice exists for; before it nothing in the
//! sprite path had been shown to put a single pixel anywhere. It holds the
//! placement and blending claims, which is what separates it from `filtering`:
//! every assertion is at a named pixel computed through
//! [`world_to_pixel`](crate::sprite::world_to_pixel), or against the
//! linear-light blend arithmetic, rather than against a picture.
//!
//! `every_batch_draws_its_own_instances_rather_than_the_first_batchs` is the
//! regression that earns the file. `SpriteRenderer::add_pass` emits one draw per
//! batch with `firstInstance` set to the batch's start, and Slang compiles
//! `SV_InstanceID` to `InstanceIndex - BaseInstance` on SPIR-V — HLSL's
//! semantics, where the id excludes the base — so every batch after the first
//! re-read the *first* batch's instances. It shipped because every other sprite
//! test registers one sheet, or two whose pixels are identical; this one uses
//! solid one-texel sheets of different colours, so the sheet that was bound and
//! the instance that was read are separable.
//!
//! Goldens: `tests/golden/sprite.png`, `sprite_multi_sheet.png` and
//! `sprite_alpha.png`.

use crate::harness::Headless;
use crate::sprite::{
    FRAME_A, FRAME_A_CORNERS, FRAME_B, FRAME_B_CORNERS, HALF_ALPHA, SOLID_SHEETS, SPRITE_EXTENT,
    alpha_sheet, assert_background, assert_the_camera_maps_a_world_unit_to_a_pixel,
    asymmetric_sheet, background_rgb, close, register_sheet, render_sprites, report_goldens, rgb,
    solid_sheet, sprite_golden, srgb_byte, srgb_decode, world_to_pixel,
};

/// **The sprite pass draws.** Two sprites, two different frames of one
/// asymmetric sheet, at two places, at a whole scale.
///
/// This is the test the whole slice exists for: before it, nothing in the sprite
/// path had been shown to put a single pixel anywhere. Every assertion is on a
/// coordinate computed from the world rect through [`world_to_pixel`], so a
/// sprite in the wrong place, upside down, mirrored, or showing the wrong frame
/// fails on a named pixel rather than only on a reference image.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_sprite_is_drawn_the_right_way_up_in_the_right_place_from_the_right_frame() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "asymmetric",
        4,
        2,
        crcbl_render::SampleMode::Pixel,
        &asymmetric_sheet(),
    );

    // 64 world units is 64 pixels is 32 pixels per texel — a whole scale, so the
    // quadrants are square blocks of one colour and their centres are exact.
    let first = [-100.0f32, 16.0, 64.0, 64.0];
    let second = [20.0f32, -80.0, 64.0, 64.0];
    let image = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[
            crcbl_render::Sprite::new(sheet, first, FRAME_A),
            crcbl_render::Sprite::new(sheet, second, FRAME_B),
        ],
    );

    // Anti-vacuity, first: a blank frame passes a golden forever after it is
    // blessed, so the frame has to be a picture before the reference is worth
    // anything. Eight texel colours plus the clear is nine.
    let colors = image.distinct_colors(32);
    assert!(
        colors >= 9,
        "the frame must contain both sprites' eight colours and the clear; found {colors}"
    );

    // The quadrant centres, in `[top-left, top-right, bottom-left, bottom-right]`
    // order — which is the order the corner tables are written in and therefore
    // the order a flip would break.
    let quadrant_centres = |rect: [f32; 4]| {
        let low = world_to_pixel([rect[0], rect[1]]);
        let quarter = rect[2] / 4.0;
        let (left, right) = (low[0] + quarter, low[0] + 3.0 * quarter);
        // `low` is the world *minimum* corner, which is the **bottom** of the
        // quad, so it is the larger screen row.
        let (top, bottom) = (low[1] - 3.0 * quarter, low[1] - quarter);
        [
            [left as u32, top as u32],
            [right as u32, top as u32],
            [left as u32, bottom as u32],
            [right as u32, bottom as u32],
        ]
    };

    for (rect, corners, name) in [
        (first, FRAME_A_CORNERS, "frame A"),
        (second, FRAME_B_CORNERS, "frame B"),
    ] {
        for (centre, expected) in quadrant_centres(rect).into_iter().zip(corners) {
            let actual = rgb(&image, centre[0], centre[1]);
            assert!(
                close(actual, expected, 2),
                "{name} at pixel ({}, {}) should be {expected:?}, got {actual:?} — a \
                 swapped pair here is a flipped U or V, and a whole different palette \
                 is the wrong frame",
                centre[0],
                centre[1]
            );
        }
    }

    // And the sprites end where they are supposed to: two pixels outside each
    // edge is still the clear colour. Without this a full-screen quad of the
    // right colours would pass everything above.
    for rect in [first, second] {
        let low = world_to_pixel([rect[0], rect[1]]);
        let high = world_to_pixel([rect[0] + rect[2], rect[1] + rect[3]]);
        let middle_y = ((low[1] + high[1]) / 2.0) as u32;
        let middle_x = ((low[0] + high[0]) / 2.0) as u32;
        assert_background(&image, low[0] as u32 - 2, middle_y);
        assert_background(&image, high[0] as u32 + 2, middle_y);
        assert_background(&image, middle_x, high[1] as u32 - 2);
        assert_background(&image, middle_x, low[1] as u32 + 2);
    }
    // Including the corners of the frame, which no sprite is anywhere near.
    assert_background(&image, 2, 2);
    assert_background(&image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);

    let verdict = sprite_golden("sprite", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}

/// **Every batch draws its own instances.** Four sprites over three sheets —
/// red, green, blue, then red again — at four separate rectangles, which is
/// four batches because a batch is a *consecutive* run of one sheet.
///
/// The regression this exists for, written up in `docs/backlog.md` before it was
/// fixed: `SpriteRenderer::add_pass` used to emit one
/// `draw(0..6, batch.instances)` per batch, so `firstInstance` was the batch's
/// start index and `sprite.slang` had to index the frame's instance buffer
/// **absolutely** for that to mean anything. It did not — Slang compiles
/// `SV_InstanceID` to `InstanceIndex - BaseInstance` on SPIR-V, which is HLSL's
/// semantics where the id excludes `StartInstanceLocation` — so every batch
/// after the first re-read the *first* batch's instances and redrew the first
/// rectangle with a later sheet bound. Three rectangles came out empty and one
/// came out the wrong colour.
///
/// **Solid one-texel sheets on purpose.** A rectangle's colour names the sheet
/// that was bound and its position names the instance that was read, so the two
/// halves of the mistake are separable: the old bug moved the colour without
/// moving the rectangle. Every sprite test above this one registers a single
/// sheet, and the two filtering tests register two sheets whose *pixels are
/// identical*, so all of them passed with the bug in place — which is exactly
/// why it shipped.
///
/// **Four sprites, not three.** The fourth names the first sheet again, so the
/// batch bases are 0, 1, 2, 3 and one sheet is bound twice at two different
/// bases. A fix that only worked while each sheet appeared once would pass with
/// three.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn every_batch_draws_its_own_instances_rather_than_the_first_batchs() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");

    let sheets: Vec<crcbl_render::SheetId> = SOLID_SHEETS
        .iter()
        .map(|(name, rgb)| {
            register_sheet(
                &mut renderer,
                headless.device.as_ref(),
                name,
                1,
                1,
                crcbl_render::SampleMode::Pixel,
                &solid_sheet(*rgb),
            )
        })
        .collect();

    // Four 48×48 rectangles in a row, 16 world units apart, so the "two pixels
    // outside the edge is still the clear colour" checks below cannot reach a
    // neighbour.
    let rects: [[f32; 4]; 4] = [
        [-116.0, 20.0, 48.0, 48.0],
        [-52.0, 20.0, 48.0, 48.0],
        [12.0, 20.0, 48.0, 48.0],
        [76.0, 20.0, 48.0, 48.0],
    ];
    // Sheet 0, 1, 2, 0 — four batches, three sheets.
    let named: [usize; 4] = [0, 1, 2, 0];

    let sprites: Vec<crcbl_render::Sprite> = rects
        .iter()
        .zip(named)
        .map(|(rect, sheet)| crcbl_render::Sprite::new(sheets[sheet], *rect, [0.0, 0.0, 1.0, 1.0]))
        .collect();

    let image = render_sprites(&headless, &mut renderer, &mut pool, &sprites);

    // Anti-vacuity: three sheet colours plus the clear is four, and the bug drew
    // one rectangle in one colour over the clear, which is two.
    let colors = image.distinct_colors(32);
    assert!(
        colors >= 4,
        "the frame must contain all three sheet colours and the clear; found {colors}"
    );

    for (index, (rect, sheet)) in rects.iter().zip(named).enumerate() {
        let (name, expected) = SOLID_SHEETS[sheet];
        let centre = world_to_pixel([rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0]);
        let (x, y) = (centre[0] as u32, centre[1] as u32);
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, expected, 2),
            "batch {index} should have drawn its own instance — rectangle {index} at \
             pixel ({x}, {y}) should be the {name} sheet {expected:?}, got {actual:?}. \
             The clear colour here means this batch drew somewhere else entirely, \
             which is the batch reading the first batch's instance."
        );
    }

    // And each rectangle stops where its own instance says it does. Without
    // this, four full-width bands would satisfy everything above.
    for rect in rects {
        let low = world_to_pixel([rect[0], rect[1]]);
        let high = world_to_pixel([rect[0] + rect[2], rect[1] + rect[3]]);
        let middle_y = ((low[1] + high[1]) / 2.0) as u32;
        let middle_x = ((low[0] + high[0]) / 2.0) as u32;
        assert_background(&image, low[0] as u32 - 2, middle_y);
        assert_background(&image, high[0] as u32 + 2, middle_y);
        assert_background(&image, middle_x, high[1] as u32 - 2);
        assert_background(&image, middle_x, low[1] as u32 + 2);
    }
    assert_background(&image, 2, 2);
    assert_background(&image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);

    let verdict = sprite_golden("sprite_multi_sheet", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}

/// **Alpha composites, it does not replace.** One sprite whose four texels are
/// opaque, half-transparent, opaque and fully transparent, over a background
/// that is not black.
///
/// Checked against the arithmetic rather than against a picture: the blend is
/// `src * a + dst * (1 - a)` in **linear** light, so the expected byte is the
/// sRGB encoding of that sum with the destination decoded first. A pass that
/// premultiplied in the shader as well as in the blender would land visibly
/// short of it, and a pass that ignored alpha entirely would land on the source.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn alpha_blending_composites_the_sprite_onto_what_is_already_there() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "alpha",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &alpha_sheet(),
    );

    let rect = [-32.0f32, -32.0, 64.0, 64.0];
    let image = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[crcbl_render::Sprite::new(sheet, rect, [0.0, 0.0, 1.0, 1.0])],
    );

    // Screen: x 96..160, y 64..128, so each texel is a 32-pixel square and the
    // quadrant centres are 16 in from each corner.
    let (x0, y0) = (96u32, 64u32);
    let opaque_red = rgb(&image, x0 + 16, y0 + 16);
    let blended = rgb(&image, x0 + 48, y0 + 16);
    let opaque_green = rgb(&image, x0 + 16, y0 + 48);
    let untouched = rgb(&image, x0 + 48, y0 + 48);
    let background = background_rgb();

    assert!(
        close(opaque_red, [255, 0, 0], 2),
        "an alpha of 1 must land on the source colour, got {opaque_red:?}"
    );
    assert!(
        close(opaque_green, [0, 255, 0], 2),
        "and so must the other opaque texel, got {opaque_green:?}"
    );
    assert!(
        close(untouched, background, 2),
        "an alpha of 0 must leave the destination exactly as it was: expected \
         {background:?}, got {untouched:?}"
    );

    // The half-transparent texel, computed rather than eyeballed. The
    // destination is the *stored* background byte decoded back to linear, not
    // `SPRITE_CLEAR` — the clear has already been through an 8-bit encode.
    let expected: [u8; 3] = std::array::from_fn(|channel| {
        let source = f32::from([255u8, 0, 0][channel]) / 255.0;
        let destination = srgb_decode(f32::from(background[channel]) / 255.0);
        srgb_byte(source.mul_add(HALF_ALPHA, destination * (1.0 - HALF_ALPHA)))
    });
    assert!(
        close(blended, expected, 3),
        "a half-transparent red over {background:?} blends to {expected:?} in linear \
         light; got {blended:?}"
    );
    // And it really is *between* the two, which is the property a premultiply
    // bug breaks in one direction and an ignored alpha breaks in the other.
    for channel in 0..3 {
        let (low, high) = (
            background[channel].min([255u8, 0, 0][channel]),
            background[channel].max([255u8, 0, 0][channel]),
        );
        assert!(
            blended[channel] > low && blended[channel] < high,
            "channel {channel} of the blend is {} — outside the open interval \
             ({low}, {high}) it has to be in",
            blended[channel]
        );
    }

    let colors = image.distinct_colors(8);
    assert!(
        colors >= 4,
        "three drawn texels and the clear is four colours; found {colors}"
    );

    let verdict = sprite_golden("sprite_alpha", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}
