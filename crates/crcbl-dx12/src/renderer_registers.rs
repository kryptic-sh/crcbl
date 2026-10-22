//! **Every register a root signature built from the renderer's layouts names is
//! the register every container of that pipeline declares.**
//!
//! Nothing in D3D12 reports a disagreement. A root signature whose range puts a
//! texture descriptor at the register a stage reads a structured buffer from is
//! accepted by pipeline creation, and the stage reads the wrong descriptor —
//! without a debug-layer message unless GPU-based validation is on, and
//! without a DRED breadcrumb, because the fault is inside the shader. So the
//! check has to be made where both halves are in hand, and on a machine with no
//! D3D12 at all: the renderer is run against the null backend's recorder, which
//! logs every pipeline with its layout's sets and each stage's DXIL container,
//! and each layout is assigned its registers by this crate's own rule and held
//! to each container's `PSV0` resource table.
//!
//! The comparison is [`crate::registers`]', the one `crate::pipeline` refuses a
//! pipeline on at creation. This runs it over every pipeline the renderer
//! builds on every geometry path, so a renderer layout that would be refused on
//! a device is a red test on any host first.
//!
//! Host-only, and therefore on every CI leg, rather than a Windows run away.

use crcbl_hal::null::{NullInstance, PipelineRecord, Recorder};
use crcbl_hal::{
    DeviceCaps, DeviceDesc, Features, Format, Instance, Limits, PushConstantRange, QueueKind,
};

use crate::dxil::psv_resources;
use crate::registers::{self, LayoutRegisters};
use crate::root;

/// Every binding of every set of one pipeline layout and its push-constant
/// block, placed by the rule `crate::device`'s `create_pipeline_layout` runs.
fn place(
    sets: &[Vec<crcbl_hal::BindGroupLayoutEntry>],
    push_constants: Option<PushConstantRange>,
) -> LayoutRegisters {
    let caps = DeviceCaps {
        features: Features::PUSH_CONSTANTS | Features::MESH_SHADER | Features::TASK_SHADER,
        limits: Limits {
            max_push_constant_size: root::MAX_PUSH_CONSTANT_BYTES,
            ..Limits::desktop()
        },
    };
    let mut bindings = Vec::new();
    for (set, entries) in sets.iter().enumerate() {
        let space = root::space_of(set).expect("every renderer layout has few sets");
        let placed = registers::place_set(entries, space, &caps.limits);
        let reduced: Vec<root::Binding> = placed
            .iter()
            .map(|placed| root::Binding {
                binding: placed.binding,
                class: placed.class,
                declared: placed.declared,
            })
            .collect();
        root::check_registers(&reduced)
            .unwrap_or_else(|error| panic!("set {set} is a layout this backend refuses: {error}"));
        bindings.extend(placed);
    }
    let push_constants = root::plan_push_constants(push_constants, &caps)
        .expect("every renderer push-constant range is one D3D12 can express")
        .map(|constants| (constants.space, constants.register));
    LayoutRegisters {
        bindings,
        push_constants,
    }
}

/// Every disagreement between one pipeline's layout and one stage's container,
/// as sentences naming both sides — the comparison
/// `crate::pipeline` refuses a pipeline on.
fn disagreements(pipeline: &PipelineRecord, entry_point: &str, dxil: &[u8]) -> Vec<String> {
    let layout = place(&pipeline.sets, pipeline.push_constants);
    let name = pipeline.label.as_deref().unwrap_or("<unlabelled>");
    layout
        .disagreements(&psv_resources(dxil))
        .into_iter()
        .map(|found| format!("pipeline `{name}`, `{entry_point}` {found}"))
        .collect()
}

/// A null device offering `features`, recording into `recorder`.
fn open(
    recorder: &Recorder,
    features: Features,
) -> (Box<dyn crcbl_hal::Device>, crcbl_hal::QueueHandle) {
    let caps = DeviceCaps {
        features,
        limits: Limits::desktop(),
    };
    let instance = NullInstance::new(caps).with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::COMPUTE,
            optional_features: features,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the null backend always has a graphics queue");
    (device, queue)
}

