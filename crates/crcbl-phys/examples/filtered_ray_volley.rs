//! A volley of filtered world rays that hit nothing, for instruction counts.
//!
//! The shape of a birdshot volley in a shooter's level: a crowd of character
//! capsules, ragdoll parts and loose items, each on a query layer of its own,
//! with the level's static colliders on another, and rays masked to the level
//! alone fired through the crowd, short of any wall. Every one of them misses,
//! so what a run costs is the broadphase walk and nothing else.
//!
//! ```text
//! cargo build --release -p crcbl-phys --example filtered_ray_volley
//! valgrind --tool=callgrind target/release/examples/filtered_ray_volley 0
//! valgrind --tool=callgrind target/release/examples/filtered_ray_volley 100000
//! ```
//!
//! The argument is how many rays to cast. The difference between the two
//! runs' totals, divided by the rays, is what one ray costs, with the scene's
//! construction cancelled out.

use std::hint::black_box;

use crcbl_core::rand::hash_unit;
use crcbl_core::trig::{cos, sin};
use crcbl_phys::{BoxCollider, Capsule, PhysicsWorld, QueryFilter, Ray};
use glam::DVec3;

const LEVEL: u32 = 1 << 0;
const CHARACTERS: u32 = 1 << 1;
const RAGDOLLS: u32 = 1 << 2;
const ITEMS: u32 = 1 << 3;

/// Half the width of the square the crowd stands in, in metres.
const CROWD_HALF_WIDTH: f64 = 40.0;
/// How far out the level's walls stand: past the crowd and past any ray.
const WALL_RING: f64 = 120.0;
/// How far each ray looks.
const RAY_LENGTH: f64 = 30.0;

const CHARACTER_COUNT: u64 = 300;
const RAGDOLL_COUNT: u64 = 40;
const PARTS_PER_RAGDOLL: u64 = 11;
const ITEM_COUNT: u64 = 200;
const WALL_COUNT: u64 = 64;

fn crowd_point(seed: u64, index: u64, height: f64) -> DVec3 {
    let at = |axis: u64| (hash_unit(seed, 2 * index + axis) * 2.0 - 1.0) * CROWD_HALF_WIDTH;
    DVec3::new(at(0), height, at(1))
}

fn volley_world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    for n in 0..CHARACTER_COUNT {
        let id = world.add_capsule(Capsule::new(crowd_point(1, n, 0.9), 0.3, 0.6));
        world.set_layers(id, CHARACTERS);
    }
    for n in 0..RAGDOLL_COUNT {
        let body = crowd_point(2, n, 0.2);
        for part in 0..PARTS_PER_RAGDOLL {
            let offset = DVec3::new(part as f64 * 0.15, 0.0, (part % 3) as f64 * 0.2);
            let id = world.add_capsule(Capsule::new(body + offset, 0.08, 0.15));
            world.set_layers(id, RAGDOLLS);
        }
    }
    for n in 0..ITEM_COUNT {
        let id = world.add_box(BoxCollider::new(crowd_point(3, n, 0.1), DVec3::splat(0.15)));
        world.set_layers(id, ITEMS);
    }
    for n in 0..WALL_COUNT {
        let angle = n as f64 / WALL_COUNT as f64 * std::f64::consts::TAU;
        let centre = DVec3::new(cos(angle), 0.0, sin(angle)) * WALL_RING + DVec3::Y * 3.0;
        let id = world.add_box(BoxCollider::new(centre, DVec3::new(6.0, 3.0, 6.0)));
        world.set_layers(id, LEVEL);
    }
    world.rebuild();
    world
}

fn main() {
    let rays: u64 = std::env::args()
        .nth(1)
        .map_or(Ok(100_000), |count| count.parse())
        .expect("the argument is a ray count");
    let mut world = volley_world();
    let filter = QueryFilter::masked(LEVEL);
    let mut hits = 0u64;
    for n in 0..rays {
        let origin = crowd_point(4, n, 1.5);
        let yaw = hash_unit(5, n) * std::f64::consts::TAU;
        let pitch = (hash_unit(6, n) - 0.5) * 0.2;
        let dir = DVec3::new(cos(yaw), pitch, sin(yaw)).normalize();
        let ray = Ray::new(origin, dir).with_bounds(0.0, RAY_LENGTH);
        hits += u64::from(world.cast_ray_filtered(black_box(&ray), filter).is_some());
    }
    println!("{rays} rays, {hits} hits");
}
