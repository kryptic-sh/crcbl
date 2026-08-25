//! The stage: what the effects play in front of, and how their particles are
//! drawn.
//!
//! ```text
//!            +X
//!             │
//!    column   │   column
//!       ▌     │      ▌
//!             │
//!  ───────────┼───────────  ground
//!        ▄▄▄  │  ▄▄▄
//!       anvil │  vent      the spam pad is behind the camera's shoulder
//!            −X
//! ```
//!
//! # Greybox and nothing else
//!
//! Every surface here is a `crcbl::greybox` primitive, which is
//! `docs/plan/sample/10-sparks.md`'s scope read plainly: the subject is what is
//! in the air, and a modelled prop in front of it would be a second thing to
//! look at. There are no binary assets in this sample at all.
//!
//! # A particle is an instance, and that is the whole rendering story
//!
//! `docs/plan/20-particles.md`'s mesh particles "inject transforms into the
//! stage 3 instance path — rocks/debris ride the normal GPU-driven pipeline
//! (culling and all) for free", and this file is that sentence. Each effect
//! owns a block of instance handles sized to its pool share, reserved once;
//! every frame the live particles are written into the front of the block and
//! whatever the last frame drew beyond them is parked out of the frustum. There
//! is no shader here, no pass and no vertex buffer of this sample's — a
//! particle is a small cube or a small sphere, drawn by the same cull and draw
//! generation as the ground it lands on.

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, column, cube, grid_material, grid_page, platform, sphere};
use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError, Projection,
};
use crcbl::shaders::mesh::GpuMaterial;
use crcbl::vfx::Live;

use crate::effects::{self, PALETTE_STEPS, PUFF_SHARE, SPAM_SHARE, SPARK_SHARE};

// ---- the world ---------------------------------------------------------------

/// How wide the ground slab is, in metres.
const GROUND_M: f32 = 16.0;

/// How thick it is. Thick enough to read as a slab from a low camera rather
/// than as a plane that disappears edge-on.
const GROUND_THICKNESS_M: f32 = 0.4;

/// The anvil the sparks come off: its footprint and its height, in metres.
const ANVIL: (f32, f32, f32) = (1.2, 0.9, 0.7);

/// The vent the smoke rises out of, on the same terms.
const VENT: (f32, f32, f32) = (1.0, 1.0, 0.25);

/// A column's side and height, in metres.
const COLUMN: (f32, f32) = (0.45, 3.2);

/// Where the anvil stands, on the ground.
const ANVIL_AT: Vec3 = Vec3::new(-2.2, 0.0, 0.0);

/// Where the vent sits.
const VENT_AT: Vec3 = Vec3::new(2.2, 0.0, 0.0);

/// Where the hostile effect erupts from — off to one side, so a reader can
/// watch the two stock effects without it filling the frame.
const SPAM_AT: Vec3 = Vec3::new(0.0, 0.05, -4.4);

/// The two columns, for something for the light to fall across.
const COLUMNS: [Vec3; 2] = [Vec3::new(-4.6, 0.0, -3.0), Vec3::new(4.6, 0.0, -3.0)];

/// Where the impact sparks are struck: the top face of the anvil.
#[must_use]
pub fn spark_origin() -> Vec3 {
    ANVIL_AT + Vec3::new(0.0, ANVIL.2, 0.0)
}

/// Where the smoke leaves the vent.
#[must_use]
pub fn puff_origin() -> Vec3 {
    VENT_AT + Vec3::new(0.0, VENT.2, 0.0)
}

/// Where the hostile effect erupts.
#[must_use]
pub const fn spam_origin() -> Vec3 {
    SPAM_AT
}

// ---- the description ---------------------------------------------------------

