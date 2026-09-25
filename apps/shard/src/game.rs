//! The simulation: one capsule walking [`crate::zone`], and the server that owns
//! it.
//!
//! ```text
//!  Stage ──▶ ShardModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                        └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::app, crate::page, crate::gpu
//! ```
//!
//! # Four verbs: *explore*, *fight*, *loot* and *level*
//!
//! `docs/plan/sample/15-shard.md`'s milestone 1 is "explore, fight, loot, level,
//! save, resume". This file is the first four of those: a character, a zone
//! with stone in it, gravity, three archetypes of foe with one ability each, a
//! blow that answers them, what they leave when they go down, and what the
//! character learns from both.
//!
//! **Experience arrives from two places and levels are read off one table.**
//! Felling a foe is worth [`foe::Kind::experience`] and taking what it left is
//! worth [`loot::Rarity::experience`], both into the stage's running total. What
//! that total is worth is [`crate::level::THRESHOLDS`], and what a level is worth
//! is a deeper health pool. Nothing here decides either — this file owns *when*
//! experience is granted and the one rule that joins a level to the pool, which
//! is that a raise widens the ceiling without healing a wound.
//!
//! **The loot moves inside the tick.** Which stack is in reach and whether it
//! fits the character's grid are the stage's answers, so the pickup crosses the
//! wire as one intent bit and is applied where every other rule is —
//! `docs/plan/34-inventory.md`'s "clients never assert item state", in the
//! smallest form a single-process sample can hold it in. The one exception is
//! the panel's drag, and [`Game::drag`] says why.
//!
//! Save and resume are the other two verbs this sample now has, and neither is
//! in here: [`crate::save`] owns the format and the platform, and what this file
//! contributes is `Stage::restore` and `Stage::snapshot` — the two functions
//! that turn the stage into a save's payload and back, under the same lock every
//! other reader of the stage takes.
//!
//! # Nothing here does collision, and that is rule 9
//!
//! Every metre the character moves goes through
//! [`CharacterController::move_and_slide`], which sweeps the capsule against
//! [`zone::world`] and slides it along what it hits. Every foe moves through the
//! *same* call against the *same* world — see [`crate::foe`] — and every
//! sighting and every blow is one [`crcbl::phys::PhysicsWorld::cast_ray`]. This
//! file decides **where from**, **which way** and **what a hit costs**; the
//! world decides what is there.
//!
//! # The character carries no collider, and that is deliberate
//!
//! A foe's sighting ray and the character's own cleave both leave the
//! character's capsule centre, and a collider there would be the first thing
//! either of them hit. `apps/breach/src/game.rs` makes the same choice for the
//! same reason, and `docs/backlog.md` records what closes it: a `cast_ray` that
//! can exclude one collider, which is an engine change and now has two callers
//! that would use it. The visible cost is the same one breach records — a foe
//! walks through the character rather than being stopped by them, while the
//! character *is* stopped by a foe, because the foes' bodies are in the world
//! and the character's is not.
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! single-player slice: the walk is a [`crcbl::ecs::GameModule`] the
//! authoritative server owns, stepped on the fixed timestep, with a client on
//! the other end of an `InMemoryTransport`. The camera is the one thing that is
//! **not** on that side, because it is presentation — and what crosses the wire
//! from it is the bearing the player was looking along when they walked.
//!
//! Note that milestone 1 ships **no networking at all** beyond this loopback,
//! and the plan says so in as many words: the shared world is milestone 2's job,
//! on native. The loopback is rule 2, not a network.
//!
//! # The lights are not in here
//!
//! [`crate::light`] is what decides how bright a torch is, and it is a function
//! of the simulated seconds `Stage::elapsed` accumulates a tick
//! at a time. So a paused zone's flames hold still, and two runs of the same
//! length are lit identically; but nothing about a light crosses the wire,
//! because a light is not something the server owns.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::ecs::{ClientInputs, GameModule, World};
use crcbl::inventory::{Cell, Grid, SlotId, Stack};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{CharacterConfig, CharacterController, MoveOutcome, PhysicsWorld};
use crcbl::render::OrbitCamera;
use crcbl::session::Loopback;

use crate::foe::{self, Foe, FoeView, Kind};
use crate::level;
use crate::loot;
use crate::zone;

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `SHD`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 1,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0053_4844,
};

/// The default simulation rate. Reaches the server, the client and the stage, so
/// there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How fast the character walks, in metres a second.
///
/// The genre's pace: brisk enough that crossing the zone is not a chore and slow
/// enough that a visitor can read the room they are crossing, which is what this
/// sample is for.
pub const WALK_SPEED: f64 = 4.2;

/// Gravity, in metres per second squared.
///
/// Integrated into a fall speed rather than applied as a fixed displacement, so
/// the character settles onto the floor at the rate a body falls at rather than
/// at whatever one tick's constant happened to be.
pub const GRAVITY: f64 = -9.81;

/// How often the `[HUD]` heartbeat is logged, in ticks: **a quarter** of a
/// second of simulated time at [`DEFAULT_TICK_HZ`].
///
/// Four times as often as most samples' and twice as often as
/// `apps/breach/src/game.rs`'s, and that is this demo's browser gate paying for
/// itself. Every wait `web/tools/browser-e2e.mjs` makes of this page is a whole
/// number of heartbeats, and this is the heaviest scene on the site: on the
/// software rasteriser that gate runs on, a simulated second costs about five
/// wall seconds here, so the heartbeat period *is* what each step of the gate
/// costs. Measured: taking it from half a second to a quarter took the browser
/// gate from 102 s to under the 90 s that step is budgeted. The driver is told
/// the period through its `beatMs` row, so the slowdown it scales every other
/// budget by stays a true reading.
pub const HEARTBEAT_TICKS: u64 = 15;

// ---------------------------------------------------------------------------
// Controls and the wire
// ---------------------------------------------------------------------------

/// What the input is asking for this tick, before it is sealed.
///
/// The four movement keys and the bearing the view is at. The **rotate** keys
/// are not here: turning the camera is presentation, it swings on the frame's
/// clock in [`crate::app`], and what the simulation needs of it is the bearing
/// below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// Whether the character is swinging this tick.
    ///
    /// A **request** rather than an event: the cadence is the server's, so a
    /// held key swings once per [`foe::STRIKE_PERIOD_S`] rather than once a
    /// tick.
    pub strike: bool,
    /// Whether the character is reaching for what is on the floor this tick.
    ///
    /// A request too, and one the *simulation* answers: what is within
    /// [`loot::LOOT_REACH_M`] and whether it fits the grid are the stage's to
    /// know, so a held key takes at most one stack a tick and takes nothing at
    /// all where there is nothing to take.
    pub pickup: bool,
    /// Where the view is pointing, in [`crate::camera::Iso::yaw`]'s measure.
    pub yaw: f32,
}

/// One client's move command.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Intent {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    strike: bool,
    pickup: bool,
    yaw: f32,
}

const INTENT_FORWARD: u8 = 1 << 0;
const INTENT_BACK: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;
/// The blow. A **request** rather than an event: the server owns the cadence, so
/// a client that sent this every tick still swings once per
/// [`foe::STRIKE_PERIOD_S`].
const INTENT_STRIKE: u8 = 1 << 4;
/// Reaching for what is on the floor. A request like the blow above, and for
/// the same reason: which stack is in reach and whether it fits is the
/// **server's** answer, and a client that decided it would be a client
/// inventing items.
const INTENT_PICKUP: u8 = 1 << 5;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 =
    INTENT_FORWARD | INTENT_BACK | INTENT_LEFT | INTENT_RIGHT | INTENT_STRIKE | INTENT_PICKUP;

/// How many bytes one sealed intent is: a flag byte and one IEEE-754 binary32
/// bearing, little-endian.
///
/// Unchanged by the pickup bit: the flag byte still had room in it, so the loot
/// verb costs the wire nothing. [`INTENT_FLAGS`] is what says which bits this
/// build defines.
const INTENT_BYTES: usize = 1 + core::mem::size_of::<f32>();

impl Intent {
    /// The forward axis, in `-1..=1`. Both keys held is neither, which is what
    /// makes releasing one of them do the obvious thing.
    fn ahead(self) -> f64 {
        f64::from(i8::from(self.forward) - i8::from(self.back))
    }

    /// The strafe axis, positive toward the character's right. See
    /// [`ahead`](Self::ahead).
    fn across(self) -> f64 {
        f64::from(i8::from(self.right) - i8::from(self.left))
    }

