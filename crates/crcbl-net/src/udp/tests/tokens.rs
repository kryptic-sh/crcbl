//! Connection tokens over loopback: who gets a challenge, who gets a session,
//! and every way a token is refused.
//!
//! A plain socket plays the client here, so each test sees exactly which
//! datagram the listener answered with; [`connect`](super::connect) covers
//! the real client taking the same path end to end.

use super::*;
use crate::seal::{Role, agree_channel};

/// What reaches `socket` within [`SETTLE_TIME`] while `listener` is read:
/// for proving a listener sent nothing, where there is nothing to wait for.
fn settled<C: Clock + Clone>(listener: &mut UdpListener<C>, socket: &UdpSocket) -> Vec<Vec<u8>> {
    let mut buffer = [0u8; 2048];
    let mut got = Vec::new();
    let start = Instant::now();
    while start.elapsed() < SETTLE_TIME {
        assert!(listener.accept().is_none(), "nothing confirmed");
        while let Ok((len, _)) = socket.recv_from(&mut buffer) {
            got.push(buffer[..len].to_vec());
        }
        thread::sleep(POLL_PAUSE);
    }
    got
}

/// The reply `presented` — a hello from `socket` made by
/// [`hello_from`]`(seed)` — earned, keyed into a sealed datagram that
/// confirms it, and the peer `listener` hands out for it.
fn confirm<C: Clock + Clone>(
    listener: &mut UdpListener<C>,
    socket: &UdpSocket,
    seed: u8,
) -> UdpTransport<C> {
    let reply = Reply::decode(&next_datagram(listener, socket), PROTOCOL).expect("a reply");
    let key_pair = KeyPair::from_secret_bytes([seed; 32]);
    let (mut sealer, _) =
        agree_channel(Role::Client, &key_pair, &reply.public_key, PROTOCOL).expect("agreed");
    let server = listener.local_addr().expect("address");
    socket
        .send_to(&sealer.seal(b"confirm").expect("sealed"), server)
        .expect("send sealed");
    let mut accepted = None;
    wait_until("the confirmed peer", || {
        accepted = listener.accept();
        accepted.is_some()
    });
    accepted.expect("accepted")
}

/// A hello without a token is answered by one challenge, no longer than the
/// hello, and nothing else: no reply, no pending entry, no key agreement.
#[test]
fn a_hello_without_a_token_gets_a_challenge_no_larger_and_no_session() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let server = listener.local_addr().expect("address");
    let socket = bind_loopback();
    let hello = hello_from(1).encode(PROTOCOL);
    socket.send_to(&hello, server).expect("send hello");
    let answer = next_datagram(&mut listener, &socket);
    let challenge = Challenge::decode(&answer, PROTOCOL).expect("a challenge");
    assert_ne!(challenge.token, NO_TOKEN);
    assert!(
        answer.len() <= hello.len(),
        "the challenge must not amplify"
    );
    assert!(
        settled(&mut listener, &socket).is_empty(),
        "one answer only"
    );
    let stats = listener.stats();
    assert_eq!(
        (stats.challenges, stats.hellos_answered, stats.pending),
        (1, 0, 0)
    );
}

/// The challenge's token presented back from the same address earns the
/// reply, and a sealed datagram under the agreed key makes it a peer.
#[test]
fn a_valid_token_gets_a_session() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let socket = bind_loopback();
    present_token(&mut listener, &socket, hello_from(1));
    let peer = confirm(&mut listener, &socket, 1);
    assert_eq!(peer.state(), UdpState::Connected);
    let stats = listener.stats();
    assert_eq!(
        (stats.challenges, stats.hellos_answered, stats.peers),
        (1, 1, 1)
    );
}

