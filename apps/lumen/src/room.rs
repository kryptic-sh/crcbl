//! The room, as data an application hands the engine.
//!
//! ```text
//!  MeshBuilder ──▶ build_meshlets ──▶ Geometry::Flat ──┐
//!  GpuMaterial rows ───────────────────────────────────┼─▶ SceneDesc ──▶ with_scene
//!  PageDesc (white + checker) ─────────────────────────┘
//!  place() ──▶ add_instance ×N
//! ```
//!
//! Nothing here names a device, and nothing here is the engine's content: the
//! meshes are baked from literals by this module's own quad builder, the
//! material rows are this sample's, and the page is built through
//! [`PageDesc::opaque_white`](crcbl::render::scene::PageDesc::opaque_white) and
//! `push_layer` like any other caller's. That is the whole of what
//! `crcbl::render::scene` bought, and this module is the first application-side
//! use of it.
//!
//! # The plan
//!
//! `docs/plan/sample/13-lumen.md`'s Scope asks for "a room with a window, a
//! mirror-grade surface, a rough metal surface, a coloured bounce wall, and a
//! moving light". Each of those is one thing below, and each is placed where the
//! [`fixed_camera`] can see it at once.
//!
//! # Two of them do not look like their names yet, and that is the model
//!
//! **The mirror-grade panel and the rough metal block render near-black**, and a
//! reader who did not expect that would call it a bug. It is not.
//! [`GpuMaterial::metallic`](crcbl::shaders::mesh::GpuMaterial::metallic) scales
//! the diffuse albedo *down* — a conductor has no diffuse lobe — and the ambient
//! term multiplies that same diffuse albedo, so a fully metallic surface out of
//! every light's specular reach has nothing left to shade with. What fills it in
//! is a reflection.
//!
//! Some of that has landed. `docs/plan/18-render-features.md`'s screen-space
//! reflections now light **the foot of the mirror panel**, and only the foot: the
//! panel faces the camera, so a ray leaving it goes back past the eye and only
//! the lowest part of the face sends one that reaches the floor while still on
//! screen — see [`MIRROR_FOOT`]. The rough metal block is untouched, because its
//! roughness is above the cutoff a single ray is honest at
//! ([`crcbl::shaders::ssr::ROUGHNESS_CUTOFF`]); the blur that lowers it and the
//! irradiance probes that would fill in a miss are both unbuilt. The debug panel
//! says so on a row of its own, and `docs/backlog.md` carries it as owed work.
//!
//! **The coloured wall does not bounce**, for the neighbouring reason: bounced
//! light is global illumination, and there is none. It is a coloured wall that
//! takes the sun; what it will look like when it bounces is what milestone 3
//! will show.
//!
//! Neither is faked. A fixture whose job is showing what the renderer does must
//! not flatter it.

use std::borrow::Cow;

use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    Camera, Capacities, DirectionalLight, ForwardRenderer, Geometry, InstanceDesc,
    InstancePoolError, Light, MeshDesc, PageDesc, PointLight, Projection, SceneDesc,
};
use crcbl::shaders::mesh::{self, GpuMaterial, MeshVertex};

// ---------------------------------------------------------------------------
// The room's dimensions
// ---------------------------------------------------------------------------

/// Half the room's width, in metres. The walls stand at `±HALF_WIDTH`.
pub const HALF_WIDTH: f32 = 3.0;

/// Half the room's depth. The back wall is at `-HALF_DEPTH` and the front wall,
/// which the fixed camera stands just inside, at `+HALF_DEPTH`.
///
/// Deeper than it is wide so the fixed camera has somewhere to stand and still
/// see the far corner: the AO measurement wants a three-surface corner in frame
/// at a plausible distance, which a cubical room seen from inside cannot give.
pub const HALF_DEPTH: f32 = 4.0;

/// Floor to ceiling, in metres. An ordinary room, so that
/// [`crcbl::render`]'s occlusion radius — half a metre — is a plausible
/// fraction of it rather than a fraction of a toy.
pub const HEIGHT: f32 = 3.0;

/// How high the window's sill is off the floor.
pub const WINDOW_SILL: f32 = 0.9;

/// How high its head is. Above eye height, so the sun comes in over the
/// camera rather than into it.
pub const WINDOW_HEAD: f32 = 2.4;

/// Half the window's run along `z`.
pub const WINDOW_HALF: f32 = 1.7;

/// How thick the room's shell is, in metres.
///
/// **A room built of slabs rather than of single quads, and the sun is why.**
/// Back faces are culled in the shadow pass as well as in the colour one, so a
/// wall that is one inward-facing quad is a wall the *sun* cannot see: it writes
/// nothing into the shadow map and casts no shadow at all. The first frame of
/// this room was exactly that — an evenly lit floor with no shaft of light on it
/// and a window that did nothing to it.
///
/// So every surface of the shell is a slab, with an outward face for the light
/// and an inward one for the camera. It also gives the window a reveal, which is
/// what a hole in a wall of some thickness actually looks like.
pub const SHELL: f32 = 0.15;

/// How far off the floor a standing person's eye is, and where [`fixed_camera`]
/// puts one.
///
/// **The number the AO tuning has never had.** `Scene::Ao` looks straight down
/// into a trough from above it, so no kernel in that frame ever straddles a
/// surface receding from the eye. Every kernel in this one does.
pub const EYE_HEIGHT: f32 = 1.6;

// ---------------------------------------------------------------------------
// Where the objects stand
// ---------------------------------------------------------------------------

/// The plinth the mirror panel stands on: minimum corner.
const PLINTH_MIN: Vec3 = Vec3::new(-2.0, 0.0, -3.2);
/// The plinth's maximum corner.
///
/// A diffuse box on the floor against the back wall, so the floor, the plinth's
/// side and the back wall meet in a **three-surface corner** the occlusion pass
/// has something to say about — and so the panel above it has a contact edge
/// rather than floating.
const PLINTH_MAX: Vec3 = Vec3::new(-0.4, 0.45, -2.4);

/// The mirror-grade panel: minimum corner.
///
/// A slab rather than a quad, so it has a silhouette against the wall behind it
/// and an edge the depth-aware blur has two depths to weigh across.
const MIRROR_MIN: Vec3 = Vec3::new(-1.85, 0.45, -2.95);
/// The mirror panel's maximum corner.
const MIRROR_MAX: Vec3 = Vec3::new(-0.55, 2.25, -2.83);

/// The plane the mirror panel's `+Z` face lies in.
///
/// Public because it is what a claim about that face has to be aimed at, and a
/// test spelling the number for itself would be a second copy of the panel's
/// own corner.
pub const MIRROR_FACE_Z: f32 = MIRROR_MAX.z;

