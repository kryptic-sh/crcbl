//! Volume as a percent, and the curves each platform stores it on.
//!
//! PulseAudio and PipeWire store a channel's volume as a linear amplitude and
//! show it on a cubic scale: the percent a person sees is the cube root of the
//! amplitude, times 100. That is PulseAudio's `pa_sw_volume_from_linear`, and
//! `pactl` and `wpctl` show the same number, so a percent here means what it
//! means in every other mixer on the desktop.
//!
//! Windows stores an endpoint's and a session's volume as a scalar from 0 to
//! 1 already on the curve its volume flyout shows, so there the percent is the
//! scalar times 100 ([`percent_from_scalar`]), and there is no boost past it
//! ([`SCALAR_MAX_PERCENT`]).

/// The highest percent a Linux sound server is asked for: PulseAudio's and
/// PipeWire's mixers allow a software boost past 100, and 150 is where
/// `pavucontrol`'s slider ends.
pub const MAX_PERCENT: u32 = 150;

/// The highest percent a scalar volume reaches: Windows' endpoint and
/// session volumes are a scalar from 0 to 1, where 1 is unity and nothing
/// boosts past it, so a set or a step there is held to this rather than to
/// [`MAX_PERCENT`].
pub const SCALAR_MAX_PERCENT: u32 = 100;

/// The percent a linear channel amplitude shows as.
#[must_use]
pub fn percent_from_linear(linear: f32) -> u32 {
    let percent = linear.max(0.0).cbrt() * 100.0;
    // Rounded to the nearest whole percent; the clamp keeps a server's stray
    // value in range so the cast cannot wrap.
    percent.round().clamp(0.0, f32::from(u16::MAX)) as u32
}

/// The linear amplitude a `percent` stores as, held to [`MAX_PERCENT`].
#[must_use]
pub fn linear_from_percent(percent: u32) -> f32 {
    let fraction = percent.min(MAX_PERCENT) as f32 / 100.0;
    fraction * fraction * fraction
}

/// The percent a scalar volume (0 to 1) shows as, rounded to the nearest
/// whole percent: a scalar of 0.55 that a float stores a hair under it still
/// shows as the 55 a person set.
#[must_use]
pub fn percent_from_scalar(scalar: f32) -> u32 {
    // The clamp keeps a driver's stray value in range so the cast cannot
    // wrap.
    (scalar.clamp(0.0, 1.0) * SCALAR_MAX_PERCENT as f32).round() as u32
}

/// The scalar volume a `percent` stores as, held to [`SCALAR_MAX_PERCENT`]:
/// Windows refuses a scalar past 1.
#[must_use]
pub fn scalar_from_percent(percent: u32) -> f32 {
    percent.min(SCALAR_MAX_PERCENT) as f32 / SCALAR_MAX_PERCENT as f32
}

/// The scalar volume a step of `delta` percent from `scalar` lands on, held
/// to `0..=`[`SCALAR_MAX_PERCENT`]. The step is taken on the percent shown,
/// so a scalar shown as 55 steps by 5 to exactly 60.
#[must_use]
pub fn stepped_scalar(scalar: f32, delta: i32) -> f32 {
    scalar_from_percent(stepped(percent_from_scalar(scalar), delta))
}

/// The percent several channels show as together: the mean of each channel's
/// percent, as `pactl` averages them. Zero for no channels.
#[must_use]
pub fn percent_from_channels(channels: &[f32]) -> u32 {
    if channels.is_empty() {
        return 0;
    }
    let sum: u32 = channels.iter().copied().map(percent_from_linear).sum();
    let count = u32::try_from(channels.len()).unwrap_or(u32::MAX);
    (sum + count / 2) / count
}

/// The percent a step of `delta` from `percent` lands on, held to
/// `0..=`[`MAX_PERCENT`].
#[must_use]
pub fn stepped(percent: u32, delta: i32) -> u32 {
    let stepped = i64::from(percent) + i64::from(delta);
    u32::try_from(stepped.clamp(0, i64::from(MAX_PERCENT))).expect("held to MAX_PERCENT")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against values `pactl` prints for the same amplitudes: 0.125 linear
    /// is 50 %, 1.0 is 100 %, and 3.375 (1.5 cubed) is 150 %.
    #[test]
    fn the_cubic_scale_matches_what_pactl_shows() {
        assert_eq!(percent_from_linear(0.125), 50);
        assert_eq!(percent_from_linear(1.0), 100);
        assert_eq!(percent_from_linear(3.375), 150);
        assert_eq!(percent_from_linear(0.0), 0);
        assert_eq!(percent_from_linear(-1.0), 0, "a stray negative is silence");
        assert!((linear_from_percent(50) - 0.125).abs() < 1e-6);
        assert!(
            (linear_from_percent(200) - 3.375).abs() < 1e-6,
            "held to 150 %"
        );
        for percent in [0, 1, 33, 50, 99, 100, 101, 149, 150] {
            assert_eq!(
                percent_from_linear(linear_from_percent(percent)),
                percent,
                "{percent} % survives the round trip"
            );
        }
    }

    #[test]
    fn channels_average_and_a_step_stays_in_range() {
        assert_eq!(percent_from_channels(&[0.125, 1.0]), 75);
        assert_eq!(percent_from_channels(&[]), 0);
        assert_eq!(stepped(50, 5), 55);
        assert_eq!(stepped(3, -5), 0);
        assert_eq!(stepped(148, 5), MAX_PERCENT);
    }

    /// **A scalar volume is held to 100 % both ways in**: a set past it and a
    /// step across it land on unity, never on a boost Windows would refuse.
    #[test]
    fn a_scalar_volume_is_a_percent_held_to_unity() {
        assert_eq!(percent_from_scalar(0.0), 0);
        assert_eq!(percent_from_scalar(0.549_999_95), 55, "a hair under 0.55");
        assert_eq!(percent_from_scalar(1.0), 100);
        assert_eq!(percent_from_scalar(1.3), 100, "a stray value is held");
        assert_eq!(percent_from_scalar(-0.2), 0);
        assert!((scalar_from_percent(40) - 0.4).abs() < f32::EPSILON);
        assert!(
            (scalar_from_percent(150) - 1.0).abs() < f32::EPSILON,
            "a set is held to 100 %"
        );
        assert!(
            (stepped_scalar(0.98, 5) - 1.0).abs() < f32::EPSILON,
            "a step is held to 100 %"
        );
        assert!((stepped_scalar(0.553, 5) - 0.6).abs() < f32::EPSILON);
        assert!(stepped_scalar(0.02, -5).abs() < f32::EPSILON);
        for percent in 0..=SCALAR_MAX_PERCENT {
            assert_eq!(
                percent_from_scalar(scalar_from_percent(percent)),
                percent,
                "{percent} % survives the round trip"
            );
        }
    }
}
