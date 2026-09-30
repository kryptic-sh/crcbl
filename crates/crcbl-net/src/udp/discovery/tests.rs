//! The host list's rules from any address, then `Announcer` and `Browser`
//! over real loopback sockets.
//!
//! Every socket binds `127.0.0.1:0`, so no test depends on a free fixed port
//! — [`super::DISCOVERY_PORT`] included — and a sandbox without loopback UDP
//! fails every socket test here loudly at the bind. **No test sends to a
//! broadcast address**: whether a runner delivers broadcast at all is the
//! runner's business. The broadcast path is covered by the socket option it
//! needs and by its periodic send aimed at a loopback socket instead; the
//! receive path by directed queries over loopback.
//!
//! Timing runs on a [`ManualClock`], so an interval or an expiry is a clock
//! advance and not a wait; the waits that remain are for loopback delivery
//! and end as soon as what they wait for arrives.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::num::NonZeroU16;
use std::thread;
use std::time::{Duration, Instant};

use super::browser::Hosts;
use super::wire::{Announcement, encode_query};
use super::*;
use crate::{ManualClock, ProtocolCompatibility};

const PROTOCOL: u32 = 0x4352_4342;

/// Every socket here binds this: loopback, any free port.
const LOOPBACK: &str = "127.0.0.1:0";

/// The port the sample host's listener is said to be on.
const GAME_PORT: u16 = 27_015;

/// The longest a test waits for loopback traffic. Reached only when what it
/// waits for never comes — which is the failure.
const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// The pause between polls while waiting.
const POLL_PAUSE: Duration = Duration::from_millis(1);

/// How long a test gives loopback to deliver something it expects **not**
/// to arrive, before concluding it did not.
const QUIET_TIME: Duration = Duration::from_millis(50);

/// The smallest step a [`ManualClock`] can take.
const TICK: Duration = Duration::from_nanos(1);

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 7,
    engine_build_id: 0xF00,
    schema_hash: 0xBA2,
};

/// Polls `done` until it holds, failing the test past [`WAIT_LIMIT`].
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < WAIT_LIMIT, "gave up waiting for {what}");
        thread::sleep(POLL_PAUSE);
    }
}

fn bind_loopback() -> UdpSocket {
    let socket = UdpSocket::bind(LOOPBACK).expect("loopback UDP must be available to these tests");
    socket.set_nonblocking(true).expect("non-blocking socket");
    socket
}

/// Everything waiting on `socket`.
fn drain(socket: &UdpSocket) -> Vec<Vec<u8>> {
    let mut buffer = [0u8; 2048];
    let mut got = Vec::new();
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((len, _)) => got.push(buffer[..len].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return got,
            Err(error) => panic!("read: {error}"),
        }
    }
}

fn sample(name: &str, game_port: u16) -> Announcement {
    let game_port = NonZeroU16::new(game_port).expect("a test port is not zero");
    let mut announcement = Announcement::new(PROTOCOL, game_port, COMPATIBILITY, name);
    announcement.players = 3;
    announcement.max_players = 8;
    announcement
}

fn addr(text: &str) -> SocketAddr {
    text.parse().expect("a test address")
}

fn announcer(broadcast_to: Option<SocketAddr>, clock: ManualClock) -> Announcer<ManualClock> {
    Announcer::bind_with(LOOPBACK, sample("Den", GAME_PORT), broadcast_to, clock)
        .expect("loopback UDP must be available to these tests")
}

fn browser(query_to: Option<SocketAddr>, clock: ManualClock) -> Browser<ManualClock> {
    let config = BrowserConfig {
        query_to,
        ..BrowserConfig::new(PROTOCOL)
    };
    Browser::bind_with(LOOPBACK, config, clock)
        .expect("loopback UDP must be available to these tests")
}

/// A host list and the stats it counts into, fed by hand.
struct Table {
    hosts: Hosts,
    config: BrowserConfig,
    stats: BrowserStats,
}

impl Table {
    fn new() -> Self {
        Self {
            hosts: Hosts::default(),
            config: BrowserConfig::new(PROTOCOL),
            stats: BrowserStats::default(),
        }
    }

    fn receive(&mut self, datagram: &[u8], from: &str, now: Duration) {
        self.hosts
            .receive(datagram, addr(from), now, &self.config, &mut self.stats);
    }
}

// ── The host list ────────────────────────────────────────────────────────────

