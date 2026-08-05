//! The engine's shaders: Slang sources, and the SPIR-V, WGSL and MSL compiled
//! from them.
//!
//! ```text
//! shaders/*.slang ──tools/compile-shaders.sh──┬─▶ spirv/*.spv    (committed)
//!                                             ├─▶ wgsl/*.wgsl    (committed)
//!                                             └─▶ msl/*.metal    (committed)
//!                                                    │
//!                          build.rs verifies ────────┤
//!                                                    ▼
//!               crcbl_shaders::TRIANGLE.spirv() / .wgsl() / .msl()
//!                                                    │
//!                                  ShaderModuleDesc { spirv, wgsl, msl }
//! ```
//!
//! Every artifact is handed over on every call and the backend picks:
//! `crcbl-vk` reads the SPIR-V, `crcbl-wgpu` reads the WGSL, `crcbl-mtl` reads
//! the MSL. See `crcbl_hal::shader` for why the seam is shaped that way and
//! what a caller owes it.
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
//! generated table — P14's MSL took it again, and DXIL will take it once more.
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
//! # Known gap: push constants have no WGSL spelling (`ui.wgsl`)
//!
//! WGSL has no push constants — they are a native-`wgpu` extension, absent from
//! WebGPU — and Slang's WGSL target does not say so. It lowers a
//! `[[vk::push_constant]]` block to a module-scope `var<uniform>` with **no**
//! `@group`/`@binding`, which is not valid WGSL. `wgsl/ui.wgsl` carries one:
//!
//! ```text
//! var<uniform> constants_0 : UiConstants_std430_0;
//! ```
//!
//! and `naga` rejects it — "Binding decoration is missing or not applicable" —
//! so `crcbl-wgpu` cannot create the `ui.slang` module. `mesh`, `tonemap` and
//! `triangle` use no push constants and compile.
//!
//! This is not fixable here: the artifact is a faithful translation of a source
//! that asks for something the target does not have. The fix is in
//! `crcbl-render`'s UI pass and `ui.slang` — a uniform buffer binding in place
//! of the push-constant block, which is the Tier B data-layout rule
//! `docs/plan/10-wasm-webgpu.md` states for this backend anyway. Regenerating
//! the artifacts is part of that change, not of the seam change that made the
//! WGSL reachable.
//!
//! No shader added here may use push constants until that lands, or it acquires
//! the same gap silently.
//!
//! # Nothing here knows a backend
//!
//! This crate has no dependencies at all, not even `crcbl-hal`. It hands out
//! `&[u32]`, `Option<&str>` and entry-point names, which is exactly what
//! `crcbl_hal::ShaderModuleDesc` takes field for field, and it stays usable by
//! a backend that has not been written yet. Each new artifact format is a new
//! accessor here and a new field there; neither names the other's types.

pub mod sha256;

/// The manifest format, and its parser.
///
/// Public because `build.rs` shares this exact code, and because P9's hot
/// reload will re-read the same file at runtime to key pipeline rebuilds by
/// shader hash (`docs/plan/06-assets-scenes.md`).
pub mod manifest;

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

/// One `OpEntryPoint` in a compiled module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryPoint {
    name: &'static str,
    stage: Stage,
}

impl EntryPoint {
    /// Names an entry point. Called only by the generated table.
    #[must_use]
    pub const fn new(name: &'static str, stage: Stage) -> Self {
        Self { name, stage }
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
            EntryPoint::new("shadowVs", Stage::Vertex),
            EntryPoint::new("mainVs", Stage::Vertex),
            EntryPoint::new("mainFs", Stage::Fragment),
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
        static ENTRIES: [EntryPoint; 1] = [EntryPoint::new("mainVs", Stage::Vertex)];
        let spirv_only = Shader::new(
            "spirv-only",
            "shaders/spirv-only.slang",
            "0000000000000000000000000000000000000000000000000000000000000000",
            b"",
            b"",
            b"",
            &ENTRIES,
        );
        assert_eq!(spirv_only.wgsl(), None);
        assert_eq!(spirv_only.msl(), None);

        // And a column that *is* present reads back, so `None` above is the
        // absence rule rather than a decode that never returns anything.
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
