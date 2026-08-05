use crate::harness::{Headless, instance};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, CompositeAlpha, Device,
    DeviceDesc, Extent3d, Features, Format, ImageAspect, ImageSubresourceLayers, Instance,
    MemoryLocation, PresentInfo, PresentMode, ResourceState, SubmitInfo, SwapchainDesc,
};

mod drawing;
mod filtering;
mod rotation;

// --- slice 4: the sprite pass, and the first pixels it is shown to draw -------
//
// Everything above this line about sprites was a recorder assertion — instance
// bytes, batching, draw ranges, teardown. `docs/backlog.md` said so under
// "Coverage gaps": nothing in `crcbl-sprite` or `crcbl_render::sprite_pass` had
// ever been checked against a real image or a real renderer, and the first
// honest check of that is a golden image through the pass. This section is it.
//
// Four things are asserted, and the last two are the ones that could not have
// been faked from the CPU side:
//
// 1. A sprite is drawn at all, the right way up, in the right place, showing the
//    frame it named. The test sheet is deliberately **asymmetric** — a different
//    colour in each corner of each frame — because `sprite.slang`'s vertex stage
//    warns that a flipped V renders every sprite upside down while looking
//    entirely plausible on a symmetric image, and a symmetric test image is how
//    that ships.
// 2. Alpha blending composites onto what is already there rather than replacing
//    it, checked against the linear-light blend arithmetic rather than eyeballed.
// 3. `SampleMode::Pixel` and `SampleMode::Smooth` produce visibly different
//    pictures at a non-integer scale, in the place and by the amount predicted.
//    That difference is the only real evidence that sharp-bilinear happened.
// 4. `Pixel` at a whole scale is **exactly flat inside each texel** — which is
//    the whole difference between sharp-bilinear and plain linear, and is
//    asserted on sampled pixel values rather than only through a reference.

/// The size the sprite suite renders at.
///
/// The same 256×192 as the mesh and the triangle, for the same reason: the
/// golden's structural metric averages over 8×8 blocks.
pub(crate) const SPRITE_EXTENT: (u32, u32) = (256, 192);

/// The background every sprite frame composites onto, in **linear** light —
/// which is what a `vkClearColorValue` on an `Rgba8UnormSrgb` attachment means.
///
/// Deliberately not black and not grey: alpha blending onto black is the one
/// background where `src * a + dst * (1 - a)` and `src * a` agree, so a
/// premultiplication mistake would be invisible.
pub(crate) const SPRITE_CLEAR: [f32; 4] = [0.10, 0.20, 0.35, 1.0];

/// Half the visible world height, chosen so **one world unit is one device
/// pixel**: 96 up and down over 192 rows, and 96 × 256/192 = 128 left and right
/// over 256 columns.
///
/// That is what makes every placement assertion below arithmetic rather than a
/// number read off a picture.
const SPRITE_HALF_HEIGHT: f32 = 96.0;

/// The camera every sprite frame is drawn with: orthographic, looking down −Z at
/// the plane the sprites live on.
fn sprite_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 0.0, 1.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::Orthographic {
            half_height: SPRITE_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// World → device pixels under [`sprite_camera`].
///
/// `x + 128` because world x runs −128..128 across 256 columns, and `96 − y`
/// because world Y is **up** and a framebuffer's rows run down.
/// [`assert_the_camera_maps_a_world_unit_to_a_pixel`] checks this against the
/// real matrix rather than leaving it as a claim.
pub(crate) fn world_to_pixel(world: [f32; 2]) -> [f32; 2] {
    [world[0] + 128.0, 96.0 - world[1]]
}

/// Confirms [`world_to_pixel`] agrees with the matrix the pass is actually
/// handed, including the Y flip `crcbl-vk`'s negative-height viewport applies.
///
/// Called from every placement test, because every one of them reads a pixel at
/// a coordinate this function produced: if the mapping is wrong the assertions
/// are checking the wrong pixels and would happily pass on a blank frame.
pub(crate) fn assert_the_camera_maps_a_world_unit_to_a_pixel() {
    let (width, height) = SPRITE_EXTENT;
    let aspect = width as f32 / height as f32;
    let view_projection = sprite_camera().view_projection(aspect);
    for world in [[0.0, 0.0], [-128.0, 96.0], [64.0, -32.0], [-100.0, 16.0]] {
        let clip = view_projection * glam::Vec4::new(world[0], world[1], 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        // The seam's convention is +Y up in NDC and `crcbl-vk` submits a
        // negative-height viewport, so the framebuffer row is the flipped half.
        let pixel = [
            0.5f32.mul_add(ndc.x, 0.5) * width as f32,
            0.5f32.mul_add(-ndc.y, 0.5) * height as f32,
        ];
        let expected = world_to_pixel(world);
        assert!(
            (pixel[0] - expected[0]).abs() < 1e-3 && (pixel[1] - expected[1]).abs() < 1e-3,
            "world {world:?} projects to pixel {pixel:?}, not the {expected:?} every \
             assertion below is written against"
        );
    }
}

/// sRGB encode, the transfer function an `Rgba8UnormSrgb` attachment applies on
/// the way out.
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055f32.mul_add(linear.powf(1.0 / 2.4), -0.055)
    }
}

