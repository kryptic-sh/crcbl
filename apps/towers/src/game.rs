//! The simulation: the field, the creeps on it, the towers shooting and holding
//! them, and the server that owns all three.
//!
//! ```text
//!  Stage ──▶ TowersModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                         └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::app, crate::page
//! ```
//!
//! # Solo is the same game over a loopback, and rule 2 has no exemption
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 is server-authoritative
//! always, and this sample's own document says the co-op build and the solo
//! build are one binary: `PlaceTower`, `UpgradeTower` and `StartWave` are
//! **commands** the client seals into bytes, the transport carries and the
//! server validates. So they are exactly that here, over `InMemoryTransport`,
//! and the validation — is that plot free, is there gold for it, has that tower
//! already been stepped up, is a wave already running — happens on the server's
//! side of the wire in `Stage::place_tower`, `Stage::upgrade_tower` and
//! [`crate::wave::Waves::start_now`]. A refused command is **counted**, so "the
//! server said no" is a number a run reports rather than something it swallows,
//! and it is **told** to the player who sent it, naming the rule — a
//! [`Refusal`], which [`Game::take_refusals`] hands the front end.
//!
//! **Co-op is the same stage on another server.** A LAN host ticks the same
//! `TowersModule` from a `crcbl::server::Host`, with every player's commands
//! at once — `run_team_tick` — and the server's world replicates the stage as
//! [`crate::replica`]'s entities, which is all a joiner draws. The `lan` module
//! has the wiring; [`Game`] hides which of the three a frame is reading.
//!
//! # What one tick does, and why it is in that order
//!
//! ```text
//!   1. commands       PlaceTower / UpgradeTower / StartWave / Restart, validated
//!   2. WaveSystem     the table releases a creep, if one is due
//!   3. the hold       every Slow tower's overlap_sphere → Creep::slow_to
//!   4. CreepSystem    every creep walks, at its speed times that hold
//!   5. the exit       overlap_sphere vs the trigger volume → a life
//!   6. ProjectileSystem  every bolt sweeps → damage, a burst, a kill, gold
//!   7. TowerSystem    every shooting tower acquires and fires
//!   8. EconomySystem  win at the end of the table, lose at zero lives
//! ```
//!
//! **The hold is written before the creeps walk**, because a hold that landed
//! after the walk would be a tick late for ever — the creep would cross the
//! tower's reach at full speed and be slowed on its way out the far side.
//! `Stage::hold_the_slowed` clears every creep's hold first rather than
//! tracking who left: see [`crate::creep`]'s module docs for why that is the only
//! encoding that cannot leak.
//!
//! **The creeps move before the bolts sweep**, which is the whole of the CCD
//! claim: a bolt's segment is tested against where its target *is* at the end
//! of this tick rather than where it was at the start of it. And **the towers
//! fire last**, so a tower does not spend a shot on a creep another tower
//! killed on the same tick.
//!
//! # Nothing here does collision or intersection, and that is rule 9
//!
//! Every question about where things are is `crcbl-phys`':
//! [`overlap_sphere`](crcbl::phys::PhysicsWorld::overlap_sphere) for a tower's
//! range, for a slow tower's reach, for a splash burst and for the exit volume,
//! [`sweep_sphere`](crcbl::phys::PhysicsWorld::sweep_sphere) for a bolt. This
//! file decides which query to ask and what an answer means.

use std::cell::Cell;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::ecs::{ClientInputs, DebugCtx, Entity, GameModule, SystemTrait, World};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{ColliderId, PhysicsWorld};
use crcbl::server::{HostModule, PeerId, PeerInputs};
use crcbl::session::Loopback;

use crate::creep::{self, Creep, CreepView};
use crate::map::{MAX_PLOTS, Map};
use crate::tower::{self, Bolt, BoltOutcome, BurstView, Tier, Tower, TowerView};
use crate::wave::{self, MAX_CREEPS, Outcome, STARTING_GOLD, STARTING_LIVES, Waves};

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half of the schema spells `TWR`.
///
/// **`protocol_version` is 2 because the command frame grew.** Slice 3 added the
/// kind a `PlaceTower` names and the `UpgradeTower` command beside it, so
/// [`INTENT_BYTES`] went from two bytes to four and the flag byte gained a
/// meaning — a breaking wire change, which is what that field is documented to
/// count. A slice 1 client hand-shaking with this server would have every
/// command read as a frame of the wrong length and silently dropped; the bump is
/// what turns that into a refused handshake. The schema hash is this sample's
/// identity and does not move with it.
///
/// **And 3 because the snapshot started carrying the field.** The server's
/// world replicates the stage as [`crate::replica::SYSTEM`] since co-op over a
/// LAN, which is what a remote player draws; a client from before would read
/// none of it, and one from after would draw an empty field against an older
/// server.
///
/// **And 4 because the host's map crosses the wire.** A LAN host sends its map
/// to every joiner at join (`crate::map`'s `wire`), and the joiner plays on
/// it whatever its own `--scene` says — so the map is no longer part of what
/// two builds must agree on, and it left the schema, where a fingerprint of it
/// had been folded in to make a joiner on another map refuse the host. A
/// client from before would wait for no map and be refused on a schema it
/// computed from its own; the bump makes the two refuse each other by version
/// instead, which the lobby names.
///
/// Every session hand-shakes on this, solo's included, so there is one rule
/// for every session rather than one for the LAN.
pub(crate) const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 4,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0054_5752,
};

/// The default simulation rate. Reaches the server, the client and the stage,
/// so there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How often the `[HUD]` heartbeat is logged, in ticks: one second of simulated
/// time at [`DEFAULT_TICK_HZ`].
pub const HEARTBEAT_TICKS: u64 = 60;

/// How long a finished run is left on screen before it starts again, in
/// seconds.
///
/// **A demo that has stopped is indistinguishable from a loop that has
/// stopped**, which is the argument `apps/breach`'s warm-up makes about its
/// empty room: a browser visitor arriving at a field with `LOST` written across
/// it and nothing moving cannot tell the difference. Long enough to read the
/// result, short enough that the next wave is on its way before anyone
/// reloads. `R` restarts a run without waiting.
pub const RESTART_S: f64 = 4.0;

// ---------------------------------------------------------------------------
// Controls and the wire
// ---------------------------------------------------------------------------

/// What the player is asking for this tick.
///
/// Every field but [`Controls::kind`] is an **edge**: building, stepping a tower
/// up, starting a wave and restarting are things a key press does once, not
/// things a held key does sixty times a second. Which plot is highlighted is not
/// here at all — that is presentation, and what crosses the wire is the plot a
/// player actually pressed build on.
///
/// [`Controls::kind`] is the exception and is a **state**: it is which kind the
/// `1`/`2`/`3` keys last picked, and it travels on every frame because it is what
/// a build means. A client that sent it only on the tick the key was pressed
/// would be asking the server to remember a client's selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// Build a tower on this plot.
    pub place: Option<u8>,
    /// …of this kind. Read only when [`Controls::place`] names a plot, and sent
    /// always — see the type's docs.
    pub kind: tower::Kind,
    /// Step the tower on this plot up a tier.
    pub upgrade: Option<u8>,
    /// Start the next wave now rather than at the end of the build phase.
    pub start_wave: bool,
    /// Throw the run away and start again.
    pub restart: bool,
}

/// One client's command frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    place: Option<u8>,
    kind: tower::Kind,
    upgrade: Option<u8>,
    start_wave: bool,
    restart: bool,
}

/// The frame a client seals for what its player asked: what
/// [`Game::set_controls`] holds for the next tick, and what a tool's play
/// controls encode (`play`'s `controls`), so the two cannot spell one command
/// two ways.
impl From<Controls> for Intent {
    fn from(controls: Controls) -> Self {
        Self {
            place: controls.place,
            kind: controls.kind,
            upgrade: controls.upgrade,
            start_wave: controls.start_wave,
            restart: controls.restart,
        }
    }
}

const INTENT_START: u8 = 1 << 0;
const INTENT_RESTART: u8 = 1 << 1;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_START | INTENT_RESTART;

/// The plot byte on a frame that is not building — or not upgrading — anything.
///
/// A sentinel rather than two more flag bits, and it is safe to be one because
/// a map has at most [`MAX_PLOTS`] plots — `the_no_plot_sentinel_is_not_a_plot`
/// asserts that it never becomes a plot index.
const PLOT_NONE: u8 = u8::MAX;

/// How many bytes one sealed command is: a flag byte, the plot to build on, the
/// kind to build there, and the plot to step up.
///
/// **Four since slice 3**, which is why [`COMPATIBILITY`]'s protocol version
/// moved with it.
const INTENT_BYTES: usize = 4;

impl Intent {
    /// The wire form handed to `Client::set_input`.
    fn to_wire(self) -> Vec<u8> {
        let mut flags = 0;
        if self.start_wave {
            flags |= INTENT_START;
        }
        if self.restart {
            flags |= INTENT_RESTART;
        }
        #[allow(clippy::cast_possible_truncation)]
        let kind = self.kind.index() as u8;
        vec![
            flags,
            self.place.unwrap_or(PLOT_NONE),
            kind,
            self.upgrade.unwrap_or(PLOT_NONE),
        ]
    }

    /// The command a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload of the wrong
    /// length, a flag outside [`INTENT_FLAGS`], or a kind byte no row of
    /// [`tower::TOWERS`] has. **The plot bytes are not checked here**, and that
    /// is deliberate — a plot number is a thing the *rules* refuse rather than a
    /// thing the format cannot express, so it travels intact and
    /// [`Stage::place_tower`] turns it down. That is where the refusal is
    /// counted, and recorded against the player who asked, to be told.
    ///
    /// The kind byte is the other way round for the same reason read the other
    /// way: there is no `Kind` to carry, so the frame cannot be decoded at all.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != INTENT_BYTES {
            return None;
        }
        let flags = bytes[0];
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        Some(Self {
            place: (bytes[1] != PLOT_NONE).then_some(bytes[1]),
            kind: tower::Kind::from_index(bytes[2])?,
            upgrade: (bytes[3] != PLOT_NONE).then_some(bytes[3]),
            start_wave: flags & INTENT_START != 0,
            restart: flags & INTENT_RESTART != 0,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's: the flags are OR-ed, because each
    /// is a thing the player asked for and a later frame that says nothing is
    /// not a retraction, and the **last** plot named wins — a player who
    /// pressed build twice inside one tick asked for the second plot, and
    /// merging two builds into one tick would drop a command rather than a
    /// keystroke. The kind is a state rather than an edge, so the last frame's
    /// is simply the current one.
    fn from_inputs(inputs: ClientInputs<'_>) -> Self {
        let mut merged = Self::default();
        for (_tick, data) in inputs.iter() {
            // A frame this build cannot read is skipped rather than taken as an
            // empty command, which would read as the player asking for nothing.
            let Some(frame) = Self::from_wire(data) else {
                continue;
            };
            merged.start_wave |= frame.start_wave;
            merged.restart |= frame.restart;
            merged.kind = frame.kind;
            if frame.place.is_some() {
                merged.place = frame.place;
            }
            if frame.upgrade.is_some() {
                merged.upgrade = frame.upgrade;
            }
        }
        merged
    }
}

/// Why the server turned a command down: each a rule of the game, never a
/// frame it could not read.
///
/// What `Stage::place_tower`, `Stage::upgrade_tower` and the `StartWave`
/// check answer, and what the player who sent the command is told — solo
/// and a LAN host's own player on this process, a joiner through
/// `crate::lan`'s refusal event, which carries [`Refusal::code`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The run is over: nothing is built, stepped up or sent until the next
    /// one starts.
    RunOver,
    /// A build names a plot the map does not have.
    NoSuchPlot,
    /// A build names a plot that already has a tower on it.
    PlotTaken,
    /// The team's purse cannot pay for it.
    NotEnoughGold,
    /// An upgrade names a plot with no tower on it — which covers a plot
    /// that is not a plot at all.
    NoTower,
    /// An upgrade names a tower already at the top tier.
    TopTier,
    /// There is no wave to bring forward: one is releasing, or the table is
    /// spent.
    NoWaveToSend,
}

