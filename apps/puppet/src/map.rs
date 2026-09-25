//! The map: what the character walks on, and what it is drawn as.
//!
//! ```text
//!            +X
//!             │        ┌──────────┐  z = -9 … -3, top 0.90   the step it cannot climb
//!             │        ├──────────┤  z = -3 …  3, top 0.30   the step it can
//!  steep      │        │          │
//!  mound  ────┼────────┤  ground  ├──────── gentle mound
//!             │        │          │
//!             │        ▲ spawn, z = 16, facing −Z
//!            −X
//! ```
//!
//! # Every surface here exists to make one answer from the controller visible
//!
//! `crcbl_phys::CharacterController` decides four things a blockout can be
//! built to show, and the map is those four side by side rather than a level:
//!
//! * **A slope it will walk up**, and one it will not. The cut is
//!   [`CharacterConfig::min_ground_normal_y`](crcbl::phys::CharacterConfig::min_ground_normal_y),
//!   45° by default, and the two mounds sit either side of it —
//!   [`GENTLE_RIM_DEG`] under it and [`STEEP_RIM_DEG`] over.
//! * **A step it will climb**, and one it will not. The cut is
//!   [`CharacterConfig::step_offset`](crcbl::phys::CharacterConfig::step_offset),
//!   0.4 m by default, and the two risers in the lane are [`LOW_STEP_TOP`] and
//!   [`HIGH_STEP_RISE`] — one under it, one over.
//!
//! Colour says which is which before anything is measured: the surfaces the
//! character can take are green and blue, the ones it cannot are red and
//! orange. `web/tools/browser-e2e.mjs` asserts the pair in the lane, in the
//! browser, off the numbers the demo logs.
//!
//! # A slope is a sphere, because `crcbl-phys` has no other shape for one
//!
//! That crate's colliders are a sphere, an **axis-aligned** box and a Y-aligned
//! capsule — [`crcbl::phys::collider`] — so there is no oriented box, no wedge
//! and no triangle mesh, and a flat ramp is not a thing this engine can collide
//! against today. What it can collide against is a sphere, and a sphere cut by
//! the ground plane is a mound: a real slope, steepest at the rim and flattening
//! toward the summit, with the contact normal coming out of the same analytic
//! surface the picture is drawn from. `crcbl-phys`'s own slope tests are built
//! the same way, out of a dome — see `character.rs`'s `dome_world`.
//!
//! So the mounds are rounded rather than wedges, and that is the honest shape of
//! what the physics can do rather than a stylistic choice. `docs/backlog.md`
//! carries the collider that would change it.
//!
//! **The mesh is a tessellation of that sphere and the collider is the sphere
//! itself**, so the two disagree by the sagitta of one facet — under a
//! centimetre at the segment counts below, and toward the *inside* of the
//! collider, so the character walks a hair above the drawn surface rather than
//! sinking into it.
//!
//! # Everything else is a box, and the box is the mesh
//!
//! The ground and the two steps are [`platform`]s, whose geometry is exactly one
//! cuboid, and each one's collider is a [`BoxCollider`] over the same corners.
//! There is one set of numbers per object and both halves read it, so a map that
//! looks like it can be walked on can be.
//!
//! # The map is a file, and this module is its loader
//!
//! `assets/scenes/blockout.scn/` is that map: a header, an environment and three
//! chunk files, read through [`crcbl::scene::scn`] — the engine's own scene
//! format, the directory of chunk files `docs/notes/tooling.md` records under
//! _What the deleted 06-assets-scenes plan left behind_.
//!
//! ```text
//! assets/scenes/blockout.scn/
//!   scene.ron          format version, name, the system manifest
//!   env.ron            the view the map opens on, and the light it sits in
//!   sys/surfaces.ron   one Surface per walkable thing: ground, steps, mounds
//!   sys/spawn.ron      where the character starts, and which way it faces
//!   sys/sun.ron        the turning sun the map is shadowed by
//! ```
//!
//! The committed files are `include_str!`ed and read back through a
//! [`MemorySource`], because a browser has no filesystem and a binary that could
//! fail to find its own map is one whose picture depends on the working
//! directory it was run from. `--scene <DIR>` is the run-time door onto a
//! *different* directory, opened with a [`DirSource`] — the same [`Map::load`]
//! call either way, which is what an [`AssetSource`] is for. `apps/breakout`
//! reads its brick grid through the same pair.
//!
//! # The component is this sample's, and that is a decision
//!
//! [`Surface`] — a greybox primitive, the collider that is the same surface, and
//! the tint it is painted with — is declared here, in the sample, rather than in
//! `crcbl-greybox` or `crcbl-scene`. A chunk's component is whatever its game
//! says it is: [`chunk_of`](crcbl::scene::scn::chunk_of) bounds it by `serde` and
//! [`ComponentHash`] and by nothing else. One consumer is not an
//! engine type, and the moment a second sample wants these same rows is the
//! moment to hoist them; `apps/breakout`'s `Brick` sits on the same side of that
//! line for the same reason.
//!
//! # The constants below are the map's *generator*, not a second copy of it
//!
//! Everything the file holds was written out of them by [`Scene::save`] once,
//! and `the_committed_map_is_what_the_writer_writes` is what keeps it that way:
//! it builds the blockout from these constants, writes it, and asserts the
//! result is byte for byte the five committed files. So a hand-edited coordinate
//! is a red test rather than a map that quietly moved — and the constants go on
//! being what `crate::game`'s tests measure the controller against, which a
//! number that only existed inside a file could not be.

use std::borrow::Cow;
use std::path::Path;

use crcbl::assets::{AssetSource, DirSource, MemorySource};
use crcbl::ecs::{ComponentHash, System, World};
use crcbl::greybox::{GREYBOX_TILE_M, cube, grid_material, grid_page, platform, sphere};
use crcbl::math::{DVec3, Mat4, Vec3};
use crcbl::phys::{BoxCollider, PhysicsWorld, Sphere};
use crcbl::reflect::Reflect;
use crcbl::registry::{Placement, Registry};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError, MeshPoolError, SkinRange,
    SkinnedInstanceDesc, SkinnedMesh,
};
use crcbl::scene::scn::{Env, IdMap, Scene, ScnError};
use crcbl::serde::{Deserialize, Serialize};
use crcbl::shaders::mesh::GpuMaterial;
use crcbl::shaders::skinning::SkinBinding;

use crate::rig;

// ---------------------------------------------------------------------------
// The ground
// ---------------------------------------------------------------------------

/// How far the ground reaches from the origin on `X` and `Z`, in metres.
///
/// Wide enough that a player who walks off the course still has floor under
/// them: this sample has no fall, no respawn and no kill plane, so the edge is
/// the only thing there is to reach and it should take a while.
pub const GROUND_HALF: f64 = 24.0;

/// How thick the ground slab is, in metres. Its **top** is `y = 0`, which is
/// what every other height on this map is measured from.
pub const GROUND_THICKNESS: f64 = 1.0;

// ---------------------------------------------------------------------------
// The lane: two steps, one over the offset and one under it
// ---------------------------------------------------------------------------

/// How far the lane's surfaces reach either side of the `Z` axis, in metres.
///
/// Wide enough that a character walking down the middle cannot step off the side
/// by accident — which would take the lane's whole claim with it — and wider
/// than the circuit [`crate::game`] walks while nobody is driving, since the
/// browser gate holds a key from wherever that circuit left the character.
pub const LANE_HALF: f64 = 5.0;

/// The top of the first step, in metres above the ground.
///
/// Under the default
/// [`step_offset`](crcbl::phys::CharacterConfig::step_offset), so walking into
/// it climbs it.
pub const LOW_STEP_TOP: f64 = 0.30;

/// The near edge of the first step, in metres along `Z`. The character spawns
/// at [`SPAWN_Z`] and walks toward `-Z`, so this is the first thing it meets.
pub const LOW_STEP_NEAR_Z: f64 = 3.0;

/// The far edge of the first step, which is also the near edge of the second.
pub const LOW_STEP_FAR_Z: f64 = -3.0;

/// How much higher the second step's top is than the first's, in metres.
///
/// Over the default [`step_offset`](crcbl::phys::CharacterConfig::step_offset),
/// so walking into it does **not** climb it. Measured as a rise above
/// [`LOW_STEP_TOP`] rather than as a height above the ground, because the rise
/// is what the controller judges and a height above the ground is not.
pub const HIGH_STEP_RISE: f64 = 0.60;

/// The top of the second step, in metres above the ground.
pub const HIGH_STEP_TOP: f64 = LOW_STEP_TOP + HIGH_STEP_RISE;

/// The far edge of the second step, in metres along `Z`.
pub const HIGH_STEP_FAR_Z: f64 = -9.0;

// ---------------------------------------------------------------------------
// The mounds: two slopes, one over the walkable angle and one under it
// ---------------------------------------------------------------------------

/// Where the gentle mound stands, and how big it is: `(x, z, radius, summit)`
/// in metres, the summit measured above the ground.
///
/// **Level with the spawn**, so walking straight left or right from where the
/// character starts is a walk into a mound: the two slopes are reachable
/// without a route to remember, in the same way the lane ahead is.
///
/// A sphere of `radius` whose summit is `summit` above `y = 0` meets the ground
/// at a horizontal distance of `sqrt(radius² − (radius − summit)²)`, and the
/// slope there — the steepest anywhere on it — has a sine of that over
/// `radius`. These numbers put it at [`GENTLE_RIM_DEG`], inside the default
/// walkable angle, so the whole mound can be walked over.
pub const GENTLE_MOUND: (f64, f64, f64, f64) = (8.0, SPAWN_Z, 6.0, 1.5);

/// The gentle mound's rim angle, in degrees, for the docs and the test that
/// holds these numbers to it. **Not read by anything that runs**: the map is
/// built from [`GENTLE_MOUND`], and this is what that arithmetic comes to.
pub const GENTLE_RIM_DEG: f64 = 41.4;

/// Where the steep mound stands, in [`GENTLE_MOUND`]'s units. Its rim is at
/// [`STEEP_RIM_DEG`], outside the default walkable angle, so the character is
/// refused at the foot of it.
pub const STEEP_MOUND: (f64, f64, f64, f64) = (-8.0, SPAWN_Z, 4.0, 2.2);

/// The steep mound's rim angle, in degrees. See [`GENTLE_RIM_DEG`].
pub const STEEP_RIM_DEG: f64 = 63.3;

