//! Shader modules.
//!
//! # The caller offers every artifact it has; the backend picks
//!
//! [`ShaderModuleDesc`] carries **one field per artifact format** — SPIR-V
//! words, WGSL source, MSL source — and the caller fills in every one it has.
//! It does not choose between them and it is not told which backend it is
//! talking to; the backend takes the format it can consume and ignores the
//! rest.
//!
//! This replaces "the seam takes SPIR-V words and nothing else", which held
//! from P0 to P5 on the premise that a backend which cannot consume SPIR-V
//! cross-compiles it. `crcbl-wgpu` disproved the premise: `naga`, the only
//! SPIR-V frontend a browser build can carry, does not implement
//! `DrawParameters`, and every artifact this engine ships declares it because
//! Slang lowers `SV_VertexID` to `gl_VertexIndex - gl_BaseVertex`. So
//! `crcbl-wgpu` could not create a shader module on **any** target — the
//! failure was not browser-specific — while `crcbl-shaders` had been emitting
//! complete WGSL since P5.3 that the seam had no field to carry.
//!
//! Recompiling the SPIR-V without `DrawParameters` would also have unblocked
//! naga. It was rejected: it constrains the Vulkan backend, which wants vertex
//! pulling, in order to widen the wgpu one, and the WGSL artifacts already
//! exist.
//!
//! ## Why fields and not a `ShaderSource` enum
//!
//! An enum with a variant per language was rejected at P0 and is still
//! rejected, for the reason first written down then: it puts artifact-format
//! *selection* above the seam, where the renderer would have to know which
//! backend it was talking to in order to hand it the right bytes. Parallel
//! fields keep the selection below the seam, which is the whole point — a
//! caller says "here is the SPIR-V, here is the WGSL, here is the MSL", every
//! backend sees the same descriptor, and P14's MSL cost exactly one field —
//! not a variant every existing `match` had to grow an arm for. DXIL will cost
//! the same.
//!
//! ## The contract
//!
//! **Callers must supply every format they hold.** Omitting one is not a
//! preference — it is a statement that the artifact does not exist, and it is
//! how a shader silently becomes unusable on a backend that has not been
//! written yet. `crcbl-shaders`' generated table hands out every one
//! (`Shader::spirv`, `Shader::wgsl`, `Shader::msl`), so the correct call site
//! names all three.
//!
//! **A descriptor must carry at least one format.** One with neither is
//! [`HalError::ShaderCompilation`].
//!
//! **A backend that is handed only formats it cannot consume must fail, by
//! name.** [`HalError::ShaderCompilation`]
//! whose message says which formats were offered and which the backend needed —
//! never a silent fallback, never a module handle that no pipeline can use.
//! [`ShaderSources`] exists so that message is spelled the same way everywhere.

use core::fmt;

use crcbl_core::Handle;

use crate::HalError;

/// Marker type for shader-module handles. Uninhabited.
#[derive(Debug)]
pub enum ShaderModule {}

/// A compiled shader module.
pub type ShaderModuleHandle = Handle<ShaderModule>;

bitflags::bitflags! {
    /// Which shader stages something applies to.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ShaderStages: u32 {
        /// Vertex stage.
        const VERTEX = 1 << 0;
        /// Fragment stage.
        const FRAGMENT = 1 << 1;
        /// Compute stage.
        const COMPUTE = 1 << 2;

        /// Every graphics stage the engine uses.
        const GRAPHICS = Self::VERTEX.bits() | Self::FRAGMENT.bits();
        /// Every stage.
        const ALL = Self::GRAPHICS.bits() | Self::COMPUTE.bits();
    }
}

bitflags::bitflags! {
    /// Which shader artifact formats are in play — offered by a caller, or
    /// consumable by a backend.
    ///
    /// One bit per field of [`ShaderModuleDesc`]. It exists so that "you gave
    /// me WGSL and I need SPIR-V" is one sentence written once rather than a
    /// phrase each backend invents, and so a test can assert what a call site
    /// supplied without reaching for the descriptor's fields one at a time.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ShaderSources: u32 {
        /// [`ShaderModuleDesc::spirv`] is non-empty.
        const SPIRV = 1 << 0;
        /// [`ShaderModuleDesc::wgsl`] is `Some`.
        const WGSL = 1 << 1;
        /// [`ShaderModuleDesc::msl`] is `Some`.
        const MSL = 1 << 2;
    }
}

