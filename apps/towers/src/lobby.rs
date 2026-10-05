//! The lobby: what a native towers opens on when the command line did not
//! already choose a session.
//!
//! ```text
//!   ┌────────────────────────────────────────┐
//!   │                 TOWERS                 │
//!   │  crcbl towers 10.0.0.7:5000 1/4 ANOTHER VERSION  ← heard, not joinable
//!   │  CONTINUE                    WAVE 3/10 │  ← only with a save
//!   │  SOLO                                  │
//!   │  HOST                              LAN │
//!   │  JOIN crcbl towers                 1/4 │  ← one row per host
//!   │  CONNECT                  10.0.0.9:500 │  ← what has been typed
//!   └────────────────────────────────────────┘
//! ```
//!
//! **Continue**, when there is a saved run to continue (`crate::save`) —
//! first, because a player coming back is the likeliest pick — then
//! **solo**, **host** (what `--host` does, on any free port) and a row per
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
//! **A save that is there and will not be resumed is not a row**: it is a
//! warning line naming why — another map, another version, a corrupt file —
//! so a player who expected to continue is told rather than shown nothing.
//!
//! # What it knows is the engine's, how it looks is towers'
//!
//! Which hosts are rows and which are lines, the typed address, which host
//! a pick means and what last went wrong are [`crcbl::lan::lobby`]'s, which
//! the sandbox's lobby reads too. The rows, their words, the solo run under
//! them and the map a host plays are towers', here.
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
use crcbl::lan::lobby::{self, LobbyChoice, LobbyNotice, LobbyPick, PickRefused};
use crcbl::lan::{LanBind, LanClient, LanGame};
use crcbl::net::PlayerId;
use crcbl::net::udp::discovery::Browser;
use crcbl::ui::menu::{Caption, Menu, MenuItem};

pub use crcbl::lan::lobby::Unjoinable;

use crate::game::Game;
use crate::lan::{JOIN_TIMEOUT, Joining};
use crate::map::Map;
use crate::menu::{CONNECT_ID, CONTINUE_ID, FIRST_LISTED_ID, HOST_ID, SOLO_ID};
use crate::save::{Checkpoint, SaveError};
use crate::wave::WAVES;

/// The lobby's heading.
pub const TITLE: &str = "TOWERS";

/// The heading of the panel a join the command line asked for waits under —
/// the lobby's own joins wait in the lobby.
pub const JOINING_TITLE: &str = "JOINING";

/// What a lobby row asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Play the saved run on, alone.
    Continue,
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
    /// The saved run, to be put in place of the solo run under the lobby,
    /// which has not ticked.
    Continue(Checkpoint),
    /// A LAN session this player hosts.
    Session(Game),
    /// A join, waiting for the host's map.
    Joining(Joining),
}

/// The lobby: the engine's [`lobby::Lobby`] — the browser, the hosts it
/// heard, the address being typed, and the last thing that went wrong — and
/// what towers starts for a pick.
#[derive(Debug)]
pub struct Lobby {
    /// What the lobby knows.
    model: lobby::Lobby,
    /// Where [`Pick::Host`] binds: every interface, as `--host` does, outside
    /// the tests.
    host_bind: LanBind,
    /// Who this player hosts and joins as, or why there is no telling — which
    /// a host or join pick shows, leaving solo and continue to play.
    player: Result<PlayerId, String>,
    tick_hz: u32,
    /// How long a chosen host has to send its map: [`JOIN_TIMEOUT`], unless a
    /// test asked for less.
    join_timeout: Duration,
    /// The saved run *Continue* resumes, if there is one.
    saved: Option<Checkpoint>,
    /// Why a save that is there will not be resumed, shown under the title.
    unresumable: Option<String>,
}

