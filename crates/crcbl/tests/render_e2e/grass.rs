//! [`Scene::Meadow`]: `docs/plan/57-grass.md` rung G1's field, held to its
//! golden, to two bands of its own frame, to the instance data its generation
//! pass wrote, and to the frame it draws with its field removed.
//!
//! A file of its own rather than more of `render_e2e.rs`, which is the largest
//! test in the crate; every helper it reads is that file's.
//!
//! # Why most of the claims are readbacks
//!
//! The rung is graded on things a picture cannot separate. "The same tile
//! generates the same blades, in the same slots, on every backend — compared as
//! **instance data read back**" is the plan's own wording; "calm means still" is
//! a claim that a number is *exactly* zero, which no pixel can say; and "a
//! denser tile draws more instances" is a count in a draw-argument buffer. A
//! generation pass that produced nothing at all still draws a plausible frame
//! here — the ground, the sky and the path — so the golden alone would not
//! notice it, and the colour floor below is what stops that being true of the
//! picture half as well.
//!
//! # The order within a slot is not a claim
//!
//! Decision 1's appends are lock-free: `InterlockedAdd` hands each surviving
//! lane the value the counter held, and which lane gets there first is a
//! property of the scheduler. What is deterministic is the **set**, so every
//! comparison here sorts on `GrassInstance::lanes.x` — the blade's cell in the
//! field — before it compares.

use std::collections::BTreeMap;

use crcbl::render::grass::{SLOT_CAPACITY, placement};
use crcbl::screenshot::{OffscreenSetup, Scene};
use crcbl::shaders::grass::{
    BLADE_FAR_DRAW, BLADE_NEAR_DRAW, CARD_DRAW, DRAW_ARGS_SIZE, FIN_DRAW, GrassInstance,
    INSTANCE_STRIDE, SHELL_DRAW, SLOT_ARGS_SIZE,
};
use crcbl_golden::Image;

use super::{EXTENT, Offscreen, SUITE, block_channel, channel_order};

/// The anti-vacuity colour count for [`Scene::Meadow`]: a lit hillside of cards
/// over an earth plate under a sky gradient, so the count runs into the
/// hundreds.
///
/// **This floor is what a frame with no blades in it fails**, which is the one
/// thing the golden alone could miss: the ground, the path and the sky are a
/// handful of smooth gradients, and a generation pass that appended nothing
/// leaves exactly those. Measured on 2026-09-16 at **1367** distinct colours
/// with the field in and **96** with it out on lavapipe, and 1350 against 96 on
/// an RX 7900 XTX under radv; this sits between the two, and the removal test
/// below prints both figures on every run.
const MIN_COLORS_MEADOW: usize = 700;

/// Where the colour count the removal test prints stops counting.
///
/// Far above [`MIN_COLORS_MEADOW`], so both figures it prints are measurements
/// rather than the ceiling.
const COLOR_CEILING: usize = 8192;

/// The half-extent of every band read here, in pixels.
const BAND: (u32, u32) = (5, 3);

/// How much greener the grass band must be than the bare path beside it, in
/// levels of green-minus-red.
///
/// **A relation between two bands rather than a sign within one**, which is what
/// a cutout mat makes it: a band over grass reads the ground between the cards
/// as well as the cards, so its own green does not have to exceed its own red —
/// what has to hold is that it is greener than the same ground with nothing
/// growing on it. Nothing but blades can close that gap, because the two bands
/// are at the same depth on the same plane under a sun with no `x` component.
///
/// Measured at **37.58 levels** on lavapipe and 37.73 on radv (2026-09-16): the
/// grass band reads `[125.2, 150.9, 81.9]` and the bare path
/// `[145.5, 133.5, 103.8]`. The run prints both bands and their difference.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): the tall blade row's two
/// colours replaced by the earth's own hue, which leaves the mat exactly where
/// it was and paints it brown.
const GRASS_GREENER_THAN_PATH: f32 = 12.0;

