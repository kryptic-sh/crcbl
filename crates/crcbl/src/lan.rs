//! LAN play over the engine's own UDP transport: a host that admits players
//! and announces itself, and a client that joins one by address or finds one
//! on the local network.
//!
//! Here, in the umbrella, because it is the one crate that names all four
//! halves of the join: `crcbl-net`'s [`udp`](crate::net::udp) links and
//! [`discovery`](crate::net::udp::discovery), `crcbl-server`'s [`Host`],
//! `crcbl-client`'s [`Client`] and `crcbl-ui`'s debug panel. Every sample
//! that plays over a LAN wires them the same way, and what differs — what the
//! host's world holds, what its players' input means — stays the sample's:
//! it hands [`LanHost::open`] its world, and reaches the [`Host`] and the
//! [`Client`] through [`LanHost::host_mut`] and [`LanClient::client_mut`].
//!
//! - **[`LanHost`]** binds a [`UdpListener`] and opens an [`Announcer`] beside
//!   it, advertising the listener's port. Every frame the listener's newly
//!   confirmed peers go to the [`Host`] as `Box<dyn Transport>`, the way the
//!   Steam path takes its listener's peers, and the announced player count
//!   follows the host's.
//! - **[`LanClient`]** connects a [`UdpTransport`] to an address and runs a
//!   [`Client`] over it — direct connect, first-class — or polls a
//!   [`Browser`], prints every host it heard, and joins the first one this
//!   build can play with. There is no list on screen to choose from yet; the
//!   backlog says what one would take.
//! - **[`LanMode`]** is what `--host [PORT]`, `--join <IP:PORT>` and
//!   `--browse` ask for, parsed by [`LanMode::consume`] so every sample reads
//!   the three flags alike.
//!
//! Both sides add a "lan" section to the F3 panel saying where they stand.
//!
//! # One datagram per snapshot
//!
//! A UDP snapshot travels on the unreliable channel, which takes one
//! datagram — [`MAX_UNRELIABLE_PAYLOAD`] bytes. A world whose snapshot grew
//! past it has each snapshot fitted to the datagram, the least urgent updates
//! held back for later ones (`crcbl::net::budget`), which the F3 panel
//! counts. Only an update that cannot fit a datagram on its own is refused,
//! which the host records by name ([`crate::server::SnapshotTooLarge`]) and
//! this module logs.
//!
//! # Native only
//!
//! Web builds have no networking, by the LOCKED rule in
//! `docs/notes/simulation.md`, and the transport is compiled out of them, so
//! this module is too.

use std::io;
use std::iter::Peekable;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::NonZeroU16;
use std::time::Duration;

use crate::args::Consumed;
use crate::client::{Client, Ended};
use crate::ecs::World;
use crate::net::ProtocolCompatibility;
use crate::net::reliable::MAX_UNRELIABLE_PAYLOAD;
use crate::net::udp::discovery::{Announcement, Announcer, Browser, DISCOVERY_PORT};
use crate::net::udp::{ConnectError, UdpListener, UdpTransport};
use crate::server::{Host, HostConfig, PeerEvent};
use crate::ui::{DebugModule, DebugSection};

/// The least time between two logged snapshot refusals: one a second says
/// the world is too big without a line every tick.
const REFUSAL_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// What a sample's LAN session is known by: on the wire, in the announce, and
/// in what it prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanGame {
    /// What the sample's printed lines start with — `sandbox`, `towers`.
    pub app: &'static str,
    /// The name a host announces.
    pub host_name: &'static str,
    /// The endpoint protocol id the sample's links speak. A listener or
    /// browser on another answers nothing and lists nothing.
    pub protocol_id: u32,
    /// What the session handshake gates on, and what a browser compares
    /// before it joins.
    pub compatibility: ProtocolCompatibility,
    /// The most players a host admits, and what it announces.
    pub max_players: u16,
}

