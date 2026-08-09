//! **Every committed `wgsl/*.wgsl` parses and validates with naga.**
//!
//! `docs/plan/02-vulkan-backend.md`'s fourth shader portability rule — "validate
//! all four artifacts, not one". `tools/compile-shaders.sh` runs `spirv-val`
//! over every fresh SPIR-V; until this file nothing looked at the WGSL at all,
//! and `wgsl/ui.wgsl` shipped for months declaring `var<uniform>` with no
//! binding decoration. naga refuses that module outright, so `crcbl-wgpu` could
//! never have created a UI pipeline from it — on any machine, with any adapter.
//! It was fixed by accident of an unrelated change and nothing would have caught
//! it either way. [`the_artifact_that_shipped_without_a_binding_is_refused`]
//! keeps the exact artifact around so the failure path stays exercised.
//!
//! naga is the right validator because it is the one `crcbl-wgpu` will actually
//! run: `wgpu` compiles WGSL through it on every native backend, so a module
//! naga rejects is a pipeline that fails to create.
//!
//! # naga accepting a module is not Dawn accepting it
//!
//! This check narrows the gap and does not close it. **Dawn enforces WGSL's
//! uniformity rule where naga does not**, and that is exactly how the UI
//! shader's non-uniform `textureSample` — sampling inside a branch on a varying
//! — passed every gate here and drew a black canvas in the browser. The web
//! target's validator is the browser's, and it is stricter than this. Rule 5's
//! differential render gate is what covers the rest; reading is what this does.
//!
//! It is an integration test rather than a `#[cfg(test)]` module in `src/`
//! because naga is a dev-dependency: the library itself still has none, which is
//! what lets a backend that has not been written yet depend on this crate for
//! bytes. See `Cargo.toml`.

use std::path::{Path, PathBuf};

/// Parses and validates one WGSL module, or renders what naga refused.
///
/// `emit_to_string` rather than `Display`: naga's own error text is one line
/// naming a handle, and the annotated form is the one that names the
/// declaration and the line it is on.
fn validate(source: &str) -> Result<(), String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map(|_| ())
    .map_err(|error| error.emit_to_string(source))
}

/// Every `wgsl/*.wgsl` this crate ships, by path.
///
/// The directory rather than the manifest, exactly as `declaration_order.rs`
/// sweeps `shaders/`, so an artifact present and unrecorded is still validated —
/// and [`the_sweep_covers_every_shader_that_declares_wgsl`] then cross-checks
/// the two, because a sweep that read the wrong directory would find nothing and
/// pass everything.
fn wgsl_artifacts() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("wgsl");
    let mut artifacts: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
                .path()
        })
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wgsl")
        })
        .collect();
    artifacts.sort();
    artifacts
}

/// **The rule.** Every committed WGSL artifact parses and validates.
///
/// Falsified permanently by
/// [`the_artifact_that_shipped_without_a_binding_is_refused`], which runs the
/// same [`validate`] over the module that really shipped broken.
#[test]
fn every_committed_wgsl_artifact_validates() {
    let artifacts = wgsl_artifacts();
    assert!(
        !artifacts.is_empty(),
        "the sweep found no wgsl/*.wgsl at all, so it validated nothing"
    );

    for path in &artifacts {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if let Err(error) = validate(&source) {
            panic!("{}: naga refuses this artifact:\n{error}", path.display());
        }
    }
}

/// The sweep validates exactly the artifacts the manifest says exist.
///
/// Both directions, for the reason `declaration_order.rs` checks both: a file in
/// `wgsl/` that no record names is one nothing regenerates, and a record whose
/// `wgsl` column names a file that is not there is a path scheme that has come
/// apart. Either way the sweep above would be reading a different set than the
/// engine embeds.
#[test]
fn the_sweep_covers_every_shader_that_declares_wgsl() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(root.join("spirv/manifest.txt"))
        .expect("the manifest is committed");
    let manifest = crcbl_shaders::manifest::parse_manifest(&text).expect("the manifest parses");

    let mut declared: Vec<PathBuf> = manifest
        .shaders
        .iter()
        .filter(|record| record.targets.iter().any(|target| target == "wgsl"))
        .map(|record| root.join(&record.wgsl))
        .collect();
    declared.sort();
    assert!(
        !declared.is_empty(),
        "no shader in the manifest declares `wgsl`, so the comparison below is vacuous"
    );
    assert_eq!(
        wgsl_artifacts(),
        declared,
        "the wgsl/ directory and the manifest's `wgsl` targets name different artifacts"
    );
}

/// The artifact that shipped broken is refused — so the sweep above is a check
/// and not a validator that accepts anything.
///
/// The fixture is `wgsl/ui.wgsl` byte for byte as it stood before
/// `refactor(shaders): one ui.slang, no tier permutation` regenerated it: Slang
/// emitted `var<uniform> constants_0` with no `@binding`/`@group`, which WGSL
/// requires on every module-scope resource. The vertex stage reads it, so the
/// whole module is unusable — and it is kept here rather than reduced to a
/// minimal repro so that what this test proves is that the real artifact would
/// have been caught.
#[test]
fn the_artifact_that_shipped_without_a_binding_is_refused() {
    let source = include_str!("artifacts/ui-without-a-binding-decoration.wgsl");
    let error = validate(source).expect_err("naga must refuse an undecorated `var<uniform>`");
    assert!(
        error.contains("Binding decoration is missing or not applicable"),
        "naga refused it for a different reason than the missing binding:\n{error}"
    );
}
