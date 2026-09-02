//! The per-probe visibility capture: the octahedral depth and depth² map
//! `mesh.slang` weighs each probe by, filled from the scene's own static
//! triangles.
//!
//! ```text
//!  SceneDesc::meshes ──▶ Occluders::from_scene ─┐
//!                                               ├─▶ capture ──▶ ProbeVisibility
//!  the instances placed so far ─────────────────┘                   │
//!                                                   upload_texture_layers ▼
//!                                             one Rg32Float layer per probe → 29
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
//! shader; this module owns filling it.
//!
//! # It is a capture of geometry, and it is not a bake
//!
//! The distinction the plan insists on, and it is the whole reason this rung is
//! allowed under the no-bake rule: what is stored is **where the walls are**,
//! not what the lights did. Every light in the scene still moves, every one of
//! them still lights the probes through the rows, and nothing here outlives the
//! geometry it was taken from. A reflection capture is the same shape.
//!
//! # Cast on the host, against triangles, and what that costs
//!
//! One ray per texel per probe — [`EXTENT`](crcbl_shaders::probe_visibility::EXTENT)² of them each — against the
//! triangles of every instance placed when [`capture`](crate::probe_visibility::capture) is called, with no
//! acceleration structure between them. That is `probes × EXTENT² × triangles`
//! ray/triangle tests, paid once, and it is the honest shape for the scenes this
//! engine draws today; `docs/backlog.md` carries what it would take to put a
//! `crcbl_phys::Bvh` under it, which is the answer when a scene's triangle count
//! makes the product matter.
//!
//! **Nothing is retained for a scene with no probes.** [`Occluders::from_scene`](crate::probe_visibility::Occluders::from_scene)
//! keeps the description's positions and indices only when the scene reserves
//! room for a probe, so a caller that never authors one pays neither the memory
//! nor the capture — the same shape as the grid itself adding exactly zero.

use crcbl_shaders::mesh::{self, MeshVertex};
use crcbl_shaders::probe::ProbeVolume;
use crcbl_shaders::probe_visibility::{
    EXTENT, FAR, LAYER_BYTES, ProbeVisibility, TEXEL_BYTES, texel_direction,
};
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

