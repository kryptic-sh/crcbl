//! The two material rows a greybox scene shades through, and the base-colour
//! page the second one samples.
//!
//! # Two rows, one page
//!
//! * [`greybox_material`] is the plain grey every primitive shows by default —
//!   [`GpuMaterial::UNTINTED`], which names no page at all and so shades a
//!   surface in its own vertex albedo, [`crate::GREYBOX_ALBEDO`].
//! * [`grid_material`] samples [`GRID_LAYER`] instead: a metric grid a dev turns
//!   on to read sizes straight off a surface.
//!
//! Both are laid out for [`SceneDesc`](crcbl_render::scene::SceneDesc)'s rule
//! that **row 0 is what an unassigned instance shades through** — the grey row
//! is first, so a primitive placed without a named material is grey rather than
//! gridded.
//!
//! # Two grids, and the difference is the tiling mode
//!
//! [`grid_material`] is the original **authored-UV** grid: one `0..1` tile
//! standing for a one-metre square, divided into [`GRID_CELLS`] cells of
//! [`GRID_CELL_M`] each. A mesh authors its UV in `0..1`, so on a one-metre face
//! the tile reads true to scale, and on a larger face the single tile stretches
//! to cover it — the honest limit of an authored UV.
//!
//! [`greybox_color_material`] instead uses [`GpuMaterial::TILING_PHYSICAL`]: the
//! shader derives the sampling UV from the surface's world-space extent, so one
//! [`greybox_page`] tile measures [`GREYBOX_TILE_M`] of surface however large the
//! face is — a 2 m face shows a 2×2 grid of the tile where a 1 m face shows one.
//! That is a **truly metric** grid at any size, and it is what the base-colour
//! sampler being `Repeat` (rather than the `ClampToEdge` it once was) buys: a
//! physical UV runs past `0..1`, and only a wrapping sampler tiles the page
//! across it. The seven [`GreyboxColor`] tiles are that grid in the blockout
//! palette — grey, red, green, blue, orange, brown and black — each a
//! [`GREYBOX_TILE_EXTENT`]² image with a [`GREYBOX_TILE_CELL_M`] sub-grid.

use std::borrow::Cow;

use crcbl_render::scene::{PageDesc, PageKind};
use crcbl_shaders::mesh::GpuMaterial;

/// The grid layer's index in the page — the number [`grid_material`]'s
/// `base_color_texture` carries. The only layer [`grid_page`] holds, and layer 0
/// is an ordinary layer: nothing is burned ahead of it.
pub const GRID_LAYER: u32 = 0;

/// The grid page's extent, in texels a side.
///
/// 32, well past the 2×2 the engine's demo checker uses: enough resolution to
/// draw thin grid lines rather than a coarse checker, so the divisions read as a
/// ruler rather than a chessboard.
pub const GRID_EXTENT: u32 = 32;

/// How many cells the grid tile is divided into on each axis.
pub const GRID_CELLS: u32 = 4;

/// The metric one grid cell stands for on a one-metre reference face, in metres.
///
/// A one-metre tile split into [`GRID_CELLS`] cells, so each cell is a quarter
/// of a metre — a division a dev can count sizes off directly.
pub const GRID_CELL_M: f32 = 1.0 / GRID_CELLS as f32;

/// The sRGB grey of the grid's field, the lit area inside a cell.
const GRID_FIELD: [u8; 4] = [0xB0, 0xB0, 0xB0, 0xFF];

/// The sRGB grey of the grid's lines, drawn along every cell boundary.
const GRID_LINE: [u8; 4] = [0x40, 0x40, 0x40, 0xFF];

/// The neutral greybox material: [`GpuMaterial::UNTINTED`], a mid-grey
/// dielectric naming no page at all. Row 0 of [`crate::scene3d`], so it is what
/// an instance placed without a named material shades through.
#[must_use]
pub fn greybox_material() -> GpuMaterial {
    GpuMaterial::UNTINTED
}

/// The scale-grid material: [`greybox_material`] with its base-colour texture
/// pointed at [`GRID_LAYER`], so the surface shows the metric grid of
/// [`grid_page`].
#[must_use]
pub fn grid_material() -> GpuMaterial {
    GpuMaterial {
        base_color_texture: GRID_LAYER,
        ..GpuMaterial::UNTINTED
    }
}