/// The middle of the mirror panel's `+Z` face, left to right.
const MIRROR_MID_X: f32 = 0.5 * (MIRROR_MIN.x + MIRROR_MAX.x);

/// The middle of the panel's **bottom edge**, where the golden suite reads the
/// reflection.
///
/// **The panel is close to screen-space reflection's worst case, and this is the
/// only part of it that is not.** The face looks straight at the camera, so a ray
/// leaving it goes back past the eye: from a point above eye height the ray rises
/// and finds the ceiling behind the camera, and from a point below it the ray
/// falls towards the floor — but only from the lowest part of the face does it
/// reach the floor while still *inside the frame*, which is the only place a
/// screen-space march can find anything at all.
///
/// So this is the bottom edge itself, and
/// `apps/lumen/tests/golden.rs` hangs its block directly above it rather than
/// centring one here. `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head`
/// is what says the band is real, with no GPU: it computes the height at which a
/// reflected ray stops landing in frame and checks this point is below it and
/// [`MIRROR_MISSES`] above it.
pub const MIRROR_FOOT: Vec3 = Vec3::new(MIRROR_MID_X, MIRROR_MIN.y, MIRROR_FACE_Z);

/// A point on the **same face** whose reflected ray finds nothing.
///
/// Same material row, same normal, same `F0`, same roughness, same absence of
/// direct light as [`MIRROR_FOOT`] — the two differ in one thing only, which is
/// whether the ray they send finds the floor while it is still on screen. That is
/// what makes the pair a claim about the reflection rather than about the panel.
pub const MIRROR_MISSES: Vec3 = Vec3::new(MIRROR_MID_X, 1.5, MIRROR_FACE_Z);

/// A floor point **inside** the shaft of sunlight the window throws.
///
/// Public because the golden suite's first claim is a ratio between this and
/// [`SHADED_FLOOR`], and a point that stopped being lit would quietly turn that
/// claim into a comparison of two shadows.
/// `the_two_floor_samples_differ_in_the_sun_and_in_nothing_else` is what checks
/// it, with no GPU: the ray back from here along [`sun`]'s direction passes
/// through the opening, the ray back from the other one does not, and neither is
/// within [`lamp`]'s reach at `t = 0`.
pub const SUNLIT_FLOOR: Vec3 = Vec3::new(2.6, 0.0, -2.6);

/// A floor point **outside** the shaft, in the wall's own shadow.
///
/// Same surface, same normal, same material, and out of the lamp's reach — so
/// the whole of what separates it from [`SUNLIT_FLOOR`] is the sun's direct
/// contribution.
///
/// **Out in the open rather than against the wall it is shadowed by**, which is
/// not decoration: the sun's shadow peter-pans at the wall's foot and leaves a
/// lit strip a metre wide there — `docs/backlog.md` records the artefact — and a
/// sample taken inside it is a measurement of the shadow bias rather than of the
/// ambient.
pub const SHADED_FLOOR: Vec3 = Vec3::new(-1.8, 0.0, -1.0);

/// The rough metal block: minimum corner.
///
/// Standing free on the open floor, between the window's light and the camera,
/// so it casts a shadow the sun draws and a contact the occlusion pass draws —
/// two different mechanisms with a visible boundary between them, which is the
/// pair a reviewer needs to tell them apart.
const BLOCK_MIN: Vec3 = Vec3::new(0.55, 0.0, -1.35);
/// The rough metal block's maximum corner.
const BLOCK_MAX: Vec3 = Vec3::new(1.45, 0.95, -0.45);

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

/// The base-colour page's extent, in texels a side.
///
/// Four, so the floor's layer is a 4×4 pattern rather than a flat colour: the
/// sampler has no mips and filters nearest, so what lands on the floor is
/// sixteen crisp cells and a texture coordinate that never varied would be
/// visible as one.
pub const PAGE_EXTENT: u32 = 4;

/// Bytes in one layer: [`PAGE_EXTENT`]² RGBA texels.
const PAGE_LAYER_BYTES: usize = (PAGE_EXTENT * PAGE_EXTENT) as usize * 4;

/// The floor's layer: a two-tone check, light on light.
///
/// Low contrast on purpose. The floor is what the window's light lands on and
/// what the occlusion bands are measured across, and a black-and-white check
/// would put a larger step across those bands than either term does — the
/// pattern would decide the measurement instead of the lighting.
const FLOOR_TEXELS: [u8; PAGE_LAYER_BYTES] = checker(0xE8, 0xC4);

/// A `PAGE_EXTENT`² RGBA check of two greys, `light` where the coordinates'
/// parity is even.
const fn checker(light: u8, dark: u8) -> [u8; PAGE_LAYER_BYTES] {
    let mut texels = [0xFFu8; PAGE_LAYER_BYTES];
    let mut index = 0;
    while index < (PAGE_EXTENT * PAGE_EXTENT) as usize {
        let x = index % PAGE_EXTENT as usize;
        let y = index / PAGE_EXTENT as usize;
        let value = if (x + y).is_multiple_of(2) {
            light
        } else {
            dark
        };
        texels[index * 4] = value;
        texels[index * 4 + 1] = value;
        texels[index * 4 + 2] = value;
        texels[index * 4 + 3] = 0xFF;
        index += 1;
    }
    texels
}

/// The page layer [`PLASTER`] and every other untextured row samples.
const UNTEXTURED_LAYER: u32 = PageDesc::UNTEXTURED_LAYER;

/// The page layer [`FLOOR`] samples — the one [`room`] appends past the white
/// one.
const FLOOR_LAYER: u32 = 1;

// ---------------------------------------------------------------------------
// The material rows
// ---------------------------------------------------------------------------

/// The walls', ceiling's and plinth's row: white plaster, and **row 0**.
///
/// Row 0 is what [`mesh::GpuInstance::default`] names, so it is what an object
/// placed without a material id shades through — see
/// [`SceneDesc::materials`]. Everything in this room names its row explicitly,
/// and the row that would be inherited by omission is still the one a reader
/// would guess.
///
/// Rough rather than [`GpuMaterial::UNTINTED`]'s half, because painted plaster
/// is: a wall with a tight highlight on it reads as polished stone and would put
/// a specular gradient across exactly the surfaces the occlusion bands are
/// measured on.
pub const PLASTER: usize = 0;

/// The floor's row: the same dielectric, sampling the page's second layer.
///
/// The only row in this scene that differs from [`PLASTER`] in its **texture**
/// and in nothing else, which is what makes the floor evidence about the page
/// rather than about a colour.
pub const FLOOR: usize = 1;

