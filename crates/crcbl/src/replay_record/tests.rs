//! The recorder against a bare [`Host`] and real clients over in-process
//! transports, writing into a temporary directory; and a [`LanHost`]'s
//! recording, on UDP loopback with no client. Towers' two-player session is
//! recorded and re-simulated bit for bit in `apps/towers`, which has a game to
//! re-simulate.
//!
//! [`LanHost`]: crate::lan::LanHost

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use super::*;
use crate::client::Client;
use crate::console::{ConVar, Flags, Registry, Table};
use crate::core::FrameClock;
use crate::ecs::World;
use crate::lan::{LanBind, LanError, LanGame, LanHost};
use crate::net::{InMemoryTransport, ProtocolCompatibility};
use crate::server::HostConfig;
use crate::store::NativeStorage;
use crate::store::replay::SpoolEnd;

const TICK_HZ: u32 = 60;

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x5245_4344,
    schema_hash: 0x5245_4344,
};

/// How many ticks the recorded sessions run.
const TICKS: usize = 120;

/// A simulation variable for the recorded host's sets; nothing reads it, so
/// a set changes no state, and the recording carries it all the same.
static T_RATE: ConVar = ConVar::new_float("t_rate", "A rate.", Flags::SIM, 0.0, 8.0, 1.0);

fn registry() -> Registry {
    static VARS: &[&ConVar] = &[&T_RATE];
    Registry::gather(&[Table::new(VARS, &[], &[])]).expect("distinct names")
}

fn host() -> Host {
    let mut host = Host::new(
        World::new(),
        HostConfig {
            max_peers: 2,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    host.set_sim_registry(registry());
    host.update(Duration::ZERO);
    host
}

/// A host with two clients each sending a frame a tick, recorded, and its
/// clock.
struct Session {
    host: Host,
    clients: Vec<Client<InMemoryTransport>>,
    now: Duration,
}

impl Session {
    fn new() -> Self {
        let mut host = host();
        let clients = (0..2)
            .map(|_| {
                let (near, far) = InMemoryTransport::pair();
                host.add(Box::new(far));
                Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY)
            })
            .collect();
        Self {
            host,
            clients,
            now: Duration::ZERO,
        }
    }

    /// One tick of the host, then every client's, each sending a frame.
    fn step(&mut self) {
        self.now += FrameClock::new(TICK_HZ).tick_dt();
        assert_eq!(self.host.update(self.now), 1, "one tick a step");
        let tick = self.host.tick_id().get().to_le_bytes();
        for (index, client) in self.clients.iter_mut().enumerate() {
            client.set_input(vec![u8::try_from(index).expect("two clients"), tick[0]]);
            client.update(self.now);
        }
    }
}

fn read(path: &Path) -> FileTransport {
    let storage = NativeStorage::at(path.parent().expect("in a directory").to_path_buf());
    FileTransport::open(&storage, Path::new(path.file_name().expect("a file")))
        .expect("the recording reads")
}

/// **The host's records do not grow while the recorder drains them**: over
/// a session of frames every tick, the input record holds at most the tick
/// since the last pull and the set record nothing — and the file holds every
/// tick's hash and the whole peer track.
#[test]
fn the_hosts_records_stay_bounded_while_the_recorder_drains_them() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("session.crpl");
    let mut session = Session::new();
    let mut recorder = Recorder::start(&path, &mut session.host, TICK_HZ).expect("it starts");
    assert_eq!(recorder.path(), path);
    assert!(path.exists() && spool_path(&path).exists());

    let start = session.host.tick_id();
    for step in 0..TICKS {
        session.step();
        if step == TICKS / 2 {
            session.host.submit_console_set(ConsoleSet {
                name: "t_rate".to_owned(),
                value: "2".to_owned(),
            });
        }
        assert!(
            session.host.peer_input_record().len() <= 1,
            "the input record held more than the tick since the last pull"
        );
        recorder.record(&mut session.host).expect("it records");
        assert!(session.host.peer_input_record().is_empty());
        assert!(session.host.sim_record().is_empty());
    }
    let last = session.host.tick_id();
    let summary = recorder.finish(&mut session.host).expect("it finishes");
    assert_eq!(summary.ticks, (start, last));
    assert_eq!(summary.sim_sets, 1);
    assert_eq!(summary.state_hashes, TICKS + 1, "the start, and every tick");
    assert!(!spool_path(&path).exists(), "the spool is removed");

    let file = read(&path);
    assert_eq!(file.tick_rate(), TICK_HZ);
    let sets: Vec<(u64, &str)> = file
        .sim_sets()
        .iter()
        .map(|recorded| (recorded.tick.get(), recorded.set.value.as_str()))
        .collect();
    assert_eq!(sets, [(start.get() + TICKS as u64 / 2 + 2, "2")]);
    assert_eq!(file.state_hashes().len(), TICKS + 1);
    assert_eq!(file.state_hashes()[0].tick, start);
    assert_eq!(file.peer_ticks().len(), summary.peer_ticks);
    // Both clients join, then send a frame every tick until the end.
    assert!(
        file.peer_ticks().len() > TICKS / 2,
        "{}",
        file.peer_ticks().len()
    );
    let joins = file
        .peer_ticks()
        .iter()
        .flat_map(|tick| &tick.roster)
        .filter(|change| change.kind == RosterChangeKind::Joined)
        .count();
    assert_eq!(joins, 2);
    assert!(
        file.peer_ticks()
            .iter()
            .any(|tick| tick.peers.len() == 2 && tick.peers.iter().all(|p| p.frames.len() == 1)),
        "the frames are in the track"
    );

    // A fresh host built the same way reproduces every hash from the file.
    let mut replayed = host();
    assert_eq!(resimulate(&mut replayed, &file), Ok(last));
}