    /// The wire form handed to `Client::set_input`.
    fn to_wire(self) -> Vec<u8> {
        let mut flags = 0;
        for (set, bit) in [
            (self.forward, INTENT_FORWARD),
            (self.back, INTENT_BACK),
            (self.left, INTENT_LEFT),
            (self.right, INTENT_RIGHT),
            (self.strike, INTENT_STRIKE),
            (self.pickup, INTENT_PICKUP),
        ] {
            if set {
                flags |= bit;
            }
        }
        let mut bytes = Vec::with_capacity(INTENT_BYTES);
        bytes.push(flags);
        bytes.extend_from_slice(&self.yaw.to_le_bytes());
        bytes
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload of the wrong
    /// length, a flag outside [`INTENT_FLAGS`], or a bearing that is not a finite
    /// number. **Validated rather than trusted**, because these are the only
    /// bytes in this sample a peer chooses — and a `NaN` bearing would reach
    /// [`OrbitCamera::walk_direction`] and put the character somewhere nothing can recover
    /// from.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != INTENT_BYTES {
            return None;
        }
        let flags = bytes[0];
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        let yaw = f32::from_le_bytes(bytes[1..].try_into().ok()?);
        if !yaw.is_finite() {
            return None;
        }
        Some(Self {
            forward: flags & INTENT_FORWARD != 0,
            back: flags & INTENT_BACK != 0,
            left: flags & INTENT_LEFT != 0,
            right: flags & INTENT_RIGHT != 0,
            strike: flags & INTENT_STRIKE != 0,
            pickup: flags & INTENT_PICKUP != 0,
            yaw,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's: the buttons are OR-ed, because each
    /// is a thing the player asked for and a later frame that says nothing is not
    /// a retraction. The **bearing is the last one**, because a view angle is a
    /// state rather than a request and the average of two angles either side of
    /// `π` points the wrong way.
    ///
    /// `held` is the bearing the last tick ran at, and is what a tick with no
    /// readable frame keeps. **The buttons and the bearing default in opposite
    /// directions on purpose**: a button is an edge and letting go is the safe
    /// reading of silence, while a bearing is a pose with no "off" — defaulting
    /// it would swing the walk to due north for one tick.
    fn from_inputs(inputs: ClientInputs<'_>, held: f32) -> Self {
        let mut merged = Self {
            yaw: held,
            ..Self::default()
        };
        for (_tick, data) in inputs.iter() {
            // A frame this build cannot read is skipped rather than taken as an
            // empty intent, which would read as the player letting go.
            let Some(frame) = Self::from_wire(data) else {
                continue;
            };
            merged.forward |= frame.forward;
            merged.back |= frame.back;
            merged.left |= frame.left;
            merged.right |= frame.right;
            merged.strike |= frame.strike;
            merged.pickup |= frame.pickup;
            merged.yaw = frame.yaw;
        }
        merged
    }
}

// ---------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------

/// Everything this sample simulates.
///
/// Behind an `Arc<Mutex<_>>` shared with [`ShardModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is what
/// reads the result, and the two are not the same call stack.
struct Stage {
    world: PhysicsWorld,
    player: CharacterController,
    /// The zone's foes, one per [`foe::POSTS`] row, in that order.
    foes: Vec<Foe>,
    /// Which loot this zone leaves. See [`loot::drop_of`]: the item a foe
    /// leaves is a function of this and the foe's index, so the same seed
    /// clears to the same haul however the fight went.
    seed: u32,
    /// What the character is carrying — the kit's one container primitive,
    /// [`loot::GRID_W`] by [`loot::GRID_H`].
    grid: Grid,
    /// What is lying on the floor, in the order it fell.
    ///
    /// **Every instance in this zone is in exactly one of `floor` and `grid`**,
    /// and both are keyed by the foe that left it — a stack leaves this vector
    /// only once [`Grid::insert`] has answered that it is in the grid, which is
    /// the ordering `docs/plan/34-inventory.md` calls the anti-dupe rule: never
    /// remove and then add.
    floor: Vec<Dropped>,
    /// How many stacks have been taken off the floor. **Monotone**, which is
    /// what a reader polling the heartbeat late needs of it.
    picked: u64,
    /// What the character has left, out of [`Stage::health_max`].
    health: u32,
    /// What they have learned, from every foe felled and every stack taken.
    ///
    /// **Monotone**: nothing spends it and nothing takes it away, which is what
    /// lets a reader polling the heartbeat late compare two lines. The level is
    /// [`level::level_for`] of it and is never stored beside it — a second copy
    /// is a second copy that can disagree.
    experience: u64,
    /// How many times they have been put down and returned to the spawn.
    downs: u64,
    /// How many of the foes had the character engaged at the end of the last
    /// tick.
    ///
    /// Kept rather than recomputed when the readout asks, because the answer is
    /// one ray per foe and the tick has already paid for them.
    engaged: usize,
    /// How many blows the character has swung — **trigger pulls**, whether or
    /// not anything was in reach.
    swings: u64,
    /// How many of those landed on a foe. One swing can land on several: the
    /// cleave answers everything within [`foe::STRIKE_REACH_M`] that has a clear
    /// line, so this counts *bodies struck* rather than swings that connected.
    hits: u64,
    /// How much health those blows took off, summed.
    dealt: u64,
    /// How much the foes' abilities have taken off the character, summed.
    ///
    /// **Monotone**, which is the half a readout needs that
    /// [`Stage::health`] cannot give: health comes back when the character is
    /// put down, so a reader that missed the dip would see a full bar and no
    /// evidence.
    taken: u64,
    /// When the next blow may be swung, in [`Stage::elapsed`] seconds.
    next_strike_at: f64,
    /// Which foe the cleave would answer, as an index into [`Stage::foes`], at
    /// the end of the last tick.
    target: Option<usize>,
    /// How fast the character is falling, in metres a second, negative downward.
    /// Zeroed the moment they are grounded.
    fall_speed: f64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What
    /// [`crate::light::flame`] is a function of, so a paused zone's flames hold
    /// still.
    elapsed: f64,
    /// Seconds of simulated time across **every** session, including the ones a
    /// save was resumed from.
    ///
    /// Separate from [`Stage::elapsed`] on purpose: elapsed is this session's
    /// clock and the torches are a function of it, so seeding it from a save
    /// would have a resumed zone open mid-flicker and the `[HUD]` heartbeat
    /// open at a tick nothing on the page expects. This is the number
    /// [`SaveHeader::playtime_secs`](crcbl::store::save::SaveHeader) means.
    playtime: f64,
    /// The bearing the last tick actually walked along.
    yaw: f32,
    /// What the last move came back with, kept so the frame and the heartbeat
    /// report the tick that happened rather than the one being asked for.
    outcome: MoveOutcome,
    /// How many ticks the move was stopped by something too steep to stand on —
    /// [`MoveOutcome::hit_wall`], counted. In this zone that is the stonework, so
    /// it is the number that says the walls are doing their job.
    blocked: u64,
    /// How many ticks it stepped up onto something —
    /// [`MoveOutcome::stepped_up`], counted. In this zone that is the dais, and
    /// it is what says the vertical variety is variety the character can use.
    climbed: u64,
}

/// One stack lying where a foe fell.
///
/// The foe's index rather than a fresh id: the roster is what bounds how many
/// instances this zone can ever hold, and [`loot::stack_id`] is the one place
/// an id is minted from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dropped {
    /// Which foe left it, as an index into [`foe::POSTS`].
    pub foe: usize,
    /// What it is, and how many of it.
    pub stack: Stack,
    /// Where it lies, in metres: **the fallen body's feet**.
    ///
    /// Where it fell rather than the post it was standing on, because a foe
    /// that noticed the character walked at them and a loot verb that sent the
    /// player back across the room for the drop would be a worse game. A
    /// *resumed* session lies it on the post, and that is not a second rule: a
    /// felled foe is restored onto its post — [`Foe::restore`] puts back its
    /// health and nothing else — so "the loot lies with the body" holds either
    /// way, and it is the body that moved.
    pub at: DVec3,
}

/// Where the character's feet are, given where their capsule's centre is.
fn feet_of(player: &CharacterController) -> f64 {
    let config = player.config();
    player.position().y - (config.radius + config.half_height)
}

impl Stage {
    /// The character on the zone's spawn, ungrounded until the first move finds
    /// the floor.
    fn new(seed: u32) -> Self {
        let config = CharacterConfig::default();
        let lift = DVec3::Y * (config.radius + config.half_height);
        let mut world = zone::world();
        let foes = foe::stand_all(&mut world);
        Self {
            world,
            player: CharacterController::new(config, zone::spawn() + lift),
            foes,
            seed,
            grid: loot::carried(),
            floor: Vec::new(),
            picked: 0,
            // The first level's pool. `crate::level`'s
            // `the_first_level_is_the_pool_the_character_starts_with` is what
            // keeps this constant and `level::health_max` from drifting apart.
            health: foe::HEALTH_MAX,
            experience: 0,
            downs: 0,
            engaged: 0,
            swings: 0,
            hits: 0,
            dealt: 0,
            taken: 0,
            next_strike_at: 0.0,
            target: None,
            fall_speed: 0.0,
            ticks: 0,
            elapsed: 0.0,
            playtime: 0.0,
            yaw: 0.0,
            outcome: MoveOutcome::default(),
            blocked: 0,
            climbed: 0,
        }
    }

    /// How many foes are still on their feet.
    fn alive(&self) -> usize {
        self.foes.iter().filter(|foe| foe.is_alive()).count()
    }

    /// How deep the character's pool is at the level they have reached.
    fn health_max(&self) -> u32 {
        level::health_max(level::level_for(self.experience))
    }

