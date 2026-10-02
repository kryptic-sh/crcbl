//! What a LAN lobby knows, with none of how it looks: the hosts a
//! [`Browser`] heard, sorted into the ones this build can join and the ones
//! it cannot (with why), the address typed for a direct connect, what a pick
//! asks for, and what last went wrong.
//!
//! ```text
//!   Browser ──poll──▶ joinable      ─┐
//!                     passed over   ─┤  the game's menu, its own labels
//!   text / Backspace ▶ address      ─┤
//!   pick ────────────▶ LobbyChoice  ─┘  the game starts it, and says how it went
//! ```
//!
//! **The model is the engine's, the presentation the game's.** A [`Lobby`]
//! decides which hosts are rows ([`Lobby::joinable`]) and which are only
//! worth a line saying why not ([`Lobby::passed_over`], each with an
//! [`Unjoinable`]), whether what was typed is an address, and which host a
//! pick means. It draws nothing: each game builds its own menu from it, with
//! its own rows beside these — towers' solo run, the sandbox's offline one —
//! and its own words for every [`LobbyNotice`].
//!
//! **A pick starts nothing.** [`Lobby::pick`] answers a [`LobbyChoice`] —
//! host, or join this address — or refuses with a [`PickRefused`] it also
//! keeps as the notice. Starting the session is the game's, since what a
//! session holds is: it tells the lobby how that went with
//! [`Lobby::join_started`] or [`Lobby::start_failed`], and later with
//! [`Lobby::join_failed`] or [`Lobby::session_ended`] — the state a player
//! is sent back to the lobby in, with the reason.
//!
//! The text arrives through [`HostedGame::text_event`](crate::engine::HostedGame::text_event),
//! with the layout applied, and Backspace through
//! [`HostedGame::key_event`](crate::engine::HostedGame::key_event).

use std::net::SocketAddr;

use crate::core::input::KeyCode;
use crate::net::ProtocolCompatibility;
use crate::net::udp::discovery::{Browser, HostEntry};
use crate::ui::edit::LineEdit;

use super::{LanError, LanGame};

/// Why a host the browser heard cannot be joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unjoinable {
    /// Its protocol version is not this build's: the wire itself differs.
    Version,
    /// Its engine build is not this one.
    Build,
    /// Its schema hash is not this session's: another game on the same
    /// protocol id.
    Game,
    /// It has every player it takes.
    Full,
}

impl Unjoinable {
    /// Why `host` cannot be joined by a session hand-shaking on `ours`, or
    /// `None` when it can. The handshake's own order — version, build,
    /// schema — and then room, which is the one that changes while it runs.
    #[must_use]
    pub fn of(ours: ProtocolCompatibility, host: &HostEntry) -> Option<Self> {
        let theirs = host.compatibility;
        if theirs.protocol_version != ours.protocol_version {
            Some(Self::Version)
        } else if theirs.engine_build_id != ours.engine_build_id {
            Some(Self::Build)
        } else if theirs.schema_hash != ours.schema_hash {
            Some(Self::Game)
        } else if host.players >= host.max_players {
            Some(Self::Full)
        } else {
            None
        }
    }

    /// The reason in a few capitals, for a lobby to show beside the host.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Version => "ANOTHER VERSION",
            Self::Build => "ANOTHER BUILD",
            Self::Game => "ANOTHER GAME",
            Self::Full => "FULL",
        }
    }
}

/// What a lobby row asks for, of the rows every LAN lobby has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyPick {
    /// Host a session.
    Host,
    /// Join the joinable host at this row, counted from the first.
    Listed(usize),
    /// Join the address typed into the lobby.
    Connect,
}

/// What a pick the lobby accepted asks the game to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyChoice {
    /// Host a session, where the game hosts — what `--host` asks for.
    Host,
    /// Join the host at this address — what `--join` asks for.
    Join(SocketAddr),
}

/// Why a pick asked for nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickRefused {
    /// The listed row is not there any more: the host went quiet, or the
    /// list moved under the pick.
    HostGone,
    /// What was typed is not an `IP:PORT`; this is what was typed.
    NotAnAddress(String),
}

