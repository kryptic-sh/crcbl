//! Two endpoints over a seeded lossy wire.
//!
//! The wire is the crate's own [`ConditionSimulator`] on its unreliable
//! channel, one per direction, each datagram one message: seeded loss,
//! duplication, reordering, latency and jitter, all timed by one shared
//! [`ManualClock`]. Every run is a pure function of its seed.

use std::collections::VecDeque;
use std::time::Duration;

use super::*;
use crate::{
    Clock, ConditionSimulator, InMemoryTransport, ManualClock, Message, SimConditions, Transport,
    TransportError,
};

pub(crate) const PROTOCOL: u32 = 0x4352_4342;

/// Simulated time per step of the driving loop: every endpoint polls and
/// receives once per step, like a game loop's frame.
pub(crate) const STEP: Duration = Duration::from_millis(5);

/// One direction of the wire. `crate::seal`'s tests run sealed traffic
/// through it too.
pub(crate) struct Wire {
    sim: ConditionSimulator<InMemoryTransport, ManualClock>,
    far: InMemoryTransport,
    arrived: VecDeque<Vec<u8>>,
}

impl Wire {
    pub(crate) fn new(conditions: SimConditions, clock: ManualClock) -> Self {
        let (near, far) = InMemoryTransport::pair();
        Self {
            sim: ConditionSimulator::with_clock(near, conditions, clock),
            far,
            arrived: VecDeque::new(),
        }
    }

    pub(crate) fn put(&mut self, datagram: Vec<u8>) {
        loop {
            match self
                .sim
                .send_unreliable(Message::unreliable(datagram.clone()))
            {
                Ok(()) => return,
                // The in-memory channel under the simulator is bounded; empty
                // it and offer the datagram again rather than lose it to a
                // limit the conditions never asked for.
                Err(TransportError::Backpressure) => self.collect(),
                Err(e) => panic!("the wire refused a datagram: {e}"),
            }
        }
    }

    /// Release everything due and gather what reached the far end.
    fn collect(&mut self) {
        loop {
            // `recv` on the near end is how the simulator is asked to release
            // what is due; nothing is ever sent to the near end, so it never
            // returns a message of its own.
            let released = self.sim.recv();
            while let Some(message) = self.far.recv().expect("the far end stays connected") {
                self.arrived.push_back(message.payload);
            }
            match released {
                Ok(None) => return,
                Ok(Some(_)) => panic!("nothing is sent towards the near end"),
                Err(TransportError::Backpressure) => {}
                Err(e) => panic!("the wire failed: {e}"),
            }
        }
    }

    pub(crate) fn take(&mut self) -> Vec<Vec<u8>> {
        self.collect();
        self.arrived.drain(..).collect()
    }
}

/// Two endpoints and the wire between them.
struct Link {
    clock: ManualClock,
    a: Endpoint<ManualClock>,
    b: Endpoint<ManualClock>,
    a_to_b: Wire,
    b_to_a: Wire,
    a_got: Vec<Delivery>,
    b_got: Vec<Delivery>,
    /// Whether each step drains deliveries into `a_got` / `b_got`.
    draining: bool,
    /// Unreliable packets `a` put on the wire.
    a_unreliable_packets: usize,
}

impl Link {
    fn new(conditions: SimConditions) -> Self {
        Self::starting_at(conditions, 0, 0)
    }

    fn starting_at(conditions: SimConditions, sequence: u16, message_id: u16) -> Self {
        let clock = ManualClock::new();
        let back = SimConditions {
            seed: conditions.seed ^ 0x9E37_79B9_7F4A_7C15,
            ..conditions.clone()
        };
        Self {
            a: Endpoint::starting_at(PROTOCOL, clock.clone(), sequence, message_id),
            b: Endpoint::starting_at(
                PROTOCOL,
                clock.clone(),
                sequence.wrapping_add(7_000),
                message_id,
            ),
            a_to_b: Wire::new(conditions, clock.clone()),
            b_to_a: Wire::new(back, clock.clone()),
            clock,
            a_got: Vec::new(),
            b_got: Vec::new(),
            draining: true,
            a_unreliable_packets: 0,
        }
    }

