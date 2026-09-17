//! [`Scene::MeadowBlades`]: `docs/plan/57-grass.md` rung G2's look, held to its
//! golden and to five relations with the frames around it — the same field
//! drawn as cards and as shells, the same field with its level switch moved,
//! the same hillside with no grass, the same field unclumped, and the same
//! blades in no wind.
//!
//! A file of its own beside `grass.rs` and `grass_shells.rs`, whose readback and
//! frame helpers it reads.
//!
//! # Where a blade is on screen
//!
//! Several claims read the pixel a blade covers, and a blade is a curve, so
//! [`blade_point`] is the CPU's copy of `grass.slang`'s `grass_blade_curve` —
//! without the wind, which every one of those claims avoids by reading only the
//! calm half, where the lean is exactly zero.

use crcbl::render::grass::placement::{self, Blade};
use crcbl::render::grass::{BladeLod, BladeLook, BladeStyle, BladeType, Clumping, GrassField};
use crcbl::screenshot::{MEADOW_BLADE_LOD, MEADOW_PATH_HALF_WIDTH, MeadowWind, Scene};
use crcbl::shaders::grass::{DEFAULT_SHELLS, GrassInstance, INSTANCE_STRIDE};
use crcbl_golden::Image;

use super::EXTENT;
use super::grass::{Slot, cells_of, frame_of, generated};
use super::grass_shells::{CLAIM_EXTENT, differing_in, frame_with, meadow_with};

/// The anti-vacuity colour count for [`Scene::MeadowBlades`]: a lit hillside of
/// shaded, clump-coloured blades over an earth plate under a sky.
///
/// **Measured on 2026-09-17 at the 8192 ceiling
/// [`a_look_switch_leaves_the_blade_placement_bit_identical`] counts to, on
/// lavapipe and on radv**, against the 96 of the bare hillside
/// `Scene::Meadow`'s removal test prints — so this is well clear of both.
const MIN_COLORS_MEADOW_BLADES: usize = 3000;

/// [`Scene::MeadowBlades`] drawn, against the reference in `tests/golden/`.
///
/// The band claim is the card meadow's own: the cover map paints grass on one
/// side of the path band and none on it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_blades_scene_draws_its_blades_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden_measuring(
        Scene::MeadowBlades,
        "meadow_blades",
        EXTENT,
        MIN_COLORS_MEADOW_BLADES,
        super::grass::the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// [`Scene::MeadowBlades`] on both geometry paths this machine can reach — the
/// blades read no geometry path, and the ground they stand on does.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_meadow_blades_scene_draws_the_same_frame_on_every_geometry_path() {
    super::draw_scene_on_every_geometry_path_measuring(
        Scene::MeadowBlades,
        "meadow_blades",
        MIN_COLORS_MEADOW_BLADES,
        super::grass::the_meadow_is_green_where_its_cover_map_says,
        super::ClaimFrame::TheOneTheGoldenIs,
    );
}

/// `field` with every row's look replaced by `look`, and nothing else.
fn with_look(field: &GrassField, look: BladeLook) -> GrassField {
    rebuilt(
        field,
        field
            .blades()
            .iter()
            .map(|row| BladeType { look, ..*row })
            .collect(),
        field.blade_lod(),
    )
}

/// `field` with `rows` for its blade table and `lod` for its level switch.
fn rebuilt(field: &GrassField, rows: Vec<BladeType>, lod: BladeLod) -> GrassField {
    GrassField::new(
        field.tiles(),
        field.tile_size(),
        field.origin(),
        field.reach(),
        field.ground().clone(),
        field.cover().clone(),
        rows,
    )
    .and_then(|rebuilt| rebuilt.with_shells(field.shells()))
    .and_then(|rebuilt| rebuilt.with_blade_lod(lod))
    .expect("the blade meadow with other rows is a field")
}

/// One frame of `field`'s generation, read back: every slot's runs, the cells
/// buffer and how many colours the frame after it holds.
fn read_back(field: GrassField) -> (Vec<Slot>, Vec<u8>, usize) {
    let mut setup = meadow_with(EXTENT, Some(field), MeadowWind::Windy);
    let slots = generated(&mut setup);
    let cells = cells_of(&mut setup);
    let colours = frame_of(&mut setup).distinct_colors(8192);
    setup.finish();
    (slots, cells, colours)
}

/// How near the level switch, in square metres of squared distance, a blade's
/// root may be before the CPU's answer about its side is not compared: the
/// GPU squares an `f32` offset whose last place a compiler may round another
/// way.
const SWITCH_SLACK: f32 = 1e-3;

