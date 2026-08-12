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
//! # How a camera turns this into a cut
//!
//! A group is the unit of decision, not a cluster. Each carries one
//! [`error`](ClusterGroup::error) and one [`bounds`](ClusterGroup::bounds), and
//! [`ClusterGroup::projected_error`] turns the pair into the pixels the group's
//! simplification would cost from a given eye. Write `E(G)` for
//! `G.projected_error(eye, ppu) > budget` — *this group is expanded*, meaning
//! its children are drawn rather than its parents. A cluster is drawn exactly
//! when
//!
//! ```text
//! !E(the group that produced it) && E(the group that contains it)
//! ```
//!
//! reading a level-0 cluster's absent producer as never expanded and a top-level
//! cluster's absent container as always expanded. That is the descent of
//! `docs/plan/25-lod.md`'s "Runtime selection", written as a local test one
//! cluster at a time — which is what lets a GPU evaluate it per cluster with no
//! communication.
//!
//! # Why the decision is per group and not per cluster
//!
//! Both halves of that test name a **group's** error and a **group's** sphere,
//! never the cluster's own. Every cluster a group produced therefore evaluates a
//! bit-identical `E(G)`, so a group's parents are drawn all together or not at
//! all — and a cut can never draw one of them while descending into another,
//! which would tear along a boundary that group never locked.
//!
//! A per-cluster distance term is exactly what breaks this. Two clusters of one
//! group have different centres, so scaling one shared error by each cluster's
//! own distance gives two different answers to one question, and the two
//! clusters split across a boundary that was simplified as a unit. That is a
//! crack, and it is invisible until a camera happens to sit at the distance
//! where the two answers differ.
//!
//! # Why the sphere makes the cut well defined
//!
//! For the descent to reach exactly one level on every part of the surface,
//! `E` has to be monotone up the DAG: `E(the producer) ⟹ E(the container)` for
//! every cluster. Monotone *stored* error is not enough once a distance divides
//! it — a closer group projects larger from a smaller number. So each group's
//! sphere is built to **contain the spheres of every group below it**, alongside
//! the error being built to dominate theirs. A containing sphere is never
//! further from the eye than a sphere inside it, so the quotient
//! `error / distance` rises up the DAG for every eye position there is, and the
//! descent has one stopping point per branch whatever the camera does.
//! `enclosing` is where the containment is established and
//! `a_group_is_never_cheaper_or_further_than_the_groups_below_it` is what holds
//! it.
//!
//! # This is the builder and the metric, and nothing else
//!
//! [`build_cluster_dag`] takes host arrays and returns host arrays, and
//! [`ClusterGroup::projected_error`] is arithmetic over two of its fields. There
//! is no GPU descent in the amplification stage, no hysteresis, no shadow bias,
//! no bake cache and no upload; nothing in the engine calls any of it yet. The
//! upload and the amplification-stage descent are the next slice, and the rule
//! they have to transcribe is the one stated above.
//!
//! [`mod@crate::lod`] remains the per-instance chain builder. This subsumes it
//! in principle (a uniform cut *is* a chain level), but nothing has been moved
//! onto it yet.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec3, Vec3};

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

/// The sphere a group's error is projected from.
///
/// Separate from [`ClusterBounds`](crate::ClusterBounds), which a cluster
/// carries for culling: that one bounds one cluster's own geometry and is as
/// tight as the builder can make it, where this one bounds a whole group's and
/// is deliberately grown to contain every sphere below it in the DAG. The two
/// answer different questions and only one of them may be used to decide a
/// level.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupBounds {
    center: [f32; 3],
    radius: f32,
}

impl GroupBounds {
    /// Centre of the sphere, in the mesh's own space.
    #[inline]
    #[must_use]
    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    /// Distance from [`center`](Self::center) to the furthest point the sphere
    /// has to contain. Never negative.
    #[inline]
    #[must_use]
    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// Whether this sphere contains `inner` whole.
    ///
    /// The relation the descent depends on, so it is a function rather than an
    /// assertion written out twice: a sphere containing another is never
    /// further from any eye than the one it contains, which is what carries
    /// monotonic error into monotonic *projected* error.
    #[inline]
    #[must_use]
    pub fn contains(&self, inner: Self) -> bool {
        let separation = f64::from(Vec3::from(self.center).distance(Vec3::from(inner.center)));
        separation + f64::from(inner.radius) <= f64::from(self.radius)
    }
}

