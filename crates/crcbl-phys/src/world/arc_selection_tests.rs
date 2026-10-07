use glam::{DQuat, DVec3};

use crate::{
    AcceleratedPath, BoxCollider, Capsule, CompoundPart, CompoundShape, QueryScratch, Transform,
    TriangleMesh,
};

use super::{ColliderId, PhysicsWorld, QueryFilter};

const RADIUS: f64 = 0.25;
const HALF: f64 = 0.5;
const EXACT: f64 = 1e-9;

#[derive(Clone, Copy, Debug)]
enum Geometry {
    Separate,
    Compound,
    Mesh,
}

const GEOMETRIES: [Geometry; 3] = [Geometry::Separate, Geometry::Compound, Geometry::Mesh];

// Surfaces facing the capsule, backed by boxes or authored as mesh quads.
fn room(world: &mut PhysicsWorld, geometry: Geometry) -> [ColliderId; 3] {
    let boxes = [
        BoxCollider::new(DVec3::new(0.0, -1.0, 0.0), DVec3::new(10.0, 1.0, 10.0)),
        BoxCollider::new(DVec3::new(0.0, 3.0, 0.0), DVec3::new(10.0, 1.0, 10.0)),
        BoxCollider::new(DVec3::new(3.0, 0.0, 0.0), DVec3::new(1.0, 10.0, 0.5)),
    ];
    match geometry {
        Geometry::Separate => boxes.map(|b| world.add_box(b)),
        Geometry::Compound => {
            let parts = boxes.map(|b| CompoundPart::from_aabb(&b.aabb()));
            let shape = CompoundShape::new(parts.to_vec()).unwrap();
            let id = world.add_compound(&shape, DVec3::ZERO, &Transform::IDENTITY);
            [id; 3]
        }
        Geometry::Mesh => {
            let mut vertices = Vec::new();
            let mut triangles = Vec::new();
            for (centre, across, up) in [
                (DVec3::ZERO, DVec3::X * 10.0, DVec3::Z * 10.0),
                (DVec3::Y * 2.0, DVec3::X * 10.0, DVec3::Z * 10.0),
                (DVec3::X * 2.0, DVec3::Y * 10.0, DVec3::Z * 0.5),
            ] {
                let base = vertices.len() as u32;
                vertices.extend([
                    centre - across - up,
                    centre + across - up,
                    centre + across + up,
                    centre - across + up,
                ]);
                triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
            }
            let id = world.add_mesh(
                TriangleMesh::new(&vertices, &triangles).unwrap(),
                Transform::IDENTITY,
            );
            [id; 3]
        }
    }
}

#[test]
fn departing_floor_does_not_hide_ceiling_or_later_wall() {
    for geometry in GEOMETRIES {
        for bound in [false, true] {
            let mut world = PhysicsWorld::new();
            let [floor, ceiling, wall] = room(&mut world, geometry);
            let path = AcceleratedPath::new(
                DVec3::Y * (RADIUS + HALF - 0.001),
                DVec3::new(4.0, 4.0, 0.0),
                DVec3::NEG_Y * 8.0,
                1.0,
            );
            let capsule = Capsule::new(path.start, RADIUS, HALF);
            let own = bound.then(|| world.add_capsule(capsule));
            let filter = QueryFilter::excluding(own);
            let before = world.broadphase_stats();
            let first = world
                .sweep_capsule_arc(&path, RADIUS, HALF, filter)
                .unwrap();
            assert_eq!(first.0, floor);
            assert!(first.1.started_inside && first.1.time == 0.0);
            let incoming = |_: ColliderId, hit: &crate::ArcHit| {
                path.velocity_at(hit.time).dot(hit.normal) < 0.0
            };
            let selected = world
                .sweep_capsule_arc_where(&path, RADIUS, HALF, filter, incoming)
                .unwrap();
            assert_eq!(selected.0, ceiling, "{geometry:?}");
            assert!(selected.1.normal.distance(DVec3::NEG_Y) < EXACT);
            let ceiling_gap = 2.0 - (path.start.y + RADIUS + HALF);
            let expected = (4.0 - (16.0 - 16.0 * ceiling_gap).sqrt()) / 8.0;
            assert!((selected.1.time - expected).abs() < EXACT, "{selected:?}");
            assert!(!selected.1.started_inside);
            let wall_hit = world
                .sweep_capsule_arc_where(&path, RADIUS, HALF, filter, |_, hit| {
                    hit.normal.x < -0.9 && incoming(wall, hit)
                })
                .unwrap();
            assert_eq!(wall_hit.0, wall);
            assert!((wall_hit.1.time - (2.0 - RADIUS) / 4.0).abs() < EXACT);
            if matches!(geometry, Geometry::Compound) {
                assert_eq!(selected.1.part, 1);
                assert_eq!(wall_hit.1.part, 2);
            }
            let mut scratch = QueryScratch::default();
            assert_eq!(
                world.overlap_queries().sweep_capsule_arc_where(
                    &path,
                    RADIUS,
                    HALF,
                    filter,
                    &mut scratch,
                    incoming
                ),
                Some(selected)
            );
            assert_eq!(world.broadphase_stats(), before);
            if let Some(id) = own {
                assert_eq!(world.aabb_of(id), Some(capsule.aabb()));
            }
        }
    }
}

