//! Breakout game logic: physics integration, ball/wall/paddle colliders,
//! gravity, collision detection, server/client replication.
//!
//! Server-authoritative physics over in-memory transport. Slice 3 adds
//! `PhysicsSystem` with swept-sphere CCD, rigid bodies, and force-based
//! integration. The ball's transform is replicated from server to client
//! automatically by `PhysicsSystem::replicate()`.

use std::time::Duration;

use crcbl_client::Client;
use crcbl_core::input::KeyCode;
use crcbl_ecs::{Entity, GameModule, World};
use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl_net::{ClientToServer, InMemoryTransport, ProtocolCompatibility};
use crcbl_phys::{ColliderComponent, GravityForce, PhysicsSystem, RigidBody, Transform};
use crcbl_server::Server;
use glam::DVec3;

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0050_335F_4252,
};

const TICK_HZ: u32 = 60;
const PADDLE_SPEED: f64 = 12.0;
pub const PADDLE_HALF_WIDTH: f64 = 5.0;
pub const PADDLE_Y: f64 = -8.0;
const WORLD_LEFT: f64 = -14.0;
const WORLD_RIGHT: f64 = 14.0;
const WORLD_TOP: f64 = 9.0;
const BALL_RADIUS: f64 = 0.3;
const BALL_START_X: f64 = 0.0;
const BALL_START_Y: f64 = -5.0;
const BALL_SPEED_X: f64 = 3.0;
const BALL_SPEED_Y: f64 = 5.0;

const ACTION_LEFT: &str = "move_left";
const ACTION_RIGHT: &str = "move_right";
const ACTION_LAUNCH: &str = "launch";

/// Brick grid layout.
const BRICK_ROWS: usize = 4;
const BRICK_COLS: usize = 10;
const BRICK_WIDTH: f64 = 2.4;
const BRICK_HEIGHT: f64 = 0.8;
const BRICK_GAP: f64 = 0.2;
const BRICK_TOP: f64 = 7.0;
const BRICK_LEFT: f64 = -(BRICK_COLS as f64 * (BRICK_WIDTH + BRICK_GAP)) / 2.0 + BRICK_WIDTH / 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    WaitingForLaunch,
    Playing,
    Won,
    Lost,
}

pub struct Game {
    pub paddle_entity: Entity,
    pub ball_entity: Entity,
    _wall_left: Entity,
    _wall_right: Entity,
    _wall_top: Entity,
    bricks: Vec<Entity>,
    action_map: ActionMap,
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    paddle_x: f64,
    launched: bool,
    pub score: u32,
    pub lives: u32,
    pub state: GameState,
    pub audio: crate::audio::Audio,
    /// Current ball X for spatial audio panning.
    pub ball_x: f64,
    /// Whether a collision sound was queued this tick (for HUD logging).
    sound_played_this_tick: bool,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("paddle_entity", &self.paddle_entity)
            .field("ball_entity", &self.ball_entity)
            .field("paddle_x", &self.paddle_x)
            .field("launched", &self.launched)
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

/// Per-tick game logic run after the ECS physics schedule.
struct BreakoutModule;

impl GameModule for BreakoutModule {
    fn name(&self) -> &str {
        "breakout"
    }

    fn register(&self, _world: &mut World) {}
}

fn reset_ball(phys: &mut PhysicsSystem, ball: Entity) {
    phys.set_transform(
        ball,
        Transform::from_position(DVec3::new(BALL_START_X, BALL_START_Y, 0.0)),
    );
    if let Some(body) = phys.body(ball) {
        let mut new_body = *body;
        new_body.velocity = DVec3::ZERO;
        phys.set_body(ball, new_body);
    }
}

impl Game {
    pub fn new(headless: bool) -> Result<Self, GameError> {
        let mut world = World::new();

        // --- Physics system ---
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));

