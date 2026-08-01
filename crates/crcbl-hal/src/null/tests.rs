//! Unit tests for the null backend.
//!
//! The "is the seam usable from outside the crate" property is checked by
//! `tests/seam_from_outside.rs` instead — an in-crate test can reach private
//! items and so cannot prove anything about leakage.

use super::*;

use crate::{
    BindGroupLayoutEntry, BindingKind, BufferUsage, ClearValue, ColorAttachment, ColorTargetState,
    DepthStencilState, ImageSubresourceRange, ImageViewType, LoadOp, MultisampleState,
    PrimitiveState, PushConstantRange, RendererTier, ResourceState, ShaderEntry, StoreOp, depth,
};

/// The SPIR-V magic number, so test modules look like modules.
const SPIRV: [u32; 5] = [0x0723_0203, 0x0001_0600, 0, 0, 0];

fn boxed(instance: NullInstance) -> (Recorder, Box<dyn Instance>) {
    let recorder = instance.recorder();
    (recorder, Box::new(instance))
}

/// Opens a device that enables everything the adapter offers.
///
/// `DeviceDesc::for_adapter` alone enables only `TIER_A`; optional features must
/// be *asked* for, which is the behaviour a real backend has and the reason this
/// helper spells it out.
fn open(instance: &dyn Instance) -> Box<dyn Device> {
    let adapters = instance.adapters();
    instance
        .create_device(&DeviceDesc {
            optional_features: Features::all(),
            ..DeviceDesc::for_adapter(adapters[0].id)
        })
        .expect("tier A adapter satisfies TIER_A")
}

#[test]
fn tier_presets_report_the_documented_tiers() {
    let a = NullInstance::tier_a();
    let b = NullInstance::tier_b();
    assert_eq!(a.adapters()[0].caps.tier(), RendererTier::A);
    assert_eq!(b.adapters()[0].caps.tier(), RendererTier::B);
    assert_eq!(a.backend(), BackendKind::Null);

    let caps = b.adapters()[0].caps;
    for absent in [
        Features::DESCRIPTOR_INDEXING,
        Features::BUFFER_DEVICE_ADDRESS,
        Features::DRAW_INDIRECT_COUNT,
        Features::MULTI_DRAW_INDIRECT,
        Features::PUSH_CONSTANTS,
        Features::TIMELINE_SEMAPHORE,
    ] {
        assert!(
            !caps.supports(absent),
            "the tier B preset must not claim {absent:?} — it models WebGPU"
        );
    }
    assert!(
        caps.supports(Features::COMPUTE),
        "GPU culling runs on tier B"
    );
}

#[test]
fn a_tier_b_device_refuses_a_tier_a_request_and_names_the_gap() {
    let instance = NullInstance::tier_b();
    let error = instance
        .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
        .expect_err("tier B cannot satisfy TIER_A");
    let HalError::UnsupportedFeatures { missing } = error else {
        panic!("expected UnsupportedFeatures, got {error:?}");
    };
    assert!(missing.contains(Features::DESCRIPTOR_INDEXING));
    assert!(missing.contains(Features::BUFFER_DEVICE_ADDRESS));
    assert!(!missing.contains(Features::COMPUTE), "tier B has compute");
}

#[test]
fn unknown_adapters_are_rejected() {
    let instance = NullInstance::tier_a();
    let error = instance
        .create_device(&DeviceDesc {
            adapter: AdapterId(7),
            ..DeviceDesc::for_adapter(AdapterId(7))
        })
        .expect_err("there is one adapter");
    assert!(matches!(error, HalError::NoSuchAdapter(7)), "{error:?}");
}

#[test]
fn queue_families_follow_the_adapter_features() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    assert!(device.queue(QueueKind::Graphics).is_some());
    assert!(device.queue(QueueKind::Compute).is_some());
    assert!(device.queue(QueueKind::Transfer).is_some());
    assert_ne!(
        device.queue(QueueKind::Graphics),
        device.queue(QueueKind::Compute),
        "distinct families must have distinct handles"
    );

    let instance = NullInstance::tier_b();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("tier B has compute");
    assert!(
        device.queue(QueueKind::Graphics).is_some(),
        "the graphics queue always exists"
    );
    assert!(device.queue(QueueKind::Compute).is_none());
    assert!(device.queue(QueueKind::Transfer).is_none());
}

#[test]
fn buffers_round_trip_through_the_host_and_reject_the_wrong_memory() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("staging"),
            size: 16,
            usage: crate::BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("staging buffer");
    device
        .write_buffer(upload, 4, &[1, 2, 3, 4])
        .expect("write");

    // Out-of-range writes are caught, not silently truncated.
    let error = device
        .write_buffer(upload, 14, &[0; 8])
        .expect_err("write past the end");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // Device-local memory is not host-writable.
    let device_local = device
        .create_buffer(&BufferDesc {
            label: Some("vertex pool"),
            size: 16,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("device local buffer");
    let error = device
        .write_buffer(device_local, 0, &[0; 4])
        .expect_err("device-local memory is not mappable");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // A zero-size buffer is a descriptor bug.
    let error = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 0,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect_err("zero size");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
}

#[test]
fn destroyed_handles_are_rejected_rather_than_aliased() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 8,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("buffer");
    device.destroy_buffer(buffer);

    let error = device
        .write_buffer(buffer, 0, &[0])
        .expect_err("stale handle");
    let HalError::InvalidHandle { kind, bits } = error else {
        panic!("expected InvalidHandle, got {error:?}");
    };
    assert_eq!(kind, "buffer");
    assert_eq!(bits, buffer.to_bits());

    // The slot is recycled with a new generation, so the old handle stays dead
    // even though a new buffer now occupies the same index.
    let fresh = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 8,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("buffer");
    assert_eq!(fresh.index(), buffer.index());
    assert_ne!(fresh.to_bits(), buffer.to_bits());
    assert!(device.write_buffer(buffer, 0, &[0]).is_err());
    assert!(device.write_buffer(fresh, 0, &[0]).is_ok());
}

#[test]
fn shader_modules_reject_non_spirv() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: None,
            spirv: &[0xDEAD_BEEF],
            wgsl: None,
        })
        .expect_err("not SPIR-V");
    assert!(matches!(error, HalError::ShaderCompilation(_)), "{error:?}");
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: None,
                spirv: &[],
                wgsl: None,
            })
            .is_err()
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("cull"),
                spirv: &SPIRV,
                wgsl: None,
            })
            .is_ok()
    );
}