/// **A recording never overwrites**: a path something exists at is refused
/// by name and left as it was, and the spool the start created first is
/// removed — so the refusal leaves no recording behind either. A spool's
/// name taken is `a_stale_spool_refuses_a_new_recording_and_is_kept`'s.
#[test]
fn a_recording_refuses_an_existing_path_and_leaves_it_alone() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("taken.crpl");
    std::fs::write(&path, b"someone's file").expect("written");
    let mut host = host();
    match Recorder::start(&path, &mut host, TICK_HZ) {
        Err(RecordError::Exists(refused)) => assert_eq!(refused, path),
        other => panic!("not refused as existing: {other:?}"),
    }
    assert_eq!(
        std::fs::read(&path).expect("still there"),
        b"someone's file"
    );
    assert!(
        !spool_path(&path).exists(),
        "the spool it created was removed"
    );
    assert!(matches!(
        refuse_existing(&path),
        Err(RecordError::Exists(_))
    ));
    assert!(refuse_existing(&dir.path().join("new.crpl")).is_ok());
}

/// Runs `session` for [`TICKS`] ticks recorded to `path`, with a set half way,
/// and abandons the recorder unfinished — as a process killed mid-session
/// leaves it, with no code run after the last pull. Answers the last tick.
fn record_unfinished(session: &mut Session, path: &Path) -> TickId {
    let mut recorder = Recorder::start(path, &mut session.host, TICK_HZ).expect("it starts");
    for step in 0..TICKS {
        session.step();
        if step == TICKS / 2 {
            session.host.submit_console_set(ConsoleSet {
                name: "t_rate".to_owned(),
                value: "2".to_owned(),
            });
        }
        recorder.record(&mut session.host).expect("it records");
    }
    drop(recorder);
    session.host.tick_id()
}

/// **A recording that never finished is recovered from its spool** and
/// re-simulates bit for bit: every tick it pulled when the spool is whole,
/// and every tick it holds a hash for when its last records are cut short.
#[test]
fn an_unfinished_recording_recovers_and_resimulates() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("killed.crpl");
    let mut session = Session::new();
    let last = record_unfinished(&mut session, &path);
    assert_eq!(
        std::fs::read(&path).expect("left"),
        b"",
        "the file is empty"
    );
    let spool = spool_path(&path);

    let recovery = recover(&spool, &path).expect("it recovers");
    assert_eq!(recovery.end, SpoolEnd::Whole);
    assert_eq!(recovery.dropped_bytes, 0);
    assert_eq!(
        recovery.state_hashes,
        TICKS + 1,
        "the start, and every tick"
    );
    assert_eq!(recovery.sim_sets, 1);
    assert_eq!(recovery.hashed_ticks.map(|(_, held)| held), Some(last));
    assert!(spool.exists(), "the spool is left for its owner");
    assert!(!beside(&path, RECOVERING_SUFFIX).exists());
    let file = read(&path);
    assert_eq!(file.peer_ticks().len(), recovery.peer_ticks);
    assert_eq!(resimulate(&mut host(), &file), Ok(last));

    // Cut a few bytes into its last records, as a kill mid-write leaves it.
    let bytes = std::fs::read(&spool).expect("the spool reads");
    for cut in [1, 7, 20, 41] {
        let torn = dir.path().join(format!("torn-{cut}.spool"));
        std::fs::write(&torn, &bytes[..bytes.len() - cut]).expect("written");
        let out = dir.path().join(format!("torn-{cut}.crpl"));
        let recovery = recover(&torn, &out).expect("it recovers");
        assert_eq!(recovery.end, SpoolEnd::CutShort, "cut {cut}");
        assert!(recovery.dropped_bytes > 0, "cut {cut}");
        assert_eq!(
            recovery.kept_bytes + recovery.dropped_bytes,
            (bytes.len() - cut) as u64
        );
        let (_, held) = recovery.hashed_ticks.expect("hashes held");
        assert!(
            held < last,
            "cut {cut}: the last tick's hash is its last record"
        );
        assert_eq!(resimulate(&mut host(), &read(&out)), Ok(held), "cut {cut}");
    }
}