impl Refusal {
    /// Every refusal, in [`Refusal::code`] order.
    pub const ALL: [Self; 7] = [
        Self::RunOver,
        Self::NoSuchPlot,
        Self::PlotTaken,
        Self::NotEnoughGold,
        Self::NoTower,
        Self::TopTier,
        Self::NoWaveToSend,
    ];

    /// Its byte on the wire. Written out rather than derived from the
    /// declaration order, so reordering the variants cannot change what an
    /// older build reads.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::RunOver => 1,
            Self::NoSuchPlot => 2,
            Self::PlotTaken => 3,
            Self::NotEnoughGold => 4,
            Self::NoTower => 5,
            Self::TopTier => 6,
            Self::NoWaveToSend => 7,
        }
    }

    /// The refusal `code` names, or `None` for a byte no refusal has.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|refusal| refusal.code() == code)
    }

    /// What the player is shown.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RunOver => "THE RUN IS OVER",
            Self::NoSuchPlot => "NO SUCH PLOT",
            Self::PlotTaken => "THAT PLOT IS TAKEN",
            Self::NotEnoughGold => "NOT ENOUGH GOLD",
            Self::NoTower => "NO TOWER TO UPGRADE",
            Self::TopTier => "ALREADY UPGRADED",
            Self::NoWaveToSend => "NO WAVE TO SEND NOW",
        }
    }
}

/// Who sent a command, as the stage records a refusal against them: a LAN
/// host's peer, or `None` for solo's one player, who has no [`PeerId`].
type Sender = Option<PeerId>;

// ---------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------

/// One splash burst, while it is still being drawn.
///
/// The damage it did was applied the instant it was raised — see
/// [`Stage::splash`] — so this is presentation and nothing else, which is why it
/// carries a time rather than a remaining lifetime: a paused demo's bursts stay
/// where they are, because [`Stage::elapsed`] is what they are measured against.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Burst {
    /// Where the bolt stopped, in metres.
    at: DVec3,
    /// How far the overlap that wounded reached, in metres.
    radius_m: f64,
    /// When it was raised, in [`Stage::elapsed`] seconds.
    raised_at: f64,
}

/// Everything this sample simulates.
///
/// Behind an `Arc<Mutex<_>>` shared with [`TowersModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is
/// what reads the result, and the two are not the same call stack.
struct Stage {
    /// The field this stage is played on. Shared with [`Game`], which answers
    /// the client's questions about it without taking the tick's lock, and
    /// kept across a restart — a restart replays the run, not the map.
    map: Arc<Map>,
    world: PhysicsWorld,
    /// The exit volume's id, which is what an overlap's answer is compared
    /// against — see [`crate::creep::has_reached_the_exit`].
    exit: ColliderId,
    creeps: Vec<Creep>,
    towers: Vec<Tower>,
    bolts: Vec<Bolt>,
    /// The bursts still being drawn, oldest first — see [`Burst`].
    bursts: Vec<Burst>,
    waves: Waves,
    /// The team's shared purse and the team's shared lives — one of each,
    /// because co-op is what this sample is for.
    gold: u32,
    lives: u32,
    kills: u64,
    leaks: u64,
    shots: u64,
    built: u64,
    /// How many of each [`tower::Kind`] have been built, indexed by
    /// [`tower::Kind::index`]. The `[HUD]` line carries all three, which is what
    /// lets a gate see that a kind key reached the server rather than only that
    /// *a* tower went up.
    built_by_kind: [u64; tower::KINDS],
    /// How many towers have been stepped up a tier.
    upgrades: u64,
    /// How many commands the server turned down. **The observable that says
    /// validation happens at all**: a build that trusted its client leaves this
    /// at zero while towers appear on occupied plots.
    refused: u64,
    /// The refusals not yet told to whoever sent the command, oldest first:
    /// taken every tick or frame by whatever serves the stage — [`Game`], or
    /// a dedicated server's `Field` — and kept across a reset, which can
    /// follow a refusal in the same tick.
    refusals: Vec<(Sender, Refusal)>,
    outcome: Outcome,
    /// When the run ended, in [`Stage::elapsed`] seconds. Only read once it
    /// has.
    ended_at: f64,
    /// How many runs this stage has played, restarts included. The one number
    /// that survives a restart.
    runs: u64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What every
    /// clock in here is measured against, so a paused demo's waves stay where
    /// they are.
    elapsed: f64,
    /// The overlap queries' output buffer, hoisted so a tick that asks one
    /// question per creep and one per tower allocates nothing.
    scratch: Vec<ColliderId>,
    /// …and the splash burst's own, because [`Stage::splash`] reads its answers
    /// back **while** it wounds and removes creeps, which is the one place a
    /// buffer and the rest of the stage are borrowed in the same breath.
    burst_scratch: Vec<ColliderId>,
}

impl Stage {
    /// An empty field on `map`, with the first build phase running.
    fn new(map: Arc<Map>) -> Self {
        let (world, exit) = map.world();
        Self {
            map,
            world,
            exit,
            creeps: Vec::new(),
            towers: Vec::new(),
            bolts: Vec::new(),
            bursts: Vec::new(),
            waves: Waves::new(),
            gold: STARTING_GOLD,
            lives: STARTING_LIVES,
            kills: 0,
            leaks: 0,
            shots: 0,
            built: 0,
            built_by_kind: [0; tower::KINDS],
            upgrades: 0,
            refused: 0,
            refusals: Vec::new(),
            outcome: Outcome::Playing,
            ended_at: 0.0,
            runs: 1,
            ticks: 0,
            elapsed: 0.0,
            scratch: Vec::new(),
            burst_scratch: Vec::new(),
        }
    }

    /// Throws the run away and starts another one.
    ///
    /// Everything goes, the physics world included — a restart that kept the
    /// old world would keep every dead creep's collider in it. What survives is
    /// [`Stage::runs`], because a demo that has played itself four times should
    /// say so, and the map the run is played on.
    fn reset(&mut self) {
        let runs = self.runs + 1;
        let refusals = std::mem::take(&mut self.refusals);
        *self = Self::new(Arc::clone(&self.map));
        self.runs = runs;
        self.refusals = refusals;
    }

    /// Counts a refused command and records it against `sender`, to be told.
    fn refuse(&mut self, sender: Sender, refusal: Refusal) {
        self.refused += 1;
        self.refusals.push((sender, refusal));
    }

    /// Whether `plot` already has a tower on it.
    fn is_taken(&self, plot: usize) -> bool {
        self.towers.iter().any(|tower| tower.plot() == plot)
    }

    /// Which of [`Stage::towers`] stands on `plot`.
    fn tower_on(&self, plot: usize) -> Option<usize> {
        self.towers.iter().position(|tower| tower.plot() == plot)
    }

    /// The server's half of the `PlaceTower` command: builds a tower of `kind`,
    /// or turns the command down.
    ///
    /// Four ways to be refused, and each is a rule rather than a format
    /// problem: the run is over, the plot is not a plot, the plot is taken, or
    /// the purse is short. A client that predicted the build would have to
    /// predict all four — and the price it would have to predict is the kind's,
    /// which is the fifth thing slice 3 put on the server's side of the wire.
    fn place_tower(&mut self, plot: u8, kind: tower::Kind) -> Result<(), Refusal> {
        let plot = plot as usize;
        let cost = kind.spec(Tier::Base).cost;
        let Some(feet) = self.map.plots().get(plot).map(crate::scene::Plot::at) else {
            return Err(Refusal::NoSuchPlot);
        };
        if self.outcome.is_over() {
            return Err(Refusal::RunOver);
        }
        if self.is_taken(plot) {
            return Err(Refusal::PlotTaken);
        }
        if self.gold < cost {
            return Err(Refusal::NotEnoughGold);
        }
        self.gold -= cost;
        self.towers.push(Tower::new(plot, feet, kind));
        self.built += 1;
        self.built_by_kind[kind.index()] += 1;
        Ok(())
    }

    /// The server's half of the `UpgradeTower` command: steps the tower on
    /// `plot` up a tier, or turns the command down.
    ///
    /// Four ways to be refused, and they are deliberately the same shape as
    /// [`Stage::place_tower`]'s: the run is over, the plot holds no tower — which
    /// covers a plot that is not a plot at all — the tower is already at the top
    /// tier, or the purse is short. The price is the kind's and the tier's, off
    /// [`tower::TOWERS`], so a client could not hard-code it even if it wanted
    /// to predict the command.
    fn upgrade_tower(&mut self, plot: u8) -> Result<(), Refusal> {
        if self.outcome.is_over() {
            return Err(Refusal::RunOver);
        }
        let Some(index) = self.tower_on(plot as usize) else {
            return Err(Refusal::NoTower);
        };
        let Some(cost) = self.towers[index].upgrade_cost() else {
            return Err(Refusal::TopTier);
        };
        if self.gold < cost {
            return Err(Refusal::NotEnoughGold);
        }
        if !self.towers[index].upgrade() {
            return Err(Refusal::TopTier);
        }
        self.gold -= cost;
        self.upgrades += 1;
        Ok(())
    }

    /// Takes `damage` off the creep whose body is `body`, and pays its bounty if
    /// that killed it.
    ///
    /// The one place a creep dies. A stale or unknown id — the ground, the exit
    /// volume, a creep another bolt killed earlier in the same tick — is nothing
    /// at all rather than an error: every caller here is handing over whatever a
    /// `crcbl-phys` query answered with.
    fn wound(&mut self, body: ColliderId, damage: u32) {
        let Some(hit) = self.creeps.iter().position(|creep| creep.body() == body) else {
            return;
        };
        if !self.creeps[hit].wounded(damage) {
            return;
        }
        let bounty = self.creeps[hit].bounty();
        self.creeps.swap_remove(hit).despawn(&mut self.world);
        self.gold += bounty;
        self.kills += 1;
    }

    /// Raises `bolt`'s splash burst where it stopped, and wounds everything in
    /// it but `direct`.
    ///
    /// Nothing at all for a bolt with no burst, which is every
    /// [`tower::Kind::Bolt`] shot — so the projectile pass calls this
    /// unconditionally and the table is what decides.
    ///
    /// `direct` is the creep the bolt struck, already wounded by
    /// [`Stage::wound`]: the overlap answers with it too, and wounding it twice
    /// would make a splash tower quietly better against one creep than against
    /// two.
    fn splash(&mut self, bolt: &Bolt, direct: Option<ColliderId>) {
        if !bolt.bursts() {
            return;
        }
        let (at, radius_m) = (bolt.at(), bolt.burst_m());
        self.bursts.push(Burst {
            at,
            radius_m,
            raised_at: self.elapsed,
        });
        {
            let Stage {
                world,
                burst_scratch,
                ..
            } = &mut *self;
            tower::burst_into(world, at, radius_m, burst_scratch);
        }
        // By index rather than by iterator, because `wound` takes the whole
        // stage: the buffer is read one id at a time and the creep list is
        // swap-removed from underneath.
        for index in 0..self.burst_scratch.len() {
            let body = self.burst_scratch[index];
            if Some(body) == direct {
                continue;
            }
            self.wound(body, bolt.damage());
        }
    }