// ---------------------------------------------------------------------------
// The character
// ---------------------------------------------------------------------------

/// Where the character's **feet** start, in metres.
///
/// On the flat, well short of the lane, and clear of both mounds.
///
/// **The run-up is long on purpose.** The browser gate holds the walk key,
/// waits for the character to have advanced, and releases it again to check
/// that it stops — and the heartbeat it reads is a second of simulated time
/// apart, which at [`crate::game::WALK_SPEED`] is several metres. A run-up of
/// one heartbeat would let a slow machine's release land after the character
/// had already been stopped by the lane, and "it stopped" would pass without
/// meaning anything. From here it is four beats to the first riser.
pub const SPAWN: DVec3 = DVec3::new(0.0, 0.0, SPAWN_Z);

/// The `Z` half of [`SPAWN`], named because the lane's constants are measured
/// against it.
pub const SPAWN_Z: f64 = 16.0;

/// The radius of the character's capsule, in metres.
///
/// The default [`CharacterConfig`](crcbl::phys::CharacterConfig)'s, restated
/// here because the *mesh* is built from it and a mesh a different size from the
/// collider is a picture that lies about where the character is.
/// `the_character_mesh_is_the_size_of_the_capsule_that_moves_it` is what holds
/// the two together.
pub const CHARACTER_RADIUS: f64 = 0.3;

/// Half the length of the character capsule's cylindrical section, in metres.
/// See [`CHARACTER_RADIUS`].
pub const CHARACTER_HALF_HEIGHT: f64 = 0.6;

/// The character's whole height, tip to tip, in metres.
pub const CHARACTER_HEIGHT: f64 = 2.0 * (CHARACTER_HALF_HEIGHT + CHARACTER_RADIUS);

/// The edge of the block that marks which way the character is facing, in
/// metres.
///
/// A capsule is rotationally symmetric, so a body turning toward the direction
/// it is moving would be invisible without it — and turning toward the motion is
/// the demo's own job, not the controller's. See [`crate::game`].
const NOSE_EDGE: f64 = 0.18;

/// How high up the body the nose sits, in metres above the feet.
const NOSE_HEIGHT: f64 = 1.45;

// ---------------------------------------------------------------------------
// The scene directory
// ---------------------------------------------------------------------------

/// The system every walkable thing is a row of: the manifest entry, the chunk
/// file's stem, and the name [`Surface`]'s codec is registered under.
const SURFACES: &str = "surfaces";

/// The system the character's start is the one row of.
const SPAWN_POINT: &str = "spawn";

/// The system the sun is the one row of.
const SUN: &str = "sun";

/// The directory the committed map lives in, and the name its keys are spelled
/// under in the built-in source.
///
/// Public because [`built_in_source`] is, and a source whose keys nobody can
/// spell is a source nobody can read.
pub const BLOCKOUT: &str = "blockout.scn";

/// `assets/scenes/blockout.scn/scene.ron`, as it is committed.
const BLOCKOUT_SCENE_RON: &str = include_str!("../assets/scenes/blockout.scn/scene.ron");
/// `assets/scenes/blockout.scn/env.ron`, as it is committed.
const BLOCKOUT_ENV_RON: &str = include_str!("../assets/scenes/blockout.scn/env.ron");
/// `assets/scenes/blockout.scn/sys/surfaces.ron`, as it is committed.
const BLOCKOUT_SURFACES_RON: &str = include_str!("../assets/scenes/blockout.scn/sys/surfaces.ron");
/// `assets/scenes/blockout.scn/sys/spawn.ron`, as it is committed.
const BLOCKOUT_SPAWN_RON: &str = include_str!("../assets/scenes/blockout.scn/sys/spawn.ron");
/// `assets/scenes/blockout.scn/sys/sun.ron`, as it is committed.
const BLOCKOUT_SUN_RON: &str = include_str!("../assets/scenes/blockout.scn/sys/sun.ron");

/// One thing the character can walk on: a greybox primitive, the collider that
/// is the same surface, and the tint the primitive is painted with.
///
/// **Puppet's own type**, for the reason the [module docs](self) give at length:
/// a chunk's component is whatever its game says it is, and one consumer is not
/// an engine type.
///
/// **Derives [`Reflect`] as well as `Serialize`**, and the two answer different
/// questions: `Serialize` is how a row reaches `sys/surfaces.ron`, `Reflect` is
/// how it reaches an editor's property panel. This type is the one in the
/// workspace that exercises the whole derive — a `String`, two arrays, and a
/// nested enum whose rows change with the variant — which is why it is annotated
/// here rather than only in a test fixture. `docs/plan/08-editor.md` feature 3
/// is the panel that reads it.
#[derive(Clone, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Surface {
    /// What the mesh is called — the label the renderer holds it under, and what
    /// a frame dump names it in.
    #[reflect(name = "Label")]
    pub label: String,
    /// Where the primitive's own origin sits, in metres.
    ///
    /// `f64` and both halves read it: this is the number the physics world is
    /// spelled in — [`BoxCollider`] and [`Sphere`] take [`DVec3`] — and a map
    /// written as `f32` would round on the way through the file and put the
    /// collider somewhere the mesh is not.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
    /// What it is, which decides the mesh and the collider together.
    #[reflect(name = "Shape")]
    pub shape: Shape,
    /// Linear RGB the greybox grid is tinted with, through this module's
    /// `painted`.
    ///
    /// The range is what a linear RGB channel is: the tint is multiplied into
    /// the grid page rather than added to it, so nothing outside `0..=1` means
    /// anything.
    #[reflect(name = "Tint", min = 0.0, max = 1.0, step = 0.01)]
    pub tint: [f32; 3],
}

/// The two primitives this map is built from.
///
/// Each carries the collider that is the **same** surface the mesh draws, which
/// is the whole claim [`Map::world`] and [`Map::place`] make together: there is
/// one set of numbers per object and the two halves both read it.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub enum Shape {
    /// A cuboid standing on the primitive's origin: [`platform`]'s geometry, and
    /// a [`BoxCollider`] over the same eight corners.
    Platform {
        /// Its extent along `X`, in metres.
        #[reflect(name = "Width", min = 0.0, max = 64.0, step = 0.1)]
        width: f64,
        /// Its extent along `Z`, in metres.
        #[reflect(name = "Depth", min = 0.0, max = 64.0, step = 0.1)]
        depth: f64,
        /// How far it rises above the origin, in metres.
        #[reflect(name = "Height", min = 0.0, max = 64.0, step = 0.1)]
        height: f64,
    },
    /// A sphere centred on the primitive's origin: [`sphere`]'s tessellation at
    /// `MOUND_RINGS` by `MOUND_SEGMENTS`, and the analytic [`Sphere`] itself as
    /// the collider.
    ///
    /// Sunk far enough that what stands above the ground is a mound, which is
    /// how this map has a slope at all — see the [module docs](self).
    Dome {
        /// Its radius, in metres.
        #[reflect(name = "Radius", min = 0.0, max = 64.0, step = 0.1)]
        radius: f64,
    },
}

impl ComponentHash for Surface {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        // The label's length before its bytes: two rows whose labels are "lo"
        // and "wstep" hash the same as "low" and "step" without it.
        hasher.write_usize(self.label.len());
        hasher.write(self.label.as_bytes());
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
        match self.shape {
            Shape::Platform {
                width,
                depth,
                height,
            } => {
                hasher.write_u8(0);
                for value in [width, depth, height] {
                    hasher.write(&value.to_bits().to_le_bytes());
                }
            }
            Shape::Dome { radius } => {
                hasher.write_u8(1);
                hasher.write(&radius.to_bits().to_le_bytes());
            }
        }
        for value in self.tint {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// The box a surface occupies, which is the box its collider already is for a
/// [`Shape::Platform`] and the box its sphere is inscribed in for a
/// [`Shape::Dome`].
///
/// [`Map::world`] reads the platform arm of this rather than repeating it, so
/// there is one statement of where a platform's centre is. The dome arm is not
/// shared: a collider there is the analytic [`Sphere`], and this is the box a
/// tool draws and picks it by.
impl Placement for Surface {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        let origin = DVec3::from_array(self.position);
        Some(match self.shape {
            // A `platform` stands *on* its origin, so the centre is half a
            // height up.
            Shape::Platform {
                width,
                depth,
                height,
            } => (
                origin + DVec3::new(0.0, 0.5 * height, 0.0),
                DVec3::new(0.5 * width, 0.5 * height, 0.5 * depth),
            ),
            Shape::Dome { radius } => (origin, DVec3::splat(radius)),
        })
    }
}

/// Where the character starts, and which way it is turned when it gets there.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Spawn {
    /// The **feet**, in metres. See [`SPAWN`], which is what the committed row
    /// was written from.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
    /// The yaw the body is turned to, in radians about `+Y`, measured the way
    /// [`crate::camera`] measures one: zero looks down `-Z`.
    ///
    /// A step and no range: a yaw is periodic, so there is no end for a widget
    /// to clamp to — and `#[reflect(min = …)]` takes a literal rather than an
    /// expression, so a range here could not have named `std::f64::consts::TAU`
    /// even if one were wanted.
    #[reflect(name = "Facing", step = 0.01)]
    pub facing: f64,
}

impl ComponentHash for Spawn {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        for value in self.position.iter().chain(&[self.facing]) {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// The volume the character occupies when it stands here: the capsule
/// [`CHARACTER_RADIUS`] and [`CHARACTER_HEIGHT`] describe, standing on the feet
/// this row spells.
///
/// The body rather than a marker of some size picked to look right — a spawn
/// point a tool draws as the thing that will stand in it is one a person can see
/// is clipping into a wall.
impl Placement for Spawn {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        let half_height = 0.5 * CHARACTER_HEIGHT;
        Some((
            DVec3::from_array(self.position) + DVec3::new(0.0, half_height, 0.0),
            DVec3::new(CHARACTER_RADIUS, half_height, CHARACTER_RADIUS),
        ))
    }
}

/// The sun the map is lit and shadowed by, as the file spells it.
///
/// The **ambient** is not here: it is `env.ron`'s, which is the format's own
/// slot for "the light a scene sits in when nothing else reaches it", and a
/// second copy of it in this row would be a second thing to keep in step. See
/// [`Map::sun`], which puts the two together.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Sun {
    /// How high it stands, as the `+Y` component of the unit vector toward it.
    /// See [`SUN_ELEVATION`]. The range is what a component of a unit vector
    /// can be.
    #[reflect(name = "Elevation", min = -1.0, max = 1.0, step = 0.01)]
    pub elevation: f32,
    /// Its colour, before the intensity below is applied to it. Linear RGB, so
    /// the range is a channel's, and the brightness above one is
    /// [`intensity`](Self::intensity)'s job rather than this field's.
    #[reflect(name = "Colour", min = 0.0, max = 1.0, step = 0.01)]
    pub color: [f32; 3],
    /// How bright it is. Above 1.0, like every other sun in this engine — see
    /// [`SUN_INTENSITY`].
    #[reflect(name = "Intensity", min = 0.0, max = 16.0, step = 0.05)]
    pub intensity: f32,
    /// How long it takes to come back round to where it started, in seconds.
    /// See [`SUN_PERIOD`].
    #[reflect(name = "Period", min = 0.1, max = 3600.0, step = 0.5)]
    pub period: f64,
}

