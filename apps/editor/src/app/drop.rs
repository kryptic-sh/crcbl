//! "Drop it and watch it fall": the ragdoll preview `docs/plan/35-ragdolls.md`
//! asks the rig editor for.
//!
//! G on a selected rigged mesh spawns the ragdoll rig fitted to it
//! ([`super::meshes::Rig::ragdoll`]) at the mesh's rest pose in a physics
//! world of its own, with a ground plane at the editor's ground and the
//! scene's [solids](solids), and steps it a frame at a time; the mesh draws in the pose the bodies stand in, from a
//! region of its own ([`super::skins::Skins`]), while every other entity of
//! the asset stays at rest. G again, or the entity going away, ends it. The
//! scene itself is never touched: nothing here is undoable because nothing
//! here is an edit.
//!
//! What it lands on is what the scene collides as when it plays: every entity
//! with a [`Body`] row, as the box it is placed as. Each stands still where
//! the scene puts it, a dynamic one too — the preview simulates the ragdoll
//! and nothing else — and an entity with no body row is not there to land on,
//! as it is not in play. It steps at a fixed sixtieth of a second a frame.

use crcbl::anim::{Palette, Pose};
use crcbl::ecs::Entity;
use crcbl::math::{DVec3, Mat4};
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, PhysicsSystem, SurfaceMaterial, Transform,
};
use crcbl::ragdoll::{Handoff, Ragdoll};
use crcbl::registry::OrientedBox;
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_physics::{BODIES, Body};

use super::meshes::Rig;
use crate::document::Document;

/// The step the preview advances by each frame, in seconds.
pub(super) const STEP: f64 = 1.0 / 60.0;

/// A thing the ragdoll lands on: a box standing still, and its surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Solid {
    pub(super) placed: OrientedBox,
    pub(super) material: SurfaceMaterial,
}

/// What the scene collides as, but `dropped`: every entity with a [`Body`]
/// row a simulation takes, placed as a box a collider can be — the rows play
/// leaves out are left out here too.
pub(super) fn solids(document: &mut Document, dropped: SceneEntityId) -> Vec<Solid> {
    let mut solids = Vec::new();
    for id in document.entities_in(BODIES) {
        if id == dropped {
            continue;
        }
        let body = document
            .component(id, BODIES)
            .and_then(|component| component.as_any().downcast_ref::<Body>().copied());
        let Some(body) = body.filter(|body| body.check().is_ok()) else {
            continue;
        };
        let Some(placed) = document.placement(id).filter(|placed| {
            placed.centre.is_finite()
                && placed.half_extents.is_finite()
                && placed.half_extents.min_element() > 0.0
        }) else {
            continue;
        };
        solids.push(Solid {
            placed,
            material: body.material(),
        });
    }
    solids
}

/// A ragdoll falling in the preview's own world.
#[derive(Debug)]
pub(super) struct DropPreview {
    id: SceneEntityId,
    asset: String,
    physics: PhysicsSystem,
    ragdoll: Ragdoll,
    pose: Pose,
    palette: Palette,
    model: Mat4,
}

impl DropPreview {
    /// The preview of entity `id`, a mesh of `asset` whose origin stands at
    /// `origin`, falling among `solids`, or [`None`] for a rig with no fitted
    /// ragdoll or nowhere to hang it.
    pub(super) fn start(
        id: SceneEntityId,
        asset: &str,
        rig: &Rig,
        origin: Mat4,
        solids: &[Solid],
    ) -> Option<Self> {
        let ragdoll_rig = rig.ragdoll.as_ref()?;
        let hangs = rig.skinned.instances.first()?.transform;
        let model = origin * hangs;
        let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        physics.add_force_provider(Box::new(GravityForce::EARTH));
        physics.add_plane(DVec3::Y, 0.0, SurfaceMaterial::DEFAULT);
        let pose = Pose::new(&rig.skeleton);
        let mut entities =
            (1..).map(|index: u64| Entity::from_bits((1 << 32) | index).expect("generation one"));
        for (solid, entity) in solids.iter().zip(&mut entities) {
            // A transform and a collider with no body is a static one.
            let transform = Transform::new(solid.placed.centre, solid.placed.rotation);
            physics.set_transform(entity, transform);
            physics.set_collider(
                entity,
                &ColliderComponent::Box {
                    offset: DVec3::ZERO,
                    half_extents: solid.placed.half_extents,
                    is_trigger: false,
                },
                &transform,
            );
            physics.set_material(entity, solid.material);
        }
        let entities: Vec<Entity> = entities.take(ragdoll_rig.bodies().len()).collect();
        let handoff = Handoff {
            pose: &pose,
            model,
            previous: None,
        };
        let ragdoll = match Ragdoll::spawn(
            &mut physics,
            ragdoll_rig,
            &rig.skeleton,
            &handoff,
            &entities,
        ) {
            Ok(ragdoll) => ragdoll,
            Err(error) => {
                crcbl::log::warn!("editor: `{asset}` will not drop: {error}");
                return None;
            }
        };
        let mut palette = Palette::new(&rig.skeleton);
        palette.compute(&rig.skeleton, &pose);
        Some(Self {
            id,
            asset: asset.to_owned(),
            physics,
            ragdoll,
            pose,
            palette,
            model,
        })
    }

    /// The entity being previewed.
    pub(super) const fn id(&self) -> SceneEntityId {
        self.id
    }

    /// The asset it is a mesh of.
    pub(super) fn asset(&self) -> &str {
        &self.asset
    }

    /// Steps the world once and reads the pose the bodies stand in into the
    /// palette. A pose the bodies cannot be read back into keeps the last one.
    pub(super) fn step(&mut self, rig: &Rig) {
        self.physics.step(STEP);
        match self
            .ragdoll
            .pose_into(&self.physics, self.model, &mut self.pose)
        {
            Ok(()) => self.palette.compute(&rig.skeleton, &self.pose),
            Err(error) => crcbl::log::warn!("editor: the drop preview kept its pose: {error}"),
        }
    }

    /// The skinning palette of the pose the bodies stand in.
    pub(super) fn palette(&self) -> &[Mat4] {
        self.palette.matrices()
    }

    /// Where each body stands now, for a test to watch it fall.
    #[cfg(test)]
    pub(super) fn heights(&self) -> Vec<f64> {
        self.ragdoll
            .body_transforms(&self.physics)
            .unwrap_or_default()
            .iter()
            .map(|transform| transform.position.y)
            .collect()
    }
}
