//! The single-player session: a server and a client joined by an in-memory
//! transport — or, for a test that wants to see a game played over a link that
//! loses things, by that transport behind a [`ConditionSimulator`]. See
//! [`Loopback::impaired`].
//!
//! ```text
//!  world ──▶ Server ──┐                    ┌──▶ Client ──▶ render state
//!  module ──▶         └── InMemoryTransport ┘
//! ```
//!
//! # Why a game does not build this itself
//!
//! Because "single-player is a loopback server" is an *engine* decision, not a
//! game's. `docs/plan/01-foundations.md` splits the simulation into an
//! authoritative half and a predicting half so that a game written for one
//! player is already written for several, and so the determinism harness has a
//! server to hash. A game that agreed to that split then had to implement it:
//! pair a transport, build a `Server`, hand it its module, build a `Client` on
//! the other end with **the same** compatibility and **the same** tick rate,
//! and spend both clocks' first update at time zero. Four games did, and wrote
//! the same twenty lines with the same comment above them.
//!
//! Getting any of it subtly wrong is a bug that does not announce itself: a
//! client on a different tick rate drifts, and one built with
//! [`ProtocolCompatibility::DEFAULT`] hand-shakes with anything.
//!
//! # What stays the game's
//!
//! Its [`ProtocolCompatibility`] — in particular the `schema_hash`, which is
//! what stops one game's client talking to another's server — and its
//! [`GameModule`]. Both are arguments here; neither has a default, because a
//! default for either is the wrong answer quietly.

use std::time::Duration;

use crcbl_core::FrameClock;
use crcbl_ecs::{GameModule, World};
use crcbl_net::{
    Clock, ConditionSimulator, InMemoryTransport, ManualClock, ProtocolCompatibility,
    SimConditions, Transport,
};

/// A server and its client, wired to each other and to nothing else.
///
/// Both halves are reachable — a game reads render state off the client and
/// drives simulation through the server — but they are constructed together,
/// because the three things they have to agree on are exactly what
/// [`Loopback::new`] takes.
#[derive(Debug)]
pub struct Loopback<T: Transport = InMemoryTransport> {
    server: crcbl_server::Server<T>,
    client: crcbl_client::Client<T>,
    tick_period: Duration,
}

/// Why a [`Loopback`] could not be built.
///
/// One variant, and it is not this crate's: [`Server`](crcbl_server::Server)
/// reads operating-system entropy for its resume credential rather than
/// issuing a predictable one, and that read can fail.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The operating system would not supply entropy for the resume token.
    #[error("the session's resume token needs OS entropy: {0}")]
    Entropy(String),
}