/// What the command line asked of the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanMode {
    /// No networking: the sample as it always ran.
    #[default]
    Off,
    /// Host a session on this UDP port — 0 for any free one.
    Host {
        /// The port to listen on.
        port: u16,
    },
    /// Join the host at this address.
    Join(SocketAddr),
    /// Find a host on the LAN and join it.
    Browse,
}

impl LanMode {
    /// Claims `arg` if it is `--host [PORT]`, `--join <IP:PORT>` or
    /// `--browse`, taking its value from `rest`.
    ///
    /// `--host`'s port is optional, so `rest` is peekable: the next argument
    /// is taken only when it parses as a port, and anything else is left for
    /// the caller to read as an argument of its own. A second mode is
    /// [`Consumed::Bad`] — which of two sessions to start is not a thing to
    /// guess — and so is an address that is not an `IP:PORT`.
    pub fn consume<I: Iterator<Item = String>>(
        &mut self,
        arg: &str,
        rest: &mut Peekable<I>,
    ) -> Consumed {
        let mode = match arg {
            "--host" => {
                let port = match rest.peek().map(|value| value.parse::<u16>()) {
                    Some(Ok(port)) => {
                        rest.next();
                        port
                    }
                    Some(Err(_)) | None => 0,
                };
                Self::Host { port }
            }
            "--join" => match rest.next() {
                Some(value) => match value.parse() {
                    Ok(addr) => Self::Join(addr),
                    Err(_) => {
                        return Consumed::Bad(format!(
                            "--join needs an IP:PORT address, not `{value}`"
                        ));
                    }
                },
                None => return Consumed::Bad("--join needs a value".to_string()),
            },
            "--browse" => Self::Browse,
            _ => return Consumed::No,
        };
        if *self != Self::Off {
            return Consumed::Bad("--host, --join and --browse exclude each other".to_string());
        }
        *self = mode;
        Consumed::Yes
    }
}

/// Why LAN play could not start.
#[derive(Debug)]
pub enum LanError {
    /// The host's listener could not be bound.
    Listen(io::Error),
    /// The browser's socket could not be bound.
    Browse(io::Error),
    /// The connect to a host could not start.
    Connect(ConnectError),
}

impl std::fmt::Display for LanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Listen(error) => write!(f, "cannot host: {error}"),
            Self::Browse(error) => write!(f, "cannot look for hosts: {error}"),
            Self::Connect(error) => write!(f, "cannot connect: {error}"),
        }
    }
}

impl std::error::Error for LanError {}

/// Where a [`LanHost`]'s two sockets bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanBind {
    /// The game listener's address.
    pub listen: SocketAddr,
    /// The announcer's address.
    pub announce_at: SocketAddr,
    /// Where the announcer broadcasts, or `None` to answer queries only.
    pub broadcast_to: Option<SocketAddr>,
}

impl LanBind {
    /// Every interface: the listener on `port` (0 for any free one), the
    /// announcer on [`DISCOVERY_PORT`], broadcasting to the IPv4 broadcast
    /// address — what `--host` asks for.
    #[must_use]
    pub const fn on_the_lan(port: u16) -> Self {
        Self {
            listen: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)),
            announce_at: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)),
            broadcast_to: Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::BROADCAST,
                DISCOVERY_PORT,
            ))),
        }
    }
}

/// A host: the listener, the announcer beside it, and the session host its
/// peers are handed to.
pub struct LanHost {
    game: LanGame,
    listener: UdpListener,
    /// `None` when the discovery port could not be bound — another host on
    /// this machine holds it. The session is still joinable by address.
    announcer: Option<Announcer>,
    host: Host,
    /// Refusals already logged, and when the last line went out.
    refusals_logged: u64,
    last_refusal_log: Option<Duration>,
}

