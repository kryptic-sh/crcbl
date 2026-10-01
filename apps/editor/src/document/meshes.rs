//! Where a scene's meshes come from: the asset source their keys are read
//! through, and the boxes measured from it.
//!
//! A [`crcbl::scene_mesh::Mesh`] row names a glTF asset by key; its box — what
//! it is drawn, picked and moved by — is the asset's, measured by the
//! document's [`MeshLibrary`] and written into the row. **Every path that puts
//! a row in the world resolves after it**: the load [`Document::open`] runs,
//! play's restore, and each command applied, undone or redone — a spawn, an
//! attach, a delete's undo, or a panel retyping the key. The colliders of the
//! meshes whose box moved are rebuilt, so a mesh picks where it is drawn.
//!
//! # Which source
//!
//! [`Document::set_assets`] names it. A document opened from a directory reads
//! from the directory holding the scene; one opened out of a compiled-in
//! source reads from an empty one, so every mesh in it is the placeholder until
//! a caller names somewhere. A key is relative to that root, so a scene keeps
//! its meshes when the root it is opened with does.

use std::collections::BTreeSet;

use crcbl::assets::AssetSource;
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_mesh::{MESHES, Mesh, MeshLibrary, mesh_of};

use super::{Document, sync_colliders};

impl Document {
    /// Reads every mesh's asset through `assets` from now on, measuring each
    /// afresh: a different root is different files under the same keys.
    pub fn set_assets(&mut self, assets: Box<dyn AssetSource>) {
        self.assets = assets;
        self.meshes = MeshLibrary::new();
        self.resolve_meshes();
    }

    /// The source a mesh's asset key is read through: what an asset browser
    /// lists.
    #[must_use]
    pub fn assets(&self) -> &dyn AssetSource {
        self.assets.as_ref()
    }

    /// The box of the asset `asset` names, in its own frame, measured through
    /// [`assets`](Self::assets) — what a mesh dropped onto a surface is stood
    /// on it by.
    ///
    /// # Errors
    ///
    /// [`crcbl::scene_mesh::MeshError`] naming the asset, as a mesh of it would
    /// report.
    pub fn measure(
        &mut self,
        asset: &str,
    ) -> Result<(crcbl::math::DVec3, crcbl::math::DVec3), crcbl::scene_mesh::MeshError> {
        self.meshes.measure(self.assets.as_ref(), asset)
    }

    /// `id`'s mesh, or [`None`] for an id this document does not hold or one
    /// the meshes system does not.
    pub fn mesh(&mut self, id: SceneEntityId) -> Option<&Mesh> {
        let entity = self.ids.entity(id)?;
        mesh_of(&mut self.world, entity)
    }

    /// Every asset a mesh in the scene is boxed by — measured, so one the
    /// source has — in key order: what the viewport makes resident to draw.
    #[must_use]
    pub fn mesh_assets(&mut self) -> BTreeSet<String> {
        let mut assets = BTreeSet::new();
        for entity in self.registry.entities(&mut self.world, &self.ids, MESHES) {
            if let Some(mesh) = mesh_of(&mut self.world, entity)
                && mesh.local_bounds().is_some()
            {
                assets.insert(mesh.asset.clone());
            }
        }
        assets
    }

    /// Every mesh whose asset could not be measured, as the last resolve found
    /// it, each naming its entity: drawn as the placeholder until it can be.
    #[must_use]
    pub fn mesh_problems(&self) -> Vec<String> {
        self.mesh_problems
            .iter()
            .filter_map(|problem| {
                let id = self.ids.id(problem.entity)?;
                Some(match self.scene.entity_name(id) {
                    Some(name) => format!("entity `{}` #{id}: {problem}", name.as_str()),
                    None => format!("entity #{id}: {problem}"),
                })
            })
            .collect()
    }

    /// A number that moves every time a mesh's box moved because its asset
    /// was measured, or stopped being: what a view of the meshes re-reads on.
    #[must_use]
    pub const fn measures(&self) -> u64 {
        self.measures
    }

    /// Writes every mesh row its asset's box, or leaves it on the placeholder
    /// with a problem, and rebuilds the colliders of those whose box moved —
    /// see the [module docs](self).
    pub(super) fn resolve_meshes(&mut self) {
        let resolution = self.meshes.resolve(&mut self.world, self.assets.as_ref());
        if !resolution.moved.is_empty() {
            sync_colliders(&self.registry, &mut self.world, resolution.moved);
            self.measures += 1;
        }
        self.mesh_problems = resolution.problems;
    }
}
