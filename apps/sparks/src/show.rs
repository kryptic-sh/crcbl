//! The show: which effects are running, when they start and stop, and what the
//! page reads off them.
//!
//! # It runs itself
//!
//! Nothing here reads a key, and that is the point rather than an omission —
//! `apps/bracket` is shaped the same way and for the same reason. A visitor who
//! loads the page sees sparks struck off the anvil, smoke coming and going at
//! the vent, and a hostile effect held at its share, with nothing to press.
//! **The browser gate depends on it**: the count it watches rise and fall has
//! to rise and fall without anything reaching the page, or the check would be
//! testing the input path instead of the simulation.
//!
//! # There is no server here, and that is the split the plan draws
//!
//! `docs/plan/20-particles.md`'s first line is that visual-only VFX are
//! "client + GPU. Spawn → simulate → render entirely on device; zero gameplay
//! reads, zero readbacks", and that "gameplay-relevant particles are not
//! particles — they're entities". So there is no `crcbl::ecs` module and no
//! `InMemoryTransport` on this page: an effect is fire-and-forget presentation,
//! and putting one behind a wire would be demonstrating the wrong thing.
//!
//! # The schedule
//!
//! ```text
//!  sparks   ▮        ▮        ▮        ▮       one burst every SPARK_PERIOD_TICKS
//!  puff     ▬▬▬▬▬▬▬▬▬·······▬▬▬▬▬▬▬▬▬·······   PUFF_ON_TICKS on, PUFF_OFF_TICKS off
//!  spam     ▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬   never stops, never grows
//! ```

use crcbl::vfx::{EffectId, Live, ParticleSystem};

use crate::effects::{self, POOL, SPAM_SHARE};
use crate::stage;

/// How fast the simulation is stepped, in ticks a second.
///
/// The engine's own default rate, and — because the whole point of the
/// randomness in `crcbl-vfx` is that a fixed seed and a fixed step replay — the
/// number that makes this page the same page for everybody who loads it.
pub const TICK_HZ: u32 = 60;

/// How often a burst of sparks is struck, in ticks.
///
/// Longer than the sparks' own lifetime, so at most one burst is alight at a
/// time and [`stage::Drawn::sparks`] never has more particles to draw than it
/// has instances. `a_burst_is_over_before_the_next_one_is_struck` asserts it.
pub const SPARK_PERIOD_TICKS: u64 = 96;

/// How long the smoke puff emits for, in ticks.
pub const PUFF_ON_TICKS: u64 = 120;

/// How long it is off for.
///
/// Comfortably longer than the puff's own lifetime, so the count reaches
/// **zero** for several heartbeats rather than merely falling — which is the
/// half of the browser gate's rise-and-fall pair that a demo can quietly fail.
pub const PUFF_OFF_TICKS: u64 = 180;

/// The published seed: the show everybody who loads the page watches.
pub const DEFAULT_SEED: u32 = 0x5EED_5A17;

/// An odd multiplier, so consecutive bursts get seeds that share no low bits.
///
/// The bursts must differ from each other — a page striking the identical
/// spray every second and a half reads as a loop rather than as an effect — and
/// the sequence must still be a function of the seed alone, which is why this
/// is a counter and not a clock.
const BURST_STRIDE: u32 = 0x9E37_79B9;

/// What the page and the heartbeat both read.
///
/// One struct rather than a dozen accessors, because the two consumers must
/// report the *same instant*: a panel that read its counts one call at a time
/// while the simulation stepped between them would show a pool that does not
/// add up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// Particles alive across every effect.
    pub live: u32,
    /// How many of them are the impact sparks'.
    pub sparks: u32,
    /// How many are the smoke puff's.
    pub puff: u32,
    /// How many are the hostile effect's.
    pub spam: u32,
    /// The hostile effect's share, which is the number [`Reading::spam`] is
    /// held at.
    pub spam_share: u32,
    /// How many spawns its budget has refused since the page opened.
    pub spam_clamped: u64,
    /// Whether the puff's emitter is running.
    pub puff_emitting: bool,
    /// Slots held by an effect's range, alive or not.
    pub reserved: u32,
    /// The pool's size.
    pub capacity: u32,
    /// How many effects exist.
    pub effects: u32,
}

