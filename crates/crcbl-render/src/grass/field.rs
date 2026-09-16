//! What a patch of grass *is*, as data — `docs/plan/57-grass.md`'s decision 2:
//! "A field is a description: bounds, a ground heightfield (texture plus CPU
//! copy), a density map and a type map, and a table of blade types."
//!
//! ```text
//!  origin ─┬─────────── tiles[0] · tile_size ───────────┐
//!          │ tile 0 │ tile 1 │ tile 2 │ tile 3 │  …     │   +X
//!          ├────────┼────────┼────────┼────────┤        │
//!          │ tile 4 │ tile 5 │ …                        │
//!          ⋮                                            ⋮   +Z
//! ```
//!
//! Every tile is [`CELLS_PER_TILE`] cells on a side and every cell may carry one
//! blade, so a tile's slot holds [`SLOT_CAPACITY`] instances and no more — which
//! is what lets the generation pass append without a bound check.
//!
//! # Two maps, one texel each
//!
//! The plan names a density map and a type map. They are **one** map here, two
//! channels of it: a cell reads one texel and gets both numbers, where two maps
//! would be two fetches of two textures whose extents would then have to be
//! argued about. [`CoverMap`] is that map, and `cover` is what this crate calls
//! it so neither half's name claims the whole.
//!
//! # No texture is named by a field
//!
//! The card every blade is drawn with is the engine's, authored in arithmetic by
//! [`super::card`]. A field that named an image would need an asset seam, a
//! page and a cook of its own, and at this rung there is one look; `docs/backlog.md`
//! carries the authored page.

use crcbl_shaders::grass::{BLADE_STRIDE, GrassBlade, INSTANCE_STRIDE};

/// Cells along one side of a tile's placement grid.
///
/// **The generation dispatch's lane count and the slot's capacity are the same
/// number**, which is what makes the append unbounded-check-free: one lane per
/// cell and one instance per surviving lane means the count cannot pass the
/// capacity. [`SLOT_CAPACITY`] is the square of this, and
/// `the_slot_holds_one_instance_per_cell` is where the two are held together.
pub const CELLS_PER_TILE: u32 = 64;

/// Instances one tile's slot holds.
pub const SLOT_CAPACITY: u32 = CELLS_PER_TILE * CELLS_PER_TILE;

/// The most tiles a field may hold.
///
/// A ceiling rather than a budget: the instance buffer is
/// `tiles · SLOT_CAPACITY · INSTANCE_STRIDE` bytes, which at this cap is 21 MiB
/// — under WebGPU's 128 MiB default maximum storage binding size, which is what
/// decision 1 asks a ring to stay inside.
pub const MAX_TILES: u32 = 64;

/// The most blade rows a field's table may hold.
///
/// The cover map's green channel indexes it and that channel holds 256 values,
/// so this is a choice about how many looks one field mixes rather than a limit
/// the encoding forces.
pub const MAX_BLADES: u32 = 16;

