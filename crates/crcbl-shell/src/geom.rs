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

/// An axis-aligned rectangle in scale-independent units, relative to a
/// window's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalRect {
    /// The top-left corner.
    pub origin: LogicalPoint,
    /// Width and height.
    pub size: LogicalSize,
}

impl LogicalRect {
    /// A rectangle at `origin` of `size`.
    #[inline]
    #[must_use]
    pub const fn new(origin: LogicalPoint, size: LogicalSize) -> Self {
        Self { origin, size }
    }
}

/// An axis-aligned rectangle in device pixels.
///
/// Each use names its space. Monitor geometry is in the desktop's, where the
/// origin is the desktop origin rather than a window corner and coordinates can
/// be negative (a monitor placed left of the primary one);
/// [`Shell::set_text_input_area`](crate::Shell::set_text_input_area) is in a
/// window's client area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhysicalRect {
    /// Left edge, in device pixels of the space the use names.
    pub x: i32,
    /// Top edge, in device pixels of the space the use names.
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
    ///
    /// **Half-open**: the left and top edges belong to this rectangle, the
    /// right and bottom ones do not. Monitors tile edge to edge — the standard
    /// side-by-side layout is `(0,0,1920,1080)` next to `(1920,0,1920,1080)` —
    /// so an inclusive far edge would put the first column of the second
    /// monitor inside *both*, and the monitor lookup in the X11 backend returns
    /// the first match. Half-open makes the tiling a partition.
    ///
    /// The subtraction is widened to `i64` *before* it happens, not after: a
    /// rectangle at a negative origin and a point near [`i32::MAX`] overflow an
    /// `i32` difference, which panics in debug and wraps to "inside" in
    /// release.
    #[must_use]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && (x as i64 - self.x as i64) < self.width as i64
            && (y as i64 - self.y as i64) < self.height as i64
    }

    /// This rectangle with its corners rounded to `radius` device pixels, as
    /// the union of rectangles
    /// [`Shell::set_input_region`](crate::Shell::set_input_region) takes.
    ///
    /// An **outer bound**: every pixel a corner's quarter circle covers at all
    /// is inside, so a panel drawn with antialiased rounded corners keeps its
    /// whole edge — which matters on Win32, where the region also clips what
    /// the window draws. Neighbouring rows with the same inset are merged, so
    /// the count grows with the radius rather than the height, and the
    /// rectangles never overlap. The radius is clamped to half the shorter
    /// side; an empty rectangle gives none.
    #[must_use]
    pub fn rounded(self, radius: u32) -> Vec<Self> {
        if self.width == 0 || self.height == 0 {
            return Vec::new();
        }
        let radius = radius.min(self.width / 2).min(self.height / 2);
        let insets: Vec<u32> = (0..radius).map(|row| corner_inset(radius, row)).collect();
        let mut rects: Vec<Self> = Vec::new();
        let mut push = |top: u32, height: u32, inset: u32| {
            if height == 0 {
                return;
            }
            let row = Self::new(
                self.x.saturating_add_unsigned(inset),
                self.y.saturating_add_unsigned(top),
                self.width - 2 * inset,
                height,
            );
            match rects.last_mut() {
                Some(last)
                    if last.x == row.x
                        && last.width == row.width
                        && i64::from(last.y) + i64::from(last.height) == i64::from(row.y) =>
                {
                    last.height += height;
                }
                _ => rects.push(row),
            }
        };
        for (row, &inset) in (0..radius).zip(&insets) {
            push(row, 1, inset);
        }
        push(radius, self.height - 2 * radius, 0);
        for (row, &inset) in (0..radius).zip(&insets).rev() {
            push(self.height - 1 - row, 1, inset);
        }
        rects
    }
}

