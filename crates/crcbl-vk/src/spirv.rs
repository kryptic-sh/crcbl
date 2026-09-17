//! Just enough SPIR-V parsing to answer "does this module have that entry
//! point, at which stage, and — for a compute one — with how many threads per
//! workgroup?".
//!
//! # Why parse at all
//!
//! `vkCreateShaderModule` accepts any well-formed module, and
//! `vkCreateGraphicsPipelines` is where a wrong `pName` is discovered. What
//! comes back from a driver at that point is
//! `VK_ERROR_INITIALIZATION_FAILED` — or, on a driver in a worse mood, a
//! validation message about a stage that "does not contain the specified entry
//! point", by which time the caller has a pipeline error naming neither the
//! module nor the name it asked for. `crcbl-hal` promises
//! [`HalError::ShaderCompilation`](crcbl_hal::HalError::ShaderCompilation) with
//! a reason, and a reason is only possible if this layer knows what the module
//! contains.
//!
//! It also catches the mistake the seam explicitly worries about — bytes passed
//! where words were wanted — one call earlier than the driver would, and with a
//! message that says so.
//!
//! The workgroup size is read for a different reason:
//! [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
//! carries the number Metal needs at its dispatch call, and this backend is
//! compiling the module that already declares it — so a descriptor that
//! disagrees with `[numthreads(…)]` fails here rather than launching the wrong
//! number of threads on the one backend that cannot check.
//! See [`require_workgroup_size`].
//!
//! # Deliberately shallow
//!
//! This reads the header and the `OpEntryPoint` instructions and stops. It does
//! **not** reflect descriptor bindings: the seam takes an explicit
//! [`BindGroupLayoutDesc`](crcbl_hal::BindGroupLayoutDesc), so a reflected
//! layout would be a second source of truth that could disagree with the
//! declared one. Full reflection belongs with the material system's permutation
//! manager (`docs/plan/37-materials.md`), where the declared parameter block is
//! the thing being checked against.
//!
//! Every function here is pure and has no `unsafe`, so the interesting half is
//! tested without a driver in the room.

use crcbl_hal::ShaderStages;

/// The SPIR-V magic number, as the first word of any valid module.
pub const MAGIC: u32 = 0x0723_0203;

/// The magic number as it reads when the words were byte-swapped — the
/// signature of a big-endian module, or of bytes reinterpreted the wrong way.
const MAGIC_REVERSED: u32 = 0x0302_2307;

/// `OpEntryPoint`, from the SPIR-V core grammar.
const OP_ENTRY_POINT: u16 = 15;

/// `OpExecutionMode`, from the SPIR-V core grammar. Its `OpExecutionModeId`
/// sibling (331) is deliberately not read; see [`workgroup_size`].
const OP_EXECUTION_MODE: u16 = 16;

/// The `LocalSize` execution mode, whose three literal operands are what
/// `[numthreads(x, y, z)]` compiles to.
const EXECUTION_MODE_LOCAL_SIZE: u32 = 17;

/// One `OpEntryPoint` in a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPoint {
    /// The name, exactly as the module spells it. Case-sensitive, because
    /// `pName` is matched byte-for-byte.
    pub name: String,
    /// The stage, if the seam has a bit for it. `None` for the execution models
    /// this engine does not use — tessellation, geometry, ray tracing — which
    /// are reported rather than dropped so an error can say *why* an entry
    /// point was rejected.
    pub stage: Option<ShaderStages>,
}