/// Every static triangle in world space, three positions each.
///
/// Built once per capture rather than transforming inside the ray loop, because
/// each triangle is then transformed once instead of once per ray.
fn world_triangles(geometry: &Occluders, occluders: &[Occluder]) -> Vec<[[f32; 3]; 3]> {
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

/// How far along `direction` from `origin` the ray meets the triangle, or
/// `None` if it misses.
///
/// **Möller & Trumbore 1997**, *Fast, Minimum Storage Ray/Triangle
/// Intersection*, written out: the barycentric solve by Cramer's rule, with no
/// precomputed plane equation and no division until the ray is known to hit.
/// Two-sided, because a probe standing inside geometry sees that geometry's
/// back faces and the distance to them is exactly what says it is inside.
///
/// `EPSILON` is what rejects a ray in the triangle's own plane, where the
/// determinant is zero and the barycentric coordinates are a division by it.
fn ray_triangle(origin: [f32; 3], direction: [f32; 3], triangle: &[[f32; 3]; 3]) -> Option<f32> {
    const EPSILON: f32 = 1.0e-8;
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    let edge1 = sub(triangle[1], triangle[0]);
    let edge2 = sub(triangle[2], triangle[0]);
    let pvec = cross(direction, edge2);
    let determinant = dot(edge1, pvec);
    if determinant.abs() < EPSILON {
        return None;
    }
    let inv = 1.0 / determinant;
    let tvec = sub(origin, triangle[0]);
    let u = dot(tvec, pvec) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = cross(tvec, edge1);
    let v = dot(direction, qvec) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = dot(edge2, qvec) * inv;
    // Behind the probe, which is a different direction's business.
    (distance > 0.0).then_some(distance)
}

/// The distance from `origin` along `direction` to the nearest triangle, or
/// [`FAR`] if the ray leaves the scene without meeting one.
fn nearest(origin: [f32; 3], direction: [f32; 3], triangles: &[[[f32; 3]; 3]]) -> f32 {
    let mut best = FAR;
    for triangle in triangles {
        if let Some(distance) = ray_triangle(origin, direction, triangle)
            && distance < best
        {
            best = distance;
        }
    }
    best
}

/// The visibility image for `volume`, captured against the static geometry
/// `occluders` place.
///
/// One layer per probe in the table's own `x`-fastest order, so layer `i` is
/// probe row `i` — which is what lets the shader index the image with the same
/// number it indexes the probe table with.
///
/// A volume with no probes, or a scene with nothing placed, captures nothing and
/// returns [`ProbeVisibility::NONE`]: there is no geometry for a map to be about,
/// and the value that occludes nothing is the honest answer.
pub(crate) fn capture(
    volume: &ProbeVolume,
    geometry: &Occluders,
    occluders: &[Occluder],
) -> ProbeVisibility {
    let probes = volume.total();
    if probes == 0 || geometry.is_empty() {
        return ProbeVisibility::NONE;
    }
    let triangles = world_triangles(geometry, occluders);
    let mut layers = vec![0u8; probes as usize * LAYER_BYTES];
    let mut at = 0usize;
    for z in 0..volume.counts[2].max(1) {
        for y in 0..volume.counts[1].max(1) {
            for x in 0..volume.counts[0].max(1) {
                let origin = volume.position([x, y, z]);
                for texel_y in 0..EXTENT {
                    for texel_x in 0..EXTENT {
                        let distance =
                            nearest(origin, texel_direction(texel_x, texel_y), &triangles);
                        layers[at..at + 4].copy_from_slice(&distance.to_le_bytes());
                        layers[at + 4..at + 8]
                            .copy_from_slice(&(distance * distance).to_le_bytes());
                        at += TEXEL_BYTES;
                    }
                }
            }
        }
    }
    debug_assert_eq!(at, layers.len());
    ProbeVisibility::new(layers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT;

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

    /// The intersector against distances written out by hand — a slip in the
    /// barycentric solve would pass every capture test here, because nothing
    /// else in this tree knows what the right answer was.
    #[test]
    fn the_intersector_reports_the_distance_along_the_ray() {
        // A triangle in the plane `z = 2`, spanning the first quadrant.
        let triangle = [[0.0, 0.0, 2.0], [4.0, 0.0, 2.0], [0.0, 4.0, 2.0]];
        assert_eq!(
            ray_triangle([0.5, 0.5, 0.0], [0.0, 0.0, 1.0], &triangle),
            Some(2.0),
            "straight at it from two units away"
        );
        // Twice the direction is half the parameter: the distance is in units
        // of `direction`, which is why every caller hands it a unit vector.
        assert_eq!(
            ray_triangle([0.5, 0.5, 0.0], [0.0, 0.0, 2.0], &triangle),
            Some(1.0)
        );
        // Past the triangle's hypotenuse, inside its bounding box.
        assert_eq!(
            ray_triangle([3.0, 3.0, 0.0], [0.0, 0.0, 1.0], &triangle),
            None
        );
        // Behind the origin.
        assert_eq!(
            ray_triangle([0.5, 0.5, 3.0], [0.0, 0.0, 1.0], &triangle),
            None
        );
        // In the triangle's own plane.
        assert_eq!(
            ray_triangle([0.5, 0.5, 2.0], [1.0, 0.0, 0.0], &triangle),
            None
        );
        // From the far side: two-sided, because a probe inside geometry has to
        // see the back of it.
        assert_eq!(
            ray_triangle([0.5, 0.5, 5.0], [0.0, 0.0, -1.0], &triangle),
            Some(3.0)
        );
    }

    /// **A probe inside a room records the room**: every direction meets a wall
    /// at the distance the room's own dimensions give.
    #[test]
    fn a_probe_in_a_box_records_the_walls_it_is_inside() {
        // A four-unit cube about the origin, one probe at its centre.
        let volume = ProbeVolume {
            origin: [0.0, 0.0, 0.0],
            inv_spacing: [0.0, 0.0, 0.0],
            counts: [1, 1, 1],
        };
        let placed = [Occluder {
            mesh: 0,
            transform: Mat4::from_scale(glam::Vec3::splat(4.0)),
        }];
        let map = capture(&volume, &one_cube(), &placed);
        assert_eq!(map.probes(), 1);
        // Straight at a face is the half extent.
        for axis in [
            [1.0f32, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            let measured = map.moments(0, axis)[0];
            assert!(
                (measured - 2.0).abs() <= 0.05,
                "straight at a face of a four-unit cube measured {measured}"
            );
        }
        // **Every texel is inside the box**, between its half extent and its
        // half diagonal — the statement that the probe is enclosed, which one
        // ray leaking out through a gap in the winding would break and no
        // single direction would show.
        let (mut low, mut high) = (f32::MAX, 0.0f32);
        for y in 0..EXTENT {
            for x in 0..EXTENT {
                let distance = map.texel(0, x, y)[0];
                low = low.min(distance);
                high = high.max(distance);
            }
        }
        assert!(
            low >= 2.0 - 1.0e-4 && high <= 2.0 * 3.0f32.sqrt() + 1.0e-4,
            "a probe at the centre of a four-unit cube recorded distances from {low} to \
             {high}, and everything it can see is between 2 and 2√3 away"
        );
        // And the moments are consistent: the second is the square of the
        // first, texel by texel, because one ray filled both.
        let straight = map.texel(0, EXTENT / 2, EXTENT / 2);
        assert!((straight[1] - straight[0] * straight[0]).abs() <= 1.0e-3 * straight[1]);
    }

    /// **The rung's whole claim, on the host**: a probe with a wall between it
    /// and a surface keeps none of its weight, and the same probe with the wall
    /// taken away keeps all of it.
    ///
    /// The wall is the only thing that differs between the two runs, so nothing
    /// else — the probe's place, the surface's place, the map's resolution —
    /// can be what moved the answer.
    #[test]
    fn a_wall_between_a_probe_and_a_surface_takes_the_probes_weight() {
        // The probe stands at `x = +1`, the shaded point on the floor at
        // `x = -1`, and the wall is a thin slab on the plane `x = 0`.
        let volume = ProbeVolume {
            origin: [1.0, 1.0, 0.0],
            inv_spacing: [0.0, 0.0, 0.0],
            counts: [1, 1, 1],
        };
        let floor = [-1.0f32, 0.0, 0.0];
        let up = [0.0f32, 1.0, 0.0];
        let wall = Occluder {
            mesh: 0,
            transform: Mat4::from_scale(glam::Vec3::new(0.05, 6.0, 6.0)),
        };
        let ground = Occluder {
            mesh: 0,
            transform: Mat4::from_translation(glam::Vec3::new(0.0, -0.5, 0.0))
                * Mat4::from_scale(glam::Vec3::new(8.0, 1.0, 8.0)),
        };

        let open = capture(&volume, &one_cube(), &[ground]);
        let walled = capture(&volume, &one_cube(), &[ground, wall]);
        let position = volume.position([0, 0, 0]);
        let seen = open.weight(0, position, floor, up);
        let hidden = walled.weight(0, position, floor, up);
        eprintln!("crcbl probes: probe weight {seen:.4} in the open, {hidden:.6} behind a wall");
        assert!(
            seen >= 0.99,
            "with nothing in the way the probe must keep its weight, and it kept {seen}"
        );
        assert_eq!(
            hidden, OCCLUDED_WEIGHT,
            "a probe on the far side of a wall must keep only the floor weight"
        );
    }

    /// The surface bias is what stops a floor shadowing itself against the probe
    /// directly above it — the case the whole grid rests on, and the one a
    /// capture with no bias gets wrong on half its fragments.
    #[test]
    fn a_floor_keeps_the_probe_standing_over_it() {
        let volume = ProbeVolume {
            origin: [0.0, 1.0, 0.0],
            inv_spacing: [0.0, 0.0, 0.0],
            counts: [1, 1, 1],
        };
        let ground = Occluder {
            mesh: 0,
            transform: Mat4::from_translation(glam::Vec3::new(0.0, -0.5, 0.0))
                * Mat4::from_scale(glam::Vec3::new(12.0, 1.0, 12.0)),
        };
        let map = capture(&volume, &one_cube(), &[ground]);
        let position = volume.position([0, 0, 0]);
        for offset in [0.0f32, 0.25, 0.5, 1.0, 2.0] {
            let point = [offset, 0.0, 0.0];
            let weight = map.weight(0, position, point, [0.0, 1.0, 0.0]);
            assert!(
                weight >= 0.99,
                "the floor {offset} unit(s) from under the probe kept only {weight} of it, \
                 which is the floor shadowing itself"
            );
        }
    }

    /// A scene that reserves no probes retains no geometry and captures nothing,
    /// which is what keeps the cost of the feature zero for every caller that
    /// does not use it.
    #[test]
    fn a_scene_with_no_probes_keeps_nothing() {
        let scene = crate::scene::demo();
        assert_eq!(scene.capacities.probes, 0);
        let ids: Vec<u32> = (0..scene.meshes.len() as u32).collect();
        let geometry = Occluders::from_scene(&scene, &ids);
        assert!(geometry.is_empty());
        assert_eq!(
            capture(&scene.probes.volume, &geometry, &[]),
            ProbeVisibility::NONE
        );
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
