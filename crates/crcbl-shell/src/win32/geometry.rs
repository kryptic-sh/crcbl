//! The arithmetic half of the Win32 backend: styles, frames, track sizes and
//! the aspect-locked drag rectangle.
//!
//! Everything in here is a **pure function of numbers the system handed us**,
//! and that is why it is a module of its own rather than five helpers scattered
//! through [`window`](super::window) and [`proc`](super::proc). Two reasons,
//! and the second is the load-bearing one:
//!
//! 1. The window procedure runs inside the system's own modal loops — a drag
//!    resize, a menu — where nothing of ours can allocate, log or fail. Keeping
//!    the arithmetic it needs in functions that touch no handle makes that
//!    property visible instead of hoped for.
//! 2. **It is the part of this backend that can be tested without Windows.**
//!    This module compiles and its tests run on every host, so a mistake in the
//!    aspect-lock algebra or in the logical-to-physical conversion is caught by
//!    `cargo test -p crcbl-shell` on the machine the engine is developed on,
//!    rather than by a CI runner nobody is watching. What genuinely needs a
//!    desktop — that `AdjustWindowRectExForDpi` returns the frame these
//!    functions are *given* — is tested on Windows, in [`window`](super::window).
//!
//! # Client pixels, window pixels, and the one that is neither
//!
//! Three coordinate spaces meet here and mixing them is the classic Win32 bug:
//!
//! * **Client** pixels are the drawable area, and are what the seam means by
//!   [`PhysicalSize`] everywhere — the swapchain's extent.
//! * **Window** pixels include the frame and the title bar. `WM_GETMINMAXINFO`
//!   and `WM_SIZING` are both in this space, which is why a minimum size that
//!   forgot the frame is a window a few dozen pixels smaller than it claimed.
//! * **Logical** units are the seam's request space; [`SizeConstraints`] is in
//!   them, and the conversion needs the scale factor of the monitor the window
//!   is *on*, which on Win32 is genuinely per-window.
//!
//! [`Frame`] is the difference between the first two, and it is a number the
//! system computes rather than one to derive: the border width depends on the
//! style, the DPI and the user's theme.

use super::ffi::{Point, Rect, dwm, style, value};
use crate::{
    AspectRatio, DisplayMode, PanelAnchor, PanelKeyboard, PanelLayer, PhysicalRect, PhysicalSize,
    PopupInput, PopupPlacement, SizeConstraints,
};

/// On every window this backend makes: without them the system may draw over
/// the client area while a swapchain is presenting into it.
const COMMON: u32 = style::CLIP_SIBLINGS | style::CLIP_CHILDREN;

/// The pixels a window rectangle has that its client rectangle does not.
///
/// Obtained from `AdjustWindowRectExForDpi` against an empty rectangle, so it
/// already accounts for the style, the extended style and the DPI. Borderless
/// windows have a zero frame, which is not a special case anywhere below — it
/// falls out of `WS_POPUP` having no border to add.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// Extra width: left border plus right border.
    pub width: i32,
    /// Extra height: top border plus caption plus bottom border.
    pub height: i32,
}

impl Frame {
    /// A window size large enough to hold `client`.
    pub const fn window_size(self, client: (i32, i32)) -> (i32, i32) {
        (client.0 + self.width, client.1 + self.height)
    }
}

/// The two style words a window is created with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Styles {
    /// `WS_*`.
    pub style: u32,
    /// `WS_EX_*`.
    pub ex_style: u32,
}

/// The styles for a display mode.
///
/// # Borderless is a style change, not a mode the system has
///
/// Windows has no "fullscreen" state to ask for — the windowing rules in
/// `docs/notes/backends.md` drop exclusive fullscreen, and the *borderless* kind is simply a window
/// with `WS_POPUP` positioned over a monitor. That has a consequence the two
/// Linux backends do not share and [`set_mode`](crate::Shell::set_mode) has to
/// account for: there is no window manager to refuse, so the effective mode is
/// known the moment the call returns.
///
/// [`COMMON`] is on both, as every rendering window sets it.
///
/// A non-resizable window loses `WS_THICKFRAME` and `WS_MAXIMIZEBOX` rather
/// than being clamped by a constraint — [`WindowDesc::resizable`](crate::WindowDesc::resizable)
/// documents that the difference from `min == max` is exactly this: the resize
/// handles and the maximize button disappear.
#[must_use]
pub fn styles(mode: DisplayMode, resizable: bool) -> Styles {
    match mode {
        DisplayMode::Windowed => {
            let mut window = style::OVERLAPPED_WINDOW;
            if !resizable {
                window &= !(style::THICK_FRAME | style::MAXIMIZE_BOX);
            }
            Styles {
                style: window | COMMON,
                ex_style: style::EX_APP_WINDOW | style::EX_WINDOW_EDGE,
            }
        }
        // No caption, no border, no system menu — and no `WS_EX_WINDOW_EDGE`,
        // which would draw a one-pixel raised edge around a window that is
        // meant to be flush with the monitor.
        DisplayMode::Borderless { .. } => Styles {
            style: style::POPUP | COMMON,
            ex_style: style::EX_APP_WINDOW,
        },
    }
}

