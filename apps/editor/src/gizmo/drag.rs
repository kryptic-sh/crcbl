//! What a drag writes: the point on the handle's axis or plane the cursor's
//! ray reaches, and the values that put the entity — or its size — there.

use crcbl::math::{DVec3, Vec2};
use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;

use crate::command::Gesture;

use super::{Axis, Grip, HANDLE_PX, Plane, Snap, widen};

/// The least half extent a scale drag writes, in metres, so the smallest box
/// a drag leaves is a centimetre across.
///
/// A half extent of zero or less is a box with no inside, so a drag past this
/// stops here, snapping or not.
pub const MIN_HALF_EXTENT: f64 = 0.005;

/// Below this, a ray and an axis are parallel and share no closest point, and
/// a ray and a plane share no crossing.
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

/// Where `ray` crosses the plane through `origin` square to `normal` — or
/// [`None`] when it runs along the plane, or crosses it behind the eye.
///
/// A ray `o + s·d` meets the plane `(x - p)·n = 0` at `s = (p - o)·n / (d·n)`.
#[must_use]
pub fn on_plane(ray: &ViewRay, origin: DVec3, normal: Axis) -> Option<DVec3> {
    let direction = widen(ray.direction).normalize_or_zero();
    let start = widen(ray.origin);
    let n = normal.unit();
    let facing = direction.dot(n);
    if facing.abs() <= PARALLEL_BELOW {
        return None;
    }
    let s = (origin - start).dot(n) / facing;
    (s >= 0.0).then(|| start + direction * s)
}

/// The pointer as a drag reads it: the ray through the scene under it, and
/// where it is in the pane's physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// The ray under it, through the pane's camera.
    pub ray: ViewRay,
    /// Where it is, from the pane's top-left.
    pub at: Vec2,
}

/// One leaf a drag sets, and what to.
#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    /// The dotted path, `position.N` or `half_extents.N`.
    pub path: String,
    /// The value it is set to.
    pub value: f64,
}

/// What the press took hold of, and where, in whatever terms that grip moves
/// in — one value, so a grip cannot be paired with another grip's grab.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    /// An axis, `grab` metres along its line from the centre.
    Move { axis: Axis, grab: f64 },
    /// A plane, at the point `grab` on it.
    MovePlane { plane: Plane, grab: DVec3 },
    /// A scale axis, `grab` metres along its line from the centre.
    Scale { axis: Axis, grab: f64 },
    /// The centre, at the pixel `at`, a handle being `span` physical pixels
    /// long at the press's scale: what a uniform scale measures travel in.
    ScaleAll { at: Vec2, span: f32 },
}

/// A drag of one handle, from the press that took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// Whose component is changing.
    pub entity: SceneEntityId,
    /// The gesture every write of this drag belongs to.
    pub gesture: Gesture,
    /// The grip's field — `position` or `half_extents` — when the press landed.
    start: [f64; 3],
    /// What the press took hold of, and where.
    hold: Hold,
    /// The selection's centre when the press landed: the anchor of the axis
    /// line and of the plane.
    origin: DVec3,
}

impl Drag {
    /// A drag of `entity` through `grip`, whose field holds `start` and whose
    /// centre is `origin`, taken by a press at `pointer` with `scale` physical
    /// pixels to a logical one.
    ///
    /// [`None`] when the press's ray runs along the axis or the plane, which
    /// leaves it nothing to take hold of.
    #[must_use]
    pub fn begin(
        entity: SceneEntityId,
        grip: Grip,
        gesture: Gesture,
        start: [f64; 3],
        origin: DVec3,
        pointer: &Pointer,
        scale: f32,
    ) -> Option<Self> {
        let ray = &pointer.ray;
        let hold = match grip {
            Grip::Move(axis) => Hold::Move {
                axis,
                grab: along(ray, origin, axis)?,
            },
            Grip::MovePlane(plane) => Hold::MovePlane {
                plane,
                grab: on_plane(ray, origin, plane.normal())?,
            },
            Grip::Scale(axis) => Hold::Scale {
                axis,
                grab: along(ray, origin, axis)?,
            },
            Grip::ScaleAll => Hold::ScaleAll {
                at: pointer.at,
                span: HANDLE_PX * scale,
            },
        };
        Some(Self {
            entity,
            gesture,
            start,
            hold,
            origin,
        })
    }

    /// The handle being dragged.
    #[must_use]
    pub const fn grip(&self) -> Grip {
        match self.hold {
            Hold::Move { axis, .. } => Grip::Move(axis),
            Hold::MovePlane { plane, .. } => Grip::MovePlane(plane),
            Hold::Scale { axis, .. } => Grip::Scale(axis),
            Hold::ScaleAll { .. } => Grip::ScaleAll,
        }
    }

    /// What the pointer at `pointer` sets, leaf by leaf — on the absolute grid
    /// `snap` describes, while there is one.
    ///
    /// - **An axis** moves its `position.N` by as far as the point on the axis
    ///   line closest to the cursor's ray has moved since the press.
    /// - **A plane** moves both its axes' leaves by as far as the ray's crossing
    ///   of the plane has moved, and never the third: the movement is measured
    ///   in the plane, so it has no part along the normal.
    /// - **A scale axis** adds the same distance along its axis to that
    ///   `half_extents.N`, so the face the handle points at follows the
    ///   cursor's travel metre for metre.
    /// - **The centre** multiplies all three half extents by one plus the
    ///   pointer's travel to the right in handle lengths — a handle's length
    ///   right doubles the size, and its length left is as small as a box goes.
    ///
    /// A half extent is never written below [`MIN_HALF_EXTENT`], and a snapped
    /// one that rounds below it is written as the minimum.
    ///
    /// [`None`] when the ray runs along the axis or plane.
    #[must_use]
    pub fn writes(&self, pointer: &Pointer, snap: Option<Snap>) -> Option<Vec<Write>> {
        let field = self.grip().mode().field();
        let set = |axis: Axis, value: f64| Write {
            path: format!("{field}.{}", axis.index()),
            value,
        };
        let position = |value: f64| snap.map_or(value, |snap| snap.grid(value));
        let half_extent = |value: f64| {
            snap.map_or(value, |snap| snap.scale(value))
                .max(MIN_HALF_EXTENT)
        };
        let start = |axis: Axis| self.start[axis.index()];
        let ray = &pointer.ray;

        Some(match self.hold {
            Hold::Move { axis, grab } => {
                let moved = along(ray, self.origin, axis)? - grab;
                vec![set(axis, position(start(axis) + moved))]
            }
            Hold::MovePlane { plane, grab } => {
                let moved = on_plane(ray, self.origin, plane.normal())? - grab;
                plane
                    .axes()
                    .into_iter()
                    .map(|axis| set(axis, position(start(axis) + moved.dot(axis.unit()))))
                    .collect()
            }
            Hold::Scale { axis, grab } => {
                let moved = along(ray, self.origin, axis)? - grab;
                vec![set(axis, half_extent(start(axis) + moved))]
            }
            Hold::ScaleAll { at, span } => {
                let factor = 1.0 + f64::from((pointer.at.x - at.x) / span);
                Axis::ALL
                    .into_iter()
                    .map(|axis| set(axis, half_extent(start(axis) * factor)))
                    .collect()
            }
        })
    }
}
