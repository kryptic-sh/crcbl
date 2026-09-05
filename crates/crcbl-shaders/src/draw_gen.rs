//! The workgroup size, uniform block and argument layout `draw_gen.slang`
//! declares, in the layouts that shader declares.
//!
//! Same reason as [`crate::cull`]: the shader fixes a number and a byte layout,
//! every producer and every reader of those has to agree with it exactly, and
//! keeping both in the crate that owns the source means there is one place to
//! change rather than one per consumer.
//!
//! What is *not* here is **which** meshes are buckets. That is the renderer's
//! decision — [`crcbl_render::ForwardRenderer`] builds one bucket per resident
//! mesh — and this crate has no idea what is resident. Where the bucket table
//! *sits* is another matter and is here, in
//! [`pack_tables`](crate::draw_gen::pack_tables): the shader reads
//! every host-written table out of one storage buffer, so the byte layout of
//! that buffer is the shader's and belongs beside the block that carries its
//! offsets.
//!
//! An instance whose mesh names no bucket scatters nowhere and is not drawn.
//! That cannot happen while every resident mesh has a bucket, which is the
//! renderer's invariant to keep; the shader has no way to report it and does not
//! pretend to.
//!
//! The re-declared `GpuInstance` and `GpuMesh` in `draw_gen.slang` are held
//! against `mesh.slang`'s by [`crate::cull`]'s
//! `the_shared_structs_are_declared_identically_in_every_shader`, which reads
//! every source rather than a pair.
//!
//! [`crcbl_render::ForwardRenderer`]: https://docs.rs/crcbl-render

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/draw_gen.slang`.
///
/// One invocation owns bucket `i` if there is one *and* scatters visible
/// instance `i` if there is one, so a caller dispatches
/// `max(buckets, visible_capacity).div_ceil(WORKGROUP_SIZE)` groups.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block.
///
/// Nine `uint`, then the padding `std140` puts in front of the `float4` that
/// follows — a `float4` is 16-aligned, so the ninth `uint` costs a whole row —
/// and then two `float4`. Checked against the `Offset` decorations `slangc`
/// emits by this module's
/// `the_draw_gen_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 80;

