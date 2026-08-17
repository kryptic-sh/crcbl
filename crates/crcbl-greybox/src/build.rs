//! The shared machinery every primitive is built with: an accumulator for
//! vertices and triangles, the byte packing a geometry pool takes, and the
//! greedy meshlet partition that turns a triangle list into the clusters a mesh
//! stage consumes.
//!
//! # Why this crate builds its own clusters
//!
//! The engine's meshlet builder is `crcbl_scene::meshlet::build_meshlets`, and a
//! greybox pack must not depend on `crcbl-scene`: that crate pulls in the glTF
//! importer, which is the whole world a prototyping-primitive crate exists to
//! avoid. `crcbl_shaders::meshlet` hands out the *record* — [`Meshlet`],
//! [`MeshClusters`] — and its cooked clusters for the engine's own demo meshes,
//! but no general partition. So [`build_clusters`] is that partition, the same
//! greedy walk `build_meshlets` runs, kept to this crate's own geometry.
//!
//! Its output is checked the way `crcbl-shaders`'
//! `the_open_box_clusters_decode_back_to_its_index_buffer` checks the cooked
//! ones: the tests decode every cluster back through its two runs and compare
//! the result to the mesh's own index buffer, so a partition that described
//! geometry the mesh does not have fails rather than draws.

use std::borrow::Cow;
use std::collections::HashMap;

use crcbl_render::scene::Geometry;
use crcbl_shaders::mesh::{MeshVertex, VERTEX_STRIDE};
use crcbl_shaders::meshlet::{
    ClusterBounds, MAX_CLUSTER_TRIANGLES, MAX_CLUSTER_VERTICES, MeshClusters, Meshlet,
};
use glam::Vec3;

/// The neutral greybox albedo every primitive carries, linear RGBA.
///
/// A flat mid grey, because the tint a greybox surface shows comes from its
/// material row and not its geometry — [`crate::greybox_material`] is
/// [`GpuMaterial::UNTINTED`](crcbl_shaders::mesh::GpuMaterial::UNTINTED), whose
/// `base_color` is `[1.0; 4]`, so the vertex colour is what the surface actually
/// shades. Mid grey rather than white so a raw, unlit capture already reads as a
/// blockout rather than a flat white cutout.
pub const GREYBOX_ALBEDO: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

/// The texture coordinates of a quad's four corners, in the winding order
/// [`MeshBuilder::quad`] emits them.
const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The texture coordinates of a triangle's three corners.
const TRI_UV: [[f32; 3]; 1] = [[0.0, 0.0, 1.0]];

