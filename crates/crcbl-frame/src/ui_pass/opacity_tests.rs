//! Opacity groups through the UI pass: the plan of passes, and what a frame
//! with a group records on the null backend.

use super::opacity::{Draw, Pass, Source, Target, plan};
use super::*;
use crcbl_hal::null::{Command, Event, NullInstance, Recorder};
use crcbl_hal::{CommandEncoderDesc, DeviceDesc, Features, Instance, QueueKind};
use crcbl_ui::draw_list::ClipRect;
use crcbl_ui::image::TextureId;

/// A run over `indices` at a half, with bounds nothing reads here.
fn group(indices: Range<u32>) -> OpacityRun {
    OpacityRun {
        indices,
        opacity: 0.5,
        bounds: ClipRect::NONE,
    }
}

fn own(indices: Range<u32>) -> Draw {
    Draw {
        indices,
        source: Source::Indices(None),
    }
}

/// The draw that blends group `n`'s layer back, when the quads start at
/// `quads`.
fn quad(n: usize, quads: u32) -> Draw {
    let start = quads + n as u32 * 6;
    Draw {
        indices: start..start + 6,
        source: Source::Layer(n),
    }
}

fn pass(target: Target, clear: bool, draws: Vec<Draw>) -> Pass {
    Pass {
        target,
        clear,
        draws,
    }
}

/// **No group, one pass**: the plan of a half with no group in it is the one
/// pass and the draws it always was, onto the frame and never cleared.
#[test]
fn a_half_with_no_group_is_one_pass_of_its_draws() {
    let runs = [TextureRun {
        texture: TextureId::new(3),
        indices: 6..12,
    }];
    assert_eq!(
        plan(0..18, &runs, &[], 18),
        [pass(
            Target::Frame,
            false,
            vec![
                own(0..6),
                Draw {
                    indices: 6..12,
                    source: Source::Indices(Some(TextureId::new(3))),
                },
                own(12..18),
            ]
        )]
    );
    assert!(
        plan(0..0, &runs, &[], 0).is_empty(),
        "an empty half draws nothing"
    );
}

/// **A group cuts its half in three**: what is before it onto the frame, the
/// group into its cleared layer, then the layer blended back and what follows.
#[test]
fn a_group_is_drawn_into_its_layer_between_two_frame_passes() {
    assert_eq!(
        plan(0..18, &[], &[group(6..12)], 18),
        [
            pass(Target::Frame, false, vec![own(0..6)]),
            pass(Target::Layer(0), true, vec![own(6..12)]),
            pass(Target::Frame, false, vec![quad(0, 18), own(12..18)]),
        ]
    );
}

/// **A group at the very start or end adds no empty pass**: nothing before
/// it, nothing after it.
#[test]
fn a_group_at_either_end_adds_no_empty_pass() {
    assert_eq!(
        plan(0..6, &[], &[group(0..6)], 6),
        [
            pass(Target::Layer(0), true, vec![own(0..6)]),
            pass(Target::Frame, false, vec![quad(0, 6)]),
        ]
    );
}

/// **A nested group is a layer inside its parent's**: drawn and blended back
/// into the parent's layer before the parent is blended onto the frame, and
/// the parent's layer cleared once, by its first pass, even when that pass is
/// the nested group's blend.
#[test]
fn a_nested_group_is_blended_into_its_parents_layer() {
    let groups = [group(0..18), group(6..12)];
    assert_eq!(
        plan(0..24, &[], &groups, 24),
        [
            pass(Target::Layer(0), true, vec![own(0..6)]),
            pass(Target::Layer(1), true, vec![own(6..12)]),
            pass(Target::Layer(0), false, vec![quad(1, 24), own(12..18)]),
            pass(Target::Frame, false, vec![quad(0, 24), own(18..24)]),
        ]
    );
    let groups = [group(0..12), group(0..6)];
    assert_eq!(
        plan(0..12, &[], &groups, 12),
        [
            pass(Target::Layer(1), true, vec![own(0..6)]),
            pass(Target::Layer(0), true, vec![quad(1, 12), own(6..12)]),
            pass(Target::Frame, false, vec![quad(0, 12)]),
        ]
    );
}

