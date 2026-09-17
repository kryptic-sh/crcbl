//! What [`super::Grass`] and [`super::GrassScene`] promise the rest of the crate,
//! held without a GPU where that is possible and on the null backend where it is
//! not.

use super::*;
use crate::grass::field::tests::{flat_cover, flat_ground, one_blade};

/// A two-by-two field of eight-metre tiles over flat ground.
fn field() -> GrassField {
    GrassField::new(
        [2, 2],
        8.0,
        [0.0, 0.0],
        1000.0,
        flat_ground(32, 0.0),
        flat_cover(32, 200),
        one_blade(),
    )
    .expect("a real field")
}

/// A device on the null backend, which records every seam call and checks the
/// obligations without a GPU.
fn null() -> (Box<dyn Device>, crcbl_hal::QueueHandle) {
    use crcbl_hal::{DeviceDesc, Instance, null::NullInstance};
    let instance = NullInstance::gpu_driven();
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc::for_adapter(adapter.id))
        .expect("the null backend opens a device");
    let queue = device
        .queue(crcbl_hal::QueueKind::Graphics)
        .expect("a queue");
    (device, queue)
}

/// [`Grass::PASSES`] is what [`crate::forward`] adds to its pass bound, so it
/// has to be the number of `add_*_pass` calls in [`Grass::add_passes`] —
/// `crate::water`'s test of the same name.
#[test]
fn the_declared_pass_count_is_the_one_the_body_adds() {
    let source = include_str!("../grass.rs");
    let body = source
        .split_once("pub(crate) fn add_passes<'a>(")
        .expect("this file declares `add_passes`")
        .1
        // `"\n    }"` and not `"\n    }\n"`: a Windows checkout reads this file
        // with CRLF endings, and the brace is followed by `\r` there.
        .split_once("\n    }")
        .expect("the function has a body")
        .0;
    let added =
        body.matches(".add_render_pass(").count() + body.matches(".add_compute_pass(").count();
    assert_eq!(added as u32, Grass::PASSES);
    // And none of them is a full-screen triangle, which is what the counters
    // would otherwise have to be told about.
    assert_eq!(body.matches("encoder.draw(").count(), 0);
}

/// **Every binding of both layouts is its shader's, in order**: the constants in
/// [`binding`] and [`gen_binding`] against what the two files declare.
#[test]
fn the_binding_constants_are_the_shaders_declaration_order() {
    let generation = include_str!("../../../crcbl-shaders/shaders/grass_gen.slang");
    for (number, name) in [
        (gen_binding::WIND, "ConstantBuffer<WindParams> wind;"),
        (
            gen_binding::WIND_DIRECTION,
            "Texture2D<float4> windDirectionLayer;",
        ),
        (
            gen_binding::WIND_INTENSITY,
            "Texture2D<float4> windIntensityLayer;",
        ),
        (gen_binding::WIND_SAMPLER, "SamplerState windSampler;"),
        (gen_binding::PARAMS, "ConstantBuffer<GrassGenParams> grass;"),
        (gen_binding::TILE, "ConstantBuffer<GrassTile> tile;"),
        (gen_binding::BLADES, "StructuredBuffer<GrassBlade> blades;"),
        (gen_binding::GROUND, "Texture2D<float> grassGround;"),
        (gen_binding::COVER, "Texture2D<float4> grassCover;"),
        (
            gen_binding::INSTANCES,
            "RWStructuredBuffer<GrassInstance> instances;",
        ),
        (
            gen_binding::ARGS,
            "RWStructuredBuffer<Atomic<uint> > drawArgs;",
        ),
        (
            gen_binding::CELLS,
            "RWStructuredBuffer<GrassInstance> grassCells;",
        ),
    ] {
        let spelled = format!("[[vk::binding({number}, 0)]]\n{name}");
        assert!(
            generation.contains(&spelled),
            "grass_gen.slang does not declare `{name}` at binding {number}"
        );
    }

    let raster = include_str!("../../../crcbl-shaders/shaders/grass.slang");
    for (number, name) in [
        (binding::FRAME, "ConstantBuffer<FrameUniforms> frame;"),
        (binding::PARAMS, "ConstantBuffer<GrassParams> grass;"),
        (binding::TILE, "ConstantBuffer<GrassTile> tile;"),
        (
            binding::INSTANCES,
            "StructuredBuffer<GrassInstance> instances;",
        ),
        (binding::BLADES, "StructuredBuffer<GrassBlade> blades;"),
        (binding::CARD, "Texture2DArray<float4> grassCard;"),
        (binding::CARD_SAMPLER, "SamplerState grassCardSampler;"),
        (binding::SHADOW_ATLAS, "DepthTexture2D shadow_atlas;"),
        (
            binding::SHADOW_SAMPLER,
            "SamplerComparisonState shadow_sampler;",
        ),
        (binding::LIGHTS, "StructuredBuffer<GpuLight> lights;"),
        (
            binding::CLUSTER_LIGHTS,
            "StructuredBuffer<uint> cluster_lights;",
        ),
        (binding::FIELD, "ConstantBuffer<GrassField> field;"),
        (
            binding::CELLS,
            "StructuredBuffer<GrassInstance> grassCells;",
        ),
        (binding::GROUND, "Texture2D<float> grassGround;"),
        (binding::WIND, "ConstantBuffer<WindParams> wind;"),
        (
            binding::WIND_DIRECTION,
            "Texture2D<float4> windDirectionLayer;",
        ),
        (
            binding::WIND_INTENSITY,
            "Texture2D<float4> windIntensityLayer;",
        ),
        (binding::WIND_SAMPLER, "SamplerState windSampler;"),
    ] {
        let spelled = format!("[[vk::binding({number}, 0)]]\n{name}");
        assert!(
            raster.contains(&spelled),
            "grass.slang does not declare `{name}` at binding {number}"
        );
    }
}

