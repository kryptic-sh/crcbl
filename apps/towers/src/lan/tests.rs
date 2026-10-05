//! A towers host — a player's, or a dedicated [`Server`] — and its joiners in
//! one process, over UDP loopback: no window, no renderer, no loop — each
//! [`Game`] driven a tick and a frame at a time, as `Towers::tick` and
//! `Towers::draw` drive it.
//!
//! Every socket binds `127.0.0.1:0`, the discovery port included, so no test
//! needs a free fixed port, and no test sends to a broadcast address: the
//! browser queries the host's announcer directly. Loopback delivery is
//! asynchronous, so each step is followed by a short pause, and every wait is
//! bounded by [`MAX_FRAMES`] — reached only when what it waits for never
//! comes.

use std::net::{Ipv4Addr, SocketAddr};
use std::thread;
use std::time::Duration;

use crcbl::client::Client;
use crcbl::ecs::World;
use crcbl::lan::{LanBind, LanClient, LanGame, LanHost};
use crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD;
use crcbl::net::udp::discovery::{Browser, BrowserConfig};
use crcbl::net::{
    InMemoryTransport, Message, MessageKind, ProtocolCompatibility, SessionState, SystemClock,
    Transport,
};
use crcbl::server::{Host, HostConfig, PeerEvent};

use super::event::{self, Event};
use super::serve::{BANS_FILE, Console, Next, STATUS_INTERVAL, Server, serve_until_quit};
use super::{JOIN_TIMEOUT, JoinFailure, Joining, MAX_PLAYERS, PROTOCOL_ID, Progress, SESSION};
use crate::game::{Controls, Game, Refusal, Stats};
use crate::map::{Map, MapError, MapWireError};
use crate::save::Vault;
use crate::tower::{self, Tier};

pub(crate) const TICK_HZ: u32 = crate::game::DEFAULT_TICK_HZ;

/// One frame at [`TICK_HZ`]: every step runs one tick of each game and about
/// one of the host's server.
pub(crate) const FRAME: Duration = Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);

/// The most frames any wait here runs: ten seconds of game time, and a few
/// seconds of wall time at [`PAUSE`] a step.
pub(crate) const MAX_FRAMES: usize = 600;

/// The pause after each step, for loopback to deliver.
pub(crate) const PAUSE: Duration = Duration::from_millis(1);

/// How many ticks a command one joiner sends may take to reach another's
/// screen: its way to the host, the host's tick, and the snapshot's way back
/// — a few ticks on loopback, bounded well above that.
const SEEN_WITHIN: usize = 30;

/// Loopback, any free port.
pub(crate) fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// Where a host binds on loopback: announcing on loopback, broadcasting
/// nowhere.
pub(crate) fn on_loopback() -> LanBind {
    LanBind {
        listen: loopback(),
        announce_at: loopback(),
        broadcast_to: None,
    }
}

/// A host on loopback, with a player of its own.
fn host() -> Game {
    host_on(&Map::built_in())
}

/// …playing `map`.
fn host_on(map: &Map) -> Game {
    Game::host(TICK_HZ, map, on_loopback(), None, next_player())
        .expect("loopback UDP must be available to these tests")
}

