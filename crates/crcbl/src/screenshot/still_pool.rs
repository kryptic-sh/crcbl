//! [`Scene::StillPool`](super::Scene::StillPool)'s content:
//! `docs/plan/55-water.md` rung 1's fixture.
//!
//! A module of its own rather than more of `screenshot.rs`, which is already
//! the largest file in this crate: everything the fixture needs — its layout,
//! its medium, its camera and the builder — is here, and the parent only names
//! the variant and its build arm.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!          far bank (sand, dry)                          z < FAR_EDGE
//!   ── shore step (sand, SHORE_FLOOR below the level) ── SHORE_START > z > FAR_EDGE
//!   deep basin (tile, DEEP_FLOOR) │ shallow basin (tile, SHALLOW_FLOOR)
//!                  post ▌         │                      NEAR_EDGE > z > SHORE_START
//!          near bank (sand, dry)                          z > NEAR_EDGE
//!                           camera, on x = 0, looking −Z
//! ```
//!
//! * **Deep and shallow side by side, mirrored about the camera's column.** A
//!   point at `+x` and its mirror at `−x` are seen at the same angle, so they
//!   carry the same Fresnel term, reflect the same sky and are lit by a sun with
//!   no `x` component alike. The depth of water under them is the only thing
//!   left that can separate them — which is what the deep-versus-shallow claim
//!   reads, on `Scene::Bloom`'s mirrored-control terms.
//! * **The shoreline is on the far side**, where the view is grazing and the
//!   surface reflects most. A shoreline seen head-on would reflect so little
//!   that the fade would have nothing to fade.
//! * **The post stands out of the deep basin.** The water just beyond it on
//!   screen bends toward the pixels the post covers, which is the case the
//!   refraction's in-front rejection exists for.
//! * **Two tones**: sand for everything at or above the shoreline and tile for
//!   the basin, so a band on dry sand and a band on sand under the shoreline
//!   compare one albedo.
//!
//! The surface of every floor and wall is [`plate_mesh`]'s one quad, scaled and
//! turned, so the only geometry is the one the shading is about.

use crate::hal::{Device, Format, GeometryPath, QueueHandle};
use crate::render::scene::SceneDesc;
use crate::render::{Camera, ForwardRenderer, Medium, Projection, WaterBody};

use super::{ForwardScene, OffscreenError, place, plate_mesh};

/// The height of the water's surface, in world units.
pub const STILL_POOL_LEVEL: f32 = 0.0;

/// How far `±x` the pool reaches — wider than the frame at every depth it
/// shows, so no side wall is ever in view.
pub const STILL_POOL_HALF_WIDTH: f32 = 12.0;

/// The pool's edge nearest the camera, along `z`.
pub const STILL_POOL_NEAR_EDGE: f32 = 0.0;

/// Where the basin gives way to the shore step, along `z`.
pub const STILL_POOL_SHORE_START: f32 = -5.5;

/// The pool's far edge, along `z`, where the shore step meets the dry far bank.
pub const STILL_POOL_FAR_EDGE: f32 = -8.0;

/// The deep basin's floor, under `x < 0`.
pub const STILL_POOL_DEEP_FLOOR: f32 = -2.0;

/// The shallow basin's floor, under `x > 0`.
pub const STILL_POOL_SHALLOW_FLOOR: f32 = -0.35;

/// The shore step's floor: a few centimetres under the surface, which is
/// thinner than `crcbl_shaders::water::SHORE_FADE` at every angle the frame
/// sees it from.
pub const STILL_POOL_SHORE_FLOOR: f32 = -0.04;

/// The post's left and right edges along `x`, the `z` it stands at, and its
/// top — out of the deep basin and above the surface.
pub const STILL_POOL_POST: [f32; 4] = [-3.2, -2.4, -1.5, 0.5];

/// What the pool is made of: clear water with the absorption tilted the way
/// water's is — red lost fastest, blue slowest — at a strength that makes a
/// two-metre basin read visibly deeper than a thirty-five-centimetre one, and
/// a scattering well under it, so the floor stays the subject.
pub const STILL_POOL_MEDIUM: Medium = Medium {
    absorption: [0.55, 0.22, 0.12],
    scattering: [0.02, 0.03, 0.04],
};

