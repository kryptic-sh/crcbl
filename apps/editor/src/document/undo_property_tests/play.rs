//! Each generated step played through the entry point the editor's UI calls
//! for it, with its draws resolved against the document as it stands.

use proptest::sample::Index;

use crcbl::math::DVec3;
use crcbl::reflect::{
    Kind, Reflect, Snapshot, Value, ValueKind, get_path, restore_path, set_path, set_variant_path,
    snapshot_path,
};
use crcbl::registry::Rotation;
use crcbl::scene::scn::{MAX_NAME_CHARS, SceneEntityId};
use crcbl::scene_mesh::MESHES;
use crcbl::ui::tree::{FieldEdit, VariantEdit};

use super::super::text_of;
use super::super::{Document, EditError};
use super::ops::{Draw, Op, Pair, Switch, TEXTS, Through};
use crate::command::EditCommand;

/// An id no entity in the test's document holds: a target drawn past the
/// held entities is this one, so every edit naming an entity is also tried
/// on one that is gone.
const ABSENT: SceneEntityId = SceneEntityId(9_999);

/// A variant no enum in the vocabulary has: what a [`Op::Switch`] drawn past
/// an enum's variants switches to, which the enum refuses.
const NO_SUCH_VARIANT: &str = "Floating";

/// What a step did to the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Outcome {
    /// Accepted, and one entry recorded.
    Recorded,
    /// Accepted, and nothing recorded or changed: a rename to the name the
    /// entity has, a nudge of entities nothing places.
    Unchanged,
    /// Refused by the document or the leaf: nothing changed or recorded.
    Refused,
    /// Nothing for the UI to act on — no entity to select, no component to
    /// draw — so the step was not played.
    Skipped,
    /// An undo or redo that moved the log.
    Walked,
    /// An undo at the bottom of the log, or a redo at the top.
    AtEnd,
    /// A save: the state the log stands at marked saved, nothing changed or
    /// recorded.
    Saved,
}

/// Facts about the steps played that the test asserts some history reached,
/// gathered as they happen.
pub(super) type Reached = Vec<&'static str>;

