//! A turned block in the document: its rotation saved and read back, and the
//! box a ray picks it by turned with it.

use super::*;

use crcbl::math::DQuat;
use crcbl::registry::Rotation;

use crate::scene::BLOCKS;

/// The middle step: `(0, 0.75, 0)`, half extents `(1.2, 0.75, 1.5)`.
const STEP: SceneEntityId = SceneEntityId(2);

/// The ground slab under every step.
const GROUND: SceneEntityId = SceneEntityId(0);

/// An eighth of a turn about `+Y`.
fn eighth_turn() -> DQuat {
    DQuat::from_rotation_y(std::f64::consts::FRAC_PI_4)
}

/// Sets `id`'s block rotation to `turn` as one command, the way the rotate
/// handle writes it: all four leaves in one batch.
pub(crate) fn turn_block(document: &mut Document, id: SceneEntityId, turn: DQuat) {
    let commands = Rotation::LEAVES
        .into_iter()
        .zip(turn.to_array())
        .map(|(leaf, value)| EditCommand::SetProperty {
            entity: id,
            system: BLOCKS.to_owned(),
            path: format!("rotation.{leaf}"),
            value: Value::Float(value),
        })
        .collect();
    document
        .apply(EditCommand::Batch(commands))
        .expect("a block has a rotation");
}

/// A ray straight down onto `(x, z)` from well above every step.
fn down_onto(x: f64, z: f64) -> Ray {
    Ray::new(DVec3::new(x, 10.0, z), DVec3::NEG_Y)
}

/// **A turned block saves its rotation and reopens turned**, and its row is
/// the only one that writes one: the unturned blocks' rows are what they
/// were.
#[test]
fn a_turned_block_saves_its_rotation_and_reopens_turned() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    let before = document.files().expect("the scene saves");
    turn_block(&mut document, STEP, eighth_turn());

    let files = document.files().expect("the scene saves");
    let blocks = &files["sys/blocks.ron"];
    assert_eq!(blocks.matches("rotation:").count(), 1, "{blocks}");
    let [x, y, z, w] = eighth_turn().to_array();
    assert!(
        blocks.contains(&format!("rotation: ({x:?}, {y:?}, {z:?}, {w:?}),")),
        "{blocks}"
    );

    let source = memory_source(files.clone()).expect("scene keys");
    let mut reopened =
        Document::open(&source, Path::new(""), crate::scene::vocabulary()).expect("it reopens");
    assert_eq!(
        reopened.placement(STEP),
        document.placement(STEP),
        "the reopened step is turned differently",
    );
    assert_eq!(reopened.files().expect("it saves"), files);

    document.undo().expect("the turn undoes");
    assert_eq!(document.files().expect("the scene saves"), before);
}

/// **A turned block picks by its turned box**: a ray past the unturned box's
/// side that the turned box's corner swings under picks the block, and a ray
/// inside the unturned box's corner that the turn swung away from passes it
/// for the ground below.
#[test]
fn a_turned_block_picks_by_its_turned_box() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    // Outside the unturned footprint (x > 1.2), inside the turned one.
    let swung_in = down_onto(1.7, 0.2);
    // Inside the unturned footprint's corner, outside the turned one.
    let swung_out = down_onto(1.1, 1.4);
    assert_eq!(document.pick(&swung_in), Some(GROUND));
    assert_eq!(document.pick(&swung_out), Some(STEP));

    turn_block(&mut document, STEP, eighth_turn());
    assert_eq!(
        document.pick(&swung_in),
        Some(STEP),
        "the corner swung under"
    );
    assert_eq!(
        document.pick(&swung_out),
        Some(GROUND),
        "the corner swung away"
    );

    document.undo().expect("the turn undoes");
    assert_eq!(document.pick(&swung_in), Some(GROUND));
    assert_eq!(document.pick(&swung_out), Some(STEP));
}

/// **A turned block's bounds hold its turned corners**, which reach further
/// than its own half extents along the world's X and Z.
#[test]
fn a_turned_blocks_bounds_hold_its_corners() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    turn_block(&mut document, STEP, eighth_turn());
    let placement = document.placement(STEP).expect("placed");
    assert_eq!(placement.rotation, eighth_turn());
    let (min, max) = document.bounds(STEP).expect("placed");
    let reach = std::f32::consts::FRAC_1_SQRT_2 * (1.2 + 1.5);
    assert!((max.x - reach).abs() < 1e-5, "{max}");
    assert!((min.z + reach).abs() < 1e-5, "{min}");
    assert!((max.y - 1.5).abs() < 1e-6, "turning about Y moved its top");
}

/// Writes `value` into `id`'s block leaf `leaf` straight through
/// [`Document::component`], as a panel does before it reports the write —
/// the one way a value reaches the world without a command.
fn write_behind(document: &mut Document, id: SceneEntityId, leaf: &str, value: f64) {
    let block = document.component(id, BLOCKS).expect("a block");
    set_path(block, leaf, &Value::Float(value)).expect("a rotation leaf");
}

/// **A property write that leaves a rotation off unit is refused and put
/// back**, as a command or as a pasted field, naming the entity, system and
/// field; one that leaves it unit — a sign flip of the identity's `w` — and
/// a write elsewhere in the component are not.
#[test]
fn a_single_leaf_write_off_unit_is_refused_and_put_back() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    let before = document.files().expect("the scene saves");
    let error = document
        .apply(EditCommand::SetProperty {
            entity: STEP,
            system: BLOCKS.to_owned(),
            path: "rotation.w".to_owned(),
            value: Value::Float(0.5),
        })
        .expect_err("half a turn's length is no rotation");
    assert!(
        matches!(&error, EditError::Rotation { entity, system, field, .. }
            if *entity == STEP && system == BLOCKS && field == "rotation"),
        "{error}"
    );
    let error = document
        .paste_field(STEP, BLOCKS, "rotation.x", "0.7071")
        .expect_err("one leaf of four");
    assert!(matches!(error, EditError::Rotation { .. }), "{error}");
    assert_eq!(document.files().expect("the scene saves"), before);
    assert!(document.log().is_empty(), "a refused write was recorded");

    document
        .paste_field(STEP, BLOCKS, "rotation.w", "-1.0")
        .expect("(0, 0, 0, -1) is a rotation");
    document
        .paste_field(STEP, BLOCKS, "position.0", "2.0")
        .expect("a position is no rotation");
    assert_eq!(document.log().len(), 2);
}

/// **A rotation off unit that reached the world without a command is a
/// problem naming its entity, system and field**, for a block and for a
/// mesh alike, and none is reported while every rotation is unit.
#[test]
fn a_rotation_off_unit_is_a_problem_by_entity_and_field() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    assert!(document.rotation_problems().is_empty());
    write_behind(&mut document, STEP, "rotation.w", 0.5);
    let problems = document.problems().expect("the scene saves");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("entity #2: `blocks`'s `rotation`: ")
            && problems[0].contains("length 0.5"),
        "{problems:?}"
    );

    let mut props = super::mesh_tests::props();
    assert!(props.rotation_problems().is_empty());
    let mesh = props
        .component(super::mesh_tests::TRIANGLE_MESH, crcbl::scene_mesh::MESHES)
        .expect("a mesh");
    set_path(mesh, "rotation.y", &Value::Float(3.0)).expect("a rotation leaf");
    let problems = props.rotation_problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("entity #1: `meshes`'s `rotation`: "),
        "{problems:?}"
    );
}
