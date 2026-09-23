use glam::DVec3;

use crate::broadphase::{Ray, Segment};
use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};

use super::{ALL_LAYERS, ColliderId, PhysicsWorld, QueryFilter, QueryScratch};

/// The layer the fixtures put their item on.
const ITEMS: u32 = 1 << 2;
/// The layer the fixtures put their wall on. Not the default: a collider on
/// [`ALL_LAYERS`] is on [`ITEMS`] too, and a mask of `ITEMS` would report it.
const LEVEL: u32 = 1 << 0;

/// An item box on [`ITEMS`] in front of a wall on [`LEVEL`], both across the
/// +X axis: whatever runs along +X from the origin meets the item first unless
/// the filter looks past it.
struct Scene {
    world: PhysicsWorld,
    item: ColliderId,
    wall: ColliderId,
}

fn scene() -> Scene {
    let mut world = PhysicsWorld::new();
    let item = world.add_box(BoxCollider::new(
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(0.5, 2.0, 2.0),
    ));
    assert!(world.set_layers(item, ITEMS));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(0.5, 5.0, 5.0),
    ));
    assert!(world.set_layers(wall, LEVEL));
    Scene { world, item, wall }
}

fn ray() -> Ray {
    Ray::new(DVec3::ZERO, DVec3::X)
}

fn path() -> Segment {
    Segment::new(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
}

#[test]
fn a_ray_skips_a_masked_out_layer_and_hits_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let id = |hit: Option<(ColliderId, _)>| hit.map(|(id, _)| id);

    assert_eq!(
        id(world.cast_ray(&ray())),
        Some(item),
        "default sees every layer"
    );
    assert_eq!(
        id(world.cast_ray_filtered(&ray(), QueryFilter::masked(!ITEMS))),
        Some(wall),
        "a mask without the item's layer must look past it to the wall"
    );
    assert_eq!(
        id(world.cast_ray_filtered(&ray(), QueryFilter::masked(ITEMS))),
        Some(item)
    );
    let view = world.overlap_queries();
    assert_eq!(
        id(view.cast_ray_filtered(&ray(), QueryFilter::masked(!ITEMS), &mut scratch)),
        Some(wall),
        "the shared view is the same query"
    );
    assert_eq!(
        id(view.cast_ray_filtered(&ray(), QueryFilter::masked(ITEMS), &mut scratch)),
        Some(item)
    );
}

#[test]
fn a_sphere_sweep_skips_a_masked_out_layer_and_hits_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let id = |hit: Option<(ColliderId, _)>| hit.map(|(id, _)| id);

    assert_eq!(id(world.sweep_sphere(&path(), 0.3)), Some(item));
    assert_eq!(
        id(world.sweep_sphere_filtered(&path(), 0.3, QueryFilter::masked(!ITEMS))),
        Some(wall)
    );
    assert_eq!(
        id(world.sweep_sphere_filtered(&path(), 0.3, QueryFilter::masked(ITEMS))),
        Some(item)
    );
    let view = world.overlap_queries();
    assert_eq!(
        id(view.sweep_sphere_filtered(&path(), 0.3, QueryFilter::masked(!ITEMS), &mut scratch)),
        Some(wall)
    );
    assert_eq!(
        id(view.sweep_sphere_filtered(&path(), 0.3, QueryFilter::masked(ITEMS), &mut scratch)),
        Some(item)
    );
}

#[test]
fn a_capsule_sweep_skips_a_masked_out_layer_and_hits_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let id = |hit: Option<(ColliderId, _)>| hit.map(|(id, _)| id);
    let (radius, half_height) = (0.3, 0.6);

    assert_eq!(
        id(world.sweep_capsule(&path(), radius, half_height)),
        Some(item)
    );
    assert_eq!(
        id(world.sweep_capsule_filtered(&path(), radius, half_height, QueryFilter::masked(!ITEMS))),
        Some(wall)
    );
    assert_eq!(
        id(world.sweep_capsule_filtered(&path(), radius, half_height, QueryFilter::masked(ITEMS))),
        Some(item)
    );
    let view = world.overlap_queries();
    assert_eq!(
        id(view.sweep_capsule_filtered(
            &path(),
            radius,
            half_height,
            QueryFilter::masked(!ITEMS),
            &mut scratch
        )),
        Some(wall)
    );
    assert_eq!(
        id(view.sweep_capsule_filtered(
            &path(),
            radius,
            half_height,
            QueryFilter::masked(ITEMS),
            &mut scratch
        )),
        Some(item)
    );
}

