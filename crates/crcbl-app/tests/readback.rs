//! `Presenter::submit_present_and_read_back` on an offscreen presenter: what
//! a program's image test does, with no display.
//!
//! The plumbing runs on the null backend everywhere. The pixels need a real
//! rasteriser, so that test is ignored here and run by CI's lavapipe job:
//!
//! ```text
//! VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
//!     cargo test -p crcbl-app --test readback -- --ignored
//! ```
//!
//! `CRCBL_ADAPTER` pins the adapter, as it does for a program.

use crcbl_app::adapter::ADAPTER_ENV_VAR;
use crcbl_app::backend::GpuBackend;
use crcbl_app::present::{Presenter, PresenterDesc};
use crcbl_app::readback::Rgba8Frame;
use crcbl_hal::{
    Barriers, ClearValue, ColorAttachment, CommandEncoderDesc, ImageBarrier, ImageSubresourceRange,
    LoadOp, Rect2d, RenderPassDesc, ResourceState, StoreOp,
};

/// The frame size: wide enough that a row is not already a multiple of the
/// staging buffer's row alignment, so the padding has to be dropped.
const EXTENT: (u32, u32) = (37, 20);
/// The top half's colour, and the bottom half's. Every channel is 0 or 1,
/// which encodes the same in sRGB as in linear, and red and blue differ so a
/// swapped channel order shows.
const TOP: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BOTTOM: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// Draws [`TOP`] over the top half and [`BOTTOM`] over the rest, and reads
/// the presented frame back.
fn draw_and_read(presenter: &mut Presenter) -> Rgba8Frame {
    let frame = presenter
        .acquire()
        .expect("acquire")
        .expect("an offscreen ring never goes out of date");
    let format = presenter.format();
    let device = presenter.device();
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("read-back halves"),
        queue: presenter.queue(),
    });
    let barrier = |from, to| {
        [ImageBarrier::new(
            frame.image,
            ImageSubresourceRange::all(format),
            from,
            to,
        )]
    };
    encoder.pipeline_barrier(&Barriers {
        images: &barrier(ResourceState::Undefined, ResourceState::ColorAttachment),
        ..Barriers::default()
    });
    let (width, height) = frame.extent;
    let half = height / 2;
    for (colour, area) in [
        (TOP, Rect2d::from_size(width, half)),
        (
            BOTTOM,
            Rect2d {
                x: 0,
                y: i32::try_from(half).expect("a small frame"),
                width,
                height: height - half,
            },
        ),
    ] {
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("read-back half"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(colour),
            }],
            depth_stencil_attachment: None,
            render_area: area,
            timestamp_writes: None,
        });
        encoder.end_render_pass();
    }
    encoder.pipeline_barrier(&Barriers {
        images: &barrier(ResourceState::ColorAttachment, ResourceState::Present),
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("finish");
    presenter
        .submit_present_and_read_back(&frame, commands)
        .expect("read back")
}

fn pixel(frame: &Rgba8Frame, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * frame.width + x) * 4) as usize;
    frame.pixels[at..at + 4].try_into().expect("four bytes")
}

/// **The read-back is the frame's size, tightly packed**, and gives every
/// object it made back: on the null backend, which runs everywhere.
#[test]
fn a_null_read_back_is_the_frame_size_and_leaves_nothing_alive() {
    let mut presenter = Presenter::open_offscreen(
        EXTENT,
        &PresenterDesc {
            label: "null read-back",
            backend: Some(GpuBackend::Null),
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    let frame = draw_and_read(&mut presenter);
    assert_eq!((frame.width, frame.height), EXTENT);
    assert_eq!(frame.format, presenter.format());
    assert_eq!(frame.pixels.len(), (EXTENT.0 * EXTENT.1 * 4) as usize);
    presenter.drain().expect("drain");
    presenter.destroy().expect("nothing left alive");
}

/// **The pixels are the frame's, in R, G, B, A order, top row first**, and
/// the same on a second frame.
#[test]
#[ignore = "needs Vulkan; CI runs it on lavapipe"]
fn a_vulkan_read_back_is_the_frame_in_rgba_order() {
    let pin = std::env::var(ADAPTER_ENV_VAR).ok();
    let mut presenter = Presenter::open_offscreen(
        EXTENT,
        &PresenterDesc {
            label: "vulkan read-back",
            backend: Some(GpuBackend::Vulkan),
            adapter_pin: pin.as_deref(),
            ..PresenterDesc::default()
        },
    )
    .expect("a Vulkan device");
    eprintln!(
        "read-back on {:?}, {:?}",
        presenter.adapter().map(|adapter| adapter.name),
        presenter.format()
    );
    let first = draw_and_read(&mut presenter);
    let (width, height) = EXTENT;
    for (x, y) in [(0, 0), (width - 1, height / 2 - 1)] {
        assert_eq!(pixel(&first, x, y), [255, 0, 0, 255], "top at {x},{y}");
    }
    for (x, y) in [(0, height / 2), (width - 1, height - 1)] {
        assert_eq!(pixel(&first, x, y), [0, 0, 255, 255], "bottom at {x},{y}");
    }
    let second = draw_and_read(&mut presenter);
    assert_eq!(first, second, "the same frame twice reads back the same");
    presenter.drain().expect("drain");
    presenter.destroy().expect("nothing left alive");
}