impl fmt::Display for ShaderSources {
    /// Names the formats in a form that reads inside a sentence:
    /// `"SPIR-V and WGSL"`, `"WGSL"`, or `"nothing"` for the empty set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (bit, name) in [
            (Self::SPIRV, "SPIR-V"),
            (Self::WGSL, "WGSL"),
            (Self::MSL, "MSL"),
        ] {
            if self.contains(bit) {
                if !first {
                    f.write_str(" and ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        if first {
            f.write_str("nothing")
        } else {
            Ok(())
        }
    }
}

/// Creation parameters for a shader module.
///
/// One field per artifact format. Fill in every one you have — see the
/// [module docs](self) for why the caller offers and the backend picks, and for
/// what a backend owes you when it cannot use what you offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaderModuleDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// SPIR-V, as 32-bit words. Empty when there is no SPIR-V artifact.
    ///
    /// Words rather than bytes because SPIR-V is defined as a word stream and
    /// every consumer needs 4-byte alignment; taking `&[u8]` would push a
    /// realignment copy into every backend. Build scripts emit `&[u32]` via
    /// `include_bytes!` + a const transmute helper, or read words at load time.
    ///
    /// Empty rather than `Option<&[u32]>` for absence: a zero-word SPIR-V
    /// module is not a thing that exists — the format's own header is five
    /// words — so the two states cannot be confused, and every existing call
    /// site keeps passing a slice.
    pub spirv: &'a [u32],
    /// WGSL source, as text. `None` when there is no WGSL artifact.
    ///
    /// `Option<&str>` rather than `""` for absence, unlike `spirv` above,
    /// because the empty string *is* a syntactically valid WGSL module — one
    /// with no entry points. Conflating it with "no artifact" would turn a
    /// truncated file into "this backend does not get WGSL" and send the caller
    /// looking in the wrong place.
    ///
    /// Text and not a parsed module: WGSL's interchange form is source, every
    /// consumer of it is a compiler, and the seam has no business owning a
    /// syntax tree.
    pub wgsl: Option<&'a str>,
    /// Metal Shading Language source, as text. `None` when there is no MSL
    /// artifact.
    ///
    /// `Option<&str>` for the same reason [`wgsl`](Self::wgsl) is one: an empty
    /// MSL translation unit is valid — it declares no functions — so conflating
    /// it with "no artifact" would turn a truncated file into "this backend
    /// does not get MSL".
    ///
    /// **Source, not a compiled `.metallib`.** Metal's offline compiler ships
    /// only with Xcode, so `crcbl-shaders` stops at MSL text and `crcbl-mtl`
    /// finishes the job with
    /// `MTLDevice::newLibraryWithSource:options:error:`. A backend that fails
    /// to compile it owes the caller Metal's own message — see
    /// [`HalError::ShaderCompilation`].
    pub msl: Option<&'a str>,
}

impl ShaderModuleDesc<'_> {
    /// Which formats this descriptor actually carries.
    ///
    /// The one place the "empty means absent" rule for
    /// [`spirv`](Self::spirv) is spelled out, so no backend re-derives it.
    #[must_use]
    pub fn provided(&self) -> ShaderSources {
        let mut sources = ShaderSources::empty();
        if !self.spirv.is_empty() {
            sources |= ShaderSources::SPIRV;
        }
        if self.wgsl.is_some() {
            sources |= ShaderSources::WGSL;
        }
        if self.msl.is_some() {
            sources |= ShaderSources::MSL;
        }
        sources
    }

    /// The error a backend owes a caller when this descriptor carries nothing
    /// it can consume — including the case where it carries nothing at all.
    ///
    /// `accepted` is what the backend can compile. The message names both
    /// sides, because "shader compilation failed" without them sends the reader
    /// to the shader source when the bug is at the call site.
    #[must_use]
    pub fn unusable(&self, accepted: ShaderSources) -> HalError {
        HalError::ShaderCompilation(format!(
            "shader module `{label}` was given {given}, but this backend can only compile \
             {accepted}",
            label = self.label.unwrap_or("<unlabelled>"),
            given = self.provided(),
        ))
    }
}

