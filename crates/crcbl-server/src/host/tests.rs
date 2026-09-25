//! The host over `InMemoryTransport`, with real `crcbl_client::Client`s on
//! the far ends wherever a client's behaviour is what is being tested, and a
//! bare transport where a test must write the handshake itself.

use std::sync::{Arc, Mutex};

use crcbl_client::{Client, Ended};
use crcbl_ecs::System;
use crcbl_net::{InMemoryTransport, Message};

use super::*;

const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0000_484f_5354,
    schema_hash: 0x0000_5045_4552,
};

const TICK_HZ: u32 = 60;
const TICK: Duration = Duration::from_nanos(16_666_667);

/// A world with one system holding one entity, so every snapshot carries
/// something.
fn world() -> World {
    let mut world = World::new();
    let entity = world.spawn();
    let mut system = System::<f32>::new("position");
    system.attach(entity, 0.0);
    world.register_system(Box::new(system));
    world
}

fn client(transport: InMemoryTransport) -> Client<InMemoryTransport> {
    Client::new_with_compatibility(World::new(), transport, TICK_HZ, COMPATIBILITY)
}

/// A host and its clients, stepped together one tick at a time.
struct Rig {
    host: Host,
    clients: Vec<Client<InMemoryTransport>>,
    /// `ids[i]` is `clients[i]`'s peer.
    ids: Vec<PeerId>,
    now: Duration,
}

impl Rig {
    fn new(max_peers: usize) -> Self {
        let mut host = Host::new(
            world(),
            HostConfig {
                max_peers,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        host.update(Duration::ZERO);
        Self {
            host,
            clients: Vec::new(),
            ids: Vec::new(),
            now: Duration::ZERO,
        }
    }

    /// One tick: the host, then every client.
    fn step(&mut self) {
        self.now += TICK;
        self.host.update(self.now);
        for client in &mut self.clients {
            client.update(self.now);
        }
    }

    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Steps until the host raises an event, and returns every event it
    /// raised on that tick.
    fn until_event(&mut self) -> Vec<PeerEvent> {
        for _ in 0..600 {
            self.step();
            let events: Vec<_> = self.host.events().collect();
            if !events.is_empty() {
                return events;
            }
        }
        panic!("no host event within 600 ticks");
    }

    /// Connects one more client, one at a time so `ids` lines up with
    /// `clients`.
    fn join(&mut self) {
        let (near, far) = InMemoryTransport::pair();
        self.host.add(Box::new(far));
        self.clients.push(client(near));
        match self.until_event()[..] {
            [PeerEvent::Joined(id)] => self.ids.push(id),
            ref other => panic!("expected one join, got {other:?}"),
        }
    }

    fn with_peers(max_peers: usize, peers: usize) -> Self {
        let mut rig = Self::new(max_peers);
        for _ in 0..peers {
            rig.join();
        }
        rig.run(3);
        rig
    }

    fn applied(&self) -> Vec<TickId> {
        self.clients.iter().map(Client::last_applied_tick).collect()
    }
}

/// A bare far end: the host gets one side, the test speaks for the other.
fn raw(host: &mut Host) -> InMemoryTransport {
    let (near, far) = InMemoryTransport::pair();
    host.add(Box::new(far));
    near
}

fn say_hello(transport: &mut InMemoryTransport, generation: u64, token: Option<ResumeToken>) {
    transport
        .send_reliable(Message::reliable(crcbl_net::encode_hello(&Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash,
            generation,
            session_token: token,
        })))
        .unwrap();
}

/// The handshake reply waiting on `transport`, skipping anything else.
fn reply(transport: &mut InMemoryTransport) -> HandshakeResult {
    while let Some(msg) = transport.recv().unwrap() {
        if let Ok(result) = crcbl_net::decode_handshake_result(&msg.payload) {
            return result;
        }
    }
    panic!("no handshake reply");
}

fn reject_code(result: &HandshakeResult) -> Option<u8> {
    match result {
        HandshakeResult::Reject { reason, .. } => Some(reason.code),
        HandshakeResult::Accept { .. } => None,
    }
}

#[test]
fn every_peer_gets_its_own_session_and_its_own_snapshots() {
    for peers in [2, 4] {
        let mut rig = Rig::with_peers(peers, peers);
        let before = rig.applied();
        rig.run(5);

        let mut sessions: Vec<_> = rig.clients.iter().map(Client::session_id).collect();
        assert!(sessions.iter().all(Option::is_some), "N = {peers}");
        sessions.sort_unstable_by_key(|id| id.map(|id| id.0));
        sessions.dedup();
        assert_eq!(sessions.len(), peers, "one session each, N = {peers}");
        for (after, before) in rig.applied().iter().zip(&before) {
            assert!(after > before, "every client keeps applying, N = {peers}");
        }
        assert_eq!(rig.host.peer_count(), peers);
        assert_eq!(rig.host.processing_error_count(), 0);
        assert_eq!(rig.host.auth_failure_count(), 0);
    }
}

#[test]
fn one_peer_more_than_the_limit_is_refused_as_full_until_a_place_frees() {
    for peers in [2, 4] {
        let mut rig = Rig::with_peers(peers, peers);
        let mut extra = raw(&mut rig.host);
        say_hello(&mut extra, 1, None);
        rig.step();
        let refused = reply(&mut extra);
        assert_eq!(
            reject_code(&refused),
            Some(RejectReason::SERVER_FULL),
            "N = {peers}: {refused:?}"
        );
        assert_eq!(rig.host.peer_count(), peers);
        assert_eq!(rig.host.pending_count(), 1, "kept, to try again");

        assert!(rig.host.kick(rig.ids[0]));
        say_hello(&mut extra, 2, None);
        rig.step();
        assert_eq!(reject_code(&reply(&mut extra)), None, "N = {peers}");
        assert_eq!(rig.host.peer_count(), peers);
    }
}

#[test]
fn an_incompatible_client_is_told_so_even_when_the_host_is_full() {
    let mut rig = Rig::with_peers(1, 1);
    let mut stranger = raw(&mut rig.host);
    stranger
        .send_reliable(Message::reliable(crcbl_net::encode_hello(&Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash ^ 1,
            generation: 1,
            session_token: None,
        })))
        .unwrap();
    rig.step();
    assert_eq!(
        reject_code(&reply(&mut stranger)),
        Some(RejectReason::SCHEMA_MISMATCH)
    );
}

