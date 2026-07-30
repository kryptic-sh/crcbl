//! Breakout game logic: input capture, server/client roundtrip, paddle state.
//!
//! Server-authoritative over in-memory transport. For Slice 2, paddle movement
//! is local (no server-side ECS replication of transforms yet — Slice 3 adds
//! physics). The server/client wire-up proves the architecture compiles and
//! initialises correctly.

use std::time::Duration;

use crcbl_client::Client;
use crcbl_core::input::KeyCode;
use crcbl_ecs::{Entity, World};
use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl_net::{ClientToServer, InMemoryTransport, ProtocolCompatibility};
use crcbl_server::Server;

/// Protocol compatibility values, explicit for a real (non-test) build.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0042_524B_4F55,
};

const TICK_HZ: u32 = 60;
const PADDLE_SPEED: f64 = 12.0;
pub const PADDLE_HALF_WIDTH: f64 = 5.0;
const WORLD_LEFT: f64 = -14.0;
const WORLD_RIGHT: f64 = 14.0;

/// The breakout game: owns input, server, client, and the game loop.
///
/// Debug is manual because some inner types do not implement Debug.
pub struct Game {
    pub paddle_entity: Entity,
    action_map: ActionMap,
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    /// Paddle X position, tracked locally (Slice 3 moves this to server ECS).
    paddle_x: f64,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("paddle_entity", &self.paddle_entity)
            .field("paddle_x", &self.paddle_x)
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
    pub fn new() -> Result<Self, GameError> {
        // Create an empty ECS world. Slice 3 adds PaddleSystem, BallSystem,
        // BrickSystem through the physics layer.
        let mut world = World::new();
        let paddle_entity = world.spawn();

        // Input bindings.
        let mut action_map = ActionMap::new();
        action_map.declare(ActionDecl {
            name: "move_left".into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::ArrowLeft)],
        });
        action_map.declare(ActionDecl {
            name: "move_right".into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::ArrowRight)],
        });

        let (server_transport, client_transport) = InMemoryTransport::pair();

        let server =
            Server::try_new_with_compatibility(world, server_transport, TICK_HZ, COMPATIBILITY)
                .map_err(|e| GameError::Server(e.to_string()))?;

        let client =
            Client::new_with_compatibility(World::new(), client_transport, TICK_HZ, COMPATIBILITY);

        Ok(Self {
            paddle_entity,
            action_map,
            server,
            client,
            paddle_x: 0.0,
        })
    }

    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.action_map.key_event(key, pressed);
    }

    /// Advance the simulation by `now` wall time. Returns the paddle X for this
    /// frame.
    pub fn step(&mut self, now: Duration) -> f64 {
        self.action_map.begin_tick(1.0 / TICK_HZ as f32);

        // Read input and move paddle locally.
        let left = self
            .action_map
            .action("move_left")
            .map(|v| match v {
                crcbl_input::ActionValue::Button(b) => matches!(
                    b.state,
                    crcbl_input::ButtonState::Pressed | crcbl_input::ButtonState::Held { .. }
                ),
                _ => false,
            })
            .unwrap_or(false);
        let right = self
            .action_map
            .action("move_right")
            .map(|v| match v {
                crcbl_input::ActionValue::Button(b) => matches!(
                    b.state,
                    crcbl_input::ButtonState::Pressed | crcbl_input::ButtonState::Held { .. }
                ),
                _ => false,
            })
            .unwrap_or(false);

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

        // Send empty input to keep the client transport flowing.
        let input_bytes = crcbl_net::encode_client_to_server(&ClientToServer::Input {
            tick: Default::default(),
            data: vec![],
        });
        self.client.set_input(input_bytes);

        // Advance server and client (proves the handshake flows).
        self.server.update(now);
        self.client.update(now);

        self.paddle_x
    }
}