/// [`Scene::StillPool`]'s one body: the rectangle between the near and far
/// edges, across the whole width.
///
/// [`Scene::StillPool`]: super::Scene::StillPool
#[must_use]
pub fn still_pool_body() -> WaterBody {
    WaterBody {
        outline: vec![
            [-STILL_POOL_HALF_WIDTH, STILL_POOL_NEAR_EDGE],
            [STILL_POOL_HALF_WIDTH, STILL_POOL_NEAR_EDGE],
            [STILL_POOL_HALF_WIDTH, STILL_POOL_FAR_EDGE],
            [-STILL_POOL_HALF_WIDTH, STILL_POOL_FAR_EDGE],
        ],
        level: STILL_POOL_LEVEL,
        medium: STILL_POOL_MEDIUM,
    }
}

/// Where [`Scene::StillPool`] is seen from: on the pool's centre line, above
/// the near bank, looking down the pool.
///
/// Public so a test can project a world point onto the frame through the
/// matrices the renderer uses.
///
/// [`Scene::StillPool`]: super::Scene::StillPool
#[must_use]
pub fn still_pool_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 4.0, 3.0),
        target: glam::Vec3::new(0.0, -2.0, -5.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
        },
    }
}

/// The sun: from behind the camera and above, with **no `x` component**, so the
/// mirrored halves of the frame are lit alike.
#[must_use]
pub fn still_pool_sun() -> crate::render::DirectionalLight {
    crate::render::DirectionalLight {
        direction: glam::Vec3::new(0.0, 1.0, 0.8).normalize(),
        ..super::dimmed_sun(0.15, 1.0)
    }
}

/// The sky: a pale horizon over a blue zenith, which is what the far, grazing
/// half of the pool reflects.
#[must_use]
pub fn still_pool_sky() -> crate::render::Sky {
    crate::render::Sky {
        zenith: glam::Vec3::new(0.015, 0.03, 0.08),
        horizon: glam::Vec3::new(0.35, 0.33, 0.30),
        ground: glam::Vec3::new(0.015, 0.015, 0.015),
    }
}

/// The plate every floor, wall and the post are drawn with — the mesh past the
/// demo scene's four.
const PLATE_MESH: usize = 4;

/// The dry banks and the shore step: sand.
const SAND: usize = 3;

/// The basin's floor and walls: a pale tile.
const TILE: usize = 4;

/// The post: a saturated red nothing else in the frame is, so a smear of it
/// onto the water beside it is unmistakable.
const POST: usize = 5;

/// The demo scene with the plate and the three rows appended.
fn still_pool_scene() -> SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.meshes.push(plate_mesh("still pool plate", 1.0));
    debug_assert_eq!(scene.meshes.len() - 1, PLATE_MESH);
    for base_color in [
        [0.85, 0.62, 0.30, 1.0],
        [0.0, 0.62, 0.72, 1.0],
        [0.80, 0.08, 0.05, 1.0],
    ] {
        scene.materials.push(crate::shaders::mesh::GpuMaterial {
            base_color,
            ..crate::shaders::mesh::GpuMaterial::UNTINTED
        });
    }
    debug_assert_eq!(scene.materials.len() - 1, POST);
    scene
}

/// A floor: the plate stretched over `x0..x1` by `z0..z1` at height `y`.
fn floor(x: [f32; 2], z: [f32; 2], y: f32) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.5 * (x[0] + x[1]), y, 0.5 * (z[0] + z[1])))
        * glam::Mat4::from_scale(glam::Vec3::new(x[1] - x[0], 1.0, z[1] - z[0]))
}

/// A wall facing `+z`, toward the camera: the plate stretched over `x0..x1` by
/// `y0..y1` and stood up at `z`. A quarter turn about `x` takes the plate's `+y`
/// normal to `+z`.
fn wall(x: [f32; 2], y: [f32; 2], z: f32) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.5 * (x[0] + x[1]), 0.5 * (y[0] + y[1]), z))
        * glam::Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
        * glam::Mat4::from_scale(glam::Vec3::new(x[1] - x[0], 1.0, y[1] - y[0]))
}

