//! The transform gizmo: handles over the selection, and the drag that moves
//! or resizes it through one of them.
//!
//! `docs/plan/08-editor.md`'s task 5, decided 2026-09-30 to be **drawn in the
//! viewport pane's screen space**, over the scene's picture, rather than
//! through the world-space debug-draw layer. So a handle is a
//! [`DrawList`] line or square placed from the selection's projected centre, a
//! fixed number of pixels from it whatever the distance — the plan's "constant
//! screen-size scaling" by construction — drawn on top of everything in the
//! pane and hit tested in the same pixels it is drawn in. The debug-draw layer
//! is lines only, depth tested and tonemapped with the scene, and would need an
//! on-top mode and a constant-size transform to do the same.
//!
//! # The two modes
//!
//! [`Mode::Translate`] draws an arrow per axis and a square per plane between
//! two of them; [`Mode::Scale`] draws a box-tipped line per axis and a square
//! at the centre that resizes evenly. **There is no rotate mode**, and the enum
//! has no variant for one: the scene format carries no rotation — a greybox
//! `Block` and breakout's `Brick` are a position and half extents — so a rotate
//! handle would have nothing to write. `docs/backlog.md` says what it takes.
//!
//! # Which field a handle writes
//!
//! Found by name through [`crcbl::reflect`]'s paths, never by naming a
//! component type: translate writes `position.N` ([`POSITION`]), the leaf an
//! arrow key nudges, and scale writes `half_extents.N` ([`HALF_EXTENTS`]).
//! A component without the field has no handles in that mode, which is how an
//! entity with nothing to resize shows no scale handles.
//!
//! # Where a drag puts the entity
//!
//! [`Drag`] says: on the axis line or the plane through the selection's centre
//! as the press found it, by as far as the cursor's ray has moved across that
//! line or plane since the press — so the entity does not jump to the cursor,
//! and a drag that comes back to where it began puts it back where it was.
//! While snapping, the value written lands on the absolute grid ([`Snap`]).
//! Every value is a property set, written through
//! [`crate::Document::apply_in`] so the whole drag is one undo.

mod drag;
mod snap;

pub use drag::{Drag, MIN_HALF_EXTENT, Pointer, Write, along, on_plane};
pub use snap::{GRID_KEY, GRID_M, SCALE_KEY, SCALE_M, Snap};

use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::render::Camera;
use crcbl::ui::draw_list::DrawList;

/// The field translate writes: a component's centre, as `position.N` —
/// [`crcbl::registry::POSITION`], the one spelling of it, which a simulated
/// body's pose is written into as well.
pub const POSITION: &str = crcbl::registry::POSITION;

/// The field scale writes: a component's half size on each axis, as
/// `half_extents.N`.
pub const HALF_EXTENTS: &str = "half_extents";

/// How long a handle is on screen, in logical pixels — the UI's, which the
/// window's scale turns into physical ones.
pub const HANDLE_PX: f32 = 90.0;

/// How close to a handle's line a press has to land to take it, in logical
/// pixels.
pub const HIT_PX: f32 = 8.0;

/// How thick a handle's line is drawn, in logical pixels.
const LINE_PX: f32 = 3.0;

/// Half the side of the square at a handle's tip, in logical pixels.
const TIP_PX: f32 = 6.0;

/// Half the side of a plane handle's square, in logical pixels: a press inside
/// it takes the plane.
const PLANE_PX: f32 = 8.0;

/// How far out a plane handle's square sits along each of its two axes'
/// arrows, as a fraction of [`HANDLE_PX`] — in the corner between them, clear
/// of the centre and of the tips.
const PLANE_AT: f32 = 0.35;

/// Half the side of the uniform-scale square at the centre, in logical pixels.
const CENTRE_PX: f32 = 8.0;

/// How opaque a plane handle's fill is, so the scene shows through it.
const PLANE_FILL_ALPHA: f32 = 0.45;

/// How nearly an axis has to point along the view before its handle is
/// dropped, as the cosine of the angle between them: past it the arrow is a
/// few pixels long and its direction on screen is noise.
const HIDDEN_BEYOND: f32 = 0.985;

/// How nearly edge-on a plane may be seen before its handle is dropped, as the
/// cosine of the angle between the view and the plane's normal: below it the
/// cursor's ray meets the plane so obliquely that a slip of the pointer is a
/// long move, and the crossing runs off to the horizon.
const EDGE_ON_BELOW: f32 = 0.2;

/// What a handle does: which handles a frame draws is a choice between these.
///
/// No rotate: see the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Move the selection along an axis or across a plane — W.
    #[default]
    Translate,
    /// Resize it along an axis or evenly — R.
    Scale,
}