    /// Adds `experience` to what the character has learned, and says so when
    /// that crossed a threshold.
    ///
    /// **A raise does not heal.** The ceiling moves and the health under it
    /// does not, so a character who levels at four health is a character at
    /// four health with further to climb — and `Stage::health` stays at or
    /// under [`Stage::health_max`] by construction, because the maximum only
    /// ever grows. Filling the pool is the *down*'s job, in [`run_tick`], and
    /// keeping the two rules apart is what stops a level-up quietly undoing a
    /// fight.
    fn gain(&mut self, experience: u32) {
        let before = level::level_for(self.experience);
        self.experience += u64::from(experience);
        let after = level::level_for(self.experience);
        if after > before {
            crcbl::log::info!(
                "level: {before} → {after} at {} experience; the pool is now {}",
                self.experience,
                level::health_max(after),
            );
        }
    }

    /// The nearest stack on the floor the character could reach, as an index
    /// into [`Stage::floor`].
    ///
    /// Nearest rather than first, for [`cleave_target`]'s reason: the verb
    /// answers the thing a player would expect it to. Measured from the
    /// character's **feet**, because a stack lies on the floor and the capsule's
    /// centre is chest height — measuring from there would make the reach depend
    /// on how tall the character is.
    fn loot_in_reach(&self) -> Option<usize> {
        let position = self.player.position();
        let feet = DVec3::new(position.x, feet_of(&self.player), position.z);
        let mut nearest: Option<(usize, f64)> = None;
        for (index, dropped) in self.floor.iter().enumerate() {
            let gap = (dropped.at - feet).length();
            if gap > loot::LOOT_REACH_M {
                continue;
            }
            if nearest.is_none_or(|(_, best)| gap < best) {
                nearest = Some((index, gap));
            }
        }
        nearest.map(|(index, _)| index)
    }

    /// Puts what foe `index` was carrying on the floor at `at`.
    ///
    /// Called once, on the tick it falls: a foe is felled exactly once — the
    /// blow that would take it to zero puts it down instead, and there is no
    /// revive — so nothing here has to guard against a second drop.
    fn drop_loot(&mut self, index: usize, at: DVec3) {
        let stack = loot::drop_of(self.seed, index);
        crcbl::log::info!(
            "loot: the {} left a {} {} x{} at {:.2} {:.2}",
            foe::POSTS[index].kind.label(),
            loot::rarity_of(self.seed, index).label(),
            loot::catalog()
                .get(stack.item())
                .map_or("something", crcbl::inventory::ItemDef::name),
            stack.count(),
            at.x,
            at.z,
        );
        self.floor.push(Dropped {
            foe: index,
            stack,
            at,
        });
    }

    /// The nearest stack in reach, into the grid.
    ///
    /// **The order is the whole of the anti-dupe rule.** The stack is taken off
    /// the floor only once [`Grid::insert`] has answered that it is in the grid;
    /// a refusal leaves it lying exactly where it was, which is the state a
    /// player with a full grid is in and not an item this function dropped on
    /// the way through.
    fn take_loot(&mut self) {
        let Some(index) = self.loot_in_reach() else {
            return;
        };
        let dropped = self.floor[index];
        if let Err(error) = loot::stow(&mut self.grid, dropped.stack) {
            crcbl::log::debug!("loot: nothing was taken ({error})");
            return;
        }
        self.floor.remove(index);
        self.picked += 1;
        // **The tier is what a find is worth**, and it is read off the seed and
        // the foe rather than off the stack, because a `Stack` carries no tier
        // — see `crate::loot`.
        self.gain(loot::rarity_of(self.seed, dropped.foe).experience());
    }

    /// Puts back on the floor every stack a felled foe left that the character
    /// is not carrying.
    ///
    /// What a resumed session's floor is, and it is **derived** rather than
    /// saved: an instance is on the floor exactly when the foe that left it is
    /// down and its stack is not in the grid. Saving the floor as well would be
    /// a second copy of the same fact, and a save whose two copies disagreed
    /// would be a save that duplicates an item or loses one.
    fn restore_floor(&mut self) {
        self.floor.clear();
        for index in 0..self.foes.len() {
            if self.foes[index].is_alive() {
                continue;
            }
            let id = loot::stack_id(index);
            if self
                .grid
                .slots()
                .any(|(_, placement)| placement.stack().id() == id)
            {
                continue;
            }
            let stack = loot::drop_of(self.seed, index);
            let at = self.foes[index].feet();
            self.floor.push(Dropped {
                foe: index,
                stack,
                at,
            });
        }
    }

    /// Puts the stage into the state a previous session left.
    ///
    /// **Every field here is one [`crate::save`]'s own decoder has already
    /// validated**, so nothing is clamped or second-guessed on the way in: a
    /// position that was not a finite number inside the zone, or a health above
    /// an archetype's own ceiling, never reaches this function — it reads as no
    /// save at all and the zone opens fresh.
    ///
    /// The fall speed is zeroed rather than saved. A restored character is
    /// standing wherever the save left them and the next
    /// [`CharacterController::move_and_slide`] is what finds the floor under
    /// them, exactly as the first tick of a fresh zone does.
    fn restore(&mut self, character: &crate::save::Character) {
        self.player.set_position(character.centre);
        self.fall_speed = 0.0;
        self.health = character.health;
        self.experience = character.experience;
        self.downs = character.downs;
        self.playtime = character.playtime_secs;
        for (foe, health) in self.foes.iter_mut().zip(character.foes) {
            foe.restore(&mut self.world, health);
        }
        self.grid = character.grid.clone();
        // After the foes, because what is left on the floor is a function of
        // which of them are down. See `Stage::restore_floor`.
        self.restore_floor();
    }