/// The styles of a [panel](crate::WindowRole::Panel).
///
/// No frame, and a tool window, so no taskbar button and no place in Alt-Tab.
/// The top two layers are topmost, which is as high as Win32 goes, so
/// [`PanelLayer::Overlay`] and [`PanelLayer::Top`] share one band. A panel
/// that takes no keyboard is never activated by a click, so hovering and
/// clicking it leave the keyboard where it was.
#[must_use]
pub fn panel_styles(layer: PanelLayer, keyboard: PanelKeyboard) -> Styles {
    let mut ex_style = style::EX_TOOL_WINDOW;
    if matches!(layer, PanelLayer::Top | PanelLayer::Overlay) {
        ex_style |= style::EX_TOPMOST;
    }
    if keyboard == PanelKeyboard::None {
        ex_style |= style::EX_NO_ACTIVATE;
    }
    Styles {
        style: style::POPUP | COMMON,
        ex_style,
    }
}

/// The styles of a [popup](crate::WindowRole::Popup): no frame, out of the
/// taskbar, topmost when its parent is, and never activated unless it grabs.
#[must_use]
pub fn popup_styles(input: PopupInput, topmost: bool) -> Styles {
    let mut ex_style = style::EX_TOOL_WINDOW;
    if topmost {
        ex_style |= style::EX_TOPMOST;
    }
    if input != PopupInput::Grab {
        ex_style |= style::EX_NO_ACTIVATE;
    }
    Styles {
        style: style::POPUP | COMMON,
        ex_style,
    }
}

/// `styles` for a window that is, or is not,
/// [transparent](crate::WindowDesc::transparent).
///
/// A transparent window adds `WS_EX_NOREDIRECTIONBITMAP`, whatever its role:
/// DWM then keeps no redirection surface for it, and what it shows is the
/// DirectComposition visual its D3D12 swapchain is bound to, blended by the
/// frame's premultiplied alpha. Nothing else changes, because the frame is the
/// role's: a panel and a popup are already frameless, and a windowed toplevel
/// keeps the caption [`SERVER_DECORATIONS`](crate::ShellCaps::SERVER_DECORATIONS)
/// promises.
#[must_use]
pub const fn with_transparency(styles: Styles, transparent: bool) -> Styles {
    if transparent {
        Styles {
            style: styles.style,
            ex_style: styles.ex_style | style::EX_NO_REDIRECTION_BITMAP,
        }
    } else {
        styles
    }
}

/// What DWM draws around a transparent window: its corners and its border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chrome {
    /// A `DWMWCP_*` value for `DWMWA_WINDOW_CORNER_PREFERENCE`.
    pub corners: u32,
    /// A colour or `DWMWA_COLOR_*` value for `DWMWA_BORDER_COLOR`.
    pub border: u32,
}

/// The chrome a transparent window of `styles` is given.
///
/// One with no caption — a panel, a popup, a borderless toplevel — draws its
/// own shape, so DWM is asked for square corners and no border, which on
/// Windows 11 would otherwise clip the shape and outline it. One with a caption
/// keeps the system's corners and border, because the system frame is what the
/// role asked for and the two belong together. Asked again after a mode
/// change, so a toplevel that goes borderless and back ends where it started.
///
/// There is no shadow to take away: DWM shadows a window with a frame, or one
/// whose class asks for `CS_DROPSHADOW`, and a frameless window of this
/// backend is neither. `DWMWA_NCRENDERING_POLICY` is left alone for the same
/// reason: a frameless window has no non-client area for DWM to render, and
/// turning it off on a captioned one would take away the caption.
#[must_use]
pub const fn transparent_chrome(styles: Styles) -> Chrome {
    if styles.style & style::CAPTION == 0 {
        Chrome {
            corners: dwm::CORNER_DO_NOT_ROUND,
            border: dwm::COLOR_NONE,
        }
    } else {
        Chrome {
            corners: dwm::CORNER_DEFAULT,
            border: dwm::COLOR_DEFAULT,
        }
    }
}

/// Space around a panel, in physical pixels: [`Margins`](crate::Margins)
/// scaled to the monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    /// From the top edge.
    pub top: i32,
    /// From the right edge.
    pub right: i32,
    /// From the bottom edge.
    pub bottom: i32,
    /// From the left edge.
    pub left: i32,
}

/// A logical length in whole pixels at `scale`: rounded, and clamped into
/// `0..=i32::MAX`, with a non-finite length read as zero. Unlike
/// [`LogicalSize::to_physical`](crate::LogicalSize::to_physical), zero stays
/// zero, which a margin needs.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped into i32's range before the cast, and a NaN casts to 0"
)]
pub fn pixels(logical: f64, scale: f64) -> i32 {
    (logical * scale).round().clamp(0.0, f64::from(i32::MAX)) as i32
}

