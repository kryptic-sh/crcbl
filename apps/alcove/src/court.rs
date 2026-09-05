//! The court, as data an application hands the engine.
//!
//! ```text
//!  MeshBuilder ──▶ build_meshlets ──▶ Geometry::Flat ──┐
//!  GpuMaterial rows ───────────────────────────────────┼─▶ SceneDesc ──▶ with_scene
//!  PageDesc::empty ────────────────────────────────────┘
//!  place() ──▶ add_instance ×N
//! ```
//!
//! Nothing here names a device and nothing here is the engine's content, on
//! `apps/lantern/src/room.rs`' terms exactly: the meshes are baked from literals
//! by this module's own quad builder, the two material rows are this sample's,
//! and the page is [`PageDesc::empty`] — every row names
//! `GpuMaterial::NO_PAGE`, so there is no image to allocate at all, and this is
//! the one sample whose surfaces must carry no pattern anywhere.
//!
//! # Every surface here is an occluder and nothing else
//!
//! `docs/plan/sample/19-alcove.md`'s Scope: "one interior scene of nothing but
//! occlusion geometry — an alcove, a stair underside, boxes resting on a floor
//! for the contact-shadow claim, a deep crease lit directly, and a curved object
//! silhouetted against distance". Each of those is one thing below, and the
//! constants that place it say which claim it carries.
//!
//! **Flat and near-untextured by choice.** Texture detail is exactly what hides
//! an ambient-occlusion artefact, so the page has one white texel in it and the
//! two material rows differ by four hundredths of an albedo — enough to tell a
//! standing object from the shell it stands in, not enough to be a second factor
//! in any product a reader is attributing to the occlusion term.
//!
//! # The court has no roof, and that is what makes the crease claim possible
//!
//! Four walls, a floor and open sky. A roofed room admits the sun through one
//! opening and lights a band of floor a couple of metres wide; every claim below
//! would then be a claim about where that band landed. Open to the sky, the
//! floor is lit everywhere the geometry does not shadow it, and what darkens a
//! surface is the geometry rather than the aperture — which is the whole subject
//! of this sample.
//!
//! # The camera, the sun and [`slot_axis`] are one line
//!
//! The three are deliberately parallel in plan, and that is what makes
//! [`crease_lit`] a claim rather than a coincidence:
//!
//! * The sun's **azimuth** is [`slot_axis`], so the sun's rays run *along* the
//!   slot between [`SLOT_WALL_HEIGHT`]-tall walls and neither wall can shadow
//!   the floor between them, at any depth. The floor of the deepest crease in
//!   this scene is in full sun by construction rather than by a measurement of
//!   where a shaft fell.
//! * The fixed camera stands on that same line, so the eye ray to
//!   [`crease_lit`] runs down the slot too and neither wall can hide it.
//! * The sun is therefore *ahead* of the camera. Every vertical surface the
//!   camera can see faces away from it and carries no direct light at all, so
//!   what models those surfaces is the ambient term alone — which is the term
//!   ambient occlusion scales, and the reason this sample can read it off a
//!   picture.
//!
//! [`PageDesc::empty`]: crcbl::render::scene::PageDesc::empty

use std::borrow::Cow;

use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    Camera, Capacities, DirectionalLight, ForwardRenderer, Geometry, InstanceDesc,
    InstancePoolError, MeshDesc, PageDesc, Projection, SceneDesc,
};
use crcbl::shaders::mesh::{self, GpuMaterial, MeshVertex};
use crcbl::shaders::vertex::UvRange;

// ---------------------------------------------------------------------------
// The court's dimensions
// ---------------------------------------------------------------------------

/// Half the court's width, in metres. The side walls stand at `±HALF_WIDTH`.
pub const HALF_WIDTH: f32 = 2.6;

/// Half the court's depth. The far wall is at `-HALF_DEPTH` and the near wall,
/// which the fixed camera stands just inside, at `+HALF_DEPTH`.
pub const HALF_DEPTH: f32 = 3.4;

/// How tall the walls stand, in metres.
///
/// Taller than a room, because the court has no roof and the walls are what the
/// frame's upper half is made of: at [`fixed_camera`]'s field of view the top
/// of the frame lands just under the top of the far wall, and a shorter wall
/// would put the clear colour across the top of every golden — which is a
/// claim the goldens' own top rows check, since every one of them is wall.
pub const WALL_HEIGHT: f32 = 2.9;

/// How thick every slab in the shell is.
///
/// Slabs rather than single quads, for `apps/lantern/src/room.rs`' reason: the
/// sun has to *see* a surface, and a plane with no thickness casts a shadow only
/// from one side.
const SHELL: f32 = 0.12;

// ---------------------------------------------------------------------------
// The sun
// ---------------------------------------------------------------------------

/// How bright the sun is, before its colour.
///
/// Above 1.0, like every other sun in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back.
const SUN_INTENSITY: f32 = 1.0;

/// The direction **towards** the sun, before normalising.
///
/// The `z` component is negative, which is what puts the sun in front of the
/// camera rather than behind it — see this module's header. The `x` component is
/// small and non-zero so the two side walls are not lit identically, and it is
/// what [`slot_axis`] is derived from rather than a second opinion about.
const SUN_TOWARDS: Vec3 = Vec3::new(-0.16, 0.74, -0.65);

/// The flat ambient term, and **the whole of what the occlusion pass scales**.
///
/// Large next to `apps/lantern`'s, and deliberately: that fixture's subject is
/// direct light and its ambient is a floor under it, while this one's subject is
/// the term itself. Every vertical surface the fixed camera sees is lit by this
/// and by nothing else, so a change in the occlusion channel is a change in the
/// picture rather than a percent of one.
const AMBIENT: Vec3 = Vec3::new(0.22, 0.235, 0.27);

/// The sun: open sky, from ahead of the camera and above.
#[must_use]
pub fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: SUN_TOWARDS.normalize(),
        color: Vec3::new(1.0, 0.97, 0.92) * SUN_INTENSITY,
        ambient: AMBIENT,
    }
}

/// The sun's azimuth: `SUN_TOWARDS` flattened onto the floor and normalised.
///
/// **The one direction three things share** — see this module's header. Derived
/// here rather than written down, so moving the sun moves the slot and the
/// camera with it instead of leaving a crease the sun no longer runs down.
#[must_use]
pub fn slot_axis() -> Vec3 {
    Vec3::new(SUN_TOWARDS.x, 0.0, SUN_TOWARDS.z).normalize()
}

// ---------------------------------------------------------------------------
// The deep crease: two walls the sun runs between
// ---------------------------------------------------------------------------

