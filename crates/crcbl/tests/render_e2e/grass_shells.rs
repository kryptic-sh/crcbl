//! [`Scene::MeadowShells`]: `docs/plan/57-grass.md` rung G3's look, held to its
//! golden and to four relations with the frames around it — the card meadow of
//! the same field, the same shells in no wind, the same shells with no fins,
//! and the same hillside with no grass at all.
//!
//! A file of its own beside `grass.rs`, whose readback helpers it reads.
//!
//! # What each claim compares, and why against another frame
//!
//! A frame of shells is a plausible picture whatever its shells got wrong: a
//! stack that ignored the placement, a layer that leaned in calm air, or fins
//! that never stood would each still draw a green hillside. So every claim here
//! is a relation to a second frame that differs in exactly the one thing the
//! claim is about, and each prints what it measured.

use crcbl::render::grass::{BladeLook, Shells, placement};
use crcbl::screenshot::{MeadowWind, OffscreenSetup, Scene};
use crcbl::shaders::grass::{DEFAULT_SHELLS, GrassInstance, INSTANCE_STRIDE};
use crcbl_golden::Image;

use super::grass::{Slot, cells_of, frame_of, generated, opened};
use super::{EXTENT, Offscreen, SUITE};

/// The anti-vacuity colour count for [`Scene::MeadowShells`]: a lit hillside of
/// shaded, patched strands over an earth plate under a sky.
///
/// **Measured on 2026-09-17 at 8083 with the field in on lavapipe and 8106 on
/// radv**, against the 96 of the bare hillside `Scene::Meadow`'s removal test
/// prints; [`a_look_switch_leaves_the_placement_bit_identical`] prints this
/// frame's figure beside the card meadow's on every run.
const MIN_COLORS_MEADOW_SHELLS: usize = 3000;

/// [`Scene::MeadowShells`] drawn, against the reference in `tests/golden/`.
///
/// The band claim is the card meadow's own, and holds for the same reason: the
/// cover map paints grass on one side of the path band and none on it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_shells_scene_draws_its_strands_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden_measuring(
        Scene::MeadowShells,
        "meadow_shells",
        EXTENT,
        MIN_COLORS_MEADOW_SHELLS,
        super::grass::the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// [`Scene::MeadowShells`] on both geometry paths this machine can reach — the
