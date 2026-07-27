//! Shader modules.
//!
//! # SPIR-V is the interchange format
//!
//! The seam takes **SPIR-V words** and nothing else. Shaders are authored in
//! Slang and compiled at build time (`docs/plan/02-vulkan-backend.md` §2.3);
//! backends that cannot consume SPIR-V directly cross-compile it — Metal and
//! DX12 via Slang's MSL/DXIL outputs (stage 9), WebGPU via SPIR-V → WGSL
//! (stage 10). That is a *backend* concern; nothing above the seam knows a
//! second shader language exists.
//!
//! The alternative — a `ShaderSource` enum with a variant per language — was
//! rejected because it would put artifact-format selection above the seam,
//! where the renderer would have to know which backend it was talking to in
//! order to hand it the right bytes.

use crcbl_core::Handle;

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

/// Creation parameters for a shader module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderModuleDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// SPIR-V, as 32-bit words.
    ///
    /// Words rather than bytes because SPIR-V is defined as a word stream and
    /// every consumer needs 4-byte alignment; taking `&[u8]` would push a
    /// realignment copy into every backend. Build scripts emit `&[u32]` via
    /// `include_bytes!` + a const transmute helper, or read words at load time.
    pub spirv: &'a [u32],
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

    #[test]
    fn shader_desc_takes_words_not_bytes() {
        // SPIR-V magic number, as the first word of any valid module.
        let spirv = [0x0723_0203u32, 0x0001_0600, 0, 0, 0];
        let desc = ShaderModuleDesc {
            label: Some("probe"),
            spirv: &spirv,
        };
        assert_eq!(desc.spirv[0], 0x0723_0203);
        assert_eq!(desc.spirv.len(), 5);
    }
}