impl std::fmt::Display for PickRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostGone => write!(f, "that host is gone"),
            Self::NotAnAddress(typed) => write!(f, "not an IP:PORT: {typed:?}"),
        }
    }
}

impl std::error::Error for PickRefused {}

/// The last thing that went wrong, for the lobby to show until the next pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyNotice {
    /// The last pick asked for nothing.
    Refused(PickRefused),
    /// The game could not start what the last pick asked for, in its words.
    CannotStart(String),
    /// The join the last pick started ended without a session, and why.
    JoinFailed(String),
    /// The session a join from here started has ended, and how.
    SessionEnded(String),
}

/// What a host's row and line are drawn from: everything [`HostEntry`] holds
/// but when it was last heard, which moves on every announce and would
/// rebuild the menu — and throw its selection away — for nothing.
fn shown(host: &HostEntry) -> (&str, SocketAddr, u16, u16, ProtocolCompatibility) {
    (
        &host.name,
        host.addr,
        host.players,
        host.max_players,
        host.compatibility,
    )
}

/// A LAN lobby's state: the browser, the hosts it heard, the address being
/// typed, the join under way, and the last thing that went wrong.
#[derive(Debug)]
pub struct Lobby {
    /// The session this build would play: what a host is compared against.
    session: LanGame,
    /// Listening for hosts, or why not — the rest of the lobby still works.
    browser: Result<Browser, String>,
    /// The hosts that are rows, in row order.
    joinable: Vec<HostEntry>,
    /// The hosts that are lines, with why.
    passed_over: Vec<(HostEntry, Unjoinable)>,
    /// The address [`LobbyPick::Connect`] joins.
    address: LineEdit,
    /// Why the last pick did not start anything, or the last join failed.
    notice: Option<LobbyNotice>,
    /// Where the join under way is going, while one is.
    joining: Option<SocketAddr>,
    /// Whether anything a menu shows changed since [`Lobby::take_changed`].
    changed: bool,
    /// Whether text arrived since [`Lobby::take_typed`].
    typed: bool,
}

impl Lobby {
    /// A lobby looking for hosts of `session` on the LAN — a [`Browser`]
    /// querying the broadcast address. A browser that cannot bind is kept as
    /// the reason ([`Lobby::browser_error`]) rather than refused: hosting
    /// and connecting need none.
    #[must_use]
    pub fn on_the_lan(session: LanGame) -> Self {
        let browser =
            Browser::open(session.protocol_id).map_err(|error| LanError::Browse(error).to_string());
        Self::new(session, browser)
    }

    /// A lobby for `session` listening with `browser`, or showing why it is
    /// not.
    #[must_use]
    pub fn new(session: LanGame, browser: Result<Browser, String>) -> Self {
        Self {
            session,
            browser,
            joinable: Vec::new(),
            passed_over: Vec::new(),
            address: LineEdit::new(),
            notice: None,
            joining: None,
            changed: true,
            typed: false,
        }
    }

    /// The session this lobby finds hosts of.
    #[must_use]
    pub const fn session(&self) -> LanGame {
        self.session
    }

    /// Reads what the browser heard, and sorts it into rows and lines. Every
    /// frame the lobby is on screen: the browser asks only while polled.
    pub fn poll(&mut self) {
        let Ok(browser) = &mut self.browser else {
            return;
        };
        browser.poll();
        let mut joinable = Vec::new();
        let mut passed_over = Vec::new();
        for host in browser.hosts() {
            match Unjoinable::of(self.session.compatibility, &host) {
                None => joinable.push(host),
                Some(why) => passed_over.push((host, why)),
            }
        }
        let same = joinable.len() == self.joinable.len()
            && passed_over.len() == self.passed_over.len()
            && joinable
                .iter()
                .zip(&self.joinable)
                .all(|(new, old)| shown(new) == shown(old))
            && passed_over
                .iter()
                .zip(&self.passed_over)
                .all(|((new, why), (old, was))| shown(new) == shown(old) && why == was);
        if !same {
            self.joinable = joinable;
            self.passed_over = passed_over;
            self.changed = true;
        }
    }

    /// Why the lobby is not looking for hosts, when it is not.
    #[must_use]
    pub fn browser_error(&self) -> Option<&str> {
        self.browser.as_ref().err().map(String::as_str)
    }