/// A player id no other call in this test binary has drawn: every player
/// in one session must be their own, or the host refuses the second as a
/// duplicate.
pub(crate) fn next_player() -> crcbl::net::PlayerId {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    crcbl::net::PlayerId::from_seed(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// A joiner: waiting for the host's map, then playing on it — or not, and
/// why.
#[derive(Debug)]
enum Joiner {
    Joining(Joining),
    Playing(Game),
    Failed(JoinFailure),
}

impl Joiner {
    /// A joiner through `lan`, given the whole [`JOIN_TIMEOUT`].
    fn through(lan: LanClient) -> Self {
        Self::Joining(Joining::new(lan, TICK_HZ, JOIN_TIMEOUT))
    }

    /// The game it plays, which it must have by now.
    fn game(&self) -> &Game {
        match self {
            Self::Playing(game) => game,
            other => panic!("the joiner is not playing: {other:?}"),
        }
    }

    fn game_mut(&mut self) -> &mut Game {
        match self {
            Self::Playing(game) => game,
            other => panic!("the joiner is not playing: {other:?}"),
        }
    }

    /// The engine's client, joining or playing.
    fn lan(&self) -> Option<&LanClient> {
        match self {
            Self::Joining(joining) => Some(joining.lan()),
            Self::Playing(game) => game.lan_client(),
            Self::Failed(_) => None,
        }
    }

    fn tick(&mut self) {
        if let Self::Playing(game) = self {
            game.tick();
        }
    }

    /// One frame of `render_dt`: the join runs, or the game reads the host.
    fn frame(self, render_dt: Duration) -> Self {
        match self {
            Self::Joining(joining) => match joining.step(render_dt) {
                Progress::Waiting(joining) => Self::Joining(joining),
                Progress::Joined(game) => Self::Playing(game),
                Progress::Failed(failure) => Self::Failed(failure),
            },
            Self::Playing(mut game) => {
                game.frame(render_dt);
                Self::Playing(game)
            }
            failed @ Self::Failed(_) => failed,
        }
    }

    /// Why the join failed, once it has.
    fn failure(&self) -> Option<&JoinFailure> {
        match self {
            Self::Failed(failure) => Some(failure),
            Self::Joining(_) | Self::Playing(_) => None,
        }
    }
}

/// A dedicated server on loopback, the time it has been served to, and every
/// status line it asked to print, with when.
struct Dedicated {
    server: Server,
    now: Duration,
    printed: Vec<(Duration, String)>,
}

fn dedicated() -> Dedicated {
    Dedicated {
        server: Server::open(
            on_loopback(),
            &Map::built_in(),
            TICK_HZ,
            None,
            Vault::nowhere(),
        )
        .expect("loopback UDP must be available to these tests"),
        now: Duration::ZERO,
        printed: Vec::new(),
    }
}

/// What a rig's joiners play in: a host with its own player, or a dedicated
/// server with none — stepped the same way, so every test below can be asked
/// of either.
trait Authority {
    /// The host's own player's tick, if it has one.
    fn tick(&mut self);
    /// Serves the session for one frame.
    fn frame(&mut self, render_dt: Duration);
    fn lan_host(&self) -> &LanHost;
    /// The stage's numbers, read off the stage itself.
    fn stats(&self) -> Stats;
}

impl Authority for Game {
    fn tick(&mut self) {
        Self::tick(self);
    }

    fn frame(&mut self, render_dt: Duration) {
        Self::frame(self, render_dt);
    }

    fn lan_host(&self) -> &LanHost {
        Self::lan_host(self).expect("a host")
    }

    fn stats(&self) -> Stats {
        Self::stats(self)
    }
}

impl Authority for Dedicated {
    fn tick(&mut self) {}

    fn frame(&mut self, render_dt: Duration) {
        self.now += render_dt;
        if let Some(line) = self.server.frame(self.now) {
            self.printed.push((self.now, line));
        }
    }

    fn lan_host(&self) -> &LanHost {
        self.server.lan()
    }

    fn stats(&self) -> Stats {
        self.server.stats()
    }
}

/// A host and its joiners, stepped together.
struct Rig<A> {
    host: A,
    joiners: Vec<Joiner>,
}

impl Rig<Game> {
    fn new() -> Self {
        Self {
            host: host(),
            joiners: Vec::new(),
        }
    }

    /// A rig with `count` joiners already playing.
    fn playing(count: usize) -> Self {
        Self::new().with_playing(count)
    }
}

impl Rig<Dedicated> {
    fn serving() -> Self {
        Self {
            host: dedicated(),
            joiners: Vec::new(),
        }
    }
}

impl<A: Authority> Rig<A> {
    /// This rig with `count` more joiners, every joiner playing.
    fn with_playing(mut self, count: usize) -> Self {
        for _ in 0..count {
            self.join();
        }
        self.until("every joiner's session", |rig| {
            rig.joiners.iter().all(playing)
        });
        self
    }

    /// Where a joiner reaches the host.
    fn address(&self) -> SocketAddr {
        (Ipv4Addr::LOCALHOST, self.host.lan_host().game_port()).into()
    }

    fn join(&mut self) {
        let client =
            LanClient::join(SESSION, next_player(), self.address(), TICK_HZ).expect("connect");
        self.joiners.push(Joiner::through(client));
    }

    /// Every player's tick, then the host's frame and every joiner's, then
    /// the pause.
    fn step(&mut self) {
        self.host.tick();
        for joiner in &mut self.joiners {
            joiner.tick();
        }
        self.host.frame(FRAME);
        self.joiners = self
            .joiners
            .drain(..)
            .map(|joiner| joiner.frame(FRAME))
            .collect();
        thread::sleep(PAUSE);
    }

    /// Steps until `done` holds, failing past [`MAX_FRAMES`]. Returns how
    /// many steps it took.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Self) -> bool) -> usize {
        for frames in 0..MAX_FRAMES {
            if done(self) {
                return frames;
            }
            self.step();
        }
        panic!("no {what} within {MAX_FRAMES} frames");
    }

    /// How many of the host's players are connected, its own among them if
    /// it has one.
    fn connected(&self) -> usize {
        let host = self.host.lan_host().host();
        host.peers()
            .filter(|&peer| host.peer_state(peer) == Some(SessionState::Connected))
            .count()
    }
}

/// Whether a joiner has the host's map, is in its session and has its
/// numbers.
fn playing(joiner: &Joiner) -> bool {
    let Joiner::Playing(game) = joiner else {
        return false;
    };
    game.lan_client()
        .and_then(LanClient::client)
        .is_some_and(|client| client.session_id().is_some())
        && game.stats().ticks > 0
}

/// Build a `kind` tower on `plot`.
fn build(plot: u8, kind: tower::Kind) -> Controls {
    Controls {
        place: Some(plot),
        kind,
        ..Controls::default()
    }
}

/// Send the next wave now.
fn send_wave() -> Controls {
    Controls {
        start_wave: true,
        ..Controls::default()
    }
}

/// **A tower one joiner builds is validated by the host and seen by the
/// other.** Joiner A's `PlaceTower` goes to the host, which builds it out of
/// the one purse; joiner B draws it within [`SEEN_WITHIN`] ticks, and reads
/// the same purse. B then asks for the same plot, and the host — not B —
/// turns it down.
#[test]
fn a_tower_one_joiner_builds_is_validated_by_the_host_and_seen_by_the_other() {
    let mut rig = Rig::playing(2);
    let purse = rig.host.stats().gold;
    let cost = tower::Kind::Splash.spec(Tier::Base).cost;

    rig.joiners[0]
        .game_mut()
        .set_controls(build(0, tower::Kind::Splash));
    rig.until("the host building joiner A's tower", |rig| {
        rig.host.stats().built == 1
    });
    assert_eq!(rig.host.stats().gold, purse - cost, "out of the one purse");
    let seen = rig.until("joiner B drawing it", |rig| {
        rig.joiners[1].game().render_state().towers[0]
            .is_some_and(|tower| tower.kind == tower::Kind::Splash && tower.tier == Tier::Base)
    });
    assert!(
        seen <= SEEN_WITHIN,
        "B saw A's tower {seen} ticks after it was built"
    );
    rig.until("joiner B reading the same purse", |rig| {
        rig.joiners[1].game().stats().gold == purse - cost
    });

    let refused = rig.host.stats().refused;
    rig.joiners[1]
        .game_mut()
        .set_controls(build(0, tower::Kind::Bolt));
    rig.until("the host refusing joiner B's build", |rig| {
        rig.host.stats().refused == refused + 1
    });
    assert_eq!(rig.host.stats().built, 1, "the plot was taken");
    rig.until("joiner A reading the refusal", |rig| {
        rig.joiners[0].game().stats().refused == refused + 1
    });
}

/// **A wave one joiner starts runs for every player.** B's `StartWave` brings
/// the first wave forward on the host, and A — and the host's own player —
/// see the wave count move and creeps walk.
#[test]
fn a_wave_one_joiner_starts_runs_for_every_player() {
    let mut rig = Rig::playing(2);
    assert_eq!(rig.host.stats().wave, 0);
    // The first wave comes by itself when the build phase runs out, so the
    // command has to be seen to bring it forward, not to wait it out.
    let due = rig
        .host
        .stats()
        .next_wave_in
        .expect("the build phase is running");
    rig.joiners[1].game_mut().set_controls(send_wave());
    let frames = rig.until("the host starting the wave", |rig| {
        rig.host.stats().wave == 1
    });
    assert!(
        (frames as f64) < 0.5 * due * f64::from(TICK_HZ),
        "the wave started {frames} ticks later, and was due by itself in {due} s"
    );
    assert_eq!(rig.host.stats().refused, 0);
    rig.until("creeps on every player's field", |rig| {
        rig.host.replicated().render.creeps_alive > 0
            && rig.joiners.iter().all(|joiner| {
                joiner.game().stats().wave == 1 && joiner.game().render_state().creeps_alive > 0
            })
    });
}

/// **A joiner who leaves does not stop the others.** A's link closes; the host
/// sees it go, and B goes on playing — its commands still validated, its field
/// still moving.
#[test]
fn a_joiner_who_leaves_does_not_stop_the_others() {
    let mut rig = Rig::playing(2);
    assert_eq!(rig.connected(), 3, "the host's own player and both joiners");

    drop(rig.joiners.remove(0));
    rig.until("the host seeing joiner A go", |rig| rig.connected() == 2);

    let ticks = rig.joiners[0].game().stats().ticks;
    rig.joiners[0]
        .game_mut()
        .set_controls(build(1, tower::Kind::Bolt));
    rig.until("the host building B's tower", |rig| {
        rig.host.stats().built == 1
    });
    rig.until("B drawing it, on a field still moving", |rig| {
        rig.joiners[0].game().render_state().towers[1].is_some()
            && rig.joiners[0].game().stats().ticks > ticks
    });
}

/// A browser whose query goes straight to `announcer`.
pub(crate) fn loopback_browser(announcer: SocketAddr) -> Browser {
    Browser::bind_with(
        loopback(),
        BrowserConfig {
            query_to: Some(announcer),
            ..BrowserConfig::new(PROTOCOL_ID)
        },
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests")
}

/// A player browsing for a host, its query sent straight to `announcer`.
fn browsing(announcer: SocketAddr) -> Joiner {
    Joiner::through(LanClient::browse(
        SESSION,
        next_player(),
        loopback_browser(announcer),
        TICK_HZ,
    ))
}

/// A map nothing like the committed field: one straight lane and two plots,
/// one either side — what a host could have loaded with `--scene`, and what
/// no joiner here has.
pub(crate) fn another_map() -> Map {
    let plot = |label: &str, z: f64| crate::scene::Plot {
        label: label.to_string(),
        position: [0.0, 0.0, z],
    };
    Map::new(
        vec![
            crcbl::math::DVec3::new(-10.0, 0.0, 0.0),
            crcbl::math::DVec3::new(10.0, 0.0, 0.0),
        ],
        vec![plot("north", -3.0), plot("south", 3.0)],
    )
    .expect("a straight lane with a plot either side is a map")
}

/// **A browser finds the towers host and plays in it.** The query goes
/// straight to the announcer's loopback port; the announce names this build
/// and the listener's port, the joiner connects where it points, and the
/// announcement counts the host's own player and the joiner.
#[test]
fn a_browser_finds_the_towers_host_and_plays_in_it() {
    let mut rig = Rig::new();
    let host = rig.host.lan_host().expect("a host");
    let announcer = host.announcer_addr().expect("the host announces");
    assert_eq!(
        host.announcement().map(|a| (a.players, a.max_players)),
        Some((1, MAX_PLAYERS)),
        "the host's own player is in from the start"
    );
    rig.joiners.push(browsing(announcer));

    rig.until("the browsed session", |rig| playing(&rig.joiners[0]));
    let joined = rig.joiners[0].lan().and_then(LanClient::host);
    assert_eq!(joined, Some(rig.address()));
    rig.until("the announcement counting both players", |rig| {
        rig.host
            .lan_host()
            .and_then(crcbl::lan::LanHost::announcement)
            .is_some_and(|announcement| announcement.players == 2)
    });
}

/// **A joiner plays on the host's map, which is none it has.** The host is on
/// [`another_map`] — two plots where the committed field has five — and two
/// joiners, one by address and one by browsing, each build their game on the
/// map the host sent: its plots, its path. A tower one of them asks for on
/// the host's **second** plot is built by the host and drawn by both, and
/// the host's own player sees it too.
#[test]
fn a_joiner_plays_on_the_hosts_map_which_is_none_it_has() {
    let map = another_map();
    assert_ne!(map, Map::built_in());
    let mut rig = Rig {
        host: host_on(&map),
        joiners: Vec::new(),
    };
    let announcer = rig
        .host
        .lan_host()
        .and_then(LanHost::announcer_addr)
        .expect("the host announces");
    rig.join();
    rig.joiners.push(browsing(announcer));
    rig.until("both joiners playing", |rig| {
        rig.joiners.iter().all(playing)
    });
    for joiner in &rig.joiners {
        assert_eq!(
            joiner.game().map(),
            &map,
            "a joiner is not on the host's map"
        );
        assert_eq!(joiner.game().stats().plots, 2);
    }

    let south = 1;
    rig.joiners[0]
        .game_mut()
        .set_controls(build(south, tower::Kind::Slow));
    rig.until("every player drawing the tower on the host's plot", |rig| {
        rig.host.render_state().towers[usize::from(south)].is_some()
            && rig
                .joiners
                .iter()
                .all(|joiner| joiner.game().render_state().towers[usize::from(south)].is_some())
    });
    assert_eq!(rig.host.stats().built_of(tower::Kind::Slow), 1);
    assert_eq!(
        rig.host.stats().refused,
        0,
        "the host's plot was the one asked for"
    );
}

/// **A host on another map is a host like any other to a browser**: it is
/// heard announcing towers' own compatibility — the map is no part of it —
/// and a browsing player joins it.
#[test]
fn a_browser_joins_a_host_on_another_map() {
    let mut rig = Rig {
        host: host_on(&another_map()),
        joiners: Vec::new(),
    };
    let announcer = rig
        .host
        .lan_host()
        .and_then(LanHost::announcer_addr)
        .expect("the host announces");
    assert_eq!(
        rig.host
            .lan_host()
            .and_then(LanHost::announcement)
            .map(|announcement| announcement.compatibility),
        Some(SESSION.compatibility)
    );
    rig.joiners.push(browsing(announcer));
    rig.until("the browsed session", |rig| playing(&rig.joiners[0]));
    assert_eq!(rig.joiners[0].game().map(), &another_map());
}

/// What the committed field's session hand-shook on before the map crossed
/// the wire: protocol version 3, and the field's fingerprint folded into the
/// schema.
const BEFORE_THE_MAP_WAS_SENT: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: SESSION.compatibility.engine_build_id,
    schema_hash: 0x9d7f_d7e0_2e75_7e3d,
};

/// A host that is nothing but the engine's: a session on an empty world,
/// announcing `game`, whose peers are sent whatever a test sends them.
struct Bare {
    lan: LanHost,
    now: Duration,
}

impl Bare {
    fn open(game: LanGame) -> Self {
        Self {
            lan: LanHost::open(game, on_loopback(), World::new(), TICK_HZ)
                .expect("loopback UDP must be available to these tests"),
            now: Duration::ZERO,
        }
    }

    fn address(&self) -> SocketAddr {
        (Ipv4Addr::LOCALHOST, self.lan.game_port()).into()
    }

    /// One frame, answering the peers who joined in it.
    fn frame(&mut self) -> Vec<PeerEvent> {
        self.now += FRAME;
        self.lan.frame(self.now)
    }
}

/// Steps `bare` and `joiner` until `done` holds, failing past
/// [`MAX_FRAMES`]. Returns the joiner, and the peer the host admitted if it
/// admitted one.
fn step_bare(
    bare: &mut Bare,
    mut joiner: Joiner,
    what: &str,
    mut on_join: impl FnMut(&mut LanHost, crcbl::server::PeerId),
    done: impl Fn(&Joiner) -> bool,
) -> Joiner {
    for _ in 0..MAX_FRAMES {
        if done(&joiner) {
            return joiner;
        }
        for event in bare.frame() {
            if let PeerEvent::Joined(peer) = event {
                on_join(&mut bare.lan, peer);
            }
        }
        joiner = joiner.frame(FRAME);
        thread::sleep(PAUSE);
    }
    panic!("no {what} within {MAX_FRAMES} frames");
}

/// **A build from before the map crossed the wire and one from after refuse
/// each other, by version** — both ways round, each side naming both
/// versions — so a joiner of the old protocol can never wait on a map, or a
/// new one play on its own.
#[test]
fn a_build_from_before_the_map_was_sent_is_refused_both_ways() {
    let old = LanGame {
        compatibility: BEFORE_THE_MAP_WAS_SENT,
        ..SESSION
    };
    let expected = format!(
        "client {}, server {}",
        BEFORE_THE_MAP_WAS_SENT.protocol_version, SESSION.compatibility.protocol_version
    );
    let mut rig = Rig::new();
    let client = LanClient::join(old, next_player(), rig.address(), TICK_HZ).expect("connect");
    rig.joiners.push(Joiner::through(client));
    rig.until("the new host refusing the old joiner", |rig| {
        rig.joiners[0].failure().is_some()
    });
    let Some(JoinFailure::Refused { reason, .. }) = rig.joiners[0].failure() else {
        panic!("not a refusal: {:?}", rig.joiners[0]);
    };
    assert!(reason.contains(&expected), "{reason}");
    assert_eq!(rig.connected(), 1, "only the host's own player is in");

    let mut bare = Bare::open(old);
    let client = LanClient::join(SESSION, next_player(), bare.address(), TICK_HZ).expect("connect");
    let joiner = step_bare(
        &mut bare,
        Joiner::through(client),
        "the old host refusing the new joiner",
        |_, _| {},
        |joiner| joiner.failure().is_some(),
    );
    let Some(JoinFailure::Refused { reason, .. }) = joiner.failure() else {
        panic!("not a refusal: {joiner:?}");
    };
    let expected = format!(
        "client {}, server {}",
        SESSION.compatibility.protocol_version, BEFORE_THE_MAP_WAS_SENT.protocol_version
    );
    assert!(reason.contains(&expected), "{reason}");
}

/// **A map the host sends that this build refuses ends the join, by name** —
/// bytes that are not a map, and a well-formed map with a plot on the lane —
/// and no game is built on either.
#[test]
fn a_malformed_map_ends_the_join_by_name() {
    let mut on_the_lane = another_map().to_wire();
    // The last plot's z, the final coordinate: onto the lane's centre line.
    let z = on_the_lane.len() - size_of::<u64>();
    on_the_lane[z..].copy_from_slice(&0.0_f64.to_bits().to_le_bytes());

    for (bytes, check) in [
        (
            b"not a towers map, nor anything like one".to_vec(),
            (|error: &MapWireError| matches!(error, MapWireError::NotAMap))
                as fn(&MapWireError) -> bool,
        ),
        (
            on_the_lane,
            |error| matches!(error, MapWireError::Map(MapError::OnTheLane { plot, .. }) if plot == "south"),
        ),
    ] {
        let mut bare = Bare::open(SESSION);
        let client =
            LanClient::join(SESSION, next_player(), bare.address(), TICK_HZ).expect("connect");
        let joiner = step_bare(
            &mut bare,
            Joiner::through(client),
            "the join ending on the map",
            |lan, peer| {
                lan.host_mut()
                    .send_event(peer, event::map(&bytes))
                    .expect("the joiner is connected");
            },
            |joiner| !matches!(joiner, Joiner::Joining(_)),
        );
        let Joiner::Failed(JoinFailure::BadMap { error, host }) = &joiner else {
            panic!("the join did not end on the map: {joiner:?}");
        };
        assert!(check(error), "{error:?}");
        assert_eq!(*host, bare.address());
        let named = joiner.failure().expect("failed").to_string();
        assert!(named.contains(&error.to_string()), "{named}");
    }
}

/// **A joiner builds nothing until the map comes, and then builds on it.**
/// A host that holds the map back: the joiner's session comes up and runs
/// for many frames with no game; the host sends the map, and the first game
/// the joiner has is on it. Held back past the timeout instead, the join ends
/// as no map — by name, and without a game.
#[test]
fn a_joiner_builds_nothing_until_the_map_comes() {
    let map = another_map();
    let mut bare = Bare::open(SESSION);
    let client = LanClient::join(SESSION, next_player(), bare.address(), TICK_HZ).expect("connect");
    let mut admitted = None;
    let in_session = |joiner: &Joiner| {
        joiner
            .lan()
            .and_then(LanClient::client)
            .is_some_and(|client| client.session_id().is_some())
    };
    let mut joiner = step_bare(
        &mut bare,
        Joiner::through(client),
        "the joiner's session",
        |_, peer| admitted = Some(peer),
        in_session,
    );
    for _ in 0..SEEN_WITHIN {
        bare.frame();
        joiner = joiner.frame(FRAME);
        thread::sleep(PAUSE);
        assert!(
            matches!(joiner, Joiner::Joining(_)),
            "a game with no map: {joiner:?}"
        );
    }
    bare.lan
        .host_mut()
        .send_event(admitted.expect("admitted"), event::map(&map.to_wire()))
        .expect("the joiner is connected");
    let joiner = step_bare(
        &mut bare,
        joiner,
        "the game",
        |_, _| {},
        |joiner| matches!(joiner, Joiner::Playing(_)),
    );
    assert_eq!(joiner.game().map(), &map);

    let client = LanClient::join(SESSION, next_player(), bare.address(), TICK_HZ).expect("connect");
    let joiner = step_bare(
        &mut bare,
        Joiner::through(client),
        "the second joiner's session",
        |_, _| {},
        in_session,
    );
    let Joiner::Failed(failure) = joiner.frame(JOIN_TIMEOUT) else {
        panic!("the join outlived its timeout with no map");
    };
    assert!(
        matches!(failure, JoinFailure::NoMap { waited, .. } if waited == JOIN_TIMEOUT),
        "{failure:?}"
    );
}

/// Messages between a client and a host over in-memory pairs, carried by
/// hand so a handshake reply can be lost on the way — as a datagram carrying
/// it could be, or could come after the client stopped waiting.
struct Relay {
    client_side: InMemoryTransport,
    host_side: InMemoryTransport,
    /// How many of the host's handshake replies to lose.
    lose_replies: usize,
}

impl Relay {
    fn carry(&mut self) {
        while let Some(msg) = self.client_side.recv().expect("in-memory") {
            forward(&mut self.host_side, msg);
        }
        while let Some(msg) = self.host_side.recv().expect("in-memory") {
            if self.lose_replies > 0 && crcbl::net::decode_handshake_result(&msg.payload).is_ok() {
                self.lose_replies -= 1;
                continue;
            }
            forward(&mut self.client_side, msg);
        }
    }
}

fn forward(to: &mut InMemoryTransport, msg: Message) {
    match msg.kind {
        MessageKind::Reliable => to.send_reliable(msg),
        MessageKind::Unreliable => to.send_unreliable(msg),
    }
    .expect("in-memory");
}

/// **A joiner whose first `Accept` was lost is sent the map again, and reads
/// it.** Its client hears nothing within its handshake timeout, says hello
/// again and drops the late first `Accept` as an old one — so the map the
/// host sealed at join no longer opens. The host accepts it again on the
/// same link, `welcome` sends the map again, and that copy is the one the
/// client reads.
#[test]
fn a_joiner_accepted_again_is_sent_the_map_again() {
    let map = another_map();
    let (_field, world, module) = crate::game::Field::open(&map, TICK_HZ);
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: usize::from(MAX_PLAYERS),
            tick_hz: TICK_HZ,
            compatibility: SESSION.compatibility,
        },
    );
    host.set_module(Box::new(module));
    let (near, client_side) = InMemoryTransport::pair();
    let (host_side, far) = InMemoryTransport::pair();
    host.add(Box::new(far));
    let mut client = Client::new_with_compatibility(
        World::new(),
        near,
        TICK_HZ,
        SESSION.compatibility,
        crcbl::net::PlayerId::from_seed(109),
    );
    let mut relay = Relay {
        client_side,
        host_side,
        lose_replies: 1,
    };
    let wire = event::map(&map.to_wire());
    let mut raised = Vec::new();
    let mut now = Duration::ZERO;
    let mut received = Vec::new();
    for _ in 0..MAX_FRAMES {
        now += FRAME;
        client.update(now);
        relay.carry();
        host.update(now);
        let events: Vec<PeerEvent> = host.events().collect();
        super::welcome(&mut host, &events, &wire, None);
        raised.extend(events);
        relay.carry();
        received.extend(client.events());
        if !received.is_empty() {
            break;
        }
    }
    assert_eq!(relay.lose_replies, 0, "the first Accept was not lost");
    assert!(
        raised
            .iter()
            .any(|event| matches!(event, PeerEvent::Reaccepted(_))),
        "the host never accepted the joiner again: {raised:?}"
    );
    let [bytes] = &received[..] else {
        panic!("not one map: {} events", received.len());
    };
    assert_eq!(event::decode(bytes).ok(), Some(Event::Map(map)));
}

