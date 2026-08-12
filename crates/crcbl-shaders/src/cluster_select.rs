//! The per-cluster record `mesh_cluster.slang`'s amplification stage descends
//! the DAG with, in the byte layout that shader declares.
//!
//! `docs/plan/25-lod.md`'s "Runtime selection" picks a **cut** through a mesh's
//! [cluster DAG](crate::cluster_dag): the set of clusters covering the surface
//! exactly once. Writing `E(G)` for "group `G`'s
//! [projected error](crate::cluster_dag::ClusterGroup::projected_error) exceeds
//! the pixel budget", a cluster is drawn exactly when
//!
//! ```text
//! !E(the group that produced it) && E(the group that contains it)
//! ```
//!
//! reading an absent producer as never expanded and an absent container as
//! always expanded. This record is those two groups, resolved per cluster at
//! bake time so the GPU evaluates the rule with **no communication between
//! workgroups**: one task group reads one record and decides one cluster.
//!
//! # Both halves name a group, and that is the whole correctness argument
//!
//! Neither sphere here is the cluster's own. `ClusterSelect::producer` is the
//! sphere and error of the group whose simplification *emitted* this cluster,
//! and `ClusterSelect::container` is the group this cluster
//! is a child of — and every cluster one group touches carries a bit-identical
//! copy of that group's numbers. So every cluster a group produced evaluates the
//! same `E(G)` from the same bits and moves together, and a cut can never draw
//! one of a group's parents while descending into another. Scaling by each
//! cluster's own centre instead is what tears a mesh along a boundary the group
//! locked; `crcbl_scene::cluster_dag`'s module docs carry that argument in full,
//! and [`crate::cluster_dag`]'s tests are what hold the artifact to it.
//!
//! Duplicating a group's five numbers into every cluster it touches is the
//! deliberate trade: a group index and a second buffer would be one indirection
//! per task group and one more array to keep in step, and this record is read
//! once by a workgroup that does nothing else.
//!
//! # A cluster with no DAG draws unconditionally
//!
//! `ClusterSelect::ALWAYS` is the record for a mesh that has no hierarchy at
//! all — the cube, the pyramid, the open box. It sets neither flag, so the
//! producer reads as never expanded and the container as always expanded, and
//! the rule above collapses to "draw it". A pool therefore carries one record
//! per cluster whatever the mesh is, rather than a second code path for meshes
//! that never descend.

use crate::cluster_dag::GroupBounds;

/// Bytes per [`ClusterSelect`], and the stride of the selection storage buffer.
///
/// Two `uint` then ten `float`, no padding: a `std430` struct of scalars has a
/// stride that is exactly the sum of its members, which is the same reason
/// [`Meshlet`](crate::meshlet::Meshlet) beside it spells its sphere as four
/// `float`s rather than a `float3` and a `float`. Checked against the
/// `ArrayStride` and the `Offset` decorations `slangc` emits by this module's
/// `the_selection_layout_matches_the_offsets_slangc_emits`.
pub const CLUSTER_SELECT_STRIDE: usize = 48;

/// One group's contribution to the descent: what its simplification costs and
/// the sphere that cost is projected from.
///
/// The pair [`ClusterGroup::projected_error`] reads, carried per cluster rather
/// than reached through an index — see the module docs.
///
/// `PartialEq` but not `Eq`: it holds floats. No `Default`, because the value a
/// caller wants for "no group here" is [`ClusterSelect::ALWAYS`]'s pair and a
/// zero error is not it — see [`ClusterSelect::flags`].
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
}

