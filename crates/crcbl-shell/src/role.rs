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
#[cfg(any(target_os = "windows", target_os = "linux", test))]
use crate::{PhysicalRect, PhysicalSize};
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
    ///
    /// Win32 has one topmost band, which `Top` uses too, so there an overlay
    /// is above application windows and shares the band with full-screen
    /// ones that are topmost themselves.
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
    /// Takes it on mapping: a launcher, a lock screen. Keys typed the moment
    /// it appears are its, or
    /// [`FocusRefused`](crate::ShellEvent::FocusRefused) says the window
    /// system would not hand it over.
    ///
    /// Held while mapped only where
    /// [`ShellCaps::PANEL_EXCLUSIVE_KEYBOARD`](crate::ShellCaps::PANEL_EXCLUSIVE_KEYBOARD)
    /// is set. Elsewhere the user can click another window and the panel
    /// hears [`Focus`](crate::ShellEvent::Focus) with `focused: false`;
    /// nothing takes the keyboard back, since that would fight the user and
    /// the system's foreground rules.
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
    ///
    /// `Escape` is still delivered: the popup hears the
    /// [`Key`](crate::ShellEvent::Key) press first and the `CloseRequested`
    /// after it, so a consumer can tell a dismissal by the keyboard from one by
    /// a click, and can answer [`CloseReply::Keep`](crate::CloseReply::Keep)
    /// to back out of a submenu instead of closing the whole menu. Only a
    /// fresh press dismisses — an auto-repeat or a release does not.
    Grab,
}

/// Whether `key`, delivered to a window whose popup input is `popup`, asks
/// for that window to close: a fresh press of `Escape` on a
/// [`PopupInput::Grab`] popup, as that variant documents. `popup` is `None`
/// for a window that is not a popup.
///
/// `Escape` is the key's **symbol**, [`Keysym::ESCAPE`](crcbl_core::input::Keysym::ESCAPE),
/// not its physical key: an xkb remap such as `caps:escape` dismisses the
/// popup, and a virtual keyboard whose own keymap puts Down on the scancode a
/// physical Escape has (`wtype` uses evdev 1) does not.
///
/// Every backend that builds popups calls this after queueing the key, and on
/// `true` queues the window's
/// [`CloseRequested`](crate::ShellEvent::CloseRequested) through its own
/// close-request path, so a request already pending is not repeated.
pub(crate) fn escape_dismisses(popup: Option<PopupInput>, key: &crate::ShellEvent) -> bool {
    popup == Some(PopupInput::Grab)
        && matches!(
            key,
            crate::ShellEvent::Key {
                keysym: crcbl_core::input::Keysym::ESCAPE,
                state: crcbl_core::input::ButtonState::Pressed,
                repeat: false,
                ..
            }
        )
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
        WindowRole::Panel(panel) => check_panel(&panel, caps, backend, has_monitor),
        WindowRole::Popup(_) if !caps.contains(ShellCaps::POPUP) => unsupported("popup windows"),
        WindowRole::Popup(_) => Ok(()),
    }
}

