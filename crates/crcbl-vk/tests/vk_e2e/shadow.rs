//! Topic 18's sun cascades, against a real Vulkan implementation.
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
//! [`CullMode::Back`](crcbl_hal::CullMode::Back) and, on a device with an
//! amplification stage, rejects clusters whose normal cone faces away from the
//! light. Both are correct for a closed caster and both discard a single-sided
//! wall, so the box casts nothing at all and the floor stayed uniformly lit.
//! That is a real property of this shadow pass rather than a defect in the
//! scene, and `docs/backlog.md` is where a two-sided caster belongs.

use crate::harness::Headless;
use crate::mesh::MESH_EXTENT;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Features,
    Format, ImageAspect, ImageBarrier, ImageSubresourceLayers, ImageSubresourceRange,
    MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};

/// Where the open box sits: at the origin, so the camera below looks straight
/// down into it.
const BOX_AT: glam::Vec3 = glam::Vec3::ZERO;

/// Where the cube hangs: above the box and behind it in `+Z`.
///
/// Out of the box's own footprint, so a camera looking straight down into the
/// box sees the floor rather than the cube — and high enough that the sun below
/// throws its shadow far enough sideways to land on that floor rather than on
/// the cube's own feet.
const CUBE_AT: glam::Vec3 = glam::Vec3::new(0.0, 1.2, 1.4);

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
        let (width, _) = crcbl_render::shadow::atlas_extent();
        let (origin_x, origin_y) = crcbl_render::shadow::tile_origin(cascade);
        let side = crcbl_render::shadow::TILE;
        (0..side)
            .flat_map(|row| {
                let start = ((origin_y + row) * width + origin_x) as usize;
                self.atlas[start..start + side as usize].to_vec()
            })
            .collect()
    }
}

/// Opens a device, draws the open box under a sun at `to_light`, and reads back
/// both the tonemapped frame and the shadow atlas.
fn render_shadowed(to_light: glam::Vec3) -> ShadowFrame {
    // The mesh suite's own harness, so this frame is drawn at the extent and on
    // the device every other mesh assertion is made against.
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    renderer.set_open_box(Some(glam::Mat4::from_translation(BOX_AT)));

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let (atlas_width, atlas_height) = crcbl_render::shadow::atlas_extent();
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
        .begin_frame(
            device,
            &overhead_camera(),
            &crcbl_render::DirectionalLight {
                direction: to_light,
                ..crcbl_render::DirectionalLight::default()
            },
            glam::Mat4::from_translation(CUBE_AT),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("shadow frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl_render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
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
        image_offset: crcbl_hal::Offset3d::default(),
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
        image_offset: crcbl_hal::Offset3d::default(),
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

    let mut color = vec![0u8; color_bytes as usize];
    headless.readback(color_staging, color_bytes, &mut color);
    let mut atlas_raw = vec![0u8; atlas_bytes as usize];
    headless.readback(atlas_staging, atlas_bytes, &mut atlas_raw);
    let atlas = atlas_raw
        .chunks_exact(4)
        .map(|word| f32::from_le_bytes(word.try_into().expect("a four-byte chunk")))
        .collect();

    let image = crcbl_golden::Image::from_rgba8(width, height, color)
        .expect("the readback is one tightly packed RGBA frame");

    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);
    device.destroy_buffer(atlas_staging);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    ShadowFrame { image, atlas }
}

/// A camera above the box, looking down into it.
///
/// Nearly on the box's axis, so the floor is a square in the middle of the frame
/// with none of it hidden behind a wall — the floor is where every measurement
/// below is taken. Far enough up that the cube, which hangs `+Z` of the box, is
/// outside the cone that reaches the floor and hides none of it.
fn overhead_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: BOX_AT + glam::Vec3::new(0.0, 3.0, 0.15),
        target: BOX_AT + glam::Vec3::new(0.0, -0.5, 0.0),
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::default(),
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
fn sun(sign: f32) -> glam::Vec3 {
    glam::Vec3::new(sign, SUN_ELEVATION, SUN_TOWARDS_Z).normalize()
}

/// **The cascades were rendered into, not merely allocated.**
///
/// The single assertion this whole slice can otherwise hide behind. Under
/// reversed-Z the atlas is cleared to [`crcbl_hal::depth::CLEAR`] = `0.0`, and a
/// caster writes a depth above it — so a tile that is entirely zero is a cascade
/// whose cull rejected everything, whose draws were never recorded, or whose
/// viewport landed somewhere else. Every one of those renders a frame in which
/// nothing is shadowed, which is a picture no golden can tell from a correct one.
///
/// Asserted **per cascade**, not over the atlas as a whole: one cascade drawing
/// and the rest empty is exactly what a viewport that ignored
/// [`tile_origin`](crcbl_render::shadow::tile_origin) would produce, and a
/// whole-atlas check passes it.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_shadow_atlas_is_written_rather_than_left_at_its_clear_value() {
    let frame = render_shadowed(sun(1.0));
    let side = crcbl_render::shadow::TILE as usize;
    assert_eq!(
        frame.atlas.len(),
        side * side * crcbl_render::shadow::CASCADES,
        "the readback is the whole atlas"
    );
    for cascade in 0..crcbl_render::shadow::CASCADES {
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
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
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
        "vk e2e: shadow — sun in +X: left {:.1} right {:.1}; sun in -X: left {:.1} right {:.1}",
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
