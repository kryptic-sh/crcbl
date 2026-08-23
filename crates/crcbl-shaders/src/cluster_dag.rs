//! A mesh's cluster DAG, in the cooked form the renderer receives it in.
//!
//! `docs/plan/25-lod.md`'s "How a DAG reaches the renderer" decided this shape.
//! The builder is `crcbl_scene::cluster_dag::build_cluster_dag`, and neither
//! this crate nor `crcbl-render` can call it: `crcbl-scene` depends on this one,
//! and the renderer must not depend on `crcbl-scene` at all — that would pull
//! `gltf` into the renderer. So the DAG arrives the way the compiled SPIR-V
//! beside it does, **as a committed artifact with a generator and a check**:
//!
//! * `tools/cook-clusters.rs` builds the DAG from the builder and writes
//!   `clusters/dunes.dag`;
//! * `--check` rebuilds it and demands the bytes be identical, and CI runs that,
//!   exactly as it runs `tools/compile-shaders.sh --check`;
//! * this module is the decoder, and the only thing a consumer sees.
//!
//! When topic 6's asset pipeline arrives it replaces the generator, not the
//! consumer.
//!
//! # What a cut is, and how a camera picks one
//!
//! Selection picks a **cut**: a set of clusters covering the surface exactly
//! once, formed by expanding some groups (draw their children) and not others
//! (draw their parents). Writing `E(G)` for
//! [`ClusterGroup::projected_error`](crate::cluster_dag::ClusterGroup::projected_error)`
//! > budget` — *this group is expanded* — a
//! cluster is drawn exactly when
//!
//! ```text
//! !E(the group that produced it) && E(the group that contains it)
//! ```
//!
//! reading a level-0 cluster's absent producer as never expanded and a top-level
//! cluster's absent container as always expanded. Both halves ask about a
//! **group**, never about the cluster's own centre, which is what lets a GPU
//! evaluate the rule per cluster with no communication and still move every
//! cluster of a group together. The reasoning — and the crack a per-cluster
//! distance term opens — is in `crcbl_scene::cluster_dag`'s module docs; this
//! module carries the numbers that rule reads and
//! `the_cooked_dag_draws_a_crack_free_cut_from_every_camera` is it, run over the
//! committed artifact.
//!
//! # The rule, the state it carries, and what is still absent
//!
//! [`ClusterDag::cut`](crate::cluster_dag::ClusterDag::cut) is that rule run
//! over a whole DAG host-side, and
//! [`ClusterDag::selection_records`](crate::cluster_dag::ClusterDag::selection_records)
//! is the same two groups resolved per cluster
//! for a GPU to run it one cluster at a time — see [`crate::cluster_select`],
//! which is the record `mesh_cluster.slang`'s amplification stage reads.
//!
//! `docs/plan/25-lod.md`'s **hysteresis** is [`ClusterDag::expand`], which turns
//! last frame's expansion into this frame's under two budgets rather than one;
//! [`ClusterDag::cut_from`] and [`ClusterDag::uniform_level_from`] are the two
//! granularities asked of the state it produces. What is still absent is that
//! plan's shadow LOD bias and its bake cache; neither exists anywhere yet.
//!
//! [`ClusterDag::expand`]: crate::cluster_dag::ClusterDag::expand
//! [`ClusterDag::cut_from`]: crate::cluster_dag::ClusterDag::cut_from
//! [`ClusterDag::uniform_level_from`]: crate::cluster_dag::ClusterDag::uniform_level_from

use std::collections::{BTreeMap, BTreeSet};

use crate::cluster_select::{ClusterSelect, GroupCost, LodBudgets, group_is_expanded};
use crate::dunes;
use crate::level_select::LevelGroup;
use crate::meshlet::{MESHLET_STRIDE, MeshClusters, Meshlet};

/// The committed DAG of [`crate::dunes`], decoded.
///
/// The artifact does not repeat level 0's positions — `build_cluster_dag`
/// documents level 0 as the caller's own array verbatim, so they are
/// [`dunes::positions`] and re-committing them would be a second copy to drift.
/// `tools/cook-clusters.rs` asserts the two agree before it writes anything.
///
/// # Panics
///
/// If the committed artifact does not decode, which is a corrupt file in the
/// tree rather than a runtime condition: it is generated, checked byte for byte
/// by CI, and read by [`include_bytes!`] from this crate's own source.
#[must_use]
pub fn dunes_dag() -> ClusterDag {
    ClusterDag::from_bytes(include_bytes!("../clusters/dunes.dag"), dunes::positions())
        .unwrap_or_else(|error| panic!("the committed dunes DAG does not decode: {error}"))
}

/// The sphere a group's error is projected from.
///
/// Separate from [`ClusterBounds`](crate::meshlet::ClusterBounds), which a
/// cluster carries for culling: that one bounds a single cluster's geometry as
/// tightly as the builder can, where this one bounds a whole group's and is
/// deliberately grown to contain every sphere below it in the DAG. **Only this
/// one may decide a level** — the containment is what carries monotone stored
/// error into monotone projected error, and a cluster's own tighter sphere would
/// break it.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupBounds {
    /// Centre of the sphere, in the mesh's own space.
    pub center: [f32; 3],
    /// Distance from [`center`](Self::center) to the furthest point the sphere
    /// has to contain. Never negative.
    pub radius: f32,
}

impl GroupBounds {
    /// Whether this sphere contains `inner` whole.
    ///
    /// The relation the descent depends on: a sphere containing another is never
    /// further from any eye than the one it contains.
    #[must_use]
    pub fn contains(&self, inner: Self) -> bool {
        let separation = f64::from(
            [0, 1, 2]
                .map(|axis| self.center[axis] - inner.center[axis])
                .iter()
                .map(|delta| delta * delta)
                .sum::<f32>()
                .sqrt(),
        );
        separation + f64::from(inner.radius) <= f64::from(self.radius)
    }
}

/// A group of neighbouring clusters, the clusters simplifying it produced, and
/// what that simplification costs.
///
/// The DAG's edges live here rather than on a cluster, because grouping is what
/// relates two levels and a cluster's parents are *the group's* parents — all of
/// them. The cost lives here for the second reason this module's docs give: a
/// group simplifies as a unit, so one error and one sphere are what every
/// cluster it touches has to be judged by.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterGroup {
    /// The clusters that were grouped, as indices into the clusters of the level
    /// this group belongs to. Ascending, and never empty.
    pub children: Vec<u32>,
    /// The clusters re-splitting the simplified group produced, as indices into
    /// the clusters of the level **above** this group's. Ascending and
    /// contiguous.
    pub parents: Vec<u32>,
    /// How far this group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    ///
    /// A quadric error, and **not a certified Hausdorff bound** — see
    /// `crcbl_scene::Simplified::max_error` for what it does and does not claim.
    pub error: f32,
    /// The sphere [`error`](Self::error) is projected from.
    pub bounds: GroupBounds,
}

impl ClusterGroup {
    /// What this group's simplification would cost on screen, in pixels, from an
    /// eye at `eye`.
    ///
    /// `pixels_per_unit` is how many pixels one unit of length subtends one unit
    /// from the eye — `0.5 * viewport_height / tan(0.5 * fov_y)` for a
    /// perspective camera — so the result is [`error`](Self::error) scaled by
    /// that and divided by the distance to the nearest point of
    /// [`bounds`](Self::bounds). Over a pixel budget means descend into this
    /// group's children; at or under means its parents are close enough.
    ///
    /// **Both arguments are in the mesh's own space**, which is where the bounds
    /// are. An instance's eye is the camera put through the inverse of its
    /// transform, and a uniform scale on that transform belongs in
    /// `pixels_per_unit`; a non-uniform one does not survive a bounding sphere
    /// and is not something this metric can express.
    ///
    /// [`f32::INFINITY`] when the eye is inside the sphere: there is no distance
    /// to divide by, and "as close as possible, so descend" is both the
    /// conservative answer and the monotone one, since an eye inside a sphere is
    /// inside every sphere containing it.
    ///
    /// This is `crcbl_scene::ClusterGroup::projected_error` on the cooked side of
    /// the seam, and `tools/cook-clusters.rs` compares the two over the whole
    /// DAG at a sweep of eyes rather than letting a second copy drift.
    #[must_use]
    pub fn projected_error(&self, eye: [f32; 3], pixels_per_unit: f32) -> f32 {
        let separation = [0, 1, 2]
            .map(|axis| eye[axis] - self.bounds.center[axis])
            .iter()
            .map(|delta| delta * delta)
            .sum::<f32>()
            .sqrt();
        let distance = separation - self.bounds.radius;
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
    /// This level's vertex positions. Level 0's are the base mesh's verbatim.
    ///
    /// Positions alone, because that is all the decimator carries: a coarser
    /// level's vertices are wherever collapses put them and belong to no vertex
    /// of the level below. [`dunes::vertex_at`] is what turns one into a
    /// [`MeshVertex`](crate::mesh::MeshVertex), by evaluating the surface rather
    /// than by interpolating an attribute nothing recorded.
    pub positions: Vec<[f32; 3]>,

    /// This level's clusters and the two arrays they index, built over this
    /// level's own geometry — everything a mesh stage needs, ready to upload.
    pub clusters: MeshClusters,

    /// Per cluster, how far its surface may depart from the base mesh's, in the
    /// mesh's own units of length. Parallel to
    /// [`clusters.clusters`](MeshClusters::clusters).
    ///
    /// **Every cluster a group produced carries the group's number**, not one of
    /// its own: a group simplifies as a unit, so its parents stand or fall
    /// together. Detail still varies across a level because different groups
    /// report different errors; what does not vary is detail *within* a group.
    /// Level 0 is the base mesh untouched and reads zero throughout.
    pub errors: Vec<f32>,

    /// Per cluster, the sphere [`errors`](Self::errors) is projected from — the
    /// bounds of the group that produced it, so parallel to that array and
    /// carrying the same value for every cluster one group produced.
    ///
    /// Level 0 was produced by no group and reads each cluster's own bounding
    /// sphere; nothing selects on it, since a level-0 cluster's producing error
    /// is zero.
    pub bounds: Vec<GroupBounds>,

    /// How this level's clusters were grouped to build the level above. Every
    /// cluster of this level is in exactly one group; empty on the top level.
    pub groups: Vec<ClusterGroup>,
}

impl DagLevel {
    /// This level's triangles as a plain index list over its own
    /// [`positions`](Self::positions), cluster by cluster.
    ///
    /// What a level costs to upload as ordinary geometry: [`positions`] and
    /// [`clusters`] describe the surface for a mesh stage, and a vertex stage
    /// pulling through an index buffer needs the same triangles spelled the
    /// other way. Every geometry path draws a DAG level out of this — the mesh
    /// path so its levels are resident in one vertex pool at all, and
    /// `docs/plan/25-lod.md`'s uniform cut because a whole level *is* a chain
    /// level.
    ///
    /// [`positions`]: Self::positions
    /// [`clusters`]: Self::clusters
    #[must_use]
    pub fn indices(&self) -> Vec<u32> {
        let mut indices = Vec::new();
        for cluster in &self.clusters.clusters {
            let run = &self.clusters.vertices[cluster.vertex_offset as usize..]
                [..cluster.vertex_count as usize];
            // A corner indexes its own cluster's run, not the array, which is
            // the whole difference between a corner and an index.
            for &corner in &self.clusters.corners[cluster.triangle_offset as usize..]
                [..cluster.triangle_count as usize * 3]
            {
                indices.push(run[usize::from(corner)]);
            }
        }
        indices
    }
}

/// A mesh as a DAG of clusters: level 0 is the base, each level above it the
/// simplified re-split of the one below.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterDag {
    /// The levels, finest first. Never empty.
    pub levels: Vec<DagLevel>,
}

