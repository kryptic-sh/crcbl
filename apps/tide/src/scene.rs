//! The four scenes, as data an application hands the engine: the courtyard
//! pool that milestone 1 builds, and an empty room standing in for each of the
//! three it does not.
//!
//! ```text
//!  MeshBuilder ──▶ build_meshlets ──▶ Geometry::Flat ──┐
//!  GpuMaterial rows ───────────────────────────────────┼─▶ SceneDesc ──▶ with_scene
//!  PageDesc::empty ────────────────────────────────────┘
//!  Stage::show ──▶ add_instance / remove_instance, set_water
//! ```
//!
//! Every scene's geometry is resident from the start, built from literals by
//! this module's own quad builder on `apps/sundial/src/plaza.rs`' terms, and a
//! scene switch is instances and water rather than a second renderer: a
//! [`Stage`] removes the objects the last scene placed, places the next scene's,
//! and hands the renderer that scene's bodies of water — none, for a room.
//! Godot-style axes throughout — `+Y` up, `-Z` forward.
//!
//! # The courtyard, and what each part of it is for
//!
//! ```text
//!                    paving
//!        ┌─ coping ───────────────────────────────┐
//!        │ deep end (DEEP_FLOOR)│shallow end        │   z = POOL_FAR
//!        │ ═══════════ lane ════╪═══════════════   │
//!        │                      │ (SHALLOW_FLOOR)   │   z = POOL_NEAR
//!        └────────────────────────────────────────┘
//!                  camera, on x = 0, looking −Z
//! ```
//!
//! `crcbl::screenshot`'s still pool is the engine's fixture for the same rung,
//! and this is laid out on its argument rather than copied from it:
//!
//! * **The deep end and the shallow end are mirrored about the camera's
//!   column.** A point at `+x` and its mirror at `−x` are seen at the same angle,
//!   carry the same Fresnel term, reflect the same sky and are lit by a sun with
//!   no `x` component alike, so the depth of water under them is the only thing
//!   left to tell them apart — which is what the golden suite's absorption claim
//!   reads. The pool is seen from its long side, which is how a pool with a deep
//!   end and a shallow end puts both in front of one camera.
//! * **The tile has red in it.** Clear water absorbs red fastest, so a floor with
//!   no red would give the claim nothing to lose.
//! * **The water sits below the coping**, as a pool's does, so the rim is dry
//!   stone around a band of wet tile.
//!
//! # The other three scenes are rooms, on purpose
//!
//! Open sea, coast and valley each wait on a rung of `docs/plan/55-water.md`
//! that has not landed, so each is an open-topped room with a label naming the
//! milestone that fills it — [`Scene::milestone`]. A room rather than nothing,
//! so a switch is a picture that changes and a frame that still draws; and no
//! water at all, so a stub draws no water pass and prices none.

use std::borrow::Cow;

use crcbl::hal::{Device, Format, HalError, QueueHandle};
use crcbl::math::Vec3;
use crcbl::render::{
    Camera, Capacities, DirectionalLight, ForwardRenderer, Geometry, InstanceDesc, InstanceHandle,
    MeshDesc, PageDesc, Projection, SceneDesc, Sky, WaterBody,
};
use crcbl::shaders::mesh::{self, GpuMaterial, MeshVertex};
use crcbl::shaders::vertex::UvRange;

use crate::medium::Preset;

// ---------------------------------------------------------------------------
// The scenes
// ---------------------------------------------------------------------------

/// Which of the gallery's four scenes is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The FFT ocean to the horizon — milestone 3.
    OpenSea,
    /// The ocean meeting a beach — milestone 5.
    Coast,
    /// A river, a waterfall and a lake — milestone 4.
    Valley,
    /// The tiled pool, and the one scene milestone 1 builds.
    ///
    /// The default, because it is the only scene with water in it and the
    /// goldens are taken from it.
    #[default]
    Courtyard,
}

