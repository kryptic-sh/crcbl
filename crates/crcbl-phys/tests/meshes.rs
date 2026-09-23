//! Static triangle meshes, run whole: the half of rung 5 of
//! `docs/plan/36-contact-solver.md` that comes "before the stairs" — bodies
//! sliding and rolling on a mesh, down stairs and a ramp, across a seam
//! without catching, not tunnelling through it, sleeping on it, and the query
//! world hitting its triangles.
//!
//! `docs/plan/sample/24-tumble.md` names no scene for the mesh alone — its
//! stairs come with the ragdolls, which need joints — so the proving scene is
//! [`stairs_and_ramp`] here. As in the other suites, every bound was measured
//! before it was written down, and each check that guards a fix is run with
//! the fix off too, to show it can fail.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, CompoundPart, CompoundShape, ContactBody, ContactSettings, GravityForce,
    MassProperties, PhysicsSystem, QueryFilter, Ray, RigidBody, Segment, SurfaceMaterial,
    Transform, TriangleMesh,
};
use glam::{DMat3, DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts at `settings` and Earth gravity.
fn system(settings: ContactSettings) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(settings);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys
}

/// Vertices and triangles being built, quad by quad.
#[derive(Default)]
struct Builder {
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
}

impl Builder {
    /// A quad `a b c d`, counter-clockwise seen from the side it faces, as
    /// the two triangles `a b c` and `a c d`.
    fn quad(&mut self, a: DVec3, b: DVec3, c: DVec3, d: DVec3) {
        let base = self.vertices.len() as u32;
        self.vertices.extend([a, b, c, d]);
        self.triangles
            .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    }

    /// A floor at height `y` over `x0..x1` by `z0..z1`, facing up.
    fn floor(&mut self, y: f64, x0: f64, x1: f64, z0: f64, z1: f64) {
        self.quad(
            DVec3::new(x0, y, z0),
            DVec3::new(x0, y, z1),
            DVec3::new(x1, y, z1),
            DVec3::new(x1, y, z0),
        );
    }

    fn build(&self) -> TriangleMesh {
        TriangleMesh::new(&self.vertices, &self.triangles).expect("a valid mesh")
    }
}

/// A square floor of half-side `half` at `y = 0`: two triangles, their seam
/// the diagonal from `(-half, 0, -half)` to `(half, 0, half)`.
fn quad(half: f64) -> TriangleMesh {
    let mut b = Builder::default();
    b.floor(0.0, -half, half, -half, half);
    b.build()
}

/// How many steps the stairs have.
const STEPS: usize = 5;
/// How deep each tread is, in metres.
const TREAD: f64 = 0.5;
/// How high each riser is, in metres.
const RISE: f64 = 0.2;
/// The top of the stairs and of the ramp.
const TOP: f64 = STEPS as f64 * RISE;
/// Where the stairs and the ramp end and the floor begins, along `x`.
const FOOT: f64 = STEPS as f64 * TREAD;
/// The stairs' lane, across `z`.
const STAIRS_Z: (f64, f64) = (-1.0, 1.0);
/// The ramp's lane, across `z`.
const RAMP_Z: (f64, f64) = (1.5, 3.5);

/// Where each lane's floor ends in a wall facing back up it, along `x`.
const END: f64 = FOOT + 6.0;

