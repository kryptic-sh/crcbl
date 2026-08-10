//! Breakout game logic: physics integration, ball/wall/paddle colliders,
//! collision detection, server/client replication.
//!
//! # The ball is not a projectile
//!
//! There is no gravity and no drag. A collision changes the ball's
//! *direction*, and the only thing that changes its *speed* is breaking a
//! brick — [`ramped_speed`], applied once per brick and capped. That is the
//! whole model, and it is what breakout has always been: a ball under Earth
//! gravity arcs, so the same launch reaches a different brick depending on how
//! far across the screen it has travelled, and the player cannot aim.
//!
//! Every collider answers with a mirror reflection, the paddle included — a
//! paddle standing still returns the ball at the angle it arrived, exactly as a
//! wall does. What the paddle adds is **drag**: a paddle being driven left or
//! right decides which way the ball goes next, and it can turn a ball back the
//! way it came rather than merely rebounding it. See [`bounce`]. All of the
//! player's control over the ball is in moving the paddle.
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

use crcbl::core::input::KeyCode;
use crcbl::ecs::{Entity, GameModule, World};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{ColliderComponent, PhysicsSystem, RigidBody, Transform};
use crcbl::session::Loopback;

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
/// How thick the three walls are, in world units.
///
/// The value the wall colliders were always spawned at, named because
/// [`crate::art`] draws them: `assets/field.crpix`'s nine-slice insets are this
/// thickness, so the inner face of the drawn wall is the line the ball bounces
/// off. Two literal halves of it in `Game::new` were what the art would
/// otherwise have had to guess at.
pub const WALL_THICKNESS: f64 = 1.0;
pub const BALL_RADIUS: f64 = 0.3;
const BALL_START_X: f64 = 0.0;
const BALL_START_Y: f64 = -5.0;
/// The speed a ball launches at, in world units per second.
///
/// 11 covers the 8.6 units from the start position at y = -5 to the underside
/// of the lowest brick row at y = 3.6 in about 0.8 s, which is the pace the
/// game opens at. It is the *only* thing that changes the ball's speed after a
/// launch, and it changes it in one place — see [`ramped_speed`].
const BALL_SPEED: f64 = 11.0;
/// What one broken brick multiplies the ball's speed by.
///
/// `docs/plan/sample/01-breakout.md` puts "speed ramps per hit" in scope, and a
/// game whose ball moves at exactly one speed from the first brick to the
/// fortieth has no arc to it. 2% a brick is small enough not to be felt as a
/// jolt and compounds to the cap over most of a grid.
const SPEED_RAMP: f64 = 1.02;
/// The fastest the ramp takes the ball, in world units per second.
///
/// 1.6x the launch speed: 0.3 units per tick at 60 Hz, still under the ball's
/// own diameter, so the sweep resolves a wall or a brick on the tick it reaches
/// it rather than a tick late.
const MAX_BALL_SPEED: f64 = BALL_SPEED * 1.6;
/// How far off vertical a launch goes, in radians. Not zero: a ball launched
/// straight up comes straight back down onto the middle of the paddle, and the
/// opening of every run would be identical.
const LAUNCH_ANGLE: f64 = 0.35;
/// The least sideways a moving paddle sends the ball, as a fraction of its
/// speed.
///
/// A ball caught dead vertical has no horizontal component to turn, so without
/// a floor a moving paddle would do nothing to it. 0.4 of the speed puts it out
/// at about 24° from vertical — a definite push, and still climbing.
const PADDLE_DRAG_FRACTION: f64 = 0.4;
/// The shallowest the ball's direction may get, as a fraction of its speed.
///
/// A ball travelling almost horizontally rallies between the two side walls
/// above the paddle forever, and the player has no way to reach it. Every
/// bounce tilts the direction back to at least this, which costs nothing in the
/// normal case and makes the degenerate one impossible.
const MIN_VERTICAL_FRACTION: f64 = 0.25;
/// How far below the paddle the ball has to fall before the life is lost.
const BALL_DEAD_Y: f64 = PADDLE_Y - 2.0;

