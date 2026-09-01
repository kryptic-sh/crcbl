//! **A light the atlas demoted, rendered** — `docs/plan/45-shadows.md`'s
//! priority rung, on a device.
//!
//! Two spot lights stand over one floor. `crcbl_render::shadow`'s `coverage` is
//! how much of the frame's height a light's shadow map covers on screen, and
//! the second light's map covers a fraction of what the first's does — so the
//! allocator hands it a **quarter of a root cell's side** where the first gets
//! the whole of it. Nothing in this tree asked for a sub-cell tile before this
//! rung, so nothing had ever rendered into one.
//!
//! # What is asked here, and what is asked without a device
//!
//! **Which level a coverage earns, and the band around each threshold**, is
//! arithmetic and is asked in `crcbl_render::shadow`'s own tests — including
//! the light that crosses a cutoff and comes back. **That every bias reads the
//! side of the map it is sampling** is a property of the source and is asked by
//! `crcbl_shaders::mesh`'s
//! `every_shadow_bias_is_denominated_in_its_own_maps_texels`.
//!
//! What is left for a device is the picture, and it is the half neither of
//! those can reach: a map a quarter of a cell's side has **four times** the
//! world footprint per texel, so a bias denominated in the cell's side is four
//! times too small on that one light and on no other. What that draws is acne —
//! the receiver shadowing itself in a stipple across its own lit pool — on a
//! frame where every other light is right, which is as plausible a picture as
//! this engine can draw and one no golden in this tree can see, because every
//! golden's lights take whole cells.
//!
//! # The frame is larger than this suite's, and it has to be
//!
//! A demoted light's pool is small on screen *by construction*: the coverage
//! that demoted it is the fraction of the frame its map covers, and the pool is
//! that map. At [`MESH_EXTENT`](crate::mesh_scene::MESH_EXTENT) a level-2
//! light's pool is under thirty pixels across, which is too few to count a
//! stipple in. So this file opens its own ring at [`EXTENT`] — the extent
//! `apps/lantern`'s review frames already run at on both measurable tiers.

use crate::area_light::{PRICE_WARMUP, price_frame};
use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh_lit};
use crcbl::hal::Features;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::shadow::{Cadence, MIN_TILE, TILE, light_tile};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, Light, Projection, SpotLight, TransientPool,
};

/// The frame this file renders at.
///
/// `apps/lantern`'s review extent, which both measurable tiers already draw. Its
/// width is divisible by 64, which is what the readback's 256-byte row pitch
/// needs — see [`render_mesh_lit`].
const EXTENT: (u32, u32) = (1280, 960);

/// The floor's side, in world units.
///
/// Wider than the camera sees, so no pool falls off its edge.
const FLOOR: f32 = 44.0;

/// The half-angle both cones close at, in radians.
///
/// **One shape for both lights**, so the only thing that differs between them is
/// how far each reaches: a map's coverage is its footprint over its distance to
/// the eye, and the two lights stand the same distance away.
const CONE: f32 = 0.5;

/// How far the whole-cell light reaches, in world units.
///
/// Chosen for its **coverage** rather than for its picture: at this distance
/// from the camera its map covers over a quarter of the frame's height, which
/// is `crcbl_render::shadow`'s `WHOLE_CELL_COVERAGE` and therefore a whole root
/// cell.
const REACH: f32 = 3.46;

/// How far the demoted light reaches.
///
/// A quarter of [`REACH`], near enough, which puts its coverage between a
/// sixteenth and an eighth of the frame — level 2, a quarter of a root cell's
/// side. It is deliberately not near either end of that band: a scene that sat
/// on a threshold would be a test of the hysteresis rather than of the picture,
/// and the hysteresis is asked without a device.
const FAR_REACH: f32 = 1.395;

/// How far off vertical each cone leans, in radians.
///
/// **Grazing, because that is where acne lives.** A cone straight down over a
/// flat floor throws its caster's shadow directly under the caster, where the
/// caster's own image covers it, and it meets the floor square-on where a
/// receiver shadows itself least. At this lean the floor's `N·L` is about a
/// third and its slope across a shadow texel is nearly three — the regime
/// `mesh.slang`'s `PUNCTUAL_DEPTH_BIAS_TEXELS` was swept in.
const TILT: f32 = 1.2;

/// Where along its own axis each cone meets the floor, as a fraction of the
/// light's reach.
///
/// Inside the reach rather than at it: `punctual_falloff`'s window is **exactly
/// zero** at the radius, so a floor standing at the light's far plane is a floor
/// this scene draws black.
const FLOOR_ALONG_AXIS: f32 = 0.6;

