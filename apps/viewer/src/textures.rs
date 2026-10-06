//! Texture hot reload: a texture beside the document is written again, and the
//! frame shows it without the document being reopened.
//!
//! ```text
//! PolledWatch (settled change) ──▶ AssetRegistry::reload ──▶ new bytes
//!   └─ decode_image: refused? ──▶ refuse_reload, the old texture stays
//!   └─ build_texture_pages over the document with the new bytes
//!        ├─ a material would sample a different layer? ──▶ refuse_reload
//!        └─ ForwardRenderer::replace_page per kind that changed
//!             ──▶ commit_reload, and the page is the file as it is now
//! ```
//!
//! # What it watches
//!
//! Every image the document names by a URI beside it —
//! [`GltfImage::key`](crcbl::scene::GltfImage::key) — and nothing else: an image
//! inside a `.glb` has no file of its own, and the document itself is
//! [`crate::watch`]'s, whose reload rebuilds this along with everything else.
//! The images are requested through an [`AssetRegistry`] over the same
//! directory [`crate::model::load`] roots the document at, so the handle a
//! reload goes through is the registry's and survives it.
//!
//! # A reload is the page rebuilt, never the scene
//!
//! The changed image is decoded first, and a file the decoder refuses — caught
//! half written, or not a PNG at all — is refused through
//! [`AssetRegistry::refuse_reload`] and the texture already on screen stays.
//! Otherwise the pages are rebuilt by
//! [`build_texture_pages`], the
//! code the document's first build used, over the kept
//! [`Model::textures`] with the new bytes in
//! place; each kind whose page changed goes to the renderer whole, and every
//! material row stays as it is. A rebuild that would point a row at a
//! different layer — an image that did not decode when the document opened,
//! and so has no layer, decoding now — is refused rather than half applied: it
//! is a new scene, and re-exporting the document is what makes one.
//!
//! **Native only**, as [`crate::watch`] is: a page has no file to write again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crcbl::assets::watch::PolledWatch;
use crcbl::assets::{AssetHandle, AssetRegistry, DirSource};
use crcbl::render::{PageDesc, PageKind};
use crcbl::scene::GltfScene;
use crcbl::scene::gltf_render::{TexturePages, build_texture_pages, decode_image};
use crcbl::shaders::mesh::GpuMaterial;

use crate::gpu::Gpu;
use crate::model::Model;

/// One watched image file: which of the document's images it is, the handle
/// its bytes are held under, and where it is on disk.
#[derive(Debug)]
struct Watched {
    image: usize,
    handle: AssetHandle,
    path: PathBuf,
}

/// The document's textures as they are on screen, and the watch over their
/// files.
#[derive(Debug)]
pub struct Textures {
    /// The document's own key, which every message names.
    key: PathBuf,
    registry: AssetRegistry<DirSource>,
    watched: Vec<Watched>,
    watch: PolledWatch,
    /// Wall-clock time since the watch was made — [`crate::watch::Watch`]'s
    /// clock, for its reason.
    clock: Duration,
    /// [`Model::textures`], with every committed reload's bytes in place.
    scene: GltfScene,
    /// The pages the renderer holds, which a rebuild is compared against.
    page: PageDesc<'static>,
    /// The material table the renderer holds, which a rebuild must equal.
    materials: Vec<GpuMaterial>,
    /// Reloads that reached the frame.
    reloads: u64,
    /// Reloads refused, the texture on screen kept.
    refusals: u64,
    /// Why the last refusal was made, as it was logged.
    last_refusal: Option<String>,
}

impl Textures {
    /// Watches the images `model` — read from `document` — names beside it,
    /// treating what is on disk now as already on screen.
    ///
    /// Which it is: the document was just loaded, and its pages built from
    /// these same files. An image whose key the registry refuses was refused
    /// by the importer already and is not watched; one whose file was missing
    /// is watched, and its reload is refused, because there is no page layer
    /// for it to land in — re-exporting the document builds one.
    #[must_use]
    pub fn new(document: &Path, model: &Model) -> Self {
        let mut registry = AssetRegistry::new(DirSource::at(crate::model::root_of(document)));
        let mut watched = Vec::new();
        for (image, entry) in model.textures.images().iter().enumerate() {
            let Some(key) = entry.key() else { continue };
            let Ok(handle) = registry.request(Path::new(key)) else {
                continue;
            };
            watched.push(Watched {
                image,
                handle,
                path: crate::model::root_of(document).join(key),
            });
        }
        let watch = PolledWatch::new(
            watched.iter().map(|watched| watched.path.clone()),
            Duration::ZERO,
        );
        Self {
            key: model.key.clone(),
            registry,
            watched,
            watch,
            clock: Duration::ZERO,
            scene: model.textures.clone(),
            page: model.render.scene.page.clone(),
            materials: model.render.scene.materials.clone(),
            reloads: 0,
            refusals: 0,
            last_refusal: None,
        }
    }

    /// How many texture reloads have reached the frame.
    #[must_use]
    pub const fn reloads(&self) -> u64 {
        self.reloads
    }