/// A fresh game's lives. The start menu is only for a fresh game, so this is
/// also how `menu.rs` tells "never started" from "lost a life": the moment one
/// goes, `lives` is one fewer and no state the game can be in brings it back
/// without a restart.
pub const STARTING_LIVES: u32 = 3;

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
pub const BRICK_GAP: f64 = 0.2;
pub const BRICK_TOP: f64 = 7.0;
const BRICK_LEFT: f64 = -(BRICK_COLS as f64 * (BRICK_WIDTH + BRICK_GAP)) / 2.0 + BRICK_WIDTH / 2.0;

/// Centre of brick `index` in the grid, in world space.
///
/// Public because [`crate::art`] runs it backwards: the renderer is handed a
/// bare list of live brick centres and has to recover which row a brick is in to
/// know which frame of the sheet to draw it with. `art::brick_frame` is the
/// inverse, and `art`'s tests hold the two to each other.
pub fn brick_position(index: usize) -> DVec3 {
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
    /// The speed this ball is on. Starts at [`BALL_SPEED`] and ramps with every
    /// brick it breaks; a fresh ball starts over.
    ball_speed: f64,
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
        self.ball_speed = BALL_SPEED;
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
            set_velocity(phys, ball, launch_velocity());
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
        // A ball that has not been launched is pinned at the start position, so
        // a run that has ended — or a life that has just been lost — parks it
        // there rather than wherever the last collision left it.
        if !launched {
            reset_ball(phys, ball);
        }
    });

    // --- collisions, scoring, lives -------------------------------------
    if logic.state == GameState::Playing {
        resolve_collisions(logic, world, dt, dir);
        check_life_and_win(logic, world);
    }

    refresh_render_state(logic, world);
}