/// **The proving scene's mesh**: two lanes down `+x` from a landing at
/// [`TOP`] to a floor at `y = 0` from [`FOOT`] to a wall at [`END`] — stairs
/// of [`STEPS`] steps in one, a ramp of the same drop in the other — as one
/// mesh, each lane's quads meeting edge to edge.
fn stairs_and_ramp() -> TriangleMesh {
    let mut b = Builder::default();
    for (z0, z1) in [STAIRS_Z, RAMP_Z] {
        b.quad(
            DVec3::new(END, 1.0, z1),
            DVec3::new(END, 1.0, z0),
            DVec3::new(END, 0.0, z0),
            DVec3::new(END, 0.0, z1),
        );
    }
    let (z0, z1) = STAIRS_Z;
    b.floor(TOP, -1.0, 0.0, z0, z1);
    for i in 0..STEPS {
        let x = i as f64 * TREAD;
        let y = (STEPS - i) as f64 * RISE;
        b.floor(y, x, x + TREAD, z0, z1);
        // The riser at the tread's far end, facing down the stairs.
        let front = x + TREAD;
        b.quad(
            DVec3::new(front, y, z0),
            DVec3::new(front, y, z1),
            DVec3::new(front, y - RISE, z1),
            DVec3::new(front, y - RISE, z0),
        );
    }
    b.floor(0.0, FOOT, END, z0, z1);

    let (z0, z1) = RAMP_Z;
    b.floor(TOP, -1.0, 0.0, z0, z1);
    b.quad(
        DVec3::new(0.0, TOP, z0),
        DVec3::new(0.0, TOP, z1),
        DVec3::new(FOOT, 0.0, z1),
        DVec3::new(FOOT, 0.0, z0),
    );
    b.floor(0.0, FOOT, END, z0, z1);
    b.build()
}

/// The height of the stairs lane's surface at `x`.
fn stairs_height(x: f64) -> f64 {
    if x < 0.0 {
        TOP
    } else if x >= FOOT {
        0.0
    } else {
        (STEPS - (x / TREAD) as usize) as f64 * RISE
    }
}

/// A static body whose collider is `mesh`, at the origin.
fn mesh_body(phys: &mut PhysicsSystem, index: u32, mesh: TriangleMesh, material: SurfaceMaterial) {
    let e = entity(index);
    phys.set_transform(e, Transform::IDENTITY);
    phys.set_collider(
        e,
        &ColliderComponent::Mesh {
            mesh,
            is_trigger: false,
        },
        &Transform::IDENTITY,
    );
    phys.set_material(e, material);
}

/// A dynamic body of `collider`, 1 kg with `inertia`, at `at` turned by
/// `rotation`.
fn body(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    rotation: DQuat,
    collider: &ColliderComponent,
    inertia: DMat3,
    material: SurfaceMaterial,
) -> Entity {
    let e = entity(index);
    phys.set_body(e, RigidBody::new_dynamic(1.0).with_inertia(inertia));
    let transform = Transform::new(at, rotation);
    phys.set_transform(e, transform);
    phys.set_collider(e, collider, &transform);
    phys.set_material(e, material);
    e
}

fn ball(phys: &mut PhysicsSystem, index: u32, at: DVec3, radius: f64) -> Entity {
    body(
        phys,
        index,
        at,
        DQuat::IDENTITY,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius,
            is_trigger: false,
        },
        MassProperties::sphere(1.0, radius, DVec3::ZERO).inertia,
        SurfaceMaterial::new(0.5, 0.0),
    )
}

fn cube(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    half: f64,
    material: SurfaceMaterial,
) -> Entity {
    let half = DVec3::splat(half);
    body(
        phys,
        index,
        at,
        DQuat::IDENTITY,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        MassProperties::cuboid(1.0, half, DVec3::ZERO).inertia,
        material,
    )
}

/// A capsule lying along `z`: its local `y` axis turned a quarter about `x`.
fn lying_capsule(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let (radius, half_height) = (0.1, 0.3);
    body(
        phys,
        index,
        at,
        crcbl_phys::rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2),
        &ColliderComponent::Capsule {
            offset: DVec3::ZERO,
            radius,
            half_height,
            is_trigger: false,
        },
        MassProperties::capsule(1.0, radius, half_height, DVec3::ZERO).inertia,
        SurfaceMaterial::new(0.5, 0.0),
    )
}