/// Hands every instruction's opcode, word offset and operands to `visit`.
///
/// The header check and the step-by-word-count walk are the same in both
/// readers below, and getting the step wrong is the classic way to read an
/// operand as an opcode — so it exists once.
///
/// # Errors
///
/// A `String` explaining what is wrong with the module: too short, not SPIR-V,
/// byte-swapped, or an instruction whose word count would run past the end.
/// Also whatever `visit` returns, unchanged.
fn walk(
    words: &[u32],
    mut visit: impl FnMut(u16, usize, &[u32]) -> Result<(), String>,
) -> Result<(), String> {
    // The header is magic, version, generator, bound, schema.
    if words.len() < 5 {
        return Err(format!(
            "a SPIR-V module has a five-word header; this is {} word(s)",
            words.len()
        ));
    }
    match words[0] {
        MAGIC => {}
        MAGIC_REVERSED => {
            return Err(
                "the module is byte-swapped: the first word reads 0x03022307, which is the \
                 SPIR-V magic with the bytes reversed. The seam takes native-endian words, so \
                 this is usually `&[u8]` reinterpreted rather than decoded."
                    .to_string(),
            );
        }
        other => {
            return Err(format!(
                "not SPIR-V: first word is {other:#010x}, expected {MAGIC:#010x}"
            ));
        }
    }

    let mut index = 5;
    while index < words.len() {
        let instruction = words[index];
        #[allow(clippy::cast_possible_truncation)]
        let opcode = (instruction & 0xffff) as u16;
        let word_count = (instruction >> 16) as usize;
        if word_count == 0 {
            return Err(format!(
                "instruction at word {index} has a zero word count, which would not terminate"
            ));
        }
        let end = index
            .checked_add(word_count)
            .ok_or_else(|| format!("instruction at word {index} overflows the module"))?;
        if end > words.len() {
            return Err(format!(
                "instruction at word {index} claims {word_count} words but only {} remain",
                words.len() - index
            ));
        }
        visit(opcode, index, &words[index + 1..end])?;
        index = end;
    }
    Ok(())
}

/// Reads the header and every `OpEntryPoint`.
///
/// # Errors
///
/// As [`walk`], plus an `OpEntryPoint` too short to hold a name or whose name
/// never terminates.
pub fn entry_points(words: &[u32]) -> Result<Vec<EntryPoint>, String> {
    let mut found = Vec::new();
    walk(words, |opcode, index, operands| {
        if opcode != OP_ENTRY_POINT {
            return Ok(());
        }
        // OpEntryPoint: execution model, entry point id, then the name as a
        // null-terminated literal string, then the interface ids.
        if operands.len() < 3 {
            return Err(format!(
                "OpEntryPoint at word {index} is {} words; the shortest legal one is 4",
                operands.len() + 1
            ));
        }
        let name = literal_string(&operands[2..])
            .ok_or_else(|| format!("OpEntryPoint at word {index} has an unterminated name"))?;
        found.push(EntryPoint {
            name,
            stage: stage_of(operands[0]),
        });
        Ok(())
    })?;
    Ok(found)
}

/// The `LocalSize` an entry point declares, if it declares one that way.
///
/// `Ok(None)` means the module has that entry point and gives its workgroup
/// size some *other* way — `OpExecutionModeId LocalSizeId`, or the deprecated
/// `WorkgroupSize` builtin decoration — both of which name specialisation
/// constants rather than literals and so cannot be answered without evaluating
/// them. Nothing `crcbl-shaders` emits does that: `[numthreads(…)]` through
/// Slang is always the literal form.
///
/// # Errors
///
/// As [`entry_points`], plus a module with no entry point of that name.
pub fn workgroup_size(words: &[u32], name: &str) -> Result<Option<[u32; 3]>, String> {
    let mut entry_id: Option<u32> = None;
    let mut declared: Vec<(u32, [u32; 3])> = Vec::new();
    walk(words, |opcode, index, operands| {
        match opcode {
            OP_ENTRY_POINT => {
                if operands.len() < 3 {
                    return Err(format!(
                        "OpEntryPoint at word {index} is {} words; the shortest legal one is 4",
                        operands.len() + 1
                    ));
                }
                let found = literal_string(&operands[2..]).ok_or_else(|| {
                    format!("OpEntryPoint at word {index} has an unterminated name")
                })?;
                if found == name {
                    entry_id = Some(operands[1]);
                }
            }
            // OpExecutionMode: entry point id, mode, then the mode's own
            // literals — three of them for LocalSize, which is why a shorter
            // instruction is some other mode rather than a malformed one.
            OP_EXECUTION_MODE
                if operands.len() >= 5 && operands[1] == EXECUTION_MODE_LOCAL_SIZE =>
            {
                declared.push((operands[0], [operands[2], operands[3], operands[4]]));
            }
            _ => {}
        }
        Ok(())
    })?;
    let entry_id =
        entry_id.ok_or_else(|| format!("the module has no entry point named {name:?}"))?;
    Ok(declared
        .into_iter()
        .find(|(target, _)| *target == entry_id)
        .map(|(_, size)| size))
}