/// Where the slot's centre line starts, at the end nearest the camera.
const SLOT_NEAR: Vec3 = Vec3::new(0.30, 0.0, 1.30);

/// How long the slot's walls run, in metres.
const SLOT_LENGTH: f32 = 1.6;

/// How far apart the slot's two inner faces stand, in metres.
///
/// Half a metre — the disc `crcbl_render::ssao::r_ssao_radius` sweeps by default
/// — so a point on the floor between them has a wall inside that radius on both
/// sides and the crease is as deep as the shipped setting can see.
const SLOT_GAP: f32 = 0.24;

/// How thick each slot wall is.
const SLOT_WALL_THICKNESS: f32 = 0.12;

/// How tall the slot's walls stand.
///
/// Above head height at the floor of the slot, so the crease is deep rather than
/// a kerb — and it costs nothing, because the sun runs *along* the slot and no
/// height of wall can shadow the floor between them.
pub const SLOT_WALL_HEIGHT: f32 = 0.75;

/// How far along the slot [`crease_lit`] sits, as a fraction of its length.
const SLOT_SAMPLE_AT: f32 = 0.35;

/// **The surface in direct sunlight inside a deep crease.**
///
/// The floor at the middle of the slot: a wall `SLOT_GAP``/2` away on each
/// side and the floor under it, and full sun on it because the sun's rays run
/// down the slot. `docs/plan/sample/19-alcove.md`'s "AO darkens the ambient term
/// and nothing else" is read here — an implementation that scaled the direct
/// term would take this point down with the rest of the crease, and this is the
/// one place in the scene where those two are separable.
#[must_use]
pub fn crease_lit() -> Vec3 {
    SLOT_NEAR + slot_axis() * (SLOT_LENGTH * SLOT_SAMPLE_AT)
}

// ---------------------------------------------------------------------------
// The alcove
// ---------------------------------------------------------------------------

/// The alcove block's outer corner nearest `-x`, `-y`, `-z`.
const ALCOVE_MIN: Vec3 = Vec3::new(-2.35, 0.0, -0.55);

/// Its outer corner nearest `+x`, `+y`, `+z`. The mouth is in the `+z` face.
const ALCOVE_MAX: Vec3 = Vec3::new(-1.05, 1.7, 0.55);

/// How deep the recess cuts into the block, from its `+z` face.
const ALCOVE_RECESS: f32 = 0.6;

/// The mouth's `x` span.
const ALCOVE_MOUTH_X: (f32, f32) = (-2.15, -1.25);

/// The mouth's `y` span: a sill and a head, both thick enough to occlude.
const ALCOVE_MOUTH_Y: (f32, f32) = (0.18, 1.15);

/// **The alcove's back corner** — the point the contact claim's other half is
/// measured at.
///
/// On the recess's back face, eight centimetres from the `-x` jamb and eight
/// above the sill: three surfaces well inside the occlusion radius. Its normal
/// is `+Z` and the sun's `z` is negative, so it carries **no direct light at
/// all** whatever the shadow atlas resolves — which is what makes the reading
/// here the ambient term times the occlusion channel and nothing else.
pub const ALCOVE_CORNER: Vec3 = Vec3::new(-2.07, 0.26, ALCOVE_MAX.z - ALCOVE_RECESS);

// ---------------------------------------------------------------------------
// The stair
// ---------------------------------------------------------------------------

/// Where the flight's lowest tread starts in `x`.
const STAIR_X0: f32 = 0.7;

/// How far each tread runs in `x`.
const STAIR_RUN: f32 = 0.3;

/// How far each tread rises above the one below it.
const STAIR_RISE: f32 = 0.3;

/// How thick a tread is.
const STAIR_THICKNESS: f32 = 0.1;

/// How far a tread stands proud of the far wall.
const STAIR_NOSING: f32 = 0.5;

/// How many treads the flight has.
///
/// Enough that the flight climbs past [`fixed_eye`] and the **undersides** of
/// its upper treads are in the frame: an underside seen from below, against the
/// wall it is cantilevered from, is the crease
/// `docs/plan/sample/19-alcove.md`'s Scope asks a stair for, and a flight whose
/// treads all sat below the eye would show none of them.
/// `the_stair_climbs_past_the_eye` is what holds that rather than this
/// sentence.
const STAIR_TREADS: usize = 6;

// ---------------------------------------------------------------------------
// The boxes resting on the floor
// ---------------------------------------------------------------------------

/// The large box's corners. Its `+z` face is what [`CONTACT_BAND`] is measured
/// against.
const BOX_MIN: Vec3 = Vec3::new(0.62, 0.0, 0.35);
/// The large box's far corner, on [`BOX_MIN`]'s terms.
const BOX_MAX: Vec3 = Vec3::new(1.32, 0.5, 1.05);

/// The post beside it, leaving [`CREVICE_GAP`] between the two.
const POST_MIN: Vec3 = Vec3::new(1.39, 0.0, 0.45);
/// The post's far corner.
const POST_MAX: Vec3 = Vec3::new(1.55, 0.75, 0.85);

/// How wide the crevice between the box and the post is.
///
/// Seven centimetres — a seventh of the shipped occlusion radius, so the two
/// faces either side of it are nearly fully occluded by each other. It is the
/// narrowest feature in the scene and the one a radius control is legible on.
pub const CREVICE_GAP: f32 = POST_MIN.x - BOX_MAX.x;

/// The low box, standing clear of everything.
const LOW_BOX_MIN: Vec3 = Vec3::new(-1.75, 0.0, 1.15);
/// The low box's far corner.
const LOW_BOX_MAX: Vec3 = Vec3::new(-1.3, 0.3, 1.6);

/// **The contact band**: floor a hand's breadth out from the large box's `+z`
/// face.
///
/// In that box's own shadow — the sun is ahead of the camera, so a box shadows
/// the floor between itself and the eye — so it carries no direct light, and a
/// few centimetres from the box's face and of the floor's own plane.
/// `docs/plan/sample/19-alcove.md`'s "boxes resting on a floor for the
/// contact-shadow claim" is read here.
pub const CONTACT_BAND: Vec3 = Vec3::new(0.97, 0.0, BOX_MAX.z + 0.045);

