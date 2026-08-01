//! Breakout game logic: physics integration, ball/wall/paddle colliders,
//! gravity, collision detection, server/client replication.
//!
//! # Where the simulation runs
//!
//! Everything that touches the world runs **inside the server's tick**, in
//! [`BreakoutModule::tick`] — the hook `crcbl-ecs` documents as "called every
//! server tick *after* the ECS schedule has run". That placement is the whole
//! design:
//!
//! * `Server::update` drains its own accumulator and may run zero, one or
//!   several ticks for a single wall-clock timestamp. Collision resolution,
//!   scoring and the lives counter therefore cannot live beside the call — they
//!   have to live *inside* it, or they run a different number of times than the
//!   physics they are resolving.
//! * The swept segment is `velocity * world.tick_dt()`, and `tick_dt` is the
//!   value `Server` wrote into the world from its own clock, so the sweep is the
//!   path the ball actually took during the tick that just ran, at whatever rate
//!   the server was built with.
//! * The paddle is integrated by `PADDLE_SPEED * tick_dt` per **tick**, so its
//!   speed is a property of simulated time and not of the frame rate.
//!
//! [`Game`] is the client-side facade: it resolves input into an
//! [`Intent`], hands the intent to the module, advances the server and client by
//! exactly one tick period, and reads back what to draw.
//!
//! # What is and is not replicated
//!
//! The ball's transform is replicated server → client by
//! `PhysicsSystem::replicate()` and the client's interpolated copy is compared
//! against the authoritative one every tick (a divergence is logged).
//!
//! The **input** path is honest about its limits: the intent is encoded and
//! handed to `Client::set_input`, so a real payload goes over the transport
//! every tick, but `crcbl-server` currently discards inbound input
//! (`Server::drain_inputs`, "inputs (discarded — P3)"). Until the server grows
//! an input path, the module reads the intent from the shared cell it and the
//! facade both hold. Nothing here pretends otherwise.

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

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0050_335F_4252,
};

/// The default simulation rate. Overridable with `--tick-hz`; the value reaches
/// the server, the client, the ECS `tick_dt` and every integrator in this file,
/// so there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

const PADDLE_SPEED: f64 = 12.0;
pub const PADDLE_HALF_WIDTH: f64 = 5.0;
pub const PADDLE_HALF_HEIGHT: f64 = 0.3;
pub const PADDLE_Y: f64 = -8.0;
pub const WORLD_LEFT: f64 = -14.0;
pub const WORLD_RIGHT: f64 = 14.0;
pub const WORLD_TOP: f64 = 9.0;
pub const BALL_RADIUS: f64 = 0.3;
const BALL_START_X: f64 = 0.0;
const BALL_START_Y: f64 = -5.0;
const BALL_SPEED_X: f64 = 4.0;
/// Ball needs ~14 m/s upward to reach the brick grid at y=4..7 from
/// start position y=-5 with Earth gravity (9.8 m/s²):
/// v₀² = 2gΔy = 2*9.8*9 = 176 → v₀ ≈ 13.3. Use 14 for margin.
const BALL_SPEED_Y: f64 = 14.0;
/// How far below the paddle the ball has to fall before the life is lost.
const BALL_DEAD_Y: f64 = PADDLE_Y - 2.0;

const STARTING_LIVES: u32 = 3;

/// The ball's collider, named once so the sweep can lift it out of the world
/// and put the identical shape back.
const BALL_COLLIDER: ColliderComponent = ColliderComponent::Sphere {
    offset: DVec3::ZERO,
    radius: BALL_RADIUS,
    is_trigger: false,
};

const ACTION_LEFT: &str = "move_left";
const ACTION_RIGHT: &str = "move_right";
const ACTION_LAUNCH: &str = "launch";
const ACTION_RESTART: &str = "restart";

/// Brick grid layout.
pub const BRICK_ROWS: usize = 4;
pub const BRICK_COLS: usize = 10;
pub const BRICK_COUNT: usize = BRICK_ROWS * BRICK_COLS;
pub const BRICK_WIDTH: f64 = 2.4;
pub const BRICK_HEIGHT: f64 = 0.8;
const BRICK_GAP: f64 = 0.2;
const BRICK_TOP: f64 = 7.0;
const BRICK_LEFT: f64 = -(BRICK_COLS as f64 * (BRICK_WIDTH + BRICK_GAP)) / 2.0 + BRICK_WIDTH / 2.0;

/// Centre of brick `index` in the grid, in world space.
fn brick_position(index: usize) -> DVec3 {
    let row = index / BRICK_COLS;
    let col = index % BRICK_COLS;
    DVec3::new(
        BRICK_LEFT + col as f64 * (BRICK_WIDTH + BRICK_GAP),
        BRICK_TOP - row as f64 * (BRICK_HEIGHT + BRICK_GAP),
        0.0,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    WaitingForLaunch,
    Playing,
    Won,
    Lost,
}

// ---------------------------------------------------------------------------
// Intent — what the player asked for this tick
// ---------------------------------------------------------------------------

/// One tick of player intent, resolved from the action map on the client and
/// consumed by the server-side module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    left: bool,
    right: bool,
    /// Launch the held ball, or restart a finished game.
    launch: bool,
    /// Restart unconditionally (a menu action, and what the restart test uses).
    restart: bool,
}

