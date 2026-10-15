//! What kind of window a [`WindowDesc`] asks for: an
//! application window, a panel, or a popup.
//!
//! Most programs want [`WindowRole::Toplevel`], the window every desktop has.
//! The other two are for desktop furniture — a status bar, a launcher, a
//! notification, a menu opened from a bar — which every window system has a
//! different mechanism for, and which no program should have to write three
//! times:
//!
//! | role | Wayland | Win32 | AppKit |
//! | --- | --- | --- | --- |
//! | [`Panel`](WindowRole::Panel) | `zwlr_layer_surface_v1` | a topmost tool window; an appbar when it reserves an edge | a non-activating `NSPanel` |
//! | [`Popup`](WindowRole::Popup) | `xdg_popup` | a `WS_POPUP` tool window | a child `NSPanel` |
//!
//! Which roles a backend can build is in its caps —
//! [`ShellCaps::PANEL`] and
//! [`ShellCaps::POPUP`] — and asking for one it cannot
//! is [`ShellError::Unsupported`], never a
//! toplevel in its place: a bar that silently became a normal window would
//! cover the desktop it was meant to sit at the edge of.

use crate::geom::LogicalRect;
use crate::monitor::MonitorId;
use crate::window::{WindowDesc, WindowId};
use crate::{ShellBackend, ShellCaps, ShellError};

/// What kind of window to create — see the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum WindowRole<'a> {
    /// An ordinary application window: decorated, in the taskbar, moved and
    /// sized by the user and the window manager.
    #[default]
    Toplevel,
    /// Desktop furniture placed against a monitor's edges — a bar, a dock, a
    /// launcher, an overlay. See [`PanelDesc`].
    Panel(PanelDesc<'a>),
    /// A window anchored to a rectangle of another, above it — a menu, a
    /// tooltip, a calendar opened from a bar. See [`PopupDesc`].
    Popup(PopupDesc),
}

/// Where a [panel](WindowRole::Panel) goes and how it behaves.
///
/// Laid out the way `wlr-layer-shell` describes a surface, because that
/// protocol is the most complete statement of what a panel is: the other
/// window systems implement a subset, and the backend docs say which.
///
/// **Its size is [`WindowDesc::size`], with one
/// exception: along an axis where it is anchored to both opposite edges, that
/// size is ignored and the panel stretches between them.** A bar anchored
/// `TOP | LEFT | RIGHT` with a size of `(0, 50)` is 50 high and as wide as the
/// monitor. Either way the size it actually gets arrives as the first
/// [`Resized`](crate::ShellEvent::Resized), as for any window, and is the one
/// to draw at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelDesc<'a> {
    /// The name window-system rules match the panel by: the layer-shell
    /// namespace on Wayland, which Sway's and Hyprland's per-layer rules key
    /// on. Empty means [`WindowDesc::app_id`].
    pub namespace: &'a str,
    /// The monitor it is placed on, or `None` for the system's choice, which
    /// is **where the user is** rather than the primary monitor: the
    /// compositor's pick on Wayland (the focused output on Sway, Hyprland and
    /// niri), and the monitor under the pointer on Win32 and AppKit. A launcher
    /// opening on a screen the user is not looking at is the failure this
    /// avoids.
    pub monitor: Option<MonitorId>,
    /// Which band of the stack it lives in.
    pub layer: PanelLayer,
    /// The edges it is held against. A panel held against two opposite edges
    /// stretches between them; one held against none is centred.
    pub anchor: PanelAnchor,
    /// Whether, and how, it keeps other windows out of its space.
    pub reserve: Reserve,
    /// Space between the panel and the edges it is anchored to, in logical
    /// units. A margin on an edge it is not anchored to does nothing.
    pub margins: Margins,
    /// Whether it takes the keyboard.
    pub keyboard: PanelKeyboard,
}

/// The band of the stack a [panel](PanelDesc) lives in, bottom to top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PanelLayer {
    /// Below every application window: a wallpaper, a desktop widget.
    Background,
    /// Above the background, below application windows.
    Bottom,
    /// Above application windows, below full-screen ones: a status bar.
    #[default]
    Top,
    /// Above everything, full-screen windows included: a launcher, a lock
    /// screen, a notification.
    Overlay,
}

