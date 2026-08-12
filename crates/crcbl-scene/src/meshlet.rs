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
//! # Clusters grow across shared edges, not along the index buffer
//!
//! The build seeds a cluster on a triangle and then repeatedly takes the
//! **adjacent** triangle that fits it best, where adjacent means *shares an
//! edge*: most vertex reuse first, then nearest the triangle it was seeded on,
//! then the lowest triangle index. When neither bound has room for the chosen
//! triangle, the cluster is closed and that triangle seeds the next one. A
//! cluster whose frontier runs dry with room to spare — one that has swallowed
//! a whole connected component — seeds again from the lowest triangle no
//! cluster has taken, but only if it can take the whole of *that* triangle's
//! component too — a cluster jumps only when it can take everything it is
//! jumping to, or its bounding sphere would span the distance it jumped.
//!
//! The alternative was to sort the triangles along a space-filling curve and
//! keep the sequential walk. Growth across edges is what this does instead, for
//! two reasons a Morton order cannot give: a curve sorts *space*, so two
//! surfaces a hair apart interleave along it and land in one cluster whose
//! bounding sphere spans the gap between them, and the vertex bound — the one
//! that actually closes most clusters — is about vertex *sharing*, which
//! adjacency measures directly and proximity only predicts.
//!
//! Growing by index order is what this replaced, and it is worth naming what
//! that cost: a cluster followed the index buffer and nothing else, so a
//! row-major grid gave clusters one full grid row wide. On the 32 × 32 dune
//! fixture the mean cluster bounding-sphere radius was 16.0 on a mesh 32
//! across — every cluster spanning the whole of it — against 6.9 here.
//! `a_dense_grid_clusters_into_compact_spheres` is what holds that, and the
//! per-cluster cull and per-cluster LOD selection are what want it: bounds that
//! name a region rather than the whole mesh.
//!
//! What it does **not** buy is a clean tiling. The clusters are grown one at a
//! time, and discs do not tile a plane, so the last few clusters of a dense
//! mesh are the interstitial scraps the earlier ones left — two of the dune
//! fixture's 23, and they still span it. A partitioner that grew every cluster
//! at once (METIS-class, or Lloyd relaxation over cluster seeds) is what
//! removes those, and it is not in this slice.
//!
//! # Determinism
//!
//! §3.5 asks for "same input hash, same clusters", so nothing here may depend
//! on an iteration order the standard library does not pin. The frontier is a
//! [`BTreeSet`] rather than a hash set, the adjacency is built through a
//! [`BTreeMap`], and the one float in the decision — the squared distance from
//! a candidate's centroid to the cluster's seed — is compared with
//! [`f32::total_cmp`] and broken on the lowest triangle index, so a tie cannot
//! be settled by whichever candidate the scan happened to reach first. No
//! trigonometry enters any of it: `sinf`/`cosf` differ in the last place
//! between libms, and a cluster boundary that moved with the C library would
//! not be the same bake on two machines.

use std::collections::{BTreeMap, BTreeSet};

use glam::Vec3;

