//! The aerial-perspective fixture: a floor to the horizon under an atmosphere
//! whose air is thick enough to see, looked at level.
//!
//! A module of its own rather than more of `screenshot.rs`, on
//! [`still_pool`](super::still_pool)'s terms: the fixture's layout, its
//! atmosphere, its camera and its builder are here, and a test reaches them
//! through the parent's re-exports.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!        dark half (x < 0)  │  light half (x > 0)        far edge, near the horizon
//!                           │
//!                           │
//!                camera, on x = 0, AERIAL_EYE_UP over the floor, looking −Z
//! ```
//!
//! * **The eye is low and level**, so the floor runs from a few units under
//!   the eye at the bottom of the frame to tens of kilometres of air at the
//!   rows just under the horizon — one frame holds a band the air barely
//!   touches and a band it covers.
//! * **A dark half and a light half, mirrored about the camera's column**, and
//!   a sun with no `x` component. A pixel at `+x` and its mirror at `−x` are
//!   the same distance along rays the same angle from the sun, so the air in
//!   front of them is the same `(in-scatter, transmittance)` — and the
//!   difference between them is `(light − dark) · T`, which is the one number
//!   that separates a transmittance from an in-scatter.
//! * **The atmosphere is a parameter, and so is its scale.** The control is the
//!   same atmosphere at zero kilometres per unit: the same sky, the same sun,
//!   the same L1 ambient on the floor, and no air in front of anything — so a
//!   band that differs between the two frames differs by the air alone.
//! * **Every effect refused**, on `atmosphere_forward`'s terms: shadows have
//!   nothing to fall on, and the occlusion pass, the reflection pair and the
//!   antialiasing resolve would each be a term the host has to model to
//!   predict a band. The fog effect is the one exception a caller may ask for,
//!   for the off-position arm.

use crate::hal::{Device, Format, QueueHandle};
use crate::render::scene::SceneDesc;
use crate::render::{
    Camera, EffectOverride, EffectRequest, Fog, ForwardRenderer, Projection, RenderEffects,
};

use super::{ForwardScene, OffscreenError, place, plate_mesh};

/// How many kilometres one world unit is in [`aerial_sky`]'s thick arm.
///
/// **Far from the metre, and that is the fixture.** At
/// [`crate::shaders::atmosphere::KM_PER_METRE`] a floor this frame can show has
/// an optical depth of air in front of it measured in thousandths, and what
/// that moves is a level here and there in the darkest pixels — nothing a
/// test could predict against. At this scale a hundred units of floor is tens
/// of kilometres, the rows under the horizon are behind enough air to move by
/// dozens of levels, and what a test reads is a prediction against the host's own
/// [`AerialView`](crate::shaders::atmosphere::AerialView) rather than a
/// rounding step. `crcbl_shaders::atmosphere`'s tests march at the same scale
/// and print how coarse its slices are there.
pub const AERIAL_KM_PER_UNIT: f32 = 0.2;

/// How far above the floor [`aerial_camera`]'s eye stands, in world units.
///
/// Low, because the rows just under a level horizon are `AERIAL_EYE_UP` over
/// the tangent of a fraction of a degree away — so the lower the eye, the more
/// rows of the frame are close enough to the horizon to be behind real air,
/// while the bottom rows stay a few units off.
pub const AERIAL_EYE_UP: f32 = 1.0;

/// The floor's extent along `x` either side of the camera's column and back
/// along `−z`, in world units.
///
/// **Far enough that the floor reaches the horizon's own row**, on
/// `ATMOSPHERE_MIRROR_FLOOR_SCALE`'s terms: at [`AERIAL_EYE_UP`] this far out is
/// under the first row below the horizon, so the frame is floor under sky
/// with no strip of the sky-view LUT's black lower hemisphere between them.
const AERIAL_FLOOR_REACH: f32 = 400.0;

/// How far behind the eye the floor starts, along `+z`, so the bottom row's
/// ray lands on it.
const AERIAL_FLOOR_BEHIND: f32 = 10.0;

/// The sun [`aerial_sky`] and [`aerial_sun`] share, towards the sun and not
/// normalised — `crcbl_render::Atmosphere` normalises on the way in.
///
/// **No `x` component**, which is the mirrored halves' whole argument, and
/// **behind the camera** at forty-five degrees, so the floor is lit face on
/// enough to be well inside the frame's range and the air the camera looks
/// through is lit from behind it — back-scatter, which keeps the in-scatter a
/// share of the floor's own radiance rather than the aureole's glare.
const AERIAL_SUN: [f32; 3] = [0.0, 1.0, 1.0];

/// The sun's colour on the floor, for [`aerial_sun`].
///
/// Well under one, so the light half is inside an eight-bit frame under the
/// clamp and the dark half is not black.
const AERIAL_LIGHT: f32 = 0.6;

/// The light half's albedo.
const AERIAL_LIGHT_ALBEDO: f32 = 0.8;

/// The dark half's albedo — dark enough that the air's in-scatter brightens it
/// by dozens of levels, and not black, so its transmittance is observable too.
const AERIAL_DARK_ALBEDO: f32 = 0.01;

/// [`aerial_scene`]'s plate mesh: the one past the demo scene's four.
const AERIAL_PLATE: usize = 4;