/// An L of two boxes, one of them turned.
fn compound(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let shape = CompoundShape::new(vec![
        CompoundPart::new(
            DVec3::new(-0.05, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::new(0.08, 0.03, 0.03),
        ),
        CompoundPart::new(
            DVec3::new(0.05, 0.06, 0.0),
            crcbl_phys::rotation_from_scaled_axis(DVec3::Y * 0.4),
            DVec3::new(0.03, 0.06, 0.03),
        ),
    ])
    .expect("two parts");
    body(
        phys,
        index,
        at,
        DQuat::IDENTITY,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape,
            is_trigger: false,
        },
        DMat3::from_diagonal(DVec3::splat(0.004)),
        SurfaceMaterial::new(0.5, 0.0),
    )
}

/// What a box sliding across a seam did, tick by tick.
#[derive(Debug)]
struct Slide {
    /// The least speed along the slide it had.
    slowest: f64,
    /// The fastest it moved vertically.
    vertical: f64,
    /// The fastest it turned.
    turning: f64,
}

/// A half-metre cube sliding at 3 m/s along `+x`, on ice, across the
/// diagonal seam of a two-triangle floor — with every edge of the floor
/// active if `all_edges`.
fn slide_across_the_seam(all_edges: bool) -> Slide {
    let ice = SurfaceMaterial::new(0.0, 0.0);
    let mut phys = system(ContactSettings::DEFAULT);
    let floor = quad(5.0);
    let floor = if all_edges {
        floor.collide_with_all_edges()
    } else {
        floor
    };
    mesh_body(&mut phys, 0, floor, ice);
    let e = cube(&mut phys, 1, DVec3::new(-2.0, 0.25, 0.0), 0.25, ice);
    // Settled first, so the slide starts from the contact at rest.
    for _ in 0..30 {
        phys.step(DT);
    }
    phys.body_mut(e).expect("a body").velocity = DVec3::X * 3.0;
    let mut slide = Slide {
        slowest: f64::INFINITY,
        vertical: 0.0,
        turning: 0.0,
    };
    // Past the seam, which crosses the slide at x = 0: 1.5 s.
    for _ in 0..90 {
        phys.step(DT);
        let body = phys.body(e).expect("a body");
        slide.slowest = slide.slowest.min(body.velocity.x);
        slide.vertical = slide.vertical.max(body.velocity.y.abs());
        slide.turning = slide.turning.max(body.angular_velocity.length());
    }
    let x = phys.transform(e).expect("placed").position.x;
    assert!(x > 1.0, "the box got to x = {x}");
    slide
}

/// **A box slides across a seam without catching on it.** A half-metre cube
/// at 3 m/s on ice, across the diagonal seam of a floor of two triangles.
///
/// With every edge active the second triangle's edge is in the box's way —
/// a ghost collision. Measured on 2026-09-23: the box slowed to 2.40 m/s,
/// jumped at 0.33 m/s and tumbled at 5.09 rad/s. With the seam inactive, as
/// the mesh builds it, it kept 3.0 m/s, moving vertically at 7.9e-5 m/s and
/// turning at 5.2e-4 rad/s at most, as it would on a plane.
#[test]
fn a_box_slides_across_a_seam_without_catching() {
    let caught = slide_across_the_seam(true);
    let clean = slide_across_the_seam(false);
    println!("all edges active: {caught:?}\nseam inactive: {clean:?}");
    assert!(
        caught.slowest < 2.9 || caught.vertical > 0.1 || caught.turning > 1.0,
        "with the seam active the box did not catch, so this scene proves nothing: {caught:?}"
    );
    assert!(clean.slowest > 2.99, "{clean:?}");
    assert!(clean.vertical < 0.01, "{clean:?}");
    assert!(clean.turning < 0.01, "{clean:?}");
}

