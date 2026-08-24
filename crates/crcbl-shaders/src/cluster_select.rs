//! The per-cluster record `mesh_cluster.slang`'s amplification stage descends
//! the DAG with, and the two budgets a group's expansion is judged against.
//!
//! `docs/plan/25-lod.md`'s "Runtime selection" picks a **cut** through a mesh's
//! [cluster DAG](crate::cluster_dag): the set of clusters covering the surface
//! exactly once. Writing `E(G)` for "group `G` is expanded", a cluster is drawn
//! exactly when
//!
//! ```text
//! !E(the group that produced it) && E(the group that contains it)
//! ```
//!
//! reading an absent producer as never expanded and an absent container as
//! always expanded. This record is those two groups, resolved per cluster at
//! bake time so the GPU evaluates the rule with **no communication between
//! workgroups**: one task group reads one record, reads two words of group
//! state, and decides one cluster.
//!
//! # Both halves name a group, and that is the whole correctness argument
//!
//! Neither index here is the cluster's own. [`ClusterSelect::producer_group`](crate::cluster_select::ClusterSelect::producer_group) is
//! the group whose simplification *emitted* this cluster and
//! [`ClusterSelect::container_group`](crate::cluster_select::ClusterSelect::container_group) is the group this cluster is a child
//! of —
//! and every cluster one group touches names that group by the **same index**,
//! so every one of them reads the same word and moves together. A cut can
//! therefore never draw one of a group's parents while descending into another.
//! Scaling by each cluster's own centre instead is what tears a mesh along a
//! boundary the group locked; `crcbl_scene::cluster_dag`'s module docs carry that
//! argument in full, and [`crate::cluster_dag`]'s tests are what hold the
//! artifact to it.
//!
//! An index rather than a copy of the group's error and sphere, and that is what
//! hysteresis changed: a decision carried between frames has to be stored
//! somewhere, and storing it per group needs the group to have a name. The
//! projection moved with it — [`group_is_expanded`](crate::cluster_select::group_is_expanded)
//! runs once per (instance, group) in `draw_gen.slang`, and the amplification
//! stage reads what it left.
//!
//! # Hysteresis: two budgets, and the state between them
//!
//! `docs/plan/25-lod.md`: "**Hysteresis** on the threshold (switch-up and
//! switch-down differ) kills boundary flicker."
//! [`LodBudgets`](crate::cluster_select::LodBudgets) is the pair, and
//! [`group_is_expanded`](crate::cluster_select::group_is_expanded) is the rule:
//!
//! ```text
//! E'(G) = e(G) > expand || (E(G) && e(G) > hold)
//! ```
//!
//! A group not expanded needs `e` over the **expand** budget to start; one
//! already expanded keeps going until `e` falls to the **hold** budget. Between
//! them the previous answer stands, which is what stops a camera drifting across
//! the boundary from switching level every frame.
//!
//! **The band cannot break the cut**, and that is not a hope: `E'` is monotone
//! up the DAG whenever `E` is. A parent group's error is at least its children's
//! and its sphere contains theirs, so `e` is monotone and so are both
//! comparisons; `E'(G)` therefore implies `E'` of every group above `G`,
//! whichever of the two terms produced it. Starting from "nothing expanded",
//! every frame after it is monotone by induction — and monotone is exactly what
//! makes a cut a cover. `crate::cluster_dag`'s
//! `a_drifting_camera_keeps_a_crack_free_cover_under_hysteresis` is that
//! statement run frame by frame over the committed DAG.
//!
//! # A cluster with no DAG draws unconditionally
//!
//! [`ClusterSelect::ALWAYS`](crate::cluster_select::ClusterSelect::ALWAYS) is the
//! record for a mesh that has no hierarchy at
//! all — the cube, the pyramid, the open box. It sets neither flag, so the
//! producer reads as never expanded and the container as always expanded, and
//! the rule above collapses to "draw it" with neither index ever read. A pool
//! therefore carries one record per cluster whatever the mesh is, rather than a
//! second code path for meshes that never descend.

use crate::cluster_dag::GroupBounds;

