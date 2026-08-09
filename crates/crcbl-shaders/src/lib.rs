//! The engine's shaders: Slang sources, and the SPIR-V, WGSL, MSL and DXIL
//! compiled from them.
//!
//! ```text
//! shaders/*.slang ──tools/compile-shaders.sh──┬─▶ spirv/*.spv          (committed)
//!                                             ├─▶ wgsl/*.wgsl          (committed)
//!                                             ├─▶ msl/*.metal          (committed)
//!                                             └─▶ dxil/*.<entry>.dxil  (committed)
//!                                                    │
//!                          build.rs verifies ────────┤
//!                                                    ▼
//!        crcbl_shaders::TRIANGLE.spirv() / .wgsl() / .msl() / .dxil(entry)
//!                                                    │
//!                             ShaderModuleDesc { spirv, wgsl, msl, dxil }
//! ```
//!
//! Every artifact is handed over on every call and the backend picks:
//! `crcbl-vk` reads the SPIR-V, `crcbl-wgpu` reads the WGSL, `crcbl-mtl` reads
//! the MSL, `crcbl-dx12` reads the DXIL. See `crcbl_hal::shader` for why the
//! seam is shaped that way and what a caller owes it.
//!
//! # DXIL is one artifact per entry point, and that is not a stylistic choice
//!
//! `dxc` compiles exactly one `-E` per invocation, and a D3D12 pipeline state
//! object takes one bytecode blob per stage — so there is nothing a container
//! holding two entry points could be handed to. The other three targets each
//! produce one artifact carrying every entry point of the module.
//!
//! That difference reaches all the way up: [`Shader::dxil`] takes an entry-point
//! name where [`Shader::wgsl`] and [`Shader::msl`] take nothing, and a D3D12
//! caller creates one shader module per stage where the others create one per
//! shader. `spirv/manifest.txt` records one `dxil` line per entry point for the
//! same reason.
//!
//! # Decision: Slang source, committed SPIR-V, no compiler in the build
//!
//! `docs/plan/02-vulkan-backend.md` §2.3 chooses Slang and gives the reasons —
//! "first-class SPIR-V target, HLSL-compatible syntax eases the DX12 stage,
//! good buffer-device-address support" — and three other topics depend on that
//! choice: `09-backends-metal-dx12.md` needs Slang's MSL and DXIL outputs,
//! `10-wasm-webgpu.md` needs a WGSL artifact at P5, and
//! `03-gpu-driven-rendering.md`'s design review says per-tier authoring is "one
//! source: Slang with a `TIER_B` capability specialization", **"Decided before
//! any shader is written, because P1's shaders become P5's inputs."** So the
//! sources here are Slang, from the first triangle.
//!
//! The *artifacts* take that topic's own escape hatch, verbatim: "Slang
//! toolchain friction in build.rs. Fallback: check in compiled SPIR-V alongside
//! sources until the toolchain story is smooth." The compiled SPIR-V is
//! committed, and **nothing in a normal build needs a shader compiler** —
//! not a contributor's first `cargo build`, not the macOS or Windows CI legs,
//! not `test (linux)`.
//!
//! The obvious hazard of committing generated files is that they rot. Three
//! things stop that, in increasing order of strength:
//!
//! 1. **`build.rs` hashes.** `spirv/manifest.txt` records the SHA-256 of every
//!    source *and* every artifact, and the build fails when either moves. This
//!    needs no compiler, so it runs on every machine and in every CI job — the
//!    check is not conditional on the very toolchain that is optional.
//! 2. **`build.rs` recompiles, when it can.** A developer who happens to have
//!    the pinned `slangc` gets a byte-for-byte comparison for free, which
//!    catches the one thing a hash cannot: a manifest regenerated against a
//!    source that was then not committed.
//! 3. **CI recompiles, always.** The `shaders` job installs the pinned `slangc`
//!    and runs `tools/compile-shaders.sh --check`, so drift is caught by a
//!    machine that is not the author's. That job is the reason the version is
//!    pinned: two Slang releases legitimately emit different SPIR-V for
//!    identical source, so an unpinned byte comparison would fail for reasons
//!    that are not drift.
//!
//! ## What this defers, deliberately
//!
//! `docs/plan/06-assets-scenes.md` wants **runtime** recompilation for shader
//! hot reload, "keyed by shader hash", at P9. Nothing here forecloses it: the
//! hash is already in the manifest and [`sha256`] is already public, and a
//! runtime path adds a `slangc`-shaped compiler behind a dev-only feature
//! without changing this crate's shape. P5's WGSL took exactly that shape —
//! another artifact column in the manifest, another `include_bytes!` in the
//! generated table — P14's MSL took it again, and DX4's DXIL took it a third
//! time, widened only by the per-entry-point split above.
//!
//! # The MSL column is *source*, not a `.metallib`
//!
//! `msl/*.metal` is Metal Shading Language text, and `crcbl-mtl` compiles it at
//! device-init time through `MTLDevice::newLibraryWithSource:options:error:`.
//! The alternative — a pre-linked `.metallib` — needs Apple's `metal` compiler,
//! which exists only on macOS with Xcode installed, and this script runs on
//! every leg of CI including the Linux one. A macOS-only step in the middle of
//! it would make the artifacts unregenerable on the machine most contributors
//! have, which is precisely the toolchain friction the committed-artifact
//! design exists to avoid. The cost is a compile at start-up per module, on the
//! same path `crcbl-wgpu` already pays for WGSL.
//!
//! What is *not* here is `03-gpu-driven-rendering.md`'s tier permutation axis.
//! The triangle does not vary by tier, and a permutation system with one
//! permutation would be a guess at the shape `37-materials.md` owns.
//!
//! # No shader here uses a push constant, and that is a rule
//!
//! WGSL has no push constants — they are a native-`wgpu` extension, absent from
//! WebGPU — and Slang's WGSL target does not say so. It lowers a
//! `[[vk::push_constant]]` block to a module-scope `var<uniform>` with **no**
//! `@group`/`@binding`, which is not valid WGSL:
//!
//! ```text
//! var<uniform> constants_0 : UiConstants_std430_0;
//! ```
//!
//! `naga` rejects that — "Binding decoration is missing or not applicable" — so
//! `crcbl-wgpu` cannot create the module at all. It is not fixable in the
//! artifact, which is a faithful translation of a source asking for something
//! the target does not have; the only fix is in the source.
//!
//! `ui.slang` carried exactly this until 2026-08, along with a `ui_tier_b.slang`
//! twin that spelled the same block as a bound uniform buffer for the WGSL
//! consumers, and a `ConstantDelivery` enum in `crcbl-render` picking between
//! the two artifacts by capability. It now takes its constants from a uniform
//! buffer on every target, like every other shader here, and the twin is gone.
//!
//! **So a shader added here may not use a push constant**, on any target,
//! however native-only it looks — a `wgsl` line in its `crcbl-targets`
//! declaration then produces an artifact nothing can load, and the fork that
//! works around it is the cost this crate has already paid once.
//!
//! The DXIL column never shared the gap: HLSL has root constants and Slang
//! lowers the same block to an ordinary `cbuffer`. The rule is WGSL's.
//!
//! # Nothing here knows a backend
//!
//! This crate has no dependencies at all, not even `crcbl-hal`. It hands out
//! `&[u32]`, `Option<&str>`, `Option<&[u8]>` and entry-point names, which is
//! exactly what `crcbl_hal::ShaderModuleDesc` takes field for field, and it
//! stays usable by
//! a backend that has not been written yet. Each new artifact format is a new
//! accessor here and a new field there; neither names the other's types.

