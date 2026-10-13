//! "Drop it and watch it fall": the ragdoll preview `docs/plan/35-ragdolls.md`
//! asks the rig editor for.
//!
//! G on a selected rigged mesh spawns the ragdoll rig fitted to it
//! ([`super::meshes::Rig::ragdoll`]) at the mesh's rest pose in a physics
//! world of its own, with a ground plane at the editor's ground and the
//! scene's [solids], and steps it a frame at a time; the mesh draws in the
//! pose the bodies stand in, from a region of its own
//! ([`super::skins::Skins`]), while every other entity of the asset stays at
//! rest. G again, or the entity going away, ends it. The
//! scene itself is never touched: nothing here is undoable because nothing
//! here is an edit.
//!
//! What it lands on is what the scene collides as when it plays: every entity
//! with a [`Body`] row, as the box it is placed as. Each stands still where
//! the scene puts it, a dynamic one too — the preview simulates the ragdoll
//! and nothing else — and an entity with no body row is not there to land on,
//! as it is not in play. It steps at a fixed sixtieth of a second a frame.
//!
//! **G while the scene plays drops the ragdoll into play itself** rather than
//! a world of its own: its bodies are spawned into the playing world's
//! [`Simulation`], so it collides with the scene's bodies, the dynamic ones
//! as they move, as a game's corpse would, and steps when the world ticks,
//! so a paused scene holds it still. The mesh's own body, if it has a row,
//! leaves the simulation, since the ragdoll takes its place; G again takes
//! the ragdoll out and leaves the mesh's body out too, until stop puts the
//! scene back, which ends the drop with the world it was spawned in. There
//! is no ground plane in play: a scene with nothing under the mesh lets it
//! fall, as it lets the scene's bodies.

use crcbl::anim::ragdoll::RagdollError;
use crcbl::anim::{Palette, Pose};
use crcbl::ecs::Entity;
use crcbl::math::{DVec3, Mat4};
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, PhysicsSystem, SurfaceMaterial, Transform,
};
use crcbl::ragdoll::{Handoff, Ragdoll, RagdollRig, SpawnError};
use crcbl::registry::OrientedBox;
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_physics::{BODIES, Body, Simulation};

use super::meshes::Rig;
use crate::document::{Document, PlayState};

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

/// Where a dropped ragdoll's bodies are simulated.
#[derive(Debug)]
enum Bodies {
    /// A world of the preview's own, stepped a frame at a time.
    Own(Box<PhysicsSystem>),
    /// The playing scene's [`Simulation`], stepped when the world ticks.
    Play,
}