    fn step(&mut self) {
        self.clock.advance(STEP);
        while let Some(datagram) = self.a.poll_outgoing() {
            if let Ok((_, PacketBody::Unreliable(_))) = decode_packet(&datagram, PROTOCOL) {
                self.a_unreliable_packets += 1;
            }
            self.a_to_b.put(datagram);
        }
        while let Some(datagram) = self.b.poll_outgoing() {
            self.b_to_a.put(datagram);
        }
        for datagram in self.a_to_b.take() {
            accept(&mut self.b, &datagram);
        }
        for datagram in self.b_to_a.take() {
            accept(&mut self.a, &datagram);
        }
        if self.draining {
            while let Some(delivery) = self.a.recv() {
                self.a_got.push(delivery);
            }
            while let Some(delivery) = self.b.recv() {
                self.b_got.push(delivery);
            }
        }
    }

    fn run(&mut self, duration: Duration) {
        for _ in 0..duration.as_nanos() / STEP.as_nanos() {
            self.step();
        }
    }

    /// Step until `done` holds, or `limit` of simulated time has passed;
    /// returns whether it held.
    fn run_until(&mut self, limit: Duration, done: impl Fn(&Self) -> bool) -> bool {
        for _ in 0..limit.as_nanos() / STEP.as_nanos() {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }
}

/// Honest traffic is never refused as malformed. A closed endpoint refusing
/// the peer's last packets, and a receiver whose application is not reading
/// refusing fragments until it does, are the expected errors.
pub(crate) fn accept(endpoint: &mut Endpoint<ManualClock>, datagram: &[u8]) {
    match endpoint.receive_datagram(datagram) {
        Ok(()) | Err(ReceiveError::Closed | ReceiveError::DeliveriesFull { .. }) => {}
        Err(e) => panic!("an honest datagram was refused: {e}"),
    }
}

pub(crate) fn payloads(deliveries: &[Delivery], channel: Channel) -> Vec<Vec<u8>> {
    deliveries
        .iter()
        .filter(|delivery| delivery.channel == channel)
        .map(|delivery| delivery.payload.clone())
        .collect()
}

/// A reliable message whose bytes say which one it is, and every so often
/// one large enough to be split.
pub(crate) fn numbered_message(n: usize) -> Vec<u8> {
    let len = if n % 37 == 5 {
        MAX_FRAGMENT_BYTES * 2 + 17
    } else {
        4 + n % 50
    };
    let mut bytes = vec![(n % 251) as u8; len];
    bytes[..4].copy_from_slice(&(n as u32).to_le_bytes());
    bytes
}

pub(crate) fn hostile_conditions(seed: u64) -> SimConditions {
    SimConditions {
        loss_rate: 0.25,
        latency: Duration::from_millis(20),
        jitter: Duration::from_millis(15),
        duplicate_rate: 0.1,
        reorder_window: 4,
        seed,
    }
}

// ── Reliable-ordered ─────────────────────────────────────────────────────────

/// **The reliability soak.** A quarter of every datagram lost, a tenth
/// duplicated, reordering by window and by jitter, both directions at once,
/// sequences and message ids starting next to the wrap so both cross it: every
/// reliable message arrives, once, in order, with its bytes intact.
#[test]
fn reliable_messages_all_arrive_once_and_in_order_through_a_hostile_wire() {
    const MESSAGES: usize = 300;
    for seed in 0..16u64 {
        let mut link = Link::starting_at(
            hostile_conditions(seed),
            65_300 + seed as u16,
            65_450 + seed as u16,
        );
        let expected: Vec<Vec<u8>> = (0..MESSAGES).map(numbered_message).collect();
        let (mut a_next, mut b_next) = (0, 0);
        let finished = loop {
            while a_next < MESSAGES
                && link
                    .a
                    .send(Channel::Reliable, expected[a_next].clone())
                    .is_ok()
            {
                a_next += 1;
            }
            while b_next < MESSAGES
                && link
                    .b
                    .send(Channel::Reliable, expected[b_next].clone())
                    .is_ok()
            {
                b_next += 1;
            }
            link.step();
            if link.b_got.len() == MESSAGES && link.a_got.len() == MESSAGES {
                break true;
            }
            if link.clock.now() > Duration::from_secs(300) {
                break false;
            }
        };
        assert!(
            finished,
            "seed {seed}: {} and {} of {MESSAGES} delivered",
            link.b_got.len(),
            link.a_got.len()
        );
        assert_eq!(
            payloads(&link.b_got, Channel::Reliable),
            expected,
            "seed {seed}"
        );
        assert_eq!(
            payloads(&link.a_got, Channel::Reliable),
            expected,
            "seed {seed}"
        );
        assert!(
            link.a.stats().resends > 0,
            "seed {seed}: the loss must have cost resends"
        );
        assert!(link.a.state().is_connected() && link.b.state().is_connected());
    }
}

/// **Fragmentation under loss.** The largest message there is, split into
/// every fragment it may have, through a lossy, duplicating, reordering wire.
#[test]
fn the_largest_reliable_message_reassembles_intact_under_loss() {
    for seed in [3u64, 11, 29] {
        let mut link = Link::new(hostile_conditions(seed));
        let message: Vec<u8> = (0..MAX_RELIABLE_MESSAGE_BYTES)
            .map(|i| (i.wrapping_mul(31) ^ (i >> 8)) as u8)
            .collect();
        link.a.send(Channel::Reliable, message.clone()).unwrap();
        assert!(link.run_until(Duration::from_secs(60), |link| !link.b_got.is_empty()));
        assert_eq!(link.b_got.len(), 1);
        assert_eq!(link.b_got[0].payload, message, "seed {seed}");
    }
}

#[test]
fn a_message_past_its_channel_limit_is_refused_and_nothing_is_queued() {
    let mut link = Link::new(SimConditions::default());
    assert!(matches!(
        link.a.send(Channel::Reliable, vec![0; MAX_RELIABLE_MESSAGE_BYTES + 1]),
        Err(TransportError::MessageTooLarge { limit, .. }) if limit == MAX_RELIABLE_MESSAGE_BYTES
    ));
    // The one-datagram rule: the unreliable channel refuses rather than
    // splits.
    assert!(matches!(
        link.a.send(Channel::UnreliableSequenced, vec![0; MAX_UNRELIABLE_PAYLOAD + 1]),
        Err(TransportError::MessageTooLarge { limit, .. }) if limit == MAX_UNRELIABLE_PAYLOAD
    ));
    assert_eq!(link.a.buffered_bytes(), 0);

    link.a
        .send(
            Channel::UnreliableSequenced,
            vec![9; MAX_UNRELIABLE_PAYLOAD],
        )
        .unwrap();
    link.step();
    assert_eq!(link.b_got.len(), 1);
    assert_eq!(link.b_got[0].payload.len(), MAX_UNRELIABLE_PAYLOAD);
}

/// A burst far longer than the ack bitfield, on a perfect wire, needs no
/// resend at all: the receiver's snapshotted acks cover all of it, not just
/// its tail.
#[test]
fn a_burst_longer_than_the_ack_bitfield_is_acknowledged_whole() {
    let mut link = Link::new(SimConditions {
        latency: Duration::from_millis(10),
        ..SimConditions::default()
    });
    link.a
        .send(Channel::Reliable, vec![1; MAX_RELIABLE_MESSAGE_BYTES])
        .unwrap();
    link.run(Duration::from_secs(2));
    assert_eq!(link.b_got.len(), 1);
    assert_eq!(link.a.stats().resends, 0);
    assert_eq!(link.a.reliable_in_flight(), 0);
}

// ── Unreliable-sequenced ─────────────────────────────────────────────────────

/// **Latest wins, and nothing is resent.** Through a wire that duplicates
/// and reorders, the delivered counters only ever rise; every send is exactly
/// one packet on the wire, however much was lost.
#[test]
fn the_sequenced_channel_never_delivers_older_after_newer_and_never_resends() {
    const SENDS: u32 = 400;
    for seed in 0..8u64 {
        let mut link = Link::starting_at(hostile_conditions(seed), 65_400, 0);
        for n in 0..SENDS {
            link.a
                .send(Channel::UnreliableSequenced, n.to_le_bytes().to_vec())
                .unwrap();
            link.step();
        }
        link.run(Duration::from_secs(1));

        let counters: Vec<u32> = payloads(&link.b_got, Channel::UnreliableSequenced)
            .iter()
            .map(|bytes| u32::from_le_bytes(bytes[..4].try_into().unwrap()))
            .collect();
        assert!(
            counters.windows(2).all(|pair| pair[0] < pair[1]),
            "seed {seed}: a stale payload was delivered after a fresher one"
        );
        // Not vacuous: a good share got through, and some were dropped for
        // arriving behind a fresher one rather than lost on the wire. With
        // jitter several sends wide, that share is well under the three
        // quarters the wire delivers.
        assert!(counters.len() > SENDS as usize / 4, "seed {seed}");
        assert!(link.b.stats().unreliable_dropped > 0, "seed {seed}");
        assert_eq!(link.a_unreliable_packets, SENDS as usize, "seed {seed}");
        assert_eq!(link.a.stats().resends, 0);
    }
}

/// A channel idle for most of the sequence space still takes a fresh
/// packet: the latest-wins mark is dragged along behind the newest packet
/// rather than left to wrap into "newer than everything".
#[test]
fn a_long_idle_sequenced_channel_still_delivers_after_the_sequence_moves_on() {
    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    let packet = |sequence: u16, body: PacketBody<'_>| {
        encode_packet(
            PROTOCOL,
            &PacketHeader {
                sequence,
                acks: None,
            },
            &body,
        )
    };
    endpoint
        .receive_datagram(&packet(0, PacketBody::Unreliable(b"old")))
        .unwrap();
    // Keepalives walk the sequence forward in jumps under half the space.
    for sequence in [16_000, 32_000, 48_000] {
        endpoint
            .receive_datagram(&packet(sequence, PacketBody::Keepalive))
            .unwrap();
    }
    endpoint
        .receive_datagram(&packet(48_001, PacketBody::Unreliable(b"fresh")))
        .unwrap();
    assert_eq!(endpoint.recv().unwrap().payload, b"old");
    assert_eq!(endpoint.recv().unwrap().payload, b"fresh");
}