/// **An event from the host this build cannot read is counted and passed
/// over**, joining or playing: one of another envelope version ahead of the
/// map, and one with a tag nobody knows after it, beside a refusal that is
/// read. The join still plays, and the game goes on.
#[test]
fn an_event_this_build_cannot_read_is_counted_and_passed_over() {
    let map = another_map();
    let mut bare = Bare::open(SESSION);
    let client = LanClient::join(SESSION, next_player(), bare.address(), TICK_HZ).expect("connect");
    let mut admitted = None;
    let mut joiner = step_bare(
        &mut bare,
        Joiner::through(client),
        "the game",
        |lan, peer| {
            admitted = Some(peer);
            let host = lan.host_mut();
            host.send_event(peer, vec![event::VERSION + 1, event::MAP_TAG])
                .expect("the joiner is connected");
            host.send_event(peer, event::map(&map.to_wire()))
                .expect("the joiner is connected");
        },
        |joiner| matches!(joiner, Joiner::Playing(_)),
    );
    assert_eq!(joiner.game().map(), &map);
    assert_eq!(joiner.game().ignored_events(), 1, "the join's count");

    let peer = admitted.expect("admitted");
    let host = bare.lan.host_mut();
    host.send_event(peer, vec![event::VERSION, 0xee])
        .expect("the joiner is connected");
    host.send_event(peer, event::refusal(Refusal::TopTier))
        .expect("the joiner is connected");
    let mut told = Vec::new();
    for _ in 0..MAX_FRAMES {
        if !told.is_empty() {
            break;
        }
        bare.frame();
        joiner = joiner.frame(FRAME);
        told.extend(joiner.game_mut().take_refusals());
        thread::sleep(PAUSE);
    }
    assert_eq!(told, [Refusal::TopTier]);
    assert_eq!(joiner.game().ignored_events(), 2);
    assert!(matches!(joiner, Joiner::Playing(_)), "{joiner:?}");
}

