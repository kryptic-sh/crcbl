//! The asset browser pane: the mesh assets the document's source lists, its
//! folders as branches.
//!
//! **What is listed is what a mesh can be made of.** The pane walks
//! [`AssetSource::list`] from the source's root and keeps an asset only where
//! [`is_mesh_asset`] admits its key — the `.glb` and `.gltf` files the glTF
//! importer reads, under a key a [`crcbl::scene_mesh::Mesh`] row may hold — so
//! nothing offered here is refused when it is chosen. A folder is listed when
//! something under it is, so a texture directory beside the models is not a
//! branch that opens onto nothing. Whether a listed file is a glTF the importer
//! accepts is known only once it is imported: one that is not is spawned as
//! the placeholder, and its problem names it.
//!
//! **Listed once, not every frame.** A directory walk is IO, so the tree is
//! read when the panels open and again when the pane's refresh button is
//! pressed or a caller hands the document another source
//! ([`super::Panels::relist_assets`]); there is no watcher to say a file
//! appeared. A source that cannot list — a browser fetching by URL answers
//! [`StorageError::Unsupported`](crcbl::store::StorageError::Unsupported) —
//! says so in the pane rather than showing an empty one.
//!
//! **A row is where a drop starts.** A press on a mesh asset's row is the
//! start of a drag into the viewport ([`super::Panels::asset_at`]), and accept
//! — Enter — on the focused one asks for that asset at the view's centre
//! ([`super::PanelFrame::spawn`]); `crate::app` carries both out. A folder's
//! row is neither.
//!
//! The walk stops at [`MAX_DEPTH`] folders down and [`MAX_LISTED`] entries
//! read, and says so, because an asset root pointed at a home directory is a
//! mistake a person makes once and should not cost them the editor.

use std::path::Path;

use crcbl::assets::AssetSource;
use crcbl::math::Vec2;
use crcbl::scene_mesh::is_mesh_asset;
use crcbl::ui::tree::{NodeKey, OutlinerId, OutlinerOptions, OutlinerState, Ui};

/// How many folders down the walk goes from the source's root.
pub const MAX_DEPTH: usize = 8;

/// How many listed entries the walk reads before it stops.
pub const MAX_LISTED: usize = 4096;

/// One listed entry: a folder or a mesh asset.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// The full key from the source's root.
    key: String,
    /// Whether it is a folder, whose children follow it.
    folder: bool,
    /// The entries directly inside a folder, as indices into
    /// [`Listing::entries`]; empty for an asset.
    children: Vec<usize>,
}

/// What a walk of the source found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Listing {
    entries: Vec<Entry>,
    /// The entries at the root, as indices into `entries`.
    roots: Vec<usize>,
    /// What the pane says in place of, or under, the tree: why nothing could
    /// be listed, that nothing was, or that the walk stopped short.
    note: Option<String>,
}

/// The pane's state between frames: the listing, the outliner's expansion and
/// selection, and the rows the last frame built.
#[derive(Debug, Default)]
pub(super) struct Browser {
    listing: Listing,
    outliner: OutlinerState,
    /// Each row the last frame built, with the item it stands for: what a
    /// pointer is hit-tested against and what the keyboard's focus is read
    /// off.
    rows: Vec<(NodeKey, OutlinerId)>,
    /// The outliner block, as the last frame laid it out.
    key: Option<NodeKey>,
    /// The refresh button, as the last frame laid it out.
    refresh: Option<NodeKey>,
}

impl Browser {
    /// A browser listing `source`, every folder open.
    pub(super) fn new(source: &dyn AssetSource) -> Self {
        let mut browser = Self::default();
        browser.relist(source);
        browser
    }

