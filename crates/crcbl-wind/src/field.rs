//! The field itself: the two layers, the weather over them, and the one
//! formula every consumer reads.
//!
//! `docs/plan/56-wind.md`'s decision 6 states it once, and this module is that
//! statement:
//!
//! ```text
//! v(x) = I(x) · S · D(x) · (1 + gust(x − o)) + Σ motors(x) + grid(x)
//! ```
//!
//! Rung W1 builds the first term. The motor list is W3 and the wake grid is W5;
//! neither has a placeholder here, because a term that is always zero reads as
//! a term that works.

use crcbl_phys::WindQuery;
use crcbl_shaders::wind::WindParams;
use glam::{DVec2, DVec3};

use crate::layers::{DirectionLayer, IntensityLayer, fract};
use crate::scroll::ScrollOffset;
use crate::weather::Weather;

/// Below this squared length a composed direction has no direction left, and
/// the base direction answers instead.
///
/// It is reached where the direction layer's four neighbours cancel — a texel
/// of `(1, 0)` beside one of `(−1, 0)`, which is an authoring mistake rather
/// than a wind — and the alternative to a floor there is a `NaN` in a velocity
/// that a physics integrator carries into a position. The value is `1e−12`,
/// a millionth of a unit vector's length, so nothing an authored layer produces
/// on purpose can fall under it.
pub const MIN_DIRECTION_LENGTH_SQUARED: f64 = 1e-12;

/// The wind field: two authored layers, a weather state and one scroll offset.
///
/// **Stateless except for the scroll.** Decision 2's weather "changes the
/// moment its state does and nothing in it carries history", and decision 7
/// puts every lag, overshoot and still-swinging branch in the consumer. The one
/// thing that accumulates is [`ScrollOffset`], because a gust that travels has
/// to remember how far it has come.
#[derive(Debug, Clone, PartialEq)]
pub struct WindField {
    weather: Weather,
    scroll: ScrollOffset,
    direction: DirectionLayer,
    intensity: IntensityLayer,
}

impl WindField {
    /// Builds a field from its weather and its two layers, with no scroll yet.
    #[must_use]
    pub const fn new(
        weather: Weather,
        direction: DirectionLayer,
        intensity: IntensityLayer,
    ) -> Self {
        Self {
            weather,
            scroll: ScrollOffset::ZERO,
            direction,
            intensity,
        }
    }

    /// The weather over the layers.
    #[inline]
    #[must_use]
    pub const fn weather(&self) -> Weather {
        self.weather
    }

    /// Replaces the weather. The field answers differently from the next sample
    /// on; nothing fades.
    #[inline]
    pub const fn set_weather(&mut self, weather: Weather) {
        self.weather = weather;
    }

    /// How far the gusts have travelled.
    #[inline]
    #[must_use]
    pub const fn scroll(&self) -> ScrollOffset {
        self.scroll
    }

    /// The coarse direction layer.
    #[inline]
    #[must_use]
    pub const fn direction_layer(&self) -> &DirectionLayer {
        &self.direction
    }

    /// The fine intensity layer.
    #[inline]
    #[must_use]
    pub const fn intensity_layer(&self) -> &IntensityLayer {
        &self.intensity
    }

    /// Advances the field by one tick of `dt` seconds.
    ///
    /// The only thing this moves is the scroll offset.
    pub fn advance(&mut self, dt: f64) {
        self.scroll.advance(&self.weather, dt);
    }

    /// The gust coordinate of a world position — `x − o` on the XZ plane.
    ///
    /// The quantity decision 3 is about. Two points a whole number of gust
    /// steps apart along the wind have the same gust coordinate a tick apart,
    /// which is what "the same gust arrives separated by distance over gust
    /// speed" means and what per-object scrolling loses.
    #[inline]
    #[must_use]
    pub fn gust_coordinate(&self, position: DVec3) -> DVec2 {
        DVec2::new(position.x, position.z) - self.scroll.metres()
    }

    /// The factor the gust front multiplies the base speed by — `1 + gust(x−o)`.
    ///
    /// Decision 3's "smooth triangle waves of `dot(x, direction) / λ` minus a
    /// phase — `x·x·(3 − 2x)` over `abs(frac(x + 0.5)·2 − 1)`, the Crysis
    /// construction". It costs no texture tap and no transcendental, which is
    /// why W1 can carry a travelling gust while the baked noise of decision 3's
    /// other half waits for W2.
    ///
    /// The result is `1` everywhere while [`Weather::gust_amplitude`] is zero,
    /// which is where every constructor leaves it.
    #[must_use]
    pub fn gust_factor(&self, position: DVec3) -> f64 {
        let along = self.gust_coordinate(position).dot(self.weather.direction())
            / self.weather.gust_wavelength;
        1.0 + self.weather.gust_amplitude * (2.0 * smooth_triangle(along) - 1.0)
    }

