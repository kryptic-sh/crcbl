//! The workgroup size, uniform block and per-vertex skin binding
//! `skinning.slang` declares, in the layouts that shader declares.
//!
//! Same reason as [`crate::clear_counters`]: the shader fixes a number and two
//! byte layouts, every producer of those has to agree with it exactly, and
//! keeping them in the crate that owns the source means there is one place to
//! change rather than one per consumer.
//!
//! # What the pass is
//!
//! `docs/plan/17-animation.md`'s skinning prepass. A joint palette and a run of
//! bind-pose vertices go in; the same vertices, blended onto the palette, come
//! out in a **transient region of the same vertex pool** — and the renderer
//! draws that region like any static mesh, because what the pass writes is
//! [`crate::mesh::MeshVertex`] byte for byte. Vertex pulling never learns
//! skinning exists.
//!
//! # Nothing dispatches it yet
//!
//! There is no render-graph pass, no bind group and no host-side consumer
//! anywhere in the workspace. This is the shader and the layout, and only that;
//! the pass, the pool's transient region and the prev/current ping-pong topic
//! 17's 2026-07-27 correction asks for are later slices. Nothing here is a stub
//! — the shader is complete and its artifacts are committed — but a reader
//! looking for the thing that runs it will not find one.
//!
//! # The bind group the pass will need
//!
//! Set 0, in this order, which is the order `skinning.slang` declares them and
//! the order `the_bindings_are_declared_in_the_order_this_module_records`
//! holds it to:
//!
//! | Binding | Resource | What it is |
//! | ------- | -------- | ---------- |
//! | 0 | uniform buffer | [`Params`](crate::skinning::Params), [`PARAMS_SIZE`](crate::skinning::PARAMS_SIZE) bytes |
//! | 1 | read-only storage | the joint palettes, [`JOINT_STRIDE`](crate::skinning::JOINT_STRIDE) bytes each |
//! | 2 | read-only storage | the skin bindings, [`SKIN_BINDING_STRIDE`](crate::skinning::SKIN_BINDING_STRIDE) bytes each |
//! | 3 | read-write storage | the vertex pool, [`crate::mesh::VERTEX_STRIDE`] bytes each |
//!
//! **Binding 3 is the pool bound once**, read at
//! [`Params::input_base`](crate::skinning::Params::input_base) and written at
//! [`Params::output_base`](crate::skinning::Params::output_base), rather than a
//! read-only view and a writable view of one buffer. WebGPU refuses the second
//! arrangement — a buffer range used as writable storage may not appear in any
//! other way in the same usage scope — so it is a `createBindGroup` failure in
//! the browser and not a matter of taste.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/skinning.slang`.
///
/// One invocation is one vertex, so a caller dispatches
/// `vertex_count.div_ceil(WORKGROUP_SIZE)` groups. That rounds up, and the tail
/// invocations of the last group are stopped by the shader's own test against
/// [`Params::vertex_count`] — see `computeMain`, which says what they would
/// otherwise overwrite.
pub const WORKGROUP_SIZE: u32 = 64;

/// Joints one vertex can be bound to.
///
/// Four, because glTF's `JOINTS_0` is a four-component attribute and
/// `crcbl_scene::GltfPrimitive::joints` reads that set only — its module docs
/// decline `JOINTS_1`, so a vertex arrives bound to at most four joints and the
/// shader's [`SkinBinding`] is exactly that wide.
///
/// [`crcbl_scene::GltfPrimitive::joints`]: https://docs.rs/crcbl-scene
pub const JOINTS_PER_VERTEX: usize = 4;

/// Bytes per palette matrix, and the stride of the joint-palette storage
/// buffer.
///
/// One `float4x4`, stored the way [`glam::Mat4::to_cols_array`] produces it and
/// read with no transpose — `shaders/mesh.slang`'s header is where that
/// convention is written down in full, and `slangc` decorates this buffer
/// identically (`ArrayStride 64`, `RowMajor`, `MatrixStride 16`). So a
/// [`crcbl_anim::Palette::matrices`] slice is uploaded as its own bytes.
///
/// [`glam::Mat4::to_cols_array`]: https://docs.rs/glam
/// [`crcbl_anim::Palette::matrices`]: https://docs.rs/crcbl-anim
pub const JOINT_STRIDE: usize = 64;

