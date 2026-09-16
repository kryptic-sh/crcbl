//! Dockable layouts: a tree of nested splits the application owns, with its
//! panes addressed by name.
//!
//! # The layout is a value, not the widget's state
//!
//! [`DockLayout`] is a plain tree of [`SplitAxis`] splits and named panes,
//! carrying **each divider's position**, that the application holds between
//! frames and [`Ui::dock`] both reads and writes. That is what makes a layout
//! saveable: the value is the save format, so an editor persists a layout with
//! whatever serializer it already has, and restoring is assigning the value
//! back. A position kept in the store instead — where [`Ui::split`] keeps its
//! own — is dropped the moment a frame does not build the split, which is
//! exactly what happens while another layout is showing.
//!
//! The plan fixes docking at splitters ("Split panes via flex + dividers only;
//! full docking is the classic time sink"), and this is that: nesting,
//! dragging, and moving a pane from one dock to another as a value operation
//! ([`DockLayout::move_pane`]). What it is **not** is a drag-to-dock gesture —
//! see that method's docs for what one would still need.
//!
//! # Panes
//!
//! Each pane is a `.dock-pane` block keyed by its name and made a focus scope
//! root, as `07-ui-debug.md` says a pane is, so focus remembers where it was in
//! each pane and a directional move resumes there. A name appearing twice is a
//! duplicate key: the tree warns and derives a key, and the layout's own
//! editing methods refuse to make one.

use std::panic::Location;

use super::{Ui, typed};
use crate::style::PseudoClasses;
use crate::tree::{Behavior, Response, SplitAxis};

/// Which side of a pane another pane docks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockSide {
    /// To its left: a row split, the moved pane first.
    Left,
    /// To its right: a row split, the moved pane second.
    Right,
    /// Above it: a column split, the moved pane first.
    Top,
    /// Below it: a column split, the moved pane second.
    Bottom,
}

impl DockSide {
    /// The axis a split for this side is laid out along.
    #[must_use]
    pub const fn axis(self) -> SplitAxis {
        match self {
            Self::Left | Self::Right => SplitAxis::Row,
            Self::Top | Self::Bottom => SplitAxis::Column,
        }
    }

    /// Whether the moved pane becomes the split's first child.
    #[must_use]
    pub const fn is_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// A dock layout: nested splits with named panes at the leaves, and each
/// divider's position. See the module docs.
#[derive(Clone, Debug, PartialEq)]
pub enum DockLayout {
    /// One pane, by the name [`Ui::dock`] passes back to its builder.
    Pane(String),
    /// Two layouts side by side along `axis`, a divider between them.
    Split {
        /// Which way the two are laid out.
        axis: SplitAxis,
        /// The first child's length along `axis` in pixels, or `None` while the
        /// two share the space equally. [`Ui::dock`] writes a drag back here.
        position: Option<f32>,
        /// The first child: the left one of a row, the top one of a column.
        first: Box<DockLayout>,
        /// The second child.
        second: Box<DockLayout>,
    },
}

impl DockLayout {
    /// One pane called `name`.
    #[must_use]
    pub fn pane(name: &str) -> Self {
        Self::Pane(name.to_owned())
    }

    /// `first` and `second` either side of a divider along `axis`, sharing the
    /// space equally until something moves it.
    #[must_use]
    pub fn split(axis: SplitAxis, first: Self, second: Self) -> Self {
        Self::Split {
            axis,
            position: None,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// Every pane's name, in the order [`Ui::dock`] builds them.
    #[must_use]
    pub fn panes(&self) -> Vec<&str> {
        let mut names = Vec::new();
        self.walk(&mut |name| names.push(name));
        names
    }

    /// Whether the layout holds a pane called `name`.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.panes().contains(&name)
    }

    /// Calls `visit` with every pane's name, in build order.
    fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a str)) {
        match self {
            Self::Pane(name) => visit(name),
            Self::Split { first, second, .. } => {
                first.walk(visit);
                second.walk(visit);
            }
        }
    }

    /// Takes the pane called `name` out, collapsing the split that held it into
    /// its sibling. Returns whether it was there.
    ///
    /// Refuses to empty the layout: removing the only pane leaves it alone and
    /// answers `false`, because a layout with no pane has nothing to build.
    pub fn remove_pane(&mut self, name: &str) -> bool {
        if matches!(self, Self::Pane(own) if own == name) {
            return false;
        }
        let Self::Split { first, second, .. } = self else {
            return false;
        };
        for near_first in [true, false] {
            let (near, far) = if near_first {
                (&**first, &**second)
            } else {
                (&**second, &**first)
            };
            if matches!(near, Self::Pane(own) if own == name) {
                let far = far.clone();
                *self = far;
                return true;
            }
        }
        first.remove_pane(name) || second.remove_pane(name)
    }

