//! The one-call prototyping scene: every primitive made resident, the grey and
//! grid materials, and the grid page — a [`SceneDesc`] a game hands straight to
//! [`ForwardRenderer::with_scene`](crcbl_render::forward::ForwardRenderer::with_scene).
//!
//! ```no_run
//! use crcbl_greybox::{scene3d, GREYBOX_CUBE, GREYBOX_GREY};
//! use crcbl_render::scene::InstanceDesc;
//! use glam::Mat4;
//!
//! // `ForwardRenderer::with_scene(&scene3d())` makes the pack resident; then
//! // place instances by their mesh and material constants.
//! let scene = scene3d();
//! let cube = InstanceDesc {
//!     mesh: GREYBOX_CUBE,
//!     material: GREYBOX_GREY,
//!     transform: Mat4::IDENTITY,
//! };
//! # let _ = (&scene, &cube);
//! ```
//!
//! # The index constants are the API
//!
//! [`SceneDesc::meshes`] is ordered, and an
//! [`InstanceDesc`](crcbl_render::scene::InstanceDesc) names its mesh by
//! that order — so a caller needs a name for each slot rather than a bare
//! integer whose meaning is the list's order. The `GREYBOX_*` mesh constants and
//! the `GREYBOX_GREY`/`GREYBOX_GRID` material constants are those names; keep
//! them and [`scene3d`]'s assembly in step, which
//! [`the_scene_constants_name_their_own_meshes`](../tests) asserts.

use std::borrow::Cow;

use crcbl_render::scene::{Capacities, Geometry, MeshDesc, ProbeGrid, SceneDesc};

use crate::material::{greybox_material, grid_material, grid_page};
use crate::primitives::{
    RAMP_ANGLES_DEG, UNIT_CUBE_M, capsule, column, cube, cylinder, doorway, platform, quad, ramp,
    sphere, stairs, wall,
};

/// The one-metre [`cube`] — [`SceneDesc::meshes`] slot 0.
pub const GREYBOX_CUBE: usize = 0;
/// The one-metre floor [`quad`].
pub const GREYBOX_QUAD: usize = 1;
/// The default [`wall`], 1 m × 2 m × 0.1 m.
pub const GREYBOX_WALL: usize = 2;
/// The default [`doorway`] frame with its 1 m × 2.1 m opening.
pub const GREYBOX_DOORWAY: usize = 3;
/// The 30° [`ramp`].
pub const GREYBOX_RAMP: usize = 4;
/// The default [`stairs`], eight 0.2 m × 0.28 m steps.
pub const GREYBOX_STAIRS: usize = 5;
/// The default [`column()`], 0.3 m square × 3 m.
pub const GREYBOX_COLUMN: usize = 6;
/// The default [`cylinder`], 0.5 m radius × 1 m.
pub const GREYBOX_CYLINDER: usize = 7;
/// The default [`sphere`], 0.5 m radius.
pub const GREYBOX_SPHERE: usize = 8;
/// The scale-figure [`capsule`], 0.5 m diameter × 1.8 m tall.
pub const GREYBOX_CAPSULE: usize = 9;
/// The default [`platform`], 1 m × 1 m × 0.2 m.
pub const GREYBOX_PLATFORM: usize = 10;

/// How many meshes [`scene3d`] makes resident.
pub const GREYBOX_MESH_COUNT: usize = 11;

/// The neutral grey material row — [`SceneDesc::materials`] slot 0, and what an
/// instance placed without a named material shades through.
pub const GREYBOX_GREY: usize = 0;
/// The scale-grid material row.
pub const GREYBOX_GRID: usize = 1;

/// The 30° ramp [`scene3d`] blocks out, from [`RAMP_ANGLES_DEG`]'s middle.
const SCENE_RAMP_ANGLE_DEG: f32 = RAMP_ANGLES_DEG[1];

/// The whole greybox pack as one resident scene: all
/// [`GREYBOX_MESH_COUNT`] primitives at their default metric sizes, the
/// [`GREYBOX_GREY`] and [`GREYBOX_GRID`] material rows, and the grid page.
///
/// Built with [`Capacities::default`], which reserves far more than a blockout
/// needs — a prototype grows instances freely without re-sizing a pool.
///
/// The mesh order is the `GREYBOX_*` constants above, in value order; place any
/// of them with an [`InstanceDesc`](crcbl_render::scene::InstanceDesc) naming
/// its constant and a material row.
#[must_use]
pub fn scene3d() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    SceneDesc {
        meshes: vec![
            mesh("greybox cube", cube(UNIT_CUBE_M)),
            mesh("greybox quad", quad(1.0, 1.0)),
            mesh("greybox wall", wall(1.0, 2.0, 0.1)),
            mesh("greybox doorway", doorway(1.6, 2.5, 0.1, 1.0, 2.1)),
            mesh("greybox ramp", ramp(1.0, SCENE_RAMP_ANGLE_DEG)),
            mesh("greybox stairs", stairs(1.0, 8, 0.2, 0.28)),
            mesh("greybox column", column(0.3, 3.0)),
            mesh("greybox cylinder", cylinder(0.5, 1.0, 24)),
            mesh("greybox sphere", sphere(0.5, 16, 24)),
            mesh("greybox capsule", capsule(0.25, 1.8, 8, 16)),
            mesh("greybox platform", platform(1.0, 1.0, 0.2)),
        ],
        materials: vec![greybox_material(), grid_material()],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}