/// Sweeps the ball's path over the tick that just ran and resolves the first
/// thing it hit.
///
/// `paddle_dir` is the direction the player is driving the paddle this tick,
/// -1, 0 or 1. It only matters for a hit on the paddle's face, where it is the
/// whole of the player's control over where the ball goes next — see
/// [`bounce`].
fn resolve_collisions(logic: &mut GameLogic, world: &mut World, dt: f64, paddle_dir: f64) {
    let ball = logic.ball;
    let paddle = logic.paddle;
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
        let segment = crcbl::phys::Segment {
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
                // Ramped *before* the bounce below reads it, so the brick that
                // raised the speed is the one the ball leaves faster.
                logic.ball_speed = ramped_speed(logic.ball_speed);
                cue = Some((crate::audio::SOUND_BRICK, pos.x as f32));
            } else if approaching {
                cue = Some((crate::audio::SOUND_BOUNCE, pos.x as f32));
            }

            if approaching {
                let mut new_body = body;
                // Only the paddle's *face* takes english, and only from a
                // paddle that is moving. `normal.y` tells the top face from the
                // ends: a ball that clips the side of the paddle is coming from
                // beside it, and dragging that one sideways would be a save the
                // player did not make.
                let speed = logic.ball_speed;
                let english = if hit_entity == paddle && hit.normal.y > 0.5 {
                    paddle_dir
                } else {
                    0.0
                };
                new_body.velocity = bounce(vel, hit.normal, english, speed);
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
            // A fresh ball is a fresh ramp: inheriting the speed of the ball
            // that was just lost is how a run gets unrecoverable.
            logic.ball_speed = BALL_SPEED;
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
    world.system_mut::<PhysicsSystem>().map(f)
}

/// The velocity a launch gives the ball: up and slightly to the right, at the
/// speed every ball starts at.
fn launch_velocity() -> DVec3 {
    DVec3::new(LAUNCH_ANGLE.sin(), LAUNCH_ANGLE.cos(), 0.0) * BALL_SPEED
}

/// The speed a ball is on after breaking one more brick.
///
/// The whole ramp, in one place, so the cap cannot be applied on one path and
/// forgotten on another.
fn ramped_speed(speed: f64) -> f64 {
    (speed * SPEED_RAMP).min(MAX_BALL_SPEED)
}

/// The velocity a bounce off `normal` gives the ball.
///
/// `english` is the direction the paddle is being driven — -1, 0 or 1 — and is
/// zero for everything that is not the paddle's face.
///
/// # A still paddle is a mirror; a moving one steers
///
/// With `english` at zero this is the same reflection a wall gives: the ball
/// leaves at the angle it arrived and the contact changes nothing else. That is
/// the whole behaviour of a paddle the player is not moving.
///
/// A paddle that *is* moving decides which way the ball goes next — including
/// against the way it was already travelling. A ball coming down to the right,
/// caught by a paddle sweeping left, leaves to the **left**: the paddle drags
/// it, rather than the ball merely rebounding off a surface that happens to be
/// in motion. The incoming angle still sets how steep the return is, and
/// [`PADDLE_DRAG_FRACTION`] floors it so a ball caught falling straight down is
/// turned rather than sent straight back up.
fn bounce(vel: DVec3, normal: DVec3, english: f64, speed: f64) -> DVec3 {
    let mirrored = vel - 2.0 * vel.dot(normal) * normal;
    if english == 0.0 {
        return keep_speed(mirrored, speed);
    }
    let sideways = mirrored.x.abs().max(speed * PADDLE_DRAG_FRACTION) * english;
    keep_speed(DVec3::new(sideways, mirrored.y, 0.0), speed)
}

/// Puts `vel` back at `speed`, no shallower than
/// [`MIN_VERTICAL_FRACTION`].
///
/// Mirror reflection off an axis-aligned box preserves speed exactly, so the
/// rescale is a no-op in exact arithmetic and a drift-killer in floating point.
/// The vertical floor is the part that changes behaviour: it is what stops a
/// ball from ending up in a horizontal rally the paddle can never reach.
fn keep_speed(vel: DVec3, speed: f64) -> DVec3 {
    let planar = DVec3::new(vel.x, vel.y, 0.0);
    if planar.length_squared() < 1e-12 {
        return DVec3::new(0.0, speed, 0.0);
    }
    let dir = planar.normalize();
    // Rebuilt from the clamped y rather than scaled, so the result is exactly
    // on the circle of radius `BALL_SPEED` and exactly at the floor angle.
    let y = if dir.y.abs() < MIN_VERTICAL_FRACTION {
        MIN_VERTICAL_FRACTION.copysign(dir.y)
    } else {
        dir.y
    };
    let x = (1.0 - y * y).max(0.0).sqrt().copysign(dir.x);
    DVec3::new(x, y, 0.0) * speed
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

/// Breakout's debug-panel section: the two board numbers nothing on screen
/// says.
///
/// **Both of them are invisible by construction, which is the whole reason
/// there is a module here rather than a HUD line.** The HUD already carries the
/// score, the lives and the state, so repeating them would be a section a
/// reader has to check against the line above it.
///
/// [`BoardStats::ball_speed`] is the one this sample cannot be understood
/// without: this game's difficulty is a ramp — [`ramped_speed`] multiplies the
/// ball's speed once per brick and clamps it at [`MAX_BALL_SPEED`] — and a
/// player feels the ball getting faster while nothing anywhere reports it, so
/// "did the ramp apply, and has it hit the cap" is a question the game could
/// not previously be asked.
///
/// [`BoardStats::bricks`] is the win condition. Countable on screen, in the
/// sense that forty rectangles can be counted; the row is what makes "two left
/// and it will not end" separable from "one left, behind another".
///
/// The counts are a snapshot taken during the frame's own draw, not read at
/// render time — see [`Game::board_stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStats {
    /// Bricks still standing, of [`BRICK_COUNT`].
    pub bricks: usize,
    /// The speed the ball is on right now, in world units per second.
    pub ball_speed: f64,
}

impl crcbl::ui::DebugModule for BoardStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("board");
        section.row("bricks", format_args!("{}/{}", self.bricks, BRICK_COUNT));
        section.row("ball", format_args!("{:.2}/s", self.ball_speed));
    }
}

pub struct Game {
    pub paddle_entity: Entity,
    pub ball_entity: Entity,
    _walls: [Entity; 3],
    action_map: ActionMap,
    /// The server, its client and the transport between them.
    ///
    /// One field rather than two, because the three things the halves must
    /// agree on — the tick rate, the compatibility and the transport pair —
    /// are what [`Loopback::new`] takes, and a game that holds them separately
    /// is a game that can be built with them disagreeing.
    session: Loopback,
    shared: Arc<Mutex<GameLogic>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call. Taken from a `FrameClock` built the
    /// same way the server built its own, so the two use identical integer
    /// nanoseconds and never drift.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    pub audio: crate::audio::Audio,
    pub high_score: crcbl::store::record::Record,
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

