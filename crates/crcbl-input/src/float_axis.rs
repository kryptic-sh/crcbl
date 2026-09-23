//! A pad value that arrives as a float, as the seam holds it.
//!
//! Shared by the backends whose platform hands them floats already in the
//! seam's ranges — `web_gamepad` from `Gamepad.axes` and
//! `GamepadButton.value`, and `game_controller` from GameController's element
//! `value`s — so the two cannot disagree about what a stray value becomes.
//! Both platforms bound their values already; the clamp only catches one that
//! strays, since the seam's promise that every axis is finite is not one to
//! rest on someone else's.

/// A stick axis as the seam holds it: −1…1, and 0 for a value that is not a
/// number at all.
///
/// **No flip here** — which axes are +down is the mapping's business, and the
/// backend's snapshot does it.
#[must_use]
pub fn stick_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A trigger as the seam holds it: 0…1, and 0 for a value that is not a
/// number.
#[must_use]
pub fn trigger_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Out-of-range values clamp, and a value that is not a number reads as
    /// rest.
    #[test]
    fn values_are_clamped_and_finite() {
        assert_eq!(stick_axis(1.5), 1.0);
        assert_eq!(stick_axis(-7.0), -1.0);
        assert_eq!(stick_axis(f32::NAN), 0.0);
        assert_eq!(stick_axis(f32::NEG_INFINITY), 0.0);
        assert_eq!(stick_axis(-0.5), -0.5);
        assert_eq!(trigger_axis(-0.1), 0.0);
        assert_eq!(trigger_axis(1.01), 1.0);
        assert_eq!(trigger_axis(f32::INFINITY), 0.0);
        assert_eq!(trigger_axis(0.5), 0.5);
    }
}
