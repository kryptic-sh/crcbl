//! [`Scene::Meadow`](super::Scene::Meadow)'s content: `docs/plan/57-grass.md`
//! rung G1's fixture, and the first milestone of
//! `docs/plan/sample/22-meadow.md`.
//!
//! A module of its own rather than more of `screenshot.rs`, which is already the
//! largest file in this crate — `still_pool`'s arrangement, down to the parent
//! naming only the variant and its build arm.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!            far, uphill                                   z = -HALF
//!   ┌── tile 0 ────────────┬─│─┬──────────── tile 1 ──┐
//!   │  dense, row 0, windy │ p │  dense, row 0, calm  │   … and a patch of
//!   ├── tile 2 ────────────┤ a ├──────────── tile 3 ──┤     row 1 in tile 1
//!   │ sparse, row 0, windy │ t │ sparse, row 0, calm  │
//!   └──────────────────────┴─h─┴──────────────────────┘   z = +HALF
//!                   camera, on x = 0, looking −Z
//! ```
//!
//! Four quadrants that differ in **one thing each**, so every claim the rung is
//! graded on is a comparison of two bands that are alike in everything else:
//!
//! * **Dense against sparse, along `z`.** The far half's cover texels are at
//!   [`MEADOW_DENSE`] and the near half's at [`MEADOW_SPARSE`]; the blade row,
//!   the ground and the wind are the same on both. "A denser tile draws more
//!   instances" is tile 0's instance count against tile 2's.
//! * **Windy against calm, along `x`.** The intensity layer is full on `x < 0`
//!   and **exactly zero** on `x > 0`. "Calm means still" is every blade on the
//!   calm side carrying a lean of exactly zero.
//! * **The path down the middle** is bare ground, and it is not decoration: it
//!   is [`MEADOW_PATH_HALF_WIDTH`] wide, which is wider than the intensity
//!   layer's one-texel blend, so **no blade stands anywhere the wind is
//!   partial**. Every blade is fully windy or exactly calm, which is what makes
//!   the calm claim an equality rather than a threshold. It is also the frame's
//!   bare-ground band, which the grass is compared against.
//! * **A patch of the second blade row** in tile 1, so the cover map's green
//!   channel is doing something and a frame that ignored it would be a different
//!   picture.
//!
//! # The ground is a plane, and the plate under it is the same plane
//!
//! `docs/plan/57-grass.md`'s decision 2: "a field takes them from a heightfield
//! the caller supplies until one exists". This one is
//! `y = MEADOW_SLOPE · (x, z)` — a hillside that rises toward the far edge and
//! leans a little across it.
//!
//! **A plane rather than a hill, because one quad can be a plane exactly.** The
//! ground has to be drawn as well as sampled, and `super::plate_mesh` is one
//! flat quad; a curved hillside would need a grid mesh of its own, and the blades
//! would float above whatever that mesh got wrong. The plane is still a real
//! test of the placement: it is neither level nor axis-aligned, so a generation
//! pass that answered `+Y` for every ground normal, or that lost the
//! heightfield's origin, draws a visibly different frame.
//! `docs/plan/sample/22-meadow.md`'s hillside is the curved one.

use crcbl_render::grass::{BladeType, CoverMap, GrassField, Heightfield, WindLayer, WindLayers};
use crcbl_wind::{Beaufort, DirectionLayer, IntensityLayer, LayerGrid, Weather, WindField};

use crate::hal::{Device, Format, GeometryPath, QueueHandle};
use crate::render::scene::SceneDesc;
use crate::render::{Camera, ForwardRenderer, Projection};

use super::{ForwardScene, OffscreenError, place, plate_mesh};

/// Tiles along each axis. Four, which is the smallest number that gives a
/// dense/sparse pair and a windy/calm pair at once.
pub const MEADOW_TILES: [u32; 2] = [2, 2];

