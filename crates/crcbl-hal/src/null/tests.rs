//! Unit tests for the null backend.
//!
//! The "is the seam usable from outside the crate" property is checked by
//! `tests/seam_from_outside.rs` instead — an in-crate test can reach private
//! items and so cannot prove anything about leakage.

use super::*;

use crate::{
    BindGroupLayoutEntry, BindingKind, BindingModel, BufferUsage, ClearValue, ColorAttachment,
    ColorTargetState, DepthStencilState, GeometryPath, ImageSubresourceRange, ImageViewType,
    LightingPath, LoadOp, MultisampleState, PrimitiveState, PushConstantRange, ResourceState,
    SampleType, SemaphoreSignal, ShaderEntry, StoreOp, depth,
};

/// The SPIR-V magic number, so test modules look like modules.
const SPIRV: [u32; 5] = [0x0723_0203, 0x0001_0600, 0, 0, 0];

/// A stand-in DXIL container. The null backend compiles nothing, so only its
/// presence is ever read — but it opens with the container magic so a reader
/// does not mistake it for arbitrary bytes.
const DXIL: &[u8] = b"DXBC\x00";

fn boxed(instance: NullInstance) -> (Recorder, Box<dyn Instance>) {
    let recorder = instance.recorder();
    (recorder, Box::new(instance))
}

/// Opens a device that enables everything the adapter offers.
///
/// `DeviceDesc::for_adapter` alone enables only `GPU_DRIVEN`; optional features
/// must be *asked* for, which is the behaviour a real backend has and the reason
/// this helper spells it out.
fn open(instance: &dyn Instance) -> Box<dyn Device> {
    let adapters = instance.adapters();
    instance
        .create_device(&DeviceDesc {
            optional_features: Features::all(),
            ..DeviceDesc::for_adapter(adapters[0].id)
        })
        .expect("the gpu_driven adapter has compute and a timeline semaphore")
}

#[test]
fn the_two_presets_select_the_documented_paths() {
    let a = NullInstance::gpu_driven();
    let b = NullInstance::portable();
    let (a_caps, b_caps) = (a.adapters()[0].caps, b.adapters()[0].caps);
    assert_eq!(a_caps.binding_model(), BindingModel::Bindless);
    assert_eq!(a_caps.geometry_path(), GeometryPath::IndirectCount);
    assert_eq!(b_caps.binding_model(), BindingModel::ArrayPages);
    assert_eq!(b_caps.geometry_path(), GeometryPath::IndirectPerBatch);
    // Neither preset reports ray tracing, so neither may select it. When a
    // preset grows the flags this is the assertion that has to be revisited.
    assert_eq!(a_caps.lighting_path(), LightingPath::Rasterised);
    assert_eq!(b_caps.lighting_path(), LightingPath::Rasterised);
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
            "the portable preset must not claim {absent:?} — it models WebGPU"
        );
    }
    assert!(
        caps.supports(Features::COMPUTE),
        "GPU culling runs on the portable preset"
    );
}

/// `required` has to be able to fail, and it has to name the gap — a
/// requirement that cannot be refused is not a gate. The portable preset lacks a
/// timeline semaphore, which the headless default does require, so this is the
/// refusal the seam owes with nothing contrived to provoke it.
#[test]
fn the_portable_preset_refuses_the_headless_default_and_names_the_gap() {
    let instance = NullInstance::portable();
    let error = instance
        .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
        .expect_err("the portable preset has no timeline semaphore");
    let HalError::UnsupportedFeatures { missing } = error else {
        panic!("expected UnsupportedFeatures, got {error:?}");
    };
    assert!(missing.contains(Features::TIMELINE_SEMAPHORE));
    assert!(
        !missing.contains(Features::COMPUTE),
        "the portable preset has compute"
    );
    // The bindless half is *optional* now: absent, and not a reason to refuse.
    assert!(
        !missing.contains(Features::DESCRIPTOR_INDEXING),
        "an optional feature must never appear in the gap"
    );
}

#[test]
fn unknown_adapters_are_rejected() {
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    assert!(device.queue(QueueKind::Graphics).is_some());
    assert!(device.queue(QueueKind::Compute).is_some());
    assert!(device.queue(QueueKind::Transfer).is_some());
    assert_ne!(
        device.queue(QueueKind::Graphics),
        device.queue(QueueKind::Compute),
        "distinct families must have distinct handles"
    );

    let instance = NullInstance::portable();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the portable preset has compute");
    assert!(
        device.queue(QueueKind::Graphics).is_some(),
        "the graphics queue always exists"
    );
    assert!(device.queue(QueueKind::Compute).is_none());
    assert!(device.queue(QueueKind::Transfer).is_none());
}

#[test]
fn buffers_round_trip_through_the_host_and_reject_the_wrong_memory() {
    let instance = NullInstance::gpu_driven();
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

/// The bytes a host write left behind are readable, which is what lets a test
/// check the *content* of an upload rather than only its offset and length.
///
/// A packed record written to the wrong offset is caught by
/// [`Event::BufferWritten`]; one written to the right offset with its fields
/// permuted is not, and that is the failure this exists for.
#[test]
fn a_written_buffer_reports_the_bytes_that_landed_in_it() {
    let recorder = Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let device = open(&instance);
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("table"),
            size: 8,
            usage: crate::BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("buffer");

    assert_eq!(
        recorder.buffer_bytes(buffer),
        Some(vec![0u8; 8]),
        "a fresh buffer reads as zeroes, so a slot nothing wrote is \
         distinguishable from one that was written"
    );
    device
        .write_buffer(buffer, 4, &[1, 2, 3, 4])
        .expect("write");
    assert_eq!(
        recorder.buffer_bytes(buffer),
        Some(vec![0, 0, 0, 0, 1, 2, 3, 4]),
        "the write must land at its offset and nowhere else"
    );

    device.destroy_buffer(buffer);
    assert_eq!(
        recorder.buffer_bytes(buffer),
        None,
        "a destroyed buffer holds nothing, rather than the bytes of whatever \
         takes its slot next"
    );
}

#[test]
fn destroyed_handles_are_rejected_rather_than_aliased() {
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: None,
            spirv: &[0xDEAD_BEEF],
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect_err("not SPIR-V");
    assert!(matches!(error, HalError::ShaderCompilation(_)), "{error:?}");
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: None,
                spirv: &[],
                wgsl: None,
                msl: None,
                dxil: &[],
            })
            .is_err()
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("cull"),
                spirv: &SPIRV,
                wgsl: None,
                msl: None,
                dxil: &[],
            })
            .is_ok()
    );
}

/// The null backend compiles nothing, so it accepts any artifact format on its
/// own — including a text one with no SPIR-V beside it, which is a legal
/// descriptor the SPIR-V magic-number check must not reject.
#[test]
fn shader_modules_accept_a_text_artifact_with_or_without_spirv() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("wgsl only"),
                spirv: &[],
                wgsl: Some("@fragment fn main() {}"),
                msl: None,
                dxil: &[],
            })
            .is_ok()
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("msl only"),
                spirv: &[],
                wgsl: None,
                msl: Some("[[fragment]] float4 main() { return 0; }"),
                dxil: &[],
            })
            .is_ok(),
        "MSL alone is a legal descriptor; crcbl-mtl is the backend that reads it"
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("dxil only"),
                spirv: &[],
                wgsl: None,
                msl: None,
                dxil: &[("vertexMain", DXIL)],
            })
            .is_ok(),
        "DXIL alone is a legal descriptor; crcbl-dx12 is the backend that reads it"
    );
    assert!(
        device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("all four"),
                spirv: &SPIRV,
                wgsl: Some("@fragment fn main() {}"),
                msl: Some("[[fragment]] float4 main() { return 0; }"),
                dxil: &[("vertexMain", DXIL)],
            })
            .is_ok()
    );
}

/// A descriptor carrying no artifact at all names the gap rather than producing
/// a module handle no pipeline could use.
#[test]
fn a_shader_module_with_no_artifact_names_the_gap() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("empty.slang"),
            spirv: &[],
            wgsl: None,
            msl: None,
            dxil: &[],
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
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("mesh.slang"),
            spirv: &SPIRV,
            wgsl: Some("@vertex fn vertexMain() {}"),
            msl: Some("[[vertex]] void vertexMain() {}"),
            dxil: &[("vertexMain", DXIL)],
        })
        .expect("every format");
    device.destroy_shader_module(module);
    assert_eq!(
        recorder.shader_modules_created(),
        vec![(Some("mesh.slang".to_string()), ShaderSources::all())]
    );
    recorder.clear();
    assert!(recorder.shader_modules_created().is_empty());
}

