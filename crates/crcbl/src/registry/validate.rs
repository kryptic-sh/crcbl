//! A registered component's own rule over one row: see [`Validate`].

use std::fmt;

use crcbl_reflect::Reflect;
use crcbl_scene::scn::RowRule;

use super::rotation;

/// What values of a component a scene may hold, beyond what its type can:
/// a mass above zero, an asset key inside its root.
///
/// **One rule, run at every door a value comes in by.** The registry runs it
///
/// * on **load**, on every row of a chunk file and on every row's text an
///   edit attaches (a paste, a spawn, an undone delete), through the codec
///   [`Registry::codecs`](super::Registry::codecs) builds — so a refusal is
///   [`ScnError::Parse`](crcbl_scene::scn::ScnError::Parse) with the file,
///   the line and the column;
/// * on **edit**, through [`Registry::validate`](super::Registry::validate),
///   which a tool calls after it writes a property and before it records the
///   write;
/// * on **save**, through [`Registry::problems`](super::Registry::problems),
///   which reads every listed chunk back through the same codec — so what it
///   reports is exactly what the next load would refuse.
///
/// The three call one private function over the row, so they cannot
/// disagree.
///
/// # Every type states it
///
/// A bound on [`Registry::register`](super::Registry::register), as
/// [`Placement`](super::Placement) is, so a component registered for a tool
/// says whether it has a rule — `impl Validate for Brick {}` is the statement
/// that it has none. The method is provided, unlike `placement`, because "no
/// rule" is the common answer and is spelled by the empty impl rather than by
/// a body every such type would repeat.
///
/// # A rule over one row, never over the scene
///
/// The rule sees one component and nothing else. A rule that needs the rest of
/// the scene — towers' path, whose legs must meet at right angles — is a
/// [`SceneCheck`](super::SceneCheck), run at save and never per edit: an edit
/// is one step of authoring, and a layout passes through states its game
/// would refuse (a corner placed before the leg it bends) on the way to one
/// it takes.
///
/// # Rotations are checked for every component
///
/// Before the component's own rule, every [`Rotation`](super::Rotation) it
/// holds is checked, found by type through its reflected fields — nested
/// ones and list items included — so a component carrying one has nothing to
/// state for it. A file's rotation is refused by its own type as it is read;
/// this is what catches one a property write left off unit.
pub trait Validate {
    /// `Ok` for a value a scene may hold, or the first field that it may not.
    ///
    /// # Errors
    ///
    /// [`FieldError`] naming the field and why.
    fn validate(&self) -> Result<(), FieldError> {
        Ok(())
    }
}

/// A component value a scene may not hold: which field, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// The field's dotted path in the component, as a property write names it
    /// — `mass`, or `rotation` for all four of a rotation's leaves.
    pub field: String,
    /// Why, in words a person reads: what a load refusal and a status line
    /// say, naming the field as they read it.
    pub message: String,
}

impl FieldError {
    /// `field` refused for `reason`.
    pub fn new(field: impl Into<String>, reason: impl fmt::Display) -> Self {
        Self {
            field: field.into(),
            message: reason.to_string(),
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FieldError {}

/// The one rule over a row: its rotations, then its own [`Validate`] — what
/// load, edit and save all run.
pub(super) fn check_row<T: Validate + Reflect>(row: &T) -> Result<(), FieldError> {
    if let Some(fault) = rotation::fault_in(row) {
        return Err(fault);
    }
    row.validate()
}

/// [`check_row`] as the rule a registered component's codec reads with.
pub(super) struct Validated;

impl<T: Validate + Reflect> RowRule<T> for Validated {
    fn check(row: &T) -> Result<(), String> {
        check_row(row).map_err(|error| error.to_string())
    }
}
