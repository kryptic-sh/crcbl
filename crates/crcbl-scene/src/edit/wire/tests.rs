use super::*;

fn name(text: &str) -> EntityName {
    EntityName::new(text).expect("a name")
}

/// One of every command that travels, and a batch nesting two of them.
fn every_command() -> Vec<EditCommand> {
    let leaves = vec![
        EditCommand::SetProperty {
            entity: SceneEntityId(7),
            system: "blocks".to_owned(),
            path: "position.1".to_owned(),
            value: Value::Float(-0.0),
        },
        EditCommand::SetProperty {
            entity: SceneEntityId(7),
            system: "blocks".to_owned(),
            path: "label".to_owned(),
            value: Value::Text("ünïcode".to_owned()),
        },
        EditCommand::SetProperty {
            entity: SceneEntityId(u32::MAX),
            system: "bodies".to_owned(),
            path: "awake".to_owned(),
            value: Value::Bool(true),
        },
        EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: "s".to_owned(),
            path: "n".to_owned(),
            value: Value::Int(i64::MIN),
        },
        EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: "s".to_owned(),
            path: "u".to_owned(),
            value: Value::UInt(u64::MAX),
        },
        EditCommand::Spawn {
            entity: SceneEntityId(9),
            rows: vec![
                SystemRow {
                    system: "blocks".to_owned(),
                    row: "Block(position: (1.0, 2.0, 3.0))".to_owned(),
                },
                SystemRow {
                    system: "sun".to_owned(),
                    row: String::new(),
                },
            ],
            name: Some(name("Gate")),
        },
        EditCommand::Spawn {
            entity: SceneEntityId(10),
            rows: Vec::new(),
            name: None,
        },
        EditCommand::Delete {
            entity: SceneEntityId(3),
        },
        EditCommand::Attach {
            entity: SceneEntityId(3),
            system: "sun".to_owned(),
            row: "Sun()".to_owned(),
        },
        EditCommand::Detach {
            entity: SceneEntityId(3),
            system: "sun".to_owned(),
        },
        EditCommand::ListSystem {
            system: "meshes".to_owned(),
            at: 2,
        },
        EditCommand::UnlistSystem {
            system: "meshes".to_owned(),
        },
        EditCommand::Rename {
            entity: SceneEntityId(3),
            name: Some(name("Step")),
        },
        EditCommand::Rename {
            entity: SceneEntityId(3),
            name: None,
        },
        EditCommand::SetEnvironment {
            path: "camera.1".to_owned(),
            value: Value::Float(32.0),
        },
        EditCommand::SetEnvironment {
            path: String::new(),
            value: Value::Text(String::new()),
        },
        EditCommand::SetVariant {
            entity: SceneEntityId(4),
            system: "props".to_owned(),
            path: "mount".to_owned(),
            value: Snapshot::Variant {
                name: "Fixed".into(),
                fields: vec![
                    Snapshot::Leaf(Value::Float(-0.0)),
                    Snapshot::Variant {
                        name: "Platform".into(),
                        fields: vec![
                            Snapshot::Leaf(Value::Text("deck".to_owned())),
                            Snapshot::Fields(vec![
                                Snapshot::Leaf(Value::UInt(3)),
                                Snapshot::Leaf(Value::Bool(false)),
                            ]),
                        ],
                    },
                ],
            },
        },
        EditCommand::SetVariant {
            entity: SceneEntityId(0),
            system: "bodies".to_owned(),
            path: "kind".to_owned(),
            value: Snapshot::Variant {
                name: "Kinematic".into(),
                fields: Vec::new(),
            },
        },
    ];
    let offsets = [
        EditCommand::OffsetProperty {
            entity: SceneEntityId(7),
            system: "blocks".to_owned(),
            path: "position.0".to_owned(),
            by: Value::Float(-0.25),
        },
        EditCommand::OffsetProperty {
            entity: SceneEntityId(u32::MAX),
            system: String::new(),
            path: String::new(),
            by: Value::Int(i64::MIN),
        },
    ];
    let nested = EditCommand::Batch(vec![
        leaves[7].clone(),
        EditCommand::Batch(vec![leaves[10].clone(), leaves[8].clone()]),
        EditCommand::Batch(Vec::new()),
    ]);
    let mut all = leaves;
    all.extend(offsets);
    all.push(nested);
    all
}

fn every_op() -> Vec<EditOp> {
    let mut ops: Vec<EditOp> = every_command().into_iter().map(EditOp::Apply).collect();
    ops.extend(every_command().into_iter().map(EditOp::ApplyFresh));
    ops.push(EditOp::Undo);
    ops.push(EditOp::Redo);
    ops
}

