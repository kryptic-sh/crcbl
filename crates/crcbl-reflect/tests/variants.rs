//! An enum's variants listed, switched between and switched back: what
//! `#[derive(Reflect)]` writes for `Reflect::variants` and
//! `Reflect::set_variant`, and the `Snapshot` an undoable switch is put back
//! with.

use crcbl_reflect::{
    Field, PathError, Range, Reflect, SetError, Snapshot, Value, Variant, get_path, restore_path,
    set_variant_path, snapshot_path,
};

/// `apps/puppet/src/map.rs`'s `Shape`, with a tuple and a unit variant beside
/// it and a skipped field whose default is not zero. `Default` because it is a
/// field of [`Mount::Fixed`], which a switch to that variant makes.
#[derive(Debug, Default, PartialEq, Reflect)]
enum Shape {
    Platform {
        #[reflect(name = "Width", min = 0.0, max = 64.0, step = 0.1)]
        width: f64,
        depth: f64,
        #[reflect(skip)]
        cache: Cache,
    },
    Dome(f64),
    #[default]
    Flat,
}

/// A field type whose `Default` is not all zeroes, so a switch that zeroed
/// its fields instead of defaulting them would show.
#[derive(Debug, PartialEq)]
struct Cache(u8);

/// What a switch puts in a skipped [`Cache`].
const CACHE_DEFAULT: u8 = 7;

impl Default for Cache {
    fn default() -> Self {
        Self(CACHE_DEFAULT)
    }
}

/// An enum inside a variant, so a snapshot has a variant under a variant.
#[derive(Debug, PartialEq, Reflect)]
enum Mount {
    Fixed { position: [f64; 3], shape: Shape },
    Loose,
}

/// A component holding an enum, which a path reaches.
#[derive(Debug, PartialEq, Reflect)]
struct Prop {
    label: String,
    mount: Mount,
}

fn dome() -> Shape {
    Shape::Dome(6.0)
}

fn prop() -> Prop {
    Prop {
        label: "post".to_owned(),
        mount: Mount::Fixed {
            // `-0.0`, so a put-back compared by `==` would miss a lost sign.
            position: [1.25, -0.0, 3.5],
            shape: Shape::Platform {
                width: 2.0,
                depth: 0.1 + 0.2,
                cache: Cache(1),
            },
        },
    }
}

