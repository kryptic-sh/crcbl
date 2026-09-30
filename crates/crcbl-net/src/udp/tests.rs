//! `UdpTransport` and `UdpListener` over real loopback sockets.
//!
//! Every socket binds `127.0.0.1:0`, so no test depends on a free fixed port,
//! and a sandbox without loopback UDP fails every test here loudly at the
//! bind rather than skipping. Tests that time something out run on a
//! [`ManualClock`] shared by both ends, so a timeout is a clock advance and
//! not a wait; the waits that remain are for loopback delivery and end as
//! soon as what they wait for arrives.
//!
//! Where a test must see or bend the traffic — capture it, replay it, tamper
//! with it, reorder it, forge a reply — the client connects through a
//! [`Proxy`], a plain socket the test drives by hand.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use super::hello::Hello;
use super::*;
use crate::conformance::{self, Link};
use crate::reliable::{MAX_RELIABLE_MESSAGE_BYTES, MAX_UNRELIABLE_PAYLOAD, PEER_TIMEOUT};
use crate::seal::{KeyPair, SEALED_TAG};
use crate::{Clock, ManualClock, Message, MessageKind, SystemClock, Transport, TransportError};

const PROTOCOL: u32 = 0x4352_4342;

/// Every socket here binds this: loopback, any free port.
const LOOPBACK: &str = "127.0.0.1:0";

/// The longest a test waits for loopback traffic. Reached only when what it
/// waits for never comes — which is the failure.
const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// The pause between polls while waiting.
const POLL_PAUSE: Duration = Duration::from_millis(1);

/// What the conformance suite's `settle` waits. `settle` sees neither end of
/// the pair, so it cannot wait for a condition; it gives loopback, which
/// delivers in microseconds, a wide margin instead.
const SETTLE_TIME: Duration = Duration::from_millis(50);

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

fn listener<C: Clock + Clone>(config: ListenerConfig, clock: C) -> UdpListener<C> {
    UdpListener::bind_with(LOOPBACK, config, clock)
        .expect("loopback UDP must be available to these tests")
}

/// Drives `end` once, where nothing is expected to arrive.
fn pump<T: Transport>(end: &mut T) {
    match end.recv() {
        Ok(None) => {}
        other => panic!("nothing was sent to this end, yet recv gave {other:?}"),
    }
}

/// A client connected to `server` and the peer `listener` accepted for it.
/// `between` runs on every poll, for a proxy that has to forward.
fn connect_through<C: Clock + Clone>(
    listener: &mut UdpListener<C>,
    server: SocketAddr,
    clock: C,
    mut between: impl FnMut(),
) -> (UdpTransport<C>, UdpTransport<C>) {
    let mut client = UdpTransport::connect_with(server, PROTOCOL, clock).expect("connect");
    let mut accepted = None;
    wait_until("the handshake", || {
        between();
        pump(&mut client);
        accepted = accepted.take().or_else(|| listener.accept());
        accepted.is_some() && client.state() == UdpState::Connected
    });
    (client, accepted.expect("accepted"))
}

fn connect<C: Clock + Clone>(
    listener: &mut UdpListener<C>,
    clock: C,
) -> (UdpTransport<C>, UdpTransport<C>) {
    let server = listener.local_addr().expect("listener address");
    connect_through(listener, server, clock, || {})
}

/// Receives on `to` until `count` messages have come, driving `from` too so
/// its acks and resends keep flowing.
fn receive<C: Clock + Clone>(
    to: &mut UdpTransport<C>,
    from: &mut UdpTransport<C>,
    count: usize,
) -> Vec<Message> {
    let mut got = Vec::new();
    wait_until("the messages", || {
        while let Some(message) = to.recv().expect("the link stays up") {
            got.push(message);
        }
        pump(from);
        got.len() >= count
    });
    got
}

fn payloads(messages: &[Message]) -> Vec<Vec<u8>> {
    messages.iter().map(|m| m.payload.clone()).collect()
}

