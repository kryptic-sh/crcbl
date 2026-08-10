//! Slice 7's nine-slice button skins, drawn.
//!
//! `crcbl_render::button_skin` asserts the quads to the float and
//! `crcbl_ui::widget` asserts the layout to the pixel. Neither can show the
//! thing a reviewer actually wants to see: **the same skin, at two very
//! different widths, with corners that did not smudge.** That is a picture, and
//! this is where it is taken — `tests/golden/button_skin_widths.png`.
//!
//! It is its own module rather than part of the sprite subtree because it is a
//! different generator's output, but it borrows that subtree's fixture whole —
//! `crate::sprite`'s extent, camera, `world_to_pixel` mapping and golden helper
//! — since a button skin is sprite quads once the geometry is worked out.

use crate::harness::Headless;
use crate::sprite::{
    SPRITE_EXTENT, assert_background, assert_the_camera_maps_a_world_unit_to_a_pixel, close,
    register_sheet, render_sprites, report_goldens, rgb, sprite_golden, world_to_pixel,
};

/// The button sheet: 96×32, three 32×32 frames side by side, one per
/// [`ButtonState`].
const BUTTON_FRAME: u32 = 32;
const BUTTON_SHEET_W: u32 = 96;
const BUTTON_SHEET_H: u32 = 32;

/// Insets **different on all four sides**, so no flip, transpose or swapped pair
/// can compare equal by accident: a mirrored X swaps the 6-px cap with the
/// 10-px one, and a mirrored Y the 4-px with the 12-px.
///
/// Every band is left non-empty at 32×32: columns 6 / 16 / 10, rows 4 / 16 / 12.
const BUTTON_NINE: crcbl_render::NineSlice = crcbl_render::NineSlice::new(6, 10, 4, 12);

/// Cell `cell` of frame `frame`, as a byte triple.
///
/// Nine distinct values — 30, 55, … 230 — in the **one channel that frame owns**:
/// idle is red, hovered green, pressed blue. Two independent things are then
/// readable off any single pixel: *which state* drew it, from the channel, and
/// *which of the nine bands* it belongs to, from the value. A state that failed
/// to swap frames shows up as the wrong channel; a band sampled from its
/// neighbour as the wrong value in the right channel.
fn button_cell_color(frame: usize, cell: usize) -> [u8; 3] {
    let value = 30 + cell as u8 * 25;
    match frame {
        0 => [value, 0, 0],
        1 => [0, value, 0],
        _ => [0, 0, value],
    }
}

/// Which of the nine cells a texel of a frame falls in, from its position inside
/// that frame.
fn button_cell(fx: u32, fy: u32) -> usize {
    let column = if fx < BUTTON_NINE.left {
        0
    } else if fx < BUTTON_FRAME - BUTTON_NINE.right {
        1
    } else {
        2
    };
    let row = if fy < BUTTON_NINE.top {
        0
    } else if fy < BUTTON_FRAME - BUTTON_NINE.bottom {
        1
    } else {
        2
    };
    row * 3 + column
}