/// Every leaf of a [`Prop`] as bits, so "restored exactly" is a bit
/// comparison and not `==`, which calls the two zeros equal.
fn bits(prop: &Prop) -> (String, Vec<u64>, String, Vec<u64>, Option<u8>) {
    let Mount::Fixed { position, shape } = &prop.mount else {
        return (
            prop.label.clone(),
            Vec::new(),
            String::new(),
            Vec::new(),
            None,
        );
    };
    let (name, leaves, cache) = match shape {
        Shape::Platform {
            width,
            depth,
            cache,
        } => ("Platform", vec![*width, *depth], Some(cache.0)),
        Shape::Dome(radius) => ("Dome", vec![*radius], None),
        Shape::Flat => ("Flat", Vec::new(), None),
    };
    (
        prop.label.clone(),
        position.map(f64::to_bits).to_vec(),
        name.to_owned(),
        leaves.into_iter().map(f64::to_bits).collect(),
        cache,
    )
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// **Every variant is listed in declaration order, each with the rows it
/// would have** — the active one's are what `fields` answers, and a skipped
/// field is in neither.
#[test]
fn an_enum_lists_every_variant_with_its_rows_in_declaration_order() {
    let shape = dome();
    let variants = shape.variants();
    let names: Vec<_> = variants.iter().map(|variant| variant.name).collect();
    assert_eq!(names, ["Platform", "Dome", "Flat"]);

    const PLATFORM: Variant = Variant {
        name: "Platform",
        fields: &[
            Field {
                name: "width",
                label: "Width",
                range: Some(Range {
                    min: 0.0,
                    max: 64.0,
                }),
                step: Some(0.1),
            },
            Field::new("depth"),
        ],
    };
    assert_eq!(variants[0], PLATFORM);
    assert_eq!(variants[1].fields, &[Field::new("0")]);
    assert!(variants[2].fields.is_empty());
    assert_eq!(
        variants[1].fields,
        shape.fields(),
        "the active variant's rows are not the ones listed for it"
    );
}

/// A struct, a leaf and a list have no variants, and refuse every switch by
/// name — the trait's provided bodies.
#[test]
fn what_is_not_an_enum_lists_nothing_and_refuses_every_switch() {
    let mut label = "post".to_owned();
    assert!(label.variants().is_empty());
    assert_eq!(
        label.set_variant("Dome"),
        Err(SetError::NoVariant {
            type_name: "String",
            variant: "Dome".to_owned(),
        })
    );
    assert_eq!(label, "post");

    let mut prop = prop();
    assert!(prop.variants().is_empty());
    assert!(prop.set_variant("Loose").is_err());
    assert!(matches!(prop.mount, Mount::Fixed { .. }));
}

// ---------------------------------------------------------------------------
// Switching
// ---------------------------------------------------------------------------

/// **A switch makes the new variant with every field at its type's default**,
/// the skipped field included, and the rows follow it.
#[test]
fn a_switch_makes_the_new_variant_from_each_fields_default() {
    let mut shape = dome();
    shape
        .set_variant("Platform")
        .expect("the enum has the variant");
    assert_eq!(
        shape,
        Shape::Platform {
            width: 0.0,
            depth: 0.0,
            cache: Cache(CACHE_DEFAULT),
        }
    );
    assert_eq!(shape.variant(), Some("Platform"));
    assert_eq!(shape.fields().len(), 2);

    shape.set_variant("Flat").expect("a unit variant");
    assert_eq!(shape, Shape::Flat);
    shape.set_variant("Dome").expect("a tuple variant");
    assert_eq!(shape, Shape::Dome(0.0));
}

/// **A switch to the variant already active changes nothing**: a picker that
/// reports the active variant chosen again must not reset its fields.
#[test]
fn a_switch_to_the_active_variant_keeps_its_fields() {
    let mut shape = dome();
    shape.set_variant("Dome").expect("the active variant");
    assert_eq!(shape, dome());
}

/// **A name the enum does not have is refused**, naming the type and the
/// name, and the value is untouched.
#[test]
fn a_variant_the_enum_lacks_is_refused_and_changes_nothing() {
    let mut shape = dome();
    assert_eq!(
        shape.set_variant("Ramp"),
        Err(SetError::NoVariant {
            type_name: "Shape",
            variant: "Ramp".to_owned(),
        })
    );
    assert_eq!(shape, dome());
}

/// A switch reaches an enum through a path, and a path that names nothing
/// says which segment.
#[test]
fn a_switch_by_path_reaches_the_enum_it_names() {
    let mut prop = prop();
    set_variant_path(&mut prop, "mount.shape", "Flat").expect("a nested enum");
    assert!(matches!(
        prop.mount,
        Mount::Fixed {
            shape: Shape::Flat,
            ..
        }
    ));
    set_variant_path(&mut prop, "mount", "Loose").expect("the outer enum");
    assert_eq!(prop.mount, Mount::Loose);
    assert!(matches!(
        set_variant_path(&mut prop, "mount.shape", "Flat"),
        Err(PathError::NoField { .. })
    ));
    assert_eq!(
        set_variant_path(&mut prop, "label", "Flat"),
        Err(PathError::Set(SetError::NoVariant {
            type_name: "String",
            variant: "Flat".to_owned(),
        }))
    );
}

// ---------------------------------------------------------------------------
// The way back
// ---------------------------------------------------------------------------

/// **A snapshot read before a switch puts back the variant and every row
/// under it, bit for bit** — a nested enum's variant, a sign on a zero — and
/// one read after it puts the switch back again. The skipped field is no row,
/// so it comes back at its default, as any switch makes it.
#[test]
fn a_snapshot_is_the_exact_inverse_of_a_switch() {
    let mut prop = prop();
    let mut original = bits(&prop);
    let before = snapshot_path(&prop, "mount").expect("an enum");

    set_variant_path(&mut prop, "mount", "Loose").expect("a variant");
    let after = snapshot_path(&prop, "mount").expect("an enum");
    assert_ne!(bits(&prop), original);

    restore_path(&mut prop, "mount", &before).expect("its own snapshot fits");
    original.4 = Some(CACHE_DEFAULT);
    assert_eq!(bits(&prop), original, "the undo is not exact");

    restore_path(&mut prop, "mount", &after).expect("its own snapshot fits");
    assert_eq!(prop.mount, Mount::Loose, "the redo is not the switch");
}

/// The snapshot names the variant at each depth, and the skipped field is in
/// no snapshot — it is not a row — so a restore that switched variants leaves
/// it at its default, while one within the variant leaves it as it was.
#[test]
fn a_snapshot_holds_the_rows_and_tags_each_enum_by_its_variant() {
    let snapshot = Snapshot::of(&dome());
    assert_eq!(
        snapshot,
        Snapshot::Variant {
            name: "Dome".into(),
            fields: vec![Snapshot::Leaf(Value::Float(6.0))],
        }
    );

    let mut prop = prop();
    let Snapshot::Fields(fields) = Snapshot::of(&prop) else {
        panic!("a struct's snapshot is its fields");
    };
    assert_eq!(fields[0], Snapshot::Leaf(Value::Text("post".to_owned())));
    let Snapshot::Variant { name, fields } = &fields[1] else {
        panic!("an enum's snapshot is its variant");
    };
    assert_eq!(*name, "Fixed");
    assert!(
        matches!(&fields[1], Snapshot::Variant { name, fields } if name == "Platform" && fields.len() == 2)
    );

    let held = snapshot_path(&prop, "mount.shape").expect("an enum");
    restore_path(&mut prop, "mount.shape", &held).expect("over itself");
    assert_eq!(
        bits(&prop).4,
        Some(1),
        "a restore over itself reset a field"
    );
}

/// **A snapshot that does not fit is refused, and the value is put back** —
/// including when the misfit is found only after the restore had switched a
/// variant and written a leaf of it. A variant's row count is checked before
/// it is switched to, so that misfit leaves even the skipped field alone.
#[test]
fn a_snapshot_that_does_not_fit_is_refused_and_the_value_put_back() {
    let mut shape = dome();
    let misfit = Snapshot::Variant {
        name: "Platform".into(),
        fields: vec![
            Snapshot::Leaf(Value::Float(9.0)),
            Snapshot::Leaf(Value::Text("deep".to_owned())),
        ],
    };
    assert!(matches!(
        misfit.restore(&mut shape),
        Err(SetError::Kind { .. })
    ));
    assert_eq!(shape, dome(), "the half-written restore was not put back");

    let mut prop = prop();
    let original = bits(&prop);
    let brick = Snapshot::Fields(vec![Snapshot::Leaf(Value::Float(1.0))]);
    assert_eq!(
        restore_path(&mut prop, "mount", &brick),
        Err(PathError::Set(SetError::Shape { type_name: "Mount" }))
    );
    let too_many = Snapshot::Variant {
        name: "Dome".into(),
        fields: vec![
            Snapshot::Leaf(Value::Float(1.0)),
            Snapshot::Leaf(Value::Float(2.0)),
        ],
    };
    assert_eq!(
        restore_path(&mut prop, "mount.shape", &too_many),
        Err(PathError::Set(SetError::Shape { type_name: "Shape" }))
    );
    assert_eq!(bits(&prop), original);
}

/// A leaf snapshot is a leaf's value, and writes back like one.
#[test]
fn a_leafs_snapshot_is_its_value() {
    let mut prop = prop();
    let held = snapshot_path(&prop, "label").expect("a leaf");
    assert_eq!(held, Snapshot::Leaf(Value::Text("post".to_owned())));
    restore_path(
        &mut prop,
        "label",
        &Snapshot::Leaf(Value::Text("gate".to_owned())),
    )
    .expect("text into text");
    assert_eq!(get_path(&prop, "label"), Ok(Value::Text("gate".to_owned())));
}

/// **A value restored over itself changes nothing, a NaN included** — a leaf
/// already holding its snapshot's value is not written, so a component that
/// holds a value no write accepts can still be put back to itself, which is
/// what the put-back after a refusal does.
#[test]
fn a_value_holding_a_nan_restores_over_itself() {
    #[derive(Debug, Reflect)]
    struct Reading {
        value: f64,
    }

    let mut reading = Reading { value: f64::NAN };
    let held = Snapshot::of(&reading);
    held.restore(&mut reading)
        .expect("nothing to write, so nothing to refuse");
    assert!(reading.value.is_nan());
}