/// **A tile block's stride is an alignment every device asks for**, and the
/// block fits inside it.
///
/// A dynamic offset that is not a multiple of the device's alignment is a
/// validation error on every backend, and the stride is written into the buffer
/// before any device is asked what its alignment is — see [`TILE_STRIDE`].
#[test]
fn the_tile_stride_clears_every_devices_alignment() {
    use crcbl_hal::Limits;
    assert!(TILE_SIZE as u64 <= u64::from(TILE_STRIDE));
    for limits in [Limits::minimum(), Limits::desktop()] {
        assert_eq!(
            u64::from(TILE_STRIDE) % limits.min_uniform_buffer_offset_alignment,
            0,
            "a stride of {TILE_STRIDE} is not a multiple of {}",
            limits.min_uniform_buffer_offset_alignment
        );
    }
}

/// **The static block is the field's numbers**, including the reciprocal the two
/// copies of the placement multiply by.
#[test]
fn the_generation_block_carries_the_fields_numbers() {
    let field = field();
    let params = gen_params_of(&field);
    assert_eq!(params.camera[3], field.reach());
    assert_eq!(params.ground[2], field.ground().metres_per_texel);
    assert_eq!(params.ground[3], 1.0 / field.ground().metres_per_texel);
    assert_eq!(params.maps, [32, 32, 32, 32]);
    assert_eq!(
        params.limits,
        [SLOT_CAPACITY, 1, CELLS_PER_TILE, field.slots()]
    );
    // The shell and fin draws' instance counts are the field's stack, whatever
    // its rows' looks — so a look switch leaves this block as it was.
    assert_eq!(
        params.looks,
        [crcbl_shaders::grass::DEFAULT_SHELLS, 1, 0, 0]
    );
    // The mesh blades' switch is the field's, and the default one here.
    assert_eq!(params.lod, [field.blade_lod().distance, 0.0, 0.0, 0.0]);
    // The camera arrives from the view, and nothing else of the block does.
    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    scene
        .set(device, queue, Some(&field))
        .expect("the field uploads");
    let live = scene.frame().expect("a field was set");
    assert_eq!(live.generation, params);
    let moved = live.gen_params(glam::Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(moved.camera, [1.0, 2.0, 3.0, field.reach()]);
    assert_eq!(moved.ground, params.ground);
    scene.destroy(device);
}

/// **A card field records one draw a slot, a shell field three**, a shell field
/// whose fins are off two, and a field with mesh blades two more for their two
/// levels — the count `crate::forward` adds to its recorded draws, and what
/// keeps a card field's command stream rung G1's.
#[test]
fn a_frame_draws_the_looks_its_field_has() {
    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    let looks = |looks: &[BladeLook], fins| {
        let rows = looks
            .iter()
            .map(|look| BladeType {
                look: *look,
                ..one_blade()[0]
            })
            .collect();
        GrassField::new(
            [2, 2],
            8.0,
            [0.0, 0.0],
            1000.0,
            flat_ground(32, 0.0),
            flat_cover(32, 200),
            rows,
        )
        .and_then(|field| field.with_shells(Shells { count: 4, fins }))
        .expect("a real field")
    };
    let shells = |fins| looks(&[BladeLook::Shells], fins);
    for (field, draws) in [
        (field(), 1),
        (shells(true), 3),
        (shells(false), 2),
        (looks(&[BladeLook::Blades], true), 3),
        (looks(&[BladeLook::Cards, BladeLook::Blades], false), 3),
        (looks(&[BladeLook::Shells, BladeLook::Blades], true), 5),
    ] {
        scene
            .set(device, queue, Some(&field))
            .expect("the field uploads");
        let frame = scene.frame().expect("a field was set");
        assert_eq!(frame.draws_per_slot(), draws, "{:?}", field.shells());
    }
    scene.destroy(device);
}

/// **A renderer with no field holds nothing to draw**, and setting one then
/// taking it away comes back to that.
#[test]
fn a_scene_with_no_field_draws_nothing() {
    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    assert!(scene.frame().is_none(), "nothing set, nothing drawn");
    assert!(scene.field().is_none());

    scene
        .set(device, queue, Some(&field()))
        .expect("the field uploads");
    let live = scene.frame().expect("a field was set");
    assert_eq!(live.slots, 4);
    assert_eq!(scene.field().expect("a field was set").slots(), 4);

    scene.set(device, queue, None).expect("removing is a set");
    assert!(scene.frame().is_none(), "the removed field is still drawn");
    assert!(scene.field().is_none());
    // Two frames, so the ring has come round and the retirement is released.
    scene.begin_frame(device);
    scene.begin_frame(device);
    scene.begin_frame(device);
    scene.destroy(device);
}

/// **Removing a field really removes it**, and the frame that follows is the
/// frame a renderer never given one draws — `tests/render_e2e/grass.rs`'s
/// `a_meadow_with_its_field_removed_is_the_meadow_never_given_one` is the pixel
/// half of the same claim.
///
/// **Shown red by sabotage** (2026-09-16, lavapipe): [`GrassScene::set`] made to
/// return early on a [`None`] field, which is the shape an author would write to
/// "skip the work". That e2e then reported 11065 pixels still differing after
/// the removal — every pixel the field had changed.
///
/// **A replaced field's resources outlive the frames that were reading them.**
///
/// The ring is two deep here, so a field replaced at frame zero is released at
/// frame three and not before — which is the whole of what the retirement list
/// is for: a caller replacing a field is the ordinary case, and the frame that
/// was submitted a moment earlier still names those handles.
#[test]
fn a_replaced_field_is_held_until_the_ring_has_come_round() {
    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    scene
        .set(device, queue, Some(&field()))
        .expect("the field uploads");
    let first = scene.frame().expect("a field").instances;

    scene
        .set(device, queue, Some(&field()))
        .expect("the second field uploads");
    let second = scene.frame().expect("a field").instances;
    assert_ne!(first, second, "a replacement reused the old buffers");
    assert_eq!(scene.retired.len(), 1);

    scene.begin_frame(device);
    scene.begin_frame(device);
    assert_eq!(
        scene.retired.len(),
        1,
        "the replacement was released while a frame of the ring could still be reading it"
    );
    scene.begin_frame(device);
    assert!(
        scene.retired.is_empty(),
        "the replacement was never released"
    );
    scene.destroy(device);
}

/// **The wind is calm until a caller sets a layer**, and the placeholder is one
/// texel of exactly that.
#[test]
fn the_wind_is_calm_until_it_is_set() {
    assert_eq!(CALM_INTENSITY[0], 0, "the calm placeholder is not calm");
    // `(255, 128)` is the complex identity, so the prevailing wind is unturned.
    assert_eq!(CALM_DIRECTION[0], 255);
    assert_eq!(CALM_DIRECTION[1], 128);

    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    let calm = scene.wind.intensity.view;
    scene
        .set_wind_layers(
            device,
            queue,
            Some(&WindLayers {
                direction: WindLayer {
                    texels: [2, 2],
                    rgba8: [255, 128, 0, 255].repeat(4),
                },
                intensity: WindLayer {
                    texels: [2, 2],
                    rgba8: [200, 0, 0, 255].repeat(4),
                },
            }),
        )
        .expect("the layers upload");
    assert_ne!(scene.wind.intensity.view, calm);
    scene
        .set_wind_layers(device, queue, None)
        .expect("removing is a set");
    assert_eq!(scene.retired.len(), 2, "both replacements were retired");
    scene.destroy(device);
}

/// A layer that is not four bytes a texel is refused, and the message names both
/// numbers.
#[test]
fn a_misshapen_wind_layer_is_refused() {
    let (device, queue) = null();
    let device = device.as_ref();
    let mut scene = GrassScene::new(device, queue, 2).expect("the placeholders upload");
    let short = WindLayers {
        direction: WindLayer {
            texels: [2, 2],
            rgba8: vec![0; 15],
        },
        intensity: WindLayer {
            texels: [2, 2],
            rgba8: vec![0; 16],
        },
    };
    assert!(matches!(
        scene.set_wind_layers(device, queue, Some(&short)),
        Err(WindLayerError::Size {
            width: 2,
            height: 2,
            expected: 16,
            found: 15,
        })
    ));
    let empty = WindLayers {
        direction: WindLayer {
            texels: [0, 2],
            rgba8: Vec::new(),
        },
        intensity: WindLayer {
            texels: [2, 2],
            rgba8: vec![0; 16],
        },
    };
    assert!(matches!(
        scene.set_wind_layers(device, queue, Some(&empty)),
        Err(WindLayerError::Empty {
            width: 0,
            height: 2
        })
    ));
    scene.destroy(device);
}

/// **The three pipelines build on the null backend**, which is what says the
/// layouts, the pipeline layouts and the two modules agree about their bindings
/// before any real device is asked.
#[test]
fn the_pipelines_build_and_release() {
    let (device, queue) = null();
    let device = device.as_ref();
    let sampler = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            label: Some("shadow"),
            compare: Some(CompareOp::Greater),
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect("a comparison sampler");
    let grass = Grass::new(device, 2, sampler).expect("the pipelines build");
    grass.destroy(device);
    device.destroy_sampler(sampler);
    let _ = queue;
}
