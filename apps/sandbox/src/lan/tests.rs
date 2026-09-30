//! A sandbox host and its clients in one process, over UDP loopback: no
//! window, no renderer, no loop — [`LanHost`] and [`LanClient`] driven frame
//! by frame with the time handed in.
//!
//! Every socket binds `127.0.0.1:0`, the discovery port included, so no test
//! needs a free fixed port, and no test sends to a broadcast address: the
//! browser queries the host's announcer directly. Loopback delivery is
//! asynchronous, so each frame is followed by a short pause, and every wait is
//! bounded by [`MAX_FRAMES`] — reached only when what it waits for never
//! comes.

use std::net::{Ipv4Addr, SocketAddr};
use std::thread;
use std::time::Duration;

use crcbl::core::TickId;
use crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD;
use crcbl::net::udp::discovery::{Browser, BrowserConfig};
use crcbl::net::{SectorId, SystemClock};

use super::imp::{COMPATIBILITY, LanClient, LanHost, MAX_PLAYERS, PROTOCOL_ID, SANDBOX};

const TICK_HZ: u32 = 60;

/// One frame at [`TICK_HZ`]: every frame runs about one host tick.
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The most frames any wait here runs: ten seconds of host time, and a few
/// seconds of wall time at [`PAUSE`] a frame.
const MAX_FRAMES: usize = 600;

/// The pause after each frame, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// A host on loopback, announcing on loopback and broadcasting nowhere.
fn host() -> LanHost {
    LanHost::open(loopback(), loopback(), None, TICK_HZ)
        .expect("loopback UDP must be available to these tests")
}

/// Where a client reaches `host`.
fn address(host: &LanHost) -> SocketAddr {
    (Ipv4Addr::LOCALHOST, host.game_port()).into()
}

/// A host and its clients, stepped together a frame at a time.
struct Rig {
    host: LanHost,
    clients: Vec<LanClient>,
    now: Duration,
}

impl Rig {
    fn new() -> Self {
        Self {
            host: host(),
            clients: Vec::new(),
            now: Duration::ZERO,
        }
    }

    fn join(&mut self) {
        let client = LanClient::join(SANDBOX, address(&self.host), TICK_HZ).expect("connect");
        self.clients.push(client);
    }

    /// Frames until `done` holds, failing past [`MAX_FRAMES`]. Returns how
    /// many it took.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Self) -> bool) -> usize {
        for frames in 0..MAX_FRAMES {
            if done(self) {
                return frames;
            }
            self.now += FRAME;
            self.host.frame(self.now);
            for client in &mut self.clients {
                client.frame(self.now);
            }
            thread::sleep(PAUSE);
        }
        panic!("no {what} within {MAX_FRAMES} frames");
    }
}

/// Whether `client` is in a session and has applied a snapshot.
fn playing(client: &LanClient) -> bool {
    client.client().is_some_and(|client| {
        client.session_id().is_some() && client.last_applied_tick() > TickId::ZERO
    })
}

/// The hash of what `client` has reconstructed of the host's world.
fn state(client: &LanClient) -> Option<u64> {
    client.client()?.baseline_state_hash_in(SectorId::ZERO)
}

/// **A client joins a host over UDP and receives its state.** The client's
/// transport connects, the session handshake runs over it, the host's
/// snapshots arrive and apply — and when a second player joins, the first
/// client's reconstruction of the host's world changes with the players
/// system, and the announcement counts both.
#[test]
fn a_client_joins_a_host_over_udp_and_receives_its_changing_state() {
    let mut rig = Rig::new();
    rig.join();
    rig.until("first client's session", |rig| playing(&rig.clients[0]));
    assert_eq!(rig.host.host().peer_count(), 1);
    let alone = state(&rig.clients[0]).expect("a reconstructed world");

    rig.join();
    rig.until("second client's session", |rig| playing(&rig.clients[1]));
    rig.until("the first client seeing the second join", |rig| {
        state(&rig.clients[0]).is_some_and(|now| now != alone)
    });
    assert_eq!(rig.host.host().peer_count(), 2);
    rig.until("the announcement counting both", |rig| {
        rig.host
            .announcement()
            .is_some_and(|announcement| announcement.players == 2)
    });
    assert_eq!(
        rig.host.announcement().map(|a| a.max_players),
        Some(MAX_PLAYERS)
    );
    for client in &rig.clients {
        let client = client.client().expect("joined");
        assert_eq!(client.processing_error_count(), 0);
        assert_eq!(client.auth_failure_count(), 0);
    }
    assert_eq!(rig.host.host().processing_error_count(), 0);
}