/// Whether the module agrees with the workgroup size a
/// [`ComputePipelineDesc`](crcbl_hal::ComputePipelineDesc) declares.
///
/// **This is the guard that keeps that field from being a second, independent
/// number.** Metal takes its threads-per-threadgroup at the dispatch call and
/// has no way to check it, so a descriptor whose size disagrees with
/// `[numthreads(…)]` runs the wrong number of threads *there* and nowhere else
/// — a bug that reproduces on exactly one backend. Vulkan is compiling the very
/// module that declares the real size, so it is the place that can say so.
///
/// # Errors
///
/// A `String` naming both sizes on a mismatch, and saying which form the module
/// used when it declares no literal `LocalSize` at all.
pub fn require_workgroup_size(words: &[u32], name: &str, declared: [u32; 3]) -> Result<(), String> {
    match workgroup_size(words, name)? {
        Some(size) if size == declared => Ok(()),
        Some(size) => Err(format!(
            "the descriptor declares a workgroup size of {declared:?} for {name:?}, but the \
             module declares [numthreads({}, {}, {})]. Take the value from the `WORKGROUP_SIZE` \
             beside the shader in `crcbl-shaders` rather than writing it twice.",
            size[0], size[1], size[2]
        )),
        None => Err(format!(
            "the module gives no literal LocalSize for {name:?}, so its workgroup size comes \
             from LocalSizeId or the WorkgroupSize builtin — specialisation constants this \
             engine's shaders do not use and this backend cannot evaluate"
        )),
    }
}

/// Whether `words` contains `name` at `stage`, and why not if it does not.
///
/// # Errors
///
/// A `String` naming the entry points the module *does* have, because the
/// overwhelmingly likely cause is a typo or a stage mix-up and the fix is
/// visible in the list.
pub fn require_entry_point(words: &[u32], name: &str, stage: ShaderStages) -> Result<(), String> {
    let found = entry_points(words)?;
    if found
        .iter()
        .any(|entry| entry.name == name && entry.stage == Some(stage))
    {
        return Ok(());
    }
    let available: Vec<String> = found
        .iter()
        .map(|entry| match entry.stage {
            Some(stage) => format!("{:?} {:?}", stage, entry.name),
            None => format!("(unsupported stage) {:?}", entry.name),
        })
        .collect();
    if found.iter().any(|entry| entry.name == name) {
        return Err(format!(
            "the module has an entry point named {name:?}, but not at {stage:?}; it has \
             [{}]",
            available.join(", ")
        ));
    }
    Err(format!(
        "the module has no {stage:?} entry point named {name:?}; it has [{}]",
        available.join(", ")
    ))
}

/// `OpTypeArray`, `OpTypeRuntimeArray`, `OpTypeStruct` and `OpTypePointer`.
const OP_TYPE_ARRAY: u16 = 28;
const OP_TYPE_RUNTIME_ARRAY: u16 = 29;
const OP_TYPE_STRUCT: u16 = 30;
const OP_TYPE_POINTER: u16 = 32;
/// `OpVariable` and `OpDecorate`.
const OP_VARIABLE: u16 = 59;
const OP_DECORATE: u16 = 71;

/// Decorations the storage-buffer reader needs.
const DECORATION_BUFFER_BLOCK: u32 = 3;
const DECORATION_ARRAY_STRIDE: u32 = 6;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;

/// Storage classes a storage buffer can live in: `StorageBuffer`, and the
/// SPIR-V 1.0 spelling — `Uniform` holding a `BufferBlock` struct.
const STORAGE_CLASS_UNIFORM: u32 = 2;
const STORAGE_CLASS_STORAGE_BUFFER: u32 = 12;

