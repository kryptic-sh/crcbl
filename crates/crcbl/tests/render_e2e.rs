//! The render layer, on whichever backend the registry opens — one frame of
//! every [`Scene`] through the engine's own renderers, read back and compared
//! against a checked-in golden.
//!
//! # Why this exists, and why here
//!
//! `docs/backlog.md`'s "The render layer has only ever run on Vulkan and wgpu"
//! is the gap: the frame graph, the cull pass, draw generation, forward and
//! tonemap execute on `crcbl-vk` (`crates/crcbl-vk/tests/vk_e2e/mesh.rs`) and on
//! native wgpu, and on nothing else. `crcbl-mtl`'s own suite proves the *HAL* —
//! dispatch, encoders, bindings, copies — and has never constructed a
//! [`ForwardRenderer`]. A green `mtl e2e` is therefore not evidence about the
//! renderer.
//!
//! This test is in `crcbl` rather than in `crcbl-mtl` because `crcbl` is the
//! only crate that depends on the renderer *and* on the backends: `crcbl-render`
//! is above the seam and names no backend, `crcbl-mtl` is below it and names no
//! renderer. A dev-dependency from `crcbl-mtl` on `crcbl-render` would be
//! acyclic — `crcbl-render`'s manifest takes `crcbl-hal` and nothing below it —
//! but it would have to rebuild the offscreen surface, swapchain, readback and
//! row-unpadding that [`crate::screenshot`](crcbl::screenshot) already owns and
//! that `tests/run-cross-backend-e2e.sh` already drives, and it could not assert
//! the thing a Metal run most needs asserted: that the *registry* picked Metal.
//!
//! # One test per scene, every backend
//!
//! [`OffscreenSetup::open`] opens whatever [`crcbl::backend::open`] selects, so
//! `CRCBL_GPU` decides which backend draws and this file is the same set of
//! tests on all of them. That is deliberate rather than incidental:
//!
//! * The golden is only trustworthy while something keeps re-deriving it, and
//!   the Metal arm cannot (see `run-render-e2e.sh` and the `mtl-e2e` job). The
//!   Vulkan arm is what stops it rotting.
//! * `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5 — a shader
//!   compiles to all four targets and *means something different on each* — has
//!   already cost this repo two real bugs (`SV_InstanceID`, `SV_VertexID`), and
//!   both were caught only by rendering one scene through two targets. MSL is a
//!   target nothing has ever crossed. Comparing Metal's frame against a
//!   Vulkan-blessed reference is the same detector pointed at the third target.
//!
//! # Why all three scenes, and why one test each
//!
//! `docs/backlog.md`'s "Decided: the four-backend compare is more scenes in
//! `render_e2e`, not a new job". This file used to draw [`Scene::Cube`] alone,
//! which exercises `mesh.slang` and `tonemap.slang`; `sprite.slang` and
//! `ui.slang` were compared across targets only by
//! `tests/run-cross-backend-e2e.sh`, which runs Vulkan against wgpu. Those two
//! shaders are precisely the ones that have diverged per target here —
//! `SV_InstanceID` in the sprite pass, `SV_VertexID` in the mesh pool — so MSL
//! and DXIL had no comparison at all for the code with the actual history.
//!
//! One `#[test]` per scene rather than one that loops, because the name is the
//! failure report: a runner nobody can attach a debugger to prints the test
//! name and a diff, and `the_sprite_scene_…` has already said which shader
//! broke before anyone reads the numbers. It also means `-E 'test(sprite)'`
//! reruns the one scene under investigation instead of three device opens.

#![cfg(feature = "render-e2e")]

use crcbl::adapter::{ADAPTER_ENV_VAR, device_type_from_name};
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::hal::{Features, Format, GeometryPath};
use crcbl::screenshot::{OffscreenSetup, Scene};
use crcbl_golden::{ChannelOrder, Golden, Image};

/// The size the goldens were blessed at.
///
/// The same 256x192 the cross-backend harness and `crcbl-vk`'s mesh suite use,
/// and for the reason those state: the structural metric averages over 8x8
/// blocks, and a smaller frame gives it too few of them to mean anything.
const EXTENT: (u32, u32) = (256, 192);

/// The anti-vacuity floor for [`Scene::Cube`]: distinct RGBA colours the frame
/// must contain.
///
/// Two blank frames compare perfectly, so a tolerance alone cannot tell "the
/// same picture" from "no picture". Measured by `run-cross-backend-e2e.sh` on
/// both ICDs at both of its sizes: the cube scene has 44-49 distinct colours and
/// a cleared frame has one. This floor is that harness's own
/// `CRCBL_CROSS_MIN_COLORS_CUBE`, so losing the cube, the pyramid or the
/// tonemap trips it.
const MIN_COLORS_CUBE: usize = 16;

/// The same, for [`Scene::Dunes`]: distinct RGBA colours that frame must hold.
///
/// A lit height field against a flat clear, so the count is dominated by shading
/// rather than by how many objects drew, and it runs into the hundreds — the
/// narrower strip
/// [`the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone`] measures
/// prints its own figure on every run. The floor sits far below that because
/// what it has to separate is a lit surface from the clear alone and from one
/// flat quad, both of which are a handful of colours; the golden is what holds
/// the picture itself.
///
/// [`the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone`]: fn@the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone
const MIN_COLORS_DUNES: usize = 32;

/// The same, for [`Scene::Sprite`]: `CRCBL_CROSS_MIN_COLORS_SPRITE`.
///
/// That harness measured 17-24 distinct colours for this scene on both ICDs at
/// both sizes, and its floor sits just under the minimum — so losing one of the
/// three batches trips it.
const MIN_COLORS_SPRITE: usize = 16;

/// The same, for [`Scene::Lights`].
///
/// Higher than [`MIN_COLORS_CUBE`] and that is the scene's whole claim: three
/// coloured point lights falling off across the same geometry produce a
/// gradient where a single directional light produced flat faces, so a frame
/// that lost the light list would fail this floor before any golden was
/// consulted.
const MIN_COLORS_LIGHTS: usize = 64;

/// The same, for [`Scene::Spot`], and it is deliberately **not** set as high as
/// this frame will go.
///
/// The frame is one flat floor with one cone on it, so a cone contributing
/// nothing leaves a uniformly lit plane: measured at 37 distinct colours on
/// lavapipe, against a correct frame that runs past every floor here. This one
/// separates those two and stops there, because the *shape* of the cone is
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]'s claim and
/// it says far more about a wrong one than a colour count can. A cone stepped
/// rather than ramped measures 60 — over this floor, and refused a few lines
/// later with a message that names what is actually wrong with it.
///
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]: fn@the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor
const MIN_COLORS_SPOT: usize = 48;

/// The same, for [`Scene::Ui`]: `CRCBL_CROSS_MIN_COLORS_UI`.
///
/// The UI frame is the least colourful of the three — the clear, the panel, the
/// translucent bar over each of those, the outline and two text colours, which
/// that harness measured as 7 on both ICDs at both sizes. A floor of 16 would
/// fail a correct frame; this one fails a frame that lost an element.
const MIN_COLORS_UI: usize = 6;