        // Ball: dynamic, sphere collider.
        let ball_entity = world.spawn();
        phys.set_body(ball_entity, RigidBody::new_dynamic(1.0));
        phys.set_transform(
            ball_entity,
            Transform::from_position(DVec3::new(BALL_START_X, BALL_START_Y, 0.0)),
        );
        phys.set_collider(
            ball_entity,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: BALL_RADIUS,
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(BALL_START_X, BALL_START_Y, 0.0)),
        );

        // Paddle: kinematic, box collider.
        let paddle_entity = world.spawn();
        phys.set_body(paddle_entity, RigidBody::new_kinematic());
        phys.set_transform(
            paddle_entity,
            Transform::from_position(DVec3::new(0.0, PADDLE_Y, 0.0)),
        );
        phys.set_collider(
            paddle_entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: DVec3::new(PADDLE_HALF_WIDTH, 0.3, 0.5),
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(0.0, PADDLE_Y, 0.0)),
        );

        // Walls: static (kinematic with no velocity), box colliders.
        let wall_left = world.spawn();
        phys.set_body(wall_left, RigidBody::new_kinematic());
        phys.set_transform(
            wall_left,
            Transform::from_position(DVec3::new(WORLD_LEFT - 0.5, 0.0, 0.0)),
        );
        phys.set_collider(
            wall_left,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: DVec3::new(0.5, WORLD_TOP, 1.0),
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(WORLD_LEFT - 0.5, 0.0, 0.0)),
        );

        let wall_right = world.spawn();
        phys.set_body(wall_right, RigidBody::new_kinematic());
        phys.set_transform(
            wall_right,
            Transform::from_position(DVec3::new(WORLD_RIGHT + 0.5, 0.0, 0.0)),
        );
        phys.set_collider(
            wall_right,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: DVec3::new(0.5, WORLD_TOP, 1.0),
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(WORLD_RIGHT + 0.5, 0.0, 0.0)),
        );

        let wall_top = world.spawn();
        phys.set_body(wall_top, RigidBody::new_kinematic());
        phys.set_transform(
            wall_top,
            Transform::from_position(DVec3::new(0.0, WORLD_TOP + 0.5, 0.0)),
        );
        phys.set_collider(
            wall_top,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: DVec3::new(WORLD_RIGHT - WORLD_LEFT, 0.5, 1.0),
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(0.0, WORLD_TOP + 0.5, 0.0)),
        );

        // --- Bricks ---
        let mut bricks = Vec::with_capacity(BRICK_ROWS * BRICK_COLS);
        for row in 0..BRICK_ROWS {
            for col in 0..BRICK_COLS {
                let bx = BRICK_LEFT + col as f64 * (BRICK_WIDTH + BRICK_GAP);
                let by = BRICK_TOP - row as f64 * (BRICK_HEIGHT + BRICK_GAP);
                let entity = world.spawn();
                phys.set_body(entity, RigidBody::new_kinematic());
                phys.set_transform(entity, Transform::from_position(DVec3::new(bx, by, 0.0)));
                phys.set_collider(
                    entity,
                    &ColliderComponent::Box {
                        offset: DVec3::ZERO,
                        half_extents: DVec3::new(BRICK_WIDTH / 2.0, BRICK_HEIGHT / 2.0, 0.5),
                        is_trigger: false,
                    },
                    &Transform::from_position(DVec3::new(bx, by, 0.0)),
                );
                bricks.push(entity);
            }
        }

        world.register_system(Box::new(phys));

        // --- Input ---
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

        // --- Transport ---
        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server =
            Server::try_new_with_compatibility(world, server_transport, TICK_HZ, COMPATIBILITY)
                .map_err(|e| GameError::Server(e.to_string()))?;

        server.set_module(Box::new(BreakoutModule));

        let client =
            Client::new_with_compatibility(World::new(), client_transport, TICK_HZ, COMPATIBILITY);

        Ok(Self {
            paddle_entity,
            ball_entity,
            _wall_left: wall_left,
            _wall_right: wall_right,
            _wall_top: wall_top,
            bricks,
            action_map,
            server,
            client,
            paddle_x: 0.0,
            launched: false,
            score: 0,
            lives: 3,
            state: GameState::WaitingForLaunch,
            audio: crate::audio::Audio::new(headless),
            ball_x: 0.0,
            sound_played_this_tick: false,
        })
    }

    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.action_map.key_event(key, pressed);
    }

    pub fn step(&mut self, now: Duration) -> f64 {
        self.action_map.begin_tick(1.0 / TICK_HZ as f32);

        let left = action_held(&self.action_map, ACTION_LEFT);
        let right = action_held(&self.action_map, ACTION_RIGHT);
        let launch = action_just_pressed(&self.action_map, ACTION_LAUNCH);

        // Handle game state transitions.
        match self.state {
            GameState::WaitingForLaunch => {
                if launch {
                    self.state = GameState::Playing;
                    self.launched = true;
                    set_ball_velocity(
                        &mut self.server,
                        self.ball_entity,
                        DVec3::new(BALL_SPEED_X, BALL_SPEED_Y, 0.0),
                    );
                }
            }
            GameState::Won | GameState::Lost => {
                if launch {
                    // Restart.
                    restart_game(&mut self.server, self.ball_entity, self.paddle_entity);
                    self.state = GameState::WaitingForLaunch;
                    self.launched = false;
                    self.score = 0;
                    self.lives = 3;
                }
            }
            GameState::Playing => { /* handled by BreakoutModule */ }
        }

        if left {
            self.paddle_x -= PADDLE_SPEED * (1.0 / TICK_HZ as f64);
        }
        if right {
            self.paddle_x += PADDLE_SPEED * (1.0 / TICK_HZ as f64);
        }
        self.paddle_x = self.paddle_x.clamp(
            WORLD_LEFT + PADDLE_HALF_WIDTH,
            WORLD_RIGHT - PADDLE_HALF_WIDTH,
        );

        // Sync paddle position.
        set_paddle_position(&mut self.server, self.paddle_entity, self.paddle_x);

        // Send input, advance simulation.
        let input_bytes = crcbl_net::encode_client_to_server(&ClientToServer::Input {
            tick: Default::default(),
            data: vec![],
        });
        self.client.set_input(input_bytes);
        self.server.update(now);
        let alpha = self.client.update(now);

        // Run game logic (collision, scoring, lives).
        self.sound_played_this_tick = false;
        if self.state == GameState::Playing {
            let server = &mut self.server;
            let ball = self.ball_entity;
            let paddle = self.paddle_entity;
            let mut ctx = GameCtx {
                bricks: &mut self.bricks,
                score: &mut self.score,
                lives: &mut self.lives,
                state: &mut self.state,
                launched: &mut self.launched,
                ball_x: &mut self.ball_x,
            };
            let audio = &mut self.audio;
            self.sound_played_this_tick = run_game_logic(server, ball, paddle, &mut ctx, audio);
        }

        // HUD: log score/lives when they change or state transitions.
        log_hud(
            &self.state,
            self.score,
            self.lives,
            self.sound_played_this_tick,
        );

        let _ = alpha;
        self.paddle_x
    }
}