/// A sphere containing every one of `parts`, which is never empty.
///
/// The centre is the midpoint of the parts' common AABB and the radius is the
/// furthest any part reaches from it — a valid bound rather than the minimal
/// one, on [`ClusterBounds`](crate::ClusterBounds)' terms exactly, and for the
/// same reason: Ritter's and Welzl's are tighter and neither was worth
/// transcribing for a first cut. It is also order-independent, which a
/// pairwise-merge formulation would not be.
///
/// **The radius is rounded away from zero**, and that is what makes
/// [`GroupBounds::contains`] hold for every part rather than nearly hold. The
/// maximum is taken in `f64`, where the error of a `sqrt` and two additions is
/// some sixteen decimal digits down; narrowing it to `f32` can lose half an ulp
/// — seven digits down — so one [`f32::next_up`] more than covers the gap the
/// narrowing opened. Without it a part can sit a rounding step outside the
/// sphere that is supposed to contain it, and the descent loses monotonicity at
/// exactly the distance where the two are indistinguishable.
fn enclosing(parts: &[GroupBounds]) -> GroupBounds {
    let mut low = DVec3::splat(f64::INFINITY);
    let mut high = DVec3::splat(f64::NEG_INFINITY);
    for part in parts {
        let center = DVec3::new(
            f64::from(part.center[0]),
            f64::from(part.center[1]),
            f64::from(part.center[2]),
        );
        low = low.min(center - f64::from(part.radius));
        high = high.max(center + f64::from(part.radius));
    }

    let center = ((low + high) * 0.5).as_vec3().to_array();
    let midpoint = DVec3::new(
        f64::from(center[0]),
        f64::from(center[1]),
        f64::from(center[2]),
    );
    let reach = parts
        .iter()
        .map(|part| {
            let offset = DVec3::new(
                f64::from(part.center[0]),
                f64::from(part.center[1]),
                f64::from(part.center[2]),
            ) - midpoint;
            offset.length() + f64::from(part.radius)
        })
        .fold(0.0f64, f64::max);

    let bounds = GroupBounds {
        center,
        radius: (reach as f32).next_up(),
    };
    // The postcondition the descent rests on, checked where it is established
    // rather than only over one fixture's DAG: every mesh anyone builds a
    // hierarchy for gets it in a debug build, and a rounding rule that stopped
    // being enough would fail here instead of surfacing as a crack.
    debug_assert!(
        parts.iter().all(|&part| bounds.contains(part)),
        "{bounds:?} does not contain every one of {parts:?}"
    );
    bounds
}