/// How far apart a channel must be before two pixels count as different colours
/// rather than the same one off two rasterisers.
///
/// The comparison below runs at `crcbl_golden::Tolerance::RASTERISER`, whose
/// `max_channel_delta` is the largest per-channel difference two rasterisers are
/// allowed to have on the *same* picture. This threshold is several times that,
/// because the question it answers is "did anything draw here", not "is this the
/// right colour" — the golden is what answers the second one.
const PAINTED_DELTA: u8 = 8;

/// How far out from the frame's centre a [`Scene::Spot`] profile runs, in
/// pixels.
///
/// Half the frame's short axis, which is the furthest a straight line from the
/// centre stays inside it — and `crcbl::screenshot`'s `SPOT_CAMERA_UP` is chosen
/// so the cone has closed well before the end of it.
const SPOT_PROFILE_LENGTH: u32 = EXTENT.1 / 2;

/// How many samples of a [`Scene::Spot`] profile the floor outside the cone is
/// averaged over.
///
/// The last of them, which is the furthest from the cone. Averaged rather than
/// read off one pixel because it is the denominator of every claim below, and a
/// single pixel of driver dither in it moves all of them.
const SPOT_FLOOR_SAMPLES: usize = 16;

/// How far a [`Scene::Spot`] profile may rise before it counts as rising rather
/// than as two rasterisers disagreeing, in summed RGB.
///
/// Several times `crcbl_golden::Tolerance::RASTERISER`'s per-channel allowance,
/// summed over three channels — the question here is whether the pool is
/// brightest on its axis, not whether two drivers agree about a value.
const SPOT_PROFILE_NOISE: u32 = 24;

/// How many samples a [`Scene::Spot`] profile must have strictly inside the
/// penumbra band before the cone counts as a ramp rather than a step.
///
/// **This is the number the whole scene exists to produce**, so it is worth
/// saying what it is not: it is not "the pool has a gradient". A step-function
/// cone draws a pool with a gradient too, because distance falloff and Lambert
/// vary across it whatever the cone does. What a step cannot do is put pixels
/// *between* the lit floor and the dark floor, and `SPOT_HEIGHT` is chosen so
/// those other terms move the lit floor by only a few per cent — well inside the
/// band's own edges. Measured at 20 or more per direction on lavapipe and on
/// radv; this floor is under that and far above the nothing a step produces.
const SPOT_PENUMBRA_MIN: usize = 12;

/// How far the four directions' penumbra widths may differ, in samples.
///
/// The pool is a circle about the frame's centre — see `spot_camera` — so the
/// four are measuring one number four ways, and they differ only in where the
/// pixel grid falls across the ramp. A pool that is not round, or not centred,
/// fails this rather than any single direction's own claims.
const SPOT_ROUNDNESS: usize = 3;

/// The anti-vacuity floor for [`Scene::SpotShadow`].
///
/// A cone on a floor with a lit pyramid in it and a shadow behind it, so the
/// count runs well past [`MIN_COLORS_SPOT`]'s frame — this floor is the same
/// number for the same reason: it separates "the cone contributed nothing" from
/// a working frame and stops there, because *where* the dark region is is
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]'s claim.
///
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]: fn@the_caster_darkens_the_floor_behind_it_and_not_beside_it
const MIN_COLORS_SPOT_SHADOW: usize = 48;

/// The half-extent of each band [`Scene::SpotShadow`] is measured over, in
/// pixels.
///
/// Small enough that the band stays inside the region it names — the shadow is
/// about 19 pixels wide where it is sampled, and the lit floor beside it is
/// bounded by the pool's own edge — and large enough that PCF's several-texel
/// gradient and a driver's dither average out.
const SPOT_SHADOW_BAND: (u32, u32) = (6, 6);

/// Where [`Scene::SpotShadow`]'s shadow falls, in pixels.
///
/// `crcbl::screenshot`'s camera looks straight down from `SPOT_SHADOW_CAMERA_UP`
/// with `+Z` at the top of the frame, and the light is at
/// `SPOT_SHADOW_LIGHT_AT` — up and towards `+Z` — so the shadow falls towards
/// `-Z`, which is *down* the frame. The caster's own image ends about 26 pixels
/// below the centre and its shadow reaches about 92; this sits between the two,
/// on the frame's vertical axis where the shadow is widest.
const SPOT_SHADOW_DARK_AT: (u32, u32) = (EXTENT.0 / 2, EXTENT.1 / 2 + 42);

/// Its mirror image across the cone's axis: the same distance from the frame's
/// centre, on the side the light comes from.
///
/// **The same distance is the point.** Everything that varies over the pool —
/// the falloff, the cone's own ramp, Lambert — is a function of distance from
/// the cone's axis, so a band at the same distance has every one of those terms
/// at the same value and differs from the dark band in exactly one thing.
const SPOT_SHADOW_LIT_AT: (u32, u32) = (EXTENT.0 / 2, EXTENT.1 / 2 - 42);

/// And beside the caster: level with the shadow, across the frame, still inside
/// the pool.
///
/// What this refuses that the mirror does not is a frame that is simply darker
/// towards the bottom — a vignette, a wrong normal on the floor, a cone aimed a
/// little short. Each of those darkens a whole row and this band is in the same
/// row as the shadow.
const SPOT_SHADOW_SIDE_AT: (u32, u32) = (EXTENT.0 / 2 + 36, EXTENT.1 / 2 + 42);

/// How much brighter the lit bands must be than the shadowed one.
///
/// A ratio rather than a difference, for the reason `crcbl-vk`'s shadow suite
/// gives: what survives Lambert, the falloff, the cone and the tonemap is which
/// side leads, and by how much in proportion. Well above the few per cent the
/// other shading terms move across the pool and far below the several times a
/// real shadow produces.
const SPOT_SHADOW_RATIO: f32 = 1.5;

/// How far above the clear the lit half of the pool must measure.
///
/// The anti-vacuity half of the two ratios above: a frame that drew nothing has
/// every band equal to the clear, and two ratios between equal numbers say
/// nothing at all.
const SPOT_SHADOW_LIT_FLOOR: f32 = 10.0;

/// What channel order [`OffscreenSetup::draw_and_readback`]'s bytes are in.
///
/// The same three lines as `crcbl-cli`'s `screenshot::channel_order` and
/// `vk_e2e/mesh.rs`'s, and not shared with either: `crcbl-golden` is a
/// **dev**-dependency here precisely so `png` reaches no shipped binary, so
/// `crcbl::screenshot` cannot name [`ChannelOrder`] to hand one out.
fn channel_order(format: Format) -> ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    }
}

/// Whether two pixels differ by more than driver noise, in RGB.
///
/// Alpha is ignored: the readback's alpha is the attachment's rather than
/// anything the scene chose, and both sprite and UI content is opaque by the
/// time it reaches the target.
fn differ(pixel: [u8; 4], other: [u8; 4]) -> bool {
    pixel[..3]
        .iter()
        .zip(&other[..3])
        .any(|(left, right)| left.abs_diff(*right) >= PAINTED_DELTA)
}

/// A lit cube and a pyramid, drawn by the engine's own renderer on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`draw_scene_and_match_its_golden`] for what each assertion group is for.
/// This scene's anti-vacuity claim is [`the_cube_is_lit_against_an_unpainted_corner`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_through_the_forward_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Cube,
        "cube",
        MIN_COLORS_CUBE,
        the_cube_scene_drew_its_geometry_and_both_material_columns,
    );
}

