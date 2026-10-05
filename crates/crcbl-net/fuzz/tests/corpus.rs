//! Named seeds from the fuzzer's corpus, replayed through the delta decoder,
//! the replay spool's recovery, the save container's reader, the scene
//! edit's messages and the scene fetch's.
//!
//! `fuzz_targets/decoder.rs` runs the decoders against bytes libFuzzer invents.
//! This target runs three of them against bytes somebody named, and the two are
//! not the same job. A fuzzer finds a crashing or wrongly-accepted input once
//! and then moves on; nothing makes it generate that input again, so a fix that
//! regresses is a fix nobody notices. Pinning the seed here — with the exact
//! [`DeltaDecodeError`] it must produce, not merely "it did not panic" — is what
//! keeps that specific input failing forever if the decoder stops rejecting it.
//!
//! It is a `#[test]` rather than a second fuzz target because it needs no
//! fuzzing runtime at all: the seeds are `include_bytes!`d out of
//! `../corpus/decoder/`, so an ordinary `cargo test` replays them in
//! milliseconds with no nightly toolchain and no `cargo fuzz`. That matters
//! because `crates/crcbl-net/fuzz/Cargo.toml` declares its own `[workspace]`:
//! the repository's `cargo nextest run --workspace` sweep does not reach this
//! directory, so these tests have to be cheap enough to run on their own.
//!
//! Every delta seed is decoded at [`Trust::Untrusted`], which is the level they
//! were written to exercise.

use crcbl_net::{DeltaDecodeError, Trust, decode_delta};

/// The corpus is decoded at the hostile-input trust level, which is what the
/// seeds were written to exercise.
fn decode_delta_untrusted(payload: &[u8]) -> Result<crcbl_net::Delta, DeltaDecodeError> {
    decode_delta(payload, Trust::Untrusted)
}

#[test]
fn named_delta_seeds_reach_their_intended_paths() {
    let minimal = include_bytes!("../corpus/decoder/delta-keyframe-minimal");
    let decoded =
        decode_delta(minimal, Trust::Untrusted).expect("minimal keyframe seed must decode");
    assert!(decoded.is_keyframe);
    assert!(decoded.systems.is_empty());

    assert!(matches!(
        decode_delta_untrusted(include_bytes!("../corpus/decoder/delta-truncated")),
        Err(DeltaDecodeError::TooShort)
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!("../corpus/decoder/delta-trailing-byte")),
        Err(DeltaDecodeError::TrailingBytes(1))
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!(
            "../corpus/decoder/delta-hostile-system-count"
        )),
        Err(DeltaDecodeError::InvalidLength(u32::MAX))
    ));
    assert!(matches!(
        decode_delta_untrusted(include_bytes!(
            "../corpus/decoder/delta-hostile-entity-count"
        )),
        Err(DeltaDecodeError::InvalidLength(4_294_967_284))
    ));
}

#[test]
fn oversized_seed_crosses_the_decoder_limit() {
    let oversized = include_bytes!("../corpus/decoder/oversized-payload");
    assert_eq!(oversized.len(), 65_537);
    assert!(matches!(
        decode_delta(oversized, Trust::Untrusted),
        Err(DeltaDecodeError::InvalidLength(65_537))
    ));
}

/// The replay spool seeds reach both ends of a recovery: the whole spool
/// keeps every record, and the same spool three bytes short drops its last,
/// a state hash, as cut short.
#[test]
fn named_replay_spool_seeds_reach_their_intended_paths() {
    use crcbl_store::replay::{SpoolEnd, SpoolRecovery, recover_spool};

    let recover = |seed: &[u8]| -> SpoolRecovery {
        recover_spool(std::io::Cursor::new(seed), &mut std::io::sink())
            .expect("a spool with a whole header recovers")
    };
    let whole = recover(include_bytes!("../corpus/decoder/replay-spool"));
    assert_eq!(whole.end, SpoolEnd::Whole);
    assert_eq!(
        (whole.sim_sets, whole.state_hashes, whole.peer_ticks),
        (1, 2, 1)
    );
    assert_eq!(whole.dropped_bytes, 0);

    let torn = recover(include_bytes!("../corpus/decoder/replay-spool-torn"));
    assert_eq!(torn.end, SpoolEnd::CutShort);
    assert_eq!(
        (torn.sim_sets, torn.state_hashes, torn.peer_ticks),
        (1, 1, 1)
    );
    assert_eq!(
        torn.dropped_bytes, 22,
        "a 25-byte hash record, cut by three"
    );
}