/// **A look switch leaves the placement bit-identical, across all three
/// looks** — rung G3's claim, extended to the third.
///
/// The blade meadow, and the same field with every row's look switched to
/// cards and to shells and nothing else changed:
///
/// * the `cells` buffer the three generation passes wrote is **the same bytes**;
/// * every appended card of the card twin, and every near and far blade of the
///   blade field, is the row of its own cell, bit for bit;
/// * the card twin appended every cell that grew; the blade field appended every
///   cell nearer the camera than the switch to the near run, every cell past it
///   the far level keeps to the far run, and nothing else; the shell twin gave
///   every slot its stack and its fins and appended nothing;
/// * every grown cell's clump, clump offset and far-level decision is the CPU
///   placement's, bit for bit — the integer searches the shader and
///   `crcbl_shaders::grass` share.
///
/// **Shown red by sabotage three times** (2026-09-17, radv), each an edit to
/// `grass_gen.slang` with the artifacts recompiled. `grass_kept_far` hashing
/// with `GRASS_LOD_SALT ^ 1u`: the first grown cell's far-level decision was
/// not the CPU's. A mesh blade row's lean zeroed before its cell is written:
/// 220847 cell bytes differed from both twins'. Every `clump.id` in
/// `generateMain` replaced by the cell: the first grown cell's clump was not
/// the CPU's.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_look_switch_leaves_the_blade_placement_bit_identical() {
    let field = crcbl::screenshot::meadow_blades_field();
    let (blade_slots, blade_cells, colours) = read_back(field.clone());
    let (card_slots, card_cells, _) = read_back(with_look(&field, BladeLook::Cards));
    let (shell_slots, shell_cells, _) = read_back(with_look(&field, BladeLook::Shells));
    eprintln!("crcbl render e2e: meadow blades — {colours} colour(s) in the blade frame");
    assert!(colours >= MIN_COLORS_MEADOW_BLADES);

    let differing = |left: &[u8], right: &[u8]| {
        left.iter()
            .zip(right)
            .filter(|(left, right)| left != right)
            .count()
    };
    let cells: Vec<GrassInstance> = blade_cells
        .chunks_exact(INSTANCE_STRIDE)
        .map(|row| GrassInstance::from_bytes(row.try_into().expect("one row")))
        .collect();
    let grown = cells.iter().filter(|cell| cell.root[3] > 0.0).count();
    let (to_cards, to_shells) = (
        differing(&blade_cells, &card_cells),
        differing(&blade_cells, &shell_cells),
    );
    eprintln!(
        "crcbl render e2e: meadow blades — {} cell byte(s) compared, {to_cards} differ from the \
         card twin's and {to_shells} from the shell twin's; {grown} cell(s) carry a blade",
        blade_cells.len()
    );
    assert!(
        grown > 20_000,
        "only {grown} cells carry a blade, so an equality of the buffers says little"
    );
    assert_eq!(
        (to_cards, to_shells),
        (0, 0),
        "a look switch moved the placement: the cells differ by {to_cards} byte(s) from the card \
         twin's and {to_shells} from the shell twin's"
    );

    let eye = crcbl::screenshot::meadow_camera().eye;
    let switch = field.blade_lod().distance;
    let (mut cards, mut near, mut far, mut dropped, mut slack) = (0, 0, 0, 0, 0);
    for (slot, ((blade, card), shell)) in blade_slots
        .iter()
        .zip(&card_slots)
        .zip(&shell_slots)
        .enumerate()
    {
        for (cell, instance) in &card.blades {
            assert_eq!(
                *instance, cells[*cell as usize],
                "the card twin's card for cell {cell} is not the cell's row"
            );
        }
        cards += card.blades.len();
        assert!(
            card.near.is_empty() && card.far.is_empty() && (card.shells, card.fins) == (0, 0),
            "the card twin's slot {slot} drew another look"
        );
        assert!(
            shell.count == 0 && shell.near.is_empty() && shell.far.is_empty(),
            "the shell twin appended to slot {slot}"
        );
        assert_eq!((shell.shells, shell.fins), (DEFAULT_SHELLS, 1));
        assert!(
            blade.count == 0 && (blade.shells, blade.fins) == (0, 0),
            "the blade field's slot {slot} drew another look"
        );
        for (cell, instance) in blade.near.iter().chain(&blade.far) {
            assert_eq!(
                *instance, cells[*cell as usize],
                "the blade appended for cell {cell} is not the cell's row"
            );
            assert!(
                !(blade.near.contains_key(cell) && blade.far.contains_key(cell)),
                "cell {cell} is in both runs"
            );
        }
        near += blade.near.len();
        far += blade.far.len();

        // Every cell of the slot that grew, against which run it landed in.
        #[expect(clippy::cast_possible_truncation, reason = "sixteen slots")]
        let placed = placement::blades_of_tile(&field, slot as u32);
        for cpu in &placed {
            let cell = cells[cpu.cell as usize];
            assert_eq!(
                (
                    cell.lanes[2],
                    cell.lanes[3] != 0,
                    [cell.clump[0], cell.clump[1]]
                ),
                (cpu.clump, cpu.kept_far, cpu.clump_offset),
                "cell {}'s clump, far-level decision or clump offset is not the CPU's",
                cpu.cell
            );
            let squared =
                (glam::Vec3::new(cell.root[0], cell.root[1], cell.root[2]) - eye).length_squared();
            if (squared - switch * switch).abs() < SWITCH_SLACK {
                slack += 1;
                continue;
            }
            let (in_near, in_far) = (
                blade.near.contains_key(&cpu.cell),
                blade.far.contains_key(&cpu.cell),
            );
            let wanted = if squared < switch * switch {
                (true, false)
            } else {
                (false, cpu.kept_far)
            };
            assert_eq!(
                (in_near, in_far),
                wanted,
                "cell {} at {:.3} m from the eye is in the wrong run (switch {switch} m, kept {})",
                cpu.cell,
                squared.sqrt(),
                cpu.kept_far
            );
            dropped += usize::from(!in_near && !in_far);
        }
    }
    eprintln!(
        "crcbl render e2e: meadow blades — the card twin appended {cards}; the blade field {near} \
         near and {far} far, dropping {dropped}; {slack} cell(s) too near the switch to compare"
    );
    assert_eq!(cards, grown, "the card twin did not append every cell");
    assert_eq!(
        near + far + dropped,
        grown,
        "the blade runs and the drops do not account for every cell"
    );
    assert!(
        near > 1000 && far > 1000 && dropped > 1000,
        "a run is nearly empty: {near} near, {far} far, {dropped} dropped"
    );
}

