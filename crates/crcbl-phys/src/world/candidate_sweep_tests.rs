use std::f64::consts::FRAC_PI_6;

use crcbl_core::rand::hash_unit;
use glam::DVec3;

use crate::broadphase::Segment;
use crate::collider::{BoxCollider, Capsule, Sphere};
use crate::components::Transform;
use crate::compound_shape::{CompoundPart, CompoundShape};
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;
use crate::query::ShapeHit;

use super::{ColliderId, PhysicsWorld, QueryFilter, QueryScratch};

/// How closely a fraction worked out by hand has to agree with a closed-form
/// sweep's: only rounding separates them.
const EXACT: f64 = 1e-12;

/// The layer the fixtures put their item on.
const ITEMS: u32 = 1 << 2;

/// The capsule every capsule fixture sweeps: [`crate::CharacterConfig`]'s
/// default shape.
const RADIUS: f64 = 0.3;
const HALF_HEIGHT: f64 = 0.6;

type Hits = Vec<(ColliderId, ShapeHit)>;

fn capsule_all(world: &mut PhysicsWorld, segment: &Segment, filter: QueryFilter) -> Hits {
    let mut hits = Vec::new();
    world.sweep_capsule_all(segment, RADIUS, HALF_HEIGHT, filter, &mut hits);
    hits
}

fn sphere_all(world: &mut PhysicsWorld, segment: &Segment, radius: f64) -> Hits {
    let mut hits = Vec::new();
    world.sweep_sphere_all(segment, radius, QueryFilter::ALL, &mut hits);
    hits
}

fn ids(hits: &Hits) -> Vec<ColliderId> {
    hits.iter().map(|&(id, _)| id).collect()
}

/// A wall filling `x >= near_x`, tall and wide enough that nothing here gets
/// past its ends.
fn wall_from(world: &mut PhysicsWorld, near_x: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(near_x + 1.0, 0.0, 0.0),
        DVec3::new(1.0, 5.0, 10.0),
    ))
}

/// **A ceiling met before a wall is reported, and so is the wall.** The
/// closest sweep stops at the ceiling; EW's ceiling-and-braking case needs the
/// wall behind it as well.
#[test]
fn a_ceiling_met_before_a_wall_is_reported_and_so_is_the_wall() {
    let mut world = PhysicsWorld::new();
    // The capsule's top is at 0.9; the ceiling's underside 0.3 above it.
    let ceiling = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 2.2, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    let wall = wall_from(&mut world, 2.0);
    let path = Segment::new(DVec3::ZERO, DVec3::new(3.0, 1.0, 0.0));

    let hits = capsule_all(&mut world, &path, QueryFilter::ALL);

    let [(first, roof), (second, face)] = hits[..] else {
        panic!("a ceiling and a wall, not {hits:?}");
    };
    assert_eq!((first, roof.normal), (ceiling, DVec3::NEG_Y));
    assert!(
        (roof.t - 0.3).abs() < EXACT,
        "the rise of 0.3 is met at {roof:?}"
    );
    assert_eq!((second, face.normal), (wall, DVec3::NEG_X));
    let flank = (2.0 - RADIUS) / 3.0;
    assert!(
        (face.t - flank).abs() < EXACT,
        "the flank reaches x = 2 at {flank}, and the wall was met at {face:?}"
    );
    assert!(!roof.started_inside && !face.started_inside);
    assert_eq!(
        world.sweep_capsule(&path, RADIUS, HALF_HEIGHT),
        Some(hits[0]),
        "the closest sweep's answer is the first"
    );
}

/// **A wall the sweep starts out grazing is flagged, and the wall ahead is
/// still reported.** A tangent wall is a hit at `t = 0` that the closest
/// sweep returns and stops at.
#[test]
fn a_grazed_wall_is_flagged_and_the_wall_ahead_still_reported() {
    let mut world = PhysicsWorld::new();
    let radius = 0.5;
    // Its -Z face is at z = radius: the sphere touches it all the way along.
    let side = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 0.0, radius + 1.0),
        DVec3::new(10.0, 5.0, 1.0),
    ));
    let ahead = wall_from(&mut world, 3.0);
    let path = Segment::new(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0));

    let hits = sphere_all(&mut world, &path, radius);

    let [(first, grazed), (second, met)] = hits[..] else {
        panic!("the grazed wall and the one ahead, not {hits:?}");
    };
    assert_eq!(first, side);
    assert!(grazed.started_inside && grazed.t == 0.0, "{grazed:?}");
    assert_eq!((second, met.normal), (ahead, DVec3::NEG_X));
    assert!(!met.started_inside);
    assert!((met.t - (3.0 - radius) / 5.0).abs() < EXACT, "{met:?}");
    assert_eq!(world.sweep_sphere(&path, radius), Some(hits[0]));
}