/// The null backend compiles nothing, so it accepts either artifact format on
/// its own — including WGSL with no SPIR-V beside it, which is a legal
/// descriptor the SPIR-V magic-number check must not reject.
#[test]
fn shader_modules_accept_wgsl_with_or_without_spirv() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("wgsl only"),
                spirv: &[],
                wgsl: Some("@fragment fn main() {}"),
            })
            .is_ok()
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("both"),
                spirv: &SPIRV,
                wgsl: Some("@fragment fn main() {}"),
            })
            .is_ok()
    );
}

/// A descriptor carrying no artifact at all names the gap rather than producing
/// a module handle no pipeline could use.
#[test]
fn a_shader_module_with_no_artifact_names_the_gap() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("empty.slang"),
            spirv: &[],
            wgsl: None,
        })
        .expect_err("a descriptor with no artifact is not a shader");
    let text = error.to_string();
    assert!(text.contains("empty.slang"), "{text}");
    assert!(text.contains("was given nothing"), "{text}");
}

/// What each call site supplied survives the module's destruction, which is
/// what makes "did this caller offer every format it has" assertable at all —
/// every real caller destroys its modules the moment its pipelines exist.
#[test]
fn created_shader_modules_are_logged_with_the_formats_they_carried() {
    let instance = NullInstance::tier_a();
    let recorder = instance.recorder();
    let device = open(&instance);
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("mesh.slang"),
            spirv: &SPIRV,
            wgsl: Some("@vertex fn vertexMain() {}"),
        })
        .expect("both formats");
    device.destroy_shader_module(module);
    assert_eq!(
        recorder.shader_modules_created(),
        vec![(
            Some("mesh.slang".to_string()),
            ShaderSources::SPIRV | ShaderSources::WGSL
        )]
    );
    recorder.clear();
    assert!(recorder.shader_modules_created().is_empty());
}

/// The Tier B trap the seam exists to make visible: a layout that asks for
/// push constants on a device without them fails at *creation*, not by silently
/// dropping the writes at record time.
#[test]
fn push_constants_and_bindless_fail_loudly_on_tier_b() {
    let instance = NullInstance::tier_b();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("tier B device");

    let layout = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("frame"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::ALL,
                kind: BindingKind::UniformBuffer { dynamic: true },
                count: 1,
                flags: crate::BindingFlags::empty(),
            }],
        })
        .expect("a plain layout is fine on tier B");

    let error = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[layout],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::ALL,
                offset: 0,
                size: 64,
            }),
        })
        .expect_err("tier B has no push constants");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    // Bindless flags are refused for the same reason.
    let error = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("bindless textures"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage,
                count: 1 << 16,
                flags: crate::BindingFlags::PARTIALLY_BOUND
                    | crate::BindingFlags::UPDATE_AFTER_BIND
                    | crate::BindingFlags::VARIABLE_COUNT,
            }],
        })
        .expect_err("tier B has no descriptor indexing");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    // And so are timeline semaphores.
    let error = device
        .create_semaphore(&SemaphoreDesc {
            label: None,
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect_err("tier B has no timeline semaphores");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
}

/// Vulkan permits a runtime-sized array only on the **highest-numbered** binding
/// of a set, and both backends additionally require it to be the last element of
/// the slice so "the variable binding is `entries.last()`" is a true reading.
/// The seam doc now states both halves; this pins them.
#[test]
fn variable_count_must_be_the_last_and_highest_binding() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let plain = BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler,
        count: 4,
        flags: crate::BindingFlags::empty(),
    };
    let bindless = BindGroupLayoutEntry {
        binding: 1,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::SampledImage,
        count: 1 << 16,
        flags: crate::BindingFlags::VARIABLE_COUNT | crate::BindingFlags::PARTIALLY_BOUND,
    };
    let error = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[bindless, plain],
        })
        .expect_err("VARIABLE_COUNT must come last");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // Last in the slice but *not* the highest binding number: Vulkan refuses
    // this, and the null backend used to accept it.
    let error = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 7,
                    ..plain
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    ..bindless
                },
            ],
        })
        .expect_err("VARIABLE_COUNT must be the highest binding number");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    assert!(
        device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: None,
                entries: &[plain, bindless],
            })
            .is_ok()
    );
}

/// A duplicated binding number is a caller bug `crcbl-vk` names and the null
/// backend used to wave through.
#[test]
fn a_duplicated_binding_number_is_refused() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let entry = BindGroupLayoutEntry {
        binding: 3,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler,
        count: 1,
        flags: crate::BindingFlags::empty(),
    };
    let error = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[entry, entry],
        })
        .expect_err("binding 3 twice");
    assert!(error.to_string().contains("twice"), "{error}");
}