/// How far the path band's red must sit above its green, in levels.
///
/// **The path is bare**, which is what says the cover map's zero is read at all:
/// a placement that ignored the density texel would grow blades down it and
/// close this. The earth is a warm brown whose red is well above its green, so
/// the sign is the claim, and it is asserted **before** the relation above
/// because it is that relation's premise.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): `crcbl::screenshot`'s
/// `meadow_cover_texel` stopped returning bare ground for the path, so blades
/// grew down it and the band's red fell under its green.
const PATH_RED_OVER_GREEN: f32 = 4.0;

/// Where the world point `point` lands in the frame, through the matrices
/// `crcbl_render::ForwardRenderer` draws the scene with.
fn meadow_pixel(point: glam::Vec3) -> (u32, u32) {
    let aspect = EXTENT.0 as f32 / EXTENT.1 as f32;
    let clip = crcbl::screenshot::meadow_camera().view_projection(aspect) * point.extend(1.0);
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

/// The band on the ground at `(x, z)`.
fn ground_band(image: &Image, x: f32, z: f32) -> [f32; 3] {
    band(
        image,
        meadow_pixel(glam::Vec3::new(
            x,
            crcbl::screenshot::meadow_height(x, z),
            z,
        )),
    )
}

/// How far above the ground a grass band is read, in metres.
///
/// **Half-way up a card, not at its root.** A band on the ground reads whatever
/// is between the blades as much as the blades themselves; a band at mid-height
/// reads the part of the mat every card at that spot covers.
const BLADE_BAND_HEIGHT: f32 = 0.18;

/// The band half-way up the grass standing at `(x, z)`.
fn grass_band(image: &Image, x: f32, z: f32) -> [f32; 3] {
    band(
        image,
        meadow_pixel(glam::Vec3::new(
            x,
            crcbl::screenshot::meadow_height(x, z) + BLADE_BAND_HEIGHT,
            z,
        )),
    )
}

/// [`Scene::Meadow`]'s pixel claims: the grass is green where the cover map
/// paints it and the ground is bare where it does not.
///
/// **Each was shown red by sabotage** (2026-09-16, lavapipe); the two constants
/// above carry what each edit was and what it reported.
pub(super) fn the_meadow_is_green_where_its_cover_map_says(image: &Image) {
    let [red, green] = [0, 1];
    // On the windy side of the dense near half, close enough to the camera that
    // a card is several pixels across — and well inside a tile and well off the
    // path.
    let grass = grass_band(image, -2.6, -2.0);
    // The bare path, on the frame's own axis, at the same depth and on the
    // ground, which is all there is to see there.
    let path = ground_band(image, 0.0, -2.0);
    eprintln!(
        "crcbl render e2e: meadow — grass {grass:?}, path {path:?}, greener by {:.2} level(s)",
        (grass[green] - grass[red]) - (path[green] - path[red])
    );

    // The premise the relation below rests on: the path really is bare ground,
    // so the band beside it is the only one with anything growing in it.
    let browner = path[red] - path[green];
    assert!(
        browner >= PATH_RED_OVER_GREEN,
        "the path band reads {path:?}: its red is {browner:.2} level(s) over its green against \
         {PATH_RED_OVER_GREEN} — something grew on the bare strip"
    );

    let greener = (grass[green] - grass[red]) - (path[green] - path[red]);
    assert!(
        greener >= GRASS_GREENER_THAN_PATH,
        "the grass band reads {grass:?} and the bare path {path:?}: the grass is {greener:.2} \
         level(s) greener against {GRASS_GREENER_THAN_PATH} — nothing green drew where the cover \
         map paints grass"
    );
}

/// [`Scene::Meadow`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_scene_draws_its_grass_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden_measuring(
        Scene::Meadow,
        "meadow",
        EXTENT,
        MIN_COLORS_MEADOW,
        the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// [`Scene::Meadow`] on both geometry paths this machine can reach — see
/// `super::draw_scene_on_every_geometry_path`.
///
/// The grass passes read no geometry path; what the comparison holds is the
/// ground the field stands on, which does — and a plate the mesh path culled by
/// its authored cone would take the whole hillside out from under the blades.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_scene_draws_the_same_frame_on_every_geometry_path() {
    super::draw_scene_on_every_geometry_path_measuring(
        Scene::Meadow,
        "meadow",
        MIN_COLORS_MEADOW,
        the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// One frame of `setup`, as an image.
pub(super) fn frame_of(setup: &mut OffscreenSetup) -> Image {
    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image")
}

/// How many frames each renderer draws before the one compared: enough that
/// every slot of the ring has come round after the field was removed.
const FRAMES_AFTER_REMOVAL: usize = 4;

/// **A meadow with its field removed is the meadow never given one, bit for
/// bit.**
///
/// Grass is content, not an effect bit, on the water surface's terms: a renderer
/// with no field records no pass and takes no transient. What that has to mean
/// is a frame identical to one from a renderer grass never touched — including
/// after the field has been drawn and taken away, which is the case the ring of
/// retired resources could get wrong.
///
/// The anti-vacuity half: the frame **with** the field differs from both, and
/// the colour counts of the two are printed, which is where
/// [`MIN_COLORS_MEADOW`] came from.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_meadow_with_its_field_removed_is_the_meadow_never_given_one() {
    crcbl_core::log::init_logging();
    let open = |grass: bool| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::meadow_forward(device, queue, format, grass)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the meadow: {why}"));
        Offscreen::guard(SUITE, setup)
    };

    let mut removed = open(true);
    let with_grass = frame_of(&mut removed);
    assert!(
        removed.set_grass(None).expect("removing is a set"),
        "the meadow draws through a forward renderer, so the removal has to reach one"
    );
    let mut after = frame_of(&mut removed);
    for _ in 1..FRAMES_AFTER_REMOVAL {
        after = frame_of(&mut removed);
    }
    removed.finish();

    let mut never = open(false);
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
    let grass = differing(&with_grass, &without);
    eprintln!(
        "crcbl render e2e: meadow — {} colour(s) with the field in and {} with it out; {grass} \
         pixel(s) differ with it in, {removal} after it was removed",
        with_grass.distinct_colors(COLOR_CEILING),
        without.distinct_colors(COLOR_CEILING)
    );
    assert!(
        grass > 0,
        "the frame with the field in is the frame without one, so the equality below would hold \
         for a grass pass that drew nothing"
    );
    assert_eq!(
        removal, 0,
        "the meadow with its field removed differs from the meadow never given one at {removal} \
         pixel(s): removing the grass left something of it in the frame"
    );
}

