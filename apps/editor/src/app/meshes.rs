//! The geometry the viewport can draw: the greybox pack, and every glTF asset
//! a scene's meshes name, resident in one renderer.
//!
//! **A renderer's meshes are fixed when it is built** —
//! [`SceneDesc`]'s pools are sized and filled at
//! [`ForwardRenderer::with_scene`](crcbl::render::ForwardRenderer::with_scene)
//! and never grow — so drawing an asset the renderer was not built with means
//! building another, the way `apps/viewer` reloads a re-exported document. A
//! [`Shelf`] is what one was built with: [`crcbl::greybox::scene3d`]'s meshes
//! first, so [`crcbl::greybox::GREYBOX_CUBE`] still names the cube, then each
//! asset's, converted by the engine's own bridge
//! ([`crcbl::scene::build_render_scene`]) and appended. `crate::app` rebuilds
//! when the document's meshes name an asset the shelf lacks, and keeps every
//! asset it already holds, so undoing and redoing a spawn does not rebuild
//! twice.
//!
//! # Materials by their factors alone
//!
//! An asset's materials are appended with every page column set to
//! [`GpuMaterial::NO_PAGE`]: a renderer holds one texture page per kind, the
//! greybox grid's, and an asset's images are resampled onto pages of their own
//! extent that cannot be merged into it. So a mesh shades by its base colour,
//! metallic, roughness and emissive factors, and its textures wait on a
//! renderer that can take pages after it is built — `docs/backlog.md` has the
//! entry.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crcbl::assets::AssetSource;
use crcbl::greybox::scene3d;
use crcbl::math::Mat4;
use crcbl::render::scene::SceneDesc;
use crcbl::scene::{build_render_scene, import_gltf};
use crcbl::shaders::mesh::GpuMaterial;

/// One instance an asset is drawn as: a (node, primitive) pair of its glTF,
/// as [`crcbl::scene::RenderScene::instances`] expands them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Part {
    /// Which of the shelf's [`SceneDesc::meshes`] it draws.
    pub(super) mesh: usize,
    /// Which of its [`SceneDesc::materials`] it shades through.
    pub(super) material: usize,
    /// Where it stands in the asset's own frame.
    pub(super) transform: Mat4,
}

/// The assets a renderer was built with, and how each is drawn — see the
/// [module docs](self).
#[derive(Debug, Default)]
pub(super) struct Shelf {
    parts: BTreeMap<String, Vec<Part>>,
}

impl Shelf {
    /// Whether every asset in `wanted` is on the shelf.
    pub(super) fn holds(&self, wanted: &BTreeSet<String>) -> bool {
        wanted.iter().all(|asset| self.parts.contains_key(asset))
    }

    /// The assets on the shelf, in key order.
    pub(super) fn assets(&self) -> BTreeSet<String> {
        self.parts.keys().cloned().collect()
    }

    /// How `asset` is drawn, or [`None`] for one not on the shelf.
    pub(super) fn parts(&self, asset: &str) -> Option<&[Part]> {
        self.parts.get(asset).map(Vec::as_slice)
    }

    /// The scene a renderer holding the greybox pack and every asset in
    /// `assets` is built from, read through `source`, and the shelf it fills.
    ///
    /// An asset that will not import is left off and logged: the document
    /// measured it a moment ago, so this is a source that changed in between,
    /// and a mesh of it is drawn as its box until the next rebuild.
    pub(super) fn build(
        source: &dyn AssetSource,
        assets: &BTreeSet<String>,
    ) -> (Self, SceneDesc<'static>) {
        let mut scene = scene3d();
        let mut shelf = Self::default();
        for asset in assets {
            let imported = match import_gltf(source, Path::new(asset)) {
                Ok(imported) => imported,
                Err(error) => {
                    crcbl::log::warn!("editor: `{asset}` is drawn as its box: {error}");
                    continue;
                }
            };
            let converted = build_render_scene(&imported, Path::new(asset));
            let (meshes, materials) = (scene.meshes.len(), scene.materials.len());
            let room = converted.scene.capacities;
            scene.capacities.vertices = scene.capacities.vertices.saturating_add(room.vertices);
            scene.capacities.indices = scene.capacities.indices.saturating_add(room.indices);
            scene.capacities.meshes = scene.capacities.meshes.saturating_add(room.meshes);
            scene.capacities.materials = scene.capacities.materials.saturating_add(room.materials);
            scene.meshes.extend(converted.scene.meshes);
            scene
                .materials
                .extend(converted.scene.materials.into_iter().map(by_factors));
            let parts = converted
                .instances
                .iter()
                .map(|instance| Part {
                    mesh: meshes + instance.mesh,
                    material: materials + instance.material,
                    transform: instance.transform,
                })
                .collect();
            shelf.parts.insert(asset.clone(), parts);
        }
        (shelf, scene)
    }
}

/// `material` with no page: what it shades as by its factors — see the
/// [module docs](self).
fn by_factors(material: GpuMaterial) -> GpuMaterial {
    GpuMaterial {
        base_color_texture: GpuMaterial::NO_PAGE,
        normal_texture: GpuMaterial::NO_PAGE,
        metallic_roughness_occlusion_texture: GpuMaterial::NO_PAGE,
        emissive_texture: GpuMaterial::NO_PAGE,
        ..material
    }
}