/// The save seeds reach the container reader's three ends: a whole save opens
/// with every field, the same save a byte short is refused by both reads as cut
/// off inside its last sector's data, and the same save with one bit of that
/// data flipped is refused by the checked read and salvaged, flagged, by the
/// other. They go through `crcbl_net_fuzz::open_save`, the call the fuzz target
/// makes.
#[test]
fn named_save_seeds_reach_their_intended_paths() {
    use crcbl_net::types::SectorId;
    use crcbl_net_fuzz::open_save;
    use crcbl_store::save::{SaveData, SaveWriter};
    use crcbl_store::{MemoryStorage, StorageSource};

    let seed = include_bytes!("../corpus/decoder/save");
    let whole = open_save(seed);
    let read = whole.checked.expect("the whole save opens").into_data();
    assert!(read.checksum_valid);
    assert_eq!(read.header.tick.get(), 42);
    assert_eq!(read.header.playtime_secs.to_bits(), 120.5_f64.to_bits());
    let sectors: Vec<(SectorId, &[u8])> = read
        .sectors
        .iter()
        .map(|sector| (sector.sector_id, sector.snapshot_data.as_slice()))
        .collect();
    assert_eq!(
        sectors,
        [
            (SectorId::ZERO, &[1, 2, 3, 4][..]),
            (SectorId { x: 1, y: -2, z: 3 }, &[0xAA, 0xBB][..]),
        ]
    );
    assert!(
        whole
            .salvaged
            .expect("the salvage read opens it too")
            .data()
            .checksum_valid
    );
    // The seed is what the writer itself writes for what it holds, so a change
    // to the container's layout shows up here as a seed to regenerate rather
    // than as a corpus that quietly stopped being saves.
    let rewritten = {
        let SaveData {
            header, sectors, ..
        } = read;
        let mut writer = SaveWriter::new(header);
        for sector in sectors {
            writer.add_sector(sector);
        }
        let storage = MemoryStorage::new();
        let path = std::path::Path::new("rewritten.crb");
        writer
            .write(&storage, path)
            .expect("a memory storage takes every write");
        storage.read(path).expect("the file just written")
    };
    assert_eq!(rewritten, seed);

    let truncated = include_bytes!("../corpus/decoder/save-truncated");
    assert_eq!(truncated[..], seed[..seed.len() - 1]);
    let cut = open_save(truncated);
    for (read, which) in [(cut.checked, "checked"), (cut.salvaged, "salvage")] {
        let error = read.expect_err("a save a byte short was opened");
        assert!(
            error.to_string().contains("truncated in sector data"),
            "{which} read: {error}"
        );
    }

    let flipped = open_save(include_bytes!("../corpus/decoder/save-bit-flipped"));
    let error = flipped
        .checked
        .expect_err("a save with a flipped bit was opened");
    assert!(error.to_string().contains("checksum mismatch"), "{error}");
    let salvaged = flipped
        .salvaged
        .expect("the salvage read takes a save whose checksum fails")
        .into_data();
    assert!(!salvaged.checksum_valid);
    assert_eq!(salvaged.sectors[1].snapshot_data, [0xAA, 0xBA]);
}

