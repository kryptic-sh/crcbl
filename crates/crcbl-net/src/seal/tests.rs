//! The seal on its own, then two endpoints talking through it over a seeded
//! lossy wire with an adversary on it.

use std::time::Duration;

use chacha20poly1305::{AeadInOut, Key, KeyInit, Tag, XChaCha20Poly1305, XNonce};

use super::nonce::Direction;
use super::*;
use crate::reliable::tests::{
    PROTOCOL, Wire, accept, hostile_conditions, numbered_message, payloads,
};
use crate::reliable::{
    Channel, Delivery, Endpoint, MAX_DATAGRAM_BYTES, MAX_PACKET_BYTES, MAX_RELIABLE_MESSAGE_BYTES,
    MAX_UNRELIABLE_PAYLOAD,
};
use crate::{Clock, ManualClock, SimConditions};

/// Bytes from a hex string; whitespace is skipped, so a vector can be laid
/// out the way its specification prints it.
pub(crate) fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert!(digits.len().is_multiple_of(2), "odd number of hex digits");
    digits
        .chunks(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("a hex digit pair")
        })
        .collect()
}

const SHARED: [u8; X25519_BYTES] = [0x42; X25519_BYTES];
const CLIENT_PUBLIC: [u8; X25519_BYTES] = [0xC1; X25519_BYTES];
const SERVER_PUBLIC: [u8; X25519_BYTES] = [0x5E; X25519_BYTES];

fn channel(role: Role, shared: &[u8; X25519_BYTES]) -> (Sealer, Opener) {
    derive_channel(role, shared, &CLIENT_PUBLIC, &SERVER_PUBLIC, PROTOCOL).unwrap()
}

/// The client's sealer and the server's opener: one direction of a link.
fn client_to_server() -> (Sealer, Opener) {
    let (sealer, _) = channel(Role::Client, &SHARED);
    let (_, opener) = channel(Role::Server, &SHARED);
    (sealer, opener)
}

/// A packet whose bytes say which one it is.
fn packet(n: u8) -> Vec<u8> {
    (0..40).map(|i: u8| i.wrapping_mul(7) ^ n).collect()
}

// ── The primitive ────────────────────────────────────────────────────────────

/// draft-irtf-cfrg-xchacha-03 Appendix A.3.1, through the crate the seal is
/// built on: the cipher is XChaCha20-Poly1305 as specified, not a lookalike.
#[test]
fn xchacha20_poly1305_matches_the_draft_vector() {
    let plaintext = hex(
        "4c616469657320616e642047656e746c656d656e206f662074686520636c6173
         73206f66202739393a204966204920636f756c64206f6666657220796f75206f
         6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73
         637265656e20776f756c642062652069742e",
    );
    let aad = hex("50515253c0c1c2c3c4c5c6c7");
    let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = hex("404142434445464748494a4b4c4d4e4f5051525354555657");
    let ciphertext = hex(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
         731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
         2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
         21f9664c97637da9768812f615c68b13b52e",
    );
    let tag = hex("c0875924c1c7987947deafd8780acf49");

    let cipher = XChaCha20Poly1305::new(&Key::try_from(&key[..]).unwrap());
    let nonce = XNonce::try_from(&nonce[..]).unwrap();
    let mut buffer = plaintext.clone();
    let got_tag = cipher
        .encrypt_inout_detached(&nonce, &aad, buffer.as_mut_slice().into())
        .unwrap();
    assert_eq!(buffer, ciphertext);
    assert_eq!(got_tag[..], tag[..]);

    cipher
        .decrypt_inout_detached(
            &nonce,
            &aad,
            buffer.as_mut_slice().into(),
            &Tag::try_from(&tag[..]).unwrap(),
        )
        .unwrap();
    assert_eq!(buffer, plaintext);
}

// ── Seal and open ────────────────────────────────────────────────────────────