/// [`Scene::Cube`]'s claims, in the order a failure would hit them: something
/// drew, and each of the material row's two columns did something visible.
fn the_cube_scene_drew_its_geometry_and_both_material_columns(image: &Image) {
    the_cube_is_lit_against_an_unpainted_corner(image);
    the_textured_pyramid_is_quartered_and_the_plain_one_is_flat(image);
}

/// The dunes patch — `docs/plan/25-lod.md`'s cluster DAG — on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// **This is the scene that says a device with no amplification stage can draw
/// a DAG at all.** Every other resident is a flat mesh, so
/// [`Scene::Cube`]'s frame is identical whether level selection works, picks
/// nothing, or picks a level nothing then draws. Here every triangle arrives
/// through a level `draw_gen.slang` chose per instance and a bucket the CPU
/// recorded a draw for, so a selection that chose a bucket with no geometry in
/// it is a black frame rather than a passing one.
///
/// On a device that reports both a mesh stage and an amplification stage the
/// same frame comes out of the per-cluster descent instead, which is what makes
/// this a comparison between the two granularities rather than a test of one:
/// the levels differ across the surface and the silhouette does not, so the
/// golden holds for both. `crcbl-vk`'s
/// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is` is where
/// that claim is made in numbers instead of in pixels.
/// `docs/plan/18-render-features.md`'s light list, drawn.
///
/// The same geometry as the cube scene under three coloured point lights and a
/// sun turned right down, so the two goldens differ in the light list alone.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_lights_scene_draws_its_point_lights_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Lights,
        "lights",
        MIN_COLORS_LIGHTS,
        each_point_light_pools_where_it_was_put_and_nowhere_else,
    );
}

/// The three pools are where the three lights are, and the background is not
/// one of them.
///
/// **Where**, not merely *whether*: a fragment stage that ignored the froxel
/// grid and summed the whole list would light every pyramid in every colour,
/// which reads as a plausible picture and passes any "is it brighter" check.
/// Each quadrant is therefore asserted to lead in *its own* channel, which no
/// single dominant light can satisfy for all three at once.
///
/// The brightest pixel of each quadrant rather than a fixed coordinate: which
/// pixel a pyramid's lit face lands on is a rasteriser's business, and the
/// brightest one in a band that holds a pyramid and the clear behind it is on
/// the pyramid. The luminance floor is what says so — the clear is dark and
/// blue-dominant, so "leads blue" alone would be satisfiable by the background
/// and this is what refuses that.
fn each_point_light_pools_where_it_was_put_and_nowhere_else(image: &Image) {
    let clear = image.pixel(1, 1).expect("inside");
    let clear_peak = u32::from(*clear.iter().take(3).max().expect("three channels"));

    // `PYRAMID_BAND` is the left column; its mirror is the right one. The
    // pyramids sit above and below the cube's row, so the halves split them.
    let half = EXTENT.1 / 2;
    let right = (EXTENT.0 - PYRAMID_BAND.end)..EXTENT.0;
    // The order `scene_lights` places them: red over the plain pyramid, blue over
    // the blue-tinted one, green over the textured one. Which colour goes where
    // is the material's doing as much as the light's — see that function.
    let quadrants = [
        ("red", 0usize, PYRAMID_BAND, 0..half),
        ("blue", 2, right, 0..half),
        ("green", 1, PYRAMID_BAND, half..EXTENT.1),
    ];
    for (name, channel, columns, rows) in quadrants {
        let mut brightest = clear;
        let mut peak = 0u32;
        for row in rows {
            for column in columns.clone() {
                let Some(pixel) = image.pixel(column, row) else {
                    continue;
                };
                let luma: u32 = pixel.iter().take(3).map(|c| u32::from(*c)).sum();
                if luma > peak {
                    peak = luma;
                    brightest = pixel;
                }
            }
        }
        eprintln!("crcbl render e2e: lights — {name} quadrant peaks at {brightest:?}");
        assert!(
            u32::from(brightest[channel]) > clear_peak * 2,
            "the {name} quadrant's brightest pixel must be lit geometry rather than \
             the clear behind it: {brightest:?} against a clear of {clear:?}"
        );
        for other in 0..3usize {
            if other == channel {
                continue;
            }
            assert!(
                brightest[channel] > brightest[other],
                "the {name} light must lead its own channel where it was put, got {brightest:?}"
            );
        }
    }
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_lights_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Lights,
        "lights",
        MIN_COLORS_LIGHTS,
        each_point_light_pools_where_it_was_put_and_nowhere_else,
    );
}

/// `docs/plan/18-render-features.md`'s **spot** light, drawn — the one light
/// kind in the list that had no rendered pixel anywhere in the tree.
///
/// `crcbl_render::Light::row`'s unit tests already pin the conversion, including
/// the clamp that widens a caller's angles rather than inverting them, and
/// `mesh.slang`'s `spot_cone` already compiled to all four targets. None of that
/// is evidence about a cone: an inverted test, a swapped pair of angles and a
/// step where the ramp should be each compile, each read plausibly, and each
/// draws a picture. [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]
/// is what tells them apart.
///
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]: fn@the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_scene_draws_its_cone_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Spot,
        "spot",
        MIN_COLORS_SPOT,
        the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Spot,
        "spot",
        MIN_COLORS_SPOT,
        the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor,
    );
}

/// The brightness of every pixel from the frame's centre outward along `step`,
/// summed over RGB.
///
/// One sample per pixel out to [`SPOT_PROFILE_LENGTH`], which is the furthest a
/// straight line from the centre of a `256x192` frame stays inside it — so the
/// four axis directions all fit and all sample the same radii.
fn spot_profile(image: &Image, step: (i32, i32)) -> Vec<u32> {
    let centre = (
        i32::try_from(EXTENT.0 / 2).expect("a frame edge fits in an i32"),
        i32::try_from(EXTENT.1 / 2).expect("a frame edge fits in an i32"),
    );
    (0..i32::try_from(SPOT_PROFILE_LENGTH).expect("a frame edge fits in an i32"))
        .map(|distance| {
            let x = centre.0 + step.0 * distance;
            let y = centre.1 + step.1 * distance;
            let pixel = image
                .pixel(
                    u32::try_from(x).expect("the profile stays inside the frame"),
                    u32::try_from(y).expect("the profile stays inside the frame"),
                )
                .expect("the profile stays inside the frame");
            pixel
                .iter()
                .take(3)
                .map(|channel| u32::from(*channel))
                .sum()
        })
        .collect()
}

