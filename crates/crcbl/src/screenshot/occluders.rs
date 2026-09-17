//! [`Scene::Occluders`](super::Scene::Occluders)' content:
//! `docs/plan/03-gpu-driven-rendering.md` §3.3's occlusion cull, with something
//! for it to cull.
//!
//! A module of its own on `still_pool`'s terms: the layout, the camera path and
//! the builder are here, and the parent names the variant and its build arm.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!                     back wall ████████████         z = BACK_WALL_Z
//!        pyramids ▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲ (behind it)
//!   crates ■■■■■■■■■■■■■■■■■■■■■■■■■■■■  (between the walls)
//!   ████████████████████ front wall                   z = 0
//!            ▲ walker (behind the front wall, walking out past its end)
//!                    camera path, in front of the front wall, looking −Z
//! ```
//!
//! * **Two walls a field apart.** The front wall hides the crate field from a
//!   low camera, and the back wall hides the pyramid rows behind it from almost
//!   anywhere the path goes — so the cull has a large hidden set on every frame
//!   and a changing one as the camera moves.
//! * **The front wall ends inside the path.** Strafing past its end disoccludes
//!   the field a column at a time, which is the frame the first phase — testing
//!   against the previous frame's depth — is most wrong about.
//! * **A walker** that starts behind the front wall and walks out past its end,
//!   which is a disocclusion of an *object*, with the camera holding still
//!   relative to it.
//!
//! [`occluders_camera`] is the path: a strafe, a cut, and a fast turn.

use glam::{Mat4, Vec3};

use crate::hal::{Device, Format, GeometryPath, QueueHandle};
use crate::render::scene::{DEMO_CUBE, DEMO_PYRAMID, DEMO_TINTED, DEMO_UNTINTED, demo};
use crate::render::{
    Camera, DirectionalLight, ForwardRenderer, InstanceDesc, InstanceHandle, OcclusionCulling,
    Projection,
};

use super::{ForwardScene, OffscreenError};

/// Frames along [`occluders_camera`]'s path.
pub const OCCLUDERS_PATH_FRAMES: usize = 32;

/// The frame of the path [`Scene::Occluders`](super::Scene::Occluders)' golden
/// is drawn from: past the front wall's end, so the frame shows both walls, the
/// field between them and the walker.
pub const OCCLUDERS_GOLDEN_FRAME: usize = 9;

/// Crates between the walls, in columns and rows.
pub const OCCLUDERS_CRATES: (usize, usize) = (24, 10);

/// Pyramids behind the back wall, in columns and rows.
pub const OCCLUDERS_PYRAMIDS: (usize, usize) = (24, 6);

/// Every instance the scene places: the floor, the two walls, the crates, the
/// pyramids and the walker.
pub const OCCLUDERS_INSTANCES: usize =
    3 + OCCLUDERS_CRATES.0 * OCCLUDERS_CRATES.1 + OCCLUDERS_PYRAMIDS.0 * OCCLUDERS_PYRAMIDS.1 + 1;

/// Half the front wall's width. Its right-hand end is inside the camera's
/// strafe, which is what disoccludes the crates.
pub const OCCLUDERS_FRONT_HALF_WIDTH: f32 = 9.0;

/// How tall both walls stand.
const WALL_HEIGHT: f32 = 3.0;

/// Where the back wall stands, along `z`.
const BACK_WALL_Z: f32 = -18.0;

/// How far apart the crates and the pyramids stand.
const SPACING: f32 = 1.4;

/// What the scene is built with: the occlusion cull on, small features kept.
///
/// **On, in the golden**, which is what puts the occlusion entry points, the
/// farthest pyramid and the late passes into every backend's frame — the golden
/// is where a pipeline that does not build on a backend is caught, whether or
/// not its first frame has a history to hide anything with.
pub const OCCLUDERS_CULLING: OcclusionCulling = OcclusionCulling {
    occlusion: true,
    small_feature_pixels: None,
};

/// The walker's model on frame `frame` of the path: behind the front wall's
/// middle at first, then out past its right-hand end by the last frame.
#[must_use]
pub fn occluders_walker(frame: usize) -> Mat4 {
    let along = frame.min(OCCLUDERS_PATH_FRAMES) as f32 / OCCLUDERS_PATH_FRAMES as f32;
    Mat4::from_translation(Vec3::new(
        along * (OCCLUDERS_FRONT_HALF_WIDTH + 4.0),
        0.6,
        -1.2,
    )) * Mat4::from_scale(Vec3::splat(1.2))
}