impl Mode {
    /// The field this mode's handles write, three leaves of it.
    #[must_use]
    pub const fn field(self) -> &'static str {
        match self {
            Self::Translate => POSITION,
            Self::Scale => HALF_EXTENTS,
        }
    }
}

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

    /// The `N` of the `position.N` or `half_extents.N` leaf this axis writes.
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

    /// The axis's own colour.
    const fn rgb(self) -> [f32; 3] {
        match self {
            Self::X => [0.90, 0.25, 0.22],
            Self::Y => [0.35, 0.80, 0.30],
            Self::Z => [0.28, 0.48, 0.95],
        }
    }
}

/// One of the three world planes a plane handle moves across, each spanned by
/// two axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// Across X and Y, holding Z.
    Xy,
    /// Across Y and Z, holding X.
    Yz,
    /// Across Z and X, holding Y.
    Zx,
}

impl Plane {
    /// All three.
    pub const ALL: [Self; 3] = [Self::Xy, Self::Yz, Self::Zx];

    /// The two axes a drag across this plane moves along.
    #[must_use]
    pub const fn axes(self) -> [Axis; 2] {
        match self {
            Self::Xy => [Axis::X, Axis::Y],
            Self::Yz => [Axis::Y, Axis::Z],
            Self::Zx => [Axis::Z, Axis::X],
        }
    }

    /// The axis square to it, which a drag across it leaves alone — and whose
    /// colour it is drawn in, the convention every tool with plane handles
    /// shares.
    #[must_use]
    pub const fn normal(self) -> Axis {
        match self {
            Self::Xy => Axis::Z,
            Self::Yz => Axis::X,
            Self::Zx => Axis::Y,
        }
    }
}

/// What a press on a handle takes hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// Translate along one axis.
    Move(Axis),
    /// Translate across one plane.
    MovePlane(Plane),
    /// Resize along one axis.
    Scale(Axis),
    /// Resize along all three in proportion.
    ScaleAll,
}

impl Grip {
    /// The mode whose handles this grip is one of — and so the field it writes.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Self::Move(_) | Self::MovePlane(_) => Mode::Translate,
            Self::Scale(_) | Self::ScaleAll => Mode::Scale,
        }
    }

    /// The colour it is drawn in, brightened while the pointer is over it or
    /// dragging it.
    fn color(self, hot: bool) -> [f32; 4] {
        let rgb = match self {
            Self::Move(axis) | Self::Scale(axis) => axis.rgb(),
            Self::MovePlane(plane) => plane.normal().rgb(),
            Self::ScaleAll => [0.70, 0.70, 0.72],
        };
        // Hot is the colour most of the way to white, so it stays its axis.
        let toward_white = if hot { 0.6 } else { 0.0 };
        let [r, g, b] = rgb.map(|channel| channel + (1.0 - channel) * toward_white);
        [r, g, b, 1.0]
    }
}

/// Where a handle is on screen, in the pane's physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// An axis handle: a line out from the selection's centre.
    Line {
        /// The selection's centre, on screen.
        from: Vec2,
        /// [`HANDLE_PX`], scaled, along the axis's direction on screen.
        to: Vec2,
    },
    /// A plane or centre handle: a square, axis-aligned on screen.
    Square {
        /// Its middle.
        centre: Vec2,
        /// Half its side.
        half: f32,
    },
}

/// A handle as drawn: what it takes hold of, and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    /// What a press on it drags.
    pub grip: Grip,
    /// Where it is on screen.
    pub shape: Shape,
}