/// Why a field could not be built.
///
/// One variant per way of being wrong rather than one string, on
/// `crcbl_wind::WindError`'s terms: a caller can tell a map that has not been
/// filled in from one that is the wrong shape, and the message names the numbers
/// it refused.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GrassError {
    /// A field with no tiles along one of its axes, or more than [`MAX_TILES`]
    /// of them.
    #[error("a {x}x{z} field of tiles is not one this crate will lay out")]
    Tiles {
        /// Tiles along `+X`.
        x: u32,
        /// Tiles along `+Z`.
        z: u32,
    },
    /// A tile side, a reach or a texel scale that is not a positive finite
    /// number of metres.
    #[error("a grass field's {what} of {metres} m is not a distance")]
    Metres {
        /// Which number was refused.
        what: &'static str,
        /// What was offered, as its bits printed — a `NaN` reaches here too.
        metres: f32,
    },
    /// A map with a zero dimension.
    #[error("a {width}x{height} grass {what} map has no texels")]
    EmptyMap {
        /// Which map was refused.
        what: &'static str,
        /// Texels along `+X`.
        width: u32,
        /// Texels along `+Z`.
        height: u32,
    },
    /// A map whose contents are not one entry per texel of the grid it was
    /// handed with.
    #[error("a {width}x{height} grass {what} map holds {expected} texels, and {found} arrived")]
    MapSize {
        /// Which map was refused.
        what: &'static str,
        /// Texels along `+X`.
        width: u32,
        /// Texels along `+Z`.
        height: u32,
        /// Entries the grid needs.
        expected: usize,
        /// Entries that arrived.
        found: usize,
    },
    /// A height that is not a finite number of metres.
    #[error("the grass ground field holds {height} at texel {at}, which is not a height")]
    Height {
        /// What was offered.
        height: f32,
        /// Where it was.
        at: usize,
    },
    /// No blade rows, or more than [`MAX_BLADES`] of them.
    #[error("a blade table of {rows} row(s) is not one this crate will hold")]
    Blades {
        /// Rows that arrived.
        rows: usize,
    },
    /// A blade row whose size or spread is not a number a card can be built
    /// from.
    #[error("blade row {row}'s {what} of {value} is not one a card is drawn with")]
    Blade {
        /// Which row was refused.
        row: usize,
        /// Which field of it.
        what: &'static str,
        /// What was offered.
        value: f32,
    },
}

/// The ground a field stands on: one height per texel, and the CPU copy of it.
///
/// **There is no terrain system**, which decision 2 says outright — "a field
/// takes them from a heightfield the caller supplies until one exists". This is
/// that heightfield.
///
/// Read with `Load` and blended by hand in both copies, never sampled: a
/// hardware bilinear filter has eight bits of sub-texel precision by
/// specification, and the CPU copy of this placement has all of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightfield {
    /// Texels along `+X` and along `+Z`.
    pub texels: [u32; 2],
    /// Metres of world between neighbouring texels.
    pub metres_per_texel: f32,
    /// The world XZ that texel `(0, 0)` stands at.
    pub origin: [f32; 2],
    /// One height in metres per texel, row-major along `+X` then `+Z`.
    pub heights: Vec<f32>,
}

/// The density and blade-row map — decision 2's density map and type map, as
/// two channels of one texel.
#[derive(Clone, Debug, PartialEq)]
pub struct CoverMap {
    /// Texels along `+X` and along `+Z`.
    pub texels: [u32; 2],
    /// Metres of world between neighbouring texels.
    pub metres_per_texel: f32,
    /// The world XZ that texel `(0, 0)` stands at.
    pub origin: [f32; 2],
    /// `[density, row]` per texel, row-major. A density of zero is bare ground
    /// and 255 is every cell; the row indexes [`GrassField::blades`] and is
    /// clamped to it.
    pub cover: Vec<[u8; 2]>,
}

/// One row of the blade table — decision 3's "one blade description", of which
/// rung G1 draws the card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeType {
    /// The colour at the blade's root. Decision 4's root-occlusion lever: a
    /// darker root is what makes a field read as having depth.
    pub root_color: [f32; 3],
    /// The colour at its tip.
    pub tip_color: [f32; 3],
    /// The tallest a blade of this row stands, in metres.
    pub height: f32,
    /// The widest half a card of this row is, in metres.
    pub half_width: f32,
    /// The fraction of [`BladeType::height`] a blade may lose to its own hash,
    /// in `0..1`.
    pub height_spread: f32,
    /// The same for [`BladeType::half_width`].
    pub width_spread: f32,
}

impl BladeType {
    /// The row as the shader reads it.
    #[must_use]
    pub fn row(&self) -> GrassBlade {
        let [root_r, root_g, root_b] = self.root_color;
        let [tip_r, tip_g, tip_b] = self.tip_color;
        GrassBlade {
            root_color: [root_r, root_g, root_b, 0.0],
            tip_color: [tip_r, tip_g, tip_b, 0.0],
            size: [
                self.height,
                self.half_width,
                self.height_spread,
                self.width_spread,
            ],
        }
    }
}