/// The camera on frame `frame` of the path.
///
/// * **Frames before the cut** strafe right at head height in front of the front
///   wall, from its middle past its end — the field disoccludes a column at a
///   time.
/// * **The cut**, halfway: high and to the left, looking down over the front
///   wall, a frame whose previous camera says nothing about this one.
/// * **After it**, a fast turn from beside the front wall's end — the view
///   swings across the field a large angle per frame.
#[must_use]
pub fn occluders_camera(frame: usize) -> Camera {
    let half = OCCLUDERS_PATH_FRAMES / 2;
    let (eye, target) = match frame {
        frame if frame < half => {
            let x = -3.0 + frame as f32 * 1.1;
            (Vec3::new(x, 1.6, 9.0), Vec3::new(x + 1.0, 1.0, -10.0))
        }
        frame if frame == half => (Vec3::new(-14.0, 9.0, 10.0), Vec3::new(0.0, 0.0, -9.0)),
        frame => {
            let turn = (frame - half) as f32 * 0.22 - 0.9;
            let eye = Vec3::new(OCCLUDERS_FRONT_HALF_WIDTH + 2.0, 1.8, 3.0);
            (eye, eye + Vec3::new(turn.sin(), -0.08, -turn.cos()))
        }
    };
    Camera {
        eye,
        target,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// The instance [`occluders_walker`] describes, for a caller moving it.
#[must_use]
pub fn occluders_walker_desc(frame: usize) -> InstanceDesc {
    InstanceDesc {
        mesh: DEMO_PYRAMID,
        material: DEMO_TINTED,
        transform: occluders_walker(frame),
    }
}

/// The scene built, with the walker's handle beside it.
#[allow(missing_debug_implementations)]
pub struct Occluders {
    /// The renderer, the golden frame's camera and the default sun.
    pub scene: ForwardScene,
    /// The walker, for a caller moving it along the path.
    pub walker: InstanceHandle,
}

/// Builds the scene on `path`, culling as `culling` asks.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer does not build on `path`.
///
/// # Panics
///
/// If the demo scene's instance capacity cannot hold
/// [`OCCLUDERS_INSTANCES`], which is a fixture that outgrew its scene.
pub fn occluders_forward_on_path(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    path: GeometryPath,
    culling: OcclusionCulling,
) -> Result<Occluders, OffscreenError> {
    let mut renderer = ForwardRenderer::with_scene_on_path(device, queue, format, &demo(), path)?;
    renderer.set_occlusion_culling(culling);
    let mut add = |mesh: usize, material: usize, transform: Mat4| {
        renderer
            .add_instance(&InstanceDesc {
                mesh,
                material,
                transform,
            })
            .expect("the demo scene holds thousands of instances")
    };
    // The floor, its top face at y = 0.
    add(
        DEMO_CUBE,
        DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -0.25, -12.0))
            * Mat4::from_scale(Vec3::new(80.0, 0.5, 80.0)),
    );
    // The front wall, across z = 0.
    add(
        DEMO_CUBE,
        DEMO_TINTED,
        Mat4::from_translation(Vec3::new(0.0, WALL_HEIGHT / 2.0, 0.0))
            * Mat4::from_scale(Vec3::new(
                2.0 * OCCLUDERS_FRONT_HALF_WIDTH,
                WALL_HEIGHT,
                0.5,
            )),
    );
    // The back wall, wide enough to cover the pyramids from every frame of the
    // path.
    add(
        DEMO_CUBE,
        DEMO_TINTED,
        Mat4::from_translation(Vec3::new(0.0, WALL_HEIGHT / 2.0, BACK_WALL_Z))
            * Mat4::from_scale(Vec3::new(48.0, WALL_HEIGHT, 0.5)),
    );
    let (columns, rows) = OCCLUDERS_CRATES;
    for row in 0..rows {
        for column in 0..columns {
            let x = (column as f32 - (columns as f32 - 1.0) / 2.0) * SPACING;
            let z = -2.5 - row as f32 * SPACING;
            add(
                DEMO_CUBE,
                DEMO_UNTINTED,
                Mat4::from_translation(Vec3::new(x, 0.4, z)) * Mat4::from_scale(Vec3::splat(0.8)),
            );
        }
    }
    let (columns, rows) = OCCLUDERS_PYRAMIDS;
    for row in 0..rows {
        for column in 0..columns {
            let x = (column as f32 - (columns as f32 - 1.0) / 2.0) * SPACING;
            let z = BACK_WALL_Z - 1.5 - row as f32 * SPACING;
            add(
                DEMO_PYRAMID,
                DEMO_TINTED,
                Mat4::from_translation(Vec3::new(x, 0.5, z)),
            );
        }
    }
    let walker = add(DEMO_PYRAMID, DEMO_TINTED, occluders_walker(0));
    renderer.set_instance(walker, &occluders_walker_desc(OCCLUDERS_GOLDEN_FRAME));
    Ok(Occluders {
        scene: ForwardScene {
            camera: occluders_camera(OCCLUDERS_GOLDEN_FRAME),
            sun: DirectionalLight::default(),
            renderer: Box::new(renderer),
        },
        walker,
    })
}
