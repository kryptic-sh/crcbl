//! The base a UI's scale is built on: fitting a reference window, or the
//! window's own scale factor.
//!
//! A host draws its UI through a
//! [`DrawList`](crcbl_ui::draw_list::DrawList) at
//! [`set_scale`](crcbl_ui::draw_list::DrawList::set_scale)`(base * multiplier)`,
//! where the multiplier is the player's `[engine.video] ui_scale`
//! ([`settings::ui_scale`](crate::settings::ui_scale)) and the base is the
//! host's choice:
//!
//! - [`fit_scale`] for a UI laid out in logical pixels for one window size —
//!   [`DEFAULT_WINDOW_SIZE`](crate::engine::DEFAULT_WINDOW_SIZE), usually — and
//!   kept in proportion at any other.
//! - [`window_scale_factor`] for a UI laid out in the window system's logical
//!   units, so it is the same physical size on a high-DPI display as on any
//!   other.
//!
//! Both answer the number itself, not only a list's setting, because a host
//! places things that are not in the list — a 3D view's viewport, an icon
//! rendered to a rectangle of its own — and those scale by the same factor.

use crcbl_shell::{LogicalSize, Shell, WindowId};

/// How many window pixels one logical pixel covers for a UI laid out for
/// `reference` and drawn into a swapchain of `extent` pixels: the smaller of
/// the two axis ratios, so the whole layout fits — a window narrower than the
/// reference's aspect fits the layout's width.
///
/// `1.0` for an empty extent or a degenerate reference, where there is no ratio
/// to take, so nothing downstream divides by zero.
#[must_use]
pub fn fit_scale(extent: (u32, u32), reference: LogicalSize) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a ratio of two window sizes, which an f32 holds to well under a pixel"
    )]
    let fit =
        (f64::from(extent.0) / reference.width).min(f64::from(extent.1) / reference.height) as f32;
    if fit.is_finite() && fit > 0.0 {
        fit
    } else {
        1.0
    }
}

/// `window`'s device pixels per logical unit, as the window system reports it
/// now: two on a display scaled to 200%.
///
/// `1.0` for a window that is not configured yet or is gone — a UI drawn at
/// one for a frame is a UI that can still be read.
#[must_use]
pub fn window_scale_factor<S: Shell + ?Sized>(shell: &S, window: WindowId) -> f64 {
    shell
        .window_state(window)
        .ok()
        .and_then(|state| state.scale_factor())
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::DEFAULT_WINDOW_SIZE;

    /// **The fit is the smaller axis ratio against the reference**, times
    /// nothing: the multiplier is the caller's to apply.
    #[test]
    fn the_fit_is_the_smaller_axis_ratio_to_the_reference() {
        let fit = |extent| fit_scale(extent, DEFAULT_WINDOW_SIZE);
        assert_eq!(fit((960, 720)), 1.0);
        assert_eq!(fit((1920, 1080)), 1.5);
        assert_eq!(fit((2560, 1440)), 2.0);
        assert_eq!(fit((480, 360)), 0.5);
        // Narrower than the reference's aspect: the width decides.
        assert_eq!(fit((960, 1440)), 1.0);
        assert_eq!(fit_scale((1000, 1000), LogicalSize::new(500.0, 250.0)), 2.0);
    }

    /// **Nothing to take a ratio of is a scale of one**, not zero or infinity.
    #[test]
    fn an_empty_extent_or_reference_fits_at_one() {
        assert_eq!(fit_scale((0, 0), DEFAULT_WINDOW_SIZE), 1.0);
        assert_eq!(fit_scale((0, 720), DEFAULT_WINDOW_SIZE), 1.0);
        assert_eq!(fit_scale((960, 720), LogicalSize::new(0.0, 0.0)), 1.0);
    }

    /// **The window's scale factor is the shell's**, before and after the
    /// window moves to a display at another scale, and one for a window the
    /// shell has not configured or no longer has.
    #[test]
    fn the_window_scale_factor_follows_the_shell() {
        let mut shell = crcbl_shell::HeadlessShell::new().with_scale_factor(1.25);
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        assert_eq!(window_scale_factor(&shell, window), 1.0, "not configured");
        shell.pump(&mut |_| {});
        shell.pump(&mut |_| {});
        assert_eq!(window_scale_factor(&shell, window), 1.25);
        shell
            .change_scale_factor(window, 2.0)
            .expect("the window is live");
        assert_eq!(window_scale_factor(&shell, window), 2.0);
        shell.destroy_window(window).expect("the window is live");
        assert_eq!(window_scale_factor(&shell, window), 1.0, "gone");
    }
}
