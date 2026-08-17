//! The mesh scene the two `ForwardRenderer` suites draw — not a test module, and
//! not a test target: `tests/gpu_scene/` holds no `main.rs`, so Cargo compiles
//! nothing here on its own.
//!
//! **Three suites pull this in with `#[path]`** — `tests/draw_gen_e2e/`,
//! `tests/forward_e2e/` and `tests/mesh_e2e/` — because they place the same
//! meshes, render them through the same graph and read back the same frames. It
//! sits beside [`harness`](crate::harness) rather than inside it because the
//! fourth suite on that fixture, `tests/sprite_e2e/`, draws no mesh at all: a
//! fixture every GPU suite opens with and a scene three of them render are
//! separate concerns, and keeping them in one file made every symbol here dead
//! code in that binary.
//!
//! The culling-statistics readback two of them do is
//! `tests/gpu_scene/cull_readback.rs`, on the same terms: `tests/mesh_e2e/`
//! reads no counter, so a scene and a statistics copy in one file would leave
//! that symbol dead in its binary.
//!
//! Everything below is `vk_e2e/mesh.rs`'s, including the `Rgba16Float` probe
//! pass — [`render_mesh`]'s `hdr` sink is what turns it into a second readback,
//! and `tests/mesh_e2e/hdr.rs` is the only caller that asks for one.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Features, Format,
    ImageAspect, ImageSubresourceLayers, MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};
use crcbl::math::Mat4;
use crcbl::render::{Camera, ForwardRenderer, InstanceDesc, InstanceHandle, TransientPool};

impl Headless {
    /// A ring of `extent`, with `optional_features` asked for and none required,
    /// and the format the surface prefers.
    ///
    /// Here rather than beside [`Headless::open_at_format`] because both callers
    /// are in the two suites that name this file: the third suite on that
    /// fixture pins a format and would leave this dead.
    ///
    /// `pub(crate)` because a suite whose probe is not a mesh scene builds its
    /// own ring out of it — `draw_gen_e2e/cull.rs` is the one that does.
    pub(crate) fn open_at(extent: (u32, u32), optional_features: Features) -> Self {
        Self::open_at_format(extent, None, optional_features)
    }

    /// A ring the mesh scenes render into, on the best geometry path the
    /// adapter offers.
    pub(crate) fn open_for_mesh() -> Self {
        Self::open_for_mesh_with(Features::GPU_DRIVEN)
    }

    /// The same ring, with a chosen optional feature set.
    ///
    /// **The set decides the forward pass's emit tail.** A device opened without
    /// [`Features::DRAW_INDIRECT_COUNT`] selects
    /// [`GeometryPath::IndirectPerBatch`](crcbl::hal::GeometryPath::IndirectPerBatch),
    /// which is the path Metal is on and which an adapter reporting everything
    /// would otherwise never select — so asking for less is the only way to run
    /// that arm on some machines at all.
    pub(crate) fn open_for_mesh_with(optional: Features) -> Self {
        Self::open_at(
            MESH_EXTENT,
            optional | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
        )
    }
}

/// The size the mesh scenes render at.
///
/// The same 256×192 `vk_e2e/mesh.rs` and `tests/render_e2e.rs` use, and for the
/// reason both state: a smaller frame gives a structural comparison too few 8×8
/// blocks to average over.
pub(crate) const MESH_EXTENT: (u32, u32) = (256, 192);

/// The animation time every mesh frame here renders at.
///
/// A constant, not a clock. Chosen so **three faces of the cube are visible at
/// once**, which is not automatic — at `0.7` its `+X` face is edge-on to this
/// camera to within a fifth of a degree — and three faces means no symmetry is
/// left for a transposed matrix or a mirrored axis to hide behind.
pub(crate) const MESH_SECONDS: f32 = 0.35;

/// Where the camera is for every mesh frame.
///
/// Far enough back that the cube does not touch the frame edge under either
/// projection, and off-axis on two of three axes so three faces are visible at
/// once.
pub(crate) fn mesh_camera(projection: crcbl::render::Projection) -> Camera {
    Camera {
        eye: crcbl::math::Vec3::new(1.6, 1.2, 2.2),
        target: crcbl::math::Vec3::ZERO,
        up: crcbl::math::Vec3::Y,
        projection,
    }
}

