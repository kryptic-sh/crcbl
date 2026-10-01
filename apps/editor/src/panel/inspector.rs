//! The inspector pane: the selected entity's components, **one section per
//! system holding it**, and the buttons that attach and detach them.
//!
//! An entity may be in several of the scene's systems, each holding one
//! component of it, so the pane draws a section per system in the order the
//! scene's manifest lists them ([`Document::systems_of`]) — a heading naming
//! the system and the component's type, then that component's rows. Every
//! field edit is reported with the system its section was drawn for, which is
//! the component it is applied to.
//!
//! **Remove** on a section detaches that system's component, and is drawn only
//! while the entity has more than one: detaching the last is refused
//! ([`EditError::NoComponent`](crate::document::EditError::NoComponent)), and
//! Delete is how an entity goes. Under the sections, one **add** button per
//! system the entity could join attaches that system's component at its
//! type's `Default` — under a heading per group
//! ([`Document::attachable_groups`]): the scene's own systems first, then each
//! game's, so another game's system is never offered unremarked. Neither is
//! carried out here:
//! the pane reports the [`Change`] and [`super::Panels::frame`] applies it
//! through the document, so a refusal reaches the status line like every
//! other.
//!
//! **A rotation is drawn as three angles** ([`overrides`]): the quaternion a
//! `crcbl::registry::Rotation` holds is no row a person can drag, so its row
//! shows degrees and writes all four leaves when one angle moves.

use crcbl::math::{DQuat, EulerRot};
use crcbl::reflect::Value;
use crcbl::registry::Rotation;
use crcbl::scene::scn::SceneEntityId;
use crcbl::ui::tree::{AXES, FieldEdit, FieldRow, InspectorOptions, NodeKey, Overrides, Ui};

use crate::document::Document;

/// What a click in the inspector asked the document for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Change {
    /// Give the entity a component in this system.
    Attach(String),
    /// Take the entity's component out of this system.
    Detach(String),
}

/// One system's section, as a frame built it.
#[derive(Clone, Debug)]
pub(super) struct Section {
    /// The system the section was drawn for.
    pub(super) system: String,
    /// Its component's rows: the inspector block.
    pub(super) fields: NodeKey,
    /// Its remove button, when it has one.
    pub(super) remove: Option<NodeKey>,
}

/// What [`build`] built and what the pane reported.
#[derive(Debug, Default)]
pub(super) struct Built {
    /// The scrolling block holding the sections, or [`None`] when nothing
    /// selected has a component to show.
    pub(super) props: Option<NodeKey>,
    /// Each section, in the order drawn.
    pub(super) sections: Vec<Section>,
    /// Each add button and the system it attaches, in the order drawn.
    pub(super) adds: Vec<(String, NodeKey)>,
    /// Each add-list heading, with the label it reads, in the order drawn.
    pub(super) headings: Vec<(String, NodeKey)>,
    /// The frame's field edits, each with the system of its section.
    pub(super) edits: Vec<(String, FieldEdit)>,
    /// The leaf a clipboard key means — its system and path — if there is one;
    /// see the panel module docs.
    pub(super) field: Option<(String, String)>,
    /// What a click asked for this frame.
    pub(super) change: Option<Change>,
}