/// [`aerial_scene`]'s light material row: the one past the demo scene's three.
const AERIAL_LIGHT_MATERIAL: usize = 3;

/// [`aerial_scene`]'s dark material row.
const AERIAL_DARK_MATERIAL: usize = 4;

/// The atmosphere [`aerial_forward`] draws under, at `km_per_unit`.
///
/// **Public because the test predicts the frame from it**, on
/// `atmosphere_sky`'s terms: the claim is that the air in front of the floor
/// is the host's own `AerialView`, and a test that built its own atmosphere
/// would be comparing two guesses. [`AERIAL_KM_PER_UNIT`] is the thick arm and
/// zero is the control.
#[must_use]
pub fn aerial_sky(km_per_unit: f32) -> crate::render::Atmosphere {
    crate::render::Atmosphere {
        sun_direction: glam::Vec3::from_array(AERIAL_SUN),
        sun_illuminance: glam::Vec3::ONE,
        altitude_km: 0.0,
        km_per_unit,
    }
}

/// The camera [`aerial_forward`] is drawn with: level, [`AERIAL_EYE_UP`] over
/// the floor on the `x = 0` column, looking along `−Z`.
///
/// Level, so the horizon is the frame's middle row and two pixels at mirrored
/// columns of one row look along mirrored rays. Public so the test can
/// unproject a pixel into the same world ray the composite does.
#[must_use]
pub fn aerial_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, AERIAL_EYE_UP, 0.0),
        target: glam::Vec3::new(0.0, AERIAL_EYE_UP, -1.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
        },
    }
}

/// The light [`aerial_forward`] runs under: `AERIAL_SUN`'s direction, at
/// `AERIAL_LIGHT`, and no flat ambient — the atmosphere's L1 rows are the
/// ambient, and they are the same in both arms.
#[must_use]
pub fn aerial_sun() -> crate::render::DirectionalLight {
    crate::render::DirectionalLight {
        direction: glam::Vec3::from_array(AERIAL_SUN).normalize(),
        color: glam::Vec3::splat(AERIAL_LIGHT),
        ambient: glam::Vec3::ZERO,
    }
}

/// The medium [`aerial_forward`]'s off-position arm draws its fog effect
/// through: thin, uniform and unlit by the sun, so the column is a small,
/// smooth term over the whole floor and the comparison that arm makes is
/// about the composite's switches rather than about the fog.
#[must_use]
pub fn aerial_fog() -> Fog {
    Fog {
        density: 0.02,
        falloff: 0.0,
        reference_height: 0.0,
        color: glam::Vec3::new(0.2, 0.22, 0.25),
        ..Fog::NONE
    }
}

/// The demo scene with a one-unit plate and the two halves' material rows
/// appended, both rough and non-metallic — Lambertian enough that a floor
/// pixel is its albedo's share of the light and not a gleam.
fn aerial_scene() -> SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.meshes.push(plate_mesh("aerial floor", 1.0));
    debug_assert_eq!(scene.meshes.len() - 1, AERIAL_PLATE);
    for albedo in [AERIAL_LIGHT_ALBEDO, AERIAL_DARK_ALBEDO] {
        scene.materials.push(crate::shaders::mesh::GpuMaterial {
            base_color: [albedo, albedo, albedo, 1.0],
            metallic: 0.0,
            roughness: 1.0,
            ..crate::shaders::mesh::GpuMaterial::UNTINTED
        });
    }
    debug_assert_eq!(scene.materials.len() - 1, AERIAL_DARK_MATERIAL);
    scene
}

/// A plate over `x` in `[x0, x1]` and `z` in `[z0, z1]`, level at `y = 0`.
fn floor(x: [f32; 2], z: [f32; 2]) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(
        0.5 * (x[0] + x[1]),
        0.0,
        0.5 * (z[0] + z[1]),
    )) * glam::Mat4::from_scale(glam::Vec3::new(x[1] - x[0], 1.0, z[1] - z[0]))
}

/// The fixture: the two halves under `atmosphere` — or none — with `fog`'s
/// effect and medium where a caller asks for it.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn aerial_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    atmosphere: Option<crate::render::Atmosphere>,
    fog: Option<Fog>,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &aerial_scene())?;
    renderer.set_effect_request(EffectRequest {
        camera: RenderEffects::empty(),
        programmatic: EffectOverride::none()
            .force(RenderEffects::VOLUMETRIC_FOG, Some(fog.is_some())),
        ..EffectRequest::default()
    });
    if let Some(fog) = fog {
        renderer.set_fog(fog);
    }
    renderer.set_atmosphere(atmosphere);
    let reach = [-AERIAL_FLOOR_REACH, AERIAL_FLOOR_BEHIND];
    place(
        &mut renderer,
        AERIAL_PLATE,
        AERIAL_DARK_MATERIAL,
        floor([-AERIAL_FLOOR_REACH, 0.0], reach),
    );
    place(
        &mut renderer,
        AERIAL_PLATE,
        AERIAL_LIGHT_MATERIAL,
        floor([0.0, AERIAL_FLOOR_REACH], reach),
    );
    Ok(ForwardScene {
        camera: aerial_camera(),
        sun: aerial_sun(),
        renderer: Box::new(renderer),
    })
}