/// The address to connect to is the announce's source IP with the port it
/// names — not the source port, and nothing the payload could say. The same
/// announce from two machines is two hosts, each at its own address.
#[test]
fn the_connect_address_is_the_source_ip_and_the_announced_port() {
    let mut table = Table::new();
    let announce = sample("Den", GAME_PORT).encode();
    table.receive(&announce, "192.168.1.7:61917", Duration::ZERO);
    table.receive(&announce, "10.0.0.9:5000", Duration::ZERO);
    let hosts = table.hosts.sorted();
    let addrs: Vec<SocketAddr> = hosts.iter().map(|host| host.addr).collect();
    assert_eq!(
        addrs,
        [
            addr(&format!("10.0.0.9:{GAME_PORT}")),
            addr(&format!("192.168.1.7:{GAME_PORT}"))
        ]
    );
    assert_eq!(
        hosts[0],
        HostEntry {
            name: "Den".to_owned(),
            addr: addrs[0],
            players: 3,
            max_players: 8,
            compatibility: COMPATIBILITY,
            last_seen: Duration::ZERO,
        }
    );
}

/// A host's broadcasts and its replies to queries — any source port, one
/// game port — are one row, refreshed by the newest.
#[test]
fn announces_from_one_host_are_one_entry_holding_the_latest() {
    let mut table = Table::new();
    table.receive(
        &sample("Den", GAME_PORT).encode(),
        "192.168.1.7:61917",
        Duration::ZERO,
    );
    let mut fuller = sample("Den", GAME_PORT);
    fuller.players = 5;
    let later = Duration::from_millis(300);
    table.receive(&fuller.encode(), "192.168.1.7:61917", later);
    table.receive(&fuller.encode(), "192.168.1.7:40000", later);
    let hosts = table.hosts.sorted();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].players, 5);
    assert_eq!(hosts[0].last_seen, later);
    assert_eq!(table.stats.announces, 3);
}

/// Sorted by name, then address, however they arrived.
#[test]
fn hosts_are_listed_by_name_then_address() {
    let mut table = Table::new();
    table.receive(&sample("b", 2).encode(), "10.0.0.1:1", Duration::ZERO);
    table.receive(&sample("a", 9).encode(), "10.0.0.3:1", Duration::ZERO);
    table.receive(&sample("b", 1).encode(), "10.0.0.1:1", Duration::ZERO);
    table.receive(&sample("a", 9).encode(), "10.0.0.2:1", Duration::ZERO);
    let listed: Vec<(String, SocketAddr)> = table
        .hosts
        .sorted()
        .into_iter()
        .map(|host| (host.name, host.addr))
        .collect();
    let expected = [
        ("a", "10.0.0.2:9"),
        ("a", "10.0.0.3:9"),
        ("b", "10.0.0.1:1"),
        ("b", "10.0.0.1:2"),
    ]
    .map(|(name, at)| (name.to_owned(), addr(at)));
    assert_eq!(listed, expected);
}

/// Past the cap a new host is dropped and counted; a listed one still
/// refreshes.
#[test]
fn a_new_host_past_the_cap_is_dropped_and_listed_ones_refresh() {
    let mut table = Table::new();
    table.config.max_hosts = 2;
    table.receive(&sample("a", 1).encode(), "10.0.0.1:1", Duration::ZERO);
    table.receive(&sample("b", 1).encode(), "10.0.0.2:1", Duration::ZERO);
    table.receive(&sample("c", 1).encode(), "10.0.0.3:1", Duration::ZERO);
    assert_eq!(table.stats.hosts_full, 1);
    let later = Duration::from_secs(1);
    table.receive(&sample("a2", 1).encode(), "10.0.0.1:1", later);
    let names: Vec<String> = table.hosts.sorted().into_iter().map(|h| h.name).collect();
    assert_eq!(names, ["a2", "b"]);
    assert_eq!(table.stats.hosts_full, 1);
}

