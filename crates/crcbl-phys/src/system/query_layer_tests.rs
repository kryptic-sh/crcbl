use crcbl_ecs::Entity;
use glam::DVec3;

use crate::collider::{Aabb, Sphere};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::compound_shape::CompoundShape;
use crate::world::{ALL_LAYERS, QueryFilter, QueryScratch};
use crate::{Ray, Segment};

use super::PhysicsSystem;

/// The layer the fixtures put their item on.
const ITEMS: u32 = 1 << 4;
/// The layer the fixtures put their wall on — not the default, which would
/// put the wall on [`ITEMS`] as well.
const LEVEL: u32 = 1 << 0;

fn entity(idx: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(idx)).expect("test entity")
}

fn slab(half_extents: DVec3) -> ColliderComponent {
    ColliderComponent::Box {
        offset: DVec3::ZERO,
        half_extents,
        is_trigger: false,
    }
}

/// An item entity at `x = 2` on [`ITEMS`] in front of a wall entity at `x = 6`
/// on [`LEVEL`], both static and across the +X axis.
fn scene() -> (PhysicsSystem, Entity, Entity) {
    let mut phys = PhysicsSystem::new();
    let (item, wall) = (entity(0), entity(1));
    phys.set_collider(
        item,
        &slab(DVec3::new(0.5, 2.0, 2.0)),
        &Transform::from_position(DVec3::new(2.0, 0.0, 0.0)),
    );
    phys.set_collider(
        wall,
        &slab(DVec3::new(0.5, 5.0, 5.0)),
        &Transform::from_position(DVec3::new(6.0, 0.0, 0.0)),
    );
    assert!(phys.set_collider_layers(item, ITEMS));
    assert!(phys.set_collider_layers(wall, LEVEL));
    (phys, item, wall)
}

fn ray() -> Ray {
    Ray::new(DVec3::ZERO, DVec3::X)
}