pub mod sha256;

/// The manifest format, and its parser.
///
/// Public because `build.rs` shares this exact code, and because P9's hot
/// reload will re-read the same file at runtime to key pipeline rebuilds by
/// shader hash (`docs/plan/06-assets-scenes.md`).
pub mod manifest;

/// The declaration-order lint over `shaders/*.slang`.
///
/// Test-only, because it has no runtime consumer: it reads the *sources* this
/// crate compiles from rather than any artifact it ships, and what it protects
/// is `crcbl-mtl`'s argument-table indices, which are decided when the MSL is
/// generated and not at any later moment a caller could ask about.
#[cfg(test)]
mod declaration_order;

/// The workgroup size and uniform block `compute_probe.slang` declares, in the
/// layouts that shader declares.
pub mod compute_probe;

/// The geometry and uniform block `mesh.slang` reads, in the layouts that
/// shader declares.
pub mod mesh;

/// The geometry `triangle.slang` pulls, in the layout that shader declares.
pub mod triangle;

use std::sync::OnceLock;

/// Little-endian `f32`s in iteration order — what `std430` means for a struct
/// of `float4`s, and what every target this engine has is.
///
/// `mesh` and `triangle` write different vertex structs but the same loop, so
/// it lives here once rather than once per module.
pub(crate) fn pack_f32_le<'a>(
    values: impl IntoIterator<Item = &'a f32>,
    capacity: usize,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Which pipeline stage an entry point is for.
