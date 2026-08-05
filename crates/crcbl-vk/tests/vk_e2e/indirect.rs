use crate::harness::Headless;
use crate::triangle::{TRIANGLE_CLEAR, TRIANGLE_EXTENT};
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, Device, Extent3d, Features, Format, ImageAspect, ImageSubresourceLayers,
    ImageSubresourceRange, LoadOp, MemoryLocation, PresentInfo, Rect2d, RenderPassDesc,
    ResourceState, StoreOp, SubmitInfo,
};

// --- the indirect draw path ------------------------------------------------
//
// `crcbl-hal`'s module docs call `draw_indexed_indirect_count` "Tier A's
// steady-state draw call — one per pass, regardless of scene size" and say
// `draw` "exists mostly for full-screen triangles and bring-up". Until this
// section, the bring-up path was the only one that had ever reached a driver.
//
// Every test here draws the *same* four triangles, one per quadrant, through
// `triangle.slang` — no new shader — and varies only the indirect arguments.
// That is what makes a backend which honoured the arguments distinguishable
// from one which ignored them and drew something reasonable anyway: each
// argument selects a different subset of the four, and the quadrants it did not
// select must still be the clear colour.

/// Where each quadrant's triangle sits, in NDC.
const QUADRANT_CENTRES: [[f32; 2]; 4] = [[-0.5, 0.5], [0.5, 0.5], [-0.5, -0.5], [0.5, -0.5]];

/// One saturated colour per quadrant, no two alike, and the fourth a pair of
/// channels rather than a third primary — so "the wrong triangle drew" is a
/// different picture rather than a dimmer one.
const QUADRANT_COLORS: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 1.0],
    [0.0, 1.0, 0.0, 1.0],
    [0.0, 0.0, 1.0, 1.0],
    [1.0, 1.0, 0.0, 1.0],
];

/// A triangle's three corners as offsets from its quadrant centre.
///
/// They sum to zero in both axes, so the centroid is the centre exactly and the
/// sample point needs no fudge factor.
const QUADRANT_CORNERS: [[f32; 2]; 3] = [[0.0, 0.30], [0.26, -0.15], [-0.26, -0.15]];

/// Vertices per quadrant triangle.
const QUADRANT_VERTICES: u32 = QUADRANT_CORNERS.len() as u32;

/// The per-channel ceiling for "this is still the clear colour".
///
/// [`TRIANGLE_CLEAR`] is dark but not black, and its blue is the largest
/// channel — the same asymmetry `a_triangle_pulled_from_a_storage_buffer_reaches_memory`
/// allows for at the frame corners.
const QUADRANT_DARK: [u32; 3] = [60, 60, 80];