/// What each player of a host's rig has been told it was refused: the
/// host's own player first, then each joiner.
fn told(rig: &mut Rig<Game>, so_far: &mut [Vec<Refusal>]) {
    so_far[0].extend(rig.host.take_refusals());
    for (joiner, told) in rig.joiners.iter_mut().zip(&mut so_far[1..]) {
        told.extend(joiner.game_mut().take_refusals());
    }
}

/// **A refusal is told to the player who sent the command, and to no
/// other.** Joiner A builds on the first plot; joiner B asks for the same
/// plot and is told it is taken — A and the host's own player are told
/// nothing. Then the host's own player asks for a plot the map does not
/// have, and only it is told so.
#[test]
fn a_refusal_is_told_to_the_player_who_sent_it_and_no_other() {
    let mut rig = Rig::playing(2);
    let mut so_far = vec![Vec::new(); 3];
    rig.joiners[0]
        .game_mut()
        .set_controls(build(0, tower::Kind::Bolt));
    rig.until("the host building A's tower", |rig| {
        rig.host.stats().built == 1
    });
    rig.joiners[1]
        .game_mut()
        .set_controls(build(0, tower::Kind::Bolt));
    for _ in 0..MAX_FRAMES {
        if !so_far[2].is_empty() {
            break;
        }
        rig.step();
        told(&mut rig, &mut so_far);
    }
    // A few more frames, for a refusal sent to the wrong player to arrive.
    for _ in 0..SEEN_WITHIN {
        rig.step();
        told(&mut rig, &mut so_far);
    }
    assert_eq!(rig.host.stats().refused, 1);
    assert_eq!(so_far, [vec![], vec![], vec![Refusal::PlotTaken]]);

    rig.host.set_controls(build(200, tower::Kind::Bolt));
    for _ in 0..MAX_FRAMES {
        if !so_far[0].is_empty() {
            break;
        }
        rig.step();
        told(&mut rig, &mut so_far);
    }
    for _ in 0..SEEN_WITHIN {
        rig.step();
        told(&mut rig, &mut so_far);
    }
    assert_eq!(
        so_far,
        [vec![Refusal::NoSuchPlot], vec![], vec![Refusal::PlotTaken]]
    );
}

