// Native MSL isolates the HAL mesh command and binding contract from Slang.
use super::*;

// These qualification probes require actual reported/callable stage support.
fn open_mesh_proof_device() -> (crate::fault::Validated, MetalDevice) {
    let (validated, device) = open_device();
    assert!(device.inner.raw.supportsFamily(MTLGPUFamily::Metal3));
    assert!(
        device
            .caps()
            .features
            .contains(Features::MESH_SHADER | Features::TASK_SHADER)
    );
    assert_eq!(device.supports(Capability::MeshShading), Support::Yes);
    assert_eq!(device.supports(Capability::TaskShaderStage), Support::Yes);
    (validated, device)
}

const MESH_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Vertex { float4 position [[position]]; };
struct Primitive { float4 color; };
using Triangle = metal::mesh<Vertex, Primitive, 3, 1, topology::triangle>;
void emit_triangle(Triangle output, float4 color) {
    const float2 positions[3] = {float2(0, .8), float2(-.8, -.8), float2(.8, -.8)};
    for (uint i = 0; i < 3; ++i) {
        Vertex v; v.position = float4(positions[i], .25, 1);
        output.set_vertex(i, v);
        output.set_index(i, i);
    }
    Primitive p; p.color = color;
    output.set_primitive(0, p);
    output.set_primitive_count(1);
}
[[mesh]] void meshMain(Triangle output) {
    emit_triangle(output, float4(64., 128., 192., 255.) / 255.);
}
[[fragment]] float4 fragmentMain(Primitive primitive [[stage_in]]) {
    return primitive.color;
}
"#;

