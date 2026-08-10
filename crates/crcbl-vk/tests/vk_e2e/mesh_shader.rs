use crate::harness::{Headless, instance};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, Device, DeviceDesc, Extent3d, Features, Format, HalError,
    ImageAspect, ImageSubresourceLayers, ImageSubresourceRange, Instance, LoadOp, MemoryLocation,
    MeshPipelineDesc, PresentInfo, PresentMode, Rect2d, RenderPassDesc, ResourceState, ShaderEntry,
    StoreOp, SubmitInfo, SwapchainDesc,
};

// --- the mesh-shading geometry path ----------------------------------------
//
// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes mesh shaders the primary
// geometry path. This is the smallest end-to-end proof that the path exists:
// one triangle emitted by a mesh stage, and the same triangle again through an
// amplification stage in front of it.
//
// Not meshlets and not cluster culling — those need a mesh asset system that
// does not exist yet. See `crates/crcbl-shaders/shaders/mesh_shader.slang`.

/// The size this suite renders at — the same 256×192 as the triangle and mesh
/// suites, for the same reason: the golden's structural metric works on 8×8
/// blocks and a smaller frame gives it too few to average over.
const MESH_SHADER_EXTENT: (u32, u32) = (256, 192);

/// The clear behind the triangle. The same one the triangle suite uses: dark,
/// and none of the vertex primaries, so "nothing drew" and "the triangle drew"
/// are not confusable.
const MESH_SHADER_CLEAR: [f32; 4] = crate::triangle::TRIANGLE_CLEAR;

impl Headless {
    /// Opens a ring at a pinned format, with both mesh stages asked for.
    ///
    /// See [`Headless::open_for_triangle`] on why the format is pinned rather
    /// than preferred. The two mesh flags are *optional* rather than required
    /// so a device without them opens and the assertion below is what fails —
    /// `HalError::UnsupportedFeatures` naming a flag says nothing about which
    /// suite wanted it.
    fn open_for_mesh_shader() -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e mesh shader"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::MESH_SHADER
                    | Features::TASK_SHADER
                    | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        assert!(
            device.caps().supports(Features::MESH_SHADER),
            "this suite needs VK_EXT_mesh_shader; radv and lavapipe both report it, and \
             `docs/backlog.md` is where a driver that does not belongs"
        );
        assert_eq!(
            device.caps().geometry_path(),
            crcbl_hal::GeometryPath::MeshShader,
            "a device reporting MESH_SHADER must select the path the flag is for"
        );
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e mesh shader ring"),
                surface,
                format,
                extent: MESH_SHADER_EXTENT,
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

/// Everything the mesh path needs, built through the seam.
///
/// There is a storage buffer and a bind group, and **no vertex buffer and no
/// index buffer**: a mesh pipeline has no vertex stage to fetch for, so the
/// geometry arrives the only way this engine has — pulled, by the mesh stage
/// itself, out of a buffer bound with `ShaderStages::MESH` visibility. That
/// visibility is what the rest of this fixture is evidence for.
struct MeshShaderResources {
    /// The pulled geometry, in `crcbl_shaders::mesh_shader`'s layout.
    vertices: crcbl_hal::BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    /// Mesh + fragment.
    mesh_only: crcbl_hal::GraphicsPipelineHandle,
    /// Task + mesh + fragment, when the device has the task stage.
    amplified: Option<crcbl_hal::GraphicsPipelineHandle>,
}

impl MeshShaderResources {
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();

        // The same staged upload the triangle suite does, for the same reason:
        // the buffer the shader reads is device-local, so the bytes go through
        // a host-visible one and a barrier.
        let bytes = crcbl_shaders::mesh_shader::vertex_bytes();
        let size = bytes.len() as u64;
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("mesh shader staging"),
                size,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(staging, 0, &bytes).expect("write");
        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("mesh shader vertices"),
                size,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a vertex buffer");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("mesh shader upload"),
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

