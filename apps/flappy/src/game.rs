//! Flappy's simulation: the bird, its gravity, and the one button that acts on
//! it.
//!
//! # The bird is a projectile, and that is the whole difference
//!
//! Breakout's ball has no gravity at all, because a ball that arcs cannot be
//! aimed. Flappy is the opposite game: the arc *is* the mechanic, and the only
//! control the player has is when to interrupt it. So the bird is a dynamic body
//! with a [`GravityForce`] provider — the same seam breakout deliberately does
//! not use — and a flap **replaces** its vertical velocity rather than adding to
//! it. That is what makes the control feel absolute: the height a flap reaches
//! does not depend on how fast the bird was already falling, so a player who
//! panics and flaps twice gets the same climb as one who flaps once.
//!
//! Forward motion is not a force. The bird advances at a constant
//! [`SCROLL_SPEED`], which nothing accelerates and nothing resists, so the
//! horizontal half of the simulation is exactly `x += SCROLL_SPEED * dt` and the
//! difficulty is a function of time rather than of anything the player did.
//!
//! # Where the simulation runs
//!
//! Inside the server's tick, in [`FlappyModule::tick`] — the hook `crcbl-ecs`
//! documents as running every server tick *after* the ECS schedule. Breakout's
//! module docs argue that placement at length and the argument is unchanged
//! here: `Server::update` may run zero, one or several ticks for one wall-clock
//! timestamp, so anything that has to happen once per *tick* cannot live beside
//! the call.
//!
//! [`Game`] is the client-side facade: it resolves input into an [`Intent`],
//! advances the server and the client by exactly one tick period, and reads back
//! what to draw.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl_client::Client;
use crcbl_core::FrameClock;
use crcbl_core::input::KeyCode;
use crcbl_ecs::{Entity, GameModule, World};
use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl_net::{InMemoryTransport, ProtocolCompatibility};
use crcbl_phys::{ColliderComponent, GravityForce, PhysicsSystem, RigidBody, Transform};
use crcbl_server::Server;
use glam::DVec3;

/// Distinct from breakout's, because they are distinct protocols: a client
/// built for one must not hand-shake with a server running the other.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0046_4C50_5059,
};

/// The default simulation rate. The value reaches the server, the client, the
/// ECS `tick_dt` and the integrator, so there is exactly one rate in the
/// process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// Downward acceleration on the bird, in world units per second squared.
///
/// Not Earth's 9.81: the world is measured in screen-fulls rather than metres,
/// and at 9.81 a flap floats for a second and a half, which reads as a balloon.
/// 26 puts the top of a flap's arc about 0.3 s after the button, which is the
/// pace the game is legible at.
pub const GRAVITY: f64 = 26.0;

/// The upward speed a flap sets, in world units per second.
///
/// Set, not added — see the module docs. Against [`GRAVITY`] it lifts the bird
/// about 1.9 units before it turns over, which is a little under half the gap
/// height, so one flap is a correction and two are a climb.
pub const FLAP_SPEED: f64 = 10.0;

/// How fast the bird advances, in world units per second. Constant for the whole
/// run: this game's difficulty comes from the pipes, not from a speed ramp.
pub const SCROLL_SPEED: f64 = 6.0;

/// The bird's collider radius.
pub const BIRD_RADIUS: f64 = 0.35;

/// Where a run starts. Not the origin: the bird sits a little way in from the
/// left of the camera's view, so there is something to see ahead of it.
pub const BIRD_START: DVec3 = DVec3::new(0.0, 0.0, 0.0);

/// The top of the playable band.
///
/// A bird that reaches it is stopped, not killed. Killing on the ceiling makes
/// the safest answer to a low gap — climb early and wait — the one that loses
/// the run, which reads as the game cheating. The floor is the opposite and
/// does kill.
pub const WORLD_CEILING: f64 = 6.0;

/// The bottom of the playable band.
pub const WORLD_FLOOR: f64 = -6.0;

/// The bird's collider, named once so a later slice's sweep can lift it out of
/// the world and put the identical shape back.
const BIRD_COLLIDER: ColliderComponent = ColliderComponent::Sphere {
    offset: DVec3::ZERO,
    radius: BIRD_RADIUS,
    is_trigger: false,
};

/// Where the first pipe stands. Far enough ahead that the course is visible
/// before the player commits to the first flap.
pub const FIRST_PIPE_X: f64 = 12.0;

/// Distance between one pipe and the next.
///
/// At [`SCROLL_SPEED`] that is 1.5 s between gaps, which is about four flaps —
/// enough to correct a bad line, not enough to coast.
pub const PIPE_SPACING: f64 = 9.0;

/// Half the width of a pipe.
pub const PIPE_HALF_WIDTH: f64 = 0.8;

/// Half the height of the hole through a pipe.
///
/// 1.7 makes the gap 3.4 units against a bird 0.7 across, so the opening is
/// nearly five bird-widths. Tight enough to need aiming, wide enough that the
/// first-time visitor the demo site sends here clears one.
pub const GAP_HALF_HEIGHT: f64 = 1.7;

/// How far from the middle of the band a gap's centre may be placed.
///
/// Bounded rather than free so that every gap is *reachable*: a gap flush with
/// the ceiling cannot be flown into, because the lid stops the climb before the
/// bird is high enough. `WORLD_CEILING - GAP_HALF_HEIGHT` would be the flush
/// case, so this leaves a bird's height of clearance on each side.
pub const GAP_CENTRE_RANGE: f64 = WORLD_CEILING - GAP_HALF_HEIGHT - 1.0;

/// How far ahead of the bird pipes are built.
///
/// Comfortably past the edge of any camera the sample will use, so a pipe is
/// never seen appearing.
const SPAWN_AHEAD: f64 = 34.0;

/// How far behind the bird a pipe survives before it is destroyed.
const CULL_BEHIND: f64 = 10.0;

/// The x of pipe `index`.
#[must_use]
pub fn pipe_x(index: u32) -> f64 {
    FIRST_PIPE_X + f64::from(index) * PIPE_SPACING
}

/// The centre of the gap through pipe `index` of the course `seed` describes.
///
/// **A pure function, not a running generator.** The obvious way to place gaps
/// is to keep an RNG in the simulation and pull the next value each time a pipe
/// is spawned, and it is the wrong way here: the value would then depend on how
/// many pipes had been spawned so far, which depends on where the bird got to,
/// which is exactly the sort of hidden state that makes two runs of the same
/// script diverge. Hashing the index instead means pipe 40 is in the same place
/// whether it was the fortieth spawned or the first — and it is why the client
/// and the server agree about the course without a byte of it being sent.
///
/// splitmix64's finaliser over `seed` and `index`, taken as 53 bits of mantissa.
#[must_use]
pub fn gap_centre(seed: u64, index: u32) -> f64 {
    let mut z = seed.wrapping_add(u64::from(index).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // The top 53 bits are the ones splitmix64 mixes best, and 53 is exactly an
    // f64's mantissa, so this is a uniform value in [0, 1) with nothing thrown
    // away twice.
    let unit = (z >> 11) as f64 / (1u64 << 53) as f64;
    (unit * 2.0 - 1.0) * GAP_CENTRE_RANGE
}

/// The seed the `runs`-th course of a game seeded with `seed` is laid out from.
///
/// A restart changes the course, because a game that dealt the same hand every
/// time would be memorised rather than played. It changes it *deterministically*
/// — the run counter is simulation state like any other — so a recorded script
/// replayed from a fresh game still meets the same pipes in the same places.
#[must_use]
fn course_seed(seed: u64, runs: u32) -> u64 {
    seed ^ u64::from(runs).wrapping_mul(0xD1B5_4A32_D192_ED03)
}

const ACTION_FLAP: &str = "flap";
const ACTION_RESTART: &str = "restart";

/// Where a run is.
///
/// Three states, against breakout's four, and no lives counter: this game is
/// lost the first time the bird touches anything, which is the shape the sample
/// exists to be different in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// The bird is held at the start. The first flap begins the run.
    WaitingToStart,
    /// Running.
    Playing,
    /// Over. A flap or a restart starts a new run.
    Dead,
}

