//! The cluster DAG: group, lock, simplify, re-split, repeat.
//!
//! `docs/plan/25-lod.md`'s "The cluster DAG" section replaced the chain with
//! this, in the shape Nanite uses (Brian Karis, *Nanite: A Deep Dive*, SIGGRAPH
//! 2021 Advances in Real-Time Rendering course). [`build_cluster_dag`] is that
//! build:
//!
//! 1. **Cluster** the base mesh with [`build_meshlets`]. Those are the leaves.
//! 2. **Group** neighbouring clusters by partitioning the cluster adjacency
//!    graph, where two clusters are adjacent when they **share an edge**.
//!    Adjacency, not proximity — two clusters that nearly touch across a gap
//!    share no edge and are never grouped.
//! 3. **Lock each group's outer boundary** and simplify. The internal cluster
//!    boundaries dissolve, so real simplification happens; the outer boundary is
//!    preserved vertex for vertex, so the group still meets its neighbours.
//! 4. **Re-split** each simplified group into fresh clusters — the parents of
//!    every cluster that went into the group.
//! 5. **Repeat.** The next level is grouped from scratch, so an edge locked at
//!    one level lands inside a group at the next and finally gets to simplify.
//!
//! A group has several children and several parents, which is what makes this a
//! DAG rather than a tree.
//!
//! # Why the locked-edge set is the whole thing
//!
//! A group's outer boundary is *interior* to the level's mesh: every edge of it
//! has two faces, one inside the group and one outside. No rule over a position
//! array and an index buffer can pick it out, which is why
//! [`simplify_with_locked_edges`] exists and why the level is handed to it in
//! **one call** with every group's boundary locked, rather than group by group.
//! Simplifying each group as a mesh of its own would put its boundary on a
//! topological border and get it locked for free — and would leave the
//! locked-edge parameter doing nothing, while splitting the level's vertices
//! into a copy per group that the next level's adjacency could no longer see
//! through.
//!
//! One call also means collapses are ordered by cost across the whole level
//! rather than restarted per group, and a collapse can never cross a group
//! boundary: both endpoints of one are locked, so only vertices interior to a
//! single group ever move. Every surviving triangle therefore belongs to
//! exactly the group its input triangle did, which is what
//! [`Simplified::source_faces`](crate::simplify::Simplified) records and what
//! the re-split reads.
//!
//! # Why every cut is crack-free
//!
//! Selection picks a **cut**: a set of clusters covering the surface exactly
//! once, formed by expanding some groups (draw their children) and not others
//! (draw their parents). Wherever two levels meet across a cut, that boundary
//! was a group boundary in the coarser level, and step 3 kept it exactly — so
//! the two sides share their boundary vertices by construction, and the cut has
//! no hole in it. That is the property this module exists for, and `tests`
//! asserts it over every cut the DAG admits — including the ones that draw
//! several levels at once, since a uniform cut is a chain level and a chain was
//! never the thing that cracked.
//!
//! # This is the builder and nothing else
//!
//! [`build_cluster_dag`] takes host arrays and returns host arrays. There is no
//! GPU descent in the amplification stage, no screen-space error projection, no
//! hysteresis, no shadow bias, no bake cache and no upload; nothing in the
//! engine calls this yet. Selection — including the rule that turns an error and
//! a pixel budget into a cut — is the next slice. Read this as its input.
//!
//! [`mod@crate::lod`] remains the per-instance chain builder. This subsumes it
//! in principle (a uniform cut *is* a chain level), but nothing has been moved
//! onto it yet.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::meshlet::{MeshletBuild, MeshletError, build_meshlets};
use crate::simplify::{SimplifyError, simplify_with_locked_edges, undirected};

/// How many clusters one group aims to hold.
///
/// A group has to be big enough that dissolving its internal cluster
/// boundaries frees more geometry than locking its outer boundary freezes — a
/// group of one is a cluster with every edge locked and simplifies nearly
/// nothing — and small enough that its outer boundary stays a short polyline.
/// Halving a group's triangles then leaves about half as many clusters as went
/// in, which is what makes each level of the DAG roughly half the width of the
/// one below.
const GROUP_TARGET_CLUSTERS: usize = 4;

/// What a level asks the decimator for, as a divisor of the triangle count of
/// the level below it.
///
/// `docs/plan/25-lod.md` step 3: "simplify its interior to roughly half its
/// triangles". Roughly, because the locked boundaries can stall the decimation
/// above the target — a group whose every edge touches its own boundary keeps
/// what it has, and the level is still built.
const LEVEL_TRIANGLE_DIVISOR: usize = 2;

/// Why a cluster DAG could not be built.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ClusterDagError {
    /// A level could not be decimated.
    #[error(transparent)]
    Simplify(#[from] SimplifyError),

    /// A level could not be clustered.
    #[error(transparent)]
    Cluster(#[from] MeshletError),
}

/// A group of neighbouring clusters, and the clusters simplifying it produced.
///
/// The DAG's edges live here rather than on a cluster: grouping is what relates
/// two levels, and a cluster's parents are *the group's* parents — all of them,
/// not one each. That is the "several children, several parents" that makes
/// this a DAG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterGroup {
    children: Vec<u32>,
    parents: Vec<u32>,
}