/// The uniform block, matching `struct DrawGenParams` in
/// `shaders/draw_gen.slang`.
///
/// `PartialEq` but not `Eq` since 2026-08: it holds the two floats
/// `docs/plan/25-lod.md`'s uniform cut selects under.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    /// Buckets in the table, which is also how many argument structures the
    /// pass writes.
    pub bucket_count: u32,
    /// Instance indices one bucket's run holds, and the stride between two
    /// buckets' runs.
    pub bucket_capacity: u32,
    /// Elements `cull.slang`'s visible list holds. Its counter can exceed this;
    /// the shader clamps before it indexes anything.
    pub visible_capacity: u32,
    /// How many groups one instance's run of the LOD hysteresis state holds —
    /// every resident mesh's group count summed, and the stride between two
    /// instances in that buffer.
    ///
    /// The same number
    /// [`ClusterDrawConstants::group_stride`](crate::meshlet::ClusterDrawConstants::group_stride)
    /// carries: this pass writes the state and the amplification stage reads it,
    /// so the two must index it identically.
    pub group_stride: u32,
    /// Where the per-bucket material modes start, in words, in the single
    /// host-written table buffer `draw_gen.slang` binds.
    ///
    /// A bucket's key is its mesh id **and** this word, which is what lets a
    /// mesh drawn by an opaque material and by a cutout one occupy two buckets —
    /// so a masked material costs a fragment stage in the depth passes for its
    /// own mesh and for no other. [`GpuMaterial::mode`](crate::mesh::GpuMaterial::mode)
    /// is the value; `crcbl_render::ForwardRenderer` is what emits one bucket
    /// per mode a scene actually holds, so an all-opaque scene's table is
    /// exactly the one it had before this region existed.
    pub bucket_modes_at: u32,
    /// Where the per-bucket cluster counts start, in words, in the same buffer.
    ///
    /// **Every host-written table shares one storage binding**, because WebGPU
    /// guarantees only eight storage buffers per shader stage and this pass
    /// bound fourteen — see `shaders/draw_gen.slang`'s header, which is where
    /// that is argued. The bucket table is at word zero and needs no offset;
    /// the offsets here say where each later region begins, and only the host
    /// knows, because each is as long as what is resident.
    ///
    /// [`pack_tables`] is what computes every one of them from the table
    /// lengths, so a caller packing the buffer and a caller filling this block
    /// cannot disagree.
    pub bucket_clusters_at: u32,
    /// Where the per-mesh [`MeshLevels`](crate::level_select::MeshLevels)
    /// records start, in words.
    pub mesh_levels_at: u32,
    /// Where the [`LevelGroup`](crate::level_select::LevelGroup) records start,
    /// in words.
    pub level_groups_at: u32,
    /// Where the level → mesh id table starts, in words.
    pub level_meshes_at: u32,
    /// The eye the uniform cut is selected from, in world space.
    ///
    /// Ordinarily the same three floats a frame writes into
    /// [`FrameUniforms::camera_position`](crate::mesh::FrameUniforms::camera_position):
    /// the two geometry paths select at different granularities and must never
    /// select from different cameras.
    ///
    /// **The two do separate on purpose in two cases, and neither is a
    /// disagreement.** A shadow cascade's frame block holds the *light*, because
    /// what that block feeds is a facing test whose viewer is the light, while
    /// detail stays denominated in the camera's pixels and keeps this field on
    /// the camera. And `crcbl_render::ForwardRenderer::set_frozen_selection_eye`
    /// pins this field alone, so a reviewer can fly away from the viewpoint a
    /// cut was chosen for and look at it — the only place a wrong cut is
    /// visible, since from the selecting eye every cut inside the budget has the
    /// same silhouette.
    pub camera_position: [f32; 3],
    /// How many pixels one unit of length subtends one unit from the eye, the
    /// pixel budget a group's projected error is compared against, and the
    /// budget an already-expanded group is held down to — `docs/plan/25-lod.md`'s
    /// hysteresis, and [`LodBudgets`](crate::cluster_select::LodBudgets)' two
    /// halves.
    ///
    /// **This block is the only place they are uploaded.** They travelled in
    /// [`FrameUniforms`](crate::mesh::FrameUniforms) as well until topic 18's
    /// light list needed that slot; no shader had read them there since the
    /// hysteresis landed, because a group's expansion is decided once per
    /// (instance, group) here and `mesh_cluster.slang`'s amplification stage
    /// reads the answer rather than the numbers.
    pub lod_params: [f32; 3],
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    ///
    /// The padding is written rather than left alone, for the reason
    /// [`crate::compute_probe::Params::to_bytes`] gives: a buffer allocated for
    /// this block is [`PARAMS_SIZE`] bytes wide and a partial write leaves the
    /// rest undefined.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut put = |at: usize, word: [u8; 4]| bytes[at..at + 4].copy_from_slice(&word);
        for (slot, value) in [
            self.bucket_count,
            self.bucket_capacity,
            self.visible_capacity,
            self.group_stride,
            self.bucket_modes_at,
            self.bucket_clusters_at,
            self.mesh_levels_at,
            self.level_groups_at,
            self.level_meshes_at,
        ]
        .into_iter()
        .enumerate()
        {
            put(slot * 4, value.to_le_bytes());
        }
        // Offset 48 is where `std140` puts the first `float4`: the nine `uint`
        // above end at 36, and a `float4` is 16-aligned.
        for (axis, value) in self.camera_position.into_iter().enumerate() {
            put(48 + axis * 4, value.to_le_bytes());
        }
        for (slot, value) in self.lod_params.into_iter().enumerate() {
            put(64 + slot * 4, value.to_le_bytes());
        }
        bytes
    }
}