use crate::simplify::undirected;

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

    /// The clusters, in the order the build grew them.
    #[inline]
    #[must_use]
    pub fn clusters(&self) -> &[Meshlet] {
        &self.clusters
    }

    /// One cluster's triangles as indices into the mesh, three per triangle —
    /// its corner run decoded back through its own vertex run.
    ///
    /// Every cluster's triangles as indices into the mesh, in cluster order.
    ///
    /// The same triangles the build was given, permuted into the order the
    /// clusters hold them — which is what a level of
    /// [`crate::cluster_dag`] publishes as its index buffer, so that the
    /// buffer and the clusters over it describe the geometry in one order.
    pub(crate) fn all_indices(&self) -> Vec<u32> {
        (0..self.clusters.len())
            .flat_map(|cluster| self.cluster_indices(cluster))
            .collect()
    }

    /// `pub(crate)` because [`crate::cluster_dag`] works in clusters and has to
    /// ask which triangles each one holds: to gather a group's geometry, and to
    /// find which clusters share an edge.
    pub(crate) fn cluster_indices(&self, cluster: usize) -> Vec<u32> {
        let cluster = self.clusters[cluster];
        let vertices =
            &self.vertices[cluster.vertex_offset as usize..][..cluster.vertex_count as usize];
        self.triangles[cluster.triangle_offset as usize..][..cluster.triangle_count as usize * 3]
            .iter()
            .map(|&corner| vertices[corner as usize])
            .collect()
    }

    /// A build holding no clusters, to [`append`](Self::append) into.
    pub(crate) fn empty() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            clusters: Vec::new(),
        }
    }

    /// Appends another build's clusters, shifting its offsets onto the end of
    /// this one's arrays.
    ///
    /// [`build_meshlets`] clusters one triangle list and knows nothing about
    /// where a cluster ought to stop. [`crate::cluster_dag`] needs each
    /// group of a level clustered on its own — a cluster straddling two groups
    /// would have two sets of parents and would not be a DAG node — so it calls
    /// the builder per group and this is what puts those builds back together
    /// as the one level's clusters.
    ///
    /// # Errors
    ///
    /// [`MeshletError::TooLarge`], for the same reason and at the same place
    /// [`build_meshlets`] raises it: the shifted offsets are narrowed to the
    /// `uint`s the record holds rather than cast.
    pub(crate) fn append(&mut self, other: &Self) -> Result<(), MeshletError> {
        for cluster in &other.clusters {
            self.clusters.push(Meshlet::new(
                self.vertices.len() + cluster.vertex_offset as usize,
                cluster.vertex_count as usize,
                self.triangles.len() + cluster.triangle_offset as usize,
                cluster.triangle_count as usize,
                cluster.bounds,
            )?);
        }
        self.vertices.extend_from_slice(&other.vertices);
        self.triangles.extend_from_slice(&other.triangles);
        Ok(())
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

    let neighbours = triangle_neighbours(indices);
    let mut build = MeshletBuild {
        vertices: Vec::new(),
        triangles: Vec::new(),
        clusters: Vec::new(),
    };
    let mut pending = Pending::new(&neighbours);
    let mut open = OpenCluster::new();

    loop {
        // Adjacent first; only a cluster with nothing left to grow into reaches
        // the index buffer for a seed, and only that one can land in another
        // connected component.
        let (next, seeded) = match open.best(positions, indices) {
            Some(next) => (next, false),
            None => match pending.seed() {
                Some(next) => (next, true),
                None => break,
            },
        };
        let spills = seeded
            && !open.vertices.is_empty()
            && !pending.component_fits(next, MAX_CLUSTER_TRIANGLES - open.corners.len() / 3);
        if spills || open.closes(face(indices, next)) {
            build.push_cluster(positions, &open.vertices, &open.corners)?;
            open = OpenCluster::new();
        }
        open.take(next, positions, indices, &neighbours, &mut pending);
    }
    if !open.vertices.is_empty() {
        build.push_cluster(positions, &open.vertices, &open.corners)?;
    }

    Ok(build)
}

/// One triangle's three indices.
fn face(indices: &[u32], triangle: u32) -> &[u32] {
    &indices[triangle as usize * 3..][..3]
}

/// A triangle's centroid, which is what the compactness term measures.
fn centroid(positions: &[[f32; 3]], face: &[u32]) -> Vec3 {
    face.iter()
        .map(|&index| Vec3::from(positions[index as usize]))
        .sum::<Vec3>()
        / 3.0
}

/// How many of `face`'s vertices a cluster holding `vertices` does not have.
///
/// A triangle that names the same vertex twice costs the vertex bound once, so
/// the scan looks at the corners already passed as well as at the cluster.
fn fresh_vertices(vertices: &[u32], face: &[u32]) -> usize {
    face.iter()
        .enumerate()
        .filter(|&(corner, index)| !vertices.contains(index) && !face[..corner].contains(index))
        .count()
}