impl LanHost {
    /// A host of `game` on `world`, bound where `bind` says and ticking at
    /// `tick_hz` — a host and its clients must agree on it. Prints where it
    /// listens, since `--join` needs the port.
    ///
    /// # Errors
    ///
    /// [`LanError::Listen`] when the listener cannot be bound. An announcer
    /// that cannot be bound is logged and left out instead: discovery is a
    /// convenience over connecting by address.
    pub fn open(
        game: LanGame,
        bind: LanBind,
        world: World,
        tick_hz: u32,
    ) -> Result<Self, LanError> {
        let listener =
            UdpListener::bind(bind.listen, game.protocol_id).map_err(LanError::Listen)?;
        let port = listener.local_addr().map_err(LanError::Listen)?.port();
        let game_port = NonZeroU16::new(port)
            .ok_or_else(|| LanError::Listen(io::Error::other("the listener reports port 0")))?;
        let mut announcement = Announcement::new(
            game.protocol_id,
            game_port,
            game.compatibility,
            game.host_name,
        );
        announcement.max_players = game.max_players;
        let announcer = match Announcer::bind_with(
            bind.announce_at,
            announcement,
            bind.broadcast_to,
            crate::net::SystemClock::new(),
        ) {
            Ok(announcer) => Some(announcer),
            Err(error) => {
                crate::log::warn!(
                    "lan: not announcing on {}: {error}; join by address instead",
                    bind.announce_at
                );
                None
            }
        };
        let mut host = Host::new(
            world,
            HostConfig {
                max_peers: usize::from(game.max_players),
                tick_hz,
                compatibility: game.compatibility,
            },
        );
        // The host's clock takes its baseline here, as `Loopback` spends
        // its first update at zero.
        host.update(Duration::ZERO);
        println!(
            "{}: hosting on UDP port {port}{}",
            game.app,
            if announcer.is_some() {
                ", announced on the LAN"
            } else {
                " (not announced; --join by address)"
            },
        );
        Ok(Self {
            game,
            listener,
            announcer,
            host,
            refusals_logged: 0,
            last_refusal_log: None,
        })
    }

    /// The port the listener is bound to.
    pub fn game_port(&self) -> u16 {
        self.listener.local_addr().map_or(0, |addr| addr.port())
    }

    /// The session host.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The session host, for what the sample does with it: add its own
    /// player's transport, set its module, change its world.
    pub fn host_mut(&mut self) -> &mut Host {
        &mut self.host
    }

    /// What the host announces, while it announces.
    pub fn announcement(&self) -> Option<&Announcement> {
        self.announcer.as_ref().map(Announcer::announcement)
    }

    /// Where the announcer answers queries, while it announces.
    pub fn announcer_addr(&self) -> Option<SocketAddr> {
        self.announcer
            .as_ref()
            .and_then(|announcer| announcer.local_addr().ok())
    }

    /// Takes in newly confirmed peers, runs the host to `now`, keeps the
    /// announcement in step with who is in, and answers discovery queries.
    ///
    /// Returns the session changes the host raised, each already logged, for
    /// the sample to reflect in its world.
    pub fn frame(&mut self, now: Duration) -> Vec<PeerEvent> {
        while let Some(peer) = self.listener.accept() {
            crate::log::info!("lan: {} connected", peer.peer_addr());
            self.host.add(Box::new(peer));
        }
        self.host.update(now);
        let events: Vec<PeerEvent> = self.host.events().collect();
        for event in &events {
            crate::log::info!("lan: {event:?}");
        }
        self.report_refusals(now);
        if let Some(announcer) = &mut self.announcer {
            let players = u16::try_from(self.host.peer_count()).unwrap_or(u16::MAX);
            if announcer.announcement().players != players {
                let mut announcement = announcer.announcement().clone();
                announcement.players = players;
                announcer.set_announcement(announcement);
            }
            announcer.poll();
        }
        events
    }