        // **The point of this slice.** `ShaderStages::MESH` is outside
        // `GRAPHICS` and `ALL`, so it is named here explicitly — and a device
        // that did not report `MESH_SHADER` refuses this call rather than the
        // draw, which is what `Headless::open_for_mesh_shader` asserted about.
        let layout_entries = [crcbl_hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl_hal::ShaderStages::MESH,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        }];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("mesh shader vertices"),
                entries: &layout_entries,
            })
            .expect("a mesh-visible layout on a device that reports MESH_SHADER");

        let group_entries = [crcbl_hal::BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: crcbl_hal::BindingResource::whole_buffer(vertices),
        }];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("mesh shader vertices"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("mesh shader"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout carrying the mesh-visible set");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("mesh_shader.slang"),
                spirv: crcbl_shaders::MESH_SHADER.spirv(),
                // `None`, and that is the point of this shader: WGSL cannot
                // express a mesh stage, so there is no artifact to offer.
                wgsl: crcbl_shaders::MESH_SHADER.wgsl(),
                msl: crcbl_shaders::MESH_SHADER.msl(),
                dxil: &[],
            })
            .expect("the committed SPIR-V is accepted");
        assert_eq!(
            crcbl_shaders::MESH_SHADER.wgsl(),
            None,
            "the target declaration is what stops a broken WGSL artifact existing"
        );

        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let describe = |label, mesh_entry, task| MeshPipelineDesc {
            label: Some(label),
            layout: pipeline_layout,
            task,
            mesh: ShaderEntry {
                module,
                entry_point: mesh_entry,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        };

        let mesh_only = device
            .create_mesh_pipeline(&describe("mesh triangle", "meshMain", None))
            .expect("a mesh pipeline");

        // The task stage is a separate capability, so it is a separate
        // question. Both this branch and its absence are real device states,
        // and neither is a skip: a device with the mesh stage and no task stage
        // still renders the golden below.
        let amplified = device.caps().supports(Features::TASK_SHADER).then(|| {
            device
                .create_mesh_pipeline(&describe(
                    "amplified mesh triangle",
                    "amplifiedMeshMain",
                    Some(ShaderEntry {
                        module,
                        entry_point: "taskMain",
                    }),
                ))
                .expect("a task + mesh pipeline")
        });

        // The seam promises pipelines built from a module stay valid once it is
        // destroyed, and every draw below happens after the module is gone.
        device.destroy_shader_module(module);

        Self {
            vertices,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            mesh_only,
            amplified,
        }
    }

    fn destroy(self, device: &dyn Device) {
        if let Some(amplified) = self.amplified {
            // The *graphics* destroy, because the seam gives both pipeline
            // kinds one handle type. A leak here would show up as the
            // validation report failing at teardown.
            device.destroy_graphics_pipeline(amplified);
        }
        device.destroy_graphics_pipeline(self.mesh_only);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.vertices);
    }
}

/// Renders one frame with `pipeline` bound and reads the pixels back.
///
/// The same acquire → render pass → barrier → copy → poll shape as the triangle
/// suite's, with `draw_mesh_tasks` where its `draw` is — which is the whole
/// difference between the two geometry paths at this level.
fn render_mesh_shader(
    headless: &Headless,
    resources: &MeshShaderResources,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = MESH_SHADER_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_SHADER_EXTENT);

    let byte_count = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("mesh shader readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let range = ImageSubresourceRange::all(headless.format);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh shader frame"),
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
        label: Some("clear + mesh triangle"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(MESH_SHADER_CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(width, height),
    });
    encoder.set_viewport(&crcbl_hal::Viewport::from_size(width, height));
    encoder.set_scissor(&Rect2d::from_size(width, height));
    encoder.bind_graphics_pipeline(pipeline);
    // The geometry the mesh stage pulls. Bound exactly as the raster path binds
    // the vertex stage's — the stage mask on the layout is the only difference.
    encoder.bind_group(0, resources.bind_group, &[], resources.pipeline_layout);
    // One workgroup. Of the *task* stage when the pipeline has one, of the mesh
    // stage otherwise — and the shader's `taskMain` dispatches exactly one mesh
    // workgroup, so both pipelines run `emit` three times either way.
    encoder.draw_mesh_tasks(1, 1, 1);
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

    let mut bytes = vec![0u8; byte_count as usize];
    headless.readback(staging, byte_count, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    crcbl_golden::Image::from_readback(width, height, &bytes, crcbl_golden::ChannelOrder::Rgba)
        .expect("the readback is exactly one image")
}