/// Tier A gets binary acquire/present semaphores; Tier B, modelling WebGPU's
/// implicit `getCurrentTexture`, gets `None` for both. The renderer's splice is
/// the same code in each case — see [`crate::swapchain`].
#[test]
fn swapchain_acquire_matches_the_tiers_sync_model() {
    for (instance, features, expect_semaphores) in [
        (NullInstance::tier_a(), Features::TIER_A, true),
        (NullInstance::tier_b(), Features::COMPUTE, false),
    ] {
        let device = instance
            .create_device(&DeviceDesc {
                required_features: features,
                ..DeviceDesc::for_adapter(AdapterId(0))
            })
            .expect("device");
        // SAFETY: the offscreen target holds no pointers, so the contract on
        // `create_surface` is vacuous for it.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen surface");

        let caps = instance
            .surface_caps(surface, AdapterId(0))
            .expect("surface caps");
        let format = caps.preferred_format().expect("a format");
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("main"),
                surface,
                format,
                extent: (320, 200),
                image_count: 3,
                present_mode: caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]),
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("swapchain");

        let first = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(first.index, 0);
        assert!(!first.suboptimal);
        assert_eq!(first.acquire_semaphore.is_some(), expect_semaphores);
        assert_eq!(first.present_semaphore.is_some(), expect_semaphores);
        // The image, the view and the extent are never optional: every backend
        // has all three, which is why they sit beside the semaphores instead of
        // being rebuilt by every caller.
        assert_eq!(first.extent, (320, 200));

        // The ring rotates and eventually wraps.
        let second = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(second.index, 1);
        assert_ne!(second.image, first.image);
        assert_ne!(
            second.view, first.view,
            "one view per ring image, not one shared view"
        );
        let third = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(third.index, 2);
        let wrapped = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(wrapped.index, 0);
        assert_eq!(wrapped.image, first.image);

        let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
        let waits: Vec<_> = wrapped.present_semaphore.into_iter().collect();
        device
            .present(
                queue,
                &PresentInfo {
                    swapchain,
                    waits: &waits,
                },
            )
            .expect("present");

        // Reconfiguring keeps the handle valid and restarts the ring.
        device
            .reconfigure_swapchain(
                swapchain,
                &SwapchainDesc {
                    label: Some("main"),
                    surface,
                    format,
                    extent: (640, 400),
                    image_count: 3,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("reconfigure");
        let after = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(after.index, 0);
        // Obligation 3: the frame reports the size it was configured at, and a
        // reconfigure shows up in it immediately.
        assert_eq!(after.extent, (640, 400));
        // A reconfigure reissues the images *and* their views, so anything held
        // across it is dead — the generational handle turning a stale reference
        // into a clean failure rather than an alias. `crcbl-vk` must do the
        // same, and this is the cheap place to find out that a caller cached
        // one.
        assert_ne!(after.image, first.image);
        assert_ne!(after.view, first.view);
        assert!(
            device
                .create_image_view(&ImageViewDesc {
                    label: None,
                    image: first.image,
                    view_type: ImageViewType::D2,
                    format,
                    range: ImageSubresourceRange::all(format),
                })
                .is_err(),
            "an image handle must not survive a reconfigure"
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }
}

/// Regression test. `reconfigure_swapchain` used to replace only the images and
/// their views, leaving the acquire/present semaphore vectors at their original
/// length — so growing the image count and acquiring past the old length
/// indexed off the end of them and panicked. The four vectors are one
/// invariant; `build_ring` is what keeps them so.
#[test]
fn reconfiguring_to_a_larger_ring_reissues_the_semaphores_too() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let desc = SwapchainDesc {
        label: None,
        surface,
        format: Format::Bgra8UnormSrgb,
        extent: (8, 8),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    let swapchain = device.create_swapchain(&desc).expect("swapchain");
    device
        .reconfigure_swapchain(
            swapchain,
            &SwapchainDesc {
                image_count: 3,
                ..desc
            },
        )
        .expect("reconfigure");

    // The third acquire is the one that used to panic.
    for expected in 0..3 {
        let frame = device.acquire_next_frame(swapchain).expect("acquire");
        assert_eq!(frame.index, expected);
        assert!(
            frame.acquire_semaphore.is_some() && frame.present_semaphore.is_some(),
            "a tier A ring has a semaphore pair per slot, for every slot"
        );
    }

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// Regression test. `reconfigure_swapchain` used to destroy the old ring before
/// building the new one, so a reconfigure that *failed* left the swapchain
/// naming destroyed images and views — a call that returned `Err` having broken
/// the object it was handed. The seam's promise is that a frame's image and
/// view are usable; a failed call must not be able to break it.
#[test]
fn a_failed_reconfigure_leaves_the_swapchain_usable() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let desc = SwapchainDesc {
        label: None,
        surface,
        format: Format::Bgra8UnormSrgb,
        extent: (8, 8),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    let swapchain = device.create_swapchain(&desc).expect("swapchain");
    let before = device.acquire_next_frame(swapchain).expect("acquire");

    // Past `max_image_2d`, so the ring cannot be built.
    let limit = device.caps().limits.max_image_2d;
    device
        .reconfigure_swapchain(
            swapchain,
            &SwapchainDesc {
                extent: (limit + 1, limit + 1),
                ..desc
            },
        )
        .expect_err("an oversized extent cannot be configured");

    // The old ring is untouched. The handle `before` named is still alive —
    // which is the whole property: the failed call destroyed nothing.
    let probe = device
        .create_image_view(&ImageViewDesc {
            label: None,
            image: before.image,
            view_type: ImageViewType::D2,
            format: Format::Bgra8UnormSrgb,
            range: ImageSubresourceRange::all(Format::Bgra8UnormSrgb),
        })
        .expect("the image the earlier frame named must still exist");
    device.destroy_image_view(probe);

    // And the ring still comes round to exactly the slot it did before, with
    // the same handles and the same extent, rather than to a reissued one.
    let mut wrapped = device.acquire_next_frame(swapchain).expect("acquire");
    while wrapped.index != before.index {
        wrapped = device.acquire_next_frame(swapchain).expect("acquire");
    }
    assert_eq!(
        (wrapped.image, wrapped.view, wrapped.extent),
        (before.image, before.view, before.extent),
        "a failed reconfigure must change nothing"
    );

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

#[test]
fn destroying_a_swapchain_releases_its_images_and_semaphores() {
    let instance = NullInstance::tier_a();
    let recorder = instance.recorder();
    let device = open(&instance);
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let before_images = recorder.live_objects(ObjectKind::Image);
    let before_views = recorder.live_objects(ObjectKind::ImageView);
    let before_semaphores = recorder.live_objects(ObjectKind::Semaphore);

    let swapchain = device
        .create_swapchain(&SwapchainDesc {
            label: None,
            surface,
            format: Format::Bgra8UnormSrgb,
            extent: (8, 8),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect("swapchain");
    assert_eq!(recorder.live_objects(ObjectKind::Image), before_images + 2);
    assert_eq!(
        recorder.live_objects(ObjectKind::ImageView),
        before_views + 2,
        "the swapchain owns a view per image, handed out on AcquiredFrame"
    );
    assert_eq!(
        recorder.live_objects(ObjectKind::Semaphore),
        before_semaphores + 4,
        "two images, each with an acquire and a present semaphore"
    );

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
    assert_eq!(recorder.live_objects(ObjectKind::Image), before_images);
    assert_eq!(recorder.live_objects(ObjectKind::ImageView), before_views);
    assert_eq!(
        recorder.live_objects(ObjectKind::Semaphore),
        before_semaphores
    );
    assert_eq!(recorder.total_live_objects(), 0, "nothing leaked");
}

#[test]
fn a_swapchain_needs_a_live_surface_and_a_real_extent() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let desc = SwapchainDesc {
        label: None,
        surface,
        format: Format::Bgra8UnormSrgb,
        extent: (0, 8),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    assert!(device.create_swapchain(&desc).is_err(), "zero extent");

    instance.destroy_surface(surface);
    let desc = SwapchainDesc {
        extent: (8, 8),
        ..desc
    };
    let error = device.create_swapchain(&desc).expect_err("dead surface");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidHandle { .. })),
        "{error:?}"
    );
}

/// Scope violations are recorded, not panicked on, so one run reports every
/// problem in a frame instead of only the first.
#[test]
fn scope_violations_are_recorded() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("bad frame"),
        queue,
    });
    // A draw with no render pass open.
    encoder.draw(0..3, 0..1);
    // A dispatch with no compute pass open.
    encoder.dispatch(1, 1, 1);
    // Closing a pass that was never opened.
    encoder.end_render_pass();
    // A pass left open at finish.
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("cull"),
    });
    // A copy inside a pass.
    encoder.fill_buffer(
        crcbl_core::Handle::from_bits(1 << 32).expect("non-zero generation"),
        0,
        4,
        0,
    );
    // An unclosed pass is the one violation that is *also* an `Err`: the seam's
    // `finish` doc names it and `crcbl-vk` returns one, because
    // `vkEndCommandBuffer` inside a rendering scope is itself illegal. Every
    // other violation above is recorded and carried past.
    let error = encoder
        .finish()
        .expect_err("a pass left open must fail the finish, not just be recorded");
    assert!(
        error.to_string().contains("still open"),
        "{error}: the error must name the unclosed pass"
    );

    let errors = recorder.validation_errors();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::OutsidePass {
                command: "Draw",
                ..
            }
        )),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::OutsidePass {
                command: "Dispatch",
                ..
            }
        )),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, ValidationError::UnopenedPass { .. })),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::InsidePass {
                command: "FillBuffer"
            }
        )),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, ValidationError::UnclosedPass)),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, ValidationError::DeadHandle { .. })),
        "a made-up buffer handle must be caught: {errors:?}"
    );
}