/// Plays `op` on `document` through the UI's entry point for it, noting in
/// `reached` any fact about it the test counts, and says what it did.
///
/// # Panics
///
/// If an undo or a redo is refused: every entry the log holds was applied
/// once and walked back in order, so its inverse must apply. If a save is
/// refused: every entity is filed under an id and each save has an empty
/// directory of its own, so it must write.
pub(super) fn play(document: &mut Document, op: &Op, reached: &mut Reached) -> Outcome {
    match op {
        Op::Write {
            target,
            system,
            leaf,
            value,
            through,
        } => write(document, (target, system, leaf), value, *through),
        Op::FieldDrag {
            target,
            system,
            leaf,
            frames,
        } => {
            let target = target_of(document, target);
            let (system, path, kind) = leaf_of(document, target, system, leaf);
            let values: Vec<Value> = frames.iter().map(|frame| frame.of(kind)).collect();
            let outcome = inspect(document, target, &system, &path, &values);
            if outcome == Outcome::Recorded && values.len() > 1 {
                reached.push("a gesture of several writes");
            }
            outcome
        }
        Op::EnvironmentWrite {
            leaf,
            value,
            through,
        } => {
            let (path, kind) = environment_leaf(document, leaf);
            let value = value.of(kind);
            match through {
                Through::Command => {
                    accepted(document.apply(EditCommand::SetEnvironment { path, value }))
                }
                Through::Inspector => inspect_environment(document, &path, &[value]),
                Through::FieldPaste => {
                    accepted(document.paste_environment_field(&path, &text_of(&value)))
                }
            }
        }
        Op::EnvironmentDrag { leaf, frames } => {
            let (path, kind) = environment_leaf(document, leaf);
            let values: Vec<Value> = frames.iter().map(|frame| frame.of(kind)).collect();
            let outcome = inspect_environment(document, &path, &values);
            if outcome == Outcome::Recorded {
                reached.push("an environment drag of several writes");
            }
            outcome
        }
        Op::Nudge {
            selection,
            axis,
            delta,
        } => nudge(document, selection, axis, *delta, reached),
        Op::Drag {
            selection,
            axis,
            offsets,
            switch,
        } => drag(
            document,
            selection,
            (axis, switch.as_ref()),
            offsets,
            reached,
        ),
        Op::Turn { target, quaternion } => turn(document, target, *quaternion),
        Op::Switch {
            target,
            variant,
            inspector,
        } => switch(document, target, variant, *inspector, reached),
        Op::Scale { target, factor } => scale(document, target, *factor),
        Op::Rename { target, name } => {
            let target = target_of(document, target);
            let current = document
                .entity_name(target)
                .map(|name| name.as_str().to_owned());
            match document.rename(target, &name_text(name, current)) {
                Ok(true) => Outcome::Recorded,
                Ok(false) => Outcome::Unchanged,
                Err(_) => Outcome::Refused,
            }
        }
        Op::Delete { selection } => {
            let Some(ids) = select(document, selection) else {
                return Outcome::Skipped;
            };
            let outcome = accepted(document.delete(&ids));
            if outcome == Outcome::Recorded && ids.len() > 1 {
                reached.push("a delete of two entities");
            }
            outcome
        }
        Op::Duplicate { selection } => {
            let Some(ids) = select(document, selection) else {
                return Outcome::Skipped;
            };
            let Ok(copies) = document.duplicate(&ids) else {
                return Outcome::Refused;
            };
            if ids.len() > 1 {
                reached.push("a duplicate of two entities");
            }
            document.set_selection(copies);
            Outcome::Recorded
        }
        Op::Paste { selection, garbled } => {
            let text = if *garbled {
                "hello".to_owned()
            } else {
                let Some(ids) = select(document, selection) else {
                    return Outcome::Skipped;
                };
                document.copy(&ids).expect("a copy of held entities")
            };
            let Ok(pasted) = document.paste(&text) else {
                return Outcome::Refused;
            };
            document.set_selection(pasted);
            Outcome::Recorded
        }
        Op::Drop { asset, x, z } => {
            let listed = document.manifest().iter().any(|each| each == MESHES);
            let Ok(id) = document.spawn_mesh(asset.get(&TEXTS), DVec3::new(*x, 0.0, *z)) else {
                return Outcome::Refused;
            };
            if !listed {
                reached.push("a drop listing meshes");
            }
            document.select(Some(id));
            Outcome::Recorded
        }
        Op::Add { system } => {
            let system = registered(document, system);
            let listed = document.manifest().contains(&system);
            let Ok(id) = document.add_entity(&system) else {
                return Outcome::Refused;
            };
            if !listed {
                reached.push("an add listing its system");
            }
            document.select(Some(id));
            Outcome::Recorded
        }
        Op::Attach { target, system } => {
            let target = target_of(document, target);
            let system = registered(document, system);
            let listed = document.manifest().contains(&system);
            let outcome = accepted(document.attach(target, &system));
            if outcome == Outcome::Recorded && !listed {
                reached.push("an attach listing its system");
            }
            outcome
        }
        Op::Detach { target, system } => {
            let target = target_of(document, target);
            let systems = document.systems_of(target);
            let system = in_or_beyond(document, systems, system);
            accepted(document.detach(target, &system))
        }
        Op::List { system, at } => {
            let system = registered(document, system);
            let at = at.index(document.manifest().len() + 2);
            accepted(document.apply(EditCommand::ListSystem { system, at }))
        }
        Op::Unlist { system, empty } => unlist(document, system, *empty, reached),
        Op::Save => {
            // A directory per save: the document has no origin, so a save is a
            // copy, which refuses a directory already holding the scene.
            let dir = tempfile::tempdir().expect("a temporary directory");
            document
                .save_to(dir.path())
                .expect("the authored scene saves into an empty directory");
            Outcome::Saved
        }
        Op::Undo => walked(document.undo().expect("an entry's inverse applies")),
        Op::Redo => walked(document.redo().expect("an entry applies again")),
    }
}

/// One enum a [`Op::Switch`] can pick: whose, in which system, at which
/// path, the variant it is in and every variant it has.
struct Picked {
    entity: SceneEntityId,
    system: String,
    path: String,
    active: &'static str,
    variants: Vec<&'static str>,
}

