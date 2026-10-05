//! The sphere overlaps answer hits: every collider kind measured through the
//! world, a compound by its deepest part, nothing for a collider the sphere
//! only touches, the filter honoured and the buffer refilled by every form,
//! the ids-only form naming exactly what the hit form does, and the same
//! world answering the same hits in the same order.

use core::f64::consts::FRAC_PI_2;

use crcbl_core::rand::hash_unit;
use glam::{DQuat, DVec3};

use crate::collider::{BoxCollider, Capsule, Sphere};
use crate::components::Transform;
use crate::compound_shape::{CompoundPart, CompoundShape};
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;
use crate::query::{self, OverlapHit, TurnedCapsule};

use super::tests::ids_of;
use super::{ColliderId, PhysicsWorld, QueryFilter, QueryScratch};

/// How closely a measured answer has to agree with one worked out by hand.
const EXACT: f64 = 1e-12;

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < EXACT
}

/// A flat two-triangle quad in its own `y = 0`, facing its own `+Y`.
fn quad() -> TriangleMesh {
    TriangleMesh::new(
        &[
            DVec3::new(-1.0, 0.0, -1.0),
            DVec3::new(1.0, 0.0, -1.0),
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(-1.0, 0.0, 1.0),
        ],
        &[[0, 2, 1], [0, 3, 2]],
    )
    .expect("a quad is a valid mesh")
}

/// The one hit an overlap at `centre` of `radius` found, for the collider it
/// was meant to find.
fn only_hit(world: &mut PhysicsWorld, centre: DVec3, radius: f64, id: ColliderId) -> OverlapHit {
    let hits = world.overlap_sphere(centre, radius);
    assert_eq!(ids_of(&hits), vec![id], "at {centre} r{radius}");
    hits[0].1
}

/// **Every collider kind answers through the world with the hit its own shape
/// gives**, spread apart so each query meets one: a sphere, a box, a turned
/// box, a standing capsule and a turned one, measured against the shape-level
/// functions, and a mesh, turned on its side, measured against the answer by
/// hand.
#[test]
fn every_collider_kind_answers_its_own_hit_through_the_world() {
    let mut world = PhysicsWorld::new();
    let sphere = Sphere::new(DVec3::new(0.0, 0.0, 0.0), 1.0);
    let boxed = BoxCollider::new(DVec3::new(10.0, 0.0, 0.0), DVec3::new(1.0, 2.0, 3.0));
    let turned_box = BoxCollider::new(DVec3::new(0.0, 0.0, 10.0), DVec3::ONE)
        .with_rotation(rotation_from_scaled_axis(DVec3::Y * 0.3));
    let standing = Capsule::new(DVec3::new(-10.0, 0.0, 0.0), 0.5, 1.0);
    let turn = rotation_from_scaled_axis(DVec3::Z * FRAC_PI_2);
    let lying = Capsule::new(DVec3::new(0.0, 0.0, -10.0), 0.5, 1.0);

    let ids = [
        world.add_sphere(sphere),
        world.add_box(boxed),
        world.add_box(turned_box),
        world.add_capsule(standing),
        world.add_turned_capsule(lying, turn),
    ];
    let probes = [
        Sphere::new(DVec3::new(1.2, 0.4, 0.0), 0.5),
        Sphere::new(DVec3::new(11.3, 2.5, 0.0), 0.75),
        Sphere::new(DVec3::new(0.9, 0.2, 10.9), 0.5),
        Sphere::new(DVec3::new(-9.0, 1.4, 0.0), 0.75),
        Sphere::new(DVec3::new(0.3, 0.8, -10.0), 0.5),
    ];
    let expected = [
        query::sphere_overlap_vs_sphere(&probes[0], &sphere),
        query::sphere_overlap_vs_box(&probes[1], &boxed),
        query::sphere_overlap_vs_box(&probes[2], &turned_box),
        query::sphere_overlap_vs_capsule(&probes[3], &standing),
        query::sphere_overlap_vs_turned_capsule(&probes[4], &TurnedCapsule::new(lying, turn)),
    ];
    for ((id, probe), expected) in ids.into_iter().zip(probes).zip(expected) {
        let expected = expected.expect("every probe is inside its shape");
        assert_eq!(
            only_hit(&mut world, probe.centre, probe.radius, id),
            expected
        );
    }

    // A quad turned a quarter about Z stands in the plane x = 20, its face —
    // its own +Y — turned to point along -X.
    let wall = world.add_mesh(quad(), Transform::new(DVec3::new(20.0, 0.0, 0.0), turn));
    let hit = only_hit(&mut world, DVec3::new(20.3, 0.25, 0.5), 0.5, wall);
    assert!(close(hit.point, DVec3::new(20.0, 0.25, 0.5)), "{hit:?}");
    assert!(close(hit.normal, DVec3::X), "{hit:?}");
    assert!((hit.depth - 0.2).abs() < EXACT, "{hit:?}");

    // A centre on a triangle has no side: it leaves along the triangle's own
    // normal, by the whole radius. Unturned, so the centre is on the plane to
    // the bit rather than a rounding to one side of it.
    let floor = world.add_mesh(quad(), Transform::from_position(DVec3::new(30.0, 0.0, 0.0)));
    let on = DVec3::new(30.25, 0.0, 0.5);
    let hit = only_hit(&mut world, on, 0.5, floor);
    assert!(close(hit.normal, DVec3::Y), "{hit:?}");
    assert!((hit.depth - 0.5).abs() < EXACT, "{hit:?}");
    assert!(close(hit.point, on), "{hit:?}");
}

