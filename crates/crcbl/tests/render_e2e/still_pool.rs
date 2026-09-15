//! [`Scene::StillPool`]: `docs/plan/55-water.md` rung 1's surface, held to its
//! golden and to four relations between bands of its own frame, and held to the
//! frame it draws with its body removed.
//!
//! A file of its own rather than more of `render_e2e.rs`, which is the largest
//! test in the crate; every helper it reads is that file's.

use crcbl::screenshot::{OffscreenSetup, Scene};
use crcbl_golden::Image;

use super::{EXTENT, Offscreen, SUITE, block_channel, channel_order};

/// The anti-vacuity colour count for [`Scene::StillPool`]: two basins, two
/// tones of floor, a post and its shadow, so a frame of one flat colour is far
/// under it.
const MIN_COLORS_STILL_POOL: usize = 64;

/// The half-extent of every band read here, in pixels.
const BAND: (u32, u32) = (4, 2);

/// Where the world point `point` lands in the frame, through the matrices
/// `crcbl_render::ForwardRenderer` draws the scene with.
fn pool_pixel(point: glam::Vec3) -> (u32, u32) {
    let aspect = EXTENT.0 as f32 / EXTENT.1 as f32;
    let clip = crcbl::screenshot::still_pool_camera().view_projection(aspect) * point.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let column = (ndc.x * 0.5 + 0.5) * EXTENT.0 as f32;
    let row = (0.5 - ndc.y * 0.5) * EXTENT.1 as f32;
    assert!(
        (0.0..EXTENT.0 as f32).contains(&column) && (0.0..EXTENT.1 as f32).contains(&row),
        "{point} lands at ({column}, {row}), outside the frame"
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "inside the frame, which the assertion above says"
    )]
    (column as u32, row as u32)
}

/// The band's mean on each channel, in code values.
fn band(image: &Image, centre: (u32, u32)) -> [f32; 3] {
    [0, 1, 2].map(|channel| block_channel(image, centre, BAND, channel))
}

/// The band centred on the water surface's point `(x, level, z)`.
fn surface_band(image: &Image, x: f32, z: f32) -> [f32; 3] {
    band(
        image,
        pool_pixel(glam::Vec3::new(x, crcbl::screenshot::STILL_POOL_LEVEL, z)),
    )
}

/// How far the shallow basin's green must sit above the deep basin's at the
/// mirrored point, in levels.
///
/// **Absorption, and nothing else can open it.** The two points are mirrored
/// about the camera's column, so the view angle, the Fresnel term, the sky they
/// reflect and the sun on the floor are equal; what differs is the water under
/// them. Measured at 28 levels on radv and on lavapipe (2026-09-15); with the
/// transmittance forced to one it is exactly zero on radv, because the frame's
/// two halves are then the same picture.
const DEEP_BELOW_SHALLOW_GREEN: f32 = 14.0;

/// How far the far band's red must sit above the near band's, in levels.
///
/// **The reflection, and it is the red channel because nothing else reaches
/// it.** The basin's tile has no red at all, so red over the basin is the light
/// the water adds: the sky it reflects and the little red it scatters. Toward
/// the far band the view grazes, the Fresnel term grows and so does the red. The
/// scattered red goes the other way — the far band's water is thinner, so it
/// scatters less — which is what makes the relation a claim about the reflection
/// rather than about the medium: with the Fresnel term forced to zero the far
/// band reads 9.25 levels *under* the near one (radv), where the frame reads
/// 4.5 over it (radv and lavapipe, 2026-09-15).
const GRAZING_ABOVE_HEAD_ON_RED: f32 = 2.0;

/// The most any channel of the shore band may differ from the dry sand beyond
/// it, in levels.
///
/// **The shoreline fade.** The shore step is four centimetres under the surface,
/// thinner than `crcbl_shaders::water::SHORE_FADE`, so the surface is drawn
/// mostly as the sand under it. Measured at 6 levels at worst on radv and on
/// lavapipe; with the fade switched off the grazing reflection over that same
/// sand opens it to 13.6 (radv, 2026-09-15).
const SHORE_FROM_DRY: f32 = 9.5;

