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
//!   build can join — passing over a full host or another build's, as the
//!   lobby does. A sample that lets the player choose runs a
//!   [`lobby::Lobby`] instead and joins the address it picks.
//! - **[`lobby`]** is that choosing, without its look: the hosts a
//!   [`Browser`] heard sorted into joinable and not (with why), the address
//!   typed for a direct connect, what a pick asks for, and the reason a join
//!   failed or a session ended — the game draws its own menu from it.
//! - **[`LanMode`]** is what `--host [PORT]`, `--join <IP:PORT>` and
//!   `--browse` ask for, parsed by [`LanMode::consume`] so every sample reads
//!   the three flags alike.
//! - **[`console`]** is a dedicated server's stdin console and the sleep to
//!   its next tick, for every headless server that serves on the wall clock
//!   until it is told to stop.
//!
//! Both sides add a "lan" section to the F3 panel saying where they stand,
//! and keep a [`netgraph::Netgraph`] — a "net" section with each link's round
//! trip, its deviation, loss, resends, bytes and snapshot size, a client's
//! playout, and a graph of the round trip, the loss and the snapshot — that a
//! sample adds beside it.
//!
//! A host records its session on request ([`LanHost::record`], what
//! `--record <FILE>` asks for) through a [`Recorder`] it pulls after every
//! frame, and finishes the file when it is told to stop or when it is
//! dropped — a window closing, or a panic unwinding through its owner.
//!
//! # One datagram per snapshot
//!
//! A UDP snapshot travels on the unreliable channel, which takes one
//! datagram — [`MAX_UNRELIABLE_PAYLOAD`] bytes. A world whose snapshot grew
//! past it has each snapshot fitted to the datagram, the least urgent updates
//! held back for later ones (`crcbl::net::budget`), which the F3 panel
//! counts; a peer whose snapshots go on not fitting is sent them less often
//! (`crcbl::server::cadence`), and the panel shows each peer's interval. An
//! update that cannot fit a datagram on its own is withheld while the rest of
//! the snapshot ships, which the host records by name
//! ([`crate::server::UpdateTooLarge`]) and this module logs; only a snapshot
//! whose framing alone overflows is refused
//! ([`crate::server::SnapshotTooLarge`]), logged the same way.
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
use std::path::Path;
use std::time::Duration;

use crate::args::Consumed;
use crate::client::{Client, Ended};
use crate::ecs::World;
use crate::net::reliable::MAX_UNRELIABLE_PAYLOAD;
use crate::net::udp::discovery::{Announcement, Announcer, Browser, DISCOVERY_PORT};
use crate::net::udp::{ConnectError, UdpListener, UdpTransport};
use crate::net::{ProtocolCompatibility, SessionEndReason};
use crate::replay_record::{RecordError, RecordSummary, Recorder};
use crate::server::{Host, HostConfig, PeerEvent};
use crate::ui::{DebugModule, DebugSection};

use self::lobby::Unjoinable;
use self::netgraph::{LinkReading, Netgraph, Role};

/// The least time between two logged snapshot refusals, or two withheld
/// updates: one a second says the world is too big without a line every
/// tick.
const REFUSAL_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// What a climbing count has had logged: a line when it has moved, at most
/// one every [`REFUSAL_LOG_INTERVAL`].
#[derive(Debug, Default)]
struct ThrottledLog {
    /// The count the last line reported.
    logged: u64,
    /// When the last line went out.
    last: Option<Duration>,
}

impl ThrottledLog {
    /// Whether `count` is due a line at `now`: it moved since the last line,
    /// and that line is [`REFUSAL_LOG_INTERVAL`] old. A line found due is
    /// taken as written.
    fn due(&mut self, count: u64, now: Duration) -> bool {
        if count == self.logged {
            return false;
        }
        if self
            .last
            .is_some_and(|last| now.saturating_sub(last) < REFUSAL_LOG_INTERVAL)
        {
            return false;
        }
        self.logged = count;
        self.last = Some(now);
        true
    }
}

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
    /// The host's recording could not start.
    Record(RecordError),
}