/// Each kind of hostile or foreign datagram is dropped, lists nothing, and
/// is counted under its own reason.
#[test]
fn malformed_and_foreign_datagrams_are_dropped_and_counted() {
    let mut table = Table::new();
    let valid = sample("Den", GAME_PORT).encode();
    let mut other_version = valid;
    other_version[1] = DISCOVERY_VERSION + 1;
    let mut other_protocol = sample("Den", GAME_PORT);
    other_protocol.protocol_id = PROTOCOL + 1;
    let mut bad_name_len = valid;
    bad_name_len[ANNOUNCE_BYTES - MAX_NAME_BYTES - 1] = MAX_NAME_BYTES as u8 + 1;
    let oversized = [valid.as_slice(), &[0; 64]].concat();
    let cases: [(&str, Vec<u8>); 8] = [
        ("empty", Vec::new()),
        ("truncated", valid[..ANNOUNCE_BYTES / 2].to_vec()),
        ("oversized", oversized),
        ("a name length past the cap", bad_name_len.to_vec()),
        ("another version", other_version.to_vec()),
        ("another protocol", other_protocol.encode().to_vec()),
        ("a query", encode_query(PROTOCOL).to_vec()),
        (
            "a hello",
            vec![crate::udp::HELLO_TAG; crate::udp::HELLO_BYTES],
        ),
    ];
    for (what, datagram) in &cases {
        table.receive(datagram, "192.168.1.7:61917", Duration::ZERO);
        assert!(table.hosts.sorted().is_empty(), "{what} was listed");
    }
    let expected = BrowserStats {
        malformed: 3,
        other_version: 1,
        other_protocol: 1,
        not_announce: 3,
        ..BrowserStats::default()
    };
    assert_eq!(table.stats, expected);
}

/// Seeded arbitrary bytes, and seeded corruptions of a valid announce, fed
/// to the list: none may panic, and the list never passes its cap.
#[test]
fn arbitrary_bytes_never_panic_the_host_list() {
    let mut seed = 0x5EED_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed >> 32
    };
    let mut table = Table::new();
    table.config.max_hosts = 8;
    let valid = sample("Den", GAME_PORT).encode();
    for i in 0..20_000_u32 {
        let from = format!("10.0.{}.{}:1", i % 7, i % 251);
        let len = (next() % (ANNOUNCE_BYTES as u64 + 8)) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        table.receive(&bytes, &from, Duration::ZERO);

        let mut corrupt = valid.to_vec();
        for _ in 0..=next() % 4 {
            let at = (next() as usize) % corrupt.len();
            corrupt[at] = next() as u8;
        }
        table.receive(&corrupt, &from, Duration::ZERO);
    }
    assert!(table.hosts.sorted().len() <= 8);
}

// ── Over loopback ────────────────────────────────────────────────────────────

/// The whole path: a browser's directed query reaches the announcer, whose
/// reply lands in the browser's list at loopback and the announced port.
#[test]
fn a_directed_query_over_loopback_lists_the_host() {
    let clock = ManualClock::new();
    let mut announcer = announcer(None, clock.clone());
    let at = announcer.local_addr().expect("announcer address");
    let mut browser = browser(Some(at), clock);
    wait_until("the host to be listed", || {
        browser.poll();
        announcer.poll();
        !browser.hosts().is_empty()
    });
    let hosts = browser.hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(
        hosts[0].addr,
        SocketAddr::from((Ipv4Addr::LOCALHOST, GAME_PORT))
    );
    assert_eq!(hosts[0].name, "Den");
    assert_eq!((hosts[0].players, hosts[0].max_players), (3, 8));
    assert_eq!(hosts[0].compatibility, COMPATIBILITY);
    assert_eq!(announcer.stats().queries_answered, 1);
    assert_eq!(browser.stats().queries_sent, 1);
}

/// The padding rule on the wire: a query shorter than the announce, or for
/// another protocol, gets nothing back; a full one gets exactly one reply,
/// no longer than itself.
#[test]
fn only_a_padded_query_is_answered_and_the_reply_is_no_larger() {
    let mut announcer = announcer(None, ManualClock::new());
    let at = announcer.local_addr().expect("announcer address");
    let asker = bind_loopback();
    let query = encode_query(PROTOCOL);
    for short in [&query[..6], &query[..QUERY_BYTES - 1]] {
        asker.send_to(short, at).expect("send a short query");
    }
    asker
        .send_to(&encode_query(PROTOCOL + 1), at)
        .expect("send another protocol's query");
    asker.send_to(&query, at).expect("send the query");
    wait_until("every query to be read", || {
        announcer.poll();
        let stats = announcer.stats();
        stats.malformed + stats.other_protocol + stats.queries_answered == 4
    });
    let stats = announcer.stats();
    assert_eq!(
        (
            stats.malformed,
            stats.other_protocol,
            stats.queries_answered
        ),
        (2, 1, 1)
    );

    let mut replies = Vec::new();
    wait_until("the reply", || {
        replies.extend(drain(&asker));
        !replies.is_empty()
    });
    thread::sleep(QUIET_TIME);
    replies.extend(drain(&asker));
    assert_eq!(replies.len(), 1, "only the padded query is answered");
    assert!(replies[0].len() <= query.len());
    assert_eq!(replies[0], announcer.announcement().encode());
}