bitflags::bitflags! {
    /// The monitor edges a [panel](PanelDesc) is held against.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct PanelAnchor: u8 {
        /// The top edge.
        const TOP = 1 << 0;
        /// The bottom edge.
        const BOTTOM = 1 << 1;
        /// The left edge.
        const LEFT = 1 << 2;
        /// The right edge.
        const RIGHT = 1 << 3;
    }
}

/// Whether a [panel](PanelDesc) keeps other windows out of its space.
///
/// **Reserving is a request a window system may be unable to grant**, which is
/// different from being unable to build the panel at all:
/// [`ShellCaps::PANEL_RESERVE`] says whether
/// this backend can. Where it cannot — macOS, which lets nothing but the menu
/// bar and the Dock reserve screen space — the panel is still created and
/// simply overlaps what is behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Reserve {
    /// It reserves nothing, and keeps clear of the space other panels have
    /// reserved — a launcher placed below a bar rather than over it.
    /// Layer-shell's exclusive zone 0.
    #[default]
    None,
    /// The edge it is anchored to gives up the panel's own extent: maximised
    /// windows stop at it. A bar is this. Layer-shell's exclusive zone equal
    /// to the panel's extent across that edge.
    ///
    /// **The panel must name one edge**: anchored to exactly one, or to one and
    /// both of the edges beside it — `TOP`, or `TOP | LEFT | RIGHT`, reserves
    /// the top. Any other anchoring has no edge to reserve, and creating it is
    /// [`ShellError::Unsupported`] on every
    /// backend; see [`PanelDesc::reserved_edge`].
    OwnExtent,
    /// It reserves nothing and is placed as if no other panel reserved
    /// anything — at the very edge, over other bars. Layer-shell's exclusive
    /// zone -1.
    IgnoreOthers,
}

impl PanelDesc<'_> {
    /// The edge [`Reserve::OwnExtent`] would reserve: the one anchored edge,
    /// alone or with both of its neighbours. `None` for any other anchoring,
    /// which has no single edge to give up.
    #[must_use]
    pub fn reserved_edge(&self) -> Option<PanelAnchor> {
        let vertical = PanelAnchor::LEFT | PanelAnchor::RIGHT;
        let horizontal = PanelAnchor::TOP | PanelAnchor::BOTTOM;
        [
            (PanelAnchor::TOP, vertical),
            (PanelAnchor::BOTTOM, vertical),
            (PanelAnchor::LEFT, horizontal),
            (PanelAnchor::RIGHT, horizontal),
        ]
        .into_iter()
        .find(|&(edge, beside)| self.anchor == edge || self.anchor == edge | beside)
        .map(|(edge, _)| edge)
    }
}

/// Space around a [panel](PanelDesc), in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    /// From the top edge.
    pub top: f64,
    /// From the right edge.
    pub right: f64,
    /// From the bottom edge.
    pub bottom: f64,
    /// From the left edge.
    pub left: f64,
}

/// Whether a [panel](PanelDesc) takes the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PanelKeyboard {
    /// Never: a click leaves focus where it was. A bar is this.
    #[default]
    None,
    /// When clicked, like an application window.
    OnDemand,
    /// Takes it on mapping and holds it while mapped: a launcher, a lock
    /// screen. Keys typed the moment it appears are its.
    Exclusive,
}

/// Where a [popup](WindowRole::Popup) goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopupDesc {
    /// The window it belongs to and moves with. Destroying the parent
    /// destroys the popup.
    pub parent: WindowId,
    /// The rectangle it is placed against, in the parent's logical
    /// coordinates — the module or button that opened it.
    pub anchor: LogicalRect,
    /// Which side of the anchor it opens on. The system flips it to the other
    /// side, or slides it along this one, rather than let it leave the screen.
    pub placement: PopupPlacement,
    /// What input it takes while open.
    pub input: PopupInput,
}

/// What input a [popup](PopupDesc) takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PopupInput {
    /// None at all: the pointer passes through it to whatever is beneath, and
    /// it never activates. A tooltip is this, so hovering stays on the bar
    /// that showed it.
    #[default]
    Passthrough,
    /// The pointer over it, as any window has; never the keyboard, and a click
    /// elsewhere leaves it open.
    Pointer,
    /// The pointer and the keyboard while it is open. A click outside it or
    /// `Escape` dismisses it, which arrives as a
    /// [`CloseRequested`](crate::ShellEvent::CloseRequested) for it. A menu or
    /// a calendar is this.
    Grab,
}

