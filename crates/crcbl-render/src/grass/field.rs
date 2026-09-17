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
//! [`super::card`], and a shell's strand is arithmetic in the shader. A field
//! that named an image would need an asset seam, a page and a cook of its own;
//! `docs/backlog.md` carries the authored page.
//!
//! # A look and a style are a blade row's, the shell stack is the field's
//!
//! Decision 3's three looks are one blade description drawn three ways, so
//! [`BladeType::look`] is a field of the row and switching it changes nothing
//! else — not which cells carry a blade, not their size, not the wind they lean
//! in. Decision 4's levers are [`BladeType::style`] for the same reason: "each
//! is a field of the blade type, so any of the three looks can be stylised".
//!
//! [`Shells`] is the field's rather than a row's, because every shell row of a
//! field stands in one stack: its count is what the shells' overdraw is
//! proportional to, and a stack cannot be sixteen layers under one row and
//! thirty-two under the next. [`BladeLod`] is the field's for the same reason:
//! where the mesh blades change level is a distance from one camera, and two
//! rows switching at two distances would pop at both.
//!
//! [`BladeType::clumping`] is a row's and is **placement**, not look: decision
//! 2's clumps move a blade's facing and height, and they move them for every
//! look alike, so a row's clumping survives a look switch unchanged.

use crcbl_shaders::grass::{
    BLADE_STRIDE, DEFAULT_SHELLS, GrassBlade, INSTANCE_STRIDE, LOOK_BLADES, LOOK_CARDS,
    LOOK_SHELLS, MAX_SHELLS, NORMAL_GROUND, NORMAL_UP,
};

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
/// [`GrassField::instance_bytes`], which at this cap stays under WebGPU's
/// 128 MiB default maximum storage binding size — decision 1's limit on a ring,
/// and `the_widest_field_fits_a_browsers_storage_binding` is where it is held.
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
    /// A blade row whose size, spread or lever is not a number a blade can be
    /// drawn with.
    #[error("blade row {row}'s {what} of {value} is not one a blade is drawn with")]
    Blade {
        /// Which row was refused.
        row: usize,
        /// Which field of it.
        what: &'static str,
        /// What was offered.
        value: f32,
    },
    /// A shell stack of no layers, or of more than [`MAX_SHELLS`].
    #[error("a stack of {count} shells is not one a field holds; 1 to {MAX_SHELLS} are")]
    Shells {
        /// Layers that were asked for.
        count: u32,
    },
    /// A level-of-detail switch that is not a positive distance, or a band
    /// before it that is negative or longer than the distance.
    #[error(
        "a blade level switch at {distance} m over a band of {band} m is not one a field draws"
    )]
    BladeLod {
        /// The switch distance that was offered.
        distance: f32,
        /// The band that was offered.
        band: f32,
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

/// Which geometry a blade row is drawn with — decision 3's looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BladeLook {
    /// Two crossed cards per blade, cut out against the cooked coverage chain:
    /// the photoreal look, and the cheapest.
    #[default]
    Cards,
    /// Acerola's shell texturing: the field's [`Shells`] stacked over the
    /// ground, each cutting out a cone about every blade's root, with fins
    /// where the stack is seen at a grazing angle.
    Shells,
    /// Ghost of Tsushima's mesh blades: a cubic Bézier per blade built from its
    /// vertex index, fifteen vertices near the camera and seven past the
    /// field's [`BladeLod`] — the realistic look.
    Blades,
}

/// Which normal a blade row shades by — decision 4's last lever.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BladeNormal {
    /// The ground's under the blade's root, so a field shades as the surface it
    /// grows on.
    #[default]
    Ground,
    /// Straight up whatever the ground does, so a hillside shades as one flat
    /// colour — the stylised half of the lever.
    Up,
}