fn encoded(op: &EditOp) -> Vec<u8> {
    encode_op(op).expect("every op here travels")
}

/// `depth` batches, each holding the next, around one delete.
fn nested(depth: usize) -> EditCommand {
    (0..depth).fold(
        EditCommand::Delete {
            entity: SceneEntityId(1),
        },
        |inner, _| EditCommand::Batch(vec![inner]),
    )
}

#[test]
fn every_op_round_trips_to_the_bit() {
    for op in every_op() {
        let bytes = encoded(&op);
        assert_eq!(bytes[0], WIRE_VERSION);
        let decoded = decode_op(&bytes).expect("what the encoder wrote");
        assert_eq!(decoded, op);
    }
    // `==` calls -0.0 and 0.0 equal; the wire carries the bits.
    let Ok(EditOp::Apply(EditCommand::SetProperty {
        value: Value::Float(zero),
        ..
    })) = decode_op(&encoded(&EditOp::Apply(every_command().remove(0))))
    else {
        panic!("the first command is a float property");
    };
    assert_eq!(zero.to_bits(), (-0.0_f64).to_bits());
}

#[test]
fn a_delete_is_spelled_as_the_module_docs_say() {
    let bytes = encoded(&EditOp::Apply(EditCommand::Delete {
        entity: SceneEntityId(0x0102_0304),
    }));
    assert_eq!(bytes, [WIRE_VERSION, OP_APPLY, DELETE, 4, 3, 2, 1]);
    assert_eq!(encoded(&EditOp::Undo), [WIRE_VERSION, OP_UNDO]);
    assert_eq!(encoded(&EditOp::Redo), [WIRE_VERSION, OP_REDO]);
}

#[test]
fn an_offset_and_fresh_ids_are_spelled_as_the_module_docs_say() {
    let offset = EditCommand::OffsetProperty {
        entity: SceneEntityId(2),
        system: "b".to_owned(),
        path: "p".to_owned(),
        by: Value::Float(0.5),
    };
    let mut spelled = vec![WIRE_VERSION, OP_APPLY, OFFSET_PROPERTY, 2, 0, 0, 0];
    for text in ["b", "p"] {
        spelled.extend_from_slice(&1_u32.to_le_bytes());
        spelled.extend_from_slice(text.as_bytes());
    }
    spelled.push(VALUE_FLOAT);
    spelled.extend_from_slice(&0.5_f64.to_bits().to_le_bytes());
    assert_eq!(encoded(&EditOp::Apply(offset)), spelled);

    let delete = EditCommand::Delete {
        entity: SceneEntityId(1),
    };
    assert_eq!(
        encoded(&EditOp::ApplyFresh(delete)),
        [WIRE_VERSION, OP_APPLY_FRESH, DELETE, 1, 0, 0, 0]
    );
}

/// **What a server announces for an op is spelled as long as the op**: an
/// offset resolved into the set of a value of its own kind, and a command
/// asking for fresh ids applied under other ids — what lets the notice
/// carry whatever the request could.
#[test]
fn a_resolved_op_is_spelled_as_long_as_the_one_sent() {
    for (by, value) in [
        (Value::Float(0.5), Value::Float(-3.25)),
        (Value::Int(1), Value::Int(i64::MAX)),
        (Value::UInt(1), Value::UInt(7)),
    ] {
        let offset = EditCommand::OffsetProperty {
            entity: SceneEntityId(2),
            system: "blocks".to_owned(),
            path: "position.1".to_owned(),
            by,
        };
        let set = EditCommand::SetProperty {
            entity: SceneEntityId(2),
            system: "blocks".to_owned(),
            path: "position.1".to_owned(),
            value,
        };
        assert_eq!(
            encoded(&EditOp::Apply(offset)).len(),
            encoded(&EditOp::Apply(set)).len()
        );
    }
    for command in every_command() {
        let fresh = encoded(&EditOp::ApplyFresh(command.clone()));
        let given = command.map_entities(&mut |id| SceneEntityId(id.0 ^ u32::MAX));
        assert_eq!(fresh.len(), encoded(&EditOp::Apply(given)).len());
    }
}

#[test]
fn an_environment_write_is_spelled_as_the_module_docs_say() {
    let bytes = encoded(&EditOp::Apply(EditCommand::SetEnvironment {
        path: "ambient.0".to_owned(),
        value: Value::Float(0.17),
    }));
    let mut spelled = vec![WIRE_VERSION, OP_APPLY, SET_ENVIRONMENT];
    spelled.extend_from_slice(&9_u32.to_le_bytes());
    spelled.extend_from_slice(b"ambient.0");
    spelled.push(VALUE_FLOAT);
    spelled.extend_from_slice(&0.17_f64.to_bits().to_le_bytes());
    assert_eq!(bytes, spelled);
}