/// A client that resent its hello before the first challenge came back gets
/// two challenges. Presenting the first earns the reply; when that reply is
/// lost, the hello it repeats carries the second token — and still gets the
/// same reply, not a refusal that would strand it until the connect times out.
#[test]
fn a_repeat_presenting_a_later_challenges_token_gets_the_same_reply() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let socket = bind_loopback();
    let first = challenge_for(&mut listener, &socket, &hello_from(1));
    let second = challenge_for(&mut listener, &socket, &hello_from(1));
    assert_ne!(first.token, second.token);
    let server = listener.local_addr().expect("address");

    let mut replies = Vec::new();
    for token in [first.token, second.token] {
        let hello = Hello {
            token,
            ..hello_from(1)
        };
        socket
            .send_to(&hello.encode(PROTOCOL), server)
            .expect("send hello");
        replies.push(Reply::decode(
            &next_datagram(&mut listener, &socket),
            PROTOCOL,
        ));
    }
    assert!(replies[0].is_some());
    assert_eq!(replies[0], replies[1], "the stored reply, resent");
    let stats = listener.stats();
    assert_eq!(
        (stats.hellos_answered, stats.conflicting, stats.pending),
        (1, 0, 1)
    );
}

/// A token is valid until [`TOKEN_LIFETIME`] after it was minted and not at
/// it; an expired one earns a fresh challenge and no session.
#[test]
fn an_expired_token_is_refused_with_a_fresh_challenge() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let (in_time, too_late) = (bind_loopback(), bind_loopback());
    let first = challenge_for(&mut listener, &in_time, &hello_from(1));
    let second = challenge_for(&mut listener, &too_late, &hello_from(2));
    let server = listener.local_addr().expect("address");

    clock.advance(TOKEN_LIFETIME - Duration::from_nanos(1));
    let presented = Hello {
        token: first.token,
        ..hello_from(1)
    };
    in_time
        .send_to(&presented.encode(PROTOCOL), server)
        .expect("send hello");
    assert!(Reply::decode(&next_datagram(&mut listener, &in_time), PROTOCOL).is_some());

    clock.advance(Duration::from_nanos(1));
    let presented = Hello {
        token: second.token,
        ..hello_from(2)
    };
    too_late
        .send_to(&presented.encode(PROTOCOL), server)
        .expect("send hello");
    let fresh = Challenge::decode(&next_datagram(&mut listener, &too_late), PROTOCOL)
        .expect("a fresh challenge");
    assert_ne!(fresh.token, second.token);
    let stats = listener.stats();
    assert_eq!(
        (stats.tokens_expired, stats.hellos_answered, stats.pending),
        (1, 1, 1)
    );
}

/// A token presented from any address but the one it was minted for earns a
/// challenge to the presenter and no session — and the token still works
/// from its own address.
#[test]
fn a_token_from_another_address_is_refused() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let (owner, thief) = (bind_loopback(), bind_loopback());
    let challenge = challenge_for(&mut listener, &owner, &hello_from(1));
    let server = listener.local_addr().expect("address");

    let stolen = Hello {
        token: challenge.token,
        ..hello_from(2)
    };
    thief
        .send_to(&stolen.encode(PROTOCOL), server)
        .expect("send hello");
    assert!(Challenge::decode(&next_datagram(&mut listener, &thief), PROTOCOL).is_some());
    let stats = listener.stats();
    assert_eq!(
        (stats.tokens_forged, stats.hellos_answered, stats.pending),
        (1, 0, 0)
    );

    let own = Hello {
        token: challenge.token,
        ..hello_from(1)
    };
    owner
        .send_to(&own.encode(PROTOCOL), server)
        .expect("send hello");
    assert!(Reply::decode(&next_datagram(&mut listener, &owner), PROTOCOL).is_some());
}

