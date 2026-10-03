//! The thing being edited: a scene, the world it was loaded into, what is
//! selected, and the history of what has been done to it.
//!
//! **Moved to `crcbl::scene_edit`** (decided 2026-10-04), so that the editor,
//! the `crcbl` CLI and the edit server share one implementation; re-exported
//! here so the editor's own paths do not churn. That module's docs hold the
//! design. What stays here is this build's half: the compiled-in document is
//! [`crate::scene::built_in_document`], and the tests below open documents
//! through this build's vocabulary — the games' components and the editor's
//! own greybox block, none of which the umbrella can name.

pub use crcbl::scene_edit::*;

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use crcbl::assets::{AssetSource, MemorySource};
#[cfg(test)]
use crcbl::ecs::{Entity, World};
#[cfg(test)]
use crcbl::math::{DVec3, Vec3};
#[cfg(test)]
use crcbl::phys::{PhysicsSystem, Ray};
#[cfg(test)]
use crcbl::reflect::{Value, set_path};
#[cfg(test)]
use crcbl::registry::Registry;
#[cfg(test)]
use crcbl::scene::scn::{NameError, SceneEntityId, ScnError};
#[cfg(test)]
use crcbl::ui::tree::{FieldEdit, VariantEdit};

#[cfg(test)]
use crate::command::{EditCommand, SystemRow};

/// Simulation space's `f64`, from render space's `f32` — the document's own
/// widening, which a test spells its expected values in.
#[cfg(test)]
fn widen(value: Vec3) -> DVec3 {
    DVec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

/// Render space's `f32`, from simulation space's `f64` — the document's own
/// narrowing, which a test spells its expected bounds in.
#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: DVec3) -> Vec3 {
    Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

/// `files`, keyed the way a source rooted at the scene directory reads them —
/// a scene in memory, for a test that loads saved text with no directory.
#[cfg(test)]
fn memory_source(files: BTreeMap<String, String>) -> Result<MemorySource, EditError> {
    let mut source = MemorySource::new();
    for (key, text) in files {
        source
            .insert(Path::new(&key), text.into_bytes())
            .map_err(|source| EditError::Write { key, source })?;
    }
    Ok(source)
}

#[cfg(test)]
mod dirty_tests;

#[cfg(test)]
mod entity_tests;

#[cfg(test)]
mod environment_tests;

#[cfg(test)]
mod field_tests;

#[cfg(test)]
pub(crate) mod mesh_tests;

#[cfg(test)]
mod naming_tests;

#[cfg(test)]
pub(crate) mod origin_tests;

#[cfg(test)]
pub(crate) mod physics_tests;

#[cfg(test)]
pub(crate) mod play_tests;

#[cfg(test)]
pub(crate) mod rotation_tests;

#[cfg(test)]
mod save_tests;

#[cfg(test)]
pub(crate) mod systems_tests;

#[cfg(test)]
mod selection_tests;

#[cfg(test)]
mod towers_play_tests;

#[cfg(test)]
mod undo_property_tests;

#[cfg(test)]
mod validation_tests;

#[cfg(test)]
mod variant_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod tests;