// ── Round trip and loss ──────────────────────────────────────────────────────

/// The estimate settles on the wire's round trip: twice the one-way latency,
/// plus at most the loop's granularity, since an ack goes out on the step
/// after the packet it answers arrives.
#[test]
fn the_round_trip_estimate_converges_on_the_wire_latency() {
    let latency = Duration::from_millis(50);
    let mut link = Link::new(SimConditions {
        latency,
        ..SimConditions::default()
    });
    for n in 0..600 {
        link.a.send(Channel::Reliable, vec![n as u8]).unwrap();
        link.step();
    }
    let stats = link.a.stats();
    let rtt = stats
        .rtt
        .expect("acks returned, so a round trip was measured");
    assert!(
        rtt >= latency * 2 && rtt <= latency * 2 + STEP * 2,
        "rtt {rtt:?} for a {latency:?} one-way wire"
    );
    assert!(
        stats.rtt_variance <= STEP,
        "jitter {:?}",
        stats.rtt_variance
    );
    assert!(stats.packet_loss < 0.01);
}

/// The loss estimate reads the wire's configured loss back, give or take.
#[test]
fn the_loss_estimate_tracks_the_wire() {
    let mut link = Link::new(SimConditions {
        loss_rate: 0.25,
        latency: Duration::from_millis(20),
        seed: 5,
        ..SimConditions::default()
    });
    for n in 0..4_000u32 {
        link.a
            .send(Channel::UnreliableSequenced, n.to_le_bytes().to_vec())
            .unwrap();
        link.step();
    }
    let stats = link.a.stats();
    // Unreliable packets are never acked promptly, so this round trip came
    // from the keepalives that run beside them.
    assert!(stats.rtt.is_some());
    let measured = stats.packets_lost as f64 / (stats.packets_lost + stats.packets_acked) as f64;
    assert!((0.18..0.32).contains(&measured), "counted loss {measured}");
    assert!(
        (0.1..0.4).contains(&stats.packet_loss),
        "smoothed loss {}",
        stats.packet_loss
    );
}