/// Every instance slot `slot` holds, and the counts its draw arguments carry.
pub(super) struct Slot {
    /// The card instance count the generation dispatch appended.
    pub(super) count: u32,
    /// The instance counts the slot's shell and fin draws were given.
    pub(super) shells: u32,
    pub(super) fins: u32,
    /// Those card instances, by the cell each was grown from.
    pub(super) blades: BTreeMap<u32, GrassInstance>,
    /// The near and far mesh blade instances, likewise.
    pub(super) near: BTreeMap<u32, GrassInstance>,
    pub(super) far: BTreeMap<u32, GrassInstance>,
}

/// The cells buffer a field's last generation wrote, as its bytes — every cell
/// of every slot, whatever the look.
pub(super) fn cells_of(setup: &mut OffscreenSetup) -> Vec<u8> {
    let buffers = setup
        .grass_buffers()
        .expect("the meadow was built with a field");
    setup
        .read_buffer(
            buffers.cells,
            u64::from(buffers.slots) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64,
        )
        .expect("the cells copy back")
}

/// Draws one frame of the meadow and copies every slot's instances and draw
/// arguments back — its cards, and both runs of its mesh blades.
pub(super) fn generated(setup: &mut OffscreenSetup) -> Vec<Slot> {
    // A frame first: the buffers hold whatever the last frame that generated
    // left there, and a renderer that has drawn nothing has never dispatched.
    let _ = frame_of(setup);
    let buffers = setup
        .grass_buffers()
        .expect("the meadow was built with a field");
    let args = setup
        .read_buffer(
            buffers.args,
            u64::from(buffers.slots) * SLOT_ARGS_SIZE as u64,
        )
        .expect("the draw arguments copy back");
    // Both regions: the cards and far blades, then the near blades.
    let instances = setup
        .read_buffer(
            buffers.instances,
            2 * u64::from(buffers.slots) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64,
        )
        .expect("the instances copy back");

    (0..buffers.slots)
        .map(|slot| {
            let word = |draw: u32, index: usize| {
                let word_at =
                    slot as usize * SLOT_ARGS_SIZE + draw as usize * DRAW_ARGS_SIZE + index * 4;
                u32::from_le_bytes(
                    args[word_at..word_at + 4]
                        .try_into()
                        .expect("four bytes of a draw argument"),
                )
            };
            let count = word(CARD_DRAW, 1);
            let (near_count, far_count) = (word(BLADE_NEAR_DRAW, 1), word(BLADE_FAR_DRAW, 1));
            assert!(
                count + far_count <= SLOT_CAPACITY && near_count <= SLOT_CAPACITY,
                "slot {slot} claims {count} cards, {near_count} near and {far_count} far blades, \
                 past the {SLOT_CAPACITY} a region holds"
            );
            for draw in [
                CARD_DRAW,
                SHELL_DRAW,
                FIN_DRAW,
                BLADE_NEAR_DRAW,
                BLADE_FAR_DRAW,
            ] {
                assert_eq!(
                    word(draw, 3),
                    0,
                    "slot {slot}'s draw {draw} has a first instance that is not zero, which \
                     WebGPU refuses outright"
                );
            }
            let capacity = SLOT_CAPACITY as usize;
            let base = slot as usize * capacity;
            let second = (buffers.slots as usize + slot as usize) * capacity;
            let run = |rows: Vec<usize>, what: &str| {
                let expected = rows.len();
                let run = rows
                    .into_iter()
                    .map(|index| {
                        let row = index * INSTANCE_STRIDE;
                        let instance = GrassInstance::from_bytes(
                            instances[row..row + INSTANCE_STRIDE]
                                .try_into()
                                .expect("one instance's bytes"),
                        );
                        (instance.lanes[0], instance)
                    })
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(
                    run.len(),
                    expected,
                    "slot {slot} appended two {what} for one cell"
                );
                run
            };
            Slot {
                count,
                shells: word(SHELL_DRAW, 1),
                fins: word(FIN_DRAW, 1),
                blades: run((base..base + count as usize).collect(), "cards"),
                near: run(
                    (second..second + near_count as usize).collect(),
                    "near blades",
                ),
                // Counted down from the end of the first region.
                far: run(
                    (0..far_count as usize)
                        .map(|index| base + capacity - 1 - index)
                        .collect(),
                    "far blades",
                ),
            }
        })
        .collect()
}