// ---- physics helpers ----

fn with_physics(server: &mut Server<InMemoryTransport>, f: impl FnOnce(&mut PhysicsSystem)) {
    let schedule = server.world_mut().schedule_mut();
    for sys in schedule.iter_mut() {
        if sys.name() == "physics"
            && let Some(phys) = sys.as_any_mut().downcast_mut::<PhysicsSystem>()
        {
            f(phys);
            return;
        }
    }
}

fn set_paddle_position(server: &mut Server<InMemoryTransport>, entity: Entity, x: f64) {
    with_physics(server, |phys| {
        if let Some(t) = phys.transform(entity) {
            let mut new_t = *t;
            new_t.position.x = x;
            new_t.position.y = PADDLE_Y;
            phys.set_transform(entity, new_t);
        }
    });
}

fn set_ball_velocity(server: &mut Server<InMemoryTransport>, entity: Entity, velocity: DVec3) {
    with_physics(server, |phys| {
        if let Some(body) = phys.body(entity) {
            let mut new_body = *body;
            new_body.velocity = velocity;
            phys.set_body(entity, new_body);
        }
    });
}

/// Mutable game state passed to collision/detection logic.
struct GameCtx<'a> {
    bricks: &'a mut Vec<Entity>,
    score: &'a mut u32,
    lives: &'a mut u32,
    state: &'a mut GameState,
    launched: &'a mut bool,
    ball_x: &'a mut f64,
}

