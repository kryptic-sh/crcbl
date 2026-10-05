//! The host over `InMemoryTransport`, with real `crcbl_client::Client`s on
//! the far ends wherever a client's behaviour is what is being tested, and a
//! bare transport where a test must write the handshake itself.

use std::sync::{Arc, Mutex};

use crcbl_client::{Client, Ended};
use crcbl_ecs::System;
use crcbl_net::{InMemoryTransport, Message};

use super::*;

pub(super) const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0000_484f_5354,
    schema_hash: 0x0000_5045_4552,
};

/// Who every hello [`say_hello`] writes says it is: one player, as one
/// client's hellos are, and none of the ids [`client`] draws.
pub(super) const HELLO_PLAYER: PlayerId = PlayerId::from_bytes([0x5E; PlayerId::BYTES]);

pub(super) const TICK_HZ: u32 = 60;
pub(super) const TICK: Duration = Duration::from_nanos(16_666_667);

/// A world with one system holding one entity, so every snapshot carries
/// something.
pub(super) fn world() -> World {
    let mut world = World::new();
    let entity = world.spawn();
    let mut system = System::<f32>::new("position");
    system.attach(entity, 0.0);
    world.register_system(Box::new(system));
    world
}

/// A player id no other call in this test binary has drawn: every client a
/// host admits at once must be its own player, or the host refuses the second
/// as a duplicate.
pub(super) fn next_player() -> PlayerId {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    PlayerId::from_seed(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

fn client(transport: InMemoryTransport) -> Client<InMemoryTransport> {
    Client::new_with_compatibility(
        World::new(),
        transport,
        TICK_HZ,
        COMPATIBILITY,
        next_player(),
    )
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

pub(super) fn say_hello(
    transport: &mut InMemoryTransport,
    generation: u64,
    token: Option<ResumeToken>,
) {
    say_hello_as(transport, generation, token, HELLO_PLAYER);
}

/// [`say_hello`], as `player`.
pub(super) fn say_hello_as(
    transport: &mut InMemoryTransport,
    generation: u64,
    token: Option<ResumeToken>,
    player: PlayerId,
) {
    transport
        .send_reliable(Message::reliable(crcbl_net::encode_hello(&Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash,
            generation,
            player,
            session_token: token,
        })))
        .unwrap();
}

/// The handshake reply waiting on `transport`, skipping anything else.
pub(super) fn reply(transport: &mut InMemoryTransport) -> HandshakeResult {
    while let Some(msg) = transport.recv().unwrap() {
        if let Ok(result) = crcbl_net::decode_handshake_result(&msg.payload) {
            return result;
        }
    }
    panic!("no handshake reply");
}

pub(super) fn reject_code(result: &HandshakeResult) -> Option<u8> {
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
            player: HELLO_PLAYER,
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

/// **An event sent the moment a peer joins reaches that peer and no other** —
/// sent before its client has read the `Accept`, which is when a game sends
/// a newcomer what it needs first: the reliable channel keeps it behind the
/// `Accept`, so the client holds the key by the time it opens it.
#[test]
fn an_event_sent_on_join_reaches_that_peer_and_no_other() {
    let mut rig = Rig::with_peers(3, 1);
    let (near, far) = InMemoryTransport::pair();
    rig.host.add(Box::new(far));
    let mut newcomer = client(near);
    newcomer.update(rig.now);
    rig.now += TICK;
    rig.host.update(rig.now);
    let [PeerEvent::Joined(id)] = rig.host.events().collect::<Vec<_>>()[..] else {
        panic!("the newcomer did not join in one tick");
    };
    rig.host
        .send_event(id, b"what a newcomer needs".to_vec())
        .expect("a connected peer takes an event");

    newcomer.update(rig.now);
    assert!(newcomer.session_id().is_some(), "the accept came first");
    assert_eq!(
        newcomer.events().collect::<Vec<_>>(),
        vec![b"what a newcomer needs".to_vec()]
    );
    rig.step();
    assert_eq!(rig.clients[0].events().count(), 0, "only the peer named");
    assert_eq!(newcomer.processing_error_count(), 0);
    assert_eq!(newcomer.auth_failure_count(), 0);
}

/// **An event that cannot go says why**: to a peer that is gone, to one whose
/// link is down, and one longer than a client reads.
#[test]
fn an_event_that_cannot_be_sent_is_refused_by_name() {
    let mut rig = Rig::with_peers(3, 2);
    let limit = crcbl_net::codec::MAX_FIELD_BYTES;
    assert!(matches!(
        rig.host.send_event(rig.ids[0], vec![0; limit + 1]),
        Err(EventNotSent::TooLarge { size, limit: named }) if size == limit + 1 && named == limit
    ));
    assert!(rig.host.send_event(rig.ids[0], vec![0; limit]).is_ok());

    assert!(rig.host.kick(rig.ids[0]));
    assert!(matches!(
        rig.host.send_event(rig.ids[0], vec![1]),
        Err(EventNotSent::NoSuchPeer(peer)) if peer == rig.ids[0]
    ));

    drop(rig.clients.remove(1));
    rig.step();
    assert_eq!(
        rig.host.peer_state(rig.ids[1]),
        Some(SessionState::Reconnecting)
    );
    assert!(matches!(
        rig.host.send_event(rig.ids[1], vec![1]),
        Err(EventNotSent::NotConnected(peer)) if peer == rig.ids[1]
    ));
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

/// **A client whose first `Accept` was lost is raised as re-accepted, and
/// what the game sends it then reaches it** — where what it was sent on
/// `Joined`, sealed before the key started over, never opens on its side.
#[test]
fn a_reaccepted_peer_is_raised_and_an_event_sent_then_reaches_it() {
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
    let mut raised = Vec::new();
    let mut now = Duration::ZERO;
    // Past the client's handshake timeout and its retry, with room.
    let ticks = Duration::from_secs(5).as_nanos() / TICK.as_nanos();
    for _ in 0..ticks {
        now += TICK;
        client.update(now);
        relay.carry();
        host.update(now);
        for event in host.events().collect::<Vec<_>>() {
            let (id, sent): (PeerId, &[u8]) = match event {
                PeerEvent::Joined(id) => (id, b"sent on join"),
                PeerEvent::Reaccepted(id) => (id, b"sent again"),
                other => panic!("unexpected {other:?}"),
            };
            host.send_event(id, sent.to_vec())
                .expect("a connected peer takes an event");
            raised.push(event);
        }
        relay.carry();
    }
    assert_eq!(relay.lose_replies, 0, "the first Accept was lost");
    let id = host.peers().next().expect("one peer");
    assert_eq!(raised, [PeerEvent::Joined(id), PeerEvent::Reaccepted(id)]);
    assert_eq!(
        client.events().collect::<Vec<_>>(),
        vec![b"sent again".to_vec()],
        "what the game sent on the re-accept did not reach the client"
    );
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

// ── Scene edits ────────────────────────────────────────────────────────────

/// A host never told to serve edits answers every one as not editable, by
/// the request's id, and holds nothing for the caller.
#[test]
fn a_host_serving_no_scene_refuses_an_edit_as_not_editable() {
    let mut rig = Rig::with_peers(2, 1);
    let id = rig.clients[0].send_edit(vec![1, 1]).expect("in session");
    rig.run(3);
    assert!(rig.host.take_edit_requests().is_empty());
    let replies: Vec<_> = rig.clients[0].edit_replies().collect();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].request_id, id);
    assert!(matches!(
        replies[0].outcome,
        crcbl_net::EditOutcome::Refused {
            reason: crcbl_net::EditRefusal::NOT_EDITABLE,
            ..
        }
    ));
}

/// A host serving edits hands each one over with the peer that sent it, in
/// admission order; a reply reaches only the peer it is sent to, and a notice
/// every connected peer, its author included.
#[test]
fn a_host_serving_edits_hands_them_over_and_sends_replies_and_notices() {
    let mut rig = Rig::with_peers(3, 2);
    rig.host.serve_edits();
    let drag = crcbl_net::EditGesture { id: 5, last: false };
    let second = rig.clients[1]
        .send_edit_in(vec![1, 2], drag)
        .expect("in session");
    let first = rig.clients[0].send_edit(vec![1, 1]).expect("in session");
    rig.run(2);
    let taken = rig.host.take_edit_requests();
    assert_eq!(
        taken,
        [
            (
                rig.ids[0],
                crcbl_net::EditRequest {
                    request_id: first,
                    gesture: None,
                    op: vec![1, 1],
                }
            ),
            (
                rig.ids[1],
                crcbl_net::EditRequest {
                    request_id: second,
                    gesture: Some(drag),
                    op: vec![1, 2],
                }
            ),
        ]
    );
    assert!(rig.host.take_edit_requests().is_empty(), "taken once");

    let reply = crcbl_net::EditReply {
        request_id: first,
        outcome: crcbl_net::EditOutcome::Applied { revision: 1 },
    };
    rig.host
        .send_edit_reply(rig.ids[0], &reply)
        .expect("a connected peer");
    let notice = crcbl_net::EditNotice {
        revision: 1,
        author: rig.ids[0].get(),
        gesture: Some(3),
        op: vec![1, 1],
    };
    assert_eq!(rig.host.broadcast_edit_notice(&notice), Ok(2));
    rig.run(1);
    assert_eq!(rig.clients[0].edit_replies().collect::<Vec<_>>(), [reply]);
    assert_eq!(rig.clients[1].edit_replies().count(), 0, "not its reply");
    for client in &mut rig.clients {
        assert_eq!(
            client.edit_notices().collect::<Vec<_>>(),
            std::slice::from_ref(&notice)
        );
    }
    assert!(matches!(
        rig.host.send_edit_reply(
            PeerId::from_raw(999),
            &crcbl_net::EditReply {
                request_id: 1,
                outcome: crcbl_net::EditOutcome::Applied { revision: 1 },
            }
        ),
        Err(EventNotSent::NoSuchPeer(_))
    ));
}
