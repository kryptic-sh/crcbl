//! Physics: broadphase queries, forces, continuous collision detection.
//!
//! This crate provides the engine's physics pillar: collider shapes, bounding
//! volumes, broadphase spatial queries (BVH), and continuous collision
//! detection (swept sphere TOI).
//!
//! # Layered architecture
//!
//! | Layer | Contents | Status |
//! | ----- | -------- | ------ |
//! | **L0** | Queries + kinematics: ray/segment/sweep/overlap, trigger volumes | Current |
//! | **L1** | Forces + ballistics + orbits: gravity, drag, thrust, integrators | Planned |
//! | **CCD** | Swept collision: TOI, motion-inflated broadphase | Current |
//! | **L2** | Contact solver: sequential impulses, warm starting, islands | Stretch |
//!
//! All spatial types use `f64` for determinism. Downcasting to `f32` happens
//! only at the render boundary via [`WorldPos::relative_to`].
//!
//! See `docs/plan/05-physics.md` for the full design.
//!
//! [`WorldPos::relative_to`]: crcbl_core::world::WorldPos::relative_to

pub mod broadphase;
pub mod collider;
pub mod components;
pub mod forces;
pub mod integrator;
pub mod query;
pub mod system;
pub mod world;

pub use broadphase::{Bvh, BvhHit, Ray, Segment};
pub use collider::{Aabb, BoxCollider, Capsule, Sphere};
pub use components::{ColliderComponent, RigidBody, Transform};
pub use forces::{DragForce, ForceProvider, GravityForce};
pub use integrator::{Integrator, SemiImplicitEuler};
pub use query::{
    ShapeHit, ray_vs_aabb, ray_vs_capsule, ray_vs_sphere, sphere_overlaps_aabb,
    sphere_overlaps_capsule, sphere_overlaps_sphere, swept_sphere_vs_aabb, swept_sphere_vs_capsule,
    swept_sphere_vs_sphere,
};
pub use system::PhysicsSystem;
pub use world::{ColliderId, PhysicsWorld};

/// Builds a test [`Entity`] from raw index + generation for use in
/// integration tests.
///
/// Real entities are only produced by `crcbl_ecs::World::spawn`; this
/// exists so tests can fabricate handles without a full `World`.
pub fn test_entity(index: u32) -> crcbl_ecs::Entity {
    crcbl_ecs::Entity::from_bits(((1u64) << 32) | index as u64)
        .expect("test entity must have non-zero generation")
}