// ---------------------------------------------------------------------------
// Intent — what the player asked for this tick
// ---------------------------------------------------------------------------

/// One tick of player intent.
///
/// **One button.** Breakout's intent has two axis flags and two edges; this has
/// one edge that means three different things depending on the state, which is
/// the whole of flappy's input design.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    /// The button, on the tick it went down. Never a held flag: a bird that
    /// climbed while the key was down would need no timing at all.
    flap: bool,
    /// Restart unconditionally, whatever the state.
    restart: bool,
}

impl Intent {
    /// The wire form handed to `Client::set_input`.
    fn to_wire(self) -> u8 {
        u8::from(self.flap) | (u8::from(self.restart) << 1)
    }
}

// ---------------------------------------------------------------------------
// Shared logic — owned jointly by the facade and the server-side module
// ---------------------------------------------------------------------------

/// One pipe: two boxes with a hole between them.
///
/// Two entities rather than one, because a collider is per entity and a pipe is
/// not a convex shape. The pair is created and destroyed together and nothing
/// else ever refers to one half.
#[derive(Clone, Copy, Debug)]
struct Pipe {
    index: u32,
    gap_centre: f64,
    top: Entity,
    bottom: Entity,
}

/// What the renderer needs to draw one pipe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipeView {
    pub x: f64,
    pub gap_centre: f64,
}

/// The mutable game state the server-side module owns.
#[derive(Debug)]
struct GameLogic {
    bird: Entity,
    intent: Intent,
    /// Refreshed each tick for the renderer, so the draw path never reaches
    /// into the physics world.
    bird_pos: DVec3,
    bird_vel: DVec3,
    state: GameState,
    /// Pipes flown past this run.
    score: u32,
    /// The index of the next pipe that can be scored. See
    /// [`score_passed_pipes`].
    next_score: u32,
    /// What ended the run, once one has.
    death: Option<Death>,
    /// The pipes that currently exist, oldest first.
    pipes: Vec<Pipe>,
    /// The next pipe index to build. Only ever goes up within a run, so the
    /// treadmill cannot build the same pipe twice.
    next_index: u32,
    /// The seed the whole game was started with. The course actually in play is
    /// [`course_seed`] of this and [`Self::runs`].
    seed: u64,
    /// How many runs have been started. Part of the simulation, so a replay
    /// meets the same courses in the same order.
    runs: u32,
    /// Live pipe positions for the renderer, reused rather than rebuilt so a
    /// steady-state tick does not allocate.
    pipe_views: Vec<PipeView>,
    /// Cues raised this tick: `(sound id, world x, world y)`. Drained by the
    /// facade, which owns the output stream — audio does not belong on the
    /// simulation's side of the seam, and a frame that ran two ticks must play
    /// both of their cues rather than only the last one's.
    cues: Vec<(u32, f64, f64)>,
    /// Ticks the module has actually run. The facade asserts this advances by
    /// exactly one per [`Game::tick`].
    ticks: u64,
}

impl GameLogic {
    fn reset_run(&mut self) {
        self.state = GameState::WaitingToStart;
        self.score = 0;
        self.next_score = 0;
        self.death = None;
    }

    /// The seed of the course in play.
    fn course(&self) -> u64 {
        course_seed(self.seed, self.runs)
    }
}

/// Per-tick game logic, run by the server after the ECS physics schedule.
///
/// `register` is empty for the same reason breakout's is: `Server::set_module`
/// does not call it, and the physics system is registered on the world in
/// [`Game::new`] before the server is built.
struct FlappyModule {
    shared: Arc<Mutex<GameLogic>>,
}

impl std::fmt::Debug for FlappyModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlappyModule").finish_non_exhaustive()
    }
}

impl GameModule for FlappyModule {
    fn name(&self) -> &str {
        "flappy"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World) {
        let mut logic = lock(&self.shared);
        run_tick(&mut logic, world);
    }
}

/// A poisoned mutex here means a previous tick panicked. The game state is plain
/// data with no invariant a panic could have half-broken, so recovering the
/// guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<GameLogic>) -> MutexGuard<'_, GameLogic> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// One tick of flappy, inside the server's tick, after physics has stepped.
fn run_tick(logic: &mut GameLogic, world: &mut World) {
    logic.ticks += 1;
    logic.cues.clear();
    let dt = world.tick_dt();
    let intent = std::mem::take(&mut logic.intent);
    let bird = logic.bird;

    // --- what the button meant this tick --------------------------------
    match logic.state {
        _ if intent.restart => restart(logic, world),
        GameState::Dead if intent.flap => restart(logic, world),
        GameState::WaitingToStart if intent.flap => {
            logic.state = GameState::Playing;
            with_physics(world, |phys| {
                // Dynamic again: a run that has ended leaves the bird
                // kinematic so that gravity stops acting on the corpse.
                phys.set_body(bird, RigidBody::new_dynamic(1.0));
                set_velocity(phys, bird, DVec3::new(SCROLL_SPEED, FLAP_SPEED, 0.0));
            });
            flap_cue(logic);
        }
        GameState::Playing if intent.flap => {
            with_physics(world, |phys| {
                // Replaces the vertical velocity; the forward half is untouched
                // because nothing in this game may change it.
                if let Some(body) = phys.body(bird) {
                    let mut flapped = *body;
                    flapped.velocity.y = FLAP_SPEED;
                    phys.set_body(bird, flapped);
                }
            });
            flap_cue(logic);
        }
        _ => {}
    }

    // --- hold the bird still until the run starts ------------------------
    //
    // `WaitingToStart` only. A dead bird stays where it died, because where it
    // died is the one piece of feedback the player gets about what went wrong.
    if logic.state == GameState::WaitingToStart {
        with_physics(world, |phys| park_bird(phys, bird));
    }

    // --- the ceiling is a lid, not a hazard ------------------------------
    //
    // A bird flown into the top of the world stops there rather than dying.
    // Killing on the ceiling makes the safest response to a low pipe — climb
    // early, wait — the one that loses the run, which reads as the game
    // cheating. The floor is the opposite and kills, but that belongs with the
    // rest of the collision handling.
    if logic.state == GameState::Playing {
        with_physics(world, |phys| {
            let Some(transform) = phys.transform(bird).copied() else {
                return;
            };
            if transform.position.y > WORLD_CEILING {
                let mut capped = transform;
                capped.position.y = WORLD_CEILING;
                phys.set_transform(bird, capped);
                if let Some(body) = phys.body(bird) {
                    let mut stopped = *body;
                    stopped.velocity.y = stopped.velocity.y.min(0.0);
                    phys.set_body(bird, stopped);
                }
            }
        });
    }

    read_bird(logic, world);

    if logic.state == GameState::Playing {
        // Collision first, so a tick that ends the run awards nothing: a bird
        // that clipped the far edge of a pipe on its way past does not get the
        // point for it.
        if let Some(cause) = fatal(logic, world, dt) {
            die(logic, world, cause);
        } else {
            score_passed_pipes(logic);
        }
    }

    advance_course(logic, world);
    refresh_render_state(logic, world);
}

/// Why a run ended. Kept apart because the two want different feedback and,
/// later, different sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Death {
    /// Flown into a pipe.
    Pipe,
    /// Flown into the ground.
    Ground,
}