/// [`Op::Switch`]: the enum `target` names among every one the held
/// entities' components hold, switched to the variant `variant` names.
///
/// Through the inspector, the switch is made in place and reported as the
/// drop-down reports it — not at all for the variant already active, which
/// the drop-down does not report as a pick. As a command, the new variant's snapshot
/// is read by switching and putting the component back, then applied.
fn switch(
    document: &mut Document,
    target: &Index,
    variant: &Index,
    inspector: bool,
    reached: &mut Reached,
) -> Outcome {
    let mut enums = Vec::new();
    for entity in held(document) {
        for system in document.systems_of(entity) {
            let Some(component) = document.component(entity, &system) else {
                continue;
            };
            let mut found = Vec::new();
            enums_of(component, "", &mut found);
            for (path, active, variants) in found {
                enums.push(Picked {
                    entity,
                    system: system.clone(),
                    path,
                    active,
                    variants,
                });
            }
        }
    }
    if enums.is_empty() {
        return Outcome::Skipped;
    }
    let picked = target.get(&enums);
    let mut names = picked.variants.clone();
    names.push(NO_SUCH_VARIANT);
    let name = *variant.get(&names);
    let component = document
        .component(picked.entity, &picked.system)
        .expect("the enum was found in it");
    let before = snapshot_path(component, &picked.path).expect("the enum was found there");
    let after = match set_variant_path(component, &picked.path, name) {
        Ok(()) => snapshot_path(component, &picked.path).expect("the enum is still there"),
        Err(_) if inspector => return Outcome::Refused,
        Err(_) => Snapshot::Variant {
            name: NO_SUCH_VARIANT.into(),
            fields: Vec::new(),
        },
    };
    let outcome = if inspector {
        if name == picked.active {
            return Outcome::Unchanged;
        }
        let switch = VariantEdit {
            path: picked.path.clone(),
            before,
            after,
        };
        accepted(document.record_edits(picked.entity, &picked.system, &[], &[switch], None))
    } else {
        restore_path(component, &picked.path, &before).expect("its own snapshot fits");
        accepted(document.apply(EditCommand::SetVariant {
            entity: picked.entity,
            system: picked.system.clone(),
            path: picked.path.clone(),
            value: after,
        }))
    };
    if outcome == Outcome::Recorded && name != picked.active {
        reached.push("a switch to another variant");
    }
    outcome
}

/// Every enum under `value` at the dotted path `prefix`: its path, its active
/// variant and every variant it has.
fn enums_of(
    value: &dyn Reflect,
    prefix: &str,
    into: &mut Vec<(String, &'static str, Vec<&'static str>)>,
) {
    if let Some(active) = value.variant() {
        let variants = value.variants().iter().map(|each| each.name).collect();
        into.push((prefix.to_owned(), active, variants));
    }
    if matches!(value.kind(), Kind::Struct | Kind::Enum) {
        for (index, field) in value.fields().iter().enumerate() {
            if let Some(child) = value.field(index) {
                let path = if prefix.is_empty() {
                    field.name.to_owned()
                } else {
                    format!("{prefix}.{}", field.name)
                };
                enums_of(child, &path, into);
            }
        }
    }
}

/// [`Op::Write`]: the leaf the draws name, written `value` `through` one of
/// the paths a single write takes.
fn write(
    document: &mut Document,
    (target, system, leaf): (&Index, &Index, &Index),
    value: &Draw,
    through: Through,
) -> Outcome {
    let target = target_of(document, target);
    let (system, path, kind) = leaf_of(document, target, system, leaf);
    let value = value.of(kind);
    match through {
        Through::Command => accepted(document.apply(EditCommand::SetProperty {
            entity: target,
            system,
            path,
            value,
        })),
        Through::Inspector => inspect(document, target, &system, &path, &[value]),
        Through::FieldPaste => {
            accepted(document.paste_field(target, &system, &path, &text_of(&value)))
        }
    }
}

/// [`Op::Nudge`], as `App::nudge` builds it: each selected entity's placing
/// component moved from what its leaf holds, as one entry, passing over an
/// entity nothing places.
fn nudge(
    document: &mut Document,
    selection: &Pair,
    axis: &Index,
    delta: f64,
    reached: &mut Reached,
) -> Outcome {
    let Some(ids) = select(document, selection) else {
        return Outcome::Skipped;
    };
    let placed = placed(document, &ids, axis);
    if placed.is_empty() {
        return Outcome::Unchanged;
    }
    let several = placed.len() > 1;
    let outcome = accepted(document.apply(moved(&placed, delta)));
    if outcome == Outcome::Recorded && several {
        reached.push("a nudge of two entities");
    }
    outcome
}