impl ComponentHash for Sun {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hasher.write(&self.elevation.to_bits().to_le_bytes());
        for value in self.color {
            hasher.write(&value.to_bits().to_le_bytes());
        }
        hasher.write(&self.intensity.to_bits().to_le_bytes());
        hasher.write(&self.period.to_bits().to_le_bytes());
    }
}

/// **A sun is not a thing in space.** It is a direction, a colour and a rate, and
/// the row is the whole of it — so an outliner lists it, a property panel edits
/// it, and a ray cannot hit it.
///
/// [`None`] rather than a box at the origin, which would put a pickable cube in
/// the middle of every map and make "the sun is selected" a thing a person
/// reached by accident.
impl Placement for Sun {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        None
    }
}

/// This sample's scene vocabulary: three components, under the names their chunk
/// files are spelled with.
///
/// **The one place those three names are joined to their types.** [`Map::load`]
/// uses it and so does any tool that opens this map, so the vocabulary the sample
/// ships and the vocabulary an editor sees are the same list rather than two that
/// agree today — `docs/plan/08-editor.md`'s component registry.
pub fn register_components(registry: &mut Registry) {
    registry.register::<Surface>(SURFACES);
    registry.register::<Spawn>(SPAWN_POINT);
    registry.register::<Sun>(SUN);
}

/// Why a directory is not one of puppet's maps.
///
/// [`ScnError`] is the format's half and says which *key* it is about; the other
/// two are this sample's, and say which *chunk* a puppet map is missing. A
/// manifest that simply does not name one of the three would otherwise load as
/// a map with no ground on it.
#[derive(Debug)]
pub enum MapError {
    /// The directory is not a scene, or a chunk in it would not read.
    Scene(ScnError),
    /// The manifest does not name one of the systems a puppet map is made of.
    Missing(&'static str),
    /// A chunk that holds exactly one row holds some other number of them.
    NotOne {
        /// Which system, which is also the file: `sys/<system>.ron`.
        system: &'static str,
        /// How many rows it actually holds.
        found: usize,
    },
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "{error}"),
            Self::Missing(system) => write!(
                f,
                "the manifest names no `{system}` chunk, which every puppet map has"
            ),
            Self::NotOne { system, found } => write!(
                f,
                "`sys/{system}.ron` holds {found} entities, and a puppet map has exactly one"
            ),
        }
    }
}

impl std::error::Error for MapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            Self::Missing(_) | Self::NotOne { .. } => None,
        }
    }
}

/// A blockout: its surfaces in the order the chunk spells them, where the
/// character starts, and the sun it stands under.
///
/// Not `Eq`: every number in it is a float. [`PartialEq`] is what
/// [`crate::Options`] needs and all a test comparing two parses of one directory
/// wants.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    env: Env,
    surfaces: Vec<Surface>,
    spawn: Spawn,
    sun: Sun,
}

impl Map {
    /// The committed `assets/scenes/blockout.scn/`, parsed.
    ///
    /// # Panics
    ///
    /// If the committed directory is not a map, naming the key and the line and
    /// column in it. It is compiled into this binary, so that is a tree in which
    /// `the_committed_map_is_what_the_writer_writes` is red as well — the panic
    /// is what stops a run starting on a map nobody could read.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(&built_in_source(), Path::new(BLOCKOUT))
            .unwrap_or_else(|error| panic!("apps/puppet/assets/scenes/{BLOCKOUT}: {error}"))
    }

    /// The map `dir` holds, read through `source`.
    ///
    /// # Errors
    ///
    /// [`MapError`]: a key that is not there, text that is not this format, a
    /// header this build does not read, a manifest naming a system with no codec
    /// — or one that leaves out a system a puppet map is made of.
    pub fn load(source: &dyn AssetSource, dir: &Path) -> Result<Self, MapError> {
        let mut registry = Registry::new();
        register_components(&mut registry);
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (scene, ids) =
            Scene::load(source, dir, &registry.codecs(), &mut world).map_err(MapError::Scene)?;

        // A manifest that names a system with no codec is `Scene::load`'s
        // refusal; a manifest that names *fewer* systems is not an error to the
        // format at all, and this is where it becomes one.
        for system in [SURFACES, SPAWN_POINT, SUN] {
            if !scene.systems().iter().any(|named| named == system) {
                return Err(MapError::Missing(system));
            }
        }

        let surfaces = rows::<Surface>(&mut world, &ids);
        let spawn = only(rows::<Spawn>(&mut world, &ids), SPAWN_POINT)?;
        let sun = only(rows::<Sun>(&mut world, &ids), SUN)?;
        Ok(Self {
            env: *scene.env(),
            surfaces,
            spawn,
            sun,
        })
    }

    /// The map the directory at `path` holds, or the message to refuse the run
    /// with.
    ///
    /// Both failures read the same way — the path, then what went wrong with it
    /// — because to a person fixing it "no such file" and "line 3, column 5" are
    /// the same kind of answer about the same argument.
    ///
    /// # Errors
    ///
    /// The refusal message, ready to print.
    pub fn read_dir(path: &str) -> Result<Self, String> {
        // Rooted at the scene directory itself and read with an empty prefix:
        // `DirSource` refuses an absolute key and a `..`, so the root is how a
        // caller says where the scene is.
        let source = DirSource::at(std::path::PathBuf::from(path));
        Self::load(&source, Path::new("")).map_err(|error| format!("{path}: {error}"))
    }

    /// The surfaces, in the order the chunk file spells them — which is the
    /// order their meshes, their painted rows and their colliders are in.
    #[must_use]
    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    /// Where the character's **feet** start.
    #[must_use]
    pub fn spawn(&self) -> DVec3 {
        DVec3::from_array(self.spawn.position)
    }

    /// The yaw the body starts turned to. See [`Spawn::facing`].
    #[must_use]
    pub fn facing(&self) -> f64 {
        self.spawn.facing
    }

    /// The view the map opens on and the light it sits in, as `env.ron` holds
    /// them.
    ///
    /// The camera half is not read by anything that runs: [`crate::camera`]
    /// rebuilds a follow camera every frame from wherever the character is, and
    /// a scene's opening eye is not that. It is written from the same constants
    /// that camera is built out of, and the writer test is what keeps the two
    /// from drifting apart.
    #[must_use]
    pub fn env(&self) -> &Env {
        &self.env
    }
}

/// One system's rows, in the file's own id order.
///
/// Sorted by [`SceneEntityId`](crcbl::scene::scn::SceneEntityId) rather than taken in
/// storage order, so the map is
/// built in the order the chunk spells it however the ECS happened to lay the
/// rows out.
///
/// Reached by component type and not by name, which [`World::system_mut`] can do
/// safely here because the three systems [`Map::load`] registers hold three
/// different types. The format itself reaches a chunk by name — see
/// [`crcbl::scene::scn`] — for the case this map does not have: two systems of
/// one component.
fn rows<T>(world: &mut World, ids: &IdMap) -> Vec<T>
where
    T: Clone + ComponentHash + 'static,
{
    let system = world
        .system_mut::<System<T>>()
        .expect("the system `Map::load` registered is in the world it registered it in");
    let mut rows: Vec<_> = system
        .iter_entities()
        .map(|(entity, row)| (ids.id(entity), row.clone()))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The single row of a system that has exactly one, or the refusal that names
/// its file.
fn only<T>(rows: Vec<T>, system: &'static str) -> Result<T, MapError> {
    let found = rows.len();
    rows.into_iter()
        .next()
        .filter(|_| found == 1)
        .ok_or(MapError::NotOne { system, found })
}

/// The committed scene directory, as a source with no filesystem under it,
/// keyed under [`BLOCKOUT`].
///
/// Public so that a **tool** can open this sample's map without one: `.scn/` is
/// the engine's own scene format, and a tool that had to find
/// `apps/puppet/assets/` on disk would be one whose behaviour depended on the
/// directory it was started from. The sample reads it through [`Map::built_in`],
/// which is this source and the loader over it.
#[must_use]
pub fn built_in_source() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, text) in [
        ("scene.ron", BLOCKOUT_SCENE_RON),
        ("env.ron", BLOCKOUT_ENV_RON),
        ("sys/surfaces.ron", BLOCKOUT_SURFACES_RON),
        ("sys/spawn.ron", BLOCKOUT_SPAWN_RON),
        ("sys/sun.ron", BLOCKOUT_SUN_RON),
    ] {
        // `format!` and not `Path::join`: an asset key is `/`-separated on every
        // host, and a key joined on Windows would not be one anywhere else.
        source
            .insert(
                Path::new(&format!("{BLOCKOUT}/{key}")),
                text.as_bytes().to_vec(),
            )
            .expect("a nested scene key is a legal asset key");
    }
    source
}

// ---------------------------------------------------------------------------
// The scene description
// ---------------------------------------------------------------------------

/// How many rings and segments the mounds are tessellated with.
///
/// Chosen against the collider rather than against a triangle budget: the mesh
/// sits inside the sphere by the sagitta of one facet, which at these counts is
/// under a centimetre on the larger mound. See the module docs.
const MOUND_RINGS: u32 = 32;
/// See [`MOUND_RINGS`].
const MOUND_SEGMENTS: u32 = 48;

/// The tint the character itself is painted with, which is the one material row
/// this map does not read out of a file.
///
/// The character is not a surface: it is [`crate::rig`]'s boxes and the nose
/// block, and a `.scn/` that had to carry its colour would be a scene file
/// describing something no scene of this sample can be without.
const BODY_TINT: [f32; 3] = [0.82, 0.68, 0.26];

