//! The atmosphere's aerial perspective on a device: the air the host's
//! `AerialView` says is in front of a floor is the air the frame puts there,
//! and nowhere else.
//!
//! A file of its own rather than more of `render_e2e.rs`, on
//! [`still_pool`](super::still_pool)'s terms; every helper it reads is that
//! file's.

use crcbl::screenshot::{
    AERIAL_EYE_UP, AERIAL_KM_PER_UNIT, OffscreenSetup, aerial_camera, aerial_fog, aerial_forward,
    aerial_sky,
};
use crcbl::shaders::atmosphere::{AerialView, SkyView};
use crcbl_golden::{Image, srgb_decode, srgb_encode};

use super::{
    EXTENT, Offscreen, SUITE, atmosphere_ray, block_channel, block_pixels, channel_order,
    scene_referred,
};

/// The row the far bands are centred on, a few under the horizon, which is
/// tens of kilometres of air away at [`AERIAL_KM_PER_UNIT`] and
/// [`AERIAL_EYE_UP`].
///
/// **A band [`BAND`] rows tall and no taller**, because the distance to the
/// floor changes by a quarter from one row to the next this close to the
/// horizon: the prediction is per pixel, and a thin band keeps it a statement
/// about a narrow range of distances.
const FAR_ROW: u32 = 98;

/// The row the near bands sit on: near the bottom of the frame, a couple of
/// units of floor from the eye.
const NEAR_ROW: u32 = 180;

/// The column the dark half's bands are centred on; the light half's are its
/// mirror, [`mirror`] of it.
const DARK_COLUMN: u32 = 64;

/// Every floor band's half-extent, in `block_pixels`' half-open terms: wide,
/// and one row either side of the centre's top edge — two rows.
const BAND: (u32, u32) = (24, 1);

/// The sky rows compared pixel for pixel between the two arms: everything from
/// near the top of the frame to well above the horizon.
const SKY_ROWS: std::ops::RangeInclusive<u32> = 8..=80;

/// The column `column` mirrors to about the camera's own, which the fixture's
/// sun and halves are symmetric about.
const fn mirror(column: u32) -> u32 {
    EXTENT.0 - 1 - column
}

/// The least the far dark band must gain over the control on each channel, in
/// levels of 255.
///
/// The anti-vacuity floor on the air itself: the dark half is dark enough that
/// the in-scatter the air adds is levels to dozens of them at tens of
/// kilometres, and a composite that composed nothing — no atmosphere reaching
/// the block, the switch misread, a LUT of the identity — gains nothing here.
/// Measured at 9.82, 20.51 and 35.97 levels in red, green and blue on the AMD
/// proprietary Vulkan driver on an RX 7900 XTX (2026-09-27) — red least, the
/// air being Rayleigh's; this is about half the least of them.
const MIN_AERIAL_LEVELS: f32 = 5.0;

/// How far a far band may sit from the host's prediction, in levels of 255.
///
/// The prediction is the control frame's own pixel, decoded, composed with the
/// host's `AerialView` along that pixel's ray and encoded again — so what is
/// left between the two is the control's eight-bit quantisation carried
/// through the composite, the `Rgba16Float` scene target the device composes
/// in, and the ray, which the GPU builds out of two matrix products. Measured
/// at 0.39 levels on [`MIN_AERIAL_LEVELS`]' adapter (2026-09-27); this is about
/// two and a half times it. Unmeasured on lavapipe, Metal and D3D12.
const AERIAL_LEVELS: f32 = 1.0;

/// The mean of `channel` over the band centred on `centre`, as the host
/// predicts it: each pixel of `control` decoded to the radiance it stands for,
/// composed with `air` along that pixel's ray at the floor's distance, and
/// encoded again.
///
/// Per pixel and then averaged, for `predicted_block_channel`'s reason: the
/// encode is not linear, and the distance changes across the band.
fn predicted_band(control: &Image, air: &AerialView, centre: (u32, u32), channel: usize) -> f32 {
    let camera = aerial_camera();
    let mut total = 0.0f32;
    let mut count = 0u32;
    for (x, y) in block_pixels(centre, BAND) {
        let ray = atmosphere_ray(&camera, x, y);
        assert!(ray[1] < 0.0, "the band at row {y} is not on the floor");
        let distance = AERIAL_EYE_UP / -ray[1];
        let pixel = control.pixel(x, y).expect("inside the frame");
        let scene = [0, 1, 2].map(|lane| srgb_decode(f32::from(pixel[lane]) / 255.0));
        let composed = air.composite(scene, ray, distance);
        total += srgb_encode(composed[channel].min(1.0)) * 255.0;
        count += 1;
    }
    total / count as f32
}