/// Which triangles share an edge with which, ascending and deduplicated.
///
/// Shared edges rather than shared vertices, and a [`BTreeMap`] rather than a
/// hash map for the reason this module's determinism section gives. An edge
/// with more than two faces on it — a non-manifold seam — makes every pair of
/// them neighbours, which is what keeps a cluster growing across one instead of
/// stopping dead at it.
fn triangle_neighbours(indices: &[u32]) -> Vec<Vec<u32>> {
    let mut users: BTreeMap<[u32; 2], Vec<u32>> = BTreeMap::new();
    for (triangle, face) in indices.chunks_exact(3).enumerate() {
        for corner in 0..3 {
            let sharing = users
                .entry(undirected(face[corner], face[(corner + 1) % 3]))
                .or_default();
            // A degenerate triangle names one edge twice, and a triangle is not
            // its own neighbour. Triangles arrive in ascending order, so the
            // repeat is always the entry just pushed.
            if sharing.last() != Some(&(triangle as u32)) {
                sharing.push(triangle as u32);
            }
        }
    }

    let mut neighbours = vec![Vec::new(); indices.len() / 3];
    for sharing in users.values() {
        for (at, &a) in sharing.iter().enumerate() {
            for &b in &sharing[at + 1..] {
                neighbours[a as usize].push(b);
                neighbours[b as usize].push(a);
            }
        }
    }
    for list in &mut neighbours {
        list.sort_unstable();
        list.dedup();
    }
    neighbours
}

/// The triangles no cluster has taken, and which connected component each is
/// in.
///
/// This is where a cluster is seeded from — when the build starts, when a
/// cluster closes, and when a cluster's frontier runs dry with room to spare —
/// and it is the lowest such triangle, which is the only thing left in the
/// build that reads the index buffer's order at all. The scan does not restart
/// at the front of the mesh for each seed: no triangle below
/// [`unseeded`](Self::unseeded) is still untaken.
struct Pending {
    taken: Vec<bool>,
    unseeded: usize,

    /// Which connected component of the adjacency graph each triangle is in.
    component: Vec<u32>,

    /// How many triangles of each component no cluster has taken yet.
    untaken: Vec<usize>,
}

impl Pending {
    fn new(neighbours: &[Vec<u32>]) -> Self {
        let mut component = vec![u32::MAX; neighbours.len()];
        let mut untaken = Vec::new();
        for start in 0..neighbours.len() {
            if component[start] != u32::MAX {
                continue;
            }
            let label = untaken.len() as u32;
            let mut size = 0usize;
            let mut reached = vec![start as u32];
            component[start] = label;
            while let Some(triangle) = reached.pop() {
                size += 1;
                for &neighbour in &neighbours[triangle as usize] {
                    if component[neighbour as usize] == u32::MAX {
                        component[neighbour as usize] = label;
                        reached.push(neighbour);
                    }
                }
            }
            untaken.push(size);
        }

        Self {
            taken: vec![false; neighbours.len()],
            unseeded: 0,
            component,
            untaken,
        }
    }

    /// The triangle to seed a cluster from, or `None` once every triangle is in
    /// one.
    fn seed(&mut self) -> Option<u32> {
        while self.unseeded < self.taken.len() && self.taken[self.unseeded] {
            self.unseeded += 1;
        }
        (self.unseeded < self.taken.len()).then_some(self.unseeded as u32)
    }

    /// Whether what is left of `triangle`'s connected component fits in
    /// `capacity` more triangles.
    ///
    /// **A cluster jumps only when it can take everything it is jumping to.**
    /// Reaching a seed at all means the cluster ran out of triangles adjacent
    /// to what it holds, and the next one along the index buffer can be
    /// anywhere — the other side of a gap, another object in the same mesh, the
    /// far side of the region earlier clusters carved out — so a cluster that
    /// took an arbitrary slice of it would get a bounding sphere spanning the
    /// distance between the two and cull nothing. It closes instead, and the
    /// next cluster starts there.
    ///
    /// Taking a whole component is the case that has to stay allowed, and it is
    /// the common one: a mesh whose vertices are split per face, which is every
    /// mesh with hard normals or a UV seam, is a heap of two-triangle
    /// components, and one cluster per two triangles would be no clustering at
    /// all.
    fn component_fits(&self, triangle: u32, capacity: usize) -> bool {
        self.untaken[self.component[triangle as usize] as usize] <= capacity
    }

