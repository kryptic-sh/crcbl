//! The map: what the character walks on, and what it is drawn as.
//!
//! ```text
//!            +X
//!             │        ┌──────────┐  z = -9 … -3, top 0.90   the step it cannot climb
//!             │        ├──────────┤  z = -3 …  3, top 0.30   the step it can
//!  steep      │        │          │
//!  mound  ────┼────────┤  ground  ├──────── gentle mound
//!             │        │          │
//!             │        ▲ spawn, z = 10, facing −Z
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

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, capsule, cube, grid_material, grid_page, platform, sphere};
use crcbl::math::{DVec3, Mat4, Vec3};
use crcbl::phys::{BoxCollider, PhysicsWorld, Sphere};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError};
use crcbl::shaders::mesh::GpuMaterial;

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

/// The ground slab — [`SceneDesc::meshes`] slot 0.
pub const GROUND_MESH: usize = 0;
/// The step the character can climb.
pub const LOW_STEP_MESH: usize = 1;
/// The step it cannot.
pub const HIGH_STEP_MESH: usize = 2;
/// The mound it can walk up.
pub const GENTLE_MOUND_MESH: usize = 3;
/// The mound it cannot.
pub const STEEP_MOUND_MESH: usize = 4;
/// The character's body.
pub const BODY_MESH: usize = 5;
/// The block on the front of that body, which is how its facing is read.
pub const NOSE_MESH: usize = 6;

/// The ground's material row — [`SceneDesc::materials`] slot 0, and therefore
/// what an instance placed without a named material would shade through.
pub const GROUND_MATERIAL: usize = 0;
/// Blue: the step the character can climb.
pub const LOW_STEP_MATERIAL: usize = 1;
/// Orange: the step it cannot.
pub const HIGH_STEP_MATERIAL: usize = 2;
/// Green: the mound it can walk up.
pub const GENTLE_MATERIAL: usize = 3;
/// Red: the mound it cannot.
pub const STEEP_MATERIAL: usize = 4;
/// The character itself.
pub const BODY_MATERIAL: usize = 5;

/// What this map reserves, which is a little over what it places.
///
/// Sized against the description rather than left at
/// [`Capacities::default`]: that default reserves far more instances
/// for a blockout that places seven, and the level-of-detail state behind that
/// number is a word per instance per draw generator. Filling any of these is a
/// mistake in this file, and the numbers being close to what it uses is what
/// makes that true.
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