#[test]
fn returning_to_rejected_floor_preserves_absolute_time() {
    for geometry in GEOMETRIES {
        let mut world = PhysicsWorld::new();
        let [floor, _, _] = room(&mut world, geometry);
        let path = AcceleratedPath::new(
            DVec3::Y * (RADIUS + HALF),
            DVec3::Y,
            DVec3::NEG_Y * 4.0,
            1.0,
        );
        let (_, closest) = world
            .sweep_capsule_arc(&path, RADIUS, HALF, QueryFilter::ALL)
            .unwrap();
        assert_eq!(closest.time, 0.0);
        let (id, hit) = world
            .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, hit| {
                path.velocity_at(hit.time).dot(hit.normal) < 0.0
            })
            .unwrap();
        assert_eq!(id, floor);
        assert!((hit.time - 0.5).abs() < EXACT, "{geometry:?}: {hit:?}");
        assert!(!hit.started_inside);
        assert!(hit.normal.distance(DVec3::Y) < EXACT);
    }
}

#[test]
fn embedded_round_shapes_can_depart_and_return_between_overlapping_endpoints() {
    for capsule in [false, true] {
        let mut world = PhysicsWorld::new();
        let id = if capsule {
            world.add_capsule(Capsule::new(DVec3::ZERO, 0.5, 0.5))
        } else {
            world.add_sphere(crate::Sphere::new(DVec3::ZERO, 0.5))
        };
        let offset = RADIUS + 0.5 - 0.01;
        let path = AcceleratedPath::new(DVec3::X * offset, DVec3::X, DVec3::NEG_X * 4.0, 0.5);
        let hit = world
            .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, hit| {
                path.velocity_at(hit.time).dot(hit.normal) < 0.0
            })
            .unwrap();
        assert_eq!(hit.0, id);
        let expected = (1.0 + (1.0_f64 - 8.0 * 0.01).sqrt()) / 4.0;
        assert!((hit.1.time - expected).abs() < EXACT, "{hit:?}");
        assert!(!hit.1.started_inside);
        assert!(hit.1.normal.distance(DVec3::X) < EXACT);
    }
}

#[test]
fn transformed_mesh_selection_uses_world_normals_points_and_original_time() {
    let mut world = PhysicsWorld::new();
    let mesh = TriangleMesh::new(
        &[
            DVec3::new(-10.0, 0.0, -10.0),
            DVec3::new(10.0, 0.0, -10.0),
            DVec3::new(10.0, 0.0, 10.0),
            DVec3::new(-10.0, 0.0, 10.0),
        ],
        &[[0, 1, 2], [0, 2, 3]],
    )
    .unwrap();
    let transform = Transform {
        position: DVec3::new(3.0, 2.0, 1.0),
        rotation: DQuat::from_rotation_arc(DVec3::Y, DVec3::X),
    };
    let wall = world.add_mesh(mesh, transform);
    let path = AcceleratedPath::new(
        transform.position + DVec3::X * (RADIUS - 0.01),
        DVec3::X,
        DVec3::NEG_X * 4.0,
        0.5,
    );
    let hit = world
        .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, hit| {
            hit.normal.x > 0.9 && path.velocity_at(hit.time).dot(hit.normal) < 0.0
        })
        .unwrap();
    assert_eq!(hit.0, wall);
    let expected = (1.0 + (1.0_f64 - 8.0 * 0.01).sqrt()) / 4.0;
    assert!((hit.1.time - expected).abs() < EXACT, "{hit:?}");
    assert!((hit.1.point.x - transform.position.x).abs() < EXACT);
    assert!(!hit.1.started_inside);
}