/// The effects on the page, and the schedule that starts and stops them.
#[derive(Debug)]
pub struct Show {
    vfx: ParticleSystem,
    seed: u32,
    tick: u64,
    bursts: u32,
    sparks: Option<EffectId>,
    puff: Option<EffectId>,
    spam: EffectId,
}

impl Show {
    /// Opens the pool and adds the one effect that never stops.
    ///
    /// # Panics
    ///
    /// If the hostile effect's description is one the simulation refuses, or if
    /// an empty pool cannot hold it. Both are `crate::effects` being wrong
    /// rather than a condition a run can be in, and
    /// `every_stock_effect_is_one_the_simulation_accepts` is what catches them
    /// before a page does.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let mut vfx = ParticleSystem::new(POOL);
        let spam = vfx
            .add(&effects::spam(), stage::spam_origin(), seed ^ BURST_STRIDE)
            .expect("the hostile effect fits an empty pool");
        Self {
            vfx,
            seed,
            tick: 0,
            bursts: 0,
            sparks: None,
            puff: None,
            spam,
        }
    }

    /// Runs the schedule and advances every effect by one tick.
    ///
    /// The schedule first, so an effect added on this tick emits on it — a
    /// burst that had to wait a frame to appear would be a burst struck a frame
    /// after the hammer.
    pub fn step(&mut self, dt: f32) {
        self.strike();
        self.breathe();
        self.vfx.step(dt);
        self.forget_finished();
        self.tick += 1;
    }

    /// Strikes a burst of sparks, if the last one has burned out and it is
    /// time.
    fn strike(&mut self) {
        if self.sparks.is_some() || !self.tick.is_multiple_of(SPARK_PERIOD_TICKS) {
            return;
        }
        let seed = self.seed ^ self.bursts.wrapping_mul(BURST_STRIDE);
        match self
            .vfx
            .add(&effects::impact_sparks(), stage::spark_origin(), seed)
        {
            Ok(id) => {
                self.sparks = Some(id);
                self.bursts = self.bursts.wrapping_add(1);
            }
            // The pool is momentarily full. Not an error and not silence: a
            // burst that never lands is exactly what a budget doing its job
            // looks like from the outside, and the panel's own clamp counters
            // are what carry it.
            Err(error) => crcbl::log::debug!("sparks: a burst found no room: {error}"),
        }
    }

    /// Starts and stops the smoke puff on its cycle.
    fn breathe(&mut self) {
        let phase = self.tick % (PUFF_ON_TICKS + PUFF_OFF_TICKS);
        if phase == 0 && self.puff.is_none() {
            match self
                .vfx
                .add(&effects::smoke_puff(), stage::puff_origin(), self.seed)
            {
                Ok(id) => self.puff = Some(id),
                Err(error) => crcbl::log::debug!("sparks: the puff found no room: {error}"),
            }
        } else if phase == PUFF_ON_TICKS
            && let Some(id) = self.puff
        {
            self.vfx.stop(id);
        }
    }

    /// Drops the handles of effects the simulation has retired.
    ///
    /// An effect removes itself once its emitter is done and its last particle
    /// has gone, so a handle held past that point is one every later lookup
    /// answers `None` for — and, for the sparks, the thing that would stop the
    /// next burst ever being struck.
    fn forget_finished(&mut self) {
        if self
            .sparks
            .is_some_and(|id| self.vfx.effect_stats(id).is_none())
        {
            self.sparks = None;
        }
        if self
            .puff
            .is_some_and(|id| self.vfx.effect_stats(id).is_none())
        {
            self.puff = None;
        }
    }

    /// How many ticks have been stepped.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    /// The impact sparks' live particles, or `None` between bursts.
    #[must_use]
    pub fn sparks(&self) -> Option<Live<'_>> {
        self.sparks.and_then(|id| self.vfx.live(id))
    }

    /// The smoke puff's, or `None` while it is off and drained.
    #[must_use]
    pub fn puff(&self) -> Option<Live<'_>> {
        self.puff.and_then(|id| self.vfx.live(id))
    }

    /// The hostile effect's, which are always there.
    #[must_use]
    pub fn spam(&self) -> Option<Live<'_>> {
        self.vfx.live(self.spam)
    }

    /// The pool, for the debug panel and for this sample's own tests.
    #[must_use]
    pub const fn vfx(&self) -> &ParticleSystem {
        &self.vfx
    }

    /// Everything the page and the heartbeat report, read at one instant.
    #[must_use]
    pub fn reading(&self) -> Reading {
        let pool = self.vfx.stats();
        let count = |id: Option<EffectId>| {
            id.and_then(|id| self.vfx.effect_stats(id))
                .map_or(0, |effect| effect.live)
        };
        let spam = self.vfx.effect_stats(self.spam);
        Reading {
            live: pool.live,
            sparks: count(self.sparks),
            puff: count(self.puff),
            spam: spam.map_or(0, |effect| effect.live),
            spam_share: SPAM_SHARE,
            spam_clamped: spam.map_or(0, |effect| effect.clamped()),
            puff_emitting: self
                .puff
                .and_then(|id| self.vfx.effect_stats(id))
                .is_some_and(|effect| effect.emitting),
            reserved: pool.reserved,
            capacity: pool.capacity,
            effects: pool.effects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tick at the published rate.
    const DT: f32 = 1.0 / TICK_HZ as f32;

    fn run(ticks: u64) -> Show {
        let mut show = Show::new(DEFAULT_SEED);
        for _ in 0..ticks {
            show.step(DT);
        }
        show
    }

    /// **The page draws particles within a tick of opening.** A demo whose
    /// first effect landed a second in would be a demo whose opening frame is
    /// an empty stage, which is the failure a smoke test reports as a pass.
    #[test]
    fn the_first_tick_already_has_particles_on_it() {
        let show = run(1);
        let reading = show.reading();
        assert!(reading.live > 0, "the first tick spawned nothing");
        assert!(reading.sparks > 0, "no burst was struck on the first tick");
        assert!(reading.puff > 0, "the puff did not start on the first tick");
        assert!(reading.spam > 0, "the hostile effect did not start");
    }

    /// **The hostile effect is held at its share, for ever.** This is
    /// `docs/plan/sample/10-sparks.md`'s budget claim, checked on the page's own
    /// numbers rather than on the simulation's — a demo can hold the budget and
    /// still report it wrongly, and the report is what the browser gate reads.
    #[test]
    fn the_hostile_effect_never_leaves_its_share() {
        let mut show = Show::new(DEFAULT_SEED);
        let mut peak = 0;
        for tick in 0..600 {
            show.step(DT);
            let reading = show.reading();
            peak = peak.max(reading.spam);
            assert!(
                reading.spam <= SPAM_SHARE,
                "on tick {tick} the hostile effect holds {} of a share of {SPAM_SHARE}",
                reading.spam
            );
        }
        assert_eq!(
            peak, SPAM_SHARE,
            "the hostile effect never filled its share, so nothing was clamped"
        );
        assert!(
            show.reading().spam_clamped > 0,
            "the hostile effect was never refused a spawn, so its count is capped by \
             something other than the budget"
        );
    }

    /// **The puff's count rises and comes back to nothing**, which is the pair
    /// the browser gate asserts. Both halves here, so a change that broke
    /// either is caught by a test that runs on every machine rather than only
    /// by one that needs a browser.
    #[test]
    fn the_puff_fills_while_it_emits_and_empties_after_it_stops() {
        let mut show = Show::new(DEFAULT_SEED);
        let mut filled = 0;
        for _ in 0..PUFF_ON_TICKS {
            show.step(DT);
            filled = filled.max(show.reading().puff);
        }
        assert!(
            filled > 0,
            "the puff never had a particle in it while its emitter ran"
        );
        assert!(
            show.reading().puff_emitting,
            "the puff stopped emitting before its cycle was up"
        );

        let mut emptied = None;
        for tick in 0..PUFF_OFF_TICKS {
            show.step(DT);
            if show.reading().puff == 0 {
                emptied = Some(tick);
                break;
            }
        }
        let emptied = emptied.unwrap_or_else(|| {
            panic!(
                "the puff still holds {} particles a full off-phase after it stopped",
                show.reading().puff
            )
        });
        assert!(
            emptied > 0,
            "the puff emptied on the tick it stopped, so nothing outlived the stop"
        );
        assert!(
            !show.reading().puff_emitting,
            "the puff is still reported as emitting after its cycle ended"
        );
    }

    /// And it starts again, so the cycle is a cycle rather than one breath.
    #[test]
    fn the_puff_comes_back_on_the_next_cycle() {
        let show = run(PUFF_ON_TICKS + PUFF_OFF_TICKS + 1);
        let reading = show.reading();
        assert!(reading.puff_emitting, "the puff did not restart");
        assert!(reading.puff > 0, "the restarted puff has no particles");
    }

    /// At most one burst is alight, which is what lets
    /// `crate::stage::Drawn::sparks` be exactly the effect's share.
    #[test]
    fn a_burst_is_over_before_the_next_one_is_struck() {
        let mut show = Show::new(DEFAULT_SEED);
        let mut struck = 0;
        let mut was_alight = false;
        for _ in 0..SPARK_PERIOD_TICKS * 6 {
            show.step(DT);
            let alight = show.reading().sparks > 0;
            if alight && !was_alight {
                struck += 1;
            }
            was_alight = alight;
            assert!(
                show.reading().sparks <= effects::SPARK_SHARE,
                "two bursts are alight at once: {} particles against a share of {}",
                show.reading().sparks,
                effects::SPARK_SHARE
            );
        }
        assert!(
            struck >= 2,
            "only {struck} burst(s) were struck over six periods, so the retrigger \
             is not retriggering"
        );
    }

    /// **The whole show replays from its seed.** The claim `crcbl-vfx` is built
    /// around, asserted through the thing a visitor actually sees.
    #[test]
    fn the_same_seed_runs_the_same_show() {
        let once = run(240);
        let twice = run(240);
        assert_eq!(once.reading(), twice.reading());
        let (a, b) = (once.vfx().pool(), twice.vfx().pool());
        for at in 0..a.capacity() as usize {
            assert_eq!(
                a.positions()[at].to_array().map(f32::to_bits),
                b.positions()[at].to_array().map(f32::to_bits),
                "slot {at} of two runs of the same seed is in a different place"
            );
        }
    }

    /// And a different seed runs a different one — the control, without which
    /// the test above passes for a show that never spawned anything.
    #[test]
    fn a_different_seed_runs_a_different_show() {
        let mut mine = Show::new(DEFAULT_SEED ^ 0x1234_5678);
        for _ in 0..240 {
            mine.step(DT);
        }
        let theirs = run(240);
        let differs = (0..mine.vfx().pool().capacity() as usize)
            .any(|at| mine.vfx().pool().positions()[at] != theirs.vfx().pool().positions()[at]);
        assert!(differs, "two seeds produced the identical pool");
    }

    /// The pool is never oversubscribed, which is what says the three shares
    /// coexist rather than starving each other.
    #[test]
    fn every_effect_keeps_its_share_while_the_others_run() {
        let show = run(600);
        let reading = show.reading();
        assert!(
            reading.reserved <= reading.capacity,
            "{} slots reserved out of {}",
            reading.reserved,
            reading.capacity
        );
        assert!(reading.effects >= 2, "effects went missing from the show");
        assert!(
            reading.live >= reading.spam,
            "the pool's own count is below one effect's"
        );
    }
}