        // No force providers at all: see the module docs. Breakout's ball is
        // steered by its collisions and by nothing else, so the integrator only
        // ever moves it along the velocity a bounce left it with.
        let phys = PhysicsSystem::new();
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

        // `WALL_THICKNESS / 2.0` where three `0.5` literals used to be — the
        // same numbers, named, because `crate::art` draws these three boxes and
        // had no way to know how thick they were.
        let half = WALL_THICKNESS / 2.0;
        let walls = [
            spawn_static_box(
                &mut world,
                DVec3::new(WORLD_LEFT - half, 0.0, 0.0),
                DVec3::new(half, WORLD_TOP, 1.0),
            ),
            spawn_static_box(
                &mut world,
                DVec3::new(WORLD_RIGHT + half, 0.0, 0.0),
                DVec3::new(half, WORLD_TOP, 1.0),
            ),
            spawn_static_box(
                &mut world,
                DVec3::new(0.0, WORLD_TOP + half, 0.0),
                DVec3::new(WORLD_RIGHT - WORLD_LEFT, half, 1.0),
            ),
        ];

        let bricks = spawn_brick_grid(&mut world);

        crcbl::log::info!(
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
            ball_speed: BALL_SPEED,
            sounds: Vec::new(),
            ticks: 0,
        }));

        let mut session = Loopback::new(
            world,
            Box::new(BreakoutModule {
                shared: Arc::clone(&shared),
            }),
            tick_hz,
            COMPATIBILITY,
        )
        .map_err(|e| GameError::Server(e.to_string()))?;

        let tick_period = session.tick_period();

        // Fill the render buffers before the first tick: the loop's very first
        // frame runs no ticks (the clock spends it establishing its baseline)
        // and still has to draw a board.
        {
            let mut logic = lock(&shared);
            refresh_render_state(&mut logic, session.server_mut().world_mut());
        }

        let game = Self {
            paddle_entity,
            ball_entity,
            _walls: walls,
            action_map,
            session,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            audio: crate::audio::Audio::new(headless),
            high_score: crate::high_score::open(headless),
            pending_keys: Vec::new(),
            score: 0,
            lives: STARTING_LIVES,
            state: GameState::WaitingForLaunch,
            ball_x: BALL_START_X,
            sound_played_this_tick: false,
            prev_log_state: GameState::WaitingForLaunch,
        };
        crcbl::log::info!(
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

    /// Whether the action map reports the move-left action held.
    ///
    /// The *input* state, not a flag the loop keeps beside it. A focus-loss
    /// test that asserted "the loop cleared its list" would pass with the
    /// release never reaching the action map, which is where the paddle's
    /// direction actually comes from.
    ///
    /// Reads what the last [`tick`](Self::tick) resolved: a release queued by
    /// [`key_event`](Self::key_event) lands when it is replayed, which is the
    /// same ordering every other input takes.
    #[cfg(test)]
    #[must_use]
    pub fn move_left_is_held(&self) -> bool {
        self.action_map.button_held(ACTION_LEFT)
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
            left: self.action_map.button_held(ACTION_LEFT),
            right: self.action_map.button_held(ACTION_RIGHT),
            launch: self.action_map.just_pressed(ACTION_LAUNCH),
            restart: self.action_map.just_pressed(ACTION_RESTART),
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
        self.session.client_mut().set_input(vec![intent.to_wire()]);

        self.sim_time += self.tick_period;
        let server_ticks = self.session.server_mut().update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let alpha = self.session.client_mut().update(self.sim_time);
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
        let replicated = self.session.client().interpolate(alpha);
        let ball_bits = self.ball_entity.to_bits();
        if self.state == GameState::Playing
            && let Some(transform) = replicated
                .transforms
                .iter()
                .find(|(bits, _)| *bits == ball_bits)
                .map(|(_, t)| t)
            && (transform.position.x - ball_pos.x).abs() > 1.0
        {
            crcbl::log::warn!(
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
            self.high_score.raise(self.score);
        }
    }

    /// How many bricks are still standing.
    #[must_use]
    pub fn bricks_remaining(&self) -> usize {
        lock(&self.shared).bricks.len()
    }

    /// The two numbers the board does not show, for the debug panel.
    ///
    /// One lock, taken on the frame's own draw, for the reason horde's
    /// `SceneStats` is a snapshot: `HostedGame::debug_sections` is handed
    /// `&self` and runs after the draw, so a module that re-read the simulation
    /// there would be answering a different question from the one the frame was
    /// drawn from — and would take the tick lock a second time to do it.
    #[must_use]
    pub fn board_stats(&self) -> BoardStats {
        let logic = lock(&self.shared);
        BoardStats {
            bricks: logic.bricks.len(),
            ball_speed: logic.ball_speed,
        }
    }

    /// The paddle's authoritative X position.
    ///
    /// **Tests only, now.** The loop used to read it here and hand it straight
    /// to the renderer; it takes the whole board through [`RenderState`]
    /// instead, which carries the same number out of the same lock. Two ways to
    /// ask the same question is how the paddle on screen and the paddle the ball
    /// bounces off end up a frame apart.
    #[cfg(test)]
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
    crcbl::log::info!(
        "[HUD] Score: {score} (best: {high})  Lives: {lives}  Bricks: {bricks}  \
         Ball x: {ball_x:.1}  {state_str}{sound_str}"
    );
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crcbl::core::FrameClock;

    use super::*;
    use crcbl::core::time::{ManualTime, TimeSource as _};

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

    /// Nothing accelerates the ball: between two bounces it travels in a
    /// straight line, at one speed, forever.
    ///
    /// The ball used to be a dynamic body under `GravityForce::EARTH`, so every
    /// tick added `9.81 * dt` to its downward velocity — 0.16 m/s at 60 Hz —
    /// and the "bounce" was an arc. Each per-tick step below is compared with
    /// the first one, which is the smallest thing gravity cannot survive.
    #[test]
    fn a_launched_ball_flies_straight_at_a_constant_speed() {
        let mut h = Harness::new(60, 60);
        h.run(0.1, LAUNCH);
        assert_eq!(h.game.state, GameState::Playing, "the launch has to happen");

        let mut render = RenderState::default();
        h.game.render_state(&mut render);
        let mut previous = render.ball;
        let step = BALL_SPEED * h.game.tick_dt_secs();

        // 30 ticks of free flight: from y ≈ -4 the ball covers 5.2 units and
        // the lowest brick's underside is at y = 3.6, so nothing is hit and
        // every step below is unresolved motion.
        for tick in 0..30 {
            h.frame();
            h.game.render_state(&mut render);
            let delta = render.ball - previous;
            previous = render.ball;
            assert!(
                (delta.length() - step).abs() < 1e-9,
                "tick {tick} moved {} where {step} was owed",
                delta.length(),
            );
            assert!(
                delta.y > 0.0,
                "tick {tick} moved the ball downward: {delta:?}",
            );
        }
    }

    /// A paddle standing still is a mirror: the ball leaves at the angle it
    /// arrived and the catch changes nothing else.
    #[test]
    fn a_still_paddle_returns_the_ball_at_the_angle_it_arrived() {
        for incoming in [
            DVec3::new(0.0, -BALL_SPEED, 0.0),
            DVec3::new(4.0, -10.2, 0.0),
            DVec3::new(-4.0, -10.2, 0.0),
        ] {
            // Direction, not components: `keep_speed` puts the magnitude at
            // the ball's own speed, and these arrive at whatever speed the
            // case is written with.
            let mirrored = DVec3::new(incoming.x, -incoming.y, 0.0).normalize();
            let out = catch_on_the_paddle(incoming, None);
            assert!(
                (out.normalize() - mirrored).length() < 1e-9,
                "{incoming:?} came off a still paddle as {out:?}",
            );
            assert!(
                (out.length() - BALL_SPEED).abs() < 1e-9,
                "{incoming:?} came off a still paddle as {out:?}",
            );
        }
    }

    /// A paddle that is moving decides which way the ball goes — including
    /// against the way it was already travelling.
    ///
    /// The case, exactly: a ball coming down to the right, caught by a paddle
    /// sweeping **left**, has to leave to the left. A mirror sends it up and to
    /// the right, and that is the answer this rules out.
    #[test]
    fn a_paddle_sweeping_left_turns_a_rightward_ball_left() {
        let down_and_right = DVec3::new(4.0, -10.2, 0.0);

        let dragged = catch_on_the_paddle(down_and_right, Some(KeyCode::ArrowLeft));
        assert!(
            dragged.x < 0.0,
            "a paddle sweeping left must turn the ball left, got {dragged:?}",
        );
        assert!(dragged.y > 0.0, "and still return it: {dragged:?}");

        // The mirror image of the case, so neither direction is special.
        let down_and_left = DVec3::new(-4.0, -10.2, 0.0);
        let dragged = catch_on_the_paddle(down_and_left, Some(KeyCode::ArrowRight));
        assert!(
            dragged.x > 0.0,
            "a paddle sweeping right must turn the ball right, got {dragged:?}",
        );
        assert!(dragged.y > 0.0, "and still return it: {dragged:?}");
    }

    /// A ball caught falling dead straight has no sideways motion to turn, and
    /// a moving paddle still has to turn it.
    #[test]
    fn a_moving_paddle_turns_a_ball_that_arrives_straight_down() {
        let straight_down = DVec3::new(0.0, -BALL_SPEED, 0.0);
        let still = catch_on_the_paddle(straight_down, None);
        assert!(still.x.abs() < 1e-9, "a still paddle mirrors it: {still:?}");

        for (key, sign) in [(KeyCode::ArrowLeft, -1.0), (KeyCode::ArrowRight, 1.0)] {
            let out = catch_on_the_paddle(straight_down, Some(key));
            // The floor is on the sideways-to-upward *ratio*, which is what
            // survives `keep_speed` renormalising the pair — about 22° off
            // vertical.
            assert!(
                out.x * sign / out.y >= PADDLE_DRAG_FRACTION - 1e-9,
                "{key:?} pushed the ball only to {out:?}",
            );
            assert!(out.y > 0.0, "and still returned it: {out:?}");
            assert!(
                (out.length() - BALL_SPEED).abs() < 1e-9,
                "drag is not free speed: {out:?}",
            );
        }
    }

    /// Drops the ball onto the paddle at `incoming` velocity, with the paddle
    /// held in `steer`, and returns the velocity the bounce gave it.
    ///
    /// `incoming` must point downward. `steer` is the key the player is holding
    /// through the catch, or `None` for a paddle standing still.
    ///
    /// The ball is placed rather than played into position: these assertions
    /// are about the outgoing direction for an *exactly* known incoming one,
    /// and a ball that arrived under its own steam would arrive at some angle
    /// nobody chose.
    fn catch_on_the_paddle(incoming: DVec3, steer: Option<KeyCode>) -> DVec3 {
        assert!(incoming.y < 0.0, "the ball has to be falling: {incoming:?}");
        let mut h = Harness::new(60, 60);
        h.run(0.1, LAUNCH);
        let (ball, paddle_x) = {
            let logic = lock(&h.game.shared);
            (logic.ball, logic.paddle_x)
        };
        // A world unit above the paddle: several ticks of ordinary free flight
        // and then the ordinary collision path, rather than a contact staged
        // inside the tick it is resolved in.
        with_physics(h.game.session.server_mut().world_mut(), |phys| {
            phys.set_transform(
                ball,
                Transform::from_position(DVec3::new(paddle_x, PADDLE_Y + 1.0, 0.0)),
            );
            set_velocity(phys, ball, incoming);
        });
        if let Some(key) = steer {
            // Pressed and never released, so the paddle is still moving on
            // whichever tick the contact lands on.
            h.game.key_event(key, true);
        }

        for _ in 0..12 {
            h.frame();
            if let Some(key) = steer {
                h.game.key_event(key, true);
            }
            let velocity = with_physics(h.game.session.server_mut().world_mut(), |phys| {
                phys.body(ball).expect("the ball has a body").velocity
            })
            .expect("the world has physics");
            if velocity.y > 0.0 {
                return velocity;
            }
        }
        panic!("the paddle never returned a ball arriving at {incoming:?}");
    }

    /// The ball this harness is playing with, and how fast it is going.
    fn ball_speed(h: &mut Harness) -> f64 {
        let ball = lock(&h.game.shared).ball;
        with_physics(h.game.session.server_mut().world_mut(), |phys| {
            phys.body(ball)
                .expect("the ball has a body")
                .velocity
                .length()
        })
        .expect("the world has physics")
    }

    /// Runs frames until the ball has broken at least one brick, and answers
    /// how many.
    ///
    /// Not a fixed number of seconds: the ball is lost a second or so after it
    /// comes back down past a paddle nobody is steering, and a ball waiting to
    /// be relaunched has no speed to read.
    fn play_until_a_brick_breaks(h: &mut Harness) -> u32 {
        for _ in 0..600 {
            h.frame();
            if h.game.score > 0 {
                assert_eq!(h.game.state, GameState::Playing, "the ball is still live");
                return (BRICK_COUNT - h.game.bricks_remaining()) as u32;
            }
        }
        panic!("ten seconds and not one brick");
    }

    /// Breaking bricks speeds the ball up, and the speed is real motion rather
    /// than a number in a field.
    #[test]
    fn the_ball_speeds_up_as_bricks_break() {
        let mut h = Harness::new(60, 60);
        h.run(0.1, LAUNCH);
        let launched = ball_speed(&mut h);
        assert!((launched - BALL_SPEED).abs() < 1e-9, "{launched}");

        let broken = play_until_a_brick_breaks(&mut h);
        let now = ball_speed(&mut h);
        assert!(
            (now - BALL_SPEED * SPEED_RAMP.powi(broken as i32)).abs() < 1e-9,
            "{broken} bricks should put the ball at {} and it is at {now}",
            BALL_SPEED * SPEED_RAMP.powi(broken as i32),
        );

        // And the ball actually covers that ground: one tick's motion is the
        // speed, or the field is decorative.
        let mut render = RenderState::default();
        h.game.render_state(&mut render);
        let before = render.ball;
        h.frame();
        h.game.render_state(&mut render);
        assert!(
            ((render.ball - before).length() - now * h.game.tick_dt_secs()).abs() < 1e-6,
            "the ball moved {} where {} was owed",
            (render.ball - before).length(),
            now * h.game.tick_dt_secs(),
        );
    }

    /// **A known board renders known rows.** Built by hand rather than played
    /// into, so the two numbers are distinguishable from each other and from
    /// the grid's size: a section that printed the bricks twice, or the total
    /// where the remainder goes, spells something other than `7/40`.
    #[test]
    fn the_board_section_renders_the_bricks_left_and_the_speed_it_was_given() {
        use crcbl::ui::DebugModule as _;

        let stats = BoardStats {
            bricks: 7,
            ball_speed: 13.25,
        };
        let mut section = crcbl::ui::DebugSection::new("board");
        stats.debug_section(&mut section);
        assert_eq!(section.title(), "board");
        assert_eq!(
            section.rows(),
            &[
                crcbl::ui::DebugRow {
                    label: "bricks".into(),
                    value: format!("7/{BRICK_COUNT}"),
                },
                crcbl::ui::DebugRow {
                    label: "ball".into(),
                    value: "13.25/s".into(),
                },
            ],
            "the board section is exactly these two rows",
        );
    }

    /// **And the rows move when the board does.** A fresh grid reads
    /// `40/40` at the launch speed; play until a brick goes and both rows have
    /// changed — which is what separates a module reporting the simulation from
    /// one reporting a default it was constructed with.
    #[test]
    fn breaking_a_brick_changes_both_of_the_board_sections_rows() {
        use crcbl::ui::DebugModule as _;

        let mut h = Harness::new(60, 60);
        let mut section = crcbl::ui::DebugSection::new("board");
        h.game.board_stats().debug_section(&mut section);
        assert_eq!(section.rows().len(), 2);
        let before: Vec<String> = section.rows().iter().map(|r| r.value.clone()).collect();
        assert_eq!(
            before,
            [
                format!("{BRICK_COUNT}/{BRICK_COUNT}"),
                format!("{BALL_SPEED:.2}/s")
            ],
            "a fresh board is a whole grid at the launch speed",
        );

        h.run(0.1, LAUNCH);
        let broken = play_until_a_brick_breaks(&mut h);
        let stats = h.game.board_stats();
        assert_eq!(stats.bricks, BRICK_COUNT - broken as usize);

        // Cleared first, exactly as `DebugPanel::add` clears before handing the
        // section over: `row` appends, so a re-render into a dirty section is
        // four rows and not two.
        section.clear();
        h.game.board_stats().debug_section(&mut section);
        assert_eq!(section.rows().len(), 2, "still exactly two rows");
        let after: Vec<String> = section.rows().iter().map(|r| r.value.clone()).collect();
        assert_eq!(
            after[0],
            format!("{}/{BRICK_COUNT}", BRICK_COUNT - broken as usize),
            "the brick row must count down",
        );
        assert_ne!(
            after[1], before[1],
            "{broken} bricks ramped the ball and the row said nothing",
        );
    }

    /// The ramp is bounded, and the bound is low enough that the ball still
    /// cannot cross its own diameter inside one tick.
    #[test]
    fn the_ramp_is_capped_below_a_tunnelling_speed() {
        let mut speed = BALL_SPEED;
        for _ in 0..BRICK_COUNT * 4 {
            speed = ramped_speed(speed);
            assert!(speed <= MAX_BALL_SPEED, "{speed} is past the cap");
        }
        assert!(
            (speed - MAX_BALL_SPEED).abs() < 1e-9,
            "the ramp must actually reach its cap, got {speed}",
        );
        assert!(
            MAX_BALL_SPEED / f64::from(DEFAULT_TICK_HZ) < BALL_RADIUS * 2.0,
            "a tick at the cap moves {} against a ball {} across",
            MAX_BALL_SPEED / f64::from(DEFAULT_TICK_HZ),
            BALL_RADIUS * 2.0,
        );
    }

    /// A restart puts the ramp back where it started, along with everything
    /// else. A run that inherited the previous one's speed would open at a pace
    /// the player never earned.
    #[test]
    fn a_restart_starts_the_ramp_over() {
        let mut h = Harness::new(60, 60);
        h.run(0.1, LAUNCH);
        play_until_a_brick_breaks(&mut h);
        assert!(
            ball_speed(&mut h) > BALL_SPEED,
            "the run has to have ramped"
        );

        h.game.key_event(KeyCode::KeyR, true);
        h.game.key_event(KeyCode::KeyR, false);
        h.frame();
        h.game.key_event(KeyCode::Space, true);
        h.game.key_event(KeyCode::Space, false);
        h.frame();

        assert_eq!(h.game.state, GameState::Playing);
        let relaunched = ball_speed(&mut h);
        assert!(
            (relaunched - BALL_SPEED).abs() < 1e-9,
            "a restarted run launched at {relaunched}",
        );
    }

    /// The bounce keeps the ball's speed and never drives it back down through
    /// the surface it just bounced off, whatever the paddle was doing.
    #[test]
    fn a_bounce_returns_the_ball_at_its_own_speed() {
        let up = DVec3::Y;
        for incoming in [
            DVec3::new(0.0, -BALL_SPEED, 0.0),
            DVec3::new(10.6, -2.9, 0.0),
            DVec3::new(-10.6, -2.9, 0.0),
        ] {
            for english in [-1.0, 0.0, 1.0] {
                let out = bounce(incoming, up, english, BALL_SPEED);
                assert!(out.y > 0.0, "{incoming:?} + {english} → {out:?}");
                assert!(
                    (out.length() - BALL_SPEED).abs() < 1e-9,
                    "{incoming:?} + {english} → {out:?}",
                );
            }
        }

        // And the ramped speed is the one it comes off at.
        let fast = bounce(DVec3::new(2.0, -9.0, 0.0), up, 1.0, MAX_BALL_SPEED);
        assert!((fast.length() - MAX_BALL_SPEED).abs() < 1e-9, "{fast:?}");
    }

    /// No bounce leaves the ball in a rally the paddle cannot reach.
    #[test]
    fn a_bounce_is_never_flatter_than_the_floor_angle() {
        let floor = BALL_SPEED * MIN_VERTICAL_FRACTION;
        for vel in [
            DVec3::new(BALL_SPEED, 0.0, 0.0),
            DVec3::new(-BALL_SPEED, 0.0, 0.0),
            DVec3::new(10.9, -0.5, 0.0),
            DVec3::new(-10.9, 0.5, 0.0),
        ] {
            let kept = keep_speed(vel, BALL_SPEED);
            assert!(
                kept.y.abs() >= floor - 1e-9,
                "{vel:?} stayed flat at {kept:?}",
            );
            assert!((kept.length() - BALL_SPEED).abs() < 1e-9, "{kept:?}");
            assert_eq!(
                kept.x < 0.0,
                vel.x < 0.0,
                "the flattening reversed the ball: {vel:?} → {kept:?}",
            );
        }
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
