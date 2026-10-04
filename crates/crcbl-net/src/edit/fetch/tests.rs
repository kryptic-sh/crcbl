use super::*;

fn files() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("scene.ron".to_owned(), "Scene(name: \"one\")".to_owned()),
        (
            "sys/blocks.ron".to_owned(),
            "Chunk(system: \"blocks\")".to_owned(),
        ),
    ])
}

/// A scene whose files take `parts` parts and a little over, so the last is
/// short.
fn large_files(parts: usize) -> BTreeMap<String, String> {
    let mut files = files();
    files.insert(
        "sys/large.ron".to_owned(),
        "x".repeat(parts * MAX_SCENE_PART_BYTES),
    );
    files
}

/// Every part of `scene` at `revision`, in order.
fn parts(revision: u64, scene: &[u8]) -> Vec<ScenePart> {
    (0..)
        .map_while(|index| scene_part(revision, scene, index))
        .collect()
}

fn refused() -> SceneReply {
    SceneReply {
        fetch_id: 4,
        outcome: SceneOutcome::Refused {
            reason: EditRefusal::BUSY,
            message: "a fetch is already in flight".to_owned(),
        },
    }
}

/// Every message a fetch travels as, encoded: what the sweeps below damage.
fn every_message() -> Vec<Vec<u8>> {
    let scene = encode_scene_files(&files()).expect("small");
    let part = SceneReply {
        fetch_id: 3,
        outcome: SceneOutcome::Part(scene_part(9, &scene, 0).expect("one part")),
    };
    vec![
        encode_scene_fetch(3),
        encode_scene_reply(&part).expect("a part always encodes"),
        encode_scene_reply(&refused()).expect("short enough"),
        scene,
    ]
}

/// Every decoder of this module over `bytes`, and an assembly of whatever
/// part they decode to.
fn decode_every_way(bytes: &[u8]) {
    let _ = decode_scene_fetch(bytes);
    let _ = decode_scene_files(bytes);
    if let Ok(SceneReply {
        outcome: SceneOutcome::Part(part),
        ..
    }) = decode_scene_reply(bytes)
    {
        let _ = SceneAssembly::new(0).push(part);
    }
}

#[test]
fn a_fetch_is_the_kind_byte_then_the_id() {
    let encoded = encode_scene_fetch(0x0102_0304_0506_0708);
    let mut expected = vec![SCENE_FETCH_KIND];
    expected.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
    assert_eq!(encoded, expected);
    assert_eq!(
        decode_scene_fetch(&encoded).expect("well formed"),
        0x0102_0304_0506_0708
    );
}

#[test]
fn a_part_and_a_refusal_round_trip() {
    let scene = encode_scene_files(&files()).expect("small");
    let part = SceneReply {
        fetch_id: 3,
        outcome: SceneOutcome::Part(scene_part(9, &scene, 0).expect("one part")),
    };
    for reply in [part, refused()] {
        let encoded = encode_scene_reply(&reply).expect("encodes");
        assert_eq!(encoded[0], SCENE_REPLY_TAG);
        assert_eq!(decode_scene_reply(&encoded).expect("well formed"), reply);
    }
    assert_eq!(decode_scene_files(&scene).expect("well formed"), files());
}

/// **A scene larger than one message arrives whole**: cut into parts, each
/// sent as its own message and decoded, and joined back into the very files
/// it was cut from.
#[test]
fn a_scene_past_one_message_is_cut_into_parts_and_joined_whole() {
    let files = large_files(5);
    let scene = encode_scene_files(&files).expect("well under the limit");
    assert!(scene.len() > crate::MAX_IN_MEMORY_MESSAGE_BYTES);
    let cut = parts(11, &scene);
    assert_eq!(cut.len(), scene.len().div_ceil(MAX_SCENE_PART_BYTES));
    assert!(
        cut[..cut.len() - 1]
            .iter()
            .all(|part| part.bytes().len() == MAX_SCENE_PART_BYTES)
    );
    let mut assembly = SceneAssembly::new(2);
    let mut fetched = None;
    for (index, part) in cut.into_iter().enumerate() {
        let reply = SceneReply {
            fetch_id: 2,
            outcome: SceneOutcome::Part(part),
        };
        let encoded = encode_scene_reply(&reply).expect("a part always encodes");
        assert!(encoded.len() + AUTH_OVERHEAD <= crate::MAX_IN_MEMORY_MESSAGE_BYTES);
        let SceneOutcome::Part(part) = decode_scene_reply(&encoded).expect("whole").outcome else {
            panic!("a part");
        };
        assert!(fetched.is_none(), "the scene was whole before part {index}");
        fetched = assembly.push(part).expect("in order");
    }
    assert_eq!(
        fetched.expect("the last part completes it"),
        FetchedScene {
            revision: 11,
            files
        }
    );
}