#[test]
fn a_sealed_packet_opens_back_to_itself_and_hides_it() {
    let (mut sealer, mut opener) = client_to_server();
    for n in 0..4u8 {
        let sealed = sealer.seal(&packet(n)).unwrap();
        assert_eq!(sealed.len(), packet(n).len() + SEAL_OVERHEAD);
        assert_eq!(sealed[0], SEALED_TAG);
        let counter = FIRST_COUNTER + u64::from(n);
        assert_eq!(sealed[1..SEAL_PREFIX_BYTES], counter.to_le_bytes());
        assert!(
            !sealed.windows(8).any(|window| window == &packet(n)[..8]),
            "the packet is not visible in the datagram"
        );
        assert_eq!(opener.open(&sealed).unwrap(), packet(n));
    }
    // An empty packet still carries a tag, and opens to empty.
    let sealed = sealer.seal(&[]).unwrap();
    assert_eq!(sealed.len(), SEAL_OVERHEAD);
    assert_eq!(opener.open(&sealed).unwrap(), Vec::<u8>::new());
}

/// The datagram is exactly the documented layout, rebuilt here from the raw
/// cipher by hand: the nonce is the direction byte, the counter and zeros; the
/// associated data is the tag byte and counter. A peer that follows the module
/// docs interoperates, and a seal that stopped authenticating its prefix —
/// which the counter's place in the nonce would otherwise hide — is caught.
#[test]
fn the_seal_is_the_documented_layout_under_xchacha20_poly1305() {
    let key = Key::from([0x77; 32]);
    let mut sealer = Sealer::new(XChaCha20Poly1305::new(&key), Direction::ServerToClient)
        .with_next_counter(0x0102_0304_0506_0708);
    let sealed = sealer.seal(&packet(3)).unwrap();

    let counter = 0x0102_0304_0506_0708u64.to_le_bytes();
    let mut nonce = [0u8; 24];
    nonce[0] = 0x02;
    nonce[1..9].copy_from_slice(&counter);
    let associated_data = [&[SEALED_TAG][..], &counter].concat();
    let mut body = packet(3);
    let tag = XChaCha20Poly1305::new(&key)
        .encrypt_inout_detached(
            &XNonce::from(nonce),
            &associated_data,
            body.as_mut_slice().into(),
        )
        .unwrap();
    let expected = [&associated_data[..], &body, &tag].concat();
    assert_eq!(sealed, expected);
}

/// Every single-bit flip anywhere in the datagram — the clear tag byte and
/// counter, the ciphertext, the Poly1305 tag — fails to open.
#[test]
fn flipping_any_bit_of_a_sealed_datagram_fails_to_open() {
    let (mut sealer, _) = client_to_server();
    let sealed = sealer.seal(&packet(9)).unwrap();
    let mut flips = 0;
    for at in 0..sealed.len() {
        for bit in 0..8 {
            let mut forged = sealed.clone();
            forged[at] ^= 1 << bit;
            let (_, mut opener) = client_to_server();
            let expected = if at == 0 {
                OpenError::NotSealed
            } else {
                OpenError::Forged
            };
            assert_eq!(
                opener.open(&forged),
                Err(expected),
                "byte {at} bit {bit} flipped"
            );
            flips += 1;
        }
    }
    assert_eq!(flips, sealed.len() * 8);
}

#[test]
fn another_key_or_the_other_direction_fails_to_open() {
    let (mut sealer, _) = client_to_server();
    let sealed = sealer.seal(&packet(1)).unwrap();

    // Another session's key.
    let mut other_shared = SHARED;
    other_shared[0] ^= 1;
    let (_, mut stranger) = channel(Role::Server, &other_shared);
    assert_eq!(stranger.open(&sealed), Err(OpenError::Forged));

    // Reflected back at its sender: the client opens server-to-client.
    let (_, mut own_opener) = channel(Role::Client, &SHARED);
    assert_eq!(own_opener.open(&sealed), Err(OpenError::Forged));

    // The direction byte stands on its own: under one key, a datagram sealed
    // one way does not open the other way.
    let key = Key::from([0x77; 32]);
    let mut one_way = Sealer::new(XChaCha20Poly1305::new(&key), Direction::ClientToServer);
    let mut other_way = Opener::new(XChaCha20Poly1305::new(&key), Direction::ServerToClient);
    let mut same_way = Opener::new(XChaCha20Poly1305::new(&key), Direction::ClientToServer);
    let sealed = one_way.seal(&packet(2)).unwrap();
    assert_eq!(other_way.open(&sealed), Err(OpenError::Forged));
    assert_eq!(same_way.open(&sealed).unwrap(), packet(2));
}

