//! Co-op over a LAN: `--host [PORT]`, `--serve [PORT]`, `--join <IP:PORT>`
//! and `--browse`, through the engine's [`crcbl::lan`].
//!
//! ```text
//!  host:   Stage ◀─ TowersModule ◀─ Host ◀─┬─ in-memory ─ this player
//!            └─▶ FieldReplica ─▶ snapshots ├─ UDP ─────── a joiner
//!                                          └─ UDP ─────── another
//!  joiner: the host's map ─▶ Joining ─▶ Game on that map
//!          snapshots ─▶ Client::replicated ─▶ replica::decode ─▶ the frame
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
//! **It plays on the host's map, whatever its own `--scene` says.** The
//! moment a joiner is admitted the host sends it the map — [`Map::to_wire`],
//! sealed on the reliable channel with `crcbl_server::Host::send_event`, so
//! it arrives behind the handshake's accept and ahead of anything else sent
//! reliably — and the joiner builds no game until it has it: a [`Joining`]
//! holds the client, reads the map back with [`Map::from_wire`], which
//! trusts none of it, and only then builds the [`Game`] on it. A joiner the
//! host accepts again on its link is sent the map again (`welcome` has why).
//! So a joiner's
//! game is never on its own map, and never draws the host's towers on its own
//! plots. Any build of this protocol joins any host, on any map.
//!
//! **A join that goes wrong says so.** A host that refuses the handshake, a
//! link that ends, a map that does not decode, or no map within
//! [`JOIN_TIMEOUT`] of choosing the host ends the [`Joining`] with a
//! [`JoinFailure`] naming which — what the lobby shows, and what a joiner
//! started from the command line logs. **So does a session that ends after
//! the map came**: the host left or shut down, removed this player, or the
//! link died, and [`Game::session_end`] says which — a join picked in the
//! lobby goes back to it with that line (`crate::lobby`), and one from the
//! command line shows it on its panel.
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

use std::net::SocketAddr;
use std::time::Duration;

use crcbl::client::{Client, Ended};
use crcbl::core::FrameClock;
use crcbl::ecs::World;
use crcbl::lan::{LanBind, LanClient, LanGame, LanHost};
use crcbl::net::udp::{CONNECT_TIMEOUT, UdpTransport};
use crcbl::net::{InMemoryTransport, SessionEndReason};
use crcbl::server::{Host, PeerEvent, PeerId};

use crate::game::{COMPATIBILITY, Game, GameError, TowersModule};
use crate::map::{Map, MapWireError};
use crate::replica::{self, Decoded};

/// The most players a towers session holds, the host's own among them: the
/// "1–4 players co-op" of `docs/plan/sample/07-towers.md`.
pub const MAX_PLAYERS: u16 = 4;

/// What towers' printed lines start with.
pub const APP: &str = "towers";

/// The endpoint protocol id towers' links speak: it spells `TWRS`. A browser
/// or a listener on another lists and answers nothing.
pub const PROTOCOL_ID: u32 = u32::from_be_bytes(*b"TWRS");

/// Towers' LAN session, as [`crcbl::lan`] knows it.
///
/// The compatibility is the one solo's handshake gates on too, so a joiner of
/// another build is refused by the handshake and passed over by a browser
/// before it. The map is not in it: the host sends its own at join.
pub const SESSION: LanGame = LanGame {
    app: APP,
    host_name: "crcbl towers",
    protocol_id: PROTOCOL_ID,
    compatibility: COMPATIBILITY,
    max_players: MAX_PLAYERS,
};

/// The longest a join may take from choosing a host to holding its map, on
/// the join's own clock.
///
/// Past the transport's own [`CONNECT_TIMEOUT`], so an address nobody answers
/// is reported as the link says it ended wherever the link says so first,
/// with the rest left for the handshake and the map — which on a LAN take a
/// few milliseconds. What it catches is everything no link reports: a host
/// that accepted the join and whose map never came.
pub const JOIN_TIMEOUT: Duration = CONNECT_TIMEOUT.saturating_mul(2);

/// Sends `map` — a [`Map::to_wire`] — to every peer `events` says joined or
/// was accepted again, but `local`: the host's own player, whose game is on
/// the map already.
///
/// **Again on a re-accept**, because the map sent at join is lost then: a
/// joiner that heard no `Accept` within its handshake timeout says hello
/// again and drops the first `Accept` as an old one, and the key the map was
/// sealed under starts over with the second
/// ([`PeerEvent::Reaccepted`]). A joiner that read the first copy is past
/// [`Joining`] and ignores the second. **Not on a resume**: a
/// [`PeerEvent::Resumed`] peer is a client that reconnected on a new link
/// within its grace period, which [`LanClient`] never does — its link ends
/// and so does the join or the game — and a client that did would keep the
/// game it had, and the map in it.
///
/// A send that fails is logged: that joiner waits out [`JOIN_TIMEOUT`] and
/// says so on its side, which is where a player can act on it.
fn welcome(host: &mut Host, events: &[PeerEvent], map: &[u8], local: Option<PeerId>) {
    for event in events {
        if let PeerEvent::Joined(peer) | PeerEvent::Reaccepted(peer) = *event
            && Some(peer) != local
            && let Err(error) = host.send_event(peer, map.to_vec())
        {
            crcbl::log::warn!("lan: the map did not go to {peer:?}: {error}");
        }
    }
}