/// The single table buffer `draw_gen.slang` binds, and where each region starts.
///
/// Produced by [`pack_tables`], which is the only thing that may produce one:
/// the bytes and the four offsets have to be computed from the same lengths, and
/// a caller that packed the buffer one way and filled
/// [`Params::mesh_levels_at`] another would have a shader reading the region in
/// front of the one it meant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tables {
    /// The whole buffer, ready to upload. Never empty.
    pub bytes: Vec<u8>,
    /// Where each region in it starts.
    pub offsets: TableOffsets,
}

/// Where each region of [`Tables::bytes`] starts, in words.
///
/// Separate from the bytes so a caller can keep it after the upload without
/// keeping a copy of the buffer: the offsets are what every later frame's
/// [`Params`] carries, and the bytes are wanted exactly once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableOffsets {
    /// [`Params::bucket_modes_at`].
    pub bucket_modes_at: u32,
    /// [`Params::bucket_clusters_at`].
    pub bucket_clusters_at: u32,
    /// [`Params::mesh_levels_at`].
    pub mesh_levels_at: u32,
    /// [`Params::level_groups_at`].
    pub level_groups_at: u32,
    /// [`Params::level_meshes_at`].
    pub level_meshes_at: u32,
}

/// Packs every host-written table `draw_gen.slang` reads into one buffer.
///
/// The regions are laid out in the order that shader's `tables` binding
/// documents: the bucket table at word zero, then the per-bucket material modes,
/// then the per-bucket cluster counts, then the per-mesh
/// [`MeshLevels`](crate::level_select::MeshLevels) records, then the
/// [`LevelGroup`](crate::level_select::LevelGroup) records, then the level →
/// mesh id table. **One buffer because a WebGPU device guarantees only eight
/// storage buffers per shader stage** and the pass bound fourteen; the tables
/// were chosen for the merge because they are written together, when a mesh
/// becomes resident, and never per frame.
///
/// **Each of the three selection regions is padded to at least one record**, so
/// a renderer whose meshes have no hierarchy at all — the ordinary case — still
/// has a word for every index the shader can form. A zero-length region would
/// otherwise put `level_meshes_at` at the end of the buffer, and every mesh's
/// `first_level` names element zero of that region whether it has a DAG or not.
///
/// # Errors
///
/// `None` if the packed buffer is longer than a `u32` of words can address,
/// which is a table built far past anything a device would allocate rather than
/// a runtime condition.
#[must_use]
pub fn pack_tables(
    bucket_meshes: &[u32],
    bucket_modes: &[u32],
    bucket_clusters: &[u32],
    mesh_levels: &[crate::level_select::MeshLevels],
    level_groups: &[crate::level_select::LevelGroup],
    level_meshes: &[u32],
) -> Option<Tables> {
    let mut bytes: Vec<u8> = Vec::new();
    let offset = |bytes: &[u8]| u32::try_from(bytes.len() / 4).ok();

    let words = |values: &[u32]| -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    };
    // At least one record, for the reason the docs give.
    let padded = |mut region: Vec<u8>, stride: usize| -> Vec<u8> {
        if region.is_empty() {
            region = vec![0u8; stride];
        }
        region
    };

    bytes.extend_from_slice(&words(bucket_meshes));
    let bucket_modes_at = offset(&bytes)?;
    bytes.extend_from_slice(&words(bucket_modes));
    let bucket_clusters_at = offset(&bytes)?;
    bytes.extend_from_slice(&words(bucket_clusters));
    let mesh_levels_at = offset(&bytes)?;
    bytes.extend_from_slice(&padded(
        crate::level_select::mesh_levels_bytes(mesh_levels),
        crate::level_select::MESH_LEVELS_STRIDE,
    ));
    let level_groups_at = offset(&bytes)?;
    bytes.extend_from_slice(&padded(
        crate::level_select::level_group_bytes(level_groups),
        crate::level_select::LEVEL_GROUP_STRIDE,
    ));
    let level_meshes_at = offset(&bytes)?;
    bytes.extend_from_slice(&padded(words(level_meshes), 4));
    // The whole buffer is bound as a descriptor, and a zero-length one is not a
    // descriptor any backend takes. The padding above guarantees it, and this is
    // what says so where a reader meets it.
    debug_assert!(!bytes.is_empty());

    Some(Tables {
        bytes,
        offsets: TableOffsets {
            bucket_modes_at,
            bucket_clusters_at,
            mesh_levels_at,
            level_groups_at,
            level_meshes_at,
        },
    })
}

