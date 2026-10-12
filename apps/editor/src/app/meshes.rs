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
//! # Rigs
//!
//! An asset with a skin keeps its first skin's [`Rig`]: the skeleton
//! ([`crcbl::scene_rig::skeleton_of`]) and the parts the skin deforms with
//! their joint bindings ([`crcbl::scene_rig::skinned_of`]), its ragdoll rig —
//! authored beside the asset as `<asset>.ragdoll.ron` ([`RAGDOLL_SUFFIX`]) or
//! else fitted to the skeleton, with the file it is resolved from, which the
//! inspector edits ([`Rig::resolve`]) — and the scene
//! reserves the room `super::skins` poses them in. What the skin conversion
//! refuses is logged, and the asset is drawn as it always was.
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

use crcbl::anim::{Palette, Pose, Skeleton};
use crcbl::assets::AssetSource;
use crcbl::greybox::scene3d;
use crcbl::math::{Mat4, Vec3};
use crcbl::ragdoll::{BodyShape, GenerateSettings, RagdollDesc, RagdollRig};
use crcbl::render::scene::SceneDesc;
use crcbl::scene::{build_render_scene, import_gltf};
use crcbl::scene_rig::{Skinned, reserve_for_skinning, skeleton_of, skinned_of};
use crcbl::shaders::mesh::GpuMaterial;

/// What an asset's authored ragdoll rig is filed under, after the asset's own
/// key: `props/guard.glb` has its rig in `props/guard.glb.ragdoll.ron`. A file
/// of its own rather than a section of the planned `.meta.ron` sidecar, because
/// a rig is an authored asset per skeleton, not metadata about a file.
pub(super) const RAGDOLL_SUFFIX: &str = ".ragdoll.ron";

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

/// An asset's first skin: its skeleton, and which of the asset's parts it
/// deforms — see the [module docs](self).
#[derive(Debug)]
pub(super) struct Rig {
    /// The skeleton, in the skin's own joint order.
    pub(super) skeleton: Skeleton,
    /// The parts the skin deforms: each [`SkinnedInstance::instance`] indexes
    /// the asset's [`Shelf::parts`].
    ///
    /// [`SkinnedInstance::instance`]: crcbl::scene_rig::SkinnedInstance::instance
    pub(super) skinned: Skinned,
    /// The skin's joint names, in palette order: what a rig file names bones
    /// by.
    pub(super) names: Vec<String>,
    /// The ragdoll rig authored beside the asset, or else the one fitted to
    /// the skeleton ([`RagdollRig::generate`]), or [`None`] where neither
    /// could be had.
    pub(super) ragdoll: Option<RagdollRig>,
    /// [`ragdoll`](Self::ragdoll) as its file spells it — the authored file,
    /// or the fitted rig named by [`names`](Self::names) — which the
    /// inspector edits and the rig is resolved from again. [`None`] without a
    /// rig, or for a fitted one on a bone whose name another shares.
    pub(super) desc: Option<RagdollDesc>,
}

impl Rig {
    /// Resolves [`desc`](Self::desc) against the skeleton again, after an
    /// edit, and makes it the rig drawn and dropped. A rig the file no longer
    /// resolves to leaves the last one in place and says why.
    pub(super) fn resolve(&mut self) -> Result<(), String> {
        let desc = self
            .desc
            .as_ref()
            .ok_or("this rig has no file to resolve")?;
        let names: Vec<&str> = self.names.iter().map(String::as_str).collect();
        let ragdoll = desc
            .resolve(&self.skeleton, &names, hangs_of(&self.skinned))
            .map_err(|error| error.to_string())?;
        self.ragdoll = Some(ragdoll);
        Ok(())
    }

    /// Each bone of the skeleton at rest, joint to parent joint, in the world
    /// of an entity whose mesh puts the asset's origin at `origin`. Empty for
    /// a rig whose skin deforms no part, which has nowhere to hang.
    pub(super) fn bones(&self, origin: Mat4) -> Vec<(Vec3, Vec3)> {
        let Some(hangs) = self.skinned.instances.first().map(|part| part.transform) else {
            return Vec::new();
        };
        let mut palette = Palette::new(&self.skeleton);
        palette.compute(&self.skeleton, &Pose::new(&self.skeleton));
        let to_world = origin * hangs;
        let at =
            |joint: usize| to_world.transform_point3(palette.globals()[joint].w_axis.truncate());
        self.skeleton
            .joints()
            .iter()
            .enumerate()
            .filter_map(|(joint, bone)| bone.parent.map(|parent| (at(parent), at(joint))))
            .collect()
    }

    /// Each body of the fitted ragdoll rig, its shape and where it stands at
    /// rest, in the world of an entity whose mesh puts the asset's origin at
    /// `origin`. Empty without a rig or a place to hang it.
    pub(super) fn ragdoll_bodies(&self, origin: Mat4) -> Vec<(BodyShape, crcbl::phys::Transform)> {
        let (Some(ragdoll), Some(hangs)) = (
            self.ragdoll.as_ref(),
            self.skinned.instances.first().map(|part| part.transform),
        ) else {
            return Vec::new();
        };
        let Some(worlds) =
            ragdoll.bodies_at(&self.skeleton, &Pose::new(&self.skeleton), origin * hangs)
        else {
            return Vec::new();
        };
        ragdoll
            .bodies()
            .iter()
            .zip(worlds)
            .map(|(body, world)| (body.shape, world))
            .collect()
    }
}

