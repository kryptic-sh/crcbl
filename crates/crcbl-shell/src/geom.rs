//! Sizes, points and rectangles, in the two coordinate spaces a window has.
//!
//! # Physical and logical are different types on purpose
//!
//! Every windowing bug that outlives a release is a scale-factor bug, and every
//! scale-factor bug is a physical value used where a logical one was meant (or
//! the reverse). So the two are separate types and the only way between them is
//! an explicit conversion that takes the scale factor:
//!
//! * **Physical** ([`PhysicalSize`], [`PhysicalPoint`], [`PhysicalRect`]) —
//!   device pixels. This is what a swapchain is sized in, what a viewport is
//!   sized in, and what a pointer position is reported in. Never guess it.
//! * **Logical** ([`LogicalSize`], [`LogicalPoint`]) — scale-independent units.
//!   This is what a *request* is expressed in ("open a 1280×720 window"), what
//!   min/max constraints are expressed in, and what the UI lays out in. It is
//!   `f64` rather than integer because fractional scaling (Wayland's
//!   `fractional-scale-v1` reports scale in 1/120ths) makes
//!   `physical / scale` non-integral in general.
//!
//! The engine's rule: **events report physical, requests carry logical.** A
//! window asks for a size in a unit the user recognizes; the renderer allocates
//! in the unit the GPU recognizes; the shell is the only thing that converts.
//!
//! # Rounding
//!
//! [`LogicalSize::to_physical`] rounds to nearest and clamps to at least 1×1.
//! Zero-sized swapchains are invalid in Vulkan, and a minimized window on Win32
//! genuinely reports 0×0 — so the clamp is not paranoia, it is the difference
//! between "the window is minimized" and a validation error.

use core::fmt;

/// A size in device pixels.
///
/// What the swapchain is created at. See the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhysicalSize {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
}

impl PhysicalSize {
    /// A size of `width` × `height` device pixels.
    #[inline]
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero — a minimized or not-yet-configured
    /// window.
    ///
    /// The frame loop uses this to skip rendering rather than to create an
    /// invalid swapchain.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Pixel count, as a `u64` so a 16K×16K target does not overflow.
    #[inline]
    #[must_use]
    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// This size in logical units at `scale_factor`.
    #[inline]
    #[must_use]
    pub fn to_logical(self, scale_factor: f64) -> LogicalSize {
        debug_assert!(scale_factor > 0.0, "scale factor must be positive");
        LogicalSize {
            width: f64::from(self.width) / scale_factor,
            height: f64::from(self.height) / scale_factor,
        }
    }

    /// The aspect ratio as a float, or `None` for an empty size.
    #[must_use]
    pub fn aspect(self) -> Option<f64> {
        if self.is_empty() {
            None
        } else {
            Some(f64::from(self.width) / f64::from(self.height))
        }
    }
}

impl fmt::Display for PhysicalSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}px", self.width, self.height)
    }
}

/// A size in scale-independent units.
///
/// What a window *request* and a size constraint are expressed in. See the
/// [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalSize {
    /// Width in logical units.
    pub width: f64,
    /// Height in logical units.
    pub height: f64,
}

impl LogicalSize {
    /// A size of `width` × `height` logical units.
    #[inline]
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// This size in device pixels at `scale_factor`.
    ///
    /// Rounds to nearest and clamps to at least 1×1; see the [module
    /// docs](self) for why the clamp exists.
    #[must_use]
    pub fn to_physical(self, scale_factor: f64) -> PhysicalSize {
        debug_assert!(scale_factor > 0.0, "scale factor must be positive");
        let scale = |value: f64| {
            let scaled = (value * scale_factor).round();
            // `clamp` handles infinities correctly (∞ saturates to `u32::MAX`)
            // but propagates NaN, which has no sensible size — so NaN becomes
            // the same 1 px a zero would.
            if scaled.is_nan() {
                1
            } else {
                scaled.clamp(1.0, f64::from(u32::MAX)) as u32
            }
        };
        PhysicalSize {
            width: scale(self.width),
            height: scale(self.height),
        }
    }
}

impl fmt::Display for LogicalSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}dp", self.width, self.height)
    }
}

/// A point in device pixels, relative to a window's top-left corner.
///
/// `f64` because pointer positions are sub-pixel: Wayland delivers them as
/// 24.8 fixed point and a 240 Hz mouse on a scaled output genuinely moves less
/// than a pixel per event. Truncating here would quantize aim.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalPoint {
    /// Distance from the left edge, in device pixels.
    pub x: f64,
    /// Distance from the top edge, in device pixels.
    pub y: f64,
}

impl PhysicalPoint {
    /// The window's top-left corner.
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    /// A point at `(x, y)` device pixels.
    #[inline]
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// This point in logical units at `scale_factor`.
    #[inline]
    #[must_use]
    pub fn to_logical(self, scale_factor: f64) -> LogicalPoint {
        debug_assert!(scale_factor > 0.0, "scale factor must be positive");
        LogicalPoint {
            x: self.x / scale_factor,
            y: self.y / scale_factor,
        }
    }