/// The ground slab.
pub const GROUND_MESH: usize = 0;
/// The anvil.
pub const ANVIL_MESH: usize = 1;
/// The vent.
pub const VENT_MESH: usize = 2;
/// A column.
pub const COLUMN_MESH: usize = 3;
/// A one-metre cube, scaled per particle: the shard the sparks and the hostile
/// effect are drawn as.
pub const SHARD_MESH: usize = 4;
/// A one-metre sphere, scaled per particle: the puff's blob.
pub const BLOB_MESH: usize = 5;

/// The ground's material row.
const GROUND_MATERIAL: usize = 0;
/// The props' row.
const PROP_MATERIAL: usize = 1;
/// The first of the sparks' [`PALETTE_STEPS`] rows.
const SPARK_PALETTE: usize = 2;
/// The first of the puff's.
const PUFF_PALETTE: usize = SPARK_PALETTE + PALETTE_STEPS;
/// The first of the hostile effect's.
const SPAM_PALETTE: usize = PUFF_PALETTE + PALETTE_STEPS;

/// How many rows the description declares.
const MATERIALS: usize = SPAM_PALETTE + PALETTE_STEPS;

/// The props [`place`] makes resident: the ground, the anvil, the vent and the
/// columns.
///
/// Derived from [`COLUMNS`] rather than written out, so adding a column cannot
/// leave the reservation one short — which is exactly what it did, and what a
/// headless run reported as "the instance pool holds 708 instances and 708 of
/// them are in use".
const PROPS: u32 = 3 + COLUMNS.len() as u32;

/// How many instances the three particle blocks and the props come to.
const INSTANCES: u32 = SPARK_SHARE + PUFF_SHARE + SPAM_SHARE + PROPS;

/// How finely the puff's blob is tessellated.
///
/// Coarse: it is a few centimetres across on screen, there are hundreds of
/// them, and every ring costs a row of vertices in a pool this sample sizes by
/// hand.
const BLOB_RINGS: u32 = 8;
/// And how many segments around it, on the same terms.
const BLOB_SEGMENTS: u32 = 12;

/// What this stage reserves, sized against what it places rather than left at
/// [`Capacities::default`] — that default reserves sixteen thousand instances,
/// and the level-of-detail state behind that number is a word per instance per
/// draw generator.
const CAPACITIES: Capacities = Capacities {
    vertices: 8 * 1024,
    indices: 32 * 1024,
    meshes: 8,
    instances: INSTANCES,
    materials: MATERIALS as u32,
    lights: 8,
    probes: 0,
};

/// A painted greybox material: the metric grid of `grid_page`, tinted, and
/// tiled physically so one tile measures [`GREYBOX_TILE_M`] of surface however
/// large the face is.
fn painted(tint: [f32; 3]) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..grid_material()
    }
}

/// Everything this stage makes resident: six meshes, two painted rows and the
/// three baked palettes behind them.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    let mut materials = vec![painted([0.26, 0.27, 0.30]), painted([0.40, 0.41, 0.45])];
    materials.extend(effects::palette(&effects::impact_sparks().modifiers.color));
    materials.extend(effects::palette(&effects::smoke_puff().modifiers.color));
    materials.extend(effects::palette(&effects::spam().modifiers.color));

    SceneDesc {
        meshes: vec![
            mesh("ground", platform(GROUND_M, GROUND_M, GROUND_THICKNESS_M)),
            mesh("anvil", platform(ANVIL.0, ANVIL.1, ANVIL.2)),
            mesh("vent", platform(VENT.0, VENT.1, VENT.2)),
            mesh("column", column(COLUMN.0, COLUMN.1)),
            // Both particle meshes are **one metre across**, so an instance's
            // scale is the particle's size in metres with no factor in between.
            mesh("shard", cube(1.0)),
            mesh("blob", sphere(0.5, BLOB_RINGS, BLOB_SEGMENTS)),
        ],
        materials,
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

// ---- what is drawn -----------------------------------------------------------

/// Where a parked instance goes: far below the ground and outside anything the
/// camera can see.
///
/// A block's handles are reserved for its whole life, so a frame with fewer
/// live particles than the last has to put the difference somewhere. Parking
/// rather than removing and re-adding: an instance handle is a slot in a pool
/// whose dirty ranges coalesce, and churning the population every frame would
/// trade one contiguous upload for a scatter of them.
const PARK: Vec3 = Vec3::new(0.0, -1_000.0, 0.0);

/// The smallest scale a particle is drawn at.
///
/// Every effect's size curve reaches zero, and a zero scale is a singular
/// transform: the renderer builds a normal basis out of the instance matrix,
/// and a matrix with no volume has no basis to build. A particle this small is
/// well under a pixel at any camera distance this sample uses, so the clamp
/// costs nothing visible.
const MIN_SCALE_M: f32 = 1.0e-4;

/// One effect's block of instances: a run of handles, and how many of them the
/// last frame drew.
#[derive(Debug)]
pub struct Block {
    handles: Vec<InstanceHandle>,
    mesh: usize,
    palette: usize,
    drawn: usize,
}

impl Block {
    /// How many instances the block holds — the effect's pool share.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.handles.len()
    }

    /// How many of them the last [`Block::write`] drew.
    #[must_use]
    pub const fn drawn(&self) -> usize {
        self.drawn
    }
}