/// **Recovery fills only an empty file or a new one**, and a recovery that
/// fails leaves the path as it was: a file that holds something is refused
/// by name, so is a file left from a recovery that did not finish, and a
/// spool that is not one is refused before anything is moved.
#[test]
fn recovery_fills_only_an_empty_file_and_leaves_it_on_failure() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("session.crpl");
    let mut session = Session::new();
    record_unfinished(&mut session, &path);
    let spool = spool_path(&path);

    let filled = dir.path().join("filled.crpl");
    std::fs::write(&filled, b"someone's file").expect("written");
    match recover(&spool, &filled) {
        Err(RecordError::NotEmpty(refused)) => assert_eq!(refused, filled),
        other => panic!("not refused as filled: {other:?}"),
    }
    assert_eq!(std::fs::read(&filled).expect("kept"), b"someone's file");

    let recovering = beside(&path, RECOVERING_SUFFIX);
    std::fs::write(&recovering, b"half").expect("written");
    match recover(&spool, &path) {
        Err(RecordError::Exists(refused)) => assert_eq!(refused, recovering),
        other => panic!("not refused as existing: {other:?}"),
    }
    assert_eq!(std::fs::read(&recovering).expect("kept"), b"half");
    std::fs::remove_file(&recovering).expect("removed");

    let garbage = dir.path().join("garbage.spool");
    std::fs::write(&garbage, b"not a spool at all").expect("written");
    match recover(&garbage, &path) {
        Err(RecordError::Store { path: named, .. }) => assert_eq!(named, path),
        other => panic!("not refused by the store: {other:?}"),
    }
    assert_eq!(std::fs::read(&path).expect("kept"), b"", "still empty");
    assert!(!recovering.exists(), "the half-written file is removed");

    recover(&spool, &path).expect("the real spool recovers");
    assert!(!read(&path).state_hashes().is_empty());
}

/// **A spool left beside a path refuses a new recording to it**, by name and
/// with the command that recovers it, and is left exactly as it was — at the
/// command line, at the start, and with the empty file gone too.
#[test]
fn a_stale_spool_refuses_a_new_recording_and_is_kept() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("again.crpl");
    let mut session = Session::new();
    record_unfinished(&mut session, &path);
    let spool = spool_path(&path);
    let before = std::fs::read(&spool).expect("the spool reads");
    let stale = |error: RecordError| match error {
        RecordError::StaleSpool {
            path: named,
            spool: left,
        } => {
            assert_eq!(named, path);
            assert_eq!(left, spool);
        }
        other => panic!("not refused as a stale spool: {other:?}"),
    };

    let mut host = host();
    stale(Recorder::start(&path, &mut host, TICK_HZ).expect_err("refused"));
    stale(refuse_existing(&path).expect_err("refused"));
    let refusal = parse(&["--record", path.to_str().expect("UTF-8")]).unwrap_err();
    assert!(refusal.contains("crcbl replay --recover "), "{refusal}");
    assert!(refusal.contains("again.crpl.spool"), "{refusal}");

    // With the empty file gone the spool still refuses: it is the recording.
    std::fs::remove_file(&path).expect("removed");
    stale(Recorder::start(&path, &mut host, TICK_HZ).expect_err("refused"));
    assert!(!path.exists(), "nothing was created");
    assert_eq!(std::fs::read(&spool).expect("kept"), before);

    // A spool's name taken by anything at all refuses the same way.
    let free = dir.path().join("free.crpl");
    std::fs::write(spool_path(&free), b"a spool").expect("written");
    match Recorder::start(&free, &mut host, TICK_HZ) {
        Err(RecordError::StaleSpool { spool: left, .. }) => assert_eq!(left, spool_path(&free)),
        other => panic!("not refused as a stale spool: {other:?}"),
    }
    assert!(!free.exists(), "no recording was created");
    assert_eq!(
        std::fs::read(spool_path(&free)).expect("still there"),
        b"a spool"
    );
}