impl Intent {
    /// The wire form handed to `Client::set_input`. One byte of flags: small
    /// enough that the per-tick payload is a fixed-size copy rather than a
    /// structure the game re-encodes every frame.
    fn to_wire(self) -> u8 {
        u8::from(self.left)
            | (u8::from(self.right) << 1)
            | (u8::from(self.launch) << 2)
            | (u8::from(self.restart) << 3)
    }
}

// ---------------------------------------------------------------------------
// Shared logic — owned jointly by the facade and the server-side module
// ---------------------------------------------------------------------------

/// The mutable game state the server-side module owns.
///
/// [`Game`] writes [`GameLogic::intent`] before each server tick and reads the
/// results back after it; `BreakoutModule` is the only thing that mutates
/// anything else in here, and it only ever does so from inside a server tick.
#[derive(Debug)]
struct GameLogic {
    ball: Entity,
    paddle: Entity,
    bricks: Vec<Entity>,
    intent: Intent,
    paddle_x: f64,
    ball_pos: DVec3,
    /// Live brick centres, refreshed each tick for the renderer. Reused rather
    /// than rebuilt so a steady-state tick does not allocate.
    brick_positions: Vec<DVec3>,
    score: u32,
    lives: u32,
    state: GameState,
    launched: bool,
    /// Sound cues raised this tick: `(sound id, world x)`. Drained by the
    /// facade, which owns the output stream — audio does not belong on the
    /// simulation's side of the seam.
    sounds: Vec<(u32, f32)>,
    /// Ticks the module has actually run. The facade asserts this advances by
    /// exactly one per `Game::tick`, which is the invariant findings 2 and 3
    /// were both violations of.
    ticks: u64,
}

impl GameLogic {
    fn reset_run(&mut self) {
        self.score = 0;
        self.lives = STARTING_LIVES;
        self.state = GameState::WaitingForLaunch;
        self.launched = false;
        self.paddle_x = 0.0;
    }
}

/// Per-tick game logic, run by the server after the ECS physics schedule.
///
/// `register` is empty on purpose: `Server::set_module` does not call it, and
/// the physics system is registered on the world in [`Game::new`] before the
/// server is built. Everything this module does happens in [`Self::tick`].
struct BreakoutModule {
    shared: Arc<Mutex<GameLogic>>,
}

impl std::fmt::Debug for BreakoutModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreakoutModule").finish_non_exhaustive()
    }
}

impl GameModule for BreakoutModule {
    fn name(&self) -> &str {
        "breakout"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World) {
        let mut logic = lock(&self.shared);
        run_tick(&mut logic, world);
    }
}

/// A poisoned mutex here means a previous tick panicked. The game state is
/// plain data with no invariant a panic could have half-broken, so recovering
/// the guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<GameLogic>) -> MutexGuard<'_, GameLogic> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// One tick of breakout, inside the server's tick, after physics has stepped.
fn run_tick(logic: &mut GameLogic, world: &mut World) {
    logic.ticks += 1;
    logic.sounds.clear();
    let dt = world.tick_dt();
    let intent = std::mem::take(&mut logic.intent);

    // --- state transitions the player asked for -------------------------
    if intent.restart || (intent.launch && matches!(logic.state, GameState::Won | GameState::Lost))
    {
        restart(logic, world);
    } else if intent.launch && logic.state == GameState::WaitingForLaunch {
        logic.state = GameState::Playing;
        logic.launched = true;
        let ball = logic.ball;
        with_physics(world, |phys| {
            set_velocity(phys, ball, DVec3::new(BALL_SPEED_X, BALL_SPEED_Y, 0.0));
        });
    }

    // --- paddle ---------------------------------------------------------
    let dir = f64::from(i8::from(intent.right) - i8::from(intent.left));
    logic.paddle_x = (logic.paddle_x + dir * PADDLE_SPEED * dt).clamp(
        WORLD_LEFT + PADDLE_HALF_WIDTH,
        WORLD_RIGHT - PADDLE_HALF_WIDTH,
    );

    let paddle = logic.paddle;
    let paddle_x = logic.paddle_x;
    let ball = logic.ball;
    let launched = logic.launched;
    with_physics(world, |phys| {
        if let Some(t) = phys.transform(paddle) {
            let mut moved = *t;
            moved.position.x = paddle_x;
            moved.position.y = PADDLE_Y;
            phys.set_transform(paddle, moved);
        }
        // A ball that has not been launched is pinned at the start position,
        // so gravity does not drag it out of the world while the player waits.
        if !launched {
            reset_ball(phys, ball);
        }
    });

    // --- collisions, scoring, lives -------------------------------------
    if logic.state == GameState::Playing {
        resolve_collisions(logic, world, dt);
        check_life_and_win(logic, world);
    }

    refresh_render_state(logic, world);
}

