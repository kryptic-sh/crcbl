//! The lobby against real hosts on UDP loopback, with no window: every
//! socket binds `127.0.0.1:0`, and the browser queries the host's announcer
//! directly, as `crate::lan::tests` does — whose helpers these are.

use std::net::{Ipv4Addr, SocketAddr};
use std::thread;
use std::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::ecs::World;
use crcbl::lan::LanHost;
use crcbl::net::ProtocolCompatibility;
use crcbl::net::udp::discovery::HostEntry;
use crcbl::ui::menu::CaptionTone;

use super::*;
use crate::lan::tests::{
    FRAME, MAX_FRAMES, PAUSE, TICK_HZ, another_map, loopback_browser, on_loopback,
};
use crate::lan::{Progress, SESSION};

/// A host on loopback playing `map`, with a player of its own.
fn host_on(map: &Map) -> Game {
    Game::host(TICK_HZ, map, on_loopback(), None)
        .expect("loopback UDP must be available to these tests")
}

/// Where `host` announces.
fn announcer(host: &Game) -> SocketAddr {
    host.lan_host()
        .and_then(LanHost::announcer_addr)
        .expect("the host announces")
}

/// Where a joiner reaches `host`.
fn address(host: &Game) -> SocketAddr {
    let port = host.lan_host().expect("a host").game_port();
    (Ipv4Addr::LOCALHOST, port).into()
}

/// A lobby on the committed field, browsing `host`'s announcer.
fn lobby_browsing(host: &Game) -> Lobby {
    Lobby::new(
        SESSION,
        Ok(loopback_browser(announcer(host))),
        on_loopback(),
        TICK_HZ,
    )
}

/// A lobby that is not browsing at all, for what needs no host.
fn lobby_alone() -> Lobby {
    Lobby::new(
        SESSION,
        Err("NOT LOOKING".to_string()),
        on_loopback(),
        TICK_HZ,
    )
}

/// Runs `step` — the host's frame — and polls `lobby` until `done` holds,
/// failing past [`MAX_FRAMES`].
fn until(mut step: impl FnMut(), lobby: &mut Lobby, what: &str, done: impl Fn(&Lobby) -> bool) {
    for _ in 0..MAX_FRAMES {
        if done(lobby) {
            return;
        }
        step();
        lobby.poll();
        thread::sleep(PAUSE);
    }
    panic!("no {what} within {MAX_FRAMES} frames");
}

/// The step that runs `host` — its own player's tick and its frame.
fn serving(host: &mut Game) -> impl FnMut() + '_ {
    move || {
        host.tick();
        host.frame(FRAME);
    }
}

/// A host speaking `compatibility` on loopback with nobody of its own — the
/// engine's alone, for a host of another version.
fn bare_host(compatibility: ProtocolCompatibility) -> LanHost {
    LanHost::open(
        crcbl::lan::LanGame {
            compatibility,
            ..SESSION
        },
        on_loopback(),
        World::new(),
        TICK_HZ,
    )
    .expect("loopback UDP must be available to these tests")
}

/// The labels of `menu`'s rows, with the hint each carries.
fn rows(menu: &Menu) -> Vec<(u64, String, String)> {
    menu.items()
        .iter()
        .map(|item| (item.id, item.label.clone(), item.hint.clone()))
        .collect()
}

/// **A host announced on loopback is a row**, between host and connect,
/// saying who it is and how many players it holds — the host's own is one of
/// four.
#[test]
fn the_lobby_lists_a_host_announced_on_loopback() {
    let mut host = host_on(&Map::built_in());
    let mut lobby = lobby_browsing(&host);
    assert_eq!(
        lobby.menu().subtitle,
        vec![Caption::hint("LOOKING FOR HOSTS ON THE LAN")],
        "a browsing lobby that has heard nothing says it is looking"
    );
    until(serving(&mut host), &mut lobby, "listed host", |lobby| {
        lobby.menu().items().len() > 3
    });
    assert_eq!(
        rows(&lobby.menu()),
        vec![
            (SOLO_ID, "SOLO".into(), String::new()),
            (HOST_ID, "HOST".into(), "LAN".into()),
            (FIRST_LISTED_ID, "JOIN crcbl towers".into(), "1/4".into()),
            (CONNECT_ID, "CONNECT".into(), "TYPE IP:PORT".into()),
        ]
    );
    assert!(lobby.take_changed(), "a new row is a menu to rebuild");
}

