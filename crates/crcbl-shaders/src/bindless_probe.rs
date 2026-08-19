//! The workgroup size, table capacity and buffer lengths
//! `bindless_probe.slang` declares — and where each committed artifact puts the
//! descriptor array.
//!
//! Same reason as [`crate::compute_probe`] for the numbers: the shader fixes
//! them, every producer has to agree exactly, and keeping them in the crate that
//! owns the source means one place to change rather than one per consumer.
//!
//! # The array is a `ParameterBlock`, so it is a set of its own
//!
//! **A caller of this shader builds two bind groups, not one.** The array is
//! declared inside a `ParameterBlock`, which Slang gives a descriptor set to
//! itself, and that is the one shape decision here a consumer cannot discover
//! from the constants below. The shader's header says why it has to be a
//! `ParameterBlock` — a bare array compiles to MSL that has no `[[buffer]]`
//! attribute at all — and what the move cost.
//!
//! # Where the array lands, per target
//!
//! This is the part no other module needs, and the reason this one exists: an
//! array of descriptors is a shape each API spells differently, and
//! `crcbl_hal::BindingFlags::VARIABLE_COUNT` is one word standing for all of
//! them. Read out of the committed artifacts rather than assumed:
//!
//! * **SPIR-V** (`spirv/bindless_probe.spv`) — `%sources_buffers` is **set 1**,
//!   binding 0, an `OpTypeArray` of [`SOURCE_CAPACITY`] `StructuredBuffer`
//!   blocks. `%destination` stays at set 0, binding 0. A sized array rather
//!   than an `OpTypeRuntimeArray`, so the module declares no
//!   `RuntimeDescriptorArray` capability: Vulkan's rule for a sized array is
//!   that it be no larger than its binding's `descriptorCount`, and
//!   `VkDescriptorSetVariableDescriptorCountAllocateInfo` still chooses a
//!   smaller length inside that ceiling — which is what
//!   `BindingFlags::VARIABLE_COUNT` asks for.
//! * **DXIL** (`dxil/bindless_probe.computeMain.dxil`) — `sources` is an SRV
//!   range of [`SOURCE_CAPACITY`] descriptors from `t0`, space 0, and
//!   `destination` is `u0`. `crcbl_dx12::binding` plans a range whose
//!   `NumDescriptors` is `u32::MAX` for a `VARIABLE_COUNT` entry, and an
//!   unbounded root-signature range covers this bounded declaration.
//! * **MSL** (`msl/bindless_probe.metal`) — `destination` is `[[buffer(0)]]`
//!   and the block is `[[buffer(1)]]`, an argument buffer whose one member is
//!   `array<uint device*, int(SOURCE_CAPACITY)>`: [`SOURCE_CAPACITY`] device
//!   addresses and nothing else. The order is the ascending `(set, binding)`
//!   order `crcbl-mtl` binds by, which `crate::declaration_order` holds the
//!   source to and the test below reads back out of the artifact.
//!
//! # No WGSL
//!
//! `crcbl-webgpu` has no binding arrays at all and `crcbl-wgpu` has no binding
//! array whose length a group chooses, so neither can reach a dispatch of this
//! shader and a WGSL artifact would be bytes nothing loads.
//!
//! [`SOURCE_CAPACITY`]: crate::bindless_probe::SOURCE_CAPACITY

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/bindless_probe.slang`.
///
/// Wider than [`WORDS_PER_SOURCE`] on purpose, so the shader's bounds check is a
/// branch the intended dispatch really takes — [`crate::push_constant_probe`]'s
/// reason.
pub const WORKGROUP_SIZE: u32 = 64;

/// Descriptors the shader's table declares room for.
///
/// **A capacity, not a length.** This is what a layout declares as its
/// `crcbl_hal::BindGroupLayoutEntry::count` for the array binding: Vulkan
/// requires a sized array to be no larger than its binding's `descriptorCount`,
/// so a layout that declared fewer would describe a shader other than this one.
/// [`SOURCE_COUNT`] is what a group then fills inside it.
///
/// The shader says why it is seven: it has to exceed [`SOURCE_COUNT`] for the
/// ceiling to be a ceiling, and with the destination beside it seven is the
/// most that fits inside `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE`.
pub const SOURCE_CAPACITY: u32 = 7;

/// Descriptors a bind group fills, and workgroups one dispatch submits: each
/// group reads one element.
///
/// This is what a caller passes as
/// `crcbl_hal::BindGroupDesc::variable_count`, and the number of distinct
/// buffers it has to bind — one per `array_index`. Fewer than
/// [`SOURCE_CAPACITY`], which is the whole of what `VARIABLE_COUNT` means.
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

/// The `(set, binding)` of every decorated resource in `words` — a SPIR-V
/// module — in ascending order.
///
/// The instruction stream is stepped through by length rather than scanned for
/// the bare decoration numbers, for [`crate::push_constant_probe::push_constant_variables`]'s
/// reason: a scan would match any operand that happened to equal one, which is
/// a check that cannot fail.
#[cfg(test)]
fn descriptor_slots(words: &[u32]) -> Vec<(u32, u32)> {
    /// `OpDecorate`, whose operands are the target, the decoration and — for
    /// these two — one literal.
    const OP_DECORATE: u32 = 71;
    /// `Decoration Binding`.
    const BINDING: u32 = 33;
    /// `Decoration DescriptorSet`.
    const DESCRIPTOR_SET: u32 = 34;
    /// Words of a SPIR-V module header, before the first instruction.
    const HEADER_WORDS: usize = 5;

    let mut sets: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut bindings: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut cursor = HEADER_WORDS;
    while cursor < words.len() {
        let length = (words[cursor] >> 16) as usize;
        assert!(length > 0, "a zero-length instruction never terminates");
        assert!(
            cursor + length <= words.len(),
            "instruction at word {cursor} runs past the end of the module"
        );
        if words[cursor] & 0xffff == OP_DECORATE && length == 4 {
            let (target, literal) = (words[cursor + 1], words[cursor + 3]);
            match words[cursor + 2] {
                DESCRIPTOR_SET => {
                    sets.insert(target, literal);
                }
                BINDING => {
                    bindings.insert(target, literal);
                }
                _ => {}
            }
        }
        cursor += length;
    }

    let mut slots: Vec<(u32, u32)> = sets
        .into_iter()
        .map(|(target, set)| {
            let binding = bindings.get(&target).copied().unwrap_or_else(|| {
                panic!("%{target} carries a DescriptorSet decoration and no Binding")
            });
            (set, binding)
        })
        .collect();
    slots.sort_unstable();
    slots
}

/// The parameter of the MSL kernel's signature that carries `[[buffer(index)]]`.
///
/// Test-only, and it exists because the *order* of Metal's buffer table is the
/// one thing about this artifact a reader cannot check by eye without knowing
/// Slang's mangling: `crcbl-mtl` binds by ascending `(set, binding)`, Slang's
/// Metal target numbers in declaration order, and a `register` annotation on
/// the block silently swaps the two — measured on this very source.
#[cfg(test)]
fn msl_buffer_parameter(msl: &str, index: u32) -> &str {
    let body = msl
        .split_once("void computeMain(")
        .expect("the MSL declares a computeMain kernel")
        .1;
    // Nesting is counted rather than the first `)` taken: every parameter that
    // matters here ends in a `[[buffer(n)]]` attribute, whose own parentheses
    // close before the list does.
    let mut depth = 1_usize;
    let end = body
        .char_indices()
        .find(|(_, character)| {
            match character {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth == 0
        })
        .expect("the kernel's parameter list closes")
        .0;
    let signature = &body[..end];
    let attribute = format!("[[buffer({index})]]");
    signature
        .split(',')
        .find(|parameter| parameter.contains(&attribute))
        .unwrap_or_else(|| panic!("no kernel parameter carries {attribute}"))
        .trim()
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
            format!("static const uint SOURCE_CAPACITY = {SOURCE_CAPACITY};"),
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

    /// The shader must declare the array as a **bounded** `ParameterBlock` in
    /// set 1, and must carry no `register` annotation.
    ///
    /// Each clause is one line to check and each has a measured artifact
    /// consequence. Without the `ParameterBlock` the MSL has no `[[buffer]]`
    /// attribute on the block at all; unbounded inside one, Metal's front end
    /// refuses the emitted source as a C99 flexible array member; and a
    /// `register` annotation put back on the block swaps Metal's two buffer
    /// indices, which is the failure `crate::declaration_order` exists to stop
    /// and which no lint over `[[vk::binding]]` can see.
    #[test]
    fn the_source_declares_a_bounded_parameter_block_in_its_own_set() {
        let source = include_str!("../shaders/bindless_probe.slang");
        for declaration in [
            "    StructuredBuffer<uint> buffers[SOURCE_CAPACITY];".to_string(),
            "[[vk::binding(0, 1)]]\nParameterBlock<Sources> sources;\n".to_string(),
        ] {
            assert!(
                source.contains(&declaration),
                "bindless_probe.slang no longer declares `{declaration}`; its array is not the \
                 bounded ParameterBlock every artifact below was built from"
            );
        }
        // The declaration above already ends in `;`, so this only adds a
        // message. The header discusses `: register(…)` at length, which is why
        // the check names the declaration rather than scanning the file.
        assert!(
            !source.contains("ParameterBlock<Sources> sources :"),
            "bindless_probe.slang has a `register` annotation on the block again; on this source \
             that moves the block to Metal's [[buffer(0)]] and the destination to [[buffer(1)]], \
             which is the reverse of the order crcbl-mtl binds by"
        );
        assert!(
            source.contains("// crcbl-targets: spirv, msl, dxil\n"),
            "bindless_probe.slang's target declaration has changed; crcbl-webgpu has no binding \
             arrays, so a WGSL artifact would be bytes nothing loads"
        );
    }

    /// The committed artifacts put the two resources where the docs above say
    /// — the fact every backend's bindless path is written against, and the one
    /// the `ParameterBlock` recast changed.
    ///
    /// Read out of the artifacts rather than restated: a Slang release that
    /// stopped giving the block a set of its own would leave the prose, the
    /// seam's two bind groups and `crcbl-mtl`'s argument-buffer index all
    /// describing a module that no longer agrees with any of them.
    ///
    /// The DXIL register and space are the one pair not asserted here, for
    /// [`crate::push_constant_probe`]'s reason: they live in the container's
    /// `RDEF` chunk and this crate has no dependencies to parse DXBC with.
    /// `crcbl-dx12`'s `dxil` module does parse it and asserts both, and the
    /// byte-for-byte recompile gate is what keeps the container the one this
    /// source produces.
    #[test]
    fn the_artifacts_put_the_table_in_a_set_of_its_own() {
        assert_eq!(
            descriptor_slots(crate::BINDLESS_PROBE.spirv()),
            [(0, 0), (1, 0)],
            "the SPIR-V no longer puts the destination at set 0 binding 0 and the descriptor \
             table at set 1 binding 0, so a caller building two bind groups is building the \
             wrong two"
        );
        // The same walk over the compute shader whose three bindings share one
        // set. A walk that read the wrong words — or that reported one set per
        // resource whatever the module said — would answer `[(0, 0), (1, 0)]`
        // here too and the assertion above would mean nothing.
        assert_eq!(
            descriptor_slots(crate::COMPUTE_PROBE.spirv()),
            [(0, 0), (0, 1), (0, 2)],
            "compute_probe.slang declares three scalar bindings in one set, so a walk that finds \
             anything else is reading something other than the decorations"
        );

        assert!(
            crate::BINDLESS_PROBE.dxil("computeMain").is_some(),
            "the shader no longer ships the DXIL container the docs above describe"
        );
        assert!(
            crate::BINDLESS_PROBE.wgsl().is_none(),
            "a WGSL artifact has appeared for a shader crcbl-webgpu cannot dispatch"
        );

        let msl = crate::BINDLESS_PROBE
            .msl()
            .expect("the shader ships the MSL its target line declares");
        assert!(
            msl.contains(&format!("array<uint device*, int({SOURCE_CAPACITY})>")),
            "the MSL's argument buffer is no longer {SOURCE_CAPACITY} device addresses; Metal \
             refuses an unbounded member outright, so this is what the length being a capacity \
             looks like in the artifact"
        );
        // Both indices, and by name: `crcbl-mtl` binds in ascending (set,
        // binding) order, so the destination must be the *first* buffer and the
        // table the second. Asserting only that each attribute appears would
        // pass with the two swapped, which is the exact regression a `register`
        // annotation reintroduces.
        assert!(
            msl_buffer_parameter(msl, 0).contains("destination"),
            "the MSL kernel's [[buffer(0)]] is `{}` rather than the destination",
            msl_buffer_parameter(msl, 0)
        );
        assert!(
            msl_buffer_parameter(msl, 1).contains("sources"),
            "the MSL kernel's [[buffer(1)]] is `{}` rather than the descriptor table",
            msl_buffer_parameter(msl, 1)
        );
    }
}