/// One tile's side, in metres.
///
/// **This is the density knob, and it is the only one.** A tile is
/// `crcbl_render::grass::CELLS_PER_TILE` cells on a side whatever its size, so
/// eight metres over sixty-four cells is a cell every twelve and a half
/// centimetres — sixty-four roots a square metre at full cover, which is what
/// closes a mat of forty-centimetre clumps whose own coverage is a seventh of
/// the card. A caller wanting a thinner field uses larger tiles; nothing about
/// the engine changes.
pub const MEADOW_TILE_SIZE: f32 = 8.0;

/// Half the field's width, in metres: the field runs `-HALF..HALF` on both axes.
pub const MEADOW_HALF: f32 = MEADOW_TILE_SIZE * MEADOW_TILES[0] as f32 * 0.5;

/// How far from the camera a blade is still generated, in metres.
///
/// Far past the field's own far corner, so **nothing is distance-culled** and
/// the instance counts the claims read are the placement's alone.
/// `the_reach_covers_the_whole_field` is where that is held.
pub const MEADOW_REACH: f32 = 100.0;

/// The cover density of the far half.
pub const MEADOW_DENSE: u8 = 255;

/// The cover density of the near half.
///
/// A little under a third, so the count claim compares two numbers that are
/// plainly different rather than two that a hash's variance could swap.
pub const MEADOW_SPARSE: u8 = 48;

/// Half the bare path's width, in metres.
///
/// **Wider than the intensity layer's blend**, which is what makes every blade
/// in this field either fully windy or exactly calm: the layer's texels are
/// [`MEADOW_INTENSITY_METRES_PER_TEXEL`] apart and a bilinear tap reaches one
/// texel either side, so a sample at `|x| >= 0.5` reads texels that are all on
/// one side of the boundary. `the_path_is_wider_than_the_winds_blend` is where
/// the two numbers are held together.
pub const MEADOW_PATH_HALF_WIDTH: f32 = 1.0;

/// The plane the ground is: `y = SLOPE.x · x + SLOPE.y · z`.
///
/// Rising toward `-Z`, which is away from the camera, and leaning a little
/// across the frame so the ground normal is on no axis.
pub const MEADOW_SLOPE: [f32; 2] = [0.06, -0.11];

/// Metres of world per texel of the ground field.
pub const MEADOW_GROUND_METRES_PER_TEXEL: f32 = 0.5;

/// Metres of world per texel of the cover map.
pub const MEADOW_COVER_METRES_PER_TEXEL: f32 = 0.5;

/// Metres of world per texel of the wind's intensity layer.
pub const MEADOW_INTENSITY_METRES_PER_TEXEL: f64 = 1.0;

/// Texels along each axis of the wind's intensity layer.
///
/// **Thirty-two at a metre is a thirty-two-metre period over a sixteen-metre
/// field**, and both halves of that matter. The array's seam — where its last
/// texel meets its first under `repeat` addressing — is the boundary the windy
/// half ends at, and it lands on `x = 0`; the *other* seam, a whole period away
/// at `x = ±16`, then falls outside the field entirely. A period equal to the
/// field's width would put that second seam on the field's own edge, and the
/// blades there would read calm on the windy side. See [`meadow_wind_layers`].
pub const MEADOW_INTENSITY_TEXELS: u32 = 32;

/// Texels along each axis of the wind's direction layer.
pub const MEADOW_DIRECTION_TEXELS: u32 = 8;

/// Metres of world per texel of the direction layer.
pub const MEADOW_DIRECTION_METRES_PER_TEXEL: f64 = 2.0;

/// The weather over the field: a strong wind blowing towards `+X`.
///
/// `crcbl_wind::Beaufort::Strong` is nine metres a second, which is well past
/// `crcbl_shaders::grass::BEND_HALF_SPEED` — so a windy blade is bent most of
/// the way to the cap and the lean is a thing a picture shows rather than a
/// thing only a readback can find.
pub const MEADOW_WEATHER: Beaufort = Beaufort::Strong;

