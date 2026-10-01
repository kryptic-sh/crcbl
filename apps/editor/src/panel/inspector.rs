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
//! system the entity could join ([`Document::attachable`]) attaches that
//! system's component at its type's `Default`. Neither is carried out here:
//! the pane reports the [`Change`] and [`super::Panels::frame`] applies it
//! through the document, so a refusal reaches the status line like every
//! other.

use crcbl::scene::scn::SceneEntityId;
use crcbl::ui::tree::{FieldEdit, InspectorOptions, NodeKey, Overrides, Ui};

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
    /// Each add button and the system it attaches.
    pub(super) adds: Vec<(String, NodeKey)>,
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
        let attachable = document.attachable(id);
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
                    for system in &attachable {
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
        built.props = Some(props.key);
        built.field = focused.or(hovered);
    });
    built
}
