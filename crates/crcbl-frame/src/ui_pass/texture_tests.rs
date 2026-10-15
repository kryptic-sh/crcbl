//! [`UiRenderer::add_passes_with_textures`]: a [`DrawList::texture`] rectangle
//! sampling an image another pass of the same graph drew.
//!
//! Every claim here is read off the null backend's recorded stream: which
//! group set 1 had bound for each draw, which view that group names, and where
//! the graph put the pass that drew the view and the barrier out of it.

use std::cell::Cell;

use crcbl_hal::null::{Command, NullInstance, Recorder};
use crcbl_hal::{
    CommandEncoderDesc, DeviceDesc, Features, ImageUsage, Instance, QueueKind, ResourceState,
};
use crcbl_ui::TextureId;
use glam::Vec2;

use super::*;
use crate::graph::{CompiledPass, RenderGraph};
use crate::transient::{TransientImageDesc, TransientPool};

const EXTENT: (u32, u32) = (64, 48);
/// The view's own target: smaller than the frame, as a pane's would be.
const VIEW_EXTENT: (u32, u32) = (16, 12);
const VIEW: TextureId = TextureId::new(7);
const WHITE: [f32; 4] = [1.0; 4];

fn open_recorded() -> (Recorder, Box<dyn Device>, QueueHandle) {
    let recorder = Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    (recorder, device, queue)
}

/// A HUD rectangle, the view in a pane, and another HUD rectangle over it: the
/// shape of an editor's viewport between two panels.
fn pane_list(texture: TextureId) -> DrawList {
    let mut list = DrawList::new();
    list.rect(Vec2::ZERO, Vec2::new(8.0, 48.0), WHITE);
    list.texture(
        Vec2::new(8.0, 0.0),
        Vec2::new(64.0, 48.0),
        texture,
        (Vec2::ZERO, Vec2::ONE),
        WHITE,
    );
    list.rect(Vec2::new(10.0, 2.0), Vec2::new(20.0, 6.0), WHITE);
    list
}

/// What one recorded frame showed.
struct Recorded {
    labels: Vec<String>,
    commands: Vec<Command>,
    /// The view the `view` pass drew into, as its body saw it.
    view: ImageViewHandle,
}

/// Records one frame: a `view` pass clearing a transient of its own, then the
/// UI over a target, sampling that transient as `mapped` names it.
///
/// The view pass is added **first**, as a caller must: the graph runs passes in
/// declaration order, and what it adds is the barrier between the two.
fn record(
    ui: &mut UiRenderer,
    device: &dyn Device,
    queue: QueueHandle,
    recorder: &Recorder,
    list: &DrawList,
    mapped: Option<TextureId>,
) -> Recorded {
    let mut pool = TransientPool::new();
    ui.begin_frame(device, list, &FontAtlas::built_in(), 1.0)
        .expect("upload");
    recorder.clear();
    let drawn = Cell::new(None);
    let labels;
    {
        let mut graph = RenderGraph::new(queue);
        let view = graph.create_image(
            "view",
            TransientImageDesc::new(
                VIEW_EXTENT,
                Format::Bgra8UnormSrgb,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            ),
        );
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
        );
        graph
            .add_render_pass("view")
            .clear_color(view, [0.25, 0.5, 0.75, 1.0])
            .execute(|ctx| drawn.set(Some(ctx.image_view(view))));
        let textures: Vec<UiTexture> = mapped
            .map(|id| UiTexture { id, image: view })
            .into_iter()
            .collect();
        ui.add_passes_with_textures(&mut graph, target, EXTENT, &textures);
        let compiled = graph.compile(&pool).expect("a legal frame");
        labels = compiled
            .passes()
            .iter()
            .map(|pass| CompiledPass::label(pass).to_owned())
            .collect();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("ui texture frame"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        device.destroy_command_buffer(commands);
    }
    pool.destroy(device);
    Recorded {
        labels,
        commands: recorder.commands(),
        view: drawn.get().expect("the view pass ran"),
    }
}

/// Each draw's index range with the group set 1 held when it was recorded.
fn draws_with_set_1(commands: &[Command]) -> Vec<(Range<u32>, BindGroupHandle)> {
    let mut bound = None;
    let mut draws = Vec::new();
    for command in commands {
        match command {
            Command::BindGroup { slot, group, .. } if *slot == TEXTURE_SET => bound = Some(*group),
            Command::DrawIndexed { indices, .. } => {
                draws.push((
                    indices.clone(),
                    bound.expect("set 1 is bound before a draw"),
                ));
            }
            _ => {}
        }
    }
    draws
}