/// Puts one of the demo scene's meshes in the frame at `transform`, and answers
/// with the handle that names it.
///
/// **Insertion order is the caller's and it is load-bearing** — the slot an
/// object lands in is `docs/plan/25-lod.md`'s hysteresis key, and it is also the
/// instance index every CPU reference in this suite is written against — so
/// every scene here places the cube first and whatever stands beside it after.
pub(crate) fn place(
    renderer: &mut ForwardRenderer,
    mesh: usize,
    material: usize,
    transform: Mat4,
) -> InstanceHandle {
    renderer
        .add_instance(&InstanceDesc {
            mesh,
            material,
            transform,
        })
        .expect("an instance pool of thousands has room for a test scene")
}

/// Puts the demo scene's cube in the frame at `transform`, and **first**, so it
/// takes the pool slot every reference here expects it in.
pub(crate) fn place_cube_at(renderer: &mut ForwardRenderer, transform: Mat4) {
    place(
        renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        transform,
    );
}

/// The same, at the spin every frame in this suite is drawn at.
pub(crate) fn place_cube(renderer: &mut ForwardRenderer) {
    place_cube_at(renderer, ForwardRenderer::spin(MESH_SECONDS));
}

/// Renders one frame of the forward pipeline **through the real render graph**
/// and reads the swapchain image back.
///
/// Deliberately [`ForwardRenderer`] and [`crcbl::render::RenderGraph`] rather
/// than a hand-built copy: a frame is only evidence about the code an
/// application runs if it *is* the code an application runs.
///
/// `hdr` is the raw `Rgba16Float` scene target, copied back beside the
/// swapchain image when a caller supplies a sink for it. **A sink rather than a
/// second function**, because the copy has to be recorded into this frame's own
/// encoder before the transient is recycled — there is no later moment to ask
/// from. `tests/mesh_e2e/hdr.rs` is the only caller that passes one; the other
/// two suites read the tonemapped frame and pass `None`, which records no probe
/// pass and no second copy at all.
pub(crate) fn render_mesh(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &Camera,
    hdr: Option<&mut Vec<u8>>,
) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("mesh readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    // `Rgba16Float`: four channels of two bytes. The row is `4 * 2 * 256`
    // bytes wide, so it satisfies the 256-byte copy pitch wgpu and D3D12
    // enforce without this having to pad — see `MESH_EXTENT`.
    let hdr_bytes = u64::from(width) * u64::from(height) * 8;
    let hdr_staging = hdr.as_ref().map(|_| {
        device
            .create_buffer(&BufferDesc {
                label: Some("mesh hdr readback"),
                size: hdr_bytes,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    });

    renderer
        .begin_frame(
            device,
            camera,
            &crcbl::render::DirectionalLight::default(),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    // Where the graph's realised HDR handle lands, so the copy below can name
    // it. `Cell` rather than a channel: the pass body runs synchronously inside
    // `execute`, on this thread.
    let hdr_handle: std::cell::Cell<Option<crcbl::hal::ImageHandle>> = std::cell::Cell::new(None);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
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
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it as a copy source and the
                // copy below needs no barrier of its own. Saying so in the
                // import is the point — there is not one hand-written barrier
                // anywhere in this path.
                final_state: ResourceState::TransferSrc,
            },
        );
        let scene = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        if hdr_staging.is_some() {
            // One extra declaration, and the graph works out that the HDR
            // target has to move from `ShaderRead` (the tonemap sampled it) to
            // `TransferSrc` (this wants to copy it).
            let sink = &hdr_handle;
            graph
                .add_compute_pass("hdr probe")
                .use_image(scene, ResourceState::TransferSrc)
                .execute(move |ctx| sink.set(Some(ctx.image(scene))));
        }
        // `&*pool`: the same pool the frame is about to be realised against, so
        // the barriers open where the last frame left off.
        graph.compile(&*pool).expect("a legal frame")
    };
    eprintln!("{}: {}", crate::SUITE, compiled.dump());
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    let layers = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Both copies are outside every pass and need no barrier: the graph left
    // both images in `TransferSrc` because both were declared that way.
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: layers,
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    if let Some(hdr_staging) = hdr_staging {
        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: hdr_staging,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: hdr_handle.get().expect("the probe pass ran"),
            image_subresource: layers,
            image_offset: crcbl::hal::Offset3d::default(),
            image_extent: Extent3d::d2(width, height),
        });
    }

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

    let mut color = poisoned(color_bytes as usize);
    headless.readback(staging, color_bytes, &mut color);
    if let (Some(hdr_staging), Some(hdr)) = (hdr_staging, hdr) {
        *hdr = poisoned(hdr_bytes as usize);
        headless.readback(hdr_staging, hdr_bytes, hdr);
        device.destroy_buffer(hdr_staging);
    }
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}