#[test]
fn the_system_queries_skip_a_masked_out_entity_and_find_it_when_masked_in() {
    let (mut phys, item, wall) = scene();
    let path = Segment::new(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
    let id = |hit: Option<(Entity, _)>| hit.map(|(e, _)| e);
    let (no_items, items) = (QueryFilter::masked(!ITEMS), QueryFilter::masked(ITEMS));

    assert_eq!(id(phys.cast_ray(&ray())), Some(item));
    assert_eq!(id(phys.cast_ray_filtered(&ray(), no_items)), Some(wall));
    assert_eq!(id(phys.cast_ray_filtered(&ray(), items)), Some(item));
    assert_eq!(id(phys.sweep_sphere(&path, 0.3)), Some(item));
    assert_eq!(
        id(phys.sweep_sphere_filtered(&path, 0.3, no_items)),
        Some(wall)
    );
    assert_eq!(
        id(phys.sweep_sphere_filtered(&path, 0.3, items)),
        Some(item)
    );

    let (centre, radius) = (DVec3::new(3.5, 0.0, 0.0), 2.5);
    assert_eq!(phys.overlap_sphere(centre, radius).len(), 2);
    assert_eq!(
        phys.overlap_sphere_filtered(centre, radius, no_items),
        vec![wall]
    );
    let mut out = Vec::new();
    phys.overlap_sphere_filtered_into(centre, radius, items, &mut out);
    assert_eq!(out, vec![item]);
    let bounds = Aabb::new(DVec3::new(0.0, -1.0, -1.0), DVec3::new(10.0, 1.0, 1.0));
    assert_eq!(phys.overlap_aabb(&bounds).len(), 2);
    assert_eq!(phys.overlap_aabb_filtered(&bounds, no_items), vec![wall]);
    assert_eq!(phys.overlap_aabb_filtered(&bounds, items), vec![item]);

    let mut scratch = QueryScratch::new();
    let view = phys.overlap_queries();
    assert_eq!(
        id(view.cast_ray_filtered(&ray(), no_items, &mut scratch)),
        Some(wall)
    );
    assert_eq!(
        id(view.cast_ray_filtered(&ray(), items, &mut scratch)),
        Some(item)
    );
    assert_eq!(
        id(view.sweep_sphere_filtered(&path, 0.3, no_items, &mut scratch)),
        Some(wall)
    );
    assert_eq!(
        id(view.sweep_sphere_filtered(&path, 0.3, items, &mut scratch)),
        Some(item)
    );
    view.overlap_sphere_filtered_into(centre, radius, no_items, &mut scratch, &mut out);
    assert_eq!(out, vec![wall]);
    view.overlap_sphere_filtered_into(centre, radius, items, &mut scratch, &mut out);
    assert_eq!(out, vec![item]);
    view.overlap_aabb_filtered_into(&bounds, no_items, &mut scratch, &mut out);
    assert_eq!(out, vec![wall]);
    view.overlap_aabb_filtered_into(&bounds, items, &mut scratch, &mut out);
    assert_eq!(out, vec![item]);
}

/// A body swept along its own path leaves itself out whatever the mask, and
/// the mask decides what else it may meet.
#[test]
fn a_body_sweep_applies_its_mask_on_top_of_its_own_exclusion() {
    let (mut phys, item, wall) = scene();
    let bullet = entity(2);
    let mut body = RigidBody::new_kinematic();
    body.velocity = DVec3::new(10.0, 0.0, 0.0);
    phys.set_body(bullet, body);
    phys.set_collider(
        bullet,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 0.2,
            is_trigger: false,
        },
        &Transform::from_position(DVec3::new(10.0, 0.0, 0.0)),
    );
    let id = |hit: Option<(Entity, _)>| hit.map(|(e, _)| e);

    assert_eq!(id(phys.sweep_body(bullet, 1.0, 0.2)), Some(item));
    assert_eq!(
        id(phys.sweep_body_filtered(bullet, 1.0, 0.2, !ITEMS)),
        Some(wall)
    );
    assert_eq!(
        id(phys.sweep_body_filtered(bullet, 1.0, 0.2, 0)),
        None,
        "a mask of nothing meets nothing — and never the body itself"
    );
}

/// Layers are the entity's: a step that moves the collider keeps them, and so
/// does a `set_collider` that replaces it, with or without a
/// `remove_collider` in between.
#[test]
fn collider_layers_survive_motion_and_replacement() {
    let mut phys = PhysicsSystem::new();
    let item = entity(0);
    let mut body = RigidBody::new_kinematic();
    body.velocity = DVec3::new(0.0, 0.0, 1.0);
    phys.set_body(item, body);
    phys.set_collider(
        item,
        &slab(DVec3::splat(0.5)),
        &Transform::from_position(DVec3::new(2.0, 0.0, 0.0)),
    );
    assert!(phys.set_collider_layers(item, ITEMS));

    phys.step(0.25);
    let moved = phys.collider_of(item).expect("a collider");
    assert_eq!(phys.world().layers(moved), Some(ITEMS), "a step keeps them");
    assert!(
        phys.cast_ray_filtered(
            &Ray::new(DVec3::new(0.0, 0.0, 0.25), DVec3::X),
            QueryFilter::masked(!ITEMS)
        )
        .is_none(),
        "and a masked ray still passes the moved item"
    );

    let transform = *phys.transform(item).expect("registered");
    phys.set_collider(
        item,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 0.5,
            is_trigger: false,
        },
        &transform,
    );
    let replaced = phys.collider_of(item).expect("a collider");
    assert_ne!(replaced, moved, "a replacement is a new collider");
    assert_eq!(
        phys.world().layers(replaced),
        Some(ITEMS),
        "and is put back on the entity's layers"
    );

    phys.remove_collider(item);
    assert_eq!(phys.collider_of(item), None);
    assert!(
        !phys.set_collider_layers(item, LEVEL),
        "an entity with no collider has nothing to tag"
    );
    phys.set_collider(item, &slab(DVec3::splat(0.5)), &transform);
    let again = phys.collider_of(item).expect("a collider");
    assert_eq!(
        phys.world().layers(again),
        Some(ITEMS),
        "a collider set after remove_collider keeps them too"
    );

    phys.remove_entity(item);
    phys.set_collider(item, &slab(DVec3::splat(0.5)), &transform);
    let fresh = phys.collider_of(item).expect("a collider");
    assert_eq!(
        phys.world().layers(fresh),
        Some(ALL_LAYERS),
        "remove_entity forgets them"
    );
}

