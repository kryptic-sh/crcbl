//! The sun's **first bounce** off this room's interior, baked into the
//! irradiance volume [`room`](crate::room::room) hands the engine.
//!
//! ```text
//!  cube-face quadrature ──▶ slab test on the interior box ──▶ which face, where
//!                                                                    │
//!  sun() ──▶ E at the hit ──▶ visibility back through the opening ────┤
//!                                                                    ▼
//!                                       GpuProbe::accumulate ──▶ ProbeGrid
//! ```
//!
//! `docs/plan/18-render-features.md`'s irradiance-probe design asks an
//! application for its own probes, "computed analytically from the room's own
//! dimension constants so that moving a wall moves the probes". This is that
//! computation, and every number in it comes from [`crate::room`].
//!
//! # There is no ray-triangle intersector in this tree, and none is needed
//!
//! The design defers a general bake on a hard prerequisite: a gather needs an
//! intersector and a BVH, and `crcbl-phys` has ray-vs-sphere, ray-vs-AABB and
//! ray-vs-capsule and nothing else. **The room's interior is one axis-aligned
//! box**, so the gather here is a slab test — the same arithmetic a ray-vs-AABB
//! is, written against six named faces because which face a ray leaves through
//! is what decides its colour.
//!
//! # No transcendental functions anywhere, and the golden is why
//!
//! Directions come from a cube-face quadrature rather than from a sphere
//! parameterised in `sin`/`cos`, and the Jacobian is evaluated as a product of
//! square roots rather than through `powf`. libm's transcendentals are **not**
//! bit-identical across platforms, and this bake is host-side Rust whose output
//! lands in `tests/golden/room.png` — a committed image compared across four
//! rasterisers and three operating systems. `sqrt` is correctly rounded by
//! IEEE 754 and carries no such risk.
//!
//! The same rule is why the floor's texture is a named limit below rather than
//! an oversight: decoding it is an sRGB transfer, and that is a `powf`.
//!
//! # What this model leaves out, named rather than omitted
//!
//! - **No occluder inside the room is modelled.** The plinth, the mirror panel,
//!   the metal block, the corner post in the downlight's cone and the lamp are
//!   all invisible to the gather, so a probe standing behind one of them still
//!   receives the wall or the floor it hides — the bounce has no contact
//!   darkening under any object, and the volume is smoother than the room is.
//! - **Only the sun bounces, and only once.** A face out of the sun's reach
//!   emits nothing at all, so there is no second bounce and no ambient bounce.
//! - **The lamp does not bounce**, for the same reason and one more: it moves,
//!   and the volume is baked once.
//! - **A ray that escapes through the window opening contributes exactly zero.**
//!   Not an omission: [`DirectionalLight::ambient`](crcbl::render::DirectionalLight::ambient)
//!   already stands for the sky and `mesh.slang` adds it flat to every fragment,
//!   so counting the sky again through the opening would double-count it.
//! - **The floor's check is not in the bake.** The frame multiplies the floor's
//!   row by the page's two-tone check and this bakes the flat row, so the floor
//!   bounces more light here than it reflects there. Deliberate, and the reason
//!   is the paragraph above: decoding the check to what the sampler produces is
//!   an sRGB transfer.
//! - **A lit patch smaller than one quadrature cell is resolved
//!   stochastically.** The coloured wall's sunlit strip is a narrow band along
//!   its foot, and from the far end of the room it is thinner than a cell — so
//!   how much of it a distant probe catches depends on where the cell centres
//!   happen to land, and moves with the sample count. It is a small share of a
//!   small term there; the probes standing beside that wall, which are the ones
//!   any claim about the bounce reads, see the strip across many cells.

use std::f32::consts::PI;

use crcbl::math::Vec3;
use crcbl::render::ProbeGrid;
use crcbl::shaders::probe::{GpuProbe, ProbeVolume};

use crate::room::{
    BOUNCE_COLOR, FLOOR_COLOR, HALF_DEPTH, HALF_WIDTH, HEIGHT, PLASTER_COLOR, SHELL, WINDOW_HALF,
    WINDOW_HEAD, WINDOW_SILL, sun,
};

// ---------------------------------------------------------------------------
// The room's interior, as one box
// ---------------------------------------------------------------------------