/// One storage buffer a module declares: where it binds, and its element
/// stride.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageBuffer {
    /// `DescriptorSet`.
    pub set: u32,
    /// `Binding`.
    pub binding: u32,
    /// `ArrayStride` on the runtime array the block's last member is.
    pub stride: u32,
}

/// Every storage buffer the module declares whose block ends in a runtime array
/// — which is every `StructuredBuffer<T>` Slang emits — with that array's
/// stride.
///
/// A descriptor array of such buffers (`StructuredBuffer<T>[N]` or unbounded)
/// reports the element buffer's stride once, at its binding.
///
/// Reflection is deliberately confined to this one fact; see the module docs
/// for why this layer does not reflect layouts. It exists so
/// [`require_storage_strides`] can hold a declared
/// [`BindingKind::StorageBuffer`](crcbl_hal::BindingKind::StorageBuffer)
/// stride to the module, exactly as [`require_workgroup_size`] holds a declared
/// workgroup size.
///
/// # Errors
///
/// As [`walk`], plus an instruction too short for its opcode's operands.
pub fn storage_buffers(words: &[u32]) -> Result<Vec<StorageBuffer>, String> {
    use std::collections::{HashMap, HashSet};

    let mut sets = HashMap::new();
    let mut bindings = HashMap::new();
    let mut strides = HashMap::new();
    let mut buffer_blocks = HashSet::new();
    let mut arrays = HashMap::new();
    let mut runtime_arrays = HashSet::new();
    let mut structs: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut pointers = HashMap::new();
    let mut variables = Vec::new();
    walk(words, |opcode, index, operands| {
        let short = || format!("instruction {opcode} at word {index} is too short");
        match opcode {
            OP_DECORATE => {
                let (&target, &decoration) = (
                    operands.first().ok_or_else(short)?,
                    operands.get(1).ok_or_else(short)?,
                );
                let literal = operands.get(2).copied();
                match decoration {
                    DECORATION_DESCRIPTOR_SET => {
                        sets.insert(target, literal.ok_or_else(short)?);
                    }
                    DECORATION_BINDING => {
                        bindings.insert(target, literal.ok_or_else(short)?);
                    }
                    DECORATION_ARRAY_STRIDE => {
                        strides.insert(target, literal.ok_or_else(short)?);
                    }
                    DECORATION_BUFFER_BLOCK => {
                        buffer_blocks.insert(target);
                    }
                    _ => {}
                }
            }
            OP_TYPE_ARRAY => {
                arrays.insert(
                    *operands.first().ok_or_else(short)?,
                    *operands.get(1).ok_or_else(short)?,
                );
            }
            OP_TYPE_RUNTIME_ARRAY => {
                let result = *operands.first().ok_or_else(short)?;
                arrays.insert(result, *operands.get(1).ok_or_else(short)?);
                runtime_arrays.insert(result);
            }
            OP_TYPE_STRUCT => {
                structs.insert(*operands.first().ok_or_else(short)?, operands[1..].to_vec());
            }
            OP_TYPE_POINTER => {
                pointers.insert(
                    *operands.first().ok_or_else(short)?,
                    (
                        *operands.get(1).ok_or_else(short)?,
                        *operands.get(2).ok_or_else(short)?,
                    ),
                );
            }
            OP_VARIABLE => {
                variables.push((
                    *operands.first().ok_or_else(short)?,
                    *operands.get(1).ok_or_else(short)?,
                    *operands.get(2).ok_or_else(short)?,
                ));
            }
            _ => {}
        }
        Ok(())
    })?;

    let mut found = Vec::new();
    for (pointer_type, variable, storage_class) in variables {
        if storage_class != STORAGE_CLASS_STORAGE_BUFFER && storage_class != STORAGE_CLASS_UNIFORM {
            continue;
        }
        let (Some(&set), Some(&binding)) = (sets.get(&variable), bindings.get(&variable)) else {
            continue;
        };
        let Some(&(_, mut pointee)) = pointers.get(&pointer_type) else {
            continue;
        };
        // A descriptor array of buffers is an array of the block type.
        while let Some(&element) = arrays.get(&pointee) {
            if structs.contains_key(&pointee) {
                break;
            }
            pointee = element;
        }
        if storage_class == STORAGE_CLASS_UNIFORM && !buffer_blocks.contains(&pointee) {
            // A uniform block, not a storage buffer.
            continue;
        }
        let Some(last) = structs.get(&pointee).and_then(|members| members.last()) else {
            continue;
        };
        if !runtime_arrays.contains(last) {
            continue;
        }
        let stride = *strides.get(last).ok_or_else(|| {
            format!(
                "the storage buffer at set {set} binding {binding} ends in a runtime array with \
                 no ArrayStride decoration"
            )
        })?;
        found.push(StorageBuffer {
            set,
            binding,
            stride,
        });
    }
    Ok(found)
}

