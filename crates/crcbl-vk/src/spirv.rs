//! Just enough SPIR-V parsing to answer "does this module have that entry
//! point, and at which stage?".
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
//! [`HalError::ShaderCompilation`] with a reason, and a reason is only possible
//! if this layer knows what the module contains.
//!
//! It also catches the mistake the seam explicitly worries about — bytes passed
//! where words were wanted — one call earlier than the driver would, and with a
//! message that says so.
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

/// Reads the header and every `OpEntryPoint`.
///
/// # Errors
///
/// A `String` explaining what is wrong with the module: too short, not SPIR-V,
/// byte-swapped, or an instruction whose word count would run past the end.
pub fn entry_points(words: &[u32]) -> Result<Vec<EntryPoint>, String> {
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

    let mut found = Vec::new();
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

        if opcode == OP_ENTRY_POINT {
            // OpEntryPoint: execution model, entry point id, then the name as a
            // null-terminated literal string, then the interface ids.
            if word_count < 4 {
                return Err(format!(
                    "OpEntryPoint at word {index} is {word_count} words; the shortest legal \
                     one is 4"
                ));
            }
            let model = words[index + 1];
            let name = literal_string(&words[index + 3..end])
                .ok_or_else(|| format!("OpEntryPoint at word {index} has an unterminated name"))?;
            found.push(EntryPoint {
                name,
                stage: stage_of(model),
            });
        }

        index = end;
    }
    Ok(found)
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
}
