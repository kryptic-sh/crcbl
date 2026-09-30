//! LAN play in the sandbox: `--host`, `--join` and `--browse`, over the
//! engine's own UDP transport.
//!
//! - **`--host [PORT]`** binds a `crcbl::net::udp::UdpListener` and opens a
//!   `discovery::Announcer` beside it, advertising the listener's port. Every frame the listener's newly
//!   confirmed peers go to a [`crcbl::server::Host`] as `Box<dyn Transport>`,
//!   the way the Steam path takes its listener's peers, and the announced
//!   player count follows the host's.
//! - **`--join <IP:PORT>`** connects a `UdpTransport` to that address and
//!   runs a [`crcbl::client::Client`] over it: direct connect, first-class.
//! - **`--browse`** polls a `discovery::Browser`, prints every host it heard, and joins the first one this build can play
//!   with. There is no list on screen to choose from yet — the backlog says
//!   what one would take.
//!
//! The host's world is the smallest one that replicates something: a
//! "players" system with an entity per admitted peer. The sandbox has no
//! game, so there is nothing to play beyond joining, receiving the host's
//! snapshots, and seeing the player count change as others come and go. The
//! F3 panel's "lan" section shows where each side stands.
//!
//! # One datagram per snapshot
//!
//! A UDP snapshot travels on the unreliable channel, which takes one
//! datagram — [`MAX_UNRELIABLE_PAYLOAD`](crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD)
//! bytes. This world's snapshot sits far under it (a test holds that); a
//! world that grew past it would have each snapshot fitted to the datagram,
//! the least urgent updates held back for later ones (`crcbl::net::budget`),
//! which the F3 panel counts. Only an update that cannot fit a datagram on
//! its own is refused, which the host records by name
//! ([`crcbl::server::SnapshotTooLarge`]) and this module logs.
//!
//! # Native only
//!
//! Web builds have no networking, by the LOCKED rule in
//! `docs/notes/simulation.md`, and the transport is compiled out of them. On
//! `wasm32` the three flags are not parsed at all and [`Lan`] is inert, as
//! `crate::steam`'s link is without its feature.

