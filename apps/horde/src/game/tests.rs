use crcbl::core::FrameClock;

use super::*;
use crcbl::core::TickId;
use crcbl::core::time::{ManualTime, TimeSource as _};

/// One entry of a script: `(tick index, key, pressed)`.
type Script = [(u64, KeyCode, bool)];

/// How many ticks one lap of the autopilot's kite takes.
///
/// Ten seconds at [`DEFAULT_TICK_HZ`], which at [`PLAYER_SPEED`] is a circle
/// of about eleven units' radius — comfortably inside the arena, and long
/// enough that the grunts left behind at the middle of it catch up.
const KITE_PERIOD: u64 = 600;

/// Drives a `Game` the way the app loop will — a frame clock at `frame_hz`,
/// a fixed-timestep accumulator at `tick_hz`, and events pumped once per
/// frame.
///
/// The frame rate and the tick rate are independent knobs on purpose. Every
/// property asserted below is a property of *simulated* time, and a loop
/// that leaked the frame rate into the simulation is what makes them
/// disagree.
struct Harness {
    game: Game,
    clock: FrameClock,
    time: ManualTime,
    frame_step: Duration,
    ticks: u64,
    /// What the autopilot currently has held down, in
    /// `[up, down, left, right]` order. Movement is a *held* action, so a
    /// controller that pressed and released every tick would be testing the
    /// edge detector rather than the player.
    held: [bool; 4],
    /// How many times `Harness::play_ticks` has restarted a finished run.
    ///
    /// A restart is the largest single piece of churn this game has — it
    /// wipes the whole field — so a soak that never reached one has not
    /// tested the path.
    restarts: u32,
    /// How many level-up screens `Harness::play_ticks` has answered, for the
    /// same reason: a soak that never opened one never froze the field.
    levels: u32,
}

/// Indices into [`Harness::held`], and the key each one drives.
const HELD_KEYS: [KeyCode; 4] = [KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD];

impl Harness {
    fn new(frame_hz: u32, tick_hz: u32) -> Self {
        Self::with_setup(
            frame_hz,
            &Setup {
                tick_hz,
                ..Setup::default()
            },
        )
    }

    fn with_setup(frame_hz: u32, setup: &Setup) -> Self {
        let mut harness = Self::at_the_title_screen(frame_hz, setup);
        // **Every harness below starts on tick 0 rather than on the title
        // screen**, so the tick indices its scripts are keyed on still mean
        // what they meant before the start screen existed. The edge is
        // *queued*, not poked: it is replayed into the action map by the
        // first `Game::tick`, which consumes it and plays the whole of that
        // tick — so tick 0 is a playing tick, exactly as it was.
        //
        // That also makes this the suite's widest check on the start path.
        // A start edge the simulation stopped honouring leaves every test
        // below looking at a frozen arena.
        harness.game.key_event(KeyCode::Space, true);
        harness.game.key_event(KeyCode::Space, false);
        harness
    }

    /// The same, left on the title screen — for the handful of tests that
    /// are *about* the title screen.
    fn at_the_title_screen(frame_hz: u32, setup: &Setup) -> Self {
        Self {
            game: Game::with_setup(setup).expect("a headless game always starts"),
            clock: FrameClock::new(setup.tick_hz),
            time: ManualTime::new(),
            frame_step: FrameClock::new(frame_hz).tick_dt(),
            ticks: 0,
            held: [false; 4],
            restarts: 0,
            levels: 0,
        }
    }

    /// A game left on the title screen, with its spawner live — the state a
    /// player's window opens in.
    fn waiting(frame_hz: u32, tick_hz: u32) -> Self {
        Self::at_the_title_screen(
            frame_hz,
            &Setup {
                tick_hz,
                ..Setup::default()
            },
        )
    }

    /// A staged board: no spawner, no enemies, the player where it is asked
    /// for. Every mechanism test starts from this, so a change to the spawn
    /// ramp cannot silently move one of them.
    fn staged(frame_hz: u32, tick_hz: u32, player: DVec3) -> Self {
        Self::staged_with_seed(frame_hz, tick_hz, player, DEFAULT_SEED)
    }

    /// [`Self::staged`] at a seed the caller chooses, for the tests that
    /// need a particular prop layout — the movement tests stage at
    /// [`MOVEMENT_CORRIDOR_SEED`] so their corridors are clear by
    /// arrangement.
    fn staged_with_seed(frame_hz: u32, tick_hz: u32, player: DVec3, seed: u64) -> Self {
        let mut harness = Self::with_setup(
            frame_hz,
            &Setup {
                tick_hz,
                seed,
                ..Setup::default()
            },
        );
        harness.game.freeze_spawns();
        harness.game.clear_enemies();
        harness.game.stage_player(player);
        harness
    }

    /// One frame: advance the clock, drain whole ticks, exactly as the app
    /// loop does — stopping at `limit` so a caller counting ticks is not at
    /// the mercy of how many a single frame happened to release.
    ///
    /// The script is keyed on the **tick** index and fed immediately before
    /// that tick runs, so the input a given tick sees is the same at every
    /// frame rate.
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
    fn run_ticks(&mut self, ticks: u64, script: &Script) {
        while self.ticks < ticks {
            self.frame(script, ticks);
        }
    }

    /// Presses or releases a held key only when its state has to change.
    fn hold(&mut self, slot: usize, want: bool) {
        if self.held[slot] != want {
            self.game.key_event(HELD_KEYS[slot], want);
            self.held[slot] = want;
        }
    }

    /// The bookkeeping a driver runs before each tick: restart a finished
    /// run, answer a level-up screen, and count both.
    ///
    /// **A restart is two edges, one tick apart**, because it lands on the
    /// title screen and the title screen is left by the same key. Only the
    /// first is counted: the second is a start, not a restart. The edge is
    /// pressed and released inside the one tick, and it is the harness that
    /// does it rather than `autopilot`, because the plan is a set of held
    /// keys and this is not one. **A level-up screen has to be answered or
    /// a soak stops.** The field freezes while it is up and the spawner
    /// does not run, so a driver that walked past it would measure a frozen
    /// field for the rest of the run — which is exactly the shape of a soak
    /// that silently tests nothing.
    fn settle(&mut self) {
        if self.game.state == GameState::Dead {
            self.restarts += 1;
        }
        if matches!(self.game.state, GameState::Dead | GameState::WaitingToStart) {
            self.game.key_event(KeyCode::KeyR, true);
            self.game.key_event(KeyCode::KeyR, false);
        }
        if self.game.state == GameState::LevelUp {
            self.levels += 1;
            self.game.key_event(KeyCode::Digit1, true);
            self.game.key_event(KeyCode::Digit1, false);
        }
    }

    /// Runs to `ticks` under the autopilot — a player who kites.
    fn play_ticks(&mut self, ticks: u64) {
        self.play_ticks_into(ticks, |_| {});
    }

    /// The tick loop behind [`Self::play_ticks`], calling `observe` with the
    /// game after every tick.
    ///
    /// The observer is how the determinism gate records a state hash per
    /// tick, so a divergence is reported at the tick it happened rather than
    /// at the end of the run. A no-op closure per tick is the cost every
    /// other soak pays for the one that wants the record.
    fn play_ticks_into(&mut self, ticks: u64, mut observe: impl FnMut(&Game)) {
        while self.ticks < ticks {
            self.time.advance(self.frame_step);
            self.clock.update(self.time.elapsed());
            while self.ticks < ticks && self.clock.consume_tick() {
                self.settle();
                let plan = autopilot(&self.game, self.ticks);
                for (slot, want) in plan.iter().copied().enumerate() {
                    self.hold(slot, want);
                }
                self.game.tick();
                self.ticks += 1;
                observe(&self.game);
            }
        }
    }

    /// Runs to `ticks` while the player stands still — the same bookkeeping
    /// as [`Self::play_ticks`], but no movement key is ever pressed.
    ///
    /// The horde converges on a stationary player, which is what ends a run
    /// the kite now survives: spawns arrive on the ring inside the arena
    /// rather than materialising beside the player, so nothing catches a
    /// player who keeps moving.
    fn stand_still(&mut self, ticks: u64) {
        // The autopilot may have left a movement key held by a previous
        // stretch.
        for slot in 0..HELD_KEYS.len() {
            self.hold(slot, false);
        }
        while self.ticks < ticks {
            self.time.advance(self.frame_step);
            self.clock.update(self.time.elapsed());
            while self.ticks < ticks && self.clock.consume_tick() {
                self.settle();
                self.game.tick();
                self.ticks += 1;
            }
        }
    }

    /// A restart, all the way back into play.
    ///
    /// **Two edges and two ticks**, because `restart` lands on the title
    /// screen: the first clears the run, the second leaves the screen. A
    /// test that wants to *see* the title screen taps once instead.
    fn restart_run(&mut self) {
        self.tap(KeyCode::KeyR);
        assert_eq!(
            self.game.state,
            GameState::WaitingToStart,
            "a restart did not land on the title screen",
        );
        self.tap(KeyCode::KeyR);
    }

    /// Presses and releases a key on the next tick, and runs it.
    fn tap(&mut self, key: KeyCode) {
        self.game.key_event(key, true);
        self.game.key_event(key, false);
        self.game.tick();
        self.ticks += 1;
    }

    /// The invariant that has to hold on **every** tick of every test that
    /// churns: the ECS holds exactly the player, the enemies, the bolts and
    /// the pickups, and the broadphase holds exactly the enemies and the
    /// pickups.
    ///
    /// This is the leak detector. An entity or a collider that outlived what
    /// it belonged to shows up here on the tick it happened.
    ///
    /// **`pickups` is both kinds**, because a potion is a [`PickupKind`] of
    /// the one pickup list rather than a population of its own — see that
    /// enum. That is what leaves these two equalities exactly as tight as
    /// they were, rather than gaining a term each and a second `Vec` for a
    /// reader to convince themselves accounts for itself.
    ///
    /// **An exact equality, with no term for the destruction queue.**
    /// `crcbl_server::Server::tick` sweeps between the game module and the
    /// snapshot, so nothing this game destroys survives the tick that
    /// destroyed it. The sum used to carry a `pending` term for entities
    /// awaiting the sweep; carrying it now would make this assertion
    /// tolerate the very defect it is here to catch.
    fn assert_nothing_leaked(&mut self) {
        let enemies = self.game.enemy_count();
        let bolts = self.game.bolt_count();
        let pickups = self.game.pickup_count();
        assert_eq!(
            self.game.entity_count(),
            1 + enemies + bolts + pickups,
            "tick {}: {enemies} enemies, {bolts} bolts and {pickups} pickups \
             do not account for the world",
            self.ticks,
        );
        // An equality, not a bound: every collider in the world is an enemy
        // or a pickup. The player is not in the broadphase and neither is a
        // bolt — see `contact_damage` and `sweep_bolts`.
        assert_eq!(
            self.game.collider_count(),
            enemies + pickups,
            "tick {}: {enemies} enemies and {pickups} pickups do not account \
             for the broadphase",
            self.ticks,
        );
    }
}

/// What the autopilot holds this tick, in [`HELD_KEYS`] order.
type Plan = [bool; 4];

/// A player who kites: walks a circle, which is what a survivors player
/// actually does and what keeps a run going long enough to churn.
///
/// Deliberately a **function of the tick index alone**, not of the enemy
/// list. Two reasons, and the second is the load-bearing one:
///
/// * reading `Game::enemies()` clones the whole view vector, which at the
///   counts this sample reaches would make the soak a test of `memcpy`;
/// * the input a given tick sees is then the same at 20 fps as at 240, so
///   the frame-rate test compares two runs of the same script rather than
///   two runs of two scripts.
///
/// A finished run is restarted by `Harness::play_ticks`, not here.
fn autopilot(game: &Game, tick: u64) -> Plan {
    if game.state == GameState::Dead {
        return [false; 4];
    }
    let phase = (tick % KITE_PERIOD) as f64 / KITE_PERIOD as f64 * std::f64::consts::TAU;
    let (x, y) = (phase.cos(), phase.sin());
    // A dead zone, so a lap is eight distinct directions rather than a
    // continuum — which keeps a held key held for a stretch of ticks instead
    // of chattering on and off at the axis crossings.
    [y > 0.4, y < -0.4, x < -0.4, x > 0.4]
}

/// The smallest gap between any two of `positions`, and the largest.
fn extremes(positions: &[DVec3]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max: f64 = 0.0;
    for (i, a) in positions.iter().enumerate() {
        for b in &positions[i + 1..] {
            let d = (*a - *b).length();
            min = min.min(d);
            max = max.max(d);
        }
    }
    (min, max)
}

// ---- the arena -----------------------------------------------------------

/// `clamp_axis` is **bit-exact** inside the arena, and saturating outside
/// it.
///
/// Exactness is not fussiness. `clamp_bodies` decides whether to write a
/// transform back — and so whether to touch the broadphase — by comparing
/// this against the position it was given, so a round trip that returned a
/// value one ulp away would re-place every body on every tick, forever.
#[test]
fn the_clamp_is_exact_inside_the_arena_and_saturates_outside_it() {
    for v in [
        -47.999_9,
        -0.5,
        0.0,
        7.25,
        47.999_9,
        0.1,
        -0.3,
        1.0 / 3.0,
        12.345_678_901_234,
    ] {
        assert_eq!(clamp_axis(v, ARENA_HALF_WIDTH), v, "{v} was moved");
    }
    for point in [
        DVec3::new(0.1, -0.3, 0.0),
        DVec3::new(1.0 / 3.0, -11.345_678_901_234, 0.0),
    ] {
        assert_eq!(
            clamp_to_arena(point, PLAYER_RADIUS),
            point,
            "{point:?} moved"
        );
    }
    assert_eq!(clamp_axis(100.0, 48.0), 48.0);
    assert_eq!(clamp_axis(-100.0, 48.0), -48.0);
    // A body wider than the space it is in has exactly one legal position.
    assert_eq!(clamp_axis(3.0, -1.0), 0.0);
    // The radius is taken off both sides, so a body is *inside* the wall.
    let corner = clamp_to_arena(DVec3::new(1e6, -1e6, 0.0), EnemyKind::Brute.radius());
    assert_eq!(corner.x, ARENA_HALF_WIDTH - EnemyKind::Brute.radius());
    assert_eq!(corner.y, -(ARENA_HALF_HEIGHT - EnemyKind::Brute.radius()));
}

/// The seed the movement tests stage at, chosen so its prop layout clears
/// the corridors they walk — pinned by
/// [`the_movement_corridors_are_clear_at_the_chosen_seed`]. Found by
/// searching `scatter_props`; any seed would do as long as the corridors
/// stay clear, and that test is what keeps them clear.
const MOVEMENT_CORRIDOR_SEED: u64 = 0x1;

/// **The movement tests' corridors are clear by arrangement, not by luck.**
///
/// `a_player_walking_at_a_wall_stops_at_it` and
/// `the_player_moves_at_the_stated_speed_and_a_diagonal_is_no_faster`
/// stage at [`MOVEMENT_CORRIDOR_SEED`], whose layout leaves every corridor
/// they walk free of props — measured here against [`scatter_props`], with
/// each prop's own radius plus the player's. Without this pin, a change to
/// `PROP_DENSITY`, `PROP_CELL` or the seed could drop a tree in front of
/// one of those paths and the failure would arrive as a movement bug; with
/// it, the failure names the layout instead. (A prop-free arena would make
/// them tests of a game that does not exist — considered and declined.)
#[test]
fn the_movement_corridors_are_clear_at_the_chosen_seed() {
    let diagonal = DVec3::new(
        ARENA_HALF_WIDTH - PLAYER_RADIUS,
        ARENA_HALF_HEIGHT - PLAYER_RADIUS,
        0.0,
    );
    let corridors = [
        // One second of +x, one second of +x+y (PLAYER_SPEED each), and
        // the wall test's full corner-to-corner diagonal.
        (DVec3::ZERO, DVec3::new(PLAYER_SPEED, 0.0, 0.0)),
        (
            DVec3::ZERO,
            DVec3::new(PLAYER_SPEED / 2f64.sqrt(), PLAYER_SPEED / 2f64.sqrt(), 0.0),
        ),
        (DVec3::ZERO, diagonal),
    ];
    let props = scatter_props(MOVEMENT_CORRIDOR_SEED);
    assert!(
        !props.is_empty(),
        "a seed whose layout has no props pins nothing"
    );
    for prop in &props {
        for &(a, b) in &corridors {
            let distance = distance_to_segment(prop.position, a, b);
            assert!(
                distance >= PLAYER_RADIUS + prop.kind.radius(),
                "a {:?} at {:?} is {distance} from the corridor {a:?} → {b:?}, \
                 inside the {}+{} reach of a walking player",
                prop.kind,
                prop.position,
                PLAYER_RADIUS,
                prop.kind.radius(),
            );
        }
    }
}

/// **The arena holds the player in**, however long they walk at a wall.
///
/// Asserted at the wall rather than merely "inside the arena": a clamp that
/// stopped a unit short would pass the weaker version and would still be
/// wrong. Staged at [`MOVEMENT_CORRIDOR_SEED`], whose layout leaves the
/// walk clear — see [`the_movement_corridors_are_clear_at_the_chosen_seed`].
#[test]
fn a_player_walking_at_a_wall_stops_at_it() {
    let mut harness = Harness::staged_with_seed(60, 60, DVec3::ZERO, MOVEMENT_CORRIDOR_SEED);
    // Long enough to cross the whole arena twice over at PLAYER_SPEED.
    harness.run_ticks(1_200, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyW, true)]);
    let player = harness.game.player;
    assert!(
        (player.x - (ARENA_HALF_WIDTH - PLAYER_RADIUS)).abs() < 1e-9,
        "the player did not stop at the right wall: {player:?}",
    );
    assert!(
        (player.y - (ARENA_HALF_HEIGHT - PLAYER_RADIUS)).abs() < 1e-9,
        "the player did not stop at the top wall: {player:?}",
    );
    harness.assert_nothing_leaked();
}

/// **Enemies are held in too**, which is where separation and the walls
/// meet: a crowd jammed into a corner is pushed outward by exactly the term
/// that has no idea the arena has edges.
///
/// The player is parked in the opposite corner, far outside
/// [`WEAPON_RANGE`], so nothing is shot and the crowd is intact at the end.
/// Both halves are asserted: nothing escaped, **and** the clamp actually
/// ran — a crowd that never reached a wall would satisfy the first on its
/// own.
#[test]
fn a_crowd_squeezed_into_a_corner_stays_inside_the_arena() {
    let far = DVec3::new(
        -(ARENA_HALF_WIDTH - PLAYER_RADIUS),
        -(ARENA_HALF_HEIGHT - PLAYER_RADIUS),
        0.0,
    );
    let mut harness = Harness::staged(60, 60, far);
    let corner = DVec3::new(ARENA_HALF_WIDTH, ARENA_HALF_HEIGHT, 0.0);
    for i in 0..30 {
        // All staged on top of each other in the corner, so separation has
        // nowhere to send them but into the two walls.
        // Planar, not `DVec3::splat`: the arena is a plane and everything
        // the game itself produces sits at `z = 0`, so a fixture with a
        // depth component would be separating in a dimension the clamp
        // deliberately leaves alone. See `docs/backlog.md`.
        let t = i as f64 * 0.01;
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, corner - DVec3::new(t, t, 0.0));
    }

    let limit = DVec3::new(
        ARENA_HALF_WIDTH - EnemyKind::Grunt.radius(),
        ARENA_HALF_HEIGHT - EnemyKind::Grunt.radius(),
        0.0,
    );
    // Checked on **every** tick rather than at the end: a body that left the
    // arena and was dragged back by its own seek would pass the end-state
    // version, and the failure names the tick it escaped on.
    let mut at_a_wall = 0;
    while harness.ticks < 180 {
        harness.run_ticks(harness.ticks + 1, &[]);
        for position in harness.game.enemy_positions() {
            assert!(
                position.x.abs() <= limit.x + 1e-9 && position.y.abs() <= limit.y + 1e-9,
                "tick {}: an enemy is outside the arena at {position:?}, \
                 against a limit of {limit:?}",
                harness.ticks,
            );
            if (position.x - limit.x).abs() < 1e-12 || (position.y - limit.y).abs() < 1e-12 {
                at_a_wall += 1;
            }
        }
    }
    assert_eq!(harness.game.enemy_count(), 30, "something killed the crowd");
    assert!(
        at_a_wall > 0,
        "nothing ever reached a wall, so the clamp was never exercised",
    );
    harness.assert_nothing_leaked();
}