#[test]
fn nested_passes_are_rejected() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("outer"),
    });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("inner"),
    });
    encoder.end_compute_pass();
    let _ = encoder.finish().expect("finish");

    let errors = recorder.validation_errors();
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, ValidationError::NestedPass { .. })),
        "{errors:?}"
    );
    assert_eq!(
        recorder.pass_labels(),
        vec!["outer".to_string(), "inner".to_string()],
        "both passes are still recorded — the stream is complete even when invalid"
    );
}

/// A frame recorded exactly the way topic 03 describes: cull in compute, then
/// draw indirect with a GPU-side count, then present. This is the shape the
/// render graph's own suite will assert on at P1.
#[test]
fn a_gpu_driven_frame_records_the_expected_stream() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");

    // Resources.
    let instances = device
        .create_buffer(&BufferDesc {
            label: Some("instances"),
            size: 1 << 16,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("instances");
    let draw_args = device
        .create_buffer(&BufferDesc {
            label: Some("draw args"),
            size: 1 << 12,
            usage: crate::BufferUsage::INDIRECT | crate::BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("draw args");
    let draw_count = device
        .create_buffer(&BufferDesc {
            label: Some("draw count"),
            size: 4,
            usage: crate::BufferUsage::INDIRECT
                | crate::BufferUsage::STORAGE
                | crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("draw count");
    let indices = device
        .create_buffer(&BufferDesc {
            label: Some("index pool"),
            size: 1 << 16,
            usage: crate::BufferUsage::INDEX,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("indices");

    let color = device
        .create_image(&ImageDesc {
            label: Some("hdr"),
            image_type: ImageType::D2,
            extent: crate::Extent3d::d2(1920, 1080),
            format: Format::Rgba16Float,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("hdr target");
    let color_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("hdr view"),
            image: color,
            view_type: crate::ImageViewType::D2,
            format: Format::Rgba16Float,
            range: crate::ImageSubresourceRange::all(Format::Rgba16Float),
        })
        .expect("hdr view");
    let depth_image = device
        .create_image(&ImageDesc {
            label: Some("depth"),
            image_type: ImageType::D2,
            extent: crate::Extent3d::d2(1920, 1080),
            format: Format::D32Float,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("depth target");
    let depth_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("depth view"),
            image: depth_image,
            view_type: crate::ImageViewType::D2,
            format: Format::D32Float,
            range: crate::ImageSubresourceRange::all(Format::D32Float),
        })
        .expect("depth view");

    // Pipelines.
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("frame"),
            spirv: &SPIRV,
            wgsl: None,
        })
        .expect("module");
    let set_layout = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("frame set"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::ALL,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crate::BindingFlags::empty(),
            }],
        })
        .expect("set layout");
    let bind_group = device
        .create_bind_group(&BindGroupDesc {
            label: Some("frame"),
            layout: set_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crate::BindingResource::whole_buffer(instances),
            }],
            variable_count: None,
        })
        .expect("bind group");
    let pipeline_layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("frame"),
            bind_group_layouts: &[set_layout],
            push_constants: None,
        })
        .expect("pipeline layout");
    let cull = device
        .create_compute_pipeline(&ComputePipelineDesc {
            label: Some("cull"),
            layout: pipeline_layout,
            compute: ShaderEntry {
                module,
                entry_point: "cull_main",
            },
        })
        .expect("cull pipeline");
    let opaque = device
        .create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("opaque"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: "vs_main",
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fs_main",
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState::default()),
            multisample: MultisampleState::default(),
            color_targets: &[ColorTargetState::opaque(Format::Rgba16Float)],
        })
        .expect("opaque pipeline");

    let timers = device
        .create_query_set(&QuerySetDesc {
            label: Some("pass timers"),
            kind: QueryKind::Timestamp,
            count: 4,
        })
        .expect("timers");

    recorder.clear();

    // The frame itself.
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("frame"),
        queue,
    });
    encoder.reset_query_set(timers, 0..4);
    encoder.write_timestamp(timers, 0);
    encoder.fill_buffer(draw_count, 0, 4, 0);
    encoder.pipeline_barrier(&Barriers {
        buffers: &[crate::BufferBarrier::new(
            draw_count,
            ResourceState::TransferDst,
            ResourceState::ShaderReadWrite,
        )],
        images: &[],
        global: false,
    });

    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("cull"),
    });
    encoder.bind_compute_pipeline(cull);
    encoder.bind_group(0, bind_group, &[], pipeline_layout);
    encoder.dispatch(64, 1, 1);
    encoder.end_compute_pass();

    encoder.pipeline_barrier(&Barriers {
        buffers: &[
            crate::BufferBarrier::new(
                draw_args,
                ResourceState::ShaderWrite,
                ResourceState::IndirectArgument,
            ),
            crate::BufferBarrier::new(
                draw_count,
                ResourceState::ShaderReadWrite,
                ResourceState::IndirectArgument,
            ),
        ],
        images: &[crate::ImageBarrier::new(
            color,
            crate::ImageSubresourceRange::all(Format::Rgba16Float),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        global: false,
    });
    encoder.write_timestamp(timers, 1);

    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("opaque"),
        color_attachments: &[ColorAttachment {
            view: color_view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color([0.0, 0.0, 0.0, 1.0]),
        }],
        depth_stencil_attachment: Some(crate::DepthStencilAttachment {
            view: depth_view,
            read_only: false,
            depth_load: LoadOp::Clear,
            depth_store: StoreOp::Store,
            stencil_load: LoadOp::DontCare,
            stencil_store: StoreOp::Discard,
            clear: ClearValue::default(),
        }),
        render_area: Rect2d::from_size(1920, 1080),
    });
    encoder.set_viewport(&Viewport::from_size(1920, 1080));
    encoder.set_scissor(&Rect2d::from_size(1920, 1080));
    encoder.bind_graphics_pipeline(opaque);
    encoder.bind_group(0, bind_group, &[], pipeline_layout);
    encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
    encoder.draw_indexed_indirect_count(&DrawIndirectCount {
        args: draw_args,
        args_offset: 0,
        count_buffer: draw_count,
        count_offset: 0,
        max_draw_count: 4096,
        stride: 20,
    });
    encoder.end_render_pass();
    encoder.write_timestamp(timers, 2);

    let command_buffer = encoder.finish().expect("finish");
    device
        .submit(queue, &SubmitInfo::new(&[command_buffer]))
        .expect("submit");

    recorder.assert_valid();
    assert_eq!(
        recorder.command_names(),
        vec![
            "ResetQuerySet",
            "WriteTimestamp",
            "FillBuffer",
            "Barrier",
            "BeginComputePass",
            "BindComputePipeline",
            "BindGroup",
            "Dispatch",
            "EndComputePass",
            "Barrier",
            "WriteTimestamp",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "BindGraphicsPipeline",
            "BindGroup",
            "BindIndexBuffer",
            "DrawIndexedIndirectCount",
            "EndRenderPass",
            "WriteTimestamp",
        ]
    );
    assert_eq!(
        recorder.pass_labels(),
        vec!["cull".to_string(), "opaque".to_string()],
        "cull runs before the draws that consume its output"
    );

    // The clear value that reached the backend is the reversed-Z far plane.
    let depth_clear = recorder
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
    assert_eq!(depth_clear, depth::CLEAR);
    assert_eq!(depth_clear, 0.0);

    let events = recorder.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Submitted { .. })),
        "the submit must appear in the stream"
    );
    let finished_at = events
        .iter()
        .position(|event| matches!(event, Event::Finished { .. }))
        .expect("finish event");
    let submitted_at = events
        .iter()
        .position(|event| matches!(event, Event::Submitted { .. }))
        .expect("submit event");
    assert!(finished_at < submitted_at, "recording precedes submission");
}