    /// Whether the point lies inside a window of `size`.
    #[must_use]
    pub fn is_inside(self, size: PhysicalSize) -> bool {
        self.x >= 0.0
            && self.y >= 0.0
            && self.x < f64::from(size.width)
            && self.y < f64::from(size.height)
    }
}

/// A point in scale-independent units, relative to a window's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalPoint {
    /// Distance from the left edge, in logical units.
    pub x: f64,
    /// Distance from the top edge, in logical units.
    pub y: f64,
}

impl LogicalPoint {
    /// A point at `(x, y)` logical units.
    #[inline]
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle in the desktop's device-pixel coordinate space.
///
/// Used for monitor geometry, where the origin is the desktop origin rather
/// than a window corner and coordinates can be negative (a monitor placed left
/// of the primary one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhysicalRect {
    /// Left edge, in desktop device pixels.
    pub x: i32,
    /// Top edge, in desktop device pixels.
    pub y: i32,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
}

impl PhysicalRect {
    /// A rectangle at `(x, y)` of `width` × `height` device pixels.
    #[inline]
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The rectangle's size.
    #[inline]
    #[must_use]
    pub const fn size(self) -> PhysicalSize {
        PhysicalSize::new(self.width, self.height)
    }

    /// Whether a desktop-space point lies inside.
    #[must_use]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && (x - self.x) as i64 <= self.width as i64
            && (y - self.y) as i64 <= self.height as i64
    }
}

impl fmt::Display for PhysicalRect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}+{}+{}", self.width, self.height, self.x, self.y)
    }
}

/// A width-to-height ratio, exact.
///
/// Stored as an integer fraction rather than an `f64` because 16:9 is not
/// representable in binary floating point, and a window that must stay
/// aspect-locked across thousands of resize events accumulates that error into
/// a visible drift. Comparisons and the fit math below stay in integers where
/// they can.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    numerator: u32,
    denominator: u32,
}