/// Enemies enter from beyond the view, so the horde walks on screen rather
/// than appearing in it.
///
/// The relation is asserted, not the number, so a later tuning pass that
/// changes either constant is told rather than left to find out.
#[test]
fn enemies_enter_from_beyond_the_view() {
    // The corner of the widest window the demo pages ever open: a 4:3
    // canvas is the reference, and a wider one shows more of the arena.
    let view_corner = (VIEW_HALF_HEIGHT * 4.0 / 3.0).hypot(VIEW_HALF_HEIGHT);
    assert!(
        SPAWN_RING > view_corner,
        "spawns at {SPAWN_RING} land inside a view whose corner is {view_corner}",
    );
    // …and not so far that the horde never arrives: half a lap of the
    // autopilot's kite.
    const { assert!(SPAWN_RING < ARENA_HALF_HEIGHT) };
    for counter in 0..500 {
        let offset = spawn_offset(DEFAULT_SEED, counter, DVec3::ZERO);
        assert!(
            (offset.length() - SPAWN_RING).abs() < 1e-9,
            "spawn {counter} was dealt at {offset:?}, off the ring",
        );
    }
}

/// The shipped spawn table, pinned to literal offsets and kinds. A change
/// to `spawn_offset`'s or `spawn_kind`'s arithmetic — or to the hash under
/// them — relocates every enemy of every run the project ships, and the
/// determinism suites compare one run against another, so they would all
/// move together. These literals are the anchor that does not.
///
/// Compared with a tolerance rather than bit-exactly: the offsets go
/// through libm's `cos`/`sin`, which agree across platforms to the last
/// ulp or two, not to the bit. The delta is four orders of magnitude below
/// the tolerance, and any real change to the arithmetic relocates a spawn
/// by far more.
#[test]
fn the_shipped_spawn_table_is_pinned_to_literal_values() {
    let offsets = [
        DVec3::new(20.39094247691221, 12.657387759852242, 0.0),
        DVec3::new(22.09133346865561, 9.379391535523855, 0.0),
        DVec3::new(-7.73781539968375, -22.71841131858513, 0.0),
    ];
    let kinds = [EnemyKind::Brute, EnemyKind::Grunt, EnemyKind::Runner];
    for (counter, (offset, kind)) in offsets.iter().zip(&kinds).enumerate() {
        let actual = spawn_offset(DEFAULT_SEED, counter as u64, DVec3::ZERO);
        let delta = (actual - *offset).abs();
        assert!(
            delta.x < 1e-9 && delta.y < 1e-9 && delta.z < 1e-9,
            "spawn {counter} relocated: {actual:?} is not within 1e-9 of {offset:?}",
        );
        assert_eq!(
            spawn_kind(DEFAULT_SEED, counter as u64),
            *kind,
            "spawn {counter}"
        );
    }
}

/// A player parked in a corner or against a wall gets spawns inside the
/// arena, never materialising on the wall beside them.
///
/// `spawn_offset` draws from the arc that stays inside the arena, so every
/// `player + offset` satisfies the fully-inside bound — asserted through
/// `clamp_to_arena`, which is the exact check `spawn_enemy` runs and which
/// must therefore never move a spawn.
#[test]
fn spawns_stay_inside_the_arena_from_a_corner_or_a_wall() {
    let radius = EnemyKind::Brute.radius();
    for player in [
        // Parked in the corner…
        DVec3::new(ARENA_HALF_WIDTH - 0.1, ARENA_HALF_HEIGHT - 0.1, 0.0),
        // …and on a wall, at the player clamp's own limit.
        DVec3::new(ARENA_HALF_WIDTH - PLAYER_RADIUS, 0.0, 0.0),
        DVec3::new(0.0, ARENA_HALF_HEIGHT - PLAYER_RADIUS, 0.0),
    ] {
        for counter in 0..300 {
            let offset = spawn_offset(DEFAULT_SEED, counter, player);
            assert!(
                (offset.length() - SPAWN_RING).abs() < 1e-9,
                "counter {counter} at {player:?} was dealt {offset:?}, off the ring",
            );
            let point = player + offset;
            assert_eq!(
                clamp_to_arena(point, radius),
                point,
                "counter {counter} at {player:?}: the clamp moved the spawn at {point:?}",
            );
        }
    }
}

/// The draw is a pure function of `(seed, counter, player)`: the same
/// triple twice gives bit-identical offsets, and a different player draws
/// a different arc.
#[test]
fn the_spawn_draw_is_a_pure_function_of_seed_counter_and_player() {
    let corner = DVec3::new(ARENA_HALF_WIDTH - 0.1, ARENA_HALF_HEIGHT - 0.1, 0.0);
    for counter in 0..100 {
        assert_eq!(
            spawn_offset(DEFAULT_SEED, counter, corner),
            spawn_offset(DEFAULT_SEED, counter, corner),
            "counter {counter}",
        );
    }
    assert!(
        (0..500).any(|counter| {
            spawn_offset(DEFAULT_SEED, counter, DVec3::ZERO)
                != spawn_offset(DEFAULT_SEED, counter, corner)
        }),
        "the corner's arc is a strict subset of the origin's, so the two \
         draws should differ somewhere",
    );
}

/// At the origin the arc is the whole circle, and the draw is bit-identical
/// to the pre-arc formula — the relation the shipped table anchors.
#[test]
fn the_origin_draws_the_full_circle() {
    let angle = hash_unit(DEFAULT_SEED, spawn_index(0, DRAW_RING_ANGLE)) * std::f64::consts::TAU;
    let plain = DVec3::new(angle.cos(), angle.sin(), 0.0) * SPAWN_RING;
    assert_eq!(spawn_offset(DEFAULT_SEED, 0, DVec3::ZERO), plain);
}

/// `spawn_arc` directly: the full circle at the origin, a strict subset at
/// a corner whose every sampled angle stays inside the arena, and never an
/// empty arc across a grid of player positions.
#[test]
fn the_spawn_arc_stays_inside_the_arena() {
    let tau = std::f64::consts::TAU;
    assert_eq!(spawn_arc(DVec3::ZERO), (0.0, tau));

    let half_x = ARENA_HALF_WIDTH - EnemyKind::Brute.radius();
    let half_y = ARENA_HALF_HEIGHT - EnemyKind::Brute.radius();
    let corner = DVec3::new(ARENA_HALF_WIDTH - 0.1, ARENA_HALF_HEIGHT - 0.1, 0.0);
    let (start, span) = spawn_arc(corner);
    assert!(
        span < tau,
        "a corner's arc should not cover the whole circle"
    );
    for i in 1..1024 {
        let angle = start + span * i as f64 / 1024.0;
        let point = corner + DVec3::new(SPAWN_RING * angle.cos(), SPAWN_RING * angle.sin(), 0.0);
        assert!(
            point.x.abs() <= half_x && point.y.abs() <= half_y,
            "sampled angle {angle} put the ring point at {point:?}, outside the arena",
        );
    }

    for x in (-45..=45).step_by(5) {
        for y in (-30..=30).step_by(6) {
            let player = DVec3::new(f64::from(x), f64::from(y), 0.0);
            let (start, span) = spawn_arc(player);
            assert!(
                span > 0.0 && span <= tau,
                "player at {player:?} was dealt an arc of {span}",
            );
            let angle = start + 0.5 * span;
            let point =
                player + DVec3::new(SPAWN_RING * angle.cos(), SPAWN_RING * angle.sin(), 0.0);
            assert!(
                point.x.abs() <= half_x && point.y.abs() <= half_y,
                "the midpoint of {player:?}'s arc is outside: {point:?}",
            );
        }
    }
}

// ---- the player and the arena's rules -------------------------------------

/// The player moves at the stated speed, and a diagonal is not faster than a
/// straight line.
///
/// The classic bug this closes is one line of arithmetic — an unnormalised
/// input vector — and it is invisible in play until somebody notices that
/// running north-east outruns the runners and running north does not.
///
/// Staged at [`MOVEMENT_CORRIDOR_SEED`]: both corridors leave the glade and
/// run through prop territory, so a tree on either path would slow the
/// player and read as a speed bug — the seed and its pin are in
/// [`the_movement_corridors_are_clear_at_the_chosen_seed`].
#[test]
fn the_player_moves_at_the_stated_speed_and_a_diagonal_is_no_faster() {
    let straight = {
        let mut harness = Harness::staged_with_seed(60, 60, DVec3::ZERO, MOVEMENT_CORRIDOR_SEED);
        harness.run_ticks(60, &[(0, KeyCode::KeyD, true)]);
        harness.game.player
    };
    let diagonal = {
        let mut harness = Harness::staged_with_seed(60, 60, DVec3::ZERO, MOVEMENT_CORRIDOR_SEED);
        harness.run_ticks(60, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyW, true)]);
        harness.game.player
    };
    assert!(
        (straight.length() - PLAYER_SPEED).abs() < 0.15,
        "a second of walking covered {}, not {PLAYER_SPEED}",
        straight.length(),
    );
    assert!(
        (diagonal.length() - straight.length()).abs() < 1e-9,
        "a diagonal covered {} against a straight line's {}",
        diagonal.length(),
        straight.length(),
    );
    assert!(
        (diagonal.x - diagonal.y).abs() < 1e-9,
        "a diagonal is not diagonal: {diagonal:?}",
    );
}

/// Opposite keys cancel rather than picking a winner.
#[test]
fn opposite_keys_stand_still() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness.run_ticks(60, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyA, true)]);
    assert_eq!(harness.game.player, DVec3::ZERO);
}

// ---- the props -----------------------------------------------------------