#[test]
fn timestamp_queries_read_back_zeros_without_failing() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let timers = device
        .create_query_set(&QuerySetDesc {
            label: None,
            kind: QueryKind::Timestamp,
            count: 2,
        })
        .expect("timers");
    let mut results = [7u64; 2];
    device.query_results(timers, 0, &mut results).expect("read");
    assert_eq!(results, [0, 0]);
    device.destroy_query_set(timers);
    assert!(
        device.query_results(timers, 0, &mut results).is_err(),
        "a destroyed query set must not resolve"
    );
}

#[test]
fn tier_b_refuses_query_kinds_it_lacks() {
    let instance = NullInstance::tier_b();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("device");
    let error = device
        .create_query_set(&QuerySetDesc {
            label: None,
            kind: QueryKind::Timestamp,
            count: 1,
        })
        .expect_err("no timestamp support");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
}

#[test]
fn wait_idle_and_semaphore_waits_are_recorded_and_satisfied() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frame"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("semaphore");
    assert_eq!(device.semaphore_value(semaphore).expect("value"), 0);
    assert!(
        device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore,
                    value: 1
                }],
                0
            )
            .expect("wait"),
        "nothing is ever outstanding, so waits succeed immediately"
    );
    device.wait_idle().expect("wait idle");
    assert!(
        recorder
            .events()
            .iter()
            .any(|event| matches!(event, Event::WaitedIdle))
    );
}

#[test]
fn images_are_validated_against_the_devices_limits() {
    let instance = NullInstance::tier_b();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("device");
    let too_big = ImageDesc {
        label: None,
        image_type: ImageType::D2,
        // Above the 8192 floor that `Limits::minimum` reports.
        extent: crate::Extent3d::d2(16384, 16384),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::SAMPLED,
        memory: MemoryLocation::DeviceLocal,
    };
    assert!(device.create_image(&too_big).is_err());
    assert!(
        device
            .create_image(&ImageDesc {
                extent: crate::Extent3d::d2(0, 8),
                ..too_big
            })
            .is_err(),
        "a zero extent is a descriptor bug"
    );
    assert!(
        device
            .create_image(&ImageDesc {
                extent: crate::Extent3d::d2(4096, 4096),
                mip_levels: 0,
                ..too_big
            })
            .is_err(),
        "an image needs at least one mip level"
    );
}

// --- object-lifetime contract (see `crate::device`) --------------------------

/// Rule 3: a surface from one instance must not be usable by a device from
/// another. Handle *bits* collide freely — both instances issue slot 0,
/// generation 1 — so this can only be caught by the owner side table, which is
/// exactly the point of the obligation.
#[test]
fn a_surface_from_another_instance_is_a_foreign_object() {
    // One shared recorder, so both instances' objects live in the same pools —
    // the hardest case, and the one a naive "does the handle resolve?" check
    // gets wrong.
    let recorder = Recorder::new();
    let first = NullInstance::tier_a().with_recorder(recorder.clone());
    let second = NullInstance::tier_a().with_recorder(recorder.clone());

    // SAFETY: an offscreen target holds no platform pointers.
    let foreign = unsafe { second.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");

    let error = first
        .surface_caps(foreign, AdapterId(0))
        .expect_err("the surface belongs to the other instance");
    assert!(
        matches!(
            error,
            HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "{error:?}"
    );

    let error = first
        .create_device(&DeviceDesc {
            compatible_surface: Some(foreign),
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect_err("foreign compatible_surface");
    assert!(matches!(error, HalError::ForeignObject { .. }), "{error:?}");

    let device = open(&first);
    let error = device
        .create_swapchain(&SwapchainDesc {
            label: None,
            surface: foreign,
            format: Format::Bgra8UnormSrgb,
            extent: (8, 8),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect_err("foreign surface");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::ForeignObject { .. })),
        "{error:?}"
    );

    // And destroying it through the wrong instance must be a no-op, not a
    // silent free of someone else's object.
    first.destroy_surface(foreign);
    assert!(
        second.surface_caps(foreign, AdapterId(0)).is_ok(),
        "the owning instance's surface survived the foreign destroy"
    );
    second.destroy_surface(foreign);
}

/// Rule 3, device scope: two devices from the *same* instance still do not
/// share resources.
#[test]
fn a_buffer_from_another_device_is_a_foreign_object() {
    let instance = NullInstance::tier_a();
    let first = open(&instance);
    let second = open(&instance);

    let buffer = first
        .create_buffer(&BufferDesc {
            label: Some("first device's"),
            size: 16,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("buffer");

    let error = second
        .write_buffer(buffer, 0, &[0; 4])
        .expect_err("the buffer belongs to the other device");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "buffer", .. }),
        "{error:?}"
    );
    // The owning device is unaffected.
    first.write_buffer(buffer, 0, &[0; 4]).expect("write");

    // Destroying through the wrong device must not free it.
    second.destroy_buffer(buffer);
    first.write_buffer(buffer, 0, &[0; 4]).expect("still alive");
    first.destroy_buffer(buffer);
    assert!(first.write_buffer(buffer, 0, &[0; 4]).is_err());
}

/// A recorded command naming another device's handle is a validation error
/// distinct from a dead one — the fixes differ.
#[test]
fn commands_naming_a_foreign_handle_are_recorded_as_such() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let first = open(instance.as_ref());
    let second = open(instance.as_ref());
    let queue = first.queue(QueueKind::Graphics).expect("queue");

    let foreign = second
        .create_buffer(&BufferDesc {
            label: None,
            size: 16,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("buffer");

    let mut encoder = first.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.fill_buffer(foreign, 0, 4, 0);
    let _ = encoder.finish().expect("finish");

    let errors = recorder.validation_errors();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::ForeignHandle {
                command: "FillBuffer",
                ..
            }
        )),
        "{errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|error| matches!(error, ValidationError::DeadHandle { .. })),
        "a live-but-foreign handle is not a dead one: {errors:?}"
    );
}