/// Where one cluster of a cut is: which level, and which cluster of that level.
///
/// The two indices together, because neither names a cluster on its own — every
/// level numbers its clusters from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterAt {
    /// Index into [`ClusterDag::levels`].
    pub level: usize,
    /// Index into that level's [`clusters.clusters`](MeshClusters::clusters).
    pub cluster: usize,
}

impl ClusterDag {
    /// The clusters a camera at `eye` draws under a budget of `budget` pixels.
    ///
    /// **The selection rule, host-side**, and the oracle a GPU descent is held
    /// to: `crcbl-vk`'s `the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`
    /// reads back what the amplification stage actually kept and compares it
    /// against this, cluster for cluster.
    ///
    /// A cluster is drawn when the group that *produced* it is not expanded and
    /// the group that *contains* it is, where a group is expanded exactly when
    /// its [`projected_error`](ClusterGroup::projected_error) exceeds `budget` —
    /// this module's docs carry the rule and why both halves name a group.
    /// `eye` and the spheres are in one space, and `pixels_per_unit` is what
    /// carries the viewport and the field of view into it.
    ///
    /// The result is ascending by level and then by cluster, so two cuts compare
    /// without sorting.
    #[must_use]
    pub fn cut(&self, eye: [f32; 3], pixels_per_unit: f32, budget: f32) -> Vec<ClusterAt> {
        self.cut_from(&self.states(|group| group.projected_error(eye, pixels_per_unit) > budget))
    }

    /// How many groups this DAG has, which is the length every expansion state
    /// this type takes or returns has.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.levels.iter().map(|here| here.groups.len()).sum()
    }

    /// Where each level's groups start in [`level_groups`](Self::level_groups)
    /// order — the numbering every expansion state is indexed by.
    fn group_bases(&self) -> Vec<usize> {
        self.levels
            .iter()
            .scan(0usize, |at, here| {
                let base = *at;
                *at += here.groups.len();
                Some(base)
            })
            .collect()
    }

    /// Every group's expansion, from a predicate over the group itself.
    fn states(&self, expanded: impl Fn(&ClusterGroup) -> bool) -> Vec<bool> {
        self.levels
            .iter()
            .flat_map(|here| &here.groups)
            .map(expanded)
            .collect()
    }

    /// One frame of `docs/plan/25-lod.md`'s hysteresis: which groups are
    /// expanded now, given which were expanded last frame.
    ///
    /// [`group_is_expanded`] over every group, in
    /// [`level_groups`](Self::level_groups) order — the rule `draw_gen.slang`
    /// runs once per (instance, group) and writes into the state buffer the
    /// amplification stage then reads. `was` is the previous frame's answer, and
    /// `&vec![false; dag.group_count()]` is where a mesh with no history starts.
    ///
    /// **The result is monotone up the DAG whenever `was` is**, which is what
    /// makes every frame's [`cut_from`](Self::cut_from) a cover — see
    /// [`crate::cluster_select`]'s module docs for the induction, and
    /// `a_drifting_camera_keeps_a_crack_free_cover_under_hysteresis` for it run
    /// over the committed artifact.
    ///
    /// # Panics
    ///
    /// If `was` is not [`group_count`](Self::group_count) long, which is a
    /// caller carrying another mesh's state rather than a runtime condition.
    #[must_use]
    pub fn expand(
        &self,
        eye: [f32; 3],
        pixels_per_unit: f32,
        budgets: LodBudgets,
        was: &[bool],
    ) -> Vec<bool> {
        assert_eq!(
            was.len(),
            self.group_count(),
            "a previous state of another DAG's shape"
        );
        self.levels
            .iter()
            .flat_map(|here| &here.groups)
            .zip(was)
            .map(|(group, &was)| {
                let cost = GroupCost {
                    error: group.error,
                    bounds: group.bounds,
                };
                group_is_expanded(&cost, eye, pixels_per_unit, budgets, was)
            })
            .collect()
    }

    /// The one level a **uniform cut** draws from an eye at `eye` under a
    /// budget of `budget` pixels.
    ///
    /// `docs/plan/25-lod.md`'s granularity for the two indirect tails: "every
    /// cluster at one depth, which is exactly a whole-mesh level". The rule is
    /// the finest level any group is expanded at, and the top level when none
    /// is — [`crate::level_select`]'s module docs carry why that number is the
    /// finest level [`cut`](Self::cut) draws anywhere, which is what makes the
    /// coarser decision comparable to the finer one rather than merely similar.
    ///
    /// `eye`, `pixels_per_unit` and `budget` mean what they mean for
    /// [`cut`](Self::cut) — this is the same metric over the same groups, asked
    /// once for the mesh instead of once for each of its clusters.
    #[must_use]
    pub fn uniform_level(&self, eye: [f32; 3], pixels_per_unit: f32, budget: f32) -> usize {
        self.uniform_level_from(
            &self.states(|group| group.projected_error(eye, pixels_per_unit) > budget),
        )
    }

    /// The one level a uniform cut draws, given which groups are expanded.
    ///
    /// [`uniform_level`](Self::uniform_level) with the expansion supplied rather
    /// than computed, so a hysteretic frame asks the same question of the state
    /// it carries. `MeshLevels::uniform_level` is this over the cooked records.
    ///
    /// # Panics
    ///
    /// If `expanded` is not [`group_count`](Self::group_count) long.
    #[must_use]
    pub fn uniform_level_from(&self, expanded: &[bool]) -> usize {
        assert_eq!(
            expanded.len(),
            self.group_count(),
            "an expansion state of another DAG's shape"
        );
        let top = self.levels.len() - 1;
        let bases = self.group_bases();
        self.levels
            .iter()
            .enumerate()
            .position(|(level, here)| expanded[bases[level]..][..here.groups.len()].contains(&true))
            .unwrap_or(top)
    }

    /// Every group of the DAG, each carrying the level it grouped — the array
    /// [`MeshLevels::uniform_level`] reads and a renderer uploads.
    ///
    /// Level order, and within a level the order [`DagLevel::groups`] holds
    /// them, so the run is reproducible and a `--check` over a cooked copy of it
    /// compares bytes rather than sets.
    ///
    /// [`MeshLevels::uniform_level`]: crate::level_select::MeshLevels#method.uniform_level
    #[must_use]
    pub fn level_groups(&self) -> Vec<LevelGroup> {
        self.levels
            .iter()
            .enumerate()
            .flat_map(|(level, here)| {
                here.groups.iter().map(move |group| LevelGroup {
                    level: u32::try_from(level)
                        .unwrap_or_else(|_| unreachable!("a DAG of a few levels")),
                    cost: GroupCost {
                        error: group.error,
                        bounds: group.bounds,
                    },
                })
            })
            .collect()
    }

    /// The clusters an "is this group expanded?" answer draws.
    ///
    /// [`cut`](Self::cut) with the projection factored out — the descent over an
    /// expansion state, which is what [`expand`](Self::expand) produces and what
    /// the GPU's state buffer holds. `expanded` is indexed in
    /// [`level_groups`](Self::level_groups) order.
    ///
    /// A cluster no group contains defaults to **not** drawn, which is only
    /// correct on the top level. That is deliberate: a level whose grouping
    /// missed a cluster leaves a hole the cover check reports by name, where a
    /// default of "drawn" would quietly produce an overlap instead.
    /// [`selection_records`](Self::selection_records) refuses such a DAG rather
    /// than encoding it, which is what keeps the two agreeing.
    ///
    /// The result is ascending by level and then by cluster, so two cuts compare
    /// without sorting.
    ///
    /// # Panics
    ///
    /// If `expanded` is not [`group_count`](Self::group_count) long.
    #[must_use]
    pub fn cut_from(&self, expanded: &[bool]) -> Vec<ClusterAt> {
        assert_eq!(
            expanded.len(),
            self.group_count(),
            "an expansion state of another DAG's shape"
        );
        let top = self.levels.len() - 1;
        let bases = self.group_bases();
        let mut drawn = Vec::new();
        for (level, here) in self.levels.iter().enumerate() {
            let count = here.clusters.clusters.len();
            let mut container = vec![level == top; count];
            for (index, group) in here.groups.iter().enumerate() {
                let open = expanded[bases[level] + index];
                for &child in &group.children {
                    container[child as usize] = open;
                }
            }

            let mut producer = vec![false; count];
            if level > 0 {
                for (index, group) in self.levels[level - 1].groups.iter().enumerate() {
                    let open = expanded[bases[level - 1] + index];
                    for &parent in &group.parents {
                        producer[parent as usize] = open;
                    }
                }
            }

            for cluster in 0..count {
                if !producer[cluster] && container[cluster] {
                    drawn.push(ClusterAt { level, cluster });
                }
            }
        }
        drawn
    }

    /// Whether `drawn` is a **crack-free cover of the surface**, and how many of
    /// its edges have a different level on either side.
    ///
    /// The property the whole DAG exists for, and the one
    /// `docs/plan/25-lod.md` calls "what its tests assert": a cut has to cover
    /// the surface exactly once, with no hole where two levels meet. Public
    /// because the host rule is no longer the only thing that produces a cut —
    /// `crcbl-vk`'s `the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`
    /// runs this over what a GPU's amplification stage actually chose, at the
    /// camera and budget the engine ships, rather than inferring the property
    /// across two facts.
    ///
    /// # How one check does both halves
    ///
    /// Every edge of the drawn triangles, keyed by the **positions** of its
    /// endpoints, must be used exactly twice — except the base mesh's own
    /// border, used once. An edge used once anywhere else is a hole: two
    /// clusters that should have met did not. An edge used three times or more
    /// is an overlap. And the two sides of an interface edge only land on the
    /// same key at all if the coarser level kept the finer level's vertices
    /// exactly, so this tests the artifact and not only its index arithmetic.
    ///
    /// The returned count is the cut's **interface** edges: those with two
    /// faces from two different levels. Zero from a cut that spans several
    /// levels means the levels never meet, which is two meshes rather than one
    /// cut, so a caller checking for a mixed cut checks this too.
    ///
    /// # Errors
    ///
    /// [`CutFault`] naming what is wrong and sampling a few of the edges that
    /// are wrong that way.
    ///
    /// # Panics
    ///
    /// If a [`ClusterAt`] names a level or a cluster this DAG does not have.
    /// That is a caller handing over indices from another mesh, not a property
    /// of any cut.
    pub fn check_cover(&self, drawn: &[ClusterAt]) -> Result<usize, CutFault> {
        let border = base_border(&self.levels[0]);
        let edges = cut_edges(self, drawn);
        let single: BTreeSet<SharedEdge> = edges
            .iter()
            .filter(|&(_, levels)| levels.len() == 1)
            .map(|(&edge, _)| edge)
            .collect();

        // **Overlap first, then holes.** A cut that draws a region twice makes
        // its border edges two-faced as well, so testing coverage first would
        // report an overlap as a missing border and name the wrong defect. The
        // three are mutually exclusive on a sound cut, so the order only decides
        // which one a broken cut is *called*.
        let crowded = || {
            edges
                .iter()
                .filter(|&(_, levels)| levels.len() > 2)
                .map(|(&edge, _)| edge)
        };
        if crowded().next().is_some() {
            return Err(CutFault {
                what: "an edge has three faces on it, which is two clusters overlapping",
                edges: crowded().count(),
                sample: crowded().take(FAULT_SAMPLE).map(edge_positions).collect(),
            });
        }
        let holes = || single.difference(&border).copied();
        if holes().next().is_some() {
            return Err(CutFault {
                what: "an edge has one face where the mesh is closed, which is a hole",
                edges: holes().count(),
                sample: holes().take(FAULT_SAMPLE).map(edge_positions).collect(),
            });
        }
        let missing = || border.difference(&single).copied();
        if missing().next().is_some() {
            return Err(CutFault {
                what: "an edge of the mesh's own border has two faces or none, so the \
                       cut does not cover the whole surface",
                edges: missing().count(),
                sample: missing().take(FAULT_SAMPLE).map(edge_positions).collect(),
            });
        }

        Ok(edges
            .values()
            .filter(|levels| levels.len() == 2 && levels[0] != levels[1])
            .count())
    }

    /// The DAG resolved into one [`ClusterSelect`] per cluster, level by level —
    /// what a GPU reads to run [`cut`](Self::cut) with no communication.
    ///
    /// `level_vertex_bases[depth]` is where that level's vertices start relative
    /// to the mesh the instance names, which is level 0's: a level is decimated
    /// separately and its vertices belong to no vertex of the level below, so a
    /// DAG is several vertex ranges and a cluster has to say which of them is
    /// its own. See [`ClusterSelect::vertex_base`].
    ///
    /// `first_group` is where this DAG's groups start in the renderer's shared
    /// [`LevelGroup`] array, and it is added to every index written — so the
    /// records name the same groups the per-(instance, group) state is keyed by.
    /// A caller holding one DAG on its own passes zero, which is
    /// [`level_groups`](Self::level_groups) order verbatim.
    ///
    /// # Panics
    ///
    /// If `level_vertex_bases` is not one entry per level, or if the DAG's
    /// grouping does not cover it: every cluster below the top level must be in
    /// exactly one group, and every cluster above level 0 must have been
    /// produced by one. Those are what make the encoded rule the same rule
    /// [`cut`](Self::cut) runs — an uncovered cluster is a hole this
    /// encoding would silently *draw* — and a DAG that fails them is a bake
    /// defect rather than a runtime condition.
    #[must_use]
    pub fn selection_records(
        &self,
        level_vertex_bases: &[u32],
        first_group: u32,
    ) -> Vec<Vec<ClusterSelect>> {
        assert_eq!(
            level_vertex_bases.len(),
            self.levels.len(),
            "a vertex base is needed for every level"
        );
        let top = self.levels.len() - 1;
        let bases = self.group_bases();
        let named = |level: usize, index: usize| {
            first_group
                + u32::try_from(bases[level] + index)
                    .unwrap_or_else(|_| unreachable!("a DAG of a few dozen groups"))
        };

        let mut out = Vec::with_capacity(self.levels.len());
        for (depth, level) in self.levels.iter().enumerate() {
            let mut records = vec![
                ClusterSelect {
                    // The level is packed into the spare `flags` bits here, at
                    // the one place that knows it: the two group flags are
                    // OR-ed in below and occupy bits 0 and 1.
                    flags: ClusterSelect::level_bits(
                        u32::try_from(depth)
                            .unwrap_or_else(|_| unreachable!("a DAG of a few dozen levels")),
                    ),
                    vertex_base: level_vertex_bases[depth],
                    ..ClusterSelect::ALWAYS
                };
                level.clusters.clusters.len()
            ];
            for (index, group) in level.groups.iter().enumerate() {
                for &child in &group.children {
                    let record = &mut records[child as usize];
                    record.flags |= ClusterSelect::HAS_CONTAINER;
                    record.container_group = named(depth, index);
                }
            }
            if depth > 0 {
                for (index, group) in self.levels[depth - 1].groups.iter().enumerate() {
                    for &parent in &group.parents {
                        let record = &mut records[parent as usize];
                        record.flags |= ClusterSelect::HAS_PRODUCER;
                        record.producer_group = named(depth - 1, index);
                    }
                }
            }

            for (index, record) in records.iter().enumerate() {
                assert!(
                    depth == top || record.flags & ClusterSelect::HAS_CONTAINER != 0,
                    "level {depth} cluster {index} is in no group, so the descent \
                     would draw it beside the parents that cover it"
                );
                assert!(
                    depth == 0 || record.flags & ClusterSelect::HAS_PRODUCER != 0,
                    "level {depth} cluster {index} was produced by no group, so it \
                     carries no error to be judged by"
                );
            }
            out.push(records);
        }
        out
    }
}

