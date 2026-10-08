//! Selection handles, pointer drags and transform-mode status.

use crcbl::core::input::Modifiers;
use crcbl::engine::Pending;
use crcbl::math::{DQuat, DVec3, Vec2, Vec3};
use crcbl::reflect::Value;
use crcbl::registry::Rotation;
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::Shell;

use super::{Drag, Editor};
use crate::command::EditCommand;
use crate::gizmo;
use crate::panel::Tone;

impl<S: Shell + ?Sized> Editor<S> {
    /// The selection's gizmo handles in the current mode, in the pane's
    /// pixels — none for nothing selected, and none for an entity without the
    /// field the mode writes: no `position` to move, no `half_extents` to
    /// resize, or no `rotation` to turn. With several selected, translate's
    /// alone, while one of them has a `position` — see the gizmo's module
    /// docs.
    pub(super) fn handles(&mut self) -> Vec<gizmo::Handle> {
        let Some((centre, frame)) = self.gizmo_centre() else {
            return Vec::new();
        };
        gizmo::handles(
            &self.camera.camera(),
            self.panels.viewport_extent(),
            centre,
            frame,
            self.panels.scale(),
            self.gizmo_mode,
        )
    }

    /// Where the handles stand, in render space, and the frame the
    /// handle's axes are turned by — or [`None`] where the current mode shows
    /// none (see [`handles`](Self::handles)).
    ///
    /// A lone entity's handles stand at the centre of its box, turned as it
    /// is; several selected share theirs at the selection's pivot
    /// ([`crate::document::Document::selection_pivot`]). Local mode uses the primary entity's
    /// axes, falling back to world axes when it has no placement.
    pub(super) fn gizmo_centre(&mut self) -> Option<(Vec3, DQuat)> {
        let id = self.document.primary()?;
        if self.document.selection().len() > 1 {
            if self.gizmo_mode != gizmo::Mode::Translate || self.group().members.is_empty() {
                return None;
            }
            let pivot = self.document.selection_pivot()?;
            let rotation = self
                .document
                .placement(id)
                .map_or(DQuat::IDENTITY, |placed| placed.rotation);
            return Some((pivot.as_vec3(), self.gizmo_space.frame(rotation)));
        }
        if !self.has_field(id, self.gizmo_mode) {
            return None;
        }
        let (min, max) = self.document.bounds(id)?;
        let placement = self.document.placement(id)?;
        let frame = if self.gizmo_mode == gizmo::Mode::Scale {
            placement.rotation
        } else {
            self.gizmo_space.frame(placement.rotation)
        };
        Some(((min + max) * 0.5, frame))
    }

    /// Every selected entity a translate moves — each one whose placing
    /// component has a `position` — and where each stands, in selection
    /// order.
    fn group(&mut self) -> gizmo::Group {
        let mut members = Vec::new();
        for id in self.document.selection().to_vec() {
            let (Some(system), Some(start)) = (
                self.document.placing_system(id),
                self.field_values(id, gizmo::POSITION),
            ) else {
                continue;
            };
            members.push(gizmo::Member {
                entity: id,
                system,
                start,
            });
        }
        gizmo::Group { members }
    }

    /// Whether the component placing `id` has the field `mode`'s handles
    /// write.
    fn has_field(&mut self, id: SceneEntityId, mode: gizmo::Mode) -> bool {
        match mode {
            gizmo::Mode::Rotate => self.rotation_of(id).is_some(),
            gizmo::Mode::Translate | gizmo::Mode::Scale => {
                self.field_values(id, mode.field()).is_some()
            }
        }
    }

    /// The three numbers `field.0` to `field.2` of the component placing `id`
    /// hold ([`crate::document::Document::placing_system`]), or [`None`] where nothing places it
    /// or it has no such field.
    ///
    /// **By name, through the component's reflected paths** — the way an arrow
    /// key finds `position` — so any registered component with the field has
    /// handles for it and no component type is named here. The placing
    /// component's, so the handles move what the picture is drawn from.
    pub(super) fn field_values(&mut self, id: SceneEntityId, field: &str) -> Option<[f64; 3]> {
        let system = self.document.placing_system(id)?;
        let mut values = [0.0; 3];
        for (index, value) in values.iter_mut().enumerate() {
            let Ok(Value::Float(read)) =
                self.document.read(id, &system, &format!("{field}.{index}"))
            else {
                return None;
            };
            *value = read;
        }
        Some(values)
    }

    /// The orientation of the component placing `id`, from its
    /// [`gizmo::ROTATION`] field — or [`None`] where nothing places it or it
    /// has no such field.
    ///
    /// By name, as [`field_values`](Self::field_values) is, and of the
    /// `crcbl::registry::Rotation` type the scene format reads: a field
    /// called `rotation` of any other type is not one the handles can write.
    pub(super) fn rotation_of(&mut self, id: SceneEntityId) -> Option<DQuat> {
        let system = self.document.placing_system(id)?;
        let component = self.document.component(id, &system)?;
        let index = component
            .fields()
            .iter()
            .position(|field| field.name == gizmo::ROTATION)?;
        component
            .field(index)?
            .as_any()
            .downcast_ref::<Rotation>()
            .map(Rotation::quat)
    }

