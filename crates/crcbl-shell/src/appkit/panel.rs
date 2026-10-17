//! Panels on AppKit: non-activating `NSPanel`s placed by their anchors.
//!
//! What each [`PanelDesc`] field becomes here:
//!
//! | field | AppKit |
//! | --- | --- |
//! | `layer` | a window level from [`geometry::panel_level`]: the desktop picture's for `Background`, just above the desktop icons for `Bottom`, above the menu bar for `Top`, the pop-up menu level for `Overlay`. Every panel is on every space and stays put in Mission Control; only `Overlay` may join a full-screen window's space |
//! | `anchor`, `margins`, the size | a frame on the screen, from [`geometry::panel_frame`], in points |
//! | `reserve` | `None` places the panel in the screen's visible frame, clear of the menu bar and the Dock; `IgnoreOthers` places it on the whole screen. `OwnExtent` is placed as `None` is and reserves nothing: macOS lets nothing but the menu bar and the Dock reserve space, which [`ShellCaps::PANEL_RESERVE`](crate::ShellCaps::PANEL_RESERVE) being clear says |
//! | `keyboard` | `None` is a panel class that can never become the key window, so clicks and hover leave the keyboard alone. `OnDemand` becomes key on a click. `Exclusive` is made key when shown, and [`FocusRefused`](crate::ShellEvent::FocusRefused) follows if it did not become key; nothing takes the keyboard back if the user clicks away, so [`PANEL_EXCLUSIVE_KEYBOARD`](crate::ShellCaps::PANEL_EXCLUSIVE_KEYBOARD) is clear. All are `NSWindowStyleMaskNonactivatingPanel`, so taking the keyboard does not bring the application forward |
//! | `monitor` | the one asked for, else the screen under the pointer |
//! | `namespace` | nothing: macOS has no rules keyed by one |
//!
//! `hidesOnDeactivate` is turned off: an `NSPanel` hides whenever its
//! application stops being active by default, which for a bar is always.
//!
//! A panel's frame is computed again when the screens change, and a panel
//! whose screen was detached is asked to close; see
//! [`ShellEvent::MonitorRemoved`](crate::ShellEvent::MonitorRemoved).

use core::ptr;

use crate::{
    LogicalSize, Margins, MonitorId, PanelAnchor, PanelDesc, PanelKeyboard, PanelLayer, Reserve,
    WindowId,
};

use super::ffi::{self, Id, NSPoint, NSRect, NSSize, Sel};
use super::geometry;
use super::monitors::Screen;
use super::shell::AppKitShell;

/// What a panel was created with, kept so it can be placed again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Panel {
    /// The screen asked for. `None` was the one under the pointer when the
    /// panel was made, and is then whichever one the panel is on.
    pub monitor: Option<MonitorId>,
    pub layer: PanelLayer,
    pub anchor: PanelAnchor,
    pub reserve: Reserve,
    pub margins: Margins,
    pub keyboard: PanelKeyboard,
    /// [`WindowDesc::size`](crate::WindowDesc::size): the size along every
    /// axis that is not stretched, in points.
    pub size: LogicalSize,
}

impl Panel {
    pub(super) fn new(desc: &PanelDesc<'_>, size: LogicalSize) -> Self {
        Self {
            monitor: desc.monitor,
            layer: desc.layer,
            anchor: desc.anchor,
            reserve: desc.reserve,
            margins: desc.margins,
            keyboard: desc.keyboard,
            size,
        }
    }

    /// Its frame on `screen`, in AppKit points with Y up.
    pub(super) fn frame_on(&self, screen: &Screen) -> NSRect {
        let area = match self.reserve {
            Reserve::IgnoreOthers => screen.frame,
            Reserve::None | Reserve::OwnExtent => screen.visible,
        };
        geometry::panel_frame(
            area,
            self.anchor,
            NSSize::new(self.size.width, self.size.height),
            self.margins,
        )
    }
}

/// Whether `point` is inside `rect`, both in AppKit points: the left and
/// bottom edges in, the right and top edges out, as `NSPointInRect` has it.
fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}

impl AppKitShell {
    /// The screen `panel` belongs on: the one it asked for, else the one under
    /// the pointer — where the user is, which is what a launcher has to open
    /// on — else the menu-bar screen.
    pub(super) fn panel_screen(&self, panel: &Panel) -> Option<Screen> {
        if let Some(id) = panel.monitor {
            return self.screens.iter().find(|screen| screen.id == id).copied();
        }
        let under = ffi::class(c"NSEvent").map(|class| {
            // SAFETY: `+[NSEvent mouseLocation]` takes nothing and answers an
            // `NSPoint` in screen points, Y up; on the main thread.
            unsafe {
                let location: unsafe extern "C" fn(Id, Sel) -> NSPoint = ffi::msg_send();
                location(class.cast(), ffi::sel(c"mouseLocation"))
            }
        });
        under
            .and_then(|point| {
                self.screens
                    .iter()
                    .find(|screen| contains(screen.frame, point))
            })
            .or_else(|| self.screens.first())
            .copied()
    }

