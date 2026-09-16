//! Where the panels are, and where that is kept between runs.
//!
//! [`DockLayout`] is a value — nested splits with a named pane at each leaf and
//! each divider's position — that [`crcbl::ui::tree::Ui::dock`] both reads and
//! writes, so the layout on screen *is* the value the editor holds and saving
//! it is saving that value. This module is the two ends of that: the layout the
//! editor opens with, and the settings key it is written to and read back from.
//!
//! # Where it is kept, and why there
//!
//! `crcbl::store::settings` — the player's own `settings.toml`, under
//! [`APP_NAME`] in this platform's config directory, reached through
//! [`crcbl::engine::SettingsSource`] so that a **headless run writes nothing**:
//! a golden run or a test may not take a layout from, or leave one in,
//! whichever home directory it happens to execute in. That is the same rule the
//! engine's video settings follow and the same seam `apps/options` writes
//! through, so the editor gains no storage path of its own.
//!
//! # …as RON text, in one key
//!
//! The value under [`LAYOUT_KEY`] is a **string holding the layout as RON**,
//! not a nest of TOML tables. A layout is a recursive tree and TOML is a
//! language of tables; RON is what this workspace already writes a structured
//! value in — a `.scn/` chunk, a camera stack, an inventory catalogue — so a
//! layout reads the same way in a settings file as it would anywhere else, and
//! a person who has made a mess of the panels can delete one line.
//!
//! [`SavedLayout`] is that RON's shape, mirroring [`DockLayout`] field for
//! field. A mirror rather than a derive on the widget's own type because
//! `crcbl-ui` has no serde dependency and this is not the reason to give it
//! one; the dock's own documentation says the application persists the value
//! "with whatever serializer it already has".
//!
//! # A layout read back is checked before it is used
//!
//! [`load`] refuses anything that is not exactly this editor's three panes.
//! A saved layout that lost the viewport — an older build's, a hand-edited
//! file, a future build's with a fourth pane — would otherwise leave the editor
//! with no viewport and no way to get one back, and the pane names are the only
//! thing the dock cannot rebuild for itself.

use crcbl::serde::{Deserialize, Serialize};
use crcbl::store::StorageError;
use crcbl::store::settings::SettingsStack;
use crcbl::ui::tree::{DockLayout, SplitAxis};

/// The name this tool keeps its settings under: `settings.toml` in this
/// platform's config directory for it.
///
/// Prefixed, unlike the samples' bare `options` and `breakout`, because the
/// editor is a tool a person installs rather than a demo they run out of the
/// workspace — and `~/.config/editor` is a directory name no program should
/// claim.
pub const APP_NAME: &str = "crcbl-editor";

/// The dotted settings key the layout is written to.
pub const LAYOUT_KEY: &str = "editor.layout";

/// The pane the scene is drawn in: a hole the panels leave, and the rectangle a
/// click has to land in to pick.
pub const VIEWPORT: &str = "viewport";

/// The pane holding the scene's entities.
pub const OUTLINER: &str = "outliner";

/// The pane holding the selected entity's fields.
pub const INSPECTOR: &str = "inspector";

/// Every pane this build knows, which is what a layout read back must hold —
/// see the module docs.
pub const PANES: [&str; 3] = [VIEWPORT, OUTLINER, INSPECTOR];

/// How little a pane may be dragged to, along and across its split, in pixels.
///
/// Wide enough for the inspector's narrowest row — a label and three
/// drag-values, each with `default.css`'s 32-pixel minimum — and tall enough
/// for a handful of outliner rows, so a divider dragged to the end leaves a
/// pane that is still a pane rather than a line.
pub const PANE_MIN: [f32; 2] = [160.0, 64.0];

/// How wide the side column starts, in pixels.
///
/// The **first** child of the outer split, which is what a [`DockLayout`]
/// position can say: the side column keeps this width when the window is
/// resized and the viewport takes the difference, which is the way round a
/// person wants it.
pub const SIDE_WIDTH: f32 = 240.0;

/// The layout a build with nothing saved opens on: the outliner over the
/// inspector in a column down the left, the viewport taking the rest.
#[must_use]
pub fn default_layout() -> DockLayout {
    DockLayout::Split {
        axis: SplitAxis::Row,
        position: Some(SIDE_WIDTH),
        first: Box::new(DockLayout::split(
            SplitAxis::Column,
            DockLayout::pane(OUTLINER),
            DockLayout::pane(INSPECTOR),
        )),
        second: Box::new(DockLayout::pane(VIEWPORT)),
    }
}