/// The two pixel budgets one threshold becomes under hysteresis.
///
/// `docs/plan/25-lod.md`'s "switch-up and switch-down differ", named for what
/// each one does to a group rather than for which way a level moves: a group
/// starts expanding above [`expand`](Self::expand) and stops below
/// [`hold`](Self::hold).
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodBudgets {
    /// What a group's projected error must exceed for an unexpanded group to
    /// expand — the switch to finer detail.
    pub expand: f32,
    /// What it must still exceed for an expanded group to stay expanded. At or
    /// below [`expand`](Self::expand); equal is [`sharp`](Self::sharp), which is
    /// no hysteresis at all.
    pub hold: f32,
}

impl LodBudgets {
    /// The pair with no band between them, which is the rule as it was before
    /// hysteresis: one threshold, and the previous answer ignored.
    ///
    /// What [`ClusterDag::cut`](crate::cluster_dag::ClusterDag::cut) runs, and
    /// the setting a test uses to show the flip-count assertion can fail.
    #[must_use]
    pub const fn sharp(budget: f32) -> Self {
        Self {
            expand: budget,
            hold: budget,
        }
    }

    /// The pair `budget` becomes when an expanded group is held down to
    /// `ratio * budget`.
    ///
    /// A ratio rather than a second absolute number because the band has to
    /// scale with the budget: a fixed offset is most of the budget at one pixel
    /// and none of it at fifty. A `ratio` of one is [`sharp`](Self::sharp).
    #[must_use]
    pub fn scaled(budget: f32, ratio: f32) -> Self {
        Self {
            expand: budget,
            hold: budget * ratio,
        }
    }
}

/// One group's contribution to the descent: what its simplification costs, the
/// sphere that cost is projected from, and the two comparisons that follow.
///
/// The pair [`ClusterGroup::projected_error`] reads, carried by
/// [`LevelGroup`](crate::level_select::LevelGroup) — the array `draw_gen.slang`
/// walks once per visible instance.
///
/// `PartialEq` but not `Eq`: it holds floats.
///
/// [`ClusterGroup::projected_error`]: crate::cluster_dag::ClusterGroup::projected_error
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupCost {
    /// How far the group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    pub error: f32,
    /// The sphere [`error`](Self::error) is projected from — the group's, grown
    /// to contain every sphere below it in the DAG.
    pub bounds: GroupBounds,
}

impl GroupCost {
    /// What this group's simplification costs on screen, in pixels, from an eye
    /// at `eye`.
    ///
    /// [`ClusterGroup::projected_error`] with the two numbers taken from this
    /// record instead of from the group — the same three arithmetic operations
    /// over the same `f32`s, and
    /// `the_two_projections_are_one_metric_over_the_whole_dag` is what holds
    /// them to being one metric rather than two spellings of one.
    ///
    /// [`ClusterGroup::projected_error`]: crate::cluster_dag::ClusterGroup::projected_error
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

    /// This group as an instance whose transform stretches a mesh-space length
    /// by at most `stretch` presents it: **both lengths multiplied, the centre
    /// and the projection left alone**.
    ///
    /// [`error`](Self::error) and [`GroupBounds::radius`](crate::cluster_dag::GroupBounds::radius)
    /// are distances in the mesh's own units, and an instance may be drawn at
    /// another size —
    /// [`GpuInstance::transform`](crate::mesh::GpuInstance::transform) carries a
    /// scale. An instance drawn ten times the size it was authored at has a
    /// simplification that moves the surface ten times as far on screen, and a
    /// sphere ten times as wide to be judged from; judging either at the mesh's
    /// own size picks the level for a picture nobody is looking at.
    ///
    /// `stretch` is the largest factor that transform can grow a length by,
    /// bounded from above — `crcbl_render::cull::max_stretch` in Rust and
    /// `max_stretch` in `draw_gen.slang` and `mesh_cluster.slang`, which
    /// `the_two_shaders_bound_a_stretched_length_with_one_function` holds to one
    /// text. It is exactly `1.0` for a rotation or a translation, where this is
    /// the identity and nothing that was already correct moves. Bounding from
    /// above is the conservative direction here: it can only raise the projected
    /// error, so a scaled instance expands a group *earlier* and draws finer,
    /// never coarser than the metric asked for.
    ///
    /// **The centre is the caller's**, which is the division of labour with
    /// `draw_gen.slang`: that shader puts the centre through the whole transform
    /// at the call site and scales exactly these two lengths, so what comes back
    /// is a group whose sphere is where the instance is and whose lengths are the
    /// size the instance draws them at.
    #[must_use]
    pub fn scaled(&self, stretch: f32) -> Self {
        Self {
            error: self.error * stretch,
            bounds: GroupBounds {
                center: self.bounds.center,
                radius: self.bounds.radius * stretch,
            },
        }
    }
}