/// Accumulates the vertices and triangles of one primitive, then closes them
/// into a [`Geometry::Flat`] with its clusters.
///
/// Two ways to add geometry, and a primitive uses whichever suits it:
///
/// * [`quad`](Self::quad), [`tri`](Self::tri) and [`cuboid`](Self::cuboid) add
///   **flat-shaded** faces — a fresh set of unshared vertices per face, each
///   carrying that face's own normal, exactly as `crcbl_shaders::mesh`'s cube
///   does. This is what the box-like primitives want.
/// * [`vertex`](Self::vertex) and [`stitch_grid`](Self::stitch_grid) add
///   **smooth-shaded** surfaces from shared vertices with per-vertex normals,
///   which is what the surfaces of revolution — cylinder, sphere, capsule —
///   want so a curve does not read as a stack of facets.
pub(crate) struct MeshBuilder {
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// An empty builder.
    pub(crate) fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Adds one vertex and returns its index, for the smooth-shaded path.
    pub(crate) fn vertex(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2]) -> u32 {
        let index = u32::try_from(self.vertices.len())
            .expect("a greybox primitive has fewer vertices than u32::MAX");
        self.vertices.push(MeshVertex {
            position: [position.x, position.y, position.z, 1.0],
            normal: [normal.x, normal.y, normal.z, 0.0],
            color: GREYBOX_ALBEDO,
            uv: [uv[0], uv[1], 0.0, 0.0],
        });
        index
    }

    /// Adds one triangle from three existing vertex indices.
    pub(crate) fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    /// A flat-shaded quad. The four corners may be given in either winding; the
    /// order is flipped if needed so the face's flat normal points along
    /// `outward`, which is how every caller says which way is out without having
    /// to get a hand-wound order right.
    pub(crate) fn quad(&mut self, corners: [Vec3; 4], outward: Vec3) {
        let mut c = corners;
        let mut uv = QUAD_UV;
        if quad_normal(&c).dot(outward) < 0.0 {
            c.reverse();
            uv.reverse();
        }
        let normal = quad_normal(&c);
        let i0 = self.vertex(c[0], normal, uv[0]);
        let i1 = self.vertex(c[1], normal, uv[1]);
        let i2 = self.vertex(c[2], normal, uv[2]);
        let i3 = self.vertex(c[3], normal, uv[3]);
        self.triangle(i0, i1, i2);
        self.triangle(i0, i2, i3);
    }

    /// A flat-shaded triangle, wound outward like [`quad`](Self::quad).
    pub(crate) fn tri(&mut self, corners: [Vec3; 3], outward: Vec3) {
        let mut c = corners;
        let mut uv = TRI_UV[0];
        let mut normal = (c[1] - c[0]).cross(c[2] - c[0]);
        if normal.dot(outward) < 0.0 {
            c.swap(1, 2);
            uv.swap(1, 2);
            normal = (c[1] - c[0]).cross(c[2] - c[0]);
        }
        let normal = normal.normalize();
        let i0 = self.vertex(c[0], normal, [uv[0], 0.0]);
        let i1 = self.vertex(c[1], normal, [uv[1], 0.0]);
        let i2 = self.vertex(c[2], normal, [uv[2], 0.0]);
        self.triangle(i0, i1, i2);
    }

    /// A flat-shaded axis-aligned box spanning `min..=max`, as six quads.
    pub(crate) fn cuboid(&mut self, min: Vec3, max: Vec3) {
        let corner = |xi: usize, yi: usize, zi: usize| {
            Vec3::new([min.x, max.x][xi], [min.y, max.y][yi], [min.z, max.z][zi])
        };
        self.quad(
            [
                corner(1, 0, 0),
                corner(1, 0, 1),
                corner(1, 1, 1),
                corner(1, 1, 0),
            ],
            Vec3::X,
        );
        self.quad(
            [
                corner(0, 0, 0),
                corner(0, 0, 1),
                corner(0, 1, 1),
                corner(0, 1, 0),
            ],
            Vec3::NEG_X,
        );
        self.quad(
            [
                corner(0, 1, 0),
                corner(1, 1, 0),
                corner(1, 1, 1),
                corner(0, 1, 1),
            ],
            Vec3::Y,
        );
        self.quad(
            [
                corner(0, 0, 0),
                corner(1, 0, 0),
                corner(1, 0, 1),
                corner(0, 0, 1),
            ],
            Vec3::NEG_Y,
        );
        self.quad(
            [
                corner(0, 0, 1),
                corner(1, 0, 1),
                corner(1, 1, 1),
                corner(0, 1, 1),
            ],
            Vec3::Z,
        );
        self.quad(
            [
                corner(0, 0, 0),
                corner(1, 0, 0),
                corner(1, 1, 0),
                corner(0, 1, 0),
            ],
            Vec3::NEG_Z,
        );
    }

    /// Stitches a grid of already-added vertex indices into triangles, two per
    /// cell.
    ///
    /// `grid[r][c]` is the vertex at row `r`, column `c`. Rows must ascend in
    /// `+Y` and columns must ascend counter-clockwise about `+Y` (increasing
    /// azimuth) — the winding below faces outward under exactly that convention,
    /// which is what every surface of revolution here is built with.
    ///
    /// A pole is a whole row of one repeated vertex index — a sphere's tip is a
    /// point, not a ring — so a cell there degenerates to a single triangle. Any
    /// triangle two of whose corners are the same vertex is dropped rather than
    /// emitted as a zero-area sliver, which is what keeps the tips clean.
    pub(crate) fn stitch_grid(&mut self, grid: &[Vec<u32>]) {
        for pair in grid.windows(2) {
            let (row, next) = (&pair[0], &pair[1]);
            for c in 0..row.len() - 1 {
                let (v00, v01, v10, v11) = (row[c], row[c + 1], next[c], next[c + 1]);
                if v00 != v10 && v10 != v11 && v11 != v00 {
                    self.triangle(v00, v10, v11);
                }
                if v00 != v11 && v11 != v01 && v01 != v00 {
                    self.triangle(v00, v11, v01);
                }
            }
        }
    }

    /// Closes the builder into a flat geometry with its meshlet clusters.
    pub(crate) fn finish(self) -> Geometry<'static> {
        let clusters = build_clusters(&self.vertices, &self.indices);
        Geometry::Flat {
            vertices: Cow::Owned(vertex_bytes(&self.vertices)),
            indices: Cow::Owned(self.indices),
            clusters,
        }
    }
}

/// The flat normal of a planar quad given counter-clockwise, unit length.
fn quad_normal(corners: &[Vec3; 4]) -> Vec3 {
    (corners[1] - corners[0])
        .cross(corners[3] - corners[0])
        .normalize()
}