/// A patch of grass, as data.
///
/// Built through [`GrassField::new`], which is the only way: every number below
/// is read by a shader that cannot check it, and a field that reached the device
/// with a zero texel scale would divide by it there.
#[derive(Clone, Debug, PartialEq)]
pub struct GrassField {
    tiles: [u32; 2],
    tile_size: f32,
    origin: [f32; 2],
    reach: f32,
    ground: Heightfield,
    cover: CoverMap,
    blades: Vec<BladeType>,
}

impl GrassField {
    /// A field of `tiles` tiles of `tile_size` metres, with its minimum corner
    /// at `origin` on the XZ plane.
    ///
    /// `reach` is how far from the camera a blade is still generated — decision
    /// 7's distance cull, which is the only cull this rung spends.
    ///
    /// # Errors
    ///
    /// A [`GrassError`] naming the first number refused. Nothing is built from a
    /// field that failed, so a caller may hand the same description back after
    /// fixing it.
    pub fn new(
        tiles: [u32; 2],
        tile_size: f32,
        origin: [f32; 2],
        reach: f32,
        ground: Heightfield,
        cover: CoverMap,
        blades: Vec<BladeType>,
    ) -> Result<Self, GrassError> {
        let [x, z] = tiles;
        if x == 0 || z == 0 || x.saturating_mul(z) > MAX_TILES {
            return Err(GrassError::Tiles { x, z });
        }
        for (what, metres) in [("tile side", tile_size), ("reach", reach)] {
            if !(metres.is_finite() && metres > 0.0) {
                return Err(GrassError::Metres { what, metres });
            }
        }
        for value in origin {
            if !value.is_finite() {
                return Err(GrassError::Metres {
                    what: "origin",
                    metres: value,
                });
            }
        }
        ground.check("ground")?;
        cover.check()?;
        if blades.is_empty() || blades.len() > MAX_BLADES as usize {
            return Err(GrassError::Blades { rows: blades.len() });
        }
        for (row, blade) in blades.iter().enumerate() {
            for (what, value, positive) in [
                ("height", blade.height, true),
                ("half-width", blade.half_width, true),
                ("height spread", blade.height_spread, false),
                ("width spread", blade.width_spread, false),
            ] {
                let ok = value.is_finite()
                    && if positive {
                        value > 0.0
                    } else {
                        (0.0..1.0).contains(&value)
                    };
                if !ok {
                    return Err(GrassError::Blade { row, what, value });
                }
            }
        }
        Ok(Self {
            tiles,
            tile_size,
            origin,
            reach,
            ground,
            cover,
            blades,
        })
    }

    /// Tiles along `+X` and along `+Z`.
    #[must_use]
    pub const fn tiles(&self) -> [u32; 2] {
        self.tiles
    }

    /// How many tiles there are, which is how many slots and how many draws.
    #[must_use]
    pub const fn slots(&self) -> u32 {
        self.tiles[0] * self.tiles[1]
    }

    /// One tile's side, in metres.
    #[must_use]
    pub const fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// The world XZ of the field's minimum corner.
    #[must_use]
    pub const fn origin(&self) -> [f32; 2] {
        self.origin
    }

    /// How far from the camera a blade is still generated, in metres.
    #[must_use]
    pub const fn reach(&self) -> f32 {
        self.reach
    }

    /// The ground the field stands on.
    #[must_use]
    pub const fn ground(&self) -> &Heightfield {
        &self.ground
    }

    /// The density and blade-row map.
    #[must_use]
    pub const fn cover(&self) -> &CoverMap {
        &self.cover
    }

    /// The blade table the cover map's rows index.
    #[must_use]
    pub fn blades(&self) -> &[BladeType] {
        &self.blades
    }

    /// The world XZ of tile `slot`'s minimum corner.
    ///
    /// # Panics
    ///
    /// If `slot` is not one of this field's — the caller holds the field and
    /// [`GrassField::slots`] says how many there are.
    #[must_use]
    pub fn tile_origin(&self, slot: u32) -> [f32; 2] {
        assert!(
            slot < self.slots(),
            "slot {slot} is not one of this field's"
        );
        let [x, z] = [slot % self.tiles[0], slot / self.tiles[0]];
        [
            self.origin[0] + x as f32 * self.tile_size,
            self.origin[1] + z as f32 * self.tile_size,
        ]
    }