/// **Choosing the listed host joins it**: the pick starts a join to that
/// host's address, which the lobby names while it waits, and the game it
/// ends in is on the host's map and gets into the session — the host on
/// [`another_map`], which the lobby's own is not.
#[test]
fn choosing_a_listed_host_joins_it() {
    let mut host = host_on(&another_map());
    let mut lobby = lobby_browsing(&host);
    until(serving(&mut host), &mut lobby, "listed host", |lobby| {
        lobby.menu().items().len() > 3
    });
    let Some(Picked::Joining(joining)) = lobby.pick(Pick::Listed(0), &Map::built_in()) else {
        panic!("the listed host did not start a join");
    };
    assert_eq!(joining.lan().host(), Some(address(&host)));
    let waiting = format!("JOINING {}", address(&host));
    assert!(
        lobby
            .menu()
            .subtitle
            .iter()
            .any(|line| line.text == waiting),
        "{:?}",
        lobby.menu().subtitle
    );
    let mut joiner = loop_until_joined(&mut host, joining);
    assert_eq!(joiner.map(), &another_map(), "not on the host's map");
    for _ in 0..MAX_FRAMES {
        if joiner.stats().ticks > 0 {
            return;
        }
        host.tick();
        joiner.tick();
        host.frame(FRAME);
        joiner.frame(FRAME);
        thread::sleep(PAUSE);
    }
    panic!("the joiner never got into the host's session");
}

/// Steps `host` and `joining` until the host's map makes the game, failing
/// past [`MAX_FRAMES`] or on a failed join.
fn loop_until_joined(host: &mut Game, joining: Joining) -> Game {
    let mut waiting = Some(joining);
    for _ in 0..MAX_FRAMES {
        host.tick();
        host.frame(FRAME);
        match waiting.take().expect("still joining").step(FRAME) {
            Progress::Waiting(next) => waiting = Some(next),
            Progress::Joined(game) => return game,
            Progress::Failed(failure) => panic!("the join failed: {failure}"),
        }
        thread::sleep(PAUSE);
    }
    panic!("no map from the host within {MAX_FRAMES} frames");
}

/// **A host of another version is a dimmed line with its reason, and not a
/// row**: nothing Enter can fire, and a pick of a row that is not there
/// starts nothing and says so.
#[test]
fn an_incompatible_host_is_listed_dimmed_and_not_joinable() {
    let mut old = bare_host(ProtocolCompatibility {
        protocol_version: SESSION.compatibility.protocol_version - 1,
        ..SESSION.compatibility
    });
    let announcer = old.announcer_addr().expect("the host announces");
    let port = old.game_port();
    let mut lobby = Lobby::new(
        SESSION,
        Ok(loopback_browser(announcer)),
        on_loopback(),
        TICK_HZ,
    );
    let mut now = Duration::ZERO;
    until(
        || {
            now += FRAME;
            old.frame(now);
        },
        &mut lobby,
        "passed-over host",
        |lobby| {
            lobby
                .menu()
                .subtitle
                .iter()
                .any(|line| line.text.contains("ANOTHER VERSION"))
        },
    );
    let menu = lobby.menu();
    let line = menu
        .subtitle
        .iter()
        .find(|line| line.text.contains("ANOTHER VERSION"))
        .expect("the line");
    assert_eq!(line.tone, CaptionTone::Hint, "dimmed: the hint colour");
    assert!(line.text.contains(&format!(":{port}")), "{}", line.text);
    assert!(
        menu.items()
            .iter()
            .all(|item| !item.label.starts_with("JOIN")),
        "a host of another version is a row: {:?}",
        rows(&menu)
    );

    assert!(lobby.pick(Pick::Listed(0), &Map::built_in()).is_none());
    let warned = lobby.menu();
    assert!(
        warned
            .subtitle
            .iter()
            .any(|line| line.tone == CaptionTone::Warning && line.text == "THAT HOST IS GONE"),
        "{:?}",
        warned.subtitle
    );
}

/// A host entry announcing `compatibility`, with `players` of four in.
fn entry(compatibility: ProtocolCompatibility, players: u16) -> HostEntry {
    HostEntry {
        name: "crcbl towers".into(),
        addr: (Ipv4Addr::LOCALHOST, 1).into(),
        players,
        max_players: crate::lan::MAX_PLAYERS,
        compatibility,
        last_seen: Duration::ZERO,
    }
}