    /// Puts a pane called `name` on `side` of the pane called `beside`,
    /// splitting it. Returns false — changing nothing — when `beside` is not in
    /// the layout, when `name` already is, or when the two names are the same.
    pub fn dock(&mut self, name: &str, beside: &str, side: DockSide) -> bool {
        if name == beside || self.holds(name) || !self.holds(beside) {
            return false;
        }
        self.dock_into(name, beside, side)
    }

    /// [`Self::dock`] once its names are known to be dockable.
    fn dock_into(&mut self, name: &str, beside: &str, side: DockSide) -> bool {
        match self {
            Self::Pane(own) if own == beside => {
                let moved = Self::pane(name);
                let target = Self::Pane(own.clone());
                *self = if side.is_first() {
                    Self::split(side.axis(), moved, target)
                } else {
                    Self::split(side.axis(), target, moved)
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                first.dock_into(name, beside, side) || second.dock_into(name, beside, side)
            }
        }
    }

    /// Moves the pane called `name` to `side` of the pane called `beside`:
    /// [`Self::remove_pane`] then [`Self::dock`], applied together so a refused
    /// move changes nothing. Returns whether it moved.
    ///
    /// **This is the layout half of docking, not a gesture.** A drag-to-dock
    /// would still need three things this does not have: a drag source (a pane
    /// header or a tab the press latches on, which the tree reports through
    /// [`Response::pressed`] but nothing yet routes as a drag payload), a drop
    /// target resolved from the pointer against last frame's pane rectangles
    /// into a side — the five-zone hit test every docking UI uses — and a
    /// preview the frame draws over the pane the drop would take. Each is
    /// input and drawing work over this method, not a change to it.
    pub fn move_pane(&mut self, name: &str, beside: &str, side: DockSide) -> bool {
        if name == beside || !self.holds(name) || !self.holds(beside) {
            return false;
        }
        let mut moved = self.clone();
        if !moved.remove_pane(name) || !moved.dock(name, beside, side) {
            return false;
        }
        *self = moved;
        true
    }
}

impl Ui {
    /// A dock layout: a `dock` block holding the nested `split` blocks
    /// `layout` describes, with a `.dock-pane` block per pane — a focus scope
    /// root, keyed by the pane's name — that `pane` fills with that name.
    ///
    /// `min` is every pane's minimum length along its split, in pixels, as
    /// [`Ui::split`] takes it. Dragging a divider, or stepping it while
    /// engaged, writes the new position back into `layout`, so the value the
    /// caller holds is always the layout as shown and is what a save writes.
    /// Returns the block's [`Response`], with [`Response::changed`] set the
    /// frame any divider moved.
    #[track_caller]
    pub fn dock(
        &mut self,
        selector: &str,
        layout: &mut DockLayout,
        min: [f32; 2],
        mut pane: impl FnMut(&mut Self, &str),
    ) -> Response {
        let selector = typed("dock", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let mut changed = false;
        let mut response = self.open_block(
            key,
            parsed,
            &[],
            Behavior::NONE,
            PseudoClasses::NONE,
            |ui| ui.dock_node(layout, min, &mut pane, &mut changed),
        );
        response.changed = changed;
        response
    }

    /// One node of a dock layout, inside its parent: a split, or the pane at a
    /// leaf. The two splits of one layout are never built under one parent, so
    /// this one call site keys them all apart.
    fn dock_node(
        &mut self,
        layout: &mut DockLayout,
        min: [f32; 2],
        pane: &mut dyn FnMut(&mut Self, &str),
        changed: &mut bool,
    ) {
        match layout {
            DockLayout::Pane(name) => {
                let name = name.clone();
                self.block_keyed_with(name.as_str(), ".dock-pane", &[], Behavior::SCOPE, |ui| {
                    pane(ui, &name)
                });
            }
            DockLayout::Split {
                axis,
                position,
                first,
                second,
            } => {
                let mut moved = false;
                let divider = self.split_at_indexed("", *axis, min, position, |ui, which| {
                    let child = if which == 0 {
                        &mut **first
                    } else {
                        &mut **second
                    };
                    ui.dock_node(child, min, pane, &mut moved);
                });
                *changed |= moved || divider.changed;
            }
        }
    }
}
