//! The translate gizmo: an arrow per axis over the selection, and the drag that
//! moves it along one.
//!
//! `docs/plan/08-editor.md`'s task 5, decided 2026-09-30 to be **drawn in the
//! viewport pane's screen space**, over the scene's picture, rather than
//! through the world-space debug-draw layer. So a handle is a
//! [`DrawList`] line from the selection's projected centre, a fixed number of
//! pixels long whatever the distance — the plan's "constant screen-size
//! scaling" by construction — drawn on top of everything in the pane and hit
//! tested in the same pixels it is drawn in. The debug-draw layer is lines
//! only, depth tested and tonemapped with the scene, and would need an on-top
//! mode and a constant-size transform to do the same.
//!
//! # Where a drag puts the entity
//!
//! On the axis line through the selection's centre, at the point closest to
//! the ray under the cursor ([`along`]) — measured from where the press
//! grabbed the handle, so the entity does not jump to the cursor. The line is
//! the one the drag began on, not one that follows the entity, so a drag that
//! comes back to where it began puts the entity back where it was.
//!
//! Every value is a property set of `position.N`, the same leaf an arrow key
//! nudges, written through [`crate::Document::apply_in`] so the whole drag is
//! one undo.

use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::render::{Camera, ViewRay};
use crcbl::scene::scn::SceneEntityId;
use crcbl::ui::draw_list::DrawList;

use crate::command::Gesture;

/// How long a handle is on screen, in logical pixels — the UI's, which the
/// window's scale turns into physical ones.
pub const HANDLE_PX: f32 = 90.0;

/// How close to a handle a press has to land to take it, in logical pixels.
pub const HIT_PX: f32 = 8.0;

/// The step a drag moves in while snapping, in metres.
pub const SNAP_M: f64 = 0.25;

/// How thick a handle's line is drawn, in logical pixels.
const LINE_PX: f32 = 3.0;

/// Half the side of the square at a handle's tip, in logical pixels.
const TIP_PX: f32 = 6.0;

/// How nearly an axis has to point along the view before its handle is
/// dropped, as the cosine of the angle between them: past it the arrow is a
/// few pixels long and its direction on screen is noise.
const HIDDEN_BEYOND: f32 = 0.985;

/// Below this, a ray and an axis are parallel and share no closest point.
const PARALLEL_BELOW: f64 = 1e-9;

/// One of the three world axes a handle moves along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// World X, drawn red.
    X,
    /// World Y, drawn green.
    Y,
    /// World Z, drawn blue.
    Z,
}

impl Axis {
    /// All three, in index order.
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The `N` of the `position.N` leaf this axis moves.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// The unit vector along this axis.
    #[must_use]
    pub const fn unit(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }

    /// The colour a handle is drawn in: the axis's own, brightened while the
    /// pointer is over it or dragging it.
    fn color(self, hot: bool) -> [f32; 4] {
        let rgb: [f32; 3] = match self {
            Self::X => [0.90, 0.25, 0.22],
            Self::Y => [0.35, 0.80, 0.30],
            Self::Z => [0.28, 0.48, 0.95],
        };
        // Hot is the colour most of the way to white, so it stays its axis.
        let toward_white = if hot { 0.6 } else { 0.0 };
        let [r, g, b] = rgb.map(|channel| channel + (1.0 - channel) * toward_white);
        [r, g, b, 1.0]
    }
}

/// A handle as drawn: its axis and its two ends, in the pane's physical
/// pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    /// Which axis it moves along.
    pub axis: Axis,
    /// The selection's centre, on screen.
    pub from: Vec2,
    /// [`HANDLE_PX`], scaled, along the axis's direction on screen.
    pub to: Vec2,
}

/// The handles for a selection centred on `origin`, seen through `camera` in a
/// pane of `extent` physical pixels at `scale` physical pixels per logical one.
///
/// Empty when the centre is behind the eye; an axis pointing along the view
/// has no handle, since its direction on screen is not a direction.
#[must_use]
pub fn handles(camera: &Camera, extent: (u32, u32), origin: Vec3, scale: f32) -> Vec<Handle> {
    let Some(from) = camera.pixel_of(origin, extent) else {
        return Vec::new();
    };
    let view = (origin - camera.eye).normalize_or_zero();
    Axis::ALL
        .into_iter()
        .filter_map(|axis| {
            let unit = narrow(axis.unit());
            if view.dot(unit).abs() > HIDDEN_BEYOND {
                return None;
            }
            // A step along the axis short enough to stay in front of the eye
            // whenever the centre is: a tenth of the distance to it.
            let step = origin.distance(camera.eye).max(f32::MIN_POSITIVE) * 0.1;
            let ahead = camera.pixel_of(origin + unit * step, extent)?;
            let direction = (ahead - from).try_normalize()?;
            Some(Handle {
                axis,
                from,
                to: from + direction * HANDLE_PX * scale,
            })
        })
        .collect()
}