#[test]
fn a_compound_is_tagged_through_its_one_query_box() {
    let mut phys = PhysicsSystem::new();
    let crate_ = entity(0);
    let shape = CompoundShape::from_aabbs(&[
        Aabb::new(DVec3::new(-0.5, -0.5, -0.5), DVec3::new(0.0, 0.5, 0.5)),
        Aabb::new(DVec3::new(0.0, -0.5, -0.5), DVec3::new(0.5, 0.5, 0.5)),
    ])
    .expect("valid parts");
    phys.set_collider(
        crate_,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape,
            is_trigger: false,
        },
        &Transform::from_position(DVec3::new(2.0, 0.0, 0.0)),
    );
    assert!(phys.set_collider_layers(crate_, ITEMS));
    let query_box = phys.collider_of(crate_).expect("the query box");
    assert_eq!(phys.world().len(), 1, "a compound is one query collider");
    assert_eq!(phys.world().layers(query_box), Some(ITEMS));
    assert_eq!(phys.entity_of(query_box), Some(crate_));
    assert!(
        phys.cast_ray_filtered(&ray(), QueryFilter::masked(!ITEMS))
            .is_none()
    );
    assert_eq!(
        phys.cast_ray_filtered(&ray(), QueryFilter::masked(ITEMS))
            .map(|(e, _)| e),
        Some(crate_)
    );
}

#[test]
fn collider_of_and_entity_of_round_trip_and_forget_stale_ids() {
    let (mut phys, item, wall) = scene();
    let item_collider = phys.collider_of(item).expect("a collider");
    let wall_collider = phys.collider_of(wall).expect("a collider");
    assert_eq!(phys.entity_of(item_collider), Some(item));
    assert_eq!(phys.entity_of(wall_collider), Some(wall));
    assert_eq!(phys.collider_of(entity(9)), None, "an unknown entity");

    // What the world's own queries answer with maps back to the entity.
    let (hit, _) = phys
        .world_mut()
        .cast_ray_filtered(&ray(), QueryFilter::excluding(Some(item_collider)))
        .expect("the wall behind the excluded item");
    assert_eq!(phys.entity_of(hit), Some(wall));

    phys.set_collider(
        item,
        &slab(DVec3::splat(0.25)),
        &Transform::from_position(DVec3::new(2.0, 0.0, 0.0)),
    );
    let replaced = phys.collider_of(item).expect("a collider");
    assert_eq!(phys.entity_of(replaced), Some(item));
    assert_eq!(
        phys.entity_of(item_collider),
        None,
        "the id a replacement retired names nothing, even with its slot reused"
    );

    phys.remove_entity(wall);
    assert_eq!(phys.collider_of(wall), None);
    assert_eq!(phys.entity_of(wall_collider), None);
    let stray = phys
        .world_mut()
        .add_sphere(Sphere::new(DVec3::new(0.0, 9.0, 0.0), 0.5));
    assert_eq!(
        stray.index(),
        wall_collider.index(),
        "the removed slot is recycled"
    );
    assert_eq!(
        phys.entity_of(stray),
        None,
        "a collider added straight to the world is no entity's"
    );
    assert_eq!(phys.entity_of(wall_collider), None);
}