/// The four triangles, in the `std430` layout `triangle.slang` declares.
fn quadrant_vertex_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (centre, color) in QUADRANT_CENTRES.iter().zip(&QUADRANT_COLORS) {
        for corner in &QUADRANT_CORNERS {
            // Clip space directly, as `triangle.slang` documents: there is no
            // camera, and `z` is inside Vulkan's 0..=w range.
            for value in [centre[0] + corner[0], centre[1] + corner[1], 0.5, 1.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for value in color {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    bytes
}

/// Where a quadrant's triangle should be sampled, in pixels.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quadrant_pixel(quadrant: usize) -> (u32, u32) {
    let (width, height) = TRIANGLE_EXTENT;
    let centre = QUADRANT_CENTRES[quadrant];
    // +Y is up in the seam's convention, which the backend's negative-height
    // viewport preserves, so the Y term is inverted here and nowhere else.
    (
        (((centre[0] + 1.0) * 0.5) * width as f32) as u32,
        (((1.0 - centre[1]) * 0.5) * height as f32) as u32,
    )
}

/// `VkDrawIndirectCommand`, little-endian.
///
/// The seam does not spell the argument layout — it is the backend's native one
/// — and this is a `crcbl-vk` test, so the Vulkan structure is what goes in.
fn draw_args(vertex_count: u32, instance_count: u32, first_vertex: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (slot, value) in [vertex_count, instance_count, first_vertex, 0]
        .iter()
        .enumerate()
    {
        out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// `VkDrawIndexedIndirectCommand`, little-endian. `vertexOffset` is signed and
/// zero throughout: the index buffer already selects the triangle.
fn draw_indexed_args(index_count: u32, instance_count: u32, first_index: u32) -> [u8; 20] {
    let mut out = [0u8; 20];
    for (slot, value) in [index_count, instance_count, first_index, 0, 0]
        .iter()
        .enumerate()
    {
        out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// Byte offsets into the one argument buffer every indirect test shares.
///
/// One buffer rather than one per call keeps the upload and its
/// `IndirectArgument` barrier in a single place, and a **non-zero** offset for
/// every argument a test actually uses is what makes "the offset was honoured"
/// checkable: a decoy at zero would otherwise be indistinguishable from the
/// real thing.
mod indirect_at {
    /// A decoy that draws all four quadrants, at the offset a backend which
    /// ignored `offset` would read.
    pub const DECOY: u64 = 0;
    /// One non-indexed draw of the first two quadrants.
    pub const DIRECT: u64 = 16;
    /// One indexed draw of the third quadrant alone.
    pub const INDEXED: u64 = 32;
    /// Two indexed draws — the first and last quadrants — at stride 20.
    pub const MULTI: u64 = 64;
    /// Two non-indexed draws, of which the count buffer selects one.
    pub const DIRECT_COUNT: u64 = 128;
    /// Four indexed draws, of which the count buffer selects three.
    pub const INDEXED_COUNT: u64 = 192;
    /// A `u32` draw count of one, for [`DIRECT_COUNT`].
    pub const COUNT_ONE: u64 = 288;
    /// A `u32` draw count of three, for [`INDEXED_COUNT`].
    pub const COUNT_THREE: u64 = 292;
    /// Bytes the buffer needs.
    pub const SIZE: u64 = 320;
}

/// `sizeof(VkDrawIndirectCommand)`.
const DRAW_ARGS_STRIDE: u32 = 16;
/// `sizeof(VkDrawIndexedIndirectCommand)`.
const DRAW_INDEXED_ARGS_STRIDE: u32 = 20;

/// The argument buffer's contents, laid out at [`indirect_at`]'s offsets.
fn indirect_args_bytes() -> Vec<u8> {
    let mut bytes = vec![0u8; indirect_at::SIZE as usize];
    let mut put = |offset: u64, source: &[u8]| {
        let at = offset as usize;
        bytes[at..at + source.len()].copy_from_slice(source);
    };
    let all = QUADRANT_VERTICES * QUADRANT_COLORS.len() as u32;

    put(indirect_at::DECOY, &draw_args(all, 1, 0));
    put(indirect_at::DIRECT, &draw_args(QUADRANT_VERTICES * 2, 1, 0));
    put(
        indirect_at::INDEXED,
        &draw_indexed_args(QUADRANT_VERTICES, 1, QUADRANT_VERTICES * 2),
    );
    put(
        indirect_at::MULTI,
        &draw_indexed_args(QUADRANT_VERTICES, 1, 0),
    );
    put(
        indirect_at::MULTI + u64::from(DRAW_INDEXED_ARGS_STRIDE),
        &draw_indexed_args(QUADRANT_VERTICES, 1, QUADRANT_VERTICES * 3),
    );
    // The count path's first argument draws one quadrant and its second draws
    // all four, so a backend that used `max_draw_count` in place of the count
    // buffer lights up the whole frame rather than one corner of it.
    put(
        indirect_at::DIRECT_COUNT,
        &draw_args(QUADRANT_VERTICES, 1, 0),
    );
    put(
        indirect_at::DIRECT_COUNT + u64::from(DRAW_ARGS_STRIDE),
        &draw_args(all, 1, 0),
    );
    for quadrant in 0..QUADRANT_COLORS.len() as u32 {
        put(
            indirect_at::INDEXED_COUNT + u64::from(quadrant * DRAW_INDEXED_ARGS_STRIDE),
            &draw_indexed_args(QUADRANT_VERTICES, 1, quadrant * QUADRANT_VERTICES),
        );
    }
    put(indirect_at::COUNT_ONE, &1u32.to_le_bytes());
    put(indirect_at::COUNT_THREE, &3u32.to_le_bytes());
    bytes
}

/// The four quadrant triangles, their indices, and the arguments that select
/// them — everything the indirect tests share.
struct QuadrantResources {
    vertices: crcbl_hal::BufferHandle,
    indices: crcbl_hal::BufferHandle,
    args: crcbl_hal::BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
}

impl QuadrantResources {
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let vertex_bytes = quadrant_vertex_bytes();
        // Identity indices: the argument's `first_index` is the lever under
        // test, so a permutation here would only make a failure harder to read.
        let index_bytes: Vec<u8> = (0..QUADRANT_VERTICES * QUADRANT_COLORS.len() as u32)
            .flat_map(u32::to_le_bytes)
            .collect();
        let args_bytes = indirect_args_bytes();

        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("quadrant upload"),
                size: (vertex_bytes.len() + index_bytes.len() + args_bytes.len()) as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        let vertex_at = 0u64;
        let index_at = vertex_bytes.len() as u64;
        let args_at = index_at + index_bytes.len() as u64;
        device
            .write_buffer(upload, vertex_at, &vertex_bytes)
            .expect("write");
        device
            .write_buffer(upload, index_at, &index_bytes)
            .expect("write");
        device
            .write_buffer(upload, args_at, &args_bytes)
            .expect("write");

        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("quadrant vertices"),
                size: vertex_bytes.len() as u64,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a vertex buffer");
        let indices = device
            .create_buffer(&BufferDesc {
                label: Some("quadrant indices"),
                size: index_bytes.len() as u64,
                usage: BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an index buffer");
        let args = device
            .create_buffer(&BufferDesc {
                label: Some("quadrant indirect arguments"),
                size: indirect_at::SIZE,
                usage: BufferUsage::INDIRECT | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an indirect buffer");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("quadrant upload"),
            queue: headless.queue,
        });
        for (src_offset, dst, size) in [
            (vertex_at, vertices, vertex_bytes.len() as u64),
            (index_at, indices, index_bytes.len() as u64),
            (args_at, args, args_bytes.len() as u64),
        ] {
            encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
                src: upload,
                src_offset,
                dst,
                dst_offset: 0,
                size,
            });
        }
        let barrier = |buffer, to| crcbl_hal::BufferBarrier {
            buffer,
            from: ResourceState::TransferDst,
            to,
            queue_transfer: None,
        };
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                barrier(vertices, ResourceState::ShaderRead),
                barrier(indices, ResourceState::IndexBuffer),
                // The barrier `crcbl-hal` calls "the single most important
                // barrier in a GPU-driven frame, and the one whose absence
                // produces 'sometimes nothing draws'".
                barrier(args, ResourceState::IndirectArgument),
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);

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
                label: Some("quadrant vertices"),
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
                label: Some("quadrant vertices"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("quadrants"),
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
                dxil: None,
            })
            .expect("the committed SPIR-V is accepted");
        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let pipeline = device
            .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
                label: Some("quadrants"),
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
        device.destroy_shader_module(module);

        Self {
            vertices,
            indices,
            args,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Renders one frame whose only draw call is whatever `record` issues.
    ///
    /// The index buffer is always bound, so an indexed and a non-indexed
    /// argument differ only in the call the test makes.
    fn render(
        &self,
        headless: &Headless,
        record: impl FnOnce(&mut dyn crcbl_hal::CommandEncoder),
    ) -> crcbl_golden::Image {
        let device = headless.device.as_ref();
        let (width, height) = TRIANGLE_EXTENT;
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        let byte_count = u64::from(width) * u64::from(height) * 4;
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("quadrant readback"),
                size: byte_count,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");

        let range = ImageSubresourceRange::all(headless.format);
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("quadrant frame"),
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
            label: Some("indirect quadrants"),
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
        encoder.bind_graphics_pipeline(self.pipeline);
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        encoder.bind_index_buffer(self.indices, 0, crcbl_hal::IndexFormat::Uint32);
        record(encoder.as_mut());
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
                },
            )
            .expect("present");

        let mut bytes = vec![0u8; byte_count as usize];
        headless.readback(staging, byte_count, &mut bytes);
        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);

        let order = match headless.format {
            Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
            _ => crcbl_golden::ChannelOrder::Rgba,
        };
        crcbl_golden::Image::from_readback(width, height, &bytes, order)
            .expect("the readback is exactly one image")
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.args);
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.vertices);
    }
}

