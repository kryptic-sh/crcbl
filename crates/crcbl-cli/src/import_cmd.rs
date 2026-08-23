//! `crcbl import` — run the glTF importer standalone and report what came out.
//!
//! `docs/plan/11-cli-headless.md` asks for "run the asset import pipeline
//! standalone; report what was imported/skipped", and this is the reporting
//! half of it: a document goes through [`crcbl_scene::import_gltf`] and the
//! counts a person wants after a bake come back — meshes, the primitives across
//! them, materials, images, nodes and instances.
//!
//! # What this verb does not take
//!
//! The topic sketches `crcbl import <gltf> [--out <dir>]`. **`--out` is not
//! built and is refused by name.** There is nothing for it to write: the
//! importer produces an in-memory [`crcbl_scene::GltfScene`], this tree has no
//! on-disk scene format — the RON scene directory is still an open decision in
//! `docs/backlog.md` — and no binary scene container. An `--out` that wrote
//! nothing, or that invented a format on the spot, would be worse than the
//! refusal. [`crate::args::IMPORT_USAGE`] says so where a user reads it.
//!
//! # The skipped half rides on the logger
//!
//! `crates/crcbl-scene/src/gltf_import.rs` already warns, through
//! `crcbl_core::log`, for every unsupported extension, every image whose URI
//! will not resolve and every primitive that is not a triangle list. So this
//! verb installs the stderr logger and lets those lines reach the terminal
//! during the import, rather than capturing them into a skip list of its own: a
//! second mechanism for something the engine already reports would go stale the
//! first time the importer learned to warn about something new.
//!
//! Installing the logger also prints its own start-up banner to stderr. That is
//! the logger's contract, not this verb's output — everything a consumer reads,
//! human line or `--json` object, is on stdout.

use std::fmt::Write as _;

use crcbl_scene::{GltfMesh, GltfScene};

use crate::args::ImportArgs;
use crate::gltf_open::open;
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs `crcbl import`.
///
/// # Errors
///
/// [`Failure`] if the path names no readable asset, or if the importer refuses
/// the document.
pub fn run(args: &ImportArgs) -> Result<Outcome, Failure> {
    // Before the import, so the importer's own warnings about what it skipped
    // are on the terminal by the time the counts are. See the module docs.
    crcbl::core::log::init_logging();

    let scene = open(&args.file)?;
    let counts = Counts::of(&scene);

    let mut human = format!("imported {}\n ", args.file.display());
    for (name, value) in counts.fields() {
        let _ = write!(human, " {name}:{value}");
    }

    let mut json = vec![("path", Json::string(args.file.display().to_string()))];
    json.extend(
        counts
            .fields()
            .map(|(name, value)| (name, Json::Number(value as i64))),
    );

    Ok(Outcome { human, json })
}

/// What one imported document holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counts {
    meshes: usize,
    /// Summed across every mesh: one glTF mesh is a list of primitives, and the
    /// primitive is the thing that becomes a draw.
    primitives: usize,
    materials: usize,
    images: usize,
    /// Every entry of the document's `nodes` array, including the ones no scene
    /// reaches — which is what [`GltfScene::nodes`] holds.
    nodes: usize,
    /// One per node that draws a mesh, with its world transform composed. Fewer
    /// than `nodes` in any document with a node that draws nothing.
    instances: usize,
}

impl Counts {
    fn of(scene: &GltfScene) -> Self {
        Self {
            meshes: scene.meshes().len(),
            primitives: scene
                .meshes()
                .iter()
                .map(|mesh| GltfMesh::primitives(mesh).len())
                .sum(),
            materials: scene.materials().len(),
            images: scene.images().len(),
            nodes: scene.nodes().len(),
            instances: scene.instances().len(),
        }
    }