/// sRGB decode, what the sampler and the blender apply on the way in.
pub(crate) fn srgb_decode(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The 8-bit value a linear channel is stored as.
pub(crate) fn srgb_byte(linear: f32) -> u8 {
    (srgb_encode(linear) * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The background byte triple the clear lands on, which is what every "this
/// pixel was not drawn on" assertion compares against.
pub(crate) fn background_rgb() -> [u8; 3] {
    [
        srgb_byte(SPRITE_CLEAR[0]),
        srgb_byte(SPRITE_CLEAR[1]),
        srgb_byte(SPRITE_CLEAR[2]),
    ]
}

/// **The asymmetric test sheet**: 4×2 texels, two 2×2 frames side by side, and
/// no two texels alike.
///
/// ```text
///   frame A          frame B
///   red    green  |  cyan   magenta
///   blue   yellow |  white  black
/// ```
///
/// Every symmetry a mistake could hide behind is broken: a flipped V swaps red
/// with blue, a flipped U swaps red with green, a transposed pair swaps green
/// with blue, and picking the wrong frame swaps the whole palette. A symmetric
/// sheet — a checkerboard, a single square, anything with a mirror line — passes
/// every one of those.
pub(crate) fn asymmetric_sheet() -> Vec<u8> {
    let row = |texels: [[u8; 4]; 4]| texels.into_iter().flatten().collect::<Vec<u8>>();
    let mut pixels = row([
        [255, 0, 0, 255],   // A top-left: red
        [0, 255, 0, 255],   // A top-right: green
        [0, 255, 255, 255], // B top-left: cyan
        [255, 0, 255, 255], // B top-right: magenta
    ]);
    pixels.extend(row([
        [0, 0, 255, 255],     // A bottom-left: blue
        [255, 255, 0, 255],   // A bottom-right: yellow
        [255, 255, 255, 255], // B bottom-left: white
        [0, 0, 0, 255],       // B bottom-right: black
    ]));
    pixels
}

/// Frame A of [`asymmetric_sheet`], as normalised UVs in image order.
pub(crate) const FRAME_A: [f32; 4] = [0.0, 0.0, 0.5, 1.0];
/// Frame B of [`asymmetric_sheet`].
pub(crate) const FRAME_B: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

/// The four corner colours of frame A, in `[top-left, top-right, bottom-left,
/// bottom-right]` order — the order the quadrant assertions read them in.
pub(crate) const FRAME_A_CORNERS: [[u8; 3]; 4] =
    [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
/// The same for frame B.
pub(crate) const FRAME_B_CORNERS: [[u8; 3]; 4] =
    [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];

/// A 2×2 sheet on its own, the same four colours as frame A.
///
/// The filtering tests use a sheet whose only frame *is* the whole image, so the
/// UV clamp at the frame's edge is the sheet's edge and there is no neighbouring
/// frame for a filter to bleed in from — which would be a second explanation for
/// any difference they measure.
pub(crate) fn quad_sheet() -> Vec<u8> {
    [
        [255u8, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A 1×1 opaque sheet of one colour.
///
/// One texel, so the whole quad is that colour whatever the filter does, and a
/// rectangle's colour is a direct read-out of **which sheet was bound** with no
/// UV arithmetic in between. That separation is the point — see
/// [`every_batch_draws_its_own_instances_rather_than_the_first_batchs`].
pub(crate) fn solid_sheet(rgb: [u8; 3]) -> Vec<u8> {
    vec![rgb[0], rgb[1], rgb[2], 255]
}

/// The three [`solid_sheet`] colours the multi-sheet test registers, with the
/// names its failure message uses.
pub(crate) const SOLID_SHEETS: [(&str, [u8; 3]); 3] = [
    ("red", [255, 0, 0]),
    ("green", [0, 255, 0]),
    ("blue", [0, 0, 255]),
];

/// A 2×2 sheet with a different alpha in each texel.
///
/// ```text
///   opaque red     half-alpha red
///   opaque green   fully transparent
/// ```
///
/// One sprite then covers all three cases at once: replace, blend, and leave
/// alone.
pub(crate) fn alpha_sheet() -> Vec<u8> {
    [
        [255u8, 0, 0, 255],
        [255, 0, 0, 128],
        [0, 255, 0, 255],
        [0, 0, 0, 0],
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The alpha the half-transparent texel is sampled with.
///
/// **Not sRGB-decoded.** Vulkan's sRGB formats apply the transfer function to
/// the colour channels only; alpha is linear, and a test that decoded it would
/// be asserting against the wrong number by 30%.
pub(crate) const HALF_ALPHA: f32 = 128.0 / 255.0;

impl Headless {
    /// Opens a ring at a pinned `Rgba8UnormSrgb` for the sprite suite.
    ///
    /// Pinned rather than preferred for the same reason
    /// [`Headless::open_for_mesh`] pins it: a golden image compared across two
    /// machines must not depend on which format each one's surface offered
    /// first. sRGB specifically, because `SpriteRenderer::register_sheet`
    /// uploads sheets as `Rgba8UnormSrgb` and the alpha blend is only in linear
    /// light if the target decodes too.
    pub(crate) fn open_for_sprites() -> Self {
        Self::open_for_sprites_at(SPRITE_EXTENT)
    }

    /// The same ring, at an extent the caller chooses.
    ///
    /// Added for the menu suite, which renders the *same* menu into two
    /// differently-shaped framebuffers to show that it stays centred in both —
    /// which needs two extents and cannot be got from one pinned ring.
    pub(crate) fn open_for_sprites_at(extent: (u32, u32)) -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e sprites"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e sprite ring"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");
        Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
        }
    }
}

/// Renders one sprite frame **through the real `SpriteRenderer` and the real
/// `RenderGraph`** and reads the swapchain image back.
///
/// A clear pass first and the sprite pass on top of it, because
/// `SpriteRenderer::add_pass` loads rather than clears — compositing onto
/// whatever is already there is the thing it is for, and a suite that cleared
/// inside the sprite pass would not be testing that.
pub(crate) fn render_sprites(
    headless: &Headless,
    renderer: &mut crcbl_render::SpriteRenderer,
    pool: &mut crcbl_render::TransientPool,
    sprites: &[crcbl_render::Sprite],
) -> crcbl_golden::Image {
    use crcbl_render::RenderGraph;

    let device = headless.device.as_ref();
    let (width, height) = SPRITE_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, SPRITE_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("sprite readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let aspect = width as f32 / height as f32;
    renderer
        .begin_frame(
            device,
            sprites,
            sprite_camera().view_projection(aspect),
            SPRITE_EXTENT,
        )
        .expect("the instance and constant buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("sprite frame"),
        queue: headless.queue,
    });

    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: SPRITE_EXTENT,
                initial: ResourceState::Undefined,
                // Read back rather than shown, so the graph is asked to leave it
                // as a copy source and the copy below needs no hand-written
                // barrier — the same trick the mesh path uses.
                final_state: ResourceState::TransferSrc,
            },
        );
        graph
            .add_render_pass("sprite background")
            .clear_color(target, SPRITE_CLEAR)
            .execute(|_| {});
        renderer.add_pass(&mut graph, target);
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
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

    let mut color = vec![0u8; color_bytes as usize];
    headless.readback(staging, color_bytes, &mut color);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}

/// Registers one sheet and returns its id.
pub(crate) fn register_sheet(
    renderer: &mut crcbl_render::SpriteRenderer,
    device: &dyn Device,
    label: &str,
    width: u32,
    height: u32,
    sample: crcbl_render::SampleMode,
    pixels: &[u8],
) -> crcbl_render::SheetId {
    renderer
        .register_sheet(
            device,
            &crcbl_render::SheetDesc {
                label,
                width,
                height,
                sample,
                pixels,
            },
        )
        .expect("the sheet uploads")
}

/// The RGB of a pixel, dropping alpha — every frame here is opaque, and carrying
/// the fourth channel through every assertion only makes them longer.
pub(crate) fn rgb(image: &crcbl_golden::Image, x: u32, y: u32) -> [u8; 3] {
    let pixel = image.pixel(x, y).unwrap_or_else(|| {
        panic!(
            "({x}, {y}) is outside a {}x{} frame",
            image.width(),
            image.height()
        )
    });
    [pixel[0], pixel[1], pixel[2]]
}

/// Whether two RGB triples agree to within `slack` on every channel.
pub(crate) fn close(actual: [u8; 3], expected: [u8; 3], slack: i32) -> bool {
    actual
        .iter()
        .zip(&expected)
        .all(|(a, e)| (i32::from(*a) - i32::from(*e)).abs() <= slack)
}

/// Asserts a pixel is the background — nothing was drawn here.
pub(crate) fn assert_background(image: &crcbl_golden::Image, x: u32, y: u32) {
    let actual = rgb(image, x, y);
    assert!(
        close(actual, background_rgb(), 2),
        "({x}, {y}) should still be the clear colour {:?}, got {actual:?}",
        background_rgb()
    );
}

/// Compares an image against a checked-in reference and **returns** the verdict
/// rather than asserting it.
///
/// Deferred on purpose: a test that panicked here would leave the renderer, the
/// pool and the device undestroyed, and the resulting `Drop` warning and dirty
/// validation report would be printed on top of the message that says what
/// actually went wrong. Every caller tears down first and unwraps last.
pub(crate) fn sprite_golden(name: &str, image: &crcbl_golden::Image) -> Result<String, String> {
    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"));
    crcbl_golden::Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
        .map(|comparison| format!("golden {name} — {}", comparison.summary()))
}

/// Reports every deferred golden verdict, failing on the first that did not
/// match.
pub(crate) fn report_goldens(verdicts: Vec<Result<String, String>>) {
    let mut failures = Vec::new();
    for verdict in verdicts {
        match verdict {
            Ok(summary) => eprintln!("vk e2e: {summary}"),
            Err(message) => failures.push(message),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
