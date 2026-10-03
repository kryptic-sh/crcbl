//! The flag parser every LAN sample reads `--host`, `--join` and `--browse`
//! through, the throttle on the host's refusal and withheld-update lines,
//! and which host a browsing [`LanClient`] picks, against announcers on UDP
//! loopback with no host behind them. The sessions themselves are driven end
//! to end by the samples' own loopback suites — `apps/sandbox/src/lan/tests.rs`
//! and `apps/towers/src/lan/tests.rs` — which is where a host and its clients
//! have a world to replicate.

use std::thread;

use super::*;
use crate::net::SystemClock;
use crate::net::udp::discovery::BrowserConfig;

/// Every argument of `argv` through [`LanMode::consume`], as a sample's
/// parser offers them: the mode, or the first refusal. An argument it does
/// not claim is an error here, since a sample would read it as its own.
fn parse(argv: &[&str]) -> Result<LanMode, String> {
    let mut mode = LanMode::Off;
    let mut args = argv.iter().map(|arg| (*arg).to_string()).peekable();
    while let Some(arg) = args.next() {
        match mode.consume(&arg, &mut args) {
            Consumed::Yes => {}
            Consumed::Bad(message) => return Err(message),
            Consumed::No | Consumed::Help => return Err(format!("not claimed: {arg}")),
        }
    }
    Ok(mode)
}

/// **The three flags reach their mode, and `--host`'s port is optional.** A
/// value after `--host` that is not a port is left for the caller rather than
/// swallowed, and no mode is the default.
#[test]
fn each_flag_reaches_its_mode_and_the_host_port_is_optional() {
    assert_eq!(parse(&[]), Ok(LanMode::Off));
    assert_eq!(parse(&["--host"]), Ok(LanMode::Host { port: 0 }));
    assert_eq!(
        parse(&["--host", "27015"]),
        Ok(LanMode::Host { port: 27_015 })
    );
    assert_eq!(
        parse(&["--join", "192.168.1.20:27015"]),
        Ok(LanMode::Join("192.168.1.20:27015".parse().unwrap()))
    );
    assert_eq!(
        parse(&["--join", "[::1]:27015"]),
        Ok(LanMode::Join("[::1]:27015".parse().unwrap()))
    );
    assert_eq!(parse(&["--browse"]), Ok(LanMode::Browse));
    assert_eq!(
        parse(&["--host", "--headless"]),
        Err("not claimed: --headless".to_string()),
        "the argument after --host is not taken as its port"
    );
    assert_eq!(
        parse(&["--host", "99999"]),
        Err("not claimed: 99999".to_string()),
        "a number past a port's range is not taken as one"
    );
}

/// **Two modes, or an address that is not one, are refused by name.**
#[test]
fn a_second_mode_or_a_bad_address_is_refused() {
    for argv in [
        &["--host", "--browse"][..],
        &["--browse", "--join", "127.0.0.1:1"],
        &["--join", "127.0.0.1:1", "--host", "2"],
        &["--browse", "--browse"],
    ] {
        assert_eq!(
            parse(argv),
            Err("--host, --join and --browse exclude each other".to_string()),
            "{argv:?}"
        );
    }
    assert_eq!(parse(&["--join"]), Err("--join needs a value".to_string()));
    for address in ["localhost", "127.0.0.1", "127.0.0.1:99999"] {
        assert_eq!(
            parse(&["--join", address]),
            Err(format!("--join needs an IP:PORT address, not `{address}`"))
        );
    }
}

/// **A count that climbs every frame is logged once an interval.** A host
/// withholding an update from every snapshot at 60 Hz for three seconds logs
/// it three times — when it first moves, then once per
/// [`REFUSAL_LOG_INTERVAL`] — and a count that stops moving logs nothing.
#[test]
fn a_climbing_count_is_logged_once_an_interval() {
    const FRAME: Duration = Duration::from_nanos(16_666_667);
    const FRAMES: u32 = 180;
    let mut log = ThrottledLog::default();
    let lines = (1..=FRAMES)
        .filter(|&frame| log.due(u64::from(frame), FRAME * frame))
        .count();
    // Frames 1, 61 and 121: the next line is due a frame past the run.
    assert_eq!(lines, 3);

    let later = FRAME * FRAMES + REFUSAL_LOG_INTERVAL;
    assert!(
        log.due(u64::from(FRAMES), later),
        "what moved since the last line is logged once the interval passes"
    );
    assert!(
        !log.due(u64::from(FRAMES), later + 10 * REFUSAL_LOG_INTERVAL),
        "a count that has not moved since is not"
    );
}

/// A session for the browsing tests alone.
const GAME: LanGame = LanGame {
    app: "lan-test",
    host_name: "lan test",
    protocol_id: u32::from_be_bytes(*b"LANT"),
    compatibility: ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 7,
        schema_hash: 11,
    },
    max_players: 4,
};

const TICK_HZ: u32 = 60;