/// The grid layer's texels: a [`GRID_EXTENT`]² RGBA8 sRGB image with a dark line
/// along every cell boundary over a mid-grey field — [`GRID_CELLS`] cells a
/// side, each [`GRID_CELL_M`] metres on a one-metre face.
#[must_use]
pub fn grid_texels() -> Vec<u8> {
    let step = GRID_EXTENT / GRID_CELLS;
    let mut texels = Vec::with_capacity((GRID_EXTENT * GRID_EXTENT) as usize * 4);
    for y in 0..GRID_EXTENT {
        for x in 0..GRID_EXTENT {
            let on_line = x % step == 0 || y % step == 0;
            texels.extend_from_slice(if on_line { &GRID_LINE } else { &GRID_FIELD });
        }
    }
    texels
}

/// The base-colour page a greybox scene uploads: the grid as [`GRID_LAYER`],
/// and nothing else.
#[must_use]
pub fn grid_page() -> PageDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::BaseColor, GRID_EXTENT);
    let layer = page.push_layer(PageKind::BaseColor, Cow::Owned(grid_texels()));
    debug_assert_eq!(layer, GRID_LAYER, "the grid is the page's only layer");
    page
}

// ---------------------------------------------------------------------------
// The physical-tiling colour tiles
// ---------------------------------------------------------------------------

/// The side of one greybox colour tile, in texels.
///
/// 1024, so a physically-tiled face carries a sharp ruler even where one tile
/// covers many metres of surface — the grid lines below are drawn several texels
/// thick, which a coarser page could not spare.
pub const GREYBOX_TILE_EXTENT: u32 = 1024;

/// How many world-space metres one greybox colour tile spans — the
/// [`tile_metres`](GpuMaterial::tile_metres) every [`greybox_color_material`]
/// carries.
///
/// One metre, so a texture cell measures one metre of surface: the whole point
/// of physical tiling, and what makes a 2 m face show a 2×2 grid of the tile.
pub const GREYBOX_TILE_M: f32 = 1.0;

/// How many cells the tile is divided into on each axis.
pub const GREYBOX_TILE_CELLS: u32 = 4;

/// The metric one cell of a greybox tile stands for, in metres: a quarter of a
/// metre, [`GREYBOX_TILE_M`] split [`GREYBOX_TILE_CELLS`] ways.
pub const GREYBOX_TILE_CELL_M: f32 = GREYBOX_TILE_M / GREYBOX_TILE_CELLS as f32;

/// How many texels thick a tile's grid lines and border are.
///
/// Eight, so the ruler stays visible when physical tiling spreads one tile
/// across a large face and the page is sampled far below one texel per pixel.
const GREYBOX_TILE_LINE_TEXELS: u32 = 8;

/// One of the seven greybox tile colours a blockout is painted in.
///
/// Each is a [`GREYBOX_TILE_EXTENT`]² grid tile — a coloured field ruled with a
/// [`GREYBOX_TILE_CELL_M`] sub-grid — sampled through
/// [`GpuMaterial::TILING_PHYSICAL`], so the grid reads as a measured one-metre
/// ruler across a surface of any size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreyboxColor {
    /// Neutral grey, the default blockout shade.
    Grey,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Blue.
    Blue,
    /// Orange.
    Orange,
    /// Brown.
    Brown,
    /// Black.
    Black,
}

impl GreyboxColor {
    /// Every colour, in the order [`greybox_page`] lays them out — so `ALL[i]`
    /// occupies page layer `i`.
    pub const ALL: [GreyboxColor; 7] = [
        Self::Grey,
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Orange,
        Self::Brown,
        Self::Black,
    ];

    /// Which layer of [`greybox_page`] this colour's tile occupies — the number
    /// a [`greybox_color_material`] row's
    /// [`base_color_texture`](GpuMaterial::base_color_texture) carries.
    ///
    /// The colours are the whole page, in [`ALL`](Self::ALL) order, from layer
    /// zero: nothing burns a layer ahead of them.
    #[must_use]
    pub const fn layer(self) -> u32 {
        match self {
            Self::Grey => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Blue => 3,
            Self::Orange => 4,
            Self::Brown => 5,
            Self::Black => 6,
        }
    }