#[test]
fn a_replay_or_a_too_old_datagram_is_refused_and_reordering_is_not() {
    let (mut sealer, mut opener) = client_to_server();
    let sealed: Vec<Vec<u8>> = (0..100u8)
        .map(|n| sealer.seal(&packet(n)).unwrap())
        .collect();
    let counter = |index: usize| FIRST_COUNTER + index as u64;

    // Out of order inside the window: all accepted.
    for index in [5, 3, 4, 0] {
        assert_eq!(opener.open(&sealed[index]).unwrap(), packet(index as u8));
    }
    // Each again: refused, however it arrives.
    for index in [3, 5, 0] {
        assert_eq!(
            opener.open(&sealed[index]),
            Err(OpenError::Replayed(counter(index)))
        );
    }
    // Once the newest is 99, the oldest the window still speaks for is
    // WIDTH - 1 behind it.
    assert!(opener.open(&sealed[99]).is_ok());
    let width = crate::auth::ReplayWindow::WIDTH as usize;
    let oldest_in_window = 99 - (width - 1);
    assert_eq!(
        opener.open(&sealed[oldest_in_window - 1]),
        Err(OpenError::Replayed(counter(oldest_in_window - 1)))
    );
    assert!(opener.open(&sealed[oldest_in_window]).is_ok());
}

/// A datagram claiming a counter far ahead, with any tag, is refused as
/// forged — and the window does not move, so the honest datagrams it would
/// have pushed out of the window still open.
#[test]
fn a_forged_datagram_does_not_advance_the_replay_window() {
    let (mut sealer, mut opener) = client_to_server();
    let first = sealer.seal(&packet(0)).unwrap();
    let second = sealer.seal(&packet(1)).unwrap();

    let mut ahead = second.clone();
    ahead[1..SEAL_PREFIX_BYTES].copy_from_slice(&(u64::MAX - 1).to_le_bytes());
    assert_eq!(opener.open(&ahead), Err(OpenError::Forged));
    let mut garbage = vec![0xEE; second.len()];
    garbage[0] = SEALED_TAG;
    assert_eq!(opener.open(&garbage), Err(OpenError::Forged));

    assert_eq!(opener.open(&second).unwrap(), packet(1));
    assert_eq!(opener.open(&first).unwrap(), packet(0));
}

#[test]
fn an_exhausted_counter_is_refused_and_never_wraps() {
    let (sealer, mut opener) = client_to_server();
    let mut sealer = sealer.with_next_counter(u64::MAX - 1);
    let penultimate = sealer.seal(&packet(0)).unwrap();
    let last = sealer.seal(&packet(1)).unwrap();
    assert_eq!(last[1..SEAL_PREFIX_BYTES], u64::MAX.to_le_bytes());
    for _ in 0..3 {
        assert_eq!(sealer.seal(&packet(2)), Err(SealError::CounterExhausted));
    }
    assert_eq!(opener.open(&penultimate).unwrap(), packet(0));
    assert_eq!(opener.open(&last).unwrap(), packet(1));
}

#[test]
fn malformed_datagrams_are_refused_with_the_reason() {
    let (mut sealer, mut opener) = client_to_server();
    assert_eq!(
        sealer.seal(&vec![0; MAX_PACKET_BYTES + 1]),
        Err(SealError::Oversized {
            size: MAX_PACKET_BYTES + 1,
            limit: MAX_PACKET_BYTES
        })
    );
    // The refusal cost no counter.
    let sealed = sealer.seal(&packet(0)).unwrap();
    assert_eq!(sealed[1..SEAL_PREFIX_BYTES], FIRST_COUNTER.to_le_bytes());

    assert_eq!(opener.open(&[]), Err(OpenError::NotSealed));
    for len in 1..SEAL_OVERHEAD {
        assert_eq!(
            opener.open(&sealed[..len]),
            Err(OpenError::TooShort { size: len })
        );
    }
    let mut oversized = vec![0; MAX_DATAGRAM_BYTES + 1];
    oversized[0] = SEALED_TAG;
    assert_eq!(
        opener.open(&oversized),
        Err(OpenError::Oversized {
            size: MAX_DATAGRAM_BYTES + 1,
            limit: MAX_DATAGRAM_BYTES
        })
    );
    assert_eq!(opener.open(&sealed).unwrap(), packet(0));
}