    /// Starts a gizmo drag if the press landed on a handle, and says whether it
    /// did — a press that missed every handle is a pick.
    pub(super) fn grab_handle(&mut self, pending: &Pending) -> bool {
        let (Some(at), Some(id)) = (pending.pointer, self.document.primary()) else {
            return false;
        };
        let handles = self.handles();
        let (corner, _) = self.panels.viewport_pixels();
        let Some(grip) = gizmo::hit(&handles, at - corner, self.panels.scale()) else {
            return false;
        };
        let Some((centre, frame)) = self.gizmo_centre() else {
            return false;
        };
        let gesture = self.document.begin_gesture();
        let grabbed = match grip {
            gizmo::Grip::Rotate(axis) => self
                .begin_turn(id, axis, gesture, centre, at - corner)
                .map(|drag| (drag, gizmo::Group::default())),
            gizmo::Grip::Move(_) | gizmo::Grip::MovePlane(_) => {
                let group = self.group();
                // A group of one is its own pivot, as `gizmo::Drag::spread`
                // says: its position is what the drag moves and snaps.
                let start = match group.members.as_slice() {
                    [] => None,
                    [only] => Some(only.start),
                    _ => self
                        .document
                        .selection_pivot()
                        .map(|pivot| pivot.to_array()),
                };
                start
                    .and_then(|start| {
                        self.begin_drag(id, grip, gesture, start, (centre, frame), at)
                    })
                    .map(|drag| (drag, group))
            }
            gizmo::Grip::Scale(_) | gizmo::Grip::ScaleAll => self
                .field_values(id, gizmo::HALF_EXTENTS)
                .and_then(|start| self.begin_drag(id, grip, gesture, start, (centre, frame), at))
                .map(|drag| (drag, gizmo::Group::default())),
        };
        let Some((drag, group)) = grabbed else {
            return false;
        };
        self.drag = Some(Drag::Gizmo(drag, group));
        true
    }

    /// A drag of an arrow, a plane or a scale handle of `id`, whose field —
    /// or, for a translate, whose pivot — holds `start`, its handles standing
    /// at `shown`'s centre and turned by its frame, pressed at `at` in window
    /// pixels.
    fn begin_drag(
        &self,
        id: SceneEntityId,
        grip: gizmo::Grip,
        gesture: crate::command::Gesture,
        start: [f64; 3],
        shown: (Vec3, DQuat),
        at: Vec2,
    ) -> Option<gizmo::Drag> {
        let (centre, frame) = shown;
        gizmo::Drag::begin(
            id,
            grip,
            gesture,
            start,
            centre.as_dvec3(),
            frame,
            &self.pointer_at(at),
            self.panels.scale(),
        )
    }

    /// A drag of `id`'s ring about `axis`, pressed at `at` in the pane's
    /// pixels, its handles drawn about `shown` — or [`None`] for an entity with
    /// no rotation, or a centre behind the eye.
    ///
    /// The pivot is the placement's own centre in `f64`, not the drawn one, so
    /// a block whose position is its centre keeps that position to the bit.
    fn begin_turn(
        &mut self,
        id: SceneEntityId,
        axis: gizmo::Axis,
        gesture: crate::command::Gesture,
        shown: Vec3,
        at: Vec2,
    ) -> Option<gizmo::Drag> {
        let rotation = self.rotation_of(id)?;
        let pivot = self.document.placement(id)?.centre;
        let position = self
            .field_values(id, gizmo::POSITION)
            .map(DVec3::from_array);
        let camera = self.camera.camera();
        let centre = camera.pixel_of(shown, self.panels.viewport_extent())?;
        let frame = self.gizmo_space.frame(rotation);
        let toward_eye = (frame * axis.unit()).dot(camera.eye.as_dvec3() - pivot);
        let facing = if toward_eye < 0.0 { -1.0 } else { 1.0 };
        Some(gizmo::Drag::turn(
            id,
            axis,
            gesture,
            pivot,
            at,
            gizmo::Turn {
                rotation,
                position,
                centre,
                facing,
            },
            self.gizmo_space,
        ))
    }

