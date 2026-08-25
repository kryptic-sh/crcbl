//! Physics: broadphase queries, forces, continuous collision detection.
//!
//! This crate provides the engine's physics pillar: collider shapes, bounding
//! volumes, broadphase spatial queries (BVH), continuous collision detection
//! (swept sphere and swept capsule TOI), and the capsule character controller
//! built on them.
//!
//! # Layered architecture
//!
//! | Layer | Contents | Status |
//! | ----- | -------- | ------ |
//! | **L0** | Queries + kinematics: ray/segment/sweep/overlap, trigger volumes, character controller | Current |
//! | **L1** | Forces + ballistics + orbits: gravity, drag, thrust, integrators, Kepler propagation | Current |
//! | **CCD** | Swept collision: TOI, motion-inflated broadphase | Current |
//! | **L2** | Contact solver: sequential impulses, warm starting, islands | Stretch |
//!
//! L1 today is the force pipeline and one integrator: [`GravityForce`],
//! [`DragForce`], [`DampingForce`] and [`ThrustForce`] feed
//! [`SemiImplicitEuler`] through [`ForceProvider`]. [`Atmosphere`] and its
//! quadratic [`AtmosphericDrag`] have landed, the [`Frames`] hierarchy carries
//! sphere-of-influence crossings, and [`propagate`] is the analytic Kepler
//! solution a coasting body is put on rails with.
//!
//! L0's kinematics side is [`CharacterController`]: a capsule that walks,
//! slides, climbs steps and stays on its ground, moved by
//! [`PhysicsWorld::sweep_capsule`] and dug out by
//! [`PhysicsWorld::capsule_penetrations_into`]. It takes a world-space
//! displacement and knows nothing about any camera, which is what lets one
//! controller serve a first-person and a third-person game.
//!
//! All spatial types use `f64` for determinism. Downcasting to `f32` happens
//! only at the render boundary via `crcbl_core::WorldPos::relative_to`.
//!
//! See `docs/plan/05-physics.md` for the full design.

pub mod atmosphere;
pub mod broadphase;
pub mod character;
pub mod collider;
pub mod components;
pub mod forces;
pub mod frames;
pub mod integrator;
pub mod orbit;
pub mod query;
pub mod system;
pub mod world;

pub use atmosphere::{Atmosphere, AtmosphericDrag};
pub use broadphase::{Bvh, BvhHit, Ray, Segment};
pub use character::{CharacterConfig, CharacterController, GroundContact, MoveOutcome};
pub use collider::{Aabb, BoxCollider, Capsule, Sphere};
pub use components::{ColliderComponent, RigidBody, Transform};
pub use forces::{DampingForce, DragForce, ForceProvider, GravityForce, PointGravity, ThrustForce};
pub use frames::{FrameId, Frames, State, sphere_of_influence};
pub use integrator::{Integrator, SemiImplicitEuler};
pub use orbit::{Orbit, propagate};
pub use query::{
    Penetration, ShapeHit, capsule_penetration_vs_aabb, capsule_penetration_vs_capsule,
    capsule_penetration_vs_sphere, ray_vs_aabb, ray_vs_capsule, ray_vs_sphere,
    sphere_overlaps_aabb, sphere_overlaps_capsule, sphere_overlaps_sphere, swept_capsule_vs_aabb,
    swept_capsule_vs_capsule, swept_capsule_vs_sphere, swept_sphere_vs_aabb,
    swept_sphere_vs_capsule, swept_sphere_vs_sphere,
};
pub use system::{EntityOverlapQueries, PhysicsSystem};
pub use world::{BroadphaseStats, ColliderId, OverlapQueries, PhysicsWorld, QueryScratch};