/// **A module that offers DXIL must offer it for every stage it is used at, and
/// the refusal names the entry point.**
///
/// A DXIL container is compiled for one entry point, so this is the one
/// artifact format a call site can under-supply: offer the vertex container and
/// not the fragment one and the SPIR-V, WGSL and MSL backends keep drawing
/// while D3D12 has no bytecode for half the pipeline. This backend compiles
/// nothing, so what it checks is the claim — and checking it here is what puts
/// the failure in the no-GPU suite rather than on the one machine with a D3D12
/// driver.
#[test]
fn a_stage_whose_entry_point_has_no_dxil_container_is_refused_by_name() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ui"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("pipeline layout");
    let targets = [ColorTargetState::opaque(Format::Rgba16Float)];
    let pipeline = |module| GraphicsPipelineDesc {
        label: Some("ui compositing"),
        layout,
        vertex: ShaderEntry {
            module,
            entry_point: "vertexMain",
        },
        fragment: Some(ShaderEntry {
            module,
            entry_point: "fragmentMain",
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        color_targets: &targets,
    };
    let module = |dxil| ShaderModuleDesc {
        label: Some("ui.slang"),
        spirv: &SPIRV,
        wgsl: None,
        msl: None,
        dxil,
    };

    let half = device
        .create_shader_module(&module(&[("vertexMain", DXIL)]))
        .expect("one container is still a module");
    let error = device
        .create_graphics_pipeline(&pipeline(half))
        .expect_err("the fragment stage has no container");
    assert!(matches!(error, HalError::ShaderCompilation(_)), "{error:?}");
    let text = error.to_string();
    assert!(text.contains("ui.slang"), "{text}");
    assert!(text.contains("fragmentMain"), "{text}");
    assert!(text.contains("vertexMain"), "{text}");

    // The same pipeline over a module carrying both containers is built, so the
    // refusal above is the missing container and not something else about the
    // descriptor.
    let both = device
        .create_shader_module(&module(&[("vertexMain", DXIL), ("fragmentMain", DXIL)]))
        .expect("module");
    device
        .create_graphics_pipeline(&pipeline(both))
        .expect("both stages have a container");

    // And a module that offered no DXIL at all is held to nothing: it made no
    // claim, and each of the other three artifacts carries every entry point.
    let none = device
        .create_shader_module(&module(&[]))
        .expect("SPIR-V alone is a module");
    device
        .create_graphics_pipeline(&pipeline(none))
        .expect("a module offering no DXIL claims nothing");
}

/// The trap the seam exists to make visible: a layout that asks for
/// push constants on a device without them fails at *creation*, not by silently
/// dropping the writes at record time.
#[test]
fn push_constants_and_bindless_fail_loudly_on_the_portable_preset() {
    let instance = NullInstance::portable();
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::COMPUTE,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the portable preset opens");

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
        .expect("a plain layout is fine without push constants");

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
        .expect_err("the portable preset has no push constants");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    // Bindless flags are refused for the same reason.
    let error = device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("bindless textures"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1 << 16,
                flags: crate::BindingFlags::PARTIALLY_BOUND
                    | crate::BindingFlags::UPDATE_AFTER_BIND
                    | crate::BindingFlags::VARIABLE_COUNT,
            }],
        })
        .expect_err("the portable preset has no descriptor indexing");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    // And so are timeline semaphores.
    let error = device
        .create_semaphore(&SemaphoreDesc {
            label: None,
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect_err("the portable preset has no timeline semaphores");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
}

/// Opens a null device reporting exactly `extra` beyond the `gpu_driven`
/// preset, plus a shader module and a pipeline layout to build pipelines from.
///
/// The preset deliberately omits both mesh flags, so this is what lets one test
/// model a device that has the capability and another model a device that does
/// not — the two sides of `docs/plan/39-capabilities.md`'s rule.
fn mesh_fixture(extra: Features) -> (Box<dyn Device>, ShaderModuleHandle, PipelineLayoutHandle) {
    let mut caps = NullInstance::gpu_driven().adapters()[0].caps;
    caps.features |= extra;
    let instance = NullInstance::new(caps);
    let device = instance
        .create_device(&DeviceDesc {
            optional_features: caps.features,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the preset opens");
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("mesh_shader.slang"),
            spirv: &SPIRV,
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect("module");
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("mesh"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("pipeline layout");
    (device, module, layout)
}

/// A mesh pipeline descriptor over `module`, with `task` optional.
fn mesh_desc<'a>(
    module: ShaderModuleHandle,
    layout: PipelineLayoutHandle,
    task: Option<ShaderEntry<'a>>,
) -> MeshPipelineDesc<'a> {
    MeshPipelineDesc {
        label: Some("mesh triangle"),
        layout,
        task,
        // `mesh_shader.slang`'s own numbers: `[numthreads(3, 1, 1)]` on both
        // mesh entry points, `[numthreads(1, 1, 1)]` on `taskMain`.
        task_workgroup_size: [1, 1, 1],
        mesh: ShaderEntry {
            module,
            entry_point: "meshMain",
        },
        mesh_workgroup_size: [3, 1, 1],
        fragment: Some(ShaderEntry {
            module,
            entry_point: "fragmentMain",
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        color_targets: &[],
    }
}

/// **The capability guard.** A device that does not report `MESH_SHADER` must
/// refuse a mesh pipeline at creation, by name — not accept it and fail at the
/// draw, where the diagnosis is a frame away from the mistake.
///
/// The `gpu_driven` preset omits both mesh flags on purpose, so this is the
/// device the majority of the suite already runs against.
#[test]
fn a_device_without_mesh_shader_refuses_a_mesh_pipeline() {
    let (device, module, layout) = mesh_fixture(Features::empty());
    assert!(!device.caps().supports(Features::MESH_SHADER));
    let error = device
        .create_mesh_pipeline(&mesh_desc(module, layout, None))
        .expect_err("the preset reports no MESH_SHADER");
    let HalError::Unsupported { backend, what } = error else {
        panic!("the refusal must name the capability, not be a generic failure");
    };
    assert_eq!(backend, BackendKind::Null);
    assert!(what.contains("MESH_SHADER"), "{what}");
}

/// A query set of no queries is refused, and refused as a bad descriptor
/// rather than as a missing capability.
///
/// Every backend in the workspace refuses this, each for its own API's reason,
/// and until 2026-08-23 the seam did not say so and nothing checked it — a
/// contract held by four coincidences. The handle such a call would return
/// names a set whose every read is out of range, so the mistake would surface
/// at the read instead of at the descriptor.
///
/// The device is opened *with* the feature, because that is the case with one
/// right answer: without it, backends check the count and the capability in
/// different orders and either refusal is correct.
#[test]
fn a_query_set_of_no_queries_is_refused_as_a_bad_descriptor() {
    let mut caps = NullInstance::gpu_driven().adapters()[0].caps;
    caps.features |= Features::TIMESTAMP_QUERY;
    let instance = NullInstance::new(caps);
    let device = instance
        .create_device(&DeviceDesc {
            optional_features: caps.features,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the preset opens");
    assert!(
        device.caps().supports(Features::TIMESTAMP_QUERY),
        "the fixture must have the feature, or this checks the wrong refusal"
    );

    let desc = |count| QuerySetDesc {
        label: Some("pass timers"),
        kind: QueryKind::Timestamp,
        count,
    };
    let set = device
        .create_query_set(&desc(2))
        .expect("a set of two queries is fine on this device");
    device.destroy_query_set(set);

    let error = device
        .create_query_set(&desc(0))
        .expect_err("a set of no queries must not be created");
    assert!(
        matches!(error, HalError::InvalidDescriptor(_)),
        "a zero count is a bad descriptor, not an absent capability: {error:?}"
    );
}

/// A device that does not report [`Features::COMPUTE`] refuses a compute
/// pipeline, and takes a graphics one from the same module and layout.
///
/// Neither [`NullInstance`] preset can reach this arm — both carry `COMPUTE` —
/// so until this test the refusal was code no run had ever entered, and the bit
/// existed only for a device nothing constructed. The graphics half is what
/// makes it a *targeted* refusal rather than a device that fails at everything:
/// a fixture broken in some unrelated way would satisfy the first assertion on
/// its own.
#[test]
fn a_device_without_compute_refuses_a_compute_pipeline() {
    let mut caps = NullInstance::gpu_driven().adapters()[0].caps;
    caps.features.remove(Features::COMPUTE);
    let instance = NullInstance::new(caps);
    // `DeviceDesc::for_adapter` requires `COMPUTE` outright — it is one of the
    // two flags the engine cannot work without — so the baseline has to be
    // written out here rather than spread from it. That is also why neither
    // preset can reach this arm: nothing in the engine opens such a device.
    let device = instance
        .create_device(&DeviceDesc {
            required_features: Features::TIMELINE_SEMAPHORE,
            optional_features: caps.features,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("a device with no compute still opens");
    assert!(
        !device.caps().supports(Features::COMPUTE),
        "the fixture was meant to withhold COMPUTE"
    );

    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("cull.slang"),
            spirv: &SPIRV,
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect("module");
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("cull"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("pipeline layout");

    let error = device
        .create_compute_pipeline(&ComputePipelineDesc {
            label: Some("cull"),
            layout,
            compute: ShaderEntry {
                module,
                entry_point: "cull_main",
            },
            workgroup_size: [64, 1, 1],
        })
        .expect_err("this device reports no COMPUTE");
    let HalError::Unsupported { backend, what } = error else {
        panic!("the refusal must name the capability, not be a generic failure");
    };
    assert_eq!(backend, BackendKind::Null);
    assert!(what.contains("compute"), "{what}");

    device
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
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &[ColorTargetState::opaque(Format::Rgba16Float)],
        })
        .expect("the refusal is the compute stage's, not the whole device's");
}

/// The two mesh stages are reported separately, so they are refused
/// separately: a device with the mesh stage and no task stage takes a mesh
/// pipeline and refuses one carrying a task entry point.
#[test]
fn the_task_stage_is_refused_on_its_own_flag() {
    let (device, module, layout) = mesh_fixture(Features::MESH_SHADER);
    device
        .create_mesh_pipeline(&mesh_desc(module, layout, None))
        .expect("MESH_SHADER alone is enough for a mesh + fragment pipeline");

    let task = ShaderEntry {
        module,
        entry_point: "taskMain",
    };
    let error = device
        .create_mesh_pipeline(&mesh_desc(module, layout, Some(task)))
        .expect_err("this device has no task stage");
    let HalError::Unsupported { what, .. } = error else {
        panic!("the refusal must name the capability");
    };
    assert!(what.contains("TASK_SHADER"), "{what}");
}

/// One storage-buffer binding visible to `stage`, which is the shape
/// `mesh_shader.slang` pulls its vertices through.
fn visible_to(stage: ShaderStages) -> [BindGroupLayoutEntry; 1] {
    [BindGroupLayoutEntry {
        binding: 0,
        visibility: stage,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: crate::BindingFlags::empty(),
    }]
}

/// The two capability-gated stages, each with a device that has it and a device
/// that does not — so one loop covers both sides of both flags.
///
/// The "lacks it" device for `TASK` still has `MESH_SHADER`, because that is
/// the device the split exists for: a real adapter reports the task stage only
/// alongside the mesh stage, and what is being checked is that the *second*
/// flag is read on its own rather than implied by the first.
const GATED_STAGES: [(ShaderStages, Features, Features, &str); 2] = [
    (
        ShaderStages::MESH,
        Features::MESH_SHADER,
        Features::empty(),
        "MESH_SHADER",
    ),
    (
        ShaderStages::TASK,
        Features::MESH_SHADER.union(Features::TASK_SHADER),
        Features::MESH_SHADER,
        "TASK_SHADER",
    ),
];

/// **The visibility guard**, which is what lets a mesh shader read a buffer at
/// all: a bind-group layout may name a mesh stage, and only on a device that
/// reports it.
///
/// Both halves are load-bearing. Refused on a capable device, the mesh path
/// could only ever draw constants; accepted on an incapable one, the layout
/// carries `VK_SHADER_STAGE_MESH_BIT_EXT` into `vkCreateDescriptorSetLayout`,
/// where the refusal names neither the binding nor the missing capability.
#[test]
fn a_mesh_visible_layout_needs_the_matching_capability() {
    for (stage, with, without, name) in GATED_STAGES {
        let entries = visible_to(stage);
        let desc = BindGroupLayoutDesc {
            label: Some("mesh shader vertices"),
            entries: &entries,
        };

        let (device, ..) = mesh_fixture(with);
        device
            .create_bind_group_layout(&desc)
            .unwrap_or_else(|error| panic!("a device reporting {name} must accept it: {error}"));

        let (device, ..) = mesh_fixture(without);
        let error = device
            .create_bind_group_layout(&desc)
            .expect_err("this device does not report the stage");
        let HalError::Unsupported { backend, what } = error else {
            panic!("the refusal must name the capability, not be a generic failure");
        };
        assert_eq!(backend, BackendKind::Null);
        assert!(what.contains(name), "{what}");
    }
}

/// The same rule on the other surface that names stages: a push-constant range
/// read by a mesh stage.
///
/// Vulkan refuses `VK_SHADER_STAGE_MESH_BIT_EXT` in a `VkPushConstantRange` on
/// a device without the feature exactly as it refuses one in a set layout, so
/// the two are checked together or the layout path is the only one policed.
#[test]
fn a_push_constant_range_naming_a_mesh_stage_needs_the_capability() {
    for (stage, with, without, name) in GATED_STAGES {
        let describe = |stages| PipelineLayoutDesc {
            label: Some("mesh shader constants"),
            bind_group_layouts: &[],
            push_constants: Some(PushConstantRange {
                stages,
                offset: 0,
                size: 16,
            }),
        };

        let (device, ..) = mesh_fixture(with);
        device
            .create_pipeline_layout(&describe(stage))
            .unwrap_or_else(|error| panic!("a device reporting {name} must accept it: {error}"));

        let (device, ..) = mesh_fixture(without);
        let error = device
            .create_pipeline_layout(&describe(stage))
            .expect_err("this device does not report the stage");
        let HalError::Unsupported { what, .. } = error else {
            panic!("the refusal must name the capability");
        };
        assert!(what.contains(name), "{what}");
    }
}

/// The guard must be invisible to everything that came before it: a layout and
/// a push-constant range naming only the stages every device has are accepted
/// on a device with both mesh flags, one of them, and neither.
///
/// This is the assertion that would have caught folding the mesh bits into
/// `ShaderStages::ALL` — which is why the stage set here is `ALL` rather than
/// `VERTEX | FRAGMENT`.
#[test]
fn the_guaranteed_stages_are_unaffected_on_every_device() {
    for extra in [
        Features::empty(),
        Features::MESH_SHADER,
        Features::MESH_SHADER | Features::TASK_SHADER,
    ] {
        let (device, ..) = mesh_fixture(extra);
        let entries = visible_to(ShaderStages::ALL);
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("frame"),
                entries: &entries,
            })
            .unwrap_or_else(|error| panic!("{extra:?} must not disturb a raster layout: {error}"));
        device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("frame"),
                bind_group_layouts: &[layout],
                push_constants: Some(PushConstantRange {
                    stages: ShaderStages::ALL,
                    offset: 0,
                    size: 16,
                }),
            })
            .unwrap_or_else(|error| panic!("{extra:?} must not disturb a raster layout: {error}"));
    }
}

/// The whole path on a device that has both stages: create, bind, dispatch,
/// destroy — and the dispatch lands in the command stream as its own command,
/// the way `draw` and `draw_indexed` do, so a frame's shape stays assertable
/// with no GPU in the room.
///
/// **Both forms of the dispatch**, because they are the two halves of one
/// claim: the CPU-counted one carries its extents and the indirect one carries
/// the buffer and offset a driver would read them from. A renderer that moved
/// its dispatch onto the GPU records the second, and a stream that could only
/// show the first would report the move as a missing draw.
#[test]
fn a_mesh_dispatch_is_recorded_like_any_other_draw() {
    let mut caps = NullInstance::gpu_driven().adapters()[0].caps;
    caps.features |= Features::MESH_SHADER | Features::TASK_SHADER;
    let instance = NullInstance::new(caps);
    let recorder = instance.recorder();
    let device = instance
        .create_device(&DeviceDesc {
            optional_features: caps.features,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the preset opens");
    assert_eq!(
        device.caps().geometry_path(),
        GeometryPath::MeshShader,
        "a device with the flag must select the path the flag is for"
    );
    let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("mesh_shader.slang"),
            spirv: &SPIRV,
            wgsl: None,
            msl: None,
            dxil: &[],
        })
        .expect("module");
    let layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("mesh"),
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("pipeline layout");
    let task = ShaderEntry {
        module,
        entry_point: "taskMain",
    };
    let pipeline = device
        .create_mesh_pipeline(&mesh_desc(module, layout, Some(task)))
        .expect("both stages are reported");
    // Three `u32`s of group counts, which is what the seam documents one
    // argument structure to be.
    let mesh_args = device
        .create_buffer(&BufferDesc {
            label: Some("mesh dispatch args"),
            size: 12,
            usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("buffer");
    recorder.clear();

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
        queue,
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("mesh"),
        color_attachments: &[],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(16, 16),
        timestamp_writes: None,
    });
    // The same `bind_graphics_pipeline` a raster pipeline uses: the seam gives
    // both kinds one handle type, so there is no second bind to get wrong.
    encoder.bind_graphics_pipeline(pipeline);
    encoder.draw_mesh_tasks(3, 2, 1);
    let indirect = DrawIndirect {
        args: mesh_args,
        offset: 12,
        draw_count: 1,
        stride: 12,
    };
    encoder.draw_mesh_tasks_indirect(&indirect);
    encoder.end_render_pass();
    let commands = encoder.finish().expect("recording succeeded");

    assert_eq!(
        recorder.command_names(),
        vec![
            "BeginRenderPass",
            "BindGraphicsPipeline",
            "DrawMeshTasks",
            "DrawMeshTasksIndirect",
            "EndRenderPass",
        ]
    );
    assert!(
        recorder
            .commands()
            .contains(&Command::DrawMeshTasks { x: 3, y: 2, z: 1 }),
        "the workgroup counts must survive into the stream, not merely the \
         command's name: {:?}",
        recorder.commands()
    );
    assert!(
        recorder
            .commands()
            .contains(&Command::DrawMeshTasksIndirect(indirect)),
        "the argument buffer and its offset must survive into the stream too — \
         they are the whole of what the indirect form says: {:?}",
        recorder.commands()
    );

    device.destroy_command_buffer(commands);
    device.destroy_buffer(mesh_args);
    // One destroy for both pipeline kinds, which is the other half of sharing
    // the handle type — a mesh pipeline left in the pool here would show up as
    // a leak at teardown.
    device.destroy_graphics_pipeline(pipeline);
    device.destroy_pipeline_layout(layout);
    device.destroy_shader_module(module);
}

/// A mesh dispatch outside a render pass is the same mistake as a `draw`
/// outside one, and must be reported the same way.
///
/// The indirect form is held to one rule more, and it is the rule that form
/// adds: the argument buffer is the whole of what the call says, so a handle
/// that names nothing is a dispatch reading its group counts from a dead
/// allocation. Both are asserted from one recording, because the null backend
/// records every violation rather than stopping at the first.
#[test]
fn a_mesh_dispatch_outside_a_render_pass_is_refused() {
    let mut caps = NullInstance::gpu_driven().adapters()[0].caps;
    caps.features |= Features::MESH_SHADER;
    let instance = NullInstance::new(caps);
    let device = instance
        .create_device(&DeviceDesc {
            optional_features: caps.features,
            ..DeviceDesc::for_adapter(AdapterId(0))
        })
        .expect("the preset opens");
    let recorder = instance.recorder();
    let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.draw_mesh_tasks(1, 1, 1);
    // Created and destroyed before the call, so the handle is well-formed and
    // names nothing — which is what `need_live` is for and what a handle the
    // test never created could not distinguish.
    let dead = device
        .create_buffer(&BufferDesc {
            label: Some("mesh dispatch args"),
            size: 12,
            usage: BufferUsage::INDIRECT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("buffer");
    device.destroy_buffer(dead);
    encoder.draw_mesh_tasks_indirect(&DrawIndirect {
        args: dead,
        offset: 0,
        draw_count: 1,
        stride: 12,
    });
    let commands = encoder.finish().expect("recorded, and carried past");
    let errors = recorder.validation_errors();
    for command in ["DrawMeshTasks", "DrawMeshTasksIndirect"] {
        assert!(
            errors.iter().any(|error| matches!(
                error,
                ValidationError::OutsidePass {
                    command: outside,
                    expected: "render",
                } if *outside == command
            )),
            "{command} outside a pass went unreported: {errors:?}"
        );
    }
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::DeadHandle {
                command: "DrawMeshTasksIndirect",
                kind: ObjectKind::Buffer,
                ..
            }
        )),
        "{errors:?}"
    );
    device.destroy_command_buffer(commands);
}

/// Vulkan permits a runtime-sized array only on the **highest-numbered** binding
/// of a set, and both backends additionally require it to be the last element of
/// the slice so "the variable binding is `entries.last()`" is a true reading.
/// The seam doc now states both halves; this pins them.
#[test]
fn variable_count_must_be_the_last_and_highest_binding() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let plain = BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler { comparison: false },
        count: 4,
        flags: crate::BindingFlags::empty(),
    };
    let bindless = BindGroupLayoutEntry {
        binding: 1,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
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
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let entry = BindGroupLayoutEntry {
        binding: 3,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler { comparison: false },
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

/// A device with timeline semaphores gets binary acquire/present semaphores;
/// the portable preset, modelling WebGPU's implicit `getCurrentTexture`, gets
/// `None` for both. The renderer's splice is
/// the same code in each case — see [`crate::swapchain`].
#[test]
fn swapchain_acquire_matches_each_presets_sync_model() {
    for (instance, features, expect_semaphores) in [
        (NullInstance::gpu_driven(), Features::GPU_DRIVEN, true),
        (NullInstance::portable(), Features::COMPUTE, false),
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
                    present_id: None,
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
    let instance = NullInstance::gpu_driven();
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
            "a semaphore-carrying ring has a pair per slot, for every slot"
        );
    }

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// The seam's answer for a device that cannot observe presents: the wait is
/// answered rather than refused, it still resolves what it was given, and not
/// even an injected timeout makes it refuse.
///
/// All three halves matter. A refusal would put a branch on every frame of
/// every caller for a condition that cannot change after device creation, and
/// the caller that skipped the branch would turn a missing capability into a
/// failed frame. Answering it without looking at the handle would let a wait on
/// a destroyed swapchain — a real ordering bug in a frame loop — pass unnoticed
/// on the one backend whose whole job is noticing. And
/// [`Recorder::report_present_wait_timeouts`] must not reach past the
/// capability: a device that cannot observe presents cannot observe one being
/// late either, so the report stays owed until a device that claims
/// [`Features::PRESENT_FEEDBACK`] takes it — which is what the second device
/// below is for, and what proves the first one refrained rather than having
/// nothing to refrain from.
#[test]
fn a_present_wait_is_answered_not_refused_and_still_checks_its_swapchain() {
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    assert!(
        !device.caps().features.contains(Features::PRESENT_FEEDBACK),
        "there is no display under this backend, so it must not claim to see one"
    );
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
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

    device
        .wait_until_presented(swapchain, 7, Duration::from_secs(30))
        .expect("a device without the capability answers, it does not refuse");
    assert!(
        recorder.events().contains(&Event::PresentWaited {
            swapchain,
            present_id: 7,
            timed_out: false,
        }),
        "the request is what a test of a caller's pacing reads, so it is recorded"
    );

    // Nor can a timeout be injected onto a device that cannot observe presents:
    // the seam's `Ok` is unconditional there, so the injection is left owed
    // rather than spent by a device that had no business honouring it.
    recorder.report_present_wait_timeouts(1);
    device
        .wait_until_presented(swapchain, 7, Duration::from_secs(30))
        .expect("still no capability, so still no refusal to make");
    let capable = NullInstance::gpu_driven()
        .with_present_feedback()
        .with_recorder(recorder.clone());
    let paced = open(&capable);
    assert!(
        paced.caps().features.contains(Features::PRESENT_FEEDBACK),
        "the builder is what makes the injection above reachable at all"
    );
    // SAFETY: an offscreen target holds no platform pointers.
    let other_surface =
        unsafe { capable.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let other_swapchain = paced
        .create_swapchain(&SwapchainDesc {
            label: None,
            surface: other_surface,
            format: Format::Bgra8UnormSrgb,
            extent: (8, 8),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect("swapchain");
    let lapsed = paced
        .wait_until_presented(other_swapchain, 7, Duration::from_secs(30))
        .expect_err("the report was still owed, so the first device that can honour it does");
    assert!(matches!(lapsed, SurfaceError::Timeout), "{lapsed}");
    paced.destroy_swapchain(other_swapchain);
    capable.destroy_surface(other_surface);

    device.destroy_swapchain(swapchain);
    let error = device
        .wait_until_presented(swapchain, 8, Duration::from_secs(30))
        .expect_err("the swapchain is gone");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidHandle { .. })),
        "{error}"
    );
    instance.destroy_surface(surface);
}

/// **An instance can report a list of adapters, and every id in it works.**
///
/// One adapter is the default and stays the default, because it is what every
/// other test in this workspace has been written against. What matters here is
/// that the list is not the only shape: `crcbl/src/engine.rs` walks it, and a
/// walk over one element has one outcome.
///
/// The ids and the *names* are both asserted, and the names are not decoration:
/// the engine logs the adapter it took, and a list whose entries all read "null
/// adapter" would leave that line unable to say which one that was.
#[test]
fn an_instance_reports_the_adapters_it_was_built_with() {
    let one = NullInstance::gpu_driven();
    let listed = one.adapters();
    assert_eq!(listed.len(), 1, "one adapter is still the default");
    assert_eq!(listed[0].id, AdapterId(0));
    assert_eq!(
        listed[0].name, "null adapter",
        "a lone adapter keeps the preset's plain name"
    );

    let many = NullInstance::gpu_driven().with_adapters(3);
    let listed = many.adapters();
    assert_eq!(
        listed.iter().map(|adapter| adapter.id).collect::<Vec<_>>(),
        vec![AdapterId(0), AdapterId(1), AdapterId(2)],
        "the ids are the indices, which is what `AdapterId` means"
    );
    assert_eq!(
        listed
            .iter()
            .map(|adapter| &*adapter.name)
            .collect::<Vec<_>>(),
        vec!["null adapter #0", "null adapter #1", "null adapter #2"],
        "distinguishable, or the engine's adapter line names nothing"
    );

    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { many.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    for adapter in &listed {
        many.surface_caps(surface, adapter.id)
            .unwrap_or_else(|error| panic!("{:?} serves this surface: {error}", adapter.name));
        many.create_device(&DeviceDesc::for_adapter(adapter.id))
            .unwrap_or_else(|error| panic!("{:?} opens a device: {error}", adapter.name));
    }
    // One past the end is still no adapter at all, on both calls.
    let past = AdapterId(3);
    assert!(
        matches!(
            many.surface_caps(surface, past),
            Err(HalError::NoSuchAdapter(3))
        ),
        "an id off the end of the list must not resolve"
    );
    assert!(matches!(
        many.create_device(&DeviceDesc::for_adapter(past)),
        Err(HalError::NoSuchAdapter(3))
    ));
    many.destroy_surface(surface);

    assert!(
        NullInstance::gpu_driven()
            .with_adapters(0)
            .adapters()
            .is_empty(),
        "a machine with no GPU is a real state, and the engine has an answer for it"
    );
}

/// **A refused adapter never serves a surface, and its neighbours are
/// untouched.**
///
/// The injection `crcbl/src/engine.rs`'s adapter walk exists for: an `Err` from
/// [`Instance::surface_caps`] means "not this one", so the engine's loop is
/// only a loop if some adapter can say it and some other adapter can still say
/// `Ok`. Both halves are asserted here, and the baseline before the injection
/// too — without it a `refuse_surface_on` that did nothing at all would be
/// indistinguishable from one that refused everything, since the assertion
/// below would be reading an adapter that never worked.
///
/// The latch is the third half: [`Recorder::refuse_surface_on`] is permanent by
/// design, so the second ask gets the same answer as the first. A counted
/// refusal would be spent by start-up's own walk and let the rejected adapter
/// answer the engine's next question — the resize-time `surface_caps` — as if
/// it had been serving all along.
#[test]
fn a_refused_adapter_never_serves_a_surface_while_its_neighbours_do() {
    let instance = NullInstance::gpu_driven().with_adapters(2);
    let recorder = instance.recorder();
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    for id in [AdapterId(0), AdapterId(1)] {
        assert!(
            instance.surface_caps(surface, id).is_ok(),
            "the baseline: both adapters serve before anything is injected"
        );
    }

    recorder.refuse_surface_on(AdapterId(0));
    let refused = instance
        .surface_caps(surface, AdapterId(0))
        .expect_err("the refused adapter has no path to this surface");
    assert!(
        matches!(
            refused,
            HalError::Unsupported {
                backend: BackendKind::Null,
                ..
            }
        ),
        "the variant `crcbl-vk` refuses with, so the engine's arm sees the real shape: {refused}"
    );
    assert!(
        instance
            .surface_caps(surface, AdapterId(0))
            .is_err_and(|error| matches!(error, HalError::Unsupported { .. })),
        "latched: asking again does not clear it"
    );

    let served = instance
        .surface_caps(surface, AdapterId(1))
        .expect("the refusal is keyed to an adapter, not to the instance");
    assert!(
        served.preferred_format().is_some(),
        "a serving adapter offers a format, which is what makes the engine take it"
    );

    // Only `surface_caps` refuses. Documented on the injector, and asserted so
    // that widening it later is a decision rather than an accident.
    instance
        .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
        .expect("a device from a refused adapter still opens");
    instance.destroy_surface(surface);
}

/// The out-of-date injection, from the seam's side: while the latch is set,
/// A present needs a frame to present, on the backend whose whole purpose is to
/// model the seam's rules with no driver in the room.
///
/// This was the last of those rules the null backend could not state: it kept a
/// ring cursor but no record of the outstanding acquire, so it had nothing to
/// refuse a present against, while `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` had
/// each been answering "present without a matching `acquire_next_frame`" all
/// along. `a_present_without_an_acquire_is_refused` in
/// `crates/crcbl/tests/hal_seam_e2e.rs` is the same assertions against a real
/// device; this is the half that needs no ICD.
///
/// The reconfigure arm is the one with teeth. A reconfigure reissues the whole
/// ring, so an index acquired before it points into images that no longer
/// exist — presenting it is a use-after-free rather than a stale number.
#[test]
fn a_present_without_a_matching_acquire_is_refused() {
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("every device has a graphics queue");
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
    let desc = |extent| SwapchainDesc {
        label: None,
        surface,
        format: Format::Bgra8UnormSrgb,
        extent,
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    let swapchain = device.create_swapchain(&desc((8, 8))).expect("swapchain");
    let present_now = || {
        device.present(
            queue,
            &PresentInfo {
                swapchain,
                waits: &[],
                present_id: None,
            },
        )
    };
    let refused = |error: SurfaceError, case: &str| {
        let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
            panic!("{case} was refused as the wrong kind of error: {error}");
        };
        assert!(
            message.contains("without a matching acquire_next_frame"),
            "{case}: {message}"
        );
    };

    recorder.clear();
    refused(
        present_now().expect_err("nothing has been acquired yet"),
        "a present before any acquire",
    );
    assert_eq!(
        recorder.events().len(),
        0,
        "a refused present must record nothing: {:?}",
        recorder.events()
    );

    // Acquired, so it goes through — a backend that refused every present would
    // satisfy the assertion above and fail this one.
    device.acquire_next_frame(swapchain).expect("acquire");
    present_now().expect("an acquired frame is one to present");

    // The present took the slot, so the same frame cannot go twice.
    refused(
        present_now().expect_err("that frame has already been presented"),
        "a second present of the same frame",
    );

    // And a reconfigure reissues the ring the index pointed into.
    device.acquire_next_frame(swapchain).expect("acquire");
    device
        .reconfigure_swapchain(swapchain, &desc((16, 16)))
        .expect("reconfigure");
    refused(
        present_now().expect_err("the reconfigure destroyed the frame that was acquired"),
        "a present across a reconfigure",
    );

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// **all three** of the calls a frame makes on a swapchain report
/// [`SurfaceError::OutOfDate`], and only a reconfigure that actually rebuilt
/// something clears it.
///
/// All three, because a resize does not pick one: a frame loop that handles it
/// at acquire and not at present — or the reverse — works on the driver it was
/// written against and stalls on the next one. And a *failed* reconfigure must
/// not clear it, or a caller whose rebuild ran out of memory would go straight
/// back to presenting against a swapchain the surface has outgrown.
#[test]
fn an_out_of_date_swapchain_fails_every_presentation_call_until_a_reconfigure_clears_it() {
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("every device has a graphics queue");
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

    // The control: a swapchain nobody has resized answers all three.
    let frame = device.acquire_next_frame(swapchain).expect("acquire");
    device
        .present(
            queue,
            &PresentInfo {
                swapchain,
                waits: frame.present_semaphore.as_slice(),
                present_id: Some(1),
            },
        )
        .expect("present");
    device
        .wait_until_presented(swapchain, 1, Duration::from_secs(30))
        .expect("wait");

    recorder.clear();
    recorder.report_swapchain_out_of_date();

    let acquire = device
        .acquire_next_frame(swapchain)
        .expect_err("the surface moved under the swapchain");
    assert!(matches!(acquire, SurfaceError::OutOfDate), "{acquire}");
    let present = device
        .present(
            queue,
            &PresentInfo {
                swapchain,
                waits: frame.present_semaphore.as_slice(),
                present_id: Some(2),
            },
        )
        .expect_err("and present is where a resize is usually noticed");
    assert!(matches!(present, SurfaceError::OutOfDate), "{present}");
    let wait = device
        .wait_until_presented(swapchain, 1, Duration::from_secs(30))
        .expect_err("and the pacing wait cannot wait for a frame that will not land");
    assert!(matches!(wait, SurfaceError::OutOfDate), "{wait}");
    assert_eq!(
        recorder.events().len(),
        0,
        "a refused presentation call must record nothing: {:?}",
        recorder.events()
    );

    // The handle is still resolved first, so a swapchain the caller destroyed
    // reports the caller's own bug rather than the resize sitting on top of it.
    let doomed = device.create_swapchain(&desc).expect("swapchain");
    device.destroy_swapchain(doomed);
    let dead = device
        .acquire_next_frame(doomed)
        .expect_err("a destroyed swapchain hands out nothing");
    assert!(
        matches!(dead, SurfaceError::Hal(HalError::InvalidHandle { .. })),
        "an out-of-date latch must not mask a stale handle: {dead}"
    );

    // A reconfigure that failed rebuilt nothing, so it clears nothing.
    recorder.fail_next_reconfigures(1);
    device
        .reconfigure_swapchain(swapchain, &desc)
        .expect_err("the injected fault");
    let still = device
        .acquire_next_frame(swapchain)
        .expect_err("a rebuild that did not happen has not fixed anything");
    assert!(matches!(still, SurfaceError::OutOfDate), "{still}");

    // One that succeeded does.
    device
        .reconfigure_swapchain(swapchain, &desc)
        .expect("reconfigure");
    device
        .acquire_next_frame(swapchain)
        .expect("the rebuilt swapchain matches the surface again");

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// The suboptimal injection, from the seam's side: it reaches
/// [`AcquiredFrame::suboptimal`], every presentation call keeps working while it
/// is owed, and **it runs out**.
///
/// Running out is the half that had to be decided rather than copied. Its
/// sibling [`Recorder::report_swapchain_out_of_date`] latches, because a
/// driver's out-of-date swapchain stays out of date until something rebuilds it
/// and a caller that ignores it has to keep being told. A latch here would
/// behave the other way round: the engine answers a suboptimal frame by
/// reconfiguring, so a latch would have a driven loop rebuild on every frame it
/// ever ran, and a test of that loop would hang instead of failing. A count
/// cannot do that.
#[test]
fn a_suboptimal_acquire_is_counted_out_and_presents_all_the_same() {
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("every device has a graphics queue");
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

    // The control: nothing has moved under this swapchain.
    assert!(
        !device
            .acquire_next_frame(swapchain)
            .expect("acquire")
            .suboptimal,
        "a swapchain nobody has disturbed still matches its surface"
    );

    const OWED: u32 = 2;
    recorder.report_suboptimal_acquires(OWED);
    for spent in 0..OWED {
        let frame = device
            .acquire_next_frame(swapchain)
            .expect("a suboptimal swapchain hands out frames; it does not refuse them");
        assert!(frame.suboptimal, "acquire {spent} of the {OWED} injected");
        let present_id = u64::from(spent) + 1;
        device
            .present(
                queue,
                &PresentInfo {
                    swapchain,
                    waits: frame.present_semaphore.as_slice(),
                    present_id: Some(present_id),
                },
            )
            .expect("and presents them, which is the whole difference from out of date");
        device
            .wait_until_presented(swapchain, present_id, Duration::from_secs(30))
            .expect("a frame that reached the swapchain can be waited for");
    }

    assert!(
        !device
            .acquire_next_frame(swapchain)
            .expect("acquire")
            .suboptimal,
        "the injection is spent; a report that did not run out would reconfigure forever"
    );

    // A refused acquire spends none of it: the out-of-date latch answers before
    // the count is read, so what a caller is owed outlives the resize it is
    // about to handle rather than being eaten by it.
    recorder.report_suboptimal_acquires(1);
    recorder.report_swapchain_out_of_date();
    let refused = device
        .acquire_next_frame(swapchain)
        .expect_err("the surface moved under the swapchain");
    assert!(matches!(refused, SurfaceError::OutOfDate), "{refused}");
    device
        .reconfigure_swapchain(swapchain, &desc)
        .expect("reconfigure");
    assert!(
        device
            .acquire_next_frame(swapchain)
            .expect("acquire")
            .suboptimal,
        "the refused acquire consumed nothing"
    );

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// The present-wait timeout, from the seam's side: a device that claims
/// [`Features::PRESENT_FEEDBACK`] can be made to let a wait lapse, the frame it
/// was waiting on is still there to be acquired and presented, and **it runs
/// out**.
///
/// The capability is half the mechanism. `wait_until_presented` has two legal
/// answers and the flag is what picks between them, so a null device that never
/// claimed the flag could only ever give the immediate one — which is why the
/// timeout arm of every frame loop in this workspace was unreachable until
/// [`NullInstance::with_present_feedback`] existed.
/// [`a_present_wait_is_answered_not_refused_and_still_checks_its_swapchain`]
/// covers the other side, that the flag is load-bearing rather than decorative:
/// the same injection reaches nothing on a device that does not claim it.
///
/// Running out rather than latching is the decision this injector owed. A latch
/// would not spin the way a latched suboptimal report would — the caller's
/// answer to a timeout is to render the frame anyway, so nothing rebuilds and
/// nothing retries — but nothing would clear it either: a resize is cleared by
/// the rebuild that answers it, and no caller has an action that makes a
/// stalled compositor catch up. The seam calls a timeout expected traffic that
/// the next wait catches up from, so the recovery is part of the behaviour, and
/// only a count lets a test watch it happen.
#[test]
fn a_present_wait_times_out_while_it_is_owed_and_is_counted_out() {
    let instance = NullInstance::gpu_driven().with_present_feedback();
    let recorder = instance.recorder();
    let device = open(&instance);
    assert!(
        device.caps().features.contains(Features::PRESENT_FEEDBACK),
        "the injection below reaches nothing on a device that cannot observe presents"
    );
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("every device has a graphics queue");
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

    // The control: nothing has been injected, so the capable device answers
    // exactly as the featureless one did.
    device
        .wait_until_presented(swapchain, 1, Duration::from_secs(30))
        .expect("a wait nobody delayed is answered at once");
    assert_eq!(
        recorder.events().last(),
        Some(&Event::PresentWaited {
            swapchain,
            present_id: 1,
            timed_out: false,
        }),
        "and is recorded as answered, which is what the injected ones are read against"
    );

    const OWED: u32 = 2;
    recorder.report_present_wait_timeouts(OWED);
    for spent in 0..OWED {
        let present_id = u64::from(spent) + 1;
        let lapsed = device
            .wait_until_presented(swapchain, present_id, Duration::from_secs(30))
            .expect_err("a timeout is still owed, so this wait must lapse");
        assert!(
            matches!(lapsed, SurfaceError::Timeout),
            "wait {spent} of the {OWED} injected: {lapsed}"
        );
        assert_eq!(
            recorder.events().last(),
            Some(&Event::PresentWaited {
                swapchain,
                present_id,
                timed_out: true,
            }),
            "the refusal is recorded beside the request, or a caller that renders \
             the frame anyway leaves a stream indistinguishable from never waiting"
        );

        // And the swapchain is untouched by it: a compositor that is behind has
        // not taken the frame loop's images away, which is the whole reason the
        // seam says to render anyway rather than to skip or to fail.
        let frame = device
            .acquire_next_frame(swapchain)
            .expect("a late display is not a swapchain that refuses images");
        device
            .present(
                queue,
                &PresentInfo {
                    swapchain,
                    waits: frame.present_semaphore.as_slice(),
                    present_id: Some(present_id),
                },
            )
            .expect("nor one that refuses presents");
    }

    device
        .wait_until_presented(swapchain, 2, Duration::from_secs(30))
        .expect("the injection is spent, so the next wait catches up");

    // A refused wait spends none of it: the out-of-date latch answers before the
    // count is read, so what a caller is owed outlives the resize rather than
    // being eaten by it — and the refused wait records nothing at all.
    recorder.report_present_wait_timeouts(1);
    recorder.report_swapchain_out_of_date();
    let events = recorder.events().len();
    let refused = device
        .wait_until_presented(swapchain, 2, Duration::from_secs(30))
        .expect_err("the surface moved under the swapchain");
    assert!(matches!(refused, SurfaceError::OutOfDate), "{refused}");
    assert_eq!(
        recorder.events().len(),
        events,
        "a wait that was never answered is not a wait that happened"
    );
    device
        .reconfigure_swapchain(swapchain, &desc)
        .expect("reconfigure");
    let lapsed = device
        .wait_until_presented(swapchain, 2, Duration::from_secs(30))
        .expect_err("the refused wait consumed nothing");
    assert!(matches!(lapsed, SurfaceError::Timeout), "{lapsed}");

    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}

/// The device-loss injection, from the seam's side: it reaches every call that
/// can report it, it does not wear off, and it is not the out-of-band error
/// channel wearing a different hat.
///
/// The last two are the point. `report_device_error` is one-shot and
/// recoverable by contract — `crcbl/src/engine.rs` asserts that taking the error
/// clears it — so a hook that latched *there* would have broken the caller it
/// was meant to leave alone. These are two states with two lifetimes, and the
/// assertions below are what keep them apart.
#[test]
fn a_lost_device_reports_it_from_every_call_that_can_and_never_recovers() {
    let instance = NullInstance::gpu_driven();
    let recorder = instance.recorder();
    let device = open(&instance);
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("every device has a graphics queue");
    // SAFETY: an offscreen target holds no platform pointers.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("surface");
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
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("uploads"),
            size: 64,
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })
        .expect("create");
    let encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    let command_buffer = encoder.finish().expect("an empty command buffer");

    recorder.lose_device("gpu hang: the driver reset the adapter");

    // A handle-taking call, a queue-taking call, the one that takes neither,
    // and a presentation call — the four shapes a caller has.
    let write = device
        .write_buffer(buffer, 0, &[1u8; 4])
        .expect_err("the device is gone");
    let submit = device
        .submit(
            queue,
            &SubmitInfo {
                command_buffers: &[command_buffer],
                waits: &[],
                signals: &[],
            },
        )
        .expect_err("the device is gone");
    let idle = device.wait_idle().expect_err("the device is gone");
    let acquire = device
        .acquire_next_frame(swapchain)
        .expect_err("the device is gone");
    for error in [write, submit, idle] {
        assert!(
            matches!(&error, HalError::DeviceLost(message) if message.contains("gpu hang")),
            "the driver's own words have to survive to the caller: {error}"
        );
    }
    assert!(
        matches!(
            &acquire,
            SurfaceError::Hal(HalError::DeviceLost(message)) if message.contains("gpu hang")
        ),
        "a presentation call reports it in the presentation vocabulary, as `crcbl-vk` does: \
         {acquire}"
    );

    // A queue call naming no work at all still reports it. `vkQueueSubmit` with
    // an empty batch is still a queue operation, and a dead queue cannot
    // perform one — this is the only call whose sole gate is the queue's.
    let bare = device
        .submit(
            queue,
            &SubmitInfo {
                command_buffers: &[],
                waits: &[],
                signals: &[],
            },
        )
        .expect_err("the device is gone");
    assert!(
        matches!(&bare, HalError::DeviceLost(message) if message.contains("gpu hang")),
        "{bare}"
    );

    // It stays lost. `report_device_error` is the one that clears when taken;
    // this is the one that cannot be waited out.
    for attempt in 0..4 {
        assert!(
            device.wait_idle().is_err(),
            "attempt {attempt} succeeded on a device that is gone for good"
        );
    }

    // And it is not the out-of-band channel: nothing was queued there, so a
    // caller draining it hears nothing and learns of the loss from its next
    // real call, which is where a driver puts it.
    assert!(
        device.take_error().is_none(),
        "device loss must not be delivered as a recoverable out-of-band error"
    );

    // Teardown still works, as it does on a real driver: a caller holding
    // objects when the device died must be able to release them.
    let live = recorder.total_live_objects();
    device.destroy_buffer(buffer);
    device.destroy_command_buffer(command_buffer);
    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
    assert!(
        recorder.total_live_objects() < live,
        "a lost device that refuses to let go of its objects leaks every one of them"
    );
}

/// Regression test. `reconfigure_swapchain` used to destroy the old ring before
/// building the new one, so a reconfigure that *failed* left the swapchain
/// naming destroyed images and views — a call that returned `Err` having broken
/// the object it was handed. The seam's promise is that a frame's image and
/// view are usable; a failed call must not be able to break it.
#[test]
fn a_failed_reconfigure_leaves_the_swapchain_usable() {
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
        timestamp_writes: None,
    });
    // A copy inside a pass.
    encoder.clear_buffer(
        crcbl_core::Handle::from_bits(1 << 32).expect("non-zero generation"),
        0,
        4,
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
                command: "ClearBuffer"
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("outer"),
        timestamp_writes: None,
    });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("inner"),
        timestamp_writes: None,
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
            msl: None,
            dxil: &[],
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
            workgroup_size: [64, 1, 1],
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
    encoder.clear_buffer(draw_count, 0, 4);
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
        timestamp_writes: Some(crate::PassTimestampWrites {
            set: timers,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
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
        timestamp_writes: Some(crate::PassTimestampWrites {
            set: timers,
            beginning_of_pass: 2,
            end_of_pass: 3,
        }),
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

    let command_buffer = encoder.finish().expect("finish");
    device
        .submit(queue, &SubmitInfo::new(&[command_buffer]))
        .expect("submit");

    recorder.assert_valid();
    assert_eq!(
        recorder.command_names(),
        vec![
            "ResetQuerySet",
            "ClearBuffer",
            "Barrier",
            "BeginComputePass",
            "BindComputePipeline",
            "BindGroup",
            "Dispatch",
            "EndComputePass",
            "Barrier",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "BindGraphicsPipeline",
            "BindGroup",
            "BindIndexBuffer",
            "DrawIndexedIndirectCount",
            "EndRenderPass",
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
    let instance = NullInstance::gpu_driven();
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

/// **A pass's timestamp pair is checked here, because there is nowhere else
/// left to check it without a GPU.**
///
/// The seam has no free-standing timestamp call: the two queries are named by
/// the pass descriptor, so a mis-aimed pair is a malformed *descriptor* and this
/// recorder is what the render graph's unit suite catches it with. Both rules
/// are WebGPU's own — `beginningOfPassWriteIndex` and `endOfPassWriteIndex` must
/// differ, and both must be queries the set holds — and both would otherwise
/// surface as a browser validation error a frame away from the pass that caused
/// it, or on Vulkan as a pass whose two writes landed in one query and measured
/// nothing.
///
/// Recorded and carried past rather than failing the encoder, which is this
/// recorder's rule for every rule violation: one run reports the whole frame's
/// worth.
#[test]
fn a_passs_timestamp_pair_is_checked_against_the_set_it_names() {
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");
    let timers = device
        .create_query_set(&QuerySetDesc {
            label: Some("timers"),
            kind: QueryKind::Timestamp,
            count: 4,
        })
        .expect("the gpu-driven preset has timestamp queries");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mis-aimed timestamps"),
        queue,
    });
    // Both ends into one query: a pass that measures nothing.
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("coincident"),
        timestamp_writes: Some(crate::PassTimestampWrites {
            set: timers,
            beginning_of_pass: 2,
            end_of_pass: 2,
        }),
    });
    encoder.end_compute_pass();
    // And one past the end of the set.
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("out of range"),
        timestamp_writes: Some(crate::PassTimestampWrites {
            set: timers,
            beginning_of_pass: 0,
            end_of_pass: 4,
        }),
    });
    encoder.end_compute_pass();
    // A legal pair records nothing, or the two above would pass against a
    // recorder that complained about every pair it saw.
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("legal"),
        timestamp_writes: Some(crate::PassTimestampWrites {
            set: timers,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
    });
    encoder.end_compute_pass();
    encoder.finish().expect("a rule violation is not a failure");

    let errors = recorder.validation_errors();
    assert_eq!(
        errors.len(),
        2,
        "exactly the two mis-aimed pairs, and nothing for the legal one: {errors:?}"
    );
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::CoincidentTimestamps {
                command: "BeginComputePass",
                index: 2,
            }
        )),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::TimestampOutOfRange {
                command: "BeginComputePass",
                index: 4,
                count: 4,
            }
        )),
        "{errors:?}"
    );

    device.destroy_query_set(timers);
}