/// The edit seeds reach each message's decoder, one per message an edit
/// travels as: a request and a notice, each with and without a gesture, both
/// outcomes of a reply, and the operation inside them — a whole one, an
/// environment write, a variant switch, an offset, a spawn asking for fresh
/// ids, one whose batches nest past the limit, and a request claiming an operation longer than any. Each whole
/// seed is also what its encoder writes, so a change to a layout shows up
/// here as a seed to regenerate rather than as a corpus that stopped being
/// edits.
#[test]
fn named_edit_seeds_reach_their_intended_paths() {
    use crcbl_net::{
        DecodeError, EditGesture, EditNotice, EditOutcome, EditRefusal, EditReply, EditRequest,
        decode_edit_notice, decode_edit_reply, decode_edit_request, encode_edit_notice,
        encode_edit_reply, encode_edit_request,
    };
    use crcbl_scene::edit::{
        EditCommand, EditOp, OpDecodeError, Snapshot, SystemRow, Value, decode_op, encode_op,
    };
    use crcbl_scene::scn::{EntityName, SceneEntityId};

    let delete = encode_op(&EditOp::Apply(EditCommand::Delete {
        entity: SceneEntityId(3),
    }))
    .expect("a delete travels");

    for (seed, request) in [
        (
            &include_bytes!("../corpus/decoder/edit-request")[..],
            EditRequest {
                request_id: 7,
                gesture: None,
                op: delete.clone(),
            },
        ),
        (
            &include_bytes!("../corpus/decoder/edit-request-gesture")[..],
            EditRequest {
                request_id: 8,
                gesture: Some(EditGesture { id: 3, last: true }),
                op: delete.clone(),
            },
        ),
    ] {
        assert_eq!(decode_edit_request(seed).expect("a whole request"), request);
        assert_eq!(encode_edit_request(&request).expect("short enough"), seed);
    }

    assert!(matches!(
        decode_edit_request(include_bytes!(
            "../corpus/decoder/edit-request-hostile-op-length"
        )),
        Err(DecodeError::InvalidLength(u32::MAX))
    ));

    for (seed, reply) in [
        (
            &include_bytes!("../corpus/decoder/edit-reply-applied")[..],
            EditReply {
                request_id: 7,
                outcome: EditOutcome::Applied { revision: 1 },
            },
        ),
        (
            &include_bytes!("../corpus/decoder/edit-reply-refused")[..],
            EditReply {
                request_id: 8,
                outcome: EditOutcome::Refused {
                    reason: EditRefusal::UNKNOWN_ENTITY,
                    message: "the scene holds no entity 999".to_owned(),
                },
            },
        ),
    ] {
        assert_eq!(decode_edit_reply(seed).expect("a whole reply"), reply);
        assert_eq!(encode_edit_reply(&reply).expect("short enough"), seed);
    }

    for (seed, notice) in [
        (
            &include_bytes!("../corpus/decoder/edit-notice")[..],
            EditNotice {
                revision: 1,
                author: 1,
                gesture: None,
                op: delete.clone(),
            },
        ),
        (
            &include_bytes!("../corpus/decoder/edit-notice-gesture")[..],
            EditNotice {
                revision: 2,
                author: 1,
                gesture: Some(5),
                op: delete.clone(),
            },
        ),
    ] {
        assert_eq!(decode_edit_notice(seed).expect("a whole notice"), notice);
        assert_eq!(encode_edit_notice(&notice).expect("short enough"), seed);
    }

    let seed = include_bytes!("../corpus/decoder/edit-op-batch");
    let op = EditOp::Apply(EditCommand::Batch(vec![
        EditCommand::Rename {
            entity: SceneEntityId(2),
            name: Some(EntityName::new("Gate").expect("a name")),
        },
        EditCommand::SetProperty {
            entity: SceneEntityId(1),
            system: "blocks".to_owned(),
            path: "position.1".to_owned(),
            value: Value::Float(2.5),
        },
    ]));
    assert_eq!(decode_op(seed).expect("a whole op"), op);
    assert_eq!(encode_op(&op).expect("it travels"), seed);

    let seed = include_bytes!("../corpus/decoder/edit-op-environment");
    let op = EditOp::Apply(EditCommand::SetEnvironment {
        path: "ambient.0".to_owned(),
        value: Value::Float(0.17),
    });
    assert_eq!(decode_op(seed).expect("a whole op"), op);
    assert_eq!(encode_op(&op).expect("it travels"), seed);

    let seed = include_bytes!("../corpus/decoder/edit-op-variant");
    let op = EditOp::Apply(EditCommand::SetVariant {
        entity: SceneEntityId(3),
        system: "bodies".to_owned(),
        path: "kind".to_owned(),
        value: Snapshot::Variant {
            name: "Platform".into(),
            fields: vec![
                Snapshot::Leaf(Value::Float(2.5)),
                Snapshot::Fields(vec![Snapshot::Leaf(Value::Bool(true))]),
            ],
        },
    });
    assert_eq!(decode_op(seed).expect("a whole op"), op);
    assert_eq!(encode_op(&op).expect("it travels"), seed);

    let seed = include_bytes!("../corpus/decoder/edit-op-offset");
    let op = EditOp::Apply(EditCommand::OffsetProperty {
        entity: SceneEntityId(1),
        system: "blocks".to_owned(),
        path: "position.0".to_owned(),
        by: Value::Float(0.01),
    });
    assert_eq!(decode_op(seed).expect("a whole op"), op);
    assert_eq!(encode_op(&op).expect("it travels"), seed);

    let seed = include_bytes!("../corpus/decoder/edit-op-fresh");
    let op = EditOp::ApplyFresh(EditCommand::Batch(vec![
        EditCommand::Spawn {
            entity: SceneEntityId(4),
            rows: vec![SystemRow {
                system: "blocks".to_owned(),
                row: "Block()".to_owned(),
            }],
            name: None,
        },
        EditCommand::Rename {
            entity: SceneEntityId(4),
            name: Some(EntityName::new("Gate").expect("a name")),
        },
    ]));
    assert_eq!(decode_op(seed).expect("a whole op"), op);
    assert_eq!(encode_op(&op).expect("it travels"), seed);

    assert_eq!(
        decode_op(include_bytes!("../corpus/decoder/edit-op-too-deep")),
        Err(OpDecodeError::TooDeep)
    );
}