/// **Balls roll down the stairs, and a ball, a box, a capsule and a compound
/// down the ramp**, onto the floor beyond, without sinking into a tread or
/// falling through anything. Measured on 2026-09-23 over the four seconds:
/// the deepest any contact point sank, or any stairs ball's centre sat below
/// a radius over the tread under it, was 5.3 mm.
#[test]
fn bodies_go_down_the_stairs_and_the_ramp() {
    let mut phys = system(ContactSettings::DEFAULT);
    mesh_body(
        &mut phys,
        0,
        stairs_and_ramp(),
        SurfaceMaterial::new(0.5, 0.0),
    );
    let radius = 0.1;
    let stair_balls: Vec<Entity> = (0..3)
        .map(|k| {
            let e = ball(
                &mut phys,
                10 + k,
                DVec3::new(-0.5, TOP + radius, -0.5 + 0.5 * f64::from(k)),
                radius,
            );
            phys.body_mut(e).expect("a body").velocity = DVec3::X * 1.5;
            e
        })
        .collect();
    // Across the ramp's lane, clear of each other: a ball, a capsule lying
    // across the slope, a box and a compound.
    let ramp_ball = ball(&mut phys, 20, DVec3::new(-0.5, TOP + radius, 1.75), radius);
    phys.body_mut(ramp_ball).expect("a body").velocity = DVec3::X;
    let sliding = cube(
        &mut phys,
        21,
        DVec3::new(-0.3, TOP + 0.1, 3.0),
        0.1,
        SurfaceMaterial::new(0.2, 0.0),
    );
    phys.body_mut(sliding).expect("a body").velocity = DVec3::X * 2.0;
    let capsule = lying_capsule(&mut phys, 22, DVec3::new(-0.5, TOP + 0.1, 2.3));
    phys.body_mut(capsule).expect("a body").velocity = DVec3::X;
    let l = compound(&mut phys, 23, DVec3::new(-0.2, TOP + 0.05, 3.3));
    phys.body_mut(l).expect("a body").velocity = DVec3::X * 2.5;

    let mut deepest: f64 = 0.0;
    let mut lowest_step = vec![STEPS; stair_balls.len()];
    // Each body, the height its centre rests at on the floor, and whether it
    // has rested there — past the foot, within a centimetre of that height.
    let mut tracked: Vec<(&str, Entity, f64, bool)> = stair_balls
        .iter()
        .map(|&e| ("stairs ball", e, radius, false))
        .chain([
            ("ramp ball", ramp_ball, radius, false),
            ("box", sliding, 0.1, false),
            ("capsule", capsule, 0.1, false),
        ])
        .collect();
    for _ in 0..240 {
        phys.step(DT);
        for (k, &e) in stair_balls.iter().enumerate() {
            let at = phys.transform(e).expect("placed").position;
            // Sunk below the surface under it, where there is one to sink
            // into: over a tread and clear of its nose.
            let clear = (at.x - (at.x / TREAD).round() * TREAD).abs() > radius;
            if clear {
                deepest = deepest.max(stairs_height(at.x) + radius - at.y);
            }
            if (0.0..FOOT).contains(&at.x) {
                lowest_step[k] = lowest_step[k].min(STEPS - 1 - (at.x / TREAD) as usize);
            }
        }
        for (name, e, rest, landed) in &mut tracked {
            let at = phys.transform(*e).expect("placed").position;
            assert!(at.x < END && at.y > 0.0, "the {name} left the mesh: {at:?}");
            *landed |= at.x > FOOT && (at.y - *rest).abs() < 0.01;
        }
        deepest = deepest.max(phys.contact_counters().worst_penetration);
    }
    for (k, lowest) in lowest_step.iter().enumerate() {
        assert_eq!(*lowest, 0, "stairs ball {k} skipped the last tread");
    }
    for (name, e, _, landed) in &tracked {
        let at = phys.transform(*e).expect("placed").position;
        assert!(
            landed,
            "the {name} never rested on the floor; it ended at {at:?}"
        );
    }
    // The compound's grip, μ = 0.5, holds on the ramp's tan 21.8° = 0.4, so
    // it slides down and comes to rest near the foot, on the ramp's surface.
    let at = phys.transform(l).expect("placed").position;
    let ramp = TOP * (1.0 - at.x / FOOT).max(0.0);
    assert!(
        at.x > 0.5 * FOOT && at.y > ramp && at.y < ramp + 0.2,
        "the compound ended at {at:?}"
    );
    println!("deepest overlap over the run: {deepest}");
    assert!(deepest < 0.01, "sank {deepest} m into the stairs");

    // Every contact with the mesh names the triangle it is with.
    let with_mesh = phys
        .contacts()
        .into_iter()
        .filter(|c| c.b == ContactBody::Entity(entity(0)) && !c.manifold.points().is_empty())
        .count();
    assert!(with_mesh > 0);
    assert!(
        phys.contacts()
            .iter()
            .all(|c| c.part_b < stairs_and_ramp().triangle_count()),
    );
}