/// The ground the field stands on, as a heightfield: the plane
/// [`MEADOW_SLOPE`] describes, sampled onto texels.
#[must_use]
pub fn meadow_ground() -> Heightfield {
    let metres = MEADOW_GROUND_METRES_PER_TEXEL;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the field is eight metres over half-metre texels, which is seventeen"
    )]
    let side = (2.0 * MEADOW_HALF / metres) as u32 + 1;
    let mut heights = Vec::with_capacity((side * side) as usize);
    for row in 0..side {
        for column in 0..side {
            let x = -MEADOW_HALF + column as f32 * metres;
            let z = -MEADOW_HALF + row as f32 * metres;
            heights.push(meadow_height(x, z));
        }
    }
    Heightfield {
        texels: [side, side],
        metres_per_texel: metres,
        origin: [-MEADOW_HALF, -MEADOW_HALF],
        heights,
    }
}

/// The ground's height at a world position, which is the plane the plate under
/// the field is rotated onto.
#[must_use]
pub fn meadow_height(x: f32, z: f32) -> f32 {
    MEADOW_SLOPE[0] * x + MEADOW_SLOPE[1] * z
}

/// The ground's unit normal, which every blade in the field shades by.
#[must_use]
pub fn meadow_ground_normal() -> glam::Vec3 {
    glam::Vec3::new(-MEADOW_SLOPE[0], 1.0, -MEADOW_SLOPE[1]).normalize()
}

/// The density and blade-row map: the layout in this module's header.
#[must_use]
pub fn meadow_cover() -> CoverMap {
    let metres = MEADOW_COVER_METRES_PER_TEXEL;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "eight metres over half-metre texels is sixteen"
    )]
    let side = (2.0 * MEADOW_HALF / metres) as u32;
    let mut cover = Vec::with_capacity((side * side) as usize);
    for row in 0..side {
        for column in 0..side {
            // Texel centres, which is what `CoverMap::under` reads a position
            // onto — see this module's `origin` below.
            let x = -MEADOW_HALF + (column as f32 + 0.5) * metres;
            let z = -MEADOW_HALF + (row as f32 + 0.5) * metres;
            cover.push(meadow_cover_texel(x, z));
        }
    }
    CoverMap {
        texels: [side, side],
        metres_per_texel: metres,
        // Texel `(0, 0)` stands half a texel inside the field's corner.
        origin: [-MEADOW_HALF + 0.5 * metres, -MEADOW_HALF + 0.5 * metres],
        cover,
    }
}

/// The patch of the second blade row, as `x` then `z` bounds in metres.
///
/// In tile 1 — far and calm — so it sits in neither of the two comparisons the
/// claims make.
pub const MEADOW_SECOND_ROW: [[f32; 2]; 2] = [[2.0, 7.0], [-7.0, -3.0]];

/// What the cover map holds at a world position: `[density, blade row]`.
#[must_use]
pub fn meadow_cover_texel(x: f32, z: f32) -> [u8; 2] {
    if x.abs() < MEADOW_PATH_HALF_WIDTH {
        return [0, 0];
    }
    let [across, along] = MEADOW_SECOND_ROW;
    let row = u8::from((across[0]..across[1]).contains(&x) && (along[0]..along[1]).contains(&z));
    // **Dense away from the camera and sparse near it**, which is a choice the
    // golden forced: an isolated card has a whole silhouette against the bare
    // earth, and a silhouette is where two rasterisers' last bits move a pixel
    // between grass and ground — the largest step in the frame. The far half is
    // where a pixel covers most ground, so that is the half that has to be a
    // closed mat; the near half is sparse and each of its cards is large enough
    // that its edge is a small share of it.
    let density = if z < 0.0 { MEADOW_DENSE } else { MEADOW_SPARSE };
    [density, row]
}