/// shells read no geometry path, and the ground they stand on does.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_shells_scene_draws_the_same_frame_on_every_geometry_path() {
    super::draw_scene_on_every_geometry_path_measuring(
        Scene::MeadowShells,
        "meadow_shells",
        MIN_COLORS_MEADOW_SHELLS,
        super::grass::the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// The meadow's hillside with `field` on it in `wind`, opened at `extent`.
fn meadow_with(
    extent: (u32, u32),
    field: Option<crcbl::render::grass::GrassField>,
    wind: MeadowWind,
) -> Offscreen {
    crcbl_core::log::init_logging();
    let setup = OffscreenSetup::open_forward(extent.0, extent.1, move |device, queue, format| {
        crcbl::screenshot::meadow_forward_with(device, queue, format, field.as_ref(), wind)
    })
    .unwrap_or_else(|why| panic!("a GPU backend opens for the meadow: {why}"));
    Offscreen::guard(SUITE, setup)
}

/// One frame of the meadow with `field` in `wind`, at `extent`.
fn frame_with(
    extent: (u32, u32),
    field: Option<crcbl::render::grass::GrassField>,
    wind: MeadowWind,
) -> Image {
    let mut setup = meadow_with(extent, field, wind);
    let image = frame_of(&mut setup);
    setup.finish();
    image
}

/// How many pixels of `left` and `right` differ inside the columns `columns`
/// and the rows `rows`.
fn differing_in(
    left: &Image,
    right: &Image,
    columns: core::ops::Range<u32>,
    rows: core::ops::Range<u32>,
) -> usize {
    rows.flat_map(|y| columns.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| left.pixel(x, y) != right.pixel(x, y))
        .count()
}

/// **A look switch leaves the placement bit-identical** — decision 3's own test
/// of the three looks, on the GPU.
///
/// The card meadow and the shell meadow are one description but for their rows'
/// looks and styles (`crcbl::screenshot`'s
/// `the_shell_meadow_is_the_card_meadow_but_its_rows`). What that has to mean
/// on the device:
///
/// * the `cells` buffer the two generation passes wrote is **the same bytes**,
///   every cell of every slot — the blades, their roots, sizes, facings, tints
///   and leans;
/// * every card the card meadow appended is the cell of the same index, bit for
///   bit, and every cell carrying a blade was appended — so the two buffers
///   hold one set of blades;
/// * the shell meadow appended no card, and gave every slot's shell draw the
///   field's shell count and its fin draw one; the card meadow gave neither.
///
/// **Shown red by sabotage** (2026-09-17, lavapipe): `grass_gen.slang` zeroing
/// a shell row's lean before writing its cell, and the artifacts recompiled.
/// 55243 cell bytes then differed, while the shell frame's colour count stayed
/// where it was — a shell's bend is sampled in its own vertex stage, so the
/// readback is what sees a cell's lean.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_look_switch_leaves_the_placement_bit_identical() {
    let mut cards = opened(Scene::Meadow);
    let card_slots = generated(&mut cards);
    let card_cells = cells_of(&mut cards);
    let card_colours = frame_of(&mut cards).distinct_colors(8192);
    cards.finish();

    let mut shells = opened(Scene::MeadowShells);
    let shell_slots = generated(&mut shells);
    let shell_cells = cells_of(&mut shells);
    let shell_colours = frame_of(&mut shells).distinct_colors(8192);
    shells.finish();

    eprintln!(
        "crcbl render e2e: meadow shells — {} colour(s) against the card meadow's {}",
        shell_colours, card_colours
    );

    let cells: Vec<GrassInstance> = card_cells
        .chunks_exact(INSTANCE_STRIDE)
        .map(|row| GrassInstance::from_bytes(row.try_into().expect("one row")))
        .collect();
    let grown = cells.iter().filter(|cell| cell.root[3] > 0.0).count();
    let differing = card_cells
        .iter()
        .zip(&shell_cells)
        .filter(|(left, right)| left != right)
        .count();
    eprintln!(
        "crcbl render e2e: meadow shells — {} cell byte(s) compared, {differing} differ; {grown} \
         cell(s) carry a blade",
        card_cells.len()
    );
    assert!(
        grown > 4000,
        "only {grown} cells carry a blade, so an equality of the buffers says little"
    );
    assert_eq!(
        differing, 0,
        "the shell meadow's generation wrote {differing} cell byte(s) the card meadow's did not: a \
         look switch moved the placement"
    );

    let mut appended = 0usize;
    for (slot, (card, shell)) in card_slots.iter().zip(&shell_slots).enumerate() {
        let Slot { blades, .. } = card;
        for (cell, instance) in blades {
            assert_eq!(
                *instance, cells[*cell as usize],
                "the card appended for cell {cell} is not the cell's own row"
            );
        }
        appended += blades.len();
        assert_eq!(
            (card.shells, card.fins),
            (0, 0),
            "the card meadow gave slot {slot} a shell or a fin draw"
        );
        assert_eq!(
            shell.count, 0,
            "the shell meadow appended {} card(s) to slot {slot}",
            shell.count
        );
        assert_eq!(
            (shell.shells, shell.fins),
            (DEFAULT_SHELLS, 1),
            "the shell meadow's slot {slot} was not given its stack and its fins"
        );
    }
    assert_eq!(
        appended, grown,
        "the card meadow appended {appended} cards and {grown} cells carry a blade"
    );
    assert!(shell_colours >= MIN_COLORS_MEADOW_SHELLS);
}

