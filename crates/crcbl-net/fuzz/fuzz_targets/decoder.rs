#![no_main]

use crcbl_net::auth::{SessionKey, open};
use crcbl_net::{
    Trust, decode_ack, decode_client_to_server, decode_delta, decode_handshake_result, decode_hello,
    decode_server_to_client,
};
use crcbl_net::ResumeToken;
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
    let _ = open(&SessionKey::derive(&ResumeToken::from_bytes([0xA5; 32])), data);
});