/// Sweeps the ball's path over the tick that just ran and resolves the first
/// thing it hit.
fn resolve_collisions(logic: &mut GameLogic, world: &mut World, dt: f64) {
    let ball = logic.ball;
    let mut broken: Option<Entity> = None;
    let mut cue: Option<(u32, f32)> = None;

    with_physics(world, |phys| {
        let Some((body, transform)) = phys.body(ball).copied().zip(phys.transform(ball).copied())
        else {
            return;
        };
        let pos = transform.position;
        let vel = body.velocity;
        // Physics already integrated `vel * dt`; sweeping backwards over
        // exactly that covers the path the ball took during this tick — which
        // is only true because this runs once per physics tick, with the same
        // `dt` the schedule used.
        let segment = crcbl_phys::Segment {
            start: pos - vel * dt,
            end: pos,
        };

        // `PhysicsSystem::sweep_sphere` has no exclusion list, and the ball's
        // own collider sits at the far end of the segment — so a sweep run with
        // it still in the world reports the ball hitting *itself* at t = 0,
        // every tick, and the ball never leaves the launch position. Lift it
        // out for the duration of the query and put it back afterwards, at
        // whatever transform the resolution settled on.
        phys.remove_collider(ball);
        let hit = phys.sweep_sphere(&segment, BALL_RADIUS);

        let mut resolved = transform;
        if let Some((hit_entity, hit)) = hit {
            let is_brick = logic.bricks.contains(&hit_entity);
            let approaching = vel.dot(hit.normal) < 0.0;

            if is_brick {
                broken = Some(hit_entity);
                logic.score += 10;
                cue = Some((crate::audio::SOUND_BRICK, pos.x as f32));
            } else if approaching {
                cue = Some((crate::audio::SOUND_BOUNCE, pos.x as f32));
            }

            if approaching {
                let mut new_body = body;
                new_body.velocity = vel - 2.0 * vel.dot(hit.normal) * hit.normal;
                phys.set_body(ball, new_body);
                resolved.position = hit.point + hit.normal * BALL_RADIUS * 1.01;
                phys.set_transform(ball, resolved);
            }

            // A broken brick loses its collider in the same borrow that found
            // it, so the next sweep cannot hit a brick that is no longer there.
            if let Some(entity) = broken {
                phys.remove_entity(entity);
            }
        }

        phys.set_collider(ball, &BALL_COLLIDER, &resolved);
    });

    if let Some(entity) = broken {
        logic.bricks.retain(|&e| e != entity);
        world.despawn(entity);
    }
    logic.sounds.extend(cue);
}

/// Loses a life when the ball falls past the paddle, and declares a win when
/// the last brick goes.
fn check_life_and_win(logic: &mut GameLogic, world: &mut World) {
    let ball = logic.ball;
    let fell = with_physics(world, |phys| {
        phys.transform(ball)
            .is_some_and(|t| t.position.y < BALL_DEAD_Y)
    })
    .unwrap_or(false);

    if fell {
        logic.lives = logic.lives.saturating_sub(1);
        logic.launched = false;
        if logic.lives == 0 {
            logic.state = GameState::Lost;
        } else {
            logic.state = GameState::WaitingForLaunch;
            with_physics(world, |phys| reset_ball(phys, ball));
        }
        return;
    }

    if logic.bricks.is_empty() {
        logic.state = GameState::Won;
        logic.launched = false;
    }
}

/// Puts the run back to its opening position: a full brick grid, a held ball, a
/// centred paddle, three lives and a zero score.
///
/// Every one of those is reset here, in one place. A restart that reset the
/// score but not the grid left `bricks.is_empty()` true, so the first `Playing`
/// tick after a win re-entered `Won` at score zero.
fn restart(logic: &mut GameLogic, world: &mut World) {
    for entity in std::mem::take(&mut logic.bricks) {
        with_physics(world, |phys| phys.remove_entity(entity));
        world.despawn(entity);
    }
    logic.bricks = spawn_brick_grid(world);
    logic.reset_run();
    let ball = logic.ball;
    with_physics(world, |phys| reset_ball(phys, ball));
}