/// Whether the bird's path over the tick that just ran ended the run.
///
/// The pipe half is a swept sphere over the path rather than a test of where
/// the bird ended up — the same `crcbl-phys` CCD query breakout's ball uses.
///
/// **Honestly: this game cannot tell the difference.** At [`SCROLL_SPEED`] the
/// bird advances a tenth of a unit a tick against a pipe 1.6 wide, so a point
/// test at the end of the tick passes every test in this file, and it still
/// does at a tick rate as coarse as 3 Hz. The sweep is here because it is the
/// correct query and because the cost is nil, not because anything demonstrates
/// it; `docs/backlog.md` records that as the coverage gap it is rather than
/// leaving a comment claiming a guarantee no check enforces.
fn fatal(logic: &GameLogic, world: &mut World, dt: f64) -> Option<Death> {
    let bird = logic.bird;
    if logic.bird_pos.y - BIRD_RADIUS <= WORLD_FLOOR {
        return Some(Death::Ground);
    }

    let hit = with_physics(world, |phys| {
        let (body, transform) = phys
            .body(bird)
            .copied()
            .zip(phys.transform(bird).copied())?;
        let segment = crcbl_phys::Segment {
            start: transform.position - body.velocity * dt,
            end: transform.position,
        };
        // The bird's own collider sits at the far end of the segment, and the
        // sweep has no exclusion list — left in the world it reports the bird
        // hitting itself at t = 0 and every run ends on its first tick. Breakout
        // pays the same price for the same reason.
        phys.remove_collider(bird);
        let hit = phys.sweep_sphere(&segment, BIRD_RADIUS);
        phys.set_collider(bird, &BIRD_COLLIDER, &transform);
        hit.map(|_| Death::Pipe)
    });
    hit.flatten()
}

/// Raises the wing-beat where the bird is.
fn flap_cue(logic: &mut GameLogic) {
    logic
        .cues
        .push((crate::audio::SOUND_FLAP, logic.bird_pos.x, logic.bird_pos.y));
}

/// Ends the run where the bird is.
fn die(logic: &mut GameLogic, world: &mut World, cause: Death) {
    logic.state = GameState::Dead;
    logic.death = Some(cause);
    let bird = logic.bird;
    // **Kinematic, not merely stopped.** Zeroing the velocity leaves the body
    // dynamic, so the integrator puts gravity straight back into it and the
    // bird sinks a fraction of a unit every tick for as long as the game-over
    // screen is up — slowly enough to look like nothing, and far enough to end
    // up off the bottom of the world.
    with_physics(world, |phys| {
        phys.set_body(bird, RigidBody::new_kinematic())
    });
    logic.cues.push((
        crate::audio::SOUND_DEATH,
        logic.bird_pos.x,
        logic.bird_pos.y,
    ));
    log::info!(
        "run over after {} pipes ({cause:?}) at x = {:.1}",
        logic.score,
        logic.bird_pos.x
    );
}

/// Counts the pipes the bird has flown past since the last tick.
///
/// A running index rather than a flag per pipe: the bird only ever moves
/// forward, so the pipes are passed in order and "which is next" is one number.
/// It is also what makes the score survive the treadmill — a pipe is scored on
/// the way past, long before it is destroyed behind.
fn score_passed_pipes(logic: &mut GameLogic) {
    while logic.next_score < logic.next_index
        && pipe_x(logic.next_score) + PIPE_HALF_WIDTH < logic.bird_pos.x
    {
        logic.next_score += 1;
        logic.score += 1;
    }
}

/// Copies the bird's authoritative state out of the physics world.
fn read_bird(logic: &mut GameLogic, world: &mut World) {
    let bird = logic.bird;
    let state = with_physics(world, |phys| {
        let position = phys.transform(bird).map(|t| t.position);
        let velocity = phys.body(bird).map(|b| b.velocity);
        position.zip(velocity)
    })
    .flatten();
    if let Some((position, velocity)) = state {
        logic.bird_pos = position;
        logic.bird_vel = velocity;
    }
}

/// Builds the pipes that have come into range and destroys the ones that have
/// gone out of it behind the bird.
///
/// **The treadmill, and the only place entities are created or destroyed after
/// start-up.** Breakout spawns its whole world once and then only ever removes
/// from it; this runs forever, at a steady couple of entities a second, which is
/// what makes it the first legible test of slot recycling under churn.
fn advance_course(logic: &mut GameLogic, world: &mut World) {
    let bird_x = logic.bird_pos.x;
    let seed = logic.course();

    // Build forward. `next_index` never rewinds inside a run, so a pipe cannot
    // be built twice however the bird moves.
    while pipe_x(logic.next_index) <= bird_x + SPAWN_AHEAD {
        let pipe = spawn_pipe(world, seed, logic.next_index);
        logic.pipes.push(pipe);
        logic.next_index += 1;
    }

    // Cull behind. Retained in place rather than filtered into a new vector: the
    // list is short and ordered, and the whole point of this function is that a
    // long run does not allocate.
    let cutoff = bird_x - CULL_BEHIND;
    let mut removed = Vec::new();
    logic.pipes.retain(|pipe| {
        let alive = pipe_x(pipe.index) >= cutoff;
        if !alive {
            removed.push(*pipe);
        }
        alive
    });
    for pipe in removed {
        despawn_pipe(world, pipe);
    }
}

/// Puts the run back to its opening position, on a course that is not the one
/// just played.
fn restart(logic: &mut GameLogic, world: &mut World) {
    for pipe in std::mem::take(&mut logic.pipes) {
        despawn_pipe(world, pipe);
    }
    logic.next_index = 0;
    logic.runs = logic.runs.wrapping_add(1);
    logic.reset_run();
    let bird = logic.bird;
    with_physics(world, |phys| park_bird(phys, bird));
    // From the parked position, so the opening course is the same whatever the
    // bird was doing when the run ended.
    logic.bird_pos = BIRD_START;
    advance_course(logic, world);
}

/// Copies the authoritative state the renderer needs out of the physics world.
fn refresh_render_state(logic: &mut GameLogic, world: &mut World) {
    read_bird(logic, world);
    logic.pipe_views.clear();
    logic
        .pipe_views
        .extend(logic.pipes.iter().map(|pipe| PipeView {
            x: pipe_x(pipe.index),
            gap_centre: pipe.gap_centre,
        }));
}

// ---------------------------------------------------------------------------
// Physics helpers
// ---------------------------------------------------------------------------

/// Runs `f` against the world's physics system, if it has one.
fn with_physics<R>(world: &mut World, f: impl FnOnce(&mut PhysicsSystem) -> R) -> Option<R> {
    for sys in world.schedule_mut().iter_mut() {
        if sys.name() == "physics"
            && let Some(phys) = sys.as_any_mut().downcast_mut::<PhysicsSystem>()
        {
            return Some(f(phys));
        }
    }
    None
}

/// Builds the two boxes of pipe `index` and returns the pair.
///
/// The halves reach a long way past the ceiling and the floor rather than
/// stopping at them: a bird that clears the lid must still not be able to fly
/// over a pipe, and a box that ended at [`WORLD_CEILING`] would let it.
fn spawn_pipe(world: &mut World, seed: u64, index: u32) -> Pipe {
    let centre = gap_centre(seed, index);
    let x = pipe_x(index);
    let far = 20.0;

    let top_bottom = centre + GAP_HALF_HEIGHT;
    let bottom_top = centre - GAP_HALF_HEIGHT;
    let top = spawn_static_box(
        world,
        DVec3::new(x, (top_bottom + WORLD_CEILING + far) / 2.0, 0.0),
        DVec3::new(
            PIPE_HALF_WIDTH,
            (WORLD_CEILING + far - top_bottom) / 2.0,
            1.0,
        ),
    );
    let bottom = spawn_static_box(
        world,
        DVec3::new(x, (bottom_top + WORLD_FLOOR - far) / 2.0, 0.0),
        DVec3::new(
            PIPE_HALF_WIDTH,
            (bottom_top - (WORLD_FLOOR - far)) / 2.0,
            1.0,
        ),
    );

    Pipe {
        index,
        gap_centre: centre,
        top,
        bottom,
    }
}

/// Destroys both halves of a pipe, in the physics world and in the ECS.
///
/// Both, and in that order. A collider left behind when the entity goes is a
/// wall the player cannot see, which is the failure mode this treadmill would
/// produce a hundred times a minute.
fn despawn_pipe(world: &mut World, pipe: Pipe) {
    with_physics(world, |phys| {
        phys.remove_entity(pipe.top);
        phys.remove_entity(pipe.bottom);
    });
    world.despawn(pipe.top);
    world.despawn(pipe.bottom);
}