/// [`Scene::Spot`]'s claim, and it is three claims: the cone has a lit core, the
/// floor outside it is dark, and **the band between them varies**.
///
/// The third is what the scene exists for, and the first two are what make it
/// mean something. Take them in the order a wrong cone hits them:
///
/// * A cone test with its sign inverted lights everything *except* the cone, so
///   the axis is dark and the far floor is bright: the core-against-floor ratio
///   goes the wrong way and fails first.
/// * A cone whose two angles arrive the wrong way round divides by a clamped
///   epsilon and comes out a step at the inner angle. That draws a lit disc
///   against a dark floor — the first two claims pass on it unchanged — and it
///   is the penumbra count that refuses it.
/// * So does a `spot_cone` written as a boolean in the first place, which is the
///   same picture by a different route.
///
/// **The penumbra count is not "the pool has a gradient".** A step-function cone
/// leaves a gradient across the pool too, because the distance falloff and
/// Lambert vary over it whatever the cone does. What a step cannot do is put
/// pixels *between* the lit floor and the dark floor, and `SPOT_HEIGHT` is
/// chosen so those other two terms move the lit floor by a few per cent — far
/// inside the band this counts, whose edges sit a fifth of the way in from each
/// end. See [`SPOT_PENUMBRA_MIN`].
///
/// Four directions rather than one, because the pool is a circle about the
/// frame's centre and four profiles of one circle are a claim about its
/// *shape*: [`SPOT_ROUNDNESS`] is what a pool that is off-centre, elongated or
/// aimed somewhere else fails, and no single profile can see any of those.
fn the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor(image: &Image) {
    let mut penumbras = Vec::new();
    for (name, step) in [
        ("right", (1i32, 0i32)),
        ("left", (-1, 0)),
        ("down", (0, 1)),
        ("up", (0, -1)),
    ] {
        let profile = spot_profile(image, step);
        let core = profile[0];
        let tail = &profile[profile.len() - SPOT_FLOOR_SAMPLES..];
        let floor = tail.iter().sum::<u32>() / SPOT_FLOOR_SAMPLES as u32;

        // The floor outside the cone has to be *lit* floor rather than black, or
        // "dark outside the cone" is a claim about an unpainted frame. The sun
        // is turned right down and is what lights it — see `dim_sun`.
        assert!(
            floor > 0,
            "the floor outside the cone is black, so the sun contributed nothing and \
             the {name} profile is not a measurement of a cone against a lit surface"
        );
        assert!(
            core >= floor * 3,
            "the cone's core must be unmistakably brighter than the floor outside it: \
             the {name} profile runs {core} at the axis against a floor of {floor}"
        );
        let peak = *profile.iter().max().expect("the profile is not empty");
        assert!(
            core + SPOT_PROFILE_NOISE >= peak,
            "the pool must be brightest on the cone's axis: the {name} profile peaks at \
             {peak} against {core} at the axis"
        );

        // The band, a fifth of the way in from each end so neither edge of it can
        // be reached by the few per cent the other shading terms move across the
        // pool. Its own edges are derived from this frame's two ends rather than
        // written down, because the tonemap is what decides where a linear
        // radiance lands and it is not this test's business to model.
        let span = core - floor;
        let low = floor + span / 5;
        let high = floor + span * 4 / 5;
        let penumbra = profile
            .iter()
            .filter(|value| **value > low && **value < high)
            .count();
        eprintln!(
            "crcbl render e2e: spot — {name} runs {core} at the axis to {floor} at the edge, \
             with {penumbra} sample(s) between {low} and {high}"
        );
        assert!(
            penumbra >= SPOT_PENUMBRA_MIN,
            "the cone's penumbra holds {penumbra} sample(s) between {low} and {high} along \
             {name}, which is a step rather than a ramp — a boolean cone draws the same lit \
             disc on the same dark floor and differs from a working one only here"
        );
        penumbras.push((name, penumbra));
    }

    let widest = penumbras
        .iter()
        .map(|(_, count)| *count)
        .max()
        .expect("four");
    let narrowest = penumbras
        .iter()
        .map(|(_, count)| *count)
        .min()
        .expect("four");
    assert!(
        widest - narrowest <= SPOT_ROUNDNESS,
        "the four directions' penumbras are {penumbras:?}, which is not one circle about the \
         frame's centre — the cone is off-axis, elongated, or aimed somewhere this camera \
         does not see square on"
    );
}

/// `docs/plan/18-render-features.md`'s **shadowed spot**, drawn — the first
/// light in this engine other than the sun that occludes.
///
/// The golden is not the evidence and cannot be: a spot whose shadow lookup
/// always returned "lit" draws [`Scene::SpotShadow`] as an evenly lit pool with a
/// pyramid in it, which is a perfectly ordinary picture, and one that always
/// returned "shadowed" draws a pool that is uniformly dim. Both would be blessed
/// without comment. [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]
/// is what tells the three apart, and `crcbl-vk`'s
/// `a_spots_shadow_follows_its_caster` is what says the dark region is this
/// caster's rather than a fixed patch.
///
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]: fn@the_caster_darkens_the_floor_behind_it_and_not_beside_it
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_shadow_scene_draws_its_shadow_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::SpotShadow,
        "spot_shadow",
        MIN_COLORS_SPOT_SHADOW,
        the_caster_darkens_the_floor_behind_it_and_not_beside_it,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_shadow_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::SpotShadow,
        "spot_shadow",
        MIN_COLORS_SPOT_SHADOW,
        the_caster_darkens_the_floor_behind_it_and_not_beside_it,
    );
}

