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
    /// Ticks the module has actually run. The facade asserts this advances by
    /// exactly one per [`Game::tick`].
    ticks: u64,
}

impl GameLogic {
    fn reset_run(&mut self) {
        self.state = GameState::WaitingToStart;
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
    let intent = std::mem::take(&mut logic.intent);
    let bird = logic.bird;

    // --- what the button meant this tick --------------------------------
    match logic.state {
        _ if intent.restart => restart(logic, world),
        GameState::Dead if intent.flap => restart(logic, world),
        GameState::WaitingToStart if intent.flap => {
            logic.state = GameState::Playing;
            with_physics(world, |phys| {
                set_velocity(phys, bird, DVec3::new(SCROLL_SPEED, FLAP_SPEED, 0.0));
            });
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
        }
        _ => {}
    }

    // --- hold the bird still until the run starts ------------------------
    if logic.state != GameState::Playing {
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

    refresh_render_state(logic, world);
}

/// Puts the run back to its opening position.
fn restart(logic: &mut GameLogic, world: &mut World) {
    logic.reset_run();
    let bird = logic.bird;
    with_physics(world, |phys| park_bird(phys, bird));
}

/// Copies the authoritative bird state the renderer needs out of the physics
/// world.
fn refresh_render_state(logic: &mut GameLogic, world: &mut World) {
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

/// Everything the renderer needs for one frame, in world space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderState {
    pub bird: DVec3,
    pub bird_velocity: DVec3,
    pub state: Option<GameState>,
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
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub state: GameState,
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
    pub fn new(tick_hz: u32) -> Result<Self, GameError> {
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
            ticks: 0,
        }));

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
            state: GameState::WaitingToStart,
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

        let (state, bird, velocity, ticks_after) = {
            let logic = lock(&self.shared);
            (logic.state, logic.bird_pos, logic.bird_vel, logic.ticks)
        };
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        self.state = state;
        self.bird = bird;
        self.bird_velocity = velocity;

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        if state_changed || self.ticks_run.is_multiple_of(60) {
            log::info!(
                "[HUD] {:?}  x: {:.1}  y: {:.1}  vy: {:+.1}",
                self.state,
                self.bird.x,
                self.bird.y,
                self.bird_velocity.y,
            );
        }
    }

    /// Everything the renderer draws, in world space.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        let logic = lock(&self.shared);
        RenderState {
            bird: logic.bird_pos,
            bird_velocity: logic.bird_vel,
            state: Some(logic.state),
        }
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
                game: Game::new(tick_hz).expect("a headless game always starts"),
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
        // Flap every few ticks for long enough to pin the bird against the top.
        let mut script = Vec::new();
        for tick in (0..240).step_by(4) {
            script.push((tick, KeyCode::Space, true));
            script.push((tick + 1, KeyCode::Space, false));
        }
        harness.run(4.0, &script);

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
