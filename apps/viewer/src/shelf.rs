//! The shelf: the Khronos CC0 models this sample ships with, and where each
//! host finds them.
//!
//! ```text
//!   apps/viewer/assets/shelf.sha256   ── the file list, one sha256 a line
//!        │
//!        ├─ tools/fetch-shelf.sh ──▶ apps/viewer/assets/shelf/<Name>/glTF/…
//!        │                              └─ DirSource::at(root) + key      (native)
//!        └─ web/build.sh ──▶ <site>/demos/viewer/assets/shelf/…
//!                                       └─ FetchSource + key `shelf/…`    (browser)
//! ```
//!
//! `docs/plan/sample/05-viewer.md` milestone 4's second item. The models are
//! the CC0 set that document's table names, in the `glTF/` form — a `.gltf`
//! with its `.bin` and its images beside it — because that is the arrangement
//! every exporter produces and the one [`crate::model::load`] is written for.
//! The alternative, a single `.glb`, would exercise none of the sibling-key
//! resolution the asset seam exists to do.
//!
//! # Only Suzanne is in this repository
//!
//! The whole shelf is about 138 MB, which is not history a repository carries
//! for a demo. [`DEFAULT`]'s files are committed because they are what the
//! viewer opens when nothing is asked for and what this crate's tests read with
//! no network; everything else is fetched by `tools/fetch-shelf.sh` at
//! [`UPSTREAM_COMMIT`], verified against a sha256 a file, into a directory
//! `.gitignore` keeps out. A shelf that has not been fetched is not an error:
//! the rows are still listed, and picking one says what to run.
//!
//! # The file list is one list, and it is not in this file
//!
//! `apps/viewer/assets/shelf.sha256` is `sha256sum` format — a hash, two
//! spaces, a path relative to the shelf root — and it is the only place the
//! *files* of a model are written down. The fetch script reads it to know what
//! to download and what to check; [`ShelfModel::files`] reads the same bytes
//! through [`include_str!`] to know what to ask a browser to fetch and what
//! this crate's tests look for on disk. The table below holds what a checksum
//! file cannot say: the name, the entry document, the licence and whether the
//! browser tab ships it.
//!
//! # What the browser ships, and why it is three of them
//!
//! The published site carries [`ShelfModel::in_browser`] models only — Suzanne,
//! Avocado and WaterBottle, 18.9 MB together against a stated budget of 25 MB
//! for the demo's assets. Suzanne alone is *pre-loaded*, because it is the
//! document the tab opens on and everything before the first frame is time a
//! visitor spends looking at a loading message; the other two are fetched when
//! someone picks them, which is what makes the on-demand path a thing that runs
//! rather than a thing that was written. The six models that are not on that
//! list are native-only: at 8–48 MB each they would put the demo site an order
//! of magnitude over the budget, and no browser row can name a document the
//! site does not carry.

use std::path::{Path, PathBuf};

use crate::model::{LoadError, Model};

/// The upstream repository every shelf model comes from.
pub const UPSTREAM_REPO: &str = "https://github.com/KhronosGroup/glTF-Sample-Assets";

/// The commit `tools/fetch-shelf.sh` fetches at, and the commit the licences
/// below were read at.
///
/// A commit and never a branch: a shelf pinned to `main` is a shelf whose
/// checksums stop matching the day someone upstream re-exports a model, and the
/// failure would arrive as a corrupt-download message about a download that was
/// fine. `the_fetch_script_pins_the_commit_this_table_names` is what keeps the
/// script and this constant from drifting apart.
pub const UPSTREAM_COMMIT: &str = "9429648735279342b4c32b8745f7904196607379";

/// The environment variable that moves the shelf root.
///
/// For a build that is not run out of its source tree, and for a test that
/// wants a shelf of its own. `root` is what reads it — named without a link,
/// because it is a native-only item and this constant is documented on
/// `wasm32` too, where the link would resolve to nothing.
pub const ROOT_ENV: &str = "CRCBL_SHELF";