/// Pixel coordinates 75% of the way from the centroid to corner `index`.
///
/// Derived from the geometry rather than guessed at as fractions of the frame,
/// for the reason the triangle suite gives: round fractions put the "apex"
/// sample outside the triangle, which is a test bug that looks exactly like a
/// Y flip.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn sample_near(index: usize) -> (u32, u32) {
    let positions = crcbl_shaders::mesh_shader::POSITIONS;
    let (width, height) = MESH_SHADER_EXTENT;
    let centroid = [
        positions.iter().map(|p| p[0]).sum::<f32>() / 3.0,
        positions.iter().map(|p| p[1]).sum::<f32>() / 3.0,
    ];
    let x = positions[index][0].mul_add(0.75, centroid[0] * 0.25);
    let y = positions[index][1].mul_add(0.75, centroid[1] * 0.25);
    // NDC to pixels. +Y is up in the seam's convention, which the backend's
    // negative-height viewport preserves, so the Y term is inverted here.
    (
        (((x + 1.0) * 0.5) * width as f32) as u32,
        (((1.0 - y) * 0.5) * height as f32) as u32,
    )
}

/// The corners of the frame, which the triangle does not cover.
fn frame_corners() -> [(u32, u32); 4] {
    let (width, height) = MESH_SHADER_EXTENT;
    [
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ]
}

