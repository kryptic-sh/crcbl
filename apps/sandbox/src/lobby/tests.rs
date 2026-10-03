//! The sandbox's lobby against real hosts on UDP loopback, with no window:
//! every socket binds `127.0.0.1:0`, and the browser queries the host's
//! announcer directly rather than the broadcast address. Each frame is
//! followed by a short pause for loopback to deliver, and every wait is
//! bounded by [`MAX_FRAMES`].

use std::net::{Ipv4Addr, SocketAddr};
use std::thread;
use std::time::Duration;

use crcbl::ecs::World;
use crcbl::lan::{LanGame, LanHost};
use crcbl::net::udp::discovery::{Browser, BrowserConfig};
use crcbl::net::{ProtocolCompatibility, SystemClock};
use crcbl::ui::menu::CaptionTone;

use super::*;

pub(crate) const TICK_HZ: u32 = 60;

/// One frame at [`TICK_HZ`].
pub(crate) const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The most frames any wait here runs.
pub(crate) const MAX_FRAMES: usize = 600;

/// The pause after each frame, for loopback to deliver.
pub(crate) const PAUSE: Duration = Duration::from_millis(1);

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// Both of a host's sockets on loopback, broadcasting nowhere.
pub(crate) fn on_loopback() -> LanBind {
    LanBind {
        listen: loopback(),
        announce_at: loopback(),
        broadcast_to: None,
    }
}

/// A host speaking `compatibility` on loopback, with nobody in it — the
/// engine's own, whose world the lobby never sees.
pub(crate) fn bare_host(compatibility: ProtocolCompatibility) -> LanHost {
    LanHost::open(
        LanGame {
            compatibility,
            ..SANDBOX
        },
        on_loopback(),
        World::new(),
        TICK_HZ,
    )
    .expect("loopback UDP must be available to these tests")
}

/// Where a joiner reaches `host`.
pub(crate) fn address(host: &LanHost) -> SocketAddr {
    (Ipv4Addr::LOCALHOST, host.game_port()).into()
}

/// A lobby whose browser queries `host`'s announcer, and which hosts on
/// loopback.
pub(crate) fn browsing(host: &LanHost) -> Lobby {
    let browser = Browser::bind_with(
        loopback(),
        BrowserConfig {
            query_to: host.announcer_addr(),
            ..BrowserConfig::new(SANDBOX.protocol_id)
        },
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    Lobby::new(
        lobby::Lobby::new(SANDBOX, Ok(browser)),
        on_loopback(),
        TICK_HZ,
    )
}

/// Serves `host` and polls `lobby` until `done` holds, failing past
/// [`MAX_FRAMES`].
fn until(host: &mut LanHost, lobby: &mut Lobby, what: &str, done: impl Fn(&Lobby) -> bool) {
    let mut now = Duration::ZERO;
    for _ in 0..MAX_FRAMES {
        if done(lobby) {
            return;
        }
        now += FRAME;
        host.frame(now);
        lobby.model_mut().poll();
        thread::sleep(PAUSE);
    }
    panic!("no {what} within {MAX_FRAMES} frames");
}

/// The rows of `menu`, with the hint each carries.
fn rows(menu: &Menu) -> Vec<(u64, String, String)> {
    menu.items()
        .iter()
        .map(|item| (item.id, item.label.clone(), item.hint.clone()))
        .collect()
}

/// **A host announced on loopback is a row**, between host and connect,
/// saying who it is and how many players it holds; until it is heard the
/// lobby says it is looking.
#[test]
fn the_lobby_lists_a_host_announced_on_loopback() {
    let mut host = bare_host(SANDBOX.compatibility);
    let mut lobby = browsing(&host);
    assert_eq!(
        lobby.menu().subtitle,
        vec![Caption::hint("LOOKING FOR SANDBOX HOSTS")]
    );
    until(&mut host, &mut lobby, "listed host", |lobby| {
        !lobby.model.joinable().is_empty()
    });
    assert_eq!(
        rows(&lobby.menu()),
        vec![
            (OFFLINE_ID, "OFFLINE".into(), String::new()),
            (HOST_ID, "HOST".into(), "LAN".into()),
            (FIRST_LISTED_ID, "JOIN crcbl sandbox".into(), "0/8".into()),
            (CONNECT_ID, "CONNECT".into(), "TYPE IP:PORT".into()),
        ]
    );
    assert!(
        lobby.menu().subtitle.is_empty(),
        "{:?}",
        lobby.menu().subtitle
    );
}

/// **Picking the listed host starts a join to its address**, which the lobby
/// names while it waits.
#[test]
fn picking_a_listed_host_joins_it() {
    let mut host = bare_host(SANDBOX.compatibility);
    let mut lobby = browsing(&host);
    until(&mut host, &mut lobby, "listed host", |lobby| {
        !lobby.model.joinable().is_empty()
    });
    let Some(Started::Joining(lan)) = lobby.pick(LobbyPick::Listed(0)) else {
        panic!("the listed host started no join");
    };
    assert_eq!(lan.joined(), Some(address(&host)));
    assert!(
        lobby
            .menu()
            .subtitle
            .contains(&Caption::hint(format!("JOINING {}", address(&host)))),
        "{:?}",
        lobby.menu().subtitle
    );
}

/// **A host of another game is a dimmed line with its reason, and not a
/// row**: a pick of a listed row starts nothing, and says so.
#[test]
fn an_incompatible_host_is_listed_dimmed_and_not_joinable() {
    let mut host = bare_host(ProtocolCompatibility {
        schema_hash: SANDBOX.compatibility.schema_hash + 1,
        ..SANDBOX.compatibility
    });
    let mut lobby = browsing(&host);
    until(&mut host, &mut lobby, "passed-over host", |lobby| {
        !lobby.model.passed_over().is_empty()
    });
    let menu = lobby.menu();
    let line = menu
        .subtitle
        .iter()
        .find(|line| line.text.ends_with("ANOTHER GAME"))
        .unwrap_or_else(|| panic!("no line for the host: {:?}", menu.subtitle));
    assert_eq!(line.tone, CaptionTone::Hint, "dimmed: the hint colour");
    assert!(line.text.contains(&address(&host).port().to_string()));
    assert_eq!(
        menu.items().len(),
        3,
        "the host is a row: {:?}",
        rows(&menu)
    );

    assert!(lobby.pick(LobbyPick::Listed(0)).is_none());
    assert!(
        lobby
            .menu()
            .subtitle
            .contains(&Caption::warning("THAT HOST IS GONE")),
        "{:?}",
        lobby.menu().subtitle
    );
}

/// **An address that is not an `IP:PORT` is refused by name**: the warning
/// quotes what was typed, and nothing starts.
#[test]
fn a_bad_address_is_refused_by_name() {
    let mut lobby = Lobby::new(
        lobby::Lobby::new(SANDBOX, Err("not looking".to_string())),
        on_loopback(),
        TICK_HZ,
    );
    lobby.model_mut().text("10.0.0.300:5000");
    assert_eq!(lobby.connect_hint(), "10.0.0.300:5000");
    assert!(lobby.pick(LobbyPick::Connect).is_none());
    assert!(
        lobby
            .menu()
            .subtitle
            .contains(&Caption::warning("NOT AN IP:PORT: \"10.0.0.300:5000\"")),
        "{:?}",
        lobby.menu().subtitle
    );
}