/// The interior box's minimum corner: the floor, the window wall and the back
/// wall.
const INTERIOR_MIN: Vec3 = Vec3::new(-HALF_WIDTH, 0.0, -HALF_DEPTH);

/// Its maximum corner: the ceiling, the coloured wall and the front wall.
const INTERIOR_MAX: Vec3 = Vec3::new(HALF_WIDTH, HEIGHT, HALF_DEPTH);

/// Which of the interior box's six faces a ray leaves through.
///
/// Named rather than carried as an axis and a sign, because the two things a
/// face decides — which material row it wears and which way its inward normal
/// points — are both lookups a reader should be able to check against
/// [`crate::room`]'s own list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    /// `x = -HALF_WIDTH`: the wall with the opening in it.
    Window,
    /// `x = +HALF_WIDTH`: the coloured one.
    Bounce,
    /// `y = 0`.
    Floor,
    /// `y = HEIGHT`.
    Ceiling,
    /// `z = -HALF_DEPTH`.
    Back,
    /// `z = +HALF_DEPTH`, behind the fixed camera.
    Front,
}

impl Face {
    /// The face at `bound` on `axis`, as the slab test finds it.
    fn of(axis: usize, positive: bool) -> Self {
        match (axis, positive) {
            (0, false) => Self::Window,
            (0, true) => Self::Bounce,
            (1, false) => Self::Floor,
            (1, true) => Self::Ceiling,
            (2, false) => Self::Back,
            _ => Self::Front,
        }
    }

    /// Which way this face looks, from inside the room.
    fn inward_normal(self) -> Vec3 {
        match self {
            Self::Window => Vec3::X,
            Self::Bounce => Vec3::NEG_X,
            Self::Floor => Vec3::Y,
            Self::Ceiling => Vec3::NEG_Y,
            Self::Back => Vec3::Z,
            Self::Front => Vec3::NEG_Z,
        }
    }

    /// The base colour of the material row this face actually shades through —
    /// [`crate::room`]'s `OBJECTS` is the list, and this is the same assignment
    /// read as a colour.
    ///
    /// # The floor is baked at its row colour
    ///
    /// [`Face::Floor`] gives `FLOOR_COLOR` flat, and the frame multiplies that
    /// row by the page's check as well. Decoding the check to the linear values
    /// the sampler produces is an sRGB transfer — a `powf` — and the module docs
    /// above refuse one in this file. What it costs is a floor that bounces more
    /// light in the bake than it reflects in the frame, so the floor's share of
    /// the volume is overstated and the coloured wall's, which has no texture, is
    /// not.
    fn albedo(self) -> Vec3 {
        let row = match self {
            Self::Bounce => BOUNCE_COLOR,
            Self::Floor => FLOOR_COLOR,
            _ => PLASTER_COLOR,
        };
        Vec3::new(row[0], row[1], row[2])
    }
}

/// Where a ray leaving `from` along `direction` meets the interior box.
///
/// The slab test: the nearest positive crossing of the three pairs of planes,
/// and which one it was. `None` only for a direction that crosses none of them,
/// which a unit direction from inside the box cannot be —
/// `every_direction_of_the_quadrature_leaves_the_room` is what says so rather
/// than a comment.
fn exit(from: Vec3, direction: Vec3) -> Option<(Vec3, Face)> {
    let mut nearest = f32::INFINITY;
    let mut leaving = None;
    for axis in 0..3 {
        let along = direction[axis];
        if along == 0.0 {
            continue;
        }
        let positive = along > 0.0;
        let bound = if positive {
            INTERIOR_MAX[axis]
        } else {
            INTERIOR_MIN[axis]
        };
        let distance = (bound - from[axis]) / along;
        if distance >= 0.0 && distance < nearest {
            nearest = distance;
            leaving = Some(Face::of(axis, positive));
        }
    }
    leaving.map(|face| (from + direction * nearest, face))
}

/// Whether a point on the window wall's plane is in the opening rather than in
/// the wall around it.
fn inside_the_opening(point: Vec3) -> bool {
    (WINDOW_SILL..=WINDOW_HEAD).contains(&point.y) && point.z.abs() <= WINDOW_HALF
}

