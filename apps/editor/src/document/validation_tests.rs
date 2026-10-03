//! A component's rule held at every door: a body's mass refused at the edit,
//! on load and reported at the save; a block's half extents refused; a
//! write no rule covers taken; and a batch judged whole.

use super::*;

use crcbl::scene_physics::BODIES;

use crate::scene::BLOCKS;

/// The middle step of the compiled-in scene, which each test gives a body.
const STEP: SceneEntityId = SceneEntityId(2);

/// The compiled-in scene with a new body — one kilogram — attached to
/// [`STEP`], as the inspector's add button attaches it.
fn with_body() -> Document {
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    document.attach(STEP, BODIES).expect("the step has no body");
    document
}

/// [`STEP`]'s mass.
fn mass(document: &mut Document) -> Value {
    document.read(STEP, BODIES, "mass").expect("a body's mass")
}

/// Writes `value` into [`STEP`]'s body at `path` straight through
/// [`Document::component`], as a panel does before it reports the write.
fn write_behind(document: &mut Document, path: &str, value: f64) {
    let body = document.component(STEP, BODIES).expect("a body");
    set_path(body, path, &Value::Float(value)).expect("a body's leaf");
}

/// Whether `error` refuses [`STEP`]'s body for its `field`.
fn refuses(error: &EditError, field: &str) -> bool {
    matches!(error, EditError::Invalid { entity, system, error }
        if *entity == STEP && system == BODIES && error.field == field)
}

/// **A mass of zero or less written through the inspector is refused and put
/// back**: the panel's write is rewound, nothing is recorded, the files are
/// what they were, and the refusal names the entity, the system and the field
/// — what the status line shows. A mass above zero is taken.
#[test]
fn a_bodys_mass_written_through_the_inspector_is_refused_and_put_back() {
    let mut document = with_body();
    let saved = document.files().expect("the scene saves");
    for refused in [0.0, -1.0] {
        let before = mass(&mut document);
        write_behind(&mut document, "mass", refused);
        let error = document
            .record_edits(
                STEP,
                BODIES,
                &[FieldEdit {
                    path: "mass".to_owned(),
                    before: before.clone(),
                    after: Value::Float(refused),
                }],
                &[],
                None,
            )
            .expect_err("no simulation takes that mass");
        assert!(refuses(&error, "mass"), "{error}");
        let text = error.to_string();
        assert!(
            text.contains(&format!("entity {STEP}")) && text.contains("`bodies`"),
            "{text}"
        );
        assert_eq!(mass(&mut document), before, "the write was not put back");
        assert_eq!(document.files().expect("the scene saves"), saved);
        assert_eq!(document.log().len(), 1, "a refused write was recorded");
    }

    document
        .apply(EditCommand::SetProperty {
            entity: STEP,
            system: BODIES.to_owned(),
            path: "mass".to_owned(),
            value: Value::Float(2.5),
        })
        .expect("a mass above zero");
    assert_eq!(mass(&mut document), Value::Float(2.5));
}

/// **The same mass in a file is refused on load**, naming the chunk and the
/// field, as the edit was.
#[test]
fn a_bodys_mass_in_a_file_is_refused_on_load_by_name() {
    let mut files = with_body().files().expect("the scene saves");
    let bodies = files.get_mut("sys/bodies.ron").expect("a bodies chunk");
    assert!(bodies.contains("mass: 1.0,"), "{bodies}");
    *bodies = bodies.replace("mass: 1.0,", "mass: 0.0,");
    let source = memory_source(files).expect("scene keys");
    let error = Document::open(&source, Path::new(""), crate::scene::vocabulary())
        .expect_err("a body of no mass");
    assert!(
        matches!(&error, EditError::Scene(ScnError::Parse { key, message, .. })
            if key == "sys/bodies.ron" && message.contains("`mass`")),
        "{error}"
    );
}

/// **A mass written without a command is reported at the save**, by the
/// chunk's file and line, as the next load would refuse it — and nothing is
/// reported before it.
#[test]
fn a_mass_written_outside_commands_is_reported_by_problems() {
    let mut document = with_body();
    assert!(document.problems().expect("it saves").is_empty());
    write_behind(&mut document, "mass", 0.0);
    let problems = document.problems().expect("it saves");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("`sys/bodies.ron` line ") && problems[0].contains("`mass`"),
        "{problems:?}"
    );
}

/// **A write no rule covers is taken**: puppet's sun has a rule over its
/// elevation, its period and every number being finite, and none over the
/// sign of its intensity — so an intensity below zero is applied and
/// recorded.
#[test]
fn a_write_no_rule_covers_is_taken() {
    use super::systems_tests::{SUN, two_systems};

    /// The sun alone in [`two_systems`].
    const LONE_SUN: SceneEntityId = SceneEntityId(5);

    let mut document = two_systems();
    document
        .apply(EditCommand::SetProperty {
            entity: LONE_SUN,
            system: SUN.to_owned(),
            path: "intensity".to_owned(),
            value: Value::Float(-100.0),
        })
        .expect("a sun has no rule over its intensity");
    assert_eq!(
        document
            .read(LONE_SUN, SUN, "intensity")
            .expect("an intensity"),
        Value::Float(-100.0),
    );
    assert_eq!(document.log().len(), 1);
}

/// **A block's half extent below zero is refused**, naming the axis — a box
/// collider refuses one, and the picking collider is rebuilt from it after
/// every edit — and one of zero is taken.
#[test]
fn a_blocks_half_extent_below_zero_is_refused() {
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    let saved = document.files().expect("the scene saves");
    let error = document
        .apply(write(BLOCKS, "half_extents.1", -5.0))
        .expect_err("no box has a negative half extent");
    assert!(
        matches!(&error, EditError::Invalid { entity, system, error }
            if *entity == STEP && system == BLOCKS && error.field == "half_extents.1"),
        "{error}"
    );
    assert_eq!(document.files().expect("the scene saves"), saved);
    document
        .apply(write(BLOCKS, "half_extents.1", 0.0))
        .expect("a flat block is a box");
}

/// One property write of [`STEP`]'s, as a batch member.
fn write(system: &str, path: &str, value: f64) -> EditCommand {
    EditCommand::SetProperty {
        entity: STEP,
        system: system.to_owned(),
        path: path.to_owned(),
        value: Value::Float(value),
    }
}

/// **A batch is judged on every member, and one refused member puts the
/// whole batch back** — the members before it and after it, in another
/// system included — wherever in the batch the refused write stands.
#[test]
fn a_batch_validates_every_member_and_rewinds_whole_on_one_failure() {
    let mut document = with_body();
    let saved = document.files().expect("the scene saves");
    let moved = || write(BLOCKS, "position.0", 3.0);
    let rougher = || write(BODIES, "friction", 0.2);
    let massless = || write(BODIES, "mass", 0.0);
    for batch in [
        vec![moved(), rougher(), massless()],
        vec![massless(), moved(), rougher()],
        vec![moved(), massless(), rougher()],
    ] {
        let error = document
            .apply(EditCommand::Batch(batch))
            .expect_err("one member leaves a body of no mass");
        assert!(refuses(&error, "mass"), "{error}");
        assert_eq!(document.files().expect("the scene saves"), saved);
        assert_eq!(document.log().len(), 1, "a refused batch was recorded");
    }
    document
        .apply(EditCommand::Batch(vec![moved(), rougher()]))
        .expect("every member is a value a scene may hold");
    assert_eq!(document.log().len(), 2);
}