/// A [`DockLayout`] as it is written down; see the module docs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "crcbl::serde")]
pub enum SavedLayout {
    /// One pane, by name.
    Pane(String),
    /// Two layouts either side of a divider.
    Split {
        /// Which way they are laid out.
        axis: SavedAxis,
        /// The first child's length along `axis` in pixels, or [`None`] while
        /// the two share the space equally.
        position: Option<f32>,
        /// The left one of a row, the top one of a column.
        first: Box<SavedLayout>,
        /// The other.
        second: Box<SavedLayout>,
    },
}

/// A [`SplitAxis`] as it is written down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "crcbl::serde")]
pub enum SavedAxis {
    /// Side by side.
    Row,
    /// One above the other.
    Column,
}

impl From<SplitAxis> for SavedAxis {
    fn from(axis: SplitAxis) -> Self {
        match axis {
            SplitAxis::Row => Self::Row,
            SplitAxis::Column => Self::Column,
        }
    }
}

impl From<SavedAxis> for SplitAxis {
    fn from(axis: SavedAxis) -> Self {
        match axis {
            SavedAxis::Row => Self::Row,
            SavedAxis::Column => Self::Column,
        }
    }
}

impl From<&DockLayout> for SavedLayout {
    fn from(layout: &DockLayout) -> Self {
        match layout {
            DockLayout::Pane(name) => Self::Pane(name.clone()),
            DockLayout::Split {
                axis,
                position,
                first,
                second,
            } => Self::Split {
                axis: (*axis).into(),
                position: *position,
                first: Box::new(Self::from(&**first)),
                second: Box::new(Self::from(&**second)),
            },
        }
    }
}

impl From<&SavedLayout> for DockLayout {
    fn from(layout: &SavedLayout) -> Self {
        match layout {
            SavedLayout::Pane(name) => Self::Pane(name.clone()),
            SavedLayout::Split {
                axis,
                position,
                first,
                second,
            } => Self::Split {
                axis: (*axis).into(),
                position: *position,
                first: Box::new(Self::from(&**first)),
                second: Box::new(Self::from(&**second)),
            },
        }
    }
}

/// `layout` as the RON text a settings file holds.
///
/// One line: a settings value is read beside scalars, and a layout pretty-printed
/// over twenty lines inside a TOML string is a value nobody can read in either
/// language.
///
/// # Errors
///
/// [`crcbl::ron::Error`] if the layout would not serialize, which for this
/// shape means a non-finite divider position.
pub fn to_ron(layout: &DockLayout) -> Result<String, crcbl::ron::Error> {
    crcbl::ron::to_string(&SavedLayout::from(layout))
}

/// A layout from the RON text [`to_ron`] wrote.
///
/// # Errors
///
/// [`crcbl::ron::error::SpannedError`] if the text is not this shape.
pub fn from_ron(text: &str) -> Result<DockLayout, crcbl::ron::error::SpannedError> {
    crcbl::ron::from_str::<SavedLayout>(text).map(|saved| DockLayout::from(&saved))
}

/// Whether `layout` holds exactly this build's panes, each once.
#[must_use]
pub fn is_this_editors(layout: &DockLayout) -> bool {
    let mut panes = layout.panes();
    panes.sort_unstable();
    let mut wanted = PANES;
    wanted.sort_unstable();
    panes == wanted
}

/// The layout `stack` holds, or [`None`] where it holds none this build can
/// use.
///
/// Text that is not a layout, and a layout that is not this editor's panes, are
/// both **logged and discarded** rather than refused: a settings file is a
/// thing people edit, and a tool that would not start because of one is worse
/// than a tool that starts with its default panels. See the module docs.
#[must_use]
pub fn load(stack: &SettingsStack) -> Option<DockLayout> {
    let text: String = stack.get(LAYOUT_KEY)?;
    let layout = match from_ron(&text) {
        Ok(layout) => layout,
        Err(error) => {
            crcbl::log::warn!("editor: {LAYOUT_KEY} is not a layout ({error}); using the default");
            return None;
        }
    };
    if !is_this_editors(&layout) {
        crcbl::log::warn!(
            "editor: {LAYOUT_KEY} holds the panes {:?}, and this build has {PANES:?}; \
             using the default",
            layout.panes(),
        );
        return None;
    }
    Some(layout)
}