/// Spawns a kinematic box collider at `centre`.
fn spawn_static_box(world: &mut World, centre: DVec3, half_extents: DVec3) -> Entity {
    let entity = world.spawn();
    let transform = Transform::from_position(centre);
    with_physics(world, |phys| {
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_transform(entity, transform);
        phys.set_collider(
            entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents,
                is_trigger: false,
            },
            &transform,
        );
    });
    entity
}

/// Returns the bird to the start, stationary, and re-seats its collider there.
fn park_bird(phys: &mut PhysicsSystem, bird: Entity) {
    let start = Transform::from_position(BIRD_START);
    phys.set_transform(bird, start);
    set_velocity(phys, bird, DVec3::ZERO);
    phys.set_collider(bird, &BIRD_COLLIDER, &start);
}

fn set_velocity(phys: &mut PhysicsSystem, entity: Entity, velocity: DVec3) {
    if let Some(body) = phys.body(entity) {
        let mut new_body = *body;
        new_body.velocity = velocity;
        phys.set_body(entity, new_body);
    }
}

// ---------------------------------------------------------------------------
// Game — the client-side facade
// ---------------------------------------------------------------------------

/// The course every game is laid out from unless a caller picks another.
///
/// A constant rather than "whatever the clock said", so the demo everyone plays
/// is the demo the tests describe, and a bug report can name a pipe.
pub const DEFAULT_SEED: u64 = 0x666C_6170_7079_0001;

/// Everything the renderer needs for one frame, in world space.
///
/// Filled through [`Game::render_state`], which reuses the caller's `pipes`
/// allocation — the treadmill hands over a fresh list every frame forever, and
/// a per-frame `Vec` for it is the sort of thing that only shows up in a
/// profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    pub bird: DVec3,
    pub bird_velocity: DVec3,
    pub pipes: Vec<PipeView>,
    pub score: u32,
    pub best: u32,
    pub state: Option<GameState>,
    pub death: Option<Death>,
}

pub struct Game {
    pub bird_entity: Entity,
    action_map: ActionMap,
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    shared: Arc<Mutex<GameLogic>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// Queued key events from the shell pump, replayed after `begin_tick`.
    pending_keys: Vec<(KeyCode, bool)>,
    pub audio: crate::audio::Audio,
    pub best: crate::best::Best,
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub state: GameState,
    pub score: u32,
    pub death: Option<Death>,
    pub bird: DVec3,
    pub bird_velocity: DVec3,
    prev_log_state: GameState,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("bird_entity", &self.bird_entity)
            .field("state", &self.state)
            .field("bird", &self.bird)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum GameError {
    Server(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(msg) => write!(f, "server creation failed: {msg}"),
        }
    }
}

impl std::error::Error for GameError {}

impl Game {
    /// Builds the world, the physics system, the server and the client.
    ///
    /// `tick_hz` is the one simulation rate in the process.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(headless: bool, tick_hz: u32) -> Result<Self, GameError> {
        Self::with_seed(headless, tick_hz, DEFAULT_SEED)
    }

    /// The same, on a named course.
    ///
    /// `seed` decides where every gap in every run of this game will be — see
    /// [`gap_centre`] — so two games built with the same seed and fed the same
    /// input are the same game, which is what the determinism tests rest on.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn with_seed(headless: bool, tick_hz: u32, seed: u64) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let mut world = World::new();

        // The force provider breakout deliberately has none of. Flappy's whole
        // mechanic is the arc between one flap and the next, so gravity is not
        // decoration here — it is the opponent.
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::new(DVec3::NEG_Y * GRAVITY)));
        world.register_system(Box::new(phys));

        let bird_entity = world.spawn();
        let start = Transform::from_position(BIRD_START);
        with_physics(&mut world, |phys| {
            phys.set_body(bird_entity, RigidBody::new_dynamic(1.0));
            phys.set_transform(bird_entity, start);
            phys.set_collider(bird_entity, &BIRD_COLLIDER, &start);
        });

        let mut action_map = ActionMap::new();
        action_map.declare(ActionDecl {
            name: ACTION_FLAP.into(),
            kind: ActionKind::Button,
            // Two keys for one action, because the browser demo is played with
            // whichever one the visitor tries first.
            bindings: vec![Binding::Key(KeyCode::Space), Binding::Key(KeyCode::ArrowUp)],
        });
        action_map.declare(ActionDecl {
            name: ACTION_RESTART.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::KeyR)],
        });

        let shared = Arc::new(Mutex::new(GameLogic {
            bird: bird_entity,
            intent: Intent::default(),
            bird_pos: BIRD_START,
            bird_vel: DVec3::ZERO,
            state: GameState::WaitingToStart,
            score: 0,
            next_score: 0,
            death: None,
            pipes: Vec::new(),
            next_index: 0,
            seed,
            runs: 0,
            pipe_views: Vec::new(),
            cues: Vec::new(),
            ticks: 0,
        }));

        // The opening stretch of course, before the first tick: the player is
        // looking at the game while deciding whether to press anything, and an
        // empty screen that fills in on the first flap reads as a bug.
        {
            let mut logic = lock(&shared);
            advance_course(&mut logic, &mut world);
        }

        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server =
            Server::try_new_with_compatibility(world, server_transport, tick_hz, COMPATIBILITY)
                .map_err(|e| GameError::Server(e.to_string()))?;
        server.set_module(Box::new(FlappyModule {
            shared: Arc::clone(&shared),
        }));

        let mut client =
            Client::new_with_compatibility(World::new(), client_transport, tick_hz, COMPATIBILITY);

        let tick_period = FrameClock::new(tick_hz).tick_dt();

        // Both clocks establish their baseline from the first `update`, which
        // therefore runs no ticks. Spending it here, at time zero, is what lets
        // `tick` promise that every later call runs exactly one.
        server.update(Duration::ZERO);
        client.update(Duration::ZERO);

        {
            let mut logic = lock(&shared);
            refresh_render_state(&mut logic, server.world_mut());
        }

        let game = Self {
            bird_entity,
            action_map,
            server,
            client,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            pending_keys: Vec::new(),
            audio: crate::audio::Audio::new(headless),
            best: crate::best::Best::load(headless),
            state: GameState::WaitingToStart,
            score: 0,
            death: None,
            bird: BIRD_START,
            bird_velocity: DVec3::ZERO,
            prev_log_state: GameState::WaitingToStart,
        };
        log::info!(
            "sim: {tick_hz} Hz, {:.3} ms per tick",
            game.tick_dt_secs() * 1e3,
        );
        Ok(game)
    }

    /// The fixed simulation step, in seconds.
    #[must_use]
    pub fn tick_dt_secs(&self) -> f64 {
        self.tick_period.as_secs_f64()
    }

    /// Queue a key event for replay at the start of the next tick.
    ///
    /// The shell pumps events once per **frame** while the action map's edge
    /// flags are per **tick**, and `ActionMap::begin_tick` resets those flags —
    /// so an event fed before `begin_tick` has its press edge erased by it.
    /// Queueing here and replaying after `begin_tick` is the order the action
    /// map asks for, and it is what makes a frame that runs no ticks lossless.
    ///
    /// It matters more here than it did in breakout. A paddle whose input
    /// arrives a tick late is a paddle that lags; a **flap** that arrives a tick
    /// late is a flap that did not happen, because there is nothing else in the
    /// input to smooth over it.
    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.pending_keys.push((key, pressed));
    }

    /// Advances the simulation by exactly one fixed tick.
    ///
    /// Call it from the loop's fixed-timestep accumulator — once per tick, not
    /// once per frame. Nothing in here reads a wall clock.
    pub fn tick(&mut self) {
        let dt = self.tick_period.as_secs_f64();
        self.action_map.begin_tick(dt as f32);
        for (key, pressed) in std::mem::take(&mut self.pending_keys) {
            self.action_map.key_event(key, pressed);
        }

        let intent = Intent {
            flap: action_just_pressed(&self.action_map, ACTION_FLAP),
            restart: action_just_pressed(&self.action_map, ACTION_RESTART),
        };

        let ticks_before = {
            let mut logic = lock(&self.shared);
            // `|=` rather than `=`: an edge raised on a frame that ran no ticks
            // must survive until a tick consumes it.
            logic.intent.flap |= intent.flap;
            logic.intent.restart |= intent.restart;
            logic.ticks
        };

        self.client.set_input(vec![intent.to_wire()]);

        self.sim_time += self.tick_period;
        let server_ticks = self.server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let _alpha = self.client.update(self.sim_time);
        self.ticks_run += 1;

        let (state, score, death, bird, velocity, ticks_after) = {
            let mut logic = lock(&self.shared);
            // Drained under the same lock the tick filled it under, so a frame
            // that ran two ticks plays both of their cues rather than the last
            // one's. `listener` is the camera's centre; see `crate::audio`.
            let cues: Vec<_> = logic.cues.drain(..).collect();
            drop(logic);
            for (id, x, y) in cues {
                let listener = self.bird.x;
                self.audio.play_at(id, listener, x, y);
            }
            let logic = lock(&self.shared);
            (
                logic.state,
                logic.score,
                logic.death,
                logic.bird_pos,
                logic.bird_vel,
                logic.ticks,
            )
        };
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        if state == GameState::Dead && self.state != GameState::Dead {
            self.best.update(score);
        }
        self.state = state;
        self.score = score;
        self.death = death;
        self.bird = bird;
        self.bird_velocity = velocity;

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        if state_changed || self.ticks_run.is_multiple_of(60) {
            log::info!(
                "[HUD] {:?}  score: {}  x: {:.1}  y: {:.1}  vy: {:+.1}",
                self.state,
                self.score,
                self.bird.x,
                self.bird.y,
                self.bird_velocity.y,
            );
        }
    }

    /// Everything the renderer draws, in world space.
    ///
    /// `out` is reused across frames so a steady-state frame does not allocate
    /// a fresh pipe list.
    pub fn render_state(&self, out: &mut RenderState) {
        let logic = lock(&self.shared);
        out.bird = logic.bird_pos;
        out.bird_velocity = logic.bird_vel;
        out.pipes.clear();
        out.pipes.extend_from_slice(&logic.pipe_views);
        out.score = logic.score;
        out.state = Some(logic.state);
        out.death = logic.death;
        drop(logic);
        out.best = self.best.get();
    }

    /// The course in play, for a caller that wants to name a pipe — a bug
    /// report, a replay header, or a test.
    #[must_use]
    pub fn course_seed(&self) -> u64 {
        lock(&self.shared).course()
    }

    /// The pipes that exist right now, nearest first.
    #[must_use]
    pub fn pipes(&self) -> Vec<PipeView> {
        lock(&self.shared).pipe_views.clone()
    }

    /// How many entities the simulation is holding.
    ///
    /// A number worth being able to see: this game creates and destroys
    /// entities forever, and one that climbs steadily is the whole failure mode
    /// the treadmill has to avoid.
    #[must_use]
    pub fn entity_count(&mut self) -> usize {
        self.server.world_mut().entity_count()
    }
}