/// Decision 4's shading levers: "a base-to-tip gradient along the blade, an
/// ambient-occlusion colour at the root, an additive tip colour (Acerola),
/// clump-coloured patches (Ghost of Tsushima), and normals taken from the ground
/// or straight up".
///
/// The gradient is [`BladeType::root_color`] and [`BladeType::tip_color`], which
/// every row has always carried; the rest are here. **[`BladeStyle::PLAIN`]
/// pulls none of them**, and a row carrying it draws exactly the picture it drew
/// before they existed — `grass.slang`'s `grass_style` returns its colour
/// unchanged for it, not merely close to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeStyle {
    /// The colour a blade darkens toward at its root.
    pub root_occlusion: [f32; 3],
    /// The fraction of a blade's height the occlusion colour reaches up it, in
    /// `0..=1`. Zero is no occlusion at all.
    pub occlusion_reach: f32,
    /// The colour added toward a blade's tip. Black adds nothing.
    pub tip_glow: [f32; 3],
    /// The fraction of a blade's height the tip colour starts at, in `0..1`; it
    /// rises linearly from there to all of itself at the tip.
    pub glow_start: f32,
    /// The colour a patch of blades leans toward.
    pub patch_color: [f32; 3],
    /// The most of [`BladeStyle::patch_color`] a patch takes, in `0..=1`; each
    /// patch takes its own hashed share of it. Zero is no patches.
    ///
    /// **A patch is a clump** — decision 2's Voronoi cells, which every blade
    /// belongs to whatever its row's [`BladeType::clumping`] — so the colour
    /// follows the same structure the facing and the height do.
    pub patch_share: f32,
    /// The normal the row shades by.
    pub normal: BladeNormal,
}

impl BladeStyle {
    /// No lever pulled: no occlusion, no glow, no patches, the ground's normal.
    pub const PLAIN: Self = Self {
        root_occlusion: [0.0; 3],
        occlusion_reach: 0.0,
        tip_glow: [0.0; 3],
        glow_start: 0.0,
        patch_color: [0.0; 3],
        patch_share: 0.0,
        normal: BladeNormal::Ground,
    };
}

impl Default for BladeStyle {
    fn default() -> Self {
        Self::PLAIN
    }
}

/// A mesh blade's shape — decision 3's "tilt and facing set the tip, bend sets
/// the midpoint … normals tilt outward so a flat blade reads as rounded".
///
/// Read by [`BladeLook::Blades`] alone; a card or a shell row carries it
/// unread, so switching a row's look keeps its shape for the day it switches
/// back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeShape {
    /// How far the tip leans out along the blade's face, as the sine of the
    /// lean, in `0..1`. The blade keeps its height as its length.
    pub tilt: f32,
    /// How far the middle of the blade bows back from the straight line between
    /// its root and its tip, as a fraction of its height, in `0..=1`.
    pub bow: f32,
    /// How far the normal tilts out toward each edge, in `0..=1`, so a flat
    /// strip shades as a rounded blade.
    pub rounding: f32,
}

impl BladeShape {
    /// A straight, flat, upright blade.
    pub const STRAIGHT: Self = Self {
        tilt: 0.0,
        bow: 0.0,
        rounding: 0.0,
    };
}

impl Default for BladeShape {
    fn default() -> Self {
        Self::STRAIGHT
    }
}

/// How much a row's blades take from their clump — decision 2's "clumps …
/// driving height, a shared facing and colour".
///
/// **Placement, for every look**: a card, a shell and a mesh blade of one row
/// and one cell stand the same way at the same height. The colour half is
/// [`BladeStyle::patch_share`], which is a lever. [`Clumping::NONE`] leaves every
/// facing and height the bits it was before clumps existed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clumping {
    /// The share of a blade's own facing its clump's shared facing replaces, in
    /// `0..=1`.
    pub facing: f32,
    /// The fraction of a blade's height its clump may take away, in `0..1` —
    /// one hashed share per clump, on top of the blade's own
    /// [`BladeType::height_spread`].
    pub height: f32,
}

impl Clumping {
    /// No clumping: every blade faces and stands by its own hash alone.
    pub const NONE: Self = Self {
        facing: 0.0,
        height: 0.0,
    };
}

impl Default for Clumping {
    fn default() -> Self {
        Self::NONE
    }
}