fn button_skin_sheet() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((BUTTON_SHEET_W * BUTTON_SHEET_H * 4) as usize);
    for y in 0..BUTTON_SHEET_H {
        for x in 0..BUTTON_SHEET_W {
            let frame = (x / BUTTON_FRAME) as usize;
            let [r, g, b] = button_cell_color(frame, button_cell(x % BUTTON_FRAME, y));
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// The three frames as nine-slice sources, built directly rather than through a
/// `Sheet` — `crcbl-vk` does not depend on `crcbl-sprite`, and the point here is
/// the geometry, not the loader.
fn button_source(frame: u32) -> crcbl_render::NineSliceSource {
    crcbl_render::NineSliceSource {
        nine: BUTTON_NINE,
        frame: crcbl_render::Rect::new(frame * BUTTON_FRAME, 0, BUTTON_FRAME, BUTTON_FRAME),
        sheet_width: BUTTON_SHEET_W,
        sheet_height: BUTTON_SHEET_H,
        texels_per_unit: 1.0,
    }
}

/// A button's rectangle on screen, in the Y-**down** pixel space `crcbl-ui` lays
/// out in. `screen_rect_to_target` turns it into the sprite pass's Y-up world.
struct ButtonRect {
    min: glam::Vec2,
    max: glam::Vec2,
    state: crcbl_render::ButtonState,
}

impl ButtonRect {
    fn new(x: f32, y: f32, width: f32, height: f32, state: crcbl_render::ButtonState) -> Self {
        Self {
            min: glam::Vec2::new(x, y),
            max: glam::Vec2::new(x + width, y + height),
            state,
        }
    }

    fn frame(&self) -> usize {
        match self.state {
            crcbl_render::ButtonState::Idle => 0,
            crcbl_render::ButtonState::Hovered => 1,
            crcbl_render::ButtonState::Pressed => 2,
        }
    }

    /// The pixel bounds this button occupies: `[left, top, right, bottom)`.
    ///
    /// Under [`sprite_camera`] one world unit is one pixel and
    /// `screen_rect_to_target` is its exact inverse, so the screen rect handed in
    /// *is* the pixel rect — asserted in the test rather than assumed.
    fn pixels(&self) -> [u32; 4] {
        [
            self.min.x as u32,
            self.min.y as u32,
            self.max.x as u32,
            self.max.y as u32,
        ]
    }
}

/// **The feature, in one picture: one skin at two very different widths.**
///
/// The top two buttons are the *same* frame of the *same* sheet — 48 px wide and
/// 224 px wide, a factor of 4.7 — and the assertion that matters is that their
/// four corner blocks come back **pixel-for-pixel identical**. That is what "the
/// corners don't get smudged" means, said as bytes rather than as an adjective.
///
/// The bottom row is the three states side by side, each drawing its own frame,
/// so a skin that recorded a state change and sampled one frame fails on the
/// channel rather than only on the reference image.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_button_skin_keeps_its_corners_at_two_very_different_widths() {
    use crcbl_render::ButtonState;

    assert_the_camera_maps_a_world_unit_to_a_pixel();

    // Two widths of one skin on top, the three states underneath. Heights are
    // equal within each group so corner blocks can be compared directly.
    let narrow = ButtonRect::new(16.0, 16.0, 48.0, 40.0, ButtonState::Idle);
    let wide = ButtonRect::new(16.0, 72.0, 224.0, 40.0, ButtonState::Idle);
    let states = [
        ButtonRect::new(8.0, 132.0, 72.0, 44.0, ButtonState::Idle),
        ButtonRect::new(92.0, 132.0, 72.0, 44.0, ButtonState::Hovered),
        ButtonRect::new(176.0, 132.0, 72.0, 44.0, ButtonState::Pressed),
    ];

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
        "button skin",
        BUTTON_SHEET_W,
        BUTTON_SHEET_H,
        crcbl_render::SampleMode::Pixel,
        &button_skin_sheet(),
    );

    let skin = crcbl_render::ButtonSkin {
        sheet,
        idle: button_source(0),
        hovered: button_source(1),
        pressed: button_source(2),
    };
    assert!(skin.insets_agree(), "the three frames share their insets");
    assert_eq!(
        skin.insets(),
        crcbl_render::SkinInsets::new(6.0, 10.0, 4.0, 12.0),
        "the insets a button would lay out with are the art's own"
    );

    // Every button through the same public path a caller would use: a screen
    // rect, flipped once, expanded, turned into sprites.
    let mut sprites: Vec<crcbl_render::Sprite> = Vec::new();
    for button in [&narrow, &wide].into_iter().chain(states.iter()) {
        let target =
            crcbl_render::screen_rect_to_target(button.min, button.max, SPRITE_EXTENT, [0.0, 0.0]);
        let quads = skin.quads(button.state, target);
        assert_eq!(
            quads.len(),
            9,
            "{:?} at {:?} lost a band",
            button.state,
            button.pixels()
        );
        sprites.extend(quads.sprites(sheet, [1.0; 4]));
    }
    assert_eq!(sprites.len(), 45, "five buttons of nine quads each");

    let image = render_sprites(&headless, &mut renderer, &mut pool, &sprites);

    // Deferred so the teardown below always runs; unwrapped at the very end.
    let verdict = sprite_golden("button_skin_widths", &image);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_button_pixels(&image, &narrow, &wide, &states);
    }));

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    report_goldens(vec![verdict]);
}