/// One cluster's half of the descent, matching `struct ClusterSelect` in
/// `shaders/mesh_cluster.slang`.
///
/// `PartialEq` but not `Eq`, for [`GroupCost`]'s reason: it holds floats, and no
/// `Default` for the same reason it has: [`ALWAYS`](Self::ALWAYS) is the record
/// a caller with no DAG wants, and it is named rather than derived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterSelect {
    /// Which of the two groups below this cluster actually has —
    /// [`HAS_PRODUCER`](Self::HAS_PRODUCER) and
    /// [`HAS_CONTAINER`](Self::HAS_CONTAINER).
    ///
    /// A flag rather than a sentinel in the numbers, because both absences have
    /// to read as a *decision* and neither has a value that would produce it: an
    /// absent producer must never expand, and a zero error still expands when
    /// the eye is inside its sphere, where [`GroupCost::projected_error`]
    /// returns infinity.
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

    /// The group whose simplification produced this cluster. Read only when
    /// [`HAS_PRODUCER`](Self::HAS_PRODUCER) is set; a level-0 cluster was
    /// produced by nothing.
    pub producer: GroupCost,

    /// The group this cluster is a child of — the one that would simplify it
    /// away. Read only when [`HAS_CONTAINER`](Self::HAS_CONTAINER) is set; a
    /// top-level cluster is contained by nothing and is drawn whenever its
    /// producer is not expanded.
    pub container: GroupCost,
}

impl ClusterSelect {
    /// [`flags`](Self::flags) bit meaning [`producer`](Self::producer) is a real
    /// group. The same number as `HAS_PRODUCER` in
    /// `shaders/mesh_cluster.slang`, held in step with it by
    /// `the_shader_declares_the_same_selection_flags`.
    pub const HAS_PRODUCER: u32 = 1;

    /// [`flags`](Self::flags) bit meaning [`container`](Self::container) is a
    /// real group.
    pub const HAS_CONTAINER: u32 = 2;

    /// The record for a cluster of a mesh with no DAG: neither group, so the
    /// descent draws it from every camera.
    ///
    /// See the module docs — this is what lets one selection buffer cover a pool
    /// holding both hierarchical and flat meshes.
    pub const ALWAYS: Self = Self {
        flags: 0,
        vertex_base: 0,
        producer: GroupCost {
            error: 0.0,
            bounds: GroupBounds {
                center: [0.0; 3],
                radius: 0.0,
            },
        },
        container: GroupCost {
            error: 0.0,
            bounds: GroupBounds {
                center: [0.0; 3],
                radius: 0.0,
            },
        },
    };

    /// Whether this cluster is drawn from an eye at `eye`, under a budget of
    /// `budget` pixels.
    ///
    /// **The descent, and the whole of it**, in the form the amplification stage
    /// runs it: two projections and two comparisons, with no knowledge of any
    /// other cluster. `taskMain` in `shaders/mesh_cluster.slang` is the same
    /// four lines, and
    /// [`ClusterDag::cut`](crate::cluster_dag::ClusterDag::cut) is what runs
    /// this over a whole DAG so a test can compare a GPU's answer against it.
    ///
    /// `eye` is in the same space as the spheres, which for an instance is the
    /// camera put through the inverse of its transform — or equivalently, and
    /// this is what the shader does, the spheres put through the transform and
    /// the camera left where it is. A uniform scale on that transform belongs in
    /// `pixels_per_unit`; a non-uniform one does not survive a bounding sphere
    /// and is not something this metric can express.
    #[must_use]
    pub fn is_drawn(&self, eye: [f32; 3], pixels_per_unit: f32, budget: f32) -> bool {
        let expanded = |cost: &GroupCost| cost.projected_error(eye, pixels_per_unit) > budget;
        let producer_expanded = self.flags & Self::HAS_PRODUCER != 0 && expanded(&self.producer);
        let container_expanded = self.flags & Self::HAS_CONTAINER == 0 || expanded(&self.container);
        !producer_expanded && container_expanded
    }