/// Where a field's mesh blades change level of detail — decision 3's "the low
/// LOD's tile is twice the size with the same blade count, the high LOD drops
/// three blades in four first, and the high LOD morphs toward the low LOD's
/// shape near the switch".
///
/// A blade whose root is nearer the camera than [`BladeLod::distance`] is drawn
/// with fifteen vertices; past it, only the one blade of every two-by-two block
/// of cells the far level keeps is drawn, with seven. Over the
/// [`BladeLod::band`] before the switch the near level narrows the other three
/// away and then morphs onto the far shape, so a blade crossing the switch does
/// not move a pixel. A band of zero is a hard switch, and pops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeLod {
    /// The switch, in metres from the camera.
    pub distance: f32,
    /// The band before it, in metres, `0..=distance`.
    pub band: f32,
}

impl Default for BladeLod {
    /// A switch at twelve metres over a band of four — a starting value rather
    /// than a measurement, and a field's own choice to make.
    fn default() -> Self {
        Self {
            distance: 12.0,
            band: 4.0,
        }
    }
}

/// A field's shell stack: how many layers, and whether fins stand where the
/// stack is seen edge-on.
///
/// **The overdraw of a shell look is proportional to [`Shells::count`]**, which
/// is decision 3's price and the reason it is a parameter: every layer is a
/// sheet over the whole field, and a fragment of each is shaded wherever the
/// sheet is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shells {
    /// Layers in the stack, `1..=MAX_SHELLS`.
    pub count: u32,
    /// Whether fins stand at silhouettes. Off, a stack seen at a grazing angle
    /// shows the gaps between its layers — which is what a test of the fins
    /// compares against.
    pub fins: bool,
}

impl Default for Shells {
    fn default() -> Self {
        Self {
            count: DEFAULT_SHELLS,
            fins: true,
        }
    }
}

/// One row of the blade table — decision 3's "one blade description", drawn as
/// its [`BladeType::look`] says.
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
    ///
    /// A shell strand's root is this half-width too, bounded by half a
    /// placement cell — see `grass.slang`'s `grass_strand_reach`.
    pub width_spread: f32,
    /// The geometry the row is drawn with.
    pub look: BladeLook,
    /// Decision 4's levers.
    pub style: BladeStyle,
    /// A mesh blade's shape, read only where [`BladeType::look`] is
    /// [`BladeLook::Blades`].
    pub shape: BladeShape,
    /// Decision 2's clumping, for every look.
    pub clumping: Clumping,
}

impl BladeType {
    /// The row as the shader reads it.
    #[must_use]
    pub fn row(&self) -> GrassBlade {
        let with = |[r, g, b]: [f32; 3], a: f32| [r, g, b, a];
        let style = self.style;
        GrassBlade {
            root_color: with(self.root_color, 0.0),
            tip_color: with(self.tip_color, 0.0),
            size: [
                self.height,
                self.half_width,
                self.height_spread,
                self.width_spread,
            ],
            occlusion: with(style.root_occlusion, style.occlusion_reach),
            glow: with(style.tip_glow, style.glow_start),
            patch: with(style.patch_color, style.patch_share),
            shape: [self.shape.tilt, self.shape.bow, self.shape.rounding, 0.0],
            clump: [self.clumping.facing, self.clumping.height, 0.0, 0.0],
            flags: [
                match self.look {
                    BladeLook::Cards => LOOK_CARDS,
                    BladeLook::Shells => LOOK_SHELLS,
                    BladeLook::Blades => LOOK_BLADES,
                },
                match style.normal {
                    BladeNormal::Ground => NORMAL_GROUND,
                    BladeNormal::Up => NORMAL_UP,
                },
                0,
                0,
            ],
        }
    }

