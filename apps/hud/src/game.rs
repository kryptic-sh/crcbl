//! The ticker: this sample's whole simulation, and the state a frame draws.
//!
//! ```text
//!  Ticker ──▶ HudModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                       └── InMemoryTransport ┘
//!                    │
//!                    └──▶ RenderState ──▶ crate::page
//! ```
//!
//! # Why a pure UI demo runs a server at all
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 — restated by this sample's
//! own doc as "still runs the standard client/server shape; the ticker is a
//! server system" — is that **no sample gets a simple mode that bypasses the
//! architecture**, because the architecture is what is being proven. A HUD demo
//! is the strongest case for an exemption and therefore the best place to
//! refuse one: the numbers the page draws are produced on the authoritative
//! side, on the fixed timestep, by a [`GameModule`] the server owns, exactly
//! like breakout's bricks and flappy's pipes.
//!
//! What it is *not* is a game. There is no player, no input and no ECS
//! component: the module owns a `Ticker` and advances it, which is the
//! "trivial ticker" the plan doc's scope allows and the hard cap it sets.
//!
//! # Everything here is integer arithmetic
//!
//! Health, mana, cooldowns, damage rolls and the wave counter are all `u32`
//! counters stepped by whole ticks, and the one source of variety is
//! [`crcbl::core::rand::hash_u64`] indexed by a roll counter. So two runs of the
//! same seed are the same run on every target, and
//! `the_same_script_replays_bit_identically` compares the state itself rather
//! than a tolerance. The floats appear once, at the edge — [`RenderState`]
//! divides a counter by its maximum so the page has a bar fraction to draw.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::core::rand::hash_u64;
use crcbl::ecs::{GameModule, World};
use crcbl::net::ProtocolCompatibility;
use crcbl::session::Loopback;

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `HUD`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0048_5544,
};

/// The default simulation rate. Reaches the server, the client and the ticker,
/// so there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// The published seed: the course of damage numbers a screenshot of this sample
/// was taken on.
pub const DEFAULT_SEED: u64 = 0x4855_4430_3031;

// ---- the pools ---------------------------------------------------------------

/// A full health pool, in points.
pub const HEALTH_MAX: u32 = 200;

/// The lowest health the ticker will leave the fake player on.
///
/// The page has no death state to draw and this sample has no game to lose, so
/// the health bar bottoms out here instead of emptying. It is a *floor*, not a
/// threshold: nothing else reads it.
pub const HEALTH_FLOOR: u32 = 8;

/// A full mana pool, in points.
pub const MANA_MAX: u32 = 120;

/// How often the fake player takes a hit, in ticks.
pub const HIT_TICKS: u64 = 45;

/// The smallest and largest hit, in points.
pub const HIT_DAMAGE: (u32, u32) = (9, 27);

/// How often health regenerates, in ticks, and by how much.
pub const HEAL_TICKS: u64 = 20;
/// See [`HEAL_TICKS`].
pub const HEAL_AMOUNT: u32 = 3;

/// How often mana regenerates, in ticks, and by how much.
pub const MANA_TICKS: u64 = 10;
/// See [`MANA_TICKS`].
pub const MANA_AMOUNT: u32 = 3;

// ---- the ability row ---------------------------------------------------------

/// How many ability slots the page draws.
pub const ABILITY_COUNT: usize = 4;

/// One slot on the ability row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilitySpec {
    /// What the slot is labelled.
    pub name: &'static str,
    /// How long the slot cools down for after a cast, in ticks.
    pub cooldown: u32,
    /// What a cast costs, in mana points.
    pub cost: u32,
    /// The smallest and largest damage number a cast rolls.
    pub damage: (u32, u32),
}

/// The four slots, cheapest first.
///
/// The order is the order they are **drawn** in. Casting walks it backwards —
/// see `run_tick` — so the expensive slots get first refusal on a full mana
/// pool and the row shows a mix of ready and cooling slots rather than one
/// cheap ability firing on every cooldown and starving the rest.
pub const ABILITIES: [AbilitySpec; ABILITY_COUNT] = [
    AbilitySpec {
        name: "STRIKE",
        cooldown: 90,
        cost: 8,
        damage: (18, 34),
    },
    AbilitySpec {
        name: "CLEAVE",
        cooldown: 210,
        cost: 22,
        damage: (40, 72),
    },
    AbilitySpec {
        name: "BOLT",
        cooldown: 330,
        cost: 38,
        damage: (85, 130),
    },
    AbilitySpec {
        name: "NOVA",
        cooldown: 510,
        cost: 60,
        damage: (150, 240),
    },
];

