//! The per-instance records `draw_gen.slang` takes a **uniform cut** with, in
//! the byte layout that shader declares.
//!
//! `docs/plan/25-lod.md`'s "Runtime selection" gives the two indirect tails a
//! coarser granularity than the mesh path's: "on `IndirectCount` and
//! `IndirectPerBatch` it runs in the cull compute pass and takes a **uniform
//! cut** — every cluster at one depth, which is exactly a whole-mesh level —
//! drawn as ordinary index ranges. Same hierarchy, same error metric, one
//! decision per instance instead of per cluster."
//!
//! So this module is [`crate::cluster_select`]'s sibling and not its rival: it
//! reads the same groups, through the same
//! [`GroupCost::projected_error`](crate::cluster_select::GroupCost::projected_error),
//! and differs only in how many answers come out — one per instance rather than
//! one per cluster.
//!
//! # The rule: the finest level any group is expanded at
//!
//! Writing `E(G)` for "group `G` is expanded" —
//! [`group_is_expanded`](crate::cluster_select::group_is_expanded), which under
//! `docs/plan/25-lod.md`'s hysteresis reads the previous frame's answer as well
//! as this frame's projection —
//! [`MeshLevels::select`](crate::level_select::MeshLevels::select) answers
//!
//! ```text
//! min { level(G) : E(G) }, or the top level when no group is expanded
//! ```
//!
//! and that is the whole of it. Reading it as a decision: a group's expansion
//! means "my simplification costs too much on screen, draw my children instead",
//! so the coarsest level a mesh may be drawn at whole is the one just below the
//! first simplification anybody refused.
//!
//! **That number is exactly the finest level the per-cluster cut draws
//! anywhere**, which is what makes the two granularities comparable rather than
//! merely similar:
//!
//! * No cluster below it is drawn per cluster. A level-`L` cluster needs
//!   `E(its container)`, and its container is a group at level `L` — so some
//!   group at `L` must be expanded, which no level below the minimum has.
//! * At least one cluster at it is. Take an expanded group at that level: its
//!   children have it as their container, and their producers are groups at the
//!   level below, none of which is expanded.
//!
//! `crate::cluster_dag`'s `the_uniform_level_is_the_finest_level_the_per_cluster
//! _cut_draws` is that statement run over the committed DAG at a sweep of eyes
//! and budgets, and `crcbl-vk`'s `the_two_geometry_paths_agree_about_how_fine_
//! the_dunes_patch_is` is it run across two real devices.
//!
//! # Why each group keeps its own sphere
//!
//! The minimum is over *groups*, each projected from **its own** bounds. The
//! alternative — one sphere for the whole mesh, or for a level, with that
//! level's worst error — is not conservative in a harmless direction: a sphere
//! containing every group's is never further from an eye than the groups it
//! contains, so it reports a larger projected error and selects a finer level.
//! For a mesh whose extent is large next to the viewing distance the eye is
//! *inside* that sphere and the answer saturates at level 0 from everywhere,
//! which is the dunes patch seen from its own edge. Keeping each group's sphere
//! is what makes the equality above hold exactly instead of approximately.
//!
//! # A mesh with no DAG selects level 0 and reads nothing
//!
//! [`MeshLevels::FLAT`](crate::level_select::MeshLevels::FLAT) is the record for
//! a mesh that has no hierarchy — the cube, the pyramid, the open box. It names
//! no groups and a top level of zero, so the loop above runs zero times and
//! answers zero, and a renderer therefore carries one record per mesh whatever
//! the mesh is.
//!
//! **A mesh whose hierarchy some other stage descends is a different case, and
//! it is not this record.** The mesh-shader path suppresses this selection with
//! a `top_level` of zero and its **real** group count: a device whose
//! amplification stage picks a cut per cluster must not have a second, coarser
//! cut applied to the same instance first — and it must still have its groups
//! judged, because [`select`](crate::level_select::MeshLevels::select) is where
//! the expansion state that stage reads is written. A top level of zero puts the
//! minimum out of reach without taking the groups away.

use crate::cluster_select::{GroupCost, LodBudgets, group_is_expanded};