/// Pushes `e` down hard enough that it leaves its first tick at
/// `speed`: a force held for one tick, which the tick's speculative contacts,
/// sized from the speed it began with, know nothing of.
fn launch_down(phys: &mut PhysicsSystem, e: Entity, speed: f64) {
    let mass = phys.body(e).expect("a body").mass;
    assert!(phys.apply_force(e, DVec3::NEG_Y * (mass * speed / DT)));
}

/// **A fast body does not tunnel through a mesh with no thickness.** A ball,
/// a cube, a capsule and a compound, each at rest 30 cm over a floor of two
/// triangles, launched down at 60 m/s within one tick. Without the sweeps
/// each goes straight through; with them each is stopped on the floor's
/// front, and stays there.
#[test]
fn a_fast_body_does_not_tunnel_through_a_thin_mesh() {
    type Make = fn(&mut PhysicsSystem, u32) -> Entity;
    let makes: [(&str, Make); 4] = [
        ("ball", |p, i| ball(p, i, DVec3::new(0.3, 0.3, 0.2), 0.05)),
        ("cube", |p, i| {
            cube(
                p,
                i,
                DVec3::new(0.3, 0.3, 0.2),
                0.05,
                SurfaceMaterial::new(0.5, 0.0),
            )
        }),
        ("capsule", |p, i| {
            lying_capsule(p, i, DVec3::new(0.3, 0.3, 0.2))
        }),
        ("compound", |p, i| compound(p, i, DVec3::new(0.3, 0.3, 0.2))),
    ];
    let run = |continuous: bool, make: Make| {
        let mut phys = system(ContactSettings {
            continuous,
            ..ContactSettings::DEFAULT
        });
        mesh_body(&mut phys, 0, quad(2.0), SurfaceMaterial::new(0.5, 0.0));
        let e = make(&mut phys, 1);
        launch_down(&mut phys, e, 60.0);
        let mut through = 0;
        let mut first = None;
        for _ in 0..60 {
            phys.step(DT);
            first.get_or_insert(phys.contact_counters());
            if phys.transform(e).expect("placed").position.y < 0.0 {
                through += 1;
            }
        }
        (through, first.expect("stepped"))
    };
    for (name, make) in makes {
        let (through, _) = run(false, make);
        assert!(
            through > 0,
            "the {name} did not tunnel without sweeps, so this scene proves nothing"
        );
        let (through, first) = run(true, make);
        assert_eq!(through, 0, "the {name} tunnelled: {first:?}");
        assert!(first.sweep_hits >= 1, "the {name}'s first tick: {first:?}");
        assert!(first.sweep_candidates >= 1, "{first:?}");
    }
}

