use super::*;

fn request() -> EditRequest {
    EditRequest {
        request_id: 0x0102_0304_0506_0708,
        gesture: None,
        op: vec![1, 0, 4, 7, 0, 0, 0],
    }
}

/// [`request`] as part of a gesture, and as the gesture's last edit.
fn gestured_requests() -> [EditRequest; 2] {
    [false, true].map(|last| EditRequest {
        gesture: Some(EditGesture {
            id: 0x0A0B_0C0D,
            last,
        }),
        ..request()
    })
}

fn replies() -> [EditReply; 2] {
    [
        EditReply {
            request_id: 9,
            outcome: EditOutcome::Applied { revision: 42 },
        },
        EditReply {
            request_id: 10,
            outcome: EditOutcome::Refused {
                reason: EditRefusal::UNKNOWN_ENTITY,
                message: "the scene holds no entity 7".to_owned(),
            },
        },
    ]
}

fn notice() -> EditNotice {
    EditNotice {
        revision: 42,
        author: 3,
        gesture: None,
        op: vec![1, 2],
    }
}

/// [`notice`] as recorded in a gesture.
fn gestured_notice() -> EditNotice {
    EditNotice {
        gesture: Some(0x1112_1314_1516_1718),
        ..notice()
    }
}

/// Every message an edit travels as, encoded: what the sweeps below damage.
fn every_message() -> Vec<Vec<u8>> {
    let mut all = vec![
        encode_edit_request(&request()).expect("short enough"),
        encode_edit_notice(&notice()).expect("short enough"),
        encode_edit_notice(&gestured_notice()).expect("short enough"),
    ];
    for request in gestured_requests() {
        all.push(encode_edit_request(&request).expect("short enough"));
    }
    for reply in replies() {
        all.push(encode_edit_reply(&reply).expect("short enough"));
    }
    all
}

/// Every decoder of this module over `bytes`, whatever they hold.
fn decode_every_way(bytes: &[u8]) {
    let _ = decode_edit_request(bytes);
    let _ = decode_edit_reply(bytes);
    let _ = decode_edit_notice(bytes);
}

#[test]
fn a_request_is_the_kind_byte_then_the_id_then_the_operation() {
    let encoded = encode_edit_request(&request()).expect("short enough");
    let mut expected = vec![EDIT_KIND];
    expected.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
    expected.push(0);
    expected.extend_from_slice(&7_u32.to_le_bytes());
    expected.extend_from_slice(&[1, 0, 4, 7, 0, 0, 0]);
    assert_eq!(encoded, expected);
    assert_eq!(
        decode_edit_request(&encoded).expect("well formed"),
        request()
    );
}

/// **A gesture travels as its marker and the client's id**: 1 for an edit
/// the gesture carries on past, 2 for its last, each followed by the id —
/// and each round-trips, so a server reads the end of a drag as the client
/// meant it.
#[test]
fn a_requests_gesture_is_its_marker_then_the_clients_id() {
    for (request, marker) in gestured_requests().into_iter().zip([1, 2]) {
        let encoded = encode_edit_request(&request).expect("short enough");
        let mut expected = vec![EDIT_KIND];
        expected.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
        expected.push(marker);
        expected.extend_from_slice(&0x0A0B_0C0D_u32.to_le_bytes());
        expected.extend_from_slice(&7_u32.to_le_bytes());
        expected.extend_from_slice(&[1, 0, 4, 7, 0, 0, 0]);
        assert_eq!(encoded, expected, "{request:?}");
        assert_eq!(decode_edit_request(&encoded).expect("well formed"), request);
    }
}

/// A notice's gesture is a marker and the server's number for it, after the
/// author.
#[test]
fn a_notices_gesture_is_its_marker_then_the_servers_number() {
    let encoded = encode_edit_notice(&gestured_notice()).expect("short enough");
    let mut expected = vec![EDIT_NOTICE_TAG];
    expected.extend_from_slice(&42_u64.to_le_bytes());
    expected.extend_from_slice(&3_u64.to_le_bytes());
    expected.push(1);
    expected.extend_from_slice(&0x1112_1314_1516_1718_u64.to_le_bytes());
    expected.extend_from_slice(&2_u32.to_le_bytes());
    expected.extend_from_slice(&[1, 2]);
    assert_eq!(encoded, expected);
}

