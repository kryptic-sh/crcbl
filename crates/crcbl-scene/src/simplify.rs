//! Mesh decimation by quadric error metrics: iterative edge collapse.
//!
//! `docs/plan/25-lod.md`'s "Auto-LOD: QEM simplification" section asks for
//! "iterative edge collapse ordered by quadric error", a recorded per-level max
//! geometric error, determinism, and degenerate/flip rejection. This module is
//! that decimator and nothing else.
//!
//! # The algorithm, and where to check it
//!
//! Michael Garland and Paul S. Heckbert, *Surface Simplification Using Quadric
//! Error Metrics*, SIGGRAPH '97, pp. 209–216. The three pieces this
//! transcribes, by the paper's own section numbering:
//!
//! - **§3, the fundamental error quadric.** A plane is `p = [n d]` with `n` a
//!   unit normal and `n·v + d = 0` on the plane. Its quadric is the outer
//!   product `K_p = p pᵀ`, and for a point `v` in homogeneous form `[v 1]`,
//!   `[v 1]ᵀ K_p [v 1] = (n·v + d)²` — the squared distance from `v` to that
//!   plane. The private `Quadric` here stores the three blocks of that
//!   symmetric 4×4 separately (`A = n nᵀ`, `b = d n`, `c = d²`) so the sum
//!   reads as arithmetic rather than as index bookkeeping.
//! - **§5, per-vertex accumulation.** A vertex's quadric is the sum of the
//!   `K_p` of the planes of the faces meeting it, so `Q(v)` is the *sum of
//!   squared distances* from `v` to those planes. Quadrics are unweighted, as
//!   the paper defines them — not area-weighted, which is a later refinement
//!   from Garland's thesis and would make a hand-derived test a claim about the
//!   weighting rather than about the quadric.
//! - **§4, the collapse position.** The minimiser of `Q` is `v̄ = -A⁻¹ b`. `A`
//!   is singular whenever the accumulated planes leave a direction
//!   unconstrained — a flat region, a straight crease — and there this falls
//!   back to whichever of the two endpoints and their midpoint has the lowest
//!   error, which is the paper's own last resort.
//!
//! An edge's cost is `Q_a + Q_b` evaluated at the position that collapse would
//! put the merged vertex at, exactly as §5 defines it.
//!
//! # This is the decimator and nothing else
//!
//! [`simplify`] takes host arrays and returns host arrays. There is no cluster
//! hierarchy, no runtime LOD selection in the cull or amplification stage, no
//! screen-space error projection, no hysteresis, no shadow bias, no CLI and no
//! GPU work; nothing in the engine calls this yet. Every one of those is a
//! later slice of topic 25. Read this module as the input to that work rather
//! than as a working LOD system.
//!
//! # What is constrained, and what is not
//!
//! The plan names three constraints. Only one of them is expressible from the
//! two arrays this takes:
//!
//! - **Position borders are locked, and that is implemented.** An edge used by
//!   any number of faces other than two is a border (or a non-manifold seam),
//!   and every vertex on one is refused as either endpoint of a collapse. So an
//!   open mesh keeps its boundary loop exactly — the plan's "optional border
//!   locking (modular/tiling meshes keep exact edges)", except that it is not
//!   optional here.
//! - **UV/normal seams are NOT constrained.** A seam is a discontinuity in an
//!   attribute this function is never given, so it cannot see one. A mesh whose
//!   UV seam is split into two coincident position vertices *does* get its seam
//!   locked, but only as a side effect: the split makes both sides open in the
//!   index topology. A seam that shares positions is invisible here and will
//!   drift. This is the artifact `docs/plan/25-lod.md` calls out under Risks,
//!   and it is not addressed.
//! - **Material boundaries are NOT constrained**, for the same reason: material
//!   assignment is per primitive and never reaches this function.
//!
//! Two more absences worth naming. Vertices are compared by *index*, not by
//! position, so a mesh with duplicated coincident vertices is several disjoint
//! surfaces to this code and every shared edge of it reads as a border. And
//! skinning weights are not carried through collapses, which
//! `docs/plan/25-lod.md` makes part of the same slice as the attribute work.
//!
//! # Determinism
//!
//! The plan requires "same input hash → identical output". Nothing here
//! iterates a hash map, and the priority order is a *strict* total order rather
//! than a cost comparison that leaves ties to the heap: candidates are keyed on
//! `(cost, lower vertex, higher vertex, versions)`, with [`f64::total_cmp`]
//! doing the cost half, so two edges of equal cost always collapse in ascending
//! vertex order instead of in whatever order [`BinaryHeap`] happens to sift
//! them. The surviving vertices are then renumbered in ascending original index
//! order and the faces emitted in original face order.
//!
//! Costs are accumulated in `f64` rather than the `f32` the positions arrive
//! in. A quadric is a sum of squares of coordinates, so it loses the low half
//! of its input's mantissa before anything else happens, and the 3×3 solve then
//! runs on that; the input and the output stay `f32`.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use glam::{DMat3, DVec3};

/// How far from singular the quadric's 3×3 block must be before its inverse is
/// trusted to place the merged vertex.
///
/// The test is against the determinant a matrix of this one's *scale* would
/// have, because `A` is a sum of outer products of unit normals and its
/// determinant therefore grows as the cube of how many faces met the vertex. An
/// absolute threshold would call a large vertex's well-conditioned matrix
/// singular and a small one's degenerate matrix invertible.
const MIN_RELATIVE_DETERMINANT: f64 = 1e-10;

/// How far from collinear a face's corners must stay for a collapse that moves
/// one of them to be allowed.
///
/// Twice a triangle's area divided by its longest edge squared is the sine of
/// its narrowest corner up to a constant, so comparing against it rejects a
/// triangle that has become a sliver of no thickness whatever the mesh's scale
/// — while a face that merely got smaller, which is the normal outcome of
/// decimation, keeps its shape and passes.
const MIN_FACE_SINE: f64 = 1e-6;

/// Why a triangle list could not be simplified.
///
/// Both are caller bugs rather than data the decimator could recover from, and
/// both are refused instead of simplified. They are deliberately not
/// [`MeshletError`](crate::meshlet::MeshletError)'s variants of the same name:
/// that enum also carries a failure mode of the meshlet *record*, which this
/// module has no equivalent of, and a caller matching on one enum for two
/// unrelated builders would have unreachable arms either way.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SimplifyError {
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

/// One simplified level: a mesh, plus the error that producing it cost.
#[derive(Clone, Debug, PartialEq)]
pub struct Simplified {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    max_error: f32,
}