    /// Bytes the instance buffer needs for this field.
    #[must_use]
    pub fn instance_bytes(&self) -> u64 {
        u64::from(self.slots()) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64
    }

    /// Bytes the blade table needs.
    #[must_use]
    pub fn blade_bytes(&self) -> u64 {
        self.blades.len() as u64 * BLADE_STRIDE as u64
    }
}

impl Heightfield {
    /// The height at `(x, z)`, blended bilinearly from the four texels around
    /// it, and the ground's unit normal there.
    ///
    /// **`grass_ground_under` in `grass_gen.slang` is the same arithmetic in the
    /// same order**, which is what lets a blade's root be compared between the
    /// two copies. It is not bit-identical: a shader compiler may contract each
    /// multiply-add here into an FMA and Rust never does, so the two agree to
    /// the last place of an `f32` rather than to the bit.
    #[must_use]
    pub fn under(&self, x: f32, z: f32) -> (f32, [f32; 3]) {
        let inverse = 1.0 / self.metres_per_texel;
        let at = [
            (x - self.origin[0]) * inverse,
            (z - self.origin[1]) * inverse,
        ];
        let base = [at[0].floor(), at[1].floor()];
        let blend = [at[0] - base[0], at[1] - base[1]];
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`floor` has already made these integral, so the cast is exact"
        )]
        let texel = [base[0] as i32, base[1] as i32];
        let corner = |dx: i32, dz: i32| self.texel(texel[0] + dx, texel[1] + dz);
        let (h00, h10, h01, h11) = (corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1));
        let lower = h00 + blend[0] * (h10 - h00);
        let upper = h01 + blend[0] * (h11 - h01);
        let height = lower + blend[1] * (upper - lower);
        let rise_x = 0.5 * ((h10 - h00) + (h11 - h01));
        let rise_z = 0.5 * ((h01 - h00) + (h11 - h10));
        let normal = normalise([-rise_x, self.metres_per_texel, -rise_z]);
        (height, normal)
    }

    /// One texel, clamped to the field's edge.
    fn texel(&self, x: i32, z: i32) -> f32 {
        let [width, height] = self.texels;
        #[expect(
            clippy::cast_possible_wrap,
            reason = "`check` refuses a map larger than an `i32` holds"
        )]
        let last = [width as i32 - 1, height as i32 - 1];
        let clamped = [x.clamp(0, last[0]), z.clamp(0, last[1])];
        #[expect(
            clippy::cast_sign_loss,
            reason = "clamped into `0..=last`, which is non-negative"
        )]
        let at = clamped[1] as usize * width as usize + clamped[0] as usize;
        self.heights[at]
    }

    /// The bytes of the `R32Float` image this is uploaded as.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.heights
            .iter()
            .flat_map(|height| height.to_le_bytes())
            .collect()
    }

    fn check(&self, what: &'static str) -> Result<(), GrassError> {
        let [width, height] = self.texels;
        check_grid(what, self.texels, self.metres_per_texel, self.origin)?;
        let expected = width as usize * height as usize;
        if self.heights.len() != expected {
            return Err(GrassError::MapSize {
                what,
                width,
                height,
                expected,
                found: self.heights.len(),
            });
        }
        for (at, value) in self.heights.iter().enumerate() {
            if !value.is_finite() {
                return Err(GrassError::Height { height: *value, at });
            }
        }
        Ok(())
    }
}