/// [`Op::Drag`], as `App::move_handle` writes it: one write a frame under
/// one gesture, each frame every selected entity at where it started plus
/// that frame's offset — along the first axis, and from the `switch` frame on
/// along its axis too, or instead. One entry if any frame was accepted, or
/// none if the drag ended where it began.
fn drag(
    document: &mut Document,
    selection: &Pair,
    (axis, switch): (&Index, Option<&Switch>),
    offsets: &[f64],
    reached: &mut Reached,
) -> Outcome {
    let Some(ids) = select(document, selection) else {
        return Outcome::Skipped;
    };
    let first = placed(document, &ids, axis);
    let (from, second, keep_first) = match switch {
        Some(switch) => (
            1 + switch.at.index(offsets.len() - 1),
            placed(document, &ids, &switch.axis),
            switch.keep_first,
        ),
        None => (offsets.len(), Vec::new(), true),
    };
    if first.is_empty() {
        return Outcome::Skipped;
    }
    let gesture = document.begin_gesture();
    let (mut early, mut late) = (0, 0);
    for (frame, offset) in offsets.iter().enumerate() {
        let leaves: Vec<Placed> = if frame < from {
            first.clone()
        } else {
            let kept = if keep_first { first.as_slice() } else { &[] };
            kept.iter().chain(&second).cloned().collect()
        };
        if leaves.is_empty() || document.apply_in(moved(&leaves, *offset), gesture).is_err() {
            continue;
        }
        if frame < from {
            early += 1;
        } else {
            late += 1;
        }
    }
    let wrote = early + late;
    if wrote > 1 {
        reached.push("a gesture of several writes");
    }
    let changed = !keep_first
        || second
            .iter()
            .any(|leaf| first.iter().all(|held| held.2 != leaf.2));
    if early > 0 && late > 0 && changed {
        reached.push("a drag whose leaves change part-way");
    }
    if wrote > 0 && first.len() > 1 {
        reached.push("a drag of two entities");
    }
    if wrote > 0 {
        Outcome::Recorded
    } else {
        Outcome::Refused
    }
}

/// One selected entity a move writes: its id, its placing system, the path
/// moved and the value that leaf held.
type Placed = (SceneEntityId, String, String, f64);

/// Each of `ids` something places, with its position along `axis`.
fn placed(document: &mut Document, ids: &[SceneEntityId], axis: &Index) -> Vec<Placed> {
    let path = format!("position.{}", axis.index(3));
    let mut placed = Vec::new();
    for &entity in ids {
        let Some(system) = document.placing_system(entity) else {
            continue;
        };
        if let Ok(Value::Float(was)) = document.read(entity, &system, &path) {
            placed.push((entity, system, path.clone(), was));
        }
    }
    placed
}

/// Every one of `placed` moved `offset` from where it was, as one entry.
fn moved(placed: &[Placed], offset: f64) -> EditCommand {
    EditCommand::one_or_batch(
        placed
            .iter()
            .map(|(entity, system, path, was)| EditCommand::SetProperty {
                entity: *entity,
                system: system.clone(),
                path: path.clone(),
                value: Value::Float(was + offset),
            })
            .collect(),
    )
}

/// [`Op::Turn`]: the target's placing component's rotation leaves set to
/// `quaternion` normalised, as one batch — a block's, for an entity nothing
/// places, which the document refuses.
fn turn(document: &mut Document, target: &Index, quaternion: [f64; 4]) -> Outcome {
    let target = target_of(document, target);
    let length = quaternion
        .iter()
        .map(|each| each * each)
        .sum::<f64>()
        .sqrt();
    let unit = if length > f64::EPSILON {
        quaternion.map(|each| each / length)
    } else {
        Rotation::IDENTITY.to_array()
    };
    let system = placing_or_block(document, target);
    let leaves = Rotation::LEAVES.into_iter().zip(unit);
    accepted(
        document.apply(EditCommand::Batch(
            leaves
                .map(|(leaf, value)| EditCommand::SetProperty {
                    entity: target,
                    system: system.clone(),
                    path: format!("rotation.{leaf}"),
                    value: Value::Float(value),
                })
                .collect(),
        )),
    )
}