    /// The hosts this build can join, in row order: what
    /// [`LobbyPick::Listed`] counts.
    #[must_use]
    pub fn joinable(&self) -> &[HostEntry] {
        &self.joinable
    }

    /// The hosts this build cannot join, each with why.
    #[must_use]
    pub fn passed_over(&self) -> &[(HostEntry, Unjoinable)] {
        &self.passed_over
    }

    /// Text typed while the lobby is up: it goes on the end of the address.
    pub fn text(&mut self, text: &str) {
        if self.address.insert(text) {
            self.typed = true;
        }
    }

    /// A key the menu did not take. Backspace takes a character off the
    /// address; nothing else here means anything.
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        if pressed && key == KeyCode::Backspace && self.address.backspace() {
            self.typed = true;
        }
    }

    /// The address typed so far.
    #[must_use]
    pub fn address(&self) -> &str {
        self.address.text()
    }

    /// The last thing that went wrong, until a join starts.
    #[must_use]
    pub const fn notice(&self) -> Option<&LobbyNotice> {
        self.notice.as_ref()
    }

    /// Where the join under way is going, while one is.
    #[must_use]
    pub const fn joining(&self) -> Option<SocketAddr> {
        self.joining
    }

    /// Whether the menu is stale, clearing it — the rows or the lines moved,
    /// a pick was made, or a join started, failed or ended.
    pub fn take_changed(&mut self) -> bool {
        core::mem::take(&mut self.changed)
    }

    /// Whether text arrived since the last ask, clearing it: a menu moves
    /// its selection onto the connect row, which is where it is going.
    pub fn take_typed(&mut self) -> bool {
        core::mem::take(&mut self.typed)
    }

    /// What `pick` asks the game to start, or why it asks for nothing — kept
    /// as the notice. Either way it replaces the join that was under way,
    /// which the caller drops.
    ///
    /// # Errors
    ///
    /// [`PickRefused::HostGone`] for a listed row that is not there, and
    /// [`PickRefused::NotAnAddress`] for a connect whose text is not an
    /// `IP:PORT`.
    pub fn pick(&mut self, pick: LobbyPick) -> Result<LobbyChoice, PickRefused> {
        let choice = match pick {
            LobbyPick::Host => Ok(LobbyChoice::Host),
            LobbyPick::Listed(row) => match self.joinable.get(row) {
                Some(host) => Ok(LobbyChoice::Join(host.addr)),
                None => Err(PickRefused::HostGone),
            },
            LobbyPick::Connect => match self.address.text().trim().parse::<SocketAddr>() {
                Ok(addr) => Ok(LobbyChoice::Join(addr)),
                Err(_) => Err(PickRefused::NotAnAddress(self.address.text().to_string())),
            },
        };
        self.joining = None;
        self.changed = true;
        if let Err(refused) = &choice {
            self.notice = Some(LobbyNotice::Refused(refused.clone()));
        }
        choice
    }

    /// The game started the join a pick asked for, to `host`: the lobby says
    /// where while it waits, and forgets what went wrong before.
    pub fn join_started(&mut self, host: SocketAddr) {
        self.joining = Some(host);
        self.notice = None;
        self.changed = true;
    }

    /// The game could not start what the last pick asked for: `why`, in its
    /// words, is the notice.
    pub fn start_failed(&mut self, why: String) {
        self.notice = Some(LobbyNotice::CannotStart(why));
        self.changed = true;
    }

    /// The join a pick started ended without a session: the lobby says why,
    /// and is what the player picks from again.
    pub fn join_failed(&mut self, why: &str) {
        self.joining = None;
        self.notice = Some(LobbyNotice::JoinFailed(why.to_string()));
        self.changed = true;
    }

    /// The session a join from here started has ended, and the player is
    /// back: the lobby says how, and is what the player picks from again.
    pub fn session_ended(&mut self, how: &str) {
        self.joining = None;
        self.notice = Some(LobbyNotice::SessionEnded(how.to_string()));
        self.changed = true;
    }

    /// Forgets the join under way without a word: the player left the lobby
    /// by a row of the game's own.
    pub fn clear_joining(&mut self) {
        self.joining = None;
    }
}

#[cfg(test)]
mod tests;
