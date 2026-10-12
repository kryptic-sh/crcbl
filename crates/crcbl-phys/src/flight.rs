//! Point-mass free flight with quadratic drag and gravity, in bounded
//! segments a caller sweeps for collisions: a bullet, a thrown grenade, a
//! shell casing.
//!
//! This is the integrator and nothing else. What a projectile is — its mass,
//! its penetration, its damage, what it keeps after passing through a wall —
//! is the game's; it wraps a [`Flight`] with those. Collision is the caller's
//! too: each [`FlightSegment`] is a path to sweep (`PhysicsSystem::cast_ray`,
//! `sweep_sphere`), and a hit stops the flight there or lets it resume.
//!
//! # The step
//!
//! Over a segment of `dt` the velocity is first damped as quadratic drag
//! alone would damp it, `v / (1 + k |v| dt)`, then gravity is added; the
//! position moves by the distance that damped velocity covers,
//! `v · ln(1 + k |v| dt) / (k |v|)` (`dt` with no drag), plus gravity's
//! `½ g dt²`. Drag alone composes exactly across segments — two half steps
//! land where one whole step does — so the segmenting only approximates how
//! drag and gravity interact. The logarithm is
//! [`crcbl_core::logarithm::ln_1p`], so a host and a client stepping the same
//! flight reach the same bits on every target.

use crcbl_core::logarithm::ln_1p;
use glam::DVec3;

use crate::GravityForce;

/// How a [`Flight`] is stepped. Host and client use the same settings to
/// stay in step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightConfig {
    /// The acceleration of gravity, in m/s².
    pub gravity: DVec3,
    /// The longest segment, in seconds.
    pub max_substep: f64,
    /// The most segments one [`Flight::advance`] makes: a longer duration is
    /// split evenly across this many rather than growing the work without
    /// bound.
    pub max_substeps: u32,
}

impl FlightConfig {
    /// [`GravityForce::EARTH`], a 240th of a second, and 256 segments.
    pub const DEFAULT: Self = Self {
        gravity: GravityForce::EARTH.acceleration,
        max_substep: 1.0 / 240.0,
        max_substeps: 256,
    };

    /// Whether the settings can step a flight: finite gravity, a finite
    /// positive substep and at least one segment.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.gravity.is_finite()
            && self.max_substep.is_finite()
            && self.max_substep > 0.0
            && self.max_substeps > 0
    }
}

impl Default for FlightConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A point mass in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    /// Where it is, in metres.
    pub position: DVec3,
    /// How fast, in m/s.
    pub velocity: DVec3,
    /// Its quadratic drag, per metre: the deceleration is this times the
    /// speed squared. Zero is a vacuum.
    pub quadratic_drag: f64,
}

/// One stretch of a flight, in order: the path to sweep and the velocities
/// at its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightSegment {
    /// Where it began.
    pub start: DVec3,
    /// Where it ended.
    pub end: DVec3,
    /// The velocity at the start.
    pub start_velocity: DVec3,
    /// The velocity at the end.
    pub end_velocity: DVec3,
    /// How long it took, in seconds.
    pub duration: f64,
}

impl Flight {
    /// Flies for `duration` seconds and returns every segment, in order.
    ///
    /// Nothing happens, and no segment comes back, for a duration that is not
    /// finite and positive, invalid settings, or a flight whose state is not
    /// finite or whose drag is negative.
    #[must_use = "the segments are the path to sweep for collisions"]
    pub fn advance(&mut self, duration: f64, config: &FlightConfig) -> Vec<FlightSegment> {
        let mut segments = Vec::new();
        self.advance_into(duration, config, &mut segments);
        segments
    }

    /// [`advance`](Self::advance), appending to `segments`, so a caller
    /// stepping many flights a frame reuses one buffer.
    pub fn advance_into(
        &mut self,
        duration: f64,
        config: &FlightConfig,
        segments: &mut Vec<FlightSegment>,
    ) {
        if !self.can_fly(duration, config) {
            return;
        }
        let steps = (duration / config.max_substep)
            .ceil()
            .clamp(1.0, f64::from(config.max_substeps));
        let dt = duration / steps;
        // `steps` is a whole number between 1 and a `u32`'s range, so the
        // conversion is exact.
        let count = steps as usize;
        segments.reserve(count);
        for _ in 0..count {
            segments.push(self.step(dt, config.gravity));
        }
    }

    /// Flies one segment of at most `max_duration` seconds and at most the
    /// substep, so a caller can sweep it and stop before the next is made.
    /// `None` on [`advance`](Self::advance)'s terms.
    pub fn advance_one_segment(
        &mut self,
        max_duration: f64,
        config: &FlightConfig,
    ) -> Option<FlightSegment> {
        self.can_fly(max_duration, config)
            .then(|| self.step(max_duration.min(config.max_substep), config.gravity))
    }

    /// Whether this flight can be stepped for `duration` under `config`.
    fn can_fly(&self, duration: f64, config: &FlightConfig) -> bool {
        duration.is_finite()
            && duration > 0.0
            && config.is_valid()
            && self.position.is_finite()
            && self.velocity.is_finite()
            && self.quadratic_drag.is_finite()
            && self.quadratic_drag >= 0.0
    }