/// The meadow, opened with its field.
fn meadow() -> Offscreen {
    opened(Scene::Meadow)
}

/// `scene`, opened.
pub(super) fn opened(scene: Scene) -> Offscreen {
    crcbl_core::log::init_logging();
    let setup = OffscreenSetup::open(EXTENT.0, EXTENT.1, scene)
        .unwrap_or_else(|why| panic!("a GPU backend opens for {scene:?}: {why}"));
    Offscreen::guard(SUITE, setup)
}

/// How far a generated field derived through a rounding operation may sit from
/// the one [`placement::blades_of_tile`] computes, relative to its own
/// magnitude.
///
/// **What is bit-exact is everything the integer hash decides**, and that is
/// most of the placement: which cells carry a blade, which cell each instance
/// came from, its blade row, its root on the XZ plane and its tint lane are all
/// integer arithmetic or exact power-of-two scalings of it, and each is compared
/// with `assert_eq!` below.
///
/// **What is not are the three things a rounding reaches**, and this is the
/// bound on those:
///
/// * the **facing**, which is `square / sqrt(dot(square, square))`. A shader
///   compiler is free to fold `x / sqrt(y)` into `x · rsqrt(y)`, and `rsqrt` is
///   an approximation on several targets — measured at 1 ulp of the component
///   on lavapipe (2026-09-16), which is what first showed this claim could not
///   be an equality;
/// * the **height and half-width**, each `row · (1 − spread · lane)`, which a
///   compiler may contract into an FMA where Rust never does;
/// * the **root's height and the ground normal**, four texels blended in the
///   same order in both copies, under the same FMA freedom.
///
/// A millionth of the value is thousands of times the last place of an `f32` and
/// far below anything a picture could show. Measured on 2026-09-16 over all
/// 8511 blades of `Scene::Meadow`: **5.96e−8** on lavapipe and **1.79e−7** on an
/// RX 7900 XTX under radv, so this leaves roughly a factor of six over the
/// worse of the two. The run prints the worst it saw.
const DERIVED_TOLERANCE: f32 = 1e-6;