/// Every packet an endpoint emits — the fullest unreliable payload, every
/// fragment of the largest reliable message — fits the datagram budget once
/// sealed, and the fullest lands on it exactly.
#[test]
fn a_sealed_endpoint_packet_never_exceeds_the_datagram_budget() {
    let (mut sealer, mut opener) = client_to_server();
    let mut endpoint = Endpoint::new(PROTOCOL, ManualClock::new());
    endpoint
        .send(
            Channel::UnreliableSequenced,
            vec![0xAA; MAX_UNRELIABLE_PAYLOAD],
        )
        .unwrap();
    endpoint
        .send(Channel::Reliable, vec![0xBB; MAX_RELIABLE_MESSAGE_BYTES])
        .unwrap();
    let mut largest = 0;
    let mut sealed_count = 0;
    while let Some(packet) = endpoint.poll_outgoing() {
        let sealed = sealer.seal(&packet).unwrap();
        assert!(sealed.len() <= MAX_DATAGRAM_BYTES);
        largest = largest.max(sealed.len());
        assert_eq!(opener.open(&sealed).unwrap(), packet);
        sealed_count += 1;
    }
    assert!(sealed_count > 1);
    assert_eq!(largest, MAX_DATAGRAM_BYTES);
}

// ── End to end ───────────────────────────────────────────────────────────────

/// How a datagram on the test wire is labelled, so the receiving side of the
/// harness knows what it must have done with it. The label is stripped before
/// the opener sees anything.
const HONEST: u8 = 0;
const FORGED: u8 = 1;

/// One side of a sealed link: an endpoint and its seal.
struct Side {
    endpoint: Endpoint<ManualClock>,
    sealer: Sealer,
    opener: Opener,
    got: Vec<Delivery>,
    /// Forgeries that reached this side; every one must be refused.
    forged_arrived: usize,
    /// Honest datagrams refused as replays: the wire duplicates.
    honest_replays: usize,
}

impl Side {
    fn new(role: Role, clock: ManualClock) -> Self {
        let (sealer, opener) = channel(role, &SHARED);
        Self {
            endpoint: Endpoint::new(PROTOCOL, clock),
            sealer,
            opener,
            got: Vec::new(),
            forged_arrived: 0,
            honest_replays: 0,
        }
    }

    fn receive(&mut self, framed: &[u8]) {
        let (label, datagram) = framed.split_first().expect("every frame is labelled");
        let opened = self.opener.open(datagram);
        if *label == FORGED {
            self.forged_arrived += 1;
            assert!(opened.is_err(), "a forged datagram opened");
            return;
        }
        match opened {
            Ok(packet) => accept(&mut self.endpoint, &packet),
            Err(OpenError::Replayed(_)) => self.honest_replays += 1,
            Err(e) => panic!("an honest datagram did not open: {e}"),
        }
    }
}

/// Deterministic forgeries of a sealed datagram: a flipped bit, a counter
/// pushed far ahead, a truncation, and noise behind a valid tag byte.
struct Adversary {
    state: u64,
}

impl Adversary {
    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state >> 33
    }

    fn forge(&mut self, sealed: &[u8]) -> Vec<Vec<u8>> {
        let mut flipped = sealed.to_vec();
        let at = self.next() as usize % flipped.len();
        flipped[at] ^= 1 << (self.next() % 8);

        let mut ahead = sealed.to_vec();
        let counter = u64::from_le_bytes(ahead[1..SEAL_PREFIX_BYTES].try_into().unwrap());
        ahead[1..SEAL_PREFIX_BYTES].copy_from_slice(&(counter + 1_000).to_le_bytes());

        let truncated = sealed[..sealed.len() - 1].to_vec();

        let mut noise: Vec<u8> = (0..sealed.len()).map(|_| self.next() as u8).collect();
        noise[0] = SEALED_TAG;

        vec![flipped, ahead, truncated, noise]
    }
}