/// How far under its green the water beyond the post's top must keep its red,
/// in levels.
///
/// **The refraction's in-front rejection.** Those pixels bend toward pixels the
/// post covers above the surface, and the post is the one red thing in the
/// frame: accepted, its colour is what the water shows. The water reads 85
/// levels more green than red on radv and on lavapipe; with the rejection
/// removed the same band reads 68 levels more *red* than green (radv,
/// 2026-09-15).
const ABOVE_POST_RED_UNDER_GREEN: f32 = 40.0;

/// [`Scene::StillPool`]'s claims, each a relation between bands of one frame
/// drawn under the clamp — `super::scene_referred`'s argument, since each
/// compares code values the ACES fit would mix across channels.
///
/// **Each was shown red by sabotage** (2026-09-15, radv): `water.slang` edited,
/// the artifacts recompiled, this run, and the file restored and recompiled.
/// The four edits and what each moved are on the four constants above.
fn the_pool_s_bands_relate_the_way_water_does(image: &Image) {
    use crcbl::screenshot::{
        STILL_POOL_FAR_EDGE, STILL_POOL_NEAR_EDGE, STILL_POOL_POST, STILL_POOL_SHORE_START,
    };
    let [red, green] = [0, 1];

    // --- deep against shallow, at mirrored points ---
    let row_z = STILL_POOL_NEAR_EDGE - 0.6;
    let deep = surface_band(image, -1.2, row_z);
    let shallow = surface_band(image, 1.2, row_z);

    // --- grazing against head-on, in the deep basin ---
    let near = surface_band(image, -1.0, STILL_POOL_NEAR_EDGE - 0.5);
    let far = surface_band(image, -1.0, STILL_POOL_SHORE_START + 0.6);

    // --- the shoreline against the dry sand beyond it ---
    let shore_z = 0.5 * (STILL_POOL_SHORE_START + STILL_POOL_FAR_EDGE);
    let shore = surface_band(image, 1.0, shore_z);
    let dry = band(
        image,
        pool_pixel(glam::Vec3::new(1.0, 0.0, STILL_POOL_FAR_EDGE - 2.5)),
    );

    // --- the water beyond the post's top, against the post ---
    //
    // Seven rows above the top edge is inside the band whose refracted landing
    // is on the post's dry half, for this camera; the post's own band is five
    // rows under the edge.
    let [post_left, post_right, post_z, post_top] = STILL_POOL_POST;
    let post_x = 0.5 * (post_left + post_right);
    let top = pool_pixel(glam::Vec3::new(post_x, post_top, post_z));
    let above = band(image, (top.0, top.1 - 7));
    let post = band(image, (top.0, top.1 + 5));

    eprintln!(
        "crcbl render e2e: still pool — deep {deep:?} shallow {shallow:?}; near {near:?} far \
         {far:?}; shore {shore:?} dry {dry:?}; above the post {above:?} post {post:?}"
    );

    // The premise the rejection claim rests on: the post is red and the water
    // is not, so a smear of one onto the other is a sign flip.
    assert!(
        post[red] > post[green] + ABOVE_POST_RED_UNDER_GREEN,
        "the post band reads {post:?}, which is not the red post — the band is not on it"
    );

    let opened = shallow[green] - deep[green];
    assert!(
        opened >= DEEP_BELOW_SHALLOW_GREEN,
        "the deep basin reads {deep:?} and the shallow one {shallow:?} at the mirrored point: \
         green is {opened:.2} level(s) apart against {DEEP_BELOW_SHALLOW_GREEN} — the water is \
         not absorbing with depth"
    );
    let grazing = far[red] - near[red];
    assert!(
        grazing >= GRAZING_ABOVE_HEAD_ON_RED,
        "the far band reads {far:?} and the near one {near:?}: red rises by {grazing:.2} \
         level(s) toward grazing against {GRAZING_ABOVE_HEAD_ON_RED} — the surface is not \
         reflecting more of the sky where it should"
    );
    let shoreline = (0..3)
        .map(|channel| (shore[channel] - dry[channel]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        shoreline <= SHORE_FROM_DRY,
        "the shore band reads {shore:?} and the dry sand {dry:?}: {shoreline:.2} level(s) apart \
         against {SHORE_FROM_DRY} — the thin water at the shore is not fading into the sand"
    );
    assert!(
        above[red] + ABOVE_POST_RED_UNDER_GREEN < above[green],
        "the water beyond the post's top reads {above:?}: its red is not {ABOVE_POST_RED_UNDER_GREEN} \
         level(s) under its green — the refraction is reading the post in front of the surface"
    );
}

/// [`Scene::StillPool`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_still_pool_scene_draws_its_water_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden_measuring(
        Scene::StillPool,
        "still_pool",
        EXTENT,
        MIN_COLORS_STILL_POOL,
        the_pool_s_bands_relate_the_way_water_does,
        super::ClaimFrame::ASecondOneUnderTheClamp,
    );
}

