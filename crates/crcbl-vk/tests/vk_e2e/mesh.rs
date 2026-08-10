//! Milestones 3, 4 and 5: the lit mesh, through the render graph.
//!
//! Depth testing, the directional light, the `Rgba16Float` scene target and its
//! tonemap, the orthographic camera, a second mesh at a non-zero base vertex,
//! and the graph's transient pool under a resize storm — against three
//! checked-in references, `tests/golden/mesh.png`, `mesh_ortho.png` and
//! `mesh_second.png`.
//!
//! It is also the suite's shared scene, which is why it is one module rather
//! than several: `queries`, `draw_gen` and `depth_probe` all import its extent,
//! camera or spin constant instead of inventing their own, so the scene the
//! goldens were blessed on is the scene those modules measure against.
//!
//! The tests worth reading are the ones a golden cannot make. A shader that
//! returned the vertex colour unchanged would pass a golden the day it was
//! blessed, so the light is checked as a ratio against the Lambert term in the
//! HDR target; and getting a transient's lifetime wrong under resize is a
//! validation error rather than a wrong picture, so that test's assertion is
//! `Headless::finish`'s report.

use crate::harness::{Headless, instance};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, CompositeAlpha, DeviceDesc,
    Extent3d, Features, Format, ImageAspect, ImageSubresourceLayers, Instance, MemoryLocation,
    PresentInfo, PresentMode, ResourceState, SubmitInfo, SwapchainDesc,
};

/// The size the mesh suite renders at.
///
/// The same 256×192 as the triangle, and for the same reason: the golden's
/// structural metric works on 8×8 blocks, and a smaller frame gives it too few
/// to average over.
pub(crate) const MESH_EXTENT: (u32, u32) = (256, 192);

/// Where the camera is for every mesh golden.
///
/// Far enough back that the cube does not touch the frame edge under either
/// projection, and off-axis on two of three axes so **three faces are visible at
/// once** — which is what makes a directional light legible and an orientation
/// mistake a different picture rather than a plausible one.
pub(crate) fn mesh_camera(projection: crcbl_render::Projection) -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(1.6, 1.2, 2.2),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection,
    }
}

/// The animation time every mesh golden renders at.
///
/// A constant, not a clock: a golden image of a spinning cube is only evidence
/// if the cube is in the same place every run.
///
/// Chosen so **three faces are visible at once**, which is not automatic — at
/// `0.7` the cube's `+X` face is edge-on to this camera to within a fifth of a
/// degree, and a two-face frame cannot show a lighting gradient however correct
/// the shader is. Three faces also means no symmetry is left for a transposed
/// matrix or a mirrored axis to hide behind.
pub(crate) const MESH_SECONDS: f32 = 0.35;

impl Headless {
    /// Opens a ring at a pinned format for the mesh suite. See
    /// [`Headless::open_for_triangle`] on why the format is pinned rather than
    /// preferred.
    pub(crate) fn open_for_mesh() -> Self {
        Self::open_for_mesh_with(Features::GPU_DRIVEN)
    }

    /// The same ring, with a chosen optional feature set.
    ///
    /// **The set decides the forward pass's emit tail.** A device opened
    /// without [`Features::DRAW_INDIRECT_COUNT`] selects
    /// [`GeometryPath::IndirectPerBatch`](crcbl_hal::GeometryPath::IndirectPerBatch),
    /// which is the path Metal is on and which no adapter this suite can see
    /// would otherwise select — so asking for less is the only way to run that
    /// arm on real hardware at all.
    pub(crate) fn open_for_mesh_with(optional: Features) -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e mesh"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: optional | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e mesh ring"),
                surface,
                format,
                extent: MESH_EXTENT,
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

/// What one mesh frame produced.
pub(crate) struct MeshFrame {
    /// The tonemapped swapchain image.
    pub(crate) image: crcbl_golden::Image,
    /// The raw `Rgba16Float` scene target, as half-floats.
    hdr: Vec<u8>,
}