impl ClusterGroup {
    /// The clusters that were grouped, as indices into
    /// [`DagLevel::clusters`](DagLevel::clusters) of the level this group
    /// belongs to. Ascending, and never empty.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[u32] {
        &self.children
    }

    /// The clusters re-splitting the simplified group produced, as indices into
    /// the clusters of the level **above** this group's.
    ///
    /// Ascending and contiguous — a group's parents are emitted together — and
    /// every cluster of the level above belongs to exactly one group's parents.
    #[inline]
    #[must_use]
    pub fn parents(&self) -> &[u32] {
        &self.parents
    }
}

/// One level of the DAG: a mesh, its clusters, their errors, and the grouping
/// that produced the level above.
#[derive(Clone, Debug, PartialEq)]
pub struct DagLevel {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    clusters: MeshletBuild,
    errors: Vec<f32>,
    groups: Vec<ClusterGroup>,
}

impl DagLevel {
    /// This level's vertices. Level 0's are the base mesh's verbatim.
    #[inline]
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// This level's triangle list, indexing [`positions`](Self::positions), in
    /// cluster order — so it is exactly what [`clusters`](Self::clusters)
    /// decodes to.
    #[inline]
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// This level's clusters, built over this level's own geometry.
    ///
    /// Above level 0 they are built one group at a time and concatenated, so no
    /// cluster straddles two groups.
    #[inline]
    #[must_use]
    pub fn clusters(&self) -> &MeshletBuild {
        &self.clusters
    }

    /// Per cluster, how far its surface may depart from the base mesh's, in the
    /// mesh's own units of length. Parallel to
    /// [`clusters().clusters()`](Self::clusters).
    ///
    /// The worst collapse charged to any vertex of any cluster the group
    /// produced, raised to at least the largest error of any cluster that went
    /// into the group. Level 0 is the base mesh untouched and reads zero
    /// throughout.
    ///
    /// **Every cluster a group produced carries the group's number, not one of
    /// its own.** That is not a shortcut: a group simplifies as a unit, so its
    /// parents stand or fall together, and a cut that drew one of them while
    /// descending into another would tear along a boundary the group never
    /// locked. Detail still varies across a level, because different groups
    /// report different errors — what does not vary is detail *within* one
    /// group.
    ///
    /// **Monotonic up the DAG by construction**, because selection needs it to
    /// be: a cut is well defined only when no cluster can be drawn while an
    /// ancestor covering it is also drawn, and that follows from a parent's
    /// error never being below its children's. Folding the group's children
    /// into the maximum is what enforces it — a group that simplifies an easy
    /// region cheaply would otherwise report less than the clusters it replaced.
    ///
    /// What the number does *not* claim is set out on
    /// [`Simplified::max_error`](crate::Simplified::max_error): it is a quadric
    /// error and has not been shown to dominate a sampled Hausdorff distance.
    #[inline]
    #[must_use]
    pub fn errors(&self) -> &[f32] {
        &self.errors
    }

    /// How this level's clusters were grouped to build the level above.
    ///
    /// Every cluster of this level is in exactly one group. Empty on the top
    /// level, which has no level above it.
    #[inline]
    #[must_use]
    pub fn groups(&self) -> &[ClusterGroup] {
        &self.groups
    }
}

/// A mesh as a DAG of clusters: level 0 is the base, each level above it the
/// simplified re-split of the one below.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterDag {
    levels: Vec<DagLevel>,
}

impl ClusterDag {
    /// The levels, finest first. Never empty — level 0 is always the base mesh,
    /// even when nothing above it could be built.
    #[inline]
    #[must_use]
    pub fn levels(&self) -> &[DagLevel] {
        &self.levels
    }
}