/// The scene fetch's seeds reach each of its decoders, one per message a
/// fetch travels as: the fetch, both outcomes of a reply, the files a reply's
/// parts join into, and a part claiming a length its index does not make.
/// Each whole seed is also what its encoder writes, and the part's seed
/// assembles into the files' seed.
#[test]
fn named_scene_fetch_seeds_reach_their_intended_paths() {
    use std::collections::BTreeMap;

    use crcbl_net::edit::decode_scene_files;
    use crcbl_net::{
        DecodeError, EditRefusal, SceneAssembly, SceneOutcome, SceneReply, decode_scene_fetch,
        decode_scene_reply, encode_scene_fetch, encode_scene_files, encode_scene_reply, scene_part,
    };

    let seed = include_bytes!("../corpus/decoder/scene-fetch");
    assert_eq!(decode_scene_fetch(seed).expect("a whole fetch"), 7);
    assert_eq!(encode_scene_fetch(7), seed);

    let seed = include_bytes!("../corpus/decoder/scene-files");
    let files = BTreeMap::from([
        ("scene.ron".to_owned(), "Scene()".to_owned()),
        ("sys/blocks.ron".to_owned(), "Chunk()".to_owned()),
    ]);
    assert_eq!(decode_scene_files(seed).expect("whole files"), files);
    assert_eq!(encode_scene_files(&files).expect("small"), seed);

    let seed = include_bytes!("../corpus/decoder/scene-reply-part");
    let reply = SceneReply {
        fetch_id: 7,
        outcome: SceneOutcome::Part(
            scene_part(3, &encode_scene_files(&files).expect("small"), 0).expect("one part"),
        ),
    };
    let decoded = decode_scene_reply(seed).expect("a whole part");
    assert_eq!(decoded, reply);
    assert_eq!(
        encode_scene_reply(&reply).expect("a part always encodes"),
        seed
    );
    let SceneOutcome::Part(part) = decoded.outcome else {
        panic!("a part");
    };
    let scene = SceneAssembly::new(7)
        .push(part)
        .expect("in order")
        .expect("the one part is the whole");
    assert_eq!((scene.revision, scene.files), (3, files));

    let seed = include_bytes!("../corpus/decoder/scene-reply-refused");
    let reply = SceneReply {
        fetch_id: 8,
        outcome: SceneOutcome::Refused {
            reason: EditRefusal::BUSY,
            message: "a scene fetch is already in flight".to_owned(),
        },
    };
    assert_eq!(decode_scene_reply(seed).expect("a whole refusal"), reply);
    assert_eq!(encode_scene_reply(&reply).expect("short enough"), seed);

    assert!(matches!(
        decode_scene_reply(include_bytes!(
            "../corpus/decoder/scene-reply-hostile-part-length"
        )),
        Err(DecodeError::InvalidLength(u32::MAX))
    ));
}