/// §3.5's path, end to end and verified: a triangle whose vertices were emitted
/// by a **mesh stage**, with no vertex stage, no vertex input and no index
/// buffer anywhere in the pipeline.
///
/// The assertions are about geometry rather than exact colour, exactly as the
/// triangle suite's are: each corner of the triangle holds a different vertex's
/// colour, the centre blends all three, and the frame corners are still the
/// clear. A mesh shader that emitted nothing, emitted one vertex, got its index
/// triple wrong, or ran with a Y flip each breaks at least one of them.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_mesh_shader_emits_a_triangle_that_reaches_memory() {
    let headless = Headless::open_for_mesh_shader();
    let resources = MeshShaderResources::new(&headless);
    let image = render_mesh_shader(&headless, &resources, resources.mesh_only);

    let names = ["apex (red)", "top left (green)", "top right (blue)"];
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
             channel {index}. A Y flip, an X mirror or a reversed index triple each produce \
             exactly this."
        );
        assert!(
            u32::from(pixel[index]) > 150,
            "{name} at ({x}, {y}) is {pixel:?}; the dominant channel must be strong, not \
             merely largest"
        );
    }

    let (width, height) = MESH_SHADER_EXTENT;
    let centre = image.pixel(width / 2, height / 2).expect("inside");
    assert!(
        centre[0] > 20 && centre[1] > 20 && centre[2] > 20,
        "the centre must blend all three vertex colours, got {centre:?} — a mesh shader that \
         emitted one vertex correctly and the other two as zero passes every corner sample \
         and fails here"
    );
    assert_eq!(centre[3], 255, "alpha 1.0 must survive");

    for corner in frame_corners() {
        let pixel = image.pixel(corner.0, corner.1).expect("inside");
        assert!(
            pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 80,
            "corner {corner:?} must still be the clear colour, got {pixel:?}"
        );
    }

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// The **amplification stage runs and its payload arrives**, which is a
/// different claim from "the pipeline was created with a task stage".
///
/// `taskMain` puts a red-killing tint in its payload and `amplifiedMeshMain`
/// multiplies by it, so this frame differs from the one above in a way only the
/// task stage can produce: the apex goes dark while green and blue survive. A
/// task stage that never ran leaves the payload undefined; one that ran and
/// delivered nothing leaves it zero and blackens all three corners. Both fail
/// here, and neither would fail a test that only checked the pipeline was
/// created.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn an_amplification_stage_delivers_its_payload_to_the_mesh_stage() {
    let headless = Headless::open_for_mesh_shader();
    let resources = MeshShaderResources::new(&headless);
    let Some(amplified) = resources.amplified else {
        // Not a skip that hides: the device reported no task stage, which is a
        // legal state the seam models separately, and `MeshShaderResources`
        // built no pipeline for it.
        eprintln!("vk e2e: no TASK_SHADER on this device; the amplification test cannot run");
        resources.destroy(headless.device.as_ref());
        headless.finish();
        return;
    };
    let image = render_mesh_shader(&headless, &resources, amplified);

    let tint = crcbl_shaders::mesh_shader::AMPLIFICATION_TINT;
    assert_eq!(tint[0], 0.0, "the tint under test kills red");

    // The apex: red times zero. Only the red channel is asserted, because the
    // sample sits 25% of the way back towards the centroid and therefore
    // carries a quarter of the green and blue corners with it — the untinted
    // frame has this same pixel's red above 150.
    let (x, y) = sample_near(0);
    let apex = image.pixel(x, y).expect("inside");
    assert!(
        u32::from(apex[0]) < 20,
        "the apex at ({x}, {y}) is {apex:?}; the payload's tint zeroes red, and without the \
         task stage this channel is the dominant one"
    );

    // The other two: unchanged, which is what makes the assertion above a
    // statement about the *payload* rather than about the triangle vanishing.
    for (index, name) in [(1, "top left (green)"), (2, "top right (blue)")] {
        let (x, y) = sample_near(index);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            u32::from(pixel[index]) > 150,
            "{name} at ({x}, {y}) is {pixel:?}; this channel is multiplied by 1.0 and must \
             survive — a payload that arrived as all zeroes blackens the whole triangle"
        );
    }

    for corner in frame_corners() {
        let pixel = image.pixel(corner.0, corner.1).expect("inside");
        assert!(
            pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 80,
            "corner {corner:?} must still be the clear colour, got {pixel:?}"
        );
    }

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// The golden-image gate for the mesh-shading geometry path.
///
/// Per `docs/plan/39-capabilities.md`, golden images are per
/// `(GeometryPath, BindingModel, LightingPath)` combination a backend actually
/// selects — and until this slice `GeometryPath::MeshShader` had none, because
/// nothing could select it. This is that image.
///
/// The tolerance is the same [`Tolerance::RASTERISER`] every other golden here
/// uses, whose numbers were measured between radv and lavapipe rather than
/// guessed.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_mesh_shader_triangle_matches_its_golden_image() {
    let headless = Headless::open_for_mesh_shader();
    let resources = MeshShaderResources::new(&headless);
    let image = render_mesh_shader(&headless, &resources, resources.mesh_only);

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/mesh_shader_triangle.png");
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
    eprintln!(
        "vk e2e: golden mesh-shader triangle — {}",
        comparison.summary()
    );

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The capability guard, against a real driver.**
///
/// A device that reports `MESH_SHADER` cannot demonstrate the refusal, so this
/// asks for the one thing that is refusable on any device: a mesh pipeline
/// naming an entry point that is not a mesh entry point. The refusal has to
/// name the stage mix-up rather than fail somewhere later, which is the same
/// promise `create_graphics_pipeline` makes.
///
/// The `MESH_SHADER`-absent arm is exercised by `crcbl-hal`'s null backend,
/// which can model a device without the flag; no driver here can.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_mesh_pipeline_naming_a_fragment_entry_point_as_its_mesh_stage_is_refused() {
    let headless = Headless::open_for_mesh_shader();
    let device = headless.device.as_ref();
    let layout = device
        .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
            label: Some("mesh shader guard"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("a pipeline layout");
    let module = device
        .create_shader_module(&crcbl_hal::ShaderModuleDesc {
            label: Some("mesh_shader.slang"),
            spirv: crcbl_shaders::MESH_SHADER.spirv(),
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect("the committed SPIR-V is accepted");

    let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
    let error = device
        .create_mesh_pipeline(&MeshPipelineDesc {
            label: Some("wrong stage"),
            layout,
            task: None,
            mesh: ShaderEntry {
                module,
                entry_point: "fragmentMain",
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect_err("`fragmentMain` is not a mesh entry point");
    let HalError::ShaderCompilation(message) = error else {
        panic!("the refusal must name the stage mix-up");
    };
    assert!(
        message.contains("but not at") && message.contains("meshMain"),
        "the message must say the name exists at another stage and list what is there: \
         {message}"
    );

    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(layout);
    headless.finish();
}