impl Lobby {
    /// A lobby looking for hosts of `session` on the LAN — a [`Browser`]
    /// querying the broadcast address — hosting where `--host` does, at
    /// `tick_hz`, as the player id this machine keeps for towers. A browser
    /// that cannot bind is shown as a warning rather than refused: solo, host
    /// and connect need none. Nor is an id that cannot be had: a host or a
    /// join picked says why, and solo plays on.
    #[must_use]
    pub fn on_the_lan(session: LanGame, tick_hz: u32) -> Self {
        let browser = Browser::open(session.protocol_id)
            .map_err(|error| format!("NOT LOOKING FOR HOSTS: {error}"));
        let player = session.player_id(false).map_err(|error| {
            crcbl::log::warn!("lobby: {error}");
            format!("CANNOT PLAY ON THE LAN: {error}")
        });
        Self::new(session, player, browser, LanBind::on_the_lan(0), tick_hz)
    }

    /// A lobby listening with `browser` and hosting on `host_bind`, hosting
    /// and joining as `player`.
    #[must_use]
    pub fn new(
        session: LanGame,
        player: Result<PlayerId, String>,
        browser: Result<Browser, String>,
        host_bind: LanBind,
        tick_hz: u32,
    ) -> Self {
        Self {
            model: lobby::Lobby::new(session, browser),
            host_bind,
            player,
            tick_hz,
            join_timeout: JOIN_TIMEOUT,
            saved: None,
            unresumable: None,
        }
    }

    /// This lobby, offering what reading the saved run answered: a
    /// *Continue* row for a run, a warning line for a save refused by name,
    /// and nothing for no save at all.
    #[must_use]
    pub fn offering(mut self, saved: Result<Option<Checkpoint>, SaveError>) -> Self {
        match saved {
            Ok(saved) => self.saved = saved,
            Err(error) => {
                crcbl::log::warn!("save: not offered to continue: {error}");
                self.unresumable = Some(format!("SAVE NOT RESUMED: {error}"));
            }
        }
        self
    }

    /// The saved run *Continue* picked would not go in under the lobby: the
    /// lobby says why, and stops offering it.
    pub(crate) fn continue_failed(&mut self, error: &SaveError) {
        self.saved = None;
        self.model.start_failed(format!("CANNOT CONTINUE: {error}"));
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
        self.model.poll();
    }

    /// Text typed while the lobby is up: it goes on the end of the address.
    pub fn text(&mut self, text: &str) {
        self.model.text(text);
    }

