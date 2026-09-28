//! Acoustic paths: which barriers a sound crosses on its way to a listener, and
//! how far it travels.
//!
//! [`build_acoustic_path`] walks one caller-authored polyline — a source,
//! optional waypoints through openings, a listener — against caller-supplied
//! axis-aligned obstacles, and returns the route's length and the barrier of
//! every obstacle it crossed, each once, in the order the sound meets them.
//!
//! ```
//! use crcbl::acoustic_path::{
//!     AcousticObstacle, AcousticPathConfig, AcousticRoute, build_acoustic_path,
//! };
//! use crcbl::math::DVec3;
//! use crcbl::phys::Aabb;
//!
//! #[derive(Clone, Copy, Debug, PartialEq)]
//! enum Wall {
//!     Wood,
//! }
//!
//! let door = AcousticObstacle {
//!     bounds: Aabb::new(DVec3::new(1.0, -1.0, -1.0), DVec3::new(2.0, 1.0, 1.0)),
//!     barrier: Wall::Wood,
//! };
//! let route = AcousticRoute {
//!     source: DVec3::ZERO,
//!     waypoints: &[],
//!     listener: DVec3::new(3.0, 0.0, 0.0),
//! };
//! let built = build_acoustic_path(AcousticPathConfig::default(), route, &[door])?;
//! assert_eq!(built.total_distance_m, 3.0);
//! assert_eq!(built.barriers, [Wall::Wood]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Ported from EW, where it was `acoustic_path` plus the `SoundPath` type from
//! its `sound` module. Every name, field, error variant, configuration limit and
//! ordering rule is EW's own; the one change is the material.
//!
//! **Here, in the umbrella, for [`crate::ui_nav`]'s reason**: it is the join of
//! two crates that should not know each other. Its obstacles are
//! [`crcbl_phys::Aabb`], tested with that box's own slab test, and its result
//! feeds [`crcbl_audio`]'s cue grammar. `crcbl-audio` is pure DSP with the
//! device at its edge and takes no physics dependency, and `crcbl-phys` has no
//! business knowing about sound, so the one crate that already names both is
//! where the geometry of a sound's route lives.
//!
//! # The material is the caller's type
//!
//! The engine cannot know what a game's walls are made of, or what each one
//! costs a sound, so the barrier an obstacle carries is a type parameter `M`.
//! EW's own barrier enum is its `M`; a game with materials keyed by id can pass
//! a `u16`. The builder only copies an obstacle's `M` into the result once per
//! crossing, so the one bound it asks for is `M: Clone`; every other type here
//! carries `M` without asking anything of it.
//!
//! Transmission losses per material, sound kinds and who hears what stay with
//! the game. This module answers only the geometric question.
//!
//! # Obstruction, not loudness — and the distance counted once
//!
//! [`spatial`](crate::audio::spatial)'s [`compute_cue`](crate::audio::spatial::compute_cue)
//! applies *distance rolloff* and nothing else: it has no idea a wall stands
//! between the two positions it is handed. This module supplies the other half,
//! the obstruction: the barriers crossed, which a game turns into a gain with
//! its own per-material losses.
//!
//! It also reports [`BuiltAcousticPath::total_distance_m`], the length of the
//! whole polyline, which is **not** the distance `compute_cue` sees. The cue
//! grammar measures the straight line between the emitter and listener it is
//! given; a route bent through a doorway is longer than that. A game therefore
//! chooses where distance attenuation happens, and must choose one place:
//!
//! - If its gain already accounts for `total_distance_m`, hand `compute_cue` a
//!   *direction* — the emitter offset scaled to lie within
//!   [`CueGrammar::rolloff_start`](crate::audio::spatial::CueGrammar::rolloff_start)
//!   of the listener — so the pan is cued and the rolloff contributes nothing.
//!   Otherwise the sound is attenuated for its distance twice.
//! - If it lets `compute_cue`'s rolloff handle distance, its gain should come
//!   from the barriers alone, accepting that the rolloff is for the straight
//!   line and not the route.
//!
//! # Determinism
//!
//! The result is a pure function of the configuration, the route and the
//! obstacle slice: no world query, no hashing, no allocation-order dependence.
//! Barriers are ordered by where the route first enters their obstacle —
//! segment by segment, then by distance along the segment — and obstacles
//! entered at the same point keep their input order. An obstacle is reported
//! once however many times the route crosses it, because it is one physical
//! barrier: a route that doubles back through a door hears the door once.
//!
//! # Bounds
//!
//! The work is `obstacles × segments`, so [`AcousticPathConfig`] caps both, and
//! a call over either cap is refused rather than truncated. Non-finite
//! positions and obstacle bounds, and empty obstacles, are refused too: nothing
//! here returns a partial path.