/// The columns either side of the frame's centre line a calm-half comparison
/// leaves out, in pixels.
///
/// **The centre line is exact and the margin is for what reads across it.** The
/// camera and its target stand on `x = 0`, so a world point on the windy side
/// projects left of the frame's centre and nothing windy can land right of it
/// — but a post pass that reads a neighbourhood (the bloom chain, the
/// reflection march's blur) carries a pixel's difference a few pixels sideways.
/// Measured on 2026-09-17: 0 differing pixels right of the centre with a margin
/// of this many columns.
const CALM_MARGIN: u32 = 8;

/// **Calm shells stand upright** — decision 5's "calm means still", for a look
/// whose bend is applied in the vertex stage and so cannot be read back.
///
/// The shell meadow in its authored wind against the same field in no wind at
/// all. The intensity layer is exactly zero on the `x > 0` side, so every shell
/// layer and every fin standing there has to be where it stands in still air —
/// **the right half of the two frames is the same pixels**. The left half, in a
/// strong wind, has to differ, or the equality would hold for a stack nothing
/// bends.
///
/// **Shown red by sabotage** (2026-09-17, lavapipe): `grass.slang`'s
/// `grass_sheet_vertex` handed `grass_lean` a velocity with a tenth of the
/// weather's base speed added — a wind that no longer reads the intensity layer
/// — and the artifacts recompiled. 6492 pixels of the calm half then differed.
///
/// **A constant displacement is not this claim's to catch**, and was tried: a
/// centimetre added to every layer moves the still-air frame by the same
/// centimetre, so the two halves stay equal. What this holds is that the wind
/// reaches no layer the intensity layer calms.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn calm_shells_stand_upright_to_the_pixel() {
    let field = || Some(crcbl::screenshot::meadow_shells_field());
    let windy = frame_with(EXTENT, field(), MeadowWind::Windy);
    let still = frame_with(EXTENT, field(), MeadowWind::Calm);

    let centre = EXTENT.0 / 2;
    let calm = differing_in(&windy, &still, centre + CALM_MARGIN..EXTENT.0, 0..EXTENT.1);
    let bent = differing_in(&windy, &still, 0..centre - CALM_MARGIN, 0..EXTENT.1);
    eprintln!(
        "crcbl render e2e: meadow shells — {bent} pixel(s) differ on the windy half, {calm} on \
         the calm half"
    );
    assert!(
        bent > 500,
        "only {bent} pixels of the windy half moved in a strong wind, so the calm half's equality \
         says little"
    );
    assert_eq!(
        calm, 0,
        "{calm} pixel(s) of the calm half differ from the same shells in still air: a layer \
         standing where the intensity layer is zero leaned"
    );
}

/// The frame's rows a fin claim reads, as fractions of its height from the
/// top: the hillside the fins stand on.
///
/// **Measured, not chosen** (2026-09-17, lavapipe and radv, at
/// [`CLAIM_EXTENT`]): drawn with and without fins, the first row that differs
/// anywhere is a third of the way down the frame and the last is row 700 of
/// 768, which the test prints as the lowest row a fin reached. A fin's fade
/// follows the view's elevation over the ground, which is a curve across the
/// frame rather than a row, so the band's edge is where that curve's lowest
/// point landed.
const FAR_ROWS: (f32, f32) = (0.30, 0.95);

/// See [`FAR_ROWS`]: the rows below the lowest fin, with four hundredths of the
/// frame between the two — the nearest strands of the hillside, seen from well
/// above.
const NEAR_ROWS: (f32, f32) = (0.95, 1.0);

/// The extent the fin and strand claims are drawn at.
///
/// **Larger than the goldens', because a strand at 256 pixels is not one a
/// fin could fill a gap beside**: `crcbl_shaders::grass::STRAND_MIN_PIXELS`
/// widens every far strand to a pixel, which at that size closes the far mat
/// by itself and leaves the fins a few hundred pixels to prove anything with.
const CLAIM_EXTENT: (u32, u32) = (1024, 768);