/// **The contact band is floor, beside the box, close enough to be a contact
/// reading.**
///
/// Checked here rather than in a test because every term is a constant: the
/// compiler evaluates it, so a nudge to [`CONTACT_BAND`] or to the box that
/// pushed the point under the box, off the end of its face, or a hand's breadth
/// too far out to be a contact shadow at all does not build. A runtime
/// assertion over the same constants is one clippy folds away.
const _: () = {
    assert!(
        CONTACT_BAND.y == 0.0,
        "the contact band is off the floor it is meant to read"
    );
    assert!(
        CONTACT_BAND.z > BOX_MAX.z,
        "the contact band is under the box rather than beside it"
    );
    assert!(
        CONTACT_BAND.z - BOX_MAX.z < 0.25,
        "the contact band is too far out from the box's face to be a contact reading"
    );
    assert!(
        CONTACT_BAND.x > BOX_MIN.x && CONTACT_BAND.x < BOX_MAX.x,
        "the contact band is off the end of the box's face"
    );
};

/// **The control for every claim above**: floor in the open, out of reach of
/// anything.
///
/// Over a metre from the nearest surface in every direction and in full sun, so
/// the occlusion channel is one here and switching the pass off must not move
/// it. It is what separates "the occlusion pass stopped darkening its corner"
/// from "the whole frame got brighter".
pub const OPEN_FLOOR: Vec3 = Vec3::new(-0.55, 0.0, 1.55);

// ---------------------------------------------------------------------------
// The curved object
// ---------------------------------------------------------------------------

/// The pedestal's corners.
const PEDESTAL_MIN: Vec3 = Vec3::new(-0.95, 0.0, -1.6);
/// The pedestal's far corner.
const PEDESTAL_MAX: Vec3 = Vec3::new(-0.45, 0.35, -1.1);

/// Where the sphere's centre stands.
pub const SPHERE_CENTRE: Vec3 = Vec3::new(-0.7, 0.77, -1.35);

/// How large the sphere is.
pub const SPHERE_RADIUS: f32 = 0.42;

/// How many segments the sphere is swept into around its axis.
const SPHERE_SEGMENTS: usize = 20;

/// How many rings it is divided into from pole to pole.
const SPHERE_RINGS: usize = 12;

/// **Where the silhouette claim is framed from.**
///
/// A second pose, and the charter asks for it: "a golden per technique, plus one
/// framing the silhouette rim". [`fixed_camera`] sees the whole court and the
/// sphere in it is a few dozen pixels across, which is not a framing anybody can
/// judge a one-pixel rim at. This one stands square in front of the sphere with
/// the far wall behind it, so the silhouette runs down the middle of the frame
/// with flat wall either side.
///
/// **Straight down `-z`**, so the sphere's limb is symmetric in the image and
/// [`frame_right`] is the world `+x` — which is what makes [`rim_outside`] a
/// horizontal step in the picture as well as in the court.
#[must_use]
pub fn rim_camera() -> Camera {
    Camera {
        eye: Vec3::new(SPHERE_CENTRE.x, SPHERE_CENTRE.y + RIM_EYE_LIFT, RIM_EYE_Z),
        target: SPHERE_CENTRE,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: RIM_FOV_Y,
            near: 0.02,
        },
    }
}

/// How far in front of the sphere [`rim_camera`] stands.
const RIM_EYE_Z: f32 = 1.15;

/// How far above the sphere's centre its eye sits.
///
/// Small: enough that the pedestal reads as a pedestal rather than as a line,
/// and little enough that the limb stays near the middle of the frame.
const RIM_EYE_LIFT: f32 = 0.13;

/// [`rim_camera`]'s vertical field of view, in radians.
///
/// Narrow, which is the whole point of the second pose: the sphere fills about
/// half the frame's height, so the handful of pixels either side of its limb are
/// a region a person can look at.
const RIM_FOV_Y: f32 = 40.0 * core::f32::consts::PI / 180.0;

/// [`rim_camera`]'s own horizontal axis, pointing to the right of its frame.
///
/// What "just outside the silhouette" is measured along: a world offset along
/// this is a horizontal offset in the *image*, which is where a halo appears.
#[must_use]
pub fn frame_right() -> Vec3 {
    let camera = rim_camera();
    (camera.target - camera.eye)
        .normalize()
        .cross(Vec3::Y)
        .normalize()
}

/// Where [`rim_camera`]'s ray through `point` meets the far wall's inner face.
///
/// The wall is the plane `z = -HALF_DEPTH` and the eye is in front of it, so the
/// ray crosses it exactly once. `the_rim_points_stand_on_the_far_wall` is what
/// holds the result inside the wall and clear of everything else.
///
/// # Panics
///
/// If `point` is not between the eye and the wall, which neither caller below
/// can produce and which would otherwise be a claim about a pixel somewhere else
/// entirely.
#[must_use]
pub fn wall_behind(point: Vec3) -> Vec3 {
    let eye = rim_camera().eye;
    let along = point - eye;
    assert!(along.z < 0.0, "{point:?} is not in front of the rim camera");
    let at = (-HALF_DEPTH - eye.z) / along.z;
    assert!(
        at > 1.0,
        "{point:?} is behind the far wall rather than in front of it"
    );
    eye + along * at
}

/// How far outside the sphere's limb [`rim_outside`] stands, in metres at the
/// sphere's own depth.
///
/// At [`rim_camera`]'s framing this is a few pixels: far enough that the block
/// is wall rather than sphere, near enough that a halo covers it.
const RIM_CLEARANCE: f32 = 0.06;

/// How far **inside** the limb [`rim_inside`] stands, on `RIM_CLEARANCE`'s
/// terms.
///
/// Larger than the clearance outside it, and measured rather than chosen: the
/// sphere's visible face carries no direct light — the sun is behind it from
/// [`rim_camera`] — so sphere and wall separate only by the occlusion on them,
/// and a block right against the limb reads within three codes of the wall. At
/// this inset it is three and a half, which is what the straddle assertion has
/// to work with.
const RIM_INSET: f32 = 0.14;

/// How far above [`rim_outside`] its comparand stands.
///
/// Nearly two occlusion radii, so the silhouette below cannot reach it, and low
/// enough that it is still wall a person can see.
const RIM_LIFT: f32 = 0.9;

/// **A point on the far wall, just outside the sphere's silhouette.**
///
/// The far wall is two metres behind the sphere — four times the shipped
/// occlusion radius — so nothing on the sphere may darken this point. It is
/// where `docs/plan/18-render-features.md`'s escalation clause becomes visible:
/// normals reconstructed from depth are exact on a plane and wrong on the one
/// pixel of wall next to a silhouette, and a halo is what that looks like.
///
/// Derived rather than written down, so it follows the sphere and the camera:
/// the sphere's centre pushed `RIM_CLEARANCE` past its own limb along
/// [`frame_right`], and then followed to the wall.
#[must_use]
pub fn rim_outside() -> Vec3 {
    wall_behind(SPHERE_CENTRE - frame_right() * (SPHERE_RADIUS + RIM_CLEARANCE))
}