/// Bytes per [`LevelGroup`], and the stride of the group region of
/// [`crate::draw_gen::Tables`].
///
/// One `uint` then five `float`, no padding, for the reason
/// [`CLUSTER_SELECT_STRIDE`](crate::cluster_select::CLUSTER_SELECT_STRIDE)
/// records: a `std430` struct of scalars has a stride that is exactly the sum of
/// its members.
///
/// **The shader no longer reads these through a typed `StructuredBuffer`.** Five
/// tables share one storage binding since the pass came down to the eight
/// storage buffers WebGPU guarantees — see `shaders/draw_gen.slang`'s header —
/// so `level_group_at` there loads six words and reinterprets five of them with
/// `asfloat`. That makes this stride a statement about *this* encoder rather
/// than about a decoration `slangc` emits, and
/// `the_shader_reads_the_selection_records_at_the_words_this_module_encodes` is
/// what holds the two spellings together.
pub const LEVEL_GROUP_STRIDE: usize = 24;

/// One group of a mesh's cluster DAG, and which level's simplification it is.
///
/// The level is the only thing this adds to [`GroupCost`]: a uniform cut needs
/// to know *which depth* an expanded group would send it to, where the
/// per-cluster descent never asks because the cluster already knows its own.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelGroup {
    /// The level whose clusters this group grouped — so an expanded group makes
    /// this the deepest a uniform cut may go.
    pub level: u32,
    /// What this group's simplification costs, and the sphere it is projected
    /// from.
    pub cost: GroupCost,
}

impl LevelGroup {
    /// The bytes one group-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; LEVEL_GROUP_STRIDE] {
        let mut bytes = [0u8; LEVEL_GROUP_STRIDE];
        let mut at = 0usize;
        let mut put = |word: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&word);
            at += 4;
        };
        put(self.level.to_le_bytes());
        put(self.cost.error.to_le_bytes());
        for axis in self.cost.bounds.center {
            put(axis.to_le_bytes());
        }
        put(self.cost.bounds.radius.to_le_bytes());
        debug_assert_eq!(at, LEVEL_GROUP_STRIDE);
        bytes
    }

    /// The inverse of [`LevelGroup::to_bytes`], so a test can decode what a
    /// buffer actually holds rather than trusting a host-side copy.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; LEVEL_GROUP_STRIDE]) -> Self {
        let word = |offset: usize| {
            bytes[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array"))
        };
        let float_at = |offset: usize| f32::from_le_bytes(word(offset));
        Self {
            level: u32::from_le_bytes(word(0)),
            cost: GroupCost {
                error: float_at(4),
                bounds: crate::cluster_dag::GroupBounds {
                    center: [float_at(8), float_at(12), float_at(16)],
                    radius: float_at(20),
                },
            },
        }
    }
}

/// Bytes per [`MeshLevels`], and the stride of the per-mesh region of
/// [`crate::draw_gen::Tables`].
///
/// Four `uint`, which is both the sum of the members and a multiple of the
/// 4-byte alignment they impose — so `std430` adds nothing. On
/// [`LEVEL_GROUP_STRIDE`]'s terms exactly since the tables were merged: pinned
/// against the shader's own word constants by
/// `the_shader_reads_the_selection_records_at_the_words_this_module_encodes`.
pub const MESH_LEVELS_STRIDE: usize = 16;

/// Where one mesh's DAG is, for an instance that names it.
///
/// Indexed by [`GpuInstance::mesh`](crate::mesh::GpuInstance::mesh), so the
/// array a renderer uploads is parallel to its mesh table and **every** entry an
/// instance can name has to be filled — [`FLAT`](Self::FLAT) is what an entry
/// with no hierarchy holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshLevels {
    /// This mesh's first [`LevelGroup`] in the shared group array.
    pub first_group: u32,
    /// How many it has. Zero for a mesh with no hierarchy, which is what makes
    /// [`uniform_level`](Self::uniform_level) answer
    /// [`top_level`](Self::top_level) without reading anything.
    pub group_count: u32,
    /// This mesh's first entry in the shared level array, whose element `d` is
    /// the mesh id that draws level `d` of this DAG.
    pub first_level: u32,
    /// The coarsest level this mesh has, so the level array's run is
    /// `top_level + 1` long.
    ///
    /// The top **level**, not the level count, and deliberately: the shader
    /// starts its search here, and a count would have to be decremented — which
    /// on a zero written by mistake wraps to `0xffff_ffff` and indexes the level
    /// array with it.
    pub top_level: u32,
}