/// The stage as the renderer holds it: the three particle blocks.
///
/// The props are not here. They are placed once at the identity of their own
/// transforms and never touched again, so there is nothing to hold on to; a
/// handle kept for something that never moves is a handle nobody reads.
#[derive(Debug)]
pub struct Drawn {
    /// The impact sparks.
    pub sparks: Block,
    /// The smoke puff.
    pub puff: Block,
    /// The hostile effect.
    pub spam: Block,
}

/// Makes the props resident and reserves the three particle blocks.
///
/// # Errors
///
/// [`InstancePoolError`] if this file's own instance capacity does not cover what
/// this file places, which is this file's numbers being wrong.
pub fn place(renderer: &mut ForwardRenderer) -> Result<Drawn, InstancePoolError> {
    // A `platform` and a `column` are both built standing on their origin's
    // plane, so a prop's transform is just where its footprint goes. The
    // ground's top face is `y = 0`, so the slab is the one thing that hangs
    // below the plane everything else stands on.
    renderer.add_instance(&InstanceDesc {
        mesh: GROUND_MESH,
        material: GROUND_MATERIAL,
        transform: Mat4::from_translation(Vec3::new(0.0, -GROUND_THICKNESS_M, 0.0)),
    })?;
    renderer.add_instance(&InstanceDesc {
        mesh: ANVIL_MESH,
        material: PROP_MATERIAL,
        transform: Mat4::from_translation(ANVIL_AT),
    })?;
    renderer.add_instance(&InstanceDesc {
        mesh: VENT_MESH,
        material: PROP_MATERIAL,
        transform: Mat4::from_translation(VENT_AT),
    })?;
    for position in COLUMNS {
        renderer.add_instance(&InstanceDesc {
            mesh: COLUMN_MESH,
            material: PROP_MATERIAL,
            transform: Mat4::from_translation(position),
        })?;
    }

    Ok(Drawn {
        sparks: reserve(renderer, SPARK_SHARE, SHARD_MESH, SPARK_PALETTE)?,
        puff: reserve(renderer, PUFF_SHARE, BLOB_MESH, PUFF_PALETTE)?,
        spam: reserve(renderer, SPAM_SHARE, SHARD_MESH, SPAM_PALETTE)?,
    })
}

/// Reserves one block's handles, parked.
fn reserve(
    renderer: &mut ForwardRenderer,
    share: u32,
    mesh: usize,
    palette: usize,
) -> Result<Block, InstancePoolError> {
    let mut handles = Vec::with_capacity(share as usize);
    for _ in 0..share {
        handles.push(renderer.add_instance(&InstanceDesc {
            mesh,
            material: palette,
            transform: parked(),
        })?);
    }
    Ok(Block {
        handles,
        mesh,
        palette,
        drawn: 0,
    })
}

