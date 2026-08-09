//! The UI pass on both tiers, asserted against `NullBackend`'s recorded stream.
//!
//! `crates/crcbl-render/tests/graph_compile.rs` is the pattern: build a frame,
//! run it through the real graph against the recording null backend, and assert
//! on the commands the backend actually saw. This file asks that suite's
//! question of the tier split introduced for `docs/plan/10-wasm-webgpu.md` —
//! **WebGPU has no push constants**, so the UI pass has two ways to hand
//! `ui.slang` its viewport, and both have to be exercised somewhere that has no
//! GPU.
//!
//! The null backend is what makes that possible: `NullInstance::tier_a` reports
//! `Features::PUSH_CONSTANTS` and `NullInstance::tier_b` does not, so one
//! process can build both renderers and compare them. Without it the Tier B
//! path would only ever run in a browser, which is the one place this project
//! cannot assert anything.

use crcbl_hal::null::{Command, NullInstance, Recorder};
use crcbl_hal::{
    CommandEncoderDesc, Device, DeviceDesc, Features, Format, ImageUsage, Instance, QueueHandle,
    QueueKind, ResourceState,
};
use crcbl_render::graph::{ImportedImage, RenderGraph};
use crcbl_render::transient::TransientPool;
use crcbl_render::{ConstantDelivery, UiRenderer};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::text::FontAtlas;

const EXTENT: (u32, u32) = (256, 192);
const TARGET_FORMAT: Format = Format::Bgra8UnormSrgb;

/// Which null preset to open. The only difference that matters here is whether
/// the device reports `Features::PUSH_CONSTANTS`.
#[derive(Clone, Copy)]
enum Tier {
    A,
    B,
}

struct Harness {
    recorder: Recorder,
    device: Box<dyn Device>,
    queue: QueueHandle,
}

impl Harness {
    fn open(tier: Tier) -> Self {
        let recorder = Recorder::new();
        let (instance, required) = match tier {
            Tier::A => (NullInstance::tier_a(), Features::GPU_DRIVEN),
            Tier::B => (NullInstance::tier_b(), Features::COMPUTE),
        };
        let instance = instance.with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("ui tier"),
                adapter: adapter.id,
                required_features: required,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        Self {
            recorder,
            device,
            queue,
        }
    }

    /// A stand-in for a swapchain image, imported exactly as an acquired frame
    /// would be — already a colour attachment, because the UI pass composites
    /// with `LoadOp::Load` on top of what the tonemap left.
    fn target(&self) -> ImportedImage {
        let image = self
            .device
            .create_image(&crcbl_hal::ImageDesc {
                label: Some("fake swapchain image"),
                image_type: crcbl_hal::ImageType::D2,
                extent: crcbl_hal::Extent3d::d2(EXTENT.0, EXTENT.1),
                format: TARGET_FORMAT,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::PRESENT,
                memory: crcbl_hal::MemoryLocation::DeviceLocal,
            })
            .expect("an image");
        let view = self
            .device
            .create_image_view(&crcbl_hal::ImageViewDesc {
                label: Some("fake swapchain view"),
                image,
                view_type: crcbl_hal::ImageViewType::D2,
                format: TARGET_FORMAT,
                range: crcbl_hal::ImageSubresourceRange::all(TARGET_FORMAT),
            })
            .expect("a view");
        ImportedImage {
            image,
            view,
            format: TARGET_FORMAT,
            extent: EXTENT,
            initial: ResourceState::ColorAttachment,
            final_state: ResourceState::Present,
        }
    }

    fn release(&self, target: &ImportedImage) {
        self.device.destroy_image_view(target.view);
        self.device.destroy_image(target.image);
    }
}

/// A HUD roughly the shape breakout draws: two lines of text and a bar.
fn hud() -> DrawList {
    let mut list = DrawList::new();
    list.text(glam::Vec2::new(8.0, 8.0), "SCORE 1200", [1.0; 4], 14.0);
    list.text(glam::Vec2::new(8.0, 24.0), "LIVES 3", [1.0; 4], 14.0);
    list.rect(
        glam::Vec2::new(0.0, 180.0),
        glam::Vec2::new(256.0, 184.0),
        [0.2, 0.4, 0.9, 1.0],
    );
    list
}