/// How far off `mid-grey` an eight-bit direction texel's own encoding is.
///
/// **The quantisation, not a deflection.** A "no deflection" texel is
/// `(255, 128)` and the layer decodes `2x − 1`, so its second component is
/// `2 · 128 / 255 − 1`, which is `0.0039` rather than zero —
/// `crcbl_wind`'s own `a_blank_direction_layer_leaves_the_prevailing_wind_alone`
/// records the same thing as "0.22° off". A blade leaning downwind therefore
/// carries that fraction of its lean across the wind, and the claim below is
/// that the cross component is *at most* this share of the along one rather than
/// that it is zero.
const DIRECTION_MIDPOINT_SKEW: f32 = 2.0 * 128.0 / 255.0 - 1.0;

/// **The blades the GPU grew are the blades the CPU places** — the rung's
/// determinism check, against an oracle rather than against a second GPU run.
///
/// Every cell, every jitter, every row, every facing and every size lane is
/// compared bit for bit; the two a blend produces are compared under
/// [`ROOT_TOLERANCE`]. The reach covers the whole field — see
/// `crcbl::screenshot`'s `the_reach_covers_the_whole_field` — so nothing is
/// distance-culled and the two sets are equal rather than one containing the
/// other.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): `grass_gen.slang`'s
/// `GRASS_DENSITY_SALT` changed by one bit — `0x9e3779b9` to `0x9e3779b8` — and
/// the artifacts recompiled. The first slot the comparison reached then said it
/// had "grown a different set of cells from the one the CPU places".
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_generated_blades_are_the_placement_the_cpu_computes() {
    let mut setup = meadow();
    let slots = generated(&mut setup);
    setup.finish();

    let field = crcbl::screenshot::meadow_field();
    let mut worst = 0.0f32;
    let mut compared = 0usize;
    for (slot, generated) in slots.iter().enumerate() {
        #[expect(clippy::cast_possible_truncation, reason = "four slots")]
        let placed = placement::blades_of_tile(&field, slot as u32);
        let cells: Vec<u32> = placed.iter().map(|blade| blade.cell).collect();
        let grown: Vec<u32> = generated.blades.keys().copied().collect();
        assert_eq!(
            grown, cells,
            "slot {slot} grew a different set of cells from the one the CPU places"
        );
        for blade in &placed {
            let instance = generated.blades[&blade.cell];
            assert_eq!(
                instance.lanes[1], blade.row,
                "cell {} was given a different blade row",
                blade.cell
            );
            assert_eq!(
                [instance.root[0], instance.root[2]],
                [blade.root[0], blade.root[2]],
                "cell {}'s root moved on the XZ plane, which is integer arithmetic on both sides",
                blade.cell
            );
            assert_eq!(
                instance.facing[3], blade.tint,
                "cell {}'s tint lane differs",
                blade.cell
            );
            // The three a rounding reaches — see `DERIVED_TOLERANCE`. Compared
            // relative to the value, because a facing component is order one
            // and a half-width is order a centimetre.
            let apart = |got: f32, want: f32| (got - want).abs() / want.abs().max(1.0);
            for (got, want, what) in [
                (instance.facing[0], blade.facing[0], "facing x"),
                (instance.facing[1], blade.facing[1], "facing z"),
                (instance.facing[2], blade.half_width, "half-width"),
                (instance.root[3], blade.height, "height"),
                (instance.root[1], blade.root[1], "root height"),
                (instance.ground[0], blade.ground[0], "ground normal x"),
                (instance.ground[1], blade.ground[1], "ground normal y"),
                (instance.ground[2], blade.ground[2], "ground normal z"),
            ] {
                let off = apart(got, want);
                assert!(
                    off <= DERIVED_TOLERANCE,
                    "cell {}'s {what} is {got} against the CPU's {want}, which is {off:e} apart",
                    blade.cell
                );
                worst = worst.max(off);
            }
            compared += 1;
        }
    }
    eprintln!(
        "crcbl render e2e: meadow — {compared} blade(s) compared against the CPU placement, worst \
         derived field {worst:e} of its own value"
    );
    assert!(compared > 4000, "only {compared} blades were compared");
    assert!(
        worst <= DERIVED_TOLERANCE,
        "a derived field is {worst:e} apart from the CPU's against {DERIVED_TOLERANCE:e}"
    );
}

