//! Topic 18's shadow maps — the sun's cascades, a shadowed spot and a shadowed
//! point light — on whichever backend `CRCBL_GPU` names.
//!
//! # What this module exists to rule out
//!
//! **A shadow pass that renders nothing produces a frame that looks entirely
//! plausible.** Every surface is lit, no artifact appears anywhere, and a golden
//! blessed from that frame passes for ever. So does a pass that renders into a
//! map nothing ever samples. Neither is visible in a picture, which means a
//! golden image is exactly the wrong instrument here and the assertions below
//! are the right one:
//!
//! * [`the_shadow_atlas_is_written_rather_than_left_at_its_clear_value`] copies
//!   the atlas back and looks at the depth in it. The reversed-Z clear is `0.0`;
//!   a caster writes something above it. This is the assertion that separates
//!   "the cascades rendered" from "the cascades exist".
//! * [`a_wall_darkens_the_floor_it_stands_on_and_the_sun_decides_which_half`]
//!   is the other half — that the map is *sampled*, and that what it darkens
//!   moves when the light does. A shadow map can be perfectly written and read
//!   through a matrix that disagrees with the one it was rendered with; the
//!   result is a shadow in the wrong place, which only a comparison between two
//!   light directions catches.
//!
//! # The scene, and why it needs no geometry of its own
//!
//! The **cube** is the caster and the **open box's floor** is the receiver, both
//! meshes the engine already has. The cube hangs above and behind the box, the
//! sun comes in over one shoulder, and its shadow falls across one half of the
//! floor; reverse the sun and it falls across the other.
//!
//! The box's *walls* are deliberately not the caster, and the reason is worth
//! writing down because it is the first thing that was tried. Its five faces
//! point **inward**, so a sun outside the box sees every one of them from
//! behind — and the shadow pass rasterises with
//! [`CullMode::Back`](crcbl::hal::CullMode::Back) and, on a device with an
//! amplification stage, rejects clusters whose normal cone faces away from the
//! light. Both are correct for a closed caster and both discard a single-sided
//! wall, so the box casts nothing at all and the floor stayed uniformly lit.
//! That is a real property of this shadow pass rather than a defect in the
//! scene, and `docs/backlog.md` is where a two-sided caster belongs.
//!
//! The spot's scene is separate and its own section below says why: a light that
//! is not the sun needs a caster the *light* can see past and the *camera*
//! cannot, which the sun's overhead arrangement does not give.

use crate::harness::{Headless, MESH_EXTENT, poisoned};
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Features,
    Format, ImageAspect, ImageBarrier, ImageSubresourceLayers, ImageSubresourceRange,
    MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};

/// Where the open box sits: at the origin, so the camera below looks straight
/// down into it.
const BOX_AT: crcbl::math::Vec3 = crcbl::math::Vec3::ZERO;

/// Where the cube hangs: above the box and behind it in `+Z`.
///
/// Out of the box's own footprint, so a camera looking straight down into the
/// box sees the floor rather than the cube — and high enough that the sun below
/// throws its shadow far enough sideways to land on that floor rather than on
/// the cube's own feet.
const CUBE_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.0, 1.2, 1.4);

/// The sun's elevation, as the `y` of a direction whose horizontal part is `1`.
///
/// **Chosen so the shadow covers about half the floor and not all of it.** A
/// caster drops `d` onto the floor and its shadow moves `d / SUN_ELEVATION`
/// sideways: too low and the whole floor is dark, so the comparison below has
/// nothing to compare; too high and the shadow lands under the cube and never
/// reaches the box.
const SUN_ELEVATION: f32 = 2.0;

/// How far the sun is towards `+Z`, on [`SUN_ELEVATION`]'s terms.
///
/// The cube is behind the box in `+Z`, so a sun with no `z` at all would throw
/// its shadow along `x` and leave it `+Z` of the floor entirely. This is what
/// walks it back over the box.
const SUN_TOWARDS_Z: f32 = 1.0;

/// A frame drawn under one sun, with its shadow atlas read back beside it.
struct ShadowFrame {
    image: crcbl_golden::Image,
    /// The atlas as depth, row-major, `atlas_extent()` of them.
    atlas: Vec<f32>,
}

impl ShadowFrame {
    /// Cascade `cascade`'s tile, as depths.
    fn tile(&self, cascade: usize) -> Vec<f32> {
        let (width, _) = crcbl::render::shadow::atlas_extent();
        let (origin_x, origin_y) = crcbl::render::shadow::tile_origin(cascade);
        let side = crcbl::render::shadow::TILE;
        (0..side)
            .flat_map(|row| {
                let start = ((origin_y + row) * width + origin_x) as usize;
                self.atlas[start..start + side as usize].to_vec()
            })
            .collect()
    }
}

