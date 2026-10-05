//! **Every query's answers on unturned scenes, to the bit, pinned.**
//!
//! The query world's common case is spheres, capsules and boxes whose axes
//! are the world's, and meshes: what games and the character controller ask
//! of it every tick, and what their determinism hashes are built on. Turning
//! offsets and capsules with their bodies, and holding compounds by their
//! parts, must leave those answers where they were. This digests every
//! answer every query family gives over random scenes of those shapes — ids,
//! fractions, points, normals and flags as bits — and over the colliders a
//! [`PhysicsSystem`] places for unturned bodies, so any change to one bit of
//! one answer moves the pin.

use crcbl_core::rand::hash_unit;
use glam::DVec3;

use crate::broadphase::{Ray, Segment};
use crate::collider::{Aabb, BoxCollider, Capsule, LyingCapsule, Sphere};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;
use crate::query::{OverlapHit, ShapeHit};
use crate::system::PhysicsSystem;

use super::{ColliderId, PhysicsWorld, QueryFilter};

/// What [`every_answer_on_unturned_scenes_is_where_it_was`] digests to.
const PINNED: u64 = 0x7cfef22959e2066a;

/// What [`every_overlap_hit_on_unturned_scenes_is_pinned`] digests to.
const PINNED_OVERLAP_HITS: u64 = 0x7fcb3f4621c891b0;

/// FNV-1a over 64-bit words: a fixed function, so the pin means the same on
/// every platform and toolchain, as a `std` hasher's need not.
struct Digest {
    state: u64,
    /// How many answers were a hit rather than nothing, so a run whose
    /// queries met nothing cannot pass by digesting only misses.
    met: usize,
}

impl Digest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET,
            met: 0,
        }
    }

    fn word(&mut self, word: u64) {
        for byte in word.to_le_bytes() {
            self.state = (self.state ^ u64::from(byte)).wrapping_mul(Self::PRIME);
        }
    }

    fn vec(&mut self, v: DVec3) {
        for c in v.to_array() {
            self.word(c.to_bits());
        }
    }

    fn id(&mut self, id: ColliderId) {
        self.word(u64::from(id.index));
        self.word(u64::from(id.generation));
    }

    fn hit(&mut self, hit: Option<(ColliderId, ShapeHit)>) {
        let Some((id, hit)) = hit else {
            self.word(u64::MAX);
            return;
        };
        self.met += 1;
        self.id(id);
        self.word(hit.t.to_bits());
        self.vec(hit.point);
        self.vec(hit.normal);
        self.word(u64::from(hit.started_inside));
    }

    fn overlap(&mut self, id: ColliderId, hit: OverlapHit) {
        self.met += 1;
        self.id(id);
        self.vec(hit.point);
        self.vec(hit.normal);
        self.word(hit.depth.to_bits());
        self.word(hit.part as u64);
    }
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

/// A tilted quad, two triangles.
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

/// Spheres, unturned boxes, upright capsules and meshes in turn, scattered
/// through a cube; every fifth is a trigger.
fn unturned_scene(seed: u64, count: u32) -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    let mut index = 0;
    for n in 0..count {
        let centre = point(seed, &mut index, 4.0);
        let size = draw(seed, &mut index, 0.2, 1.2);
        let id = match n % 4 {
            0 => world.add_sphere(Sphere::new(centre, size)),
            1 => world.add_box(BoxCollider::new(
                centre,
                DVec3::new(size, size * 0.5, size * 1.5),
            )),
            2 => world.add_capsule(Capsule::new(centre, size * 0.5, size)),
            _ => world.add_mesh(
                ramp(),
                Transform::new(
                    centre,
                    rotation_from_scaled_axis(point(seed, &mut index, 1.5)),
                ),
            ),
        };
        assert!(world.set_trigger(id, n % 5 == 2));
    }
    world
}