#[test]
fn the_portable_preset_refuses_query_kinds_it_lacks() {
    let instance = NullInstance::portable();
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
                    value: 0
                }],
                0
            )
            .expect("wait"),
        "the timeline was created holding 0, so a wait for 0 is already satisfied"
    );
    device.wait_idle().expect("wait idle");
    assert!(
        recorder
            .events()
            .iter()
            .any(|event| matches!(event, Event::WaitedIdle))
    );
}

/// A host signal moves the timeline, a host wait answers against where it got
/// to, and a signal that would move it backwards is refused rather than
/// accepted.
///
/// **What turns it red.** Dropping the `<=` check makes the second half return
/// `Ok`, which is the whole failure: a timeline that goes backwards leaves every
/// waiter past the higher value asleep on a device that reports nothing wrong.
/// Reporting the value as a constant — which this backend did until
/// [`Device::signal_semaphore`] existed — turns the first half red, and so does
/// ignoring `initial_value`, which is why the semaphore is not created at zero.
///
/// The waits are here rather than in a test of their own because they are what
/// makes the signal observable: a `wait_semaphores` that answered `Ok(true)`
/// unconditionally — which this backend did, on the grounds that nothing is ever
/// outstanding — passes both of them while reading the counter not at all, so
/// the value above and the value below have to be asked for together. Widening
/// the comparison to `>=` turns the `9` case red; narrowing it away turns the
/// `10` case red.
///
/// Refusing with anything but [`HalError::InvalidDescriptor`] is red too: the
/// seam splits "a number you can correct" from "a capability this backend
/// lacks", and a caller branching on `Unsupported` to pick a fallback would take
/// the fallback for a typo.
#[test]
fn a_host_signal_moves_a_timeline_forwards_and_only_forwards() {
    let (_recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frame"),
            kind: SemaphoreKind::Timeline { initial_value: 5 },
        })
        .expect("the gpu_driven preset has timeline semaphores");
    assert_eq!(
        device.semaphore_value(semaphore).expect("value"),
        5,
        "the timeline did not start where it was created"
    );

    device.signal_semaphore(semaphore, 9).expect("forwards");
    assert_eq!(device.semaphore_value(semaphore).expect("value"), 9);

    // The host wait, against the value the timeline actually holds. `0` is the
    // timeout, and it is ignored: nothing here is outstanding, so the answer is
    // immediate in both directions rather than after a wait.
    for reached in [0, 5, 9] {
        assert!(
            device
                .wait_semaphores(
                    &[SemaphoreWait {
                        semaphore,
                        value: reached
                    }],
                    0
                )
                .expect("wait"),
            "the timeline holds 9, so a wait for {reached} has been satisfied"
        );
    }
    assert!(
        !device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore,
                    value: 10
                }],
                u64::MAX
            )
            .expect("a timeout is not an error"),
        "nothing has signalled 10, so the wait timed out — `Ok(false)`, not the \
         `Ok(true)` that would tell a caller a value it will never receive had arrived"
    );

    for backwards in [0, 5, 9] {
        let error = device
            .signal_semaphore(semaphore, backwards)
            .expect_err("a timeline only moves forwards");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "signalling {backwards} over 9: {error:?}"
        );
    }
    assert_eq!(
        device.semaphore_value(semaphore).expect("value"),
        9,
        "a refused signal must leave the counter where it was"
    );

    // A binary semaphore has no value to signal, exactly as it has none to
    // read — and it is the kind every device must still hand out.
    let binary = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("acquire"),
            kind: SemaphoreKind::Binary,
        })
        .expect("WSI acquire needs one on every device");
    let error = device
        .signal_semaphore(binary, 1)
        .expect_err("a binary semaphore carries no value");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
    // And the same from the waiting side, which this backend used to answer
    // `Ok(true)` to. Every real backend refuses it; a null device that reported
    // the wait satisfied taught a caller reading its behaviour that a call no
    // device accepts is one that works.
    let error = device
        .wait_semaphores(
            &[SemaphoreWait {
                semaphore: binary,
                value: 0,
            }],
            0,
        )
        .expect_err("a binary semaphore carries no value to wait for either");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

    device.destroy_semaphore(binary);
    device.destroy_semaphore(semaphore);
    let error = device
        .signal_semaphore(semaphore, 10)
        .expect_err("the handle is dead");
    assert!(
        matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
        "{error:?}"
    );
}

