//! What an effect is: where particles start, how fast they arrive, and the
//! fixed menu of modifiers that act on them.
//!
//! `docs/plan/20-particles.md` gives this in RON, and this is the same shape in
//! Rust. The asset format is deliberately not here — `docs/backlog.md` says why
//! — so an effect is written as a value and the parser is the slice that adds
//! `EffectDesc: Deserialize` and nothing else.
//!
//! # The modifier menu is a struct, not a list
//!
//! [`Modifiers`] has one field per modifier, and adding a modifier is adding a
//! field. That is the plan's contract read literally: "the modifier set is a
//! fixed menu evaluated from the effect's params (not a per-particle VM)", with
//! "modifier creep toward a VM" named as the risk. A `Vec<Modifier>` would be
//! the first step down that road — it makes order significant, makes the same
//! modifier applicable twice, and puts a branch in the inner loop that a
//! compute shader would have to carry too.

use glam::Vec3;

use crate::ramp::{Curve, Gradient};

/// Where a particle starts and which way it leaves.
///
/// Two shapes, which is what the demo needs and no more; the plan's list also
/// has sphere, hemisphere, box, ring and mesh-surface, and each is a `match`
/// arm and a sampling formula when something asks for one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Every particle starts at the effect's origin and leaves in a uniformly
    /// random direction.
    ///
    /// Uniform over the *sphere*, not over the two hashed words: sampling
    /// spherical angles directly would crowd particles at the poles. See
    /// `crate::spawn` for the formula.
    Point,
    /// Every particle starts at the effect's origin and leaves inside a cone.
    Cone {
        /// The cone's centre line. Normalised when the effect is validated, so
        /// a caller may hand over any non-zero vector.
        axis: Vec3,
        /// Half the cone's opening, in radians. Zero is a beam; `PI` is
        /// [`Shape::Point`].
        half_angle: f32,
    },
}

/// How many particles an effect asks for, and when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spawn {
    /// One shot, on the first step after the effect is added.
    Burst {
        /// How many particles the shot asks for. The pool may grant fewer.
        count: u32,
    },
    /// A steady stream until the effect is stopped.
    Rate {
        /// Particles per second. Carried across steps as a fraction, so a rate
        /// below one per step still emits.
        per_second: f32,
    },
}

/// The fixed modifier menu, evaluated from parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifiers {
    /// Constant acceleration, in metres per second squared. Add it to the
    /// velocity each step.
    pub gravity: Vec3,
    /// Linear drag coefficient, in reciprocal seconds.
    ///
    /// Applied as `v / (1 + drag * dt)` — backward Euler — rather than
    /// `v * exp(-drag * dt)`. Two reasons, and neither is performance: the
    /// implicit form is unconditionally stable, so a large `drag` slows a
    /// particle instead of oscillating it, and it is exact IEEE arithmetic,
    /// where `exp` is a library function whose last bit differs between
    /// platforms. This crate's determinism claim is narrower than that anyway
    /// (see the crate docs), but the inner loop should not be what narrows it.
    pub drag: f32,
    /// A multiplier on the particle's own base size, over its normalised age.
    ///
    /// A multiplier rather than an absolute size, which is how the plan's
    /// `Curve([(0.0, 1.0), (1.0, 0.2)])` reads: the size a particle is *born*
    /// at is hashed from [`EffectDesc::size`], and this shapes it.
    pub size: Curve,
    /// The particle's colour, over its normalised age. Linear RGBA.
    pub color: Gradient,
}

impl Default for Modifiers {
    /// No gravity, no drag, constant size and opaque white — the effect of
    /// doing nothing, so a caller overrides only the fields it means.
    fn default() -> Self {
        Self {
            gravity: Vec3::ZERO,
            drag: 0.0,
            size: Curve::constant(1.0),
            color: Gradient::constant(glam::Vec4::ONE),
        }
    }
}

/// Why an effect description could not be simulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectError {
    /// A field is not a finite number.
    ///
    /// A `NaN` anywhere in the parameters spreads to every particle it touches
    /// and never leaves — a `NaN` position fails every retirement comparison,
    /// so the particle occupies its slot until the effect is removed.
    NotFinite {
        /// Which field.
        field: &'static str,
    },
    /// A field is outside the range that field can mean.
    OutOfRange {
        /// Which field.
        field: &'static str,
    },
    /// The effect asked for no slots at all, so it could never emit.
    NoBudget,
}