/// The distance from `point` to the segment `a → b`.
fn distance_to_segment(point: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let length = ab.length_squared();
    let t = if length > 0.0 {
        ((point - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point - (a + ab * t)).length()
}

/// A prop of this game's own scatter with `clear` units of open ground round
/// it and the same off every wall, so a test can walk into it without
/// meeting something else on the way.
///
/// Taken from the real layout rather than staged, because a prop is not
/// something the simulation can be handed — the whole list is a function of
/// the seed. The `expect` is the test's own anti-vacuity: a scatter that
/// stopped producing anything walkable-to fails here rather than passing a
/// collision test that never touched a prop.
fn isolated_prop(props: &[PropView], kind: PropKind, clear: f64) -> PropView {
    *props
        .iter()
        .find(|prop| {
            prop.kind == kind
                && ARENA_HALF_WIDTH - prop.position.x.abs() > clear
                && ARENA_HALF_HEIGHT - prop.position.y.abs() > clear
                && props.iter().all(|other| {
                    std::ptr::eq(*prop, other) || (other.position - prop.position).length() > clear
                })
        })
        .unwrap_or_else(|| panic!("no {kind:?} in the scatter has {clear} units round it"))
}

/// **The layout is a pure function of the seed**, and two seeds are two
/// arenas.
///
/// Derived twice from the seed *independently* — through two whole `Game`s,
/// not two calls that could be reading one memoised answer — because the
/// scatter is simulation the player collides with, and a layout that differed
/// between a run and its replay, or between a client and a server, is the
/// failure this exists to rule out.
///
/// A constant function is perfectly deterministic, so the second half is what
/// makes the first mean anything: different seeds must give different arenas.
#[test]
fn the_prop_layout_is_a_pure_function_of_the_seed() {
    let setup = |seed| Setup {
        seed,
        ..Setup::default()
    };
    let dealt = |seed| {
        Game::with_setup(&setup(seed))
            .expect("a headless game always starts")
            .props()
    };

    let first = dealt(DEFAULT_SEED);
    assert!(!first.is_empty(), "the default seed was dealt no props");
    assert_eq!(
        first,
        dealt(DEFAULT_SEED),
        "two games on one seed stand in two different arenas",
    );
    // …and the free function agrees with what the game is holding, so the
    // tests below may use either.
    assert_eq!(first, scatter_props(DEFAULT_SEED));

    // Different seeds, different arenas — asserted as *layouts* rather than
    // as counts, because two different scatters of the same size are a pass
    // for a count and a failure for this.
    let mut seen = std::collections::HashSet::new();
    for run in 0..64u64 {
        let seed = DEFAULT_SEED.wrapping_add(run.wrapping_mul(0x9E37_79B9));
        let props = dealt(seed);
        assert!(!props.is_empty(), "seed {seed:#x} was dealt no props");
        let key: Vec<(u64, u64, PropKind)> = props
            .iter()
            .map(|prop| {
                (
                    prop.position.x.to_bits(),
                    prop.position.y.to_bits(),
                    prop.kind,
                )
            })
            .collect();
        assert!(seen.insert(key), "seed {seed:#x} repeated another's arena");
    }
}

/// **A restart does not re-deal the scenery**, which is the one place this
/// game's determinism story differs from the horde's.
///
/// `run_seed` deals a new hand of enemies every run on purpose. The arena is
/// not a hand: a player who learns where the cover is keeps that between
/// attempts, exactly as they keep where the walls are. See `scatter_props`.
#[test]
fn a_restart_deals_a_new_horde_and_the_same_arena() {
    let mut harness = Harness::new(60, 60);
    // One tick first, so the queued start edge is spent before the restart
    // edge arrives — two on one tick is a start, not a start and a restart.
    harness.run_ticks(1, &[]);
    let before = harness.game.props();
    let horde_before = harness.game.run_seed();
    harness.restart_run();
    assert_eq!(harness.game.props(), before, "the arena was re-dealt");
    assert_ne!(
        harness.game.run_seed(),
        horde_before,
        "the horde was not re-dealt, so this test compares nothing",
    );
}

/// **A run never starts inside a prop**, on any seed.
///
/// `place_player` puts the wizard at the origin on a fresh game and on every
/// restart, and `scatter_props` refuses to deal anything whose disc reaches
/// the clearing round it. Asserted over a spread of seeds, and each one is
/// asserted to have produced a scatter first — the whole check is vacuous on
/// an empty arena, which is exactly what a broken scatter would give.
#[test]
fn a_run_never_starts_inside_a_prop() {
    for run in 0..256u64 {
        let seed = DEFAULT_SEED.wrapping_add(run.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let props = scatter_props(seed);
        assert!(!props.is_empty(), "seed {seed:#x} was dealt no props");
        for prop in &props {
            let gap = prop.position.length() - prop.kind.radius();
            assert!(
                gap >= PROP_SPAWN_CLEARANCE - 1e-9,
                "seed {seed:#x}: a {:?} leaves {gap} of clear ground at the \
                 spawn, against {PROP_SPAWN_CLEARANCE}",
                prop.kind,
            );
        }
    }

    // …and the player really is put at the origin and left there, rather
    // than being pushed off it by something the arithmetic above missed.
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness.run_ticks(30, &[]);
    assert_eq!(harness.game.player, DVec3::ZERO);
}

/// **The arena clamp can never push the player into a prop**, which is the
/// other half of "and cannot be pushed into one".
///
/// The lattice keeps every prop at least its own radius plus a player's
/// *diameter* off the wall, so a player squashed against that wall — the one
/// position they have no way out of — is at worst exactly touching. Asserted
/// by walking the whole clamped boundary rather than by re-deriving the
/// inequality: `push_out_of_props` must leave every clamped point alone.
#[test]
fn the_arena_clamp_never_pushes_the_player_into_a_prop() {
    for run in 0..32u64 {
        let seed = DEFAULT_SEED.wrapping_add(run.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let props = scatter_props(seed);
        assert!(!props.is_empty(), "seed {seed:#x} was dealt no props");
        // A fine walk round the inside of all four walls, plus the corners,
        // which is where two clamps meet.
        let steps = 2_000;
        for step in 0..=steps {
            let t = f64::from(step) / f64::from(steps);
            let x = -ARENA_HALF_WIDTH + t * 2.0 * ARENA_HALF_WIDTH;
            let y = -ARENA_HALF_HEIGHT + t * 2.0 * ARENA_HALF_HEIGHT;
            for outside in [
                DVec3::new(x, 1e6, 0.0),
                DVec3::new(x, -1e6, 0.0),
                DVec3::new(1e6, y, 0.0),
                DVec3::new(-1e6, y, 0.0),
            ] {
                let clamped = clamp_to_arena(outside, PLAYER_RADIUS);
                assert_eq!(
                    push_out_of_props(clamped, PLAYER_RADIUS, &props),
                    clamped,
                    "seed {seed:#x}: the wall put the player at {clamped:?}, \
                     which is inside a prop",
                );
            }
        }
    }
}

/// **The scatter is scenery, not a maze**, and neither half is left to the
/// eye.
///
/// The failure at one end is a 96 × 72 arena the player is penned into while
/// the horde walks through the pen; at the other it is scenery nobody sees.
/// So: the gap between any two props is wider than the player, the arena is
/// overwhelmingly open ground, and the count is pinned — the numbers here are
/// what `PROP_DENSITY`'s reasoning claims, so a change to it is told rather
/// than left to be noticed in play.
#[test]
fn the_scatter_is_sparse_and_never_pens_the_player_in() {
    let arena = 4.0 * ARENA_HALF_WIDTH * ARENA_HALF_HEIGHT;
    let (mut fewest, mut most) = (usize::MAX, 0usize);
    for run in 0..64u64 {
        let seed = DEFAULT_SEED.wrapping_add(run.wrapping_mul(0x1234_5678_9ABC_DEF1));
        let props = scatter_props(seed);
        fewest = fewest.min(props.len());
        most = most.max(props.len());

        let mut covered = 0.0;
        for (index, prop) in props.iter().enumerate() {
            covered += std::f64::consts::PI * prop.kind.radius().powi(2);
            for other in &props[index + 1..] {
                let gap = (other.position - prop.position).length()
                    - prop.kind.radius()
                    - other.kind.radius();
                assert!(
                    gap > 2.0 * PLAYER_RADIUS,
                    "seed {seed:#x}: two props leave a {gap}-unit gap, which \
                     the player is {} across and cannot fit through",
                    2.0 * PLAYER_RADIUS,
                );
            }
            // Inside the arena, by the margin the clamp proof needs.
            let inset = prop.kind.radius() + 2.0 * PLAYER_RADIUS;
            assert!(
                prop.position.x.abs() <= ARENA_HALF_WIDTH - inset + 1e-9
                    && prop.position.y.abs() <= ARENA_HALF_HEIGHT - inset + 1e-9,
                "seed {seed:#x}: a prop at {:?} is too close to a wall",
                prop.position,
            );
        }
        assert!(
            covered < arena / 50.0,
            "seed {seed:#x}: the props cover {covered:.0} of {arena:.0} \
             square units, which is scenery you have to walk round",
        );
    }
    assert!(
        (30..=70).contains(&fewest) && (30..=70).contains(&most),
        "the scatter dealt between {fewest} and {most} props over 64 seeds",
    );
}

/// **A player walking at a prop is stopped by it, at its surface**, and an
/// unobstructed walk of the same length is not stopped at all.
///
/// The control is the half that makes this a test. A player aimed at a prop
/// and *missing* it stops nowhere and passes any assertion about not having
/// reached the far side — so the same script is run over open ground and
/// asserted to have covered the whole distance, and the obstructed run is
/// asserted to have finished exactly one radius pair from the prop's centre
/// rather than merely somewhere short.
#[test]
fn a_player_walking_at_a_prop_stops_at_its_surface() {
    const APPROACH: f64 = 6.0;
    const TICKS: u64 = 120;
    // Twice the approach, so a walk that was not stopped ends up well past
    // the prop rather than just short of the far side.
    let travel = PLAYER_SPEED * f64::from(u32::try_from(TICKS).expect("small")) / 60.0;
    assert!(travel > 2.0 * APPROACH, "the script does not overshoot");

    let props = scatter_props(DEFAULT_SEED);
    let target = isolated_prop(&props, PropKind::Tree, 9.0);
    let start = target.position - DVec3::X * APPROACH;

    let mut harness = Harness::staged(60, 60, start);
    harness.run_ticks(TICKS, &[(0, KeyCode::KeyD, true)]);
    let stopped = harness.game.player;

    let clear = target.kind.radius() + PLAYER_RADIUS;
    assert!(
        ((stopped - target.position).length() - clear).abs() < 1e-9,
        "the player finished {} from the trunk and its surface is at \
         {clear}: {stopped:?} against {:?}",
        (stopped - target.position).length(),
        target.position,
    );
    assert!(
        (stopped.y - start.y).abs() < 1e-9,
        "a head-on approach slid sideways: {stopped:?} from {start:?}",
    );
    // It walked, rather than being stopped where it stood.
    assert!(
        stopped.x - start.x > APPROACH - clear - 1e-9,
        "the player only covered {} of the {} to the trunk",
        stopped.x - start.x,
        APPROACH - clear,
    );

    // The control: the same script over ground with nothing on it.
    let open = (0..)
        .map(|i| start + DVec3::Y * (f64::from(i) * 0.25 + 3.0))
        .take(400)
        .find(|from| {
            from.y.abs() < ARENA_HALF_HEIGHT - PLAYER_RADIUS
                && props.iter().all(|prop| {
                    distance_to_segment(prop.position, *from, *from + DVec3::X * travel)
                        > prop.kind.radius() + PLAYER_RADIUS + 0.5
                })
        })
        .expect("the arena holds an unobstructed corridor");
    let mut harness = Harness::staged(60, 60, open);
    harness.run_ticks(TICKS, &[(0, KeyCode::KeyD, true)]);
    assert!(
        (harness.game.player.x - open.x - travel).abs() < 0.15,
        "an unobstructed walk covered {} of {travel}",
        harness.game.player.x - open.x,
    );
}

/// **A player walking into a prop off-centre slides round it**, which is the
/// difference between scenery and glue.
///
/// The push takes the component of the approach that was *into* the disc and
/// leaves the component along it, exactly as `clamp_axis` takes one axis at a
/// wall — so a player pressing east against a trunk they met off-centre
/// keeps sliding until they are past it, on a single held key.
///
/// The offset is well inside the pair of radii, so the straight-line path
/// really does run into the trunk: a version that missed it would slide
/// nowhere and pass a weaker assertion about having got past.
#[test]
fn a_player_walking_into_a_prop_off_centre_slides_round_it() {
    let props = scatter_props(DEFAULT_SEED);
    let target = isolated_prop(&props, PropKind::Tree, 9.0);
    let clear = target.kind.radius() + PLAYER_RADIUS;
    let offset = 0.35;
    assert!(
        offset < clear,
        "the approach misses the trunk, so nothing is being slid round",
    );

    let start = target.position - DVec3::X * 6.0 + DVec3::Y * offset;
    let mut harness = Harness::staged(60, 60, start);
    harness.run_ticks(180, &[(0, KeyCode::KeyD, true)]);
    let end = harness.game.player;

    assert!(
        end.x > target.position.x + clear - 1e-9,
        "the player stuck on the trunk at {end:?}, west of {:?}",
        target.position,
    );
    assert!(
        end.y - start.y > 1.0,
        "the player got past without sliding: it left y = {} and finished \
         at y = {}",
        start.y,
        end.y,
    );
}

/// **Enemies and bolts pass through the props, and that is the decision.**
///
/// `docs/plan/sample/03-horde.md`'s hard cap bars pathfinding, so a prop the
/// horde had to route around would be pathfinding wearing a tree costume —
/// see `PropKind`. Both halves are asserted here because both are things a
/// later change could quietly take away, and neither would show up as a
/// failure anywhere else: the horde would simply get slower.
#[test]
fn enemies_and_bolts_pass_through_the_props_the_player_cannot() {
    let props = scatter_props(DEFAULT_SEED);
    let target = isolated_prop(&props, PropKind::Tree, 9.0);
    let full = EnemyKind::Brute.max_hp();

    // **The horde walks through.** The player stands far enough back that
    // the gun never fires, so what crosses the trunk is a seeking body and
    // nothing else — a brute shot to pieces on the way would prove nothing.
    let far = target.position - DVec3::X * (WEAPON_RANGE + 2.0);
    let mut harness = Harness::staged(60, 60, far);
    harness
        .game
        .stage_enemy(EnemyKind::Brute, target.position + DVec3::X * 3.0);
    let mut walked_through = false;
    // Stopped the moment it has, rather than run on: the brute is closing
    // on the player the whole time, and a long enough run brings it into
    // `WEAPON_RANGE` and turns this into a test with a gun in it.
    while harness.ticks < 150 && !walked_through {
        harness.run_ticks(harness.ticks + 1, &[]);
        for position in harness.game.enemy_positions() {
            if (position - target.position).length() < target.kind.radius() {
                walked_through = true;
            }
        }
    }
    assert_eq!(harness.game.bolts_fired(), 0, "the gun joined in");
    assert!(
        walked_through,
        "the brute never stood inside the trunk, so nothing was tested",
    );

    // **The gun shoots through.** Close enough to fire, and short enough
    // that the brute is still on the far side of the trunk when the damage
    // is read — so the bolts that landed crossed it.
    let player = target.position - DVec3::X * 3.0;
    let mut harness = Harness::staged(60, 60, player);
    let brute = harness
        .game
        .stage_enemy(EnemyKind::Brute, target.position + DVec3::X * 3.0);
    assert_eq!(harness.game.enemy_hp(brute), Some(full));
    harness.run_ticks(30, &[]);
    let left = harness
        .game
        .enemy_hp(brute)
        .expect("half a second of a four-a-second gun does not kill a brute");
    assert!(
        left < full,
        "no bolt reached the brute through the trunk: {left} of {full}",
    );
    let standing = harness
        .game
        .enemy_positions()
        .first()
        .copied()
        .expect("the brute is alive");
    assert!(
        standing.x > target.position.x + target.kind.radius(),
        "the brute walked clear of the trunk before it was hit, so the shot \
         had a clear line: {standing:?} against {:?}",
        target.position,
    );
}

/// **A prop is neither an entity nor a collider**, which is what leaves the
/// leak test's two exact equalities meaning what they meant.
///
/// `Harness::assert_nothing_leaked` says the world holds the player, the
/// enemies, the bolts and the gems and nothing else, and that the broadphase
/// holds the enemies and the gems and nothing else. Scenery that had entered
/// either would break both — and would also have put itself in front of every
/// separation query the sample exists to measure.
#[test]
fn the_props_are_neither_entities_nor_colliders() {
    let mut harness = Harness::new(60, 60);
    assert!(
        !harness.game.props().is_empty(),
        "an empty arena makes the counts below vacuous",
    );
    harness.play_ticks(240);
    assert!(harness.game.enemy_count() > 0, "nothing to count against");
    harness.assert_nothing_leaked();
}

/// `PROP_MAX_RADIUS` really is the largest, so the spacing this file
/// `const`-asserts was checked against the right number.
///
/// It cannot be a fold over [`PropKind::ALL`] — a `const` assertion needs a
/// constant — so a third kind added below the tree is exactly the change
/// that would leave it stale.
#[test]
fn the_largest_prop_radius_is_the_one_the_spacing_was_checked_against() {
    let largest = PropKind::ALL
        .iter()
        .map(|kind| kind.radius())
        .fold(0.0f64, f64::max);
    assert_eq!(largest, PROP_MAX_RADIUS);
}

// ---- seeking and separation, which is what this sample is for -------------

/// **Enemies seek the player.**
///
/// The distance is asserted against the *speed they are supposed to travel
/// at*, not against "it went down": an enemy that drifted a hundredth of a
/// unit a second would satisfy the weaker version.
#[test]
fn every_kind_of_enemy_seeks_the_player_at_its_own_speed() {
    for kind in EnemyKind::ALL {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let start = DVec3::new(0.0, -30.0, 0.0);
        let enemy = harness.game.stage_enemy(kind, start);
        harness.run_ticks(120, &[]);
        let position = harness
            .game
            .enemy_position(enemy)
            .unwrap_or_else(|| panic!("{kind:?} died: nothing in this test can kill it"));

        let travelled = (position - start).length();
        let expected = kind.speed() * 2.0;
        assert!(
            (travelled - expected).abs() < 0.1,
            "{kind:?} covered {travelled} in two seconds, not {expected}",
        );
        assert!(
            position.length() < start.length(),
            "{kind:?} moved away from the player: {position:?}",
        );
        // …and straight at the player, not merely nearer.
        assert!(
            position.x.abs() < 1e-9,
            "{kind:?} wandered off the line to the player: {position:?}",
        );
    }
}

/// **A crowd does not end up co-located.**
///
/// The property separation exists for, and the one a broken implementation
/// satisfies vacuously if everything simply sits on the player — so the
/// player is parked 36 units away and out of weapon range, and what is
/// measured is the crowd's *internal* spacing while it travels.
///
/// Both halves are asserted:
///
/// * the knot starts closer than the neighbourhood, so the mechanism is
///   genuinely engaged rather than untouched;
/// * every pair ends at least `r_a + r_b` apart, which is the physical claim
///   — no two bodies are inside one another — and the crowd's spread grows
///   from a point to something a screen can see.
///
/// With `SEPARATION_STRENGTH` at zero the whole crowd rides on top of itself
/// and the minimum gap stays at its starting value.
#[test]
fn a_crowd_seeking_one_player_comes_apart_rather_than_stacking() {
    let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
    const CROWD: usize = 20;
    for i in 0..CROWD {
        // A knot 0.19 units across, which is inside two grunts' radii.
        let t = i as f64 * 0.01;
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(t, -6.0 - t, 0.0));
    }

    let (min_before, max_before) = extremes(&harness.game.enemy_positions());
    let touching = 2.0 * EnemyKind::Grunt.radius();
    assert!(
        min_before < touching,
        "the crowd did not start interpenetrating, so this proves nothing: \
         {min_before} against {touching}",
    );

    harness.run_ticks(180, &[]);
    assert_eq!(
        harness.game.enemy_count(),
        CROWD,
        "something killed part of the crowd, so the spacing below is of \
         fewer bodies than were staged",
    );
    let positions = harness.game.enemy_positions();
    let (min_after, max_after) = extremes(&positions);

    assert!(
        min_after >= touching,
        "two grunts are still inside each other: {min_after} against {touching}",
    );
    assert!(
        max_after > max_before + 3.0,
        "the crowd never spread: {max_before} to {max_after}",
    );
    // And it is still a crowd going somewhere, not an explosion: the
    // centroid has to have travelled towards the player.
    let centroid = positions.iter().copied().sum::<DVec3>() / CROWD as f64;
    assert!(
        centroid.y > -6.0 + 5.0,
        "the crowd stopped seeking while it separated: {centroid:?}",
    );
    harness.assert_nothing_leaked();
}

/// **Two enemies at exactly the same point come apart**, which is the case
/// with no direction between them and the one [`spawn_jitter`] exists for.
///
/// A horde converging on one player produces this constantly. Without the
/// tie-break a coincident pair is a fixed point of `separation_push` and
/// the two ride on top of each other forever — which the test above cannot
/// see, because twenty bodies staged a hundredth of a unit apart are never
/// exactly coincident.
#[test]
fn two_enemies_at_exactly_one_point_still_come_apart() {
    let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
    let spot = DVec3::new(0.0, -6.0, 0.0);
    let a = harness.game.stage_enemy(EnemyKind::Grunt, spot);
    let b = harness.game.stage_enemy(EnemyKind::Grunt, spot);
    assert_eq!(
        harness.game.enemy_position(a),
        harness.game.enemy_position(b),
        "the two were not staged on the same point",
    );

    harness.run_ticks(120, &[]);
    let (Some(a), Some(b)) = (
        harness.game.enemy_position(a),
        harness.game.enemy_position(b),
    ) else {
        panic!("one of them died: nothing in this test can kill it");
    };
    let gap = (a - b).length();
    assert!(
        gap >= 2.0 * EnemyKind::Grunt.radius(),
        "a coincident pair is still coincident after two seconds: {gap}",
    );
}

/// **The separation query radius is exactly the neighbourhood**, which is an
/// assumption about `crcbl-phys` and not about this file.
///
/// [`separation_query_radius`] omits the *neighbour's* radius on purpose —
/// see its docs — which is only correct because `overlap_sphere` is
/// shape-aware. If that ever became an AABB-versus-point test the horde
/// would quietly stop seeing half its neighbours and nothing else here would
/// notice. The board below has a body just inside the boundary and one just
/// outside it, for each pair of kinds, and the expected set is computed from
/// the distances rather than read off a passing run.
#[test]
fn the_separation_query_radius_is_exactly_the_neighbourhood() {
    for subject in EnemyKind::ALL {
        for neighbour in EnemyKind::ALL {
            let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
            let me = harness.game.stage_enemy(subject, DVec3::ZERO);
            let desired = subject.radius() + neighbour.radius() + SEPARATION_SLACK;
            let inside = harness
                .game
                .stage_enemy(neighbour, DVec3::new(desired - 0.01, 0.0, 0.0));
            let outside = harness
                .game
                .stage_enemy(neighbour, DVec3::new(0.0, -(desired + 0.01), 0.0));

            let found = harness.game.separation_neighbours(me);
            let mut expected = vec![me, inside];
            expected.sort_unstable_by_key(|entity| entity.to_bits());
            assert_eq!(
                found,
                expected,
                "{subject:?} against {neighbour:?} at a desired gap of {desired}: \
                 the body at {} should be in and the one at {} should be out",
                desired - 0.01,
                desired + 0.01,
            );
            assert!(
                !found.contains(&outside),
                "{subject:?} saw a {neighbour:?} past its neighbourhood",
            );
        }
    }
}

/// The query returns the subject itself, which is why `steer_enemies`
/// filters it out.
///
/// A regression guard rather than a wish: the day `crcbl-phys` grows an
/// entity-shaped overlap with an exclusion list (`docs/backlog.md`), this
/// goes red and the filter can go.
#[test]
fn an_enemy_finds_itself_in_its_own_neighbourhood() {
    let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
    let alone = harness.game.stage_enemy(EnemyKind::Grunt, DVec3::ZERO);
    assert_eq!(harness.game.separation_neighbours(alone), vec![alone]);
}

// ---- the weapon ----------------------------------------------------------

/// **A bolt that crosses an enemy inside one tick still hits it.**
///
/// The whole point of sweeping `prev → cur` rather than testing where the
/// bolt ended up. The tick rate is turned down until one tick of travel is
/// wider than the enemy, and the test then *proves the discrete test would
/// have missed*: it reads the bolt's real position on one side, computes
/// where the next tick puts it, asserts both are clear of the target, and
/// only then asserts the kill.
/// **The target is moving too**, which is why both positions are read from
/// the simulation rather than assumed. A brute closing at 1.9 units a second
/// covers half a unit per tick at this rate, which is more than its own
/// radius — a version of this test that pinned the enemy at the origin
/// "passed" while the bolt was in fact landing on a body that had walked
/// into it, which is not the property being claimed.
#[test]
fn a_bolt_that_crosses_an_enemy_within_one_tick_still_hits_it() {
    // 4 Hz: a quarter-second step, so a bolt covers 7.5 units while the
    // brute it must not skip is 2 units across including the bolt.
    let mut harness = Harness::staged(4, 4, DVec3::new(0.0, -13.0, 0.0));
    let brute = harness.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
    let dt = harness.game.tick_dt_secs();
    let reach = EnemyKind::Brute.radius() + BOLT_RADIUS;
    assert!(
        BOLT_SPEED * dt > 2.0 * reach,
        "this tick rate does not make the bolt skip the enemy at all: \
         {} against {}",
        BOLT_SPEED * dt,
        2.0 * reach,
    );

    // Tick one: the gun acquires and fires. Nothing has moved yet.
    harness.run_ticks(1, &[]);
    assert_eq!(harness.game.bolt_count(), 1, "nothing was fired");
    assert_eq!(
        harness.game.enemy_hp(brute),
        Some(EnemyKind::Brute.max_hp()),
        "something hit it before it was fired at",
    );

    // Tick two: one step of flight, which lands the bolt short. The oldest
    // bolt is the one fired first, because `bolts` is appended to.
    harness.run_ticks(2, &[]);
    let Some(bolt_before) = harness.game.bolts().first().map(|bolt| bolt.position) else {
        panic!("the bolt vanished before it reached anything");
    };
    let enemy_before = harness.game.enemy_position(brute).expect("the brute");
    assert!(
        (bolt_before - enemy_before).length() > reach,
        "a point test would already have hit: bolt {bolt_before:?}, \
         enemy {enemy_before:?}",
    );

    // Tick three: the step that goes straight over it.
    harness.run_ticks(3, &[]);
    let enemy_after = harness
        .game
        .enemy_position(brute)
        .expect("a brute survives one bolt");
    let bolt_after = bolt_before + DVec3::new(0.0, BOLT_SPEED * dt, 0.0);
    assert!(
        (bolt_after - enemy_after).length() > reach,
        "a point test would have hit on the far side: bolt {bolt_after:?}, \
         enemy {enemy_after:?}",
    );
    // Both ends of the step are clear of the enemy, at the enemy's real
    // position on each of those ticks — so nothing but the sweep can
    // account for the damage.
    let hp = harness.game.enemy_hp(brute).expect("still alive");
    assert!(
        (hp - (EnemyKind::Brute.max_hp() - BOLT_DAMAGE)).abs() < 1e-9,
        "the bolt stepped over the enemy: it went from {bolt_before:?} to \
         {bolt_after:?} past a body at {enemy_before:?} then {enemy_after:?}, \
         and the brute is on {hp}",
    );
    harness.assert_nothing_leaked();
}

/// **A bolt is never swept over ground it did not travel.**
///
/// The reason the gun fires *after* the sweep — see `run_tick`. A sweep
/// reconstructs `prev` as `position - velocity * dt`, so a bolt swept on the
/// tick it was created is swept from a point one whole step *behind the
/// muzzle*, through the thing that fired it, to the muzzle. At 60 Hz that
/// segment is half a unit and hides inside the player; at 4 Hz it is 7.5
/// units of arena behind them.
///
/// The decoy below sits in exactly that stretch and is **further from the
/// player than the target**, so the gun has no reason to aim at it — the
/// only thing that can touch it is a sweep over ground the bolt never
/// covered.
#[test]
fn a_bolt_is_never_swept_over_ground_it_did_not_travel() {
    let mut harness = Harness::staged(4, 4, DVec3::ZERO);
    let dt = harness.game.tick_dt_secs();
    let target = harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 3.0, 0.0));
    let behind = -(BOLT_SPEED * dt) / 2.0;
    let decoy = harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, behind, 0.0));
    assert!(
        behind.abs() > 3.0,
        "the decoy at {behind} is nearer than the target, so the gun would \
         legitimately shoot it",
    );

    harness.run_ticks(2, &[]);
    assert_eq!(
        harness.game.enemy_hp(decoy),
        Some(EnemyKind::Brute.max_hp()),
        "the bolt was swept backwards through the player on the tick it \
         was fired, and hit something {behind} units behind them",
    );
    // Second, because a gun that fired nothing at all would leave the decoy
    // untouched too and satisfy the assertion above for the wrong reason.
    assert!(
        harness
            .game
            .enemy_hp(target)
            .is_some_and(|hp| hp < EnemyKind::Brute.max_hp()),
        "the gun never hit the target, so the decoy's health proves nothing",
    );
}

/// A bolt fired at an enemy at the ordinary tick rate hits it too — the case
/// the CCD test above deliberately makes impossible for a point test,
/// asserted here for the case that is not.
///
/// And it takes exactly the number of bolts the damage table says, which is
/// what makes [`BOLT_DAMAGE`] a number rather than a decoration.
#[test]
fn an_enemy_dies_to_exactly_the_number_of_bolts_its_hit_points_say() {
    for kind in EnemyKind::ALL {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
        let enemy = harness.game.stage_enemy(kind, DVec3::ZERO);
        let expected = (kind.max_hp() / BOLT_DAMAGE).ceil() as u64;

        // Long enough for `expected` shots plus their flight, and not so
        // long that the enemy reaches the player.
        let limit = harness.ticks + 60 * (expected + 2);
        let mut fired_at_death = None;
        while harness.ticks < limit && fired_at_death.is_none() {
            harness.run_ticks(harness.ticks + 1, &[]);
            if harness.game.kills == 1 {
                fired_at_death = Some(harness.game.bolts_fired());
            }
        }

        let fired =
            fired_at_death.unwrap_or_else(|| panic!("{kind:?} never died in {limit} ticks"));
        assert_eq!(
            fired,
            expected,
            "{kind:?} took {fired} bolts against {} hit points at {BOLT_DAMAGE} each",
            kind.max_hp(),
        );
        assert!(
            harness.game.enemy_hp(enemy).is_none(),
            "{kind:?} was killed and is still on the list",
        );
        assert_eq!(harness.game.enemy_count(), 0);
        harness.assert_nothing_leaked();
    }
}

/// **Damage lands before death.** The intermediate state is what says the
/// hit points are being subtracted rather than the enemy being deleted on
/// the first touch.
#[test]
fn a_bolt_takes_hit_points_off_before_it_takes_the_enemy_off() {
    let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
    let brute = harness.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
    assert_eq!(
        harness.game.enemy_hp(brute),
        Some(EnemyKind::Brute.max_hp())
    );

    // One shot's flight: 6 units at 30 units a second is a fifth of a
    // second, and the cooldown is a quarter, so exactly one bolt has landed.
    harness.run_ticks(20, &[]);
    let hp = harness
        .game
        .enemy_hp(brute)
        .expect("a brute survives one bolt");
    assert!(
        (hp - (EnemyKind::Brute.max_hp() - BOLT_DAMAGE)).abs() < 1e-9,
        "one bolt took the brute to {hp}",
    );
    assert_eq!(harness.game.kills, 0, "it died to one bolt");
}

