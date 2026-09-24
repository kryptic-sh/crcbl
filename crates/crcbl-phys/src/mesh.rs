//! Static triangle meshes: level geometry — floors, ramps, stairs — that the
//! contact solver and the query world both collide triangle by triangle.
//!
//! A [`TriangleMesh`] is vertices and triangles in its own frame, validated
//! once, with a bounding volume hierarchy over its triangles and a record of
//! which of each triangle's edges are **active**. It goes on a static or
//! kinematic body as [`crate::ColliderComponent::Mesh`], or straight into a
//! [`crate::PhysicsWorld`] with [`crate::PhysicsWorld::add_mesh`].
//!
//! # Validation: refused, not skipped
//!
//! [`TriangleMesh::new`] refuses a mesh with no triangles, a vertex holding a
//! `NaN` or an infinity, an index past the vertices, and a **degenerate
//! triangle** — one whose height across its longest edge is under
//! [`TriangleMesh::MIN_ASPECT`] of that edge, zero-area and repeated-index
//! triangles included. A degenerate triangle has no normal to push along, and
//! skipping it silently would renumber every triangle after it, so the
//! triangle indices a [`ContactReport`](crate::ContactReport) or a
//! [`MeshHit`] names would no longer be the indices the game gave. Refusing
//! names the first offender instead, and the game cleans its data once.
//!
//! # Active edges: no catching on seams
//!
//! A box sliding across a floor made of two triangles meets the seam between
//! them: to the second triangle alone, the box's leading edge is sinking into
//! _its_ edge, and the separating axis says so with a normal pointing back
//! against the slide — a ghost collision that jolts or stops the box. The fix
//! transcribed here is Jolt Physics' (`Jolt/Geometry/ActiveEdges.h`, Jorrit
//! Rouwé), the precomputed form of Bullet's `btAdjustInternalEdgeContacts`:
//!
//! - **At build**, each edge two triangles share is marked active only if it
//!   is **convex** and bends more than [`TriangleMesh::ACTIVE_EDGE_COSINE`]'s
//!   angle (`IsEdgeActive`); a coplanar seam or a concave crease is inactive.
//!   An edge of one triangle, or of three or more, is active.
//! - **At contact**, a contact on an inactive edge, or on a vertex whose two
//!   edges are both inactive, pushes along the **triangle's normal** instead
//!   of the direction it found (`FixNormal`). A stair's nose is active, so a
//!   ball rolls off it round; a seam in a floor is not.
//!
//! Jolt's `FixNormal` also takes the pair's relative velocity as a hint for a
//! body grazing a triangle's inactive edge side-on; that part is not
//! transcribed — see `docs/notes/simulation.md` and `docs/backlog.md`.
//!
//! Edges are shared by vertex **position**, not by index: vertices at the
//! same point are welded first, so a mesh an exporter wrote with each face's
//! corners repeated still finds its seams. Only exactly equal positions weld.
//!
//! # One-sided to contacts, two-sided to queries
//!
//! To the contact solver a triangle is **one-sided**: a shape whose centre is
//! behind its plane — against its normal, by the right-hand winding
//! `(v1 − v0) × (v2 − v0)` — is not collided with it, as Box2D v3's chain
//! segments are (`b2CollideChainSegmentAndPolygon`, "polygon is behind
//! segment"). So a body pushed a little through a floor is pushed back up, not
//! down and out of the level. To rays, sweeps and overlaps a triangle is a
//! surface with two sides, and a hit's normal faces the side it came from.
//!
//! # Cost
//!
//! In a system with contacts **each triangle is a broadphase proxy of its
//! own**, as each part of a compound is (see [`crate::contact`]), so a contact
//! is one body against one triangle, with the triangle's own manifold, feature
//! ids and warm start. The query world keeps one entry per mesh and descends
//! the mesh's own tree.

pub(crate) mod geometry;
mod query;

use std::fmt;
use std::sync::Arc;

use glam::DVec3;

pub use self::query::MeshHit;
pub(crate) use self::query::{MeshScratch, PlacedMesh};
use crate::broadphase::Bvh;
use crate::collider::Aabb;

/// A static triangle mesh, validated, with active edges and a tree over its
/// triangles: see the [module docs](self).
///
/// Cloning copies a pointer: the data is shared behind an [`Arc`].
#[derive(Clone)]
pub struct TriangleMesh {
    data: Arc<MeshData>,
}

