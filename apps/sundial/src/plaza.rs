//! The plaza, as data an application hands the engine.
//!
//! ```text
//!  MeshBuilder ──▶ build_meshlets ──▶ Geometry::Flat ──┐
//!  GpuMaterial rows ───────────────────────────────────┼─▶ SceneDesc ──▶ with_scene
//!  PageDesc::opaque_white ─────────────────────────────┘
//!  place() ──▶ add_instance ×N
//! ```
//!
//! Nothing here names a device and nothing here is the engine's content, on
//! `apps/alcove/src/court.rs`' terms exactly: the meshes are baked from literals
//! by this module's own quad builder, the two material rows are this sample's,
//! and the page is [`PageDesc::opaque_white`] with no layer pushed onto it.
//! Godot-style axes throughout — `+Y` up, `-Z` forward.
//!
//! # Every shape here is here because a named shadow artefact needs somewhere to
//! appear
//!
//! `docs/plan/sample/18-sundial.md`'s "Proves": "the scene is built so that
//! acne, peter-panning and a cascade seam each have a surface that would show
//! them". Each of those is one thing below, and the constants that place it say
//! which claim it carries:
//!
//! * **The pavement** — a large flat plane at a grazing sun, which is where
//!   shadow acne lives. Twenty-one metres deep, so it runs past the sun's own
//!   [`crcbl::render::shadow::DISTANCE`] and the cascades' far end is in frame.
//! * **The colonnade** — [`COLONNADE_COUNT`] columns marching away from the
//!   camera, and the repeated caster the cascade seam crosses. The nearest
//!   column's foot is inside the first cascade and the furthest one's is outside
//!   it, and [`SEAM_COLUMN`]'s shadow crosses the boundary once, in frame. The
//!   module's own `the_colonnade_straddles_the_cascade_split` measures all of
//!   that against [`crcbl::render::Cascades`]' own split rather than against a
//!   number written here.
//! * **The plinth** — a block resting *on* the pavement, so its contact point is
//!   checkable. [`PLINTH_CONTACT`] is pavement five centimetres from the block's
//!   near face and inside the block's own shadow; peter-panning is exactly the
//!   artefact that lifts a shadow off its caster and lights that point.
//! * **The counters** — [`COUNTERS`] cubes hanging at graded heights over one
//!   strip of pavement, which is what makes contact hardening a thing a picture
//!   can show: a PCSS penumbra widens with the gap between caster and receiver
//!   and a fixed-width filter's does not.
//! * **The parapet** — a low wall closing the far end, so the plane has an edge
//!   rather than running to the horizon. Low deliberately: a tall backdrop at a
//!   grazing sun throws a shadow the length of the plaza and every reading below
//!   would be taken inside it.
//!
//! # The counters hang from nothing, and that is deliberate
//!
//! A post under one would stand in the very shadow the block's penumbra is
//! measured in — its own shadow starts at the post's foot and runs the same way
//! the block's does — so the reading would be of two casters at two heights
//! rather than of one. The cubes are unsupported for the same reason a physics
//! fixture drops a ball in a vacuum.
//!
//! # The lights have no fittings
//!
//! [`lights`] places two point lights and a spot and no geometry for any of
//! them. A housing modelled around a punctual light is geometry inside that
//! light's own shadow map, which occludes the light it was drawn to represent —
//! `apps/lantern`'s lamp is unmodelled for the same reason.
//!
//! [`PageDesc::opaque_white`]: crcbl::render::scene::PageDesc::opaque_white

use std::borrow::Cow;

use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    Camera, Capacities, ForwardRenderer, Geometry, InstanceDesc, InstancePoolError, Light,
    MeshDesc, PageDesc, PointLight, Projection, SceneDesc, SpotLight,
};
use crcbl::shaders::mesh::{self, GpuMaterial, MeshVertex};
use crcbl::shaders::vertex::UvRange;

// ---------------------------------------------------------------------------
// The pavement
// ---------------------------------------------------------------------------

/// Half the pavement's width, in metres.
pub const HALF_WIDTH: f32 = 9.0;

/// How far the pavement reaches away from the camera, in metres.
///
/// Past [`crcbl::render::shadow::DISTANCE`]'s half, so the far cascade has
/// surface in it and a reader can see where the sun's shadows stop being drawn
/// at all — which is the honest failure a shadow map has and the one a fixture
/// should show rather than hide.
pub const FAR_EDGE: f32 = -12.0;

/// How far it reaches behind the camera.
///
/// Set by the longest shadow in the scene rather than by the view: the sun's
/// shadows run towards `+z`, and at the bottom of [`crate::sun`]'s arc a column
/// throws one `COLUMN_HEIGHT / tan(elevation)` long from a foot already at
/// `-4.2`. `the_grazing_sun_keeps_its_shadows_on_the_pavement` measures that
/// against every tick of the clock. A shadow that ran off the plane's near lip
/// would end in a hard straight line no light in the scene accounts for, behind
/// the camera in the fixture pose but square in front of the free one.
pub const NEAR_EDGE: f32 = 11.5;

/// How thick the pavement slab is.
///
/// A slab rather than a single quad, for `apps/alcove/src/court.rs`' reason: the
/// sun has to *see* a surface, and a plane with no thickness casts a shadow only
/// from one side.
const PAVEMENT_THICKNESS: f32 = 0.25;

// ---------------------------------------------------------------------------
// The colonnade
// ---------------------------------------------------------------------------

/// Where the colonnade stands in `x`.
pub const COLONNADE_X: f32 = -1.55;

/// Half the side of one column, in metres.
const COLUMN_HALF: f32 = 0.17;

/// How tall a column stands.
pub const COLUMN_HEIGHT: f32 = 2.8;

/// Where the nearest column stands in `z`.
///
/// Just inside the bottom of the fixed camera's frame, and — this is the point —
/// **inside the first cascade**, where the far end of the row is outside it. The
/// split is [`crcbl::render::Cascades`]' own and moves with the camera's near
/// plane, so `the_colonnade_straddles_the_cascade_split` measures it rather than
/// trusting this sentence.
pub const COLONNADE_NEAR_Z: f32 = 3.4;

/// How far apart consecutive columns stand, in metres.
pub const COLONNADE_SPACING: f32 = 1.9;

/// How many columns the colonnade has.
///
/// Five, which is what reaches from the bottom of the frame to the parapet at
/// [`COLONNADE_SPACING`]. A repeated caster rather than one: a single column's
/// shadow crossing a cascade boundary is an accident of where it stands, and a
/// row of them crossing it is the boundary.
pub const COLONNADE_COUNT: usize = 5;

/// Which column's shadow the cascade seam runs across in frame.
///
/// The nearest two columns stand inside the first cascade and their shadows
/// stay in it; this one's foot is outside and its shadow reaches in, so the
/// boundary crosses it once, near the foot, where the fixed camera can see it.
/// `the_colonnade_straddles_the_cascade_split` measures every part of that
/// against [`crcbl::render::Cascades`]' own split rather than against this
/// paragraph.
pub const SEAM_COLUMN: usize = 2;

/// Where column `index` stands, on the pavement.
#[must_use]
pub fn column_foot(index: usize) -> Vec3 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the colonnade is five columns long"
    )]
    let step = index as f32;
    Vec3::new(
        COLONNADE_X,
        0.0,
        COLONNADE_SPACING.mul_add(-step, COLONNADE_NEAR_Z),
    )
}

// ---------------------------------------------------------------------------
// The plinth, and the contact claim
// ---------------------------------------------------------------------------

/// The plinth's corner nearest `-x`, `-y`, `-z`.
const PLINTH_MIN: Vec3 = Vec3::new(-0.80, 0.0, 1.55);

/// Its corner nearest `+x`, `+y`, `+z`.
const PLINTH_MAX: Vec3 = Vec3::new(0.40, 0.70, 2.75);

/// How far out from the plinth's near face [`PLINTH_CONTACT`] is read.
///
/// Five centimetres — under a fifth of the block's height, so the point is
/// inside the block's shadow at every sun the clock reaches, and far enough out
/// that a block of pixels around it is pavement rather than the block's own
/// side.
const CONTACT_CLEARANCE: f32 = 0.05;

/// **The contact point**: pavement a hand's breadth from the plinth's near face,
/// in the plinth's own shadow.
///
/// `docs/plan/sample/18-sundial.md`'s peter-panning half. A shadow biased too far
/// towards its light detaches from the object casting it, and the first thing
/// that lights is the pavement immediately around the contact — so a reading here
/// that is *not* dark is the artefact, and it is one no golden of the whole frame
/// would name.
pub const PLINTH_CONTACT: Vec3 = Vec3::new(
    (PLINTH_MIN.x + PLINTH_MAX.x) * 0.5,
    0.0,
    PLINTH_MAX.z + CONTACT_CLEARANCE,
);

