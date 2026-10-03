//! What a drag writes: the point on the handle's axis or plane the cursor's
//! ray reaches, or the angle it sweeps round a ring, and the values that put
//! the entity — its place, its size or its turn — there.

use crcbl::math::{DQuat, DVec3, Vec2};
use crcbl::registry::Rotation;
use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;

use crate::command::Gesture;

use super::{Axis, Grip, HANDLE_PX, POSITION, Plane, ROTATION, Snap, widen};

/// The least half extent a scale drag writes, in metres, so the smallest box
/// a drag leaves is a centimetre across.
///
/// A half extent of zero or less is a box with no inside, so a drag past this
/// stops here, snapping or not.
pub const MIN_HALF_EXTENT: f64 = 0.005;

/// Below this, a ray and an axis are parallel and share no closest point, and
/// a ray and a plane share no crossing.
const PARALLEL_BELOW: f64 = 1e-9;

/// How near the selection's centre on screen, in physical pixels, a turning
/// pointer has no angle round it: the direction from the centre to a pointer
/// that close is noise.
pub const TURN_DEAD_PX: f32 = 2.0;

/// How far along the line through `origin` in the unit direction `along`
/// the point closest to `ray` lies, in metres from `origin` — or [`None`]
/// when the two are parallel.
///
/// The closest points of two lines, with both directions unit length: for a
/// ray `o + s·d` and a line `p + t·a`, `w = o - p` and `b = d·a`, the line's
/// parameter is `t = (a·w - b·(d·w)) / (1 - b²)`.
#[must_use]
pub fn along(ray: &ViewRay, origin: DVec3, unit: DVec3) -> Option<f64> {
    let direction = widen(ray.direction).normalize_or_zero();
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

/// The angle the pointer has swept round `centre` on screen going from `from`
/// to `to`, in radians, counter-clockwise as a person sees it — the pane's
/// `y` runs down — and wrapped into `-π..=π`.
///
/// [`None`] when either point is within [`TURN_DEAD_PX`] of the centre.
#[must_use]
pub fn swept(centre: Vec2, from: Vec2, to: Vec2) -> Option<f64> {
    let (from, to) = (from - centre, to - centre);
    if from.length() < TURN_DEAD_PX || to.length() < TURN_DEAD_PX {
        return None;
    }
    // `atan2` on a y-down pane grows clockwise, so the visible
    // counter-clockwise sweep is the press's angle less the pointer's.
    let angle_of = |offset: Vec2| f64::from(offset.y).atan2(f64::from(offset.x));
    let angle = angle_of(from) - angle_of(to);
    let turn = std::f64::consts::TAU;
    Some(angle - turn * (angle / turn).round())
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
    /// The dotted path: `position.N`, `half_extents.N` or `rotation.L`.
    pub path: String,
    /// The value it is set to.
    pub value: f64,
}

/// Where a turn starts: what a ring's drag needs of the entity when the press
/// lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    /// Its rotation when the press landed.
    pub rotation: DQuat,
    /// Its placing component's `position`, when it has one — which a turn
    /// about the centre swings round it if the two differ.
    pub position: Option<DVec3>,
    /// Its centre on screen, in the pane's physical pixels: what the angle is
    /// swept round.
    pub centre: Vec2,
    /// `1.0` when the ring's axis points towards the eye or across the view,
    /// `-1.0` when it points away: a sweep seen from behind the axis turns
    /// the other way about it.
    pub facing: f64,
}

/// One entity a translate drag moves: whose, through which system's
/// component, and the `position` it held when the press landed.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// Whose component moves.
    pub entity: SceneEntityId,
    /// The system holding the component that places it.
    pub system: String,
    /// Its `position` when the press landed.
    pub start: [f64; 3],
}

/// What a translate drag moves: every selected entity with a `position`, by
/// one delta — `docs/plan/08-editor.md`'s shared-pivot translate. See
/// [`Drag::spread`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Each entity it moves, in selection order.
    pub members: Vec<Member>,
}

/// What the press took hold of, and where, in whatever terms that grip moves
/// in — one value, so a grip cannot be paired with another grip's grab.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    /// An axis, `grab` metres along its line from the centre.
    Move { axis: Axis, grab: f64 },
    /// A plane, at the point `grab` on it.
    MovePlane { plane: Plane, grab: DVec3 },
    /// A scale axis, turned into the box's frame as `unit`, `grab` metres
    /// along its line from the centre.
    Scale { axis: Axis, unit: DVec3, grab: f64 },
    /// The centre, at the pixel `at`, a handle being `span` physical pixels
    /// long at the press's scale: what a uniform scale measures travel in.
    ScaleAll { at: Vec2, span: f32 },
    /// A ring, pressed at the pixel `at`, turning from `from`.
    Rotate { axis: Axis, at: Vec2, from: Turn },
}