use crcbl_phys::Aabb;
use glam::DVec3;

/// [`AcousticPathConfig::default`]'s obstacle cap.
pub const DEFAULT_MAXIMUM_OBSTACLES: usize = 64;

/// [`AcousticPathConfig::default`]'s waypoint cap.
pub const DEFAULT_MAXIMUM_WAYPOINTS: usize = 8;

/// One static obstruction that can attenuate sound along an acoustic route.
///
/// `bounds` is inclusive, as [`Aabb`] is everywhere: a route that only grazes
/// a face or an edge has crossed the obstacle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticObstacle<M> {
    /// The obstacle's extent, in the same space as the route.
    pub bounds: Aabb,
    /// What the obstacle is made of, reported once when the route crosses it.
    pub barrier: M,
}

/// A caller-authored route from a source, through optional openings, to a
/// listener.
///
/// The route is taken as given. Choosing waypoints — through a doorway, around
/// a corner — is the caller's pathfinding, not this module's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticRoute<'a> {
    /// Where the sound starts.
    pub source: DVec3,
    /// Intermediate points, visited in order.
    pub waypoints: &'a [DVec3],
    /// Where the sound is heard.
    pub listener: DVec3,
}

/// Bounds for caller-provided obstacle and opening-route work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcousticPathConfig {
    /// The most obstacles one call may test against.
    pub maximum_obstacles: usize,
    /// The most waypoints one route may carry, excluding its two ends.
    pub maximum_waypoints: usize,
}

impl Default for AcousticPathConfig {
    fn default() -> Self {
        Self {
            maximum_obstacles: DEFAULT_MAXIMUM_OBSTACLES,
            maximum_waypoints: DEFAULT_MAXIMUM_WAYPOINTS,
        }
    }
}

impl AcousticPathConfig {
    /// Whether both caps are nonzero. A zero cap is refused as a configuration
    /// mistake rather than read as "no obstacles" or "direct routes only".
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.maximum_obstacles > 0 && self.maximum_waypoints > 0
    }
}

/// Construction rejected invalid route-builder tuning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcousticPathConfigError {
    /// A cap in the [`AcousticPathConfig`] is zero.
    InvalidConfiguration,
}

impl core::fmt::Display for AcousticPathConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                f.write_str("acoustic path configuration has a zero obstacle or waypoint cap")
            }
        }
    }
}

impl std::error::Error for AcousticPathConfigError {}

/// An acoustic route was rejected without constructing a partial path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcousticPathBuildError {
    /// A cap in the [`AcousticPathConfig`] is zero.
    InvalidConfiguration,
    /// More obstacles than [`AcousticPathConfig::maximum_obstacles`].
    TooManyObstacles,
    /// More waypoints than [`AcousticPathConfig::maximum_waypoints`].
    TooManyWaypoints,
    /// The source, the listener or a waypoint has a non-finite coordinate.
    InvalidPosition,
    /// An obstacle's bounds are non-finite or empty.
    InvalidObstacleBounds,
    /// A segment's length, or the running total, overflowed to a non-finite
    /// value, or a crossing could not be located along its segment.
    InvalidSegmentLength,
}