/// How much of the ground the bare stack shows on the far hillside the fins
/// must cover, as a fraction of it.
///
/// Measured on 2026-09-17 at [`CLAIM_EXTENT`]: the fins covered **0.1303** of
/// what the bare stack showed on lavapipe and 0.1304 on radv; this is half of
/// that, and the run prints the figure.
const FINS_COVER: f32 = 0.065;

/// **Fins fill the silhouette the bare stack leaves open, and stand nowhere
/// else.**
///
/// Three frames of one hillside: the shell meadow, the same field with its fins
/// off, and no field at all. A pixel equal to the bare frame's is ground seen
/// through the grass. In the far band — the hillside at a grazing angle, where
/// a ray passes between the layers — the frame with fins shows the ground at
/// [`FINS_COVER`] fewer of the pixels the frame without them does. In the near
/// band, where the ground is seen from above and every fin is folded away in
/// the vertex stage, the two are the same pixels.
///
/// **Shown red by sabotage twice** (2026-09-17, lavapipe), each edit to
/// `finVertexMain`'s fade and the artifacts recompiled. Held at no less than
/// one half, so no fin folds away: 3219 pixels of the near band differed.
/// Multiplied by zero, so every fin folds away: the far band covered nothing.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn fins_fill_the_far_hillside_and_stand_nowhere_else() {
    let with_fins = |fins| {
        crcbl::screenshot::meadow_shells_field()
            .with_shells(Shells {
                count: DEFAULT_SHELLS,
                fins,
            })
            .expect("the default stack is a stack")
    };
    let finned = frame_with(CLAIM_EXTENT, Some(with_fins(true)), MeadowWind::Windy);
    let bare_stack = frame_with(CLAIM_EXTENT, Some(with_fins(false)), MeadowWind::Windy);
    let ground = frame_with(CLAIM_EXTENT, None, MeadowWind::Windy);

    let rows = |(top, bottom): (f32, f32)| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fractions of the frame's height, in 0..=1"
        )]
        let at = |fraction: f32| (fraction * CLAIM_EXTENT.1 as f32) as u32;
        at(top)..at(bottom)
    };
    let showing = |image: &Image, band: core::ops::Range<u32>| {
        band.flat_map(|y| (0..CLAIM_EXTENT.0).map(move |x| (x, y)))
            .filter(|&(x, y)| image.pixel(x, y) == ground.pixel(x, y))
            .count()
    };
    let far_finned = showing(&finned, rows(FAR_ROWS));
    let far_bare = showing(&bare_stack, rows(FAR_ROWS));
    let near = differing_in(&finned, &bare_stack, 0..CLAIM_EXTENT.0, rows(NEAR_ROWS));
    let far_differing = differing_in(&finned, &bare_stack, 0..CLAIM_EXTENT.0, rows(FAR_ROWS));
    let covered = 1.0 - far_finned as f32 / far_bare.max(1) as f32;
    // The lowest row a fin reached, which is where the two bands were drawn
    // from — printed so a move of it is a number rather than a failure.
    let lowest_fin = (0..CLAIM_EXTENT.1)
        .rev()
        .find(|&y| differing_in(&finned, &bare_stack, 0..CLAIM_EXTENT.0, y..y + 1) > 0);
    eprintln!(
        "crcbl render e2e: meadow shells — the lowest row a fin reached is {lowest_fin:?} of {}",
        CLAIM_EXTENT.1
    );
    eprintln!(
        "crcbl render e2e: meadow shells — far band shows the ground at {far_finned} pixel(s) \
         with fins and {far_bare} without, {covered:.4} of it covered ({far_differing} differ); \
         near band {near} pixel(s) differ"
    );
    assert!(
        covered >= FINS_COVER,
        "the far hillside shows the ground at {far_finned} pixels with fins and {far_bare} \
         without, {covered} of it covered against {FINS_COVER}: the fins filled nothing"
    );
    assert_eq!(
        near, 0,
        "{near} pixel(s) of the near band differ with the fins on: a fin stood where the ground is \
         seen from above"
    );
}

/// How far up a placed blade the strand claim reads, as a fraction of the
/// blade's height.
const STRAND_SHARE: f32 = 0.25;