/// Every query family over one scene, into `digest`.
fn digest_scene(seed: u64, world: &mut PhysicsWorld, digest: &mut Digest) {
    let mut index = 10_000;
    let mut ids = Vec::new();
    let mut hits = Vec::new();
    let mut pushes = Vec::new();
    for _ in 0..30 {
        let start = point(seed, &mut index, 6.0);
        let end = point(seed, &mut index, 6.0);
        let radius = draw(seed, &mut index, 0.05, 0.8);
        let half_height = draw(seed, &mut index, 0.0, 1.0);
        let segment = Segment::new(start, end);

        digest.hit(world.cast_ray(&Ray::new(start, end - start)));
        digest.hit(world.sweep_sphere(&segment, radius));
        digest.hit(world.sweep_capsule(&segment, radius, half_height));
        for capsule in [false, true] {
            if capsule {
                world.sweep_capsule_all(&segment, radius, half_height, QueryFilter::ALL, &mut hits);
            } else {
                world.sweep_sphere_all(&segment, radius, QueryFilter::ALL, &mut hits);
            }
            digest.word(hits.len() as u64);
            for &hit in &hits {
                digest.hit(Some(hit));
            }
        }

        world.overlap_sphere_into(start, radius, &mut ids);
        digest.word(ids.len() as u64);
        // The ids alone, as before the overlaps answered hits: what they find
        // must not have moved. The hits are pinned on their own, below.
        ids.iter().for_each(|&(id, _)| digest.id(id));
        let aabb = Aabb::new(start.min(end), start.max(end));
        let boxed = world.overlap_aabb(&aabb);
        digest.word(boxed.len() as u64);
        boxed.iter().for_each(|&id| digest.id(id));

        let capsule = Capsule::new(start, radius, half_height);
        world.capsule_penetrations_into(&capsule, None, &mut pushes);
        digest.word(pushes.len() as u64);
        for &(id, push) in &pushes {
            digest.id(id);
            digest.vec(push.normal);
            digest.word(push.depth.to_bits());
        }

        let yaw = draw(seed, &mut index, -3.0, 3.0);
        let lying = LyingCapsule::new(start, yaw, radius, half_height * 2.0);
        match world.lying_capsule_blocker(&lying, QueryFilter::ALL) {
            Some(id) => digest.id(id),
            None => digest.word(u64::MAX),
        }
        match world.sweep_lying_capsule(&lying, end - start, QueryFilter::ALL) {
            Some((id, contact)) => {
                digest.id(id);
                digest.word(contact.t.to_bits());
                digest.vec(contact.normal);
                digest.word(u64::from(contact.started_inside));
            }
            None => digest.word(u64::MAX),
        }
    }
}

/// A body per collider kind, each unturned and its collider offset from it,
/// as a [`PhysicsSystem`] places them in its query world.
fn digest_system(seed: u64, digest: &mut Digest) {
    let mut phys = PhysicsSystem::new();
    let mut index = 20_000;
    for n in 0..12_u32 {
        let entity = crcbl_ecs::Entity::from_bits((1u64 << 32) | u64::from(n + 1))
            .expect("generation 1 is never zero");
        let transform = Transform::from_position(point(seed, &mut index, 4.0));
        let offset = point(seed, &mut index, 1.0);
        let size = draw(seed, &mut index, 0.2, 1.2);
        let component = match n % 3 {
            0 => ColliderComponent::Sphere {
                offset,
                radius: size,
                is_trigger: false,
            },
            1 => ColliderComponent::Box {
                offset,
                half_extents: DVec3::splat(size),
                is_trigger: false,
            },
            _ => ColliderComponent::Capsule {
                offset,
                radius: size * 0.5,
                half_height: size,
                is_trigger: false,
            },
        };
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_transform(entity, transform);
        phys.set_collider(entity, &component, &transform);
        let id = phys.collider_of(entity).expect("a collider");
        let bounds = phys.world().aabb_of(id).expect("in the world");
        digest.vec(bounds.min);
        digest.vec(bounds.max);
    }
    phys.step(1.0 / 60.0);
    let world = phys.world_mut();
    digest_scene(seed, world, digest);
}

#[test]
fn every_answer_on_unturned_scenes_is_where_it_was() {
    let mut digest = Digest::new();
    for seed in 0..12_u64 {
        let mut world = unturned_scene(seed, 16);
        digest_scene(seed, &mut world, &mut digest);
        digest_system(seed, &mut digest);
    }
    assert!(digest.met > 1_000, "only {} hits were digested", digest.met);
    assert_eq!(digest.state, PINNED, "{:#018x}", digest.state);
}

/// **Every sphere overlap's hits on the same scenes, to the bit, in order,
/// pinned**: the points, normals, depths and parts, as bits, so a change to
/// any one of them or to the order they come in moves the pin, on whatever
/// target runs it.
#[test]
fn every_overlap_hit_on_unturned_scenes_is_pinned() {
    let mut digest = Digest::new();
    let mut hits = Vec::new();
    for seed in 0..12_u64 {
        let mut world = unturned_scene(seed, 16);
        let mut index = 30_000;
        for _ in 0..48 {
            let centre = point(seed, &mut index, 5.0);
            let radius = draw(seed, &mut index, 0.1, 2.5);
            world.overlap_sphere_into(centre, radius, &mut hits);
            digest.word(hits.len() as u64);
            for &(id, hit) in &hits {
                digest.overlap(id, hit);
            }
        }
    }
    assert!(digest.met > 300, "only {} hits were digested", digest.met);
    assert_eq!(digest.state, PINNED_OVERLAP_HITS, "{:#018x}", digest.state);
}