/// **Stopping short of a wall is not a wall hit.** A sweep that ends a
/// little before the wall reports the ceiling it does meet and nothing past
/// its own end — EW's braking case that stops short.
#[test]
fn a_wall_past_the_end_of_the_sweep_is_not_reported() {
    let mut world = PhysicsWorld::new();
    let ceiling = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 2.2, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    let wall = wall_from(&mut world, 2.0);
    // The flank stops a hundredth short of the wall's face.
    let short = 2.0 - RADIUS - 0.01;
    let path = Segment::new(DVec3::ZERO, DVec3::new(short, 1.0, 0.0));

    assert_eq!(
        ids(&capsule_all(&mut world, &path, QueryFilter::ALL)),
        [ceiling]
    );
    // And the wall is there to be met: a hundredth further, it is.
    let longer = Segment::new(DVec3::ZERO, DVec3::new(short + 0.02, 1.0, 0.0));
    assert_eq!(
        ids(&capsule_all(&mut world, &longer, QueryFilter::ALL)),
        [ceiling, wall]
    );
}

/// **A reused buffer holds only this sweep's hits.** A game keeps one `Vec`
/// across ticks, so whatever the last sweep left in it must not survive into
/// the next answer — for either shape.
#[test]
fn a_reused_buffer_holds_only_this_sweeps_hits() {
    let mut world = PhysicsWorld::new();
    let wall = wall_from(&mut world, 2.0);
    let path = Segment::new(DVec3::ZERO, DVec3::new(4.0, 1.0, 0.0));
    let stale = sphere_all(&mut world, &path, RADIUS);
    assert_eq!(ids(&stale), [wall], "the fixture meets its wall");

    let mut hits = stale.clone();
    hits.extend(stale.iter().copied());
    world.sweep_sphere_all(&path, RADIUS, QueryFilter::ALL, &mut hits);
    assert_eq!(ids(&hits), [wall], "the sphere sweep kept a stale hit");

    hits.extend(stale.iter().copied());
    world.sweep_capsule_all(&path, RADIUS, HALF_HEIGHT, QueryFilter::ALL, &mut hits);
    assert_eq!(ids(&hits), [wall], "the capsule sweep kept a stale hit");
}

/// **Triggers, masked colliders and the excluded one are left out, and
/// everything behind them is kept** — the closest sweep's rules, which the
/// list must not loosen.
#[test]
fn triggers_masked_and_excluded_colliders_are_left_out() {
    let mut world = PhysicsWorld::new();
    let body = world.add_capsule(Capsule::new(DVec3::ZERO, RADIUS, HALF_HEIGHT));
    let trigger = world.add_box(BoxCollider::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.2, 2.0, 2.0),
    ));
    assert!(world.set_trigger(trigger, true));
    let item = world.add_box(BoxCollider::new(
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(0.2, 2.0, 2.0),
    ));
    assert!(world.set_layers(item, ITEMS));
    let wall = wall_from(&mut world, 4.0);
    let path = Segment::new(DVec3::ZERO, DVec3::new(6.0, 0.0, 0.0));

    let everything = capsule_all(&mut world, &path, QueryFilter::ALL);
    assert_eq!(
        ids(&everything),
        [body, item, wall],
        "a trigger is never solid"
    );
    assert!(
        everything[0].1.started_inside,
        "the body is where it starts"
    );

    let movement = QueryFilter::excluding(Some(body)).with_mask(!ITEMS);
    let filtered = capsule_all(&mut world, &path, movement);
    assert_eq!(ids(&filtered), [wall]);
    assert_eq!(
        filtered[0], everything[2],
        "leaving colliders out does not move the wall behind them"
    );
    assert_eq!(
        world.sweep_capsule_filtered(&path, RADIUS, HALF_HEIGHT, movement),
        Some(filtered[0])
    );

    // The trigger is in the way, so skipping it is a branch the run took.
    assert!(world.set_trigger(trigger, false));
    assert_eq!(
        ids(&capsule_all(&mut world, &path, QueryFilter::ALL)),
        [body, trigger, item, wall]
    );
}

