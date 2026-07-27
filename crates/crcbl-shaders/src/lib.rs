//! The engine's shaders: Slang sources, and the SPIR-V compiled from them.
//!
//! ```text
//! shaders/*.slang ──tools/compile-shaders.sh──▶ spirv/*.spv  (committed)
//!                                                    │
//!                          build.rs verifies ────────┤
//!                                                    ▼
//!                                        crcbl_shaders::TRIANGLE.spirv()
//!                                                    │
//!                                          ShaderModuleDesc { spirv }
//! ```
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
//! without changing this crate's shape. The same is true of P5's WGSL and P14's
//! MSL/DXIL — each is another artifact column in the manifest and another
//! `include_bytes!` in the generated table.
//!
//! What is *not* here is `03-gpu-driven-rendering.md`'s tier permutation axis.
//! The triangle does not vary by tier, and a permutation system with one
//! permutation would be a guess at the shape `37-materials.md` owns.
//!
//! # Nothing here knows a backend
//!
//! This crate has no dependencies at all, not even `crcbl-hal`. It hands out
//! `&[u32]` and entry-point names, which is exactly what
//! `crcbl_hal::ShaderModuleDesc` takes, and
//! it stays usable by a backend that has not been written yet.

pub mod sha256;

/// The manifest format, and its parser.
///
/// Public because `build.rs` shares this exact code, and because P9's hot
/// reload will re-read the same file at runtime to key pipeline rebuilds by
/// shader hash (`docs/plan/06-assets-scenes.md`).
pub mod manifest;

/// The geometry `triangle.slang` pulls, in the layout that shader declares.
pub mod triangle;

use std::sync::OnceLock;

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
    bytes: &'static [u8],
    entry_points: &'static [EntryPoint],
    /// The byte stream decoded into words, once.
    ///
    /// Decoding rather than transmuting: `include_bytes!` yields a `&[u8]` with
    /// no alignment guarantee, and reinterpreting it as `&[u32]` is exactly the
    /// unaligned-read unsoundness that would earn this crate its first `unsafe`
    /// block. A shader is a few kilobytes and is decoded once per process, so
    /// the copy is not worth reasoning about.
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
        bytes: &'static [u8],
        entry_points: &'static [EntryPoint],
    ) -> Self {
        Self {
            name,
            source,
            source_sha256,
            bytes,
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
    #[must_use]
    pub fn spirv(&self) -> &[u32] {
        self.words.get_or_init(|| {
            self.bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        })
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
                shader.bytes.len(),
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
        static AMBIGUOUS: Shader = Shader::new(
            "ambiguous",
            "shaders/ambiguous.slang",
            "0000000000000000000000000000000000000000000000000000000000000000",
            &[],
            &[
                EntryPoint::new("shadowVs", Stage::Vertex),
                EntryPoint::new("mainVs", Stage::Vertex),
                EntryPoint::new("mainFs", Stage::Fragment),
            ],
        );
        assert_eq!(AMBIGUOUS.entry_point(Stage::Vertex), None);
        assert_eq!(AMBIGUOUS.entry_point(Stage::Fragment), Some("mainFs"));
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