    /// Lets go of every creep, then has each [`tower::Kind::Slow`] tower hold
    /// what its own overlap finds.
    ///
    /// The release comes first and covers every creep on the field, which is what
    /// makes "the hold ends when the creep leaves" a fact about this tick rather
    /// than a timer — see [`crate::creep`]'s module docs. A tower that held
    /// something is drawn hot for [`tower::FLASH_S`], so a slow tower with
    /// nothing in reach is plainly a slow tower with nothing in reach.
    fn hold_the_slowed(&mut self, now: f64) {
        for creep in &mut self.creeps {
            creep.release();
        }
        for index in 0..self.towers.len() {
            let tower = self.towers[index];
            let spec = tower.spec();
            if !spec.slows() {
                continue;
            }
            let muzzle = tower.muzzle();
            let held = {
                let Stage {
                    world,
                    creeps,
                    scratch,
                    ..
                } = &mut *self;
                tower::hold(
                    world,
                    creeps,
                    muzzle,
                    spec.range_m,
                    spec.slow_factor,
                    scratch,
                )
            };
            if held > 0 {
                self.towers[index].fired(now);
            }
        }
    }
}

/// One tick of the simulation: a command in, and the seven systems in the order
/// the module docs give.
fn run_tick(stage: &mut Stage, intent: Intent, dt: f64) {
    run_team_tick(stage, &[(None, intent)], dt);
}

/// One tick with every player's command in — one per player, in the order the
/// host admitted them, each with who sent it — and then the seven systems.
///
/// **One team, one run.** A restart from anyone throws the run away for
/// everyone, before any other command is read. The rest are validated one
/// player at a time against the one purse, so two players building on the same
/// plot in the same tick get one tower and one refusal, and the first admitted
/// is the one who built it. Each refusal is recorded against the player who
/// sent the command (`Stage::refusals`), for them to be told.
///
/// **No players, no run.** A tick with no command frame at all is a session
/// nobody holds a place in — only a dedicated server's, since solo and a
/// listen host always have their own player — and the stage holds still
/// through it: no clock, so no build phase running out and no wave released
/// at an empty field. A player whose link dropped still holds a place through
/// its grace period, so a run goes on while they reconnect, as it would with
/// them in it.
///
/// **An emptied run is thrown away, once.** A run that had started and then
/// lost its last player — every grace period over, so nobody can come back to
/// it — is reset rather than kept half-played: the next group to join finds a
/// fresh field, not the lives and gold the last one left behind. A reset stage
/// has ticked nothing, so the empty ticks after it hold still like any other.
fn run_team_tick(stage: &mut Stage, intents: &[(Sender, Intent)], dt: f64) {
    if intents.is_empty() {
        if stage.ticks > 0 {
            stage.reset();
        }
        return;
    }
    if intents.iter().any(|(_, intent)| intent.restart) {
        stage.reset();
        return;
    }
    for &(sender, intent) in intents {
        if let Some(plot) = intent.place
            && let Err(refusal) = stage.place_tower(plot, intent.kind)
        {
            stage.refuse(sender, refusal);
        }
        if let Some(plot) = intent.upgrade
            && let Err(refusal) = stage.upgrade_tower(plot)
        {
            stage.refuse(sender, refusal);
        }
        if intent.start_wave {
            if stage.outcome.is_over() {
                stage.refuse(sender, Refusal::RunOver);
            } else if !stage.waves.start_now(stage.elapsed) {
                stage.refuse(sender, Refusal::NoWaveToSend);
            }
        }
    }

    if stage.outcome.is_over() {
        // A finished run is left on screen and then played again — see
        // [`RESTART_S`]. Nothing else steps: the creeps that were walking went
        // with the life that ended it or with the wave that finished.
        if stage.elapsed - stage.ended_at >= RESTART_S {
            stage.reset();
            return;
        }
        stage.ticks += 1;
        stage.elapsed += dt;
        return;
    }

    // 1. The table releases, at most one creep a tick — see
    //    `crate::wave::Waves::step`.
    if let Some(release) = stage.waves.step(stage.elapsed) {
        let creep = Creep::spawn(&mut stage.world, stage.map.path(), release.kind);
        stage.creeps.push(creep);
    }

    // 2. Every slow tower holds what is inside its reach, before anything walks.
    stage.hold_the_slowed(stage.elapsed);

    // 3. Every creep walks — at its kind's speed times whatever is holding it —
    //    and writes its sphere where the walk left it.
    for creep in &mut stage.creeps {
        creep.advance(&mut stage.world, stage.map.path(), dt);
    }

    // 4. The exit volume takes what reached it. One overlap per creep, against
    //    the trigger `crate::map` registered.
    let mut leaked = 0_u32;
    {
        let Stage {
            world,
            creeps,
            exit,
            scratch,
            ..
        } = &mut *stage;
        let mut index = 0;
        while index < creeps.len() {
            if creep::has_reached_the_exit(world, &creeps[index], *exit, scratch) {
                creeps.swap_remove(index).despawn(world);
                leaked += 1;
            } else {
                index += 1;
            }
        }
    }
    if leaked > 0 {
        stage.leaks += u64::from(leaked);
        stage.lives = stage.lives.saturating_sub(leaked);
    }

    // 5. Every bolt flies, against the creeps as they are *now*. A bolt that
    //    stops raises its burst, if its kind has one.
    let now = stage.elapsed;
    stage
        .bursts
        .retain(|burst| now - burst.raised_at < tower::BURST_S);
    let mut index = 0;
    while index < stage.bolts.len() {
        let outcome = {
            let Stage {
                world,
                creeps,
                bolts,
                ..
            } = &mut *stage;
            bolts[index].step(world, creeps, dt)
        };
        match outcome {
            BoltOutcome::Flying => index += 1,
            BoltOutcome::Spent => {
                let bolt = stage.bolts.swap_remove(index);
                stage.splash(&bolt, None);
            }
            BoltOutcome::Hit(body) => {
                let bolt = stage.bolts.swap_remove(index);
                stage.wound(body, bolt.damage());
                stage.splash(&bolt, Some(body));
            }
        }
    }

    // 6. Every ready shooting tower acquires and fires. A slow tower is not
    //    here: its whole effect was step 2.
    for index in 0..stage.towers.len() {
        let spec = stage.towers[index].spec();
        if !spec.fires() || !stage.towers[index].is_ready(now) {
            continue;
        }
        let muzzle = stage.towers[index].muzzle();
        let target = {
            let Stage {
                world,
                creeps,
                scratch,
                ..
            } = &mut *stage;
            tower::acquire(world, creeps, muzzle, spec.range_m, scratch)
        };
        if let Some(creep) = target {
            let bolt = Bolt::fire(muzzle, &stage.creeps[creep], spec);
            stage.bolts.push(bolt);
            stage.towers[index].fired(now);
            stage.shots += 1;
        }
    }

    // 7. Win at the end of the table, lose at zero lives.
    if stage.lives == 0 {
        stage.outcome = Outcome::Lost;
        stage.ended_at = stage.elapsed;
    } else if stage.waves.is_exhausted() && stage.creeps.is_empty() {
        stage.outcome = Outcome::Won;
        stage.ended_at = stage.elapsed;
    }

    stage.ticks += 1;
    stage.elapsed += dt;
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

pub(crate) mod play;

/// The stage, as the server hosts it.
///
/// `register` is empty for the same reason `apps/breach`'s is: the whole
/// simulation is the [`Stage`] behind the shared cell. The one system the
/// server's world holds is [`FieldReplica`], which [`server_world`] registers
/// rather than the module, because it only reads the stage.
///
/// **Two hosts, one tick.** Solo's `Server` hands it one client's frames as a
/// [`GameModule`]; a LAN host's [`Host`](crcbl::server::Host) hands it every
/// player's as a [`HostModule`], and [`run_team_tick`] reads them one player
/// at a time.
pub(crate) struct TowersModule {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for TowersModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TowersModule").finish_non_exhaustive()
    }
}

impl GameModule for TowersModule {
    fn name(&self) -> &str {
        "towers"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        let mut stage = lock(&self.shared);
        run_tick(&mut stage, Intent::from_inputs(inputs), dt);
    }
}

impl HostModule for TowersModule {
    fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>) {
        let dt = world.tick_dt();
        let intents: Vec<(Sender, Intent)> = inputs
            .iter()
            .map(|(peer, frames)| (Some(peer), Intent::from_inputs(frames)))
            .collect();
        let mut stage = lock(&self.shared);
        run_team_tick(&mut stage, &intents, dt);
    }
}

/// The stage as the server's world replicates it: every snapshot, the frame's
/// own view of the stage, written as [`crate::replica`]'s entities.
///
/// What a remote player draws — see that module's docs. Registered on every
/// server, solo's too, so the one wire format is exercised by every run and
/// solo's client reconstructs the same field a LAN client does.
struct FieldReplica {
    shared: Arc<Mutex<Stage>>,
    /// Whether a snapshot that left something out has been logged: once is
    /// enough to say the field outgrew the wire, and every snapshot after it
    /// would be the same line.
    reported_refusal: Cell<bool>,
}

impl SystemTrait for FieldReplica {
    fn name(&self) -> &str {
        crate::replica::SYSTEM
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        let render = render_state_of(&lock(&self.shared));
        let towers = render.towers.iter().flatten().count();
        1 + towers + render.creeps_alive + render.bolts_flying + render.bursts_live
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        let (render, stats) = {
            let stage = lock(&self.shared);
            (render_state_of(&stage), stats_of(&stage))
        };
        let encoded = crate::replica::encode(&render, &stats, out);
        if encoded.refused > 0 && !self.reported_refusal.replace(true) {
            crcbl::log::warn!(
                "replica: {} of the field's entities would not fit the wire and were left out \
                 of the snapshot; a remote player does not see them",
                encoded.refused
            );
        }
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A new stage on `map`, the world a server hosts it in, and the module that
/// ticks it.
fn server_world(map: &Arc<Map>) -> (Arc<Mutex<Stage>>, World, TowersModule) {
    let shared = Arc::new(Mutex::new(Stage::new(Arc::clone(map))));
    let mut world = World::new();
    world.register_system(Box::new(FieldReplica {
        shared: Arc::clone(&shared),
        reported_refusal: Cell::new(false),
    }));
    let module = TowersModule {
        shared: Arc::clone(&shared),
    };
    (shared, world, module)
}

/// A stage a server outside this file ticks, read only for its numbers: a
/// dedicated server's, which has no [`Game`] around it — see
/// `crate::lan::serve`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Field(Arc<Mutex<Stage>>);

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Field").finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Field {
    /// A new stage on `map`, the world a host serves it from and the module
    /// that ticks it, with the start-up line logged as every other mode logs
    /// it.
    pub(crate) fn open(map: &Map, tick_hz: u32) -> (Self, World, TowersModule) {
        let (shared, world, module) = server_world(&Arc::new(map.clone()));
        log_the_rules(
            tick_hz,
            crcbl::core::FrameClock::new(tick_hz).tick_dt(),
            map,
        );
        (Self(shared), world, module)
    }

    /// The stage's numbers, read as [`Game::stats`] reads them.
    pub(crate) fn stats(&self) -> Stats {
        stats_of(&lock(&self.0))
    }

    /// Takes the refusals not yet told, oldest first, each with the peer
    /// who sent the command — `None` only on a solo stage, which no host
    /// serves.
    pub(crate) fn take_refusals(&self) -> Vec<(Option<PeerId>, Refusal)> {
        std::mem::take(&mut lock(&self.0).refusals)
    }
}

/// The shared stage, with a poisoned lock treated as the stage it was left in.
///
/// A panic inside the tick is a bug this sample would rather report through its
/// own numbers than through a second panic in the frame that reads them.
fn lock(shared: &Arc<Mutex<Stage>>) -> MutexGuard<'_, Stage> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// What a frame reads
// ---------------------------------------------------------------------------

/// Everything the frame draws, snapshotted once per draw.
///
/// A plain `Copy` struct rather than a borrow of the stage: the frame runs on
/// the frame's thread and the stage is behind a mutex the tick holds, and a
/// frame that read through the lock would be holding it for the length of a
/// draw. The pools are fixed-size for the same reason — a heap allocation here
/// would be one per draw — which is why the per-plot pools are [`MAX_PLOTS`]
/// wide rather than as wide as the map the run is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderState {
    /// Every creep on the field, the first [`RenderState::creeps_alive`] of
    /// them live.
    pub creeps: [CreepView; MAX_CREEPS],
    pub creeps_alive: usize,
    /// One entry per plot: `None` for an empty plot, the tower for a built one.
    /// Every entry past the map's last plot is `None`.
    pub towers: [Option<TowerView>; MAX_PLOTS],
    /// Every bolt in the air, the first [`RenderState::bolts_flying`] of them
    /// live. Bolts past the pool are simulated and not drawn — see
    /// [`crate::map::Map::max_bolts`].
    pub bolts: [DVec3; MAX_PLOTS],
    pub bolts_flying: usize,
    /// Every splash burst still being drawn, the first
    /// [`RenderState::bursts_live`] of them live — see
    /// [`crate::map::Map::max_bursts`].
    pub bursts: [BurstView; MAX_PLOTS],
    pub bursts_live: usize,
    pub gold: u32,
    pub lives: u32,
    /// How many waves have been started, out of [`crate::wave::WAVES`].
    pub wave: usize,
    pub kills: u64,
    pub leaks: u64,
    pub outcome: Outcome,
    /// How long until the next wave starts, in seconds, or `None` while one is
    /// releasing or the table is spent.
    pub next_wave_in: Option<f64>,
}

