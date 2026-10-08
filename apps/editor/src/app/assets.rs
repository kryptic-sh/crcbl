//! Asset-browser drags and placement.

use crcbl::engine::Pending;
use crcbl::math::DVec3;
use crcbl::shell::Shell;

use super::Editor;
use crate::document::{Document, EditError};
use crate::panel::Tone;

impl<S: Shell + ?Sized> Editor<S> {
    /// Takes hold of the asset a press on the browser landed on, and drops
    /// the one held where the button comes up over the viewport — see the
    /// module docs.
    pub(super) fn drag_asset(&mut self, pending: &Pending, in_viewport: bool) {
        if pending.pointer_pressed && !in_viewport {
            self.dragged = pending.pointer.and_then(|at| self.panels.asset_at(at));
        }
        if !pending.pointer_released {
            return;
        }
        let Some(asset) = self.dragged.take() else {
            return;
        };
        if let (true, Some(at)) = (in_viewport, pending.pointer) {
            let ray = self.ray_at(at);
            let point = self.document.drop_point(&ray);
            self.place(&asset, point);
        }
    }

    /// Places `asset` where the ray through the middle of the viewport meets
    /// the ground: Enter on the asset browser.
    pub(super) fn place_at_centre(&mut self, asset: &str) {
        let (min, max) = self.panels.viewport_pixels();
        let ray = self.ray_at((min + max) * 0.5);
        self.place(asset, Document::ground_point(&ray));
    }

    /// Spawns a mesh of `asset` standing on `point` and selects it, saying on
    /// the status line what was placed — and that it is a placeholder, and
    /// why, when its asset will not load — or why nothing was.
    pub(super) fn place(&mut self, asset: &str, point: Result<DVec3, EditError>) {
        let placed = point.and_then(|point| self.document.spawn_mesh(asset, point));
        let id = match placed {
            Ok(id) => id,
            Err(error) => {
                crcbl::log::warn!("editor: {error}");
                self.panels.set_status(error.to_string(), Tone::Warning);
                return;
            }
        };
        self.document.select(Some(id));
        let prefix = format!("entity #{id}: ");
        let problem = self
            .document
            .mesh_problems()
            .into_iter()
            .find(|problem| problem.starts_with(&prefix));
        match problem {
            Some(problem) => self.panels.set_status(
                format!("Placed #{id} as a placeholder: {problem}"),
                Tone::Warning,
            ),
            None => self
                .panels
                .set_status(format!("Placed `{asset}` as #{id}"), Tone::Info),
        }
    }
}
