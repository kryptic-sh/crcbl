//! A dedicated server: `towers --serve [PORT]`.
//!
//! ```text
//!  wall clock ─▶ Server::frame ─▶ LanHost ─▶ Host ─▶ TowersModule ─▶ Stage
//!                    │                        ├─ UDP ─ a player
//!                    └─▶ the status line      └─ UDP ─ another
//! ```
//!
//! The same authoritative stage a `--host` runs, on the same
//! [`crcbl::lan::LanHost`], announced on the LAN the same way — and nothing
//! else: **no window, no renderer and no player of its own.** Every player is
//! a joiner, found by `--browse` or reached with `--join`, so the four places
//! [`super::MAX_PLAYERS`] allows are all theirs.
//!
//! # Why not the loop, and why not `--headless`
//!
//! Towers' [`Loop`](crate::Loop) opens a shell and a GPU before the game, and
//! `--headless` is the engine's name for a run on an offscreen ring with a
//! stepped clock and a frame budget — reproducible frames for CI, which is the
//! opposite of a server that keeps wall time until it is killed. So this is
//! its own entry point with its own flag: [`serve`] ticks the host on
//! [`Instant`] and sleeps to the next tick boundary, and the flags that only
//! mean something to a window or a renderer are refused beside it by
//! `crate::args`.
//!
//! # Nobody in it, nothing running
//!
//! With no player in the session the run holds still — no build phase running
//! out, no wave sent at an empty field — and the first joiner's tick starts it
//! where it stopped. That rule is `crate::game`'s `run_team_tick`, which sees
//! a tick with no command frame at all; a player whose link dropped still
//! holds a place through its grace period, so the run goes on while they
//! reconnect.
//!
//! # Stopping
//!
//! It runs until the process is killed. The workspace has no signal handling
//! to hang a clean shutdown on — no Ctrl+C hook tells the players
//! `SHUTTING_DOWN` before the sockets close — so a joiner sees its link time
//! out instead; `docs/backlog.md` records it.

use std::convert::Infallible;
use std::thread;
use std::time::{Duration, Instant};

use crcbl::core::FrameClock;
use crcbl::lan::{LanBind, LanHost};

use super::{APP, MAX_PLAYERS, SESSION, welcome};
use crate::game::{Field, GameError, Stats};
use crate::map::Map;
use crate::wave::{Outcome, WAVES};

/// The longest a running server goes without printing its status line. A
/// change of players, wave or outcome prints one at once.
pub(crate) const STATUS_INTERVAL: Duration = Duration::from_secs(10);

/// What the status line leads with; a change in any of it prints the line at
/// once rather than at the next [`STATUS_INTERVAL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Headline {
    players: usize,
    wave: usize,
    outcome: Outcome,
    runs: u64,
}

/// A towers session served with no player of its own.
pub(crate) struct Server {
    lan: LanHost,
    field: Field,
    /// The map every player is sent as they join, encoded once.
    map: Vec<u8>,
    /// What the last status line said, and when the next is due regardless.
    printed: Option<Headline>,
    next_status: Duration,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("lan", &self.lan)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Serves a new run on `map`, bound where `bind` says, ticking at
    /// `tick_hz`. Every player is sent `map` as they join.
    ///
    /// # Errors
    ///
    /// [`GameError::Lan`] if the listener would not bind.
    pub fn open(bind: LanBind, map: &Map, tick_hz: u32) -> Result<Self, GameError> {
        let (field, world, module) = Field::open(map, tick_hz);
        let mut lan = LanHost::open(SESSION, bind, world, tick_hz).map_err(GameError::Lan)?;
        lan.host_mut().set_module(Box::new(module));
        Ok(Self {
            lan,
            field,
            map: map.to_wire(),
            printed: None,
            next_status: Duration::ZERO,
        })
    }

    /// Runs the server to `now` — the time since it opened — and answers the
    /// status line when it is due to be printed: when it has news, or has
    /// been quiet for [`STATUS_INTERVAL`].
    pub fn frame(&mut self, now: Duration) -> Option<String> {
        let events = self.lan.frame(now);
        welcome(&mut self.lan, &events, &self.map, None);
        let headline = self.headline();
        if self.printed == Some(headline) && now < self.next_status {
            return None;
        }
        self.printed = Some(headline);
        self.next_status = now + STATUS_INTERVAL;
        Some(self.status())
    }