    /// The first of this row's numbers a shader could not draw with, as the
    /// name and value [`GrassError::Blade`] reports.
    fn refusal(&self) -> Option<(&'static str, f32)> {
        let style = self.style;
        let unit = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        let below_one = |value: f32| value.is_finite() && (0.0..1.0).contains(&value);
        let colour = |value: f32| value.is_finite() && value >= 0.0;
        let checks = [
            (
                "height",
                self.height,
                self.height.is_finite() && self.height > 0.0,
            ),
            (
                "half-width",
                self.half_width,
                self.half_width.is_finite() && self.half_width > 0.0,
            ),
            (
                "height spread",
                self.height_spread,
                below_one(self.height_spread),
            ),
            (
                "width spread",
                self.width_spread,
                below_one(self.width_spread),
            ),
            (
                "occlusion reach",
                style.occlusion_reach,
                unit(style.occlusion_reach),
            ),
            ("glow start", style.glow_start, below_one(style.glow_start)),
            ("patch share", style.patch_share, unit(style.patch_share)),
            ("tilt", self.shape.tilt, below_one(self.shape.tilt)),
            ("bow", self.shape.bow, unit(self.shape.bow)),
            ("rounding", self.shape.rounding, unit(self.shape.rounding)),
            (
                "clump facing",
                self.clumping.facing,
                unit(self.clumping.facing),
            ),
            (
                "clump height",
                self.clumping.height,
                below_one(self.clumping.height),
            ),
        ];
        let colours = [
            ("root occlusion colour", style.root_occlusion),
            ("tip glow", style.tip_glow),
            ("patch colour", style.patch_color),
        ]
        .into_iter()
        .flat_map(|(what, rgb)| rgb.map(|value| (what, value, colour(value))));
        checks
            .into_iter()
            .chain(colours)
            .find(|(_, _, ok)| !ok)
            .map(|(what, value, _)| (what, value))
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
    shells: Shells,
    blade_lod: BladeLod,
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
            if let Some((what, value)) = blade.refusal() {
                return Err(GrassError::Blade { row, what, value });
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
            shells: Shells::default(),
            blade_lod: BladeLod::default(),
        })
    }

    /// This field with its shell stack replaced.
    ///
    /// # Errors
    ///
    /// [`GrassError::Shells`] for a stack of no layers or of more than
    /// [`MAX_SHELLS`], and then the field is not built.
    pub fn with_shells(mut self, shells: Shells) -> Result<Self, GrassError> {
        if !(1..=MAX_SHELLS).contains(&shells.count) {
            return Err(GrassError::Shells {
                count: shells.count,
            });
        }
        self.shells = shells;
        Ok(self)
    }

    /// This field with its mesh blades' level switch replaced.
    ///
    /// # Errors
    ///
    /// [`GrassError::BladeLod`] for a switch that is not a positive finite
    /// distance, or a band that is negative, not finite or longer than the
    /// distance — and then the field is not built.
    pub fn with_blade_lod(mut self, lod: BladeLod) -> Result<Self, GrassError> {
        let BladeLod { distance, band } = lod;
        if !(distance.is_finite()
            && distance > 0.0
            && band.is_finite()
            && (0.0..=distance).contains(&band))
        {
            return Err(GrassError::BladeLod { distance, band });
        }
        self.blade_lod = lod;
        Ok(self)
    }

    /// Where the field's mesh blades change level of detail. Read by the draw
    /// only where a row's look is [`BladeLook::Blades`].
    #[must_use]
    pub const fn blade_lod(&self) -> BladeLod {
        self.blade_lod
    }

    /// Whether any row of the blade table is drawn as mesh blades — which is
    /// whether a frame records the two blade draws at all.
    #[must_use]
    pub fn draws_blades(&self) -> bool {
        self.blades
            .iter()
            .any(|blade| blade.look == BladeLook::Blades)
    }

    /// The field's shell stack. Read by the draw only where a row's look is
    /// [`BladeLook::Shells`].
    #[must_use]
    pub const fn shells(&self) -> Shells {
        self.shells
    }

    /// Whether any row of the blade table is drawn as shells — which is whether
    /// a frame records the shell and fin draws at all.
    #[must_use]
    pub fn draws_shells(&self) -> bool {
        self.blades
            .iter()
            .any(|blade| blade.look == BladeLook::Shells)
    }

    /// How tall the shell stack stands, in metres: the tallest shell row's
    /// height, which no blade of any row exceeds. Zero where no row is shells.
    #[must_use]
    pub fn shell_stack(&self) -> f32 {
        self.blades
            .iter()
            .filter(|blade| blade.look == BladeLook::Shells)
            .fold(0.0, |tallest, blade| tallest.max(blade.height))
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

    /// Bytes the cells buffer needs for this field: one row per cell of every
    /// tile.
    #[must_use]
    pub fn cell_bytes(&self) -> u64 {
        u64::from(self.slots()) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64
    }

    /// Bytes the instance buffer needs for this field: two regions of
    /// [`GrassField::cell_bytes`] each — the cards and the far mesh blades in
    /// the first, the near mesh blades in the second.
    #[must_use]
    pub fn instance_bytes(&self) -> u64 {
        2 * self.cell_bytes()
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
            look: BladeLook::Cards,
            style: BladeStyle::PLAIN,
            shape: BladeShape::STRAIGHT,
            clumping: Clumping::NONE,
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
        let widest = GrassField::new(
            [MAX_TILES, 1],
            8.0,
            [0.0, 0.0],
            100.0,
            flat_ground(8, 0.0),
            flat_cover(8, 8),
            one_blade(),
        )
        .expect("the widest field is a field")
        .instance_bytes();
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
        // A plain card row pulls no lever: every reach, start and share is the
        // zero the shader returns its colour unchanged for.
        assert_eq!(row.occlusion, [0.0; 4]);
        assert_eq!(row.glow, [0.0; 4]);
        assert_eq!(row.patch, [0.0; 4]);
        assert_eq!(row.flags, [LOOK_CARDS, NORMAL_GROUND, 0, 0]);

        let styled = BladeType {
            look: BladeLook::Shells,
            style: BladeStyle {
                root_occlusion: [0.1, 0.2, 0.3],
                occlusion_reach: 0.4,
                tip_glow: [0.5, 0.6, 0.7],
                glow_start: 0.8,
                patch_color: [0.9, 1.0, 1.1],
                patch_share: 0.25,
                normal: BladeNormal::Up,
            },
            ..one_blade()[0]
        }
        .row();
        assert_eq!(styled.occlusion, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(styled.glow, [0.5, 0.6, 0.7, 0.8]);
        assert_eq!(styled.patch, [0.9, 1.0, 1.1, 0.25]);
        assert_eq!(styled.flags, [LOOK_SHELLS, NORMAL_UP, 0, 0]);
        // A plain row's shape and clumping are all zero, which is what the
        // shaders read as a straight blade and no clump.
        assert_eq!(row.shape, [0.0; 4]);
        assert_eq!(row.clump, [0.0; 4]);

        let blade = BladeType {
            look: BladeLook::Blades,
            shape: BladeShape {
                tilt: 0.25,
                bow: 0.5,
                rounding: 0.75,
            },
            clumping: Clumping {
                facing: 0.125,
                height: 0.375,
            },
            ..one_blade()[0]
        }
        .row();
        assert_eq!(blade.shape, [0.25, 0.5, 0.75, 0.0]);
        assert_eq!(blade.clump, [0.125, 0.375, 0.0, 0.0]);
        assert_eq!(blade.flags, [LOOK_BLADES, NORMAL_GROUND, 0, 0]);
    }

    /// **Every lever a shader divides by or blends with is refused out of
    /// range**, and the shell stack's count is held to the block it fills.
    #[test]
    fn a_field_refuses_levers_and_stacks_a_shader_could_not_draw() {
        let with_style = |style: BladeStyle| {
            GrassField::new(
                [1, 1],
                8.0,
                [0.0, 0.0],
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                vec![BladeType {
                    style,
                    ..one_blade()[0]
                }],
            )
        };
        for (style, what) in [
            (
                BladeStyle {
                    occlusion_reach: 1.5,
                    ..BladeStyle::PLAIN
                },
                "occlusion reach",
            ),
            (
                BladeStyle {
                    glow_start: 1.0,
                    ..BladeStyle::PLAIN
                },
                "glow start",
            ),
            (
                BladeStyle {
                    patch_share: -0.1,
                    ..BladeStyle::PLAIN
                },
                "patch share",
            ),
            (
                BladeStyle {
                    tip_glow: [0.0, f32::NAN, 0.0],
                    ..BladeStyle::PLAIN
                },
                "tip glow",
            ),
        ] {
            assert!(
                matches!(with_style(style), Err(GrassError::Blade { row: 0, what: refused, .. }) if refused == what),
                "{what} was not refused"
            );
        }
        let field = with_style(BladeStyle::PLAIN).expect("a plain row is a real row");
        assert_eq!(field.shells(), Shells::default());
        for count in [0, MAX_SHELLS + 1] {
            assert_eq!(
                field.clone().with_shells(Shells { count, fins: true }),
                Err(GrassError::Shells { count })
            );
        }
        let stacked = field
            .clone()
            .with_shells(Shells {
                count: MAX_SHELLS,
                fins: false,
            })
            .expect("the widest stack is one the block holds");
        assert_eq!(stacked.shells().count, MAX_SHELLS);

        let with_shape = |shape: BladeShape, clumping: Clumping| {
            GrassField::new(
                [1, 1],
                8.0,
                [0.0, 0.0],
                10.0,
                flat_ground(8, 0.0),
                flat_cover(8, 8),
                vec![BladeType {
                    shape,
                    clumping,
                    ..one_blade()[0]
                }],
            )
        };
        for (shape, clumping, what) in [
            (
                BladeShape {
                    tilt: 1.0,
                    ..BladeShape::STRAIGHT
                },
                Clumping::NONE,
                "tilt",
            ),
            (
                BladeShape {
                    bow: f32::INFINITY,
                    ..BladeShape::STRAIGHT
                },
                Clumping::NONE,
                "bow",
            ),
            (
                BladeShape {
                    rounding: -0.5,
                    ..BladeShape::STRAIGHT
                },
                Clumping::NONE,
                "rounding",
            ),
            (
                BladeShape::STRAIGHT,
                Clumping {
                    facing: 1.25,
                    ..Clumping::NONE
                },
                "clump facing",
            ),
            (
                BladeShape::STRAIGHT,
                Clumping {
                    height: 1.0,
                    ..Clumping::NONE
                },
                "clump height",
            ),
        ] {
            assert!(
                matches!(with_shape(shape, clumping), Err(GrassError::Blade { row: 0, what: refused, .. }) if refused == what),
                "{what} was not refused"
            );
        }

        assert_eq!(field.blade_lod(), BladeLod::default());
        for (distance, band) in [
            (0.0, 0.0),
            (f32::NAN, 1.0),
            (10.0, -1.0),
            (10.0, 10.5),
            (10.0, f32::INFINITY),
        ] {
            assert!(
                matches!(
                    field.clone().with_blade_lod(BladeLod { distance, band }),
                    Err(GrassError::BladeLod { .. })
                ),
                "a switch at {distance} m over {band} m was not refused"
            );
        }
        let hard = field
            .with_blade_lod(BladeLod {
                distance: 6.0,
                band: 0.0,
            })
            .expect("a hard switch is a switch");
        assert_eq!(hard.blade_lod().band, 0.0);
    }

    /// **The shell stack is the tallest shell row**, and a field with none has
    /// none — so a card field records no shell draw.
    #[test]
    fn the_stack_stands_as_tall_as_the_tallest_shell_row() {
        let mut rows = one_blade();
        rows.push(BladeType {
            height: 0.9,
            look: BladeLook::Cards,
            ..rows[0]
        });
        let cards = GrassField::new(
            [1, 1],
            8.0,
            [0.0, 0.0],
            10.0,
            flat_ground(8, 0.0),
            flat_cover(8, 8),
            rows.clone(),
        )
        .expect("a real field");
        assert!(!cards.draws_shells());
        assert_eq!(cards.shell_stack(), 0.0);

        rows[0].look = BladeLook::Shells;
        let mixed = GrassField::new(
            [1, 1],
            8.0,
            [0.0, 0.0],
            10.0,
            flat_ground(8, 0.0),
            flat_cover(8, 8),
            rows,
        )
        .expect("a real field");
        assert!(mixed.draws_shells());
        assert!(!mixed.draws_blades());
        // The taller row is a card row, so the stack is the shell row's height.
        assert_eq!(mixed.shell_stack(), 0.4);
    }

    /// The buffer sizes are what the shaders index, so a field that fits its
    /// slots fits its buffer.
    #[test]
    fn the_buffers_are_the_size_the_shaders_index() {
        let field = field();
        assert_eq!(
            field.cell_bytes(),
            u64::from(field.slots()) * u64::from(SLOT_CAPACITY) * INSTANCE_STRIDE as u64
        );
        assert_eq!(field.instance_bytes(), 2 * field.cell_bytes());
        assert_eq!(field.blade_bytes(), BLADE_STRIDE as u64);
    }
}