    /// Reads the listing again from `source`, keeping nothing of the old one
    /// but which folders were closed.
    pub(super) fn relist(&mut self, source: &dyn AssetSource) {
        let closed: Vec<String> = self
            .listing
            .entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| entry.folder && !self.outliner.is_expanded(row(*index)))
            .map(|(_, entry)| entry.key.clone())
            .collect();
        self.listing = list(source);
        self.outliner = OutlinerState::new();
        for (index, entry) in self.listing.entries.iter().enumerate() {
            if entry.folder {
                self.outliner
                    .set_expanded(row(index), !closed.contains(&entry.key));
            }
        }
        self.rows.clear();
    }

    /// Every mesh asset listed, in tree order — what the pane offers.
    pub(super) fn assets(&self) -> Vec<&str> {
        let mut assets = Vec::new();
        self.walk(&self.listing.roots, &mut |entry| {
            if !entry.folder {
                assets.push(entry.key.as_str());
            }
        });
        assets
    }

    /// Every folder listed, in tree order.
    pub(super) fn folders(&self) -> Vec<&str> {
        let mut folders = Vec::new();
        self.walk(&self.listing.roots, &mut |entry| {
            if entry.folder {
                folders.push(entry.key.as_str());
            }
        });
        folders
    }

    /// What the pane says beside the tree, if anything.
    pub(super) fn note(&self) -> Option<&str> {
        self.listing.note.as_deref()
    }

    /// The outliner block, as the last frame laid it out.
    pub(super) const fn key(&self) -> Option<NodeKey> {
        self.key
    }

    /// The refresh button, as the last frame laid it out.
    #[cfg(test)]
    pub(super) const fn refresh_button(&self) -> Option<NodeKey> {
        self.refresh
    }

    /// The mesh asset whose row the last frame laid out under `at`, in the
    /// tree's own pixels, or [`None`] over a folder's row or none.
    pub(super) fn asset_at(&self, ui: &Ui, at: Vec2) -> Option<&str> {
        let inside = |key: NodeKey| {
            ui.rect(key).is_some_and(|(min, max)| {
                at.x >= min.x && at.x < max.x && at.y >= min.y && at.y < max.y
            })
        };
        // A row scrolled half out of the pane is laid out past its edge, so
        // the pane's own rectangle is asked too.
        if !self.key.is_some_and(inside) {
            return None;
        }
        self.rows
            .iter()
            .find(|&&(key, _)| inside(key))
            .and_then(|&(_, id)| self.asset_of(id))
    }

    /// The mesh asset whose row holds the tree's focus, if one does: what
    /// accept on the pane asks for.
    pub(super) fn focused_asset(&self, ui: &Ui) -> Option<&str> {
        let focused = ui.focused()?;
        self.rows
            .iter()
            .find(|&&(key, _)| key == focused)
            .and_then(|&(_, id)| self.asset_of(id))
    }

    /// The mesh asset `id`'s row stands for, or [`None`] for a folder's.
    fn asset_of(&self, id: OutlinerId) -> Option<&str> {
        usize::try_from(id.0)
            .ok()
            .and_then(|index| self.listing.entries.get(index))
            .filter(|entry| !entry.folder)
            .map(|entry| entry.key.as_str())
    }

    /// The rows the last frame built, each with the key of the entry it
    /// stands for.
    #[cfg(test)]
    pub(super) fn rows(&self) -> Vec<(NodeKey, &str)> {
        self.rows
            .iter()
            .filter_map(|&(key, id)| {
                let index = usize::try_from(id.0).ok()?;
                Some((key, self.listing.entries.get(index)?.key.as_str()))
            })
            .collect()
    }

    /// Calls `visit` with every entry under `indices`, depth first.
    fn walk<'a>(&'a self, indices: &[usize], visit: &mut impl FnMut(&'a Entry)) {
        for &index in indices {
            let entry = &self.listing.entries[index];
            visit(entry);
            self.walk(&entry.children, visit);
        }
    }

    /// The pane: a title with the refresh button, the tree, and the note.
    /// Returns whether the refresh button was pressed.
    pub(super) fn build(&mut self, ui: &mut Ui, options: &OutlinerOptions) -> bool {
        let Self {
            listing,
            outliner,
            rows,
            key,
            refresh: button,
        } = self;
        let mut refresh = false;
        rows.clear();
        ui.block(".editor-panel", &[], |ui| {
            ui.block(".section-head", &[], |ui| {
                ui.span(".section-title", "Assets", &[]);
                let pressed = ui.button("#assets-refresh", "Refresh");
                refresh = pressed.clicked;
                *button = Some(pressed.key);
            });
            *key = Some(
                ui.outliner_with(
                    "#assets",
                    outliner,
                    options,
                    |out| {
                        for &index in &listing.roots {
                            flatten(out, &listing.entries, index);
                        }
                    },
                    |ui, item| {
                        if let Some(built) = ui.current_key() {
                            rows.push((built, item.id));
                        }
                        let label = usize::try_from(item.id.0)
                            .ok()
                            .and_then(|index| listing.entries.get(index))
                            .map_or_else(String::new, label_of);
                        ui.span(".outliner-label", label.as_str(), &[]);
                    },
                )
                .key,
            );
            if let Some(note) = &listing.note {
                ui.span(".editor-note", note.as_str(), &[]);
            }
        });
        refresh
    }
}