/// The most polls any wait in the browsing tests runs.
const MAX_POLLS: usize = 600;

/// The pause after each poll, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// What an announcer for `name` announces: a game on `port`, speaking
/// `compatibility`, with `players` of [`GAME`]'s maximum in.
fn announcement(
    name: &str,
    port: u16,
    compatibility: ProtocolCompatibility,
    players: u16,
) -> Announcement {
    let mut announcement = Announcement::new(
        GAME.protocol_id,
        NonZeroU16::new(port).expect("a game port"),
        compatibility,
        name,
    );
    announcement.players = players;
    announcement.max_players = GAME.max_players;
    announcement
}

/// An announcer on loopback, broadcasting nowhere: it answers queries.
fn announcer(announcement: Announcement) -> Announcer {
    Announcer::bind_with(loopback(), announcement, None, SystemClock::new())
        .expect("loopback UDP must be available to these tests")
}

/// Sends a query from `browser` to each of `announcers`, lets them answer,
/// and polls `browser` for the replies.
fn ask(browser: &mut Browser, announcers: &mut [Announcer]) {
    for announcer in announcers.iter() {
        browser.query(announcer.local_addr().expect("announcer address"));
    }
    thread::sleep(PAUSE);
    for announcer in announcers.iter_mut() {
        announcer.poll();
    }
    thread::sleep(PAUSE);
    browser.poll();
}

/// A loopback browser, querying nothing by itself, that has heard every one
/// of `announcers`, so a [`LanClient`] handed it judges all of them on its
/// first frame rather than whichever answered first.
fn hearing(announcers: &mut [Announcer]) -> Browser {
    let mut browser = Browser::bind_with(
        loopback(),
        BrowserConfig::new(GAME.protocol_id),
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    for _ in 0..MAX_POLLS {
        if browser.hosts().len() == announcers.len() {
            return browser;
        }
        ask(&mut browser, announcers);
    }
    panic!("not every announcer heard within {MAX_POLLS} polls");
}

/// Where a client joining `announcer`'s host connects: loopback, and the
/// game port it announces.
fn game_addr(announcer: &Announcer) -> SocketAddr {
    (
        Ipv4Addr::LOCALHOST,
        announcer.announcement().game_port.get(),
    )
        .into()
}

/// **A browser passes a full host over and joins one with room.** The full
/// host sorts first, so a pick that judged compatibility alone would take
/// it — and be refused by it.
#[test]
fn a_browser_joins_a_host_with_room_over_a_full_one() {
    let ours = GAME.compatibility;
    let mut announcers = [
        announcer(announcement("a full", 5101, ours, GAME.max_players)),
        announcer(announcement("b open", 5102, ours, 1)),
    ];
    let mut client = LanClient::browse(GAME, hearing(&mut announcers), TICK_HZ);
    client.frame(Duration::ZERO);
    assert_eq!(client.host(), Some(game_addr(&announcers[1])));
}

/// **A browser that hears only a full host stays looking, and joins it once
/// it has room.** Nothing joinable is no pick at all, as with no host: the
/// browser goes on asking, and the next announce that has a free slot is
/// joined.
#[test]
fn a_browser_hearing_only_a_full_host_goes_on_looking() {
    let ours = GAME.compatibility;
    let mut announcers = [announcer(announcement(
        "full",
        5103,
        ours,
        GAME.max_players,
    ))];
    let mut client = LanClient::browse(GAME, hearing(&mut announcers), TICK_HZ);
    let mut now = Duration::ZERO;
    for _ in 0..10 {
        now += Duration::from_millis(16);
        let Phase::Browsing(browser) = &mut client.phase else {
            panic!("stopped looking with only a full host heard");
        };
        ask(browser, &mut announcers);
        assert_eq!(browser.hosts().len(), 1, "the full host is still heard");
        client.frame(now);
    }
    assert_eq!(client.host(), None, "still looking");

    announcers[0].set_announcement(announcement("full", 5103, ours, GAME.max_players - 1));
    for _ in 0..MAX_POLLS {
        let Phase::Browsing(browser) = &mut client.phase else {
            break;
        };
        ask(browser, &mut announcers);
        now += Duration::from_millis(16);
        client.frame(now);
    }
    assert_eq!(client.host(), Some(game_addr(&announcers[0])));
}

/// **A browser passes over a host of another build.** It sorts first and has
/// room, so only the compatibility check keeps the pick off it.
#[test]
fn a_browser_joins_its_own_build_over_another() {
    let ours = GAME.compatibility;
    let other_build = ProtocolCompatibility {
        engine_build_id: ours.engine_build_id + 1,
        ..ours
    };
    let mut announcers = [
        announcer(announcement("a elsewhere", 5104, other_build, 1)),
        announcer(announcement("b open", 5105, ours, 1)),
    ];
    let mut client = LanClient::browse(GAME, hearing(&mut announcers), TICK_HZ);
    client.frame(Duration::ZERO);
    assert_eq!(client.host(), Some(game_addr(&announcers[1])));
}
