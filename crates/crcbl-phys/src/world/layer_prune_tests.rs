//! **The broadphase's layer pruning changes no query's answer.**
//!
//! Every node of the query world's tree carries the union of its subtree's
//! layers, and a filtered query skips any subtree whose union shares no bit
//! with its mask ([`super::ResolvedFilter::candidates`]). These tests run every
//! filtered query family over random layered scenes twice — once on the tree
//! as it is, once on a copy whose unions are all saturated, which prunes
//! nothing — and require the same answers in the same order. A union that
//! drops a bit fails here as a missed hit.

use glam::DVec3;

use crate::broadphase::{Bvh, Ray, Segment};
use crate::collider::{Aabb, BoxCollider, Capsule, LyingCapsule, Sphere};
use crate::components::Transform;
use crate::integrator::rotation_from_scaled_axis;
use crate::query::AcceleratedPath;

use super::query_digest_tests::{draw, point, ramp};
use super::{
    ALL_LAYERS, ColliderId, LayerMatch, OverlapQueries, PhysicsWorld, QueryFilter, QueryScratch,
};

/// The layers the scenes deal from: four bits, so a random mask misses whole
/// subtrees, plus colliders left on [`ALL_LAYERS`] and on none at all.
fn dealt_layers(seed: u64, index: &mut u64) -> u32 {
    let roll = draw(seed, index, 0.0, 1.0);
    if roll < 0.1 {
        ALL_LAYERS
    } else if roll < 0.15 {
        0
    } else {
        1 << (draw(seed, index, 0.0, 4.0) as u32)
    }
}

fn add_dealt(world: &mut PhysicsWorld, seed: u64, index: &mut u64, kind: u32) -> ColliderId {
    let centre = point(seed, index, 6.0);
    let size = draw(seed, index, 0.2, 1.0);
    match kind % 4 {
        0 => world.add_sphere(Sphere::new(centre, size)),
        1 => world.add_box(BoxCollider::new(
            centre,
            DVec3::new(size, size * 0.5, size * 1.5),
        )),
        2 => world.add_capsule(Capsule::new(centre, size * 0.5, size)),
        _ => world.add_mesh(
            ramp(),
            Transform::new(centre, rotation_from_scaled_axis(point(seed, index, 1.5))),
        ),
    }
}

/// A layered scene, built, then churned through every path that touches the
/// tree's unions after a build: layer changes in place, refits, removals, and
/// inserts that are then moved onto a layer.
fn churned_scene(seed: u64) -> (PhysicsWorld, Vec<ColliderId>) {
    let mut world = PhysicsWorld::new();
    let mut index = 0;
    let mut ids = Vec::new();
    for n in 0..80 {
        let id = add_dealt(&mut world, seed, &mut index, n);
        assert!(world.set_layers(id, dealt_layers(seed, &mut index)));
        assert!(world.set_trigger(id, n % 7 == 3));
        ids.push(id);
    }
    world.rebuild();
    for step in 0..120u32 {
        let pick = (draw(seed, &mut index, 0.0, ids.len() as f64)) as usize;
        match step % 4 {
            0 => assert!(world.set_layers(ids[pick], dealt_layers(seed, &mut index))),
            1 => {
                assert!(world.set_sphere(ids[pick], Sphere::new(point(seed, &mut index, 6.0), 0.5)))
            }
            2 => {
                assert!(world.remove(ids[pick]));
                ids.swap_remove(pick);
            }
            _ => {
                let id = add_dealt(&mut world, seed, &mut index, step);
                assert!(world.set_layers(id, dealt_layers(seed, &mut index)));
                ids.push(id);
            }
        }
    }
    assert!(world.bvh.is_some(), "the churn kept the built tree");
    (world, ids)
}