// The point is derived from the plinth's own corners, so what these hold is that
// an edit to *those* leaves it a contact reading: on the pavement, past the near
// face rather than under the block, close enough to that face to be a reading of
// the contact rather than of the shadow's middle, and across the face rather than
// off its end. Compile-time, because every term is a constant — a test could only
// re-check what the compiler has already settled.
const _: () = {
    assert!(PLINTH_CONTACT.y == 0.0, "the contact is off the ground");
    assert!(
        PLINTH_CONTACT.z > PLINTH_MAX.z,
        "the contact is under the plinth rather than beside it"
    );
    assert!(
        PLINTH_CONTACT.z - PLINTH_MAX.z < 0.25,
        "the contact is too far from the plinth's face to be a contact reading"
    );
    assert!(
        PLINTH_CONTACT.x > PLINTH_MIN.x && PLINTH_CONTACT.x < PLINTH_MAX.x,
        "the contact is off the end of the plinth's face"
    );
};

/// **The control for every darkness claim**: pavement in the open, out of reach
/// of anything.
///
/// In full sun at every tick of the clock and outside all three punctual lights'
/// radii, so a run in which this is not bright is a run in which the whole frame
/// moved rather than one in which a shadow landed.
/// `nothing_reaches_the_open_pavement` is what holds it clear of the lights, and
/// `the_open_pavement_is_in_sun_at_every_tick` of the sun that nothing casts a
/// shadow onto it.
pub const OPEN_PAVEMENT: Vec3 = Vec3::new(1.5, 0.0, -4.0);

// ---------------------------------------------------------------------------
// The counters: the penumbra ladder
// ---------------------------------------------------------------------------

/// Half the side of one counter, in metres.
///
/// A cube 0.44 m across. Large enough that its shadow keeps a fully dark core at
/// the widest penumbra the ladder reaches — a caster narrower than its own
/// penumbra has no umbra left to measure the penumbra against.
pub const COUNTER_HALF: f32 = 0.22;

/// Where the counters hang in `z`.
pub const COUNTER_Z: f32 = -2.4;

/// Each counter: where it stands in `x`, and the clear height of its **underside**
/// above the pavement.
///
/// The gap and not the centre, because the gap is what the claim is about: PCSS
/// sizes its filter from the distance between the blocker it found and the
/// receiver, so a penumbra that does not widen down this list is a filter of
/// fixed width wearing PCSS's name.
///
/// Three rather than two: two readings can be ordered by chance, and the middle
/// one is what makes the claim monotone.
pub const COUNTERS: [(f32, f32); 3] = [(1.60, 0.5), (2.85, 1.7), (4.10, 3.3)];

/// Where counter `index`'s centre hangs.
#[must_use]
pub fn counter_centre(index: usize) -> Vec3 {
    let (x, gap) = COUNTERS[index];
    Vec3::new(x, gap + COUNTER_HALF, COUNTER_Z)
}

/// Where counter `index`'s shadow lands on the pavement under `sky`.
///
/// The caster's centre followed along the sun to `y = 0`. Derived rather than
/// written down, so a nudge to the clock or to a counter moves the block a claim
/// is read in rather than leaving it beside the shadow it meant to measure.
#[must_use]
pub fn counter_shadow(index: usize, sky: crate::sun::Sky) -> Vec3 {
    let centre = counter_centre(index);
    let towards = sky.towards();
    // `towards.y` is strictly positive for every tick — `sun.rs`'s
    // `the_sun_stays_above_the_horizon_and_sweeps_its_whole_range` is what says
    // so — and the sun is above the counter, so this is a step forward along the
    // ray rather than backwards through it.
    let along = centre.y / towards.y;
    Vec3::new(
        centre.x - towards.x * along,
        0.0,
        centre.z - towards.z * along,
    )
}

// ---------------------------------------------------------------------------
// The parapet
// ---------------------------------------------------------------------------

/// Half the parapet's width.
const PARAPET_HALF_WIDTH: f32 = 7.0;

/// Where the parapet stands, and how thick it is.
const PARAPET_Z: (f32, f32) = (FAR_EDGE + 0.6, FAR_EDGE + 1.0);

/// How tall the parapet stands.
///
/// Low, and that is the whole of its design: at the sweep's most grazing sun a
/// caster throws [`crate::sun::Sky::shadow_reach`] metres of shadow per metre of
/// height, so a backdrop tall enough to close the frame would lay a shadow the
/// length of the plaza across every reading below it.
const PARAPET_HEIGHT: f32 = 0.9;

// ---------------------------------------------------------------------------
// The lights
// ---------------------------------------------------------------------------

/// How far a lamp reaches, in world units.
///
/// **A hard bound**: `PointLight::radius` is where the shading window reaches
/// zero and what the clustering pass culls against. Short enough that neither
/// lamp reaches [`PLINTH_CONTACT`], [`OPEN_PAVEMENT`] or any counter's shadow —
/// `nothing_reaches_the_open_pavement` is what holds that, so every reading in
/// the golden suite is the sun's and the ambient term's alone.
const LAMP_REACH: f32 = 3.4;

/// How bright a lamp is, before its colour.
const LAMP_INTENSITY: f32 = 3.2;

/// Where the near lamp hangs: over the colonnade, a little above the columns.
const NEAR_LAMP_AT: Vec3 = Vec3::new(COLONNADE_X, COLUMN_HEIGHT + 0.25, -0.4);

/// Where the far lamp hangs, at the other end of the colonnade.
const FAR_LAMP_AT: Vec3 = Vec3::new(COLONNADE_X, COLUMN_HEIGHT + 0.25, -4.2);

/// Where the colonnade's downlight hangs.
const SPOT_AT: Vec3 = Vec3::new(-0.9, 3.4, 0.4);

/// The floor point its **axis** lands on.
///
/// Across the colonnade rather than straight down, so the cone rakes the middle
/// column and puts a punctual shadow on the pavement — a downlight aimed at its
/// own mounting point lights a circle with nothing in it.
const SPOT_POOL: Vec3 = Vec3::new(-2.3, 0.0, 0.4);

/// How far the downlight reaches, on [`LAMP_REACH`]'s terms exactly.
const SPOT_REACH: f32 = 3.6;

/// Half-angle of the cone's bright core, in radians.
const SPOT_INNER_ANGLE: f32 = 13.0 * core::f32::consts::PI / 180.0;

/// Half-angle at which the cone closes, in radians.
///
/// Twice [`SPOT_INNER_ANGLE`], so half the cone's width is penumbra: the ramp
/// between the two is the one thing a cone written as a boolean cannot produce.
const SPOT_OUTER_ANGLE: f32 = 26.0 * core::f32::consts::PI / 180.0;

/// How bright the downlight is, before its colour.
const SPOT_INTENSITY: f32 = 4.0;

/// Every light in the plaza that is not the sun.
///
/// **Two point lights and a spot, which is exactly what the atlas's light region
/// holds beside a second cube.** `crcbl::render::shadow`'s `LIGHT_TILES` is
/// fourteen and a point light's map is `POINT_FACES` of them, so two cubes and a
/// cone is thirteen tiles and three of the four `LIGHT_SLOTS` — one short of both
/// budgets, which is what makes this a scene where every light is shadowed rather
/// than one where the budget is the subject.
/// `every_light_in_the_plaza_is_given_a_run_of_tiles` reads that back off
/// `crcbl::render::shadow::Selection` itself, with no GPU in the room.
///
/// The sun is not in it: [`ForwardRenderer::begin_frame`] takes one of its own.
///
/// [`ForwardRenderer::begin_frame`]: crcbl::render::ForwardRenderer::begin_frame
#[must_use]
pub fn lights() -> [Light; 3] {
    [
        Light::Point(PointLight {
            position: NEAR_LAMP_AT,
            radius: LAMP_REACH,
            // Warm against the sun's near-white, so which light lit a surface is
            // legible in the picture rather than a brightness difference.
            color: Vec3::new(1.0, 0.66, 0.34) * LAMP_INTENSITY,
            fill: false,
        }),
        Light::Point(PointLight {
            position: FAR_LAMP_AT,
            radius: LAMP_REACH,
            color: Vec3::new(1.0, 0.66, 0.34) * LAMP_INTENSITY,
            fill: false,
        }),
        Light::Spot(SpotLight {
            position: SPOT_AT,
            radius: SPOT_REACH,
            // Cool, for the lamps' reason.
            color: Vec3::new(0.70, 0.81, 1.0) * SPOT_INTENSITY,
            direction: (SPOT_POOL - SPOT_AT).normalize(),
            inner_angle: SPOT_INNER_ANGLE,
            outer_angle: SPOT_OUTER_ANGLE,
            fill: false,
        }),
    ]
}

// ---------------------------------------------------------------------------
// The material rows
// ---------------------------------------------------------------------------

/// The pavement's, the colonnade's and the parapet's row.
///
/// **Row 0**, which is what [`mesh::GpuInstance::default`] names, so it is the
/// row an object placed without a material id shades through.
pub const GROUND_MATERIAL: usize = 0;

/// The plinth's and the counters' row.
pub const OBJECT_MATERIAL: usize = 1;

/// How rough every surface here is.
///
/// One value for both rows, and high: a tight highlight would put a specular
/// gradient across exactly the pavement a shadow edge is measured on.
const ROUGHNESS: f32 = 0.9;

