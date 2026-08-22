//! The arithmetic half of the AppKit backend: the Y flip, points versus backing
//! pixels, style masks and window placement.
//!
//! Everything in here is a **pure function of numbers AppKit handed us**, and
//! that is why it is a module of its own rather than a scattering of helpers
//! through [`window`](super::window) and [`monitors`](super::monitors). The
//! Win32 backend splits `win32::geometry` out for two reasons and the second is
//! the load-bearing one; both apply here and the second applies harder:
//!
//! 1. A delegate callback runs inside `[NSApp sendEvent:]`, which runs inside
//!    this crate's own `pump`. Keeping the arithmetic it needs in functions that
//!    touch no object makes that property visible rather than hoped for.
//! 2. **It is the part of this backend that can be tested without a Mac.** This
//!    module compiles and its tests run on every host, so a mistake in the
//!    coordinate flip or in the points-to-pixels conversion is caught by
//!    `cargo test -p crcbl-shell` on the Linux machine this engine is developed
//!    on, rather than by a CI runner nobody is watching. That matters more here
//!    than it did for Windows, because more of this backend is a conversion:
//!    macOS is the only platform of the five whose coordinate system disagrees
//!    with the seam's on the direction of the Y axis.
//!
//! # Three spaces, and the flip between two of them
//!
//! * **AppKit screen space** — points, origin at the **bottom-left** of the
//!   primary display, **Y increasing upwards**. `NSScreen.frame`,
//!   `NSWindow.frame` and everything `setFrame:` takes are in it.
//! * **Desktop space** — the seam's [`PhysicalRect`], origin at the *top-left*,
//!   Y increasing downwards, matching Win32's virtual screen and X11's screen.
//! * **Backing pixels** — points multiplied by the window's or screen's
//!   `backingScaleFactor`, which is the seam's [`PhysicalSize`] and the extent a
//!   swapchain is created at.
//!
//! [`Flip`] is the first conversion and [`physical_size`] the second, and
//! **they are separate steps on purpose**: the flip has to happen in points,
//! because the global coordinate space *is* points. A desktop whose displays run
//! at different scales has no single pixel space at all — the caveat
//! [`MonitorInfo::bounds`](crate::MonitorInfo::bounds) already states for
//! Wayland — so scaling before flipping would compute a Y offset in one screen's
//! pixels against another screen's height.
//!
//! # Logical units *are* points, exactly
//!
//! The seam's [`LogicalSize`] is "device pixels divided by the scale factor",
//! and an AppKit point is "backing pixel divided by `backingScaleFactor`". They
//! are the same unit, so [`SizeConstraints`] converts to `setContentMinSize:`
//! with no arithmetic at all. Win32 has to multiply by the DPI and X11 by
//! `Xft.dpi`; this is the one platform whose own geometry is already in the
//! seam's request space, and [`content_limits`] is where that shows.

use crate::{AspectRatio, DisplayMode, LogicalSize, PhysicalRect, PhysicalSize, SizeConstraints};

use super::ffi::{NSPoint, NSRect, NSSize, style, value};

/// The conversion between AppKit's Y-up screen space and the seam's Y-down
/// desktop space.
///
/// One number: the **top edge of the primary display**, in AppKit points. Every
/// rectangle in either space is a reflection about it.
///
/// # Why the primary display and not the whole desktop
///
/// AppKit's origin is the bottom-left of the primary display, so its `y = 0` is
/// that display's bottom edge — not the bottom of the union of all displays. A
/// second display placed above the primary one therefore has a *positive* origin
/// in AppKit and a *negative* one in desktop space, which is exactly what Win32
/// and X11 report for the same arrangement. Reflecting about the union's height
/// instead would put every window on a multi-display desktop at the wrong Y and
/// would be right on a single-display one, which is what a CI runner is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flip {
    /// `primary.frame.origin.y + primary.frame.size.height`, in points.
    top: f64,
}