/// One stage of a pipeline: a module plus the entry point to use from it.
///
/// Slang emits several entry points into one module routinely (a vertex and
/// fragment pair, or a cull shader and its compaction pass), so the entry point
/// is named per-stage rather than assumed to be `main`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderEntry<'a> {
    /// Module the code lives in.
    pub module: ShaderModuleHandle,
    /// Entry-point name, as it appears in the SPIR-V `OpEntryPoint`.
    pub entry_point: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_groups_are_unions_of_their_members() {
        assert!(ShaderStages::GRAPHICS.contains(ShaderStages::VERTEX));
        assert!(ShaderStages::GRAPHICS.contains(ShaderStages::FRAGMENT));
        assert!(!ShaderStages::GRAPHICS.contains(ShaderStages::COMPUTE));
        assert!(ShaderStages::ALL.contains(ShaderStages::GRAPHICS));
        assert!(ShaderStages::ALL.contains(ShaderStages::COMPUTE));
    }

    /// SPIR-V magic number, as the first word of any valid module.
    const MAGIC: u32 = 0x0723_0203;

    #[test]
    fn shader_desc_takes_words_not_bytes() {
        let spirv = [MAGIC, 0x0001_0600, 0, 0, 0];
        let desc = ShaderModuleDesc {
            label: Some("probe"),
            spirv: &spirv,
            wgsl: None,
            msl: None,
        };
        assert_eq!(desc.spirv[0], MAGIC);
        assert_eq!(desc.spirv.len(), 5);
    }

    /// The contract's "empty means absent" rule, in both fields, read back
    /// through the one function every backend uses to read it.
    #[test]
    fn provided_reports_exactly_the_formats_that_are_present() {
        let spirv = [MAGIC, 0x0001_0600, 0, 0, 0];
        assert_eq!(
            ShaderModuleDesc::default().provided(),
            ShaderSources::empty()
        );
        assert_eq!(
            ShaderModuleDesc {
                spirv: &spirv,
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::SPIRV
        );
        assert_eq!(
            ShaderModuleDesc {
                wgsl: Some("@fragment fn main() {}"),
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::WGSL
        );
        assert_eq!(
            ShaderModuleDesc {
                msl: Some("[[fragment]] float4 main() { return 0; }"),
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::MSL
        );
        assert_eq!(
            ShaderModuleDesc {
                spirv: &spirv,
                wgsl: Some("@fragment fn main() {}"),
                msl: Some("[[fragment]] float4 main() { return 0; }"),
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::SPIRV | ShaderSources::WGSL | ShaderSources::MSL
        );
    }

    /// An empty text string is a *present* artifact, not an absent one — the
    /// distinction `Option` exists to keep, for both text formats.
    #[test]
    fn an_empty_text_artifact_is_present_not_absent() {
        assert_eq!(
            ShaderModuleDesc {
                wgsl: Some(""),
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::WGSL
        );
        assert_eq!(
            ShaderModuleDesc {
                msl: Some(""),
                ..ShaderModuleDesc::default()
            }
            .provided(),
            ShaderSources::MSL
        );
    }

    #[test]
    fn source_sets_name_themselves_in_a_sentence() {
        assert_eq!(ShaderSources::empty().to_string(), "nothing");
        assert_eq!(ShaderSources::SPIRV.to_string(), "SPIR-V");
        assert_eq!(ShaderSources::WGSL.to_string(), "WGSL");
        assert_eq!(ShaderSources::MSL.to_string(), "MSL");
        assert_eq!(
            (ShaderSources::SPIRV | ShaderSources::WGSL).to_string(),
            "SPIR-V and WGSL"
        );
        assert_eq!(
            ShaderSources::all().to_string(),
            "SPIR-V and WGSL and MSL",
            "every bit must have a name in the sentence, or a backend's refusal \
             would silently omit the format the caller actually supplied"
        );
    }

    /// The error a backend owes names both sides of the gap, and the module, so
    /// the reader looks at the call site rather than at the shader source.
    #[test]
    fn the_unusable_error_names_the_gap() {
        let desc = ShaderModuleDesc {
            label: Some("mesh.slang"),
            wgsl: Some("@fragment fn main() {}"),
            ..ShaderModuleDesc::default()
        };
        let HalError::ShaderCompilation(message) = desc.unusable(ShaderSources::SPIRV) else {
            panic!("an unusable descriptor is a shader-compilation failure");
        };
        assert!(message.contains("mesh.slang"), "{message}");
        assert!(message.contains("was given WGSL"), "{message}");
        assert!(message.contains("only compile SPIR-V"), "{message}");
    }

    #[test]
    fn a_descriptor_with_no_artifact_at_all_says_so() {
        let HalError::ShaderCompilation(message) =
            ShaderModuleDesc::default().unusable(ShaderSources::all())
        else {
            panic!("an unusable descriptor is a shader-compilation failure");
        };
        assert!(message.contains("was given nothing"), "{message}");
    }
}
