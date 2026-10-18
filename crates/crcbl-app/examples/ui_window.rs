//! One frame of `crcbl-ui` on a window, through a [`Presenter`] and
//! `crcbl-frame`'s UI pass — the whole of what a status bar or a launcher needs
//! from the engine, with no `crcbl` umbrella.
//!
//! ```text
//! cargo run -p crcbl-app --example ui_window              # a real window
//! cargo run -p crcbl-app --example ui_window -- --headless  # no window, null backend
//! ```
//!
//! A real program keeps the loop going — pumping events, resizing on
//! `ShellEvent::Resized`, rebuilding its draw list — where this draws once and
//! tears down.

use std::time::Duration;

use crcbl_app::backend::GpuBackend;
use crcbl_app::present::{FrameOutcome, Presenter, PresenterDesc};
use crcbl_app::window::{open_window, wait_for_configure};
use crcbl_frame::{ImportedImage, InitialClaim, RenderGraph, TransientPool, UiRenderer};
use crcbl_hal::{CommandEncoderDesc, ResourceState};
use crcbl_shell::{HeadlessShell, Shell, WindowDesc};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::image::ImageAtlas;
use crcbl_ui::text::FontAtlas;
use glam::Vec2;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headless = std::env::args().any(|arg| arg == "--headless");
    let mut shell: Box<dyn Shell> = if headless {
        Box::new(HeadlessShell::new())
    } else {
        crcbl_shell::open()?
    };
    let window = open_window(
        shell.as_mut(),
        Duration::ZERO,
        &WindowDesc {
            title: "crcbl-app ui_window",
            ..WindowDesc::default()
        },
    )?;
    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    let adapter_pin = crcbl_app::adapter::pin();
    let desc = PresenterDesc {
        label: "ui_window",
        // A headless run has no display to pick a backend for.
        backend: headless.then_some(GpuBackend::Null),
        adapter_pin: adapter_pin.as_deref(),
        ..PresenterDesc::default()
    };
    let mut presenter = Presenter::open(shell.as_ref(), window, extent, &desc)?;

    // As deep as the presenter's ring, so no slot is rewritten while the
    // device may still read it.
    let mut ui = UiRenderer::with_ring(
        presenter.device(),
        presenter.queue(),
        presenter.format(),
        ImageAtlas::new(),
        presenter.frames_in_flight(),
    )?;
    let mut pool = TransientPool::new();
    let atlas = FontAtlas::built_in();
    let mut list = DrawList::new();
    list.rect(
        Vec2::new(16.0, 16.0),
        Vec2::new(240.0, 64.0),
        [0.2, 0.3, 0.5, 1.0],
    );
    list.text(Vec2::new(28.0, 32.0), "crcbl-app", [1.0; 4], 16.0);

    let outcome = match presenter.acquire()? {
        // The swapchain went out of date before the first frame and has been
        // rebuilt; a loop would draw on the next turn.
        None => FrameOutcome::Reconfigured,
        Some(frame) => {
            ui.begin_frame(presenter.device(), &list, &atlas, 1.0)?;
            let mut graph = RenderGraph::new(presenter.queue());
            let target = graph.import_image(
                "swapchain",
                ImportedImage {
                    image: frame.image,
                    view: frame.view,
                    format: presenter.format(),
                    extent: frame.extent,
                    // The acquire's semaphore orders this frame after the last
                    // use of the image, so its old contents are discarded.
                    initial: ResourceState::Undefined,
                    claim: InitialClaim::Acquired,
                    final_state: ResourceState::Present,
                },
            );
            graph
                .add_render_pass("backdrop")
                .clear_color(target, [0.05, 0.05, 0.08, 1.0])
                .execute(|_| {});
            ui.add_passes(&mut graph, target, frame.extent);
            let compiled = graph.compile(&pool)?;

            let mut encoder = presenter
                .device()
                .create_command_encoder(&CommandEncoderDesc {
                    label: Some("ui_window frame"),
                    queue: presenter.queue(),
                });
            compiled.execute(presenter.device(), &mut pool, encoder.as_mut(), None)?;
            let commands = encoder.finish()?;
            presenter.submit_and_present(&frame, commands)?
        }
    };
    println!("ui_window: {outcome:?} at {}x{}", extent.0, extent.1);

    presenter.drain()?;
    ui.destroy(presenter.device());
    pool.destroy(presenter.device());
    presenter.destroy()?;
    shell.destroy_window(window)?;
    Ok(())
}