/// Why a committed DAG could not be decoded.
///
/// A hand-written `Display` rather than a `thiserror` derive, because this crate
/// has no dependencies at all — the same decision `crate::sha256` and
/// [`MeshletTooLarge`](crate::meshlet::MeshletTooLarge) record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagDecodeError {
    /// What the decoder was reading when it gave up.
    pub what: &'static str,
    /// Byte offset it had reached.
    pub at: usize,
}

impl std::fmt::Display for DagDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl std::error::Error for DagDecodeError {}

/// What every artifact this codec writes begins with, so a file that is not one
/// is refused rather than read as numbers.
const MAGIC: [u8; 8] = *b"CRCBLDAG";

/// The format's version, bumped whenever the byte layout below changes.
///
/// A committed artifact and its decoder move in one commit, so this cannot
/// legitimately mismatch — it is here for the case that does happen: a stale
/// artifact arriving through a merge, which would otherwise decode as garbage
/// counts and allocate on them.
const VERSION: u32 = 1;

impl ClusterDag {
    /// The bytes this DAG is committed as.
    ///
    /// Little-endian scalars throughout, no alignment padding except the corner
    /// array's tail, and every array preceded by its own count — so the decoder
    /// is a forward walk that never seeks and never trusts a length it has not
    /// read. The clusters are written with [`Meshlet::to_bytes`], which is the
    /// layout the GPU reads them in, so the artifact holds the record rather
    /// than a second spelling of it.
    ///
    /// **Level 0's positions are not written.** They are the array the builder
    /// was handed, which the consumer already has; `dunes_dag` supplies
    /// [`dunes::positions`] and the generator asserts the two agree.
    ///
    /// # Panics
    ///
    /// If a level's [`errors`](DagLevel::errors) or [`bounds`](DagLevel::bounds)
    /// is not parallel to its clusters. Those two are the one pair of arrays
    /// written without a count of their own — they are per cluster by
    /// definition — so a mismatch here is an artifact whose errors are read off
    /// by one and a DAG that selects the wrong level everywhere. It is a bake
    /// failure rather than a runtime condition, and it refuses rather than
    /// writing the file.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        push_count(&mut bytes, self.levels.len());
        for (depth, level) in self.levels.iter().enumerate() {
            // Level 0's positions are the caller's own array; every other
            // level's are the decimator's output and have nowhere else to live.
            let positions: &[[f32; 3]] = if depth == 0 { &[] } else { &level.positions };
            push_count(&mut bytes, positions.len());
            for position in positions {
                push_floats(&mut bytes, position);
            }

            assert!(
                level.errors.len() == level.clusters.clusters.len()
                    && level.bounds.len() == level.clusters.clusters.len(),
                "level {depth} has {} clusters but {} errors and {} spheres",
                level.clusters.clusters.len(),
                level.errors.len(),
                level.bounds.len()
            );
            push_count(&mut bytes, level.clusters.clusters.len());
            for cluster in &level.clusters.clusters {
                bytes.extend_from_slice(&cluster.to_bytes());
            }
            push_count(&mut bytes, level.clusters.vertices.len());
            for &vertex in &level.clusters.vertices {
                bytes.extend_from_slice(&vertex.to_le_bytes());
            }
            push_count(&mut bytes, level.clusters.corners.len());
            bytes.extend_from_slice(&crate::meshlet::corner_bytes(&level.clusters.corners));

            push_floats(&mut bytes, &level.errors);
            for sphere in &level.bounds {
                push_sphere(&mut bytes, *sphere);
            }

            push_count(&mut bytes, level.groups.len());
            for group in &level.groups {
                push_count(&mut bytes, group.children.len());
                for &child in &group.children {
                    bytes.extend_from_slice(&child.to_le_bytes());
                }
                push_count(&mut bytes, group.parents.len());
                for &parent in &group.parents {
                    bytes.extend_from_slice(&parent.to_le_bytes());
                }
                bytes.extend_from_slice(&group.error.to_le_bytes());
                push_sphere(&mut bytes, group.bounds);
            }
        }
        bytes
    }

    /// The inverse of [`to_bytes`](Self::to_bytes), with level 0's positions
    /// supplied by the caller.
    ///
    /// # Errors
    ///
    /// [`DagDecodeError`] naming what ran out of bytes, for a truncated or
    /// corrupt artifact. A count that would address past the end of the input is
    /// refused before anything is allocated on it, so a hostile length cannot
    /// reserve memory the file never had.
    ///
    /// Also for a `base_positions` that is not the array this artifact was cooked
    /// over — `check_indices_are_in_range` is what turns that from an
    /// out-of-bounds panic in whoever walks the clusters into a refusal here.
    pub fn from_bytes(bytes: &[u8], base_positions: Vec<[f32; 3]>) -> Result<Self, DagDecodeError> {
        let mut reader = Reader { bytes, at: 0 };
        if reader.take("the magic", MAGIC.len())? != MAGIC {
            return Err(DagDecodeError {
                what: "the magic is not a cooked cluster DAG",
                at: 0,
            });
        }
        let version = reader.u32("the version")?;
        if version != VERSION {
            return Err(DagDecodeError {
                what: "the version is not the one this decoder writes",
                at: MAGIC.len(),
            });
        }

        // Every level writes five counts before anything else, and every group
        // two counts, an error and a sphere. Those are the smallest a level and
        // a group can be, which is what makes the capacity bounds below real
        // ones rather than decoration.
        const LEVEL_MINIMUM: usize = 5 * 4;
        const GROUP_MINIMUM: usize = 2 * 4 + 4 + 16;

        let level_count = reader.count("the level count", LEVEL_MINIMUM)?;
        let mut base_positions = Some(base_positions);
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            let decoded = reader.array("a position", 3 * 4, |reader| {
                Ok([
                    reader.f32("a position")?,
                    reader.f32("a position")?,
                    reader.f32("a position")?,
                ])
            })?;
            // Level 0 alone was written with no positions, because they are the
            // caller's own array; `base_positions` is therefore taken exactly
            // once and every later level uses what it decoded.
            let positions = match base_positions.take() {
                Some(base) => base,
                None => decoded,
            };

            let clusters = reader.array("a cluster", MESHLET_STRIDE, |reader| {
                let element = reader.take("a cluster", MESHLET_STRIDE)?;
                let element: &[u8; MESHLET_STRIDE] = element
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("take returned the length asked for"));
                Ok(Meshlet::from_bytes(element))
            })?;
            let vertices = reader.array("a cluster vertex", 4, |reader| {
                reader.u32("a cluster vertex")
            })?;
            let corner_count = reader.count("the corner count", 1)?;
            let corners =
                reader.take("the corners", corner_count.div_ceil(4) * 4)?[..corner_count].to_vec();

            let errors = (0..clusters.len())
                .map(|_| reader.f32("a cluster error"))
                .collect::<Result<Vec<f32>, _>>()?;
            let bounds = (0..clusters.len())
                .map(|_| reader.sphere("a cluster sphere"))
                .collect::<Result<Vec<GroupBounds>, _>>()?;

            let group_count = reader.count("the group count", GROUP_MINIMUM)?;
            let mut groups = Vec::with_capacity(group_count);
            for _ in 0..group_count {
                let children =
                    reader.array("a group child", 4, |reader| reader.u32("a group child"))?;
                let parents =
                    reader.array("a group parent", 4, |reader| reader.u32("a group parent"))?;
                groups.push(ClusterGroup {
                    children,
                    parents,
                    error: reader.f32("a group error")?,
                    bounds: reader.sphere("a group sphere")?,
                });
            }

            let level = DagLevel {
                positions,
                clusters: MeshClusters {
                    clusters,
                    vertices,
                    corners,
                },
                errors,
                bounds,
                groups,
            };
            check_indices_are_in_range(&level, reader.at)?;
            levels.push(level);
        }

        if reader.at != bytes.len() {
            return Err(DagDecodeError {
                what: "there are bytes after the last level",
                at: reader.at,
            });
        }
        Ok(Self { levels })
    }
}

