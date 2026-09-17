//! The CPU copy of `grass_gen.slang`'s placement, and the question gameplay
//! asks of it.
//!
//! `docs/plan/57-grass.md`'s decision 2 draws the line: "**Gameplay never reads
//! animated grass.** … The CPU answers 'is this position in grass, and how tall'
//! from the static maps; everything that moves is visual only." [`cover_at`] is
//! that answer, and it reads the cover map and the blade table — never a blade,
//! never the wind.
//!
//! [`blades_of_tile`] is the whole placement, and it exists for the rung's own
//! check: "the same tile generates the same blades, in the same slots, on every
//! backend — compared as instance data read back". A comparison needs something
//! to compare *against*, and a second GPU run is not that: two runs of one
//! shader agree about a hash that is wrong in the same way twice.
//!
//! # What is exact, and what is not
//!
//! **Which blades exist is bit-identical**, and that is the load-bearing half.
//! The cell id is an integer, [`crcbl_shaders::grass::hash`] is integer, the
//! cover map is read with `Load` and its density compared as the eight-bit
//! number the texel holds — so the accepted set, every cell's jitter, its blade
//! row, its facing and its size lanes are the same bits here and on every
//! target. So are its clump, the offset from the clump's point and whether the
//! far level of detail keeps it: [`crcbl_shaders::grass::clump_of`] and
//! [`crcbl_shaders::grass::kept_far`] are integer searches over the cell's
//! place in the whole field.
//!
//! **A root's height and the ground's normal are not.** Both are a bilinear
//! blend of four texels, written in the same order in both copies, and a shader
//! compiler may contract each multiply-add into an FMA where Rust never does.
//! The two therefore agree to within the last place of an `f32`, which is the
//! same freedom [`crcbl_shaders::wind::MAX_CPU_GPU_ERROR`] is written for and
//! the reason `crates/crcbl/tests/render_e2e/grass.rs` compares those two fields
//! under a tolerance and everything else exactly.

use crcbl_shaders::grass::{
    CLUMP_UNITS, FACING_SALT, SIZE_SALT, TINT_SALT, accepts, clump_of, hash, kept_far, unit_pair,
};

use super::field::{CELLS_PER_TILE, GrassField, SLOT_CAPACITY};

/// One blade, as the CPU places it.
///
/// Everything a generated instance carries except the lean, which is the wind's
/// and is visual only — decision 2's last line, and the reason this struct has
/// no field for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blade {
    /// The blade's cell in the field: `slot · SLOT_CAPACITY + lane`, which is
    /// the lane every hash below is drawn from and the key a determinism
    /// comparison sorts on.
    pub cell: u32,
    /// The row of the field's blade table it was given.
    pub row: u32,
    /// The world-space point it stands at.
    pub root: [f32; 3],
    /// Its height in metres, after its own hash has taken a share of the row's.
    pub height: f32,
    /// Half its card's width in metres, likewise.
    pub half_width: f32,
    /// The unit direction its card faces, on the XZ plane.
    pub facing: [f32; 2],
    /// The ground's unit normal under it, which is the normal it shades by.
    pub ground: [f32; 3],
    /// Its own tint lane, in `0..1`.
    pub tint: f32,
    /// Its clump's identity — `crcbl_shaders::grass::Clump::id`.
    pub clump: u32,
    /// Where it stands from its clump's point, in metres along `+X` and `+Z`.
    pub clump_offset: [f32; 2],
    /// Whether the far level of detail keeps it.
    pub kept_far: bool,
}