/// Builds a renderer on `tier`, runs one HUD frame through the real graph, and
/// returns the delivery it chose and every command the backend saw.
fn record_one_frame(tier: Tier) -> (ConstantDelivery, Vec<Command>) {
    let harness = Harness::open(tier);
    let mut pool = TransientPool::new();
    let mut renderer = UiRenderer::new(harness.device.as_ref(), harness.queue, TARGET_FORMAT)
        .expect("neither tier is refused");
    let delivery = renderer.constant_delivery();

    let atlas = FontAtlas::built_in();
    let list = hud();
    renderer
        .begin_frame(harness.device.as_ref(), &list, &atlas, 1.0)
        .expect("upload");

    let target = harness.target();
    // Everything up to here — the atlas upload, the geometry write — is
    // start-up and per-frame CPU work, and neither is the thing under test.
    harness.recorder.clear();

    let mut graph = RenderGraph::new(harness.queue);
    let swap = graph.import_image("swapchain", target);
    renderer.add_pass(&mut graph, swap, EXTENT);
    let compiled = graph.compile(&pool).expect("a legal frame");

    let mut encoder = harness.device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ui frame"),
        queue: harness.queue,
    });
    compiled
        .execute(harness.device.as_ref(), &mut pool, encoder.as_mut(), None)
        .expect("executed");
    let commands = encoder.finish().expect("recording succeeded");
    let stream = harness.recorder.commands();

    harness.device.destroy_command_buffer(commands);
    renderer.destroy(harness.device.as_ref());
    pool.destroy(harness.device.as_ref());
    harness.release(&target);
    harness.recorder.assert_valid();

    (delivery, stream)
}

fn names(stream: &[Command]) -> Vec<&'static str> {
    stream.iter().map(Command::name).collect()
}

/// **Tier A records exactly what it has always recorded.** The pass binds its
/// pipeline and its group, writes the viewport as a push constant, binds the
/// index buffer and draws — in that order, inside the graph's own pass scope.
///
/// This is the no-regression half: the uniform path was added *beside* this
/// one, not in place of it.
#[test]
fn the_push_constant_tier_records_a_push_constant_write() {
    let (delivery, stream) = record_one_frame(Tier::A);
    assert_eq!(delivery, ConstantDelivery::PushConstants);
    assert_eq!(
        names(&stream),
        vec![
            // The graph's own transition into the colour-attachment state, and
            // the one back to `Present` at the end. Both are the graph's, not
            // this pass's; see `crcbl_render`'s "one rule".
            "Barrier",
            "BeginRenderPass",
            "SetViewport",
            "SetScissor",
            "BindGraphicsPipeline",
            "BindGroup",
            "PushConstants",
            "BindIndexBuffer",
            "DrawIndexed",
            "EndRenderPass",
            "Barrier",
        ],
        "the Tier A stream is the one this pass has always recorded"
    );

    let push = stream
        .iter()
        .find_map(|command| match command {
            Command::PushConstants {
                stages,
                offset,
                len,
            } => Some((*stages, *offset, *len)),
            _ => None,
        })
        .expect("Tier A writes one");
    assert_eq!(push.0, crcbl_hal::ShaderStages::VERTEX);
    assert_eq!(push.1, 0);
    assert_eq!(push.2, 8, "two f32s of viewport, and nothing else");
}

/// **Tier B records the same frame with no push constant at all**, because
/// there is no such command to record on a device that has none. The viewport
/// arrives through the bind group instead.
#[test]
fn the_uniform_tier_records_no_push_constant() {
    let (delivery, stream) = record_one_frame(Tier::B);
    assert_eq!(delivery, ConstantDelivery::UniformBuffer);
    assert!(
        !names(&stream).contains(&"PushConstants"),
        "a device with no push constants must not be handed one: {:?}",
        names(&stream)
    );
    // It still draws. A tier that quietly recorded nothing would also pass the
    // assertion above.
    assert!(names(&stream).contains(&"DrawIndexed"));
    assert!(names(&stream).contains(&"BindGroup"));
}

/// **And the difference is exactly that one command.** Both tiers open the same
/// pass, set the same dynamic state, bind the same group at the same slot with
/// no dynamic offsets, and draw the same indices; the push-constant write is
/// the whole delta.
///
/// This is the assertion that would catch a Tier B path that "worked" by
/// dropping the draw, binding a second group, or taking a different pass shape
/// — which is the failure mode a `contains("PushConstants") == false` check
/// cannot see.
#[test]
fn the_two_tiers_differ_by_exactly_the_push_constant_write() {
    let (_, tier_a) = record_one_frame(Tier::A);
    let (_, tier_b) = record_one_frame(Tier::B);

    let mut without_push = names(&tier_a);
    without_push.retain(|name| *name != "PushConstants");
    assert_eq!(
        without_push,
        names(&tier_b),
        "the tiers must differ only in how the viewport is delivered"
    );

    // The bind is identical too — same slot, no dynamic offsets on either side.
    // The uniform buffer is bound whole, which is why `dynamic_offsets` is
    // empty rather than `[0]`; see `crcbl_render::ui_pass`'s docs.
    let bind = |stream: &[Command]| {
        stream
            .iter()
            .find_map(|command| match command {
                Command::BindGroup {
                    slot,
                    dynamic_offsets,
                    ..
                } => Some((*slot, dynamic_offsets.clone())),
                _ => None,
            })
            .expect("both tiers bind one")
    };
    assert_eq!(bind(&tier_a), (0, Vec::new()));
    assert_eq!(bind(&tier_b), (0, Vec::new()));
}