/// Where the shelf sits under a browser demo's asset base.
///
/// `crcbl::web::ASSET_BASE` is `assets/` and this is the directory under it, so
/// a browser key is `shelf/Suzanne/glTF/Suzanne.gltf` and the URL the shim gets
/// is `assets/shelf/Suzanne/glTF/Suzanne.gltf`. `web/build.sh` is what puts the
/// files there.
pub const WEB_PREFIX: &str = "shelf";

/// One model on the shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShelfModel {
    /// What the panel row says, and the upstream model directory's name.
    pub name: &'static str,
    /// The model's directory, relative to the shelf root — `Suzanne/glTF`.
    pub dir: &'static str,
    /// The document to open inside [`ShelfModel::dir`].
    pub entry: &'static str,
    /// The licence, as read from the model's own `README.md` at
    /// [`UPSTREAM_COMMIT`].
    ///
    /// Recorded per model rather than once for the table, because provenance is
    /// a property of the file and not of the shelf: the day one of these
    /// arrives under different terms, the row is where it says so.
    pub licence: &'static str,
    /// Whether the browser tab carries this model at all — see the [module
    /// docs](self).
    pub in_browser: bool,
}

impl ShelfModel {
    /// The document's key, relative to the shelf root.
    #[must_use]
    pub fn key(&self) -> PathBuf {
        PathBuf::from(format!("{}/{}", self.dir, self.entry))
    }

    /// The document's key as a browser demo's asset source spells it.
    #[must_use]
    pub fn web_key(&self) -> PathBuf {
        PathBuf::from(format!("{WEB_PREFIX}/{}/{}", self.dir, self.entry))
    }

    /// Where this model came from: the upstream directory at
    /// [`UPSTREAM_COMMIT`].
    ///
    /// Built rather than stored, because a URL repeated per row is nine copies
    /// of one fact and eight of them would keep the old commit after a re-pin.
    #[must_use]
    pub fn source_url(&self) -> String {
        format!(
            "{UPSTREAM_REPO}/tree/{UPSTREAM_COMMIT}/Models/{}/glTF",
            self.name
        )
    }

    /// Every file this model needs, as a key relative to the shelf root.
    ///
    /// Read out of `apps/viewer/assets/shelf.sha256`, which is the one place
    /// the file list is written — see the [module docs](self).
    pub fn files(&self) -> impl Iterator<Item = &'static str> {
        let dir = self.dir;
        CHECKSUMS.lines().filter_map(move |line| {
            let path = line.split_once("  ")?.1;
            let rest = path.strip_prefix(dir)?;
            rest.starts_with('/').then_some(path)
        })
    }
}

