//! The per-probe visibility capture: the scene's own static triangles, kept for
//! the GPU pass that draws them from every probe.
//!
//! ```text
//!  SceneDesc::meshes ──▶ Occluders::from_scene ─┐
//!                                               ├─▶ world_triangles ─┐
//!  the instances placed so far ─────────────────┘                    │
//!                                                                    ▼
//!                                          crate::probe_capture ──▶ one
//!                                          Rg32Float layer per probe → 29
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s decision of 2026-08-30. The grid stays
//! what it was — one L1 row per probe, added to the ambient term — and gains the
//! one thing Majercik et al. 2019 identify as what makes a probe grid stop
//! leaking: each probe records how far away the nearest surface is in every
//! direction, and a fragment further from the probe than that in its own
//! direction is behind a wall from it and takes none of its light.
//! [`crcbl_shaders::probe_visibility`] owns the image's layout, the octahedral
//! mapping and the Chebyshev bound, because those are a contract with the
//! shader; [`crate::probe_capture`] owns filling it; and this module owns the
//! geometry it is filled *from*.
//!
//! # It is a capture of geometry, and it is not a bake
//!
//! The distinction the plan insists on, and it is the whole reason this rung is
//! allowed under the no-bake rule: what is stored is **where the walls are**,
//! not what the lights did. Every light in the scene still moves, every one of
//! them still lights the probes through the rows, and nothing here outlives the
//! geometry it was taken from. A reflection capture is the same shape.
//!
//! # The triangles are kept on the host, and drawn on the device
//!
//! [`Occluders`](crate::probe_visibility::Occluders) holds each resident mesh's
//! level-0 triangles and
//! [`world_triangles`](crate::probe_visibility::world_triangles) places them,
//! which is the whole of this module's work: one transform per vertex per
//! capture, and then a soup the capture pass uploads and rasterises six times
//! per probe. The walk is here rather than in
//! the pass because a mesh may be a cluster DAG, whose level-0 triangles have
//! to be gathered before anything can draw them, and because a scene that
//! reserves no probes must not pay even that.
//!
//! **Nothing is retained for a scene with no probes.**
//! [`Occluders::from_scene`](crate::probe_visibility::Occluders::from_scene)
//! keeps the description's positions and indices only when the scene reserves
//! room for a probe, so a caller that never authors one pays neither the memory
//! nor the capture — the same shape as the grid itself adding exactly zero.

use crcbl_shaders::mesh::{self, MeshVertex};
use glam::Mat4;

use crate::scene::{Geometry, SceneDesc};

crcbl_console::convar! {
    /// Weigh each probe by whether it can see the surface it lights: on ships.
    pub static r_probe_visibility: bool = true;
}

/// Whether the shading read should use a captured map at all.
///
/// Off binds the one-texel placeholder instead, which occludes nothing — so the
/// switch is which image is named rather than a branch in the shader, on
/// [`crate::ssao`]'s terms and the ambient-occlusion channel's.
pub(crate) fn enabled() -> bool {
    r_probe_visibility.get_bool()
}

/// One resident mesh's level-0 triangles, in mesh space.
///
/// **Level 0 alone**, whatever the mesh's path: a DAG's coarser levels
/// approximate the same surface, and a visibility map wants the surface the
/// camera will see rather than the one a distant draw would.
#[derive(Clone, Debug, PartialEq)]
struct MeshTriangles {
    /// Vertex positions, in the description's own order.
    positions: Vec<[f32; 3]>,
    /// Three indices into [`MeshTriangles::positions`] per triangle.
    indices: Vec<u32>,
}

/// The triangles of every mesh a scene made resident, by mesh-table id.
///
/// Empty for a scene that reserved no probes — see the [module docs](self) on
/// why that is the point rather than an optimisation.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Occluders {
    /// `(mesh table id, triangles)`, ascending by id so a lookup is a search.
    meshes: Vec<(u32, MeshTriangles)>,
}

