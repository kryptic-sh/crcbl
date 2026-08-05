// The engine's first compute shader, and the only source here that draws
// nothing.
//
// It exists because the compute half of `crcbl-hal` had no test that reached a
// driver: `begin_compute_pass`, `bind_compute_pipeline`, `dispatch` and
// `dispatch_indirect` were covered only by the null backend, which records the
// calls and executes none of them. A dispatch that silently did nothing is
// indistinguishable from a correct one until something reads the result back,
// and reading a result back needs a shader that produces one.
//
// # Squares, so the expected value is not this shader
//
// The CPU side has to be able to state what the result should be *without*
// re-deriving it from this file, or the check passes whatever the dispatch did.
// A square is a closed form: given `source[i]`, the test writes `source[i] *
// source[i]` out for itself. Feeding a value that is distinct per index then
// means an index the shader got wrong lands a wrong value in a wrong slot,
// rather than a right one somewhere else.
//
// # The element count is a uniform, not a push constant and not a length query
//
// Push constants have no WGSL spelling — see this crate's docs on `ui.wgsl` —
// and no shader added here may use them until that is fixed. `GetDimensions` on
// a `RWStructuredBuffer` was the other candidate and is rejected by Slang's
// *Metal* target ("entrypoint uses features that are not available in 'compute'
// stage for 'metal' compilation target"), so it would have made this artifact
// unbuildable on one of the three targets the compile script emits. A uniform
// buffer is what `mesh.slang` already uses for the same reason.

/// The one thing an invocation needs that is not buffer contents, matching
/// `crcbl_shaders::compute_probe::Params`.
struct Params
{
    /// Elements in `source` and `destination`. A dispatch is a whole number of
    /// workgroups, so the last one runs past the end whenever this is not a
    /// multiple of the workgroup size.
    uint count;
};

[[vk::binding(0, 0)]]
ConstantBuffer<Params> params;

/// Read-only input. `StructuredBuffer` rather than `RWStructuredBuffer` is what
/// makes this a `BindingKind::StorageBuffer { read_only: true }` on the Rust
/// side, exactly as in `triangle.slang`.
[[vk::binding(1, 0)]]
StructuredBuffer<uint> source;

/// Written output, one element per invocation.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> destination;

/// One dimension, so a dispatch is `(groups, 1, 1)` and the element index is the
/// global invocation index. That is the smallest mapping which still leaves a
/// visible boundary in the output when only part of the buffer is dispatched —
/// which is how `dispatch_indirect` is told apart from `dispatch`.
///
/// `crcbl_shaders::compute_probe::WORKGROUP_SIZE` carries the same number, and
/// that module's test reads this file to check the two have not drifted.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= params.count)
    {
        return;
    }

    destination[index] = source[index] * source[index];
}