/// **A dedicated server tells a player what it refused them**, as a host
/// does: a wave sent while one is releasing is refused as there being no
/// wave to send.
#[test]
fn a_dedicated_server_tells_a_player_what_it_refused() {
    let mut rig = Rig::serving().with_playing(1);
    rig.joiners[0].game_mut().set_controls(send_wave());
    rig.until("the wave", |rig| rig.host.stats().wave == 1);
    rig.joiners[0].game_mut().set_controls(send_wave());
    let mut told = Vec::new();
    for _ in 0..MAX_FRAMES {
        if !told.is_empty() {
            break;
        }
        rig.step();
        told.extend(rig.joiners[0].game_mut().take_refusals());
    }
    assert_eq!(told, [Refusal::NoWaveToSend]);
}

/// **A host nobody answers ends the join as no answer**, once the join's
/// timeout has passed on its own clock — a socket that takes the hello and
/// says nothing, so no link reports anything first.
#[test]
fn a_host_nobody_answers_ends_the_join_as_no_answer() {
    let silent = std::net::UdpSocket::bind(loopback()).expect("loopback UDP");
    let address = silent.local_addr().expect("bound");
    let client = LanClient::join(SESSION, next_player(), address, TICK_HZ).expect("connect");
    let joiner = Joiner::through(client).frame(FRAME);
    assert!(matches!(joiner, Joiner::Joining(_)), "{joiner:?}");
    let Joiner::Failed(failure) = joiner.frame(JOIN_TIMEOUT) else {
        panic!("the join outlived its timeout");
    };
    assert!(
        matches!(failure, JoinFailure::NoAnswer { host, .. } if host == address),
        "{failure:?}"
    );
}

/// The plan `a_splash_and_a_slow_tower_hold_the_whole_table` wins with.
const PLAN: [tower::Kind; 5] = {
    use crate::tower::Kind::{Bolt, Slow, Splash};
    [Splash, Bolt, Bolt, Bolt, Slow]
};

/// Long enough for the whole table with every wave brought forward, in
/// frames, with a wide margin.
const TABLE_FRAMES: usize = 5 * 60 * 60;

/// What a player following `plan` asks for next, read off `game`'s field:
/// the plots in order, then each tower stepped up, then the next wave
/// brought forward the moment the table allows — never anything the purse
/// cannot pay for, so the field is as crowded as a winning team makes it.
///
/// A build or an upgrade on a plot this player does not `own` is left for
/// its owner — the team waits for it rather than skipping ahead — so a team
/// that splits the plots between its players takes the same steps in the
/// same order as one player who owns them all.
fn next_move(game: &Game, plan: &[tower::Kind], owns: impl Fn(usize) -> bool) -> Controls {
    let render = game.render_state();
    let stats = game.stats();
    for (plot, kind) in plan.iter().enumerate() {
        if render.towers[plot].is_none() {
            return if owns(plot) && stats.gold >= kind.spec(Tier::Base).cost {
                build(plot as u8, *kind)
            } else {
                Controls::default()
            };
        }
    }
    for (plot, tower) in render.towers.iter().enumerate() {
        if let Some(tower) = tower
            && tower.tier == Tier::Base
            && stats.gold >= tower.kind.spec(Tier::Upgraded).cost
        {
            return if owns(plot) {
                Controls {
                    upgrade: Some(plot as u8),
                    ..Controls::default()
                }
            } else {
                Controls::default()
            };
        }
    }
    if stats.next_wave_in.is_some() {
        send_wave()
    } else {
        Controls::default()
    }
}

/// **Towers' snapshot fits one datagram through the whole table, with
/// nothing held back.** Four players — the host's own and three joiners —
/// take turns playing `crate::game`'s winning plan on the committed field and
/// bring every wave forward the moment the table lets them, so the field is as
/// crowded as a team that wins gets it. Every snapshot the host sends is
/// fitted to [`MAX_UNRELIABLE_PAYLOAD`]; none may need to hold an update back
/// or be refused. The largest is printed, for `docs/backlog.md`.
#[test]
fn the_towers_snapshot_fits_one_datagram_through_the_table_with_nothing_held_back() {
    let mut rig = Rig::playing(usize::from(MAX_PLAYERS) - 1);
    assert_eq!(Map::built_in().plots().len(), PLAN.len());
    let (mut peak_creeps, mut peak_bolts) = (0, 0);
    for frame in 0..TABLE_FRAMES {
        let stats = rig.host.stats();
        if stats.outcome.is_over() {
            break;
        }
        // Each player takes a turn, so every link carries commands.
        let controls = next_move(&rig.host, &PLAN, |_| true);
        match frame % usize::from(MAX_PLAYERS) {
            0 => rig.host.set_controls(controls),
            joiner => rig.joiners[joiner - 1].game_mut().set_controls(controls),
        }
        rig.step();
        peak_creeps = peak_creeps.max(stats.creeps);
        peak_bolts = peak_bolts.max(stats.bolts);
    }

    let stats = rig.host.stats();
    assert_eq!(
        stats.outcome,
        crate::wave::Outcome::Won,
        "the team did not win: wave {}, {} leaks",
        stats.wave,
        stats.leaks
    );
    let host = rig.host.lan_host().expect("a host").host();
    let largest = host.largest_snapshot_bytes();
    println!(
        "towers over UDP: largest snapshot {largest} of {MAX_UNRELIABLE_PAYLOAD} bytes, \
         {peak_creeps} creeps and {peak_bolts} bolts at the peak, {} towers, {} players",
        stats.towers,
        rig.connected()
    );
    assert!(peak_creeps >= 10, "a crowded field: {peak_creeps} creeps");
    assert!(largest > 0, "snapshots were sent");
    assert!(largest <= MAX_UNRELIABLE_PAYLOAD);
    assert_eq!(host.held_back_update_count(), 0, "no update held back");
    assert_eq!(host.oversized_snapshot_count(), 0, "no snapshot refused");
    assert_eq!(host.processing_error_count(), 0);
    for joiner in &rig.joiners {
        let joiner = joiner.game();
        let client = joiner
            .lan_client()
            .and_then(LanClient::client)
            .expect("joined");
        assert_eq!(client.processing_error_count(), 0);
        assert_eq!(joiner.replicated().undecodable, 0);
    }
}