/// Every filtered query family's answers to `count` random probes, written
/// out as text, and how many of them were a hit rather than nothing.
fn answers(view: OverlapQueries<'_>, ids: &[ColliderId], seed: u64) -> (Vec<String>, usize) {
    let mut scratch = QueryScratch::new();
    let mut index = 50_000;
    let mut out = Vec::new();
    let mut met = 0;
    let mut note = |out: &mut Vec<String>, hit: bool, text: String| {
        met += usize::from(hit);
        out.push(text);
    };
    let (mut hits, mut overlaps, mut boxed, mut pushes, mut normals) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for probe in 0..120 {
        let start = point(seed, &mut index, 7.0);
        let end = point(seed, &mut index, 7.0);
        let radius = draw(seed, &mut index, 0.05, 0.8);
        let half_height = draw(seed, &mut index, 0.0, 1.0);
        let segment = Segment::new(start, end);
        let mask = draw(seed, &mut index, 0.0, 16.0) as u32;
        let exclude = (probe % 3 == 0).then(|| ids[probe % ids.len()]);
        let layer_match = if probe % 2 == 0 {
            LayerMatch::Shares
        } else {
            LayerMatch::Within
        };
        let filter = QueryFilter::masked(mask)
            .with_exclude(exclude)
            .with_layer_match(layer_match);

        let ray = view.cast_ray_filtered(&Ray::new(start, end - start), filter, &mut scratch);
        note(&mut out, ray.is_some(), format!("ray {ray:?}"));
        let sphere = view.sweep_sphere_filtered(&segment, radius, filter, &mut scratch);
        note(&mut out, sphere.is_some(), format!("sphere {sphere:?}"));
        let capsule =
            view.sweep_capsule_filtered(&segment, radius, half_height, filter, &mut scratch);
        note(&mut out, capsule.is_some(), format!("capsule {capsule:?}"));
        view.sweep_sphere_all(&segment, radius, filter, &mut scratch, &mut hits);
        note(&mut out, !hits.is_empty(), format!("spheres {hits:?}"));
        view.sweep_capsule_all(
            &segment,
            radius,
            half_height,
            filter,
            &mut scratch,
            &mut hits,
        );
        note(&mut out, !hits.is_empty(), format!("capsules {hits:?}"));

        view.overlap_sphere_filtered_into(start, radius * 3.0, filter, &mut scratch, &mut overlaps);
        note(
            &mut out,
            !overlaps.is_empty(),
            format!("overlap {overlaps:?}"),
        );
        let aabb = Aabb::new(start.min(end), start.max(end));
        view.overlap_aabb_filtered_into(&aabb, filter, &mut scratch, &mut boxed);
        note(&mut out, !boxed.is_empty(), format!("boxed {boxed:?}"));

        let upright = Capsule::new(start, radius * 2.0, half_height);
        view.capsule_penetrations_filtered_into(&upright, filter, &mut scratch, &mut pushes);
        note(&mut out, !pushes.is_empty(), format!("pushes {pushes:?}"));
        normals.clear();
        view.capsule_contact_normals_into(&upright, filter, &mut scratch, &mut normals);
        note(
            &mut out,
            !normals.is_empty(),
            format!("normals {normals:?}"),
        );

        let yaw = draw(seed, &mut index, -3.0, 3.0);
        let lying = LyingCapsule::new(start, yaw, radius, half_height * 2.0);
        let blocker = view.lying_capsule_blocker(&lying, filter, &mut scratch);
        note(&mut out, blocker.is_some(), format!("blocker {blocker:?}"));
        let slid = view.sweep_lying_capsule(&lying, end - start, filter, &mut scratch);
        note(&mut out, slid.is_some(), format!("lying {slid:?}"));

        let path = AcceleratedPath::new(start, end - start, DVec3::new(0.0, -9.8, 0.0), 1.0);
        let arc = view.sweep_capsule_arc_where(
            &path,
            radius,
            half_height,
            filter,
            &mut scratch,
            |_, _| true,
        );
        note(&mut out, arc.is_some(), format!("arc {arc:?}"));
    }
    (out, met)
}

