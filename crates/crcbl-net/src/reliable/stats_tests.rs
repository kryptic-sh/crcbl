//! The netgraph's figures off a scripted wire: round trip, jitter, loss over
//! the window, and bytes and resends over it, each checked against what the
//! script makes them by hand.
//!
//! [`super::tests`] runs the endpoints over the seeded `ConditionSimulator`,
//! whose loss and delay are random draws; that proves convergence, not a
//! value. Here each direction is a [`ScriptedWire`] that delays or drops the
//! n-th datagram exactly as the test says, on the one shared [`ManualClock`],
//! so every figure is a number worked out in the test's own comments.

use std::time::Duration;

use super::tests::{PROTOCOL, STEP, accept};
use super::*;
use crate::{Clock, ManualClock};

/// What becomes of a datagram: delivered after this long, or `None` to drop
/// it. Asked with the datagram's index on its wire, counting from zero, and
/// its decoded body.
type Fate = Box<dyn FnMut(usize, &PacketBody<'_>) -> Option<Duration>>;

/// One direction of a link, delaying and dropping exactly as scripted, and
/// logging what it carried so a test can sum bytes independently of the
/// endpoint's own counters.
struct ScriptedWire {
    fate: Fate,
    sent: usize,
    in_flight: Vec<(Duration, Vec<u8>)>,
    /// Every datagram put on the wire: when, and how long.
    put_log: Vec<(Duration, usize)>,
    /// Every datagram delivered: when, and how long.
    delivered_log: Vec<(Duration, usize)>,
}

impl ScriptedWire {
    fn new(fate: Fate) -> Self {
        Self {
            fate,
            sent: 0,
            in_flight: Vec::new(),
            put_log: Vec::new(),
            delivered_log: Vec::new(),
        }
    }

    fn put(&mut self, now: Duration, datagram: Vec<u8>) {
        self.put_log.push((now, datagram.len()));
        let (_, body) = decode_packet(&datagram, PROTOCOL).expect("an endpoint's own packet");
        let fate = (self.fate)(self.sent, &body);
        self.sent += 1;
        if let Some(delay) = fate {
            self.in_flight.push((now + delay, datagram));
        }
    }

    /// Everything due by `now`, in the order it falls due.
    fn take(&mut self, now: Duration) -> Vec<Vec<u8>> {
        // Stable, so two datagrams due together keep the order they were
        // sent in.
        self.in_flight.sort_by_key(|(due, _)| *due);
        let due = self.in_flight.partition_point(|(due, _)| *due <= now);
        let arrived: Vec<Vec<u8>> = self.in_flight.drain(..due).map(|(_, d)| d).collect();
        for datagram in &arrived {
            self.delivered_log.push((now, datagram.len()));
        }
        arrived
    }
}

/// Two endpoints over two scripted wires. Each step advances the clock by
/// [`STEP`], then `a` polls, `b` polls, and each takes what is due — so a
/// packet arriving in one step is answered in the next, and a round trip is
/// the two delays plus one step.
struct Link {
    clock: ManualClock,
    a: Endpoint<ManualClock>,
    b: Endpoint<ManualClock>,
    a_to_b: ScriptedWire,
    b_to_a: ScriptedWire,
}

impl Link {
    fn new(a_to_b: Fate, b_to_a: Fate) -> Self {
        let clock = ManualClock::new();
        Self {
            a: Endpoint::new(PROTOCOL, clock.clone()),
            b: Endpoint::new(PROTOCOL, clock.clone()),
            a_to_b: ScriptedWire::new(a_to_b),
            b_to_a: ScriptedWire::new(b_to_a),
            clock,
        }
    }

    fn step(&mut self) {
        self.clock.advance(STEP);
        let now = self.clock.now();
        while let Some(datagram) = self.a.poll_outgoing() {
            self.a_to_b.put(now, datagram);
        }
        while let Some(datagram) = self.b.poll_outgoing() {
            self.b_to_a.put(now, datagram);
        }
        for datagram in self.a_to_b.take(now) {
            accept(&mut self.b, &datagram);
        }
        for datagram in self.b_to_a.take(now) {
            accept(&mut self.a, &datagram);
        }
        while self.a.recv().is_some() {}
        while self.b.recv().is_some() {}
    }

    fn run(&mut self, duration: Duration) {
        for _ in 0..duration.as_nanos() / STEP.as_nanos() {
            self.step();
        }
    }

    /// Steps until `a` has measured `samples` round trips' worth of change in
    /// its estimate — tracked through the smoothed value, which every sample
    /// on this link moves.
    fn run_until_rtt_changes(&mut self, samples: usize) -> Vec<(Option<Duration>, Duration)> {
        let mut seen = Vec::new();
        let mut last = (self.a.stats().rtt, self.a.stats().rtt_variance);
        for _ in 0..10_000 {
            self.step();
            let stats = self.a.stats();
            let now = (stats.rtt, stats.rtt_variance);
            if now != last {
                seen.push(now);
                last = now;
                if seen.len() == samples {
                    return seen;
                }
            }
        }
        panic!("only {} round trips were measured: {seen:?}", seen.len());
    }
}

/// Every datagram after `delay`, none dropped.
fn steady(delay: Duration) -> Fate {
    Box::new(move |_, _| Some(delay))
}

const fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// The window `now` reads: the [`STATS_BUCKETS`] complete buckets before the
/// one `now` is in.
fn window_at(now: Duration) -> std::ops::Range<Duration> {
    let bucket = now.as_nanos() / STATS_BUCKET.as_nanos();
    let end = Duration::from_nanos(u64::try_from(bucket * STATS_BUCKET.as_nanos()).unwrap());
    end.saturating_sub(STATS_WINDOW)..end
}

fn bytes_in(log: &[(Duration, usize)], window: &std::ops::Range<Duration>) -> u64 {
    log.iter()
        .filter(|(at, _)| window.contains(at))
        .map(|&(_, len)| len as u64)
        .sum()
}

/// **The round trip and the jitter are RFC 6298's over the scripted
/// delays.** An idle link measures on keepalives alone. `a`'s keepalives
/// cross in 10 ms and 30 ms by turns, every other packet in 10 ms, and `b`
/// acks on the step after arrival, so the samples are 10 + 5 + 10 = 25 ms
/// and 30 + 5 + 10 = 45 ms by turns. By hand:
///
/// - 25: SRTT 25, RTTVAR 12.5.
/// - 45: RTTVAR 0.75 * 12.5 + 0.25 * 20 = 14.375; SRTT 0.875 * 25 +
///   0.125 * 45 = 27.5.
/// - 25: RTTVAR 0.75 * 14.375 + 0.25 * 2.5 = 11.40625; SRTT 0.875 * 27.5 +
///   0.125 * 25 = 27.1875.
#[test]
fn the_round_trip_and_jitter_follow_a_scripted_delay_pattern() {
    let mut keepalives = 0usize;
    let mut link = Link::new(
        Box::new(move |_, body| {
            Some(if matches!(body, PacketBody::Keepalive) {
                keepalives += 1;
                if keepalives % 2 == 1 { ms(10) } else { ms(30) }
            } else {
                ms(10)
            })
        }),
        steady(ms(10)),
    );
    let seen = link.run_until_rtt_changes(3);
    assert_eq!(
        seen,
        [
            (Some(ms(25)), Duration::from_micros(12_500)),
            (
                Some(Duration::from_micros(27_500)),
                Duration::from_micros(14_375)
            ),
            (
                Some(Duration::from_nanos(27_187_500)),
                Duration::from_nanos(11_406_250),
            ),
        ],
    );
}

/// **Loss over the window is the script's.** `a` sends two packets every
/// keepalive interval — its keepalive, and the ack for `b`'s — and the wire
/// drops every fourth, so one packet in four is lost. The pattern repeats
/// every two intervals, and the window holds a whole number of repeats, so
/// once the link is steady the window judges exactly a quarter lost, from
/// any phase. The smoothed figure is a different thing — an average that
/// has been converging since the start — and is not what this reads.
#[test]
fn loss_over_the_window_matches_a_scripted_drop_pattern() {
    let mut link = Link::new(
        Box::new(|index, _| (index % 4 != 3).then_some(ms(10))),
        steady(ms(10)),
    );
    link.run(STATS_WINDOW * 3);
    for _ in 0..(STATS_BUCKET.as_nanos() / STEP.as_nanos()) * 4 {
        link.step();
        let recent = link.a.stats().recent;
        assert_eq!(
            (recent.packets_lost, recent.packets_acked),
            (5, 15),
            "at {:?}",
            link.clock.now()
        );
        assert_eq!(recent.loss(), Some(0.25));
    }
    assert_eq!(
        link.b.stats().recent.loss(),
        Some(0.0),
        "b's direction drops nothing"
    );
}

/// **Bytes over the window are what crossed the wire in it.** The endpoint's
/// windowed counts are held against the wire's own log of what `a` put on
/// it and what reached `a`, summed over the same complete buckets, at every
/// step across several bucket boundaries.
#[test]
fn bytes_over_the_window_are_what_crossed_the_wire() {
    let mut link = Link::new(steady(ms(15)), steady(ms(5)));
    let mut grow = 0usize;
    link.run(STATS_WINDOW + STATS_BUCKET / 2);
    for n in 0..(STATS_BUCKET.as_nanos() / STEP.as_nanos()) * 3 {
        // Traffic that grows, so a window off by a bucket reads differently.
        grow += 1;
        for _ in 0..(n % 3) {
            link.a
                .send(Channel::UnreliableSequenced, vec![7; 40 + grow])
                .unwrap();
        }
        link.step();
        let now = link.clock.now();
        let window = window_at(now);
        let recent = link.a.stats().recent;
        assert_eq!(
            recent.bytes_sent,
            bytes_in(&link.a_to_b.put_log, &window),
            "sent, at {now:?}"
        );
        assert_eq!(
            recent.bytes_received,
            bytes_in(&link.b_to_a.delivered_log, &window),
            "received, at {now:?}"
        );
        assert!(recent.bytes_sent > 0 && recent.bytes_received > 0);
    }
}

/// **A resend shows in the window, then rolls out of it.** One reliable
/// message whose first transmission the wire drops is sent again on its
/// timeout; the window counts that resend from the bucket after it, for
/// exactly [`STATS_WINDOW`], and the session total keeps it.
#[test]
fn a_resend_enters_the_window_and_rolls_out_of_it() {
    let mut link = Link::new(
        Box::new(|index, body| {
            let first_fragment = index == 0 && matches!(body, PacketBody::Reliable(_));
            (!first_fragment).then_some(ms(10))
        }),
        steady(ms(10)),
    );
    link.a.send(Channel::Reliable, b"hello".to_vec()).unwrap();
    let mut resent_at = None;
    for _ in 0..10_000 {
        link.step();
        if link.a.stats().resends == 1 {
            resent_at = Some(link.clock.now());
            break;
        }
    }
    let resent_at = resent_at.expect("the dropped fragment was sent again");
    assert_eq!(
        link.a.stats().recent.resends,
        0,
        "the resend's bucket is still filling"
    );

    let resend_bucket = resent_at.as_nanos() / STATS_BUCKET.as_nanos();
    let enters =
        Duration::from_nanos(u64::try_from((resend_bucket + 1) * STATS_BUCKET.as_nanos()).unwrap());
    let leaves = enters + STATS_WINDOW;
    while link.clock.now() < leaves {
        let in_window = u64::from(link.clock.now() >= enters);
        assert_eq!(
            link.a.stats().recent.resends,
            in_window,
            "at {:?}",
            link.clock.now()
        );
        link.step();
    }
    assert_eq!(link.a.stats().recent.resends, 0, "rolled out at {leaves:?}");
    assert_eq!(link.a.stats().resends, 1, "the session total keeps it");
}