/// The eye, and the frame's projection at `extent`.
fn view(extent: (u32, u32)) -> (glam::Vec3, glam::Mat4) {
    let camera = crcbl::screenshot::meadow_camera();
    (
        camera.eye,
        camera.view_projection(extent.0 as f32 / extent.1 as f32),
    )
}

/// Where `point` lands in a frame of `extent` seen through `projection`, in
/// pixels, or [`None`] off the frame.
fn project(projection: glam::Mat4, extent: (u32, u32), point: glam::Vec3) -> Option<(f32, f32)> {
    let clip = projection * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let at = (
        (ndc.x * 0.5 + 0.5) * extent.0 as f32,
        (0.5 - ndc.y * 0.5) * extent.1 as f32,
    );
    ((0.0..extent.0 as f32).contains(&at.0) && (0.0..extent.1 as f32).contains(&at.1)).then_some(at)
}

/// The point `t` of the way up `blade` of row `row`, in still air — the CPU's
/// copy of `grass.slang`'s `grass_blade_curve` and its Bézier.
fn blade_point(blade: &Blade, row: &BladeType, t: f32) -> glam::Vec3 {
    let height = blade.height;
    let face = glam::Vec3::new(blade.facing[0], 0.0, blade.facing[1]);
    let tilt = row.shape.tilt;
    let upright = (1.0 - tilt * tilt).max(0.0).sqrt();
    let chord = glam::Vec3::new(0.0, height * upright, 0.0) + face * (height * tilt);
    let across = glam::Vec3::new(face.x * upright, -tilt, face.z * upright);
    let bow = across * (row.shape.bow * height);
    let p0 = glam::Vec3::from(blade.root);
    let p1 = p0 + chord / 3.0 - bow;
    let p2 = p0 + chord * (2.0 / 3.0) - bow;
    let p3 = p0 + chord;
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Every blade `field` places on the calm side of the path, with its row.
fn calm_blades(field: &GrassField) -> Vec<(Blade, BladeType)> {
    (0..field.slots())
        .flat_map(|slot| placement::blades_of_tile(field, slot))
        .filter(|blade| blade.root[0] > MEADOW_PATH_HALF_WIDTH)
        .map(|blade| (blade, field.blades()[blade.row as usize]))
        .collect()
}

/// How far the level switch moves between the two frames of each pair the pop
/// claim compares, in metres.
const SWITCH_STEP: f32 = 0.2;

/// The switch distances the pairs start at, in metres: the meadow's own and
/// three beyond it, so enough blades cross a step to count.
const SWITCHES: [f32; 4] = [6.0, 7.0, 8.0, 9.0];

/// The cover density of the thinned meadow the pop and edge-on claims read:
/// thin enough that a blade's on-screen box holds little but that blade.
const THIN_DENSITY: u8 = 12;

/// The blade meadow at [`THIN_DENSITY`] everywhere off the path, its rows
/// passed through `row`.
fn thinned_meadow(row: impl Fn(BladeType) -> BladeType) -> GrassField {
    let meadow = crcbl::screenshot::meadow_blades_field();
    let mut cover = meadow.cover().clone();
    for texel in &mut cover.cover {
        if texel[0] > 0 {
            texel[0] = THIN_DENSITY;
        }
    }
    GrassField::new(
        meadow.tiles(),
        meadow.tile_size(),
        meadow.origin(),
        meadow.reach(),
        meadow.ground().clone(),
        cover,
        meadow.blades().iter().map(|each| row(*each)).collect(),
    )
    .and_then(|thinned| thinned.with_blade_lod(meadow.blade_lod()))
    .expect("the thinned meadow is a field")
}

/// The half-width of the pop and width claims' blades, in metres: several
/// pixels at every distance they read, so a sub-pixel move of a blade's edges
/// is a small share of it.
const WIDE_HALF_WIDTH: f32 = 0.04;

/// Which blades a frame pair across a switch compares, by where the root
/// stands from the eye against the pair's first switch `distance`, its band
/// `band` and [`SWITCH_STEP`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Crossing {
    /// Between the two switches: in the far draw on the first frame and the
    /// near draw on the second.
    Crosses,
    /// Inside the band's second half on both frames: morphing, and moved by
    /// the step's share of it.
    Morphs,
    /// Inside the band's first half on both frames: dropping, likewise.
    Drops,
}

