//! A dedicated server: `towers --serve [PORT]`.
//!
//! ```text
//!  wall clock ─▶ Server::frame ─▶ LanHost ─▶ Host ─▶ TowersModule ─▶ Stage
//!                    │                        ├─ UDP ─ a player
//!                    └─▶ the status line,     └─ UDP ─ another
//!                        a line per link
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
//! The server reads its stdin as a console — the dedicated-server norm —
//! through [`crcbl::lan::console`], whose thread reads the lines and hands
//! them over a channel the loop drains between frames ([`Console::obey`]).
//! `quit` ends every session with `SessionEndReason::SHUTTING_DOWN`
//! (`Host::shutdown`), so each player is told before the sockets close, and
//! [`serve`] answers the last status line; `status` prints the status line
//! now; `save [SLOT]` and `load` are the run's (below); `ban`, `unban` and
//! `bans` are the denylist's (below); anything else prints the commands there
//! are.
//!
//! # The status line, and each player's link
//!
//! The status line leads with the players, the wave, lives, gold, the run
//! and how it stands, and under it comes a line per player's link — the
//! player's id, then round trip, loss and bytes a second each way, from the
//! host's netgraph
//! ([`crcbl::lan::netgraph::LinkReading::summary`]) — so the line itself
//! stays one a reader can scan. Only the headline's news prints it early;
//! a link's figures moving waits for the interval, or for `status`. The
//! rest of the netgraph's table — the round trip's deviation, resends and
//! the snapshot's size — is a joiner's F3 panel's to show; the console has
//! no `net` command.
//!
//! # Saving: `save`, `load` and `--resume`
//!
//! The server keeps the run between waves in its own file,
//! [`SERVER_FILE`](crate::save::SERVER_FILE) in the data directory: an
//! autosave at each wave's end, `save` at the console between waves — both
//! through [`Server::save`], the one save path, which writes with the
//! player's own writer, and `save slot2` into a named slot's file beside it —
//! and `load` to put the saved run back — under the players in it,
//! who see it in the next snapshot, since a snapshot is the whole field. A
//! loaded run waits for a player before it moves, and an empty server does
//! not throw it away as it does a run its last player left. `--serve
//! --resume` loads it before the first frame and refuses to start without
//! it. A recorded session refuses `load` and `--resume`: a recording is
//! re-simulated from a fresh run, so a run swapped in under it would be one
//! the recording could not reproduce.
//!
//! # Banning: `ban`, `unban` and `bans`
//!
//! `ban <PLAYER> [REASON]` refuses a player at the handshake from then on,
//! with the reason shown to them, and kicks them if they are in;
//! `unban <PLAYER>` lifts it; `bans` lists every ban. A player is named by
//! the id the status line prints beside their link. The list is kept in
//! [`BANS_FILE`] beside the server's run, read when the server starts and
//! written after every change ([`crcbl::lan::bans`]). **A ban holds back a
//! player who keeps their id, and nobody else**: the id is self-asserted on a
//! LAN server, and a player who draws a new one is a stranger to the list —
//! `crcbl::net::PlayerId`'s docs say why.
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

use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use std::path::Path;

use crcbl::core::FrameClock;
use crcbl::lan::console::{ConsoleLines, stdin_lines, until_next_tick};
use crcbl::lan::netgraph::Link;
use crcbl::lan::{LanBind, LanError, LanHost};
use crcbl::net::{PlayerId, SessionEndReason};
use crcbl::replay_record::{RecordError, RecordSummary};
use crcbl::server::PeerId;

use super::{APP, MAX_PLAYERS, SESSION, event, tell, welcome};
use crate::game::{Autosave, Field, GameError, Stats};
use crate::map::Map;
use crate::save::{SaveError, Vault};
use crate::wave::{Outcome, WAVES};
use crcbl::save::{SaveDesk, SaveFailure, SaveRequest, SaveTrigger, Saved};