impl MeshFrame {
    /// The linear HDR value at a texel, decoded from `Rgba16Float`.
    fn hdr_pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let index = ((y * MESH_EXTENT.0 + x) * 4) as usize * 2;
        let mut out = [0.0f32; 4];
        for (channel, value) in out.iter_mut().enumerate() {
            let bits = u16::from_le_bytes(
                self.hdr[index + channel * 2..index + channel * 2 + 2]
                    .try_into()
                    .expect("two bytes"),
            );
            *value = half_to_f32(bits);
        }
        out
    }

    /// The brightest linear channel anywhere in the HDR target.
    fn peak_hdr(&self) -> f32 {
        let mut peak = 0.0f32;
        for y in 0..MESH_EXTENT.1 {
            for x in 0..MESH_EXTENT.0 {
                // Alpha is a constant 1.0 and would mask the interesting number.
                for channel in self.hdr_pixel(x, y).iter().take(3) {
                    peak = peak.max(*channel);
                }
            }
        }
        peak
    }
}

/// Decodes an IEEE binary16 into an `f32`.
///
/// Written out rather than pulled in: this is the only place in the engine that
/// reads a `Rgba16Float` on the CPU, and a dependency for twelve lines of shifts
/// would be a `cargo deny` conversation about a test helper.
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let value = match exponent {
        // Zero or subnormal.
        0 => {
            if mantissa == 0 {
                0
            } else {
                // Renormalise: shift the mantissa up until its leading bit
                // falls off, decrementing the exponent as it goes.
                let leading = mantissa.leading_zeros() - 21;
                let mantissa = (mantissa << (leading + 1)) & 0x3ff;
                ((127 - 15 - leading) << 23) | (mantissa << 13)
            }
        }
        // Infinity or NaN.
        31 => 0xff << 23 | (mantissa << 13),
        _ => ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(sign | value)
}

/// Renders one frame of the forward pipeline **through the real render graph**
/// and reads back both the swapchain image and the HDR scene target.
///
/// Deliberately `crcbl_render::ForwardRenderer` and `crcbl_render::RenderGraph`
/// rather than a hand-built copy: a golden image is only evidence about the code
/// the sandbox runs if it *is* the code the sandbox runs.
pub(crate) fn render_mesh(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    camera: &crcbl_render::Camera,
) -> MeshFrame {
    use crcbl_render::{ForwardRenderer, RenderGraph};

    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    // `Rgba16Float`: four channels of two bytes.
    let hdr_bytes = u64::from(width) * u64::from(height) * 8;
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
    let color_staging = staging("mesh readback", color_bytes);
    let hdr_staging = staging("mesh hdr readback", hdr_bytes);

    renderer
        .begin_frame(
            device,
            camera,
            &crcbl_render::DirectionalLight::default(),
            ForwardRenderer::spin(MESH_SECONDS),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    // Where the graph's realised HDR handle lands, so the copy below can name
    // it. `Cell` rather than a channel: the pass body runs synchronously inside
    // `execute`, on this thread.
    let hdr_handle: std::cell::Cell<Option<crcbl_hal::ImageHandle>> = std::cell::Cell::new(None);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
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
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it as a copy source and the
                // copy below needs no barrier of its own. Saying so in the
                // import is the whole point — there is still not one
                // hand-written barrier anywhere in this file's mesh path.
                final_state: ResourceState::TransferSrc,
            },
        );
        let scene = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        // One extra declaration, and the graph works out that the HDR target
        // has to move from `ShaderRead` (the tonemap sampled it) to
        // `TransferSrc` (this wants to copy it).
        let sink = &hdr_handle;
        graph
            .add_compute_pass("hdr probe")
            .use_image(scene, ResourceState::TransferSrc)
            .execute(move |ctx| sink.set(Some(ctx.image(scene))));
        // `&*pool`: the same pool the frame is about to be realised
        // against, so the barriers open where the last frame left off.
        graph.compile(&*pool).expect("a legal frame")
    };
    eprintln!("vk e2e: {}", compiled.dump());
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    let scene_image = hdr_handle.get().expect("the probe pass ran");
    let layers = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Both copies are outside every pass and need no barrier: the graph left
    // both images in `TransferSrc` because both were declared that way.
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: color_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: layers,
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: hdr_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: scene_image,
        image_subresource: layers,
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
    headless.readback(color_staging, color_bytes, &mut color);
    let mut hdr = vec![0u8; hdr_bytes as usize];
    headless.readback(hdr_staging, hdr_bytes, &mut hdr);

    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);
    device.destroy_buffer(hdr_staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    MeshFrame {
        image: crcbl_golden::Image::from_readback(width, height, &color, order)
            .expect("the readback is exactly one image"),
        hdr,
    }
}