impl Scene {
    /// Every scene, in `docs/plan/sample/21-tide.md`'s order, which is the order
    /// `N` and the page button walk them.
    pub const ALL: [Self; 4] = [Self::OpenSea, Self::Coast, Self::Valley, Self::Courtyard];

    /// What the panel, the heartbeat and the page call it — one token, on
    /// [`Preset::label`]'s terms.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenSea => "open-sea",
            Self::Coast => "coast",
            Self::Valley => "valley",
            Self::Courtyard => "courtyard",
        }
    }

    /// Parses a [`Scene::label`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.label() == name)
    }

    /// The next one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::OpenSea => Self::Coast,
            Self::Coast => Self::Valley,
            Self::Valley => Self::Courtyard,
            Self::Courtyard => Self::OpenSea,
        }
    }

    /// The milestone of `docs/plan/sample/21-tide.md` that builds this scene,
    /// or `None` for the one that is built.
    #[must_use]
    pub const fn milestone(self) -> Option<u32> {
        match self {
            Self::OpenSea => Some(3),
            Self::Coast => Some(5),
            Self::Valley => Some(4),
            Self::Courtyard => None,
        }
    }

    /// The label a stub room draws over itself, or `None` for the courtyard.
    #[must_use]
    pub fn placard(self) -> Option<String> {
        self.milestone().map(|milestone| {
            format!(
                "{}: an empty room until milestone {milestone} fills it",
                self.label().replace('-', " ").to_uppercase()
            )
        })
    }

    /// The bodies of water this scene draws, in `medium`.
    #[must_use]
    pub fn bodies(self, medium: Preset) -> Vec<WaterBody> {
        match self {
            Self::Courtyard => vec![pool_body(medium)],
            Self::OpenSea | Self::Coast | Self::Valley => Vec::new(),
        }
    }

    /// Which mesh each of this scene's objects is and which row it shades
    /// through, in the order [`Stage`] places them.
    const fn objects(self) -> &'static [(usize, usize)] {
        match self {
            Self::Courtyard => &COURTYARD_OBJECTS,
            Self::OpenSea | Self::Coast | Self::Valley => &ROOM_OBJECTS,
        }
    }
}

// ---------------------------------------------------------------------------
// The courtyard's layout
// ---------------------------------------------------------------------------

/// The height of the water's surface, in metres — a hand's width under the
/// coping, which sits at zero.
pub const LEVEL: f32 = -0.15;

/// How far `±x` the pool reaches inside its walls.
pub const POOL_HALF_LENGTH: f32 = 7.0;

/// The pool's wall nearest the camera, along `z`.
pub const POOL_NEAR: f32 = 0.0;

/// The pool's far wall, along `z`.
pub const POOL_FAR: f32 = -6.0;

/// The deep end's floor, under `x < 0`.
pub const DEEP_FLOOR: f32 = -2.0;

/// The shallow end's floor, under `x > 0`.
pub const SHALLOW_FLOOR: f32 = -0.9;

/// How wide the coping is around the pool.
const COPING_WIDTH: f32 = 0.45;

/// How far the paving reaches from the pool's centre line, in every direction
/// the camera can look along it — past the far plane's reach at every pose.
const PAVING_REACH: f32 = 30.0;

/// Where the lane line runs along `z`: down the middle of the pool's width.
pub const LANE_Z: f32 = 0.5 * (POOL_NEAR + POOL_FAR);

/// Half the lane line's width.
const LANE_HALF_WIDTH: f32 = 0.12;

/// How far short of each end wall the lane line stops.
const LANE_END_GAP: f32 = 0.6;

/// How far above the floor the lane line is laid, so the two surfaces do not
/// fight for the same depth.
const LANE_LIFT: f32 = 0.004;

// The layout's own order, checked where it is written: the deep end is the
// deeper, both floors are under the water, and the water is under the coping.
const _: () = assert!(DEEP_FLOOR < SHALLOW_FLOOR && SHALLOW_FLOOR < LEVEL && LEVEL < 0.0);