/// The two blade rows: tall green grass, and a shorter yellower one.
#[must_use]
pub fn meadow_blades() -> Vec<BladeType> {
    vec![
        BladeType {
            root_color: [0.190, 0.300, 0.090],
            tip_color: [0.310, 0.490, 0.130],
            height: 0.55,
            // **A card is a tuft, not a blade** — `crcbl_render::grass::card`
            // draws four strands across it — so its width is a clump's rather
            // than a stalk's. At this field's sixty-four roots a square metre,
            // eighteen centimetres of card is what closes the mat.
            half_width: 0.20,
            height_spread: 0.25,
            width_spread: 0.35,
        },
        BladeType {
            root_color: [0.330, 0.285, 0.080],
            tip_color: [0.500, 0.425, 0.115],
            height: 0.36,
            half_width: 0.15,
            height_spread: 0.40,
            width_spread: 0.30,
        },
    ]
}

/// The field this fixture draws.
///
/// # Panics
///
/// Never for the numbers above; the constructor's refusals are all things this
/// module fixes, and the message names the one that moved if a constant here is
/// edited into something a shader could not survive.
#[must_use]
pub fn meadow_field() -> GrassField {
    GrassField::new(
        MEADOW_TILES,
        MEADOW_TILE_SIZE,
        [-MEADOW_HALF, -MEADOW_HALF],
        MEADOW_REACH,
        meadow_ground(),
        meadow_cover(),
        meadow_blades(),
    )
    .unwrap_or_else(|why| unreachable!("the meadow's own field: {why}"))
}

/// One texel of the intensity layer, by its index along `+X`.
///
/// **The wrap seam is the boundary.** The layer's period is
/// `MEADOW_INTENSITY_TEXELS · MEADOW_INTENSITY_METRES_PER_TEXEL` metres and its
/// texel `k` stands at `(k + 0.5)` texels from the world origin, so the second
/// half of the array is the world's `-X` side and the first half is its `+X`
/// side. Full in the second half and zero in the first is therefore "windy where
/// `x < 0`, calm where `x > 0`" with the transition sitting on `x = 0`, where
/// the bare path is.
fn meadow_intensity_texel(column: u32) -> u8 {
    if column >= MEADOW_INTENSITY_TEXELS / 2 {
        255
    } else {
        0
    }
}

/// The two authored wind layers, as the renderer uploads them.
#[must_use]
pub fn meadow_wind_layers() -> WindLayers {
    let side = MEADOW_INTENSITY_TEXELS;
    let mut intensity = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..side {
        for column in 0..side {
            intensity.extend_from_slice(&[meadow_intensity_texel(column), 0, 0, 255]);
        }
    }
    let direction_texels = MEADOW_DIRECTION_TEXELS * MEADOW_DIRECTION_TEXELS;
    WindLayers {
        // `(255, 128)` is the complex identity, so the prevailing wind reaches
        // every blade unturned — this fixture's claim is about intensity, and a
        // deflection would make "which way a blade leans" a second variable.
        direction: WindLayer {
            texels: [MEADOW_DIRECTION_TEXELS; 2],
            rgba8: [255u8, 128, 0, 255].repeat(direction_texels as usize),
        },
        intensity: WindLayer {
            texels: [side; 2],
            rgba8: intensity,
        },
    }
}

/// The authoritative CPU field the layers above describe.
///
/// **This is what narrows the `f64` formula to `f32`** — see
/// `crcbl_wind::WindField::gpu_params`, which is the one place that happens —
/// so the fixture hands the renderer numbers physics would have read rather
/// than a second derivation of them.
///
/// # Panics
///
/// Never for the constants above.
#[must_use]
pub fn meadow_wind_field() -> WindField {
    let layers = meadow_wind_layers();
    let direction_grid = LayerGrid::new(
        MEADOW_DIRECTION_TEXELS,
        MEADOW_DIRECTION_TEXELS,
        MEADOW_DIRECTION_METRES_PER_TEXEL,
    )
    .unwrap_or_else(|why| unreachable!("the meadow's direction grid: {why}"));
    let intensity_grid = LayerGrid::new(
        MEADOW_INTENSITY_TEXELS,
        MEADOW_INTENSITY_TEXELS,
        MEADOW_INTENSITY_METRES_PER_TEXEL,
    )
    .unwrap_or_else(|why| unreachable!("the meadow's intensity grid: {why}"));
    let direction = DirectionLayer::from_rgba8(direction_grid, &layers.direction.rgba8)
        .unwrap_or_else(|why| unreachable!("the meadow's direction layer: {why}"));
    let intensity = IntensityLayer::from_rgba8(intensity_grid, &layers.intensity.rgba8)
        .unwrap_or_else(|why| unreachable!("the meadow's intensity layer: {why}"));
    let weather = Weather::from_beaufort(glam::DVec2::X, MEADOW_WEATHER)
        .unwrap_or_else(|why| unreachable!("the meadow's weather: {why}"));
    WindField::new(weather, direction, intensity)
}