/// The shared half of a [`TriangleMesh`].
#[derive(Clone)]
struct MeshData {
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    /// Each triangle's unit normal.
    normals: Vec<DVec3>,
    /// Each triangle's active edges: bit `i` for edge `i`, from vertex `i` to
    /// vertex `i + 1`.
    active: Vec<u8>,
    /// Over the triangles, each named by its index.
    bvh: Bvh,
    bounds: Aabb,
}

/// Why [`TriangleMesh::new`] refused a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    /// No triangles: nothing to collide with.
    NoTriangles,
    /// More triangles than a `u32` can name.
    TooManyTriangles {
        /// How many it was given.
        count: usize,
    },
    /// The vertex at `index` holds a `NaN` or an infinity.
    NonFiniteVertex {
        /// Its index.
        index: usize,
    },
    /// Triangle `triangle` names a vertex that does not exist.
    IndexOutOfRange {
        /// The triangle.
        triangle: usize,
        /// The index it named.
        index: u32,
    },
    /// Triangle `triangle` is degenerate: see [`TriangleMesh::MIN_ASPECT`].
    DegenerateTriangle {
        /// The triangle.
        triangle: usize,
    },
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTriangles => write!(f, "a triangle mesh needs at least one triangle"),
            Self::TooManyTriangles { count } => {
                write!(
                    f,
                    "a triangle mesh has at most 2^32 - 1 triangles, not {count}"
                )
            }
            Self::NonFiniteVertex { index } => {
                write!(f, "mesh vertex {index} is not finite")
            }
            Self::IndexOutOfRange { triangle, index } => {
                write!(
                    f,
                    "mesh triangle {triangle} names vertex {index}, which does not exist"
                )
            }
            Self::DegenerateTriangle { triangle } => {
                write!(f, "mesh triangle {triangle} is degenerate")
            }
        }
    }
}

impl std::error::Error for MeshError {}

impl TriangleMesh {
    /// The least a triangle's height across its longest edge may be, as a
    /// share of that edge, before it is refused as degenerate: a micrometre
    /// across a metre. Below it the normal is rounding.
    pub const MIN_ASPECT: f64 = 1.0e-6;

    /// The cosine of the angle a convex edge between two triangles must bend
    /// by to be active: five degrees, Jolt's default
    /// `mActiveEdgeCosThresholdAngle`. A seam flatter than that is treated as
    /// flat.
    pub const ACTIVE_EDGE_COSINE: f64 = 0.996_194_698_091_745_5;

    /// The cosine past which two triangles' normals are opposite, back to
    /// back: one degree, Jolt's.
    const BACK_TO_BACK_COSINE: f64 = 0.999_847_695_156_391_2;

    /// A mesh of `triangles`, each three indices into `vertices` wound so
    /// `(v1 − v0) × (v2 − v0)` is its outward normal.
    ///
    /// # Errors
    ///
    /// See the [module docs](self): [`MeshError::NoTriangles`] and
    /// [`MeshError::TooManyTriangles`] first, then the first non-finite vertex
    /// in vertex order, then the first bad triangle in triangle order — an
    /// index out of range before degeneracy.
    pub fn new(vertices: &[DVec3], triangles: &[[u32; 3]]) -> Result<Self, MeshError> {
        if triangles.is_empty() {
            return Err(MeshError::NoTriangles);
        }
        if u32::try_from(triangles.len()).is_err() {
            return Err(MeshError::TooManyTriangles {
                count: triangles.len(),
            });
        }
        if let Some(index) = vertices.iter().position(|v| !v.is_finite()) {
            return Err(MeshError::NonFiniteVertex { index });
        }
        let mut normals = Vec::with_capacity(triangles.len());
        for (triangle, indices) in triangles.iter().enumerate() {
            if let Some(&index) = indices.iter().find(|&&i| i as usize >= vertices.len()) {
                return Err(MeshError::IndexOutOfRange { triangle, index });
            }
            let [a, b, c] = indices.map(|i| vertices[i as usize]);
            let cross = (b - a).cross(c - a);
            let longest = (b - a)
                .length_squared()
                .max((c - b).length_squared())
                .max((a - c).length_squared());
            // |cross| is the longest edge times the height across it. One so
            // large its products overflow has no normal to take either.
            let twice_area = cross.length();
            if !twice_area.is_finite() || twice_area <= Self::MIN_ASPECT * longest {
                return Err(MeshError::DegenerateTriangle { triangle });
            }
            normals.push(cross.normalize());
        }

        let active = active_edges(vertices, triangles, &normals);
        let boxes: Vec<(Aabb, u32)> = triangles
            .iter()
            .enumerate()
            .map(|(index, t)| {
                (
                    triangle_bounds(&t.map(|i| vertices[i as usize])),
                    index as u32,
                )
            })
            .collect();
        let bounds = boxes
            .iter()
            .fold(Aabb::EMPTY, |bounds, (aabb, _)| bounds.union(*aabb));
        Ok(Self {
            data: Arc::new(MeshData {
                vertices: vertices.to_vec(),
                triangles: triangles.to_vec(),
                normals,
                active,
                bvh: Bvh::build(boxes),
                bounds,
            }),
        })
    }