/// Writes `layout` into `stack` under [`LAYOUT_KEY`].
///
/// Writing it is not saving it: [`crcbl::engine::SettingsSource::save`] is what
/// puts the stack on disk, and what answers whether there was anywhere to put
/// it.
///
/// # Errors
///
/// [`StorageError`] if the stack has no writable layer, or if the layout would
/// not serialize.
pub fn store(stack: &mut SettingsStack, layout: &DockLayout) -> Result<(), StorageError> {
    let text = to_ron(layout).map_err(|error| StorageError::Other(error.to_string()))?;
    stack.set(LAYOUT_KEY, &text)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::store::MemoryStorage;
    use crcbl::ui::tree::DockSide;

    /// A stack over memory, which is what a headless run gets and what a test
    /// wants: writable, and nowhere near a home directory.
    fn stack() -> SettingsStack {
        SettingsStack::from_storage(&MemoryStorage::new())
    }

    /// **The default layout is this build's three panes**, and the side column
    /// is the first child of the outer split so that a resize moves the
    /// viewport rather than the panels.
    #[test]
    fn the_default_layout_is_this_builds_panes_with_the_side_column_fixed() {
        let layout = default_layout();
        assert_eq!(layout.panes(), [OUTLINER, INSPECTOR, VIEWPORT]);
        assert!(is_this_editors(&layout));
        let DockLayout::Split {
            axis,
            position,
            first,
            ..
        } = &layout
        else {
            panic!("the default layout is a split");
        };
        assert_eq!(*axis, SplitAxis::Row);
        assert_eq!(
            *position,
            Some(SIDE_WIDTH),
            "the side column has no width of its own, so a resize would share it",
        );
        assert_eq!(first.panes(), [OUTLINER, INSPECTOR], "{first:?}");
    }

    /// **A layout survives a save and a restore**, dividers included — the
    /// whole claim, through the settings stack a run actually writes to rather
    /// than through the text on its own.
    #[test]
    fn a_layout_round_trips_through_a_settings_stack() {
        let storage = MemoryStorage::new();
        let mut moved = default_layout();
        // A layout somebody has worked on: a dragged divider, and the inspector
        // moved to the other side of the viewport.
        if let DockLayout::Split { position, .. } = &mut moved {
            *position = Some(311.5);
        }
        assert!(
            moved.move_pane(INSPECTOR, VIEWPORT, DockSide::Right),
            "the inspector would not move",
        );
        assert_ne!(moved, default_layout());

        let mut saved = SettingsStack::from_storage(&storage);
        store(&mut saved, &moved).expect("a stack over memory is writable");
        saved
            .save(&storage, std::path::Path::new("settings.toml"))
            .expect("memory takes a write");

        let restored = load(&SettingsStack::from_storage(&storage));
        assert_eq!(
            restored,
            Some(moved),
            "the layout did not come back as it was written",
        );
    }

    /// A stack with no layout key has no layout, which is a first run.
    #[test]
    fn an_empty_settings_stack_has_no_layout() {
        assert_eq!(load(&stack()), None);
    }

    /// **A layout that is not this build's panes is discarded**, which is what
    /// keeps a hand-edited or an older file from leaving the editor with no
    /// viewport to click in. Each half is refused on its own: text that is not
    /// a layout, and a layout whose panes are not these.
    #[test]
    fn a_layout_this_build_cannot_use_is_discarded() {
        let mut stack = stack();
        stack
            .set(LAYOUT_KEY, &"Pane(".to_owned())
            .expect("writable");
        assert_eq!(load(&stack), None, "text that is not RON was accepted");

        let short = DockLayout::split(
            SplitAxis::Row,
            DockLayout::pane(OUTLINER),
            DockLayout::pane(INSPECTOR),
        );
        assert!(!is_this_editors(&short));
        store(&mut stack, &short).expect("writable");
        assert_eq!(load(&stack), None, "a layout with no viewport was accepted");

        let extra = {
            let mut layout = default_layout();
            assert!(layout.dock("notes", VIEWPORT, DockSide::Bottom));
            layout
        };
        assert!(!is_this_editors(&extra));
        store(&mut stack, &extra).expect("writable");
        assert_eq!(load(&stack), None, "a pane this build has no builder for");

        store(&mut stack, &default_layout()).expect("writable");
        assert_eq!(
            load(&stack),
            Some(default_layout()),
            "and a layout this build can use is still taken",
        );
    }

    /// The RON is one line, so a settings file stays readable beside its
    /// scalars.
    #[test]
    fn the_saved_text_is_one_line() {
        let text = to_ron(&default_layout()).expect("a finite layout");
        assert!(!text.contains('\n'), "{text}");
        assert_eq!(
            from_ron(&text).expect("what we just wrote"),
            default_layout()
        );
    }
}