impl Simplified {
    /// The surviving vertices, renumbered densely in ascending original index
    /// order.
    ///
    /// A vertex the surviving triangles do not reference is dropped, including
    /// one the input never referenced either.
    #[inline]
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The surviving triangles, in the input's face order, indexing
    /// [`positions`](Self::positions).
    #[inline]
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The largest quadric error any single collapse in this level cost, in the
    /// mesh's own units of length.
    ///
    /// Precisely: the maximum over every collapse performed of
    /// `sqrt((Q_a + Q_b)(v̄))`, where `v̄` is where that collapse put the merged
    /// vertex. By §3 of the paper that quantity is the square root of the sum
    /// of squared distances from `v̄` to the planes of all the original faces
    /// folded into those two quadrics — so it bounds the distance from the new
    /// vertex to any *one* of them, and it is zero exactly when the new vertex
    /// still lies on every plane it came from (a flat region decimates for
    /// free).
    ///
    /// **It is not a certified Hausdorff bound.** It measures the new vertex
    /// against accumulated planes, not the two surfaces against each other, and
    /// the quadric is known to understate error where a surface folds back on
    /// itself. `docs/plan/25-lod.md`'s testing section wants a property test
    /// asserting the reported error dominates a sampled Hausdorff distance;
    /// that test does not exist yet and this number has not been shown to
    /// satisfy it.
    ///
    /// Zero when no collapse happened.
    #[inline]
    #[must_use]
    pub fn max_error(&self) -> f32 {
        self.max_error
    }
}

/// Decimate a triangle list to at most `target_triangles` triangles.
///
/// `positions` and `indices` are a triangle list exactly as
/// [`GltfPrimitive`](crate::GltfPrimitive) holds them; taking the two arrays
/// rather than the primitive is what lets this be driven from literals. A
/// target *count* rather than a ratio is the primitive here — the plan's
/// ~50/25/12/6% chain is the caller multiplying and rounding.
///
/// The result is deterministic: the same arrays and target produce
/// byte-identical output, on any run and in any order relative to other work.
///
/// Simplification stops early — above the target — when no remaining collapse
/// is allowed. A mesh whose every edge touches a border comes back unchanged
/// rather than damaged, and a coarse closed mesh stalls once every remaining
/// edge would fold or invert something. So does a mesh already at or below the
/// target, and one with no indices.
///
/// # Errors
///
/// [`SimplifyError::PartialTriangle`] when `indices` is not a whole number of
/// triangles, and [`SimplifyError::IndexOutOfRange`] when an index names a
/// vertex `positions` does not have. Both are checked before any collapse
/// happens, so a refused mesh produces nothing rather than a prefix.
///
/// # Panics
///
/// It does not, on any input. The two checks above are exactly what the
/// indexing below would otherwise trip on.
pub fn simplify(
    positions: &[[f32; 3]],
    indices: &[u32],
    target_triangles: usize,
) -> Result<Simplified, SimplifyError> {
    if !indices.len().is_multiple_of(3) {
        return Err(SimplifyError::PartialTriangle {
            count: indices.len(),
        });
    }
    if let Some(&index) = indices
        .iter()
        .find(|&&index| index as usize >= positions.len())
    {
        return Err(SimplifyError::IndexOutOfRange {
            index,
            vertices: positions.len(),
        });
    }

    let mut mesh = Decimator::new(positions, indices);
    let mut heap = mesh.initial_candidates();
    let mut max_cost = 0.0f64;

    while mesh.live_faces > target_triangles {
        let Some(Reverse(candidate)) = heap.pop() else {
            break;
        };
        let [a, b] = candidate.edge;
        if !mesh.is_current(&candidate) || !mesh.collapse_allowed(a, b, candidate.target) {
            continue;
        }
        mesh.collapse(a, b, candidate.target, &mut heap);
        // A maximum because that is what the metric is defined as, even though
        // the costs come out of the heap non-decreasing — see
        // `the_costs_of_the_collapses_performed_never_decrease`.
        max_cost = max_cost.max(candidate.cost);
    }

    Ok(mesh.finish(max_cost.sqrt() as f32))
}

/// A pending edge collapse, ordered by cost with every tie broken.
///
/// `versions` is what makes lazy invalidation work: a candidate is stale — its
/// cost computed against a quadric that has since absorbed another vertex's —
/// exactly when either endpoint's version has moved on since it was pushed. It
/// is part of the ordering key only so that [`Ord`] is a total order and [`Eq`]
/// agrees with it; nothing reads the order of two candidates for one edge.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    cost: f64,
    target: DVec3,
    /// The endpoints, lower index first.
    edge: [u32; 2],
    /// The endpoints' versions when this was pushed, in `edge`'s order.
    versions: [u32; 2],
}

impl Candidate {
    fn key(&self) -> (&f64, [u32; 2], [u32; 2]) {
        (&self.cost, self.edge, self.versions)
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        let (cost, edge, versions) = self.key();
        let (other_cost, other_edge, other_versions) = other.key();
        cost.total_cmp(other_cost)
            .then_with(|| edge.cmp(&other_edge))
            .then_with(|| versions.cmp(&other_versions))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

/// The fundamental error quadric of §3, as its three blocks.
///
/// `error_at(v) = v·(A v) + 2 b·v + c`, which is the homogeneous
/// `[v 1]ᵀ K [v 1]` written out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Quadric {
    /// `n nᵀ`, summed.
    a: DMat3,
    /// `d n`, summed.
    b: DVec3,
    /// `d²`, summed.
    c: f64,
}

impl Quadric {
    /// The quadric of no planes at all, which is zero everywhere.
    ///
    /// Spelled out rather than derived: `DMat3`'s [`Default`] is the *identity*
    /// matrix, so a derived `Default` would seed every vertex with the quadric
    /// of the three coordinate planes and charge every collapse the squared
    /// distance from the origin.
    const ZERO: Self = Self {
        a: DMat3::ZERO,
        b: DVec3::ZERO,
        c: 0.0,
    };

    /// `K_p` for the plane `n·v + d = 0`, whose `n` must already be a unit
    /// normal — `error_at` returns a squared *distance* only because it is.
    fn from_plane(normal: DVec3, offset: f64) -> Self {
        Self {
            a: DMat3::from_cols(normal * normal.x, normal * normal.y, normal * normal.z),
            b: normal * offset,
            c: offset * offset,
        }
    }

    /// `K_p` for the plane a triangle lies in, or `None` for a triangle with no
    /// area, which lies in no one plane and constrains nothing.
    fn from_triangle(p0: DVec3, p1: DVec3, p2: DVec3) -> Option<Self> {
        let normal = (p1 - p0).cross(p2 - p0).try_normalize()?;
        Some(Self::from_plane(normal, -normal.dot(p0)))
    }