    /// The wind velocity at a world position, in metres per second.
    ///
    /// Horizontal: `y` is always zero. Decision 6 lists a terrain-following
    /// vertical component and an updraft channel as candidates and decides
    /// neither, and a component that is always zero is cheaper to add later
    /// than to explain now.
    ///
    /// The position's own `y` is read for nothing — the field is a function of
    /// the XZ plane at this rung — and is taken rather than dropped at the call
    /// site so that a consumer sampling at a body's centre does not have to
    /// know that.
    #[must_use]
    pub fn sample(&self, position: DVec3) -> DVec3 {
        let (x, z) = (position.x, position.z);
        let intensity = self.intensity.sample(x, z);
        let direction = self.direction_at(x, z);
        // Decision 6's first term, in its order: the intensity, the weather's
        // speed and the gust factor are one scalar, and the direction carries
        // it. Nothing short-circuits on a zero intensity — "calm means calm"
        // is a property of this multiply, and a branch that returned early
        // would make the test that checks it a test of the branch.
        let speed = intensity * self.weather.speed * self.gust_factor(position);
        DVec3::new(direction.x * speed, 0.0, direction.y * speed)
    }

    /// The unit direction the wind blows towards at a world position.
    ///
    /// `D(x)` in decision 6. The layer's texel is a **deflection** and the
    /// weather's direction is the prevailing wind, and the two compose by the
    /// product of the complex numbers they stand for: `(a + bi)(c + di)`. That
    /// is a rotation written without an angle — decision 9's last line — so
    /// turning the weather turns the whole field and the terrain's channelling
    /// rides along with it, and a layer of `(1, 0)` leaves the prevailing wind
    /// exactly as it was.
    ///
    /// The renormalise afterwards is decision 1's square root, spent once here
    /// rather than once per tap.
    #[must_use]
    pub fn direction_at(&self, x: f64, z: f64) -> DVec2 {
        let base = self.weather.direction();
        let deflection = self.direction.sample(x, z);
        let turned = DVec2::new(
            base.x * deflection.x - base.y * deflection.y,
            base.x * deflection.y + base.y * deflection.x,
        );
        let length_squared = turned.length_squared();
        // Written as the positive test so a `NaN` length — which compares false
        // against everything — takes the fallback rather than reaching the
        // square root.
        if length_squared > MIN_DIRECTION_LENGTH_SQUARED {
            turned / length_squared.sqrt()
        } else {
            base
        }
    }

    /// The uniform block the GPU copy of this field reads, for a frame whose
    /// camera is at `camera`.
    ///
    /// Everything here is camera-relative or already wrapped, which is decision
    /// 3's "the renderer receives the offset relative to the camera so a float
    /// never holds a world-scale coordinate":
    ///
    /// * **The gust phase** is `dot(camera − o, direction) / λ` with its whole
    ///   part dropped. The gust front has period one in that quantity, so
    ///   dropping whole turns changes nothing and what crosses to the GPU is a
    ///   number in `0..1` however far the session has scrolled.
    /// * **Each layer's UV** is the camera's texture coordinate, wrapped. The
    ///   shader adds `posRel.xz · uvPerMetre`, and under `repeat` addressing
    ///   that reads exactly the texels the world coordinate names.
    ///
    /// The `f32` narrowing happens here, in the crate that owns the `f64`
    /// formula, so the agreement test compares two answers to the same question
    /// rather than two questions.
    #[must_use]
    pub fn gpu_params(&self, camera: DVec3) -> WindParams {
        let direction = self.weather.direction();
        let phase = self.gust_coordinate(camera).dot(direction) / self.weather.gust_wavelength;
        let direction_grid = self.direction.grid();
        let intensity_grid = self.intensity.grid();
        let uv = |grid: crate::layers::LayerGrid| {
            let at = grid.uv(camera.x, camera.z);
            let per_metre = grid.uv_per_metre();
            (
                [at.x as f32, at.y as f32],
                [per_metre.x as f32, per_metre.y as f32],
            )
        };
        let (direction_uv, direction_uv_per_metre) = uv(direction_grid);
        let (intensity_uv, intensity_uv_per_metre) = uv(intensity_grid);
        WindParams {
            base_direction: [direction.x as f32, direction.y as f32],
            base_speed: self.weather.speed as f32,
            gust_amplitude: self.weather.gust_amplitude as f32,
            gust_phase: fract(phase) as f32,
            inv_gust_wavelength: (1.0 / self.weather.gust_wavelength) as f32,
            direction_uv,
            direction_uv_per_metre,
            intensity_uv,
            intensity_uv_per_metre,
        }
    }
}

impl WindQuery for WindField {
    fn wind_at(&self, position: DVec3) -> DVec3 {
        self.sample(position)
    }
}