impl Flip {
    /// The flip implied by the primary display's frame.
    #[must_use]
    pub const fn about_primary(primary: NSRect) -> Self {
        Self {
            top: primary.max_y(),
        }
    }

    /// A rectangle moved between the two spaces.
    ///
    /// **The same function both ways**, because a reflection is its own inverse:
    /// the top edge of a rectangle in one space is the bottom edge of the same
    /// rectangle in the other, and `top − (y + height)` computes each from the
    /// other. Having one function rather than a `to_`/`from_` pair is not
    /// tidiness — two of them would be two chances to write the formula down
    /// differently, and the round trip is the property that catches it.
    #[must_use]
    pub const fn rect(self, rect: NSRect) -> NSRect {
        NSRect::new(
            rect.origin.x,
            self.top - rect.max_y(),
            rect.size.width,
            rect.size.height,
        )
    }

    /// A **point** moved between the two spaces.
    ///
    /// The same reflection with no height to subtract, which is what makes it
    /// the simpler of the pair rather than a special case of it: a rectangle's
    /// origin is its *bottom* edge in one space and its *top* edge in the other,
    /// so [`rect`](Self::rect) has to reflect `y + height` while a point
    /// reflects `y`. Writing `rect` in terms of this one is the identity
    /// `rect.origin.y ↦ top − (y + height)`, and the test below asserts it so
    /// the two can never disagree about where the edge is.
    ///
    /// This is the space `CGWarpMouseCursorPosition` takes and the space
    /// `CGEventGetLocation` answers in — CoreGraphics puts its origin at the
    /// top-left of the main display, which is the seam's convention and the
    /// opposite of AppKit's. So this function is the whole of the warp's second
    /// reflection; see [`pointer::warp_source`](super::pointer::warp_source) for
    /// the first.
    ///
    /// Its own inverse, for the reason [`rect`](Self::rect) gives.
    #[must_use]
    pub const fn point(self, point: NSPoint) -> NSPoint {
        NSPoint {
            x: point.x,
            y: self.top - point.y,
        }
    }
}

/// A rectangle of points as the seam's desktop rectangle, at `scale`.
///
/// The rectangle must already have been through [`Flip::rect`]; this only
/// scales. Rounding is to nearest, so a fractional point origin — which a
/// display arrangement can produce — does not truncate a window one pixel off
/// its screen.
#[must_use]
pub fn physical_rect(flipped_points: NSRect, scale: f64) -> PhysicalRect {
    let scale = usable_scale(scale);
    PhysicalRect::new(
        round_to_i32(flipped_points.origin.x * scale),
        round_to_i32(flipped_points.origin.y * scale),
        round_to_u32(flipped_points.size.width * scale),
        round_to_u32(flipped_points.size.height * scale),
    )
}

/// A size in points as the seam's size in backing pixels.
///
/// # The conversion this whole module exists to not get wrong
///
/// `NSWindow.frame` and `NSView.bounds` are in **points**; a swapchain is
/// created in **pixels**. On a 1× display they are the same number, which is why
/// mixing them up produces something that looks perfectly correct on a CI runner
/// and is half the size it should be on every Retina Mac — that is to say, on
/// every Mac. The tests for this function are written at 2× deliberately.
#[must_use]
pub fn physical_size(points: NSSize, scale: f64) -> PhysicalSize {
    let scale = usable_scale(scale);
    PhysicalSize::new(
        round_to_u32(points.width * scale).max(1),
        round_to_u32(points.height * scale).max(1),
    )
}

/// The seam's logical size as AppKit points.
///
/// The identity, in a different type. See the module docs for why that is a
/// fact about macOS rather than a shortcut here.
#[must_use]
pub const fn points(size: LogicalSize) -> NSSize {
    NSSize::new(size.width, size.height)
}