#[test]
fn a_part_past_the_last_or_of_no_scene_is_not_cut() {
    let scene = encode_scene_files(&files()).expect("small");
    assert!(scene_part(1, &scene, 1).is_none());
    assert!(scene_part(1, &[], 0).is_none());
    assert!(scene_part(1, &vec![0; MAX_SCENE_BYTES + 1], 0).is_none());
    // The largest scene's last part is the last the part count allows.
    let index = u32::try_from(MAX_SCENE_PARTS - 1).expect("a few hundred");
    assert_eq!(
        part_len(u32::try_from(MAX_SCENE_BYTES).expect("fits"), index),
        Some(MAX_SCENE_PART_BYTES)
    );
    assert_eq!(
        part_len(u32::try_from(MAX_SCENE_BYTES).expect("fits"), index + 1),
        None
    );
}

#[test]
fn an_encoder_refuses_files_past_each_limit() {
    let many: BTreeMap<String, String> = (0..=MAX_SCENE_FILES)
        .map(|n| (format!("sys/{n:05}.ron"), String::new()))
        .collect();
    assert_eq!(
        encode_scene_files(&many).expect_err("too many").what,
        "file count"
    );
    let long_path = BTreeMap::from([("p".repeat(MAX_SCENE_PATH_BYTES + 1), String::new())]);
    assert_eq!(
        encode_scene_files(&long_path).expect_err("too long").what,
        "path"
    );
    let large = BTreeMap::from([("big.ron".to_owned(), "x".repeat(MAX_SCENE_BYTES))]);
    let error = encode_scene_files(&large).expect_err("too large");
    assert_eq!((error.what, error.limit), ("scene", MAX_SCENE_BYTES));

    // At the limits, each round-trips.
    let at_limit = BTreeMap::from([("p".repeat(MAX_SCENE_PATH_BYTES), "t".to_owned())]);
    let encoded = encode_scene_files(&at_limit).expect("at the limit");
    assert_eq!(
        decode_scene_files(&encoded).expect("at the limit"),
        at_limit
    );
    let mut fill = String::new();
    let overhead = 4 + 2 + "big.ron".len() + 4;
    fill.push_str(&"x".repeat(MAX_SCENE_BYTES - overhead));
    let at_limit = BTreeMap::from([("big.ron".to_owned(), fill)]);
    let encoded = encode_scene_files(&at_limit).expect("at the limit");
    assert_eq!(encoded.len(), MAX_SCENE_BYTES);
    assert_eq!(
        decode_scene_files(&encoded).expect("at the limit"),
        at_limit
    );
}

