//! Meshlet clustering: a triangle list becomes bounded clusters.
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.5 makes mesh shaders the primary
//! geometry path and names this as its first piece — "a mesh becomes clusters
//! of a bounded triangle count with per-cluster bounds and a normal cone.
//! Deterministic — same input hash, same clusters."
//!
//! # This is the builder and nothing else
//!
//! [`build_meshlets`] takes host arrays and returns host arrays. There is no
//! GPU upload, no amplification or mesh shader, no bake cache, and no
//! `GeometryPath::MeshShader` emit tail; nothing in the engine calls this yet.
//! Every one of those is a later slice. Read this module as the input to that
//! work rather than as a working mesh-shader path.
//!
//! # The record is `crcbl-shaders`', and the builder is this crate's
//!
//! [`Meshlet`] and [`ClusterBounds`] live in [`crcbl_shaders::meshlet`] and are
//! re-exported here.
//! `crcbl-render` has to read them and must not depend on this crate — it would
//! pull `gltf` into the renderer — so the record sits in the one crate both
//! sides already share, beside `GpuMaterial` and `MeshVertex`, and the builder
//! stays here where its first producer ([`GltfPrimitive`](crate::GltfPrimitive))
//! is.
//!
//! # The layout
//!
//! The meshoptimizer/NVIDIA three-array layout, because it is what a mesh
//! shader consumes and because a vertex bound is meaningless without it:
//! [`MeshletBuild::vertices`] is the original vertex indices, run per cluster;
//! [`MeshletBuild::triangles`] is three corners per triangle, each a `u8`
//! index *into that cluster's run of the vertex array*; and
//! [`MeshletBuild::clusters`] names both runs plus the cluster's bounds. The
//! corner being a `u8` is the whole reason [`MAX_CLUSTER_VERTICES`] exists.
//!
//! # Greedy sequential clustering, and what it costs
//!
//! The build walks the index buffer in triangle order, keeps the current
//! cluster's vertex set, and closes the cluster when the next triangle would
//! push it past either bound. That is deterministic by construction — no
//! hashing, no sort by a float, no iteration over a hash map — which is the
//! property §3.5 asks for.
//!
//! Its weakness is that it follows the index buffer's order and nothing else.
//! A mesh whose triangles are already spatially coherent clusters well; one
//! whose are not gets clusters with poor locality, and therefore loose
//! bounding spheres and wide normal cones that cull almost nothing. A spatial
//! pre-sort of the triangles is the improvement, and it is not in this slice.

use glam::Vec3;

pub use crcbl_shaders::meshlet::{
    ClusterBounds, MAX_CLUSTER_TRIANGLES, MAX_CLUSTER_VERTICES, MESHLET_STRIDE, Meshlet,
    MeshletTooLarge,
};

/// How much of a cluster's summed normal weight must survive cancellation for
/// that sum to name a direction.
///
/// A closed shape's area-weighted normals sum to zero in exact arithmetic and
/// to rounding noise in `f32`, so testing the sum against zero would hand a
/// garbage axis — normalised noise — to a backface cull, which would then drop
/// the geometry. Comparing against the *weight that went in*, rather than an
/// absolute length, makes the test independent of the mesh's scale.
const CONE_AXIS_EPSILON: f32 = 1e-4;