impl Crossing {
    /// The group a blade `from_eye` metres away belongs to, if any. The edges
    /// are a centimetre inside, so no root's side is a question of rounding.
    fn of(from_eye: f32, distance: f32, band: f32) -> Option<Self> {
        let inside = |low: f32, high: f32| (low + 0.01..high - 0.01).contains(&from_eye);
        if inside(distance, distance + SWITCH_STEP) {
            Some(Self::Crosses)
        } else if inside(distance - 0.5 * band + SWITCH_STEP, distance) {
            Some(Self::Morphs)
        } else if inside(distance - band + SWITCH_STEP, distance - 0.5 * band) {
            Some(Self::Drops)
        } else {
            None
        }
    }
}

/// **The level switch does not pop**: a blade crossing it between two frames
/// changes shape no more than a blade dropping inside the band does, and
/// changes shade no more than a blade morphing inside it does — and, with the
/// band taken away, it changes shape several times as much.
///
/// **The field is the meadow thinned and widened** — see [`thinned_meadow`]
/// and [`WIDE_HALF_WIDTH`] — so a blade's on-screen box holds little but that
/// blade and a sub-pixel move of its edges is a small share of it. It stands
/// in still air, so every blade is where the CPU says.
///
/// For each of [`SWITCHES`], four frames: the switch there and one
/// [`SWITCH_STEP`] further, each with the meadow's band and with a band of
/// zero. Inside every blade's box, between the pair, two things are summed
/// against what the blade covers of the bare hillside on the first frame:
///
/// * its **shape** change — pixels grass covers on one frame and not the
///   other, which is what a blade dropped or widened moves;
/// * its **shade** change — the colour difference, in code values over three
///   channels, of pixels grass covers on both, which is what a normal
///   blending toward the clump's moves. The morph's own change of *shape* is
///   under a pixel at these distances for the meadow's blades, so the morph is
///   read here, where it shows.
///
/// A step moves every blade inside the band by the step's share of its drop or
/// morph, and a crossing blade by the same share of its morph. One that popped
/// would change by the whole of it.
///
/// Measured on 2026-09-17: a crossing blade reshaped **0.0159** of what it
/// covers and reshaded it by **4.8**, against a dropping blade's 0.1302 and a
/// morphing blade's 20.1, and **0.3320** with no band, on radv; lavapipe's
/// run prints its own beside them.
///
/// **Shown red by sabotage twice** (2026-09-17, radv), each an edit to
/// `grass.slang`'s `grass_blade_vertex` with the artifacts recompiled. The
/// near level never morphing: a crossing blade reshaded by 37.8 against a
/// morphing one's 12.4. The near level never dropping: a crossing blade
/// reshaped 0.3396 against a dropping one's 0.0029.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_blade_level_switch_does_not_pop() {
    let thinned = thinned_meadow(|row| BladeType {
        half_width: WIDE_HALF_WIDTH,
        ..row
    });
    let band = MEADOW_BLADE_LOD.band;
    let at = |distance: f32, band: f32| {
        frame_with(
            CLAIM_EXTENT,
            Some(rebuilt(
                &thinned,
                thinned.blades().to_vec(),
                BladeLod { distance, band },
            )),
            MeadowWind::Calm,
        )
    };
    let ground = frame_with(CLAIM_EXTENT, None, MeadowWind::Calm);
    let (eye, projection) = view(CLAIM_EXTENT);
    let placed: Vec<(Blade, BladeType)> = (0..thinned.slots())
        .flat_map(|slot| placement::blades_of_tile(&thinned, slot))
        .map(|blade| (blade, thinned.blades()[blade.row as usize]))
        .collect();

    // `[reshaped, reshaded, covered]` inside the blade's box, between `before`
    // and `after`.
    let changed = |before: &Image, after: &Image, blade: &Blade, row: &BladeType| {
        let ends =
            [0.0, 1.0].map(|t| project(projection, CLAIM_EXTENT, blade_point(blade, row, t)));
        let [Some(root), Some(tip)] = ends else {
            return None;
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "inside the frame, which `project` says"
        )]
        let span = |low: f32, high: f32, pad: f32, limit: u32| {
            (low.min(high) - pad).max(0.0) as u32..((low.max(high) + pad + 1.0) as u32).min(limit)
        };
        // The blade's width either side of its axis, in pixels, as far as the
        // far level widens it.
        let clip = projection * glam::Vec3::from(blade.root).extend(1.0);
        let pad = crcbl::shaders::grass::BLADE_FAR_WIDEN
            * WIDE_HALF_WIDTH
            * 0.5
            * CLAIM_EXTENT.0 as f32
            * projection.x_axis.x
            / clip.w.max(1e-3)
            + 1.0;
        let (columns, rows) = (
            span(root.0, tip.0, pad, CLAIM_EXTENT.0),
            span(root.1, tip.1, 1.0, CLAIM_EXTENT.1),
        );
        let mut counts = [0usize; 3];
        for y in rows {
            for x in columns.clone() {
                let bare = ground.pixel(x, y);
                let (was, is) = (before.pixel(x, y), after.pixel(x, y));
                let (was_covered, is_covered) = (was != bare, is != bare);
                counts[0] += usize::from(was_covered != is_covered);
                if let (true, true, Some(was), Some(is)) = (was_covered, is_covered, was, is) {
                    counts[1] += (0..3)
                        .map(|at| usize::from(was[at].abs_diff(is[at])))
                        .sum::<usize>();
                }
                counts[2] += usize::from(was_covered);
            }
        }
        Some(counts)
    };
    // `[soft reshaped, soft reshaded, soft covered, hard reshaped, hard
    // reshaded, hard covered]` and how many blades, per group.
    let mut sums: [([usize; 6], usize); 3] = [([0; 6], 0); 3];
    let slot_of = |group| match group {
        Crossing::Crosses => 0,
        Crossing::Morphs => 1,
        Crossing::Drops => 2,
    };
    for distance in SWITCHES {
        let (smooth, smooth_moved) = (at(distance, band), at(distance + SWITCH_STEP, band));
        let (hard, hard_moved) = (at(distance, 0.0), at(distance + SWITCH_STEP, 0.0));
        for (blade, row) in &placed {
            let from_eye = glam::Vec3::from(blade.root).distance(eye);
            let Some(group) = Crossing::of(from_eye, distance, band) else {
                continue;
            };
            if let (Some(soft), Some(popped)) = (
                changed(&smooth, &smooth_moved, blade, row),
                changed(&hard, &hard_moved, blade, row),
            ) {
                let (group_sums, count) = &mut sums[slot_of(group)];
                for (sum, value) in group_sums.iter_mut().zip(soft.into_iter().chain(popped)) {
                    *sum += value;
                }
                *count += 1;
            }
        }
    }
    let share = |changed: usize, covered: usize| changed as f32 / covered.max(1) as f32;
    let [
        (crossing, crossed),
        (morphing, morphed),
        (dropping, dropped),
    ] = sums;
    let (crossing_shape, crossing_shade) = (
        share(crossing[0], crossing[2]),
        share(crossing[1], crossing[2]),
    );
    let popped_shape = share(crossing[3], crossing[5]);
    let morphing_shade = share(morphing[1], morphing[2]);
    let dropping_shape = share(dropping[0], dropping[2]);
    eprintln!(
        "crcbl render e2e: meadow blades — {crossed} blade(s) crossed a switch, {morphed} morphed \
         and {dropped} dropped inside a band; with the band a crossing blade reshaped \
         {crossing_shape:.4} of what it covers and reshaded it by {crossing_shade:.3}, a dropping \
         blade reshaped {dropping_shape:.4} and a morphing one reshaded {morphing_shade:.3}; with \
         no band a crossing blade reshaped {popped_shape:.4}"
    );
    assert!(
        crossed > 40 && morphed > 40 && dropped > 40,
        "too few blades to compare: {crossed} crossing, {morphed} morphing, {dropped} dropping"
    );
    assert!(
        popped_shape >= POP_OVER_BAND * crossing_shape,
        "without the band a crossing blade reshaped {popped_shape:.4} of what it covers and with \
         it {crossing_shape:.4}: the configuration does not pop, so the claims below say nothing"
    );
    assert!(
        crossing_shape <= CROSSING_OVER_BAND * dropping_shape,
        "a blade crossing the switch reshaped {crossing_shape:.4} of what it covers and one \
         dropping inside the band {dropping_shape:.4}: the switch pops in shape"
    );
    assert!(
        crossing_shade <= CROSSING_OVER_BAND * morphing_shade,
        "a blade crossing the switch reshaded by {crossing_shade:.3} and one morphing inside the \
         band by {morphing_shade:.3}: the switch pops in shade"
    );
}