impl Occluders {
    /// The description's geometry, kept for the capture — or nothing at all if
    /// the description reserves no probes.
    ///
    /// `ids` is one mesh-table id per [`SceneDesc::meshes`] entry, in the same
    /// order, which is what an instance's `mesh` field resolves to. It is
    /// handed in rather than recomputed because the renderer has already
    /// resolved it and a second copy of that arithmetic is a second thing that
    /// can disagree.
    pub(crate) fn from_scene(scene: &SceneDesc<'_>, ids: &[u32]) -> Self {
        if scene.capacities.probes == 0 {
            return Self::default();
        }
        let mut meshes: Vec<(u32, MeshTriangles)> = Vec::with_capacity(scene.meshes.len());
        for (index, desc) in scene.meshes.iter().enumerate() {
            let Some(&id) = ids.get(index) else {
                continue;
            };
            let triangles = match &desc.geometry {
                Geometry::Flat {
                    vertices, indices, ..
                } => MeshTriangles {
                    positions: vertices
                        .chunks_exact(mesh::VERTEX_STRIDE)
                        .map(|record| {
                            let bytes: &[u8; mesh::VERTEX_STRIDE] =
                                record.try_into().unwrap_or_else(|_| {
                                    unreachable!("chunks_exact yields whole records")
                                });
                            MeshVertex::from_bytes(bytes).position
                        })
                        .collect(),
                    indices: indices.to_vec(),
                },
                Geometry::Dag { dag, .. } => {
                    let Some(level) = dag.levels.first() else {
                        continue;
                    };
                    MeshTriangles {
                        positions: level.positions.clone(),
                        indices: level.indices(),
                    }
                }
            };
            meshes.push((id, triangles));
        }
        meshes.sort_by_key(|(id, _)| *id);
        Self { meshes }
    }

    /// Whether there is anything to capture against.
    pub(crate) fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// The triangles mesh-table id `mesh` draws, if the capture kept them.
    fn triangles(&self, mesh: u32) -> Option<&MeshTriangles> {
        self.meshes
            .binary_search_by_key(&mesh, |(id, _)| *id)
            .ok()
            .map(|at| &self.meshes[at].1)
    }
}

/// One object standing in the scene when the capture runs: the mesh-table id it
/// draws and the transform that puts it in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Occluder {
    /// The mesh-table id, as [`crcbl_shaders::mesh::GpuInstance`] carries it.
    pub(crate) mesh: u32,
    /// Mesh space to world space.
    pub(crate) transform: Mat4,
}