/// **Siblings are layers one after the other**, the first blended back
/// before the second is drawn — so the graph can alias the two layers — and
/// the next sibling after a nested group is found past its descendants.
#[test]
fn siblings_follow_one_another_past_each_ones_descendants() {
    let groups = [group(0..12), group(6..12), group(12..18)];
    assert_eq!(
        plan(0..18, &[], &groups, 18),
        [
            pass(Target::Layer(0), true, vec![own(0..6)]),
            pass(Target::Layer(1), true, vec![own(6..12)]),
            pass(Target::Layer(0), false, vec![quad(1, 18)]),
            pass(Target::Frame, false, vec![quad(0, 18)]),
            pass(Target::Layer(2), true, vec![own(12..18)]),
            pass(Target::Frame, false, vec![quad(2, 18)]),
        ]
    );
}

/// **A half plans only its own groups**: the HUD half's group is not the
/// overlay's, and the overlay's is not the HUD's.
#[test]
fn each_half_plans_only_its_own_groups() {
    let groups = [group(0..6), group(12..18)];
    assert_eq!(
        plan(0..12, &[], &groups, 18),
        [
            pass(Target::Layer(0), true, vec![own(0..6)]),
            pass(Target::Frame, false, vec![quad(0, 18), own(6..12)]),
        ]
    );
    assert_eq!(
        plan(12..18, &[], &groups, 18),
        [
            pass(Target::Layer(1), true, vec![own(12..18)]),
            pass(Target::Frame, false, vec![quad(1, 18)]),
        ]
    );
}

/// **A texture run inside a group is still a draw of its own**, in the layer.
#[test]
fn a_texture_run_inside_a_group_is_split_in_its_layer() {
    let runs = [TextureRun {
        texture: TextureId::new(1),
        indices: 6..12,
    }];
    let passes = plan(0..12, &runs, &[group(0..12)], 12);
    assert_eq!(
        passes[0],
        pass(
            Target::Layer(0),
            true,
            vec![
                own(0..6),
                Draw {
                    indices: 6..12,
                    source: Source::Indices(Some(TextureId::new(1))),
                },
            ]
        )
    );
}

fn open_recorded() -> (Recorder, Box<dyn Device>, QueueHandle) {
    let recorder = Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::PUSH_CONSTANTS,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    (recorder, device, queue)
}

const EXTENT: (u32, u32) = (64, 48);

/// A square at `x`, eight pixels across.
fn square(list: &mut DrawList, x: f32) {
    list.rect(
        glam::Vec2::new(x, 4.0),
        glam::Vec2::new(x + 8.0, 12.0),
        [1.0; 4],
    );
}