/// Milestones 3 and 4: a depth-tested, lit, spinning cube drawn through the
/// render graph, against a checked-in reference.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_lit_mesh_through_the_graph_matches_its_golden_image() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    // Something drew, and it is not the whole frame: the clear must still be
    // visible in the corners, or the "cube" is a full-screen quad.
    let corner = frame.image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = frame.image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mesh.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden
        .check(&frame.image)
        .expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!("vk e2e: golden mesh — {}", comparison.summary());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Milestone 5: the orthographic camera is a **projection-matrix swap and
/// nothing else**.
///
/// The assertion is in two halves, and both matter. The golden proves the
/// orthographic frame is the one that was reviewed; comparing it against the
/// perspective frame proves the swap actually did something, so a
/// `Projection::Orthographic` that silently fell through to perspective could
/// not pass.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_orthographic_camera_is_a_projection_swap_and_matches_its_golden() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");

    let ortho = crcbl_render::Projection::Orthographic {
        half_height: 0.9,
        near: 0.1,
        far: 100.0,
    };
    // The *same* renderer, the same pipeline, the same geometry, the same
    // shader, the same graph. One field differs.
    let perspective_frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );
    let frame = render_mesh(&headless, &mut renderer, &mut pool, &mesh_camera(ortho));

    let differing = (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0).map(move |x| (x, y)))
        .filter(|(x, y)| frame.image.pixel(*x, *y) != perspective_frame.image.pixel(*x, *y))
        .count();
    assert!(
        differing > (MESH_EXTENT.0 * MESH_EXTENT.1) as usize / 100,
        "swapping the projection must change the picture; only {differing} pixels moved"
    );

    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mesh_ortho.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden
        .check(&frame.image)
        .expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!("vk e2e: golden ortho mesh — {}", comparison.summary());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Where the second-mesh golden puts the pyramid, in world units.
///
/// Left of the cube from this camera and clear of it: the two meshes have to be
/// separable by eye, because the failure this golden exists to catch is one mesh
/// drawing the *other's* vertices. Measured against [`two_mesh_camera`] rather
/// than guessed — at [`mesh_camera`]'s distance the cube hides the pyramid
/// entirely, which was the first thing this golden showed.
const PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-2.0, 0.0, 0.0);

/// [`mesh_camera`] pulled back far enough for two meshes to fit side by side.
///
/// The same eye direction, the same target, the same projection — only the
/// distance differs, because a frame that has to hold the cube *and* a mesh
/// beside it needs about twice the width the cube alone does. Keeping the
/// direction is what makes this frame comparable to the cube golden's: three
/// cube faces are visible in both.
fn two_mesh_camera() -> crcbl_render::Camera {
    let mut camera = mesh_camera(crcbl_render::Projection::default());
    camera.eye *= 1.7;
    camera
}

/// `docs/plan/03-gpu-driven-rendering.md` §3.1's pool, drawn with **two**
/// residents in it — which is the first frame in which a base vertex means
/// anything at all.
///
/// The pyramid is the pool's second mesh, so it is at a non-zero
/// `MeshRange::base_vertex`. Rendered against the cube's own golden camera, and
/// the assertions below are what make it evidence rather than a picture:
///
/// * The cube must still be where the cube golden has it, so this cannot pass by
///   drawing anything anywhere.
/// * The pyramid's pixels must carry a colour **no cube face has**. That is the
///   half that fails when the base vertex is lost: the draw then reads the
///   cube's first sixteen vertices, and the frame comes out with a piece of a
///   second cube in the pyramid's place — which is exactly what it did before
///   `mesh.slang` grew its `DrawConstants` block, on Vulkan and not on WebGPU.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_mesh_at_a_non_zero_base_vertex_draws_its_own_geometry() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    renderer.set_pyramid(Some(glam::Mat4::from_translation(PYRAMID_AT)));

    let frame = render_mesh(&headless, &mut renderer, &mut pool, &two_mesh_camera());

    // The cube is still there, in the middle where the camera points.
    let centre = frame.image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must still be the cube, got {centre:?}"
    );

    // And somewhere in the left third there is a hue the cube does not own. The
    // check is on the *ratios* between channels rather than on an exact colour,
    // because the value that reaches the swapchain has been through Lambert,
    // Blinn and a tonemap; what survives all three is which channel leads.
    // `PYRAMID_SIDE_COLORS`' visible sides are the `+Z` violet and the `+X`
    // teal, and no cube face is either.
    let violet = (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0 / 3).map(move |x| (x, y)))
        .filter_map(|(x, y)| frame.image.pixel(x, y))
        .filter(|pixel| {
            let (r, g, b) = (
                u32::from(pixel[0]),
                u32::from(pixel[1]),
                u32::from(pixel[2]),
            );
            b > 100 && r > g + 30 && b > g + 30
        })
        .count();
    assert!(
        violet > 200,
        "the pyramid's violet side is missing: only {violet} texel(s) of it. A frame \
         that lost the base vertex draws the cube's own vertices here instead, and no \
         cube face is violet"
    );

    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mesh_second.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden
        .check(&frame.image)
        .expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!("vk e2e: golden second mesh — {}", comparison.summary());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Milestone 4, measured rather than eyeballed: the directional light produces a