    /// A short human-readable name, used in scene labels and test failures.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Grey => "grey",
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Orange => "orange",
            Self::Brown => "brown",
            Self::Black => "black",
        }
    }

    /// The sRGB field colour of the tile's cells — the lit area inside the grid.
    const fn field(self) -> [u8; 4] {
        match self {
            Self::Grey => [0xB0, 0xB0, 0xB0, 0xFF],
            Self::Red => [0xC2, 0x3B, 0x38, 0xFF],
            Self::Green => [0x3C, 0xA0, 0x4A, 0xFF],
            Self::Blue => [0x3B, 0x5B, 0xC8, 0xFF],
            Self::Orange => [0xE0, 0x86, 0x24, 0xFF],
            Self::Brown => [0x7A, 0x4F, 0x2C, 0xFF],
            Self::Black => [0x1E, 0x1E, 0x1E, 0xFF],
        }
    }

    /// The sRGB colour of the tile's grid lines and border.
    ///
    /// A dark line over the coloured field, except on [`Black`](Self::Black),
    /// whose field is already dark — there the line is *lighter* so the ruler
    /// stays legible rather than vanishing into the field.
    const fn line(self) -> [u8; 4] {
        match self {
            Self::Black => [0x5A, 0x5A, 0x5A, 0xFF],
            _ => [0x2A, 0x2A, 0x2A, 0xFF],
        }
    }
}

/// The texels of one [`GreyboxColor`] tile: a [`GREYBOX_TILE_EXTENT`]² RGBA8 sRGB
/// image, the colour's field ruled with a [`GREYBOX_TILE_CELLS`]-cell sub-grid
/// and a border, both `GREYBOX_TILE_LINE_TEXELS` texels thick.
#[must_use]
pub fn greybox_color_texels(color: GreyboxColor) -> Vec<u8> {
    let extent = GREYBOX_TILE_EXTENT;
    let cell = extent / GREYBOX_TILE_CELLS;
    let thickness = GREYBOX_TILE_LINE_TEXELS;
    let field = color.field();
    let line = color.line();
    let mut texels = Vec::with_capacity((extent * extent) as usize * 4);
    for y in 0..extent {
        for x in 0..extent {
            // A line at the near edge of every cell — which covers the tile's
            // left and top border — plus the far edge, so the outer border is
            // closed on all four sides.
            let on_line = x % cell < thickness
                || y % cell < thickness
                || x >= extent - thickness
                || y >= extent - thickness;
            texels.extend_from_slice(if on_line { &line } else { &field });
        }
    }
    texels
}

/// The base-colour page the greybox colour tiles upload: the seven
/// [`GreyboxColor`] tiles in [`GreyboxColor::ALL`] order and nothing else, so
/// [`GreyboxColor::layer`] names each one.
#[must_use]
pub fn greybox_page() -> PageDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::BaseColor, GREYBOX_TILE_EXTENT);
    for color in GreyboxColor::ALL {
        let layer = page.push_layer(PageKind::BaseColor, Cow::Owned(greybox_color_texels(color)));
        debug_assert_eq!(
            layer,
            color.layer(),
            "the colour tiles are the page, in ALL order"
        );
    }
    page
}

/// A physical-tiling material for one [`GreyboxColor`]: [`greybox_material`]
/// pointed at the colour's [`greybox_page`] layer and switched to
/// [`GpuMaterial::TILING_PHYSICAL`] at [`GREYBOX_TILE_M`] per tile.
///
/// So a surface it shades shows the colour's one-metre ruler true to scale at any
/// size — one cell of the grid per [`GREYBOX_TILE_CELL_M`] of surface, one tile
/// per metre.
#[must_use]
pub fn greybox_color_material(color: GreyboxColor) -> GpuMaterial {
    GpuMaterial {
        base_color_texture: color.layer(),
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..GpuMaterial::UNTINTED
    }
}
