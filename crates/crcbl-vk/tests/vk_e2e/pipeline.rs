//! The pipeline-creation surface: bind-group layouts, entry-point resolution
//! and samplers.
//!
//! What these have in common — and why they are one module rather than parts of
//! the milestones that use them — is that each is decided by `crcbl-vk` before
//! a driver object exists, and each is a refusal rather than a picture. A
//! layout carrying `BindingFlags` must be *rejected* by a device without
//! `DESCRIPTOR_INDEXING` instead of quietly downgraded, and an entry point the
//! module does not have must be named here rather than surfacing as the
//! driver's initialisation failure, which names neither the module nor the
//! stage.
//!
//! The bindless test asserts both arms on every machine. The accepting one
//! comes from whatever this device reports; the refusing one comes from a
//! second device opened without `DESCRIPTOR_INDEXING`, because every adapter
//! this suite can reach reports it and the refusal would otherwise be compiled
//! and run nowhere.

use crate::harness::Headless;
use crate::triangle::TRIANGLE_EXTENT;
use crcbl_hal::{Features, SampleType};

/// The tier story for bind-group layouts, against whatever this machine is.
///
/// The seam requires a device without `DESCRIPTOR_INDEXING` to **reject** a
/// layout that sets any [`BindingFlags`](crcbl_hal::BindingFlags), rather than
/// ignoring it — "a bindless array quietly downgraded to a fixed one reads
/// garbage at index 4097". The first branch runs whichever arm this driver's
/// tier selects; the refusal is then asserted again against a device opened
/// without the feature, so it runs everywhere rather than only on a driver
/// nobody here has.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_bindless_capable_layout_is_accepted_or_refused_according_to_the_tier() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let indexing = device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING);

    // The Tier A shape: a runtime-sized texture array on the last binding.
    let entries = [
        crcbl_hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl_hal::ShaderStages::VERTEX,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        },
        crcbl_hal::BindGroupLayoutEntry {
            binding: 1,
            visibility: crcbl_hal::ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::SampledImage {
                view_type: crcbl_hal::ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            // The seam's "as many as you can"; the backend clamps it to the
            // device's own `max_bindless_descriptors`.
            count: u32::MAX,
            flags: crcbl_hal::BindingFlags::VARIABLE_COUNT
                | crcbl_hal::BindingFlags::PARTIALLY_BOUND
                | crcbl_hal::BindingFlags::UPDATE_AFTER_BIND,
        },
    ];
    let result = device.create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
        label: Some("bindless probe"),
        entries: &entries,
    });

    match result {
        Ok(layout) => {
            assert!(
                indexing,
                "a device that reports no DESCRIPTOR_INDEXING must not accept a bindless layout"
            );
            assert!(
                device.caps().limits.max_bindless_descriptors > 0,
                "a Tier A device must report a bindless ceiling to clamp u32::MAX against"
            );
            device.destroy_bind_group_layout(layout);
        }
        Err(error) => {
            assert!(
                !indexing,
                "a Tier A device must accept the bindless shape: {error}"
            );
            assert!(
                matches!(error, crcbl_hal::HalError::Unsupported { .. }),
                "the refusal must be loud and typed, not an InvalidDescriptor: {error}"
            );
            eprintln!("vk e2e: Tier B device refused the bindless layout, as required: {error}");
        }
    }

    // **The refusal arm, on a device manufactured to have it.** The branch
    // above takes whichever arm this driver's tier selects, and every adapter
    // this suite can reach — radv and lavapipe both — reports
    // `DESCRIPTOR_INDEXING`, so the `Err` half was compiled and executed
    // nowhere. Subtracting the feature is what reaches it, the same move
    // `mesh`'s geometry-path sweep makes to reach `IndirectPerBatch`.
    let lesser = Headless::open_pinning_format(
        "vk e2e bindless tier b",
        Features::DEBUG_MARKERS,
        TRIANGLE_EXTENT,
    );
    let tier_b = lesser.device.as_ref();
    assert!(
        !tier_b
            .caps()
            .features
            .contains(Features::DESCRIPTOR_INDEXING),
        "this device is opened without DESCRIPTOR_INDEXING; if it reports the \
         feature anyway the subtraction is not happening and the arm below \
         would be the Tier A one wearing this one's name"
    );
    let error = tier_b
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: None,
            entries: &entries,
        })
        .expect_err("a device without DESCRIPTOR_INDEXING must refuse a bindless layout");
    assert!(
        matches!(error, crcbl_hal::HalError::Unsupported { .. }),
        "the refusal must be loud and typed, not an InvalidDescriptor: {error}"
    );
    lesser.finish();

    // `VARIABLE_COUNT` anywhere but last is a caller bug on *every* tier.
    let misplaced = [entries[1], entries[0]];
    let error = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: None,
            entries: &misplaced,
        })
        .expect_err("VARIABLE_COUNT is only legal on the last binding");
    eprintln!("vk e2e: misplaced VARIABLE_COUNT refused: {error}");

    headless.finish();
}

