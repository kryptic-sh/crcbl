//! `crcbl-hal`'s numbered seam obligations, against the reference backend.
//!
//! The obligations are stated in `crcbl-hal` and covered there against the null
//! backend, which can model any device it likes. This module is the other half:
//! obligations 1 and 2b against a real driver, where breaking one is a
//! use-after-free inside the driver rather than a failed assertion — so both of
//! these tests assert the validation report and nothing else can.
//!
//! # Why only two obligations are left here
//!
//! Obligations 3 and 4 — a handle crossing devices, a surface crossing
//! instances, a zero extent — are **return-value** claims, identical on every
//! backend, and they had a hand-maintained copy in `crcbl-vk`, in `crcbl-wgpu`
//! and inside `crcbl-mtl`'s and `crcbl-dx12`'s own `src/`. They now live once,
//! in `crates/crcbl/tests/hal_seam_e2e.rs`, which runs on whichever backend
//! `CRCBL_GPU` names — so `CRCBL_GPU=vk crates/crcbl/tests/run-hal-seam-e2e.sh`
//! is where Vulkan is held to them.
//!
//! What could not go with them is what only Vulkan can say. Both tests below
//! rest on `ValidationReport::assert_clean`: obligation 1's failure mode is a
//! `VkInstance` destroyed under a live `VkDevice`, and 2b's is a
//! `VkSurfaceKHR` destroyed under a live `VkSwapchainKHR`. Neither produces a
//! failed return value on any driver — the layer is the only witness, and there
//! is no cross-backend equivalent of it.
//!
//! This module is also explicit about the half it cannot reach. An offscreen
//! surface is `VK_NULL_HANDLE`, so there is no `vkDestroySurfaceKHR` to defer
//! and 2b's zombie list never engages here; that bookkeeping is asserted in
//! `instance.rs` instead, because the only surfaces this suite can create are
//! the ones it cannot exercise.

use crate::harness::{CLEAR, Headless, instance};
use crcbl_hal::{
    Barriers, BufferDesc, BufferUsage, ClearValue, ColorAttachment, CommandEncoderDesc, DeviceDesc,
    Features, ImageSubresourceRange, Instance, LoadOp, MemoryLocation, PresentInfo, Rect2d,
    RenderPassDesc, ResourceState, StoreOp, SubmitInfo,
};

/// Obligation 1: a `Device` may outlive its `Instance`. Getting this wrong is a
/// use-after-free inside the driver, which is exactly the class of bug the
/// validation layer reports — so the report is the assertion.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_vulkan_device_outlives_the_instance_that_made_it() {
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
fn a_vulkan_swapchain_keeps_working_after_its_surface_handle_is_destroyed() {
    let mut headless = Headless::open();
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
        timestamp_writes: None,
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
    headless.device.destroy();
    headless.instance.validation_report().assert_clean();
}