    /// This mesh with **every edge active**, seams included: Jolt's
    /// `EActiveEdgeMode::CollideWithAll`. A body sliding across a seam then
    /// catches on it, which is what the active edges are there to stop; this
    /// is for comparing against that, not for a level.
    #[must_use]
    pub fn collide_with_all_edges(mut self) -> Self {
        Arc::make_mut(&mut self.data).active.fill(0b111);
        self
    }

    /// The vertices, as given.
    #[must_use]
    pub fn vertices(&self) -> &[DVec3] {
        &self.data.vertices
    }

    /// The triangles, as given: three indices into [`vertices`](Self::vertices)
    /// each.
    #[must_use]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.data.triangles
    }

    /// How many triangles there are.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.data.triangles.len()
    }

    /// Triangle `index`'s three corners.
    ///
    /// # Panics
    ///
    /// Panics if there is no triangle `index`.
    #[must_use]
    pub fn corners(&self, index: usize) -> [DVec3; 3] {
        self.data.triangles[index].map(|i| self.data.vertices[i as usize])
    }

    /// Triangle `index`'s unit normal.
    ///
    /// # Panics
    ///
    /// Panics if there is no triangle `index`.
    #[must_use]
    pub fn normal(&self, index: usize) -> DVec3 {
        self.data.normals[index]
    }

    /// Triangle `index`'s active edges: bit `i` set when edge `i`, from its
    /// vertex `i` to vertex `i + 1`, is active. See the [module docs](self).
    ///
    /// # Panics
    ///
    /// Panics if there is no triangle `index`.
    #[must_use]
    pub fn active_edges(&self, index: usize) -> u8 {
        self.data.active[index]
    }

    /// The bounds of every triangle, in the mesh's frame.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        self.data.bounds
    }

    /// The tree over the triangles, each leaf named by its triangle's index.
    pub(crate) fn bvh(&self) -> &Bvh {
        &self.data.bvh
    }
}

impl PartialEq for TriangleMesh {
    /// Equal when the vertices, the triangles and the active edges are: the
    /// normals and the tree follow from those.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
            || (self.data.vertices == other.data.vertices
                && self.data.triangles == other.data.triangles
                && self.data.active == other.data.active)
    }
}

impl fmt::Debug for TriangleMesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TriangleMesh")
            .field("vertices", &self.data.vertices.len())
            .field("triangles", &self.data.triangles.len())
            .field("bounds", &self.data.bounds)
            .finish()
    }
}

/// The bounds of a triangle's three corners.
pub(crate) fn triangle_bounds(corners: &[DVec3; 3]) -> Aabb {
    Aabb::new(
        corners[0].min(corners[1]).min(corners[2]),
        corners[0].max(corners[1]).max(corners[2]),
    )
}

