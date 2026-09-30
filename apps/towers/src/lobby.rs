//! The lobby: what a native towers opens on when the command line did not
//! already choose a session.
//!
//! ```text
//!   ┌────────────────────────────────────────┐
//!   │                 TOWERS                 │
//!   │  crcbl towers 10.0.0.7:5000 1/4 ANOTHER VERSION  ← heard, not joinable
//!   │  SOLO                                  │
//!   │  HOST                              LAN │
//!   │  JOIN crcbl towers                 1/4 │  ← one row per host
//!   │  CONNECT                  10.0.0.9:500 │  ← what has been typed
//!   └────────────────────────────────────────┘
//! ```
//!
//! **Solo**, **host** (what `--host` does, on any free port) and a row per
//! LAN host a [`Browser`] hears that this build can play with — what
//! `--browse` would have joined, chosen instead of taken first. A host it
//! cannot play with is not a row: it is a line under the title in the hint
//! colour, dimmer than the rows, with the reason — another version, another
//! build, another game, or full — because a row is something Enter can fire
//! and there is nothing to fire. A host on another map is a row like any
//! other: it sends its map to whoever joins (`crate::lan`). **Connect** joins
//! the address typed into the lobby, which is what `--join` does; the text
//! arrives through [`crcbl::engine::HostedGame::text_event`], with the layout
//! applied, and Backspace takes a character off it.
//!
//! # The lobby picks, and `crate::app` starts
//!
//! A pick is `Lobby::pick`: it opens the [`Game`] a host row asks for, or the
//! [`Joining`] a join asks for — or answers `Picked::Solo`, because the solo
//! game under the lobby has not ticked and is already the run a player
//! choosing solo gets — or records why it could not, which the next menu
//! shows as a warning. `crate::app::Towers` swaps a game in: a host drops the
//! lobby, and the browser with it; a join sets the lobby aside, with the solo
//! game that was under it, for as long as its session runs.
//!
//! # A join keeps the lobby up until the host's map is in
//!
//! A join has no game until the host has sent its map, so the lobby stays on
//! screen while it waits, saying `JOINING` and where; the field under it is
//! still the solo run that has not started. When the map comes the joined
//! game replaces both. When the join fails instead — the host refused it, the
//! link ended, the map did not decode, or nothing came within
//! [`JOIN_TIMEOUT`] — the lobby is still there, and says why
//! (`Lobby::join_failed`), so the player picks again rather than being left
//! on an empty field.
//!
//! # A session that ends comes back here
//!
//! Once the joined game is up, the lobby waits, set aside and not polled,
//! until its session ends — the host left or shut down, removed this player,
//! or the link died. Then the player is back in it, over the idle solo game
//! that was under it on this process's own map, and the lobby says how the
//! session ended (`Lobby::session_ended`). Its browser is the one it had, so
//! the hosts it lists are heard again from the next frame.
//!
//! # Native only, and only when asked for nothing
//!
//! The browser build has no networking and opens on the field, as it always
//! has. A native run opens here only when its command line chose nothing —
//! see `crate::args` for which flags skip it — so every script, CI run and
//! headless test starts where it always did.

use std::net::SocketAddr;
use std::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::lan::{LanBind, LanClient, LanGame};
use crcbl::net::ProtocolCompatibility;
use crcbl::net::udp::discovery::{Browser, HostEntry};
use crcbl::ui::edit::LineEdit;
use crcbl::ui::menu::{Caption, Menu, MenuItem};

use crate::game::Game;
use crate::lan::{JOIN_TIMEOUT, Joining};
use crate::map::Map;
use crate::menu::{CONNECT_ID, FIRST_LISTED_ID, HOST_ID, SOLO_ID};

/// The lobby's heading.
pub const TITLE: &str = "TOWERS";

/// The heading of the panel a join the command line asked for waits under —
/// the lobby's own joins wait in the lobby.
pub const JOINING_TITLE: &str = "JOINING";

/// What a lobby row asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Play alone.
    Solo,
    /// Host a session and play in it.
    Host,
    /// Join the listed host at this row, counted from the first.
    Listed(usize),
    /// Join the address typed into the lobby.
    Connect,
}