/// Whether `end` has reported the end of its link.
fn has_ended<T: Transport>(end: &mut T) -> bool {
    loop {
        match end.recv() {
            Ok(Some(_)) => {}
            Ok(None) => return false,
            Err(TransportError::Disconnected) => return true,
            Err(error) => panic!("recv failed: {error}"),
        }
    }
}

/// A man in the middle the test drives by hand: whatever reaches it from the
/// listener is downstream, anything else upstream, and nothing moves on
/// unless the test says so.
struct Proxy {
    socket: UdpSocket,
    server: SocketAddr,
    client: Option<SocketAddr>,
}

/// Which way a datagram through the proxy was going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Up,
    Down,
}

/// Datagrams seen at the proxy, in arrival order, with the way each went.
type Seen = Vec<(Way, Vec<u8>)>;

impl Proxy {
    fn new(server: SocketAddr) -> Self {
        Self {
            socket: bind_loopback(),
            server,
            client: None,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.socket.local_addr().expect("proxy address")
    }

    /// Everything that reached the proxy, held.
    fn arrived(&mut self) -> Seen {
        let mut buffer = [0u8; 2048];
        let mut got = Vec::new();
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, from)) if from == self.server => {
                    got.push((Way::Down, buffer[..len].to_vec()))
                }
                Ok((len, from)) => {
                    self.client = Some(from);
                    got.push((Way::Up, buffer[..len].to_vec()));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return got,
                Err(error) => panic!("proxy read: {error}"),
            }
        }
    }

    fn deliver(&self, way: Way, datagram: &[u8]) {
        let to = match way {
            Way::Up => self.server,
            Way::Down => self.client.expect("the client has sent something"),
        };
        self.socket.send_to(datagram, to).expect("proxy send");
    }

    /// Everything that arrived, passed on, and returned for inspection.
    fn forward(&mut self) -> Seen {
        let got = self.arrived();
        for (way, datagram) in &got {
            self.deliver(*way, datagram);
        }
        got
    }

    /// Collects held datagrams until `done` says it has what it needs.
    fn hold_until(&mut self, what: &str, mut done: impl FnMut(&[(Way, Vec<u8>)]) -> bool) -> Seen {
        let mut held = Vec::new();
        wait_until(what, || {
            held.extend(self.arrived());
            done(&held)
        });
        held
    }
}

/// A client connected through `proxy`, with every datagram of the
/// handshake forwarded and returned.
fn connect_via<C: Clock + Clone>(
    listener: &mut UdpListener<C>,
    proxy: &mut Proxy,
    clock: C,
) -> (UdpTransport<C>, UdpTransport<C>, Seen) {
    let mut seen = Vec::new();
    let (client, peer) = connect_through(listener, proxy.addr(), clock, || {
        seen.extend(proxy.forward());
    });
    (client, peer, seen)
}

// ── Conformance ──────────────────────────────────────────────────────────────

/// Pairs over loopback: a client, and the peer a listener accepted for it.
struct Loopback {
    listener: UdpListener,
}

impl Loopback {
    fn new() -> Self {
        Self {
            listener: listener(ListenerConfig::new(PROTOCOL), SystemClock::new()),
        }
    }
}

impl Link for Loopback {
    type Transport = UdpTransport;

    fn pair(&mut self) -> (UdpTransport, UdpTransport) {
        connect(&mut self.listener, SystemClock::new())
    }

    fn settle(&mut self) {
        thread::sleep(SETTLE_TIME);
    }

    fn max_message_bytes(&self) -> usize {
        MAX_RELIABLE_MESSAGE_BYTES
    }

    fn max_unreliable_message_bytes(&self) -> usize {
        MAX_UNRELIABLE_PAYLOAD
    }
}

// One test per check, so a failure says which promise broke.
#[test]
fn conformance_reliable_is_received_before_unreliable() {
    conformance::reliable_is_received_before_unreliable(&mut Loopback::new());
}