/// **A compound answers with its deepest part**, named by index; of two parts
/// equally deep, the lower.
#[test]
fn a_compound_answers_with_its_deepest_part() {
    let part =
        |x: f64| CompoundPart::new(DVec3::new(x, 0.0, 0.0), DQuat::IDENTITY, DVec3::splat(0.5));
    let shape = CompoundShape::new(vec![part(-1.5), part(1.5)]).expect("a valid shape");
    let mut world = PhysicsWorld::new();
    let id = world.add_compound(&shape, DVec3::ZERO, &Transform::IDENTITY);

    // Nearer the right part's face at x = 1, and clear of the left's at -1.
    let right = only_hit(&mut world, DVec3::new(0.2, 0.0, 0.0), 1.25, id);
    assert_eq!(right.part, 1);
    assert!(close(right.normal, DVec3::NEG_X), "{right:?}");
    assert!((right.depth - 0.45).abs() < EXACT, "{right:?}");

    // Midway, a quarter into each: the lower part answers.
    let tie = only_hit(&mut world, DVec3::ZERO, 1.25, id);
    assert_eq!(tie.part, 0, "{tie:?}");
    assert!(close(tie.normal, DVec3::X), "{tie:?}");
    assert!(close(tie.point, DVec3::new(-1.0, 0.0, 0.0)), "{tie:?}");
}

/// **A collider the sphere only touches is not returned**, for each kind
/// whose touching distance is exact in `f64`; moved in by a hair, it is.
#[test]
fn a_collider_the_sphere_only_touches_is_not_returned() {
    let mut world = PhysicsWorld::new();
    world.add_sphere(Sphere::new(DVec3::ZERO, 2.0));
    world.add_box(BoxCollider::new(DVec3::new(10.0, 0.0, 0.0), DVec3::ONE));
    world.add_capsule(Capsule::new(DVec3::new(-10.0, 0.0, 0.0), 0.5, 1.0));
    world.add_mesh(quad(), Transform::from_position(DVec3::new(0.0, 0.0, 10.0)));

    // Each touch, and the step inward that makes it an overlap.
    let touches = [
        (DVec3::new(3.5, 0.0, 0.0), DVec3::NEG_X),
        (DVec3::new(10.0, 2.5, 0.0), DVec3::NEG_Y),
        (DVec3::new(-10.0, 3.0, 0.0), DVec3::NEG_Y),
        (DVec3::new(0.0, 1.5, 10.0), DVec3::NEG_Y),
    ];
    let hair = 1e-9;
    for (centre, inward) in touches {
        assert!(
            world.overlap_sphere(centre, 1.5).is_empty(),
            "a touch at {centre} was reported",
        );
        let hits = world.overlap_sphere(centre + inward * hair, 1.5);
        assert_eq!(hits.len(), 1, "an overlap at {centre} was missed");
        assert!(hits[0].1.depth > 0.0);
    }
}