/// **Each reason a host is passed over is named**, in the handshake's order,
/// and a host with room on this session is joinable — whatever its map,
/// which is no part of what it announces.
#[test]
fn a_host_is_passed_over_for_its_version_its_build_its_game_or_its_room() {
    let ours = SESSION.compatibility;
    assert_eq!(Unjoinable::of(ours, &entry(ours, 1)), None);
    let cases = [
        (
            ProtocolCompatibility {
                protocol_version: ours.protocol_version + 1,
                ..ours
            },
            Unjoinable::Version,
        ),
        (
            ProtocolCompatibility {
                engine_build_id: ours.engine_build_id + 1,
                ..ours
            },
            Unjoinable::Build,
        ),
        (
            ProtocolCompatibility {
                schema_hash: ours.schema_hash ^ 0b10,
                ..ours
            },
            Unjoinable::Game,
        ),
    ];
    for (theirs, why) in cases {
        assert_eq!(Unjoinable::of(ours, &entry(theirs, 1)), Some(why));
    }
    assert_eq!(
        Unjoinable::of(ours, &entry(ours, crate::lan::MAX_PLAYERS)),
        Some(Unjoinable::Full)
    );
    assert_eq!(Unjoinable::Game.label(), "ANOTHER GAME");
}

/// **The connect field parses what was typed, and refuses what is not an
/// `IP:PORT`** — by showing why, and starting nothing — then joins the
/// address once it is one. Backspace takes a character off it, and the typing
/// is reported so the menu can move onto the row.
#[test]
fn the_connect_field_parses_and_refuses_a_bad_address() {
    let mut host = host_on(&Map::built_in());
    let mut lobby = lobby_alone();
    assert_eq!(lobby.connect_hint(), "TYPE IP:PORT");
    assert!(!lobby.take_typed());

    lobby.text("127.0.0.1:");
    lobby.text("x");
    assert!(lobby.take_typed(), "typing was not reported");
    assert!(lobby.pick(Pick::Connect, &Map::built_in()).is_none());
    assert!(lobby.take_changed(), "the warning needs the menu rebuilt");
    let menu = lobby.menu();
    assert!(
        menu.subtitle
            .iter()
            .any(|line| line.tone == CaptionTone::Warning
                && line.text == "NOT AN IP:PORT: \"127.0.0.1:x\""),
        "{:?}",
        menu.subtitle
    );

    lobby.key(KeyCode::Backspace, true);
    lobby.key(KeyCode::Backspace, false);
    assert_eq!(lobby.address(), "127.0.0.1:", "one press, one character");
    lobby.text(&address(&host).port().to_string());
    assert_eq!(lobby.connect_hint(), address(&host).to_string());
    let Some(Picked::Joining(joining)) = lobby.pick(Pick::Connect, &Map::built_in()) else {
        panic!("a good address started nothing");
    };
    assert_eq!(joining.lan().host(), Some(address(&host)));
    host.frame(FRAME);
}

/// **Solo is the run already under the lobby, and host hosts** — on the
/// lobby's own bind, which here is loopback.
#[test]
fn solo_keeps_the_run_and_host_hosts() {
    let mut lobby = lobby_alone();
    assert!(matches!(
        lobby.pick(Pick::Solo, &Map::built_in()),
        Some(Picked::Solo)
    ));
    let Some(Picked::Session(game)) = lobby.pick(Pick::Host, &Map::built_in()) else {
        panic!("hosting started nothing");
    };
    let host = game.lan_host().expect("a host");
    assert_eq!(host.host().peer_count(), 1, "the host's own player is in");
}

/// The ids of `menu`'s rows, in order.
fn row_ids(menu: &Menu) -> Vec<crcbl::ui::WidgetId> {
    menu.items().iter().map(|item| item.id).collect()
}

/// **The lobby offers *Continue* exactly when there is a saved run**: first,
/// with the wave it was saved at, when reading the save found one; not at
/// all when there was none; and as a warning line naming why, not a row, for
/// a save that is there and refused.
#[test]
fn the_lobby_offers_continue_only_when_there_is_a_save() {
    use crate::menu::CONTINUE_ID;

    let without = lobby_alone().offering(Ok(None)).menu();
    assert!(
        !row_ids(&without).contains(&CONTINUE_ID),
        "continue offered with no save"
    );
    assert_eq!(row_ids(&without)[0], SOLO_ID);

    let saved = crate::save::tests::a_first_waves_end();
    let with = lobby_alone().offering(Ok(Some(saved.clone()))).menu();
    assert_eq!(
        row_ids(&with)[..2],
        [CONTINUE_ID, SOLO_ID],
        "continue is not the first row"
    );
    assert_eq!(
        with.items()[0].hint,
        format!("WAVE {}/{}", saved.wave(), crate::wave::WAVES.len())
    );

    let refused = lobby_alone()
        .offering(Err(crate::save::SaveError::OtherMap))
        .menu();
    assert!(!row_ids(&refused).contains(&CONTINUE_ID));
    assert!(
        refused
            .subtitle
            .iter()
            .any(|line| line.tone == CaptionTone::Warning && line.text.contains("another map")),
        "the refused save was not named: {:?}",
        refused.subtitle
    );
}
