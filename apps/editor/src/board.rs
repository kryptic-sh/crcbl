//! The one scene vocabulary slice 1 knows: `apps/breakout`'s board.
//!
//! A `.scn/` directory is a manifest of system names and one RON chunk file per
//! system, and reading a chunk needs the **type** its rows are of — that is the
//! bound on [`crcbl::scene::scn::chunk_of`] and the reason a scene cannot be
//! opened by a tool that knows no components. `docs/plan/08-editor.md`'s
//! missing-pieces list has no component registry in the tree, so this module is
//! the registry: a hand-written list of what the editor can open, with one
//! entry.
//!
//! That entry is `crcbl_breakout::Brick` — `apps/breakout`'s, whose lib is named
//! `crcbl_breakout` — which is where `#[derive(Reflect)]` landed
//! first and which that file names as "the first component an editor edits".
//!
//! # What this module owns, and what a registry would take over
//!
//! Four things, and only the fourth is really breakout's:
//!
//! 1. the chunk codecs [`crcbl::scene::scn::Scene::load`] and
//!    [`crcbl::scene::scn::Scene::save`] are given;
//! 2. registering the systems a loaded world needs, [`PhysicsSystem`] included
//!    — an entity with no collider does not pick;
//! 3. reaching one entity's component as `&mut dyn Reflect`, which is what an
//!    [`EditCommand`](crate::command::EditCommand) is applied to;
//! 4. what a brick's *placement* is — its centre and its half extents — which
//!    is what a collider and a bounds box are built from.
//!
//! A registry replaces 1–3 with a lookup by system name. **4 is the open
//! question**, and it is the plan's, not this module's: a component says which
//! of its fields are editable and says nothing about which of them is a
//! position. `crcbl::phys::Transform` on the entity would answer it for every
//! component at once, and that is a scene-format change — the plan's missing
//! piece 6 — rather than something to invent here.

use crcbl::ecs::{Entity, System, World};
use crcbl::math::DVec3;
use crcbl::phys::{ColliderComponent, PhysicsSystem, RigidBody, Transform};
use crcbl::reflect::Reflect;
use crcbl::scene::scn::{IdMap, SystemChunk, chunk_of};

pub use crcbl_breakout::Brick;

/// The manifest entry, the chunk file's stem and the name the codec and the
/// system are both registered under.
///
/// The same string `apps/breakout/src/scene.rs` spells, because it is the same
/// file being read.
pub const BRICKS: &str = "bricks";

/// The codecs [`crcbl::scene::scn::Scene::load`] and
/// [`crcbl::scene::scn::Scene::save`] read and write this vocabulary's chunk
/// files with.
#[must_use]
pub fn codecs() -> Vec<Box<dyn SystemChunk>> {
    vec![chunk_of::<Brick>(BRICKS)]
}

/// Registers the systems a board is loaded into: one per chunk, plus the
/// physics system every pick goes through.
///
/// The order is the order [`crcbl::ecs::Schedule`] will hold them in, and it is
/// not load-bearing: nothing here ticks. The editor never calls
/// [`World::tick`], because a scene being edited is not a scene being
/// simulated — `docs/plan/08-editor.md`'s edit-mode schedule is the missing
/// piece that would make that a switch rather than an omission.
pub fn register(world: &mut World) {
    world.register_system(Box::new(System::<Brick>::new(BRICKS)));
    world.register_system(Box::new(PhysicsSystem::new()));
}

/// `entity`'s component, for an [`EditCommand`](crate::command::EditCommand) to
/// be applied to.
///
/// [`None`] for an entity this vocabulary holds nothing for, which is what an
/// id from another scene or a despawned one gets.
///
/// `&mut World` although a read would do: [`crcbl::ecs::SystemTrait`] exposes
/// `as_any_mut` and no shared `as_any`, so the only way to reach a `System<T>`
/// by name is through a unique borrow. `crcbl_scene::scn::SystemChunk::write`
/// carries the same note for the same reason.
pub fn component(world: &mut World, entity: Entity) -> Option<&mut dyn Reflect> {
    let brick = world.system_mut::<System<Brick>>()?.get_mut(entity)?;
    Some(brick)
}