/// Whether a group is expanded this frame, given whether it was last frame.
///
/// **The hysteresis rule, and the whole of it**, in the form `draw_gen.slang`'s
/// `group_is_expanded` runs it — see this module's docs for the two budgets and
/// for why the band cannot break a cut. `was` is the previous frame's answer for
/// this same (instance, group) pair, and `false` is what a pair with no history
/// starts at.
#[must_use]
pub fn group_is_expanded(
    cost: &GroupCost,
    eye: [f32; 3],
    pixels_per_unit: f32,
    budgets: LodBudgets,
    was: bool,
) -> bool {
    let error = cost.projected_error(eye, pixels_per_unit);
    error > budgets.expand || (was && error > budgets.hold)
}

/// Bytes per [`ClusterSelect`], and the stride of the selection storage buffer.
///
/// Four `uint`, no padding: a `std430` struct of scalars has a stride that is
/// exactly the sum of its members, which is the same reason
/// [`Meshlet`](crate::meshlet::Meshlet) beside it spells its sphere as four
/// `float`s rather than a `float3` and a `float`. Checked against the
/// `ArrayStride` and the `Offset` decorations `slangc` emits by this module's
/// `the_selection_layout_matches_the_offsets_slangc_emits`.
pub const CLUSTER_SELECT_STRIDE: usize = 16;

/// One cluster's half of the descent, matching `struct ClusterSelect` in
/// `shaders/mesh_cluster.slang`.
///
/// Four words and no floats: what a cluster contributes to the descent is
/// *which* two groups judge it, and the judging itself happens once per group
/// in `draw_gen.slang`. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterSelect {
    /// Which of the two groups below this cluster actually has —
    /// [`HAS_PRODUCER`](Self::HAS_PRODUCER) and
    /// [`HAS_CONTAINER`](Self::HAS_CONTAINER).
    ///
    /// A flag rather than a sentinel index, because both absences have to read
    /// as a *decision*: an absent producer must never expand and an absent
    /// container must always be expanded, which no index can say.
    pub flags: u32,

    /// Added to [`GpuMesh::base_vertex`](crate::mesh::GpuMesh::base_vertex) when
    /// this cluster's vertex run is resolved.
    ///
    /// **Which level of the DAG this cluster's geometry lives in.** Every level
    /// is decimated separately and its vertices belong to no vertex of the level
    /// below, so a DAG is several vertex ranges rather than one — where the
    /// instance names a single mesh, and so a single base. This is the
    /// difference between the two, and it is zero for level 0 and for every mesh
    /// with no DAG at all.
    pub vertex_base: u32,

    /// The group whose simplification produced this cluster, as an index into
    /// the shared [`LevelGroup`](crate::level_select::LevelGroup) array — the
    /// same numbering the per-(instance, group) state is keyed by.
    ///
    /// Read only when [`HAS_PRODUCER`](Self::HAS_PRODUCER) is set; a level-0
    /// cluster was produced by nothing.
    pub producer_group: u32,

    /// The group this cluster is a child of — the one that would simplify it
    /// away — in the same numbering.
    ///
    /// Read only when [`HAS_CONTAINER`](Self::HAS_CONTAINER) is set; a top-level
    /// cluster is contained by nothing and is drawn whenever its producer is not
    /// expanded.
    pub container_group: u32,
}

impl ClusterSelect {
    /// [`flags`](Self::flags) bit meaning
    /// [`producer_group`](Self::producer_group) names a real group. The same
    /// number as `HAS_PRODUCER` in `shaders/mesh_cluster.slang`, held in step
    /// with it by `the_shader_declares_the_same_selection_flags`.
    pub const HAS_PRODUCER: u32 = 1;

    /// The same, for the containing group.
    pub const HAS_CONTAINER: u32 = 2;

    /// The first [`flags`](Self::flags) bit the DAG level occupies.
    ///
    /// Bits 0 and 1 are [`HAS_PRODUCER`](Self::HAS_PRODUCER) and
    /// [`HAS_CONTAINER`](Self::HAS_CONTAINER); everything above them is the
    /// level this cluster's geometry was decimated to, which the LOD tint reads
    /// so a viewer can see a cut spanning several levels of one mesh.
    ///
    /// **Packed rather than given a field of its own**, because a fifth word
    /// moves [`CLUSTER_SELECT_STRIDE`] from 16 to 20 and with it every index
    /// into the selection buffer — a GPU-visible layout change on the mesh path
    /// for every backend, to carry a number that fits in bits nothing uses.
    ///
    /// Held in step with `mesh_cluster.slang` by
    /// `the_shader_declares_the_same_selection_flags`.
    pub const LEVEL_SHIFT: u32 = 2;