/// How many times more a crossing blade must reshape with no band than with
/// one: a seventh of the twenty-one times measured.
const POP_OVER_BAND: f32 = 3.0;

/// How many times more a crossing blade may change than a blade that stayed in
/// the band's matching half. Both measured relations are under a quarter of
/// this, and both sabotages put the crossing blade past twice it.
const CROSSING_OVER_BAND: f32 = 1.5;

/// How far a blade is from edge-on, as `|side · view|`, before the claims
/// below count it as seen along its edge.
const EDGE_ON: f32 = 0.9;

/// How far from edge-on a blade is, as the same number, before they count it
/// as seen face-on.
const FACE_ON: f32 = 0.3;

/// The share of edge-on blades whose row must show them.
const EDGE_ON_SHOWN: f32 = 0.97;

/// How much of a face-on blade's width, on its row, an edge-on one must keep.
///
/// Measured on 2026-09-17 at **0.936** on radv, and at 0.687 with the width's
/// turn toward the view taken out — so this sits between the two.
const EDGE_ON_KEEPS: f32 = 0.8;

/// How many pixels either side of a blade's point the width reading counts on
/// its row: a wide blade's face-on width at the nearest distance it is read
/// at, and not so many that the neighbours beside it swamp the blade.
const ROW_REACH: u32 = 12;