/// One frame of `setup`, read back.
fn frame_of(setup: &mut OffscreenSetup) -> Image {
    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image")
}

/// How many frames each renderer draws before the one compared in the
/// off-position arm: enough that every slot of the ring has come round, so a
/// slot still holding an aerial LUT would be read.
const FRAMES: usize = 4;

/// **The air in front of a floor is the host's `AerialView`, composed before
/// the local fog, off the sky, and off entirely without an atmosphere.**
///
/// `crcbl::screenshot::aerial_forward` is the fixture: a floor to the horizon
/// in a dark half and a light half, mirrored about the camera's column under a
/// sun with no `x` in it, drawn at [`AERIAL_KM_PER_UNIT`] and — the control —
/// at zero kilometres per unit, which is the same sky, sun and ambient with no
/// air in front of anything. Five claims:
///
/// 1. **The far dark band gains** at least [`MIN_AERIAL_LEVELS`] over the
///    control, and both far bands are the host's prediction to
///    [`AERIAL_LEVELS`] — the control's own pixels composed with the host LUT
///    along their rays, which is a prediction rather than "looks hazier".
/// 2. **The near dark band moves less than a tenth of that.** A uniform term,
///    or one that read the distance wrong, passes the first and fails this.
/// 3. **The far contrast falls**: light minus dark is `(light − dark) · T` and
///    `T` is under one. An in-scatter composed without its transmittance
///    passes the first two and fails this.
/// 4. **The sky is byte-identical** between the two arms. `sky.slang` already
///    integrated the whole of a sky pixel's ray, and a composite that charged
///    the sky its air as well would fog it twice.
/// 5. **The off position**: the fixture with the fog effect and no atmosphere,
///    drawn by a renderer that has just been drawing under a thick atmosphere,
///    is byte-identical to the same frame from a renderer no atmosphere ever
///    touched — the aerial LUT still in its ring slots included.
///
/// **Shown red by sabotage** (2026-09-27, the AMD proprietary Vulkan driver on
/// an RX 7900 XTX), three times: the composite reading the air on sky pixels
/// too, at the LUT's far end, reported 18688 sky pixels differing (claim 4);
/// the composite adding the in-scatter without the transmittance missed the
/// host's prediction by 4.73 levels on the far dark band's red (claim 1); and
/// `crcbl_render::volumetric` writing the aerial switch on for a frame with no
/// atmosphere failed the off position (claim 5). Each restored and green.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_air_in_front_of_a_far_floor_is_the_host_aerial_lut() {
    crcbl_core::log::init_logging();
    let open = |atmosphere: Option<crcbl::render::Atmosphere>, fog: Option<crcbl::render::Fog>| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                aerial_forward(device, queue, format, atmosphere, fog).map(scene_referred)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the aerial scene: {why}"));
        Offscreen::guard(SUITE, setup)
    };
    let draw = |atmosphere, fog| {
        let mut setup = open(atmosphere, fog);
        let image = frame_of(&mut setup);
        setup.finish();
        image
    };

    let thick = aerial_sky(AERIAL_KM_PER_UNIT);
    let hazy = draw(Some(thick), None);
    let clear = draw(Some(aerial_sky(0.0)), None);
    let sky = SkyView::build(&thick.parameters());
    let air = sky.aerial();

    let far_dark = (DARK_COLUMN, FAR_ROW);
    let far_light = (mirror(DARK_COLUMN), FAR_ROW);
    let near_dark = (DARK_COLUMN, NEAR_ROW);

    // 1. The far bands gain, and gain what the host says.
    let mut worst = 0.0f32;
    let mut gain = [0.0f32; 3];
    for (channel, gained) in gain.iter_mut().enumerate() {
        *gained = block_channel(&hazy, far_dark, BAND, channel)
            - block_channel(&clear, far_dark, BAND, channel);
        for centre in [far_dark, far_light] {
            let measured = block_channel(&hazy, centre, BAND, channel);
            let predicted = predicted_band(&clear, air, centre, channel);
            let miss = (measured - predicted).abs();
            worst = worst.max(miss);
            assert!(
                miss <= AERIAL_LEVELS,
                "the far band at {centre:?} measures {measured:.2} on channel {channel} and the \
                 host's own AerialView over the control frame predicts {predicted:.2}, a miss of \
                 {miss:.2} level(s) against {AERIAL_LEVELS}"
            );
        }
        assert!(
            *gained >= MIN_AERIAL_LEVELS,
            "the far dark band gains {gained:.2} level(s) on channel {channel} over the control, \
             under {MIN_AERIAL_LEVELS}: the air did not reach the frame"
        );
    }

    // 2. The near band moves by a fraction of that, channel by channel.
    let mut near = [0.0f32; 3];
    for (channel, moved) in near.iter_mut().enumerate() {
        *moved = (block_channel(&hazy, near_dark, BAND, channel)
            - block_channel(&clear, near_dark, BAND, channel))
        .abs();
        assert!(
            *moved < 0.1 * gain[channel],
            "the near dark band moves {moved:.2} level(s) on channel {channel}, which is not \
             under a tenth of the far band's {:.2}: the air is not a function of the distance",
            gain[channel]
        );
    }

    // 3. The far contrast falls.
    let mut contrast = [0.0f32; 2];
    for (slot, image) in contrast.iter_mut().zip([&hazy, &clear]) {
        for channel in 0..3 {
            *slot += (block_channel(image, far_light, BAND, channel)
                - block_channel(image, far_dark, BAND, channel))
                / 3.0;
        }
    }
    let [hazy_contrast, clear_contrast] = contrast;
    assert!(
        hazy_contrast < clear_contrast - 1.0,
        "the far light-minus-dark contrast reads {hazy_contrast:.2} through the air and \
         {clear_contrast:.2} without it: the air adds light without taking any away"
    );

    // 4. The sky is the sky-view LUT's alone in both arms.
    let sky_differs = SKY_ROWS
        .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
        .filter(|&(x, y)| hazy.pixel(x, y) != clear.pixel(x, y))
        .count();
    assert_eq!(
        sky_differs, 0,
        "{sky_differs} sky pixel(s) differ between the thick air and none: the composite charged \
         the sky for air the sky-view LUT had already integrated"
    );

    // 5. The off position.
    let mut touched = open(Some(thick), Some(aerial_fog()));
    let mut under_air = frame_of(&mut touched);
    for _ in 1..FRAMES {
        under_air = frame_of(&mut touched);
    }
    assert!(
        touched.set_atmosphere(None),
        "the aerial scene draws through a forward renderer, so the removal has to reach one"
    );
    let mut after = frame_of(&mut touched);
    for _ in 1..FRAMES {
        after = frame_of(&mut touched);
    }
    touched.finish();
    let mut never = open(None, Some(aerial_fog()));
    let mut fogged = frame_of(&mut never);
    for _ in 1..FRAMES {
        fogged = frame_of(&mut never);
    }
    never.finish();
    let differing = |left: &Image, right: &Image| {
        (0..EXTENT.1)
            .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
            .filter(|&(x, y)| left.pixel(x, y) != right.pixel(x, y))
            .count()
    };
    let air_moved = differing(&under_air, &fogged);
    let removal = differing(&after, &fogged);
    assert!(
        air_moved > 0,
        "the fogged frame under the thick air is the fogged frame without it, so the equality \
         below would hold for a composite that composed no air at all"
    );
    assert_eq!(
        removal, 0,
        "the fogged floor with its atmosphere removed differs from one never given an \
         atmosphere at {removal} pixel(s): the air outlived the atmosphere"
    );

    eprintln!(
        "crcbl render e2e: aerial — the far dark band gains {gain:.2?} level(s) in red, green \
         and blue, the near one moves {near:.2?}; both far bands sit within {worst:.2} of the host's \
         AerialView; the far contrast falls from {clear_contrast:.2} to {hazy_contrast:.2}; \
         {sky_differs} sky pixel(s) differ; with fog, the air moved {air_moved} pixel(s) and its \
         removal left {removal}"
    );
}