/// The label [`on_demand`] gives the skinning pass it builds, which names its
/// pipeline.
const SKINNING_LABEL: &str = "renderer registers skinning";

/// Every pipeline label that only [`on_demand`] makes the renderer build, so the
/// test can tell a pipeline that stopped being built from one that agreed.
///
/// `forward probes` is the label `crcbl_render::ForwardRenderer` gives its
/// irradiance-probe gather.
const ON_DEMAND_LABELS: [&str; 5] = [
    "forward probes",
    "probe capture",
    "probe octahedral",
    "debug draw",
    SKINNING_LABEL,
];

/// A forward renderer and a skinning pass that, between them, have built the
/// pipelines below, which `crcbl-render` makes only once a caller asks for
/// them — each through the call a game makes:
///
/// - the probe gather, built by `with_scene` for a volume that asks for
///   [`ProbeUpdate::EveryFrame`](crcbl_render::ProbeUpdate::EveryFrame);
/// - the probe capture and octahedral resolve, built by
///   `capture_probe_visibility` over a placed instance;
/// - the debug-draw pipeline, built by the first `begin_frame` that has a
///   segment while `r_debug_draw` is on, a switch this function leaves to its
///   caller;
/// - the skinning dispatch, built by `Skinning::new` against the renderer's
///   own vertex pool.
fn on_demand(
    device: &dyn crcbl_hal::Device,
    queue: crcbl_hal::QueueHandle,
    target: Format,
    shape: &str,
) -> (crcbl_render::ForwardRenderer, crcbl_render::Skinning) {
    use crcbl_render::scene::{DEMO_CUBE, DEMO_UNTINTED};
    use crcbl_shaders::probe::{GpuProbe, ProbeSteps, ProbeVolume};

    let mut scene = crcbl_render::scene::demo();
    scene.capacities.probes = 1;
    scene.probes = crcbl_render::ProbeGrid {
        volume: ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts: [1, 1, 1],
            levels: 1,
            steps: ProbeSteps::default(),
        },
        probes: vec![GpuProbe::ZERO],
        update: crcbl_render::ProbeUpdate::EveryFrame,
    };
    let mut forward = crcbl_render::ForwardRenderer::with_scene(device, queue, target, &scene)
        .unwrap_or_else(|error| panic!("{shape}: a probe-updating renderer builds: {error}"));

    // A capture needs something placed for its maps to be about.
    forward
        .add_instance(&crcbl_render::InstanceDesc {
            mesh: DEMO_CUBE,
            material: DEMO_UNTINTED,
            transform: Default::default(),
        })
        .unwrap_or_else(|error| panic!("{shape}: the cube is placed: {error}"));
    forward
        .capture_probe_visibility(device, queue)
        .unwrap_or_else(|error| panic!("{shape}: the probe capture runs: {error}"));

    forward
        .debug_draw()
        .sphere(Default::default(), 1.0, [1.0; 4]);
    forward
        .begin_frame(
            device,
            &crcbl_render::Camera::default(),
            &crcbl_render::DirectionalLight::default(),
            (64, 48),
        )
        .unwrap_or_else(|error| panic!("{shape}: a debug-drawing frame begins: {error}"));

    let skinned = forward
        .reserve_skinned(DEMO_CUBE)
        .unwrap_or_else(|error| panic!("{shape}: the cube is reserved for skinning: {error}"));
    let skinning = crcbl_render::Skinning::new(
        device,
        &crcbl_render::SkinningDesc {
            label: Some(SKINNING_LABEL),
            frames: crcbl_render::forward::FRAMES_IN_FLIGHT,
            ranges: 1,
            joints: 1,
            bindings: skinned.vertex_count(),
            vertices: forward.vertex_buffer(),
            attribute_base: forward.attribute_base(),
        },
    )
    .unwrap_or_else(|error| panic!("{shape}: the skinning pass builds: {error}"));
    forward.release_skinned(skinned);
    (forward, skinning)
}

