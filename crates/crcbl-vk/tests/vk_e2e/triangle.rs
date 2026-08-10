use crate::harness::{Headless, instance};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, Device, DeviceDesc, Extent3d, Features, Format,
    ImageAspect, ImageSubresourceLayers, ImageSubresourceRange, Instance, LoadOp, MemoryLocation,
    PresentInfo, PresentMode, ReadbackDesc, ReadbackState, Rect2d, RenderPassDesc, ResourceState,
    StoreOp, SubmitInfo, SwapchainDesc,
};

// --- milestone 2: the triangle ---------------------------------------------

/// The size the triangle suite renders at.
///
/// Larger than [`EXTENT`] on purpose: the golden image's structural metric
/// works on 8×8 blocks, and a 64×48 frame is only 48 of them — few enough that
/// one block of edge disagreement moves the mean SSIM more than it should. It
/// is still small enough that lavapipe renders it in milliseconds.
pub(crate) const TRIANGLE_EXTENT: (u32, u32) = (256, 192);

/// The clear behind the triangle. Dark, and none of the vertex primaries, so
/// "the triangle did not draw" and "the triangle drew" are not confusable.
pub(crate) const TRIANGLE_CLEAR: [f32; 4] = [0.02, 0.03, 0.06, 1.0];

impl Headless {
    /// Opens a ring at a pinned format, so a golden image means the same thing
    /// on every driver.
    ///
    /// `preferred_format()` is what the sandbox uses and is right there — but a
    /// golden image compared across two drivers must not depend on which format
    /// each of them happened to prefer, or a format change would look like a
    /// rendering regression.
    pub(crate) fn open_for_triangle() -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e triangle"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::GPU_DRIVEN | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e triangle ring"),
                surface,
                format,
                extent: TRIANGLE_EXTENT,
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

/// Everything milestone 2 needs, built through the seam.
struct TriangleResources {
    vertices: crcbl_hal::BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
}

impl TriangleResources {
    /// Builds the pipeline and stages the geometry in.
    ///
    /// Deliberately the *same* shape as `apps/sandbox`'s `Triangle`, and the
    /// same geometry constant, so this suite is evidence about the code the
    /// sandbox runs rather than about a second triangle that resembles it.
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let bytes = crcbl_shaders::triangle::vertex_bytes();
        let size = bytes.len() as u64;

        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("triangle staging"),
                size,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(staging, 0, &bytes).expect("write");
        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("triangle vertices"),
                size,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a vertex buffer");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("triangle upload"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: staging,
            src_offset: 0,
            dst: vertices,
            dst_offset: 0,
            size,
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[crcbl_hal::BufferBarrier {
                buffer: vertices,
                from: ResourceState::TransferDst,
                to: ResourceState::ShaderRead,
                queue_transfer: None,
            }],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);

        let layout_entries = [crcbl_hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl_hal::ShaderStages::VERTEX,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        }];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("triangle vertices"),
                entries: &layout_entries,
            })
            .expect("a layout with no descriptor-indexing flags works on both tiers");

        let group_entries = [crcbl_hal::BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: crcbl_hal::BindingResource::whole_buffer(vertices),
        }];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("triangle vertices"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("triangle"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("triangle.slang"),
                spirv: crcbl_shaders::TRIANGLE.spirv(),
                wgsl: crcbl_shaders::TRIANGLE.wgsl(),
                msl: crcbl_shaders::TRIANGLE.msl(),
                dxil: &[],
            })
            .expect("the committed SPIR-V is accepted");

        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let pipeline = device
            .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
                label: Some("triangle"),
                layout: pipeline_layout,
                vertex: crcbl_hal::ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(crcbl_hal::ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: crcbl_hal::PrimitiveState::default(),
                depth_stencil: None,
                multisample: crcbl_hal::MultisampleState::default(),
                color_targets: &color_targets,
            })
            .expect("a graphics pipeline");
        // The seam promises pipelines built from a module stay valid once it is
        // destroyed, and this is where that promise is actually exercised: every
        // draw below happens after the module is gone.
        device.destroy_shader_module(module);

        Self {
            vertices,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.vertices);
    }
}

/// Renders the triangle into the ring and reads the pixels back.
///
/// The whole `crcbl screenshot` path in one function: acquire, barrier, render
/// pass, draw, barrier to `TransferSrc`, copy to a host-readable buffer, poll.
fn render_triangle(headless: &Headless, resources: &TriangleResources) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = TRIANGLE_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, TRIANGLE_EXTENT);

    let byte_count = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("triangle readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let range = ImageSubresourceRange::all(headless.format);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("triangle frame"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            range,
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("clear + triangle"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(TRIANGLE_CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(width, height),
    });
    encoder.set_viewport(&crcbl_hal::Viewport::from_size(width, height));
    encoder.set_scissor(&Rect2d::from_size(width, height));
    encoder.bind_graphics_pipeline(resources.pipeline);
    encoder.bind_group(0, resources.bind_group, &[], resources.pipeline_layout);
    // Three vertices, and no geometry bound to the pipeline at all.
    encoder.draw(0..3, 0..1);
    encoder.end_render_pass();
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            range,
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
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

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("triangle pixels"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = vec![0u8; byte_count as usize];
    // Poll with a deadline, never a fixed sleep — `docs/plan/12-testing.md`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match device
            .poll_readback(readback, &mut bytes)
            .expect("the readback did not fail")
        {
            ReadbackState::Ready => break,
            ReadbackState::Pending => assert!(
                std::time::Instant::now() < deadline,
                "the readback never completed"
            ),
        }
        std::thread::yield_now();
    }
    device.destroy_readback(readback);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &bytes, order)
        .expect("the readback is exactly one image")
}

