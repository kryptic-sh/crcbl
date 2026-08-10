//! The workgroup size and uniform block `cull.slang` declares, in the layouts
//! that shader declares.
//!
//! Same reason as [`crate::compute_probe`]: the shader fixes a number and a byte
//! layout, every producer of those has to agree with it exactly, and keeping
//! both in the crate that owns the source means there is one place to change
//! rather than one per consumer.
//!
//! What is *not* here is the frustum itself. Extracting six planes from a
//! view-projection matrix needs a matrix type, and this crate has no
//! dependencies at all — not even `glam`. [`crcbl_render::cull`] owns that, and
//! hands the result here as six `[f32; 4]`s.
//!
//! [`crcbl_render::cull`]: https://docs.rs/crcbl-render

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/cull.slang`.
///
/// A caller dispatches `instances.div_ceil(WORKGROUP_SIZE)` groups; the shader
/// discards the invocations past [`Params::instance_count`] that the last group
/// brings.
pub const WORKGROUP_SIZE: u32 = 64;

/// Planes in a frustum, and the length of [`Params::planes`].
pub const PLANE_COUNT: usize = 6;

/// Bytes of the uniform block.
///
/// Six `float4` (96) then two `uint`, rounded up to the 16-byte multiple
/// `std140` requires of a uniform block's size. Checked against the `Offset`
/// decorations `slangc` emits by this module's
/// `the_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 112;

/// The uniform block, matching `struct CullParams` in `shaders/cull.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// The camera frustum's six half-spaces: `[nx, ny, nz, d]`, with a point
    /// inside when `n · p + d >= 0`.
    ///
    /// **Not normalized, and the shader does not want them to be.** Its test
    /// scales linearly with `n`, so normalizing changes no answer — and under
    /// the engine's reversed-Z infinite projection one plane comes out with a
    /// zero normal, which normalizing would turn into `NaN`. See
    /// [`crcbl_render::cull::Frustum`], which produces these.
    ///
    /// [`crcbl_render::cull::Frustum`]: https://docs.rs/crcbl-render
    pub planes: [[f32; 4]; PLANE_COUNT],
    /// Instances to test. Invocations at or past this index do nothing.
    pub instance_count: u32,
    /// Elements the visible list can hold. A survivor past this is counted and
    /// not written — see the shader, where the counter is deliberately the
    /// unbounded half.
    pub capacity: u32,
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    ///
    /// The tail padding is written rather than left alone, for the reason
    /// [`crate::compute_probe::Params::to_bytes`] gives: a buffer allocated for
    /// this block is [`PARAMS_SIZE`] bytes wide and a partial write leaves the
    /// rest undefined.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0usize;
        for plane in &self.planes {
            for value in plane {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        for value in [self.instance_count, self.capacity] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, 104, "the last eight bytes are std140 tail padding");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size.
    ///
    /// Nothing else can catch this, for the reason
    /// [`crate::compute_probe`]'s twin gives: the shader compiles, the dispatch
    /// succeeds, and a mismatch shows up only as a partly-tested instance array
    /// — which reads as "those instances were culled".
    #[test]
    fn the_workgroup_size_matches_the_numthreads_the_shader_declares() {
        let source = include_str!("../shaders/cull.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "cull.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted from the \
             shader"
        );
    }

    /// The offsets `slangc` actually emitted for `CullParams`, read out of the
    /// disassembly.
    #[test]
    fn the_params_block_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_arr_v4float_int_6 ArrayStride 16`, and
        // `OpMemberDecorate %CullParams_std140 n Offset …`: 0, 96, 100.
        assert_eq!(PARAMS_SIZE, 112);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a block that is not \
             one already is a block the shader and the CPU disagree about the width of"
        );

        let mut planes = [[0.0f32; 4]; PLANE_COUNT];
        for (index, plane) in planes.iter_mut().enumerate() {
            *plane = [index as f32, 0.0, 0.0, 0.0];
        }
        let bytes = Params {
            planes,
            instance_count: 7,
            capacity: 9,
        }
        .to_bytes();
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        for index in 0..PLANE_COUNT {
            assert_eq!(
                float_at(index * 16),
                index as f32,
                "plane {index} at offset {}",
                index * 16
            );
        }
        assert_eq!(uint_at(96), 7, "instance_count at offset 96");
        assert_eq!(uint_at(100), 9, "capacity at offset 100");
        assert!(
            bytes[104..].iter().all(|byte| *byte == 0),
            "the std140 tail padding is written, and it is zero: {:?}",
            &bytes[104..]
        );
    }

    /// `cull.slang` and `draw_gen.slang` re-declare `GpuInstance` and `GpuMesh`
    /// because there is no shared header — the compile script hashes one source
    /// per artifact, so an `#include` would be a file whose edits nothing
    /// notices. This is what keeps the copies from drifting: it compares the
    /// *fields*, so a reworded doc comment is not a failure and a renamed,
    /// retyped, reordered, added or removed field is.
    ///
    /// A drift here is not a compile error anywhere. Every file builds; the
    /// shaders simply read different bytes out of the same buffer.
    #[test]
    fn the_shared_structs_are_declared_identically_in_every_shader() {
        let mesh = include_str!("../shaders/mesh.slang");
        let others = [
            ("cull.slang", include_str!("../shaders/cull.slang")),
            ("draw_gen.slang", include_str!("../shaders/draw_gen.slang")),
        ];
        for name in ["GpuInstance", "GpuMesh"] {
            let declared = struct_fields(mesh, name);
            assert!(
                !declared.is_empty(),
                "`struct {name}` was not found in mesh.slang, so this comparison checked nothing"
            );
            for (source, text) in others {
                assert_eq!(
                    declared,
                    struct_fields(text, name),
                    "`struct {name}` differs between mesh.slang and {source}; the two shaders \
                     would read the same buffer with different layouts"
                );
            }
        }
    }

    /// The field lines of `struct name` in `source`, with comments and blank
    /// lines dropped and each line trimmed.
    ///
    /// Empty when the struct is not there at all, which the caller asserts
    /// against — a matcher that silently found nothing would make two files
    /// with no structs in them agree perfectly.
    fn struct_fields(source: &str, name: &str) -> Vec<String> {
        let opening = format!("struct {name}\n{{\n");
        let Some(start) = source.find(&opening) else {
            return Vec::new();
        };
        let body = &source[start + opening.len()..];
        let end = body.find("\n};").unwrap_or(body.len());
        body[..end]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(str::to_string)
            .collect()
    }

    /// The comparison above must be able to see a difference, which a matcher
    /// that returned the same thing for every input could not.
    #[test]
    fn the_struct_comparison_notices_a_changed_field() {
        let original = "struct Thing\n{\n    /// doc\n    uint a;\n    float b;\n};\n";
        let renamed = "struct Thing\n{\n    uint a;\n    float c;\n};\n";
        assert_eq!(
            struct_fields(original, "Thing"),
            vec!["uint a;".to_string(), "float b;".to_string()],
            "comments and blank lines are dropped and fields are not"
        );
        assert_ne!(
            struct_fields(original, "Thing"),
            struct_fields(renamed, "Thing")
        );
        assert!(struct_fields(original, "Absent").is_empty());
    }
}