/// real gradient across the cube's faces.
///
/// A shader that returned the vertex colour unchanged would draw a perfectly
/// good cube and pass a golden image the day it was blessed. What it could not
/// do is make two faces of *different* colours differ in brightness by the same
/// factor the Lambert term predicts — which is what this checks, using the HDR
/// target so the sRGB transfer function is not in the way.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_directional_light_actually_shades_the_mesh() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    // Collect the linear luminance of every pixel the cube covers, in the HDR
    // target — where the values are the shader's own output rather than an sRGB
    // encoding of it.
    let mut lit: Vec<f32> = Vec::new();
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [r, g, b, _] = frame.hdr_pixel(x, y);
            let luminance = 0.2126f32.mul_add(r, 0.7152f32.mul_add(g, 0.0722 * b));
            // Anything above the clear colour's luminance is geometry.
            if luminance > 0.05 {
                lit.push(luminance);
            }
        }
    }
    assert!(
        lit.len() > 1000,
        "the cube must cover a meaningful part of the frame; got {} pixels",
        lit.len()
    );
    lit.sort_by(f32::total_cmp);
    let dimmest = lit[lit.len() / 20];
    let brightest = lit[lit.len() - lit.len() / 20 - 1];
    assert!(
        brightest > dimmest * 1.5,
        "a directional light must produce a gradient across the faces: the 95th \
         percentile is {brightest} and the 5th is {dimmest}, a ratio of {}",
        brightest / dimmest
    );
    // And nothing is pure black: the ambient term exists so an unlit face is
    // dark rather than invisible.
    assert!(dimmest > 0.0, "the ambient term must lift the unlit faces");

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **HDR from P1 is real**, not a format enum.
///
/// `docs/plan/ROADMAP.md`'s correction asks for an `Rgba16Float` scene target
/// and a trivial tonemap from the first lit mesh. This reads the scene target
/// back and asserts it carries a value above 1.0 — which an `Rgba8` attachment
/// could not have held — and that the tonemapped swapchain pixel underneath it
/// is at the top of its range, which is the tonemap doing the one thing it does.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_hdr_target_carries_values_an_eight_bit_target_could_not() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    let peak = frame.peak_hdr();
    eprintln!("vk e2e: peak linear value in the HDR target — {peak}");
    assert!(
        peak > 1.0,
        "the Blinn highlight must exceed 1.0 somewhere, or the RGBA16F target is \
         carrying nothing an Rgba8 one could not; peak was {peak}"
    );
    assert!(
        peak.is_finite() && peak < 100.0,
        "a peak of {peak} is a NaN or a runaway, not a specular highlight"
    );

    // Find the brightest texel and check the tonemap clamped it rather than
    // letting it wrap or go black.
    let mut hottest = (0u32, 0u32, 0.0f32);
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let value = frame
                .hdr_pixel(x, y)
                .iter()
                .take(3)
                .fold(0.0f32, |peak, channel| peak.max(*channel));
            if value > hottest.2 {
                hottest = (x, y, value);
            }
        }
    }
    let pixel = frame
        .image
        .pixel(hottest.0, hottest.1)
        .expect("inside the frame");
    let brightest_channel = pixel[..3].iter().copied().max().expect("three channels");
    assert_eq!(
        brightest_channel, 255,
        "the tonemap must clamp a linear {} to the top of the swapchain's range, \
         got {pixel:?} at ({}, {})",
        hottest.2, hottest.0, hottest.1
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// A resize storm, driven through the **render graph** rather than around it.
///
/// This is the path with the most new moving parts at P1.3 and the least
/// obvious failure mode. Every size change invalidates both scene transients,
/// so the pool must hand out new ones and retire the old; and the tonemap's bind
/// group names a *graph-owned* view, so it must be rebuilt when that view
/// changes and destroyed when it does — while a previous frame may still be
/// reading it.
///
/// Getting any of that wrong is a validation error rather than a wrong picture,
/// which is why the assertion is the report: `Headless::finish` fails on any
/// error or warning, and on the layer never having loaded.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_graph_and_its_pool_survive_a_resize_storm() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    let camera = mesh_camera(crcbl_render::Projection::default());

    // Sizes chosen to be genuinely different rather than a nudge — including one
    // that is not a multiple of anything, because a row-pitch assumption hides
    // behind round numbers.
    let sizes = [
        MESH_EXTENT,
        (64, 48),
        (300, 130),
        (17, 5),
        (256, 192),
        (64, 48),
        MESH_EXTENT,
    ];
    for extent in sizes {
        device
            .reconfigure_swapchain(
                headless.swapchain,
                &SwapchainDesc {
                    label: Some("vk e2e mesh ring"),
                    surface: headless.surface,
                    format: headless.format,
                    extent,
                    image_count: 2,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("reconfigure keeps the handle valid");

        // Two frames per size, so the second one exercises the *reuse* path
        // rather than only the create path.
        for _ in 0..2 {
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("an image");
            assert_eq!(acquired.extent, extent);
            renderer
                .begin_frame(
                    device,
                    &camera,
                    &crcbl_render::DirectionalLight::default(),
                    crcbl_render::ForwardRenderer::spin(MESH_SECONDS),
                    extent,
                )
                .expect("uniforms");

            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("resize frame"),
                queue: headless.queue,
            });
            let compiled = {
                let mut graph = crcbl_render::RenderGraph::new(headless.queue);
                let target = graph.import_image(
                    "swapchain",
                    crcbl_render::ForwardRenderer::present_target(
                        acquired.image,
                        acquired.view,
                        headless.format,
                        extent,
                    ),
                );
                let _ = renderer.add_passes(&mut graph, target, extent);
                graph.compile(&pool).expect("a legal frame")
            };
            // Every *render* pass renders at the size that was just
            // configured, which is the graph deriving its render area from the
            // attachments rather than from anything remembered. A compute pass
            // has no attachments and so no area — asserting one on the cull and
            // draw-argument dispatches would be asserting on a zero the graph
            // fills in for them.
            let mut rendered = 0;
            for pass in compiled.passes() {
                if pass.kind() != crcbl_render::PassKind::Render {
                    continue;
                }
                rendered += 1;
                assert_eq!(
                    (pass.render_area().width, pass.render_area().height),
                    extent,
                    "pass {:?} rendered at the wrong size",
                    pass.label()
                );
            }
            assert!(
                rendered >= 2,
                "the forward and tonemap passes, or this loop checked nothing"
            );
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("executed");
            let commands = encoder.finish().expect("recorded");
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
            // Nothing is pipelined here, so an idle stands in for the frame
            // loop's timeline wait before the pool retires anything.
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);
            pool.retire_unused(device);
        }

        // The pool must converge rather than accumulate one pair of targets per
        // size the window passed through. Two live transients, plus at most
        // `RETIRE_AFTER_FRAMES` generations of stale ones.
        let ceiling = 2 * (crcbl_render::transient::RETIRE_AFTER_FRAMES as usize + 1);
        assert!(
            pool.image_count() <= ceiling,
            "after resizing to {extent:?} the pool holds {} images, over the {ceiling} \
             a bounded retirement allows",
            pool.image_count()
        );
    }

    renderer.destroy(device);
    pool.destroy(device);
    // The report is the assertion: a bind group destroyed while in flight, a
    // transient freed too early, or a stale view sampled would all land here.
    headless.finish();
}