/// Run the game logic (collision, scoring, lives) after a server tick.
fn run_game_logic(
    server: &mut Server<InMemoryTransport>,
    ball_entity: Entity,
    paddle_entity: Entity,
    ctx: &mut GameCtx<'_>,
    audio: &mut crate::audio::Audio,
) -> bool {
    let mut to_despawn: Vec<Entity> = Vec::new();
    let mut sound_played = false;

    {
        let world = server.world_mut();
        let schedule = world.schedule_mut();
        for sys in schedule.iter_mut() {
            if sys.name() == "physics"
                && let Some(phys) = sys.as_any_mut().downcast_mut::<PhysicsSystem>()
            {
                // Read ball X position for spatial audio.
                if let Some(transform) = phys.transform(ball_entity) {
                    *ctx.ball_x = transform.position.x;
                }

                let dt = 1.0 / TICK_HZ as f64;

                let hit = if let (Some(body), Some(transform)) =
                    (phys.body(ball_entity), phys.transform(ball_entity))
                {
                    let pos = transform.position;
                    let segment = crcbl_phys::Segment {
                        start: pos,
                        end: pos + body.velocity * dt,
                    };
                    phys.sweep_sphere(&segment, BALL_RADIUS)
                } else {
                    None
                };

                if let Some((hit_entity, hit_info)) = hit {
                    // Copy out body/transform before mutating.
                    let (body_val, transform_val, vel, normal) = {
                        if let (Some(body), Some(transform)) =
                            (phys.body(ball_entity), phys.transform(ball_entity))
                        {
                            (*body, *transform, body.velocity, hit_info.normal)
                        } else {
                            continue;
                        }
                    };

                    if let Some(idx) = ctx.bricks.iter().position(|&e| e == hit_entity) {
                        ctx.bricks.swap_remove(idx);
                        *ctx.score += 10;
                        to_despawn.push(hit_entity);
                        audio.play_panned(crate::audio::SOUND_BRICK, *ctx.ball_x as f32);
                        sound_played = true;
                        if vel.dot(normal) < 0.0 {
                            let reflected = vel - 2.0 * vel.dot(normal) * normal;
                            let mut new_body = body_val;
                            new_body.velocity = reflected;
                            phys.set_body(ball_entity, new_body);
                            let mut new_transform = transform_val;
                            new_transform.position = hit_info.point + normal * BALL_RADIUS * 1.01;
                            phys.set_transform(ball_entity, new_transform);
                        }
                    } else if vel.dot(normal) < 0.0 {
                        let reflected = vel - 2.0 * vel.dot(normal) * normal;
                        let mut new_body = body_val;
                        new_body.velocity = reflected;
                        phys.set_body(ball_entity, new_body);
                        let mut new_transform = transform_val;
                        new_transform.position = hit_info.point + normal * BALL_RADIUS * 1.01;
                        phys.set_transform(ball_entity, new_transform);
                        audio.play_panned(crate::audio::SOUND_BOUNCE, *ctx.ball_x as f32);
                        sound_played = true;
                    }
                }

                // Ball fell below paddle.
                if *ctx.state == GameState::Playing
                    && let Some(transform) = phys.transform(ball_entity)
                    && transform.position.y < PADDLE_Y - 2.0
                {
                    *ctx.lives = ctx.lives.saturating_sub(1);
                    if *ctx.lives == 0 {
                        *ctx.state = GameState::Lost;
                        *ctx.launched = false;
                    } else {
                        reset_ball(phys, ball_entity);
                        *ctx.launched = false;
                        *ctx.state = GameState::WaitingForLaunch;
                    }
                }

                if *ctx.state == GameState::Playing && ctx.bricks.is_empty() {
                    *ctx.state = GameState::Won;
                }

                let _ = paddle_entity;
            }
        }
    } // drop schedule and world borrow

    // Despawn hit bricks after the physics borrow is released.
    for entity in to_despawn {
        server.world_mut().despawn(entity);
    }

    sound_played
}

fn restart_game(
    server: &mut Server<InMemoryTransport>,
    ball_entity: Entity,
    _paddle_entity: Entity,
) {
    with_physics(server, |phys| {
        reset_ball(phys, ball_entity);
    });
}

// ---- HUD (console-based until UI compositing pass exists) ----

fn log_hud(state: &GameState, score: u32, lives: u32, sound_played: bool) {
    let state_str = match state {
        GameState::WaitingForLaunch => "WAITING — press Space to launch",
        GameState::Playing => "PLAYING",
        GameState::Won => "YOU WIN! — press Space to restart",
        GameState::Lost => "GAME OVER — press Space to restart",
    };
    let sound_str = if sound_played { " 🔊" } else { "" };
    log::info!("[HUD] Score: {score}  Lives: {lives}  {state_str}{sound_str}");
}

// ---- input helpers ----

fn action_held(map: &ActionMap, name: &str) -> bool {
    map.action(name)
        .map(|v| match v {
            crcbl_input::ActionValue::Button(b) => matches!(
                b.state,
                crcbl_input::ButtonState::Pressed | crcbl_input::ButtonState::Held { .. }
            ),
            _ => false,
        })
        .unwrap_or(false)
}

fn action_just_pressed(map: &ActionMap, name: &str) -> bool {
    map.action(name)
        .map(|v| match v {
            crcbl_input::ActionValue::Button(b) => b.just_pressed,
            _ => false,
        })
        .unwrap_or(false)
}