#[test]
fn conformance_recv_reliable_never_returns_unreliable() {
    conformance::recv_reliable_never_returns_unreliable(&mut Loopback::new());
}

#[test]
fn conformance_recv_reliable_returns_reliable_traffic_in_order() {
    conformance::recv_reliable_returns_reliable_traffic_in_order(&mut Loopback::new());
}

#[test]
fn conformance_the_send_method_sets_the_kind() {
    conformance::the_send_method_sets_the_kind(&mut Loopback::new());
}

#[test]
fn conformance_reliable_messages_arrive_in_order() {
    conformance::reliable_messages_arrive_in_order(&mut Loopback::new());
}

#[test]
fn conformance_an_oversized_message_names_its_size_and_the_limit() {
    conformance::an_oversized_message_names_its_size_and_the_limit(&mut Loopback::new());
}

#[test]
fn conformance_a_dropped_peer_is_disconnected() {
    conformance::a_dropped_peer_is_disconnected(&mut Loopback::new());
}

#[test]
fn conformance_reliable_messages_sent_before_a_drop_arrive_first() {
    conformance::reliable_messages_sent_before_a_drop_arrive_first(&mut Loopback::new());
}

// ── Delivery ─────────────────────────────────────────────────────────────────

/// A run of reliable messages, one of them many datagrams long, arrives
/// complete and in order, labelled reliable, both ways.
#[test]
fn reliable_messages_arrive_complete_and_in_order_over_loopback() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), SystemClock::new());
    let (mut client, mut peer) = connect(&mut listener, SystemClock::new());
    let large: Vec<u8> = (0..30_000_u32).map(|i| (i % 251) as u8).collect();
    let mut sent = Vec::new();
    for i in 0..100_u32 {
        let payload = if i == 50 {
            large.clone()
        } else {
            i.to_le_bytes().to_vec()
        };
        client
            .send_reliable(Message::reliable(payload.clone()))
            .expect("send");
        sent.push(payload);
    }
    let got = receive(&mut peer, &mut client, sent.len());
    assert!(got.iter().all(|m| m.kind == MessageKind::Reliable));
    assert_eq!(payloads(&got), sent);

    peer.send_reliable(Message::reliable(b"back".to_vec()))
        .expect("send back");
    assert_eq!(
        payloads(&receive(&mut client, &mut peer, 1)),
        [b"back".to_vec()]
    );
}

/// Two unreliable messages delivered newest first: the older is dropped on
/// arrival, because the channel is latest-wins.
#[test]
fn an_unreliable_message_older_than_one_delivered_is_dropped() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let (mut client, mut peer, _) = connect_via(&mut listener, &mut proxy, clock);

    client
        .send_unreliable(Message::unreliable(b"older".to_vec()))
        .expect("send");
    client
        .send_unreliable(Message::unreliable(b"newer".to_vec()))
        .expect("send");
    let held = proxy.hold_until("both datagrams", |held| {
        held.iter().filter(|(way, _)| *way == Way::Up).count() >= 2
    });
    for (way, datagram) in held.iter().rev() {
        proxy.deliver(*way, datagram);
    }
    let mut got = Vec::new();
    wait_until("the newer message and the drop", || {
        while let Some(message) = peer.recv().expect("up") {
            got.push(message);
        }
        peer.stats().endpoint.unreliable_dropped >= 1
    });
    assert_eq!(payloads(&got), [b"newer".to_vec()]);
    assert!(got.iter().all(|m| m.kind == MessageKind::Unreliable));
}

// ── Handshake ────────────────────────────────────────────────────────────────

