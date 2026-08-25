//! What the debug panel reads.
//!
//! `docs/plan/20-particles.md`'s VFX panel wants "live effects, particle counts
//! vs budgets, pool occupancy", and its budget rule is that a hostile effect
//! "clamps to its pool share and the panel shows it". Showing it is what these
//! two types are for: every number a clamp produces is counted, so the
//! difference between what an emitter asked for and what it got is a reading
//! rather than an inference from a frame rate.

use crate::system::EffectId;

/// One effect's occupancy and its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectStats {
    /// Which effect.
    pub id: EffectId,
    /// How many of its particles are alive now.
    pub live: u32,
    /// How many slots the pool actually gave it.
    pub reserved: u32,
    /// How many slots it asked the pool for — its `max_particles`.
    ///
    /// Below [`reserved`](Self::reserved) when the pool was too crowded to give
    /// it its whole share.
    pub budget: u32,
    /// How many particles its emitter has asked for since it was added.
    pub requested: u64,
    /// How many it was allowed to spawn.
    pub granted: u64,
    /// Whether its emitter is still running.
    pub emitting: bool,
}

impl EffectStats {
    /// How many spawns the budget refused.
    ///
    /// Zero for an effect inside its share, and climbing for one outside it —
    /// which is the panel reading that says a clamp is doing the work rather
    /// than the frame time.
    pub fn clamped(&self) -> u64 {
        self.requested.saturating_sub(self.granted)
    }

    /// Whether the pool gave this effect less than it asked for.
    pub fn short_of_budget(&self) -> bool {
        self.reserved < self.budget
    }
}

/// The whole pool's occupancy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolStats {
    /// How many slots the pool has.
    pub capacity: u32,
    /// How many particles are alive across every effect.
    pub live: u32,
    /// How many slots are held by an effect's range, alive or not.
    pub reserved: u32,
    /// How many effects exist.
    pub effects: u32,
    /// How many free runs the ranges have been broken into.
    ///
    /// One is an unfragmented pool. It climbs as effects of different sizes
    /// come and go, and it is what a "the pool has room but not in one piece"
    /// clamp looks like from outside.
    pub free_spans: u32,
    /// How many particles every emitter has asked for since the pool was made,
    /// including effects that have since been removed.
    pub requested: u64,
    /// How many of those were allowed to spawn.
    pub granted: u64,
}

impl PoolStats {
    /// How many spawns the budgets refused, over the pool's whole life.
    pub fn clamped(&self) -> u64 {
        self.requested.saturating_sub(self.granted)
    }
}