/// The inspector pane for `selected`, labelled `label`.
pub(super) fn build(
    ui: &mut Ui,
    document: &mut Document,
    selected: Option<SceneEntityId>,
    label: &str,
    overrides: &Overrides,
) -> Built {
    let mut built = Built::default();
    ui.block(".editor-panel", &[], |ui| {
        let Some(id) = selected else {
            ui.span(".editor-title", "Properties", &[]);
            ui.span(".editor-note", "Nothing is selected", &[]);
            return;
        };
        let systems = document.systems_of(id);
        if systems.is_empty() {
            ui.span(".editor-title", "Properties", &[]);
            let note = format!("#{id} has no editable component");
            ui.span(".editor-note", note.as_str(), &[]);
            return;
        }
        ui.span(".editor-title", label, &[]);
        let options = InspectorOptions {
            overrides: Some(overrides),
            ..InspectorOptions::default()
        };
        let removable = systems.len() > 1;
        let attachable = document.attachable_groups(id);
        // A focused leaf anywhere outranks a hovered one anywhere: the
        // keyboard's own target is the one a key means.
        let mut focused = None;
        let mut hovered = None;
        let props = ui.block("#props", &[], |ui| {
            for system in &systems {
                ui.block_keyed(system, ".inspector-section", &[], |ui| {
                    let Some(component) = document.component(id, system) else {
                        return;
                    };
                    let kind = component.type_name().rsplit("::").next().unwrap_or("");
                    let heading = format!("{system} · {kind}");
                    let mut remove = None;
                    ui.block(".section-head", &[], |ui| {
                        ui.span(".section-title", heading.as_str(), &[]);
                        if removable {
                            let button = ui.button(".section-remove", "Remove");
                            if button.clicked {
                                built.change = Some(Change::Detach(system.clone()));
                            }
                            remove = Some(button.key);
                        }
                    });
                    let Some(component) = document.component(id, system) else {
                        return;
                    };
                    let inspection = ui.inspector_with(".section-fields", component, &options);
                    built.sections.push(Section {
                        system: system.clone(),
                        fields: inspection.response.key,
                        remove,
                    });
                    built.edits.extend(
                        inspection
                            .edits
                            .into_iter()
                            .map(|edit| (system.clone(), edit)),
                    );
                    if let Some(path) = inspection.focused {
                        focused = Some((system.clone(), path));
                    }
                    if let Some(path) = inspection.hovered {
                        hovered = Some((system.clone(), path));
                    }
                });
            }
            if !attachable.is_empty() {
                ui.block(".inspector-add", &[], |ui| {
                    ui.span(".section-title", "Add a component", &[]);
                    // One line per group, its label and then its buttons,
                    // wrapping when they do not fit across the pane.
                    for group in &attachable {
                        ui.block_keyed(&group.label, ".add-group", &[], |ui| {
                            let heading = ui.span(".add-group-label", group.label.as_str(), &[]);
                            built.headings.push((group.label.clone(), heading.key));
                            for system in &group.systems {
                                let text = format!("+ {system}");
                                let button = ui.button(".section-add", text.as_str());
                                if button.clicked {
                                    built.change = Some(Change::Attach(system.clone()));
                                }
                                built.adds.push((system.clone(), button.key));
                            }
                        });
                    }
                });
            }
        });
        built.props = Some(props.key);
        built.field = focused.or(hovered);
    });
    built
}

/// The order a rotation's three angles are composed in on its row: about X,
/// then Y, then Z.
const EULER: EulerRot = EulerRot::XYZ;

/// How far one pixel of a drag turns an angle on the rotation row, in degrees.
const DEGREE_STEP: f32 = 0.5;

/// How far an angle on the rotation row is held either way of zero, in
/// degrees: a whole turn, which every orientation is inside.
const DEGREE_RANGE: f32 = 360.0;

/// The inspector's per-type rows: [`Overrides::vectors`], and a
/// [`Rotation`] drawn as three angles.
pub(super) fn overrides() -> Overrides {
    let mut overrides = Overrides::vectors();
    overrides.register::<Rotation>(rotation_row);
    overrides
}

/// A [`Rotation`] as three angles in degrees on one row — the view every
/// editor of this shape gives a turn, over the quaternion the file holds.
///
/// A dragged angle is composed back with the other two (in [`EULER`] order)
/// and the quaternion's four leaves written at once, which
/// [`super::Panels::apply_edits`] makes one command — never one leaf of the
/// four alone. The angles are read back from the quaternion each frame, so
/// near a right-angle pitch two of them trade places as Euler angles do; the
/// orientation does not jump. The widgets name no leaf for the clipboard keys:
/// an angle is not a leaf the file holds.
fn rotation_row(ui: &mut Ui, field: &mut FieldRow<'_>) {
    let Some(rotation) = field.value.as_any().downcast_ref::<Rotation>() else {
        return;
    };
    let degrees = degrees_of(rotation.quat());
    let label = field.label;
    let mut turned = None;
    ui.block(".inspector-row", &[], |ui| {
        ui.span(".inspector-label", label, &[]);
        for (index, axis) in AXES.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let mut number = degrees[index] as f32;
            let mut moved = false;
            // Keyed by the axis, as a vector row's are.
            ui.block_keyed(*axis, ".inspector-axis", &[], |ui| {
                ui.span(".inspector-axis-label", *axis, &[]);
                let response = ui.drag_value(
                    ".inspector-field",
                    &mut number,
                    -DEGREE_RANGE..=DEGREE_RANGE,
                    DEGREE_STEP,
                    DEGREE_STEP,
                );
                moved = response.changed;
            });
            if moved {
                let mut angles = degrees;
                angles[index] = f64::from(number);
                turned = Some(angles);
            }
        }
    });
    if let Some(angles) = turned {
        let quat = quat_of(angles);
        for (leaf, value) in Rotation::LEAVES.into_iter().zip(quat.to_array()) {
            field.set(leaf, Value::Float(value));
        }
    }
}

/// `quat` as the three angles its row shows, in degrees.
pub(super) fn degrees_of(quat: DQuat) -> [f64; 3] {
    let (x, y, z) = quat.to_euler(EULER);
    [x, y, z].map(f64::to_degrees)
}

/// The quaternion three angles in degrees compose to.
pub(super) fn quat_of(degrees: [f64; 3]) -> DQuat {
    let [x, y, z] = degrees.map(f64::to_radians);
    DQuat::from_euler(EULER, x, y, z)
}
