//! The workgroup size, array length and buffer lengths
//! `bindless_probe.slang` declares — and where each committed artifact puts the
//! descriptor array.
//!
//! Same reason as [`crate::compute_probe`] for the numbers: the shader fixes
//! them, every producer has to agree exactly, and keeping them in the crate that
//! owns the source means one place to change rather than one per consumer.
//!
//! # Where the array lands, per target
//!
//! This is the part no other module needs, and the reason this one exists: an
//! array of descriptors is a shape each API spells differently, and
//! `crcbl_hal::BindingFlags::VARIABLE_COUNT` is one word standing for all of
//! them. Read out of the committed artifacts rather than assumed:
//!
//! * **SPIR-V** (`spirv/bindless_probe.spv`) — `%sources` is set 0, binding 1,
//!   an `OpTypeRuntimeArray` of the `StructuredBuffer` block, under
//!   `OpCapability RuntimeDescriptorArray`. That capability is the artifact's
//!   own statement that its array has no compile-time length, and it is the
//!   thing a Vulkan device must report `runtimeDescriptorArray` for.
//!   `%destination` is set 0, binding 0.
//! * **DXIL** (`dxil/bindless_probe.computeMain.dxil`) — `sources` is an
//!   **unbounded** SRV range at `t0`, space 0, and `destination` is `u0`. A
//!   D3D12 root signature therefore carries a descriptor range whose
//!   `NumDescriptors` is `u32::MAX`, which is how the API spells unbounded —
//!   see `crcbl_dx12::binding`, which plans exactly that for a `VARIABLE_COUNT`
//!   entry.
//!
//! The space is the one thing this source has to state by hand, and the shader
//! says why at the declaration: Slang puts an unbounded array in a space of its
//! own unless a `register` annotation pins it, and `crcbl-dx12` builds every
//! range in space 0.
//!
//! # No MSL and no WGSL
//!
//! `crcbl-mtl` binds flat argument tables and refuses every `BindingFlags` at
//! `create_bind_group_layout`; `crcbl-webgpu` has no binding arrays at all.
//! Neither can reach a dispatch of this shader, so those two artifacts would be
//! bytes nothing loads.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/bindless_probe.slang`.
///
/// Wider than [`WORDS_PER_SOURCE`] on purpose, so the shader's bounds check is a
/// branch the intended dispatch really takes — [`crate::push_constant_probe`]'s
/// reason.
pub const WORKGROUP_SIZE: u32 = 64;

/// Descriptors in the array, and workgroups one dispatch submits: each group
/// reads one element.
///
/// This is what a caller passes as
/// `crcbl_hal::BindGroupDesc::variable_count`, and the number of distinct
/// buffers it has to bind — one per `array_index`.
pub const SOURCE_COUNT: u32 = 4;

/// Words each source buffer holds, and words each workgroup copies out of it.
pub const WORDS_PER_SOURCE: u32 = 4;

/// Words the destination holds: one block of [`WORDS_PER_SOURCE`] per
/// descriptor.
///
/// Source `i`'s word `w` lands at `i * WORDS_PER_SOURCE + w`, so a readback
/// compared against the words the caller wrote names *which* descriptor was read
/// wrongly rather than only that one was.
pub const DESTINATION_WORDS: u32 = SOURCE_COUNT * WORDS_PER_SOURCE;

/// Bytes one source buffer occupies.
pub const SOURCE_BYTES: u64 = WORDS_PER_SOURCE as u64 * size_of::<u32>() as u64;

/// Bytes the destination occupies.
pub const DESTINATION_BYTES: u64 = DESTINATION_WORDS as u64 * size_of::<u32>() as u64;