/// Every index a decoded level holds names something that level has.
///
/// **This is what makes [`ClusterDag::from_bytes`] safe to hand a wrong
/// `base_positions`.** Level 0's positions come from the caller rather than from
/// the file, so an array of the wrong length decodes without complaint and then
/// panics — or draws another mesh's geometry — in whoever walks the clusters.
/// The three ranges are the three ways that shows up, and each is checked here
/// rather than left as a precondition a caller has no way to satisfy: a
/// cluster's run inside the vertex array, a cluster's corners inside the corner
/// array, and every entry of the run inside the positions.
///
/// `at` is the reader's position, so the error points at the level that failed.
fn check_indices_are_in_range(level: &DagLevel, at: usize) -> Result<(), DagDecodeError> {
    let refuse = |what| Err(DagDecodeError { what, at });
    for cluster in &level.clusters.clusters {
        let run = (cluster.vertex_offset as usize)..;
        let Some(run) = level
            .clusters
            .vertices
            .get(run)
            .and_then(|run| run.get(..cluster.vertex_count as usize))
        else {
            return refuse("a cluster's vertex run is not inside the vertex array");
        };
        if level
            .clusters
            .corners
            .get(cluster.triangle_offset as usize..)
            .and_then(|corners| corners.get(..cluster.triangle_count as usize * 3))
            .is_none()
        {
            return refuse("a cluster's corners are not inside the corner array");
        }
        if run
            .iter()
            .any(|&vertex| vertex as usize >= level.positions.len())
        {
            return refuse("a cluster names a vertex the level does not have");
        }
    }
    // A corner indexes its own cluster's run, so the bound is the run's length
    // and not the array's — which is the whole difference between a corner and
    // an index, and a corner past its run reads a neighbouring cluster's vertex.
    for cluster in &level.clusters.clusters {
        let corners = &level.clusters.corners[cluster.triangle_offset as usize..]
            [..cluster.triangle_count as usize * 3];
        if corners
            .iter()
            .any(|&corner| u32::from(corner) >= cluster.vertex_count)
        {
            return refuse("a corner reaches past its own cluster's vertex run");
        }
    }
    Ok(())
}

fn push_count(bytes: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count)
        .unwrap_or_else(|_| unreachable!("a bake that produced 4 billion of anything"));
    bytes.extend_from_slice(&count.to_le_bytes());
}

fn push_floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_sphere(bytes: &mut Vec<u8>, sphere: GroupBounds) {
    push_floats(bytes, &sphere.center);
    bytes.extend_from_slice(&sphere.radius.to_le_bytes());
}

/// A forward walk over the artifact that refuses to read past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, what: &'static str, length: usize) -> Result<&'a [u8], DagDecodeError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(DagDecodeError { what, at: self.at })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(DagDecodeError { what, at: self.at })?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, DagDecodeError> {
        let word = self.take(what, 4)?;
        Ok(u32::from_le_bytes(word.try_into().unwrap_or_else(|_| {
            unreachable!("take returned the length asked for")
        })))
    }

    fn f32(&mut self, what: &'static str) -> Result<f32, DagDecodeError> {
        Ok(f32::from_bits(self.u32(what)?))
    }

    fn sphere(&mut self, what: &'static str) -> Result<GroupBounds, DagDecodeError> {
        Ok(GroupBounds {
            center: [self.f32(what)?, self.f32(what)?, self.f32(what)?],
            radius: self.f32(what)?,
        })
    }

    /// A count, refused unless `stride` bytes of it each could still be read.
    ///
    /// **The bound is the point**: without it a corrupt count reserves capacity
    /// the file never had, which is an allocation decided by the file rather
    /// than by the reader. It is not a validity check — the elements are read
    /// and can still run out — it is what keeps the failure a decode error
    /// instead of an out-of-memory abort.
    fn count(&mut self, what: &'static str, stride: usize) -> Result<usize, DagDecodeError> {
        let count = self.u32(what)? as usize;
        let available = self.bytes.len() - self.at;
        if count.saturating_mul(stride) > available {
            return Err(DagDecodeError { what, at: self.at });
        }
        Ok(count)
    }

    /// A counted array of `stride`-byte elements.
    fn array<T>(
        &mut self,
        what: &'static str,
        stride: usize,
        mut element: impl FnMut(&mut Self) -> Result<T, DagDecodeError>,
    ) -> Result<Vec<T>, DagDecodeError> {
        let count = self.count(what, stride)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(element(self)?);
        }
        Ok(values)
    }
}

/// An edge named by where its endpoints *are* rather than by which vertex they
/// were, so it is the same edge on either side of a level change.
///
/// Two levels have unrelated vertex numbering and unrelated index buffers, so an
/// index pair cannot say whether they meet. A bit pattern can, and exactly: a
/// vertex the decimator never moved comes through the level change unchanged, so
/// the coarser level's copy of a locked boundary vertex is bit-identical to the
/// finer level's. Comparing bits rather than distances is what makes "they meet"
/// mean *meet*, with no tolerance to tune.
type SharedEdge = [[u32; 3]; 2];

fn shared_edge(positions: &[[f32; 3]], a: u32, b: u32) -> SharedEdge {
    let mut edge = [positions[a as usize], positions[b as usize]].map(|p| p.map(f32::to_bits));
    edge.sort_unstable();
    edge
}