/// Asserts exactly which quadrants the frame holds.
///
/// The point is the `false` entries as much as the `true` ones: an argument
/// that was read and obeyed draws a *subset*, and a backend that ignored the
/// arguments draws either nothing or everything. Both are caught here and
/// neither would be caught by looking only at what should be there.
fn assert_quadrants(image: &crcbl_golden::Image, drawn: [bool; 4], what: &str) {
    // A `drawn` that is all-true or all-false could not distinguish those two
    // failures, so the shape of the expectation is checked before the pixels.
    assert!(
        drawn.iter().any(|d| *d) && drawn.iter().any(|d| !*d),
        "{what}: an expectation of {drawn:?} cannot tell an honoured argument \
         from an ignored one"
    );
    for (quadrant, expected) in drawn.iter().enumerate() {
        let (x, y) = quadrant_pixel(quadrant);
        let pixel = image
            .pixel(x, y)
            .unwrap_or_else(|| panic!("{what}: quadrant {quadrant} sample ({x}, {y}) is outside"));
        let color = QUADRANT_COLORS[quadrant];
        for channel in 0..3 {
            let value = u32::from(pixel[channel]);
            if *expected && color[channel] == 1.0 {
                assert!(
                    value > 150,
                    "{what}: quadrant {quadrant} at ({x}, {y}) is {pixel:?}; channel \
                     {channel} must be strong because this quadrant was drawn"
                );
            } else {
                assert!(
                    value < QUADRANT_DARK[channel],
                    "{what}: quadrant {quadrant} at ({x}, {y}) is {pixel:?}; channel \
                     {channel} must be the clear colour. Expected drawn = {expected}, \
                     colour {color:?}."
                );
            }
        }
    }
    assert_eq!(
        image.pixel(0, 0).expect("inside")[3],
        255,
        "{what}: alpha 1.0 must survive"
    );
}