/// What a pick that worked started.
#[derive(Debug)]
pub(crate) enum Picked {
    /// The solo run already under the lobby.
    Solo,
    /// A LAN session this player hosts.
    Session(Game),
    /// A join, waiting for the host's map.
    Joining(Joining),
}

/// Why a host the browser heard cannot be joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unjoinable {
    /// Its protocol version is not this build's: the wire itself differs.
    Version,
    /// Its engine build is not this one.
    Build,
    /// Its schema hash is not towers': another game on towers' protocol id.
    /// The map is not in it — a host sends its own at join.
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

    /// What the lobby says beside the host.
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

/// The lobby: the browser, the hosts it heard, the address being typed, and
/// the last thing that went wrong.
#[derive(Debug)]
pub struct Lobby {
    /// The session this build would play: what a host is compared against,
    /// and what a join hand-shakes on.
    session: LanGame,
    /// Where [`Pick::Host`] binds: every interface, as `--host` does, outside
    /// the tests.
    host_bind: LanBind,
    tick_hz: u32,
    /// Listening for hosts, or why not — shown under the title, and the rest
    /// of the lobby still works.
    browser: Result<Browser, String>,
    /// The hosts that are rows, in row order.
    joinable: Vec<HostEntry>,
    /// The hosts that are lines, with why.
    passed_over: Vec<(HostEntry, Unjoinable)>,
    /// The address [`Pick::Connect`] joins.
    address: LineEdit,
    /// Why the last pick did not start anything, or the last join failed.
    notice: Option<String>,
    /// Where the join under way is going, while one is.
    joining: Option<String>,
    /// How long a chosen host has to send its map: [`JOIN_TIMEOUT`], unless a
    /// test asked for less.
    join_timeout: Duration,
    /// Whether the rows or the lines changed since [`Lobby::take_changed`].
    changed: bool,
    /// Whether text arrived since [`Lobby::take_typed`].
    typed: bool,
}

impl Lobby {
    /// A lobby looking for hosts of `session` on the LAN — a [`Browser`]
    /// querying the broadcast address — hosting where `--host` does, at
    /// `tick_hz`. A browser that cannot bind is shown as a warning rather
    /// than refused: solo, host and connect need none.
    #[must_use]
    pub fn on_the_lan(session: LanGame, tick_hz: u32) -> Self {
        let browser = Browser::open(session.protocol_id)
            .map_err(|error| format!("NOT LOOKING FOR HOSTS: {error}"));
        Self::new(session, browser, LanBind::on_the_lan(0), tick_hz)
    }

    /// A lobby listening with `browser` and hosting on `host_bind`.
    #[must_use]
    pub fn new(
        session: LanGame,
        browser: Result<Browser, String>,
        host_bind: LanBind,
        tick_hz: u32,
    ) -> Self {
        Self {
            session,
            host_bind,
            tick_hz,
            browser,
            joinable: Vec::new(),
            passed_over: Vec::new(),
            address: LineEdit::new(),
            notice: None,
            joining: None,
            join_timeout: JOIN_TIMEOUT,
            changed: true,
            typed: false,
        }
    }

    /// This lobby, giving a chosen host `timeout` rather than
    /// [`JOIN_TIMEOUT`] to send its map, so a test sees a join time out in a
    /// few frames.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn with_join_timeout(mut self, timeout: Duration) -> Self {
        self.join_timeout = timeout;
        self
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

    /// What [`Pick::Connect`]'s row shows where a key hint would be.
    #[must_use]
    pub fn connect_hint(&self) -> String {
        if self.address.is_empty() {
            "TYPE IP:PORT".to_string()
        } else {
            self.address.text().to_string()
        }
    }

    /// Whether the menu is stale, clearing it — the rows or the lines moved,
    /// or a pick failed and there is a warning to show.
    pub fn take_changed(&mut self) -> bool {
        core::mem::take(&mut self.changed)
    }