/// An item on [`ITEMS`] and a wall on [`LEVEL`], both reached by
/// [`reach`], so every filtered form has something to keep and something to
/// leave out.
const ITEMS: u32 = 1 << 2;
const LEVEL: u32 = 1 << 0;

fn reach() -> (DVec3, f64) {
    (DVec3::new(3.5, 0.0, 0.0), 2.5)
}

fn layered() -> (PhysicsWorld, ColliderId, ColliderId) {
    let mut world = PhysicsWorld::new();
    let item = world.add_box(BoxCollider::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::splat(0.5),
    ));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::splat(0.5),
    ));
    assert!(world.set_layers(item, ITEMS));
    assert!(world.set_layers(wall, LEVEL));
    (world, item, wall)
}

/// **Every filtered form keeps what its filter admits, with the hit the
/// unfiltered query gives it** — by mask and by exclusion, owned, into a
/// buffer and through the shared view.
#[test]
fn the_filtered_forms_keep_what_the_filter_admits_with_its_hit() {
    let (mut world, item, wall) = layered();
    let (centre, radius) = reach();
    let all = world.overlap_sphere(centre, radius);
    let hit_of = |id: ColliderId| {
        all.iter()
            .find(|(found, _)| *found == id)
            .map(|&(_, hit)| hit)
            .expect("the unfiltered query reaches both")
    };
    let (item_only, wall_only) = (vec![(item, hit_of(item))], vec![(wall, hit_of(wall))]);

    for (filter, expected) in [
        (QueryFilter::masked(ITEMS), &item_only),
        (QueryFilter::masked(LEVEL), &wall_only),
        (QueryFilter::excluding(Some(item)), &wall_only),
        (QueryFilter::excluding(Some(wall)), &item_only),
    ] {
        assert_eq!(
            &world.overlap_sphere_filtered(centre, radius, filter),
            expected
        );
        let mut out = Vec::new();
        world.overlap_sphere_filtered_into(centre, radius, filter, &mut out);
        assert_eq!(&out, expected);
        let mut scratch = QueryScratch::new();
        world.overlap_queries().overlap_sphere_filtered_into(
            centre,
            radius,
            filter,
            &mut scratch,
            &mut out,
        );
        assert_eq!(&out, expected);
    }
}

/// **Every `_into` form refills its buffer rather than appending to it**: a
/// buffer arriving with something in it comes back with the answer and
/// nothing else.
#[test]
fn every_into_form_refills_its_buffer() {
    let (mut world, item, _) = layered();
    let (centre, radius) = reach();
    let owned = world.overlap_sphere(centre, radius);
    let stale = (item, owned[0].1);
    let mut scratch = QueryScratch::new();

    let mut out = vec![stale; 3];
    world.overlap_sphere_into(centre, radius, &mut out);
    assert_eq!(out, owned, "PhysicsWorld::overlap_sphere_into");

    let mut out = vec![stale; 3];
    world.overlap_sphere_filtered_into(centre, radius, QueryFilter::ALL, &mut out);
    assert_eq!(out, owned, "PhysicsWorld::overlap_sphere_filtered_into");

    let view = world.overlap_queries();
    let mut out = vec![stale; 3];
    view.overlap_sphere_into(centre, radius, &mut scratch, &mut out);
    assert_eq!(out, owned, "OverlapQueries::overlap_sphere_into");

    let mut out = vec![stale; 3];
    view.overlap_sphere_filtered_into(centre, radius, QueryFilter::ALL, &mut scratch, &mut out);
    assert_eq!(out, owned, "OverlapQueries::overlap_sphere_filtered_into");

    let mut ids = vec![item; 3];
    view.overlap_sphere_ids_into(centre, radius, &mut scratch, &mut ids);
    assert_eq!(
        ids,
        ids_of(&owned),
        "OverlapQueries::overlap_sphere_ids_into"
    );
}