/// **A body at rest on a mesh sleeps, and wakes when struck**: a crate
/// dropped on the floor sleeps, costing no narrow phase with the triangles
/// under it; a ball dropped on it wakes it.
#[test]
fn a_body_on_a_mesh_sleeps_and_wakes_as_on_anything_else() {
    let mut phys = system(ContactSettings::DEFAULT);
    mesh_body(&mut phys, 0, quad(3.0), SurfaceMaterial::new(0.6, 0.0));
    let crate_ = cube(
        &mut phys,
        1,
        DVec3::new(0.1, 0.4, -0.2),
        0.25,
        SurfaceMaterial::new(0.6, 0.0),
    );
    let mut slept_at = None;
    for tick in 0..180 {
        phys.step(DT);
        if phys.is_sleeping(crate_) {
            slept_at = Some(tick);
            break;
        }
    }
    let slept_at = slept_at.expect("the crate never slept");
    println!("the crate slept at tick {slept_at}");
    let at_rest = phys.transform(crate_).expect("placed").position;
    assert!((at_rest.y - 0.25).abs() < 0.01, "{at_rest:?}");
    phys.step(DT);
    let counters = phys.contact_counters();
    assert_eq!(
        counters.touching, 0,
        "a sleeping crate's contacts are collided: {counters:?}"
    );
    assert_eq!(counters.bodies, 0, "{counters:?}");

    let dropped = ball(&mut phys, 2, DVec3::new(0.1, 1.2, -0.2), 0.1);
    let mut woke = false;
    for _ in 0..60 {
        phys.step(DT);
        woke |= !phys.is_sleeping(crate_);
    }
    assert!(woke, "the ball landed on the crate and it stayed asleep");
    let _ = dropped;
}

/// **The query world hits the triangles exactly, and respects layers.** A ray
/// down onto a tread strikes it at its height; a capsule swept down lands on
/// its end; a sphere over the gap under the nose of a step overlaps nothing
/// though it is well inside the mesh's bounds; and a mask that leaves the
/// mesh's layer out sees none of it.
#[test]
fn the_query_world_hits_the_mesh_exactly_and_respects_layers() {
    let mut phys = PhysicsSystem::new();
    let stairs = entity(0);
    phys.set_transform(stairs, Transform::IDENTITY);
    phys.set_collider(
        stairs,
        &ColliderComponent::Mesh {
            mesh: stairs_and_ramp(),
            is_trigger: false,
        },
        &Transform::IDENTITY,
    );
    const LEVEL: u32 = 1 << 3;
    assert!(phys.set_collider_layers(stairs, LEVEL));

    // Over the third tread, x ∈ [1.0, 1.5], at y = 0.6.
    let (e, hit) = phys
        .cast_ray(&Ray::new(DVec3::new(1.2, 5.0, 0.3), DVec3::NEG_Y))
        .expect("the tread");
    assert_eq!(e, stairs);
    assert!((hit.point.y - 0.6).abs() < 1e-12, "{hit:?}");
    assert_eq!(hit.normal, DVec3::Y);

    // A riser, from down the stairs: the fourth, at x = 2.0, y ∈ [0.2, 0.4].
    let (_, hit) = phys
        .cast_ray(&Ray::new(DVec3::new(5.0, 0.3, 0.0), DVec3::NEG_X))
        .expect("the riser");
    assert!(
        (hit.point.x - 2.0).abs() < 1e-12 && hit.normal == DVec3::X,
        "{hit:?}"
    );

    // A ball dropped onto the ramp lane's floor lands a radius above it.
    let (_, hit) = phys
        .sweep_sphere(
            &Segment::new(DVec3::new(4.0, 3.0, 2.5), DVec3::new(4.0, -1.0, 2.5)),
            0.25,
        )
        .expect("the floor");
    assert!((hit.t - (3.0 - 0.25) / 4.0).abs() < 1e-12, "{hit:?}");

    // Under the landing's edge at x = 0, below the first tread and in front
    // of nothing: inside the mesh's bounds, touching no triangle.
    assert!(
        phys.overlap_sphere(DVec3::new(-0.5, 0.3, 0.0), 0.2)
            .is_empty()
    );
    assert_eq!(
        phys.overlap_sphere(DVec3::new(0.25, 1.05, 0.0), 0.1),
        [stairs]
    );

    // A mask without the level's layer sees none of it.
    let blind = QueryFilter::masked(!LEVEL);
    assert!(
        phys.cast_ray_filtered(&Ray::new(DVec3::new(1.2, 5.0, 0.3), DVec3::NEG_Y), blind)
            .is_none()
    );
    assert!(
        phys.overlap_sphere_filtered(DVec3::new(0.25, 1.05, 0.0), 0.1, blind)
            .is_empty()
    );
    assert!(
        phys.sweep_sphere_filtered(
            &Segment::new(DVec3::new(4.0, 3.0, 2.5), DVec3::new(4.0, -1.0, 2.5)),
            0.25,
            blind
        )
        .is_none()
    );
}