/// The mean brightness of a `width` by `height` block of the frame centred on
/// `(x, y)`, summed over RGB.
///
/// A block rather than a pixel, for the reason `crcbl-vk`'s shadow suite gives:
/// PCF makes a shadow edge a gradient several texels wide, and a single sample
/// lands wherever that gradient happens to fall.
fn block_brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in centre.1.saturating_sub(half.1)..(centre.1 + half.1).min(EXTENT.1) {
        for x in centre.0.saturating_sub(half.0)..(centre.0 + half.0).min(EXTENT.0) {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty block measures nothing");
    total / (count as f32 * 3.0)
}

/// [`Scene::SpotShadow`]'s claim: **the floor is dark where the caster blocks
/// the light and lit where it does not**.
///
/// Three bands of the same frame, all inside the cone's pool and all on the
/// floor, and the claim is the relation between them rather than any absolute
/// colour — which is what survives Lambert, the falloff, the cone and the
/// tonemap:
///
/// * `SPOT_SHADOW_DARK_AT` is the floor just behind the caster, where the light
///   comes from `+Z` at 45° and the pyramid stands between. It must be the
///   darkest of the three.
/// * `SPOT_SHADOW_LIT_AT` is its mirror image on the far side, the same distance
///   from the frame's centre and the same distance from the cone's axis. It must
///   be unmistakably brighter.
/// * `SPOT_SHADOW_SIDE_AT` is beside the caster, level with the shadow but
///   across the frame, where nothing is between the light and the floor.
///
/// **The mirror is what makes it evidence.** A shadow lookup that always returned
/// "shadowed" darkens all three equally and fails the ratio; one that always
/// returned "lit" leaves all three equal and fails it the other way. A vignette,
/// a wrong normal or a cone aimed slightly off darkens one *side* of the frame,
/// which the third band refuses — it is level with the dark band and equally far
/// from the cone's axis, so nothing that varies with position alone can separate
/// the two.
fn the_caster_darkens_the_floor_behind_it_and_not_beside_it(image: &Image) {
    let dark = block_brightness(image, SPOT_SHADOW_DARK_AT, SPOT_SHADOW_BAND);
    let lit = block_brightness(image, SPOT_SHADOW_LIT_AT, SPOT_SHADOW_BAND);
    let side = block_brightness(image, SPOT_SHADOW_SIDE_AT, SPOT_SHADOW_BAND);
    eprintln!(
        "crcbl render e2e: spot shadow — behind the caster {dark:.1}, mirrored {lit:.1}, \
         beside it {side:.1}"
    );
    assert!(
        dark * SPOT_SHADOW_RATIO < lit,
        "the floor behind the caster must be unmistakably darker than its mirror across the \
         pool: {dark:.1} against {lit:.1}, which is not a shadow"
    );
    assert!(
        dark * SPOT_SHADOW_RATIO < side,
        "and darker than the floor beside it at the same distance from the cone's axis: \
         {dark:.1} against {side:.1} — a frame that is merely dim on one side satisfies the \
         first claim and not this one"
    );
    // The lit bands are lit *floor*, not a black frame the shadow is invisible
    // against. Without this the two ratios above are satisfiable by a scene that
    // drew nothing at all.
    let clear = block_brightness(image, (2, 2), (2, 2));
    assert!(
        lit > clear + SPOT_SHADOW_LIT_FLOOR,
        "the lit half of the pool measures {lit:.1} against a clear of {clear:.1}, so there \
         is no lit floor here for a shadow to be a shadow against"
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_dunes_scene_draws_its_cluster_dag_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Dunes,
        "dunes",
        MIN_COLORS_DUNES,
        the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone,
    );
}

/// [`Scene::Dunes`]'s anti-vacuity claim: lit ground below the horizon, clear
/// sky above it, and shading that varies down the patch.
///
/// The three together are what a frame drawn from an empty bucket fails. A
/// selection that resolved to a level with no geometry leaves the whole frame at
/// the clear, which the first two catch; one that drew a single flat quad — or
/// the patch collapsed to its coarsest level with the camera on top of it —
/// passes those and fails the third, because the dunes are a height field and a
/// lit height field is not one colour.
fn the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone(image: &Image) {
    // The camera looks along the patch from four units up, so the horizon sits
    // near the middle of the frame: a row a fifth of the way down is sky and one
    // four fifths down is ground, whichever level was selected.
    let sky = image.pixel(EXTENT.0 / 2, EXTENT.1 / 5).expect("inside");
    let ground = image.pixel(EXTENT.0 / 2, EXTENT.1 * 4 / 5).expect("inside");
    let clear = image.pixel(1, 1).expect("inside");
    assert!(
        !differ(sky, clear),
        "the sky at the top of the frame is {sky:?} against a clear of {clear:?} — the \
         patch is not where this camera puts it"
    );
    assert!(
        differ(ground, clear),
        "the ground at the bottom of the frame is still the clear {clear:?} (got \
         {ground:?}) — the selected level drew nothing"
    );
    // Down the middle of the patch, below the horizon. A height field lit by one
    // directional light shades every slope differently; a flat quad does not.
    let shades = distinct_colors_in(
        image,
        EXTENT.0 / 2 - 8..EXTENT.0 / 2 + 8,
        EXTENT.1 * 3 / 5..EXTENT.1,
    );
    eprintln!("crcbl render e2e: dunes — {shades} shade(s) down the patch's middle");
    assert!(
        shades >= 8,
        "the patch's middle holds {shades} distinct colour(s), which is a flat surface \
         rather than a lit height field"
    );
}

/// Four sprites over three batches and two sheets — `sprite.slang` — on the
/// backend `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// This is the scene `SV_InstanceID` broke: the third batch starts at instance
/// 3, and a backend that reads the wrong lowering draws the last rectangle in
/// the first's place. [`every_sprite_slot_is_painted_and_the_gaps_are_not`] is
/// what makes that a failure here rather than only in the golden's numbers.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_through_the_sprite_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Sprite,
        "sprite",
        MIN_COLORS_SPRITE,
        every_sprite_slot_is_painted_and_the_gaps_are_not,
    );
}

/// Rectangles, an outline and glyph-atlas text — `ui.slang` — on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds`] for
/// this scene's anti-vacuity claim, which is a blend rather than a lit centre:
/// the UI pass loads its target rather than clearing it, so "it composited"
/// is the thing worth asserting.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_through_the_ui_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ui,
        "ui",
        MIN_COLORS_UI,
        the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds,
    );
}

