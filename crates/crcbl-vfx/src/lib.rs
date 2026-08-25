//! Particle simulation: pooled effects, stateless randomness, a fixed modifier
//! menu.
//!
//! The first slice of `docs/plan/20-particles.md`, and **the CPU staging of
//! it**. That document's destination is a GPU-resident system — spawn and
//! update in compute, an alive count feeding indirect draw arguments, no CPU
//! involvement after the spawn command. Nothing here is that. What is here is
//! the data layout, the randomness and the parameter set that design needs,
//! arranged so the compute pass replaces the loop in `src/system.rs` rather
//! than replacing the crate:
//!
//! ```text
//! ParticlePool     structure of arrays, one entry per attribute — the SSBO's layout
//! RangeAllocator   per-effect allocation ranges out of that pool, with a clamp
//! pcg3d            per-particle randomness as a hash of (seed, index, stream)
//! EffectDesc       the emitter, the shape and the fixed modifier menu
//! ParticleSystem   effects over a pool, stepped at a fixed rate
//! ```
//!
//! # What this slice deliberately does not have
//!
//! No renderer, and no shader. `docs/plan/20-particles.md`'s mesh particles
//! "inject transforms into the stage 3 instance path", so an effect reaches the
//! screen as ordinary instances and rides the culling and draw generation that
//! already exist — `apps/sparks` is the consumer that does it. Billboards,
//! flipbooks, soft particles, ribbons, depth collision and sorting are all
//! later slices with their own passes.
//!
//! No asset format either: an effect is a value, and the RON schema, the bake
//! and the hot reload are one slice with the authoring workbench that drives
//! them. `docs/backlog.md` records both, and what it would take.
//!
//! # Determinism
//!
//! The property the whole design is arranged around, and the one
//! `docs/plan/20-particles.md` says makes golden-frame testing of effects
//! possible: same effects, same seeds, same `dt`, same number of steps, and the
//! pool is bit-for-bit what it was. `tests/determinism.rs` asserts it over
//! every array in the pool.
//!
//! What buys it is that no particle draws from a shared stream.
//! [`hash::pcg3d`] turns (effect seed, particle index, stream) into three
//! words, so particle *k* is the same particle whether it was the first spawned
//! this frame or the ten thousandth, whether the effect beside it retired early
//! or not, and whether the loop runs here or in a workgroup. See [`hash`] for
//! why that rules out `crcbl-rand`'s `Rng`, which is a stream.
//!
//! The claim is per machine — [`ParticleSystem`] says exactly how far it goes.

pub mod effect;
pub mod hash;
mod particle;
pub mod pool;
pub mod ramp;
pub mod ranges;
pub mod stats;
pub mod system;

pub use effect::{EffectDesc, EffectError, Modifiers, Shape, Spawn};
pub use pool::{Live, ParticlePool};
pub use ramp::{Curve, Gradient, Lerp, Ramp, RampError};
pub use ranges::{RangeAllocator, SlotRange};
pub use stats::{EffectStats, PoolStats};
pub use system::{AddError, EffectId, ParticleSystem};