/// **A turned box reports its turned face**, to both sweeps.
#[test]
fn a_turned_box_reports_its_turned_face() {
    let mut world = PhysicsWorld::new();
    let turn = rotation_from_scaled_axis(DVec3::Y * FRAC_PI_6);
    let slab = world.add_box(
        BoxCollider::new(DVec3::new(2.0, 0.0, 0.0), DVec3::new(0.25, 2.0, 5.0)).with_rotation(turn),
    );
    let wall = wall_from(&mut world, 4.0);
    let path = Segment::new(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0));
    let face = turn * DVec3::NEG_X;

    for hits in [
        capsule_all(&mut world, &path, QueryFilter::ALL),
        sphere_all(&mut world, &path, RADIUS),
    ] {
        assert_eq!(ids(&hits), [slab, wall]);
        let met = hits[0].1;
        assert!(
            (met.normal - face).length() < 1e-9,
            "met {:?}, the turned face is {face:?}",
            met.normal
        );
        assert!(!met.started_inside && met.t > 0.0 && met.t < hits[1].1.t);
    }
}

/// **Hits at the same fraction come in collider index order, and the closest
/// sweep picks the first of them.** Two boxes share the face the sweep meets
/// along the edge between them; the lower index was added last, into a
/// recycled slot, so neither insertion order nor the tree's can stand in for
/// the rule.
#[test]
fn hits_at_the_same_fraction_come_in_collider_index_order() {
    let mut world = PhysicsWorld::new();
    let placeholder = world.add_sphere(Sphere::new(DVec3::new(0.0, 50.0, 0.0), 1.0));
    // Both faces at x = 3, one above y = 0 and one below it: a sphere on the
    // axis touches both at once.
    let below = world.add_box(BoxCollider::new(
        DVec3::new(4.0, -2.0, 0.0),
        DVec3::new(1.0, 2.0, 2.0),
    ));
    assert!(world.remove(placeholder));
    let above = world.add_box(BoxCollider::new(
        DVec3::new(4.0, 2.0, 0.0),
        DVec3::new(1.0, 2.0, 2.0),
    ));
    assert!(above.index() < below.index(), "the slot was recycled");
    let path = Segment::new(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0));

    for radius in [0.25, 0.5, 1.0] {
        let hits = sphere_all(&mut world, &path, radius);
        assert_eq!(ids(&hits), [above, below], "radius {radius}");
        assert_eq!(hits[0].1.t, hits[1].1.t, "a tie, at radius {radius}");
        assert_eq!(world.sweep_sphere(&path, radius), Some(hits[0]));
    }
}

/// A collider placed by one draw of [`random_scene`].
enum Placed {
    Sphere(Sphere),
    Box(BoxCollider),
    Capsule(Capsule),
    Mesh(TriangleMesh, Transform),
    Compound(CompoundShape, Transform),
}

/// A value in `[low, high)` from draw `index` of `seed`.
fn draw(seed: u64, index: &mut u64, low: f64, high: f64) -> f64 {
    *index += 1;
    low + (high - low) * hash_unit(seed, *index)
}

fn point(seed: u64, index: &mut u64, reach: f64) -> DVec3 {
    DVec3::new(
        draw(seed, index, -reach, reach),
        draw(seed, index, -reach, reach),
        draw(seed, index, -reach, reach),
    )
}

/// Two boxes side by side with a gap between them, the second turned in the
/// shape's frame, as an item's parts are.
fn item(size: f64, turn: glam::DQuat) -> CompoundShape {
    CompoundShape::new(vec![
        CompoundPart::new(
            DVec3::new(-size, 0.0, 0.0),
            glam::DQuat::IDENTITY,
            DVec3::new(size * 0.5, size, size * 0.5),
        ),
        CompoundPart::new(
            DVec3::new(size, 0.0, 0.0),
            turn,
            DVec3::new(size * 0.5, size * 0.5, size),
        ),
    ])
    .expect("a valid shape")
}

