// crcbl-targets: spirv, wgsl, msl, dxil

// Zeroing the counters `cull.slang`, `draw_gen.slang` and
// `mesh_cluster.slang`'s amplification stage only ever add to, from inside the
// frame graph.
//
// All three accumulate: the two survivor counts and each bucket's instance
// count are atomic adds, so a counter carrying the previous frame's total makes
// an argument structure claim more instances than its run holds, or a culling
// statistic report a frame that has already been drawn. Something has to write
// the zero, and this is that something.
//
// # Why a dispatch, and not the fill the seam already has
//
// `CommandEncoder::fill_buffer` is the idiomatic way to zero an indirect count
// and it is what this would be if the four backends could all record one. They
// cannot: `crcbl-dx12` refuses it, because D3D12's fill is
// `ClearUnorderedAccessViewUint` and that takes a descriptor from a
// shader-visible heap, which is a frame resource belonging to a slice that has
// a root signature to bind it against. Metal's `fillBuffer:range:value:` also
// repeats a *byte* rather than a word, and wgpu offers only a zero clear.
//
// A dispatch has none of that: every backend that can run the two passes this
// zeroes for can run this one, by construction. It also lands *inside* the
// graph, where a fill cannot go — copies and fills are legal only outside a
// pass, and a render-graph frame is passes end to end — which is what lets the
// three counters be device-local at last. They were `HostUpload` only so the
// CPU could zero them before the frame, and D3D12 has no unordered-access view
// of an upload-heap resource at all.
//
// # It is its own dispatch, not a prologue inside `cull.slang`
//
// Zeroing a counter that another invocation of the same dispatch may already
// have added to is a race with no fix inside one dispatch: there is no
// cross-workgroup ordering to hang it on. The zero has to complete, and be made
// visible, before the first `add` — which is a barrier, and a barrier between
// two dispatches is two passes. The render graph computes it from what each
// pass declares.

/// The three lengths this pass cannot derive for itself, matching
/// `crcbl_shaders::clear_counters::Params`.
struct ClearParams
{
    /// Words in the indirect-argument buffer: one bucket's structure is
    /// `crcbl_shaders::draw_gen::DRAW_ARGS_WORDS` of them, and there is one
    /// structure per bucket. Computed on the host from that constant, so this
    /// shader does not re-declare the argument layout.
    uint args_words;
    /// Words in the per-bucket draw-count buffer, which is one per bucket.
    uint counts_words;
    /// Words in the culling-statistics buffer, which is
    /// `crcbl_shaders::cull::STATS_WORDS`. A parameter rather than a literal
    /// because two different shaders own one word each of it, and a shader that
    /// zeroed a prefix would leave the other's counter carrying last frame's
    /// total.
    uint stats_words;
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size.
    uint pad0;
};

[[vk::binding(0, 0)]]
ConstantBuffer<ClearParams> clear;

/// The frame's culling statistics: `cull.slang`'s surviving instances in
/// element 0 and `mesh_cluster.slang`'s surviving clusters in element 1. Both
/// are atomic adds, so both start here at zero.
[[vk::binding(1, 0)]]
RWStructuredBuffer<uint> cull_stats;

/// The indirect arguments, as the flat array of words `draw_gen.slang` writes
/// them through. Zeroed whole rather than only at the accumulating word: the
/// generating pass rewrites every static word of every bucket it owns, so there
/// is nothing here worth preserving.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> args;

/// One draw count per bucket. `draw_gen.slang` stores a `1` into a bucket that
/// took a survivor and leaves the rest at the zero written here, which is how a
/// bucket with nothing in it draws nothing.
[[vk::binding(3, 0)]]
RWStructuredBuffer<uint> draw_counts;

/// Invocations per workgroup, matching
/// `crcbl_shaders::clear_counters::WORKGROUP_SIZE`.
///
/// One invocation covers the same index in all three buffers, so a dispatch is
/// sized for the longest of them. That is `args_words`, which is never zero
/// while there is at least one bucket — and a bucket table with no buckets
/// generates no draws and is refused where it is built.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;

    if (index < clear.stats_words)
    {
        cull_stats[index] = 0;
    }
    if (index < clear.args_words)
    {
        args[index] = 0;
    }
    if (index < clear.counts_words)
    {
        draw_counts[index] = 0;
    }
}
