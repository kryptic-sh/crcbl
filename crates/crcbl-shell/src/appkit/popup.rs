//! Popups on AppKit: child panels placed against a rectangle of their parent.
//!
//! A [`WindowRole::Popup`](crate::WindowRole::Popup) is one of the
//! [`panel`](super::panel) classes, borderless and non-activating, added to
//! its parent with `addChildWindow:ordered:` when shown: a child window moves
//! with its parent and stays above it. Its frame is worked out here by
//! [`geometry::popup_frame`] from the [`PopupDesc::anchor`] in the parent's
//! content view, flipped or slid to stay in the visible frame of the parent's
//! screen.
//!
//! [`PopupInput`] becomes:
//!
//! * `Passthrough` — a class that never becomes key, with
//!   `ignoresMouseEvents`, so the pointer reaches what is beneath.
//! * `Pointer` — a class that never becomes key.
//! * `Grab` — a class that can, made key when shown, and asked to close when
//!   it stops being key: a click outside it makes something else key.

use core::ptr;

use crate::{PopupDesc, PopupInput, ShellError};

use super::ffi::{self, Id, NSRect, NSSize, Sel};
use super::geometry;
use super::shell::AppKitShell;

/// `NSWindowAbove`, as `addChildWindow:ordered:` takes it.
const ORDERED_ABOVE: ffi::NSInteger = 1;

impl AppKitShell {
    /// Where a popup of `size` goes; see the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a parent that is not a live window.
    pub(super) fn popup_frame_for(
        &self,
        popup: &PopupDesc,
        size: NSSize,
    ) -> Result<NSRect, ShellError> {
        let parent = self.window(popup.parent)?.window;
        // SAFETY: a live window of this shell; `frame` answers an `NSRect`,
        // and `contentRectForFrameRect:` takes and answers one.
        let content = unsafe {
            let frame = ffi::msg_rect(parent, ffi::sel(c"frame"));
            let content_of: unsafe extern "C" fn(Id, Sel, NSRect) -> NSRect = ffi::msg_send_stret();
            content_of(parent, ffi::sel(c"contentRectForFrameRect:"), frame)
        };
        let anchor = popup.anchor;
        // The anchor is measured down from the content's top-left; AppKit's Y
        // is up, so its top edge is the content's top minus its offset.
        let anchor = NSRect::new(
            content.origin.x + anchor.origin.x,
            content.max_y() - anchor.origin.y - anchor.size.height,
            anchor.size.width,
            anchor.size.height,
        );
        let visible = self
            .screen_of(parent)
            .and_then(|id| self.screens.iter().find(|screen| screen.id == id))
            .or_else(|| self.screens.first())
            .map_or(content, |screen| screen.visible);
        Ok(geometry::popup_frame(
            anchor,
            size,
            popup.placement,
            visible,
        ))
    }

    /// The one `NSPanel` default a popup must not keep, and the pointer for a
    /// passthrough one.
    ///
    /// # Safety
    ///
    /// `window` must be a live `NSPanel`, on the main thread.
    pub(super) unsafe fn configure_popup(window: Id, input: PopupInput) {
        // SAFETY: the caller guarantees a live panel; both setters take a
        // `BOOL`.
        unsafe {
            ffi::msg_set_bool(window, ffi::sel(c"setHidesOnDeactivate:"), false);
            ffi::msg_set_bool(
                window,
                ffi::sel(c"setIgnoresMouseEvents:"),
                input == PopupInput::Passthrough,
            );
        }
    }

    /// Shows a popup as its parent's child window: made key if it grabs,
    /// otherwise brought to the front without becoming key.
    ///
    /// # Safety
    ///
    /// Both must be live windows of this shell, on the main thread.
    pub(super) unsafe fn show_popup(parent: Id, window: Id, input: PopupInput) {
        // SAFETY: the caller guarantees both windows; `nil` is the documented
        // sender, and `addChildWindow:ordered:` takes a window and an
        // `NSWindowOrderingMode`.
        unsafe {
            if input == PopupInput::Grab {
                ffi::msg1_void(window, ffi::sel(c"makeKeyAndOrderFront:"), ptr::null_mut());
            } else {
                ffi::msg_void(window, ffi::sel(c"orderFrontRegardless"));
            }
            let add: unsafe extern "C" fn(Id, Sel, Id, ffi::NSInteger) = ffi::msg_send();
            add(
                parent,
                ffi::sel(c"addChildWindow:ordered:"),
                window,
                ORDERED_ABOVE,
            );
        }
    }
}