    /// The deepest level [`flags`](Self::flags) can carry.
    ///
    /// A DAG deep enough to overflow this would have more levels than a mesh has
    /// triangles: quarry's face, which is the densest content in the repository,
    /// builds twelve.
    pub const MAX_LEVEL: u32 = (1 << (u32::BITS - Self::LEVEL_SHIFT)) - 1;

    /// The [`flags`](Self::flags) bits that carry `level`.
    ///
    /// # Panics
    ///
    /// If `level` exceeds [`MAX_LEVEL`](Self::MAX_LEVEL), which is a DAG built
    /// wrong rather than a runtime condition.
    #[must_use]
    pub const fn level_bits(level: u32) -> u32 {
        assert!(
            level <= Self::MAX_LEVEL,
            "a DAG level past what ClusterSelect::flags can carry"
        );
        level << Self::LEVEL_SHIFT
    }

    /// The DAG level this cluster's geometry was decimated to, zero for the
    /// finest and for a mesh with no DAG at all.
    #[must_use]
    pub const fn level(&self) -> u32 {
        self.flags >> Self::LEVEL_SHIFT
    }

    /// The record for a cluster of a mesh with no DAG: neither group, so the
    /// descent draws it from every camera and neither index is ever read.
    ///
    /// See the module docs — this is what lets one selection buffer cover a pool
    /// holding both hierarchical and flat meshes.
    pub const ALWAYS: Self = Self {
        flags: 0,
        vertex_base: 0,
        producer_group: 0,
        container_group: 0,
    };

    /// Whether this cluster is drawn, given which groups are expanded.
    ///
    /// **The descent, and the whole of it**, in the form the amplification stage
    /// runs it: two flag tests and two array reads, with no knowledge of any
    /// other cluster and no arithmetic at all. `taskMain` in
    /// `shaders/mesh_cluster.slang` is the same four lines, and
    /// [`ClusterDag::cut_from`](crate::cluster_dag::ClusterDag::cut_from) is
    /// what runs this over a whole DAG so a test can compare a GPU's answer
    /// against it.
    ///
    /// `expanded` is indexed by the same group numbering
    /// [`producer_group`](Self::producer_group) uses — for one mesh's DAG on its
    /// own that is [`ClusterDag::level_groups`] order, and for a renderer it is
    /// that run offset by the mesh's `first_group`.
    ///
    /// # Panics
    ///
    /// If a flag is set and `expanded` is too short to hold the index beside it,
    /// which is a table built wrong rather than a runtime condition.
    ///
    /// [`ClusterDag::level_groups`]: crate::cluster_dag::ClusterDag::level_groups
    #[must_use]
    pub fn is_drawn(&self, expanded: &[bool]) -> bool {
        let producer_expanded =
            self.flags & Self::HAS_PRODUCER != 0 && expanded[self.producer_group as usize];
        let container_expanded =
            self.flags & Self::HAS_CONTAINER == 0 || expanded[self.container_group as usize];
        !producer_expanded && container_expanded
    }

    /// The bytes one selection-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CLUSTER_SELECT_STRIDE] {
        let mut bytes = [0u8; CLUSTER_SELECT_STRIDE];
        for (slot, value) in [
            self.flags,
            self.vertex_base,
            self.producer_group,
            self.container_group,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// The inverse of [`ClusterSelect::to_bytes`].
    ///
    /// So a test can decode what a selection buffer actually holds rather than
    /// trusting a host-side copy of it — the same reason
    /// [`Meshlet::from_bytes`](crate::meshlet::Meshlet::from_bytes) exists.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; CLUSTER_SELECT_STRIDE]) -> Self {
        let word = |offset: usize| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        Self {
            flags: word(0),
            vertex_base: word(4),
            producer_group: word(8),
            container_group: word(12),
        }
    }
}