/// The vertices as the little-endian `f32` bytes a geometry pool holds —
/// position, normal, colour, uv per vertex, [`VERTEX_STRIDE`] bytes each, which
/// is exactly what `crcbl_shaders::mesh::cube_vertex_bytes` produces.
fn vertex_bytes(vertices: &[MeshVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_STRIDE);
    for vertex in vertices {
        for value in vertex
            .position
            .iter()
            .chain(&vertex.normal)
            .chain(&vertex.color)
            .chain(&vertex.uv)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// Partitions a triangle list into meshlet clusters, the greedy walk
/// `crcbl_scene::meshlet::build_meshlets` runs.
///
/// A cluster grows one triangle at a time until the next triangle would push it
/// past [`MAX_CLUSTER_VERTICES`] distinct vertices or [`MAX_CLUSTER_TRIANGLES`]
/// triangles, at which point it closes and the next opens. Triangles are never
/// reordered, so the clusters decode back to the original index buffer in order
/// — the property this crate's tests assert.
///
/// The bounds are conservative rather than tight: the centre and radius are the
/// vertex AABB's midpoint and its furthest vertex, exactly as
/// `crcbl_shaders::meshlet::cube_clusters` writes them, and the normal cone is
/// [`ClusterBounds::OMNIDIRECTIONAL_CUTOFF`] — a cone that rejects nothing. A
/// per-cluster cone would let the amplification stage cull wholly back-facing
/// clusters, but a greybox mesh is a handful of clusters a prototype draws a few
/// of, so the safe cone that can never wrongly drop a triangle is the right
/// trade here; getting a cone wrong silently removes geometry.
fn build_clusters(vertices: &[MeshVertex], indices: &[u32]) -> MeshClusters {
    let mut clusters = Vec::new();
    let mut out_vertices: Vec<u32> = Vec::new();
    let mut out_corners: Vec<u8> = Vec::new();

    let mut triangle = 0;
    while triangle * 3 < indices.len() {
        let vertex_offset = out_vertices.len();
        let triangle_offset = out_corners.len();
        let mut local: HashMap<u32, u8> = HashMap::new();
        let mut run: Vec<u32> = Vec::new();
        let mut corners: Vec<u8> = Vec::new();

        while triangle * 3 < indices.len() && corners.len() / 3 < MAX_CLUSTER_TRIANGLES {
            let tri = [
                indices[triangle * 3],
                indices[triangle * 3 + 1],
                indices[triangle * 3 + 2],
            ];

            // How many of this triangle's vertices this cluster does not hold
            // yet, counting a repeated vertex once.
            let mut fresh: Vec<u32> = Vec::new();
            for &global in &tri {
                if !local.contains_key(&global) && !fresh.contains(&global) {
                    fresh.push(global);
                }
            }
            if run.len() + fresh.len() > MAX_CLUSTER_VERTICES {
                break;
            }

            for &global in &tri {
                let corner = *local.entry(global).or_insert_with(|| {
                    let corner = u8::try_from(run.len())
                        .expect("a cluster references at most MAX_CLUSTER_VERTICES vertices");
                    run.push(global);
                    corner
                });
                corners.push(corner);
            }
            triangle += 1;
        }

        let triangle_count = corners.len() / 3;
        clusters.push(
            Meshlet::new(
                vertex_offset,
                run.len(),
                triangle_offset,
                triangle_count,
                cluster_bounds(&run, vertices),
            )
            .expect("greybox cluster offsets fit a u32"),
        );
        out_vertices.extend_from_slice(&run);
        out_corners.extend_from_slice(&corners);
    }

    MeshClusters {
        clusters,
        vertices: out_vertices,
        corners: out_corners,
    }
}

/// One cluster's bounding sphere over the vertices its run names, with the cone
/// that culls nothing — see [`build_clusters`] for why the cone is left open.
fn cluster_bounds(run: &[u32], vertices: &[MeshVertex]) -> ClusterBounds {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for &global in run {
        let position = vertices[global as usize].position;
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let mut radius = 0.0_f32;
    for &global in run {
        let position = vertices[global as usize].position;
        let distance = ((position[0] - center[0]).powi(2)
            + (position[1] - center[1]).powi(2)
            + (position[2] - center[2]).powi(2))
        .sqrt();
        radius = radius.max(distance);
    }
    ClusterBounds {
        center,
        radius,
        cone_axis: ClusterBounds::OMNIDIRECTIONAL_AXIS,
        cone_cutoff: ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
    }
}