/// The handshake is one hello and one reply of the documented size, the
/// reply no longer than the hello; everything after is sealed, carries no
/// plaintext, and opens on the far side both ways — so both derived the same
/// keys.
#[test]
fn the_handshake_keys_both_sides_and_everything_after_it_is_sealed() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let (mut client, mut peer, handshake) = connect_via(&mut listener, &mut proxy, clock);

    let (hello, reply) = (&handshake[0], &handshake[1]);
    assert_eq!(
        (hello.0, hello.1[0], hello.1.len()),
        (Way::Up, HELLO_TAG, HELLO_BYTES)
    );
    assert_eq!(
        (reply.0, reply.1[0], reply.1.len()),
        (Way::Down, HELLO_REPLY_TAG, HELLO_BYTES)
    );
    assert!(reply.1.len() <= hello.1.len(), "the reply must not amplify");
    assert!(
        handshake[2..].iter().all(|(_, d)| d[0] == SEALED_TAG),
        "{handshake:?}"
    );

    const MARKER: &[u8] = b"plaintext marker";
    client
        .send_reliable(Message::reliable(MARKER.to_vec()))
        .expect("send");
    peer.send_reliable(Message::reliable(MARKER.to_vec()))
        .expect("send");
    let mut wire = Vec::new();
    let (mut at_peer, mut at_client) = (Vec::new(), Vec::new());
    wait_until("the marker both ways", || {
        wire.extend(proxy.forward());
        while let Some(message) = peer.recv().expect("up") {
            at_peer.push(message.payload);
        }
        while let Some(message) = client.recv().expect("up") {
            at_client.push(message.payload);
        }
        !at_peer.is_empty() && !at_client.is_empty()
    });
    assert_eq!(
        (at_peer, at_client),
        (vec![MARKER.to_vec()], vec![MARKER.to_vec()])
    );
    for (_, datagram) in &wire {
        assert_eq!(datagram[0], SEALED_TAG);
        assert!(
            !datagram.windows(MARKER.len()).any(|w| w == MARKER),
            "a payload crossed the wire in clear"
        );
    }
}

/// A reply forged by someone who did not see the hello — its nonce is not
/// the hello's — is ignored, and the real reply still keys the link.
#[test]
fn a_reply_without_the_hellos_nonce_is_ignored() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let mut client =
        UdpTransport::connect_with(proxy.addr(), PROTOCOL, clock.clone()).expect("connect");
    let held = proxy.hold_until("the hello", |held| !held.is_empty());

    let forger = KeyPair::from_secret_bytes([0x33; 32]);
    let forged = Hello {
        nonce: [0; HELLO_NONCE_BYTES],
        public_key: forger.public_key(),
    }
    .encode(HELLO_REPLY_TAG, PROTOCOL);
    proxy.deliver(Way::Down, &forged);
    wait_until("the forged reply", || {
        pump(&mut client);
        client.stats().stray >= 1
    });
    assert_eq!(client.state(), UdpState::Connecting);

    for (way, datagram) in &held {
        proxy.deliver(*way, datagram);
    }
    let mut accepted = None;
    wait_until("the real handshake", || {
        proxy.forward();
        pump(&mut client);
        accepted = accepted.take().or_else(|| listener.accept());
        accepted.is_some() && client.state() == UdpState::Connected
    });
    let mut peer = accepted.expect("accepted");
    client
        .send_reliable(Message::reliable(b"keys agree".to_vec()))
        .expect("send");
    let mut got = Vec::new();
    wait_until("the message", || {
        proxy.forward();
        while let Some(message) = peer.recv().expect("up") {
            got.push(message.payload);
        }
        !got.is_empty()
    });
    assert_eq!(got, [b"keys agree".to_vec()]);
    assert_eq!(client.stats().stray, 1);
}