/// The share of the near tile's roots the shells must cover.
///
/// Measured on 2026-09-17 at [`CLAIM_EXTENT`]: **174 of 174** on lavapipe and
/// on radv. Not all of them, because a root is read at one pixel and a nearer
/// strand's edge may cross it in either frame.
const SHELLS_COVER_ROOTS: f32 = 0.95;

/// The share of the same roots a card field must leave open.
///
/// Measured on 2026-09-17: the cards covered **140** of 174 on lavapipe and 139
/// on radv, so left a fifth open; this is half of that.
const CARDS_LEAVE_ROOTS: f32 = 0.10;

/// **Shells draw strands where the placement put blades, which a card field of
/// the same placement does not.**
///
/// A card is a tuft of strands spread across its width with a gap at its
/// centre, and a shell strand is a cone standing on the root itself. So over
/// every blade the CPU places in the near, sparse, calm quadrant — where a root
/// is several pixels from the next and nothing leans — the point a quarter of
/// the way up the blade is read in three frames: the shell meadow, the card
/// meadow and the bare hillside. The shells cover it at far more roots than
/// the cards do.
///
/// **Shown red by sabotage** (2026-09-17, lavapipe): `shellFragmentMain`
/// measured a strand's disc from half a cell beside its root, and the artifacts
/// recompiled. The shells then covered 150 of the 174 roots.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn shells_draw_strands_at_the_roots_a_card_field_leaves_open() {
    let shells = frame_with(
        CLAIM_EXTENT,
        Some(crcbl::screenshot::meadow_shells_field()),
        MeadowWind::Windy,
    );
    let cards = frame_with(
        CLAIM_EXTENT,
        Some(crcbl::screenshot::meadow_field()),
        MeadowWind::Windy,
    );
    let ground = frame_with(CLAIM_EXTENT, None, MeadowWind::Windy);

    let field = crcbl::screenshot::meadow_shells_field();
    let aspect = CLAIM_EXTENT.0 as f32 / CLAIM_EXTENT.1 as f32;
    let view = crcbl::screenshot::meadow_camera().view_projection(aspect);
    let (mut roots, mut by_shells, mut by_cards) = (0usize, 0usize, 0usize);
    // Tile 3 is the near, sparse, calm one — see the meadow's layout.
    for blade in placement::blades_of_tile(&field, 3) {
        assert_eq!(field.blades()[blade.row as usize].look, BladeLook::Shells);
        let point = glam::Vec3::new(
            blade.root[0],
            blade.root[1] + STRAND_SHARE * blade.height,
            blade.root[2],
        );
        let clip = view * point.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        let column = (ndc.x * 0.5 + 0.5) * CLAIM_EXTENT.0 as f32;
        let row = (0.5 - ndc.y * 0.5) * CLAIM_EXTENT.1 as f32;
        if !(0.0..CLAIM_EXTENT.0 as f32).contains(&column)
            || !(0.0..CLAIM_EXTENT.1 as f32).contains(&row)
        {
            continue;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "inside the frame, which the test above says"
        )]
        let (x, y) = (column as u32, row as u32);
        roots += 1;
        by_shells += usize::from(shells.pixel(x, y) != ground.pixel(x, y));
        by_cards += usize::from(cards.pixel(x, y) != ground.pixel(x, y));
    }
    eprintln!(
        "crcbl render e2e: meadow shells — of {roots} root(s) in frame, the shells cover \
         {by_shells} and the cards {by_cards}"
    );
    assert!(
        roots > 100,
        "only {roots} roots of the near tile are in frame"
    );
    let share = |count: usize| count as f32 / roots as f32;
    assert!(
        share(by_shells) >= SHELLS_COVER_ROOTS,
        "the shells cover {by_shells} of {roots} roots against {SHELLS_COVER_ROOTS} of them: a \
         strand does not stand where the placement put its blade"
    );
    assert!(
        1.0 - share(by_cards) >= CARDS_LEAVE_ROOTS,
        "the cards cover {by_cards} of {roots} roots, leaving under {CARDS_LEAVE_ROOTS} of them \
         open — so the shells' coverage says nothing a card field would not"
    );
}