/// Why a triangle list could not be clustered.
///
/// The first two are caller bugs rather than data the builder could recover
/// from, and both are refused instead of clustered, because an index the
/// position array does not have has no bounds and a partial triangle has no
/// corners. The third is the mesh being larger than the *record* can describe,
/// which is a property of the input rather than a mistake in it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshletError {
    /// A cluster's offsets outgrew the `uint`s [`Meshlet`] holds.
    ///
    /// The record is what a shader reads, so its offsets are 32-bit where this
    /// builder counts in `usize`. The narrowing happens as each cluster is
    /// closed and is checked rather than cast: a wrapped offset would name
    /// another cluster's corners, which draws a plausible picture of the wrong
    /// geometry rather than failing.
    ///
    /// Reaching it takes a mesh of more than four billion cluster-vertices —
    /// no `u32` index buffer can address one — so this is a bound the type
    /// system states rather than a case the bake is expected to hit.
    #[error("a cluster outgrew the record: {0}")]
    TooLarge(#[from] MeshletTooLarge),

    /// The index buffer's length is not a whole number of triangles.
    #[error("{count} indices is not a whole number of triangles")]
    PartialTriangle {
        /// The index buffer's length.
        count: usize,
    },

    /// An index names a vertex the position array does not have.
    #[error("index {index} names a vertex outside a {vertices}-vertex mesh")]
    IndexOutOfRange {
        /// The offending index.
        index: u32,
        /// How many positions the mesh actually has.
        vertices: usize,
    },
}

/// The three arrays [`build_meshlets`] produces.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshletBuild {
    vertices: Vec<u32>,
    triangles: Vec<u8>,
    clusters: Vec<Meshlet>,
}

impl MeshletBuild {
    /// Original vertex indices, one run per cluster in cluster order.
    ///
    /// A cluster's run is `vertex_offset..vertex_offset + vertex_count`. The
    /// same original index appears once per cluster that references it, so
    /// this is longer than the mesh's vertex count.
    #[inline]
    #[must_use]
    pub fn vertices(&self) -> &[u32] {
        &self.vertices
    }

    /// Triangle corners, three per triangle, one run per cluster in cluster
    /// order.
    ///
    /// A cluster's run is
    /// `triangle_offset..triangle_offset + triangle_count * 3`, and every
    /// corner in it is an index into *that cluster's* run of
    /// [`vertices`](Self::vertices) — never into the mesh directly.
    #[inline]
    #[must_use]
    pub fn triangles(&self) -> &[u8] {
        &self.triangles
    }

    /// The clusters, in the order the walk built them.
    #[inline]
    #[must_use]
    pub fn clusters(&self) -> &[Meshlet] {
        &self.clusters
    }

    /// Appends one cluster and its two runs, narrowing the host offsets to the
    /// `uint`s the record holds.
    ///
    /// [`Meshlet::new`](crcbl_shaders::meshlet::Meshlet::new) is where the
    /// narrowing is checked, which is why this is fallible at all — see
    /// [`MeshletError::TooLarge`].
    fn push_cluster(
        &mut self,
        positions: &[[f32; 3]],
        vertices: &[u32],
        corners: &[u8],
    ) -> Result<(), MeshletError> {
        self.clusters.push(Meshlet::new(
            self.vertices.len(),
            vertices.len(),
            self.triangles.len(),
            corners.len() / 3,
            cluster_bounds(positions, vertices, corners),
        )?);
        self.vertices.extend_from_slice(vertices);
        self.triangles.extend_from_slice(corners);
        Ok(())
    }
}