#[test]
fn a_dropped_link_puts_only_that_session_in_reconnecting() {
    let mut rig = Rig::with_peers(3, 3);
    // The client's end goes; the other two carry on.
    let (spare, _unheard) = InMemoryTransport::pair();
    rig.clients[1].reconnect(spare);
    assert_eq!(rig.until_event(), [PeerEvent::Lost(rig.ids[1])]);

    let before = rig.applied();
    rig.run(5);
    let after = rig.applied();
    assert_eq!(
        rig.host.peer_state(rig.ids[1]),
        Some(SessionState::Reconnecting)
    );
    for i in [0, 2] {
        assert_eq!(
            rig.host.peer_state(rig.ids[i]),
            Some(SessionState::Connected)
        );
        assert!(after[i] > before[i], "peer {i} still ticking");
    }
    assert_eq!(rig.host.peer_count(), 3);
}

#[test]
fn a_lost_peer_resumes_its_own_session_within_the_grace_period() {
    let mut rig = Rig::with_peers(2, 2);
    let session = rig.clients[1].session_id();
    let (near, far) = InMemoryTransport::pair();
    rig.clients[1].reconnect(near);
    assert_eq!(rig.until_event(), [PeerEvent::Lost(rig.ids[1])]);

    // Its place is held: a newcomer is refused, not admitted into it.
    let mut stranger = raw(&mut rig.host);
    say_hello(&mut stranger, 1, None);
    rig.step();
    assert_eq!(
        reject_code(&reply(&mut stranger)),
        Some(RejectReason::SERVER_FULL)
    );

    rig.host.add(Box::new(far));
    assert_eq!(rig.until_event(), [PeerEvent::Resumed(rig.ids[1])]);
    assert_eq!(rig.clients[1].session_id(), session);
    assert_eq!(rig.host.peer_count(), 2);
    let before = rig.applied();
    rig.run(5);
    assert!(rig.applied()[1] > before[1], "snapshots flow again");
}