/// What this map reserves, which is a little over what it places.
///
/// Sized against the description rather than left at
/// [`Capacities::default`]: that default reserves far more instances
/// for a blockout that places seven, and the level-of-detail state behind that
/// number is a word per instance per draw generator. Filling any of these is a
/// mistake in this file, and the numbers being close to what it uses is what
/// makes that true.
// A skinned part costs **three** runs of its own vertex count in the pool: the
// resident bind pose, and the two halves of the ping-pong the dispatch writes
// into. The character's boxes are a few dozen vertices each beside the mounds'
// thousands, so the headroom below covers them without moving; `place` returns
// the pool's refusal rather than this file guessing, and
// `the_map_fits_the_pools_it_reserves` is what asserts it fits.
const CAPACITIES: Capacities = Capacities {
    vertices: 32 * 1024,
    indices: 128 * 1024,
    meshes: 16,
    instances: 64,
    materials: 16,
    lights: 16,
    probes: 0,
};

/// A painted greybox material: the metric grid of [`grid_page`], tinted, and
/// tiled **physically** so one tile measures [`GREYBOX_TILE_M`] of surface
/// however large the face is.
///
/// The tint is this map's own and the grid is the engine's. Physical tiling
/// rather than the authored kind [`grid_material`] comes with, because these
/// surfaces are metres across and an authored `0..1` tile would stretch one
/// square over the whole of the ground — see `crcbl_greybox::material`, which
/// makes the same distinction and spends a 1024²-texel page on it. This spends
/// the 32² grid page instead, because a demo that runs in a browser should not
/// upload eight megatexels to show a ruler.
fn painted(tint: [f32; 3]) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..grid_material()
    }
}

impl Map {
    /// Where the character's own meshes start: one slot per [`crate::rig::parts`]
    /// entry, in that list's order, straight after the surfaces the file names.
    ///
    /// A base rather than a constant per limb, because the parts are a `rig` list
    /// and this file must not be a second copy of it — and a *method* rather than
    /// a constant, because how many slots come before it is now the chunk file's
    /// answer and not this module's.
    #[must_use]
    pub fn character_mesh_base(&self) -> usize {
        self.surfaces.len()
    }

    /// The block on the front of the body, which is how its facing is read.
    #[must_use]
    pub fn nose_mesh(&self) -> usize {
        self.character_mesh_base() + rig::PARTS
    }

    /// The character's own material row — this module's `BODY_TINT`, the one
    /// that is not a surface's.
    #[must_use]
    pub fn body_material(&self) -> usize {
        self.surfaces.len()
    }

    /// Everything this map makes resident: a mesh per surface, then the
    /// character's limbs and its nose; a painted row per surface, then the body;
    /// and the grid page they all sample.
    ///
    /// Mesh slot `i` and material slot `i` are surface `i`, in the order
    /// `sys/surfaces.ron` spells them, which is what
    /// [`Map::place`] relies on to place each one through its own colour.
    #[must_use]
    pub fn scene(&self) -> SceneDesc<'static> {
        SceneDesc {
            meshes: self
                .surfaces
                .iter()
                .map(|surface| MeshDesc {
                    label: Cow::Owned(surface.label.clone()),
                    geometry: surface.geometry(),
                })
                .chain(rig::parts().into_iter().map(|part| MeshDesc {
                    label: Cow::Borrowed(part.label),
                    geometry: part.geometry,
                }))
                .chain([MeshDesc {
                    label: Cow::Borrowed("nose"),
                    geometry: cube(NOSE_EDGE as f32),
                }])
                .collect(),
            materials: self
                .surfaces
                .iter()
                .map(|surface| painted(surface.tint))
                .chain([painted(BODY_TINT)])
                .collect(),
            page: grid_page(),
            probes: ProbeGrid::default(),
            capacities: CAPACITIES,
        }
    }
}

impl Surface {
    /// The geometry this surface is drawn as.
    ///
    /// Narrowed to `f32` here and only here: the file and the collider are `f64`
    /// because the physics world is, and the vertex pool is not.
    fn geometry(&self) -> Geometry<'static> {
        match self.shape {
            Shape::Platform {
                width,
                depth,
                height,
            } => platform(width as f32, depth as f32, height as f32),
            Shape::Dome { radius } => sphere(radius as f32, MOUND_RINGS, MOUND_SEGMENTS),
        }
    }

    /// Where its origin sits, as the renderer wants it.
    fn origin(&self) -> Vec3 {
        Vec3::new(
            self.position[0] as f32,
            self.position[1] as f32,
            self.position[2] as f32,
        )
    }
}

/// The character, as the renderer holds it: one skinned instance per limb and
/// the block that says which way it is facing.
///
/// Handed back by [`Map::place`] because everything else on this map is written
/// once and never again, and these are rewritten every frame from wherever the
/// simulation put the character and whatever pose [`crate::anim`] put it in.
///
/// Not `Copy`, which the capsule it replaced was: a [`SkinnedMesh`] owns two
/// runs of the vertex pool, and a second copy of one would be a second thing
/// trying to give them back.
#[derive(Debug)]
pub struct Character {
    parts: Vec<Limb>,
    nose: InstanceHandle,
    /// The mesh slot the nose was made resident in, and the material row the
    /// whole body shades through.
    ///
    /// Carried rather than recomputed, because both are a function of how many
    /// surfaces the *loaded* map has — see [`Map::nose_mesh`] — and a character
    /// that read them from a different map than the one it was placed on would
    /// draw somebody else's mesh in somebody else's colour.
    nose_mesh: usize,
    body_material: usize,
}

/// One drawn limb: its reserved region of the vertex pool, the instance that
/// draws it, and the skin the dispatch reads.
#[derive(Debug)]
struct Limb {
    mesh: SkinnedMesh,
    instance: InstanceHandle,
    bindings: Vec<SkinBinding>,
}

impl Character {
    /// Points the drawn character at `position` — the **centre** of the
    /// controller's capsule — turned to `facing` radians about `+Y`.
    ///
    /// `facing` is measured the way [`crate::camera`] measures a yaw: zero looks
    /// down `-Z`, which is where the engine's default camera looks.
    ///
    /// Every limb takes the *same* transform. The joints are not in it —
    /// §3.7.4.2 of glTF's own wording, which
    /// [`Palette`](crcbl::anim::Palette) restates: only the joint transforms
    /// deform a skinned mesh, and where the character stands is an ordinary
    /// instance placement on top of that.
    pub fn place_at(&self, renderer: &mut ForwardRenderer, position: DVec3, facing: f64) {
        // The capsule's centre is what the controller holds; the *rig* rests its
        // feet on `y = 0`, as everything standing in the greybox pack does. So
        // the body is drawn from the feet, which is a half-height and a radius
        // below the centre.
        let feet = Vec3::new(
            position.x as f32,
            (position.y - (CHARACTER_RADIUS + CHARACTER_HALF_HEIGHT)) as f32,
            position.z as f32,
        );
        let body = Mat4::from_translation(feet) * Mat4::from_rotation_y(facing as f32);
        for limb in &self.parts {
            renderer.set_skinned_instance(
                limb.instance,
                &SkinnedInstanceDesc {
                    mesh: &limb.mesh,
                    material: self.body_material,
                    transform: body,
                },
            );
        }
        renderer.set_instance(
            self.nose,
            &InstanceDesc {
                mesh: self.nose_mesh,
                material: self.body_material,
                // Just clear of the torso, on the body's own forward axis — so
                // this is where the rotation above becomes visible.
                transform: body
                    * Mat4::from_translation(Vec3::new(
                        0.0,
                        NOSE_HEIGHT as f32,
                        -(CHARACTER_RADIUS + 0.5 * NOSE_EDGE) as f32,
                    )),
            },
        );
    }

    /// What the skinning dispatch is asked to write this frame: one range per
    /// limb, every one of them reading the same `palette`.
    ///
    /// One palette for every range because there is one skeleton — the limbs
    /// are separate meshes of one character, not separate characters.
    #[must_use]
    pub fn ranges<'a>(&'a self, palette: &'a [Mat4]) -> Vec<SkinRange<'a>> {
        self.parts
            .iter()
            .map(|limb| limb.mesh.skin_range(palette, &limb.bindings))
            .collect()
    }

    /// How many skinned ranges, palette matrices and skin bindings a
    /// [`Skinning`](crcbl::render::Skinning) has to hold for this character.
    ///
    /// Counted off what was actually reserved rather than restated from
    /// [`crate::rig`], so a limb added there cannot be one the pass has no room
    /// for.
    #[must_use]
    pub fn skinning_capacities(&self) -> (u32, u32, u32) {
        let ranges = u32::try_from(self.parts.len()).expect("a handful of limbs");
        let bindings = self
            .parts
            .iter()
            .map(|limb| limb.mesh.vertex_count())
            .sum::<u32>();
        // Every range carries the whole palette, so the joint budget is the
        // skeleton's length once per range and not once altogether.
        let joints = ranges * u32::try_from(rig::JOINTS).expect("a nine-joint rig");
        (ranges, joints, bindings)
    }

    /// Gives the vertex-pool runs back, which has to happen before the renderer
    /// that owns the pool is destroyed.
    pub fn release(self, renderer: &mut ForwardRenderer) {
        for limb in self.parts {
            renderer.release_skinned(limb.mesh);
        }
    }
}

/// Why the map could not be placed.
///
/// Two pools can refuse it and they refuse differently: the instance pool holds
/// what is drawn, and the vertex pool holds the runs a skinned limb is written
/// into. Both are this file's numbers being wrong rather than a condition a run
/// can be in, but it is the caller that has to report one.
#[derive(Debug)]
pub enum PlaceError {
    /// `CAPACITIES`'s instance count does not cover the map.
    Instances(InstancePoolError),
    /// `CAPACITIES`'s vertex count does not cover the character's skinned
    /// regions, which are two runs per limb on top of the resident bind pose.
    Regions(MeshPoolError),
}

impl std::fmt::Display for PlaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instances(error) => write!(f, "the instance pool refused the map: {error}"),
            Self::Regions(error) => {
                write!(f, "the vertex pool refused the character's limbs: {error}")
            }
        }
    }
}

impl std::error::Error for PlaceError {}

impl From<InstancePoolError> for PlaceError {
    fn from(error: InstancePoolError) -> Self {
        Self::Instances(error)
    }
}

impl From<MeshPoolError> for PlaceError {
    fn from(error: MeshPoolError) -> Self {
        Self::Regions(error)
    }
}