/// **A gesture marker this layout does not have is refused**, on a request
/// and on a notice alike — a notice has no last edit to mark, so the
/// request's 2 is one of them.
#[test]
fn an_unknown_gesture_marker_is_refused() {
    let marker_at = 1 + 8;
    for marker in [3, 0xFF] {
        let mut request = encode_edit_request(&request()).expect("short enough");
        request[marker_at] = marker;
        assert!(
            matches!(
                decode_edit_request(&request),
                Err(DecodeError::UnknownTag { tag }) if tag == marker
            ),
            "request marker {marker}"
        );
    }
    let marker_at = 1 + 8 + 8;
    for marker in [2, 3, 0xFF] {
        let mut notice = encode_edit_notice(&notice()).expect("short enough");
        notice[marker_at] = marker;
        assert!(
            matches!(
                decode_edit_notice(&notice),
                Err(DecodeError::UnknownTag { tag }) if tag == marker
            ),
            "notice marker {marker}"
        );
    }
}

#[test]
fn both_outcomes_of_a_reply_and_a_notice_round_trip() {
    for reply in replies() {
        let encoded = encode_edit_reply(&reply).expect("short enough");
        assert_eq!(encoded[0], EDIT_REPLY_TAG);
        assert_eq!(decode_edit_reply(&encoded).expect("well formed"), reply);
    }
    for notice in [notice(), gestured_notice()] {
        let encoded = encode_edit_notice(&notice).expect("short enough");
        assert_eq!(encoded[0], EDIT_NOTICE_TAG);
        assert_eq!(decode_edit_notice(&encoded).expect("well formed"), notice);
    }
}

#[test]
fn a_refusal_code_this_build_does_not_know_is_kept_and_printed_by_number() {
    let reply = EditReply {
        request_id: 1,
        outcome: EditOutcome::Refused {
            reason: EditRefusal(0xEE),
            message: String::new(),
        },
    };
    let decoded = decode_edit_reply(&encode_edit_reply(&reply).expect("short enough"))
        .expect("an unknown code is no malformed reply");
    assert_eq!(decoded, reply);
    assert_eq!(EditRefusal(0xEE).name(), None);
    assert_eq!(EditRefusal(0xEE).to_string(), "refusal 0xee");
    assert_eq!(EditRefusal::NOT_EDITABLE.to_string(), "not editable");
}

/// The numbers are the protocol: a renumbering would make an older client read
/// one refusal as another.
#[test]
fn the_refusal_codes_keep_their_numbers_and_names() {
    let codes = [
        (EditRefusal::MALFORMED, 0x01, "malformed"),
        (
            EditRefusal::UNSUPPORTED_VERSION,
            0x02,
            "unsupported version",
        ),
        (EditRefusal::NOT_EDITABLE, 0x03, "not editable"),
        (EditRefusal::UNKNOWN_ENTITY, 0x04, "unknown entity"),
        (EditRefusal::UNKNOWN_SYSTEM, 0x05, "unknown system"),
        (EditRefusal::UNKNOWN_PATH, 0x06, "unknown path"),
        (EditRefusal::INVALID, 0x07, "invalid"),
        (EditRefusal::CONFLICT, 0x08, "conflict"),
        (EditRefusal::NOTHING_TO_UNDO, 0x09, "nothing to undo"),
        (EditRefusal::NOTHING_TO_REDO, 0x0A, "nothing to redo"),
        (EditRefusal::FAILED, 0x0B, "failed"),
        (EditRefusal::BUSY, 0x0C, "busy"),
        (EditRefusal::TOO_LARGE, 0x0D, "too large"),
    ];
    for (code, number, name) in codes {
        assert_eq!(code.0, number);
        assert_eq!(code.name(), Some(name));
    }
}