/// Connecting to a port nobody answers: the hello is resent on its interval,
/// sends push back, and at the connect timeout — not before — the link ends
/// with a typed reason.
#[test]
fn a_connect_nobody_answers_times_out_with_its_own_reason() {
    let clock = ManualClock::new();
    let silent = bind_loopback();
    let mut client = UdpTransport::connect_with(
        silent.local_addr().expect("address"),
        PROTOCOL,
        clock.clone(),
    )
    .expect("connect");
    let hellos = |count: usize| {
        let mut buffer = [0u8; 2048];
        let mut seen = 0;
        wait_until("the hellos", || {
            while let Ok((len, _)) = silent.recv_from(&mut buffer) {
                assert_eq!((buffer[0], len), (HELLO_TAG, HELLO_BYTES));
                seen += 1;
            }
            seen >= count
        });
    };
    hellos(1);
    assert!(client.is_connected(), "connecting counts as coming up");
    assert!(matches!(
        client.send_reliable(Message::reliable(b"early".to_vec())),
        Err(TransportError::Backpressure)
    ));

    clock.advance(HELLO_RESEND_INTERVAL);
    pump(&mut client);
    hellos(1);

    clock.advance(CONNECT_TIMEOUT - HELLO_RESEND_INTERVAL - Duration::from_nanos(1));
    pump(&mut client);
    assert_eq!(client.state(), UdpState::Connecting);
    clock.advance(Duration::from_nanos(1));
    assert!(matches!(client.recv(), Err(TransportError::Disconnected)));
    assert_eq!(client.end_reason(), Some(EndReason::ConnectTimedOut));
    assert!(!client.is_connected());
}

// ── Hostile traffic ──────────────────────────────────────────────────────────

/// Datagrams from a third socket — to the client and to the listener — are
/// counted and dropped, and the link carries on untouched.
#[test]
fn datagrams_from_a_stranger_are_dropped_without_disturbing_the_link() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), SystemClock::new());
    let (mut client, mut peer) = connect(&mut listener, SystemClock::new());
    let stranger = bind_loopback();
    let client_addr = SocketAddr::from((
        [127, 0, 0, 1],
        client.local_addr().expect("client address").port(),
    ));
    let listener_addr = listener.local_addr().expect("listener address");
    let mut garbage = vec![0xEE; 64];
    garbage[0] = SEALED_TAG;
    for datagram in [garbage.as_slice(), &[HELLO_REPLY_TAG; HELLO_BYTES]] {
        stranger.send_to(datagram, client_addr).expect("send");
        stranger.send_to(datagram, listener_addr).expect("send");
    }
    wait_until("the stranger's datagrams", || {
        pump(&mut client);
        assert!(listener.accept().is_none(), "nothing confirmed");
        client.stats().foreign >= 2 && listener.stats().unknown_source >= 2
    });

    client
        .send_reliable(Message::reliable(b"still here".to_vec()))
        .expect("send");
    assert_eq!(
        payloads(&receive(&mut peer, &mut client, 1)),
        [b"still here".to_vec()]
    );
    peer.send_reliable(Message::reliable(b"and here".to_vec()))
        .expect("send");
    assert_eq!(
        payloads(&receive(&mut client, &mut peer, 1)),
        [b"and here".to_vec()]
    );
    assert_eq!(client.stats().unopened, 0, "the client's filter came first");
}

/// A captured sealed datagram sent again is refused by the replay window:
/// counted, not delivered twice, and the link carries on.
#[test]
fn a_replayed_datagram_is_dropped() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let (mut client, mut peer, _) = connect_via(&mut listener, &mut proxy, clock);

    client
        .send_reliable(Message::reliable(b"once".to_vec()))
        .expect("send");
    let captured = proxy.hold_until("the message", |held| {
        held.iter().any(|(way, _)| *way == Way::Up)
    });
    for (way, datagram) in &captured {
        proxy.deliver(*way, datagram);
    }
    assert_eq!(
        payloads(&receive(&mut peer, &mut client, 1)),
        [b"once".to_vec()]
    );

    let before = peer.stats().unopened;
    let replayed = captured.iter().filter(|(way, _)| *way == Way::Up).count() as u64;
    for (way, datagram) in &captured {
        proxy.deliver(*way, datagram);
    }
    wait_until("the replay", || {
        pump(&mut peer);
        peer.stats().unopened >= before + replayed
    });

    client
        .send_reliable(Message::reliable(b"next".to_vec()))
        .expect("send");
    let mut got = Vec::new();
    wait_until("the next message", || {
        proxy.forward();
        while let Some(message) = peer.recv().expect("up") {
            got.push(message.payload);
        }
        !got.is_empty()
    });
    assert_eq!(got, [b"next".to_vec()], "the replay was delivered");
}

