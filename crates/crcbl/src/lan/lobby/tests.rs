//! The lobby's model against real announcers on UDP loopback, with no game
//! and no window: every socket binds `127.0.0.1:0`, and the browser's
//! queries go straight to each announcer rather than to a broadcast address.
//! Loopback delivery is asynchronous, so each poll is followed by a short
//! pause, and every wait is bounded by [`MAX_POLLS`].

use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU16;
use std::thread;
use std::time::Duration;

use super::*;
use crate::net::SystemClock;
use crate::net::udp::discovery::{Announcement, Announcer, BrowserConfig};

/// The most polls any wait here runs.
const MAX_POLLS: usize = 600;

/// The pause after each poll, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// A session for these tests alone.
const SESSION: LanGame = LanGame {
    app: "lobby-test",
    host_name: "lobby test",
    protocol_id: u32::from_be_bytes(*b"LOBY"),
    compatibility: ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 7,
        schema_hash: 11,
    },
    max_players: 4,
};

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// An announcer on loopback for `name`, whose game listens on `port`,
/// speaking `compatibility` with `players` of [`SESSION`]'s maximum in.
fn announcer(
    name: &str,
    port: u16,
    compatibility: ProtocolCompatibility,
    players: u16,
) -> Announcer {
    let mut announcement = Announcement::new(
        SESSION.protocol_id,
        NonZeroU16::new(port).expect("a game port"),
        compatibility,
        name,
    );
    announcement.players = players;
    announcement.max_players = SESSION.max_players;
    Announcer::bind_with(loopback(), announcement, None, SystemClock::new())
        .expect("loopback UDP must be available to these tests")
}

/// A browser on loopback that queries nothing by itself: [`listen`] asks
/// each announcer directly.
fn browser() -> Browser {
    Browser::bind_with(
        loopback(),
        BrowserConfig::new(SESSION.protocol_id),
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests")
}

/// A lobby that is not browsing at all, for what needs no host.
fn alone() -> Lobby {
    Lobby::new(SESSION, Err("not looking".to_string()))
}

/// Queries every one of `announcers` from `lobby`'s browser, answers, and
/// polls until `done` holds, failing past [`MAX_POLLS`].
fn listen(
    lobby: &mut Lobby,
    announcers: &mut [Announcer],
    what: &str,
    done: impl Fn(&Lobby) -> bool,
) {
    let addresses: Vec<SocketAddr> = announcers
        .iter()
        .map(|announcer| announcer.local_addr().expect("announcer address"))
        .collect();
    for _ in 0..MAX_POLLS {
        if done(lobby) {
            return;
        }
        if let Ok(browser) = &mut lobby.browser {
            for &to in &addresses {
                browser.query(to);
            }
        }
        thread::sleep(PAUSE);
        for announcer in announcers.iter_mut() {
            announcer.poll();
        }
        thread::sleep(PAUSE);
        lobby.poll();
    }
    panic!("no {what} within {MAX_POLLS} polls");
}

/// **A host is judged by what it announces**: one this session can join is
/// a row, and one of another build or one that is full is a line with the
/// reason — sorted by the browser's own order — and hearing the same hosts
/// again, a moment later, is no change for a menu to rebuild.
#[test]
fn the_hosts_heard_are_sorted_into_joinable_and_passed_over_with_why() {
    let ours = SESSION.compatibility;
    let other_build = ProtocolCompatibility {
        engine_build_id: ours.engine_build_id + 1,
        ..ours
    };
    let mut announcers = [
        announcer("a open", 5001, ours, 1),
        announcer("b elsewhere", 5002, other_build, 1),
        announcer("c full", 5003, ours, SESSION.max_players),
    ];
    let mut lobby = Lobby::new(SESSION, Ok(browser()));
    assert!(lobby.take_changed(), "a new lobby is a menu to build");
    listen(&mut lobby, &mut announcers, "three hosts", |lobby| {
        lobby.joinable().len() + lobby.passed_over().len() == 3
    });

    let rows: Vec<(&str, u16)> = lobby
        .joinable()
        .iter()
        .map(|host| (host.name.as_str(), host.addr.port()))
        .collect();
    assert_eq!(rows, vec![("a open", 5001)]);
    let lines: Vec<(&str, Unjoinable)> = lobby
        .passed_over()
        .iter()
        .map(|(host, why)| (host.name.as_str(), *why))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("b elsewhere", Unjoinable::Build),
            ("c full", Unjoinable::Full)
        ]
    );
    assert!(lobby.take_changed(), "new hosts are a menu to rebuild");

    // Heard again: only when they were last heard moved.
    let before = lobby.browser.as_ref().expect("browsing").stats().announces;
    listen(
        &mut lobby,
        &mut announcers,
        "the hosts heard again",
        |lobby| lobby.browser.as_ref().expect("browsing").stats().announces >= before + 3,
    );
    assert!(
        !lobby.take_changed(),
        "the same hosts heard again rebuilt the menu"
    );
}

