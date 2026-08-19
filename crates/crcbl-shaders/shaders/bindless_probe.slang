// crcbl-targets: spirv, msl, dxil

// The only shader here that declares an array of descriptors.
//
// `crcbl-hal` carries `BindingFlags::VARIABLE_COUNT`, `BindGroupDesc::
// variable_count`, `BindGroupEntry::array_index` and
// `Capability::BindlessDescriptorArray`, and until this file no committed
// artifact declared a resource array at all: every `Texture2DArray` in this
// directory is one layered image rather than an array of descriptors, and no
// WGSL contains `binding_array`. So each backend's bindless path — Vulkan's
// `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT`, a D3D12 descriptor
// range, Metal's argument buffer — had no shader to be proved against, and
// `Capability::BindlessDescriptorArray` was the one binding-model capability
// the agnostic seam suite could not drive.
//
// # An array of buffers, not of textures
//
// `sources` is an array of read-only storage buffers. A texture array would
// make the readback depend on a sampler, a view type, a texel format and a
// filtering rule, none of which this capability is about — every one of those
// is a way for the test to fail while the descriptor array worked. Buffers have
// none: a word read out of `sources.buffers[i]` is the word the caller wrote
// into the buffer it bound at element `i`, exactly.
//
// # The array is a `ParameterBlock`, and it is bounded
//
// Both halves are Metal's, and both were measured rather than assumed against
// Slang 2026.14:
//
// * Declared as a bare `StructuredBuffer<uint> sources[]`, `slangc -target
//   metal` **exits 0** and emits a binding index of `18446744073709551615`
//   ((uint64)-1) on an entry-point parameter carrying no `[[buffer]]` attribute
//   at all. Unusable, and silently so — which is why this file's MSL is read
//   rather than its compiler's exit code believed.
// * Bounding the array without wrapping it hands Metal the same unattributed
//   parameter. So the `ParameterBlock` is what produces a `[[buffer(n)]]`, and
//   it is not optional.
// * Left **unbounded** inside the `ParameterBlock`, Metal's own front end
//   refuses the emitted MSL: `error: flexible array members are a C99 feature`.
//   So the length has to be a compile-time constant.
//
// A bounded array inside a `ParameterBlock` lowers to `array<uint device*,
// int(SOURCE_CAPACITY)>` — an ordinary `metal::array` of device addresses — and
// a kernel reading through it returns the right words on real hardware, which
// `crcbl_mtl::binding`'s probe measured on an Apple Paravirtual device before
// this file was recast.
//
// **The cost is a descriptor set, and it is the shape every consumer has to
// carry.** A `ParameterBlock` is a set of its own: `destination` stays at set 0
// binding 0 and `sources` moves to **set 1** binding 0, so a caller builds two
// bind groups where it used to build one. That is the arrangement an engine
// wants anyway — a descriptor table is bound once per frame while the sets
// beside it change per draw — but it is a change of shape rather than of
// spelling, and `crcbl_shaders::bindless_probe` records it per target.
//
// **What it also costs is the runtime array.** The SPIR-V now declares an
// `OpTypeArray` of `SOURCE_CAPACITY` rather than an `OpTypeRuntimeArray` under
// `OpCapability RuntimeDescriptorArray`, and the DXIL range is bounded rather
// than open-ended. Neither is what `BindingFlags::VARIABLE_COUNT` needs: a
// Vulkan binding's `descriptorCount` is the *upper bound* a set is allocated
// under, `VkDescriptorSetVariableDescriptorCountAllocateInfo` chooses the
// smaller length, and the Vulkan interface rule for a sized array is only that
// "the size of the array must be no larger than the number of descriptors in
// the binding". A capacity the layout declares room for and a length the group
// chooses inside it is exactly that.
//
// # THERE IS NO `register` ANNOTATION, AND THERE MUST NOT BE ONE
//
// The unbounded form needed `: register(t0, space0)`, because Slang moves an
// unbounded array into a register space of its own and `crcbl-dx12` builds
// every descriptor range with `RegisterSpace: 0`. A bounded array is already in
// space 0 — measured with `-target hlsl`, which emits `sources_buffers_0[int(7)]
// : register(t0)` — so the annotation is now unnecessary, and it is worse than
// unnecessary: adding it back **swaps Metal's buffer indices**. With it,
// `slangc -target metal` puts `sources` at `[[buffer(0)]]` and `destination` at
// `[[buffer(1)]]`, which is the reverse of the ascending-(set, binding) order
// `crcbl-mtl` binds by and the exact failure `crcbl_shaders`' `declaration_order`
// lint exists to stop. Measured, on this pair, with 2026.14.
//
// `crcbl_dx12::dxil`'s `resource_table` asserts space 0 over every committed
// container, so a Slang release that moved this one is caught there rather than
// pinned here.
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
// # No WGSL
//
// `crcbl-webgpu` refuses this capability: WebGPU has no binding arrays at all,
// and `crcbl-wgpu` refuses too because a wgpu binding array's length is its
// layout's fixed count rather than something a group chooses. Neither backend
// can reach a dispatch of this shader, so a WGSL artifact would be bytes
// nothing loads. `push_constant_raster.slang` omits the same target for an
// unrelated reason — WGSL has no push constants — so the two declarations read
// alike and are not the same rule.

