//! `crcbl edit <DIR> --serve [PORT]` — a scene directory served to the
//! clients of the edit protocol until `quit` is typed at its console.
//!
//! ```text
//!  wall clock ─▶ Server::frame ─▶ EditServer ─▶ Document ─▶ DIR, its history
//!                    │   ▲           │                       and its lock
//!                    │   └─ UdpListener: each client's link
//!                    └─▶ the status line
//!  stdin ─▶ ConsoleLines ─▶ Console::obey, between frames
//! ```
//!
//! Towers' dedicated server (`apps/towers/src/lan/serve.rs`) is the
//! precedent: no window, the wall clock, a console on stdin through
//! [`crcbl::lan::console`], and a status line. What it serves is the scene's
//! [`Document`] through [`EditServer`], so a client's edit takes the path a
//! `crcbl scene` verb or a key in the editor takes — the same validation, the
//! same refusals, the same history — and its followers hear of it as a
//! notice ([`crcbl::scene_edit::SceneFollower`]).
//!
//! # Decided 2026-10-05
//!
//! * **The scene is locked for the whole session**, from before it is read
//!   to `quit` ([`crate::scene_cmd`]'s lock, the same refusal and exit
//!   code): while it is served a `crcbl scene` edit or an editor opening it
//!   is refused, so remote edits are the only way in and nothing writes the
//!   files between two of the server's saves.
//! * **Opened as the one-shot verbs open it**, with its history
//!   ([`Document::open_with_history`]): an undo a client asks for walks back
//!   an edit an earlier run made, and a history the scene refuses exits 3
//!   rather than being served without it.
//! * **Saved after every update that applied an operation**, the scene and
//!   its history ([`Document::save_with_history`]). The one-shot verbs save
//!   each edit before they return, and a client told an edit applied should
//!   be able to trust it outlives a crash of the server. The cost is one
//!   save — every file of the scene written atomically, and the lock's check
//!   reading them back first — at the rate people edit, and every operation
//!   read in one update shares it. Saving on an interval, or only at
//!   `quit`, would bound that cost under a script sending edits in a loop,
//!   at the price of losing what was acknowledged since.
//! * **Not while a client's drag is open** (decided 2026-10-05): a save
//!   seals the history's entry on top, so saving between a drag's frames
//!   would leave one entry a save, and the history on disk a drag in pieces
//!   that `crcbl scene undo` walks back a piece at a time. The update that
//!   ends the gesture saves it as one entry — its last frame, any other
//!   operation, its client's link going down
//!   ([`EditServer::gesture_open`]). A drag's frames before then are
//!   acknowledged and not yet on disk, as they are not in the editor until
//!   the button comes up; `save` and `quit` at the console save at once,
//!   and the drag carries on as an entry of its own.
//! * **A save that fails keeps the edits and serves on.** The status line
//!   says the scene is not saved and why, the next applied operation or
//!   `save` tries again, and a `quit` that cannot save says why and serves
//!   on rather than exit with edits only it holds; ending the process is how
//!   to drop them.
//! * **Loopback unless `--lan`.** Any admitted peer may edit
//!   (`crcbl::scene_edit::serve`'s decisions), and admission asks only that
//!   the client's build agrees, so a server on every interface lets anyone
//!   who reaches the port rewrite the scene; binding every interface is also
//!   what raises the system's firewall question on Windows. Loopback serves
//!   every client on this machine and asks nothing.
//! * **Not announced on the LAN.** The samples announce on the shared
//!   discovery port, which one program on a machine can hold, and no client
//!   browses for an edit server yet: a client connects to the address the
//!   server prints. An announcement waits on a client that browses.
//! * **Malformed input is counted, never fatal**: a datagram no link
//!   claims, a message that will not decode and one that fails its seal
//!   are each counted where they were read — the listener's and the host's
//!   counters — and the status line names their sum. An operation that will
//!   not decode is refused to its author as malformed, as any refusal is.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use crcbl::core::FrameClock;
use crcbl::lan::console::{ConsoleLines, stdin_lines, until_next_tick};
use crcbl::net::SessionEndReason;
use crcbl::net::udp::UdpListener;
use crcbl::scene_edit::serve::{EDIT_PROTOCOL_ID, EDIT_TICK_HZ, edit_compatibility};
use crcbl::scene_edit::{Document, EditServer, SceneLock};
use crcbl::server::HostConfig;

use crate::json::Json;
use crate::report::{Failure, Outcome};
use crate::scene_args::ServeArgs;
use crate::scene_cmd;

/// What the server's printed lines start with, and the verb its failures
/// are reported under.
const APP: &str = "edit";

/// The most clients a served scene admits at once: a few people and their
/// scripts, with room to spare.
const MAX_CLIENTS: usize = 8;

/// The longest a running server goes without printing its status line. A
/// change in its headline prints one at once.
pub(crate) const STATUS_INTERVAL: Duration = Duration::from_secs(10);

/// What the console's help line names.
const COMMANDS: &str = "status, save, quit";