///
/// Deliberately *not* `crcbl_hal::ShaderStages`: this crate has no dependency
/// on the seam, and a caller maps one to the other in the single line where it
/// builds a `ShaderEntry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Vertex stage.
    Vertex,
    /// Fragment stage.
    Fragment,
    /// Compute stage.
    Compute,
}

/// One `OpEntryPoint` in a compiled module, and the DXIL compiled for it.
///
/// **The DXIL lives here rather than on [`Shader`] because a container holds one
/// entry point.** `dxc` compiles a single `-E` per invocation and a D3D12
/// pipeline state object takes one bytecode blob per stage, so there is nothing
/// a container carrying a whole module could be handed to — where the SPIR-V,
/// WGSL and MSL artifacts each carry every entry point of their module and sit
/// on the shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryPoint {
    name: &'static str,
    stage: Stage,
    /// The DXIL container for this entry point. `&[]` when not compiled
    /// (DX4+).
    dxil_bytes: &'static [u8],
}

impl EntryPoint {
    /// Names an entry point. Called only by the generated table.
    #[must_use]
    pub const fn new(name: &'static str, stage: Stage, dxil_bytes: &'static [u8]) -> Self {
        Self {
            name,
            stage,
            dxil_bytes,
        }
    }

    /// The compiled DXIL container for this entry point, or `None` when there is
    /// no DXIL artifact.
    ///
    /// This is what `crcbl_hal::ShaderModuleDesc::dxil` takes, in the shape it
    /// takes it: raw bytes, because a DXIL container is a *signed binary blob*
    /// whose digest covers its own contents — neither decoded nor realigned on
    /// the way. `crcbl-dx12` hands the slice straight to
    /// `D3D12_SHADER_BYTECODE`; the other backends ignore it.
    #[must_use]
    pub const fn dxil(&self) -> Option<&'static [u8]> {
        if self.dxil_bytes.is_empty() {
            None
        } else {
            Some(self.dxil_bytes)
        }
    }

    /// The name, exactly as it appears in the SPIR-V `OpEntryPoint` — which is
    /// what a backend matches against, so its case is load-bearing.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The stage it runs at.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }
}

/// A compiled shader, and where it came from.
#[derive(Debug)]
pub struct Shader {
    name: &'static str,
    source: &'static str,
    source_sha256: &'static str,
    /// The SPIR-V artifact, as raw bytes.
    spirv_bytes: &'static [u8],
    /// The WGSL artifact, as raw UTF-8 bytes. `&[]` when not compiled (P5+).
    wgsl_bytes: &'static [u8],
    /// The MSL artifact, as raw UTF-8 bytes. `&[]` when not compiled (P14+).
    msl_bytes: &'static [u8],
    entry_points: &'static [EntryPoint],
    /// The SPIR-V byte stream decoded into words, once.
    words: OnceLock<Vec<u32>>,
}