/// How the edge-on and face-on blades of `placed` — `field`'s — show in
/// `blades` against the bare hillside `ground`, both drawn at `extent`:
/// `[count, shown on the row,
/// covered pixels on the row]` for each of the two, over every blade drawn at
/// full width — before the band, or kept by the far level — whose root is
/// nearer the eye than `within`.
fn edge_and_face(
    field: &GrassField,
    placed: &[(Blade, BladeType)],
    [blades, ground]: [&Image; 2],
    extent: (u32, u32),
    within: f32,
) -> [[usize; 3]; 2] {
    let (eye, projection) = view(extent);
    let BladeLod { distance, band } = field.blade_lod();
    let mut counts = [[0usize; 3]; 2];
    for (blade, row) in placed {
        let from_eye = glam::Vec3::from(blade.root).distance(eye);
        if from_eye >= within || !(from_eye < distance - band || blade.kept_far) {
            continue;
        }
        let point = blade_point(blade, row, 0.25);
        let Some((column, line)) = project(projection, extent, point) else {
            continue;
        };
        let side = glam::Vec3::new(-blade.facing[1], 0.0, blade.facing[0]);
        let along = side.dot((eye - point).normalize()).abs();
        let group = if along > EDGE_ON {
            0
        } else if along < FACE_ON {
            1
        } else {
            continue;
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "inside the frame, which `project` says"
        )]
        let (x, y) = (column as u32, line as u32);
        let near = |reach: u32| {
            differing_in(
                blades,
                ground,
                x.saturating_sub(reach)..(x + reach + 1).min(extent.0),
                y..y + 1,
            )
        };
        counts[group][0] += 1;
        counts[group][1] += usize::from(near(1) > 0);
        counts[group][2] += near(ROW_REACH);
    }
    counts
}