// ---- input helpers ----------------------------------------------------------

fn action_just_pressed(map: &ActionMap, name: &str) -> bool {
    map.action(name).is_some_and(|v| match v {
        crcbl_input::ActionValue::Button(b) => b.just_pressed,
        _ => false,
    })
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::time::{ManualTime, TimeSource as _};

    /// One entry of a script: `(tick index, key, pressed)`.
    type Script = [(u64, KeyCode, bool)];

    /// Drives a `Game` the way the app loop will — a frame clock at `frame_hz`,
    /// a fixed-timestep accumulator at `tick_hz`, and events pumped once per
    /// frame — for a number of simulated seconds.
    ///
    /// The frame rate and the tick rate are independent knobs on purpose. Every
    /// property asserted below is a property of *simulated* time, and a loop
    /// that leaked the frame rate into the simulation is exactly what makes them
    /// disagree — which is the bug class this sample was chosen to make obvious.
    struct Harness {
        game: Game,
        clock: FrameClock,
        time: ManualTime,
        frame_step: Duration,
        ticks: u64,
    }

    impl Harness {
        fn new(frame_hz: u32, tick_hz: u32) -> Self {
            Self {
                game: Game::new(true, tick_hz).expect("a headless game always starts"),
                clock: FrameClock::new(tick_hz),
                time: ManualTime::new(),
                frame_step: FrameClock::new(frame_hz).tick_dt(),
                ticks: 0,
            }
        }

        /// One frame: advance the clock, drain whole ticks, exactly as the app
        /// loop does — stopping at `limit` so a caller counting ticks is not at
        /// the mercy of how many a single frame happened to release.
        ///
        /// The script is keyed on the **tick** index and fed immediately before
        /// that tick runs, so the input a given tick sees is the same at every
        /// frame rate. Keying it on frames instead is what would make these
        /// comparisons meaningless.
        fn frame(&mut self, script: &Script, limit: u64) {
            self.time.advance(self.frame_step);
            self.clock.update(self.time.elapsed());
            while self.ticks < limit && self.clock.consume_tick() {
                for &(at, key, pressed) in script {
                    if at == self.ticks {
                        self.game.key_event(key, pressed);
                    }
                }
                self.game.tick();
                self.ticks += 1;
            }
        }

        /// Runs frames until the simulation has run exactly `ticks` of them.
        ///
        /// Counted in ticks rather than seconds because that is the only unit
        /// two different frame rates agree on to the tick: a whole number of
        /// frames is a slightly different length of simulated time at 20 fps
        /// than at 240, so "two seconds" ends a tick or two apart and any
        /// exact comparison between the runs would be measuring the rounding.
        fn run_ticks(&mut self, ticks: u64, script: &Script) {
            while self.ticks < ticks {
                self.frame(script, ticks);
            }
        }

        /// Runs `ticks` of them under [`autopilot`] — a player who is trying.
        fn fly_ticks(&mut self, ticks: u64) {
            while self.ticks < ticks {
                self.time.advance(self.frame_step);
                self.clock.update(self.time.elapsed());
                while self.ticks < ticks && self.clock.consume_tick() {
                    if self.game.state == GameState::WaitingToStart || autopilot(&self.game) {
                        self.game.key_event(KeyCode::Space, true);
                        self.game.key_event(KeyCode::Space, false);
                    }
                    self.game.tick();
                    self.ticks += 1;
                }
            }
        }

        /// Runs for `seconds` of simulated wall time.
        fn run(&mut self, seconds: f64, script: &Script) {
            let frames = (seconds / self.frame_step.as_secs_f64()).round() as u64;
            for _ in 0..frames {
                self.frame(script, u64::MAX);
            }
        }
    }

    /// Press and release the flap key on `tick`.
    fn flap_at(tick: u64) -> [(u64, KeyCode, bool); 2] {
        [
            (tick, KeyCode::Space, true),
            (tick + 1, KeyCode::Space, false),
        ]
    }

    /// How far below the line into the next gap the autopilot lets itself sink
    /// before flapping.
    ///
    /// It has to be about the height a flap gains, or the controller ratchets:
    /// flap while still above the target and the next flap starts higher again,
    /// and after four of them the bird is riding the top of the gap instead of
    /// the middle of it. A whole unit of sag against a gap 1.7 either side
    /// leaves room for both the sag and the climb.
    const SAG: f64 = 1.0;

    /// How far ahead the autopilot looks, in seconds. Small: enough to notice
    /// that it is already falling fast, not enough to act on a fall that has
    /// not started.
    const LOOKAHEAD: f64 = 0.05;

    /// A competent player, in four lines: aim at the gap ahead, and flap when
    /// the bird has sagged a unit below the line into it.
    ///
    /// Deliberately a **decision per tick from simulation state**, not a
    /// recorded script. That is what makes it usable in the frame-rate tests:
    /// the state a given tick sees is the same at 20 fps as at 240, so the
    /// controller presses the button on the same ticks in both and the runs are
    /// comparable. A controller keyed on frames would not be.
    ///
    /// It is not a good player — it flies the middle of every gap and takes no
    /// account of the one after — but it clears more than thirty pipes, which
    /// is all any test here asks of it.
    fn autopilot(game: &Game) -> bool {
        let target = game
            .pipes()
            .iter()
            .find(|pipe| pipe.x + PIPE_HALF_WIDTH >= game.bird.x)
            .map_or(0.0, |pipe| pipe.gap_centre);
        game.bird.y + game.bird_velocity.y * LOOKAHEAD < target - SAG
    }

    /// The bird does not move until the player asks it to. Without this, a page
    /// that loads while the visitor is reading has already lost the run.
    #[test]
    fn a_bird_nobody_has_flapped_stays_exactly_where_it_started() {
        let mut harness = Harness::new(60, 60);
        harness.run(2.0, &[]);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(
            harness.game.bird, BIRD_START,
            "gravity must not act on a run that has not begun"
        );
    }

    /// The first flap starts the run and is itself a flap — not merely a
    /// "begin" that the player then has to follow with a real one.
    #[test]
    fn the_first_flap_starts_the_run_and_lifts_the_bird() {
        let mut harness = Harness::new(60, 60);
        harness.run(0.5, &flap_at(3));
        assert_eq!(harness.game.state, GameState::Playing);
        assert!(
            harness.game.bird.y > BIRD_START.y,
            "the flap that started the run must have lifted the bird: {:?}",
            harness.game.bird
        );
        assert!(
            harness.game.bird.x > BIRD_START.x,
            "and the run must be moving forward"
        );
    }

    /// A flap sets the climb rather than adding to it: the arc a flap starts is
    /// the same whether the bird was gliding or plummeting when it was pressed.
    ///
    /// Adding an impulse instead would make a late flap weaker exactly when the
    /// player needs it most, and a double flap twice as strong for no reason
    /// the player can see.
    #[test]
    fn a_flap_replaces_the_fall_instead_of_fighting_it() {
        // Two runs that arrive at the flap moving very differently: one has been
        // falling for four ticks, the other for forty.
        for fall in [30, 60] {
            let mut harness = Harness::new(60, 60);
            // The whole script, so the key is released again — a flap key still
            // held down raises no second press edge, and the test would be
            // measuring that instead of what it means to.
            harness.run_ticks(1 + fall, &flap_at(0));

            let falling = harness.game.bird_velocity.y;
            assert!(
                falling < 0.0,
                "the bird should be falling by now: {falling}"
            );

            harness.game.key_event(KeyCode::Space, true);
            harness.game.tick();
            assert!(
                (harness.game.bird_velocity.y - FLAP_SPEED).abs() < 1e-9,
                "a flap after {fall} ticks of falling at {falling} left the bird at {}, \
                 not {FLAP_SPEED} — an impulse was added rather than the climb set",
                harness.game.bird_velocity.y
            );
        }
    }

    /// The flap lands on the tick the key arrived, not the tick after.
    ///
    /// Breakout's paddle can hide a tick of input delay inside a continuous
    /// movement; a flap cannot, because it *is* the whole input. This is the
    /// assertion that would catch an event queue drained in the wrong order.
    #[test]
    fn a_flap_takes_effect_on_the_tick_its_key_arrived() {
        let mut harness = Harness::new(60, 60);
        // One tick to start the run, then land a second flap on a known tick
        // and stop the moment it has run.
        harness.run(0.1, &flap_at(0));
        let before = harness.game.bird_velocity.y;
        assert!(
            before < FLAP_SPEED,
            "the bird should be past its peak climb"
        );

        harness.game.key_event(KeyCode::Space, true);
        harness.game.tick();
        assert!(
            (harness.game.bird_velocity.y - FLAP_SPEED).abs() < 1e-9,
            "the tick that consumed the press must be the tick that flapped: {}",
            harness.game.bird_velocity.y
        );
    }

    /// Gravity is real: an un-flapped bird falls, and falls faster the longer it
    /// has been falling.
    #[test]
    fn an_un_flapped_bird_accelerates_downward() {
        let mut harness = Harness::new(60, 60);
        harness.run(0.05, &flap_at(0));

        let mut last_velocity = f64::INFINITY;
        for _ in 0..30 {
            harness.game.tick();
            let velocity = harness.game.bird_velocity.y;
            assert!(
                velocity < last_velocity,
                "a falling bird must keep accelerating: {velocity} came after {last_velocity}"
            );
            last_velocity = velocity;
        }
        assert!(
            harness.game.bird_velocity.y < -1.0,
            "half a second of gravity has to show: {}",
            harness.game.bird_velocity.y
        );
    }

    /// Forward motion is constant, and is not something gravity or a flap can
    /// touch. The whole difficulty curve rests on it.
    #[test]
    fn the_bird_advances_at_one_unchanging_pace() {
        let mut harness = Harness::new(60, 60);
        harness.run(0.05, &flap_at(0));

        let dt = harness.game.tick_dt_secs();
        for tick in 0..90 {
            let before = harness.game.bird.x;
            if tick == 30 {
                harness.game.key_event(KeyCode::Space, true);
            }
            harness.game.tick();
            let advanced = harness.game.bird.x - before;
            assert!(
                (advanced - SCROLL_SPEED * dt).abs() < 1e-9,
                "tick {tick} advanced {advanced}, not {}",
                SCROLL_SPEED * dt
            );
        }
    }

    /// **The frame rate is not the tick rate.** The same script at 20, 60 and
    /// 240 frames a second must reach the same place, because the simulation
    /// runs on its own fixed step and the frame loop only decides how often it
    /// is asked to.
    ///
    /// This is the assertion that caught three real bugs in breakout, and this
    /// sample was chosen partly because a per-frame integration bug would be
    /// blatant here: difficulty is a function of time, so a bird that fell per
    /// *frame* would be unplayable at 240 and trivial at 20.
    #[test]
    fn the_same_script_reaches_the_same_place_at_every_frame_rate() {
        let script = [
            (5, KeyCode::Space, true),
            (6, KeyCode::Space, false),
            (30, KeyCode::Space, true),
            (31, KeyCode::Space, false),
            (55, KeyCode::Space, true),
            (56, KeyCode::Space, false),
        ];

        let mut reference: Option<(DVec3, DVec3)> = None;
        for frame_hz in [20, 60, 240] {
            let mut harness = Harness::new(frame_hz, 60);
            harness.run_ticks(120, &script);
            assert_eq!(harness.ticks, 120);
            let observed = (harness.game.bird, harness.game.bird_velocity);
            match reference {
                None => {
                    assert!(
                        observed.0.y.abs() > 0.1,
                        "a run that went nowhere would compare equal for the wrong reason"
                    );
                    reference = Some(observed);
                }
                Some(expected) => assert_eq!(
                    observed, expected,
                    "{frame_hz} fps diverged from the reference run",
                ),
            }
        }
    }

    /// The ceiling stops the bird instead of killing it. A player who climbs
    /// early to clear a low gap is playing correctly, and a lid that killed
    /// would punish exactly that.
    #[test]
    fn the_ceiling_holds_the_bird_rather_than_ending_the_run() {
        let mut harness = Harness::new(60, 60);
        // Flap every few ticks for long enough to pin the bird against the top,
        // and stop before the first pipe — this is about the lid, and a run that
        // ended on a pipe would prove nothing either way.
        let mut script = Vec::new();
        for tick in (0..60).step_by(4) {
            script.push((tick, KeyCode::Space, true));
            script.push((tick + 1, KeyCode::Space, false));
        }
        harness.run_ticks(60, &script);
        assert!(
            harness.game.bird.x < FIRST_PIPE_X - PIPE_HALF_WIDTH,
            "the bird reached the first pipe: {}",
            harness.game.bird.x
        );

        assert_eq!(harness.game.state, GameState::Playing);
        assert!(
            harness.game.bird.y <= WORLD_CEILING + 1e-9,
            "the bird flew through the lid: {}",
            harness.game.bird.y
        );
        assert!(
            harness.game.bird.y > WORLD_CEILING - 1.0,
            "the bird should be held against the lid, not somewhere below it: {}",
            harness.game.bird.y
        );
    }

    // ---- dying and scoring ---------------------------------------------------

    /// A bird nobody flaps hits the ground, and that ends the run.
    #[test]
    fn hitting_the_ground_ends_the_run() {
        let mut harness = Harness::new(60, 60);
        harness.run_ticks(120, &flap_at(0));

        assert_eq!(harness.game.state, GameState::Dead);
        assert_eq!(harness.game.death, Some(Death::Ground));
        assert!(
            harness.game.bird.x < FIRST_PIPE_X,
            "this run should have ended before the first pipe, at x = {}",
            harness.game.bird.x
        );
    }

    /// Flying into a pipe ends the run, and says it was the pipe.
    ///
    /// The line flown is deliberately the naive one — hold the middle of the
    /// world and hope — so the run reaches a pipe under its own steam and dies
    /// on it rather than on the floor.
    #[test]
    fn flying_into_a_pipe_ends_the_run() {
        let mut harness = Harness::new(60, 60);
        // A player who keeps the bird level at y = 0 and takes no notice of
        // where the gaps are.
        for _ in 0..600 {
            if harness.game.state != GameState::Playing || harness.game.bird.y < 0.0 {
                harness.game.key_event(KeyCode::Space, true);
                harness.game.key_event(KeyCode::Space, false);
            }
            if harness.game.state == GameState::Dead && harness.ticks > 0 {
                break;
            }
            harness.run_ticks(harness.ticks + 1, &[]);
        }

        assert_eq!(harness.game.state, GameState::Dead);
        assert_eq!(
            harness.game.death,
            Some(Death::Pipe),
            "the run should have ended on a pipe, at x = {} y = {}",
            harness.game.bird.x,
            harness.game.bird.y
        );
        assert!(
            harness.game.bird.y - BIRD_RADIUS > WORLD_FLOOR,
            "and not on the floor: y = {}",
            harness.game.bird.y
        );
    }

    /// A dead bird stays where it died. Where it died is the only feedback the
    /// player gets about what went wrong.
    #[test]
    fn a_dead_bird_stays_where_it_died() {
        let mut harness = Harness::new(60, 60);
        harness.run_ticks(120, &flap_at(0));
        assert_eq!(harness.game.state, GameState::Dead);

        let resting = harness.game.bird;
        harness.run_ticks(180, &[]);
        assert_eq!(
            harness.game.bird, resting,
            "the bird kept moving after the run ended"
        );
        assert_eq!(harness.game.bird_velocity, DVec3::ZERO);
    }

    /// A pipe flown past is a point, and only one.
    #[test]
    fn every_pipe_flown_past_scores_exactly_once() {
        let mut harness = Harness::new(60, 60);
        harness.fly_ticks(600);

        assert_eq!(
            harness.game.state,
            GameState::Playing,
            "the run ended early"
        );
        // Every pipe whose trailing edge is behind the bird, and no others.
        let expected = (0..)
            .take_while(|&index| pipe_x(index) + PIPE_HALF_WIDTH < harness.game.bird.x)
            .count();
        assert!(expected >= 5, "only {expected} pipes were passed");
        assert_eq!(
            harness.game.score as usize, expected,
            "the score does not match the pipes actually behind the bird at x = {}",
            harness.game.bird.x
        );
    }

    /// The score never moves while the bird sits in front of a pipe it has not
    /// passed — the failure a per-tick "is a pipe near?" test would produce.
    #[test]
    fn the_score_does_not_move_while_a_pipe_is_still_ahead() {
        let mut harness = Harness::new(60, 60);
        harness.fly_ticks(100);
        assert!(
            harness.game.bird.x < FIRST_PIPE_X - PIPE_HALF_WIDTH,
            "the bird has already reached the first pipe: {}",
            harness.game.bird.x
        );
        assert_eq!(harness.game.score, 0);
    }

    /// A restart clears the score, and the next run starts from zero however
    /// far the last one got.
    #[test]
    fn a_restart_clears_the_score() {
        let mut harness = Harness::new(60, 60);
        harness.fly_ticks(400);
        assert!(harness.game.score > 0, "the run must score something first");

        harness.game.key_event(KeyCode::KeyR, true);
        harness.game.tick();

        assert_eq!(harness.game.score, 0);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.death, None);
    }

    /// **The determinism criterion.** The same script reaches the same score at
    /// every frame rate.
    ///
    /// The autopilot is the script here: it decides from simulation state, so a
    /// given tick sees the same world at 20 fps as at 240 and presses the button
    /// on the same ticks. A score that differed would mean the simulation had
    /// learned something about the frame loop.
    #[test]
    fn the_same_flight_reaches_the_same_score_at_every_frame_rate() {
        let mut reference: Option<(u32, DVec3, GameState)> = None;
        for frame_hz in [20, 60, 240] {
            let mut harness = Harness::new(frame_hz, 60);
            harness.fly_ticks(900);
            let observed = (harness.game.score, harness.game.bird, harness.game.state);
            match reference {
                None => {
                    assert!(observed.0 >= 5, "only {} pipes scored", observed.0);
                    reference = Some(observed);
                }
                Some(expected) => {
                    assert_eq!(observed, expected, "{frame_hz} fps flew a different run",)
                }
            }
        }
    }

    /// Two games given the same seed and the same flight agree about
    /// everything — the replay property, without a replay file.
    #[test]
    fn two_games_on_one_seed_fly_the_same_run() {
        let run = || {
            let mut harness = Harness::new(60, 60);
            harness.fly_ticks(900);
            (harness.game.score, harness.game.bird, harness.game.pipes())
        };
        let first = run();
        assert!(first.0 >= 5, "only {} pipes scored", first.0);
        assert_eq!(first, run());
    }

    // ---- cues and the best score ---------------------------------------------

    /// A flap is heard, and so is the end of a run.
    ///
    /// The cue queue is filled by the simulation and drained by the facade, so
    /// a cue that never crossed that seam would be a game that ran silently
    /// while every other test passed.
    #[test]
    fn a_flap_and_a_death_reach_the_audio() {
        let mut harness = Harness::new(60, 60);
        assert_eq!(harness.game.audio.voices(), 0, "silence before the run");

        harness.run_ticks(1, &flap_at(0));
        assert!(
            harness.game.audio.voices() >= 1,
            "the flap that started the run was not heard"
        );

        // Fall to the ground without flapping again.
        let before = harness.game.audio.voices();
        harness.run_ticks(120, &flap_at(0));
        assert_eq!(harness.game.state, GameState::Dead);
        assert!(
            harness.game.audio.voices() > before,
            "the end of the run was not heard"
        );
    }

    /// The best score is recorded when the run ends, once.
    #[test]
    fn the_best_score_is_taken_when_a_run_ends() {
        let mut harness = Harness::new(60, 60);
        assert_eq!(harness.game.best.get(), 0);

        harness.fly_ticks(500);
        let scored = harness.game.score;
        assert!(scored > 0, "the run has to score something first");
        assert_eq!(
            harness.game.best.get(),
            0,
            "a run still in progress is not a best score"
        );

        // Stop flying and hit the ground.
        harness.run_ticks(harness.ticks + 200, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert_eq!(
            harness.game.best.get(),
            scored,
            "the finished run was not recorded"
        );
    }

    // ---- the course ---------------------------------------------------------

    /// Two games told the same seed lay out the same course, pipe for pipe.
    ///
    /// The property everything else about the pipes rests on: the client and
    /// the server agree about where the gaps are without a byte of the course
    /// being sent, and a replay meets the pipes the recording met.
    #[test]
    fn the_same_seed_lays_out_the_same_course() {
        for seed in [0, 1, DEFAULT_SEED, u64::MAX] {
            let first: Vec<f64> = (0..500).map(|i| gap_centre(seed, i)).collect();
            let second: Vec<f64> = (0..500).map(|i| gap_centre(seed, i)).collect();
            assert_eq!(first, second, "seed {seed} is not a function of its index");
        }
    }

    /// Different seeds are different courses. Without this the seed would be
    /// decoration and every game would be the same game.
    #[test]
    fn different_seeds_lay_out_different_courses() {
        let a: Vec<f64> = (0..100).map(|i| gap_centre(1, i)).collect();
        let b: Vec<f64> = (0..100).map(|i| gap_centre(2, i)).collect();
        let shared = a.iter().zip(&b).filter(|(x, y)| x == y).count();
        assert!(
            shared < 5,
            "{shared} of 100 gaps coincided between two seeds"
        );
    }

    /// **Every gap can be flown into.** The ceiling stops a climb, so a gap
    /// placed flush against it is one the bird can never reach — a course that
    /// is not merely hard but impossible, and one no amount of playtesting
    /// would find if it were the four-hundredth pipe.
    #[test]
    fn every_gap_the_generator_can_produce_is_reachable() {
        for seed in [0, 7, DEFAULT_SEED, u64::MAX] {
            for index in 0..2000 {
                let centre = gap_centre(seed, index);
                assert!(
                    centre + GAP_HALF_HEIGHT < WORLD_CEILING,
                    "seed {seed} pipe {index}: the gap runs into the lid at {centre}"
                );
                assert!(
                    centre - GAP_HALF_HEIGHT > WORLD_FLOOR,
                    "seed {seed} pipe {index}: the gap runs into the floor at {centre}"
                );
            }
        }
    }

    /// The generator uses the whole band it is allowed rather than clustering in
    /// the middle. A course of near-identical gaps is one flap held down.
    #[test]
    fn the_generator_uses_the_whole_band() {
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for index in 0..2000 {
            let centre = gap_centre(DEFAULT_SEED, index);
            lowest = lowest.min(centre);
            highest = highest.max(centre);
        }
        assert!(
            lowest < -GAP_CENTRE_RANGE * 0.9 && highest > GAP_CENTRE_RANGE * 0.9,
            "the gaps only ever fell between {lowest} and {highest}"
        );
    }

    /// A game has a course before anybody presses anything. A screen that fills
    /// in on the first flap reads as a bug.
    #[test]
    fn the_course_is_laid_out_before_the_first_tick() {
        let harness = Harness::new(60, 60);
        let pipes = harness.game.pipes();
        assert!(!pipes.is_empty(), "no pipes at all");
        assert_eq!(pipes[0].x, FIRST_PIPE_X);
        let furthest = pipes.last().expect("non-empty").x;
        assert!(
            furthest <= SPAWN_AHEAD && furthest + PIPE_SPACING > SPAWN_AHEAD,
            "the course should reach exactly as far ahead as the window allows, \
             not {furthest}"
        );
    }

    /// Pipes are built ahead of the bird and destroyed behind it, forever.
    #[test]
    fn pipes_are_built_ahead_of_the_bird_and_destroyed_behind_it() {
        let mut harness = Harness::new(60, 60);
        harness.fly_ticks(600);

        let bird = harness.game.bird.x;
        assert!(bird > 40.0, "the bird has to actually travel: {bird}");

        let pipes = harness.game.pipes();
        assert!(!pipes.is_empty(), "the course ran out from under the bird");
        for pipe in &pipes {
            assert!(
                pipe.x >= bird - CULL_BEHIND - PIPE_SPACING,
                "a pipe at {} survived well behind a bird at {bird}",
                pipe.x
            );
            assert!(
                pipe.x <= bird + SPAWN_AHEAD + PIPE_SPACING,
                "a pipe at {} was built far beyond a bird at {bird}",
                pipe.x
            );
        }
        assert!(
            pipes
                .iter()
                .all(|p| p.x + PIPE_HALF_WIDTH > bird - CULL_BEHIND - PIPE_SPACING),
            "pipes behind the cutoff are still in the world"
        );
    }

    /// **A run that goes on forever must not grow forever.** The treadmill
    /// creates and destroys a couple of entities a second; if the destruction
    /// half were missing, the world would climb steadily and nothing else in
    /// this suite would notice.
    #[test]
    fn a_long_run_keeps_the_world_the_same_size() {
        // One bird plus both halves of every pipe in the window, which is
        // `CULL_BEHIND + SPAWN_AHEAD` wide and therefore holds at most this many
        // — the count breathes by one pipe as the window slides, so a ceiling is
        // the honest assertion and a fixed number would not be.
        let window = ((CULL_BEHIND + SPAWN_AHEAD) / PIPE_SPACING).floor() as usize + 1;
        let ceiling = 1 + 2 * window;

        let mut harness = Harness::new(60, 60);
        let mut worst = 0;
        for step in 1..=40 {
            harness.fly_ticks(step * 60);
            worst = worst.max(harness.game.entity_count());
        }

        let travelled = harness.game.bird.x;
        assert!(
            travelled > 150.0,
            "the run has to pass many pipes to mean anything: x = {travelled}"
        );
        let built = (travelled + SPAWN_AHEAD - FIRST_PIPE_X) / PIPE_SPACING;
        assert!(
            built > 20.0,
            "only {built} pipes were ever built, which is not enough churn to test"
        );
        assert!(
            worst <= ceiling,
            "the world peaked at {worst} entities against a window that holds {ceiling}, \
             so pipes are being built and never destroyed"
        );
    }

    /// The course a run meets does not depend on how often the frame loop asked
    /// for a tick.
    #[test]
    fn the_course_is_the_same_at_every_frame_rate() {
        let mut reference: Option<Vec<PipeView>> = None;
        for frame_hz in [20, 60, 240] {
            let mut harness = Harness::new(frame_hz, 60);
            harness.fly_ticks(600);
            let pipes = harness.game.pipes();
            match &reference {
                None => {
                    assert!(pipes.len() > 2, "too few pipes to compare");
                    reference = Some(pipes);
                }
                Some(expected) => {
                    assert_eq!(&pipes, expected, "{frame_hz} fps met a different course");
                }
            }
        }
    }

    /// A restart deals a different course. A game that replayed the same hand
    /// every time would be memorised rather than played.
    #[test]
    fn a_restart_deals_a_different_course() {
        let mut harness = Harness::new(60, 60);
        let first = harness.game.pipes();

        harness.game.key_event(KeyCode::KeyR, true);
        harness.game.tick();
        let second = harness.game.pipes();

        assert_eq!(
            first.len(),
            second.len(),
            "the same stretch of course should be laid out"
        );
        assert_eq!(first[0].x, second[0].x, "and start in the same place");
        assert_ne!(
            first, second,
            "a restart re-dealt the identical course, so the seed advance did nothing"
        );
    }

    /// …and it deals a *predictable* different course. The run counter is
    /// simulation state, so a recorded script replayed from a fresh game meets
    /// the same pipes on its second run as the recording did on its second run.
    #[test]
    fn two_games_with_one_seed_agree_about_every_course() {
        let course = |restarts: u32| {
            let mut harness = Harness::new(60, 60);
            for _ in 0..restarts {
                harness.game.key_event(KeyCode::KeyR, true);
                harness.game.tick();
            }
            harness.game.pipes()
        };
        for restarts in 0..4 {
            assert_eq!(
                course(restarts),
                course(restarts),
                "run {restarts} was not reproducible"
            );
        }
    }

    /// A restart puts the run back to a bird that has not moved, whatever it was
    /// doing.
    #[test]
    fn a_restart_returns_the_bird_to_the_start() {
        let mut harness = Harness::new(60, 60);
        harness.run(1.0, &flap_at(2));
        assert_ne!(harness.game.bird, BIRD_START);

        harness.game.key_event(KeyCode::KeyR, true);
        harness.game.tick();

        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.bird, BIRD_START);
        assert_eq!(harness.game.bird_velocity, DVec3::ZERO);
    }
}