impl Default for RenderState {
    /// An empty field.
    ///
    /// Written out rather than derived: `#[derive(Default)]` reaches for
    /// `<[T; N]>::Default`, which the standard library only implements up to
    /// thirty-two elements, and [`MAX_CREEPS`] is the whole wave table.
    fn default() -> Self {
        Self {
            creeps: [CreepView::default(); MAX_CREEPS],
            creeps_alive: 0,
            towers: [None; MAX_PLOTS],
            bolts: [DVec3::ZERO; MAX_PLOTS],
            bolts_flying: 0,
            bursts: [BurstView::default(); MAX_PLOTS],
            bursts_live: 0,
            gold: 0,
            lives: 0,
            wave: 0,
            kills: 0,
            leaks: 0,
            outcome: Outcome::default(),
            next_wave_in: None,
        }
    }
}

/// The stage's numbers, for the debug overlay and the `[HUD]` line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub ticks: u64,
    pub gold: u32,
    pub lives: u32,
    pub wave: usize,
    pub creeps: usize,
    pub towers: usize,
    /// How many plots the map has, which is how many towers it can hold.
    pub plots: usize,
    pub bolts: usize,
    pub kills: u64,
    pub leaks: u64,
    pub shots: u64,
    pub built: u64,
    /// How many of each [`tower::Kind`] are on the field, in
    /// [`tower::ALL`]'s order — see `Stage::built_by_kind`.
    pub built_by_kind: [u64; tower::KINDS],
    /// How many towers have been stepped up a tier.
    pub upgrades: u64,
    /// How many commands the server turned down — see `Stage::refused`.
    pub refused: u64,
    pub outcome: Outcome,
    pub runs: u64,
    pub next_wave_in: Option<f64>,
}

impl Stats {
    /// How many towers of `kind` have been built.
    #[must_use]
    pub const fn built_of(&self, kind: tower::Kind) -> u64 {
        self.built_by_kind[kind.index()]
    }
}

impl crcbl::ui::DebugModule for Stats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("towers");
        section.row("tick", format_args!("{}", self.ticks));
        section.row("gold", format_args!("{}", self.gold));
        section.row(
            "lives",
            format_args!("{}/{}", self.lives, crate::wave::STARTING_LIVES),
        );
        section.row(
            "wave",
            format_args!("{}/{}", self.wave, crate::wave::WAVES.len()),
        );
        match self.next_wave_in {
            Some(seconds) => section.row("next", format_args!("{seconds:.1} s")),
            None => section.row_str("next", "--"),
        }
        section.row("creeps", format_args!("{}", self.creeps));
        section.row("towers", format_args!("{}/{}", self.towers, self.plots));
        // One row per kind, because "three towers" says nothing about whether
        // the kind keys reached the server.
        for kind in tower::ALL {
            section.row(kind.label(), format_args!("{}", self.built_of(kind)));
        }
        section.row("upgrades", format_args!("{}", self.upgrades));
        section.row("bolts", format_args!("{}", self.bolts));
        section.row("kills", format_args!("{}", self.kills));
        section.row("leaks", format_args!("{}", self.leaks));
        section.row("shots", format_args!("{}", self.shots));
        section.row("built", format_args!("{}", self.built));
        // The one row that says the server is validating rather than obeying.
        section.row("refused", format_args!("{}", self.refused));
        section.row_str("outcome", self.outcome.label());
        section.row("runs", format_args!("{}", self.runs));
    }
}

// ---------------------------------------------------------------------------
// The facade
// ---------------------------------------------------------------------------

/// What can stop towers before it starts.
#[derive(Debug)]
pub enum GameError {
    /// The operating system would not seed the server's resume credential.
    Server(String),
    /// A LAN session could not start: a socket would not bind, or a connect
    /// would not begin. Native builds only — see [`crate::lan`].
    #[cfg(not(target_arch = "wasm32"))]
    Lan(crcbl::lan::LanError),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(message) => write!(f, "server creation failed: {message}"),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Lan(error) => write!(f, "LAN session failed: {error}"),
        }
    }
}

impl std::error::Error for GameError {}

/// Which side of which wire this game's client is on.
enum Link {
    /// Solo: the server and its client over an `InMemoryTransport`.
    Solo(Box<Loopback>),
    /// Hosting a LAN session: the server is a [`Host`](crcbl::server::Host)
    /// behind a UDP listener, and this player is one of its clients.
    #[cfg(not(target_arch = "wasm32"))]
    Host(Box<crate::lan::HostLink>),
    /// In someone else's LAN session: a client and nothing else, and the field
    /// is what its snapshots carry.
    #[cfg(not(target_arch = "wasm32"))]
    Remote(Box<crate::lan::RemoteLink>),
}

/// The stage, its server, its client, and the clock that drives all three —
/// or, joined to a remote host, the client alone.
pub struct Game {
    link: Link,
    /// The stage, wherever this process runs the server: solo, or hosting.
    /// `None` on a remote client, which has no stage — see [`crate::replica`].
    shared: Option<Arc<Mutex<Stage>>>,
    /// The stage's map, held here as well so the client can read the plots it
    /// lists without taking the tick's lock. The same allocation the stage
    /// holds, so the two cannot be different maps. On a remote client it is
    /// the one the host sent at join — see `crate::lan`.
    map: Arc<Map>,
    /// Exactly one tick period per [`Game::tick`], so the client's clock — and
    /// solo's server's — yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// What the player asked for, sent on the next tick.
    pending: Intent,
    /// Solo's refusals, taken off the stage every tick and not yet taken by
    /// [`Game::take_refusals`]. A LAN link holds its own.
    refusals: Vec<Refusal>,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ticks_run", &self.ticks_run)
            .finish_non_exhaustive()
    }
}

impl Game {
    /// Builds the server, its client and the stage between them, on `map`.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential, or if the loopback session did not
    /// come up.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(tick_hz: u32, map: &Map) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let map = Arc::new(map.clone());
        let (shared, world, module) = server_world(&map);

        // The world's one system is the field's replica. What the server
        // ticks is the module, and what the module owns is the stage.
        let mut session = Loopback::new(world, Box::new(module), tick_hz, COMPATIBILITY)
            .map_err(|error| GameError::Server(error.to_string()))?;
        let tick_period = session.tick_period();

        // **One tick spent on the handshake, before the first command.**
        // `Server::update` drains the transport inside `tick`, so the client's
        // hello is not read until a tick runs, and until the session is up the
        // client drops every input frame it is asked to send. Spending it here
        // is what makes the player's first key the first the simulation sees.
        session.client_mut().update(tick_period);
        session.server_mut().update(tick_period);
        session.client_mut().update(tick_period);
        if session.server().session_state() != crcbl::net::SessionState::Connected {
            return Err(GameError::Server(
                "the loopback session did not come up in its first tick".into(),
            ));
        }

