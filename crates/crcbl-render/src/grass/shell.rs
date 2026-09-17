//! The shell look's static block: `docs/plan/57-grass.md` rung G3's stack of
//! layers, and where the placement lattice its strands are looked up in stands.
//!
//! > **Shells**: the shell index is the instance index, so a field of shells is
//! > one instanced draw rather than one per layer as Acerola's repository draws
//! > them. His per-shell `pow(i/N, e)` height curve and ambient-occlusion term
//! > are per-shell constants computed on the CPU into a table, which removes
//! > `pow` from the shader; the per-cell hash is an integer hash.
//!
//! [`field_block_of`] is that table and the numbers around it. What a shell
//! draws with them is `grass.slang`'s — see its header — and which cells carry
//! a strand is the generation pass's, so nothing here decides a strand.

use crcbl_shaders::grass::{FieldBlock, shell_layers};

use super::field::{CELLS_PER_TILE, GrassField, SLOT_CAPACITY};

/// The block `grass.slang` reads a field's lattice, ground and shell stack
/// from.
///
/// Built for every field, a card field's included, because the shells' pipeline
/// binds it whether or not a frame draws with it; a field with no shell row
/// carries a stack of zero metres, and records no shell draw to read it.
#[must_use]
pub fn field_block_of(field: &GrassField) -> FieldBlock {
    let ground = field.ground();
    let cover = field.cover();
    let [origin_x, origin_z] = field.origin();
    let [tiles_x, tiles_z] = field.tiles();
    let shells = field.shells();
    let lod = field.blade_lod();
    FieldBlock {
        origin: [
            origin_x,
            origin_z,
            field.tile_size(),
            field.tile_size() / CELLS_PER_TILE as f32,
        ],
        tiles: [tiles_x, tiles_z, CELLS_PER_TILE, SLOT_CAPACITY],
        ground: [
            ground.origin[0],
            ground.origin[1],
            ground.metres_per_texel,
            1.0 / ground.metres_per_texel,
        ],
        maps: [
            ground.texels[0],
            ground.texels[1],
            cover.texels[0],
            cover.texels[1],
        ],
        stack: [
            field.shell_stack(),
            shells.count as f32,
            f32::from(u8::from(shells.fins)),
            0.0,
        ],
        blades: [lod.distance, lod.band, 0.0, 0.0],
        layers: shell_layers(shells.count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grass::field::tests::{flat_cover, flat_ground, one_blade};
    use crate::grass::field::{BladeLook, Shells};
    use crcbl_shaders::grass::{
        DEFAULT_SHELLS, FIN_GRAZE_FULL, FIN_GRAZE_START, MAX_SHELLS, SHELL_OCCLUSION_BIAS,
    };

    fn field(look: BladeLook, shells: Shells) -> GrassField {
        GrassField::new(
            [3, 2],
            4.0,
            [-6.0, 1.5],
            50.0,
            flat_ground(16, 0.0),
            flat_cover(16, 200),
            vec![crate::grass::BladeType {
                look,
                ..one_blade()[0]
            }],
        )
        .and_then(|field| field.with_shells(shells))
        .expect("a real field")
    }

    /// **The block is the field's own numbers**: its lattice, its ground's
    /// reciprocal — the one the generation pass multiplies by — and a stack as
    /// tall as its shell row.
    #[test]
    fn the_block_carries_the_fields_lattice_and_stack() {
        let shells = Shells {
            count: 8,
            fins: false,
        };
        let block = field_block_of(&field(BladeLook::Shells, shells));
        assert_eq!(block.origin, [-6.0, 1.5, 4.0, 4.0 / CELLS_PER_TILE as f32]);
        assert_eq!(block.tiles, [3, 2, CELLS_PER_TILE, SLOT_CAPACITY]);
        let generation = crate::grass::gen_params_of(&field(BladeLook::Shells, shells));
        assert_eq!(block.ground, generation.ground);
        assert_eq!(block.maps, generation.maps);
        assert_eq!(block.stack, [one_blade()[0].height, 8.0, 0.0, 0.0]);
        let lod = crate::grass::BladeLod::default();
        assert_eq!(block.blades, [lod.distance, lod.band, 0.0, 0.0]);
        // Rows past the count are zero, so a shader reading one reads nothing.
        assert_eq!(block.layers[8], [0.0; 4]);
        assert!(block.layers[7][0] > 0.0);
    }

    /// **A look switch moves the stack and nothing else of the block.** The
    /// lattice and the ground are the placement's, which decision 3 says a look
    /// must not touch.
    #[test]
    fn a_card_field_and_a_shell_field_share_every_number_but_the_stack() {
        let cards = field_block_of(&field(BladeLook::Cards, Shells::default()));
        let shells = field_block_of(&field(BladeLook::Shells, Shells::default()));
        assert_eq!(cards.stack[0], 0.0, "a card field has a shell stack");
        assert!(shells.stack[0] > 0.0);
        assert_eq!(
            FieldBlock {
                stack: shells.stack,
                ..cards
            },
            shells
        );
    }

    /// **The table climbs to the top of the stack and lights each layer more
    /// than the one under it**, and its lowest layer keeps the bias.
    ///
    /// Swept over every count a field may hold, since the table is what
    /// replaces a `pow` a fragment would otherwise evaluate — a curve that
    /// dipped or overshot anywhere would draw a layer out of order.
    #[test]
    fn the_shell_table_climbs_to_the_top_of_the_stack() {
        for count in 1..=MAX_SHELLS {
            let layers = shell_layers(count);
            let live = &layers[..count as usize];
            assert_eq!(live.last().expect("a layer")[0], 1.0, "{count} shells");
            for pair in live.windows(2) {
                assert!(pair[0][0] < pair[1][0], "{count} shells: heights {pair:?}");
                assert!(
                    pair[0][1] <= pair[1][1],
                    "{count} shells: occlusion {pair:?}"
                );
            }
            assert!(
                live[0][0] > 0.0,
                "the lowest of {count} shells is on the ground"
            );
            assert!(live[0][1] > SHELL_OCCLUSION_BIAS && live[0][1] <= 1.0);
            assert!(layers[count as usize..].iter().all(|row| *row == [0.0; 4]));
        }
    }

    /// **The table is the curve it documents**: height `s^1.5` and occlusion
    /// `s` plus the bias, `s = (i + 1) / count`. The platform's `powf` is only
    /// the oracle here; the table builds the power from exact operations.
    #[test]
    fn the_shell_table_is_its_documented_curve() {
        for count in 1..=MAX_SHELLS {
            let layers = shell_layers(count);
            for (shell, row) in layers[..count as usize].iter().enumerate() {
                let share = (shell as f32 + 1.0) / count as f32;
                let height = share.powf(1.5);
                assert!(
                    (row[0] - height).abs() <= f32::EPSILON * 2.0,
                    "shell {shell} of {count}: height {} against {height}",
                    row[0]
                );
                assert_eq!(row[1], (share + SHELL_OCCLUSION_BIAS).min(1.0));
            }
        }
    }

    /// The tallest shell row the fin thresholds are argued for, in metres —
    /// `crcbl::screenshot`'s meadow, whose tall row is this height.
    const ARGUED_STACK: f32 = 0.55;

    /// The side of the placement cell the fin thresholds are argued for, in
    /// metres: an eight-metre tile, the meadow's.
    const ARGUED_CELL: f32 = 8.0 / CELLS_PER_TILE as f32;

    /// How far above the parting elevation's sine the fins must start fading
    /// in, so a stack a little taller or a little sparser than the argued one
    /// still has its fins before its gaps.
    const GRAZE_MARGIN: f32 = 0.1;

    /// **The fins start before the shells part, with room to spare.**
    ///
    /// A ray at elevation `θ` travels `gap / tan θ` sideways between two
    /// layers, and passes between them without meeting a strand once that is
    /// wider than the strand. The earliest that can happen — the steepest view
    /// that sees ground through the stack — is at the **widest** gap the
    /// table leaves, against the **widest** a strand can be: a whole cell, a
    /// root at half a cell's reach either side. Any narrower strand parts at a
    /// shallower angle, where the fins are further in still. So the sine of
    /// that elevation, for [`DEFAULT_SHELLS`] layers over [`ARGUED_STACK`], has
    /// to sit [`GRAZE_MARGIN`] under `FIN_GRAZE_START`.
    ///
    /// **Shown red by sabotage** (2026-09-17): `FIN_GRAZE_START` at its first
    /// value, 0.35, against a parting sine this measures at 0.376.
    #[test]
    fn the_fins_open_before_the_shells_do() {
        let layers = shell_layers(DEFAULT_SHELLS);
        let widest_gap = layers[..DEFAULT_SHELLS as usize]
            .windows(2)
            .map(|pair| (pair[1][0] - pair[0][0]) * ARGUED_STACK)
            .fold(0.0f32, f32::max);
        // `tan θ = gap / strand`, and the fins speak in `sin θ`.
        let tangent = widest_gap / ARGUED_CELL;
        let parting = tangent / (1.0 + tangent * tangent).sqrt();
        eprintln!(
            "grass shells: the widest gap is {widest_gap:.4} m, parting a {ARGUED_CELL} m strand \
             below sin θ = {parting:.3}; fins fade in from {FIN_GRAZE_START} to {FIN_GRAZE_FULL}"
        );
        assert!(
            parting + GRAZE_MARGIN <= FIN_GRAZE_START,
            "the stack parts at sin θ = {parting}, within {GRAZE_MARGIN} of where the fins start \
             at {FIN_GRAZE_START}"
        );
        const { assert!(FIN_GRAZE_FULL < FIN_GRAZE_START) };
    }
}
