//! A dedicated server: `towers --serve [PORT]`.
//!
//! ```text
//!  wall clock ─▶ Server::frame ─▶ LanHost ─▶ Host ─▶ TowersModule ─▶ Stage
//!                    │                        ├─ UDP ─ a player
//!                    └─▶ the status line      └─ UDP ─ another
//!  stdin ─▶ reader thread ─▶ channel ─▶ Console::obey, between frames
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
//! opposite of a server that keeps wall time until it is told to stop. So
//! this is its own entry point with its own flag: [`serve`] ticks the host on
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
//! # Stopping: a console on stdin
//!
//! The server reads its stdin as a console — the dedicated-server norm, and
//! `std` alone, so it behaves the same on every OS. A read blocks, so a
//! thread of its own reads the lines and hands them over a channel, which
//! the loop drains between frames ([`Console::obey`]). `quit` ends every
//! session with `SessionEndReason::SHUTTING_DOWN` (`Host::shutdown`), so
//! each player is told before the sockets close, and [`serve`] answers the
//! last status line; `status` prints the status line now; anything else
//! prints the commands there are.
//!
//! # Recording: `--record <FILE>`
//!
//! A server asked to record starts its [`LanHost`]'s recording before its
//! first frame, so the file opens on a tick no player was in yet and a host
//! built on the same map re-simulates it from there. `quit` finishes the file
//! after every session ends and prints what it holds; a recording that does
//! not finish whole is the run's error. Ctrl+C leaves the file empty, with its
//! spool beside it, as it leaves the players without a goodbye.
//!
//! **Stdin closing is not a quit.** A server started with no console — its
//! input at its end from the start, under a service manager say — keeps
//! serving, and only the reader thread ends. Ctrl+C still kills it without
//! the goodbye, and the players see their links time out: the workspace has
//! no signal hook, and the console is what was chosen instead of one.

use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use std::path::Path;

use crcbl::core::FrameClock;
use crcbl::lan::{LanBind, LanError, LanHost};
use crcbl::net::SessionEndReason;
use crcbl::replay_record::{RecordError, RecordSummary};

use super::{APP, MAX_PLAYERS, SESSION, event, tell, welcome};
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
    /// The map every player is sent as they join, as its event, encoded
    /// once.
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
    /// `tick_hz`, recording it to the new file `record` names, if it names
    /// one. Every player is sent `map` as they join, and the refusals of
    /// their commands.
    ///
    /// # Errors
    ///
    /// [`GameError::Lan`] if the listener would not bind or the recording
    /// would not start.
    pub fn open(
        bind: LanBind,
        map: &Map,
        tick_hz: u32,
        record: Option<&Path>,
    ) -> Result<Self, GameError> {
        let (field, world, module) = Field::open(map, tick_hz);
        let mut lan = LanHost::open(SESSION, bind, world, tick_hz).map_err(GameError::Lan)?;
        lan.host_mut().set_module(Box::new(module));
        if let Some(path) = record {
            lan.record(path).map_err(GameError::Lan)?;
        }
        Ok(Self {
            lan,
            field,
            map: event::map(&map.to_wire()),
            printed: None,
            next_status: Duration::ZERO,
        })
    }

    /// Runs the server to `now` — the time since it opened — and answers the
    /// status line when it is due to be printed: when it has news, or has
    /// been quiet for [`STATUS_INTERVAL`].
    pub fn frame(&mut self, now: Duration) -> Option<String> {
        let events = self.lan.frame(now);
        welcome(self.lan.host_mut(), &events, &self.map, None);
        tell(self.lan.host_mut(), &self.field.take_refusals());
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

    /// The engine's LAN host, for the tests that end its sessions.
    #[cfg(test)]
    pub const fn lan_mut(&mut self) -> &mut LanHost {
        &mut self.lan
    }

    /// Ends every session, each player told the server is shutting down
    /// before its link closes. The host itself carries on, empty.
    pub fn shutdown(&mut self) {
        self.lan
            .host_mut()
            .shutdown(SessionEndReason::SHUTTING_DOWN);
    }

    /// Stops recording and finishes the file, if the server records — see
    /// [`LanHost::stop_recording`].
    pub fn stop_recording(&mut self) -> Option<Result<RecordSummary, RecordError>> {
        self.lan.stop_recording()
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

/// What the console's help line names.
const COMMANDS: &str = "status, quit";

/// A line typed at the console, read.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// End every session and stop.
    Quit,
    /// Print the status line now.
    Status,
    /// Nothing but blanks: nothing to answer.
    Blank,
    /// Anything else, trimmed.
    Unknown(String),
}

impl Command {
    /// The command `line` is, ignoring the blanks around it and the case.
    fn parse(line: &str) -> Self {
        let word = line.trim();
        if word.is_empty() {
            Self::Blank
        } else if word.eq_ignore_ascii_case("quit") {
            Self::Quit
        } else if word.eq_ignore_ascii_case("status") {
            Self::Status
        } else {
            Self::Unknown(word.to_string())
        }
    }
}

/// Whether the serve loop goes on after reading the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Serve another frame.
    Serve,
    /// `quit` was typed.
    Quit,
}

