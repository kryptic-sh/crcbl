//! The workgroup size, uniform block and argument layout `draw_gen.slang`
//! declares, in the layouts that shader declares.
//!
//! Same reason as [`crate::cull`]: the shader fixes a number and a byte layout,
//! every producer and every reader of those has to agree with it exactly, and
//! keeping both in the crate that owns the source means there is one place to
//! change rather than one per consumer.
//!
//! What is *not* here is the bucket table. Which meshes are buckets is the
//! renderer's decision — [`crcbl_render::ForwardRenderer`] builds one bucket per
//! resident mesh — and this crate has no idea what is resident.
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
/// Three `uint`, the padding `std140` puts in front of the `float4` that
/// follows, and two `float4`. Checked against the `Offset` decorations `slangc`
/// emits by this module's
/// `the_draw_gen_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 48;

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
    /// The eye the uniform cut is selected from, in world space.
    ///
    /// The same three floats a frame writes into
    /// [`FrameUniforms::camera_position`](crate::mesh::FrameUniforms::camera_position):
    /// the two geometry paths select at different granularities and must never
    /// select from different cameras.
    pub camera_position: [f32; 3],
    /// How many pixels one unit of length subtends one unit from the eye, and
    /// the pixel budget a group's projected error is compared against — the two
    /// numbers of
    /// [`FrameUniforms::lod_params`](crate::mesh::FrameUniforms::lod_params),
    /// for that field's reason.
    pub lod_params: [f32; 2],
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
        ]
        .into_iter()
        .enumerate()
        {
            put(slot * 4, value.to_le_bytes());
        }
        // Offset 12 is `pad0`, and 16 is where `std140` puts the first `float4`.
        for (axis, value) in self.camera_position.into_iter().enumerate() {
            put(16 + axis * 4, value.to_le_bytes());
        }
        for (slot, value) in self.lod_params.into_iter().enumerate() {
            put(32 + slot * 4, value.to_le_bytes());
        }
        bytes
    }
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
        // 32.
        assert_eq!(PARAMS_SIZE, 48);
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
            camera_position: [1.5, 2.5, 3.5],
            lod_params: [4.5, 5.5],
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 2, "bucket_count at offset 0");
        assert_eq!(uint_at(4), 5, "bucket_capacity at offset 4");
        assert_eq!(uint_at(8), 9, "visible_capacity at offset 8");
        assert_eq!(uint_at(12), 0, "pad0 at offset 12, and it is written");
        assert_eq!(float_at(16), 1.5, "camera_position at offset 16");
        assert_eq!(float_at(24), 3.5, "and it is three floats wide");
        assert_eq!(uint_at(28), 0, "the fourth component is padding, and zero");
        assert_eq!(float_at(32), 4.5, "lod_params at offset 32");
        assert_eq!(float_at(36), 5.5, "and its budget beside it");
        assert!(
            bytes[40..].iter().all(|byte| *byte == 0),
            "the std140 tail padding is written, and it is zero: {:?}",
            &bytes[40..]
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