/// [`Scene::StillPool`] on both geometry paths this machine can reach — see
/// `super::draw_scene_on_every_geometry_path`.
///
/// The water passes read no geometry path; what the comparison holds is the
/// frame they draw over, which does. The floors and the post are the plate
/// turned upright, and a mesh path that culled a turned cluster by its authored
/// cone would lose the walls the refraction reads.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_still_pool_scene_draws_the_same_frame_on_every_geometry_path() {
    super::draw_scene_on_every_geometry_path_measuring(
        Scene::StillPool,
        "still_pool",
        MIN_COLORS_STILL_POOL,
        the_pool_s_bands_relate_the_way_water_does,
        super::ClaimFrame::ASecondOneUnderTheClamp,
    );
}

/// One frame of `setup`, as an image.
fn frame_of(setup: &mut OffscreenSetup) -> Image {
    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image")
}

/// How many frames each renderer draws before the one compared: enough that
/// every slot of the ring has come round after the body was removed, so a slot
/// still holding the pool's buffers would be read.
const FRAMES_AFTER_REMOVAL: usize = 4;

/// **A still pool with its body removed is the pool never given one, bit for
/// bit.**
///
/// Water is content, not an effect bit, on the sky pass's terms: a renderer with
/// no body records no water pass and takes no transient. What that has to mean
/// is a frame identical to one from a renderer water never touched — including
/// after the body has been drawn and taken away, which is the case the ring of
/// per-slot buffers could get wrong.
///
/// The anti-vacuity half: the frame **with** the body differs from both, or the
/// equality would hold for a pass that drew nothing.
///
/// **Shown red by sabotage** (2026-09-15, radv): `WaterBodies::set` made to
/// return early on an empty slice, so the removal left the pool drawing. This
/// reported 24874 pixels differing after the removal — every pixel the body
/// had changed.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_still_pool_with_its_body_removed_is_the_pool_never_given_one() {
    crcbl_core::log::init_logging();
    let open = |bodies: Vec<crcbl::render::WaterBody>| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::still_pool_forward(device, queue, format, &bodies)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the still pool: {why}"));
        Offscreen::guard(SUITE, setup)
    };

    let mut removed = open(vec![crcbl::screenshot::still_pool_body()]);
    let with_water = frame_of(&mut removed);
    assert!(
        removed.set_water(&[]).expect("an empty set is a set"),
        "the still pool draws through a forward renderer, so the removal has to reach one"
    );
    let mut after = frame_of(&mut removed);
    for _ in 1..FRAMES_AFTER_REMOVAL {
        after = frame_of(&mut removed);
    }
    removed.finish();

    let mut never = open(Vec::new());
    let mut without = frame_of(&mut never);
    for _ in 1..=FRAMES_AFTER_REMOVAL {
        without = frame_of(&mut never);
    }
    never.finish();

    let differing = |left: &Image, right: &Image| {
        (0..EXTENT.1)
            .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
            .filter(|&(x, y)| left.pixel(x, y) != right.pixel(x, y))
            .count()
    };
    let removal = differing(&after, &without);
    let water = differing(&with_water, &without);
    eprintln!(
        "crcbl render e2e: still pool — {water} pixel(s) differ with the body in, {removal} \
         after it was removed"
    );
    assert!(
        water > 0,
        "the frame with the body in is the frame without one, so the equality below would hold \
         for a water pass that drew nothing"
    );
    assert_eq!(
        removal, 0,
        "the pool with its body removed differs from the pool never given one at {removal} \
         pixel(s): removing water left something of it in the frame"
    );
}