/// What one frame of this module draws.
///
/// A struct rather than four arguments, because there are two scenes here now —
/// the sun's wall and the spot's caster — and every line of the plumbing below
/// is the same for both. What differs is exactly these four things.
struct ShadowScene<'a> {
    /// What goes in the frame besides the cube: the box for the sun's scene, the
    /// pyramid for the spot's.
    prepare: &'a dyn Fn(&mut crcbl::render::ForwardRenderer),
    camera: crcbl::render::Camera,
    sun: crcbl::render::DirectionalLight,
    /// Where the caller puts the cube — the wall's hanging caster in the sun's
    /// scene, the floor in the spot's.
    model: crcbl::math::Mat4,
}

/// Opens a device, draws `scene`, and reads back both the tonemapped frame and
/// the shadow atlas.
fn render_scene(scene: &ShadowScene<'_>) -> ShadowFrame {
    // The mesh suite's own harness, so this frame is drawn at the extent and on
    // the device every other mesh assertion is made against.
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    // The cube first and whatever else the scene wants above it, which is the
    // order the pool has always been filled in.
    crate::harness::place_cube_at(&mut renderer, scene.model);
    (scene.prepare)(&mut renderer);

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let (atlas_width, atlas_height) = crcbl::render::shadow::atlas_extent();
    let color_bytes = u64::from(width) * u64::from(height) * 4;
    // `D32Float`: one channel of four bytes.
    let atlas_bytes = u64::from(atlas_width) * u64::from(atlas_height) * 4;
    let staging = |label, size| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let color_staging = staging("shadow readback", color_bytes);
    let atlas_staging = staging("shadow atlas readback", atlas_bytes);

    renderer
        .begin_frame(device, &scene.camera, &scene.sun, MESH_EXTENT)
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("shadow frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl::render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                final_state: ResourceState::TransferSrc,
            },
        );
        let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        graph.compile(&pool).expect("a legal frame")
    };
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: color_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });

    // **The one hand-written barrier in this module.** The atlas belongs to the
    // renderer, not to the graph, and the graph left it in `ShaderRead` because
    // that is what the colour pass wanted — so a reader outside the frame has to
    // ask for it. Every barrier *inside* the frame is still the graph's.
    let range = ImageSubresourceRange::all(Format::D32Float);
    let to_source = [ImageBarrier::new(
        renderer.shadow_atlas(),
        range,
        ResourceState::ShaderRead,
        ResourceState::TransferSrc,
    )];
    encoder.pipeline_barrier(&Barriers {
        images: &to_source,
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: atlas_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: renderer.shadow_atlas(),
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(atlas_width, atlas_height),
    });

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    device.wait_idle().expect("idle");

    let mut color = poisoned(color_bytes as usize);
    headless.readback(color_staging, color_bytes, &mut color);
    let mut atlas_raw = poisoned(atlas_bytes as usize);
    headless.readback(atlas_staging, atlas_bytes, &mut atlas_raw);
    let atlas = atlas_raw
        .chunks_exact(4)
        .map(|word| f32::from_le_bytes(word.try_into().expect("a four-byte chunk")))
        .collect();

    // Through the ring's own channel order rather than as raw RGBA. The Vulkan
    // original could assume the latter because its fixture only ever opened one
    // format; this fixture takes whatever the surface prefers, and wgpu prefers
    // BGRA. Every measurement below is a mean over the three colour channels, so
    // the order does not change a number — it is here so the pixels a future
    // assertion reads are the pixels it names.
    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    let image = crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image");

    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);
    device.destroy_buffer(atlas_staging);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    ShadowFrame { image, atlas }
}

/// The open box under a sun at `to_light`, with the cube hanging over it.
fn render_shadowed(to_light: crcbl::math::Vec3) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &|renderer| {
            crate::harness::place(
                renderer,
                crcbl::render::scene::DEMO_OPEN_BOX,
                crcbl::render::scene::DEMO_UNTINTED,
                crcbl::math::Mat4::from_translation(BOX_AT),
            );
        },
        camera: overhead_camera(),
        sun: crcbl::render::DirectionalLight {
            direction: to_light,
            ..crcbl::render::DirectionalLight::default()
        },
        model: crcbl::math::Mat4::from_translation(CUBE_AT),
    })
}

/// A camera above the box, looking down into it.
///
/// Nearly on the box's axis, so the floor is a square in the middle of the frame
/// with none of it hidden behind a wall — the floor is where every measurement
/// below is taken. Far enough up that the cube, which hangs `+Z` of the box, is
/// outside the cone that reaches the floor and hides none of it.
fn overhead_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: BOX_AT + crcbl::math::Vec3::new(0.0, 3.0, 0.15),
        target: BOX_AT + crcbl::math::Vec3::new(0.0, -0.5, 0.0),
        up: crcbl::math::Vec3::Y,
        projection: crcbl::render::Projection::default(),
    }
}