/// A submission's signal advances the timeline it names, so a host wait on that
/// value is then satisfied.
///
/// **The half that separates this backend's model from "the host signal is the
/// only thing that counts".** Nothing here signals from the host at all: the
/// counter moves because a submission said it would, which is the whole of
/// [`Capability::TimelineSemaphore`] — "a backend that hands out a handle whose
/// value never moves has not got this, whatever its return codes say". This
/// backend claims that capability on every preset granting
/// [`Features::TIMELINE_SEMAPHORE`], and until `submit` applied the signals the
/// claim was not true.
///
/// **What turns it red.** Recording [`Event::Submitted`] without applying the
/// signals — what `submit` used to do — leaves the counter at its initial value,
/// so the value and the wait both go red while the event assertion still passes.
/// The binary semaphore rides along to prove the apply *skips* what carries no
/// value rather than refusing it: the swapchain's present semaphore is in every
/// frame's signal list, so a `submit` that refused one would break the engine's
/// frame loop.
#[test]
fn a_submissions_signal_advances_the_timeline_it_names() {
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");
    let timeline = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frame"),
            kind: SemaphoreKind::Timeline { initial_value: 5 },
        })
        .expect("the gpu_driven preset has timeline semaphores");
    let present = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("present"),
            kind: SemaphoreKind::Binary,
        })
        .expect("every device hands one out for WSI");
    assert!(
        !device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore: timeline,
                    value: 9
                }],
                0
            )
            .expect("wait"),
        "nothing has signalled 9 yet, so the wait must not already be satisfied"
    );

    device
        .submit(
            queue,
            &SubmitInfo {
                command_buffers: &[],
                waits: &[],
                signals: &[
                    SemaphoreSignal {
                        semaphore: present,
                        value: 0,
                    },
                    SemaphoreSignal {
                        semaphore: timeline,
                        value: 9,
                    },
                ],
            },
        )
        .expect("a signal-only submission");

    assert_eq!(
        device.semaphore_value(timeline).expect("value"),
        9,
        "the submission signalled 9, and work that never executes has already finished"
    );
    assert!(
        device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore: timeline,
                    value: 9
                }],
                0
            )
            .expect("wait"),
        "a host wait must see the value a submission signalled"
    );
    assert!(
        recorder
            .events()
            .iter()
            .any(|event| matches!(event, Event::Submitted { .. })),
        "advancing the counter must not cost the recorded submission"
    );

    device.destroy_semaphore(present);
    device.destroy_semaphore(timeline);
}