/// The panel half of [`check`], which
/// [`Shell::set_panel`](crate::Shell::set_panel) applies to a panel's new
/// descriptor too, so a panel cannot be reconfigured into one it could not
/// have been created as.
fn check_panel(
    panel: &PanelDesc<'_>,
    caps: ShellCaps,
    backend: ShellBackend,
    has_monitor: impl Fn(MonitorId) -> bool,
) -> Result<(), ShellError> {
    let unsupported = |what| Err(ShellError::Unsupported { backend, what });
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

/// What every backend checks before it reconfigures a panel with
/// [`Shell::set_panel`](crate::Shell::set_panel): that the window is a panel,
/// whose descriptor is `was`, that the new descriptor keeps its namespace and
/// its monitor, and that it passes the checks a new panel would.
///
/// The namespace and the monitor are fixed by `wlr-layer-shell`, which names
/// both in `get_layer_surface` and has no request to change either. Refusing
/// a change on every backend, rather than only where the protocol does, keeps
/// a panel that works on one system from failing on another.
pub(crate) fn check_set_panel(
    window: WindowId,
    was: Option<PanelDesc<'_>>,
    panel: &PanelDesc<'_>,
    caps: ShellCaps,
    backend: ShellBackend,
    has_monitor: impl Fn(MonitorId) -> bool,
) -> Result<(), ShellError> {
    let Some(was) = was else {
        return Err(not_a_panel(window));
    };
    if panel.namespace != was.namespace {
        return Err(ShellError::InvalidDescriptor(
            "set_panel: a panel's namespace is fixed when it is created".to_string(),
        ));
    }
    if panel.monitor != was.monitor {
        return Err(ShellError::InvalidDescriptor(
            "set_panel: a panel's monitor is fixed when it is created".to_string(),
        ));
    }
    check_panel(panel, caps, backend, has_monitor)
}

/// What [`Shell::set_panel`](crate::Shell::set_panel) answers for a window
/// that is not a panel, in every backend's words alike.
pub(crate) fn not_a_panel(window: WindowId) -> ShellError {
    ShellError::InvalidDescriptor(format!("set_panel: {window:?} is not a panel"))
}

/// A [`PanelDesc`] kept past the call that gave it, for the backends that
/// have no panel record of their own: the namespace owned, the rest as given.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeptPanel {
    namespace: String,
    /// The descriptor with an empty namespace, which [`desc`](Self::desc)
    /// puts back.
    desc: PanelDesc<'static>,
}

impl KeptPanel {
    pub(crate) fn new(desc: &PanelDesc<'_>) -> Self {
        Self {
            namespace: desc.namespace.to_string(),
            desc: PanelDesc {
                namespace: "",
                ..*desc
            },
        }
    }

    /// The descriptor as it was given.
    pub(crate) fn desc(&self) -> PanelDesc<'_> {
        PanelDesc {
            namespace: &self.namespace,
            ..self.desc
        }
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
/// Compiled where its callers are: the backends that place popups
/// themselves.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux", test))]
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
/// What [`Shell::reposition_popup`](crate::Shell::reposition_popup) answers
/// for a window that is not a popup, in every backend's words alike.
pub(crate) fn not_a_popup(window: WindowId) -> ShellError {
    ShellError::InvalidDescriptor(format!("reposition_popup: {window:?} is not a popup"))
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux", test))]
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

#[cfg(any(target_os = "windows", target_os = "linux", test))]
/// Where a popup of `size` goes against `anchor`, both in desktop pixels:
/// [`place_popup`]'s placement, flipped or
/// slid to stay in `area`, in whole pixels.
#[must_use]
pub(crate) fn popup_rect(
    anchor: PhysicalRect,
    size: PhysicalSize,
    placement: PopupPlacement,
    area: PhysicalRect,
) -> PhysicalRect {
    let placed = |rect: PhysicalRect| Placed {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
        width: f64::from(rect.width),
        height: f64::from(rect.height),
    };
    let (x, y) = place_popup(
        placed(anchor),
        (f64::from(size.width), f64::from(size.height)),
        placement,
        placed(area),
    );
    // Whole pixels in, whole pixels out: every input is an integer, so the
    // arithmetic is exact and the cast only narrows the range.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped into i32's range before the cast"
    )]
    let narrow = |value: f64| value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    PhysicalRect::new(narrow(x), narrow(y), size.width, size.height)
}

#[cfg(test)]
mod tests {
    use super::{
        PanelAnchor, PanelDesc, PopupDesc, PopupInput, PopupPlacement, Reserve, WindowRole,
    };
    use crate::{
        HeadlessShell, LogicalRect, MonitorId, Shell, ShellCaps, ShellError, ShellEvent, WindowDesc,
    };
    use crate::{PhysicalRect, PhysicalSize};