/// Descriptors the `sources` block declares room for.
///
/// Matches `crcbl_shaders::bindless_probe::SOURCE_CAPACITY`. This is a
/// **capacity**, not a length: the bind group binds [`SOURCE_COUNT`] of them and
/// the layout above it declares this many, which is what
/// `BindingFlags::VARIABLE_COUNT` means.
///
/// Seven, and the number is squeezed between two constraints rather than picked:
///
/// * It must exceed `SOURCE_COUNT`, or the ceiling is not a ceiling and a
///   backend that ignored `BindGroupDesc::variable_count` and sized the array
///   from its layout would pass the seam's exercise anyway.
/// * Vulkan requires a sized array to be no larger than its binding's
///   `descriptorCount`, so the layout has to declare all seven — and
///   `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE` is 8 storage buffers in
///   one stage, summed across every set of a pipeline layout. With
///   `destination` beside it, seven is the largest capacity that fits.
///
/// So the probe binds four descriptors into a table declared for seven, and
/// nothing above it can grow without the portability check refusing the layout
/// by name.
static const uint SOURCE_CAPACITY = 7;

/// Descriptors the bind group fills — and workgroups one dispatch submits, since
/// each group reads one element.
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

/// The written output, at set 0 binding 0 — declared first because the block
/// below it is set 1, and `crcbl_shaders`' `declaration_order` lint requires
/// ascending `(set, binding)` in declaration order so that Slang's Metal target,
/// which numbers its argument table in declaration order and ignores
/// `[[vk::binding]]`, agrees with the index `crcbl-mtl` computes.
[[vk::binding(0, 0)]]
RWStructuredBuffer<uint> destination;

/// The descriptor table's contents: [`SOURCE_CAPACITY`] read-only storage
/// buffers, of which a bind group fills the first [`SOURCE_COUNT`].
///
/// A `struct` because `ParameterBlock` takes one, and one member because that
/// member is the whole point — an argument buffer of nothing but device
/// addresses is the shape `crcbl_mtl::binding`'s probe measured.
struct Sources
{
    StructuredBuffer<uint> buffers[SOURCE_CAPACITY];
};

/// The descriptor table, in a descriptor set of its own.
///
/// `ParameterBlock` rather than a bare array is what makes the MSL loadable at
/// all, and it is what moves this to set 1; the header says why both, and what
/// the move costs a caller. `[[vk::binding(0, 1)]]` states the set the block
/// lands in rather than leaving it to be read out of the artifact — Slang picks
/// the same set either way, measured, and the attribute is also what the
/// `declaration_order` lint sorts on.
[[vk::binding(0, 1)]]
ParameterBlock<Sources> sources;

/// One workgroup per bound descriptor, copying that element's words into the
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
    // Bounds the *descriptor* index against the length the bind group chose,
    // which is `SOURCE_COUNT` and not `SOURCE_CAPACITY`. The elements between
    // the two are declared and never written: reading one is undefined
    // behaviour rather than a wrong number — a fault on Vulkan, a null device
    // address on Metal. The intended dispatch never takes this branch.
    if (source >= SOURCE_COUNT)
    {
        return;
    }

    destination[source * WORDS_PER_SOURCE + thread] = sources.buffers[source][thread];
}
