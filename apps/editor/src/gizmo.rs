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

mod drag;

pub use drag::{Drag, SNAP_M, along};

use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::render::Camera;
use crcbl::ui::draw_list::DrawList;

/// How long a handle is on screen, in logical pixels — the UI's, which the
/// window's scale turns into physical ones.
pub const HANDLE_PX: f32 = 90.0;

/// How close to a handle a press has to land to take it, in logical pixels.
pub const HIT_PX: f32 = 8.0;

/// How thick a handle's line is drawn, in logical pixels.
const LINE_PX: f32 = 3.0;

/// Half the side of the square at a handle's tip, in logical pixels.
const TIP_PX: f32 = 6.0;

/// How nearly an axis has to point along the view before its handle is
/// dropped, as the cosine of the angle between them: past it the arrow is a
/// few pixels long and its direction on screen is noise.
const HIDDEN_BEYOND: f32 = 0.985;

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
mod tests;