/// A [`SharedEdge`] back as the two positions it was keyed on, for a message a
/// reader can locate on the mesh.
fn edge_positions(edge: SharedEdge) -> [[f32; 3]; 2] {
    edge.map(|point| point.map(f32::from_bits))
}

/// One cluster's triangles, as indices into its level's positions.
fn cluster_triangles(clusters: &MeshClusters, cluster: usize) -> Vec<[u32; 3]> {
    let cluster = &clusters.clusters[cluster];
    let run = &clusters.vertices[cluster.vertex_offset as usize..][..cluster.vertex_count as usize];
    clusters.corners[cluster.triangle_offset as usize..][..cluster.triangle_count as usize * 3]
        .chunks_exact(3)
        .map(|face| [0, 1, 2].map(|corner| run[usize::from(face[corner])]))
        .collect()
}

/// Every edge of a cut's triangles, by position, and the levels of the clusters
/// that carry it.
fn cut_edges(dag: &ClusterDag, drawn: &[ClusterAt]) -> BTreeMap<SharedEdge, Vec<usize>> {
    let mut edges: BTreeMap<SharedEdge, Vec<usize>> = BTreeMap::new();
    for &ClusterAt { level, cluster } in drawn {
        let positions = &dag.levels[level].positions;
        for face in cluster_triangles(&dag.levels[level].clusters, cluster) {
            for corner in 0..3 {
                let edge = shared_edge(positions, face[corner], face[(corner + 1) % 3]);
                edges.entry(edge).or_default().push(level);
            }
        }
    }
    edges
}

/// The base mesh's own boundary loop, by position: the edges a cut is *supposed*
/// to leave with one face. An open sheet has its outer ring here and a closed
/// shape has nothing.
///
/// Taken off level 0 rather than from the caller's own arrays, because level 0
/// *is* the base mesh — `every_level_decodes_to_triangles_over_its_own_positions`
/// is what says so — and an edge count does not care that the clusters permuted
/// the triangles.
fn base_border(level: &DagLevel) -> BTreeSet<SharedEdge> {
    let mut uses: BTreeMap<SharedEdge, usize> = BTreeMap::new();
    for face in level.indices().chunks_exact(3) {
        for corner in 0..3 {
            *uses
                .entry(shared_edge(
                    &level.positions,
                    face[corner],
                    face[(corner + 1) % 3],
                ))
                .or_default() += 1;
        }
    }
    uses.into_iter()
        .filter(|&(_, uses)| uses != 2)
        .map(|(edge, _)| edge)
        .collect()
}

/// Why a set of clusters is not a crack-free cover of the surface.
///
/// A hand-written `Display` rather than a `thiserror` derive, because this crate
/// has no dependencies at all — the same decision [`DagDecodeError`] records.
///
/// `PartialEq` but not `Eq`: [`sample`](Self::sample) holds floats.
#[derive(Clone, Debug, PartialEq)]
pub struct CutFault {
    /// What is wrong with it, in one phrase.
    pub what: &'static str,
    /// How many of the cut's edges are wrong that way.
    pub edges: usize,
    /// A few of them, as the positions of their two endpoints.
    ///
    /// A sample rather than all of them: a patch's border is hundreds of edges
    /// and printing every one buries the handful that actually differ.
    pub sample: Vec<[[f32; 3]; 2]>,
}

impl std::fmt::Display for CutFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} edge(s), first {:?}",
            self.what, self.edges, self.sample
        )
    }
}

impl std::error::Error for CutFault {}

