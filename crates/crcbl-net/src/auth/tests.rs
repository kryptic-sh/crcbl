use super::*;

fn key() -> SessionKey {
    SessionKey::derive(&ResumeToken::from_bytes([0xA5; 32]))
}

#[test]
fn hmac_matches_rfc4231_test_case_1() {
    // RFC 4231 §4.2: key = 0x0b × 20, data = "Hi There".
    let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
    let expected: [u8; 32] = [
        0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b, 0xf1,
        0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c, 0x2e, 0x32,
        0xcf, 0xf7,
    ];
    assert_eq!(mac, expected);
}

#[test]
fn hmac_matches_rfc4231_test_case_2() {
    // RFC 4231 §4.3: key = "Jefe", data = "what do ya want for nothing?".
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    let expected: [u8; 32] = [
        0x5b, 0xdc, 0xc1, 0x46, 0xbf, 0x60, 0x75, 0x4e, 0x6a, 0x04, 0x24, 0x26, 0x08, 0x95, 0x75,
        0xc7, 0x5a, 0x00, 0x3f, 0x08, 0x9d, 0x27, 0x39, 0x83, 0x9d, 0xec, 0x58, 0xb9, 0x64, 0xec,
        0x38, 0x43,
    ];
    assert_eq!(mac, expected);
}

#[test]
fn hmac_hashes_oversized_keys() {
    // RFC 4231 §4.6: key = 0xaa × 131 (longer than the block), so the key
    // is hashed first.
    let mac = hmac_sha256(
        &[0xaa; 131],
        b"Test Using Larger Than Block-Size Key - Hash Key First",
    );
    let expected: [u8; 32] = [
        0x60, 0xe4, 0x31, 0x59, 0x1e, 0xe0, 0xb6, 0x7f, 0x0d, 0x8a, 0x26, 0xaa, 0xcb, 0xf5, 0xb7,
        0x7f, 0x8e, 0x0b, 0xc6, 0x21, 0x37, 0x28, 0xc5, 0x14, 0x05, 0x46, 0x04, 0x0f, 0x0e, 0xe3,
        0x7f, 0x54,
    ];
    assert_eq!(mac, expected);
}

#[test]
fn an_envelope_opens_back_to_the_counter_and_payload_it_sealed() {
    let key = key();
    let sealed = seal(&key, 7, b"payload");
    let (counter, payload) = open(&key, &sealed).expect("own envelope verifies");
    assert_eq!(counter, 7);
    assert_eq!(payload, b"payload");
}

#[test]
fn envelope_is_tagged_and_sized() {
    let sealed = seal(&key(), 1, b"abc");
    assert_eq!(sealed[0], AUTH_TAG);
    assert_eq!(sealed.len(), AUTH_OVERHEAD + 3);
}

#[test]
fn a_different_key_does_not_verify() {
    let sealed = seal(&key(), 1, b"payload");
    let other = SessionKey::derive(&ResumeToken::from_bytes([0x5A; 32]));
    assert_eq!(open(&other, &sealed), Err(AuthError::BadMac));
}

#[test]
fn tampering_with_any_field_fails() {
    let key = key();
    let sealed = seal(&key, 3, b"payload");
    for index in 0..sealed.len() {
        let mut forged = sealed.clone();
        forged[index] ^= 0x01;
        if index == 0 {
            assert_eq!(open(&key, &forged), Err(AuthError::NotSealed));
        } else {
            assert_eq!(open(&key, &forged), Err(AuthError::BadMac));
        }
    }
}

#[test]
fn unsealed_and_truncated_payloads_are_rejected() {
    let key = key();
    assert_eq!(open(&key, &[]), Err(AuthError::NotSealed));
    assert_eq!(open(&key, &[0x30, 0x00]), Err(AuthError::NotSealed));
    assert_eq!(open(&key, &[AUTH_TAG; 4]), Err(AuthError::TooShort));
    // An envelope of exactly the overhead carries an empty payload.
    let empty = seal(&key, 1, &[]);
    assert_eq!(empty.len(), AUTH_OVERHEAD);
    assert_eq!(open(&key, &empty).expect("verifies").1, b"");
}