#[test]
fn a_decoder_refuses_every_truncation_trailing_byte_and_other_tag() {
    for good in every_message() {
        for len in 0..good.len() {
            let cut = &good[..len];
            assert!(
                decode_scene_fetch(cut).is_err()
                    && decode_scene_reply(cut).is_err()
                    && decode_scene_files(cut).is_err(),
                "{len} of {} bytes decoded",
                good.len()
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(
            decode_scene_fetch(&trailing).is_err()
                && decode_scene_reply(&trailing).is_err()
                && decode_scene_files(&trailing).is_err(),
            "a trailing byte was ignored"
        );
    }
    let mut other = encode_scene_fetch(1);
    other[0] = super::super::EDIT_KIND;
    assert!(matches!(
        decode_scene_fetch(&other),
        Err(DecodeError::UnknownTag { tag }) if tag == super::super::EDIT_KIND
    ));
    let mut outcome = encode_scene_reply(&refused()).expect("short enough");
    outcome[9] = 7;
    assert!(matches!(
        decode_scene_reply(&outcome),
        Err(DecodeError::UnknownTag { tag: 7 })
    ));
}

/// The part header a decoder reads before any of a part's bytes, with no
/// bytes behind it.
fn part_header(total_len: u32, index: u32, part_len: u32) -> Vec<u8> {
    let mut header = vec![SCENE_REPLY_TAG];
    header.extend_from_slice(&1_u64.to_le_bytes());
    header.push(OUTCOME_PART);
    header.extend_from_slice(&1_u64.to_le_bytes());
    header.extend_from_slice(&total_len.to_le_bytes());
    header.extend_from_slice(&index.to_le_bytes());
    header.extend_from_slice(&part_len.to_le_bytes());
    header
}

/// **A part's length is checked against its index and the whole before
/// anything is read for it**: a whole of nothing or past the limit, an
/// index past the last part, and a part length other than the one the index
/// makes are each refused by length, with no bytes behind the claim.
#[test]
fn a_part_whose_lengths_disagree_is_refused_before_its_bytes_are_read() {
    let max = u32::try_from(MAX_SCENE_BYTES).expect("fits");
    let full = u32::try_from(MAX_SCENE_PART_BYTES).expect("fits");
    let cases = [
        (part_header(0, 0, 0), 0),
        (part_header(max + 1, 0, full), max + 1),
        (part_header(10, 1, 0), 10),
        (part_header(full + 1, 0, full + 1), full + 1),
        (part_header(full + 1, 1, 2), 2),
        (part_header(10, 0, u32::MAX), u32::MAX),
    ];
    for (bytes, len) in cases {
        assert!(
            matches!(decode_scene_reply(&bytes), Err(DecodeError::InvalidLength(got)) if got == len),
            "{bytes:?}"
        );
    }
}

/// The files' own framing: keys repeated or out of order, a count past the
/// limit with nothing behind it, a key past its limit, and text that is not
/// UTF-8.
#[test]
fn files_out_of_order_or_past_their_limits_are_refused() {
    let entry = |path: &str, text: &[u8]| {
        let mut bytes = u16::try_from(path.len())
            .expect("short")
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(path.as_bytes());
        bytes.extend_from_slice(&u32::try_from(text.len()).expect("short").to_le_bytes());
        bytes.extend_from_slice(text);
        bytes
    };
    let scene = |entries: &[Vec<u8>]| {
        let mut bytes = u32::try_from(entries.len())
            .expect("few")
            .to_le_bytes()
            .to_vec();
        for entry in entries {
            bytes.extend_from_slice(entry);
        }
        bytes
    };
    for (first, second) in [("b", "a"), ("a", "a")] {
        assert!(matches!(
            decode_scene_files(&scene(&[entry(first, b""), entry(second, b"")])),
            Err(SceneFilesError::Unordered { path }) if path == second
        ));
    }
    assert!(
        decode_scene_files(&scene(&[entry("a", b""), entry("b", b"")])).is_ok(),
        "ascending keys are a scene"
    );
    let count = u32::try_from(MAX_SCENE_FILES + 1).expect("fits");
    assert!(matches!(
        decode_scene_files(&count.to_le_bytes()),
        Err(SceneFilesError::Wire(DecodeError::InvalidLength(got))) if got == count
    ));
    let long = "p".repeat(MAX_SCENE_PATH_BYTES + 1);
    assert!(matches!(
        decode_scene_files(&scene(&[entry(&long, b"")])),
        Err(SceneFilesError::Wire(DecodeError::InvalidLength(_)))
    ));
    assert!(decode_scene_files(&scene(&[entry("a", &[0xFF])])).is_err());
    assert!(decode_scene_files(&vec![0; MAX_SCENE_BYTES + 1]).is_err());
}

/// **An assembly takes parts strictly in order and from one scene**: a part
/// skipped, repeated, or naming another revision or length than the first
/// spends it, and parts that join into something that is not files are
/// refused as such.
#[test]
fn an_assembly_refuses_a_part_out_of_order_or_from_another_scene() {
    let scene = encode_scene_files(&large_files(2)).expect("small");
    let cut = parts(5, &scene);
    assert_eq!(cut.len(), 3);

    let mut skipped = SceneAssembly::new(1);
    assert!(matches!(
        skipped.push(cut[1].clone()),
        Err(SceneAssemblyError::OutOfOrder {
            expected: 0,
            got: 1
        })
    ));
    let mut repeated = SceneAssembly::new(1);
    assert!(repeated.push(cut[0].clone()).expect("first").is_none());
    assert!(matches!(
        repeated.push(cut[0].clone()),
        Err(SceneAssemblyError::OutOfOrder {
            expected: 1,
            got: 0
        })
    ));

    let other_revision = parts(6, &scene);
    let mut changed = SceneAssembly::new(1);
    changed.push(cut[0].clone()).expect("first");
    assert!(matches!(
        changed.push(other_revision[1].clone()),
        Err(SceneAssemblyError::Changed)
    ));
    let longer = encode_scene_files(&large_files(3)).expect("small");
    let mut changed = SceneAssembly::new(1);
    changed.push(cut[0].clone()).expect("first");
    assert!(matches!(
        changed.push(parts(5, &longer)[1].clone()),
        Err(SceneAssemblyError::Changed)
    ));

    let garbage = vec![0xEE; 40];
    let mut not_files = SceneAssembly::new(1);
    assert!(matches!(
        not_files.push(scene_part(5, &garbage, 0).expect("one part")),
        Err(SceneAssemblyError::Files(_))
    ));
    assert_eq!(not_files.received(), garbage.len());
}

/// **Garbage never panics**: every prefix of every message, and every message
/// with each bit flipped in turn, through every decoder here and an assembly
/// of any part that still decodes.
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