// ── Keepalive, timeout and disconnect ────────────────────────────────────────

/// Two idle peers keep each other alive, and measure the round trip, on
/// keepalives alone.
#[test]
fn idle_peers_keep_each_other_alive_with_keepalives() {
    let mut link = Link::new(SimConditions {
        latency: Duration::from_millis(15),
        ..SimConditions::default()
    });
    link.run(PEER_TIMEOUT * 3);
    assert!(link.a.state().is_connected() && link.b.state().is_connected());
    assert!(link.a.stats().packets_sent > 0);
    assert!(link.a.stats().rtt.is_some());
    assert!(link.a_got.is_empty() && link.b_got.is_empty());
}

/// A requested keepalive goes out on the next poll, with no time passed, and
/// once only: the interval then governs again.
#[test]
fn a_requested_keepalive_goes_out_at_once_and_once() {
    let clock = ManualClock::new();
    let mut endpoint = Endpoint::new(PROTOCOL, clock.clone());
    assert!(endpoint.poll_outgoing().is_none(), "nothing is due yet");
    endpoint.request_keepalive();
    let datagram = endpoint.poll_outgoing().expect("the requested keepalive");
    assert!(matches!(
        decode_packet(&datagram, PROTOCOL),
        Ok((_, PacketBody::Keepalive))
    ));
    assert!(
        endpoint.poll_outgoing().is_none(),
        "one keepalive per request"
    );
}