#[test]
fn a_peer_back_after_its_grace_period_gets_a_fresh_session() {
    let mut rig = Rig::with_peers(2, 2);
    rig.host.set_session_config(SessionConfig {
        reconnect_grace_period: Duration::from_millis(100),
        ..SessionConfig::default()
    });
    let session = rig.clients[1].session_id();
    let (near, far) = InMemoryTransport::pair();
    rig.clients[1].reconnect(near);
    assert_eq!(rig.until_event(), [PeerEvent::Lost(rig.ids[1])]);
    assert_eq!(rig.until_event(), [PeerEvent::Left(rig.ids[1])]);
    assert_eq!(rig.host.peer_state(rig.ids[1]), None);

    // The client offers its old token, is refused, and joins afresh.
    rig.host.add(Box::new(far));
    let joined = match rig.until_event()[..] {
        [PeerEvent::Joined(id)] => id,
        ref other => panic!("expected a join, got {other:?}"),
    };
    assert_ne!(joined, rig.ids[1]);
    rig.run(3);
    assert!(rig.clients[1].session_id().is_some());
    assert_ne!(rig.clients[1].session_id(), session);
}

#[test]
fn a_connected_peers_token_on_another_link_is_refused() {
    let mut rig = Rig::new(4);
    let mut owner = raw(&mut rig.host);
    say_hello(&mut owner, 1, None);
    rig.step();
    let HandshakeResult::Accept { resume_token, .. } = reply(&mut owner) else {
        panic!("the owner is admitted");
    };

    let mut thief = raw(&mut rig.host);
    say_hello(&mut thief, 1, Some(resume_token));
    rig.step();
    let refused = reply(&mut thief);
    assert_eq!(
        reject_code(&refused),
        Some(RejectReason::INVALID_SESSION_TOKEN)
    );
    // Refused as a live session's, not as an expired one's: the reason a
    // client (and its player) is given has to be the true one.
    assert!(
        matches!(&refused, HandshakeResult::Reject { reason, .. } if reason.msg.contains("another link")),
        "{refused:?}"
    );
    assert_eq!(rig.host.peer_count(), 1);
    let owner_id = rig.host.peers().next().expect("one peer");
    assert_eq!(rig.host.peer_state(owner_id), Some(SessionState::Connected));
}

#[test]
fn a_hello_on_a_peers_own_link_is_answered_by_its_token() {
    let mut rig = Rig::new(4);
    let mut owner = raw(&mut rig.host);
    say_hello(&mut owner, 1, None);
    rig.step();
    let HandshakeResult::Accept {
        resume_token,
        session_id,
        ..
    } = reply(&mut owner)
    else {
        panic!("the owner is admitted");
    };

    say_hello(&mut owner, 2, Some(resume_token));
    rig.step();
    let again = reply(&mut owner);
    assert!(
        matches!(again, HandshakeResult::Accept { session_id: same, .. } if same == session_id),
        "{again:?}"
    );

    say_hello(&mut owner, 3, Some(ResumeToken::from_bytes([7; 32])));
    rig.step();
    assert_eq!(
        reject_code(&reply(&mut owner)),
        Some(RejectReason::INVALID_SESSION_TOKEN)
    );
    assert_eq!(rig.host.peer_count(), 1);
}

#[test]
fn shutdown_tells_every_peer_the_host_left_before_their_links_close() {
    for peers in [2, 4] {
        let mut rig = Rig::with_peers(peers, peers);
        rig.host.shutdown(SessionEndReason::HOST_LEFT);
        rig.step();
        for (i, client) in rig.clients.iter().enumerate() {
            assert_eq!(
                client.ended(),
                Some(Ended::ByServer(SessionEndReason::HOST_LEFT)),
                "peer {i} of {peers}"
            );
            assert!(!client.is_connected(), "and then the link closed");
        }
        assert_eq!(rig.host.peer_count(), 0);
    }
}

#[test]
fn a_host_that_vanishes_without_a_word_reads_as_lost() {
    let Rig {
        host,
        mut clients,
        now,
        ..
    } = Rig::with_peers(2, 2);
    drop(host);
    for client in &mut clients {
        client.update(now + TICK);
        assert_eq!(client.ended(), Some(Ended::Lost));
    }
}

#[test]
fn a_kick_ends_one_session_and_tells_that_peer_why() {
    let mut rig = Rig::with_peers(3, 3);
    assert!(rig.host.kick(rig.ids[0]));
    assert!(!rig.host.kick(rig.ids[0]), "already gone");
    rig.step();
    assert_eq!(
        rig.clients[0].ended(),
        Some(Ended::ByServer(SessionEndReason::KICKED))
    );
    let before = rig.applied();
    rig.run(5);
    let after = rig.applied();
    for i in [1, 2] {
        assert_eq!(rig.clients[i].ended(), None);
        assert!(after[i] > before[i], "peer {i} still ticking");
    }
    assert_eq!(rig.host.peer_count(), 2);
    assert!(
        rig.host.events().next().is_none(),
        "the game kicked; no event"
    );
}