/// Milestone 2, end to end and verified: a triangle whose vertices came out of a
/// storage buffer, with **no vertex input state anywhere**.
///
/// The assertions are about geometry rather than exact colour, because that is
/// what distinguishes "the triangle drew" from "something drew": each corner of
/// the frame holds a different vertex's colour, and the centre holds a blend of
/// all three. A pipeline that ignored the storage buffer, a vertex order that
/// was mirrored, or a Y flip would each break at least one of them.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_triangle_pulled_from_a_vulkan_storage_buffer_reaches_memory() {
    let headless = Headless::open_for_triangle();
    let resources = TriangleResources::new(&headless);
    let image = render_triangle(&headless, &resources);

    let (width, height) = TRIANGLE_EXTENT;

    // Sample points are derived from the geometry rather than guessed at as
    // fractions of the frame: 75% of the way from the centroid to each vertex is
    // comfortably inside the triangle whatever its shape, and near enough to a
    // corner that one vertex colour dominates. Picking round fractions of the
    // frame instead put the "apex" sample *above* the apex, which is a test bug
    // that looks exactly like a Y flip.
    let vertices = crcbl_shaders::triangle::VERTICES;
    let centroid = [
        vertices.iter().map(|v| v.position[0]).sum::<f32>() / 3.0,
        vertices.iter().map(|v| v.position[1]).sum::<f32>() / 3.0,
    ];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sample_near = |index: usize| -> (u32, u32) {
        let vertex = vertices[index].position;
        let x = vertex[0].mul_add(0.75, centroid[0] * 0.25);
        let y = vertex[1].mul_add(0.75, centroid[1] * 0.25);
        // NDC to pixels. +Y is up in the seam's convention, which the backend's
        // negative-height viewport preserves, so the Y term is inverted here and
        // nowhere else.
        (
            (((x + 1.0) * 0.5) * width as f32) as u32,
            (((1.0 - y) * 0.5) * height as f32) as u32,
        )
    };

    // Dominance rather than absolute values: the target is sRGB, so the exact
    // level depends on the transfer function, but "this corner is redder than it
    // is green or blue" is true under any encoding and is what actually
    // distinguishes a correct triangle from a mirrored or rotated one.
    let names = ["apex (red)", "bottom right (green)", "bottom left (blue)"];
    for (index, name) in names.iter().enumerate() {
        let (x, y) = sample_near(index);
        let pixel = image
            .pixel(x, y)
            .unwrap_or_else(|| panic!("{name} sample ({x}, {y}) is outside the frame"));
        let dominant = (0..3)
            .max_by_key(|channel| pixel[*channel])
            .expect("three channels");
        assert_eq!(
            dominant, index,
            "{name} at ({x}, {y}) is {pixel:?}; channel {dominant} dominates rather than \
             channel {index}. A Y flip, an X mirror or a reversed vertex order each produce \
             exactly this."
        );
        assert!(
            u32::from(pixel[index]) > 150,
            "{name} at ({x}, {y}) is {pixel:?}; the dominant channel must be strong, not \
             merely largest"
        );
    }

    // The centre is a blend of all three, which is the property that proves the
    // fragment stage really interpolated a per-vertex attribute rather than
    // outputting a constant.
    let centre = image.pixel(width / 2, height / 2).expect("inside");
    assert!(
        centre[0] > 20 && centre[1] > 20 && centre[2] > 20,
        "the centre must blend all three vertex colours, got {centre:?}"
    );
    assert_eq!(centre[3], 255, "alpha 1.0 must survive");

    // And the very corners of the frame are still the clear colour: the
    // triangle does not cover them, so a pipeline that drew a full-screen quad
    // would fail here.
    for corner in [
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ] {
        let pixel = image.pixel(corner.0, corner.1).expect("inside");
        assert!(
            pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 80,
            "corner {corner:?} must still be the clear colour, got {pixel:?}"
        );
    }

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// The golden-image gate: the rendered triangle against a checked-in reference.
///
/// `docs/plan/12-testing.md` schedules this for P1 and specifies the shape —
/// "per-pixel tolerance + SSIM-style metric (rasterizers differ slightly);
/// regenerate via `--bless` flag; diffs uploaded as CI artifacts on failure".
///
/// The tolerance is [`Tolerance::RASTERISER`], whose numbers were measured
/// between radv and lavapipe rather than guessed; `crcbl-golden`'s crate docs
/// carry the measurements and its unit tests pin that the same tolerance still
/// rejects a triangle that moved by a few pixels.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_vulkan_triangle_matches_its_golden_image() {
    let headless = Headless::open_for_triangle();
    let resources = TriangleResources::new(&headless);
    let image = render_triangle(&headless, &resources);

    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/triangle.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden.check(&image).expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            // Destroyed before the panic so the device teardown does not report
            // leaked objects on top of the real failure.
            resources.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    // Printed on success too: the numbers are how the tolerance stays honest
    // across drivers, and a run that quietly passes teaches nothing.
    eprintln!("vk e2e: golden triangle — {}", comparison.summary());

    resources.destroy(headless.device.as_ref());
    headless.finish();
}
