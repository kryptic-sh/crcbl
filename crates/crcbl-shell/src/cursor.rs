//! Pointer capture modes and cursor shapes.
//!
//! # Three pointer modes, and why "hidden" is not one of them
//!
//! [`PointerMode`] is about *where the pointer may go*; visibility is a
//! separate axis, set through [`Shell::set_cursor`](crate::Shell::set_cursor)
//! with `None`. Collapsing the two — a `Hidden` mode — is tempting and wrong:
//! a text editor hides the cursor while typing without capturing it, and a
//! strategy game confines the pointer to the window while keeping it perfectly
//! visible. Two axes, two settings.
//!
//! # Cursor shapes are a fixed enum, not an image
//!
//! [`CursorIcon`] names shapes rather than carrying pixels because every
//! platform already has themed, correctly-scaled, user-configured cursors and
//! shipping our own would mean shipping them at every scale factor and ignoring
//! the user's theme. Custom cursor images are a real feature for an editor
//! (drag-and-drop ghosts) and are deliberately out of scope here; they can be
//! added as a separate call that takes a pixel buffer without disturbing this
//! enum.
//!
//! The names follow the CSS cursor keywords, which is the same vocabulary the
//! Wayland `cursor-shape-v1` protocol adopted and which every X11 cursor theme
//! is indexed by.

/// Where the pointer is allowed to go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerMode {
    /// Normal: the pointer moves freely and can leave the window.
    #[default]
    Free,

    /// The pointer stays inside the window but keeps moving visibly.
    ///
    /// What a strategy game's edge-scrolling wants. Requires
    /// [`ShellCaps::POINTER_CONFINE`](crate::ShellCaps::POINTER_CONFINE).
    Confined,

    /// The pointer is pinned in place and only relative motion is reported.
    ///
    /// What a first-person camera wants. Requires
    /// [`ShellCaps::POINTER_LOCK`](crate::ShellCaps::POINTER_LOCK), and in
    /// practice also
    /// [`ShellCaps::RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION)
    /// — see [`ShellCaps::has_mouselook`](crate::ShellCaps::has_mouselook).
    ///
    /// While locked, [`ShellEvent::PointerMotion`](crate::ShellEvent::PointerMotion)
    /// carries `abs: None`: there is no meaningful absolute position, and
    /// reporting the frozen one would make anything that reads it appear to
    /// work until the day it does not.
    Locked,
}

impl PointerMode {
    /// Whether this mode constrains the pointer at all.
    #[inline]
    #[must_use]
    pub const fn is_captured(self) -> bool {
        !matches!(self, Self::Free)
    }

    /// The capability a shell must report for this mode to be settable.
    ///
    /// [`Free`](Self::Free) needs none, so it returns an empty set — which is
    /// what makes `caps.contains(mode.required_cap())` correct for all three
    /// without a special case at the call site.
    #[inline]
    #[must_use]
    pub const fn required_cap(self) -> crate::ShellCaps {
        match self {
            Self::Free => crate::ShellCaps::empty(),
            Self::Confined => crate::ShellCaps::POINTER_CONFINE,
            Self::Locked => crate::ShellCaps::POINTER_LOCK,
        }
    }

    /// What this mode is called in a
    /// [`ShellError::Unsupported`](crate::ShellError::Unsupported).
    ///
    /// Every backend needs this string in
    /// [`set_pointer_mode`](crate::Shell::set_pointer_mode) and each one used
    /// to spell the table again, with an `unreachable!()` for
    /// [`Free`](Self::Free) on the grounds that it needs no capability. That
    /// reasoning is correct today and is a panic the day a mode is added, so
    /// the function is total instead.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "a free pointer",
            Self::Confined => "pointer confinement",
            Self::Locked => "pointer lock",
        }
    }
}

/// A standard cursor shape, defined in [`crcbl_core::input`] so the UI can name
/// one without depending on the shell.
pub use crcbl_core::input::CursorIcon;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShellCaps;

    #[test]
    fn capture_modes_declare_the_cap_they_need() {
        assert!(!PointerMode::Free.is_captured());
        assert!(PointerMode::Confined.is_captured());
        assert!(PointerMode::Locked.is_captured());

        // The property that makes the check uniform at the call site.
        for mode in [
            PointerMode::Free,
            PointerMode::Confined,
            PointerMode::Locked,
        ] {
            assert!(ShellCaps::DESKTOP.contains(mode.required_cap()));
        }
        assert!(ShellCaps::empty().contains(PointerMode::Free.required_cap()));
        assert!(!ShellCaps::POINTER_CONFINE.contains(PointerMode::Locked.required_cap()));
        assert_eq!(PointerMode::default(), PointerMode::Free);
    }
}