/// A host's side: the engine's LAN host, and this player's own client of it.
#[derive(Debug)]
pub(crate) struct HostLink {
    lan: LanHost,
    local: Client<InMemoryTransport>,
    /// This player's own session, which is sent no map.
    local_peer: Option<PeerId>,
    /// The map every joiner is sent, encoded once.
    map: Vec<u8>,
    /// Wall time since the session started, summed from each frame's
    /// `render_dt`: the host's clock.
    wall: Duration,
}

impl HostLink {
    /// Hosts `world` — the stage's replica on `map` — ticked by `module` as
    /// `game`, bound where `bind` says, and joins it as this player. Every
    /// other player is sent `map` as they join. Answers the link and the tick
    /// period, with the first tick spent on this player's handshake.
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
        map: &Map,
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
        let events = lan.frame(tick_period);
        local.update(tick_period);
        if local.session_id().is_none() {
            return Err(GameError::Server(
                "the host's own session did not come up in its first tick".into(),
            ));
        }
        // The only transport the host had was this player's, so the first
        // session it admitted is theirs.
        let local_peer = events.iter().find_map(|event| match *event {
            PeerEvent::Joined(peer) => Some(peer),
            _ => None,
        });
        let map = map.to_wire();
        welcome(lan.host_mut(), &events, &map, local_peer);
        Ok((
            Self {
                lan,
                local,
                local_peer,
                map,
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
        let events = self.lan.frame(self.wall);
        welcome(self.lan.host_mut(), &events, &self.map, self.local_peer);
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

    /// Reads what the host sent, without moving the client's clock from
    /// `sim_time`.
    ///
    /// The host sends one event, the map, and [`Joining`] took it before
    /// this game was built; anything after it means nothing to towers and is
    /// taken off the queue and logged rather than left to fill it.
    pub fn frame(&mut self, sim_time: Duration) {
        self.lan.frame(sim_time);
        if let Some(client) = self.lan.client_mut() {
            for event in client.events() {
                crcbl::log::warn!(
                    "lan: an event of {} bytes from the host after its map, ignored",
                    event.len()
                );
            }
        }
    }

    /// The engine's LAN client: the F3 section and the session.
    pub const fn lan(&self) -> &LanClient {
        &self.lan
    }

    /// How the session ended, in words, once it has: the host left, shut
    /// down or removed this player, or the link died.
    pub fn ended(&self) -> Option<String> {
        let client = self.lan.client()?;
        client.ended().map(|ended| how_it_ended(ended, client))
    }

    /// What the client has reconstructed of the host's field: an empty one
    /// until a session's first snapshot applies.
    pub fn replicated(&self) -> Decoded {
        self.lan.client().map_or_else(Decoded::default, |client| {
            replica::decode(client.replicated(replica::SYSTEM))
        })
    }
}

/// How a session `client` was in ended, in words: what the host said, or
/// what the link reported. What a failed join and an ended game both show.
fn how_it_ended(ended: Ended, client: &Client<UdpTransport>) -> String {
    match ended {
        Ended::ByServer(SessionEndReason::HOST_LEFT) => "the host left".to_string(),
        Ended::ByServer(SessionEndReason::SHUTTING_DOWN) => "the server shut down".to_string(),
        Ended::ByServer(SessionEndReason::KICKED) => "the host removed this player".to_string(),
        // A code this build does not know still ends the session.
        Ended::ByServer(reason) => format!("the host ended the session: {reason:?}"),
        Ended::Lost => match client.transport().end_reason() {
            Some(reason) => format!("the link ended: {reason:?}"),
            None => "the link ended".to_string(),
        },
    }
}

/// Why a join ended without a game.
#[derive(Debug)]
pub enum JoinFailure {
    /// The host refused the handshake, for good — another build.
    Refused {
        /// The host.
        host: SocketAddr,
        /// What its refusal said.
        reason: String,
    },
    /// The host ended the session, or the link did, before the map came.
    Ended {
        /// The host.
        host: SocketAddr,
        /// How.
        how: String,
    },
    /// The host sent a map this build refuses.
    BadMap {
        /// The host.
        host: SocketAddr,
        /// What is wrong with it.
        error: MapWireError,
    },
    /// Nothing answered within the join's timeout: no session came up.
    NoAnswer {
        /// The host.
        host: SocketAddr,
        /// How long it was given.
        waited: Duration,
    },
    /// The session came up and the map did not come within the join's
    /// timeout.
    NoMap {
        /// The host.
        host: SocketAddr,
        /// How long it was given.
        waited: Duration,
    },
}

impl std::fmt::Display for JoinFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { host, reason } => write!(f, "{host} refused the join: {reason}"),
            Self::Ended { host, how } => write!(f, "{host}: {how}"),
            Self::BadMap { host, error } => {
                write!(f, "{host} sent a map this build refuses: {error}")
            }
            Self::NoAnswer { host, waited } => {
                write!(f, "no answer from {host} in {:.1} s", waited.as_secs_f64())
            }
            Self::NoMap { host, waited } => {
                write!(f, "no map from {host} in {:.1} s", waited.as_secs_f64())
            }
        }
    }
}

