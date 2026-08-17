//! Greybox prototyping primitives, sized in real-world metres.
//!
//! A blockout kit for the 3D half of a game: cubes, walls, doorways, ramps,
//! stairs, columns and the round primitives, plus a capsule the size of a person
//! to check a space against. Every generator returns a
//! [`Geometry::Flat`](crcbl_render::scene::Geometry::Flat) — procedural vertex
//! bytes, indices and meshlet clusters — so the pack is host data with no device
//! in sight and builds for every target the engine has, the browser included.
//!
//! # Metres, because the world is metric
//!
//! The engine's world unit is one metre: physics is SI and the unit cube spans
//! `[-0.5, 0.5]`. So a number passed to [`cube`], [`wall`] or [`capsule`] is a
//! measurement in the game, and `capsule(0.25, 1.8, …)` is a 1.8-metre-tall
//! human reference with no conversion anywhere. The crate's tests assert those
//! sizes back out of the geometry, which is the whole promise of the pack.
//!
//! # Two ways in
//!
//! * **One primitive at a time** — call a generator and drop its geometry into
//!   your own [`SceneDesc`](crcbl_render::scene::SceneDesc).
//! * **The whole kit at once** — [`scene3d`] builds a ready
//!   [`SceneDesc`](crcbl_render::scene::SceneDesc) of every primitive with the
//!   grey and grid materials, so a game does
//!   `ForwardRenderer::with_scene(&scene3d())` and then places instances by the
//!   `GREYBOX_*` mesh constants.
//!
//! # Materials
//!
//! [`greybox_material`] is the plain grey a blockout wears; [`grid_material`]
//! shows the metric grid of [`grid_page`], a quarter-metre ruler on the surface.
//! See the [`material`] module for the grid's one-metre-tile convention.

mod build;
pub mod material;
pub mod primitives;
pub mod scene;

pub use build::GREYBOX_ALBEDO;
pub use material::{
    GRID_CELL_M, GRID_CELLS, GRID_EXTENT, GRID_LAYER, greybox_material, grid_material, grid_page,
    grid_texels,
};
pub use primitives::{
    RAMP_ANGLES_DEG, RAMP_WIDTH_M, UNIT_CUBE_M, capsule, column, cube, cylinder, doorway, platform,
    quad, ramp, sphere, stairs, unit_cube, wall,
};
pub use scene::{
    GREYBOX_CAPSULE, GREYBOX_COLUMN, GREYBOX_CUBE, GREYBOX_CYLINDER, GREYBOX_DOORWAY, GREYBOX_GREY,
    GREYBOX_GRID, GREYBOX_MESH_COUNT, GREYBOX_PLATFORM, GREYBOX_QUAD, GREYBOX_RAMP, GREYBOX_SPHERE,
    GREYBOX_STAIRS, GREYBOX_WALL, scene3d,
};