/// The SPIR-V magic number, as the first word of any valid module.
pub const SPIRV_MAGIC: u32 = 0x0723_0203;

impl Shader {
    /// Declares a shader. Called only by the generated table.
    #[must_use]
    pub const fn new(
        name: &'static str,
        source: &'static str,
        source_sha256: &'static str,
        spirv_bytes: &'static [u8],
        wgsl_bytes: &'static [u8],
        msl_bytes: &'static [u8],
        entry_points: &'static [EntryPoint],
    ) -> Self {
        Self {
            name,
            source,
            source_sha256,
            spirv_bytes,
            wgsl_bytes,
            msl_bytes,
            entry_points,
            words: OnceLock::new(),
        }
    }

    /// The shader's name, which is its source file's stem.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The Slang source path, relative to this crate's root.
    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.source
    }

    /// The SHA-256 of the source this was compiled from.
    ///
    /// The identity P9's hot reload will key a pipeline rebuild on.
    #[must_use]
    pub const fn source_sha256(&self) -> &'static str {
        self.source_sha256
    }

    /// The compiled module, as SPIR-V words.
    ///
    /// This is what `crcbl_hal::ShaderModuleDesc::spirv`
    /// takes.
    ///
    /// # Panics
    ///
    /// If the embedded artifact's length is not a multiple of four. `build.rs`
    /// hash-checks every artifact, so this cannot happen for a manifest that
    /// agrees with the tree — but `chunks_exact` would otherwise *drop* the
    /// trailing partial word of a truncated `.spv` and hand the driver a
    /// silently shortened module.
    #[must_use]
    pub fn spirv(&self) -> &[u32] {
        self.words.get_or_init(|| {
            let chunks = self.spirv_bytes.chunks_exact(4);
            assert!(
                chunks.remainder().is_empty(),
                "shader `{}`: the committed SPIR-V is {} bytes, which is not a whole number of \
                 32-bit words — the artifact is truncated",
                self.name,
                self.spirv_bytes.len(),
            );
            chunks
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        })
    }

    /// The compiled WGSL source, valid UTF-8, or `None` for a shader with no
    /// WGSL artifact.
    ///
    /// This is what `crcbl_hal::ShaderModuleDesc::wgsl` takes, in the shape it
    /// takes it: the seam spells "no artifact" as `None` there, so this spells
    /// it as `None` here and a call site is `wgsl: MESH.wgsl()` with nothing in
    /// between. The wgpu backend prefers it over the SPIR-V; the Vulkan backend
    /// ignores it.
    ///
    /// # Panics
    ///
    /// If the embedded artifact is not valid UTF-8. Mapping that to absence
    /// would turn a corrupt artifact into "this shader has no WGSL", which is a
    /// legitimate state, and the wgpu backend would then report the shader as
    /// missing rather than as broken.
    #[must_use]
    pub fn wgsl(&self) -> Option<&str> {
        self.text_artifact(self.wgsl_bytes, "WGSL")
    }

    /// The compiled Metal Shading Language source, valid UTF-8, or `None` for a
    /// shader with no MSL artifact.
    ///
    /// This is what `crcbl_hal::ShaderModuleDesc::msl` takes, in the shape it
    /// takes it, exactly as [`wgsl`](Self::wgsl) is — and it is *source*, not a
    /// compiled `.metallib`: `crcbl-mtl` hands it to
    /// `MTLDevice::newLibraryWithSource:options:error:`. The crate docs say why
    /// the artifact stops at source. The Vulkan and wgpu backends ignore it.
    ///
    /// # Panics
    ///
    /// If the embedded artifact is not valid UTF-8, for the reason
    /// [`wgsl`](Self::wgsl) gives: absence is a legitimate state and mapping
    /// corruption onto it would report a broken shader as a missing one.
    #[must_use]
    pub fn msl(&self) -> Option<&str> {
        self.text_artifact(self.msl_bytes, "MSL")
    }

    /// The compiled DXIL container for one entry point, or `None` for a shader
    /// with no DXIL artifact — or an entry point this shader does not have.
    ///
    /// **This is the one accessor that takes an argument, because DXIL is the
    /// one target with an artifact per entry point.** `dxc` compiles a single
    /// `-E`, and a D3D12 pipeline state object takes one bytecode blob per
    /// stage, so there is nothing a container holding both a vertex and a
    /// fragment entry point could be handed to. A caller therefore creates one
    /// `crcbl_hal::ShaderModuleDesc` per stage, each with the container for that
    /// stage's entry point, where SPIR-V, WGSL and MSL each get one module for
    /// the whole shader.
    ///
    /// It is a lookup by name because the containers hang off
    /// [`EntryPoint::dxil`], which is where the per-entry-point shape is
    /// expressed rather than restated.
    #[must_use]
    pub fn dxil(&self, entry_point: &str) -> Option<&'static [u8]> {
        self.entry_points
            .iter()
            .find(|entry| entry.name == entry_point)
            .and_then(EntryPoint::dxil)
    }

    /// One text artifact, decoded — the body [`wgsl`](Self::wgsl) and
    /// [`msl`](Self::msl) share.
    ///
    /// `&[]` is absence, because an empty artifact is not something the
    /// generator can emit for a column that exists; anything else must be
    /// UTF-8, and is not silently downgraded to absence if it is not.
    fn text_artifact<'a>(&self, bytes: &'a [u8], what: &str) -> Option<&'a str> {
        if bytes.is_empty() {
            return None;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(error) => panic!(
                "shader `{}`: the committed {what} artifact is not valid UTF-8 ({error})",
                self.name
            ),
        }
    }

    /// Every entry point the module exposes.
    #[must_use]
    pub const fn entry_points(&self) -> &'static [EntryPoint] {
        self.entry_points
    }

    /// The entry point for `stage`, if the module has exactly one.
    ///
    /// `None` rather than a panic, and `None` for *two* matches as well as
    /// none: a module with two vertex entry points is a real thing Slang can
    /// emit, and picking one arbitrarily would draw the wrong geometry rather
    /// than fail.
    #[must_use]
    pub fn entry_point(&self, stage: Stage) -> Option<&'static str> {
        let mut found = None;
        for entry in self.entry_points {
            if entry.stage == stage {
                if found.is_some() {
                    return None;
                }
                found = Some(entry.name);
            }
        }
        found
    }
}

