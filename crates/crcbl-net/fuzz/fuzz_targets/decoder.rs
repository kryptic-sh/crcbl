#![no_main]

use crcbl_net::{
    decode_ack, decode_client_to_server, decode_delta, decode_handshake_result, decode_hello,
    decode_server_to_client, decode_trusted_delta,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_hello(data);
    let _ = decode_handshake_result(data);
    let _ = decode_ack(data);
    let _ = decode_client_to_server(data);
    let _ = decode_server_to_client(data);
    let _ = decode_delta(data);
    let _ = decode_trusted_delta(data);
});