    /// Gives a just-created panel window its level, its place in spaces, and
    /// the one `NSPanel` default a panel must not keep.
    ///
    /// # Safety
    ///
    /// `window` must be a live `NSPanel`, on the main thread.
    pub(super) unsafe fn configure_panel(window: Id, panel: &Panel) {
        let (key, above) = geometry::panel_level(panel.layer);
        // SAFETY: a pure query of the window server.
        let level = unsafe { ffi::CGWindowLevelForKey(key) } as ffi::NSInteger + above;
        // SAFETY: the caller guarantees a live panel; each setter takes the
        // type it is given here.
        unsafe {
            ffi::msg_set_isize(window, ffi::sel(c"setLevel:"), level);
            ffi::msg_set_usize(
                window,
                ffi::sel(c"setCollectionBehavior:"),
                geometry::panel_collection(panel.layer),
            );
            ffi::msg_set_bool(window, ffi::sel(c"setHidesOnDeactivate:"), false);
            // A panel that takes the keyboard on a click, and not before.
            ffi::msg_set_bool(
                window,
                ffi::sel(c"setBecomesKeyOnlyIfNeeded:"),
                panel.keyboard == PanelKeyboard::OnDemand,
            );
        }
    }

    /// Shows a panel: made key if it takes the keyboard outright, otherwise
    /// brought to the front without becoming key.
    ///
    /// `false` when an exclusive panel asked to become key and AppKit did not
    /// make it so; the caller reports it as
    /// [`FocusRefused`](crate::ShellEvent::FocusRefused). Judged by reading
    /// `isKeyWindow` back after `makeKeyAndOrderFront:`, as Win32 asks who
    /// holds the foreground after `SetForegroundWindow` rather than trusting
    /// the request.
    ///
    /// # Safety
    ///
    /// `window` must be a live panel of this shell, on the main thread.
    #[must_use]
    pub(super) unsafe fn show_panel(window: Id, keyboard: PanelKeyboard) -> bool {
        // SAFETY: the caller guarantees a live window; `nil` is the
        // documented sender, `orderFrontRegardless` takes nothing, and
        // `isKeyWindow` returns a `BOOL`.
        unsafe {
            match keyboard {
                PanelKeyboard::Exclusive => {
                    ffi::msg1_void(window, ffi::sel(c"makeKeyAndOrderFront:"), ptr::null_mut());
                    ffi::msg_bool(window, ffi::sel(c"isKeyWindow"))
                }
                PanelKeyboard::None | PanelKeyboard::OnDemand => {
                    ffi::msg_void(window, ffi::sel(c"orderFrontRegardless"));
                    true
                }
            }
        }
    }

    /// Places every panel again after the screens changed, and asks each
    /// panel created for a screen in `removed` to close.
    pub(super) fn place_panels(&mut self, removed: &[MonitorId]) {
        let panels: Vec<(WindowId, Panel)> = self
            .windows_iter()
            .filter_map(|(handle, window)| Some((handle, window.panel?)))
            .collect();
        for (window, panel) in panels {
            if panel
                .monitor
                .is_some_and(|monitor| removed.contains(&monitor))
            {
                self.close_requested(window);
                continue;
            }
            self.place_panel(window, &panel);
        }
    }

    /// Moves one panel to where it belongs now. The resize it causes is
    /// reported as any other, through `windowDidResize:`.
    fn place_panel(&self, handle: WindowId, panel: &Panel) {
        let Ok(state) = self.window(handle) else {
            return;
        };
        let window = state.window;
        let screen = match panel.monitor {
            Some(_) => self.panel_screen(panel),
            None => self
                .screen_of(window)
                .and_then(|id| self.screens.iter().find(|screen| screen.id == id).copied()),
        };
        let Some(screen) = screen else { return };
        let frame = panel.frame_on(&screen);
        // SAFETY: a live window of this shell; `setFrame:display:` takes an
        // `NSRect` and a `BOOL`.
        unsafe {
            let set: unsafe extern "C" fn(Id, Sel, NSRect, ffi::ObjcBool) = ffi::msg_send();
            set(window, ffi::sel(c"setFrame:display:"), frame, ffi::YES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_inside_a_rect_by_its_left_and_bottom_edges() {
        let rect = NSRect::new(-1440.0, 0.0, 1440.0, 900.0);
        assert!(contains(rect, NSPoint { x: -1440.0, y: 0.0 }));
        assert!(contains(rect, NSPoint { x: -0.5, y: 899.5 }));
        assert!(
            !contains(rect, NSPoint { x: 0.0, y: 10.0 }),
            "the right edge is out"
        );
        assert!(
            !contains(rect, NSPoint { x: -10.0, y: 900.0 }),
            "the top edge is out"
        );
    }
}