/// A `backingScaleFactor` as the seam's scale factor.
///
/// AppKit reports 1.0 or 2.0 and nothing between: a display running a "scaled"
/// HiDPI mode still answers 2.0 and changes the *point* resolution instead.
/// That is why [`FRACTIONAL_SCALE`](crate::ShellCaps::FRACTIONAL_SCALE) is clear
/// on this backend while it is set on Win32 and on Wayland — see
/// [`caps`](super::AppKitShell::caps).
///
/// A zero or negative factor means the object was `nil`; 1.0 is the answer that
/// leaves a window the size it asked for rather than one collapsed to a point.
#[must_use]
pub fn usable_scale(backing: f64) -> f64 {
    if backing.is_finite() && backing > 0.0 {
        backing
    } else {
        1.0
    }
}

/// The `NSWindowStyleMask` for a display mode.
///
/// # Borderless is a style mask, not a mode the system has
///
/// `docs/plan/15-windowing.md` drops exclusive fullscreen and keeps two modes,
/// and neither of them is macOS's *Spaces* fullscreen — `toggleFullScreen:`,
/// which moves the window to a space of its own with an animation, a menu bar
/// that slides, and a set of failure modes that are entirely its own. Borderless
/// here is the same recipe as on Windows: remember the mask and the frame, drop
/// to `NSWindowStyleMaskBorderless`, and set the frame to the target screen's.
///
/// A non-resizable window loses `NSWindowStyleMaskResizable` rather than being
/// clamped by a constraint —
/// [`WindowDesc::resizable`](crate::WindowDesc::resizable) documents that the
/// difference from `min == max` is exactly this: the resize handles and the
/// zoom button go away.
#[must_use]
pub fn style_mask(mode: DisplayMode, resizable: bool) -> usize {
    match mode {
        DisplayMode::Windowed => {
            let mut mask = style::TITLED | style::CLOSABLE | style::MINIATURIZABLE;
            if resizable {
                mask |= style::RESIZABLE;
            }
            mask
        }
        // Not `TITLED | RESIZABLE` with a hidden title bar, which is the other
        // recipe people reach for: that leaves the window with a frame to
        // account for, and the whole point is that the content rectangle and the
        // frame rectangle become the same thing.
        DisplayMode::Borderless { .. } => style::BORDERLESS,
    }
}

/// The presentation options a mode wants from `NSApplication`.
///
/// Process-wide state rather than a window property — there is one menu bar —
/// so [`AppKitShell`](super::AppKitShell) owns restoring it, exactly as the
/// Win32 backend owns giving the cursor clip back. See
/// [`PRESENTATION_BORDERLESS`](value::PRESENTATION_BORDERLESS) for why the two
/// bits are never set apart.
#[must_use]
pub const fn presentation_options(mode: DisplayMode) -> usize {
    match mode {
        DisplayMode::Windowed => value::PRESENTATION_DEFAULT,
        DisplayMode::Borderless { .. } => value::PRESENTATION_BORDERLESS,
    }
}

/// [`SizeConstraints`] as the two `NSSize`s `setContentMinSize:` and
/// `setContentMaxSize:` take.
///
/// No scale factor, and no frame to add: AppKit's limits are on the **content**
/// rectangle and are in points, which is the seam's own request space. Both of
/// the other desktop backends have to do arithmetic here and both can get it
/// wrong — Win32's has to add the caption height or the minimum is tens of
/// pixels off.
///
/// `None` becomes AppKit's own default, which for the minimum is zero and for
/// the maximum is [`f64::MAX`] — the values the class documents as "no limit",
/// rather than a number this backend invented.
#[must_use]
pub fn content_limits(constraints: SizeConstraints) -> (NSSize, NSSize) {
    let min = constraints.min.map_or(NSSize::new(0.0, 0.0), points);
    let max = constraints
        .max
        .map_or(NSSize::new(f64::MAX, f64::MAX), points);
    (min, max)
}