/// [`GROUND_MATERIAL`]'s base colour.
///
/// Light, because a shadow is read against what surrounds it, and not `1.0`:
/// a surface that reflected every photon it received clips the tonemap's top end
/// across the whole plaza.
pub(crate) const GROUND_COLOR: [f32; 4] = [0.80, 0.79, 0.76, 1.0];

/// [`OBJECT_MATERIAL`]'s base colour.
///
/// Darker than the ground by enough to tell a caster from the surface it stands
/// on, and near-uniform otherwise: an albedo step across a shadow edge is a
/// second factor in a reading that is meant to be about the filter.
pub(crate) const OBJECT_COLOR: [f32; 4] = [0.62, 0.60, 0.57, 1.0];

/// The page's extent, in texels a side.
///
/// One: every material row names [`PageDesc::UNTEXTURED_LAYER`], so the page
/// carries a single white texel and the frame has no pattern in it anywhere —
/// texture detail is what hides a shadow artefact.
///
/// [`PageDesc::UNTEXTURED_LAYER`]: crcbl::render::scene::PageDesc::UNTEXTURED_LAYER
const PAGE_EXTENT: u32 = 1;

// ---------------------------------------------------------------------------
// The meshes
// ---------------------------------------------------------------------------

/// Where each resident mesh is in [`SceneDesc::meshes`], and therefore what an
/// [`InstanceDesc::mesh`] naming it carries.
pub const PAVEMENT_MESH: usize = 0;
/// The parapet closing the far end.
pub const PARAPET_MESH: usize = 1;
/// The colonnade, all [`COLONNADE_COUNT`] columns in one mesh.
pub const COLONNADE_MESH: usize = 2;
/// The plinth resting on the pavement.
pub const PLINTH_MESH: usize = 3;
/// The three counters.
pub const COUNTERS_MESH: usize = 4;

/// Which way a quad faces along the axis its plane is perpendicular to.
///
/// The engine culls back faces and calls counter-clockwise front, so a quad's
/// corner order decides whether it is a wall or a hole. Naming the direction
/// rather than writing four coordinates per quad keeps that decision in one
/// place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Facing {
    /// The face's normal points along `+axis`.
    Positive,
    /// The face's normal points along `-axis`.
    Negative,
}

/// A triangle list under construction, and the positions `build_meshlets` needs
/// beside it.
#[derive(Debug, Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    vertices: Vec<RawVertex>,
    indices: Vec<u32>,
}

/// One vertex on the way to a [`MeshVertex`].
#[derive(Clone, Copy, Debug)]
struct RawVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl MeshBuilder {
    /// Appends one quad, given its corners already in counter-clockwise order
    /// seen from `normal`'s side, and two triangles over them.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let base = u32::try_from(self.vertices.len())
            .unwrap_or_else(|_| unreachable!("a plaza of a few hundred vertices"));
        for corner in corners {
            self.positions.push([corner.x, corner.y, corner.z]);
            self.vertices.push(RawVertex {
                position: [corner.x, corner.y, corner.z],
                normal: [normal.x, normal.y, normal.z],
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A quad in the plane `x`, spanning `y` and `z`.
    fn quad_x(&mut self, x: f32, facing: Facing, y: (f32, f32), z: (f32, f32)) {
        let at = |y: f32, z: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(y.0, z.1), at(y.0, z.0), at(y.1, z.0), at(y.1, z.1)],
                Vec3::X,
            ),
            Facing::Negative => self.quad(
                [at(y.0, z.0), at(y.0, z.1), at(y.1, z.1), at(y.1, z.0)],
                Vec3::NEG_X,
            ),
        }
    }

    /// A quad in the plane `y`, spanning `x` and `z`.
    fn quad_y(&mut self, y: f32, facing: Facing, x: (f32, f32), z: (f32, f32)) {
        let at = |x: f32, z: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(x.0, z.1), at(x.1, z.1), at(x.1, z.0), at(x.0, z.0)],
                Vec3::Y,
            ),
            Facing::Negative => self.quad(
                [at(x.0, z.0), at(x.1, z.0), at(x.1, z.1), at(x.0, z.1)],
                Vec3::NEG_Y,
            ),
        }
    }

    /// A quad in the plane `z`, spanning `x` and `y`.
    fn quad_z(&mut self, z: f32, facing: Facing, x: (f32, f32), y: (f32, f32)) {
        let at = |x: f32, y: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(x.0, y.0), at(x.1, y.0), at(x.1, y.1), at(x.0, y.1)],
                Vec3::Z,
            ),
            Facing::Negative => self.quad(
                [at(x.1, y.0), at(x.0, y.0), at(x.0, y.1), at(x.1, y.1)],
                Vec3::NEG_Z,
            ),
        }
    }

    /// A closed box between `min` and `max`, every face pointing **out**.
    fn box_outward(&mut self, min: Vec3, max: Vec3) {
        let (x, y, z) = ((min.x, max.x), (min.y, max.y), (min.z, max.z));
        self.quad_x(max.x, Facing::Positive, y, z);
        self.quad_x(min.x, Facing::Negative, y, z);
        self.quad_y(max.y, Facing::Positive, x, z);
        self.quad_y(min.y, Facing::Negative, x, z);
        self.quad_z(max.z, Facing::Positive, x, y);
        self.quad_z(min.z, Facing::Negative, x, y);
    }

    /// A box of `half` extents about `centre`, axis-aligned.
    fn cube(&mut self, centre: Vec3, half: Vec3) {
        self.box_outward(centre - half, centre + half);
    }

    /// The mesh this builder describes, clustered.
    ///
    /// # Panics
    ///
    /// If [`crcbl::scene::build_meshlets`] refuses the triangle list, which for
    /// literals written in this file would be a mistake in this file rather than
    /// a condition a run can be in.
    fn finish(self, label: &'static str) -> MeshDesc<'static> {
        let clusters = crcbl::scene::build_meshlets(&self.positions, &self.indices)
            .unwrap_or_else(|why| panic!("{label} is a whole number of triangles: {why}"))
            .into_clusters();
        // Every surface samples the page's one white texel, so every vertex
        // carries the same texture coordinate and the range is degenerate on
        // purpose — see `PAGE_EXTENT`.
        let uv_range = UvRange::from_uvs(&[[0.0, 0.0]]);
        let vertices: Vec<MeshVertex> = self
            .vertices
            .iter()
            .map(|vertex| {
                MeshVertex::from_normal(
                    vertex.position,
                    vertex.normal,
                    // White, so the **material row** is the whole of what
                    // colours a surface.
                    [1.0, 1.0, 1.0, 1.0],
                    [0.0, 0.0],
                    &uv_range,
                )
            })
            .collect();
        MeshDesc {
            label: Cow::Borrowed(label),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(mesh::vertex_bytes(&vertices)),
                uv_range,
                indices: Cow::Owned(self.indices),
                clusters,
                // No `MESH_AUTHORED_TANGENTS`: nothing here samples a normal map,
                // so the plaza has no authored tangent to claim.
                flags: 0,
            },
        }
    }
}