#[test]
fn each_peers_input_reaches_the_module_under_its_own_id() {
    /// Every (peer, frame) the module was handed, across ticks.
    type Seen = Arc<Mutex<Vec<(PeerId, Vec<u8>)>>>;
    struct Record(Seen);
    impl HostModule for Record {
        fn tick(&mut self, _world: &mut World, inputs: PeerInputs<'_>) {
            let mut seen = self.0.lock().expect("not poisoned");
            for (peer, frames) in inputs.iter() {
                for (_, data) in frames.iter() {
                    seen.push((peer, data.to_vec()));
                }
            }
        }
    }

    let mut rig = Rig::with_peers(2, 2);
    let seen = Arc::new(Mutex::new(Vec::new()));
    rig.host.set_module(Box::new(Record(Arc::clone(&seen))));
    rig.clients[0].set_input(vec![0xA0]);
    rig.clients[1].set_input(vec![0xB1]);
    rig.run(5);

    let seen = seen.lock().expect("not poisoned");
    assert!(seen.contains(&(rig.ids[0], vec![0xA0])), "{seen:?}");
    assert!(seen.contains(&(rig.ids[1], vec![0xB1])), "{seen:?}");
    assert!(
        seen.iter()
            .all(|(peer, data)| (*peer == rig.ids[0]) == (data == &[0xA0])),
        "no input under the other peer's id: {seen:?}"
    );
}

#[test]
fn a_pending_link_that_never_says_hello_is_closed() {
    let mut rig = Rig::new(2);
    let mut silent = raw(&mut rig.host);
    let ticks = PENDING_SILENCE_LIMIT.as_nanos() / TICK.as_nanos();
    rig.run(usize::try_from(ticks).expect("fits") - 2);
    assert_eq!(rig.host.pending_count(), 1, "not yet");
    rig.run(4);
    assert_eq!(rig.host.pending_count(), 0);
    assert!(matches!(silent.recv(), Err(TransportError::Disconnected)));
}

#[test]
fn a_pending_link_is_held_to_the_inbound_budget() {
    let mut rig = Rig::new(2);
    rig.host
        .set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 4,
            bytes_per_second: 1 << 20,
        });
    let mut flood = raw(&mut rig.host);
    for _ in 0..16 {
        flood
            .send_unreliable(Message::unreliable(vec![0xFF]))
            .unwrap();
    }
    rig.step();
    assert_eq!(rig.host.processing_error_count(), 4, "four read");
    assert!(rig.host.rate_limited_message_count() > 0);
}

#[test]
fn a_token_less_hello_on_a_peers_own_link_gets_its_session_again() {
    let mut rig = Rig::new(4);
    let mut owner = raw(&mut rig.host);
    say_hello(&mut owner, 1, None);
    rig.step();
    let HandshakeResult::Accept {
        resume_token,
        session_id,
        ..
    } = reply(&mut owner)
    else {
        panic!("the owner is admitted");
    };

    say_hello(&mut owner, 2, None);
    rig.step();
    let again = reply(&mut owner);
    assert!(
        matches!(
            again,
            HandshakeResult::Accept { generation: 2, session_id: same, resume_token: token, .. }
                if same == session_id && token == resume_token
        ),
        "{again:?}"
    );
    assert_eq!(rig.host.peer_count(), 1, "the same place, not a second one");
}

/// Messages between a real client and the host, carried by hand so one can
/// be lost on the way.
struct Relay {
    client_side: InMemoryTransport,
    host_side: InMemoryTransport,
    /// How many of the host's handshake replies to lose.
    lose_replies: usize,
}

impl Relay {
    fn carry(&mut self) {
        while let Some(msg) = self.client_side.recv().unwrap() {
            forward(&mut self.host_side, msg);
        }
        while let Some(msg) = self.host_side.recv().unwrap() {
            if self.lose_replies > 0 && crcbl_net::decode_handshake_result(&msg.payload).is_ok() {
                self.lose_replies -= 1;
                continue;
            }
            forward(&mut self.client_side, msg);
        }
    }
}

fn forward(to: &mut InMemoryTransport, msg: Message) {
    match msg.kind {
        crcbl_net::MessageKind::Reliable => to.send_reliable(msg).unwrap(),
        crcbl_net::MessageKind::Unreliable => to.send_unreliable(msg).unwrap(),
    }
}