/// A pipeline naming an entry point the module does not have must be refused
/// **here**, with the available ones listed — not by the driver, which reports
/// it as an initialisation failure naming neither.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_missing_entry_point_is_named_before_the_driver_sees_it() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();

    let module = device
        .create_shader_module(&crcbl_hal::ShaderModuleDesc {
            label: Some("triangle.slang"),
            spirv: crcbl_shaders::TRIANGLE.spirv(),
            wgsl: crcbl_shaders::TRIANGLE.wgsl(),
            msl: crcbl_shaders::TRIANGLE.msl(),
            dxil: &[],
        })
        .expect("the committed SPIR-V is accepted");
    let pipeline_layout = device
        .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("an empty pipeline layout");
    let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];

    // `main` is what a GLSL habit reaches for, and Slang emits neither.
    let error = device
        .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: None,
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "main",
            },
            fragment: None,
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect_err("there is no entry point called `main`");
    let text = error.to_string();
    assert!(
        text.contains("vertexMain"),
        "the list must be shown: {text}"
    );

    // And naming the right entry point at the wrong stage gets its own wording.
    let error = device
        .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: None,
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "fragmentMain",
            },
            fragment: None,
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect_err("fragmentMain is not a vertex entry point");
    assert!(error.to_string().contains("but not at"), "{error}");

    // Bytes where words were wanted, which is the mistake the seam's docs single
    // out, must be caught here rather than by `vkCreateShaderModule`.
    let error = device
        .create_shader_module(&crcbl_hal::ShaderModuleDesc {
            label: None,
            spirv: &[0x0302_2307, 0, 0, 0, 0],
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect_err("a byte-swapped module is not SPIR-V");
    assert!(error.to_string().contains("byte-swapped"), "{error}");

    device.destroy_pipeline_layout(pipeline_layout);
    device.destroy_shader_module(module);
    headless.finish();
}

/// Samplers, which land with the rest of the pipeline surface at P1.2 and are
/// the one part of it milestone 2 does not otherwise exercise.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn samplers_honour_the_seams_defaults_and_its_limits() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();

    let sampler = device
        .create_sampler(&crcbl_hal::SamplerDesc::default())
        .expect("the default trilinear repeating sampler");
    device.destroy_sampler(sampler);

    // Reversed-Z reaches the comparison sampler too: a shadow test asking "is
    // the fragment closer?" is `Greater`, and the seam says so.
    let shadow = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            label: Some("shadow pcf"),
            compare: Some(crcbl_hal::CompareOp::Greater),
            address_mode: [crcbl_hal::SamplerAddressMode::ClampToBorder; 3],
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect("a comparison sampler");
    device.destroy_sampler(shadow);

    // Anisotropy past the device's ceiling is an error, not a clamp: silently
    // sampling differently from what was asked for is a golden-image difference
    // nobody can explain.
    //
    // **Which error depends on why the device will not do it**, and this
    // fixture does not ask for `SAMPLER_ANISOTROPY`. Without it the ceiling is
    // 1.0 — the value that turns anisotropy off — so `ceiling + 1.0` is a
    // request for a capability this device has not got, and the seam spells
    // that `Unsupported`; a caller branching on that variant to pick a fallback
    // would miss an `InvalidDescriptor`. The real-ceiling arm, on a device that
    // *has* the feature, is
    // `a_sampler_above_the_anisotropy_ceiling_is_refused` in
    // `crates/crcbl/tests/hal_seam_e2e.rs`, which opens its own device for it.
    let caps = device.caps();
    let ceiling = caps.limits.max_sampler_anisotropy;
    let error = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            anisotropy: ceiling + 1.0,
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect_err("anisotropy past the limit must be refused");
    if caps.features.contains(Features::SAMPLER_ANISOTROPY) {
        assert!(
            matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
            "this device can filter anisotropically, so a number past its ceiling is a bad \
             descriptor: {error}"
        );
    } else {
        assert!(
            matches!(error, crcbl_hal::HalError::Unsupported { .. }),
            "this device has not got SAMPLER_ANISOTROPY, so any anisotropic request is a \
             capability it lacks rather than a number to correct: {error}"
        );
    }

    headless.finish();
}