/// The console's lines as the serve loop reads them: whatever sends them —
/// stdin's reader thread, or a test.
pub(crate) struct Console {
    lines: Receiver<String>,
    /// Whether the sender is gone — stdin at its end — which is logged once.
    closed: bool,
}

impl Console {
    /// A console reading `lines`.
    pub(crate) const fn new(lines: Receiver<String>) -> Self {
        Self {
            lines,
            closed: false,
        }
    }

    /// A console on this process's stdin: a thread reads it a line at a
    /// time into the channel, and ends when stdin does or a read fails —
    /// either logged, neither a quit.
    fn on_stdin() -> Self {
        let (sender, lines) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("towers-console".into())
            .spawn(move || {
                for line in io::stdin().lock().lines() {
                    match line {
                        Ok(line) => {
                            if sender.send(line).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            crcbl::log::warn!("serve: the console's input failed: {error}");
                            return;
                        }
                    }
                }
            });
        if let Err(error) = reader {
            crcbl::log::warn!("serve: no console, the reader thread did not start: {error}");
        }
        Self::new(lines)
    }

    /// Answers every line waiting, printing through `print`, and says
    /// whether to serve on: [`Next::Quit`] at the first `quit`, leaving any
    /// line after it unread.
    pub(crate) fn obey(&mut self, server: &Server, print: &mut dyn FnMut(&str)) -> Next {
        loop {
            match self.lines.try_recv() {
                Ok(line) => match Command::parse(&line) {
                    Command::Quit => return Next::Quit,
                    Command::Status => print(&server.status()),
                    Command::Blank => {}
                    Command::Unknown(word) => print(&format!(
                        "{APP}: no command {word:?}; the commands are {COMMANDS}"
                    )),
                },
                Err(TryRecvError::Empty) => return Next::Serve,
                Err(TryRecvError::Disconnected) => {
                    if !self.closed {
                        self.closed = true;
                        crcbl::log::info!(
                            "serve: the console's input ended; serving on, with no console"
                        );
                    }
                    return Next::Serve;
                }
            }
        }
    }
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

/// Serves `map` on UDP `port` (0 for any free one), announced on the LAN, at
/// `tick_hz` on the wall clock, with a console on stdin — until `quit` is
/// typed at it — recording to the new file `record` names, if it names one.
/// Answers the last status line.
///
/// # Errors
///
/// [`GameError::Lan`] if the listener would not bind, or the recording would
/// not start or did not finish whole.
pub(crate) fn serve(
    port: u16,
    map: &Map,
    tick_hz: u32,
    record: Option<&Path>,
) -> Result<String, GameError> {
    let mut server = Server::open(LanBind::on_the_lan(port), map, tick_hz, record)?;
    println!("{APP}: console: {COMMANDS}");
    let tick = FrameClock::new(tick_hz).tick_dt();
    let started = Instant::now();
    serve_until_quit(
        &mut server,
        &mut Console::on_stdin(),
        tick,
        || started.elapsed(),
        &mut |line| println!("{line}"),
    )
}

/// Serves `server` a frame a `tick` on the clock `now` reads, reading
/// `console` between frames and printing through `print`, until it says
/// `quit`: then every session ends (`Server::shutdown`), the recording, if
/// there is one, is finished and what it holds printed, and the last status
/// line is answered.
///
/// # Errors
///
/// [`GameError::Lan`] when the recording did not finish whole.
pub(crate) fn serve_until_quit(
    server: &mut Server,
    console: &mut Console,
    tick: Duration,
    mut now: impl FnMut() -> Duration,
    print: &mut dyn FnMut(&str),
) -> Result<String, GameError> {
    loop {
        if let Some(status) = server.frame(now()) {
            print(&status);
        }
        if console.obey(server, print) == Next::Quit {
            server.shutdown();
            if let Some(finished) = server.stop_recording() {
                let summary = finished.map_err(|error| GameError::Lan(LanError::Record(error)))?;
                print(&format!("{APP}: {summary}"));
            }
            return Ok(server.status());
        }
        thread::sleep(until_next_tick(now(), tick));
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

    /// **The console reads a line whatever its case and its blanks**, and
    /// anything else is not a command.
    #[test]
    fn the_console_reads_its_two_commands_and_nothing_else() {
        assert_eq!(Command::parse("quit"), Command::Quit);
        assert_eq!(Command::parse("  QUIT\r"), Command::Quit);
        assert_eq!(Command::parse("Status"), Command::Status);
        assert_eq!(Command::parse(" \t"), Command::Blank);
        assert_eq!(
            Command::parse("quit now"),
            Command::Unknown("quit now".into())
        );
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