/// **A dynamic body refuses a mesh**, whichever is given first.
#[test]
#[should_panic(expected = "a triangle mesh is for static and kinematic bodies")]
fn a_mesh_on_a_dynamic_body_is_refused() {
    let mut phys = system(ContactSettings::DEFAULT);
    let e = entity(0);
    phys.set_body(e, RigidBody::new_dynamic(1.0));
    phys.set_collider(
        e,
        &ColliderComponent::Mesh {
            mesh: quad(1.0),
            is_trigger: false,
        },
        &Transform::IDENTITY,
    );
}

/// The other order: a mesh first, then a dynamic body.
#[test]
#[should_panic(expected = "a triangle mesh is for static and kinematic bodies")]
fn a_dynamic_body_on_a_mesh_is_refused() {
    let mut phys = system(ContactSettings::DEFAULT);
    mesh_body(&mut phys, 0, quad(1.0), SurfaceMaterial::DEFAULT);
    phys.set_body(entity(0), RigidBody::new_dynamic(1.0));
}

/// **A mesh scene hashes the same on two runs**: the stairs and the ramp with
/// their bodies, stepped four seconds.
#[test]
fn a_mesh_scene_hashes_the_same_twice() {
    let run = || {
        let mut phys = system(ContactSettings::DEFAULT);
        mesh_body(
            &mut phys,
            0,
            stairs_and_ramp(),
            SurfaceMaterial::new(0.5, 0.0),
        );
        for k in 0..4u32 {
            let e = ball(
                &mut phys,
                10 + k,
                DVec3::new(-0.5, TOP + 0.1, -0.6 + 0.4 * f64::from(k)),
                0.1,
            );
            phys.body_mut(e).expect("a body").velocity = DVec3::X * 1.5;
        }
        let e = cube(
            &mut phys,
            20,
            DVec3::new(-0.3, TOP + 0.1, 2.5),
            0.1,
            SurfaceMaterial::new(0.2, 0.0),
        );
        phys.body_mut(e).expect("a body").velocity = DVec3::X;
        let mut points = 0;
        for _ in 0..240 {
            phys.step(DT);
            points += phys.contact_counters().points;
        }
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        phys.hash_state(&mut hasher);
        (hasher.0, points)
    };
    let (first, points) = run();
    assert!(points > 0);
    assert_eq!(run(), (first, points), "two runs disagreed");
}