    fn is_untaken(&self, triangle: u32) -> bool {
        !self.taken[triangle as usize]
    }

    fn take(&mut self, triangle: u32) {
        self.taken[triangle as usize] = true;
        self.untaken[self.component[triangle as usize] as usize] -= 1;
    }
}

/// The cluster being filled and the frontier it grows across.
struct OpenCluster {
    /// Original vertex indices in first-seen order: this cluster's vertex run.
    vertices: Vec<u32>,

    /// Its corners, three per triangle, indexing [`vertices`](Self::vertices).
    corners: Vec<u8>,

    /// Untaken triangles sharing an edge with one already in the cluster.
    frontier: BTreeSet<u32>,

    /// The centroid of the triangle this cluster was seeded on, which the
    /// compactness term measures distance from.
    ///
    /// **The seed and not the cluster's own moving centre**, which is what
    /// keeps a cluster round. Measured from a centre that moves with the
    /// cluster, a cluster growing along a strip of triangles finds the
    /// candidates at each end of the strip equidistant and takes them
    /// alternately, so it grows into a strip as long as the mesh — which is
    /// exactly what the residue between two rounds of clusters is made of.
    /// Measured from a fixed point, growth is nearest-first and the cluster is
    /// a disc around it however the geometry it is picking through is shaped.
    seed: Vec3,
}

impl OpenCluster {
    fn new() -> Self {
        Self {
            vertices: Vec::with_capacity(MAX_CLUSTER_VERTICES),
            corners: Vec::with_capacity(MAX_CLUSTER_TRIANGLES * 3),
            frontier: BTreeSet::new(),
            seed: Vec3::ZERO,
        }
    }

    /// Whether `face` would push this cluster past either bound.
    ///
    /// An empty cluster always takes the triangle: three vertices and one
    /// triangle are inside both bounds, so the build cannot stall on a triangle
    /// no cluster will accept.
    fn closes(&self, face: &[u32]) -> bool {
        if self.vertices.is_empty() {
            return false;
        }
        if self.corners.len() / 3 >= MAX_CLUSTER_TRIANGLES {
            return true;
        }
        self.vertices.len() + fresh_vertices(&self.vertices, face) > MAX_CLUSTER_VERTICES
    }

    /// The frontier triangle to take next, or `None` when the frontier is
    /// empty and the cluster has to be seeded from the mesh instead.
    ///
    /// Most vertex reuse first, then nearest this cluster's box centre, then
    /// the lowest triangle index. Reuse leads because the vertex bound is what
    /// closes most clusters and a triangle that adds none is free against it —
    /// and because a triangle sharing two edges with the cluster is filling a
    /// notch in its outline, which is what keeps the outline round.
    fn best(&self, positions: &[[f32; 3]], indices: &[u32]) -> Option<u32> {
        let key = |triangle: u32| {
            let face = face(indices, triangle);
            (
                fresh_vertices(&self.vertices, face),
                centroid(positions, face).distance_squared(self.seed),
            )
        };
        // `min_by` keeps the first of equal elements and a `BTreeSet` iterates
        // ascending, so an exact tie falls to the lowest triangle index rather
        // than to whichever the scan reached first.
        self.frontier.iter().copied().min_by(|&a, &b| {
            let (fresh_a, near_a) = key(a);
            let (fresh_b, near_b) = key(b);
            fresh_a.cmp(&fresh_b).then(near_a.total_cmp(&near_b))
        })
    }

    /// Adds `triangle` to the cluster and its untaken neighbours to the
    /// frontier.
    fn take(
        &mut self,
        triangle: u32,
        positions: &[[f32; 3]],
        indices: &[u32],
        neighbours: &[Vec<u32>],
        pending: &mut Pending,
    ) {
        pending.take(triangle);
        self.frontier.remove(&triangle);
        if self.vertices.is_empty() {
            self.seed = centroid(positions, face(indices, triangle));
        }
        for &index in face(indices, triangle) {
            let local = self
                .vertices
                .iter()
                .position(|&seen| seen == index)
                .unwrap_or_else(|| {
                    self.vertices.push(index);
                    self.vertices.len() - 1
                });
            self.corners.push(local as u8);
        }
        for &neighbour in &neighbours[triangle as usize] {
            if pending.is_untaken(neighbour) {
                self.frontier.insert(neighbour);
            }
        }
    }
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
    use crate::simplify::tests::{dunes, height_field};