    fn error_at(&self, v: DVec3) -> f64 {
        v.dot(self.a * v) + 2.0 * self.b.dot(v) + self.c
    }

    /// `v̄ = -A⁻¹ b`, the point of least error, or `None` when `A` is too near
    /// singular to invert — see [`MIN_RELATIVE_DETERMINANT`].
    fn minimizer(&self) -> Option<DVec3> {
        let scale = self
            .a
            .to_cols_array()
            .iter()
            .fold(0.0f64, |largest, entry| largest.max(entry.abs()));
        let determinant = self.a.determinant();
        if !determinant.is_finite() || determinant.abs() <= MIN_RELATIVE_DETERMINANT * scale.powi(3)
        {
            return None;
        }
        let v = self.a.inverse() * -self.b;
        v.is_finite().then_some(v)
    }
}

impl std::ops::Add for Quadric {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            a: self.a + other.a,
            b: self.b + other.b,
            c: self.c + other.c,
        }
    }
}

/// The mesh mid-decimation.
///
/// Faces are never moved, only vacated, so a face's slot is a stable name for
/// it and `incident` can hold slots rather than back-pointers that shift.
#[derive(Debug)]
struct Decimator {
    positions: Vec<DVec3>,
    /// `None` once a face has been collapsed away.
    faces: Vec<Option<[u32; 3]>>,
    /// Face slots touching each vertex. A superset while collapses are in
    /// flight: entries for vacated slots are dropped when the vertex is next
    /// merged into.
    incident: Vec<Vec<usize>>,
    quadrics: Vec<Quadric>,
    alive: Vec<bool>,
    /// On a border or a non-manifold edge, so never an endpoint of a collapse.
    locked: Vec<bool>,
    /// Bumped when a vertex's quadric absorbs another's, to invalidate the
    /// candidates that were costed against the old one.
    version: Vec<u32>,
    live_faces: usize,
}

impl Decimator {
    fn new(positions: &[[f32; 3]], indices: &[u32]) -> Self {
        let positions: Vec<DVec3> = positions
            .iter()
            .map(|p| DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
            .collect();
        let faces: Vec<Option<[u32; 3]>> = indices
            .chunks_exact(3)
            .map(|face| Some([face[0], face[1], face[2]]))
            .collect();

        let mut incident = vec![Vec::new(); positions.len()];
        let mut quadrics = vec![Quadric::ZERO; positions.len()];
        let mut edge_uses: BTreeMap<[u32; 2], usize> = BTreeMap::new();
        for (slot, face) in faces
            .iter()
            .enumerate()
            .filter_map(|(slot, face)| Some((slot, (*face)?)))
        {
            let corners = face.map(|index| positions[index as usize]);
            let quadric = Quadric::from_triangle(corners[0], corners[1], corners[2]);
            for corner in 0..3 {
                incident[face[corner] as usize].push(slot);
                if let Some(quadric) = quadric {
                    quadrics[face[corner] as usize] = quadrics[face[corner] as usize] + quadric;
                }
                *edge_uses
                    .entry(undirected(face[corner], face[(corner + 1) % 3]))
                    .or_default() += 1;
            }
        }

        let mut locked = vec![false; positions.len()];
        for (edge, uses) in &edge_uses {
            if *uses != 2 {
                locked[edge[0] as usize] = true;
                locked[edge[1] as usize] = true;
            }
        }

        Self {
            live_faces: faces.len(),
            alive: vec![true; positions.len()],
            version: vec![0; positions.len()],
            positions,
            faces,
            incident,
            quadrics,
            locked,
        }
    }

    /// Every collapsible edge, costed once.
    fn initial_candidates(&self) -> BinaryHeap<Reverse<Candidate>> {
        let mut edges: BTreeSet<[u32; 2]> = BTreeSet::new();
        for face in self.faces.iter().flatten() {
            for corner in 0..3 {
                edges.insert(undirected(face[corner], face[(corner + 1) % 3]));
            }
        }
        let mut heap = BinaryHeap::with_capacity(edges.len());
        for edge in edges {
            self.push_edge(&mut heap, edge[0], edge[1]);
        }
        heap
    }

    fn push_edge(&self, heap: &mut BinaryHeap<Reverse<Candidate>>, a: u32, b: u32) {
        let edge = undirected(a, b);
        let [lo, hi] = edge.map(|index| index as usize);
        if lo == hi || self.locked[lo] || self.locked[hi] || !self.alive[lo] || !self.alive[hi] {
            return;
        }
        let (target, cost) = self.collapse_target(lo, hi);
        heap.push(Reverse(Candidate {
            cost,
            target,
            edge,
            versions: [self.version[lo], self.version[hi]],
        }));
    }

    /// Where collapsing `a` and `b` puts the merged vertex, and what that
    /// costs: §4's `v̄`, or the best of the two endpoints and their midpoint
    /// when `A` will not invert.
    fn collapse_target(&self, a: usize, b: usize) -> (DVec3, f64) {
        let quadric = self.quadrics[a] + self.quadrics[b];
        let target = quadric.minimizer().unwrap_or_else(|| {
            let (pa, pb) = (self.positions[a], self.positions[b]);
            [pa, pb, (pa + pb) * 0.5]
                .into_iter()
                .map(|candidate| (candidate, quadric.error_at(candidate)))
                .reduce(|best, next| if next.1 < best.1 { next } else { best })
                .map_or(pa, |(candidate, _)| candidate)
        });
        // Rounding can carry a sum of squares a hair below zero, and a negative
        // cost would sort ahead of a genuinely free collapse.
        (target, quadric.error_at(target).max(0.0))
    }

    /// Whether a popped candidate was costed against the quadrics both its
    /// endpoints still have.
    fn is_current(&self, candidate: &Candidate) -> bool {
        let [lo, hi] = candidate.edge.map(|index| index as usize);
        self.alive[lo]
            && self.alive[hi]
            && self.version[lo] == candidate.versions[0]
            && self.version[hi] == candidate.versions[1]
    }

    /// Live face slots touching `v`, with the stale entries filtered out.
    fn faces_of(&self, v: usize) -> impl Iterator<Item = (usize, [u32; 3])> + '_ {
        self.incident[v]
            .iter()
            .filter_map(|&slot| Some((slot, self.faces[slot]?)))
    }

    /// The vertices sharing a live face edge with `v`.
    fn neighbours(&self, v: usize) -> BTreeSet<u32> {
        self.faces_of(v)
            .flat_map(|(_, face)| face)
            .filter(|&corner| corner as usize != v)
            .collect()
    }