/// Every static triangle in world space, three positions each — the soup
/// [`crate::probe_capture`] uploads and draws.
///
/// Each vertex is transformed once per instance rather than once per triangle
/// corner, because a mesh's index buffer names the same vertex three times on
/// average and the transform is the expensive half.
pub(crate) fn world_triangles(geometry: &Occluders, occluders: &[Occluder]) -> Vec<[[f32; 3]; 3]> {
    let mut soup = Vec::new();
    for placed in occluders {
        let Some(mesh) = geometry.triangles(placed.mesh) else {
            continue;
        };
        let world: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|position| {
                placed
                    .transform
                    .transform_point3(glam::Vec3::from_array(*position))
                    .to_array()
            })
            .collect();
        for triangle in mesh.indices.chunks_exact(3) {
            let corner = |lane: usize| world.get(triangle[lane] as usize).copied();
            // A description whose indices run past its vertices is refused by
            // `MeshClusters::check` before it is made resident, so this is the
            // shape of the check rather than a case with a scene behind it.
            if let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) {
                soup.push([a, b, c]);
            }
        }
    }
    soup
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's twelve triangles, centred on the origin, wound so its
    /// normals face outwards. Placed by a transform, it is a wall or a room.
    fn cube() -> (Vec<[f32; 3]>, Vec<u32>) {
        let positions = vec![
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, // -Z
            4, 5, 6, 4, 6, 7, // +Z
            0, 4, 7, 0, 7, 3, // -X
            1, 2, 6, 1, 6, 5, // +X
            0, 1, 5, 0, 5, 4, // -Y
            3, 7, 6, 3, 6, 2, // +Y
        ];
        (positions, indices)
    }

    /// [`Occluders`] holding one mesh under id 0, with `cube`'s triangles.
    fn one_cube() -> Occluders {
        let (positions, indices) = cube();
        Occluders {
            meshes: vec![(0, MeshTriangles { positions, indices })],
        }
    }

    /// **The placement is where a wall's world position comes from**, and it is
    /// the only arithmetic left on the host: an instance's transform applied to
    /// its mesh's own vertices, once per vertex rather than once per triangle
    /// corner.
    ///
    /// Written out against a scaled, translated cube whose corners are known by
    /// hand, because a transposed matrix or a transform applied in the wrong
    /// order would still produce a plausible-looking box somewhere else.
    #[test]
    fn a_placed_mesh_puts_its_triangles_where_the_transform_says() {
        let placed = [Occluder {
            mesh: 0,
            transform: Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0))
                * Mat4::from_scale(glam::Vec3::new(4.0, 2.0, 6.0)),
        }];
        let soup = world_triangles(&one_cube(), &placed);
        assert_eq!(soup.len(), 12, "a cube is twelve triangles");
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for triangle in &soup {
            for corner in triangle {
                for axis in 0..3 {
                    low[axis] = low[axis].min(corner[axis]);
                    high[axis] = high[axis].max(corner[axis]);
                }
            }
        }
        // The unit cube scaled by (4, 2, 6) and moved ten along `x`.
        for (axis, (want_low, want_high)) in [(8.0, 12.0), (-1.0, 1.0), (-3.0, 3.0)]
            .into_iter()
            .enumerate()
        {
            assert!(
                (low[axis] - want_low).abs() <= 1.0e-5 && (high[axis] - want_high).abs() <= 1.0e-5,
                "axis {axis} spans {} to {}, not {want_low} to {want_high}",
                low[axis],
                high[axis]
            );
        }
    }

    /// An instance naming a mesh the capture did not keep places nothing, and
    /// does not take the meshes that were kept with it.
    #[test]
    fn an_unkept_mesh_places_no_triangles() {
        let placed = [
            Occluder {
                mesh: 7,
                transform: Mat4::IDENTITY,
            },
            Occluder {
                mesh: 0,
                transform: Mat4::IDENTITY,
            },
        ];
        assert_eq!(world_triangles(&one_cube(), &placed).len(), 12);
    }

    /// A scene that reserves no probes retains no geometry and places nothing,
    /// which is what keeps the cost of the feature zero for every caller that
    /// does not use it.
    #[test]
    fn a_scene_with_no_probes_keeps_nothing() {
        let scene = crate::scene::demo();
        assert_eq!(scene.capacities.probes, 0);
        let ids: Vec<u32> = (0..scene.meshes.len() as u32).collect();
        let geometry = Occluders::from_scene(&scene, &ids);
        assert!(geometry.is_empty());
        assert!(world_triangles(&geometry, &[]).is_empty());
    }

    /// A scene that does reserve probes keeps every mesh's triangles, under the
    /// table id an instance names.
    #[test]
    fn a_scene_with_probes_keeps_its_triangles() {
        let mut scene = crate::scene::demo();
        scene.capacities.probes = 1;
        let ids: Vec<u32> = (0..scene.meshes.len() as u32).map(|id| id * 3).collect();
        let geometry = Occluders::from_scene(&scene, &ids);
        assert_eq!(geometry.meshes.len(), scene.meshes.len());
        for (index, id) in ids.iter().enumerate() {
            let kept = geometry
                .triangles(*id)
                .unwrap_or_else(|| panic!("mesh {index} was kept under id {id}"));
            assert!(!kept.positions.is_empty());
            assert!(kept.indices.len().is_multiple_of(3));
        }
        assert_eq!(
            geometry.triangles(1),
            None,
            "and nothing under an id no mesh has"
        );
    }
}