    /// What this session would leave for the next.
    fn snapshot(&self) -> crate::save::Character {
        let mut foes = [0; foe::FOES];
        for (slot, foe) in foes.iter_mut().zip(&self.foes) {
            *slot = if foe.is_alive() { foe.health() } else { 0 };
        }
        crate::save::Character {
            centre: self.player.position(),
            health: self.health,
            experience: self.experience,
            downs: self.downs,
            foes,
            playtime_secs: self.playtime,
            tick: self.ticks,
            grid: self.grid.clone(),
        }
    }
}

/// The nearest living foe the cleave would answer, or `None`.
///
/// Nearest rather than first, so the readout names the body a player would
/// expect to be answering — and the same query the trigger resolves with, so
/// what the panel says is what the blow does. A foe behind a pillar is not in
/// the answer, because [`foe::can_see`] is what decides.
fn cleave_target(world: &mut PhysicsWorld, centre: DVec3, foes: &[Foe]) -> Option<usize> {
    let mut nearest: Option<(usize, f64)> = None;
    for (index, target) in foes.iter().enumerate() {
        if !foe::can_see(world, centre, target, foe::STRIKE_REACH_M) {
            continue;
        }
        let gap = (target.centre() - centre).length();
        if nearest.is_none_or(|(_, best)| gap < best) {
            nearest = Some((index, gap));
        }
    }
    nearest.map(|(index, _)| index)
}

/// One tick of the foes: each one looks, moves, and takes its ability if one is
/// due.
///
/// **Nothing here decides how a foe behaves** — see [`crate::foe`]. What this
/// function owns is the order the three happen in and what an ability that
/// lands costs the character.
fn step_foes(stage: &mut Stage, dt: f64) {
    let now = stage.elapsed;
    let Stage {
        world,
        player,
        foes,
        health,
        taken,
        engaged,
        ..
    } = stage;
    // The character has already moved this tick, so a foe reacts to where they
    // are now rather than to where they were.
    let centre = player.position();
    for foe in foes.iter_mut() {
        foe.advance(world, centre, now, dt);
    }
    // Counted after the walk, so a foe that stepped out from behind a doorpost
    // this tick is engaged on it rather than on the next one.
    *engaged = foes.iter().filter(|foe| foe.is_engaged(now)).count();
    for foe in foes.iter_mut() {
        if let Some(damage) = foe.strikes(world, centre, now) {
            *taken += u64::from(damage);
            *health = health.saturating_sub(damage);
        }
    }
}

/// The character's cleave: everything within [`foe::STRIKE_REACH_M`] with a
/// clear line takes [`foe::STRIKE_DAMAGE`].
///
/// Resolved **after** the foes have moved, for the reason breach's plates give:
/// a body that had not been moved yet is one that cannot be hit where it is
/// drawn.
fn swing(stage: &mut Stage) {
    stage.next_strike_at = stage.elapsed + foe::STRIKE_PERIOD_S;
    stage.swings += 1;
    let centre = stage.player.position();
    let mut fell: Vec<(usize, DVec3)> = Vec::new();
    let Stage {
        world,
        foes,
        hits,
        dealt,
        ..
    } = stage;
    for (index, foe) in foes.iter_mut().enumerate() {
        if !foe::can_see(world, centre, foe, foe::STRIKE_REACH_M) {
            continue;
        }
        *hits += 1;
        *dealt += u64::from(foe::STRIKE_DAMAGE.min(foe.health()));
        if foe.wounded(world, foe::STRIKE_DAMAGE) {
            fell.push((index, foe.feet()));
        }
    }
    // After the loop, because the drop and the experience both read the seed
    // and the floor off the stage the loop is holding pieces of.
    for (index, at) in fell {
        stage.gain(stage.foes[index].kind().experience());
        stage.drop_loot(index, at);
    }
}

/// One tick of the simulation: an intent in, a displacement through the world.
fn run_tick(stage: &mut Stage, intent: Intent, dt: f64) {
    stage.yaw = intent.yaw;

    // **The walk conversion**: a bearing and two axes become a direction in the
    // world. Everything below this line is metres.
    let direction =
        OrbitCamera::walk_direction(f64::from(intent.yaw), intent.ahead(), intent.across());
    let horizontal = direction * WALK_SPEED * dt;

    // Gravity is integrated while the character is off the floor and reset the
    // moment they are on it. A grounded `move_and_slide` discards the vertical it
    // is asked for anyway, so this is about what the *next* tick falls at.
    stage.fall_speed += GRAVITY * dt;
    let motion = horizontal + DVec3::Y * stage.fall_speed * dt;

    let outcome = stage.player.move_and_slide(&mut stage.world, motion);
    if outcome.grounded {
        stage.fall_speed = 0.0;
    } else if outcome.hit_ceiling {
        stage.fall_speed = stage.fall_speed.min(0.0);
    }
    stage.blocked += u64::from(outcome.hit_wall);
    stage.climbed += u64::from(outcome.stepped_up);
    stage.outcome = outcome;

    // **The foes move before the blow is resolved**, and the readout is taken
    // between the two — see [`step_foes`] and [`swing`].
    step_foes(stage, dt);
    let centre = stage.player.position();
    stage.target = {
        let Stage { world, foes, .. } = &mut *stage;
        cleave_target(world, centre, foes)
    };
    // The cadence is the **server's**: a client holding the key down still
    // swings once per period, and one that sent the flag every tick gains
    // nothing by it.
    if intent.strike && stage.elapsed >= stage.next_strike_at {
        swing(stage);
    }

    // **The loot moves inside the tick and nowhere else.** What is in reach and
    // whether it fits are questions about the stage, so a client that decided
    // either would be a client asserting item state — which is the one thing
    // `docs/plan/34-inventory.md` says a client never does. There is no
    // cadence on it: taking one stack a tick is already bounded by how many
    // are within `loot::LOOT_REACH_M`.
    if intent.pickup {
        stage.take_loot();
    }

    // **The character can lose.** Running out returns them to the spawn with
    // full health and one more down against their name — which is what makes
    // the health a pool rather than a number that only falls. **Full is the
    // pool their *level* allows**, not the one they started the zone with:
    // `Stage::gain` widens the ceiling and never fills it, so this is the one
    // place a deeper pool is actually poured.
    if stage.health == 0 {
        stage.health = stage.health_max();
        stage.downs += 1;
        let config = *stage.player.config();
        stage
            .player
            .set_position(zone::spawn() + DVec3::Y * (config.radius + config.half_height));
        stage.fall_speed = 0.0;
    }

    stage.ticks += 1;
    stage.elapsed += dt;
    stage.playtime += dt;
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// The stage, as the server hosts it.
///
/// `register` is empty for the same reason `apps/puppet`'s is: the whole
/// simulation is the [`Stage`] behind the shared cell, and there is no ECS system
/// to register.
struct ShardModule {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for ShardModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShardModule").finish_non_exhaustive()
    }
}

impl GameModule for ShardModule {
    fn name(&self) -> &str {
        "shard"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        let mut stage = lock(&self.shared);
        let held = stage.yaw;
        run_tick(&mut stage, Intent::from_inputs(inputs, held), dt);
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
/// A plain struct rather than a borrow of the stage: the frame runs on the
/// frame's thread and the stage is behind a mutex the tick holds, and a frame
/// that read through the lock would be holding it for the length of a draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderState {
    /// The **centre** of the character's capsule.
    pub position: DVec3,
    /// Where the feet are, in metres above the floor plane — what the figure is
    /// drawn at and what the camera pivots on.
    pub feet: DVec3,
    /// Whether the character is standing on something.
    pub grounded: bool,
    /// Whether the last move was stopped by something too steep to stand on.
    pub blocked: bool,
    /// Seconds of simulated time — what [`crate::light`] is a function of.
    pub elapsed: f64,
    /// One view per [`foe::POSTS`] row, in that order.
    pub foes: [FoeView; foe::FOES],
    /// What the character has left, out of the pool their level allows.
    pub health: u32,
    /// What they have learned. The level and the pool are both
    /// [`crate::level`]'s of this — carried alone so the frame and the
    /// simulation cannot disagree about which level a total is.
    pub experience: u64,
    /// How many foes are still on their feet.
    pub alive: usize,
    /// How many stacks the character is carrying.
    pub carried: usize,
    /// How many are lying on the floor.
    pub floor: usize,
    /// Whether one of those is close enough to take.
    pub in_reach: bool,
}

/// The stage's numbers, for the debug overlay and the `[HUD]` line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub ticks: u64,
    pub position: DVec3,
    pub feet: f64,
    pub grounded: bool,
    /// How many ticks the walk was refused by stone.
    pub blocked: u64,
    /// How many ticks it stepped up onto the dais.
    pub climbed: u64,
    /// Seconds of simulated time.
    pub elapsed: f64,
    /// Seconds of simulated time across every session, including the ones this
    /// one resumed from. What `crate::save::SaveStats` reports and what the
    /// save's header carries.
    pub playtime: f64,
    /// The bearing the last tick walked along, in radians.
    pub yaw: f32,
    /// How many foes are still on their feet.
    pub alive: usize,
    /// How many of them have the character engaged.
    ///
    /// **The control for every claim about the fight**, on the line itself: a
    /// build that engaged unconditionally reports this at its ceiling from the
    /// first tick, and one that never noticed anything leaves it at zero for the
    /// whole run.
    pub engaged: usize,
    /// What the character has left, out of the pool their level allows.
    pub health: u32,
    /// What they have learned. **Monotone**, so a reader polling the heartbeat
    /// late cannot miss a level — see [`crate::level`].
    pub experience: u64,
    /// How many times they have been put down and returned to the spawn.
    pub downs: u64,
    /// Blows swung, and the bodies those blows landed on. `swings` above `hits`
    /// is a swing that reached nothing, which is the control for the cleave
    /// resolving against the world rather than counting key presses.
    pub swings: u64,
    pub hits: u64,
    /// How much health the character has taken off the foes, and how much the
    /// foes have taken off them.
    pub dealt: u64,
    pub taken: u64,
    /// Which archetype the cleave would answer, or `None` for a swing that would
    /// reach nothing.
    pub target: Option<Kind>,
    /// How many stacks the character is carrying, and what they weigh in grams
    /// — [`crcbl::inventory::Grid::weight_g`], which is the kit's flat sum over
    /// one grid.
    pub carried: usize,
    pub weight_g: u64,
    /// How many stacks are lying on the floor.
    pub floor: usize,
    /// How many have been taken off it. **Monotone**, so a reader that polls
    /// the heartbeat late cannot miss one.
    pub picked: u64,
}

impl crcbl::ui::DebugModule for Stats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("shard");
        section.row("tick", format_args!("{}", self.ticks));
        section.row(
            "pos",
            format_args!(
                "{:.2} {:.2} {:.2}",
                self.position.x, self.feet, self.position.z
            ),
        );
        section.row("bearing", format_args!("{:.2}", self.yaw));
        section.row(
            "ground",
            format_args!("{}", if self.grounded { "yes" } else { "no" }),
        );
        section.row("blocked", format_args!("{}", self.blocked));
        section.row("climbed", format_args!("{}", self.climbed));
        section.row(
            "health",
            format_args!("{}/{}", self.health, self.health_max()),
        );
        section.row(
            "level",
            format_args!("{} ({} xp)", self.level(), self.experience),
        );
        section.row("downs", format_args!("{}", self.downs));
        section.row("foes", format_args!("{}/{}", self.alive, foe::FOES));
        section.row("engaged", format_args!("{}", self.engaged));
        section.row_str("target", self.target_label());
        section.row("swings", format_args!("{}/{}", self.hits, self.swings));
        section.row("damage", format_args!("{} / {}", self.dealt, self.taken));
        section.row(
            "carried",
            format_args!("{} ({} g)", self.carried, self.weight_g),
        );
        section.row(
            "loot",
            format_args!("{} down, {} taken", self.floor, self.picked),
        );
        section.row("elapsed", format_args!("{:.1} s", self.elapsed));
    }
}

impl Stats {
    /// Which level the character has reached, off their experience.
    #[must_use]
    pub const fn level(&self) -> u32 {
        level::level_for(self.experience)
    }

    /// How deep their pool is at it.
    #[must_use]
    pub const fn health_max(&self) -> u32 {
        level::health_max(self.level())
    }

    /// What the cleave would answer, as one word.
    ///
    /// `"none"` rather than an empty string, so a heartbeat that names it cannot
    /// be read as a missing field.
    #[must_use]
    pub fn target_label(&self) -> &'static str {
        self.target.map_or("none", Kind::label)
    }
}

// ---------------------------------------------------------------------------
// The facade
// ---------------------------------------------------------------------------

/// What can stop shard before it starts.
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

/// The stage, its server, its client, and the clock that drives all three.
pub struct Game {
    session: Loopback,
    shared: Arc<Mutex<Stage>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// What the player is holding down, sent on the next tick.
    pending: Intent,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ticks_run", &self.ticks_run)
            .finish_non_exhaustive()
    }
}