fn pipeline(
    device: &MetalDevice,
    module: ShaderModuleHandle,
    layout: PipelineLayoutHandle,
    task: Option<&str>,
    mesh: &str,
) -> GraphicsPipelineHandle {
    device
        .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
            label: Some("native mesh proof"),
            layout,
            task: task.map(|entry_point| ShaderEntry {
                module,
                entry_point,
            }),
            task_workgroup_size: [1, 1, 1],
            mesh: ShaderEntry {
                module,
                entry_point: mesh,
            },
            mesh_workgroup_size: [1, 1, 1],
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &[ColorTargetState::opaque(Format::Rgba8Unorm)],
        })
        .expect("native mesh pipeline")
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_direct_and_indirect_pixels() {
    let (_validated, device) = open_device();
    let module = device
        .create_shader_module(&msl_module(MESH_MSL, "native mesh proof"))
        .expect("native mesh MSL compiles");
    let layout = empty_layout(&device);
    let pipeline = pipeline(&device, module, layout, None, "meshMain");
    let direct = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        encoder.bind_graphics_pipeline(pipeline);
        encoder.draw_mesh_tasks(1, 1, 1);
    });
    assert_ink_triangle(&direct, Format::Rgba8Unorm);

    // Offset skips a zero launch; stride skips another zero launch. Only the
    // second requested record paints, so dropping count/stride/offset blanks it.
    let words: [u32; 15] = [0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1];
    let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_ne_bytes).collect();
    let args = device
        .create_buffer(&BufferDesc {
            label: Some("mesh indirect offset/stride/count"),
            size: bytes.len() as u64,
            usage: BufferUsage::INDIRECT,
            memory: MemoryLocation::HostUpload,
        })
        .expect("indirect buffer");
    device.write_buffer(args, 0, &bytes).unwrap();
    let indirect = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        encoder.bind_graphics_pipeline(pipeline);
        encoder.draw_mesh_tasks_indirect(&crcbl_hal::DrawIndirect {
            args,
            offset: 12,
            stride: 24,
            draw_count: 2,
        });
        // Recorded replay must retain the native indirect resource.
        device.destroy_buffer(args);
    });
    assert_eq!(
        indirect, direct,
        "indirect launches reproduce all direct pixels"
    );
    device.destroy_graphics_pipeline(pipeline);
    device.destroy_pipeline_layout(layout);
    device.destroy_shader_module(module);
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_object_payload_stage_bindings_and_recorded_lifetimes() {
    use crcbl_hal::{
        BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, PushConstantRange,
        ShaderStages,
    };
    let (_validated, device) = open_mesh_proof_device();
    let source = format!(
        r#"{MESH_MSL}
struct Payload {{ uint4 color; }};
[[object]] void objectMain(object_data Payload& payload [[payload]],
    device const uint4& values [[buffer(0)]], constant uint4& constants [[buffer(3)]],
    mesh_grid_properties grid) {{
    payload.color = values + constants;
    grid.set_threadgroups_per_grid(uint3(constants.x == 1 ? 1 : 0, 1, 1));
}}
[[mesh]] void payloadMesh(Triangle output, object_data const Payload& payload [[payload]],
    device const uint4& values [[buffer(1)]], constant uint4& constants [[buffer(3)]]) {{
    emit_triangle(output, float4(payload.color + values + constants));
}}
[[fragment]] float4 boundFragment(Primitive primitive [[stage_in]],
    device const uint4& values [[buffer(2)]], constant uint4& constants [[buffer(3)]]) {{
    return (primitive.color + float4(values + constants)) / 255.;
}}
"#
    )
    .replace(
        "[[fragment]] float4 fragmentMain",
        "[[fragment]] float4 unusedFragment",
    )
    .replace("boundFragment", "fragmentMain");
    let module = device
        .create_shader_module(&msl_module(&source, "native object payload"))
        .expect("object payload MSL");
    let stages = [
        ShaderStages::TASK,
        ShaderStages::MESH,
        ShaderStages::FRAGMENT,
    ];
    let entries: Vec<_> = stages
        .iter()
        .enumerate()
        .map(|(index, &visibility)| BindGroupLayoutEntry {
            binding: index as u32,
            visibility,
            kind: BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        })
        .collect();
    let set = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("stage-specific mesh buffers"),
            entries: &entries,
        })
        .unwrap();
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("three buffers then constants"),
            bind_group_layouts: &[set],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::TASK | ShaderStages::MESH | ShaderStages::FRAGMENT,
                offset: 0,
                size: 16,
            }),
        })
        .unwrap();
    let buffers: Vec<_> = [[10u32, 20, 30, 40], [5, 6, 7, 8], [46, 96, 146, 195]]
        .into_iter()
        .map(|words| {
            let buffer = device
                .create_buffer(&BufferDesc {
                    label: Some("stage values"),
                    size: 16,
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })
                .unwrap();
            let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_ne_bytes).collect();
            device.write_buffer(buffer, 0, &bytes).unwrap();
            buffer
        })
        .collect();
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(i, &buffer)| BindGroupEntry {
            binding: i as u32,
            array_index: 0,
            resource: BindingResource::whole_buffer(buffer),
        })
        .collect();
    let group = device
        .create_bind_group(&BindGroupDesc {
            label: None,
            layout: set,
            entries: &entries,
            variable_count: None,
        })
        .unwrap();
    let pipeline = pipeline(&device, module, layout, Some("objectMain"), "payloadMesh");
    let culled = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        encoder.bind_graphics_pipeline(pipeline);
        encoder.bind_group(0, group, &[], layout);
        let data: Vec<u8> = [0u32, 2, 3, 4]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        encoder.push_constants(
            ShaderStages::TASK | ShaderStages::MESH | ShaderStages::FRAGMENT,
            0,
            &data,
            layout,
        );
        encoder.draw_mesh_tasks(1, 1, 1);
    });
    assert!(
        culled.chunks_exact(4).all(|pixel| pixel == CLEAR_TEXEL),
        "object stage with a zero mesh launch leaves every pixel clear"
    );
    let bytes = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        encoder.bind_graphics_pipeline(pipeline);
        encoder.bind_group(0, group, &[], layout);
        let data: Vec<u8> = [1u32, 2, 3, 4]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        encoder.push_constants(
            ShaderStages::TASK | ShaderStages::MESH | ShaderStages::FRAGMENT,
            0,
            &data,
            layout,
        );
        encoder.draw_mesh_tasks(1, 1, 1);
        // Destroy every externally owned input before the recorded pass replays.
        device.destroy_bind_group(group);
        for &buffer in &buffers {
            device.destroy_buffer(buffer);
        }
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_shader_module(module);
    });
    assert_ink_triangle(&bytes, Format::Rgba8Unorm);
    device.destroy_pipeline_layout(layout);
    device.destroy_bind_group_layout(set);
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_depth_only_and_pass_timestamps() {
    let (_validated, device) = open_device();
    assert!(
        device.caps().features.contains(Features::TIMESTAMP_QUERY),
        "proof requires stage counters"
    );
    let module = device
        .create_shader_module(&msl_module(MESH_MSL, "mesh depth proof"))
        .unwrap();
    let layout = empty_layout(&device);
    for depth_only in [false, true] {
        let format = if depth_only {
            Format::D32Float
        } else {
            Format::Rgba8Unorm
        };
        let targets = [ColorTargetState::opaque(format)];
        let pipeline = device
            .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                label: Some("native timed mesh"),
                layout,
                task: None,
                task_workgroup_size: [1, 1, 1],
                mesh: ShaderEntry {
                    module,
                    entry_point: "meshMain",
                },
                mesh_workgroup_size: [1, 1, 1],
                fragment: (!depth_only).then_some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: depth_only.then_some(DepthStencilState {
                    format,
                    depth_write: true,
                    depth_compare: CompareOp::Always,
                    stencil: None,
                    bias: DepthBias::default(),
                }),
                multisample: MultisampleState::default(),
                color_targets: if depth_only { &[] } else { &targets },
            })
            .unwrap();
        {
            let bound = device.inner.graphics_pipeline_raw(pipeline).unwrap();
            println!(
                "native mesh pipeline (requested one thread): device={} Apple9={} Metal3={} mesh_threads={} mesh_width={} mesh_grid={}",
                device.inner.raw.name(),
                device.inner.raw.supportsFamily(MTLGPUFamily::Apple9),
                device.inner.raw.supportsFamily(MTLGPUFamily::Metal3),
                bound.raw.maxTotalThreadsPerMeshThreadgroup(),
                bound.raw.meshThreadExecutionWidth(),
                bound.raw.maxTotalThreadgroupsPerMeshGrid()
            );
        }
        let image = device
            .create_image(&ImageDesc {
                label: Some("native mesh timed target"),
                image_type: ImageType::D2,
                extent: CANVAS,
                format,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::TRANSFER_SRC
                    | if depth_only {
                        ImageUsage::DEPTH_STENCIL_ATTACHMENT
                    } else {
                        ImageUsage::COLOR_ATTACHMENT
                    },
            })
            .unwrap();
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format,
                range: ImageSubresourceRange::all(format),
            })
            .unwrap();
        let queries = device
            .create_query_set(&QuerySetDesc {
                label: Some("native mesh timestamps"),
                kind: QueryKind::Timestamp,
                count: 2,
            })
            .unwrap();
        let readback = readback_buffer(&device, CANVAS_BYTES as u64);
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        let colors = [ColorAttachment {
            view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }];
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("native timed mesh"),
            color_attachments: if depth_only { &[] } else { &colors },
            depth_stencil_attachment: depth_only.then_some(DepthStencilAttachment {
                view,
                read_only: false,
                depth_load: LoadOp::Clear,
                depth_store: StoreOp::Store,
                stencil_load: LoadOp::DontCare,
                stencil_store: StoreOp::Discard,
                clear: ClearValue {
                    depth: 1.0,
                    ..ClearValue::default()
                },
            }),
            render_area: Rect2d::from_size(CANVAS.width, CANVAS.height),
            timestamp_writes: Some(crcbl_hal::PassTimestampWrites {
                set: queries,
                beginning_of_pass: 0,
                end_of_pass: 1,
            }),
        });
        encoder.set_viewport(&Viewport::from_size(CANVAS.width, CANVAS.height));
        encoder.set_scissor(&Rect2d::from_size(CANVAS.width, CANVAS.height));
        encoder.bind_graphics_pipeline(pipeline);
        encoder.draw_mesh_tasks(1, 1, 1);
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                image,
                ImageSubresourceRange::all(format),
                if depth_only {
                    ResourceState::DepthStencilWrite
                } else {
                    ResourceState::ColorAttachment
                },
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        let mut copy = whole_image_copy_of(image, readback, CANVAS);
        if depth_only {
            copy.image_subresource.aspect = ImageAspect::DEPTH;
        }
        encoder.copy_image_to_buffer(&copy);
        let commands = encoder.finish().unwrap();
        // Native encoders must retain resources after the HAL owners disappear,
        // including in the interval between submit and GPU completion.
        device.submit(queue, &SubmitInfo::new(&[commands])).unwrap();
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_image_view(view);
        device.destroy_image(image);
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: CANVAS_BYTES as u64,
                after: None,
            })
            .unwrap();
        let bytes = drain(&device, request, CANVAS_BYTES);
        if depth_only {
            let depth = |x, y| f32::from_ne_bytes(texel_at(&bytes, x, y));
            assert_eq!(depth(CANVAS.width / 2, CANVAS.height / 2), 0.25);
            for (x, y) in [
                (0, 0),
                (CANVAS.width - 1, 0),
                (0, CANVAS.height - 1),
                (CANVAS.width - 1, CANVAS.height - 1),
            ] {
                assert_eq!(depth(x, y), 1.0, "corner depth stays clear");
            }
        } else {
            assert_ink_triangle(&bytes, format);
        }
        let mut timestamps = [0; 2];
        device.query_results(queries, 0, &mut timestamps).unwrap();
        println!("native mesh depth_only={depth_only} timestamps_ns={timestamps:?}");
        assert!(
            timestamps[0] > 0 && timestamps[1] > timestamps[0],
            "mesh pass timestamps bracket executed work"
        );
        device.destroy_readback(request);
        device.destroy_buffer(readback);
        device.destroy_command_buffer(commands);
        device.destroy_query_set(queries);
    }
    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(layout);
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_object_textures_samplers_and_bindless_nonzero_elements() {
    use crcbl_hal::{
        BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, FilterMode, SampleType,
        SamplerAddressMode, ShaderStages,
    };
    let (_validated, device) = open_mesh_proof_device();
    let source = format!(
        r#"{MESH_MSL}
struct Payload {{ float4 color; }};
struct Sources {{ device const uint* values[2]; }};
[[object]] void sampledObject(object_data Payload& payload [[payload]], mesh_grid_properties grid,
    texture2d<float> image [[texture(0)]], sampler filtering [[sampler(0)]],
    constant Sources& sources [[buffer(0)]]) {{
    float4 sampled = image.sample(filtering, float2(1.25, .5), level(0));
    payload.color = float4(float(*sources.values[1]), round(sampled.g * 255.), 0, 255);
    grid.set_threadgroups_per_grid(uint3(1));
}}
[[mesh]] void sampledMesh(Triangle output, object_data const Payload& payload [[payload]],
    texture2d<float> image [[texture(1)]], sampler filtering [[sampler(1)]],
    constant Sources& sources [[buffer(1)]]) {{
    float4 sampled = image.sample(filtering, float2(1.25, .5), level(0));
    float4 color = payload.color;
    color.b = round(sampled.b * 255.) * float(*sources.values[1]);
    emit_triangle(output, color / 255.);
}}
"#
    );
    let module = device
        .create_shader_module(&msl_module(&source, "native sampled mesh"))
        .unwrap();
    let mut sets = Vec::new();
    let mut groups = Vec::new();
    let mut images = Vec::new();
    let mut views = Vec::new();
    let mut samplers = Vec::new();
    let mut buffers = Vec::new();
    for (stage, pixels, address, value) in [
        (
            ShaderStages::TASK,
            [255, 0, 0, 255, 0, 128, 0, 255],
            SamplerAddressMode::ClampToEdge,
            64u32,
        ),
        (
            ShaderStages::MESH,
            [0, 0, 192, 255, 255, 0, 0, 255],
            SamplerAddressMode::Repeat,
            1u32,
        ),
    ] {
        let image = device
            .create_image(&ImageDesc {
                label: Some("stage-distinct texture"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(2, 1),
                format: Format::Rgba8Unorm,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            })
            .unwrap();
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .unwrap();
        let upload = device
            .create_buffer(&BufferDesc {
                label: None,
                size: 8,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .unwrap();
        device.write_buffer(upload, 0, &pixels).unwrap();
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.copy_buffer_to_image(&whole_image_copy_of(image, upload, Extent3d::d2(2, 1)));
        let commands = encoder.finish().unwrap();
        device.submit(queue, &SubmitInfo::new(&[commands])).unwrap();
        device.wait_idle().unwrap();
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);
        let sampler = device
            .create_sampler(&SamplerDesc {
                address_mode: [address; 3],
                mag_filter: FilterMode::Nearest,
                min_filter: FilterMode::Nearest,
                mip_filter: FilterMode::Nearest,
                ..SamplerDesc::default()
            })
            .unwrap();
        let mut sources = Vec::new();
        for word in [0u32, value] {
            let buffer = device
                .create_buffer(&BufferDesc {
                    label: Some("nonzero bindless element"),
                    size: 4,
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })
                .unwrap();
            device.write_buffer(buffer, 0, &word.to_ne_bytes()).unwrap();
            sources.push(buffer);
        }
        let set = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: None,
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: stage,
                        kind: BindingKind::SampledImage {
                            view_type: ImageViewType::D2,
                            sample_type: SampleType::Float,
                        },
                        count: 1,
                        flags: BindingFlags::empty(),
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: stage,
                        kind: BindingKind::Sampler { comparison: false },
                        count: 1,
                        flags: BindingFlags::empty(),
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: stage,
                        kind: BindingKind::StorageBuffer {
                            read_only: true,
                            dynamic: false,
                        },
                        count: 2,
                        flags: BindingFlags::VARIABLE_COUNT,
                    },
                ],
            })
            .unwrap();
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: set,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::Sampler(sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(sources[0]),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 1,
                        resource: BindingResource::whole_buffer(sources[1]),
                    },
                ],
                variable_count: Some(2),
            })
            .unwrap();
        sets.push(set);
        groups.push(group);
        images.push(image);
        views.push(view);
        samplers.push(sampler);
        buffers.extend(sources);
    }
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &sets,
            push_constants: None,
        })
        .unwrap();
    let pipeline = pipeline(
        &device,
        module,
        layout,
        Some("sampledObject"),
        "sampledMesh",
    );
    let pixels = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        encoder.bind_graphics_pipeline(pipeline);
        for (slot, &group) in groups.iter().enumerate() {
            encoder.bind_group(slot as u32, group, &[], layout);
        }
        encoder.draw_mesh_tasks(1, 1, 1);
        for &group in &groups {
            device.destroy_bind_group(group);
        }
        for &buffer in &buffers {
            device.destroy_buffer(buffer);
        }
        for &view in &views {
            device.destroy_image_view(view);
        }
        for &image in &images {
            device.destroy_image(image);
        }
        for &sampler in &samplers {
            device.destroy_sampler(sampler);
        }
    });
    assert_ink_triangle(&pixels, Format::Rgba8Unorm);
    device.destroy_graphics_pipeline(pipeline);
    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(layout);
    for set in sets {
        device.destroy_bind_group_layout(set);
    }
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_same_pass_raster_mesh_argument_replacement() {
    use crcbl_hal::{
        BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, PushConstantRange,
        ShaderStages,
    };
    let (_validated, device) = open_mesh_proof_device();
    let source = format!(
        r#"{MESH_MSL}
struct RasterVertex {{ float4 position [[position]]; float4 color; }};
[[vertex]] RasterVertex coloredVertex(uint index [[vertex_id]], constant float4& color [[buffer(0)]]) {{
    const float2 positions[3] = {{float2(0, .8), float2(-.8, -.8), float2(.8, -.8)}};
    RasterVertex v; v.position = float4(positions[index], .25, 1); v.color = color; return v;
}}
[[mesh]] void coloredMesh(Triangle output, constant float4& color [[buffer(0)]]) {{
    emit_triangle(output, float4(color.zxy, color.w));
}}
"#
    );
    let module = device
        .create_shader_module(&msl_module(&source, "mixed raster mesh proof"))
        .unwrap();
    let stages = ShaderStages::VERTEX | ShaderStages::MESH;
    let set = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: stages,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            }],
        })
        .unwrap();
    let grouped_layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[set],
            push_constants: None,
        })
        .unwrap();
    let inline_layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[],
            push_constants: Some(PushConstantRange {
                stages,
                offset: 0,
                size: 16,
            }),
        })
        .unwrap();
    let raster = |layout| {
        device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("mixed proof raster"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "coloredVertex",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &[ColorTargetState::opaque(Format::Rgba8Unorm)],
            })
            .unwrap()
    };
    let raster_grouped = raster(grouped_layout);
    let raster_inline = raster(inline_layout);
    let mesh_grouped = pipeline(&device, module, grouped_layout, None, "coloredMesh");
    let mesh_inline = pipeline(&device, module, inline_layout, None, "coloredMesh");
    let colors = [
        [1f32, 0., 0., 1.],
        [0., 1., 0., 1.],
        [0., 0., 1., 1.],
        [1., 1., 0., 1.],
        [1., 0., 1., 1.],
        [0., 1., 1., 1.],
    ];
    // Mesh rotates RGB, so the first raster/mesh switch can reuse the exact
    // same group and native buffer while still producing distinct pixels.
    // A cache incorrectly shared between Vertex and Mesh would skip that bind.
    let inputs = [
        colors[0], colors[0], colors[1], colors[4], colors[4], colors[5],
    ];
    let mut buffers = Vec::new();
    let mut groups = Vec::new();
    for color in inputs {
        let buffer = device
            .create_buffer(&BufferDesc {
                label: None,
                size: 16,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .unwrap();
        let data: Vec<u8> = color.into_iter().flat_map(f32::to_ne_bytes).collect();
        device.write_buffer(buffer, 0, &data).unwrap();
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: set,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                }],
                variable_count: None,
            })
            .unwrap();
        buffers.push(buffer);
        groups.push(group);
    }
    let pixels = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
        for (tile, (pipeline, is_mesh, inline)) in [
            (raster_grouped, false, false),
            (mesh_grouped, true, false),
            (mesh_inline, true, true),
            (mesh_grouped, true, false),
            (raster_inline, false, true),
            (raster_grouped, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            encoder.set_viewport(&Viewport {
                x: (tile * 10) as f32,
                ..Viewport::from_size(10, CANVAS.height)
            });
            encoder.set_scissor(&Rect2d {
                x: (tile * 10) as i32,
                ..Rect2d::from_size(10, CANVAS.height)
            });
            encoder.bind_graphics_pipeline(pipeline);
            if inline {
                let bytes: Vec<u8> = inputs[tile]
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect();
                encoder.push_constants(stages, 0, &bytes, inline_layout);
            } else {
                let group = if tile == 1 { groups[0] } else { groups[tile] };
                encoder.bind_group(0, group, &[], grouped_layout);
            }
            if is_mesh {
                encoder.draw_mesh_tasks(1, 1, 1);
            } else {
                encoder.draw(0..3, 0..1);
            }
        }
        for &group in &groups {
            device.destroy_bind_group(group);
        }
        for &buffer in &buffers {
            device.destroy_buffer(buffer);
        }
    });
    for (tile, color) in colors.into_iter().enumerate() {
        let expected = color.map(|component| (component * 255.) as u8);
        assert_eq!(
            texel_at(&pixels, tile as u32 * 10 + 5, CANVAS.height / 2),
            expected,
            "tile {tile}: raster/mesh and group/inline replacement must preserve the last writer"
        );
        assert_eq!(texel_at(&pixels, tile as u32 * 10, 0), CLEAR_TEXEL);
    }
    for pipeline in [raster_grouped, raster_inline, mesh_grouped, mesh_inline] {
        device.destroy_graphics_pipeline(pipeline);
    }
    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(grouped_layout);
    device.destroy_pipeline_layout(inline_layout);
    device.destroy_bind_group_layout(set);
}