/// Checks each declared storage-buffer stride against the module, at the
/// `(set, binding)` it names.
///
/// A declared binding the module does not use is not an error — a layout is
/// shared by every stage of a pipeline and may describe resources another
/// stage's module reads — so only a binding **both** declare is compared.
///
/// # Errors
///
/// A `String` naming the binding, both strides and the likely cause.
pub fn require_storage_strides(words: &[u32], declared: &[StorageBuffer]) -> Result<(), String> {
    let module = storage_buffers(words)?;
    for layout in declared {
        let Some(shader) = module
            .iter()
            .find(|shader| shader.set == layout.set && shader.binding == layout.binding)
        else {
            continue;
        };
        if shader.stride != layout.stride {
            return Err(format!(
                "the storage buffer at set {} binding {} is declared with a stride of {} bytes, \
                 and the shader's element is {} bytes. BindingKind::StorageBuffer's stride is the \
                 size of one element of the array the shader declares — the element type on \
                 one side has changed without the other",
                layout.set, layout.binding, layout.stride, shader.stride
            ));
        }
    }
    Ok(())
}

/// The seam's stage bit for a SPIR-V execution model.
fn stage_of(model: u32) -> Option<ShaderStages> {
    match model {
        0 => Some(ShaderStages::VERTEX),
        4 => Some(ShaderStages::FRAGMENT),
        5 => Some(ShaderStages::COMPUTE),
        // `VK_EXT_mesh_shader`'s pair. **Not** the `NV` pair at 5267 and 5268:
        // those are a different extension with a different SPIR-V surface, and
        // `crcbl-vk` enables only `VK_EXT_mesh_shader`, so accepting an NV
        // module here would report an entry point the pipeline then could not
        // use.
        5364 => Some(ShaderStages::TASK),
        5365 => Some(ShaderStages::MESH),
        // Tessellation (1, 2), geometry (3), OpenCL kernels (6), and the ray
        // tracing models. Real execution models the engine has no vocabulary
        // for; `None` says so rather than pretending they are compute.
        _ => None,
    }
}

