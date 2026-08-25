//! The simulation: effects over a pool, stepped at a fixed rate.

use glam::Vec3;

use crate::effect::{EffectDesc, EffectError, Spawn};
use crate::particle::{Life, life, motion};
use crate::pool::{Live, ParticlePool};
use crate::ranges::{RangeAllocator, SlotRange};
use crate::stats::{EffectStats, PoolStats};

/// A handle to an effect instance, valid until the effect is removed or
/// finishes on its own.
///
/// Never reused: the counter behind it only goes up, so a handle to a finished
/// effect reads as absent rather than as somebody else's effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectId(u64);

/// Why an effect could not be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddError {
    /// The description is not one that can be simulated.
    Effect(EffectError),
    /// Every slot in the pool is already owned by an effect.
    ///
    /// Distinct from the budget clamp, which is not an error: an effect that
    /// asks for more than is free still gets a range and still emits. This is
    /// the case where there is nothing at all to give.
    PoolFull,
}

impl From<EffectError> for AddError {
    fn from(error: EffectError) -> Self {
        Self::Effect(error)
    }
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Effect(error) => write!(f, "{error}"),
            Self::PoolFull => write!(f, "the particle pool has no free slot"),
        }
    }
}

impl std::error::Error for AddError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Effect(error) => Some(error),
            Self::PoolFull => None,
        }
    }
}

/// One live effect instance.
#[derive(Clone, Debug)]
struct Effect {
    id: EffectId,
    desc: EffectDesc,
    origin: Vec3,
    seed: u32,
    range: SlotRange,
    live: u32,
    /// The index the next particle of this effect is hashed from, which only
    /// ever goes up.
    next_index: u32,
    emitting: bool,
    /// The fraction of a particle a rate emitter is owed, carried between
    /// steps so a rate below one per step still emits.
    carry: f32,
    requested: u64,
    granted: u64,
}

impl Effect {
    fn stats(&self) -> EffectStats {
        EffectStats {
            id: self.id,
            live: self.live,
            reserved: self.range.len,
            budget: self.desc.max_particles,
            requested: self.requested,
            granted: self.granted,
            emitting: self.emitting,
        }
    }
}

/// Effects over one pool.
///
/// # A step
///
/// ```
/// use crcbl_vfx::{EffectDesc, Modifiers, ParticleSystem, Shape, Spawn};
/// use glam::Vec3;
///
/// let mut vfx = ParticleSystem::new(4096);
/// let sparks = EffectDesc {
///     spawn: Spawn::Burst { count: 64 },
///     shape: Shape::Cone { axis: Vec3::Y, half_angle: 0.5 },
///     lifetime: (0.2, 0.6),
///     speed: (4.0, 9.0),
///     size: (0.03, 0.06),
///     spin: (-8.0, 8.0),
///     modifiers: Modifiers { gravity: Vec3::new(0.0, -9.8, 0.0), drag: 1.2, ..Modifiers::default() },
///     max_particles: 128,
/// };
///
/// let id = vfx.add(&sparks, Vec3::ZERO, 0x5EED)?;
/// vfx.step(1.0 / 60.0);
/// assert_eq!(vfx.live(id).map(|live| live.len()), Some(64));
/// # Ok::<(), crcbl_vfx::AddError>(())
/// ```
///
/// # Determinism, and exactly how far it goes
///
/// The same effects added in the same order with the same seeds, stepped with
/// the same `dt` the same number of times, leave the pool bit-for-bit
/// identical. `tests/determinism.rs` asserts that over every array, and it is
/// what makes golden frames of an effect possible at all.
///
/// **The claim is per machine, not across machines.** The randomness is integer
/// arithmetic and the integrator is `+`, `*` and `/`, all of them
/// exactly-rounded — but a cone's direction goes through `sin_cos`, which is a
/// library function whose last bit differs between platforms. That is the same
/// limit `docs/backlog.md` records for the rest of the engine's float output,
/// and it is the right limit here too: the destination for this simulation is a
/// compute shader, where the trigonometry is hardware and agrees with no CPU's.
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pool: ParticlePool,
    ranges: RangeAllocator,
    effects: Vec<Effect>,
    next_id: u64,
    live: u32,
    requested: u64,
    granted: u64,
}

impl ParticleSystem {
    /// A system over a pool of `capacity` particles.
    ///
    /// The pool is allocated here and never grows: a budget that can be
    /// exceeded is not a budget, and the whole point of
    /// `docs/plan/20-particles.md`'s pool share is that the worst case is known
    /// before the frame starts.
    pub fn new(capacity: u32) -> Self {
        Self {
            pool: ParticlePool::new(capacity),
            ranges: RangeAllocator::new(capacity),
            effects: Vec::new(),
            next_id: 0,
            live: 0,
            requested: 0,
            granted: 0,
        }
    }

