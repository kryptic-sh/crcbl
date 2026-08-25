//! The particle pool: one array per attribute, sized once.
//!
//! # Structure of arrays, and why it is not a CPU micro-optimisation
//!
//! `docs/plan/20-particles.md` asks for a "global particle pool (SSBO,
//! structure-of-arrays: position, velocity, age/lifetime, size, rotation,
//! color seed, atlas frame, emitter id)". The layout is chosen for the
//! destination, not for this step: a compute pass reads one attribute across a
//! workgroup's worth of particles at a time, and an array of structs would make
//! every one of those reads a strided gather. Keeping the CPU staging in the
//! same layout means the buffers can be uploaded as they stand and the update
//! rewritten in place, rather than the pool being transposed on the way to the
//! GPU.
//!
//! # The arrays, against the plan's list
//!
//! Six of the plan's eight are here under their own names. The two that differ:
//!
//! * **`color` holds the evaluated colour, not a "color seed".** In the GPU
//!   design the seed is what a shader needs to look a gradient up per fragment.
//!   Here the update writes the colour the draw consumes, which is what makes
//!   [`Live`] a set of slices a renderer can walk without evaluating anything.
//! * **`index` holds the particle's spawn index, in place of an "emitter id".**
//!   An emitter id is what a *compacting* global pool needs to know whose
//!   parameters apply to a slot; with per-effect ranges the range says that
//!   already, and a field nothing reads is one this project deletes. The index
//!   is what a stateless hash is keyed on, and the update re-derives the
//!   particle's base size and spin from it every step rather than storing
//!   either — the plan's replayability, demonstrated rather than asserted.
//!
//! `atlas frame` has no array because there are no flipbooks in this slice.

use glam::{Vec3, Vec4};

/// The alive particles of one effect, as parallel slices.
///
/// What a renderer reads. The four slices are the same length and are indexed
/// together: `position[i]`, `rotation[i]`, `size[i]` and `color[i]` are one
/// particle.
#[derive(Clone, Copy, Debug)]
pub struct Live<'a> {
    /// Where each particle is, in the effect's own space — which is world
    /// space, because an effect's origin is baked into the particle at spawn.
    pub position: &'a [Vec3],
    /// How far each has turned about its own axis, in radians.
    pub rotation: &'a [f32],
    /// How wide each is, in metres.
    pub size: &'a [f32],
    /// What colour each is, linear RGBA.
    pub color: &'a [Vec4],
}

impl Live<'_> {
    /// How many particles are alive.
    pub fn len(&self) -> usize {
        self.position.len()
    }

    /// Whether the effect has no live particles.
    pub fn is_empty(&self) -> bool {
        self.position.is_empty()
    }
}

/// One array per particle attribute, all `capacity` long.
///
/// Every slot exists for the pool's whole life; an effect owns a
/// [`SlotRange`](crate::SlotRange) of them and its live particles occupy the
/// front of that range. Slots outside any live range hold whatever the last
/// particle there left, which nothing reads — the alive count is the boundary,
/// not a sentinel value in the data.
#[derive(Clone, Debug)]
pub struct ParticlePool {
    pub(crate) position: Vec<Vec3>,
    pub(crate) velocity: Vec<Vec3>,
    pub(crate) age: Vec<f32>,
    pub(crate) lifetime: Vec<f32>,
    pub(crate) size: Vec<f32>,
    pub(crate) rotation: Vec<f32>,
    pub(crate) color: Vec<Vec4>,
    pub(crate) index: Vec<u32>,
}

impl ParticlePool {
    /// A pool of `capacity` slots, every array allocated and zeroed.
    ///
    /// Allocated once. Nothing in a step grows an array, which is what lets the
    /// same buffers become GPU allocations later without the sizes moving.
    pub fn new(capacity: u32) -> Self {
        let n = capacity as usize;
        Self {
            position: vec![Vec3::ZERO; n],
            velocity: vec![Vec3::ZERO; n],
            age: vec![0.0; n],
            lifetime: vec![0.0; n],
            size: vec![0.0; n],
            rotation: vec![0.0; n],
            color: vec![Vec4::ZERO; n],
            index: vec![0; n],
        }
    }

    /// How many slots the pool has.
    pub fn capacity(&self) -> u32 {
        self.position.len() as u32
    }

    /// Every slot's position, live or not.
    pub fn positions(&self) -> &[Vec3] {
        &self.position
    }

    /// Every slot's velocity, in metres per second.
    pub fn velocities(&self) -> &[Vec3] {
        &self.velocity
    }

    /// Every slot's age, in seconds since it was spawned.
    pub fn ages(&self) -> &[f32] {
        &self.age
    }

    /// Every slot's lifetime, in seconds — the age it retires at.
    pub fn lifetimes(&self) -> &[f32] {
        &self.lifetime
    }

    /// Every slot's width, in metres.
    pub fn sizes(&self) -> &[f32] {
        &self.size
    }

    /// Every slot's rotation about its own axis, in radians.
    pub fn rotations(&self) -> &[f32] {
        &self.rotation
    }

    /// Every slot's colour, linear RGBA.
    pub fn colors(&self) -> &[Vec4] {
        &self.color
    }

    /// Every slot's spawn index within its effect — the hash key its
    /// randomness is drawn from.
    pub fn indices(&self) -> &[u32] {
        &self.index
    }

    /// Move the particle at `from` onto `to`, every array together.
    ///
    /// The compaction the update does when a particle retires: the last live
    /// particle of the range takes the dead one's slot, so the live particles
    /// stay at the front of the range with no gaps. Which is the same thing a
    /// stream compaction does on the GPU, and it is why [`Live`] can be a
    /// slice.
    pub(crate) fn move_slot(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        self.position[to] = self.position[from];
        self.velocity[to] = self.velocity[from];
        self.age[to] = self.age[from];
        self.lifetime[to] = self.lifetime[from];
        self.size[to] = self.size[from];
        self.rotation[to] = self.rotation[from];
        self.color[to] = self.color[from];
        self.index[to] = self.index[from];
    }
}