impl core::fmt::Display for AcousticPathBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                f.write_str("acoustic path configuration has a zero obstacle or waypoint cap")
            }
            Self::TooManyObstacles => {
                f.write_str("more acoustic obstacles than the configured cap")
            }
            Self::TooManyWaypoints => f.write_str("more route waypoints than the configured cap"),
            Self::InvalidPosition => f.write_str("acoustic route has a non-finite position"),
            Self::InvalidObstacleBounds => {
                f.write_str("acoustic obstacle has non-finite or empty bounds")
            }
            Self::InvalidSegmentLength => {
                f.write_str("acoustic route segment length is not finite")
            }
        }
    }
}

impl std::error::Error for AcousticPathBuildError {}

/// One candidate route from source to listener, borrowed: its length and the
/// barriers it crosses.
///
/// This is the shape a game's propagation model consumes — typically several
/// of them, one per candidate route — and [`BuiltAcousticPath::as_sound_path`]
/// is how a built path lends itself as one.
#[derive(Debug, PartialEq)]
pub struct SoundPath<'a, M> {
    /// The route's length, in the route's units (metres, by convention).
    pub distance_m: f64,
    /// Each physical barrier occurs at most once; callers retain identity while
    /// building this bounded list rather than reporting mesh overlaps.
    pub barriers: &'a [M],
}

// By hand rather than derived: a derive would demand `M: Clone`/`M: Copy`, and
// a borrowed slice is copyable whatever it holds.
impl<M> Clone for SoundPath<'_, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for SoundPath<'_, M> {}

/// Owned path data ready to borrow as a [`SoundPath`].
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltAcousticPath<M> {
    /// The length of the whole polyline, source through every waypoint to the
    /// listener. See the module docs for how this relates to
    /// [`compute_cue`](crate::audio::spatial::compute_cue)'s rolloff.
    pub total_distance_m: f64,
    /// The barrier of every obstacle the route crossed, once each, in the order
    /// the route first entered them.
    pub barriers: Vec<M>,
}

impl<M> BuiltAcousticPath<M> {
    /// Borrows this path as a [`SoundPath`].
    #[must_use]
    pub fn as_sound_path(&self) -> SoundPath<'_, M> {
        SoundPath {
            distance_m: self.total_distance_m,
            barriers: &self.barriers,
        }
    }
}

/// Validates configuration before it is used to build an acoustic path.
///
/// # Errors
///
/// [`AcousticPathConfigError::InvalidConfiguration`] when
/// [`AcousticPathConfig::is_valid`] is false.
pub fn validate_acoustic_path_config(
    config: AcousticPathConfig,
) -> Result<(), AcousticPathConfigError> {
    if config.is_valid() {
        Ok(())
    } else {
        Err(AcousticPathConfigError::InvalidConfiguration)
    }
}