/// [`Op::Scale`]: the target's placing component's half extents multiplied
/// by `factor`, as one batch.
fn scale(document: &mut Document, target: &Index, factor: f64) -> Outcome {
    let target = target_of(document, target);
    let system = placing_or_block(document, target);
    let mut commands = Vec::new();
    for axis in 0..3 {
        let path = format!("half_extents.{axis}");
        let was = match document.read(target, &system, &path) {
            Ok(Value::Float(was)) => was,
            _ => 1.0,
        };
        commands.push(EditCommand::SetProperty {
            entity: target,
            system: system.clone(),
            path,
            value: Value::Float(was * factor),
        });
    }
    accepted(document.apply(EditCommand::Batch(commands)))
}

/// The system placing `target`, or the blocks for one nothing places — a
/// gizmo would draw no handle there, so the write it gets is one the
/// document refuses.
fn placing_or_block(document: &mut Document, target: SceneEntityId) -> String {
    document
        .placing_system(target)
        .unwrap_or_else(|| crate::scene::BLOCKS.to_owned())
}

/// [`Op::Unlist`].
fn unlist(document: &mut Document, system: &Index, empty: bool, reached: &mut Reached) -> Outcome {
    let listed = document.manifest().to_vec();
    let system = if empty {
        let empty: Vec<String> = listed
            .into_iter()
            .filter(|system| document.entities_in(system).is_empty())
            .collect();
        if empty.is_empty() {
            return Outcome::Skipped;
        }
        system.get(&empty).clone()
    } else {
        in_or_beyond(document, listed, system)
    };
    let systems = document.manifest();
    let inside = systems
        .iter()
        .position(|each| *each == system)
        .is_some_and(|at| at + 1 < systems.len());
    let outcome = accepted(document.apply(EditCommand::UnlistSystem { system }));
    if outcome == Outcome::Recorded && inside {
        reached.push("an unlisting from the manifest's middle");
    }
    outcome
}

/// [`Outcome::Recorded`] for an accepted edit, [`Outcome::Refused`] for a
/// refused one.
fn accepted<T>(result: Result<T, EditError>) -> Outcome {
    if result.is_ok() {
        Outcome::Recorded
    } else {
        Outcome::Refused
    }
}

/// [`Outcome::Walked`] for an undo or redo that moved the log,
/// [`Outcome::AtEnd`] for one that did not.
const fn walked(moved: bool) -> Outcome {
    if moved {
        Outcome::Walked
    } else {
        Outcome::AtEnd
    }
}

/// Every id the document holds, in outline order.
fn held(document: &mut Document) -> Vec<SceneEntityId> {
    document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect()
}

/// The entity `draw` names: one the document holds, or [`ABSENT`] for a draw
/// past them.
fn target_of(document: &mut Document, draw: &Index) -> SceneEntityId {
    let held = held(document);
    held.get(draw.index(held.len() + 1))
        .copied()
        .unwrap_or(ABSENT)
}

/// Selects the entities `pair` names, as clicks would, and hands back the
/// selection — or [`None`] for a document holding nothing to select.
fn select(document: &mut Document, pair: &Pair) -> Option<Vec<SceneEntityId>> {
    let held = held(document);
    if held.is_empty() {
        return None;
    }
    let first = *pair.first.get(&held);
    let second = pair.second.as_ref().map(|second| *second.get(&held));
    document.set_selection([first].into_iter().chain(second));
    Some(document.selection().to_vec())
}

/// The registered system `draw` names, listed or not.
fn registered(document: &Document, draw: &Index) -> String {
    let systems: Vec<&str> = document.registry().systems().collect();
    (*draw.get(&systems)).to_owned()
}

/// The system of `systems` that `draw` names — or, for a draw past them, the
/// first registered system not among them.
fn in_or_beyond(document: &Document, mut systems: Vec<String>, draw: &Index) -> String {
    let beyond = document
        .registry()
        .systems()
        .find(|each| !systems.iter().any(|system| system == each))
        .map(str::to_owned);
    systems.extend(beyond);
    draw.get(&systems).clone()
}