    /// A key the menu did not take. Backspace takes a character off the
    /// address; nothing else here means anything.
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        self.model.key(key, pressed);
    }

    /// The address typed so far.
    #[must_use]
    pub fn address(&self) -> &str {
        self.model.address()
    }

    /// What [`Pick::Connect`]'s row shows where a key hint would be.
    #[must_use]
    pub fn connect_hint(&self) -> String {
        if self.model.address().is_empty() {
            "TYPE IP:PORT".to_string()
        } else {
            self.model.address().to_string()
        }
    }

    /// Whether the menu is stale, clearing it — the rows or the lines moved,
    /// or a pick failed and there is a warning to show.
    pub fn take_changed(&mut self) -> bool {
        self.model.take_changed()
    }

    /// Whether text arrived since the last ask, clearing it: the menu moves
    /// its selection onto the connect row, which is where it is going.
    pub fn take_typed(&mut self) -> bool {
        self.model.take_typed()
    }

    /// The panel: the rows, and under the title the lines and the warning.
    #[must_use]
    pub fn menu(&self) -> Menu {
        let mut items = Vec::new();
        if let Some(saved) = &self.saved {
            items.push(MenuItem::new(
                CONTINUE_ID,
                "CONTINUE",
                format!("WAVE {}/{}", saved.wave(), WAVES.len()),
            ));
        }
        items.push(MenuItem::new(SOLO_ID, "SOLO", ""));
        items.push(MenuItem::new(HOST_ID, "HOST", "LAN"));
        for (id, host) in (FIRST_LISTED_ID..).zip(self.model.joinable()) {
            items.push(MenuItem::new(
                id,
                format!("JOIN {}", host.name),
                format!("{}/{}", host.players, host.max_players),
            ));
        }
        items.push(MenuItem::new(CONNECT_ID, "CONNECT", self.connect_hint()));
        let mut menu = Menu::new(TITLE, items);
        if let Some(why) = &self.unresumable {
            menu.subtitle.push(Caption::warning(why.clone()));
        }
        match self.model.browser_error() {
            Some(why) => menu.subtitle.push(Caption::warning(why.to_string())),
            None if self.model.joinable().is_empty() && self.model.passed_over().is_empty() => {
                menu.subtitle.push("LOOKING FOR HOSTS ON THE LAN".into());
            }
            None => {}
        }
        for (host, why) in self.model.passed_over() {
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
        if let Some(host) = self.model.joining() {
            menu.subtitle.push(format!("JOINING {host}").into());
        }
        if let Some(notice) = self.model.notice() {
            menu.subtitle.push(Caption::warning(notice_line(notice)));
        }
        menu
    }

    /// The join a pick started ended without a game: the lobby says why, and
    /// is what the player picks from again.
    pub(crate) fn join_failed(&mut self, why: &str) {
        self.model.join_failed(why);
    }

    /// The session a join from here started has ended, and the player is
    /// back: the lobby says how, and is what the player picks from again.
    pub(crate) fn session_ended(&mut self, how: &str) {
        self.model.session_ended(how);
    }

    /// Starts what `pick` asks for — a host on `map`, the local one — or
    /// records why it could not. A join does not use `map`: it plays on the
    /// host's.
    pub(crate) fn pick(&mut self, pick: Pick, map: &Map) -> Option<Picked> {
        let pick = match pick {
            Pick::Continue => {
                self.model.clear_joining();
                let Some(saved) = self.saved.clone() else {
                    self.model.start_failed("THERE IS NO SAVED RUN".to_string());
                    return None;
                };
                return Some(Picked::Continue(saved));
            }
            Pick::Solo => {
                self.model.clear_joining();
                return Some(Picked::Solo);
            }
            Pick::Host => LobbyPick::Host,
            Pick::Listed(row) => LobbyPick::Listed(row),
            Pick::Connect => LobbyPick::Connect,
        };
        // Whatever this pick starts — or fails to — replaces the join that
        // was under way, which the caller drops; a refusal is the notice.
        let choice = self.model.pick(pick).ok()?;
        let started = self.player.clone().and_then(|player| match choice {
            LobbyChoice::Host => Game::host(self.tick_hz, map, self.host_bind, None, player)
                .map(Picked::Session)
                .map_err(|error| format!("CANNOT HOST: {error}")),
            LobbyChoice::Join(addr) => self.join(player, addr),
        });
        match started {
            Ok(picked) => {
                if let Picked::Joining(joining) = &picked
                    && let Some(host) = joining.lan().host()
                {
                    self.model.join_started(host);
                }
                Some(picked)
            }
            Err(why) => {
                self.model.start_failed(why);
                None
            }
        }
    }

    /// A join to the host at `addr` as `player`, which waits for the host's
    /// map.
    fn join(&self, player: PlayerId, addr: SocketAddr) -> Result<Picked, String> {
        LanClient::join(self.model.session(), player, addr, self.tick_hz)
            .map(|client| Picked::Joining(Joining::new(client, self.tick_hz, self.join_timeout)))
            .map_err(|error| format!("CANNOT JOIN {addr}: {error}"))
    }
}

/// The warning line `notice` is, in towers' words.
fn notice_line(notice: &LobbyNotice) -> String {
    match notice {
        LobbyNotice::Refused(PickRefused::HostGone) => "THAT HOST IS GONE".to_string(),
        LobbyNotice::Refused(PickRefused::NotAnAddress(typed)) => {
            format!("NOT AN IP:PORT: {typed:?}")
        }
        LobbyNotice::Refused(PickRefused::NotAHost(addr)) => format!("NO HOST AT {addr}"),
        LobbyNotice::CannotStart(why) => why.clone(),
        LobbyNotice::JoinFailed(why) => format!("JOIN FAILED: {why}"),
        LobbyNotice::SessionEnded(how) => format!("SESSION ENDED: {how}"),
    }
}

#[cfg(test)]
mod tests;