/// Runs `crcbl edit --serve`: serves the scene until `quit`, and answers the
/// last status line.
///
/// # Errors
///
/// [`Failure`] before anything is served: the scene held by another
/// program, a history refused, a scene that will not open, and a port that
/// will not bind — each with its exit code.
pub fn run(args: &ServeArgs) -> Result<Outcome, Failure> {
    let ip = if args.lan {
        Ipv4Addr::UNSPECIFIED
    } else {
        Ipv4Addr::LOCALHOST
    };
    let mut server = Server::open(&args.dir, SocketAddr::from((ip, args.port)))?;
    println!(
        "{APP}: serving `{}` on UDP {}{}",
        args.dir.display(),
        server.local_addr(),
        if args.lan {
            ", every interface: anyone who reaches the port can edit the scene"
        } else {
            ", this machine only (--lan serves the network)"
        }
    );
    println!("{APP}: console: {COMMANDS}");
    let tick = FrameClock::new(EDIT_TICK_HZ).tick_dt();
    let started = Instant::now();
    let last = serve_until_quit(
        &mut server,
        &mut Console::on_stdin(),
        tick,
        || started.elapsed(),
        &mut |line| println!("{line}"),
    );
    Ok(Outcome {
        human: last,
        json: Vec::new(),
    })
}

/// What the status line leads with; a change in any of it prints the line at
/// once rather than at the next [`STATUS_INTERVAL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Headline {
    clients: usize,
    revision: u64,
    position: usize,
    saved: bool,
}

/// A scene served to edit clients over UDP, and the lock it holds on its
/// directory — see the module docs.
pub(crate) struct Server {
    edit: EditServer,
    listener: UdpListener,
    /// Where the listener is bound: what clients connect to.
    addr: SocketAddr,
    dir: PathBuf,
    /// Held from [`open`](Self::open) to [`quit`](Self::quit).
    lock: Option<SceneLock>,
    /// The revision the files on disk stand at.
    saved_revision: u64,
    /// Why the last save failed, until one lands.
    unsaved: Option<String>,
    /// What the last status line said, and when the next is due regardless.
    printed: Option<Headline>,
    next_status: Duration,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("dir", &self.dir)
            .field("listener", &self.listener)
            .field("revision", &self.edit.revision())
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Locks the scene at `dir`, opens it with its history, and listens for
    /// clients at `listen`, with none connected yet.
    ///
    /// # Errors
    ///
    /// As a `crcbl scene` edit's: the lock held ([`crate::report::EXIT_LOCKED`]),
    /// the history refused ([`crate::report::EXIT_HISTORY`]), the scene not
    /// opening; and a listener that will not bind. Nothing is served and the
    /// lock is let go.
    pub fn open(dir: &Path, listen: SocketAddr) -> Result<Self, Failure> {
        let lock = scene_cmd::lock(dir, APP)?;
        let vocabulary = crcbl_editor::scene::vocabulary();
        let compatibility = edit_compatibility(&vocabulary);
        let document = Document::open_with_history(dir, vocabulary)
            .map_err(|error| scene_cmd::opening(dir, APP, &error))?;
        let unbound = |error: std::io::Error| {
            Failure::new(format!("cannot listen on UDP {listen}: {error}"))
                .with("verb", Json::string(APP))
        };
        let listener = UdpListener::bind(listen, EDIT_PROTOCOL_ID).map_err(unbound)?;
        let addr = listener.local_addr().map_err(unbound)?;
        let mut edit = EditServer::new(
            document,
            HostConfig {
                max_peers: MAX_CLIENTS,
                tick_hz: EDIT_TICK_HZ,
                compatibility,
            },
        );
        // The host's clock takes its baseline here, as `LanHost::open`'s
        // does.
        edit.update(Duration::ZERO);
        Ok(Self {
            edit,
            listener,
            addr,
            dir: dir.to_path_buf(),
            lock: Some(lock),
            saved_revision: 0,
            unsaved: None,
            printed: None,
            next_status: Duration::ZERO,
        })
    }

    /// The address clients connect to.
    pub const fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Takes in newly connected clients, answers their edits and fetches at
    /// `now` — the time since serving began — saves what that applied, and
    /// answers the status line when it is due: when its headline changed, or
    /// it has been quiet for [`STATUS_INTERVAL`].
    pub fn frame(&mut self, now: Duration) -> Option<String> {
        while let Some(peer) = self.listener.accept() {
            crcbl::log::info!("{APP}: {} connected", peer.peer_addr());
            self.edit.host_mut().add(Box::new(peer));
        }
        self.edit.update(now);
        for event in self.edit.host_mut().events() {
            crcbl::log::info!("{APP}: {event:?}");
        }
        if self.edit.revision() != self.saved_revision
            && !self.edit.gesture_open()
            && let Err(why) = self.save_now()
        {
            crcbl::log::warn!("{APP}: the scene was not saved: {why}");
        }
        let headline = self.headline();
        if self.printed == Some(headline) && now < self.next_status {
            return None;
        }
        self.printed = Some(headline);
        self.next_status = now + STATUS_INTERVAL;
        Some(self.status())
    }