/// Draws one frame of `scene` and compares it against `tests/golden/{golden}.png`.
///
/// **What the assertions are for, in the order a failure would hit them:**
///
/// 1. The backend that opened is the one that was asked for. Every backend
///    draws these scenes identically by construction, so a Metal run that fell
///    back to wgpu would produce a passing frame and prove nothing about Metal.
/// 2. The adapter it opened is the class
///    [`ADAPTER_ENV_VAR`](crcbl::adapter::ADAPTER_ENV_VAR) named, and both the
///    adapter and the pin are printed. The same argument one layer down: this
///    frame died on `windows-latest` with `DXGI_ERROR_DEVICE_REMOVED` because
///    the first enumerated adapter is not a usable device there.
/// 3. The device's [`GeometryPath`](crcbl::hal::GeometryPath) is reported, and
///    the frame is drawn through whichever indirect tail it selects. Metal
///    reports no `DRAW_INDIRECT_COUNT` — the flag is absent from the API rather
///    than unimplemented — so it selects
///    [`IndirectPerBatch`](crcbl::hal::GeometryPath::IndirectPerBatch), the arm
///    that until now had only ever run on Vulkan behind a forced selector.
/// 4. Something drew: at least `min_colors` colours, plus `inspect`'s
///    scene-specific claim about *where* it drew. A full-screen quad and a blank
///    frame both fail these and neither is distinguishable by a tolerance.
/// 5. The picture is the one that was reviewed.
///
/// Both halves of 4 are per scene, because the scenes are not equally colourful
/// and they are not the same shape: the cube has a lit centre against a dark
/// clear, the sprite scene has four rectangles with gaps between them, and the
/// UI frame is a composite over a background it never clears.
fn draw_scene_and_match_its_golden(
    scene: Scene,
    golden: &str,
    min_colors: usize,
    inspect: fn(&Image),
) {
    // **Install a logger before opening anything.** Without one, every
    // `log::info!` a backend emits on the way to a device — the adapter it
    // chose, the surface it built, whether a validation layer loaded — goes
    // nowhere. Two sessions were spent diagnosing a D3D12 failure inside
    // `open` with no backend output at all in the run, for exactly this
    // reason: the panic message was the only evidence, and it named the call
    // that noticed rather than the one that caused it.
    crcbl_core::log::init_logging();

    // `unwrap_or_else` rather than `expect`, which would format the error with
    // `Debug` and escape the newlines out of the adapter listing a pin miss
    // carries — on a runner nobody can log into, that listing is the whole
    // diagnosis.
    let mut setup = OffscreenSetup::open(EXTENT.0, EXTENT.1, scene)
        .unwrap_or_else(|why| panic!("a GPU backend opens for the {golden} scene: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    // Printed unconditionally, and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "crcbl render e2e: {backend} selected {:?} / {:?} / {:?} for the {golden} scene",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );

    // The adapter line, and the raw pin beside it. `run-render-e2e.sh` matches
    // the pin back against what it exported, because the one failure this test
    // cannot see is the variable never reaching this process: the pin would be
    // `None` here, `select` would take the first adapter, and every assertion
    // below would pass on a device nobody asked for.
    let adapter = setup.adapter();
    let requested_adapter = crcbl::adapter::pin();
    eprintln!(
        "crcbl render e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = requested_adapter.as_deref().unwrap_or("<unset>"),
    );

    // **The device took the best path its adapter offers.** The frame alone
    // cannot say — every path draws this scene identically, which is what
    // `the_*_scene_draws_the_same_frame_on_every_geometry_path` checks — so a
    // request that omitted a selector's flag would leave the renderer on a
    // lesser tail and every assertion below would still pass. That is not
    // hypothetical: `Features::MESH_SHADER` is not part of `GPU_DRIVEN`, so
    // until `OffscreenSetup::OPTIONAL_FEATURES` named it, an adapter reporting
    // mesh shaders drew this frame through `IndirectCount` and nothing said so.
    assert_eq!(
        caps.geometry_path(),
        adapter.caps.geometry_path(),
        "adapter {} offers {:?} and the device opened on {:?} — a selector's flag \
         was not asked for, so the frame took a lesser tail than this machine can run",
        adapter.name,
        adapter.caps.geometry_path(),
        caps.geometry_path(),
    );

    // A pin the loader ignored is the failure this catches, and it is the same
    // class as a suite that runs no tests. Both names go through the mappings
    // that already exist rather than a third table.
    if let Ok(requested) = std::env::var(BACKEND_ENV_VAR) {
        let opened = GpuBackend::from_name(&backend.to_string())
            .expect("every backend the registry can open has a GpuBackend spelling");
        assert_eq!(
            Some(opened),
            GpuBackend::from_name(&requested),
            "{BACKEND_ENV_VAR}={requested} was asked for and {backend} drew the frame"
        );
    }
    // Same shape one layer down: `select` refuses a class it cannot find, so
    // this can only fire if it resolved to something else — but it is the
    // assertion that makes the pin's arrival observable rather than assumed.
    if let Some(requested) = requested_adapter.as_deref() {
        let want = device_type_from_name(requested)
            .unwrap_or_else(|| panic!("{ADAPTER_ENV_VAR}={requested} is not a device class"));
        assert_eq!(
            adapter.device_type, want,
            "{ADAPTER_ENV_VAR}={requested} was asked for and adapter {} ({:?}) drew the frame",
            adapter.name, adapter.device_type
        );
    }

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else — so a run that panicked
    // on the pixels first would report a wrong picture where the real answer is
    // that the GPU never finished drawing it.
    setup.finish().expect("the device reaches idle");

    assert_eq!(
        (width, height),
        EXTENT,
        "the swapchain handed back an extent the golden was not blessed at"
    );
    let image = Image::from_readback(width, height, &pixels, channel_order(format))
        .expect("the readback is exactly one image");

    let colors = image.distinct_colors(min_colors);
    assert!(
        colors >= min_colors,
        "a {golden} frame with {colors} distinct colour(s) (counted to {min_colors}) is not \
         evidence — nothing drew, or only the clear did"
    );
    inspect(&image);

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{golden}.png"));
    let golden_image = Golden::new(reference);
    let comparison = golden_image
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    eprintln!(
        "crcbl render e2e: golden {golden} on {backend} — {}",
        comparison.summary()
    );
}

/// The cube on both geometry paths this machine can reach — see
/// [`draw_scene_on_every_geometry_path`].
///
/// **This is the scene the comparison is about.** [`Scene::Cube`] is the only one
/// of the three drawn by `crcbl-render`'s `ForwardRenderer`, and that renderer is
/// the only thing above the seam that branches on
/// [`GeometryPath`](crcbl::hal::GeometryPath): a mesh device records
/// `draw_mesh_tasks` against a mesh pipeline with no vertex stage and no index
/// buffer, where the others record an indirect call reading the same pool through
/// a vertex stage. Three instances, a material table and a base-colour page, none
/// of which `crcbl-vk`'s own `mesh.png` scene has.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Cube,
        "cube",
        MIN_COLORS_CUBE,
        the_cube_scene_drew_its_geometry_and_both_material_columns,
    );
}

/// The dunes patch on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`].
///
/// **This is the two granularities of `docs/plan/25-lod.md` compared in
/// pixels.** The mesh arm descends the cluster DAG per cluster in its
/// amplification stage and the lesser arm takes a uniform cut per instance in
/// the cull pass; both are asserted here to produce the *same frame*, byte for
/// byte.
///
/// That is a real claim rather than a tautology, and it is one this scene's
/// camera and budget make available: at a one-pixel budget from two units off
/// the patch's near edge every group is expanded, so the per-cluster cut is the
/// whole of level 0 and the uniform cut's level is 0 — the same triangles by two
/// entirely different routes, one through an index pool and a vertex stage and
/// one through cluster records and a mesh stage.
///
/// **It is also why this is a pixel comparison and not the comparison.** Move
/// the camera back or raise the budget and the two paths diverge *by design*:
/// the per-cluster cut mixes levels across the surface where the uniform one
/// cannot, and the frames stop being equal. What stays true at every camera is
/// that the uniform cut's level is the finest level the per-cluster cut draws,
/// and that is asserted in numbers by `crcbl-shaders`'
/// `the_uniform_level_is_the_finest_level_the_per_cluster_cut_draws` and on real
/// devices by `crcbl-vk`'s
/// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_dunes_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Dunes,
        "dunes",
        MIN_COLORS_DUNES,
        the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone,
    );
}

/// The sprite scene on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`].
///
/// `crcbl-render`'s sprite pass reads no
/// [`GeometryPath`](crcbl::hal::GeometryPath), so the two arms here differ only in
/// whether the *device* was opened with mesh shading enabled. That is the claim
/// worth having and it is not the cube's: enabling an extension changes a Vulkan
/// device's pipeline cache, its enabled feature struct and, on some drivers, its
/// shader compiler — and nothing else in this tree would notice if that moved a
/// pass that never asked for it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Sprite,
        "sprite",
        MIN_COLORS_SPRITE,
        every_sprite_slot_is_painted_and_the_gaps_are_not,
    );
}

/// The UI scene on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`], and
/// [`the_sprite_scene_draws_the_same_frame_on_every_geometry_path`] for what an
/// arm proves about a pass that reads no geometry path.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Ui,
        "ui",
        MIN_COLORS_UI,
        the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds,
    );
}