/// Words in one indexed-indirect argument structure.
pub const DRAW_ARGS_WORDS: usize = 5;

/// Bytes of one indexed-indirect argument structure, which is the `stride` a
/// [`DrawIndirect`] or [`DrawIndirectCount`] over this buffer carries.
///
/// [`DrawIndirect`]: https://docs.rs/crcbl-hal
/// [`DrawIndirectCount`]: https://docs.rs/crcbl-hal
pub const DRAW_ARGS_SIZE: usize = DRAW_ARGS_WORDS * 4;

/// One indexed indirect draw's arguments, matching what `draw_gen.slang` writes.
///
/// **The layout is not this engine's to choose.** It is
/// `VkDrawIndexedIndirectCommand`, and `D3D12_DRAW_INDEXED_ARGUMENTS` and
/// `wgpu`'s `DrawIndexedIndirectArgs` are the same five words in the same order
/// — a driver reads these bytes directly, so the only correct layout is the
/// one every API already agreed on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawIndexedArgs {
    /// Indices this draw reads, starting at [`first_index`](Self::first_index).
    pub index_count: u32,
    /// Instances of it to draw. Written by the GPU: this is the word
    /// `draw_gen.slang` increments as it scatters, so it is both the count and
    /// the pass's slot allocator.
    pub instance_count: u32,
    /// First index, in the shared index pool.
    pub first_index: u32,
    /// Added to every index before it selects a vertex. **Signed**, and always
    /// zero here — the mesh's base vertex reaches the shader through the mesh
    /// table instead, because the four shader targets disagree about what a
    /// draw's base vertex does to `SV_VertexID`. See `mesh.slang`'s header.
    pub vertex_offset: i32,
    /// The draw's first instance. Always zero here, for the same disagreement's
    /// sake: `SV_InstanceID` counts from zero on every target only while this
    /// is zero.
    pub first_instance: u32,
}

impl DrawIndexedArgs {
    /// The arguments as the bytes an indirect buffer holds.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; DRAW_ARGS_SIZE] {
        let mut bytes = [0u8; DRAW_ARGS_SIZE];
        for (slot, value) in [
            self.index_count,
            self.instance_count,
            self.first_index,
            #[allow(clippy::cast_sign_loss)]
            {
                self.vertex_offset as u32
            },
            self.first_instance,
        ]
        .into_iter()
        .enumerate()
        {
            let at = slot * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// The arguments decoded from the bytes an indirect buffer holds — the
    /// shape a readback compares against the draws a CPU would have recorded.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; DRAW_ARGS_SIZE]) -> Self {
        let word = |slot: usize| {
            u32::from_le_bytes(
                bytes[slot * 4..slot * 4 + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("a four-byte window of a fixed-size array")),
            )
        };
        Self {
            index_count: word(0),
            instance_count: word(1),
            first_index: word(2),
            #[allow(clippy::cast_possible_wrap)]
            vertex_offset: word(3) as i32,
            first_instance: word(4),
        }
    }
}

/// Words in one mesh-dispatch argument structure.
pub const MESH_ARGS_WORDS: usize = 3;

/// Bytes of one mesh-dispatch argument structure, which is the `stride` a
/// [`DrawIndirect`] over this buffer carries.
///
/// [`DrawIndirect`]: https://docs.rs/crcbl-hal
pub const MESH_ARGS_SIZE: usize = MESH_ARGS_WORDS * 4;