    /// Saves the scene and its history as the document stands, or says why
    /// not, which is kept for the status line until a save lands.
    fn save_now(&mut self) -> Result<(), String> {
        match self.edit.document_mut().save_with_history() {
            Ok(()) => {
                self.saved_revision = self.edit.revision();
                self.unsaved = None;
                Ok(())
            }
            Err(error) => {
                let why = error.to_string();
                self.unsaved = Some(why.clone());
                Err(why)
            }
        }
    }

    /// The console's `save`, as the line it prints.
    pub fn save(&mut self) -> String {
        if self.edit.revision() == self.saved_revision {
            return format!("{APP}: already saved at revision {}", self.saved_revision);
        }
        match self.save_now() {
            Ok(()) => format!("{APP}: saved at revision {}", self.saved_revision),
            Err(why) => format!("{APP}: not saved: {why}"),
        }
    }

    /// The console's `quit`: saves what is not saved, ends every client's
    /// session with [`SessionEndReason::SHUTTING_DOWN`] and lets the lock go,
    /// answering the last status line — or, when the save fails, keeps
    /// serving and answers why.
    ///
    /// # Errors
    ///
    /// The line saying the scene would not save; nothing was ended.
    pub fn quit(&mut self) -> Result<String, String> {
        if self.edit.revision() != self.saved_revision {
            self.save_now().map_err(|why| {
                format!(
                    "{APP}: not quitting, the scene would not save: {why}; `save` again once \
                     that is put right, or end the process to drop the edits"
                )
            })?;
        }
        self.edit
            .host_mut()
            .shutdown(SessionEndReason::SHUTTING_DOWN);
        self.lock = None;
        Ok(self.status())
    }

    /// Malformed input counted so far: datagrams no link claimed, messages
    /// that would not decode, and messages that failed their seal.
    pub fn malformed_count(&self) -> u64 {
        let host = self.edit.host();
        let listener = self.listener.stats();
        host.processing_error_count()
            + host.auth_failure_count()
            + listener.malformed
            + listener.unknown_source
    }

    /// The status line: the scene, its revision and history, who is in,
    /// whether it is saved, and the malformed input counted, when there was
    /// any.
    pub fn status(&self) -> String {
        let document = self.edit.document();
        let saved = match &self.unsaved {
            Some(why) if self.edit.revision() != self.saved_revision => {
                format!("NOT SAVED: {why}")
            }
            _ if self.edit.revision() != self.saved_revision => {
                "a drag under way, saved when it ends".to_owned()
            }
            _ => "saved".to_owned(),
        };
        let malformed = match self.malformed_count() {
            0 => String::new(),
            count => format!(", {count} malformed messages refused"),
        };
        format!(
            "{APP}: `{}` at revision {}, history at {} of {}, {}/{MAX_CLIENTS} clients, \
             {saved}{malformed}",
            self.dir.display(),
            self.edit.revision(),
            document.log().position(),
            document.log().len(),
            self.edit.host().peer_count(),
        )
    }

    fn headline(&self) -> Headline {
        Headline {
            clients: self.edit.host().peer_count(),
            revision: self.edit.revision(),
            position: self.edit.document().log().position(),
            saved: self.edit.revision() == self.saved_revision,
        }
    }
}

/// A line typed at the console, read.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// Save, end every session and stop.
    Quit,
    /// Print the status line now.
    Status,
    /// Save now.
    Save,
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
        } else if word.eq_ignore_ascii_case("save") {
            Self::Save
        } else {
            Self::Unknown(word.to_owned())
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

/// The console's lines as the serve loop reads them: stdin's, or a test's.
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
        Self::new(stdin_lines("crcbl-edit-console"))
    }

    /// Answers every line waiting, printing through `print`, and says
    /// whether to serve on: [`Next::Quit`] at the first `quit`, leaving any
    /// line after it unread.
    pub(crate) fn obey(&mut self, server: &mut Server, print: &mut dyn FnMut(&str)) -> Next {
        while let Some(line) = self.lines.next_line() {
            match Command::parse(&line) {
                Command::Quit => return Next::Quit,
                Command::Status => print(&server.status()),
                Command::Save => print(&server.save()),
                Command::Blank => {}
                Command::Unknown(word) => print(&format!(
                    "{APP}: no command {word:?}; the commands are {COMMANDS}"
                )),
            }
        }
        Next::Serve
    }
}

/// Serves `server` a frame a `tick` on the clock `now` reads, reading
/// `console` between frames and printing through `print`, until a `quit`
/// whose save lands ([`Server::quit`]); answers the last status line.
pub(crate) fn serve_until_quit(
    server: &mut Server,
    console: &mut Console,
    tick: Duration,
    mut now: impl FnMut() -> Duration,
    print: &mut dyn FnMut(&str),
) -> String {
    loop {
        if let Some(status) = server.frame(now()) {
            print(&status);
        }
        if console.obey(server, print) == Next::Quit {
            match server.quit() {
                Ok(last) => return last,
                Err(why) => print(&why),
            }
        }
        thread::sleep(until_next_tick(now(), tick));
    }
}

#[cfg(test)]
mod tests;