/// Draws `scene` twice — once on the best [`GeometryPath`] this adapter reports
/// and once on the path it selects without [`Features::MESH_SHADER`] — and
/// asserts the two frames are **byte for byte the same picture**.
///
/// # Why this has to exist before the mesh path is selected by default
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5 calls the mesh shader the primary
/// geometry path and its design rule is that "the lesser path is a constraint on
/// data layout, not a separate renderer". `crcbl-vk`'s
/// `every_geometry_path_draws_the_same_frame` already checks that on the vk
/// suite's own scene: one mesh, one pyramid, no material table. It says nothing
/// about the scene this crate's goldens are blessed on, which has three instances
/// across two buckets, a material table and a sampled base-colour page — every
/// piece of per-draw data the mesh stage has to fetch for itself because there is
/// no input assembler to hand it one.
///
/// So this is the check that lets `OffscreenSetup::OPTIONAL_FEATURES` ask for
/// mesh shading at all. A second, per-path golden would have hidden exactly what
/// it is for: a difference here is a **bug in the mesh path**, not two legitimate
/// pictures.
///
/// # The arms are asked for by subtraction
///
/// An adapter reports what it reports, so the only way one machine reaches more
/// than one path is to open a device *without* the flag that selects the better
/// one — `crcbl-vk`'s `Headless::open_for_mesh_with` and this crate's
/// [`OffscreenSetup::open_with`] exist for that and nothing else.
///
/// # On a device with no mesh shaders
///
/// Both arms land on the same path, and the test says so rather than passing
/// quietly: the arms are asserted to differ **exactly when the adapter reports
/// the flag**, so "this backend has no mesh shaders" is a checked claim instead
/// of a skip. `crcbl-wgpu`, `crcbl-dx12` and `crcbl-mtl` all report none — see
/// each backend's `caps` — so on those three this is a self-comparison and the
/// printed line is what says so.
fn draw_scene_on_every_geometry_path(
    scene: Scene,
    name: &str,
    min_colors: usize,
    inspect: fn(&Image),
) {
    crcbl_core::log::init_logging();

    let mut frames: Vec<(GeometryPath, Image)> = Vec::new();
    let mut adapter_offers_mesh = None;

    // **Both mesh-stage flags come out of the lesser arm, not just the one that
    // names the path.** `Features::TASK_SHADER` is an amplification stage in
    // front of a mesh stage, so a backend asked for it enables the mesh stage
    // too — and a device that ended up on `MeshShader` after `MESH_SHADER` was
    // subtracted is a self-comparison wearing a cross-path label, which is the
    // failure the assertion below reports.
    let mesh_stage = Features::MESH_SHADER.union(Features::TASK_SHADER);
    for optional in [
        OffscreenSetup::OPTIONAL_FEATURES.union(mesh_stage),
        OffscreenSetup::OPTIONAL_FEATURES.difference(mesh_stage),
    ] {
        let mut setup = OffscreenSetup::open_with(EXTENT.0, EXTENT.1, scene, optional)
            .unwrap_or_else(|why| panic!("a GPU backend opens for the {name} scene: {why}"));
        let path = setup.caps().geometry_path();
        let offers_mesh = setup
            .adapter()
            .caps
            .features
            .contains(Features::MESH_SHADER);
        eprintln!(
            "crcbl render e2e: {name} on {backend} adapter {adapter:?} — asked for \
             MESH_SHADER: {asked}, adapter has it: {offers_mesh}, drew through {path:?}",
            backend = setup.backend(),
            adapter = setup.adapter().name,
            asked = optional.contains(Features::MESH_SHADER),
        );
        assert_eq!(
            *adapter_offers_mesh.get_or_insert(offers_mesh),
            offers_mesh,
            "the two arms opened different adapters, so they are not a comparison"
        );

        let format = setup.format();
        let ((width, height), pixels) = setup
            .draw_and_readback()
            .unwrap_or_else(|why| panic!("the {name} frame renders on {path:?}: {why}"));
        setup.finish().expect("the device reaches idle");

        assert_eq!((width, height), EXTENT, "{path:?} drew a different extent");
        frames.push((
            path,
            Image::from_readback(width, height, &pixels, channel_order(format))
                .expect("the readback is exactly one image"),
        ));
    }

    let adapter_offers_mesh = adapter_offers_mesh.expect("both arms opened a device");
    let (best_path, best) = &frames[0];
    let (lesser_path, lesser) = &frames[1];

    // **The anti-vacuity claim, and it is two claims.** Two blank frames match
    // perfectly, so the frame has to hold the scene before comparing it means
    // anything — that is `min_colors` and `inspect`, the same pair the golden
    // test uses. And two frames drawn by the same code match perfectly too, so
    // the paths have to actually differ: derived from the adapter rather than
    // written down, because a backend reporting no mesh shaders is a legitimate
    // run of this test and a silent one is not.
    let colors = best.distinct_colors(min_colors);
    assert!(
        colors >= min_colors,
        "a {name} frame with {colors} distinct colour(s) (counted to {min_colors}) is not \
         evidence — comparing it against another frame like it proves nothing"
    );
    inspect(best);
    assert_eq!(
        best_path != lesser_path,
        adapter_offers_mesh,
        "the adapter {} mesh shading and the two arms selected {best_path:?} and \
         {lesser_path:?} — one of those two facts is wrong, and a self-comparison \
         that reads as a cross-path one is worse than no test",
        if adapter_offers_mesh {
            "reports"
        } else {
            "reports no"
        },
    );

    // Byte equality, not the golden's rasteriser tolerance: this is one adapter
    // and one driver drawing one scene, so every differing pixel is a difference
    // the two submissions made.
    assert_eq!(
        best.pixels(),
        lesser.pixels(),
        "{best_path:?} and {lesser_path:?} draw the {name} scene differently"
    );
    eprintln!(
        "crcbl render e2e: {name} is byte-identical on {best_path:?} and {lesser_path:?} \
         ({colors} distinct colour(s))"
    );
}

/// [`Scene::Cube`]'s anti-vacuity claim: a corner still holding the clear, and a
/// centre that is not it.
///
/// The bounds are absolute rather than relative to the corner because this
/// scene's clear is near-black and its cube is lit — "dark here, bright there"
/// is the claim, and it is the one a full-screen quad and a blank frame both
/// fail.
fn the_cube_is_lit_against_an_unpainted_corner(image: &Image) {
    let corner = image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = image.pixel(EXTENT.0 / 2, EXTENT.1 / 2).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );
}

/// The left-hand column of [`Scene::Cube`], in pixels: wide enough for a
/// pyramid, narrow enough to exclude the cube.
///
/// The pyramids sit at world `x = -1.05` spanning `±0.4`, which at this frame's
/// 4:3 aspect is `-0.94 ..= -0.42` in NDC — pixels 8 to 74. The cube starts at
/// NDC `-0.325`, pixel 86. Eighty is between them, so this band is the pyramids
/// and the clear behind them and nothing else.
const PYRAMID_BAND: std::ops::Range<u32> = 0..80;

/// **The texture column of the material row, read off the frame.**
///
/// `Scene::Cube`'s left column holds two instances of one mesh at one
/// orientation whose materials differ in nothing but their base-colour page
/// layer: the upper names the page's white layer, the lower a layer of four
/// unequal texels. So the upper pyramid's faces are flat — one lit colour each,
/// because the mesh has flat normals — and the lower one's are quartered.
///
/// Counting distinct colours is what tells those apart without knowing which
/// texel landed where. It is also the assertion that fails if the *UV* is
/// missing rather than the index: a fragment stage handed a constant texture
/// coordinate samples one texel over a whole face and produces a flat pyramid
/// in a different shade, which passes "the two differ" and fails this.
fn the_textured_pyramid_is_quartered_and_the_plain_one_is_flat(image: &Image) {
    let half = EXTENT.1 / 2;
    let plain = distinct_colors_in(image, PYRAMID_BAND, 0..half);
    let textured = distinct_colors_in(image, PYRAMID_BAND, half..EXTENT.1);
    eprintln!(
        "crcbl render e2e: pyramid column — {plain} colour(s) above, {textured} below \
         (the lower one samples a four-texel layer)"
    );
    // Each visible face of the plain pyramid is one flat colour; each face of
    // the textured one is up to four. Three faces are visible from this camera,
    // so the gap is several colours wide and not one.
    assert!(
        textured >= plain + 4,
        "the lower pyramid samples a four-texel layer and the upper one a flat white layer, \
         so it must hold several more distinct colours: {textured} below vs {plain} above"
    );
}