/// The leaf a write of `target` names: one of the systems holding it — or,
/// for a draw past them, a registered system that does not — and one of that
/// component's leaves, with its kind. A component the document cannot reach
/// is written at `position.0` as a float, which the document refuses.
fn leaf_of(
    document: &mut Document,
    target: SceneEntityId,
    system: &Index,
    leaf: &Index,
) -> (String, String, ValueKind) {
    let systems = document.systems_of(target);
    let system = in_or_beyond(document, systems, system);
    let mut leaves = Vec::new();
    if let Some(component) = document.component(target, &system) {
        leaves_of(component, "", &mut leaves);
    }
    if leaves.is_empty() {
        return (system, "position.0".to_owned(), ValueKind::Float);
    }
    let (path, kind) = leaf.get(&leaves).clone();
    (system, path, kind)
}

/// Every leaf of `value` below the dotted path `prefix`, with its kind.
fn leaves_of(value: &dyn Reflect, prefix: &str, into: &mut Vec<(String, ValueKind)>) {
    let below = |segment: &str| {
        if prefix.is_empty() {
            segment.to_owned()
        } else {
            format!("{prefix}.{segment}")
        }
    };
    match value.kind() {
        Kind::Leaf(kind) => into.push((prefix.to_owned(), kind)),
        Kind::Struct | Kind::Enum => {
            for (index, field) in value.fields().iter().enumerate() {
                if let Some(child) = value.field(index) {
                    leaves_of(child, &below(field.name), into);
                }
            }
        }
        Kind::List { len } => {
            for index in 0..len {
                if let Some(child) = value.field(index) {
                    leaves_of(child, &below(&index.to_string()), into);
                }
            }
        }
    }
}

/// The inspector's half of a write: each value written into the leaf in
/// place, read back, and reported to [`Document::record_edits`] — under one
/// gesture when there are several, as a drag reports one edit a frame. A value
/// the leaf refuses is no edit to report.
fn inspect(
    document: &mut Document,
    target: SceneEntityId,
    system: &str,
    path: &str,
    values: &[Value],
) -> Outcome {
    let gesture = (values.len() > 1).then(|| document.begin_gesture());
    let mut recorded = false;
    for value in values {
        let Some(component) = document.component(target, system) else {
            return Outcome::Skipped;
        };
        let Ok(before) = get_path(component, path) else {
            return Outcome::Skipped;
        };
        if set_path(component, path, value).is_err() {
            continue;
        }
        let after = get_path(component, path).expect("a leaf just written");
        let edit = FieldEdit {
            path: path.to_owned(),
            before,
            after,
        };
        recorded |= document
            .record_edits(target, system, &[edit], &[], gesture)
            .is_ok();
    }
    if recorded {
        Outcome::Recorded
    } else {
        Outcome::Refused
    }
}

/// The leaf of the scene's environment `draw` names, with its kind.
fn environment_leaf(document: &Document, draw: &Index) -> (String, ValueKind) {
    let mut leaves = Vec::new();
    leaves_of(&document.environment(), "", &mut leaves);
    draw.get(&leaves).clone()
}

/// The inspector's half of an environment write: each value written into a
/// copy of the environment, read back, and reported to
/// [`Document::record_environment`] — under one gesture when there are
/// several, as a drag reports one edit a frame. A value the leaf refuses is
/// no edit to report.
fn inspect_environment(document: &mut Document, path: &str, values: &[Value]) -> Outcome {
    let gesture = (values.len() > 1).then(|| document.begin_gesture());
    let mut recorded = false;
    for value in values {
        let mut environment = document.environment();
        let before = get_path(&environment, path).expect("a leaf of the environment");
        if set_path(&mut environment, path, value).is_err() {
            continue;
        }
        let after = get_path(&environment, path).expect("a leaf just written");
        let edit = FieldEdit {
            path: path.to_owned(),
            before,
            after,
        };
        recorded |= document.record_environment(&[edit], gesture).is_ok();
    }
    if recorded {
        Outcome::Recorded
    } else {
        Outcome::Refused
    }
}

/// What a rename names an entity, from the draw `draw`: a name, the name it
/// has (`current`, which changes nothing), nothing at all — empty or blank —
/// or text no name may be: a control character, or one character too long.
fn name_text(draw: &Index, current: Option<String>) -> String {
    let names = [
        "Gate".to_owned(),
        "Spawner".to_owned(),
        current.unwrap_or_default(),
        String::new(),
        "   ".to_owned(),
        "bell\u{7}".to_owned(),
        "n".repeat(MAX_NAME_CHARS + 1),
    ];
    draw.get(&names).clone()
}