#[test]
fn a_sphere_overlap_skips_a_masked_out_layer_and_reports_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let mut out = Vec::new();
    // Reaches both boxes: the item's face at x = 1.5, the wall's at x = 5.5.
    let (centre, radius) = (DVec3::new(3.5, 0.0, 0.0), 2.5);

    let mut all = world.overlap_sphere(centre, radius);
    all.sort_by_key(|id| id.index());
    assert_eq!(all, vec![item, wall], "default reports both");
    assert_eq!(
        world.overlap_sphere_filtered(centre, radius, QueryFilter::masked(!ITEMS)),
        vec![wall]
    );
    world.overlap_sphere_filtered_into(centre, radius, QueryFilter::masked(ITEMS), &mut out);
    assert_eq!(out, vec![item]);
    let view = world.overlap_queries();
    view.overlap_sphere_filtered_into(
        centre,
        radius,
        QueryFilter::masked(!ITEMS),
        &mut scratch,
        &mut out,
    );
    assert_eq!(out, vec![wall]);
    view.overlap_sphere_filtered_into(
        centre,
        radius,
        QueryFilter::masked(ITEMS),
        &mut scratch,
        &mut out,
    );
    assert_eq!(out, vec![item]);
}

#[test]
fn an_aabb_overlap_skips_a_masked_out_layer_and_reports_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let mut out = Vec::new();
    let bounds = Aabb::new(DVec3::new(0.0, -1.0, -1.0), DVec3::new(10.0, 1.0, 1.0));

    let mut all = world.overlap_aabb(&bounds);
    all.sort_by_key(|id| id.index());
    assert_eq!(all, vec![item, wall], "default reports both");
    assert_eq!(
        world.overlap_aabb_filtered(&bounds, QueryFilter::masked(!ITEMS)),
        vec![wall]
    );
    assert_eq!(
        world.overlap_aabb_filtered(&bounds, QueryFilter::masked(ITEMS)),
        vec![item]
    );
    let view = world.overlap_queries();
    view.overlap_aabb_filtered_into(&bounds, QueryFilter::masked(!ITEMS), &mut scratch, &mut out);
    assert_eq!(out, vec![wall]);
    view.overlap_aabb_filtered_into(&bounds, QueryFilter::masked(ITEMS), &mut scratch, &mut out);
    assert_eq!(out, vec![item]);
}

#[test]
fn capsule_penetrations_skip_a_masked_out_layer_and_report_it_when_masked_in() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let mut scratch = QueryScratch::new();
    let mut out = Vec::new();
    // Straddles the item's +X face and the wall's -X face at once.
    let capsule = Capsule::new(DVec3::new(3.75, 0.0, 0.0), 2.0, 0.5);
    let ids = |out: &Vec<(ColliderId, _)>| {
        let mut ids: Vec<_> = out.iter().map(|(id, _)| *id).collect();
        ids.sort_by_key(|id| id.index());
        ids
    };

    world.capsule_penetrations_into(&capsule, None, &mut out);
    assert_eq!(ids(&out), vec![item, wall], "default reports both");
    world.capsule_penetrations_filtered_into(&capsule, QueryFilter::masked(!ITEMS), &mut out);
    assert_eq!(ids(&out), vec![wall]);
    world.capsule_penetrations_filtered_into(&capsule, QueryFilter::masked(ITEMS), &mut out);
    assert_eq!(ids(&out), vec![item]);
    let view = world.overlap_queries();
    view.capsule_penetrations_filtered_into(
        &capsule,
        QueryFilter::masked(!ITEMS),
        &mut scratch,
        &mut out,
    );
    assert_eq!(ids(&out), vec![wall]);
    view.capsule_penetrations_filtered_into(
        &capsule,
        QueryFilter::masked(ITEMS),
        &mut scratch,
        &mut out,
    );
    assert_eq!(ids(&out), vec![item]);
}