/// A submission whose signal does not move a timeline forwards is refused, and
/// leaves no event behind.
///
/// The rule [`Device::signal_semaphore`] already enforces, applied to the other
/// way a timeline can be driven — otherwise `submit` is the one path on this
/// backend that can move one backwards, and a caller whose values collide finds
/// out on a real driver instead of on the recorder.
///
/// **What turns it red.** Dropping the check turns the refusal assertions red.
/// Applying the signals or pushing [`Event::Submitted`] *before* validating them
/// turns the counter or the event assertion red instead: a call that answered
/// `Err` must leave the stream and the objects as it found them, which is the
/// property `reconfigure_swapchain` puts its own failure first for.
#[test]
fn a_submission_signalling_a_timeline_backwards_is_refused_and_records_nothing() {
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("queue");
    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frame"),
            kind: SemaphoreKind::Timeline { initial_value: 9 },
        })
        .expect("the gpu_driven preset has timeline semaphores");

    for backwards in [0, 5, 9] {
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore,
                        value: backwards,
                    }],
                },
            )
            .expect_err("a timeline only moves forwards");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "a submission signalling {backwards} onto a timeline holding 9: {error:?}"
        );
    }
    assert_eq!(
        device.semaphore_value(semaphore).expect("value"),
        9,
        "a refused submission must leave the counter where it was"
    );
    let events = recorder.events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Submitted { .. })),
        "a refused submission must record nothing: {events:?}"
    );

    device.destroy_semaphore(semaphore);
}