/// Decodes a SPIR-V literal string: UTF-8 packed four bytes per word, low byte
/// first, null-terminated, zero-padded to a word boundary.
fn literal_string(words: &[u32]) -> Option<String> {
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        for shift in [0, 8, 16, 24] {
            #[allow(clippy::cast_possible_truncation)]
            let byte = ((word >> shift) & 0xff) as u8;
            if byte == 0 {
                // The terminator ends the string even mid-word, which is what
                // makes the padding invisible.
                return Some(String::from_utf8_lossy(&bytes).into_owned());
            }
            bytes.push(byte);
        }
    }
    // Ran off the end without a terminator.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a module with the given entry points, so the parser is tested
    /// against bytes this file did not also produce by accident.
    fn module(entries: &[(u32, &str)]) -> Vec<u32> {
        let mut words = vec![MAGIC, 0x0001_0500, 0, 32, 0];
        for (model, name) in entries {
            let mut operands = vec![*model, 1];
            operands.extend(pack(name));
            // One interface id, so the instruction is longer than its name.
            operands.push(7);
            #[allow(clippy::cast_possible_truncation)]
            let word_count = (operands.len() + 1) as u32;
            words.push((word_count << 16) | u32::from(OP_ENTRY_POINT));
            words.extend(operands);
        }
        words
    }

    /// [`module`], with a `LocalSize` execution mode on the entry point id
    /// every entry there is given.
    fn compute_module(name: &str, size: [u32; 3]) -> Vec<u32> {
        let mut words = module(&[(5, name)]);
        words.push((6u32 << 16) | u32::from(OP_EXECUTION_MODE));
        words.extend([1, EXECUTION_MODE_LOCAL_SIZE, size[0], size[1], size[2]]);
        words
    }

    /// A SPIR-V literal string: bytes packed low-first, null-terminated, padded.
    fn pack(text: &str) -> Vec<u32> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn a_vertex_and_fragment_pair_in_one_module_is_read_back() {
        let words = module(&[(0, "vertexMain"), (4, "fragmentMain")]);
        let found = entry_points(&words).expect("parses");
        assert_eq!(
            found,
            vec![
                EntryPoint {
                    name: "vertexMain".to_string(),
                    stage: Some(ShaderStages::VERTEX),
                },
                EntryPoint {
                    name: "fragmentMain".to_string(),
                    stage: Some(ShaderStages::FRAGMENT),
                },
            ]
        );
    }

    /// A name whose length is an exact multiple of four needs a whole extra
    /// word for its terminator. Getting that wrong reads the next operand as
    /// text, which is the classic literal-string bug.
    #[test]
    fn names_at_every_alignment_round_trip() {
        for name in ["a", "ab", "abc", "abcd", "abcde", "main", "vertexMain"] {
            let words = module(&[(0, name)]);
            let found = entry_points(&words).expect("parses");
            assert_eq!(found.len(), 1, "{name}");
            assert_eq!(found[0].name, name);
        }
    }

    #[test]
    fn compute_is_recognised_and_unsupported_models_are_reported_not_dropped() {
        let words = module(&[(5, "cull"), (3, "geometry")]);
        let found = entry_points(&words).expect("parses");
        assert_eq!(found[0].stage, Some(ShaderStages::COMPUTE));
        assert_eq!(
            found[1].stage, None,
            "a geometry shader must be reported with no stage, not silently skipped"
        );
        assert_eq!(found[1].name, "geometry");
    }

    /// The two `VK_EXT_mesh_shader` models, and the two `NV` ones that must
    /// **not** be mistaken for them.
    ///
    /// The numbers are the ones this backend's own artifact carries —
    /// `crates/crcbl-shaders/spirv/mesh_shader.spv` uses 5365 and 5364 — and
    /// they are fixed by the SPIR-V grammar rather than by this crate. Getting
    /// the pair the wrong way round would accept a task entry point wherever a
    /// mesh one was asked for, which the driver then rejects with a message
    /// naming neither.
    #[test]
    fn the_ext_mesh_models_map_and_the_nv_ones_do_not() {
        let words = module(&[
            (5365, "meshMain"),
            (5364, "taskMain"),
            (5268, "meshNv"),
            (5267, "taskNv"),
        ]);
        let found = entry_points(&words).expect("parses");
        assert_eq!(found[0].stage, Some(ShaderStages::MESH));
        assert_eq!(found[1].stage, Some(ShaderStages::TASK));
        assert_eq!(found[2].stage, None, "VK_NV_mesh_shader is a different API");
        assert_eq!(found[3].stage, None);
        require_entry_point(&words, "meshMain", ShaderStages::MESH).expect("present");
        let error = require_entry_point(&words, "taskMain", ShaderStages::MESH)
            .expect_err("the task entry point is not a mesh one");
        assert!(error.contains("but not at"), "{error}");
    }

    /// The mistake the seam's docs single out: `&[u8]` reinterpreted as words.
    #[test]
    fn a_byte_swapped_module_says_so_specifically() {
        let words = [MAGIC_REVERSED, 0, 0, 0, 0];
        let error = entry_points(&words).expect_err("must be refused");
        assert!(error.contains("byte-swapped"), "{error}");
    }

    #[test]
    fn a_module_that_is_not_spirv_at_all_names_the_word_it_found() {
        let error = entry_points(&[0xdead_beef, 0, 0, 0, 0]).expect_err("must be refused");
        assert!(error.contains("0xdeadbeef"), "{error}");
        let short = entry_points(&[MAGIC, 0]).expect_err("a two-word module is not a module");
        assert!(short.contains("five-word header"), "{short}");
    }

    /// A truncated module must not be walked off the end. This is the only
    /// place in this file where a bug would be a panic rather than a wrong
    /// answer, so it gets its own test.
    #[test]
    fn a_truncated_instruction_is_an_error_rather_than_a_panic() {
        let mut words = module(&[(0, "vertexMain")]);
        words.truncate(words.len() - 2);
        let error = entry_points(&words).expect_err("the last instruction runs off the end");
        assert!(error.contains("only"), "{error}");

        // A zero word count would loop forever rather than overrun.
        let looping = [MAGIC, 0, 0, 0, 0, 0];
        let error = entry_points(&looping).expect_err("zero word count");
        assert!(error.contains("zero word count"), "{error}");
    }

    #[test]
    fn requiring_a_missing_entry_point_lists_what_is_there() {
        let words = module(&[(0, "vertexMain"), (4, "fragmentMain")]);
        require_entry_point(&words, "vertexMain", ShaderStages::VERTEX).expect("present");

        let error = require_entry_point(&words, "main", ShaderStages::VERTEX)
            .expect_err("there is no `main`");
        assert!(
            error.contains("vertexMain"),
            "the list must be shown: {error}"
        );

        // The stage mix-up gets its own wording, because "no entry point named
        // fragmentMain" would be actively misleading when there is one.
        let error = require_entry_point(&words, "fragmentMain", ShaderStages::VERTEX)
            .expect_err("wrong stage");
        assert!(error.contains("but not at"), "{error}");
    }

    /// The guard behind
    /// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size):
    /// the size the module declares is read back, and one that disagrees is
    /// refused with both numbers in the message.
    ///
    /// **What turns it red.** Reading the mode's operands at the wrong offset —
    /// the declared size comes back as the entry id or the mode number, neither
    /// of which is `[64, 2, 1]`, and the `y` of 2 is what makes an all-ones
    /// tail unable to pass by accident. Treating "the module declares nothing"
    /// as agreement — the bare module. Comparing loosely enough that
    /// `[32, 2, 1]` and `[64, 2, 1]` agree — the mismatch case.
    #[test]
    fn a_compute_entry_points_declared_workgroup_size_is_read_and_enforced() {
        let words = compute_module("computeMain", [64, 2, 1]);
        assert_eq!(
            workgroup_size(&words, "computeMain").expect("parses"),
            Some([64, 2, 1])
        );
        require_workgroup_size(&words, "computeMain", [64, 2, 1]).expect("the declared size");

        let error = require_workgroup_size(&words, "computeMain", [32, 2, 1])
            .expect_err("a descriptor that disagrees with the shader");
        assert!(error.contains("[32, 2, 1]"), "{error}");
        assert!(error.contains("numthreads(64, 2, 1)"), "{error}");

        // A module with no execution mode at all: the size is not merely
        // assumed to be whatever the descriptor said.
        let bare = module(&[(5, "computeMain")]);
        assert_eq!(workgroup_size(&bare, "computeMain").expect("parses"), None);
        let error = require_workgroup_size(&bare, "computeMain", [64, 1, 1])
            .expect_err("nothing to check against is not a pass");
        assert!(error.contains("LocalSizeId"), "{error}");

        let error = workgroup_size(&words, "notThere").expect_err("no such entry point");
        assert!(error.contains("notThere"), "{error}");
    }

    /// The parser against a real `slangc` artifact, and the constant
    /// `crcbl-shaders` publishes for callers to fill the descriptor with.
    ///
    /// The synthetic modules above are built by this file, so they can only
    /// prove it is self-consistent. This one is the committed
    /// `spirv/compute_probe.spv`, and it is what says the two numbers a caller
    /// juggles — `crcbl_shaders::compute_probe::WORKGROUP_SIZE` and the
    /// shader's own `[numthreads(…)]` — are the same number, without a driver
    /// in the room.
    #[test]
    fn the_committed_compute_artifact_declares_the_size_crcbl_shaders_publishes() {
        let words = crcbl_shaders::COMPUTE_PROBE.spirv();
        let entry_point = crcbl_shaders::COMPUTE_PROBE
            .entry_point(crcbl_shaders::Stage::Compute)
            .expect("the probe has a compute entry point");
        assert_eq!(
            workgroup_size(words, entry_point).expect("the committed artifact parses"),
            Some([crcbl_shaders::compute_probe::WORKGROUP_SIZE, 1, 1])
        );

        // Every compute entry point of the cull module — the frustum cull and
        // the two occlusion phases — named, because the module has three and a
        // stage looked up by kind is ambiguous.
        let cull = crcbl_shaders::CULL.spirv();
        for cull_entry in ["computeMain", "occlusionMain", "lateMain"] {
            assert_eq!(
                workgroup_size(cull, cull_entry).expect("the committed artifact parses"),
                Some([crcbl_shaders::cull::WORKGROUP_SIZE, 1, 1]),
                "{cull_entry}"
            );
        }
    }

    /// Instructions the parser does not care about must be skipped by their
    /// word count, not by scanning — otherwise an `OpName` containing the bytes
    /// of an opcode would be read as one.
    #[test]
    fn unrelated_instructions_are_stepped_over_by_word_count() {
        let mut words = vec![MAGIC, 0x0001_0500, 0, 32, 0];
        // A five-word OpCapability-shaped instruction with a payload that
        // happens to contain OP_ENTRY_POINT in the low bits.
        words.push((5u32 << 16) | 17);
        words.extend([u32::from(OP_ENTRY_POINT), 0, 0, 0]);
        words.extend(module(&[(0, "real")]).split_off(5));
        let found = entry_points(&words).expect("parses");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "real");
    }

    /// **The strides come out of the committed modules at their bindings**, for
    /// element types whose sizes the sources fix: `triangle.slang`'s
    /// `StructuredBuffer<Vertex>`, two `float4`s, and the compute probe's
    /// `uint` buffers. A reader that took the stride off the wrong type, or
    /// missed a decoration, reports a different number here.
    #[test]
    fn the_committed_modules_report_the_strides_their_sources_declare() {
        let triangle = storage_buffers(crcbl_shaders::TRIANGLE.spirv()).expect("triangle");
        assert_eq!(
            triangle,
            [StorageBuffer {
                set: 0,
                binding: 0,
                stride: crcbl_shaders::triangle::VERTEX_STRIDE as u32,
            }]
        );

        let probe = storage_buffers(crcbl_shaders::COMPUTE_PROBE.spirv()).expect("probe");
        assert!(
            !probe.is_empty(),
            "the compute probe declares storage buffers"
        );
        assert!(probe.iter().all(|buffer| buffer.stride == 4), "{probe:?}");
    }

    /// **A declared stride that disagrees with the module is refused, naming the
    /// binding and both numbers; one that agrees, or names a binding the module
    /// does not use, is not.** The last half matters because one layout serves
    /// every stage of a pipeline and each stage's module declares only what it
    /// reads.
    #[test]
    fn a_declared_stride_is_held_to_the_module_at_its_binding() {
        let words = crcbl_shaders::TRIANGLE.spirv();
        let at = |binding, stride| StorageBuffer {
            set: 0,
            binding,
            stride,
        };
        require_storage_strides(words, &[at(0, 32)]).expect("the declared stride matches");
        require_storage_strides(words, &[at(7, 4)])
            .expect("a binding this module does not declare is not compared");

        let error = require_storage_strides(words, &[at(0, 4)]).expect_err("4 is not 32");
        assert!(error.contains("set 0 binding 0"), "{error}");
        assert!(error.contains("stride of 4 bytes"), "{error}");
        assert!(error.contains("32 bytes"), "{error}");
    }
}