/// The mean brightness of a horizontal band of the floor, over `x` in
/// `columns`.
///
/// A band rather than one pixel: PCF makes a shadow edge a gradient a few texels
/// wide, and a single sample lands wherever that gradient happens to be.
fn band_brightness(image: &crcbl_golden::Image, columns: std::ops::Range<u32>) -> f32 {
    let rows = (MESH_EXTENT.1 * 7 / 16)..(MESH_EXTENT.1 * 9 / 16);
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in rows {
        for x in columns.clone() {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// A sun coming from `+X` or `-X` at [`SUN_ELEVATION`].
fn sun(sign: f32) -> crcbl::math::Vec3 {
    crcbl::math::Vec3::new(sign, SUN_ELEVATION, SUN_TOWARDS_Z).normalize()
}

/// **The cascades were rendered into, not merely allocated.**
///
/// The single assertion this whole slice can otherwise hide behind. Under
/// reversed-Z the atlas is cleared to [`crcbl::hal::depth::CLEAR`] = `0.0`, and a
/// caster writes a depth above it — so a tile that is entirely zero is a cascade
/// whose cull rejected everything, whose draws were never recorded, or whose
/// viewport landed somewhere else. Every one of those renders a frame in which
/// nothing is shadowed, which is a picture no golden can tell from a correct one.
///
/// Asserted **per cascade**, not over the atlas as a whole: one cascade drawing
/// and the rest empty is exactly what a viewport that ignored
/// [`tile_origin`](crcbl::render::shadow::tile_origin) would produce, and a
/// whole-atlas check passes it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_shadow_atlas_is_written_rather_than_left_at_its_clear_value() {
    let frame = render_shadowed(sun(1.0));
    let side = crcbl::render::shadow::TILE as usize;
    assert_eq!(
        frame.atlas.len(),
        side * side * crcbl::render::shadow::TILES,
        "the readback is the whole atlas"
    );
    // **The light tiles are untouched in a scene with no shadowed light**, which
    // is the other half of the same claim: a free tile that got drawn into would
    // be a viewport landing where it does not belong, and a cascade's map
    // written over a light's is a picture no golden can see.
    for light_tile in 0..crcbl::render::shadow::LIGHT_TILES {
        let tile = frame.tile(crcbl::render::shadow::light_tile(light_tile));
        assert!(
            tile.iter().all(|depth| *depth == crcbl::hal::depth::CLEAR),
            "light tile {light_tile} holds depths in a frame with no shadowed light in it"
        );
    }
    for cascade in 0..crcbl::render::shadow::CASCADES {
        let tile = frame.tile(cascade);
        let written = tile.iter().filter(|depth| **depth > 0.0).count();
        assert!(
            written > 0,
            "cascade {cascade}'s tile is entirely at the reversed-Z clear, so nothing was \
             rendered into it — which is a shadow map that shadows nothing and a frame that \
             looks correct"
        );
        // Not a stray texel either: the box is a solid object a few units
        // across, and a cascade covering it writes a real area of the tile. One
        // per mille is far below what the geometry occupies and far above what
        // a mis-transformed sliver would.
        let fraction = written as f64 / tile.len() as f64;
        assert!(
            fraction > 0.001,
            "cascade {cascade} wrote only {written} of {} texels ({fraction:.5}), which is a \
             sliver rather than a caster",
            tile.len()
        );
        // And every depth is a legal reversed-Z one, which is what says the
        // projection's range is the one the comparison sampler is testing
        // against rather than an arbitrary scale that happens to be non-zero.
        assert!(
            tile.iter().all(|depth| (0.0..=1.0).contains(depth)),
            "cascade {cascade} holds a depth outside 0..1"
        );
    }
}

/// **The map is sampled, and the sun decides where the shadow lands.**
///
/// The box's `+X` wall stands between a sun in `+X` and the floor, so the floor's
/// `+X` half is dark and its `-X` half is lit. Move the sun to `-X` and the two
/// swap. Both halves of that are asserted, and the second is what makes it
/// evidence: a renderer that darkened one side of everything — a lighting bug, a
/// wrong normal, a vignette — would satisfy the first on its own and cannot
/// satisfy a pair that swaps.
///
/// Measured as a ratio between two bands of the same frame rather than as an
/// absolute colour, for the reason the mesh suite's own light test gives: what
/// survives Lambert, Blinn and the tonemap is which side leads.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_wall_darkens_the_floor_it_stands_on_and_the_sun_decides_which_half() {
    // The outer sixths of the frame are the walls seen edge-on; the inner
    // quarters either side of centre are floor under both suns.
    let left = (MESH_EXTENT.0 * 6 / 16)..(MESH_EXTENT.0 * 8 / 16);
    let right = (MESH_EXTENT.0 * 8 / 16)..(MESH_EXTENT.0 * 10 / 16);

    let from_plus_x = render_shadowed(sun(1.0));
    let from_minus_x = render_shadowed(sun(-1.0));

    let plus = (
        band_brightness(&from_plus_x.image, left.clone()),
        band_brightness(&from_plus_x.image, right.clone()),
    );
    let minus = (
        band_brightness(&from_minus_x.image, left),
        band_brightness(&from_minus_x.image, right),
    );
    eprintln!(
        "crcbl forward e2e: shadow — sun in +X: left {:.1} right {:.1}; sun in -X: left {:.1} right {:.1}",
        plus.0, plus.1, minus.0, minus.1
    );

    // A margin rather than "any difference at all": the two bands are lit by
    // the same light at slightly different angles, so a small difference is
    // ordinary shading. A shadow is not small.
    const MARGIN: f32 = 8.0;
    assert!(
        plus.0 + MARGIN < plus.1,
        "with the sun in +X the cube's shadow must fall across the -X half of the floor, \
         but that half measured {:.1} against {:.1}",
        plus.0,
        plus.1
    );
    assert!(
        minus.1 + MARGIN < minus.0,
        "and the sun in -X must darken the other half, but it measured {:.1} against {:.1}",
        minus.1,
        minus.0
    );
}

// ---------------------------------------------------------------------------
// The shadowed spot
// ---------------------------------------------------------------------------
//
// `docs/plan/18-render-features.md`'s 2026-08-13 slice: a light other than the
// sun that occludes. The scene is `crcbl::screenshot`'s `Scene::SpotShadow` in
// every number that matters — the same floor, the same light, the same camera —
// so a failure here and a moved golden there are the same failure seen twice
// rather than two scenes to keep in step.
//
// What this module adds that the golden cannot is **motion**: the golden is one
// frame, and a shadow painted at a fixed place would match it for ever. Moving
// the caster and watching the dark region follow is the only assertion that
// separates a shadow map from a decal.

/// How far above the floor the spot hangs, and how far towards `+Z`.
///
/// 45° from vertical. `crcbl::screenshot`'s `SPOT_SHADOW_LIGHT_AT` carries the
/// argument for the angle: with the camera overhead and the light overhead too,
/// a shadow falls under the object that casts it and the object's own image
/// covers it.
const SPOT_LIGHT_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.0, 1.2, 1.2);