/// One mesh built by `fill`.
fn mesh_of(label: &'static str, fill: impl FnOnce(&mut MeshBuilder)) -> MeshDesc<'static> {
    let mut builder = MeshBuilder::default();
    fill(&mut builder);
    builder.finish(label)
}

/// The colonnade: [`COLONNADE_COUNT`] columns marching away from the camera.
fn colonnade(builder: &mut MeshBuilder) {
    for index in 0..COLONNADE_COUNT {
        let (min, max) = column_box(index);
        builder.box_outward(min, max);
    }
}

/// The three counters, hanging.
fn counters(builder: &mut MeshBuilder) {
    for index in 0..COUNTERS.len() {
        builder.cube(counter_centre(index), Vec3::splat(COUNTER_HALF));
    }
}

// ---------------------------------------------------------------------------
// The description
// ---------------------------------------------------------------------------

/// Everything the plaza makes resident: every mesh, both material rows and the
/// one-texel page.
///
/// The mesh order is [`PAVEMENT_MESH`] through [`COUNTERS_MESH`] and the row
/// order is [`GROUND_MATERIAL`] then [`OBJECT_MATERIAL`]; both are load-bearing,
/// and the constants above are how an instance names one.
#[must_use]
pub fn plaza() -> SceneDesc<'static> {
    SceneDesc {
        meshes: vec![
            mesh_of("pavement", |builder| {
                builder.box_outward(
                    Vec3::new(-HALF_WIDTH, -PAVEMENT_THICKNESS, FAR_EDGE),
                    Vec3::new(HALF_WIDTH, 0.0, NEAR_EDGE),
                );
            }),
            mesh_of("parapet", |builder| {
                let (min, max) = parapet_box();
                builder.box_outward(min, max);
            }),
            mesh_of("colonnade", colonnade),
            mesh_of("plinth", |builder| {
                builder.box_outward(PLINTH_MIN, PLINTH_MAX);
            }),
            mesh_of("counters", counters),
        ],
        materials: vec![
            GpuMaterial {
                base_color: GROUND_COLOR,
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
                roughness: ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color: OBJECT_COLOR,
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
                roughness: ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
        ],
        page: PageDesc::opaque_white(PAGE_EXTENT),
        probes: crcbl::render::ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// How much of each pool [`plaza`] reserves.
///
/// Every one of these is device-local memory taken at start-up and never grown,
/// so they are the plaza's ceiling and not its size:
/// `the_plaza_fits_the_capacities_it_reserves` is what checks the description
/// against them with no GPU in the room.
pub const CAPACITIES: Capacities = Capacities {
    vertices: 1024,
    indices: 2048,
    meshes: 8,
    instances: 16,
    materials: 4,
    // The sun is not in this count — `ForwardRenderer::begin_frame` takes it —
    // so it is `lights()`' own length with room for one more.
    lights: 4,
    // No irradiance volume. A probe grid is a second source of indirect light in
    // every block a shadow is read in, and what this fixture measures is the
    // difference between a shadowed reading and a lit one.
    probes: 0,
};

/// Which mesh each object in the plaza is and which row it shades through, in
/// the order [`place`] inserts them.
///
/// **Insertion order is the caller's and it is load-bearing**, on
/// `crcbl::render::scene`'s terms: the slot an object lands in is
/// `docs/plan/25-lod.md`'s hysteresis key, so a golden compared across two runs
/// needs the two runs to have placed things in the same order.
const OBJECTS: [(usize, usize); 5] = [
    (PAVEMENT_MESH, GROUND_MATERIAL),
    (PARAPET_MESH, GROUND_MATERIAL),
    (COLONNADE_MESH, GROUND_MATERIAL),
    (PLINTH_MESH, OBJECT_MATERIAL),
    (COUNTERS_MESH, OBJECT_MATERIAL),
];

/// Puts every object in `renderer`, and reports how many.
///
/// The geometry is already at world scale and world position — every mesh above
/// is built where it stands — so each transform is the identity.
///
/// # Errors
///
/// [`InstancePoolError::PoolFull`] if [`CAPACITIES`]'s instance count does not
/// cover the plaza, which is a mistake in this file rather than a condition a run
/// can be in — but it is the caller that would have to report it, so it is
/// returned rather than unwrapped.
pub fn place(renderer: &mut ForwardRenderer) -> Result<usize, InstancePoolError> {
    let mut placed = 0;
    for (mesh, material) in OBJECTS {
        renderer.add_instance(&InstanceDesc {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        })?;
        placed += 1;
    }
    Ok(placed)
}

// ---------------------------------------------------------------------------
// What a reading can see
// ---------------------------------------------------------------------------

/// Column `index`'s box, as `(min, max)` corners.
fn column_box(index: usize) -> (Vec3, Vec3) {
    let foot = column_foot(index);
    (
        Vec3::new(foot.x - COLUMN_HALF, 0.0, foot.z - COLUMN_HALF),
        Vec3::new(foot.x + COLUMN_HALF, COLUMN_HEIGHT, foot.z + COLUMN_HALF),
    )
}

/// Counter `index`'s box, on the same terms.
fn counter_box(index: usize) -> (Vec3, Vec3) {
    let centre = counter_centre(index);
    (
        centre - Vec3::splat(COUNTER_HALF),
        centre + Vec3::splat(COUNTER_HALF),
    )
}

/// The parapet's box, on the same terms.
fn parapet_box() -> (Vec3, Vec3) {
    (
        Vec3::new(-PARAPET_HALF_WIDTH, 0.0, PARAPET_Z.0),
        Vec3::new(PARAPET_HALF_WIDTH, PARAPET_HEIGHT, PARAPET_Z.1),
    )
}

/// Whether the segment from `eye` to `at` passes through the box `min .. max`.
///
/// The slab test, with the far end of the segment left open: a reading *on* a
/// solid's own surface is not a reading hidden by it, and every point this is
/// asked about is a point on the pavement.
fn crosses(eye: Vec3, at: Vec3, min: Vec3, max: Vec3) -> bool {
    /// How much of the segment's far end is left out of the test, as a fraction.
    ///
    /// A sight line that ends on the pavement runs into the pavement, and the
    /// solids stand on it — so without this every reading beside a column's foot
    /// would answer "hidden by that column".
    const OPEN_END: f32 = 0.999;

    let along = at - eye;
    let (mut enter, mut leave) = (0.0f32, OPEN_END);
    for axis in 0..3 {
        let (from, span) = (eye[axis], along[axis]);
        if span.abs() < f32::EPSILON {
            if from < min[axis] || from > max[axis] {
                return false;
            }
            continue;
        }
        let (near, far) = ((min[axis] - from) / span, (max[axis] - from) / span);
        enter = enter.max(near.min(far));
        leave = leave.min(near.max(far));
        if enter > leave {
            return false;
        }
    }
    true
}

/// Whether any of the plaza's own geometry stands between `eye` and `at`.
///
/// **This plaza hides a good deal of its own floor**, and a reading that did not
/// ask would be a reading of whatever stands in front of the pavement it meant
/// to measure. The colonnade is the reason: it is a row of [`COLUMN_HEIGHT`]
/// columns across the middle of the frame, and the shadows it throws land
/// *behind* it from
/// [`fixed_camera`] over much of their length — so a walk along one of those
/// shadows crosses stretches where what the camera sees is a column's own face.
///
/// The boxes are the ones [`plaza`] builds its meshes from, through the same
/// corner helpers, rather than a second copy of them.
#[must_use]
pub fn hidden_from(eye: Vec3, at: Vec3) -> bool {
    (0..COLONNADE_COUNT)
        .map(column_box)
        .chain((0..COUNTERS.len()).map(counter_box))
        .chain([parapet_box(), (PLINTH_MIN, PLINTH_MAX)])
        .any(|(min, max)| crosses(eye, at, min, max))
}

/// Whether any of the plaza's punctual lights reaches `at`.
///
/// [`lights`]' own radii, which are hard bounds: `PointLight::radius` is where
/// the shading window reaches zero. A reading inside one of them carries that
/// lamp's own shadow as well as the sun's, and the two are not the same claim —
/// a punctual light's map is one tile of the atlas with no cascades in it at
/// all.
#[must_use]
pub fn lamplit(at: Vec3) -> bool {
    lights().iter().any(|light| {
        let (position, radius) = light.sphere();
        position.distance(at) < radius
    })
}

// ---------------------------------------------------------------------------
// The cameras
// ---------------------------------------------------------------------------

/// Where the fixed camera stands.
pub const FIXED_EYE: Vec3 = Vec3::new(0.0, 1.75, 6.0);

/// What it looks at: down the plaza, a little below eye height.
const FIXED_TARGET: Vec3 = Vec3::new(0.0, 0.30, 0.0);

/// The vertical field of view [`fixed_camera`] and [`pavement_camera`] share,
/// in radians.
///
/// The free camera opens on it too, because it starts at the fixture pose.
/// [`counter_camera`] takes a wider one of its own, for the reason its doc
/// gives.
const FOV_Y: f32 = 60.0 * core::f32::consts::PI / 180.0;

/// How close to the eye a surface may be and still be drawn, in metres.
///
/// **It is what sets where the first cascade ends**, and that is why this sample
/// does not take the two centimetres every other one opens with.
/// `Cascades::splits` blends a logarithmic and a uniform division of
/// `near .. DISTANCE`, and the logarithmic half is
/// `near * (DISTANCE / near).powf(ratio)` — so a near plane at two centimetres
/// puts the first split about four metres from the eye, which on a camera
/// standing 1.75 m up is a circle of pavement under four metres across. Every
/// claim this fixture makes lives either side of that circle or inside it:
/// **the cascade seam** has to be somewhere a shadow crosses it in frame, and
/// **the penumbra ladder** only widens at all where `sun_penumbra_texels`'
/// estimate is not clamped, which is the near cascade's texel and not the outer
/// one's.
///
/// Half a metre moves the split to a little over six, which is where the
/// colonnade and the counters both stand. Nothing in this scene is nearer to
/// either camera than the pavement it is standing on, so the clip costs
/// nothing — and the split is read back out of [`crcbl::render::Cascades`] by
/// the tests below rather than assumed, so a change here fails loudly instead of
/// quietly moving what the goldens are about.
const NEAR: f32 = 0.5;

/// The pose every fixture golden is taken from, and the one the free camera
/// starts at.
#[must_use]
pub fn fixed_camera() -> Camera {
    Camera {
        eye: FIXED_EYE,
        target: FIXED_TARGET,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            near: NEAR,
        },
    }
}

/// Where the counter pose stands.
const COUNTER_EYE: Vec3 = Vec3::new(2.85, 1.45, 2.60);

/// What it looks at: the middle counter's foot.
const COUNTER_TARGET: Vec3 = Vec3::new(2.85, 1.05, -1.60);

/// The counter pose's vertical field of view, in radians.
///
/// Wide, and the first cascade is why. The penumbra claim can only be read where
/// PCSS's estimate is not clamped, which is inside cascade 0 — a sphere of a
/// little over four metres around the eye — so the camera has to stand close
/// enough that all three shadows are inside it, and then a two-metre caster only
/// fits in a wide lens.
const COUNTER_FOV_Y: f32 = 66.0 * core::f32::consts::PI / 180.0;

/// **Where the penumbra claim is framed from.**
///
/// A second pose, and it is owed for `apps/alcove`'s `rim_camera`'s reason: at
/// [`fixed_camera`] the counters' shadows are a few dozen pixels across and a
/// penumbra a centimetre wide is not something a block average can see. This one
/// stands beside the counters with all three of their shadows in front of it.
///
/// **And inside the first cascade**, which is the harder constraint.
/// `sun_penumbra_texels` in `shaders/mesh.slang` clamps its estimate into
/// `SHADOW_FILTER_TEXELS .. SHADOW_SEARCH_TEXELS` — two to eight texels **of the
/// cascade the fragment landed in** — and a cascade's texel is six times coarser
/// in the outer one, so the same separation that widens a penumbra to eight
/// texels near the eye does not reach the lower clamp out there.
/// `the_counter_pose_reads_the_ladder_inside_the_first_cascade` is what holds
/// every one of the three shadows inside it, clear of the cross-fade band.
#[must_use]
pub fn counter_camera() -> Camera {
    Camera {
        eye: COUNTER_EYE,
        target: COUNTER_TARGET,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: COUNTER_FOV_Y,
            near: NEAR,
        },
    }
}

/// Where the pavement pose stands.
///
/// A little above eye height, a metre back from [`FIXED_EYE`] and off the
/// plaza's own axis, looking **across** the colonnade rather than down it.
/// Three things pin it, and they pull against each other:
///
/// * **Off the axis, so the columns stop standing in front of their own
///   shadows.** [`hidden_from`] refuses half of what `tests/golden.rs`'s cascade
///   walk samples from [`fixed_camera`] — the colonnade is a row of
///   [`COLUMN_HEIGHT`] columns across the middle of that frame and the pavement
///   its shadows fall on is behind them. From here it refuses under a quarter,
///   and the walks that keep a shell of pavement either side of the cascade
///   split are several columns' rather than one's.
///   `the_pavement_pose_frames_the_shadows_the_fixed_one_stands_in_front_of` is
///   what measures that.
/// * **Near enough that [`PLINTH_CONTACT`] and every station past it stay
///   inside cascade 0**, clear of the cross-fade band. `r_shadow_bias` and
///   `r_shadow_normal_offset` are counts of texels *of the cascade the fragment
///   landed in*, and the outer cascade's texel is several times the near one's
///   here — so a pose that pushed the contact past the split would be reading
///   those two counts at a different size, and the stations `tests/golden.rs`
///   swept from the fixture pose would not be the stations here.
/// * **Low enough that the acne block is still seen at a grazing incidence.**
///   That reading is a count of pixels below their own neighbourhood's median,
///   so it is a screen-space statistic: the shallower the view of the block, the
///   more shadow texels fall in one pixel and the more of the speckle the count
///   can see. Raised, the rise a constant bias is worth over that block falls
///   away, and `tests/golden.rs`'s floor under it stops being cleared.
///
/// **The height is where the last two of those meet, and it was swept** — on
/// radv at `CLAIM_EXTENT` with the rest of this pose held still, three runs per
/// station and the same digits every time (2026-09-05). The first two columns
/// are `tests/golden.rs`'s two acne readings, the third the ratio its cascade
/// walk bounds:
///
/// | eye height | `no bias` dots | `no offset` dots | walk ratio |
/// | --- | --- | --- | --- |
/// | 1.9 | `3.5980%` | `43.6812%` | `0.59` |
/// | 2.0 (this) | `3.3629%` | `41.9155%` | `0.96` |
/// | 2.1 | `2.8518%` | `39.3059%` | `3.25` |
/// | 2.2 | `2.2845%` | `37.0671%` | `2.31` |
/// | 2.5 | `1.4857%` | `29.4400%` | `3.97` |
///
/// At 2.5 the constant bias's rise is under the floor that clause is held over
/// and the row goes red; from 2.1 up the walk's own denominator — the steepest
/// step the same walk shows clear of the band — falls to a fraction of a level
/// and the ratio is a reading of the pavement's noise, which is the defect
/// `tests/golden.rs`'s `CASCADE_UNSEPARATED_RUNG` is about. Here the two steps
/// the ratio is made of are `0.90` and `0.94` out of 255, so neither of them is
/// noise, and 1.9 is a working station on the other side.
pub const PAVEMENT_EYE: Vec3 = Vec3::new(0.5, 2.0, 7.0);

/// What it looks at: the pavement between the plinth and the colonnade, on the
/// far side of the columns from the eye.
const PAVEMENT_TARGET: Vec3 = Vec3::new(-0.5, 0.30, 0.5);

/// **Where the pavement's own shadows are framed from.**
///
/// A third pose, and the one both of `tests/golden.rs`'s pavement claims are
/// read a **second** time from: the plinth's contact with the shadow a bias
/// trades against acne, and the colonnade's shadows where they cross the cascade
/// split. [`fixed_camera`] frames both and is the pose every constant either
/// claim holds to was swept on, so what those claims were short of is a second
/// place for a mis-set count or a hard cascade edge to show.
///
/// [`counter_camera`] could not be that place. It stands past the plinth's near
/// face looking away down the plaza, so [`PLINTH_CONTACT`] and every station
/// past it is behind its eye; and the colonnade stands across the plaza from the
/// counters, so no part of those shadows is both on screen there and inside the
/// shell of distance the split runs through.
///
/// **What it buys is the colonnade's own occlusion**, and what it is held to is
/// on [`PAVEMENT_EYE`], which carries the three constraints that place it.
/// `the_pavement_pose_frames_the_shadows_the_fixed_one_stands_in_front_of`
/// measures the two that can be measured here, with no GPU, rather than
/// asserting them in this paragraph.
#[must_use]
pub fn pavement_camera() -> Camera {
    Camera {
        eye: PAVEMENT_EYE,
        target: PAVEMENT_TARGET,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            near: NEAR,
        },
    }
}