    /// How many texture reloads were refused, the texture on screen kept.
    #[must_use]
    pub const fn refusals(&self) -> u64 {
        self.refusals
    }

    /// Why the last refused reload was refused, in the words the log used.
    #[must_use]
    pub fn last_refusal(&self) -> Option<&str> {
        self.last_refusal.as_deref()
    }

    /// The image files being watched, in the document's image order.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.watched.iter().map(|watched| watched.path.as_path())
    }

    /// Advances the clock by `dt` wall-clock seconds, reloads every image
    /// whose file has settled since, and puts each on screen or says why not.
    ///
    /// `dt` is the frame's, for [`crate::watch::Watch::poll`]'s reason. Every
    /// failure keeps the texture already drawn and is logged once.
    pub fn poll(&mut self, dt: f64, gpu: &mut Gpu) {
        self.clock += Duration::from_secs_f64(dt);
        for path in self.watch.poll(self.clock) {
            let Some(handle) = self
                .watched
                .iter()
                .find(|watched| watched.path == path)
                .map(|watched| watched.handle)
            else {
                continue;
            };
            if !self.registry.reload(handle) {
                let why = self
                    .registry
                    .get(handle)
                    .and_then(|asset| asset.reload_failure().map(ToString::to_string))
                    .unwrap_or_else(|| {
                        "it was not loaded when the document was opened, so it has no page \
                         layer to land in; re-export the document to add it"
                            .to_owned()
                    });
                self.refuse(&path, why);
            }
        }
        self.registry.poll();
        let ready: Vec<AssetHandle> = self
            .watched
            .iter()
            .map(|watched| watched.handle)
            .filter(|handle| {
                self.registry
                    .get(*handle)
                    .is_some_and(|asset| asset.reloaded().is_some())
            })
            .collect();
        for handle in ready {
            self.apply(handle, gpu);
        }
    }

    /// Puts `handle`'s reloaded bytes on screen, committing them, or refuses
    /// them and keeps what is there.
    fn apply(&mut self, handle: AssetHandle, gpu: &mut Gpu) {
        let Some(bytes) = self
            .registry
            .get(handle)
            .and_then(|asset| asset.reloaded())
            .map(<[u8]>::to_vec)
        else {
            return;
        };
        // Every image the file is, since two entries of the document's
        // `images` array may name one file.
        let images: Vec<usize> = self
            .watched
            .iter()
            .filter(|watched| watched.handle == handle)
            .map(|watched| watched.image)
            .collect();
        let path = self
            .watched
            .iter()
            .find(|watched| watched.handle == handle)
            .map_or_else(PathBuf::new, |watched| watched.path.clone());
        match self.rebuilt(&images, bytes, gpu) {
            Ok((scene, pages)) => {
                self.registry.commit_reload(handle);
                self.scene = scene;
                self.page = pages.page;
                self.reloads += 1;
                crcbl::log::info!(
                    "viewer: {} reloaded into {}",
                    path.display(),
                    self.key.display()
                );
            }
            Err(why) => {
                self.registry.refuse_reload(handle, why.clone());
                self.refuse(&path, why);
            }
        }
    }

    /// Counts and logs a refused reload of the file at `path`.
    fn refuse(&mut self, path: &Path, why: String) {
        self.refusals += 1;
        crcbl::log::warn!(
            "viewer: {} was written again and the texture on screen was kept: {why}",
            path.display(),
        );
        self.last_refusal = Some(why);
    }

    /// The document with `bytes` as every one of `images`, and its pages — on
    /// the renderer already — or why not.
    ///
    /// On an error the renderer holds the pages it held before: a kind
    /// replaced before a later one was refused is put back.
    fn rebuilt(
        &self,
        images: &[usize],
        bytes: Vec<u8>,
        gpu: &mut Gpu,
    ) -> Result<(GltfScene, TexturePages), String> {
        let mime = images
            .first()
            .and_then(|image| self.scene.images().get(*image))
            .and_then(|entry| entry.mime());
        decode_image(&bytes, mime)?;
        let mut scene = self.scene.clone();
        for image in images {
            scene.set_image_bytes(*image, bytes.clone());
        }
        let pages = build_texture_pages(&scene, &self.key);
        if pages.materials != self.materials {
            return Err(
                "the rebuilt pages would point a material at a different layer, which \
                        is a new scene rather than a new texture; re-export the document to \
                        see it"
                    .to_owned(),
            );
        }
        let mut replaced = Vec::new();
        for kind in PageKind::ALL {
            if pages.page.extent(kind) == self.page.extent(kind)
                && pages.page.layers(kind) == self.page.layers(kind)
            {
                continue;
            }
            if let Err(error) = gpu.replace_page(kind, &pages.page) {
                for kind in replaced {
                    if let Err(error) = gpu.replace_page(kind, &self.page) {
                        crcbl::log::warn!(
                            "viewer: the {} page could not be put back after a refused reload, \
                             so it shows the new texture: {error}",
                            kind.label(),
                        );
                    }
                }
                return Err(format!(
                    "the renderer refused the {} page: {error}",
                    kind.label()
                ));
            }
            replaced.push(kind);
        }
        Ok((scene, pages))
    }
}