/// **A dedicated server with nobody in it sends no wave, and the first player
/// who finds it starts the run.** Well past the build phase with no player
/// in, the stage has not ticked and no wave has gone out, while the status
/// line says so on its interval and the announcement counts nobody — the
/// server has no player of its own. A browser then finds it, joins, and the
/// build phase runs down from where it stood; the status line counts the
/// player the moment they are in.
#[test]
fn a_dedicated_server_holds_its_run_until_a_player_who_found_it_joins() {
    let mut rig = Rig::serving();
    let due = rig
        .host
        .stats()
        .next_wave_in
        .expect("the build phase is running");
    let host = rig.host.lan_host();
    let announcer = host.announcer_addr().expect("the server announces");
    assert_eq!(
        host.announcement().map(|a| (a.players, a.max_players)),
        Some((0, MAX_PLAYERS)),
        "a dedicated server has no player of its own"
    );

    // Past the build phase and past two status intervals: a player in the
    // session would have seen the first wave long before. Nobody is on the
    // wire, so there is nothing to pause for.
    let empty = (2.0 * due).max(2.5 * STATUS_INTERVAL.as_secs_f64());
    for _ in 0..(empty * f64::from(TICK_HZ)).ceil() as usize {
        rig.host.frame(FRAME);
    }
    let stats = rig.host.stats();
    assert_eq!(stats.wave, 0, "a wave was sent at nobody");
    assert_eq!(stats.ticks, 0, "the stage ticked with nobody in it");
    assert_eq!(stats.next_wave_in, Some(due), "the build phase ran down");
    let waiting = &rig.host.printed;
    assert_eq!(
        waiting.len(),
        3,
        "a line at the start and one per interval: {waiting:?}"
    );
    for (_, line) in waiting {
        assert!(line.starts_with("towers: 0/4 players, wave 0/"), "{line}");
        assert!(line.ends_with("waiting for a player"), "{line}");
    }
    for pair in waiting.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            gap >= STATUS_INTERVAL && gap < STATUS_INTERVAL + FRAME,
            "{gap:?} between two status lines"
        );
    }

    rig.joiners.push(browsing(announcer));
    rig.until("the browsed session", |rig| playing(&rig.joiners[0]));
    let joined = rig.joiners[0].lan().and_then(LanClient::host);
    assert_eq!(joined, Some(rig.address()));
    let (_, printed) = rig.host.printed.last().expect("a line on the join");
    let line = printed.lines().next().unwrap_or_default();
    assert!(line.starts_with("towers: 1/4 players"), "{printed}");
    assert!(line.ends_with("playing"), "{printed}");
    rig.until("the build phase running down for the player", |rig| {
        rig.host.stats().next_wave_in.is_some_and(|next| next < due)
    });
    rig.until("the announcement counting the player", |rig| {
        rig.host
            .lan_host()
            .announcement()
            .is_some_and(|announcement| announcement.players == 1)
    });
}

/// **Four players win the whole table on a dedicated server** — milestone
/// 3's exit criterion, in one process: four joiners and no player of the
/// server's own, each taking turns to play [`PLAN`] off its **own** replica
/// of the field, bringing every wave forward as the table allows, until the
/// server's stage has been through every row and won. The plots are split
/// between the players, so every plot built is some one player's command
/// reaching the server; every player reads the win, and so does the status
/// line.
#[test]
fn four_players_win_the_whole_table_on_a_dedicated_server() {
    let mut rig = Rig::serving().with_playing(usize::from(MAX_PLAYERS));
    assert_eq!(
        rig.connected(),
        usize::from(MAX_PLAYERS),
        "four joiners, and the server holds no place of its own"
    );
    for frame in 0..TABLE_FRAMES {
        if rig.host.stats().outcome.is_over() {
            break;
        }
        let players = rig.joiners.len();
        let player = frame % players;
        let controls = next_move(rig.joiners[player].game(), &PLAN, |plot| {
            plot % players == player
        });
        rig.joiners[player].game_mut().set_controls(controls);
        rig.step();
    }

    let stats = rig.host.stats();
    assert_eq!(
        stats.outcome,
        crate::wave::Outcome::Won,
        "the team did not win: wave {}, {} leaks",
        stats.wave,
        stats.leaks
    );
    let waves = crate::wave::WAVES.len();
    assert_eq!(stats.wave, waves, "every row was played");
    assert_eq!(
        stats.towers,
        PLAN.len(),
        "every player's plots were built, so every player's commands counted"
    );
    rig.until("every player reading the win", |rig| {
        rig.joiners
            .iter()
            .all(|joiner| joiner.game().stats().outcome == crate::wave::Outcome::Won)
    });
    assert!(
        rig.host.printed.iter().any(|(_, printed)| {
            let line = printed.lines().next().unwrap_or_default();
            line.starts_with(&format!("towers: 4/4 players, wave {waves}/{waves}"))
                && line.ends_with("won")
        }),
        "no status line said the table was won"
    );
}

/// **`quit` at a dedicated server's console tells every player the server
/// shut down.** The serve loop reads a console that says `status`, a word
/// it does not know, a blank and `quit` — and a `status` after it, which it
/// never reads. It answers the first two, ends every session and returns
/// the last status line; each of the two players' clients then reads the
/// sealed session end, and its game says how the session ended.
#[test]
fn quit_at_the_console_tells_every_player_the_server_shut_down() {
    let mut rig = Rig::serving().with_playing(2);
    let (typed, lines) = std::sync::mpsc::channel();
    for line in ["status", "frobnicate", "", "quit", "status"] {
        typed.send(line.to_string()).expect("the console is open");
    }
    let mut printed = Vec::new();
    let now = rig.host.now + FRAME;
    let last = serve_until_quit(
        &mut rig.host.server,
        &mut Console::new(lines),
        FRAME,
        || now,
        &mut |line| printed.push(line.to_string()),
    )
    .expect("nothing to record, so nothing to fail");
    let [.., status, unknown] = &printed[..] else {
        panic!("the console answered too little: {printed:?}");
    };
    assert!(status.starts_with("towers: 2/4 players"), "{status}");
    assert_eq!(
        unknown,
        "towers: no command \"frobnicate\"; the commands are status, save [SLOT], load, \
         ban PLAYER [REASON], unban PLAYER, bans, quit"
    );
    assert!(last.starts_with("towers: 0/4 players"), "{last}");
    assert_eq!(last.lines().count(), 1, "a link listed with nobody in");
    assert_eq!(rig.connected(), 0, "a session outlived the quit");

    rig.until("every player told the server shut down", |rig| {
        rig.joiners
            .iter()
            .all(|joiner| joiner.game().session_end().as_deref() == Some("the server shut down"))
    });
    for joiner in &rig.joiners {
        let client = joiner.lan().and_then(LanClient::client).expect("joined");
        assert_eq!(
            client.ended(),
            Some(crcbl::client::Ended::ByServer(
                crcbl::net::SessionEndReason::SHUTTING_DOWN
            ))
        );
    }
}

