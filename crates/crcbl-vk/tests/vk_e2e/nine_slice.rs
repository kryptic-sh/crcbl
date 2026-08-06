use crate::harness::Headless;
use crate::sprite::{
    SPRITE_EXTENT, assert_background, assert_the_camera_maps_a_world_unit_to_a_pixel, close,
    register_sheet, render_sprites, report_goldens, rgb, sprite_golden, world_to_pixel,
};

// --- slice 6: nine-slice geometry, drawn --------------------------------------
//
// `crcbl_render::nine_slice` is exact arithmetic over rectangles and its unit
// tests assert the quads to the float. What they cannot show is the thing the
// feature exists for: that those quads, handed to the real pass on a real
// driver, come back as one picture — corners the size they were drawn at, edges
// stretched on one axis, and **no seam anywhere between the bands**.
//
// A seam is the failure mode a rect assertion is worst at. Two quads can share
// an edge exactly in world space and still leave a visible line if their UVs
// disagree by a texel, or if the sampler bleeds across a band boundary. Both are
// pixel facts, and this is where they are checked.

/// The nine-slice test sheet: 48×48 texels, a 3×3 grid of 16-texel blocks, and
/// **no two blocks alike**.
///
/// ```text
///   red     green   blue
///   yellow  white   cyan
///   magenta grey    black
/// ```
///
/// Every symmetry a mistake could hide behind is broken. A transposed axis swaps
/// green with yellow, a flipped V swaps red with magenta, a corner drawn with
/// the centre's UVs comes out white, and a band sampled from its neighbour comes
/// out the neighbour's colour rather than a plausible blend of the two.
const NINE_SLICE_COLORS: [[u8; 3]; 9] = [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [255, 255, 255],
    [0, 255, 255],
    [255, 0, 255],
    [128, 128, 128],
    [0, 0, 0],
];

/// The sheet's size in texels, and the inset on all four sides.
const NINE_SLICE_SIZE: u32 = 48;
const NINE_SLICE_INSET: u32 = 16;