/// The transform a parked instance carries.
fn parked() -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::splat(MIN_SCALE_M), Quat::IDENTITY, PARK)
}

impl Block {
    /// Writes one effect's live particles into this block, and parks the rest.
    ///
    /// Returns how many were drawn, which is the live count clamped to the
    /// block — they are equal unless this file's share and the effect's have
    /// drifted apart, which `the_blocks_are_the_shares_they_draw` asserts they
    /// have not.
    ///
    /// `palette` is the effect's baked rows, in the order [`scene`] appended
    /// them, and is what turns the simulation's colour into a material index.
    pub fn write(
        &mut self,
        renderer: &mut ForwardRenderer,
        live: Live<'_>,
        palette: &[GpuMaterial],
    ) -> usize {
        let count = live.len().min(self.handles.len());
        for at in 0..count {
            let scale = live.size[at].max(MIN_SCALE_M);
            // A tumble rather than a spin about one axis: the shards are cubes,
            // and a cube turning about `Y` alone reads as a cube sliding.
            let turn = live.rotation[at];
            let rotation = Quat::from_rotation_y(turn) * Quat::from_rotation_x(turn * TUMBLE);
            renderer.set_instance(
                self.handles[at],
                &InstanceDesc {
                    mesh: self.mesh,
                    material: self.palette + effects::nearest_row(palette, live.color[at]),
                    transform: Mat4::from_scale_rotation_translation(
                        Vec3::splat(scale),
                        rotation,
                        live.position[at],
                    ),
                },
            );
        }
        for at in count..self.drawn {
            renderer.set_instance(
                self.handles[at],
                &InstanceDesc {
                    mesh: self.mesh,
                    material: self.palette,
                    transform: parked(),
                },
            );
        }
        self.drawn = count;
        count
    }

    /// Parks everything this block last drew, for an effect that is no longer
    /// there to ask.
    pub fn clear(&mut self, renderer: &mut ForwardRenderer) {
        for at in 0..self.drawn {
            renderer.set_instance(
                self.handles[at],
                &InstanceDesc {
                    mesh: self.mesh,
                    material: self.palette,
                    transform: parked(),
                },
            );
        }
        self.drawn = 0;
    }
}

/// How fast a shard turns about `X` relative to `Y`, so the two do not
/// synchronise into a wobble.
const TUMBLE: f32 = 0.73;

// ---- the frame's camera and light --------------------------------------------

/// How long the camera takes to go once round, in seconds.
const ORBIT_PERIOD_S: f64 = 42.0;

/// How far out it sits, and how high, in metres.
const ORBIT: (f32, f32) = (9.0, 3.6);

/// What it looks at.
const FOCUS: Vec3 = Vec3::new(0.0, 1.1, 0.0);