/// **A blade seen edge-on is still at least a pixel wide, and keeps most of its
/// width where it has width to keep.**
///
/// Two readings over blades whose side is within [`EDGE_ON`] of pointing at the
/// camera, against blades within [`FACE_ON`] of facing it, each at the row of
/// the frame a quarter of the way up the blade, around the point the curve
/// passes through it:
///
/// * **The thinned meadow's calm half, everywhere, at the goldens' extent** —
///   see [`thinned_meadow`] — so neighbours rarely stand in front of one
///   another and most blades are under a pixel wide by their own width. A strip
///   at least a pixel wide covers a pixel centre on every row it crosses, so
///   that row differs from the bare hillside's within a pixel either side, and
///   at least [`EDGE_ON_SHOWN`] of the edge-on blades must. A flat blade
///   narrower than a pixel misses it; the pixel floor is what holds this.
/// * **The meadow's own cover at [`WIDE_HALF_WIDTH`], in still air, nearer
///   than the switch, at the claims' extent**: the pixels covered on the row
///   within [`ROW_REACH`] either side, per edge-on blade, are at least
///   [`EDGE_ON_KEEPS`] of the same per face-on blade. A flat blade seen along
///   its side keeps only the sine of the angle of its width, and the view-space
///   turn is what gives the rest back — decision 3's "so a field stays full".
///
/// Measured on 2026-09-17 on radv: 70 of 70 edge-on blades shown, and 0.936 of
/// the width kept.
///
/// **Shown red by sabotage three times** (2026-09-17, radv), each an edit to
/// `grass.slang`'s `grass_blade_point` with the artifacts recompiled. The width
/// laid along the blade's own side alone: 0.687 of the width kept. The pixel
/// floor taken out: 48 of 70 edge-on blades shown. Both: 25 of 70, and 0.683.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn edge_on_blades_stay_a_pixel_wide() {
    let ground = frame_with(EXTENT, None, MeadowWind::Windy);
    let thin = thinned_meadow(|row| row);
    let thin_frame = frame_with(EXTENT, Some(thin.clone()), MeadowWind::Windy);
    let [[edge_on, edge_shown, _], [face_on, face_shown, _]] = edge_and_face(
        &thin,
        &calm_blades(&thin),
        [&thin_frame, &ground],
        EXTENT,
        f32::INFINITY,
    );

    // Every blade, on both sides of the path, in still air.
    let meadow = crcbl::screenshot::meadow_blades_field();
    let wide = rebuilt(
        &meadow,
        meadow
            .blades()
            .iter()
            .map(|row| BladeType {
                half_width: WIDE_HALF_WIDTH,
                ..*row
            })
            .collect(),
        meadow.blade_lod(),
    );
    let still_ground = frame_with(CLAIM_EXTENT, None, MeadowWind::Calm);
    let wide_frame = frame_with(CLAIM_EXTENT, Some(wide.clone()), MeadowWind::Calm);
    let placed: Vec<(Blade, BladeType)> = (0..wide.slots())
        .flat_map(|slot| placement::blades_of_tile(&wide, slot))
        .map(|blade| (blade, wide.blades()[blade.row as usize]))
        .collect();
    let [[wide_edge, _, edge_covered], [wide_face, _, face_covered]] = edge_and_face(
        &wide,
        &placed,
        [&wide_frame, &still_ground],
        CLAIM_EXTENT,
        wide.blade_lod().distance,
    );
    let kept = (edge_covered as f32 / wide_edge.max(1) as f32)
        / (face_covered as f32 / wide_face.max(1) as f32).max(f32::EPSILON);
    eprintln!(
        "crcbl render e2e: meadow blades — {edge_shown} of {edge_on} edge-on blade(s) shown, \
         {face_shown} of {face_on} face-on; wide and near, {wide_edge} edge-on blade(s) cover \
         {edge_covered} pixel(s) of their rows and {wide_face} face-on {face_covered}, {kept:.3} \
         of the width kept"
    );
    assert!(
        edge_on > 30 && wide_edge > 30 && wide_face > 30,
        "too few blades to read: {edge_on} edge-on, and {wide_edge} edge-on and {wide_face} \
         face-on near"
    );
    assert!(
        edge_shown as f32 >= EDGE_ON_SHOWN * edge_on as f32,
        "only {edge_shown} of {edge_on} edge-on blades show on their own row: an edge-on blade \
         fell under a pixel"
    );
    assert!(
        kept >= EDGE_ON_KEEPS,
        "an edge-on blade covers {kept:.3} of the row a face-on one does: a blade turned edge-on \
         thinned the field"
    );
}

/// How far apart on the ground two blades may stand, in metres, for the clump
/// claim to compare their colours.
const NEIGHBOURS: f32 = 0.35;

/// How many times further apart in colour two neighbouring blades of different
/// clumps must be than two of one clump, over the same ratio unclumped.
///
/// Measured on 2026-09-17: **1.784** against 1.010 unclumped on radv and 1.758
/// against 1.010 on lavapipe, so the relation stood at 1.77 and 1.74 times.
const CLUMPS_OVER_NONE: f32 = 1.3;

/// The mean squared difference of `green` between neighbouring blades of
/// different clumps, over the same between neighbours of one clump.
///
/// **Neighbours only**, so a colour that changes across the frame — the light,
/// the distance, the blade row — is the same on both sides of the ratio and
/// cancels: what is left is whether a clump's edge is an edge in the picture.
fn clump_contrast(read: &[([f32; 2], u32, f32)]) -> f32 {
    let (mut apart, mut apart_pairs, mut together, mut together_pairs) = (0.0f64, 0usize, 0.0, 0);
    for (index, (at, clump, green)) in read.iter().enumerate() {
        for (other_at, other_clump, other_green) in &read[index + 1..] {
            let offset = [at[0] - other_at[0], at[1] - other_at[1]];
            if offset[0] * offset[0] + offset[1] * offset[1] > NEIGHBOURS * NEIGHBOURS {
                continue;
            }
            let squared = f64::from((green - other_green) * (green - other_green));
            if clump == other_clump {
                together += squared;
                together_pairs += 1;
            } else {
                apart += squared;
                apart_pairs += 1;
            }
        }
    }
    let ratio =
        (apart / apart_pairs.max(1) as f64) / (together / together_pairs.max(1) as f64).max(1e-9);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a ratio of two mean squares, printed and compared"
    )]
    let ratio = ratio as f32;
    ratio
}

