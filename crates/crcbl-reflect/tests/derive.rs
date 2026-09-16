//! `#[derive(Reflect)]` against the trait, on the shapes a component actually
//! has.
//!
//! An integration test rather than a unit one because the derive expands to
//! `::crcbl_reflect::…` paths, which only resolve from outside the crate — and
//! because what is being checked is the pair working together, not either half.
//! `crates/crcbl-reflect-derive/src/expand.rs` holds the tests about the tokens.

use crcbl_reflect::{Field, Kind, Range, Reflect, SetError, Value, ValueKind, get_path, set_path};

// ---------------------------------------------------------------------------
// The fixtures, which are the real components' shapes
// ---------------------------------------------------------------------------

/// `apps/breakout/src/scene.rs`'s `Brick`: two arrays of `f64`.
#[derive(Debug, PartialEq, Reflect)]
struct Brick {
    position: [f64; 3],
    #[reflect(name = "Half extents", min = 0.0, max = 32.0, step = 0.05)]
    half_extents: [f64; 3],
}

/// A struct inside a struct, which is what the panel recurses through.
#[derive(Debug, PartialEq, Reflect)]
struct Placement {
    label: String,
    brick: Brick,
    #[reflect(skip)]
    cached_volume: f64,
}

/// `apps/puppet/src/map.rs`'s `Shape`: a struct variant beside a tuple one and a
/// unit one.
#[derive(Debug, PartialEq, Reflect)]
enum Shape {
    Platform { width: f64, depth: f64 },
    Dome(f64),
    Flat,
}

/// The `crate = "…"` override, aimed at this crate under a path that is not the
/// default `::crcbl_reflect`.
#[derive(Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl_reflect")]
struct Redirected {
    value: u32,
}