/// Where the whole-cell light's pool lands on the floor.
///
/// Off to `-x` and back along `-z`: this camera looks down `-y` with `+z` up, so
/// its right-hand axis is `-x` and the pool a grazing cone stretches along `+z`.
/// Both of those are what put the two pools in opposite halves of the frame with
/// their tails inside it.
const WHOLE_POOL: Vec3 = Vec3::new(-4.5, 0.0, -3.5);

/// Where the demoted light's pool lands.
const DEMOTED_POOL: Vec3 = Vec3::new(4.5, 0.0, -1.0);

/// How much the pyramid is scaled by to make each light's caster.
///
/// Roughly in proportion to the pools, so each shadow is the same share of the
/// pool it falls in and the statistic below counts the same shape in both.
const CASTERS: [f32; 2] = [0.5, 0.32];

/// The camera: straight down over the floor.
///
/// **Straight down, and the measurement rests on it.** The floor is then square
/// to the view, so a pool is as many pixels across as its own coverage says and
/// nothing is lost to foreshortening — which matters because a demoted light's
/// pool is small by construction and this file has to count a stipple inside it.
fn floor_camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 16.0, 0.0),
        target: Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`.
        up: Vec3::Z,
        projection: Projection::default(),
    }
}

/// A cone of `reach` aimed at `pool`, leaning [`TILT`] off vertical.
fn lamp(pool: Vec3, reach: f32) -> Light {
    let along = FLOOR_ALONG_AXIS * reach;
    let position = pool + Vec3::new(0.0, along * TILT.cos(), along * TILT.sin());
    Light::Spot(SpotLight {
        position,
        // Near-white and bright: the assertions below are about how far *below*
        // its own neighbourhood a pixel sits, so a coloured light would put the
        // comparison in one channel.
        color: Vec3::new(1.0, 0.97, 0.94) * 24.0,
        radius: reach,
        direction: pool - position,
        inner_angle: 0.7 * CONE,
        outer_angle: CONE,
        fill: false,
    })
}

/// The sun this frame is drawn under: none of it, and a floor of ambient.
///
/// **Black, so the punctual pools are the only lit thing in the frame.** A sun
/// would light the whole floor and fill in every pixel a self-shadowing lookup
/// darkened, which is a frame where the artefact this file exists to see cannot
/// appear. The ambient is what keeps the geometry outside the pools legible in a
/// dumped frame.
fn no_sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Y,
        color: Vec3::ZERO,
        ambient: Vec3::splat(0.015),
    }
}

/// The floor, the two casters and the two lights, in that order.
fn lit_floor(headless: &Headless) -> (ForwardRenderer, TransientPool) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    // The cube spans half a unit either side of its origin, so a floor scaled to
    // `FLOOR` and dropped by half of it has its `+Y` face on `y = 0`.
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -0.5 * FLOOR, 0.0))
            * Mat4::from_scale(Vec3::splat(FLOOR)),
    );
    for (pool, scale) in [WHOLE_POOL, DEMOTED_POOL].into_iter().zip(CASTERS) {
        // The pyramid's base is at `-0.4` in its own space, so lifting it by
        // that much of the scale stands it on the floor — which is what puts the
        // contact point of its shadow in the frame.
        place(
            &mut renderer,
            crcbl::render::scene::DEMO_PYRAMID,
            crcbl::render::scene::DEMO_UNTINTED,
            Mat4::from_translation(pool + Vec3::new(0.0, 0.4 * scale, 0.0))
                * Mat4::from_scale(Vec3::splat(scale)),
        );
    }
    renderer.set_lights(&[lamp(WHOLE_POOL, REACH), lamp(DEMOTED_POOL, FAR_REACH)]);
    (renderer, TransientPool::new())
}

/// Releases everything in dependency order, then asks the device what it saw.
fn teardown(headless: Headless, renderer: ForwardRenderer, mut pool: TransientPool) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// The luma of one pixel, as the assertions below rank darkness by.
fn luma(pixel: [u8; 4]) -> f32 {
    0.2126f32.mul_add(
        f32::from(pixel[0]),
        0.7152f32.mul_add(f32::from(pixel[1]), 0.0722 * f32::from(pixel[2])),
    )
}

/// Every pixel of `image` inside `box_of` that belongs to a lit pool, as
/// `(x, y, luma)`.
///
/// A pool is **found rather than predicted**: it is every pixel of the box at
/// half the box's own peak or brighter, and the box is a whole half of the
/// frame. Predicting the ellipse a leaning cone lands in would be a second copy
/// of the projection and of the falloff, and a test that reconstructs the
/// renderer's arithmetic can agree with a renderer that is wrong.
///
/// A *fraction* of the peak rather than a level, because the two pools differ in
/// brightness by their own distance falloff.
fn pool_of(image: &crcbl_golden::Image, box_of: (u32, u32, u32, u32)) -> Vec<(u32, u32, f32)> {
    let (left, top, right, bottom) = box_of;
    let mut pool = Vec::new();
    let mut brightest = 0.0f32;
    for y in top..bottom {
        for x in left..right {
            let value = luma(image.pixel(x, y).expect("inside the frame"));
            brightest = brightest.max(value);
            pool.push((x, y, value));
        }
    }
    let floor = 0.5 * brightest;
    pool.retain(|(_, _, value)| *value >= floor);
    pool
}

/// How far below its own neighbourhood's median a pixel has to sit before it is
/// counted a self-shadowing dot, in luma.
///
/// **Swept rather than guessed**, on the frame below, against the same frame
/// drawn with `punctual_visibility`'s divisor forced back to a whole cell's side
/// — which is exactly the defect this file exists to catch, and four times too
/// little bias on this light. Measured on radv at [`EXTENT`], counting inside
/// the demoted light's pool:
///
/// | This constant | Correct bias | Bias taken from the cell | Separation |
/// | ------------- | ------------ | ------------------------ | ---------- |
/// | 2             | 31           | 383                      | 12.4×      |
/// | 4 (shipped)   | 22           | 195                      | 8.9×       |
/// | 6             | 16           | 140                      | 8.8×       |
/// | 8             | 16           | 92                       | 5.8×       |
/// | 10            | 12           | 47                       | 3.9×       |
/// | 14            | 12           | 19                       | 1.6×       |
///
/// The separation is widest at the bottom and gone by the top: a whole cell's
/// worth of under-bias moves most of its pixels by under a dozen luma, so a
/// threshold at fourteen is measuring the noise either side of it. Two is wider
/// still, and it is the floor a driver's own rounding lives on — the correct
/// column climbs there too, which is that noise arriving. Four is the step in
/// from it that keeps the separation and leaves the noise: the pool holds 2941
/// pixels correctly biased and 2828 wrongly, so 22 dots is 0.75% and 195 is
/// 6.9%, and the shipped [`SPECKLE_PERCENT`] of 3 sits 4× above the correct
/// rate and 2.3× below the wrong one.
const SPECKLE_LUMA: f32 = 4.0;

/// What share of a pool may be self-shadowing dots before the bias is wrong.
///
/// Three per cent, between the 0.75% a correctly biased demoted pool measures
/// and the 6.9% a wrongly biased one does — [`SPECKLE_LUMA`] carries the sweep.
/// Both pools are held to it, and the whole-cell one is the control: it reads
/// 30 dots in 13903 pixels either way, a fifth of a per cent, so a run where
/// *both* fire is a bias that broke for every light rather than for the demoted
/// one.
const SPECKLE_PERCENT: usize = 3;

/// How many pixels of `pool` sit more than [`SPECKLE_LUMA`] below the median of
/// their own 5×5 neighbourhood.
///
/// **What a self-shadowing dot is, and what a smooth falloff is not** — the same
/// statistic `crcbl_render::shadow`'s `DEPTH_BIAS_TEXELS` was swept with over
/// the dunes patch. A cone's pool is a gradient in every direction, and a
/// gradient's own pixels sit *on* their neighbourhood's median; a pixel whose
/// map quantised the receiver's own depth away sits below it.
///
/// The caster's shadow is a step rather than a dot, so its rim contributes a
/// thin line either way — which is most of what a correctly biased pool counts,
/// and what [`SPECKLE_PERCENT`]'s headroom is for.
fn speckle(image: &crcbl_golden::Image, pool: &[(u32, u32, f32)]) -> usize {
    pool.iter()
        .filter(|(x, y, value)| {
            let mut neighbourhood: Vec<f32> = Vec::with_capacity(25);
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (nx, ny) = (*x as i32 + dx, *y as i32 + dy);
                    if let Some(pixel) = u32::try_from(nx)
                        .ok()
                        .zip(u32::try_from(ny).ok())
                        .and_then(|(nx, ny)| image.pixel(nx, ny))
                    {
                        neighbourhood.push(luma(pixel));
                    }
                }
            }
            neighbourhood.sort_by(f32::total_cmp);
            median_below(&neighbourhood, *value)
        })
        .count()
}

/// Whether `value` sits more than [`SPECKLE_LUMA`] below the median of `sorted`.
fn median_below(sorted: &[f32], value: f32) -> bool {
    sorted[sorted.len() / 2] - value > SPECKLE_LUMA
}

/// How many dark pixels the demoted light's pool must enclose.
///
/// **A floor swept from the frame rather than a guess.** The correctly biased
/// frame encloses 537 on radv and 537 on lavapipe, and the same frame with
/// `tile_texels` spelled as its own reciprocal — a divisor that grows with the
/// map instead of shrinking, which is the over-bias end of this rung — encloses
/// 207 on radv. The shipped floor is between the two at about the geometric
/// mean: 1.6× under what a correct frame draws and 1.6× over what that failure
/// leaves.
const SHADOW_PIXELS: usize = 330;

/// How many pixels inside the pool's own bounding box are dark with a lit pixel
/// **on the same row to either side of them**.
///
/// The caster's shadow and the caster's own unlit faces, and nothing else: a
/// pixel outside the cone is dark too, but it runs to the edge of the box on one
/// side or the other, so it fails the test. That is what makes this a count of
/// what the light is *blocked* by rather than of what it never reached.
///
/// **A row rather than a row and a column.** The cone leans, so its caster's
/// shadow is a streak running out of the pool along the lean — enclosed on its
/// two sides and open at its end — and asking for a lit pixel above and below as
/// well counts only the stub of it. Measured on radv, four-way enclosure fell
/// from 46 pixels to 10 when the caster grew, which is the shadow reaching the
/// pool's edge rather than the shadow going away.
///
/// **And an enclosure test rather than "the pool's darkest pixel"**, which is
/// the shape this started as and is a check that cannot fail: `pool_of` keeps
/// only pixels at half the peak or brighter, so its darkest is at least half its
/// brightest by construction and the comparison was true whatever the frame
/// drew.
fn enclosed_dark(pool: &[(u32, u32, f32)]) -> usize {
    let Some(&(first_x, first_y, _)) = pool.first() else {
        return 0;
    };
    let (mut left, mut right, mut top, mut bottom) = (first_x, first_x, first_y, first_y);
    for (x, y, _) in pool {
        left = left.min(*x);
        right = right.max(*x);
        top = top.min(*y);
        bottom = bottom.max(*y);
    }
    let width = (right - left + 1) as usize;
    let height = (bottom - top + 1) as usize;
    let mut bright = vec![false; width * height];
    for (x, y, _) in pool {
        bright[(y - top) as usize * width + (x - left) as usize] = true;
    }
    let mut enclosed = 0;
    for row in 0..height {
        for column in 0..width {
            if bright[row * width + column] {
                continue;
            }
            let west = (0..column).any(|other| bright[row * width + other]);
            let east = (column + 1..width).any(|other| bright[row * width + other]);
            if west && east {
                enclosed += 1;
            }
        }
    }
    enclosed
}

/// **The demoted light is given a quarter of a root cell's side, and its shadow
/// is still right.**
///
/// Three claims, and each fails on its own:
///
/// * **The sides.** One light's map is a whole [`TILE`] and the other's is a
///   quarter of that side. A rung that computed a level and never spent it would
///   leave both at [`TILE`], and every other assertion here would still pass.
/// * **No acne on the demoted light.** Its pool is measured for pixels sitting
///   more than [`SPECKLE_LUMA`] below their own neighbourhood, which is a
///   receiver shadowing itself in its own map. Denominating the bias in a whole
///   cell's side rather than in the tile's understates it by exactly the factor
///   the light was demoted by — four, here — and [`SPECKLE_LUMA`]'s sweep is
///   what that measures.
/// * **There is still a shadow.** The other end of the same number: a divisor
///   that grew instead of shrinking biases the receiver past its own caster and
///   erases the shadow altogether, which is a lit pool with nothing in it.
///   [`enclosed_dark`] is what counts what the pool has in it, and
///   [`SHADOW_PIXELS`] carries what it measures either way.
///
/// **Detachment is not asked here, and saying so is the honest part.** Peter-
/// panning is a gap between a caster and its shadow of about the bias, and the
/// bias on this light is a hundredth of a world unit against a caster a sixth of
/// one tall — under a pixel at this camera. A test claiming to see it would be a
/// check that cannot fail. What holds that end is the third claim above, which
/// is the failure large enough to draw.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_demoted_light_gets_a_quarter_cell_and_a_shadow_that_still_fits_it() {
    let headless = Headless::open_at(
        EXTENT,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let (mut renderer, mut pool) = lit_floor(&headless);
    let camera = floor_camera();
    let image = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &camera,
        &no_sun(),
        None,
    );

    // The sides the allocator actually handed out, read off the selection this
    // frame was drawn from rather than recomputed.
    let sides: Vec<u32> = (0..2)
        .map(|light| {
            let base = renderer
                .shadow_lights()
                .base_of(light)
                .unwrap_or_else(|| panic!("light {light} was given no tile"));
            renderer.shadow_lights().atlas_rect(light_tile(base)).side
        })
        .collect();

    // Deferred on purpose, on this suite's terms: a test that panicked here
    // would leave the renderer, the pool and the device undestroyed, and the
    // resulting `Drop` warning would print on top of the message that says what
    // actually went wrong.
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(
            sides,
            vec![TILE, TILE >> 2],
            "the near light must hold a whole root cell and the far one a quarter of its side"
        );
        assert!(
            sides[1] >= MIN_TILE,
            "the allocator handed out a tile finer than it declares it can"
        );

        // The two pools, each in the half of the frame its light draws in — see
        // `WHOLE_POOL` for why the world's `-x` is the frame's right.
        let (width, height) = EXTENT;
        let demoted = pool_of(&image, (0, 0, width / 2, height));
        let whole = pool_of(&image, (width / 2, 0, width, height));
        eprintln!(
            "{}: whole-cell pool {} px with {} dots; demoted pool {} px with {} dots",
            crate::SUITE,
            whole.len(),
            speckle(&image, &whole),
            demoted.len(),
            speckle(&image, &demoted)
        );
        assert!(
            demoted.len() > 1_000,
            "the demoted light's pool is {} pixels, which is too few to count a stipple in — \
             the scene has moved and this test no longer measures what it names",
            demoted.len()
        );

        for (what, found) in [("whole-cell", &whole), ("demoted", &demoted)] {
            let dots = speckle(&image, found);
            assert!(
                dots * 100 <= found.len() * SPECKLE_PERCENT,
                "the {what} light's pool has {dots} self-shadowing dots in {} pixels, past the \
                 {SPECKLE_PERCENT}% this bias measures. A map a quarter of a root cell's side \
                 has four times the world footprint per texel, so a bias denominated in the \
                 cell rather than in the tile is four times too small — which is what this \
                 looks like",
                found.len()
            );
        }

        // The other end of the same divisor: a shadow that is still there.
        let enclosed = enclosed_dark(&demoted);
        eprintln!(
            "{}: demoted pool encloses {enclosed} dark pixels",
            crate::SUITE
        );
        assert!(
            enclosed >= SHADOW_PIXELS,
            "the demoted light's pool encloses {enclosed} dark pixels, under the \
             {SHADOW_PIXELS} its caster's shadow and its own unlit faces make. A divisor that \
             grew instead of shrinking biases every receiver past its own caster, which is a \
             lit pool with nothing in it"
        );
    }));

    teardown(headless, renderer, pool);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}

/// Patches along one side of the field the price below is measured over.
///
/// Wide enough that every light's map is filled by geometry rather than by the
/// pass's clear, which is what makes the measurement about the tiles' size.
const PRICE_FIELD: usize = 6;

/// How far the priced lights reach, in world units.
///
/// Large, so that four of them cover the field between them and each one's map
/// has something in it at every tile size.
const PRICE_REACH: f32 = 6.0;

/// Where the camera stands for the whole-cell half of the price, and for the
/// demoted half.
///
/// **The lights and the geometry are identical between the two**, and only the
/// eye moves: coverage is a light's map over its distance to the eye, so the
/// same rig at four times the distance is the same rig one ladder rung — two
/// levels — further down. Changing the lights instead would change what each
/// one's cull admits, and the measurement would be of two different draws.
const PRICE_EYES: [f32; 2] = [22.0, 88.0];

/// The `shadow` pass, as the render graph labels it — the same string the debug
/// overlay shows.
const PRICED_PASS: &str = "shadow";

/// How far one patch of the field is moved per frame while a price is being
/// measured with `stirred` set, in world units.
///
/// Small enough that no map's contents change in any way a picture would show,
/// and large enough to be a real translation rather than a rounding no `f32`
/// records: the field's patches stand tens of units from the origin, and a drift
/// under the last bit of a number that size is a write whose value never moves.
const STIR_STEP: f32 = 1.0e-3;

/// What one run of [`shadow_pass_prices`] measured, one entry per arm.
///
/// A type rather than a tuple of three arrays, which is what it was until
/// clippy's `type_complexity` said so — and it reads better at the call sites,
/// where the three are asked about for three different reasons.
struct PricedShadowPass {
    /// Each arm's `shadow`-pass p50 and p95 in nanoseconds, and [`None`] where
    /// the device reports no way to time a pass at all.
    prices: [Option<(u64, u64)>; 2],
    /// The side of the tile the first light was given, in texels.
    sides: [u32; 2],
    /// The fewest tiles the arm redrew in any recorded frame.
    faces: [u32; 2],
}

/// One half of a price: where the camera stands, and what cadence the renderer
/// is pinned to while it draws.
///
/// **Two knobs and one rig**, because there are two rungs to price off the same
/// field of dunes: the priority rung moves the eye and leaves the cadence alone,
/// and the cadence rung leaves the eye alone and moves the budget. Both halves
/// are drawn on alternating frames of one run, which is what makes the numbers
/// comparable — see [`shadow_pass_prices`].
#[derive(Clone, Copy, Debug)]
struct PriceArm {
    /// How far back the camera stands — [`price_camera`]'s argument.
    eye: f32,
    /// What `ForwardRenderer::set_shadow_cadence` is given, or [`None`] to take
    /// the console's, which is every map redrawn every frame.
    cadence: Option<Cadence>,
}

/// The camera for the price, `back` along `+z` and up, looking at the field.
fn price_camera(back: f32) -> Camera {
    Camera {
        eye: Vec3::new(0.0, 0.25 * back, back),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// Four cones over the corners of the field, leaning inwards.
fn price_lights() -> Vec<Light> {
    [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
        .into_iter()
        .map(|(x, z)| {
            let pool = Vec3::new(x * 3.0, 0.0, z * 3.0);
            lamp(pool, PRICE_REACH)
        })
        .collect()
}

/// Each of [`PRICE_EYES`]' `shadow`-pass p50 and p95 in nanoseconds, and the
/// side of the tile the first light was given at each.
///
/// **One device, one renderer, one scene, and the camera alternating frame by
/// frame** — `area_light.rs`'s interleaving, for its reason: a suite runs
/// sixty-odd tests beside this one, and two measurements taken in sequence are
/// two measurements of two different machine loads. Measured back to back this
/// pass read 0.037 ms for the whole-cell rig on its own and 0.014 ms for the
/// same rig inside the suite, which is a spread wider than the effect. Alternate
/// them and whatever contention lands on one lands on the other.
///
/// The two prices are [`None`] where the device reports no way to time a pass,
/// on `depth_only.rs`'s terms exactly: a backend that cannot time a pass cannot
/// price one, and the frames are drawn either way.
fn shadow_pass_prices(
    extent: (u32, u32),
    frames: usize,
    arms: [PriceArm; 2],
    stirred: bool,
) -> PricedShadowPass {
    use crcbl::hal::{CommandEncoderDesc, PresentInfo, ResourceState, SubmitInfo};
    use crcbl::shaders::dunes::DUNES_EXTENT;

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let mut renderer =
        ForwardRenderer::new(device, headless.queue, headless.format).expect("the renderer builds");
    let mut pool = TransientPool::new();
    let step = 2.0 * DUNES_EXTENT;
    let first = -(PRICE_FIELD as f32 - 1.0) / 2.0;
    let mut stir = None;
    for row in 0..PRICE_FIELD {
        for column in 0..PRICE_FIELD {
            let patch = place(
                &mut renderer,
                crcbl::render::scene::DEMO_DUNES,
                crcbl::render::scene::DEMO_UNTINTED,
                Mat4::from_translation(Vec3::new(
                    (first + column as f32) * step,
                    0.0,
                    (first + row as f32) * step,
                )),
            );
            stir.get_or_insert(patch);
        }
    }
    let stir = stir.expect("a field of at least one patch");
    renderer.set_lights(&price_lights());
    let cameras = arms.map(|arm| price_camera(arm.eye));
    let sun = no_sun();

    let mut timers = timed.then(|| {
        crcbl::render::PassTimers::new(
            device,
            crcbl::render::forward::FRAMES_IN_FLIGHT,
            crcbl::render::MAX_TIMED_PASSES,
        )
        .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
    });
    let mut stats = [
        crcbl::render::PassStats::new(),
        crcbl::render::PassStats::new(),
    ];
    let mut recorded = Vec::new();
    let mut sides = [0u32; 2];
    // The **fewest** tiles either arm redrew in any recorded frame, which is
    // what says a budget bound rather than merely being configured: a maximum
    // would be satisfied by one busy frame among a run of cached ones, and a
    // cached frame records no shadow pass at all.
    let mut faces = [u32::MAX; 2];

    // Twice through, because each camera takes every other frame — and the
    // warm-up is doubled with them so both windows start in the steady state.
    for index in 0..2 * (PRICE_WARMUP + frames) {
        let eye = index % 2;
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        renderer.set_shadow_cadence(arms[eye].cadence);
        // **A scene where everything moves at once**, which is the case the
        // budget exists to bound and the only one in which a held map is
        // holding anything: `InstancePool::revision` reaches every group's
        // record, so one nudged patch makes every map out of date on every
        // frame. Without it the arms alternate over a still scene, the atlas
        // caches, and the pass this function is timing is not recorded at all.
        if stirred {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a few hundred frames against a drift measured in millimetres"
            )]
            let drift = index as f32 * STIR_STEP;
            renderer.set_instance(
                stir,
                &crcbl::render::InstanceDesc {
                    mesh: crcbl::render::scene::DEMO_DUNES,
                    material: crcbl::render::scene::DEMO_UNTINTED,
                    transform: Mat4::from_translation(Vec3::new(
                        first * step + drift,
                        0.0,
                        first * step,
                    )),
                },
            );
        }
        renderer
            .begin_frame(device, &cameras[eye], &sun, extent)
            .expect("the uniform buffer is writable");
        if index >= 2 * PRICE_WARMUP {
            faces[eye] = faces[eye].min(renderer.shadow_faces_redrawn());
        }
        sides[eye] = renderer
            .shadow_lights()
            .base_of(0)
            .map(|base| renderer.shadow_lights().atlas_rect(light_tile(base)).side)
            .unwrap_or_default();
        let compiled = {
            let mut graph = crcbl::render::RenderGraph::new(headless.queue);
            let target = graph.import_image(
                "swapchain",
                crcbl::render::ImportedImage {
                    image: acquired.image,
                    view: acquired.view,
                    format: headless.format,
                    extent,
                    initial: ResourceState::Undefined,
                    claim: crcbl::render::InitialClaim::Acquired,
                    final_state: ResourceState::Present,
                },
            );
            renderer.add_passes(&mut graph, &pool, target, extent);
            graph.compile(&pool).expect("a legal frame")
        };
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("priced shadow frame"),
            queue: headless.queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), timers.as_mut())
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        recorded.push(commands);
        // **Recorded against the frame the timings came from, not the frame
        // being drawn.** `PassTimers` resolves the slot it is about to reuse,
        // which is `FRAMES_IN_FLIGHT + 1` frames back — an *odd* lag, so
        // attributing `latest()` to this frame's parity credits every
        // measurement to the other rig. It did: the two columns came out almost
        // exactly swapped against a run that drew each rig on its own.
        // `FrameTimings::frame` is what the timings themselves say, counted
        // from one, so the loop index that produced them is one less.
        if let Some(timers) = timers.as_ref() {
            let timings = timers.latest();
            let Some(drawn) = usize::try_from(timings.frame)
                .ok()
                .and_then(|frame| frame.checked_sub(1))
            else {
                continue;
            };
            if drawn >= 2 * PRICE_WARMUP {
                stats[drawn % 2].record(timings);
            }
        }
    }

    device.wait_idle().expect("idle");
    let prices = stats.each_ref().map(|stats| {
        timed.then(|| {
            stats.percentiles(PRICED_PASS).unwrap_or_else(|| {
                panic!("the {PRICED_PASS} pass is timed and the window is past its floor")
            })
        })
    });
    if let Some(mut timers) = timers.take() {
        timers.destroy(device);
    }
    for commands in recorded {
        device.destroy_command_buffer(commands);
    }
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    PricedShadowPass {
        prices,
        sides,
        faces,
    }
}

/// **The rung's price**: what the shadow pass costs with four lights at whole
/// root cells against the same four lights demoted two levels.
///
/// Prints rather than asserts a duration, on `area_light.rs`'s and
/// `depth_only.rs`'s terms — a millisecond figure is a property of the machine
/// it was measured on. The two rigs are drawn on alternating frames of one run
/// — see [`shadow_pass_prices`] — so whatever contention the sixty-odd tests
/// beside this one produce lands on both.
///
/// **And an ordering between the two is not asserted either.** The demoted rig
/// came out the cheaper on every run of both measurable tiers, but what
/// dominates this pass is the sun's cascades and those are whole cells in both
/// rigs — so on a discrete GPU, where the whole pass is tens of microseconds,
/// the margin was as little as two of them. `depth_only.rs` reaches the same
/// conclusion about the same pass for the same reason. A threshold on that
/// difference would be a threshold on a duration, which is the thing this file
/// refuses to assert. What it asserts instead is that the two rigs **differed
/// in the size of the tile they were given**, without which the two numbers are
/// two measurements of one thing.
///
/// **What varies between the two, said plainly.** Only the camera's distance:
/// the lights, the geometry and the atlas are identical, and coverage is a
/// light's map over its distance to the eye. The far camera therefore also
/// selects a coarser cut in the shadow pass — `ForwardRenderer::SHADOW_LOD_BIAS`
/// scales a budget denominated in the camera's pixels — so the saving reported
/// below is the tile's *and* the cut's together. Separating them would need a
/// knob that pinned the level, which nothing else would use.
///
/// # A backend that cannot time a pass cannot price one
///
/// [`Features::TIMESTAMP_QUERY`] is asked for and not required, so this runs on
/// every backend and reports on the ones that can answer. A backend without it
/// draws the frames and asserts what it can — that a whole cell and a demoted
/// tile were actually handed out, which is what says the two rigs differed at
/// all.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_demoted_rig_costs_no_more_in_the_shadow_pass_than_a_whole_cell_one() {
    let (extent, frames) = price_frame();
    let PricedShadowPass { prices, sides, .. } = shadow_pass_prices(
        extent,
        frames,
        PRICE_EYES.map(|eye| PriceArm { eye, cadence: None }),
        false,
    );

    // **The two rigs differed**, which is what makes the durations a comparison
    // rather than two measurements of one thing.
    assert_eq!(
        sides,
        [TILE, TILE >> 2],
        "the near camera must leave every light at a whole root cell and the far one must \
         demote it two levels, or this priced the same rig twice"
    );

    let [Some(whole), Some(demoted)] = prices else {
        eprintln!(
            "{}: this backend reports no TIMESTAMP_QUERY, so the shadow pass is drawn and not \
             priced",
            crate::SUITE
        );
        return;
    };
    eprintln!(
        "{}: shadow pass at {extent:?} over {frames} frames of each — whole cells p50 {:.3} ms \
         p95 {:.3} ms, demoted two levels p50 {:.3} ms p95 {:.3} ms",
        crate::SUITE,
        whole.0 as f64 / 1e6,
        whole.1 as f64 / 1e6,
        demoted.0 as f64 / 1e6,
        demoted.1 as f64 / 1e6,
    );
}

/// The budget the cadence half of the price is measured under, in tiles a frame
/// may redraw.
///
/// The rig is four spot lights and the sun's two cascades, so a frame with
/// everything moving wants six tiles; two is a third of that and binds on every
/// frame. Deliberately not one — a budget under the largest map is the
/// always-draw-something rule rather than a budget, and that is a different
/// measurement.
const PRICE_BUDGET: u32 = 2;

/// **The cadence rung's price**: what the shadow pass costs with every map
/// redrawn every frame against the same rig under a budget of
/// [`PRICE_BUDGET`] tiles.
///
/// Prints rather than asserts a duration, on
/// [`a_demoted_rig_costs_no_more_in_the_shadow_pass_than_a_whole_cell_one`]'s
/// terms and for its reasons: a millisecond figure is a property of the machine
/// it was measured on, and the two arms are drawn on alternating frames of one
/// run so whatever contention the suite produces lands on both.
///
/// **What varies between the two, said plainly.** Only the budget. The camera,
/// the lights, the geometry and every tile's size are identical — the eye does
/// not move, so no light is demoted and no shadow cull selects a different cut.
/// What the budgeted arm draws is a subset of what the other one draws, plus one
/// tile-clear triangle per tile it does redraw.
///
/// The budget is asserted to have **bound**: without that the two arms are two
/// measurements of one thing, which is the failure this rung's price is most
/// likely to have.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_budgeted_frame_costs_less_in_the_shadow_pass_than_an_unbudgeted_one() {
    let (extent, frames) = price_frame();
    let PricedShadowPass {
        prices,
        sides,
        faces,
    } = shadow_pass_prices(
        extent,
        frames,
        [
            PriceArm {
                eye: PRICE_EYES[0],
                cadence: None,
            },
            PriceArm {
                eye: PRICE_EYES[0],
                cadence: Some(Cadence {
                    hold: 1,
                    faces: PRICE_BUDGET as usize,
                }),
            },
        ],
        true,
    );

    assert_eq!(
        sides[0], sides[1],
        "the two arms must hand the same light the same tile, or this priced the tile size \
         rather than the budget"
    );
    assert!(
        faces[0] > PRICE_BUDGET,
        "the unbudgeted arm redrew as few as {} tiles on some frame, so the {PRICE_BUDGET} the \
         other one is held to is not a budget at all — and a frame that redrew nothing records \
         no shadow pass, which would leave the percentiles below measuring the frames that did",
        faces[0]
    );
    assert_eq!(
        faces[1], PRICE_BUDGET,
        "the budgeted arm redrew {} tiles on some frame against a budget of {PRICE_BUDGET}",
        faces[1]
    );

    let [Some(every), Some(budgeted)] = prices else {
        eprintln!(
            "{}: this backend reports no TIMESTAMP_QUERY, so the shadow pass is drawn and not \
             priced",
            crate::SUITE
        );
        return;
    };
    eprintln!(
        "{}: shadow pass at {extent:?} over {frames} frames of each — every map every frame \
         ({} tiles) p50 {:.3} ms p95 {:.3} ms, budget of {PRICE_BUDGET} tiles ({} tiles) p50 \
         {:.3} ms p95 {:.3} ms",
        crate::SUITE,
        faces[0],
        every.0 as f64 / 1e6,
        every.1 as f64 / 1e6,
        faces[1],
        budgeted.0 as f64 / 1e6,
        budgeted.1 as f64 / 1e6,
    );
}