/// The gun aims at the **nearest** enemy, not at whichever the broadphase
/// happened to hand back first.
///
/// Four targets rather than two, and the assertion is the **order they die
/// in**. With two it is a coin toss whether a gun that took the first result
/// off the tree happened to pick the right one — that version of this test
/// passed while `min_by` was replaced by `next()`. Four one-shot runners at
/// even spacing, staged in a scrambled order, is a one-in-twenty-four
/// coincidence instead.
#[test]
fn the_gun_shoots_the_nearest_enemy_first() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // Distances 3, 6, 9, 12, staged 12, 3, 9, 6 — so neither the list order
    // nor its reverse is the answer — and each at a different bearing, so
    // the broadphase's own traversal order is a spatial partition rather
    // than a distance ranking. Four in a line does not distinguish the two:
    // the tree visits a colinear board nearest-first anyway, and that
    // version of this test passed under `next()`.
    let mut staged: Vec<(f64, Entity)> = Vec::new();
    for (distance, bearing) in [(12.0, 290.0), (3.0, 200.0), (9.0, 110.0), (6.0, 20.0)] {
        let angle: f64 = f64::to_radians(bearing);
        let entity = harness.game.stage_enemy(
            EnemyKind::Runner,
            DVec3::new(angle.cos(), angle.sin(), 0.0) * distance,
        );
        staged.push((distance, entity));
    }
    assert!(
        staged.iter().all(|(d, _)| *d < WEAPON_RANGE),
        "a target out of range would never be shot at all",
    );

    // Every runner dies to one bolt, so the order they leave the field in is
    // the order the gun chose them in.
    let mut order = Vec::new();
    while order.len() < staged.len() && harness.ticks < 300 {
        harness.run_ticks(harness.ticks + 1, &[]);
        for (distance, entity) in &staged {
            if harness.game.enemy_hp(*entity).is_none() && !order.contains(distance) {
                order.push(*distance);
            }
        }
    }
    assert_eq!(
        order,
        vec![3.0, 6.0, 9.0, 12.0],
        "the gun did not work outwards from the player",
    );
    assert_eq!(harness.game.kills, 4);
    harness.assert_nothing_leaked();
}

/// Nothing in range is nothing fired, and the cooldown is not spent on it.
#[test]
fn the_gun_holds_its_fire_when_nothing_is_in_range() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // Comfortably outside the weapon's reach, and stationary because it is
    // only there to prove the gun can see nothing rather than that the field
    // is empty.
    harness
        .game
        .stage_enemy(EnemyKind::Grunt, DVec3::new(WEAPON_RANGE + 20.0, 0.0, 0.0));
    harness.run_ticks(30, &[]);
    assert_eq!(harness.game.bolts_fired(), 0, "it shot at nothing");

    // Now put something in reach: the gun must fire on the very next tick,
    // not a cooldown later.
    harness
        .game
        .stage_enemy(EnemyKind::Grunt, DVec3::new(3.0, 0.0, 0.0));
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(
        harness.game.bolts_fired(),
        1,
        "the gun was not ready when a target arrived",
    );
}

/// **The wizard faces the way the input last pointed**, and nothing else
/// turns it.
///
/// Every clause here is a way of getting it wrong that would look fine in a
/// screenshot: a facing taken from the velocity turns the wrong way against
/// a wall, one taken from the aim spins with the crowd, one that resets on
/// key-up flickers every time the player stops, and one driven by any key
/// rather than the horizontal pair turns on `W`.
#[test]
fn the_wizard_faces_the_way_the_input_last_pointed() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let facing = |harness: &Harness| lock(&harness.game.shared).player_facing;
    let walking = |harness: &Harness| lock(&harness.game.shared).player_moving;

    assert_eq!(facing(&harness), Facing::Right, "the way it is drawn");
    assert!(!walking(&harness), "nobody has pressed anything");

    let step = |harness: &mut Harness, key, down| {
        harness.game.key_event(key, down);
        harness.run_ticks(harness.ticks + 1, &[]);
    };

    step(&mut harness, KeyCode::KeyA, true);
    assert_eq!(facing(&harness), Facing::Left);
    assert!(walking(&harness));

    // …and both reach the renderer. `art::Scene::build` takes a
    // `RenderState` and nothing else, so a `render_state` that forgot either
    // field would leave the wizard permanently facing right and standing
    // still, with every assertion in this test still passing.
    let mut out = RenderState::default();
    harness.game.render_state(&mut out);
    assert_eq!(out.player_facing, Facing::Left);
    assert!(out.player_walking);

    // Released, and it keeps the facing it had. This is the flicker.
    step(&mut harness, KeyCode::KeyA, false);
    assert_eq!(facing(&harness), Facing::Left, "it snapped back on key-up");
    assert!(!walking(&harness));

    // Straight up: walking, and still facing left, because facing is a
    // left/right property and `W` says nothing about it.
    step(&mut harness, KeyCode::KeyW, true);
    assert_eq!(facing(&harness), Facing::Left, "`W` turned the wizard");
    assert!(walking(&harness));
    step(&mut harness, KeyCode::KeyW, false);

    step(&mut harness, KeyCode::KeyD, true);
    assert_eq!(facing(&harness), Facing::Right);

    // Both horizontals: nothing is being asked for, so the facing stands and
    // the wizard is not walking either.
    step(&mut harness, KeyCode::KeyA, true);
    assert_eq!(
        facing(&harness),
        Facing::Right,
        "a cancelled input turned it"
    );
    assert!(!walking(&harness), "a cancelled input walked it");
    step(&mut harness, KeyCode::KeyD, false);
    assert_eq!(facing(&harness), Facing::Left, "and `A` is still down");

    // Dead is not walking, whatever is held. Without this the death screen
    // shows a corpse walking on the spot.
    harness.game.set_player_hp(0.000_1);
    harness
        .game
        .stage_enemy(EnemyKind::Brute, harness.game.player);
    harness.run_ticks(harness.ticks + 8, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert!(!walking(&harness), "the dead wizard kept walking");
}

/// **A bolt leaves the head of the staff, on the side the wizard is
/// facing — even when that is the side the target is not on.**
///
/// The decision [`staff_muzzle`] records, asserted rather than described.
/// The wizard is turned left and the only enemy is due east, so a muzzle
/// mirrored to the *firing* side and a muzzle mirrored to the *facing* side
/// are a body's width apart and this can tell them apart.
#[test]
fn a_bolt_leaves_the_staff_on_the_side_the_wizard_faces() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // One tick of `A`, so the wizard is turned and then standing still: the
    // bolt's position is read against where the player actually is, and a
    // player still moving would make that a moving target.
    harness.game.key_event(KeyCode::KeyA, true);
    harness.run_ticks(harness.ticks + 1, &[]);
    harness.game.key_event(KeyCode::KeyA, false);
    harness.run_ticks(harness.ticks + 2, &[]);
    assert_eq!(lock(&harness.game.shared).player_facing, Facing::Left);
    assert_eq!(harness.game.bolts_fired(), 0, "there was nothing to shoot");

    // Now give it a target on the other side, and let it fire exactly once.
    let target = DVec3::new(4.0, 0.0, 0.0);
    harness.game.stage_enemy(EnemyKind::Grunt, target);
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.bolts_fired(), 1);

    let bolt = harness.game.bolts()[0].position;
    let player = harness.game.player;
    let want = player + staff_muzzle(Facing::Left);
    assert!(
        (bolt - want).length() < 1e-9,
        "the bolt started at {bolt:?}, and the staff head is at {want:?}",
    );
    // …and the two candidates really are far apart, so the assertion above
    // is not satisfied by both of them.
    let mirrored = player + staff_muzzle(Facing::Right);
    assert!(
        (want - mirrored).length() > 2.0 * STAFF_MUZZLE.x - 1e-9,
        "the two muzzles are the same point, so this test cannot fail",
    );
    // The bolt starts behind the aim and flies through the wizard, which is
    // the documented consequence and not an accident.
    assert!(
        bolt.x < player.x,
        "the bolt did not start on the staff side"
    );
    assert!(
        harness.game.bolts()[0].position.x < target.x,
        "the bolt did not start short of its target",
    );
}

/// **A bolt outlives the range it was fired at.** A shot that expired short
/// of its target would make [`WEAPON_RANGE`] a lie.
///
/// The relation is asserted, not the number, so a later tuning pass that
/// changes either constant is told rather than left to find out.
#[test]
fn the_reach_of_a_bolt_covers_the_weapons_range() {
    let reach = BOLT_SPEED * BOLT_LIFE;
    // The staff head is the third term: the range is measured from the
    // player's centre and the bolt starts at the muzzle, which can be on the
    // far side of the wizard from the target.
    assert!(
        reach > WEAPON_RANGE + max_enemy_radius() + STAFF_MUZZLE.length(),
        "a bolt reaches {reach}, short of a target at {WEAPON_RANGE}",
    );
    // …and does not cross the whole arena, or the range is decoration.
    assert!(reach < ARENA_HALF_WIDTH);
}

/// A bolt that hits nothing expires, and takes its entity with it.
#[test]
fn a_bolt_that_hits_nothing_expires() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // A runner well off to one side: in range, so the gun fires, and fast
    // enough that it has left the bolt's line by the time the bolt arrives.
    harness
        .game
        .stage_enemy(EnemyKind::Runner, DVec3::new(0.0, 12.0, 0.0));
    harness.run_ticks(1, &[]);
    assert_eq!(harness.game.bolt_count(), 1, "nothing was fired");

    // The bolt outlives its target, so take the target away and let the
    // bolt fly on into an empty arena. Removing it is not a kill — nothing
    // shot it — which is what the count below says.
    harness.game.clear_enemies();
    let life_ticks = (BOLT_LIFE / harness.game.tick_dt_secs()).ceil() as u64;
    harness.run_ticks(harness.ticks + life_ticks + 3, &[]);

    assert_eq!(harness.game.bolt_count(), 0, "the bolt outlived its life");
    assert_eq!(harness.game.kills, 0, "it hit something it should not have");
    assert_eq!(
        harness.game.entity_count(),
        1,
        "the expired bolt left its entity behind: the player should be all \
         that is left",
    );
    assert_eq!(harness.game.pending_despawns(), 0, "the sweep never ran");
    harness.assert_nothing_leaked();
}

// ---- contact damage, death and the clock ---------------------------------

/// **Contact damage applies at the stated rate, and stops when it stops.**
///
/// The rate is asserted against the table rather than "hit points went
/// down": a game that took one point per touching enemy per tick would pass
/// the weaker version and would make the tick rate the difficulty.
#[test]
fn contact_damage_runs_at_the_stated_rate_and_stops_when_it_does() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // A brute, because it survives long enough under the gun to keep
    // touching, and because it is the loudest number in the table.
    harness.game.stage_enemy(
        EnemyKind::Brute,
        DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
    );

    let ticks = 30;
    harness.run_ticks(ticks, &[]);
    let taken = PLAYER_MAX_HP - harness.game.player_hp;
    let expected = EnemyKind::Brute.contact_dps() * ticks as f64 * harness.game.tick_dt_secs();
    assert!(
        (taken - expected).abs() < 0.5,
        "half a second against a brute took {taken} hit points, not {expected}",
    );

    // Walk away: nothing is touching, so nothing more is taken.
    harness.game.clear_enemies();
    let after = harness.game.player_hp;
    harness.run_ticks(harness.ticks + 60, &[]);
    assert_eq!(
        harness.game.player_hp, after,
        "hit points kept draining with an empty arena",
    );
}

/// **A crowd is worse than one enemy**, which is the whole argument for a
/// damage *rate* summed over what is touching.
#[test]
fn standing_in_a_crowd_hurts_more_than_standing_next_to_one() {
    let taken = |count: usize| {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let angle = std::f64::consts::TAU / count as f64;
        for i in 0..count {
            let a = angle * i as f64;
            let r = PLAYER_RADIUS + EnemyKind::Grunt.radius() - 0.1;
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, DVec3::new(a.cos(), a.sin(), 0.0) * r);
        }
        harness.run_ticks(6, &[]);
        PLAYER_MAX_HP - harness.game.player_hp
    };
    let one = taken(1);
    let six = taken(6);
    assert!(one > 0.0, "one grunt did no damage at all");
    assert!(
        six > 4.0 * one,
        "six grunts did {six} against one grunt's {one}",
    );
}

/// **Hit points reach zero, and that is the death screen.**
///
/// The clock stops and the kill count freezes, which is what makes the
/// screen a report of the run rather than a live HUD with a caption.
#[test]
fn hit_points_reach_zero_and_the_run_ends_with_its_numbers_frozen() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness.game.stage_enemy(
        EnemyKind::Brute,
        DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
    );
    // Two ticks' worth of a brute, so the death arrives inside this test
    // rather than four seconds into it.
    harness
        .game
        .set_player_hp(EnemyKind::Brute.contact_dps() * 2.0 * harness.game.tick_dt_secs());

    harness.run_ticks(30, &[]);
    assert_eq!(harness.game.state, GameState::Dead, "the run did not end");
    assert_eq!(harness.game.player_hp, 0.0, "hit points went negative");
    let elapsed = harness.game.elapsed;
    let kills = harness.game.kills;
    assert!(elapsed > 0.0, "the run ended before it started");

    harness.run_ticks(harness.ticks + 120, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert_eq!(
        harness.game.elapsed, elapsed,
        "the clock kept running after death",
    );
    assert_eq!(harness.game.kills, kills, "kills kept counting after death");
    assert_eq!(
        harness.game.player_hp, 0.0,
        "a dead player kept taking damage",
    );
    harness.assert_nothing_leaked();
}

/// **A bolt still in flight when the player dies kills nothing afterwards.**
///
/// The death screen freezes the kill count ([`GameState::Dead`] says so),
/// but a bolt the gun fired *before* the death tick keeps flying — and
/// before the fix the ungated sweep kept resolving it, so a bolt that
/// would reach an enemy after the death tick added a kill, a kill sound
/// and a gem drop the frozen counter had already reported.
#[test]
fn a_bolt_in_flight_at_death_kills_nothing_afterwards() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // A runner down the gun's line — far enough that the bolt fired at it
    // is still travelling when the player dies. One bolt kills a runner,
    // so this is the enemy the in-flight bolt would take with it.
    let runner = harness
        .game
        .stage_enemy(EnemyKind::Runner, DVec3::new(7.07, -7.07, 0.0));
    // Tick 0: the only enemy in range is the runner, so the gun fires at it.
    harness.run_ticks(1, &[]);
    assert_eq!(harness.game.bolts_fired(), 1, "the gun did not fire");

    // Now the death source, on the *other* side of the player from the
    // staff — the bolt's path to the runner misses it: the muzzle sits at
    // `STAFF_MUZZLE` from the player's centre, and the brute is tucked in
    // opposite it, clear of the line to the runner. Just enough hit points
    // that the contact damage runs the player out long before the bolt
    // reaches the runner.
    harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(-0.884, -0.884, 0.0));
    harness.game.set_player_hp(5.0);

    // Run to the death tick, and confirm the bolt is still in flight with
    // the runner still alive — the state the finding names.
    let mut death_kills = 0;
    for _ in 0..60 {
        harness.run_ticks(harness.ticks + 1, &[]);
        if harness.game.state == GameState::Dead {
            death_kills = harness.game.kills;
            break;
        }
    }
    assert_eq!(harness.game.state, GameState::Dead, "the run did not end");
    assert!(
        harness.game.enemy_hp(runner).is_some(),
        "the runner died before the player did, so there is no bolt in \
         flight that would kill it",
    );
    assert!(
        !harness.game.bolt_positions().is_empty(),
        "no bolt was in flight at the death tick",
    );

    // More than enough ticks for the in-flight bolt to have reached the
    // runner (it was ~10 units out, and a bolt covers 30 a second).
    harness.run_ticks(harness.ticks + 120, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert_eq!(
        harness.game.kills, death_kills,
        "a bolt in flight at death kept counting kills after the death tick",
    );
    harness.assert_nothing_leaked();
}

/// **The horde keeps moving behind the death screen**, which is what makes
/// it a game over rather than a screenshot.
#[test]
fn the_horde_keeps_converging_after_the_player_dies() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let far = harness
        .game
        .stage_enemy(EnemyKind::Grunt, DVec3::new(0.0, -30.0, 0.0));
    harness.game.stage_enemy(
        EnemyKind::Brute,
        DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
    );
    harness.game.set_player_hp(0.5);

    harness.run_ticks(2, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    let before = harness.game.enemy_position(far).expect("the far grunt");

    harness.run_ticks(harness.ticks + 60, &[]);
    let after = harness.game.enemy_position(far).expect("the far grunt");
    assert!(
        after.y > before.y + 2.0,
        "the horde froze with the clock: {before:?} to {after:?}",
    );
}

/// **The clock counts simulated seconds**, not frames and not ticks.
#[test]
fn the_clock_counts_simulated_seconds() {
    for tick_hz in [20, 60, 144] {
        let mut harness = Harness::staged(60, tick_hz, DVec3::ZERO);
        harness.run_ticks(u64::from(tick_hz) * 3, &[]);
        assert!(
            (harness.game.elapsed - 3.0).abs() < 0.05,
            "{tick_hz} Hz reported {} seconds for three",
            harness.game.elapsed,
        );
    }
}

// ---- the title screen ----------------------------------------------------

/// **The title screen does not play the game.**
///
/// Ten simulated seconds of it leave the world bit-identical. Asserted on
/// the whole of [`RenderState`] — the struct the renderer draws from, and
/// the only thing a player can actually see — rather than on the state enum,
/// because an enum comparison passes just as happily on a simulation that
/// ran every line of its tick and merely mislabelled itself.
///
/// The tick count is asserted too: a run that never ticked would satisfy
/// "nothing changed" without testing anything at all.
#[test]
fn the_title_screen_does_not_advance_the_simulation() {
    let mut harness = Harness::waiting(60, 60);
    assert_eq!(harness.game.state, GameState::WaitingToStart);
    let mut before = RenderState::default();
    harness.game.render_state(&mut before);

    harness.run_ticks(600, &[]);

    let mut after = RenderState::default();
    harness.game.render_state(&mut after);
    assert_eq!(harness.ticks, 600, "the frames ran no ticks");
    assert_eq!(
        before, after,
        "ten seconds of title screen changed the world"
    );
    // …and the same again through the facade's own mirrors, which are what
    // the HUD and the browser gate read. `SPAWN_INTERVAL_START` is half a
    // second, so a spawner that ran for ten of them owed nineteen enemies.
    assert_eq!(harness.game.state, GameState::WaitingToStart);
    assert_eq!(harness.game.enemies_spawned(), 0, "the spawner ran");
    assert_eq!(harness.game.bolts_fired(), 0, "the gun fired");
    assert_eq!(harness.game.elapsed, 0.0, "the run clock ran");
    assert_eq!(harness.game.enemy_count(), 0);
    assert_eq!(harness.game.player, DVec3::ZERO);
}

/// **Either key that starts a run starts it**, and what follows is a run:
/// the clock moves and the spawner deals.
///
/// The run counter is asserted *not* to move, which is the difference
/// between starting and restarting — a start implemented as a restart would
/// hand the session's first run the second run's horde.
#[test]
fn the_start_key_begins_the_run() {
    for key in [KeyCode::Space, KeyCode::KeyR] {
        let mut harness = Harness::waiting(60, 60);
        harness.run_ticks(60, &[]);
        assert_eq!(harness.game.state, GameState::WaitingToStart, "{key:?}");
        let seed = harness.game.run_seed();

        harness.tap(key);
        assert_eq!(
            harness.game.state,
            GameState::Playing,
            "{key:?} did not start the run",
        );
        assert_eq!(harness.game.run, 1, "{key:?} restarted instead of starting");
        assert_eq!(harness.game.run_seed(), seed, "{key:?} re-dealt the run");

        harness.run_ticks(harness.ticks + 120, &[]);
        assert!(
            harness.game.elapsed > 1.5,
            "{key:?}: the clock never started: {}",
            harness.game.elapsed,
        );
        assert!(
            harness.game.enemies_spawned() > 0,
            "{key:?}: the spawner never dealt",
        );
    }
}