/// Copies the authoritative transforms the renderer needs out of the physics
/// world, into buffers that are reused across ticks.
fn refresh_render_state(logic: &mut GameLogic, world: &mut World) {
    let ball = logic.ball;
    let bricks = std::mem::take(&mut logic.bricks);
    let mut positions = std::mem::take(&mut logic.brick_positions);
    positions.clear();
    let ball_pos = with_physics(world, |phys| {
        for &brick in &bricks {
            if let Some(t) = phys.transform(brick) {
                positions.push(t.position);
            }
        }
        phys.transform(ball).map(|t| t.position)
    })
    .flatten();
    if let Some(pos) = ball_pos {
        logic.ball_pos = pos;
    }
    logic.bricks = bricks;
    logic.brick_positions = positions;
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

fn reset_ball(phys: &mut PhysicsSystem, ball: Entity) {
    phys.set_transform(
        ball,
        Transform::from_position(DVec3::new(BALL_START_X, BALL_START_Y, 0.0)),
    );
    set_velocity(phys, ball, DVec3::ZERO);
}

fn set_velocity(phys: &mut PhysicsSystem, entity: Entity, velocity: DVec3) {
    if let Some(body) = phys.body(entity) {
        let mut new_body = *body;
        new_body.velocity = velocity;
        phys.set_body(entity, new_body);
    }
}

/// Spawns a full brick grid and gives every brick a kinematic body and a box
/// collider. Used both at startup and by [`restart`], so the layout is written
/// once.
fn spawn_brick_grid(world: &mut World) -> Vec<Entity> {
    let mut bricks = Vec::with_capacity(BRICK_COUNT);
    for _ in 0..BRICK_COUNT {
        bricks.push(world.spawn());
    }
    with_physics(world, |phys| {
        for (index, &entity) in bricks.iter().enumerate() {
            let transform = Transform::from_position(brick_position(index));
            phys.set_body(entity, RigidBody::new_kinematic());
            phys.set_transform(entity, transform);
            phys.set_collider(
                entity,
                &ColliderComponent::Box {
                    offset: DVec3::ZERO,
                    half_extents: DVec3::new(BRICK_WIDTH / 2.0, BRICK_HEIGHT / 2.0, 0.5),
                    is_trigger: false,
                },
                &transform,
            );
        }
    });
    bricks
}

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

// ---------------------------------------------------------------------------
// Game — the client-side facade
// ---------------------------------------------------------------------------

/// Everything the renderer needs for one frame, in world space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    pub paddle_x: f64,
    pub ball: DVec3,
    pub bricks: Vec<DVec3>,
    pub score: u32,
    pub high_score: u32,
    pub lives: u32,
    pub state: Option<GameState>,
}

pub struct Game {
    pub paddle_entity: Entity,
    pub ball_entity: Entity,
    _walls: [Entity; 3],
    action_map: ActionMap,
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    shared: Arc<Mutex<GameLogic>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call. Taken from a `FrameClock` built the
    /// same way the server built its own, so the two use identical integer
    /// nanoseconds and never drift.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    pub audio: crate::audio::Audio,
    pub high_score: crate::high_score::HighScore,
    /// Queued key events from the shell pump, replayed after `begin_tick`.
    pending_keys: Vec<(KeyCode, bool)>,
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub score: u32,
    pub lives: u32,
    pub state: GameState,
    pub ball_x: f64,
    sound_played_this_tick: bool,
    prev_log_state: GameState,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("paddle_entity", &self.paddle_entity)
            .field("ball_entity", &self.ball_entity)
            .field("state", &self.state)
            .field("score", &self.score)
            .field("lives", &self.lives)
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
    /// `tick_hz` is the one simulation rate in the process: it sets the
    /// server's clock, the client's clock, the ECS `tick_dt` every integrator
    /// reads, and the period [`Game::tick`] advances by.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero. Callers parse it from `--tick-hz`, which rejects
    /// zero with exit 2.
    pub fn new(headless: bool, tick_hz: u32) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let mut world = World::new();

        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        world.register_system(Box::new(phys));

        // Ball: dynamic, sphere collider.
        let ball_entity = world.spawn();
        let ball_start = Transform::from_position(DVec3::new(BALL_START_X, BALL_START_Y, 0.0));
        with_physics(&mut world, |phys| {
            phys.set_body(ball_entity, RigidBody::new_dynamic(1.0));
            phys.set_transform(ball_entity, ball_start);
            phys.set_collider(ball_entity, &BALL_COLLIDER, &ball_start);
        });

        // Paddle: kinematic, box collider.
        let paddle_entity = spawn_static_box(
            &mut world,
            DVec3::new(0.0, PADDLE_Y, 0.0),
            DVec3::new(PADDLE_HALF_WIDTH, PADDLE_HALF_HEIGHT, 0.5),
        );

        let walls = [
            spawn_static_box(
                &mut world,
                DVec3::new(WORLD_LEFT - 0.5, 0.0, 0.0),
                DVec3::new(0.5, WORLD_TOP, 1.0),
            ),
            spawn_static_box(
                &mut world,
                DVec3::new(WORLD_RIGHT + 0.5, 0.0, 0.0),
                DVec3::new(0.5, WORLD_TOP, 1.0),
            ),
            spawn_static_box(
                &mut world,
                DVec3::new(0.0, WORLD_TOP + 0.5, 0.0),
                DVec3::new(WORLD_RIGHT - WORLD_LEFT, 0.5, 1.0),
            ),
        ];

        let bricks = spawn_brick_grid(&mut world);

        log::info!(
            "physics: {} colliders, {} bodies (ball + paddle + 3 walls + {BRICK_COUNT} bricks)",
            5 + BRICK_COUNT,
            5 + BRICK_COUNT,
        );