/// Each triangle's active-edge bits: see the module docs.
///
/// Vertices are welded by exact position first, then every edge is keyed by
/// its two welded ends and the keys sorted, so the triangles sharing an edge
/// come out next to each other in an order that depends only on the input.
fn active_edges(vertices: &[DVec3], triangles: &[[u32; 3]], normals: &[DVec3]) -> Vec<u8> {
    // Weld: each vertex's id is the lowest index at its exact position, `-0.0`
    // and `+0.0` alike.
    let bits = |v: DVec3| v.to_array().map(|x| if x == 0.0 { 0 } else { x.to_bits() });
    let mut order: Vec<u32> = (0..vertices.len() as u32).collect();
    order.sort_unstable_by_key(|&i| (bits(vertices[i as usize]), i));
    let mut welded = vec![0u32; vertices.len()];
    let mut run = 0usize;
    for k in 0..order.len() {
        if bits(vertices[order[k] as usize]) != bits(vertices[order[run] as usize]) {
            run = k;
        }
        welded[order[k] as usize] = order[run];
    }

    // (lower end, upper end, triangle, edge) for every edge.
    let mut edges: Vec<(u32, u32, u32, u8)> = Vec::with_capacity(triangles.len() * 3);
    for (t, indices) in triangles.iter().enumerate() {
        for e in 0..3u8 {
            let p = welded[indices[e as usize] as usize];
            let q = welded[indices[(e as usize + 1) % 3] as usize];
            edges.push((p.min(q), p.max(q), t as u32, e));
        }
    }
    edges.sort_unstable();

    let mut active = vec![0u8; triangles.len()];
    let mut start = 0;
    while start < edges.len() {
        let key = (edges[start].0, edges[start].1);
        let mut end = start + 1;
        while end < edges.len() && (edges[end].0, edges[end].1) == key {
            end += 1;
        }
        let shared = &edges[start..end];
        if let [(_, _, t1, e1), (_, _, t2, e2)] = *shared {
            let direction = |t: u32, e: u8| {
                let indices = triangles[t as usize];
                vertices[indices[(e as usize + 1) % 3] as usize]
                    - vertices[indices[e as usize] as usize]
            };
            let (n1, n2) = (normals[t1 as usize], normals[t2 as usize]);
            if is_edge_active(n1, n2, direction(t1, e1)) {
                active[t1 as usize] |= 1 << e1;
            }
            if is_edge_active(n2, n1, direction(t2, e2)) {
                active[t2 as usize] |= 1 << e2;
            }
        } else {
            // One triangle's edge is the mesh's boundary, and three or more
            // triangles on one edge have no single neighbour to be flat with.
            for &(_, _, t, e) in shared {
                active[t as usize] |= 1 << e;
            }
        }
        start = end;
    }
    active
}