/// A restart puts the clock, the hit points, the kills and the player back,
/// and deals a horde that is not the one just played.
///
/// **It lands on the title screen**, which is the one thing here that
/// changed when the start screen arrived: the board it puts back is the
/// same, and it takes a second edge to be playing on it.
#[test]
fn a_restart_puts_everything_back_and_deals_a_new_horde() {
    let mut harness = Harness::new(60, 60);
    harness.play_ticks(600);
    let first_seed = harness.game.run_seed();
    assert!(
        harness.game.enemy_count() > 0,
        "the run has to have dealt something first",
    );
    harness.game.stage_player(DVec3::new(11.0, -7.0, 0.0));

    harness.tap(KeyCode::KeyR);
    assert_eq!(harness.game.state, GameState::WaitingToStart);
    assert_eq!(harness.game.kills, 0);
    assert_eq!(harness.game.player_hp, PLAYER_MAX_HP);
    assert_eq!(
        harness.game.player,
        DVec3::ZERO,
        "the player was not moved back"
    );
    assert_eq!(harness.game.enemy_count(), 0, "the field was not cleared");
    assert_eq!(harness.game.bolt_count(), 0, "a bolt survived the restart");
    assert!(
        harness.game.elapsed < harness.game.tick_dt_secs() * 2.0,
        "the clock was not reset: {}",
        harness.game.elapsed,
    );
    assert_ne!(
        harness.game.run_seed(),
        first_seed,
        "a restart re-dealt the identical run, so the seed advance did nothing",
    );
    harness.assert_nothing_leaked();

    // And the second edge is what plays it, on the board the first one
    // dealt: the seed does not move again.
    let dealt = harness.game.run_seed();
    harness.tap(KeyCode::KeyR);
    assert_eq!(harness.game.state, GameState::Playing);
    assert_eq!(
        harness.game.run_seed(),
        dealt,
        "leaving the title screen re-dealt the run",
    );
}

/// A dead run restarts, which is the only way out of the death screen —
/// **onto the title screen**, and a second press from there into play.
#[test]
fn restarting_after_a_death_starts_a_new_run() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness.game.stage_enemy(
        EnemyKind::Brute,
        DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
    );
    harness.game.set_player_hp(0.5);
    harness.run_ticks(2, &[]);
    assert_eq!(harness.game.state, GameState::Dead);

    harness.tap(KeyCode::Space);
    assert_eq!(harness.game.state, GameState::WaitingToStart);
    assert_eq!(harness.game.player_hp, PLAYER_MAX_HP);
    assert_eq!(harness.game.enemy_count(), 0);

    harness.tap(KeyCode::Space);
    assert_eq!(harness.game.state, GameState::Playing);
}

// ---- spawning ------------------------------------------------------------

/// The spawn rate ramps and then stops ramping, and never runs backwards.
#[test]
fn the_spawn_rate_ramps_to_a_floor_and_no_further() {
    assert_eq!(spawn_interval(0.0), SPAWN_INTERVAL_START);
    assert_eq!(spawn_interval(SPAWN_RAMP_SECONDS), SPAWN_INTERVAL_MIN);
    assert_eq!(
        spawn_interval(SPAWN_RAMP_SECONDS * 10.0),
        SPAWN_INTERVAL_MIN
    );
    assert_eq!(spawn_interval(-5.0), SPAWN_INTERVAL_START, "no time travel");
    let mut previous = f64::INFINITY;
    for step in 0..600 {
        let interval = spawn_interval(f64::from(step));
        assert!(interval <= previous, "the rate went backwards at {step}s");
        assert!(interval >= SPAWN_INTERVAL_MIN);
        previous = interval;
    }
    assert!(
        spawn_interval(SPAWN_RAMP_SECONDS / 2.0) < SPAWN_INTERVAL_START,
        "the ramp does nothing in the middle",
    );
}

/// The spawner deals all three kinds, in something like the table's
/// proportions.
///
/// A share, not a count, and asserted loosely — the point is that no kind is
/// unreachable, which is exactly what a mistyped comparison in
/// [`EnemyKind::from_roll`] would produce and what no other test would see.
#[test]
fn the_spawner_deals_every_kind() {
    let mut counts = [0u32; 3];
    for counter in 0..10_000 {
        let index = match spawn_kind(DEFAULT_SEED, counter) {
            EnemyKind::Grunt => 0,
            EnemyKind::Runner => 1,
            EnemyKind::Brute => 2,
        };
        counts[index] += 1;
    }
    for (index, kind) in EnemyKind::ALL.iter().enumerate() {
        assert!(
            counts[index] > 500,
            "{kind:?} came up {} times in ten thousand",
            counts[index],
        );
    }
    assert!(
        counts[0] > counts[1] && counts[1] > counts[2],
        "the mix is not the table's: {counts:?}",
    );
}

/// **The field never exceeds its cap**, and the cap is genuinely reached
/// rather than being a number nothing gets near.
#[test]
fn the_field_never_exceeds_the_enemy_cap() {
    const CAP: usize = 25;
    let mut harness = Harness::with_setup(
        60,
        &Setup {
            max_enemies: CAP,
            ..Setup::default()
        },
    );
    // **The player is kept alive by hand**, because the spawner only runs
    // while the run is: a stationary player is dead inside ten seconds, and
    // what would then be under test is the death screen rather than the cap.
    let mut peak = 0;
    while harness.ticks < 7_200 {
        harness.game.set_player_hp(PLAYER_MAX_HP);
        // The spawner does not run while the level-up screen is up, so a
        // run that walked past one would stop spawning and the cap would
        // never be reached — see `Harness::play_ticks`.
        if harness.game.state == GameState::LevelUp {
            harness.game.key_event(KeyCode::Digit1, true);
            harness.game.key_event(KeyCode::Digit1, false);
        }
        harness.run_ticks(harness.ticks + 1, &[]);
        peak = peak.max(harness.game.enemy_count());
        assert!(
            harness.game.enemy_count() <= CAP,
            "tick {}: {} enemies against a cap of {CAP}",
            harness.ticks,
            harness.game.enemy_count(),
        );
    }
    assert_eq!(peak, CAP, "the cap was never reached, so it is untested");
    assert!(
        harness.game.enemies_spawned() > CAP as u64,
        "only {} were ever spawned, so the cap did nothing",
        harness.game.enemies_spawned(),
    );
    harness.assert_nothing_leaked();
}

/// The list and the entity index stay in step across a `swap_remove`.
///
/// The failure this guards is silent and specific: `swap_remove` moves the
/// *last* enemy into the hole, and the map entry that named its old slot has
/// to follow it. Forget that and every later lookup of the moved enemy
/// resolves to the wrong body — a bolt damages a stranger, and nothing
/// panics.
#[test]
fn an_enemy_index_survives_a_swap_remove() {
    let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
    // Three in a row up the gun's line, so the nearest dies first and the
    // last one in the list is moved into its slot.
    let near = harness
        .game
        .stage_enemy(EnemyKind::Runner, DVec3::new(0.0, 0.0, 0.0));
    let mid = harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 4.0, 0.0));
    let far = harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 6.0, 0.0));

    harness.run_ticks(30, &[]);
    assert_eq!(harness.game.kills, 1, "the nearest should have died");
    assert!(harness.game.enemy_hp(near).is_none());
    // Both survivors still resolve, and to themselves: a broken index would
    // hand back one of them for the other, or nothing at all.
    assert!(
        harness.game.enemy_hp(mid).is_some(),
        "the middle one is lost"
    );
    assert!(harness.game.enemy_hp(far).is_some(), "the far one is lost");
    assert!(
        harness.game.enemy_position(far).expect("the far one").y
            > harness.game.enemy_position(mid).expect("the middle one").y,
        "the two swapped identities",
    );
    harness.assert_nothing_leaked();
}

// ---- the entity lifecycle, under pressure --------------------------------

/// **Thousands of bodies come and go, and nothing is left behind.**
///
/// Staged rather than played, because the point is the *count*: three
/// thousand colliders inserted into the broadphase, indexed, and taken out
/// again, with the entity count, the collider count and the destruction
/// queue all accounted for exactly at each stage. A run that reached three
/// thousand through the spawner would take a simulated hour.
#[test]
fn thousands_of_bodies_come_and_go_without_leaking() {
    const BODIES: usize = 3_000;
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let baseline = harness.game.entity_count();
    assert_eq!(baseline, 1, "a staged board is the player and nothing else");

    // A grid well clear of the player, so nothing is shot and nothing does
    // contact damage while it is being counted.
    let mut staged = Vec::with_capacity(BODIES);
    for i in 0..BODIES {
        let x = -40.0 + (i % 60) as f64 * 1.3;
        let y = 20.0 + (i / 60) as f64 * 0.25;
        staged.push(
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, DVec3::new(x, y, 0.0)),
        );
    }
    assert_eq!(harness.game.enemy_count(), BODIES);
    assert_eq!(
        harness.game.collider_count(),
        BODIES,
        "colliders went missing"
    );
    assert_eq!(harness.game.entity_count(), 1 + BODIES);

    // One tick, so every one of them is steered, queried and clamped at
    // least once — a leak that only happens on the hot path would otherwise
    // never be reached.
    harness.run_ticks(1, &[]);
    harness.assert_nothing_leaked();

    // Cleared through the **restart** the game itself runs, not through a
    // test helper: a bulk teardown is the thing under test here, and a
    // helper that removed the colliders would prove only that the helper
    // does.
    harness.tap(KeyCode::KeyR);
    assert_eq!(harness.game.enemy_count(), 0);
    assert_eq!(
        harness.game.collider_count(),
        0,
        "the broadphase kept {BODIES} invisible walls",
    );

    // Extra ticks so a death that lands on the last one is still counted;
    // the sweep itself is immediate, since `Server::tick` runs it between
    // the module and the snapshot.
    harness.run_ticks(harness.ticks + 3, &[]);
    assert_eq!(
        harness.game.pending_despawns(),
        0,
        "the queue never emptied"
    );
    assert_eq!(
        harness.game.entity_count(),
        baseline,
        "{BODIES} bodies left something behind",
    );
    harness.assert_nothing_leaked();
    // And the run genuinely churned, which is what stops all of the above
    // being true of a game that did nothing.
    assert!(harness.game.enemies_spawned() >= BODIES as u64);
    assert_eq!(staged.len(), BODIES);
}

/// **A long run leaks nothing**, checked on every tick rather than at the
/// end — a leak that is cleaned up before the last tick is still a leak
/// while it is happening, and the failure names the tick it started on.
///
/// Smaller than `thousands_of_bodies_come_and_go_without_leaking` in bodies
/// and much larger in ticks, because the two catch different things: that
/// one catches a collider left behind by a bulk removal, this one catches a
/// body leaked by the spawn/kill/expire paths a thousand ticks in.
#[test]
fn a_long_run_leaks_nothing() {
    let mut harness = Harness::with_setup(
        60,
        &Setup {
            max_enemies: 120,
            ..Setup::default()
        },
    );
    let mut peak = 0;
    while harness.ticks < 9_000 {
        // The kite outruns the horde on its own now that spawns arrive on
        // the ring inside the arena — a kiting player would never die, and
        // the restart would never be exercised. Every other lap the player
        // stands still instead: the crowd catches up, the run ends, and
        // `stand_still`'s bookkeeping starts the next one.
        if harness.ticks % (2 * KITE_PERIOD) < KITE_PERIOD {
            harness.play_ticks(harness.ticks + 1);
        } else {
            harness.stand_still(harness.ticks + 1);
        }
        harness.assert_nothing_leaked();
        peak = peak.max(harness.game.entity_count());
    }

    let spawned = harness.game.enemies_spawned();
    let fired = harness.game.bolts_fired();
    assert!(
        spawned >= 250,
        "only {spawned} enemies were ever spawned, which is not enough churn",
    );
    assert!(
        fired >= 400,
        "only {fired} bolts were ever fired, which is not enough churn",
    );
    assert!(
        harness.game.kills > 0,
        "the run killed nothing, so the enemy death path was never exercised",
    );
    assert!(
        harness.restarts > 0,
        "the soak never finished a run, so the restart — which wipes the \
         whole field and is the largest single piece of churn in the game — \
         was never exercised",
    );
    // The most the world can legitimately hold: the player, the enemy cap,
    // every bolt that can be in the air at once and every pickup the ground
    // will keep — plus as many again waiting for the deferred sweep. Derived
    // rather than measured: a number taken from a passing run breaks on the
    // next tuning change for a reason that is not a leak.
    //
    // **The gems were missing from this and it passed anyway**, because a
    // soak that killed a little less left fewer of them lying about than the
    // enemy cap did. They are the largest population in the run by a wide
    // margin, so leaving them out was a bound on the wrong thing; the
    // per-tick equality in `assert_nothing_leaked` is what carries the exact
    // claim, and this is the growth bound over it.
    //
    // Potions needed no term of their own here: they are the same list and
    // the same ceiling, which is the whole reason `PickupKind` is a variant
    // rather than a second population.
    let bolts_in_flight = (BOLT_LIFE / FIRE_COOLDOWN).ceil() as usize + 1;
    let ceiling = 2 * (1 + 120 + bolts_in_flight + MAX_PICKUPS);
    assert!(
        peak <= ceiling,
        "the world peaked at {peak} entities against a ceiling of {ceiling}",
    );
    assert!(
        peak > 1,
        "the world never grew, so the ceiling proves nothing"
    );

    harness.run_ticks(harness.ticks + 3, &[]);
    assert_eq!(
        harness.game.pending_despawns(),
        0,
        "the destruction queue never emptied",
    );
    harness.assert_nothing_leaked();
    crcbl::log::info!(
        "soak: {spawned} spawned, {fired} bolts, {} kills, {} restarts, \
         peak {peak} entities",
        harness.game.kills,
        harness.restarts,
    );
}

// ---- determinism ---------------------------------------------------------

/// **The determinism criterion.** The same script replays to the same
/// outcome, twice, bit-identically.
///
/// Everything observable is compared, not just the kill count: a run that
/// agreed about the number and disagreed about where the horde was would be
/// a coincidence, not determinism. That includes every enemy position, which
/// is what makes it a test of the separation sum's order as well — see
/// `steer_enemies`.
#[test]
fn the_same_script_replays_bit_identically() {
    let run = || {
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                max_enemies: 120,
                ..Setup::default()
            },
        );
        harness.play_ticks(4_800);
        (
            harness.game.elapsed,
            harness.game.kills,
            harness.game.player_hp,
            harness.game.player,
            harness.game.enemies(),
            harness.game.bolts(),
            harness.game.enemies_spawned(),
            harness.game.bolts_fired(),
            // The loot, which is where the potion roll shows up: a drop
            // decided by anything but the seed and the kill counter would
            // put a different flask on a different patch of ground here.
            harness.game.pickups_on_the_ground(),
            harness.game.potions_dropped(),
        )
    };
    let first = run();
    assert!(
        first.6 > 50,
        "the reference run spawned {}, which is not enough to compare",
        first.6,
    );
    assert!(
        !first.4.is_empty(),
        "the reference run ended with an empty field"
    );
    assert!(first.7 > 0, "the reference run fired nothing");
    assert!(first.1 > 0, "the reference run killed nothing");
    assert!(
        first.9 > 0,
        "the reference run dropped no potion, so the roll is compared but \
         never exercised",
    );
    assert!(
        !first.8.is_empty(),
        "the reference run left no loot to compare"
    );
    assert_eq!(first, run());
}

/// The bits of one `DVec3`, so a comparison is bit-identity rather than
/// numeric equality — `-0.0 == 0.0` and a `NaN` equals nothing, and neither
/// is what "the parallel pass produced the same answer" means.
fn bits(v: DVec3) -> (u64, u64, u64) {
    (v.x.to_bits(), v.y.to_bits(), v.z.to_bits())
}

/// One tick's observable state, folded to a 64-bit hash.
///
/// The state the parallel pass can perturb — every movable position plus
/// the counters — hashed per tick so a divergence is reported at the tick
/// it happened rather than at the end of the run. The fold is
/// [`crcbl::core::rand::hash_u64`] over a fixed order, which is integer
/// arithmetic identical on every target. A hash can collide and the values
/// cannot, so it is a where-not-whether instrument, not a substitute for
/// the end-of-run comparison `the_same_script_replays_bit_identically_at_
/// every_worker_count` still makes.
fn state_hash(game: &Game) -> u64 {
    let mut h: u64 = 0x5348_4153_4821_21D0;
    let mut fold = |value: u64| h = crcbl::core::rand::hash_u64(h, value);
    fold(game.kills);
    fold(game.player_hp.to_bits());
    fold(game.player.x.to_bits());
    fold(game.player.y.to_bits());
    for position in game.enemy_positions() {
        fold(position.x.to_bits());
        fold(position.y.to_bits());
        fold(position.z.to_bits());
    }
    for bolt in game.bolts() {
        fold(bolt.position.x.to_bits());
        fold(bolt.position.y.to_bits());
        fold(bolt.position.z.to_bits());
    }
    for (position, kind) in game.pickups_on_the_ground() {
        fold(position.x.to_bits());
        fold(position.y.to_bits());
        fold(position.z.to_bits());
        match kind {
            PickupKind::Health => fold(1),
            // Two folds for the payloaded arm, one for the bare one, so
            // the kinds fold structurally different sequences and an Xp
            // of any amount can never hash like a potion.
            PickupKind::Xp(amount) => {
                fold(0);
                fold(amount);
            }
        }
    }
    h
}

/// Runs one [`steer_enemies`] over a staged crowd at a chosen worker count,
/// and hands back the pool it used with the velocities it decided.
///
/// The pool comes back because the test that calls this has to prove the
/// parallel run was *parallel*; see
/// [`steering_is_bit_identical_however_many_workers_run_it`].
fn steer_a_staged_crowd(workers: Option<usize>, crowd: usize) -> (Pool, Vec<DVec3>) {
    let mut harness = Harness::with_setup(
        60,
        &Setup {
            max_enemies: crowd,
            workers,
            ..Setup::default()
        },
    );
    assert_eq!(
        harness.game.stage_field(crowd),
        crowd,
        "the field did not fill"
    );
    // **Two seconds of play before the pass being measured**, and it is
    // load-bearing rather than warm-up. A freshly staged field is a regular
    // lattice, so an enemy's neighbours are symmetric about it and their
    // separation pushes sum to the same `f64` in either order — a
    // reassociated sum would be invisible. Measured: with the crowd staged
    // and not run, reversing the neighbour list on worker threads left this
    // test green. A field that has converged for two seconds is irregular
    // and the same mutation turns it red.
    harness.play_ticks(120);

    let mut pool = steer_pool(workers).expect("a steering pool");
    let shared = Arc::clone(&harness.game.shared);
    let world = harness.game.session.server_mut().world_mut();
    let mut logic = lock(&shared);
    steer_enemies(&mut logic, world, &mut pool);
    let velocities = logic.steer_velocities.clone();
    drop(logic);
    (pool, velocities)
}

/// **The whole adoption in one assertion: the crowd steers to the same bits
/// however many threads decided them.**
///
/// Three worker counts against the browser's own pool — zero workers, every
/// chunk on the calling thread — and the comparison is over the raw `f64`
/// bits, because a separation sum that had been reassociated would still be
/// numerically close and is exactly the defect this exists to catch.
///
/// Two things stop it being vacuous, and both are asserted rather than
/// argued. The crowd splits into more chunks than one, so `par_for` takes
/// the parallel path instead of the single-chunk shortcut it runs inline by
/// construction. And the pool the parallel run used is then made to prove it
/// spreads work: a probe `par_for` on that same pool, whose chunks each wait
/// for a second thread to arrive, so a pool that ran everything on the
/// caller fails here instead of quietly making the comparison above a
/// comparison of two serial runs.
#[test]
fn steering_is_bit_identical_however_many_workers_run_it() {
    // Thirteen chunks at `STEER_CHUNK`, which is more than this machine has
    // cores and comfortably more than one.
    const CROWD: usize = 800;
    assert!(
        CROWD.div_ceil(STEER_CHUNK) > 1,
        "a crowd of {CROWD} is one chunk, which `par_for` runs inline",
    );

    let (serial_pool, serial) = steer_a_staged_crowd(Some(0), CROWD);
    assert_eq!(serial_pool.workers(), 0, "the serial run had workers");
    assert!(
        serial.len() > STEER_CHUNK,
        "the field was down to {} enemies by the time the pass ran",
        serial.len(),
    );
    assert!(
        serial.iter().any(|v| *v != DVec3::ZERO),
        "every enemy was left standing still, so nothing was compared",
    );
    assert!(
        serial.iter().any(|v| bits(*v) != bits(serial[0])),
        "every enemy steered identically, so a chunk that dropped its \
         results would still match",
    );
    let serial_bits: Vec<_> = serial.iter().copied().map(bits).collect();

    for workers in [1_usize, 3, 7] {
        let (mut pool, parallel) = steer_a_staged_crowd(Some(workers), CROWD);
        assert_eq!(pool.workers(), workers, "the pool was not the size asked");
        let parallel_bits: Vec<_> = parallel.iter().copied().map(bits).collect();
        assert_eq!(
            serial_bits, parallel_bits,
            "{workers} workers steered the crowd differently",
        );

        // The pool that just ran the pass, made to show it is not running
        // everything on this thread. Bounded, so a pool whose workers never
        // arrive goes red rather than hanging.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let arrived = std::sync::atomic::AtomicUsize::new(0);
        let threads = Mutex::new(std::collections::HashSet::new());
        let mut probe = vec![0_u8; 64];
        pool.par_for(&mut probe, 8, |_, _| {
            lock_set(&threads).insert(std::thread::current().id());
            arrived.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            while arrived.load(std::sync::atomic::Ordering::SeqCst) < 2
                && std::time::Instant::now() < deadline
            {
                std::thread::yield_now();
            }
        });
        assert!(
            lock_set(&threads).len() >= 2,
            "the {workers}-worker pool ran every chunk on the calling \
             thread, so the comparison above compared two serial runs: {:?}",
            lock_set(&threads),
        );
    }
}

