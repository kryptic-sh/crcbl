#![no_main]

use crcbl_net::ResumeToken;
use crcbl_net::auth::{SessionKey, open};
use crcbl_net::reliable::{Endpoint, decode_packet};
use crcbl_net::seal::{KeyPair, Role, agree_channel};
use crcbl_net::{
    ManualClock, Trust, decode_ack, decode_client_to_server, decode_delta, decode_handshake_result,
    decode_hello, decode_server_to_client,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_hello(data);
    let _ = decode_handshake_result(data);
    let _ = decode_ack(data);
    let _ = decode_client_to_server(data);
    let _ = decode_server_to_client(data);
    let _ = decode_delta(data, Trust::Untrusted);
    let _ = decode_delta(data, Trust::Authenticated);
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