    /// Add an effect at `origin`, drawing its randomness from `seed`.
    ///
    /// The effect emits from the next [`step`](Self::step); nothing is spawned
    /// here.
    ///
    /// # Errors
    ///
    /// [`AddError::Effect`] if the description cannot be simulated, or
    /// [`AddError::PoolFull`] if no slot at all is free. Asking for a bigger
    /// share than is free is **not** an error — the range is clamped to what
    /// there is and [`EffectStats::short_of_budget`] says so.
    pub fn add(
        &mut self,
        desc: &EffectDesc,
        origin: Vec3,
        seed: u32,
    ) -> Result<EffectId, AddError> {
        let desc = desc.validated()?;
        let range = self
            .ranges
            .alloc_clamped(desc.max_particles)
            .ok_or(AddError::PoolFull)?;
        let id = EffectId(self.next_id);
        self.next_id += 1;
        self.effects.push(Effect {
            id,
            desc,
            origin,
            seed,
            range,
            live: 0,
            next_index: 0,
            emitting: true,
            carry: 0.0,
            requested: 0,
            granted: 0,
        });
        Ok(id)
    }

    /// Stop an effect's emitter, leaving the particles it already has to live
    /// out their lifetimes.
    ///
    /// Returns whether an effect by that id was there to stop. The effect
    /// removes itself once its last particle retires.
    pub fn stop(&mut self, id: EffectId) -> bool {
        match self.effects.iter_mut().find(|effect| effect.id == id) {
            Some(effect) => {
                effect.emitting = false;
                true
            }
            None => false,
        }
    }

    /// Remove an effect and every particle it holds, now.
    ///
    /// Returns whether an effect by that id was there to remove.
    pub fn remove(&mut self, id: EffectId) -> bool {
        match self.effects.iter().position(|effect| effect.id == id) {
            Some(at) => {
                let effect = self.effects.remove(at);
                self.live -= effect.live;
                self.ranges.free(effect.range);
                true
            }
            None => false,
        }
    }

    /// Advance every effect by `dt` seconds.
    ///
    /// Existing particles age, move and are retired first; the emitters then
    /// spawn into whatever room that left. Emitting after the update rather
    /// than before it is what puts a new particle at its emitter for the frame
    /// it is born on — emitting first would give a burst one step of travel
    /// before it is ever drawn, which reads as a hollow shell rather than a
    /// flash.
    ///
    /// # Panics
    ///
    /// If `dt` is negative or not finite. A backwards step is not a rewind —
    /// ages would run down and nothing would ever retire — and a `NaN` one
    /// silently poisons every position it reaches, so both fail here rather
    /// than in a frame nobody is looking at.
    pub fn step(&mut self, dt: f32) {
        assert!(
            dt.is_finite() && dt >= 0.0,
            "a particle step needs a finite, non-negative dt, got {dt}"
        );
        // Backwards, so removing a finished effect does not skip the one that
        // slid into its place.
        for at in (0..self.effects.len()).rev() {
            let effect = &mut self.effects[at];
            update(effect, &mut self.pool, dt);
            let (requested, granted) = emit(effect, &mut self.pool, dt);
            self.requested = self.requested.saturating_add(requested);
            self.granted = self.granted.saturating_add(granted);
            if !effect.emitting && effect.live == 0 {
                let effect = self.effects.remove(at);
                self.ranges.free(effect.range);
            }
        }
        self.live = self.effects.iter().map(|effect| effect.live).sum();
    }

    /// The alive particles of one effect, as the parallel slices a renderer
    /// walks. `None` once the effect is gone.
    pub fn live(&self, id: EffectId) -> Option<Live<'_>> {
        let effect = self.effects.iter().find(|effect| effect.id == id)?;
        let span = effect.range.start as usize..(effect.range.start + effect.live) as usize;
        Some(Live {
            position: &self.pool.positions()[span.clone()],
            rotation: &self.pool.rotations()[span.clone()],
            size: &self.pool.sizes()[span.clone()],
            color: &self.pool.colors()[span],
        })
    }

    /// The pool itself, for a test or a panel that wants more than [`Live`]
    /// hands out.
    pub fn pool(&self) -> &ParticlePool {
        &self.pool
    }

    /// Every effect's occupancy, in the order the effects were added.
    pub fn effects(&self) -> impl Iterator<Item = EffectStats> + '_ {
        self.effects.iter().map(Effect::stats)
    }

    /// One effect's occupancy. `None` once the effect is gone.
    pub fn effect_stats(&self, id: EffectId) -> Option<EffectStats> {
        self.effects
            .iter()
            .find(|effect| effect.id == id)
            .map(Effect::stats)
    }

    /// The whole pool's occupancy.
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            capacity: self.ranges.capacity(),
            live: self.live,
            reserved: self.ranges.capacity() - self.ranges.free_slots(),
            effects: self.effects.len() as u32,
            free_spans: self.ranges.spans().len() as u32,
            requested: self.requested,
            granted: self.granted,
        }
    }

    /// How many particles are alive across every effect.
    pub fn live_count(&self) -> u32 {
        self.live
    }
}