/// Where [`Scene::Meadow`] is seen from: on the path, low, looking up the
/// hillside.
///
/// Public so a test can project a world point onto the frame through the
/// matrices the renderer draws with.
///
/// [`Scene::Meadow`]: super::Scene::Meadow
#[must_use]
pub fn meadow_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 1.30, 7.60),
        target: glam::Vec3::new(0.0, -0.10, -1.20),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
        },
    }
}

/// The sun: from behind the camera and high, with **no `x` component**, so the
/// windy and calm halves of the frame are lit alike and the only thing
/// separating them is the lean.
#[must_use]
pub fn meadow_sun() -> crate::render::DirectionalLight {
    crate::render::DirectionalLight {
        direction: glam::Vec3::new(0.0, 1.0, 0.55).normalize(),
        // **A key well above the ambient**, which is what lets a blade read its
        // own colour: the ambient term is neutral, so a field lit mostly by it
        // comes out the grey its albedo's luminance says rather than the green
        // its hue does — which is exactly what the first band claim in
        // `tests/render_e2e/grass.rs` is about.
        ..super::dimmed_sun(0.85, 0.45)
    }
}

/// The sky the field stands under: a pale horizon over a blue zenith.
#[must_use]
pub fn meadow_sky() -> crate::render::Sky {
    crate::render::Sky {
        zenith: glam::Vec3::new(0.030, 0.055, 0.120),
        horizon: glam::Vec3::new(0.330, 0.330, 0.310),
        ground: glam::Vec3::new(0.020, 0.018, 0.014),
    }
}

/// The plate the ground is drawn with — the mesh past the demo scene's four.
const PLATE_MESH: usize = 4;

/// The ground's material: bare earth, which is what the path shows.
const EARTH: usize = 3;

/// The demo scene with the plate and the earth row appended.
fn meadow_scene() -> SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.meshes.push(plate_mesh("meadow ground", 1.0));
    debug_assert_eq!(scene.meshes.len() - 1, PLATE_MESH);
    // **A pale, warm earth rather than a dark one**, and the brightness is a
    // measurement rather than a taste: a card's silhouette against the ground is
    // the largest step in this frame, and a step is where two rasterisers' last
    // bits move a pixel from one side of it to the other. Lifting the ground
    // until its *green* is within the golden's gross-difference threshold of the
    // mat's takes those flips out of that budget entirely, while its red stays
    // over its green — which is the bare-path claim, and what makes it earth.
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [0.190, 0.155, 0.088, 1.0],
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(scene.materials.len() - 1, EARTH);
    scene
}

/// How far past the field the ground plate reaches, in metres.
///
/// Wide enough that its edge is never in frame: the camera is low and the
/// hillside rises away from it, so a plate that stopped at the field's own edge
/// would show the clear colour along the horizon instead of ground.
const GROUND_REACH: f32 = 60.0;

/// The plate, turned onto the ground's plane and stretched over it.
///
/// A rotation of the `+Y` quad onto the plane's normal is exactly the plane,
/// because a rotation about the origin takes the plane through the origin with
/// normal `+Y` to the plane through the origin with the rotated normal — which
/// is the one [`meadow_height`] describes. The scale is applied first so the
/// quad is stretched in its own plane rather than sheared out of it.
fn ground_model() -> glam::Mat4 {
    let turn = glam::Quat::from_rotation_arc(glam::Vec3::Y, meadow_ground_normal());
    glam::Mat4::from_quat(turn) * glam::Mat4::from_scale(glam::Vec3::splat(GROUND_REACH))
}

