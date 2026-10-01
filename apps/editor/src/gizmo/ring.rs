//! The rotate handles: a ring per world axis, drawn and hit tested as the
//! polyline the axis's circle about the selection projects to.
//!
//! A ring is the circle square to its axis through the selection's centre,
//! sized so that its widest reach on screen is [`RING_PX`] — the same
//! constant-screen-size rule the arrows follow. It is sampled at
//! [`RING_SEGMENTS`] points and the polyline through them is both what is
//! drawn and what a press is measured against, so the target is exactly the
//! picture. A ring seen edge-on flattens to a line through the centre, and is
//! still a ring a press can take: the drag measures the angle swept about the
//! centre on screen, which an edge-on ring has as well as a face-on one.

use crcbl::math::{Vec2, Vec3};
use crcbl::render::Camera;

use super::{Axis, narrow};

/// How many points a ring is sampled at: enough that a polyline of this many
/// sides reads as a circle at [`RING_PX`].
pub const RING_SEGMENTS: usize = 48;

/// How far a ring reaches from the centre on screen where it is widest, in
/// logical pixels: the length of an arrow, so the three modes' handles cover
/// the same ground.
pub const RING_PX: f32 = super::HANDLE_PX;

/// The ring about `axis` through `origin`, seen through `camera` in a pane of
/// `extent` physical pixels at `scale` physical pixels per logical one: its
/// [`RING_SEGMENTS`] points in the pane's physical pixels, in order round the
/// circle.
///
/// [`None`] when the centre, or any point of the small world circle the ring
/// is measured from, is behind the eye.
#[must_use]
pub fn ring(
    camera: &Camera,
    extent: (u32, u32),
    origin: Vec3,
    axis: Axis,
    scale: f32,
) -> Option<[Vec2; RING_SEGMENTS]> {
    let centre = camera.pixel_of(origin, extent)?;
    // A world circle a tenth of the distance to the eye across — small
    // enough to stay in front of the eye whenever the centre is, as the
    // arrows' step does — then scaled on screen to the ring's size.
    let step = origin.distance(camera.eye).max(f32::MIN_POSITIVE) * 0.1;
    let unit = narrow(axis.unit());
    let (u, v) = unit.any_orthonormal_pair();
    let offsets: Vec<Vec2> = (0..RING_SEGMENTS)
        .map(|index| {
            let turn = std::f32::consts::TAU * index as f32 / RING_SEGMENTS as f32;
            let (sin, cos) = turn.sin_cos();
            camera
                .pixel_of(origin + (u * cos + v * sin) * step, extent)
                .map(|at| at - centre)
        })
        .collect::<Option<_>>()?;
    let reach = across(camera, origin, step, extent, centre)?;
    let grow = RING_PX * scale / reach;
    let mut points = [Vec2::ZERO; RING_SEGMENTS];
    for (point, offset) in points.iter_mut().zip(offsets) {
        *point = centre + offset * grow;
    }
    Some(points)
}

/// How many pixels a world step of `step` square to the view at `origin`
/// covers on screen: what a ring's world circle is scaled against, so a ring
/// whose circle faces the eye is [`RING_PX`] across whatever the distance.
fn across(
    camera: &Camera,
    origin: Vec3,
    step: f32,
    extent: (u32, u32),
    centre: Vec2,
) -> Option<f32> {
    let view = (origin - camera.eye).normalize_or_zero();
    let side = view.cross(camera.up).try_normalize().unwrap_or_else(|| {
        // Looking straight along the camera's up: any direction square to
        // the view is as good as another.
        view.any_orthonormal_vector()
    });
    let reach = camera
        .pixel_of(origin + side * step, extent)?
        .distance(centre);
    (reach > 0.0).then_some(reach)
}

/// How far `at` is from the closed polyline through `points`.
#[must_use]
pub fn distance_to_ring(at: Vec2, points: &[Vec2; RING_SEGMENTS]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&from, &to)| super::distance_to_segment(at, from, to))
        .fold(f32::INFINITY, f32::min)
}
