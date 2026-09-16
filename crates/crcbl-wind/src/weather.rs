//! The world's weather: a base direction, a base speed, and the gust front's
//! shape.
//!
//! `docs/plan/56-wind.md`'s decision 2: "Over the two layers sits a weather
//! state: a base direction and a base speed." Nothing here carries history —
//! the field changes the moment its state does, and lag, overshoot and a branch
//! still swinging after the gust has passed are each consumer's own state
//! (decision 7).

use glam::DVec2;

use crate::WindError;

/// The five wind speeds *God of War*'s authors named from the Beaufort scale.
///
/// `docs/plan/56-wind.md`'s decision 2 lists them, and this is the scale a
/// preset picks from rather than a number a fixture invents. They are speeds
/// only: a preset cannot pick a direction for a world it knows nothing about,
/// so [`Weather::from_beaufort`] takes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Beaufort {
    /// 0.5 m/s — leaves hang still.
    Still,
    /// 2 m/s.
    Calm,
    /// 5 m/s.
    Breezy,
    /// 9 m/s.
    Strong,
    /// 15 m/s.
    Violent,
}

impl Beaufort {
    /// Every preset, weakest first — for a fixture that sweeps the scale.
    pub const ALL: [Self; 5] = [
        Self::Still,
        Self::Calm,
        Self::Breezy,
        Self::Strong,
        Self::Violent,
    ];

    /// The preset's base speed in metres per second.
    #[inline]
    #[must_use]
    pub const fn speed(self) -> f64 {
        match self {
            Self::Still => 0.5,
            Self::Calm => 2.0,
            Self::Breezy => 5.0,
            Self::Strong => 9.0,
            Self::Violent => 15.0,
        }
    }
}

/// Metres between one gust front and the next, when a caller does not choose.
///
/// **A starting value, not a measurement.** `docs/plan/56-wind.md` states the
/// gust front's *construction* — decision 3's smoothed triangle wave — and no
/// wavelength, because the distance between gusts is a thing a fixture tunes
/// against what it is drawing. It changes nothing until a caller raises
/// [`Weather::gust_amplitude`] off the zero [`Weather::new`] leaves it at.
pub const DEFAULT_GUST_WAVELENGTH: f64 = 32.0;

/// A base direction, a base speed, and the travelling gust front over them.
///
/// The direction is private because it is a **unit** horizontal vector and
/// nothing outside may hand it one that is not: every constructor normalises,
/// and a direction that cannot be normalised is refused rather than stored and
/// documented against. That is also why it is a vector and never an angle —
/// decision 9's last line, and what lets [`crate::WindField`] compose it with
/// the direction layer by a complex product rather than by `atan2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weather {
    /// Unit horizontal vector the wind blows **towards**, in the XZ plane.
    direction: DVec2,
    /// Base speed in m/s — `S` in decision 6's formula.
    pub speed: f64,
    /// Metres per second the gust front travels at.
    ///
    /// Its own quantity, because decision 3 accumulates the scroll offset as
    /// `o ← o + direction · gust speed · Δt` rather than at the base speed. It
    /// starts equal to [`Self::speed`], which is the physical reading — a gust
    /// is carried by the air it is in — and a fixture that wants a front
    /// rolling faster or slower than the air says so.
    pub gust_speed: f64,
    /// Peak fractional swing of the gust about 1, so `0.25` means the wind
    /// ranges over three quarters to five quarters of its base speed.
    ///
    /// **Zero from every constructor**, which makes a field with no gusting
    /// exactly `I · S · D` — decision 6's formula with its gust term at one.
    /// Above 1 the factor would go negative and reverse the wind, so
    /// [`Weather::set_gust`] refuses it.
    pub gust_amplitude: f64,
    /// Metres between one gust front and the next.
    pub gust_wavelength: f64,
}

impl Weather {
    /// Builds a weather state from a direction and a base speed.
    ///
    /// The direction is normalised. Gusting starts off — see
    /// [`Self::gust_amplitude`] — and [`Self::gust_speed`] starts at `speed`.
    ///
    /// # Errors
    ///
    /// [`WindError::DegenerateDirection`] if `direction` is zero-length or not
    /// finite; [`WindError::NegativeSpeed`] if `speed` is negative or not
    /// finite.
    pub fn new(direction: DVec2, speed: f64) -> Result<Self, WindError> {
        if !speed.is_finite() || speed < 0.0 {
            return Err(WindError::NegativeSpeed { speed });
        }
        Ok(Self {
            direction: unit(direction)?,
            speed,
            gust_speed: speed,
            gust_amplitude: 0.0,
            gust_wavelength: DEFAULT_GUST_WAVELENGTH,
        })
    }