/// A sealed datagram with one bit flipped does not open: it is counted and
/// dropped, the link stays up, and the untouched original is still taken.
#[test]
fn a_tampered_datagram_is_dropped() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let (mut client, mut peer, _) = connect_via(&mut listener, &mut proxy, clock);

    client
        .send_reliable(Message::reliable(b"intact".to_vec()))
        .expect("send");
    let captured = proxy.hold_until("the message", |held| {
        held.iter().any(|(way, _)| *way == Way::Up)
    });
    let (_, original) = captured
        .iter()
        .find(|(way, _)| *way == Way::Up)
        .expect("upstream");
    let mut tampered = original.clone();
    let last = tampered.len() - 1;
    tampered[last / 2] ^= 0x01;
    proxy.deliver(Way::Up, &tampered);
    wait_until("the tampered datagram", || {
        pump(&mut peer);
        peer.stats().unopened >= 1
    });
    assert!(peer.is_connected());

    proxy.deliver(Way::Up, original);
    let mut got = Vec::new();
    wait_until("the original", || {
        proxy.forward();
        while let Some(message) = peer.recv().expect("up") {
            got.push(message.payload);
        }
        !got.is_empty()
    });
    assert_eq!(got, [b"intact".to_vec()]);
}

/// A valid hello from `socket` with the key `seed` makes.
fn send_hello(socket: &UdpSocket, to: SocketAddr, seed: u8) -> [u8; HELLO_BYTES] {
    let hello = Hello {
        nonce: [seed; HELLO_NONCE_BYTES],
        public_key: KeyPair::from_secret_bytes([seed; 32]).public_key(),
    }
    .encode(HELLO_TAG, PROTOCOL);
    socket.send_to(&hello, to).expect("send hello");
    hello
}

/// Replies waiting on `socket`, each checked to be no longer than a hello.
fn replies(socket: &UdpSocket) -> usize {
    let mut buffer = [0u8; 2048];
    let mut count = 0;
    while let Ok((len, _)) = socket.recv_from(&mut buffer) {
        assert_eq!((buffer[0], len), (HELLO_REPLY_TAG, HELLO_BYTES));
        count += 1;
    }
    count
}