    #[test]
    fn a_popup_opens_on_its_side_and_flips_or_slides_to_stay_on_screen() {
        let area = PhysicalRect::new(0, 0, 1920, 1040);
        let button = PhysicalRect::new(100, 100, 80, 30);
        let menu = PhysicalSize::new(200, 300);
        assert_eq!(
            super::popup_rect(button, menu, PopupPlacement::Below, area),
            PhysicalRect::new(100, 130, 200, 300),
            "below, left edges aligned"
        );
        assert_eq!(
            super::popup_rect(button, menu, PopupPlacement::Above, area),
            PhysicalRect::new(100, 130, 200, 300),
            "no room above, so flipped below"
        );
        assert_eq!(
            super::popup_rect(button, menu, PopupPlacement::Right, area),
            PhysicalRect::new(180, 100, 200, 300),
            "a submenu, top edges aligned"
        );
        assert_eq!(
            super::popup_rect(button, menu, PopupPlacement::Left, area),
            PhysicalRect::new(180, 100, 200, 300),
            "no room to the left, so flipped right"
        );
        let low = PhysicalRect::new(1850, 1000, 60, 30);
        assert_eq!(
            super::popup_rect(low, menu, PopupPlacement::Below, area),
            PhysicalRect::new(1720, 700, 200, 300),
            "flipped above, and slid back onto the screen"
        );
        let tall = PhysicalSize::new(200, 2000);
        assert_eq!(
            super::popup_rect(button, tall, PopupPlacement::Below, area).y,
            130,
            "fits neither side, so it stays on its own"
        );
    }

    #[test]
    fn only_a_fresh_escape_press_on_a_grabbing_popup_dismisses_it() {
        use crcbl_core::KeyCode;
        use crcbl_core::input::{ButtonState, DeviceId, Keysym, Modifiers, Scancode};

        let window = crate::WindowId::from_bits(1 << 32).expect("a valid handle");
        let key = |key_code, keysym, state, repeat| ShellEvent::Key {
            window,
            device: DeviceId(1),
            time: crcbl_core::EventTime::ZERO,
            scancode: Scancode(0),
            key_code,
            keysym,
            state,
            repeat,
            modifiers: Modifiers::empty(),
        };
        let pressed = ButtonState::Pressed;
        let escape = key(Some(KeyCode::Escape), Keysym::ESCAPE, pressed, false);
        let grab = Some(PopupInput::Grab);
        assert!(super::escape_dismisses(grab, &escape));
        // The symbol decides: Caps Lock remapped to Escape (`caps:escape`)
        // dismisses, and wtype's Down on evdev 1, Escape's scancode, does not.
        let remapped = key(Some(KeyCode::CapsLock), Keysym::ESCAPE, pressed, false);
        assert!(super::escape_dismisses(grab, &remapped));
        let wtype_down = key(Some(KeyCode::Escape), Keysym::DOWN, pressed, false);
        assert!(!super::escape_dismisses(grab, &wtype_down));

        // Not a release, a repeat, another key, or a key with no symbol.
        let released = key(
            Some(KeyCode::Escape),
            Keysym::ESCAPE,
            ButtonState::Released,
            false,
        );
        let repeated = key(Some(KeyCode::Escape), Keysym::ESCAPE, pressed, true);
        let enter = key(Some(KeyCode::Enter), Keysym::RETURN, pressed, false);
        let unnamed = key(None, Keysym::NONE, pressed, false);
        for other in [&released, &repeated, &enter, &unnamed] {
            assert!(!super::escape_dismisses(grab, other), "{other:?}");
        }
        // And not on a popup that does not hold the keyboard, or on a window
        // that is no popup at all.
        for input in [
            Some(PopupInput::Pointer),
            Some(PopupInput::Passthrough),
            None,
        ] {
            assert!(!super::escape_dismisses(input, &escape), "{input:?}");
        }
        // Only a key event is a keystroke.
        assert!(!super::escape_dismisses(
            grab,
            &ShellEvent::CloseRequested { window }
        ));
    }

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
