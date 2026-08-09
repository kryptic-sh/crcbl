use crate::harness::{CLEAR, Headless, instance};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferDesc, BufferUsage, ClearValue, ColorAttachment, CommandEncoderDesc,
    CompositeAlpha, DeviceDesc, Features, Format, ImageSubresourceRange, Instance, LoadOp,
    MemoryLocation, PresentInfo, PresentMode, Rect2d, RenderPassDesc, ResourceState, StoreOp,
    SubmitInfo, SurfaceError, SwapchainDesc,
};

/// Obligation 4: a zero extent is the caller's problem, and the error says so
/// rather than the backend guessing a size.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_zero_extent_swapchain_is_refused_with_a_reason() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    // SAFETY: `Offscreen` names no platform object.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen");
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("vk e2e"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect("a device opens");

    let error = device
        .create_swapchain(&SwapchainDesc {
            label: None,
            surface,
            format: Format::Rgba8UnormSrgb,
            extent: (0, 0),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect_err("a minimized window means 'not yet', not 'guess'");
    let SurfaceError::Hal(crcbl_hal::HalError::InvalidDescriptor(message)) = error else {
        panic!("wrong variant");
    };
    assert!(message.contains("do not create one yet"), "{message}");

    instance.destroy_surface(surface);
    drop(device);
    instance.validation_report().assert_clean();
}

/// Obligation 3: handles do not cross devices, and the failure is detected
/// rather than undefined.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_buffer_from_one_device_is_foreign_to_another() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let open = || match instance.create_device(&DeviceDesc::for_adapter(adapter.id)) {
        Ok(device) => device,
        Err(error) => {
            // `for_adapter` requires compute and a timeline semaphore, which a
            // driver may not have — but that is the *only* reason it may fail
            // here, and swallowing any error would let a genuine one hide
            // behind the fallback.
            assert!(
                matches!(error, crcbl_hal::HalError::UnsupportedFeatures { .. }),
                "the only permitted refusal of the headless default is a named \
                 feature gap: {error}"
            );
            instance
                .create_device(&DeviceDesc {
                    label: None,
                    adapter: adapter.id,
                    required_features: Features::empty(),
                    optional_features: Features::empty(),
                    compatible_surface: None,
                })
                .expect("a featureless headless device opens on any adapter")
        }
    };
    let first = open();
    let second = open();

    let buffer = first
        .create_buffer(&BufferDesc {
            label: Some("foreign"),
            size: 256,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a buffer");

    let error = second
        .write_buffer(buffer, 0, &[1, 2, 3, 4])
        .expect_err("a buffer from another device must be refused");
    // `ForeignObject` specifically, not "either error will do". Every object
    // table is per-device, so this used to be answered by the handle happening
    // to miss the second device's pool — which stops being true the moment two
    // devices allocate in step, and at that point device A accepted device B's
    // handle outright. A handle now carries the tag of the device that issued
    // it, so the answer is decided rather than lucky.
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::ForeignObject { kind: "buffer", .. }
        ),
        "a live handle from another device is foreign, not merely unresolvable: {error}"
    );

    first.destroy_buffer(buffer);
    drop(second);
    drop(first);
    instance.validation_report().assert_clean();
}

/// Obligation 1: a `Device` may outlive its `Instance`. Getting this wrong is a
/// use-after-free inside the driver, which is exactly the class of bug the
/// validation layer reports — so the report is the assertion.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_device_outlives_the_instance_that_made_it() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("outlives"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("a headless device opens");

    // A clone so the report survives the instance object being dropped; the
    // *underlying* `VkInstance` must stay alive because the device does.
    let report_source = instance.clone();
    drop(instance);

    // The device still works, which is the property under test.
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("after the instance went away"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the device outlived its instance");
    device.write_buffer(buffer, 0, &[7; 64]).expect("write");
    device.destroy_buffer(buffer);
    device.wait_idle().expect("idle");
    drop(device);

    report_source.validation_report().assert_clean();
}

/// **Obligation 2b**, as far as a headless run can take it: `destroy_surface`
/// invalidates the handle *immediately*, and everything already built on that
/// surface goes on working.
///
/// The half that cannot be reached from here is the one whose failure mode is
/// undefined behaviour: an offscreen "surface" is `VK_NULL_HANDLE`, so there is
/// no `vkDestroySurfaceKHR` to defer and the zombie list never engages. That
/// bookkeeping is asserted directly in `instance.rs`
/// (`a_surface_with_a_live_swapchain_defers_its_driver_object`), because the
/// only surfaces this suite can create are the ones it cannot exercise.
///
/// What this *does* prove against a real driver: a frame rendered and presented
/// through a swapchain whose surface handle is already gone raises nothing from
/// the validation layer, which is the observable the deferral exists to keep.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_swapchain_keeps_working_after_its_surface_handle_is_destroyed() {
    let headless = Headless::open();
    let device = &headless.device;

    // The handle dies here, mid-life of the swapchain built on it.
    headless.instance.destroy_surface(headless.surface);
    let error = headless
        .instance
        .surface_caps(headless.surface, crcbl_hal::AdapterId(0))
        .expect_err("the handle is invalid the moment the caller destroys it");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidHandle { .. }),
        "a destroyed surface handle is stale, not foreign: {error}"
    );

    // And the swapchain on it still renders a whole frame.
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring outlives the surface handle");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("after the surface handle went away"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            ImageSubresourceRange::all(headless.format),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("clear"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
    });
    encoder.end_render_pass();
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
    device.destroy_command_buffer(commands);

    // Teardown in the order the deferral is for: the swapchain last, long after
    // the surface handle. `Headless::finish` cannot be used because it destroys
    // the surface, which this test already did.
    device.destroy_swapchain(headless.swapchain);
    drop(headless.device);
    headless.instance.validation_report().assert_clean();
}

/// **Obligation 3, across two instances.** Handle bits are only unique within
/// the backend that issued them, so two `VkInstance`s genuinely hand out
/// identical ones — a surface crossing them must be detected, and detected as
/// `ForeignObject` rather than as a stale handle, because the two send a reader
/// to different bugs.
///
/// `crcbl-hal`'s null backend covers this; the reference backend did not, and
/// no vk test opened two instances at all.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_surface_from_one_instance_is_foreign_to_another() {
    let owner = instance();
    let other = instance();

    // SAFETY: `Offscreen` names no platform object at all.
    let surface =
        unsafe { owner.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen always works");
    let adapter = other.adapters().remove(0);

    let error = other
        .surface_caps(surface, adapter.id)
        .expect_err("this surface belongs to the other instance");
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "a live handle from another instance is foreign, not unresolvable: {error}"
    );

    let error = other
        .create_device(&DeviceDesc {
            label: Some("foreign surface"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect_err("and the same on the device-creation path");
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "{error}"
    );

    // The other instance must not be able to free it either, which is what
    // stops a stray `destroy_surface` from turning a bit collision into a
    // double free.
    other.destroy_surface(surface);
    owner
        .surface_caps(surface, owner.adapters().remove(0).id)
        .expect("the owning instance still has its surface");

    owner.destroy_surface(surface);
    owner.validation_report().assert_clean();
    other.validation_report().assert_clean();
}