/// Builds one bounded acoustic path without engine/world queries or
/// pathfinding.
///
/// Touching an obstacle boundary counts as an intersection, matching the
/// inclusive [`Aabb`] interface. Obstacles are emitted once by input identity,
/// ordered by their first route encounter and then input order for equal entry.
/// Zero-length segments are skipped: they travel nowhere and cross nothing,
/// even inside an obstacle.
///
/// # Errors
///
/// Every [`AcousticPathBuildError`] variant, as documented there. The checks
/// run before any segment is walked, except the segment-length ones, and no
/// error returns a partial path.
pub fn build_acoustic_path<M: Clone>(
    config: AcousticPathConfig,
    route: AcousticRoute<'_>,
    obstacles: &[AcousticObstacle<M>],
) -> Result<BuiltAcousticPath<M>, AcousticPathBuildError> {
    if !config.is_valid() {
        return Err(AcousticPathBuildError::InvalidConfiguration);
    }
    if obstacles.len() > config.maximum_obstacles {
        return Err(AcousticPathBuildError::TooManyObstacles);
    }
    if route.waypoints.len() > config.maximum_waypoints {
        return Err(AcousticPathBuildError::TooManyWaypoints);
    }
    validate_route(route, obstacles)?;

    let mut points = Vec::with_capacity(route.waypoints.len() + 2);
    points.push(route.source);
    points.extend_from_slice(route.waypoints);
    points.push(route.listener);

    let mut total_distance_m = 0.0;
    let mut barriers = Vec::new();
    let mut encountered = vec![false; obstacles.len()];
    for segment in points.windows(2) {
        let start = segment[0];
        let end = segment[1];
        let offset = end - start;
        let length_m = offset.length();
        if !length_m.is_finite() {
            return Err(AcousticPathBuildError::InvalidSegmentLength);
        }
        if length_m == 0.0 {
            continue;
        }
        total_distance_m += length_m;
        if !total_distance_m.is_finite() {
            return Err(AcousticPathBuildError::InvalidSegmentLength);
        }

        let direction = offset / length_m;
        let inverse_direction = direction.recip();
        let direction_is_negative = [direction.x < 0.0, direction.y < 0.0, direction.z < 0.0];
        let mut crossings = Vec::new();
        for (index, obstacle) in obstacles.iter().enumerate() {
            if encountered[index]
                || !obstacle.bounds.intersect_ray(
                    start,
                    inverse_direction,
                    direction_is_negative,
                    0.0,
                    length_m,
                )
            {
                continue;
            }
            let Some(entry_distance_m) = segment_entry_distance(start, direction, obstacle.bounds)
            else {
                return Err(AcousticPathBuildError::InvalidSegmentLength);
            };
            crossings.push((entry_distance_m, index));
        }
        // Equal entry distances fall back to input index, so the order is
        // fixed by the inputs alone and never by how the crossings were found.
        crossings.sort_by(
            |(left_distance, left_index), (right_distance, right_index)| {
                left_distance
                    .total_cmp(right_distance)
                    .then(left_index.cmp(right_index))
            },
        );
        for (_, index) in crossings {
            encountered[index] = true;
            barriers.push(obstacles[index].barrier.clone());
        }
    }

    Ok(BuiltAcousticPath {
        total_distance_m,
        barriers,
    })
}

fn validate_route<M>(
    route: AcousticRoute<'_>,
    obstacles: &[AcousticObstacle<M>],
) -> Result<(), AcousticPathBuildError> {
    if !route.source.is_finite()
        || !route.listener.is_finite()
        || route.waypoints.iter().any(|waypoint| !waypoint.is_finite())
    {
        return Err(AcousticPathBuildError::InvalidPosition);
    }
    if obstacles.iter().any(|obstacle| {
        !obstacle.bounds.min.is_finite()
            || !obstacle.bounds.max.is_finite()
            || obstacle.bounds.is_empty()
    }) {
        return Err(AcousticPathBuildError::InvalidObstacleBounds);
    }
    Ok(())
}

/// How far along the unit `direction` from `start` the ray enters `bounds`,
/// clamped to zero when `start` is already inside; `None` when it misses.
///
/// The slab test, written out rather than taken from [`Aabb::intersect_ray`],
/// because that answers only *whether* and the ordering needs *where*.
fn segment_entry_distance(start: DVec3, direction: DVec3, bounds: Aabb) -> Option<f64> {
    let start = start.to_array();
    let direction = direction.to_array();
    let minimum = bounds.min.to_array();
    let maximum = bounds.max.to_array();
    let mut entry = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;

    for axis in 0..3 {
        if direction[axis] == 0.0 {
            if start[axis] < minimum[axis] || start[axis] > maximum[axis] {
                return None;
            }
            continue;
        }
        let first = (minimum[axis] - start[axis]) / direction[axis];
        let second = (maximum[axis] - start[axis]) / direction[axis];
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
    }

    (entry <= exit).then_some(entry.max(0.0))
}

#[cfg(test)]
mod tests;