/// Whether the ray from the room through both faces of the window's reveal
/// stays in the aperture.
///
/// The wall is a shell: its inner opening is at `-HALF_WIDTH` and its outer
/// opening is [`SHELL`] further along the ray. A ray that only clears the inner
/// plane hits the head or jamb of the reveal before it can reach the sun.
fn passes_through_window(point: Vec3, direction: Vec3) -> bool {
    let inner_x = -HALF_WIDTH;
    let outer_x = inner_x - SHELL;
    let inner_distance = (inner_x - point.x) / direction.x;
    let outer_distance = (outer_x - point.x) / direction.x;
    let inner = point + direction * inner_distance;
    let outer = point + direction * outer_distance;
    inside_the_opening(inner) && inside_the_opening(outer)
}

/// Whether the sun reaches `point`, analytically.
///
/// Marched **back** along the direction the sun comes from: the light arrives
/// only if that ray leaves the room through the window wall and passes through
/// both planes of the opening. A ray that meets the ceiling or an end wall first
/// is blocked by it, which is why the point matters and not only the face — the
/// shaft through the opening covers part of the floor and part of the coloured
/// wall rather than all of either.
fn sun_reaches(point: Vec3) -> bool {
    match exit(point, sun().direction) {
        Some((_, Face::Window)) => passes_through_window(point, sun().direction),
        _ => false,
    }
}

/// The radiance leaving the interior box towards a probe, along a ray that left
/// it at `point` through `face`.
///
/// `albedo · E / π` — one Lambertian bounce, with `E` the sun's direct
/// irradiance at the point: its colour, the clamped cosine against the face's
/// inward normal, and the window's visibility. A face the sun cannot see, and a
/// point of a lit face the shaft does not reach, both emit nothing.
fn outgoing(point: Vec3, face: Face) -> Vec3 {
    // The sky, which `frame.ambient` already carries — see the module docs.
    if face == Face::Window && inside_the_opening(point) {
        return Vec3::ZERO;
    }
    let sun = sun();
    let cosine = face.inward_normal().dot(sun.direction).max(0.0);
    if cosine == 0.0 || !sun_reaches(point) {
        return Vec3::ZERO;
    }
    face.albedo() * sun.color * cosine / PI
}

// ---------------------------------------------------------------------------
// The quadrature
// ---------------------------------------------------------------------------

/// Cells a side on each face of the direction cube.
///
/// Even, so no cell centre lands on a face's axis and every direction has three
/// non-zero components — which is what keeps [`exit`]'s degenerate arm
/// unreachable. Large enough that the shaft's edge on the floor falls inside a
/// cell rather than between two: the whole gather is six of these squared per
/// probe, and the bake runs once at start-up.
const CUBE_FACE_SAMPLES: usize = 32;

/// Directions over the whole sphere with the solid angle each stands for, as
/// `GpuProbe::accumulate` wants them.
///
/// The six faces of a cube, each a `CUBE_FACE_SAMPLES²` grid of cell centres
/// over `[-1, 1]²`. A cell's direction is the point on the face normalised, and
/// its solid angle is the projected-area Jacobian `(2/k)² / (1 + u² + v²)^{3/2}`
/// — evaluated as `s · √s` so that the one `sqrt` the normalisation already
/// needs is the only irrational operation in the whole quadrature. See the
/// module docs for why that matters.
fn cube_quadrature() -> Vec<(Vec3, f32)> {
    #[allow(clippy::cast_precision_loss)]
    let step = 2.0 / CUBE_FACE_SAMPLES as f32;
    let mut samples = Vec::with_capacity(6 * CUBE_FACE_SAMPLES * CUBE_FACE_SAMPLES);
    #[allow(clippy::cast_precision_loss)]
    let centre = |cell: usize| -1.0 + step * (cell as f32 + 0.5);
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            for row in 0..CUBE_FACE_SAMPLES {
                let u = centre(row);
                for column in 0..CUBE_FACE_SAMPLES {
                    let v = centre(column);
                    let mut point = Vec3::ZERO;
                    point[axis] = sign;
                    point[(axis + 1) % 3] = u;
                    point[(axis + 2) % 3] = v;
                    // `|point|² = 1 + u² + v²` by construction, so the same root
                    // normalises the direction and closes the Jacobian.
                    let square = 1.0 + u * u + v * v;
                    let root = square.sqrt();
                    samples.push((point / root, step * step / (square * root)));
                }
            }
        }
    }
    samples
}

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