/// Where cascade 0 ends for `camera` under `sky`, in metres from the eye.
///
/// [`crcbl::render::Cascades`]' own answer rather than a number written here:
/// the split is a function of the camera's near plane and
/// `crcbl::render::shadow::DISTANCE`, and a copy of it in this file is one that
/// goes stale the day either moves.
#[must_use]
pub fn cascade_split(camera: &Camera, sky: crate::sun::Sky) -> f32 {
    crcbl::render::Cascades::new(camera, sky.towards()).far[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun::{FIXTURE_TICK, NOON_TICK, SWEEP_TICKS, Sky};

    /// What fraction of a cascade's reach the shader fades into the next one
    /// over.
    ///
    /// `CASCADE_FADE_FRACTION` in `shaders/mesh.slang`, spelled here because
    /// nothing exports it: a fragment inside the band is a mixture of two
    /// cascades' answers and belongs to neither, so a claim about "which cascade
    /// this reading is in" has to stay out of it.
    const FADE_FRACTION: f32 = 0.1;

    /// **The description fits the pools it reserves**, with no GPU in the room.
    #[test]
    fn the_plaza_fits_the_capacities_it_reserves() {
        let scene = plaza();
        let (mut vertices, mut indices) = (0usize, 0usize);
        for mesh in &scene.meshes {
            let Geometry::Flat {
                vertices: bytes,
                indices: ids,
                ..
            } = &mesh.geometry
            else {
                panic!(
                    "the plaza describes {:?} as something other than a flat mesh",
                    mesh.label
                )
            };
            vertices += bytes.len() / mesh::VERTEX_STRIDE;
            indices += ids.len();
        }
        assert!(
            vertices <= CAPACITIES.vertices as usize,
            "the plaza has {vertices} vertices and reserves {}",
            CAPACITIES.vertices
        );
        assert!(
            indices <= CAPACITIES.indices as usize,
            "the plaza has {indices} indices and reserves {}",
            CAPACITIES.indices
        );
        assert!(
            scene.meshes.len() <= CAPACITIES.meshes as usize,
            "the plaza has {} meshes and reserves {}",
            scene.meshes.len(),
            CAPACITIES.meshes
        );
        assert!(
            scene.materials.len() <= CAPACITIES.materials as usize,
            "the plaza has {} material rows and reserves {}",
            scene.materials.len(),
            CAPACITIES.materials
        );
        assert!(
            OBJECTS.len() <= CAPACITIES.instances as usize,
            "the plaza places {} instances and reserves {}",
            OBJECTS.len(),
            CAPACITIES.instances
        );
        assert!(
            lights().len() <= CAPACITIES.lights as usize,
            "the plaza carries {} punctual lights and reserves {}",
            lights().len(),
            CAPACITIES.lights
        );
    }

    /// **Every mesh id names a mesh, every object shades through a row that
    /// exists, and every mesh is placed.**
    ///
    /// A mesh described and never inserted costs the memory and draws nothing,
    /// and the frame it leaves is a plaza with one object quietly missing — which
    /// is exactly the failure a golden cannot distinguish from a deliberate
    /// scene.
    #[test]
    fn every_mesh_the_plaza_describes_is_placed_and_shaded() {
        let scene = plaza();
        assert_eq!(
            scene.meshes.len(),
            COUNTERS_MESH + 1,
            "the mesh ids run to {COUNTERS_MESH} and the plaza describes {} meshes",
            scene.meshes.len()
        );
        for (mesh, material) in OBJECTS {
            assert!(mesh < scene.meshes.len(), "an object names mesh {mesh}");
            assert!(
                material < scene.materials.len(),
                "an object shades through row {material}"
            );
        }
        for id in 0..=COUNTERS_MESH {
            assert!(
                OBJECTS.iter().any(|(mesh, _)| *mesh == id),
                "mesh {id} is described and not placed"
            );
        }
        for (id, mesh) in scene.meshes.iter().enumerate() {
            let Geometry::Flat { indices, .. } = &mesh.geometry else {
                panic!("mesh {id} is not a flat mesh")
            };
            assert!(
                !indices.is_empty(),
                "mesh {id} ({:?}) has no triangles",
                mesh.label
            );
        }
    }

    /// **The colonnade straddles the cascade split, and [`SEAM_COLUMN`]'s shadow
    /// crosses it inside the frame.**
    ///
    /// `docs/plan/sample/18-sundial.md`'s "geometry crossing a cascade boundary",
    /// measured rather than asserted in prose. Three halves, and each is worth
    /// having on its own:
    ///
    /// * The nearest column's foot is inside cascade 0, clear of the cross-fade
    ///   band, and the furthest one's is outside the split — so the boundary
    ///   passes down the colonnade rather than in front of it or behind it.
    /// * [`SEAM_COLUMN`]'s shadow, walked from its foot to its tip at the fixture
    ///   sun, crosses the split **exactly once**. That is the claim the fixture is
    ///   actually for: a cascade seam shows on a shadow's own edge, where the two
    ///   cascades' texel footprints and biases differ, and not on a lit surface.
    /// * The crossing point projects **inside the fixed camera's frame**. A
    ///   boundary a shadow crosses behind the camera is one no golden can see, and
    ///   that is the failure this third clause exists to catch.
    ///
    /// The split is [`crcbl::render::Cascades`]' own. A colonnade laid out
    /// against a number copied into this file would go on passing after the
    /// camera's near plane moved and the split with it.
    #[test]
    fn the_colonnade_straddles_the_cascade_split() {
        /// The aspect the goldens are taken at.
        const ASPECT: f32 = 4.0 / 3.0;

        let camera = fixed_camera();
        let sky = Sky::at(FIXTURE_TICK);
        let split = cascade_split(&camera, sky);
        let band = split * (1.0 - FADE_FRACTION);

        let near_at = column_foot(0).distance(camera.eye);
        let far_at = column_foot(COLONNADE_COUNT - 1).distance(camera.eye);
        assert!(
            near_at < band,
            "the near column's foot is {near_at:.3} m from the eye, and cascade 0 fades out from \
             {band:.3} m — so it is inside the cross-fade rather than inside the cascade"
        );
        assert!(
            far_at > split,
            "the far column's foot is {far_at:.3} m from the eye and the split is at {split:.3} \
             m, so the whole colonnade is in one cascade and the boundary does not cross it"
        );

        // The seam column's shadow, from its foot to its tip: the top corner
        // followed along the sun to the pavement.
        let foot = column_foot(SEAM_COLUMN);
        let towards = sky.towards();
        let along = COLUMN_HEIGHT / towards.y;
        let tip = Vec3::new(foot.x - towards.x * along, 0.0, foot.z - towards.z * along);

        let steps = 512;
        let mut crossings: Vec<Vec3> = Vec::new();
        let mut previous = foot.distance(camera.eye) < split;
        let mut walked = foot;
        for step in 1..=steps {
            #[expect(clippy::cast_precision_loss, reason = "a step index of 512")]
            let at = foot.lerp(tip, step as f32 / steps as f32);
            let inside = at.distance(camera.eye) < split;
            if inside != previous {
                crossings.push(walked.lerp(at, 0.5));
                previous = inside;
            }
            walked = at;
        }
        assert_eq!(
            crossings.len(),
            1,
            "column {SEAM_COLUMN}'s shadow crosses the split {} times between {foot:?} and \
             {tip:?}, and a seam a shadow runs across exactly once is what this fixture is laid \
             out for",
            crossings.len()
        );

        let crossing = crossings[0];
        let clip = camera.view_projection(ASPECT) * crossing.extend(1.0);
        assert!(
            clip.w > 0.0,
            "the cascade crossing at {crossing:?} is behind the camera"
        );
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0,
            "the cascade crossing at {crossing:?} projects to {ndc:?}, outside the frame the \
             goldens are taken in — so nothing a golden can see is on the boundary"
        );
    }

    /// **The counter pose reads the whole ladder inside the first cascade.**
    ///
    /// [`counter_camera`]'s doc says why it has to: `sun_penumbra_texels` clamps
    /// its estimate into a texel count *of the cascade the fragment landed in*,
    /// and the outer cascade's texel is coarse enough that every separation this
    /// scene has falls on the lower clamp there — which is a filter of fixed
    /// width, and exactly the thing the claim is meant to tell PCSS apart from.
    ///
    /// So all three shadows have to be inside cascade 0 and clear of the
    /// cross-fade band, at the sun the reading is taken under.
    #[test]
    fn the_counter_pose_reads_the_ladder_inside_the_first_cascade() {
        let camera = counter_camera();
        let sky = Sky::at(NOON_TICK);
        let split = cascade_split(&camera, sky);
        let band = split * (1.0 - FADE_FRACTION);
        for index in 0..COUNTERS.len() {
            let at = counter_shadow(index, sky).distance(camera.eye);
            assert!(
                at < band,
                "counter {index}'s shadow lands {at:.3} m from the counter pose's eye, and \
                 cascade 0 fades out from {band:.3} m — the penumbra estimate there is the \
                 outer cascade's, which is clamped flat"
            );
        }
    }

    /// **The counters' shadows do not touch each other, or the plinth, or the
    /// control point.**
    ///
    /// Under the sun the ladder is read at. Two shadows that overlapped would be
    /// one reading of two casters, and the width measured across the overlap
    /// would belong to neither.
    #[test]
    fn the_counters_shadows_stand_apart_from_each_other_and_from_everything_else() {
        /// How far apart two shadow centres must stand, in metres. Twice a
        /// counter's own width plus the widest penumbra the ladder reaches, so
        /// the umbra of one is never inside the penumbra of another.
        const CLEARANCE: f32 = 1.0;

        let sky = Sky::at(NOON_TICK);
        let centres: Vec<Vec3> = (0..COUNTERS.len())
            .map(|index| counter_shadow(index, sky))
            .collect();
        for (index, centre) in centres.iter().enumerate() {
            for (other, against) in centres.iter().enumerate().skip(index + 1) {
                let apart = centre.distance(*against);
                assert!(
                    apart > CLEARANCE,
                    "counters {index} and {other} throw shadows {apart:.3} m apart, inside the \
                     {CLEARANCE} m of clear pavement each one's penumbra wants"
                );
            }
            assert!(
                centre.distance(PLINTH_CONTACT) > CLEARANCE,
                "counter {index}'s shadow lands on the plinth's contact reading"
            );
            assert!(
                centre.distance(OPEN_PAVEMENT) > CLEARANCE,
                "counter {index}'s shadow lands on the control point"
            );
        }
    }

    /// **The ladder is a ladder**: each counter hangs higher than the one before
    /// it, and the tallest hangs high enough for PCSS to have widened its filter
    /// past the floor the shader clamps it to.
    ///
    /// The second half is the one that would otherwise go silently wrong.
    /// `sun_penumbra_texels` clamps at `SHADOW_FILTER_TEXELS`, which is what
    /// `disc` takes everywhere — so a ladder whose every rung fell on that clamp
    /// would draw two identical frames under the two filters and the contact
    /// hardening claim would be vacuous rather than false.
    #[test]
    fn the_counters_climb_and_the_tallest_clears_the_filters_own_floor() {
        /// The lowest separation, in metres, at which
        /// `sun_penumbra_texels` returns more than `SHADOW_FILTER_TEXELS`.
        ///
        /// `separation * SHADOW_SUN_TAN_RADIUS / texel_world`, past two: with
        /// `SHADOW_SUN_TAN_RADIUS` at 0.02 and a whole root cell of
        /// `crcbl::render::shadow::TILE` texels over `2 * split` metres, that is
        /// `2 * texel_world / 0.02`. Written as a function of the split below
        /// rather than as a number, because both terms move with the camera.
        const SUN_TAN_RADIUS: f32 = 0.02;

        for pair in COUNTERS.windows(2) {
            assert!(
                pair[1].1 > pair[0].1,
                "the counters hang at {:?} and {:?}, which is not a ladder",
                pair[0],
                pair[1]
            );
        }

        let camera = counter_camera();
        let sky = Sky::at(NOON_TICK);
        let split = cascade_split(&camera, sky);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a shadow tile is a few hundred texels"
        )]
        let texel_world = 2.0 * split / crcbl::render::shadow::TILE as f32;
        let floor = 2.0 * texel_world / SUN_TAN_RADIUS;

        // The separation is measured along the light, not vertically: the sun is
        // not overhead, so a caster hanging `gap` above the pavement is
        // `gap / towards.y` of light-path away from what it shadows.
        let (lowest, tallest) = (COUNTERS[0].1, COUNTERS[COUNTERS.len() - 1].1);
        let along = sky.towards().y.recip();
        assert!(
            lowest * along < floor,
            "the lowest counter is {:.3} m of light-path above the pavement and the filter's own \
             floor is at {floor:.3} m — so even the bottom of the ladder widens the penumbra, \
             and the claim has no control in it",
            lowest * along
        );
        assert!(
            tallest * along > floor * 1.5,
            "the tallest counter is {:.3} m of light-path above the pavement and the filter's \
             floor is at {floor:.3} m, so the top of the ladder barely clears the width `disc` \
             takes everywhere",
            tallest * along
        );
    }

    /// **The contact reading stays inside the plinth's own shadow at every tick
    /// of the clock.**
    ///
    /// What makes it a peter-panning check rather than a reading of a dark patch:
    /// a point the shadow never reached would be dark for some other reason, or
    /// lit under a correct renderer too, and either way the golden's claim would
    /// say nothing. Where the point *sits* is settled beside
    /// [`PLINTH_CONTACT`] itself, at compile time.
    #[test]
    fn the_contact_reading_is_pavement_in_the_plinths_own_shadow() {
        // Walk the whole sweep: the shadow swings with the azimuth, and a
        // contact that fell out of it at one end of the arc would leave the
        // grazing golden's claim about a lit patch of pavement.
        for tick in (0..SWEEP_TICKS).step_by(10) {
            let towards = Sky::at(tick).towards();
            assert!(
                ray_meets_box(PLINTH_CONTACT, towards, PLINTH_MIN, PLINTH_MAX),
                "at tick {tick} the ray from the contact to the sun misses the plinth, so the \
                 contact is lit and there is no peter-panning to see"
            );
            // The control, and what makes the line above a claim rather than a
            // helper that says yes: the open pavement is the one point in this
            // scene nothing shadows, at any tick.
            assert!(
                !ray_meets_box(OPEN_PAVEMENT, towards, PLINTH_MIN, PLINTH_MAX),
                "at tick {tick} the plinth shadows the open pavement, which is the control \
                 every darkness reading is taken against"
            );
        }
    }

    /// Whether the ray leaving `from` towards `towards` meets the box
    /// `min..=max`, by the slab method.
    ///
    /// A whole-box test rather than the top face alone: the light reaching a
    /// point five centimetres from a face clips the *side* of the block a
    /// centimetre off the ground and never reaches the height of its top, so a
    /// top-plane test calls the contact lit at every grazing sun there is.
    fn ray_meets_box(from: Vec3, towards: Vec3, min: Vec3, max: Vec3) -> bool {
        let (mut entry, mut exit) = (0.0_f32, f32::INFINITY);
        for axis in 0..3 {
            let (origin, direction) = (from[axis], towards[axis]);
            if direction.abs() < 1e-6 {
                if origin < min[axis] || origin > max[axis] {
                    return false;
                }
                continue;
            }
            let first = (min[axis] - origin) / direction;
            let second = (max[axis] - origin) / direction;
            entry = entry.max(first.min(second));
            exit = exit.min(first.max(second));
            if entry > exit {
                return false;
            }
        }
        true
    }

    /// **Nothing reaches the control point.**
    ///
    /// [`OPEN_PAVEMENT`] separates "the shadow got darker" from "the frame got
    /// darker", and it can only do that if nothing else moves it: no punctual
    /// light within its own radius, and no caster up-sun of it at any tick.
    #[test]
    fn nothing_reaches_the_open_pavement() {
        for (index, light) in lights().iter().enumerate() {
            let (position, radius) = light.sphere();
            let apart = position.distance(OPEN_PAVEMENT);
            assert!(
                apart > radius,
                "light {index} stands {apart:.3} m from the control point and reaches {radius}, \
                 so the control is lit by something other than the sun"
            );
            let contact = position.distance(PLINTH_CONTACT);
            assert!(
                contact > radius,
                "light {index} stands {contact:.3} m from the contact reading and reaches \
                 {radius}, so a peter-panning check there would be reading a lamp"
            );
        }
    }

    /// **The control point is in full sun at every tick of the clock.**
    ///
    /// Walked against every caster in the plaza rather than argued: a shadow that
    /// swung across the control at one end of the sweep would make the grazing
    /// golden's claim a comparison of two shadows.
    #[test]
    fn the_open_pavement_is_in_sun_at_every_tick() {
        /// Every caster, as an axis-aligned box.
        fn casters() -> Vec<(Vec3, Vec3)> {
            let mut boxes = vec![
                (PLINTH_MIN, PLINTH_MAX),
                (
                    Vec3::new(-PARAPET_HALF_WIDTH, 0.0, PARAPET_Z.0),
                    Vec3::new(PARAPET_HALF_WIDTH, PARAPET_HEIGHT, PARAPET_Z.1),
                ),
            ];
            for index in 0..COLONNADE_COUNT {
                let foot = column_foot(index);
                boxes.push((
                    Vec3::new(foot.x - COLUMN_HALF, 0.0, foot.z - COLUMN_HALF),
                    Vec3::new(foot.x + COLUMN_HALF, COLUMN_HEIGHT, foot.z + COLUMN_HALF),
                ));
            }
            for index in 0..COUNTERS.len() {
                let centre = counter_centre(index);
                let half = Vec3::splat(COUNTER_HALF);
                boxes.push((centre - half, centre + half));
            }
            boxes
        }

        /// How many heights the ray is sampled at across one caster's band.
        const BAND_STEPS: usize = 16;

        let boxes = casters();
        assert!(boxes.len() > COLONNADE_COUNT, "the caster list is short");
        for tick in (0..SWEEP_TICKS).step_by(5) {
            let towards = Sky::at(tick).towards();
            for (min, max) in &boxes {
                // Walked through the box's own height band rather than sampled
                // at its two faces: the ray's `x` and `z` move as it climbs, so
                // a ray that entered the box's footprint above the bottom face
                // and left it below the top one would clear both of those and
                // still pass through the box.
                for step in 0..=BAND_STEPS {
                    #[expect(clippy::cast_precision_loss, reason = "a step index of 16")]
                    let fraction = step as f32 / BAND_STEPS as f32;
                    let height = min.y + (max.y - min.y) * fraction;
                    if height <= 0.0 {
                        continue;
                    }
                    let at = OPEN_PAVEMENT + towards * (height / towards.y);
                    assert!(
                        at.x < min.x || at.x > max.x || at.z < min.z || at.z > max.z,
                        "at tick {tick} the ray from the control point to the sun passes through \
                         the box between {min:?} and {max:?} at {at:?}"
                    );
                }
            }
        }
    }

    /// **Every light in the plaza is given a run of tiles**, off the engine's own
    /// allocator and with no GPU in the room.
    ///
    /// `docs/plan/sample/18-sundial.md`'s Scope says "at least one spot and two
    /// point lights, because two point lights is exactly what the 2026-08-26
    /// re-tiling bought and what a third would exceed". This reads that back: a
    /// point light's run is `POINT_FACES` tiles, two of them and a spot is
    /// thirteen of the fourteen the light region has, and
    /// `crcbl::render::shadow::Selection` is what hands them out. A fourth light,
    /// or a third point light, would leave one of them lighting without
    /// occluding — which is a frame that draws and a fixture that has stopped
    /// being about every light in it.
    #[test]
    fn every_light_in_the_plaza_is_given_a_run_of_tiles() {
        use crcbl::render::shadow::{LIGHT_TILES, POINT_FACES, Selection};

        let plaza_lights = lights();
        let points = plaza_lights
            .iter()
            .filter(|light| matches!(light, Light::Point(_)))
            .count();
        let spots = plaza_lights
            .iter()
            .filter(|light| matches!(light, Light::Spot(_)))
            .count();
        assert_eq!(points, 2, "the charter asks for two point lights");
        assert!(spots >= 1, "the charter asks for at least one spot");
        assert!(
            points * POINT_FACES + spots <= LIGHT_TILES,
            "{points} cubes and {spots} cones want {} tiles and the light region holds \
             {LIGHT_TILES}",
            points * POINT_FACES + spots
        );

        // Every pose, because the ranking is by what a map covers on *screen*
        // and the cameras stand in different places.
        for (name, camera) in [
            ("the fixed pose", fixed_camera()),
            ("the counter pose", counter_camera()),
            ("the pavement pose", pavement_camera()),
        ] {
            let mut selection = Selection::default();
            selection.update(&plaza_lights, &camera);
            let held: Vec<usize> = selection
                .slots()
                .iter()
                .flatten()
                .map(|assignment| assignment.light)
                .collect();
            for index in 0..plaza_lights.len() {
                assert!(
                    held.contains(&index),
                    "light {index} got no run of tiles from {name}, so it lights without \
                     occluding: the allocator held {held:?}"
                );
            }
        }
    }

    /// **The two poses see what they are aimed at**, and every read point is in
    /// front of the camera reading it.
    ///
    /// A point that projects outside the frame is a claim about a pixel that is
    /// not there, and `tests/golden.rs`'s own `project` would panic on it — on a
    /// device, at the end of a run. This asks the same question here.
    #[test]
    fn every_read_point_is_inside_the_frame_it_is_read_from() {
        /// The aspect the goldens are taken at.
        const ASPECT: f32 = 4.0 / 3.0;

        let fixed = fixed_camera();
        let counter = counter_camera();
        let noon = Sky::at(NOON_TICK);
        let mut points: Vec<(&str, Vec3, &Camera)> = vec![
            ("the contact", PLINTH_CONTACT, &fixed),
            ("the control", OPEN_PAVEMENT, &fixed),
            ("the near column's foot", column_foot(0), &fixed),
        ];
        for index in 0..COUNTERS.len() {
            points.push(("a counter's shadow", counter_shadow(index, noon), &counter));
            points.push(("a counter", counter_centre(index), &counter));
        }
        for (name, point, camera) in points {
            let clip = camera.view_projection(ASPECT) * point.extend(1.0);
            assert!(clip.w > 0.0, "{name} at {point:?} is behind its camera");
            let ndc = clip.truncate() / clip.w;
            assert!(
                ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0,
                "{name} at {point:?} projects to {ndc:?}, outside its own frame"
            );
        }
    }

    /// **The pavement pose frames the shadows the fixed one stands in front
    /// of.**
    ///
    /// [`pavement_camera`]'s reason, measured here rather than argued in its
    /// doc. Two halves, and the pose is owed for both:
    ///
    /// * **The plinth's contact and the stations past it are in frame.** That is
    ///   what [`counter_camera`] is short of — they stand behind its eye — and
    ///   it is why the bias pair in `tests/golden.rs` had one pose and not two.
    /// * **More of the colonnade's shadow reaches this frame**, over the window
    ///   of distance the cascade split runs through, where "reaches" is the
    ///   golden suite's own set of refusals: [`hidden_from`], [`lamplit`] and
    ///   the frame itself. From [`FIXED_EYE`], down the plaza's own axis, the
    ///   colonnade stands in front of most of the pavement its own shadows fall
    ///   on; from [`PAVEMENT_EYE`] the sight lines cross the row instead of
    ///   running along it.
    ///
    /// The second half is a comparison and not a floor, because what it is about
    /// is the *difference* between the two poses — a floor would go on holding
    /// if the pose were moved back down to eye level and the colonnade were
    /// moved out of its way instead.
    #[test]
    fn the_pavement_pose_frames_the_shadows_the_fixed_one_stands_in_front_of() {
        /// The aspect every reading in `tests/golden.rs` is taken at.
        const ASPECT: f32 = 4.0 / 3.0;
        /// How far past [`PLINTH_CONTACT`], along the plinth's own shadow, the
        /// stations `tests/golden.rs`'s `BEYOND_CONTACT` reads stand, in metres
        /// of pavement. Spelled here because nothing exports them.
        const BEYOND: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];

        let in_frame = |camera: &Camera, at: Vec3| {
            let clip = camera.view_projection(ASPECT) * at.extend(1.0);
            if clip.w <= 0.0 {
                return false;
            }
            let ndc = clip.truncate() / clip.w;
            ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0
        };

        let camera = pavement_camera();
        for out in BEYOND {
            let at = PLINTH_CONTACT + Vec3::new(0.0, 0.0, out);
            assert!(
                in_frame(&camera, at),
                "the contact reading {out:.1} m out at {at:?} is behind the pavement pose's eye \
                 or outside its frame, so `tests/golden.rs`'s `project` would panic on it"
            );
            assert!(
                !hidden_from(camera.eye, at),
                "the plaza's own geometry stands between the pavement pose and the contact \
                 reading {out:.1} m out at {at:?}, so what is read there is a solid's face"
            );
        }

        // How many walks along the colonnade's shadows a pose can read on both
        // sides of its own cascade split — the golden suite's own window, and
        // its own refusals.
        //
        // Off the shadow's axis and not down the middle of it, because that is
        // where a cascade switch shows and where `tests/golden.rs` reads: two
        // cascades answer the same nothing deep in an umbra and the same
        // everything out on open pavement, and they differ at the edge. The
        // offsets are [`COLUMN_HALF`]'s own multiples, which is where the
        // geometric edge of a vertical caster's shadow stands.
        const OFF_AXIS: [f32; 6] = [-1.5, -1.0, -0.5, 0.5, 1.0, 1.5];

        let sky = Sky::at(FIXTURE_TICK);
        let towards = sky.towards();
        let axis = Vec3::new(-towards.x, 0.0, -towards.z).normalize();
        let perp = Vec3::new(axis.z, 0.0, -axis.x);
        let walks_read = |camera: &Camera| {
            let split = cascade_split(camera, sky);
            let band = split * FADE_FRACTION;
            let (near_end, far_end) = (band.mul_add(-2.0, split), band.mul_add(0.5, split));
            let steps = 2048;
            let mut walks = 0u32;
            for column in 0..COLONNADE_COUNT {
                let foot = column_foot(column);
                let along = COLUMN_HEIGHT / towards.y;
                let tip = Vec3::new(foot.x - towards.x * along, 0.0, foot.z - towards.z * along);
                for lateral in OFF_AXIS {
                    let sideways = perp * (lateral * COLUMN_HALF);
                    let (mut inside, mut outside) = (false, false);
                    for step in 0..=steps {
                        #[expect(clippy::cast_precision_loss, reason = "a step index of 2048")]
                        let at = foot.lerp(tip, step as f32 / steps as f32) + sideways;
                        let distance = at.distance(camera.eye);
                        if distance < near_end
                            || distance >= far_end
                            || hidden_from(camera.eye, at)
                            || lamplit(at)
                            || !in_frame(camera, at)
                        {
                            continue;
                        }
                        if distance < split - band {
                            inside = true;
                        } else if distance > split {
                            outside = true;
                        }
                    }
                    if inside && outside {
                        walks += 1;
                    }
                }
            }
            walks
        };

        let (fixed, raised) = (walks_read(&fixed_camera()), walks_read(&camera));
        assert!(
            fixed >= 1,
            "the fixed pose reads none of the colonnade's shadows across its own split, so the \
             comparison below is against nothing and `tests/golden.rs`'s cascade walk reads \
             nothing from that pose either"
        );
        assert!(
            raised > fixed,
            "the pavement pose reads {raised} walks along the colonnade's shadows across the \
             cascade split and the fixed pose reads {fixed} — a second pose that sees no more \
             of them than the first is a second frame of the same claim"
        );
    }

    /// **The grazing golden's sun really is the lowest one the clock reaches**,
    /// and the plaza is long enough to hold the shadows it throws.
    ///
    /// A shadow that ran off the end of the pavement would leave the acne golden
    /// a picture of a plane's edge.
    #[test]
    fn the_grazing_sun_keeps_its_shadows_on_the_pavement() {
        // The far column, whose shadow is the longest thing in the scene that
        // has to stay on the plane — and every tick, not only the lowest sun:
        // the arc swings the azimuth as well, so the reach and the sideways
        // drift peak at different ends of it.
        let foot = column_foot(COLONNADE_COUNT - 1);
        let (mut furthest, mut widest) = (f32::MIN, 0.0_f32);
        for tick in 0..SWEEP_TICKS {
            let towards = Sky::at(tick).towards();
            let along = COLUMN_HEIGHT / towards.y;
            let tip = Vec3::new(foot.x - towards.x * along, 0.0, foot.z - towards.z * along);
            assert!(
                tip.z < NEAR_EDGE && tip.z > FAR_EDGE,
                "at tick {tick} the far column's shadow reaches z={:.2} and the pavement runs \
                 {FAR_EDGE}..{NEAR_EDGE}",
                tip.z
            );
            assert!(
                tip.x.abs() < HALF_WIDTH,
                "at tick {tick} the far column's shadow reaches x={:.2} and the pavement is \
                 {HALF_WIDTH} half-wide",
                tip.x
            );
            furthest = furthest.max(tip.z);
            widest = widest.max(tip.x.abs());
        }
        // Anti-vacuity: a plane large enough for the whole sweep is only worth
        // asserting if the sweep actually walks out towards its edges.
        assert!(
            furthest > NEAR_EDGE - 2.0 && widest > HALF_WIDTH - 4.0,
            "the longest shadow stops at z={furthest:.2} and x={widest:.2}, well inside a \
             {HALF_WIDTH}-half-wide plane running {FAR_EDGE}..{NEAR_EDGE} — the pavement is \
             sized for a sweep the sun no longer takes"
        );
    }
}
