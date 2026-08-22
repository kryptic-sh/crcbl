//! The UI pass's recorded command stream, asserted against `NullBackend`.
//!
//! `crates/crcbl-render/tests/graph_compile.rs` is the pattern: build a frame,
//! run it through the real graph against the recording null backend, and assert
//! on the commands the backend actually saw.
//!
//! This file used to ask that question of a **tier split**. The pass had two
//! ways to hand `ui.slang` its viewport — a push constant where the device had
//! them, a uniform buffer where it did not, because WebGPU has none — and two
//! shader artifacts to match. It has one of each now, and what is left to assert
//! is that the one path is the same path on a device with push constants and on
//! a device without: same commands, in the same order, with no push-constant
//! write on either.
//!
//! The null backend is what makes that comparison possible without a GPU.
//! `NullInstance::gpu_driven` reports `Features::PUSH_CONSTANTS` and
//! `NullInstance::portable` does not, so one process can build both renderers.

use crcbl_hal::null::{Command, NullInstance, Recorder};
use crcbl_hal::{
    CommandEncoderDesc, Device, DeviceDesc, Features, Format, ImageUsage, Instance, QueueHandle,
    QueueKind, ResourceState,
};
use crcbl_render::UiRenderer;
use crcbl_render::graph::{ImportedImage, InitialClaim, RenderGraph};
use crcbl_render::transient::TransientPool;
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::text::FontAtlas;

const EXTENT: (u32, u32) = (256, 192);
const TARGET_FORMAT: Format = Format::Bgra8UnormSrgb;

/// Which null preset to open. The only difference that matters here is whether
/// the device reports `Features::PUSH_CONSTANTS`.
#[derive(Clone, Copy)]
enum Preset {
    /// Reports `Features::PUSH_CONSTANTS`, as a native Vulkan device does.
    WithPushConstants,
    /// Reports none, as every WebGPU device does.
    WithoutPushConstants,
}

struct Harness {
    recorder: Recorder,
    device: Box<dyn Device>,
    queue: QueueHandle,
}

impl Harness {
    fn open(preset: Preset) -> Self {
        let recorder = Recorder::new();
        let (instance, required) = match preset {
            Preset::WithPushConstants => (NullInstance::gpu_driven(), Features::GPU_DRIVEN),
            Preset::WithoutPushConstants => (NullInstance::portable(), Features::COMPUTE),
        };
        let instance = instance.with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("ui pass"),
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
            // A hand-made image with no acquire in front of it, so it is the
            // checked kind; every `target()` is a fresh handle the ledger has
            // never seen, so nothing contradicts the `Load` state above.
            claim: InitialClaim::Tracked,
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

/// Builds a renderer on `preset`, runs one HUD frame through the real graph, and
/// returns every command the backend saw.
fn record_one_frame(preset: Preset) -> Vec<Command> {
    let harness = Harness::open(preset);
    let mut pool = TransientPool::new();
    let mut renderer = UiRenderer::new(harness.device.as_ref(), harness.queue, TARGET_FORMAT)
        .expect("neither device is refused");

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

    stream
}

fn names(stream: &[Command]) -> Vec<&'static str> {
    stream.iter().map(Command::name).collect()
}

/// **The pass binds its pipeline and its group, binds the index buffer and
/// draws**, inside the graph's own pass scope, and records no `PushConstants` —
/// on a device that has them.
///
/// The viewport arrives through the bind group instead. Spelling the whole
/// stream out rather than asserting the absence of one command is what would
/// catch a path that "worked" by dropping the draw or binding a second group.
#[test]
fn the_pass_records_no_push_constant_even_where_they_exist() {
    let stream = record_one_frame(Preset::WithPushConstants);
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
            "BindIndexBuffer",
            "DrawIndexed",
            "EndRenderPass",
            "Barrier",
        ],
        "the uniform-buffer path is the only path, on every device"
    );
}

/// **And a device with no push constants records exactly the same frame.** That
/// equality is the property the tier split cost: the two used to differ by a
/// `PushConstants` write, a pipeline layout, a bind-group layout and a shader
/// artifact.
///
/// The bind is identical too — same slot, no dynamic offsets. The uniform buffer
/// is bound whole, which is why `dynamic_offsets` is empty rather than `[0]`;
/// see `crcbl_render::ui_pass`'s docs.
#[test]
fn a_device_without_push_constants_records_the_same_frame() {
    let with = record_one_frame(Preset::WithPushConstants);
    let without = record_one_frame(Preset::WithoutPushConstants);

    assert_eq!(
        names(&with),
        names(&without),
        "one path means one stream, whatever the device reports"
    );

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
            .expect("both bind one")
    };
    assert_eq!(bind(&with), (0, Vec::new()));
    assert_eq!(bind(&without), (0, Vec::new()));
}