#[test]
fn an_encoder_refuses_what_its_decoder_would_and_carries_the_limits() {
    let long_op = EditRequest {
        request_id: 1,
        gesture: None,
        op: vec![0; MAX_EDIT_OP_BYTES + 1],
    };
    assert_eq!(
        encode_edit_request(&long_op).expect_err("too long"),
        EditTooLong {
            field: "op",
            len: MAX_EDIT_OP_BYTES + 1,
            limit: MAX_EDIT_OP_BYTES,
        }
    );
    let long_notice = EditNotice {
        op: long_op.op.clone(),
        ..notice()
    };
    assert_eq!(
        encode_edit_notice(&long_notice)
            .expect_err("too long")
            .field,
        "op"
    );
    let long_message = EditReply {
        request_id: 1,
        outcome: EditOutcome::Refused {
            reason: EditRefusal::FAILED,
            message: "m".repeat(MAX_EDIT_MESSAGE_BYTES + 1),
        },
    };
    assert_eq!(
        encode_edit_reply(&long_message)
            .expect_err("too long")
            .field,
        "message"
    );

    // At the limits, each round-trips — and a request of a gesture at the op
    // limit is exactly as long as a command's data may be.
    let at_limit = EditRequest {
        request_id: 1,
        gesture: Some(EditGesture { id: 1, last: true }),
        op: vec![0xA5; MAX_EDIT_OP_BYTES],
    };
    let encoded = encode_edit_request(&at_limit).expect("at the limit");
    assert_eq!(encoded.len(), MAX_FIELD_BYTES);
    assert_eq!(
        decode_edit_request(&encoded).expect("at the limit"),
        at_limit
    );
    let notice_at_limit = EditNotice {
        gesture: Some(u64::MAX),
        op: at_limit.op,
        ..notice()
    };
    let encoded = encode_edit_notice(&notice_at_limit).expect("at the limit");
    assert_eq!(
        decode_edit_notice(&encoded).expect("at the limit"),
        notice_at_limit
    );
    let at_limit = EditReply {
        request_id: 1,
        outcome: EditOutcome::Refused {
            reason: EditRefusal::FAILED,
            message: "m".repeat(MAX_EDIT_MESSAGE_BYTES),
        },
    };
    let encoded = encode_edit_reply(&at_limit).expect("at the limit");
    assert_eq!(decode_edit_reply(&encoded).expect("at the limit"), at_limit);
}

#[test]
fn a_decoder_refuses_every_truncation_trailing_byte_and_other_tag() {
    for good in every_message() {
        for len in 0..good.len() {
            let cut = &good[..len];
            assert!(
                decode_edit_request(cut).is_err()
                    && decode_edit_reply(cut).is_err()
                    && decode_edit_notice(cut).is_err(),
                "{len} of {} bytes decoded",
                good.len()
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(
            decode_edit_request(&trailing).is_err()
                && decode_edit_reply(&trailing).is_err()
                && decode_edit_notice(&trailing).is_err(),
            "a trailing byte was ignored"
        );
    }
    let mut other = encode_edit_request(&request()).expect("short enough");
    other[0] = 0x7F;
    assert!(matches!(
        decode_edit_request(&other),
        Err(DecodeError::UnknownTag { tag: 0x7F })
    ));
    let mut outcome = encode_edit_reply(&replies()[0]).expect("short enough");
    outcome[9] = 7;
    assert!(matches!(
        decode_edit_reply(&outcome),
        Err(DecodeError::UnknownTag { tag: 7 })
    ));
}

#[test]
fn a_length_past_its_limit_is_refused_before_anything_is_read_for_it() {
    // An op claimed past the limit, with no op bytes behind the claim.
    let mut huge = vec![EDIT_KIND];
    huge.extend_from_slice(&1_u64.to_le_bytes());
    huge.push(GESTURE_NONE);
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode_edit_request(&huge),
        Err(DecodeError::InvalidLength(u32::MAX))
    ));
    let mut notice = vec![EDIT_NOTICE_TAG];
    notice.extend_from_slice(&[0; 16]);
    notice.push(GESTURE_NONE);
    notice.extend_from_slice(&(u32::try_from(MAX_EDIT_OP_BYTES).unwrap() + 1).to_le_bytes());
    assert!(matches!(
        decode_edit_notice(&notice),
        Err(DecodeError::InvalidLength(_))
    ));
    // A message claimed past its limit, and one that is not UTF-8.
    let mut reply = vec![EDIT_REPLY_TAG];
    reply.extend_from_slice(&[0; 8]);
    reply.extend_from_slice(&[OUTCOME_REFUSED, 1]);
    let mut long = reply.clone();
    long.extend_from_slice(&u16::MAX.to_le_bytes());
    assert!(matches!(
        decode_edit_reply(&long),
        Err(DecodeError::InvalidLength(_))
    ));
    reply.extend_from_slice(&1_u16.to_le_bytes());
    reply.push(0xFF);
    assert!(decode_edit_reply(&reply).is_err());
}

/// **Garbage never panics**: every prefix of every message, and every message
/// with each bit flipped in turn, through every decoder here — the unit half
/// of what the decoder fuzz target does with bytes it invents.
#[test]
fn every_prefix_and_every_flipped_bit_decodes_without_panicking() {
    for good in every_message() {
        for len in 0..=good.len() {
            decode_every_way(&good[..len]);
        }
        for bit in 0..good.len() * 8 {
            let mut flipped = good.clone();
            flipped[bit / 8] ^= 1 << (bit % 8);
            decode_every_way(&flipped);
        }
    }
}