impl std::fmt::Display for EffectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotFinite { field } => write!(f, "effect field `{field}` is not finite"),
            Self::OutOfRange { field } => {
                write!(f, "effect field `{field}` is outside its allowed range")
            }
            Self::NoBudget => write!(f, "effect asked for a budget of zero particles"),
        }
    }
}

impl std::error::Error for EffectError {}

/// One effect, as a value.
///
/// The pairs are `(min, max)` and are hashed per particle from the particle's
/// own index, so two particles of one effect differ and particle *k* of a given
/// seed is always the same particle.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDesc {
    /// How many particles, and when.
    pub spawn: Spawn,
    /// Where they start and which way they go.
    pub shape: Shape,
    /// Seconds a particle lives, `(min, max)`.
    pub lifetime: (f32, f32),
    /// Metres per second it leaves at, `(min, max)`.
    pub speed: (f32, f32),
    /// Metres across it is born, `(min, max)`, before [`Modifiers::size`].
    pub size: (f32, f32),
    /// Radians per second it turns about its own axis, `(min, max)`.
    ///
    /// Signed on purpose: a symmetric range is debris tumbling both ways.
    pub spin: (f32, f32),
    /// The modifier menu.
    pub modifiers: Modifiers,
    /// The effect's share of the pool, in slots.
    ///
    /// This is the budget, and it is a hard cap rather than a hint: an effect
    /// never holds more than this many particles at once, whatever its spawn
    /// asks for. The pool may grant fewer when it is crowded — see
    /// [`RangeAllocator::alloc_clamped`](crate::RangeAllocator::alloc_clamped).
    pub max_particles: u32,
}

impl EffectDesc {
    /// Check the description, and normalise what needs normalising.
    ///
    /// Returns the description with [`Shape::Cone`]'s axis unit length, because
    /// the spawn path builds a frame from it and a frame from a non-unit axis
    /// silently scales every particle's velocity.
    ///
    /// # Errors
    ///
    /// [`EffectError`] naming the first field that is not finite, is outside
    /// its range, or leaves the effect with no budget.
    pub fn validated(&self) -> Result<EffectDesc, EffectError> {
        let finite = |value: f32, field| {
            if value.is_finite() {
                Ok(())
            } else {
                Err(EffectError::NotFinite { field })
            }
        };
        let pair = |(lo, hi): (f32, f32), field| -> Result<(), EffectError> {
            finite(lo, field)?;
            finite(hi, field)?;
            if hi < lo {
                return Err(EffectError::OutOfRange { field });
            }
            Ok(())
        };

        pair(self.lifetime, "lifetime")?;
        if self.lifetime.0 <= 0.0 {
            // A zero lifetime makes the normalised age a division by zero, and
            // a negative one retires the particle before it is drawn.
            return Err(EffectError::OutOfRange { field: "lifetime" });
        }
        pair(self.speed, "speed")?;
        pair(self.size, "size")?;
        pair(self.spin, "spin")?;
        finite(self.modifiers.drag, "drag")?;
        if self.modifiers.drag < 0.0 {
            return Err(EffectError::OutOfRange { field: "drag" });
        }
        if !self.modifiers.gravity.is_finite() {
            return Err(EffectError::NotFinite { field: "gravity" });
        }
        if self.max_particles == 0 {
            return Err(EffectError::NoBudget);
        }

        let mut out = self.clone();
        match self.spawn {
            Spawn::Burst { .. } => {}
            Spawn::Rate { per_second } => {
                finite(per_second, "per_second")?;
                if per_second < 0.0 {
                    return Err(EffectError::OutOfRange {
                        field: "per_second",
                    });
                }
            }
        }
        if let Shape::Cone { axis, half_angle } = self.shape {
            if !axis.is_finite() {
                return Err(EffectError::NotFinite { field: "axis" });
            }
            finite(half_angle, "half_angle")?;
            if !(0.0..=std::f32::consts::PI).contains(&half_angle) {
                return Err(EffectError::OutOfRange {
                    field: "half_angle",
                });
            }
            let axis = axis
                .try_normalize()
                .ok_or(EffectError::OutOfRange { field: "axis" })?;
            out.shape = Shape::Cone { axis, half_angle };
        }
        Ok(out)
    }
}