/// **The renderer's layouts agree with every container they are used with, on
/// every geometry path.**
///
/// Each device shape builds a different set of pipelines from the same
/// layouts: the mesh path with and without its task stage, and the indirect
/// raster path a device without mesh shading takes. The mesh path is the one
/// that matters most — its pipeline takes its task and mesh stages from
/// `mesh_cluster.slang` and its fragment stage from `mesh.slang`, and one root
/// signature serves all three.
///
/// The pipelines the renderer builds only on demand — see [`on_demand`] — are
/// built on every shape too, so their layouts are held to their containers here
/// rather than first on a device.
#[test]
fn every_renderer_layout_agrees_with_every_container_it_is_used_with() {
    let target = Format::Rgba8UnormSrgb;
    let mesh = Features::GPU_DRIVEN | Features::MESH_SHADER;
    let shapes = [
        ("mesh path with a task stage", mesh | Features::TASK_SHADER),
        ("mesh path without a task stage", mesh),
        ("indirect raster path", Features::GPU_DRIVEN),
    ];
    let mut failures = Vec::new();
    let mut checked: Vec<(String, String)> = Vec::new();
    // Process-global, and nothing else in this crate's tests reads it; put back
    // below.
    crcbl_render::debug_draw::r_debug_draw
        .set(&crcbl_console::Value::Bool(true))
        .expect("`r_debug_draw` is a writable bool");
    for (shape, features) in shapes {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder, features);
        let forward = crcbl_render::ForwardRenderer::new(device.as_ref(), queue, target)
            .unwrap_or_else(|error| panic!("{shape}: the forward renderer builds: {error}"));
        let sprites =
            crcbl_render::sprite_pass::SpriteRenderer::new(device.as_ref(), queue, target)
                .unwrap_or_else(|error| panic!("{shape}: the sprite renderer builds: {error}"));
        let ui = crcbl_render::ui_pass::UiRenderer::new(device.as_ref(), queue, target)
            .unwrap_or_else(|error| panic!("{shape}: the UI renderer builds: {error}"));
        let grid = crcbl_render::grid::Grid::new(device.as_ref(), 2, target, Format::D32Float)
            .unwrap_or_else(|error| panic!("{shape}: the grid builds: {error}"));
        let (configured, skinning) = on_demand(device.as_ref(), queue, target, shape);

        let pipelines = recorder.pipelines_created();
        assert!(!pipelines.is_empty(), "{shape}: no pipeline was recorded");
        for pipeline in &pipelines {
            for stage in &pipeline.stages {
                let dxil = stage.dxil.as_deref().unwrap_or_else(|| {
                    panic!(
                        "{shape}: pipeline {:?} offers no DXIL for `{}`, so D3D12 could not build it",
                        pipeline.label, stage.entry_point
                    )
                });
                failures.extend(
                    disagreements(pipeline, &stage.entry_point, dxil)
                        .into_iter()
                        .map(|failure| format!("{shape}: {failure}")),
                );
                checked.push((
                    pipeline.label.clone().unwrap_or_default(),
                    stage.entry_point.clone(),
                ));
            }
        }
        drop((forward, sprites, ui, grid, configured, skinning));
    }
    crcbl_render::debug_draw::r_debug_draw
        .set(&crcbl_console::Value::Bool(false))
        .expect("`r_debug_draw` is a writable bool");

    // The case this test exists for has to be among what it checked, or a
    // renderer that stopped building its mesh pipeline would pass it vacuously.
    for entry_point in [
        "taskMain",
        "amplifiedMeshMain",
        "meshMain",
        "fragmentMain",
        "vertexMain",
    ] {
        assert!(
            checked.iter().any(|(_, checked)| checked == entry_point),
            "no pipeline ran `{entry_point}`, so the geometry paths this test is about went \
             unchecked"
        );
    }
    // Likewise each pipeline built only on demand, or a renderer that stopped
    // building one on the call `on_demand` makes would pass this vacuously.
    for label in ON_DEMAND_LABELS {
        assert!(
            checked.iter().any(|(checked, _)| checked == label),
            "no pipeline labelled `{label}` was checked, so a pipeline the renderer \
             builds only on demand went unchecked"
        );
    }
    assert!(
        failures.is_empty(),
        "{} register disagreement(s) between the renderer's layouts and its containers:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