/// How many edges of a sound cut go unreported when [`ClusterDag::check_cover`]
/// samples a fault.
const FAULT_SAMPLE: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meshlet::ClusterBounds;

    /// How many pixels one unit of length subtends one unit from the eye, for
    /// the frames this workspace's goldens are drawn at: a 192-pixel-high
    /// viewport at a 60-degree vertical field of view.
    ///
    /// Written as a literal rather than `0.5 * 192.0 / (PI / 6.0).tan()` because
    /// `tanf` is not correctly rounded and differs in the last place between
    /// libms — the same argument [`crate::dunes`] makes about `sinf` — and the
    /// counts below are pinned by equality.
    const PIXELS_PER_UNIT: f32 = 166.0;

    /// Every threshold at which a cut changes, plus one past the last, so a
    /// sweep visits every distinct cut a global error budget admits.
    fn every_threshold(dag: &ClusterDag) -> Vec<f32> {
        let mut errors: Vec<f32> = dag
            .levels
            .iter()
            .flat_map(|level| level.errors.iter().copied())
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

    /// The committed artifact is exactly what this codec writes: decoding it and
    /// encoding the result reproduces the file byte for byte.
    ///
    /// This is the half of the arrangement that runs with no `crcbl-scene` in
    /// sight. `tools/cook-clusters.rs --check` is the other half and the one
    /// that can tell whether the *builder* still produces it; this one catches a
    /// decoder that reads the file into something else and hands it on.
    #[test]
    fn the_committed_artifact_round_trips_through_the_codec() {
        let dag = dunes_dag();
        assert!(
            dag.levels.len() > 3,
            "a DAG of {} levels proves little",
            dag.levels.len()
        );
        assert_eq!(
            dag.to_bytes(),
            include_bytes!("../clusters/dunes.dag"),
            "the committed bytes are not what this codec writes for what it read"
        );
        assert_eq!(
            dag.levels[0].positions,
            dunes::positions(),
            "level 0 is the model"
        );
    }

    /// Every level's clusters describe triangles over that level's own
    /// positions, and level 0's are the model's own triangles.
    ///
    /// An offset short by one cluster, a corner run that restarted in the wrong
    /// place, or a run length read off the wrong field each survive a
    /// byte-comparison against a file written with the same mistake, and none
    /// of them survive this.
    #[test]
    fn every_level_decodes_to_triangles_over_its_own_positions() {
        let dag = dunes_dag();
        for (depth, level) in dag.levels.iter().enumerate() {
            let mut triangles = 0usize;
            for cluster in 0..level.clusters.clusters.len() {
                for face in cluster_triangles(&level.clusters, cluster) {
                    assert!(
                        face.iter()
                            .all(|&index| (index as usize) < level.positions.len()),
                        "level {depth} cluster {cluster} names a vertex the level does not have"
                    );
                    assert!(
                        face[0] != face[1] && face[1] != face[2] && face[0] != face[2],
                        "level {depth} cluster {cluster} has a degenerate triangle {face:?}"
                    );
                    triangles += 1;
                }
            }
            assert_eq!(
                triangles,
                level
                    .clusters
                    .clusters
                    .iter()
                    .map(|cluster| cluster.triangle_count as usize)
                    .sum::<usize>(),
                "level {depth} decoded a different number of triangles than its clusters claim"
            );
            assert_eq!(level.errors.len(), level.clusters.clusters.len());
            assert_eq!(level.bounds.len(), level.clusters.clusters.len());
        }

        // Level 0 is the model's own triangles, permuted into cluster order.
        let mut decoded: Vec<[u32; 3]> = (0..dag.levels[0].clusters.clusters.len())
            .flat_map(|cluster| cluster_triangles(&dag.levels[0].clusters, cluster))
            .collect();
        let mut expected: Vec<[u32; 3]> = dunes::indices()
            .chunks_exact(3)
            .map(|face| [face[0], face[1], face[2]])
            .collect();
        decoded.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            decoded, expected,
            "level 0 is not the dunes patch's triangles"
        );
    }

    /// **The DAG gets coarser at every level**, which every other invariant here
    /// is satisfied by a builder that never simplified at all.
    #[test]
    fn the_dag_gets_coarser_at_every_level() {
        let dag = dunes_dag();
        let triangles: Vec<u32> = dag
            .levels
            .iter()
            .map(|level| {
                level
                    .clusters
                    .clusters
                    .iter()
                    .map(|cluster| cluster.triangle_count)
                    .sum()
            })
            .collect();
        let clusters: Vec<usize> = dag
            .levels
            .iter()
            .map(|level| level.clusters.clusters.len())
            .collect();
        for pair in triangles.windows(2) {
            assert!(pair[1] < pair[0], "triangles {triangles:?} do not shrink");
        }
        for pair in clusters.windows(2) {
            assert!(pair[1] < pair[0], "clusters {clusters:?} do not shrink");
        }
    }

    /// **Error never decreases up the DAG, and every cluster a group produced
    /// carries that group's number.**
    ///
    /// Monotonic error is what makes a cut well defined: along the groups
    /// covering any one region the errors never fall, so the descent's test
    /// holds at exactly one of them. Lose it and a cluster can be drawn while an
    /// ancestor covering it is drawn too, which is an overlap rather than a cut.
    #[test]
    fn the_error_never_decreases_up_the_dag() {
        let dag = dunes_dag();
        assert!(
            dag.levels[0].errors.iter().all(|&error| error == 0.0),
            "level 0 is the base mesh untouched and must read zero"
        );
        let mut groups = 0usize;
        for (depth, level) in dag.levels.iter().enumerate() {
            for (index, group) in level.groups.iter().enumerate() {
                assert!(
                    !group.children.is_empty(),
                    "level {depth} group {index} is empty"
                );
                assert!(
                    !group.parents.is_empty(),
                    "level {depth} group {index} produced nothing"
                );
                for &child in &group.children {
                    assert!(
                        group.error >= level.errors[child as usize],
                        "level {depth} group {index} costs {} and its child {child} already \
                         cost {}",
                        group.error,
                        level.errors[child as usize]
                    );
                }
                for &parent in &group.parents {
                    assert_eq!(
                        dag.levels[depth + 1].errors[parent as usize],
                        group.error,
                        "level {depth} group {index}'s parent {parent} carries an error \
                         that is not the group's"
                    );
                    assert_eq!(
                        dag.levels[depth + 1].bounds[parent as usize],
                        group.bounds,
                        "level {depth} group {index}'s parent {parent} carries a sphere \
                         that is not the group's"
                    );
                }
                groups += 1;
            }
        }
        assert!(groups > 0, "no group was checked, so nothing was");
    }

    /// **A group's sphere contains every sphere below it**, which is what
    /// carries monotone stored error into monotone *projected* error.
    ///
    /// A containing sphere is never further from any eye than one inside it, so
    /// `error / distance` rises up the DAG for every eye there is and the
    /// descent has one stopping point per branch whatever the camera does.
    /// Without it a closer group can project larger from a smaller error, and
    /// the cut stops being a cut.
    #[test]
    fn a_group_sphere_contains_every_sphere_below_it() {
        let dag = dunes_dag();
        let mut checked = 0usize;
        for (depth, level) in dag.levels.iter().enumerate() {
            for (index, group) in level.groups.iter().enumerate() {
                for &child in &group.children {
                    assert!(
                        group.bounds.contains(level.bounds[child as usize]),
                        "level {depth} group {index}'s sphere {:?} does not contain its \
                         child {child}'s {:?}",
                        group.bounds,
                        level.bounds[child as usize]
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no containment was checked");
    }

    /// **Every cut a global error budget admits is a crack-free cover of the
    /// surface**, over a sweep that visits each distinct one.
    #[test]
    fn every_error_budget_draws_a_crack_free_cut() {
        let dag = dunes_dag();
        let thresholds = every_threshold(&dag);
        assert!(
            thresholds.len() > 3,
            "a sweep of {thresholds:?} visits too few cuts"
        );

        let mut mixed = 0usize;
        for threshold in &thresholds {
            let drawn = dag.cut_from(&dag.states(|group| *threshold < group.error));
            let interfaces = dag
                .check_cover(&drawn)
                .unwrap_or_else(|fault| panic!("a budget of {threshold} draws a cut with {fault}"));
            let levels: BTreeSet<usize> = drawn.iter().map(|at| at.level).collect();
            if levels.len() > 1 {
                mixed += 1;
                assert!(
                    interfaces > 0,
                    "a cut spanning {levels:?} whose levels never meet is not a mixed cut, \
                     it is two meshes"
                );
            }
        }
        assert!(
            mixed > 0,
            "no error budget drew more than one level, so the sweep never exercised an \
             interface between two"
        );
    }

    /// **Every camera position draws a crack-free cut**, and most of them draw
    /// more than one level.
    ///
    /// This is the projected-error rule an amplification stage will run, which
    /// is where a per-cluster distance term would tear a group in half. Running
    /// it over the cooked artifact rather than over an in-memory DAG is the
    /// point: it is the committed bytes that reach the renderer.
    #[test]
    fn the_cooked_dag_draws_a_crack_free_cut_from_every_camera() {
        let dag = dunes_dag();

        let (mut mixed, mut cuts) = (0usize, 0usize);
        for eye in EYES {
            for budget in BUDGETS {
                let drawn = dag.cut(eye, PIXELS_PER_UNIT, budget);
                let interfaces = dag.check_cover(&drawn).unwrap_or_else(|fault| {
                    panic!("an eye at {eye:?} under {budget} draws a cut with {fault}")
                });
                let levels: BTreeSet<usize> = drawn.iter().map(|at| at.level).collect();
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

    /// **The uniform cut's level is the finest level the per-cluster cut
    /// draws**, at every eye and every budget of the sweep.
    ///
    /// This is what makes `docs/plan/25-lod.md`'s two granularities one
    /// hierarchy and one metric rather than two answers that happen to look
    /// alike: the coarse decision is not an approximation of the fine one, it is
    /// the fine one's own floor. [`crate::level_select`]'s module docs carry the
    /// argument; this runs it over the committed artifact, and `crcbl-vk`'s
    /// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is` runs it
    /// across two real devices.
    ///
    /// The sweep is asserted to have produced several distinct levels, because
    /// a comparison in which both sides always answered zero would pass with
    /// neither of them working.
    #[test]
    fn the_uniform_level_is_the_finest_level_the_per_cluster_cut_draws() {
        let dag = dunes_dag();
        let mut seen = BTreeSet::new();
        for eye in EYES {
            for budget in BUDGETS {
                let drawn = dag.cut(eye, PIXELS_PER_UNIT, budget);
                let finest = drawn
                    .iter()
                    .map(|at| at.level)
                    .min()
                    .expect("a cut always draws something");
                let uniform = dag.uniform_level(eye, PIXELS_PER_UNIT, budget);
                assert_eq!(
                    uniform, finest,
                    "from {eye:?} under {budget} the uniform cut takes level {uniform} and \
                     the per-cluster cut's finest is {finest}"
                );
                seen.insert(uniform);
            }
        }
        assert!(
            seen.len() > 2,
            "the sweep chose {seen:?}, which is too few distinct levels for the equality \
             to have been tested at more than one of them"
        );
    }

    /// **Every uniform cut is a crack-free cover of the surface**, asserted
    /// rather than argued from "it is one whole level".
    ///
    /// The argument is sound — a level's clusters partition the surface, so no
    /// two detail levels meet anywhere — but it is an argument about a builder
    /// this crate does not run, where [`ClusterDag::check_cover`] is a
    /// measurement over the committed bytes. A level whose re-split dropped a
    /// triangle passes the argument and fails this.
    ///
    /// The interface count is asserted to be zero for the same reason it is
    /// asserted to be positive for a mixed cut: a uniform cut with an interface
    /// edge would be one holding two levels, which is not what was drawn.
    #[test]
    fn every_uniform_cut_is_a_crack_free_cover() {
        let dag = dunes_dag();
        for (level, here) in dag.levels.iter().enumerate() {
            let whole: Vec<ClusterAt> = (0..here.clusters.clusters.len())
                .map(|cluster| ClusterAt { level, cluster })
                .collect();
            let interfaces = dag
                .check_cover(&whole)
                .unwrap_or_else(|fault| panic!("level {level} drawn whole has {fault}"));
            assert_eq!(
                interfaces, 0,
                "level {level} drawn whole reports {interfaces} interface edge(s), so it \
                 holds more than one level"
            );
        }
    }

    /// **The thing per-cluster selection is for**, in numbers: one draw of one
    /// mesh whose near end is at a finer level than its far end.
    ///
    /// The distance term is what produces it. A level's stored errors barely
    /// vary within the level — the decimator targets a triangle count, so a
    /// level's error frontier is flat — and a cut reading them alone would be
    /// uniform at every camera. Split the drawn clusters by how far up the patch
    /// they sit and the two ends report different levels.
    ///
    /// The histograms are pinned rather than compared loosely, because "the near
    /// end is finer" is satisfied by a cut one cluster away from uniform, and
    /// that is not the claim.
    #[test]
    fn a_receding_patch_draws_its_near_end_finer_than_its_far_end() {
        let dag = dunes_dag();
        let eye = EYES[0];
        let drawn = dag.cut(eye, PIXELS_PER_UNIT, MIXING_BUDGET);

        let mut near: BTreeMap<usize, usize> = BTreeMap::new();
        let mut far: BTreeMap<usize, usize> = BTreeMap::new();
        for &ClusterAt { level, cluster } in &drawn {
            let depth = dag.levels[level].clusters.clusters[cluster].bounds.center[2];
            if depth < NEAR_THIRD {
                *near.entry(level).or_default() += 1;
            } else if depth > FAR_THIRD {
                *far.entry(level).or_default() += 1;
            }
        }
        assert_eq!(
            near,
            NEAR_HISTOGRAM.iter().copied().collect::<BTreeMap<_, _>>(),
            "the near third of the patch is not the levels it was cooked to be"
        );
        assert_eq!(
            far,
            FAR_HISTOGRAM.iter().copied().collect::<BTreeMap<_, _>>(),
            "the far third of the patch is not the levels it was cooked to be"
        );
        assert!(
            near.keys().min() < far.keys().min(),
            "the near end {near:?} is not drawn finer than the far end {far:?}"
        );

        // And the two ends are one cut of one mesh, not two pictures.
        let interfaces = dag
            .check_cover(&drawn)
            .unwrap_or_else(|fault| panic!("the mixing budget draws a cut with {fault}"));
        assert!(
            interfaces > 20,
            "only {interfaces} edges have one level on one side and another on the other, \
             which is too few to be the seam between the two ends"
        );
    }

    /// **The per-cluster encoding is the same rule as the descent**, over every
    /// camera and budget the sweep visits.
    ///
    /// [`ClusterDag::cut_from`] walks the DAG level by level with both groups
    /// in hand; [`ClusterSelect::is_drawn`] answers for one cluster out of two
    /// group indices, which is what a task group has. They are the same
    /// statement only while the resolution in
    /// [`ClusterDag::selection_records`] put the right group in each half, and
    /// the failure that would produce — a container named where a producer
    /// belongs — inverts the descent rather than crashing.
    #[test]
    fn the_per_cluster_records_draw_the_cut_the_descent_does() {
        let dag = dunes_dag();
        let bases: Vec<u32> = (0..dag.levels.len() as u32).collect();
        let records = dag.selection_records(&bases, 0);
        assert_eq!(records.len(), dag.levels.len());
        for (depth, level) in dag.levels.iter().enumerate() {
            assert_eq!(records[depth].len(), level.clusters.clusters.len());
            assert!(
                records[depth]
                    .iter()
                    .all(|record| record.vertex_base == bases[depth]),
                "level {depth}'s records did not take the base they were given"
            );
            // The level rides in the spare `flags` bits, so this also asserts
            // the two group flags did not land on top of it: `HAS_CONTAINER` is
            // OR-ed into most of these records after the level is packed.
            assert!(
                records[depth]
                    .iter()
                    .all(|record| record.level() as usize == depth),
                "level {depth}'s records did not carry their own level"
            );
        }

        let mut compared = 0usize;
        for eye in EYES {
            for budget in BUDGETS {
                let expected = dag.cut(eye, PIXELS_PER_UNIT, budget);
                let state = dag.expand(
                    eye,
                    PIXELS_PER_UNIT,
                    LodBudgets::sharp(budget),
                    &vec![false; dag.group_count()],
                );
                let mut drawn = Vec::new();
                for (level, here) in records.iter().enumerate() {
                    for (cluster, record) in here.iter().enumerate() {
                        if record.is_drawn(&state) {
                            drawn.push(ClusterAt { level, cluster });
                        }
                    }
                }
                assert_eq!(
                    drawn, expected,
                    "the per-cluster records draw a different cut from {eye:?} \
                     under {budget}"
                );
                assert!(!drawn.is_empty(), "an empty cut compares equal to nothing");
                compared += 1;
            }
        }
        assert_eq!(compared, EYES.len() * BUDGETS.len());
    }

    /// **The cover check refuses a torn cut**, on each of the two ways a cut
    /// tears — because a checker that passes whatever it is handed is worse
    /// than no checker at all, and every cut this module produces is sound by
    /// construction, so nothing else here would ever make it fire.
    ///
    /// Both tears start from a real cut rather than from a fixture: drop one of
    /// its clusters and the surface has a hole where that cluster was; draw
    /// every cluster twice and every interior edge has four faces on it.
    #[test]
    fn the_cover_check_refuses_a_torn_cut() {
        let dag = dunes_dag();
        let sound = dag.cut(EYES[0], PIXELS_PER_UNIT, MIXING_BUDGET);
        let interfaces = dag.check_cover(&sound).expect("the real cut is sound");
        assert!(
            interfaces > 0,
            "a mixed cut whose levels never meet would make the tears below \
             untestable"
        );

        let mut holed = sound.clone();
        let dropped = holed.remove(sound.len() / 2);
        let fault = dag
            .check_cover(&holed)
            .expect_err("a cut missing {dropped:?} covers the surface with a hole in it");
        assert!(
            fault.what.contains("hole"),
            "dropping {dropped:?} was reported as {fault}"
        );
        assert!(
            !fault.sample.is_empty(),
            "a fault has to say where: {fault}"
        );
        assert_eq!(
            fault.edges,
            fault.edges.max(fault.sample.len()),
            "the sample cannot be longer than the count it samples"
        );

        let doubled: Vec<ClusterAt> = sound.iter().chain(&sound).copied().collect();
        let fault = dag
            .check_cover(&doubled)
            .expect_err("a cut drawn twice covers the surface twice");
        assert!(
            fault.what.contains("overlapping"),
            "drawing the cut twice was reported as {fault}"
        );

        // And the sound cut still passes, so the two above are about the tears
        // rather than about a checker that refuses everything.
        assert_eq!(dag.check_cover(&sound), Ok(interfaces));
    }

    /// A DAG whose grouping leaves a cluster uncovered is **refused** rather
    /// than encoded, because the encoding has no way to say "not drawn" and
    /// would draw that cluster beside the parents covering it.
    ///
    /// The one place [`ClusterDag::cut_from`]'s default and
    /// [`ClusterSelect`]'s flags could disagree, closed by the assertion rather
    /// than by a comment saying it cannot happen.
    #[test]
    fn a_grouping_that_misses_a_cluster_is_refused() {
        let mut dag = dunes_dag();
        // Drop one child from one group of level 0, which leaves that cluster
        // in no group at all while every other level stays well formed.
        let orphan = dag.levels[0].groups[0].children.pop().expect("a child");
        let bases = vec![0u32; dag.levels.len()];
        let refusal = std::panic::catch_unwind(|| dag.selection_records(&bases, 0))
            .expect_err("an uncovered cluster has to be refused");
        let message = refusal
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains(&format!("level 0 cluster {orphan} is in no group")),
            "the refusal must name the cluster, and said {message:?}"
        );
    }

    /// The eye [`DRIFT_BUDGET`]'s oscillation runs at, `back` units past the
    /// patch's near edge.
    ///
    /// [`EYES`]`[0]` with only the distance free, so the one term that moves
    /// between two frames of the drift is the one the metric divides by.
    fn eye_back(back: f32) -> [f32; 3] {
        [0.0, 4.0, -dunes::DUNES_EXTENT - back]
    }

    /// The budget the drift is measured under, and how far the hold budget sits
    /// below it.
    ///
    /// The ratio is what the renderer ships (`crcbl_render::LOD_HOLD_RATIO`),
    /// spelled again here because this crate cannot see that one and the two are
    /// tuning rather than a shared contract.
    const DRIFT_BUDGET: f32 = 32.0;
    const DRIFT_HOLD_RATIO: f32 = 0.8;

    /// How many frames the oscillation runs for, and how many of them are on the
    /// far side of the boundary.
    ///
    /// Even, so the walk ends where it started and a flip count of one cannot be
    /// "it moved once and the sweep stopped".
    const DRIFT_FRAMES: usize = 40;

    /// The distance at which a uniform cut changes level, bisected rather than
    /// written down.
    ///
    /// A boundary is a property of the committed artifact and of
    /// [`PIXELS_PER_UNIT`], so a constant here would be a number to re-derive
    /// every time either moved. The bracket is [`DRIFT_NEAR`] to [`DRIFT_FAR`],
    /// which the assertion below confirms straddles one.
    const DRIFT_NEAR: f32 = 2.0;
    const DRIFT_FAR: f32 = 1000.0;

    /// Half the width of the oscillation, as a fraction of the boundary
    /// distance.
    ///
    /// Small enough that the projected error moves by far less than the
    /// hysteresis band — that is what makes "the camera drifts across the
    /// threshold" the thing under test rather than "the camera leaves the band"
    /// — and large enough to survive the bisection's own tolerance, which stops
    /// an order of magnitude below it.
    const DRIFT_SWING: f32 = 1.0e-3;

    /// Where the level a uniform cut selects changes, to within
    /// [`DRIFT_SWING`] / 10 of the distance.
    fn drift_boundary(dag: &ClusterDag) -> f32 {
        let level = |back: f32| dag.uniform_level(eye_back(back), PIXELS_PER_UNIT, DRIFT_BUDGET);
        let (near, far) = (level(DRIFT_NEAR), level(DRIFT_FAR));
        assert_ne!(
            near, far,
            "the bracket {DRIFT_NEAR}..{DRIFT_FAR} draws one level, so there is no \
             boundary in it to drift across"
        );
        let (mut lo, mut hi) = (DRIFT_NEAR, DRIFT_FAR);
        while hi - lo > lo * DRIFT_SWING / 10.0 {
            let mid = 0.5 * (lo + hi);
            if level(mid) == near {
                lo = mid
            } else {
                hi = mid
            }
        }
        0.5 * (lo + hi)
    }

    /// The distances one oscillation visits: a square wave straddling `at`,
    /// alternating sides every frame so every step is a crossing.
    fn drift_path(at: f32) -> Vec<f32> {
        let swing = at * DRIFT_SWING;
        (0..DRIFT_FRAMES)
            .map(|frame| {
                if frame % 2 == 0 {
                    at - swing
                } else {
                    at + swing
                }
            })
            .collect()
    }

    /// **A camera oscillating across a threshold settles on one level**, where
    /// the same camera under one threshold changes level every frame.
    ///
    /// `docs/plan/25-lod.md`: "**Hysteresis** on the threshold (switch-up and
    /// switch-down differ) kills boundary flicker." The flicker is the
    /// observable and this is it, counted: the camera steps back and forth
    /// across the distance at which a uniform cut changes level, by a thousandth
    /// of that distance, and the level it selects is recorded each frame.
    ///
    /// Three things are asserted, and the first is what makes the second mean
    /// anything:
    ///
    /// * **One threshold flicks the level on nearly every frame.** That number
    ///   is printed and asserted to be most of the walk, so a swing that had
    ///   quietly stopped crossing the boundary — which would make the hysteretic
    ///   count zero for the wrong reason — fails here.
    /// * **Two thresholds change it at most once.** Once and not never, because
    ///   the state starts collapsed and the first frame over the expand budget
    ///   is a real switch.
    /// * **And a decisive move still switches**, out to [`DRIFT_FAR`] and back,
    ///   or the feature is "never change level" wearing a band.
    #[test]
    fn a_camera_drifting_across_a_threshold_settles_on_one_level() {
        let dag = dunes_dag();
        let at = drift_boundary(&dag);
        let path = drift_path(at);

        let walk = |budgets: LodBudgets, path: &[f32]| {
            let mut state = vec![false; dag.group_count()];
            let levels: Vec<usize> = path
                .iter()
                .map(|&back| {
                    state = dag.expand(eye_back(back), PIXELS_PER_UNIT, budgets, &state);
                    dag.uniform_level_from(&state)
                })
                .collect();
            let flips = levels.windows(2).filter(|pair| pair[0] != pair[1]).count();
            (levels, flips)
        };

        let (sharp_levels, sharp_flips) = walk(LodBudgets::sharp(DRIFT_BUDGET), &path);
        let (held_levels, held_flips) =
            walk(LodBudgets::scaled(DRIFT_BUDGET, DRIFT_HOLD_RATIO), &path);
        println!(
            "drift at {at} units back: {sharp_flips} flip(s) with one threshold, \
             {held_flips} with two"
        );
        assert!(
            sharp_flips >= DRIFT_FRAMES - 2,
            "one threshold flipped {sharp_flips} time(s) over {DRIFT_FRAMES} frames \
             ({sharp_levels:?}), so the swing is not crossing the boundary and the \
             count below would be low for the wrong reason"
        );
        assert!(
            held_flips <= 1,
            "two thresholds flipped {held_flips} time(s) ({held_levels:?}), which is \
             not settling"
        );

        // And a decisive move still switches: out to the far end of the bracket
        // and back in, under the same band.
        let (decisive, decisive_flips) = walk(
            LodBudgets::scaled(DRIFT_BUDGET, DRIFT_HOLD_RATIO),
            &[at, DRIFT_FAR, DRIFT_FAR, at, at],
        );
        println!("decisive move under two thresholds: {decisive:?}");
        assert!(
            decisive_flips >= 2,
            "a camera pulled out to {DRIFT_FAR} units and brought back selected \
             {decisive:?}, so hysteresis is not a band, it is a latch"
        );
    }

    /// **Every frame of that drift draws a crack-free cover**, at the
    /// per-cluster granularity the mesh path selects at.
    ///
    /// The band is the one thing that could break the DAG's whole reason for
    /// existing: a cut is a cover only while expansion is monotone up the
    /// hierarchy, and a per-group memory is exactly what could hold a child
    /// collapsed under an expanded parent. [`crate::cluster_select`]'s module
    /// docs carry the induction that says it cannot; this runs it.
    ///
    /// The cut's own flicker is counted beside it, for
    /// `a_camera_drifting_across_a_threshold_settles_on_one_level`'s reason: a
    /// sweep in which the cut never moved under one threshold either would prove
    /// nothing about the one where it does not move under two.
    #[test]
    fn a_drifting_camera_keeps_a_crack_free_cover_under_hysteresis() {
        let dag = dunes_dag();
        let at = drift_boundary(&dag);
        let path = drift_path(at);

        let walk = |budgets: LodBudgets| {
            let mut state = vec![false; dag.group_count()];
            let mut cuts: Vec<Vec<ClusterAt>> = Vec::new();
            for &back in &path {
                state = dag.expand(eye_back(back), PIXELS_PER_UNIT, budgets, &state);
                let drawn = dag.cut_from(&state);
                dag.check_cover(&drawn).unwrap_or_else(|fault| {
                    panic!("a cut {back} units back under {budgets:?} has {fault}")
                });
                // The two granularities have to keep agreeing under the band,
                // because `crcbl-vk` compares one device's per-cluster cut
                // against another's uniform one and would otherwise report a
                // disagreement that is really this.
                assert_eq!(
                    dag.uniform_level_from(&state),
                    drawn
                        .iter()
                        .map(|at| at.level)
                        .min()
                        .expect("a cut always draws something"),
                    "the uniform level is not the finest level the cut draws, \
                     {back} units back under {budgets:?}"
                );
                cuts.push(drawn);
            }
            cuts.windows(2).filter(|pair| pair[0] != pair[1]).count()
        };

        let sharp = walk(LodBudgets::sharp(DRIFT_BUDGET));
        let held = walk(LodBudgets::scaled(DRIFT_BUDGET, DRIFT_HOLD_RATIO));
        println!("drift cuts: {sharp} change(s) with one threshold, {held} with two");
        assert!(
            sharp >= DRIFT_FRAMES - 2,
            "the cut moved {sharp} time(s) under one threshold, so this sweep is not \
             at a boundary at all"
        );
        assert!(
            held <= 1,
            "the cut moved {held} time(s) under two thresholds, which is not settling"
        );
    }

    /// Pixel budgets the patch actually changes its cut across.
    ///
    /// Below the smallest of these every camera here draws level 0 whole and
    /// above the largest the DAG has run out of levels, so a sweep outside the
    /// band would be a sweep over one answer. The band is a property of this
    /// model's errors and its size, not a tuning the engine ships.
    const BUDGETS: [f32; 5] = [8.0, 16.0, 32.0, 64.0, 128.0];

    /// The budget the near/far histograms are read at: inside the band where
    /// the near end's groups and the far end's fall on opposite sides of the
    /// test at [`EYES`]`[0]`.
    const MIXING_BUDGET: f32 = 32.0;

    /// How many clusters of each level the near and far thirds of the patch draw
    /// at [`MIXING_BUDGET`] from [`EYES`]`[0]`.
    ///
    /// Read off a run of the committed artifact, and pinned: this is the
    /// observable the whole slice exists to produce, so it is the one number
    /// here that has no derivation to check it against — a change in it is a
    /// change in what the model shows, and has to be looked at rather than
    /// absorbed.
    const NEAR_HISTOGRAM: &[(usize, usize)] = &[(0, 13), (1, 11)];
    const FAR_HISTOGRAM: &[(usize, usize)] = &[(2, 13)];

    /// Where the near third of the patch ends and the far third begins, along
    /// the axis running away from [`EYES`]`[0]`. The middle third is left out of
    /// both histograms: it is where the two ends change over, and which level a
    /// cluster there takes is the thing the budget decides rather than the thing
    /// being asserted.
    const NEAR_THIRD: f32 = -dunes::DUNES_EXTENT / 3.0;
    const FAR_THIRD: f32 = dunes::DUNES_EXTENT / 3.0;

    /// Where the sweep puts the camera, in the model's own space.
    ///
    /// `dunes` lays the patch out centred on the origin in `x` and `z` with the
    /// height on `y`, so an eye at negative `z` and a small `y` is a viewer
    /// standing at one edge of a ground plane that recedes away from them — the
    /// shape `docs/plan/25-lod.md`'s per-cluster selection exists for. The rest
    /// are off a corner, high above, and level with the middle, so the sweep is
    /// not one camera's arrangement holding.
    const EYES: [[f32; 3]; 5] = [
        [0.0, 4.0, -dunes::DUNES_EXTENT - 2.0],
        [0.0, 8.0, -dunes::DUNES_EXTENT - 24.0],
        [
            -dunes::DUNES_EXTENT - 16.0,
            6.0,
            -dunes::DUNES_EXTENT - 16.0,
        ],
        [0.0, 4.0 * dunes::DUNES_EXTENT, 0.0],
        [0.0, 3.0, 0.0],
    ];

    /// A decoder that reads past the end of its input, or reads a file that is
    /// not one of these, refuses by name rather than producing a plausible DAG.
    #[test]
    fn a_corrupt_artifact_is_refused_rather_than_decoded() {
        let good = dunes_dag().to_bytes();
        let base = dunes::positions();

        assert!(
            ClusterDag::from_bytes(&good, base.clone()).is_ok(),
            "the artifact this test perturbs has to decode first"
        );

        let mut wrong_magic = good.clone();
        wrong_magic[0] ^= 0xff;
        assert_eq!(
            ClusterDag::from_bytes(&wrong_magic, base.clone()),
            Err(DagDecodeError {
                what: "the magic is not a cooked cluster DAG",
                at: 0
            })
        );

        let mut wrong_version = good.clone();
        wrong_version[MAGIC.len()] = wrong_version[MAGIC.len()].wrapping_add(1);
        assert_eq!(
            ClusterDag::from_bytes(&wrong_version, base.clone()),
            Err(DagDecodeError {
                what: "the version is not the one this decoder writes",
                at: MAGIC.len()
            })
        );

        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            ClusterDag::from_bytes(&trailing, base.clone()),
            Err(DagDecodeError {
                what: "there are bytes after the last level",
                at: good.len()
            })
        );

        // A truncation at every length, so the walk is shown to run out of bytes
        // rather than to read whatever is next in memory.
        for length in [
            0,
            4,
            MAGIC.len(),
            MAGIC.len() + 4,
            good.len() / 2,
            good.len() - 1,
        ] {
            assert!(
                ClusterDag::from_bytes(&good[..length], base.clone()).is_err(),
                "{length} bytes of the artifact decoded as a whole DAG"
            );
        }

        // A count that would address more than the file holds is refused before
        // anything is allocated on it.
        let mut huge_level_count = good.clone();
        huge_level_count[MAGIC.len() + 4..MAGIC.len() + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            ClusterDag::from_bytes(&huge_level_count, base).map(|dag| dag.levels.len()),
            Err(DagDecodeError {
                what: "the level count",
                at: MAGIC.len() + 8
            })
        );
    }

    /// **A `base_positions` that is not the array the artifact was cooked over is
    /// refused**, rather than decoding into a DAG that panics in whoever walks
    /// it.
    ///
    /// Level 0's positions come from the caller and not from the file, which is
    /// what buys the artifact its largest array back — and is the one way a
    /// well-formed file still decodes into something wrong. Nothing in the file
    /// can catch it; only the index ranges can.
    #[test]
    fn positions_the_artifact_was_not_cooked_over_are_refused() {
        let good = dunes_dag().to_bytes();
        let mut short = dunes::positions();
        short.truncate(short.len() - 1);
        assert_eq!(
            ClusterDag::from_bytes(&good, short)
                .err()
                .map(|error| error.what),
            Some("a cluster names a vertex the level does not have"),
        );

        // A longer array is not refused — every index still names a vertex — so
        // the check above is about reachability rather than about equality, and
        // says so rather than pretending to a check it does not make.
        let mut long = dunes::positions();
        long.push([0.0; 3]);
        assert!(ClusterDag::from_bytes(&good, long).is_ok());
    }

    /// A cluster whose runs reach past the arrays they index is refused by name,
    /// on each of the three ranges that can go wrong.
    ///
    /// Reached by building the levels directly rather than by perturbing the
    /// artifact: a byte flip lands wherever it lands, and every one of these has
    /// to be shown to fire.
    #[test]
    fn a_cluster_reaching_past_its_arrays_is_refused_by_name() {
        let sound = |clusters: Vec<Meshlet>, vertices: Vec<u32>, corners: Vec<u8>| DagLevel {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            errors: vec![0.0; clusters.len()],
            bounds: vec![
                GroupBounds {
                    center: [0.0; 3],
                    radius: 1.0
                };
                clusters.len()
            ],
            clusters: MeshClusters {
                clusters,
                vertices,
                corners,
            },
            groups: Vec::new(),
        };
        let cluster = |vertex_count, triangle_count| {
            Meshlet::new(0, vertex_count, 0, triangle_count, ClusterBounds::default())
                .expect("small numbers")
        };

        // The shape everything below is a single change away from, so the
        // refusals are about the change rather than about the fixture.
        let whole = sound(vec![cluster(3, 1)], vec![0, 1, 2], vec![0, 1, 2]);
        assert_eq!(check_indices_are_in_range(&whole, 0), Ok(()));

        for (level, expected) in [
            (
                sound(vec![cluster(4, 1)], vec![0, 1, 2], vec![0, 1, 2]),
                "a cluster's vertex run is not inside the vertex array",
            ),
            (
                sound(vec![cluster(3, 2)], vec![0, 1, 2], vec![0, 1, 2]),
                "a cluster's corners are not inside the corner array",
            ),
            (
                sound(vec![cluster(3, 1)], vec![0, 1, 9], vec![0, 1, 2]),
                "a cluster names a vertex the level does not have",
            ),
            (
                sound(vec![cluster(3, 1)], vec![0, 1, 2], vec![0, 1, 3]),
                "a corner reaches past its own cluster's vertex run",
            ),
        ] {
            assert_eq!(
                check_indices_are_in_range(&level, 7),
                Err(DagDecodeError {
                    what: expected,
                    at: 7
                })
            );
        }
    }

    /// The error message names what ran out and where, because a corrupt bake
    /// artifact is something someone has to act on.
    #[test]
    fn the_decode_error_names_what_and_where() {
        let message = DagDecodeError {
            what: "a cluster sphere",
            at: 4096,
        }
        .to_string();
        assert!(message.contains("a cluster sphere"), "{message}");
        assert!(message.contains("4096"), "{message}");
    }
}