/// How many probes the volume holds on each axis.
///
/// Denser along `z` than across, because the room is: the shaft of light runs
/// the depth of the floor and the coloured wall's lit strip runs with it, so `z`
/// is the axis the bounce actually varies along. Three in `y` puts a probe below
/// eye height, one at it and one above, which is the least that lets the floor's
/// bounce fade towards the ceiling rather than fill the room evenly.
pub const PROBE_COUNTS: [u32; 3] = [4, 3, 5];

/// How many probes that is — what [`crate::room::CAPACITIES`] reserves and what
/// `ProbeGrid::check` holds the table against.
pub const PROBE_TOTAL: u32 = PROBE_COUNTS[0] * PROBE_COUNTS[1] * PROBE_COUNTS[2];

/// How far apart the probes stand on each axis: the interior box's extent
/// divided by the counts.
///
/// **Not the distance between the outermost probes**, which would be the extent
/// over `count - 1` and would put a probe in each wall. This is the cell size of
/// a `PROBE_COUNTS` partition of the room, and [`grid_origin`] places the first
/// probe half a cell in from the corner — so every probe stands in open air.
fn spacing() -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let counts = Vec3::new(
        PROBE_COUNTS[0] as f32,
        PROBE_COUNTS[1] as f32,
        PROBE_COUNTS[2] as f32,
    );
    (INTERIOR_MAX - INTERIOR_MIN) / counts
}

/// Where probe `(0, 0, 0)` stands: the centre of the corner cell.
fn grid_origin() -> Vec3 {
    INTERIOR_MIN + 0.5 * spacing()
}

/// Where probe `(x, y, z)` stands, in world space.
fn probe_position(cell: [u32; 3]) -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let steps = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
    grid_origin() + spacing() * steps
}

/// One probe: the whole sphere gathered from `position`.
fn bake_with(position: Vec3, quadrature: &[(Vec3, f32)], suppress_bounce_wall: bool) -> GpuProbe {
    let mut probe = GpuProbe::ZERO;
    for &(direction, solid_angle) in quadrature {
        // A direction that leaves the box through no face carries no light.
        // `every_direction_of_the_quadrature_leaves_the_room` is what says this
        // arm is unreachable rather than silently swallowing a miss.
        let Some((point, face)) = exit(position, direction) else {
            continue;
        };
        let radiance = if suppress_bounce_wall && face == Face::Bounce {
            Vec3::ZERO
        } else {
            outgoing(point, face)
        };
        probe.accumulate(direction.to_array(), radiance.to_array(), solid_angle);
    }
    probe
}

/// One probe: the whole sphere gathered from `position`.
fn bake(position: Vec3, quadrature: &[(Vec3, f32)]) -> GpuProbe {
    bake_with(position, quadrature, false)
}