/// Bytes per [`SkinBinding`], and the stride of the skin-binding storage
/// buffer.
///
/// A `uint4` then a `float4`, with no padding anywhere. Checked against the
/// `ArrayStride` and `Offset` decorations `slangc` emits by this module's
/// `the_skin_binding_layout_matches_the_offsets_slangc_emits`.
pub const SKIN_BINDING_STRIDE: usize = 32;

/// Bytes of the uniform block.
///
/// Six `uint`, rounded up to the 16-byte multiple `std140` requires of a
/// uniform block's size. Checked against the `Offset` decorations `slangc`
/// emits by this module's
/// `the_skin_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 32;

/// The uniform block, matching `struct SkinParams` in `shaders/skinning.slang`.
///
/// One block describes one **range**: a contiguous run of vertices belonging to
/// one skinned primitive of one animated instance. A frame with three animated
/// characters is three of these and three dispatches; the GPU-driven form,
/// where one dispatch walks a table of ranges, needs a range table that the
/// slice allocating the transient pool region owns and is deliberately not
/// guessed at here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// Vertices in this range. Invocations at or past it do nothing.
    pub vertex_count: u32,
    /// First vertex of the **bind-pose** run, as an index into the vertex pool.
    pub input_base: u32,
    /// First vertex of the **skinned** run, as an index into the same pool.
    ///
    /// The transient region `docs/plan/17-animation.md` asks for: a different
    /// range of one buffer rather than a second buffer, which is what lets a
    /// skinned mesh be drawn, culled and shadowed by passes that only ever knew
    /// about the pool.
    ///
    /// It may not overlap the bind-pose run, and [`to_bytes`](Self::to_bytes)
    /// is where that is enforced rather than merely asked for.
    pub output_base: u32,
    /// First entry of this range's run of [`SkinBinding`]s.
    ///
    /// Its own base rather than [`input_base`](Self::input_base) reused: the
    /// skin bindings are a buffer holding only skinned geometry, so a pool full
    /// of static meshes costs nothing there. One index serving both would force
    /// that buffer to be as long as the vertex pool.
    pub binding_base: u32,
    /// First matrix of this range's joint palette.
    ///
    /// One buffer holds every animated instance's palette, so a range names
    /// where its own begins.
    pub joint_base: u32,
    /// Joints in this range's palette.
    ///
    /// Every [`SkinBinding::joints`] entry is clamped against it by the shader,
    /// so a malformed asset naming a joint its skin has not got produces a
    /// wrong vertex rather than a read past the end of a storage buffer — which
    /// is undefined on the backends that do not bound every access.
    ///
    /// Never zero; [`to_bytes`](Self::to_bytes) refuses one.
    pub joint_count: u32,
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    ///
    /// The tail padding is written rather than left alone, for
    /// [`crate::cull::Params::to_bytes`]' reason: a buffer allocated for this
    /// block is [`PARAMS_SIZE`] bytes wide and a partial write leaves the rest
    /// undefined.
    ///
    /// # Panics
    ///
    /// Two preconditions of the dispatch are checked here, because this is the
    /// last place on the CPU that sees them and there is no place at all on the
    /// GPU that could:
    ///
    /// * **The two vertex ranges overlap.** They are ranges of one buffer, so a
    ///   vertex read after its own slot has been written is one invocation's
    ///   output feeding another's input, with nothing anywhere ordering the
    ///   two. There is no answer to give, and the picture it produces is a mesh
    ///   that deforms differently on every run and on every device.
    /// * **The palette is empty.** The shader clamps a joint index against
    ///   `joint_count - 1`, which for zero joints is every index in the buffer.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        assert!(
            self.joint_count > 0,
            "a skinned range with an empty joint palette has nothing to blend onto, and the \
             shader's index clamp would wrap"
        );
        let count = u64::from(self.vertex_count);
        let input = u64::from(self.input_base);
        let output = u64::from(self.output_base);
        assert!(
            count == 0 || input + count <= output || output + count <= input,
            "the bind-pose range {input}..{} and the skinned range {output}..{} overlap; they \
             are ranges of one buffer, so an invocation would read a vertex another invocation \
             has already overwritten",
            input + count,
            output + count
        );

        let mut bytes = [0u8; PARAMS_SIZE];
        for (slot, value) in [
            self.vertex_count,
            self.input_base,
            self.output_base,
            self.binding_base,
            self.joint_base,
            self.joint_count,
        ]
        .into_iter()
        .enumerate()
        {
            let at = slot * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// One vertex's joints and their weights, matching `struct SkinBinding` in
/// `shaders/skinning.slang`.
///
/// One per **skinned** vertex, in a buffer parallel to the bind-pose run rather
/// than to the whole vertex pool — see [`Params::binding_base`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinBinding {
    /// Which joints of this range's palette own the vertex, each relative to
    /// [`Params::joint_base`].
    ///
    /// # Why these are `u32` and not the `u16` the file stored
    ///
    /// glTF stores `JOINTS_0` as unsigned bytes or shorts and
    /// `crcbl_scene::GltfPrimitive::joints` widens either to `[u16; 4]`, so the
    /// obvious packing for the GPU is two `u32`s holding four halves. It buys
    /// **nothing**: `std430` aligns the `float4` of weights to 16, so a pair of
    /// `u32`s here is followed by eight bytes of padding and
    /// [`SKIN_BINDING_STRIDE`] is 32 either way. The packed spelling would cost
    /// the same memory, plus a shift and a mask per joint per vertex, plus a
    /// packing rule this side has to reproduce exactly — and WGSL has no 16-bit
    /// integer type to unpack into, so the browser would do that arithmetic in
    /// 32-bit registers regardless.
    ///
    /// Widening is therefore the cheaper choice as well as the plainer one, and
    /// it is a layout that cannot drift.
    ///
    /// [`crcbl_scene::GltfPrimitive::joints`]: https://docs.rs/crcbl-scene
    pub joints: [u32; JOINTS_PER_VERTEX],
    /// How much of the vertex each of those joints owns, in the same order.
    ///
    /// **Passed to the GPU exactly as the file stored them.** The specification
    /// asks that the four sum to one and
    /// `crcbl_scene::GltfPrimitive::weights` reports them unrenormalised, on
    /// the grounds that "a file whose weights do not sum is one whose author
    /// should hear about it"; the shader takes the same position, and its
    /// `computeMain` is where the whole argument is. A set that does not sum
    /// deflates its vertex toward the palette's origin, which is the loudest
    /// failure available and costs nothing to get.
    ///
    /// [`crcbl_scene::GltfPrimitive::weights`]: https://docs.rs/crcbl-scene
    pub weights: [f32; JOINTS_PER_VERTEX],
}