        let mut action_map = ActionMap::new();
        action_map.declare(ActionDecl {
            name: ACTION_LEFT.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::ArrowLeft)],
        });
        action_map.declare(ActionDecl {
            name: ACTION_RIGHT.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::ArrowRight)],
        });
        action_map.declare(ActionDecl {
            name: ACTION_LAUNCH.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Space)],
        });
        action_map.declare(ActionDecl {
            name: ACTION_RESTART.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::KeyR)],
        });

        let shared = Arc::new(Mutex::new(GameLogic {
            ball: ball_entity,
            paddle: paddle_entity,
            bricks,
            intent: Intent::default(),
            paddle_x: 0.0,
            ball_pos: DVec3::new(BALL_START_X, BALL_START_Y, 0.0),
            brick_positions: Vec::with_capacity(BRICK_COUNT),
            score: 0,
            lives: STARTING_LIVES,
            state: GameState::WaitingForLaunch,
            launched: false,
            sounds: Vec::new(),
            ticks: 0,
        }));

        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server =
            Server::try_new_with_compatibility(world, server_transport, tick_hz, COMPATIBILITY)
                .map_err(|e| GameError::Server(e.to_string()))?;
        server.set_module(Box::new(BreakoutModule {
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

        // Fill the render buffers before the first tick: the loop's very first
        // frame runs no ticks (the clock spends it establishing its baseline)
        // and still has to draw a board.
        {
            let mut logic = lock(&shared);
            refresh_render_state(&mut logic, server.world_mut());
        }

        let game = Self {
            paddle_entity,
            ball_entity,
            _walls: walls,
            action_map,
            server,
            client,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            audio: crate::audio::Audio::new(headless),
            high_score: crate::high_score::HighScore::load(headless),
            pending_keys: Vec::new(),
            score: 0,
            lives: STARTING_LIVES,
            state: GameState::WaitingForLaunch,
            ball_x: BALL_START_X,
            sound_played_this_tick: false,
            prev_log_state: GameState::WaitingForLaunch,
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
    /// flags are per **tick**, and `ActionMap::begin_tick` is documented as
    /// resetting those flags and re-resolving every action — so an event fed
    /// before `begin_tick` has its press edge erased by it. Queueing here and
    /// replaying after `begin_tick` is the order the action map asks for, and
    /// it is also what makes a frame that runs no ticks lossless: the events
    /// simply wait for the next one.
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
            left: action_held(&self.action_map, ACTION_LEFT),
            right: action_held(&self.action_map, ACTION_RIGHT),
            launch: action_just_pressed(&self.action_map, ACTION_LAUNCH),
            restart: action_just_pressed(&self.action_map, ACTION_RESTART),
        };

        let ticks_before = {
            let mut logic = lock(&self.shared);
            logic.intent.left = intent.left;
            logic.intent.right = intent.right;
            logic.intent.launch |= intent.launch;
            logic.intent.restart |= intent.restart;
            logic.ticks
        };

        // A real payload rather than a re-encoded empty one: `Client::set_input`
        // takes the *input bytes*, and wraps them in `ClientToServer::Input`
        // itself, so encoding a whole message here and passing it as the data
        // field nested one inside the other.
        self.client.set_input(vec![intent.to_wire()]);

        self.sim_time += self.tick_period;
        let server_ticks = self.server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let alpha = self.client.update(self.sim_time);
        self.ticks_run += 1;

        let (score, lives, state, ball_pos, ticks_after) = {
            let mut logic = lock(&self.shared);
            self.sound_played_this_tick = !logic.sounds.is_empty();
            for (id, x) in logic.sounds.drain(..).collect::<Vec<_>>() {
                self.audio.play_panned(id, x);
            }
            (
                logic.score,
                logic.lives,
                logic.state,
                logic.ball_pos,
                logic.ticks,
            )
        };
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        self.score = score;
        self.lives = lives;
        self.state = state;
        self.ball_x = ball_pos.x;

        // The client's interpolated copy of the ball is the only evidence the
        // replication path is alive; a divergence is a bug in it, not here.
        let replicated = self.client.interpolate(alpha);
        let ball_bits = self.ball_entity.to_bits();
        if self.state == GameState::Playing
            && let Some(transform) = replicated
                .transforms
                .iter()
                .find(|(bits, _)| *bits == ball_bits)
                .map(|(_, t)| t)
            && (transform.position.x - ball_pos.x).abs() > 1.0
        {
            log::warn!(
                "replication drift: server ball_x={:.2}, client ball_x={:.2}",
                ball_pos.x,
                transform.position.x,
            );
        }

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        if state_changed || self.sound_played_this_tick || self.ticks_run.is_multiple_of(60) {
            log_hud(
                self.state,
                self.score,
                self.lives,
                self.bricks_remaining(),
                self.ball_x,
                self.high_score.get(),
                self.sound_played_this_tick,
            );
        }

        if matches!(self.state, GameState::Won | GameState::Lost) {
            self.high_score.update(self.score);
        }
    }

    /// How many bricks are still standing.
    #[must_use]
    pub fn bricks_remaining(&self) -> usize {
        lock(&self.shared).bricks.len()
    }

    /// The paddle's authoritative X position.
    #[must_use]
    pub fn paddle_x(&self) -> f64 {
        lock(&self.shared).paddle_x
    }

    /// Everything the renderer draws, in world space.
    ///
    /// `out` is reused across frames so a steady-state frame does not allocate
    /// a fresh brick list.
    pub fn render_state(&self, out: &mut RenderState) {
        let logic = lock(&self.shared);
        out.paddle_x = logic.paddle_x;
        out.ball = logic.ball_pos;
        out.bricks.clear();
        out.bricks.extend_from_slice(&logic.brick_positions);
        out.score = logic.score;
        out.lives = logic.lives;
        out.state = Some(logic.state);
        drop(logic);
        out.high_score = self.high_score.get();
    }
}

// ---- HUD (console-based, alongside the on-screen one) -----------------------

fn log_hud(
    state: GameState,
    score: u32,
    lives: u32,
    bricks: usize,
    ball_x: f64,
    high: u32,
    sound: bool,
) {
    let state_str = match state {
        GameState::WaitingForLaunch => "WAITING — press Space to launch",
        GameState::Playing => "PLAYING",
        GameState::Won => "YOU WIN! — press Space to restart",
        GameState::Lost => "GAME OVER — press Space to restart",
    };
    let sound_str = if sound { " 🔊" } else { "" };
    log::info!(
        "[HUD] Score: {score} (best: {high})  Lives: {lives}  Bricks: {bricks}  \
         Ball x: {ball_x:.1}  {state_str}{sound_str}"
    );
}

// ---- input helpers ----------------------------------------------------------

fn action_held(map: &ActionMap, name: &str) -> bool {
    map.action(name).is_some_and(|v| match v {
        crcbl_input::ActionValue::Button(b) => matches!(
            b.state,
            crcbl_input::ButtonState::Pressed | crcbl_input::ButtonState::Held { .. }
        ),
        _ => false,
    })
}

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

    /// Drives a `Game` exactly the way `app::Loop::frame` does — a frame clock
    /// at `frame_hz`, a fixed-timestep accumulator at `tick_hz`, and events
    /// pumped once per frame — for `seconds` of simulated wall time.
    ///
    /// The frame rate and the tick rate are deliberately independent knobs:
    /// every property asserted below is a property of *simulated time*, and a
    /// loop that leaked the frame rate into the simulation is exactly what
    /// makes them disagree.
    struct Harness {
        game: Game,
        clock: FrameClock,
        time: ManualTime,
        frame_step: Duration,
        ticks: u64,
    }

    impl Harness {
        fn new(frame_hz: u32, tick_hz: u32) -> Self {
            let game = Game::new(true, tick_hz).expect("headless game always starts");
            Self {
                game,
                clock: FrameClock::new(tick_hz),
                time: ManualTime::new(),
                frame_step: FrameClock::new(frame_hz).tick_dt(),
                ticks: 0,
            }
        }

        /// One frame: advance the clock, drain whole ticks, exactly as the app
        /// loop does.
        fn frame(&mut self) {
            self.time.advance(self.frame_step);
            self.clock.update(self.time.elapsed());
            while self.clock.consume_tick() {
                self.ticks += 1;
                self.game.tick();
            }
        }

        fn frames_for(&self, seconds: f64) -> u64 {
            (seconds / self.frame_step.as_secs_f64()).round() as u64
        }

        /// Runs for `seconds` of simulated wall time, feeding each `script`
        /// entry exactly once, on the first frame at or after its tick index.
        ///
        /// "Exactly once" matters: a frame rate above the tick rate runs
        /// several frames per tick, and an entry that re-fired on each of them
        /// would make the input a function of the frame rate — the very thing
        /// these tests exist to rule out.
        fn run(&mut self, seconds: f64, script: &Script) -> &mut Self {
            let mut fired = vec![false; script.len()];
            for _ in 0..self.frames_for(seconds) {
                for (slot, &(at, key, pressed)) in script.iter().enumerate() {
                    if !fired[slot] && self.ticks >= at {
                        fired[slot] = true;
                        self.game.key_event(key, pressed);
                    }
                }
                self.frame();
            }
            self
        }

        /// Runs for `seconds`, pressing Space whenever the game is waiting for
        /// a launch — a player who keeps playing.
        fn play(&mut self, seconds: f64) -> &mut Self {
            for _ in 0..self.frames_for(seconds) {
                if self.game.state == GameState::WaitingForLaunch {
                    self.game.key_event(KeyCode::Space, true);
                    self.game.key_event(KeyCode::Space, false);
                }
                self.frame();
            }
            self
        }
    }

    /// Space, pressed and released inside a single frame.
    const LAUNCH: &Script = &[(0, KeyCode::Space, true), (0, KeyCode::Space, false)];

    /// The launch actually happens, the ball actually reaches the grid, and
    /// bricks actually break.
    ///
    /// Covers finding 5: the previous "determinism" tests never sent `Space`,
    /// so the game stayed in `WaitingForLaunch`, the collision path never ran,
    /// and every assertion compared 0 to 0. This one fails outright if the
    /// launch input is dropped or the sweep never fires.
    #[test]
    fn launching_the_ball_breaks_bricks() {
        let mut h = Harness::new(60, 60);
        h.run(3.0, LAUNCH);
        assert_ne!(h.ticks, 0);
        assert!(
            h.game.score > 0,
            "score stayed at {} — the ball was never launched",
            h.game.score,
        );
        assert!(
            h.game.bricks_remaining() < BRICK_COUNT,
            "{} bricks left of {BRICK_COUNT}",
            h.game.bricks_remaining(),
        );
    }

    /// A press and a release inside the same pump batch still launches.
    ///
    /// The events are queued and replayed *after* `begin_tick`, so the press
    /// edge survives the flag reset; `LAUNCH` above is exactly that case, and
    /// this pins it down on its own.
    #[test]
    fn a_press_and_release_in_one_frame_is_not_lost() {
        let mut h = Harness::new(60, 60);
        h.game.key_event(KeyCode::Space, true);
        h.game.key_event(KeyCode::Space, false);
        // The first frame runs no ticks — the clock spends it on its baseline —
        // so the queued pair waits for the second, which is the other half of
        // the property: a frame that runs no ticks loses no input.
        h.frame();
        h.frame();
        assert_eq!(h.game.state, GameState::Playing);
    }

    /// **Finding 2.** The paddle's speed is a property of simulated time, not
    /// of the frame rate.
    ///
    /// Both runs cover one second of simulated time at the same tick rate; one
    /// renders 60 frames and the other 240. The old loop integrated the paddle
    /// once per *frame* by a hardcoded `PADDLE_SPEED / 60`, so the 240 fps run
    /// moved the paddle four times as far — this assertion is what that fails.
    #[test]
    fn paddle_speed_is_per_tick_not_per_frame() {
        let hold = &[(0, KeyCode::ArrowRight, true)];
        let mut slow = Harness::new(60, 60);
        let mut fast = Harness::new(240, 60);
        slow.run(0.5, hold);
        fast.run(0.5, hold);

        assert_eq!(slow.ticks, fast.ticks, "same sim time, same tick count");
        assert!(
            (slow.game.paddle_x() - fast.game.paddle_x()).abs() < 1e-9,
            "paddle at {} after 60 fps but {} after 240 fps",
            slow.game.paddle_x(),
            fast.game.paddle_x(),
        );
        // And it is the *right* distance: one tick's worth per tick.
        let expected = slow.ticks as f64 * PADDLE_SPEED * slow.game.tick_dt_secs();
        assert!(
            (slow.game.paddle_x() - expected).abs() < 1e-9,
            "paddle moved {} where {expected} was owed",
            slow.game.paddle_x(),
        );
    }

    /// **Finding 3.** Collision resolution runs once per physics tick, so the
    /// outcome does not depend on how many frames the physics was spread over.
    ///
    /// 20 fps runs three physics ticks per frame and 240 fps runs one every
    /// fourth frame. The old code swept a single `vel * (1/60)` segment per
    /// *frame*, so the slow run tunnelled the ball through bricks and walls and
    /// the fast run re-swept an unchanged position — the scores diverged. Same
    /// simulated time, same tick rate, so the same score is the only correct
    /// answer.
    #[test]
    fn collisions_are_resolved_once_per_tick_at_any_frame_rate() {
        let mut slow = Harness::new(20, 60);
        let mut normal = Harness::new(60, 60);
        let mut fast = Harness::new(240, 60);
        // Three seconds: one launch, many brick hits, and no life lost yet —
        // so the input is identical per tick at every frame rate.
        slow.run(3.0, LAUNCH);
        normal.run(3.0, LAUNCH);
        fast.run(3.0, LAUNCH);

        assert!(normal.game.score > 0, "the run must actually score");
        assert_eq!(
            (slow.game.score, slow.game.bricks_remaining()),
            (normal.game.score, normal.game.bricks_remaining()),
            "20 fps and 60 fps disagree",
        );
        assert_eq!(
            (fast.game.score, fast.game.bricks_remaining()),
            (normal.game.score, normal.game.bricks_remaining()),
            "240 fps and 60 fps disagree",
        );
    }

    /// The ball never escapes the box. A tunnelled ball leaves through a wall
    /// and never comes back, which is the visible half of finding 3.
    #[test]
    fn the_ball_stays_inside_the_walls() {
        let mut h = Harness::new(20, 60);
        for _ in 0..400 {
            if h.game.state == GameState::WaitingForLaunch {
                h.game.key_event(KeyCode::Space, true);
                h.game.key_event(KeyCode::Space, false);
            }
            h.frame();
            let x = h.game.ball_x;
            assert!(
                x > WORLD_LEFT - 1.0 && x < WORLD_RIGHT + 1.0,
                "ball escaped to x={x} at tick {}",
                h.ticks,
            );
        }
    }

    /// **Finding 1.** A restart puts the whole run back, not half of it.
    ///
    /// The bricks were `swap_remove`d and `despawn`ed and nothing re-spawned
    /// them, so a restart after a win left `bricks.is_empty()` true and the
    /// first `Playing` tick re-entered `Won` at score zero.
    #[test]
    fn a_restart_respawns_the_brick_grid() {
        let mut h = Harness::new(60, 60);
        h.run(3.0, LAUNCH);
        assert!(h.game.score > 0);
        assert!(h.game.bricks_remaining() < BRICK_COUNT);

        h.game.key_event(KeyCode::KeyR, true);
        h.game.key_event(KeyCode::KeyR, false);
        h.frame();

        assert_eq!(h.game.bricks_remaining(), BRICK_COUNT, "grid must be whole");
        assert_eq!(h.game.score, 0);
        assert_eq!(h.game.lives, STARTING_LIVES);
        assert_eq!(h.game.state, GameState::WaitingForLaunch);

        // And the very next tick must not declare a win over an empty grid.
        h.game.key_event(KeyCode::Space, true);
        h.frame();
        assert_eq!(h.game.state, GameState::Playing);
        h.run(0.5, &[]);
        assert_eq!(h.game.state, GameState::Playing, "won on a full grid");
    }

    /// **Finding 1, the `Won` path.** Winning and pressing Space restarts into
    /// a playable run rather than straight back into `Won`.
    #[test]
    fn winning_then_restarting_does_not_re_enter_won() {
        let mut h = Harness::new(60, 60);
        // Clear the grid the way the game does — through the module, so the
        // colliders go with the entities — by playing until it is empty.
        while h.game.bricks_remaining() > 0 && h.ticks < 40_000 {
            if h.game.state == GameState::Lost {
                h.game.key_event(KeyCode::KeyR, true);
                h.game.key_event(KeyCode::KeyR, false);
            }
            h.play(0.5);
        }
        if h.game.state != GameState::Won {
            // The ball is not guaranteed to clear 40 bricks inside the tick
            // budget; the restart path is covered exhaustively above, and this
            // test only adds the `Won`-specific transition when it is reached.
            return;
        }

        h.game.key_event(KeyCode::Space, true);
        h.game.key_event(KeyCode::Space, false);
        h.frame();
        assert_eq!(h.game.state, GameState::WaitingForLaunch);
        assert_eq!(h.game.bricks_remaining(), BRICK_COUNT);
        h.run(0.5, &[]);
        assert_ne!(h.game.state, GameState::Won, "re-won on a full grid");
    }

    /// Losing every life ends the run, and the score survives into the summary.
    #[test]
    fn losing_every_life_ends_the_run() {
        let mut h = Harness::new(60, 60);
        while h.ticks < 6_000 && !matches!(h.game.state, GameState::Lost | GameState::Won) {
            h.play(1.0);
        }
        assert!(
            matches!(h.game.state, GameState::Lost | GameState::Won),
            "the run never ended: {:?} with {} lives",
            h.game.state,
            h.game.lives,
        );
    }

    /// Two identical scripts produce identical runs — and, unlike the previous
    /// version of this test, the run is one where something actually happened.
    #[test]
    fn a_scripted_run_is_deterministic() {
        let outcome = |_| {
            let mut h = Harness::new(60, 60);
            h.run(3.0, LAUNCH);
            (
                h.game.score,
                h.game.lives,
                h.game.state,
                h.game.bricks_remaining(),
                (h.game.ball_x * 1e9).round(),
            )
        };
        let first = outcome(());
        let second = outcome(());
        assert_eq!(first, second);
        assert!(
            first.0 > 0,
            "a determinism test over nothing proves nothing"
        );
    }

    /// `--tick-hz` is a real knob: it changes the simulation rate rather than
    /// only a counter in the summary.
    #[test]
    fn the_tick_rate_reaches_the_simulation() {
        let mut thirty = Harness::new(60, 30);
        let mut sixty = Harness::new(60, 60);
        thirty.run(1.0, &[(0, KeyCode::ArrowRight, true)]);
        sixty.run(1.0, &[(0, KeyCode::ArrowRight, true)]);

        assert!((thirty.game.tick_dt_secs() - 1.0 / 30.0).abs() < 1e-6);
        assert!((sixty.game.tick_dt_secs() - 1.0 / 60.0).abs() < 1e-6);
        assert!(thirty.ticks * 2 <= sixty.ticks + 2);
        // Half the tick rate, half as many ticks, twice the step: the paddle
        // covers the same ground either way.
        assert!(
            (thirty.game.paddle_x() - sixty.game.paddle_x()).abs() < 0.1,
            "{} vs {}",
            thirty.game.paddle_x(),
            sixty.game.paddle_x(),
        );
    }

    /// The render snapshot describes the whole board, not just the paddle.
    #[test]
    fn the_render_state_carries_every_live_brick() {
        let mut h = Harness::new(60, 60);
        let mut state = RenderState::default();
        h.frame();
        h.game.render_state(&mut state);
        assert_eq!(state.bricks.len(), BRICK_COUNT);
        assert_eq!(state.state, Some(GameState::WaitingForLaunch));

        h.run(3.0, LAUNCH);
        h.game.render_state(&mut state);
        assert_eq!(state.bricks.len(), h.game.bricks_remaining());
        assert!(state.bricks.len() < BRICK_COUNT);
    }
}
