//! Co-op over a LAN: `--host [PORT]`, `--serve [PORT]`, `--join <IP:PORT>`
//! and `--browse`, through the engine's [`crcbl::lan`].
//!
//! ```text
//!  host:   Stage ◀─ TowersModule ◀─ Host ◀─┬─ in-memory ─ this player
//!            └─▶ FieldReplica ─▶ snapshots ├─ UDP ─────── a joiner
//!                                          └─ UDP ─────── another
//!  joiner: snapshots ─▶ Client::replicated ─▶ replica::decode ─▶ the frame
//! ```
//!
//! # The host is a player too
//!
//! A host runs the one authoritative [`Stage`](crate::game) on a
//! [`crcbl::server::Host`] behind a UDP listener, and plays in it the way
//! everyone else does: its own client joins over an in-memory pair, which is
//! the listen-host shape that type's docs describe, so the host's commands
//! are sealed, carried and validated exactly as a joiner's are. Every
//! player's command frame reaches `crate::game`'s `run_team_tick` in the
//! order the host admitted them, against **one** purse and **one** pool of
//! lives — the co-op the sample's plan asks for. A restart from any player
//! restarts the run for all of them.
//!
//! # Or nobody plays on the server
//!
//! `--serve` is the same host with no player of its own, no window and no
//! renderer: a dedicated server every player joins, ticking on the wall
//! clock. The `serve` module has it, and the rule for a session nobody is
//! in.
//!
//! # A joiner has no stage
//!
//! A joiner is a client and nothing else. What it draws is what the host's
//! snapshots carry — [`crate::replica`]'s entities, the frame's own view of
//! the host's stage — so its [`crate::game::RenderState`] and
//! [`crate::game::Stats`] are the host's, a round trip late. Its commands go
//! out a tick at a time, exactly as solo's.
//!
//! **It draws its own map.** The plots and the path are this process's
//! `--scene` (or the committed field), and the host's map never crosses the
//! wire. What does is its fingerprint, folded into the handshake's
//! compatibility by [`crate::game`]'s `compatibility`: a browser passes over a
//! host on another map, and the handshake refuses a direct join to one as a
//! schema mismatch, so a joiner never draws the host's towers on its own
//! plots. Sending the host's map at join, so any joiner can play any host, is
//! recorded in `docs/backlog.md`.
//!
//! # Wall time, every frame
//!
//! As `apps/sandbox`'s session does, a host serves its peers on the frame's
//! wall time through [`crate::game::Game::frame`], paused or not — so the
//! pause menu is this player's and not the session's, and a host with it open
//! goes on simulating for the others. Only the commands are on the tick.
//!
//! # Native only
//!
//! Web builds have no networking, by the LOCKED rule in
//! `docs/notes/simulation.md`: the browser build offers none of the four
//! flags and has no LAN link.

use std::time::Duration;

use crcbl::client::Client;
use crcbl::core::FrameClock;
use crcbl::ecs::World;
use crcbl::lan::{LanBind, LanClient, LanGame, LanHost};
use crcbl::net::InMemoryTransport;

use crate::game::{GameError, TowersModule, compatibility};
use crate::map::Map;
use crate::replica::{self, Decoded};

/// The most players a towers session holds, the host's own among them: the
/// "1–4 players co-op" of `docs/plan/sample/07-towers.md`.
pub const MAX_PLAYERS: u16 = 4;

/// What towers' printed lines start with.
pub const APP: &str = "towers";

/// The endpoint protocol id towers' links speak: it spells `TWRS`. A browser
/// or a listener on another lists and answers nothing.
pub const PROTOCOL_ID: u32 = u32::from_be_bytes(*b"TWRS");

/// Towers' LAN session on `map`, as [`crcbl::lan`] knows it.
///
/// The compatibility is the one solo's handshake gates on too, with the map
/// folded in — see [`crate::game`]'s `compatibility` — so a joiner of another
/// build or on another map is refused by the handshake and passed over by a
/// browser before it.
#[must_use]
pub fn session(map: &Map) -> LanGame {
    LanGame {
        app: APP,
        host_name: "crcbl towers",
        protocol_id: PROTOCOL_ID,
        compatibility: compatibility(map),
        max_players: MAX_PLAYERS,
    }
}

