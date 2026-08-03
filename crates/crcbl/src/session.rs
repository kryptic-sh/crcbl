//! The single-player session: a server and a client joined by an in-memory
//! transport.
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
use crcbl_net::{InMemoryTransport, ProtocolCompatibility};

/// A server and its client, wired to each other and to nothing else.
///
/// Both halves are reachable — a game reads render state off the client and
/// drives simulation through the server — but they are constructed together,
/// because the three things they have to agree on are exactly what
/// [`Loopback::new`] takes.
#[derive(Debug)]
pub struct Loopback {
    server: crcbl_server::Server<InMemoryTransport>,
    client: crcbl_client::Client<InMemoryTransport>,
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

impl Loopback {
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
    pub fn server(&self) -> &crcbl_server::Server<InMemoryTransport> {
        &self.server
    }

    /// The authoritative half, to drive.
    pub fn server_mut(&mut self) -> &mut crcbl_server::Server<InMemoryTransport> {
        &mut self.server
    }

    /// The predicting half.
    #[must_use]
    pub fn client(&self) -> &crcbl_client::Client<InMemoryTransport> {
        &self.client
    }

    /// The predicting half, to drive.
    pub fn client_mut(&mut self) -> &mut crcbl_client::Client<InMemoryTransport> {
        &mut self.client
    }

    /// Both halves at once, for the step that advances them together.
    ///
    /// A game's tick reads input into the client and time into the server in
    /// the same statement, and two separate `&mut` borrows of `self` will not
    /// let it.
    pub fn both_mut(
        &mut self,
    ) -> (
        &mut crcbl_server::Server<InMemoryTransport>,
        &mut crcbl_client::Client<InMemoryTransport>,
    ) {
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