// ---- waves and the damage ticker ---------------------------------------------

/// How long a wave lasts, in ticks. Ten seconds at [`DEFAULT_TICK_HZ`].
pub const WAVE_TICKS: u64 = 600;

/// How long the wave banner stays on screen after a wave turns over, in ticks.
pub const BANNER_TICKS: u32 = 120;

/// How often the ticker logs its `[HUD]` line, in ticks.
///
/// A second of simulated time at [`DEFAULT_TICK_HZ`], and the same cadence every
/// other sample uses — `web/tools/browser-e2e.mjs` watches for that heartbeat to
/// tell a paused demo from a running one, and its window is sized for it.
pub const HEARTBEAT_TICKS: u64 = 60;

/// How long a damage number stays on the page, in ticks.
pub const DAMAGE_LIFETIME: u32 = 90;

/// How many columns the damage numbers are dealt into.
///
/// Successive numbers take successive lanes so that two raised on nearby ticks
/// do not draw on top of each other, which is what a real damage ticker does
/// and what makes this one worth having as a fixture.
pub const DAMAGE_LANES: usize = 3;

/// One number on the damage ticker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Damage {
    /// The number drawn.
    pub amount: u32,
    /// How many ticks ago it was raised. Retired at [`DAMAGE_LIFETIME`].
    pub age: u32,
    /// Which of [`DAMAGE_LANES`] columns it drifts up.
    pub lane: usize,
}

// ---- the ticker --------------------------------------------------------------

/// Everything this sample simulates.
///
/// Lives behind an `Arc<Mutex<_>>` shared with [`HudModule`], for the reason
/// every sample's does: [`crcbl::server::Server::set_module`] takes ownership of
/// the module and never hands it back, so the only way to read what a tick
/// produced is a cell both sides hold.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ticker {
    /// What every roll is indexed off.
    seed: u64,
    /// Ticks this ticker has run. The facade asserts it advances by exactly one
    /// per [`Game::tick`].
    ticks: u64,
    /// Which wave the fake fight is on. Starts at one.
    wave: u32,
    /// Ticks the wave banner has left on screen, or zero.
    banner: u32,
    health: u32,
    mana: u32,
    /// Ticks left on each slot of [`ABILITIES`], by index.
    cooldowns: [u32; ABILITY_COUNT],
    /// Live damage numbers, oldest first.
    damage: Vec<Damage>,
    /// How many numbers have been rolled. This is the index into the seed's
    /// stream, so it is simulation state and not a statistic: two runs that
    /// rolled a different *number* of times would diverge from here on.
    rolls: u64,
    /// The lane the next damage number takes.
    lane: usize,
}

impl Ticker {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            ticks: 0,
            wave: 1,
            banner: BANNER_TICKS,
            health: HEALTH_MAX,
            mana: MANA_MAX,
            cooldowns: [0; ABILITY_COUNT],
            damage: Vec::new(),
            rolls: 0,
            lane: 0,
        }
    }

    /// The next number in this seed's stream, in `low..=high`.
    ///
    /// # Panics
    ///
    /// If `high` is below `low`, which every caller reads off [`ABILITIES`] or
    /// [`HIT_DAMAGE`] and none can get wrong at run time.
    fn roll(&mut self, (low, high): (u32, u32)) -> u32 {
        assert!(high >= low, "a roll range runs upwards");
        self.rolls += 1;
        let span = u64::from(high - low) + 1;
        low + u32::try_from(hash_u64(self.seed, self.rolls) % span).unwrap_or(0)
    }

    /// Raises a damage number in the next lane.
    fn hit_for(&mut self, amount: u32) {
        self.damage.push(Damage {
            amount,
            age: 0,
            lane: self.lane,
        });
        self.lane = (self.lane + 1) % DAMAGE_LANES;
    }
}