/// **`quit` finishes a recording server's file**, whole: it reads back with
/// a state hash for the tick the server opened on and for every tick it
/// ran, the two players' joins and their frames, and the quit prints what it
/// holds. The players were told the server shut down before the file was
/// written, and a fresh host on the same map re-simulates it tick for tick.
#[test]
fn quit_at_the_console_finishes_a_recording_servers_file() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("served.crpl");
    let mut rig = Rig {
        host: Dedicated {
            server: Server::open(
                on_loopback(),
                &Map::built_in(),
                TICK_HZ,
                Some(&path),
                Vault::nowhere(),
            )
            .expect("loopback UDP must be available to these tests"),
            now: Duration::ZERO,
            printed: Vec::new(),
        },
        joiners: Vec::new(),
    }
    .with_playing(2);
    assert_eq!(rig.host.server.lan().recording(), Some(path.as_path()));
    rig.joiners[0]
        .game_mut()
        .set_controls(build(0, tower::Kind::Bolt));
    rig.until("the tower built", |rig| rig.host.stats().built == 1);
    let last = rig.host.server.lan().host().tick_id();

    let (typed, lines) = std::sync::mpsc::channel();
    typed.send("quit".to_string()).expect("the console is open");
    let mut printed = Vec::new();
    let now = rig.host.now + FRAME;
    serve_until_quit(
        &mut rig.host.server,
        &mut Console::new(lines),
        FRAME,
        || now,
        &mut |line| printed.push(line.to_string()),
    )
    .expect("the recording finishes");
    assert_eq!(
        rig.host.server.lan().recording(),
        None,
        "the quit stopped it"
    );
    let recorded = printed
        .iter()
        .find(|line| line.starts_with("towers: recorded ticks 0 to "))
        .unwrap_or_else(|| panic!("the quit printed no recording: {printed:?}"));
    assert!(recorded.contains("served.crpl"), "{recorded}");

    let storage = crcbl::store::NativeStorage::at(dir.path().to_path_buf());
    let file =
        crcbl::store::replay::FileTransport::open(&storage, std::path::Path::new("served.crpl"))
            .expect("the file reads back whole");
    let hashes = file.state_hashes();
    assert_eq!(hashes.first().map(|hash| hash.tick.get()), Some(0));
    // The quit's own frame ran one more tick.
    let end = hashes.last().expect("hashed").tick;
    assert!(end > last, "{} after {}", end.get(), last.get());
    let joins = file
        .peer_ticks()
        .iter()
        .flat_map(|tick| &tick.roster)
        .filter(|change| change.kind == crcbl::store::replay::RosterChangeKind::Joined)
        .count();
    assert_eq!(joins, 2);
    assert!(
        file.peer_ticks().iter().any(|tick| !tick.peers.is_empty()),
        "the players' frames are in it"
    );

    let (_, world, module) = crate::game::Field::open(&Map::built_in(), TICK_HZ);
    let mut replayed = Host::new(
        world,
        HostConfig {
            max_peers: usize::from(MAX_PLAYERS),
            tick_hz: TICK_HZ,
            compatibility: SESSION.compatibility,
        },
    );
    replayed.set_module(Box::new(module));
    replayed.update(Duration::ZERO);
    assert_eq!(
        crcbl::replay_record::resimulate(&mut replayed, &file),
        Ok(end)
    );
}

/// **A playing host's recording is finished when its game is dropped** — a
/// window closing — with its own player's input in it beside a joiner's, and
/// a fresh host on the same map re-simulates it tick for tick.
#[test]
fn a_hosts_recording_is_finished_when_its_game_is_dropped() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("hosted.crpl");
    let mut rig = Rig {
        host: Game::host(
            TICK_HZ,
            &Map::built_in(),
            on_loopback(),
            Some(&path),
            next_player(),
        )
        .expect("loopback UDP must be available to these tests"),
        joiners: Vec::new(),
    }
    .with_playing(1);
    rig.host.set_controls(build(1, tower::Kind::Splash));
    rig.joiners[0]
        .game_mut()
        .set_controls(build(0, tower::Kind::Bolt));
    rig.until("both towers built", |rig| rig.host.stats().built == 2);
    let last = rig.host.lan_host().expect("a host").host().tick_id();
    drop(rig);

    let storage = crcbl::store::NativeStorage::at(dir.path().to_path_buf());
    let file =
        crcbl::store::replay::FileTransport::open(&storage, std::path::Path::new("hosted.crpl"))
            .expect("the file reads back whole");
    assert_eq!(file.state_hashes().last().map(|hash| hash.tick), Some(last));
    let (_, world, module) = crate::game::Field::open(&Map::built_in(), TICK_HZ);
    let mut replayed = Host::new(
        world,
        HostConfig {
            max_peers: usize::from(MAX_PLAYERS),
            tick_hz: TICK_HZ,
            compatibility: SESSION.compatibility,
        },
    );
    replayed.set_module(Box::new(module));
    replayed.update(Duration::ZERO);
    assert_eq!(
        crcbl::replay_record::resimulate(&mut replayed, &file),
        Ok(last)
    );
}

/// **A console whose input ended is not a quit**: the server serves on,
/// with nothing printed, and says so in the log only once.
#[test]
fn a_console_whose_input_ended_is_not_a_quit() {
    let mut rig = Rig::serving().with_playing(1);
    let (typed, lines) = std::sync::mpsc::channel::<String>();
    drop(typed);
    let mut console = Console::new(lines);
    let mut printed = Vec::new();
    for _ in 0..3 {
        assert_eq!(
            console.obey(&mut rig.host.server, &mut |line| printed
                .push(line.to_string())),
            Next::Serve
        );
    }
    assert_eq!(printed, Vec::<String>::new());
    rig.step();
    assert_eq!(rig.connected(), 1, "the player's session ended");
}

/// **A dedicated server's status line lists each player's link under it**:
/// `  peer N (PLAYER): rtt R ms, loss L%, in I B/s, out O B/s`, a line per
/// player in
/// the host's order, each with the figures the host's netgraph recorded for
/// that peer — and the line printed when the second player came in already
/// carries both.
#[test]
fn a_dedicated_servers_status_line_lists_each_players_link() {
    let mut rig = Rig::serving().with_playing(2);
    let (_, joined) = rig.host.printed.last().expect("a line on the join");
    assert_eq!(joined.lines().count(), 3, "{joined}");
    rig.until("both links measured", |rig| {
        let links = rig.host.lan_host().netgraph().links();
        links.len() == 2
            && links.iter().all(|link| {
                link.reading
                    .stats
                    .is_some_and(|stats| stats.rtt.is_some() && stats.recent.loss().is_some())
            })
    });

    let status = rig.host.server.status();
    let lines: Vec<&str> = status.lines().collect();
    assert!(lines[0].starts_with("towers: 2/4 players"), "{status}");
    let host = rig.host.lan_host();
    let peers: Vec<u64> = host.host().peers().map(|peer| peer.get()).collect();
    assert_eq!(lines.len(), 1 + peers.len(), "{status}");
    for ((line, link), peer) in lines[1..].iter().zip(host.netgraph().links()).zip(peers) {
        assert_eq!(link.id, peer, "in the host's order");
        let player = host
            .host()
            .player(crcbl::server::PeerId::from_raw(peer))
            .expect("in session");
        let stats = link.reading.stats.expect("measured");
        let rtt = stats.rtt.expect("measured").as_secs_f64() * 1_000.0;
        let loss = stats.recent.loss().expect("judged") * 100.0;
        assert_eq!(
            *line,
            format!(
                "  peer {peer} ({player}): rtt {rtt:.1} ms, loss {loss:.1}%, in {} B/s, out {} B/s",
                stats.recent.received_per_second(),
                stats.recent.sent_per_second(),
            )
        );
    }
}