/// The same lock recovery `lock` does, for the probe's thread set.
fn lock_set<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// **The design's `--threads 1` versus `--threads N` gate, on a whole
/// run rather than one pass.**
///
/// `docs/backlog.md` records this as the test the pool did not have because
/// nothing was driving it: the same input script at every worker count.
/// `--workers` is what drives it, and horde is the sim it was waiting for.
///
/// It is a strictly stronger claim than
/// [`steering_is_bit_identical_however_many_workers_run_it`] because it
/// closes the loop: a velocity difference too small to matter in one tick
/// still moves a body, which moves the broadphase, which changes the next
/// tick's neighbourhoods — so twenty-four hundred ticks of it end
/// somewhere visibly different, and the kill count and the loot are in the
/// comparison to say so.
///
/// The comparison is **per tick**: a [`state_hash`] recorded after every
/// tick, compared by position, so a divergence is reported at the tick it
/// happened rather than at the end of the run. The end-of-run state is
/// still compared for real afterwards, because a hash can collide and the
/// values cannot.
#[test]
fn the_same_script_replays_bit_identically_at_every_worker_count() {
    let run = |workers: Option<usize>| {
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                max_enemies: 400,
                workers,
                ..Setup::default()
            },
        );
        // Staged rather than waited for: the spawner needs minutes to reach
        // a crowd that splits into more than one chunk, and the gun clears
        // the field faster than it fills. This is the same fixture the scale
        // measurement uses, for the same reason.
        let staged = harness.game.stage_field(300);
        // One hash per tick, so the comparison below can name the tick the
        // runs first disagreed on. Split at the midway read so the field is
        // still large when it is sampled — a death and restart wipe it.
        let mut hashes = Vec::with_capacity(2_700);
        harness.play_ticks_into(300, |game| hashes.push(state_hash(game)));
        // Read midway as well as at the end, because the field does not
        // survive: three hundred enemies on top of the wizard is a death and
        // a restart, which wipes it. This is the sample taken while the
        // crowd is still large enough that the pass was genuinely splitting
        // — and the restart it dies into is churn the comparison covers for
        // free.
        let midway = harness.game.enemy_positions();
        harness.play_ticks_into(2_700, |game| hashes.push(state_hash(game)));
        (
            staged,
            midway,
            harness.game.elapsed,
            harness.game.kills,
            harness.game.player_hp,
            harness.game.player,
            harness.game.enemies(),
            harness.game.bolts(),
            harness.game.enemies_spawned(),
            harness.game.bolts_fired(),
            harness.game.pickups_on_the_ground(),
            hashes,
        )
    };

    let serial = run(Some(0));
    let serial_hashes = &serial.11;
    assert!(
        serial.1.len() > STEER_CHUNK,
        "the reference run was down to {} enemies by tick 300, which is one \
         steering chunk — the worker counts below would have nothing to split",
        serial.1.len(),
    );
    assert!(serial.3 > 0, "the reference run killed nothing");
    assert!(serial.9 > 0, "the reference run fired nothing");

    for workers in [1_usize, 3] {
        let candidate = run(Some(workers));
        let divergence = serial_hashes
            .iter()
            .zip(&candidate.11)
            .position(|(a, b)| a != b);
        assert_eq!(
            divergence,
            None,
            "the run at {workers} workers diverged from the serial one at \
             tick {tick}: state hash {serial_hash:016x} against \
             {candidate_hash:016x}",
            tick = divergence.unwrap_or(0) + 1,
            serial_hash = serial_hashes[divergence.unwrap_or(0)],
            candidate_hash = candidate.11[divergence.unwrap_or(0)],
        );
        assert_eq!(serial, candidate, "the run diverged at {workers} workers",);
    }
    // And the default, which is whatever this machine had to spare — the
    // configuration every other test in this file, and every CI run, has
    // actually been using.
    let candidate = run(None);
    let divergence = serial_hashes
        .iter()
        .zip(&candidate.11)
        .position(|(a, b)| a != b);
    assert_eq!(
        divergence,
        None,
        "the run at the machine's own worker count diverged from the serial \
         one at tick {tick}: state hash {serial_hash:016x} against \
         {candidate_hash:016x}",
        tick = divergence.unwrap_or(0) + 1,
        serial_hash = serial_hashes[divergence.unwrap_or(0)],
        candidate_hash = candidate.11[divergence.unwrap_or(0)],
    );
    assert_eq!(
        serial, candidate,
        "the run diverged at the machine's own worker count"
    );
}

/// **The frame rate is not the tick rate.** The same script reaches the same
/// place at 20, 60 and 240 frames a second, because the simulation runs on
/// its own fixed step and the frame loop only decides how often it is asked
/// to.
#[test]
fn the_same_run_plays_out_the_same_at_every_frame_rate() {
    type Observed = (f64, u64, f64, DVec3, Vec<EnemyView>);
    let mut reference: Option<Observed> = None;
    for frame_hz in [20, 60, 240] {
        let mut harness = Harness::with_setup(
            frame_hz,
            &Setup {
                max_enemies: 80,
                ..Setup::default()
            },
        );
        harness.play_ticks(900);
        assert_eq!(harness.ticks, 900);
        let observed = (
            harness.game.elapsed,
            harness.game.kills,
            harness.game.player_hp,
            harness.game.player,
            harness.game.enemies(),
        );
        match &reference {
            None => {
                assert!(!observed.4.is_empty(), "the reference run spawned nothing");
                reference = Some(observed);
            }
            Some(expected) => assert_eq!(
                &observed, expected,
                "{frame_hz} fps played a different game",
            ),
        }
    }
}

/// Two games given the same seed play the same game, and two given different
/// seeds do not — without the second half the seed would be decoration.
#[test]
fn two_games_on_one_seed_play_the_same_game() {
    let run = |seed: u64| {
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                seed,
                max_enemies: 80,
                ..Setup::default()
            },
        );
        harness.play_ticks(600);
        harness.game.enemies()
    };
    for seed in [1, DEFAULT_SEED] {
        assert_eq!(run(seed), run(seed), "seed {seed} was not reproducible");
    }
    assert_ne!(run(1), run(2), "two seeds played the same game");
}

/// …and a *restart* is predictable too, so a recorded script replayed from a
/// fresh game meets the same horde on its second run as the recording did on
/// its second run.
#[test]
fn two_games_with_one_seed_agree_about_every_run() {
    let seeds = |restarts: u32| {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // One tick to spend the harness's queued start edge, or the first
        // `restart_run` below would find a game still on the title screen
        // and merely start it.
        harness.run_ticks(1, &[]);
        let mut seen = vec![harness.game.run_seed()];
        for _ in 0..restarts {
            // The whole restart, not just its first edge: a second `R` on
            // the title screen *starts* rather than restarts, so a single
            // tap per iteration would deal the same run twice.
            harness.restart_run();
            seen.push(harness.game.run_seed());
        }
        seen
    };
    for restarts in 0..4 {
        assert_eq!(
            seeds(restarts),
            seeds(restarts),
            "run {restarts} was not reproducible",
        );
    }
    let four = seeds(3);
    assert_eq!(four.len(), 4);
    for (index, seed) in four.iter().enumerate() {
        assert!(
            !four[..index].contains(seed),
            "restart {index} re-dealt an earlier run's seed",
        );
    }
}

// ---- the movement action -------------------------------------------------

/// [`MOVE_SECTOR`] is the angle it claims to be, computed rather than
/// eyeballed.
///
/// A transcribed constant is a transcription until something checks it, and
/// nothing else in this file would notice a digit dropped from the middle of
/// it: the eight sectors would simply stop being equal.
#[test]
fn the_eight_sectors_are_the_angle_they_claim() {
    let want = (std::f32::consts::PI / 8.0).sin();
    assert!(
        (MOVE_SECTOR - want).abs() < 1e-6,
        "MOVE_SECTOR is {MOVE_SECTOR}, sin(π/8) is {want}",
    );

    // Either side of the boundary between "due east" and "north-east", at
    // full deflection. One degree in from each side, so the check is about
    // the split and not about a float landing exactly on it.
    let at = |degrees: f32| {
        let radians = degrees.to_radians();
        eight_way(radians.cos(), radians.sin())
    };
    assert_eq!(at(21.5), (false, false, false, true), "east");
    assert_eq!(at(23.5), (true, false, false, true), "north-east");
    assert_eq!(at(66.5), (true, false, false, true), "still north-east");
    assert_eq!(at(68.5), (true, false, false, false), "north");
    assert_eq!(at(180.0), (false, false, true, false), "west");
    assert_eq!(at(-90.0), (false, true, false, false), "south");
}

/// A thumb that has barely moved asks for nothing at all.
#[test]
fn a_stick_inside_the_dead_zone_asks_for_nothing() {
    assert_eq!(eight_way(0.0, 0.0), (false, false, false, false));
    let inside = MOVE_DEAD_ZONE * 0.99;
    assert_eq!(
        eight_way(inside, 0.0),
        (false, false, false, false),
        "a nudge inside the dead zone started a walk",
    );
    // …and a hair outside it does ask, or the dead zone is the whole pad.
    let outside = MOVE_DEAD_ZONE * 1.01;
    assert_eq!(eight_way(outside, 0.0), (false, false, false, true));
}

/// **The keyboard still asks for exactly what it always asked for.**
///
/// The four button actions became one `Axis2`, and the thing that must not
/// have changed is what a key press means. Driven through the real action
/// map rather than through [`eight_way`] alone, because the composite's
/// normalisation is half of the answer.
#[test]
fn the_keyboard_asks_for_what_it_always_did() {
    let mut game = Game::new(true, DEFAULT_TICK_HZ).expect("a game");
    let directions = |game: &Game| {
        let (x, y) = game.action_map.axis2(ACTION_MOVE);
        eight_way(x, y)
    };

    game.key_event(KeyCode::KeyW, true);
    game.tick();
    assert_eq!(directions(&game), (true, false, false, false), "W is north");

    game.key_event(KeyCode::KeyD, true);
    game.tick();
    assert_eq!(
        directions(&game),
        (true, false, false, true),
        "W and D together are north-east, not one of them",
    );

    game.key_event(KeyCode::KeyS, true);
    game.tick();
    assert_eq!(
        directions(&game),
        (false, false, false, true),
        "W and S cancel, which is what four separate buttons also did",
    );

    // The arrows are the second composite, and they mean the same thing.
    for key in [KeyCode::KeyW, KeyCode::KeyD, KeyCode::KeyS] {
        game.key_event(key, false);
    }
    game.key_event(KeyCode::ArrowLeft, true);
    game.tick();
    assert_eq!(directions(&game), (false, false, true, false), "west");
}

/// **The stick and the keyboard are one action**, and the stick's value
/// survives the ticks between the frames a finger reports on.
#[test]
fn the_stick_drives_the_same_action_the_keys_do() {
    let mut game = Game::new(true, DEFAULT_TICK_HZ).expect("a game");

    game.stick_moved(0.0, -1.0);
    game.tick();
    let (x, y) = game.action_map.axis2(ACTION_MOVE);
    assert!(
        (x.abs() < 1e-6) && (y + 1.0).abs() < 1e-6,
        "the stick reached the move action as ({x}, {y})",
    );
    assert_eq!(eight_way(x, y), (false, true, false, false), "south");

    // Five ticks with nothing reported: a finger resting on the glass
    // moves nothing and sends nothing, and the wizard must keep walking.
    for _ in 0..5 {
        game.tick();
    }
    let (_, y) = game.action_map.axis2(ACTION_MOVE);
    assert!(
        (y + 1.0).abs() < 1e-6,
        "the stick centred itself under a thumb that never let go: y is {y}",
    );

    // Centred, and the walk stops.
    game.stick_moved(0.0, 0.0);
    game.tick();
    assert_eq!(game.action_map.axis2(ACTION_MOVE), (0.0, 0.0));
}

// ---- experience, pickups and the level-up --------------------------------

/// Every field of an [`Intent`] survives the wire form, **and comes back
/// off it as itself**.
///
/// The choice is the one that could silently not: it is two bits at the top
/// of a `u8` that already carried five flags, and a shift one place out
/// would take a button with it — in either direction, which is why the
/// decode is checked here rather than trusted to be the encode read
/// backwards.
#[test]
fn the_wire_form_carries_every_bit_of_intent() {
    let mut seen = Vec::new();
    for choose in 0..=UPGRADE_CHOICES as u8 {
        for flags in 0..32u8 {
            let intent = Intent {
                up: flags & 1 != 0,
                down: flags & 2 != 0,
                left: flags & 4 != 0,
                right: flags & 8 != 0,
                restart: flags & 16 != 0,
                choose,
            };
            let wire = intent.to_wire();
            assert!(
                !seen.contains(&wire),
                "{intent:?} shares a wire form with an earlier intent",
            );
            seen.push(wire);
            assert_eq!(Intent::from_wire(&[wire]), Some(intent));
        }
    }
    assert_eq!(seen.len(), 4 * 32, "the loop did not cover what it claims");
}

/// Puts one intent on the wire and runs exactly one tick of the pair.
///
/// The whole input path and nothing beside it: no action map, no queued key
/// event, no write to the shared cell. The same three calls in the same
/// order [`Game::tick`] makes, so what these tests exercise is the path the
/// game plays through.
fn send_one_tick(game: &mut Game, intent: Intent) {
    game.sim_time += game.tick_period;
    let (server, client) = game.session.both_mut();
    client.set_input(vec![intent.to_wire()]);
    client.update(game.sim_time);
    assert_eq!(
        server.update(game.sim_time),
        1,
        "one tick period in must be exactly one server tick out",
    );
    client.update(game.sim_time);
}

/// **The wizard walks on bytes that only ever existed as bytes.**
///
/// The only thing that happens here is `Client::set_input` with an intent's
/// wire form and one tick of the pair. The run starting, the figure turning
/// and the arena position moving are the whole input path — encode, seal,
/// transport, unseal, decode, apply — and a server that drops what a client
/// sends leaves the game on its title screen with the wizard at the origin,
/// because there is no other way into this simulation.
#[test]
fn the_wizard_walks_on_an_intent_that_only_ever_travelled_as_bytes() {
    let mut game = Game::new(true, DEFAULT_TICK_HZ).expect("a headless game always starts");
    assert_eq!(lock(&game.shared).state, GameState::WaitingToStart);
    assert_eq!(lock(&game.shared).player_pos, DVec3::ZERO);

    send_one_tick(
        &mut game,
        Intent {
            restart: true,
            ..Intent::default()
        },
    );
    assert_eq!(
        lock(&game.shared).state,
        GameState::Playing,
        "the start edge never reached the simulation",
    );

    // A held direction. The velocity is written this tick and integrated at
    // the top of the next one, so the position moves on the tick after —
    // which is what the third frame below reads.
    send_one_tick(
        &mut game,
        Intent {
            right: true,
            ..Intent::default()
        },
    );
    {
        let logic = lock(&game.shared);
        assert!(logic.player_moving, "the walk never started");
        assert_eq!(logic.player_facing, Facing::Right);
    }

    // And a frame that holds nothing is not the previous one repeated: the
    // server clears its queue every tick, and this one decodes to a player
    // holding no key at all.
    send_one_tick(&mut game, Intent::default());
    let step = PLAYER_SPEED * game.tick_dt_secs();
    {
        let logic = lock(&game.shared);
        assert!(
            (logic.player_pos.x - step).abs() < 1e-9,
            "one tick of a held right key is {step} and the wizard is at {}",
            logic.player_pos.x,
        );
        assert!(!logic.player_moving, "the walk never stopped");
    }
    assert_eq!(
        game.session.server().dropped_input_count(),
        0,
        "a frame was refused rather than applied",
    );
}

/// **A frame from something that is not this build is refused.** These are
/// the only bytes in the game a peer chooses: the count of them and the bits
/// inside them.
#[test]
fn a_frame_this_build_did_not_write_is_refused() {
    assert!(
        Intent::from_wire(&[INTENT_FLAGS]).is_some(),
        "every bit this build defines must decode",
    );

    // The bit above the choice, which no `to_wire` of this game sets.
    assert_eq!(Intent::from_wire(&[!INTENT_FLAGS]), None);
    assert_eq!(Intent::from_wire(&[u8::MAX]), None);

    // And the length is the other half — `set_input` takes a `Vec`, so how
    // many bytes arrive is the peer's choice too.
    assert_eq!(Intent::from_wire(&[]), None);
    assert_eq!(Intent::from_wire(&[0, 0]), None);
}

/// Several frames in one tick — a client whose clock ran ahead — fold into
/// one intent: the directions are the latest word, the restart edge survives
/// from whichever frame raised it, and the **first** choice named wins.
#[test]
fn several_frames_in_one_tick_fold_into_one_intent() {
    let frames: Vec<(TickId, Vec<u8>)> = [
        Intent {
            up: true,
            restart: true,
            ..Intent::default()
        },
        Intent {
            right: true,
            choose: 1,
            ..Intent::default()
        },
        Intent {
            right: true,
            choose: 3,
            ..Intent::default()
        },
    ]
    .iter()
    .enumerate()
    .map(|(i, intent)| (TickId::from_raw(i as u64), vec![intent.to_wire()]))
    .collect();

    assert_eq!(
        Intent::from_inputs(ClientInputs::new(&frames, 0)),
        Intent {
            up: false,
            down: false,
            left: false,
            right: true,
            restart: true,
            // The second frame's, not the third's: a stray later press must
            // not overwrite the upgrade the player chose.
            choose: 1,
        },
    );

    // An unreadable frame is skipped rather than read as an empty intent,
    // which would be the player letting go of everything.
    let with_rubbish = vec![
        (TickId::ZERO, vec![u8::MAX]),
        (
            TickId::ZERO,
            vec![
                Intent {
                    up: true,
                    ..Intent::default()
                }
                .to_wire(),
            ],
        ),
    ];
    assert!(Intent::from_inputs(ClientInputs::new(&with_rubbish, 0)).up);

    // A tick nothing arrived for is a player holding nothing.
    assert_eq!(
        Intent::from_inputs(ClientInputs::empty()),
        Intent::default()
    );
}

/// **XP drops where an enemy died, is collected on contact, and nothing is
/// left behind.**
///
/// All three halves, because each fails silently on its own: a gem that
/// never dropped, a gem that could not be picked up, and a gem picked up
/// whose collider stayed in the broadphase as an invisible obstacle.
#[test]
fn an_enemy_that_dies_drops_a_gem_and_walking_over_it_banks_the_experience() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let at = DVec3::new(4.0, 0.0, 0.0);
    harness.game.stage_enemy(EnemyKind::Grunt, at);
    assert_eq!(harness.game.pickup_count(), 0);

    // Shot until it dies. Six bolts' worth of ticks is plenty for a grunt,
    // which takes two.
    harness.run_ticks(harness.ticks + 90, &[]);
    assert_eq!(harness.game.enemy_count(), 0, "the grunt never died");
    assert_eq!(harness.game.kills, 1);
    assert_eq!(harness.game.pickup_count(), 1, "no gem was dropped");
    let gem = harness.game.pickup_positions()[0];
    // Where it *died*, which is not quite where it was staged: a grunt walks
    // towards the player while it is being shot. The bound is one step of
    // its own travel over the ticks it survived, not a shrug.
    assert!(
        (gem - at).length() < 2.0 && gem.x > 1.0,
        "the gem landed at {gem:?}, not on the path the grunt walked from {at:?}",
    );
    assert_eq!(harness.game.xp(), 0, "the gem banked itself");
    harness.assert_nothing_leaked();

    // Walk onto it. The player is at the origin and the gem is four units
    // away; PLAYER_SPEED covers that in well under a second.
    harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);
    assert_eq!(
        harness.game.xp(),
        EnemyKind::Grunt.xp(),
        "walking over the gem banked nothing",
    );
    assert_eq!(harness.game.pickup_count(), 0, "the gem was not removed");
    harness.assert_nothing_leaked();
}

/// A brute's gem is worth more than a grunt's, which is what makes the
/// level-up rate track effort rather than bodies.
#[test]
fn a_brutes_gem_is_worth_more_than_a_grunts() {
    assert!(EnemyKind::Brute.xp() > EnemyKind::Grunt.xp());
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let entity = harness
        .game
        .stage_pickup(DVec3::new(0.2, 0.0, 0.0), PickupKind::Xp(7));
    assert!(harness.game.pickup_positions().len() == 1, "{entity:?}");
    harness.run_ticks(harness.ticks + 2, &[]);
    assert_eq!(harness.game.xp(), 7, "the gem's own value was not banked");
}