/// How many pixels a lever must move, in either look, before the claim below
/// counts it as pulled.
///
/// Measured on 2026-09-17 at [`EXTENT`]: the fewest any lever set moved was
/// **12217** pixels — the normal, on the card meadow, on radv; lavapipe's
/// fewest was 12219 of the same pair, and the shell meadow's pairs moved over
/// 15500 on both. This is well under all of them, and the run prints every
/// pair's figure.
const LEVERS_MOVE: usize = 5000;

/// Which of decision 4's levers a restyled frame pulls.
#[derive(Clone, Copy, Debug)]
enum Levers {
    /// The shell meadow's colour levers — occlusion, glow and patches — over
    /// the ground's normal.
    Colour,
    /// No colour lever, and the normal straight up.
    Normal,
}

/// **The levers restyle every look, each kind of them on its own.**
///
/// Decision 4: "each is a field of the blade type, so any of the three looks can
/// be stylised". So the shell meadow's styles are put on the card rows as well,
/// split into the colour levers and the normal, and each look is drawn with
/// each and with neither. Every pair differs, which is the claim that no lever
/// is a shell-only branch — the colour levers are applied in the fragment stage
/// and the normal in the vertex stage, so a look that dropped either would draw
/// one of these pairs the same. That a plain row is untouched by them is
/// `Scene::Meadow`'s golden, which predates them.
///
/// **Shown red by sabotage twice** (2026-09-17, lavapipe), the artifacts
/// recompiled each time. `fragmentMain` returning the card's colour without
/// `grass_style`: the colour levers moved 0 pixels of the card meadow.
/// `shellFragmentMain` shading every strand by the ground's normal: the normal
/// lever moved 0 pixels of the shell meadow.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_levers_restyle_cards_and_shells_alike() {
    use crcbl::render::grass::{BladeNormal, BladeStyle, BladeType, GrassField};

    let restyled = |look: BladeLook, levers: Option<Levers>| {
        let styles = crcbl::screenshot::meadow_shell_blades();
        let rows = crcbl::screenshot::meadow_blades()
            .into_iter()
            .zip(styles)
            .map(|(row, shell)| BladeType {
                look,
                style: match levers {
                    None => BladeStyle::PLAIN,
                    Some(Levers::Colour) => BladeStyle {
                        normal: BladeNormal::Ground,
                        ..shell.style
                    },
                    Some(Levers::Normal) => BladeStyle {
                        normal: BladeNormal::Up,
                        ..BladeStyle::PLAIN
                    },
                },
                ..row
            })
            .collect();
        let card = crcbl::screenshot::meadow_field();
        GrassField::new(
            card.tiles(),
            card.tile_size(),
            card.origin(),
            card.reach(),
            card.ground().clone(),
            card.cover().clone(),
            rows,
        )
        .expect("the meadow's field with other rows is a field")
    };
    let mut moved = Vec::new();
    for look in [BladeLook::Cards, BladeLook::Shells] {
        let plain = frame_with(EXTENT, Some(restyled(look, None)), MeadowWind::Windy);
        for levers in [Levers::Colour, Levers::Normal] {
            let styled = frame_with(
                EXTENT,
                Some(restyled(look, Some(levers))),
                MeadowWind::Windy,
            );
            moved.push((
                look,
                levers,
                differing_in(&styled, &plain, 0..EXTENT.0, 0..EXTENT.1),
            ));
        }
    }
    eprintln!("crcbl render e2e: meadow shells — pixels each lever set moved, by look: {moved:?}");
    for (look, levers, pixels) in moved {
        assert!(
            pixels >= LEVERS_MOVE,
            "the {levers:?} levers moved {pixels} pixel(s) of the {look:?} meadow against \
             {LEVERS_MOVE}: that look does not draw them"
        );
    }
}