/// Rule 2: destroying the surface before its swapchain invalidates the handle
/// immediately, and the swapchain built on it is unusable afterwards — an
/// error, never undefined behaviour.
#[test]
fn destroying_a_surface_out_of_order_is_detected_not_undefined() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let desc = SwapchainDesc {
        label: None,
        surface,
        format: Format::Bgra8UnormSrgb,
        extent: (8, 8),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    let swapchain = device.create_swapchain(&desc).expect("swapchain");

    // Out of order: surface first.
    instance.destroy_surface(surface);

    // The swapchain object itself is still tracked, so acquiring from it does
    // not fault — the backend deferred the real teardown. But nothing new can
    // be built on the dead surface handle.
    device
        .acquire_next_frame(swapchain)
        .expect("still acquirable");
    let error = device.create_swapchain(&desc).expect_err("dead surface");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidHandle { .. })),
        "{error:?}"
    );

    device.destroy_swapchain(swapchain);
    let error = device
        .acquire_next_frame(swapchain)
        .expect_err("dead swapchain");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidHandle { .. })),
        "{error:?}"
    );
}

/// Rule 1: a device keeps working after its instance is dropped. If the null
/// backend held a borrow rather than a shared handle, this would not compile;
/// if it held a raw pointer, it would be a use-after-free.
#[test]
fn a_device_outlives_its_instance() {
    let recorder = Recorder::new();
    let device = {
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::tier_a().with_recorder(recorder.clone()));
        let adapters = instance.adapters();
        instance
            .create_device(&DeviceDesc {
                optional_features: Features::all(),
                ..DeviceDesc::for_adapter(adapters[0].id)
            })
            .expect("device")
        // `instance` is dropped here.
    };

    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("after the instance went away"),
            size: 32,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the device still works");
    device
        .write_buffer(buffer, 0, &[1, 2, 3, 4])
        .expect("write");
    device.destroy_buffer(buffer);
    device.wait_idle().expect("wait idle");
    assert_eq!(recorder.total_live_objects(), 0);
}

// --- readback ---------------------------------------------------------------

#[test]
fn readback_completes_and_yields_the_buffers_bytes() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("culling stats"),
            size: 16,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("readback buffer");

    let request = device
        .request_readback(&ReadbackDesc {
            label: Some("stats"),
            buffer,
            offset: 4,
            size: 8,
            after: None,
        })
        .expect("request");

    let mut out = [0xFFu8; 8];
    assert_eq!(
        device.poll_readback(request, &mut out).expect("poll"),
        ReadbackState::Ready
    );
    assert_eq!(out, [0; 8], "nothing executed, so the bytes are zeros");

    // Polling again is legal and idempotent until the request is destroyed.
    assert_eq!(
        device.poll_readback(request, &mut out).expect("poll"),
        ReadbackState::Ready
    );

    device.destroy_readback(request);
    assert!(
        device.poll_readback(request, &mut out).is_err(),
        "a destroyed readback must not resolve"
    );
    device.destroy_buffer(buffer);
    assert_eq!(recorder.total_live_objects(), 0);
}

/// The property a blocking API could never have: a caller must be able to be
/// told "not yet" and come back next frame. This is what the browser's
/// `mapAsync` does on every readback, so the poll loop has to be exercised.
#[test]
fn a_pending_readback_reports_pending_and_leaves_the_output_untouched() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    recorder.set_readback_latency(3);

    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 4,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("readback buffer");
    let request = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer,
            offset: 0,
            size: 4,
            after: None,
        })
        .expect("request");

    let mut out = [0xAAu8; 4];
    for poll in 0..3 {
        assert_eq!(
            device.poll_readback(request, &mut out).expect("poll"),
            ReadbackState::Pending,
            "poll {poll}"
        );
        assert_eq!(out, [0xAA; 4], "a pending poll must not write the output");
    }
    assert_eq!(
        device.poll_readback(request, &mut out).expect("poll"),
        ReadbackState::Ready
    );
    assert_eq!(out, [0; 4]);

    device.destroy_readback(request);
    device.destroy_buffer(buffer);
}

#[test]
fn readback_validates_its_buffer_memory_range_and_output_length() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);

    // Wrong memory location: WebGPU would reject the MAP_READ usage too.
    let upload = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 8,
            usage: crate::BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("buffer");
    let error = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer: upload,
            offset: 0,
            size: 8,
            after: None,
        })
        .expect_err("not host-readable");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    let readable = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 8,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("buffer");

    // Out of range.
    let error = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer: readable,
            offset: 4,
            size: 8,
            after: None,
        })
        .expect_err("past the end");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // Output slice must match the requested size exactly.
    let request = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer: readable,
            offset: 0,
            size: 8,
            after: None,
        })
        .expect("request");
    let error = device
        .poll_readback(request, &mut [0; 4])
        .expect_err("wrong output length");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // Destroying the source between request and poll is caught, not read.
    device.destroy_buffer(readable);
    let error = device
        .poll_readback(request, &mut [0; 8])
        .expect_err("source is gone");
    assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    device.destroy_readback(request);
}

