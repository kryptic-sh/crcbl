//! The asset browser: what it lists out of the document's source, what it
//! says when there is nothing to list, and its refresh.

use super::*;

use std::cell::RefCell;
use std::rc::Rc;

use crcbl::assets::AssetSource;
use crcbl::store::StorageError;

/// A source holding meshes, folders and files a mesh cannot be made of.
fn mixed() -> MemorySource {
    let mut source = MemorySource::new();
    for key in [
        "notes.txt",
        "props/crate.glb",
        "props/sign.GLTF",
        "props/textures/wood.png",
        "readme.md",
        "rocks/big/boulder.glb",
        "x.glb.bak",
    ] {
        source
            .insert(Path::new(key), b"bytes".to_vec())
            .expect("a legal key");
    }
    source
}

/// The panels over the greybox scene, its meshes read from `source`.
fn page_over(source: impl AssetSource + 'static) -> Page {
    let mut document = Document::built_in().expect("the compiled-in scene is a scene");
    document.set_assets(Box::new(source));
    let mut page = Page::over(document);
    page.idle();
    page
}

/// **The browser lists the mesh assets a mesh can be made of, and the folders
/// holding them** — no other file, and no folder of textures alone — and
/// draws a row for each.
#[test]
fn the_browser_lists_only_mesh_assets_and_the_folders_holding_them() {
    let page = page_over(mixed());
    assert_eq!(
        page.panels.listed_assets(),
        [
            "props/crate.glb",
            "props/sign.GLTF",
            "rocks/big/boulder.glb"
        ],
    );
    assert_eq!(
        page.panels.listed_folders(),
        ["props", "rocks", "rocks/big"]
    );
    assert_eq!(page.panels.assets_note(), None);
    let rows: Vec<&str> = page
        .panels
        .asset_rows()
        .into_iter()
        .map(|(_, key)| key)
        .collect();
    assert_eq!(
        rows,
        [
            "props",
            "props/crate.glb",
            "props/sign.GLTF",
            "rocks",
            "rocks/big",
            "rocks/big/boulder.glb"
        ],
        "every listed entry has a row, folders open",
    );
}

/// A source that cannot enumerate what it holds — a browser fetching by URL.
#[derive(Debug)]
struct Fetching;

impl AssetSource for Fetching {
    fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotFound(key.to_path_buf()))
    }
}

/// **A source that cannot list says so, and one holding no mesh says that**,
/// rather than either showing an empty pane.
#[test]
fn the_browser_says_why_it_lists_nothing() {
    let page = page_over(Fetching);
    assert!(page.panels.listed_assets().is_empty());
    let note = page.panels.assets_note().expect("a note");
    assert!(note.contains("cannot be listed"), "{note}");

    let mut only_text = MemorySource::new();
    only_text
        .insert(Path::new("docs/readme.md"), b"text".to_vec())
        .expect("a legal key");
    let page = page_over(only_text);
    let note = page.panels.assets_note().expect("a note");
    assert!(note.contains("No .glb or .gltf"), "{note}");
}

/// A source whose files a test adds to after the panels listed it.
#[derive(Debug, Clone, Default)]
struct Shared(Rc<RefCell<MemorySource>>);

impl AssetSource for Shared {
    fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
        self.0.borrow().read(key)
    }

    fn list(&self, dir: &Path) -> Result<Vec<crcbl::assets::AssetEntry>, StorageError> {
        self.0.borrow().list(dir)
    }
}

/// **The refresh button lists the source again**, and handing the document
/// another source lists that one: there is no watcher to say a file arrived.
#[test]
fn refresh_lists_the_source_again() {
    let shared = Shared::default();
    let mut page = page_over(shared.clone());
    assert!(page.panels.listed_assets().is_empty());

    shared
        .0
        .borrow_mut()
        .insert(Path::new("crate.glb"), b"bytes".to_vec())
        .expect("a legal key");
    page.idle();
    assert!(
        page.panels.listed_assets().is_empty(),
        "the browser walked the source on a still frame",
    );
    let refresh = page
        .panels
        .refresh_button()
        .expect("the pane drew its button");
    let at = page.centre(refresh);
    page.click(at);
    assert_eq!(page.panels.listed_assets(), ["crate.glb"]);

    page.document.set_assets(Box::new(mixed()));
    page.panels.relist_assets(&page.document);
    assert_eq!(page.panels.listed_assets().len(), 3);
}