/// **Clumps are visible structure** — a measured relation, not an eyeball.
///
/// Every calm blade drawn at full width has its pixel two thirds of the way up
/// read, where it shows, beside the clump the CPU placement says it belongs to.
/// Over every pair of those blades standing within [`NEIGHBOURS`] of each
/// other, the green channel's mean squared difference between blades of
/// different clumps is divided by the same between blades of one clump — see
/// [`clump_contrast`]. The same field with its clumping and its patch lever
/// taken away is read the same way over the same clump identities, which it
/// still carries but no longer draws; the clumped contrast has to be
/// [`CLUMPS_OVER_NONE`] times that one.
///
/// **Shown red by sabotage** (2026-09-17, radv): every `clump.id` in
/// `grass_gen.slang`'s `generateMain` replaced by the blade's own cell, and the
/// artifacts recompiled — clumps 1.034 times apart against 1.010 unclumped.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn clumps_are_visible_structure() {
    let field = crcbl::screenshot::meadow_blades_field();
    let unclumped = rebuilt(
        &field,
        field
            .blades()
            .iter()
            .map(|row| BladeType {
                style: BladeStyle {
                    patch_share: 0.0,
                    ..row.style
                },
                clumping: Clumping::NONE,
                ..*row
            })
            .collect(),
        field.blade_lod(),
    );
    let ground = frame_with(CLAIM_EXTENT, None, MeadowWind::Windy);
    let (eye, projection) = view(CLAIM_EXTENT);
    let BladeLod { distance, band } = field.blade_lod();
    let measured = |field: &GrassField| {
        let frame = frame_with(CLAIM_EXTENT, Some(field.clone()), MeadowWind::Windy);
        let mut read = Vec::new();
        for (blade, row) in calm_blades(field) {
            // Every blade drawn at full width: before the band, or kept.
            if glam::Vec3::from(blade.root).distance(eye) >= distance - band && !blade.kept_far {
                continue;
            }
            let Some((column, line)) = project(
                projection,
                CLAIM_EXTENT,
                blade_point(&blade, &row, 2.0 / 3.0),
            ) else {
                continue;
            };
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "inside the frame, which `project` says"
            )]
            let (x, y) = (column as u32, line as u32);
            let (Some(pixel), Some(bare)) = (frame.pixel(x, y), ground.pixel(x, y)) else {
                continue;
            };
            if pixel != bare {
                read.push((
                    [blade.root[0], blade.root[2]],
                    blade.clump,
                    f32::from(pixel[1]),
                ));
            }
        }
        (clump_contrast(&read), read.len())
    };
    let (clumped, read) = measured(&field);
    let (plain, plain_read) = measured(&unclumped);
    eprintln!(
        "crcbl render e2e: meadow blades — neighbouring clumps {clumped:.3} times as far apart in \
         green as one clump over {read} blade(s), {plain:.3} unclumped over {plain_read}"
    );
    assert!(
        read > 300 && plain_read > 300,
        "too few blades read: {read} and {plain_read}"
    );
    assert!(
        clumped >= CLUMPS_OVER_NONE * plain,
        "neighbouring clumps are {clumped:.3} times as far apart in colour as one clump, against \
         {plain:.3} unclumped: the clumps are not structure a picture shows"
    );
}

/// The columns either side of the frame's centre line a calm-half comparison
/// leaves out, in pixels — `grass_shells.rs`' margin, for its reason.
const CALM_MARGIN: u32 = 8;

/// **Calm blades stand upright** — decision 5's "calm means still", for a look
/// whose bend is applied in the vertex stage.
///
/// The blade meadow in its authored wind against the same field in no wind at
/// all: the right half of the two frames is the same pixels, and the left half,
/// in a strong wind, is not.
///
/// **Shown red by sabotage** (2026-09-17, radv): `grass.slang`'s
/// `grass_blade_curve` moving the tip by two millimetres per metre a second of
/// the weather's base speed — a wind that no longer reads the intensity layer
/// — and the artifacts recompiled. 5955 pixels of the calm half then differed.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn calm_blades_stand_upright_to_the_pixel() {
    let field = || Some(crcbl::screenshot::meadow_blades_field());
    let windy = frame_with(EXTENT, field(), MeadowWind::Windy);
    let still = frame_with(EXTENT, field(), MeadowWind::Calm);

    let centre = EXTENT.0 / 2;
    let calm = differing_in(&windy, &still, centre + CALM_MARGIN..EXTENT.0, 0..EXTENT.1);
    let bent = differing_in(&windy, &still, 0..centre - CALM_MARGIN, 0..EXTENT.1);
    eprintln!(
        "crcbl render e2e: meadow blades — {bent} pixel(s) differ on the windy half, {calm} on \
         the calm half"
    );
    assert!(
        bent > 500,
        "only {bent} pixels of the windy half moved in a strong wind, so the calm half's equality \
         says little"
    );
    assert_eq!(
        calm, 0,
        "{calm} pixel(s) of the calm half differ from the same blades in still air: a blade \
         standing where the intensity layer is zero leaned"
    );
}