/// Every pixel claim the button golden makes, split out so the test above can run
/// it inside `catch_unwind` and still tear the device down.
fn assert_button_pixels(
    image: &crcbl_golden::Image,
    narrow: &ButtonRect,
    wide: &ButtonRect,
    states: &[ButtonRect],
) {
    // --- the screen rect really is the pixel rect -------------------------
    //
    // Every assertion below indexes pixels with the coordinates the buttons were
    // laid out in, so if `screen_rect_to_target` and the camera disagree they are
    // all reading the wrong place and would pass on a blank frame.
    for button in [narrow, wide].into_iter().chain(states.iter()) {
        let target =
            crcbl_render::screen_rect_to_target(button.min, button.max, SPRITE_EXTENT, [0.0, 0.0]);
        let low = world_to_pixel([target[0], target[1]]);
        let high = world_to_pixel([target[0] + target[2], target[1] + target[3]]);
        assert_eq!(
            [low[0], high[1], high[0], low[1]],
            button.pixels().map(|v| v as f32),
            "the flip and the camera disagree about where this button lands"
        );
    }

    // --- THE FEATURE: the corner blocks are identical at both widths ------
    //
    // Not "the same size" — the same *pixels*. A corner that stretched, or that
    // slid its UVs, or that got filtered differently because its quad changed
    // size, all fail here, and none of them fails a size-only check.
    let [nx0, ny0, nx1, ny1] = narrow.pixels();
    let [wx0, wy0, wx1, wy1] = wide.pixels();
    assert_eq!(ny1 - ny0, wy1 - wy0, "the two buttons must be equally tall");
    let (left, right) = (BUTTON_NINE.left, BUTTON_NINE.right);
    let (top, bottom) = (BUTTON_NINE.top, BUTTON_NINE.bottom);

    // Each corner is walked from its own outer corner inwards, and **one pixel
    // past its own edge**. That extra row and column is what makes the block pin
    // where the corner *ends* rather than only what it contains: a cap that grew
    // changes that one column and nothing inside it.
    //
    // Measured, not assumed. Scaling the fixed bands by `1 + extent / 1000` grows
    // this skin's 6-px left cap to 6.3 px at 48 wide and 7.3 px at 224 — both
    // still start with six columns of the corner's colour, so a block of exactly
    // `left` columns compared equal and this assertion stayed green. Only the
    // band-boundary check further down caught it. With the extra column, this one
    // catches it too.
    let corners = [
        ("top-left", false, false, left + 1, top + 1),
        ("top-right", true, false, right + 1, top + 1),
        ("bottom-left", false, true, left + 1, bottom + 1),
        ("bottom-right", true, true, right + 1, bottom + 1),
    ];
    // The pixel `(dx, dy)` inwards from one corner of a button.
    let corner_pixel = |bounds: [u32; 4], from_right: bool, from_bottom: bool, dx, dy| {
        let [x0, y0, x1, y1] = bounds;
        (
            if from_right { x1 - 1 - dx } else { x0 + dx },
            if from_bottom { y1 - 1 - dy } else { y0 + dy },
        )
    };
    for (name, from_right, from_bottom, w, h) in corners {
        assert!(w > 1 && h > 1);
        for dy in 0..h {
            for dx in 0..w {
                let (ax, ay) = corner_pixel(narrow.pixels(), from_right, from_bottom, dx, dy);
                let (bx, by) = corner_pixel(wide.pixels(), from_right, from_bottom, dx, dy);
                let a = rgb(image, ax, ay);
                let b = rgb(image, bx, by);
                assert!(
                    close(a, b, 2),
                    "the {name} corner differs between a 48px button and a 224px \
                     one, {dx} across and {dy} down from its outer corner: {a:?} \
                     against {b:?} — this is the smudge the whole feature exists \
                     to prevent"
                );
            }
        }
    }

    // --- and they are the right corners, not identically wrong ------------
    //
    // The colour must change at exactly `left` pixels in from the edge, at both
    // widths: one pixel earlier or later is a cap that scaled with the target.
    for (label, [x0, y0, x1, _y1]) in [("narrow", narrow.pixels()), ("wide", wide.pixels())] {
        let band_row = y0 + top / 2; // inside the top row of the nine
        for (x, cell, what) in [
            (x0 + left - 1, 0usize, "the left cap's last column"),
            (x0 + left, 1, "the top edge's first column"),
            (x1 - right - 1, 1, "the top edge's last column"),
            (x1 - right, 2, "the right cap's first column"),
        ] {
            let expected = button_cell_color(0, cell);
            let actual = rgb(image, x, band_row);
            assert!(
                close(actual, expected, 2),
                "{label}: {what} at ({x}, {band_row}) should be {expected:?}, got \
                 {actual:?} — the fixed bands moved with the target"
            );
        }
    }

    // --- the edges DID stretch --------------------------------------------
    //
    // Without this, a renderer that drew nothing but four corners at every size
    // would satisfy every assertion above.
    let narrow_edge = nx1 - right - (nx0 + left);
    let wide_edge = wx1 - right - (wx0 + left);
    assert_eq!(
        wide_edge - narrow_edge,
        (wx1 - wx0) - (nx1 - nx0),
        "the top edge must absorb the whole difference in width"
    );
    // And the wide button really is drawing its top edge out where the narrow one
    // does not even reach.
    let beyond = nx1 + 20;
    assert!(
        beyond < wx1 - right,
        "the sample must be inside the wide edge"
    );
    let actual = rgb(image, beyond, wy0 + top / 2);
    assert!(
        close(actual, button_cell_color(0, 1), 2),
        "at ({beyond}, {}) the wide button should still be stretching its top \
         edge, got {actual:?}",
        wy0 + top / 2
    );

    // --- each state drew its own frame ------------------------------------
    for button in states {
        let [x0, y0, x1, y1] = button.pixels();
        let frame = button.frame();
        let centre = ((x0 + x1) / 2, (y0 + y1) / 2);
        let expected = button_cell_color(frame, 4); // the centre band
        let actual = rgb(image, centre.0, centre.1);
        assert!(
            close(actual, expected, 2),
            "{:?} drew {actual:?} at its centre, not frame {frame}'s {expected:?} \
             — the state swapped nothing",
            button.state
        );
        // The channel this state owns dominates, which no other state's frame can
        // satisfy: a wrong frame is a wrong channel, not a near miss.
        let dominant = (0..3).max_by_key(|c| actual[*c]).expect("three channels");
        assert_eq!(
            dominant, frame,
            "{:?} lit channel {dominant}, which belongs to frame {dominant}",
            button.state
        );
    }

    // --- no seam anywhere, in any of the five ------------------------------
    //
    // Every scale here is whole, so under `Pixel` each button is exactly its
    // frame's nine flat colours. A one-texel gap between bands shows the clear
    // colour through, a UV that overran shows a tenth colour, and a filter
    // bleeding across a boundary shows a blend. All three fail this.
    for button in [narrow, wide].into_iter().chain(states.iter()) {
        let [x0, y0, x1, y1] = button.pixels();
        let frame = button.frame();
        let palette: Vec<[u8; 3]> = (0..9).map(|cell| button_cell_color(frame, cell)).collect();
        let mut seen = [false; 9];
        for y in y0..y1 {
            for x in x0..x1 {
                let value = rgb(image, x, y);
                let Some(band) = palette.iter().position(|c| close(value, *c, 2)) else {
                    panic!(
                        "({x}, {y}) inside the {:?} button is {value:?}, which is \
                         none of frame {frame}'s nine colours — a seam between two \
                         bands, a UV that ran past one, or a filter blending across \
                         a band boundary",
                        button.state
                    );
                };
                seen[band] = true;
            }
        }
        assert!(
            seen.iter().all(|hit| *hit),
            "the {:?} button must show all nine of its bands; missing {:?}",
            button.state,
            seen.iter()
                .enumerate()
                .filter(|(_, hit)| !**hit)
                .map(|(band, _)| band)
                .collect::<Vec<_>>()
        );
    }

    // --- and nothing was drawn outside them --------------------------------
    for button in [narrow, wide].into_iter().chain(states.iter()) {
        let [x0, y0, x1, y1] = button.pixels();
        assert_background(image, x0 - 2, (y0 + y1) / 2);
        assert_background(image, x1 + 1, (y0 + y1) / 2);
        assert_background(image, (x0 + x1) / 2, y0 - 2);
    }
    assert_background(image, 2, 2);
    assert_background(image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);
    // The gap between the narrow button's right edge and the wide one's — the
    // clearest place a runaway stretch would show.
    assert_background(image, nx1 + 40, (ny0 + ny1) / 2);
}