/// The coloured wall's row.
///
/// Saturated, so the light it would bounce is unmistakably its own colour when
/// there is finally a bounce to see. Today it is a coloured wall and the room
/// beside it is not tinted, which is exactly what "there is no GI" looks like.
pub const BOUNCE: usize = 2;

/// The mirror-grade panel's row: metallic, and as smooth as the lobe is worth
/// evaluating at.
///
/// **This row is the sample's honest failure**, and the module docs say why.
/// Not zero roughness: `mesh.slang` clamps it away from zero, and a lobe
/// narrower than a pixel is a highlight a rasteriser can miss entirely — so the
/// sun's reflection in it is a small bright spot rather than nothing at all,
/// which is the difference between "metal with no reflections" and "an object
/// that failed to draw".
pub const MIRROR: usize = 3;

/// The rough metal block's row: the same conductor at a roughness that spreads
/// its lobe over most of a face.
///
/// The pair with [`MIRROR`] is the point: two rows differing in **roughness
/// alone**, so the frame shows what that one column does to a conductor and
/// nothing else varies between them.
pub const ROUGH_METAL: usize = 4;

/// How rough [`PLASTER`] is.
const PLASTER_ROUGHNESS: f32 = 0.9;

/// How rough [`MIRROR`] is.
const MIRROR_ROUGHNESS: f32 = 0.05;

/// How rough [`ROUGH_METAL`] is. Well clear of [`MIRROR_ROUGHNESS`], so the two
/// conductors are not a subtle pair.
const ROUGH_METAL_ROUGHNESS: f32 = 0.55;

/// [`PLASTER`]'s base colour: painted white, which is not `1.0`.
///
/// **An albedo rather than a factor of one, and the frame is why.** A surface
/// that reflected every photon it received, under a sun above `1.0`, clips the
/// tonemap's top end over the whole room — the first frame of this room was a
/// white floor at 250/255 with a shaft of sunlight on it nobody could see. Real
/// white paint is around here.
const PLASTER_COLOR: [f32; 4] = [0.82, 0.80, 0.78, 1.0];

/// [`FLOOR`]'s base colour, on [`PLASTER_COLOR`]'s terms and darker still: this
/// row is multiplied by [`FLOOR_TEXELS`] as well, which is what a floor with a
/// pattern on it reflects.
const FLOOR_COLOR: [f32; 4] = [0.62, 0.60, 0.57, 1.0];

/// [`BOUNCE`]'s base colour: a strong warm red, at a factor a diffuse surface
/// can carry without clipping under the sun.
const BOUNCE_COLOR: [f32; 4] = [0.72, 0.11, 0.09, 1.0];

/// [`ROUGH_METAL`]'s base colour, which for a conductor is its **F0**: brass.
///
/// Coloured rather than grey so that the specular this row does have is
/// recognisably a metal's rather than a dielectric's white highlight — the one
/// visible thing left when the diffuse and the ambient are both gone.
const ROUGH_METAL_COLOR: [f32; 4] = [0.95, 0.78, 0.45, 1.0];

// ---------------------------------------------------------------------------
// The meshes
// ---------------------------------------------------------------------------

/// Where each resident mesh is in [`SceneDesc::meshes`], and therefore what an
/// [`InstanceDesc::mesh`] naming it carries.
///
/// Written as one enum-like block of constants rather than derived, because the
/// order of that list is load-bearing in the four ways `crcbl::render::scene`'s
/// module docs open with — and `the_room_places_every_mesh_it_makes_resident`
/// is what holds these against the list.
pub const FLOOR_MESH: usize = 0;
/// The ceiling, on [`FLOOR_MESH`]'s terms.
pub const CEILING_MESH: usize = 1;
/// The back wall (`-z`).
pub const BACK_WALL_MESH: usize = 2;
/// The front wall (`+z`), behind the fixed camera.
pub const FRONT_WALL_MESH: usize = 3;
/// The `-x` wall, with the window opening in it.
pub const WINDOW_WALL_MESH: usize = 4;
/// The `+x` wall — the coloured one.
pub const BOUNCE_WALL_MESH: usize = 5;
/// The plinth the mirror panel stands on.
pub const PLINTH_MESH: usize = 6;
/// The mirror-grade panel.
pub const MIRROR_MESH: usize = 7;
/// The rough metal block.
pub const BLOCK_MESH: usize = 8;

/// Which way a quad faces along the axis its plane is perpendicular to.
///
/// The engine culls back faces and calls counter-clockwise front, so a quad's
/// corner order decides whether it is a wall or a hole. Naming the direction
/// rather than writing four coordinates per quad is what keeps that decision in
/// one place: [`MeshBuilder`]'s three quad methods are the only code in this
/// sample that knows the winding rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Facing {
    /// The face's normal points along `+axis`.
    Positive,
    /// The face's normal points along `-axis`.
    Negative,
}

/// A triangle list under construction, and the positions `build_meshlets` needs
/// beside it.
///
/// Four vertices per quad, never shared with a neighbour, for the reason
/// `crcbl_shaders::mesh::OPEN_BOX_VERTEX_COUNT` records: a shared corner gets an
/// averaged normal, which is the opposite of what a flat face wants.
#[derive(Debug, Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
}

/// The texture coordinates of a quad's four corners, in the order the quad
/// methods below emit them — `crcbl_shaders::mesh`'s `QUAD_UV`, which is not
/// public, spelled once here.
const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