/// How far above the floor the camera stands, looking straight down.
const SPOT_CAMERA_UP: f32 = 1.3;

/// How much the pyramid is scaled by to get the caster, and how far its base is
/// lifted so it stands on the floor.
const SPOT_CASTER_SCALE: f32 = 0.5;

/// How far the caster is moved off the frame's axis, in world units.
///
/// Far enough that its shadow lands wholly inside one of the two bands measured
/// below, and inside the cone's pool at that distance from the axis — see
/// `SPOT_BAND_COLUMNS`.
const SPOT_CASTER_OFFSET: f32 = 0.30;

/// How much the cube is scaled by to get the floor, and how far it is dropped so
/// its `+Y` face is the plane `y = 0`.
///
/// `crcbl::screenshot`'s `SPOT_FLOOR_SCALE`, and large enough here for the same
/// reason: the floor runs past every edge of the frame, so there is no
/// silhouette anywhere for a band to measure instead of the floor.
const SPOT_FLOOR_SCALE: f32 = 8.0;

/// The rows the bands below are measured over: level with the shadow, a third of
/// the way down from the frame's centre.
///
/// The light comes from `+Z` and `+Z` is the top of the frame, so the shadow
/// falls *down* it. The caster's own image ends about 26 rows below centre and
/// its shadow reaches about 92; this band sits between them.
const SPOT_BAND_ROWS: std::ops::Range<u32> = (MESH_EXTENT.1 / 2 + 36)..(MESH_EXTENT.1 / 2 + 48);

/// How far either side of the frame's axis each band sits, in columns.
///
/// The caster at `SPOT_CASTER_OFFSET` throws its shadow over roughly columns
/// 153 to 191 in this band's rows, and the cone's pool reaches to about column
/// 192 — so a band inside 162..174 is inside the shadow and inside the pool at
/// once, and its mirror is lit floor at the same distance from the cone's axis.
const SPOT_BAND_COLUMNS: (u32, u32) = (34, 46);

/// The spot this suite lights its floor with, on `Scene::SpotShadow`'s terms.
fn spot_light() -> crcbl::render::Light {
    crcbl::render::Light::Spot(crcbl::render::SpotLight {
        position: SPOT_LIGHT_AT,
        radius: 3.4,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        // Along the cone, away from the light: from the light to the floor's
        // centre.
        direction: -SPOT_LIGHT_AT,
        inner_angle: 0.18,
        outer_angle: 0.28,
    })
}

/// A camera straight down over the floor's centre.
fn spot_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, SPOT_CAMERA_UP, 0.0),
        target: crcbl::math::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // direction the light comes from at the top of the frame.
        up: crcbl::math::Vec3::Z,
        projection: crcbl::render::Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// Draws the floor under the spot, with the caster at `caster` on the `x` axis —
