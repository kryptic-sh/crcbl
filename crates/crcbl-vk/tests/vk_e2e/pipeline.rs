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
//! Which arm the bindless test takes depends on what the device reports, and
//! both are asserted — which is exactly why this suite is run on radv and on
//! lavapipe rather than on one of them.

use crate::harness::Headless;
use crcbl_hal::{Features, SampleType};

/// The tier story for bind-group layouts, against whatever this machine is.
///
/// The seam requires a device without `DESCRIPTOR_INDEXING` to **reject** a
/// layout that sets any [`BindingFlags`](crcbl_hal::BindingFlags), rather than
/// ignoring it — "a bindless array quietly downgraded to a fixed one reads
/// garbage at index 4097". Which branch runs depends on the driver, and both are
/// asserted, which is exactly why this suite runs on radv and lavapipe.
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
    let ceiling = device.caps().limits.max_sampler_anisotropy;
    let error = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            anisotropy: ceiling + 1.0,
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect_err("anisotropy past the limit must be refused");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
        "{error}"
    );

    headless.finish();
}