#[test]
fn open_never_panics_on_arbitrary_bytes() {
    let key = key();
    let mut state: u64 = 0x4352_4342_4c41_5554;
    for _ in 0..2_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let len = (state % 96) as usize;
        let mut payload = Vec::with_capacity(len + 1);
        payload.push(AUTH_TAG);
        let mut h = state;
        for _ in 0..len {
            h ^= h << 13;
            h ^= h >> 7;
            h ^= h << 17;
            payload.push(h as u8);
        }
        let _ = open(&key, &payload);
    }
}

#[test]
fn replay_window_rejects_repeats_and_accepts_reorder() {
    let mut window = ReplayWindow::default();
    assert!(!window.accept(0));
    assert!(window.accept(1));
    assert!(!window.accept(1));
    assert!(window.accept(5));
    assert!(window.accept(3));
    assert!(!window.accept(3));
    assert!(window.accept(4));
    assert!(window.accept(2));
}

#[test]
fn replay_window_drops_counters_past_its_width() {
    let mut window = ReplayWindow::default();
    assert!(window.accept(1));
    assert!(window.accept(1 + ReplayWindow::WIDTH));
    // 1 is now exactly `WIDTH` behind the head and no longer representable.
    assert!(!window.accept(1));
    assert!(window.accept(2));
    // A jump clears the window entirely rather than leaving stale bits.
    assert!(window.accept(10_000));
    assert!(!window.accept(2));
    assert!(window.accept(9_999));
}

#[test]
fn session_crypto_rejects_a_replayed_packet() {
    let token = ResumeToken::from_bytes([7; 32]);
    let mut sender = SessionCrypto::from_token(&token);
    let mut receiver = SessionCrypto::from_token(&token);

    let first = sender.seal(b"ack-1").expect("counter available");
    let second = sender.seal(b"ack-2").expect("counter available");
    assert_eq!(receiver.open(&first).expect("verifies"), b"ack-1");
    assert_eq!(receiver.open(&second).expect("verifies"), b"ack-2");
    assert_eq!(receiver.open(&first), Err(AuthError::Replayed(1)));
    assert_eq!(receiver.open(&second), Err(AuthError::Replayed(2)));
}

#[test]
fn session_crypto_counters_start_at_one_and_increment() {
    let mut crypto = SessionCrypto::from_token(&ResumeToken::from_bytes([1; 32]));
    let key = SessionKey::derive(&ResumeToken::from_bytes([1; 32]));
    for expected in 1..=3u64 {
        let sealed = crypto.seal(b"x").expect("counter available");
        assert_eq!(open(&key, &sealed).expect("verifies").0, expected);
    }
}

#[test]
fn exhausted_counter_space_refuses_to_seal() {
    let mut crypto = SessionCrypto::from_token(&ResumeToken::from_bytes([2; 32]));
    crypto.next_counter = u64::MAX;
    assert!(crypto.seal(b"last").is_ok());
    assert_eq!(crypto.seal(b"over"), Err(AuthError::CounterExhausted));
}

#[test]
fn key_derivation_is_token_specific_and_not_the_token() {
    let token = ResumeToken::from_bytes([0x11; 32]);
    let a = SessionKey::derive(&token);
    let b = SessionKey::derive(&token);
    let c = SessionKey::derive(&ResumeToken::from_bytes([0x12; 32]));
    assert_eq!(a.0, b.0);
    assert_ne!(a.0, c.0);
    assert_ne!(a.0, [0x11; 32]);
}

#[test]
fn debug_redacts_the_key() {
    let debug = format!("{:?}", key());
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("165"));
}

#[test]
fn constant_time_eq_compares_contents_and_length() {
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"ab"));
}

#[test]
fn an_auth_error_names_the_check_that_failed_and_the_offending_counter() {
    assert_eq!(
        AuthError::Replayed(9).to_string(),
        "replayed or out-of-window counter: 9"
    );
    assert_eq!(
        AuthError::BadMac.to_string(),
        "message authentication code does not match"
    );
}