/// `rect` cut to `depth` from its `edge`: the far side is moved and the side
/// against the edge is kept.
///
/// What an appbar does twice: once to propose a strip of its monitor, and
/// again after `ABM_QUERYPOS`, which may have moved the near side off the
/// taskbar or another appbar and left the far side wherever it put it.
#[must_use]
pub fn with_depth(rect: Rect, edge: PanelAnchor, depth: i32) -> Rect {
    let mut rect = rect;
    match edge {
        PanelAnchor::BOTTOM => rect.top = rect.bottom.saturating_sub(depth),
        PanelAnchor::LEFT => rect.right = rect.left.saturating_add(depth),
        PanelAnchor::RIGHT => rect.left = rect.right.saturating_sub(depth),
        _ => rect.bottom = rect.top.saturating_add(depth),
    }
    rect
}

/// Where a popup of `size` goes against `anchor`, both in desktop pixels:
/// [`role::place_popup`](crate::role::place_popup)'s placement, flipped or
/// slid to stay in `area`, in whole pixels.
#[must_use]
pub fn popup_rect(
    anchor: PhysicalRect,
    size: PhysicalSize,
    placement: PopupPlacement,
    area: PhysicalRect,
) -> PhysicalRect {
    let placed = |rect: PhysicalRect| crate::role::Placed {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
        width: f64::from(rect.width),
        height: f64::from(rect.height),
    };
    let (x, y) = crate::role::place_popup(
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

/// Where a panel of `size` goes in `area`: `insets` away from each edge it is
/// anchored to, stretched between two opposite anchored edges, and centred on
/// an axis anchored to neither. That is `wlr-layer-shell`'s placement, which
/// [`PanelDesc`](crate::PanelDesc) is written in.
#[must_use]
pub fn panel_rect(
    area: PhysicalRect,
    anchor: PanelAnchor,
    size: PhysicalSize,
    insets: Insets,
) -> PhysicalRect {
    let (x, width) = place_on_axis(
        (area.x, area.width),
        (
            anchor.contains(PanelAnchor::LEFT),
            anchor.contains(PanelAnchor::RIGHT),
        ),
        (insets.left, insets.right),
        size.width,
    );
    let (y, height) = place_on_axis(
        (area.y, area.height),
        (
            anchor.contains(PanelAnchor::TOP),
            anchor.contains(PanelAnchor::BOTTOM),
        ),
        (insets.top, insets.bottom),
        size.height,
    );
    PhysicalRect::new(x, y, width, height)
}

/// One axis of [`panel_rect`]: where a panel of `length` starts along an
/// area's `(start, length)`, and how long it is, given which ends it is
/// anchored to and its insets from them.
fn place_on_axis(
    (area_start, area_length): (i32, u32),
    (at_start, at_end): (bool, bool),
    (inset_start, inset_end): (i32, i32),
    length: u32,
) -> (i32, u32) {
    let area_length = i64::from(area_length);
    let length = i64::from(length);
    let (offset, length) = match (at_start, at_end) {
        (true, true) => (
            i64::from(inset_start),
            (area_length - i64::from(inset_start) - i64::from(inset_end)).max(1),
        ),
        (true, false) => (i64::from(inset_start), length),
        (false, true) => (area_length - i64::from(inset_end) - length, length),
        (false, false) => ((area_length - length) / 2, length),
    };
    let start = i64::from(area_start) + offset;
    (
        i32::try_from(start).unwrap_or(if start < 0 { i32::MIN } else { i32::MAX }),
        u32::try_from(length).unwrap_or(u32::MAX),
    )
}

/// The interactive resize limits a `WM_GETMINMAXINFO` reports, in **window**
/// pixels.
///
/// `None` in either field means "the system's own default", which for the
/// maximum is the virtual screen and for the minimum is roughly the width of
/// the caption buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// Smallest the user may drag to.
    pub min: Option<Point>,
    /// Largest the user may drag to.
    pub max: Option<Point>,
}

/// [`SizeConstraints`] converted into the space `WM_GETMINMAXINFO` answers in.
///
/// Two conversions, in this order: logical to physical at the window's own
/// scale factor, then client to window by adding the frame. Skipping the
/// second is the bug this function exists to not have — a 320×240 minimum
/// would become a window whose *client area* can be dragged down to about
/// 300×200, and the renderer's own minimum would then be violated by a window
/// that reported honouring it.
///
/// [`SizeConstraints::aspect`] is deliberately not represented here: an aspect
/// ratio is not a limit on a size, it is a relation between two, and Win32
/// expresses it in `WM_SIZING` instead. See [`adjust_sizing_rect`].
#[must_use]
pub fn track_sizes(constraints: SizeConstraints, scale_factor: f64, frame: Frame) -> Track {
    let convert = |logical: crate::LogicalSize| {
        let physical = logical.to_physical(scale_factor);
        let (width, height) = frame.window_size((
            i32::try_from(physical.width).unwrap_or(i32::MAX),
            i32::try_from(physical.height).unwrap_or(i32::MAX),
        ));
        Point {
            x: width,
            y: height,
        }
    };
    Track {
        min: constraints.min.map(convert),
        max: constraints.max.map(convert),
    }
}

/// The drag rectangle of a `WM_SIZING`, corrected to an aspect ratio.
///
/// This is the Win32 half of
/// [`ShellCaps::ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED),
/// and the windowing rules name it exactly — "Windows `WM_SIZING` rect
/// adjust". The system sends the message *while* the user drags, with the
/// rectangle it is about to apply; writing a corrected rectangle back makes the
/// window snap to the ratio live rather than after the button is released.
///
/// # Which edge follows which
///
/// `edge` is the `WMSZ_*` the user grabbed, and the rule is that the edge being
/// dragged never moves — correcting it would fight the mouse, which is what a
/// naive implementation feels like:
///
/// | Dragging | Fixed | Corrected |
/// | --- | --- | --- |
/// | a vertical edge (left/right) | the width the user chose | the height, growing downwards |
/// | a horizontal edge (top/bottom) | the height the user chose | the width, growing rightwards |
/// | a corner | the width | the height, at whichever of the two horizontal edges is being dragged |
///
/// A corner takes width as the driver rather than picking the larger change,
/// because "larger" flips between frames when the pointer moves diagonally and
/// the window then oscillates.
///
/// The correction is applied to the **client** area, so the frame is subtracted
/// and added back: an aspect-locked 16:9 *window* including a 31-pixel caption
/// is not a 16:9 render target, which is the only thing the ratio was about.
#[must_use]
pub fn adjust_sizing_rect(rect: Rect, edge: usize, aspect: AspectRatio, frame: Frame) -> Rect {
    let client_width = (rect.width() - frame.width).max(1);
    let client_height = (rect.height() - frame.height).max(1);
    // In i64, and only then narrowed: a rectangle dragged to the far edge of a
    // multi-monitor desktop times a numerator overflows an i32 multiply, which
    // panics in debug and wraps in release.
    let height_for = |width: i32| -> i32 {
        let height = i64::from(width) * i64::from(aspect.denominator())
            / i64::from(aspect.numerator()).max(1);
        i32::try_from(height.max(1)).unwrap_or(i32::MAX)
    };
    let width_for = |height: i32| -> i32 {
        let width = i64::from(height) * i64::from(aspect.numerator())
            / i64::from(aspect.denominator()).max(1);
        i32::try_from(width.max(1)).unwrap_or(i32::MAX)
    };

    let mut adjusted = rect;
    match edge {
        value::WMSZ_LEFT | value::WMSZ_RIGHT => {
            adjusted.bottom = rect.top + height_for(client_width) + frame.height;
        }
        value::WMSZ_TOP | value::WMSZ_BOTTOM => {
            adjusted.right = rect.left + width_for(client_height) + frame.width;
        }
        value::WMSZ_TOP_LEFT | value::WMSZ_TOP_RIGHT => {
            adjusted.top = rect.bottom - (height_for(client_width) + frame.height);
        }
        value::WMSZ_BOTTOM_LEFT | value::WMSZ_BOTTOM_RIGHT => {
            adjusted.bottom = rect.top + height_for(client_width) + frame.height;
        }
        // Not a `WMSZ_*` this backend knows. Leaving the rectangle alone is the
        // only safe answer: the alternative is moving an edge the user is not
        // dragging.
        _ => {}
    }
    adjusted
}

/// A monitor's or window's DPI as the seam's scale factor.
///
/// 96 is 100% by definition — [`USER_DEFAULT_SCREEN_DPI`](value::DEFAULT_DPI) —
/// and every step the Windows display settings offer is a whole percentage of
/// it, so 125% arrives as 120 and 1.25 comes out. That non-integer result is
/// what [`ShellCaps::FRACTIONAL_SCALE`](crate::ShellCaps::FRACTIONAL_SCALE)
/// claims on this backend.
///
/// A zero DPI means the call failed; 1.0 is the answer that leaves a window the
/// size it asked for rather than one collapsed to a point.
#[must_use]
pub fn scale_from_dpi(dpi: u32) -> f64 {
    if dpi == 0 {
        return 1.0;
    }
    f64::from(dpi) / f64::from(value::DEFAULT_DPI)
}

/// A `RECT` as the seam's desktop rectangle.
///
/// Win32 virtual-screen coordinates are signed and the origin is the primary
/// monitor's top-left, so a monitor to the left of it has a negative `x` —
/// which [`PhysicalRect`] is signed to hold, and which is why the width comes
/// from the *difference* rather than from the right edge.
#[must_use]
pub fn desktop_rect(rect: Rect) -> PhysicalRect {
    PhysicalRect::new(
        rect.left,
        rect.top,
        rect.width().unsigned_abs(),
        rect.height().unsigned_abs(),
    )
}

/// The seam's rectangle as a `RECT` in the same space, which is how an input
/// method is told where the caret is.
///
/// The far edges saturate rather than wrap: an area reaching past `i32::MAX`
/// is clipped to it, where a wrapped edge would land left of the near one.
#[must_use]
pub const fn rect_from(area: PhysicalRect) -> Rect {
    Rect {
        left: area.x,
        top: area.y,
        right: area.x.saturating_add_unsigned(area.width),
        bottom: area.y.saturating_add_unsigned(area.height),
    }
}

/// A client `RECT` as the seam's physical size.
///
/// `GetClientRect` always answers with a zero origin, so only the extent is
/// meaningful.
#[must_use]
pub fn client_size(rect: Rect) -> PhysicalSize {
    PhysicalSize::new(rect.width().unsigned_abs(), rect.height().unsigned_abs())
}

/// Where to put a window of `size` so it sits in the middle of `area`.
///
/// Used for the initial placement, because `CW_USEDEFAULT` cascades windows
/// from the top-left of the *primary* monitor and would ignore the monitor a
/// borderless-elsewhere window is about to move to anyway. Clamped to the
/// area's origin so a window larger than the work area starts with its title
/// bar on screen rather than above it.
#[must_use]
pub fn centred(area: PhysicalRect, size: (i32, i32)) -> (i32, i32) {
    let free_x = i32::try_from(area.width).unwrap_or(i32::MAX) - size.0;
    let free_y = i32::try_from(area.height).unwrap_or(i32::MAX) - size.1;
    (area.x + (free_x / 2).max(0), area.y + (free_y / 2).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalSize;

    const WINDOWED_FRAME: Frame = Frame {
        width: 16,
        height: 39,
    };

    #[test]
    fn a_panel_is_a_frameless_tool_window_topmost_on_the_top_layers() {
        for layer in [PanelLayer::Top, PanelLayer::Overlay] {
            let styles = panel_styles(layer, PanelKeyboard::Exclusive);
            assert_ne!(styles.ex_style & style::EX_TOPMOST, 0, "{layer:?}");
        }
        for layer in [PanelLayer::Background, PanelLayer::Bottom] {
            let styles = panel_styles(layer, PanelKeyboard::Exclusive);
            assert_eq!(styles.ex_style & style::EX_TOPMOST, 0, "{layer:?}");
        }
        let bar = panel_styles(PanelLayer::Top, PanelKeyboard::None);
        assert_ne!(
            bar.ex_style & style::EX_TOOL_WINDOW,
            0,
            "kept out of the taskbar"
        );
        assert_eq!(bar.ex_style & style::EX_APP_WINDOW, 0);
        assert_ne!(bar.style & style::POPUP, 0);
        assert_eq!(bar.style & style::CAPTION, 0, "no title bar");
        assert_ne!(
            bar.ex_style & style::EX_NO_ACTIVATE,
            0,
            "a click keeps the keyboard"
        );
        for keyboard in [PanelKeyboard::OnDemand, PanelKeyboard::Exclusive] {
            let styles = panel_styles(PanelLayer::Top, keyboard);
            assert_eq!(styles.ex_style & style::EX_NO_ACTIVATE, 0, "{keyboard:?}");
        }
    }

    #[test]
    fn a_transparent_window_of_any_role_has_no_redirection_surface_and_nothing_else_changes() {
        let every_role = [
            styles(DisplayMode::Windowed, true),
            styles(DisplayMode::Borderless { monitor: None }, true),
            panel_styles(PanelLayer::Top, PanelKeyboard::None),
            popup_styles(PopupInput::Passthrough, true),
        ];
        for opaque in every_role {
            assert_eq!(opaque.ex_style & style::EX_NO_REDIRECTION_BITMAP, 0);
            assert_eq!(with_transparency(opaque, false), opaque);
            let transparent = with_transparency(opaque, true);
            assert_ne!(
                transparent.ex_style & style::EX_NO_REDIRECTION_BITMAP,
                0,
                "{opaque:?}"
            );
            assert_eq!(transparent.style, opaque.style, "the frame is the role's");
            assert_eq!(
                transparent.ex_style & !style::EX_NO_REDIRECTION_BITMAP,
                opaque.ex_style,
                "every other extended style is kept"
            );
        }
    }

    #[test]
    fn a_frameless_transparent_window_is_square_and_unbordered_and_a_captioned_one_is_the_systems()
    {
        let bare = Chrome {
            corners: dwm::CORNER_DO_NOT_ROUND,
            border: dwm::COLOR_NONE,
        };
        for frameless in [
            panel_styles(PanelLayer::Overlay, PanelKeyboard::Exclusive),
            popup_styles(PopupInput::Grab, false),
            styles(DisplayMode::Borderless { monitor: None }, true),
        ] {
            assert_eq!(
                transparent_chrome(with_transparency(frameless, true)),
                bare,
                "{frameless:?}"
            );
        }
        for captioned in [
            styles(DisplayMode::Windowed, true),
            styles(DisplayMode::Windowed, false),
        ] {
            assert_eq!(
                transparent_chrome(with_transparency(captioned, true)),
                Chrome {
                    corners: dwm::CORNER_DEFAULT,
                    border: dwm::COLOR_DEFAULT,
                },
                "{captioned:?}"
            );
        }
    }

    #[test]
    fn a_depth_moves_the_far_side_and_keeps_the_edge() {
        let monitor = Rect {
            left: -1920,
            top: 40,
            right: 0,
            bottom: 1080,
        };
        let cut = |edge, depth| {
            let rect = with_depth(monitor, edge, depth);
            (rect.left, rect.top, rect.right, rect.bottom)
        };
        assert_eq!(cut(PanelAnchor::TOP, 30), (-1920, 40, 0, 70));
        assert_eq!(cut(PanelAnchor::BOTTOM, 30), (-1920, 1050, 0, 1080));
        assert_eq!(cut(PanelAnchor::LEFT, 48), (-1920, 40, -1872, 1080));
        assert_eq!(cut(PanelAnchor::RIGHT, 48), (-48, 40, 0, 1080));
    }

    #[test]
    fn a_popup_is_activated_only_when_it_grabs_and_is_topmost_with_its_parent() {
        for input in [PopupInput::Passthrough, PopupInput::Pointer] {
            let styles = popup_styles(input, false);
            assert_ne!(styles.ex_style & style::EX_NO_ACTIVATE, 0, "{input:?}");
        }
        let menu = popup_styles(PopupInput::Grab, false);
        assert_eq!(menu.ex_style & style::EX_NO_ACTIVATE, 0);
        assert_ne!(menu.ex_style & style::EX_TOOL_WINDOW, 0);
        assert_eq!(menu.ex_style & style::EX_TOPMOST, 0);
        assert_ne!(menu.style & style::POPUP, 0);
        assert_ne!(
            popup_styles(PopupInput::Grab, true).ex_style & style::EX_TOPMOST,
            0,
            "above the topmost bar that opened it"
        );
    }

    #[test]
    fn a_popup_opens_on_its_side_and_flips_or_slides_to_stay_on_screen() {
        let area = PhysicalRect::new(0, 0, 1920, 1040);
        let button = PhysicalRect::new(100, 100, 80, 30);
        let menu = PhysicalSize::new(200, 300);
        assert_eq!(
            popup_rect(button, menu, PopupPlacement::Below, area),
            PhysicalRect::new(100, 130, 200, 300),
            "below, left edges aligned"
        );
        assert_eq!(
            popup_rect(button, menu, PopupPlacement::Above, area),
            PhysicalRect::new(100, 130, 200, 300),
            "no room above, so flipped below"
        );
        assert_eq!(
            popup_rect(button, menu, PopupPlacement::Right, area),
            PhysicalRect::new(180, 100, 200, 300),
            "a submenu, top edges aligned"
        );
        assert_eq!(
            popup_rect(button, menu, PopupPlacement::Left, area),
            PhysicalRect::new(180, 100, 200, 300),
            "no room to the left, so flipped right"
        );
        let low = PhysicalRect::new(1850, 1000, 60, 30);
        assert_eq!(
            popup_rect(low, menu, PopupPlacement::Below, area),
            PhysicalRect::new(1720, 700, 200, 300),
            "flipped above, and slid back onto the screen"
        );
        let tall = PhysicalSize::new(200, 2000);
        assert_eq!(
            popup_rect(button, tall, PopupPlacement::Below, area).y,
            130,
            "fits neither side, so it stays on its own"
        );
    }

    #[test]
    fn a_margin_in_pixels_rounds_and_keeps_zero() {
        assert_eq!(pixels(0.0, 1.5), 0);
        assert_eq!(pixels(3.0, 1.5), 5);
        assert_eq!(pixels(8.0, 1.25), 10);
        assert_eq!(pixels(-4.0, 1.0), 0);
        assert_eq!(pixels(f64::NAN, 1.0), 0);
        assert_eq!(pixels(f64::INFINITY, 1.0), i32::MAX);
    }

    #[test]
    fn a_panel_is_placed_as_layer_shell_places_one() {
        let area = PhysicalRect::new(100, 40, 1920, 1040);
        let bar = PhysicalSize::new(0, 30);
        let insets = Insets {
            top: 4,
            right: 8,
            bottom: 12,
            left: 16,
        };
        let across_the_top = PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT;
        assert_eq!(
            panel_rect(area, across_the_top, bar, Insets::default()),
            PhysicalRect::new(100, 40, 1920, 30),
            "stretched between the sides, at its own height"
        );
        assert_eq!(
            panel_rect(area, across_the_top, bar, insets),
            PhysicalRect::new(116, 44, 1920 - 16 - 8, 30),
        );
        let dock = PhysicalSize::new(64, 500);
        assert_eq!(
            panel_rect(area, PanelAnchor::RIGHT, dock, insets),
            PhysicalRect::new(100 + 1920 - 8 - 64, 40 + (1040 - 500) / 2, 64, 500),
            "held off the right edge, centred down it"
        );
        assert_eq!(
            panel_rect(area, PanelAnchor::BOTTOM | PanelAnchor::LEFT, dock, insets),
            PhysicalRect::new(116, 40 + 1040 - 12 - 500, 64, 500),
            "in the corner"
        );
        let launcher = PhysicalSize::new(600, 400);
        assert_eq!(
            panel_rect(area, PanelAnchor::empty(), launcher, insets),
            PhysicalRect::new(100 + (1920 - 600) / 2, 40 + (1040 - 400) / 2, 600, 400),
            "centred, and an unanchored panel's insets do nothing"
        );
        assert_eq!(
            panel_rect(area, PanelAnchor::all(), launcher, insets),
            PhysicalRect::new(116, 44, 1920 - 24, 1040 - 16),
        );
    }

    #[test]
    fn a_windowed_style_has_a_caption_and_a_borderless_one_has_nothing() {
        let windowed = styles(DisplayMode::Windowed, true);
        assert_ne!(windowed.style & style::CAPTION, 0, "the title bar is why");
        assert_ne!(windowed.style & style::THICK_FRAME, 0);
        assert_ne!(windowed.style & style::MAXIMIZE_BOX, 0);
        assert_eq!(windowed.style & style::POPUP, 0);

        // The bit `SERVER_DECORATIONS` is claiming: a decorated window really
        // does carry a caption, so the UI layer must not draw its own.
        let borderless = styles(DisplayMode::Borderless { monitor: None }, true);
        assert_ne!(borderless.style & style::POPUP, 0);
        assert_eq!(borderless.style & style::CAPTION, 0, "no title bar");
        assert_eq!(borderless.style & style::THICK_FRAME, 0);
        assert_eq!(
            borderless.ex_style & style::EX_WINDOW_EDGE,
            0,
            "a raised edge on a window flush with the monitor is visible"
        );

        // Both clip, because a swapchain presents into the client area.
        for these in [windowed, borderless] {
            assert_ne!(these.style & style::CLIP_SIBLINGS, 0);
            assert_ne!(these.style & style::CLIP_CHILDREN, 0);
        }
    }

    #[test]
    fn a_fixed_size_window_loses_its_resize_handles_and_maximize_button() {
        // `WindowDesc::resizable` is documented as doing exactly this, and as
        // being a different thing from `min == max`.
        let fixed = styles(DisplayMode::Windowed, false);
        assert_eq!(fixed.style & style::THICK_FRAME, 0);
        assert_eq!(fixed.style & style::MAXIMIZE_BOX, 0);
        assert_ne!(fixed.style & style::CAPTION, 0, "still a normal window");
        assert_ne!(fixed.style & style::MINIMIZE_BOX, 0, "minimize survives");
        // Borderless ignores it: there is nothing to take away.
        assert_eq!(
            styles(DisplayMode::Borderless { monitor: None }, false),
            styles(DisplayMode::Borderless { monitor: None }, true)
        );
    }

    #[test]
    fn track_sizes_are_logical_scaled_then_grown_by_the_frame() {
        let constraints = SizeConstraints {
            min: Some(LogicalSize::new(320.0, 240.0)),
            max: Some(LogicalSize::new(1920.0, 1080.0)),
            aspect: None,
        };
        let track = track_sizes(constraints, 1.0, WINDOWED_FRAME);
        assert_eq!(track.min, Some(Point { x: 336, y: 279 }));
        assert_eq!(track.max, Some(Point { x: 1936, y: 1119 }));

        // At 150% the *logical* minimum is the same and the window is bigger.
        let scaled = track_sizes(constraints, 1.5, WINDOWED_FRAME);
        assert_eq!(
            scaled.min,
            Some(Point {
                x: 480 + 16,
                y: 360 + 39
            })
        );

        // A borderless window's frame is zero, so client and window agree.
        let flush = track_sizes(constraints, 1.0, Frame::default());
        assert_eq!(flush.min, Some(Point { x: 320, y: 240 }));

        assert_eq!(
            track_sizes(SizeConstraints::NONE, 1.0, WINDOWED_FRAME),
            Track::default()
        );
    }

    #[test]
    fn dragging_a_side_corrects_the_other_axis_and_never_the_dragged_edge() {
        // 16:9 with a 1000-pixel-wide client area wants 562 pixels of height.
        let rect = Rect {
            left: 100,
            top: 100,
            right: 100 + 1000 + WINDOWED_FRAME.width,
            bottom: 100 + 400 + WINDOWED_FRAME.height,
        };
        let right = adjust_sizing_rect(
            rect,
            value::WMSZ_RIGHT,
            AspectRatio::WIDESCREEN,
            WINDOWED_FRAME,
        );
        assert_eq!(right.left, rect.left, "the dragged edge never moves");
        assert_eq!(right.right, rect.right, "nor the width it chose");
        assert_eq!(right.top, rect.top, "growth is downwards");
        assert_eq!(right.height() - WINDOWED_FRAME.height, 562);

        // Dragging the bottom fixes the height and corrects the width.
        let bottom = adjust_sizing_rect(
            rect,
            value::WMSZ_BOTTOM,
            AspectRatio::WIDESCREEN,
            WINDOWED_FRAME,
        );
        assert_eq!(bottom.bottom, rect.bottom);
        assert_eq!(bottom.left, rect.left);
        assert_eq!(bottom.width() - WINDOWED_FRAME.width, 400 * 16 / 9);
    }

    #[test]
    fn dragging_a_top_corner_moves_the_top_and_a_bottom_corner_moves_the_bottom() {
        // The half that is easy to get backwards: with the pointer on the
        // top-left, the *bottom* is what stays put.
        let rect = Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 800,
        };
        let flush = Frame::default();
        let top_left = adjust_sizing_rect(rect, value::WMSZ_TOP_LEFT, AspectRatio::SQUARE, flush);
        assert_eq!(top_left.bottom, 800, "the anchor");
        assert_eq!(top_left.top, 0);

        let wide = adjust_sizing_rect(rect, value::WMSZ_TOP_RIGHT, AspectRatio::WIDESCREEN, flush);
        assert_eq!(wide.bottom, 800);
        assert_eq!(wide.height(), 800 * 9 / 16);

        let low = adjust_sizing_rect(
            rect,
            value::WMSZ_BOTTOM_LEFT,
            AspectRatio::WIDESCREEN,
            flush,
        );
        assert_eq!(low.top, 0, "the anchor, the other way up");
        assert_eq!(low.height(), 800 * 9 / 16);
    }

    #[test]
    fn an_unknown_drag_edge_leaves_the_rectangle_exactly_as_it_was() {
        // `WMSZ_*` runs 1..=8. A zero arrives from a `SendMessage` that is not
        // a real drag, and moving an edge for it would resize a window nobody
        // touched.
        let rect = Rect {
            left: -10,
            top: -20,
            right: 630,
            bottom: 460,
        };
        assert_eq!(
            adjust_sizing_rect(rect, 0, AspectRatio::WIDESCREEN, Frame::default()),
            rect
        );
        assert_eq!(
            adjust_sizing_rect(rect, 99, AspectRatio::CLASSIC, WINDOWED_FRAME),
            rect
        );
    }

    #[test]
    fn a_degenerate_drag_rectangle_still_produces_a_positive_size() {
        // The rectangle can arrive smaller than the frame while the user drags
        // an edge past the opposite one; a zero or negative client size would
        // divide into a zero height and then into a window of no size at all.
        let collapsed = Rect {
            left: 500,
            top: 500,
            right: 501,
            bottom: 501,
        };
        let fixed = adjust_sizing_rect(
            collapsed,
            value::WMSZ_BOTTOM_RIGHT,
            AspectRatio::WIDESCREEN,
            WINDOWED_FRAME,
        );
        assert!(fixed.height() > 0, "{fixed:?}");
    }

    #[test]
    fn ninety_six_dots_per_inch_is_one_hundred_per_cent() {
        assert!((scale_from_dpi(96) - 1.0).abs() < f64::EPSILON);
        // The values Windows' own display settings produce, and the reason
        // `FRACTIONAL_SCALE` is set: 125% is not an integer scale.
        assert!((scale_from_dpi(120) - 1.25).abs() < f64::EPSILON);
        assert!((scale_from_dpi(144) - 1.5).abs() < f64::EPSILON);
        assert!((scale_from_dpi(192) - 2.0).abs() < f64::EPSILON);
        // A failed `GetDpiForWindow` answers zero, and a window scaled by zero
        // has no pixels.
        assert!((scale_from_dpi(0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_text_input_area_becomes_its_rect_and_saturates_at_the_far_edge() {
        let caret = PhysicalRect::new(40, 60, 8, 20);
        let rect = rect_from(caret);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (40, 60, 48, 80)
        );
        assert_eq!(desktop_rect(rect), caret, "the round trip is exact");
        let huge = rect_from(PhysicalRect::new(-5, 10, u32::MAX, u32::MAX));
        assert_eq!(
            (huge.left, huge.top, huge.right, huge.bottom),
            (-5, 10, i32::MAX, i32::MAX)
        );
    }

    #[test]
    fn a_monitor_left_of_the_primary_one_has_a_negative_origin() {
        // Virtual-screen coordinates are signed, and this is the case a `u32`
        // conversion turns into four billion.
        let left_of_primary = Rect {
            left: -1920,
            top: -120,
            right: 0,
            bottom: 960,
        };
        assert_eq!(
            desktop_rect(left_of_primary),
            PhysicalRect::new(-1920, -120, 1920, 1080)
        );
        assert_eq!(
            client_size(Rect {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720
            }),
            PhysicalSize::new(1280, 720)
        );
    }

    #[test]
    fn a_window_is_centred_in_the_work_area_and_never_above_it() {
        let work = PhysicalRect::new(0, 32, 1920, 1048);
        assert_eq!(centred(work, (1280, 720)), (320, 32 + 164));
        // A window taller than the work area starts at the top of it: half of a
        // negative gap would put the title bar off screen, where it cannot be
        // grabbed.
        assert_eq!(centred(work, (3000, 2000)), (0, 32));
        // And the origin is carried, so the second monitor is a second monitor.
        let right = PhysicalRect::new(1920, 0, 1280, 1024);
        assert_eq!(centred(right, (1280, 1024)), (1920, 0));
    }
}