/// The two halves of a filter narrow together: the exclusion drops one
/// collider the mask admits, and the mask drops what the exclusion leaves.
#[test]
fn an_exclusion_and_a_mask_combine() {
    let Scene {
        mut world,
        item,
        wall,
    } = scene();
    let second = world.add_box(BoxCollider::new(
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(0.5, 2.0, 2.0),
    ));
    assert!(world.set_layers(second, ITEMS));
    let id = |hit: Option<(ColliderId, _)>| hit.map(|(id, _)| id);

    let items_but_first = QueryFilter::masked(ITEMS).with_exclude(Some(item));
    assert_eq!(
        id(world.cast_ray_filtered(&ray(), items_but_first)),
        Some(second),
        "excluding the first item leaves the second, and the mask still hides the wall"
    );
    assert_eq!(
        id(world.sweep_sphere_filtered(&path(), 0.3, items_but_first)),
        Some(second)
    );
    let no_items_no_wall = QueryFilter::excluding(Some(wall)).with_mask(!ITEMS);
    assert_eq!(
        id(world.cast_ray_filtered(&ray(), no_items_no_wall)),
        None,
        "the mask hides both items and the exclusion the wall"
    );
    assert_eq!(
        id(world.sweep_capsule_filtered(&path(), 0.3, 0.6, no_items_no_wall)),
        None
    );
    assert!(
        world
            .overlap_aabb_filtered(
                &Aabb::new(DVec3::new(0.0, -1.0, -1.0), DVec3::new(10.0, 1.0, 1.0)),
                no_items_no_wall,
            )
            .is_empty()
    );
}

/// A filter admitting a trigger's layer does not make the trigger solid, and
/// the overlap queries that report triggers still honour the mask on them.
#[test]
fn triggers_stay_non_solid_under_a_filter_and_overlaps_still_mask_them() {
    let Scene {
        mut world,
        item,
        wall: _,
    } = scene();
    let trigger = world.add_box(BoxCollider::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.25, 2.0, 2.0),
    ));
    assert!(world.set_trigger(trigger, true));
    assert!(world.set_layers(trigger, ITEMS));
    let id = |hit: Option<(ColliderId, _)>| hit.map(|(id, _)| id);
    let items = QueryFilter::masked(ITEMS);

    assert_eq!(id(world.cast_ray_filtered(&ray(), items)), Some(item));
    assert_eq!(
        id(world.sweep_sphere_filtered(&path(), 0.1, items)),
        Some(item)
    );
    assert_eq!(
        id(world.sweep_capsule_filtered(&path(), 0.1, 0.2, items)),
        Some(item)
    );
    let mut out = Vec::new();
    world.capsule_penetrations_filtered_into(
        &Capsule::new(DVec3::new(1.0, 0.0, 0.0), 0.1, 0.2),
        items,
        &mut out,
    );
    assert!(out.is_empty(), "a trigger has no push-out: {out:?}");

    let at_trigger = (DVec3::new(1.0, 0.0, 0.0), 0.1);
    assert_eq!(
        world.overlap_sphere_filtered(at_trigger.0, at_trigger.1, items),
        vec![trigger],
        "an overlap still reports a trigger its mask admits"
    );
    assert!(
        world
            .overlap_sphere_filtered(at_trigger.0, at_trigger.1, QueryFilter::masked(!ITEMS))
            .is_empty(),
        "and skips one it does not"
    );
}

#[test]
fn layers_survive_a_move_and_reset_when_the_slot_is_reused() {
    let mut world = PhysicsWorld::new();
    let item = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    assert_eq!(
        world.layers(item),
        Some(ALL_LAYERS),
        "the default is every bit"
    );
    assert!(world.set_layers(item, ITEMS));
    // Built tree, so the move is a refit rather than a rebuild.
    let _ = world.cast_ray(&ray());
    assert!(world.set_sphere(item, Sphere::new(DVec3::new(3.0, 0.0, 0.0), 0.5)));
    assert_eq!(world.layers(item), Some(ITEMS), "a move keeps the layers");
    assert!(
        world
            .cast_ray_filtered(&ray(), QueryFilter::masked(!ITEMS))
            .is_none(),
        "and the query still honours them after it"
    );

    assert!(world.remove(item));
    assert_eq!(world.layers(item), None, "a removed id has no layers");
    assert!(!world.set_layers(item, ITEMS), "and cannot be given any");
    let reused = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    assert_eq!(reused.index(), item.index(), "the slot must be recycled");
    assert_eq!(
        world.layers(reused),
        Some(ALL_LAYERS),
        "a recycled slot starts on every layer again"
    );
}