/// Every argument of `argv` through [`consume`]: the path, or the first
/// refusal.
fn parse(argv: &[&str]) -> Result<Option<PathBuf>, String> {
    let mut record = None;
    let mut args = argv.iter().map(|arg| (*arg).to_string()).peekable();
    while let Some(arg) = args.next() {
        match consume(&mut record, &arg, &mut args) {
            Consumed::Yes => {}
            Consumed::Bad(message) => return Err(message),
            Consumed::No | Consumed::Help => return Err(format!("not claimed: {arg}")),
        }
    }
    Ok(record)
}

/// **`--record` takes a file nothing exists at**, once; a missing value, a
/// second flag and an existing path are refused by name.
#[test]
fn the_record_flag_takes_one_new_file() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let new = dir.path().join("new.crpl");
    let new_arg = new.to_str().expect("a UTF-8 temporary path");
    assert_eq!(parse(&[]), Ok(None));
    assert_eq!(parse(&["--record", new_arg]), Ok(Some(new.clone())));
    assert_eq!(
        parse(&["--record"]),
        Err("--record needs a file".to_string())
    );
    assert_eq!(
        parse(&["--record", new_arg, "--record", new_arg]),
        Err("--record was given twice".to_string())
    );
    assert_eq!(parse(&["--other"]), Err("not claimed: --other".to_string()));

    let taken = dir.path().join("taken.crpl");
    std::fs::write(&taken, b"").expect("written");
    let refusal = parse(&["--record", taken.to_str().expect("UTF-8")]).unwrap_err();
    assert!(
        refusal.starts_with("--record: refusing to record to "),
        "{refusal}"
    );
    assert!(refusal.contains("taken.crpl"), "{refusal}");
    assert!(
        refusal.ends_with("a recording never overwrites"),
        "{refusal}"
    );
}

/// The host of a game nothing joins, on loopback.
fn loopback_host() -> LanHost {
    let loopback: SocketAddr = (Ipv4Addr::LOCALHOST, 0).into();
    LanHost::open(
        LanGame {
            app: "record-test",
            host_name: "record test",
            protocol_id: u32::from_be_bytes(*b"RECT"),
            compatibility: COMPATIBILITY,
            max_players: 2,
        },
        LanBind {
            listen: loopback,
            announce_at: loopback,
            broadcast_to: None,
        },
        World::new(),
        TICK_HZ,
    )
    .expect("loopback UDP must be available to these tests")
}

/// **A LAN host finishes its recording when it is dropped** — a window
/// closing, or a panic unwinding — with every tick it ran; it refuses a
/// second recording while it records, and stopping one finishes it.
#[test]
fn a_lan_host_finishes_its_recording_when_dropped() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("lan.crpl");
    let mut lan = loopback_host();
    assert_eq!(lan.recording(), None);
    assert!(lan.stop_recording().is_none());
    lan.record(&path).expect("it records");
    assert_eq!(lan.recording(), Some(path.as_path()));
    match lan.record(&dir.path().join("second.crpl")) {
        Err(LanError::Record(RecordError::Recording(current))) => assert_eq!(current, path),
        other => panic!("a second recording was not refused: {other:?}"),
    }
    let period = FrameClock::new(TICK_HZ).tick_dt();
    for frame in 1..=10 {
        lan.frame(period * frame);
    }
    let last = lan.host().tick_id();
    drop(lan);

    let file = read(&path);
    assert_eq!(file.state_hashes().len(), 11, "the start and ten ticks");
    assert_eq!(file.state_hashes().last().map(|hash| hash.tick), Some(last));
    assert!(!spool_path(&path).exists());

    let mut lan = loopback_host();
    let stopped = dir.path().join("stopped.crpl");
    lan.record(&stopped).expect("it records");
    lan.frame(period);
    let summary = lan
        .stop_recording()
        .expect("it was recording")
        .expect("it finishes");
    assert_eq!(summary.state_hashes, 2);
    assert_eq!(lan.recording(), None);
    assert_eq!(read(&stopped).state_hashes().len(), 2);
}