/// **A gem is a trigger, so a bolt flies through it.**
///
/// The property the whole pickup design rests on: gems are in the same
/// broadphase the weapon sweeps, and a solid one would eat every shot fired
/// across a battlefield covered in loot.
#[test]
fn a_bolt_flies_through_a_gem_and_kills_what_is_behind_it() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let enemy = harness
        .game
        .stage_enemy(EnemyKind::Brute, DVec3::new(6.0, 0.0, 0.0));
    // Directly on the line of fire, and closer than the target.
    harness
        .game
        .stage_pickup(DVec3::new(3.0, 0.0, 0.0), PickupKind::Xp(1));
    let before = harness.game.enemy_hp(enemy).expect("a live brute");
    harness.run_ticks(harness.ticks + 60, &[]);
    let after = harness.game.enemy_hp(enemy).expect("still alive");
    assert!(
        after < before,
        "the gem in the way absorbed every bolt: {before} -> {after}",
    );
    assert_eq!(harness.game.pickup_count(), 1, "a bolt destroyed the gem");
}

/// **The gun does not aim at the loot.**
///
/// `overlap_sphere` does not skip triggers, so `fire`'s target query has to
/// reject gems itself — and a gun that locked onto one would stop shooting
/// the moment the field had anything to pick up.
#[test]
fn the_gun_ignores_gems_when_there_is_nothing_to_shoot() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness
        .game
        .stage_pickup(DVec3::new(4.0, 3.0, 0.0), PickupKind::Xp(1));
    harness.run_ticks(harness.ticks + 120, &[]);
    assert_eq!(
        harness.game.bolts_fired(),
        0,
        "the gun fired at a gem it cannot hurt",
    );
}

/// **A full field stops dropping gems** rather than growing a collider per
/// kill forever, and it says how many it refused.
#[test]
fn a_field_full_of_gems_drops_no_more() {
    let mut harness = Harness::staged(60, 60, DVec3::new(40.0, 30.0, 0.0));
    for i in 0..MAX_PICKUPS {
        harness.game.stage_pickup(
            DVec3::new(-40.0 + (i % 60) as f64 * 0.1, -30.0, 0.0),
            PickupKind::Xp(1),
        );
    }
    assert_eq!(harness.game.pickup_count(), MAX_PICKUPS);
    assert_eq!(harness.game.pickups_dropped(), 0);
    harness.game.stage_pickup(DVec3::ZERO, PickupKind::Xp(1));
    assert_eq!(
        harness.game.pickup_count(),
        MAX_PICKUPS,
        "the cap did not hold"
    );
    assert_eq!(
        harness.game.pickups_dropped(),
        1,
        "the refusal was not counted"
    );
}

// ---- potions -------------------------------------------------------------

/// **A potion is worth seconds of contact, not a run**, which is the whole
/// of what [`POTION_HEAL`] is tuned against.
///
/// Stated as two relations rather than as the number itself: a potion has to
/// buy more than a moment inside the kind the player actually meets, and it
/// must not be a reset. Both are read off `contact_dps` and
/// [`PLAYER_MAX_HP`], so re-tuning either moves this instead of leaving the
/// argument on [`POTION_DROP_CHANCE`] quietly wrong.
#[test]
fn a_potion_is_worth_seconds_of_contact_and_not_a_run() {
    // A `const` block, so this fails the *build* rather than a run: every
    // term is a constant or a `const fn`, and a relation between constants
    // that only breaks at test time is one a branch can be merged with.
    const {
        assert!(
            POTION_HEAL > EnemyKind::Grunt.contact_dps(),
            "a potion buys under a second inside the mass, which is a pickup \
             nobody would cross ground for",
        );
        assert!(
            POTION_HEAL < PLAYER_MAX_HP / 2.0,
            "a potion undoes half a run's damage, so contact stops mattering",
        );
        // …and the brute, which is what drops it, can take it back fastest.
        assert!(
            EnemyKind::Brute.contact_dps() > POTION_HEAL,
            "a brute cannot spend a potion in a second, so the kind that pays \
             for the heal is not the kind the heal is priced against",
        );
    }
}

/// **Walking over a potion heals the player, by exactly what it is worth.**
///
/// Three traps, all of them silent:
///
/// * a heal tested at full health passes whether healing works or not, so
///   the player is hurt first and the *amount* is asserted rather than "it
///   went up";
/// * a pickup test where the player never reaches the potion passes whether
///   collection works or not, so the closest approach is recorded and
///   checked against the radius the query actually uses;
/// * a potion collected as a gem would bank experience instead, which the
///   hit points alone would not notice — so the bank is asserted empty too.
#[test]
fn walking_over_a_potion_heals_the_player_by_what_it_is_worth() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let at = DVec3::new(2.0, 0.0, 0.0);
    harness.game.stage_pickup(at, PickupKind::Health);
    // Hurt by more than a potion is worth, so nothing here can be a clamp.
    let hurt = PLAYER_MAX_HP - POTION_HEAL - 10.0;
    harness.game.set_player_hp(hurt);

    let reach = harness.game.stats().pickup_radius + LOOT_RADIUS;
    harness.game.key_event(KeyCode::KeyD, true);
    let mut closest = f64::MAX;
    let deadline = harness.ticks + 120;
    while harness.game.pickup_count() > 0 && harness.ticks < deadline {
        harness.run_ticks(harness.ticks + 1, &[]);
        closest = closest.min((harness.game.player - at).length());
    }

    assert_eq!(harness.game.pickup_count(), 0, "the potion was never taken");
    assert!(
        closest <= reach,
        "the player never got within {reach} of the potion — closest was \
         {closest} — so nothing here is a test of collection",
    );
    assert!(
        (harness.game.player_hp - (hurt + POTION_HEAL)).abs() < 1e-9,
        "{hurt} hit points plus a potion came to {}",
        harness.game.player_hp,
    );
    assert_eq!(harness.game.xp(), 0, "the potion was banked as experience");
    harness.assert_nothing_leaked();
}

/// **A potion never heals past the ceiling the run has reached — and the
/// ceiling moves.**
///
/// [`Upgrade::Vitality`] raises [`Stats::max_hp`], so a heal clamped to
/// [`PLAYER_MAX_HP`] would silently stop paying out the moment a run took
/// it, and the bar would look right the whole time.
///
/// The overheal is arranged rather than hoped for: the player is left
/// missing less than a potion is worth, so a clamp that did nothing would
/// show up as hit points past the maximum. The assertion that the clamp
/// landed on the *right* maximum is the one against [`PLAYER_MAX_HP`] — the
/// run ends above it, which is impossible if the constant was the ceiling.
#[test]
fn a_potion_never_heals_past_the_ceiling_the_run_has_reached() {
    let mut harness = with_upgrade(Some(Upgrade::Vitality));
    let ceiling = harness.game.stats().max_hp;
    assert!(
        ceiling > PLAYER_MAX_HP,
        "the fixture did not raise the ceiling, so the clamp below is \
         against the constant either way",
    );

    let missing = POTION_HEAL / 2.0;
    harness.game.set_player_hp(ceiling - missing);
    harness
        .game
        .stage_pickup(harness.game.player, PickupKind::Health);
    harness.run_ticks(harness.ticks + 2, &[]);

    assert_eq!(harness.game.pickup_count(), 0, "the potion was never taken");
    assert!(
        (harness.game.player_hp - ceiling).abs() < 1e-9,
        "an overheal left {} against a ceiling of {ceiling}",
        harness.game.player_hp,
    );
    assert!(
        harness.game.player_hp > PLAYER_MAX_HP,
        "the heal clamped to the constant rather than to this run's own \
         maximum: {} against {PLAYER_MAX_HP}",
        harness.game.player_hp,
    );
}

/// **A brute's death can leave a potion, the other two kinds' never can, and
/// not every brute's does.**
///
/// All three halves, because each is vacuous alone: a rule that dropped
/// nothing satisfies "not every brute", a rule that dropped from everything
/// satisfies "brutes drop", and either would look like a working feature in
/// a screenshot of one kill.
///
/// **How many kills to run is taken from [`drops_potion`] rather than
/// guessed**, because at [`POTION_DROP_CHANCE`] a dozen dead brutes usually
/// leave nothing — a fixture sized by intuition is exactly the drop-rate
/// test that proves nothing. The run is played out to the first kill the
/// roll pays on, and what is asserted is that the *simulation* agreed:
/// `damage_enemy` consulted the same hand at the same index, which a drop
/// wired to a fresh RNG or to the wrong counter would not.
///
/// Killed by the gun, on a staged board, so the path measured is the one a
/// run takes. Each enemy is staged inside [`WEAPON_RANGE`] and far enough
/// out that it dies before it can touch the player, which the untouched hit
/// points at the end are what confirm.
#[test]
fn brutes_leave_potions_and_the_other_kinds_never_do() {
    let seed = run_seed(DEFAULT_SEED, 0);
    let paying = (0..256)
        .find(|kill| drops_potion(seed, *kill, EnemyKind::Brute))
        .expect("the hand pays out somewhere in its first two hundred kills");
    assert!(
        paying > 0,
        "the very first kill of the run pays out, so nothing below can show \
         that a brute may leave nothing",
    );
    let kills = paying + 1;

    let killed = |kind: EnemyKind| {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        for _ in 0..kills {
            harness.game.stage_enemy(kind, DVec3::new(10.0, 0.0, 0.0));
            harness.run_ticks(harness.ticks + 150, &[]);
        }
        assert_eq!(
            harness.game.kills, kills,
            "{kind:?} did not all die, so the kill indices below are not the \
             ones the roll was read at",
        );
        assert_eq!(
            harness.game.player_hp, PLAYER_MAX_HP,
            "{kind:?} reached the player",
        );
        harness.assert_nothing_leaked();
        harness.game.potions_dropped()
    };

    assert_eq!(
        killed(EnemyKind::Brute),
        1,
        "{kills} dead brutes left something other than the one potion the \
         roll says is in them",
    );
    for kind in [EnemyKind::Grunt, EnemyKind::Runner] {
        assert_eq!(killed(kind), 0, "a {kind:?} left a potion");
    }
}

/// **A brute leaves its potion beside its gem, not on top of it.**
///
/// Both drop from one death, so without the offset the gem would be painted
/// under the potion every time the rare drop happened. One pickup diameter
/// apart is the two discs touching — near enough to walk over together, far
/// enough that both are visible.
#[test]
fn a_brutes_potion_lands_beside_its_gem() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    // The first kill of this run that the roll actually pays out on, so the
    // fixture is not at the mercy of which index the seed happens to favour.
    let seed = harness.game.run_seed();
    let paying = (0..64)
        .find(|kill| drops_potion(seed, *kill, EnemyKind::Brute))
        .expect("some kill in the first sixty-four pays out");
    for _ in 0..=paying {
        harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(10.0, 0.0, 0.0));
        harness.run_ticks(harness.ticks + 200, &[]);
    }
    assert_eq!(harness.game.kills, paying + 1);
    assert_eq!(harness.game.potions_dropped(), {
        (0..=paying)
            .filter(|kill| drops_potion(seed, *kill, EnemyKind::Brute))
            .count() as u64
    });

    let ground = harness.game.pickups_on_the_ground();
    let potion = ground
        .iter()
        .rfind(|(_, kind)| *kind == PickupKind::Health)
        .expect("the paying kill left one");
    let gem = ground
        .iter()
        .filter(|(_, kind)| matches!(kind, PickupKind::Xp(_)))
        .min_by(|a, b| {
            (a.0 - potion.0)
                .length()
                .total_cmp(&(b.0 - potion.0).length())
        })
        .expect("every kill leaves one");
    let apart = (gem.0 - potion.0).length();
    assert!(
        apart > LOOT_RADIUS,
        "the potion is drawn over its own gem: {apart} apart",
    );
    assert!(
        apart <= 2.0 * LOOT_RADIUS + 1e-9,
        "the potion landed {apart} from its gem, which is further than the \
         two discs touching",
    );
}

/// **The drop roll is a pure function of the run**, which is what makes a
/// potion the same event in a replay, on a server and on a client.
///
/// The three properties that matter, and the third is the one a hand-rolled
/// hash of a position would fail: the answer depends on the run's seed, on
/// which kill it is, and on nothing else at all.
#[test]
fn the_potion_roll_is_a_pure_function_of_the_run() {
    let seed = run_seed(DEFAULT_SEED, 0);
    let sequence = |seed: u64| -> Vec<bool> {
        (0..512)
            .map(|kill| drops_potion(seed, kill, EnemyKind::Brute))
            .collect()
    };
    let first = sequence(seed);
    assert_eq!(first, sequence(seed), "the same run dealt two hands");

    // A restart is a different run, so it must deal a different hand — a
    // roll that ignored the seed would make every run's potions identical.
    let next = sequence(run_seed(DEFAULT_SEED, 1));
    assert_ne!(first, next, "a restart dealt the same potions");

    // …and the hand is neither empty nor everything, which both of the
    // comparisons above would be satisfied by.
    let dropped = first.iter().filter(|paid| **paid).count();
    assert!(
        dropped > 0 && dropped < first.len(),
        "{dropped} of {} rolls paid out",
        first.len(),
    );
}

/// **The measured drop rate, over a seeded run of the real game.**
///
/// [`POTION_DROP_CHANCE`] is an argument about how often a *player* sees a
/// potion, and that is a property of the drop rule and the spawn table
/// together — `EnemyKind::from_roll` decides what the player actually meets,
/// so a rate reasoned about from the constant alone is a rate nobody
/// measured. This kills a few hundred things under the autopilot and reads
/// the two counters off the run.
///
/// The bounds are one-sided on purpose and both are load-bearing: too
/// generous and contact damage stops being the pressure this genre is made
/// of, too rare and the drop is a feature nobody sees. They are wide enough
/// that a tuning change moves the logged figure without failing, and tight
/// enough that dropping from every kind, or from none, fails.
#[test]
fn potions_drop_from_brutes_at_the_rate_the_constant_says() {
    let mut harness = Harness::with_setup(
        60,
        &Setup {
            max_enemies: 120,
            ..Setup::default()
        },
    );
    harness.play_ticks(6_000);

    // One run, so the two counters cover the same kills: `potions_dropped`
    // is a count of the whole game and `Game::kills` is reset by `restart`.
    assert_eq!(
        harness.restarts, 0,
        "the run ended inside the window, so the ratio below is a count of \
         potions from every run against the kills of the last one",
    );
    let kills = harness.game.kills;
    let potions = harness.game.potions_dropped();
    assert!(
        kills > 150,
        "only {kills} kills, which is too few to measure a rate against",
    );
    assert!(potions > 0, "{kills} kills left no potion at all");
    assert!(
        potions * 20 < kills,
        "{potions} potions from {kills} kills is better than one kill in \
         twenty, and `from_roll` only deals a brute one in ten — so the drop \
         is no longer gated on the kind that earns it",
    );
    assert!(
        potions * 500 > kills,
        "{potions} potions from {kills} kills is rarer than one kill in five \
         hundred, which is a feature a run never meets",
    );
    crcbl::log::info!(
        "potions: {potions} from {kills} kills over {} ticks ({:.2}%), \
         {} still on the ground",
        harness.ticks,
        100.0 * potions as f64 / kills as f64,
        harness
            .game
            .pickups_on_the_ground()
            .iter()
            .filter(|(_, kind)| *kind == PickupKind::Health)
            .count(),
    );
}

/// **Banking the threshold opens the level-up screen**, takes the threshold
/// out of the bank and leaves the remainder.
#[test]
fn banking_the_threshold_opens_the_level_up_screen() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    assert_eq!(harness.game.level, 1);
    assert_eq!(harness.game.offer(), None);

    // One short of the threshold: nothing happens, which is what makes the
    // assertion below about the threshold and not about any XP at all.
    harness.game.bank_xp(xp_for_next_level(1) - 1);
    harness.run_ticks(harness.ticks + 2, &[]);
    assert_eq!(harness.game.state, GameState::Playing);
    assert_eq!(harness.game.level, 1);

    harness.game.bank_xp(3);
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp);
    assert_eq!(harness.game.level, 2);
    assert_eq!(
        harness.game.xp(),
        2,
        "the threshold was not taken out of the bank",
    );
    assert!(harness.game.offer().is_some(), "no offer was rolled");
}

/// **An offer is exactly three distinct upgrades from the pool**, at every
/// level of every run — and across enough of them the whole pool appears,
/// which is what says the shuffle is a shuffle and not a fixed prefix.
#[test]
fn an_offer_is_three_distinct_upgrades_and_the_pool_is_used() {
    let mut seen: Vec<Upgrade> = Vec::new();
    for seed in 0..64u64 {
        for level in 1..40u32 {
            let offer = upgrade_offer(seed, level);
            assert_eq!(offer.len(), UPGRADE_CHOICES);
            for (index, upgrade) in offer.iter().enumerate() {
                assert!(
                    Upgrade::ALL.contains(upgrade),
                    "{upgrade:?} is not in the pool",
                );
                assert!(
                    !offer[..index].contains(upgrade),
                    "seed {seed} level {level} offered {upgrade:?} twice: {offer:?}",
                );
                if !seen.contains(upgrade) {
                    seen.push(*upgrade);
                }
            }
        }
    }
    assert_eq!(
        seen.len(),
        Upgrade::ALL.len(),
        "only {seen:?} of the pool is ever offered",
    );
    // Deterministic: the same seed and level deal the same three.
    assert_eq!(upgrade_offer(7, 3), upgrade_offer(7, 3));
    assert_ne!(
        upgrade_offer(7, 3),
        upgrade_offer(8, 3),
        "the offer does not depend on the seed",
    );
}

/// The digit keys the level-up screen is driven by, in offer order.
const CHOICE_KEYS: [KeyCode; UPGRADE_CHOICES] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

/// A run seed whose **first** level-up offers `upgrade`, and where in the
/// offer it sits.
///
/// Searched rather than constructed, because `upgrade_offer` is a pure
/// function of the run seed and the level and there is no way to ask it for
/// a particular answer. The first level-up takes the run to level 2, so that
/// is the level to search.
fn seed_offering(upgrade: Upgrade) -> (u64, usize) {
    (0..4096u64)
        .find_map(|seed| {
            let offer = upgrade_offer(run_seed(seed, 0), 2);
            offer
                .iter()
                .position(|found| *found == upgrade)
                .map(|index| (seed, index))
        })
        .unwrap_or_else(|| panic!("no seed under 4096 offers {upgrade:?} at level 2"))
}

/// A staged run that has taken `upgrade` and nothing else, or — for `None` —
/// the same run with no upgrade at all.
///
/// The two are the same seed and the same board, so anything that differs
/// between them is the upgrade.
fn with_upgrade(upgrade: Option<Upgrade>) -> Harness {
    let (seed, index) = upgrade.map_or((DEFAULT_SEED, 0), seed_offering);
    let mut harness = Harness::with_setup(
        60,
        &Setup {
            seed,
            ..Setup::default()
        },
    );
    harness.game.freeze_spawns();
    harness.game.clear_enemies();
    harness.game.stage_player(DVec3::ZERO);
    let Some(upgrade) = upgrade else {
        return harness;
    };
    harness.game.bank_xp(xp_for_next_level(1));
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp, "no screen opened");
    assert_eq!(
        harness.game.offer().expect("an offer")[index],
        upgrade,
        "the search found the wrong seed",
    );
    harness.tap(CHOICE_KEYS[index]);
    assert_eq!(
        harness.game.state,
        GameState::Playing,
        "the screen stayed up"
    );
    harness
}