/// **The sandbox's snapshot fits one datagram, with room.** The unreliable
/// channel takes [`MAX_UNRELIABLE_PAYLOAD`] bytes; a snapshot past it would
/// be refused outright. Measured on a full host, since each player adds to
/// the players system.
#[test]
fn the_sandboxes_snapshot_fits_one_datagram_with_every_player_in() {
    let mut rig = Rig::new();
    for _ in 0..MAX_PLAYERS {
        rig.join();
    }
    rig.until("every client's session", |rig| {
        rig.clients.iter().all(playing)
    });
    let largest = rig.host.host().largest_snapshot_bytes();
    assert!(largest > 0, "snapshots were sent");
    assert!(
        largest <= MAX_UNRELIABLE_PAYLOAD / 4,
        "the sandbox's snapshot is {largest} of {MAX_UNRELIABLE_PAYLOAD} bytes"
    );
    assert_eq!(rig.host.host().oversized_snapshot_count(), 0);
}

/// **A browser finds the host's announcer and joins where it points.** The
/// query goes straight to the announcer's loopback port instead of a
/// broadcast address; the entry's address is the announce's source IP with
/// the port it named, which is the host's listener — and the client plays.
#[test]
fn a_browser_finds_the_host_and_joins_the_address_it_announced() {
    let mut rig = Rig::new();
    let announcer = rig.host.announcer_addr().expect("the host announces");
    let browser = Browser::bind_with(
        loopback(),
        BrowserConfig {
            query_to: Some(announcer),
            ..BrowserConfig::new(PROTOCOL_ID)
        },
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    rig.clients
        .push(LanClient::browse(SANDBOX, browser, TICK_HZ));

    rig.until("the browsed session", |rig| playing(&rig.clients[0]));
    assert_eq!(rig.clients[0].host(), Some(address(&rig.host)));
    assert_eq!(rig.host.host().peer_count(), 1);
}

/// **A browser does not join a host of another build.** The announce says
/// what the handshake would refuse, so the host is passed over before any
/// connect: the browser hears it and stays looking.
#[test]
fn a_browser_passes_over_a_host_of_another_build() {
    use crcbl::net::udp::discovery::{Announcement, Announcer};
    use std::num::NonZeroU16;

    let other_build = crcbl::net::ProtocolCompatibility {
        schema_hash: COMPATIBILITY.schema_hash + 1,
        ..COMPATIBILITY
    };
    let mut announcer = Announcer::bind_with(
        loopback(),
        Announcement::new(
            PROTOCOL_ID,
            NonZeroU16::new(1).expect("non-zero"),
            other_build,
            "elsewhere",
        ),
        None,
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    let mut browser = Browser::bind_with(
        loopback(),
        BrowserConfig {
            query_to: Some(announcer.local_addr().expect("announcer address")),
            ..BrowserConfig::new(PROTOCOL_ID)
        },
        SystemClock::new(),
    )
    .expect("loopback UDP must be available to these tests");
    // Heard first by a bare browser, so the test knows the announce arrives.
    let mut heard = false;
    for _ in 0..MAX_FRAMES {
        browser.poll();
        announcer.poll();
        if !browser.hosts().is_empty() {
            heard = true;
            break;
        }
        thread::sleep(PAUSE);
    }
    assert!(heard, "the announce reaches a browser");

    let mut client = LanClient::browse(SANDBOX, browser, TICK_HZ);
    let mut now = Duration::ZERO;
    for _ in 0..10 {
        now += FRAME;
        announcer.poll();
        client.frame(now);
        thread::sleep(PAUSE);
    }
    assert_eq!(client.host(), None, "still looking");
}