/// The assets a renderer was built with, and how each is drawn — see the
/// [module docs](self).
#[derive(Debug, Default)]
pub(super) struct Shelf {
    parts: BTreeMap<String, Vec<Part>>,
    rigs: BTreeMap<String, Rig>,
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

    /// `asset`'s rig, or [`None`] for one with no skin, or not on the shelf.
    pub(super) fn rig(&self, asset: &str) -> Option<&Rig> {
        self.rigs.get(asset)
    }

    /// [`rig`](Self::rig), to edit.
    pub(super) fn rig_mut(&mut self, asset: &str) -> Option<&mut Rig> {
        self.rigs.get_mut(asset)
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
            if let Some(rig) = rig_of(source, asset, &imported, &converted) {
                // Two mesh entries and two vertex regions per deformed part,
                // twice: the shared rest-pose region and the drop preview's,
                // which `Skins` reserves — see `reserve_for_skinning`.
                reserve_for_skinning(&mut scene.capacities, &rig.skinned);
                reserve_for_skinning(&mut scene.capacities, &rig.skinned);
                shelf.rigs.insert(asset.clone(), rig);
            }
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

/// `asset`'s first skin as a [`Rig`], or [`None`] for an asset with none or one
/// whose skin the conversion refused, with the reasons logged.
fn rig_of(
    source: &dyn AssetSource,
    asset: &str,
    imported: &crcbl::scene::GltfScene,
    converted: &crcbl::scene::RenderScene,
) -> Option<Rig> {
    let skin = imported.skins().first()?;
    let mut skips = Vec::new();
    let skinned = skinned_of(imported, converted, &mut skips);
    for skip in &skips {
        crcbl::log::warn!(
            "editor: `{asset}`: {} at {}: {}",
            skip.feature,
            skip.at,
            skip.why
        );
    }
    match skeleton_of(skin, imported.nodes()) {
        Ok((skeleton, order)) => {
            let hangs = hangs_of(&skinned);
            let names: Vec<String> = order
                .nodes()
                .iter()
                .map(|&node| imported.nodes()[node].name().unwrap_or("").to_owned())
                .collect();
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            let (desc, ragdoll) = match authored(source, asset, &skeleton, &borrowed, hangs) {
                Some((desc, ragdoll)) => (Some(desc), Some(ragdoll)),
                None => fitted(asset, &skeleton, &borrowed, hangs),
            };
            Some(Rig {
                skeleton,
                skinned,
                names,
                ragdoll,
                desc,
            })
        }
        Err(error) => {
            crcbl::log::warn!("editor: `{asset}` keeps no rig: {error}");
            None
        }
    }
}

/// Where a skin's skeleton hangs in its asset: under its first deformed part.
/// A rig is fitted and resolved at that scale, which is the scale every
/// entity of the asset draws it at.
fn hangs_of(skinned: &Skinned) -> Mat4 {
    skinned
        .instances
        .first()
        .map_or(Mat4::IDENTITY, |part| part.transform)
}

/// The ragdoll rig fitted to `asset`'s skeleton at the scale it `hangs` at,
/// and its file named by the joint `names`; what could not be had is logged.
fn fitted(
    asset: &str,
    skeleton: &Skeleton,
    names: &[&str],
    hangs: Mat4,
) -> (Option<RagdollDesc>, Option<RagdollRig>) {
    let ragdoll = match RagdollRig::generate(skeleton, hangs, &GenerateSettings::DEFAULT) {
        Ok(ragdoll) => ragdoll,
        Err(error) => {
            crcbl::log::warn!("editor: `{asset}` fits no ragdoll rig: {error}");
            return (None, None);
        }
    };
    let desc = ragdoll
        .to_desc(names)
        .map_err(|error| {
            crcbl::log::warn!("editor: `{asset}`'s fitted rig cannot be named: {error}");
        })
        .ok();
    (desc, Some(ragdoll))
}

/// `asset`'s authored ragdoll rig ([`RAGDOLL_SUFFIX`]), read and resolved
/// against its skeleton's joint `names` at the scale the skeleton `hangs` at,
/// or [`None`] for an asset with none. A file that is there and will not read
/// or resolve is logged, and the fitted rig stands in for it.
fn authored(
    source: &dyn AssetSource,
    asset: &str,
    skeleton: &Skeleton,
    names: &[&str],
    hangs: Mat4,
) -> Option<(RagdollDesc, RagdollRig)> {
    let key = format!("{asset}{RAGDOLL_SUFFIX}");
    let bytes = match source.read(Path::new(&key)) {
        Ok(bytes) => bytes,
        Err(crcbl::store::StorageError::NotFound(_)) => return None,
        Err(error) => {
            crcbl::log::warn!("editor: `{key}` will not read: {error}");
            return None;
        }
    };
    let rig = String::from_utf8(bytes)
        .map_err(|error| error.to_string())
        .and_then(|text| RagdollDesc::from_ron(&text).map_err(|error| error.to_string()))
        .and_then(|desc| {
            let rig = desc
                .resolve(skeleton, names, hangs)
                .map_err(|error| error.to_string())?;
            Ok((desc, rig))
        });
    match rig {
        Ok(rig) => Some(rig),
        Err(error) => {
            crcbl::log::warn!(
                "editor: `{key}` is not a rig for `{asset}`, so it is fitted: {error}"
            );
            None
        }
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

#[cfg(test)]
mod tests;