/// **The cost of a sizable mesh**, measured and printed, not asserted: a
/// 256 × 256 grid of half-metre cells — 131 072 triangles over 128 m square,
/// dished towards its middle — with a thousand balls dropped on it, and ten
/// thousand rays cast down at it. Run it in release:
///
/// ```text
/// cargo test --release -p crcbl-phys --test meshes -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn the_cost_of_a_sizable_mesh() {
    use std::time::Instant;
    const CELLS: usize = 256;
    const CELL: f64 = 0.5;
    let height = |i: usize, j: usize| {
        let x = (i as f64 - CELLS as f64 / 2.0) * CELL;
        let z = (j as f64 - CELLS as f64 / 2.0) * CELL;
        0.002 * (x * x + z * z)
    };
    let mut vertices = Vec::with_capacity((CELLS + 1) * (CELLS + 1));
    for i in 0..=CELLS {
        for j in 0..=CELLS {
            let x = (i as f64 - CELLS as f64 / 2.0) * CELL;
            let z = (j as f64 - CELLS as f64 / 2.0) * CELL;
            vertices.push(DVec3::new(x, height(i, j), z));
        }
    }
    let at = |i: usize, j: usize| (i * (CELLS + 1) + j) as u32;
    let mut triangles = Vec::with_capacity(CELLS * CELLS * 2);
    for i in 0..CELLS {
        for j in 0..CELLS {
            // Counter-clockwise from above: x then z is clockwise, so z first.
            triangles.push([at(i, j), at(i, j + 1), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i + 1, j)]);
        }
    }
    let clock = Instant::now();
    let mesh = TriangleMesh::new(&vertices, &triangles).expect("a valid grid");
    let built = clock.elapsed().as_secs_f64();
    assert_eq!(mesh.normal(0).y.signum(), 1.0);

    let mut phys = system(ContactSettings::DEFAULT);
    let clock = Instant::now();
    mesh_body(&mut phys, 0, mesh.clone(), SurfaceMaterial::new(0.5, 0.0));
    let registered = clock.elapsed().as_secs_f64();
    for k in 0..1000u32 {
        let (i, j) = (f64::from(k % 32), f64::from(k / 32));
        ball(
            &mut phys,
            10 + k,
            DVec3::new((i - 16.0) * 1.5, 8.0 + 0.1 * j, (j - 16.0) * 1.5),
            0.2,
        );
    }
    let start = Instant::now();
    let mut clock = || start.elapsed().as_secs_f64();
    let mut sums = [0.0f64; 5];
    let (mut points, mut pairs) = (0, 0);
    const TICKS: usize = 300;
    for _ in 0..TICKS {
        phys.step_timed(DT, &mut clock);
        let counters = phys.contact_counters();
        let stages = counters.stages.expect("timed");
        for (sum, value) in sums.iter_mut().zip([
            stages.broadphase,
            stages.narrow_phase,
            stages.solver,
            stages.islands,
            stages.continuous,
        ]) {
            *sum += value;
        }
        points += counters.points;
        pairs += counters.pairs;
    }
    let mean = |sum: f64| sum / TICKS as f64 * 1e6;
    let counters = phys.contact_counters();

    let clock = Instant::now();
    let mut hits = 0;
    for k in 0..10_000u32 {
        let (u, v) = (f64::from(k % 100), f64::from(k / 100));
        let ray = Ray::new(
            DVec3::new(u * 1.2 - 60.0, 50.0, v * 1.2 - 60.0),
            DVec3::NEG_Y,
        );
        hits += usize::from(mesh.cast_ray(&ray).is_some());
    }
    let rays = clock.elapsed().as_secs_f64();
    assert_eq!(hits, 10_000);

    println!(
        "mesh of {} triangles: built in {:.1} ms, registered as proxies in {:.1} ms",
        mesh.triangle_count(),
        built * 1e3,
        registered * 1e3
    );
    println!(
        "1000 balls over {TICKS} ticks, mean per tick: broadphase {:.1} µs, narrow phase {:.1} µs, \
         solver {:.1} µs, islands {:.1} µs, continuous {:.1} µs; mean points {:.0}, pairs {:.0}; \
         at the end {} awake, {} asleep",
        mean(sums[0]),
        mean(sums[1]),
        mean(sums[2]),
        mean(sums[3]),
        mean(sums[4]),
        points as f64 / TICKS as f64,
        pairs as f64 / TICKS as f64,
        counters.bodies,
        counters.sleeping,
    );
    println!(
        "10000 rays down at it: {:.2} µs a ray",
        rays / 10_000.0 * 1e6
    );
}

/// FNV-1a, for a digest that means the same in every build.
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