impl Loopback<InMemoryTransport> {
    /// Builds both halves and spends their first update.
    ///
    /// `world` is the server's — already populated, because the module's
    /// entities have to exist before the first tick — and the client is given
    /// an empty one, which it fills from snapshots.
    ///
    /// **The first `update` on each clock is spent here, at time zero.** A
    /// [`FrameClock`] establishes its baseline on its first update and
    /// therefore runs no ticks for it; doing that at construction is what lets
    /// a game's own `tick` promise that every later call runs exactly one.
    /// Leaving it to the caller means the first frame of the game silently
    /// simulates nothing.
    ///
    /// # Errors
    ///
    /// [`SessionError::Entropy`] if the OS would not seed the server's resume
    /// token.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero, or if `compatibility` leaves either identifier
    /// zero — both come from [`Server`](crcbl_server::Server) and
    /// [`Client`](crcbl_client::Client), and both are programming errors
    /// rather than conditions.
    pub fn new(
        world: World,
        module: Box<dyn GameModule>,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Result<Self, SessionError> {
        let (server_transport, client_transport) = InMemoryTransport::pair();
        Self::join(
            world,
            module,
            tick_hz,
            compatibility,
            server_transport,
            client_transport,
        )
    }
}

/// What [`Loopback::impaired`] offsets the server end's seed by.
///
/// Any value that is not zero would do; this one spells `SRVR` so a seed read
/// out of a failing run is recognisable as the derived half of the pair.
const SERVER_SEED_SALT: u64 = 0x5352_5652;

impl Loopback<ConditionSimulator<InMemoryTransport>> {
    /// The same pair, with both directions behind a
    /// [`ConditionSimulator`] running `conditions`.
    ///
    /// What it is for: every sample plays over [`InMemoryTransport`], which
    /// drops nothing, delays nothing and reorders nothing, so a game's own
    /// behaviour under loss or latency is exercised by no test anywhere. This
    /// is the constructor that lets one be written — a scripted run over a
    /// seeded impairment pattern, reproducing exactly.
    ///
    /// **Both ends are wrapped, and they are given different seeds.** One seed
    /// for both starts the two directions on the same draw sequence, which is
    /// one impairment pattern sampled twice rather than two; `conditions.seed`
    /// is the client's and the server's is derived from it, so the caller's
    /// single seed still reproduces the whole run.
    ///
    /// # Errors
    ///
    /// [`SessionError::Entropy`], exactly as [`Loopback::new`].
    ///
    /// # Panics
    ///
    /// As [`Loopback::new`]: a zero `tick_hz`, or a `compatibility` leaving
    /// either identifier zero.
    pub fn impaired(
        world: World,
        module: Box<dyn GameModule>,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
        conditions: SimConditions,
    ) -> Result<Self, SessionError> {
        Self::impaired_on(
            world,
            module,
            tick_hz,
            compatibility,
            conditions,
            crcbl_net::SystemClock::new(),
        )
    }
}

impl Loopback<ConditionSimulator<InMemoryTransport, ManualClock>> {
    /// The same pair over a clock the caller drives, and the handle to drive it.
    ///
    /// **What this buys is a latency test that spends no wall time.** A
    /// `ConditionSimulator` on the wall clock schedules a delayed message at a
    /// real instant, so a run over one has to sleep past every delay and then
    /// passes or fails on how evenly the machine happened to space its ticks.
    /// Driven by hand it is exact: advance the clock by a tick period per tick,
    /// beside the simulated time the two halves are already updated with, and
    /// the run is a pure function of its seed at any latency.
    ///
    /// The returned [`ManualClock`] is a clone of the one **both** ends hold,
    /// so one [`ManualClock::advance`] moves the whole link. Handing it back
    /// rather than taking one is deliberate: two ends given two clocks is two
    /// timelines, and there is no way to make that mistake through this
    /// signature.
    ///
    /// # Errors
    ///
    /// [`SessionError::Entropy`], exactly as [`Loopback::new`].
    ///
    /// # Panics
    ///
    /// As [`Loopback::new`]: a zero `tick_hz`, or a `compatibility` leaving
    /// either identifier zero.
    pub fn impaired_on_a_manual_clock(
        world: World,
        module: Box<dyn GameModule>,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
        conditions: SimConditions,
    ) -> Result<(Self, ManualClock), SessionError> {
        let clock = ManualClock::new();
        let session = Self::impaired_on(
            world,
            module,
            tick_hz,
            compatibility,
            conditions,
            clock.clone(),
        )?;
        Ok((session, clock))
    }
}

impl<C: Clock + Clone> Loopback<ConditionSimulator<InMemoryTransport, C>> {
    /// Both directions behind a [`ConditionSimulator`] running `conditions`,
    /// scheduling against `clock`.
    ///
    /// What it is for: every sample plays over [`InMemoryTransport`], which
    /// drops nothing, delays nothing and reorders nothing, so a game's own
    /// behaviour under loss or latency is exercised by no test anywhere. This
    /// is the constructor that lets one be written — a scripted run over a
    /// seeded impairment pattern, reproducing exactly.
    ///
    /// **Both ends are wrapped, and they are given different seeds.** One seed
    /// for both starts the two directions on the same draw sequence, which is
    /// one impairment pattern sampled twice rather than two; `conditions.seed`
    /// is the client's and the server's is derived from it, so the caller's
    /// single seed still reproduces the whole run.
    ///
    /// **Both ends share one clock**, because a link whose two directions
    /// disagree about the time is not a link.
    ///
    /// # Errors
    ///
    /// [`SessionError::Entropy`], exactly as [`Loopback::new`].
    ///
    /// # Panics
    ///
    /// As [`Loopback::new`]: a zero `tick_hz`, or a `compatibility` leaving
    /// either identifier zero.
    pub fn impaired_on(
        world: World,
        module: Box<dyn GameModule>,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
        conditions: SimConditions,
        clock: C,
    ) -> Result<Self, SessionError> {
        let (server_transport, client_transport) = InMemoryTransport::pair();
        let server_conditions = SimConditions {
            seed: conditions.seed ^ SERVER_SEED_SALT,
            ..conditions.clone()
        };
        Self::join(
            world,
            module,
            tick_hz,
            compatibility,
            ConditionSimulator::with_clock(server_transport, server_conditions, clock.clone()),
            ConditionSimulator::with_clock(client_transport, conditions, clock),
        )
    }
}

impl<T: Transport> Loopback<T> {
    /// Builds a server and a client on the two ends of an already-paired
    /// transport, and spends each clock's first update.
    fn join(
        world: World,
        module: Box<dyn GameModule>,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
        server_transport: T,
        client_transport: T,
    ) -> Result<Self, SessionError> {
        let mut server = crcbl_server::Server::try_new_with_compatibility(
            world,
            server_transport,
            tick_hz,
            compatibility,
        )
        .map_err(|error| SessionError::Entropy(error.to_string()))?;
        server.set_module(module);

        let mut client = crcbl_client::Client::new_with_compatibility(
            World::new(),
            client_transport,
            tick_hz,
            compatibility,
        );

        let tick_period = FrameClock::new(tick_hz).tick_dt();

        server.update(Duration::ZERO);
        client.update(Duration::ZERO);

        Ok(Self {
            server,
            client,
            tick_period,
        })
    }

