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

pub struct Game {
    pub paddle_entity: Entity,
    pub ball_entity: Entity,
    _wall_left: Entity,
    _wall_right: Entity,
    _wall_top: Entity,
    action_map: ActionMap,
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    paddle_x: f64,
    launched: bool,
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
struct BreakoutModule {
    ball_entity: Entity,
    paddle_entity: Entity,
    wall_left: Entity,
    wall_right: Entity,
    wall_top: Entity,
}

impl GameModule for BreakoutModule {
    fn name(&self) -> &str {
        "breakout"
    }

    fn register(&self, _world: &mut World) {
        // Systems are already registered in Game::new before the module is set.
    }

    fn tick(&mut self, world: &mut World) {
        let schedule = world.schedule_mut();
        for sys in schedule.iter_mut() {
            if sys.name() == "physics"
                && let Some(phys) = sys.as_any_mut().downcast_mut::<PhysicsSystem>()
            {
                handle_collisions(
                    phys,
                    self.ball_entity,
                    self.paddle_entity,
                    self.wall_left,
                    self.wall_right,
                    self.wall_top,
                );
            }
        }
    }
}

/// Sweep the ball forward and reflect velocity off any hit surface.
fn handle_collisions(
    phys: &mut PhysicsSystem,
    ball: Entity,
    paddle: Entity,
    wall_left: Entity,
    wall_right: Entity,
    wall_top: Entity,
) {
    // Copy body and transform values, then drop the immutable borrows.
    let (body_val, transform_val) = {
        let body = match phys.body(ball) {
            Some(b) => *b,
            None => return,
        };
        let transform = match phys.transform(ball) {
            Some(t) => *t,
            None => return,
        };
        (body, transform)
    };

    let dt = 1.0 / TICK_HZ as f64;
    let vel = body_val.velocity;
    let pos = transform_val.position;

    // Sweep the ball along its velocity (CCD).
    let segment = crcbl_phys::Segment {
        start: pos,
        end: pos + vel * dt,
    };

    if let Some((_hit_entity, hit)) = phys.sweep_sphere(&segment, BALL_RADIUS) {
        let normal = hit.normal;
        if vel.dot(normal) < 0.0 {
            let reflected = vel - 2.0 * vel.dot(normal) * normal;
            let mut new_body = body_val;
            new_body.velocity = reflected;
            phys.set_body(ball, new_body);

            let mut new_transform = transform_val;
            new_transform.position = hit.point + normal * BALL_RADIUS * 1.01;
            phys.set_transform(ball, new_transform);

            log::debug!(
                "ball hit {:?}, normal={:?}, vel_before={:?}, vel_after={:?}",
                _hit_entity,
                normal,
                vel,
                reflected,
            );
        }
    }

    let _ = paddle;
    let _ = wall_left;
    let _ = wall_right;
    let _ = wall_top;
}

impl Game {
    pub fn new() -> Result<Self, GameError> {
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

        server.set_module(Box::new(BreakoutModule {
            ball_entity,
            paddle_entity,
            wall_left,
            wall_right,
            wall_top,
        }));

        let client =
            Client::new_with_compatibility(World::new(), client_transport, TICK_HZ, COMPATIBILITY);

        Ok(Self {
            paddle_entity,
            ball_entity,
            _wall_left: wall_left,
            _wall_right: wall_right,
            _wall_top: wall_top,
            action_map,
            server,
            client,
            paddle_x: 0.0,
            launched: false,
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

        // Launch ball.
        if launch && !self.launched {
            self.launched = true;
            set_ball_velocity(
                &mut self.server,
                self.ball_entity,
                DVec3::new(BALL_SPEED_X, BALL_SPEED_Y, 0.0),
            );
        }

        // Sync paddle position to physics system.
        set_paddle_position(&mut self.server, self.paddle_entity, self.paddle_x);

        // Send input.
        let input_bytes = crcbl_net::encode_client_to_server(&ClientToServer::Input {
            tick: Default::default(),
            data: vec![],
        });
        self.client.set_input(input_bytes);

        // Advance simulation.
        self.server.update(now);
        let alpha = self.client.update(now);

        // Log ball position from interpolated state.
        let state = self.client.interpolate(alpha);
        let ball_bits = self.ball_entity.to_bits();
        for (entity_bits, transform) in state.transforms {
            if entity_bits == ball_bits {
                log::debug!(
                    "ball pos: ({:.2}, {:.2}) vel: {:?}",
                    transform.position.x,
                    transform.position.y,
                    "from snapshot",
                );
            }
        }

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