impl Game {
    /// Builds the server, its client and the stage between them.
    ///
    /// `seed` is what the zone's loot is rolled from — see [`loot::drop_of`] —
    /// and `restore` is what a previous session left, or `None` for a zone that
    /// opens fresh. It is applied to the stage **before** the server is built
    /// and therefore before any tick has run, so the first tick a resumed
    /// session takes is one from the state that was saved rather than one from
    /// the spawn — see `Stage::restore`.
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
    pub fn new(
        tick_hz: u32,
        seed: u32,
        restore: Option<crate::save::Character>,
    ) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let mut stage = Stage::new(seed);
        if let Some(character) = &restore {
            stage.restore(character);
        }
        let shared = Arc::new(Mutex::new(stage));

        // An empty world, and that is the honest shape: this sample has no entity
        // and no ECS system. What the server hosts is the module, and what the
        // module owns is the stage.
        let session = Loopback::new(
            World::new(),
            Box::new(ShardModule {
                shared: Arc::clone(&shared),
            }),
            tick_hz,
            COMPATIBILITY,
        )
        .map_err(|error| GameError::Server(error.to_string()))?;

        let tick_period = session.tick_period();
        let mut game = Self {
            session,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            pending: Intent::default(),
        };

        // **One tick spent on the handshake, before the character moves.**
        // `Server::update` drains the transport inside `tick`, so the client's
        // hello is not read until a tick runs, and until the session is up the
        // client drops every input frame it is asked to send. Spending it here is
        // what makes the player's first key the first the simulation sees.
        game.sim_time = tick_period;
        game.session.client_mut().update(game.sim_time);
        game.session.server_mut().update(game.sim_time);
        game.session.client_mut().update(game.sim_time);
        if game.session.server().session_state() != crcbl::net::SessionState::Connected {
            return Err(GameError::Server(
                "the loopback session did not come up in its first tick".into(),
            ));
        }