/// **Taking an upgrade changes what the simulation does** — every one of
/// them, measured as behaviour rather than as a field that moved.
///
/// Each arm names an *observable*: how far the player walked, how many bolts
/// left the gun, how much damage landed, whether a shot was taken at all,
/// how much of the bar came back, whether a gem out of reach was collected.
/// A test that read `Game::stats()` back would pass on an `apply_upgrade`
/// that wrote the number and on nothing that read it.
#[test]
fn every_upgrade_in_the_pool_changes_what_the_simulation_does() {
    for upgrade in Upgrade::ALL {
        let (mut base, mut up) = (with_upgrade(None), with_upgrade(Some(upgrade)));
        match upgrade {
            Upgrade::SwiftBoots => {
                let walk = |h: &mut Harness| {
                    let from = h.game.player;
                    h.run_ticks(h.ticks + 60, &[(h.ticks, KeyCode::KeyD, true)]);
                    h.game.player.x - from.x
                };
                let (slow, fast) = (walk(&mut base), walk(&mut up));
                assert!(fast > slow + 0.4, "walked {slow} then {fast}");
            }
            Upgrade::RapidFire => {
                let shots = |h: &mut Harness| {
                    // A dozen brutes, so the gun never runs out of targets:
                    // one dies to six bolts and the count would then be
                    // measuring how long a brute lasts.
                    for i in 0..12 {
                        h.game
                            .stage_enemy(EnemyKind::Brute, DVec3::new(5.0, -6.0 + i as f64, 0.0));
                    }
                    h.run_ticks(h.ticks + 180, &[]);
                    h.game.bolts_fired()
                };
                let (slow, fast) = (shots(&mut base), shots(&mut up));
                assert!(fast > slow, "fired {slow} then {fast}");
            }
            Upgrade::HeavyBolts => {
                let hurt = |h: &mut Harness| {
                    let brute = h
                        .game
                        .stage_enemy(EnemyKind::Brute, DVec3::new(5.0, 0.0, 0.0));
                    h.run_ticks(h.ticks + 40, &[]);
                    h.game.enemy_hp(brute).expect("a live brute")
                };
                let (light, heavy) = (hurt(&mut base), hurt(&mut up));
                assert!(heavy < light, "left {light} hp then {heavy}");
            }
            Upgrade::LongBarrel => {
                let fired = |h: &mut Harness| {
                    // Outside the base reach and inside the extended one, so
                    // this is a shot that could not otherwise be taken. A
                    // brute because it is the slowest thing in the game: the
                    // target walks in, and the window below has to be short
                    // enough that it has not walked into the *base* reach by
                    // the end of it — 1.65 units at 1.9 a second is 52 ticks.
                    h.game
                        .stage_enemy(EnemyKind::Brute, DVec3::new(WEAPON_RANGE + 2.5, 0.0, 0.0));
                    h.run_ticks(h.ticks + 20, &[]);
                    h.game.bolts_fired()
                };
                assert_eq!(fired(&mut base), 0, "the base reach already covers it");
                assert!(fired(&mut up) > 0, "the longer barrel did not reach");
            }
            Upgrade::Vitality => {
                // The heal is the observable a `max_hp += 25` alone would
                // not produce: the run took the upgrade at full health, so
                // the ceiling moved and the bar has to follow it.
                assert!(up.game.player_hp > base.game.player_hp);
                let survive = |h: &mut Harness| {
                    h.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
                    h.run_ticks(h.ticks + 120, &[]);
                    h.game.player_hp
                };
                let (weak, tough) = (survive(&mut base), survive(&mut up));
                assert!(tough > weak, "left {weak} hp then {tough}");
            }
            Upgrade::Magnet => {
                let collect = |h: &mut Harness| {
                    // Out of reach of a bare player — `PLAYER_RADIUS +
                    // LOOT_RADIUS` is 0.85 — and inside a magnet's.
                    h.game
                        .stage_pickup(DVec3::new(1.1, 0.0, 0.0), PickupKind::Xp(1));
                    h.run_ticks(h.ticks + 10, &[]);
                    h.game.xp()
                };
                assert_eq!(collect(&mut base), 0, "it was already in reach");
                assert_eq!(collect(&mut up), 1, "the magnet did not reach");
            }
        }
    }
}

/// **A restart takes every upgrade back off.**
///
/// The plan's non-goals bar meta-progression, and this is what makes that a
/// property of the code rather than of nobody having written the carry-over.
#[test]
fn a_restart_takes_every_upgrade_back_off() {
    let mut harness = with_upgrade(Some(Upgrade::HeavyBolts));
    assert_ne!(harness.game.stats(), Stats::default());
    assert!(harness.game.level > 1);
    harness.tap(KeyCode::KeyR);
    assert_eq!(harness.game.stats(), Stats::default());
    assert_eq!(harness.game.level, 1);
    assert_eq!(harness.game.xp(), 0);
    assert_eq!(harness.game.offer(), None);
}

/// **The field does not advance while the level-up screen is up.**
///
/// Every moving thing, and the run clock with them: the enemies stay where
/// they were, the bolts hang in the air, nothing spawns, nothing takes
/// damage. Asserted as bit equality rather than as a tolerance, because the
/// mechanism is a zeroed velocity and `position += 0 * dt` is exact.
#[test]
fn the_field_does_not_advance_while_the_level_up_screen_is_up() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    for i in 0..8 {
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(6.0 + i as f64, 2.0, 0.0));
    }
    // Run until there is actually a bolt in flight rather than for a
    // number of ticks that happens to leave one: a bolt lives 0.6 s and the
    // gun fires every 0.25, so "some ticks later" lands on an empty sky
    // about as often as not.
    for _ in 0..120 {
        harness.run_ticks(harness.ticks + 1, &[]);
        if harness.game.bolt_count() > 0 {
            break;
        }
    }
    assert!(harness.game.bolt_count() > 0, "no bolt to freeze");

    harness.game.bank_xp(xp_for_next_level(1));
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp);
    // One more tick, so the zeroed velocities have been through the
    // integrator once and the field is at rest rather than mid-step.
    harness.run_ticks(harness.ticks + 1, &[]);

    let enemies = harness.game.enemy_positions();
    let bolts = harness.game.bolt_positions();
    let (elapsed, kills, hp) = (
        harness.game.elapsed,
        harness.game.kills,
        harness.game.player_hp,
    );
    let (live, shots) = (harness.game.enemy_count(), harness.game.bolt_count());

    // Held movement keys too: a frozen field that the *player* could still
    // walk across would be half a freeze.
    harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);

    assert_eq!(harness.game.state, GameState::LevelUp, "it closed itself");
    assert_eq!(harness.game.enemy_positions(), enemies, "the crowd moved");
    assert_eq!(harness.game.bolt_positions(), bolts, "the bolts moved");
    assert_eq!(harness.game.player, DVec3::ZERO, "the player walked away");
    assert_eq!(harness.game.elapsed, elapsed, "the run clock advanced");
    assert_eq!(harness.game.kills, kills);
    assert_eq!(harness.game.player_hp, hp, "damage was dealt");
    assert_eq!(harness.game.enemy_count(), live, "the spawner ran");
    assert_eq!(harness.game.bolt_count(), shots, "a bolt expired");
    harness.assert_nothing_leaked();
}

/// **And it starts again when the screen closes** — the freeze is a pause,
/// not a stop. The bolts are the half that could not recover on their own:
/// an enemy is given a fresh velocity every tick and a bolt is not.
#[test]
fn taking_an_upgrade_puts_the_field_back_in_motion() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    for i in 0..4 {
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(6.0 + i as f64, 2.0, 0.0));
    }
    for _ in 0..120 {
        harness.run_ticks(harness.ticks + 1, &[]);
        if harness.game.bolt_count() > 0 {
            break;
        }
    }
    harness.game.bank_xp(xp_for_next_level(1));
    harness.run_ticks(harness.ticks + 2, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp);
    assert!(harness.game.bolt_count() > 0, "no bolt to thaw");

    let frozen_enemies = harness.game.enemy_positions();
    let frozen_bolts = harness.game.bolt_positions();
    harness.tap(KeyCode::Digit1);
    assert_eq!(harness.game.state, GameState::Playing);
    harness.run_ticks(harness.ticks + 4, &[]);

    assert_ne!(
        harness.game.enemy_positions(),
        frozen_enemies,
        "the crowd never started moving again",
    );
    assert_ne!(
        harness.game.bolt_positions(),
        frozen_bolts,
        "the bolts never got their velocity back",
    );
    assert!(harness.game.elapsed > 0.0);
}

/// A choice that names no button is ignored, and the screen stays up.
///
/// **No frame on the wire can ask for one**, which is the first half and is
/// asserted below over every byte a peer could send: the choice's field is
/// exactly as wide as the offer, so `Intent::from_wire` either refuses the
/// byte or hands back a button that exists. The guard in `apply_choice` is
/// the second half and is the one called directly here — it is what a caller
/// inside this file could still get wrong, and a level-up screen closed on
/// an upgrade nobody took has no way back.
#[test]
fn a_choice_outside_the_offer_takes_nothing() {
    for byte in 0..=u8::MAX {
        if let Some(intent) = Intent::from_wire(&[byte]) {
            assert!(
                intent.choose <= UPGRADE_CHOICES as u8,
                "byte {byte:#04x} decoded to choice {}",
                intent.choose,
            );
        }
    }

    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    harness.game.bank_xp(xp_for_next_level(1));
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp);
    let before = harness.game.stats();
    {
        let mut logic = lock(&harness.game.shared);
        let world = harness.game.session.server_mut().world_mut();
        apply_choice(&mut logic, world, UPGRADE_CHOICES);
    }
    harness.run_ticks(harness.ticks + 1, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp, "it closed anyway");
    assert_eq!(harness.game.stats(), before, "something was applied");
}

// -----------------------------------------------------------------------
// Audio
// -----------------------------------------------------------------------

/// **All six cues fire, and each one is heard where its event happened.**
///
/// Counted with [`crate::audio::Audio::plays`] rather than `voices()`: a
/// voice is reaped by the audio thread on a clock nothing here controls, and
/// this game's cap refuses a voice outright on a busy frame — so a test
/// written against the live voice count would be a race *and* would report a
/// cue that happened as one that did not. That is flappy's trap, and it is
/// worse here.
#[test]
fn every_cue_fires_and_carries_the_position_of_what_raised_it() {
    use crate::audio::{
        SOUND_DEATH, SOUND_HEAL, SOUND_KILL, SOUND_LEVEL, SOUND_PICKUP, SOUND_SHOT,
    };

    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let plays = |h: &Harness, id| h.game.audio.plays(id);
    for id in [
        SOUND_SHOT,
        SOUND_KILL,
        SOUND_PICKUP,
        SOUND_HEAL,
        SOUND_LEVEL,
        SOUND_DEATH,
    ] {
        assert_eq!(plays(&harness, id), 0, "cue {id} fired before anything did");
    }

    // A grunt in range: the gun fires at it, and it dies.
    let at = DVec3::new(4.0, 0.0, 0.0);
    harness.game.stage_enemy(EnemyKind::Grunt, at);
    harness.run_ticks(harness.ticks + 90, &[]);
    assert!(plays(&harness, SOUND_SHOT) > 0, "the gun was silent");
    // Read here, before the fixture walks the player east: the muzzle
    // asserted at the bottom is the one this facing had when the gun fired.
    let facing = lock(&harness.game.shared).player_facing;
    assert_eq!(plays(&harness, SOUND_KILL), 1, "the kill was silent");
    assert_eq!(plays(&harness, SOUND_PICKUP), 0, "the gem banked itself");

    // …and the gem it left, walked onto.
    harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);
    assert_eq!(plays(&harness, SOUND_PICKUP), 1, "the gem was silent");
    assert_eq!(plays(&harness, SOUND_HEAL), 0, "a gem played the heal");

    // …and a potion further along the same walk, which is the sixth cue and
    // the whole reason it is a sixth rather than a second use of the gem's.
    harness
        .game
        .stage_pickup(harness.game.player + DVec3::X * 2.0, PickupKind::Health);
    harness.run_ticks(harness.ticks + 60, &[]);
    assert_eq!(plays(&harness, SOUND_HEAL), 1, "the potion was silent");
    assert_eq!(
        plays(&harness, SOUND_PICKUP),
        1,
        "the potion played the gem's cue as well",
    );

    // A level, which is the one cue that is about the run rather than about
    // a place.
    harness.game.key_event(KeyCode::KeyD, false);
    harness.game.bank_xp(xp_for_next_level(harness.game.level));
    harness.run_ticks(harness.ticks + 2, &[]);
    assert_eq!(harness.game.state, GameState::LevelUp);
    assert_eq!(plays(&harness, SOUND_LEVEL), 1, "the level was silent");

    // And the end of the run. The player walked east to reach the gem, so
    // the brute goes where the player *is* — a fixture that staged it at the
    // origin would touch nothing and this would report a silent death that
    // never happened.
    harness.tap(KeyCode::Digit1);
    harness.game.set_player_hp(0.000_1);
    let player = harness.game.player;
    harness.game.stage_enemy(EnemyKind::Brute, player);
    harness.run_ticks(harness.ticks + 8, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert_eq!(plays(&harness, SOUND_DEATH), 1, "the death was silent");

    // **Where**, not just whether. Every cue carries a world position, and
    // a `play_at` handed a constant would satisfy every count above.
    let played = harness.game.audio.played().to_vec();
    let position_of = |want: u32| {
        played
            .iter()
            .find(|(id, _, _)| *id == want)
            .map(|(_, x, y)| DVec3::new(*x, *y, 0.0))
            .unwrap_or_else(|| panic!("cue {want} was counted, so it was played"))
    };

    // The shot leaves the **head of the staff**, not the player's centre and
    // not a point along the aim: the wizard has never pressed a horizontal
    // key in this fixture, so it is still facing the way it was drawn, and
    // the first bolt starts at that facing's muzzle whatever direction the
    // grunt is in.
    assert_eq!(
        facing,
        Facing::Right,
        "nothing in this fixture turns the wizard before it fires",
    );
    let shot = position_of(SOUND_SHOT);
    assert!(
        (shot - staff_muzzle(facing)).length() < 1e-9,
        "the shot was heard at {shot:?}, not at the staff head",
    );
    let killed_at = position_of(SOUND_KILL);
    assert!(
        (killed_at - at).length() < 2.0,
        "the kill was heard at {killed_at:?}, not near the grunt at {at:?}",
    );
    // The level is heard on the player, who by now has walked east to the
    // gem — so it is nowhere near either of the two above.
    let level = position_of(SOUND_LEVEL);
    assert!(
        level.x > 3.0 && (level - killed_at).length() > 1.0,
        "the level was heard at {level:?}, not on the player",
    );
    let distinct: std::collections::HashSet<(u64, u64)> = played
        .iter()
        .map(|(_, x, y)| (x.to_bits(), y.to_bits()))
        .collect();
    assert!(
        distinct.len() >= 3,
        "every cue was heard in the same place: {distinct:?}",
    );
}

/// The cue queue is drained every tick, so it cannot grow without bound and
/// a tick's cues never leak into the next one's.
///
/// The failure this guards is the one a queue filled inside the tick and
/// read outside it invites: a drain that missed a path — a frame that ran
/// two ticks, a level-up's early return — leaves cues sitting in simulation
/// state, which at this game's kill rate is an unbounded `Vec` on the hot
/// path.
#[test]
fn the_cue_queue_never_survives_the_tick_that_filled_it() {
    let mut harness = Harness::new(60, 60);
    harness.play_ticks(1_200);
    assert!(harness.game.kills > 0, "the soak killed nothing");
    assert_eq!(
        lock(&harness.game.shared).cues.len(),
        0,
        "cues were left in the simulation",
    );
    // A frame that runs several ticks at once must drain all of them, not
    // the last one's: a frame clock at 10 Hz over a 60 Hz tick runs six.
    let mut slow = Harness::new(10, 60);
    slow.play_ticks(600);
    assert_eq!(lock(&slow.game.shared).cues.len(), 0);
    assert!(
        slow.game.audio.plays(crate::audio::SOUND_SHOT) > 0,
        "a six-tick frame played none of its cues",
    );
}

// -----------------------------------------------------------------------
// The record
// -----------------------------------------------------------------------

/// **Both edges bank a record**: dying, and pressing restart on a live run.
///
/// The second is this game's own and the one a copy of asteroids' would
/// miss — asteroids' game is over when the ship runs out and the score is
/// frozen, while a horde run can be abandoned at any moment and is still
/// worth what it lasted.
///
/// The file itself is asserted in `crate::best`'s own suite; this is the
/// wiring.
#[test]
fn the_longest_run_is_banked_by_a_death_and_by_a_restart() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    assert_eq!(harness.game.best.get(), 0);

    // Three seconds of survival, then a restart. The record is the run that
    // ended, not the one that started.
    //
    // `elapsed as u32` rather than a literal 3: `elapsed` is 180 additions
    // of 1/60 and lands a few ulps under three, which the record truncates
    // to 2 — and the HUD's clock truncates the same way, so the two agree
    // and a literal here would be asserting arithmetic rather than
    // behaviour.
    harness.run_ticks(harness.ticks + 180, &[]);
    let abandoned = harness.game.elapsed;
    assert!(abandoned > 2.9, "the run was only {abandoned}s long");
    harness.restart_run();
    // One tick's worth, not zero: the tick that leaves the title screen
    // counts itself.
    assert!(
        harness.game.elapsed < 0.02,
        "the restart did not reset the clock: {}",
        harness.game.elapsed,
    );
    assert_eq!(
        harness.game.best.get(),
        abandoned as u32,
        "the abandoned run was worth nothing",
    );

    // A shorter run that ends in a death does not beat it…
    harness.game.freeze_spawns();
    harness.game.clear_enemies();
    harness.run_ticks(harness.ticks + 60, &[]);
    harness.game.set_player_hp(0.000_1);
    harness
        .game
        .stage_enemy(EnemyKind::Brute, harness.game.player);
    harness.run_ticks(harness.ticks + 8, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert!(
        harness.game.elapsed < abandoned,
        "the short run was not short"
    );
    assert_eq!(
        harness.game.best.get(),
        abandoned as u32,
        "a shorter run took the record",
    );

    // …and a longer one does.
    harness.restart_run();
    harness.game.freeze_spawns();
    harness.game.clear_enemies();
    harness.run_ticks(harness.ticks + 300, &[]);
    let survived = harness.game.elapsed;
    harness.game.set_player_hp(0.000_1);
    harness
        .game
        .stage_enemy(EnemyKind::Brute, harness.game.player);
    harness.run_ticks(harness.ticks + 8, &[]);
    assert_eq!(harness.game.state, GameState::Dead);
    assert!(survived > abandoned, "the long run was not longer");
    assert_eq!(
        harness.game.best.get(),
        harness.game.elapsed as u32,
        "the longest run did not take the record",
    );
}

/// The record reaches the renderer, and it is the **facade's** number: a
/// replay of the same script must not depend on what an earlier session
/// survived, so it is read outside the simulation's lock.
#[test]
fn the_record_reaches_the_render_state_without_entering_the_simulation() {
    let mut harness = Harness::staged(60, 60, DVec3::ZERO);
    let mut render = RenderState::default();
    harness.game.render_state(&mut render);
    assert_eq!(render.best, 0);

    harness.game.best.update(212.0);
    harness.game.render_state(&mut render);
    assert_eq!(render.best, 212);

    // Nothing the simulation hashes changed: the same seed and script still
    // produce the same field.
    let mut fresh = Harness::staged(60, 60, DVec3::ZERO);
    fresh.run_ticks(120, &[]);
    harness.run_ticks(120, &[]);
    assert_eq!(harness.game.enemy_positions(), fresh.game.enemy_positions());
}

// -----------------------------------------------------------------------
// The scale fixture
// -----------------------------------------------------------------------

/// **A prefilled field is the size it was asked for, inside the arena, and
/// made of the game's own mix of kinds.**
///
/// The fixture every number in `docs/plan/sample/03-horde.md` is taken
/// through, so a fixture that quietly staged a tenth of what it was asked
/// for — or piled the whole field onto one wall, which is what a 1.25-unit
/// grid does at ten thousand — would make every one of those numbers a
/// measurement of something else.
#[test]
fn a_prefilled_field_is_the_size_and_shape_it_was_asked_for() {
    let mut game = Game::with_setup(&Setup {
        max_enemies: 10_000,
        ..Setup::default()
    })
    .expect("a headless game always starts");
    assert_eq!(game.stage_field(10_000), 10_000);
    assert_eq!(game.enemy_count(), 10_000);

    let positions = game.enemy_positions();
    for position in &positions {
        assert!(
            position.x.abs() <= ARENA_HALF_WIDTH && position.y.abs() <= ARENA_HALF_HEIGHT,
            "{position:?} is outside the arena",
        );
    }
    // Spread, not stacked: a fixture that put them all in one place would
    // pass every count above and measure a crowd that does not exist.
    let distinct: std::collections::HashSet<(i64, i64)> = positions
        .iter()
        .map(|p| ((p.x * 100.0) as i64, (p.y * 100.0) as i64))
        .collect();
    assert_eq!(
        distinct.len(),
        10_000,
        "the grid put two enemies in one spot"
    );

    // …and the view holds a real crowd rather than the whole field, which is
    // the number the render measurement turns on.
    let mut render = RenderState::default();
    game.render_state(&mut render);
    assert_eq!(render.enemies.len(), 10_000);

    // Every kind, from the spawner's own table.
    let mut kinds: Vec<EnemyKind> = render.enemies.iter().map(|e| e.kind).collect();
    kinds.sort_unstable_by_key(|k| format!("{k:?}"));
    kinds.dedup();
    assert_eq!(kinds.len(), 3, "the prefill deals one kind: {kinds:?}");

    // The cap is honoured rather than ignored.
    assert_eq!(game.stage_field(500), 0, "the prefill went past the cap");
}
