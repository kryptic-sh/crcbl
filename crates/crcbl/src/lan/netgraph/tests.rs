//! The netgraph's rows and graphs from figures the test chose, its history's
//! bound and cadence, and a host with two clients over UDP loopback showing
//! a row per peer and a link per client with the figures their links report.

use std::net::{Ipv4Addr, SocketAddr};
use std::thread;

use super::*;
use crate::ecs::World;
use crate::lan::{LanBind, LanClient, LanGame, LanHost};
use crate::net::reliable::WindowCounts;
use crate::net::{ProtocolCompatibility, Transport};
use crate::ui::DebugRow;

/// `per_second` bytes a second, as the window's count.
fn over_the_window(per_second: u64) -> u64 {
    per_second * u64::try_from(STATS_WINDOW.as_millis()).unwrap() / 1_000
}

/// A measured link: a 12 ms round trip with 1.5 ms of jitter, a quarter of
/// the window's packets lost, two resends, and 1200 B/s in and 3400 out.
fn measured() -> EndpointStats {
    EndpointStats {
        rtt: Some(Duration::from_millis(12)),
        rtt_variance: Duration::from_micros(1_500),
        recent: WindowCounts {
            bytes_sent: over_the_window(3_400),
            bytes_received: over_the_window(1_200),
            packets_acked: 15,
            packets_lost: 5,
            resends: 2,
        },
        ..EndpointStats::default()
    }
}

fn section(netgraph: &Netgraph) -> DebugSection {
    let mut out = DebugSection::default();
    netgraph.debug_section(&mut out);
    out
}

fn row(label: &str, value: &str) -> DebugRow {
    DebugRow {
        label: label.to_owned(),
        value: value.to_owned(),
    }
}

/// **A host shows a row per peer, with that peer's figures under the
/// heads**, and a peer whose transport measures nothing — a listen host's
/// own player — as dashes beside its snapshot size.
#[test]
fn a_host_shows_a_row_per_peer_with_its_figures() {
    let mut netgraph = Netgraph::new(Role::Host);
    netgraph.record(
        Duration::ZERO,
        [
            (
                1,
                LinkReading {
                    stats: Some(measured()),
                    snapshot_bytes: 857,
                },
            ),
            (
                3,
                LinkReading {
                    stats: None,
                    snapshot_bytes: 412,
                },
            ),
        ],
    );
    let out = section(&netgraph);
    assert_eq!(out.title(), "net");
    assert_eq!(
        &out.rows()[..3],
        [
            row("link", "   rtt   jit  loss rsnd  in B/s out B/s snap B"),
            row("peer 1", "  12.0   1.5  25.0    2    1200    3400    857"),
            row("peer 3", "     -     -     -    -       -       -    412"),
        ]
    );
    let graphs: Vec<&str> = out.graphs().iter().map(|g| g.label.as_str()).collect();
    assert_eq!(
        graphs,
        ["peer 1 rtt", "peer 1 snap", "peer 3 rtt", "peer 3 snap"]
    );
}

/// **A client shows one link, named for the host**, and before it is keyed
/// — no round trip yet — the round trip and jitter are dashes while the
/// counts it has are shown.
#[test]
fn a_client_shows_its_one_link_to_the_host() {
    let mut netgraph = Netgraph::new(Role::Client);
    let unmeasured = EndpointStats {
        rtt: None,
        ..measured()
    };
    netgraph.record(
        Duration::ZERO,
        [(
            0,
            LinkReading {
                stats: Some(unmeasured),
                snapshot_bytes: 0,
            },
        )],
    );
    let out = section(&netgraph);
    let links: Vec<&DebugRow> = out.rows().iter().filter(|r| r.label == "host").collect();
    assert_eq!(
        links,
        [&row(
            "host",
            "     -     -  25.0    2    1200    3400      0"
        )]
    );
    assert_eq!(
        out.rows()
            .iter()
            .filter(|r| r.label.starts_with("peer"))
            .count(),
        0
    );
}

/// With no link at all — a client still looking for a host — the section
/// says so rather than drawing an empty table.
#[test]
fn no_links_is_said_in_words() {
    let mut netgraph = Netgraph::new(Role::Client);
    netgraph.record(Duration::ZERO, []);
    assert_eq!(section(&netgraph).rows(), [row("links", "none yet")]);
    assert!(section(&netgraph).graphs().is_empty());
}

