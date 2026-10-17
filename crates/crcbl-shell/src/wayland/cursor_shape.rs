//! Cursor shapes through `cursor-shape-v1`.
//!
//! The client names a shape from the CSS vocabulary
//! [`CursorIcon::as_css_name`] already speaks, and the compositor draws the
//! user's own themed cursor at the output's scale. The alternative — loading an
//! XCursor theme and attaching its image to a cursor surface — is a theme loader
//! and a second presentation path inside the shell, and a cursor that disagrees
//! with the user's theme the moment either side changes.
//!
//! One `wp_cursor_shape_device_v1` per seat pointer, created when the pointer
//! appears or the manager is bound, whichever is later, and destroyed before the
//! pointer is released. [`ShellCaps::CURSOR_SHAPE`](crate::ShellCaps::CURSOR_SHAPE)
//! follows the manager having been bound by the end of `open`'s registry round
//! trip.

use crate::CursorIcon;

use super::protocol::cursor_shape::wp_cursor_shape_device_v1::{self, shape};
use super::protocol::cursor_shape::wp_cursor_shape_manager_v1;
use super::{ObjectKind, WaylandShell};

/// See [`COMPOSITOR_VERSION`](super::COMPOSITOR_VERSION). Version 2 adds only
/// shapes, and [`shape_of`] maps no [`CursorIcon`] to any of them, so binding it
/// would gain nothing; a shape added there is one a version-1 manager would
/// refuse with `invalid_shape`, and the unit test below fails on such a mapping.
pub(super) const CURSOR_SHAPE_VERSION: u32 = 1;

/// The protocol's `shape` entry for `icon`: the entry whose name is the icon's
/// CSS keyword.
pub(super) const fn shape_of(icon: CursorIcon) -> u32 {
    match icon {
        CursorIcon::Default => shape::DEFAULT,
        CursorIcon::Pointer => shape::POINTER,
        CursorIcon::Text => shape::TEXT,
        CursorIcon::Crosshair => shape::CROSSHAIR,
        CursorIcon::Wait => shape::WAIT,
        CursorIcon::Progress => shape::PROGRESS,
        CursorIcon::NotAllowed => shape::NOT_ALLOWED,
        CursorIcon::Grab => shape::GRAB,
        CursorIcon::Grabbing => shape::GRABBING,
        CursorIcon::Move => shape::MOVE,
        CursorIcon::ResizeNorthSouth => shape::NS_RESIZE,
        CursorIcon::ResizeEastWest => shape::EW_RESIZE,
        CursorIcon::ResizeNorthEastSouthWest => shape::NESW_RESIZE,
        CursorIcon::ResizeNorthWestSouthEast => shape::NWSE_RESIZE,
        CursorIcon::ResizeColumn => shape::COL_RESIZE,
        CursorIcon::ResizeRow => shape::ROW_RESIZE,
    }
}

impl WaylandShell {
    /// Gives a seat's pointer its shape device, if there is a pointer and a
    /// manager and it has none yet.
    ///
    /// A pointer already over one of our windows gets that window's cursor
    /// straight away: the device can arrive after the `wl_pointer.enter` that
    /// would otherwise have applied it, when the manager is advertised late.
    pub(super) fn add_cursor_shape(&mut self, index: usize) {
        let manager = self.conn.cursor_shape_manager;
        let seat = &self.seats[index];
        if manager.is_null() || seat.pointer.is_null() || !seat.cursor_shape.is_null() {
            return;
        }
        let pointer = seat.pointer;
        // SAFETY: the manager and the pointer are both live on this
        // connection.
        let device = unsafe { wp_cursor_shape_manager_v1::get_pointer(manager, pointer) };
        if device.is_null() {
            return;
        }
        // The device sends no events; see `ObjectKind::Ignored`.
        self.conn.watch(device, ObjectKind::Ignored);
        self.seats[index].cursor_shape = device;
        if let Some(window) = self.seats[index].pointer_focus {
            self.apply_cursor(index, window);
        }
    }

    /// [`add_cursor_shape`](Self::add_cursor_shape) for every seat. Called
    /// after each registry batch, as
    /// [`ensure_data_devices`](Self::ensure_data_devices) is and for its
    /// reason.
    pub(super) fn ensure_cursor_shapes(&mut self) {
        if self.conn.cursor_shape_manager.is_null() {
            return;
        }
        for index in 0..self.seats.len() {
            self.add_cursor_shape(index);
        }
        self.conn.flush();
    }

    /// Destroys a seat's shape device, for a pointer that is going away.
    pub(super) fn remove_cursor_shape(&mut self, index: usize) {
        let device = core::mem::replace(&mut self.seats[index].cursor_shape, core::ptr::null_mut());
        // SAFETY: a null proxy is skipped, a live one is this connection's,
        // and `destroy` is its own destructor.
        unsafe {
            self.conn
                .release(device, wp_cursor_shape_device_v1::destroy)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vendored protocol, read as text so the expected values come from the
    /// XML the build generated from rather than from a second hand-written
    /// table.
    const XML: &str = include_str!("../../wayland-protocols/cursor-shape-v1.xml");

    /// `(value, since)` of the `shape` entry called `name`.
    fn entry(name: &str) -> Option<(u32, u32)> {
        let start = format!("<entry name=\"{name}\" ");
        let line = XML
            .lines()
            .map(str::trim_start)
            .find(|line| line.starts_with(&start))?;
        let attribute = |key: &str| {
            line.split(&format!(" {key}=\""))
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(|value| value.parse::<u32>().expect("a decimal attribute"))
        };
        Some((attribute("value")?, attribute("since").unwrap_or(1)))
    }

    #[test]
    fn every_icon_is_the_entry_its_css_keyword_names_at_the_bound_version() {
        for &icon in CursorIcon::ALL {
            let name = icon.as_css_name().replace('-', "_");
            let (value, since) =
                entry(&name).unwrap_or_else(|| panic!("{icon:?}: no `{name}` entry in the XML"));
            assert_eq!(shape_of(icon), value, "{icon:?} is `{name}`");
            assert!(
                since <= CURSOR_SHAPE_VERSION,
                "{icon:?} is `{name}`, since version {since}, which a manager bound at \
                 {CURSOR_SHAPE_VERSION} refuses with invalid_shape"
            );
        }
        // The reader above finds what it is asked for, including a `since`.
        assert_eq!(entry("dnd_ask"), Some((shape::DND_ASK, 2)));
        assert_eq!(entry("no_such_shape"), None);
    }
}