/// The shelf, in the order the panel lists it.
///
/// Every one is CC0-1.0, read on 2026-08-30 from each model's own `README.md`
/// at [`UPSTREAM_COMMIT`]. `AntiqueCamera` is the one candidate from
/// `docs/plan/sample/05-viewer.md`'s table that is **not** here: its geometry
/// and textures are CC0, but a UX3D trademark is baked into one texture under a
/// separate licence reference whose own text says UX3D "reserves the right to
/// remove the Mark or unilaterally change the terms of use" — which is exactly
/// the obligation-to-track the plan's "CC0 first" rule exists to avoid.
pub const SHELF: [ShelfModel; 9] = [
    ShelfModel {
        name: "Suzanne",
        dir: "Suzanne/glTF",
        entry: "Suzanne.gltf",
        licence: "CC0-1.0",
        in_browser: true,
    },
    ShelfModel {
        name: "Avocado",
        dir: "Avocado/glTF",
        entry: "Avocado.gltf",
        licence: "CC0-1.0",
        in_browser: true,
    },
    ShelfModel {
        name: "WaterBottle",
        dir: "WaterBottle/glTF",
        entry: "WaterBottle.gltf",
        licence: "CC0-1.0",
        in_browser: true,
    },
    ShelfModel {
        name: "BoomBox",
        dir: "BoomBox/glTF",
        entry: "BoomBox.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
    ShelfModel {
        name: "Corset",
        dir: "Corset/glTF",
        entry: "Corset.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
    ShelfModel {
        name: "Lantern",
        dir: "Lantern/glTF",
        entry: "Lantern.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
    ShelfModel {
        name: "BarramundiFish",
        dir: "BarramundiFish/glTF",
        entry: "BarramundiFish.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
    ShelfModel {
        name: "SciFiHelmet",
        dir: "SciFiHelmet/glTF",
        entry: "SciFiHelmet.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
    ShelfModel {
        name: "FlightHelmet",
        dir: "FlightHelmet/glTF",
        entry: "FlightHelmet.gltf",
        licence: "CC0-1.0",
        in_browser: false,
    },
];

/// The shelf entry the viewer opens when nothing is asked for.
///
/// Suzanne, on both hosts, which is what
/// `docs/plan/sample/05-viewer.md` milestone 4 says in as many words. It is
/// also the one model committed to this repository, so this index is the one
/// the tests can rely on being on disk.
pub const DEFAULT: usize = 0;

/// The file list and its hashes, as `tools/fetch-shelf.sh` reads them.
const CHECKSUMS: &str = include_str!("../assets/shelf.sha256");

/// The shelf entry `index` names.
#[must_use]
pub fn model(index: usize) -> Option<&'static ShelfModel> {
    SHELF.get(index)
}

// ---------------------------------------------------------------------------
// Native: a directory
// ---------------------------------------------------------------------------

/// Where the shelf is on this machine.
///
/// [`ROOT_ENV`] when it is set, and `apps/viewer/assets/shelf` in this crate's
/// own source tree otherwise. The second is a compile-time path and is
/// deliberately the *fallback*: it is what makes `cargo run -p viewer` and this
/// crate's tests find the shelf with nothing configured, and it is a directory
/// that will not exist on a machine the binary was copied to — which is what
/// [`ROOT_ENV`] is for, and why a shelf that is not there is a listed row that
/// says so rather than a start-up failure.
///
/// There is no lookup beside the executable. Nothing in this workspace packages
/// a binary with an asset directory today, so a `current_exe()` candidate would
/// be a path no build produces.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn root() -> PathBuf {
    match std::env::var_os(ROOT_ENV) {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/shelf"),
    }
}

/// Where the shelf entry `index` is on this machine.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn path_of(index: usize) -> PathBuf {
    root().join(SHELF[index.min(SHELF.len() - 1)].key())
}

/// The path the viewer opens and watches when no model was named.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn default_path() -> PathBuf {
    path_of(DEFAULT)
}

/// Opens shelf entry `index`, and says which path a re-export watch should
/// follow.
///
/// **Through [`crate::model::load_from`] over a [`DirSource`] rooted at the
/// shelf**, not at the model's own directory: a glTF `uri` resolves beside the
/// *document's key*, so one source rooted once serves every model on the shelf
/// and a key that tried to leave it is refused by the same rule that refuses
/// one on the command line.
///
/// [`DirSource`]: crcbl::assets::DirSource
///
/// # Errors
///
/// [`LoadError`] exactly as a path on the command line produces one — including
/// the [`NotFound`](crcbl::store::StorageError::NotFound) a shelf that has not
/// been fetched gives, which names the file that is missing.
#[cfg(not(target_arch = "wasm32"))]
pub fn load(index: usize) -> Result<(Model, PathBuf), LoadError> {
    open_at(&root(), index)
}

/// [`load`] out of a shelf root the caller names.
///
/// The seam [`ROOT_ENV`] exists for, reachable without setting it: a test that
/// wants an empty shelf, or a second shelf, says so here rather than writing to
/// the process environment — which `cargo test`'s shared process would race
/// against every other test in this file.
///
/// # Errors
///
/// [`LoadError`], exactly as [`load`] produces one.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_at(root: &Path, index: usize) -> Result<(Model, PathBuf), LoadError> {
    let entry = model(index).ok_or_else(|| LoadError::NotAFile(PathBuf::from("shelf")))?;
    let path = root.join(entry.key());
    let source = crcbl::assets::DirSource::at(root.to_path_buf());
    let model = crate::model::load_from(&source, &entry.key(), &path)?;
    Ok((model, path))
}

