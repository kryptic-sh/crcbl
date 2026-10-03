//! The rotation's rules: what a file may hold, what it is read back as, and
//! which value is left out.

use glam::DVec3;

use super::*;

/// **A quaternion within [`ROTATION_TOLERANCE`] of unit is kept exactly as
/// written**, so a hand-typed one saves back as itself, and its orientation
/// is read normalised.
#[test]
fn a_nearly_unit_quaternion_is_kept_as_written_and_read_normalised() {
    let typed = [0.0, 0.6, 0.0, 0.80003];
    let rotation = Rotation::try_from(typed).expect("a hand-typed length within the tolerance");
    assert_eq!(rotation.to_array(), typed, "the numbers were rewritten");
    let quat = rotation.quat();
    assert!((quat.length() - 1.0).abs() < 1e-15, "{quat:?} is not unit");
    let turned = quat * DVec3::X;
    assert!(
        turned.abs_diff_eq(DVec3::new(0.28, 0.0, -0.96), 1e-4),
        "a turn of (0, 0.6, 0, 0.8) about +Y took +X to {turned}",
    );
}

/// **A length further than the tolerance from one is refused by name**, on
/// either side of it — not normalised into numbers nobody wrote.
#[test]
fn a_quaternion_off_unit_by_more_than_the_tolerance_is_refused() {
    for numbers in [
        [0.0, 0.0, 0.0, 1.0 + 2.0 * ROTATION_TOLERANCE],
        [0.0, 0.0, 0.0, 1.0 - 2.0 * ROTATION_TOLERANCE],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0, 1.0],
    ] {
        let error = Rotation::try_from(numbers).expect_err("off unit");
        assert!(
            matches!(error, RotationError::NotUnit { .. }),
            "{numbers:?}: {error:?}"
        );
        assert!(error.to_string().contains("unit quaternion"), "{error}");
    }
    let edge = [0.0, 0.0, 0.0, 1.0 + 0.5 * ROTATION_TOLERANCE];
    assert!(Rotation::try_from(edge).is_ok(), "inside the tolerance");
}

/// **A NaN or an infinity is refused by name**, before its length is asked.
#[test]
fn a_quaternion_with_a_number_that_is_not_finite_is_refused() {
    for numbers in [
        [f64::NAN, 0.0, 0.0, 1.0],
        [0.0, f64::INFINITY, 0.0, 1.0],
        [0.0, 0.0, f64::NEG_INFINITY, 1.0],
    ] {
        let error = Rotation::try_from(numbers).expect_err("not finite");
        assert!(
            matches!(error, RotationError::NotFinite(_)),
            "{numbers:?}: {error:?}"
        );
        assert!(error.to_string().contains("finite"), "{error}");
    }
}

/// **Only the identity itself is left out**: `w = -1` turns nothing too and
/// is still written, so what is omitted is what [`Default`] reads back.
#[test]
fn only_the_exact_identity_is_the_identity() {
    assert!(Rotation::default().is_identity());
    assert_eq!(Rotation::default(), Rotation::IDENTITY);
    let negated = Rotation::try_from([0.0, 0.0, 0.0, -1.0]).expect("unit");
    assert!(!negated.is_identity());
    let turned = Rotation::new(DQuat::from_rotation_y(0.5)).expect("unit");
    assert!(!turned.is_identity());
}

/// **A negative zero is not the identity**, so a row writes it and reads it
/// back as the same bits — a row that left it out would read back `+0.0`,
/// which is what an undone delete in the editor once restored.
#[test]
fn a_negative_zero_is_written_and_reads_back_as_itself() {
    let signed = Rotation::try_from([-0.0, 0.0, 0.0, 1.0]).expect("unit");
    assert_eq!(signed, Rotation::IDENTITY, "`==` cannot tell them apart");
    assert!(!signed.is_identity());
    let text = ron::to_string(&signed).expect("four numbers");
    let back: Rotation = ron::from_str(&text).expect("its own text");
    assert_eq!(
        back.to_array().map(f64::to_bits),
        signed.to_array().map(f64::to_bits),
        "{text} did not read back as the bits written",
    );
}

/// **A value a reflected write left off unit still reads as a rotation**: its
/// direction, normalised — and all four zero, which has none, as the
/// identity. `check` reports both, as a load would.
#[test]
fn a_reflected_write_off_unit_reads_as_its_direction() {
    let mut rotation = Rotation::IDENTITY;
    crcbl_reflect::set_path(&mut rotation, "w", &crcbl_reflect::Value::Float(3.0))
        .expect("w is a leaf");
    assert_eq!(rotation.quat(), DQuat::IDENTITY);
    assert!(matches!(
        rotation.check(),
        Err(RotationError::NotUnit { .. })
    ));
    crcbl_reflect::set_path(&mut rotation, "w", &crcbl_reflect::Value::Float(0.0))
        .expect("w is a leaf");
    assert_eq!(rotation.quat(), DQuat::IDENTITY, "zero has no direction");
    crcbl_reflect::set_path(&mut rotation, "y", &crcbl_reflect::Value::Float(-2.0))
        .expect("y is a leaf");
    assert!(
        rotation
            .quat()
            .abs_diff_eq(DQuat::from_xyzw(0.0, -1.0, 0.0, 0.0), 0.0)
    );
}

/// The reflected leaves are the four a path names, in the file's order.
#[test]
fn the_leaves_are_x_y_z_w() {
    let rotation = Rotation::new(DQuat::from_xyzw(0.5, 0.5, 0.5, 0.5)).expect("unit");
    let names: Vec<&str> = rotation.fields().iter().map(|field| field.name).collect();
    assert_eq!(names, Rotation::LEAVES);
}

/// A component carrying rotations at the top, nested, and in a list.
#[derive(Default, Reflect)]
#[reflect(crate = "crcbl_reflect")]
struct Rig {
    rotation: Rotation,
    arm: Arm,
    joints: [Rotation; 2],
}

#[derive(Default, Reflect)]
#[reflect(crate = "crcbl_reflect")]
struct Arm {
    length: f64,
    rotation: Rotation,
}

/// **The walk finds a rotation off unit wherever a component holds it**, by
/// its path, the first in field order first — and nothing in a component
/// whose rotations are all unit.
#[test]
fn the_walk_finds_every_rotation_off_unit_by_its_path() {
    use crcbl_reflect::{Value, set_path};

    let mut rig = Rig::default();
    assert_eq!(fault_in(&rig), None);
    for path in ["rotation.w", "arm.rotation.x", "joints.1.y"] {
        set_path(&mut rig, path, &Value::Float(2.0)).expect("a leaf");
    }
    for (fixed, field) in [
        ("rotation.w", "rotation"),
        ("arm.rotation.x", "arm.rotation"),
        ("joints.1.y", "joints.1"),
    ] {
        let fault = fault_in(&rig).expect("a rotation off unit");
        assert_eq!(fault.field, field);
        set_path(
            &mut rig,
            fixed,
            &Value::Float(f64::from(u8::from(fixed.ends_with('w')))),
        )
        .expect("a leaf");
    }
    assert_eq!(fault_in(&rig), None, "every rotation is unit again");
    set_path(&mut rig, "arm.rotation.x", &Value::Float(2.0)).expect("a leaf");
    assert!(
        fault_in(&rig.arm)
            .expect("off unit")
            .message
            .contains(&format!("length {}", 5.0_f64.sqrt())),
    );
}