/// A selection array as the bytes a storage-buffer upload takes.
#[must_use]
pub fn selection_bytes(records: &[ClusterSelect]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|record| record.to_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_dag::dunes_dag;

    /// The offsets `slangc` emitted for `ClusterSelect`, read out of the
    /// disassembly of `spirv/mesh_cluster.spv`. Four `uint`s in a row permute
    /// silently — a container index read as a producer index inverts the descent
    /// rather than crashing — so the byte each lands on is pinned rather than
    /// assumed.
    #[test]
    fn the_selection_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_ClusterSelect_std430 ArrayStride 16`, and
        // `OpMemberDecorate %ClusterSelect_std430 n Offset …`.
        assert_eq!(CLUSTER_SELECT_STRIDE, 16);

        let record = ClusterSelect {
            flags: 1,
            vertex_base: 2,
            producer_group: 3,
            container_group: 4,
        };
        let bytes = record.to_bytes();
        assert_eq!(bytes.len(), CLUSTER_SELECT_STRIDE);
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "flags at offset 0");
        assert_eq!(uint_at(4), 2, "vertex_base at offset 4");
        assert_eq!(uint_at(8), 3, "producer_group at offset 8");
        assert_eq!(uint_at(12), 4, "container_group at offset 12");

        // And the decode agrees with the encode, field for field.
        assert_eq!(ClusterSelect::from_bytes(&bytes), record);
        assert_eq!(
            selection_bytes(&[record, record]).len(),
            2 * CLUSTER_SELECT_STRIDE,
            "an array uploads at the stride the shader reads"
        );
    }

    /// **The shader's flag values are these constants**, and the two live in
    /// different languages with nothing but this to hold them together. A
    /// shader reading the container bit as the producer bit inverts the descent
    /// on every level-0 cluster.
    #[test]
    fn the_shader_declares_the_same_selection_flags() {
        let source = include_str!("../shaders/mesh_cluster.slang");
        for (name, value) in [
            ("HAS_PRODUCER", ClusterSelect::HAS_PRODUCER),
            ("HAS_CONTAINER", ClusterSelect::HAS_CONTAINER),
            ("LEVEL_SHIFT", ClusterSelect::LEVEL_SHIFT),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&declaration),
                "shaders/mesh_cluster.slang must declare `{declaration}`, or its \
                 descent reads a flag this crate does not write"
            );
        }
    }

    /// **A cluster with neither group is drawn whatever the group state says**,
    /// which is what lets a pool hold a mesh with no DAG beside one with a deep
    /// DAG — and it holds with an empty state array, because neither index is
    /// reached at all.
    #[test]
    fn a_cluster_with_no_dag_is_drawn_whatever_is_expanded() {
        assert!(
            ClusterSelect::ALWAYS.is_drawn(&[]),
            "a flat mesh's cluster was dropped with no groups in the world"
        );
        for state in [[false, false], [false, true], [true, false], [true, true]] {
            assert!(
                ClusterSelect::ALWAYS.is_drawn(&state),
                "a flat mesh's cluster was dropped under {state:?}"
            );
        }
        // And the flags are what did it, not the zero indices: the same record
        // with both groups declared present is dropped as soon as group 0
        // expands.
        let inside = ClusterSelect {
            flags: ClusterSelect::HAS_PRODUCER | ClusterSelect::HAS_CONTAINER,
            ..ClusterSelect::ALWAYS
        };
        assert!(
            !inside.is_drawn(&[true]),
            "an expanded producer must drop its cluster, or the flags decide nothing"
        );
    }

    /// **The two `projected_error`s are one metric**, over every group of the
    /// committed DAG and a sweep of eyes.
    ///
    /// [`GroupCost::projected_error`] is a second spelling of
    /// [`ClusterGroup::projected_error`](crate::cluster_dag::ClusterGroup::projected_error),
    /// and that is exactly the shape two copies drift in — a level chosen
    /// slightly wrong looks like a level. `tools/cook-clusters.rs` holds the
    /// *builder's* spelling to the cooked one for bit equality; this holds the
    /// cooked one to the record `draw_gen.slang` actually reads, and it closes
    /// the chain.
    #[test]
    fn the_two_projections_are_one_metric_over_the_whole_dag() {
        let dag = dunes_dag();
        let (mut compared, mut infinite) = (0usize, 0usize);
        for level in &dag.levels {
            for group in &level.groups {
                let cost = GroupCost {
                    error: group.error,
                    bounds: group.bounds,
                };
                for eye in EYES {
                    let theirs = group.projected_error(eye, PIXELS_PER_UNIT);
                    let ours = cost.projected_error(eye, PIXELS_PER_UNIT);
                    assert_eq!(
                        ours.to_bits(),
                        theirs.to_bits(),
                        "from {eye:?} the record says {ours} and the group says {theirs}"
                    );
                    compared += 1;
                    infinite += usize::from(ours.is_infinite());
                }
            }
        }
        assert!(compared > 0, "no group was compared, so nothing was");
        assert!(
            infinite > 0,
            "no eye landed inside a group's sphere, so the infinity branch was \
             never compared"
        );
    }

    /// **The two shaders project the error with one function**, character for
    /// character.
    ///
    /// There are three spellings of `docs/plan/25-lod.md`'s metric now, and this
    /// is the one that closes the ring:
    ///
    /// * [`ClusterGroup::projected_error`](crate::cluster_dag::ClusterGroup::projected_error)
    ///   and [`GroupCost::projected_error`] are held equal by
    ///   `the_two_projections_are_one_metric_over_the_whole_dag` above, over
    ///   every group of the committed DAG.
    /// * `draw_gen.slang`'s `group_is_expanded` is held to `GroupCost` by
    ///   `crcbl-vk`'s `the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`,
    ///   which compares the *decision* over every group on a real device.
    /// * `mesh_cluster.slang`'s copy — the screen-error heatmap's, which shades
    ///   a cluster by the number rather than by the decision — has no device
    ///   test of its own and could not usefully get one: a colour is not a cut,
    ///   so nothing on the far side would notice a drifted copy. What can be
    ///   checked cheaply and exactly is that it *is* a copy.
    ///
    /// So this asserts the function text, not the behaviour, and it is the
    /// stronger assertion of the two available here: equal text cannot differ
    /// under any input. **It goes red on a one-character edit to either file**,
    /// which is the point — an overlay drawn from a slightly different metric is
    /// a picture that disagrees with the cut it is drawn over while looking
    /// entirely plausible.
    ///
    /// The doc comments above the two are deliberately *not* compared: each
    /// says why its own file has the copy, and holding those equal would be
    /// holding two explanations to one wording.
    #[test]
    fn the_two_shaders_project_the_error_with_one_function() {
        const SIGNATURE: &str = "float projected_error(float error, float3 center, float radius, \
                                 float3 eye, float pixels_per_unit)";
        const INSIDE: &str = "static const float INSIDE_THE_SPHERE = 3.4028235e38;";
        let body = format!(
            "{SIGNATURE}\n\
             {{\n    \
             float3 delta = eye - center;\n    \
             float separation = sqrt(delta.x * delta.x + delta.y * delta.y + delta.z * delta.z);\n    \
             float distance = separation - radius;\n    \
             if (distance <= 0.0)\n    \
             {{\n        \
             return INSIDE_THE_SPHERE;\n    \
             }}\n    \
             return error * pixels_per_unit / distance;\n\
             }}\n"
        );
        for (name, source) in [
            ("draw_gen.slang", include_str!("../shaders/draw_gen.slang")),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
            ),
        ] {
            assert!(
                source.contains(INSIDE),
                "{name} must declare `{INSIDE}`, or the two files disagree about what an eye \
                 inside a group's sphere costs"
            );
            assert!(
                source.contains(&body),
                "{name} does not carry this exact function, so the metric now has two shader \
                 spellings:\n{body}"
            );
        }

        // And the sentinel really is the largest finite `f32`, which is what
        // makes it stand in for the infinity the Rust returns: every comparison
        // either takes part in is against a finite budget, and both are above
        // every one of those.
        let literal: f32 = INSIDE
            .rsplit_once("= ")
            .expect("the declaration has a value")
            .1
            .trim_end_matches(';')
            .parse()
            .expect("the sentinel is a float");
        assert_eq!(
            literal.to_bits(),
            f32::MAX.to_bits(),
            "the shaders' sentinel is {literal}, which is not f32::MAX"
        );
        let inside = GroupCost {
            error: 0.0,
            bounds: crate::cluster_dag::GroupBounds {
                center: [0.0; 3],
                radius: 1.0,
            },
        };
        let ours = inside.projected_error([0.0; 3], PIXELS_PER_UNIT);
        assert!(
            ours.is_infinite() && ours > 0.0,
            "the Rust answers {ours} from inside the sphere, so the sentinel above is standing \
             in for something else — and note it answered that with a zero error, which is the \
             branch being taken before `error` is read"
        );
        // The two agree on every budget **strictly below the sentinel**, which
        // is every budget a caller can set: `set_lod_error_budget` takes a pixel
        // count and the shadow bias scales it by a constant, so the range in
        // play is the one swept here.
        for budget in [f32::MIN_POSITIVE, 1.0, 16.0, 4096.0, 1.0e30] {
            assert_eq!(
                literal > budget,
                ours > budget,
                "the sentinel and the infinity disagree about the budget {budget}"
            );
        }
    }

    /// `max_stretch`, character for character, as both shaders that carry a
    /// mesh-space length across an instance transform must declare it.
    ///
    /// The signature and body only — the doc comment above each copy is
    /// deliberately not compared, on the same terms as `projected_error` above:
    /// each file says why *it* has the copy, and holding two explanations to one
    /// wording buys nothing.
    const MAX_STRETCH: &str = concat!(
        "float max_stretch(float3x3 basis)\n",
        "{\n",
        "    float3x3 gram = mul(transpose(basis), basis);\n",
        "    float bound = 0.0;\n",
        "    for (uint row = 0; row < 3; ++row)\n",
        "    {\n",
        "        bound = max(bound, abs(gram[row][0]) + abs(gram[row][1]) + abs(gram[row][2]));\n",
        "    }\n",
        "    return sqrt(bound);\n",
        "}\n",
    );

    /// **The two shaders bound a stretched length with one function**, character
    /// for character, and each of them really uses it on an instance transform.
    ///
    /// Three call sites now scale a mesh-space length by the largest singular
    /// value of an instance's 3×3 basis: `draw_gen.slang`'s `select_level`,
    /// which judges a group's radius and error at the size the instance is drawn
    /// at; `mesh_cluster.slang`'s `cluster_heat`, which shades a cluster by the
    /// same projection and would otherwise tint a scaled instance by a number
    /// the selection no longer uses; and that file's `cluster_survives`, which
    /// carries a cluster's bounding sphere across the same transform for the
    /// cull. A bound that differed between them is one instance judged at two
    /// sizes — a cut chosen for one and an overlay drawn for another.
    ///
    /// Equal text cannot differ under any input, which is the stronger of the
    /// two assertions available here, and it is the one
    /// `every_shader_that_transforms_a_normal_builds_it_with_one_function` and
    /// `the_two_shaders_project_the_error_with_one_function` already make for
    /// the other two functions the shaders share.
    ///
    /// **The second half is what stops a shader declaring the function and never
    /// calling it.** A copy of `max_stretch` sitting beside a projection of the
    /// unscaled radius passes a declaration check and selects exactly as wrongly
    /// as before, which is the bug this closed.
    #[test]
    fn the_two_shaders_bound_a_stretched_length_with_one_function() {
        for (name, source, uses) in [
            (
                "draw_gen.slang",
                include_str!("../shaders/draw_gen.slang"),
                &[
                    "float stretch = max_stretch((float3x3)instance.transform);",
                    "group.error * stretch",
                    "group.radius * stretch",
                ][..],
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                &[
                    "float stretch = max_stretch((float3x3)transform);",
                    "group.error * stretch",
                    "group.radius * stretch",
                    "float radius = cluster.radius * max_stretch(basis);",
                ][..],
            ),
        ] {
            assert!(
                source.contains(MAX_STRETCH),
                "{name} does not carry this exact function, so one instance transform has two \
                 bounds and the cut and the overlay are drawn from different ones:\n{MAX_STRETCH}"
            );
            for use_site in uses {
                assert!(
                    source.contains(use_site),
                    "{name} declares `max_stretch` and does not spell `{use_site}`, so either \
                     the declaration is decoration or a mesh-space length is still being judged \
                     at the mesh's own size"
                );
            }
        }
    }

    /// **Stretching a group multiplies both of its lengths and moves nothing
    /// else**, and at `1.0` it moves nothing at all.
    ///
    /// [`GroupCost::scaled`] is the Rust half of the three shader call sites
    /// above. The identity case is asserted on the *bits*, because it is what
    /// every instance in the tree is at: a `scaled` that perturbed a record there
    /// would move every level-of-detail golden for no reason.
    #[test]
    fn a_stretched_group_scales_both_lengths_and_costs_more() {
        let cost = GroupCost {
            error: 0.25,
            bounds: crate::cluster_dag::GroupBounds {
                center: [1.0, -2.0, 3.0],
                radius: 4.0,
            },
        };
        let eye = [0.0, 0.0, 40.0];
        let plain = cost.projected_error(eye, PIXELS_PER_UNIT);
        assert!(plain.is_finite(), "the eye is outside the sphere: {plain}");

        let same = cost.scaled(1.0);
        assert_eq!(same.error.to_bits(), cost.error.to_bits());
        assert_eq!(
            same.bounds.radius.to_bits(),
            cost.bounds.radius.to_bits(),
            "a stretch of one must leave a record's bits alone"
        );
        assert_eq!(
            same.projected_error(eye, PIXELS_PER_UNIT).to_bits(),
            plain.to_bits(),
            "an instance at the identity must project exactly what it did before"
        );

        let bigger = cost.scaled(4.0);
        assert_eq!(bigger.error, 1.0, "the error is a mesh-space length");
        assert_eq!(bigger.bounds.radius, 16.0, "and so is the radius");
        assert_eq!(
            bigger.bounds.center, cost.bounds.center,
            "the centre is the caller's, taken through the whole transform"
        );
        assert!(
            bigger.projected_error(eye, PIXELS_PER_UNIT) > plain,
            "a group drawn four times its authored size must cost more on screen than \
             {plain}, or the scaling reaches the projection and cancels"
        );
    }

    /// **The band is where the previous answer decides**, and only there.
    ///
    /// Three regions, over a real group of the committed DAG: above the expand
    /// budget both histories expand, below the hold budget neither does, and
    /// between them the answer is whatever it was. A rule that ignored `was`
    /// would agree on two of the three, which is why the middle is asserted for
    /// both histories rather than for one.
    #[test]
    fn the_previous_answer_decides_inside_the_band_and_nowhere_else() {
        let dag = dunes_dag();
        let group = &dag.levels[0].groups[0];
        let cost = GroupCost {
            error: group.error,
            bounds: group.bounds,
        };
        let eye = EYES[1];
        let error = cost.projected_error(eye, PIXELS_PER_UNIT);
        assert!(
            error.is_finite() && error > 0.0,
            "this eye has to be outside the sphere for the sweep to mean anything, \
             and the projection came out {error}"
        );
        let budgets = LodBudgets {
            expand: error * 2.0,
            hold: error / 2.0,
        };
        let at = |budgets, was| group_is_expanded(&cost, eye, PIXELS_PER_UNIT, budgets, was);

        // Above the expand budget: expanded either way.
        let over = LodBudgets::scaled(error / 2.0, 0.5);
        assert!(
            at(over, false),
            "an error over the expand budget must expand"
        );
        assert!(at(over, true), "and stay expanded");

        // Below the hold budget: collapsed either way.
        let under = LodBudgets::scaled(error * 2.0, 1.0);
        assert!(
            !at(under, false),
            "an error under both budgets must not expand"
        );
        assert!(!at(under, true), "and must give up if it was");

        // Inside the band: the previous answer, and it is the only thing that
        // differs between these two calls.
        assert!(!at(budgets, false), "an unexpanded group in the band waits");
        assert!(at(budgets, true), "an expanded group in the band holds");

        // And with no band at all the history stops mattering.
        let sharp = LodBudgets::sharp(error * 2.0);
        assert_eq!(
            at(sharp, false),
            at(sharp, true),
            "a sharp threshold must answer the same whatever happened last frame"
        );
    }

    /// How many pixels one unit of length subtends one unit from the eye, for
    /// the comparison above. A round number: nothing here turns on the figure,
    /// it only has to be the same one on both sides.
    const PIXELS_PER_UNIT: f32 = 166.0;

    /// Eyes the comparison runs at: at one edge of the patch, off a corner,
    /// high above, and inside the geometry — the last so the infinity branch is
    /// compared rather than assumed.
    const EYES: [[f32; 3]; 4] = [
        [0.0, 4.0, -crate::dunes::DUNES_EXTENT - 2.0],
        [
            -crate::dunes::DUNES_EXTENT - 8.0,
            12.0,
            -crate::dunes::DUNES_EXTENT - 8.0,
        ],
        [0.0, 4.0 * crate::dunes::DUNES_EXTENT, 0.0],
        [0.0, 0.0, 0.0],
    ];
}