/// Age, integrate and retire one effect's particles.
///
/// Retirement is a swap with the last live particle of the range, so the live
/// ones stay packed at the front — the same compaction a GPU update does, and
/// what makes [`ParticleSystem::live`] a slice rather than an index list.
fn update(effect: &mut Effect, pool: &mut ParticlePool, dt: f32) {
    let desc = &effect.desc;
    let mut slot = effect.range.start as usize;
    let mut end = (effect.range.start + effect.live) as usize;
    while slot < end {
        pool.age[slot] += dt;
        // The lifetime comes off the array rather than out of a hash, because
        // this test runs for every particle including the ones about to go and
        // there is no reason to hash for those.
        if pool.age[slot] >= pool.lifetime[slot] {
            end -= 1;
            pool.move_slot(end, slot);
            continue;
        }
        let velocity =
            (pool.velocity[slot] + desc.modifiers.gravity * dt) / (1.0 + desc.modifiers.drag * dt);
        pool.velocity[slot] = velocity;
        pool.position[slot] += velocity * dt;

        shade(pool, slot, desc, life(desc, effect.seed, pool.index[slot]));
        slot += 1;
    }
    effect.live = end as u32 - effect.range.start;
}

/// Ask the emitter how many particles it wants, and spawn what fits.
///
/// Returns what it asked for and what it got, which differ exactly when the
/// effect's pool share is the thing holding it back.
fn emit(effect: &mut Effect, pool: &mut ParticlePool, dt: f32) -> (u64, u64) {
    let want = match effect.desc.spawn {
        Spawn::Burst { count } => {
            if effect.emitting {
                effect.emitting = false;
                count
            } else {
                0
            }
        }
        Spawn::Rate { .. } if !effect.emitting => 0,
        Spawn::Rate { per_second } => {
            effect.carry += per_second * dt;
            if effect.carry >= u32::MAX as f32 {
                // A rate this high has already lost every fraction it was
                // carrying; keeping the remainder would only overflow again.
                effect.carry = 0.0;
                u32::MAX
            } else {
                let whole = effect.carry.floor();
                effect.carry -= whole;
                whole as u32
            }
        }
    };

    let room = effect.range.len - effect.live;
    let granted = want.min(room);
    effect.requested = effect.requested.saturating_add(want as u64);
    effect.granted = effect.granted.saturating_add(granted as u64);

    let desc = &effect.desc;
    for _ in 0..granted {
        let slot = (effect.range.start + effect.live) as usize;
        let index = effect.next_index;
        // Wrapping, so an emitter that has run for a very long time keeps
        // spawning; the hash treats every index alike and a repeat after four
        // billion particles is a repeat nobody can see.
        effect.next_index = effect.next_index.wrapping_add(1);

        let motion = motion(desc, effect.seed, index);
        let drawn = life(desc, effect.seed, index);
        pool.position[slot] = effect.origin;
        pool.velocity[slot] = motion.direction * motion.speed;
        pool.age[slot] = 0.0;
        pool.lifetime[slot] = drawn.lifetime;
        pool.index[slot] = index;
        shade(pool, slot, desc, drawn);
        effect.live += 1;
    }

    (want as u64, granted as u64)
}

/// Write the four attributes a draw reads, from the particle's age and what its
/// hash hands back.
///
/// Called by both the update and the spawn, and that is the point rather than
/// tidiness: a spawn that wrote these itself would write a *different* zero.
/// `spin * 0.0` is negative zero for a particle spinning backwards, a literal
/// `0.0` is positive zero, and two runs that differ only in which one they took
/// are two runs that are not bit-identical. `tests/determinism.rs`'s
/// `a_zero_step_changes_nothing` is what found it.
fn shade(pool: &mut ParticlePool, slot: usize, desc: &EffectDesc, drawn: Life) {
    let t = (pool.age[slot] / pool.lifetime[slot]).clamp(0.0, 1.0);
    pool.size[slot] = drawn.base_size * desc.modifiers.size.eval(t);
    pool.rotation[slot] = drawn.spin * pool.age[slot];
    pool.color[slot] = desc.modifiers.color.eval(t);
}