/// Distinct RGBA colours inside a rectangle of `image`.
///
/// [`Image::distinct_colors`] answers for the whole frame, which cannot separate
/// one object from another; this is the same question asked of a region.
fn distinct_colors_in(
    image: &Image,
    columns: std::ops::Range<u32>,
    rows: std::ops::Range<u32>,
) -> usize {
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        for column in columns.clone() {
            if let Some(pixel) = image.pixel(column, row) {
                seen.insert(pixel);
            }
        }
    }
    seen.len()
}

/// Where a point in [`Scene::Sprite`]'s world lands in this frame.
///
/// That scene is drawn with an orthographic camera down −Z with Y up and a fixed
/// half height in world units, so the half width is that times the frame's
/// aspect — `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT` and `sprite_camera`,
/// both private to that module. This is therefore the one place the projection
/// is written down twice; a change to it moves the picture, and re-blessing the
/// golden is the step that will notice.
fn sprite_pixel(world_x: f32, world_y: f32) -> (u32, u32) {
    /// `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT`.
    const HALF_HEIGHT: f32 = 100.0;

    let half_width = HALF_HEIGHT * EXTENT.0 as f32 / EXTENT.1 as f32;
    let x = (world_x / half_width + 1.0) / 2.0 * EXTENT.0 as f32;
    let y = (1.0 - world_y / HALF_HEIGHT) / 2.0 * EXTENT.1 as f32;
    (x as u32, y as u32)
}

/// [`Scene::Sprite`]'s anti-vacuity claim: all four rectangles drew, and the
/// background between them did not.
///
/// The clear is read off the frame's corner rather than written down here — the
/// sprite pass loads its target instead of clearing it, so the corner is the
/// scene's clear by construction and comparing against it needs no assumption
/// about sRGB encoding or channel order.
///
/// The four samples are a quarter of a rectangle in from each centre rather than
/// on it: every sprite is a 2x2 or 4x2 texel sheet, so the exact centre lands on
/// a texel boundary and which of the four colours it resolves to is the
/// sampler's business. A quarter in is squarely inside one texel whichever way
/// V runs.
///
/// **This is the `SV_InstanceID` detector.** Three batches over two sheets, the
/// third starting at instance 3: a backend that reads the wrong lowering draws
/// the last rectangle over the first and leaves its own slot at the clear, which
/// is a slot that fails here by name rather than a number in a summary line.
fn every_sprite_slot_is_painted_and_the_gaps_are_not(image: &Image) {
    /// The world-space centres of `crcbl::screenshot`'s `SPRITE_RECTS`: four
    /// 40x40 rectangles on `y = 0`, ten units apart so no two of them touch.
    const SLOT_CENTRES: [f32; 4] = [-75.0, -25.0, 25.0, 75.0];
    /// Two of the three gaps between those rectangles, in world x. A quarter of
    /// the frame apart, and neither is the centre of a rectangle under any
    /// rotation of the batch order.
    const GAPS: [f32; 2] = [-50.0, 0.0];
    /// A quarter of a rectangle, in world units — see the sampling note above.
    const QUARTER: f32 = 10.0;

    let clear = image.pixel(1, 1).expect("inside");

    for (slot, centre) in SLOT_CENTRES.iter().enumerate() {
        let (x, y) = sprite_pixel(centre - QUARTER, QUARTER);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            differ(pixel, clear),
            "sprite slot {slot} at ({x}, {y}) is still the clear {clear:?} (got {pixel:?}) — \
             that batch did not draw, or it drew somewhere else"
        );
    }

    for gap in GAPS {
        let (x, y) = sprite_pixel(gap, 0.0);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            !differ(pixel, clear),
            "the gap between two sprites at ({x}, {y}) is painted {pixel:?} against a clear of \
             {clear:?} — the sprites are not rectangles where the scene puts them"
        );
    }
}

/// Where a fraction of the frame lands in pixels.
///
/// `crcbl::screenshot`'s `ui_draw_list` lays [`Scene::Ui`] out as fractions of
/// the extent, in the pass's Y-down screen pixels, so that the same picture
/// arrives at every `--size`. The samples below are written in the same
/// fractions its rectangles are.
fn ui_pixel(fraction_x: f32, fraction_y: f32) -> (u32, u32) {
    (
        (fraction_x * EXTENT.0 as f32) as u32,
        (fraction_y * EXTENT.1 as f32) as u32,
    )
}

/// [`Scene::Ui`]'s anti-vacuity claim: the panel drew, and the translucent bar
/// blended rather than replaced.
///
/// There is no lit centre to assert here and no useful "the middle is not the
/// clear" — the UI pass **loads** its target rather than clearing it, so what is
/// worth proving is that it composited onto what was already there. The bar
/// straddles the panel's bottom edge, so it covers two different backgrounds:
/// blending gives two different results and replacing gives one. That is a claim
/// a blank frame fails, a frame that lost the panel fails, and a frame whose
/// blend state came up as `Replace` also fails.
///
/// The three samples avoid the two lines of text by construction: both start at
/// 12% of the width, and the panel sample is far to the right of where the
/// longest of them can reach while the bar samples sit below the second line.
fn the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds(image: &Image) {
    let clear = image.pixel(1, 1).expect("inside");

    // Inside the panel (8%-92% across, 10%-62% down), outside the bar and
    // clear of the outline's band at the frame's edge.
    let (x, y) = ui_pixel(0.88, 0.25);
    let panel = image.pixel(x, y).expect("inside");
    assert!(
        differ(panel, clear),
        "the UI panel at ({x}, {y}) is still the clear {clear:?} (got {panel:?}) — nothing \
         filled it"
    );

    // The bar (30%-70% across, 45%-85% down) over the panel, then over the
    // clear below the panel's bottom edge.
    let (x, y) = ui_pixel(0.50, 0.50);
    let over_panel = image.pixel(x, y).expect("inside");
    let (x, y) = ui_pixel(0.62, 0.80);
    let over_clear = image.pixel(x, y).expect("inside");
    assert!(
        differ(over_panel, clear) && differ(over_clear, clear),
        "the translucent bar drew nothing: over the panel {over_panel:?}, over the clear \
         {over_clear:?}, against a clear of {clear:?}"
    );
    assert!(
        differ(over_panel, over_clear),
        "the bar is {over_panel:?} over the panel and {over_clear:?} over the clear — the same \
         colour on two backgrounds means the pass replaced rather than blended"
    );
    assert!(
        differ(over_panel, panel),
        "the bar over the panel is {over_panel:?} and the bare panel is {panel:?} — the bar did \
         not reach the panel at all"
    );
}