    /// Every cluster's corners decoded back to original vertex indices, in
    /// emission order, with each cluster's record checked against the bounds on
    /// the way through.
    ///
    /// Comparing this against the input's triangles is the invariant that
    /// catches an off-by-one in either offset: a wrong `vertex_offset` decodes
    /// to the wrong original indices, a wrong `triangle_offset` decodes the
    /// wrong corners, and either a dropped or a duplicated triangle changes
    /// what comes out.
    ///
    /// **Compare it through [`sorted`]**, because the builder grows clusters
    /// across shared edges and its emission order is therefore its own rather
    /// than the index buffer's. What the invariant claims is that every input
    /// triangle reaches exactly one cluster, and a comparison of two sorted
    /// lists says exactly that — a dropped, duplicated or misdecoded triangle
    /// still fails it, and only the ordering stops being asserted.
    ///
    /// `pub(crate)` because [`crate::lod`] applies the same invariant to every
    /// level of a chain, and a second copy of a decoder is a second thing to
    /// keep right.
    pub(crate) fn decoded(build: &MeshletBuild) -> Vec<[u32; 3]> {
        (0..build.clusters().len())
            .flat_map(|cluster| cluster_triangles(build, cluster))
            .collect()
    }

    /// Triangles in a canonical order, so comparing two lists asks whether they
    /// hold the same triangles the same number of times.
    ///
    /// Each triple keeps its own winding — only the list is sorted — so a
    /// triangle emitted the other way round is still a different triangle.
    pub(crate) fn sorted(mut triangles: Vec<[u32; 3]>) -> Vec<[u32; 3]> {
        triangles.sort_unstable();
        triangles
    }