/// A tuple struct, whose rows are named by their position.
#[derive(Debug, PartialEq, Reflect)]
struct Metres(f64, #[reflect(skip)] u8);

/// A unit struct: no rows at all, and still a `Kind::Struct`.
#[derive(Debug, PartialEq, Reflect)]
struct Marker;

fn brick() -> Brick {
    Brick {
        position: [1.0, 2.0, 3.0],
        half_extents: [0.5, 0.25, 0.75],
    }
}

// ---------------------------------------------------------------------------
// Object safety
// ---------------------------------------------------------------------------

/// The whole crate rests on this compiling: a panel holds a value it does not
/// know the type of.
fn as_object(value: &dyn Reflect) -> Kind {
    value.kind()
}

#[test]
fn a_derived_type_is_reachable_through_dyn_reflect() {
    assert_eq!(as_object(&brick()), Kind::Struct);
    assert_eq!(as_object(&Shape::Flat), Kind::Enum);
    assert_eq!(as_object(&1.5_f64), Kind::Leaf(ValueKind::Float));
    assert_eq!(as_object(&[1.0_f64, 2.0]), Kind::List { len: 2 });
}

// ---------------------------------------------------------------------------
// A struct
// ---------------------------------------------------------------------------

#[test]
fn a_struct_reports_its_short_name_and_one_row_per_field() {
    let brick = brick();
    assert_eq!(brick.type_name(), "Brick");
    assert_eq!(brick.kind(), Kind::Struct);
    assert_eq!(brick.variant(), None, "a struct has no variant");
    assert_eq!(brick.get(), None, "a struct is not a leaf");
    assert_eq!(
        brick.fields(),
        &[
            Field::new("position"),
            Field {
                name: "half_extents",
                label: "Half extents",
                range: Some(Range {
                    min: 0.0,
                    max: 32.0
                }),
                step: Some(0.05),
            },
        ]
    );
}

#[test]
fn a_structs_rows_are_reachable_by_index_in_the_order_fields_lists_them() {
    let mut brick = brick();
    assert_eq!(brick.field(0).map(Reflect::type_name), Some("[f64; 3]"));
    assert_eq!(brick.field(1).map(Reflect::type_name), Some("[f64; 3]"));
    assert!(brick.field(2).is_none(), "there is no third row");
    assert!(brick.field_mut(2).is_none(), "nor for a write");

    let element = brick
        .field_mut(0)
        .and_then(|row| row.field_mut(2))
        .expect("the third element of the position");
    element
        .set(&Value::Float(-4.0))
        .expect("an f64 takes a float");
    assert_eq!(brick.position, [1.0, 2.0, -4.0]);
}

#[test]
fn writing_a_value_into_a_struct_is_refused_by_naming_it() {
    let mut brick = brick();
    assert_eq!(
        brick.set(&Value::Float(1.0)),
        Err(SetError::NotALeaf { type_name: "Brick" })
    );
}

#[test]
fn a_derived_value_downcasts_through_as_any_for_a_per_type_override() {
    let mut brick = brick();
    let object: &dyn Reflect = &brick;
    assert_eq!(
        object.as_any().downcast_ref::<Brick>().map(|b| b.position),
        Some([1.0, 2.0, 3.0])
    );
    assert!(
        object.as_any().downcast_ref::<Placement>().is_none(),
        "a different type is not this one"
    );

    let object: &mut dyn Reflect = &mut brick;
    object
        .as_any_mut()
        .downcast_mut::<Brick>()
        .expect("the same type")
        .position[0] = 9.0;
    assert_eq!(brick.position[0], 9.0);
}

// ---------------------------------------------------------------------------
// A nested struct, and a skipped field
// ---------------------------------------------------------------------------

#[test]
fn a_nested_struct_is_walked_through_by_path() {
    let mut placement = Placement {
        label: "corner".to_owned(),
        brick: brick(),
        cached_volume: 0.1875,
    };

    assert_eq!(
        get_path(&placement, "brick.half_extents.1"),
        Ok(Value::Float(0.25))
    );
    assert!(
        set_path(&mut placement, "brick", &Value::Float(0.0)).is_err(),
        "a path that stops on a struct writes nothing"
    );

    set_path(&mut placement, "brick.half_extents.1", &Value::Float(2.5))
        .expect("the leaf is an f64");
    assert_eq!(placement.brick.half_extents, [0.5, 2.5, 0.75]);

    set_path(&mut placement, "label", &Value::Text("edge".to_owned()))
        .expect("the leaf is a String");
    assert_eq!(placement.label, "edge");
}

#[test]
fn a_skipped_field_has_no_row_no_index_and_no_path() {
    let placement = Placement {
        label: "corner".to_owned(),
        brick: brick(),
        cached_volume: 0.1875,
    };

    let names: Vec<_> = placement.fields().iter().map(|row| row.name).collect();
    assert_eq!(names, ["label", "brick"], "`cached_volume` is skipped");
    assert!(
        placement.field(2).is_none(),
        "the skipped field is not the third row either"
    );
    assert!(matches!(
        get_path(&placement, "cached_volume"),
        Err(crcbl_reflect::PathError::NoField { .. })
    ));
}

// ---------------------------------------------------------------------------
// An enum
// ---------------------------------------------------------------------------

#[test]
fn an_enum_describes_the_active_variant_and_only_it() {
    let platform = Shape::Platform {
        width: 4.0,
        depth: 2.0,
    };
    assert_eq!(platform.kind(), Kind::Enum);
    assert_eq!(platform.variant(), Some("Platform"));
    assert_eq!(
        platform
            .fields()
            .iter()
            .map(|row| row.name)
            .collect::<Vec<_>>(),
        ["width", "depth"]
    );
    assert_eq!(get_path(&platform, "width"), Ok(Value::Float(4.0)));

    let dome = Shape::Dome(6.0);
    assert_eq!(dome.variant(), Some("Dome"));
    assert_eq!(
        dome.fields().iter().map(|row| row.name).collect::<Vec<_>>(),
        ["0"],
        "a tuple variant's row is named by its position"
    );
    assert_eq!(get_path(&dome, "0"), Ok(Value::Float(6.0)));
    assert!(
        get_path(&dome, "width").is_err(),
        "`width` belongs to the other variant"
    );

    let flat = Shape::Flat;
    assert_eq!(flat.variant(), Some("Flat"));
    assert!(flat.fields().is_empty());
    assert!(flat.field(0).is_none());
}

#[test]
fn an_enums_active_variant_is_written_through_like_any_other_field() {
    let mut shape = Shape::Platform {
        width: 4.0,
        depth: 2.0,
    };
    set_path(&mut shape, "depth", &Value::Float(7.25)).expect("the leaf is an f64");
    assert_eq!(
        shape,
        Shape::Platform {
            width: 4.0,
            depth: 7.25
        }
    );
}

// ---------------------------------------------------------------------------
// The remaining shapes
// ---------------------------------------------------------------------------

#[test]
fn a_tuple_struct_names_its_rows_by_position_and_still_skips() {
    let mut metres = Metres(3.0, 7);
    assert_eq!(metres.kind(), Kind::Struct);
    assert_eq!(
        metres
            .fields()
            .iter()
            .map(|row| row.name)
            .collect::<Vec<_>>(),
        ["0"],
        "the second field carries `skip`"
    );
    set_path(&mut metres, "0", &Value::Float(4.5)).expect("the leaf is an f64");
    assert_eq!(metres, Metres(4.5, 7));
}

#[test]
fn a_unit_struct_is_a_struct_with_no_rows() {
    let marker = Marker;
    assert_eq!(marker.kind(), Kind::Struct);
    assert!(marker.fields().is_empty());
    assert!(marker.field(0).is_none());
    assert_eq!(marker.variant(), None);
}

#[test]
fn the_crate_attribute_produces_a_working_impl() {
    let mut redirected = Redirected { value: 7 };
    assert_eq!(get_path(&redirected, "value"), Ok(Value::UInt(7)));
    set_path(&mut redirected, "value", &Value::UInt(9)).expect("the leaf is a u32");
    assert_eq!(redirected, Redirected { value: 9 });
}

// ---------------------------------------------------------------------------
// Every primitive, read then written back
// ---------------------------------------------------------------------------

/// Reads a value out of one instance and writes it into a different one, then
/// reads it back.
///
/// Starting from `Default::default()` is what makes the write observable: a
/// round trip that began at the value it was checking would pass without the
/// write happening at all.
macro_rules! round_trip {
    ($($name:ident: $ty:ty = $value:expr, $kind:expr);* $(;)?) => {
        $(
            #[test]
            fn $name() {
                let original: $ty = $value;
                assert_eq!(Reflect::kind(&original), Kind::Leaf($kind));
                assert_eq!(Reflect::type_name(&original), stringify!($ty));

                let read = Reflect::get(&original).expect("a primitive is a leaf");
                assert_eq!(read.kind(), $kind);

                let mut target = <$ty>::default();
                assert_ne!(target, original, "the fixture must differ from the default");
                Reflect::set(&mut target, &read).expect("a leaf takes its own value");
                assert_eq!(target, original);
                assert_eq!(Reflect::get(&target), Some(read));
            }
        )*
    };
}

round_trip! {
    a_bool_round_trips: bool = true, ValueKind::Bool;
    an_i8_round_trips: i8 = -12, ValueKind::Int;
    an_i16_round_trips: i16 = -300, ValueKind::Int;
    an_i32_round_trips: i32 = -70_000, ValueKind::Int;
    an_i64_round_trips: i64 = -5_000_000_000, ValueKind::Int;
    an_isize_round_trips: isize = -1234, ValueKind::Int;
    a_u8_round_trips: u8 = 200, ValueKind::UInt;
    a_u16_round_trips: u16 = 40_000, ValueKind::UInt;
    a_u32_round_trips: u32 = 3_000_000_000, ValueKind::UInt;
    a_u64_round_trips: u64 = 18_000_000_000_000_000_000, ValueKind::UInt;
    a_usize_round_trips: usize = 1234, ValueKind::UInt;
    an_f32_round_trips: f32 = -2.5, ValueKind::Float;
    an_f64_round_trips: f64 = 1.0e300, ValueKind::Float;
    a_string_round_trips: String = "a b".to_owned(), ValueKind::Text;
}

#[test]
fn an_f64_survives_the_leaf_where_an_f32_would_round_it() {
    // The rounding `apps/breakout/src/scene.rs` warns about: a board written as
    // `f32` moves. Computed rather than spelled, so the claim is about the two
    // widths and not about a literal someone typed.
    let original = 0.1_f64 + 0.7_f64;
    assert_ne!(
        f64::from(original as f32).to_bits(),
        original.to_bits(),
        "the fixture must be a number f32 cannot hold, or this proves nothing"
    );

    let mut target = 0.0_f64;
    Reflect::set(&mut target, &Reflect::get(&original).expect("a leaf"))
        .expect("an f64 takes an f64");
    assert_eq!(target.to_bits(), original.to_bits());
}

// ---------------------------------------------------------------------------
// The refusals
// ---------------------------------------------------------------------------

#[test]
fn a_value_of_the_wrong_kind_is_refused_and_changes_nothing() {
    let mut value = 3_i32;
    assert_eq!(
        Reflect::set(&mut value, &Value::Text("nine".to_owned())),
        Err(SetError::Kind {
            type_name: "i32",
            expected: ValueKind::Int,
            actual: ValueKind::Text,
        })
    );
    assert_eq!(value, 3);

    let mut flag = true;
    assert!(Reflect::set(&mut flag, &Value::Int(1)).is_err());
    assert!(flag, "a refused write leaves the flag alone");
}

#[test]
fn an_integer_too_wide_for_its_leaf_is_refused_rather_than_truncated() {
    let mut narrow = 3_i8;
    assert_eq!(
        Reflect::set(&mut narrow, &Value::Int(300)),
        Err(SetError::Range {
            type_name: "i8",
            value: Value::Int(300),
        })
    );
    assert_eq!(narrow, 3, "not 44, which is 300 truncated to eight bits");

    let mut unsigned = 3_u32;
    assert!(Reflect::set(&mut unsigned, &Value::UInt(u64::MAX)).is_err());
    assert_eq!(unsigned, 3);
}

#[test]
fn a_float_past_f32s_range_is_refused_rather_than_saturated_to_infinity() {
    let mut narrow = 1.0_f32;
    assert_eq!(
        Reflect::set(&mut narrow, &Value::Float(1.0e300)),
        Err(SetError::Range {
            type_name: "f32",
            value: Value::Float(1.0e300),
        })
    );
    assert_eq!(narrow, 1.0, "not `inf`, which is the `as` cast's answer");
}

#[test]
fn a_non_finite_float_is_refused_by_both_float_leaves() {
    // `assert_eq!` cannot say this: `NaN != NaN`, so a derived `PartialEq` over
    // a struct holding one never reports equal.
    let mut wide = 1.0_f64;
    let refusal = Reflect::set(&mut wide, &Value::Float(f64::NAN));
    assert!(
        matches!(
            refusal,
            Err(SetError::NotFinite {
                type_name: "f64",
                value,
            }) if value.is_nan()
        ),
        "{refusal:?}"
    );
    assert_eq!(wide, 1.0);

    // `NotFinite` specifically, not merely an error: without the finite guard
    // an infinity narrows to `f32::INFINITY` and the saturation guard below it
    // refuses the same write as `Range`. An `is_err()` here would pass with the
    // guard deleted, which a sabotage is what found.
    let mut narrow = 1.0_f32;
    let refusal = Reflect::set(&mut narrow, &Value::Float(f64::INFINITY));
    assert_eq!(
        refusal,
        Err(SetError::NotFinite {
            type_name: "f32",
            value: f64::INFINITY,
        })
    );
    assert_eq!(narrow, 1.0);
}

// ---------------------------------------------------------------------------
// The engine's own value types
// ---------------------------------------------------------------------------

#[test]
fn a_glam_vector_is_a_struct_of_its_components() {
    let mut vector = glam::Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(vector.type_name(), "Vec3");
    assert_eq!(vector.kind(), Kind::Struct);
    assert_eq!(
        vector
            .fields()
            .iter()
            .map(|row| row.name)
            .collect::<Vec<_>>(),
        ["x", "y", "z"]
    );
    set_path(&mut vector, "z", &Value::Float(-1.5)).expect("a component is an f32");
    assert_eq!(vector, glam::Vec3::new(1.0, 2.0, -1.5));
}

#[test]
fn a_glam_quaternion_of_f64_is_reachable_component_by_component() {
    let mut rotation = glam::DQuat::IDENTITY;
    assert_eq!(
        rotation
            .fields()
            .iter()
            .map(|row| row.name)
            .collect::<Vec<_>>(),
        ["x", "y", "z", "w"]
    );
    assert_eq!(get_path(&rotation, "w"), Ok(Value::Float(1.0)));
    set_path(&mut rotation, "x", &Value::Float(0.5)).expect("a component is an f64");
    assert_eq!(rotation.x, 0.5);
}

// ---------------------------------------------------------------------------
// The undo shape the whole crate exists for
// ---------------------------------------------------------------------------

#[test]
fn an_edit_recorded_as_a_path_and_a_value_undoes_exactly() {
    let mut brick = brick();
    let before = brick.clone_state();

    // The command: this path, this new value, and what it replaced.
    let path = "half_extents.0";
    let old = get_path(&brick, path).expect("the leaf is an f64");
    set_path(&mut brick, path, &Value::Float(4.0)).expect("an f64 takes a float");
    assert_ne!(brick.clone_state(), before, "the edit landed");

    // Its inverse, which is the same call with the value it recorded.
    set_path(&mut brick, path, &old).expect("the old value was admissible");
    assert_eq!(brick.clone_state(), before);
}

/// A cheap stand-in for `World::hash_state`: the brick's numbers, in order.
trait State {
    fn clone_state(&self) -> Vec<f64>;
}

impl State for Brick {
    fn clone_state(&self) -> Vec<f64> {
        self.position
            .iter()
            .chain(&self.half_extents)
            .copied()
            .collect()
    }
}