#[test]
fn images_are_validated_against_the_devices_limits() {
    let instance = NullInstance::portable();
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
    let first = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let second = NullInstance::gpu_driven().with_recorder(recorder.clone());

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
    let instance = NullInstance::gpu_driven();
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    encoder.clear_buffer(foreign, 0, 4);
    let _ = encoder.finish().expect("finish");

    let errors = recorder.validation_errors();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::ForeignHandle {
                command: "ClearBuffer",
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
    let instance = NullInstance::gpu_driven();
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
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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

/// **A host-visible buffer cannot fill a binding a shader writes**, and it can
/// still fill every read-only one.
///
/// The seam rule [`MemoryLocation`] states: D3D12's upload and readback heaps
/// admit no unordered access view, so the combination the other three backends
/// permit is one that removes a D3D12 device. `crcbl-dx12` refuses it at the
/// view it would have built; the null backend refuses the class, so a call site
/// meets it in the no-GPU suite instead of on a WARP run.
///
/// The read-only half is the assertion that keeps the refusal honest. Every
/// uniform block and every staged instance and material table in the engine is a
/// host-visible buffer behind a read-only binding, so a check that refused those
/// too would take the engine down rather than the bug.
#[test]
fn a_host_visible_buffer_cannot_fill_a_writable_storage_binding() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);

    let layout_of = |read_only| {
        device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("staged instances"),
                entries: &[BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::StorageBuffer {
                        read_only,
                        dynamic: false,
                    },
                    count: 1,
                    flags: crate::BindingFlags::empty(),
                }],
            })
            .expect("a storage-buffer layout")
    };
    let writable = layout_of(false);
    let readable = layout_of(true);

    let buffer_in = |memory| {
        device
            .create_buffer(&BufferDesc {
                label: Some("instances"),
                size: 64,
                usage: BufferUsage::STORAGE,
                memory,
            })
            .expect("the null backend always allocates")
    };
    let group_of = |layout, buffer| {
        device.create_bind_group(&BindGroupDesc {
            label: None,
            layout,
            entries: &[BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: crate::BindingResource::whole_buffer(buffer),
            }],
            variable_count: None,
        })
    };

    // What the rule forbids, in both host-visible locations.
    for memory in [MemoryLocation::HostUpload, MemoryLocation::HostReadback] {
        let buffer = buffer_in(memory);
        let error =
            group_of(writable, buffer).expect_err("a host-visible buffer bound for writing");
        let text = error.to_string();
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(text.contains("binding 3"), "{memory:?}: {text}");
        assert!(text.contains(&format!("{memory:?}")), "{memory:?}: {text}");
        assert!(text.contains("DeviceLocal"), "{memory:?}: {text}");
        assert!(
            text.contains("ALLOW_UNORDERED_ACCESS"),
            "{memory:?}: {text}"
        );

        // ...and the same buffer through a read-only binding is the engine's
        // own staged-table shape, which must stay legal.
        let group = group_of(readable, buffer).unwrap_or_else(|why| {
            panic!("a read-only binding of a {memory:?} buffer is legal everywhere: {why}")
        });
        device.destroy_bind_group(group);
        device.destroy_buffer(buffer);
    }

    // And the location the rule requires passes through both.
    let device_local = buffer_in(MemoryLocation::DeviceLocal);
    for layout in [writable, readable] {
        let group = group_of(layout, device_local).expect("a device-local storage buffer");
        device.destroy_bind_group(group);
    }

    device.destroy_buffer(device_local);
    device.destroy_bind_group_layout(writable);
    device.destroy_bind_group_layout(readable);
}

