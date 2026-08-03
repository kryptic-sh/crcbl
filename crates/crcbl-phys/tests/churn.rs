//! Property tests for dynamic broadphase churn.
//!
//! The [`Bvh`] can be inserted into and removed from without a rebuild, which
//! is what lets a game spawn and kill colliders every tick. Everything that
//! can go wrong with that is invisible to a spot check: a lost element still
//! answers most queries correctly, and a tree that has degenerated into a
//! linked list answers *all* of them correctly, just slowly. So these tests
//! check the two things a spot check cannot —
//!
//! * **exactness**: after every operation in a long random sequence, every
//!   query returns exactly what a brute-force scan of the live set returns;
//! * **quality**: the tree's depth stays logarithmic in the element count
//!   across that sequence, including the input that degenerates it worst.

use std::collections::HashSet;

use crcbl_phys::{Aabb, Bvh, PhysicsWorld, Segment, Sphere};
use glam::DVec3;

// ---------------------------------------------------------------------------
// Deterministic randomness
// ---------------------------------------------------------------------------

/// The engine's index hash, under this file's shorter names.
///
/// The reason it is an index hash rather than a generator matters as much to a
/// churn test as to a game: the value depends only on the index, never on how
/// much of the test has run, so a failure at operation 431 is reproducible by
/// replaying the seed rather than by having kept the whole history.
use crcbl_core::rand::{hash_u64 as mix, hash_unit as unit};

/// A box of half-extent `half` somewhere in a cube `span` across.
fn random_box(seed: u64, index: u64, span: f64, half: f64) -> Aabb {
    let centre = DVec3::new(
        (unit(seed, index * 3) - 0.5) * span,
        (unit(seed, index * 3 + 1) - 0.5) * span,
        (unit(seed, index * 3 + 2) - 0.5) * span,
    );
    Aabb::from_centre_half(centre, DVec3::splat(half))
}

// ---------------------------------------------------------------------------
// Exactness against brute force
// ---------------------------------------------------------------------------