impl Map {
    /// Places every object on the map and hands back the character.
    ///
    /// Surface `i` is placed through mesh slot `i` and painted row `i`, which is
    /// what [`Map::scene`] made resident in that order.
    ///
    /// # Errors
    ///
    /// [`PlaceError`] if either pool is smaller than this map — see that type.
    /// Anything reserved before the refusal is given back, so a caller that
    /// reports the error and tears the renderer down leaks nothing.
    pub fn place(&self, renderer: &mut ForwardRenderer) -> Result<Character, PlaceError> {
        // A `platform` rises from its own origin and a `sphere` is centred on
        // one, so the drop that puts the ground's top at `y = 0` and the sink
        // that turns a sphere into a mound are both in the position the chunk
        // file holds — there is nothing left to do here but translate.
        for (mesh, surface) in self.surfaces.iter().enumerate() {
            renderer.add_instance(&InstanceDesc {
                mesh,
                material: mesh,
                transform: Mat4::from_translation(surface.origin()),
            })?;
        }

        // The character last, and at the identity: `Character::place_at` writes
        // it all before the first frame is drawn, from the simulation's own
        // position rather than from a copy of the spawn kept here.
        //
        // A limb is *not* also added as an ordinary instance. It would be drawn
        // twice — once deformed and once at the bind pose, in the same place —
        // and the second copy is the one that would still be there if the
        // dispatch stopped running.
        let body_material = self.body_material();
        let mut parts = Vec::with_capacity(rig::PARTS);
        for (index, part) in rig::parts().into_iter().enumerate() {
            let placed = reserve_limb(
                renderer,
                self.character_mesh_base() + index,
                body_material,
                part.bindings,
            );
            match placed {
                Ok(limb) => parts.push(limb),
                Err(error) => {
                    for limb in parts {
                        renderer.release_skinned(limb.mesh);
                    }
                    return Err(error);
                }
            }
        }
        let nose_mesh = self.nose_mesh();
        let nose = renderer.add_instance(&InstanceDesc {
            mesh: nose_mesh,
            material: body_material,
            transform: Mat4::IDENTITY,
        })?;
        Ok(Character {
            parts,
            nose,
            nose_mesh,
            body_material,
        })
    }
}

/// Reserves one limb's two vertex-pool runs and the instance that draws them.
fn reserve_limb(
    renderer: &mut ForwardRenderer,
    mesh: usize,
    material: usize,
    bindings: Vec<SkinBinding>,
) -> Result<Limb, PlaceError> {
    let skinned = renderer.reserve_skinned(mesh)?;
    let instance = renderer.add_skinned_instance(&SkinnedInstanceDesc {
        mesh: &skinned,
        material,
        transform: Mat4::IDENTITY,
    });
    match instance {
        Ok(instance) => Ok(Limb {
            mesh: skinned,
            instance,
            bindings,
        }),
        Err(error) => {
            renderer.release_skinned(skinned);
            Err(PlaceError::Instances(error))
        }
    }
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

impl Map {
    /// The same map, as the colliders the character sweeps against.
    ///
    /// Every one of them comes out of the same [`Surface`] row its mesh does, so
    /// there is one set of numbers per object and both halves read it: a
    /// [`Shape::Platform`] is a [`BoxCollider`] over the cuboid `platform` draws,
    /// and a [`Shape::Dome`] is the analytic [`Sphere`] its tessellation
    /// approximates.
    #[must_use]
    pub fn world(&self) -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        for surface in &self.surfaces {
            let origin = DVec3::from_array(surface.position);
            match surface.shape {
                // A `BoxCollider` is a centre and half-extents, which is exactly
                // what `Placement` answers — and a `platform` stands *on* its
                // origin, so that centre is half a height up. Read from there
                // rather than repeated, so the collider and the box a tool draws
                // cannot disagree.
                Shape::Platform { .. } => {
                    let (centre, half_extents) =
                        surface.placement().expect("a surface is a thing in space");
                    world.add_box(BoxCollider::new(centre, half_extents));
                }
                Shape::Dome { radius } => {
                    world.add_sphere(Sphere::new(origin, radius));
                }
            }
        }
        world
    }
}

/// The steepest slope anywhere on a mound, in **radians**: the angle its surface
/// makes with the horizontal where it meets the ground.
///
/// A sphere of `radius` whose summit stands `summit` above `y = 0` meets the
/// plane at a horizontal distance of `sqrt(radius² − (radius − summit)²)`, and
/// the surface normal there leans from vertical by the arcsine of that over
/// `radius`. Everything further in is shallower, so this one angle decides
/// whether the whole mound can be walked on.
#[must_use]
pub fn rim_angle(radius: f64, summit: f64) -> f64 {
    let ground_radius = (radius * radius - (radius - summit).powi(2)).sqrt();
    (ground_radius / radius).asin()
}

// ---------------------------------------------------------------------------
// The light
// ---------------------------------------------------------------------------

/// How bright the sun is, before its colour.
///
/// Above 1.0, like every other sun in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back.
pub const SUN_INTENSITY: f32 = 2.2;

/// The sun's own colour, before [`SUN_INTENSITY`]: barely warm daylight.
pub const SUN_COLOR: [f32; 3] = [1.0, 0.97, 0.90];

/// The light a face no sun reaches is left with, linear RGB — `env.ron`'s
/// `ambient`, and what [`Map::sun`] hands the renderer as such.
///
/// Small and cool, standing for the sky. A black face would make every shadow a
/// measurement of an unpainted frame.
pub const SUN_AMBIENT: [f32; 3] = [0.10, 0.11, 0.14];

/// How high the sun stands, as the `+Y` component of the unit vector toward it.
///
/// Chosen for the shadows rather than for the light: high enough that a shadow
/// is a shape under the thing casting it rather than a stripe across the whole
/// map, low enough that it is a shape at all. A sun directly overhead would put
/// the character's shadow under its own feet, where nothing can see it.
pub const SUN_ELEVATION: f32 = 0.78;

/// How long the sun takes to come back round to where it started, in seconds.
///
/// **It turns, and that is not decoration.** Milestone 1's whole subject is the
/// map "with shadows already on", and a shadow that never moves is
/// indistinguishable from a dark patch painted on the ground — which is the one
/// thing an eyeball test for "does it read as grounded" must not be fooled by.
/// A turn of a few degrees a second is slow enough to be weather and fast
/// enough that every shadow on the map is visibly a shadow.
///
/// It is also the only thing in this sample that moves while nobody is playing
/// and nothing is walking, which `web/tools/browser-e2e.mjs`'s "the canvas
/// changes between frames" reads: the character is kinematic, so a released key
/// is a still character, and after that gate has driven the walk there would be
/// nothing left on screen in motion. `apps/lantern`'s orbiting lamp is the same
/// answer to the same question.
pub const SUN_PERIOD: f64 = 45.0;