        log_the_rules(tick_hz, tick_period, &map);
        Ok(Self {
            link: Link::Solo(Box::new(session)),
            shared: Some(shared),
            map,
            tick_period,
            sim_time: tick_period,
            ticks_run: 0,
            pending: Intent::default(),
            refusals: Vec::new(),
        })
    }

    /// Hosts a LAN session of `map` bound where `bind` says, with this player
    /// one of its clients — see [`crate::lan`].
    ///
    /// The server runs on the frame's wall time from here on, through
    /// [`Game::frame`], so a host with its pause menu open goes on serving the
    /// others; this player's commands still go out a tick at a time from
    /// [`Game::tick`].
    ///
    /// # Errors
    ///
    /// [`GameError::Lan`] if the listener would not bind, and
    /// [`GameError::Server`] if this player's own session did not come up in
    /// the first tick.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn host(tick_hz: u32, map: &Map, bind: crcbl::lan::LanBind) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let map = Arc::new(map.clone());
        let (shared, world, module) = server_world(&map);
        let served = (Field(Arc::clone(&shared)), world, module);
        let (link, tick_period) =
            crate::lan::HostLink::open(crate::lan::SESSION, bind, served, tick_hz, &map)?;
        log_the_rules(tick_hz, tick_period, &map);
        Ok(Self {
            link: Link::Host(Box::new(link)),
            shared: Some(shared),
            map,
            tick_period,
            // `HostLink::open` spent the first tick on this player's handshake,
            // as `Game::new` does on solo's.
            sim_time: tick_period,
            ticks_run: 0,
            pending: Intent::default(),
            refusals: Vec::new(),
        })
    }

    /// Plays in the LAN session `client` is in, on `map` — the one its host
    /// sent at join. Built by `crate::lan::Joining` once that map has
    /// arrived, and by nothing else, so a joiner's game is never on any other
    /// map. `now` is the client's clock so far, which this game's carries on
    /// from, and `ignored_events` how many of the host's events the join
    /// could not read, which its count carries on from.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn joined(
        tick_hz: u32,
        map: Map,
        client: crcbl::lan::LanClient,
        now: Duration,
        ignored_events: u64,
    ) -> Self {
        assert!(tick_hz > 0, "tick rate must be positive");
        Self {
            link: Link::Remote(Box::new(crate::lan::RemoteLink::new(
                client,
                ignored_events,
            ))),
            shared: None,
            map: Arc::new(map),
            // The client clock's own step, so one period is exactly one tick
            // of it, as `Loopback::tick_period` is solo's.
            tick_period: crcbl::core::FrameClock::new(tick_hz).tick_dt(),
            sim_time: now,
            ticks_run: 0,
            pending: Intent::default(),
            refusals: Vec::new(),
        }
    }

    /// Records what the player asked for, to be sent on the next tick.
    pub fn set_controls(&mut self, controls: Controls) {
        self.pending = controls.into();
    }

    /// Sends this tick's command and advances this player's client by exactly
    /// one tick — and, solo, the server and the stage with it.
    ///
    /// A LAN host's server is not advanced here but by [`Game::frame`], on the
    /// frame's wall time; see `Game::host`.
    pub fn tick(&mut self) {
        self.sim_time += self.tick_period;
        // The bytes are the whole command path: the client seals them, the
        // transport carries them and the module decodes them, exactly as a
        // remote client's would be.
        let input = core::mem::take(&mut self.pending).to_wire();
        match &mut self.link {
            Link::Solo(session) => {
                let (server, client) = session.both_mut();
                client.set_input(input);

                // Send, simulate, then receive — and the send has to come
                // first. `Client::update` is the only thing that puts input on
                // the wire and the server drains the wire at the top of its
                // tick, so a client updated only after the server posts this
                // tick's commands to the next one.
                client.update(self.sim_time);
                let server_ticks = server.update(self.sim_time);
                debug_assert_eq!(
                    server_ticks, 1,
                    "one tick period in must be exactly one server tick out",
                );
                // Consumes no tick — the clock has not moved between the two —
                // and is there to take the snapshot this tick produced.
                client.update(self.sim_time);
                // Solo's one player sent every command, so every refusal is
                // theirs to be told.
                if let Some(shared) = &self.shared {
                    let refusals = std::mem::take(&mut lock(shared).refusals);
                    self.refusals
                        .extend(refusals.into_iter().map(|(_, refusal)| refusal));
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Link::Host(host) => host.tick(input, self.sim_time),
            #[cfg(not(target_arch = "wasm32"))]
            Link::Remote(remote) => remote.tick(input, self.sim_time),
        }
        self.ticks_run += 1;
    }

    /// Serves a LAN session for one frame covering `render_dt`, paused or
    /// not: a host runs its server to the frame's wall time, and either side
    /// reads what arrived. Solo has nothing to serve between ticks.
    ///
    /// Every frame rather than every tick for the reason `apps/sandbox`'s LAN
    /// session is: a side that stopped reading while its pause menu was open
    /// would time every link out.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn frame(&mut self, render_dt: Duration) {
        match &mut self.link {
            Link::Solo(_) => {}
            #[cfg(not(target_arch = "wasm32"))]
            Link::Host(host) => host.frame(render_dt, self.sim_time),
            #[cfg(not(target_arch = "wasm32"))]
            Link::Remote(remote) => remote.frame(self.sim_time),
        }
    }

    /// Takes the refusals of this player's commands since the last call,
    /// oldest first: the server's own, solo or hosting, or what the host
    /// told a joiner.
    pub fn take_refusals(&mut self) -> Vec<Refusal> {
        match &mut self.link {
            Link::Solo(_) => std::mem::take(&mut self.refusals),
            #[cfg(not(target_arch = "wasm32"))]
            Link::Host(host) => host.take_refusals(),
            #[cfg(not(target_arch = "wasm32"))]
            Link::Remote(remote) => remote.take_refusals(),
        }
    }

    /// Events from the host this joiner could not read, counted and
    /// ignored — see `crate::lan::event`. Zero solo and hosting.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn ignored_events(&self) -> u64 {
        match &self.link {
            Link::Remote(remote) => remote.ignored_events(),
            Link::Solo(_) | Link::Host(_) => 0,
        }
    }

    /// The F3 panel's "lan" section, during a LAN session.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn lan_section(&self) -> Option<&dyn crcbl::ui::DebugModule> {
        match &self.link {
            Link::Solo(_) => None,
            Link::Host(host) => Some(host.lan()),
            Link::Remote(remote) => Some(remote.lan()),
        }
    }

    /// The LAN host, while this game hosts one.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn lan_host(&self) -> Option<&crcbl::lan::LanHost> {
        match &self.link {
            Link::Host(host) => Some(host.lan()),
            Link::Solo(_) | Link::Remote(_) => None,
        }
    }

    /// The LAN client, while this game plays in someone else's session.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn lan_client(&self) -> Option<&crcbl::lan::LanClient> {
        match &self.link {
            Link::Remote(remote) => Some(remote.lan()),
            Link::Solo(_) | Link::Host(_) => None,
        }
    }

    /// How the LAN session this game joined ended, in words, once it has —
    /// the host left or shut down, removed this player, or the link died.
    /// `None` solo, hosting, and while the session runs.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn session_end(&self) -> Option<String> {
        match &self.link {
            Link::Remote(remote) => remote.ended(),
            Link::Solo(_) | Link::Host(_) => None,
        }
    }

    /// The field the run is played on.
    #[must_use]
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// How many times [`Game::tick`] has been called.
    #[must_use]
    pub const fn ticks_run(&self) -> u64 {
        self.ticks_run
    }

    /// What the frame should draw: read off the stage where this process runs
    /// the server, and off the host's snapshots where it does not.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        match &self.shared {
            Some(shared) => render_state_of(&lock(shared)),
            None => self.replicated().render,
        }
    }

    /// The stage's numbers for the debug panel and the `[HUD]` line, read as
    /// [`Game::render_state`] is.
    #[must_use]
    pub fn stats(&self) -> Stats {
        match &self.shared {
            Some(shared) => stats_of(&lock(shared)),
            None => self.replicated().stats,
        }
    }

    /// What this game's client has reconstructed of the server's field.
    ///
    /// Solo's client and a host's own player reconstruct it too, from the
    /// same snapshots a remote player's does — which is what makes every mode
    /// the same game over the wire, and what their tests compare.
    #[must_use]
    pub fn replicated(&self) -> crate::replica::Decoded {
        match &self.link {
            Link::Solo(session) => {
                crate::replica::decode(session.client().replicated(crate::replica::SYSTEM))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Link::Host(host) => host.replicated(),
            #[cfg(not(target_arch = "wasm32"))]
            Link::Remote(remote) => remote.replicated(),
        }
    }
}

/// The start-up line: the rate, the table and the purse.
fn log_the_rules(tick_hz: u32, tick_period: Duration, map: &Map) {
    crcbl::log::info!(
        "sim: {tick_hz} Hz, {:.3} ms per tick, {} waves of up to {MAX_CREEPS} creeps over a \
         {:.1} m path, {} tower kinds, {} lives and {} gold",
        tick_period.as_secs_f64() * 1e3,
        wave::WAVES.len(),
        map.path().length(),
        tower::KINDS,
        STARTING_LIVES,
        STARTING_GOLD,
    );
}

/// What the frame should draw of `stage`.
fn render_state_of(stage: &Stage) -> RenderState {
    let mut creeps = [CreepView::default(); MAX_CREEPS];
    for (slot, creep) in creeps.iter_mut().zip(stage.creeps.iter()) {
        *slot = creep.view();
    }
    let mut bolts = [DVec3::ZERO; MAX_PLOTS];
    for (slot, bolt) in bolts.iter_mut().zip(stage.bolts.iter()) {
        *slot = bolt.at();
    }
    let mut bursts = [BurstView::default(); MAX_PLOTS];
    for (slot, burst) in bursts.iter_mut().zip(stage.bursts.iter()) {
        *slot = BurstView {
            centre: burst.at,
            radius_m: burst.radius_m,
        };
    }
    let now = stage.elapsed;
    RenderState {
        creeps,
        creeps_alive: stage.creeps.len().min(MAX_CREEPS),
        towers: core::array::from_fn(|plot| {
            stage
                .towers
                .iter()
                .find(|tower| tower.plot() == plot)
                .map(|tower| TowerView {
                    kind: tower.kind(),
                    tier: tower.tier(),
                    working: tower.is_firing(now),
                })
        }),
        bolts,
        bolts_flying: stage.bolts.len().min(stage.map.max_bolts()),
        bursts,
        bursts_live: stage.bursts.len().min(stage.map.max_bursts()),
        gold: stage.gold,
        lives: stage.lives,
        wave: stage.waves.started(),
        kills: stage.kills,
        leaks: stage.leaks,
        outcome: stage.outcome,
        next_wave_in: stage.waves.next_in(now),
    }
}