/// `update_bind_group` on a layout without `UPDATE_AFTER_BIND` is the error
/// `Device::update_bind_group` promises, and `crcbl-vk` produces. The null
/// backend used to accept it, which made the doc untestable.
#[test]
fn updating_a_group_without_update_after_bind_is_refused() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let slot = |flags| BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::ALL,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
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
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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
    let instance = NullInstance::gpu_driven();
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    let instance = NullInstance::gpu_driven();
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
                    present_id: None,
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
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
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
    let (_recorder, instance) = boxed(NullInstance::gpu_driven());
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

/// A buffer range longer than the slot's limit is refused, in both spellings.
///
/// The two range limits are the ones a caller walks into by accident rather
/// than by asking for something exotic: the uniform ceiling is 64 KiB on the
/// desktop preset, small enough that an ordinary table outgrows it, and the
/// buffer itself is created without complaint because a buffer that size is
/// perfectly legal — binding *all of it* to a uniform slot is what is not.
///
/// Both spellings are checked because `WHOLE_BUFFER` is the commoner one and
/// carries no number to compare: a check reading only explicit sizes would pass
/// every `whole_buffer` binding, which is most of them. The `offset` case is
/// what shows the sentinel is resolved against what is actually bound — one
/// byte in, the same over-large buffer fits.
#[test]
fn a_buffer_range_over_the_slots_limit_is_refused() {
    let instance = NullInstance::gpu_driven();
    let device = open(&instance);
    let limits = device.caps().limits;

    for (kind, ceiling, named) in [
        (
            BindingKind::UniformBuffer { dynamic: false },
            limits.max_uniform_buffer_range,
            "max_uniform_buffer_range",
        ),
        (
            BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            limits.max_storage_buffer_range,
            "max_storage_buffer_range",
        ),
    ] {
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("one buffer"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    kind,
                    count: 1,
                    flags: crate::BindingFlags::empty(),
                }],
            })
            .expect("a single-buffer layout");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("one byte too long"),
                size: ceiling + 1,
                usage: BufferUsage::UNIFORM | BufferUsage::STORAGE,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer of any size is legal; binding all of it is not");
        let group_of = |offset, size| {
            device.create_bind_group(&BindGroupDesc {
                label: None,
                layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: crate::BindingResource::Buffer {
                        buffer,
                        offset,
                        size,
                    },
                }],
                variable_count: None,
            })
        };

        for (case, offset, size) in [
            ("whole buffer", 0, crate::BindingResource::WHOLE_BUFFER),
            ("explicit size", 0, ceiling + 1),
        ] {
            let error = group_of(offset, size).expect_err("a range over the ceiling");
            let text = error.to_string();
            assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
            assert!(text.contains("binding 0"), "{case}: {text}");
            assert!(text.contains(named), "{case}: {text}");
            assert!(text.contains(&(ceiling + 1).to_string()), "{case}: {text}");
            assert!(text.contains(&ceiling.to_string()), "{case}: {text}");
        }

        // Exactly the ceiling is allowed, by either spelling.
        group_of(1, crate::BindingResource::WHOLE_BUFFER)
            .expect("one byte in, the rest of the buffer is exactly the ceiling");
        group_of(0, ceiling).expect("a range of exactly the ceiling");
    }
}