/// Two endpoints, each behind its seal, and the lossy wire between them.
struct SealedLink {
    clock: ManualClock,
    client: Side,
    server: Side,
    to_server: Wire,
    to_client: Wire,
    adversary: Option<Adversary>,
}

impl SealedLink {
    fn new(conditions: SimConditions, adversary: Option<Adversary>) -> Self {
        let clock = ManualClock::new();
        let back = SimConditions {
            seed: conditions.seed ^ 0x9E37_79B9_7F4A_7C15,
            ..conditions.clone()
        };
        Self {
            client: Side::new(Role::Client, clock.clone()),
            server: Side::new(Role::Server, clock.clone()),
            to_server: Wire::new(conditions, clock.clone()),
            to_client: Wire::new(back, clock.clone()),
            clock,
            adversary,
        }
    }

    fn send_all(from: &mut Side, wire: &mut Wire, adversary: &mut Option<Adversary>) {
        while let Some(packet) = from.endpoint.poll_outgoing() {
            let sealed = from.sealer.seal(&packet).expect("an endpoint packet seals");
            if let Some(adversary) = adversary {
                for forged in adversary.forge(&sealed) {
                    wire.put([&[FORGED][..], &forged].concat());
                }
            }
            wire.put([&[HONEST][..], &sealed].concat());
        }
    }

    fn step(&mut self) {
        self.clock.advance(crate::reliable::tests::STEP);
        Self::send_all(&mut self.client, &mut self.to_server, &mut self.adversary);
        Self::send_all(&mut self.server, &mut self.to_client, &mut self.adversary);
        for framed in self.to_server.take() {
            self.server.receive(&framed);
        }
        for framed in self.to_client.take() {
            self.client.receive(&framed);
        }
        for side in [&mut self.client, &mut self.server] {
            while let Some(delivery) = side.endpoint.recv() {
                side.got.push(delivery);
            }
        }
    }
}

/// Reliable traffic both ways through seal, a hostile wire and open: every
/// message arrives once and in order. With an adversary forging several
/// datagrams for each honest one, every forgery is refused and the honest
/// traffic still gets through — which it could not if a forged counter had
/// dragged the replay window ahead of it.
fn sealed_link_delivers_everything(adversary: bool) {
    const MESSAGES: usize = 200;
    for seed in 0..4u64 {
        let mut link = SealedLink::new(
            hostile_conditions(seed),
            adversary.then_some(Adversary { state: seed }),
        );
        let expected: Vec<Vec<u8>> = (0..MESSAGES).map(numbered_message).collect();
        let (mut client_next, mut server_next) = (0, 0);
        loop {
            while client_next < MESSAGES
                && link
                    .client
                    .endpoint
                    .send(Channel::Reliable, expected[client_next].clone())
                    .is_ok()
            {
                client_next += 1;
            }
            while server_next < MESSAGES
                && link
                    .server
                    .endpoint
                    .send(Channel::Reliable, expected[server_next].clone())
                    .is_ok()
            {
                server_next += 1;
            }
            link.step();
            if link.client.got.len() == MESSAGES && link.server.got.len() == MESSAGES {
                break;
            }
            assert!(
                link.clock.now() < Duration::from_secs(300),
                "seed {seed}: {} and {} of {MESSAGES} delivered",
                link.server.got.len(),
                link.client.got.len()
            );
        }
        assert_eq!(payloads(&link.server.got, Channel::Reliable), expected);
        assert_eq!(payloads(&link.client.got, Channel::Reliable), expected);
        assert!(
            link.server.honest_replays > 0,
            "seed {seed}: the wire's duplicates must have met the replay window"
        );
        for side in [&link.client, &link.server] {
            assert_eq!(side.forged_arrived > 0, adversary, "seed {seed}");
        }
    }
}

#[test]
fn two_endpoints_talk_through_the_seal_over_a_hostile_wire() {
    sealed_link_delivers_everything(false);
}

#[test]
fn a_tampering_adversary_is_refused_and_honest_traffic_still_arrives() {
    sealed_link_delivers_everything(true);
}