/// Hellos from more addresses than the pending cap: the cap holds, the rest
/// go unanswered, one address cannot swap its pending key, and the pending
/// entries expire. Then the peer cap holds, and a freed slot is taken.
#[test]
fn the_pending_and_peer_caps_hold_under_a_flood() {
    let clock = ManualClock::new();
    let config = ListenerConfig {
        max_peers: 1,
        max_pending: 2,
        ..ListenerConfig::new(PROTOCOL)
    };
    let mut listener = listener(config, clock.clone());
    let server = listener.local_addr().expect("address");

    let flooders: Vec<UdpSocket> = (0..4).map(|_| bind_loopback()).collect();
    for (seed, socket) in (1..).zip(&flooders) {
        send_hello(socket, server, seed);
    }
    wait_until("the flood", || {
        assert!(listener.accept().is_none(), "nothing confirmed");
        let stats = listener.stats();
        stats.hellos_answered + stats.pending_full >= 4
    });
    let stats = listener.stats();
    assert_eq!(
        (stats.pending, stats.hellos_answered, stats.pending_full),
        (2, 2, 2)
    );
    let mut answered = vec![0; flooders.len()];
    wait_until("the replies", || {
        for (count, socket) in answered.iter_mut().zip(&flooders) {
            *count += replies(socket);
        }
        answered.iter().sum::<usize>() >= 2
    });
    assert_eq!(answered.iter().sum::<usize>(), 2);
    let pending = answered
        .iter()
        .position(|&n| n == 1)
        .expect("an answered flooder");

    // One address, many keys: the first handshake stands. Its own hello
    // repeated is answered again, no larger.
    for seed in 100..150 {
        send_hello(&flooders[pending], server, seed);
    }
    let seed = u8::try_from(pending + 1).expect("small");
    send_hello(&flooders[pending], server, seed);
    wait_until("the conflicting hellos", || {
        assert!(listener.accept().is_none(), "nothing confirmed");
        listener.stats().conflicting >= 50
    });
    wait_until("the repeated reply", || {
        assert!(listener.accept().is_none(), "nothing confirmed");
        replies(&flooders[pending]) == 1
    });
    assert_eq!(listener.stats().pending, 2);

    clock.advance(HANDSHAKE_TIMEOUT);
    assert!(listener.accept().is_none(), "nothing confirmed");
    assert_eq!((listener.stats().pending, listener.stats().expired), (0, 2));

    let (first, first_peer) = connect(&mut listener, clock.clone());
    let mut second = UdpTransport::connect_with(server, PROTOCOL, clock.clone()).expect("connect");
    wait_until("the second hello", || {
        pump(&mut second);
        assert!(listener.accept().is_none(), "nothing confirmed");
        listener.stats().peers_full >= 1
    });
    assert_eq!(listener.stats().peers, 1);
    assert_eq!(second.state(), UdpState::Connecting);

    drop((first, first_peer));
    assert_eq!(listener.stats().peers, 0);
    clock.advance(HELLO_RESEND_INTERVAL);
    let mut accepted = None;
    wait_until("the freed slot", || {
        pump(&mut second);
        accepted = accepted.take().or_else(|| listener.accept());
        accepted.is_some() && second.state() == UdpState::Connected
    });
}

// ── Ending ───────────────────────────────────────────────────────────────────

/// A client closing, and a host dropping its peer, each reach the other side
/// as a disconnect — and free the listener's slot.
#[test]
fn a_graceful_close_is_reported_as_a_disconnect_on_the_other_side() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), SystemClock::new());

    let (mut client, mut peer) = connect(&mut listener, SystemClock::new());
    client.disconnect();
    assert_eq!(client.end_reason(), Some(EndReason::Closed));
    wait_until("the disconnect at the host", || has_ended(&mut peer));
    assert_eq!(peer.end_reason(), Some(EndReason::PeerDisconnected));
    assert_eq!(listener.stats().peers, 0);

    let (mut client, peer) = connect(&mut listener, SystemClock::new());
    drop(peer);
    wait_until("the disconnect at the client", || has_ended(&mut client));
    assert_eq!(client.end_reason(), Some(EndReason::PeerDisconnected));
}

/// A peer that goes silent is timed out after the peer timeout, reported as
/// a timeout and not a disconnect, and its slot freed.
#[test]
fn a_silent_peer_times_out_distinctly_from_a_disconnect() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let (mut client, mut peer) = connect(&mut listener, clock.clone());
    // Let the handshake's last datagrams land, so nothing from the client is
    // still in flight when the clock jumps.
    for _ in 0..2 {
        thread::sleep(SETTLE_TIME);
        pump(&mut peer);
        pump(&mut client);
    }
    thread::sleep(SETTLE_TIME);
    pump(&mut peer);

    clock.advance(PEER_TIMEOUT - Duration::from_nanos(1));
    pump(&mut peer);
    assert_eq!(peer.state(), UdpState::Connected);
    clock.advance(Duration::from_nanos(1));
    assert!(matches!(peer.recv(), Err(TransportError::Disconnected)));
    assert_eq!(peer.end_reason(), Some(EndReason::TimedOut));
    assert_eq!(listener.stats().peers, 0);
}