/// Every blade tile `slot` places, in cell order.
///
/// **No cull.** The generation dispatch drops a blade whose root is further than
/// the field's reach from the camera, and there is no camera here: this answers
/// what the field *has*, and the pass answers what a frame *draws*. A comparison
/// between the two therefore holds "every blade the GPU emitted is one this
/// places" always, and holds set equality when the reach covers the tile — which
/// is how `crates/crcbl/tests/render_e2e/grass.rs` reads it.
///
/// # Panics
///
/// If `slot` is not one of `field`'s.
#[must_use]
pub fn blades_of_tile(field: &GrassField, slot: u32) -> Vec<Blade> {
    let origin = field.tile_origin(slot);
    let side = field.tile_size() / CELLS_PER_TILE as f32;
    let rows = field.blades();
    let [tile_x, tile_z] = [slot % field.tiles()[0], slot / field.tiles()[0]];
    let mut blades = Vec::new();
    for lane in 0..SLOT_CAPACITY {
        let cell = slot * SLOT_CAPACITY + lane;
        let jitter_lane = hash(cell);
        let jitter = unit_pair(jitter_lane);
        let along = [
            (lane % CELLS_PER_TILE) as f32,
            (lane / CELLS_PER_TILE) as f32,
        ];
        let world = [
            origin[0] + (along[0] + jitter[0]) * side,
            origin[1] + (along[1] + jitter[1]) * side,
        ];

        let [density, texel_row] = field.cover().under(world[0], world[1]);
        if !accepts(cell, density) {
            continue;
        }
        let row = u32::from(texel_row).min(rows.len() as u32 - 1);
        let blade = rows[row as usize];

        let global = [
            tile_x * CELLS_PER_TILE + lane % CELLS_PER_TILE,
            tile_z * CELLS_PER_TILE + lane / CELLS_PER_TILE,
        ];
        let clump = clump_of(global, jitter_lane);
        let (height, ground) = field.ground().under(world[0], world[1]);
        let spread = unit_pair(hash(cell ^ SIZE_SALT));
        let clump_height = unit_pair(hash(clump.id ^ SIZE_SALT))[0];
        let unit = side * (1.0 / CLUMP_UNITS as f32);
        blades.push(Blade {
            cell,
            row,
            root: [world[0], height, world[1]],
            height: blade.height
                * (1.0 - blade.height_spread * spread[0])
                * (1.0 - blade.clumping.height * clump_height),
            half_width: blade.half_width * (1.0 - blade.width_spread * spread[1]),
            facing: facing_of(cell, clump.id, blade.clumping.facing),
            ground,
            tint: unit_pair(hash(cell ^ TINT_SALT))[0],
            clump: clump.id,
            clump_offset: [clump.offset[0] as f32 * unit, clump.offset[1] as f32 * unit],
            kept_far: kept_far(global),
        });
    }
    blades
}

/// The unit direction cell `cell`'s blade faces, in clump `clump` with the row's
/// clump facing share `share`.
///
/// A point of the square, normalised — `grass_gen.slang`'s construction, and
/// decision 9's last line: a direction is a vector rather than an angle, so
/// nothing here reaches for a transcendental. The square is not the circle, so
/// a diagonal is about `sqrt(2)` times as likely as an axis. The clump's shared
/// point is drawn the same way from its identity, and `share` of the way toward
/// it is taken before the one normalisation — so a share of zero is the blade's
/// own point, bit for bit.
#[must_use]
pub fn facing_of(cell: u32, clump: u32, share: f32) -> [f32; 2] {
    let square_of = |lane: u32| {
        let pair = unit_pair(hash(lane ^ FACING_SALT));
        [pair[0] * 2.0 - 1.0, pair[1] * 2.0 - 1.0]
    };
    let (own, shared) = (square_of(cell), square_of(clump));
    let square = [
        own[0] + (shared[0] - own[0]) * share,
        own[1] + (shared[1] - own[1]) * share,
    ];
    let length_squared = square[0] * square[0] + square[1] * square[1];
    if length_squared > 1e-8 {
        let scale = 1.0 / length_squared.sqrt();
        [square[0] * scale, square[1] * scale]
    } else {
        [1.0, 0.0]
    }
}