        crcbl::log::info!(
            "sim: {tick_hz} Hz, {:.3} ms per tick, walking at {WALK_SPEED} m/s across a \
             {}x{} tile zone",
            tick_period.as_secs_f64() * 1e3,
            zone::COLS,
            zone::ROWS,
        );
        Ok(game)
    }

    /// Records what the player is asking for, to be sent on the next tick.
    pub fn set_controls(&mut self, controls: Controls) {
        self.pending = Intent {
            forward: controls.forward,
            back: controls.back,
            left: controls.left,
            right: controls.right,
            strike: controls.strike,
            pickup: controls.pickup,
            yaw: controls.yaw,
        };
    }

    /// Advances the server, and with it the stage, by exactly one tick.
    pub fn tick(&mut self) {
        self.sim_time += self.tick_period;
        let (server, client) = self.session.both_mut();

        // The bytes are the whole input path: the client seals them, the
        // transport carries them and the module decodes them, exactly as a remote
        // client's would be.
        client.set_input(self.pending.to_wire());

        // Send, simulate, then receive — and the send has to come first.
        // `Client::update` is the only thing that puts input on the wire and the
        // server drains the wire at the top of its tick, so a client updated only
        // after the server posts this tick's controls to the next one.
        client.update(self.sim_time);
        let server_ticks = server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        // Consumes no tick — the clock has not moved between the two — and is
        // there to take the snapshot this tick produced.
        client.update(self.sim_time);
        self.ticks_run += 1;
    }

    /// How many times [`Game::tick`] has been called.
    #[must_use]
    pub const fn ticks_run(&self) -> u64 {
        self.ticks_run
    }

    /// What the frame should draw.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        let stage = lock(&self.shared);
        let position = stage.player.position();
        RenderState {
            position,
            feet: DVec3::new(position.x, feet_of(&stage.player), position.z),
            grounded: stage.outcome.grounded,
            blocked: stage.outcome.hit_wall,
            elapsed: stage.elapsed,
            foes: foe::views(&stage.foes, stage.elapsed),
            health: stage.health,
            experience: stage.experience,
            alive: stage.alive(),
            carried: stage.grid.len(),
            floor: stage.floor.len(),
            in_reach: stage.loot_in_reach().is_some(),
        }
    }

    /// What this session would leave for the next, read off the stage the
    /// server owns.
    ///
    /// Under the same lock every other reader takes, so a snapshot is one
    /// tick's state rather than a mixture of two — which for a save is the
    /// difference between a character standing where their health says they
    /// were and one who is not.
    #[must_use]
    pub fn snapshot(&self) -> crate::save::Character {
        lock(&self.shared).snapshot()
    }

    /// The stage's numbers for the debug panel and the `[HUD]` line.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let stage = lock(&self.shared);
        Stats {
            ticks: stage.ticks,
            position: stage.player.position(),
            feet: feet_of(&stage.player),
            grounded: stage.outcome.grounded,
            blocked: stage.blocked,
            climbed: stage.climbed,
            elapsed: stage.elapsed,
            playtime: stage.playtime,
            yaw: stage.yaw,
            alive: stage.alive(),
            engaged: stage.engaged,
            health: stage.health,
            experience: stage.experience,
            downs: stage.downs,
            swings: stage.swings,
            hits: stage.hits,
            dealt: stage.dealt,
            taken: stage.taken,
            target: stage.target.map(|index| stage.foes[index].kind()),
            carried: stage.grid.len(),
            weight_g: loot::weight_g(&stage.grid),
            floor: stage.floor.len(),
            picked: stage.picked,
        }
    }

    /// What this zone's loot is rolled from — the item, the count and the tier.
    ///
    /// Read off the stage rather than kept a second time beside it, for
    /// `Stage::restore_floor`'s reason: the seed the simulation is actually
    /// using is the only one a tier drawn on the panel may be rolled from.
    #[must_use]
    pub fn seed(&self) -> u32 {
        lock(&self.shared).seed
    }

    /// What the character is carrying, for the panel that draws it.
    ///
    /// A clone rather than a borrow, for [`RenderState`]'s reason: the grid is
    /// behind the tick's lock and a panel that read through it would hold the
    /// lock for the length of a draw. It is [`loot::GRID_W`] by
    /// [`loot::GRID_H`] cells and at most [`foe::FOES`] placements, and
    /// [`crate::app`] takes it only on the frames the panel is open.
    #[must_use]
    pub fn grid(&self) -> Grid {
        lock(&self.shared).grid.clone()
    }

    /// Moves the stack at `slot` so its origin is `at` — the cell a panel's
    /// drag landed it on, grab offset already applied by
    /// [`crcbl::ui::grid_drag`]. Answers whether anything moved.
    ///
    /// **This is the one mutation that does not cross the wire**, and the
    /// reason is that there is no wire command to carry it: `Intent` is a flag
    /// byte and a bearing, and a cell pair is neither. `docs/plan/34-inventory.md`'s
    /// `Move` command and its server-side validation are the kit's server half,
    /// which is not built — so a drag here reaches the stage through the same
    /// lock a snapshot does, in a process where the client and the server are
    /// the same memory. `docs/backlog.md` carries it as what breach's adoption
    /// has to force.
    ///
    /// The move itself is [`crcbl::inventory::Grid::move_within`], which is
    /// atomic: a refused drag leaves the grid exactly as it was, down to the
    /// slot id the panel is holding.
    pub fn drag(&mut self, slot: SlotId, at: Cell) -> bool {
        let mut stage = lock(&self.shared);
        let Some(placement) = stage.grid.slot(slot) else {
            return false;
        };
        stage
            .grid
            .move_within(loot::catalog(), slot, at, placement.rotation())
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::inventory::StackId;

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// **The beat `web/tools/browser-e2e.mjs` divides by is this sample's
    /// heartbeat**: [`HEARTBEAT_TICKS`] at [`DEFAULT_TICK_HZ`], in milliseconds.
    ///
    /// Not an assertion in the gate but its denominator. Group B reads the
    /// observed beat against `beatMs` to work out how far behind real time the
    /// machine is running the demo, and scales every later budget by that, so a
    /// period changed here and not there stretches or shrinks every timeout in
    /// the run and nothing reddens.
    #[test]
    fn the_browser_gates_beat_is_this_samples_heartbeat() {
        let millis = HEARTBEAT_TICKS * 1_000;
        let hz = u64::from(DEFAULT_TICK_HZ);
        assert_eq!(
            millis % hz,
            0,
            "the heartbeat is not a whole number of milliseconds, which `beatMs` cannot spell"
        );
        assert_eq!(
            crcbl_sample_test::browser_gate_demo_expectation("shard", &["beatMs"]),
            (millis / hz).to_string(),
            "the browser gate's shard beatMs is not HEARTBEAT_TICKS at DEFAULT_TICK_HZ"
        );
    }

    /// A stage that has already found the floor.
    fn ready() -> Stage {
        let mut stage = Stage::new(loot::DEFAULT_SEED);
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// Holds `intent` for `seconds`.
    fn hold(stage: &mut Stage, intent: Intent, seconds: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (seconds / DT).round() as u64;
        for _ in 0..ticks {
            run_tick(stage, intent, DT);
        }
    }

    /// Every key held, walking away from the camera.
    const AHEAD: Intent = Intent {
        forward: true,
        back: false,
        left: false,
        right: false,
        strike: false,
        pickup: false,
        yaw: 0.0,
    };

    /// And walking towards it, which from the spawn is where the dais is.
    const BACK: Intent = Intent {
        forward: false,
        back: true,
        left: false,
        right: false,
        strike: false,
        pickup: false,
        yaw: 0.0,
    };

    /// Swinging, standing still.
    const SWING: Intent = Intent {
        forward: false,
        back: false,
        left: false,
        right: false,
        strike: true,
        pickup: false,
        yaw: 0.0,
    };

    /// Reaching for what is on the floor, standing still.
    const REACH: Intent = Intent {
        forward: false,
        back: false,
        left: false,
        right: false,
        strike: false,
        pickup: true,
        yaw: 0.0,
    };

    /// Walking into the zone with the blow held down.
    const CHARGE: Intent = Intent {
        strike: true,
        ..AHEAD
    };

    /// **Held input walks the character and released input stops them**, which is
    /// the same pair the browser gate asserts and the reason it can be asserted
    /// there: if it were true only in a headless test, the browser check would be
    /// a check of the shim rather than of the controller.
    #[test]
    fn the_character_walks_while_asked_to_and_stops_when_they_are_not() {
        let mut stage = ready();
        let start = stage.player.position();
        hold(&mut stage, AHEAD, 1.0);
        let walked = stage.player.position();
        let covered = (walked - start).length();
        assert!(
            covered > 0.5 * WALK_SPEED,
            "a second of walking covered {covered:.2} m at {WALK_SPEED} m/s",
        );
        // A bearing of zero walks down -Z, which is where the zone reaches.
        assert!(walked.z < start.z - 0.5 * WALK_SPEED);

        hold(&mut stage, Intent::default(), 1.0);
        let stopped = stage.player.position();
        assert!(
            (stopped - walked).length() < 1e-6,
            "it kept moving after the key came up: {walked:?} then {stopped:?}",
        );
    }

    /// **The walk goes where the bearing says**, which is the seam this whole
    /// sample turns on: the camera is presentation and the only thing the
    /// simulation is ever told about it is this one angle.
    ///
    /// A quarter turn anticlockwise about `+Y` puts "away from the camera" along
    /// `−X`, which is [`OrbitCamera::walk_direction`]'s measure and not a sign
    /// this test is free to choose — `a_zero_bearing_walks_into_the_zone` in that
    /// module is what pins the convention, and this is it reaching the wire.
    #[test]
    fn the_bearing_on_the_wire_is_what_the_walk_follows() {
        let turned = Intent {
            yaw: core::f32::consts::FRAC_PI_2,
            ..AHEAD
        };
        let mut stage = ready();
        let start = stage.player.position();
        hold(&mut stage, turned, 0.6);
        let walked = stage.player.position();
        assert!(
            walked.x < start.x - 1.0,
            "a turned bearing walked to {walked:?} from {start:?}",
        );
        assert!(
            (walked.z - start.z).abs() < 0.3,
            "it drifted down the old bearing to {walked:?}",
        );
    }

    /// **The character walks onto the dais and stays on it**, which is what makes
    /// the zone's vertical variety something the controller uses rather than
    /// scenery it walks round.
    ///
    /// The wall is the control: the same walk into stone is refused, so a
    /// controller that climbed everything would pass the step and fail this.
    #[test]
    fn the_dais_is_stepped_onto_and_the_wall_is_not() {
        let mut stage = ready();
        // Straight down +Z from the spawn — towards the camera — is the dais,
        // and a second and a half at `WALK_SPEED` reaches its middle rather
        // than crossing it.
        hold(&mut stage, BACK, 1.5);
        assert!(
            stage.climbed > 0,
            "nothing was stepped onto on the way to the dais",
        );
        let feet = feet_of(&stage.player);
        assert!(
            (feet - zone::DAIS_HEIGHT).abs() < 0.05,
            "the character's feet are at {feet:.3} m, and the dais is at {:.3} m",
            zone::DAIS_HEIGHT,
        );

        // …and the stone at the far end of the zone refuses them.
        hold(&mut stage, BACK, 20.0);
        assert!(stage.blocked > 0, "nothing ever stopped the walk");
        let stopped = stage.player.position();
        hold(&mut stage, BACK, 2.0);
        assert!(
            (stage.player.position() - stopped).length() < 0.05,
            "the character walked through the far wall",
        );
    }

    /// **A sealed intent survives the wire, and nothing else does.** These are
    /// the only bytes in this sample a peer chooses, and a `NaN` bearing reaching
    /// [`OrbitCamera::walk_direction`] is unrecoverable.
    #[test]
    fn only_an_intent_this_build_sealed_reads_back() {
        for intent in [
            Intent::default(),
            AHEAD,
            Intent {
                back: true,
                left: true,
                right: true,
                strike: true,
                pickup: true,
                yaw: -2.5,
                ..Intent::default()
            },
            Intent {
                strike: true,
                ..Intent::default()
            },
            // The bit this slice added, on its own: a build that sealed it and
            // did not read it back would hand the simulation a pickup that
            // never happened.
            REACH,
        ] {
            let wire = intent.to_wire();
            assert_eq!(wire.len(), INTENT_BYTES);
            assert_eq!(Intent::from_wire(&wire), Some(intent));
        }

        assert_eq!(Intent::from_wire(&[]), None, "an empty frame");
        assert_eq!(Intent::from_wire(&[0; INTENT_BYTES + 1]), None, "too long");
        let mut spurious = Intent::default().to_wire();
        spurious[0] = 0xC0;
        assert_eq!(
            Intent::from_wire(&spurious),
            None,
            "a flag we never write: {INTENT_FLAGS:#04x} is every bit this build seals",
        );
        let mut nan = Intent::default().to_wire();
        nan[1..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Intent::from_wire(&nan), None, "a bearing that is not one");
    }

    /// **A blow that reaches nothing kills nothing, and one that reaches a foe
    /// does.** The pair the browser gate makes in a browser, made here where a
    /// failure names the step.
    ///
    /// The first half is the control, and it is the whole claim that the cleave
    /// is resolved against the world: a build that counted key presses passes
    /// the kill and fails this, because on the spawn there is nothing within
    /// [`foe::STRIKE_REACH_M`] of the character at all.
    #[test]
    fn a_blow_that_reaches_nothing_kills_nothing_and_one_that_reaches_a_foe_does() {
        let mut stage = ready();
        assert_eq!(stage.alive(), foe::FOES, "the zone opened with foes down");

        // Swinging at the empty spawn, for long enough that the cadence lets
        // several blows through.
        hold(&mut stage, SWING, 2.0);
        assert!(stage.swings > 1, "the cadence let one blow through in 2 s");
        assert_eq!(stage.hits, 0, "a blow at an empty room landed on something");
        assert_eq!(stage.dealt, 0);
        assert_eq!(stage.alive(), foe::FOES);
        assert_eq!(stage.target, None, "something was in reach on the spawn");

        // …and then walking up the corridor into the husk on the doorway, with
        // the blow held down.
        let swung = stage.swings;
        hold(&mut stage, CHARGE, 8.0);
        assert!(
            stage.swings > swung,
            "the walk swallowed the blow: {swung} then {}",
            stage.swings,
        );
        assert!(
            stage.hits > 0,
            "nothing was ever within reach of the cleave"
        );
        assert!(stage.dealt > 0);
        assert!(
            stage.alive() < foe::FOES,
            "{} foes still standing after {} blow(s) landed",
            stage.alive(),
            stage.hits,
        );
    }

    /// **A foe notices the character when they come at it, and not before.**
    ///
    /// The "not before" is the control, and it is what the browser gate's
    /// engagement check depends on: every post is out of [`foe::NOTICE_M`] of
    /// the spawn, so a build that engaged unconditionally fails here rather than
    /// making that gate meaningless.
    #[test]
    fn a_foe_notices_the_character_only_once_they_come_at_it() {
        let mut stage = ready();
        hold(&mut stage, Intent::default(), 3.0);
        assert_eq!(
            stage.engaged, 0,
            "a foe engaged a character standing on the spawn",
        );
        assert_eq!(stage.taken, 0, "something reached them on the spawn");
        assert_eq!(stage.health, foe::HEALTH_MAX);

        hold(&mut stage, AHEAD, 6.0);
        assert!(
            stage.engaged > 0,
            "nothing noticed the character walking up the corridor at it",
        );
    }

    /// **A foe's ability costs the character health**, which is what makes the
    /// zone something a player can lose in.
    ///
    /// The control is the run above it: `taken` sat at zero for three seconds
    /// with the character on the spawn, so this is not a counter that was always
    /// climbing.
    #[test]
    fn a_foe_that_reaches_the_character_costs_them_health() {
        let mut stage = ready();
        hold(&mut stage, Intent::default(), 2.0);
        let untouched = stage.taken;
        assert_eq!(untouched, 0);

        // Walking into the husk and standing there, without swinging back.
        hold(&mut stage, AHEAD, 10.0);
        assert!(
            stage.taken > 0,
            "{} foe(s) engaged and none of them ever landed anything",
            stage.engaged,
        );
        assert!(
            stage.health < foe::HEALTH_MAX || stage.downs > 0,
            "the character took {} damage and still has all {} of their health",
            stage.taken,
            stage.health,
        );
    }

    /// **A session that resumes a snapshot opens where the last one stopped**,
    /// and a session handed nothing opens on the spawn with the zone intact.
    ///
    /// The second half is the control, and it is the one that matters: every
    /// reading the first half asserts — the position, the health, the downs, the
    /// standing count — is one a *fresh* zone also has a value for, so without
    /// the pair "it resumed" would pass for a build that ignored the argument
    /// entirely and always opened the same way.
    ///
    /// The snapshot is taken from a stage that was actually played rather than
    /// written by hand, so what is asserted is a round trip through the two
    /// functions a save goes through and not a struct this test filled in.
    #[test]
    fn a_resumed_session_opens_where_the_last_one_stopped_and_a_fresh_one_does_not() {
        // A stage walked away from the spawn, wounded, with its husk felled and
        // its warden hurt — a state nothing about opening a zone can produce.
        let mut played = ready();
        hold(&mut played, AHEAD, 1.0);
        played.health = 37;
        played.downs = 4;
        played.experience = 62;
        played.foes[0].wounded(&mut played.world, foe::HEALTH_MAX);
        played.foes[2].wounded(&mut played.world, 40);
        let saved = played.snapshot();
        assert_eq!(saved.foes[0], 0, "the husk was not felled");
        assert!(saved.centre.z < zone::spawn().z - 1.0, "it never walked");

        // ---- the stage, where the comparison can be exact --------------------
        let mut restored = Stage::new(loot::DEFAULT_SEED);
        restored.restore(&saved);
        assert_eq!(
            restored.snapshot(),
            crate::save::Character {
                // The one field that is provenance rather than state: it says
                // which tick wrote the save, and a session that resumes one
                // counts its own ticks from zero.
                tick: 0,
                ..saved.clone()
            },
            "a field did not survive the round trip",
        );
        // …and this session's own clock starts again, which is what keeps the
        // torches opening at the start of their cycle and the heartbeat at the
        // tick every reader expects. `playtime` is the one that carries over.
        assert!(saved.tick > 0, "the played stage never ticked");
        assert_eq!(restored.ticks, 0);
        assert_eq!(restored.elapsed, 0.0);
        assert_eq!(restored.playtime, saved.playtime_secs);

        // ---- and through the facade, which spends one tick on the handshake --
        let resumed = Game::new(DEFAULT_TICK_HZ, loot::DEFAULT_SEED, Some(saved.clone()))
            .expect("the loopback comes up");
        let stats = resumed.stats();
        assert!(
            (stats.position.x - saved.centre.x).abs() < 1e-9
                && (stats.position.z - saved.centre.z).abs() < 1e-9,
            "it opened at {:?} rather than at {:?}",
            stats.position,
            saved.centre,
        );
        assert_eq!(stats.health, saved.health);
        assert_eq!(stats.experience, saved.experience);
        assert_eq!(stats.level(), level::level_for(saved.experience));
        assert_eq!(stats.downs, saved.downs);
        assert_eq!(stats.alive, foe::FOES - 1, "the felled foe was back up");

        let fresh =
            Game::new(DEFAULT_TICK_HZ, loot::DEFAULT_SEED, None).expect("the loopback comes up");
        let opened = fresh.stats();
        assert_eq!(
            opened.alive,
            foe::FOES,
            "a fresh zone opened already cleared"
        );
        assert_eq!(opened.health, foe::HEALTH_MAX);
        assert_eq!(opened.experience, 0, "a fresh zone opened part-levelled");
        assert_eq!(opened.level(), 1);
        assert_eq!(opened.downs, 0);
        assert!(
            (opened.position.z - zone::spawn().z).abs() < 1e-9,
            "a fresh zone opened at {:?} rather than on the spawn",
            opened.position,
        );
        assert!(
            opened.playtime < stats.playtime,
            "a fresh zone opened with {:.2} s of play behind it, against the \
             resumed session's {:.2} s",
            opened.playtime,
            stats.playtime,
        );
    }

    /// **A run of the whole game walks the character and reports it**, which is
    /// the one check that says the server, the client, the transport and the
    /// stage are all joined up.
    #[test]
    fn the_loopback_carries_a_held_key_to_the_controller() {
        let mut game = Game::new(DEFAULT_TICK_HZ, loot::DEFAULT_SEED, None)
            .expect("the loopback always comes up");
        for _ in 0..30 {
            game.tick();
        }
        let start = game.render_state();
        assert!(start.grounded, "the character never found the floor");

        game.set_controls(Controls {
            forward: true,
            ..Controls::default()
        });
        for _ in 0..60 {
            game.tick();
        }
        let walked = game.render_state();
        assert!(
            walked.position.z < start.position.z - 1.0,
            "a second of walking got from {:?} to {:?}",
            start.position,
            walked.position,
        );
        assert!(walked.elapsed > start.elapsed, "the clock stood still");

        game.set_controls(Controls::default());
        for _ in 0..60 {
            game.tick();
        }
        let stopped = game.render_state();
        assert!(
            (stopped.position - walked.position).length() < 0.01,
            "it kept moving with nothing held",
        );
        // One more tick than `Game::tick` ran, and exactly one: `Game::new`
        // spends a tick bringing the loopback session up before the player can
        // move, which the module sees and the caller's counter does not.
        assert_eq!(game.stats().ticks, game.ticks_run() + 1);
    }

    // ---- the loot loop -----------------------------------------------------

    /// Walks into the zone with the blow held until something falls, and
    /// answers how long that took. Panics rather than looping forever on a
    /// build where nothing can be felled.
    fn until_something_falls(stage: &mut Stage) -> f64 {
        let mut seconds = 0.0;
        while stage.alive() == foe::FOES {
            assert!(seconds < 30.0, "nothing fell in 30 s of charging");
            run_tick(stage, CHARGE, DT);
            seconds += DT;
        }
        seconds
    }

    /// How many instances this zone has produced: one per felled foe, and there
    /// is no other source.
    fn felled(stage: &Stage) -> usize {
        foe::FOES - stage.alive()
    }

    /// **A felled foe leaves a stack on the floor, and it is the one the seed
    /// says.** The first half of the loot loop: without it the pickup verb has
    /// nothing to answer.
    ///
    /// The control is the run before the kill — the floor is empty for every
    /// tick of the walk up the corridor, so this is a drop rather than a zone
    /// that opened with items lying in it.
    #[test]
    fn a_felled_foe_leaves_something_on_the_floor() {
        let mut stage = ready();
        hold(&mut stage, AHEAD, 1.0);
        assert!(stage.floor.is_empty(), "the zone opened with loot on it");

        until_something_falls(&mut stage);
        assert_eq!(
            stage.floor.len(),
            felled(&stage),
            "{} foe(s) went down and {} stack(s) are on the floor",
            felled(&stage),
            stage.floor.len(),
        );

        let dropped = stage.floor[0];
        assert_eq!(
            dropped.stack,
            loot::drop_of(loot::DEFAULT_SEED, dropped.foe),
            "the drop is not the one this seed rolls for that foe",
        );
        assert!(
            dropped.at.y.abs() < 1.0,
            "the stack is at {:?}, which is not on the floor",
            dropped.at,
        );

        // …and every id is its own, which is what makes a grid holding two of
        // them holding two items rather than one twice.
        let mut ids: Vec<_> = stage.floor.iter().map(|held| held.stack.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), stage.floor.len(), "two stacks share an id");
    }

    /// **The stack moves off the floor inside a tick and nowhere else, and the
    /// zone still holds exactly what it dropped.**
    ///
    /// Two claims, and the second is the one that cannot be faked: the total
    /// across the floor and the grid is the number of felled foes before the
    /// pickup and after it. A `take_loot` written as remove-then-insert passes
    /// the first and fails this the moment the insert is refused; one that
    /// forgot to take the stack off the floor fails it the other way.
    #[test]
    fn a_pickup_moves_a_stack_into_the_grid_only_inside_a_tick() {
        let mut stage = ready();
        until_something_falls(&mut stage);
        let held = felled(&stage);
        assert_eq!(stage.floor.len() + stage.grid.len(), held);
        assert!(stage.grid.is_empty(), "the character opened carrying loot");
        assert!(
            stage.loot_in_reach().is_some(),
            "the body fell out of reach of the character that felled it",
        );

        // Standing on it takes nothing: the stage moves loot in `run_tick` and
        // in no other call, which is what `a_pickup_key_reaches_the_simulation`
        // in `crate::app` asserts from the other end.
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.grid.is_empty(), "a tick with no reach took the stack");

        run_tick(&mut stage, REACH, DT);
        assert_eq!(stage.grid.len(), 1, "the pickup tick took nothing");
        assert_eq!(stage.picked, 1);
        assert_eq!(
            stage.floor.len() + stage.grid.len(),
            felled(&stage),
            "the zone gained or lost an instance across the pickup",
        );

        // The stack that arrived is the one that was lying there, id and all —
        // not a fresh instance of the same item.
        let carried = stage
            .grid
            .slots()
            .next()
            .expect("the grid holds the stack")
            .1
            .stack();
        assert_eq!(
            carried,
            loot::drop_of(loot::DEFAULT_SEED, carried_foe(carried))
        );

        // …and a second reach with nothing left in range takes nothing rather
        // than the same stack again.
        let after = stage.grid.len();
        run_tick(&mut stage, REACH, DT);
        assert_eq!(stage.grid.len(), after, "the same stack was taken twice");
    }

    /// Which foe minted `stack`, which every stack in this zone has.
    fn carried_foe(stack: Stack) -> usize {
        loot::foe_of(stack.id(), foe::FOES).expect("every stack names a foe")
    }

    // ---- the level verb ---------------------------------------------------

    /// **Felling a foe teaches the character, and taking what it left teaches
    /// more.** The two places experience is granted, and the only two — a build
    /// that granted it per tick, per swing or per frame would report a total
    /// that is not the sum of the archetypes that fell.
    ///
    /// The control is the run before the kill: `experience` is zero for every
    /// tick of the walk up the corridor, so this is a grant rather than a
    /// counter that was always climbing.
    #[test]
    fn felling_a_foe_teaches_the_character_and_taking_what_it_left_teaches_more() {
        let mut stage = ready();
        hold(&mut stage, AHEAD, 1.0);
        assert_eq!(stage.experience, 0, "the zone opened part-levelled");
        assert_eq!(level::level_for(stage.experience), 1);

        until_something_falls(&mut stage);
        // Summed over whatever fell, because one cleave answers everything in
        // reach and two bodies can go down on one tick.
        let fell: u64 = stage
            .foes
            .iter()
            .filter(|foe| !foe.is_alive())
            .map(|foe| u64::from(foe.kind().experience()))
            .sum();
        assert!(fell > 0, "nothing fell, so nothing was owed");
        assert_eq!(
            stage.experience,
            fell,
            "{} foe(s) fell, worth {fell}, and the character learned {}",
            felled(&stage),
            stage.experience,
        );

        // …and the find on top of it, worth what its tier is worth.
        assert!(!stage.floor.is_empty(), "a felled foe left nothing");
        let tier = loot::rarity_of(stage.seed, stage.floor[0].foe);
        let before = stage.experience;
        let carried = stage.grid.len();
        run_tick(&mut stage, REACH, DT);
        assert_eq!(stage.grid.len(), carried + 1, "the pickup took nothing");
        assert_eq!(
            stage.experience,
            before + u64::from(tier.experience()),
            "a {} find taught {} rather than the {} its tier is worth",
            tier.label(),
            stage.experience - before,
            tier.experience(),
        );
    }

    /// **A level turns exactly on the table's row, and it widens the pool
    /// without healing the wound; the down is what pours it.**
    ///
    /// Three claims and each is the other's control. One experience short of
    /// the row is still the level below — a build comparing with `>` would fail
    /// that and pass everything else. The health held across the raise is what
    /// says a level is a ceiling rather than a heal, which a build that refilled
    /// on level-up would fail. And the refill after the down has to be the
    /// **new** pool, which a build that kept `foe::HEALTH_MAX` as the refill
    /// would fail while passing both of the others.
    #[test]
    fn a_level_turns_on_the_tables_row_and_widens_the_pool_without_healing() {
        let mut stage = ready();
        let row = level::THRESHOLDS[1];
        let short = u32::try_from(row - 1).expect("the first row fits one grant");

        stage.health = 12;
        stage.gain(short);
        assert_eq!(
            level::level_for(stage.experience),
            1,
            "the level turned one experience early",
        );
        assert_eq!(stage.health_max(), foe::HEALTH_MAX);

        stage.gain(1);
        assert_eq!(
            level::level_for(stage.experience),
            2,
            "{row} experience did not turn the level the table says it does",
        );
        assert_eq!(stage.health, 12, "the level healed a wound");
        assert_eq!(
            stage.health_max(),
            foe::HEALTH_MAX + level::HEALTH_PER_LEVEL,
            "the level did not widen the pool",
        );

        // …and the pool a down pours is the one the level allows, not the one
        // the character opened the zone with.
        stage.health = 0;
        run_tick(&mut stage, Intent::default(), DT);
        assert_eq!(stage.downs, 1, "running out did not put them down");
        assert_eq!(
            stage.health,
            stage.health_max(),
            "the down refilled the pool the character started with",
        );
        assert!(stage.health > foe::HEALTH_MAX);
    }

    /// **A grid with no room leaves the stack where it fell.** The refusal path,
    /// and the one that would duplicate an item if the pickup took it off the
    /// floor first and then found nowhere to put it.
    #[test]
    fn a_full_grid_leaves_the_stack_on_the_floor() {
        let mut stage = ready();
        until_something_falls(&mut stage);
        let down = stage.floor.len();
        assert!(down > 0);

        // Filled with something that is not this zone's, so the ids cannot be
        // confused with a drop's.
        let filler = loot::catalog()
            .id_of("bandage")
            .expect("the shipped table has a bandage in it");
        let mut spare = 0u32;
        while loot::stow(
            &mut stage.grid,
            Stack::new(filler, StackId(1_000 + spare), 1),
        )
        .is_ok()
        {
            spare += 1;
            assert!(spare < 1_000, "a 4x4 grid took a thousand items");
        }
        let full = stage.grid.len();

        run_tick(&mut stage, REACH, DT);
        assert_eq!(stage.floor.len(), down, "a refused pickup took the stack");
        assert_eq!(stage.grid.len(), full, "a full grid took one more");
        assert_eq!(stage.picked, 0, "a refused pickup counted as one");
    }

    /// **A drag moves an item between two cells, and a drag onto an occupied
    /// one puts it back.**
    ///
    /// The second is the control and it is the sharper claim: `Grid::move_within`
    /// is atomic, so a refused drag has to leave the item exactly where it was
    /// — a panel built on remove-then-place would pass the first assertion and
    /// leave the grid holding nothing after the second.
    #[test]
    fn a_drag_moves_an_item_between_cells_and_a_taken_cell_refuses_it() {
        use crcbl::inventory::Rotation;

        let bandage = loot::catalog().id_of("bandage").expect("a bandage");
        let mut grid = loot::carried();
        let first = grid
            .place(
                loot::catalog(),
                Stack::new(bandage, loot::stack_id(0), 1),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("an empty grid takes a 1x1");
        grid.place(
            loot::catalog(),
            Stack::new(bandage, loot::stack_id(1), 1),
            Cell::new(2, 2),
            Rotation::Deg0,
        )
        .expect("and a second one, elsewhere");

        let mut stage = ready();
        stage.grid = grid;
        let carrying = stage.snapshot();
        let mut game = Game::new(DEFAULT_TICK_HZ, loot::DEFAULT_SEED, Some(carrying))
            .expect("the loopback comes up");

        assert!(
            game.drag(first, Cell::new(1, 0)),
            "a drag onto an empty cell was refused",
        );
        let moved = game.grid();
        assert!(moved.at(Cell::new(0, 0)).is_none(), "it did not leave");
        assert!(moved.at(Cell::new(1, 0)).is_some(), "it did not arrive");

        assert!(
            !game.drag(first, Cell::new(2, 2)),
            "a drag onto a taken cell was accepted",
        );
        let back = game.grid();
        assert!(
            back.at(Cell::new(1, 0)).is_some(),
            "a refused drag lost the item it was carrying",
        );
        assert_eq!(back.len(), 2, "a refused drag changed what the grid holds");
        assert_eq!(
            back.slot(back.at(Cell::new(2, 2)).expect("the other item"))
                .expect("its placement")
                .stack()
                .id(),
            loot::stack_id(1),
            "the drag overwrote what it was dropped onto",
        );
    }

    /// **A resumed session carries what the last one picked up, and what it
    /// left on the floor is still there.**
    ///
    /// The floor is derived rather than saved — see `Stage::restore_floor` — so
    /// this is the check that the derivation and the save agree: one felled foe
    /// looted, one felled foe not, and the session that comes back holds
    /// exactly one of each with the same [`crcbl::inventory::StackId`]s.
    #[test]
    fn a_session_that_looted_comes_back_carrying_it() {
        let mut played = ready();
        // Two foes down by hand, so the state is exact rather than whatever a
        // charge produced, and one of the two drops taken.
        for index in [0, 1] {
            played.foes[index].wounded(&mut played.world, foe::HEALTH_MAX);
            let at = played.foes[index].feet();
            played.drop_loot(index, at);
        }
        assert_eq!(played.floor.len(), 2);
        let taken = played.floor[0].stack;
        loot::stow(&mut played.grid, taken).expect("an empty grid takes the first drop");
        played.floor.remove(0);

        let saved = played.snapshot();
        let mut restored = Stage::new(loot::DEFAULT_SEED);
        restored.restore(&saved);

        assert_eq!(
            restored.grid.len(),
            1,
            "the resumed character is not carrying what they took",
        );
        assert_eq!(
            restored
                .grid
                .slots()
                .next()
                .expect("the one stack")
                .1
                .stack(),
            taken,
            "the stack came back as a different instance",
        );
        assert_eq!(
            restored.floor.len(),
            1,
            "what was left lying is not on the floor of the resumed zone",
        );
        assert_eq!(
            restored.floor[0].stack, played.floor[0].stack,
            "the floor came back holding something else",
        );
        assert_eq!(
            restored.floor.len() + restored.grid.len(),
            felled(&restored),
            "the resumed zone holds a different number of instances",
        );
    }
}