/// One mesh-shading dispatch's workgroup counts, matching what `draw_gen.slang`
/// writes and what `CommandEncoder::draw_mesh_tasks_indirect` reads.
///
/// **The layout is not this engine's to choose**, for
/// [`DrawIndexedArgs`]' reason: it is `VkDrawMeshTasksIndirectCommandEXT`, and
/// `D3D12_DISPATCH_MESH_ARGUMENTS` and Metal's
/// `MTLDispatchThreadgroupsIndirectArguments` are the same three words in the
/// same order.
///
/// [`group_count_y`](Self::group_count_y) is the one the GPU decides: it is how
/// many instances survived culling into this bucket, so a dispatch reading it
/// launches no workgroups for the ones that did not. The other two are the
/// bucket's mesh's cluster count and one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshTasksArgs {
    /// Workgroups on x: clusters in the bucket's mesh, one workgroup each.
    pub group_count_x: u32,
    /// Workgroups on y: **surviving** instances in the bucket's run, which is
    /// the same number `DrawIndexedArgs::instance_count` carries and the whole
    /// reason this structure is read from memory rather than passed.
    pub group_count_y: u32,
    /// Workgroups on z, which is one: a (cluster, instance) pair is two
    /// dimensions and the third is what the APIs' structures all have.
    pub group_count_z: u32,
}