    /// Logs a snapshot the transport refused as too long, naming the limit —
    /// at most once per [`REFUSAL_LOG_INTERVAL`].
    fn report_refusals(&mut self, now: Duration) {
        let refused = self.host.oversized_snapshot_count();
        if refused == self.refusals_logged {
            return;
        }
        if self
            .last_refusal_log
            .is_some_and(|last| now.saturating_sub(last) < REFUSAL_LOG_INTERVAL)
        {
            return;
        }
        if let Some(snapshot) = self.host.last_oversized_snapshot() {
            crate::log::warn!(
                "lan: {snapshot} ({refused} refused so far); one entity's update must fit a UDP \
                 datagram on its own"
            );
        }
        self.refusals_logged = refused;
        self.last_refusal_log = Some(now);
    }
}

impl std::fmt::Debug for LanHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanHost")
            .field("game", &self.game.app)
            .field("listener", &self.listener)
            .field("announcer", &self.announcer)
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

impl DebugModule for LanHost {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("lan");
        out.row_str("role", "host");
        out.row("port", format_args!("{}", self.game_port()));
        out.row(
            "announced",
            format_args!(
                "{}",
                if self.announcer.is_some() {
                    "yes"
                } else {
                    "no"
                }
            ),
        );
        out.row(
            "players",
            format_args!("{}/{}", self.host.peer_count(), self.game.max_players),
        );
        out.row(
            "snapshot",
            format_args!(
                "{} of {MAX_UNRELIABLE_PAYLOAD} bytes",
                self.host.largest_snapshot_bytes()
            ),
        );
        out.row(
            "held back",
            format_args!("{}", self.host.held_back_update_count()),
        );
        out.row(
            "refused",
            format_args!("{}", self.host.oversized_snapshot_count()),
        );
    }
}

/// A client: looking for a host, or in a session with one.
#[derive(Debug)]
pub struct LanClient {
    game: LanGame,
    phase: Phase,
    tick_hz: u32,
    /// Whether the session's start and end have been logged.
    reported_session: bool,
    reported_end: bool,
}

#[derive(Debug)]
enum Phase {
    Browsing(Browser),
    Joined {
        host: SocketAddr,
        client: Box<Client<UdpTransport>>,
    },
    /// A connect to the chosen host could not start; logged, and nothing
    /// more happens.
    Failed,
}

impl LanClient {
    /// Starts a connect to the host of `game` at `addr`, ticking at
    /// `tick_hz`.
    ///
    /// # Errors
    ///
    /// [`LanError::Connect`] when no socket could be bound or no secret
    /// drawn.
    pub fn join(game: LanGame, addr: SocketAddr, tick_hz: u32) -> Result<Self, LanError> {
        Ok(Self::in_phase(game, connect(game, addr, tick_hz)?, tick_hz))
    }

    /// Looks for hosts of `game` with `browser`, and joins the first this
    /// build can play with.
    pub fn browse(game: LanGame, browser: Browser, tick_hz: u32) -> Self {
        Self::in_phase(game, Phase::Browsing(browser), tick_hz)
    }

    /// Looks for hosts of `game` on the LAN — a [`Browser`] querying the
    /// broadcast address — and joins the first this build can play with.
    ///
    /// # Errors
    ///
    /// [`LanError::Browse`] when the browser's socket cannot be bound.
    pub fn browse_the_lan(game: LanGame, tick_hz: u32) -> Result<Self, LanError> {
        let browser = Browser::open(game.protocol_id).map_err(LanError::Browse)?;
        Ok(Self::browse(game, browser, tick_hz))
    }

    fn in_phase(game: LanGame, phase: Phase, tick_hz: u32) -> Self {
        Self {
            game,
            phase,
            tick_hz,
            reported_session: false,
            reported_end: false,
        }
    }

    /// The session client, once a host is chosen.
    pub fn client(&self) -> Option<&Client<UdpTransport>> {
        match &self.phase {
            Phase::Joined { client, .. } => Some(client),
            Phase::Browsing(_) | Phase::Failed => None,
        }
    }