/// Where the frame is seen from at `seconds`.
///
/// A slow orbit, and the only thing on this stage that moves without an effect
/// running. It is what makes the props read as solid — a still camera over a
/// still stage is a picture, and a viewer cannot tell one from a frozen loop.
#[must_use]
pub fn camera(seconds: f64) -> Camera {
    let angle = std::f64::consts::TAU * (seconds / ORBIT_PERIOD_S);
    #[allow(clippy::cast_possible_truncation)]
    let (sin, cos) = (angle.sin() as f32, angle.cos() as f32);
    Camera {
        eye: Vec3::new(ORBIT.0 * sin, ORBIT.1, ORBIT.0 * cos),
        target: FOCUS,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// How high the sun sits, as the `Y` of a unit direction.
const SUN_ELEVATION: f32 = 0.72;

/// What lights the stage.
///
/// Fixed rather than turning: this sample's moving parts are the effects, and a
/// light that swung as well would make it harder to tell what changed.
#[must_use]
pub fn sun() -> DirectionalLight {
    let flat = (1.0 - SUN_ELEVATION * SUN_ELEVATION).sqrt();
    DirectionalLight {
        direction: Vec3::new(flat * 0.6, SUN_ELEVATION, flat * 0.8),
        color: Vec3::new(1.0, 0.98, 0.92) * 1.5,
        ambient: Vec3::new(0.12, 0.13, 0.17),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The capacities cover what this file places**, which is the one thing
    /// about a hand-sized description that a picture would not show: an
    /// instance the pool refused is an effect that silently draws nothing.
    #[test]
    fn the_description_reserves_what_this_file_places() {
        let scene = scene();
        assert_eq!(
            scene.materials.len(),
            MATERIALS,
            "the material rows and the palette offsets have drifted apart"
        );
        assert!(
            scene.meshes.len() <= scene.capacities.meshes as usize,
            "{} meshes against a table of {}",
            scene.meshes.len(),
            scene.capacities.meshes
        );
        assert_eq!(
            scene.capacities.materials as usize,
            scene.materials.len(),
            "the material table is not the size of the rows put in it"
        );
        assert_eq!(
            scene.capacities.instances, INSTANCES,
            "the instance capacity is not the blocks plus the props"
        );
    }

    /// The palettes are appended in the order the offsets name them, so a
    /// particle drawn through `SPARK_PALETTE + n` gets the sparks' colour and
    /// not the puff's.
    #[test]
    fn each_palette_starts_where_its_offset_says() {
        let scene = scene();
        for (offset, gradient) in [
            (SPARK_PALETTE, effects::impact_sparks().modifiers.color),
            (PUFF_PALETTE, effects::smoke_puff().modifiers.color),
            (SPAM_PALETTE, effects::spam().modifiers.color),
        ] {
            let baked = effects::palette(&gradient);
            for (step, row) in baked.iter().enumerate() {
                assert_eq!(
                    scene.materials[offset + step].base_color,
                    row.base_color,
                    "row {} is not step {step} of the palette that starts at {offset}",
                    offset + step
                );
            }
        }
    }

    /// Each block is exactly its effect's share, so a live particle never has
    /// to be dropped for want of an instance to draw it with.
    #[test]
    fn the_blocks_are_the_shares_they_draw() {
        assert_eq!(SPARK_SHARE, effects::impact_sparks().max_particles);
        assert_eq!(PUFF_SHARE, effects::smoke_puff().max_particles);
        assert_eq!(SPAM_SHARE, effects::spam().max_particles);
    }

    /// Every effect erupts from something a reader can see it coming off.
    #[test]
    fn every_emitter_sits_on_the_prop_it_belongs_to() {
        assert_eq!(spark_origin().y, ANVIL.2, "the sparks are not on the anvil");
        assert_eq!(puff_origin().y, VENT.2, "the smoke is not at the vent");
        assert!(
            spark_origin().distance(puff_origin()) > 2.0,
            "the two stock effects are on top of each other"
        );
        for origin in [spark_origin(), puff_origin(), spam_origin()] {
            assert!(
                origin.x.abs() < GROUND_M * 0.5 && origin.z.abs() < GROUND_M * 0.5,
                "{origin:?} is off the ground slab"
            );
        }
    }

    /// The camera goes round and comes back, so the orbit is an orbit rather
    /// than a drift that eventually leaves the stage behind.
    #[test]
    fn the_camera_orbits_the_stage() {
        let start = camera(0.0);
        let quarter = camera(ORBIT_PERIOD_S * 0.25);
        let round = camera(ORBIT_PERIOD_S);
        assert!(
            start.eye.distance(quarter.eye) > 1.0,
            "the camera did not move over a quarter of its period"
        );
        assert!(
            start.eye.distance(round.eye) < 1.0e-3,
            "the camera did not come back after a full period"
        );
        for seconds in [0.0, 7.0, 19.0, 33.0] {
            assert!(
                (camera(seconds).eye.distance(FOCUS) - camera(0.0).eye.distance(FOCUS)).abs()
                    < 1.0e-3,
                "the camera's distance from the stage changed at {seconds}s"
            );
        }
    }
}
