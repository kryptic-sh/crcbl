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

use crcbl::lan::{LanBind, LanClient, LanHost};
use crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD;
use crcbl::net::udp::discovery::{Browser, BrowserConfig};
use crcbl::net::{SessionState, SystemClock};

use super::serve::{STATUS_INTERVAL, Server};
use super::{LAN, MAX_PLAYERS};
use crate::game::{Controls, Game, Stats};
use crate::map::Map;
use crate::tower::{self, Tier};

const TICK_HZ: u32 = crate::game::DEFAULT_TICK_HZ;

/// One frame at [`TICK_HZ`]: every step runs one tick of each game and about
/// one of the host's server.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);

/// The most frames any wait here runs: ten seconds of game time, and a few
/// seconds of wall time at [`PAUSE`] a step.
const MAX_FRAMES: usize = 600;

/// The pause after each step, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// How many ticks a command one joiner sends may take to reach another's
/// screen: its way to the host, the host's tick, and the snapshot's way back
/// — a few ticks on loopback, bounded well above that.
const SEEN_WITHIN: usize = 30;

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// Where a host binds on loopback: announcing on loopback, broadcasting
/// nowhere.
fn on_loopback() -> LanBind {
    LanBind {
        listen: loopback(),
        announce_at: loopback(),
        broadcast_to: None,
    }
}

/// A host on loopback, with a player of its own.
fn host() -> Game {
    Game::host(TICK_HZ, &Map::built_in(), on_loopback())
        .expect("loopback UDP must be available to these tests")
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
        server: Server::open(on_loopback(), &Map::built_in(), TICK_HZ)
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
    joiners: Vec<Game>,
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
        let client = LanClient::join(LAN, self.address(), TICK_HZ).expect("connect");
        self.joiners
            .push(Game::join(TICK_HZ, &Map::built_in(), client));
    }

    /// Every player's tick, then the host's frame and every joiner's, then
    /// the pause.
    fn step(&mut self) {
        self.host.tick();
        for game in &mut self.joiners {
            game.tick();
        }
        self.host.frame(FRAME);
        for game in &mut self.joiners {
            game.frame(FRAME);
        }
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

/// Whether a joiner is in a session and has the host's numbers.
fn playing(game: &Game) -> bool {
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

    rig.joiners[0].set_controls(build(0, tower::Kind::Splash));
    rig.until("the host building joiner A's tower", |rig| {
        rig.host.stats().built == 1
    });
    assert_eq!(rig.host.stats().gold, purse - cost, "out of the one purse");
    let seen = rig.until("joiner B drawing it", |rig| {
        rig.joiners[1].render_state().towers[0]
            .is_some_and(|tower| tower.kind == tower::Kind::Splash && tower.tier == Tier::Base)
    });
    assert!(
        seen <= SEEN_WITHIN,
        "B saw A's tower {seen} ticks after it was built"
    );
    rig.until("joiner B reading the same purse", |rig| {
        rig.joiners[1].stats().gold == purse - cost
    });

    let refused = rig.host.stats().refused;
    rig.joiners[1].set_controls(build(0, tower::Kind::Bolt));
    rig.until("the host refusing joiner B's build", |rig| {
        rig.host.stats().refused == refused + 1
    });
    assert_eq!(rig.host.stats().built, 1, "the plot was taken");
    rig.until("joiner A reading the refusal", |rig| {
        rig.joiners[0].stats().refused == refused + 1
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
    rig.joiners[1].set_controls(send_wave());
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
            && rig
                .joiners
                .iter()
                .all(|game| game.stats().wave == 1 && game.render_state().creeps_alive > 0)
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

    let ticks = rig.joiners[0].stats().ticks;
    rig.joiners[0].set_controls(build(1, tower::Kind::Bolt));
    rig.until("the host building B's tower", |rig| {
        rig.host.stats().built == 1
    });
    rig.until("B drawing it, on a field still moving", |rig| {
        rig.joiners[0].render_state().towers[1].is_some() && rig.joiners[0].stats().ticks > ticks
    });
}

/// A player browsing for a host, its query sent straight to `announcer`.
fn browsing(announcer: SocketAddr) -> Game {
    let browser = Browser::bind_with(
        loopback(),
        BrowserConfig {
            query_to: Some(announcer),
            ..BrowserConfig::new(LAN.protocol_id)
        },
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    Game::join(
        TICK_HZ,
        &Map::built_in(),
        LanClient::browse(LAN, browser, TICK_HZ),
    )
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
    let joined = rig.joiners[0].lan_client().and_then(LanClient::host);
    assert_eq!(joined, Some(rig.address()));
    rig.until("the announcement counting both players", |rig| {
        rig.host
            .lan_host()
            .and_then(crcbl::lan::LanHost::announcement)
            .is_some_and(|announcement| announcement.players == 2)
    });
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
            joiner => rig.joiners[joiner - 1].set_controls(controls),
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
    let joined = rig.joiners[0].lan_client().and_then(LanClient::host);
    assert_eq!(joined, Some(rig.address()));
    let (_, line) = rig.host.printed.last().expect("a line on the join");
    assert!(line.starts_with("towers: 1/4 players"), "{line}");
    assert!(line.ends_with("playing"), "{line}");
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
        let controls = next_move(&rig.joiners[player], &PLAN, |plot| plot % players == player);
        rig.joiners[player].set_controls(controls);
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
            .all(|game| game.stats().outcome == crate::wave::Outcome::Won)
    });
    assert!(
        rig.host.printed.iter().any(|(_, line)| {
            line.starts_with(&format!("towers: 4/4 players, wave {waves}/{waves}"))
                && line.ends_with("won")
        }),
        "no status line said the table was won"
    );
}

/// **A player leaving mid-run does not stop a dedicated server.** A wave is
/// running when one of two players' link closes; the server sees them go,
/// and the other goes on playing — its commands still validated, its field
/// still moving.
#[test]
fn a_player_leaving_mid_run_does_not_stop_a_dedicated_server() {
    let mut rig = Rig::serving().with_playing(2);
    rig.joiners[0].set_controls(send_wave());
    rig.until("the wave the first player sent", |rig| {
        rig.host.stats().wave == 1
    });

    drop(rig.joiners.remove(0));
    rig.until("the server seeing the player go", |rig| {
        rig.connected() == 1
    });

    let ticks = rig.joiners[0].stats().ticks;
    rig.joiners[0].set_controls(build(1, tower::Kind::Bolt));
    rig.until("the server building the other's tower", |rig| {
        rig.host.stats().built == 1
    });
    rig.until("the other drawing it, on a field still moving", |rig| {
        rig.joiners[0].render_state().towers[1].is_some() && rig.joiners[0].stats().ticks > ticks
    });
}