/// The courtyard's one body: the pool inside its walls.
#[must_use]
pub fn pool_body(medium: Preset) -> WaterBody {
    WaterBody {
        outline: vec![
            [-POOL_HALF_LENGTH, POOL_NEAR],
            [POOL_HALF_LENGTH, POOL_NEAR],
            [POOL_HALF_LENGTH, POOL_FAR],
            [-POOL_HALF_LENGTH, POOL_FAR],
        ],
        level: LEVEL,
        medium: medium.medium(),
    }
}

// ---------------------------------------------------------------------------
// A room's layout
// ---------------------------------------------------------------------------

/// How far `±x` a stub room reaches.
const ROOM_HALF_WIDTH: f32 = 9.0;

/// A stub room's far wall and near wall, along `z` — the fixed camera stands
/// inside it.
const ROOM_Z: (f32, f32) = (-12.0, 6.0);

/// How tall a stub room's walls are. Open-topped, so the sun and the sky still
/// reach its floor.
const ROOM_HEIGHT: f32 = 3.0;

// ---------------------------------------------------------------------------
// The material rows
// ---------------------------------------------------------------------------

/// The paving around the pool.
pub const PAVING_MATERIAL: usize = 0;
/// The coping: the pool's rim.
pub const COPING_MATERIAL: usize = 1;
/// The pool's tile, walls and floors alike.
pub const TILE_MATERIAL: usize = 2;
/// The lane line on the pool's floor.
pub const LANE_MATERIAL: usize = 3;
/// A stub room's floor and walls.
pub const ROOM_MATERIAL: usize = 4;

/// How rough every surface here is: matt stone and tile, so no highlight moves
/// across a band a claim reads.
const ROUGHNESS: f32 = 0.8;

/// Each row's base colour, in [`PAVING_MATERIAL`] .. [`ROOM_MATERIAL`] order.
const COLORS: [[f32; 4]; 5] = [
    [0.46, 0.43, 0.39, 1.0],
    [0.80, 0.78, 0.73, 1.0],
    [0.46, 0.70, 0.80, 1.0],
    [0.08, 0.12, 0.24, 1.0],
    [0.70, 0.68, 0.64, 1.0],
];

// ---------------------------------------------------------------------------
// The meshes
// ---------------------------------------------------------------------------

/// The paving, around the coping's outer edge.
pub const PAVING_MESH: usize = 0;
/// The coping ring.
pub const COPING_MESH: usize = 1;
/// The basin: both floors, every wall and the step between the two ends.
pub const BASIN_MESH: usize = 2;
/// The lane line.
pub const LANE_MESH: usize = 3;
/// A stub room.
pub const ROOM_MESH: usize = 4;

/// The courtyard's objects, on [`Scene::objects`]' terms.
const COURTYARD_OBJECTS: [(usize, usize); 4] = [
    (PAVING_MESH, PAVING_MATERIAL),
    (COPING_MESH, COPING_MATERIAL),
    (BASIN_MESH, TILE_MATERIAL),
    (LANE_MESH, LANE_MATERIAL),
];

/// A stub room's one object.
const ROOM_OBJECTS: [(usize, usize); 1] = [(ROOM_MESH, ROOM_MATERIAL)];