/// or with no caster at all.
fn render_spot(caster: Option<f32>) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &move |renderer| {
            renderer.set_lights(&[spot_light()]);
            if let Some(x) = caster {
                // The pyramid's base is at `-0.4` in its own space, so lifting
                // it by that much of the scale stands it on `y = 0`. A caster
                // floating above the floor would hide a shadow detached from it,
                // which is what too much bias looks like.
                crate::harness::place(
                    renderer,
                    crcbl::render::scene::DEMO_PYRAMID,
                    crcbl::render::scene::DEMO_UNTINTED,
                    crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
                        x,
                        0.4 * SPOT_CASTER_SCALE,
                        0.0,
                    )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_CASTER_SCALE)),
                );
            }
        },
        camera: spot_camera(),
        // Dim, so the pool and the shadow in it are the spot's work: a bright
        // sun would light the shadowed floor from a direction the spot's map
        // knows nothing about, and the ratio below would be measuring the sun.
        sun: crcbl::render::DirectionalLight {
            color: crcbl::render::DirectionalLight::default().color * 0.03,
            ambient: crcbl::render::DirectionalLight::default().ambient * 0.09,
            ..crcbl::render::DirectionalLight::default()
        },
        // The cube, scaled into a floor whose `+Y` face is the plane `y = 0`.
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * SPOT_FLOOR_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_FLOOR_SCALE)),
    })
}