    /// The counts in report order, named.
    ///
    /// One list rather than two: the human line and the `--json` object are
    /// both rendered from this, so neither can carry a number the other does
    /// not.
    fn fields(self) -> [(&'static str, usize); 6] {
        [
            ("meshes", self.meshes),
            ("primitives", self.primitives),
            ("materials", self.materials),
            ("images", self.images),
            ("nodes", self.nodes),
            ("instances", self.instances),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// A directory of this test binary's own, named so two tests running at
    /// once cannot share one.
    fn scratch(what: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "crcbl-import-unit-{what}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        directory
    }

    /// Writes a document whose six counts are **all different**, so an assertion
    /// on it cannot pass by reading the wrong field.
    ///
    /// meshes 2, primitives 3, materials 4, images 1, nodes 6, instances 5:
    /// mesh 1 carries two primitives, node `spare` draws nothing, and the image
    /// is a sibling file the importer reads through the same source the document
    /// came from.
    ///
    /// Written as glTF's *text* form with a little-endian float dump beside it,
    /// which is `crates/crcbl-cli/tests/cli.rs`'s reason too: a `.glb` checked
    /// into the repository is a blob nobody reviewing a change could read, and
    /// `crcbl-scene`'s own `gltf_fixture` is `#[cfg(test)]` and invisible from
    /// here.
    fn write_fixture(directory: &Path) -> PathBuf {
        // One triangle, shared by every primitive: this exercises the counting,
        // not the geometry.
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices: [u32; 3] = [0, 1, 2];
        let mut bin = Vec::new();
        for position in positions {
            for component in position {
                bin.extend_from_slice(&component.to_le_bytes());
            }
        }
        let indices_at = bin.len();
        for index in indices {
            bin.extend_from_slice(&index.to_le_bytes());
        }

        let primitive = r#"{"attributes":{"POSITION":0},"indices":1}"#;
        let document = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,
"scenes":[{{"nodes":[0,1,2,3,4,5]}}],
"nodes":[{{"name":"hull","mesh":0}},{{"name":"wing_a","mesh":1}},
{{"name":"wing_b","mesh":1}},{{"name":"wing_c","mesh":0}},
{{"name":"wing_d","mesh":1}},{{"name":"spare"}}],
"meshes":[{{"name":"hull","primitives":[{primitive}]}},
{{"name":"wing","primitives":[{primitive},{primitive}]}}],
"materials":[{{"name":"paint"}},{{"name":"glass"}},{{"name":"rubber"}},{{"name":"chrome"}}],
"images":[{{"name":"decal","uri":"decal.png"}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},
{{"bufferView":1,"componentType":5125,"count":3,"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{indices_at}}},
{{"buffer":0,"byteOffset":{indices_at},"byteLength":{}}}],
"buffers":[{{"byteLength":{},"uri":"parts.bin"}}]}}"#,
            bin.len() - indices_at,
            bin.len(),
        );

        let path = directory.join("parts.gltf");
        std::fs::write(&path, document).expect("the fixture document is written");
        std::fs::write(directory.join("parts.bin"), bin).expect("the fixture buffer");
        // Never decoded — the importer carries an image's bytes still encoded —
        // so any content will do.
        std::fs::write(directory.join("decal.png"), b"not really a png").expect("the image");
        path
    }

    /// Every count comes from its own field of the imported document. All six
    /// differ, so a report that read the wrong one — or hardcoded any of them —
    /// cannot agree with this.
    #[test]
    fn the_counts_are_the_document_s_own() {
        let directory = scratch("counts");
        let file = write_fixture(&directory);

        let scene = open(&file).expect("the fixture imports");
        assert_eq!(
            Counts::of(&scene),
            Counts {
                meshes: 2,
                primitives: 3,
                materials: 4,
                images: 1,
                nodes: 6,
                instances: 5,
            }
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The human line and the `--json` object are one list rendered twice, and
    /// this is what says so: every name appears in both, against the same
    /// number.
    #[test]
    fn the_json_fields_mirror_the_human_line() {
        let directory = scratch("mirror");
        let file = write_fixture(&directory);

        let outcome = run(&ImportArgs {
            file: file.clone(),
            json: true,
        })
        .expect("the fixture imports");

        assert!(
            outcome
                .human
                .starts_with(&format!("imported {}", file.display())),
            "{}",
            outcome.human
        );
        assert_eq!(
            outcome.json[0],
            ("path", Json::string(file.display().to_string()))
        );
        for (name, value) in Counts::of(&open(&file).expect("the fixture imports")).fields() {
            assert!(
                outcome.human.contains(&format!(" {name}:{value}")),
                "the human line is missing {name}:{value}: {}",
                outcome.human
            );
            assert!(
                outcome.json.contains(&(name, Json::Number(value as i64))),
                "the JSON object is missing {name}: {:?}",
                outcome.json
            );
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// An image the document names and the directory does not have is a *skip*,
    /// not a failure: the import succeeds, the image is still counted, and the
    /// reason went to the logger this verb installs. See the module docs.
    #[test]
    fn an_unresolvable_image_is_skipped_rather_than_fatal() {
        let directory = scratch("skip");
        let path = directory.join("absent.gltf");
        std::fs::write(
            &path,
            r#"{"asset":{"version":"2.0"},"images":[{"name":"decal","uri":"decal.png"}]}"#,
        )
        .expect("the document is written");

        let outcome = run(&ImportArgs {
            file: path,
            json: false,
        })
        .expect("a missing image does not fail the import");
        assert!(outcome.human.contains("images:1"), "{}", outcome.human);
        assert!(outcome.human.contains("meshes:0"), "{}", outcome.human);

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A file that is not glTF at all fails, naming the file — the exit-1 half
    /// of the contract.
    #[test]
    fn a_file_that_is_not_gltf_fails_naming_it() {
        let directory = scratch("garbage");
        let path = directory.join("notes.gltf");
        std::fs::write(&path, "this is not a glTF document").expect("the file is written");

        let failure = run(&ImportArgs {
            file: path,
            json: false,
        })
        .expect_err("prose is not a document");
        assert!(
            failure.message.contains("notes.gltf"),
            "{}",
            failure.message
        );
        assert_eq!(failure.code, crate::report::EXIT_FAILED);

        let _ = std::fs::remove_dir_all(&directory);
    }
}