/// Where `entity` stands and how far it reaches, in simulation space.
///
/// The centre and the **half** extents, which is what both a box collider and a
/// debug-draw box want. [`None`] for an entity this vocabulary holds nothing
/// for.
#[must_use]
pub fn placement(world: &mut World, entity: Entity) -> Option<(DVec3, DVec3)> {
    let brick = world.system_mut::<System<Brick>>()?.get(entity)?;
    Some((brick.position(), brick.half_extents()))
}

/// Gives every entity in `ids` the collider a ray picks it by, replacing any it
/// already had.
///
/// Called once after a load and again after any edit that moved or resized the
/// thing edited — **not** only after a load. A collider left where the brick
/// used to be is the failure this exists to prevent, and it is one a picture
/// would not show: the brick draws in its new place and picks in its old one.
///
/// Kinematic, like `apps/breakout`'s own bricks: the body is what the broadphase
/// tracks, and nothing integrates it because nothing here ticks.
pub fn sync_colliders(world: &mut World, entities: impl IntoIterator<Item = Entity>) {
    let placements: Vec<(Entity, DVec3, DVec3)> = entities
        .into_iter()
        .filter_map(|entity| {
            placement(world, entity).map(|(centre, half_extents)| (entity, centre, half_extents))
        })
        .collect();
    let Some(phys) = world.system_mut::<PhysicsSystem>() else {
        return;
    };
    for (entity, centre, half_extents) in placements {
        let transform = Transform::from_position(centre);
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_transform(entity, transform);
        phys.set_collider(
            entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents,
                is_trigger: false,
            },
            &transform,
        );
    }
}

/// The entities the system called `system` holds, in the order the file spells
/// them.
///
/// Empty for a name this vocabulary does not know — which a loaded scene cannot
/// contain, because [`crcbl::scene::scn::Scene::load`] refuses a manifest entry
/// with no codec, and which is still the honest answer rather than a panic.
///
/// The sort is what makes the order file order: [`IdMap`] is keyed by
/// [`crcbl::scene::scn::SceneEntityId`], while `System::iter_entities` yields
/// storage order, which swap-remove makes a function of attach history.
#[must_use]
pub fn entities(world: &mut World, ids: &IdMap, system: &str) -> Vec<Entity> {
    if system != BRICKS {
        return Vec::new();
    }
    let Some(system) = world.system_mut::<System<Brick>>() else {
        return Vec::new();
    };
    let mut rows: Vec<_> = system
        .iter_entities()
        .filter_map(|(entity, _)| ids.id(entity).map(|id| (id, entity)))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().map(|(_, entity)| entity).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The codec list and the registered systems agree on the name, which is
    /// what [`crcbl::scene::scn::Scene::load`] joins them by — a manifest entry
    /// whose codec and whose system disagree is a load error rather than a
    /// silently empty chunk.
    #[test]
    fn the_codec_and_the_system_are_registered_under_one_name() {
        let codecs = codecs();
        assert_eq!(codecs.len(), 1);
        assert_eq!(codecs[0].name(), BRICKS);

        let mut world = World::new();
        register(&mut world);
        let names: Vec<String> = world
            .schedule_mut()
            .iter_mut()
            .map(|system| system.name().to_owned())
            .collect();
        assert!(names.contains(&BRICKS.to_owned()), "{names:?}");
    }

    /// A component that is not this vocabulary's answers `None` rather than
    /// panicking or reaching some other entity's row.
    #[test]
    fn an_entity_with_no_brick_has_no_component_and_no_placement() {
        let mut world = World::new();
        register(&mut world);
        let stranger = world.spawn();
        assert!(component(&mut world, stranger).is_none());
        assert!(placement(&mut world, stranger).is_none());
    }
}