/// Build a mesh's cluster DAG.
///
/// `positions` and `indices` are a triangle list exactly as
/// [`GltfPrimitive`](crate::GltfPrimitive) holds them, and become level 0
/// verbatim. Levels are added until one of them fails to hold fewer clusters
/// than the level below it, which is the point at which grouping has stopped
/// buying anything; a mesh of a single cluster gets level 0 alone.
///
/// The result is deterministic: the same arrays produce an identical DAG,
/// because every step it composes is — the grouping walks clusters in index
/// order and breaks weight ties on the lower index, and both builders it calls
/// are deterministic in their own right.
///
/// # Errors
///
/// [`ClusterDagError::Cluster`] and [`ClusterDagError::Simplify`] carry the two
/// builders' refusals of the arrays: a partial triangle or an index outside the
/// mesh, either of which is caught by the first clustering of the base.
///
/// # Panics
///
/// It does not, on any input.
pub fn build_cluster_dag(
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Result<ClusterDag, ClusterDagError> {
    let clusters = build_meshlets(positions, indices)?;
    let mut levels = vec![DagLevel {
        errors: vec![0.0; clusters.clusters().len()],
        clusters,
        positions: positions.to_vec(),
        indices: indices.to_vec(),
        groups: Vec::new(),
    }];

    while let Some((groups, above)) = coarsen(levels.last().expect("level 0 is always there"))? {
        levels.last_mut().expect("level 0 is always there").groups = groups;
        levels.push(above);
    }

    Ok(ClusterDag { levels })
}

/// One group–lock–simplify–resplit round, or `None` when it bought nothing.
///
/// Returns `below`'s grouping alongside the level it produced, because the two
/// only mean anything together: the grouping names cluster indices in the new
/// level.
fn coarsen(below: &DagLevel) -> Result<Option<(Vec<ClusterGroup>, DagLevel)>, ClusterDagError> {
    let cluster_count = below.clusters.clusters().len();
    if cluster_count <= 1 {
        return Ok(None);
    }
    let mut groups = group_clusters(below);

    // The level's faces, laid out group by group, and which group each went
    // into. Grouping is over clusters and a group's clusters need not be
    // adjacent in the index buffer, so this is a reordering and not a slicing.
    let mut ordered = Vec::with_capacity(below.indices.len());
    let mut face_group = Vec::with_capacity(below.indices.len() / 3);
    for (group, members) in groups.iter().enumerate() {
        for &cluster in &members.children {
            let faces = below.clusters.cluster_indices(cluster as usize);
            face_group.extend(std::iter::repeat_n(group, faces.len() / 3));
            ordered.extend_from_slice(&faces);
        }
    }

    let locked = group_boundary(&ordered, &face_group);
    let simplified = simplify_with_locked_edges(
        &below.positions,
        &ordered,
        ordered.len() / 3 / LEVEL_TRIANGLE_DIVISOR,
        &locked,
    )?;

    // Split the survivors back into the groups their input triangles were in.
    let mut faces_of = vec![Vec::new(); groups.len()];
    for (face, &source) in simplified
        .indices()
        .chunks_exact(3)
        .zip(simplified.source_faces())
    {
        faces_of[face_group[source as usize]].extend_from_slice(face);
    }

    let mut clusters = MeshletBuild::empty();
    let mut errors = Vec::new();
    let mut indices = Vec::new();
    for (group, faces) in groups.iter_mut().zip(&faces_of) {
        let split = build_meshlets(simplified.positions(), faces)?;
        let first = clusters.clusters().len();
        clusters.append(&split)?;
        let error = (first..clusters.clusters().len())
            .map(|parent| cluster_error(&clusters, parent, simplified.vertex_errors()))
            .chain(
                group
                    .children
                    .iter()
                    .map(|&child| below.errors[child as usize]),
            )
            .fold(0.0f32, f32::max);
        for parent in first..clusters.clusters().len() {
            errors.push(error);
            group.parents.push(parent as u32);
        }
        indices.extend_from_slice(faces);
    }

    if clusters.clusters().len() >= cluster_count {
        return Ok(None);
    }
    Ok(Some((
        groups,
        DagLevel {
            positions: simplified.positions().to_vec(),
            indices,
            clusters,
            errors,
            groups: Vec::new(),
        },
    )))
}

/// The worst error charged to any vertex one cluster references.
fn cluster_error(clusters: &MeshletBuild, cluster: usize, vertex_errors: &[f32]) -> f32 {
    let record = clusters.clusters()[cluster];
    clusters.vertices()[record.vertex_offset as usize..][..record.vertex_count as usize]
        .iter()
        .map(|&vertex| vertex_errors[vertex as usize])
        .fold(0.0f32, f32::max)
}

/// Every edge whose two faces are in different groups.
///
/// This is the set that has to be locked, and the reason it cannot be inferred:
/// each of these edges has exactly two faces, so it is interior to the mesh and
/// indistinguishable from any other interior edge without knowing the grouping.
/// The mesh's own borders are not in here and do not need to be — the decimator
/// locks those itself.
fn group_boundary(indices: &[u32], face_group: &[usize]) -> Vec<[u32; 2]> {
    let mut owner: BTreeMap<[u32; 2], usize> = BTreeMap::new();
    let mut shared: BTreeSet<[u32; 2]> = BTreeSet::new();
    for (face, &group) in indices.chunks_exact(3).zip(face_group) {
        for corner in 0..3 {
            let edge = undirected(face[corner], face[(corner + 1) % 3]);
            match owner.get(&edge) {
                None => {
                    owner.insert(edge, group);
                }
                Some(&first) if first != group => {
                    shared.insert(edge);
                }
                Some(_) => {}
            }
        }
    }
    shared.into_iter().collect()
}

/// Partition a level's clusters into groups of neighbours.
///
/// Greedy and deterministic: seed from the lowest ungrouped cluster, then
/// repeatedly take the ungrouped cluster sharing the most edges with the group
/// so far, breaking ties on the lower index, until the group is
/// [`GROUP_TARGET_CLUSTERS`] wide or has no ungrouped neighbour left. A seed
/// that could not grow at all joins its best-connected neighbour's group
/// instead of standing alone, because a group of one locks every edge it has
/// and would simplify nothing. A cluster with no neighbours at all — a
/// disconnected shell — is the one case that does stand alone.
///
/// This is a graph partition and a poor one: it optimises nothing globally
/// where METIS-class partitioners minimise the edge cut, so a group's boundary
/// is longer than it needs to be and less geometry is free to move. Its virtue
/// is that it is a partition of the *adjacency* graph and not of space, which
/// is what step 2 requires, and that it has no dependency.
fn group_clusters(level: &DagLevel) -> Vec<ClusterGroup> {
    let adjacency = cluster_adjacency(level);
    let mut group_of: Vec<Option<usize>> = vec![None; adjacency.len()];
    let mut groups: Vec<ClusterGroup> = Vec::new();

    for seed in 0..adjacency.len() {
        if group_of[seed].is_some() {
            continue;
        }
        let index = groups.len();
        group_of[seed] = Some(index);
        let mut children = vec![seed];
        while children.len() < GROUP_TARGET_CLUSTERS {
            let Some(next) = best_ungrouped_neighbour(&adjacency, &children, &group_of) else {
                break;
            };
            group_of[next] = Some(index);
            children.push(next);
        }

        if children.len() == 1
            && let Some((_, host)) = adjacency[seed]
                .iter()
                .filter_map(|(&other, &weight)| Some((weight, group_of[other]?)))
                .max_by_key(|&(weight, host)| (weight, Reverse(host)))
        {
            group_of[seed] = Some(host);
            groups[host].children.push(seed as u32);
            groups[host].children.sort_unstable();
            continue;
        }

        children.sort_unstable();
        groups.push(ClusterGroup {
            children: children.into_iter().map(|cluster| cluster as u32).collect(),
            parents: Vec::new(),
        });
    }

    groups
}

/// The ungrouped cluster sharing the most edges with the clusters gathered so
/// far, or `None` when they have no ungrouped neighbour.
fn best_ungrouped_neighbour(
    adjacency: &[BTreeMap<usize, usize>],
    gathered: &[usize],
    group_of: &[Option<usize>],
) -> Option<usize> {
    let mut shared: BTreeMap<usize, usize> = BTreeMap::new();
    for &member in gathered {
        for (&other, &edges) in &adjacency[member] {
            if group_of[other].is_none() {
                *shared.entry(other).or_default() += edges;
            }
        }
    }
    shared
        .into_iter()
        .max_by_key(|&(cluster, edges)| (edges, Reverse(cluster)))
        .map(|(cluster, _)| cluster)
}

/// How many edges each pair of clusters shares.
///
/// **Shared edges, not nearby bounds.** Two clusters on either side of a gap
/// have bounding spheres that may overlap and no edge in common, and grouping
/// them would lock a boundary that is not one and simplify across a hole.
fn cluster_adjacency(level: &DagLevel) -> Vec<BTreeMap<usize, usize>> {
    let count = level.clusters.clusters().len();
    let mut users: BTreeMap<[u32; 2], Vec<usize>> = BTreeMap::new();
    for cluster in 0..count {
        for face in level.clusters.cluster_indices(cluster).chunks_exact(3) {
            for corner in 0..3 {
                let edge = undirected(face[corner], face[(corner + 1) % 3]);
                let sharing = users.entry(edge).or_default();
                if !sharing.contains(&cluster) {
                    sharing.push(cluster);
                }
            }
        }
    }

    let mut adjacency = vec![BTreeMap::new(); count];
    for sharing in users.values() {
        for (at, &a) in sharing.iter().enumerate() {
            for &b in &sharing[at + 1..] {
                *adjacency[a].entry(b).or_default() += 1;
                *adjacency[b].entry(a).or_default() += 1;
            }
        }
    }
    adjacency
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meshlet::tests::{cluster_triangles, decoded, triangles_of};
    use crate::simplify::tests::{dunes, height_field};

    /// How wide the dense fixture is. Big enough that level 0 clusters into
    /// tens of meshlets — a handful cannot be grouped into anything — and small
    /// enough that a whole DAG is a fraction of a second.
    const DENSE_SIDE: usize = 32;

    /// An edge named by where its endpoints *are* rather than by which vertex
    /// they were, so it is the same edge on either side of a level change.
    ///
    /// Two levels have unrelated vertex numbering and unrelated index buffers,
    /// so an index pair cannot say whether they meet. A bit pattern can, and
    /// exactly: a vertex the decimator never moved comes through `f32` → `f64` →
    /// `f32` unchanged, so the coarser level's copy of a locked boundary vertex
    /// is bit-identical to the finer level's. Comparing bits rather than
    /// distances is what makes "they meet" mean *meet*, with no tolerance to
    /// tune.
    type SharedEdge = [[u32; 3]; 2];

    fn shared_edge(positions: &[[f32; 3]], a: u32, b: u32) -> SharedEdge {
        let mut edge = [positions[a as usize], positions[b as usize]].map(|p| p.map(f32::to_bits));
        edge.sort_unstable();
        edge
    }

    /// The dense fixture's DAG.
    fn dense_dag() -> (Vec<[f32; 3]>, Vec<u32>, ClusterDag) {
        let (positions, indices) = height_field(DENSE_SIDE, dunes);
        let dag = build_cluster_dag(&positions, &indices).unwrap();
        (positions, indices, dag)
    }

    /// Asserts the DAG is not the trivial one — the same geometry re-clustered
    /// at every level — which every other invariant here is satisfied by. A
    /// builder that never simplifies produces perfect cuts, perfect coverage,
    /// monotonic error and perfect determinism.
    fn assert_levels_got_coarser(dag: &ClusterDag) {
        let triangles: Vec<usize> = dag
            .levels()
            .iter()
            .map(|level| level.indices().len() / 3)
            .collect();
        let clusters: Vec<usize> = dag
            .levels()
            .iter()
            .map(|level| level.clusters().clusters().len())
            .collect();
        assert!(triangles.len() > 2, "a DAG of {triangles:?} proves nothing");
        for pair in triangles.windows(2) {
            assert!(pair[1] < pair[0], "triangles {triangles:?} do not shrink");
        }
        for pair in clusters.windows(2) {
            assert!(pair[1] < pair[0], "clusters {clusters:?} do not shrink");
        }
    }

    /// The clusters a global error threshold draws.
    ///
    /// `docs/plan/25-lod.md`'s descent, written host-side: a cluster is drawn
    /// when the group that *produced* it is within the budget and the group that
    /// *contains* it is not — descend while a group's error exceeds the
    /// threshold, stop when it does not. [`DagLevel::errors`] is the producing
    /// group's error, and the containing group's is the error its parents carry;
    /// a top-level cluster has no containing group and is treated as having an
    /// infinite one, so a large enough threshold draws the whole top level.
    ///
    /// Monotonic error is what makes this a cut: along the groups covering any
    /// one region the errors never decrease, so the test "produced-by within,
    /// containing without" holds at exactly one of them.
    fn cut(dag: &ClusterDag, threshold: f32) -> Vec<(usize, usize)> {
        let mut drawn = Vec::new();
        for (level, below) in dag.levels().iter().enumerate() {
            let mut containing = vec![f32::INFINITY; below.clusters().clusters().len()];
            for group in below.groups() {
                let above = dag.levels()[level + 1].errors();
                let error = above[group.parents()[0] as usize];
                for &child in group.children() {
                    containing[child as usize] = error;
                }
            }
            for (cluster, &produced_by) in below.errors().iter().enumerate() {
                if produced_by <= threshold && threshold < containing[cluster] {
                    drawn.push((level, cluster));
                }
            }
        }
        drawn
    }

    /// Every threshold at which the cut changes, plus one past the last, so a
    /// sweep visits every distinct cut the DAG admits.
    fn every_threshold(dag: &ClusterDag) -> Vec<f32> {
        let mut errors: Vec<f32> = dag
            .levels()
            .iter()
            .flat_map(|level| level.errors().iter().copied())
            .collect();
        errors.sort_by(f32::total_cmp);
        errors.dedup();
        let last = *errors.last().expect("level 0 has clusters");
        errors
            .windows(2)
            .map(|pair| 0.5 * (pair[0] + pair[1]))
            .chain([last + 1.0])
            .collect()
    }

    /// Every edge of a cut's triangles, by position, and the levels of the
    /// clusters that carry it.
    fn cut_edges(dag: &ClusterDag, drawn: &[(usize, usize)]) -> BTreeMap<SharedEdge, Vec<usize>> {
        let mut edges: BTreeMap<SharedEdge, Vec<usize>> = BTreeMap::new();
        for &(level, cluster) in drawn {
            let positions = dag.levels()[level].positions();
            for face in cluster_triangles(dag.levels()[level].clusters(), cluster) {
                for corner in 0..3 {
                    let edge = shared_edge(positions, face[corner], face[(corner + 1) % 3]);
                    edges.entry(edge).or_default().push(level);
                }
            }
        }
        edges
    }

    /// The base mesh's own boundary loop, by position: the edges a cut is
    /// *supposed* to leave with one face.
    fn base_border(positions: &[[f32; 3]], indices: &[u32]) -> BTreeSet<SharedEdge> {
        let mut uses: BTreeMap<SharedEdge, usize> = BTreeMap::new();
        for face in indices.chunks_exact(3) {
            for corner in 0..3 {
                *uses
                    .entry(shared_edge(positions, face[corner], face[(corner + 1) % 3]))
                    .or_default() += 1;
            }
        }
        uses.into_iter()
            .filter(|&(_, uses)| uses != 2)
            .map(|(edge, _)| edge)
            .collect()
    }

    /// Asserts a cut is a crack-free cover of the surface, and returns how many
    /// of its edges have a different level on either side.
    ///
    /// One check does both halves, and it is the strongest form of either.
    /// Every edge of the drawn triangles, keyed by the *positions* of its
    /// endpoints, must be used exactly twice — except the base mesh's own
    /// border, used once. An edge used once anywhere else is a hole: two
    /// clusters that should have met did not, because the coarser one moved the
    /// vertices. An edge used three times or more is an overlap. And the two
    /// sides of an interface edge only land on the same key at all if the
    /// coarser level kept the finer level's vertices exactly.
    fn assert_cut_is_a_crack_free_cover(
        dag: &ClusterDag,
        drawn: &[(usize, usize)],
        border: &BTreeSet<SharedEdge>,
    ) -> usize {
        let edges = cut_edges(dag, drawn);
        let single: BTreeSet<SharedEdge> = edges
            .iter()
            .filter(|&(_, levels)| levels.len() == 1)
            .map(|(&edge, _)| edge)
            .collect();
        assert_eq!(
            single, *border,
            "the cut's one-sided edges are not the base mesh's border, so it \
             has a hole or does not cover the whole surface"
        );
        assert!(
            edges.values().all(|levels| levels.len() <= 2),
            "an edge with three faces on it is two clusters overlapping"
        );
        edges
            .values()
            .filter(|levels| levels.len() == 2 && levels[0] != levels[1])
            .count()
    }

    #[test]
    fn a_dense_mesh_becomes_a_dag_that_gets_coarser_at_every_level() {
        let (positions, indices, dag) = dense_dag();

        assert_eq!(indices.len() / 3, 2048, "the fixture's triangles");
        assert_eq!(
            dag.levels()[0].positions(),
            positions,
            "level 0 is verbatim"
        );
        assert_eq!(dag.levels()[0].indices(), indices);
        assert_eq!(
            dag.levels()
                .iter()
                .map(|level| level.clusters().clusters().len())
                .collect::<Vec<_>>(),
            [34, 18, 10, 6, 5, 3],
            "tens of clusters at the leaves is what makes grouping possible"
        );
        assert_eq!(
            dag.levels()
                .iter()
                .map(|level| level.indices().len() / 3)
                .collect::<Vec<_>>(),
            [2048, 1024, 512, 272, 206, 128]
        );
        assert_levels_got_coarser(&dag);
        assert!(
            dag.levels()
                .last()
                .expect("a DAG has levels")
                .groups()
                .is_empty(),
            "the top level has nothing above it to group into"
        );
    }

    /// The property the DAG exists for, over every cut it admits — not one
    /// hand-picked cut, and emphatically not only the uniform ones. A uniform
    /// cut is a chain level, and a chain was never what cracked.
    #[test]
    fn every_cut_is_a_crack_free_cover_of_the_surface() {
        let (positions, indices, dag) = dense_dag();
        assert_levels_got_coarser(&dag);
        let border = base_border(&positions, &indices);
        assert_eq!(border.len(), 4 * DENSE_SIDE, "the patch's outer ring");

        let mut mixed = 0;
        let mut deepest = 0;
        for threshold in every_threshold(&dag) {
            let drawn = cut(&dag, threshold);
            let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
            let levels: BTreeSet<usize> = drawn.iter().map(|&(level, _)| level).collect();
            if levels.len() > 1 {
                mixed += 1;
                assert!(
                    interfaces > 0,
                    "a cut spanning {levels:?} whose levels never meet is not \
                     a mixed cut, it is two meshes"
                );
            }
            deepest = deepest.max(levels.len());
        }

        assert!(
            mixed > 5,
            "only {mixed} of the thresholds gave a cut of more than one level, \
             so this is mostly testing uniform cuts"
        );
        assert!(
            deepest >= 3,
            "no threshold drew three levels at once, so the deepest interface \
             this checked was one level to the next"
        );
    }

    /// The same property stated the way it fails: two levels meet along a
    /// boundary, and the coarser one has to have kept the finer one's vertices
    /// exactly. Separated out from the sweep because it is what a reader is
    /// looking for, and because it pins that the interface is *large* rather
    /// than a stray edge or two.
    #[test]
    fn where_two_levels_meet_across_a_cut_their_vertices_coincide() {
        let (positions, indices, dag) = dense_dag();
        assert_levels_got_coarser(&dag);
        // Inside the spread of level 0's group errors, so some groups are still
        // drawn as their four leaf clusters and their neighbours are already
        // drawn as the two clusters that replaced them.
        let threshold = 0.93;
        let drawn = cut(&dag, threshold);

        let levels: BTreeSet<usize> = drawn.iter().map(|&(level, _)| level).collect();
        assert_eq!(levels, BTreeSet::from([0, 1]), "the cut has to be mixed");
        let triangles: usize = drawn
            .iter()
            .map(|&(level, cluster)| {
                dag.levels()[level].clusters().clusters()[cluster].triangle_count as usize
            })
            .sum();
        assert!(
            (dag.levels()[1].indices().len() / 3..indices.len() / 3).contains(&triangles),
            "{triangles} triangles is not between the two levels this mixes, \
             so it is a uniform cut wearing a mixture's clothes"
        );

        let edges = cut_edges(&dag, &drawn);
        let interface: Vec<&SharedEdge> = edges
            .iter()
            .filter(|&(_, levels)| levels == &vec![0, 1])
            .map(|(edge, _)| edge)
            .collect();
        assert!(
            interface.len() > 20,
            "only {} edges have a leaf cluster on one side and a parent on the \
             other, which is too few to be the boundary between the two",
            interface.len()
        );
        // Every one of those is an edge whose endpoints came out of two
        // different decimated meshes at bit-identical positions; had the level
        // above moved either of them, the two sides would key differently and
        // each would read as a one-sided edge instead.
        let border = base_border(&positions, &indices);
        for edge in interface {
            assert!(!border.contains(edge), "an interface edge is on the border");
        }
        assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
    }

    /// The invariant that makes a cut well defined: a cluster is never drawn
    /// while an ancestor covering it is also drawn, which follows from a
    /// parent's error never dipping below its children's.
    #[test]
    fn the_error_never_decreases_up_the_dag() {
        let (_, _, dag) = dense_dag();
        assert_levels_got_coarser(&dag);

        assert!(
            dag.levels()[0].errors().iter().all(|&error| error == 0.0),
            "level 0 is the base mesh and cost nothing"
        );
        for (level, below) in dag.levels().iter().enumerate() {
            let above = dag.levels().get(level + 1);
            for group in below.groups() {
                let above = above.expect("a level with groups has a level above");
                let parents: Vec<f32> = group
                    .parents()
                    .iter()
                    .map(|&parent| above.errors()[parent as usize])
                    .collect();
                assert!(
                    parents.windows(2).all(|pair| pair[0] == pair[1]),
                    "a group's parents report {parents:?} rather than one error"
                );
                for &child in group.children() {
                    assert!(
                        parents[0] >= below.errors()[child as usize],
                        "a parent at level {} reports {}, below the {} its \
                         child at level {level} already cost",
                        level + 1,
                        parents[0],
                        below.errors()[child as usize],
                    );
                }
            }
            assert!(below.errors().iter().all(|error| error.is_finite()));
        }

        let coarsest = dag
            .levels()
            .last()
            .expect("a DAG has levels")
            .errors()
            .iter()
            .fold(0.0f32, |worst, &error| worst.max(error));
        let finest = dag.levels()[1]
            .errors()
            .iter()
            .fold(0.0f32, |worst, &error| worst.max(error));
        assert!(
            coarsest > finest && finest > 0.0,
            "the top of the DAG cost {coarsest} against level 1's {finest}, so \
             'never decreases' is being satisfied by nothing happening"
        );
        let spread: BTreeSet<u32> = dag.levels()[1]
            .errors()
            .iter()
            .map(|e| e.to_bits())
            .collect();
        assert!(
            spread.len() > 1,
            "every group of level 0 reports the same error, so a threshold can \
             never cut between them and no cut is ever mixed"
        );
    }

    #[test]
    fn the_same_mesh_twice_gives_an_identical_dag() {
        let (positions, indices) = height_field(DENSE_SIDE, dunes);

        let first = build_cluster_dag(&positions, &indices).unwrap();
        let second = build_cluster_dag(&positions, &indices).unwrap();

        assert_levels_got_coarser(&first);
        assert_eq!(first, second);
    }

    /// [`build_meshlets`]' own decode invariant, per level, plus the structural
    /// half: a level's clusters have to name that level's own vertices, and
    /// each has to belong to exactly one group on each side.
    #[test]
    fn every_cluster_decodes_to_its_own_level_and_sits_in_exactly_one_group() {
        let (_, _, dag) = dense_dag();
        assert_levels_got_coarser(&dag);

        for (index, level) in dag.levels().iter().enumerate() {
            assert_eq!(
                decoded(level.clusters()),
                triangles_of(level.indices()),
                "level {index}'s clusters do not decode to its own triangles"
            );
            assert_eq!(level.errors().len(), level.clusters().clusters().len());

            let mut grouped: Vec<u32> = level
                .groups()
                .iter()
                .flat_map(|group| group.children().iter().copied())
                .collect();
            grouped.sort_unstable();
            if index + 1 < dag.levels().len() {
                assert_eq!(
                    grouped,
                    (0..level.clusters().clusters().len() as u32).collect::<Vec<_>>(),
                    "level {index}'s clusters are not each in exactly one group"
                );
                let mut produced: Vec<u32> = level
                    .groups()
                    .iter()
                    .flat_map(|group| group.parents().iter().copied())
                    .collect();
                produced.sort_unstable();
                assert_eq!(
                    produced,
                    (0..dag.levels()[index + 1].clusters().clusters().len() as u32)
                        .collect::<Vec<_>>(),
                    "level {} has a cluster no group produced",
                    index + 1
                );
            }
        }
    }

    /// Grouping partitions the *adjacency graph*, so every group has to be
    /// connected in it: reachable from any of its clusters to any other by
    /// steps across shared edges. A grouping by bounding-sphere proximity would
    /// satisfy every other assertion here and fail this one.
    #[test]
    fn every_group_is_connected_through_shared_edges() {
        let (_, _, dag) = dense_dag();
        assert_levels_got_coarser(&dag);

        let mut multi = 0;
        for (index, level) in dag.levels().iter().enumerate() {
            let adjacency = cluster_adjacency(level);
            for group in level.groups() {
                let members: BTreeSet<u32> = group.children().iter().copied().collect();
                let mut reached = BTreeSet::from([group.children()[0]]);
                let mut frontier = vec![group.children()[0]];
                while let Some(cluster) = frontier.pop() {
                    for &neighbour in adjacency[cluster as usize].keys() {
                        let neighbour = neighbour as u32;
                        if members.contains(&neighbour) && reached.insert(neighbour) {
                            frontier.push(neighbour);
                        }
                    }
                }
                assert_eq!(
                    reached,
                    members,
                    "level {index}'s group {:?} is not one connected piece",
                    group.children()
                );
                multi += usize::from(members.len() > 1);
            }
        }
        assert!(
            multi > 5,
            "only {multi} groups hold more than one cluster, so connectedness \
             is mostly being satisfied by groups of one"
        );
    }

    /// Two sheets a hair apart: every cluster of one has clusters of the other
    /// well inside its bounding sphere, and shares no edge with any of them.
    /// Grouping on proximity would merge them and simplify across the gap;
    /// grouping on shared edges cannot.
    #[test]
    fn clusters_that_only_nearly_touch_are_never_grouped() {
        // A size at which the greedy clustering happens to close a cluster
        // exactly where the first sheet ends. It does not in general — the walk
        // follows index order and knows nothing about connected components, so
        // at most sizes one cluster holds triangles from both sheets and the
        // two are then genuinely edge-adjacent through it. That is a weakness
        // of `build_meshlets`, which its own docs record, and the assertion
        // below is what stops this test quietly becoming vacuous under it.
        let (sheet, faces) = height_field(15, dunes);
        let positions: Vec<[f32; 3]> = sheet
            .iter()
            .copied()
            .chain(sheet.iter().map(|&[x, y, z]| [x, y, z + 0.01]))
            .collect();
        let offset = sheet.len() as u32;
        let indices: Vec<u32> = faces
            .iter()
            .copied()
            .chain(faces.iter().map(|index| index + offset))
            .collect();

        let dag = build_cluster_dag(&positions, &indices).unwrap();

        let level = &dag.levels()[0];
        let sheet_of = |cluster: usize| {
            let corners = level.clusters().cluster_indices(cluster);
            let sheets: BTreeSet<bool> = corners.iter().map(|&corner| corner < offset).collect();
            assert_eq!(
                sheets.len(),
                1,
                "cluster {cluster} holds triangles from both sheets, so they \
                 share an edge through it and this test says nothing"
            );
            corners[0] < offset
        };
        let sheets: BTreeSet<bool> = (0..level.clusters().clusters().len())
            .map(sheet_of)
            .collect();
        assert_eq!(sheets.len(), 2, "the fixture has to cluster both sheets");

        for group in level.groups() {
            let spanned: BTreeSet<bool> = group
                .children()
                .iter()
                .map(|&child| sheet_of(child as usize))
                .collect();
            assert_eq!(
                spanned.len(),
                1,
                "group {:?} spans both sheets, so it was grouped by proximity",
                group.children()
            );
        }
        // And the reason it cannot: the two sheets are two components of the
        // adjacency graph, however close together they sit.
        let adjacency = cluster_adjacency(level);
        for (cluster, neighbours) in adjacency.iter().enumerate() {
            for &neighbour in neighbours.keys() {
                assert_eq!(
                    sheet_of(cluster),
                    sheet_of(neighbour),
                    "clusters {cluster} and {neighbour} are on different sheets \
                     and share an edge"
                );
            }
        }
    }

    #[test]
    fn a_mesh_that_clusters_into_one_is_a_dag_of_one_level() {
        let (positions, indices) = height_field(4, dunes);

        let dag = build_cluster_dag(&positions, &indices).unwrap();

        assert_eq!(dag.levels()[0].clusters().clusters().len(), 1);
        assert_eq!(dag.levels().len(), 1, "nothing to group, so nothing above");
        assert!(dag.levels()[0].groups().is_empty());
    }

    #[test]
    fn a_mesh_with_no_triangles_is_a_dag_of_one_empty_level() {
        let dag = build_cluster_dag(&[], &[]).unwrap();

        assert_eq!(dag.levels().len(), 1);
        assert!(dag.levels()[0].indices().is_empty());
        assert!(dag.levels()[0].clusters().clusters().is_empty());
        assert!(dag.levels()[0].errors().is_empty());
    }

    #[test]
    fn an_index_buffer_that_is_not_whole_triangles_is_refused() {
        assert_eq!(
            build_cluster_dag(&[[0.0; 3]], &[0, 0]),
            Err(ClusterDagError::Cluster(MeshletError::PartialTriangle {
                count: 2
            })),
            "the base mesh's clustering is the first thing to read the arrays"
        );
    }

    #[test]
    fn an_index_outside_the_mesh_is_refused() {
        assert_eq!(
            build_cluster_dag(&[[0.0; 3]], &[0, 0, 7]),
            Err(ClusterDagError::Cluster(MeshletError::IndexOutOfRange {
                index: 7,
                vertices: 1,
            }))
        );
    }
}