/// **The view is its own draw, bound to a group naming the view the other
/// pass drew; the rectangles either side are drawn around it with the blank**
/// — which is what makes the pane show the rendered view rather than a region
/// of the image atlas.
#[test]
fn a_texture_rectangle_samples_the_view_another_pass_drew() {
    let (recorder, device, queue) = open_recorded();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let frame = record(
        &mut ui,
        device.as_ref(),
        queue,
        &recorder,
        &pane_list(VIEW),
        Some(VIEW),
    );
    let draws = draws_with_set_1(&frame.commands);
    let ranges: Vec<Range<u32>> = draws.iter().map(|(range, _)| range.clone()).collect();
    assert_eq!(
        ranges,
        [0..6, 6..12, 12..18],
        "the view splits its half into three draws"
    );
    assert_eq!(draws[0].1, ui.blank_group, "the first rect samples nothing");
    assert_eq!(
        draws[2].1, ui.blank_group,
        "the rect over the pane samples nothing"
    );
    assert_ne!(
        draws[1].1, ui.blank_group,
        "the pane drew with the blank, not the view"
    );
    assert_eq!(
        ui.texture_groups.views(),
        [frame.view],
        "the one group set 1 bound for the pane must name the view the other pass drew into",
    );
    assert_eq!(
        ui.counters().draws,
        3,
        "the counters count the same three draws"
    );

    ui.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **The graph puts the view pass first and the barrier out of it between the
/// two**: the view is drawn as a colour attachment, moved to `ShaderRead`, and
/// only then does the pass sampling it begin — the ordering an editor pane
/// needs, read off the recorded stream rather than off the declarations.
#[test]
fn the_view_is_drawn_and_made_readable_before_the_ui_samples_it() {
    let (recorder, device, queue) = open_recorded();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let frame = record(
        &mut ui,
        device.as_ref(),
        queue,
        &recorder,
        &pane_list(VIEW),
        Some(VIEW),
    );
    assert_eq!(frame.labels, ["view", "ui-composite"]);

    let begins: Vec<(usize, ImageViewHandle)> = frame
        .commands
        .iter()
        .enumerate()
        .filter_map(|(at, command)| match command {
            Command::BeginRenderPass {
                color_attachments, ..
            } => Some((at, color_attachments[0].view)),
            _ => None,
        })
        .collect();
    let [(view_begin, drawn_into), (ui_begin, _)] = begins[..] else {
        panic!("two render passes, got {begins:?}");
    };
    assert_eq!(drawn_into, frame.view, "the first pass draws the view");

    let into_shader_read = frame.commands[view_begin..ui_begin]
        .iter()
        .filter_map(|command| match command {
            Command::Barrier { images, .. } => Some(images),
            _ => None,
        })
        .flatten()
        .filter(|barrier| {
            barrier.from == ResourceState::ColorAttachment
                && barrier.to == ResourceState::ShaderRead
        })
        .count();
    assert_eq!(
        into_shader_read, 1,
        "between drawing the view and sampling it, exactly one barrier must move it \
         from the attachment it was drawn as to a shader read",
    );

    ui.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **A texture the call was not handed draws transparent and declares
/// nothing**: set 1 holds the blank for every draw, and nothing makes the view
/// readable, because nothing reads it.
#[test]
fn a_texture_nobody_handed_over_binds_the_blank() {
    let (recorder, device, queue) = open_recorded();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let frame = record(
        &mut ui,
        device.as_ref(),
        queue,
        &recorder,
        &pane_list(TextureId::new(8)),
        Some(VIEW),
    );
    let draws = draws_with_set_1(&frame.commands);
    assert_eq!(draws.len(), 3, "the run is still a draw of its own");
    assert!(
        draws.iter().all(|(_, group)| *group == ui.blank_group),
        "an unnamed texture must not draw whatever was bound last: {draws:?}"
    );
    assert!(ui.texture_groups.views().is_empty());
    let read = frame
        .commands
        .iter()
        .filter_map(|command| match command {
            Command::Barrier { images, .. } => Some(images),
            _ => None,
        })
        .flatten()
        .any(|barrier| barrier.to == ResourceState::ShaderRead);
    assert!(
        !read,
        "nothing samples the view, so nothing moves it to a read"
    );

    ui.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **A group is kept while frames sample its view and released once they stop**
/// — on its slot's turn after a frame through that slot went without it — and
/// `destroy` hands back whatever is still held.
#[test]
fn a_texture_group_is_released_once_frames_stop_sampling_it() {
    let (recorder, device, queue) = open_recorded();
    let before = recorder.total_live_objects();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let viewed = pane_list(VIEW);
    for _ in 0..FRAMES_IN_FLIGHT {
        record(
            &mut ui,
            device.as_ref(),
            queue,
            &recorder,
            &viewed,
            Some(VIEW),
        );
    }
    let held = ui.texture_groups.views().len();
    assert!(held >= 1, "a sampled view holds a group");

    // A slot's group survives the slot's first turn without the view, which
    // is when the frame that bound it may still be in flight, and goes on the
    // turn after: a frame through that slot has then run without it.
    let plain = DrawList::new();
    for _ in 0..2 * FRAMES_IN_FLIGHT {
        ui.begin_frame(device.as_ref(), &plain, &FontAtlas::built_in(), 1.0)
            .expect("upload");
    }
    assert!(
        ui.texture_groups.views().is_empty(),
        "a whole ring without the view must release its groups"
    );

    record(
        &mut ui,
        device.as_ref(),
        queue,
        &recorder,
        &viewed,
        Some(VIEW),
    );
    ui.destroy(device.as_ref());
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "destroy must hand back the texture groups, the blank and its layout"
    );
    recorder.assert_valid();
}