/// A ragdoll falling, in the preview's own world or in play's.
#[derive(Debug)]
pub(super) struct DropPreview {
    id: SceneEntityId,
    asset: String,
    bodies: Bodies,
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
        let (ragdoll_rig, model) = hung(rig, origin)?;
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
        let ragdoll = match spawn(&mut physics, ragdoll_rig, rig, &pose, model, &entities) {
            Ok(ragdoll) => ragdoll,
            Err(error) => {
                crcbl::log::warn!("editor: `{asset}` will not drop: {error}");
                return None;
            }
        };
        Some(Self::new(
            id,
            asset,
            Bodies::Own(Box::new(physics)),
            ragdoll,
            rig,
            pose,
            model,
        ))
    }

    /// The drop of entity `id`, a mesh of `asset` whose origin stands at
    /// `origin`, into the scene `document` plays: its bodies spawned as
    /// entities of the playing world into its [`Simulation`], and the mesh's
    /// own body taken out of it — or why not, to say on the status line.
    pub(super) fn start_in_play(
        id: SceneEntityId,
        asset: &str,
        rig: &Rig,
        origin: Mat4,
        document: &mut Document,
    ) -> Result<Self, String> {
        let (ragdoll_rig, model) =
            hung(rig, origin).ok_or_else(|| format!("`{asset}` has no ragdoll to drop"))?;
        let own = document.ids().entity(id);
        let world = document.world_mut();
        if world.system_mut::<Simulation>().is_none() {
            return Err(format!(
                "`{asset}` cannot drop into play: nothing simulates bodies, since no entity \
                 has a body row"
            ));
        }
        let entities: Vec<Entity> = ragdoll_rig.bodies().iter().map(|_| world.spawn()).collect();
        let Some(simulation) = world.system_mut::<Simulation>() else {
            unreachable!("checked before the spawn");
        };
        let physics = simulation.physics_mut();
        let pose = Pose::new(&rig.skeleton);
        match spawn(physics, ragdoll_rig, rig, &pose, model, &entities) {
            Ok(ragdoll) => {
                if let Some(own) = own {
                    physics.remove_entity(own);
                }
                Ok(Self::new(
                    id,
                    asset,
                    Bodies::Play,
                    ragdoll,
                    rig,
                    pose,
                    model,
                ))
            }
            Err(error) => {
                for entity in entities {
                    world.despawn(entity);
                }
                Err(format!("`{asset}` will not drop: {error}"))
            }
        }
    }

    /// The drop of `ragdoll`, spawned in `bodies` at `pose`.
    fn new(
        id: SceneEntityId,
        asset: &str,
        bodies: Bodies,
        ragdoll: Ragdoll,
        rig: &Rig,
        pose: Pose,
        model: Mat4,
    ) -> Self {
        let mut palette = Palette::new(&rig.skeleton);
        palette.compute(&rig.skeleton, &pose);
        Self {
            id,
            asset: asset.to_owned(),
            bodies,
            ragdoll,
            pose,
            palette,
            model,
        }
    }

    /// Ends the drop: a ragdoll in play takes its bodies and its entities out
    /// of the playing world; one in a world of its own goes with that world.
    pub(super) fn end(self, document: &mut Document) {
        let Bodies::Play = self.bodies else {
            return;
        };
        if document.play_state() == PlayState::Editing {
            // Stop threw the world away, and the bodies with it.
            return;
        }
        let entities = self.ragdoll.entities().to_vec();
        let world = document.world_mut();
        if let Some(simulation) = world.system_mut::<Simulation>() {
            self.ragdoll.remove(simulation.physics_mut());
        }
        for entity in entities {
            world.despawn(entity);
        }
    }

    /// The entity being previewed.
    pub(super) const fn id(&self) -> SceneEntityId {
        self.id
    }

    /// The asset it is a mesh of.
    pub(super) fn asset(&self) -> &str {
        &self.asset
    }

    /// Steps a world of its own once and reads the pose the bodies stand in
    /// into the palette, or says the drop is over: its bodies went with the
    /// world stop threw away. A pose the bodies cannot be read back into
    /// keeps the last one.
    pub(super) fn step(&mut self, rig: &Rig, document: &mut Document) -> bool {
        let physics = match &mut self.bodies {
            Bodies::Own(physics) => {
                physics.step(STEP);
                &*physics
            }
            Bodies::Play => match document.world_mut().system_mut::<Simulation>() {
                Some(simulation) => simulation.physics(),
                None => return false,
            },
        };
        match self.ragdoll.pose_into(physics, self.model, &mut self.pose) {
            Ok(()) => self.palette.compute(&rig.skeleton, &self.pose),
            Err(RagdollError::BodyCount) => return false,
            Err(error) => crcbl::log::warn!("editor: the drop preview kept its pose: {error}"),
        }
        true
    }

    /// The skinning palette of the pose the bodies stand in.
    pub(super) fn palette(&self) -> &[Mat4] {
        self.palette.matrices()
    }

    /// The entities its bodies are, for a test to take one away.
    #[cfg(test)]
    pub(super) fn entities(&self) -> &[Entity] {
        self.ragdoll.entities()
    }

    /// Where each body stands now, for a test to watch it fall.
    #[cfg(test)]
    pub(super) fn heights(&self, document: &mut Document) -> Vec<f64> {
        let physics = match &self.bodies {
            Bodies::Own(physics) => Some(&**physics),
            Bodies::Play => document
                .world_mut()
                .system_mut::<Simulation>()
                .map(|simulation| simulation.physics()),
        };
        physics
            .and_then(|physics| self.ragdoll.body_transforms(physics))
            .unwrap_or_default()
            .iter()
            .map(|transform| transform.position.y)
            .collect()
    }
}

/// The ragdoll of `rig` and where a mesh of it whose origin stands at
/// `origin` hangs its skin, or [`None`] for a rig with no fitted ragdoll or
/// nowhere to hang it.
fn hung(rig: &Rig, origin: Mat4) -> Option<(&RagdollRig, Mat4)> {
    let ragdoll = rig.ragdoll.as_ref()?;
    Some((ragdoll, origin * rig.skinned.instances.first()?.transform))
}

/// `ragdoll`, fitted to `rig`, spawned into `physics` at `pose` placed by
/// `model`, as `entities`.
fn spawn(
    physics: &mut PhysicsSystem,
    ragdoll: &RagdollRig,
    rig: &Rig,
    pose: &Pose,
    model: Mat4,
    entities: &[Entity],
) -> Result<Ragdoll, SpawnError> {
    let handoff = Handoff {
        pose,
        model,
        previous: None,
    };
    Ragdoll::spawn(physics, ragdoll, &rig.skeleton, &handoff, entities)
}