#[test]
fn a_readback_may_name_an_explicit_completion_point() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frame"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("semaphore");
    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 4,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("buffer");

    let request = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer,
            offset: 0,
            size: 4,
            after: Some(SemaphoreWait {
                semaphore,
                value: 3,
            }),
        })
        .expect("request");
    assert_eq!(
        device.poll_readback(request, &mut [0; 4]).expect("poll"),
        ReadbackState::Ready
    );

    // A dead semaphore in `after` is caught at request time.
    device.destroy_semaphore(semaphore);
    let error = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer,
            offset: 0,
            size: 4,
            after: Some(SemaphoreWait {
                semaphore,
                value: 3,
            }),
        })
        .expect_err("dead semaphore");
    assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");

    device.destroy_readback(request);
    device.destroy_buffer(buffer);
}

// --- validation the null backend used to skip --------------------------------
//
// The null backend is documented as the reference validator the graph suite
// runs against, so anything it accepts that `crcbl-vk` rejects lets CI
// green-light a command stream Vulkan refuses. These pin the gaps closed.

/// A bind group must be checked against the layout it claims to conform to:
/// the binding must exist, the array index must be inside it, and the resource
/// must be the right *kind*. None of that used to happen here.
#[test]
fn a_bind_group_is_checked_against_its_layout() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);

    let layout = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("frame"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::ALL,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 2,
                flags: crate::BindingFlags::empty(),
            }],
        })
        .expect("set layout");
    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 64,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("buffer");
    let sampler = device
        .create_sampler(&crate::SamplerDesc::default())
        .expect("sampler");

    let entry = |binding, array_index, resource| BindGroupEntry {
        binding,
        array_index,
        resource,
    };
    fn group<'a>(
        layout: crate::BindGroupLayoutHandle,
        entries: &'a [BindGroupEntry],
    ) -> BindGroupDesc<'a> {
        BindGroupDesc {
            label: None,
            layout,
            entries,
            variable_count: None,
        }
    }

    device
        .create_bind_group(&group(
            layout,
            &[entry(0, 1, crate::BindingResource::whole_buffer(buffer))],
        ))
        .expect("index 1 of a two-element binding is in range");

    let error = device
        .create_bind_group(&group(
            layout,
            &[entry(7, 0, crate::BindingResource::whole_buffer(buffer))],
        ))
        .expect_err("binding 7 is not declared");
    assert!(error.to_string().contains("does not declare"), "{error}");

    let error = device
        .create_bind_group(&group(
            layout,
            &[entry(0, 2, crate::BindingResource::whole_buffer(buffer))],
        ))
        .expect_err("index 2 of a two-element binding is out of range");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    let error = device
        .create_bind_group(&group(
            layout,
            &[entry(0, 0, crate::BindingResource::Sampler(sampler))],
        ))
        .expect_err("a sampler cannot fill a storage-buffer binding");
    assert!(error.to_string().contains('0'), "{error}");
}

/// `update_bind_group` on a layout without `UPDATE_AFTER_BIND` is the error
/// `Device::update_bind_group` promises, and `crcbl-vk` produces. The null
/// backend used to accept it, which made the doc untestable.
#[test]
fn updating_a_group_without_update_after_bind_is_refused() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let slot = |flags| BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::ALL,
        kind: BindingKind::SampledImage,
        count: 1,
        flags,
    };

    let plain = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[slot(crate::BindingFlags::empty())],
        })
        .expect("layout");
    let plain_group = device
        .create_bind_group(&BindGroupDesc {
            label: None,
            layout: plain,
            entries: &[],
            variable_count: None,
        })
        .expect("group");
    let error = device
        .update_bind_group(plain_group, &[])
        .expect_err("no UPDATE_AFTER_BIND");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    let bindless = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: None,
            entries: &[slot(crate::BindingFlags::UPDATE_AFTER_BIND)],
        })
        .expect("layout");
    let bindless_group = device
        .create_bind_group(&BindGroupDesc {
            label: None,
            layout: bindless,
            entries: &[],
            variable_count: None,
        })
        .expect("group");
    device
        .update_bind_group(bindless_group, &[])
        .expect("UPDATE_AFTER_BIND is what the flag is for");
}

/// `PushConstantRange { offset: u32::MAX, size: 1 }` panicked in debug and
/// wrapped to `0` in release — which then *passed* the limit check it was
/// supposed to fail.
#[test]
fn a_push_constant_range_that_overflows_is_refused_not_wrapped() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let error = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::ALL,
                offset: u32::MAX,
                size: 1,
            }),
        })
        .expect_err("the range ends past every possible budget");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
}

/// The range check `Device::query_results` documents, which was impossible
/// before: `first_query` was ignored and the set's size was never stored.
#[test]
fn query_results_are_bounded_by_the_sets_size() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let set = device
        .create_query_set(&QuerySetDesc {
            label: Some("timers"),
            kind: QueryKind::Timestamp,
            count: 4,
        })
        .expect("query set");

    let mut out = [1u64; 2];
    device
        .query_results(set, 2, &mut out)
        .expect("2..4 is inside a four-query set");
    assert_eq!(out, [0, 0], "a device with no timestamps reports zeros");

    let error = device
        .query_results(set, 3, &mut out)
        .expect_err("3..5 runs off the end");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
}

/// Image validation used to compare `max(width, height)` against
/// `max_image_2d` for every image type, leaving `max_image_3d` read by nothing,
/// a volume's depth checked against nothing, and `samples` unvalidated.
#[test]
fn images_are_validated_per_type_and_sample_count() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    let limits = device.caps().limits;
    let base = ImageDesc {
        label: None,
        image_type: ImageType::D3,
        extent: crate::Extent3d {
            width: 4,
            height: 4,
            depth_or_layers: limits.max_image_3d + 1,
        },
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::SAMPLED,
        memory: MemoryLocation::DeviceLocal,
    };

    let error = device
        .create_image(&base)
        .expect_err("a volume's depth is bounded by max_image_3d");
    assert!(error.to_string().contains("max_image_3d"), "{error}");

    let legal_extent = crate::Extent3d {
        width: 4,
        height: 4,
        depth_or_layers: 4,
    };
    device
        .create_image(&ImageDesc {
            extent: legal_extent,
            ..base
        })
        .expect("a 4x4x4 volume is fine");

    // A sample count is a mask underneath, so `3` is `TYPE_1 | TYPE_2`.
    let error = device
        .create_image(&ImageDesc {
            extent: legal_extent,
            samples: 3,
            ..base
        })
        .expect_err("3 is not a power of two");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    let error = device
        .create_image(&ImageDesc {
            extent: legal_extent,
            samples: limits.max_sample_count * 2,
            ..base
        })
        .expect_err("past the device's ceiling");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // An image nothing may ever do anything with is a caller bug.
    let error = device
        .create_image(&ImageDesc {
            extent: legal_extent,
            usage: ImageUsage::empty(),
            ..base
        })
        .expect_err("no usage flags");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

    // A volume's depth joins the mip chain; a 4x4x4 one has three levels.
    let error = device
        .create_image(&ImageDesc {
            extent: legal_extent,
            mip_levels: 4,
            ..base
        })
        .expect_err("a 4x4x4 volume has three mip levels, not four");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
}

