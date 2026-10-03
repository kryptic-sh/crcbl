//! The lobby: what a native sandbox opens on when the command line chose no
//! session.
//!
//! ```text
//!   ┌──────────────────────────────────────────┐
//!   │                   LAN                    │
//!   │  crcbl sandbox 10.0.0.7:5000 1/8 FULL    │  ← heard, not joinable
//!   │  OFFLINE                                 │
//!   │  HOST                                LAN │
//!   │  JOIN crcbl sandbox                  1/8 │  ← one row per host
//!   │  CONNECT                    10.0.0.9:500 │  ← what has been typed
//!   └──────────────────────────────────────────┘
//! ```
//!
//! What it knows is [`crcbl::lan::lobby`]'s — the hosts a browser heard,
//! which of them this build can join and why not, the typed address, which
//! host a pick means, and why the last join failed or session ended — as
//! towers' lobby's is. What is here is the sandbox's look, and what each row
//! starts: **offline** is the sandbox as it always ran, with no session;
//! **host** is what `--host` does, on any free port; a **join** row or
//! **connect** is what `--join` does, to the host listed or the address
//! typed.
//!
//! # A join waits in the lobby, and a session that ends comes back to it
//!
//! A join keeps the lobby on screen, saying `JOINING` and where, until the
//! host admits this player — `crate::app` watches the [`Lan`] it started.
//! When the host refuses it or the link ends first, the lobby says why and
//! the player picks again. Once the session is up the lobby is set aside, not
//! polled, until it ends; then it is back, saying how.
//!
//! # Native only, and only when asked for nothing
//!
//! The web build has no networking and no lobby. A native run opens on one
//! only when its command line chose no session and is not a script — see
//! `crate::args` — so every headless run and every scripted windowed one
//! starts where it always did.

use std::net::SocketAddr;

use crcbl::lan::LanBind;
use crcbl::lan::lobby::{self, LobbyChoice, LobbyNotice, LobbyPick, PickRefused};
use crcbl::ui::menu::{Caption, Menu, MenuItem};

use crate::lan::{Lan, SANDBOX};
use crate::menu::{CONNECT_ID, FIRST_LISTED_ID, HOST_ID, OFFLINE_ID};

/// The lobby's heading.
pub const TITLE: &str = "LAN";

/// The sandbox's lobby: the engine's model, where a host binds, and the
/// rate a session it starts ticks at.
#[derive(Debug)]
pub struct Lobby {
    /// What the lobby knows.
    model: lobby::Lobby,
    /// Where a host binds: every interface, as `--host` does, outside the
    /// tests.
    host_bind: LanBind,
    tick_hz: u32,
}

impl Lobby {
    /// A lobby looking for sandbox hosts on the LAN, hosting where `--host`
    /// does, at `tick_hz`.
    #[must_use]
    pub fn on_the_lan(tick_hz: u32) -> Self {
        Self::new(
            lobby::Lobby::on_the_lan(SANDBOX),
            LanBind::on_the_lan(0),
            tick_hz,
        )
    }

    /// A lobby knowing what `model` does, hosting on `host_bind`.
    #[must_use]
    pub const fn new(model: lobby::Lobby, host_bind: LanBind, tick_hz: u32) -> Self {
        Self {
            model,
            host_bind,
            tick_hz,
        }
    }

    /// What the lobby knows, for the frame to read and feed.
    pub const fn model_mut(&mut self) -> &mut lobby::Lobby {
        &mut self.model
    }

    /// What the connect row shows where a key hint would be.
    #[must_use]
    pub fn connect_hint(&self) -> String {
        if self.model.address().is_empty() {
            "TYPE IP:PORT".to_string()
        } else {
            self.model.address().to_string()
        }
    }

    /// The panel: the rows, and under the title the hosts it cannot join,
    /// the join under way and the last thing that went wrong.
    #[must_use]
    pub fn menu(&self) -> Menu {
        let mut items = vec![
            MenuItem::new(OFFLINE_ID, "OFFLINE", ""),
            MenuItem::new(HOST_ID, "HOST", "LAN"),
        ];
        for (id, host) in (FIRST_LISTED_ID..).zip(self.model.joinable()) {
            items.push(MenuItem::new(
                id,
                format!("JOIN {}", host.name),
                format!("{}/{}", host.players, host.max_players),
            ));
        }
        items.push(MenuItem::new(CONNECT_ID, "CONNECT", self.connect_hint()));
        let mut menu = Menu::new(TITLE, items);
        match self.model.browser_error() {
            Some(why) => menu.subtitle.push(Caption::warning(why.to_string())),
            None if self.model.joinable().is_empty() => {
                menu.subtitle.push("LOOKING FOR SANDBOX HOSTS".into());
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

    /// Starts what `pick` asks for, or records why it could not: the session,
    /// and whether it is a join — which keeps the lobby up until the host
    /// admits this player.
    pub fn pick(&mut self, pick: LobbyPick) -> Option<Started> {
        let started = match self.model.pick(pick).ok()? {
            LobbyChoice::Host => Lan::host(self.host_bind, self.tick_hz, None)
                .map(Started::Hosting)
                .map_err(|error| format!("CANNOT HOST: {error}")),
            LobbyChoice::Join(addr) => self.join(addr),
        };
        match started {
            Ok(started) => Some(started),
            Err(why) => {
                self.model.start_failed(why);
                None
            }
        }
    }

    /// A join to the host at `addr`, which the lobby names while it waits.
    fn join(&mut self, addr: SocketAddr) -> Result<Started, String> {
        let lan = Lan::join(addr, self.tick_hz)
            .map_err(|error| format!("CANNOT JOIN {addr}: {error}"))?;
        self.model.join_started(addr);
        Ok(Started::Joining(lan))
    }
}

/// What a pick started.
#[derive(Debug)]
pub enum Started {
    /// A session this sandbox hosts: the lobby's work is done.
    Hosting(Lan),
    /// A join, which the lobby stays up for until the host admits it.
    Joining(Lan),
}

/// The warning line `notice` is, in the sandbox's words.
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
pub(crate) mod tests;