/// A tilted quad, two triangles, as a mesh collider would hold a ramp.
fn ramp() -> TriangleMesh {
    TriangleMesh::new(
        &[
            DVec3::new(-1.0, 0.0, -1.0),
            DVec3::new(1.0, 0.0, -1.0),
            DVec3::new(1.0, 0.5, 1.0),
            DVec3::new(-1.0, 0.5, 1.0),
        ],
        &[[0, 2, 1], [0, 3, 2]],
    )
    .expect("a quad is a valid mesh")
}

/// One collider per [`Placed`] kind in turn, scattered through a cube, each
/// on a layer of its own so a mask can ask for it alone; every seventh is a
/// trigger, so the triggers fall on every kind in turn.
fn random_scene(seed: u64, count: u32) -> (PhysicsWorld, Vec<(ColliderId, bool)>) {
    let mut world = PhysicsWorld::new();
    let mut placed = Vec::new();
    let mut index = 0;
    for n in 0..count {
        let centre = point(seed, &mut index, 4.0);
        let size = draw(seed, &mut index, 0.2, 1.2);
        let turn = rotation_from_scaled_axis(point(seed, &mut index, 1.5));
        let shape = match n % 6 {
            0 => Placed::Sphere(Sphere::new(centre, size)),
            1 => Placed::Box(BoxCollider::new(centre, DVec3::splat(size))),
            2 => Placed::Box(
                BoxCollider::new(centre, DVec3::new(size, size * 0.5, size * 1.5))
                    .with_rotation(turn),
            ),
            3 => Placed::Capsule(Capsule::new(centre, size * 0.5, size)),
            4 => Placed::Mesh(ramp(), Transform::new(centre, turn)),
            _ => Placed::Compound(item(size, turn), Transform::new(centre, turn)),
        };
        let id = match shape {
            Placed::Sphere(s) => world.add_sphere(s),
            Placed::Box(b) => world.add_box(b),
            Placed::Capsule(c) => world.add_capsule(c),
            Placed::Mesh(m, at) => world.add_mesh(m, at),
            Placed::Compound(shape, at) => world.add_compound(&shape, DVec3::ZERO, &at),
        };
        assert!(world.set_layers(id, 1 << n));
        let trigger = n % 7 == 3;
        assert!(world.set_trigger(id, trigger));
        placed.push((id, trigger));
    }
    (world, placed)
}

/// Every bit a hit carries, so two compare as identical rather than equal.
fn bits(hit: Option<(ColliderId, ShapeHit)>) -> Option<(ColliderId, [u64; 7], bool, usize)> {
    hit.map(|(id, hit)| {
        let [x, y, z] = hit.point.to_array().map(f64::to_bits);
        let [nx, ny, nz] = hit.normal.to_array().map(f64::to_bits);
        (
            id,
            [hit.t.to_bits(), x, y, z, nx, ny, nz],
            hit.started_inside,
            hit.part,
        )
    })
}