/// A group of neighbouring clusters, the clusters simplifying it produced, and
/// what that simplification costs.
///
/// The DAG's edges live here rather than on a cluster: grouping is what relates
/// two levels, and a cluster's parents are *the group's* parents — all of them,
/// not one each. That is the "several children, several parents" that makes
/// this a DAG. The *cost* lives here for a second reason, set out in this
/// module's "Why the decision is per group and not per cluster": a group
/// simplifies as a unit, so one error and one sphere are what every cluster it
/// touches has to be judged by.
///
/// `PartialEq` but not `Eq`, unlike the version of this that carried only two
/// index lists: it holds floats now.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterGroup {
    children: Vec<u32>,
    parents: Vec<u32>,
    error: f32,
    bounds: GroupBounds,
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

    /// How far this group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    ///
    /// The same number every one of this group's [`parents`](Self::parents)
    /// reports through [`DagLevel::errors`], which is the identity
    /// `the_error_never_decreases_up_the_dag` asserts — the two spellings exist
    /// because a descent wants it per group and a cluster-indexed upload wants
    /// it per cluster, and they are written from one variable.
    ///
    /// What the number does *not* claim is set out on
    /// [`Simplified::max_error`](crate::Simplified::max_error): it is a quadric
    /// error and has not been shown to dominate a sampled Hausdorff distance.
    #[inline]
    #[must_use]
    pub fn error(&self) -> f32 {
        self.error
    }

    /// The sphere [`error`](Self::error) is projected from.
    ///
    /// Contains this group's children, its parents, and the bounds of every
    /// group below it in the DAG — see this module's "Why the sphere makes the
    /// cut well defined" for what the last of those is load-bearing for.
    #[inline]
    #[must_use]
    pub fn bounds(&self) -> GroupBounds {
        self.bounds
    }

    /// What this group's simplification would cost on screen, in pixels, from
    /// an eye at `eye`.
    ///
    /// `pixels_per_unit` is how many pixels one unit of length subtends one unit
    /// from the eye — `0.5 * viewport_height / tan(0.5 * fov_y)` for a
    /// perspective camera — so the result is [`error`](Self::error) scaled by
    /// that and divided by the distance to the nearest point of
    /// [`bounds`](Self::bounds). Compare it against a pixel budget: over budget
    /// means descend into this group's children, at or under means its parents
    /// are close enough.
    ///
    /// **Both arguments are in the mesh's own space**, which is where the
    /// bounds are. An instance's eye is the camera put through the inverse of
    /// its transform, and a uniform scale on that transform belongs in
    /// `pixels_per_unit`; a non-uniform one does not survive a bounding sphere
    /// at all and is not something this metric can express.
    ///
    /// [`f32::INFINITY`] when the eye is inside the sphere — there is no
    /// distance to divide by, and "as close as possible, so descend" is both the
    /// conservative answer and the monotone one: an eye inside a sphere is
    /// inside every sphere containing it, so every group above answers the same.
    #[must_use]
    pub fn projected_error(&self, eye: Vec3, pixels_per_unit: f32) -> f32 {
        let distance = eye.distance(Vec3::from(self.bounds.center)) - self.bounds.radius;
        if distance <= 0.0 {
            return f32::INFINITY;
        }
        self.error * pixels_per_unit / distance
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
    bounds: Vec<GroupBounds>,
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

    /// Per cluster, the sphere [`errors`](Self::errors) is projected from — the
    /// bounds of the group that produced it, so parallel to that array and
    /// carrying the same value for every cluster one group produced.
    ///
    /// Level 0 was produced by no group, and reads each cluster's own bounding
    /// sphere. Nothing selects on it — a level-0 cluster's producing error is
    /// zero, so there is no descending past it — and it is what the groups of
    /// level 0 are built to enclose.
    #[inline]
    #[must_use]
    pub fn bounds(&self) -> &[GroupBounds] {
        &self.bounds
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

    /// This DAG in [`crcbl_shaders::cluster_dag::ClusterDag`], the cooked form
    /// that has a codec and that the renderer receives a DAG in.
    ///
    /// A **transcription and nothing else**: every number below is read straight
    /// off this DAG, because a converter that computed anything of its own would
    /// be a second implementation of the builder with no test between them. The
    /// two type sets mirror each other field for field precisely so that this
    /// can be true — see [`crcbl_shaders::cluster_dag`]'s own docs for why there
    /// are two of them at all (this crate depends on that one, and the renderer
    /// may depend on neither).
    ///
    /// This is the whole path from a built DAG to bytes on disk:
    /// [`to_bytes`](crcbl_shaders::cluster_dag::ClusterDag::to_bytes) is the
    /// other half, and `crcbl lod gen` is the two of them run over a glTF.
    #[must_use]
    pub fn cook(&self) -> crcbl_shaders::cluster_dag::ClusterDag {
        crcbl_shaders::cluster_dag::ClusterDag {
            levels: self
                .levels
                .iter()
                .map(|level| crcbl_shaders::cluster_dag::DagLevel {
                    positions: level.positions.clone(),
                    clusters: crcbl_shaders::meshlet::MeshClusters {
                        clusters: level.clusters.clusters().to_vec(),
                        vertices: level.clusters.vertices().to_vec(),
                        corners: level.clusters.triangles().to_vec(),
                    },
                    errors: level.errors.clone(),
                    bounds: level.bounds.iter().map(cook_sphere).collect(),
                    groups: level
                        .groups
                        .iter()
                        .map(|group| crcbl_shaders::cluster_dag::ClusterGroup {
                            children: group.children.clone(),
                            parents: group.parents.clone(),
                            error: group.error,
                            bounds: cook_sphere(&group.bounds),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// One sphere in the cooked crate's spelling of it.
fn cook_sphere(bounds: &GroupBounds) -> crcbl_shaders::cluster_dag::GroupBounds {
    crcbl_shaders::cluster_dag::GroupBounds {
        center: bounds.center,
        radius: bounds.radius,
    }
}

/// Build a mesh's cluster DAG.
///
/// `positions` and `indices` are a triangle list exactly as
/// [`GltfPrimitive`](crate::GltfPrimitive) holds them, and become level 0 — its
/// positions verbatim, and its triangles permuted into the order its clusters
/// hold them, which is what [`DagLevel::indices`] means at every level.
/// Levels are added until one of them fails to hold fewer clusters
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
        bounds: (0..clusters.clusters().len())
            .map(|cluster| cluster_sphere(&clusters, cluster))
            .collect(),
        indices: clusters.all_indices(),
        clusters,
        positions: positions.to_vec(),
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
    let grouping = group_clusters(below);

    // The level's faces, laid out group by group, and which group each went
    // into. Grouping is over clusters and a group's clusters need not be
    // adjacent in the index buffer, so this is a reordering and not a slicing.
    let mut ordered = Vec::with_capacity(below.indices.len());
    let mut face_group = Vec::with_capacity(below.indices.len() / 3);
    for (group, members) in grouping.iter().enumerate() {
        for &cluster in members {
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
    let mut faces_of = vec![Vec::new(); grouping.len()];
    for (face, &source) in simplified
        .indices()
        .chunks_exact(3)
        .zip(simplified.source_faces())
    {
        faces_of[face_group[source as usize]].extend_from_slice(face);
    }

    let mut clusters = MeshletBuild::empty();
    let mut groups = Vec::with_capacity(grouping.len());
    let mut errors = Vec::new();
    let mut bounds = Vec::new();
    let mut indices = Vec::new();
    for (children, faces) in grouping.into_iter().zip(&faces_of) {
        let split = build_meshlets(simplified.positions(), faces)?;
        let first = clusters.clusters().len();
        clusters.append(&split)?;
        let parents: Vec<u32> = (first..clusters.clusters().len())
            .map(|parent| parent as u32)
            .collect();

        // Both folds run over the same two sets — what this group produced and
        // what went into it — because both halves of the descent's monotonicity
        // are the same statement about a group and the groups below it: it
        // costs at least what they cost, and it reaches at least as far.
        let error = parents
            .iter()
            .map(|&parent| cluster_error(&clusters, parent as usize, simplified.vertex_errors()))
            .chain(children.iter().map(|&child| below.errors[child as usize]))
            .fold(0.0f32, f32::max);
        let reach: Vec<GroupBounds> = parents
            .iter()
            .map(|&parent| cluster_sphere(&clusters, parent as usize))
            .chain(children.iter().flat_map(|&child| {
                [
                    cluster_sphere(&below.clusters, child as usize),
                    below.bounds[child as usize],
                ]
            }))
            .collect();
        let sphere = enclosing(&reach);

        for &parent in &parents {
            errors.push(error);
            bounds.push(sphere);
            indices.extend(clusters.cluster_indices(parent as usize));
        }
        groups.push(ClusterGroup {
            children,
            parents,
            error,
            bounds: sphere,
        });
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
            bounds,
            groups: Vec::new(),
        },
    )))
}

/// One cluster's own bounding sphere, as the thing [`enclosing`] folds.
fn cluster_sphere(clusters: &MeshletBuild, cluster: usize) -> GroupBounds {
    let bounds = clusters.clusters()[cluster].bounds;
    GroupBounds {
        center: bounds.center,
        radius: bounds.radius,
    }
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
///
/// It returns the memberships alone rather than [`ClusterGroup`]s, because a
/// group's parents, error and bounds are all decided by the simplification that
/// has not happened yet — and a group built here would have to hold three
/// placeholders until it had, one of which is a sphere that would select
/// nonsense if anything ever read it early.
fn group_clusters(level: &DagLevel) -> Vec<Vec<u32>> {
    let adjacency = cluster_adjacency(level);
    let mut group_of: Vec<Option<usize>> = vec![None; adjacency.len()];
    let mut groups: Vec<Vec<u32>> = Vec::new();

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
            groups[host].push(seed as u32);
            groups[host].sort_unstable();
            continue;
        }

        children.sort_unstable();
        groups.push(children.into_iter().map(|cluster| cluster as u32).collect());
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
    use crate::meshlet::tests::{cluster_triangles, decoded, sorted, triangles_of};
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
        descend(dag, |group| threshold < group.error())
    }

    /// The clusters an "is this group expanded?" answer draws.
    ///
    /// `docs/plan/25-lod.md`'s descent, and the whole of it: a cluster is drawn
    /// when the group that *produced* it is not expanded and the group that
    /// *contains* it is. Both halves ask `expanded` about a **group**, which is
    /// why one answer per group is all the rule needs and why every cluster a
    /// group produced moves together.
    ///
    /// [`cut`] passes a global error threshold and
    /// [`camera_cut`] passes the projected-error rule a GPU would run; the
    /// descent is written once and neither owns a copy of it.
    ///
    /// A cluster that no group contains defaults to **not** drawn, which is
    /// only correct on the top level — every other level's clusters are each in
    /// exactly one group. That is deliberate: a level whose grouping missed a
    /// cluster leaves a hole in the cover, which
    /// [`assert_cut_is_a_crack_free_cover`] reports by name, where a default of
    /// "drawn" would quietly produce an overlap instead.
    fn descend(dag: &ClusterDag, expanded: impl Fn(&ClusterGroup) -> bool) -> Vec<(usize, usize)> {
        let top = dag.levels().len() - 1;
        let mut drawn = Vec::new();
        for (level, here) in dag.levels().iter().enumerate() {
            let count = here.clusters().clusters().len();
            let mut container = vec![level == top; count];
            for group in here.groups() {
                let open = expanded(group);
                for &child in group.children() {
                    container[child as usize] = open;
                }
            }

            let mut producer = vec![false; count];
            if level > 0 {
                for group in dag.levels()[level - 1].groups() {
                    let open = expanded(group);
                    for &parent in group.parents() {
                        producer[parent as usize] = open;
                    }
                }
            }

            for cluster in 0..count {
                if !producer[cluster] && container[cluster] {
                    drawn.push((level, cluster));
                }
            }
        }
        drawn
    }

    /// The clusters a camera at `eye` draws — the rule an amplification stage
    /// will run, evaluated host-side.
    fn camera_cut(dag: &ClusterDag, eye: Vec3, budget: f32) -> Vec<(usize, usize)> {
        descend(dag, |group| {
            group.projected_error(eye, PIXELS_PER_UNIT) > budget
        })
    }

    /// How many pixels one unit of length subtends one unit from the eye, for
    /// the frames this workspace's goldens are drawn at: a 192-pixel-high
    /// viewport at a 60-degree vertical field of view.
    ///
    /// Written as a literal rather than `0.5 * 192.0 / (PI / 6.0).tan()`
    /// because `tanf` is not correctly rounded and differs in the last place
    /// between libms — the same argument `crcbl_shaders::dunes` makes — and the
    /// counts below are pinned by equality. Nothing here turns on the exact
    /// figure; it sets the scale at which the budget picks a level.
    const PIXELS_PER_UNIT: f32 = 166.0;

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

    /// Every cut reached by expanding one group at a time, deepest group first,
    /// starting from the top level.
    ///
    /// A cut is *any* set of clusters covering the surface once, and swapping a
    /// group's parents for its children turns one cut into another — which is
    /// the definition [`cut`] applies a global error budget to. The budget is
    /// the narrower thing, and on this fixture it never draws three levels at
    /// once: the decimator orders collapses by cost across the whole level, so
    /// a level's groups all report much the same error, the levels' error bands
    /// do not overlap, and a rising threshold passes each band in turn.
    ///
    /// **They used to overlap for a bad reason.** While `build_meshlets` grew
    /// clusters along the index buffer, every cluster spanned the mesh, groups
    /// locked boundaries that ran end to end, and a group could stall — keep
    /// every triangle it was given and carry its children's error up unchanged.
    /// That one number then appeared in three levels' error lists at once, and
    /// it was what a threshold cut three levels on. So the depth is not
    /// something the budget lost; it is something a degenerate group was
    /// providing. The DAG admits the deep cuts either way, and this reaches
    /// them the way the definition does, by always expanding the deepest group
    /// it can: a cut here draws level 0 in one place while most of the surface
    /// is still at the top.
    fn expanded_cuts(dag: &ClusterDag) -> Vec<Vec<(usize, usize)>> {
        let top = dag.levels().len() - 1;
        let mut drawn: BTreeSet<(usize, usize)> =
            (0..dag.levels()[top].clusters().clusters().len())
                .map(|cluster| (top, cluster))
                .collect();
        let mut cuts = vec![drawn.iter().copied().collect()];

        while let Some((level, group)) = (0..top).find_map(|level| {
            dag.levels()[level]
                .groups()
                .iter()
                .find(|group| {
                    group
                        .parents()
                        .iter()
                        .all(|&parent| drawn.contains(&(level + 1, parent as usize)))
                })
                .map(|group| (level, group))
        }) {
            for &parent in group.parents() {
                drawn.remove(&(level + 1, parent as usize));
            }
            for &child in group.children() {
                drawn.insert((level, child as usize));
            }
            cuts.push(drawn.iter().copied().collect());
        }
        cuts
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
        assert_eq!(
            sorted(triangles_of(dag.levels()[0].indices())),
            sorted(triangles_of(&indices)),
            "level 0 is the base mesh's own triangles, in its clusters' order"
        );
        assert_eq!(
            dag.levels()
                .iter()
                .map(|level| level.clusters().clusters().len())
                .collect::<Vec<_>>(),
            [23, 17, 9, 5, 3],
            "tens of clusters at the leaves is what makes grouping possible"
        );
        assert_eq!(
            dag.levels()
                .iter()
                .map(|level| level.indices().len() / 3)
                .collect::<Vec<_>>(),
            [2048, 1024, 512, 256, 128]
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
        let budgeted: Vec<Vec<(usize, usize)>> = every_threshold(&dag)
            .into_iter()
            .map(|threshold| cut(&dag, threshold))
            .collect();
        for drawn in budgeted.iter().chain(&expanded_cuts(&dag)) {
            let interfaces = assert_cut_is_a_crack_free_cover(&dag, drawn, &border);
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
            "only {mixed} of the cuts held more than one level, so this is \
             mostly testing uniform cuts"
        );
        assert!(
            deepest >= 3,
            "no cut drew three levels at once, so the deepest interface this \
             checked was one level to the next"
        );
        // The budgeted cuts are the ones selection will actually ask for, so
        // they have to be more than the two ends of the DAG on their own.
        assert!(
            budgeted
                .iter()
                .filter(|drawn| {
                    drawn
                        .iter()
                        .map(|&(level, _)| level)
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 1
                })
                .count()
                > 5,
            "an error budget almost never draws a mixed cut on this fixture"
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
        let threshold = 0.66;
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
                // The two spellings of one number: what the group charges, and
                // what each cluster it produced carries. A descent reads the
                // first and an upload indexed by cluster reads the second, and
                // nothing recomputes either — so they are one variable in the
                // builder and this is what says they still are.
                assert_eq!(
                    parents[0],
                    group.error(),
                    "a group of level {level} charges {} and its parents carry {}",
                    group.error(),
                    parents[0],
                );
                for &parent in group.parents() {
                    assert_eq!(
                        above.bounds()[parent as usize],
                        group.bounds(),
                        "a parent at level {} bounds something other than the group that \
                         produced it",
                        level + 1,
                    );
                }
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

    /// The monotonicity the *projected* metric needs, which is strictly more
    /// than the stored error's.
    ///
    /// A group has to cost at least what the groups below it cost **and** reach
    /// at least as far as they do. The first alone is not enough once a
    /// distance divides it: a group whose sphere sat closer to the eye than one
    /// below it would project a larger error from a smaller number, the descent
    /// would find two stopping points along one branch, and the cut would draw
    /// a cluster and its ancestor together.
    ///
    /// Stated over the pair the descent actually reads — for each cluster, the
    /// group that produced it against the group that contains it — rather than
    /// over levels, because that pair is what
    /// [`ClusterGroup::projected_error`] is asked about and a level is not.
    #[test]
    fn a_group_is_never_cheaper_or_further_than_the_groups_below_it() {
        let (_, _, dag) = dense_dag();
        assert_levels_got_coarser(&dag);

        let mut checked = 0;
        for (level, here) in dag.levels().iter().enumerate() {
            for group in here.groups() {
                for &child in group.children() {
                    let child = child as usize;
                    assert!(
                        group.error() >= here.errors()[child],
                        "a group of level {level} costs {} and the group that produced its \
                         child {child} cost {}",
                        group.error(),
                        here.errors()[child],
                    );
                    assert!(
                        group.bounds().contains(here.bounds()[child]),
                        "a group of level {level} bounds {:?} and does not contain the {:?} \
                         its child {child} was produced from, so the descent can invert",
                        group.bounds(),
                        here.bounds()[child],
                    );
                    checked += 1;
                }
            }
        }

        assert!(
            checked > 20,
            "only {checked} producer/container pairs exist, so this is not a sweep"
        );
        // Anti-vacuity: containment is trivial if every sphere is the same
        // sphere, and a DAG whose groups all bound the whole mesh would satisfy
        // every assertion above while making the distance term constant and the
        // near/far difference below impossible.
        let radii: Vec<f32> = dag
            .levels()
            .iter()
            .flat_map(|level| level.groups())
            .map(|group| group.bounds().radius())
            .collect();
        let tightest = radii.iter().copied().fold(f32::INFINITY, f32::min);
        let widest = radii.iter().copied().fold(0.0f32, f32::max);
        assert!(
            widest > 2.0 * tightest,
            "group radii run {tightest} to {widest}, which is one sphere wearing several \
             names — containment is then trivial and no cut can ever be mixed"
        );
    }

    /// The rule a GPU will run, over cameras rather than over thresholds: every
    /// eye position produces a cut, and every one of those cuts is a crack-free
    /// cover of the surface.
    ///
    /// [`every_cut_is_a_crack_free_cover_of_the_surface`] sweeps a global error
    /// budget, which is the host-side statement. This is the runtime one, and
    /// it is a different claim: the budget is now divided by a distance that
    /// varies across the mesh, so a cut here mixes levels because of *where the
    /// camera is* rather than because two groups simplified differently. That
    /// is the case a per-cluster distance term breaks and a per-group one does
    /// not.
    #[test]
    fn every_camera_position_draws_a_crack_free_cut() {
        let (positions, indices, dag) = dense_dag();
        assert_levels_got_coarser(&dag);
        let border = base_border(&positions, &indices);

        let mut mixed = 0;
        let mut cuts = 0;
        for &eye in &EYES {
            for budget in BUDGETS {
                let drawn = camera_cut(&dag, eye, budget);
                let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
                let levels: BTreeSet<usize> = drawn.iter().map(|&(level, _)| level).collect();
                if levels.len() > 1 {
                    mixed += 1;
                    assert!(
                        interfaces > 0,
                        "a cut spanning {levels:?} from {eye:?} whose levels never meet is \
                         not a mixed cut, it is two meshes"
                    );
                }
                cuts += 1;
            }
        }

        assert_eq!(
            cuts,
            EYES.len() * BUDGETS.len(),
            "the sweep has to have run"
        );
        assert!(
            mixed > cuts / 2,
            "only {mixed} of {cuts} camera cuts held more than one level, so this is \
             mostly re-testing uniform cuts under a new name"
        );
    }

    /// **The thing per-cluster selection is for**, in numbers: one draw of one
    /// mesh whose near end is at a finer level than its far end.
    ///
    /// A ground plane receding from the viewer is the shape that makes this
    /// visible, and the distance term is what produces it — the stored errors
    /// barely vary within a level (`simplify` targets a triangle count, so a
    /// level's error frontier is flat), and a cut that read them alone would be
    /// uniform at every camera. Split the drawn clusters by how far up the
    /// patch they sit and the two ends report different levels.
    ///
    /// The histograms are pinned rather than compared loosely, because "the
    /// near end is finer" is satisfied by a cut that is one cluster away from
    /// uniform, and that is not the claim.
    #[test]
    fn a_receding_plane_draws_its_near_end_finer_than_its_far_end() {
        let (positions, indices, dag) = dense_dag();
        assert_levels_got_coarser(&dag);

        // The eye stands just off the near edge, low over the surface, so the
        // far edge is some fifteen times further away than the near one.
        let eye = EYES[0];
        let drawn = camera_cut(&dag, eye, MIXING_BUDGET);

        let mut near: BTreeMap<usize, usize> = BTreeMap::new();
        let mut far: BTreeMap<usize, usize> = BTreeMap::new();
        for &(level, cluster) in &drawn {
            let depth = dag.levels()[level].clusters().clusters()[cluster]
                .bounds
                .center[1];
            if depth < NEAR_THIRD {
                *near.entry(level).or_default() += 1;
            } else if depth > FAR_THIRD {
                *far.entry(level).or_default() += 1;
            }
        }

        assert_eq!(
            near,
            BTreeMap::from([(0, 7)]),
            "the near third of the plane is not seven leaf clusters"
        );
        assert_eq!(
            far,
            BTreeMap::from([(1, 5)]),
            "the far third of the plane is not five clusters of level 1"
        );
        // And the two ends are one cut of one mesh, not two pictures: the same
        // set of clusters covers the whole surface exactly once, with no hole
        // where the levels meet.
        let border = base_border(&positions, &indices);
        let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
        assert!(
            interfaces > 20,
            "only {interfaces} edges have one level on one side and another on the other, \
             which is too few to be the seam between the two ends"
        );
    }

    /// Pixel budgets the fixture actually changes its cut across.
    ///
    /// Below the smallest of these every camera here draws level 0 whole and
    /// above the largest the DAG has run out of levels, so a sweep outside the
    /// band would be a sweep over one answer. The band is a property of this
    /// fixture's errors and its size, not a tuning the engine ships.
    const BUDGETS: [f32; 5] = [8.0, 16.0, 32.0, 64.0, 128.0];

    /// The budget [`a_receding_plane_draws_its_near_end_finer_than_its_far_end`]
    /// reads its histograms at: inside the band where level 0's groups and
    /// level 1's fall on opposite sides of the test at this camera.
    const MIXING_BUDGET: f32 = 32.0;

    /// Where the near third of the patch ends and the far third begins, in the
    /// fixture's own units — it spans `0..=DENSE_SIDE` along the axis running
    /// away from [`EYES`]`[0]`. The middle third is left out of both
    /// histograms: it is where the two ends change over, and which level a
    /// cluster there takes is the thing the budget decides rather than the
    /// thing being asserted.
    const NEAR_THIRD: f32 = DENSE_SIDE as f32 / 3.0;
    const FAR_THIRD: f32 = DENSE_SIDE as f32 * 2.0 / 3.0;

    /// Where the near/far sweep puts the camera, in the fixture's own space.
    ///
    /// `height_field` lays the patch out on `0..=DENSE_SIDE` in x and y with the
    /// height on z, so an eye at negative y with a small z is a viewer standing
    /// at one edge of a ground plane that recedes away from them — the shape
    /// `docs/plan/25-lod.md`'s per-cluster selection exists for. The rest are
    /// off a corner, high above, and level with the middle, so the sweep is not
    /// one camera's arrangement holding.
    const EYES: [Vec3; 5] = [
        Vec3::new(16.0, -2.0, 2.0),
        Vec3::new(16.0, -12.0, 6.0),
        Vec3::new(-8.0, -8.0, 4.0),
        Vec3::new(16.0, 16.0, 40.0),
        Vec3::new(16.0, 16.0, 3.0),
    ];

    fn sphere(center: [f32; 3], radius: f32) -> GroupBounds {
        GroupBounds { center, radius }
    }

    /// [`enclosing`] on three arrangements whose answers are arithmetic rather
    /// than a measurement, and the round-up that makes
    /// [`GroupBounds::contains`] hold rather than nearly hold.
    ///
    /// Every coordinate here is a small integer, so the AABB, its midpoint and
    /// each part's reach from it are all exact in `f32` — which is what lets
    /// the radius be compared against the exact answer's `next_up` instead of
    /// against a tolerance.
    #[test]
    fn a_group_sphere_contains_every_sphere_it_was_built_from() {
        // One part: the same sphere, one rounding step out.
        let one = enclosing(&[sphere([1.0, 2.0, 3.0], 4.0)]);
        assert_eq!(one.center(), [1.0, 2.0, 3.0]);
        assert_eq!(one.radius(), 4.0f32.next_up());
        assert!(one.contains(sphere([1.0, 2.0, 3.0], 4.0)));

        // Two disjoint parts: centred between them, reaching both.
        let pair = enclosing(&[sphere([0.0; 3], 1.0), sphere([10.0, 0.0, 0.0], 1.0)]);
        assert_eq!(pair.center(), [5.0, 0.0, 0.0]);
        assert_eq!(pair.radius(), 6.0f32.next_up());
        assert!(pair.contains(sphere([0.0; 3], 1.0)));
        assert!(pair.contains(sphere([10.0, 0.0, 0.0], 1.0)));

        // A part wholly inside another: the outer one, ungrown. A merge that
        // simply summed the two would report 12 here.
        let nested = enclosing(&[sphere([0.0; 3], 10.0), sphere([1.0, 0.0, 0.0], 1.0)]);
        assert_eq!(nested.center(), [0.0; 3]);
        assert_eq!(nested.radius(), 10.0f32.next_up());

        // And the property the descent needs, over a part that is *not* at a
        // representable distance from the midpoint: the round-up is what makes
        // this true rather than true-to-a-tolerance.
        let awkward = [
            sphere([0.1, 0.2, 0.3], 0.7),
            sphere([-1.3, 2.7, 0.9], 1.1),
            sphere([5.5, -0.4, 3.3], 0.2),
        ];
        let grown = enclosing(&awkward);
        for part in awkward {
            assert!(grown.contains(part), "{grown:?} does not contain {part:?}");
        }
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
        // No cluster holds triangles from both sheets, and at this size that is
        // structural rather than lucky: `build_meshlets` grows a cluster across
        // shared edges, and it takes a second connected component only when it
        // can take the whole of it — a sheet this size is far too large. The
        // assertion below is what holds that, because a cluster spanning both
        // sheets would make them edge-adjacent through it and leave this test
        // saying nothing.
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

    /// [`ClusterDag::cook`] carries every field across, and the codec on the
    /// far side is its inverse.
    ///
    /// Field by field against the *builder's* DAG rather than only through the
    /// round trip: a transcription that dropped a group's parents, or read a
    /// cluster's own sphere where the group's belongs, would encode and decode
    /// perfectly and be wrong in the one direction that matters.
    #[test]
    fn a_cooked_dag_is_the_built_one_and_survives_the_codec() {
        let (positions, _, dag) = dense_dag();
        let cooked = dag.cook();

        assert_eq!(cooked.levels.len(), dag.levels().len());
        assert!(
            dag.levels().len() > 2,
            "a DAG of {} levels does not exercise the grouping",
            dag.levels().len()
        );
        let mut groups = 0usize;
        for (depth, (level, mirror)) in dag.levels().iter().zip(&cooked.levels).enumerate() {
            assert_eq!(mirror.positions, level.positions(), "level {depth}");
            assert_eq!(
                mirror.clusters.clusters,
                level.clusters().clusters(),
                "level {depth}"
            );
            assert_eq!(
                mirror.clusters.vertices,
                level.clusters().vertices(),
                "level {depth}"
            );
            assert_eq!(
                mirror.clusters.corners,
                level.clusters().triangles(),
                "level {depth}"
            );
            assert_eq!(mirror.errors, level.errors(), "level {depth}");
            assert_eq!(
                mirror.bounds.len(),
                level.bounds().len(),
                "level {depth}'s spheres"
            );
            for (sphere, source) in mirror.bounds.iter().zip(level.bounds()) {
                assert_eq!(
                    (sphere.center, sphere.radius),
                    (source.center(), source.radius())
                );
            }
            assert_eq!(mirror.groups.len(), level.groups().len(), "level {depth}");
            for (group, source) in mirror.groups.iter().zip(level.groups()) {
                assert_eq!(group.children, source.children());
                assert_eq!(group.parents, source.parents());
                assert_eq!(group.error, source.error());
                assert_eq!(group.bounds.center, source.bounds().center());
                assert_eq!(group.bounds.radius, source.bounds().radius());
                groups += 1;
            }
        }
        assert!(groups > 0, "no group was compared, so nothing was checked");

        assert_eq!(
            crcbl_shaders::cluster_dag::ClusterDag::from_bytes(&cooked.to_bytes(), positions)
                .as_ref(),
            Ok(&cooked),
            "the artifact does not read back as what was cooked"
        );
    }
}
