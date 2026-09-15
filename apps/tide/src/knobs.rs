//! The three knobs — which scene, which medium, which camera — as one cell every
//! route to them writes.
//!
//! ```text
//!  key / pause row ──┐
//!  page export ──────┼──▶ CELL ──▶ Tide::draw ──▶ Stage::show, the camera
//!  --scene & co. ────┘
//! ```
//!
//! # Why a cell of this sample's own
//!
//! `apps/sundial`'s filter knobs are console variables the engine declares, so
//! a key, a pause row, a page export and a typed console line all write one cell
//! and there is no copy to keep in step. Tide's knobs are not the engine's — no
//! console variable names a scene of this sample — so this module is that cell:
//! [`crcbl::debug_view`]'s shape, one value written by whoever is driving and
//! read back by whoever draws. `crate::app::Tide` holds a *reading* of it,
//! taken once a frame, so the pause panel, the debug overlay, the heartbeat and
//! the summary all report one instant.
//!
//! **A [`std::sync::Mutex`] rather than three atomics**, so a reset is one
//! indivisible write rather than three a reader can land between.

use crate::medium::Preset;
use crate::menu::CameraMode;
use crate::scene::Scene;

/// Where all three knobs stand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Knobs {
    /// Which scene the next frame stages.
    pub scene: Scene,
    /// What its water is made of.
    pub medium: Preset,
    /// Which camera it is seen through.
    pub camera: CameraMode,
}

/// The one cell. See the module header.
static CELL: std::sync::Mutex<Knobs> = std::sync::Mutex::new(Knobs {
    scene: Scene::Courtyard,
    medium: Preset::ClearPool,
    camera: CameraMode::Fixed,
});

/// [`CELL`], with a poisoned lock taken anyway: nothing that holds it can
/// panic, so a poisoned lock means the process is already over, and refusing to
/// answer would turn that into a second failure inside an export a page called.
fn cell() -> std::sync::MutexGuard<'static, Knobs> {
    CELL.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Where the knobs stand now.
#[must_use]
pub fn read() -> Knobs {
    *cell()
}

/// Puts all three knobs at `knobs`, and answers with where they stand.
pub fn set(knobs: Knobs) -> Knobs {
    *cell() = knobs;
    knobs
}

/// Moves the scene on to the next one, and answers with where the knobs stand.
pub fn cycle_scene() -> Knobs {
    let mut knobs = cell();
    knobs.scene = knobs.scene.next();
    *knobs
}

/// Moves the medium on to the next preset.
pub fn cycle_medium() -> Knobs {
    let mut knobs = cell();
    knobs.medium = knobs.medium.next();
    *knobs
}

/// Moves the camera on to the next pose.
pub fn cycle_camera() -> Knobs {
    let mut knobs = cell();
    knobs.camera = knobs.camera.next();
    *knobs
}

/// Every knob back to where a run with no flags opens.
pub fn reset() -> Knobs {
    set(Knobs::default())
}

/// Holds the cell for one test, and puts it back when dropped.
///
/// `cargo test` runs a crate's checks as threads of one process, and this cell
/// is process-global, so a check that moves a knob holds this for as long as it
/// reads one — `apps/sundial/src/filter.rs`' `held`, for the same reason.
#[cfg(test)]
pub(crate) struct Held {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for Held {
    fn drop(&mut self) {
        reset();
    }
}

/// Serialises every check that moves the cell.
#[cfg(test)]
static SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Takes [`SWITCH`] and starts from the defaults.
#[cfg(test)]
pub(crate) fn held() -> Held {
    let guard = SWITCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reset();
    Held { _guard: guard }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Each cycle moves its own knob and no other**, and a reset puts all
    /// three back.
    #[test]
    fn each_cycle_moves_its_own_knob_and_reset_puts_all_three_back() {
        let _held = held();
        let start = read();
        assert_eq!(start, Knobs::default());

        let moved = cycle_scene();
        assert_ne!(moved.scene, start.scene);
        assert_eq!((moved.medium, moved.camera), (start.medium, start.camera));

        let moved = cycle_medium();
        assert_ne!(moved.medium, start.medium);
        assert_ne!(
            moved.scene, start.scene,
            "the medium cycle put the scene back"
        );

        let moved = cycle_camera();
        assert_ne!(moved.camera, start.camera);
        assert_eq!(read(), moved, "a cycle answers with what the cell holds");

        assert_eq!(reset(), Knobs::default());
        assert_eq!(read(), Knobs::default());
    }
}