#[test]
fn incoming_overlap_remains_selectable_and_all_rejected_terminates() {
    for geometry in GEOMETRIES {
        let mut world = PhysicsWorld::new();
        room(&mut world, geometry);
        for velocity in [DVec3::NEG_Y, DVec3::ZERO, DVec3::X] {
            let path = AcceleratedPath::new(DVec3::Y * (RADIUS + HALF), velocity, DVec3::ZERO, 1.0);
            assert_eq!(
                world.sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, _| true),
                world.sweep_capsule_arc(&path, RADIUS, HALF, QueryFilter::ALL)
            );
            assert!(
                world
                    .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, _| false)
                    .is_none()
            );
            if velocity == DVec3::NEG_Y {
                let hit = world
                    .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, hit| {
                        path.velocity_at(hit.time).dot(hit.normal) < 0.0
                    })
                    .unwrap()
                    .1;
                assert!(hit.started_inside && hit.time == 0.0);
            }
        }
    }
}

#[test]
fn filters_and_ties_apply_before_contact_selection() {
    let mut world = PhysicsWorld::new();
    let wall = BoxCollider::new(DVec3::X * 2.0, DVec3::new(0.25, 10.0, 10.0));
    let excluded = world.add_box(wall);
    let masked = world.add_box(wall);
    world.set_layers(masked, 2);
    let trigger = world.add_box(wall);
    world.set_trigger(trigger, true);
    let first = world.add_box(wall);
    let second = world.add_box(wall);
    let path = AcceleratedPath::new(DVec3::ZERO, DVec3::X * 4.0, DVec3::ZERO, 1.0);
    let filter = QueryFilter::excluding(Some(excluded)).with_mask(1);
    for _ in 0..3 {
        let hit = world
            .sweep_capsule_arc_where(&path, RADIUS, HALF, filter, |id, _| {
                assert!(id == first || id == second);
                true
            })
            .unwrap();
        assert_eq!(hit.0, first);
        world.rebuild();
    }
}

#[test]
fn non_approaching_slope_does_not_hide_finite_wall() {
    let mut world = PhysicsWorld::new();
    let normal = DVec3::new(0.0, 1.0, 4.0).normalize();
    let rotation = DQuat::from_rotation_arc(DVec3::Y, normal);
    let path = AcceleratedPath::new(DVec3::Y * 5.0, DVec3::X * 4.0, DVec3::NEG_Y * 8.0, 1.0);
    let centre = path.start - DVec3::Y * HALF - normal * (RADIUS + 0.05 + 0.1);
    let slope = world
        .add_box(BoxCollider::new(centre, DVec3::new(10.0, 0.1, 10.0)).with_rotation(rotation));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(1.5, 5.0, 0.0),
        DVec3::new(0.1, 10.0, 0.1),
    ));
    assert_eq!(
        world
            .sweep_capsule_arc(&path, RADIUS, HALF, QueryFilter::ALL)
            .unwrap()
            .0,
        slope
    );
    let hit = world
        .sweep_capsule_arc_where(&path, RADIUS, HALF, QueryFilter::ALL, |_, hit| {
            let flat = DVec3::new(hit.normal.x, 0.0, hit.normal.z);
            flat.dot(path.velocity_at(hit.time)) < -1e-9
        })
        .unwrap();
    assert_eq!(hit.0, wall);
    assert!((hit.1.time - (1.4 - RADIUS) / 4.0).abs() < EXACT);
}