/// **A peer that left is dropped with its history, and one that joins
/// starts with none**, while a peer that stays keeps its own.
#[test]
fn a_peer_that_leaves_takes_its_history_and_a_new_one_starts_empty() {
    let reading = LinkReading {
        stats: Some(measured()),
        snapshot_bytes: 100,
    };
    let mut netgraph = Netgraph::new(Role::Host);
    netgraph.record(Duration::ZERO, [(1, reading), (2, reading)]);
    netgraph.record(HISTORY_INTERVAL, [(2, reading), (5, reading)]);
    let ids: Vec<u64> = netgraph.links().iter().map(|link| link.id).collect();
    assert_eq!(ids, [2, 5]);
    assert_eq!(netgraph.links()[0].rtt_ms.len(), 2, "peer 2 kept its own");
    assert_eq!(netgraph.links()[1].rtt_ms.len(), 1, "peer 5 started afresh");
}

/// **The history is bounded**: past [`HISTORY_SAMPLES`] the oldest goes,
/// and what is held is the newest, in order.
#[test]
fn the_history_holds_its_newest_samples_and_no_more() {
    let mut history = History::default();
    let pushed = HISTORY_SAMPLES + 10;
    for sample in 0..pushed {
        history.push(sample as f32);
    }
    assert_eq!(history.len(), HISTORY_SAMPLES);
    let held: Vec<f32> = history.iter().collect();
    let newest: Vec<f32> = (pushed - HISTORY_SAMPLES..pushed)
        .map(|s| s as f32)
        .collect();
    assert_eq!(held, newest);
}

/// **A link's history samples once an interval, on its boundaries**: a
/// second of 16 ms frames takes one sample per boundary passed, not one a
/// frame, and a frame slower than the interval takes one, not several. The
/// graph runs no faster than [`HISTORY_INTERVAL`] whatever the frame rate.
#[test]
fn the_history_samples_once_an_interval_whatever_the_frame_rate() {
    let frame = Duration::from_millis(16);
    let mut netgraph = Netgraph::new(Role::Client);
    let mut now = Duration::ZERO;
    while now < Duration::from_secs(1) {
        let reading = LinkReading {
            stats: Some(EndpointStats {
                rtt: Some(now),
                ..EndpointStats::default()
            }),
            snapshot_bytes: 0,
        };
        netgraph.record(now, [(0, reading)]);
        now += frame;
    }
    let boundaries = Duration::from_secs(1).as_nanos() / HISTORY_INTERVAL.as_nanos();
    let rtt = &netgraph.links()[0].rtt_ms;
    assert_eq!(rtt.len() as u128, boundaries);
    // The first frame at or past each boundary: 0, 112, 208, 304 ms ...
    assert_eq!(
        rtt.iter().take(4).collect::<Vec<_>>(),
        [0.0, 112.0, 208.0, 304.0]
    );

    netgraph.record(now + HISTORY_INTERVAL * 3, [(0, LinkReading::default())]);
    assert_eq!(
        netgraph.links()[0].rtt_ms.len() as u128,
        boundaries + 1,
        "a slow frame is one sample"
    );
}

/// **The graphs plot the history** against one round-trip scale for every
/// link — the largest sample, and no less than [`RTT_SCALE_FLOOR_MS`] — and
/// the snapshot against a datagram's payload.
#[test]
fn the_graphs_plot_the_history_against_their_scales() {
    let reading = |rtt_ms: u64, snapshot_bytes: usize| LinkReading {
        stats: Some(EndpointStats {
            rtt: Some(Duration::from_millis(rtt_ms)),
            ..EndpointStats::default()
        }),
        snapshot_bytes,
    };
    let mut netgraph = Netgraph::new(Role::Host);
    netgraph.record(Duration::ZERO, [(1, reading(5, 0)), (2, reading(10, 0))]);
    assert_eq!(
        netgraph.rtt_scale_ms(),
        RTT_SCALE_FLOOR_MS,
        "short round trips"
    );

    let half = MAX_UNRELIABLE_PAYLOAD / 2;
    netgraph.record(
        HISTORY_INTERVAL,
        [(1, reading(40, half)), (2, reading(10, 0))],
    );
    assert_eq!(netgraph.rtt_scale_ms(), 40.0);
    let out = section(&netgraph);
    let graph = |label: &str| {
        out.graphs()
            .iter()
            .find(|graph| graph.label == label)
            .unwrap_or_else(|| panic!("no {label} graph"))
            .bars
            .clone()
    };
    assert_eq!(graph("peer 1 rtt"), [5.0 / 40.0, 1.0]);
    assert_eq!(graph("peer 2 rtt"), [0.25, 0.25], "on peer 1's scale");
    assert_eq!(
        graph("peer 1 snap"),
        [0.0, half as f32 / MAX_UNRELIABLE_PAYLOAD as f32]
    );
}