    /// How long one simulation tick covers.
    ///
    /// Derived from the same `tick_hz` both halves were built with, so a game
    /// advancing its own clock by this cannot drift from the pair.
    #[must_use]
    pub fn tick_period(&self) -> Duration {
        self.tick_period
    }

    /// The authoritative half.
    #[must_use]
    pub fn server(&self) -> &crcbl_server::Server<T> {
        &self.server
    }

    /// The authoritative half, to drive.
    pub fn server_mut(&mut self) -> &mut crcbl_server::Server<T> {
        &mut self.server
    }

    /// The predicting half.
    #[must_use]
    pub fn client(&self) -> &crcbl_client::Client<T> {
        &self.client
    }

    /// The predicting half, to drive.
    pub fn client_mut(&mut self) -> &mut crcbl_client::Client<T> {
        &mut self.client
    }

    /// Both halves at once, for the step that advances them together.
    ///
    /// A game's tick reads input into the client and time into the server in
    /// the same statement, and two separate `&mut` borrows of `self` will not
    /// let it.
    pub fn both_mut(&mut self) -> (&mut crcbl_server::Server<T>, &mut crcbl_client::Client<T>) {
        (&mut self.server, &mut self.client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-zero on both counts: `ProtocolCompatibility::DEFAULT` is the
    /// placeholder and both constructors reject it.
    const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
        protocol_version: 3,
        engine_build_id: 0x0043_5243_424C,
        schema_hash: 0x0000_5445_5354,
    };

    #[derive(Debug)]
    struct NoopModule;

    impl GameModule for NoopModule {
        fn name(&self) -> &str {
            "test"
        }

        /// Empty for the reason every sample's is: `set_module` does not call
        /// it, and each game populates its world before handing it over.
        fn register(&self, _world: &mut World) {}
    }

    /// The baseline update is the part a caller forgets, so it is the part
    /// asserted: a freshly built session has run no ticks and is ready to run
    /// exactly one for the next whole period.
    #[test]
    fn a_new_session_has_spent_its_baseline_and_runs_one_tick_for_one_period() {
        let mut session = Loopback::new(World::new(), Box::new(NoopModule), 60, COMPATIBILITY)
            .expect("OS entropy is available in a test process");
        assert_eq!(
            session.server().tick_id().get(),
            0,
            "a new session has run no ticks"
        );

        let period = session.tick_period();
        session.server_mut().update(period);
        assert_eq!(
            session.server().tick_id().get(),
            1,
            "one period after construction did not run exactly one tick, which \
             is what the baseline update at time zero exists to make true"
        );
    }

    /// `tick_period` is derived from the rate both halves were built with, so a
    /// different rate has to produce a different period — otherwise a game
    /// pacing itself by it would drift at every rate but 60.
    #[test]
    fn the_tick_period_follows_the_rate_the_session_was_built_at() {
        for hz in [30u32, 60, 120] {
            let session = Loopback::new(World::new(), Box::new(NoopModule), hz, COMPATIBILITY)
                .expect("OS entropy is available in a test process");
            assert_eq!(session.tick_period(), FrameClock::new(hz).tick_dt(), "{hz}");
        }
    }

    /// A simulator configured to impair nothing is the plain pair: the
    /// wrapper is in the path on both ends and costs the session nothing.
    #[test]
    fn an_impaired_session_with_no_impairment_still_comes_up_and_replicates() {
        let mut session = Loopback::impaired(
            World::new(),
            Box::new(NoopModule),
            60,
            COMPATIBILITY,
            SimConditions::default(),
        )
        .expect("OS entropy is available in a test process");

        let period = session.tick_period();
        let mut sim_time = Duration::ZERO;
        for _ in 0..8 {
            sim_time += period;
            let (server, client) = session.both_mut();
            client.update(sim_time);
            server.update(sim_time);
            client.update(sim_time);
        }

        assert_eq!(
            session.server().session_state(),
            crcbl_net::SessionState::Connected,
        );
        assert!(
            session.client().last_applied_tick().get() > 0,
            "the client applied nothing in eight ticks over a link that impairs \
             nothing",
        );
    }

    /// And the conditions are actually applied, which the test above cannot
    /// tell: a link that carries nothing never gets the two halves talking.
    #[test]
    fn a_link_that_drops_everything_never_establishes_the_session() {
        let mut session = Loopback::impaired(
            World::new(),
            Box::new(NoopModule),
            60,
            COMPATIBILITY,
            SimConditions {
                loss_rate: 1.0,
                ..Default::default()
            },
        )
        .expect("OS entropy is available in a test process");

        let period = session.tick_period();
        let mut sim_time = Duration::ZERO;
        for _ in 0..8 {
            sim_time += period;
            let (server, client) = session.both_mut();
            client.update(sim_time);
            server.update(sim_time);
            client.update(sim_time);
        }

        assert_eq!(
            session.server().session_state(),
            crcbl_net::SessionState::Disconnected,
            "the hello reached the server over a link that drops everything, so \
             the simulator is not in the path",
        );
        assert_eq!(session.client().last_applied_tick().get(), 0);
    }

    /// Both halves are on the same transport pair, which is the whole point:
    /// what the server publishes is what this client receives.
    #[test]
    fn the_client_receives_what_its_own_server_publishes() {
        let mut session = Loopback::new(World::new(), Box::new(NoopModule), 60, COMPATIBILITY)
            .expect("OS entropy is available in a test process");

        // **Cumulative, not per-tick.** Both clocks take the time elapsed since
        // the session began and drain their own accumulator; handing each the
        // period over and over runs one tick and then nothing, which is how
        // this test failed the first time it was written.
        let period = session.tick_period();
        let mut sim_time = Duration::ZERO;
        for expected in 1..=8u64 {
            sim_time += period;
            let (server, client) = session.both_mut();
            let ran = server.update(sim_time);
            client.update(sim_time);
            assert_eq!(ran, 1, "one period in is one tick out");
            assert_eq!(server.tick_id().get(), expected);
        }

        assert!(
            session.client().last_applied_tick().get() > 0,
            "the client applied nothing in eight ticks, so the two halves are \
             not talking to each other"
        );
    }
}