/// A token buys one handshake. Once its session has come and gone, the same
/// hello replayed from the same address — well inside the token's lifetime —
/// earns a challenge, not a second agreement.
#[test]
fn a_spent_token_is_refused() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let socket = bind_loopback();
    let presented = present_token(&mut listener, &socket, hello_from(1));
    drop(confirm(&mut listener, &socket, 1));
    assert_eq!(listener.stats().peers, 0);
    // The dropped peer's disconnect packets, so the next read is the answer.
    settled(&mut listener, &socket);

    let server = listener.local_addr().expect("address");
    socket.send_to(&presented, server).expect("send hello");
    assert!(Challenge::decode(&next_datagram(&mut listener, &socket), PROTOCOL).is_some());
    let stats = listener.stats();
    assert_eq!(
        (stats.tokens_spent, stats.hellos_answered, stats.pending),
        (1, 1, 0)
    );
}

/// A listener bound again at the same address — a restart — has a new key,
/// so a token the old one minted earns a challenge and no session.
#[test]
fn a_restarted_listener_refuses_the_old_runs_tokens() {
    let clock = ManualClock::new();
    let mut before = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let server = before.local_addr().expect("address");
    let socket = bind_loopback();
    let challenge = challenge_for(&mut before, &socket, &hello_from(1));
    drop(before);

    let mut after = UdpListener::bind_with(server, ListenerConfig::new(PROTOCOL), clock)
        .expect("the address a listener just released binds again");
    let presented = Hello {
        token: challenge.token,
        ..hello_from(1)
    };
    socket
        .send_to(&presented.encode(PROTOCOL), server)
        .expect("send hello");
    assert!(Challenge::decode(&next_datagram(&mut after, &socket), PROTOCOL).is_some());
    let stats = after.stats();
    assert_eq!(
        (stats.tokens_forged, stats.hellos_answered, stats.pending),
        (1, 0, 0)
    );
}

/// The previous transport version's hello — its layout, and the current
/// layout under its version byte — is not answered at all.
#[test]
fn an_old_versions_hello_is_not_answered() {
    let mut listener = listener(ListenerConfig::new(PROTOCOL), ManualClock::new());
    let server = listener.local_addr().expect("address");
    let socket = bind_loopback();
    let hello = hello_from(1);
    let mut old_layout = vec![HELLO_TAG, TRANSPORT_VERSION - 1];
    old_layout.extend_from_slice(&PROTOCOL.to_le_bytes());
    old_layout.extend_from_slice(&hello.nonce);
    old_layout.extend_from_slice(&hello.public_key);
    let mut old_version = hello.encode(PROTOCOL);
    old_version[1] = TRANSPORT_VERSION - 1;
    for datagram in [old_layout.as_slice(), &old_version] {
        socket.send_to(datagram, server).expect("send hello");
    }
    wait_until("the old hellos", || {
        assert!(listener.accept().is_none(), "nothing confirmed");
        listener.stats().malformed >= 2
    });
    assert!(settled(&mut listener, &socket).is_empty(), "no answer");
    assert_eq!(listener.stats().challenges, 0);
}

/// The previous version's reply — the same layout as the current one, under
/// its version byte — is not taken by a client, which carries on connecting
/// and takes the real handshake when it comes.
#[test]
fn an_old_versions_reply_is_not_taken() {
    let clock = ManualClock::new();
    let mut listener = listener(ListenerConfig::new(PROTOCOL), clock.clone());
    let mut proxy = Proxy::new(listener.local_addr().expect("address"));
    let mut client =
        UdpTransport::connect_with(proxy.addr(), PROTOCOL, clock.clone()).expect("connect");
    let held = proxy.hold_until("the hello", |held| !held.is_empty());
    let hello = Hello::decode(&held[0].1, PROTOCOL).expect("a hello");

    let mut old = Reply {
        nonce: hello.nonce,
        public_key: KeyPair::from_secret_bytes([0x33; 32]).public_key(),
    }
    .encode(PROTOCOL);
    old[1] = TRANSPORT_VERSION - 1;
    proxy.deliver(Way::Down, &old);
    wait_until("the old reply", || {
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
    assert_eq!(client.stats().stray, 1);
}