    /// Whether text arrived since the last ask, clearing it: the menu moves
    /// its selection onto the connect row, which is where it is going.
    pub fn take_typed(&mut self) -> bool {
        core::mem::take(&mut self.typed)
    }

    /// The panel: the rows, and under the title the lines and the warning.
    #[must_use]
    pub fn menu(&self) -> Menu {
        let mut items = vec![
            MenuItem::new(SOLO_ID, "SOLO", ""),
            MenuItem::new(HOST_ID, "HOST", "LAN"),
        ];
        for (id, host) in (FIRST_LISTED_ID..).zip(&self.joinable) {
            items.push(MenuItem::new(
                id,
                format!("JOIN {}", host.name),
                format!("{}/{}", host.players, host.max_players),
            ));
        }
        items.push(MenuItem::new(CONNECT_ID, "CONNECT", self.connect_hint()));
        let mut menu = Menu::new(TITLE, items);
        match &self.browser {
            Err(why) => menu.subtitle.push(Caption::warning(why.clone())),
            Ok(_) if self.joinable.is_empty() && self.passed_over.is_empty() => {
                menu.subtitle.push("LOOKING FOR HOSTS ON THE LAN".into());
            }
            Ok(_) => {}
        }
        for (host, why) in &self.passed_over {
            menu.subtitle.push(
                format!(
                    "{} {} {}/{} {}",
                    host.name,
                    host.addr,
                    host.players,
                    host.max_players,
                    why.label()
                )
                .into(),
            );
        }
        if let Some(host) = &self.joining {
            menu.subtitle.push(format!("JOINING {host}").into());
        }
        if let Some(notice) = &self.notice {
            menu.subtitle.push(Caption::warning(notice.clone()));
        }
        menu
    }

    /// The join a pick started ended without a game: the lobby says why, and
    /// is what the player picks from again.
    pub(crate) fn join_failed(&mut self, why: &str) {
        self.joining = None;
        self.notice = Some(format!("JOIN FAILED: {why}"));
        self.changed = true;
    }

    /// The session a join from here started has ended, and the player is
    /// back: the lobby says how, and is what the player picks from again.
    pub(crate) fn session_ended(&mut self, how: &str) {
        self.joining = None;
        self.notice = Some(format!("SESSION ENDED: {how}"));
        self.changed = true;
    }

    /// Starts what `pick` asks for — a host on `map`, the local one — or
    /// records why it could not. A join does not use `map`: it plays on the
    /// host's.
    pub(crate) fn pick(&mut self, pick: Pick, map: &Map) -> Option<Picked> {
        let started = match pick {
            Pick::Solo => {
                self.joining = None;
                return Some(Picked::Solo);
            }
            Pick::Host => Game::host(self.tick_hz, map, self.host_bind)
                .map(Picked::Session)
                .map_err(|error| format!("CANNOT HOST: {error}")),
            Pick::Listed(row) => match self.joinable.get(row) {
                Some(host) => self.join(host.addr),
                None => Err("THAT HOST IS GONE".to_string()),
            },
            Pick::Connect => match self.address.text().trim().parse::<SocketAddr>() {
                Ok(addr) => self.join(addr),
                Err(_) => Err(format!("NOT AN IP:PORT: {:?}", self.address.text())),
            },
        };
        // Whatever this pick started — or failed to — replaces the join that
        // was under way, which the caller drops.
        self.joining = None;
        self.changed = true;
        match started {
            Ok(picked) => {
                if let Picked::Joining(joining) = &picked {
                    self.joining = joining.lan().host().map(|host| host.to_string());
                    self.notice = None;
                }
                Some(picked)
            }
            Err(why) => {
                self.notice = Some(why);
                None
            }
        }
    }

    /// A join to the host at `addr`, which waits for the host's map.
    fn join(&self, addr: SocketAddr) -> Result<Picked, String> {
        LanClient::join(self.session, addr, self.tick_hz)
            .map(|client| Picked::Joining(Joining::new(client, self.tick_hz, self.join_timeout)))
            .map_err(|error| format!("CANNOT JOIN {addr}: {error}"))
    }
}

#[cfg(test)]
mod tests;