include!(concat!(env!("OUT_DIR"), "/shaders.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    /// Every artifact really is SPIR-V, in the byte order the decode assumes.
    ///
    /// The magic number is byte-order-sensitive on purpose: a big-endian module
    /// would read back as `0x03022307` here, which is precisely the "passed the
    /// wrong bytes" mistake that otherwise surfaces as a driver error on
    /// someone else's machine.
    #[test]
    fn every_shipped_shader_is_little_endian_spirv() {
        assert!(!ALL.is_empty(), "the crate ships no shaders at all");
        for shader in ALL {
            let words = shader.spirv();
            assert!(
                words.len() >= 5,
                "{}: a SPIR-V module has at least a five-word header",
                shader.name()
            );
            assert_eq!(
                words[0],
                SPIRV_MAGIC,
                "{}: first word is {:#010x}, not the SPIR-V magic",
                shader.name(),
                words[0]
            );
            assert_eq!(
                shader.spirv().len() * 4,
                shader.spirv_bytes.len(),
                "{}: the artifact is not a whole number of words",
                shader.name()
            );
        }
    }

    /// The decode is memoised, and memoising must not change the answer.
    #[test]
    fn spirv_words_are_stable_across_calls() {
        for shader in ALL {
            let first = shader.spirv().to_vec();
            assert_eq!(shader.spirv(), first.as_slice(), "{}", shader.name());
        }
    }

    /// A shader nothing can address is a shader that will fail at pipeline
    /// creation on a machine that is not this one.
    #[test]
    fn every_shader_exposes_at_least_one_addressable_entry_point() {
        for shader in ALL {
            assert!(
                !shader.entry_points().is_empty(),
                "{} declares no entry points",
                shader.name()
            );
            for entry in shader.entry_points() {
                assert!(!entry.name().is_empty(), "{}", shader.name());
            }
        }
    }

    /// The triangle is milestone 2's shader, and the pair of entry points in one
    /// module is the concrete reason `ShaderEntry` names one per stage.
    #[test]
    fn the_triangle_has_a_vertex_and_a_fragment_entry_point_in_one_module() {
        assert_eq!(TRIANGLE.entry_point(Stage::Vertex), Some("vertexMain"));
        assert_eq!(TRIANGLE.entry_point(Stage::Fragment), Some("fragmentMain"));
        assert_eq!(TRIANGLE.entry_point(Stage::Compute), None);
        assert_eq!(TRIANGLE.entry_points().len(), 2);
        assert_eq!(TRIANGLE.name(), "triangle");
    }

    /// Two entry points at the same stage must be reported as ambiguous rather
    /// than silently resolved — picking one would draw the wrong thing.
    #[test]
    fn an_ambiguous_stage_resolves_to_nothing() {
        static ENTRIES: [EntryPoint; 3] = [
            EntryPoint::new("shadowVs", Stage::Vertex, b""),
            EntryPoint::new("mainVs", Stage::Vertex, b""),
            EntryPoint::new("mainFs", Stage::Fragment, b""),
        ];
        let ambiguous = Shader::new(
            "ambiguous",
            "shaders/ambiguous.slang",
            "0000000000000000000000000000000000000000000000000000000000000000",
            b"",
            b"",
            b"",
            &ENTRIES,
        );
        assert_eq!(ambiguous.entry_point(Stage::Vertex), None);
        assert_eq!(ambiguous.entry_point(Stage::Fragment), Some("mainFs"));
    }

    /// Every shipped shader has a WGSL artifact, and it names the entry points
    /// the manifest recorded from the SPIR-V.
    ///
    /// The wgpu backend addresses a WGSL module by the *same* entry-point name
    /// it would use for the SPIR-V one, because `ShaderEntry` is per-stage and
    /// format-blind. Slang happens to keep the names across targets; if a
    /// future release mangled them, every wgpu pipeline would fail at creation
    /// on a machine with a GPU and nowhere else. This is that check, with no
    /// GPU.
    #[test]
    fn every_shipped_shader_has_wgsl_naming_the_same_entry_points() {
        for shader in ALL {
            let wgsl = shader
                .wgsl()
                .unwrap_or_else(|| panic!("{}: no WGSL artifact", shader.name()));
            for entry in shader.entry_points() {
                let attribute = match entry.stage() {
                    Stage::Vertex => "@vertex",
                    Stage::Fragment => "@fragment",
                    Stage::Compute => "@compute",
                };
                assert!(
                    wgsl.contains(attribute),
                    "{}: WGSL declares no {attribute} stage",
                    shader.name()
                );
                assert!(
                    wgsl.contains(&format!("fn {}(", entry.name())),
                    "{}: WGSL has no `fn {}(`, so the entry point the manifest records from \
                     the SPIR-V is not addressable in the WGSL",
                    shader.name(),
                    entry.name()
                );
            }
        }
    }

    /// Every shipped shader has an MSL artifact, and it names the entry points
    /// the manifest recorded from the SPIR-V.
    ///
    /// The same check as the WGSL one above and for the same reason: the Metal
    /// backend looks a function up **by name** in the compiled `MTLLibrary`, so
    /// a Slang release that mangled names across targets would fail every
    /// `newFunctionWithName:` on a Mac and nowhere else. This is that check,
    /// with no Mac.
    ///
    /// The attribute is checked as well as the name, because a stage-qualified
    /// function is the only kind Metal will accept into a pipeline slot — an
    /// unqualified `vertexMain` compiles and then fails at pipeline creation.
    #[test]
    fn every_shipped_shader_has_msl_naming_the_same_entry_points() {
        assert!(!ALL.is_empty(), "the crate ships no shaders at all");
        for shader in ALL {
            let msl = shader
                .msl()
                .unwrap_or_else(|| panic!("{}: no MSL artifact", shader.name()));
            for entry in shader.entry_points() {
                let attribute = match entry.stage() {
                    Stage::Vertex => "[[vertex]]",
                    Stage::Fragment => "[[fragment]]",
                    Stage::Compute => "[[kernel]]",
                };
                assert!(
                    msl.contains(attribute),
                    "{}: MSL declares no {attribute} stage",
                    shader.name()
                );
                assert!(
                    msl.contains(&format!("{}(", entry.name())),
                    "{}: MSL has no `{}(`, so the entry point the manifest records from the \
                     SPIR-V is not addressable in the MSL",
                    shader.name(),
                    entry.name()
                );
            }
        }
    }

    /// A shader with no text column reports absence, not an empty source — the
    /// distinction `crcbl_hal::ShaderModuleDesc::wgsl` and `::msl` are
    /// `Option`s for.
    #[test]
    fn a_shader_without_a_text_artifact_reports_none() {
        static BARE: [EntryPoint; 1] = [EntryPoint::new("mainVs", Stage::Vertex, b"")];
        let spirv_only = Shader::new(
            "spirv-only",
            "shaders/spirv-only.slang",
            "0000000000000000000000000000000000000000000000000000000000000000",
            b"",
            b"",
            b"",
            &BARE,
        );
        assert_eq!(spirv_only.wgsl(), None);
        assert_eq!(spirv_only.msl(), None);
        assert_eq!(spirv_only.dxil("mainVs"), None);

        // And a column that *is* present reads back, so `None` above is the
        // absence rule rather than a decode that never returns anything.
        static ENTRIES: [EntryPoint; 1] = [EntryPoint::new("mainVs", Stage::Vertex, b"DXBC-ish")];
        static PRESENT: Shader = Shader::new(
            "text-only",
            "shaders/text-only.slang",
            "0000000000000000000000000000000000000000000000000000000000000000",
            b"",
            b"@vertex fn mainVs() {}",
            b"[[vertex]] void mainVs() {}",
            &ENTRIES,
        );
        assert_eq!(PRESENT.wgsl(), Some("@vertex fn mainVs() {}"));
        assert_eq!(PRESENT.msl(), Some("[[vertex]] void mainVs() {}"));
        assert_eq!(PRESENT.dxil("mainVs"), Some(b"DXBC-ish".as_slice()));
        assert_eq!(
            PRESENT.dxil("mainPs"),
            None,
            "an entry point this shader does not have must not resolve to another \
             one's container"
        );
    }

    /// **Every shipped shader has a signed DXIL container for every entry
    /// point, at the shader model the manifest pins.**
    ///
    /// The container header is a fixed binary layout, so all of this is
    /// checkable with no Windows in the loop — which is the point, because
    /// `crcbl-dx12` compiles only on Windows and CI is its only executor. The
    /// three things asserted are the three that go wrong silently:
    ///
    /// * The **digest** is sixteen zero bytes until `libdxil.so` signs the
    ///   container. An unsigned one compiles, hashes and commits, and is then
    ///   refused by every real D3D12 driver at pipeline creation — the furthest
    ///   possible point from the mistake.
    /// * The **program kind** in the `DXIL` part says which stage the container
    ///   is for. A vertex container generated for a fragment entry point would
    ///   otherwise be caught only by `CreateGraphicsPipelineState`.
    /// * The **declared container size** must be the file's, or the artifact is
    ///   truncated.
    #[test]
    fn every_shipped_shader_has_a_signed_dxil_container_per_entry_point() {
        assert!(!ALL.is_empty(), "the crate ships no shaders at all");
        for shader in ALL {
            for entry in shader.entry_points() {
                let blob = shader
                    .dxil(entry.name())
                    .unwrap_or_else(|| panic!("{}: no DXIL for `{}`", shader.name(), entry.name()));
                let header = dxil_container(blob)
                    .unwrap_or_else(|error| panic!("{}:{}: {error}", shader.name(), entry.name()));
                assert_ne!(
                    header.digest,
                    [0u8; 16],
                    "{}:{} has an all-zero container digest, so dxc never signed it",
                    shader.name(),
                    entry.name()
                );
                assert_eq!(
                    header.program_kind,
                    dxil_program_kind(entry.stage()),
                    "{}:{} is a container for the wrong stage",
                    shader.name(),
                    entry.name()
                );
                assert_eq!(
                    header.shader_model,
                    (6, 6),
                    "{}:{} was not compiled at the pinned shader model",
                    shader.name(),
                    entry.name()
                );
            }
        }
    }

    /// What [`dxil_container`] reads out of a `DxilContainerHeader`.
    struct DxilHeader {
        digest: [u8; 16],
        /// `DXIL::ShaderKind` from the `DXIL` part's `ProgramVersion`.
        program_kind: u32,
        shader_model: (u32, u32),
    }

    /// `DXIL::ShaderKind` for a stage. Fixed by the container format, not by
    /// this crate — pixel is `0`, vertex `1`, compute `5`.
    const fn dxil_program_kind(stage: Stage) -> u32 {
        match stage {
            Stage::Fragment => 0,
            Stage::Vertex => 1,
            Stage::Compute => 5,
        }
    }

    /// Parses the fixed part of a DXBC/DXIL container header.
    ///
    /// Deliberately hand-written and deliberately tiny: this is the *format's*
    /// layout, fixed outside this repository, and the alternative is a Windows
    /// dependency in a test whose whole value is running without one.
    ///
    /// ```text
    /// 0  u32     magic "DXBC"
    /// 4  [u8;16] digest, all-zero until libdxil.so signs it
    /// 20 u16 u16 container version
    /// 24 u32     container size in bytes
    /// 28 u32     part count
    /// 32 u32[]   part offsets; each part is a fourcc, a u32 size, then data
    /// ```
    ///
    /// The `DXIL` part's data opens with a `DxilProgramHeader`, whose first
    /// word packs the shader kind and model as `kind << 16 | major << 4 | minor`.
    fn dxil_container(blob: &[u8]) -> Result<DxilHeader, String> {
        let word = |at: usize| -> Result<u32, String> {
            blob.get(at..at + 4)
                .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .ok_or_else(|| format!("the container is {} bytes, truncated at {at}", blob.len()))
        };
        if blob.get(..4) != Some(b"DXBC") {
            return Err("does not open with the DXBC container magic".to_string());
        }
        let digest: [u8; 16] = blob[4..20].try_into().expect("20 bytes were just indexed");
        let declared = word(24)? as usize;
        if declared != blob.len() {
            return Err(format!(
                "declares {declared} bytes but is {}, so it is truncated",
                blob.len()
            ));
        }
        for part in 0..word(28)? as usize {
            let offset = word(32 + part * 4)? as usize;
            if blob.get(offset..offset + 4) == Some(b"DXIL") {
                // The part header is a fourcc and a size, so the program header
                // starts eight bytes in.
                let version = word(offset + 8)?;
                return Ok(DxilHeader {
                    digest,
                    program_kind: version >> 16,
                    shader_model: ((version >> 4) & 0xF, version & 0xF),
                });
            }
        }
        Err("has no DXIL part, so it carries no bytecode".to_string())
    }

    /// The recorded hash is the drift check's whole basis, so it must actually
    /// be the hash of the file that is committed.
    #[test]
    fn the_recorded_source_hash_matches_the_source_on_disk() {
        for shader in ALL {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(shader.source());
            let bytes =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                sha256::sha256_hex(&bytes),
                shader.source_sha256(),
                "{} has drifted from its manifest entry; run \
                 crates/crcbl-shaders/tools/compile-shaders.sh",
                shader.source()
            );
        }
    }
}