/// What the live set says the answer to an AABB query is, with no tree in the
/// picture at all.
fn brute_force_aabb(live: &[(usize, u32, Aabb)], query: &Aabb) -> Vec<u32> {
    let mut ids: Vec<u32> = live
        .iter()
        .filter(|(_, _, aabb)| aabb.intersects(query))
        .map(|(_, id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids
}

fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids
}

#[test]
fn bvh_churn_answers_exactly_what_brute_force_does() {
    // Start from a bulk build, then churn: that is the shape of a real frame
    // loop, which builds a level's colliders once and then spawns and kills
    // bullets against it.
    const INITIAL: usize = 64;
    const OPS: u64 = 600;

    let mut live: Vec<(usize, u32, Aabb)> = Vec::new();
    let elements: Vec<(Aabb, u32)> = (0..INITIAL)
        .map(|i| (random_box(1, i as u64, 200.0, 1.5), i as u32))
        .collect();
    let mut bvh = Bvh::build(elements.clone());
    for (i, (aabb, id)) in elements.iter().enumerate() {
        live.push((i, *id, *aabb));
    }

    let mut next_id = INITIAL as u32;
    for op in 0..OPS {
        // Remove roughly as often as insert, so the population stays bounded
        // while both paths get exercised; never remove from an empty set.
        let removing = !live.is_empty() && mix(2, op).is_multiple_of(2);
        if removing {
            let pick = (mix(3, op) as usize) % live.len();
            let (element_index, _, _) = live.swap_remove(pick);
            assert!(
                bvh.remove(element_index),
                "op {op}: removing live element {element_index} reported failure"
            );
        } else {
            let aabb = random_box(4, op, 200.0, 1.5);
            let element_index = bvh.insert(aabb, next_id);
            live.push((element_index, next_id, aabb));
            next_id += 1;
        }

        assert_eq!(bvh.len(), live.len(), "op {op}: element count diverged");

        // Query with boxes of several sizes, so both the "hits nearly
        // everything" and the "hits nothing" ends are covered.
        for probe in 0..4u64 {
            let half = [0.5, 5.0, 40.0, 500.0][probe as usize];
            let query = random_box(5, op * 4 + probe, 200.0, half);
            assert_eq!(
                sorted(bvh.traverse_aabb(&query)),
                brute_force_aabb(&live, &query),
                "op {op} probe {probe}: AABB query disagreed with brute force"
            );
        }

        // And with a segment, which descends the same tree by a different rule.
        let start = DVec3::new(
            (unit(6, op * 2) - 0.5) * 200.0,
            (unit(6, op * 2 + 1) - 0.5) * 200.0,
            0.0,
        );
        let end = start + DVec3::new(unit(7, op) * 100.0 - 50.0, unit(7, op + 1) * 100.0, 0.0);
        let segment = Segment::new(start, end);
        let hits = sorted(
            bvh.traverse_segment(&segment)
                .into_iter()
                .map(|hit| hit.element_id)
                .collect(),
        );
        let expected = brute_force_segment(&live, &segment);
        assert_eq!(
            hits, expected,
            "op {op}: segment query disagreed with brute force"
        );
    }

    // The sequence really did both things, or the assertions above proved
    // nothing about the paths they were meant to cover.
    assert!(
        next_id as usize > INITIAL,
        "the sequence never inserted anything"
    );
    assert!(
        live.len() < next_id as usize,
        "the sequence never removed anything"
    );
}

/// Brute-force segment answer: the same slab test the tree runs at each node,
/// applied to every live element instead.
fn brute_force_segment(live: &[(usize, u32, Aabb)], segment: &Segment) -> Vec<u32> {
    let dir = segment.dir();
    if dir.length() <= 0.0 {
        return Vec::new();
    }
    let inv_dir = dir.recip();
    let dir_is_neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
    let mut ids: Vec<u32> = live
        .iter()
        .filter(|(_, _, aabb)| aabb.intersect_ray(segment.start, inv_dir, dir_is_neg, 0.0, 1.0))
        .map(|(_, id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn removed_elements_are_gone_and_their_indices_recycled() {
    // A tree that keeps answering with an element it was told to drop is the
    // failure this whole file exists to catch, so it gets a direct test too.
    let mut bvh = Bvh::build((0..8).map(|i| {
        (
            Aabb::from_centre_half(DVec3::new(i as f64 * 10.0, 0.0, 0.0), DVec3::splat(1.0)),
            100 + i as u32,
        )
    }));
    let everything = Aabb::from_centre_half(DVec3::new(35.0, 0.0, 0.0), DVec3::splat(1000.0));

    assert_eq!(bvh.traverse_aabb(&everything).len(), 8);
    assert!(bvh.remove(3));
    assert!(!bvh.traverse_aabb(&everything).contains(&103));
    assert_eq!(bvh.len(), 7);

    // A second removal of the same element is a no-op, not a corruption: a
    // caller sweeping deferred destructions can ask twice.
    assert!(!bvh.remove(3));
    assert_eq!(bvh.len(), 7);

    // The freed index comes back, and now names the new element.
    let reused = bvh.insert(
        Aabb::from_centre_half(DVec3::new(-500.0, 0.0, 0.0), DVec3::splat(1.0)),
        999,
    );
    assert_eq!(reused, 3, "the freed element index should be recycled");
    let ids = bvh.traverse_aabb(&Aabb::from_centre_half(
        DVec3::new(-500.0, 0.0, 0.0),
        DVec3::splat(2.0),
    ));
    assert_eq!(ids, vec![999]);
}

// ---------------------------------------------------------------------------
// Tree quality
// ---------------------------------------------------------------------------

/// The depth an AVL-balanced tree over `leaves` elements cannot exceed.
///
/// A BVH over `L` leaves has `2L-1` nodes. AVL's height bound is
/// `h ≤ 1.4405·log2(N+2) - 0.33` in edges; `1.5·log2(2L) + 1` counted in nodes
/// is that bound with the constants rounded outward, so a tree that violates
/// this is not merely worse than measured — it is not height-balanced at all.
///
/// Depth is the quality observable because it is what query cost is bounded
/// by: a traversal descends at most this far before it reaches leaves, so a
/// depth that grows linearly is a broadphase that has stopped being one. The
/// alternative observables — node count, nodes visited per query — either
/// cannot degrade (node count is exactly `2L-1` by construction) or vary with
/// the query as much as with the tree.
fn avl_depth_bound(leaves: usize) -> f64 {
    if leaves <= 1 {
        return 1.0;
    }
    1.5 * ((2 * leaves) as f64).log2() + 1.0
}

#[test]
fn bvh_depth_stays_logarithmic_under_churn() {
    const TARGET: usize = 512;
    const OPS: u64 = 4_000;

    let mut bvh = Bvh::build((0..TARGET).map(|i| (random_box(11, i as u64, 400.0, 1.0), i as u32)));
    let mut live: Vec<usize> = (0..TARGET).collect();
    let bound = avl_depth_bound(TARGET);
    let mut peak = bvh.depth();

    // Steady state: kill one, spawn one, forever. This is asteroids' bullet
    // stream, and it is the sequence under which an unbalanced tree rots —
    // each insertion picks a site by bounds alone, and nothing ever undoes a
    // bad choice.
    for (next, op) in (TARGET as u64..).zip(0..OPS) {
        let pick = (mix(12, op) as usize) % live.len();
        let element_index = live.swap_remove(pick);
        assert!(bvh.remove(element_index));
        live.push(bvh.insert(random_box(11, next, 400.0, 1.0), next as u32));

        let depth = bvh.depth();
        peak = peak.max(depth);
        assert!(
            (depth as f64) <= bound,
            "op {op}: depth {depth} exceeded the AVL bound {bound:.1} for \
             {TARGET} elements"
        );
    }
    assert_eq!(bvh.len(), TARGET, "churn changed the population size");
    // Assert the run actually built a deep tree: a bound is not evidence if
    // the tree it bounds is trivial.
    assert!(peak >= 9, "peak depth {peak} — the tree never got deep");
}

#[test]
fn bvh_depth_survives_coincident_elements() {
    // The degenerate input for a surface-area heuristic: every box in the same
    // place, so every candidate site costs the same and the heuristic has no
    // signal at all. Before the tree was balanced on the way back up, 1024
    // coincident boxes under 20k churn operations reached depth 623 against an
    // ideal of 11 — effectively a linked list. This is the test that says it
    // cannot come back.
    const TARGET: usize = 256;
    const OPS: u64 = 3_000;

    let same_place = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
    let mut bvh = Bvh::build((0..TARGET).map(|i| (same_place, i as u32)));
    let mut live: Vec<usize> = (0..TARGET).collect();
    let bound = avl_depth_bound(TARGET);

    for op in 0..OPS {
        let pick = (mix(13, op) as usize) % live.len();
        assert!(bvh.remove(live.swap_remove(pick)));
        live.push(bvh.insert(same_place, TARGET as u32 + op as u32));
        let depth = bvh.depth();
        assert!(
            (depth as f64) <= bound,
            "op {op}: coincident elements drove depth to {depth}, past the \
             AVL bound {bound:.1}"
        );
    }

    // And it still answers: a balanced tree that lost elements is no better
    // than a degenerate one that kept them.
    let ids = bvh.traverse_aabb(&same_place);
    assert_eq!(ids.len(), TARGET);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn churn_is_bit_identical_across_runs() {
    // The crate's whole f64-everywhere rule exists so that two runs of the same
    // inputs agree exactly. The BVH is part of that: the insertion heuristic
    // compares floating-point areas, so a tie broken differently would put an
    // element somewhere else, and the *order* query results come back in would
    // change even though the set did not.
    let run = || {
        let mut bvh =
            Bvh::build((0..128).map(|i| (random_box(21, i as u64, 300.0, 2.0), i as u32)));
        let mut live: Vec<usize> = (0..128).collect();
        for op in 0..500u64 {
            let pick = (mix(22, op) as usize) % live.len();
            assert!(bvh.remove(live.swap_remove(pick)));
            live.push(bvh.insert(random_box(21, 128 + op, 300.0, 2.0), 128 + op as u32));
        }
        // Unsorted, on purpose: tree order is the thing being pinned.
        let probe = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(60.0));
        (bvh.depth(), bvh.len(), bvh.traverse_aabb(&probe))
    };

    let (depth_a, len_a, hits_a) = run();
    let (depth_b, len_b, hits_b) = run();
    assert_eq!(depth_a, depth_b, "tree shape differed between runs");
    assert_eq!(len_a, len_b);
    assert_eq!(hits_a, hits_b, "query results differed between runs");
    assert!(!hits_a.is_empty(), "the probe hit nothing — nothing pinned");
}

// ---------------------------------------------------------------------------
// PhysicsWorld: sphere overlap against the churned broadphase
// ---------------------------------------------------------------------------

#[test]
fn world_overlap_sphere_matches_brute_force_under_churn() {
    // `PhysicsWorld::overlap_sphere` is the entry point asteroids uses for
    // ship-vs-rock: broadphase cull by the query sphere's AABB, then an exact
    // sphere-vs-sphere test on each candidate. It existed before this slice;
    // what is new is that the adds and removes underneath it no longer throw
    // the tree away, so this is the test that the two together still agree
    // with a scan.
    const OPS: u64 = 400;

    let mut world = PhysicsWorld::new();
    let mut live: Vec<(crcbl_phys::ColliderId, Sphere)> = Vec::new();

    for i in 0..40u64 {
        let centre = DVec3::new(
            (unit(31, i * 2) - 0.5) * 100.0,
            (unit(31, i * 2 + 1) - 0.5) * 100.0,
            0.0,
        );
        let sphere = Sphere::new(centre, 1.0 + unit(32, i) * 3.0);
        live.push((world.add_sphere(sphere), sphere));
    }
    // Force the tree to exist, so every subsequent add/remove takes the
    // incremental path rather than being absorbed by a lazy rebuild.
    let _ = world.overlap_sphere(DVec3::ZERO, 1.0);

    for op in 0..OPS {
        if !live.is_empty() && mix(33, op).is_multiple_of(2) {
            let pick = (mix(34, op) as usize) % live.len();
            let (id, _) = live.swap_remove(pick);
            assert!(world.remove(id), "op {op}: removing a live collider failed");
        } else {
            let centre = DVec3::new(
                (unit(35, op * 2) - 0.5) * 100.0,
                (unit(35, op * 2 + 1) - 0.5) * 100.0,
                0.0,
            );
            let sphere = Sphere::new(centre, 1.0 + unit(36, op) * 3.0);
            live.push((world.add_sphere(sphere), sphere));
        }
        assert_eq!(world.len(), live.len(), "op {op}: collider count diverged");

        let query_centre = DVec3::new(
            (unit(37, op * 2) - 0.5) * 100.0,
            (unit(37, op * 2 + 1) - 0.5) * 100.0,
            0.0,
        );
        let query_radius = 2.0 + unit(38, op) * 20.0;
        let query = Sphere::new(query_centre, query_radius);

        let got: HashSet<_> = world
            .overlap_sphere(query_centre, query_radius)
            .into_iter()
            .collect();
        let expected: HashSet<_> = live
            .iter()
            .filter(|(_, sphere)| {
                // Exact sphere-vs-sphere, the same predicate the narrow phase
                // applies — written out here so the test does not check the
                // code against itself.
                let delta = sphere.centre - query.centre;
                delta.length_squared() <= (sphere.radius + query.radius).powi(2)
            })
            .map(|(id, _)| *id)
            .collect();

        assert_eq!(
            got, expected,
            "op {op}: overlap_sphere disagreed with brute force"
        );
    }
}