impl std::fmt::Display for LanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Listen(error) => write!(f, "cannot host: {error}"),
            Self::Browse(error) => write!(f, "cannot look for hosts: {error}"),
            Self::Connect(error) => write!(f, "cannot connect: {error}"),
            Self::Record(error) => write!(f, "cannot record: {error}"),
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
    /// The host's tick rate, which a recording's header carries.
    tick_hz: u32,
    /// The session's recording, while it records.
    recorder: Option<Recorder>,
    /// Snapshot refusals logged.
    refusals: ThrottledLog,
    /// Withheld updates logged.
    withheld: ThrottledLog,
    /// Each peer's link, for the "net" section.
    netgraph: Netgraph,
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
            tick_hz,
            recorder: None,
            refusals: ThrottledLog::default(),
            withheld: ThrottledLog::default(),
            netgraph: Netgraph::new(Role::Host),
        })
    }

    /// Records the session to a new file at `path` from now on, until
    /// [`stop_recording`](Self::stop_recording) or until this host is
    /// dropped. Started before the first frame — straight after
    /// [`open`](Self::open), before any player's transport is added — the
    /// file is one a fresh host built the same way re-simulates from its
    /// first tick; [`crate::replay_record`]'s module docs have what a
    /// recording holds and where one starts.
    ///
    /// # Errors
    ///
    /// [`LanError::Record`]: something exists at `path` — a recording never
    /// overwrites — the file could not be created, or this host is already
    /// recording.
    pub fn record(&mut self, path: &Path) -> Result<(), LanError> {
        if let Some(recorder) = &self.recorder {
            return Err(LanError::Record(RecordError::Recording(
                recorder.path().to_path_buf(),
            )));
        }
        let recorder =
            Recorder::start(path, &mut self.host, self.tick_hz).map_err(LanError::Record)?;
        crate::log::info!("lan: recording the session to {}", path.display());
        self.recorder = Some(recorder);
        Ok(())
    }

    /// The file the session is being recorded to, while it is.
    pub fn recording(&self) -> Option<&Path> {
        self.recorder.as_ref().map(Recorder::path)
    }

    /// Stops recording and finishes the file — `None` when it was not
    /// recording. The host serves on.
    ///
    /// # Errors
    ///
    /// The recording's [`RecordError`], from [`Recorder::finish`].
    pub fn stop_recording(&mut self) -> Option<Result<RecordSummary, RecordError>> {
        let recorder = self.recorder.take()?;
        Some(recorder.finish(&mut self.host))
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

    /// Each peer's link, as of the last [`frame`](Self::frame): what a sample
    /// adds to its panel beside this host's own "lan" section.
    pub fn netgraph(&self) -> &Netgraph {
        &self.netgraph
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
        if let Some(recorder) = &mut self.recorder
            && let Err(error) = recorder.record(&mut self.host)
        {
            crate::log::error!("lan: {error}; the recording stops here");
            log_finished(self.stop_recording());
        }
        let events: Vec<PeerEvent> = self.host.events().collect();
        for event in &events {
            crate::log::info!("lan: {event:?}");
        }
        let host = &self.host;
        self.netgraph.record(
            now,
            host.peers().map(|peer| {
                let reading = LinkReading {
                    stats: host.peer_link_stats(peer),
                    snapshot_bytes: host
                        .peer_stats(peer)
                        .map_or(0, |stats| stats.last_snapshot_bytes),
                    playout: None,
                };
                (peer.get(), reading)
            }),
        );
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

    /// Logs a snapshot the transport refused as too long, and an update
    /// withheld as too long for any snapshot, each naming the limit — each at
    /// most once per [`REFUSAL_LOG_INTERVAL`].
    fn report_refusals(&mut self, now: Duration) {
        let refused = self.host.oversized_snapshot_count();
        if self.refusals.due(refused, now)
            && let Some(snapshot) = self.host.last_oversized_snapshot()
        {
            crate::log::warn!(
                "lan: {snapshot} ({refused} refused so far); a snapshot's framing must fit a UDP \
                 datagram"
            );
        }
        let withheld = self.host.oversized_update_count();
        if self.withheld.due(withheld, now)
            && let Some(update) = self.host.last_oversized_update()
        {
            crate::log::warn!(
                "lan: {update} ({withheld} withheld so far); one entity's update must fit a UDP \
                 datagram on its own, and a remote player sees it stale until it does"
            );
        }
    }
}

/// Finishes the recording, if there is one: a window closing drops its host,
/// and so does a panic unwinding through the host's owner, and the file is
/// written whole either way with every tick the host ran.
impl Drop for LanHost {
    fn drop(&mut self) {
        log_finished(self.stop_recording());
    }
}

/// Logs how a recording that stopped with no one to answer it finished.
fn log_finished(finished: Option<Result<RecordSummary, RecordError>>) {
    match finished {
        Some(Ok(summary)) => crate::log::info!("lan: {summary}"),
        Some(Err(error)) => {
            crate::log::error!("lan: the recording did not finish whole: {error}");
        }
        None => {}
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
        out.row(
            "withheld",
            format_args!("{}", self.host.oversized_update_count()),
        );
        let intervals: Vec<String> = self
            .host
            .peers()
            .filter_map(|peer| self.host.peer_stats(peer))
            .map(|stats| stats.snapshot_interval_ticks.to_string())
            .collect();
        out.row(
            "snapshot every",
            format_args!("{} tick(s)", intervals.join(" ")),
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
    /// The link to the host, for the "net" section.
    netgraph: Netgraph,
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
    /// build can join: the first a [`lobby::Lobby`] would make a row, by
    /// [`Unjoinable::of`]. A host that is full or of another build is
    /// printed with why and passed over; with none joinable it goes on
    /// looking.
    pub fn browse(game: LanGame, browser: Browser, tick_hz: u32) -> Self {
        Self::in_phase(game, Phase::Browsing(browser), tick_hz)
    }

    /// Looks for hosts of `game` on the LAN — a [`Browser`] querying the
    /// broadcast address — and joins the first this build can join.
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
            netgraph: Netgraph::new(Role::Client),
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

    /// The link to the host, as of the last [`frame`](Self::frame) — none
    /// while looking for one: what a sample adds to its panel beside this
    /// client's own "lan" section.
    pub fn netgraph(&self) -> &Netgraph {
        &self.netgraph
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
        self.play(now);
        let link = self.client().map(LinkReading::of_client);
        self.netgraph.record(now, link.map(|reading| (0, reading)));
    }

    fn play(&mut self, now: Duration) {
        let game = self.game;
        match &mut self.phase {
            Phase::Browsing(browser) => {
                browser.poll();
                let hosts = browser.hosts();
                let Some(chosen) = hosts
                    .iter()
                    .find(|host| Unjoinable::of(game.compatibility, host).is_none())
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
                        match Unjoinable::of(game.compatibility, host) {
                            Some(why) => format!(", {}", why.label()),
                            None => String::new(),
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
                    if let Some(refusal) = client.handshake_refusal() {
                        crate::log::warn!("lan: {host} refused us: {}", refusal.msg);
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

/// How a session `client` was in ended, in words: what the host said, or
/// what the link reported. What a failed join and an ended game both show.
pub fn how_it_ended(ended: Ended, client: &Client<UdpTransport>) -> String {
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

pub mod console;
pub mod lobby;
pub mod netgraph;

#[cfg(test)]
mod tests;