#[test]
#[ignore = "requires real Metal mesh hardware"]
fn native_mesh_writable_task_mesh_bindless_readback() {
    use crcbl_hal::{
        BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, ShaderStages,
    };
    let (_validated, device) = open_mesh_proof_device();
    let source = format!(
        r#"{MESH_MSL}
struct Sources {{ device uint* values[2]; }};
struct Payload {{ uint value; }};
[[object]] void writingObject(object_data Payload& payload [[payload]], mesh_grid_properties grid,
    constant Sources& output [[buffer(0)]]) {{
    payload.value = 0x10203040u;
    *output.values[1] = payload.value;
    grid.set_threadgroups_per_grid(uint3(1));
}}
[[mesh]] void writingMesh(Triangle mesh, object_data const Payload& payload [[payload]],
    constant Sources& output [[buffer(1)]]) {{
    *output.values[1] = payload.value ^ 0x55667788u;
    emit_triangle(mesh, float4(64.,128.,192.,255.) / 255.);
}}
"#
    );
    let module = device
        .create_shader_module(&msl_module(&source, "writable task mesh proof"))
        .unwrap();
    let mut sets = Vec::new();
    let mut groups = Vec::new();
    let mut buffers = Vec::new();
    for stage in [ShaderStages::TASK, ShaderStages::MESH] {
        let set = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: None,
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: stage,
                    kind: BindingKind::StorageBuffer {
                        read_only: false,
                        dynamic: false,
                    },
                    count: 2,
                    flags: BindingFlags::VARIABLE_COUNT,
                }],
            })
            .unwrap();
        let sources: Vec<_> = (0..2)
            .map(|_| {
                device
                    .create_buffer(&BufferDesc {
                        label: Some("writable bindless element"),
                        size: 4,
                        usage: BufferUsage::STORAGE
                            | BufferUsage::TRANSFER_SRC
                            | BufferUsage::TRANSFER_DST,
                        memory: MemoryLocation::DeviceLocal,
                    })
                    .unwrap()
            })
            .collect();
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: set,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(sources[0]),
                    },
                    BindGroupEntry {
                        binding: 0,
                        array_index: 1,
                        resource: BindingResource::whole_buffer(sources[1]),
                    },
                ],
                variable_count: Some(2),
            })
            .unwrap();
        sets.push(set);
        groups.push(group);
        buffers.extend(sources);
    }
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &sets,
            push_constants: None,
        })
        .unwrap();
    let pipeline = pipeline(
        &device,
        module,
        layout,
        Some("writingObject"),
        "writingMesh",
    );
    let queue = device.queue(QueueKind::Graphics).unwrap();
    let readback = readback_buffer(&device, 16);
    let (image, view) = color_target_of(&device, CANVAS, Format::Rgba8Unorm);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    for &buffer in &buffers {
        encoder.clear_buffer(buffer, 0, 4);
    }
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("writable task mesh"),
        color_attachments: &[ColorAttachment {
            view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Discard,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(CANVAS.width, CANVAS.height),
        timestamp_writes: None,
    });
    encoder.set_viewport(&Viewport::from_size(CANVAS.width, CANVAS.height));
    encoder.set_scissor(&Rect2d::from_size(CANVAS.width, CANVAS.height));
    encoder.bind_graphics_pipeline(pipeline);
    for (slot, &group) in groups.iter().enumerate() {
        encoder.bind_group(slot as u32, group, &[], layout);
    }
    encoder.draw_mesh_tasks(1, 1, 1);
    // Recorded bindings must retain both address tables before replay starts.
    for &group in &groups {
        device.destroy_bind_group(group);
    }
    encoder.end_render_pass();
    for (element, &buffer) in buffers.iter().enumerate() {
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: buffer,
            src_offset: 0,
            dst: readback,
            dst_offset: element as u64 * 4,
            size: 4,
        });
        // Copy recording/native encoders retain the output after owner removal.
        device.destroy_buffer(buffer);
    }
    let commands = encoder.finish().unwrap();
    device.submit(queue, &SubmitInfo::new(&[commands])).unwrap();
    let request = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer: readback,
            offset: 0,
            size: 16,
            after: None,
        })
        .unwrap();
    let bytes = drain(&device, request, 16);
    let words: Vec<_> = bytes
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes(word.try_into().unwrap()))
        .collect();
    assert_eq!(
        words,
        [0, 0x10203040, 0, 0x10203040 ^ 0x55667788],
        "task and mesh writes target element1, preserve element0 and survive owner destruction"
    );
    device.destroy_readback(request);
    device.destroy_buffer(readback);
    device.destroy_command_buffer(commands);
    device.destroy_graphics_pipeline(pipeline);
    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(layout);
    device.destroy_image_view(view);
    device.destroy_image(image);
    for set in sets {
        device.destroy_bind_group_layout(set);
    }
}