/// An aspect ratio as the `NSSize` `setContentAspectRatio:` takes.
///
/// This is the macOS half of
/// [`ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED), and
/// `docs/plan/15-windowing.md` names it exactly — "macOS
/// `setContentAspectRatio`". Unlike the Win32 backend there is no rectangle
/// algebra to write: AppKit enforces the ratio during the drag itself, and the
/// terms go across as a size whose *proportion* is what is read.
///
/// `NSSize::new(0.0, 0.0)` is what clears it, which is why the return type is
/// not an `Option`: there is one call either way.
#[must_use]
pub fn aspect_size(aspect: Option<AspectRatio>) -> NSSize {
    aspect.map_or(NSSize::new(0.0, 0.0), |ratio| {
        NSSize::new(f64::from(ratio.numerator()), f64::from(ratio.denominator()))
    })
}

/// Where to put a window of `size` so it sits in the middle of `area`, in
/// AppKit points.
///
/// `area` is a screen's `visibleFrame` — the part not covered by the menu bar or
/// the Dock — and the result is a bottom-left origin in the same space, so no
/// flip is involved. Clamped to the area's origin so that a window larger than
/// the visible frame starts with its **title bar** on screen: in a Y-up space
/// that is the *top*, so the clamp is on the high side of Y rather than the low
/// one, which is the sign error this function exists to have made once.
#[must_use]
pub fn centred(area: NSRect, size: NSSize) -> NSRect {
    let free_x = area.size.width - size.width;
    let free_y = area.size.height - size.height;
    NSRect::new(
        area.origin.x + (free_x / 2.0).max(0.0),
        // A negative gap would put the window's top above the visible frame,
        // where the title bar cannot be grabbed. Pinning the *top* to the top of
        // the area means the origin lands at `max_y − height`.
        if free_y >= 0.0 {
            area.origin.y + free_y / 2.0
        } else {
            area.max_y() - size.height
        },
        size.width,
        size.height,
    )
}

/// Rounds to the nearest `i32`, saturating.
fn round_to_i32(value: f64) -> i32 {
    let rounded = value.round();
    if rounded.is_nan() {
        0
    } else {
        rounded.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    }
}