#[test]
fn a_variant_switch_is_spelled_as_the_module_docs_say() {
    let bytes = encoded(&EditOp::Apply(EditCommand::SetVariant {
        entity: SceneEntityId(2),
        system: "b".to_owned(),
        path: "k".to_owned(),
        value: Snapshot::Variant {
            name: "On".into(),
            fields: vec![
                Snapshot::Leaf(Value::Bool(true)),
                Snapshot::Fields(Vec::new()),
            ],
        },
    }));
    let mut spelled = vec![WIRE_VERSION, OP_APPLY, SET_VARIANT, 2, 0, 0, 0];
    for text in ["b", "k"] {
        spelled.extend_from_slice(&1_u32.to_le_bytes());
        spelled.extend_from_slice(text.as_bytes());
    }
    spelled.push(SNAPSHOT_VARIANT);
    spelled.extend_from_slice(&2_u32.to_le_bytes());
    spelled.extend_from_slice(b"On");
    spelled.extend_from_slice(&2_u32.to_le_bytes());
    spelled.extend_from_slice(&[SNAPSHOT_LEAF, VALUE_BOOL, 1]);
    spelled.push(SNAPSHOT_FIELDS);
    spelled.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(bytes, spelled);
}

/// `depth` lists of fields, each holding the next, around one leaf.
fn nested_snapshot(depth: usize) -> Snapshot {
    (0..depth).fold(Snapshot::Leaf(Value::Bool(true)), |inner, _| {
        Snapshot::Fields(vec![inner])
    })
}

/// A switch writing `value` into entity 1's `kind` in `bodies`.
fn switch_of(value: Snapshot) -> EditOp {
    EditOp::Apply(EditCommand::SetVariant {
        entity: SceneEntityId(1),
        system: "bodies".to_owned(),
        path: "kind".to_owned(),
        value,
    })
}

#[test]
fn snapshots_nest_to_the_limit_and_no_further_either_way() {
    let deepest = switch_of(nested_snapshot(MAX_SNAPSHOT_DEPTH));
    assert_eq!(decode_op(&encoded(&deepest)), Ok(deepest));
    assert_eq!(
        encode_op(&switch_of(nested_snapshot(MAX_SNAPSHOT_DEPTH + 1))),
        Err(OpEncodeError::SnapshotTooDeep)
    );
    // Hand-spelled, since the encoder will not: one level past the limit.
    let mut bytes = encoded(&switch_of(Snapshot::Leaf(Value::Bool(true))));
    let leaf = bytes.split_off(bytes.len() - MIN_SNAPSHOT_BYTES);
    for _ in 0..=MAX_SNAPSHOT_DEPTH {
        bytes.push(SNAPSHOT_FIELDS);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&leaf);
    assert_eq!(decode_op(&bytes), Err(OpDecodeError::SnapshotTooDeep));
}

#[test]
fn batches_nest_to_the_limit_and_no_further_either_way() {
    let deepest = EditOp::Apply(nested(MAX_BATCH_DEPTH));
    assert_eq!(decode_op(&encoded(&deepest)), Ok(deepest));
    assert_eq!(
        encode_op(&EditOp::Apply(nested(MAX_BATCH_DEPTH + 1))),
        Err(OpEncodeError::TooDeep)
    );
    // Hand-spelled, since the encoder will not: one level past the limit.
    let mut bytes = vec![WIRE_VERSION, OP_APPLY];
    for _ in 0..=MAX_BATCH_DEPTH {
        bytes.push(BATCH);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&[DELETE, 1, 0, 0, 0]);
    assert_eq!(decode_op(&bytes), Err(OpDecodeError::TooDeep));
}