/// A peer that stops answering is declared timed out after exactly
/// [`PEER_TIMEOUT`] of silence, and not a step before.
#[test]
fn a_silent_peer_is_timed_out_after_the_timeout_and_not_before() {
    let clock = ManualClock::new();
    let mut endpoint = Endpoint::new(PROTOCOL, clock.clone());
    clock.advance(PEER_TIMEOUT - Duration::from_nanos(1));
    endpoint.update();
    assert_eq!(endpoint.state(), EndpointState::Connected);
    clock.advance(Duration::from_nanos(1));
    endpoint.update();
    assert_eq!(endpoint.state(), EndpointState::TimedOut);
    assert!(matches!(
        endpoint.send(Channel::Reliable, Vec::new()),
        Err(TransportError::Disconnected)
    ));
    assert!(endpoint.poll_outgoing().is_none());
}

/// A graceful disconnect crosses a lossy wire and is reported as a
/// disconnect, not a timeout, on both sides.
#[test]
fn a_disconnect_reaches_the_peer_as_a_disconnect_not_a_timeout() {
    let mut link = Link::new(SimConditions {
        loss_rate: 0.3,
        latency: Duration::from_millis(20),
        seed: 17,
        ..SimConditions::default()
    });
    link.run(Duration::from_millis(200));
    link.a.disconnect();
    assert_eq!(link.a.state(), EndpointState::Disconnecting);
    link.run(Duration::from_millis(200));
    assert_eq!(link.a.state(), EndpointState::Disconnected);
    assert_eq!(link.b.state(), EndpointState::PeerDisconnected);

    // And it stays that: the timeout never overwrites it.
    link.run(PEER_TIMEOUT * 2);
    assert_eq!(link.b.state(), EndpointState::PeerDisconnected);
    assert!(matches!(
        link.b.send(Channel::Reliable, Vec::new()),
        Err(TransportError::Disconnected)
    ));
}