    /// The engine's LAN host, for the tests that join it.
    #[cfg(test)]
    pub const fn lan(&self) -> &LanHost {
        &self.lan
    }

    /// How many players hold a place in the session, one whose link dropped
    /// among them until its grace period runs out — the count the run goes
    /// on for, and the one the announcement carries.
    pub fn players(&self) -> usize {
        self.lan.host().peer_count()
    }

    /// The stage's numbers.
    pub fn stats(&self) -> Stats {
        self.field.stats()
    }

    /// The status line: who is in, and how the run stands.
    pub fn status(&self) -> String {
        status_line(self.players(), &self.stats())
    }

    fn headline(&self) -> Headline {
        let stats = self.stats();
        Headline {
            players: self.players(),
            wave: stats.wave,
            outcome: stats.outcome,
            runs: stats.runs,
        }
    }
}

/// The status line for `players` in a run standing at `stats`.
fn status_line(players: usize, stats: &Stats) -> String {
    let state = if players == 0 {
        "waiting for a player"
    } else {
        stats.outcome.label()
    };
    format!(
        "{}: {players}/{} players, wave {}/{}, {} lives, {} gold, run {}, {state}",
        APP,
        MAX_PLAYERS,
        stats.wave,
        WAVES.len(),
        stats.lives,
        stats.gold,
        stats.runs,
    )
}

/// Serves `map` on UDP `port` (0 for any free one), announced on the LAN, at
/// `tick_hz` on the wall clock — until the process is killed, so it answers
/// only if the server could not start.
///
/// # Errors
///
/// [`GameError::Lan`] if the listener would not bind.
pub(crate) fn serve(port: u16, map: &Map, tick_hz: u32) -> Result<Infallible, GameError> {
    let mut server = Server::open(LanBind::on_the_lan(port), map, tick_hz)?;
    let tick = FrameClock::new(tick_hz).tick_dt();
    let started = Instant::now();
    loop {
        if let Some(status) = server.frame(started.elapsed()) {
            println!("{status}");
        }
        thread::sleep(until_next_tick(started.elapsed(), tick));
    }
}

/// How long from `elapsed` to the next whole `tick`: sleeping to the boundary
/// rather than for a whole tick keeps the ticks on the wall clock's grid
/// however long a frame's work took. A frame that overran a tick is caught up
/// by the host's own clock, which runs every tick that came due.
fn until_next_tick(elapsed: Duration, tick: Duration) -> Duration {
    let tick_ns = tick.as_nanos();
    let into_tick = elapsed.as_nanos() % tick_ns;
    Duration::from_nanos(
        u64::try_from(tick_ns - into_tick).expect("a tick at a positive rate is under a second"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The sleep lands on the next tick boundary**, from anywhere inside a
    /// tick — including exactly on one, which waits a whole tick rather than
    /// none and spinning.
    #[test]
    fn the_sleep_lands_on_the_next_tick_boundary() {
        let tick = Duration::from_millis(16);
        assert_eq!(until_next_tick(Duration::ZERO, tick), tick);
        assert_eq!(
            until_next_tick(Duration::from_millis(5), tick),
            Duration::from_millis(11)
        );
        assert_eq!(
            until_next_tick(Duration::from_millis(16 * 7 + 15), tick),
            Duration::from_millis(1)
        );
        assert_eq!(until_next_tick(Duration::from_millis(32), tick), tick);
    }

    /// **The status line says who is in and how the run stands**, and an
    /// empty session says it is waiting rather than "playing" a run that is
    /// holding still.
    #[test]
    fn the_status_line_names_the_players_the_wave_the_lives_and_the_gold() {
        let stats = Stats {
            wave: 3,
            lives: 17,
            gold: 240,
            runs: 2,
            ..Stats::default()
        };
        assert_eq!(
            status_line(2, &stats),
            format!(
                "towers: 2/4 players, wave 3/{}, 17 lives, 240 gold, run 2, playing",
                WAVES.len()
            )
        );
        assert!(
            status_line(0, &stats).ends_with("waiting for a player"),
            "{}",
            status_line(0, &stats)
        );
    }
}
