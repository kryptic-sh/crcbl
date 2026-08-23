//! Turning a glTF path typed on a command line into an imported scene.
//!
//! Two verbs read a glTF — `crcbl lod` and `crcbl import` — and both reach the
//! importer through [`crcbl_assets::AssetSource`], which addresses a *key under
//! a root* rather than a path. Splitting a command-line path into those two,
//! and refusing a file name that is not a legal asset key, is one answer rather
//! than two, so it lives here instead of in whichever command needed it first.

use std::path::{Path, PathBuf};

use crcbl_assets::DirSource;
use crcbl_scene::{GltfScene, import_gltf};
use crcbl_store::web::canonical_key;

use crate::report::Failure;

/// Imports the glTF the command line named.
///
/// The directory becomes the source's root and the file name becomes the key.
/// The key rules are the engine's own and are checked here rather than left to
/// surface as `path escapes the storage root`, which is true and says nothing
/// about the name that was actually the problem.
///
/// # Errors
///
/// [`Failure`] if the path names no file, if that file's name is not a legal
/// asset key, or if the importer refuses the document.
pub fn open(file: &Path) -> Result<GltfScene, Failure> {
    let Some(name) = file.file_name() else {
        return Err(Failure::new(format!(
            "{} does not name a file to read",
            file.display()
        )));
    };
    let key = canonical_key(Path::new(name)).map_err(|_| {
        Failure::new(format!(
            "`{}` is not a usable asset name: a key is ASCII letters, digits, `.`, `_` and \
             `-`, because the same name has to resolve as a URL path in a browser",
            name.to_string_lossy()
        ))
    })?;
    let root = match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        // `crcbl lod stats model.glb`: the root is where the shell was run.
        _ => PathBuf::from("."),
    };

    import_gltf(&DirSource::at(root), Path::new(&key))
        .map_err(|error| Failure::new(format!("cannot import {}: {error}", file.display())))
}