// ---------------------------------------------------------------------------
// Browser: a fetch source
// ---------------------------------------------------------------------------

/// The page's asset source, as an [`AssetSource`](crcbl::assets::AssetSource).
///
/// `FetchSource` is a [`StorageSource`](crcbl::store::StorageSource) and the
/// two traits are deliberately not one — `crcbl_assets::source`'s module docs
/// say why — so this is the four-line adapter those docs describe, written
/// where it finally has a consumer.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
struct FetchAssets(std::rc::Rc<crcbl::store::web::FetchSource>);

#[cfg(target_arch = "wasm32")]
impl crcbl::assets::AssetSource for FetchAssets {
    fn read(&self, key: &Path) -> Result<Vec<u8>, crcbl::store::StorageError> {
        crcbl::store::StorageSource::read(&*self.0, key)
    }
}

/// The key the viewer opens when no model was named, as a page spells it.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn default_path() -> PathBuf {
    SHELF[DEFAULT].web_key()
}

/// Opens shelf entry `index` out of the page's asset source.
///
/// **Every file first, then the document.** A browser source answers
/// [`Pending`](crcbl::store::StorageError::Pending) for a key it has started
/// fetching, and `crcbl::scene::import_gltf` treats an image it cannot read as
/// a *skip* rather than a failure — so loading while the textures were still in
/// flight would succeed, with the model on screen untextured and a skip list
/// blaming the document. Asking every key's [`FetchState`] before the import
/// runs is what makes the retry loop converge on the whole model instead.
///
/// [`FetchState`]: crcbl::store::web::FetchState
///
/// # Errors
///
/// [`LoadError::Storage`] carrying `Pending` while anything is still in flight
/// — the caller is expected to try again next frame — and the loader's own
/// errors once everything is resident.
#[cfg(target_arch = "wasm32")]
pub fn load(index: usize) -> Result<(Model, PathBuf), LoadError> {
    use crcbl::store::StorageError;
    use crcbl::store::web::FetchState;

    let entry = model(index).ok_or_else(|| LoadError::NotAFile(PathBuf::from("shelf")))?;
    let key = entry.web_key();
    let Some(source) = crate::web::asset_source() else {
        return Err(LoadError::Storage {
            path: key.clone(),
            why: StorageError::NotFound(key),
        });
    };

    let mut waiting = false;
    for file in entry.files() {
        let file = PathBuf::from(format!("{WEB_PREFIX}/{file}"));
        match source.state(&file) {
            FetchState::Ready => {}
            FetchState::Failed { status } => {
                return Err(LoadError::Storage {
                    path: file.clone(),
                    why: StorageError::Io(std::io::Error::other(format!(
                        "the page could not fetch it (HTTP {status})"
                    ))),
                });
            }
            FetchState::Unknown | FetchState::Queued | FetchState::InFlight => {
                // Idempotent, and cheap on a key already queued or in flight —
                // which is what makes calling this every frame the way the
                // fetch source is documented to be driven.
                if let Err(error) = source.request(&file) {
                    return Err(LoadError::Storage {
                        path: file,
                        why: error,
                    });
                }
                waiting = true;
            }
        }
    }
    if waiting {
        return Err(LoadError::Storage {
            path: key.clone(),
            why: StorageError::Pending(key),
        });
    }

    let source = FetchAssets(source);
    let model = crate::model::load_from(&source, &key, &key)?;
    Ok((model, key))
}