    /// One segment of `dt`: see the module docs.
    fn step(&mut self, dt: f64, gravity: DVec3) -> FlightSegment {
        let start = self.position;
        let start_velocity = self.velocity;
        let drag = self.quadratic_drag * start_velocity.length();
        let reach = if drag > 0.0 {
            ln_1p(drag * dt) / drag
        } else {
            dt
        };
        let end = start + start_velocity * reach + gravity * (0.5 * dt * dt);
        let end_velocity = start_velocity / (1.0 + drag * dt) + gravity * dt;
        self.position = end;
        self.velocity = end_velocity;
        FlightSegment {
            start,
            end,
            start_velocity,
            end_velocity,
            duration: dt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-9;

    /// The platform's logarithm: the closed form's reference, never a value
    /// the flight uses.
    #[expect(
        clippy::disallowed_methods,
        reason = "an independent reference for the engine's ln_1p"
    )]
    fn platform_ln(x: f64) -> f64 {
        x.ln()
    }

    fn bullet(drag: f64) -> Flight {
        Flight {
            position: DVec3::new(1.0, 2.0, 3.0),
            velocity: DVec3::new(800.0, 0.0, 0.0),
            quadratic_drag: drag,
        }
    }

    /// **In a vacuum it is a parabola**, whatever the segmenting.
    #[test]
    fn without_drag_it_follows_the_parabola() {
        let mut flight = bullet(0.0);
        let start = flight;
        let t = 0.73;
        let segments = flight.advance(t, &FlightConfig::DEFAULT);
        let g = FlightConfig::DEFAULT.gravity;
        let want = start.position + start.velocity * t + g * (0.5 * t * t);
        assert!(
            flight.position.distance(want) < TOLERANCE,
            "{} vs {want}",
            flight.position
        );
        assert!(flight.velocity.distance(start.velocity + g * t) < TOLERANCE);
        assert_eq!(segments.len(), (t * 240.0).ceil() as usize);
    }

    /// **Drag alone composes exactly**: with no gravity, a flight of any
    /// segmenting lands on the closed form `v(t) = v₀ / (1 + k v₀ t)`,
    /// `x(t) = ln(1 + k v₀ t) / k`.
    #[test]
    fn drag_alone_lands_on_the_closed_form() {
        let config = FlightConfig {
            gravity: DVec3::ZERO,
            ..FlightConfig::DEFAULT
        };
        let k = 1e-3;
        let mut flight = bullet(k);
        let (p0, v0) = (flight.position, flight.velocity.x);
        let t = 0.9;
        let _ = flight.advance(t, &config);
        let speed = v0 / (1.0 + k * v0 * t);
        let distance = platform_ln(1.0 + k * v0 * t) / k;
        assert!(
            (flight.velocity.x - speed).abs() < 1e-9,
            "{} vs {speed}",
            flight.velocity.x
        );
        assert!(
            (flight.position.x - (p0.x + distance)).abs() < 1e-9,
            "{} vs {}",
            flight.position.x,
            p0.x + distance
        );
        assert_eq!(flight.position.y, p0.y);
    }

    /// **The segments are the path, in order**: each starts where the last
    /// ended, at its velocity, and their durations add up to the request.
    #[test]
    fn the_segments_join_up_and_add_up() {
        let mut flight = bullet(5e-4);
        let start = flight;
        let segments = flight.advance(0.1, &FlightConfig::DEFAULT);
        assert_eq!(segments[0].start, start.position);
        assert_eq!(segments[0].start_velocity, start.velocity);
        for pair in segments.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
            assert_eq!(pair[0].end_velocity, pair[1].start_velocity);
        }
        let last = segments.last().unwrap();
        assert_eq!(
            (last.end, last.end_velocity),
            (flight.position, flight.velocity)
        );
        let total: f64 = segments.iter().map(|s| s.duration).sum();
        assert!((total - 0.1).abs() < 1e-15);
        assert!(flight.velocity.x < start.velocity.x, "drag slowed it");
        assert!(flight.velocity.y < 0.0, "gravity pulled it down");
    }

    /// **A long advance is capped at the segment count**, split evenly; one
    /// segment at a time is capped at the substep.
    #[test]
    fn the_work_is_bounded() {
        let config = FlightConfig::DEFAULT;
        let mut flight = bullet(1e-3);
        let segments = flight.advance(10.0, &config);
        assert_eq!(segments.len(), config.max_substeps as usize);
        assert!((segments[0].duration - 10.0 / 256.0).abs() < 1e-15);
        let one = flight.advance_one_segment(1.0, &config).unwrap();
        assert_eq!(one.duration, config.max_substep);
        let short = flight.advance_one_segment(1e-4, &config).unwrap();
        assert_eq!(short.duration, 1e-4);
    }

    /// Bad durations, settings and states step nothing.
    #[test]
    fn bad_input_steps_nothing() {
        let config = FlightConfig::DEFAULT;
        let mut flight = bullet(1e-3);
        let before = flight;
        assert!(flight.advance(0.0, &config).is_empty());
        assert!(flight.advance(f64::NAN, &config).is_empty());
        let broken = FlightConfig {
            max_substeps: 0,
            ..config
        };
        assert!(flight.advance(1.0, &broken).is_empty());
        assert!(flight.advance_one_segment(1.0, &broken).is_none());
        assert_eq!(flight, before);
        let mut negative = bullet(-1.0);
        assert!(negative.advance(1.0, &config).is_empty());
    }
}