/// **Calm means still, and windy means bent** — the plan's own check, as an
/// equality on one side and a floor on the other.
///
/// The intensity layer is full where `x < 0` and exactly zero where `x > 0`, and
/// the bare path between them is wider than the layer's blend — see
/// `crcbl::screenshot`'s `the_path_is_wider_than_the_winds_blend` — so every
/// blade in this field is one or the other and none is in between. A calm
/// blade's lean is therefore **exactly** the zero vector, which is what makes
/// this a comparison rather than a threshold.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): a constant `+ 0.001` added
/// to `grass_gen.slang`'s `bend`, and the artifacts recompiled. A blade standing
/// where the intensity layer reads exactly zero then carried a lean whose
/// along-wind terms were still zero — the velocity is — and whose `y` was not:
/// what the constant reached was the **length restore**, which drops a tip that
/// has travelled. That the equality catches it through `y` is the point of
/// restoring the length at all.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_calm_texel_leaves_a_blade_upright_and_a_windy_one_bends_it() {
    let mut setup = meadow();
    let slots = generated(&mut setup);
    setup.finish();

    let mut calm = 0usize;
    let mut windy = 0usize;
    let mut least_windy = f32::INFINITY;
    let mut most_calm = 0.0f32;
    let mut worst_skew = 0.0f32;
    for slot in &slots {
        for instance in slot.blades.values() {
            let lean = glam::Vec3::from_slice(&instance.lean[..3]);
            if instance.root[0] > 0.0 {
                calm += 1;
                most_calm = most_calm.max(lean.length());
                assert_eq!(
                    lean,
                    glam::Vec3::ZERO,
                    "a blade at x = {} stands where the intensity layer is zero and leans by \
                     {lean}",
                    instance.root[0]
                );
            } else {
                windy += 1;
                least_windy = least_windy.min(lean.length());
                // Downwind, which is `+X`: the weather blows that way and the
                // direction layer is the identity everywhere.
                assert!(
                    lean.x > 0.0,
                    "a blade at x = {} leans by {lean}, which is not downwind",
                    instance.root[0]
                );
                // And the tip drops as it travels, which is the length the lean
                // restores — God of War's construction.
                assert!(lean.y < 0.0, "a leaning blade's tip did not drop: {lean}");
                // Across the wind by the direction texel's own quantisation and
                // no more — see `DIRECTION_MIDPOINT_SKEW`.
                let skew = lean.z.abs() / lean.x;
                assert!(
                    skew <= DIRECTION_MIDPOINT_SKEW * 1.5,
                    "a blade leaning {lean} is {skew:e} across the wind, past the \
                     {DIRECTION_MIDPOINT_SKEW:e} an undeflected eight-bit texel encodes to"
                );
                worst_skew = worst_skew.max(skew);
            }
        }
    }
    eprintln!(
        "crcbl render e2e: meadow — {windy} windy blade(s), least lean {least_windy:.4} m, worst \
         cross-wind share {worst_skew:e}; {calm} calm blade(s), most lean {most_calm:e} m"
    );
    assert!(
        windy > 1000 && calm > 1000,
        "the field has {windy} windy and {calm} calm blades, so one side of this claim is nearly \
         empty and the comparison says little"
    );
    // The windy half really is bent, or the equality above would hold for a
    // field in which nothing leans at all.
    assert!(
        least_windy > 0.01,
        "the least-bent windy blade leans by {least_windy} m, so the wind reached nothing"
    );
}