    /// The bytes one selection-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CLUSTER_SELECT_STRIDE] {
        let mut bytes = [0u8; CLUSTER_SELECT_STRIDE];
        let mut at = 0usize;
        let mut put = |word: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&word);
            at += 4;
        };
        put(self.flags.to_le_bytes());
        put(self.vertex_base.to_le_bytes());
        for cost in [self.producer, self.container] {
            put(cost.error.to_le_bytes());
            for axis in cost.bounds.center {
                put(axis.to_le_bytes());
            }
            put(cost.bounds.radius.to_le_bytes());
        }
        debug_assert_eq!(at, CLUSTER_SELECT_STRIDE);
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
            bytes[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array"))
        };
        let uint_at = |offset: usize| u32::from_le_bytes(word(offset));
        let float_at = |offset: usize| f32::from_le_bytes(word(offset));
        let cost = |at: usize| GroupCost {
            error: float_at(at),
            bounds: GroupBounds {
                center: [float_at(at + 4), float_at(at + 8), float_at(at + 12)],
                radius: float_at(at + 16),
            },
        };
        Self {
            flags: uint_at(0),
            vertex_base: uint_at(4),
            producer: cost(8),
            container: cost(28),
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
    /// disassembly of `spirv/mesh_cluster.spv`. Twelve scalars in a row permute
    /// silently — a container sphere read as a producer sphere selects a level
    /// rather than crashing — so the byte each lands on is pinned rather than
    /// assumed.
    #[test]
    fn the_selection_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_ClusterSelect_std430 ArrayStride 48`, and
        // `OpMemberDecorate %ClusterSelect_std430 n Offset …`.
        assert_eq!(CLUSTER_SELECT_STRIDE, 48);

        let record = ClusterSelect {
            flags: 1,
            vertex_base: 2,
            producer: GroupCost {
                error: 3.0,
                bounds: GroupBounds {
                    center: [4.0, 5.0, 6.0],
                    radius: 7.0,
                },
            },
            container: GroupCost {
                error: 8.0,
                bounds: GroupBounds {
                    center: [9.0, 10.0, 11.0],
                    radius: 12.0,
                },
            },
        };
        let bytes = record.to_bytes();
        assert_eq!(bytes.len(), CLUSTER_SELECT_STRIDE);
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "flags at offset 0");
        assert_eq!(uint_at(4), 2, "vertex_base at offset 4");
        assert_eq!(float_at(8), 3.0, "producer.error at offset 8");
        assert_eq!(float_at(12), 4.0, "producer.bounds.center at offset 12");
        assert_eq!(float_at(20), 6.0, "and it is three floats wide");
        assert_eq!(float_at(24), 7.0, "producer.bounds.radius at offset 24");
        assert_eq!(float_at(28), 8.0, "container.error at offset 28");
        assert_eq!(float_at(32), 9.0, "container.bounds.center at offset 32");
        assert_eq!(float_at(40), 11.0, "and it is three floats wide");
        assert_eq!(float_at(44), 12.0, "container.bounds.radius at offset 44");

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
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&declaration),
                "shaders/mesh_cluster.slang must declare `{declaration}`, or its \
                 descent reads a flag this crate does not write"
            );
        }
    }

    /// **A cluster with neither group is drawn from every camera**, which is
    /// what lets a pool hold a mesh with no DAG beside one with a deep DAG.
    ///
    /// The eye sweep matters: [`ClusterSelect::ALWAYS`]'s spheres are a point at
    /// the origin, and an eye *at* the origin is inside them — the case
    /// [`GroupCost::projected_error`] answers with infinity, and the one a
    /// record relying on a zero error rather than on its flags would get wrong.
    #[test]
    fn a_cluster_with_no_dag_is_drawn_from_every_camera() {
        for eye in [[0.0; 3], [1.0, 2.0, 3.0], [-40.0, 6.0, -40.0]] {
            for budget in [0.0, 1.0, 32.0, 1.0e9] {
                assert!(
                    ClusterSelect::ALWAYS.is_drawn(eye, 166.0, budget),
                    "a flat mesh's cluster was dropped at {eye:?} under {budget}"
                );
            }
        }
        // And the flags are what did it, not the zeroes: the same record with
        // both groups declared present is dropped at the origin, where both
        // project to infinity.
        let inside = ClusterSelect {
            flags: ClusterSelect::HAS_PRODUCER | ClusterSelect::HAS_CONTAINER,
            ..ClusterSelect::ALWAYS
        };
        assert!(
            !inside.is_drawn([0.0; 3], 166.0, 32.0),
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
    /// cooked one to the record the GPU actually reads, and it closes the chain.
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