/// Whether the edge running along `direction` in the triangle of normal `n1`,
/// shared with the triangle of normal `n2`, is active — Jolt's
/// `ActiveEdges::IsEdgeActive`: back to back is active, concave is not, and
/// convex is when it bends past [`TriangleMesh::ACTIVE_EDGE_COSINE`].
fn is_edge_active(n1: DVec3, n2: DVec3, direction: DVec3) -> bool {
    let cosine = n1.dot(n2);
    if cosine < -TriangleMesh::BACK_TO_BACK_COSINE {
        return true;
    }
    if n1.cross(n2).dot(direction) < 0.0 {
        return false;
    }
    cosine < TriangleMesh::ACTIVE_EDGE_COSINE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square floor of side 2 at `y = 0`, split along its diagonal from
    /// `(-1, 0, -1)` to `(1, 0, 1)`, both triangles facing up.
    fn quad() -> (Vec<DVec3>, Vec<[u32; 3]>) {
        (
            vec![
                DVec3::new(-1.0, 0.0, -1.0),
                DVec3::new(1.0, 0.0, -1.0),
                DVec3::new(1.0, 0.0, 1.0),
                DVec3::new(-1.0, 0.0, 1.0),
            ],
            vec![[0, 3, 2], [0, 2, 1]],
        )
    }

    /// **What is refused, and which offender is named.**
    #[test]
    fn a_mesh_refuses_what_cannot_be_collided_with() {
        let (vertices, triangles) = quad();
        assert!(TriangleMesh::new(&vertices, &triangles).is_ok());
        assert_eq!(
            TriangleMesh::new(&vertices, &[]),
            Err(MeshError::NoTriangles)
        );
        let mut poisoned = vertices.clone();
        poisoned[2].y = f64::NAN;
        assert_eq!(
            TriangleMesh::new(&poisoned, &triangles),
            Err(MeshError::NonFiniteVertex { index: 2 })
        );
        poisoned[2].y = f64::INFINITY;
        assert_eq!(
            TriangleMesh::new(&poisoned, &triangles),
            Err(MeshError::NonFiniteVertex { index: 2 })
        );
        assert_eq!(
            TriangleMesh::new(&vertices, &[[0, 3, 2], [0, 2, 4]]),
            Err(MeshError::IndexOutOfRange {
                triangle: 1,
                index: 4
            })
        );
        // A repeated index, three points in a line, and a sliver a tenth of
        // a micrometre high across a metre.
        for bad in [[0, 0, 2], [0, 1, 1]] {
            assert_eq!(
                TriangleMesh::new(&vertices, &[[0, 3, 2], bad]),
                Err(MeshError::DegenerateTriangle { triangle: 1 })
            );
        }
        let line = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
        assert_eq!(
            TriangleMesh::new(&line, &[[0, 1, 2]]),
            Err(MeshError::DegenerateTriangle { triangle: 0 })
        );
        let sliver = [DVec3::ZERO, DVec3::X, DVec3::new(0.5, 1.0e-7, 0.0)];
        assert_eq!(
            TriangleMesh::new(&sliver, &[[0, 1, 2]]),
            Err(MeshError::DegenerateTriangle { triangle: 0 })
        );
        let thin = [DVec3::ZERO, DVec3::X, DVec3::new(0.5, 1.0e-5, 0.0)];
        assert!(TriangleMesh::new(&thin, &[[0, 1, 2]]).is_ok());
    }

    /// **A flat seam is inactive and the outline is active**: the diagonal
    /// of the quad is edge 2 of its first triangle (`2 → 0`) and edge 0 of its
    /// second (`0 → 2`), and every other edge is the boundary.
    #[test]
    fn a_flat_seam_is_inactive_and_the_outline_active() {
        let (vertices, triangles) = quad();
        let mesh = TriangleMesh::new(&vertices, &triangles).unwrap();
        assert_eq!(mesh.active_edges(0), 0b011);
        assert_eq!(mesh.active_edges(1), 0b110);
        assert_eq!(mesh.normal(0), DVec3::Y);
        let all = mesh.collide_with_all_edges();
        assert_eq!((all.active_edges(0), all.active_edges(1)), (0b111, 0b111));
    }

    /// **Welded by position**: the same quad written with each triangle's
    /// corners as vertices of its own still finds its seam.
    #[test]
    fn repeated_corners_weld_into_one_seam() {
        let (vertices, triangles) = quad();
        let unshared: Vec<DVec3> = triangles
            .iter()
            .flat_map(|t| t.map(|i| vertices[i as usize]))
            .collect();
        let mesh = TriangleMesh::new(&unshared, &[[0, 1, 2], [3, 4, 5]]).unwrap();
        assert_eq!(mesh.active_edges(0), 0b011);
        assert_eq!(mesh.active_edges(1), 0b110);
    }

    /// **A step's nose is active and its crease is not.** The nose, where
    /// the tread turns down into the riser below it, is convex; the riser's
    /// foot, where it meets the floor in front, is concave.
    #[test]
    fn a_stairs_nose_is_active_and_its_crease_is_not() {
        // Floor x ∈ [-1, 0] at y = 0; riser x = 0, y ∈ [0, 1], facing -x;
        // tread y = 1 over x ∈ [0, 1]; all across z ∈ [0, 1].
        let vertices = [
            DVec3::new(-1.0, 0.0, 0.0),
            DVec3::new(-1.0, 0.0, 1.0),
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(0.0, 1.0, 0.0),
            DVec3::new(0.0, 1.0, 1.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 1.0),
        ];
        let triangles = [
            [0, 1, 3],
            [0, 3, 2], // floor
            [2, 3, 5],
            [2, 5, 4], // riser
            [4, 5, 7],
            [4, 7, 6], // tread
        ];
        let mesh = TriangleMesh::new(&vertices, &triangles).unwrap();
        assert_eq!(mesh.normal(0), DVec3::Y);
        assert_eq!(mesh.normal(2), -DVec3::X);
        assert_eq!(mesh.normal(4), DVec3::Y);
        // Floor triangle 1's edge 1 (3 → 2) is the crease: concave.
        assert_eq!(mesh.active_edges(1) & 0b010, 0, "the crease");
        // Riser triangle 3's edge 1 (5 → 4) is the nose: convex, a right angle.
        assert_ne!(mesh.active_edges(3) & 0b010, 0, "the nose");
        // The tread's own seam, 7 → 4 in triangle 4 and 4 → 7 in 5, is flat.
        assert_eq!(mesh.active_edges(4) & 0b100, 0, "the tread's seam");
        assert_eq!(mesh.active_edges(5) & 0b001, 0, "the tread's seam");
    }
}