impl MeshBuilder {
    /// Appends one quad, given its corners already in counter-clockwise order
    /// seen from `normal`'s side, and two triangles over them.
    ///
    /// **The one place the winding rule lives.** `0 1 2, 0 2 3` preserves the
    /// corner order, exactly as `crcbl_shaders::mesh::cube_indices` does.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let base = u32::try_from(self.vertices.len())
            .unwrap_or_else(|_| unreachable!("a room of a few hundred vertices"));
        for (corner, uv) in corners.iter().zip(&QUAD_UV) {
            self.positions.push([corner.x, corner.y, corner.z]);
            self.vertices.push(MeshVertex {
                position: [corner.x, corner.y, corner.z, 1.0],
                normal: [normal.x, normal.y, normal.z, 0.0],
                // White, so the **material row** is what colours a surface. The
                // engine's own demo carries a diagnostic hue per face instead;
                // this sample's subject is the material table and the light, and
                // a vertex colour under both would be a third factor in every
                // product a reader is trying to attribute.
                color: [1.0, 1.0, 1.0, 1.0],
                uv: [uv[0], uv[1], 0.0, 0.0],
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

    /// The mesh this builder describes, clustered.
    ///
    /// # Panics
    ///
    /// If [`crcbl::scene::build_meshlets`] refuses the triangle list, which for
    /// literals written in this file would be a mistake in this file rather than
    /// a condition a run can be in — every quad above emits a whole number of
    /// triangles over indices it has just pushed.
    fn finish(self, label: &'static str) -> MeshDesc<'static> {
        let clusters = crcbl::scene::build_meshlets(&self.positions, &self.indices)
            .unwrap_or_else(|why| panic!("{label} is a whole number of triangles: {why}"))
            .into_clusters();
        let mut bytes = Vec::with_capacity(self.vertices.len() * mesh::VERTEX_STRIDE);
        for vertex in &self.vertices {
            for value in vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.color)
                .chain(&vertex.uv)
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        MeshDesc {
            label: Cow::Borrowed(label),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(bytes),
                indices: Cow::Owned(self.indices),
                clusters,
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

/// The `-x` wall, with a rectangular opening in it.
///
/// Four slabs around the hole rather than a wall with a window painted on it,
/// because the sun has to come **through** it: a hole in the geometry is a hole
/// in the shadow map, and the bright quadrilateral on the floor is the shadow
/// pass's own evidence rather than a decal.
fn window_wall(builder: &mut MeshBuilder) {
    let slab = |builder: &mut MeshBuilder, y: (f32, f32), z: (f32, f32)| {
        builder.box_outward(
            Vec3::new(-HALF_WIDTH - SHELL, y.0, z.0),
            Vec3::new(-HALF_WIDTH, y.1, z.1),
        );
    };
    let run = (-HALF_DEPTH, HALF_DEPTH);
    // Below the sill and above the head, both the full run of the wall.
    slab(builder, (0.0, WINDOW_SILL), run);
    slab(builder, (WINDOW_HEAD, HEIGHT), run);
    // The two piers beside the opening, between sill and head.
    let opening = (WINDOW_SILL, WINDOW_HEAD);
    slab(builder, opening, (-HALF_DEPTH, -WINDOW_HALF));
    slab(builder, opening, (WINDOW_HALF, HALF_DEPTH));
}

/// A slab of the shell between two corners, as its own mesh.
fn shell_slab(label: &'static str, min: Vec3, max: Vec3) -> MeshDesc<'static> {
    mesh_of(label, |builder| builder.box_outward(min, max))
}

// ---------------------------------------------------------------------------
// The description
// ---------------------------------------------------------------------------

/// Everything the room makes resident: nine meshes, five material rows and a
/// two-layer page.
///
/// The mesh order is [`FLOOR_MESH`] through [`BLOCK_MESH`] and the row order is
/// [`PLASTER`] through [`ROUGH_METAL`]; both are load-bearing, and the constants
/// above are how an instance names one.
///
/// # The capacities are sized for this room rather than defaulted
///
/// `Capacities::default` reserves what the engine's own demo scene has always
/// reserved, which is sixteen thousand instances and a quarter of a megabyte of
/// index buffer for a room of twenty-seven quads. Sizing them down is the
/// cheapest demonstration of what
/// [`Capacities`] is for — and it is not free
/// to get wrong in the other direction, so
/// `the_room_fits_the_capacities_it_reserves` is what checks the description
/// against them with no GPU in the room.
#[must_use]
pub fn room() -> SceneDesc<'static> {
    let mut page = PageDesc::opaque_white(PAGE_EXTENT);
    let floor_layer = page.push_layer(&FLOOR_TEXELS[..]);
    debug_assert_eq!(
        floor_layer, FLOOR_LAYER,
        "the floor's check is the layer past the white one"
    );

    // The floor and the ceiling cover the room's own footprint; the four walls
    // stand beside them rather than on them, so no two slabs share a plane and
    // nothing z-fights. The end walls run the extra `SHELL` each way, which is
    // what closes the four vertical corners.
    let (west, east) = (-HALF_WIDTH, HALF_WIDTH);
    let (back, front) = (-HALF_DEPTH, HALF_DEPTH);
    let outer = (west - SHELL, east + SHELL);

    SceneDesc {
        meshes: vec![
            shell_slab(
                "floor",
                Vec3::new(west, -SHELL, back),
                Vec3::new(east, 0.0, front),
            ),
            // **The ceiling caps the walls rather than sitting between
            // them.** Stopped at the room's own footprint it left a gap over
            // the top of every wall, and the sun came through the one above the
            // window wall and laid a bright band along the back wall's head —
            // a light leak, and one that reads as a shadow-map artefact rather
            // than as the hole it is.
            shell_slab(
                "ceiling",
                Vec3::new(outer.0, HEIGHT, back - SHELL),
                Vec3::new(outer.1, HEIGHT + SHELL, front + SHELL),
            ),
            shell_slab(
                "back wall",
                Vec3::new(outer.0, 0.0, back - SHELL),
                Vec3::new(outer.1, HEIGHT, back),
            ),
            shell_slab(
                "front wall",
                Vec3::new(outer.0, 0.0, front),
                Vec3::new(outer.1, HEIGHT, front + SHELL),
            ),
            mesh_of("window wall", window_wall),
            shell_slab(
                "bounce wall",
                Vec3::new(east, 0.0, back),
                Vec3::new(east + SHELL, HEIGHT, front),
            ),
            mesh_of("plinth", |b| b.box_outward(PLINTH_MIN, PLINTH_MAX)),
            mesh_of("mirror panel", |b| b.box_outward(MIRROR_MIN, MIRROR_MAX)),
            mesh_of("metal block", |b| b.box_outward(BLOCK_MIN, BLOCK_MAX)),
        ],
        materials: vec![
            // **First, so it is row 0** — the row `mesh::GpuInstance::default`
            // names. The layer is written out rather than left to `UNTINTED`'s
            // own zero, so the two agreeing is visible at the call site.
            GpuMaterial {
                base_color: PLASTER_COLOR,
                base_color_texture: UNTEXTURED_LAYER,
                roughness: PLASTER_ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color: FLOOR_COLOR,
                base_color_texture: FLOOR_LAYER,
                roughness: PLASTER_ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color: BOUNCE_COLOR,
                base_color_texture: UNTEXTURED_LAYER,
                roughness: PLASTER_ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color_texture: UNTEXTURED_LAYER,
                metallic: 1.0,
                roughness: MIRROR_ROUGHNESS,
                ..GpuMaterial::UNTINTED
            },
            GpuMaterial {
                base_color: ROUGH_METAL_COLOR,
                base_color_texture: UNTEXTURED_LAYER,
                metallic: 1.0,
                roughness: ROUGH_METAL_ROUGHNESS,
            },
        ],
        page,
        capacities: CAPACITIES,
    }
}

/// How much of each pool [`room`] reserves.
///
/// Every one of these is device-local memory taken at start-up and never grown,
/// so they are the room's ceiling and not its size: a description that outgrew
/// one is refused by `ForwardRenderer::with_scene` before the first device
/// object exists, and the instance count is the one no description can be
/// measured against — filling it is `InstancePoolError::PoolFull` from
/// [`place`].
pub const CAPACITIES: Capacities = Capacities {
    vertices: 4 * 1024,
    indices: 8 * 1024,
    meshes: 32,
    instances: 64,
    materials: 8,
    lights: 8,
};

/// Which mesh each object in the room is, and which row it shades through, in
/// the order [`place`] inserts them.
///
/// **Insertion order is the caller's and it is load-bearing**, on
/// `crcbl::render::scene`'s terms: the slot an object lands in is
/// `docs/plan/25-lod.md`'s hysteresis key, so a golden compared across two runs
/// needs the two runs to have placed things in the same order. One list, walked
/// once, is what makes that true by construction rather than by two call sites
/// agreeing.
const OBJECTS: [(usize, usize); 9] = [
    (FLOOR_MESH, FLOOR),
    (CEILING_MESH, PLASTER),
    (BACK_WALL_MESH, PLASTER),
    (FRONT_WALL_MESH, PLASTER),
    (WINDOW_WALL_MESH, PLASTER),
    (BOUNCE_WALL_MESH, BOUNCE),
    (PLINTH_MESH, PLASTER),
    (MIRROR_MESH, MIRROR),
    (BLOCK_MESH, ROUGH_METAL),
];

/// Puts every object of the room in `renderer`, and reports how many.
///
/// The geometry is already at world scale and world position — every mesh above
/// is built where it stands — so each transform is the identity. That is
/// deliberate rather than lazy: `GpuInstance::transform` must be **rigid**
/// because the shader transforms normals with its 3×3 part and no inverse
/// transpose, and a room built at its own size never has to reason about which
/// scales are safe on which faces.
///
/// # Errors
///
/// [`InstancePoolError::PoolFull`] if [`CAPACITIES`]'s instance count does not
/// cover the room, which is a mistake in this file rather than a condition a run
/// can be in — but it is the caller that would have to report it, so it is
/// returned rather than unwrapped.
pub fn place(renderer: &mut ForwardRenderer) -> Result<usize, InstancePoolError> {
    for (mesh, material) in OBJECTS {
        renderer.add_instance(&InstanceDesc {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        })?;
    }
    Ok(OBJECTS.len())
}

// ---------------------------------------------------------------------------
// The lights
// ---------------------------------------------------------------------------

/// How bright the sun is, before its colour.
///
/// Above 1.0, like every other sun in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back, so a key light
/// that peaked at 1.0 would be one this pipeline could not tell from a dimmer
/// one.
const SUN_INTENSITY: f32 = 1.6;

/// The sun: daylight coming in through the window.
///
/// **The direction is the room's, not a taste.** It is the vector *towards* the
/// light, so a positive `-x` component puts the sun outside the window wall and
/// a positive `y` puts it above the head of the opening — which is what makes
/// the light travel `+x` and down, through the opening, onto the floor and up
/// the coloured wall. Turn it round and the window is a dark rectangle in a wall
/// lit from inside, which is a perfectly plausible picture of nothing.
///
/// The ambient is cool and small: it stands for the sky, it is the **only**
/// indirect term there is, and it is what the occlusion pass scales. Large
/// enough that a surface no light reaches is dark rather than black — a black
/// floor would make every occlusion band a measurement of an unpainted frame.
#[must_use]
pub fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(-0.90, 0.30, 0.31).normalize(),
        color: Vec3::new(1.0, 0.96, 0.88) * SUN_INTENSITY,
        ambient: Vec3::new(0.055, 0.062, 0.078),
    }
}

/// How far from the room's centre the lamp orbits.
const LAMP_ORBIT: f32 = 1.6;

/// How high it hangs.
const LAMP_HEIGHT: f32 = 1.9;

/// How long one orbit takes, in seconds. Slow enough to read as a moving light
/// rather than a strobe.
const LAMP_PERIOD: f32 = 12.0;

/// How far the lamp reaches, in world units.
///
/// **A hard bound**: `PointLight::radius` is where the shading window reaches
/// zero and what the clustering pass culls against. Under half the room's depth,
/// so the light is a pool that moves rather than a second sun — a lamp reaching
/// every wall would light the whole room evenly and there would be nothing to
/// see it move against.
///
/// It is also short of [`SUNLIT_FLOOR`] and [`SHADED_FLOOR`], which is what lets
/// the golden suite's first claim be about the window: two floor samples the
/// lamp reached would differ in it as well as in the sun, and the ratio between
/// them would be a measurement of both.
const LAMP_REACH: f32 = 3.0;

/// How bright it is, before its colour.
const LAMP_INTENSITY: f32 = 4.0;

/// The moving light, at `seconds` into the run.
///
/// A warm point light orbiting the middle of the room, below the ceiling. Warm
/// against the sun's near-white, so which light lit a surface is legible in the
/// picture rather than a brightness difference — and it is the light that
/// occludes: `crcbl::render::shadow`'s selection hands the atlas's light tiles
/// to the most influential candidate, and this is the only candidate.
///
/// A pure function of the time, so a frame at `t` is the same frame on every
/// machine — which is what makes a golden of it worth anything.
#[must_use]
pub fn lamp(seconds: f32) -> Light {
    let angle = std::f32::consts::TAU * (seconds / LAMP_PERIOD);
    Light::Point(PointLight {
        position: Vec3::new(
            LAMP_ORBIT * angle.cos(),
            LAMP_HEIGHT,
            LAMP_ORBIT * angle.sin(),
        ),
        radius: LAMP_REACH,
        color: Vec3::new(1.0, 0.62, 0.28) * LAMP_INTENSITY,
    })
}

// ---------------------------------------------------------------------------
// The cameras
// ---------------------------------------------------------------------------

/// Where the fixed camera stands: just inside the front wall, at
/// [`EYE_HEIGHT`], on the room's axis.
///
/// **On the axis rather than off it, and the room's width is why.** A six-metre
/// room seen through a sixty-degree lens does not hold both side walls in frame
/// from anywhere except its centre line — an oblique camera puts one of them
/// behind the viewer and reduces the other to a sliver, which is what the first
/// framing of this room did to the coloured wall. From here the two recede
/// symmetrically, the back wall closes the view, and every object stands between
/// them.
const FIXED_EYE: Vec3 = Vec3::new(0.0, EYE_HEIGHT, 3.3);

/// What it looks at: the middle of the back wall, a little below eye height.
///
/// A shallow pitch rather than a level view, because the floor is what most of
/// this fixture's evidence is on — the shaft of sunlight, the two contact
/// shadows and the occlusion bands are all down there — and a level camera at
/// eye height puts the nearest visible floor four metres away.
const FIXED_TARGET: Vec3 = Vec3::new(0.0, 1.0, -HALF_DEPTH);

/// The vertical field of view the room is drawn with, in radians.
///
/// Sixty degrees, the same as every other camera in the tree, so a frame from
/// this sample and a frame from the golden suite are the same lens.
const FOV_Y: f32 = std::f32::consts::FRAC_PI_3;

/// The camera the goldens are taken from, and the pose the free-fly camera
/// starts at.
///
/// **An eye-height camera inside a room, which is the thing `Scene::Ao` has
/// never had.** That scene looks straight down into a trough, so every surface
/// in it is at one depth and its occlusion kernels never straddle two. Here the
/// floor recedes from 1.5 metres to 7, the block stands in front of the far
/// wall, and every band the occlusion pass writes is on a surface going away
/// from the eye.
#[must_use]
pub fn fixed_camera() -> Camera {
    Camera {
        eye: FIXED_EYE,
        target: FIXED_TARGET,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            // Close, because the camera stands inside the room and the front
            // wall is a metre and a half behind it: a far near-plane would clip
            // the floor out from under the viewer.
            near: 0.02,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every mesh the description makes resident is placed**, and every row it
    /// declares is named by something.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a material nobody can see — both of which leave a
    /// perfectly plausible picture, which is why neither is visible in a golden.
    #[test]
    fn the_room_places_every_mesh_it_makes_resident() {
        let scene = room();
        let labels: Vec<&str> = scene.meshes.iter().map(|m| m.label.as_ref()).collect();
        assert_eq!(
            [
                FLOOR_MESH,
                CEILING_MESH,
                BACK_WALL_MESH,
                FRONT_WALL_MESH,
                WINDOW_WALL_MESH,
                BOUNCE_WALL_MESH,
                PLINTH_MESH,
                MIRROR_MESH,
                BLOCK_MESH,
            ]
            .map(|mesh| labels[mesh]),
            [
                "floor",
                "ceiling",
                "back wall",
                "front wall",
                "window wall",
                "bounce wall",
                "plinth",
                "mirror panel",
                "metal block",
            ],
            "an index that names the wrong entry places the wrong mesh, and the frame \
             still draws"
        );

        for (mesh, label) in labels.iter().enumerate() {
            assert!(
                OBJECTS.iter().any(|(placed, _)| *placed == mesh),
                "{label} is resident and nothing places it"
            );
        }
        for row in 0..scene.materials.len() {
            assert!(
                OBJECTS.iter().any(|(_, named)| *named == row),
                "material row {row} is declared and nothing shades through it"
            );
        }
        // And the two conductors differ in roughness alone, which is the whole
        // of what that pair is evidence about.
        let mirror = scene.materials[MIRROR];
        let rough = scene.materials[ROUGH_METAL];
        assert_eq!(mirror.metallic, rough.metallic, "both rows are conductors");
        assert!(
            mirror.roughness < rough.roughness,
            "the mirror row must be the smoother of the pair: {mirror:?} vs {rough:?}"
        );
    }

    /// **The room fits inside the pools it reserves**, checked with no GPU.
    ///
    /// `ForwardRenderer::with_scene` refuses a description that outgrows one,
    /// naming the pool — but only on a machine with a device, and only when
    /// somebody runs the e2e suite. This is the same arithmetic in the plain
    /// suite, so a quad added to the room fails here first.
    #[test]
    fn the_room_fits_the_capacities_it_reserves() {
        let scene = room();
        let mut vertices = 0u32;
        let mut indices = 0u32;
        for desc in &scene.meshes {
            let Geometry::Flat {
                vertices: bytes,
                indices: list,
                ..
            } = &desc.geometry
            else {
                panic!("{}: the room has no cluster DAG in it", desc.label);
            };
            assert!(
                bytes.len().is_multiple_of(mesh::VERTEX_STRIDE),
                "{}: {} byte(s) is not a whole number of vertices",
                desc.label,
                bytes.len()
            );
            vertices += (bytes.len() / mesh::VERTEX_STRIDE) as u32;
            indices += list.len() as u32;
        }
        assert!(
            vertices <= CAPACITIES.vertices,
            "{vertices} vertices in a pool of {}",
            CAPACITIES.vertices
        );
        assert!(
            indices <= CAPACITIES.indices,
            "{indices} indices in a pool of {}",
            CAPACITIES.indices
        );
        assert!(scene.meshes.len() as u32 <= CAPACITIES.meshes);
        assert!(scene.materials.len() as u32 <= CAPACITIES.materials);
        assert!(OBJECTS.len() as u32 <= CAPACITIES.instances);
        // The sun takes a row of the light list too, so the lamp needs room
        // beside it rather than exactly the whole list.
        const {
            assert!(
                1 + 1 < CAPACITIES.lights,
                "the sun and the lamp both need a row"
            );
        }
    }

    /// **Every quad faces into the room**, which is the one mistake in this file
    /// that produces a picture rather than an error.
    ///
    /// Back faces are culled, so a wall wound the wrong way is a wall that is
    /// simply not there — and a room with one missing wall looks like a room
    /// seen from an angle. The check is the winding rule itself: the cross
    /// product of a triangle's first two edges must point the way its vertices'
    /// normal does.
    #[test]
    fn every_face_of_the_room_is_wound_the_way_its_normal_points() {
        for desc in &room().meshes {
            let Geometry::Flat {
                vertices, indices, ..
            } = &desc.geometry
            else {
                panic!("{}: the room has no cluster DAG in it", desc.label);
            };
            let read = |vertex: u32, at: usize| {
                let base = vertex as usize * mesh::VERTEX_STRIDE + at * 4;
                f32::from_le_bytes(
                    vertices[base..base + 4]
                        .try_into()
                        .expect("four bytes of an f32"),
                )
            };
            let position =
                |vertex: u32| Vec3::new(read(vertex, 0), read(vertex, 1), read(vertex, 2));
            // The normal sits after the four-float position.
            let normal = |vertex: u32| Vec3::new(read(vertex, 4), read(vertex, 5), read(vertex, 6));

            assert!(!indices.is_empty(), "{}: no triangles", desc.label);
            for triangle in indices.chunks_exact(3) {
                let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
                let face = (position(b) - position(a)).cross(position(c) - position(a));
                assert!(
                    face.normalize().dot(normal(a)) > 0.99,
                    "{}: triangle {triangle:?} winds away from its normal {:?}",
                    desc.label,
                    normal(a)
                );
            }
        }
    }

    /// The window is a hole rather than a wall with a window painted on it: no
    /// vertex of the window wall lies inside the opening.
    ///
    /// A wall built as one slab with a texture would light the floor evenly and
    /// leave the shadow map with nothing to cut — and the shaft of light this
    /// scene's whole framing is built around would simply not be there.
    ///
    /// Strict inequalities, with a margin: every slab around the hole has an
    /// edge **on** the opening's boundary, which is what makes it an opening
    /// rather than a gap.
    #[test]
    fn the_window_is_an_opening_and_not_a_painted_rectangle() {
        const MARGIN: f32 = 1e-3;
        let mut builder = MeshBuilder::default();
        window_wall(&mut builder);
        assert!(!builder.positions.is_empty(), "the wall has no geometry");

        for corner in &builder.positions {
            let inside = corner[1] > WINDOW_SILL + MARGIN
                && corner[1] < WINDOW_HEAD - MARGIN
                && corner[2].abs() < WINDOW_HALF - MARGIN;
            assert!(!inside, "{corner:?} is inside the opening");
        }
        // And the piers really reach it, on both sides: a wall that stopped
        // short would leave a gap rather than a window, and every vertex would
        // still be outside the opening.
        for edge in [-WINDOW_HALF, WINDOW_HALF] {
            assert!(
                builder
                    .positions
                    .iter()
                    .any(|corner| (corner[2] - edge).abs() < MARGIN),
                "nothing reaches the opening's edge at z = {edge}"
            );
        }
        for edge in [WINDOW_SILL, WINDOW_HEAD] {
            assert!(
                builder
                    .positions
                    .iter()
                    .any(|corner| (corner[1] - edge).abs() < MARGIN),
                "nothing reaches the opening's edge at y = {edge}"
            );
        }
    }

    /// The sun comes from outside the window wall and above the opening's head.
    ///
    /// Both halves decide whether there is a shaft of light in the frame at all,
    /// and neither is visible in any assertion about the description.
    #[test]
    fn the_sun_is_outside_the_window_and_above_its_head() {
        let towards = sun().direction;
        assert!(
            towards.x < 0.0,
            "the sun must be beyond the -x wall, not inside the room: {towards:?}"
        );
        assert!(towards.y > 0.0, "and above it: {towards:?}");
        // Where the ray through the middle of the opening lands on the floor.
        // Travelling `-towards`, from the window's centre, down to `y = 0`.
        let opening = Vec3::new(-HALF_WIDTH, 0.5 * (WINDOW_SILL + WINDOW_HEAD), 0.0);
        let travel = -towards;
        let landing = opening + travel * (opening.y / -travel.y);
        assert!(
            landing.x > -HALF_WIDTH && landing.x < HALF_WIDTH,
            "the shaft lands at {landing:?}, outside the room"
        );
        assert!(
            landing.z.abs() < HALF_DEPTH,
            "the shaft lands at {landing:?}, past an end wall"
        );
    }

    /// The lamp stays inside the room, all the way round, and its pool does not
    /// reach every wall at once.
    #[test]
    fn the_lamp_orbits_inside_the_room() {
        for step in 0..24 {
            let seconds = LAMP_PERIOD * step as f32 / 24.0;
            let Light::Point(light) = lamp(seconds) else {
                panic!("the lamp is a point light");
            };
            assert!(
                light.position.x.abs() < HALF_WIDTH
                    && light.position.z.abs() < HALF_DEPTH
                    && light.position.y > 0.0
                    && light.position.y < HEIGHT,
                "the lamp left the room at {seconds}s: {:?}",
                light.position
            );
        }
        // A pool, not a second sun: the far wall is out of reach from the
        // orbit's nearest approach to it.
        const {
            assert!(
                LAMP_REACH < HALF_DEPTH + (HALF_DEPTH - LAMP_ORBIT),
                "a lamp reaching both end walls at once lights the room evenly"
            );
        }
    }

    /// The fixed camera stands in the room, at eye height, looking at something
    /// inside it.
    ///
    /// A camera outside its own room renders the outside of a box, which every
    /// other assertion in this file would still pass.
    #[test]
    fn the_fixed_camera_stands_inside_the_room_at_eye_height() {
        let camera = fixed_camera();
        assert!(
            camera.eye.x.abs() < HALF_WIDTH
                && camera.eye.z.abs() < HALF_DEPTH
                && camera.eye.y > 0.0
                && camera.eye.y < HEIGHT,
            "the camera is outside the room: {:?}",
            camera.eye
        );
        assert_eq!(camera.eye.y, EYE_HEIGHT);
        assert!(
            camera.target.x.abs() <= HALF_WIDTH && camera.target.z.abs() <= HALF_DEPTH,
            "the camera looks at a point outside the room: {:?}",
            camera.target
        );
        // Facing the back of the room, which is where every object stands.
        assert!(
            camera.target.z < camera.eye.z,
            "the camera looks at the wall behind it"
        );
    }

    /// **[`SUNLIT_FLOOR`] and [`SHADED_FLOOR`] differ in the sun and in nothing
    /// else**, which is what makes the golden suite's first ratio a claim about
    /// the window rather than about two arbitrary pixels.
    ///
    /// Four things, and the frame shows none of them: both are on the floor
    /// inside the room; the ray back along the sun's direction leaves through
    /// the opening from one and hits the wall from the other; neither is inside
    /// [`lamp`]'s radius at `t = 0`, so the light that moves contributes to
    /// neither; and neither is under an object that would occlude the sun before
    /// it arrives.
    #[test]
    fn the_two_floor_samples_differ_in_the_sun_and_in_nothing_else() {
        let towards = sun().direction;
        // Back along the sun, from a floor point to the plane of the window
        // wall: where it crosses `x = -HALF_WIDTH`, as `(height, run)`.
        let back_to_the_wall = |point: Vec3| {
            let steps = (point.x + HALF_WIDTH) / towards.x.abs();
            (point.y + towards.y * steps, point.z + towards.z * steps)
        };

        for point in [SUNLIT_FLOOR, SHADED_FLOOR] {
            assert_eq!(point.y, 0.0, "{point:?} is not on the floor");
            assert!(
                point.x.abs() < HALF_WIDTH && point.z.abs() < HALF_DEPTH,
                "{point:?} is outside the room"
            );
            let Light::Point(light) = lamp(0.0) else {
                panic!("the lamp is a point light");
            };
            assert!(
                point.distance(light.position) > light.radius,
                "{point:?} is inside the lamp's radius, so the two samples do not differ \
                 in the sun alone"
            );
            // Clear of every object's footprint, so nothing stands between the
            // window and the floor here.
            for (min, max) in [
                (PLINTH_MIN, PLINTH_MAX),
                (MIRROR_MIN, MIRROR_MAX),
                (BLOCK_MIN, BLOCK_MAX),
            ] {
                assert!(
                    point.x < min.x || point.x > max.x || point.z < min.z || point.z > max.z,
                    "{point:?} is under the object spanning {min:?}..{max:?}"
                );
            }
        }

        let (lit_height, lit_run) = back_to_the_wall(SUNLIT_FLOOR);
        assert!(
            lit_height > WINDOW_SILL && lit_height < WINDOW_HEAD && lit_run.abs() < WINDOW_HALF,
            "the ray back from {SUNLIT_FLOOR:?} meets the wall at ({lit_height:.2}, \
             {lit_run:.2}), which is not inside the opening"
        );
        let (dark_height, dark_run) = back_to_the_wall(SHADED_FLOOR);
        assert!(
            !(WINDOW_SILL..=WINDOW_HEAD).contains(&dark_height) || dark_run.abs() > WINDOW_HALF,
            "the ray back from {SHADED_FLOOR:?} meets the wall at ({dark_height:.2}, \
             {dark_run:.2}), which is inside the opening — both samples are lit"
        );
    }

    /// [`MIRROR_FOOT`] and [`MIRROR_MISSES`] are on one face of one material and
    /// differ in **one** thing: whether the ray each sends finds the floor while
    /// it is still inside the frame.
    ///
    /// Worked out here rather than eyeballed off a picture, on
    /// [`the_two_floor_samples_differ_in_the_sun_and_in_nothing_else`]'s terms.
    /// The face's normal is `+Z`, so reflecting the view ray about it flips the
    /// `z` component and leaves `x` and `y` alone; the reflected ray falls
    /// towards the floor for any point below eye height and meets it at a
    /// position this test projects through [`fixed_camera`]. The transition — the
    /// height above which that position leaves the frame — is bisected rather
    /// than written down, so the two constants are checked against the camera
    /// they are read under rather than against a number somebody wrote once.
    ///
    /// [`the_two_floor_samples_differ_in_the_sun_and_in_nothing_else`]: fn@the_two_floor_samples_differ_in_the_sun_and_in_nothing_else
    #[test]
    fn the_mirror_panel_reflects_at_its_foot_and_not_at_its_head() {
        let camera = fixed_camera();
        // The golden suite's own aspect: both extents it draws at are 4:3.
        let view_projection = camera.view_projection(4.0 / 3.0);
        // Where the ray reflected off the `+Z` face at `height` meets the floor,
        // and whether that point is inside the frame. `None` at or above eye
        // height, where the ray rises instead of falling.
        let lands_in_frame = |height: f32| {
            if height >= camera.eye.y {
                return false;
            }
            let steps = height / (camera.eye.y - height);
            let hit = Vec3::new(
                MIRROR_MID_X + (MIRROR_MID_X - camera.eye.x) * steps,
                0.0,
                MIRROR_FACE_Z + (camera.eye.z - MIRROR_FACE_Z) * steps,
            );
            let clip = view_projection * hit.extend(1.0);
            clip.w > 0.0 && (clip.x / clip.w).abs() < 1.0 && (clip.y / clip.w).abs() < 1.0
        };

        for point in [MIRROR_FOOT, MIRROR_MISSES] {
            assert_eq!(
                point.z, MIRROR_FACE_Z,
                "{point:?} is not on the panel's front face"
            );
            assert!(
                point.x > MIRROR_MIN.x
                    && point.x < MIRROR_MAX.x
                    && point.y >= MIRROR_MIN.y
                    && point.y <= MIRROR_MAX.y,
                "{point:?} is off the panel's front face, so it is not the mirror material"
            );
            let Light::Point(light) = lamp(0.0) else {
                panic!("the lamp is a point light");
            };
            assert!(
                point.distance(light.position) > light.radius,
                "{point:?} is inside the lamp's radius, so the two samples do not differ in \
                 the reflection alone"
            );
        }
        assert_eq!(
            (MIRROR_FOOT.x, MIRROR_MISSES.x),
            (MIRROR_MID_X, MIRROR_MID_X),
            "the two samples must be in the same column of the face, or they differ in the \
             direction their rays leave in as well as in the height"
        );

        // The transition, bisected between the panel's foot and its head.
        let (mut reflects, mut misses) = (MIRROR_FOOT.y, MIRROR_MAX.y);
        assert!(
            lands_in_frame(reflects) && !lands_in_frame(misses),
            "the panel's foot must reflect into the frame and its head must not, or there is \
             no transition between them to find"
        );
        for _ in 0..40 {
            let middle = 0.5 * (reflects + misses);
            if lands_in_frame(middle) {
                reflects = middle;
            } else {
                misses = middle;
            }
        }
        // The block the golden hangs above `MIRROR_FOOT` is as tall as the
        // caller's own, so the band has to be a real share of the face rather
        // than a line: an eighth of the panel's height is what the frame this was
        // set against measures, and a band that had shrunk to nothing would leave
        // the golden's claim resting on a couple of rows.
        let band = reflects - MIRROR_FOOT.y;
        assert!(
            band > 0.1,
            "only {band:.3} of the panel's height reflects into the frame, which is too thin \
             a band for a block to sit in — the camera or the panel has moved"
        );
        assert!(
            MIRROR_MISSES.y > misses,
            "{MIRROR_MISSES:?} is at or below the height where reflections stop landing in \
             frame ({misses:.3}), so it is not a control"
        );
    }

    /// The floor's layer is a check rather than a flat colour, and layer 0 is
    /// still the white one every untextured row samples.
    #[test]
    fn the_page_carries_a_white_layer_and_a_floor_that_is_not_flat() {
        let page = room().page;
        assert_eq!(page.extent(), PAGE_EXTENT);
        assert!(
            page.layers()[UNTEXTURED_LAYER as usize]
                .iter()
                .all(|&texel| texel == PageDesc::WHITE),
            "every untextured material is scaled by layer 0"
        );
        let floor = &page.layers()[FLOOR_LAYER as usize];
        assert_eq!(floor.len(), PAGE_LAYER_BYTES);
        assert!(
            floor.chunks_exact(4).any(|texel| texel != &floor[..4]),
            "a flat floor layer would pass with no texture coordinate at all"
        );
    }
}