/// **A player leaving mid-run does not stop a dedicated server.** A wave is
/// running when one of two players' link closes; the server sees them go,
/// and the other goes on playing — its commands still validated, its field
/// still moving.
#[test]
fn a_player_leaving_mid_run_does_not_stop_a_dedicated_server() {
    let mut rig = Rig::serving().with_playing(2);
    rig.joiners[0].game_mut().set_controls(send_wave());
    rig.until("the wave the first player sent", |rig| {
        rig.host.stats().wave == 1
    });

    drop(rig.joiners.remove(0));
    rig.until("the server seeing the player go", |rig| {
        rig.connected() == 1
    });

    let ticks = rig.joiners[0].game().stats().ticks;
    rig.joiners[0]
        .game_mut()
        .set_controls(build(1, tower::Kind::Bolt));
    rig.until("the server building the other's tower", |rig| {
        rig.host.stats().built == 1
    });
    rig.until("the other drawing it, on a field still moving", |rig| {
        rig.joiners[0].game().render_state().towers[1].is_some()
            && rig.joiners[0].game().stats().ticks > ticks
    });
}

/// A dedicated server on loopback saving in the scratch directory `dir`, and
/// recording to `record` if it names a file.
fn saving_server(dir: &std::path::Path, record: Option<&std::path::Path>) -> Server {
    Server::open(
        on_loopback(),
        &Map::built_in(),
        TICK_HZ,
        record,
        Vault::at(dir.to_path_buf(), crate::save::SERVER_FILE),
    )
    .expect("loopback UDP must be available to these tests")
}

/// Types `lines` at `server`'s console and answers what it printed.
fn typed_at(server: &mut Server, lines: &[&str]) -> Vec<String> {
    let (typed, read) = std::sync::mpsc::channel();
    for line in lines {
        typed
            .send((*line).to_string())
            .expect("the console is open");
    }
    let mut printed = Vec::new();
    assert_eq!(
        Console::new(read).obey(server, &mut |line| printed.push(line.to_string())),
        Next::Serve
    );
    printed
}

/// **`ban` at a dedicated server's console removes a player and refuses them
/// with the reason, `bans` lists it, the list outlives the server, and
/// `unban` lets the player back.** The player is named by the id the status
/// line prints, and comes back as the same id — a restarted client.
#[test]
fn a_dedicated_server_bans_and_unbans_a_player_at_its_console() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bans = dir.path().join(BANS_FILE);
    let mut rig = Rig::serving();
    assert_eq!(rig.host.server.keep_bans(&bans).expect("no file yet"), 0);
    let mut rig = rig.with_playing(1);
    let player = rig.joiners[0]
        .lan()
        .and_then(LanClient::client)
        .map(Client::player)
        .expect("the joiner has a client");
    assert!(
        rig.host.server.status().contains(&player.to_string()),
        "the status line names the player"
    );

    let printed = typed_at(&mut rig.host.server, &[&format!("ban {player} griefing")]);
    assert!(
        printed[0].starts_with(&format!("towers: banned {player}, and kicked peer ")),
        "{printed:?}"
    );
    rig.until("the banned player out", |rig| rig.connected() == 0);

    let address = rig.address();
    let refused = |rig: &mut Rig<Dedicated>| {
        let client = LanClient::join(SESSION, player, address, TICK_HZ).expect("connect");
        rig.joiners = vec![Joiner::through(client)];
        rig.until("the join refused", |rig| rig.joiners[0].failure().is_some());
        rig.joiners[0].failure().map(ToString::to_string)
    };
    let failure = refused(&mut rig).expect("refused");
    assert!(
        failure.ends_with("refused the join: banned from this server: griefing"),
        "{failure}"
    );

    let listed = typed_at(&mut rig.host.server, &["bans"]);
    assert_eq!(listed, [format!("towers: 1 banned\n  {player} griefing")]);
    let mut restarted = saving_server(dir.path(), None);
    assert_eq!(restarted.keep_bans(&bans).expect("the file reads"), 1);
    assert_eq!(
        typed_at(&mut restarted, &["bans"]),
        listed,
        "the list outlived its server"
    );

    let printed = typed_at(&mut rig.host.server, &[&format!("unban {player}")]);
    assert_eq!(printed, [format!("towers: unbanned {player}")]);
    let client = LanClient::join(SESSION, player, address, TICK_HZ).expect("connect");
    rig.joiners = vec![Joiner::through(client)];
    rig.until("the unbanned player back", |rig| {
        rig.joiners.iter().all(playing)
    });
    assert_eq!(
        crcbl::server::Denylist::parse(&std::fs::read_to_string(&bans).expect("kept"))
            .expect("reads")
            .len(),
        0,
        "the unban was written"
    );
}

/// **`save` and `load` at a dedicated server's console keep the run and put
/// it back.** `load` with nothing saved says so; `save` writes the run in
/// the server's own file; a run saved at a wave's end, put in that file, is
/// what `load` then serves — and an empty server holds it, rather than
/// throwing it away as it does a run its last player left.
#[test]
fn a_dedicated_server_saves_and_loads_its_run_at_the_console() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut server = saving_server(dir.path(), None);
    server.frame(FRAME);

    let printed = typed_at(&mut server, &["load"]);
    assert_eq!(
        printed,
        ["towers: not loaded: there is no save to resume"],
        "{printed:?}"
    );

    let printed = typed_at(&mut server, &["save"]);
    assert!(
        printed[0].starts_with("towers: saved wave 0/"),
        "{printed:?}"
    );
    let file = dir.path().join(crate::save::SERVER_FILE);
    assert!(file.is_file(), "the save is not in the server's file");

    let saved = crate::save::tests::a_first_waves_end();
    Vault::at(dir.path().to_path_buf(), crate::save::SERVER_FILE)
        .store(&saved)
        .expect("the scratch directory is writable");
    let printed = typed_at(&mut server, &["load"]);
    assert!(
        printed[0].starts_with(&format!("towers: loaded wave {}/", saved.wave())),
        "{printed:?}"
    );
    let stats = server.stats();
    assert_eq!(
        (stats.wave, stats.gold, stats.lives),
        (saved.wave(), saved.gold(), saved.lives())
    );

    // Nobody is in it: a run a player left would be reset on the next tick,
    // and a loaded one waits.
    for frame in 2..60 {
        server.frame(FRAME * frame);
    }
    assert_eq!(
        server.stats().wave,
        saved.wave(),
        "an empty server threw the loaded run away"
    );
}

/// **`save slot2` at a dedicated server's console goes through the server's
/// one save path into the slot's own file**, beside the server's file and
/// never over it, and the save resumes; the desk records the server's console
/// as what asked.
#[test]
fn a_dedicated_servers_save_takes_a_slot_through_its_one_path() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut server = saving_server(dir.path(), None);
    server.frame(FRAME);

    let printed = typed_at(&mut server, &["save slot2"]);
    assert!(
        printed[0].starts_with("towers: saved wave 0/"),
        "{printed:?}"
    );
    assert!(
        !dir.path().join(crate::save::SERVER_FILE).exists(),
        "the slot's save went to the server's own file"
    );
    let slot = Vault::at(dir.path().to_path_buf(), "towers-server-slot2.crb")
        .load(&Map::built_in())
        .expect("the slot's save reads back");
    assert_eq!(slot.map(|saved| saved.wave()), Some(0));
    assert_eq!(
        server.desk().last_trigger(),
        Some(crcbl::save::SaveTrigger::ServerConsole)
    );

    let printed = typed_at(&mut server, &["save ../elsewhere"]);
    assert!(
        printed[0].starts_with("towers: `../elsewhere` is not a slot name"),
        "{printed:?}"
    );
}

/// **A recorded session refuses `load`**, by name: the recording is
/// re-simulated from a fresh run, and could not reproduce one swapped in
/// under it.
#[test]
fn a_recording_server_refuses_to_load() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let record = dir.path().join("served.crpl");
    let mut server = saving_server(dir.path(), Some(&record));
    server.frame(FRAME);
    let printed = typed_at(&mut server, &["save", "load"]);
    assert!(
        printed[0].starts_with("towers: saved wave 0/"),
        "{printed:?}"
    );
    assert!(
        printed[1].starts_with("towers: not loaded: the session is being recorded"),
        "{printed:?}"
    );
}

/// What a joiner draws and hears of the host's field.
mod presentation;