/// **A frame with a group records the planned passes, clears the layer,
/// blends it back through the premultiplied pipeline, and writes the layer's
/// quad where its indices point** — and gives everything back.
#[test]
fn a_group_frame_draws_into_a_cleared_layer_and_blends_it_back() {
    use crate::graph::{CompiledPass, RenderGraph};
    use crate::transient::{TransientImageDesc, TransientPool};

    let (recorder, device, queue) = open_recorded();
    let before = recorder.total_live_objects();
    let mut pool = TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let mut list = DrawList::new();
    square(&mut list, 0.0);
    list.push_opacity(0.5);
    square(&mut list, 10.5);
    list.pop_opacity().expect("pushed above");
    square(&mut list, 30.0);
    ui.begin_frame(device.as_ref(), &list, &FontAtlas::built_in(), 1.0)
        .expect("upload");
    assert_eq!(ui.last_index_count[ui.frame], 18, "the list's own indices");
    recorder.clear();

    let mut graph = RenderGraph::new(queue);
    let target = graph.create_image(
        "target",
        TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
    );
    ui.add_passes(&mut graph, target, EXTENT);
    let compiled = graph.compile(&pool).expect("a legal frame");
    let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
    assert_eq!(labels, ["ui-composite", "ui-opacity", "ui-composite"]);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("group frame"),
        queue,
    });
    compiled
        .execute(device.as_ref(), &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let commands = encoder.finish().expect("recording succeeded");

    let stream = recorder.commands();
    let loads: Vec<LoadOp> = stream
        .iter()
        .filter_map(|command| match command {
            Command::BeginRenderPass {
                color_attachments, ..
            } => Some(color_attachments[0].load),
            _ => None,
        })
        .collect();
    assert_eq!(loads, [LoadOp::Load, LoadOp::Clear, LoadOp::Load]);
    let draws: Vec<(GraphicsPipelineHandle, Range<u32>)> = {
        let mut bound = None;
        stream
            .iter()
            .filter_map(|command| match command {
                Command::BindGraphicsPipeline(pipeline) => {
                    bound = Some(*pipeline);
                    None
                }
                Command::DrawIndexed { indices, .. } => {
                    Some((bound.expect("a pipeline is bound"), indices.clone()))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        draws,
        [
            (ui.pipeline, 0..6),
            (ui.layer_pipelines[0], 6..12),
            (ui.composite_pipeline, 18..24),
            (ui.pipeline, 12..18),
        ]
    );

    // The quad goes after the list's three squares, and is the run's own.
    let quad_writes: Vec<(u64, usize)> = recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::BufferWritten {
                buffer,
                offset,
                len,
            } if buffer == ui.vertex_buffers[ui.frame] => Some((offset, len)),
            _ => None,
        })
        .collect();
    let vertex = size_of::<Vertex2d>();
    assert_eq!(quad_writes, [((12 * vertex) as u64, 4 * vertex)]);
    let run = &ui.last_groups[ui.frame][0];
    assert_eq!(run.bounds.min, glam::Vec2::new(10.5, 4.0));
    let quad = run.layer_quad(EXTENT);
    assert_eq!(quad[3].pos, glam::Vec2::new(10.0, 4.0), "snapped outward");
    assert_eq!(quad[1].pos, glam::Vec2::new(19.0, 12.0));

    let counters = ui.counters();
    assert_eq!(counters.draws, 4, "before, the group, its blend, after");
    assert_eq!(counters.triangles, Some(8));

    device.destroy_command_buffer(commands);
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    assert_eq!(recorder.total_live_objects(), before, "nothing leaked");
    recorder.assert_valid();
}

/// **A group at zero or at one adds no pass**: the frame is the one
/// `ui-composite` pass an ungrouped list draws.
#[test]
fn a_group_at_zero_or_one_adds_no_pass() {
    use crate::graph::{CompiledPass, RenderGraph};
    use crate::transient::{TransientImageDesc, TransientPool};

    let (_recorder, device, queue) = open_recorded();
    let mut pool = TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    for opacity in [0.0, 1.0] {
        let mut list = DrawList::new();
        square(&mut list, 0.0);
        list.push_opacity(opacity);
        square(&mut list, 10.0);
        list.pop_opacity().expect("pushed above");
        ui.begin_frame(device.as_ref(), &list, &FontAtlas::built_in(), 1.0)
            .expect("upload");
        let mut graph = RenderGraph::new(queue);
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
        );
        ui.add_passes(&mut graph, target, EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");
        let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
        assert_eq!(labels, ["ui-composite"], "opacity {opacity}");
    }
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
}

/// **Siblings share one physical layer and only nesting adds another**: the
/// graph aliases each sibling's layer onto the last one's, so a row of faded
/// boxes costs one image the target's size, not one each.
#[test]
fn sibling_layers_alias_and_nested_ones_do_not() {
    use crate::graph::RenderGraph;
    use crate::transient::{TransientImageDesc, TransientPool};

    let (_recorder, device, queue) = open_recorded();
    let pool = TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let siblings = {
        let mut list = DrawList::new();
        for x in [0.0, 10.0, 20.0] {
            list.push_opacity(0.5);
            square(&mut list, x);
            list.pop_opacity().expect("pushed above");
        }
        list
    };
    let nested = {
        let mut list = DrawList::new();
        list.push_opacity(0.5);
        list.push_opacity(0.5);
        square(&mut list, 0.0);
        list.pop_opacity().expect("pushed above");
        list.pop_opacity().expect("pushed above");
        list
    };
    for (list, physical) in [(siblings, 2), (nested, 3)] {
        ui.begin_frame(device.as_ref(), &list, &FontAtlas::built_in(), 1.0)
            .expect("upload");
        let mut graph = RenderGraph::new(queue);
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
        );
        ui.add_passes(&mut graph, target, EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");
        assert_eq!(
            compiled.physical_image_count(),
            physical,
            "the target and the layers"
        );
    }
    ui.destroy(device.as_ref());
}