/// The file a dedicated server keeps its denylist in, beside its run in the
/// data directory.
pub(crate) const BANS_FILE: &str = "towers-bans.txt";

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
    /// The map the run is played on, which a save is read against.
    map: Map,
    /// The map every player is sent as they join, as its event, encoded
    /// once.
    map_event: Vec<u8>,
    /// Where the run is saved — the data directory outside the tests.
    vault: Vault,
    /// What every save went through: the console's and the autosave's.
    desk: SaveDesk,
    /// Which wave's end was last autosaved.
    autosave: Autosave,
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
    /// one, and saving it in `vault`. Every player is sent `map` as they
    /// join, and the refusals of their commands.
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
        vault: Vault,
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
            map: map.clone(),
            map_event: event::map(&map.to_wire()),
            desk: vault.desk(),
            vault,
            autosave: Autosave::default(),
            printed: None,
            next_status: Duration::ZERO,
        })
    }

    /// Runs the server to `now` — the time since it opened — and answers the
    /// status line when it is due to be printed: when it has news, or has
    /// been quiet for [`STATUS_INTERVAL`].
    pub fn frame(&mut self, now: Duration) -> Option<String> {
        let events = self.lan.frame(now);
        welcome(self.lan.host_mut(), &events, &self.map_event, None);
        tell(self.lan.host_mut(), &self.field.take_refusals());
        if self.field.wave_end(&mut self.autosave).is_some() {
            match self.save(SaveRequest::new(SaveTrigger::Autosave)) {
                Ok(saved) => crcbl::log::info!("serve: autosaved the end of {}", saved.summary),
                Err(failure) => crcbl::log::warn!("serve: the autosave failed: {failure}"),
            }
        }
        let headline = self.headline();
        if self.printed == Some(headline) && now < self.next_status {
            return None;
        }
        self.printed = Some(headline);
        self.next_status = now + STATUS_INTERVAL;
        Some(self.status())
    }

    /// Keeps the server's denylist in the file at `path` — see
    /// [`LanHost::keep_bans`]. Returns how many players it bans.
    ///
    /// # Errors
    ///
    /// [`GameError::Lan`] when the file is there and will not read whole.
    pub fn keep_bans(&mut self, path: &Path) -> Result<usize, GameError> {
        self.lan.keep_bans(path).map_err(GameError::Lan)
    }

    /// `ban`, as the line the console prints: who was banned and whether
    /// they were kicked, or why the list was not kept.
    fn ban_line(&mut self, player: PlayerId, reason: &str) -> String {
        match self.lan.ban(player, reason) {
            Ok(Some(peer)) => format!("{APP}: banned {player}, and kicked peer {}", peer.get()),
            Ok(None) => format!("{APP}: banned {player}"),
            Err(error) => format!("{APP}: banned {player}, but {error}"),
        }
    }

    /// `unban`, as the line the console prints.
    fn unban_line(&mut self, player: PlayerId) -> String {
        match self.lan.unban(player) {
            Ok(true) => format!("{APP}: unbanned {player}"),
            Ok(false) => format!("{APP}: {player} is not banned"),
            Err(error) => format!("{APP}: unbanned {player}, but {error}"),
        }
    }

    /// `bans`, as the lines the console prints: a line a ban, by id.
    fn bans_lines(&self) -> String {
        let list = self.lan.host().denylist();
        let mut lines = format!("{APP}: {} banned", list.len());
        for (player, reason) in list.iter() {
            let line = format!("  {player} {reason}");
            lines.push('\n');
            lines.push_str(line.trim_end());
        }
        lines
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

    /// The status line — who is in, and how the run stands — and under it a
    /// line per player's link, as the netgraph last recorded it. A player
    /// gone since that frame — every one, straight after a shutdown — is
    /// not listed.
    pub fn status(&self) -> String {
        let mut status = status_line(self.players(), &self.stats());
        let host = self.lan.host();
        for link in self.lan.netgraph().links() {
            // A player gone since the netgraph's frame has no player id.
            if let Some(player) = host.player(PeerId::from_raw(link.id)) {
                status.push('\n');
                status.push_str(&link_line(link, player));
            }
        }
        status
    }

    /// **The server's one save path**: the run, if no wave is coming in,
    /// through [`crate::save::write_run`] — the writer a player's game uses —
    /// into the server's own file or the slot `request` names. The console's
    /// `save` and the autosave at a wave's end both come here.
    ///
    /// # Errors
    ///
    /// A [`SaveFailure`] naming why nothing was written: a wave coming in, a
    /// finished run, or a write the backend refused.
    pub fn save(&mut self, request: SaveRequest) -> Result<Saved, SaveFailure> {
        let checkpoint = self.field.checkpoint();
        self.desk.take(request, |request| {
            crate::save::write_run(&self.vault, checkpoint, request)
        })
    }

    /// [`Server::save`], as the line the console prints: what was saved, or
    /// why not.
    fn save_line(&mut self, request: SaveRequest) -> String {
        match self.save(request) {
            Ok(saved) => {
                let stats = self.stats();
                format!(
                    "{APP}: saved {}, {} lives, {} gold",
                    saved.summary, stats.lives, stats.gold
                )
            }
            Err(SaveFailure::Refused(why)) => format!("{APP}: not saved: {}", why.to_lowercase()),
            Err(failure) => format!("{APP}: not saved: {failure}"),
        }
    }

    /// The desk every save went through, for the tests that read what it
    /// recorded.
    #[cfg(test)]
    pub const fn desk(&self) -> &SaveDesk {
        &self.desk
    }

    /// Puts the saved run back in place of the one being played, under
    /// every player in it.
    ///
    /// # Errors
    ///
    /// [`SaveError::Recording`] while the session is recorded,
    /// [`SaveError::NoSave`] when there is none, and whatever the save itself
    /// is refused for — another map, another version, a corrupt file.
    pub fn load(&mut self) -> Result<(), SaveError> {
        if let Some(path) = self.lan.recording() {
            return Err(SaveError::Recording(path.to_path_buf()));
        }
        let checkpoint = self.vault.load(&self.map)?.ok_or(SaveError::NoSave)?;
        self.field.restore(&checkpoint)?;
        self.autosave.restored(&checkpoint);
        Ok(())
    }

    /// [`Server::load`], as the line the console prints.
    fn load_line(&mut self) -> String {
        match self.load() {
            Ok(()) => {
                let stats = self.stats();
                format!(
                    "{APP}: loaded wave {}/{}, {} lives, {} gold",
                    stats.wave,
                    WAVES.len(),
                    stats.lives,
                    stats.gold,
                )
            }
            Err(error) => format!("{APP}: not loaded: {error}"),
        }
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

/// One player's link under the status line, indented so the status line
/// still leads, naming the player as `ban` takes them:
/// `  peer 2 (00112233445566778899aabbccddeeff): rtt 0.4 ms, loss 0.0%, in
/// 1200 B/s, out 3400 B/s`.
fn link_line(link: &Link, player: PlayerId) -> String {
    format!("  peer {} ({player}): {}", link.id, link.reading.summary())
}

/// What the console's help line names.
const COMMANDS: &str = "status, save [SLOT], load, ban PLAYER [REASON], unban PLAYER, bans, quit";

/// A line typed at the console, read.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// End every session and stop.
    Quit,
    /// Print the status line now.
    Status,
    /// Save the run, if no wave is coming in, into the server's own file or
    /// the slot named.
    Save(SaveRequest),
    /// A command whose arguments it refused, and the line saying why.
    Refused(String),
    /// Put the saved run back.
    Load,
    /// Refuse a player at the handshake from now on, for the reason given,
    /// kicking them if they are in.
    Ban(PlayerId, String),
    /// Lift a player's ban.
    Unban(PlayerId),
    /// List every ban.
    Bans,
    /// Nothing but blanks: nothing to answer.
    Blank,
    /// Anything else, trimmed.
    Unknown(String),
}

impl Command {
    /// The command `line` is, ignoring the blanks around it and the case of
    /// the command's word. `save` takes a slot after it, read by the engine's
    /// own grammar ([`SaveRequest::from_args`]) so the debug console's `save`
    /// and this one mean the same thing; every other command is one word.
    fn parse(line: &str) -> Self {
        let word = line.trim();
        let mut words = word.split_whitespace();
        if words
            .next()
            .is_some_and(|first| first.eq_ignore_ascii_case("save"))
        {
            let args: Vec<&str> = words.collect();
            return match SaveRequest::from_args(SaveTrigger::ServerConsole, &args) {
                Ok(request) => Self::Save(request),
                Err(why) => Self::Refused(why),
            };
        }
        let mut words = word.split_whitespace();
        let first = words.next().unwrap_or_default();
        if first.eq_ignore_ascii_case("ban") || first.eq_ignore_ascii_case("unban") {
            let ban = first.eq_ignore_ascii_case("ban");
            let Some(id) = words.next() else {
                return Self::Refused(format!("{} needs a player id", first.to_lowercase()));
            };
            let player = match id.parse() {
                Ok(player) => player,
                Err(error) => return Self::Refused(format!("{id:?}: {error}")),
            };
            let rest: Vec<&str> = words.collect();
            return match (ban, rest.is_empty()) {
                (true, _) => Self::Ban(player, rest.join(" ")),
                (false, true) => Self::Unban(player),
                (false, false) => Self::Refused("unban takes a player id and nothing else".into()),
            };
        }
        if word.is_empty() {
            Self::Blank
        } else if word.eq_ignore_ascii_case("quit") {
            Self::Quit
        } else if word.eq_ignore_ascii_case("status") {
            Self::Status
        } else if word.eq_ignore_ascii_case("load") {
            Self::Load
        } else if word.eq_ignore_ascii_case("bans") {
            Self::Bans
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
#[derive(Debug)]
pub(crate) struct Console {
    lines: ConsoleLines,
}

impl Console {
    /// A console reading `lines`.
    pub(crate) const fn new(lines: Receiver<String>) -> Self {
        Self {
            lines: ConsoleLines::new(lines),
        }
    }

    /// A console on this process's stdin — see [`stdin_lines`].
    fn on_stdin() -> Self {
        Self::new(stdin_lines("towers-console"))
    }

    /// Answers every line waiting, printing through `print`, and says
    /// whether to serve on: [`Next::Quit`] at the first `quit`, leaving any
    /// line after it unread.
    pub(crate) fn obey(&mut self, server: &mut Server, print: &mut dyn FnMut(&str)) -> Next {
        while let Some(line) = self.lines.next_line() {
            match Command::parse(&line) {
                Command::Quit => return Next::Quit,
                Command::Status => print(&server.status()),
                Command::Save(request) => print(&server.save_line(request)),
                Command::Refused(why) => print(&format!("{APP}: {why}")),
                Command::Load => print(&server.load_line()),
                Command::Ban(player, reason) => print(&server.ban_line(player, &reason)),
                Command::Unban(player) => print(&server.unban_line(player)),
                Command::Bans => print(&server.bans_lines()),
                Command::Blank => {}
                Command::Unknown(word) => print(&format!(
                    "{APP}: no command {word:?}; the commands are {COMMANDS}"
                )),
            }
        }
        Next::Serve
    }
}

/// Serves `map` on UDP `port` (0 for any free one), announced on the LAN, at
/// `tick_hz` on the wall clock, with a console on stdin — until `quit` is
/// typed at it — recording to the new file `record` names, if it names one,
/// and saving in the data directory, from the saved run when `resume` says
/// so. Answers the last status line.
///
/// # Errors
///
/// [`GameError::Lan`] if the listener would not bind, or the recording would
/// not start or did not finish whole, and [`GameError::Resume`] if `resume`
/// found no run it would resume.
pub(crate) fn serve(
    port: u16,
    map: &Map,
    tick_hz: u32,
    record: Option<&Path>,
    resume: bool,
) -> Result<String, GameError> {
    let mut server = Server::open(
        LanBind::on_the_lan(port),
        map,
        tick_hz,
        record,
        Vault::server(),
    )?;
    match crcbl::store::NativeStorage::data_root(APP) {
        Some(root) => {
            let path = root.join(BANS_FILE);
            let banned = server.keep_bans(&path)?;
            println!("{APP}: {banned} banned, kept in {}", path.display());
        }
        None => crcbl::log::warn!(
            "serve: this platform names no data directory; bans last until the server stops"
        ),
    }
    if resume {
        server.load().map_err(GameError::Resume)?;
        println!("{}", server.status());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// **The console reads a line whatever its case and its blanks**, and
    /// anything else is not a command.
    #[test]
    fn the_console_reads_its_commands_and_nothing_else() {
        assert_eq!(Command::parse("quit"), Command::Quit);
        assert_eq!(Command::parse("  QUIT\r"), Command::Quit);
        assert_eq!(Command::parse("Status"), Command::Status);
        assert_eq!(
            Command::parse(" save "),
            Command::Save(SaveRequest::new(SaveTrigger::ServerConsole))
        );
        assert_eq!(
            Command::parse("SAVE  slot2"),
            Command::Save(
                SaveRequest::new(SaveTrigger::ServerConsole)
                    .in_slot(crcbl::save::Slot::new("slot2").expect("a bare name"))
            )
        );
        assert!(
            matches!(Command::parse("save ../x"), Command::Refused(why) if why.contains("slot")),
            "a slot that is a path is refused"
        );
        assert_eq!(Command::parse("LOAD"), Command::Load);
        assert_eq!(Command::parse(" \t"), Command::Blank);
        assert_eq!(
            Command::parse("quit now"),
            Command::Unknown("quit now".into())
        );
    }

    /// **`ban`, `unban` and `bans` read a player id and a reason**, either
    /// case, the reason being every word after the id; a missing id, one that
    /// does not read, and an unban with more are refused, naming why.
    #[test]
    fn the_console_reads_its_ban_commands() {
        let player = PlayerId::from_seed(3);
        assert_eq!(
            Command::parse(&format!("BAN {player}  griefing the  base ")),
            Command::Ban(player, "griefing the base".into())
        );
        assert_eq!(
            Command::parse(&format!("ban {player}")),
            Command::Ban(player, String::new())
        );
        assert_eq!(
            Command::parse(&format!("unban {}", player.to_string().to_uppercase())),
            Command::Unban(player)
        );
        assert_eq!(Command::parse(" Bans "), Command::Bans);
        assert!(
            matches!(Command::parse("ban"), Command::Refused(why) if why.contains("player id"))
        );
        assert!(
            matches!(Command::parse("unban xyz"), Command::Refused(why) if why.contains("\"xyz\""))
        );
        assert!(matches!(
            Command::parse(&format!("unban {player} now")),
            Command::Refused(_)
        ));
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