impl SkinBinding {
    /// The binding as the bytes a storage buffer holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; SKIN_BINDING_STRIDE] {
        let mut bytes = [0u8; SKIN_BINDING_STRIDE];
        let mut at = 0usize;
        for joint in self.joints {
            bytes[at..at + 4].copy_from_slice(&joint.to_le_bytes());
            at += 4;
        }
        for weight in self.weights {
            bytes[at..at + 4].copy_from_slice(&weight.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, SKIN_BINDING_STRIDE, "the struct has no padding at all");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source, once, for every test below that reads it.
    const SOURCE: &str = include_str!("../shaders/skinning.slang");

    /// The constant and the shader must name the same workgroup size.
    ///
    /// Nothing else can catch this, for [`crate::cull`]'s twin's reason: the
    /// shader compiles, the dispatch succeeds, and a dispatch sized against the
    /// wrong number skins a *prefix* of the range — so the character's head
    /// animates and its legs stay in the bind pose, which reads as a rigging
    /// mistake rather than as a dispatch that was too short.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_skinning_slang_declares() {
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            SOURCE.contains(&declaration),
            "skinning.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted from \
             the shader"
        );
    }

    /// The offsets `slangc` actually emitted for `SkinParams`, read out of the
    /// disassembly.
    #[test]
    fn the_skin_params_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %SkinParams_std140 n Offset …`: 0, 4, 8, 12, 16, 20.
        assert_eq!(PARAMS_SIZE, 32);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a block that is \
             not one already is a block the shader and the CPU disagree about the width of"
        );
        let bytes = Params {
            vertex_count: 11,
            input_base: 22,
            output_base: 33,
            binding_base: 44,
            joint_base: 55,
            joint_count: 66,
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 11, "vertex_count at offset 0");
        assert_eq!(uint_at(4), 22, "input_base at offset 4");
        assert_eq!(uint_at(8), 33, "output_base at offset 8");
        assert_eq!(uint_at(12), 44, "binding_base at offset 12");
        assert_eq!(uint_at(16), 55, "joint_base at offset 16");
        assert_eq!(uint_at(20), 66, "joint_count at offset 20");
        assert!(
            bytes[24..].iter().all(|byte| *byte == 0),
            "the std140 tail padding is written, and it is zero: {:?}",
            &bytes[24..]
        );
    }

    /// The offsets and stride `slangc` actually emitted for `SkinBinding`.
    ///
    /// A drift here is silent in the way a vertex format always is: the shader
    /// reads the four weights out of the bytes where the four joint indices
    /// are, which is a finite float for every bit pattern a small index can
    /// have, so the mesh deforms wrongly and nothing reports anything.
    #[test]
    fn the_skin_binding_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_SkinBinding_std430 ArrayStride 32`, and
        // `OpMemberDecorate %SkinBinding_std430 n Offset …`: 0, 16.
        assert_eq!(SKIN_BINDING_STRIDE, 32);
        let bytes = SkinBinding {
            joints: [1, 2, 3, 4],
            weights: [0.5, 0.25, 0.125, 0.0625],
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        for index in 0..JOINTS_PER_VERTEX {
            assert_eq!(
                uint_at(index * 4),
                index as u32 + 1,
                "joint {index} at offset {}",
                index * 4
            );
        }
        assert_eq!(float_at(16), 0.5, "weights start at offset 16");
        assert_eq!(float_at(20), 0.25, "weight 1 at offset 20");
        assert_eq!(float_at(24), 0.125, "weight 2 at offset 24");
        assert_eq!(float_at(28), 0.0625, "weight 3 at offset 28");
    }

    /// **The pass writes what the vertex stage pulls**, which is the whole
    /// design: a skinned draw is an ordinary draw over a different range of the
    /// pool.
    ///
    /// So the stride the shader writes at is [`crate::mesh::VERTEX_STRIDE`],
    /// and `slangc` says so — `OpDecorate %_runtimearr_MeshVertex_std430
    /// ArrayStride 64`, with members at 0, 16, 32 and 48. A skinned vertex
    /// written at any other stride lands the renderer's next vertex inside this
    /// one.
    #[test]
    fn the_skinned_vertex_is_written_at_the_pools_own_stride() {
        assert_eq!(crate::mesh::VERTEX_STRIDE, 64);
        assert_eq!(
            crate::mesh::VERTEX_STRIDE,
            4 * 16,
            "four float4s, which is what both `struct MeshVertex` declarations say"
        );
    }

    /// **Which resource is at which binding**, in the order the shader declares
    /// them.
    ///
    /// `crate::declaration_order` checks that every source numbers its
    /// resources in the order it declares them, which is `crcbl-mtl`'s rule and
    /// says nothing about *what* is at binding 2. This says it: a bind group
    /// built against a different assignment binds the palette where the skin
    /// bindings belong, and every backend accepts that layout happily because
    /// both are read-only storage buffers of the right kind.
    ///
    /// The declared text rather than a parsed type, so a `StructuredBuffer` that
    /// became an `RWStructuredBuffer` — which changes the descriptor type a
    /// layout has to ask for — is a failure too.
    #[test]
    fn the_bindings_are_declared_in_the_order_this_module_records() {
        let expected = [
            "ConstantBuffer<SkinParams> skin",
            "StructuredBuffer<float4x4> joints",
            "StructuredBuffer<SkinBinding> bindings",
            "RWStructuredBuffer<MeshVertex> vertices",
        ];
        let found = bindings(SOURCE);
        assert_eq!(
            found.len(),
            expected.len(),
            "skinning.slang declares {} bound resources, not {}: {found:?}",
            found.len(),
            expected.len()
        );
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                found[index],
                (index as u32, (*want).to_string()),
                "binding {index} is not what this module's bind-group table records"
            );
        }
    }

    /// Every `[[vk::binding(n, 0)]]` in `source`, paired with the declaration
    /// it decorates, in declaration order.
    ///
    /// Set 1 and up would be dropped silently, which is why
    /// [`the_binding_scan_reads_the_declaration_it_decorates`] asserts on a
    /// fixture: `skinning.slang` has one set and a scan that quietly found
    /// nothing would make an empty file agree with every expectation.
    fn bindings(source: &str) -> Vec<(u32, String)> {
        let mut found = Vec::new();
        let mut lines = source.lines();
        while let Some(line) = lines.next() {
            let Some(rest) = line.trim().strip_prefix("[[vk::binding(") else {
                continue;
            };
            let Some((number, _)) = rest.split_once(',') else {
                continue;
            };
            let Ok(binding) = number.trim().parse::<u32>() else {
                continue;
            };
            let declaration = lines
                .next()
                .expect("a binding attribute is never the last line of a shader")
                .trim()
                .trim_end_matches(';')
                .to_string();
            found.push((binding, declaration));
        }
        found
    }

    /// The scan reads the line after the attribute, and reads the number out of
    /// the attribute — so the comparison above is over what the shader really
    /// says and not over an empty list.
    #[test]
    fn the_binding_scan_reads_the_declaration_it_decorates() {
        let source = "[[vk::binding(0, 0)]]\nConstantBuffer<P> p;\n\n\
                      [[vk::binding(7, 0)]]\nRWStructuredBuffer<uint> q;\n";
        assert_eq!(
            bindings(source),
            vec![
                (0, "ConstantBuffer<P> p".to_string()),
                (7, "RWStructuredBuffer<uint> q".to_string()),
            ]
        );
        assert!(bindings("uint x;\n").is_empty());
    }

    /// Overlapping ranges are refused, and the refusal is not a rule that
    /// refuses everything.
    ///
    /// Four cases, and the two that must pass are what stop this reporting a
    /// correct dispatch as broken: ranges that touch without overlapping are
    /// the ordinary case for a pool that suballocates back to back.
    #[test]
    fn to_bytes_refuses_two_vertex_ranges_that_overlap() {
        let range = |input, output, count| Params {
            vertex_count: count,
            input_base: input,
            output_base: output,
            binding_base: 0,
            joint_base: 0,
            joint_count: 1,
        };
        // Disjoint, either way round, and touching at the boundary.
        let _ = range(0, 100, 100).to_bytes();
        let _ = range(100, 0, 100).to_bytes();
        // An empty range overlaps nothing, whatever the two bases say.
        let _ = range(50, 50, 0).to_bytes();

        for (input, output, count) in [(0, 50, 100), (50, 0, 100), (10, 10, 1)] {
            let error = std::panic::catch_unwind(|| range(input, output, count).to_bytes())
                .expect_err("overlapping ranges must be refused");
            let message = panic_message(&*error);
            assert!(
                message.contains("overlap"),
                "the panic names something other than the overlap: {message}"
            );
        }
    }

    /// An empty palette is refused, because the shader's index clamp is
    /// `joint_count - 1`.
    #[test]
    fn to_bytes_refuses_an_empty_joint_palette() {
        let error = std::panic::catch_unwind(|| {
            Params {
                vertex_count: 1,
                input_base: 0,
                output_base: 1,
                binding_base: 0,
                joint_base: 0,
                joint_count: 0,
            }
            .to_bytes()
        })
        .expect_err("an empty palette must be refused");
        let message = panic_message(&*error);
        assert!(
            message.contains("empty joint palette"),
            "the panic names something other than the palette: {message}"
        );
    }

    /// The text a caught panic carries.
    ///
    /// Both shapes, because `assert!` produces one of two payloads and which
    /// one is decided by whether the message has format arguments in it: a
    /// formatted message arrives as a `String` and a literal one as a
    /// `&'static str`. A test that asked for only the first reports a correct
    /// refusal as a failure, which is how this helper came to exist.
    fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
        payload.downcast_ref::<String>().map_or_else(
            || {
                (*payload
                    .downcast_ref::<&str>()
                    .expect("a panic payload is a String or a &str"))
                .to_string()
            },
            Clone::clone,
        )
    }

    /// **The shader stops the tail invocations itself**, and does it before it
    /// reads anything.
    ///
    /// A dispatch is `vertex_count.div_ceil(WORKGROUP_SIZE)` groups, so the
    /// last group carries up to `WORKGROUP_SIZE - 1` invocations for vertices
    /// the range has not got. Nothing else stops them: not a clamp, not the
    /// buffer's length, not a backend's robustness — the ranges either side of
    /// this one in the pool are real vertices belonging to another mesh, so a
    /// tail invocation that ran would overwrite them with plausible values and
    /// nothing would report a thing.
    #[test]
    fn the_shader_discards_the_invocations_past_the_range() {
        assert!(
            SOURCE.contains("if (index >= skin.vertex_count)"),
            "skinning.slang no longer discards the tail of the last workgroup"
        );
    }
}