/// Indirect draws whose argument count is known on the CPU — Tier B's draw path,
/// and the shape every backend has.
///
/// Each argument sits at a **non-zero** offset with a decoy at zero that would
/// draw all four quadrants, so "read the arguments at the offset it was given"
/// is what the assertions actually distinguish. The multi-draw arm runs only
/// where [`Features::MULTI_DRAW_INDIRECT`] is reported, and the arms that ran
/// are named in the output so a run cannot silently check less than it looks.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn indirect_draws_execute_the_arguments_they_were_given() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let resources = QuadrantResources::new(&headless);
    let mut arms: Vec<&str> = Vec::new();

    // Non-indexed, one draw. `vertex_count` is the lever: `SV_VertexID` is
    // `gl_VertexIndex - gl_BaseVertex`, so `first_vertex` shifts both and the
    // shader pulls from zero whatever it is — which is why the argument here
    // selects a *prefix* rather than a slice.
    let image = resources.render(&headless, |encoder| {
        encoder.draw_indirect(&crcbl_hal::DrawIndirect {
            args: resources.args,
            offset: indirect_at::DIRECT,
            draw_count: 1,
            stride: DRAW_ARGS_STRIDE,
        });
    });
    assert_quadrants(&image, [true, true, false, false], "draw_indirect");
    arms.push("draw_indirect");

    // Indexed, one draw. Here `first_index` genuinely selects a slice, because
    // `SV_VertexID` is the value read out of the index buffer.
    let image = resources.render(&headless, |encoder| {
        encoder.draw_indexed_indirect(&crcbl_hal::DrawIndirect {
            args: resources.args,
            offset: indirect_at::INDEXED,
            draw_count: 1,
            stride: DRAW_INDEXED_ARGS_STRIDE,
        });
    });
    assert_quadrants(&image, [false, false, true, false], "draw_indexed_indirect");
    arms.push("draw_indexed_indirect");

    if device
        .caps()
        .features
        .contains(Features::MULTI_DRAW_INDIRECT)
    {
        // Two argument structures, one call. The two quadrants are the first
        // and the *last*, so a backend that read one structure, or read both at
        // the wrong stride, cannot land on this pair by accident.
        let image = resources.render(&headless, |encoder| {
            encoder.draw_indexed_indirect(&crcbl_hal::DrawIndirect {
                args: resources.args,
                offset: indirect_at::MULTI,
                draw_count: 2,
                stride: DRAW_INDEXED_ARGS_STRIDE,
            });
        });
        assert_quadrants(&image, [true, false, false, true], "multi-draw indirect");
        arms.push("multi-draw indirect (draw_count = 2)");
    } else {
        eprintln!(
            "vk e2e: Tier B device — no MULTI_DRAW_INDIRECT, so one argument per call is \
             all that was exercised"
        );
        arms.push("no MULTI_DRAW_INDIRECT on this device");
    }

    // Not decoration: a run that took no arm at all would otherwise pass having
    // rendered nothing, which is the shape this project has shipped broken.
    assert!(!arms.is_empty(), "no indirect arm ran");
    eprintln!("vk e2e: indirect draws — arms taken: {}", arms.join(", "));

    resources.destroy(device);
    headless.finish();
}

