//! The seam, exercised the way a real consumer will exercise it.
//!
//! This file is the load-bearing half of the slice's test suite, and it is an
//! *integration* test rather than a unit test for one reason: an in-crate test
//! can reach private items, so it cannot prove anything about what the crate
//! exposes. This one compiles against `crcbl-hal`'s public API only.
//!
//! It asserts three things:
//!
//! 1. **The seam is usable through trait objects.** Everything after
//!    construction goes through `Box<dyn Instance>` / `Box<dyn Device>` /
//!    `Box<dyn CommandEncoder>`. If any trait stopped being object-safe, this
//!    file would not compile.
//! 2. **No backend-specific type appears in a public signature.** Every value
//!    produced by the seam is bound to an explicitly written type here. The only
//!    concrete backend type named anywhere is [`NullInstance`] at construction,
//!    plus the [`Recorder`] used to read the stream back — everything else is a
//!    trait object, a POD descriptor, or a handle. A method that returned a
//!    backend type would force a `crcbl_hal::null::…` name into one of these
//!    annotations.
//! 3. **The recorded stream is what the frame asked for**, including pass order.
//!
//! It also runs the *same frame function* against both tier presets, which is
//! the property topic 03 depends on: one renderer, two tiers, differing only in
//! the draw tail.

use crcbl_hal::null::{Command, NullInstance, ObjectKind, Recorder};
use crcbl_hal::{
    AdapterInfo, Barriers, BufferBarrier, BufferDesc, BufferHandle, BufferUsage, ClearValue,
    ColorAttachment, ColorTargetState, CommandBufferHandle, CommandEncoder, CommandEncoderDesc,
    DepthStencilAttachment, DepthStencilState, Device, DeviceCaps, DeviceDesc, DrawIndirect,
    DrawIndirectCount, Extent3d, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc,
    ImageViewHandle, ImageViewType, Instance, LoadOp, MemoryLocation, MultisampleState,
    PresentInfo, PresentMode, PrimitiveState, QueueFamily, QueueHandle, ReadbackDesc,
    ReadbackHandle, ReadbackState, Rect2d, RenderPassDesc, RendererTier, ShaderEntry,
    ShaderModuleDesc, ShaderModuleHandle, StoreOp, SubmitInfo, SurfaceCaps, SurfaceHandle,
    SurfaceTarget, SwapchainDesc, SwapchainHandle, Viewport, depth,
};

/// A minimal valid SPIR-V header — magic word plus version.
const SPIRV: [u32; 5] = [0x0723_0203, 0x0001_0600, 0, 0, 0];

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

/// Everything a frame needs, all named by handle rather than by backend type.
struct Frame {
    device: Box<dyn Device>,
    queue: QueueHandle,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    color: ImageHandle,
    color_view: ImageViewHandle,
    depth_view: ImageViewHandle,
    draw_args: BufferHandle,
    draw_count: BufferHandle,
    pipeline: GraphicsPipelineHandle,
    tier: RendererTier,
}