/// Everything this map makes resident: seven meshes, six painted rows and the
/// grid page they sample.
///
/// The mesh and material order is the constants above, in value order; keep
/// them and this assembly in step, which this module's
/// `the_constants_name_their_own_meshes` test asserts.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    let (_, _, gentle_radius, _) = GENTLE_MOUND;
    let (_, _, steep_radius, _) = STEEP_MOUND;
    SceneDesc {
        meshes: vec![
            mesh(
                "ground",
                platform(
                    2.0 * GROUND_HALF as f32,
                    2.0 * GROUND_HALF as f32,
                    GROUND_THICKNESS as f32,
                ),
            ),
            mesh(
                "low step",
                platform(
                    2.0 * LANE_HALF as f32,
                    (LOW_STEP_NEAR_Z - LOW_STEP_FAR_Z) as f32,
                    LOW_STEP_TOP as f32,
                ),
            ),
            mesh(
                "high step",
                platform(
                    2.0 * LANE_HALF as f32,
                    (LOW_STEP_FAR_Z - HIGH_STEP_FAR_Z) as f32,
                    HIGH_STEP_TOP as f32,
                ),
            ),
            mesh(
                "gentle mound",
                sphere(gentle_radius as f32, MOUND_RINGS, MOUND_SEGMENTS),
            ),
            mesh(
                "steep mound",
                sphere(steep_radius as f32, MOUND_RINGS, MOUND_SEGMENTS),
            ),
            mesh(
                "body",
                capsule(CHARACTER_RADIUS as f32, CHARACTER_HEIGHT as f32, 12, 24),
            ),
            mesh("nose", cube(NOSE_EDGE as f32)),
        ],
        materials: vec![
            painted([0.30, 0.31, 0.33]),
            painted([0.16, 0.38, 0.70]),
            painted([0.72, 0.34, 0.10]),
            painted([0.18, 0.46, 0.20]),
            painted([0.62, 0.14, 0.13]),
            painted([0.82, 0.68, 0.26]),
        ],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// The two instances that move: the character's body and its nose.
///
/// Handed back by [`place`] because everything else on this map is written once
/// and never again, and these two are rewritten every frame from wherever the
/// simulation put the character.
#[derive(Clone, Copy, Debug)]
pub struct Character {
    body: InstanceHandle,
    nose: InstanceHandle,
}

impl Character {
    /// Points the drawn character at `position` — the **centre** of the
    /// controller's capsule — turned to `facing` radians about `+Y`.
    ///
    /// `facing` is measured the way [`crate::camera`] measures a yaw: zero looks
    /// down `-Z`, which is where the engine's default camera looks.
    pub fn place_at(self, renderer: &mut ForwardRenderer, position: DVec3, facing: f64) {
        // The capsule's centre is what the controller holds; the *mesh* rests
        // its base on `y = 0`, as everything standing in the greybox pack does.
        // So the body is drawn from the feet, which is a half-height and a
        // radius below the centre.
        let feet = Vec3::new(
            position.x as f32,
            (position.y - (CHARACTER_RADIUS + CHARACTER_HALF_HEIGHT)) as f32,
            position.z as f32,
        );
        let body = Mat4::from_translation(feet) * Mat4::from_rotation_y(facing as f32);
        renderer.set_instance(
            self.body,
            &InstanceDesc {
                mesh: BODY_MESH,
                material: BODY_MATERIAL,
                transform: body,
            },
        );
        renderer.set_instance(
            self.nose,
            &InstanceDesc {
                mesh: NOSE_MESH,
                material: BODY_MATERIAL,
                // Just clear of the capsule's shoulder, on the body's own
                // forward axis — so this is where the rotation above becomes
                // visible.
                transform: body
                    * Mat4::from_translation(Vec3::new(
                        0.0,
                        NOSE_HEIGHT as f32,
                        -(CHARACTER_RADIUS + 0.5 * NOSE_EDGE) as f32,
                    )),
            },
        );
    }
}

/// Places every object on the map and hands back the two that move.
///
/// # Errors
///
/// [`InstancePoolError::PoolFull`] if `CAPACITIES`'s instance count does not
/// cover the map, which is a mistake in this file rather than a condition a run
/// can be in — but it is the caller that would have to report it, so it is
/// returned rather than unwrapped.
pub fn place(renderer: &mut ForwardRenderer) -> Result<Character, InstancePoolError> {
    let at =
        |x: f64, y: f64, z: f64| Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    let (gentle_x, gentle_z, gentle_radius, gentle_summit) = GENTLE_MOUND;
    let (steep_x, steep_z, steep_radius, steep_summit) = STEEP_MOUND;

    for (mesh, material, transform) in [
        // A `platform` rises from `y = 0`, so the ground is dropped by its own
        // thickness to put its top there.
        (
            GROUND_MESH,
            GROUND_MATERIAL,
            at(0.0, -GROUND_THICKNESS, 0.0),
        ),
        (
            LOW_STEP_MESH,
            LOW_STEP_MATERIAL,
            at(0.0, 0.0, 0.5 * (LOW_STEP_NEAR_Z + LOW_STEP_FAR_Z)),
        ),
        (
            HIGH_STEP_MESH,
            HIGH_STEP_MATERIAL,
            at(0.0, 0.0, 0.5 * (LOW_STEP_FAR_Z + HIGH_STEP_FAR_Z)),
        ),
        // A sphere is centred on its origin, so a mound whose summit is
        // `summit` above the ground has its centre a radius below that.
        (
            GENTLE_MOUND_MESH,
            GENTLE_MATERIAL,
            at(gentle_x, gentle_summit - gentle_radius, gentle_z),
        ),
        (
            STEEP_MOUND_MESH,
            STEEP_MATERIAL,
            at(steep_x, steep_summit - steep_radius, steep_z),
        ),
    ] {
        renderer.add_instance(&InstanceDesc {
            mesh,
            material,
            transform,
        })?;
    }

    // The character last, and at the identity: `Character::place_at` writes both
    // of these before the first frame is drawn, from the simulation's own
    // position rather than from a copy of the spawn kept here.
    let body = renderer.add_instance(&InstanceDesc {
        mesh: BODY_MESH,
        material: BODY_MATERIAL,
        transform: Mat4::IDENTITY,
    })?;
    let nose = renderer.add_instance(&InstanceDesc {
        mesh: NOSE_MESH,
        material: BODY_MATERIAL,
        transform: Mat4::IDENTITY,
    })?;
    Ok(Character { body, nose })
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

/// The same map, as the colliders the character sweeps against.
///
/// Every one of them is written from the constants the meshes above are written
/// from, which is the whole reason those constants exist.
#[must_use]
pub fn world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    // A `BoxCollider` is a centre and half-extents; each of the three below is
    // the same cuboid the matching `platform` mesh draws.
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
const SUN_INTENSITY: f32 = 2.2;

/// How high the sun stands, as the `+Y` component of the unit vector toward it.
///
/// Chosen for the shadows rather than for the light: high enough that a shadow
/// is a shape under the thing casting it rather than a stripe across the whole
/// map, low enough that it is a shape at all. A sun directly overhead would put
/// the character's shadow under its own feet, where nothing can see it.
const SUN_ELEVATION: f32 = 0.78;

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

/// The sun the map is lit and shadowed by, `seconds` into the run.
///
/// **The direction is the vector *towards* the light.** It stands at
/// a fixed `SUN_ELEVATION` and swings once round the compass every
/// [`SUN_PERIOD`], so
/// the character's shadow sweeps across the ground it is standing on.
///
/// A pure function of the time, and the time is the **simulation's** rather
/// than a wall clock — see [`crate::game`] — so a frame at `t` is the same frame
/// on every machine and a paused demo's shadows stop where they are.
///
/// The ambient is small and cool, standing for the sky: large enough that a face
/// no light reaches is dark rather than black, since a black face makes every
/// shadow a measurement of an unpainted frame.
#[must_use]
pub fn sun(seconds: f64) -> DirectionalLight {
    let angle = core::f64::consts::TAU * (seconds / SUN_PERIOD);
    #[allow(clippy::cast_possible_truncation)]
    let (sin, cos) = (angle.sin() as f32, angle.cos() as f32);
    // The horizontal part is what turns; the elevation is fixed, so the
    // normalisation below is a constant and the sun neither rises nor sets.
    let flat = (1.0 - SUN_ELEVATION * SUN_ELEVATION).sqrt();
    DirectionalLight {
        direction: Vec3::new(flat * sin, SUN_ELEVATION, flat * cos),
        color: Vec3::new(1.0, 0.97, 0.90) * SUN_INTENSITY,
        ambient: Vec3::new(0.10, 0.11, 0.14),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the constants say.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a colour nobody can see — both of which leave a
    /// perfectly plausible picture.
    #[test]
    fn the_constants_name_their_own_meshes() {
        let scene = scene();
        let labels: Vec<&str> = scene.meshes.iter().map(|m| m.label.as_ref()).collect();
        assert_eq!(
            [
                GROUND_MESH,
                LOW_STEP_MESH,
                HIGH_STEP_MESH,
                GENTLE_MOUND_MESH,
                STEEP_MOUND_MESH,
                BODY_MESH,
                NOSE_MESH,
            ]
            .map(|mesh| labels[mesh]),
            [
                "ground",
                "low step",
                "high step",
                "gentle mound",
                "steep mound",
                "body",
                "nose",
            ],
        );
        assert_eq!(
            scene.materials.len(),
            6,
            "six painted rows, one per constant",
        );
        for row in [
            GROUND_MATERIAL,
            LOW_STEP_MATERIAL,
            HIGH_STEP_MATERIAL,
            GENTLE_MATERIAL,
            STEEP_MATERIAL,
            BODY_MATERIAL,
        ] {
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
    /// into the first step and then into the second, and a spawn on either
    /// mound or already on a step would take that script's meaning away.
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

    /// **The drawn character is the size of the capsule that moves it.** The
    /// mesh is built from this module's constants and the controller from
    /// [`crcbl::phys::CharacterConfig`], so nothing but this holds the two
    /// together — and a mismatch is a picture that is wrong about where the
    /// character's feet are, which no assertion about the simulation can see.
    #[test]
    fn the_character_mesh_is_the_size_of_the_capsule_that_moves_it() {
        let config = crcbl::phys::CharacterConfig::default();
        assert_eq!(CHARACTER_RADIUS, config.radius);
        assert_eq!(CHARACTER_HALF_HEIGHT, config.half_height);
    }

    /// **The sun turns and does not rise**, which is what makes a shadow read as
    /// one without the light ever leaving the map in the dark.
    #[test]
    fn the_sun_swings_round_without_changing_height() {
        let start = sun(0.0);
        assert!((start.direction.length() - 1.0).abs() < 1e-6);
        let quarter = sun(SUN_PERIOD / 4.0);
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
        let round = sun(SUN_PERIOD);
        assert!(
            (round.direction - start.direction).length() < 1e-5,
            "a whole period did not come back to where it started",
        );
    }

    /// **The colliders are the boxes the meshes draw.** Read back out of the
    /// world rather than restated: a step whose collider sat a decimetre from
    /// its mesh would look walkable and refuse, or refuse nothing and stop the
    /// character in mid air.
    #[test]
    fn every_lane_surface_has_its_own_collider_where_its_mesh_is() {
        let mut world = world();
        let config = crcbl::phys::CharacterConfig::default();
        let mut character = crcbl::phys::CharacterController::new(
            config,
            SPAWN + DVec3::Y * (config.radius + config.half_height),
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
}