/// Whether `(x, z)` stands in grass, and how tall that grass is.
///
/// **Decision 2's gameplay answer, and it is deliberately not a blade.** Ghost
/// of Tsushima's stealth grass "returns a constant height per type … for
/// consistency": what a crouching character is hidden by must not change because
/// a gust bent the blade beside them, or because a blade's own hash made it
/// short. So this reads the cover map's texel and the blade row's *authored*
/// height, and nothing that a frame drew.
///
/// [`None`] where the cover map says bare ground.
#[must_use]
pub fn cover_at(field: &GrassField, x: f32, z: f32) -> Option<f32> {
    let [density, row] = field.cover().under(x, z);
    if density == 0 {
        return None;
    }
    let rows = field.blades();
    let row = usize::from(row).min(rows.len() - 1);
    Some(rows[row].height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grass::field::tests::{flat_cover, flat_ground, one_blade};
    use crate::grass::field::{BladeType, CoverMap};

    fn field_with(cover: CoverMap, blades: Vec<BladeType>) -> GrassField {
        GrassField::new(
            [2, 1],
            8.0,
            [0.0, 0.0],
            1000.0,
            flat_ground(32, 0.0),
            cover,
            blades,
        )
        .expect("a real field")
    }

    /// **The same tile places the same blades twice**, bit for bit — the
    /// cheapest half of the rung's determinism check, and the half that holds
    /// on a machine with no GPU at all.
    #[test]
    fn a_tile_places_the_same_blades_twice() {
        let field = field_with(flat_cover(32, 200), one_blade());
        let first = blades_of_tile(&field, 0);
        assert_eq!(first, blades_of_tile(&field, 0));
        assert!(!first.is_empty(), "the tile placed nothing to compare");
    }

    /// **Every blade is inside its own tile**, and its cell is unique across the
    /// field — which is what makes the cell a key a comparison can sort on.
    #[test]
    fn every_blade_lands_in_its_own_tile_under_a_cell_of_its_own() {
        let field = field_with(flat_cover(32, 255), one_blade());
        let mut cells = std::collections::HashSet::new();
        for slot in 0..field.slots() {
            let [ox, oz] = field.tile_origin(slot);
            let side = field.tile_size();
            for blade in blades_of_tile(&field, slot) {
                assert!(
                    (ox..ox + side).contains(&blade.root[0])
                        && (oz..oz + side).contains(&blade.root[2]),
                    "cell {} landed at {:?}, outside tile {slot} at ({ox}, {oz})",
                    blade.cell,
                    blade.root
                );
                assert!(cells.insert(blade.cell), "cell {} twice", blade.cell);
                let length = blade.facing[0].hypot(blade.facing[1]);
                assert!(
                    (length - 1.0).abs() < 1e-6,
                    "{:?} is not unit",
                    blade.facing
                );
            }
        }
    }

    /// **A denser tile places more blades**, monotonically over the whole
    /// eight-bit range — the claim the golden's own density relation rests on,
    /// measured where it is cheap to sweep.
    #[test]
    fn a_denser_cover_texel_places_more_blades() {
        let mut last = 0;
        for density in [0u8, 32, 64, 128, 192, 255] {
            let field = field_with(flat_cover(32, density), one_blade());
            let placed = blades_of_tile(&field, 0).len();
            eprintln!("grass placement: density {density} placed {placed} blade(s)");
            assert!(
                placed >= last,
                "density {density} placed {placed}, fewer than the {last} the density under it did"
            );
            if density == 0 {
                assert_eq!(placed, 0, "bare ground placed {placed} blade(s)");
            }
            last = placed;
        }
        assert!(
            last as u32 > SLOT_CAPACITY - 64,
            "full density placed only {last} of {SLOT_CAPACITY} cells"
        );
    }

    /// **A blade is never taller or wider than its row**, and never shorter than
    /// the spread allows — the spread takes a share away rather than adding one.
    #[test]
    fn a_blade_stays_inside_its_rows_size() {
        let row = one_blade()[0];
        let field = field_with(flat_cover(32, 255), one_blade());
        let blades = blades_of_tile(&field, 0);
        let (mut shortest, mut tallest) = (f32::INFINITY, 0.0f32);
        for blade in &blades {
            assert!(blade.height <= row.height, "{} m", blade.height);
            assert!(blade.half_width <= row.half_width);
            assert!(blade.height >= row.height * (1.0 - row.height_spread));
            assert!((0.0..1.0).contains(&blade.tint));
            shortest = shortest.min(blade.height);
            tallest = tallest.max(blade.height);
        }
        eprintln!(
            "grass placement: heights {shortest} m to {tallest} m over {} blades",
            blades.len()
        );
        // And the spread is actually used: a field whose blades were all the
        // row's height would satisfy every bound above.
        assert!(
            tallest - shortest > row.height * row.height_spread * 0.9,
            "the heights span only {} m of the {} m the spread allows",
            tallest - shortest,
            row.height * row.height_spread
        );
    }

    /// **The cover map's row picks the blade**, and a row past the table is
    /// clamped into it rather than indexing out.
    #[test]
    fn the_cover_maps_row_picks_the_blade() {
        let mut rows = one_blade();
        rows.push(BladeType {
            height: 0.05,
            ..rows[0]
        });
        let mut cover = flat_cover(32, 255);
        for (at, texel) in cover.cover.iter_mut().enumerate() {
            texel[1] = if at % 2 == 0 { 0 } else { 1 };
        }
        // And one texel naming a row the table does not hold.
        cover.cover[0][1] = 200;
        let field = field_with(cover, rows.clone());
        let mut seen = [0usize; 2];
        for blade in blades_of_tile(&field, 0) {
            seen[blade.row as usize] += 1;
            assert!(blade.height <= rows[blade.row as usize].height);
        }
        assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
        // The out-of-range texel is under the field's first cells, and it
        // resolves to the last row rather than to nothing.
        assert_eq!(
            u32::from(200u8).min(rows.len() as u32 - 1),
            1,
            "a row past the table clamps into it"
        );
    }

    /// **No clumping is the placement clumps were added to, bit for bit**, and
    /// every blade carries a clump and a far-level decision all the same.
    ///
    /// The height and the facing are recomputed here from the formulas with no
    /// clump term at all, and compared exactly: a share of zero multiplies by
    /// one and moves the facing's point by nothing.
    ///
    /// **Shown red by sabotage** (2026-09-17): the clump's share of the height
    /// fixed at a quarter whatever the row asks for.
    #[test]
    fn no_clumping_leaves_every_blade_where_it_stood() {
        let row = one_blade()[0];
        let field = field_with(flat_cover(32, 255), one_blade());
        let blades = blades_of_tile(&field, 1);
        for blade in &blades {
            let spread = unit_pair(hash(blade.cell ^ SIZE_SALT));
            assert_eq!(
                blade.height,
                row.height * (1.0 - row.height_spread * spread[0])
            );
            let pair = unit_pair(hash(blade.cell ^ FACING_SALT));
            let square = [pair[0] * 2.0 - 1.0, pair[1] * 2.0 - 1.0];
            let scale = 1.0 / (square[0] * square[0] + square[1] * square[1]).sqrt();
            assert_eq!(blade.facing, [square[0] * scale, square[1] * scale]);
        }
        // Tile 1 is the second along `+X`, so its cells' places start a tile
        // over — and the far level keeps about a quarter of them.
        let kept = blades.iter().filter(|blade| blade.kept_far).count();
        let clumps: std::collections::HashSet<u32> =
            blades.iter().map(|blade| blade.clump).collect();
        eprintln!(
            "grass placement: {kept} of {} blades kept far, {} clumps",
            blades.len(),
            clumps.len()
        );
        assert!((blades.len() / 5..blades.len() / 3).contains(&kept));
        assert!(clumps.len() > 30, "{} clumps over a tile", clumps.len());
    }

    /// **A clumped row stands by its clump**: at a full facing share every
    /// blade of a clump faces one way, and a clump's height share lowers every
    /// blade of the clump by one shared fraction.
    ///
    /// **Shown red by sabotage** (2026-09-17): the shared facing drawn from the
    /// blade's own cell rather than its clump.
    #[test]
    fn a_clumped_row_faces_and_stands_by_its_clump() {
        let clumped = |facing, height| {
            vec![BladeType {
                clumping: crate::grass::field::Clumping { facing, height },
                ..one_blade()[0]
            }]
        };
        let plain = blades_of_tile(&field_with(flat_cover(32, 255), clumped(0.0, 0.0)), 0);
        let full = blades_of_tile(&field_with(flat_cover(32, 255), clumped(1.0, 0.5)), 0);
        assert_eq!(plain.len(), full.len());
        let mut by_clump: std::collections::HashMap<u32, Vec<(Blade, Blade)>> =
            std::collections::HashMap::new();
        for (before, after) in plain.iter().zip(&full) {
            assert_eq!((before.cell, before.clump), (after.cell, after.clump));
            by_clump
                .entry(after.clump)
                .or_default()
                .push((*before, *after));
        }
        let mut shared_facings = 0usize;
        for blades in by_clump.values() {
            let facing = blades[0].1.facing;
            let ratio = blades[0].1.height / blades[0].0.height;
            for (before, after) in blades {
                assert_eq!(after.facing, facing, "clump {} faces two ways", after.clump);
                assert!(
                    (after.height / before.height - ratio).abs() < 1e-5,
                    "clump {} lowers its blades by two shares",
                    after.clump
                );
                assert!((0.5..=1.0).contains(&(after.height / before.height)));
            }
            shared_facings += usize::from(blades.len() > 1);
        }
        eprintln!(
            "grass placement: {} clumps over a tile, {shared_facings} with more than one blade",
            by_clump.len()
        );
        assert!(shared_facings > 20);
    }

    /// **Gameplay reads the row's height, not a blade's** — so a field of blades
    /// whose own hashes made them short still hides a character to the row's
    /// height, and a bare texel answers nothing.
    #[test]
    fn the_gameplay_query_answers_the_rows_height_and_not_a_blades() {
        let mut rows = one_blade();
        rows.push(BladeType {
            height: 1.25,
            ..rows[0]
        });
        let mut cover = flat_cover(32, 255);
        cover.cover[5 + 32 * 5] = [0, 0];
        cover.cover[6 + 32 * 5] = [180, 1];
        let field = field_with(cover, rows.clone());
        assert_eq!(cover_at(&field, 5.0, 5.0), None);
        assert_eq!(cover_at(&field, 6.0, 5.0), Some(1.25));
        assert_eq!(cover_at(&field, 7.0, 5.0), Some(rows[0].height));
        // Every blade of that row is shorter than the answer, which is the
        // point: the query is a property of the map, not of what grew.
        for blade in blades_of_tile(&field, 0).iter().filter(|b| b.row == 1) {
            assert!(blade.height <= 1.25);
        }
    }
}