/// Obligation 3 covers queues too. `submit`, `present` and
/// `create_command_encoder` used to ignore the queue argument entirely, so
/// another device's handle — synthesised from the kind index alone, and
/// therefore *identical* — submitted happily.
#[test]
fn a_queue_from_another_device_is_foreign() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let first = open(instance.as_ref());
    let second = open(instance.as_ref());

    let mine = first.queue(QueueKind::Graphics).expect("queue");
    let theirs = second.queue(QueueKind::Graphics).expect("queue");
    assert_ne!(
        mine, theirs,
        "two devices must not synthesise the same queue handle"
    );

    let error = first
        .submit(theirs, &SubmitInfo::new(&[]))
        .expect_err("that queue belongs to the other device");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "queue", .. }),
        "{error:?}"
    );

    // An encoder created against a foreign queue reports it at `finish`, where
    // there is an error path — `create_command_encoder` has none.
    let encoder = first.create_command_encoder(&CommandEncoderDesc {
        label: None,
        queue: theirs,
    });
    let error = encoder.finish().expect_err("foreign queue");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "queue", .. }),
        "{error:?}"
    );

    // The device's own queue still works.
    first.submit(mine, &SubmitInfo::new(&[])).expect("submit");
    let _ = recorder;
}

/// A rejected poll must not consume one of the simulated latency's polls: the
/// caller would otherwise observe a shorter latency than it configured, which
/// makes the latency it is being tested against a lie.
#[test]
fn a_rejected_poll_does_not_advance_the_simulated_readback() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let device = open(instance.as_ref());
    recorder.set_readback_latency(2);

    let buffer = device
        .create_buffer(&BufferDesc {
            label: None,
            size: 8,
            usage: crate::BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("buffer");
    let readback = device
        .request_readback(&ReadbackDesc {
            label: None,
            buffer,
            offset: 0,
            size: 8,
            after: None,
        })
        .expect("readback");

    // Three polls with the wrong slice length. Each must be rejected and change
    // nothing.
    let mut wrong = [0u8; 4];
    for _ in 0..3 {
        let error = device
            .poll_readback(readback, &mut wrong)
            .expect_err("the slice is the wrong length");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    let mut right = [0u8; 8];
    for poll in 0..2 {
        assert_eq!(
            device.poll_readback(readback, &mut right).expect("poll"),
            ReadbackState::Pending,
            "poll {poll} must still be pending: the configured latency is 2"
        );
    }
    assert_eq!(
        device.poll_readback(readback, &mut right).expect("poll"),
        ReadbackState::Ready
    );
}

/// `SwapchainDesc::format` must be one of `SurfaceCaps::formats`, and the ring
/// size must be clamped to the min/max the same backend reports.
#[test]
fn a_swapchain_format_must_be_one_the_surface_offers() {
    let instance = NullInstance::tier_a();
    let device = open(&instance);
    // SAFETY: `Offscreen` names no platform object.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let caps = instance
        .surface_caps(surface, AdapterId(0))
        .expect("surface caps");

    let desc = |format| SwapchainDesc {
        label: None,
        surface,
        format,
        extent: (64, 48),
        image_count: 99,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };

    let error = device
        .create_swapchain(&desc(Format::D32Float))
        .expect_err("no surface presents a depth format");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(_))),
        "{error:?}"
    );

    let swapchain = device
        .create_swapchain(&desc(caps.formats[0]))
        .expect("a reported format is accepted");

    // 99 images is clamped to the reported maximum, not to a literal that
    // happens to agree with it.
    let mut seen = std::collections::HashSet::new();
    for _ in 0..(caps.max_image_count + 1) {
        let frame = device
            .acquire_next_frame(swapchain)
            .expect("the ring hands out images");
        seen.insert(frame.image);
        device
            .present(
                device.queue(QueueKind::Graphics).expect("queue"),
                &PresentInfo {
                    swapchain,
                    waits: &[],
                },
            )
            .expect("present");
    }
    assert!(
        seen.len() as u32 <= caps.max_image_count,
        "the ring must hold at most {} images, saw {}",
        caps.max_image_count,
        seen.len()
    );

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// The simulated device latency applies to requests made *after* it is set, and
/// each request counts down on its own — the same contract
/// `set_readback_latency` carries, and the one a test that opens two devices in
/// one process depends on.
#[test]
fn device_latency_applies_per_request_and_only_after_it_is_set() {
    let (recorder, instance) = boxed(NullInstance::tier_a());
    let adapter = instance.adapters()[0].id;

    // Requested before the latency is set: unaffected.
    let mut instant = instance
        .request_device(&DeviceDesc::for_adapter(adapter))
        .expect("request");
    recorder.set_device_latency(2);
    assert!(
        instant.poll().expect("poll").is_ready(),
        "a request already in flight must not pick up a latency set later"
    );

    // Two requests after it: both count down independently.
    let mut first = instance
        .request_device(&DeviceDesc::for_adapter(adapter))
        .expect("request");
    let mut second = instance
        .request_device(&DeviceDesc::for_adapter(adapter))
        .expect("request");
    for _ in 0..2 {
        assert!(!first.poll().expect("poll").is_ready());
        assert!(!second.poll().expect("poll").is_ready());
    }
    assert!(first.poll().expect("poll").is_ready());
    assert!(second.poll().expect("poll").is_ready());
}

/// A device that arrived through the polled path is the same device the
/// blocking wrapper would have produced, ownership stamping included: a buffer
/// from one is foreign to the other.
#[test]
fn a_polled_device_stamps_its_own_ownership_like_any_other() {
    let (_recorder, instance) = boxed(NullInstance::tier_a());
    let adapter = instance.adapters()[0].id;

    let mut request = instance
        .request_device(&DeviceDesc::for_adapter(adapter))
        .expect("request");
    let polled = request
        .poll()
        .expect("poll")
        .into_device()
        .expect("ready on the first poll");
    let blocking = instance
        .create_device(&DeviceDesc::for_adapter(adapter))
        .expect("the blocking wrapper");

    let buffer = polled
        .create_buffer(&BufferDesc {
            label: Some("polled device's buffer"),
            size: 64,
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("create");
    let error = blocking
        .write_buffer(buffer, 0, &[0u8; 4])
        .expect_err("another device's buffer is foreign");
    assert!(matches!(error, HalError::ForeignObject { .. }), "{error:?}");
    polled.destroy_buffer(buffer);
}