/// Builds a device and its per-frame resources from a boxed instance.
///
/// Every binding below carries an explicit type annotation on purpose: a
/// backend type escaping through a public signature would have to be written
/// here, and there is nowhere to write one.
fn setup(instance: &dyn Instance) -> Frame {
    let adapters: Vec<AdapterInfo> = instance.adapters();
    assert_eq!(adapters.len(), 1);
    let caps: DeviceCaps = adapters[0].caps;
    let tier: RendererTier = caps.tier();

    // Ask only for what the adapter has; the tier decides the rest.
    let required = if tier.is_a() {
        Features::TIER_A
    } else {
        Features::COMPUTE
    };
    let device: Box<dyn Device> = instance
        .create_device(&DeviceDesc {
            label: Some("frame device"),
            adapter: adapters[0].id,
            required_features: required,
            optional_features: Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
            compatible_surface: None,
        })
        .expect("the adapter satisfies what we asked for");
    assert_eq!(device.caps().tier(), tier);

    let queue: QueueHandle = device
        .queue(QueueFamily::Graphics)
        .expect("the graphics family always exists");

    // SAFETY: `Offscreen` carries no platform pointers, so
    // `create_surface`'s contract — live handles that outlive the surface — is
    // vacuously satisfied.
    let surface: SurfaceHandle = unsafe {
        instance.create_surface(&SurfaceTarget::Offscreen {
            width: WIDTH,
            height: HEIGHT,
        })
    }
    .expect("offscreen surface");

    let surface_caps: SurfaceCaps = instance
        .surface_caps(surface, adapters[0].id)
        .expect("surface caps");
    let present_format: Format = surface_caps.preferred_format().expect("a surface format");
    assert!(
        present_format.is_srgb(),
        "the tonemap pass writes display-referred values into an sRGB swapchain"
    );
    let present_mode: PresentMode =
        surface_caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]);

    let swapchain: SwapchainHandle = device
        .create_swapchain(&SwapchainDesc {
            label: Some("main"),
            surface,
            format: present_format,
            extent: (WIDTH, HEIGHT),
            image_count: 2,
            present_mode,
            composite_alpha: surface_caps.composite_alpha[0],
        })
        .expect("swapchain");

    // HDR offscreen target — RGBA16F from P1, per the roadmap.
    let color: ImageHandle = device
        .create_image(&ImageDesc {
            label: Some("hdr"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(WIDTH, HEIGHT),
            format: Format::Rgba16Float,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("hdr target");
    assert!(Format::Rgba16Float.is_hdr_capable());

    let color_view: ImageViewHandle = device
        .create_image_view(&ImageViewDesc {
            label: Some("hdr view"),
            image: color,
            view_type: ImageViewType::D2,
            format: Format::Rgba16Float,
            range: ImageSubresourceRange::all(Format::Rgba16Float),
        })
        .expect("hdr view");

    let depth_image: ImageHandle = device
        .create_image(&ImageDesc {
            label: Some("depth"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(WIDTH, HEIGHT),
            format: Format::D32Float,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("depth target");
    let depth_view: ImageViewHandle = device
        .create_image_view(&ImageViewDesc {
            label: Some("depth view"),
            image: depth_image,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: ImageSubresourceRange::all(Format::D32Float),
        })
        .expect("depth view");

    let draw_args: BufferHandle = device
        .create_buffer(&BufferDesc {
            label: Some("draw args"),
            size: 4096,
            usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("draw args");
    let draw_count: BufferHandle = device
        .create_buffer(&BufferDesc {
            label: Some("draw count"),
            size: 4,
            usage: BufferUsage::INDIRECT | BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("draw count");

    let module: ShaderModuleHandle = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("opaque"),
            spirv: &SPIRV,
        })
        .expect("shader module");
    let layout = device
        .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
            label: Some("opaque"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("pipeline layout");
    let pipeline: GraphicsPipelineHandle = device
        .create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("opaque"),
            layout,
            vertex: ShaderEntry {
                module,
                entry_point: "vs_main",
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fs_main",
            }),
            primitive: PrimitiveState::default(),
            // Reversed-Z by default; the test below checks the compare op that
            // actually reached the backend.
            depth_stencil: Some(DepthStencilState::default()),
            multisample: MultisampleState::default(),
            color_targets: &[ColorTargetState::opaque(Format::Rgba16Float)],
        })
        .expect("graphics pipeline");
    assert_eq!(
        DepthStencilState::default().depth_compare,
        crcbl_hal::CompareOp::Greater
    );

    Frame {
        device,
        queue,
        surface,
        swapchain,
        color,
        color_view,
        depth_view,
        draw_args,
        draw_count,
        pipeline,
        tier,
    }
}

/// Records and submits one frame. Identical for both tiers except the draw
/// tail, which is exactly the split topic 03 specifies.
fn render(frame: &Frame) {
    let device: &dyn Device = frame.device.as_ref();
    let acquired = device
        .acquire_next_frame(frame.swapchain)
        .expect("acquire never fails on an offscreen swapchain");

    let mut encoder: Box<dyn CommandEncoder> = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("frame"),
        queue: frame.queue,
    });

    encoder.begin_debug_label("frame");
    encoder.fill_buffer(frame.draw_count, 0, 4, 0);
    encoder.pipeline_barrier(&Barriers {
        buffers: &[
            BufferBarrier::new(
                frame.draw_args,
                crcbl_hal::ResourceState::ShaderWrite,
                crcbl_hal::ResourceState::IndirectArgument,
            ),
            BufferBarrier::new(
                frame.draw_count,
                crcbl_hal::ResourceState::TransferDst,
                crcbl_hal::ResourceState::IndirectArgument,
            ),
        ],
        images: &[crcbl_hal::ImageBarrier::new(
            frame.color,
            ImageSubresourceRange::all(Format::Rgba16Float),
            crcbl_hal::ResourceState::Undefined,
            crcbl_hal::ResourceState::ColorAttachment,
        )],
        global: false,
    });

    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("opaque"),
        color_attachments: &[ColorAttachment {
            view: frame.color_view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color([0.0, 0.0, 0.0, 1.0]),
        }],
        depth_stencil_attachment: Some(DepthStencilAttachment {
            view: frame.depth_view,
            depth_load: LoadOp::Clear,
            depth_store: StoreOp::Store,
            stencil_load: LoadOp::DontCare,
            stencil_store: StoreOp::Discard,
            clear: ClearValue::default(),
        }),
        render_area: Rect2d::from_size(WIDTH, HEIGHT),
    });
    encoder.set_viewport(&Viewport::from_size(WIDTH, HEIGHT));
    encoder.set_scissor(&Rect2d::from_size(WIDTH, HEIGHT));
    encoder.bind_graphics_pipeline(frame.pipeline);

    // The one place the tiers differ.
    if device.caps().supports(Features::DRAW_INDIRECT_COUNT) {
        encoder.draw_indexed_indirect_count(&DrawIndirectCount {
            args: frame.draw_args,
            args_offset: 0,
            count_buffer: frame.draw_count,
            count_offset: 0,
            max_draw_count: 1024,
            stride: 20,
        });
    } else {
        // Tier B: one indirect call per bucket, count fixed on the CPU.
        encoder.draw_indexed_indirect(&DrawIndirect {
            args: frame.draw_args,
            offset: 0,
            draw_count: 1,
            stride: 20,
        });
    }

    encoder.end_render_pass();
    encoder.end_debug_label();

    let command_buffer: CommandBufferHandle = encoder.finish().expect("finish");

    // The acquire/present semaphores splice in without a tier branch — `None`
    // on an implicit-acquire backend simply contributes nothing.
    let waits: Vec<_> = acquired
        .acquire_semaphore
        .into_iter()
        .map(|semaphore| crcbl_hal::SemaphoreWait {
            semaphore,
            value: 0,
        })
        .collect();
    let signals: Vec<_> = acquired
        .present_semaphore
        .into_iter()
        .map(|semaphore| crcbl_hal::SemaphoreSignal {
            semaphore,
            value: 0,
        })
        .collect();
    device
        .submit(
            frame.queue,
            &SubmitInfo {
                command_buffers: &[command_buffer],
                waits: &waits,
                signals: &signals,
            },
        )
        .expect("submit");

    let present_waits: Vec<_> = acquired.present_semaphore.into_iter().collect();
    device
        .present(
            frame.queue,
            &PresentInfo {
                swapchain: frame.swapchain,
                waits: &present_waits,
            },
        )
        .expect("present");

    device.destroy_command_buffer(command_buffer);
}

/// The whole point: one frame function, two tiers, no `#[cfg]` and no backend
/// type in sight.
#[test]
fn the_same_frame_runs_on_both_tiers_through_trait_objects() {
    for (preset, expected_tier) in [
        (NullInstance::tier_a(), RendererTier::A),
        (NullInstance::tier_b(), RendererTier::B),
    ] {
        let recorder = Recorder::new();
        let instance: Box<dyn Instance> = Box::new(preset.with_recorder(recorder.clone()));

        let frame = setup(instance.as_ref());
        assert_eq!(frame.tier, expected_tier);

        recorder.clear();
        render(&frame);
        recorder.assert_valid();

        let mut expected = vec![
            "BeginDebugLabel",
            "FillBuffer",
            "Barrier",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "BindGraphicsPipeline",
        ];
        expected.push(if expected_tier.is_a() {
            "DrawIndexedIndirectCount"
        } else {
            "DrawIndexedIndirect"
        });
        expected.extend(["EndRenderPass", "EndDebugLabel"]);
        assert_eq!(recorder.command_names(), expected, "tier {expected_tier:?}");
        assert_eq!(recorder.pass_labels(), vec!["opaque".to_string()]);

        // The depth attachment reached the backend cleared to the reversed-Z
        // far plane, not to the conventional-Z 1.0.
        let cleared = recorder
            .commands()
            .into_iter()
            .find_map(|command| match command {
                Command::BeginRenderPass {
                    depth_stencil_attachment: Some(attachment),
                    ..
                } => Some(attachment.clear.depth),
                _ => None,
            })
            .expect("the opaque pass has a depth attachment");
        assert_eq!(cleared, depth::CLEAR);
        assert_eq!(cleared, 0.0);

        // Tearing down releases everything the frame created.
        frame.device.destroy_swapchain(frame.swapchain);
        instance.destroy_surface(frame.surface);
    }
}

/// Generic use still works: the traits are `?Sized`-friendly, so the same
/// function accepts `&dyn Device` and a concrete backend alike.
#[test]
fn the_seam_is_also_usable_generically() {
    fn tier_of<D: Device + ?Sized>(device: &D) -> RendererTier {
        device.caps().tier()
    }

    let instance: Box<dyn Instance> = Box::new(NullInstance::tier_a());
    let adapters = instance.adapters();
    let device: Box<dyn Device> = instance
        .create_device(&DeviceDesc::for_adapter(adapters[0].id))
        .expect("device");

    // Through the trait object …
    assert_eq!(tier_of(device.as_ref()), RendererTier::A);
    // … and monomorphised over the boxed type.
    assert_eq!(tier_of(&*device), RendererTier::A);
}

/// A frame that creates transients and destroys them must leave the backend
/// exactly as it found it. This is the leak assertion the graph's transient
/// pool will lean on at P1.
#[test]
fn a_full_setup_and_teardown_leaks_nothing() {
    let recorder = Recorder::new();
    let instance: Box<dyn Instance> =
        Box::new(NullInstance::tier_a().with_recorder(recorder.clone()));
    assert_eq!(recorder.total_live_objects(), 0);

    {
        let device: Box<dyn Device> = instance
            .create_device(&DeviceDesc::for_adapter(instance.adapters()[0].id))
            .expect("device");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("scratch"),
                size: 256,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("buffer");
        let image = device
            .create_image(&ImageDesc {
                label: Some("scratch"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(16, 16),
                format: Format::Rgba8Unorm,
                mip_levels: Extent3d::d2(16, 16).full_mip_levels(),
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("image");
        assert_eq!(recorder.live_objects(ObjectKind::Buffer), 1);
        assert_eq!(recorder.live_objects(ObjectKind::Image), 1);

        device.destroy_buffer(buffer);
        device.destroy_image(image);
    }

    assert_eq!(
        recorder.total_live_objects(),
        0,
        "every object created above was destroyed"
    );
}

/// Errors are usable from outside without naming a backend type, and they name
/// the specific gap rather than merely failing.
#[test]
fn errors_cross_the_seam_intact() {
    let instance: Box<dyn Instance> = Box::new(NullInstance::tier_b());
    let error: HalError = instance
        .create_device(&DeviceDesc::for_adapter(instance.adapters()[0].id))
        .expect_err("tier B cannot satisfy TIER_A");
    let text = error.to_string();
    assert!(text.contains("DESCRIPTOR_INDEXING"), "{text}");
    assert!(matches!(error, HalError::UnsupportedFeatures { .. }));
}

/// The `crcbl screenshot` shape, written the way topic 11's CLI and the P1
/// golden-image e2e will write it: render, copy to a readable buffer, then
/// **poll with a deadline** rather than block.
///
/// This exact loop is what has to work in a browser, where a blocking read is
/// impossible. Nothing here is native-only, and nothing here names a backend
/// type.
#[test]
fn the_screenshot_readback_path_polls_instead_of_blocking() {
    let recorder = Recorder::new();
    let instance: Box<dyn Instance> =
        Box::new(NullInstance::tier_b().with_recorder(recorder.clone()));
    let adapters = instance.adapters();
    let device: Box<dyn Device> = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(adapters[0].id)
        })
        .expect("tier B device");
    let queue = device.queue(QueueFamily::Graphics).expect("queue");

    let pixels = WIDTH as u64 * HEIGHT as u64 * u64::from(Format::Rgba8Unorm.block_size());
    let staging: BufferHandle = device
        .create_buffer(&BufferDesc {
            label: Some("screenshot staging"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("staging buffer");
    let source: ImageHandle = device
        .create_image(&ImageDesc {
            label: Some("screenshot source"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(WIDTH, HEIGHT),
            format: Format::Rgba8Unorm,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("source image");

    // Record and submit the copy.
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("screenshot"),
        queue,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[],
        images: &[crcbl_hal::ImageBarrier::new(
            source,
            ImageSubresourceRange::all(Format::Rgba8Unorm),
            crcbl_hal::ResourceState::ColorAttachment,
            crcbl_hal::ResourceState::TransferSrc,
        )],
        global: false,
    });
    encoder.copy_image_to_buffer(&crcbl_hal::BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: source,
        image_subresource: crcbl_hal::ImageSubresourceLayers {
            aspect: crcbl_hal::ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(WIDTH, HEIGHT),
    });
    let command_buffer = encoder.finish().expect("finish");
    device
        .submit(queue, &SubmitInfo::new(&[command_buffer]))
        .expect("submit");

    // Make the backend behave like a browser: the map does not resolve on the
    // first poll.
    recorder.set_readback_latency(2);

    let request: ReadbackHandle = device
        .request_readback(&ReadbackDesc {
            label: Some("screenshot"),
            buffer: staging,
            offset: 0,
            size: pixels,
            after: None,
        })
        .expect("request readback");

    // The poll loop. Bounded by an iteration deadline rather than a sleep, per
    // `docs/plan/12-testing.md`'s frame-poll discipline.
    let mut image = vec![0u8; pixels as usize];
    let mut polls = 0;
    let mut state = ReadbackState::Pending;
    while polls < 16 {
        polls += 1;
        state = device
            .poll_readback(request, &mut image)
            .expect("poll readback");
        if state.is_ready() {
            break;
        }
        // A real caller renders the next frame here; the point is that it can.
    }
    assert_eq!(state, ReadbackState::Ready, "readback never completed");
    assert_eq!(polls, 3, "two pending polls, then the ready one");
    assert_eq!(image.len(), pixels as usize);
    assert!(image.iter().all(|byte| *byte == 0));

    device.destroy_readback(request);
    device.destroy_buffer(staging);
    device.destroy_image(source);
    device.destroy_command_buffer(command_buffer);
    assert_eq!(recorder.total_live_objects(), 0, "nothing leaked");
}

/// A device keeps working after the `Instance` that made it is dropped —
/// [`crate::device`]'s rule 1, checked from outside the crate because that is
/// where the `'static` in `Box<dyn Device>` is actually relied upon.
#[test]
fn a_device_outlives_the_instance_that_made_it() {
    let device: Box<dyn Device> = {
        let instance: Box<dyn Instance> = Box::new(NullInstance::tier_a());
        let adapters = instance.adapters();
        instance
            .create_device(&DeviceDesc::for_adapter(adapters[0].id))
            .expect("device")
    };
    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 64,
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("the device is still usable");
    device.destroy_buffer(buffer);
}