    /// The live faces holding both `a` and `b` — two of them on a manifold
    /// interior edge, and those two are what the collapse removes.
    fn shared_faces(&self, a: usize, b: usize) -> Vec<usize> {
        self.faces_of(a)
            .filter(|(_, face)| face.contains(&(b as u32)))
            .map(|(slot, _)| slot)
            .collect()
    }

    /// Whether collapsing `a` and `b` to `target` leaves a mesh worth having.
    ///
    /// Three refusals, in the order they get cheaper to check:
    ///
    /// - The edge must be a manifold interior edge — exactly two faces. Border
    ///   locking has already excluded the one-face case, so this catches a
    ///   non-manifold edge that shares no vertex with any border.
    /// - The **link condition**: `a` and `b` must share exactly the two
    ///   neighbours opposite those two faces. A third common neighbour `c`
    ///   means two distinct edges `(a,c)` and `(b,c)` would merge into one,
    ///   which gives that edge four faces — a fold, and a closed mesh that is
    ///   no longer closed by any edge count.
    /// - No surviving face may invert or become a sliver, which is
    ///   `docs/plan/25-lod.md`'s degenerate/flip rejection.
    fn collapse_allowed(&self, a: u32, b: u32, target: DVec3) -> bool {
        let (a, b) = (a as usize, b as usize);
        let shared = self.shared_faces(a, b);
        if shared.len() != 2 {
            return false;
        }
        let common = self.neighbours(a).intersection(&self.neighbours(b)).count();
        if common != 2 {
            return false;
        }
        [a, b].into_iter().all(|v| {
            self.faces_of(v)
                .filter(|(slot, _)| !shared.contains(slot))
                .all(|(_, face)| self.survives(face, a, b, target))
        })
    }

    /// Whether one face keeps its facing and its shape when `a` and `b` move to
    /// `target`.
    ///
    /// **This is a per-collapse guard, not an invariant over the whole
    /// decimation.** Each collapse is refused if it reverses a face, but
    /// nothing stops a face turning a little under each of several collapses
    /// until it has come all the way round. That is not hypothetical: pop the
    /// heap in descending cost order instead of ascending and a face of the
    /// `spikes` height field in the tests ends up facing `-Z` with every
    /// individual collapse having passed this check. Keeping the *cheapest
    /// first* order is what keeps each step small enough for the local test to
    /// stand in for the global one.
    fn survives(&self, face: [u32; 3], a: usize, b: usize, target: DVec3) -> bool {
        let before = face.map(|index| self.positions[index as usize]);
        let after = face.map(|index| {
            if index as usize == a || index as usize == b {
                target
            } else {
                self.positions[index as usize]
            }
        });

        let old_normal = (before[1] - before[0]).cross(before[2] - before[0]);
        let new_normal = (after[1] - after[0]).cross(after[2] - after[0]);

        let longest_edge = [
            after[1] - after[0],
            after[2] - after[1],
            after[0] - after[2],
        ]
        .into_iter()
        .fold(0.0f64, |longest, edge| longest.max(edge.length_squared()));
        if new_normal.length_squared() <= (MIN_FACE_SINE * longest_edge).powi(2) {
            return false;
        }
        // A face that had no area to begin with faces nowhere, so there is no
        // orientation for the collapse to have reversed.
        old_normal.length_squared() == 0.0 || old_normal.dot(new_normal) > 0.0
    }

    /// Merge `b` into `a` at `target`, and re-cost the edges that changed.
    ///
    /// Only `a`'s quadric moves — §5 accumulates `Q_a + Q_b` into the survivor
    /// and leaves the neighbours' alone — so `a`'s version is the only one that
    /// needs bumping, and the candidates for edges that touch neither `a` nor
    /// `b` stay current.
    fn collapse(
        &mut self,
        a: u32,
        b: u32,
        target: DVec3,
        heap: &mut BinaryHeap<Reverse<Candidate>>,
    ) {
        let (a, b) = (a as usize, b as usize);
        for slot in self.shared_faces(a, b) {
            self.faces[slot] = None;
            self.live_faces -= 1;
        }

        self.positions[a] = target;
        self.quadrics[a] = self.quadrics[a] + self.quadrics[b];
        self.alive[b] = false;
        self.version[a] += 1;

        for slot in std::mem::take(&mut self.incident[b]) {
            let Some(face) = self.faces[slot].as_mut() else {
                continue;
            };
            for corner in face.iter_mut() {
                if *corner as usize == b {
                    *corner = a as u32;
                }
            }
            debug_assert!(
                face[0] != face[1] && face[1] != face[2] && face[2] != face[0],
                "the link condition should have refused a collapse that repeats a corner"
            );
            self.incident[a].push(slot);
        }
        let faces = &self.faces;
        self.incident[a].retain(|&slot| faces[slot].is_some());
        self.incident[a].sort_unstable();
        self.incident[a].dedup();

        for neighbour in self.neighbours(a) {
            self.push_edge(heap, a as u32, neighbour);
        }
    }

    /// Compact to dense arrays: surviving vertices in ascending original index
    /// order, surviving faces in original face order.
    fn finish(self, max_error: f32) -> Simplified {
        let mut used = vec![false; self.positions.len()];
        for face in self.faces.iter().flatten() {
            for &corner in face {
                used[corner as usize] = true;
            }
        }

        let mut remap = vec![0u32; self.positions.len()];
        let mut positions = Vec::new();
        for (index, position) in self.positions.iter().enumerate() {
            if used[index] {
                remap[index] = positions.len() as u32;
                positions.push([position.x as f32, position.y as f32, position.z as f32]);
            }
        }

        let indices = self
            .faces
            .iter()
            .flatten()
            .flat_map(|face| face.map(|corner| remap[corner as usize]))
            .collect();

        Simplified {
            positions,
            indices,
            max_error,
        }
    }
}

