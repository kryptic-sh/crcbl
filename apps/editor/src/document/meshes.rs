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
//! # Dropping an asset into the scene
//!
//! [`Document::spawn_mesh`] is what a drop from the asset browser does: a new
//! entity with one mesh row of the asset, standing on the point the drop lands
//! on ([`Document::drop_point`]: the surface under the pointer, else the ground
//! plane `y = 0`) — the bottom centre of the asset's box, or of the
//! placeholder's for one that will not load, put on that point. One
//! [`EditCommand::Spawn`], so one undo takes it back — wrapped in a
//! [`EditCommand::Batch`] with the [`EditCommand::ListSystem`] that adds
//! `meshes` to the manifest when the scene had none, so that undo also puts
//! the files back as they were. Refused in play mode, like every edit.
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
use crcbl::math::DVec3;
use crcbl::phys::{PhysicsSystem, Ray};
use crcbl::render::ViewRay;
use crcbl::scene::scn::{SceneEntityId, row_text};
use crcbl::scene_mesh::{MESHES, Mesh, MeshLibrary, MeshPathError, check_asset, mesh_of};

use super::{Document, EditError, sync_colliders, widen};
use crate::command::{EditCommand, SystemRow};

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

    /// Spawns a mesh of `asset` standing on `point`, as one undoable entry,
    /// and returns its id — the drop the module docs of `document::meshes`
    /// describe.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode; [`EditError::Asset`] for a key no
    /// mesh may name, the empty key included; [`EditError::NoSystem`] for a
    /// vocabulary with no meshes. An asset that is a key and will not load is
    /// **not** refused: it is spawned as the placeholder, and
    /// [`mesh_problems`](Self::mesh_problems) names it.
    pub fn spawn_mesh(&mut self, asset: &str, point: DVec3) -> Result<SceneEntityId, EditError> {
        self.refuse_in_play()?;
        if asset.is_empty() {
            return Err(EditError::Asset(MeshPathError::Extension(String::new())));
        }
        check_asset(asset).map_err(EditError::Asset)?;
        if !self.registry.contains(MESHES) {
            return Err(EditError::NoSystem(MESHES.to_owned()));
        }
        let local = self.measure(asset).ok();
        let row = row_text(MESHES, &Mesh::standing_on(asset, point, local))?;
        let id = self.ids.next_id();
        let spawn = EditCommand::Spawn {
            entity: id,
            rows: vec![SystemRow {
                system: MESHES.to_owned(),
                row,
            }],
            name: None,
        };
        let command = self.listing_first(MESHES, spawn);
        self.apply(command)?;
        Ok(id)
    }

    /// Where a drop along `ray` lands: the first surface it strikes, else
    /// where it meets the ground plane `y = 0` in front of the camera.
    ///
    /// # Errors
    ///
    /// [`EditError::NoGround`] for a ray that strikes nothing and runs level
    /// with the ground or away from it.
    pub fn drop_point(&mut self, ray: &ViewRay) -> Result<DVec3, EditError> {
        let ray = Ray::new(widen(ray.origin), widen(ray.direction));
        if let Some((_, hit)) = self
            .world
            .system_mut::<PhysicsSystem>()
            .and_then(|physics| physics.cast_ray(&ray))
        {
            return Ok(hit.point);
        }
        ground(&ray).ok_or(EditError::NoGround)
    }

    /// Where `ray` meets the ground plane `y = 0` in front of the camera,
    /// whatever stands between — the point a keyboard drop at the view's
    /// centre lands on, which a person can see coming whatever is selected.
    ///
    /// # Errors
    ///
    /// [`EditError::NoGround`] for a ray level with the ground or leaving it.
    pub fn ground_point(ray: &ViewRay) -> Result<DVec3, EditError> {
        ground(&Ray::new(widen(ray.origin), widen(ray.direction))).ok_or(EditError::NoGround)
    }

    /// Writes every mesh row its asset's box, or leaves it on the placeholder
    /// with a problem, and rebuilds the colliders of those whose box moved —
    /// see the module docs of `document::meshes`.
    pub(super) fn resolve_meshes(&mut self) {
        let resolution = self.meshes.resolve(&mut self.world, self.assets.as_ref());
        if !resolution.moved.is_empty() {
            sync_colliders(&self.registry, &mut self.world, resolution.moved);
            self.measures += 1;
        }
        self.mesh_problems = resolution.problems;
    }
}

/// Where `ray` meets the ground plane `y = 0` ahead of its origin, or [`None`]
/// for a ray level with it or leaving it.
fn ground(ray: &Ray) -> Option<DVec3> {
    let distance = -ray.origin.y / ray.dir.y;
    (distance.is_finite() && distance >= 0.0).then(|| ray.origin + ray.dir * distance)
}