    /// [`MeshletBuild::cluster_indices`] as triangles, with the record's two
    /// counts checked against the bounds first — a corner outside its own
    /// cluster's vertex run is caught by the decode's own indexing.
    ///
    /// `pub(crate)` because [`crate::cluster_dag`] applies the same invariant to
    /// every level of a DAG, whose clusters are built one group at a time.
    pub(crate) fn cluster_triangles(build: &MeshletBuild, cluster: usize) -> Vec<[u32; 3]> {
        let record = build.clusters()[cluster];
        let vertex_count = record.vertex_count as usize;
        let triangle_count = record.triangle_count as usize;
        assert!(
            vertex_count <= MAX_CLUSTER_VERTICES,
            "cluster references {vertex_count} vertices"
        );
        assert!(
            triangle_count <= MAX_CLUSTER_TRIANGLES,
            "cluster holds {triangle_count} triangles"
        );
        build
            .cluster_indices(cluster)
            .chunks_exact(3)
            .map(|triangle| [triangle[0], triangle[1], triangle[2]])
            .collect()
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
        assert_eq!(sorted(decoded(&build)), sorted(triangles_of(&indices)));
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
        assert_eq!(sorted(decoded(&build)), sorted(triangles_of(&indices)));
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
                (15, MAX_CLUSTER_TRIANGLES),
                (6, 160 - MAX_CLUSTER_TRIANGLES)
            ],
            "the triangle bound closed the first cluster: a hub and a 16-vertex \
             ring is far short of MAX_CLUSTER_VERTICES, and 10 laps of 16 is \
             160 triangles"
        );
        assert!(counts[0].0 < MAX_CLUSTER_VERTICES);
        // Neither cluster is the whole ring, because the ten copies of one
        // spoke's triangle share every edge with each other and are taken
        // together: the first cluster is 124 triangles over 14 of the ring's
        // 16 vertices, and the second is what is left of the last two spokes.
        assert!(counts[0].0 < 17 && counts[1].0 < 17);
        assert_eq!(sorted(decoded(&build)), sorted(triangles_of(&indices)));
    }

    /// **The clusters of a dense mesh are compact**, which is the whole reason
    /// the build grows across shared edges rather than along the index buffer.
    ///
    /// The mean cluster bounding-sphere radius over the dune fixture is the
    /// number, and the mesh is 32 across: growing by index order gave every
    /// cluster one full grid row, so the mean radius was 16.0 — half the mesh,
    /// per cluster, on a fixture that has 23 of them. A per-cluster cull tests
    /// that sphere and a per-cluster LOD selection descends by that region's
    /// error, and neither means anything while the region is the whole mesh.
    ///
    /// Both halves are asserted because neither is enough on its own: a mean
    /// says nothing about how many clusters are behind it, and a count says
    /// nothing about how tight they are.
    #[test]
    fn a_dense_grid_clusters_into_compact_spheres() {
        let (positions, indices) = height_field(32, dunes);

        let build = build_meshlets(&positions, &indices).unwrap();

        let radii: Vec<f32> = build
            .clusters()
            .iter()
            .map(|cluster| cluster.bounds.radius)
            .collect();
        assert_eq!(radii.len(), 23, "the fixture's clusters");
        let mean = radii.iter().sum::<f32>() / radii.len() as f32;
        assert!(
            mean < 8.0,
            "the mean cluster radius is {mean} on a mesh 32 across, where index \
             order gave 16.0 — so the clusters are no more local than the rows \
             they used to be"
        );
        // And the mean is not being carried by a few tight clusters among
        // sprawling ones. Two of these are the interstitial scraps the disc
        // packing leaves, which this pins rather than hides — see the module
        // docs.
        let compact = radii.iter().filter(|&&radius| radius < 8.0).count();
        assert_eq!(
            compact, 21,
            "clusters inside a quarter of the mesh: {radii:?}"
        );
        assert_eq!(sorted(decoded(&build)), sorted(triangles_of(&indices)));
    }

    /// **A cluster takes a second connected component whole or not at all.**
    ///
    /// Two grids a hundred units apart, each too large for one cluster. A
    /// cluster that ran out of its own grid and mopped up part of the other
    /// would get a bounding sphere spanning the gap — a sphere that fails every
    /// cull and is nowhere near the geometry it claims to bound — and it is the
    /// index buffer's order, not any distance, that would have led it there.
    ///
    /// The other half of the rule is
    /// [`a_mesh_of_disjoint_triangles_closes_each_cluster_on_the_vertex_bound`]:
    /// there every triangle is its own component and one cluster takes 21 of
    /// them, because each fits whole.
    #[test]
    fn a_cluster_never_holds_part_of_a_second_component() {
        let (grid, faces) = coplanar_grid(9);
        let positions: Vec<[f32; 3]> = grid
            .iter()
            .copied()
            .chain(grid.iter().map(|&[x, y, z]| [x + 100.0, y, z]))
            .collect();
        let offset = grid.len() as u32;
        let indices: Vec<u32> = faces
            .iter()
            .copied()
            .chain(faces.iter().map(|index| index + offset))
            .collect();

        let build = build_meshlets(&positions, &indices).unwrap();

        assert!(
            build.clusters().len() > 2,
            "each grid has to be more than one cluster for a cluster to run out \
             of one mid-way, got {} for the pair",
            build.clusters().len()
        );
        for cluster in 0..build.clusters().len() {
            let grids: BTreeSet<bool> = build
                .cluster_indices(cluster)
                .iter()
                .map(|&index| index < offset)
                .collect();
            assert_eq!(
                grids.len(),
                1,
                "cluster {cluster} holds triangles from both grids, a hundred \
                 units apart, so its sphere spans the gap"
            );
        }
        for cluster in build.clusters() {
            assert!(
                cluster.bounds.radius < 10.0,
                "a cluster of a 9 x 9 grid has radius {}, so it reached the \
                 other grid",
                cluster.bounds.radius
            );
        }
        assert_eq!(sorted(decoded(&build)), sorted(triangles_of(&indices)));
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
        assert_eq!(sorted(decoded(&backward)), sorted(triangles_of(&reversed)));
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
