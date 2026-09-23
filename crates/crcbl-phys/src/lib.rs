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
//! | **L2** | Contact solver: sequential impulses, warm starting, islands | Rung 1 |
//!
//! L1 today is the force pipeline and one integrator: [`GravityForce`],
//! [`DragForce`], [`DampingForce`] and [`ThrustForce`] feed
//! [`SemiImplicitEuler`] through [`ForceProvider`]. The integrator turns bodies
//! as well as moving them — torque, an inertia tensor from [`MassProperties`],
//! the gyroscopic term by the implicit midpoint rule, and the quaternion turned
//! to match — which is rung 0 of `docs/plan/36-contact-solver.md`. This crate's
//! `clippy.toml` refuses the platform's transcendental functions: the sine and
//! cosine it constructs are `crcbl_core::trig`'s, and the three platform calls
//! still standing — `AtmosphericDrag`'s exponential, the sphere of influence's
//! power and the Kepler solution's Stumpff functions — each say why where they
//! are made.
//! [`SurfaceMaterial`] carries each body's friction and restitution.
//!
//! L2 is rungs 1 and 2 of `docs/plan/36-contact-solver.md`, in [`contact`]: a
//! system made with [`PhysicsSystem::with_contacts`] collides spheres,
//! capsules and boxes through split broadphase trees, analytic manifolds, a
//! cached separating axis test with clipping for box pairs, and a substepped
//! soft solver with warm starting by feature id, centroid and twist friction,
//! speculative contacts and a restitution pass, and raises a
//! [`KineticContact`] for each hard impact. Convex hulls, islands and sleep
//! are later rungs.
//!
//! [`Atmosphere`] and its
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
//! [`AabbCompound`] is L0's query surface for a rigid body made of several
//! boxes: a ray cast and a closest-point query against local-space parts at a
//! [`Transform`], naming the part each answer came from.
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
pub mod compound;
pub mod contact;
pub mod forces;
pub mod frames;
pub mod integrator;
pub mod mass;
pub mod material;
pub mod orbit;
pub mod query;
pub mod system;
pub mod wind;
pub mod world;

pub use atmosphere::{Atmosphere, AtmosphericDrag};
pub use broadphase::{Bvh, BvhHit, Ray, Segment};
pub use character::{
    CharacterConfig, CharacterController, GroundContact, GroundProbe, MoveOutcome,
};
pub use collider::{Aabb, BoxCollider, Capsule, Sphere};
pub use components::{ColliderComponent, RigidBody, Transform};
pub use compound::{AabbCompound, CompoundError, CompoundHit, CompoundPoint};
pub use contact::{
    ContactBody, ContactCounters, ContactReport, ContactSettings, KineticContact, KineticSource,
    PlaneId, StageTimes,
};
pub use forces::{DampingForce, DragForce, ForceProvider, GravityForce, PointGravity, ThrustForce};
pub use frames::{FrameId, Frames, State, sphere_of_influence};
pub use integrator::{
    GYROSCOPIC_ITERATIONS, Integrator, MAX_ROTATION_LENGTH_ERROR, SemiImplicitEuler, SpinStep,
    cayley_rotation, gyroscopic_step, integrate_rotation, rotation_from_scaled_axis,
};
pub use mass::MassProperties;
pub use material::{CombineRule, ContactMaterial, SurfaceMaterial};
pub use orbit::{Orbit, propagate};
pub use query::{
    Penetration, ShapeHit, capsule_penetration_vs_aabb, capsule_penetration_vs_capsule,
    capsule_penetration_vs_sphere, ray_vs_aabb, ray_vs_capsule, ray_vs_sphere,
    sphere_overlaps_aabb, sphere_overlaps_capsule, sphere_overlaps_sphere, swept_capsule_vs_aabb,
    swept_capsule_vs_capsule, swept_capsule_vs_sphere, swept_sphere_vs_aabb,
    swept_sphere_vs_capsule, swept_sphere_vs_sphere,
};
pub use system::{EntityOverlapQueries, PhysicsSystem};
pub use wind::WindQuery;
pub use world::{BroadphaseStats, ColliderId, OverlapQueries, PhysicsWorld, QueryScratch};