/// The room's irradiance volume, gathered from the room's own dimensions.
///
/// Rows in the `x`-fastest order [`ProbeGrid::probes`] declares, so the table
/// and the volume's counts address the same probe —
/// `every_probe_reads_back_at_its_own_position` is what checks that against
/// [`crcbl::shaders::probe::irradiance_at`] rather than against a second copy of
/// the index arithmetic.
#[must_use]
pub fn probes() -> ProbeGrid {
    let quadrature = cube_quadrature();
    let mut rows = Vec::with_capacity(PROBE_TOTAL as usize);
    for z in 0..PROBE_COUNTS[2] {
        for y in 0..PROBE_COUNTS[1] {
            for x in 0..PROBE_COUNTS[0] {
                rows.push(bake(probe_position([x, y, z]), &quadrature));
            }
        }
    }
    ProbeGrid {
        volume: ProbeVolume {
            origin: grid_origin().to_array(),
            // **The reciprocal**, which is what the field is: the shader
            // multiplies by it where it would otherwise divide. Inverted here and
            // nowhere else, and a volume carrying the spacing itself would light
            // the room from the wrong place.
            inv_spacing: (Vec3::ONE / spacing()).to_array(),
            counts: PROBE_COUNTS,
        },
        probes: rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::shaders::probe::irradiance_at;
    use crcbl::shaders::probe_visibility::ProbeVisibility;

    /// The red-to-blue ratio of a colour, which is what every claim about the
    /// bounce's tint is measured in.
    ///
    /// A ratio between two channels of one value rather than an absolute, for
    /// `apps/lantern/tests/golden.rs`'s reason: an absolute is a second golden
    /// written in numbers, and it moves when the sun's intensity does.
    fn redness(colour: [f32; 3]) -> f32 {
        colour[0] / colour[2]
    }

    /// How much redder the environment beside the coloured wall has to be than
    /// the environment across the room from it.
    ///
    /// **Twenty-five per cent, against a measured twenty-nine with the coloured
    /// wall and twenty-one when only [`Face::Bounce`] is suppressed.** The
    /// threshold lies between those controls, so neutral plaster and floor bounce
    /// cannot satisfy the test. The remaining margin is quadrature error: the
    /// coloured wall's sunlit strip is thinner than one quadrature cell seen from
    /// the far end of the room, so how much of it the *distant* probe resolves
    /// moves with the sample count.
    const BOUNCE_TINT: f32 = 1.25;

    /// **The quadrature's solid angles sum to `4π`**, which is the check a wrong
    /// Jacobian fails and nothing else in this file would.
    ///
    /// The Jacobian scales *every* sample by the same factor, so getting it
    /// wrong scales the whole room's bounce and leaves a picture that is merely
    /// brighter or dimmer than it should be — plausible, and invisible in a ratio
    /// between two blocks of one frame. `4π` is not a number this file chose.
    ///
    /// **The tolerance is the midpoint rule's own error, not slack in the
    /// claim.** A tenth of a per cent, where the measured miss at
    /// [`CUBE_FACE_SAMPLES`] is a fifth of that and where the transcription slip
    /// this is aimed at — losing one of the two roots, so the exponent is `1`
    /// rather than `3/2` — overshoots the sphere by twenty-two per cent.
    #[test]
    fn the_quadrature_covers_the_whole_sphere() {
        let sphere = 4.0 * PI;
        let total: f32 = cube_quadrature().iter().map(|(_, angle)| angle).sum();
        assert!(
            (total - sphere).abs() < sphere * 1e-3,
            "the quadrature's solid angles sum to {total} and a sphere is {sphere}"
        );
        // And it is a partition of the sphere rather than of one side of it: no
        // two samples share a direction, and the six faces contribute alike.
        let samples = cube_quadrature();
        assert_eq!(samples.len(), 6 * CUBE_FACE_SAMPLES * CUBE_FACE_SAMPLES);
        for (direction, solid_angle) in &samples {
            assert!(
                (direction.length() - 1.0).abs() < 1e-5,
                "{direction:?} is not a unit direction"
            );
            assert!(*solid_angle > 0.0, "a sample stands for no solid angle");
        }
    }

    /// **Every direction of the quadrature leaves the room through some face**,
    /// from every probe position — which is what makes [`exit`]'s `None` arm and
    /// [`bake`]'s `continue` unreachable rather than a swallowed miss.
    #[test]
    fn every_direction_of_the_quadrature_leaves_the_room() {
        let quadrature = cube_quadrature();
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let at = probe_position([x, y, z]);
                    for (direction, _) in &quadrature {
                        let (point, _) = exit(at, *direction).unwrap_or_else(|| {
                            panic!("{direction:?} leaves {at:?} through no face at all")
                        });
                        // And it lands on the box rather than past it.
                        for axis in 0..3 {
                            assert!(
                                point[axis] >= INTERIOR_MIN[axis] - 1e-3
                                    && point[axis] <= INTERIOR_MAX[axis] + 1e-3,
                                "{direction:?} from {at:?} leaves the box at {point:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **No probe stands inside a wall**, which is the grid's own arithmetic and
    /// the one mistake here that would light the room from outside it.
    ///
    /// Half a cell in from each face on every axis, so the outermost probes are
    /// clear of the shell and the volume covers the room rather than a box
    /// inside it.
    #[test]
    fn every_probe_stands_in_open_air() {
        let grid = probes();
        assert_eq!(grid.probes.len(), PROBE_TOTAL as usize);
        assert_eq!(grid.volume.total(), PROBE_TOTAL);
        assert_eq!(grid.volume.counts, PROBE_COUNTS);
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let at = probe_position([x, y, z]);
                    for axis in 0..3 {
                        assert!(
                            at[axis] > INTERIOR_MIN[axis] && at[axis] < INTERIOR_MAX[axis],
                            "probe ({x}, {y}, {z}) is at {at:?}, which is not inside the room"
                        );
                    }
                }
            }
        }
        // The spacing is the room over the counts, so moving a wall moves every
        // probe rather than only the volume's bounds.
        assert_eq!(
            spacing() * Vec3::new(4.0, 3.0, 5.0),
            INTERIOR_MAX - INTERIOR_MIN,
            "the cells must partition the room exactly"
        );
    }

    /// **The volume's header addresses the table it is shipped with**: reading
    /// the grid at a probe's own world position gives that probe's own
    /// irradiance.
    ///
    /// Three separate mistakes fail here and nowhere else — an origin at the
    /// room's corner rather than at the first cell's centre, an `inv_spacing`
    /// carrying the spacing instead of its reciprocal, and a table written in
    /// `z`-fastest order. Each of them leaves a lit room that is lit from the
    /// wrong place, which is a picture.
    #[test]
    fn every_probe_reads_back_at_its_own_position() {
        let grid = probes();
        let normal = [0.0, 0.0, 1.0];
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let index = ((z * PROBE_COUNTS[1] + y) * PROBE_COUNTS[0] + x) as usize;
                    let at = probe_position([x, y, z]);
                    assert_eq!(
                        irradiance_at(
                            &grid.volume,
                            &grid.probes,
                            &ProbeVisibility::NONE,
                            at.to_array(),
                            normal,
                        ),
                        grid.probes[index].irradiance(normal),
                        "the grid reads a different row at probe ({x}, {y}, {z})'s own \
                         position {at:?}"
                    );
                }
            }
        }
    }

    /// **The environment beside the coloured wall is redder than the
    /// environment across the room from it**, with no GPU — the claim
    /// `apps/lantern/tests/golden.rs` makes about pixels, made about the values
    /// those pixels are shaded from.
    ///
    /// # What is compared, and why it is the sum rather than the probe alone
    ///
    /// `mesh.slang` shades from `frame.ambient.rgb + probe_irradiance(...)`, and
    /// the ambient is cool where the bounce is warm, so a claim about the probe
    /// on its own is a claim about a term no surface ever sees by itself. This
    /// adds the two the same way the shader does and measures red-to-blue on the
    /// sum. It is also the more conservative reading: the flat ambient dilutes
    /// the difference rather than widening it.
    ///
    /// # Which normal, and what the other ones say
    ///
    /// `+Z`, which is the back wall's — the surface the golden suite reads, and
    /// the one both of these probes actually light. Facing the probes `-X`
    /// instead, the way a surface standing off the coloured wall would look,
    /// distinguishes nothing here: L1's linear band subtracts what arrives from
    /// `+X` when the normal points away from it, and the window-side probe's
    /// whole environment is on that side, so `irradiance` clamps it to exactly
    /// zero and there is no ratio left to take.
    ///
    /// # It fails in both directions
    ///
    /// The same two positions read against a table of [`GpuProbe::ZERO`] rows
    /// are asserted **equal**, which is what a flat ambient gives and what this
    /// claim has to be able to fail as.
    #[test]
    fn the_environment_beside_the_coloured_wall_is_the_red_one() {
        let grid = probes();
        let ambient = sun().ambient;
        // The corner probes at one height and one depth, one at each end of the
        // room: the same row of the grid, mirrored across the room's axis.
        let (height, depth) = (1, 0);
        let beside_the_wall = probe_position([PROBE_COUNTS[0] - 1, height, depth]);
        let beside_the_window = probe_position([0, height, depth]);
        // The back wall's normal, which is the surface the golden reads this on.
        let facing = [0.0, 0.0, 1.0];
        let received = |rows: &[GpuProbe], at: Vec3| {
            let probe = irradiance_at(
                &grid.volume,
                rows,
                &ProbeVisibility::NONE,
                at.to_array(),
                facing,
            );
            redness((Vec3::from(probe) + ambient).to_array())
        };

        let tinted = received(&grid.probes, beside_the_wall);
        let plain = received(&grid.probes, beside_the_window);
        assert!(
            tinted > plain * BOUNCE_TINT,
            "the environment beside the coloured wall has a red-to-blue of {tinted:.3} and \
             the environment across the room from it {plain:.3} — the one saturated row in \
             this room is not reaching the gather"
        );

        // The neutral plaster and floor are still gathered: this control removes
        // only Face::Bounce, so it can tell the coloured wall's tint from a
        // spatial colour gradient the neutral bounce happens to make.
        let quadrature = cube_quadrature();
        let mut without_coloured_wall = Vec::with_capacity(PROBE_TOTAL as usize);
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    without_coloured_wall.push(bake_with(
                        probe_position([x, y, z]),
                        &quadrature,
                        true,
                    ));
                }
            }
        }
        let neutral_tinted = received(&without_coloured_wall, beside_the_wall);
        let neutral_plain = received(&without_coloured_wall, beside_the_window);
        assert!(
            neutral_tinted <= neutral_plain * BOUNCE_TINT,
            "without Face::Bounce, the red-to-blue ratios are {neutral_tinted:.3} beside and \
             {neutral_plain:.3} across the room; their ratio still clears {BOUNCE_TINT:.3}"
        );

        // And with the rows zeroed, both points read the flat ambient and the two
        // are the same number — so the assertion above is about the bounce and
        // not about where the two points stand.
        let zeroed = vec![GpuProbe::ZERO; PROBE_TOTAL as usize];
        assert_eq!(
            received(&zeroed, beside_the_wall),
            received(&zeroed, beside_the_window),
            "a volume of zeroed rows must leave the two points identical"
        );
    }

    #[test]
    fn the_window_reveal_blocks_rays_that_only_clear_its_inner_opening() {
        let direction = sun().direction;
        // This is the review's head-edge repro: it crosses the inner opening at
        // y = 2.38, then the outer plane above WINDOW_HEAD.
        let head = Vec3::new(3.0, 0.38, -2.067);
        let (inner_head, face) = exit(head, direction).expect("the head ray leaves the room");
        assert_eq!(face, Face::Window);
        assert!(inside_the_opening(inner_head));
        assert!(!passes_through_window(head, direction));
        assert!(!sun_reaches(head));

        // Start from an inner crossing clear of the jamb, then march back into
        // the room. The ray reaches the outer plane past WINDOW_HALF.
        let inner_jamb = Vec3::new(-HALF_WIDTH, 2.2, WINDOW_HALF - 0.02);
        let jamb = inner_jamb - direction * 6.0;
        let (crossing, face) = exit(jamb, direction).expect("the jamb ray leaves the room");
        assert_eq!(face, Face::Window);
        assert!(inside_the_opening(crossing));
        assert!(!passes_through_window(jamb, direction));
        assert!(!sun_reaches(jamb));
    }

    /// The window is a hole rather than a mirror: a ray leaving through the
    /// opening carries nothing, and one leaving through the wall around it
    /// carries nothing either, because that wall's inner face never sees the sun.
    ///
    /// The pair matters because the two zeroes have different causes — the first
    /// is the sky `frame.ambient` already carries and the second is a face
    /// turned away from the light — and a bake that lost the opening test would
    /// still pass the second half.
    #[test]
    fn the_window_wall_emits_nothing_through_the_hole_or_around_it() {
        let middle = Vec3::new(
            -HALF_WIDTH,
            0.5 * (WINDOW_SILL + WINDOW_HEAD),
            0.5 * WINDOW_HALF,
        );
        assert!(inside_the_opening(middle));
        assert_eq!(outgoing(middle, Face::Window), Vec3::ZERO);

        let pier = Vec3::new(-HALF_WIDTH, 0.5 * WINDOW_SILL, 0.0);
        assert!(!inside_the_opening(pier));
        assert_eq!(outgoing(pier, Face::Window), Vec3::ZERO);

        // And the shaft is real: the floor inside it emits and the floor outside
        // it does not, which is the whole of what `sun_reaches` decides.
        assert!(
            sun_reaches(crate::room::SUNLIT_FLOOR),
            "the floor point the golden suite calls sunlit is not"
        );
        assert!(
            !sun_reaches(crate::room::SHADED_FLOOR),
            "the floor point the golden suite calls shaded is in the shaft"
        );
        assert!(outgoing(crate::room::SUNLIT_FLOOR, Face::Floor).length() > 0.0);
        assert_eq!(outgoing(crate::room::SHADED_FLOOR, Face::Floor), Vec3::ZERO);
    }
}