    /// Moves, resizes or turns the dragged entity — or moves every entity of
    /// `group`, for a translate — to where the pointer at `at` puts it,
    /// snapped while Ctrl is held: one write of the drag's gesture, so the
    /// whole drag undoes at once.
    ///
    /// A handle that sets several leaves at once — a plane, the centre, a
    /// ring, any translate of several entities — sets them as one
    /// [`EditCommand::Batch`], which the log folds like a leaf
    /// (`crate::command::UndoLog::record_in`).
    pub(super) fn move_handle(&mut self, drag: &gizmo::Drag, group: &gizmo::Group, at: Vec2) {
        let snap = self
            .modifiers
            .contains(Modifiers::CTRL)
            .then_some(self.snap);
        let pointer = self.pointer_at(at);
        let set = |entity, system: String, write: gizmo::Write| EditCommand::SetProperty {
            entity,
            system,
            path: write.path,
            value: Value::Float(write.value),
        };
        let commands: Vec<EditCommand> = if group.members.is_empty() {
            let Some(writes) = drag.writes(&pointer, snap) else {
                return;
            };
            // Read each move rather than held by the drag: nothing but the
            // drag edits the scene while it runs, so the answer cannot move
            // under it.
            let Some(system) = self.document.placing_system(drag.entity) else {
                return;
            };
            writes
                .into_iter()
                .map(|write| set(drag.entity, system.clone(), write))
                .collect()
        } else {
            let Some(writes) = drag.spread(group, &pointer, snap) else {
                return;
            };
            writes
                .into_iter()
                .map(|(member, write)| set(member.entity, member.system.clone(), write))
                .collect()
        };
        let command = EditCommand::one_or_batch(commands);
        if let Err(error) = self.document.apply_in(command, drag.gesture) {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Draws the selection's handles over the pane, the one being dragged or
    /// under `pointer` brightened.
    pub(super) fn draw_gizmo(&mut self, pointer: Vec2) {
        let handles = self.handles();
        if handles.is_empty() {
            return;
        }
        let (corner, _) = self.panels.viewport_pixels();
        let hot = match &self.drag {
            Some(Drag::Gizmo(drag, _)) => Some(drag.grip()),
            _ => gizmo::hit(&handles, pointer - corner, self.panels.scale()),
        };
        self.panels
            .overlay_viewport(|list| gizmo::draw(list, &handles, corner, hot));
    }

    /// Shows `mode`'s handles from now on, and says on the status line what
    /// they do — or, for scale and rotate, why the selection has none.
    pub(super) fn choose_mode(&mut self, mode: gizmo::Mode) {
        self.gizmo_mode = mode;
        let (text, tone) = match mode {
            gizmo::Mode::Translate => (
                format!(
                    "Translate: drag an arrow along its axis or a square across its plane; \
                     hold Ctrl to snap to the {} m grid",
                    self.snap.grid_step()
                ),
                Tone::Info,
            ),
            gizmo::Mode::Scale => self.field_status(
                mode,
                "Scale: select an entity with half extents to resize it",
                format!(
                    "Scale: drag a box to resize along its axis or the centre to resize \
                     evenly; hold Ctrl to snap half extents to {} m",
                    self.snap.scale_step()
                ),
                "resize",
            ),
            gizmo::Mode::Rotate => self.field_status(
                mode,
                "Rotate: select an entity with a rotation to turn it",
                format!(
                    "Rotate: drag a ring to turn about its axis; hold Ctrl to snap to {}°",
                    self.snap.angle_step()
                ),
                "turn",
            ),
        };
        let axes = if mode == gizmo::Mode::Scale || self.gizmo_space == gizmo::Space::Local {
            "Local"
        } else {
            "World"
        };
        self.panels.set_status(
            format!("{text}. {axes} axes; X toggles translate/rotate axes"),
            tone,
        );
    }

    /// What the status line says when a mode whose handles need a field is
    /// chosen: `none` with nothing selected, why it shows no handles with
    /// several selected — it acts on one entity — `usage` when the selection
    /// has the field, and why it shows no handles when it does not — it has
    /// nothing to `verb`.
    fn field_status(
        &mut self,
        mode: gizmo::Mode,
        none: &str,
        usage: String,
        verb: &str,
    ) -> (String, Tone) {
        let Some(id) = self.document.primary() else {
            return (none.to_owned(), Tone::Info);
        };
        let label = match mode {
            gizmo::Mode::Translate => "Translate",
            gizmo::Mode::Scale => "Scale",
            gizmo::Mode::Rotate => "Rotate",
        };
        let count = self.document.selection().len();
        if count > 1 {
            return (
                format!(
                    "{label}: {count} entities are selected, and it acts on one at a time; \
                     select one to {verb} it"
                ),
                Tone::Warning,
            );
        }
        if self.has_field(id, mode) {
            return (usage, Tone::Info);
        }
        let placing = self.document.placing_system(id);
        let kind = placing
            .and_then(|system| self.document.component(id, &system))
            .map_or("entity", |component| {
                component
                    .type_name()
                    .rsplit("::")
                    .next()
                    .unwrap_or("entity")
            });
        (
            format!(
                "{label}: this {kind} has no `{}` field, so it has nothing to {verb}",
                mode.field()
            ),
            Tone::Warning,
        )
    }
}