/// **Each reason is named in the handshake's order**, and a host with room
/// on this session's terms is joinable.
#[test]
fn a_host_is_passed_over_for_its_version_its_build_its_game_or_its_room() {
    let ours = SESSION.compatibility;
    let entry = |compatibility, players| HostEntry {
        name: "h".into(),
        addr: (Ipv4Addr::LOCALHOST, 1).into(),
        players,
        max_players: 4,
        compatibility,
        last_seen: Duration::ZERO,
    };
    assert_eq!(Unjoinable::of(ours, &entry(ours, 3)), None);
    let everything_else = ProtocolCompatibility {
        protocol_version: ours.protocol_version + 1,
        engine_build_id: ours.engine_build_id + 1,
        schema_hash: ours.schema_hash + 1,
    };
    assert_eq!(
        Unjoinable::of(ours, &entry(everything_else, 4)),
        Some(Unjoinable::Version),
        "the version is asked first"
    );
    let build_and_game = ProtocolCompatibility {
        protocol_version: ours.protocol_version,
        ..everything_else
    };
    assert_eq!(
        Unjoinable::of(ours, &entry(build_and_game, 4)),
        Some(Unjoinable::Build)
    );
    let game = ProtocolCompatibility {
        schema_hash: ours.schema_hash + 1,
        ..ours
    };
    assert_eq!(
        Unjoinable::of(ours, &entry(game, 4)),
        Some(Unjoinable::Game)
    );
    assert_eq!(
        Unjoinable::of(ours, &entry(ours, 4)),
        Some(Unjoinable::Full)
    );
    assert_eq!(
        Unjoinable::of(ours, &entry(ours, 5)),
        Some(Unjoinable::Full)
    );
}

/// **A listed row is the host it lists, and a row that is gone is refused
/// by name**: the pick asks to join the host's announced address, and the
/// refusal is the notice.
#[test]
fn a_listed_pick_joins_its_host_and_a_row_that_is_gone_is_refused() {
    let mut announcers = [announcer("only", 5004, SESSION.compatibility, 0)];
    let mut lobby = Lobby::new(SESSION, Ok(browser()));
    listen(&mut lobby, &mut announcers, "the host", |lobby| {
        !lobby.joinable().is_empty()
    });
    let host: SocketAddr = (Ipv4Addr::LOCALHOST, 5004).into();
    assert_eq!(
        lobby.pick(LobbyPick::Listed(0)),
        Ok(LobbyChoice::Join(host))
    );
    assert_eq!(lobby.notice(), None);
    assert_eq!(lobby.pick(LobbyPick::Listed(1)), Err(PickRefused::HostGone));
    assert_eq!(
        lobby.notice(),
        Some(&LobbyNotice::Refused(PickRefused::HostGone))
    );
    assert_eq!(lobby.pick(LobbyPick::Host), Ok(LobbyChoice::Host));
}

/// **The connect field parses what was typed, and refuses what is not an
/// `IP:PORT` by name** — the text it refused, in the refusal and the notice
/// — and joins it once it is one. Backspace takes one character a press,
/// and typing is reported.
#[test]
fn the_connect_field_parses_and_refuses_a_bad_address_by_name() {
    let mut lobby = alone();
    assert!(!lobby.take_typed());
    lobby.text("127.0.0.1:");
    lobby.text("x");
    assert!(lobby.take_typed(), "typing was not reported");
    let refused = PickRefused::NotAnAddress("127.0.0.1:x".to_string());
    assert_eq!(lobby.pick(LobbyPick::Connect), Err(refused.clone()));
    assert_eq!(lobby.notice(), Some(&LobbyNotice::Refused(refused)));
    assert!(lobby.take_changed(), "the refusal needs the menu rebuilt");

    lobby.key(KeyCode::Backspace, true);
    lobby.key(KeyCode::Backspace, false);
    lobby.key(KeyCode::Enter, true);
    assert_eq!(lobby.address(), "127.0.0.1:", "one press, one character");
    assert!(lobby.take_typed(), "a backspace was not reported");
    lobby.text("5000 ");
    assert!(lobby.take_typed());
    assert_eq!(
        lobby.pick(LobbyPick::Connect),
        Ok(LobbyChoice::Join((Ipv4Addr::LOCALHOST, 5000).into())),
        "the address, trimmed"
    );
    lobby.text("");
    assert!(!lobby.take_typed(), "nothing typed is no typing");
}