/// A lattice of spheres with a capsule, a box and a mesh among them, so a
/// query lands on a neighbourhood of every kind.
fn crowd() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    for x in -3..=3 {
        for z in -3..=3 {
            let centre = DVec3::new(f64::from(x), 0.0, f64::from(z));
            world.add_sphere(Sphere::new(centre, 0.35));
        }
    }
    world.add_capsule(Capsule::new(DVec3::new(0.5, 0.0, 0.5), 0.3, 1.0));
    world.add_box(BoxCollider::new(
        DVec3::new(-1.5, 0.0, 1.5),
        DVec3::splat(0.4),
    ));
    world.add_mesh(quad(), Transform::from_position(DVec3::new(0.0, -0.2, 0.0)));
    world
}

/// The seed the crowd tests draw their queries from.
const PROBE_SEED: u64 = 0x006f_7665_726c_6170;

/// The queries every crowd test asks, drawn over the lattice and off its
/// points, so no answer sits on a boundary.
fn probes() -> impl Iterator<Item = (DVec3, f64)> {
    (0..40_u64).map(|n| {
        let draw =
            |k: u64, low: f64, high: f64| low + (high - low) * hash_unit(PROBE_SEED, n * 4 + k);
        let centre = DVec3::new(draw(0, -3.5, 3.5), draw(1, -0.5, 0.5), draw(2, -3.5, 3.5));
        (centre, draw(3, 0.2, 1.6))
    })
}

/// **The ids-only form names exactly the colliders the hit form does, in the
/// same order** — the property the crowd pass swapping one for the other
/// depends on.
#[test]
fn the_ids_form_names_what_the_hit_form_does_in_its_order() {
    let mut world = crowd();
    let mut scratch = QueryScratch::new();
    let (mut hits, mut ids) = (Vec::new(), Vec::new());
    let mut biggest = 0;
    let view = world.overlap_queries();
    for (centre, radius) in probes() {
        view.overlap_sphere_into(centre, radius, &mut scratch, &mut hits);
        view.overlap_sphere_ids_into(centre, radius, &mut scratch, &mut ids);
        assert_eq!(ids, ids_of(&hits), "at {centre} r{radius}");
        biggest = biggest.max(ids.len());
    }
    assert!(
        biggest > 4,
        "the widest query found {biggest}: not a neighbourhood"
    );
}

/// **The same world answers the same hits in the same order**: two worlds
/// built by one history — adds, a removal and a slot reused — agree on every
/// answer, value for value.
#[test]
fn the_same_world_answers_the_same_hits_in_the_same_order() {
    let build = || {
        let mut world = crowd();
        let doomed = world.add_sphere(Sphere::new(DVec3::new(0.2, 0.0, 0.2), 0.5));
        assert!(world.remove(doomed));
        world.add_box(BoxCollider::new(
            DVec3::new(0.7, 0.0, -0.7),
            DVec3::splat(0.3),
        ));
        world
    };
    let (mut first, mut second) = (build(), build());
    let mut met = 0;
    for (centre, radius) in probes() {
        let a = first.overlap_sphere(centre, radius);
        let b = second.overlap_sphere(centre, radius);
        assert_eq!(a, b, "at {centre} r{radius}");
        met += a.len();
    }
    assert!(met > 40, "only {met} hits were compared");
}