/// The broadcast send path, aimed at a loopback socket in place of the
/// broadcast address: both sockets may broadcast, the first poll sends at
/// once, and the next goes out one [`ANNOUNCE_INTERVAL`] later, not before.
#[test]
fn the_announcer_broadcasts_once_per_interval() {
    let clock = ManualClock::new();
    let listener = bind_loopback();
    let mut announcer = announcer(Some(listener.local_addr().expect("address")), clock.clone());
    assert!(announcer.may_broadcast().expect("read SO_BROADCAST"));
    let browser = browser(None, clock.clone());
    assert!(browser.may_broadcast().expect("read SO_BROADCAST"));

    let mut heard = Vec::new();
    announcer.poll();
    wait_until("the first broadcast", || {
        heard.extend(drain(&listener));
        !heard.is_empty()
    });
    clock.advance(ANNOUNCE_INTERVAL - TICK);
    announcer.poll();
    thread::sleep(QUIET_TIME);
    heard.extend(drain(&listener));
    assert_eq!(heard.len(), 1, "nothing before the interval");
    clock.advance(TICK);
    announcer.poll();
    wait_until("the second broadcast", || {
        heard.extend(drain(&listener));
        heard.len() == 2
    });
    assert_eq!(announcer.stats().broadcasts, 2);
    assert!(
        heard
            .iter()
            .all(|d| *d == announcer.announcement().encode())
    );
}

/// A browser bound where the broadcasts land lists the host without
/// asking.
#[test]
fn a_listening_browser_lists_a_host_from_its_broadcast() {
    let clock = ManualClock::new();
    let mut browser = browser(None, clock.clone());
    let at = browser.local_addr().expect("browser address");
    let mut announcer = announcer(Some(at), clock);
    announcer.poll();
    wait_until("the host to be listed", || {
        browser.poll();
        !browser.hosts().is_empty()
    });
    assert_eq!(browser.stats().queries_sent, 0);
    assert_eq!(browser.hosts()[0].name, "Den");
}

/// A host is listed until it has been silent for [`HOST_EXPIRY`], and not
/// a moment after; hearing from it again restarts the wait.
#[test]
fn a_host_unheard_for_the_expiry_leaves_the_list() {
    let clock = ManualClock::new();
    let mut browser = browser(None, clock.clone());
    let to = browser.local_addr().expect("browser address");
    let host = bind_loopback();
    let announce = sample("Den", GAME_PORT).encode();
    let hear = |browser: &mut Browser<ManualClock>| {
        let before = browser.stats().announces;
        host.send_to(&announce, to).expect("send the announce");
        wait_until("the announce", || {
            browser.poll();
            browser.stats().announces > before
        });
    };

    hear(&mut browser);
    clock.advance(HOST_EXPIRY - TICK);
    browser.poll();
    assert_eq!(browser.hosts().len(), 1, "listed until the expiry");
    hear(&mut browser);
    clock.advance(HOST_EXPIRY - TICK);
    browser.poll();
    assert_eq!(browser.hosts().len(), 1, "the refresh restarted the wait");
    clock.advance(TICK);
    browser.poll();
    assert!(browser.hosts().is_empty(), "gone at the expiry");
    assert_eq!(browser.stats().expired, 1);
}

/// A datagram longer than any discovery datagram is dropped over a real
/// socket too — as malformed where the socket cuts it to the buffer, or as
/// a read error where the operating system refuses the read (Windows).
#[test]
fn an_oversized_datagram_over_loopback_is_dropped() {
    let mut browser = browser(None, ManualClock::new());
    let to = browser.local_addr().expect("browser address");
    let sender = bind_loopback();
    let oversized = [sample("Den", GAME_PORT).encode().as_slice(), &[0; 200]].concat();
    sender.send_to(&oversized, to).expect("send");
    wait_until("the datagram to be read", || {
        browser.poll();
        let stats = browser.stats();
        stats.malformed + stats.receive_errors == 1
    });
    assert!(browser.hosts().is_empty());
}

/// Nobody answering is an empty list, not a hang: the browser keeps
/// querying at its interval and every poll returns.
#[test]
fn a_silent_network_is_an_empty_list() {
    let clock = ManualClock::new();
    let silent = bind_loopback().local_addr().expect("address");
    let mut browser = browser(Some(silent), clock.clone());
    for _ in 0..3 {
        browser.poll();
        browser.poll();
        clock.advance(QUERY_INTERVAL);
    }
    thread::sleep(QUIET_TIME);
    browser.poll();
    assert!(browser.hosts().is_empty());
    assert_eq!(browser.stats().queries_sent, 4);
}