/// Every placement, as a material row and a model matrix, in insertion order.
fn placements() -> Vec<(usize, glam::Mat4)> {
    const BANK: f32 = 60.0;
    let width = STILL_POOL_HALF_WIDTH;
    let [post_left, post_right, post_z, post_top] = STILL_POOL_POST;
    vec![
        // The dry banks, level with the surface and outside its outline.
        (
            SAND,
            floor([-BANK, BANK], [STILL_POOL_NEAR_EDGE, 8.0], STILL_POOL_LEVEL),
        ),
        (
            SAND,
            floor(
                [-BANK, BANK],
                [-BANK, STILL_POOL_FAR_EDGE],
                STILL_POOL_LEVEL,
            ),
        ),
        // The shore step and the lip that climbs from it to the far bank.
        (
            SAND,
            floor(
                [-width, width],
                [STILL_POOL_FAR_EDGE, STILL_POOL_SHORE_START],
                STILL_POOL_SHORE_FLOOR,
            ),
        ),
        (
            SAND,
            wall(
                [-width, width],
                [STILL_POOL_SHORE_FLOOR, STILL_POOL_LEVEL],
                STILL_POOL_FAR_EDGE,
            ),
        ),
        // The two basins, and the walls that climb from each to the shore step.
        (
            TILE,
            floor(
                [-width, 0.0],
                [STILL_POOL_SHORE_START, STILL_POOL_NEAR_EDGE],
                STILL_POOL_DEEP_FLOOR,
            ),
        ),
        (
            TILE,
            floor(
                [0.0, width],
                [STILL_POOL_SHORE_START, STILL_POOL_NEAR_EDGE],
                STILL_POOL_SHALLOW_FLOOR,
            ),
        ),
        (
            TILE,
            wall(
                [-width, 0.0],
                [STILL_POOL_DEEP_FLOOR, STILL_POOL_SHORE_FLOOR],
                STILL_POOL_SHORE_START,
            ),
        ),
        (
            TILE,
            wall(
                [0.0, width],
                [STILL_POOL_SHALLOW_FLOOR, STILL_POOL_SHORE_FLOOR],
                STILL_POOL_SHORE_START,
            ),
        ),
        // The post, standing out of the deep basin.
        (
            POST,
            wall(
                [post_left, post_right],
                [STILL_POOL_DEEP_FLOOR, post_top],
                post_z,
            ),
        ),
    ]
}

/// [`Scene::StillPool`] drawn with `bodies` in place of the fixture's own pool.
///
/// **Public so a test can draw the same frame with no water**, which is what the
/// off-switch claim compares against: the same renderer, the same content, and
/// an empty slice.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built, and
/// [`OffscreenError::Water`] if a body cannot be meshed.
///
/// [`Scene::StillPool`]: super::Scene::StillPool
pub fn still_pool_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    bodies: &[WaterBody],
) -> Result<ForwardScene, OffscreenError> {
    still_pool_forward_on_path(
        device,
        queue,
        format,
        bodies,
        device.preferred_geometry_path(),
    )
}

/// [`still_pool_forward`] on exactly the geometry tail `path`, which is how
/// [`Scene::StillPool`]'s build arm honours a requested path.
///
/// # Errors
///
/// [`still_pool_forward`]'s, and [`OffscreenError::Hal`] carrying
/// `HalError::UnsupportedFeatures` if the device lacks `path`.
///
/// [`Scene::StillPool`]: super::Scene::StillPool
pub(super) fn still_pool_forward_on_path(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    bodies: &[WaterBody],
    path: GeometryPath,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer =
        ForwardRenderer::with_scene_on_path(device, queue, format, &still_pool_scene(), path)?;
    renderer.set_sky(still_pool_sky());
    for (material, model) in placements() {
        place(&mut renderer, PLATE_MESH, material, model);
    }
    if let Err(error) = renderer.set_water(bodies) {
        renderer.destroy(device);
        return Err(OffscreenError::Water(error));
    }
    Ok(ForwardScene {
        camera: still_pool_camera(),
        sun: still_pool_sun(),
        renderer: Box::new(renderer),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The mirrored halves really are mirrored**: every placement on one side
    /// of `x = 0` but the post has its twin at `−x`, and the sun and the camera
    /// have no `x` component. The deep-versus-shallow claim reads two mirrored
    /// bands and rests on this.
    #[test]
    fn the_halves_differ_only_in_depth() {
        let camera = still_pool_camera();
        assert_eq!(camera.eye.x, 0.0);
        assert_eq!(camera.target.x, 0.0);
        assert_eq!(still_pool_sun().direction.x, 0.0);
        let [post_left, post_right, ..] = STILL_POOL_POST;
        assert!(post_right <= 0.0, "the post stands in the deep half");
        assert!(post_left > -STILL_POOL_HALF_WIDTH);
    }
}