/// **The first hit is the closest sweep's answer, to the bit, and every hit
/// is the one that collider gives on its own.** Random scenes of spheres,
/// boxes turned and not, capsules, meshes, compounds and triggers, swept by
/// spheres and capsules along random segments.
///
/// The list is held to two oracles that share no ordering code with it: the
/// closest sweep, for which comes first, and the closest sweep masked down to
/// each collider's own layer, for what each entry holds and whether it should
/// be there at all. A list that dropped a hidden candidate, kept a trigger,
/// mislabelled a hit or ordered two wrongly fails one of them.
#[test]
fn the_first_hit_is_the_closest_sweeps_and_each_hit_its_colliders_own() {
    let count = 20;
    let mut scratch = QueryScratch::new();
    let (mut most, mut started_inside, mut sweeps) = (0, 0, 0);
    // Hits per kind of collider, in `random_scene`'s order of kinds, so a
    // kind the sweeps never met is a gap the run reports.
    let mut met = [0_usize; 6];
    // Compound entries past each compound's first: the parts behind a part.
    let mut later_parts = 0;
    for seed in 0..40_u64 {
        let (mut world, placed) = random_scene(seed, count);
        let mut index = 1_000;
        for _ in 0..25 {
            let start = point(seed, &mut index, 6.0);
            let end = point(seed, &mut index, 6.0);
            let segment = Segment::new(start, end);
            let radius = draw(seed, &mut index, 0.05, 0.8);
            let half_height = draw(seed, &mut index, 0.0, 1.0);
            let excluded = placed[(seed as usize + sweeps) % placed.len()].0;

            for capsule in [false, true] {
                let mut all = Vec::new();
                let mut shared = Vec::new();
                let mut filtered = Vec::new();
                let filter = QueryFilter::excluding(Some(excluded));
                let closest = |world: &mut PhysicsWorld, filter| {
                    if capsule {
                        world.sweep_capsule_filtered(&segment, radius, half_height, filter)
                    } else {
                        world.sweep_sphere_filtered(&segment, radius, filter)
                    }
                };
                if capsule {
                    world.sweep_capsule_all(
                        &segment,
                        radius,
                        half_height,
                        QueryFilter::ALL,
                        &mut all,
                    );
                    world.sweep_capsule_all(&segment, radius, half_height, filter, &mut filtered);
                    world.overlap_queries().sweep_capsule_all(
                        &segment,
                        radius,
                        half_height,
                        QueryFilter::ALL,
                        &mut scratch,
                        &mut shared,
                    );
                } else {
                    world.sweep_sphere_all(&segment, radius, QueryFilter::ALL, &mut all);
                    world.sweep_sphere_all(&segment, radius, filter, &mut filtered);
                    world.overlap_queries().sweep_sphere_all(
                        &segment,
                        radius,
                        QueryFilter::ALL,
                        &mut scratch,
                        &mut shared,
                    );
                }
                let context = format!("seed {seed}, {segment:?}, capsule {capsule}");

                assert_eq!(
                    bits(all.first().copied()),
                    bits(closest(&mut world, QueryFilter::ALL)),
                    "{context}"
                );
                assert_eq!(
                    bits(filtered.first().copied()),
                    bits(closest(&mut world, filter)),
                    "{context}"
                );
                let without: Hits = all
                    .iter()
                    .copied()
                    .filter(|&(id, _)| id != excluded)
                    .collect();
                assert_eq!(filtered, without, "{context}");
                assert_eq!(shared, all, "the shared view is the same query: {context}");

                for pair in all.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    assert!(
                        a.1.t < b.1.t
                            || (a.1.t == b.1.t
                                && (a.0.index(), a.1.part) < (b.0.index(), b.1.part)),
                        "out of order: {pair:?}, {context}"
                    );
                }
                for (n, &(id, trigger)) in placed.iter().enumerate() {
                    let alone = closest(&mut world, QueryFilter::masked(1 << n));
                    let listed = all.iter().copied().find(|&(listed, _)| listed == id);
                    assert_eq!(bits(listed), bits(alone), "collider {n}, {context}");
                    if trigger {
                        assert_eq!(listed, None, "a trigger, {context}");
                    }
                    met[n % met.len()] += usize::from(listed.is_some());
                    later_parts += all
                        .iter()
                        .filter(|&&(listed, _)| listed == id)
                        .count()
                        .saturating_sub(1);
                }
                for &(_, hit) in &all {
                    assert!((0.0..=1.0).contains(&hit.t), "{hit:?}, {context}");
                }
                most = most.max(all.len());
                started_inside += all.iter().filter(|(_, hit)| hit.started_inside).count();
            }
            sweeps += 1;
        }
    }
    assert!(
        most >= 4,
        "the busiest sweep met {most} colliders, so ordering was barely tested"
    );
    assert!(
        met.iter().all(|&hits| hits > 0),
        "hits per kind (sphere, box, turned box, capsule, mesh, compound): {met:?}"
    );
    assert!(
        later_parts > 0,
        "no sweep met a second part of one compound, so per-part entries went untested"
    );
    assert!(
        started_inside > 0,
        "no sweep started inside anything, so the flagged entries went untested"
    );
}