    /// Builds a weather state at one of the [`Beaufort`] presets' speeds.
    ///
    /// # Errors
    ///
    /// As [`Self::new`]; a preset's speed is always a valid one, so only the
    /// direction can be refused.
    pub fn from_beaufort(direction: DVec2, preset: Beaufort) -> Result<Self, WindError> {
        Self::new(direction, preset.speed())
    }

    /// The unit horizontal direction the wind blows towards.
    #[inline]
    #[must_use]
    pub const fn direction(self) -> DVec2 {
        self.direction
    }

    /// Points the wind somewhere else, normalising what it is given.
    ///
    /// # Errors
    ///
    /// [`WindError::DegenerateDirection`] if `direction` is zero-length or not
    /// finite. The old direction is kept when it is.
    pub fn set_direction(&mut self, direction: DVec2) -> Result<(), WindError> {
        self.direction = unit(direction)?;
        Ok(())
    }

    /// Turns gusting on, or changes its shape.
    ///
    /// # Errors
    ///
    /// [`WindError::GustAmplitude`] if `amplitude` is outside `0..=1` or not
    /// finite — above one the gust factor goes negative and the wind blows
    /// backwards, which is not a gust. [`WindError::GustWavelength`] if
    /// `wavelength` is not a positive finite number of metres.
    pub fn set_gust(&mut self, amplitude: f64, wavelength: f64) -> Result<(), WindError> {
        if !amplitude.is_finite() || !(0.0..=1.0).contains(&amplitude) {
            return Err(WindError::GustAmplitude { amplitude });
        }
        if !wavelength.is_finite() || wavelength <= 0.0 {
            return Err(WindError::GustWavelength { wavelength });
        }
        self.gust_amplitude = amplitude;
        self.gust_wavelength = wavelength;
        Ok(())
    }
}

/// Normalises a horizontal direction, refusing one that has no direction.
///
/// The floor is on the squared length, so the square root is only taken once
/// the vector is known to have one.
fn unit(direction: DVec2) -> Result<DVec2, WindError> {
    let length_squared = direction.length_squared();
    if !length_squared.is_finite() || length_squared <= 0.0 {
        return Err(WindError::DegenerateDirection { direction });
    }
    Ok(direction / length_squared.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five speeds `docs/plan/56-wind.md`'s decision 2 names, in order.
    #[test]
    fn the_presets_are_the_beaufort_speeds_the_plan_names() {
        let speeds: Vec<f64> = Beaufort::ALL.iter().map(|preset| preset.speed()).collect();
        assert_eq!(speeds, vec![0.5, 2.0, 5.0, 9.0, 15.0]);
    }

    #[test]
    fn a_direction_is_stored_as_a_unit_vector() {
        let weather = Weather::new(DVec2::new(3.0, 4.0), 5.0).expect("a real direction");
        assert!((weather.direction().length() - 1.0).abs() < 1e-15);
        assert_eq!(weather.direction(), DVec2::new(0.6, 0.8));
    }

    #[test]
    fn a_direction_with_no_direction_is_refused() {
        assert!(matches!(
            Weather::new(DVec2::ZERO, 5.0),
            Err(WindError::DegenerateDirection { .. })
        ));
        assert!(matches!(
            Weather::new(DVec2::new(f64::NAN, 0.0), 5.0),
            Err(WindError::DegenerateDirection { .. })
        ));
        let mut weather = Weather::new(DVec2::X, 5.0).expect("a real direction");
        assert!(weather.set_direction(DVec2::ZERO).is_err());
        assert_eq!(weather.direction(), DVec2::X, "the old direction is kept");
    }

    #[test]
    fn a_negative_speed_is_refused() {
        assert!(matches!(
            Weather::new(DVec2::X, -1.0),
            Err(WindError::NegativeSpeed { .. })
        ));
    }

    #[test]
    fn gusting_starts_off_and_travels_with_the_air() {
        let weather = Weather::from_beaufort(DVec2::X, Beaufort::Breezy).expect("a real direction");
        assert_eq!(weather.gust_amplitude, 0.0);
        assert_eq!(weather.gust_speed, weather.speed);
    }

    #[test]
    fn a_gust_that_would_reverse_the_wind_is_refused() {
        let mut weather = Weather::new(DVec2::X, 5.0).expect("a real direction");
        assert!(matches!(
            weather.set_gust(1.5, 32.0),
            Err(WindError::GustAmplitude { .. })
        ));
        assert!(matches!(
            weather.set_gust(0.5, 0.0),
            Err(WindError::GustWavelength { .. })
        ));
        assert_eq!(
            weather.gust_amplitude, 0.0,
            "neither refusal wrote anything"
        );
        weather.set_gust(0.25, 16.0).expect("a real gust");
        assert_eq!(weather.gust_amplitude, 0.25);
        assert_eq!(weather.gust_wavelength, 16.0);
    }
}