/// What every backend checks before it builds a window: that its caps cover
/// the role and the transparency asked for, that a reserved panel names one
/// edge, and that a panel's monitor exists.
///
/// The popup's parent is the backend's to check, since only it knows its
/// windows.
pub(crate) fn check(
    desc: &WindowDesc<'_>,
    caps: ShellCaps,
    backend: ShellBackend,
    has_monitor: impl Fn(MonitorId) -> bool,
) -> Result<(), ShellError> {
    let unsupported = |what| Err(ShellError::Unsupported { backend, what });
    if desc.transparent && !caps.contains(ShellCaps::TRANSPARENT) {
        return unsupported("transparent windows");
    }
    match desc.role {
        WindowRole::Toplevel => Ok(()),
        WindowRole::Panel(panel) => {
            if !caps.contains(ShellCaps::PANEL) {
                return unsupported("panel windows");
            }
            if panel.reserve == Reserve::OwnExtent && panel.reserved_edge().is_none() {
                return unsupported("a reserved panel that is not held against one edge");
            }
            match panel.monitor {
                Some(monitor) if !has_monitor(monitor) => Err(ShellError::NoSuchMonitor(monitor.0)),
                _ => Ok(()),
            }
        }
        WindowRole::Popup(_) if !caps.contains(ShellCaps::POPUP) => unsupported("popup windows"),
        WindowRole::Popup(_) => Ok(()),
    }
}

/// Which side of its anchor a [popup](PopupDesc) opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PopupPlacement {
    /// Below the anchor, left edges aligned.
    #[default]
    Below,
    /// Above the anchor, left edges aligned.
    Above,
    /// To the left of the anchor, top edges aligned.
    Left,
    /// To the right of the anchor, top edges aligned: a submenu.
    Right,
}