#[cfg(not(target_arch = "wasm32"))]
pub use imp::LanMode;
pub use imp::{Lan, LanError};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::HashMap;
    use std::io;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::num::NonZeroU16;
    use std::time::Duration;

    use crcbl::client::{Client, Ended};
    use crcbl::ecs::{Entity, System, World};
    use crcbl::net::ProtocolCompatibility;
    use crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD;
    use crcbl::net::udp::discovery::{Announcement, Announcer, Browser, DISCOVERY_PORT};
    use crcbl::net::udp::{ConnectError, UdpListener, UdpTransport};
    use crcbl::server::{Host, HostConfig, PeerEvent, PeerId};
    use crcbl::ui::{DebugModule, DebugPanel, DebugSection};

    /// The endpoint protocol id the sandbox's links speak — `SBOX`. A
    /// listener or browser on another answers nothing and lists nothing.
    pub const PROTOCOL_ID: u32 = u32::from_be_bytes(*b"SBOX");

    /// What the session handshake gates on. The schema hash spells `SBOX`,
    /// so no other sample's client is admitted.
    pub const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 0x0043_5243_424C,
        schema_hash: 0x0000_5342_4F58,
    };

    /// The most players a sandbox host admits, and what it announces.
    pub const MAX_PLAYERS: u16 = 8;

    /// The replicated system holding one entity per admitted peer.
    const PLAYERS: &str = "players";

    /// The name a host announces.
    const HOST_NAME: &str = "crcbl sandbox";

    /// The least time between two logged snapshot refusals: one a second
    /// says the world is too big without a line every tick.
    const REFUSAL_LOG_INTERVAL: Duration = Duration::from_secs(1);

    /// What the command line asked of the network.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum LanMode {
        /// No networking: the sandbox as it always ran.
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

    /// The sandbox's side of a LAN session, or none.
    #[derive(Debug)]
    pub struct Lan {
        role: Role,
        /// Wall time since the session started, summed from each frame's
        /// `render_dt`, so a paused sandbox keeps its links alive.
        now: Duration,
    }

    #[derive(Debug)]
    enum Role {
        Off,
        Host(Box<LanHost>),
        Client(Box<LanClient>),
    }

    impl Lan {
        /// No networking.
        pub const fn off() -> Self {
            Self {
                role: Role::Off,
                now: Duration::ZERO,
            }
        }

        /// Starts what `mode` asks for, ticking at `tick_hz` — a host and
        /// its clients must agree on it. Prints where a host listens, since
        /// `--join` needs the port.
        ///
        /// # Errors
        ///
        /// [`LanError`] when a socket could not be bound or a connect could
        /// not start.
        pub fn start(mode: LanMode, tick_hz: u32) -> Result<Self, LanError> {
            let role = match mode {
                LanMode::Off => Role::Off,
                LanMode::Host { port } => {
                    let host = LanHost::open(
                        (Ipv4Addr::UNSPECIFIED, port).into(),
                        (Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT).into(),
                        Some(SocketAddr::V4(SocketAddrV4::new(
                            Ipv4Addr::BROADCAST,
                            DISCOVERY_PORT,
                        ))),
                        tick_hz,
                    )?;
                    println!(
                        "sandbox: hosting on UDP port {}{}",
                        host.game_port(),
                        if host.announcer.is_some() {
                            ", announced on the LAN"
                        } else {
                            " (not announced; --join by address)"
                        },
                    );
                    Role::Host(Box::new(host))
                }
                LanMode::Join(addr) => Role::Client(Box::new(LanClient::join(addr, tick_hz)?)),
                LanMode::Browse => {
                    let browser = Browser::open(PROTOCOL_ID).map_err(LanError::Browse)?;
                    Role::Client(Box::new(LanClient::browse(browser, tick_hz)))
                }
            };
            Ok(Self {
                role,
                now: Duration::ZERO,
            })
        }

        /// Serves the session for one frame covering `render_dt`.
        pub fn frame(&mut self, render_dt: Duration) {
            self.now += render_dt;
            match &mut self.role {
                Role::Off => {}
                Role::Host(host) => host.frame(self.now),
                Role::Client(client) => client.frame(self.now),
            }
        }

        /// Adds the "lan" section to the F3 panel — only with a session, so
        /// a run without one has the panel it always had.
        pub fn debug_sections(&self, panel: &mut DebugPanel) {
            match &self.role {
                Role::Off => {}
                Role::Host(host) => panel.add(host.as_ref()),
                Role::Client(client) => panel.add(client.as_ref()),
            }
        }
    }

    /// A host: the listener, the announcer beside it, and the session host
    /// its peers are handed to.
    pub struct LanHost {
        listener: UdpListener,
        /// `None` when the discovery port could not be bound — another host
        /// on this machine holds it. The session is still joinable by
        /// address.
        announcer: Option<Announcer>,
        host: Host,
        /// Each admitted peer's entity in the "players" system.
        players: HashMap<PeerId, Entity>,
        /// Refusals already logged, and when the last line went out.
        refusals_logged: u64,
        last_refusal_log: Option<Duration>,
    }

    impl LanHost {
        /// A host listening on `listen`, announcing from `announce_at` and
        /// broadcasting to `broadcast_to` (or answering queries only, for
        /// `None`), ticking at `tick_hz`.
        ///
        /// # Errors
        ///
        /// [`LanError::Listen`] when the listener cannot be bound. An
        /// announcer that cannot be bound is logged and left out instead:
        /// discovery is a convenience over connecting by address.
        pub fn open(
            listen: SocketAddr,
            announce_at: SocketAddr,
            broadcast_to: Option<SocketAddr>,
            tick_hz: u32,
        ) -> Result<Self, LanError> {
            let listener = UdpListener::bind(listen, PROTOCOL_ID).map_err(LanError::Listen)?;
            let port = listener.local_addr().map_err(LanError::Listen)?.port();
            let game_port = NonZeroU16::new(port)
                .ok_or_else(|| LanError::Listen(io::Error::other("the listener reports port 0")))?;
            let mut announcement =
                Announcement::new(PROTOCOL_ID, game_port, COMPATIBILITY, HOST_NAME);
            announcement.max_players = MAX_PLAYERS;
            let announcer = match Announcer::bind_with(
                announce_at,
                announcement,
                broadcast_to,
                crcbl::net::SystemClock::new(),
            ) {
                Ok(announcer) => Some(announcer),
                Err(error) => {
                    crcbl::log::warn!(
                        "lan: not announcing on {announce_at}: {error}; join by address instead"
                    );
                    None
                }
            };
            let mut host = Host::new(
                world(),
                HostConfig {
                    max_peers: usize::from(MAX_PLAYERS),
                    tick_hz,
                    compatibility: COMPATIBILITY,
                },
            );
            // The host's clock takes its baseline here, as `Loopback` spends
            // its first update at zero.
            host.update(Duration::ZERO);
            Ok(Self {
                listener,
                announcer,
                host,
                players: HashMap::new(),
                refusals_logged: 0,
                last_refusal_log: None,
            })
        }

        /// The port the listener is bound to.
        pub fn game_port(&self) -> u16 {
            self.listener.local_addr().map_or(0, |addr| addr.port())
        }

        /// The session host.
        #[cfg(test)]
        pub fn host(&self) -> &Host {
            &self.host
        }

        /// What the host announces, while it announces.
        #[cfg(test)]
        pub fn announcement(&self) -> Option<&Announcement> {
            self.announcer.as_ref().map(Announcer::announcement)
        }

        /// Where the announcer answers queries, while it announces.
        #[cfg(test)]
        pub fn announcer_addr(&self) -> Option<SocketAddr> {
            self.announcer
                .as_ref()
                .and_then(|announcer| announcer.local_addr().ok())
        }

        /// Takes in newly confirmed peers, runs the host to `now`, keeps the
        /// players system and the announcement in step with who is in, and
        /// answers discovery queries.
        pub fn frame(&mut self, now: Duration) {
            while let Some(peer) = self.listener.accept() {
                crcbl::log::info!("lan: {} connected", peer.peer_addr());
                self.host.add(Box::new(peer));
            }
            self.host.update(now);
            let events: Vec<PeerEvent> = self.host.events().collect();
            for event in events {
                crcbl::log::info!("lan: {event:?}");
                self.apply(event);
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
        }

        /// One session change, reflected in the players system: an entity
        /// per admitted peer, marked connected or not.
        fn apply(&mut self, event: PeerEvent) {
            let world = self.host.world_mut();
            match event {
                PeerEvent::Joined(peer) => {
                    let entity = world.spawn();
                    players(world).attach(entity, true);
                    self.players.insert(peer, entity);
                }
                PeerEvent::Lost(peer) | PeerEvent::Resumed(peer) => {
                    if let Some(connected) = self
                        .players
                        .get(&peer)
                        .and_then(|&entity| players(world).get_mut(entity))
                    {
                        *connected = matches!(event, PeerEvent::Resumed(_));
                    }
                }
                PeerEvent::Left(peer) => {
                    if let Some(entity) = self.players.remove(&peer) {
                        world.despawn(entity);
                    }
                }
            }
        }

        /// Logs a snapshot the transport refused as too long, naming the
        /// limit — at most once per [`REFUSAL_LOG_INTERVAL`].
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
                crcbl::log::warn!(
                    "lan: {snapshot} ({refused} refused so far); one entity's update must fit \
                     a UDP datagram on its own"
                );
            }
            self.refusals_logged = refused;
            self.last_refusal_log = Some(now);
        }
    }

    impl std::fmt::Debug for LanHost {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("LanHost")
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
                format_args!("{}/{MAX_PLAYERS}", self.host.peer_count()),
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

    /// The host's world: an empty "players" system the peers fill.
    fn world() -> World {
        let mut world = World::new();
        world.register_system(Box::new(System::<bool>::new(PLAYERS)));
        world
    }

    /// The players system, which [`world`] always registers.
    fn players(world: &mut World) -> &mut System<bool> {
        world
            .system_mut::<System<bool>>()
            .expect("the host's world registers the players system")
    }

    /// A client: looking for a host, or in a session with one.
    #[derive(Debug)]
    pub struct LanClient {
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
        /// Starts a connect to the host at `addr`.
        ///
        /// # Errors
        ///
        /// [`LanError::Connect`] when no socket could be bound or no secret
        /// drawn.
        pub fn join(addr: SocketAddr, tick_hz: u32) -> Result<Self, LanError> {
            Ok(Self::in_phase(connect(addr, tick_hz)?, tick_hz))
        }

        /// Looks for hosts with `browser`, and joins the first this build can
        /// play with.
        pub fn browse(browser: Browser, tick_hz: u32) -> Self {
            Self::in_phase(Phase::Browsing(browser), tick_hz)
        }

        fn in_phase(phase: Phase, tick_hz: u32) -> Self {
            Self {
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

        /// The host joined, once one is chosen.
        pub fn host(&self) -> Option<SocketAddr> {
            match &self.phase {
                Phase::Joined { host, .. } => Some(*host),
                Phase::Browsing(_) | Phase::Failed => None,
            }
        }

        /// Browses or plays for one frame, at `now`.
        pub fn frame(&mut self, now: Duration) {
            match &mut self.phase {
                Phase::Browsing(browser) => {
                    browser.poll();
                    let hosts = browser.hosts();
                    let Some(chosen) = hosts
                        .iter()
                        .find(|host| host.compatibility == COMPATIBILITY)
                    else {
                        return;
                    };
                    for host in &hosts {
                        println!(
                            "sandbox: LAN host {:?} at {}, {}/{} players{}",
                            host.name,
                            host.addr,
                            host.players,
                            host.max_players,
                            if host.compatibility == COMPATIBILITY {
                                ""
                            } else {
                                ", another build"
                            },
                        );
                    }
                    println!("sandbox: joining {}", chosen.addr);
                    self.phase = match connect(chosen.addr, self.tick_hz) {
                        Ok(phase) => phase,
                        Err(error) => {
                            crcbl::log::error!("lan: {error}");
                            Phase::Failed
                        }
                    };
                }
                Phase::Joined { host, client } => {
                    client.update(now);
                    if !self.reported_session
                        && let Some(session) = client.session_id()
                    {
                        crcbl::log::info!("lan: in session {session:?} with {host}");
                        self.reported_session = true;
                    }
                    if !self.reported_end {
                        if client.handshake_blocked() {
                            crcbl::log::warn!("lan: {host} runs another build and refused us");
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

    /// Starts a connect to `addr` and the session client over it. The
    /// client says hello once the transport is up; until then its sends
    /// are backpressure, which it retries.
    fn connect(addr: SocketAddr, tick_hz: u32) -> Result<Phase, LanError> {
        let transport = UdpTransport::connect(addr, PROTOCOL_ID).map_err(LanError::Connect)?;
        let client =
            Client::new_with_compatibility(World::new(), transport, tick_hz, COMPATIBILITY);
        Ok(Phase::Joined {
            host: addr,
            client: Box::new(client),
        })
    }

    fn log_end(host: SocketAddr, ended: Ended, client: &Client<UdpTransport>) {
        match ended {
            Ended::ByServer(reason) => {
                crcbl::log::warn!("lan: {host} ended the session: {reason:?}");
            }
            Ended::Lost => crcbl::log::warn!(
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
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::time::Duration;

    use crcbl::ui::DebugPanel;

    /// No networking in a web build.
    #[derive(Debug)]
    pub struct Lan;

    impl Lan {
        /// No session.
        pub const fn off() -> Self {
            Self
        }

        /// Nothing to serve.
        pub fn frame(&mut self, _render_dt: Duration) {}

        /// No section without a session.
        pub fn debug_sections(&self, _panel: &mut DebugPanel) {}
    }

    /// Nothing to fail at: a web build starts no LAN session.
    #[derive(Debug)]
    pub enum LanError {}

    impl std::fmt::Display for LanError {
        fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match *self {}
        }
    }
}
