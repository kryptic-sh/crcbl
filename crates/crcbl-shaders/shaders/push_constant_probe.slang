// crcbl-targets: spirv, msl, dxil

// The only shader here that reads a push constant, and it draws nothing.
//
// `crcbl-hal` carries `PushConstantRange`, `Features::PUSH_CONSTANTS` and
// `CommandEncoder::push_constants`, and nothing has ever dispatched through
// them: every `push_constants:` field in `crcbl-render` is `None`, and until
// this file no committed artifact declared a block at all. So each backend's
// push-constant path — `vkCmdPushConstants`, a D3D12 root constant, Metal's
// `setBytes:length:atIndex:` — had no shader to be proved against, and nothing
// in the tree could even say which slot a compiled artifact puts the block at.
// That question is this file's whole payload; the three answers are recorded in
// `crcbl_shaders::push_constant_probe`, read out of the artifacts beside this
// source.
//
// # The output is the constants, so nothing else can have produced it
//
// One invocation writes one word of the block into `destination`, and the
// dispatch reads nothing else — no vertex data, no uniform, no bound buffer of
// any kind. A readback holding the pattern the caller pushed is therefore a
// value the shader had no other way to obtain, where a shader that also read a
// bound buffer could have passed by copying that instead.
//
// Word *i* to element *i*, rather than a checksum of the block: a checksum is
// satisfied by the right bytes at the wrong offsets, and the offsets are
// exactly what differs between a Vulkan push-constant range, a root-constant
// slot and a Metal buffer argument.
//
// # No WGSL, and Slang's own output is the reason
//
// WGSL has no push constants, and Slang's WGSL target does not say so. At
// 2026.14 it compiles this source with **exit 0** and emits
//
//     var<uniform> constants_0 : ProbeConstants_std430_0;
//
// with no `@group`/`@binding` — which is not valid WGSL, and is the exact
// defect `wgsl/ui.wgsl` shipped with until 2026-08. The `// crcbl-targets:`
// line above is what keeps that artifact out of the tree.
//
// It is not the only guard. Add `wgsl` to that line and the compile script will
// commit the broken module, but `tests/wgsl_validation.rs` validates every
// `wgsl/*.wgsl` with naga and refuses this one by name — so the exclusion holds
// even if the declaration is edited by someone who has not read this comment.
// See this crate's docs, "A push constant costs the WGSL target".

/// Words the block carries, the elements `destination` needs, and the length of
/// `ProbeConstants::values`. Matches
/// `crcbl_shaders::push_constant_probe::WORD_COUNT`.
static const uint WORD_COUNT = 4;

/// The written output — the only resource this shader binds, and the reason it
/// is declared before the push constant. Slang's Metal target hands out buffer
/// indices in declaration order and ignores `[[vk::binding]]`, so a push
/// constant declared first would take `buffer(0)` and shift this off the slot
/// `crcbl-mtl` computes for it. `crcbl_shaders`' `declaration_order` lint is
/// what enforces that, and this is the first shipped source to exercise its
/// push-constant arm.
[[vk::binding(0, 0)]]
RWStructuredBuffer<uint> destination;

/// The push-constant block, matching
/// `crcbl_shaders::push_constant_probe::Constants`.
///
/// A `uint4` rather than a `uint[4]`, and the difference is not cosmetic. Slang
/// lays a push-constant block out as `std430`, so an array is sixteen tightly
/// packed bytes in the SPIR-V either way — but an array in an HLSL `cbuffer`
/// takes a sixteen-byte register per element, and `dxc` reports the resulting
/// constant buffer as fifty-two bytes where the `uint4` form is sixteen. One
/// `PushConstantRange` describes every backend only while the block's layout
/// does not vary by target, so the vector is the spelling that keeps it so.
struct ProbeConstants
{
    /// The words written out, in order: `values[i]` lands in `destination[i]`.
    uint4 values;
};

/// Declared last, after every numbered binding — see `destination` above.
[[vk::push_constant]]
ConstantBuffer<ProbeConstants> constants;

/// One group covers the block: a caller dispatches `(1, 1, 1)`.
///
/// The workgroup is wider than `WORD_COUNT` on purpose, so the bounds check
/// below is a branch the intended dispatch really takes rather than dead code:
/// most of the group's invocations return, and a shader that lost the guard
/// would write past the destination buffer on the very first dispatch.
///
/// `crcbl_shaders::push_constant_probe::WORKGROUP_SIZE` carries the same number,
/// and that module's test reads this file to check the two have not drifted.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= WORD_COUNT)
    {
        return;
    }

    destination[index] = constants.values[index];
}
