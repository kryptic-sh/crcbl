// crcbl-targets: spirv, msl

// The argument structures a *count-limited* multi-draw issues, packed by the
// GPU because the count is a GPU value.
//
// # Why this shader exists, and why only Metal reads it
//
// `CommandEncoder::draw_indirect_count` takes its draw count from device
// memory. Vulkan and D3D12 each have a call that reads it —
// `vkCmdDrawIndirectCount` and an `ExecuteIndirect` with a count buffer — and
// Metal has none: `drawPrimitives:indirectBuffer:indirectBufferOffset:` emits
// exactly one draw and reads no count at all.
//
// What Metal *does* have is a draw whose every argument is a GPU-written word,
// **including the instance count** — and a draw of zero instances renders
// nothing. That is the whole mechanism: the pass issues `max_draw_count`
// ordinary indirect draws unconditionally, and this kernel makes the ones past
// the count draw nothing by giving them no instances.
//
// So the target list is `spirv, msl` rather than all four. `spirv` is required
// of every source — the entry points the manifest records are read out of it —
// and `msl` is the artifact `crcbl-mtl` compiles. WGSL and DXIL are not
// declared because no backend would load them: `crcbl-webgpu` has no
// count-buffer draw to emulate and refuses the capability outright, and
// `crcbl-dx12` issues a real count-limited `ExecuteIndirect`.
//
// # It packs into a second buffer rather than patching the first
//
// The obvious form is to zero `instanceCount` **in place**, in the caller's own
// argument buffer. It is one word per structure and no allocation, and it is
// silently wrong the second time the same buffer is drawn from: the zeroes
// survive, so a later frame whose count is *larger* draws no instances for
// every structure an earlier, smaller count happened to reach. Nothing observes
// that but the picture.
//
// This kernel therefore copies. Structure `i` of the caller's buffer is read at
// `first_word + i * source_stride_words` and written tightly packed at
// `i * structure_words` of a buffer the backend owns, and the draws read the
// packed copy. The caller's arguments are never written to, which is what every
// other backend's count-limited draw also promises.
//
// # What a structure is, and why this shader is not told
//
// `MTLDrawPrimitivesIndirectArguments` is four `uint32_t` and
// `MTLDrawIndexedPrimitivesIndirectArguments` is five — field for field the
// layouts Vulkan calls `VkDrawIndirectCommand` and `VkDrawIndexedIndirectCommand`.
// The instance count is the second word of both. **Neither number is written
// down here**: `structure_words` and `instance_word` arrive in the uniform
// block, computed by the host from the layout it already owns, exactly as
// `clear_counters.slang` takes its lengths rather than re-declaring
// `draw_gen.slang`'s argument layout.

/// The one-block description of the copy, matching
/// `crcbl_shaders::indirect_count_args::Params`.
struct PackParams
{
    /// Index of the `uint` draw count within `count`.
    ///
    /// A word index rather than a bound buffer offset, so the host never has to
    /// know what offset alignment a `device` buffer binding requires on the
    /// device it is running on. Both buffers below are bound at zero.
    uint count_word;
    /// Index of the first source structure's first word within `source`.
    uint first_word;
    /// Words between consecutive source structures. The caller's stride, which
    /// may be larger than a structure — that is
    /// `Capability::IndirectArgumentPaddedStride`, and it is honoured here by
    /// striding the *read* while the write stays tight.
    uint source_stride_words;
    /// Words in one argument structure, which is also the packed stride.
    uint structure_words;
    /// Index of the instance count within a structure. Zeroed for every
    /// structure at or past the draw count, which is what makes that draw a
    /// no-op.
    uint instance_word;
    /// Structures the pass will issue, and the ceiling the count is clamped to.
    ///
    /// The dispatch is sized from this, and it is checked again inside the
    /// kernel because a workgroup covers `WORKGROUP_SIZE` indices whether or
    /// not the bound is a multiple of it.
    uint max_draw_count;
};

[[vk::binding(0, 0)]]
ConstantBuffer<PackParams> pack;

/// The buffer holding the GPU-written draw count, read at
/// `pack.count_word`.
[[vk::binding(1, 0)]]
StructuredBuffer<uint> count;

/// The caller's argument buffer, as the flat array of words this kernel copies
/// out of. Read-only: see the header.
[[vk::binding(2, 0)]]
StructuredBuffer<uint> source;

/// The backend's packed copy, which the draws read. One structure per issued
/// draw, `pack.structure_words` apart, with no padding between them.
[[vk::binding(3, 0)]]
RWStructuredBuffer<uint> packed;

/// Invocations per workgroup, matching
/// `crcbl_shaders::indirect_count_args::WORKGROUP_SIZE`.
///
/// One invocation packs one whole structure — four or five words — rather than
/// one word, because the number of structures is the bound the host already
/// has and a word-granular grid would need the product of two host values to
/// size it for no gain at this scale.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint structure = thread.x;
    if (structure >= pack.max_draw_count)
    {
        return;
    }
    // The seam's rule, and the one a caller can observe: a count buffer holding
    // more than `max_draw_count` still draws `max_draw_count` times, which is
    // what `vkCmdDrawIndirectCount` does with the same two numbers.
    uint drawn = min(count[pack.count_word], pack.max_draw_count);
    uint from = pack.first_word + structure * pack.source_stride_words;
    uint to = structure * pack.structure_words;
    for (uint word = 0; word < pack.structure_words; ++word)
    {
        packed[to + word] = source[from + word];
    }
    // Past the count: the structure is issued as a draw all the same, and draws
    // nothing because it has no instances.
    if (structure >= drawn)
    {
        packed[to + pack.instance_word] = 0;
    }
}