impl MeshLevels {
    /// The record for a mesh with no hierarchy, or one whose hierarchy another
    /// stage descends: no groups, one level, and that level is the mesh itself.
    ///
    /// [`first_level`](Self::first_level) is the caller's to fill in — it is a
    /// position in the shared array and this type cannot know one.
    pub const FLAT: Self = Self {
        first_group: 0,
        group_count: 0,
        first_level: 0,
        top_level: 0,
    };

    /// The level a uniform cut draws this mesh at, from an eye at `eye` under a
    /// single budget of `budget` pixels and **no history**.
    ///
    /// [`select`](Self::select) with the two budgets equal and a fresh state, so
    /// the previous answer stops mattering — the rule as it stood before
    /// hysteresis, kept because it is what
    /// [`ClusterDag::uniform_level`](crate::cluster_dag::ClusterDag::uniform_level)
    /// is and what a caller sweeping budgets wants. A frame's real answer is
    /// [`select`](Self::select)'s, because a frame has a previous frame.
    ///
    /// `eye` is in the same space as the spheres, which for an instance means
    /// the spheres go through its transform and the camera stays where it is.
    ///
    /// # Panics
    ///
    /// If this record names groups `groups` does not hold, which is a table
    /// built wrong rather than a runtime condition.
    #[must_use]
    pub fn uniform_level(
        &self,
        groups: &[LevelGroup],
        eye: [f32; 3],
        pixels_per_unit: f32,
        budget: f32,
    ) -> u32 {
        let mut state = vec![false; groups.len()];
        self.select(
            groups,
            eye,
            pixels_per_unit,
            LodBudgets::sharp(budget),
            &mut state,
        )
    }

    /// One frame of this mesh's selection: every one of its groups' expansion
    /// updated in `state`, and the level a uniform cut draws returned.
    ///
    /// **`computeMain` in `draw_gen.slang` is this function**, one invocation per
    /// visible instance, and the two halves are deliberately one loop:
    ///
    /// * The state is written for **every** group, whatever the level answer is,
    ///   because the mesh path descends the same groups per cluster and reads
    ///   exactly this buffer. A mesh whose record has a `top_level` of zero —
    ///   which is how `docs/plan/25-lod.md`'s per-cluster path suppresses the
    ///   uniform cut — still needs its groups judged.
    /// * The level is then the finest level any group came out expanded at, which
    ///   is [`uniform_level`](Self::uniform_level)'s rule over the state rather
    ///   than over a second projection.
    ///
    /// `state` is indexed by position in `groups`, so a renderer's array is one
    /// run per mesh and this touches only its own. It is the **previous frame's**
    /// answers on the way in and this frame's on the way out;
    /// [`ClusterDag::expand`](crate::cluster_dag::ClusterDag::expand) is the same
    /// update stated over the hierarchy instead of over these records.
    ///
    /// # Panics
    ///
    /// If this record names groups `groups` does not hold, or if `state` is not
    /// parallel to `groups` — a table built wrong rather than a runtime
    /// condition.
    pub fn select(
        &self,
        groups: &[LevelGroup],
        eye: [f32; 3],
        pixels_per_unit: f32,
        budgets: LodBudgets,
        state: &mut [bool],
    ) -> u32 {
        assert_eq!(
            state.len(),
            groups.len(),
            "the expansion state is not parallel to the group array"
        );
        let mine = self.first_group as usize..self.first_group as usize + self.group_count as usize;
        let mut chosen = self.top_level;
        for at in mine {
            let group = &groups[at];
            let expanded = group_is_expanded(&group.cost, eye, pixels_per_unit, budgets, state[at]);
            state[at] = expanded;
            if expanded && group.level < chosen {
                chosen = group.level;
            }
        }
        chosen
    }
}