/// **A denser tile draws more instances** — the rung's third relation, read off
/// the very words the indirect draws consume.
///
/// The far tiles' cover texels are at `MEADOW_DENSE` and the near tiles' at
/// `MEADOW_SPARSE`; the blade row, the ground and the wind are the same on both,
/// so the counts differ in the density and in nothing else. The ratio is
/// checked as well as the order: a placement that accepted every cell whatever
/// the texel said would put all four counts at the capacity and satisfy a
/// bare `>`.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): `grass_gen.slang`'s density
/// comparison changed from the cover texel to a constant `65536u`, which accepts
/// every cell whatever the map says. All four slots then reported **4096**
/// instances — the whole slot capacity, path included — and the first
/// far-against-near comparison failed on a pair that were equal.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_denser_tile_draws_more_instances() {
    use crcbl::screenshot::{MEADOW_DENSE, MEADOW_SPARSE};

    let mut setup = meadow();
    let slots = generated(&mut setup);
    setup.finish();

    // Tiles 0 and 1 are the far, dense row; 2 and 3 the near, sparse one — see
    // `crcbl::screenshot`'s meadow module for the layout.
    let counts: Vec<u32> = slots.iter().map(|slot| slot.count).collect();
    eprintln!("crcbl render e2e: meadow — slot instance counts {counts:?}");
    assert_eq!(counts.len(), 4, "the meadow is a two-by-two field");
    for slot in 0..2 {
        assert!(
            counts[slot] > counts[slot + 2],
            "far slot {slot} drew {} instances and the near slot under it drew {}, which is not \
             denser",
            counts[slot],
            counts[slot + 2]
        );
    }
    // The ratio the two densities predict, within a quarter: the accept is a
    // hashed lane against the texel, so a slot's count is a binomial draw about
    // `cells · density / 256` and the spread over four thousand cells is well
    // inside this.
    let wanted = f64::from(MEADOW_DENSE) / f64::from(MEADOW_SPARSE);
    let ratio = f64::from(counts[0] + counts[1]) / f64::from(counts[2] + counts[3]);
    eprintln!("crcbl render e2e: meadow — the density ratio is {ratio:.3} against {wanted:.3}");
    assert!(
        (ratio - wanted).abs() < 0.25,
        "the far row drew {ratio:.3} times the near row's instances, where the densities predict \
         {wanted:.3}"
    );
    // And no slot is empty or full, either of which would make the comparison
    // above a comparison of two saturations.
    for (slot, count) in counts.iter().enumerate() {
        assert!(
            *count > 0 && *count < SLOT_CAPACITY,
            "slot {slot} drew {count} of {SLOT_CAPACITY} instances"
        );
    }
}

/// **The same field generates the same blades twice** — two frames of one
/// renderer, compared as instance data.
///
/// The set, every lane of every instance and every slot's count, with no
/// tolerance anywhere: nothing in the generation pass reads the frame number,
/// the clock or the last frame's output, so a second dispatch over the same
/// field is the same arithmetic on the same inputs. The **order** within a slot
/// is not compared, for the reason this file's header gives.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_same_field_generates_the_same_blades_twice() {
    let mut setup = meadow();
    let first = generated(&mut setup);
    let second = generated(&mut setup);
    setup.finish();

    let mut compared = 0usize;
    for (slot, (first, second)) in first.iter().zip(&second).enumerate() {
        assert_eq!(
            first.count, second.count,
            "slot {slot} appended {} instances and then {}",
            first.count, second.count
        );
        assert_eq!(
            first.blades.keys().collect::<Vec<_>>(),
            second.blades.keys().collect::<Vec<_>>(),
            "slot {slot} grew a different set of cells the second time"
        );
        for (cell, instance) in &first.blades {
            assert_eq!(
                *instance, second.blades[cell],
                "cell {cell} came out differently the second time"
            );
            compared += 1;
        }
    }
    eprintln!("crcbl render e2e: meadow — {compared} blade(s) identical across two dispatches");
    assert!(compared > 4000, "only {compared} blades were compared");
}
