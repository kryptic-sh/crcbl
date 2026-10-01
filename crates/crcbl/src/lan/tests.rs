//! The flag parser every LAN sample reads `--host`, `--join` and `--browse`
//! through, and the throttle on the host's refusal and withheld-update
//! lines. The sessions themselves are driven end to end by the samples'
//! own loopback suites — `apps/sandbox/src/lan/tests.rs` and
//! `apps/towers/src/lan/tests.rs` — which is where a host and its clients
//! have a world to replicate.

use super::*;

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
