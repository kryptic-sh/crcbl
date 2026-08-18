// crcbl-targets: spirv, dxil

// The only shader here that declares an array of descriptors.
//
// `crcbl-hal` carries `BindingFlags::VARIABLE_COUNT`, `BindGroupDesc::
// variable_count`, `BindGroupEntry::array_index` and
// `Capability::BindlessDescriptorArray`, and until this file no committed
// artifact declared a resource array at all: every `Texture2DArray` in this
// directory is one layered image rather than an array of descriptors, no
// committed SPIR-V declares `RuntimeDescriptorArray`, and no WGSL contains
// `binding_array`. So each backend's bindless path — Vulkan's
// `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT`, a D3D12 descriptor
// range with an unbounded `NumDescriptors` — had no shader to be proved
// against, and `Capability::BindlessDescriptorArray` was the one binding-model
// capability the agnostic seam suite could not drive.
//
// # An array of buffers, not of textures
//
// `sources` is an array of read-only storage buffers. A texture array would
// make the readback depend on a sampler, a view type, a texel format and a
// filtering rule, none of which this capability is about — every one of those
// is a way for the test to fail while the descriptor array worked. Buffers have
// none: a word read out of `sources[i]` is the word the caller wrote into the
// buffer it bound at element `i`, exactly.
//
// # The array is last, and it is the highest-numbered binding
//
// `crcbl_shaders`' `declaration_order` lint requires ascending `(set, binding)`
// in declaration order, and `crcbl_hal::BindGroupLayoutDesc::check_entries`
// requires a `VARIABLE_COUNT` entry to be **both** the last entry of its layout
// and its highest binding number — Vulkan's rule is the second half, since a
// runtime-sized array is only legal on the highest-numbered binding of a set.
// `destination` at binding 0 and `sources` at binding 1 is the one order that
// satisfies all of it, and it is why the written buffer is declared first here
// where every other probe in this directory declares its inputs first.
//
// # The descriptor index is dynamically uniform, and that is not decoration
//
// Each workgroup reads one element of the array, so the index is
// `SV_GroupID.x`: one value for every invocation in the group. Vulkan's
// `shaderStorageBufferArrayDynamicIndexing` is defined only for a dynamically
// uniform index, and an index that is not needs `NonUniformEXT` on the access
// and `shaderStorageBufferArrayNonUniformIndexing` from the device — a
// different feature, a different SPIR-V decoration and a different test.
// `push_constant_probe.slang` records what a divergent index costs on real
// hardware when the rule is only *documented*; this shader keeps the index
// uniform by construction instead, and the word index inside the buffer — which
// is an ordinary load and has no such rule — is the divergent one.
//
// # No MSL and no WGSL
//
// `crcbl-mtl` answers `Support::No` for this capability and refuses every
// `BindingFlags` at `create_bind_group_layout`, because Metal binds flat
// argument tables of a fixed size. `crcbl-webgpu` refuses it too. Neither
// backend can reach a dispatch of this shader, so an MSL or WGSL artifact would
// be bytes nothing loads — the same reasoning `push_constant_raster.slang` gives
// for its own two-target declaration, arriving at a different pair.

/// Descriptors in the `sources` array — and workgroups one dispatch submits,
/// since each group reads one element.
///
/// Matches `crcbl_shaders::bindless_probe::SOURCE_COUNT`. Four rather than two,
/// so that a backend which bound the array's *first* descriptor for every index
/// — the flat-argument-table failure this capability exists to separate out — is
/// visible as three wrong elements rather than one.
static const uint SOURCE_COUNT = 4;

/// Words each source buffer holds, and words each workgroup copies out of it.
///
/// Matches `crcbl_shaders::bindless_probe::WORDS_PER_SOURCE`. More than one, so
/// that "the right descriptor" and "the right offset inside it" are separate
/// observations: a descriptor array bound one element out shifts whole blocks of
/// four, where a buffer read at the wrong offset shifts within a block.
static const uint WORDS_PER_SOURCE = 4;

/// The written output, at binding 0 — declared first because the array below it
/// must be both last and highest-numbered. See the header.
[[vk::binding(0, 0)]]
RWStructuredBuffer<uint> destination;

/// The descriptor array, unbounded: its length is chosen when the bind group is
/// created, which is what `BindGroupDesc::variable_count` names and what
/// `BindingFlags::VARIABLE_COUNT` makes legal.
///
/// Unbounded rather than `sources[SOURCE_COUNT]`, and the difference is the
/// whole capability. A fixed bound compiles to an ordinary array of four
/// descriptors that a flat argument table can hold; `[]` compiles to a SPIR-V
/// `OpTypeRuntimeArray` under the `RuntimeDescriptorArray` capability and to a
/// D3D12 descriptor range whose `NumDescriptors` is unbounded, which is the
/// declaration a backend either expresses or does not.
///
/// THE `register` ANNOTATION IS LOAD-BEARING, AND IT IS THE ONLY ONE IN THIS
/// DIRECTORY.
///
/// Slang moves an **unbounded** array into a register space of its own —
/// measured, not assumed: without it `dxc -dumpbin` reports this binding at
/// `t0,space1`, because an unbounded range would otherwise consume the rest of
/// its register class and Slang keeps it out of everything else's way.
/// `crcbl-dx12` builds every descriptor range with `RegisterSpace: 0` (see that
/// crate's `binding::ranges`), and its `dxil` module asserts space 0 over every
/// committed container, so an artifact in space 1 names a range no root
/// signature this engine builds describes. `[[vk::binding]]` governs the SPIR-V
/// and `register` the HLSL, so pinning the second changes nothing above.
[[vk::binding(1, 0)]]
StructuredBuffer<uint> sources[] : register(t0, space0);

/// One workgroup per array element, copying that element's words into the
/// element's block of `destination`.
///
/// The workgroup is wider than [`WORDS_PER_SOURCE`] on purpose, for
/// `push_constant_probe.slang`'s reason: most of the group's invocations return,
/// so the bounds check is a branch the intended dispatch really takes rather
/// than dead code, and a shader that lost it would write past the destination on
/// the very first dispatch.
///
/// `crcbl_shaders::bindless_probe::WORKGROUP_SIZE` carries the same number, and
/// that module's test reads this file to check the two have not drifted.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 group: SV_GroupID, uint thread: SV_GroupIndex)
{
    if (thread >= WORDS_PER_SOURCE)
    {
        return;
    }

    // Dynamically uniform: every invocation of one workgroup has the same
    // `SV_GroupID`. See the header on why that matters here and nowhere else in
    // this file.
    uint source = group.x;
    // Bounds the *descriptor* index rather than the word index. The array is
    // unbounded in the shader and its length is chosen at bind-group creation,
    // so nothing in the type system stops a dispatch wider than SOURCE_COUNT
    // groups from indexing past the descriptors that group holds — which is
    // undefined behaviour rather than a wrong number, a fault instead of a
    // failed assertion. The intended dispatch never takes this branch.
    if (source >= SOURCE_COUNT)
    {
        return;
    }

    destination[source * WORDS_PER_SOURCE + thread] = sources[source][thread];
}