/// The mean brightness of the band `SPOT_BAND_COLUMNS` from the frame's axis, on
/// the **world** `+X` side when `sign` is positive.
///
/// **World `+X` is the frame's left.** The camera looks down `-Y` with `+Z` up,
/// and a right-handed basis built from those two puts screen-right at `-X`:
/// `cross((0,-1,0), (0,0,1))` is `(-1,0,0)`. So the flip is here rather than in
/// every caller, and the callers can say "the band the caster is over" and mean
/// it.
fn spot_band(frame: &ShadowFrame, sign: i32) -> f32 {
    let centre = i32::try_from(MESH_EXTENT.0 / 2).expect("a frame edge fits in an i32");
    let near = i32::try_from(SPOT_BAND_COLUMNS.0).expect("a band offset fits in an i32");
    let far = i32::try_from(SPOT_BAND_COLUMNS.1).expect("a band offset fits in an i32");
    let columns = if sign >= 0 {
        (centre - far)..(centre - near)
    } else {
        (centre + near)..(centre + far)
    };
    let columns = u32::try_from(columns.start).expect("inside the frame")
        ..u32::try_from(columns.end).expect("inside the frame");
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in SPOT_BAND_ROWS {
        for x in columns.clone() {
            let pixel = frame.image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// How much brighter the lit band must be than the shadowed one.
///
/// A ratio rather than a difference, on `a_wall_darkens_the_floor_it_stands_on`'s
/// terms: what survives Lambert, the falloff, the cone and the tonemap is which
/// side leads and by how much in proportion. The two bands are the same distance
/// from the cone's axis, so every term but the shadow has the same value in
/// both.
const SPOT_SHADOW_RATIO: f32 = 1.5;

/// **A spot's shadow map is written, and it is the map the slot says it is.**
///
/// The tile a shadowed light was given must hold real depths, and the *other*
/// light tile must not: a viewport that ignored `shadow::tile_origin` would
/// write the cascades' tiles or the free one, and every one of those renders a
/// frame that looks entirely plausible.
///
/// The pass is one `LoadOp::Clear` over the whole atlas, so a tile nothing drew
/// into is exactly `depth::CLEAR` — which is what makes "is it at the clear" a
/// question with a crisp answer rather than a threshold.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_shadowed_spot_fills_the_tile_it_was_given_and_no_other() {
    let frame = render_spot(Some(0.0));
    let held = frame.tile(crcbl::render::shadow::light_tile(0));
    let written = held.iter().filter(|depth| **depth > 0.0).count();
    let fraction = written as f64 / held.len() as f64;
    eprintln!(
        "crcbl forward e2e: spot shadow — slot 0's tile holds {written} written texel(s) ({fraction:.4})"
    );
    assert!(
        written > 0,
        "the one shadowed light's tile is entirely at the reversed-Z clear, so its map was \
         never rendered — which is a spot that shadows nothing and a frame that looks correct"
    );
    // **A real area, not a stray texel.** The caster alone covers about a sixth
    // of this tile — the cone's frustum is barely wider than the pyramid at the
    // depth it stands at — so a sliver is a mis-transformed caster and this
    // floor is well under what a correct one writes.
    //
    // It is deliberately not "most of the tile", even though the floor runs past
    // every edge of the cone and geometrically covers all of it. On a device
    // with an amplification stage the floor's clusters are rejected before they
    // are drawn, because `cluster_survives` transforms a cluster's bounding
    // *centre* by the instance transform and leaves its **radius alone** — which
    // is correct for the rigid transform `GpuInstance::transform` documents and
    // eight times too small for a cube scaled into a floor. Nothing in the
    // picture changes: a receiver missing from its own shadow map is a receiver
    // that is not self-shadowed, which is what a correct bias produces anyway,
    // and `crcbl`'s `the_spot_shadow_scene_draws_the_same_frame_on_every_geometry_path`
    // is what says the two paths agree pixel for pixel. It is in
    // `docs/backlog.md` rather than fixed here.
    assert!(
        fraction > 0.05,
        "slot 0's tile wrote only {fraction:.4} of its texels, which is a sliver rather than \
         a caster"
    );
    assert!(
        held.iter().all(|depth| (0.0..=1.0).contains(depth)),
        "slot 0's tile holds a depth outside 0..1, so its reversed-Z range is not the one \
         the comparison sampler tests against"
    );
    // And every other light tile is untouched, which is what says the viewport
    // went where `tile_origin` put it rather than one tile along — and, since a
    // spot is one tile of a region six wide, that a spot did not render the five
    // faces it does not have.
    for light_tile in 1..crcbl::render::shadow::LIGHT_TILES {
        let free = frame.tile(crcbl::render::shadow::light_tile(light_tile));
        assert!(
            free.iter().all(|depth| *depth == crcbl::hal::depth::CLEAR),
            "light tile {light_tile} holds no map and was written anyway"
        );
    }
}

/// **The caster darkens the floor, and removing it lights that floor back up.**
///
/// The half a golden cannot make: a frame with a dark patch in it is a frame
/// with a dark patch in it, and a `spot_visibility` hard-wired to zero over the
/// caster's own footprint would draw one. What separates a shadow from a decal is
/// that it is *absent* when the caster is, and this is that comparison — the same
/// pixels, the same light, the same floor, differing in one instance.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn removing_a_spots_caster_lights_the_floor_it_darkened() {
    let with = render_spot(Some(SPOT_CASTER_OFFSET));
    let without = render_spot(None);
    let shadowed = spot_band(&with, 1);
    let lit = spot_band(&without, 1);
    eprintln!(
        "crcbl forward e2e: spot shadow — the band measures {shadowed:.1} with the caster and {lit:.1} \
         without it"
    );
    assert!(
        shadowed * SPOT_SHADOW_RATIO < lit,
        "the band under the caster's shadow measures {shadowed:.1}, and the same band with no \
         caster at all measures {lit:.1} — a shadow is the difference between those two, and \
         there is not one here"
    );
}

/// **The dark region follows the caster**, which is the whole claim.
///
/// A shadow map read through a matrix that disagrees with the one it was
/// rendered with puts a shadow somewhere — often somewhere plausible — and a
/// single frame cannot tell that from a correct one. Two frames can: the caster
/// moves from `-X` to `+X` and the dark band has to move with it, which no fixed
/// patch and no lighting bug can do.
///
/// Both halves are asserted, and the second is what makes it evidence: a
/// renderer that darkened one side of everything satisfies the first on its own.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_spots_shadow_follows_its_caster() {
    let from_plus_x = render_spot(Some(SPOT_CASTER_OFFSET));
    let from_minus_x = render_spot(Some(-SPOT_CASTER_OFFSET));

    // `.0` is the band on the world `-X` side, `.1` the one on `+X`.
    let plus = (spot_band(&from_plus_x, -1), spot_band(&from_plus_x, 1));
    let minus = (spot_band(&from_minus_x, -1), spot_band(&from_minus_x, 1));
    eprintln!(
        "crcbl forward e2e: spot shadow — caster in +X: over it {:.1} across from it {:.1}; caster in -X: \
         over +X {:.1} over -X {:.1}",
        plus.1, plus.0, minus.1, minus.0
    );

    assert!(
        plus.1 * SPOT_SHADOW_RATIO < plus.0,
        "a caster in +X must darken the +X band, but it measured {:.1} against {:.1} on the \
         other side",
        plus.1,
        plus.0
    );
    assert!(
        minus.0 * SPOT_SHADOW_RATIO < minus.1,
        "and moving it to -X must darken the other band, but it measured {:.1} against {:.1}",
        minus.0,
        minus.1
    );
}

// ---------------------------------------------------------------------------
// The shadowed point light
// ---------------------------------------------------------------------------
//
// `docs/plan/18-render-features.md`'s six-tiles decision: one light, six atlas
// tiles, and a face picked per fragment out of the direction to the light. The
// scene is `crcbl::screenshot`'s `Scene::PointShadow` in every number that
// matters, so a failure here and a moved golden there are the same failure seen
// twice.
//
// What this module adds that the golden cannot is the **atlas**: which of the six
// tiles were rendered into and which was not. A frame drawn through five correct
// faces and one wrong one is a picture; a tile that holds depths where its face
// looks at empty sky is not.

/// How far above the floor the point light hangs, on `Scene::PointShadow`'s
/// terms: low enough that the floor past `|x| = POINT_LIGHT_UP` is on the side
/// faces rather than under the light on the `-Y` one.
const POINT_LIGHT_UP: f32 = 0.5;