impl MeshTasksArgs {
    /// The arguments as the bytes an indirect buffer holds.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MESH_ARGS_SIZE] {
        let mut bytes = [0u8; MESH_ARGS_SIZE];
        for (slot, value) in [self.group_count_x, self.group_count_y, self.group_count_z]
            .into_iter()
            .enumerate()
        {
            let at = slot * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// The arguments decoded from the bytes an indirect buffer holds — the
    /// shape a readback compares against the dispatch a CPU would have sized.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; MESH_ARGS_SIZE]) -> Self {
        let word = |slot: usize| {
            u32::from_le_bytes(
                bytes[slot * 4..slot * 4 + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("a four-byte window of a fixed-size array")),
            )
        };
        Self {
            group_count_x: word(0),
            group_count_y: word(1),
            group_count_z: word(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size, for the
    /// reason [`crate::cull`]'s twin gives: a mismatch shows up only as a partly
    /// generated draw list, which reads as "those instances were culled".
    #[test]
    fn the_workgroup_size_matches_the_numthreads_draw_gen_slang_declares() {
        let source = include_str!("../shaders/draw_gen.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "draw_gen.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted from the \
             shader"
        );
    }

    /// The offsets `slangc` actually emitted for `DrawGenParams`, read out of
    /// the disassembly.
    #[test]
    fn the_draw_gen_params_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %DrawGenParams_std140 n Offset …`: 0, 4, 8, 12, 16,
        // 20, 24, 28, 32, 48, 64.
        assert_eq!(PARAMS_SIZE, 80);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a block that is not \
             one already is a block the shader and the CPU disagree about the width of"
        );
        let bytes = Params {
            bucket_count: 2,
            bucket_capacity: 5,
            visible_capacity: 9,
            group_stride: 11,
            bucket_modes_at: 12,
            bucket_clusters_at: 13,
            mesh_levels_at: 17,
            level_groups_at: 19,
            level_meshes_at: 23,
            camera_position: [1.5, 2.5, 3.5],
            lod_params: [4.5, 5.5, 6.5],
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 2, "bucket_count at offset 0");
        assert_eq!(uint_at(4), 5, "bucket_capacity at offset 4");
        assert_eq!(uint_at(8), 9, "visible_capacity at offset 8");
        assert_eq!(uint_at(12), 11, "group_stride at offset 12");
        assert_eq!(uint_at(16), 12, "bucket_modes_at at offset 16");
        assert_eq!(uint_at(20), 13, "bucket_clusters_at at offset 20");
        assert_eq!(uint_at(24), 17, "mesh_levels_at at offset 24");
        assert_eq!(uint_at(28), 19, "level_groups_at at offset 28");
        assert_eq!(uint_at(32), 23, "level_meshes_at at offset 32");
        assert!(
            bytes[36..48].iter().all(|byte| *byte == 0),
            "the row std140 pads out before the first float4 is written, and it is zero: {:?}",
            &bytes[36..48]
        );
        assert_eq!(float_at(48), 1.5, "camera_position at offset 48");
        assert_eq!(float_at(56), 3.5, "and it is three floats wide");
        assert_eq!(uint_at(60), 0, "the fourth component is padding, and zero");
        assert_eq!(float_at(64), 4.5, "lod_params at offset 64");
        assert_eq!(float_at(68), 5.5, "and its expand budget beside it");
        assert_eq!(float_at(72), 6.5, "and the hold budget after that");
        assert!(
            bytes[76..].iter().all(|byte| *byte == 0),
            "the std140 tail padding is written, and it is zero: {:?}",
            &bytes[76..]
        );
    }

    /// The regions [`pack_tables`] lays out are the ones the offsets it returns
    /// name, and every one of them decodes back to what went in.
    ///
    /// Both halves matter and neither implies the other: a packer whose offsets
    /// were all zero would still round-trip the first region, and one that got
    /// the offsets right while writing a region at the wrong stride would still
    /// place them.
    #[test]
    fn the_packed_tables_decode_at_the_offsets_they_report() {
        use crate::cluster_dag::GroupBounds;
        use crate::cluster_select::GroupCost;
        use crate::level_select::{LEVEL_GROUP_STRIDE, LevelGroup, MESH_LEVELS_STRIDE, MeshLevels};

        let bucket_meshes = [7u32, 8, 9];
        // Two modes over three buckets, so a packer that wrote the mesh ids into
        // this region — or read it at the bucket table's offset — is a different
        // set of words rather than the same ones.
        let bucket_modes = [0u32, 1, 0];
        let bucket_clusters = [70u32, 80, 90];
        let mesh_levels = [
            MeshLevels {
                first_group: 0,
                group_count: 2,
                first_level: 0,
                top_level: 1,
            },
            MeshLevels {
                first_group: 2,
                group_count: 0,
                first_level: 2,
                top_level: 0,
            },
        ];
        let group = |level: u32, error: f32| LevelGroup {
            level,
            cost: GroupCost {
                error,
                bounds: GroupBounds {
                    center: [1.0, 2.0, 3.0],
                    radius: 4.0,
                },
            },
        };
        let level_groups = [group(0, 0.25), group(1, 0.5)];
        let level_meshes = [11u32, 12, 13];

        let packed = pack_tables(
            &bucket_meshes,
            &bucket_modes,
            &bucket_clusters,
            &mesh_levels,
            &level_groups,
            &level_meshes,
        )
        .expect("a table this small addresses");
        let word_at = |word: u32| {
            let at = word as usize * 4;
            u32::from_le_bytes(packed.bytes[at..at + 4].try_into().expect("4"))
        };

        for (bucket, mesh) in bucket_meshes.iter().enumerate() {
            let bucket = u32::try_from(bucket).expect("small");
            assert_eq!(word_at(bucket), *mesh, "bucket table at word 0");
            assert_eq!(
                word_at(packed.offsets.bucket_modes_at + bucket),
                bucket_modes[bucket as usize],
                "material modes at bucket_modes_at"
            );
            assert_eq!(
                word_at(packed.offsets.bucket_clusters_at + bucket),
                bucket_clusters[bucket as usize],
                "cluster counts at bucket_clusters_at"
            );
        }
        for (index, record) in mesh_levels.iter().enumerate() {
            let at = packed.offsets.mesh_levels_at
                + u32::try_from(index * MESH_LEVELS_STRIDE / 4).expect("small");
            assert_eq!(word_at(at), record.first_group);
            assert_eq!(word_at(at + 1), record.group_count);
            assert_eq!(word_at(at + 2), record.first_level);
            assert_eq!(word_at(at + 3), record.top_level);
        }
        for (index, record) in level_groups.iter().enumerate() {
            let at = packed.offsets.level_groups_at
                + u32::try_from(index * LEVEL_GROUP_STRIDE / 4).expect("small");
            let start = at as usize * 4;
            assert_eq!(
                LevelGroup::from_bytes(
                    packed.bytes[start..start + LEVEL_GROUP_STRIDE]
                        .try_into()
                        .expect("one whole record")
                ),
                *record
            );
        }
        for (index, mesh) in level_meshes.iter().enumerate() {
            let at = packed.offsets.level_meshes_at + u32::try_from(index).expect("small");
            assert_eq!(word_at(at), *mesh, "level table at level_meshes_at");
        }
    }

    /// A renderer whose meshes have no hierarchy is the ordinary case, and every
    /// index the shader can still form has to land inside the buffer.
    ///
    /// `first_level` names element zero of the level region for a flat mesh too,
    /// so a region packed at zero length would put that read past the end.
    #[test]
    fn the_empty_selection_regions_are_padded_to_one_record() {
        let packed = pack_tables(&[3], &[0], &[0], &[], &[], &[]).expect("addresses");
        let words = u32::try_from(packed.bytes.len() / 4).expect("small");
        assert!(
            packed.offsets.level_meshes_at < words,
            "the level region starts at word {} of a {words}-word buffer",
            packed.offsets.level_meshes_at
        );
        assert!(
            packed.offsets.level_groups_at < packed.offsets.level_meshes_at,
            "an empty group region still holds a record"
        );
        assert!(
            packed.offsets.mesh_levels_at < packed.offsets.level_groups_at,
            "an empty per-mesh region still holds a record"
        );
    }

    /// The argument words are in the order every API's own structure puts them,
    /// and a round trip through the bytes preserves all five — including the
    /// signed one, which is the field a naive `as u32` cast would flatten.
    #[test]
    fn the_argument_words_round_trip_in_the_order_the_apis_fixed() {
        let args = DrawIndexedArgs {
            index_count: 36,
            instance_count: 3,
            first_index: 12,
            vertex_offset: -7,
            first_instance: 0,
        };
        let bytes = args.to_bytes();
        assert_eq!(bytes.len(), 20, "sizeof(VkDrawIndexedIndirectCommand)");
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 36, "index_count first");
        assert_eq!(uint_at(4), 3, "then instance_count");
        assert_eq!(uint_at(8), 12, "then first_index");
        assert_eq!(uint_at(12), (-7i32) as u32, "then the signed vertex_offset");
        assert_eq!(uint_at(16), 0, "then first_instance");
        assert_eq!(DrawIndexedArgs::from_bytes(&bytes), args);
    }

    /// The word indices this module implies and the ones the shader stores
    /// through must be the same, or the arguments are written into each other's
    /// fields and a driver reads a plausible, wrong draw.
    #[test]
    fn the_shader_names_the_same_word_indices_this_module_lays_out() {
        let source = include_str!("../shaders/draw_gen.slang");
        for (name, index) in [
            ("ARG_INDEX_COUNT", 0),
            ("ARG_INSTANCE_COUNT", 1),
            ("ARG_FIRST_INDEX", 2),
            ("ARG_VERTEX_OFFSET", 3),
            ("ARG_FIRST_INSTANCE", 4),
        ] {
            let declaration = format!("static const uint {name} = {index};");
            assert!(
                source.contains(&declaration),
                "draw_gen.slang does not declare `{declaration}`"
            );
        }
        let declaration = format!("static const uint DRAW_ARGS_WORDS = {DRAW_ARGS_WORDS};");
        assert!(
            source.contains(&declaration),
            "draw_gen.slang does not declare `{declaration}`"
        );
    }

    /// **The scatter's routing key is the mesh id and the material mode**, in
    /// the bits this crate writes them into.
    ///
    /// Three claims, and the third is the one a constant check alone would
    /// miss. The two `static const uint` lines have to be the host's numbers, or
    /// the shader reads the mode out of the wrong bits of
    /// [`GpuInstance::flags`](crate::mesh::GpuInstance::flags) and every
    /// instance answers mode zero — which routes a cutout into the opaque bucket
    /// and looks exactly like a scene with no cutout in it. And the scatter's
    /// skip has to compare **both** halves: a shader that grew the region, the
    /// offset and the accessor while going on comparing the mesh alone passes
    /// every layout check in this module and sends both twins to whichever
    /// bucket comes first.
    #[test]
    fn the_scatter_routes_by_the_mesh_and_the_material_mode() {
        use crate::mesh::GpuInstance;

        let source = include_str!("../shaders/draw_gen.slang");
        for (name, value) in [
            (
                "INSTANCE_MATERIAL_MODE_SHIFT",
                GpuInstance::MATERIAL_MODE_SHIFT,
            ),
            (
                "INSTANCE_MATERIAL_MODE_MASK",
                GpuInstance::MATERIAL_MODE_MASK,
            ),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&declaration),
                "draw_gen.slang does not declare `{declaration}`, so the pass reads the \
                 material mode out of bits the host does not write it into"
            );
        }
        assert!(
            source.contains("if (bucket_mesh(bucket) != mesh_id || bucket_mode(bucket) != mode)"),
            "draw_gen.slang's scatter no longer skips a bucket whose mode differs, so a mesh's \
             opaque and masked twins collapse into whichever bucket the table lists first"
        );
    }

    /// The mesh-dispatch words are in the order every API's own structure puts
    /// them, and a round trip preserves all three.
    ///
    /// A driver reads these bytes as workgroup counts. Two of them swapped is a
    /// dispatch of `clusters` instances of `instances` clusters, which on a
    /// scene with one instance and one cluster is the same number and on any
    /// other is a frame that draws the wrong amount of geometry.
    #[test]
    fn the_mesh_dispatch_words_round_trip_in_the_order_the_apis_fixed() {
        let args = MeshTasksArgs {
            group_count_x: 5,
            group_count_y: 2,
            group_count_z: 1,
        };
        let bytes = args.to_bytes();
        assert_eq!(bytes.len(), 12, "sizeof(VkDrawMeshTasksIndirectCommandEXT)");
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 5, "group_count_x first");
        assert_eq!(uint_at(4), 2, "then group_count_y");
        assert_eq!(uint_at(8), 1, "then group_count_z");
        assert_eq!(MeshTasksArgs::from_bytes(&bytes), args);
    }

    /// The word indices this module implies and the ones the shader stores
    /// through must be the same, for
    /// [`the_shader_names_the_same_word_indices_this_module_lays_out`]'s
    /// reason.
    ///
    /// [`the_shader_names_the_same_word_indices_this_module_lays_out`]: fn@the_shader_names_the_same_word_indices_this_module_lays_out
    #[test]
    fn the_shader_names_the_same_mesh_dispatch_word_indices_this_module_lays_out() {
        let source = include_str!("../shaders/draw_gen.slang");
        for (name, index) in [
            ("MESH_ARG_GROUP_X", 0),
            ("MESH_ARG_GROUP_Y", 1),
            ("MESH_ARG_GROUP_Z", 2),
        ] {
            let declaration = format!("static const uint {name} = {index};");
            assert!(
                source.contains(&declaration),
                "draw_gen.slang does not declare `{declaration}`"
            );
        }
        let declaration = format!("static const uint MESH_ARGS_WORDS = {MESH_ARGS_WORDS};");
        assert!(
            source.contains(&declaration),
            "draw_gen.slang does not declare `{declaration}`"
        );
    }
}