/// How many columns of row `row` (counted from the edge) lie wholly outside a
/// corner's quarter circle of `radius`: the columns the disc does not reach.
///
/// The circle's centre is `radius` in from both edges, and the row's nearest
/// point to it is its inner edge, `radius - row - 1` away vertically, where
/// the circle reaches `s = sqrt(radius² - d²)` from the centre. A column
/// overlaps the disc exactly when its inner edge is past `radius - s`, so the
/// inset is `floor(radius - s)`, which in integers is `radius - ceil(s)`.
fn corner_inset(radius: u32, row: u32) -> u32 {
    let (radius, d) = (u64::from(radius), u64::from(radius - row - 1));
    let squared = radius * radius - d * d;
    let root = squared.isqrt();
    let ceil = if root * root == squared {
        root
    } else {
        root + 1
    };
    u32::try_from(radius - ceil).expect("the inset is at most the radius, which is a u32")
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
    /// the windowing rules in `docs/notes/backends.md` make in-renderer letterboxing the
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

    #[test]
    fn tiled_monitors_partition_the_desktop() {
        // The standard side-by-side layout. Every point belongs to exactly one
        // of the two, which is what a monitor lookup that returns the first
        // match needs to be correct.
        let first = PhysicalRect::new(0, 0, 1920, 1080);
        let second = PhysicalRect::new(1920, 0, 1920, 1080);
        for (x, y) in [(0, 0), (1919, 0), (0, 1079), (1919, 1079)] {
            assert!(first.contains(x, y), "({x},{y}) is on the first monitor");
            assert!(!second.contains(x, y));
        }
        for (x, y) in [(1920, 0), (3839, 1079)] {
            assert!(second.contains(x, y), "({x},{y}) is on the second monitor");
            assert!(!first.contains(x, y), "the shared edge belongs to one");
        }
        // Past the far edge in either axis is outside.
        assert!(!first.contains(0, 1080));
        assert!(!second.contains(3840, 0));
    }

    #[test]
    fn hit_testing_a_far_away_point_does_not_overflow() {
        // `x - self.x` overflows an i32 here; the widening must happen first.
        let negative_origin = PhysicalRect::new(-1, 0, 1920, 1080);
        assert!(!negative_origin.contains(i32::MAX, 0));
        assert!(!PhysicalRect::new(i32::MIN, 0, 1920, 1080).contains(i32::MAX, 0));
        // The widest legal rectangle really does reach that far — and stops one
        // pixel short of `i32::MIN + u32::MAX`, because the far edge is open.
        let widest = PhysicalRect::new(i32::MIN, 0, u32::MAX, 1080);
        assert!(widest.contains(i32::MAX - 1, 0));
        assert!(!widest.contains(i32::MAX, 0));
    }

    /// Whether a disc of `radius` centred `radius` in from a corner overlaps
    /// the pixel `(x, y)` counted from that corner: the pixel's nearest point
    /// to the centre is its inner corner, and the overlap is strict, as an
    /// antialiased edge with any coverage at all needs.
    fn corner_covers(radius: i64, x: i64, y: i64) -> bool {
        if x >= radius || y >= radius {
            return true;
        }
        let (dx, dy) = (radius - x - 1, radius - y - 1);
        dx * dx + dy * dy < radius * radius
    }

    #[test]
    fn a_rounded_rectangle_is_exactly_the_pixels_its_corners_cover() {
        for (rect, radius) in [
            (PhysicalRect::new(0, 0, 40, 30), 10),
            (PhysicalRect::new(-7, 13, 31, 64), 6),
            (PhysicalRect::new(5, 5, 20, 20), 10),
            // Clamped to half the shorter side.
            (PhysicalRect::new(0, 0, 9, 50), 30),
            (PhysicalRect::new(0, 0, 12, 8), 1),
            (PhysicalRect::new(3, 4, 12, 8), 0),
        ] {
            let rects = rect.rounded(radius);
            let r = i64::from(radius.min(rect.width / 2).min(rect.height / 2));
            let (width, height) = (i64::from(rect.width), i64::from(rect.height));
            for y in 0..height {
                for x in 0..width {
                    let covered = corner_covers(r, x.min(width - 1 - x), y.min(height - 1 - y));
                    let px = i32::try_from(i64::from(rect.x) + x).expect("small");
                    let py = i32::try_from(i64::from(rect.y) + y).expect("small");
                    let hits = rects.iter().filter(|part| part.contains(px, py)).count();
                    assert_eq!(
                        hits,
                        usize::from(covered),
                        "{rect} r{radius}: pixel ({x},{y}) in {hits} of {rects:?}"
                    );
                }
            }
            // Nothing outside the rectangle.
            for part in &rects {
                assert!(part.x >= rect.x && part.y >= rect.y, "{part} in {rect}");
                assert!(
                    i64::from(part.x) + i64::from(part.width) <= i64::from(rect.x) + width
                        && i64::from(part.y) + i64::from(part.height) <= i64::from(rect.y) + height,
                    "{part} in {rect}"
                );
            }
        }
    }

    #[test]
    fn a_rounded_corner_insets_rows_by_the_known_amounts_and_merges_equal_ones() {
        // Radius 10: the row nearest the edge reaches the circle at
        // `10 - sqrt(19)`, so five whole columns are outside; then four, two,
        // two, one, and nothing from row five on.
        let rects = PhysicalRect::new(0, 0, 100, 60).rounded(10);
        let insets: Vec<(i32, u32)> = rects.iter().map(|part| (part.x, part.height)).collect();
        assert_eq!(
            insets,
            [
                (5, 1),
                (4, 1),
                (2, 2),
                (1, 1),
                (0, 50),
                (1, 1),
                (2, 2),
                (4, 1),
                (5, 1)
            ]
        );
        assert_eq!(rects[4], PhysicalRect::new(0, 5, 100, 50));
        // No radius is the rectangle itself, and an empty one is nothing.
        let square = PhysicalRect::new(3, 4, 12, 8);
        assert_eq!(square.rounded(0), [square]);
        assert!(PhysicalRect::new(0, 0, 0, 8).rounded(4).is_empty());
    }
}