/// The steady-state draw path: the draw *count* comes out of GPU memory too.
///
/// `crcbl-hal` calls `draw_indexed_indirect_count` "Tier A's steady-state draw
/// call — one per pass, regardless of scene size", and until now it had never
/// touched a driver. Both tiers are covered: with
/// [`Features::DRAW_INDIRECT_COUNT`] the count buffer selects three of four
/// arguments, and without it the call must be refused at record time rather
/// than handed to a driver that has no entry point for it.
///
/// The count is deliberately **less** than `max_draw_count`. A backend that
/// passed the maximum through, or ignored the count buffer, draws the quadrant
/// the count excludes — which is the one assertion here that a merely-plausible
/// implementation cannot satisfy.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_steady_state_indirect_count_draw_path_reads_its_count_from_the_gpu() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let resources = QuadrantResources::new(&headless);
    let quadrants = QUADRANT_COLORS.len() as u32;

    if !device
        .caps()
        .features
        .contains(Features::DRAW_INDIRECT_COUNT)
    {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("indirect count on a Tier B device"),
            queue: headless.queue,
        });
        encoder.draw_indexed_indirect_count(&crcbl_hal::DrawIndirectCount {
            args: resources.args,
            args_offset: indirect_at::INDEXED_COUNT,
            count_buffer: resources.args,
            count_offset: indirect_at::COUNT_THREE,
            max_draw_count: quadrants,
            stride: DRAW_INDEXED_ARGS_STRIDE,
        });
        let error = encoder
            .finish()
            .expect_err("a device without DRAW_INDIRECT_COUNT must refuse the call");
        assert!(
            matches!(error, crcbl_hal::HalError::Unsupported { .. }),
            "the refusal must be loud and typed, not a silent no-op: {error}"
        );
        eprintln!("vk e2e: indirect count — Tier B device refused the call: {error}");
        resources.destroy(device);
        headless.finish();
        return;
    }

    eprintln!("vk e2e: indirect count — Tier A device, the count buffer is read on the GPU");
    assert!(
        device.caps().limits.max_draw_indirect_count >= quadrants,
        "a device reporting DRAW_INDIRECT_COUNT must allow the draws this test asks for; \
         max_draw_indirect_count is {}",
        device.caps().limits.max_draw_indirect_count
    );

    // Non-indexed first: two arguments, count one. The second would draw every
    // quadrant, so "the count was honoured" and "the maximum was used instead"
    // are three quadrants apart.
    let image = resources.render(&headless, |encoder| {
        encoder.draw_indirect_count(&crcbl_hal::DrawIndirectCount {
            args: resources.args,
            args_offset: indirect_at::DIRECT_COUNT,
            count_buffer: resources.args,
            count_offset: indirect_at::COUNT_ONE,
            max_draw_count: 2,
            stride: DRAW_ARGS_STRIDE,
        });
    });
    assert_quadrants(&image, [true, false, false, false], "draw_indirect_count");

    // And the one the seam calls the steady-state path: four arguments, count
    // three, one call for the whole pass.
    let image = resources.render(&headless, |encoder| {
        encoder.draw_indexed_indirect_count(&crcbl_hal::DrawIndirectCount {
            args: resources.args,
            args_offset: indirect_at::INDEXED_COUNT,
            count_buffer: resources.args,
            count_offset: indirect_at::COUNT_THREE,
            max_draw_count: quadrants,
            stride: DRAW_INDEXED_ARGS_STRIDE,
        });
    });
    assert_quadrants(
        &image,
        [true, true, true, false],
        "draw_indexed_indirect_count",
    );

    resources.destroy(device);
    headless.finish();
}