/// Puts the entry at `index`, and a folder's children under it, in the
/// outliner's model.
fn flatten(out: &mut crcbl::ui::tree::OutlinerBuilder<'_>, entries: &[Entry], index: usize) {
    let entry = &entries[index];
    if entry.folder {
        out.branch(row(index), |out| {
            for &child in &entry.children {
                flatten(out, entries, child);
            }
        });
    } else {
        out.leaf(row(index));
    }
}

/// The row the entry at `index` is drawn as.
const fn row(index: usize) -> OutlinerId {
    OutlinerId(index as u64)
}

/// What a row reads: the entry's own name, a folder's with a trailing `/`.
fn label_of(entry: &Entry) -> String {
    let name = entry.key.rsplit('/').next().unwrap_or(&entry.key);
    if entry.folder {
        format!("{name}/")
    } else {
        name.to_owned()
    }
}

/// Walks `source` from its root — see the [module docs](self).
fn list(source: &dyn AssetSource) -> Listing {
    let mut listing = Listing::default();
    let mut read = 0;
    match walk(source, "", 0, &mut listing.entries, &mut read) {
        Ok(roots) => listing.roots = roots,
        Err(error) => {
            listing.note = Some(format!("The asset source cannot be listed: {error}"));
            return listing;
        }
    }
    listing.note = if read >= MAX_LISTED {
        Some(format!(
            "Stopped after {MAX_LISTED} entries: point --assets at a smaller directory"
        ))
    } else if listing.roots.is_empty() {
        Some("No .glb or .gltf assets under the asset root".to_owned())
    } else {
        None
    };
    listing
}

/// The entries kept under `dir`, appended to `entries`, as their indices —
/// only mesh assets, and folders holding one. `read` counts every entry
/// listed, kept or not, against [`MAX_LISTED`].
///
/// # Errors
///
/// The root's listing refused; a folder below it that will not list is left
/// out instead, since the rest of the tree is still worth offering.
fn walk(
    source: &dyn AssetSource,
    dir: &str,
    depth: usize,
    entries: &mut Vec<Entry>,
    read: &mut usize,
) -> Result<Vec<usize>, crcbl::store::StorageError> {
    let listed = source.list(Path::new(dir))?;
    let mut kept = Vec::new();
    for found in listed {
        if *read >= MAX_LISTED {
            break;
        }
        *read += 1;
        if found.is_dir {
            if depth + 1 >= MAX_DEPTH {
                continue;
            }
            let children = match walk(source, &found.key, depth + 1, entries, read) {
                Ok(children) => children,
                Err(error) => {
                    crcbl::log::warn!("editor: `{}` is not listed: {error}", found.key);
                    continue;
                }
            };
            if children.is_empty() {
                continue;
            }
            entries.push(Entry {
                key: found.key,
                folder: true,
                children,
            });
        } else if is_mesh_asset(&found.key) {
            entries.push(Entry {
                key: found.key,
                folder: false,
                children: Vec::new(),
            });
        } else {
            continue;
        }
        kept.push(entries.len() - 1);
    }
    Ok(kept)
}