#[test]
fn a_client_whose_first_accept_was_lost_takes_the_session_up_on_its_retry() {
    let mut host = Host::new(
        world(),
        HostConfig {
            max_peers: 1,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    let (near, client_side) = InMemoryTransport::pair();
    let (host_side, far) = InMemoryTransport::pair();
    host.add(Box::new(far));
    let mut client = client(near);
    let mut relay = Relay {
        client_side,
        host_side,
        lose_replies: 1,
    };
    let mut now = Duration::ZERO;
    let mut step = |host: &mut Host, client: &mut Client<InMemoryTransport>, relay: &mut Relay| {
        now += TICK;
        client.update(now);
        relay.carry();
        host.update(now);
        relay.carry();
    };
    // Past the client's handshake timeout, its token-less retry, and the
    // host's authentication deadline.
    let ticks = (AUTHENTICATION_DEADLINE + Duration::from_secs(5)).as_nanos() / TICK.as_nanos();
    for _ in 0..ticks {
        step(&mut host, &mut client, &mut relay);
    }
    assert_eq!(relay.lose_replies, 0, "the first Accept was lost");
    assert!(
        client.session_id().is_some(),
        "the retry's Accept was taken"
    );
    assert_eq!(host.peer_count(), 1);
    let id = host.peers().next().expect("one peer");
    assert_eq!(host.peer_state(id), Some(SessionState::Connected));
    assert!(
        client.last_applied_tick() > TickId::ZERO,
        "snapshots open under the session's key"
    );
    let left: Vec<_> = host
        .events()
        .filter(|event| matches!(event, PeerEvent::Left(_)))
        .collect();
    assert_eq!(left, [], "the session was taken up, so it stays");
}

#[test]
fn a_session_its_client_never_takes_up_ends_at_the_deadline() {
    let mut rig = Rig::new(1);
    let mut owner = raw(&mut rig.host);
    say_hello(&mut owner, 1, None);
    rig.step();
    assert!(matches!(reply(&mut owner), HandshakeResult::Accept { .. }));
    let id = rig.host.peers().next().expect("admitted");
    let _ = rig.host.events().count();

    // Nothing sealed ever comes back.
    let ticks = AUTHENTICATION_DEADLINE.as_nanos() / TICK.as_nanos();
    rig.run(usize::try_from(ticks).expect("fits") - 2);
    assert_eq!(rig.host.peer_count(), 1, "not yet");
    rig.run(4);
    assert_eq!(rig.host.peer_count(), 0, "the place is free again");
    assert_eq!(rig.host.events().collect::<Vec<_>>(), [PeerEvent::Left(id)]);
}

#[test]
fn clients_that_answer_under_their_key_are_never_ended_by_the_deadline() {
    let mut rig = Rig::with_peers(2, 2);
    let _ = rig.host.events().count();
    let ticks = (AUTHENTICATION_DEADLINE * 2).as_nanos() / TICK.as_nanos();
    rig.run(usize::try_from(ticks).expect("fits"));
    assert_eq!(rig.host.peer_count(), 2);
    assert_eq!(rig.host.events().collect::<Vec<_>>(), []);
}

#[test]
fn a_session_accepted_again_opens_the_clients_restarted_key() {
    let mut rig = Rig::new(1);
    let mut owner = raw(&mut rig.host);
    say_hello(&mut owner, 1, None);
    rig.step();
    let HandshakeResult::Accept { resume_token, .. } = reply(&mut owner) else {
        panic!("admitted");
    };
    let seal_ack = |crypto: &mut crcbl_net::SessionCrypto, owner: &mut InMemoryTransport| {
        let sealed = crypto
            .seal(&crcbl_net::encode_ack(SectorId::ZERO, TickId::ZERO))
            .unwrap();
        owner.send_unreliable(Message::unreliable(sealed)).unwrap();
    };
    let mut first = crcbl_net::SessionCrypto::from_token(&resume_token);
    for _ in 0..3 {
        seal_ack(&mut first, &mut owner);
    }
    rig.step();
    assert_eq!(rig.host.auth_failure_count(), 0);

    // The client says hello again and, on the Accept, starts its key over.
    say_hello(&mut owner, 2, None);
    rig.step();
    assert!(matches!(reply(&mut owner), HandshakeResult::Accept { .. }));
    let mut restarted = crcbl_net::SessionCrypto::from_token(&resume_token);
    seal_ack(&mut restarted, &mut owner);
    rig.step();
    assert_eq!(
        rig.host.auth_failure_count(),
        0,
        "the restarted counter read as a replay"
    );
}