/// Which way a quad faces along the axis its plane is perpendicular to —
/// `apps/sundial/src/plaza.rs`' `Facing`, for its reason: the engine culls back
/// faces, so corner order decides whether a quad is a wall or a hole.
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
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// Appends one quad, corners counter-clockwise seen from `normal`'s side.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let base = u32::try_from(self.positions.len())
            .unwrap_or_else(|_| unreachable!("a courtyard of a few hundred vertices"));
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
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

    /// An upward quad in the plane `y`, spanning `x` and `z`. Nothing here is
    /// seen from below, so there is no downward one.
    fn floor(&mut self, y: f32, x: (f32, f32), z: (f32, f32)) {
        let at = |x: f32, z: f32| Vec3::new(x, y, z);
        self.quad(
            [at(x.0, z.1), at(x.1, z.1), at(x.1, z.0), at(x.0, z.0)],
            Vec3::Y,
        );
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
        // No surface samples a page — every row names `GpuMaterial::NO_PAGE` —
        // so every vertex carries one texture coordinate and the range is
        // degenerate on purpose.
        let uv_range = UvRange::from_uvs(&[[0.0, 0.0]]);
        let vertices: Vec<MeshVertex> = self
            .positions
            .iter()
            .zip(&self.normals)
            .map(|(position, normal)| {
                // White, so the material row is the whole of what colours a
                // surface.
                MeshVertex::from_normal(*position, *normal, [1.0; 4], [0.0, 0.0], &uv_range)
            })
            .collect();
        MeshDesc {
            label: Cow::Borrowed(label),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(mesh::vertex_bytes(&vertices)),
                uv_range,
                indices: Cow::Owned(self.indices),
                clusters,
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

/// A flat ring between an inner and an outer rectangle at height `y`, as four
/// quads: the two long sides whole, the two ends between them.
fn ring(builder: &mut MeshBuilder, y: f32, inner: [f32; 4], outer: [f32; 4]) {
    let [ix0, ix1, iz0, iz1] = inner;
    let [ox0, ox1, oz0, oz1] = outer;
    builder.floor(y, (ox0, ox1), (iz1, oz1));
    builder.floor(y, (ox0, ox1), (oz0, iz0));
    builder.floor(y, (ox0, ix0), (iz0, iz1));
    builder.floor(y, (ix1, ox1), (iz0, iz1));
}

/// The pool's inside: the two floors, the walls that climb from each to the
/// coping, and the step face the deep end shows the shallow end's floor on.
fn basin(builder: &mut MeshBuilder) {
    let length = POOL_HALF_LENGTH;
    for (x, floor) in [((-length, 0.0), DEEP_FLOOR), ((0.0, length), SHALLOW_FLOOR)] {
        builder.floor(floor, x, (POOL_FAR, POOL_NEAR));
        // The far wall faces the camera and the near one faces away from it.
        builder.quad_z(POOL_FAR, Facing::Positive, x, (floor, 0.0));
        builder.quad_z(POOL_NEAR, Facing::Negative, x, (floor, 0.0));
    }
    builder.quad_x(
        -length,
        Facing::Positive,
        (DEEP_FLOOR, 0.0),
        (POOL_FAR, POOL_NEAR),
    );
    builder.quad_x(
        length,
        Facing::Negative,
        (SHALLOW_FLOOR, 0.0),
        (POOL_FAR, POOL_NEAR),
    );
    builder.quad_x(
        0.0,
        Facing::Negative,
        (DEEP_FLOOR, SHALLOW_FLOOR),
        (POOL_FAR, POOL_NEAR),
    );
}

/// The lane line, one strip per end at that end's floor.
fn lane(builder: &mut MeshBuilder) {
    let z = (LANE_Z - LANE_HALF_WIDTH, LANE_Z + LANE_HALF_WIDTH);
    let reach = POOL_HALF_LENGTH - LANE_END_GAP;
    builder.floor(DEEP_FLOOR + LANE_LIFT, (-reach, 0.0), z);
    builder.floor(SHALLOW_FLOOR + LANE_LIFT, (0.0, reach), z);
}

/// A stub room: a floor and four walls facing in.
fn room(builder: &mut MeshBuilder) {
    let x = (-ROOM_HALF_WIDTH, ROOM_HALF_WIDTH);
    let (far, near) = ROOM_Z;
    let y = (0.0, ROOM_HEIGHT);
    builder.floor(0.0, x, (far, near));
    builder.quad_z(far, Facing::Positive, x, y);
    builder.quad_z(near, Facing::Negative, x, y);
    builder.quad_x(x.0, Facing::Positive, y, (far, near));
    builder.quad_x(x.1, Facing::Negative, y, (far, near));
}

/// Everything every scene makes resident: every mesh, every material row and
/// the empty page.
///
/// The mesh order is [`PAVING_MESH`] through [`ROOM_MESH`] and the row order is
/// [`PAVING_MATERIAL`] through [`ROOM_MATERIAL`]; both are load-bearing.
#[must_use]
pub fn desc() -> SceneDesc<'static> {
    let pool = [-POOL_HALF_LENGTH, POOL_HALF_LENGTH, POOL_FAR, POOL_NEAR];
    let coping = [
        pool[0] - COPING_WIDTH,
        pool[1] + COPING_WIDTH,
        pool[2] - COPING_WIDTH,
        pool[3] + COPING_WIDTH,
    ];
    let paving = [-PAVING_REACH, PAVING_REACH, -PAVING_REACH, PAVING_REACH];
    SceneDesc {
        meshes: vec![
            mesh_of("paving", |builder| ring(builder, 0.0, coping, paving)),
            mesh_of("coping", |builder| ring(builder, 0.0, pool, coping)),
            mesh_of("basin", basin),
            mesh_of("lane", lane),
            mesh_of("room", room),
        ],
        materials: COLORS
            .iter()
            .map(|base_color| GpuMaterial {
                base_color: *base_color,
                base_color_texture: GpuMaterial::NO_PAGE,
                roughness: ROUGHNESS,
                ..GpuMaterial::UNTINTED
            })
            .collect(),
        page: PageDesc::empty(),
        probes: crcbl::render::ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// How much of each pool [`desc`] reserves — the ceiling, not the size;
/// `the_scenes_fit_the_capacities_they_reserve` holds the description to it.
pub const CAPACITIES: Capacities = Capacities {
    vertices: 256,
    indices: 512,
    meshes: 8,
    instances: 8,
    materials: 8,
    // No punctual light: the sun is the courtyard's only light.
    lights: 1,
    // No irradiance volume, on sundial's ground: a probe grid is a second source
    // of indirect light in every band a claim reads.
    probes: 0,
};

/// The renderer every scene draws through, with nothing placed yet.
///
/// # Errors
///
/// [`HalError`] if the description does not fit what it reserves or a HAL call
/// failed.
pub fn renderer(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
) -> Result<ForwardRenderer, HalError> {
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &desc())?;
    renderer.set_sky(sky());
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Staging a scene
// ---------------------------------------------------------------------------

/// Which scene and which medium a renderer is drawing, and the objects it
/// placed for them.
///
/// The one writer of a renderer's instances and water in this sample, so what a
/// frame draws and what [`Stage::scene`] reports cannot disagree.
#[derive(Debug)]
pub struct Stage {
    scene: Scene,
    medium: Preset,
    placed: Vec<InstanceHandle>,
}

impl Stage {
    /// Places `scene` in `medium` on a renderer that has nothing placed.
    ///
    /// # Errors
    ///
    /// [`Stage::show`]'s.
    pub fn new(
        renderer: &mut ForwardRenderer,
        scene: Scene,
        medium: Preset,
    ) -> Result<Self, HalError> {
        let mut stage = Self {
            scene,
            medium,
            placed: Vec::new(),
        };
        stage.place(renderer, scene, medium)?;
        Ok(stage)
    }

    /// Which scene is placed.
    #[must_use]
    pub const fn scene(&self) -> Scene {
        self.scene
    }

    /// Which medium its water is in.
    #[must_use]
    pub const fn medium(&self) -> Preset {
        self.medium
    }

    /// Moves the renderer to `scene` in `medium`, doing nothing if it is already
    /// there.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if a body cannot be meshed or the scene
    /// does not fit [`CAPACITIES`] — mistakes in this file rather than conditions
    /// a run can be in, returned so the frame reports them rather than panicking.
    pub fn show(
        &mut self,
        renderer: &mut ForwardRenderer,
        scene: Scene,
        medium: Preset,
    ) -> Result<(), HalError> {
        if (scene, medium) == (self.scene, self.medium) {
            return Ok(());
        }
        self.place(renderer, scene, medium)
    }

    /// Sets the water first, because a refused body replaces nothing and the
    /// objects are then left as they were; then swaps the objects if the scene
    /// changed.
    fn place(
        &mut self,
        renderer: &mut ForwardRenderer,
        scene: Scene,
        medium: Preset,
    ) -> Result<(), HalError> {
        renderer.set_water(&scene.bodies(medium)).map_err(|error| {
            HalError::InvalidDescriptor(format!("tide's {} water: {error}", scene.label()))
        })?;
        self.medium = medium;
        if self.placed.is_empty() || scene != self.scene {
            for handle in self.placed.drain(..) {
                renderer.remove_instance(handle);
            }
            for &(mesh, material) in scene.objects() {
                let handle = renderer
                    .add_instance(&InstanceDesc {
                        mesh,
                        material,
                        transform: crcbl::math::Mat4::IDENTITY,
                    })
                    .map_err(|error| {
                        HalError::InvalidDescriptor(format!(
                            "tide's {} does not fit its own instance pool: {error}",
                            scene.label()
                        ))
                    })?;
                self.placed.push(handle);
            }
        }
        self.scene = scene;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The light
// ---------------------------------------------------------------------------

/// The sun: from behind the camera and above, with **no `x` component**, so the
/// deep end and the shallow end are lit alike.
///
/// **Fixed.** `apps/sundial`'s scripted clock is that sample's own module and a
/// sample does not link another, so reusing it would mean copying it; milestone 1
/// reads nothing a moving sun would add, and the rung that does — glints at a
/// grazing sun — is the one to give tide a clock.
#[must_use]
pub fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.0, 1.0, 0.3).normalize(),
        color: Vec3::new(0.60, 0.58, 0.54),
        ambient: Vec3::splat(0.03),
    }
}

/// The sky: a pale horizon over a blue zenith, which is what the far, grazing
/// half of the pool reflects.
#[must_use]
pub const fn sky() -> Sky {
    Sky {
        zenith: Vec3::new(0.03, 0.07, 0.18),
        horizon: Vec3::new(0.20, 0.21, 0.22),
        ground: Vec3::new(0.02, 0.02, 0.018),
    }
}

// ---------------------------------------------------------------------------
// The cameras
// ---------------------------------------------------------------------------

/// Where the fixed camera stands: on the pool's centre line, above the near
/// coping.
pub const FIXED_EYE: Vec3 = Vec3::new(0.0, 2.6, 3.2);

/// What it looks at: the far half of the pool's floor.
const FIXED_TARGET: Vec3 = Vec3::new(0.0, -0.8, -4.0);

/// The vertical field of view every pose shares, in radians.
const FOV_Y: f32 = 60.0 * core::f32::consts::PI / 180.0;

/// How close to the eye a surface may be and still be drawn, in metres.
const NEAR: f32 = 0.05;

/// The pose every golden is taken from, and the one the free camera starts at.
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

/// How many fixed steps one turn of the orbit takes.
pub const ORBIT_TICKS: u64 = 1200;

/// How far from the pool's centre the orbit runs, and how high.
const ORBIT_RADIUS: f32 = 11.0;
/// How high the orbit runs.
const ORBIT_HEIGHT: f32 = 4.0;

/// The orbit's pose at `tick` fixed steps into it: a pure function of the count,
/// so tick `k` is one pose in every process, on sundial's clock's terms.
#[must_use]
pub fn orbit_camera(tick: u64) -> Camera {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the remainder of one turn is under two thousand"
    )]
    let turn = (tick % ORBIT_TICKS) as f32 / ORBIT_TICKS as f32;
    let angle = turn * core::f32::consts::TAU;
    let centre = Vec3::new(0.0, LEVEL, LANE_Z);
    Camera {
        eye: centre
            + Vec3::new(
                angle.sin() * ORBIT_RADIUS,
                ORBIT_HEIGHT,
                angle.cos() * ORBIT_RADIUS,
            ),
        target: centre,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            near: NEAR,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The halves differ only in depth**: the camera and the sun have no `x`
    /// component and the body is symmetric about `x = 0` — the layout's order is
    /// a `const` assertion beside the constants. The golden suite's absorption
    /// claim rests on all of it.
    #[test]
    fn the_deep_end_and_the_shallow_end_are_mirrored_about_the_camera() {
        let camera = fixed_camera();
        assert_eq!(camera.eye.x, 0.0);
        assert_eq!(camera.target.x, 0.0);
        assert_eq!(sun().direction.x, 0.0);
        let body = pool_body(Preset::default());
        for [x, z] in &body.outline {
            assert!(
                body.outline.contains(&[-x, *z]),
                "the outline is not mirrored at {x}, {z}"
            );
        }
    }

    /// **Only the courtyard has water, and every other scene names the milestone
    /// that fills it** — which is also the whole of what a stub promises.
    #[test]
    fn only_the_courtyard_has_water_and_every_stub_names_its_milestone() {
        for scene in Scene::ALL {
            let bodies = scene.bodies(Preset::default());
            assert_eq!(Scene::from_name(scene.label()), Some(scene));
            match scene.milestone() {
                None => {
                    assert_eq!(bodies.len(), 1, "{scene:?}");
                    assert!(scene.placard().is_none());
                }
                Some(milestone) => {
                    assert!(bodies.is_empty(), "{scene:?} is a stub with water in it");
                    let placard = scene.placard().expect("a stub has a placard");
                    assert!(
                        placard.contains(&format!("milestone {milestone}")),
                        "{placard}"
                    );
                }
            }
        }
        let mut scene = Scene::default();
        for _ in 0..Scene::ALL.len() {
            scene = scene.next();
        }
        assert_eq!(
            scene,
            Scene::default(),
            "the cycle must wrap after every scene"
        );
    }

    /// **Every scene fits the capacities it reserves**, with no GPU: vertices,
    /// indices, meshes, rows, and the most objects any one scene places.
    #[test]
    fn the_scenes_fit_the_capacities_they_reserve() {
        let desc = desc();
        let (mut vertices, mut indices) = (0usize, 0usize);
        for mesh in &desc.meshes {
            let Geometry::Flat {
                vertices: bytes,
                indices: list,
                ..
            } = &mesh.geometry
            else {
                panic!("{} is not flat", mesh.label);
            };
            vertices += bytes.len() / std::mem::size_of::<MeshVertex>();
            indices += list.len();
        }
        let fits = |used: usize, capacity: u32| used <= capacity as usize;
        assert!(fits(vertices, CAPACITIES.vertices), "{vertices} vertices");
        assert!(fits(indices, CAPACITIES.indices), "{indices} indices");
        assert!(fits(desc.meshes.len(), CAPACITIES.meshes));
        assert!(fits(desc.materials.len(), CAPACITIES.materials));
        assert_eq!(desc.materials.len(), COLORS.len());
        // A switch removes the last scene's objects before it places the next
        // one's, so the pool never holds more than one scene.
        for scene in Scene::ALL {
            assert!(fits(scene.objects().len(), CAPACITIES.instances));
            for &(mesh, material) in scene.objects() {
                assert!(mesh < desc.meshes.len() && material < desc.materials.len());
            }
        }
    }

    /// **The orbit is a pure function of its tick** and comes back round.
    #[test]
    fn the_orbit_is_a_function_of_its_tick() {
        assert_eq!(orbit_camera(40).eye, orbit_camera(40).eye);
        assert_ne!(orbit_camera(40).eye, orbit_camera(41).eye);
        let start = orbit_camera(0).eye;
        let round = orbit_camera(ORBIT_TICKS).eye;
        assert!(start.distance(round) < 1e-4, "{start} against {round}");
    }
}
