//! What a steady-state frame of the GPU-driven renderer asks of the device,
//! counted on the null backend's recorder: stage 3's exit criteria of no
//! per-frame descriptor writes and no frame-loop readbacks but the cull-stats
//! ring's.

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#![cfg(not(target_arch = "wasm32"))]

use crcbl_hal::null::{Event, NullInstance, ObjectKind, Recorder};
use crcbl_hal::{
    CommandEncoderDesc, Device, DeviceDesc, Format, ImageUsage, Instance, QueueKind, ResourceState,
    SubmitInfo,
};
use crcbl_render::graph::{ImportedImage, InitialClaim, RenderGraph};
use crcbl_render::transient::TransientPool;
use crcbl_render::{Camera, DirectionalLight, ForwardRenderer, InstanceDesc, Projection};
use glam::{Mat4, Vec3};

const EXTENT: (u32, u32) = (256, 192);
/// The instance count stage 3's criteria name.
const INSTANCES: u32 = 10_000;
/// Frames drawn before counting: every frame in flight through once, and the
/// transients settled.
const WARMUP: usize = crcbl_render::forward::FRAMES_IN_FLIGHT + 4;
/// Frames counted.
const COUNTED: usize = 60;

/// What the device has been asked for so far that the criteria count.
#[derive(Debug)]
struct Counts {
    bind_groups_created: usize,
    bind_group_updates: u64,
    /// The label of the buffer each readback read, in order.
    readbacks: Vec<Option<String>>,
}

fn counts(recorder: &Recorder) -> Counts {
    let events = recorder.events();
    Counts {
        bind_groups_created: events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::Created {
                        kind: ObjectKind::BindGroup,
                        ..
                    }
                )
            })
            .count(),
        bind_group_updates: recorder.bind_group_updates(),
        readbacks: events
            .iter()
            .filter_map(|event| match event {
                Event::ReadbackRequested { buffer, .. } => Some(recorder.buffer_label(*buffer)),
                _ => None,
            })
            .collect(),
    }
}

/// **A steady-state frame of 10 000 instances creates no bind group, writes no
/// descriptor and reads back one buffer, the cull-stats ring's.** Stage 3's
/// exit criteria for the GPU-driven path, counted after a warm-up rather than
/// argued from the design.
#[test]
fn a_steady_state_frame_writes_no_descriptors_and_reads_back_only_the_cull_stats() {
    let recorder = Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("steady state"),
            adapter: adapter.id,
            required_features: crcbl_hal::Features::GPU_DRIVEN,
            optional_features: crcbl_hal::Features::empty(),
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let device: &dyn Device = device.as_ref();
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    let format = Format::Rgba8UnormSrgb;

    let mut scene = crcbl_render::scene::demo();
    scene.capacities.instances = INSTANCES;
    let mut renderer =
        ForwardRenderer::with_scene(device, queue, format, &scene).expect("the renderer builds");
    for index in 0..INSTANCES {
        let (x, z) = ((index % 100) as f32 - 50.0, (index / 100) as f32 - 50.0);
        renderer
            .add_instance(&InstanceDesc {
                mesh: crcbl_render::scene::DEMO_CUBE,
                material: crcbl_render::scene::DEMO_UNTINTED,
                transform: Mat4::from_translation(Vec3::new(x, 0.0, z)),
            })
            .expect("within the capacity");
    }

    let image = device
        .create_image(&crcbl_hal::ImageDesc {
            label: Some("fake swapchain image"),
            image_type: crcbl_hal::ImageType::D2,
            extent: crcbl_hal::Extent3d::d2(EXTENT.0, EXTENT.1),
            format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::PRESENT,
        })
        .expect("an image");
    let view = device
        .create_image_view(&crcbl_hal::ImageViewDesc {
            label: Some("fake swapchain view"),
            image,
            view_type: crcbl_hal::ImageViewType::D2,
            format,
            range: crcbl_hal::ImageSubresourceRange::all(format),
        })
        .expect("a view");
    let camera = Camera {
        eye: Vec3::new(0.0, 30.0, 60.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    };
    let mut pool = TransientPool::new();

    let frame = |renderer: &mut ForwardRenderer, pool: &mut TransientPool| {
        renderer
            .begin_frame(device, &camera, &DirectionalLight::default(), EXTENT)
            .expect("begin_frame");
        let mut graph = RenderGraph::new(queue);
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image,
                view,
                format,
                extent: EXTENT,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Acquired,
                final_state: ResourceState::Present,
            },
        );
        renderer.add_passes(&mut graph, pool, target, EXTENT);
        let compiled = graph.compile(pool).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("steady state"),
            queue,
        });
        compiled
            .execute(device, pool, encoder.as_mut(), None)
            .expect("the frame executes");
        let commands = encoder.finish().expect("recorded");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
    };
    for _ in 0..WARMUP {
        frame(&mut renderer, &mut pool);
    }
    let before = counts(&recorder);
    for _ in 0..COUNTED {
        frame(&mut renderer, &mut pool);
    }
    let after = counts(&recorder);

    assert_eq!(
        after.bind_groups_created - before.bind_groups_created,
        0,
        "a steady-state frame created bind groups"
    );
    assert_eq!(
        after.bind_group_updates - before.bind_group_updates,
        0,
        "a steady-state frame wrote descriptors into existing bind groups"
    );
    let read: Vec<_> = after.readbacks[before.readbacks.len()..].to_vec();
    assert_eq!(read.len(), COUNTED, "one readback a frame: {read:?}");
    assert!(
        read.iter()
            .all(|label| label.as_deref() == Some("cull stats readback")),
        "every frame-loop readback is the cull-stats ring's: {read:?}"
    );
}