/// A drag of one handle, from the press that took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// Whose component a resize or a turn changes: the primary selected
    /// entity. A translate moves its [`Group`] instead, whose pivot `start`
    /// and `origin` then are.
    pub entity: SceneEntityId,
    /// The gesture every write of this drag belongs to.
    pub gesture: Gesture,
    /// The grip's field — `position` or `half_extents` — when the press
    /// landed; a turn holds what it starts from in its [`Turn`].
    start: [f64; 3],
    /// What the press took hold of, and where.
    hold: Hold,
    /// The selection's centre when the press landed: the anchor of the axis
    /// line and of the plane, and the pivot of a turn.
    origin: DVec3,
}

impl Drag {
    /// A drag of `entity` through `grip` — an arrow, a plane or a scale
    /// handle — whose field holds `start` and whose centre is `origin`, its
    /// box turned by `frame`, taken by a press at `pointer` with `scale`
    /// physical pixels to a logical one.
    ///
    /// A scale axis is measured along the box's own axis, `frame` turning the
    /// world's: a half extent is along the box's axes, and so is its handle.
    ///
    /// [`None`] when the press's ray runs along the axis or the plane, which
    /// leaves it nothing to take hold of, and for a ring, which
    /// [`turn`](Self::turn) begins.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        entity: SceneEntityId,
        grip: Grip,
        gesture: Gesture,
        start: [f64; 3],
        origin: DVec3,
        frame: DQuat,
        pointer: &Pointer,
        scale: f32,
    ) -> Option<Self> {
        let ray = &pointer.ray;
        let hold = match grip {
            Grip::Move(axis) => Hold::Move {
                axis,
                grab: along(ray, origin, axis.unit())?,
            },
            Grip::MovePlane(plane) => Hold::MovePlane {
                plane,
                grab: on_plane(ray, origin, plane.normal())?,
            },
            Grip::Scale(axis) => {
                let unit = frame * axis.unit();
                Hold::Scale {
                    axis,
                    unit,
                    grab: along(ray, origin, unit)?,
                }
            }
            Grip::ScaleAll => Hold::ScaleAll {
                at: pointer.at,
                span: HANDLE_PX * scale,
            },
            Grip::Rotate(_) => return None,
        };
        Some(Self {
            entity,
            gesture,
            start,
            hold,
            origin,
        })
    }

    /// A drag of `entity`'s ring about `axis`, through its centre `origin`,
    /// taken by a press at `at` on the pane, turning from `from`.
    #[must_use]
    pub const fn turn(
        entity: SceneEntityId,
        axis: Axis,
        gesture: Gesture,
        origin: DVec3,
        at: Vec2,
        from: Turn,
    ) -> Self {
        Self {
            entity,
            gesture,
            start: [0.0; 3],
            hold: Hold::Rotate { axis, at, from },
            origin,
        }
    }

    /// The handle being dragged.
    #[must_use]
    pub const fn grip(&self) -> Grip {
        match self.hold {
            Hold::Move { axis, .. } => Grip::Move(axis),
            Hold::MovePlane { plane, .. } => Grip::MovePlane(plane),
            Hold::Scale { axis, .. } => Grip::Scale(axis),
            Hold::ScaleAll { .. } => Grip::ScaleAll,
            Hold::Rotate { axis, .. } => Grip::Rotate(axis),
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
    /// - **A scale axis** adds the same distance along the box's own axis to
    ///   that `half_extents.N`, so the face the handle points at follows the
    ///   cursor's travel metre for metre.
    /// - **The centre** multiplies all three half extents by one plus the
    ///   pointer's travel to the right in handle lengths — a handle's length
    ///   right doubles the size, and its length left is as small as a box goes.
    /// - **A ring** turns the entity about its axis through the centre by the
    ///   angle the pointer has swept round the centre on screen since the
    ///   press, counter-clockwise as seen from the axis's tip — to the nearest
    ///   multiple of the angle step while snapping, measured from the press —
    ///   writing all four `rotation` leaves, and the three `position` leaves
    ///   swung round the centre with it where the component has a position.
    ///
    /// A half extent is never written below [`MIN_HALF_EXTENT`], and a snapped
    /// one that rounds below it is written as the minimum.
    ///
    /// [`None`] when the ray runs along the axis or plane, or the pointer is on
    /// a ring's centre.
    #[must_use]
    pub fn writes(&self, pointer: &Pointer, snap: Option<Snap>) -> Option<Vec<Write>> {
        let field = self.grip().mode().field();
        let set = |axis: Axis, value: f64| Write {
            path: format!("{field}.{}", axis.index()),
            value,
        };
        let half_extent = |value: f64| {
            snap.map_or(value, |snap| snap.scale(value))
                .max(MIN_HALF_EXTENT)
        };
        let start = |axis: Axis| self.start[axis.index()];
        let ray = &pointer.ray;

        Some(match self.hold {
            Hold::Move { .. } | Hold::MovePlane { .. } => self
                .translated(pointer, snap)?
                .into_iter()
                .map(|(axis, value)| set(axis, value))
                .collect(),
            Hold::Scale { axis, unit, grab } => {
                let moved = along(ray, self.origin, unit)? - grab;
                vec![set(axis, half_extent(start(axis) + moved))]
            }
            Hold::ScaleAll { at, span } => {
                let factor = 1.0 + f64::from((pointer.at.x - at.x) / span);
                Axis::ALL
                    .into_iter()
                    .map(|axis| set(axis, half_extent(start(axis) * factor)))
                    .collect()
            }
            Hold::Rotate { axis, at, from } => {
                let angle = swept(from.centre, at, pointer.at)? * from.facing;
                let angle = snap.map_or(angle, |snap| snap.angle(angle));
                self.turned(axis, angle, &from)
            }
        })
    }

    /// Where an arrow or a plane drag puts the point it moves — the press's
    /// [`start`](Self::begin) — axis by axis: each axis the grip moves along,
    /// and the value it lands on, on the absolute grid `snap` describes while
    /// there is one.
    ///
    /// [`None`] for a grip that does not translate, and when the ray runs
    /// along the axis or plane.
    #[must_use]
    pub fn translated(&self, pointer: &Pointer, snap: Option<Snap>) -> Option<Vec<(Axis, f64)>> {
        let position = |value: f64| snap.map_or(value, |snap| snap.grid(value));
        let start = |axis: Axis| self.start[axis.index()];
        let ray = &pointer.ray;
        match self.hold {
            Hold::Move { axis, grab } => {
                let moved = along(ray, self.origin, axis.unit())? - grab;
                Some(vec![(axis, position(start(axis) + moved))])
            }
            Hold::MovePlane { plane, grab } => {
                let moved = on_plane(ray, self.origin, plane.normal())? - grab;
                Some(
                    plane
                        .axes()
                        .into_iter()
                        .map(|axis| (axis, position(start(axis) + moved.dot(axis.unit()))))
                        .collect(),
                )
            }
            Hold::Scale { .. } | Hold::ScaleAll { .. } | Hold::Rotate { .. } => None,
        }
    }

    /// What an arrow or a plane drag of a [`Group`] sets, member by member:
    /// the point the drag moves is the group's pivot, landing where
    /// [`translated`](Self::translated) puts it, and every member moves by
    /// the same delta on each axis the grip moves along.
    ///
    /// **A group of one is its own pivot**: its member is set to the landed
    /// value itself rather than its start plus the delta, which can differ
    /// from it in the last bit — so a lone entity snapped onto the grid lands
    /// on it exactly, as it did before groups.
    ///
    /// [`None`] where [`translated`](Self::translated) is.
    #[must_use]
    pub fn spread<'a>(
        &self,
        group: &'a Group,
        pointer: &Pointer,
        snap: Option<Snap>,
    ) -> Option<Vec<(&'a Member, Write)>> {
        let landed = self.translated(pointer, snap)?;
        let mut writes = Vec::with_capacity(group.members.len() * landed.len());
        for member in &group.members {
            for &(axis, value) in &landed {
                let index = axis.index();
                let value = match group.members.as_slice() {
                    [_] => value,
                    _ => member.start[index] + (value - self.start[index]),
                };
                writes.push((
                    member,
                    Write {
                        path: format!("{POSITION}.{index}"),
                        value,
                    },
                ));
            }
        }
        Some(writes)
    }

    /// The leaves that turn the entity `angle` radians about `axis` through
    /// the centre, from where `from` says it started.
    fn turned(&self, axis: Axis, angle: f64, from: &Turn) -> Vec<Write> {
        let turn = DQuat::from_axis_angle(axis.unit(), angle);
        let rotation = (turn * from.rotation).normalize();
        let mut writes: Vec<Write> = Rotation::LEAVES
            .into_iter()
            .zip(rotation.to_array())
            .map(|(leaf, value)| Write {
                path: format!("{ROTATION}.{leaf}"),
                value,
            })
            .collect();
        if let Some(position) = from.position {
            // The position swings round the centre: unmoved where it is the
            // centre, as a block's is, and carried round where it is not, as
            // a mesh's origin is.
            let swung = self.origin + turn * (position - self.origin);
            writes.extend(Axis::ALL.into_iter().map(|each| Write {
                path: format!("{POSITION}.{}", each.index()),
                value: swung[each.index()],
            }));
        }
        writes
    }
}