/// One tick of the fixture.
///
/// The order matters and is the order a fight reads in: the wave turns over,
/// the enemy hits, the pools recover, the cooldowns run down, the numbers
/// already on screen age off, and one ability is cast.
///
/// Three of those placements are load-bearing. Casting **after** the cooldowns
/// tick is what lets a slot whose last tick took it to zero be cast on that same
/// tick rather than a tick later; ageing **before** the cast is what makes a
/// number raised this tick draw at age zero rather than already one tick into
/// its fade; and running the banner's clock down **before** the wave check is
/// what makes it up for [`BANNER_TICKS`] ticks rather than one short, because a
/// banner raised and then decremented in the same tick starts a tick in.
fn run_tick(t: &mut Ticker) {
    t.ticks += 1;

    t.banner = t.banner.saturating_sub(1);
    if t.ticks.is_multiple_of(WAVE_TICKS) {
        t.wave += 1;
        t.banner = BANNER_TICKS;
    }

    if t.ticks.is_multiple_of(HIT_TICKS) {
        let hit = t.roll(HIT_DAMAGE);
        t.health = t.health.saturating_sub(hit).max(HEALTH_FLOOR);
    }
    if t.ticks.is_multiple_of(HEAL_TICKS) {
        t.health = (t.health + HEAL_AMOUNT).min(HEALTH_MAX);
    }
    if t.ticks.is_multiple_of(MANA_TICKS) {
        t.mana = (t.mana + MANA_AMOUNT).min(MANA_MAX);
    }

    for cooldown in &mut t.cooldowns {
        *cooldown = cooldown.saturating_sub(1);
    }

    for number in &mut t.damage {
        number.age += 1;
    }
    t.damage.retain(|number| number.age < DAMAGE_LIFETIME);

    // Backwards, so the expensive slots get first refusal on the pool — see
    // [`ABILITIES`]. At most one cast a tick: two would empty the pool in a
    // burst and leave the whole row cooling at once, which is the one state a
    // row of cooldown indicators has nothing to show.
    for slot in (0..ABILITY_COUNT).rev() {
        let ability = ABILITIES[slot];
        if t.cooldowns[slot] == 0 && t.mana >= ability.cost {
            t.mana -= ability.cost;
            t.cooldowns[slot] = ability.cooldown;
            let amount = t.roll(ability.damage);
            t.hit_for(amount);
            break;
        }
    }
}

/// The ticker, as the server hosts it.
///
/// `register` is empty for the same reason breakout's and flappy's are:
/// `Server::set_module` does not call it, and this sample has no ECS system to
/// register — the whole simulation is the [`Ticker`] behind the shared cell.
struct HudModule {
    shared: Arc<Mutex<Ticker>>,
}

impl std::fmt::Debug for HudModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HudModule").finish_non_exhaustive()
    }
}

impl GameModule for HudModule {
    fn name(&self) -> &str {
        "hud"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, _world: &mut World) {
        run_tick(&mut lock(&self.shared));
    }
}

/// A poisoned mutex here means a previous tick panicked. The ticker is plain
/// counters with no invariant a panic could have half-broken, so recovering the
/// guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<Ticker>) -> MutexGuard<'_, Ticker> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- what a frame draws ------------------------------------------------------

/// One ability slot, as the page draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AbilityView {
    /// Ticks left on the cooldown, or zero when the slot is ready.
    pub cooldown: u32,
    /// Those ticks in seconds, for the slot's own label.
    pub remaining: f32,
}

impl AbilityView {
    /// Whether the slot can be cast.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.cooldown == 0
    }

    /// How much of `slot`'s cooldown is still to run, in `0..=1`.
    ///
    /// One at the instant of the cast and zero when the slot is ready again,
    /// which is the direction a cooldown sweep is drawn in: it covers the slot
    /// and retreats.
    #[must_use]
    pub fn sweep(&self, slot: usize) -> f32 {
        let full = ABILITIES[slot].cooldown;
        if full == 0 {
            return 0.0;
        }
        self.cooldown as f32 / full as f32
    }
}

