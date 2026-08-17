//! The two material rows a greybox scene shades through, and the base-colour
//! page the second one samples.
//!
//! # Two rows, one page
//!
//! * [`greybox_material`] is the plain grey every primitive shows by default —
//!   [`GpuMaterial::UNTINTED`], which samples the page's white layer 0 and so
//!   shades a surface in its own vertex albedo, [`crate::GREYBOX_ALBEDO`].
//! * [`grid_material`] samples [`GRID_LAYER`] instead: a metric grid a dev turns
//!   on to read sizes straight off a surface.
//!
//! Both are laid out for [`SceneDesc`](crcbl_render::scene::SceneDesc)'s rule
//! that **row 0 is what an unassigned instance shades through** — the grey row
//! is first, so a primitive placed without a named material is grey rather than
//! gridded.
//!
//! # The grid is a one-metre reference tile
//!
//! The engine's base-colour page samples with `ClampToEdge` and no mip chain, so
//! a UV outside `0..1` does not repeat — the grid cannot tile across a face by
//! itself. It is therefore authored as **one `0..1` tile** standing for a one
//! metre square, divided into [`GRID_CELLS`] cells of [`GRID_CELL_M`] each. On
//! the default one-metre primitives a face reads true to scale; on a larger face
//! the single tile stretches to cover it, which is the honest limit of a clamped
//! page and is documented rather than hidden. A truly metric grid at any size
//! wants a `Repeat` sampler the page does not offer today.

use std::borrow::Cow;

use crcbl_render::scene::PageDesc;
use crcbl_shaders::mesh::GpuMaterial;

/// The grid layer's index in the page — the number [`grid_material`]'s
/// `base_color_texture` carries. Layer 0 is the white untextured layer
/// [`PageDesc::opaque_white`] owns, so the grid is the first layer past it.
pub const GRID_LAYER: u32 = 1;

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

/// The neutral greybox material: [`GpuMaterial::UNTINTED`], a mid-grey dielectric
/// on the page's white layer. Row 0 of [`crate::scene3d`], so it is what an
/// instance placed without a named material shades through.
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

/// The base-colour page a greybox scene uploads: the white untextured layer 0,
/// then the grid as [`GRID_LAYER`].
#[must_use]
pub fn grid_page() -> PageDesc<'static> {
    let mut page = PageDesc::opaque_white(GRID_EXTENT);
    let layer = page.push_layer(Cow::Owned(grid_texels()));
    debug_assert_eq!(
        layer, GRID_LAYER,
        "the grid is the one layer past the white one"
    );
    page
}