/// An edge as its two endpoints, lower index first, so the same edge reached
/// from either of its faces is the same key.
fn undirected(a: u32, b: u32) -> [u32; 2] {
    if a < b { [a, b] } else { [b, a] }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Every undirected edge of a triangle list and how many faces use it.
    fn edge_uses(indices: &[u32]) -> BTreeMap<[u32; 2], usize> {
        let mut uses = BTreeMap::new();
        for face in indices.chunks_exact(3) {
            for corner in 0..3 {
                *uses
                    .entry(undirected(face[corner], face[(corner + 1) % 3]))
                    .or_insert(0) += 1;
            }
        }
        uses
    }

    /// The positions of every vertex on a border edge, sorted by bit pattern so
    /// the comparison neither depends on vertex numbering nor invents an
    /// ordering on floats.
    fn border_positions(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[u32; 3]> {
        let mut on_border: BTreeSet<u32> = BTreeSet::new();
        for (edge, uses) in edge_uses(indices) {
            if uses != 2 {
                on_border.extend(edge);
            }
        }
        let mut bits: Vec<[u32; 3]> = on_border
            .into_iter()
            .map(|vertex| positions[vertex as usize].map(f32::to_bits))
            .collect();
        bits.sort_unstable();
        bits
    }

    /// A closed torus as a `major` × `minor` grid of quads, wrapped both ways.
    ///
    /// A torus rather than a sphere because it needs no poles: every vertex has
    /// the same valence, every edge is used by exactly two faces, and there is
    /// no seam — so "closed" is a property of the construction rather than of
    /// my index arithmetic being right at two singular points.
    ///
    /// `pub(crate)` because [`crate::lod`] builds its chains out of the same
    /// fixture: a curved closed surface is what makes a level's error nonzero,
    /// and a second copy of this would be a second torus to keep closed.
    pub(crate) fn torus(major: usize, minor: usize) -> (Vec<[f32; 3]>, Vec<u32>) {
        let (big, small) = (2.0f32, 0.75f32);
        let mut positions = Vec::new();
        for i in 0..major {
            let u = std::f32::consts::TAU * i as f32 / major as f32;
            for j in 0..minor {
                let v = std::f32::consts::TAU * j as f32 / minor as f32;
                let ring = big + small * v.cos();
                positions.push([ring * u.cos(), ring * u.sin(), small * v.sin()]);
            }
        }
        let vertex = |i: usize, j: usize| ((i % major) * minor + (j % minor)) as u32;
        let mut indices = Vec::new();
        for i in 0..major {
            for j in 0..minor {
                let (a, b) = (vertex(i, j), vertex(i + 1, j));
                let (c, d) = (vertex(i + 1, j + 1), vertex(i, j + 1));
                indices.extend([a, b, c, a, c, d]);
            }
        }
        (positions, indices)
    }

    /// A `side` × `side` grid of quads over `[0, side]²` with `height` giving
    /// the third coordinate, every face wound counter-clockwise seen from `+Z`.
    ///
    /// Open on all four sides, so its outer ring is a border by the definition
    /// this module locks on, and a height field, so every face's normal has a
    /// positive `z` — which is what makes an inverted face detectable in the
    /// output without tracking which input face it came from.
    ///
    /// `pub(crate)` because [`crate::lod`] needs a mesh whose coarsest level
    /// stalls above its target, and a locked border is what stalls one.
    pub(crate) fn height_field(
        side: usize,
        height: fn(f32, f32) -> f32,
    ) -> (Vec<[f32; 3]>, Vec<u32>) {
        let stride = side + 1;
        let positions = (0..stride)
            .flat_map(|y| {
                (0..stride).map(move |x| {
                    let (x, y) = (x as f32, y as f32);
                    [x, y, height(x, y)]
                })
            })
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

    /// Adversarial on purpose. Both wavelengths are close to two grid steps and
    /// neither divides it, so the sampled surface is a field of spikes with no
    /// two neighbouring vertices on the same slope — which is what it took to
    /// find collapses the flip guard has to refuse. A gentler field
    /// (`0.9 sin(2x) sin(2y)`, tried first) never produces one at any target
    /// down to an eighth, and its test passes with the guard deleted.
    pub(crate) fn spikes(x: f32, y: f32) -> f32 {
        3.0 * (3.1 * x).sin() * (2.9 * y).cos()
    }

    /// A closed tetrahedron on the origin and the three unit axis points, wound
    /// outward.
    ///
    /// Every face lies in a plane whose quadric can be written down: `x = 0`,
    /// `y = 0`, `z = 0`, and `x + y + z = 1`.
    fn unit_tetrahedron() -> (Vec<[f32; 3]>, Vec<u32>) {
        (
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        )
    }

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn quadric_of(positions: &[[f32; 3]], indices: &[u32], vertex: usize) -> Quadric {
        Decimator::new(positions, indices).quadrics[vertex]
    }

    /// §3: `[v 1]ᵀ K_p [v 1]` is the squared distance from `v` to `p`'s plane.
    ///
    /// Both planes and both distances are written down from the definition,
    /// not read off a run.
    #[test]
    fn a_plane_quadric_evaluates_to_the_squared_distance_to_that_plane() {
        // The z = 0 plane, from a triangle in it wound counter-clockwise.
        let flat =
            Quadric::from_triangle(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0)).unwrap();
        assert_eq!(flat.a, DMat3::from_cols(DVec3::ZERO, DVec3::ZERO, DVec3::Z));
        assert_eq!(flat.b, DVec3::ZERO);
        assert_eq!(flat.c, 0.0);
        assert_eq!(
            flat.error_at(v(5.0, -3.0, 2.0)),
            4.0,
            "the point is 2 above the z = 0 plane, wherever it is in x and y"
        );
        assert_eq!(flat.error_at(v(5.0, -3.0, 0.0)), 0.0);

        // The x + y + z = 1 plane: unit normal (1,1,1)/sqrt(3), offset
        // -1/sqrt(3). The origin is 1/sqrt(3) from it, so its squared distance
        // is 1/3.
        let slanted =
            Quadric::from_triangle(v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0), v(0.0, 0.0, 1.0)).unwrap();
        assert!(
            (slanted.error_at(DVec3::ZERO) - 1.0 / 3.0).abs() < 1e-15,
            "got {}",
            slanted.error_at(DVec3::ZERO)
        );
        assert!(
            slanted.error_at(v(1.0, 0.0, 0.0)).abs() < 1e-15,
            "a point on the plane is at no distance from it, got {}",
            slanted.error_at(v(1.0, 0.0, 0.0))
        );
        // Two units along the unit normal is two units away.
        let along = DVec3::splat(2.0 / 3.0f64.sqrt()) + v(1.0, 0.0, 0.0);
        assert!(
            (slanted.error_at(along) - 4.0).abs() < 1e-14,
            "got {}",
            slanted.error_at(along)
        );

        assert!(
            Quadric::from_triangle(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0)).is_none(),
            "collinear corners lie in no one plane"
        );
    }

    /// §4: `v̄ = -A⁻¹ b`, on a sum whose minimum is arithmetic on paper.
    ///
    /// Summing the quadrics of `x = 0`, `y = 0`, `z = 0` and `x = 1` gives
    /// `f(v) = x² + y² + z² + (x - 1)²`, whose gradient vanishes at
    /// `y = z = 0`, `2x + 2(x - 1) = 0` — so `v̄ = (1/2, 0, 0)` and
    /// `f(v̄) = 1/4 + 1/4 = 1/2`.
    #[test]
    fn the_minimizer_of_four_known_planes_is_the_point_the_definition_gives() {
        let quadric = [
            Quadric::from_plane(DVec3::X, 0.0),
            Quadric::from_plane(DVec3::Y, 0.0),
            Quadric::from_plane(DVec3::Z, 0.0),
            Quadric::from_plane(DVec3::X, -1.0),
        ]
        .into_iter()
        .reduce(|sum, next| sum + next)
        .unwrap();

        let minimizer = quadric.minimizer().expect("four planes span three axes");
        assert!(
            (minimizer - v(0.5, 0.0, 0.0)).length() < 1e-15,
            "got {minimizer}"
        );
        assert!(
            (quadric.error_at(minimizer) - 0.5).abs() < 1e-15,
            "got {}",
            quadric.error_at(minimizer)
        );
    }

    /// The plane the quadrics come from is a real face rather than a literal,
    /// so this also checks that `from_triangle` orients and offsets the plane
    /// the way `from_plane` is tested to.
    #[test]
    fn a_quadric_summed_from_faces_matches_one_summed_from_their_planes() {
        let (positions, indices) = unit_tetrahedron();
        let origin = quadric_of(&positions, &indices, 0);

        // The three faces at the origin lie in x = 0, y = 0 and z = 0, so the
        // sum is the squared distance to the origin itself.
        for point in [v(0.3, -1.2, 4.0), v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0)] {
            assert!(
                (origin.error_at(point) - point.length_squared()).abs() < 1e-15,
                "at {point}, got {}",
                origin.error_at(point)
            );
        }
    }

    /// §5's edge cost on a mesh small enough to minimise on paper.
    ///
    /// Collapsing the tetrahedron's `(0,0,0)`–`(1,0,0)` edge sums
    /// `Q_0 = x² + y² + z²` (planes `x = 0`, `y = 0`, `z = 0`) with
    /// `Q_1 = y² + z² + s²/3` where `s = x + y + z - 1` (planes `y = 0`,
    /// `z = 0`, `x + y + z = 1`), giving
    /// `f(v) = x² + 2y² + 2z² + s²/3`.
    ///
    /// Setting the gradient to zero: `2x + 2s/3 = 0` and `4y + 2s/3 = 0` and
    /// `4z + 2s/3 = 0`, so `x = -s/3` and `y = z = -s/6`. Substituting into
    /// `s = x + y + z - 1` gives `s = -2s/3 - 1`, i.e. `s = -3/5`. So
    /// `v̄ = (1/5, 1/10, 1/10)` and
    /// `f(v̄) = 1/25 + 2/100 + 2/100 + (9/25)/3 = 1/5`.
    #[test]
    fn collapsing_a_known_tetrahedrons_edge_costs_what_the_definition_gives() {
        let (positions, indices) = unit_tetrahedron();
        let mesh = Decimator::new(&positions, &indices);

        let (target, cost) = mesh.collapse_target(0, 1);

        assert!(
            (target - v(0.2, 0.1, 0.1)).length() < 1e-14,
            "the quadric-optimal position, got {target}"
        );
        assert!(
            (cost - 0.2).abs() < 1e-14,
            "the summed quadric at that position, got {cost}"
        );
    }

    /// The singular case §4's fallback exists for: two faces meeting along the
    /// x axis constrain `y` and `z` and leave `x` free, so `A` has rank two.
    #[test]
    fn a_singular_quadric_falls_back_to_an_endpoint_or_the_midpoint() {
        // The z = 0 and y = 0 planes, which every point on the x axis lies on.
        let quadric = Quadric::from_plane(DVec3::Z, 0.0) + Quadric::from_plane(DVec3::Y, 0.0);
        assert!(
            quadric.minimizer().is_none(),
            "two planes cannot pin three axes"
        );

        // A hinge: two triangles sharing the x-axis edge 0–1, one in each plane.
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let indices = [0, 1, 2, 0, 3, 1];
        let mesh = Decimator::new(&positions, &indices);

        let (target, cost) = mesh.collapse_target(0, 1);

        assert!(
            cost < 1e-30,
            "every point on the x axis is free, got {cost}"
        );
        assert!(
            [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.5, 0.0, 0.0)].contains(&target),
            "the fallback picks one of the endpoints or the midpoint, got {target}"
        );
    }

    /// The case [`MIN_RELATIVE_DETERMINANT`] exists for, as opposed to the
    /// exactly-singular one above: `x = 0`, `y = 0`, and a third plane whose
    /// normal is a whisker off `Y`, so `z` is *nearly* unconstrained.
    ///
    /// With `n₃ = (0, c, s)` and `s ≈ 1e-6`, the accumulated `A` is
    /// `[[1,0,0], [0,1+c²,cs], [0,cs,s²]]`, whose determinant is `s²` — small
    /// but not zero. Its inverse is finite, so a guard that only tested for a
    /// NaN would accept it, and solving gives `v̄ = (0, 0, 1/s)`: a vertex a
    /// million units off a mesh whose planes all pass within one unit of the
    /// origin.
    #[test]
    fn a_nearly_singular_quadric_is_refused_before_its_inverse_flies_off() {
        let grazing = v(0.0, 1.0, 1e-6).normalize();
        let quadric = Quadric::from_plane(DVec3::X, 0.0)
            + Quadric::from_plane(DVec3::Y, 0.0)
            + Quadric::from_plane(grazing, -1.0);

        let escaped = quadric.a.inverse() * -quadric.b;
        assert!(
            quadric.a.determinant() > 0.0 && quadric.a.inverse().is_finite(),
            "this has to be an invertible matrix for refusing it to mean anything"
        );
        assert!(
            escaped.z > 1e5,
            "the derivation says 1/s, about 1e6 — got {escaped}"
        );
        assert!(
            quadric.minimizer().is_none(),
            "the relative-determinant test is what refuses this; without it the \
             merged vertex goes to {escaped}"
        );

        // The other half: the same three planes at a healthy angle do invert.
        let healthy = v(0.0, 1.0, 1.0).normalize();
        let sound = Quadric::from_plane(DVec3::X, 0.0)
            + Quadric::from_plane(DVec3::Y, 0.0)
            + Quadric::from_plane(healthy, -1.0);
        assert!(
            sound.minimizer().is_some(),
            "the threshold must not be refusing every three-plane vertex"
        );
    }

    /// No two distinct candidates compare equal, so the pop order is decided
    /// entirely by the data.
    ///
    /// Without this, two equally-costed edges are indistinguishable to the
    /// heap and which one collapses first falls to [`BinaryHeap`]'s sift order
    /// — reproducible for one build of one standard library, and exactly the
    /// kind of thing a bake cache keyed on a content hash must not depend on.
    /// A test rather than an inference because it cannot be observed from the
    /// outside: within a single process the heap is deterministic either way.
    #[test]
    fn two_collapses_of_equal_cost_are_still_strictly_ordered() {
        let edge = Candidate {
            cost: 1.5,
            target: DVec3::ZERO,
            edge: [3, 9],
            versions: [0, 0],
        };
        let cheaper_edge = Candidate {
            edge: [3, 8],
            target: DVec3::ONE,
            ..edge
        };
        let older = Candidate {
            versions: [1, 0],
            ..edge
        };

        assert_eq!(edge.cmp(&edge), Ordering::Equal, "a candidate is itself");
        assert_eq!(
            cheaper_edge.cmp(&edge),
            Ordering::Less,
            "the same cost breaks to the lower endpoints, whatever the target"
        );
        assert_eq!(
            edge.cmp(&older),
            Ordering::Less,
            "and the same endpoints break to the versions, so no two entries \
             in the heap are ever indistinguishable"
        );
        assert_ne!(edge, cheaper_edge);
        assert_ne!(edge, older);
    }

    /// A candidate costed before either endpoint absorbed another vertex is
    /// refused when it comes back off the heap, and one whose endpoints have
    /// not moved is not.
    ///
    /// Accepting a stale one is not a structural failure — the mesh stays
    /// closed and unflipped, because the guards run at collapse time — so
    /// nothing observable from outside catches it. What it corrupts is the
    /// position and the reported cost, which would be the ones for a quadric
    /// two faces out of date.
    #[test]
    fn a_candidate_costed_before_its_endpoint_grew_is_stale() {
        let (positions, indices) = torus(8, 6);
        let mut mesh = Decimator::new(&positions, &indices);
        let mut heap = mesh.initial_candidates();
        let mut before: Vec<Candidate> = heap.iter().map(|entry| entry.0).collect();
        before.sort_unstable();

        let Some(Reverse(collapsed)) = heap.pop() else {
            unreachable!("a torus has edges")
        };
        let [a, b] = collapsed.edge;
        assert!(mesh.collapse_allowed(a, b, collapsed.target));
        mesh.collapse(a, b, collapsed.target, &mut heap);

        let touching = before
            .iter()
            .find(|candidate| candidate.edge != collapsed.edge && candidate.edge.contains(&a))
            .expect("the merged vertex had other edges");
        let disjoint = before
            .iter()
            .find(|candidate| !candidate.edge.contains(&a) && !candidate.edge.contains(&b))
            .expect("a torus has edges nowhere near this one");

        assert!(
            !mesh.is_current(touching),
            "{:?} was costed against the quadric the merged vertex has since \
             outgrown",
            touching.edge
        );
        assert!(
            mesh.is_current(disjoint),
            "{:?} touches neither endpoint, so re-costing it would be wasted \
             work",
            disjoint.edge
        );

        let [lo, hi] = touching.edge.map(|index| index as usize);
        let (_, recosted) = mesh.collapse_target(lo, hi);
        assert!(
            recosted > touching.cost,
            "the stale cost {} understates the real one {recosted}, which is \
             what makes accepting it wrong",
            touching.cost
        );
    }

    /// Why [`Simplified::max_error`] taking a maximum and taking the *last*
    /// collapse's cost come to the same thing here.
    ///
    /// The heap pops the cheapest live candidate, and a collapse only ever adds
    /// one positive semi-definite quadric to another, so a candidate created
    /// after a collapse costs at least what that collapse did. The popped costs
    /// are therefore non-decreasing. Written as a test rather than a comment
    /// because it is a claim about the priority order, which is the part of
    /// this module a change could quietly break.
    #[test]
    fn the_costs_of_the_collapses_performed_never_decrease() {
        let (positions, indices) = torus(8, 6);
        let mut mesh = Decimator::new(&positions, &indices);
        let mut heap = mesh.initial_candidates();
        let mut costs = Vec::new();

        while mesh.live_faces > indices.len() / 3 / 4 {
            let Some(Reverse(candidate)) = heap.pop() else {
                break;
            };
            let [a, b] = candidate.edge;
            if !mesh.is_current(&candidate) || !mesh.collapse_allowed(a, b, candidate.target) {
                continue;
            }
            mesh.collapse(a, b, candidate.target, &mut heap);
            costs.push(candidate.cost);
        }

        assert!(
            costs.len() > 10,
            "only {} collapses to look at",
            costs.len()
        );
        assert!(costs.windows(2).all(|pair| pair[0] <= pair[1]), "{costs:?}");
        assert!(
            costs[0] < costs[costs.len() - 1],
            "every collapse cost the same, so the ordering is not being exercised"
        );
    }

    /// A quarter of a coarse torus, because that is where the link condition
    /// starts having to refuse collapses: three of them here, and deleting the
    /// check leaves an edge with four faces on it. Half of a fine torus needs
    /// no refusals at all and passes with the check deleted.
    #[test]
    fn a_closed_torus_simplified_to_a_target_is_still_closed() {
        let (positions, indices) = torus(8, 6);
        assert!(
            edge_uses(&indices).values().all(|&uses| uses == 2),
            "the fixture has to be closed for the result to say anything"
        );
        let target = indices.len() / 3 / 4;

        let level = simplify(&positions, &indices, target).unwrap();

        assert_eq!(
            level.indices().len() / 3,
            target,
            "the decimation has to have happened for 'still closed' to mean anything"
        );
        for (edge, uses) in edge_uses(level.indices()) {
            assert_eq!(uses, 2, "edge {edge:?} bounds a hole or a fold");
        }
        assert!(
            level.max_error() > 0.0,
            "a curved surface cannot be decimated for free"
        );
        assert!(level.max_error().is_finite());
    }

    #[test]
    fn an_open_patch_keeps_every_vertex_on_its_border() {
        let (positions, indices) = height_field(10, spikes);
        let before = border_positions(&positions, &indices);
        assert_eq!(before.len(), 40, "the outer ring of an 11 x 11 grid");
        let target = indices.len() / 3 * 3 / 4;

        let level = simplify(&positions, &indices, target).unwrap();

        assert_eq!(
            level.indices().len() / 3,
            target,
            "the decimation has to have happened for 'border kept' to mean anything"
        );
        assert_eq!(
            border_positions(level.positions(), level.indices()),
            before,
            "the boundary loop moved, gained or lost a vertex"
        );
    }

    /// The predicate, on a face whose moving corner crosses the opposite edge.
    ///
    /// `(0,0,0), (1,0,0), (0,1,0)` is wound counter-clockwise, so its normal is
    /// `+Z`. Moving the first corner to `(1,1,0)` puts it on the far side of
    /// the `x + y = 1` line through the other two, and the cross product of
    /// `(0,-1,0)` and `(-1,0,0)` is `-Z`.
    #[test]
    fn a_collapse_that_would_invert_a_face_is_refused() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mesh = Decimator::new(&positions, &[0, 1, 2]);

        assert!(
            mesh.survives([0, 1, 2], 0, 0, v(0.2, 0.2, 0.0)),
            "a corner that stays on its own side keeps the face"
        );
        assert!(
            !mesh.survives([0, 1, 2], 0, 0, v(1.0, 1.0, 0.0)),
            "the corner crossed the opposite edge, so the normal reversed"
        );
        assert!(
            !mesh.survives([0, 1, 2], 0, 0, v(0.5, 0.5, 0.0)),
            "the corner landed on the opposite edge, so the face has no area"
        );
        assert!(
            !mesh.survives([0, 1, 2], 0, 0, v(-1.0, 2.0, 0.0)),
            "collinear with the opposite edge however far along it"
        );

        // The sliver the facing test alone would let through: a nanometre
        // inside the x + y = 1 line, so the normal still points at +Z and the
        // dot product is still positive — but the corner is 1e-9/sqrt(2) from
        // the opposite edge, giving a face of area 5e-10 across a base of
        // sqrt(2). Only MIN_FACE_SINE refuses it.
        let sliver = v(0.5, 0.5 - 1e-9, 0.0);
        let normal = (v(1.0, 0.0, 0.0) - sliver).cross(v(0.0, 1.0, 0.0) - sliver);
        assert!(
            normal.z > 0.0,
            "the sliver has to keep its facing for this to isolate the shape \
             test, got {normal}"
        );
        assert!(
            !mesh.survives([0, 1, 2], 0, 0, sliver),
            "a face of no thickness is degenerate however it faces"
        );
    }

    /// And the same guardrail seen from outside: every face of a height field
    /// has a normal with a positive `z`, so a face in the output that does not
    /// is one the decimation turned over.
    ///
    /// The size and the target are both load-bearing — this is the smallest of
    /// the [`spikes`] fields at which deleting [`Decimator::survives`] actually
    /// turns three faces over. Note that this passing is not proof that no
    /// sequence of accepted collapses can invert a face; see that function.
    #[test]
    fn simplifying_a_height_field_leaves_every_face_facing_the_way_it_did() {
        let (positions, indices) = height_field(16, spikes);
        let target = indices.len() / 3 / 4;

        let level = simplify(&positions, &indices, target).unwrap();

        assert_eq!(level.indices().len() / 3, target);
        for face in level.indices().chunks_exact(3) {
            let corners = [0, 1, 2]
                .map(|corner| DVec3::from(level.positions()[face[corner] as usize].map(f64::from)));
            let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
            assert!(
                normal.z > 0.0,
                "face {face:?} at {corners:?} faces away from +Z"
            );
        }
    }

    #[test]
    fn the_same_mesh_simplified_twice_is_identical() {
        let (positions, indices) = torus(16, 10);
        let target = indices.len() / 3 / 2;

        let first = simplify(&positions, &indices, target).unwrap();
        let second = simplify(&positions, &indices, target).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.positions(), second.positions());
        assert_eq!(first.indices(), second.indices());
        assert_eq!(first.max_error().to_bits(), second.max_error().to_bits());
        // The half that stops a decimator which returns its input from passing:
        // the output is smaller than the input, and it tracks the target.
        assert_eq!(first.indices().len() / 3, target);
        assert!(first.indices().len() < indices.len());
        assert!(first.positions().len() < positions.len());
    }

    /// Decimating further runs the same collapses and then more of them, so the
    /// worst collapse of the coarser level cannot be cheaper than the finer
    /// level's.
    #[test]
    fn a_coarser_target_gives_a_different_mesh_and_at_least_as_much_error() {
        let (positions, indices) = torus(16, 10);
        let triangles = indices.len() / 3;

        let fine = simplify(&positions, &indices, triangles * 3 / 4).unwrap();
        let coarse = simplify(&positions, &indices, triangles / 4).unwrap();

        assert_eq!(fine.indices().len() / 3, triangles * 3 / 4);
        assert_eq!(coarse.indices().len() / 3, triangles / 4);
        assert_ne!(fine.indices(), coarse.indices());
        assert!(
            coarse.max_error() >= fine.max_error(),
            "{} is not at least {}",
            coarse.max_error(),
            fine.max_error()
        );
        assert!(fine.max_error() > 0.0);
    }

    #[test]
    fn a_target_at_or_above_the_input_leaves_the_mesh_alone() {
        let (positions, indices) = torus(8, 6);
        let triangles = indices.len() / 3;

        let level = simplify(&positions, &indices, triangles).unwrap();

        assert_eq!(level.positions(), positions);
        assert_eq!(level.indices(), indices);
        assert_eq!(level.max_error(), 0.0, "no collapse happened");
    }

    #[test]
    fn a_mesh_whose_every_vertex_is_on_a_border_is_returned_unchanged() {
        // Two triangles sharing an edge: every one of the four vertices is on
        // the outer boundary, so no collapse has an unlocked pair.
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let indices = [0, 1, 2, 0, 2, 3];

        let level = simplify(&positions, &indices, 1).unwrap();

        assert_eq!(level.indices(), indices, "asked for 1 triangle, kept 2");
        assert_eq!(level.max_error(), 0.0);
    }

    #[test]
    fn a_mesh_with_no_indices_yields_nothing() {
        let level = simplify(&[], &[], 0).unwrap();

        assert!(level.positions().is_empty());
        assert!(level.indices().is_empty());
        assert_eq!(level.max_error(), 0.0);
    }

    #[test]
    fn an_index_buffer_that_is_not_whole_triangles_is_refused() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

        assert_eq!(
            simplify(&positions, &[0, 1, 2, 0], 1),
            Err(SimplifyError::PartialTriangle { count: 4 })
        );
    }

    #[test]
    fn an_index_naming_a_vertex_the_mesh_does_not_have_is_refused() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

        assert_eq!(
            simplify(&positions, &[0, 1, 3], 1),
            Err(SimplifyError::IndexOutOfRange {
                index: 3,
                vertices: 3,
            }),
            "refused whole, rather than simplified up to the bad index"
        );
    }
}