/// A host's side: the engine's LAN host, and this player's own client of it.
#[derive(Debug)]
pub(crate) struct HostLink {
    lan: LanHost,
    local: Client<InMemoryTransport>,
    /// Wall time since the session started, summed from each frame's
    /// `render_dt`: the host's clock.
    wall: Duration,
}

impl HostLink {
    /// Hosts `world` — the stage's replica — ticked by `module` as `game`,
    /// bound where `bind` says, and joins it as this player. Answers the link
    /// and the tick period, with the first tick spent on this player's
    /// handshake.
    ///
    /// # Errors
    ///
    /// [`GameError::Lan`] if the listener would not bind, and
    /// [`GameError::Server`] if this player's session did not come up in the
    /// first tick.
    pub fn open(
        game: LanGame,
        bind: LanBind,
        world: World,
        module: TowersModule,
        tick_hz: u32,
    ) -> Result<(Self, Duration), GameError> {
        let mut lan = LanHost::open(game, bind, world, tick_hz).map_err(GameError::Lan)?;
        lan.host_mut().set_module(Box::new(module));
        let (server_end, client_end) = InMemoryTransport::pair();
        lan.host_mut().add(Box::new(server_end));
        let mut local =
            Client::new_with_compatibility(World::new(), client_end, tick_hz, game.compatibility);
        // The client clock's own step, so one period is exactly one tick of it.
        let tick_period = FrameClock::new(tick_hz).tick_dt();

        // One tick on the handshake, as `Game::new` spends solo's: the hello
        // goes out, the host's tick admits it, and the accept comes back.
        local.update(tick_period);
        lan.frame(tick_period);
        local.update(tick_period);
        if local.session_id().is_none() {
            return Err(GameError::Server(
                "the host's own session did not come up in its first tick".into(),
            ));
        }
        Ok((
            Self {
                lan,
                local,
                wall: tick_period,
            },
            tick_period,
        ))
    }

    /// Sends this player's command, advancing its client's clock to
    /// `sim_time` — one tick.
    pub fn tick(&mut self, input: Vec<u8>, sim_time: Duration) {
        self.local.set_input(input);
        self.local.update(sim_time);
    }

    /// Runs the host to the wall time `render_dt` later, then reads what it
    /// sent this player — without moving the client's clock from
    /// `sim_time`, so no command goes out twice.
    pub fn frame(&mut self, render_dt: Duration, sim_time: Duration) {
        self.wall += render_dt;
        self.lan.frame(self.wall);
        self.local.update(sim_time);
    }

    /// The engine's LAN host: the F3 section and the session's numbers.
    pub const fn lan(&self) -> &LanHost {
        &self.lan
    }

    /// What this player's client has reconstructed of the field.
    pub fn replicated(&self) -> Decoded {
        replica::decode(self.local.replicated(replica::SYSTEM))
    }
}

/// A joiner's side: the engine's LAN client, browsing or in a session.
#[derive(Debug)]
pub(crate) struct RemoteLink {
    lan: LanClient,
}

impl RemoteLink {
    /// Plays through `lan`.
    pub const fn new(lan: LanClient) -> Self {
        Self { lan }
    }

    /// Sends this player's command, once there is a session to send it on,
    /// advancing the client's clock to `sim_time` — one tick.
    pub fn tick(&mut self, input: Vec<u8>, sim_time: Duration) {
        if let Some(client) = self.lan.client_mut() {
            client.set_input(input);
        }
        self.lan.frame(sim_time);
    }

    /// Browses, or reads what the host sent, without moving the client's
    /// clock from `sim_time`.
    pub fn frame(&mut self, sim_time: Duration) {
        self.lan.frame(sim_time);
    }

    /// The engine's LAN client: the F3 section and the session.
    pub const fn lan(&self) -> &LanClient {
        &self.lan
    }

    /// What the client has reconstructed of the host's field: an empty one
    /// until a session's first snapshot applies.
    pub fn replicated(&self) -> Decoded {
        self.lan.client().map_or_else(Decoded::default, |client| {
            replica::decode(client.replicated(replica::SYSTEM))
        })
    }
}

pub(crate) mod serve;

#[cfg(test)]
mod tests;