/// A group array as the bytes a storage-buffer upload takes.
#[must_use]
pub fn level_group_bytes(groups: &[LevelGroup]) -> Vec<u8> {
    groups.iter().flat_map(LevelGroup::to_bytes).collect()
}

/// A per-mesh array as the bytes a storage-buffer upload takes.
#[must_use]
pub fn mesh_levels_bytes(meshes: &[MeshLevels]) -> Vec<u8> {
    meshes
        .iter()
        .flat_map(|entry| {
            let mut bytes = [0u8; MESH_LEVELS_STRIDE];
            for (slot, value) in [
                entry.first_group,
                entry.group_count,
                entry.first_level,
                entry.top_level,
            ]
            .into_iter()
            .enumerate()
            {
                bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_dag::{GroupBounds, dunes_dag};

    /// The byte each field of a group record lands on. Six scalars in a row
    /// permute silently — a radius read as an error selects a level rather than
    /// crashing — so each is pinned rather than assumed.
    ///
    /// This is the *encoder's* layout; that the shader reads the same words is
    /// [`the_shader_reads_the_selection_records_at_the_words_this_module_encodes`].
    ///
    /// [`the_shader_reads_the_selection_records_at_the_words_this_module_encodes`]: fn@the_shader_reads_the_selection_records_at_the_words_this_module_encodes
    #[test]
    fn the_level_group_layout_is_six_scalars_in_declaration_order() {
        assert_eq!(LEVEL_GROUP_STRIDE, 24);

        let group = LevelGroup {
            level: 1,
            cost: GroupCost {
                error: 2.0,
                bounds: GroupBounds {
                    center: [3.0, 4.0, 5.0],
                    radius: 6.0,
                },
            },
        };
        let bytes = group.to_bytes();
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().expect("4")),
            1,
            "level at offset 0"
        );
        assert_eq!(float_at(4), 2.0, "error at offset 4");
        assert_eq!(float_at(8), 3.0, "center at offset 8");
        assert_eq!(float_at(16), 5.0, "and it is three floats wide");
        assert_eq!(float_at(20), 6.0, "radius at offset 20");
        assert_eq!(LevelGroup::from_bytes(&bytes), group);
        assert_eq!(
            level_group_bytes(&[group, group]).len(),
            2 * LEVEL_GROUP_STRIDE,
            "an array uploads at the stride the shader reads"
        );
    }

    /// The same, for [`MeshLevels`] — four words whose order decides which
    /// region each is read against.
    #[test]
    fn the_mesh_levels_layout_is_four_words_in_declaration_order() {
        assert_eq!(MESH_LEVELS_STRIDE, 16);
        let bytes = mesh_levels_bytes(&[MeshLevels {
            first_group: 7,
            group_count: 8,
            first_level: 9,
            top_level: 10,
        }]);
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(bytes.len(), MESH_LEVELS_STRIDE);
        assert_eq!(uint_at(0), 7, "first_group at offset 0");
        assert_eq!(uint_at(4), 8, "group_count at offset 4");
        assert_eq!(uint_at(8), 9, "first_level at offset 8");
        assert_eq!(uint_at(12), 10, "top_level at offset 12");
    }

    /// **The shader declares the same struct fields in the same order.**
    ///
    /// Two languages, no shared header, and a field order that permutes without
    /// failing: `crate::cull`'s
    /// `the_shared_structs_are_declared_identically_in_every_shader` does this
    /// for the structs several shaders repeat, and these two live in one shader
    /// and this module — so this is where they are held together.
    #[test]
    fn the_shader_declares_the_selection_records_this_module_encodes() {
        let source = include_str!("../shaders/draw_gen.slang");
        for field in [
            "uint level;",
            "float error;",
            "float center_x;",
            "float center_y;",
            "float center_z;",
            "float radius;",
        ] {
            assert!(
                source.contains(field),
                "draw_gen.slang must declare `{field}` in `struct LevelGroup`"
            );
        }
        for field in [
            "uint first_group;",
            "uint group_count;",
            "uint first_level;",
            "uint top_level;",
        ] {
            assert!(
                source.contains(field),
                "draw_gen.slang must declare `{field}` in `struct MeshLevels`"
            );
        }
    }

    /// **The shader loads each field from the word this module wrote it to.**
    ///
    /// The records reach `draw_gen.slang` as raw words now that five tables
    /// share one storage binding, so nothing in the toolchain lays them out for
    /// both sides any more: the shader names a word index per field and this
    /// crate writes a byte offset per field, and a permutation between them is
    /// silent — a radius read as an error selects a level rather than crashing.
    ///
    /// The index is **found in the encoder's own output** rather than written
    /// here as a literal, so an edit to
    /// [`LevelGroup::to_bytes`]/[`mesh_levels_bytes`] that moved a field turns
    /// this red instead of being blessed by it.
    #[test]
    fn the_shader_reads_the_selection_records_at_the_words_this_module_encodes() {
        let source = include_str!("../shaders/draw_gen.slang");
        let declares = |declaration: &str| {
            assert!(
                source.contains(declaration),
                "draw_gen.slang does not declare `{declaration}`"
            );
        };
        declares(&format!(
            "static const uint MESH_LEVELS_WORDS = {};",
            MESH_LEVELS_STRIDE / 4
        ));
        declares(&format!(
            "static const uint LEVEL_GROUP_WORDS = {};",
            LEVEL_GROUP_STRIDE / 4
        ));

        // Distinct sentinels, so "which word holds this field" has exactly one
        // answer in each encoding.
        let mesh = mesh_levels_bytes(&[MeshLevels {
            first_group: 101,
            group_count: 102,
            first_level: 103,
            top_level: 104,
        }]);
        let group = LevelGroup {
            level: 201,
            cost: GroupCost {
                error: 202.0,
                bounds: GroupBounds {
                    center: [203.0, 204.0, 205.0],
                    radius: 206.0,
                },
            },
        }
        .to_bytes();
        let word = |bytes: &[u8], index: usize| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().expect("4"))
        };
        let mut pinned = 0;
        for (bytes, stride, name, encoded) in [
            (
                &mesh[..],
                MESH_LEVELS_STRIDE,
                "MESH_LEVELS_FIRST_GROUP",
                101,
            ),
            (
                &mesh[..],
                MESH_LEVELS_STRIDE,
                "MESH_LEVELS_GROUP_COUNT",
                102,
            ),
            (
                &mesh[..],
                MESH_LEVELS_STRIDE,
                "MESH_LEVELS_FIRST_LEVEL",
                103,
            ),
            (&mesh[..], MESH_LEVELS_STRIDE, "MESH_LEVELS_TOP_LEVEL", 104),
            (&group[..], LEVEL_GROUP_STRIDE, "LEVEL_GROUP_LEVEL", 201),
            (
                &group[..],
                LEVEL_GROUP_STRIDE,
                "LEVEL_GROUP_ERROR",
                202.0f32.to_bits(),
            ),
            (
                &group[..],
                LEVEL_GROUP_STRIDE,
                "LEVEL_GROUP_CENTER_X",
                203.0f32.to_bits(),
            ),
            (
                &group[..],
                LEVEL_GROUP_STRIDE,
                "LEVEL_GROUP_CENTER_Y",
                204.0f32.to_bits(),
            ),
            (
                &group[..],
                LEVEL_GROUP_STRIDE,
                "LEVEL_GROUP_CENTER_Z",
                205.0f32.to_bits(),
            ),
            (
                &group[..],
                LEVEL_GROUP_STRIDE,
                "LEVEL_GROUP_RADIUS",
                206.0f32.to_bits(),
            ),
        ] {
            let index = (0..stride / 4)
                .find(|slot| word(bytes, *slot) == encoded)
                .unwrap_or_else(|| panic!("{name}'s sentinel is in no word of the encoding"));
            declares(&format!("static const uint {name} = {index};"));
            pinned += 1;
        }
        assert_eq!(
            pinned,
            MESH_LEVELS_STRIDE / 4 + LEVEL_GROUP_STRIDE / 4,
            "every word of both records must be pinned, or one of them is read from a word \
             nothing checks"
        );
    }

    /// **A mesh with no hierarchy is drawn at level 0 from every camera**,
    /// which is what lets one table cover a renderer holding both kinds of mesh
    /// — and what suppresses this selection on the path that selects per
    /// cluster.
    #[test]
    fn a_flat_mesh_selects_level_zero_from_every_camera() {
        let dag = dunes_dag();
        // Real groups, so the answer comes from `group_count` being zero rather
        // than from there being nothing to find.
        let groups = dag.level_groups();
        assert!(!groups.is_empty(), "the committed DAG has groups");
        for eye in [[0.0; 3], [1.0, 2.0, 3.0], [-40.0, 6.0, -40.0]] {
            for budget in [f32::NEG_INFINITY, 0.0, 1.0, 32.0, f32::INFINITY] {
                assert_eq!(
                    MeshLevels::FLAT.uniform_level(&groups, eye, 166.0, budget),
                    0,
                    "a flat mesh chose another level at {eye:?} under {budget}"
                );
            }
        }
    }

    /// **The records answer what the DAG answers**, over the committed artifact
    /// and a sweep of eyes and budgets.
    ///
    /// [`MeshLevels::uniform_level`] is the shader's loop and
    /// [`ClusterDag::uniform_level`](crate::cluster_dag::ClusterDag::uniform_level)
    /// is the rule stated over the hierarchy itself; they are two spellings and
    /// that is exactly the shape two copies drift in. The sweep is asserted to
    /// have produced more than one answer, because a comparison in which both
    /// sides always said zero would pass without either of them working.
    #[test]
    fn the_records_choose_the_level_the_dag_rule_chooses() {
        let dag = dunes_dag();
        let groups = dag.level_groups();
        let record = MeshLevels {
            first_group: 0,
            group_count: u32::try_from(groups.len()).expect("small"),
            first_level: 0,
            top_level: u32::try_from(dag.levels.len() - 1).expect("small"),
        };
        let mut seen = std::collections::BTreeSet::new();
        for eye in EYES {
            for budget in BUDGETS {
                let theirs = dag.uniform_level(eye, PIXELS_PER_UNIT, budget);
                let ours = record.uniform_level(&groups, eye, PIXELS_PER_UNIT, budget);
                assert_eq!(
                    u32::try_from(theirs).expect("small"),
                    ours,
                    "from {eye:?} under {budget} the DAG says {theirs} and the records say {ours}"
                );
                seen.insert(ours);
            }
        }
        assert!(
            seen.len() > 1,
            "the sweep produced one answer ({seen:?}), so the comparison never had \
             anything to disagree about"
        );
    }

    /// How many pixels one unit of length subtends one unit from the eye, for
    /// the sweeps above. Nothing turns on the figure; it only has to be the same
    /// one on both sides.
    const PIXELS_PER_UNIT: f32 = 166.0;

    /// Eyes the sweeps run at: at the patch's near edge, off a corner, high
    /// above, and inside the geometry — the last so the "eye inside the sphere"
    /// branch is compared rather than assumed.
    const EYES: [[f32; 3]; 4] = [
        [0.0, 4.0, -crate::dunes::DUNES_EXTENT - 2.0],
        [
            -crate::dunes::DUNES_EXTENT - 8.0,
            12.0,
            -crate::dunes::DUNES_EXTENT - 8.0,
        ],
        [0.0, 400.0 * crate::dunes::DUNES_EXTENT, 0.0],
        [0.0, 0.0, 0.0],
    ];

    /// Budgets the sweeps run at, from one no group satisfies to one every
    /// group does — so both ends of the rule are exercised and the levels
    /// between them are reached.
    const BUDGETS: [f32; 7] = [
        f32::NEG_INFINITY,
        0.5,
        1.0,
        32.0,
        1.0e3,
        1.0e6,
        f32::INFINITY,
    ];
}
