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

use super::ffi::{Point, Rect, style, value};
use crate::{AspectRatio, DisplayMode, PhysicalRect, PhysicalSize, SizeConstraints};

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
/// Windows has no "fullscreen" state to ask for — `docs/plan/15-windowing.md`
/// drops exclusive fullscreen, and the *borderless* kind is simply a window
/// with `WS_POPUP` positioned over a monitor. That has a consequence the two
/// Linux backends do not share and [`set_mode`](crate::Shell::set_mode) has to
/// account for: there is no window manager to refuse, so the effective mode is
/// known the moment the call returns.
///
/// `WS_CLIPSIBLINGS | WS_CLIPCHILDREN` are on both, as every rendering window
/// sets them: without them the system may draw over the client area while a
/// swapchain is presenting into it.
///
/// A non-resizable window loses `WS_THICKFRAME` and `WS_MAXIMIZEBOX` rather
/// than being clamped by a constraint — [`WindowDesc::resizable`](crate::WindowDesc::resizable)
/// documents that the difference from `min == max` is exactly this: the resize
/// handles and the maximize button disappear.
#[must_use]
pub fn styles(mode: DisplayMode, resizable: bool) -> Styles {
    const COMMON: u32 = style::CLIP_SIBLINGS | style::CLIP_CHILDREN;
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
/// and `docs/plan/15-windowing.md` names it exactly — "Windows `WM_SIZING` rect
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