/// [`Scene::Meadow`] built with or without its field.
///
/// **Public so a test can draw the same frame with no grass**, which is what the
/// off-switch claim compares against: the same renderer, the same ground, the
/// same sky, and a field of [`None`].
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built, and
/// [`OffscreenError::Grass`] if the field or a wind layer cannot be uploaded.
///
/// [`Scene::Meadow`]: super::Scene::Meadow
pub fn meadow_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    grass: bool,
) -> Result<ForwardScene, OffscreenError> {
    meadow_forward_on_path(
        device,
        queue,
        format,
        grass,
        device.preferred_geometry_path(),
    )
}

/// [`meadow_forward`] on exactly the geometry tail `path`, which is how
/// [`Scene::Meadow`]'s build arm honours a requested path.
///
/// # Errors
///
/// [`meadow_forward`]'s, and [`OffscreenError::Hal`] carrying
/// `HalError::UnsupportedFeatures` if the device lacks `path`.
///
/// [`Scene::Meadow`]: super::Scene::Meadow
pub(super) fn meadow_forward_on_path(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    grass: bool,
    path: GeometryPath,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer =
        ForwardRenderer::with_scene_on_path(device, queue, format, &meadow_scene(), path)?;
    renderer.set_sky(meadow_sky());
    place(&mut renderer, PLATE_MESH, EARTH, ground_model());
    if grass {
        let field = meadow_field();
        if let Err(error) = build_grass(device, queue, &mut renderer, &field) {
            renderer.destroy(device);
            return Err(error);
        }
    }
    Ok(ForwardScene {
        camera: meadow_camera(),
        sun: meadow_sun(),
        renderer: Box::new(renderer),
    })
}