/// Whether `error` is the shelf saying "not yet", rather than a document that
/// will not load.
///
/// The browser's on-demand fetch reports it every frame until the last file
/// lands; nothing else in this crate produces it, because
/// [`DirSource`](crcbl::assets::DirSource) and
/// [`MemorySource`](crcbl::assets::MemorySource) both answer from bytes they
/// already have.
#[must_use]
pub fn is_pending(error: &LoadError) -> bool {
    matches!(
        error,
        LoadError::Storage {
            why: crcbl::store::StorageError::Pending(_),
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row names a document inside its own directory, and no two rows
    /// name the same model.
    #[test]
    fn the_table_is_well_formed() {
        for entry in &SHELF {
            assert_eq!(
                entry.dir,
                format!("{}/glTF", entry.name),
                "{}'s directory is not the upstream one",
                entry.name,
            );
            assert!(
                entry.entry.ends_with(".gltf"),
                "{} opens {}, which is not a .gltf",
                entry.name,
                entry.entry,
            );
            assert_eq!(entry.licence, "CC0-1.0", "{} is not CC0", entry.name);
            assert!(
                entry.source_url().contains(UPSTREAM_COMMIT),
                "{}'s source url does not name the pinned commit",
                entry.name,
            );
        }
        let mut names: Vec<&str> = SHELF.iter().map(|entry| entry.name).collect();
        names.sort_unstable();
        let listed = names.len();
        names.dedup();
        assert_eq!(names.len(), listed, "a model is on the shelf twice");
        assert_eq!(SHELF[DEFAULT].name, "Suzanne", "the default moved");
    }

    /// **Every row's file list is non-empty and includes its own document**,
    /// which is what stands between a renamed directory and a shelf row that
    /// silently asks a browser to fetch nothing at all before deciding the
    /// model is ready.
    #[test]
    fn every_row_has_files_and_one_of_them_is_the_document() {
        for entry in &SHELF {
            let files: Vec<&str> = entry.files().collect();
            assert!(
                files.len() >= 2,
                "{} lists {} file(s); a .gltf with no buffer beside it is not \
                 one of these models",
                entry.name,
                files.len(),
            );
            let key = entry.key();
            let key = key.to_str().expect("the key is utf-8");
            assert!(
                files.contains(&key),
                "{key} is not in the checksum file, so nothing would fetch it",
            );
            for file in files {
                assert!(
                    file.starts_with(entry.dir),
                    "{file} is not one of {}'s files",
                    entry.name,
                );
            }
        }
    }

    /// The checksum file every other check reads is `sha256sum` format and
    /// covers only the models on the shelf — a stale row for a model that was
    /// dropped would otherwise be downloaded for ever by the fetch script and
    /// noticed by nobody.
    #[test]
    fn the_checksum_file_is_sha256sum_format_and_covers_the_shelf_exactly() {
        let mut counted = 0;
        for line in CHECKSUMS.lines() {
            let (hash, path) = line.split_once("  ").unwrap_or_else(|| {
                panic!("{line:?} is not a `sha256sum` line");
            });
            assert_eq!(hash.len(), 64, "{hash:?} is not a sha256");
            assert!(
                hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{hash:?} is not hexadecimal",
            );
            assert!(
                SHELF.iter().any(|entry| path.starts_with(entry.dir)),
                "{path} belongs to no model on the shelf",
            );
            counted += 1;
        }
        let listed: usize = SHELF.iter().map(|entry| entry.files().count()).sum();
        assert_eq!(counted, listed, "a checksum line reached no shelf row");
    }

    /// **The script and this table agree on the pin and on the browser
    /// subset.** They are two files in two languages and each fact has to be
    /// one: a re-pin that moved only the script would fetch models these
    /// licences were never read at, and a browser subset that disagreed would
    /// put a row in the tab naming a model the site does not carry.
    #[test]
    fn the_fetch_script_pins_the_commit_and_the_subset_this_table_names() {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fetch-shelf.sh");
        let script = std::fs::read_to_string(&script)
            .unwrap_or_else(|why| panic!("{} could not be read: {why}", script.display()));
        assert!(
            script.contains(&format!("COMMIT={UPSTREAM_COMMIT}")),
            "tools/fetch-shelf.sh does not pin {UPSTREAM_COMMIT}",
        );
        let subset = script
            .lines()
            .find_map(|line| line.strip_prefix("WEB_MODELS=\""))
            .and_then(|rest| rest.strip_suffix('"'))
            .expect("tools/fetch-shelf.sh declares WEB_MODELS on one line");
        let wanted: Vec<&str> = SHELF
            .iter()
            .filter(|entry| entry.in_browser)
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            subset.split_whitespace().collect::<Vec<_>>(),
            wanted,
            "tools/fetch-shelf.sh copies a different set into the demo site",
        );
    }

    /// **Every file of every row is on disk**, which is the check that a
    /// fetched shelf is a whole one.
    ///
    /// Skipped — loudly, by name — when the shelf has not been fetched, because
    /// only Suzanne is committed and the rest is 138 MB. `tools/fetch-shelf.sh`
    /// is what makes it run; CI's `test (linux)` job calls it.
    #[test]
    fn every_shelf_file_is_on_disk_once_the_shelf_is_fetched() {
        let root = root();
        let fetched: Vec<&ShelfModel> = SHELF
            .iter()
            .filter(|entry| root.join(entry.key()).exists())
            .collect();
        assert!(
            fetched.iter().any(|entry| entry.name == "Suzanne"),
            "{} has no Suzanne, and Suzanne is committed to this repository — \
             either the shelf root is wrong or the files were deleted",
            root.display(),
        );
        for entry in &fetched {
            for file in entry.files() {
                let path = root.join(file);
                assert!(
                    path.exists(),
                    "{} is missing; run tools/fetch-shelf.sh",
                    path.display(),
                );
            }
        }
        // **Announced rather than asserted, and only for the models this
        // repository does not carry.** Everything above ran: Suzanne is
        // committed, so the committed half of the shelf is checked on every
        // machine. The rest is 138 MB that a developer is not made to download
        // to run the tests, and the line below is what stops "the shelf was
        // never there" reading as "the shelf is whole". CI's `test (linux)` job
        // fetches it, which is where this test asks the real question.
        if fetched.len() != SHELF.len() {
            eprintln!(
                "SKIPPED {} of {} shelf models: they are not in {} — run \
                 tools/fetch-shelf.sh to check them",
                SHELF.len() - fetched.len(),
                SHELF.len(),
                root.display(),
            );
        }
    }

    /// The committed default opens, off the real file, with no network and no
    /// fetch script.
    #[test]
    fn the_default_model_loads_from_the_committed_shelf() {
        let (model, path) = load(DEFAULT).expect("Suzanne is committed to this repository");
        assert_eq!(model.key, SHELF[DEFAULT].key());
        assert_eq!(path, default_path());
        assert_eq!(
            model.render.instances.len(),
            1,
            "Suzanne is one mesh in one node",
        );
        assert!(
            model.bounds.half_extent().max_element() > 0.0,
            "a document with no box cannot be framed",
        );
    }

    /// A shelf root with nothing in it is a load error naming the file, not a
    /// panic — the case a binary run outside its source tree is in.
    ///
    /// Through [`open_at`] rather than by pointing [`ROOT_ENV`] at the
    /// directory: `cargo test` runs this file's tests as threads of one
    /// process, so writing to the environment here would be read by the test
    /// above while it was loading the real shelf.
    #[test]
    fn an_unfetched_shelf_reports_the_missing_document() {
        let empty = tempfile::tempdir().expect("a temporary directory");
        let error = open_at(empty.path(), DEFAULT).expect_err("an empty shelf holds no document");
        let message = error.to_string();
        assert!(
            message.contains("Suzanne.gltf"),
            "the message does not name the missing document: {message}",
        );
        assert!(!is_pending(&error), "a directory read is never pending");
    }
}