impl CoverMap {
    /// The texel under `(x, z)` — nearest, never blended, which is what makes
    /// the density test that reads it an integer comparison.
    ///
    /// `grass_cover_under` in `grass_gen.slang` is the same three lines.
    #[must_use]
    pub fn under(&self, x: f32, z: f32) -> [u8; 2] {
        let inverse = 1.0 / self.metres_per_texel;
        let at = [
            (x - self.origin[0]) * inverse,
            (z - self.origin[1]) * inverse,
        ];
        let [width, height] = self.texels;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "`floor` has made these integral and `check` bounds the extents"
        )]
        let texel = [
            ((at[0] + 0.5).floor() as i32).clamp(0, width as i32 - 1),
            ((at[1] + 0.5).floor() as i32).clamp(0, height as i32 - 1),
        ];
        #[expect(
            clippy::cast_sign_loss,
            reason = "clamped into `0..=last`, which is non-negative"
        )]
        let at = texel[1] as usize * width as usize + texel[0] as usize;
        self.cover[at]
    }

    /// The bytes of the `Rgba8Unorm` image this is uploaded as: density in red,
    /// the blade row in green, and zeroes in the two channels no shader reads.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.cover
            .iter()
            .flat_map(|[density, row]| [*density, *row, 0, 0])
            .collect()
    }

    fn check(&self) -> Result<(), GrassError> {
        let [width, height] = self.texels;
        check_grid("cover", self.texels, self.metres_per_texel, self.origin)?;
        let expected = width as usize * height as usize;
        if self.cover.len() != expected {
            return Err(GrassError::MapSize {
                what: "cover",
                width,
                height,
                expected,
                found: self.cover.len(),
            });
        }
        Ok(())
    }
}

/// The extent, scale and origin checks both maps share.
fn check_grid(
    what: &'static str,
    texels: [u32; 2],
    metres_per_texel: f32,
    origin: [f32; 2],
) -> Result<(), GrassError> {
    let [width, height] = texels;
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(GrassError::EmptyMap {
            what,
            width,
            height,
        });
    }
    if !(metres_per_texel.is_finite() && metres_per_texel > 0.0) {
        return Err(GrassError::Metres {
            what: "texel scale",
            metres: metres_per_texel,
        });
    }
    for value in origin {
        if !value.is_finite() {
            return Err(GrassError::Metres {
                what: "map origin",
                metres: value,
            });
        }
    }
    Ok(())
}