/// **A join that fails or a session that ends leaves the lobby saying
/// why**, with no join under way, and a join that starts says where and
/// clears what went wrong before.
#[test]
fn a_failed_join_or_an_ended_session_is_the_notice_the_player_returns_to() {
    let host: SocketAddr = (Ipv4Addr::LOCALHOST, 5005).into();
    let mut lobby = alone();
    lobby.take_changed();

    lobby.start_failed("cannot host".to_string());
    assert_eq!(
        lobby.notice(),
        Some(&LobbyNotice::CannotStart("cannot host".to_string()))
    );
    assert!(lobby.take_changed());

    lobby.join_started(host);
    assert_eq!(lobby.joining(), Some(host));
    assert_eq!(lobby.notice(), None, "a join clears the last notice");
    assert!(lobby.take_changed());

    lobby.join_failed("the host refused");
    assert_eq!(lobby.joining(), None);
    assert_eq!(
        lobby.notice(),
        Some(&LobbyNotice::JoinFailed("the host refused".to_string()))
    );
    assert!(lobby.take_changed());

    lobby.join_started(host);
    lobby.session_ended("the host left");
    assert_eq!(lobby.joining(), None);
    assert_eq!(
        lobby.notice(),
        Some(&LobbyNotice::SessionEnded("the host left".to_string()))
    );
    assert!(lobby.take_changed());

    lobby.join_started(host);
    assert_eq!(
        lobby.pick(LobbyPick::Host),
        Ok(LobbyChoice::Host),
        "a pick replaces the join under way"
    );
    assert_eq!(lobby.joining(), None);
    lobby.join_started(host);
    lobby.clear_joining();
    assert_eq!(lobby.joining(), None);
}

/// **A browser that could not bind is the reason, and the rest works**: no
/// hosts, and hosting and connecting still pick.
#[test]
fn a_lobby_that_is_not_browsing_says_why_and_still_picks() {
    let mut lobby = alone();
    lobby.poll();
    assert_eq!(lobby.browser_error(), Some("not looking"));
    assert!(lobby.joinable().is_empty() && lobby.passed_over().is_empty());
    assert_eq!(lobby.pick(LobbyPick::Host), Ok(LobbyChoice::Host));
    assert_eq!(
        Lobby::new(SESSION, Ok(browser())).browser_error(),
        None,
        "a browsing lobby has no reason"
    );
}

/// **An address no host can be at is refused by name** — port 0, nobody's
/// address, everybody's, a group's — and the longest `IP:PORT` there is
/// fits the field exactly, while typing past it is dropped.
#[test]
fn an_address_no_host_can_be_at_is_refused_and_the_field_holds_the_longest() {
    for typed in [
        "127.0.0.1:0",
        "0.0.0.0:5000",
        "[::]:5000",
        "255.255.255.255:5000",
        "224.0.0.1:5000",
        "[ff02::1]:5000",
    ] {
        let mut lobby = alone();
        lobby.text(typed);
        let addr: SocketAddr = typed.parse().expect("an IP:PORT");
        assert_eq!(
            lobby.pick(LobbyPick::Connect),
            Err(PickRefused::NotAHost(addr)),
            "{typed}"
        );
        assert_eq!(
            lobby.notice(),
            Some(&LobbyNotice::Refused(PickRefused::NotAHost(addr)))
        );
    }

    let longest = "[fe80:ffff:ffff:ffff:ffff:ffff:255.255.255.255%4294967295]:65535";
    assert_eq!(longest.chars().count(), MAX_ADDRESS_CHARS);
    let mut lobby = alone();
    lobby.text(longest);
    lobby.text("9");
    assert_eq!(lobby.address(), longest, "typing past the field was kept");
    assert_eq!(
        lobby.pick(LobbyPick::Connect),
        Ok(LobbyChoice::Join(
            longest.parse().expect("the longest parses")
        ))
    );

    // A control character is dropped before the room is counted, so it
    // takes none of it.
    let mut lobby = alone();
    lobby.text(&longest[..MAX_ADDRESS_CHARS - 1]);
    lobby.text("\u{7}5");
    assert_eq!(
        lobby.address(),
        longest,
        "a control character took the room"
    );
}