/// One damage number, as the page draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageView {
    /// The number.
    pub amount: u32,
    /// How far through its life it is, in `0..1`. The page turns this into
    /// height and fade.
    pub age: f32,
    /// Which of [`DAMAGE_LANES`] columns it drifts up.
    pub lane: usize,
}

/// Everything [`crate::page`] draws, snapshotted once a frame.
///
/// A plain struct rather than a borrow of the ticker: the page runs on the
/// frame's thread and the ticker is behind a mutex the server's tick holds, and
/// a page that read through the lock would be holding it for the length of a
/// draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    /// Ticks the simulation has run.
    pub tick: u64,
    /// Which wave the fight is on.
    pub wave: u32,
    /// Whether the wave banner is up this frame.
    pub banner: bool,
    /// Health remaining, in points.
    pub health: u32,
    /// Mana remaining, in points.
    pub mana: u32,
    /// The ability row, in [`ABILITIES`] order.
    pub abilities: [AbilityView; ABILITY_COUNT],
    /// The damage ticker, oldest first. Refilled rather than reallocated.
    pub damage: Vec<DamageView>,
}

impl RenderState {
    /// How much of the health pool is left, in `0..=1` — the health bar's fill.
    #[must_use]
    pub fn health_fraction(&self) -> f32 {
        self.health as f32 / HEALTH_MAX as f32
    }

    /// How much of the mana pool is left, in `0..=1` — the mana bar's fill.
    #[must_use]
    pub fn mana_fraction(&self) -> f32 {
        self.mana as f32 / MANA_MAX as f32
    }

    /// How many ability slots can be cast right now.
    #[must_use]
    pub fn ready_count(&self) -> usize {
        self.abilities
            .iter()
            .filter(|ability| ability.is_ready())
            .count()
    }
}

// ---- the debug panel's section -----------------------------------------------

/// The ticker's numbers, for the debug overlay.
///
/// Snapshotted in [`crate::app::Hud`]'s `draw` rather than read at panel time,
/// because `HostedGame::debug_sections` is handed `&self` while reading the
/// ticker needs the lock. Flappy's `CourseStats` is the same arrangement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudStats {
    /// Ticks the simulation has run.
    pub ticks: u64,
    /// Which wave the fight is on.
    pub wave: u32,
    /// Health remaining, in points.
    pub health: u32,
    /// Mana remaining, in points.
    pub mana: u32,
    /// How many ability slots are ready.
    pub ready: usize,
    /// How many damage numbers are on the page.
    pub damage: usize,
    /// How many numbers this run has rolled — the ticker's rng cursor, and the
    /// one number here that a replay comparison hangs on.
    pub rolls: u64,
}

impl crcbl::ui::DebugModule for HudStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("hud");
        section.row("tick", format_args!("{}", self.ticks));
        section.row("wave", format_args!("{}", self.wave));
        section.row("health", format_args!("{}/{HEALTH_MAX}", self.health));
        section.row("mana", format_args!("{}/{MANA_MAX}", self.mana));
        section.row("ready", format_args!("{}/{ABILITY_COUNT}", self.ready));
        section.row("dmg", format_args!("{}", self.damage));
        section.row("rolls", format_args!("{}", self.rolls));
    }
}

// ---- the facade --------------------------------------------------------------

/// What can stop hud before it starts.
#[derive(Debug)]
pub enum GameError {
    /// The operating system would not seed the server's resume credential.
    Server(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(message) => write!(f, "server creation failed: {message}"),
        }
    }
}

impl std::error::Error for GameError {}

/// The ticker, its server, its client, and the clock that drives all three.
pub struct Game {
    /// The server, its client and the transport between them. One field rather
    /// than three: the tick rate, the compatibility and the transport pair are
    /// what both halves must agree on, and [`Loopback::new`] is where they are
    /// made to.
    session: Loopback,
    shared: Arc<Mutex<Ticker>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// The tick period in seconds, for the ability row's countdown labels.
    tick_secs: f32,
    /// The wave the last [`Game::log_heartbeat`] line reported, so a wave that
    /// turns over between two heartbeats gets a line of its own. Zero before the
    /// first tick, which is a wave the ticker never has — so the opening line is
    /// logged on tick one rather than a second into the run.
    logged_wave: u32,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ticks_run", &self.ticks_run)
            .finish_non_exhaustive()
    }
}

