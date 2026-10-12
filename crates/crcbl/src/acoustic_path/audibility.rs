//! Which of a sound's candidate routes is heard, and how loudly: the loudest
//! of them, over a range each barrier shortens.
//!
//! A game builds a few [`SoundPath`]s for one event — straight through the
//! wall, round through the door — and [`loudest_path`] picks one. Paths
//! never sum: two routes through two openings do not make a footstep louder
//! than either. What a barrier lets through, how far a sound carries and how
//! loud it starts are the game's, passed in; this is only the selection and
//! the falloff.

use super::SoundPath;

/// The route [`loudest_path`] chose, and what it worked out for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudiblePath {
    /// Its index in the paths given.
    pub path_index: usize,
    /// How far the sound carries along it: the range times its transmission.
    pub effective_range_m: f64,
    /// How loud it arrives: the source gain, times the transmission, times
    /// how much of the effective range is left at the listener.
    pub audible_gain: f64,
    /// The product of every barrier's transmission along it, `1` for none.
    pub transmission: f64,
}

/// Why [`loudest_path`] refused its input. Nothing is chosen on an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudibilityError {
    /// The source gain is not finite or is negative.
    InvalidSourceGain,
    /// The range is not finite or is negative.
    InvalidRange,
    /// A path's distance is not finite or is negative.
    InvalidDistance,
    /// A barrier's transmission is not finite or lies outside `0..=1`.
    InvalidTransmission,
}

impl core::fmt::Display for AudibilityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidSourceGain => "a sound's gain is finite and not negative",
            Self::InvalidRange => "a sound's range is finite and not negative",
            Self::InvalidDistance => "a sound path's distance is finite and not negative",
            Self::InvalidTransmission => "a barrier's transmission is finite and within 0..=1",
        })
    }
}

impl std::error::Error for AudibilityError {}

/// The loudest of `paths` for a sound starting at `source_gain` and carrying
/// `range_m` unobstructed, each barrier passing the share `transmission`
/// gives it; `None` when no path reaches its listener.
///
/// Along a path whose barriers pass a product `t`, the sound carries
/// `range_m · t` and arrives at `source_gain · t · (1 − distance / (range_m ·
/// t))`; a path at or past its effective range is not heard. Of equal gains
/// the earlier path wins, so the choice follows the caller's order and
/// nothing else.
///
/// # Errors
///
/// [`AudibilityError`] for a gain, range, distance or transmission out of
/// range, checked before anything is chosen.
pub fn loudest_path<M>(
    source_gain: f64,
    range_m: f64,
    paths: &[SoundPath<'_, M>],
    transmission: impl Fn(&M) -> f64,
) -> Result<Option<AudiblePath>, AudibilityError> {
    if !source_gain.is_finite() || source_gain < 0.0 {
        return Err(AudibilityError::InvalidSourceGain);
    }
    if !range_m.is_finite() || range_m < 0.0 {
        return Err(AudibilityError::InvalidRange);
    }
    let mut chosen: Option<AudiblePath> = None;
    for (path_index, path) in paths.iter().enumerate() {
        if !path.distance_m.is_finite() || path.distance_m < 0.0 {
            return Err(AudibilityError::InvalidDistance);
        }
        let mut passed = 1.0;
        for barrier in path.barriers {
            let share = transmission(barrier);
            if !(0.0..=1.0).contains(&share) {
                return Err(AudibilityError::InvalidTransmission);
            }
            passed *= share;
        }
        let effective_range_m = range_m * passed;
        if path.distance_m >= effective_range_m {
            continue;
        }
        let audible_gain = source_gain * passed * (1.0 - path.distance_m / effective_range_m);
        if audible_gain <= 0.0 {
            continue;
        }
        if chosen.is_none_or(|best| audible_gain > best.audible_gain) {
            chosen = Some(AudiblePath {
                path_index,
                effective_range_m,
                audible_gain,
                transmission: passed,
            });
        }
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Wall {
        Door,
        Concrete,
    }

    fn passes(wall: &Wall) -> f64 {
        match wall {
            Wall::Door => 0.5,
            Wall::Concrete => 0.1,
        }
    }

    fn path(distance_m: f64, barriers: &[Wall]) -> SoundPath<'_, Wall> {
        SoundPath {
            distance_m,
            barriers,
        }
    }

    /// **The loudest route wins, and routes never add up.** Straight through
    /// concrete is short but muffled; round through the door is longer and
    /// louder; neither sums with the other.
    #[test]
    fn the_loudest_route_is_chosen() {
        let paths = [path(10.0, &[Wall::Concrete]), path(20.0, &[Wall::Door])];
        let heard = loudest_path(1.0, 100.0, &paths, passes).unwrap().unwrap();
        assert_eq!(heard.path_index, 1);
        assert_eq!(heard.transmission, 0.5);
        assert_eq!(heard.effective_range_m, 50.0);
        assert!((heard.audible_gain - 0.5 * (1.0 - 20.0 / 50.0)).abs() < 1e-15);
        // Concrete's 0.1 would carry 10 m, which the 10 m route only reaches.
        let muffled = [path(10.0, &[Wall::Concrete])];
        assert_eq!(loudest_path(1.0, 100.0, &muffled, passes), Ok(None));
    }

    /// **Barriers multiply, an open path passes everything, and a tie keeps
    /// the caller's order.**
    #[test]
    fn barriers_multiply_and_ties_keep_order() {
        let both = [path(1.0, &[Wall::Door, Wall::Door])];
        let heard = loudest_path(2.0, 40.0, &both, passes).unwrap().unwrap();
        assert_eq!(heard.transmission, 0.25);
        assert_eq!(heard.effective_range_m, 10.0);
        let open = [path(3.0, &[]), path(3.0, &[])];
        let heard = loudest_path(1.0, 30.0, &open, passes).unwrap().unwrap();
        assert_eq!((heard.path_index, heard.transmission), (0, 1.0));
        assert_eq!(
            loudest_path(0.0, 30.0, &open, passes),
            Ok(None),
            "silence is not heard"
        );
        assert_eq!(loudest_path(1.0, 30.0, &[], passes), Ok(None));
    }

    /// Bad input is refused before anything is chosen.
    #[test]
    fn bad_input_is_refused() {
        let paths = [path(1.0, &[Wall::Door])];
        assert_eq!(
            loudest_path(-1.0, 10.0, &paths, passes),
            Err(AudibilityError::InvalidSourceGain)
        );
        assert_eq!(
            loudest_path(1.0, f64::NAN, &paths, passes),
            Err(AudibilityError::InvalidRange)
        );
        assert_eq!(
            loudest_path(1.0, 10.0, &[path(-1.0, &[])], passes),
            Err(AudibilityError::InvalidDistance)
        );
        assert_eq!(
            loudest_path(1.0, 10.0, &paths, |_| 1.5),
            Err(AudibilityError::InvalidTransmission)
        );
    }
}