/// The `mode` handles for a selection centred on `origin`, seen through
/// `camera` in a pane of `extent` physical pixels at `scale` physical pixels
/// per logical one.
///
/// Empty when the centre is behind the eye. An axis pointing along the view
/// has no handle, since its direction on screen is not a direction, and a
/// plane seen nearly edge-on has none, since a slip of the pointer would be a
/// long move.
#[must_use]
pub fn handles(
    camera: &Camera,
    extent: (u32, u32),
    origin: Vec3,
    scale: f32,
    mode: Mode,
) -> Vec<Handle> {
    let Some(from) = camera.pixel_of(origin, extent) else {
        return Vec::new();
    };
    let view = (origin - camera.eye).normalize_or_zero();
    let directions = Axis::ALL.map(|axis| {
        let unit = narrow(axis.unit());
        if view.dot(unit).abs() > HIDDEN_BEYOND {
            return None;
        }
        // A step along the axis short enough to stay in front of the eye
        // whenever the centre is: a tenth of the distance to it.
        let step = origin.distance(camera.eye).max(f32::MIN_POSITIVE) * 0.1;
        let ahead = camera.pixel_of(origin + unit * step, extent)?;
        (ahead - from).try_normalize()
    });
    let length = HANDLE_PX * scale;

    let mut handles: Vec<Handle> = Axis::ALL
        .into_iter()
        .filter_map(|axis| {
            let direction = directions[axis.index()]?;
            Some(Handle {
                grip: match mode {
                    Mode::Translate => Grip::Move(axis),
                    Mode::Scale => Grip::Scale(axis),
                },
                shape: Shape::Line {
                    from,
                    to: from + direction * length,
                },
            })
        })
        .collect();
    match mode {
        Mode::Translate => handles.extend(Plane::ALL.into_iter().filter_map(|plane| {
            let [a, b] = plane.axes();
            let (a, b) = (directions[a.index()]?, directions[b.index()]?);
            if view.dot(narrow(plane.normal().unit())).abs() < EDGE_ON_BELOW {
                return None;
            }
            Some(Handle {
                grip: Grip::MovePlane(plane),
                shape: Shape::Square {
                    centre: from + (a + b) * length * PLANE_AT,
                    half: PLANE_PX * scale,
                },
            })
        })),
        Mode::Scale => handles.push(Handle {
            grip: Grip::ScaleAll,
            shape: Shape::Square {
                centre: from,
                half: CENTRE_PX * scale,
            },
        }),
    }
    handles
}

/// The handle under `at`, a point in the pane's physical pixels, or [`None`].
///
/// **A square takes the press before a line does**: a plane handle sits in
/// the corner between two arrows and the centre square sits where every
/// line begins, so where a square and a line overlap the press is the square's
/// — it is drawn on top, and it is the smaller target, while a line can still
/// be taken anywhere along the rest of its length. Among squares the one whose
/// middle is nearest wins; among lines, the nearest within [`HIT_PX`], at
/// `scale`.
#[must_use]
pub fn hit(handles: &[Handle], at: Vec2, scale: f32) -> Option<Grip> {
    let square = nearest(handles.iter().filter_map(|handle| match handle.shape {
        Shape::Square { centre, half } => {
            let offset = (at - centre).abs();
            (offset.x <= half && offset.y <= half).then(|| (handle.grip, at.distance(centre)))
        }
        Shape::Line { .. } => None,
    }));
    square.or_else(|| {
        nearest(handles.iter().filter_map(|handle| match handle.shape {
            Shape::Line { from, to } => {
                let distance = distance_to_segment(at, from, to);
                (distance <= HIT_PX * scale).then_some((handle.grip, distance))
            }
            Shape::Square { .. } => None,
        }))
    })
}

/// The grip of the nearest of `found`, each paired with how far it is.
fn nearest(found: impl Iterator<Item = (Grip, f32)>) -> Option<Grip> {
    found
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(grip, _)| grip)
}

/// Draws `handles` into `list`, offset by `offset` — the pane's top-left in
/// window pixels — with `hot` brightened.
///
/// Lines first and squares over them, which is the order [`hit`] takes a press
/// in. A translate arrow ends in a filled square and a scale line in a hollow
/// box, so the two modes read apart at a glance.
pub fn draw(list: &mut DrawList, handles: &[Handle], offset: Vec2, hot: Option<Grip>) {
    for handle in handles {
        let Shape::Line { from, to } = handle.shape else {
            continue;
        };
        let color = handle.grip.color(hot == Some(handle.grip));
        let from = list.to_logical(from + offset);
        let to = list.to_logical(to + offset);
        list.line(from, to, LINE_PX, color);
        let (min, max) = (to - Vec2::splat(TIP_PX), to + Vec2::splat(TIP_PX));
        match handle.grip {
            Grip::Scale(_) => list.rect_outline(min, max, LINE_PX, color),
            _ => list.rect(min, max, color),
        }
    }
    for handle in handles {
        let Shape::Square { centre, half } = handle.shape else {
            continue;
        };
        let color = handle.grip.color(hot == Some(handle.grip));
        let centre = list.to_logical(centre + offset);
        // `half` is physical pixels and the list draws in logical ones.
        let half = Vec2::splat(half / list.scale());
        let (min, max) = (centre - half, centre + half);
        if let Grip::MovePlane(_) = handle.grip {
            let [r, g, b, _] = color;
            list.rect(min, max, [r, g, b, PLANE_FILL_ALPHA]);
            list.rect_outline(min, max, LINE_PX * 0.5, color);
        } else {
            list.rect(min, max, color);
        }
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