/// Rounds to the nearest `u32`, saturating, with negatives becoming zero.
fn round_to_u32(value: f64) -> u32 {
    let rounded = value.round();
    if rounded.is_nan() {
        0
    } else {
        rounded.clamp(0.0, f64::from(u32::MAX)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MonitorId;

    /// A 1920×1080 primary display, which puts the flip at 1080.
    const PRIMARY: NSRect = NSRect::new(0.0, 0.0, 1920.0, 1080.0);

    #[test]
    fn the_primary_display_is_the_same_rectangle_in_both_spaces() {
        // The one arrangement where the flip is invisible, and therefore the one
        // a CI runner with a single display would pass whatever this function
        // did. Every other case below is the reason the fixtures exist.
        let flip = Flip::about_primary(PRIMARY);
        assert_eq!(flip.rect(PRIMARY), PRIMARY);
        assert_eq!(
            physical_rect(flip.rect(PRIMARY), 1.0),
            PhysicalRect::new(0, 0, 1920, 1080)
        );
    }

    #[test]
    fn a_display_above_the_primary_one_has_a_negative_desktop_origin() {
        // AppKit stacks upwards from the primary display's bottom-left, so a
        // display *above* the primary has a positive Y there — and a negative
        // one in the space Win32 and X11 report, which is what the seam means.
        let flip = Flip::about_primary(PRIMARY);
        let above = NSRect::new(0.0, 1080.0, 1920.0, 1080.0);
        assert_eq!(flip.rect(above), NSRect::new(0.0, -1080.0, 1920.0, 1080.0));

        // And below, which is the sign the naive `top - y` gets wrong: the
        // rectangle's *height* is part of the reflection.
        let below = NSRect::new(0.0, -1080.0, 1920.0, 1080.0);
        assert_eq!(flip.rect(below), NSRect::new(0.0, 1080.0, 1920.0, 1080.0));

        // Left and right are untouched — only Y is flipped.
        let left = NSRect::new(-1920.0, 0.0, 1920.0, 1080.0);
        assert_eq!(flip.rect(left), NSRect::new(-1920.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn a_display_of_a_different_height_reflects_about_its_own_top_edge() {
        // The case that catches a flip written as `height - y` rather than
        // `top - (y + height)`: a shorter display sitting beside a taller one,
        // bottom-aligned the way macOS's own arrangement pane does it.
        let flip = Flip::about_primary(PRIMARY);
        let shorter = NSRect::new(1920.0, 0.0, 1280.0, 720.0);
        // Its bottom edge is the primary's bottom edge, so its *top* is 360
        // points below the primary's top.
        assert_eq!(
            flip.rect(shorter),
            NSRect::new(1920.0, 360.0, 1280.0, 720.0)
        );
    }

    #[test]
    fn the_flip_is_its_own_inverse() {
        // The round trip is the property, and it is what makes one function
        // correct where a `to_`/`from_` pair would be two chances to disagree.
        let flip = Flip::about_primary(PRIMARY);
        for rect in [
            PRIMARY,
            NSRect::new(0.0, 1080.0, 1920.0, 1080.0),
            NSRect::new(-1440.0, -320.0, 1440.0, 900.0),
            NSRect::new(37.5, -0.5, 1.0, 1.0),
        ] {
            assert_eq!(flip.rect(flip.rect(rect)), rect, "{rect:?}");
        }
    }

    #[test]
    fn a_point_reflects_about_the_same_edge_a_rectangle_does() {
        // The relation that makes the pair one function rather than two: a
        // rectangle's flipped origin is its *top* edge flipped as a point. A
        // `Flip::point` written as `top - y - height`, or a `Flip::rect` written
        // without the height, would disagree here and nowhere else.
        let flip = Flip::about_primary(PRIMARY);
        for rect in [
            PRIMARY,
            NSRect::new(1920.0, 0.0, 1280.0, 720.0),
            NSRect::new(-1440.0, -320.0, 1440.0, 900.0),
        ] {
            let top_edge = NSPoint {
                x: rect.origin.x,
                y: rect.max_y(),
            };
            assert_eq!(flip.point(top_edge), flip.rect(rect).origin, "{rect:?}");
        }

        // The CoreGraphics space a warp is expressed in: the top-left of the
        // primary display is the origin, and AppKit calls the same place
        // `(0, 1080)`.
        assert_eq!(
            flip.point(NSPoint { x: 0.0, y: 1080.0 }),
            NSPoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(
            flip.point(NSPoint { x: 640.0, y: 540.0 }),
            NSPoint { x: 640.0, y: 540.0 },
            "the middle of a 1080-tall display is its own reflection"
        );

        // And it is its own inverse, which is what makes one function correct
        // where a `to_`/`from_` pair would be two chances to disagree.
        for point in [
            NSPoint { x: 0.0, y: 0.0 },
            NSPoint {
                x: -37.5,
                y: 2160.0,
            },
            NSPoint { x: 1919.0, y: -1.0 },
        ] {
            assert_eq!(flip.point(flip.point(point)), point, "{point:?}");
        }
    }

    #[test]
    fn a_window_at_the_top_of_the_primary_display_is_at_desktop_y_zero() {
        // The concrete case a menu-bar-aware placement produces: a window whose
        // top edge is the screen's top edge. In AppKit its origin is
        // `1080 - height`; in desktop space it must be zero, not `1080`.
        let flip = Flip::about_primary(PRIMARY);
        let window = NSRect::new(320.0, 1080.0 - 720.0, 1280.0, 720.0);
        assert_eq!(flip.rect(window), NSRect::new(320.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn points_become_backing_pixels_and_a_retina_window_is_twice_the_size() {
        // The half-size-on-every-Mac bug, asserted at the scale that can see it.
        // At 1.0 this function is the identity and proves nothing, which is
        // exactly what a single-scale CI runner would exercise.
        let content = NSSize::new(1280.0, 720.0);
        assert_eq!(physical_size(content, 1.0), PhysicalSize::new(1280, 720));
        assert_eq!(physical_size(content, 2.0), PhysicalSize::new(2560, 1440));

        // A whole screen, through both steps in the order they must happen.
        let flip = Flip::about_primary(NSRect::new(0.0, 0.0, 1440.0, 900.0));
        let retina = NSRect::new(0.0, 0.0, 1440.0, 900.0);
        assert_eq!(
            physical_rect(flip.rect(retina), 2.0),
            PhysicalRect::new(0, 0, 2880, 1800),
            "a 1440x900-point Retina panel is 2880x1800 backing pixels"
        );

        // A zero or absent scale factor leaves the window the size it asked for.
        assert_eq!(physical_size(content, 0.0), PhysicalSize::new(1280, 720));
        assert_eq!(
            physical_size(content, f64::NAN),
            PhysicalSize::new(1280, 720)
        );
        // And a window dragged to nothing still yields a legal extent.
        assert_eq!(
            physical_size(NSSize::new(0.0, 0.0), 2.0),
            PhysicalSize::new(1, 1)
        );
    }

    #[test]
    fn a_scaled_hidpi_mode_is_two_times_a_larger_point_size() {
        // What macOS calls "looks like 1680x1050" on a 2880x1800 panel: the
        // point resolution changes and `backingScaleFactor` stays 2.0. So the
        // backing store is 3360x2100 and is *then* downscaled by the compositor,
        // and 3360x2100 is what a swapchain must be created at. Reporting the
        // panel's 2880x1800 instead would be a swapchain that does not match the
        // layer.
        assert_eq!(
            physical_size(NSSize::new(1680.0, 1050.0), 2.0),
            PhysicalSize::new(3360, 2100)
        );
    }

    #[test]
    fn a_windowed_mask_has_a_title_bar_and_a_borderless_one_has_nothing() {
        let windowed = style_mask(DisplayMode::Windowed, true);
        assert_ne!(windowed & style::TITLED, 0, "the title bar is why");
        assert_ne!(windowed & style::RESIZABLE, 0);
        assert_ne!(windowed & style::CLOSABLE, 0);

        // The bit `SERVER_DECORATIONS` is claiming: a decorated window really
        // does carry a title bar, so the UI layer must not draw its own.
        let borderless = style_mask(DisplayMode::Borderless { monitor: None }, true);
        assert_eq!(borderless, style::BORDERLESS);
        assert_eq!(borderless & style::TITLED, 0, "no title bar");
        assert_eq!(
            style_mask(DisplayMode::Borderless { monitor: None }, false),
            borderless,
            "there is nothing left to take away"
        );
    }

    #[test]
    fn a_fixed_size_window_loses_its_resize_handles() {
        // `WindowDesc::resizable` is documented as doing exactly this, and as
        // being a different thing from `min == max`.
        let fixed = style_mask(DisplayMode::Windowed, false);
        assert_eq!(fixed & style::RESIZABLE, 0);
        assert_ne!(fixed & style::TITLED, 0, "still a normal window");
        assert_ne!(fixed & style::MINIATURIZABLE, 0, "minimize survives");
    }

    #[test]
    fn borderless_asks_for_both_presentation_bits_or_neither() {
        // Setting `AutoHideMenuBar` without a Dock bit raises an Objective-C
        // exception, which unwinds into Rust and is undefined behaviour. This
        // asserts the pairing rather than the number.
        let borderless = presentation_options(DisplayMode::Borderless { monitor: None });
        assert_ne!(borderless & value::PRESENTATION_AUTO_HIDE_MENU_BAR, 0);
        assert_ne!(
            borderless & value::PRESENTATION_AUTO_HIDE_DOCK,
            0,
            "the menu-bar bit is only legal beside a Dock bit"
        );
        assert_eq!(
            presentation_options(DisplayMode::Windowed),
            value::PRESENTATION_DEFAULT
        );
        assert_eq!(
            presentation_options(DisplayMode::Borderless {
                monitor: Some(MonitorId(2))
            }),
            borderless,
            "which display it covers does not change what the menu bar does"
        );
    }

    #[test]
    fn content_limits_are_logical_units_unchanged() {
        // The macOS difference from the other two desktop backends: no scale
        // factor and no frame. A minimum of 320x240 logical is a minimum of
        // 320x240 points, at 1x and at 2x alike.
        let constraints = SizeConstraints {
            min: Some(LogicalSize::new(320.0, 240.0)),
            max: Some(LogicalSize::new(1920.0, 1080.0)),
            aspect: None,
        };
        let (min, max) = content_limits(constraints);
        assert_eq!(min, NSSize::new(320.0, 240.0));
        assert_eq!(max, NSSize::new(1920.0, 1080.0));

        // Absent limits are AppKit's own "no limit", not a number invented here.
        let (min, max) = content_limits(SizeConstraints::NONE);
        assert_eq!(min, NSSize::new(0.0, 0.0));
        assert_eq!(max, NSSize::new(f64::MAX, f64::MAX));
    }

    #[test]
    fn an_aspect_ratio_crosses_as_its_two_terms_and_zero_clears_it() {
        assert_eq!(
            aspect_size(Some(AspectRatio::WIDESCREEN)),
            NSSize::new(16.0, 9.0)
        );
        assert_eq!(
            aspect_size(Some(AspectRatio::CLASSIC)),
            NSSize::new(4.0, 3.0)
        );
        assert_eq!(
            aspect_size(None),
            NSSize::new(0.0, 0.0),
            "zero is what AppKit reads as no ratio at all"
        );
    }

    #[test]
    fn a_window_is_centred_in_the_visible_frame_and_never_above_it() {
        // A visible frame with a 25-point menu bar taken off the top: in a Y-up
        // space that is height, not origin.
        let visible = NSRect::new(0.0, 0.0, 1920.0, 1055.0);
        let placed = centred(visible, NSSize::new(1280.0, 720.0));
        assert_eq!(placed.origin.x, 320.0);
        assert_eq!(placed.origin.y, (1055.0 - 720.0) / 2.0);
        assert_eq!(placed.size, NSSize::new(1280.0, 720.0));

        // A window taller than the visible frame is pinned by its **top**, so
        // its origin goes *below* the area's origin — the opposite sign to the
        // Win32 equivalent, and the whole reason this function is not a copy of
        // it. The runner this has to hold on is the one whose display is
        // smaller than `WindowDesc::default`.
        let tall = centred(visible, NSSize::new(3000.0, 2000.0));
        assert_eq!(tall.max_y(), visible.max_y(), "the top edges line up");
        assert!(tall.origin.y < visible.origin.y, "{tall:?}");

        // And the origin is carried, so a second display is a second display.
        let right = NSRect::new(1920.0, 0.0, 1280.0, 1024.0);
        assert_eq!(
            centred(right, NSSize::new(1280.0, 1024.0)).origin,
            super::super::ffi::NSPoint { x: 1920.0, y: 0.0 }
        );
    }

    #[test]
    fn rounding_saturates_rather_than_wrapping() {
        // A display arrangement can put a screen at a fractional point origin,
        // and `as i32` on an out-of-range float is a wrap in release.
        assert_eq!(round_to_i32(0.5), 1);
        assert_eq!(round_to_i32(-0.5), -1);
        assert_eq!(round_to_i32(f64::INFINITY), i32::MAX);
        assert_eq!(round_to_i32(f64::NEG_INFINITY), i32::MIN);
        assert_eq!(round_to_i32(f64::NAN), 0);
        assert_eq!(round_to_u32(-5.0), 0);
        assert_eq!(round_to_u32(f64::INFINITY), u32::MAX);
        assert_eq!(round_to_u32(f64::NAN), 0);
    }
}