// ── Over UDP loopback ────────────────────────────────────────────────────────

const GAME: LanGame = LanGame {
    app: "netgraph-test",
    host_name: "netgraph test",
    protocol_id: u32::from_be_bytes(*b"NETG"),
    compatibility: ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 7,
        schema_hash: 13,
    },
    max_players: 4,
};

const TICK_HZ: u32 = 60;

/// One frame at [`TICK_HZ`].
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The most frames the loopback test waits.
const MAX_FRAMES: usize = 1_200;

/// The pause after each frame, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// Whether a link's round trip has been measured.
fn measured_link(netgraph: &Netgraph) -> usize {
    netgraph
        .links()
        .iter()
        .filter(|link| link.reading.stats.is_some_and(|stats| stats.rtt.is_some()))
        .count()
}

/// **A host with two clients shows a row per peer, and each client one
/// link, with the figures their links report.** Everything binds loopback;
/// the frames are driven by hand.
#[test]
fn a_host_and_two_clients_over_loopback_show_their_links() {
    let bind = LanBind {
        listen: loopback(),
        announce_at: loopback(),
        broadcast_to: None,
    };
    let mut host = LanHost::open(GAME, bind, World::new(), TICK_HZ)
        .expect("loopback UDP must be available to these tests");
    let address: SocketAddr = (Ipv4Addr::LOCALHOST, host.game_port()).into();
    let mut clients = [
        LanClient::join(GAME, address, TICK_HZ).expect("connect"),
        LanClient::join(GAME, address, TICK_HZ).expect("connect"),
    ];

    let mut now = Duration::ZERO;
    let ready = |host: &LanHost, clients: &[LanClient]| {
        measured_link(host.netgraph()) == 2
            && clients.iter().all(|client| {
                measured_link(client.netgraph()) == 1
                    && client.netgraph().links()[0].reading.snapshot_bytes > 0
            })
    };
    let mut frames = 0;
    while !ready(&host, &clients) {
        assert!(frames < MAX_FRAMES, "the links were not measured in time");
        frames += 1;
        now += FRAME;
        host.frame(now);
        for client in &mut clients {
            client.frame(now);
        }
        thread::sleep(PAUSE);
    }

    let lan = host.host();
    let peers: Vec<u64> = lan.peers().map(|peer| peer.get()).collect();
    let shown: Vec<u64> = host.netgraph().links().iter().map(|link| link.id).collect();
    assert_eq!(shown, peers, "a row per peer, in the host's order");
    assert_eq!(peers.len(), 2);
    for (link, peer) in host.netgraph().links().iter().zip(lan.peers()) {
        let reported = lan
            .peer_link_stats(peer)
            .expect("a UDP peer measures its link");
        let recorded = link.reading.stats.expect("recorded at the last frame");
        // Read after the frame with nothing driven since, so every figure
        // but the windowed ones — which move with the wall clock — agrees.
        assert_eq!(recorded.rtt, reported.rtt);
        assert_eq!(recorded.packets_acked, reported.packets_acked);
        assert_eq!(recorded.bytes_sent, reported.bytes_sent);
        assert_eq!(
            link.reading.snapshot_bytes,
            lan.peer_stats(peer).unwrap().last_snapshot_bytes
        );
        assert!(link.reading.snapshot_bytes > 0);
    }
    let rows = section(host.netgraph());
    let peer_rows: Vec<&str> = rows
        .rows()
        .iter()
        .filter(|row| row.label.starts_with("peer "))
        .map(|row| row.label.as_str())
        .collect();
    assert_eq!(
        peer_rows,
        peers
            .iter()
            .map(|id| format!("peer {id}"))
            .collect::<Vec<_>>()
    );

    for client in &clients {
        let session = client.client().expect("joined");
        let [link] = client.netgraph().links() else {
            panic!("a client shows one link: {:?}", client.netgraph().links());
        };
        let reported = session.transport().link_stats().expect("keyed");
        assert_eq!(link.reading.stats.unwrap().rtt, reported.rtt);
        assert_eq!(link.reading.snapshot_bytes, session.last_snapshot_bytes());
        let rows = section(client.netgraph());
        assert_eq!(
            rows.rows().iter().filter(|row| row.label == "host").count(),
            1
        );
    }
}