/// The field, the two layers and the weather, in the order a caller sets them.
fn build_grass(
    device: &dyn Device,
    queue: QueueHandle,
    renderer: &mut ForwardRenderer,
    field: &GrassField,
) -> Result<(), OffscreenError> {
    renderer.set_grass(device, queue, Some(field))?;
    renderer.set_wind_layers(device, queue, Some(&meadow_wind_layers()))?;
    // The block the GPU copy reads, narrowed from the authoritative `f64`
    // formula at the camera this fixture draws from — decision 3's "the
    // renderer receives the offset relative to the camera".
    let eye = meadow_camera().eye;
    renderer.set_wind(meadow_wind_field().gpu_params(glam::DVec3::new(
        f64::from(eye.x),
        f64::from(eye.y),
        f64::from(eye.z),
    )));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The reach covers the whole field**, so nothing this fixture draws is
    /// distance-culled and every instance count a claim reads is the
    /// placement's alone.
    #[test]
    fn the_reach_covers_the_whole_field() {
        let eye = meadow_camera().eye;
        let mut furthest = 0.0f32;
        for x in [-MEADOW_HALF, MEADOW_HALF] {
            for z in [-MEADOW_HALF, MEADOW_HALF] {
                let root = glam::Vec3::new(x, meadow_height(x, z), z);
                furthest = furthest.max(root.distance(eye));
            }
        }
        eprintln!("meadow: the furthest corner is {furthest:.3} m from the eye");
        assert!(
            furthest < MEADOW_REACH,
            "a corner {furthest} m away is past the field's reach of {MEADOW_REACH} m"
        );
    }

    /// **The path is wider than the wind's blend**, which is what makes "calm
    /// means still" an equality: a blade can only stand where the intensity
    /// layer's four taps are all on one side of the boundary.
    #[test]
    fn the_path_is_wider_than_the_winds_blend() {
        let blend = MEADOW_INTENSITY_METRES_PER_TEXEL as f32;
        assert!(
            MEADOW_PATH_HALF_WIDTH >= blend,
            "a path {MEADOW_PATH_HALF_WIDTH} m wide either side of the boundary does not clear \
             the intensity layer's {blend} m blend"
        );
        let field = meadow_wind_field();
        // Sampled through the authoritative copy at the nearest position a blade
        // can stand: full on one side, exactly nothing on the other.
        let windy = field.sample(glam::DVec3::new(
            -f64::from(MEADOW_PATH_HALF_WIDTH),
            0.0,
            0.0,
        ));
        let calm = field.sample(glam::DVec3::new(
            f64::from(MEADOW_PATH_HALF_WIDTH),
            0.0,
            0.0,
        ));
        eprintln!(
            "meadow: the wind is {windy} at the path's windy edge and {calm} at its calm one"
        );
        assert!(windy.length() > 8.0, "the windy edge reads {windy}");
        assert_eq!(calm, glam::DVec3::ZERO, "the calm edge reads {calm}");
    }

    /// **The four quadrants differ in one thing each**, which is what every
    /// claim rests on: the cover map at a point of tile 0 and the mirror of it
    /// in tile 2 agree about the row and differ about the density, and the pair
    /// across `x` agree about both.
    #[test]
    fn the_quadrants_differ_in_one_thing_each() {
        let far = meadow_cover_texel(-2.0, -2.0);
        let near = meadow_cover_texel(-2.0, 2.0);
        assert_eq!(far, [MEADOW_DENSE, 0]);
        assert_eq!(near, [MEADOW_SPARSE, 0]);
        assert_eq!(meadow_cover_texel(2.0, 2.0), [MEADOW_SPARSE, 0]);
        // The path, and it is bare on both sides of the boundary.
        assert_eq!(meadow_cover_texel(-0.5, 0.0), [0, 0]);
        assert_eq!(meadow_cover_texel(0.5, 0.0), [0, 0]);
        // And the second row's patch is in tile 1: far and calm.
        assert_eq!(meadow_cover_texel(3.0, -5.0), [MEADOW_DENSE, 1]);
    }

    /// **The heightfield is the plane the plate is turned onto**, which is what
    /// keeps the blades standing on the ground rather than above or through it.
    #[test]
    fn the_ground_field_and_the_plate_are_one_plane() {
        let ground = meadow_ground();
        let model = ground_model();
        for (x, z) in [(0.0, 0.0), (3.0, -3.0), (-3.5, 2.5), (1.25, 0.75)] {
            let (sampled, normal) = ground.under(x, z);
            let wanted = meadow_height(x, z);
            assert!(
                (sampled - wanted).abs() < 1e-5,
                "the field reads {sampled} at ({x}, {z}) where the plane is {wanted}"
            );
            assert!(
                (glam::Vec3::from(normal) - meadow_ground_normal()).length() < 1e-5,
                "the field's normal at ({x}, {z}) is {normal:?}"
            );
            // The plate's own surface, found by taking a point of the unit quad
            // through the model matrix — it has to land on the same plane.
            let on_quad = model.transform_point3(glam::Vec3::new(x / GROUND_REACH, 0.0, 0.0));
            let plate = meadow_height(on_quad.x, on_quad.z);
            assert!(
                (on_quad.y - plate).abs() < 1e-4,
                "the plate's point {on_quad:?} is not on the ground's plane"
            );
        }
        // And the ground is not level, or the normal claim above would hold for
        // a generation pass that answered `+Y` and never read the field.
        assert!(
            meadow_ground_normal().y < 0.999,
            "a level ground makes the normal claim vacuous"
        );
    }

    /// The sun and the camera have no `x` component, so the windy and the calm
    /// halves of the frame are lit alike — `still_pool`'s mirrored-control
    /// argument, over the axis this fixture splits.
    #[test]
    fn the_halves_differ_only_in_the_wind() {
        let camera = meadow_camera();
        assert_eq!(camera.eye.x, 0.0);
        assert_eq!(camera.target.x, 0.0);
        assert_eq!(meadow_sun().direction.x, 0.0);
    }
}