impl std::error::Error for JoinFailure {}

/// A joiner waiting for the host's map: the client in the session, or on its
/// way into one, and no game yet — see the module docs.
#[derive(Debug)]
pub struct Joining {
    /// Boxed, as the game's link boxes it, so a join is small to move
    /// between the states it passes through.
    lan: Box<LanClient>,
    tick_hz: u32,
    /// Wall time since the join started, summed from each frame's
    /// `render_dt`: the client's clock, and what the timeout is measured on.
    now: Duration,
    /// When a host was chosen — at once for an address, when the browser
    /// finds one for a browse.
    chosen_at: Option<Duration>,
    /// How long a chosen host has to send its map: [`JOIN_TIMEOUT`], unless a
    /// test asked for less.
    timeout: Duration,
}

/// Where a [`Joining`] stands after a frame.
#[derive(Debug)]
pub enum Progress {
    /// Still waiting: browsing, handshaking, or in the session with no map.
    Waiting(Joining),
    /// The map came, and this is the game on it.
    Joined(Game),
    /// The join is over, and why.
    Failed(JoinFailure),
}

impl Joining {
    /// Waits, through `lan`, for a host's map, at `tick_hz`, giving a chosen
    /// host `timeout` to send it — [`JOIN_TIMEOUT`] outside the tests.
    #[must_use]
    pub fn new(lan: LanClient, tick_hz: u32, timeout: Duration) -> Self {
        Self {
            lan: Box::new(lan),
            tick_hz,
            now: Duration::ZERO,
            chosen_at: None,
            timeout,
        }
    }

    /// The engine's LAN client: where it is joining, and the F3 section.
    #[must_use]
    pub const fn lan(&self) -> &LanClient {
        &self.lan
    }

    /// Runs the join for a frame covering `render_dt`, paused or not, and
    /// says where it stands. The first event the host sends is its map: a
    /// map that decodes is the game, and one that does not ends the join.
    pub fn step(mut self, render_dt: Duration) -> Progress {
        self.now += render_dt;
        self.lan.frame(self.now);
        let (Some(host), Some(client)) = (self.lan.host(), self.lan.client_mut()) else {
            // Still browsing: no host is chosen, so none is late.
            return Progress::Waiting(self);
        };
        let chosen_at = *self.chosen_at.get_or_insert(self.now);
        if let Some(refusal) = client.handshake_refusal() {
            return Progress::Failed(JoinFailure::Refused {
                host,
                reason: refusal.msg.clone(),
            });
        }
        // The host sends one event, its map; anything behind it in the same
        // read goes with the drain, as `RemoteLink::frame` drops what comes
        // after.
        let map = client.events().next();
        if let Some(bytes) = map {
            return match Map::from_wire(&bytes) {
                Ok(map) => Progress::Joined(Game::joined(self.tick_hz, map, *self.lan, self.now)),
                Err(error) => {
                    crcbl::log::warn!("lan: {host} sent a map this build refuses: {error}");
                    Progress::Failed(JoinFailure::BadMap { host, error })
                }
            };
        }
        if let Some(ended) = client.ended() {
            let how = how_it_ended(ended, client);
            return Progress::Failed(JoinFailure::Ended { host, how });
        }
        if self.now.saturating_sub(chosen_at) >= self.timeout {
            let waited = self.timeout;
            let failure = if client.session_id().is_some() {
                JoinFailure::NoMap { host, waited }
            } else {
                JoinFailure::NoAnswer { host, waited }
            };
            crcbl::log::warn!("lan: {failure}");
            return Progress::Failed(failure);
        }
        Progress::Waiting(self)
    }
}

pub(crate) mod serve;

#[cfg(test)]
pub(crate) mod tests;