/// **A point the sphere covers, inside the same silhouette.**
///
/// The anti-vacuity half of the rim claim: a bound on how much the wall beside a
/// silhouette may darken says nothing unless there is a silhouette there, and
/// two blocks of flat wall would satisfy it perfectly. This one projects to a
/// pixel the sphere is drawn at, so the pair straddles a real depth
/// discontinuity — a couple of metres of it.
#[must_use]
pub fn rim_inside() -> Vec3 {
    SPHERE_CENTRE - frame_right() * (SPHERE_RADIUS - RIM_INSET)
}

/// **The same wall, far from the silhouette**, and the comparand
/// [`rim_outside`] is held against.
///
/// Directly above it by `RIM_LIFT`, so it is the same surface, the same
/// material and the same normal, with nothing within the occlusion radius: a run
/// in which this is not near white is a run in which the block is not on flat
/// wall at all, and the halo ratio would be a ratio of two darkened readings.
#[must_use]
pub fn rim_far() -> Vec3 {
    rim_outside() + Vec3::Y * RIM_LIFT
}

// ---------------------------------------------------------------------------
// The material rows
// ---------------------------------------------------------------------------

/// The shell's row: the floor, the walls, the alcove, the stair and the slot.
///
/// **Row 0**, which is what [`mesh::GpuInstance::default`] names, so it is the
/// row an object placed without a material id shades through.
pub const SHELL_MATERIAL: usize = 0;

/// The standing objects' row: the boxes, the post, the pedestal and the sphere.
///
/// Four hundredths darker than [`SHELL_MATERIAL`] and identical in every other
/// column. Near-uniform is the charter's word: enough to tell an object from the
/// floor it rests on, not enough for a reader to mistake an albedo step for an
/// occlusion band.
pub const OBJECT_MATERIAL: usize = 1;

/// How rough every surface in this court is.
///
/// One value for both rows, and high: a tight highlight would put a specular
/// gradient across exactly the surfaces the occlusion is measured on.
const ROUGHNESS: f32 = 0.9;

/// [`SHELL_MATERIAL`]'s base colour.
///
/// Not `1.0`, for `apps/lantern`'s reason: a surface that reflected every photon
/// it received clips the tonemap's top end across the whole floor.
pub(crate) const SHELL_COLOR: [f32; 4] = [0.82, 0.81, 0.79, 1.0];

/// [`OBJECT_MATERIAL`]'s base colour.
pub(crate) const OBJECT_COLOR: [f32; 4] = [0.78, 0.77, 0.75, 1.0];

// ---------------------------------------------------------------------------
// The meshes
// ---------------------------------------------------------------------------