    /// The session client, once a host is chosen, for what the sample sends
    /// over it.
    pub fn client_mut(&mut self) -> Option<&mut Client<UdpTransport>> {
        match &mut self.phase {
            Phase::Joined { client, .. } => Some(client),
            Phase::Browsing(_) | Phase::Failed => None,
        }
    }

    /// The host joined, once one is chosen.
    pub fn host(&self) -> Option<SocketAddr> {
        match &self.phase {
            Phase::Joined { host, .. } => Some(*host),
            Phase::Browsing(_) | Phase::Failed => None,
        }
    }

    /// Browses or plays for one frame, at `now`.
    pub fn frame(&mut self, now: Duration) {
        let game = self.game;
        match &mut self.phase {
            Phase::Browsing(browser) => {
                browser.poll();
                let hosts = browser.hosts();
                let Some(chosen) = hosts
                    .iter()
                    .find(|host| host.compatibility == game.compatibility)
                else {
                    return;
                };
                for host in &hosts {
                    println!(
                        "{}: LAN host {:?} at {}, {}/{} players{}",
                        game.app,
                        host.name,
                        host.addr,
                        host.players,
                        host.max_players,
                        if host.compatibility == game.compatibility {
                            ""
                        } else {
                            ", another build"
                        },
                    );
                }
                println!("{}: joining {}", game.app, chosen.addr);
                self.phase = match connect(game, chosen.addr, self.tick_hz) {
                    Ok(phase) => phase,
                    Err(error) => {
                        crate::log::error!("lan: {error}");
                        Phase::Failed
                    }
                };
            }
            Phase::Joined { host, client } => {
                client.update(now);
                if !self.reported_session
                    && let Some(session) = client.session_id()
                {
                    crate::log::info!("lan: in session {session:?} with {host}");
                    self.reported_session = true;
                }
                if !self.reported_end {
                    if client.handshake_blocked() {
                        crate::log::warn!("lan: {host} runs another build and refused us");
                        self.reported_end = true;
                    } else if let Some(ended) = client.ended() {
                        log_end(*host, ended, client);
                        self.reported_end = true;
                    }
                }
            }
            Phase::Failed => {}
        }
    }
}

/// Starts a connect to `addr` and the session client over it. The client
/// says hello once the transport is up; until then its sends are
/// backpressure, which it retries.
fn connect(game: LanGame, addr: SocketAddr, tick_hz: u32) -> Result<Phase, LanError> {
    let transport = UdpTransport::connect(addr, game.protocol_id).map_err(LanError::Connect)?;
    let client =
        Client::new_with_compatibility(World::new(), transport, tick_hz, game.compatibility);
    Ok(Phase::Joined {
        host: addr,
        client: Box::new(client),
    })
}

fn log_end(host: SocketAddr, ended: Ended, client: &Client<UdpTransport>) {
    match ended {
        Ended::ByServer(reason) => {
            crate::log::warn!("lan: {host} ended the session: {reason:?}");
        }
        Ended::Lost => crate::log::warn!(
            "lan: the link to {host} ended: {:?}",
            client.transport().end_reason()
        ),
    }
}

impl DebugModule for LanClient {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("lan");
        out.row_str("role", "client");
        let (Some(host), Some(client)) = (self.host(), self.client()) else {
            out.row_str(
                "host",
                match self.phase {
                    Phase::Failed => "connect failed",
                    Phase::Browsing(_) | Phase::Joined { .. } => "looking",
                },
            );
            return;
        };
        out.row("host", format_args!("{host}"));
        match client.session_id() {
            Some(session) => out.row("session", format_args!("{}", session.0)),
            None => out.row_str("session", "handshaking"),
        }
        out.row(
            "applied",
            format_args!("tick {}", client.last_applied_tick().get()),
        );
    }
}

#[cfg(test)]
mod tests;