impl AspectRatio {
    /// 16:9.
    pub const WIDESCREEN: Self = Self {
        numerator: 16,
        denominator: 9,
    };
    /// 4:3.
    pub const CLASSIC: Self = Self {
        numerator: 4,
        denominator: 3,
    };
    /// 1:1.
    pub const SQUARE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    /// A ratio of `width : height`.
    ///
    /// # Panics
    ///
    /// If either term is zero. A degenerate aspect ratio has no correct
    /// behaviour and silently substituting one hides the caller's bug.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        assert!(
            width > 0 && height > 0,
            "aspect ratio terms must be positive"
        );
        Self {
            numerator: width,
            denominator: height,
        }
    }

    /// The width term.
    #[inline]
    #[must_use]
    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    /// The height term.
    #[inline]
    #[must_use]
    pub const fn denominator(self) -> u32 {
        self.denominator
    }

    /// The ratio as a float, for shader constants and letterbox math.
    #[inline]
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    /// The largest size with this aspect ratio that fits inside `bounds`.
    ///
    /// This is the letterbox computation, and it lives here because
    /// `docs/plan/15-windowing.md` makes in-renderer letterboxing the
    /// **universal fallback**: Wayland has no aspect hint at all, and a tiling
    /// window manager can force any size on any backend, so every consumer that
    /// aspect-locks needs this math and none of them should write it twice.
    ///
    /// Returns a size of at most `bounds` in both axes, and at least 1×1.
    #[must_use]
    pub fn fit(self, bounds: PhysicalSize) -> PhysicalSize {
        if bounds.is_empty() {
            return PhysicalSize::new(1, 1);
        }
        // Compare `bounds.width / bounds.height` against `n / d` without
        // dividing: cross-multiply in u64, which cannot overflow for u32 terms.
        let bounds_ratio = u64::from(bounds.width) * u64::from(self.denominator);
        let target_ratio = u64::from(bounds.height) * u64::from(self.numerator);
        if bounds_ratio > target_ratio {
            // Too wide: height is the limit, pillarbox.
            let width = target_ratio / u64::from(self.denominator);
            PhysicalSize::new(width.max(1) as u32, bounds.height)
        } else {
            // Too tall (or exact): width is the limit, letterbox.
            let height = bounds_ratio / u64::from(self.numerator);
            PhysicalSize::new(bounds.width, height.max(1) as u32)
        }
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.numerator, self.denominator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_conversions_round_trip_at_integer_scales() {
        let logical = LogicalSize::new(1280.0, 720.0);
        assert_eq!(logical.to_physical(1.0), PhysicalSize::new(1280, 720));
        assert_eq!(logical.to_physical(2.0), PhysicalSize::new(2560, 1440));
        assert_eq!(
            PhysicalSize::new(2560, 1440).to_logical(2.0),
            LogicalSize::new(1280.0, 720.0)
        );
    }

    #[test]
    fn fractional_scale_rounds_and_never_produces_zero() {
        // 125% is what `fractional-scale-v1` reports as 150/120.
        assert_eq!(
            LogicalSize::new(1280.0, 720.0).to_physical(1.25),
            PhysicalSize::new(1600, 900)
        );
        // A window dragged to nothing still yields a legal swapchain size.
        assert_eq!(
            LogicalSize::new(0.0, 0.0).to_physical(1.0),
            PhysicalSize::new(1, 1)
        );
        assert_eq!(
            LogicalSize::new(f64::NAN, 4.0).to_physical(1.0),
            PhysicalSize::new(1, 4)
        );
        // Overflow saturates rather than wrapping: `1e300 * 1e300` is infinite,
        // and a wrapped `as u32` would have produced a plausible-looking small
        // number.
        assert_eq!(
            LogicalSize::new(1e300, 4.0).to_physical(1e300),
            PhysicalSize::new(u32::MAX, u32::MAX)
        );
    }

    #[test]
    fn empty_sizes_are_detectable_rather_than_clamped_away() {
        // A *reported* size of 0 is real information (minimized), so
        // `PhysicalSize` never clamps — only the logical→physical conversion
        // does, because that one feeds a swapchain.
        let minimized = PhysicalSize::new(0, 0);
        assert!(minimized.is_empty());
        assert_eq!(minimized.aspect(), None);
        assert_eq!(minimized.area(), 0);
        assert!(!PhysicalSize::new(1, 1).is_empty());
        assert_eq!(PhysicalSize::new(3840, 2160).area(), 8_294_400);
        assert_eq!(PhysicalSize::new(1920, 1080).to_string(), "1920x1080px");
        assert_eq!(LogicalSize::new(16.0, 9.0).to_string(), "16x9dp");
    }

    #[test]
    fn letterbox_fit_is_exact_for_the_common_ratios() {
        // 16:9 content in a 16:10 window letterboxes vertically.
        let fitted = AspectRatio::WIDESCREEN.fit(PhysicalSize::new(1920, 1200));
        assert_eq!(fitted, PhysicalSize::new(1920, 1080));

        // 16:9 content in a 21:9 window pillarboxes horizontally.
        let fitted = AspectRatio::WIDESCREEN.fit(PhysicalSize::new(2560, 1080));
        assert_eq!(fitted, PhysicalSize::new(1920, 1080));

        // An exact match is unchanged — no drift, which is the reason the ratio
        // is an integer fraction.
        let exact = PhysicalSize::new(1280, 720);
        let mut size = exact;
        for _ in 0..1_000 {
            size = AspectRatio::WIDESCREEN.fit(size);
        }
        assert_eq!(size, exact);

        assert_eq!(
            AspectRatio::CLASSIC.fit(PhysicalSize::new(1000, 1000)),
            PhysicalSize::new(1000, 750)
        );
        assert_eq!(
            AspectRatio::SQUARE.fit(PhysicalSize::new(1000, 400)),
            PhysicalSize::new(400, 400)
        );
        assert_eq!(
            AspectRatio::WIDESCREEN.fit(PhysicalSize::new(0, 0)),
            PhysicalSize::new(1, 1)
        );
        // Degenerate but legal: never returns a zero dimension.
        assert_eq!(
            AspectRatio::new(1000, 1).fit(PhysicalSize::new(10, 10)),
            PhysicalSize::new(10, 1)
        );
    }

    #[test]
    fn aspect_ratio_accessors_and_display() {
        let ratio = AspectRatio::new(21, 9);
        assert_eq!(ratio.numerator(), 21);
        assert_eq!(ratio.denominator(), 9);
        assert!((ratio.as_f64() - 21.0 / 9.0).abs() < 1e-12);
        assert_eq!(ratio.to_string(), "21:9");
        assert_eq!(AspectRatio::WIDESCREEN, AspectRatio::new(16, 9));
    }

    #[test]
    #[should_panic(expected = "aspect ratio terms must be positive")]
    fn a_degenerate_aspect_ratio_is_rejected() {
        let _ = AspectRatio::new(16, 0);
    }

    #[test]
    fn points_convert_and_hit_test() {
        let point = PhysicalPoint::new(200.0, 100.0);
        assert_eq!(point.to_logical(2.0), LogicalPoint::new(100.0, 50.0));
        assert!(point.is_inside(PhysicalSize::new(640, 360)));
        assert!(!point.is_inside(PhysicalSize::new(100, 100)));
        assert!(!PhysicalPoint::new(-1.0, 0.0).is_inside(PhysicalSize::new(640, 360)));
        assert_eq!(PhysicalPoint::ORIGIN, PhysicalPoint::default());
        assert_eq!(LogicalPoint::new(0.0, 0.0), LogicalPoint::default());
    }

    #[test]
    fn monitor_rectangles_handle_negative_origins() {
        // A second monitor placed to the left of the primary one.
        let left = PhysicalRect::new(-1920, 0, 1920, 1080);
        assert!(left.contains(-1000, 500));
        assert!(!left.contains(10, 500));
        assert_eq!(left.size(), PhysicalSize::new(1920, 1080));
        assert_eq!(left.to_string(), "1920x1080+-1920+0");
    }
}