/// Cluster a triangle list into meshlets.
///
/// `indices` is a triangle list over `positions`, exactly as
/// [`GltfPrimitive`](crate::GltfPrimitive) holds them; taking the two arrays
/// rather than the primitive is what lets this be driven from literals.
///
/// The result is deterministic: the same two arrays produce byte-identical
/// output arrays, on any run and in any order relative to other work.
///
/// # Errors
///
/// [`MeshletError::PartialTriangle`] when `indices` is not a whole number of
/// triangles, and [`MeshletError::IndexOutOfRange`] when an index names a
/// vertex `positions` does not have. Both are checked before any clustering
/// happens, so a refused mesh produces nothing rather than a prefix.
///
/// [`MeshletError::TooLarge`] when a cluster's offsets outgrow the `uint`s the
/// record holds. That one is checked as each cluster is closed rather than up
/// front, because it is a property of the *output* — see
/// [`Meshlet::new`](crcbl_shaders::meshlet::Meshlet::new), which is the only
/// place this crate narrows a `usize` at all.
///
/// A mesh with no indices is not an error: it yields no clusters and three
/// empty arrays.
///
/// # Panics
///
/// It does not, on any input. The two checks above are exactly what the
/// indexing below would otherwise trip on.
pub fn build_meshlets(
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Result<MeshletBuild, MeshletError> {
    if !indices.len().is_multiple_of(3) {
        return Err(MeshletError::PartialTriangle {
            count: indices.len(),
        });
    }
    if let Some(&index) = indices
        .iter()
        .find(|&&index| index as usize >= positions.len())
    {
        return Err(MeshletError::IndexOutOfRange {
            index,
            vertices: positions.len(),
        });
    }

    let mut build = MeshletBuild {
        vertices: Vec::new(),
        triangles: Vec::new(),
        clusters: Vec::new(),
    };
    let mut vertices: Vec<u32> = Vec::with_capacity(MAX_CLUSTER_VERTICES);
    let mut corners: Vec<u8> = Vec::with_capacity(MAX_CLUSTER_TRIANGLES * 3);

    for triangle in indices.chunks_exact(3) {
        if closes_cluster(&vertices, &corners, triangle) {
            build.push_cluster(positions, &vertices, &corners)?;
            vertices.clear();
            corners.clear();
        }
        for &index in triangle {
            let local = vertices
                .iter()
                .position(|&seen| seen == index)
                .unwrap_or_else(|| {
                    vertices.push(index);
                    vertices.len() - 1
                });
            corners.push(local as u8);
        }
    }
    if !vertices.is_empty() {
        build.push_cluster(positions, &vertices, &corners)?;
    }

    Ok(build)
}

/// Whether `triangle` would push the open cluster past either bound.
///
/// An empty cluster always takes the triangle: three vertices and one triangle
/// are inside both bounds, so the walk cannot stall on a triangle no cluster
/// will accept.
fn closes_cluster(vertices: &[u32], corners: &[u8], triangle: &[u32]) -> bool {
    if vertices.is_empty() {
        return false;
    }
    if corners.len() / 3 >= MAX_CLUSTER_TRIANGLES {
        return true;
    }
    // A triangle that names the same vertex twice costs the bound once, so the
    // scan looks at the corners already passed as well as the open cluster.
    let fresh = triangle
        .iter()
        .enumerate()
        .filter(|&(corner, index)| !vertices.contains(index) && !triangle[..corner].contains(index))
        .count();
    vertices.len() + fresh > MAX_CLUSTER_VERTICES
}

/// The bounding sphere and normal cone over one cluster's own geometry.
fn cluster_bounds(positions: &[[f32; 3]], vertices: &[u32], corners: &[u8]) -> ClusterBounds {
    let position = |index: u32| Vec3::from(positions[index as usize]);
    let corner = |local: u8| position(vertices[local as usize]);

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for &index in vertices {
        min = min.min(position(index));
        max = max.max(position(index));
    }
    let center = (min + max) * 0.5;
    let radius = vertices
        .iter()
        .map(|&index| center.distance(position(index)))
        .fold(0.0, f32::max);

    // Twice the area-weighted average normal, and the total weight that went
    // into it, so the cancellation test below is a ratio rather than a length.
    let mut sum = Vec3::ZERO;
    let mut weight = 0.0;
    for triangle in corners.chunks_exact(3) {
        let normal = triangle_normal(
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
        );
        sum += normal;
        weight += normal.length();
    }

    let axis = (sum.length() > CONE_AXIS_EPSILON * weight)
        .then(|| sum.try_normalize())
        .flatten();
    let Some(axis) = axis else {
        return ClusterBounds {
            center: center.into(),
            radius,
            cone_axis: ClusterBounds::OMNIDIRECTIONAL_AXIS,
            cone_cutoff: ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
        };
    };

    let mut cutoff = 1.0f32;
    for triangle in corners.chunks_exact(3) {
        // A zero-area triangle faces nowhere, so it constrains nothing. It
        // contributed nothing to `sum` either, so skipping it here keeps the
        // two halves of the cone consistent.
        if let Some(normal) = triangle_normal(
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
        )
        .try_normalize()
        {
            cutoff = cutoff.min(axis.dot(normal));
        }
    }

    ClusterBounds {
        center: center.into(),
        radius,
        cone_axis: axis.into(),
        cone_cutoff: cutoff.clamp(
            ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
            -ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
        ),
    }
}

/// A triangle's un-normalised normal: the cross product of two of its edges,
/// whose length is twice the triangle's area. Zero for a degenerate triangle.
fn triangle_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    (b - a).cross(c - a)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Every cluster's corners decoded back to original vertex indices, in
    /// emission order, with each corner checked against its own cluster's
    /// vertex run on the way through.
    ///
    /// Comparing this against the input's triangles is the invariant that
    /// catches an off-by-one in either offset: a wrong `vertex_offset` decodes
    /// to the wrong original indices, a wrong `triangle_offset` decodes the
    /// wrong corners, and either a dropped or a duplicated triangle changes
    /// the sequence.
    ///
    /// `pub(crate)` because [`crate::lod`] applies the same invariant to every
    /// level of a chain, and a second copy of a decoder is a second thing to
    /// keep right.
    pub(crate) fn decoded(build: &MeshletBuild) -> Vec<[u32; 3]> {
        let mut triangles = Vec::new();
        for cluster in build.clusters() {
            let vertex_count = cluster.vertex_count as usize;
            let triangle_count = cluster.triangle_count as usize;
            assert!(
                vertex_count <= MAX_CLUSTER_VERTICES,
                "cluster references {vertex_count} vertices"
            );
            assert!(
                triangle_count <= MAX_CLUSTER_TRIANGLES,
                "cluster holds {triangle_count} triangles"
            );
            let vertices = &build.vertices()[cluster.vertex_offset as usize..][..vertex_count];
            let corners =
                &build.triangles()[cluster.triangle_offset as usize..][..triangle_count * 3];
            for triangle in corners.chunks_exact(3) {
                triangles.push([0, 1, 2].map(|corner| {
                    let local = usize::from(triangle[corner]);
                    assert!(
                        local < vertices.len(),
                        "corner {local} is outside its cluster's {} vertices",
                        vertices.len()
                    );
                    vertices[local]
                }));
            }
        }
        triangles
    }

    /// The input's triangles, to compare [`decoded`] against.
    pub(crate) fn triangles_of(indices: &[u32]) -> Vec<[u32; 3]> {
        indices
            .chunks_exact(3)
            .map(|triangle| [triangle[0], triangle[1], triangle[2]])
            .collect()
    }

    fn assert_finite(bounds: &ClusterBounds) {
        for value in bounds
            .center
            .iter()
            .chain(&bounds.cone_axis)
            .chain([&bounds.radius, &bounds.cone_cutoff])
        {
            assert!(value.is_finite(), "{bounds:?} carries a NaN or an infinity");
        }
    }

    /// `count` triangles sharing no vertices at all, every one wound so its
    /// normal is exactly `+Z`. Three fresh vertices per triangle is what makes
    /// this the mesh that reaches [`MAX_CLUSTER_VERTICES`] soonest.
    fn disjoint_triangles(count: usize) -> (Vec<[f32; 3]>, Vec<u32>) {
        let mut positions = Vec::new();
        for triangle in 0..count {
            let x = triangle as f32;
            positions.extend([[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]]);
        }
        let indices = (0..count as u32 * 3).collect();
        (positions, indices)
    }

    /// A `side` by `side` grid of quads in the `z = 0` plane, two triangles
    /// each, every one wound so its normal is exactly `+Z`.
    fn coplanar_grid(side: usize) -> (Vec<[f32; 3]>, Vec<u32>) {
        let stride = side + 1;
        let positions = (0..stride)
            .flat_map(|y| (0..stride).map(move |x| [x as f32, y as f32, 0.0]))
            .collect();
        let mut indices = Vec::new();
        for y in 0..side as u32 {
            for x in 0..side as u32 {
                let stride = stride as u32;
                let corner = y * stride + x;
                indices.extend([corner, corner + 1, corner + stride]);
                indices.extend([corner + 1, corner + stride + 1, corner + stride]);
            }
        }
        (positions, indices)
    }

    /// A hub-and-ring triangle fan walked `laps` times over the same vertices,
    /// every triangle wound so its normal is exactly `+Z`.
    ///
    /// Repeating the lap is deliberate. A closed manifold with
    /// [`MAX_CLUSTER_VERTICES`] vertices has exactly
    /// [`MAX_CLUSTER_TRIANGLES`] faces by Euler's formula, so no real mesh
    /// reaches the triangle bound *strictly* inside the vertex bound; a
    /// repeated fan is the cheap way to make the triangle bound the one that
    /// closes a cluster.
    fn fan_laps(ring: u32, laps: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
        let mut positions = vec![[0.0, 0.0, 0.0]];
        positions.extend((0..ring).map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / ring as f32;
            [angle.cos(), angle.sin(), 0.0]
        }));
        let mut indices = Vec::new();
        for _ in 0..laps {
            for step in 0..ring {
                indices.extend([0, 1 + step, 1 + (step + 1) % ring]);
            }
        }
        (positions, indices)
    }

    /// A closed tetrahedron over `vertices`, every face wound so its normal
    /// points away from the shape's centroid.
    ///
    /// Winding it by construction rather than by hand is what makes "these
    /// normals cancel" a claim about the shape instead of a claim about my
    /// arithmetic.
    fn closed_tetrahedron(vertices: [[f32; 3]; 4]) -> (Vec<[f32; 3]>, Vec<u32>) {
        let corner = |index: u32| Vec3::from(vertices[index as usize]);
        let centroid = (0..4).map(corner).sum::<Vec3>() / 4.0;
        let mut indices = Vec::new();
        for face in [[0u32, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let [a, b, c] = face.map(corner);
            let outward = (a + b + c) / 3.0 - centroid;
            if triangle_normal(a, b, c).dot(outward) < 0.0 {
                indices.extend([face[0], face[2], face[1]]);
            } else {
                indices.extend(face);
            }
        }
        (vertices.to_vec(), indices)
    }

    /// **The clusters `crcbl-shaders` ships for its own two meshes are the
    /// ones this builder produces**, array for array.
    ///
    /// `crcbl_shaders::meshlet::cube_clusters` and its pyramid sibling are what
    /// `crcbl-render`'s mesh-shader path uploads, and they are cooked *there*
    /// because the renderer cannot reach this crate — §3.5 makes the meshlet
    /// build a bake step for that reason. This is the check that makes that
    /// arrangement honest: cooked data with the producer beside it, in the one
    /// crate that can see both.
    ///
    /// It fails if the builder's clustering changes, if a mesh grows past the
    /// cluster count the crate cooked for it, if a bound in the committed
    /// record drifts by one ulp, or if a mesh's vertex order stops being the
    /// dense ascending one that makes its corner run its own index buffer.
    ///
    /// **The open box is the one that is more than one cluster**, and it is the
    /// only one here whose committed record is computed rather than written
    /// out — see `crcbl_shaders::meshlet::open_box_clusters`. Comparing it for
    /// equality is what makes that computation a claim about this builder
    /// rather than about a second implementation of it.
    #[test]
    fn the_hardcoded_meshes_cluster_the_way_the_shaders_crate_says() {
        for (name, vertices, indices, cooked, expected_clusters) in [
            (
                "cube",
                crcbl_shaders::mesh::cube_vertices(),
                crcbl_shaders::mesh::cube_indices(),
                crcbl_shaders::meshlet::cube_clusters(),
                1,
            ),
            (
                "pyramid",
                crcbl_shaders::mesh::pyramid_vertices(),
                crcbl_shaders::mesh::pyramid_indices(),
                crcbl_shaders::meshlet::pyramid_clusters(),
                1,
            ),
            (
                "open box",
                crcbl_shaders::mesh::open_box_vertices(),
                crcbl_shaders::mesh::open_box_indices(),
                crcbl_shaders::meshlet::open_box_clusters(),
                crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            ),
        ] {
            let positions: Vec<[f32; 3]> = vertices
                .iter()
                .map(|vertex| [vertex.position[0], vertex.position[1], vertex.position[2]])
                .collect();
            let built = build_meshlets(&positions, &indices).expect("a demo mesh clusters");

            assert_eq!(
                built.clusters(),
                cooked.clusters,
                "{name}: the committed clusters are not what the builder produces"
            );
            assert_eq!(
                built.vertices(),
                cooked.vertices,
                "{name}: the committed vertex run is not the builder's"
            );
            assert_eq!(
                built.triangles(),
                cooked.corners,
                "{name}: the committed corner run is not the builder's"
            );
            // Anti-vacuity: three empty arrays would compare equal to three
            // empty arrays, and a mesh that clustered into nothing is exactly
            // what a broken builder produces.
            assert_eq!(
                built.clusters().len(),
                expected_clusters,
                "{name} is {expected_clusters} cluster(s)"
            );
            assert_eq!(
                built.triangles().len(),
                indices.len(),
                "{name}: every triangle reached a cluster"
            );
            // And the mesh that exists to be several clusters is several: a
            // cluster after the first starts part way into the mesh's own
            // vertex run, which is the case no single-cluster mesh can produce
            // however many of them are resident.
            for cluster in built.clusters().iter().skip(1) {
                assert!(
                    cluster.vertex_offset > 0 && cluster.triangle_offset > 0,
                    "{name}: cluster after the first is at offsets ({}, {}), so nothing here \
                     exercises a run that does not start at zero",
                    cluster.vertex_offset,
                    cluster.triangle_offset
                );
            }
        }
    }

    #[test]
    fn a_single_triangle_is_one_cluster_whose_cone_is_that_triangles_own_normal() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices = [0, 1, 2];

        let build = build_meshlets(&positions, &indices).unwrap();

        assert_eq!(build.clusters().len(), 1);
        assert_eq!(build.vertices(), indices);
        assert_eq!(build.triangles(), [0, 1, 2]);
        let cluster = build.clusters()[0];
        assert_eq!(cluster.vertex_count, 3);
        assert_eq!(cluster.triangle_count, 1);
        assert_eq!(
            cluster.bounds.cone_axis,
            [0.0, 0.0, 1.0],
            "the CCW triangle in the z = 0 plane faces +Z"
        );
        assert_eq!(
            cluster.bounds.cone_cutoff, 1.0,
            "one triangle's cone is that one direction, so the cosine of its \
             half angle is exactly 1"
        );
        assert_eq!(
            cluster.bounds.center,
            [0.5, 0.5, 0.0],
            "the midpoint of the (0,0,0)..(1,1,0) AABB"
        );
        assert_eq!(
            cluster.bounds.radius,
            0.5f32.hypot(0.5),
            "the centre reaches each of the three corners at the AABB's half diagonal"
        );
        assert_finite(&cluster.bounds);
    }

    #[test]
    fn a_grid_of_coplanar_triangles_gives_every_cluster_the_same_axis_and_a_cutoff_of_one() {
        let (positions, indices) = coplanar_grid(8);

        let build = build_meshlets(&positions, &indices).unwrap();

        assert!(
            build.clusters().len() > 1,
            "the grid must be larger than one cluster for this to say anything, \
             got {} cluster(s)",
            build.clusters().len()
        );
        // The exact answers are +Z and 1, but a cluster's axis is a sum of N
        // unit normals normalised back down, and `N * (1 / N)` is not 1 in f32
        // for every N. One ulp of 1.0 is the whole error budget that leaves.
        for cluster in build.clusters() {
            let axis = Vec3::from(cluster.bounds.cone_axis);
            assert!(
                (axis - Vec3::Z).length() <= f32::EPSILON,
                "every triangle in the grid faces +Z, so every cluster's axis \
                 does, got {axis}"
            );
            assert!(
                (cluster.bounds.cone_cutoff - 1.0).abs() <= f32::EPSILON,
                "coplanar triangles all sit on the axis, so the smallest dot is \
                 1, got {}",
                cluster.bounds.cone_cutoff
            );
            assert_finite(&cluster.bounds);
        }
        assert_eq!(decoded(&build), triangles_of(&indices));
    }

    #[test]
    fn a_closed_tetrahedron_gets_the_omnidirectional_cone_rather_than_a_nan() {
        // Irregular on purpose: a regular tetrahedron's normals cancel to
        // exactly zero in f32 and would never exercise CONE_AXIS_EPSILON.
        let (positions, indices) = closed_tetrahedron([
            [0.3, 1.7, -0.9],
            [2.1, -0.4, 0.6],
            [-1.2, 0.5, 1.9],
            [0.8, -1.1, -1.4],
        ]);

        let build = build_meshlets(&positions, &indices).unwrap();

        assert_eq!(build.clusters().len(), 1, "four faces fit one cluster");
        let bounds = build.clusters()[0].bounds;
        assert_finite(&bounds);
        assert_eq!(bounds.cone_axis, ClusterBounds::OMNIDIRECTIONAL_AXIS);
        assert_eq!(bounds.cone_cutoff, ClusterBounds::OMNIDIRECTIONAL_CUTOFF);
        assert!(
            bounds.radius > 1.0,
            "the sphere still bounds the shape; only the cone gave up, got {}",
            bounds.radius
        );
    }

    #[test]
    fn a_pair_of_opposing_triangles_gets_the_omnidirectional_cone() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        // The same triangle wound both ways: two normals that cancel exactly.
        let indices = [0, 1, 2, 0, 2, 1];

        let build = build_meshlets(&positions, &indices).unwrap();

        let bounds = build.clusters()[0].bounds;
        assert_finite(&bounds);
        assert_eq!(bounds.cone_axis, ClusterBounds::OMNIDIRECTIONAL_AXIS);
        assert_eq!(bounds.cone_cutoff, ClusterBounds::OMNIDIRECTIONAL_CUTOFF);
    }

    #[test]
    fn a_cluster_of_nothing_but_zero_area_triangles_gets_the_omnidirectional_cone() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let indices = [0, 1, 2];

        let build = build_meshlets(&positions, &indices).unwrap();

        let bounds = build.clusters()[0].bounds;
        assert_finite(&bounds);
        assert_eq!(bounds.cone_axis, ClusterBounds::OMNIDIRECTIONAL_AXIS);
        assert_eq!(bounds.cone_cutoff, ClusterBounds::OMNIDIRECTIONAL_CUTOFF);
    }

    #[test]
    fn a_zero_area_triangle_beside_a_real_one_leaves_the_cone_to_the_real_one() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
        ];
        let indices = [0, 1, 2, 0, 1, 3];

        let build = build_meshlets(&positions, &indices).unwrap();

        let bounds = build.clusters()[0].bounds;
        assert_finite(&bounds);
        assert_eq!(bounds.cone_axis, [0.0, 0.0, 1.0]);
        assert_eq!(
            bounds.cone_cutoff, 1.0,
            "the collinear triangle faces nowhere, so it narrows nothing"
        );
    }

    #[test]
    fn a_mesh_of_disjoint_triangles_closes_each_cluster_on_the_vertex_bound() {
        // 64 / 3 is 21 whole triangles, so a 22nd would need 66 vertices.
        let (positions, indices) = disjoint_triangles(30);

        let build = build_meshlets(&positions, &indices).unwrap();

        let counts: Vec<_> = build
            .clusters()
            .iter()
            .map(|cluster| {
                (
                    cluster.vertex_count as usize,
                    cluster.triangle_count as usize,
                )
            })
            .collect();
        assert_eq!(
            counts,
            [(63, 21), (27, 9)],
            "the vertex bound closed the first cluster: three more vertices \
             would pass MAX_CLUSTER_VERTICES while 21 triangles is nowhere \
             near MAX_CLUSTER_TRIANGLES"
        );
        assert!(counts[0].1 < MAX_CLUSTER_TRIANGLES);
        assert!(counts[0].0 + 3 > MAX_CLUSTER_VERTICES);
        assert_eq!(decoded(&build), triangles_of(&indices));
    }

    #[test]
    fn a_repeated_fan_closes_each_cluster_on_the_triangle_bound() {
        let (positions, indices) = fan_laps(16, 10);

        let build = build_meshlets(&positions, &indices).unwrap();

        let counts: Vec<_> = build
            .clusters()
            .iter()
            .map(|cluster| {
                (
                    cluster.vertex_count as usize,
                    cluster.triangle_count as usize,
                )
            })
            .collect();
        assert_eq!(
            counts,
            [
                (17, MAX_CLUSTER_TRIANGLES),
                (17, 160 - MAX_CLUSTER_TRIANGLES)
            ],
            "the triangle bound closed the first cluster: a hub and a 16-vertex \
             ring is far short of MAX_CLUSTER_VERTICES, and 10 laps of 16 is \
             160 triangles"
        );
        assert!(counts[0].0 < MAX_CLUSTER_VERTICES);
        assert_eq!(decoded(&build), triangles_of(&indices));
    }

    #[test]
    fn the_same_mesh_built_twice_gives_identical_clusters() {
        let (positions, indices) = coplanar_grid(8);

        let first = build_meshlets(&positions, &indices).unwrap();
        let second = build_meshlets(&positions, &indices).unwrap();

        assert!(first.clusters().len() > 1, "the comparison needs clusters");
        assert_eq!(first.vertices(), second.vertices());
        assert_eq!(first.triangles(), second.triangles());
        assert_eq!(first.clusters(), second.clusters());
        assert_eq!(first, second);
    }

    /// The determinism test above would pass on a builder that always returned
    /// the same thing, so this is the half that says the output depends on the
    /// input at all.
    #[test]
    fn reversing_the_triangle_order_changes_the_clustering() {
        let (positions, indices) = disjoint_triangles(30);
        let reversed: Vec<u32> = triangles_of(&indices).into_iter().rev().flatten().collect();

        let forward = build_meshlets(&positions, &indices).unwrap();
        let backward = build_meshlets(&positions, &reversed).unwrap();

        assert_ne!(
            forward.vertices(),
            backward.vertices(),
            "the same triangles in the opposite order land in different clusters"
        );
        assert_eq!(decoded(&backward), triangles_of(&reversed));
    }

    #[test]
    fn a_mesh_with_no_indices_yields_no_clusters() {
        let build = build_meshlets(&[], &[]).unwrap();

        assert!(build.clusters().is_empty());
        assert!(build.vertices().is_empty());
        assert!(build.triangles().is_empty());
    }

    #[test]
    fn an_index_buffer_that_is_not_whole_triangles_is_refused() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

        assert_eq!(
            build_meshlets(&positions, &[0, 1, 2, 0]),
            Err(MeshletError::PartialTriangle { count: 4 })
        );
    }

    #[test]
    fn an_index_naming_a_vertex_the_mesh_does_not_have_is_refused() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

        assert_eq!(
            build_meshlets(&positions, &[0, 1, 3]),
            Err(MeshletError::IndexOutOfRange {
                index: 3,
                vertices: 3,
            }),
            "refused whole, rather than clustered up to the bad index"
        );
    }
}