/// `bvh` with every node's union on every bit: a tree the masked walks
/// prune nothing from, short of a mask of `0` that every filter refuses
/// anyway.
fn unpruned(bvh: &Bvh) -> Bvh {
    let mut bvh = bvh.clone();
    bvh.saturate_layers();
    bvh
}

#[test]
fn every_filtered_query_answers_alike_with_and_without_pruning() {
    for seed in [3_u64, 0x51ed, 0xbead_cafe] {
        let (mut world, ids) = churned_scene(seed);
        let pruned = world.overlap_queries();
        let flat = unpruned(pruned.bvh);
        let unpruned_view = OverlapQueries {
            bvh: &flat,
            ..pruned
        };

        let (with, met) = answers(pruned, &ids, seed);
        let (without, _) = answers(unpruned_view, &ids, seed);
        assert_eq!(with.len(), without.len());
        for (a, b) in with.iter().zip(&without) {
            assert_eq!(a, b, "seed {seed:#x}: pruning changed an answer");
        }
        // Not a comparison of misses: the probes met things, under every
        // family, often enough for a wrong union to show.
        assert!(
            met * 4 > with.len(),
            "seed {seed:#x}: only {met} of {} answers were hits",
            with.len()
        );
    }
}

/// The stale-union case named: a tree built while no collider was on
/// `ITEMS`, so no node's union holds it, and then one collider moved onto it.
/// A union not brought up to date prunes that collider's whole path, and every
/// query for `ITEMS` misses it.
#[test]
fn a_collider_moved_onto_a_new_layer_after_the_build_is_still_found() {
    const LEVEL: u32 = 1 << 0;
    const ITEMS: u32 = 1 << 3;
    let mut world = PhysicsWorld::new();
    let mut ids = Vec::new();
    for i in 0..32 {
        let id = world.add_box(BoxCollider::new(
            DVec3::new(f64::from(i) * 3.0, 0.0, 0.0),
            DVec3::splat(0.5),
        ));
        assert!(world.set_layers(id, LEVEL));
        ids.push(id);
    }
    let target = ids[21];
    let ray = Ray::new(DVec3::new(63.0, 5.0, 0.0), DVec3::NEG_Y);
    let path = Segment::new(DVec3::new(63.0, 5.0, 0.0), DVec3::new(63.0, -5.0, 0.0));
    assert!(
        world
            .cast_ray_filtered(&ray, QueryFilter::masked(ITEMS))
            .is_none(),
        "nothing is on ITEMS yet; this query also builds the tree"
    );
    assert!(world.bvh.is_some());

    assert!(world.set_layers(target, ITEMS));
    assert_eq!(
        world
            .cast_ray_filtered(&ray, QueryFilter::masked(ITEMS))
            .map(|(id, _)| id),
        Some(target),
        "a ray for the new layer"
    );
    assert_eq!(
        world
            .sweep_sphere_filtered(&path, 0.25, QueryFilter::masked(ITEMS))
            .map(|(id, _)| id),
        Some(target),
        "a sweep for the new layer"
    );
    assert_eq!(
        world.overlap_aabb_filtered(
            &Aabb::from_centre_half(DVec3::new(63.0, 0.0, 0.0), DVec3::splat(1.0)),
            QueryFilter::within(ITEMS),
        ),
        [target],
        "an overlap for the new layer"
    );
    assert!(
        world
            .cast_ray_filtered(&ray, QueryFilter::masked(LEVEL))
            .is_none(),
        "and it left the old one"
    );

    // A collider inserted into the built tree, then moved onto a layer only
    // it is on.
    let late = world.add_sphere(Sphere::new(DVec3::new(200.0, 0.0, 0.0), 0.5));
    assert!(world.set_layers(late, 1 << 7));
    assert_eq!(
        world
            .cast_ray_filtered(
                &Ray::new(DVec3::new(200.0, 5.0, 0.0), DVec3::NEG_Y),
                QueryFilter::masked(1 << 7),
            )
            .map(|(id, _)| id),
        Some(late),
    );
    assert_eq!(world.broadphase_stats().rebuilds, 1, "all in place");
}