/// The handle under `at`, a point in the pane's physical pixels: the nearest
/// one within [`HIT_PX`], at `scale`, of its line, or [`None`].
#[must_use]
pub fn hit(handles: &[Handle], at: Vec2, scale: f32) -> Option<Axis> {
    handles
        .iter()
        .map(|handle| (handle.axis, distance_to_segment(at, handle.from, handle.to)))
        .filter(|(_, distance)| *distance <= HIT_PX * scale)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(axis, _)| axis)
}

/// Draws `handles` into `list`, offset by `offset` — the pane's top-left in
/// window pixels — with `hot` brightened.
pub fn draw(list: &mut DrawList, handles: &[Handle], offset: Vec2, hot: Option<Axis>) {
    for handle in handles {
        let color = handle.axis.color(hot == Some(handle.axis));
        let from = list.to_logical(handle.from + offset);
        let to = list.to_logical(handle.to + offset);
        list.line(from, to, LINE_PX, color);
        list.rect(to - Vec2::splat(TIP_PX), to + Vec2::splat(TIP_PX), color);
    }
}

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

/// How far `at` is from the segment `from`–`to`.
fn distance_to_segment(at: Vec2, from: Vec2, to: Vec2) -> f32 {
    let span = to - from;
    let t = if span.length_squared() > 0.0 {
        ((at - from).dot(span) / span.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    at.distance(from + span * t)
}

/// Simulation space's `f64`, from render space's `f32`.
fn widen(value: Vec3) -> DVec3 {
    DVec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

/// Render space's `f32`, from simulation space's `f64`.
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: DVec3) -> Vec3 {
    Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENT: (u32, u32) = (800, 600);

    /// A camera up and to the side of the origin, looking at it, so no axis
    /// points along the view.
    fn camera() -> Camera {
        Camera {
            eye: Vec3::new(6.0, 5.0, 8.0),
            target: Vec3::ZERO,
            ..Camera::default()
        }
    }

    /// **Each handle is [`HANDLE_PX`] long and points where its axis goes on
    /// screen**: the far end of a world step along the axis projects onto the
    /// handle's own line, beyond its start.
    #[test]
    fn each_handle_points_along_its_axis_on_screen() {
        let camera = camera();
        let handles = handles(&camera, EXTENT, Vec3::ZERO, 1.0);
        assert_eq!(handles.len(), 3);
        for handle in &handles {
            assert!((handle.from.distance(handle.to) - HANDLE_PX).abs() < 1e-3);
            let ahead = camera
                .pixel_of(narrow(handle.axis.unit()) * 2.0, EXTENT)
                .expect("in front");
            let along_handle = (handle.to - handle.from).normalize();
            let along_axis = (ahead - handle.from).normalize();
            assert!(
                along_handle.dot(along_axis) > 0.999,
                "{:?} points {along_handle:?}, its axis goes {along_axis:?}",
                handle.axis,
            );
        }
    }

    /// **The handle is the same size on screen however far away the selection
    /// is** — the plan's constant screen-size scaling — and twice as many
    /// physical pixels at twice the scale.
    #[test]
    fn a_handle_is_the_same_size_near_and_far() {
        let camera = camera();
        for origin in [Vec3::ZERO, Vec3::new(-20.0, -10.0, -40.0)] {
            for scale in [1.0, 2.0] {
                for handle in handles(&camera, EXTENT, origin, scale) {
                    let length = handle.from.distance(handle.to);
                    assert!((length - HANDLE_PX * scale).abs() < 1e-3, "{length}");
                }
            }
        }
    }

    /// An axis pointing straight at the camera has no handle, and a centre
    /// behind the eye has none at all.
    #[test]
    fn an_axis_along_the_view_and_a_centre_behind_the_eye_have_no_handle() {
        let looking_down_z = Camera {
            eye: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            ..Camera::default()
        };
        let axes: Vec<Axis> = handles(&looking_down_z, EXTENT, Vec3::ZERO, 1.0)
            .iter()
            .map(|handle| handle.axis)
            .collect();
        assert_eq!(axes, [Axis::X, Axis::Y]);
        assert!(handles(&looking_down_z, EXTENT, Vec3::new(0.0, 0.0, 20.0), 1.0).is_empty());
    }

    /// A press on a handle's line takes it, one beside the line misses, and
    /// the nearest of two takes it where they meet.
    #[test]
    fn a_press_takes_the_handle_under_it() {
        let handles = handles(&camera(), EXTENT, Vec3::ZERO, 1.0);
        for handle in &handles {
            let middle = (handle.from + handle.to) * 0.5;
            assert_eq!(hit(&handles, middle, 1.0), Some(handle.axis));
            let normal = (handle.to - handle.from).normalize().perp();
            let beside = handle.to + normal * (HIT_PX * 3.0);
            assert_ne!(hit(&handles, beside, 1.0), Some(handle.axis));
        }
        let far = handles[0].from + Vec2::splat(HANDLE_PX * 4.0);
        assert_eq!(hit(&handles, far, 1.0), None);
    }

    /// **The closest point on the axis is the one a ray aimed at it passes
    /// through**, against values worked by hand.
    #[test]
    fn along_finds_the_closest_point_on_the_axis() {
        // A ray straight down onto x = 3 meets the X axis there.
        let down = ViewRay {
            origin: Vec3::new(3.0, 10.0, 0.0),
            direction: Vec3::NEG_Y,
        };
        let t = along(&down, DVec3::ZERO, Axis::X).expect("not parallel");
        assert!((t - 3.0).abs() < 1e-9, "{t}");
        // From an anchor at x = 1 the same point is 2 along.
        let t = along(&down, DVec3::new(1.0, 0.0, 0.0), Axis::X).expect("not parallel");
        assert!((t - 2.0).abs() < 1e-9, "{t}");
        // A skew ray passing above the Z axis at z = -4.
        let skew = ViewRay {
            origin: Vec3::new(-5.0, 2.0, -4.0),
            direction: Vec3::X,
        };
        let t = along(&skew, DVec3::ZERO, Axis::Z).expect("not parallel");
        assert!((t + 4.0).abs() < 1e-9, "{t}");
        // An oblique ray through x = 5 on the axis, so the `b` term is not
        // zero: every ray above is square to its axis and would pass with
        // that term's sign wrong.
        let oblique = ViewRay {
            origin: Vec3::new(1.0, 2.0, 2.0),
            direction: Vec3::new(4.0, -2.0, -2.0).normalize(),
        };
        let t = along(&oblique, DVec3::ZERO, Axis::X).expect("not parallel");
        assert!((t - 5.0).abs() < 1e-5, "{t}");
        // And a ray along the axis has no closest point.
        let parallel = ViewRay {
            origin: Vec3::new(0.0, 1.0, 0.0),
            direction: Vec3::X,
        };
        assert_eq!(along(&parallel, DVec3::ZERO, Axis::X), None);
    }

    /// **A drag moves by as far as the cursor moved along the axis, from where
    /// it grabbed**, and snaps that distance to whole steps.
    #[test]
    fn a_drag_moves_by_the_distance_along_the_axis() {
        let at = |x: f32| ViewRay {
            origin: Vec3::new(x, 10.0, 0.0),
            direction: Vec3::NEG_Y,
        };
        let drag = Drag::begin(
            SceneEntityId(1),
            Axis::X,
            Gesture(1),
            -3.0,
            DVec3::new(-3.0, 0.25, 0.0),
            &at(-2.5),
        )
        .expect("not parallel");
        assert_eq!(
            drag.value(&at(-2.5), false),
            Some(-3.0),
            "no move, no change"
        );
        let moved = drag.value(&at(-1.3), false).expect("not parallel");
        assert!((moved - (-1.8)).abs() < 1e-6, "{moved}");
        let snapped = drag.value(&at(-1.3), true).expect("not parallel");
        assert!((snapped - (-1.75)).abs() < 1e-12, "{snapped}");
    }
}