/// `vector` scaled to unit length, or `+Y` where it has no length to scale.
///
/// The shader's `normalize` with the degenerate case written out: a ground
/// field that is flat everywhere has a rise of zero on both axes and a texel
/// scale that `check_grid` has already refused to let be zero, so the fallback
/// is unreachable from a checked field — and it is here rather than absent
/// because the alternative is a `NaN` in a normal that shades every blade of a
/// tile black.
fn normalise(vector: [f32; 3]) -> [f32; 3] {
    let length_squared = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    if length_squared > 0.0 {
        let scale = 1.0 / length_squared.sqrt();
        [vector[0] * scale, vector[1] * scale, vector[2] * scale]
    } else {
        [0.0, 1.0, 0.0]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A flat ground field of `texels` square at one metre a texel.
    pub(crate) fn flat_ground(texels: u32, height: f32) -> Heightfield {
        Heightfield {
            texels: [texels, texels],
            metres_per_texel: 1.0,
            origin: [0.0, 0.0],
            heights: vec![height; (texels * texels) as usize],
        }
    }

    /// A cover map of `texels` square at one metre a texel, every texel at
    /// `density` on row zero.
    pub(crate) fn flat_cover(texels: u32, density: u8) -> CoverMap {
        CoverMap {
            texels: [texels, texels],
            metres_per_texel: 1.0,
            origin: [0.0, 0.0],
            cover: vec![[density, 0]; (texels * texels) as usize],
        }
    }

    pub(crate) fn one_blade() -> Vec<BladeType> {
        vec![BladeType {
            root_color: [0.05, 0.12, 0.03],
            tip_color: [0.35, 0.55, 0.15],
            height: 0.4,
            half_width: 0.03,
            height_spread: 0.4,
            width_spread: 0.3,
        }]
    }

    pub(super) fn field() -> GrassField {
        GrassField::new(
            [2, 2],
            8.0,
            [0.0, 0.0],
            100.0,
            flat_ground(32, 0.0),
            flat_cover(32, 255),
            one_blade(),
        )
        .expect("a real field")
    }

    /// **A slot holds exactly one instance per cell**, which is what lets the
    /// generation pass append with no bound on the atomic's result.
    #[test]
    fn the_slot_holds_one_instance_per_cell() {
        assert_eq!(SLOT_CAPACITY, CELLS_PER_TILE * CELLS_PER_TILE);
    }

    /// **The whole instance buffer fits a browser's guaranteed binding size**,
    /// which decision 1 names: "its 128 MiB default storage binding size means
    /// one instance buffer per ring rather than one for the field".
    #[test]
    fn the_widest_field_fits_a_browsers_storage_binding() {
        const WEBGPU_MAX_STORAGE_BINDING: u64 = 128 * 1024 * 1024;
        let widest = u64::from(MAX_TILES) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64;
        eprintln!("grass: the widest field's instances are {widest} byte(s)");
        assert!(
            widest <= WEBGPU_MAX_STORAGE_BINDING,
            "{MAX_TILES} tiles of {SLOT_CAPACITY} instances is {widest} bytes, past the \
             {WEBGPU_MAX_STORAGE_BINDING} a browser guarantees"
        );
    }

    /// Every number a shader divides by or indexes with is refused here, and
    /// each refusal names what it refused.
    #[test]
    fn a_field_refuses_every_number_a_shader_could_not_survive() {
        let build = |tiles, tile_size, reach, ground, cover, blades| {
            GrassField::new(tiles, tile_size, [0.0, 0.0], reach, ground, cover, blades)
        };
        assert_eq!(
            build(
                [0, 2],
                8.0,
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                one_blade()
            ),
            Err(GrassError::Tiles { x: 0, z: 2 })
        );
        assert_eq!(
            build(
                [MAX_TILES, 2],
                8.0,
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                one_blade()
            ),
            Err(GrassError::Tiles { x: MAX_TILES, z: 2 })
        );
        assert!(matches!(
            build(
                [1, 1],
                0.0,
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                one_blade()
            ),
            Err(GrassError::Metres {
                what: "tile side",
                ..
            })
        ));
        assert!(matches!(
            build(
                [1, 1],
                8.0,
                f32::NAN,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                one_blade()
            ),
            Err(GrassError::Metres { what: "reach", .. })
        ));
        let mut short = flat_ground(8, 0.0);
        short.heights.pop();
        assert!(matches!(
            build([1, 1], 8.0, 10.0, short, flat_cover(8, 8), one_blade()),
            Err(GrassError::MapSize { what: "ground", .. })
        ));
        let mut nan = flat_ground(8, 0.0);
        nan.heights[3] = f32::INFINITY;
        assert!(matches!(
            build([1, 1], 8.0, 10.0, nan, flat_cover(8, 8), one_blade()),
            Err(GrassError::Height { at: 3, .. })
        ));
        let mut scaleless = flat_cover(8, 8);
        scaleless.metres_per_texel = 0.0;
        assert!(matches!(
            build(
                [1, 1],
                8.0,
                10.0,
                flat_ground(8, 0.0),
                scaleless,
                one_blade()
            ),
            Err(GrassError::Metres {
                what: "texel scale",
                ..
            })
        ));
        assert_eq!(
            build(
                [1, 1],
                8.0,
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                Vec::new()
            ),
            Err(GrassError::Blades { rows: 0 })
        );
        let mut flat_blade = one_blade();
        flat_blade[0].height = 0.0;
        assert!(matches!(
            build(
                [1, 1],
                8.0,
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                flat_blade
            ),
            Err(GrassError::Blade {
                row: 0,
                what: "height",
                ..
            })
        ));
    }

    /// **Tiles are laid out along `+X` then `+Z`**, from the field's own corner,
    /// and every slot's origin is one tile side from its neighbour's.
    #[test]
    fn the_tiles_tile_the_field() {
        let field = GrassField::new(
            [3, 2],
            4.0,
            [-6.0, 10.0],
            50.0,
            flat_ground(32, 0.0),
            flat_cover(32, 128),
            one_blade(),
        )
        .expect("a real field");
        assert_eq!(field.slots(), 6);
        assert_eq!(field.tile_origin(0), [-6.0, 10.0]);
        assert_eq!(field.tile_origin(2), [2.0, 10.0]);
        assert_eq!(field.tile_origin(3), [-6.0, 14.0]);
        assert_eq!(field.tile_origin(5), [2.0, 14.0]);
    }

    /// **A slope reads back as its slope**, and a flat field's normal is `+Y`.
    ///
    /// The heights rise by one metre per texel along `+X`, so the ground leans
    /// at 45° about `+Z` and the normal's `x` and `y` are equal and opposite in
    /// sign to the rise.
    #[test]
    fn the_ground_answers_its_own_height_and_slope() {
        let flat = flat_ground(8, 2.5);
        let (height, normal) = flat.under(3.25, 4.75);
        assert_eq!(height, 2.5);
        assert_eq!(normal, [0.0, 1.0, 0.0]);

        let ramp = Heightfield {
            texels: [8, 8],
            metres_per_texel: 1.0,
            origin: [0.0, 0.0],
            heights: (0..64).map(|at| (at % 8) as f32).collect(),
        };
        let (height, normal) = ramp.under(3.5, 2.0);
        assert!(
            (height - 3.5).abs() < 1e-6,
            "the ramp read {height} at x=3.5"
        );
        let root_half = core::f32::consts::FRAC_1_SQRT_2;
        assert!((normal[0] + root_half).abs() < 1e-6, "{normal:?}");
        assert!((normal[1] - root_half).abs() < 1e-6, "{normal:?}");
        assert!(normal[2].abs() < 1e-6, "{normal:?}");
    }

    /// **The cover map is nearest**, so a position anywhere inside a texel's
    /// square reads that texel and nothing of its neighbours.
    #[test]
    fn the_cover_map_reads_one_texel() {
        let mut cover = flat_cover(4, 0);
        cover.cover[2 + 4] = [200, 1];
        for (x, z) in [(2.0, 1.0), (1.51, 0.51), (2.49, 1.49)] {
            assert_eq!(cover.under(x, z), [200, 1], "at ({x}, {z})");
        }
        assert_eq!(cover.under(1.49, 1.0), [0, 0]);
        assert_eq!(cover.under(2.0, 1.51), [0, 0]);
        // And the edges clamp rather than wrapping or indexing out.
        assert_eq!(cover.under(-100.0, -100.0), [0, 0]);
        assert_eq!(cover.under(100.0, 100.0), [0, 0]);
    }

    /// The two uploads are the byte layouts the seam takes: four bytes a texel
    /// for the heights and four for the cover.
    #[test]
    fn the_maps_upload_as_the_formats_they_are_bound_as() {
        let ground = flat_ground(2, 1.5);
        let bytes = ground.to_bytes();
        assert_eq!(bytes.len(), 4 * 4);
        assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().expect("4")), 1.5);
        let cover = flat_cover(2, 77);
        let bytes = cover.to_bytes();
        assert_eq!(bytes.len(), 4 * 4);
        assert_eq!(&bytes[0..4], &[77, 0, 0, 0]);
    }

    /// The blade row reaches the shader's layout with its colours split and its
    /// four sizes in the order the shader reads them.
    #[test]
    fn a_blade_row_is_the_shaders_row() {
        let row = one_blade()[0].row();
        assert_eq!(row.root_color, [0.05, 0.12, 0.03, 0.0]);
        assert_eq!(row.tip_color, [0.35, 0.55, 0.15, 0.0]);
        assert_eq!(row.size, [0.4, 0.03, 0.4, 0.3]);
    }

    /// The buffer sizes are what the shaders index, so a field that fits its
    /// slots fits its buffer.
    #[test]
    fn the_buffers_are_the_size_the_shaders_index() {
        let field = field();
        assert_eq!(
            field.instance_bytes(),
            u64::from(field.slots()) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64
        );
        assert_eq!(field.blade_bytes(), BLADE_STRIDE as u64);
    }
}