#[test]
fn a_decoder_names_what_is_wrong() {
    assert_eq!(
        decode_op(&[WIRE_VERSION + 1, OP_UNDO]),
        Err(OpDecodeError::Version {
            found: WIRE_VERSION + 1
        })
    );
    assert_eq!(
        decode_op(&[WIRE_VERSION, 9]),
        Err(OpDecodeError::UnknownOp(9))
    );
    assert_eq!(
        decode_op(&[WIRE_VERSION, OP_APPLY, 0x7F]),
        Err(OpDecodeError::UnknownCommand(0x7F))
    );
    assert_eq!(
        decode_op(&[WIRE_VERSION, OP_UNDO, 0]),
        Err(OpDecodeError::Trailing(1))
    );
    // A rename whose presence byte is neither 0 nor 1.
    assert_eq!(
        decode_op(&[WIRE_VERSION, OP_APPLY, RENAME, 1, 0, 0, 0, 2]),
        Err(OpDecodeError::NotABool {
            offset: 7,
            found: 2
        })
    );
    // A name that is not one: a line break in it.
    let mut bad_name = vec![WIRE_VERSION, OP_APPLY, RENAME, 1, 0, 0, 0, 1];
    bad_name.extend_from_slice(&3_u32.to_le_bytes());
    bad_name.extend_from_slice(b"a\nb");
    assert_eq!(
        decode_op(&bad_name),
        Err(OpDecodeError::Name(NameError::Control('\n')))
    );
    // Text that is not UTF-8.
    let mut bad_text = vec![WIRE_VERSION, OP_APPLY, UNLIST_SYSTEM];
    bad_text.extend_from_slice(&1_u32.to_le_bytes());
    bad_text.push(0xFF);
    assert_eq!(
        decode_op(&bad_text),
        Err(OpDecodeError::NotUtf8 { offset: 7 })
    );
    // An unknown value tag.
    let mut bad_value = vec![WIRE_VERSION, OP_APPLY, SET_PROPERTY, 0, 0, 0, 0];
    bad_value.extend_from_slice(&[0; 8]);
    bad_value.push(0x55);
    assert_eq!(
        decode_op(&bad_value),
        Err(OpDecodeError::UnknownValue(0x55))
    );
    // An unknown snapshot tag.
    let mut bad_snapshot = encoded(&switch_of(Snapshot::Leaf(Value::Bool(true))));
    let at = bad_snapshot.len() - MIN_SNAPSHOT_BYTES;
    bad_snapshot.truncate(at);
    bad_snapshot.push(0x66);
    assert_eq!(
        decode_op(&bad_snapshot),
        Err(OpDecodeError::UnknownSnapshot(0x66))
    );
}

#[test]
fn a_count_the_bytes_cannot_hold_is_refused_before_it_is_allocated() {
    let mut batch = vec![WIRE_VERSION, OP_APPLY, BATCH];
    batch.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_op(&batch),
        Err(OpDecodeError::Count {
            offset: 3,
            count: u32::MAX
        })
    );
    // Two rows claimed and room for one: a row is at least two lengths.
    let mut spawn = vec![WIRE_VERSION, OP_APPLY, SPAWN, 0, 0, 0, 0];
    spawn.extend_from_slice(&2_u32.to_le_bytes());
    spawn.extend_from_slice(&[0; MIN_ROW_BYTES + 1]);
    assert_eq!(
        decode_op(&spawn),
        Err(OpDecodeError::Count {
            offset: 7,
            count: 2
        })
    );
    // Two fields claimed and room for one: a snapshot is at least a leaf
    // holding a bool.
    let mut fields = encoded(&switch_of(Snapshot::Leaf(Value::Bool(true))));
    fields.truncate(fields.len() - MIN_SNAPSHOT_BYTES);
    let offset = fields.len() + 1;
    fields.push(SNAPSHOT_FIELDS);
    fields.extend_from_slice(&2_u32.to_le_bytes());
    fields.extend_from_slice(&[SNAPSHOT_LEAF, VALUE_BOOL, 1, 0]);
    assert_eq!(
        decode_op(&fields),
        Err(OpDecodeError::Count { offset, count: 2 })
    );
    // A text longer than the bytes left.
    let mut text = vec![WIRE_VERSION, OP_APPLY, UNLIST_SYSTEM];
    text.extend_from_slice(&5_u32.to_le_bytes());
    text.extend_from_slice(b"abc");
    assert_eq!(
        decode_op(&text),
        Err(OpDecodeError::Count {
            offset: 3,
            count: 5
        })
    );
}

/// **Garbage never panics**: every prefix of every op, and every op with each
/// bit flipped in turn — the unit half of what the decoder fuzz target does
/// with bytes it invents. Every proper prefix is refused, too: an op cut short
/// is never read as a shorter one.
#[test]
fn every_prefix_and_every_flipped_bit_decodes_without_panicking() {
    for op in every_op() {
        let bytes = encoded(&op);
        for len in 0..bytes.len() {
            assert!(
                decode_op(&bytes[..len]).is_err(),
                "{len} of {} bytes of {op:?} decoded",
                bytes.len()
            );
        }
        for bit in 0..bytes.len() * 8 {
            let mut flipped = bytes.clone();
            flipped[bit / 8] ^= 1 << (bit % 8);
            let _ = decode_op(&flipped);
        }
    }
}