impl Game {
    /// Builds the server, its client and the ticker between them.
    ///
    /// `tick_hz` is the one simulation rate in the process; `seed` decides every
    /// damage number this run will roll, so two games built with the same seed
    /// and stepped the same number of times are the same game — which is what
    /// `the_same_script_replays_bit_identically` rests on.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(tick_hz: u32, seed: u64) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let shared = Arc::new(Mutex::new(Ticker::new(seed)));

        // An empty world, and that is the honest shape. This sample has no
        // entity and no ECS system: what the server hosts is the module, and
        // what the module owns is the ticker.
        let session = Loopback::new(
            World::new(),
            Box::new(HudModule {
                shared: Arc::clone(&shared),
            }),
            tick_hz,
            COMPATIBILITY,
        )
        .map_err(|error| GameError::Server(error.to_string()))?;

        let tick_period = session.tick_period();
        crcbl::log::info!(
            "sim: {tick_hz} Hz, {:.3} ms per tick, seed {seed:#x}",
            tick_period.as_secs_f64() * 1e3,
        );
        Ok(Self {
            session,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            tick_secs: tick_period.as_secs_f32(),
            logged_wave: 0,
        })
    }

    /// Advances the server, and with it the ticker, by exactly one tick.
    pub fn tick(&mut self) {
        let before = lock(&self.shared).ticks;

        self.sim_time += self.tick_period;
        let server_ticks = self.session.server_mut().update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        self.session.client_mut().update(self.sim_time);
        self.ticks_run += 1;

        debug_assert_eq!(
            lock(&self.shared).ticks,
            before + u64::from(server_ticks),
            "the ticker must run exactly once per server tick",
        );
        self.log_heartbeat();
    }

    /// The `[HUD]` line, on the same cadence and in the same shape breakout,
    /// flappy, asteroids and horde use: every sixty ticks, which is a second of
    /// simulated time, plus the tick a wave turns over on so the change is not
    /// swallowed by the gap.
    ///
    /// It is the only thing this sample logs from inside the tick, and
    /// `web/tools/browser-e2e.mjs` reads two different claims out of it. One is
    /// the same claim every demo makes — a heartbeat exists while the demo runs
    /// and stops while it is paused, which is how a browser tells a paused loop
    /// from a running one. The other is this sample's, and it is the only one
    /// available here: hud takes **no input**, so nothing external can be shown
    /// to have reached the simulation, and "the ticker is advancing under its
    /// own steam" has to be read off a number the ticker moves. `rolls` is that
    /// number — the rng cursor, which only advances when the script actually
    /// rolls a hit or a cast, so a ticker that ran and did nothing would leave
    /// it standing still.
    fn log_heartbeat(&mut self) {
        let ticker = lock(&self.shared);
        let wave_changed = ticker.wave != self.logged_wave;
        if !wave_changed && !ticker.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  wave: {}  hp: {}/{HEALTH_MAX}  mana: {}/{MANA_MAX}  \
             ready: {}/{ABILITY_COUNT}  dmg: {}  rolls: {}",
            ticker.ticks,
            ticker.wave,
            ticker.health,
            ticker.mana,
            ticker
                .cooldowns
                .iter()
                .filter(|cooldown| **cooldown == 0)
                .count(),
            ticker.damage.len(),
            ticker.rolls,
        );
        self.logged_wave = ticker.wave;
    }

    /// How many times [`Game::tick`] has been called.
    #[must_use]
    pub const fn ticks_run(&self) -> u64 {
        self.ticks_run
    }

    /// Refills `out` with what the page should draw this frame.
    pub fn render_state(&self, out: &mut RenderState) {
        let ticker = lock(&self.shared);
        out.tick = ticker.ticks;
        out.wave = ticker.wave;
        out.banner = ticker.banner > 0;
        out.health = ticker.health;
        out.mana = ticker.mana;
        for (slot, view) in out.abilities.iter_mut().enumerate() {
            view.cooldown = ticker.cooldowns[slot];
            view.remaining = view.cooldown as f32 * self.tick_secs;
        }
        out.damage.clear();
        out.damage
            .extend(ticker.damage.iter().map(|number| DamageView {
                amount: number.amount,
                age: number.age as f32 / DAMAGE_LIFETIME as f32,
                lane: number.lane,
            }));
    }

    /// The ticker's numbers for the debug panel.
    #[must_use]
    pub fn stats(&self) -> HudStats {
        let ticker = lock(&self.shared);
        HudStats {
            ticks: ticker.ticks,
            wave: ticker.wave,
            health: ticker.health,
            mana: ticker.mana,
            ready: ticker
                .cooldowns
                .iter()
                .filter(|cooldown| **cooldown == 0)
                .count(),
            damage: ticker.damage.len(),
            rolls: ticker.rolls,
        }
    }

    /// The whole ticker folded to a 64-bit hash, over its **logical** values.
    ///
    /// Field by field in a fixed order through [`hash_u64`], which is integer
    /// arithmetic identical on every target — never over the bytes the struct
    /// happens to occupy, which would fold in padding and a `Vec`'s heap
    /// pointer. A hash can collide and the values cannot, so this is the
    /// per-tick *where* instrument and the end-of-run comparison in
    /// `the_same_script_replays_bit_identically` is the *whether*.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let ticker = lock(&self.shared);
        let mut h: u64 = 0x4855_4420_4649_5854;
        let mut fold = |value: u64| h = hash_u64(h, value);
        fold(ticker.ticks);
        fold(u64::from(ticker.wave));
        fold(u64::from(ticker.banner));
        fold(u64::from(ticker.health));
        fold(u64::from(ticker.mana));
        fold(ticker.rolls);
        fold(ticker.lane as u64);
        for cooldown in ticker.cooldowns {
            fold(u64::from(cooldown));
        }
        for number in &ticker.damage {
            fold(u64::from(number.amount));
            fold(u64::from(number.age));
            fold(number.lane as u64);
        }
        h
    }
}

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A game stepped `ticks` times on `seed`.
    fn played(seed: u64, ticks: u64) -> Game {
        let mut game = Game::new(DEFAULT_TICK_HZ, seed).expect("OS entropy in a test process");
        for _ in 0..ticks {
            game.tick();
        }
        game
    }

    fn render(game: &Game) -> RenderState {
        let mut state = RenderState::default();
        game.render_state(&mut state);
        state
    }

    /// **The determinism criterion.** The same script — this sample's script is
    /// its tick count, because it takes no input — replays to the same state,
    /// twice, and agrees at every tick along the way rather than only at the
    /// end.
    ///
    /// The per-tick hashes are what make a divergence report *where*: two runs
    /// that ended up equal by luck after diverging in the middle would pass an
    /// end-state comparison alone.
    #[test]
    fn the_same_script_replays_bit_identically() {
        let run = || {
            let mut game =
                Game::new(DEFAULT_TICK_HZ, DEFAULT_SEED).expect("OS entropy in a test process");
            let mut hashes = Vec::with_capacity(1_800);
            for _ in 0..1_800 {
                game.tick();
                hashes.push(game.state_hash());
            }
            (game.stats(), render(&game), hashes)
        };

        let first = run();
        assert!(
            first.0.rolls > 40,
            "the reference run rolled {} numbers, which is not enough to compare",
            first.0.rolls,
        );
        assert!(first.0.wave > 1, "the reference run never turned a wave");
        assert!(
            !first.1.damage.is_empty(),
            "the reference run ended with an empty damage ticker",
        );
        assert!(
            first.1.abilities.iter().any(|a| !a.is_ready()),
            "the reference run ended with nothing on cooldown",
        );

        let second = run();
        let divergence = first
            .2
            .iter()
            .zip(&second.2)
            .position(|(a, b)| a != b)
            .map(|at| at + 1);
        assert_eq!(divergence, None, "the two runs diverged at a tick");
        assert_eq!(first, second, "two identical scripts must agree exactly");
    }

    /// The ticker's state at a named tick, so a change to the rhythm is a change
    /// somebody has to bless rather than notice.
    ///
    /// Tick 600 is the first wave turn-over, which is why it is the one picked:
    /// it exercises the wave counter, the banner it raises, both regenerators
    /// and the cast rule at once.
    #[test]
    fn the_ticker_reaches_a_known_state_at_the_first_wave_turn_over() {
        let game = played(DEFAULT_SEED, WAVE_TICKS);
        let state = render(&game);
        assert_eq!(state.tick, 600);
        assert_eq!(state.wave, 2, "600 ticks is one wave");
        assert!(state.banner, "a wave that turned over raises its banner");
        assert_eq!(state.health, 46);
        assert_eq!(state.mana, 42);
        assert_eq!(
            state.abilities.map(|ability| ability.cooldown),
            [60, 33, 62, 0],
            "the row is a mix of ready and cooling, which is what it exists to show",
        );
        assert_eq!(state.ready_count(), 1);
        assert_eq!(
            state.damage.iter().map(|d| d.amount).collect::<Vec<_>>(),
            vec![30],
        );

        // And the banner comes down again, on the tick it is owed and not one
        // later. Up for `BANNER_TICKS` ticks counting the turn-over itself, so
        // the last tick it is up on is `600 + 120 - 1`.
        let last_up = WAVE_TICKS + u64::from(BANNER_TICKS) - 1;
        assert!(render(&played(DEFAULT_SEED, last_up)).banner);
        assert!(!render(&played(DEFAULT_SEED, last_up + 1)).banner);
    }

    /// The bars report the fraction of each pool that is left — the number the
    /// page multiplies its track width by.
    #[test]
    fn a_bars_fill_is_the_fraction_of_its_pool_that_is_left() {
        let full = render(&played(DEFAULT_SEED, 0));
        assert_eq!(full.health, HEALTH_MAX);
        assert_eq!(full.health_fraction(), 1.0, "a full pool fills its track");
        assert_eq!(full.mana_fraction(), 1.0);

        // Half of each pool, built directly: the ticker never sits on a round
        // number, and a fraction test that had to hunt for one would be
        // asserting the rhythm rather than the arithmetic.
        let half = RenderState {
            health: HEALTH_MAX / 2,
            mana: MANA_MAX / 2,
            ..RenderState::default()
        };
        assert_eq!(half.health_fraction(), 0.5);
        assert_eq!(half.mana_fraction(), 0.5);
        assert_eq!(RenderState::default().health_fraction(), 0.0);
    }

    /// A slot is ready until it is cast and cools for exactly its own cooldown.
    ///
    /// Slot 3 is `NOVA`, the only one the opening full pool can pay for, so the
    /// very first tick casts it and nothing else can.
    #[test]
    fn a_slot_cools_for_exactly_its_own_cooldown_after_a_cast() {
        let fresh = render(&played(DEFAULT_SEED, 0));
        assert!(
            fresh.abilities.iter().all(AbilityView::is_ready),
            "every slot starts ready",
        );
        assert_eq!(fresh.ready_count(), ABILITY_COUNT);

        let cast = render(&played(DEFAULT_SEED, 1));
        assert_eq!(
            cast.abilities[3].cooldown, ABILITIES[3].cooldown,
            "the full pool pays for the most expensive slot first",
        );
        assert_eq!(cast.abilities[3].sweep(3), 1.0, "a fresh cooldown is full");
        assert!(!cast.abilities[3].is_ready());
        assert_eq!(cast.ready_count(), ABILITY_COUNT - 1);
        // 510 ticks at 60 Hz. Compared to a tolerance rather than by equality
        // because the impl multiplies by the tick period the clock reports and
        // this expression divides — they agree to six figures and not to the
        // last bit, and pinning the last bit would be asserting the arithmetic
        // rather than the countdown.
        let seconds = ABILITIES[3].cooldown as f32 / DEFAULT_TICK_HZ as f32;
        assert!(
            (cast.abilities[3].remaining - seconds).abs() < 1e-3,
            "{} is not {seconds} seconds",
            cast.abilities[3].remaining,
        );

        // Halfway down: cast on tick 1, then 255 ticks of countdown. The sweep
        // is what the page draws over the slot, and it has to retreat rather
        // than grow.
        let midway = render(&played(
            DEFAULT_SEED,
            u64::from(ABILITIES[3].cooldown / 2) + 1,
        ));
        assert_eq!(midway.abilities[3].cooldown, ABILITIES[3].cooldown / 2);
        assert_eq!(midway.abilities[3].sweep(3), 0.5);
    }

    /// A number leaves the ticker when its life runs out, and not before.
    #[test]
    fn a_damage_number_is_retired_when_its_life_runs_out() {
        let raised = render(&played(DEFAULT_SEED, 1));
        assert_eq!(raised.damage.len(), 1, "the first tick casts once");
        assert_eq!(raised.damage[0].age, 0.0);

        let last = render(&played(DEFAULT_SEED, u64::from(DAMAGE_LIFETIME)));
        assert!(
            last.damage
                .iter()
                .any(|d| d.amount == raised.damage[0].amount),
            "the first number must survive its whole life: {:?}",
            last.damage,
        );

        let gone = render(&played(DEFAULT_SEED, u64::from(DAMAGE_LIFETIME) + 1));
        assert!(
            !gone.damage.iter().any(|d| d.age >= 1.0),
            "nothing past its life stays on the page: {:?}",
            gone.damage,
        );
    }

    /// The seed reaches the rolls, so `--seed` is a flag that changes the run
    /// rather than a number the parser stores and nothing reads.
    #[test]
    fn a_different_seed_rolls_different_damage() {
        let amounts = |seed: u64| {
            render(&played(seed, 400))
                .damage
                .iter()
                .map(|number| number.amount)
                .collect::<Vec<_>>()
        };
        let published = amounts(DEFAULT_SEED);
        assert!(
            !published.is_empty(),
            "a run with no numbers proves nothing"
        );
        assert_ne!(published, amounts(DEFAULT_SEED + 1));
        assert_eq!(published, amounts(DEFAULT_SEED), "the same seed repeats");
    }

    /// One call to [`Game::tick`] is one server tick is one turn of the ticker.
    ///
    /// The join the `debug_assert`s inside `tick` make in a debug build, made
    /// here as a real assertion so a release-mode run is covered too.
    #[test]
    fn every_call_to_tick_advances_the_server_and_the_ticker_by_one() {
        let mut game = Game::new(DEFAULT_TICK_HZ, DEFAULT_SEED).expect("OS entropy");
        for expected in 1..=30u64 {
            game.tick();
            assert_eq!(game.ticks_run(), expected);
            assert_eq!(game.stats().ticks, expected, "the ticker kept pace");
        }
    }

    /// The state hash covers the values that move, which is the property that
    /// makes it worth comparing at all: a hash folding only the tick count
    /// would agree across two runs that disagreed about everything else.
    #[test]
    fn the_state_hash_changes_when_the_state_does() {
        let mut game = Game::new(DEFAULT_TICK_HZ, DEFAULT_SEED).expect("OS entropy");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..120 {
            game.tick();
            seen.insert(game.state_hash());
        }
        assert_eq!(seen.len(), 120, "some ticks hashed the same");
        assert_ne!(
            game.state_hash(),
            played(DEFAULT_SEED + 1, 120).state_hash(),
            "two seeds that rolled different numbers hashed the same",
        );
    }

    /// The panel section carries the ticker's own numbers, not a heading alone.
    #[test]
    fn the_debug_section_reports_the_ticker_that_produced_it() {
        let game = played(DEFAULT_SEED, WAVE_TICKS);
        let stats = game.stats();
        let mut section = crcbl::ui::DebugSection::default();
        crcbl::ui::DebugModule::debug_section(&stats, &mut section);

        assert_eq!(section.title(), "hud");
        let rows: Vec<(&str, &str)> = section
            .rows()
            .iter()
            .map(|row| (row.label.as_str(), row.value.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("tick", "600"),
                ("wave", "2"),
                ("health", "46/200"),
                ("mana", "42/120"),
                ("ready", "1/4"),
                ("dmg", "1"),
                ("rolls", "26"),
            ],
        );
    }
}
