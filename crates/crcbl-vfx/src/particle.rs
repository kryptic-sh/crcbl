//! The per-particle values, hashed from (effect seed, particle index).
//!
//! Every random number a particle ever has comes from this module, and none of
//! it is stored except where the update reads it more than once. Particle *k*
//! of effect seed *s* draws the same lifetime, the same direction and the same
//! spin whenever it is asked, on any step and in any order — which is what
//! `docs/plan/20-particles.md` means by "stateless, replayable".
//!
//! # Two streams, six values
//!
//! [`pcg3d`] returns three words per call, so the values a
//! particle needs are grouped into two calls by *when* they are wanted:
//! [`Motion`] at spawn only, [`Life`] on every step. Splitting them the other
//! way — one call per value — would triple the update's hashing for nothing.

use glam::{Quat, Vec3};

use crate::effect::{EffectDesc, Shape};
use crate::hash::{pcg3d, range, unit};

/// The stream that decides where a particle goes: two words of direction and
/// one of speed. Drawn once, at spawn.
const STREAM_MOTION: u32 = 0;

/// The stream that decides what a particle looks like and how long it lasts.
/// Drawn again on every step, because storing it would be storing what a hash
/// can hand back.
const STREAM_LIFE: u32 = 1;

/// A full turn, for the azimuth of both shapes.
const TAU: f32 = std::f32::consts::TAU;

/// Where a particle starts out going, drawn at spawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Motion {
    /// Unit direction.
    pub direction: Vec3,
    /// Metres per second along it.
    pub speed: f32,
}

/// What a particle is, re-drawn on every step.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Life {
    /// The age it retires at, in seconds.
    pub lifetime: f32,
    /// The width it is born at, in metres, before the size curve.
    pub base_size: f32,
    /// Radians per second about its own axis.
    pub spin: f32,
}

/// The motion of particle `index` of an effect seeded `seed`.
pub(crate) fn motion(desc: &EffectDesc, seed: u32, index: u32) -> Motion {
    let words = pcg3d([seed, index, STREAM_MOTION]);
    Motion {
        direction: direction(desc.shape, words[0], words[1]),
        speed: range(words[2], desc.speed.0, desc.speed.1),
    }
}

/// The life of particle `index` of an effect seeded `seed`.
pub(crate) fn life(desc: &EffectDesc, seed: u32, index: u32) -> Life {
    let words = pcg3d([seed, index, STREAM_LIFE]);
    Life {
        lifetime: range(words[0], desc.lifetime.0, desc.lifetime.1),
        base_size: range(words[1], desc.size.0, desc.size.1),
        spin: range(words[2], desc.spin.0, desc.spin.1),
    }
}

/// A unit direction inside `shape`, from two hashed words.
///
/// # Both shapes sample the solid angle, not the angles
///
/// The cosine of the polar angle is what is drawn uniformly, and the azimuth
/// separately. Drawing the polar *angle* uniformly instead is the classic
/// mistake, and it is one that looks almost right: a sphere so sampled is dense
/// at its poles, and a wide cone so sampled is a dense pencil with a thin
/// skirt. Uniform-in-cosine is uniform per unit of surface, which is what an
/// author drawing a cone means by it.
fn direction(shape: Shape, u: u32, v: u32) -> Vec3 {
    let (cos_theta, axis) = match shape {
        Shape::Point => (range(u, 1.0, -1.0), Vec3::Z),
        Shape::Cone { axis, half_angle } => (range(u, 1.0, half_angle.cos()), axis),
    };
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let (sin_phi, cos_phi) = (TAU * unit(v)).sin_cos();
    let local = Vec3::new(sin_theta * cos_phi, sin_theta * sin_phi, cos_theta);
    if axis == Vec3::Z {
        local
    } else {
        // `axis` is unit length — `EffectDesc::validated` is what guarantees it
        // — which is `from_rotation_arc`'s precondition.
        Quat::from_rotation_arc(Vec3::Z, axis) * local
    }
}