impl Map {
    /// The sun the map is lit and shadowed by, `seconds` into the run.
    ///
    /// **The direction is the vector *towards* the light.** It stands at the
    /// file's own fixed elevation and swings once round the compass every
    /// [`Sun::period`], so the character's shadow sweeps across the ground it is
    /// standing on.
    ///
    /// A pure function of the time, and the time is the **simulation's** rather
    /// than a wall clock — see [`crate::game`] — so a frame at `t` is the same
    /// frame on every machine and a paused demo's shadows stop where they are.
    ///
    /// The ambient is `env.ron`'s, standing for the sky: small and cool, and
    /// large enough that a face no light reaches is dark rather than black,
    /// since a black face makes every shadow a measurement of an unpainted
    /// frame.
    #[must_use]
    pub fn sun(&self, seconds: f64) -> DirectionalLight {
        let sun = self.sun;
        let angle = core::f64::consts::TAU * (seconds / sun.period);
        #[allow(clippy::cast_possible_truncation)]
        let (sin, cos) = (angle.sin() as f32, angle.cos() as f32);
        // The horizontal part is what turns; the elevation is fixed, so the
        // normalisation below is a constant and the sun neither rises nor sets.
        let flat = (1.0 - sun.elevation * sun.elevation).sqrt();
        DirectionalLight {
            direction: Vec3::new(flat * sin, sun.elevation, flat * cos),
            color: Vec3::from_array(sun.color) * sun.intensity,
            ambient: Vec3::from_array(self.env.ambient),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::scene::scn::SystemChunk;

    use crcbl::phys::Aabb;
    use crcbl::reflect::{Kind, Value, get_path, set_path};
    use crcbl::scene::scn::EnvCamera;

    /// **The blockout as this module built it before it was a file**, written in
    /// the same expressions the old `scene()` and `world()` used and in the same
    /// order.
    ///
    /// This is the truth the committed directory is held to, from both sides:
    /// `the_committed_map_is_what_the_writer_writes` generates the five files out
    /// of it, and `the_committed_map_parses_to_the_blockout_this_module_builds`
    /// reads them back. Written from the constants rather than as literals, so a
    /// change to one of them moves the map and the expectation together — which
    /// is what those constants are for.
    fn the_blockout() -> Vec<Surface> {
        let (gentle_x, gentle_z, gentle_radius, gentle_summit) = GENTLE_MOUND;
        let (steep_x, steep_z, steep_radius, steep_summit) = STEEP_MOUND;
        vec![
            // A `platform` rises from `y = 0`, so the ground is dropped by its
            // own thickness to put its top there.
            Surface {
                label: "ground".to_string(),
                position: [0.0, -GROUND_THICKNESS, 0.0],
                shape: Shape::Platform {
                    width: 2.0 * GROUND_HALF,
                    depth: 2.0 * GROUND_HALF,
                    height: GROUND_THICKNESS,
                },
                tint: [0.30, 0.31, 0.33],
            },
            Surface {
                label: "low step".to_string(),
                position: [0.0, 0.0, 0.5 * (LOW_STEP_NEAR_Z + LOW_STEP_FAR_Z)],
                shape: Shape::Platform {
                    width: 2.0 * LANE_HALF,
                    depth: LOW_STEP_NEAR_Z - LOW_STEP_FAR_Z,
                    height: LOW_STEP_TOP,
                },
                tint: [0.16, 0.38, 0.70],
            },
            Surface {
                label: "high step".to_string(),
                position: [0.0, 0.0, 0.5 * (LOW_STEP_FAR_Z + HIGH_STEP_FAR_Z)],
                shape: Shape::Platform {
                    width: 2.0 * LANE_HALF,
                    depth: LOW_STEP_FAR_Z - HIGH_STEP_FAR_Z,
                    height: HIGH_STEP_TOP,
                },
                tint: [0.72, 0.34, 0.10],
            },
            // A sphere is centred on its origin, so a mound whose summit is
            // `summit` above the ground has its centre a radius below that.
            Surface {
                label: "gentle mound".to_string(),
                position: [gentle_x, gentle_summit - gentle_radius, gentle_z],
                shape: Shape::Dome {
                    radius: gentle_radius,
                },
                tint: [0.18, 0.46, 0.20],
            },
            Surface {
                label: "steep mound".to_string(),
                position: [steep_x, steep_summit - steep_radius, steep_z],
                shape: Shape::Dome {
                    radius: steep_radius,
                },
                tint: [0.62, 0.14, 0.13],
            },
        ]
    }

    /// The environment the committed `env.ron` holds.
    ///
    /// The ambient is [`SUN_AMBIENT`], which [`Map::sun`] reads back out of here
    /// and hands the renderer — so this half of the file is load-bearing and a
    /// change to it changes the picture.
    ///
    /// The camera is not read by anything that runs: [`crate::camera`] rebuilds a
    /// follow camera every frame around wherever the character is. What it is
    /// written from is that camera's own standoff and focus height at the spawn,
    /// **with the pitch levelled** — [`crate::camera::START_PITCH`] would put a
    /// sine in a file that is compared byte for byte on three platforms, and
    /// `f32::sin` is the host's libm rather than something Rust pins. So the row
    /// is "behind the character at chest height", exactly, and the two constants
    /// it is made of cannot drift away from the camera that uses them.
    fn env() -> Env {
        let focus = Vec3::new(
            SPAWN.x as f32,
            SPAWN.y as f32 + crate::camera::FOCUS_HEIGHT,
            SPAWN.z as f32,
        );
        Env {
            camera: EnvCamera {
                position: (focus + Vec3::Z * crate::camera::DISTANCE).to_array(),
                look_at: focus.to_array(),
            },
            ambient: SUN_AMBIENT,
        }
    }

    /// The spawn the committed `sys/spawn.ron` holds: [`SPAWN`], facing the way
    /// `crate::game`'s stage starts the body turned.
    const fn spawn() -> Spawn {
        Spawn {
            position: [SPAWN.x, SPAWN.y, SPAWN.z],
            facing: 0.0,
        }
    }

    /// The sun the committed `sys/sun.ron` holds, from the constants it was
    /// written out of.
    const fn sun() -> Sun {
        Sun {
            elevation: SUN_ELEVATION,
            color: SUN_COLOR,
            intensity: SUN_INTENSITY,
            period: SUN_PERIOD,
        }
    }

    /// The whole blockout in a world of its own, ready to be written out.
    ///
    /// Ids are handed out in manifest order — the surfaces, then the spawn, then
    /// the sun — because that is the order [`Scene::load`] reads them back in,
    /// and a writer that numbered them any other way would produce a file that
    /// does not round-trip.
    fn generated() -> (Scene, IdMap, World) {
        let mut world = World::new();
        world.register_system(Box::new(System::<Surface>::new(SURFACES)));
        world.register_system(Box::new(System::<Spawn>::new(SPAWN_POINT)));
        world.register_system(Box::new(System::<Sun>::new(SUN)));
        let mut ids = IdMap::new();

        let surfaces: Vec<_> = the_blockout()
            .into_iter()
            .map(|surface| {
                let entity = world.spawn();
                ids.assign(entity);
                (entity, surface)
            })
            .collect();
        let spawn_entity = world.spawn();
        ids.assign(spawn_entity);
        let sun_entity = world.spawn();
        ids.assign(sun_entity);

        let system = world
            .system_mut::<System<Surface>>()
            .expect("just registered");
        for (entity, surface) in surfaces {
            system.attach(entity, surface);
        }
        world
            .system_mut::<System<Spawn>>()
            .expect("just registered")
            .attach(spawn_entity, spawn());
        world
            .system_mut::<System<Sun>>()
            .expect("just registered")
            .attach(sun_entity, sun());

        let scene = Scene::new(
            "blockout",
            vec![
                SURFACES.to_string(),
                SPAWN_POINT.to_string(),
                SUN.to_string(),
            ],
            env(),
        );
        (scene, ids, world)
    }

    /// The codecs a puppet map is read and written through — this sample's own
    /// registration rather than a second list, so the writer test is about the
    /// vocabulary the sample ships.
    fn codecs() -> Vec<Box<dyn SystemChunk>> {
        let mut registry = Registry::new();
        register_components(&mut registry);
        registry.codecs()
    }

    /// Asserts a committed file is what the writer wrote, and says *where* it
    /// stopped agreeing when it is not.
    ///
    /// Line by line rather than as two strings: a chunk file is several lines per
    /// row, and an `assert_eq!` over the pair prints both in full for one changed
    /// coordinate.
    fn assert_committed(key: &str, written: &str, committed: &str) {
        if written == committed {
            return;
        }
        match written
            .lines()
            .zip(committed.lines())
            .position(|(written, committed)| written != committed)
        {
            Some(line) => panic!(
                "{key} line {}: the writer writes `{}`, the file has `{}`",
                line + 1,
                written.lines().nth(line).expect("the line just compared"),
                committed.lines().nth(line).expect("the line just compared"),
            ),
            None => panic!(
                "{key}: the writer writes {} lines, the file has {}",
                written.lines().count(),
                committed.lines().count(),
            ),
        }
    }

    /// **The committed map is exactly what the writer writes** from the constants
    /// above, which is what lets the blockout live in a file without a second
    /// copy of it in code.
    ///
    /// Generated once by [`Scene::save`] and maintained that way: ron prints a
    /// float through Rust's shortest-round-trip `Display`, which round-trips a
    /// *parsed* value and not a typed one, so a hand-edited coordinate is a
    /// failure here rather than a map that quietly moved.
    ///
    /// Byte for byte, on every platform: `.gitattributes` pins `*.ron` to LF, so
    /// a Windows checkout hands `include_str!` the bytes the repository holds and
    /// nothing here has to fold a CRLF the transport might have added.
    #[test]
    fn the_committed_map_is_what_the_writer_writes() {
        let (scene, ids, mut world) = generated();
        let files = scene
            .save(&mut world, &ids, &codecs())
            .expect("a generated blockout is writable");

        assert_committed("scene.ron", &files["scene.ron"], BLOCKOUT_SCENE_RON);
        assert_committed("env.ron", &files["env.ron"], BLOCKOUT_ENV_RON);
        assert_committed(
            "sys/surfaces.ron",
            &files["sys/surfaces.ron"],
            BLOCKOUT_SURFACES_RON,
        );
        assert_committed("sys/spawn.ron", &files["sys/spawn.ron"], BLOCKOUT_SPAWN_RON);
        assert_committed("sys/sun.ron", &files["sys/sun.ron"], BLOCKOUT_SUN_RON);
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            [
                "env.ron",
                "scene.ron",
                "sys/spawn.ron",
                "sys/sun.ron",
                "sys/surfaces.ron",
            ],
            "the manifest's files and nothing else"
        );
        for (key, text) in &files {
            assert!(!text.contains('\r'), "the newline is pinned in {key}");
        }
    }

    /// The other half of the same claim: what the committed files *parse* to is
    /// the blockout this module used to build in code. A writer that agreed with
    /// itself while dropping a field would pass the byte comparison above.
    #[test]
    fn the_committed_map_parses_to_the_blockout_this_module_builds() {
        let map = Map::built_in();
        assert_eq!(map.surfaces(), the_blockout());
        assert_eq!(map.spawn(), SPAWN);
        assert_eq!(map.facing(), 0.0);
        assert_eq!(map.env(), &env());
        // The sun's row, through the light it actually produces: the ambient in
        // it is `env.ron`'s and the rest is `sys/sun.ron`'s, and this is the one
        // place both are read at once.
        let light = map.sun(0.0);
        assert_eq!(light.direction.y, SUN_ELEVATION);
        assert_eq!(light.color, Vec3::from_array(SUN_COLOR) * SUN_INTENSITY);
        assert_eq!(light.ambient, Vec3::from_array(SUN_AMBIENT));
    }

    /// **The colliders the file produces are the ones this module used to add.**
    ///
    /// The old `world()` body, kept verbatim below, is the expectation — so this
    /// is the check that the [`Shape`] rows were transcribed correctly, which no
    /// comparison between the file and [`the_blockout`] could make: both of those
    /// are the same transcription.
    ///
    /// Compared as AABBs read back out of each world, which is the only shape a
    /// [`PhysicsWorld`] hands back. A box and a sphere with the same bounds would
    /// pass this and fail `the_committed_map_parses_to_the_blockout_this_module_builds`,
    /// which pins the shape.
    #[test]
    fn the_colliders_are_the_ones_this_module_used_to_add() {
        /// `world()` as it stood before the map became a file.
        fn the_old_colliders() -> PhysicsWorld {
            let mut world = PhysicsWorld::new();
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, -0.5 * GROUND_THICKNESS, 0.0),
                DVec3::new(GROUND_HALF, 0.5 * GROUND_THICKNESS, GROUND_HALF),
            ));
            world.add_box(BoxCollider::new(
                DVec3::new(
                    0.0,
                    0.5 * LOW_STEP_TOP,
                    0.5 * (LOW_STEP_NEAR_Z + LOW_STEP_FAR_Z),
                ),
                DVec3::new(
                    LANE_HALF,
                    0.5 * LOW_STEP_TOP,
                    0.5 * (LOW_STEP_NEAR_Z - LOW_STEP_FAR_Z),
                ),
            ));
            world.add_box(BoxCollider::new(
                DVec3::new(
                    0.0,
                    0.5 * HIGH_STEP_TOP,
                    0.5 * (LOW_STEP_FAR_Z + HIGH_STEP_FAR_Z),
                ),
                DVec3::new(
                    LANE_HALF,
                    0.5 * HIGH_STEP_TOP,
                    0.5 * (LOW_STEP_FAR_Z - HIGH_STEP_FAR_Z),
                ),
            ));
            for (x, z, radius, summit) in [GENTLE_MOUND, STEEP_MOUND] {
                world.add_sphere(Sphere::new(DVec3::new(x, summit - radius, z), radius));
            }
            world
        }

        /// Every collider's bounds, in an order neither world chose.
        fn bounds(world: &mut PhysicsWorld) -> Vec<[f64; 6]> {
            let everything = Aabb::new(DVec3::splat(-1.0e9), DVec3::splat(1.0e9));
            let ids = world.overlap_aabb(&everything);
            let mut bounds: Vec<[f64; 6]> = ids
                .into_iter()
                .map(|id| world.aabb_of(id).expect("an id the world just returned"))
                .map(|aabb| {
                    [
                        aabb.min.x, aabb.min.y, aabb.min.z, aabb.max.x, aabb.max.y, aabb.max.z,
                    ]
                })
                .collect();
            bounds.sort_by(|a, b| a.partial_cmp(b).expect("no map coordinate is NaN"));
            bounds
        }

        let mut old = the_old_colliders();
        let mut new = Map::built_in().world();
        assert_eq!(old.len(), 5, "the blockout is five colliders");
        assert_eq!(bounds(&mut new), bounds(&mut old));
    }

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the chunk file spells them.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a colour nobody can see — both of which leave a
    /// perfectly plausible picture.
    #[test]
    fn the_surfaces_name_their_own_meshes() {
        let map = Map::built_in();
        let scene = map.scene();
        let labels: Vec<&str> = scene.meshes.iter().map(|m| m.label.as_ref()).collect();
        assert_eq!(
            labels[..map.character_mesh_base()],
            [
                "ground",
                "low step",
                "high step",
                "gentle mound",
                "steep mound",
            ],
        );
        // The character's own slots are `crate::rig`'s parts, in its order:
        // `Map::character_mesh_base` is a base rather than a slot per limb, so
        // this is what says the two lists have not drifted apart.
        assert_eq!(
            labels[map.character_mesh_base()..map.nose_mesh()],
            rig::parts().map(|part| part.label)[..],
        );
        assert_eq!(labels[map.nose_mesh()], "nose");
        assert_eq!(scene.meshes.len(), map.nose_mesh() + 1);
        assert_eq!(
            scene.materials.len(),
            map.body_material() + 1,
            "one painted row per surface, and the body",
        );
        for row in 0..scene.materials.len() {
            assert_eq!(
                scene.materials[row].tiling,
                GpuMaterial::TILING_PHYSICAL,
                "row {row} must measure its grid in metres, not in its own UV",
            );
        }
    }

    /// **The two mounds straddle the angle the controller walks up**, which is
    /// the whole reason there are two of them. The default config's cut is a
    /// cosine, so the comparison is made in the same terms rather than in
    /// degrees.
    #[test]
    fn one_mound_is_walkable_and_the_other_is_not() {
        let config = crcbl::phys::CharacterConfig::default();
        let (_, _, gentle_radius, gentle_summit) = GENTLE_MOUND;
        let (_, _, steep_radius, steep_summit) = STEEP_MOUND;
        let gentle = rim_angle(gentle_radius, gentle_summit);
        let steep = rim_angle(steep_radius, steep_summit);
        assert!(
            gentle.cos() > config.min_ground_normal_y,
            "the gentle mound's rim is {:.1}°, which the controller refuses",
            gentle.to_degrees(),
        );
        assert!(
            steep.cos() < config.min_ground_normal_y,
            "the steep mound's rim is {:.1}°, which the controller would walk up",
            steep.to_degrees(),
        );
        // And the documented angles are the ones the numbers come to, to the
        // tenth of a degree the constants are written at.
        assert!((gentle.to_degrees() - GENTLE_RIM_DEG).abs() < 0.05);
        assert!((steep.to_degrees() - STEEP_RIM_DEG).abs() < 0.05);
    }

    /// **The two steps straddle the offset the controller climbs**, on the same
    /// terms. The rise the controller judges is the one *above what the
    /// character is standing on*, which for the second step is
    /// [`HIGH_STEP_RISE`] and not its height above the ground.
    #[test]
    fn one_step_is_climbable_and_the_other_is_not() {
        let config = crcbl::phys::CharacterConfig::default();
        assert!(
            LOW_STEP_TOP < config.step_offset,
            "the low step is {LOW_STEP_TOP} m, which is not under the offset",
        );
        assert!(
            HIGH_STEP_RISE > config.step_offset + config.skin_width,
            "the high step rises {HIGH_STEP_RISE} m, which the offset's skin-width band reaches",
        );
    }

    /// **The character is spawned on flat ground with the lane ahead of it**,
    /// which is what the browser gate's held key depends on: it walks from here
    /// into the first step and then into the second, and a spawn on either mound
    /// or already on a step would take that script's meaning away.
    #[test]
    fn the_spawn_is_on_the_flat_with_the_lane_in_front_of_it() {
        // Both halves are constants, so the compiler is what checks the first:
        // a spawn past the lane's near edge fails to build rather than failing
        // to run.

        const { assert!(SPAWN.z > LOW_STEP_NEAR_Z) };
        for (x, z, radius, summit) in [GENTLE_MOUND, STEEP_MOUND] {
            let ground_radius = (radius * radius - (radius - summit).powi(2)).sqrt();
            let gap = ((SPAWN.x - x).powi(2) + (SPAWN.z - z).powi(2)).sqrt();
            assert!(
                gap > ground_radius + CHARACTER_RADIUS,
                "the spawn is inside the mound at ({x}, {z})",
            );
        }
    }

    /// **The drawn character is the size of the capsule that moves it.** The mesh
    /// is built from this module's constants and the controller from
    /// [`crcbl::phys::CharacterConfig`], so nothing but this holds the two
    /// together — and a mismatch is a picture that is wrong about where the
    /// character's feet are, which no assertion about the simulation can see.
    #[test]
    fn the_character_mesh_is_the_size_of_the_capsule_that_moves_it() {
        let config = crcbl::phys::CharacterConfig::default();
        assert_eq!(CHARACTER_RADIUS, config.radius);
        assert_eq!(CHARACTER_HALF_HEIGHT, config.half_height);
        // The rig is built to these, and `rig::the_rig_fits_the_capsule_that_
        // moves_it` is what holds its boxes inside them.
        assert_eq!(rig::HEIGHT, CHARACTER_HEIGHT as f32);
        assert_eq!(rig::REACH, CHARACTER_RADIUS as f32);
    }

    /// **The sun turns and does not rise**, which is what makes a shadow read as
    /// one without the light ever leaving the map in the dark.
    #[test]
    fn the_sun_swings_round_without_changing_height() {
        let map = Map::built_in();
        let start = map.sun(0.0);
        assert!((start.direction.length() - 1.0).abs() < 1e-6);
        let quarter = map.sun(SUN_PERIOD / 4.0);
        assert!(
            (quarter.direction.y - start.direction.y).abs() < 1e-6,
            "the sun rose from {} to {}",
            start.direction.y,
            quarter.direction.y,
        );
        assert!(
            (quarter.direction - start.direction).length() > 0.5,
            "a quarter turn moved the sun by {}",
            (quarter.direction - start.direction).length(),
        );
        let round = map.sun(SUN_PERIOD);
        assert!(
            (round.direction - start.direction).length() < 1e-5,
            "a whole period did not come back to where it started",
        );
    }

    /// **The colliders are the boxes the meshes draw.** Read back out of the
    /// world rather than restated: a step whose collider sat a decimetre from its
    /// mesh would look walkable and refuse, or refuse nothing and stop the
    /// character in mid air.
    #[test]
    fn every_lane_surface_has_its_own_collider_where_its_mesh_is() {
        let map = Map::built_in();
        let mut world = map.world();
        let config = crcbl::phys::CharacterConfig::default();
        let mut character = crcbl::phys::CharacterController::new(
            config,
            map.spawn() + DVec3::Y * (config.radius + config.half_height),
        );
        character.move_and_slide(&mut world, DVec3::ZERO);
        assert!(character.is_grounded(), "the spawn has no floor under it",);
        let feet = |c: &crcbl::phys::CharacterController| {
            c.position().y - (config.radius + config.half_height)
        };
        assert!(
            feet(&character).abs() < config.skin_width * 3.0,
            "the character settled at {} rather than on the ground",
            feet(&character),
        );
    }

    /// A directory with no `scene.ron` is refused by the key that is missing, and
    /// the key is spelled the way `--scene` points at it — the scene directory
    /// *is* the root, so the header is `scene.ron` and not
    /// `blockout.scn/scene.ron`. A loader looking under the built-in name would
    /// report a file the caller never named.
    #[test]
    fn a_directory_with_no_header_is_refused_by_the_missing_key() {
        let dir = std::env::temp_dir().join(format!("puppet-empty-{}.scn", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a directory with no scene.ron is not a map");
        assert!(message.contains("scene.ron"), "{message}");
        assert!(message.contains("path not found"), "{message}");
        assert!(
            !message.contains(BLOCKOUT),
            "the key must be the caller's, not the built-in map's: {message}"
        );
    }

    /// **A scene that is not a puppet map is refused by the chunk it is missing.**
    ///
    /// A manifest naming a system with no codec is the format's own refusal; a
    /// manifest naming *fewer* systems is not, and without this check it would
    /// load as a map with no ground, no spawn and no sun — a run that starts and
    /// draws an empty world.
    #[test]
    fn a_manifest_that_leaves_out_a_chunk_is_refused_by_its_name() {
        let dir = std::env::temp_dir().join(format!("puppet-thin-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"thin\", systems: [\"surfaces\", \"spawn\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(dir.join("env.ron"), BLOCKOUT_ENV_RON).expect("the temp dir is writable");
        std::fs::write(dir.join("sys").join("surfaces.ron"), BLOCKOUT_SURFACES_RON)
            .expect("the temp dir is writable");
        std::fs::write(dir.join("sys").join("spawn.ron"), BLOCKOUT_SPAWN_RON)
            .expect("the temp dir is writable");

        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a scene with no sun is not a puppet map");
        // The *manifest*, and not the chunk that came back empty: a loader that
        // read on and found no rows would refuse this directory too, with a
        // message about a file the manifest never named.
        assert!(message.contains("manifest"), "{message}");
        assert!(message.contains("sun"), "{message}");

        // And a chunk that holds the wrong number of rows is refused by its file
        // rather than absorbed: two spawns is not "the first one".
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"thin\", systems: [\"surfaces\", \"spawn\", \"sun\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(dir.join("sys").join("sun.ron"), BLOCKOUT_SUN_RON)
            .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("spawn.ron"),
            "Chunk(system: \"spawn\", entities: [])",
        )
        .expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a map with no spawn is not a puppet map");
        assert!(message.contains("sys/spawn.ron"), "{message}");
        assert!(message.contains('0'), "{message}");

        // And two spawns is not "the first one" either, which is the half a
        // chunk with no rows cannot show. Numbered past every id the chunks
        // beside it claim: a second claim on one of those is the format's own
        // `DuplicateId`, which would refuse this directory for another reason.
        std::fs::write(
            dir.join("sys").join("spawn.ron"),
            "Chunk(system: \"spawn\", entities: [\n    (7, (position: (0.0, 0.0, 0.0), \
             facing: 0.0)),\n    (8, (position: (1.0, 0.0, 0.0), facing: 0.0)),\n])",
        )
        .expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a map with two spawns is not a puppet map");
        assert!(message.contains("sys/spawn.ron"), "{message}");
        assert!(message.contains('2'), "{message}");
    }

    // -- reflection ----------------------------------------------------------

    #[test]
    fn a_surface_describes_its_own_rows_and_its_shapes_separately() {
        let surface = Surface {
            label: "ground".to_owned(),
            position: [0.0, 0.0, 0.0],
            shape: Shape::Platform {
                width: 2.0 * GROUND_HALF,
                depth: 2.0 * GROUND_HALF,
                height: GROUND_THICKNESS,
            },
            tint: [0.5, 0.5, 0.5],
        };

        assert_eq!(surface.kind(), Kind::Struct);
        assert_eq!(
            surface
                .fields()
                .iter()
                .map(|row| row.label)
                .collect::<Vec<_>>(),
            ["Label", "Position", "Shape", "Tint"]
        );

        // The nested enum describes the variant the surface is actually in.
        let shape = surface.field(2).expect("the shape row");
        assert_eq!(shape.kind(), Kind::Enum);
        assert_eq!(shape.variant(), Some("Platform"));
        assert_eq!(
            shape
                .fields()
                .iter()
                .map(|row| row.name)
                .collect::<Vec<_>>(),
            ["width", "depth", "height"]
        );

        let dome = Surface {
            shape: Shape::Dome { radius: 4.0 },
            ..surface
        };
        let shape = dome.field(2).expect("the shape row");
        assert_eq!(shape.variant(), Some("Dome"));
        assert_eq!(
            shape
                .fields()
                .iter()
                .map(|row| row.name)
                .collect::<Vec<_>>(),
            ["radius"],
            "the other variant's rows are not here"
        );
    }

    #[test]
    fn an_edit_into_a_surfaces_shape_reaches_the_variant_that_is_active() {
        let mut surface = Surface {
            label: "mound".to_owned(),
            position: [0.0, 0.0, 0.0],
            shape: Shape::Dome {
                radius: GENTLE_MOUND.2,
            },
            tint: [0.5, 0.5, 0.5],
        };

        let old = get_path(&surface, "shape.radius").expect("the row is an f64");
        assert_eq!(old, Value::Float(GENTLE_MOUND.2));
        assert!(
            get_path(&surface, "shape.width").is_err(),
            "`width` is the platform variant's"
        );

        set_path(&mut surface, "shape.radius", &Value::Float(9.5)).expect("an f64 takes a float");
        assert_eq!(surface.shape, Shape::Dome { radius: 9.5 });
        set_path(&mut surface, "shape.radius", &old).expect("the recorded value");
        assert_eq!(
            surface.shape,
            Shape::Dome {
                radius: GENTLE_MOUND.2
            }
        );
    }

    /// **The inspector widget over the real component**, which is the claim
    /// `crcbl-ui`'s own tests cannot make: that crate sits below `apps/` and
    /// its fixture is a copy of this type. Here the panel is built over a
    /// `Surface` itself — one row per field, a closed header building no body,
    /// the registered vector override drawing the position as three
    /// drag-values — and a dragged component is reported as a path and the
    /// value it replaced, which `crcbl::reflect::set_path` puts back.
    #[test]
    fn an_inspector_over_a_surface_draws_its_rows_and_reports_an_undoable_edit() {
        use crcbl::math::Vec2;
        use crcbl::ui::style::Declaration;
        use crcbl::ui::text::FontAtlas;
        use crcbl::ui::tree::{
            AvailableSpace, FlexDirection, Inspection, InspectorOptions, LengthAuto, NavInput,
            Overrides, Ui,
        };
        use crcbl::ui::widget::PointerInput;

        /// The page the panel is laid out in, in pixels.
        const PAGE: f32 = 400.0;

        fn inspect(
            ui: &mut Ui,
            pointer: PointerInput,
            surface: &mut Surface,
            overrides: &Overrides,
        ) -> Inspection {
            ui.begin_frame_with(pointer, NavInput::default());
            let size = [
                Declaration::FlexDirection(FlexDirection::Column),
                Declaration::Width(LengthAuto::Px(PAGE)),
                Declaration::Height(LengthAuto::Px(PAGE)),
            ];
            let mut built = None;
            ui.block("#page", &size, |ui| {
                let options = InspectorOptions {
                    overrides: Some(overrides),
                    ..InspectorOptions::default()
                };
                built = Some(ui.inspector_with("#props", surface, &options));
            });
            ui.layout(
                Vec2::ZERO,
                AvailableSpace::definite(Vec2::splat(PAGE)),
                &FontAtlas::built_in(),
            );
            built.expect("the page builds the inspector")
        }

        let mut surface = Surface {
            label: "mound".to_owned(),
            position: [1.0, 2.0, 3.0],
            shape: Shape::Dome {
                radius: GENTLE_MOUND.2,
            },
            tint: [0.5, 0.25, 0.125],
        };
        let overrides = Overrides::vectors();
        let mut ui = Ui::new();
        let away = PointerInput::hovering(Vec2::splat(-1.0));
        let first = inspect(&mut ui, away, &mut surface, &overrides);

        assert!(first.edits.is_empty(), "a still frame edited the surface");
        let rows = ui.child_keys(first.response.key);
        assert_eq!(
            rows.len(),
            surface.fields().len(),
            "the panel is not one row per field of the component"
        );
        // The shape is the third field and a `Kind::Enum`, so its row is a
        // collapsing header — closed, so the header alone is built.
        assert_eq!(
            ui.child_keys(rows[2]).len(),
            1,
            "the closed shape header built its body"
        );
        // The position is the second field and the override draws it as its
        // label and one block per axis.
        let axes = ui.child_keys(rows[1]);
        assert_eq!(axes.len(), 4, "the vector override did not draw three axes");

        // Drag the position's X past the drag threshold: one step a pixel.
        let drag = ui.child_keys(axes[1])[1];
        let (min, max) = ui.rect(drag).expect("the axis was laid out");
        let on = (min + max) * 0.5;
        let held = |pos: Vec2| PointerInput {
            pos,
            down: true,
            released: false,
        };
        inspect(&mut ui, held(on), &mut surface, &overrides);
        let dragged = inspect(
            &mut ui,
            held(on + Vec2::new(8.0, 0.0)),
            &mut surface,
            &overrides,
        );

        let [edit] = dragged.edits.as_slice() else {
            panic!("the drag reported {:?}", dragged.edits);
        };
        assert_eq!(edit.path, "position.0", "the edit names another field");
        assert_eq!(
            edit.before,
            Value::Float(1.0),
            "the edit does not carry what the field held"
        );
        assert_ne!(surface.position[0], 1.0, "the drag moved nothing");

        // Undoing it is the same call with the value it replaced.
        set_path(&mut surface, &edit.path, &edit.before).expect("an f64 takes a float");
        assert_eq!(surface.position[0], 1.0, "the undo did not restore it");
        assert_eq!(
            get_path(&surface, &edit.path),
            Ok(edit.before.clone()),
            "the reported path does not read back what it replaced"
        );
    }

    #[test]
    fn the_suns_rows_carry_the_ranges_its_own_units_have() {
        let sun = Sun {
            elevation: SUN_ELEVATION,
            color: [1.0, 0.95, 0.9],
            intensity: SUN_INTENSITY,
            period: SUN_PERIOD,
        };
        let by_name = |name: &str| {
            *sun.fields()
                .iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("no row called `{name}`"))
        };

        assert_eq!(by_name("elevation").label, "Elevation");
        let elevation = by_name("elevation").range.expect("a unit-vector component");
        assert_eq!((elevation.min, elevation.max), (-1.0, 1.0));
        let colour = by_name("color").range.expect("a linear channel");
        assert_eq!((colour.min, colour.max), (0.0, 1.0));
        assert_eq!(get_path(&sun, "period"), Ok(Value::Float(SUN_PERIOD)));
    }

    #[test]
    fn a_spawns_facing_is_a_step_with_no_range_because_a_yaw_is_periodic() {
        let spawn = Spawn {
            position: SPAWN.to_array(),
            facing: 0.0,
        };
        let facing = spawn.fields()[1];
        assert_eq!(facing.name, "facing");
        assert_eq!(facing.range, None);
        assert_eq!(facing.step, Some(0.01));
    }

    /// **The two step heights `web/tools/browser-e2e.mjs` walks against are
    /// this map's.**
    ///
    /// `walk.highStep` is the gate's *refusal* control: the highest the feet
    /// ever get must stay under it. Raised there and not here, the bound stops
    /// bounding, and "it gets onto the low step and no further" passes on a
    /// character that climbed both. `it_gets_onto_the_low_step_and_no_further`
    /// asserts the same pair against these constants and never reads the
    /// driver, so it pins nothing there.
    ///
    /// Compared as numbers rather than as spellings: [`HIGH_STEP_TOP`] is a sum,
    /// and its `f64` is not the decimal the driver writes.
    #[test]
    fn the_browser_gates_step_heights_are_this_maps() {
        /// Far under anything the gate's own walk tolerance could tell apart,
        /// and far over the rounding a sum of two decimals picks up.
        const MIRROR_TOLERANCE_M: f64 = 1e-9;

        for (field, metres) in [("lowStep", LOW_STEP_TOP), ("highStep", HIGH_STEP_TOP)] {
            let written =
                crcbl_sample_test::browser_gate_demo_expectation("puppet", &["walk", field]);
            let parsed: f64 = written
                .parse()
                .unwrap_or_else(|_| panic!("the gate's walk.{field} is `{written}`, not a number"));
            assert!(
                (parsed - metres).abs() < MIRROR_TOLERANCE_M,
                "the browser gate's puppet walk.{field} is {written} m and this map's is {metres} m"
            );
        }
    }
}
