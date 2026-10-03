#![no_main]

use std::net::SocketAddr;
use std::time::Duration;

use crcbl_ecs::quantize::{self, Codec, Field, Fixed, SmallestThree};
use crcbl_net::ResumeToken;
use crcbl_net::auth::{SessionKey, open};
use crcbl_net::reliable::{Endpoint, decode_packet};
use crcbl_net::seal::{KeyPair, Role, agree_channel};
use crcbl_net::udp::{Challenge, Hello, Reply, TokenKey};
use crcbl_net::{
    ManualClock, Trust, decode_ack, decode_client_to_server, decode_console_reply,
    decode_console_set, decode_delta, decode_edit_notice, decode_edit_reply, decode_edit_request,
    decode_handshake_result, decode_hello, decode_server_to_client,
};
use libfuzzer_sys::fuzz_target;

/// Every codec at once, at widths that leave padding bits, so the schema
/// decoder's length, padding and unwritten-code checks all see hostile bytes.
const EVERY_CODEC: &[Field] = &[
    Field {
        name: "fixed",
        codec: Codec::Fixed(Fixed::new(-1.0, 1.0, 13)),
    },
    Field {
        name: "half",
        codec: Codec::Half,
    },
    Field {
        name: "rotation",
        codec: Codec::Rotation(SmallestThree::new(9)),
    },
    Field {
        name: "exact",
        codec: Codec::Exact,
    },
];

fuzz_target!(|data: &[u8]| {
    let _ = decode_hello(data);
    let _ = decode_handshake_result(data);
    let _ = decode_ack(data);
    let _ = decode_client_to_server(data);
    let _ = decode_server_to_client(data);
    // A command's data once its message has opened — what a server reads from
    // any admitted peer — and the reply a client reads back.
    let _ = decode_console_set(data);
    let _ = decode_console_reply(data);
    // A scene edit's envelope, its operation as the server serving the scene
    // decodes it, and the reply and the notice a client reads back.
    let _ = decode_edit_request(data);
    let _ = crcbl_scene::edit::decode_op(data);
    let _ = decode_edit_reply(data);
    let _ = decode_edit_notice(data);
    // A replay file, header, entries and input section with its peer track:
    // a file a player may have been sent, read before anything about it is
    // trusted.
    let _ = crcbl_store::replay::FileTransport::decode(data);
    // A replay spool, as `crcbl replay --recover` reads the one a killed
    // recording left: its header, then framed records up to the first that
    // is cut short, damaged or refused, each decoded and checked.
    let _ = crcbl_store::replay::recover_spool(std::io::Cursor::new(data), &mut std::io::sink());
    // A save file, the container every game's saves go in: a player can be
    // handed one, and a damaged one is still read field by field by the
    // salvage path.
    let _ = crcbl_net_fuzz::open_save(data);
    let _ = decode_delta(data, Trust::Untrusted);
    let _ = decode_delta(data, Trust::Authenticated);
    // A snapshot's entity blobs, once it has opened and applied: the client
    // reads each physics entry as a transform in either wire form.
    let _ = crcbl_phys::Transform::decode_wire(data);
    let mut values = [0.0; quantize::value_count(EVERY_CODEC)];
    let _ = quantize::decode_values(EVERY_CODEC, data, &mut values);
    // The authenticated envelope is the outermost parser on the wire now, so
    // it sees hostile bytes before anything else does.
    let _ = open(
        &SessionKey::derive(&ResumeToken::from_bytes([0xA5; 32])),
        data,
    );
    // The seal's opener is the outermost parser a UDP transport will have: it
    // reads the clear prefix of every datagram before anything authenticates
    // it. Without the key the fuzzer reaches the framing checks and the tag
    // verification, not a successful open, which is the surface a spoofer has.
    let server = KeyPair::from_secret_bytes([0x5E; 32]);
    let client = KeyPair::from_secret_bytes([0xC1; 32]);
    if let Ok((_, mut opener)) = agree_channel(Role::Server, &server, &client.public_key(), 0) {
        let _ = opener.open(data);
        let _ = opener.open(data);
    }
    // The UDP handshake's plaintext datagrams, and the connection token a
    // hello carries: a listener reads them from any address before anything
    // is keyed. The protocol id sits after the tag and version byte, read
    // from the input so the fuzzer can get past that filter into the bodies.
    // Without the key the token reaches its framing and the MAC check, the
    // surface a spoofer has.
    let handshake_protocol = data
        .get(2..6)
        .map_or(0, |id| u32::from_le_bytes([id[0], id[1], id[2], id[3]]));
    let _ = Challenge::decode(data, handshake_protocol);
    let _ = Reply::decode(data, handshake_protocol);
    let tokens = TokenKey::from_secret_bytes([0x7E; 32]);
    let client = SocketAddr::from(([127, 0, 0, 1], 4100));
    let _ = tokens.verify(data, client, handshake_protocol, Duration::ZERO);
    if let Some(hello) = Hello::decode(data, handshake_protocol) {
        let _ = tokens.verify(&hello.token, client, handshake_protocol, Duration::ZERO);
    }
    // The reliability layer's packet decoder, and the endpoint state machine
    // behind it: under the seal a datagram only reaches them once it has
    // opened, but they must hold on bytes they were never meant to see. The
    // protocol id is read from the input so the fuzzer can get past the
    // filter and into the bodies, and the datagram is fed to a live endpoint
    // twice so the duplicate path runs too.
    let protocol_id = data
        .get(..4)
        .map_or(0, |id| u32::from_le_bytes([id[0], id[1], id[2], id[3]]));
    let _ = decode_packet(data, protocol_id);
    let mut endpoint = Endpoint::new(protocol_id, ManualClock::new());
    let _ = endpoint.receive_datagram(data);
    let _ = endpoint.receive_datagram(data);
    while endpoint.poll_outgoing().is_some() {}
    while endpoint.recv().is_some() {}
});