/// Where each resident mesh is in [`SceneDesc::meshes`], and therefore what an
/// [`InstanceDesc::mesh`] naming it carries.
pub const FLOOR_MESH: usize = 0;
/// The far wall (`-z`), on [`FLOOR_MESH`]'s terms.
pub const FAR_WALL_MESH: usize = 1;
/// The near wall (`+z`), behind the fixed camera.
pub const NEAR_WALL_MESH: usize = 2;
/// The `-x` wall.
pub const WEST_WALL_MESH: usize = 3;
/// The `+x` wall.
pub const EAST_WALL_MESH: usize = 4;
/// The alcove block, recess and all.
pub const ALCOVE_MESH: usize = 5;
/// The flight of cantilevered treads.
pub const STAIR_MESH: usize = 6;
/// The slot's two walls.
pub const SLOT_MESH: usize = 7;
/// The large box.
pub const BOX_MESH: usize = 8;
/// The post beside it.
pub const POST_MESH: usize = 9;
/// The low box.
pub const LOW_BOX_MESH: usize = 10;
/// The pedestal.
pub const PEDESTAL_MESH: usize = 11;
/// The sphere.
pub const SPHERE_MESH: usize = 12;

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
        self.quad_shaded(corners, [normal; 4]);
    }

    /// [`MeshBuilder::quad`] with a normal per corner.
    ///
    /// The one thing a curved surface needs that a box does not: a sphere's
    /// facets share their corners' directions with their neighbours, and a face
    /// normal repeated four times would draw a polyhedron rather than the ball
    /// `docs/plan/sample/19-alcove.md` asks for.
    fn quad_shaded(&mut self, corners: [Vec3; 4], normals: [Vec3; 4]) {
        Self::facing_its_normals(&[corners[0], corners[1], corners[2]], &normals);
        let base = self.push_corners(&corners, &normals);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Refuses a face whose winding disagrees with the normals it claims.
    ///
    /// **The contract every builder above states, enforced rather than
    /// trusted**: a face is counter-clockwise seen from its normals' side, which
    /// is what `CullMode::Back` culls the other side of. A face wound the wrong
    /// way carries a plausible normal, lights plausibly, and is simply not
    /// there from outside — the slot's walls drew that way for a day before a
    /// person looked at them from above. So the geometric normal of the first
    /// triangle is held to the same hemisphere as every authored normal, and a
    /// court that disagrees does not build.
    ///
    /// # Panics
    ///
    /// If any of `normals` points away from the side `corners` wind
    /// counter-clockwise from.
    fn facing_its_normals(corners: &[Vec3; 3], normals: &[Vec3]) {
        let geometric = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        for normal in normals {
            assert!(
                geometric.dot(*normal) > 0.0,
                "a face at {corners:?} winds clockwise seen from its normal {normal:?}, so it \
                 would be culled from the side it claims to face"
            );
        }
    }

    /// Appends one triangle, counter-clockwise seen from its normals' side.
    ///
    /// The sphere's two pole rings and nothing else: a quad there would have two
    /// coincident corners and a zero-area triangle in it.
    fn tri(&mut self, corners: [Vec3; 3], normals: [Vec3; 3]) {
        Self::facing_its_normals(&corners, &normals);
        let base = self.push_corners(&corners, &normals);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Pushes the corners and returns the index the first one landed at.
    fn push_corners(&mut self, corners: &[Vec3], normals: &[Vec3]) -> u32 {
        let base = u32::try_from(self.vertices.len())
            .unwrap_or_else(|_| unreachable!("a court of a few thousand vertices"));
        for (corner, normal) in corners.iter().zip(normals) {
            self.positions.push([corner.x, corner.y, corner.z]);
            self.vertices.push(RawVertex {
                position: [corner.x, corner.y, corner.z],
                normal: [normal.x, normal.y, normal.z],
            });
        }
        base
    }

    /// A closed box between `min` and `max`, every face pointing **out**.
    fn box_outward(&mut self, min: Vec3, max: Vec3) {
        self.box_frame(
            (min + max) * 0.5,
            [Vec3::X, Vec3::Y, Vec3::Z],
            (max - min) * 0.5,
        );
    }

    /// A box of `half` extents about `centre`, turned so its local `+x` runs
    /// along `along` and its `+y` stays up.
    ///
    /// The one thing in this court that is not axis-aligned, and it has to be:
    /// the slot's walls run along [`slot_axis`], which is the sun's own azimuth
    /// and therefore not a coordinate axis.
    fn box_along(&mut self, centre: Vec3, along: Vec3, half: Vec3) {
        let forward = along.normalize();
        let up = Vec3::Y;
        let side = forward.cross(up).normalize();
        self.box_frame(centre, [forward, up, side], half);
    }

    /// A closed box about `centre` in the right-handed frame `axes`, `half`
    /// along each axis, every face pointing **out**.
    ///
    /// **One corner order for both boxes.** [`box_outward`](Self::box_outward)
    /// and [`box_along`](Self::box_along) used to spell their six faces
    /// separately, and the turned copy had four of them wound the other way —
    /// the slot's walls were there from inside and culled from outside, which
    /// is what a person looking down into the slot reported on 2026-09-04.
    /// Each face below is the axis-aligned one's order with `x`, `y` and `z`
    /// read as the frame's three axes, and
    /// [`facing_its_normals`](Self::facing_its_normals) is what now refuses a
    /// transcription that disagrees with itself.
    fn box_frame(&mut self, centre: Vec3, axes: [Vec3; 3], half: Vec3) {
        let [x, y, z] = axes;
        let at = |sx: f32, sy: f32, sz: f32| {
            centre + x * (half.x * sx) + y * (half.y * sy) + z * (half.z * sz)
        };
        // Each face counter-clockwise seen from outside: `+x`, `-x`, `+y`,
        // `-y`, `+z`, `-z`.
        self.quad(
            [
                at(1.0, -1.0, 1.0),
                at(1.0, -1.0, -1.0),
                at(1.0, 1.0, -1.0),
                at(1.0, 1.0, 1.0),
            ],
            x,
        );
        self.quad(
            [
                at(-1.0, -1.0, -1.0),
                at(-1.0, -1.0, 1.0),
                at(-1.0, 1.0, 1.0),
                at(-1.0, 1.0, -1.0),
            ],
            -x,
        );
        self.quad(
            [
                at(-1.0, 1.0, 1.0),
                at(1.0, 1.0, 1.0),
                at(1.0, 1.0, -1.0),
                at(-1.0, 1.0, -1.0),
            ],
            y,
        );
        self.quad(
            [
                at(-1.0, -1.0, -1.0),
                at(1.0, -1.0, -1.0),
                at(1.0, -1.0, 1.0),
                at(-1.0, -1.0, 1.0),
            ],
            -y,
        );
        self.quad(
            [
                at(-1.0, -1.0, 1.0),
                at(1.0, -1.0, 1.0),
                at(1.0, 1.0, 1.0),
                at(-1.0, 1.0, 1.0),
            ],
            z,
        );
        self.quad(
            [
                at(1.0, -1.0, -1.0),
                at(-1.0, -1.0, -1.0),
                at(-1.0, 1.0, -1.0),
                at(1.0, 1.0, -1.0),
            ],
            -z,
        );
    }

    /// A sphere of `radius` about `centre`, with smooth normals.
    ///
    /// Quads between the rings and triangles at the two poles, where a quad
    /// would have two coincident corners. `rings` counts the bands from pole to
    /// pole and `segments` the sweeps around the axis.
    fn sphere(&mut self, centre: Vec3, radius: f32, segments: usize, rings: usize) {
        let point = |segment: usize, ring: usize| {
            #[allow(clippy::cast_precision_loss)]
            let theta = core::f32::consts::TAU * segment as f32 / segments as f32;
            #[allow(clippy::cast_precision_loss)]
            let phi = core::f32::consts::PI * ring as f32 / rings as f32;
            let direction = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            (centre + direction * radius, direction)
        };
        for segment in 0..segments {
            let next = segment + 1;
            for ring in 0..rings {
                let (a, an) = point(segment, ring);
                let (b, bn) = point(next, ring);
                let (c, cn) = point(next, ring + 1);
                let (d, dn) = point(segment, ring + 1);
                if ring == 0 {
                    self.tri([a, c, d], [an, cn, dn]);
                } else if ring + 1 == rings {
                    self.tri([a, b, c], [an, bn, cn]);
                } else {
                    self.quad_shaded([a, b, c, d], [an, bn, cn, dn]);
                }
            }
        }
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
        // No surface here samples a page at all — every material row names
        // `GpuMaterial::NO_PAGE` — so every vertex carries the same texture
        // coordinate and the range is degenerate on purpose.
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
                // so the court has no authored tangent to claim.
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

/// A slab of the shell between two corners, as its own mesh.
fn slab(label: &'static str, min: Vec3, max: Vec3) -> MeshDesc<'static> {
    mesh_of(label, |builder| builder.box_outward(min, max))
}

/// The alcove block: five slabs around a recess whose mouth faces `+z`.
fn alcove(builder: &mut MeshBuilder) {
    let mouth_z = ALCOVE_MAX.z;
    let back_z = mouth_z - ALCOVE_RECESS;
    // Behind the recess: the whole footprint, floor to head.
    builder.box_outward(ALCOVE_MIN, Vec3::new(ALCOVE_MAX.x, ALCOVE_MAX.y, back_z));
    // The sill and the head, spanning the mouth's own width.
    let mouth = (ALCOVE_MOUTH_X.0, ALCOVE_MOUTH_X.1);
    builder.box_outward(
        Vec3::new(mouth.0, ALCOVE_MIN.y, back_z),
        Vec3::new(mouth.1, ALCOVE_MOUTH_Y.0, mouth_z),
    );
    builder.box_outward(
        Vec3::new(mouth.0, ALCOVE_MOUTH_Y.1, back_z),
        Vec3::new(mouth.1, ALCOVE_MAX.y, mouth_z),
    );
    // The two jambs, between sill and head.
    builder.box_outward(
        Vec3::new(ALCOVE_MIN.x, ALCOVE_MOUTH_Y.0, back_z),
        Vec3::new(mouth.0, ALCOVE_MOUTH_Y.1, mouth_z),
    );
    builder.box_outward(
        Vec3::new(mouth.1, ALCOVE_MOUTH_Y.0, back_z),
        Vec3::new(ALCOVE_MAX.x, ALCOVE_MOUTH_Y.1, mouth_z),
    );
}

/// The flight: [`STAIR_TREADS`] slabs cantilevered from the far wall.
fn stair(builder: &mut MeshBuilder) {
    for tread in 0..STAIR_TREADS {
        #[allow(clippy::cast_precision_loss)]
        let step = tread as f32;
        let x0 = STAIR_X0 + step * STAIR_RUN;
        let y0 = step * STAIR_RISE;
        builder.box_outward(
            Vec3::new(x0, y0, -HALF_DEPTH),
            Vec3::new(
                x0 + STAIR_RUN,
                y0 + STAIR_THICKNESS,
                -HALF_DEPTH + STAIR_NOSING,
            ),
        );
    }
}

/// The slot: two walls either side of [`slot_axis`], `SLOT_GAP` apart.
fn slot(builder: &mut MeshBuilder) {
    let axis = slot_axis();
    let centre = SLOT_NEAR + axis * (SLOT_LENGTH * 0.5);
    let side = axis.cross(Vec3::Y).normalize();
    let offset = SLOT_GAP * 0.5 + SLOT_WALL_THICKNESS * 0.5;
    let half = Vec3::new(
        SLOT_LENGTH * 0.5,
        SLOT_WALL_HEIGHT * 0.5,
        SLOT_WALL_THICKNESS * 0.5,
    );
    for sign in [-1.0f32, 1.0] {
        builder.box_along(
            centre + side * (offset * sign) + Vec3::Y * (SLOT_WALL_HEIGHT * 0.5),
            axis,
            half,
        );
    }
}

// ---------------------------------------------------------------------------
// The description
// ---------------------------------------------------------------------------

/// Everything the court makes resident: every mesh, both material rows and the
/// one-texel page.
///
/// The mesh order is [`FLOOR_MESH`] through [`SPHERE_MESH`] and the row order is
/// [`SHELL_MATERIAL`] then [`OBJECT_MATERIAL`]; both are load-bearing, and the
/// constants above are how an instance names one.
#[must_use]
pub fn court() -> SceneDesc<'static> {
    let (west, east) = (-HALF_WIDTH, HALF_WIDTH);
    let (far, near) = (-HALF_DEPTH, HALF_DEPTH);
    let outer = (west - SHELL, east + SHELL);

    SceneDesc {
        meshes: vec![
            slab(
                "floor",
                Vec3::new(west, -SHELL, far),
                Vec3::new(east, 0.0, near),
            ),
            slab(
                "far wall",
                Vec3::new(outer.0, 0.0, far - SHELL),
                Vec3::new(outer.1, WALL_HEIGHT, far),
            ),
            slab(
                "near wall",
                Vec3::new(outer.0, 0.0, near),
                Vec3::new(outer.1, WALL_HEIGHT, near + SHELL),
            ),
            slab(
                "west wall",
                Vec3::new(west - SHELL, 0.0, far),
                Vec3::new(west, WALL_HEIGHT, near),
            ),
            slab(
                "east wall",
                Vec3::new(east, 0.0, far),
                Vec3::new(east + SHELL, WALL_HEIGHT, near),
            ),
            mesh_of("alcove", alcove),
            mesh_of("stair", stair),
            mesh_of("slot", slot),
            mesh_of("box", |builder| builder.box_outward(BOX_MIN, BOX_MAX)),
            mesh_of("post", |builder| builder.box_outward(POST_MIN, POST_MAX)),
            mesh_of("low box", |builder| {
                builder.box_outward(LOW_BOX_MIN, LOW_BOX_MAX);
            }),
            mesh_of("pedestal", |builder| {
                builder.box_outward(PEDESTAL_MIN, PEDESTAL_MAX);
            }),
            mesh_of("sphere", |builder| {
                builder.sphere(SPHERE_CENTRE, SPHERE_RADIUS, SPHERE_SEGMENTS, SPHERE_RINGS);
            }),
        ],
        materials: vec![
            GpuMaterial {
                base_color: SHELL_COLOR,
                base_color_texture: GpuMaterial::NO_PAGE,
                roughness: ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color: OBJECT_COLOR,
                base_color_texture: GpuMaterial::NO_PAGE,
                roughness: ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
        ],
        page: PageDesc::empty(),
        probes: crcbl::render::ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// How much of each pool [`court`] reserves.
///
/// Every one of these is device-local memory taken at start-up and never grown,
/// so they are the court's ceiling and not its size:
/// `the_court_fits_the_capacities_it_reserves` is what checks the description
/// against them with no GPU in the room.
pub const CAPACITIES: Capacities = Capacities {
    vertices: 4 * 1024,
    indices: 8 * 1024,
    meshes: 16,
    instances: 32,
    materials: 4,
    lights: 4,
    // No irradiance volume. This is the one sample whose subject is the ambient
    // term itself, and a probe grid added to it is a second source of indirect
    // light in every block the occlusion claims are read from.
    probes: 0,
};

/// Which mesh each object in the court is and which row it shades through, in
/// the order [`place`] inserts them.
///
/// **Insertion order is the caller's and it is load-bearing**, on
/// `crcbl::render::scene`'s terms: the slot an object lands in is
/// `docs/plan/25-lod.md`'s hysteresis key, so a golden compared across two runs
/// needs the two runs to have placed things in the same order.
const OBJECTS: [(usize, usize); 13] = [
    (FLOOR_MESH, SHELL_MATERIAL),
    (FAR_WALL_MESH, SHELL_MATERIAL),
    (NEAR_WALL_MESH, SHELL_MATERIAL),
    (WEST_WALL_MESH, SHELL_MATERIAL),
    (EAST_WALL_MESH, SHELL_MATERIAL),
    (ALCOVE_MESH, SHELL_MATERIAL),
    (STAIR_MESH, SHELL_MATERIAL),
    (SLOT_MESH, SHELL_MATERIAL),
    (BOX_MESH, OBJECT_MATERIAL),
    (POST_MESH, OBJECT_MATERIAL),
    (LOW_BOX_MESH, OBJECT_MATERIAL),
    (PEDESTAL_MESH, OBJECT_MATERIAL),
    (SPHERE_MESH, OBJECT_MATERIAL),
];

/// Puts every object in `renderer`, and reports how many.
///
/// The geometry is already at world scale and world position — every mesh above
/// is built where it stands — so each transform is the identity.
///
/// # Errors
///
/// [`InstancePoolError::PoolFull`] if [`CAPACITIES`]'s instance count does not
/// cover the court, which is a mistake in this file rather than a condition a
/// run can be in — but it is the caller that would have to report it, so it is
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
// The camera
// ---------------------------------------------------------------------------

/// Where the fixed camera stands.
///
/// **On [`slot_axis`]' own line**, extended back from `SLOT_NEAR` — see this
/// module's header. `the_fixed_camera_stands_on_the_slots_axis` is what holds it
/// there, so moving the sun moves the camera rather than leaving the crease
/// claim looking into a wall.
#[must_use]
pub fn fixed_eye() -> Vec3 {
    SLOT_NEAR - slot_axis() * EYE_BEHIND_SLOT + Vec3::Y * EYE_HEIGHT
}

/// How far back along [`slot_axis`] the eye stands from `SLOT_NEAR`.
const EYE_BEHIND_SLOT: f32 = 1.5;

/// How high the eye stands, in metres.
///
/// Under the top three treads of the stair, which is what puts their undersides
/// in the frame.
const EYE_HEIGHT: f32 = 1.05;

/// How far ahead the camera looks, along [`slot_axis`].
const TARGET_AHEAD: f32 = 3.2;

/// How far the target sits below [`EYE_HEIGHT`].
///
/// The whole of the camera's downward tilt. Enough that the slot's floor is well
/// inside the frame rather than at its bottom edge, and little enough that the
/// far wall still fills the top of it.
const TARGET_DROP: f32 = 1.0;

/// The vertical field of view, in radians.
const FOV_Y: f32 = 65.0 * core::f32::consts::PI / 180.0;

/// The pose every golden is taken from, and the one the free camera starts at.
#[must_use]
pub fn fixed_camera() -> Camera {
    let eye = fixed_eye();
    Camera {
        eye,
        target: eye + slot_axis() * TARGET_AHEAD - Vec3::Y * TARGET_DROP,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            // Close, because the camera stands inside the court and the near
            // wall is under two metres behind it.
            near: 0.02,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The description fits the pools it reserves**, with no GPU in the room.
    ///
    /// [`CAPACITIES`] is device-local memory taken once and never grown, and a
    /// court that outgrew one of them would fail at start-up on a device rather
    /// than here. The instance count is the one [`place`] would trip over; the
    /// rest are what `ForwardRenderer::with_scene` uploads into.
    #[test]
    fn the_court_fits_the_capacities_it_reserves() {
        let scene = court();
        let (mut vertices, mut indices) = (0usize, 0usize);
        for mesh in &scene.meshes {
            let Geometry::Flat {
                vertices: bytes,
                indices: ids,
                ..
            } = &mesh.geometry
            else {
                panic!(
                    "the court describes {:?} as something other than a flat mesh",
                    mesh.label
                )
            };
            vertices += bytes.len() / mesh::VERTEX_STRIDE;
            indices += ids.len();
        }
        assert!(
            vertices <= CAPACITIES.vertices as usize,
            "the court has {vertices} vertices and reserves {}",
            CAPACITIES.vertices
        );
        assert!(
            indices <= CAPACITIES.indices as usize,
            "the court has {indices} indices and reserves {}",
            CAPACITIES.indices
        );
        assert!(
            scene.meshes.len() <= CAPACITIES.meshes as usize,
            "the court has {} meshes and reserves {}",
            scene.meshes.len(),
            CAPACITIES.meshes
        );
        assert!(
            scene.materials.len() <= CAPACITIES.materials as usize,
            "the court has {} material rows and reserves {}",
            scene.materials.len(),
            CAPACITIES.materials
        );
        assert!(
            OBJECTS.len() <= CAPACITIES.instances as usize,
            "the court places {} instances and reserves {}",
            OBJECTS.len(),
            CAPACITIES.instances
        );
    }

    /// **Every mesh id names a mesh, and every object shades through a row that
    /// exists.**
    ///
    /// The ids are indices into two vectors written by hand in a different part
    /// of this file, and an id one past the end is a plausible-looking scene
    /// missing a wall — or, worse, one shading a wall through the object row.
    #[test]
    fn every_object_names_a_mesh_and_a_material_that_exist() {
        let scene = court();
        assert_eq!(
            scene.meshes.len(),
            SPHERE_MESH + 1,
            "the mesh ids run to {SPHERE_MESH} and the court describes {} meshes",
            scene.meshes.len()
        );
        for (mesh, material) in OBJECTS {
            assert!(
                mesh < scene.meshes.len(),
                "an object names mesh {mesh} and the court has {}",
                scene.meshes.len()
            );
            assert!(
                material < scene.materials.len(),
                "an object shades through row {material} and the court has {}",
                scene.materials.len()
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

    /// **Every mesh is placed.**
    ///
    /// A mesh described and never inserted costs the memory and draws nothing,
    /// and the frame it leaves is a court with one object quietly missing —
    /// which is exactly the failure the goldens cannot distinguish from a
    /// deliberate scene.
    #[test]
    fn every_mesh_the_court_describes_is_placed() {
        let placed: Vec<usize> = OBJECTS.iter().map(|(mesh, _)| *mesh).collect();
        for id in 0..=SPHERE_MESH {
            assert!(
                placed.contains(&id),
                "mesh {id} is described and not placed"
            );
        }
    }

    /// Every vertex of one of the court's meshes, in world space.
    ///
    /// The meshes are described as `MeshVertex` bytes, so this is the only way
    /// a test can ask where a wall actually stands rather than where the
    /// constant that ought to have placed it says.
    fn mesh_positions(scene: &SceneDesc<'static>, mesh: usize) -> Vec<Vec3> {
        let Geometry::Flat { vertices, .. } = &scene.meshes[mesh].geometry else {
            panic!("mesh {mesh} is not a flat mesh")
        };
        vertices
            .chunks_exact(mesh::VERTEX_STRIDE)
            .map(|chunk| {
                let bytes: &[u8; mesh::VERTEX_STRIDE] = chunk
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("exact chunk"));
                let at = MeshVertex::from_bytes(bytes).position;
                Vec3::new(at[0], at[1], at[2])
            })
            .collect()
    }

    /// **The slot is built along the line the sun and the camera share.**
    ///
    /// This module's header rests the whole crease claim on the three being
    /// parallel, and two thirds of that is guaranteed by construction:
    /// [`slot_axis`] is derived from `SUN_TOWARDS`, and [`fixed_camera`] aims
    /// along [`slot_axis`], so no edit can pull them apart. **The walls are
    /// not.** They are triangles, placed by a builder that takes a direction as
    /// an argument, and a slot laid along `+z` instead would leave every
    /// derived point exactly where it is while the sun crossed it at an angle
    /// and shadowed the crease.
    ///
    /// So this reads the slot mesh back and asks the geometry: no vertex is
    /// further off the axis than the gap and the wall thickness allow, and
    /// there are vertices on **both** sides of the crease sample, which is what
    /// makes it a crease rather than a spot beside a wall.
    #[test]
    fn the_slot_is_built_along_the_axis_the_sun_and_camera_share() {
        let scene = court();
        let axis = slot_axis();
        let sideways = axis.cross(Vec3::Y).normalize();
        let widest = SLOT_GAP / 2.0 + SLOT_WALL_THICKNESS;

        let crease = crease_lit();
        let (mut left, mut right) = (false, false);
        for at in mesh_positions(&scene, SLOT_MESH) {
            let off = (at - crease).dot(sideways);
            assert!(
                off.abs() <= widest + 1e-3,
                "a slot vertex at {at:?} stands {off:.4} off the axis, and the walls reach only \
                 {widest:.4} — the slot is not built along {axis:?}"
            );
            let along = (at - SLOT_NEAR).dot(axis);
            assert!(
                (-1e-3..=SLOT_LENGTH + 1e-3).contains(&along),
                "a slot vertex at {at:?} is {along:.4} along the axis, outside the slot's own \
                 {SLOT_LENGTH}"
            );
            if off < -SLOT_GAP / 4.0 {
                left = true;
            }
            if off > SLOT_GAP / 4.0 {
                right = true;
            }
        }
        assert!(
            left && right,
            "the slot has walls on only one side of the crease sample, so it is not a crease"
        );
    }

    /// **The crease sample is floor, inside the court, with the sun above it.**
    ///
    /// What the slot test above cannot say. Its lateral position is guaranteed
    /// by construction — [`crease_lit`] is [`slot_axis`] stepped from
    /// `SLOT_NEAR`, so it is on the centre line whatever the constants say —
    /// and these three are not: `SLOT_NEAR` lifted off the floor, a slot pushed
    /// through a wall, or a sun rotated under the horizon each leave a scene
    /// that draws and a claim that means something else.
    #[test]
    fn the_crease_sample_is_sunlit_floor_inside_the_court() {
        let crease = crease_lit();
        assert!(
            crease.y.abs() < 1e-6,
            "the crease sample is at y={}, off the floor it is meant to read",
            crease.y
        );
        assert!(
            crease.x.abs() < HALF_WIDTH && crease.z.abs() < HALF_DEPTH,
            "the crease sample at {crease:?} is outside the court's own walls"
        );
        assert!(
            sun().direction.y > 0.0,
            "the sun points at {:?}, which is below the horizon, so nothing in the court is \
             sunlit and the crease claim is about two shaded readings",
            sun().direction
        );
    }

    /// **The stair climbs past the eye.**
    ///
    /// [`STAIR_TREADS`]'s doc says the upper treads' undersides are in the
    /// frame, which is the crease the charter asks a stair for. A flight one
    /// tread shorter, or a rise one notch smaller, would put every tread below
    /// the eye and leave the undersides out of the picture without changing
    /// anything a golden could name.
    #[test]
    fn the_stair_climbs_past_the_eye() {
        #[allow(clippy::cast_precision_loss)]
        let top = (STAIR_TREADS - 1) as f32 * STAIR_RISE;
        assert!(
            top > fixed_eye().y,
            "the top tread's soffit is at y={top} and the eye is at y={}, so no underside is in \
             the frame",
            fixed_eye().y
        );
    }

    /// **The rim blocks land on the far wall, on either side of the sphere.**
    ///
    /// [`rim_outside`] and [`rim_far`] are the two blocks the halo bound is a
    /// ratio of, and both have to be flat far wall: one of them landing on the
    /// sphere, on the pedestal or past the wall's edge would turn that ratio
    /// into a comparison of two different surfaces. [`rim_inside`] is the
    /// opposite — it has to be *on* the sphere.
    #[test]
    fn the_rim_points_stand_on_the_far_wall() {
        for (name, point) in [("outside", rim_outside()), ("far", rim_far())] {
            assert!(
                (point.z + HALF_DEPTH).abs() < 1e-4,
                "the {name} rim block is at z={} and the far wall is at z={}",
                point.z,
                -HALF_DEPTH
            );
            assert!(
                point.x.abs() < HALF_WIDTH && point.y > 0.0 && point.y < WALL_HEIGHT,
                "the {name} rim block is at {point:?}, off the far wall's face"
            );
            assert!(
                (point - SPHERE_CENTRE).length() > SPHERE_RADIUS,
                "the {name} rim block is inside the sphere"
            );
        }
        assert!(
            (rim_inside() - SPHERE_CENTRE).length() < SPHERE_RADIUS,
            "the inside rim block is at {:?}, which is not on the sphere",
            rim_inside()
        );
    }

    /// **Nothing stands within reach of the control point.**
    ///
    /// [`OPEN_FLOOR`] is what separates "the corner got darker" from "the frame
    /// got darker", and it can only do that if the occlusion channel there is
    /// one. The bound is a clearance in metres rather than a claim about the
    /// gather: it is comfortably over the shipped `r_ssao_radius`, and what the
    /// reading actually is at every radius the sample sweeps is measured on a
    /// device by `the_court_darkens_where_it_is_enclosed`, which holds the two
    /// frames to the same number rather than to a tolerance.
    ///
    /// This test is the geometric half — that the point did not drift toward
    /// something while the court was being laid out — and it is checkable with
    /// no GPU in the room.
    #[test]
    fn the_open_floor_stands_clear_of_everything() {
        /// Metres of clear space the control point wants around it.
        ///
        /// Over twice the shipped occlusion radius, so a nudge to either does
        /// not quietly turn the control into a second reading.
        const CLEARANCE: f32 = 0.7;

        let boxes = [
            (BOX_MIN, BOX_MAX),
            (POST_MIN, POST_MAX),
            (LOW_BOX_MIN, LOW_BOX_MAX),
            (PEDESTAL_MIN, PEDESTAL_MAX),
            (ALCOVE_MIN, ALCOVE_MAX),
        ];
        for (min, max) in boxes {
            let nearest = Vec3::new(
                OPEN_FLOOR.x.clamp(min.x, max.x),
                OPEN_FLOOR.y.clamp(min.y, max.y),
                OPEN_FLOOR.z.clamp(min.z, max.z),
            );
            let distance = (nearest - OPEN_FLOOR).length();
            assert!(
                distance > CLEARANCE,
                "the open floor is {distance:.3} from the box between {min:?} and {max:?}, \
                 inside the {CLEARANCE} of clear space the control wants"
            );
        }
    }
}