// ── Hostile input and bounds ─────────────────────────────────────────────────

/// Malformed and forged datagrams mid-transfer: each is refused with a typed
/// error, leaves every counter and buffer exactly as it was, and the transfer
/// then completes intact.
#[test]
fn hostile_datagrams_are_refused_without_touching_the_link() {
    let mut link = Link::new(hostile_conditions(41));
    let message: Vec<u8> = (0..MAX_FRAGMENT_BYTES * 5).map(|i| i as u8).collect();
    link.a.send(Channel::Reliable, message.clone()).unwrap();
    link.run(Duration::from_millis(40));

    let header = |acks| PacketHeader {
        sequence: 30_000,
        acks,
    };
    let forged_fragment = |message_id: u16, index: u8, count: u8, len: usize| {
        let payload = vec![0xEE; len];
        encode_packet(
            PROTOCOL,
            &header(None),
            &PacketBody::Reliable(Fragment {
                message_id,
                index,
                count,
                payload: &payload,
            }),
        )
    };
    let keepalive = encode_packet(PROTOCOL, &header(None), &PacketBody::Keepalive);
    let mut hostile: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0xFF; 3],
        vec![0; MAX_PACKET_BYTES + 1],
        encode_packet(PROTOCOL ^ 1, &header(None), &PacketBody::Keepalive),
        [keepalive.as_slice(), &[0]].concat(),
        // Out of the reorder window, a zero and an excessive fragment count,
        // an index past its count, and a middle fragment of the wrong size.
        forged_fragment(1_000, 0, 1, 8),
        forged_fragment(0, 0, 0, 8),
        forged_fragment(0, 0, u8::MAX, 8),
        forged_fragment(0, 3, 3, 8),
        forged_fragment(0, 0, 3, 8),
    ];
    let mut unknown = keepalive.clone();
    unknown[4] = 0x7E;
    hostile.push(unknown);
    let mut stray_acks = keepalive;
    stray_acks[8] = 0x55;
    hostile.push(stray_acks);
    let mut seed = 0xBAD_u64;
    for _ in 0..500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = (seed >> 33) as usize % (MAX_PACKET_BYTES + 32);
        hostile.push(
            (0..len)
                .map(|i| (seed >> (i % 7 * 8)) as u8 ^ i as u8)
                .collect(),
        );
    }

    for datagram in &hostile {
        let stats = link.b.stats();
        let buffered = link.b.buffered_bytes();
        let state = link.b.state();
        let acks = link.b.current_acks();
        let refused = link.b.receive_datagram(datagram);
        assert!(
            refused.is_err(),
            "a {}-byte hostile datagram was accepted",
            datagram.len()
        );
        assert_eq!(link.b.stats(), stats);
        assert_eq!(link.b.buffered_bytes(), buffered);
        assert_eq!(link.b.state(), state);
        assert_eq!(
            link.b.current_acks(),
            acks,
            "a refused packet must not be acked"
        );
    }

    assert!(link.run_until(Duration::from_secs(60), |link| !link.b_got.is_empty()));
    assert_eq!(link.b_got.len(), 1);
    assert_eq!(link.b_got[0].payload, message);
}