/// A uniform binding longer than `max_uniform_buffer_range`, against a driver.
///
/// Binding more of a buffer than the slot's range limit allows is
/// `VUID-VkWriteDescriptorSet-descriptorType-00332`, which the validation layer
/// reports and a release driver does not — so the seam refuses it first, and
/// this is that refusal arriving from a real device's own reported limit rather
/// than from a limit the null backend chose.
///
/// # What this machine can and cannot reach
///
/// The over-long *explicit* range runs everywhere: the seam refuses before
/// `vkUpdateDescriptorSets` is called, so the buffer need only be big enough to
/// exist. The `WHOLE_BUFFER` spelling needs a buffer past the ceiling, and
/// whether one can be allocated is the driver's business — RADV reports
/// `maxUniformBufferRange` as 4 GiB−1, over `maxBufferSize`, so no such buffer
/// exists on it. That arm therefore runs only where the ceiling is small enough
/// to pass, and says which happened. `crcbl-hal`'s null backend covers both
/// spellings unconditionally, since it can report any limit it likes.
///
/// The storage ceiling has no test here at all, for the same reason and more
/// so: drivers report it at or near 4 GiB. That also means this test cannot
/// tell the two limits apart — RADV reports both as 4 GiB−1, so reading the
/// storage limit for a uniform slot passes here and is caught by the null
/// backend's test, where the two differ.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_uniform_binding_over_the_range_limit_is_refused() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let limits = device.caps().limits;
    let ceiling = limits.max_uniform_buffer_range;

    let layout = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("one uniform"),
            entries: &[crcbl_hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            }],
        })
        .expect("a single uniform binding");
    let buffer_of = |size| {
        device
            .create_buffer(&crcbl_hal::BufferDesc {
                label: Some("uniform range"),
                size,
                usage: crcbl_hal::BufferUsage::UNIFORM,
                memory: crcbl_hal::MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer")
    };
    let group_of = |buffer, offset, size| {
        device.create_bind_group(&crcbl_hal::BindGroupDesc {
            label: None,
            layout,
            entries: &[crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            }],
            variable_count: None,
        })
    };
    let refused = |error: crcbl_hal::HalError, case: &str| {
        assert!(
            matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        let text = error.to_string();
        assert!(text.contains("binding 0"), "{case}: {text}");
        assert!(text.contains(&ceiling.to_string()), "{case}: {text}");
    };

    // An explicit range one byte over, which never reaches the driver.
    let small = buffer_of(4096);
    refused(
        group_of(small, 0, ceiling + 1).expect_err("a range over the ceiling"),
        "explicit size",
    );
    // …and the same buffer bound whole, which is under it.
    let group = group_of(small, 0, crcbl_hal::BindingResource::WHOLE_BUFFER)
        .expect("a whole small buffer is under the ceiling");
    device.destroy_bind_group(group);

    // The sentinel over the ceiling, where the hardware admits such a buffer.
    let over_size = ceiling.saturating_add(1);
    if over_size <= u64::from(u32::MAX) && over_size < (1 << 30) {
        let over = buffer_of(over_size);
        refused(
            group_of(over, 0, crcbl_hal::BindingResource::WHOLE_BUFFER)
                .expect_err("a whole buffer over the ceiling"),
            "whole buffer",
        );
        // One aligned offset in, the rest of it fits again.
        let offset = limits.min_uniform_buffer_offset_alignment.max(1);
        let group = group_of(over, offset, crcbl_hal::BindingResource::WHOLE_BUFFER)
            .expect("the rest of the buffer is under the ceiling");
        device.destroy_bind_group(group);
        device.destroy_buffer(over);
        eprintln!("vk e2e: uniform ceiling {ceiling}, whole-buffer arm ran");
    } else {
        eprintln!(
            "vk e2e: uniform ceiling {ceiling} is past what this driver will allocate; the \
             whole-buffer arm did not run"
        );
    }

    device.destroy_buffer(small);
    device.destroy_bind_group_layout(layout);

    headless.finish();
}

/// A writable storage binding of a host-visible buffer, against a driver.
///
/// `BufferDesc::memory` states the rule: a buffer a shader writes must be
/// `MemoryLocation::DeviceLocal`, because D3D12's upload and readback heaps
/// refuse `ALLOW_UNORDERED_ACCESS` at creation. Vulkan has no such restriction
/// — a `HostUpload` buffer bound to a writable storage slot is accepted by the
/// driver and by the validation layer, which is exactly why nothing here
/// caught it and why the refusal has to be `crcbl-vk`'s own.
///
/// The read-only arm is not decoration: the seam's rule is about *writable*
/// slots, and a check that refused every host-visible storage binding would
/// pass the first assertion while breaking staging readback. Asserting the
/// acceptance is what tells the two apart.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_writable_storage_binding_refuses_host_visible_memory() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();

    let layout_of = |read_only| {
        device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("one storage buffer"),
                entries: &[crcbl_hal::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: crcbl_hal::ShaderStages::COMPUTE,
                    kind: crcbl_hal::BindingKind::StorageBuffer {
                        read_only,
                        dynamic: false,
                    },
                    count: 1,
                    flags: crcbl_hal::BindingFlags::empty(),
                }],
            })
            .expect("a single storage binding")
    };
    let buffer_of = |memory| {
        device
            .create_buffer(&crcbl_hal::BufferDesc {
                label: Some("storage"),
                size: 256,
                usage: crcbl_hal::BufferUsage::STORAGE,
                memory,
            })
            .expect("a storage buffer")
    };
    let group_of = |layout, buffer| {
        device.create_bind_group(&crcbl_hal::BindGroupDesc {
            label: None,
            layout,
            entries: &[crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::Buffer {
                    buffer,
                    offset: 0,
                    size: crcbl_hal::BindingResource::WHOLE_BUFFER,
                },
            }],
            variable_count: None,
        })
    };

    let writable = layout_of(false);
    let read_only = layout_of(true);
    let upload = buffer_of(crcbl_hal::MemoryLocation::HostUpload);
    let readback = buffer_of(crcbl_hal::MemoryLocation::HostReadback);
    let device_local = buffer_of(crcbl_hal::MemoryLocation::DeviceLocal);

    for (buffer, what) in [(upload, "HostUpload"), (readback, "HostReadback")] {
        let error = group_of(writable, buffer).expect_err("a shader cannot write host memory");
        assert!(
            matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
            "{what}: {error}"
        );
        let text = error.to_string();
        assert!(text.contains("binding 0"), "{what}: {text}");
        assert!(text.contains(what), "{what}: {text}");
        assert!(text.contains("DeviceLocal"), "{what}: {text}");

        // The same buffer in a read-only slot is untouched by the rule.
        let group = group_of(read_only, buffer).expect("a read-only storage binding is fine");
        device.destroy_bind_group(group);
    }

    // …and the writable slot still accepts the memory it is for.
    let group =
        group_of(writable, device_local).expect("device-local memory is what the rule asks for");
    device.destroy_bind_group(group);

    device.destroy_buffer(upload);
    device.destroy_buffer(readback);
    device.destroy_buffer(device_local);
    device.destroy_bind_group_layout(writable);
    device.destroy_bind_group_layout(read_only);

    headless.finish();
}