/// Crysis's smoothed triangle wave: period one, range `0..=1`, `C¹` at both
/// ends of every period.
///
/// `abs(frac(u + 0.5)·2 − 1)` is the triangle and `s·s·(3 − 2s)` is the
/// smoothstep over it, which is decision 3's construction verbatim. Every
/// operation is one IEEE-754 specifies exactly — no transcendental, so decision
/// 9's rule is satisfied by there being nothing to satisfy it about.
#[inline]
#[must_use]
pub fn smooth_triangle(u: f64) -> f64 {
    let s = (fract(u + 0.5) * 2.0 - 1.0).abs();
    s * s * (3.0 - 2.0 * s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::LayerGrid;

    /// A 1×1 direction layer of `(1, 0)` — "no deflection" everywhere.
    fn undeflected() -> DirectionLayer {
        let grid = LayerGrid::new(1, 1, 8.0).expect("a real grid");
        DirectionLayer::from_rgba8(grid, &[255, 128, 0, 255]).expect("one texel")
    }

    /// A 1×1 intensity layer at `red / 255` everywhere.
    fn flat_intensity(red: u8) -> IntensityLayer {
        let grid = LayerGrid::new(1, 1, 2.0).expect("a real grid");
        IntensityLayer::from_rgba8(grid, &[red, 0, 0, 255]).expect("one texel")
    }

    fn field(red: u8, speed: f64) -> WindField {
        let weather = Weather::new(DVec2::X, speed).expect("a real direction");
        WindField::new(weather, undeflected(), flat_intensity(red))
    }

    #[test]
    fn a_blank_direction_layer_leaves_the_prevailing_wind_alone() {
        let mut field = field(255, 5.0);
        for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-0.6, 0.8)] {
            let mut weather = field.weather();
            weather
                .set_direction(DVec2::new(x, y))
                .expect("a real direction");
            field.set_weather(weather);
            let sample = field.sample(DVec3::new(13.0, 2.0, -7.0));
            let direction = DVec2::new(sample.x, sample.z).normalize();
            // The bound is the eight-bit encoding's own: a unorm midpoint is
            // 128/255 rather than 0.5, so "no deflection" is 0.22° off and the
            // composed direction inherits exactly that. Anything looser would
            // stop being a check; anything tighter is a claim about a texture
            // format this layer does not have.
            assert!(
                (direction - DVec2::new(x, y).normalize()).length() < 0.005,
                "an undeflected layer answered {direction} for a wind towards ({x}, {y})"
            );
            assert!((sample.length() - 5.0).abs() < 1e-12);
            assert_eq!(sample.y, 0.0, "the field is horizontal at this rung");
        }
    }

    #[test]
    fn the_smooth_triangle_has_period_one_and_stays_in_range() {
        for step in -400..400 {
            let u = f64::from(step) * 0.0137;
            let here = smooth_triangle(u);
            assert!(
                (0.0..=1.0).contains(&here),
                "smooth_triangle({u}) is {here}"
            );
            for turns in [1.0, 3.0, -5.0, 1000.0] {
                let there = smooth_triangle(u + turns);
                assert!(
                    (here - there).abs() < 1e-12,
                    "the wave is not periodic: {here} at {u}, {there} a turn on"
                );
            }
        }
        // The two extremes, and that they are where the construction puts them.
        assert!(smooth_triangle(0.0).abs() < 1e-15);
        assert!((smooth_triangle(0.5) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn gusting_is_off_until_it_is_asked_for() {
        let field = field(255, 5.0);
        for step in 0..50 {
            let position = DVec3::new(f64::from(step) * 3.3, 0.0, 1.0);
            assert_eq!(field.gust_factor(position), 1.0);
        }
    }

    #[test]
    fn a_gust_swings_the_speed_about_the_base_and_never_reverses_it() {
        let mut field = field(255, 5.0);
        let mut weather = field.weather();
        weather.set_gust(1.0, 20.0).expect("a real gust");
        field.set_weather(weather);
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for step in 0..500 {
            let position = DVec3::new(f64::from(step) * 0.11, 0.0, 0.0);
            let factor = field.gust_factor(position);
            assert!(factor >= 0.0, "a gust of amplitude 1 reversed the wind");
            lowest = lowest.min(factor);
            highest = highest.max(factor);
        }
        assert!(lowest < 0.01, "the trough is reached: {lowest}");
        assert!(highest > 1.99, "the crest is reached: {highest}");
    }

    #[test]
    fn a_degenerate_direction_layer_falls_back_rather_than_answering_nan() {
        // Two texels that cancel exactly at the midpoint between them.
        let grid = LayerGrid::new(2, 1, 4.0).expect("a real grid");
        let layer = DirectionLayer::from_rgba8(grid, &[255, 128, 0, 255, 0, 128, 0, 255])
            .expect("two texels");
        let weather = Weather::new(DVec2::X, 3.0).expect("a real direction");
        let field = WindField::new(weather, layer, flat_intensity(255));
        // Texel centres are at x = 2 and x = 6; halfway is x = 4.
        let sample = field.sample(DVec3::new(4.0, 0.0, 0.0));
        assert!(sample.is_finite(), "{sample} is not a velocity");
        assert!((sample.length() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn the_trait_answers_what_the_field_does() {
        let field = field(200, 7.0);
        let query: &dyn WindQuery = &field;
        for step in 0..20 {
            let position = DVec3::new(f64::from(step), 1.0, -f64::from(step));
            assert_eq!(query.wind_at(position), field.sample(position));
        }
    }
}