/// A sender that is never drained stops at its caps with backpressure, and
/// never holds more than they allow.
#[test]
fn a_sender_whose_link_is_dead_stops_at_its_caps() {
    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    let mut accepted = 0;
    loop {
        match endpoint.send(Channel::Reliable, vec![0; 100]) {
            Ok(()) => accepted += 1,
            Err(TransportError::Backpressure) => break,
            Err(e) => panic!("unexpected refusal: {e}"),
        }
    }
    assert_eq!(accepted, RELIABLE_WINDOW);

    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    while endpoint
        .send(Channel::Reliable, vec![0; MAX_RELIABLE_MESSAGE_BYTES])
        .is_ok()
    {}
    assert!(endpoint.buffered_bytes() <= MAX_RELIABLE_BYTES_IN_FLIGHT);
    assert!(endpoint.buffered_bytes() + MAX_RELIABLE_MESSAGE_BYTES > MAX_RELIABLE_BYTES_IN_FLIGHT);

    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    for _ in 0..MAX_QUEUED_UNRELIABLE {
        endpoint
            .send(Channel::UnreliableSequenced, vec![0; 8])
            .unwrap();
    }
    assert!(matches!(
        endpoint.send(Channel::UnreliableSequenced, vec![0; 8]),
        Err(TransportError::Backpressure)
    ));
}

/// A peer flooding forged fragments that never complete fills reassembly to
/// its budget and is refused there, however long it keeps going.
#[test]
fn a_fragment_flood_is_held_to_the_reassembly_budget() {
    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    let count = u8::try_from(MAX_FRAGMENTS_PER_MESSAGE).unwrap();
    let full = vec![0u8; MAX_FRAGMENT_BYTES];
    let mut sequence = 0u16;
    let mut refused = 0;
    for message_id in 0..u16::try_from(RELIABLE_WINDOW).unwrap() {
        for index in 0..count - 1 {
            let datagram = encode_packet(
                PROTOCOL,
                &PacketHeader {
                    sequence,
                    acks: None,
                },
                &PacketBody::Reliable(Fragment {
                    message_id,
                    index,
                    count,
                    payload: &full,
                }),
            );
            sequence = sequence.wrapping_add(1);
            match endpoint.receive_datagram(&datagram) {
                Ok(()) => {}
                Err(ReceiveError::ReassemblyFull { .. }) => refused += 1,
                Err(e) => panic!("unexpected refusal: {e}"),
            }
            assert!(endpoint.buffered_bytes() <= MAX_RELIABLE_BYTES_IN_FLIGHT);
        }
    }
    assert!(refused > 0, "the flood must have reached the budget");
}

/// **Backpressure end to end.** A receiver whose application stops reading
/// fills its delivery queue to the cap and then refuses; the sender feels it
/// as its own backpressure; once the receiver reads again, everything
/// arrives, in order.
#[test]
fn a_receiver_that_stops_reading_pushes_back_on_the_sender() {
    let mut link = Link::new(SimConditions {
        latency: Duration::from_millis(5),
        ..SimConditions::default()
    });
    link.draining = false;
    let mut sent = Vec::new();
    let mut pushed_back = false;
    for n in 0..MAX_DELIVERED_MESSAGES * 2 {
        let message = (n as u32).to_le_bytes().to_vec();
        match link.a.send(Channel::Reliable, message.clone()) {
            Ok(()) => sent.push(message),
            Err(TransportError::Backpressure) => {
                pushed_back = true;
                break;
            }
            Err(e) => panic!("unexpected refusal: {e}"),
        }
        link.step();
    }
    // Let it settle with the receiver still not reading.
    link.run(Duration::from_secs(2));
    assert!(pushed_back, "the sender must have felt the full receiver");
    assert!(link.b.buffered_bytes() <= MAX_DELIVERED_BYTES + MAX_RELIABLE_BYTES_IN_FLIGHT);
    assert!(link.b.delivered_len() >= MAX_DELIVERED_MESSAGES);
    assert!(link.b.delivered_len() <= MAX_DELIVERED_MESSAGES + RELIABLE_WINDOW);

    link.draining = true;
    assert!(
        link.run_until(Duration::from_secs(60), |link| link.b_got.len()
            == sent.len())
    );
    assert_eq!(payloads(&link.b_got, Channel::Reliable), sent);
}