/// A rectangle in a backend's own Y-down space — desktop pixels on Win32,
/// flipped screen points on AppKit — for [`place_popup`].
///
/// Compiled where its callers are: the two backends that place popups
/// themselves.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where a popup of `width × height` goes against `anchor`: on the
/// `placement` side of it, flipped to the other side when it does not fit in
/// `area` and the other side does, and slid along the side it is on to stay
/// in `area`.
///
/// The placement `xdg_positioner` is asked for on Wayland, worked out here
/// for the window systems that have no one to ask. Y grows downwards.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub(crate) fn place_popup(
    anchor: Placed,
    (width, height): (f64, f64),
    placement: PopupPlacement,
    area: Placed,
) -> (f64, f64) {
    let (left, top) = (anchor.x, anchor.y);
    let (right, bottom) = (left + anchor.width, top + anchor.height);
    let (area_right, area_bottom) = (area.x + area.width, area.y + area.height);
    // The near start on the placement's axis, or the flip when only that fits.
    let side = |near: f64, far: f64, extent: f64, low: f64, high: f64| {
        if near >= low && near + extent <= high {
            near
        } else if far >= low && far + extent <= high {
            far
        } else {
            near
        }
    };
    let slide = |start: f64, extent: f64, low: f64, high: f64| start.min(high - extent).max(low);
    match placement {
        PopupPlacement::Below => (
            slide(left, width, area.x, area_right),
            side(bottom, top - height, height, area.y, area_bottom),
        ),
        PopupPlacement::Above => (
            slide(left, width, area.x, area_right),
            side(top - height, bottom, height, area.y, area_bottom),
        ),
        PopupPlacement::Left => (
            side(left - width, right, width, area.x, area_right),
            slide(top, height, area.y, area_bottom),
        ),
        PopupPlacement::Right => (
            side(right, left - width, width, area.x, area_right),
            slide(top, height, area.y, area_bottom),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PanelAnchor, PanelDesc, PopupDesc, PopupInput, PopupPlacement, Reserve, WindowRole,
    };
    use crate::{
        HeadlessShell, LogicalRect, MonitorId, Shell, ShellCaps, ShellError, ShellEvent, WindowDesc,
    };

    fn panel(anchor: PanelAnchor) -> PanelDesc<'static> {
        PanelDesc {
            anchor,
            ..PanelDesc::default()
        }
    }

    #[test]
    fn a_reserved_edge_is_one_edge_alone_or_with_both_neighbours() {
        let top = PanelAnchor::TOP;
        let sides = PanelAnchor::LEFT | PanelAnchor::RIGHT;
        assert_eq!(panel(top).reserved_edge(), Some(top));
        assert_eq!(panel(top | sides).reserved_edge(), Some(top));
        let left = PanelAnchor::LEFT;
        let ends = PanelAnchor::TOP | PanelAnchor::BOTTOM;
        assert_eq!(panel(left | ends).reserved_edge(), Some(left));
        assert_eq!(
            panel(PanelAnchor::BOTTOM).reserved_edge(),
            Some(PanelAnchor::BOTTOM)
        );
        // A corner, a centre and an everything name no single edge.
        assert_eq!(panel(top | PanelAnchor::LEFT).reserved_edge(), None);
        assert_eq!(panel(PanelAnchor::empty()).reserved_edge(), None);
        assert_eq!(panel(PanelAnchor::all()).reserved_edge(), None);
        assert_eq!(panel(sides).reserved_edge(), None);
    }

    fn with_role(role: WindowRole<'_>) -> WindowDesc<'_> {
        WindowDesc {
            role,
            ..WindowDesc::default()
        }
    }

    fn refused(result: Result<crate::WindowId, ShellError>) -> &'static str {
        match result {
            Err(ShellError::Unsupported { what, .. }) => what,
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn a_role_or_transparency_the_caps_lack_is_refused_not_downgraded() {
        let mut shell = HeadlessShell::new().with_caps(
            ShellCaps::DESKTOP - (ShellCaps::PANEL | ShellCaps::POPUP | ShellCaps::TRANSPARENT),
        );
        let parent = shell
            .create_window(&WindowDesc::default())
            .expect("a toplevel");
        assert_eq!(
            refused(shell.create_window(&with_role(WindowRole::Panel(PanelDesc::default())))),
            "panel windows"
        );
        let popup = WindowRole::Popup(PopupDesc {
            parent,
            anchor: LogicalRect::default(),
            placement: PopupPlacement::Below,
            input: PopupInput::Grab,
        });
        assert_eq!(
            refused(shell.create_window(&with_role(popup))),
            "popup windows"
        );
        let transparent = WindowDesc {
            transparent: true,
            ..WindowDesc::default()
        };
        assert_eq!(
            refused(shell.create_window(&transparent)),
            "transparent windows"
        );
    }

    #[test]
    fn a_reserved_panel_needs_one_edge_and_a_real_monitor() {
        let mut shell = HeadlessShell::new();
        let cornered = PanelDesc {
            reserve: Reserve::OwnExtent,
            ..panel(PanelAnchor::TOP | PanelAnchor::LEFT)
        };
        assert_eq!(
            refused(shell.create_window(&with_role(WindowRole::Panel(cornered)))),
            "a reserved panel that is not held against one edge"
        );
        let bar = PanelDesc {
            reserve: Reserve::OwnExtent,
            ..panel(PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT)
        };
        shell
            .create_window(&with_role(WindowRole::Panel(bar)))
            .expect("a bar names the top edge");
        let elsewhere = PanelDesc {
            monitor: Some(MonitorId(u32::MAX)),
            ..PanelDesc::default()
        };
        assert!(matches!(
            shell.create_window(&with_role(WindowRole::Panel(elsewhere))),
            Err(ShellError::NoSuchMonitor(_))
        ));
    }

    #[test]
    fn a_popup_needs_a_live_parent_and_goes_with_it() {
        let mut shell = HeadlessShell::new();
        let parent = shell
            .create_window(&WindowDesc::default())
            .expect("a toplevel");
        let popup_of = |parent| {
            WindowRole::Popup(PopupDesc {
                parent,
                anchor: LogicalRect::default(),
                placement: PopupPlacement::Below,
                input: PopupInput::Passthrough,
            })
        };
        let popup = shell
            .create_window(&with_role(popup_of(parent)))
            .expect("a popup");
        let submenu = shell
            .create_window(&with_role(popup_of(popup)))
            .expect("a popup's popup");
        shell.pump(&mut |_| {});

        shell.destroy_window(parent).expect("destroy");
        let mut destroyed = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::WindowDestroyed { window } = event {
                destroyed.push(window);
            }
        });
        assert_eq!(
            destroyed,
            [submenu, popup, parent],
            "children first, each reported"
        );
        assert!(
            shell.create_window(&with_role(popup_of(parent))).is_err(),
            "a dead parent"
        );
    }
}