/// **An indirect draw is held to the seam's argument rules, all three of them,
/// with no device in the room.**
///
/// [`crate::indirect`] states them once — a four-byte-aligned offset, a stride
/// no smaller than one argument structure, and structures that fit inside the
/// buffer — and this backend is the only one that can answer all three in a
/// unit test: it recorded the buffer's size when it created it, where
/// `crcbl-vk` needs a device and `crcbl-webgpu`'s encoder cannot reach a length
/// at all. None of the three is a mistake an API reports: a misaligned indirect
/// offset is `VUID-vkCmdDrawIndirect-offset-02710`, which has no error code —
/// the driver reads the arguments from the wrong bytes and draws something else.
///
/// **The legal draws are half the test.** A check that refused every indirect
/// draw would satisfy the refusals below and break every frame in the
/// workspace, so the accepting side runs first: a multi-draw at a padded
/// stride and a one-draw caller's `stride: 0`, which the seam accepts because
/// a single draw never strides.
///
/// **What turns it red.** Dropping the check from `draw_indirect` or
/// `draw_indexed_indirect` turns its refusals green; using
/// [`structure_bytes`](crate::structure_bytes)`(false)` for the indexed form
/// accepts a 16-byte stride that skips a word of every structure after the
/// first; measuring the buffer by its recorded `bytes` rather than its `size`
/// refuses the legal draws too, because a device-local buffer keeps no
/// contents.
#[test]
fn an_indirect_draw_is_held_to_the_seams_argument_rules() {
    let (recorder, instance) = boxed(NullInstance::gpu_driven());
    let device = open(instance.as_ref());
    let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
    let args = device
        .create_buffer(&BufferDesc {
            label: Some("draw args"),
            size: 64,
            usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("buffer");
    // Attachment-less: this test is about the arguments, and a pass with no
    // views to resolve keeps the recorded stream to the draws themselves.
    let pass = RenderPassDesc {
        label: Some("indirect"),
        color_attachments: &[],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(16, 16),
        timestamp_writes: None,
    };

    recorder.clear();
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.begin_render_pass(&pass);
    // Two structures 32 bytes apart from offset 4: a padded stride, which is
    // what `Capability::IndirectArgumentPaddedStride` promises is honoured
    // rather than tightened. Last byte read is 4 + 32 + 16 = 52, inside 64.
    encoder.draw_indirect(&DrawIndirect {
        args,
        offset: 4,
        draw_count: 2,
        stride: 32,
    });
    // One draw never strides, so `stride: 0` is legal and never told to an API.
    encoder.draw_indexed_indirect(&DrawIndirect {
        args,
        offset: 0,
        draw_count: 1,
        stride: 0,
    });
    encoder.end_render_pass();
    encoder.finish().expect("both draws obey the seam's rules");
    recorder.assert_valid();
    assert_eq!(
        recorder.command_names(),
        vec![
            "BeginRenderPass",
            "DrawIndirect",
            "DrawIndexedIndirect",
            "EndRenderPass",
        ]
    );

    // One encoder per refusal: the first hard failure is the one `finish`
    // reports, so a second on the same encoder would be masked.
    for (what, command, indexed, draw) in [
        (
            "an offset that is not a multiple of four",
            "DrawIndirect",
            false,
            DrawIndirect {
                args,
                offset: 2,
                draw_count: 1,
                stride: 16,
            },
        ),
        (
            "a stride below one 16-byte structure",
            "DrawIndirect",
            false,
            DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 12,
            },
        ),
        (
            "a stride below one 20-byte indexed structure",
            "DrawIndexedIndirect",
            true,
            DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 16,
            },
        ),
        (
            "two structures 32 bytes apart from offset 32 of a 64-byte buffer",
            "DrawIndirect",
            false,
            DrawIndirect {
                args,
                offset: 32,
                draw_count: 2,
                stride: 32,
            },
        ),
    ] {
        recorder.clear();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_render_pass(&pass);
        if indexed {
            encoder.draw_indexed_indirect(&draw);
        } else {
            encoder.draw_indirect(&draw);
        }
        encoder.end_render_pass();

        let Err(HalError::InvalidDescriptor(reported)) = encoder.finish() else {
            panic!("{what} must fail the finish with an InvalidDescriptor");
        };
        let errors = recorder.validation_errors();
        let Some(ValidationError::InvalidIndirectArguments {
            command: recorded_command,
            message,
        }) = errors
            .iter()
            .find(|error| matches!(error, ValidationError::InvalidIndirectArguments { .. }))
        else {
            panic!("{what} must be recorded as well as reported: {errors:?}");
        };
        assert_eq!(*recorded_command, command, "{what}");
        assert_eq!(
            *message, reported,
            "{what}: `finish` reports the seam's own message, unchanged"
        );
        // A `finish` that fails publishes no stream at all — it returns before
        // the commands reach the recorder — so the refusal above is the whole
        // of what a test can read back, which is why it is recorded and not
        // only returned.
        assert!(
            recorder.command_names().is_empty(),
            "{what}: a failed finish commits nothing"
        );
    }

    device.destroy_buffer(args);
}