/// `stage`'s numbers for the debug panel and the `[HUD]` line.
fn stats_of(stage: &Stage) -> Stats {
    Stats {
        ticks: stage.ticks,
        gold: stage.gold,
        lives: stage.lives,
        wave: stage.waves.started(),
        creeps: stage.creeps.len(),
        towers: stage.towers.len(),
        plots: stage.map.plots().len(),
        bolts: stage.bolts.len(),
        kills: stage.kills,
        leaks: stage.leaks,
        shots: stage.shots,
        built: stage.built,
        built_by_kind: stage.built_by_kind,
        upgrades: stage.upgrades,
        refused: stage.refused,
        outcome: stage.outcome,
        runs: stage.runs,
        next_wave_in: stage.waves.next_in(stage.elapsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{self, CREEP_RADIUS};
    use crate::tower::Kind::{Bolt as BoltKind, Slow, Splash};
    use crate::wave::WAVES;

    /// An empty field on the committed map, with the first build phase running.
    fn new_stage() -> Stage {
        Stage::new(Arc::new(Map::built_in()))
    }

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// Long enough for the whole table to be released and walked out, in
    /// seconds, with a wide margin. Derived from the table rather than guessed,
    /// so a row added to [`WAVES`] does not quietly truncate a run.
    fn long_enough_for_the_table() -> f64 {
        let releases: f64 = WAVES
            .iter()
            .map(|wave| {
                (0..wave.creeps())
                    .filter_map(|index| wave.gap_after(index))
                    .sum::<f64>()
            })
            .sum();
        let walk = Map::built_in().path().length() / creep::Kind::Tanky.spec().speed;
        releases + (WAVES.len() + 2) as f64 * wave::GAP_S + 3.0 * walk
    }

    /// Runs the stage for `seconds`, asking for nothing.
    fn idle(stage: &mut Stage, seconds: f64) {
        for _ in 0..(seconds / DT).round() as u64 {
            run_tick(stage, Intent::default(), DT);
        }
    }

    /// One command, on its own tick.
    fn command(stage: &mut Stage, intent: Intent) {
        run_tick(stage, intent, DT);
    }

    /// Runs the stage until the run ends, for at most `seconds`.
    ///
    /// A run rather than a fixed number of ticks, because a finished run
    /// **starts itself again** — see [`RESTART_S`] — so a test that idled for a
    /// round number of seconds would be asking about whichever run happened to
    /// be in progress when it stopped counting.
    fn until_over(stage: &mut Stage, seconds: f64) -> Outcome {
        for _ in 0..(seconds / DT).round() as u64 {
            run_tick(stage, Intent::default(), DT);
            if stage.outcome.is_over() {
                return stage.outcome;
            }
        }
        stage.outcome
    }

    /// The command that builds a `kind` tower on `plot`.
    const fn build(plot: u8, kind: tower::Kind) -> Intent {
        Intent {
            place: Some(plot),
            kind,
            upgrade: None,
            start_wave: false,
            restart: false,
        }
    }

    /// The command that steps the tower on `plot` up a tier.
    const fn step_up(plot: u8) -> Intent {
        Intent {
            place: None,
            kind: tower::Kind::Bolt,
            upgrade: Some(plot),
            start_wave: false,
            restart: false,
        }
    }

    /// The command that sends the next wave now.
    const fn send_wave() -> Intent {
        Intent {
            place: None,
            kind: tower::Kind::Bolt,
            upgrade: None,
            start_wave: true,
            restart: false,
        }
    }

    /// Which plot is labelled `label`.
    fn plot(label: &str) -> u8 {
        let at = Map::built_in()
            .plots()
            .iter()
            .position(|plot| plot.label == label)
            .unwrap_or_else(|| panic!("the map has no {label} plot"));
        #[allow(clippy::cast_possible_truncation)]
        let at = at as u8;
        at
    }

    /// A `kind` tower on the plot labelled `label` of `stage`'s map, not yet on
    /// the field.
    fn tower_on(stage: &Stage, label: &str, kind: tower::Kind) -> Tower {
        let at = usize::from(plot(label));
        Tower::new(at, stage.map.plots()[at].at(), kind)
    }

    /// Puts a creep of `kind` on the field, walked `along` metres in.
    ///
    /// Walked rather than placed, because `Creep::advance` is what writes the
    /// sphere every query here reads.
    fn creep_at(stage: &mut Stage, kind: creep::Kind, along: f64) -> ColliderId {
        let mut creep = Creep::spawn(&mut stage.world, stage.map.path(), kind);
        let ticks = (along / (kind.spec().speed * DT)).round() as u64;
        for _ in 0..ticks {
            creep.advance(&mut stage.world, stage.map.path(), DT);
        }
        let body = creep.body();
        stage.creeps.push(creep);
        body
    }

    /// What a creep with this body has left, or `None` once it is dead.
    fn health_of(stage: &Stage, body: ColliderId) -> Option<u32> {
        stage
            .creeps
            .iter()
            .find(|creep| creep.body() == body)
            .map(Creep::health)
    }

    /// The creep with this body, or `None` once it has left the field.
    ///
    /// By body rather than by index, because the wave table goes on releasing
    /// creeps underneath a test and `swap_remove` moves whoever is left.
    fn creep_with(stage: &Stage, body: ColliderId) -> Option<&Creep> {
        stage.creeps.iter().find(|creep| creep.body() == body)
    }

    /// What a player following `plan` asks for this tick.
    ///
    /// Builds the plots in order and then steps each tower up, and **asks for
    /// nothing it cannot pay for** — so a run played by this policy leaves
    /// `refused` at zero, which the tests below assert. The purse is read off the
    /// stage rather than tracked here: the server owns it.
    fn next_purchase(stage: &Stage, plan: &[tower::Kind]) -> Intent {
        for (at, kind) in plan.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let at_byte = at as u8;
            if !stage.is_taken(at) {
                return if stage.gold >= kind.spec(Tier::Base).cost {
                    build(at_byte, *kind)
                } else {
                    Intent::default()
                };
            }
        }
        for tower in &stage.towers {
            if let Some(cost) = tower.upgrade_cost()
                && stage.gold >= cost
            {
                #[allow(clippy::cast_possible_truncation)]
                let at_byte = tower.plot() as u8;
                return step_up(at_byte);
            }
        }
        Intent::default()
    }

    /// What happened over one run of the table played to `plan`.
    struct Played {
        stage: Stage,
        /// The most creeps, bolts and bursts the run ever had at once — what
        /// `crate::map`'s three pools have to cover.
        peak_creeps: usize,
        peak_bolts: usize,
        peak_bursts: usize,
    }

    /// Plays the whole table with `plan` on the plots, buying as the purse
    /// allows, and stops the moment the run is decided.
    fn play(plan: &[tower::Kind]) -> Played {
        let mut stage = new_stage();
        let (mut peak_creeps, mut peak_bolts, mut peak_bursts) = (0, 0, 0);
        for _ in 0..(long_enough_for_the_table() / DT).round() as u64 {
            if stage.outcome.is_over() {
                break;
            }
            let intent = next_purchase(&stage, plan);
            run_tick(&mut stage, intent, DT);
            peak_creeps = peak_creeps.max(stage.creeps.len());
            peak_bolts = peak_bolts.max(stage.bolts.len());
            peak_bursts = peak_bursts.max(stage.bursts.len());
        }
        Played {
            stage,
            peak_creeps,
            peak_bolts,
            peak_bursts,
        }
    }

    /// **A command survives the wire, and a frame this build did not write is
    /// refused rather than read as an empty command.**
    ///
    /// The refusal matters because an unreadable frame taken as `default()`
    /// would be indistinguishable from a player asking for nothing, and the
    /// merge below would then treat it as one.
    #[test]
    fn a_command_survives_the_wire_and_nonsense_does_not() {
        for intent in [
            Intent::default(),
            build(3, Splash),
            build(0, Slow),
            step_up(4),
            send_wave(),
            Intent {
                place: Some(1),
                kind: Splash,
                upgrade: Some(2),
                start_wave: true,
                restart: true,
            },
        ] {
            let wire = intent.to_wire();
            assert_eq!(wire.len(), INTENT_BYTES);
            assert_eq!(Intent::from_wire(&wire), Some(intent));
        }

        assert_eq!(Intent::from_wire(&[]), None, "an empty frame was read");
        assert_eq!(
            Intent::from_wire(&[0, PLOT_NONE]),
            None,
            "a slice 1 frame was read as a slice 3 one",
        );
        assert_eq!(
            Intent::from_wire(&[0, 0, 0, 0, 0]),
            None,
            "a long frame was read",
        );
        assert_eq!(
            Intent::from_wire(&[0b1000_0000, PLOT_NONE, 0, PLOT_NONE]),
            None,
            "a flag this build never sets was read",
        );
        // **A kind byte no row has is a format problem rather than a rules one**,
        // because there is no `tower::Kind` to carry — see `Intent::from_wire`.
        #[allow(clippy::cast_possible_truncation)]
        let past_the_table = tower::KINDS as u8;
        assert_eq!(
            Intent::from_wire(&[0, 0, past_the_table, PLOT_NONE]),
            None,
            "a kind byte past the table was read",
        );
    }

    /// **The sentinel is not a plot**, which is what lets one byte carry both
    /// "build here" and "build nothing".
    #[test]
    fn the_no_plot_sentinel_is_not_a_plot() {
        assert!(
            (PLOT_NONE as usize) >= MAX_PLOTS,
            "the sentinel names plot {PLOT_NONE}",
        );
    }

    /// **A tower is built only when the plot is free, the plot is a plot and
    /// the gold is there** — and every refusal is counted.
    ///
    /// Four refusals and one success, because the four are what the server is
    /// for: a build that trusted its client passes the success and leaves
    /// `refused` at zero. The price is the **kind's**, which is the half slice 3
    /// added: a splash tower the purse cannot reach is refused where a bolt
    /// tower would have gone up.
    #[test]
    fn a_tower_is_built_only_when_the_rules_allow_it() {
        let mut stage = new_stage();
        command(&mut stage, build(0, BoltKind));
        assert_eq!(stage.towers.len(), 1, "the first build was refused");
        assert_eq!(stage.gold, STARTING_GOLD - BoltKind.spec(Tier::Base).cost);
        assert_eq!(stage.refused, 0);
        assert_eq!(stage.built_by_kind[BoltKind.index()], 1);
        assert_eq!(stage.built_by_kind[Splash.index()], 0);

        // The same plot again.
        command(&mut stage, build(0, BoltKind));
        assert_eq!(stage.towers.len(), 1, "it built twice on one plot");
        assert_eq!(stage.refused, 1);

        // A plot that is not a plot — the byte the wire carried intact.
        command(&mut stage, build(200, BoltKind));
        assert_eq!(stage.towers.len(), 1, "it built on plot 200");
        assert_eq!(stage.refused, 2);

        // A kind the purse cannot reach on a plot that is free, which the purse
        // *could* have reached had it been a bolt tower.
        let purse = stage.gold;
        assert!(
            purse >= BoltKind.spec(Tier::Base).cost && purse < 2 * Splash.spec(Tier::Base).cost,
            "the purse at {purse} does not separate the two kinds' prices",
        );
        command(&mut stage, build(1, Splash));
        command(&mut stage, build(2, Splash));
        assert_eq!(
            stage.refused, 3,
            "a splash tower nobody could afford went up"
        );
        assert_eq!(
            stage.built_by_kind[Splash.index()],
            1,
            "the first splash tower was refused too",
        );

        // Spend down to nothing, then ask again.
        let mut at = 2;
        while stage.gold >= BoltKind.spec(Tier::Base).cost && at < stage.map.plots().len() {
            command(&mut stage, build(at as u8, BoltKind));
            at += 1;
        }
        assert!(
            stage.gold < BoltKind.spec(Tier::Base).cost,
            "the purse is not empty",
        );
        let (built, refused) = (stage.towers.len(), stage.refused);
        let last = (stage.map.plots().len() - 1) as u8;
        command(&mut stage, build(last, BoltKind));
        assert_eq!(stage.towers.len(), built, "it built with no gold");
        assert_eq!(stage.refused, refused + 1);
    }

    /// **A tower is stepped up only when there is a tower, a tier left and the
    /// gold for it** — and every refusal is counted.
    ///
    /// The three refusals are the whole of the `UpgradeTower` command's server
    /// side, and the middle one is the interesting case: a command the server
    /// **accepted twice** would give a player two tiers for the price of one,
    /// and nothing on the client could tell.
    #[test]
    fn an_upgrade_is_built_only_when_the_rules_allow_it() {
        let mut stage = new_stage();

        // An empty plot — which is also every plot that is not a plot at all.
        command(&mut stage, step_up(0));
        assert_eq!(stage.upgrades, 0, "an empty plot was upgraded");
        assert_eq!(stage.refused, 1);
        command(&mut stage, step_up(200));
        assert_eq!(stage.refused, 2, "plot 200 was upgraded");

        command(&mut stage, build(0, BoltKind));
        let purse = stage.gold;
        let cost = BoltKind.spec(Tier::Upgraded).cost;
        assert!(purse >= cost, "the opening purse cannot reach one upgrade");

        command(&mut stage, step_up(0));
        assert_eq!(stage.upgrades, 1, "the upgrade was refused");
        assert_eq!(
            stage.gold,
            purse - cost,
            "the purse did not pay the upgrade"
        );
        assert_eq!(stage.towers[0].tier(), Tier::Upgraded);
        assert_eq!(stage.refused, 2, "the upgrade was counted as a refusal");

        // …and a second one on the same plot, which is the refusal that matters.
        //
        // **The purse is filled first, and that is load-bearing.** After one
        // upgrade the opening purse cannot reach a second, so a server that
        // consulted the price and nothing else would refuse this for the wrong
        // reason and the tier rule would go untested. With gold to spare, the
        // only thing left that can say no is "there is no tier to buy".
        stage.gold = 10 * BoltKind.spec(Tier::Upgraded).cost;
        let purse = stage.gold;
        command(&mut stage, step_up(0));
        assert_eq!(stage.upgrades, 1, "it was upgraded twice");
        assert_eq!(stage.towers[0].tier(), Tier::Upgraded);
        assert_eq!(stage.gold, purse, "a refused upgrade still took the gold");
        assert_eq!(stage.refused, 3);

        // And one nobody can pay for: a splash tower is dear enough that the
        // purse cannot reach its upgrade after building it.
        let mut stage = new_stage();
        command(&mut stage, build(1, Splash));
        assert_eq!(stage.towers.len(), 1, "the splash tower was refused");
        assert!(
            stage.gold < Splash.spec(Tier::Upgraded).cost,
            "the purse at {} can reach the {} gold upgrade, so this proves nothing",
            stage.gold,
            Splash.spec(Tier::Upgraded).cost,
        );
        command(&mut stage, step_up(1));
        assert_eq!(stage.upgrades, 0, "it upgraded with no gold");
        assert_eq!(stage.refused, 1);
    }

    /// **A splash burst wounds more than the creep the bolt struck** — and it
    /// does not wound the whole field.
    ///
    /// Three creeps and two controls, which is what makes the claim mean
    /// anything:
    ///
    /// * the **neighbour** is further from the target than a direct hit could
    ///   possibly reach — asserted from [`CREEP_RADIUS`] and
    ///   [`crate::map::BOLT_RADIUS`] before the tick — so a build whose splash
    ///   tower only wounded what it hit leaves it untouched;
    /// * the **bystander** is outside the burst, so a build that wounded every
    ///   creep on the field, or ran the overlap at the wrong radius, wounds it;
    /// * and the target's own health says the burst did **not** wound it twice,
    ///   which is what a burst that forgot its direct target would do.
    #[test]
    fn a_splash_burst_wounds_more_than_the_creep_the_bolt_struck() {
        let spec = Splash.spec(Tier::Base);
        let mut stage = new_stage();

        // Spaced along the opening leg, far from a corner. The neighbour is
        // inside the burst and out of reach of the impact; the bystander is
        // outside the burst altogether.
        let gap = 1.5;
        let outside = spec.burst_m + CREEP_RADIUS + 1.0;
        let kind = creep::Kind::Fast;
        let target = creep_at(&mut stage, kind, 10.0);
        let neighbour = creep_at(&mut stage, kind, 10.0 + gap);
        let bystander = creep_at(&mut stage, kind, 10.0 + outside);

        // The controls, taken off the geometry rather than assumed.
        let reach_of_a_direct_hit = CREEP_RADIUS + (CREEP_RADIUS + map::BOLT_RADIUS);
        assert!(
            gap > reach_of_a_direct_hit,
            "the neighbour is {gap} m out, inside the {reach_of_a_direct_hit:.2} m a bolt \
             stopping anywhere on the target could also touch",
        );
        assert!(
            gap + CREEP_RADIUS < spec.burst_m,
            "the neighbour is not inside the {} m burst",
            spec.burst_m,
        );
        assert!(
            outside - CREEP_RADIUS > spec.burst_m,
            "the bystander is inside the burst",
        );

        // One splash bolt, fired by hand at the target: no tower on the field,
        // so nothing else can be what wounded anybody.
        let whole = kind.spec().health;
        let tower = tower_on(&stage, "entry", Splash);
        let aimed = stage
            .creeps
            .iter()
            .find(|creep| creep.body() == target)
            .expect("the target is on the field");
        let bolt = Bolt::fire(tower.muzzle(), aimed, spec);
        stage.bolts.push(bolt);
        // The health window that makes every reading below legible: one helping
        // of this damage leaves a creep alive with a number to compare, and two
        // kill it outright — so "wounded twice" shows up as an empty field slot
        // rather than as a smaller number nobody would question.
        assert!(
            whole > spec.damage && whole <= 2 * spec.damage,
            "a {} creep's {whole} health cannot tell one burst from two at {} damage",
            kind.label(),
            spec.damage,
        );

        // Long enough for the bolt to land and no longer.
        for _ in 0..30 {
            if stage.bolts.is_empty() {
                break;
            }
            run_tick(&mut stage, Intent::default(), DT);
        }
        assert!(stage.bolts.is_empty(), "the bolt never landed");

        assert_eq!(
            health_of(&stage, target),
            Some(whole - spec.damage),
            "the creep the bolt struck was wounded {} times",
            match health_of(&stage, target) {
                Some(left) => format!("to {left} rather than once"),
                None => "to death, so twice".to_string(),
            },
        );
        assert_eq!(
            health_of(&stage, neighbour),
            Some(whole - spec.damage),
            "the burst did not wound the creep beside its impact point",
        );
        assert_eq!(
            health_of(&stage, bystander),
            Some(whole),
            "the burst wounded a creep outside it",
        );
        assert_eq!(
            stage.kills, 0,
            "something died, so the arithmetic is not this"
        );
    }

    /// **A splash impact leaves a burst on screen, and it is gone a moment
    /// later.**
    ///
    /// The second half is the control: a burst pool that never retired would
    /// fill with every impact of the run and draw the whole history of the
    /// field — and `crate::map::Map::max_bursts`' one-slot-per-plot argument would be
    /// wrong with it.
    #[test]
    fn a_burst_is_drawn_and_then_retired() {
        let mut stage = new_stage();
        let target = creep_at(&mut stage, creep::Kind::Tanky, 10.0);
        let spec = Splash.spec(Tier::Base);
        let tower = tower_on(&stage, "entry", Splash);
        let aimed = stage
            .creeps
            .iter()
            .find(|creep| creep.body() == target)
            .expect("the target is on the field");
        stage.bolts.push(Bolt::fire(tower.muzzle(), aimed, spec));

        let mut seen = 0;
        for _ in 0..30 {
            run_tick(&mut stage, Intent::default(), DT);
            seen = seen.max(stage.bursts.len());
            if stage.bolts.is_empty() {
                break;
            }
        }
        assert_eq!(seen, 1, "the impact raised {seen} bursts rather than one");
        assert!(
            stage.bursts.len() <= stage.map.max_bursts(),
            "{} bursts are being drawn into a pool of {}",
            stage.bursts.len(),
            stage.map.max_bursts(),
        );

        idle(&mut stage, tower::BURST_S + 4.0 * DT);
        assert!(stage.bursts.is_empty(), "the burst is still being drawn");
    }

    /// **A slow tower holds the creeps it covers, and lets them go the moment
    /// they walk out of its reach.**
    ///
    /// The second half is the claim, and the arithmetic is what makes it one: the
    /// creep ends the run **ahead** of where a hold that never ended would have
    /// left it and **behind** where no hold at all would have. Both ends are
    /// worked out from the tick the hold actually began rather than assumed, so
    /// a tower whose reach moves does not have to move a number in this test.
    #[test]
    fn a_slow_tower_holds_the_creeps_it_covers_and_lets_them_go() {
        let seconds = 5.0;
        let mut stage = new_stage();
        let tower = tower_on(&stage, "entry", Slow);
        let factor = tower.spec().slow_factor;
        stage.towers.push(tower);
        let kind = creep::Kind::Fast;
        // One creep of this test's own, **followed by its body** rather than by
        // its index: the wave table goes on releasing creeps of its own and
        // `swap_remove` moves whoever is left when one of them leaks.
        let body = creep_at(&mut stage, kind, 2.0);
        let start = creep_with(&stage, body)
            .expect("it was just put there")
            .along();

        let mut entered: Option<(f64, f64)> = None;
        let mut held_ticks = 0_u64;
        let mut free_after_the_hold = 0_u64;
        let mut last_step = 0.0;
        for tick in 0..(seconds / DT).round() as u64 {
            let before = creep_with(&stage, body)
                .expect("the creep left the field")
                .along();
            run_tick(&mut stage, Intent::default(), DT);
            let creep = creep_with(&stage, body).expect("the creep left the field");
            last_step = creep.along() - before;
            if creep.is_slowed() {
                held_ticks += 1;
                if entered.is_none() {
                    entered = Some((tick as f64 * DT, before));
                }
                assert!(
                    (last_step - kind.spec().speed * factor * DT).abs() < 1e-9,
                    "a held creep walked {last_step} m in a tick rather than {}",
                    kind.spec().speed * factor * DT,
                );
            } else if held_ticks > 0 {
                free_after_the_hold += 1;
            }
        }
        let (entered_at, entered_along) = entered.expect("the creep was never held at all");
        let creep = creep_with(&stage, body).expect("the creep left the field");

        assert!(held_ticks > 0, "nothing was ever held");
        assert!(
            free_after_the_hold > 0,
            "the creep never got out of the tower's reach, so the release is untested",
        );
        assert!(
            !creep.is_slowed(),
            "the creep is still held well past the tower's reach",
        );
        assert!(
            (last_step - kind.spec().speed * DT).abs() < 1e-9,
            "the last tick walked {last_step} m rather than a free {}",
            kind.spec().speed * DT,
        );

        // The two ends, derived: a hold that never let go, and no hold at all.
        let stuck = entered_along + kind.spec().speed * factor * (seconds - entered_at);
        let free = start + kind.spec().speed * seconds;
        let walked = creep.along();
        assert!(
            walked > stuck + 1.0,
            "the creep reached {walked:.2} m against the {stuck:.2} m a hold that never ended \
             would have left it at",
        );
        assert!(
            walked < free - 1.0,
            "the creep reached {walked:.2} m against the {free:.2} m an unheld one would have, \
             so nothing held it",
        );
        assert_eq!(
            health_of(&stage, body),
            Some(kind.spec().health),
            "a slow tower wounded something",
        );
        assert_eq!(stage.shots, 0, "a slow tower fired");
    }

    /// **A creep that reaches the exit costs a life**, and the field is left
    /// without it.
    #[test]
    fn a_creep_that_reaches_the_exit_costs_a_life() {
        let mut stage = new_stage();
        command(&mut stage, send_wave());
        assert_eq!(stage.refused, 0, "the first wave refused to start");

        // Long enough for the whole first wave to walk the path and no longer:
        // the second wave's own creeps must not be what this counts.
        let first = WAVES[0];
        let slowest = (0..first.creeps())
            .filter_map(|index| first.kind_at(index))
            .map(|kind| kind.spec().speed)
            .fold(f64::INFINITY, f64::min);
        let span: f64 = (0..first.creeps())
            .filter_map(|index| first.gap_after(index))
            .sum();
        let walk = stage.map.path().length() / slowest;
        idle(&mut stage, walk + span + 1.0);
        assert_eq!(
            stage.leaks,
            u64::from(first.creeps()),
            "an empty field let {} of {} through",
            stage.leaks,
            first.creeps(),
        );
        assert_eq!(stage.lives, STARTING_LIVES - first.creeps());
        assert_eq!(stage.kills, 0, "an empty field killed something");
    }

    /// **A field with no towers on it loses the run**, which is what makes
    /// building the game. The table releases more creeps than the team has
    /// lives — `crate::wave` asserts that inequality — and this is the outcome
    /// it produces.
    #[test]
    fn a_field_with_no_towers_on_it_loses_the_run() {
        let mut stage = new_stage();
        assert_eq!(
            until_over(&mut stage, long_enough_for_the_table()),
            Outcome::Lost,
            "an empty field survived: {} leaks and {} lives left",
            stage.leaks,
            stage.lives,
        );
        assert_eq!(stage.lives, 0);
        assert_eq!(stage.kills, 0, "an empty field killed something");
        assert_eq!(stage.leaks, u64::from(STARTING_LIVES));
        assert!(
            stage.waves.started() < wave::WAVES.len() || !stage.creeps.is_empty(),
            "it lost only once the table was spent, so the lives outlasted the waves",
        );
    }

    /// **The last row needs a splash tower *and* a slow tower**, and four plans
    /// played out are what says so.
    ///
    /// `docs/plan/sample/07-towers.md`'s scope line asks for three tower kinds,
    /// and three kinds are three kinds only if one of them is not enough. So this
    /// plays the whole table four ways on the same five plots, each upgraded as
    /// the purse allows:
    ///
    /// | plan | outcome |
    /// | --- | --- |
    /// | five bolt towers | the last row overruns it |
    /// | a splash tower and four bolts | the same |
    /// | a slow tower and four bolts | the same |
    /// | a splash tower **and** a slow tower | held, by
    ///   `a_splash_and_a_slow_tower_hold_the_whole_table` |
    ///
    /// The three losing plans are the control for the winning one: without them
    /// "a field holds the table" is a claim about having built five towers rather
    /// than about what was built. Each of the three builds every plot and buys
    /// every upgrade — asserted, so what failed is the **plan** and not the purse
    /// — and each loses on the tenth row with a field that cleared the other
    /// nine.
    #[test]
    fn neither_a_splash_nor_a_slow_tower_alone_can_hold_the_last_row() {
        for plan in [
            [BoltKind, BoltKind, BoltKind, BoltKind, BoltKind],
            [Splash, BoltKind, BoltKind, BoltKind, BoltKind],
            [BoltKind, BoltKind, BoltKind, BoltKind, Slow],
        ] {
            let played = play(&plan);
            let stage = &played.stage;
            let names: Vec<&str> = plan.iter().map(|kind| kind.label()).collect();
            assert_eq!(
                stage.outcome,
                Outcome::Lost,
                "{names:?} held the whole table: {} kills, {} leaks, {} lives left at wave {}",
                stage.kills,
                stage.leaks,
                stage.lives,
                stage.waves.started(),
            );
            // It was really built and really upgraded, so what failed is the plan
            // rather than the purchase.
            assert_eq!(
                stage.towers.len(),
                stage.map.plots().len(),
                "{names:?} did not fill the field",
            );
            assert_eq!(
                stage.upgrades,
                stage.map.plots().len() as u64,
                "{names:?} bought {} of {} upgrades",
                stage.upgrades,
                stage.map.plots().len(),
            );
            assert_eq!(
                stage.refused, 0,
                "{names:?} asked for something it could not pay for",
            );
            // …and it cleared the nine rows before the one that overran it, which
            // is what makes the failure the last row's rather than the first's.
            assert_eq!(
                stage.waves.started(),
                WAVES.len(),
                "{names:?} was overrun before the last row, at wave {}",
                stage.waves.started(),
            );
        }
    }

    /// **A splash tower and a slow tower hold the whole table**, and the kills
    /// pay for them: the run opens with three bolt towers' worth of gold and buys
    /// the rest out of bounties.
    ///
    /// The end-to-end claim for everything slice 3a added — three tower kinds,
    /// the upgrade command, three creep kinds and ten waves — and the plan
    /// `neither_a_splash_nor_a_slow_tower_alone_can_hold_the_last_row` is the
    /// control for. **Where they stand is part of the plan**: the splash tower is
    /// on the plot the creeps meet first, where a wave is still in its press, and
    /// the slow tower is at the gate, where what is left has to get past every
    /// other tower again.
    ///
    /// It also measures the **three instance pools** over a whole run, which is
    /// what `MAX_CREEPS`, `Map::max_bolts` and `Map::max_bursts`' arguments
    /// rest on: each is a bound argued from the rules, and this is the reading
    /// beside it.
    #[test]
    fn a_splash_and_a_slow_tower_hold_the_whole_table() {
        let played = play(&[Splash, BoltKind, BoltKind, BoltKind, Slow]);
        let stage = &played.stage;
        assert_eq!(
            stage.outcome,
            Outcome::Won,
            "the plan lost the table: {} kills, {} leaks, {} lives left at wave {}",
            stage.kills,
            stage.leaks,
            stage.lives,
            stage.waves.started(),
        );
        assert_eq!(
            stage.towers.len(),
            stage.map.plots().len(),
            "not every plot was built"
        );
        assert_eq!(stage.built_by_kind[Splash.index()], 1);
        assert_eq!(stage.built_by_kind[Slow.index()], 1);
        assert_eq!(
            stage.built_by_kind[BoltKind.index()],
            (stage.map.plots().len() - 2) as u64,
        );
        assert_eq!(
            stage.upgrades,
            stage.map.plots().len() as u64,
            "the plan was not upgraded"
        );
        assert_eq!(
            stage.refused, 0,
            "the policy asked for something it could not pay for",
        );
        assert_eq!(
            stage.kills,
            MAX_CREEPS as u64 - stage.leaks,
            "the kills and the leaks do not account for every creep",
        );
        // The bounties paid for it: the opening purse is three bolt towers and
        // the plan is five towers and five upgrades.
        assert!(
            stage.built as u32 * BoltKind.spec(Tier::Base).cost > STARTING_GOLD,
            "the opening purse paid for every tower, so nothing was earned",
        );

        // **The pools cover what a full run puts on the field.** Measured rather
        // than argued, because what decides each is the map's geometry and the
        // table's tempo.
        assert!(
            played.peak_creeps > 0 && played.peak_creeps <= MAX_CREEPS,
            "{} creeps were on the field at once, against a pool of {MAX_CREEPS}",
            played.peak_creeps,
        );
        assert!(
            played.peak_bolts > 0 && played.peak_bolts <= stage.map.max_bolts(),
            "{} bolts were in the air at once, against a pool of {}",
            played.peak_bolts,
            stage.map.max_bolts(),
        );
        assert!(
            played.peak_bursts > 0 && played.peak_bursts <= stage.map.max_bursts(),
            "{} bursts were drawn at once, against a pool of {}",
            played.peak_bursts,
            stage.map.max_bursts(),
        );
    }

    /// **A finished run plays itself again**, so a demo nobody is watching is
    /// never a still picture — see [`RESTART_S`].
    #[test]
    fn a_finished_run_starts_itself_again() {
        let mut stage = new_stage();
        assert_eq!(
            until_over(&mut stage, long_enough_for_the_table()),
            Outcome::Lost,
        );
        assert_eq!(stage.runs, 1);

        idle(&mut stage, RESTART_S + 1.0);
        assert_eq!(stage.outcome, Outcome::Playing, "the run never restarted");
        assert_eq!(stage.runs, 2, "the restart was not counted");
        assert_eq!(stage.lives, STARTING_LIVES, "the lives did not come back");
        assert_eq!(stage.gold, STARTING_GOLD);
        assert_eq!(stage.waves.started(), 0);
    }

    /// **`R` throws the run away at once**, without waiting for it to end — and
    /// it takes the upgrades with it.
    #[test]
    fn the_restart_command_starts_the_run_over() {
        let mut stage = new_stage();
        command(&mut stage, build(0, BoltKind));
        command(&mut stage, step_up(0));
        idle(&mut stage, 5.0);
        assert!(stage.waves.started() > 0 && stage.towers.len() == 1);
        assert_eq!(stage.upgrades, 1);

        command(
            &mut stage,
            Intent {
                place: None,
                kind: tower::Kind::Bolt,
                upgrade: None,
                start_wave: false,
                restart: true,
            },
        );
        assert!(stage.towers.is_empty(), "the towers survived the restart");
        assert!(stage.creeps.is_empty(), "the creeps survived the restart");
        assert_eq!(stage.gold, STARTING_GOLD);
        assert_eq!(stage.upgrades, 0, "the upgrades survived the restart");
        assert_eq!(stage.built_by_kind, [0; tower::KINDS]);
        assert_eq!(stage.runs, 2);
    }

    /// **A team with nobody in it holds the run still** — a dedicated server
    /// with no player sends no wave at an empty field — and the first player's
    /// tick picks it up where it stopped. See `run_team_tick`.
    #[test]
    fn a_team_with_nobody_in_it_holds_the_run_still() {
        let mut stage = new_stage();
        let due = stage
            .waves
            .next_in(stage.elapsed)
            .expect("the build phase is running");
        // Twice the build phase: a player in the session would have seen the
        // first wave arrive half-way through.
        let ticks = (2.0 * due / DT).ceil() as u64;
        for _ in 0..ticks {
            run_team_tick(&mut stage, &[], DT);
        }
        assert_eq!(stage.waves.started(), 0, "a wave was sent at nobody");
        assert_eq!(stage.ticks, 0, "the stage ticked with nobody in it");
        assert_eq!(
            stage.waves.next_in(stage.elapsed),
            Some(due),
            "the build phase ran down with nobody in it"
        );

        for _ in 0..ticks {
            run_team_tick(&mut stage, &[(None, Intent::default())], DT);
        }
        assert!(
            stage.waves.started() > 0,
            "the first player's run never went on"
        );
    }

    /// **A run its last player left is reset once**, so the next group starts
    /// on a fresh field; the empty ticks after that hold still.
    #[test]
    fn a_run_its_last_player_left_is_reset_once() {
        let mut stage = new_stage();
        command(&mut stage, build(0, BoltKind));
        command(&mut stage, send_wave());
        idle(&mut stage, 6.0);
        assert!(
            !stage.towers.is_empty() && stage.ticks > 0,
            "the run never started"
        );
        let runs = stage.runs;

        run_team_tick(&mut stage, &[], DT);
        assert_eq!(stage.runs, runs + 1, "an emptied run was kept");
        assert!(
            stage.towers.is_empty(),
            "the reset kept the last group's towers"
        );
        assert_eq!(stage.waves.started(), 0);

        for _ in 0..10 {
            run_team_tick(&mut stage, &[], DT);
        }
        assert_eq!(
            stage.runs,
            runs + 1,
            "an empty session reset more than once"
        );
    }

    /// **Every refusal names the rule that refused it**, recorded against
    /// the player who sent the command — solo's here, who has no peer — in
    /// the order the commands were read, and kept across a reset until
    /// whatever serves the stage takes it to tell.
    #[test]
    fn every_refusal_names_the_rule_that_refused_it() {
        let mut stage = new_stage();
        command(&mut stage, build(200, BoltKind));
        command(&mut stage, build(0, BoltKind));
        command(&mut stage, build(0, Splash));
        command(&mut stage, step_up(1));
        command(&mut stage, step_up(0));
        stage.gold = 10 * BoltKind.spec(Tier::Upgraded).cost;
        command(&mut stage, step_up(0));
        stage.gold = 0;
        command(&mut stage, build(1, BoltKind));
        command(&mut stage, send_wave());
        command(&mut stage, send_wave());
        assert_eq!(
            stage.refusals,
            [
                (None, Refusal::NoSuchPlot),
                (None, Refusal::PlotTaken),
                (None, Refusal::NoTower),
                (None, Refusal::TopTier),
                (None, Refusal::NotEnoughGold),
                (None, Refusal::NoWaveToSend),
            ]
        );
        assert_eq!(stage.refused, 6, "a refusal recorded but not counted");

        stage.outcome = Outcome::Lost;
        command(&mut stage, build(2, BoltKind));
        assert_eq!(stage.refusals.last(), Some(&(None, Refusal::RunOver)));
        stage.reset();
        assert_eq!(
            stage.refusals.len(),
            7,
            "the reset lost what was not yet told"
        );
    }

    /// **Every refusal has its own byte, and reads back from it** — and no
    /// other byte reads as one.
    #[test]
    fn every_refusal_has_its_own_code() {
        for refusal in Refusal::ALL {
            assert_eq!(Refusal::from_code(refusal.code()), Some(refusal));
        }
        let known = (0..=u8::MAX)
            .filter(|&code| Refusal::from_code(code).is_some())
            .count();
        assert_eq!(known, Refusal::ALL.len());
    }

    /// **A kill pays its kind's bounty**, which is the whole of the economy.
    /// The first wave is every one of it a fast creep, so the purse moves by a
    /// multiple of one number.
    #[test]
    fn a_kill_pays_its_bounty() {
        let mut stage = new_stage();
        command(&mut stage, build(0, BoltKind));
        let purse = stage.gold;
        command(&mut stage, send_wave());
        // One creep's worth of walking past one tower.
        idle(&mut stage, 6.0);
        assert!(stage.kills > 0, "one tower killed nothing in six seconds");
        let bounty = creep::Kind::Fast.spec().bounty;
        assert_eq!(
            stage.gold,
            purse + stage.kills as u32 * bounty,
            "the purse does not match {} kills at {bounty} gold",
            stage.kills,
        );
    }
}
