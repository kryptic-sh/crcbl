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

pub use broadphase::{Bvh, BvhHit, Ray, Segment};
pub use collider::{Aabb, BoxCollider, Capsule, Sphere};
