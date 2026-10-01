//! What a drag writes: the point on the handle's axis the cursor's ray comes
//! closest to, and the value that puts the entity there.

use crcbl::math::DVec3;
use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;

use crate::command::Gesture;

use super::{Axis, widen};

/// The step a drag moves in while snapping, in metres.
pub const SNAP_M: f64 = 0.25;

/// Below this, a ray and an axis are parallel and share no closest point.
const PARALLEL_BELOW: f64 = 1e-9;

/// How far along the axis line through `origin` the point closest to `ray`
/// lies, in metres from `origin` — or [`None`] when the two are parallel.
///
/// The closest points of two lines, with both directions unit length: for a
/// ray `o + s·d` and a line `p + t·a`, `w = o - p` and `b = d·a`, the line's
/// parameter is `t = (a·w - b·(d·w)) / (1 - b²)`.
#[must_use]
pub fn along(ray: &ViewRay, origin: DVec3, axis: Axis) -> Option<f64> {
    let direction = widen(ray.direction).normalize_or_zero();
    let unit = axis.unit();
    let w = widen(ray.origin) - origin;
    let b = direction.dot(unit);
    let denominator = 1.0 - b * b;
    (denominator > PARALLEL_BELOW).then(|| (unit.dot(w) - b * direction.dot(w)) / denominator)
}

/// A drag of one handle, from the press that took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// Whose position is moving.
    pub entity: SceneEntityId,
    /// Along which axis.
    pub axis: Axis,
    /// The gesture every write of this drag belongs to.
    pub gesture: Gesture,
    /// The `position.N` value when the press landed.
    start: f64,
    /// Where on the axis line the press grabbed it.
    grab: f64,
    /// The selection's centre when the press landed: the line's anchor.
    origin: DVec3,
}

impl Drag {
    /// A drag of `entity`'s `axis`, whose `position.N` holds `start` and whose
    /// centre is `origin`, taken by a press whose ray is `ray`.
    ///
    /// [`None`] when the ray runs along the axis, which has no point to grab.
    #[must_use]
    pub fn begin(
        entity: SceneEntityId,
        axis: Axis,
        gesture: Gesture,
        start: f64,
        origin: DVec3,
        ray: &ViewRay,
    ) -> Option<Self> {
        Some(Self {
            entity,
            axis,
            gesture,
            start,
            grab: along(ray, origin, axis)?,
            origin,
        })
    }

    /// The `position.N` value the cursor's ray `ray` puts the entity at: moved
    /// along the axis by as far as the closest point has moved since the press,
    /// in whole [`SNAP_M`] steps while `snap` is on.
    ///
    /// [`None`] when the ray runs along the axis.
    #[must_use]
    pub fn value(&self, ray: &ViewRay, snap: bool) -> Option<f64> {
        let moved = along(ray, self.origin, self.axis)? - self.grab;
        let moved = if snap {
            (moved / SNAP_M).round() * SNAP_M
        } else {
            moved
        };
        Some(self.start + moved)
    }
}