/// Whether `words` — a SPIR-V module — declares
/// `OpCapability RuntimeDescriptorArray`.
///
/// The instruction stream is stepped through by length rather than scanned for
/// the bare capability number, for [`crate::push_constant_probe::push_constant_variables`]'s
/// reason: a scan would match any operand that happened to equal it, which is a
/// check that cannot fail.
#[cfg(test)]
fn declares_runtime_descriptor_array(words: &[u32]) -> bool {
    /// `OpCapability`, whose single operand is the capability.
    const OP_CAPABILITY: u32 = 17;
    /// `Capability RuntimeDescriptorArray`.
    const RUNTIME_DESCRIPTOR_ARRAY: u32 = 5302;
    /// Words of a SPIR-V module header, before the first instruction.
    const HEADER_WORDS: usize = 5;

    let mut cursor = HEADER_WORDS;
    while cursor < words.len() {
        let length = (words[cursor] >> 16) as usize;
        assert!(length > 0, "a zero-length instruction never terminates");
        assert!(
            cursor + length <= words.len(),
            "instruction at word {cursor} runs past the end of the module"
        );
        if words[cursor] & 0xffff == OP_CAPABILITY
            && length == 2
            && words[cursor + 1] == RUNTIME_DESCRIPTOR_ARRAY
        {
            return true;
        }
        cursor += length;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size, for the
    /// reason [`crate::compute_probe`]'s twin of this test gives: a mismatch
    /// compiles, dispatches, and shows up only as a partially written buffer.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_the_source_declares() {
        let source = include_str!("../shaders/bindless_probe.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "bindless_probe.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted \
             from the shader"
        );
    }

    /// Both lengths are the shader's own bounds, and the shader writes past its
    /// destination — or reads past the descriptors the bind group holds — if
    /// either disagrees.
    #[test]
    fn the_lengths_match_the_bounds_the_source_declares() {
        let source = include_str!("../shaders/bindless_probe.slang");
        for declaration in [
            format!("static const uint SOURCE_COUNT = {SOURCE_COUNT};"),
            format!("static const uint WORDS_PER_SOURCE = {WORDS_PER_SOURCE};"),
        ] {
            assert!(
                source.contains(&declaration),
                "bindless_probe.slang does not declare `{declaration}`; this module has drifted \
                 from the shader"
            );
        }
    }

    /// The shader must declare an **unbounded** array and pin its HLSL register
    /// space, or the artifacts this module documents are a different shape.
    ///
    /// Both halves are one line to check and each has an artifact consequence:
    /// a fixed bound is an ordinary descriptor array rather than a runtime-sized
    /// one, and a missing `register` annotation moves the DXIL range into
    /// `space1`, which no root signature `crcbl-dx12` builds names.
    #[test]
    fn the_source_declares_an_unbounded_array_in_space_zero() {
        let source = include_str!("../shaders/bindless_probe.slang");
        assert!(
            source.contains("StructuredBuffer<uint> sources[] : register(t0, space0);"),
            "bindless_probe.slang no longer declares `sources` as an unbounded array pinned to \
             space 0"
        );
        assert!(
            source.contains("// crcbl-targets: spirv, dxil\n"),
            "bindless_probe.slang's target declaration has changed; crcbl-mtl and crcbl-webgpu \
             both refuse this capability, so an MSL or WGSL artifact would be bytes nothing loads"
        );
    }

    /// The committed artifacts really do declare the array the docs above
    /// describe — the fact every backend's bindless path is written against.
    ///
    /// Read out of the artifact rather than restated, because a Slang release
    /// that stopped emitting the capability would leave the prose describing a
    /// module that no longer says it. The DXIL register and space are the one
    /// pair not asserted here, for [`crate::push_constant_probe`]'s reason:
    /// they live in the container's `RDEF` chunk and this crate has no
    /// dependencies to parse DXBC with. `crcbl-dx12`'s `dxil` module does parse
    /// it and asserts both, and the byte-for-byte recompile gate is what keeps
    /// the container the one this source produces.
    #[test]
    fn the_spirv_declares_a_runtime_descriptor_array() {
        assert!(
            declares_runtime_descriptor_array(crate::BINDLESS_PROBE.spirv()),
            "the SPIR-V no longer declares OpCapability RuntimeDescriptorArray, which is the \
             artifact's own statement that its descriptor array has no compile-time length"
        );
        // The same walk over the compute shader whose three bindings are all
        // scalars. A walk that matched the capability number anywhere in the
        // stream — or that returned true for every module — would report one
        // here too and the assertion above would mean nothing.
        assert!(
            !declares_runtime_descriptor_array(crate::COMPUTE_PROBE.spirv()),
            "compute_probe.slang declares three scalar bindings and no array, so its SPIR-V has \
             no RuntimeDescriptorArray capability — a walk that finds one is matching something \
             else"
        );

        assert!(
            crate::BINDLESS_PROBE.dxil("computeMain").is_some(),
            "the shader no longer ships the DXIL container the docs above describe"
        );
        assert!(
            crate::BINDLESS_PROBE.msl().is_none() && crate::BINDLESS_PROBE.wgsl().is_none(),
            "an MSL or WGSL artifact has appeared for a shader neither crcbl-mtl nor \
             crcbl-webgpu can dispatch"
        );
    }
}