/// How far its reach is, in world units.
const POINT_REACH: f32 = 3.0;

/// How far from the light's axis a caster stands.
const POINT_CASTER_AT: f32 = 0.6;

/// How much the pyramid is scaled by to get a caster.
const POINT_CASTER_SCALE: f32 = 0.3;

/// How far above the floor the camera stands, looking straight down.
const POINT_CAMERA_UP: f32 = 2.2;

/// How far out along a shadow's own axis the bands below are measured.
///
/// Past the caster's own image and past `POINT_LIGHT_UP`, which is what puts the
/// band on a side face of the light's map — `crcbl`'s `POINT_SHADOW_AT` carries
/// the full argument.
const POINT_BAND_AT: f32 = 1.0;

/// The half-extent of each band, in pixels.
const POINT_BAND: u32 = 6;

/// The point light this suite lights its floor with.
fn point_light() -> crcbl::render::Light {
    crcbl::render::Light::Point(crcbl::render::PointLight {
        position: crcbl::math::Vec3::new(0.0, POINT_LIGHT_UP, 0.0),
        radius: POINT_REACH,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
    })
}

/// A camera straight down over the floor's centre.
fn point_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, POINT_CAMERA_UP, 0.0),
        target: crcbl::math::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`.
        up: crcbl::math::Vec3::Z,
        projection: crcbl::render::Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// A caster standing on the floor at `at`.
///
/// The pyramid's base is at `-0.4` in its own space, so lifting it by that much
/// of the scale stands it on `y = 0`. A caster floating above the floor would
/// hide a shadow detached from it, which is what too much bias looks like.
fn point_caster(at: crcbl::math::Vec3) -> crcbl::math::Mat4 {
    crcbl::math::Mat4::from_translation(
        at + crcbl::math::Vec3::new(0.0, 0.4 * POINT_CASTER_SCALE, 0.0),
    ) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(POINT_CASTER_SCALE))
}

/// Draws the floor under the point light with one caster at `caster`, and a
/// second one at `second` — or with neither.
fn render_point(
    caster: Option<crcbl::math::Vec3>,
    second: Option<crcbl::math::Vec3>,
) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &move |renderer| {
            renderer.set_lights(&[point_light()]);
            // Two rows rather than one, so the pair is two objects in the frame
            // rather than one drawn twice — the untinted row first, on the order
            // the pool has always been filled in.
            for (at, material) in [
                (caster, crcbl::render::scene::DEMO_UNTINTED),
                (second, crcbl::render::scene::DEMO_TINTED),
            ] {
                if let Some(at) = at {
                    crate::harness::place(
                        renderer,
                        crcbl::render::scene::DEMO_PYRAMID,
                        material,
                        point_caster(at),
                    );
                }
            }
        },
        camera: point_camera(),
        // Dim, on `render_spot`'s terms: a bright sun would light the shadowed
        // floor from a direction this light's map knows nothing about, and the
        // ratios below would be measuring the sun.
        sun: crcbl::render::DirectionalLight {
            color: crcbl::render::DirectionalLight::default().color * 0.03,
            ambient: crcbl::render::DirectionalLight::default().ambient * 0.09,
            ..crcbl::render::DirectionalLight::default()
        },
        // The cube, scaled into a floor whose `+Y` face is the plane `y = 0`.
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * SPOT_FLOOR_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_FLOOR_SCALE)),
    })
}

/// The mean brightness of the band of floor at `(x, z)` in **world** units.
///
/// **World `+X` is the frame's left and `+Z` its top**, on `spot_band`'s terms
/// exactly: the camera looks down `-Y` with `+Z` up, and a right-handed basis
/// built from those two puts screen-right at `-X`.
fn point_band(frame: &ShadowFrame, x: f32, z: f32) -> f32 {
    // The frame's short half-axis covers `POINT_CAMERA_UP * tan(30°)` of floor.
    let pixels_per_unit = (f64::from(MESH_EXTENT.1) / 2.0)
        / (f64::from(POINT_CAMERA_UP) * (30.0f64).to_radians().tan());
    let centre = |extent: u32, world: f32| {
        let half = f64::from(extent) / 2.0;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "every band this module measures is inside the frame"
        )]
        let pixel = (half - f64::from(world) * pixels_per_unit) as u32;
        pixel
    };
    let column = centre(MESH_EXTENT.0, x);
    let row = centre(MESH_EXTENT.1, z);
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in row.saturating_sub(POINT_BAND)..(row + POINT_BAND).min(MESH_EXTENT.1) {
        for x in column.saturating_sub(POINT_BAND)..(column + POINT_BAND).min(MESH_EXTENT.0) {
            let pixel = frame.image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// **A point light's tiles are its six faces, in `shadow::face_axis`' order.**
///
/// The assertion that pins the face convention at the *atlas* rather than in the
/// picture, and it is deliberately about which tile is **empty**: the light hangs
/// over a floor with nothing above it, so the `+Y` face — index 2, and the only
/// index this scene can name from outside — looks at empty sky and its tile has
/// to come back at the clear. A renderer that rendered one face six times, or
/// built the six in another order, writes depths there.
///
/// The two casters are what make the other half checkable without depending on
/// which geometry path ran: on a device with an amplification stage the floor's
/// own clusters are rejected before they reach this map — `cluster_survives`
/// leaves a cluster's bounding radius unscaled and the floor is a scaled cube,
/// which `a_shadowed_spot_fills_the_tile_it_was_given_and_no_other` records in
/// full — so the only thing certain to be in *any* face is a caster. One stands
/// out along `+X` and one out along `-Z`, so faces 0 and 5 hold geometry on every
/// path.
///
/// The pass is one `LoadOp::Clear` over the whole atlas, so a tile nothing drew
/// into is exactly `depth::CLEAR` — which makes "is it at the clear" a question
/// with a crisp answer rather than a threshold.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_shadowed_point_lights_faces_are_the_six_the_host_built() {
    let frame = render_point(
        Some(crcbl::math::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
        Some(crcbl::math::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
    );
    // The one light is the only candidate and the region is empty, so `Selection`
    // gives it the whole of it from tile 0 — which is the base `mesh.slang` reads
    // off the row and adds a face to.
    let mut written = [0usize; crcbl::render::shadow::POINT_FACES];
    for (face, count) in written.iter_mut().enumerate() {
        let tile = frame.tile(crcbl::render::shadow::light_tile(face));
        *count = tile.iter().filter(|depth| **depth > 0.0).count();
        assert!(
            tile.iter().all(|depth| (0.0..=1.0).contains(depth)),
            "face {face} holds a depth outside 0..1, so its reversed-Z range is not the one the \
             comparison sampler tests against"
        );
    }
    eprintln!("crcbl forward e2e: point shadow — written texels per face {written:?}");

    // `+Y`, which `shadow::face_axis` puts at index 2.
    assert_eq!(
        written[2], 0,
        "face 2 looks straight up from a light with nothing above it and holds {} written \
         texel(s), so the six matrices are not the six faces `shadow::face_axis` names",
        written[2]
    );
    // `+X` and `-Z`, which each hold a caster. A real area rather than a stray
    // texel: a caster covers a few per cent of a face's tile — the light is close
    // to it and a face is a 90° frustum — and a mis-transformed sliver covers far
    // less.
    let texels = f64::from(crcbl::render::shadow::TILE).powi(2);
    for face in [0, 5] {
        assert!(
            written[face] > 0,
            "face {face} has a caster standing in it and is entirely at the reversed-Z clear, so \
             its map was never rendered — a face that shadows nothing and a frame that looks \
             correct"
        );
        let fraction = written[face] as f64 / texels;
        assert!(
            fraction > 0.01,
            "face {face} wrote only {fraction:.4} of its texels, which is a sliver rather than a \
             caster"
        );
    }
}

/// **The dark region follows the caster from one face of the map to another**,
/// which is the claim six tiles exist for.
///
/// A caster out along `+X` darkens the floor `+X` of the light and leaves the
/// floor `-Z` of it lit; move the same caster to `-Z` and the two swap. Neither
/// frame can be told from a working one on its own — the first is exactly what a
/// face lookup stuck on `+X` draws — and no lighting bug, no fixed patch and no
/// single working face can produce both.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_point_lights_shadow_follows_its_caster_from_one_face_to_another() {
    let along_x = render_point(
        Some(crcbl::math::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
        None,
    );
    let along_z = render_point(
        Some(crcbl::math::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
        None,
    );

    // `.0` is the band out along `+X`, `.1` the one out along `-Z`.
    let with_x = (
        point_band(&along_x, POINT_BAND_AT, 0.0),
        point_band(&along_x, 0.0, -POINT_BAND_AT),
    );
    let with_z = (
        point_band(&along_z, POINT_BAND_AT, 0.0),
        point_band(&along_z, 0.0, -POINT_BAND_AT),
    );
    eprintln!(
        "crcbl forward e2e: point shadow — caster in +X: +X band {:.1} -Z band {:.1}; caster in -Z: \
         +X band {:.1} -Z band {:.1}",
        with_x.0, with_x.1, with_z.0, with_z.1
    );

    assert!(
        with_x.0 * SPOT_SHADOW_RATIO < with_x.1,
        "a caster in +X must darken the +X band and leave the -Z one lit, but they measured \
         {:.1} and {:.1}",
        with_x.0,
        with_x.1
    );
    assert!(
        with_z.1 * SPOT_SHADOW_RATIO < with_z.0,
        "and moving it to -Z must darken the -Z band instead, but they measured {:.1} and \
         {:.1} — a face lookup stuck on one face draws the first frame and not this one",
        with_z.1,
        with_z.0
    );
}