fn nine_slice_sheet() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((NINE_SLICE_SIZE * NINE_SLICE_SIZE * 4) as usize);
    for y in 0..NINE_SLICE_SIZE {
        for x in 0..NINE_SLICE_SIZE {
            let cell = (y / NINE_SLICE_INSET) * 3 + (x / NINE_SLICE_INSET);
            let [r, g, b] = NINE_SLICE_COLORS[cell as usize];
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// **A nine-slice stretched well beyond its source, drawn.** A 48×48 frame
/// expanded to 192×160 world units — four times as wide and over three times as
/// tall — with 16-texel insets.
///
/// The scales are whole on purpose, all of them: the corners are 1×, the top and
/// bottom edges 10× across, the left and right edges 8× down, and the centre
/// 10× by 8×. Under [`SampleMode::Pixel`] that makes every band an exactly flat
/// block, so the assertions below can be equality on colours rather than "close
/// to a gradient", and **the whole target must be exactly the sheet's nine
/// colours and nothing else**. A one-texel seam between two bands would show as
/// the clear colour or as a blend, and either fails that count.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_nine_slice_stretched_beyond_its_source_keeps_its_corners_and_shows_no_seam() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    // The geometry first, from the same call the frame is drawn from: nine
    // quads, because every band of this slice is non-empty at this target.
    let source = crcbl_render::NineSliceSource {
        nine: crcbl_render::NineSlice::new(
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
        ),
        frame: crcbl_render::Rect::new(0, 0, NINE_SLICE_SIZE, NINE_SLICE_SIZE),
        sheet_width: NINE_SLICE_SIZE,
        sheet_height: NINE_SLICE_SIZE,
        texels_per_unit: 1.0,
    };
    // World 192 × 160 at 1 unit per pixel: screen x 32..224, y 16..176.
    let target = [-96.0f32, -80.0, 192.0, 160.0];
    let quads = source.expand(target);
    assert_eq!(quads.len(), 9, "every band of this slice has extent");

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
        "nine slice",
        NINE_SLICE_SIZE,
        NINE_SLICE_SIZE,
        crcbl_render::SampleMode::Pixel,
        &nine_slice_sheet(),
    );

    let sprites: Vec<crcbl_render::Sprite> = quads.sprites(sheet, [1.0; 4]).collect();
    assert_eq!(sprites.len(), 9);
    let image = render_sprites(&headless, &mut renderer, &mut pool, &sprites);

    // The band boundaries in screen pixels. `low`/`high` come from the world
    // rect through `world_to_pixel`, so a mistake in the expansion shows up as a
    // wrong colour rather than as a wrong coordinate.
    let low = world_to_pixel([target[0], target[1]]);
    let high = world_to_pixel([target[0] + target[2], target[1] + target[3]]);
    let (left, right) = (low[0] as u32, high[0] as u32); // 32, 224
    let (top, bottom) = (high[1] as u32, low[1] as u32); // 16, 176
    let inset = NINE_SLICE_INSET;
    assert_eq!((left, right, top, bottom), (32, 224, 16, 176));

    // --- every band is the colour it was cut from -------------------------
    let columns = [left + inset / 2, (left + right) / 2, right - inset / 2];
    let rows = [top + inset / 2, (top + bottom) / 2, bottom - inset / 2];
    for (row_index, y) in rows.into_iter().enumerate() {
        for (column_index, x) in columns.into_iter().enumerate() {
            let expected = NINE_SLICE_COLORS[row_index * 3 + column_index];
            let actual = rgb(&image, x, y);
            assert!(
                close(actual, expected, 2),
                "band ({row_index}, {column_index}) at pixel ({x}, {y}) should be \
                 {expected:?}, got {actual:?} — a swapped pair here is a flipped \
                 axis, and white is a corner drawn with the centre's UVs"
            );
        }
    }

    // --- the corners really are their natural size ------------------------
    //
    // The whole claim of a nine-slice: at 4× the source width the caps did not
    // grow. So the colour changes at exactly `inset` pixels in from each edge,
    // and not a pixel earlier or later.
    let middle_row = (top + bottom) / 2;
    let middle_column = (left + right) / 2;
    for (x, y, expected, what) in [
        (
            left + inset - 1,
            rows[0],
            NINE_SLICE_COLORS[0],
            "left cap ends",
        ),
        (
            left + inset,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge starts",
        ),
        (
            right - inset - 1,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge ends",
        ),
        (
            right - inset,
            rows[0],
            NINE_SLICE_COLORS[2],
            "right cap starts",
        ),
        (
            columns[0],
            top + inset - 1,
            NINE_SLICE_COLORS[0],
            "top cap ends",
        ),
        (
            columns[0],
            top + inset,
            NINE_SLICE_COLORS[3],
            "left edge starts",
        ),
        (
            columns[0],
            bottom - inset - 1,
            NINE_SLICE_COLORS[3],
            "left edge ends",
        ),
        (
            columns[0],
            bottom - inset,
            NINE_SLICE_COLORS[6],
            "bottom cap starts",
        ),
        // And the middle of the stretched bands, so "the caps are 16" is not
        // satisfied by a sprite that is only the caps.
        (
            middle_column,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge middle",
        ),
        (
            columns[0],
            middle_row,
            NINE_SLICE_COLORS[3],
            "left edge middle",
        ),
    ] {
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, expected, 2),
            "{what}: ({x}, {y}) should be {expected:?}, got {actual:?} — a corner \
             that stretched with the target moves this boundary"
        );
    }

    // --- no seam, anywhere ------------------------------------------------
    //
    // Every scale here is whole, so under `Pixel` the target is exactly the
    // sheet's nine flat colours. A one-texel gap between two bands shows the
    // clear colour through; a UV that overran shows a tenth colour; a filter
    // bleeding across a band boundary shows a blend. All three fail this.
    // Which of the nine each pixel matched, rather than a set of raw byte
    // triples: two neighbouring drivers can round the grey block to 127 and 128
    // and both are the same band, so counting distinct *values* would fail for a
    // reason that has nothing to do with a seam.
    let mut seen = [false; 9];
    for y in top..bottom {
        for x in left..right {
            let value = rgb(&image, x, y);
            let band = NINE_SLICE_COLORS
                .iter()
                .position(|expected| close(value, *expected, 2));
            let Some(band) = band else {
                panic!(
                    "({x}, {y}) is {value:?}, which is none of the sheet's nine \
                     colours — a seam between two bands, a UV that ran past one, \
                     or a filter blending across a band boundary"
                );
            };
            seen[band] = true;
        }
    }
    assert!(
        seen.iter().all(|hit| *hit),
        "the stretched slice must show all nine of its bands; missing {:?}",
        seen.iter()
            .enumerate()
            .filter(|(_, hit)| !**hit)
            .map(|(band, _)| NINE_SLICE_COLORS[band])
            .collect::<Vec<_>>()
    );

    // --- and it ends where it was told to ---------------------------------
    for (x, y) in [
        (left - 2, middle_row),
        (right + 1, middle_row),
        (middle_column, top - 2),
        (middle_column, bottom + 1),
    ] {
        assert_background(&image, x, y);
    }
    assert_background(&image, 2, 2);
    assert_background(&image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);

    let verdict = sprite_golden("sprite_nine_slice", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}
