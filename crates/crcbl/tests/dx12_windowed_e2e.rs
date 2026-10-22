//! A [`Presenter`] on a real Win32 window and a D3D12 device, resized after it
//! has presented.
//!
//! # The gap this closes
//!
//! lyr hit `DXGI_ERROR_INVALID_CALL` from `ResizeBuffers` on a real D3D12
//! window when a bar resized in place by `set_panel` called
//! [`Presenter::resize`]: a command buffer the presenter still had in flight
//! held a back buffer, and `crcbl-dx12`'s `reconfigure_swapchain` documents
//! that it cannot release a reference the caller still owns.
//! `Presenter::reconfigure` now drains first, and
//! `a_resize_after_presenting_reconfigures_with_nothing_in_flight` in
//! `crcbl-app` checks that ordering against the null device. Nothing ran it
//! against DXGI: `tests/windowed_e2e.rs` is Vulkan only, and every D3D12 suite
//! presents offscreen, where a reconfigure makes new ring images and never
//! reaches `ResizeBuffers`. `crcbl-dx12`'s own
//! `a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd` does call
//! `ResizeBuffers` on a real `HWND`, but it destroys every command buffer before
//! the resize — the order the presenter has to get right, written out by hand.
//!
//! So every test here opens a window through `crcbl-shell`'s Win32 backend,
//! presents to it through the same [`Presenter`] lyr uses — which keeps frames
//! in flight between presents, as [`PresenterDesc::frames_in_flight`] says —
//! resizes, and presents again at the new size.
//!
//! # Why a file of its own, and not a D3D12 arm of `windowed_e2e.rs`
//!
//! That suite is Vulkan by construction, and its module docs say so: it names
//! `crcbl_vk::VkInstance` because its instrument, `validation_report`, is a
//! `crcbl-vk` type, and its assertions about acquire semaphores are about a WSI
//! D3D12 does not have — D3D12's acquire is implicit and hands back no
//! semaphore. It also drives the seam call by call, where the defect here lives
//! one level up, in the order [`Presenter`] makes those calls. An arm that
//! shared its fixture would have to branch on the backend in every helper; what
//! the two files share is a window resized from outside, and that is
//! `tests/win32/outside_resize.rs`.
//!
//! # The instruments
//!
//! * **That the resize reached `ResizeBuffers`:** [`AcquiredFrame::extent`] is
//!   the new size and the acquired image is a handle the swapchain never handed
//!   out before. On a windowed swapchain `crcbl-dx12` records the new extent,
//!   and fetches the back buffers again, only after `ResizeBuffers` has
//!   returned success — the offscreen ring is the only other path that changes
//!   either, and [`open`] refuses a surface that is not the window's own `HWND`.
//!   Both are `crcbl-dx12`'s bookkeeping rather than a reading of the
//!   `IDXGISwapChain3` itself, which no seam call exposes.
//! * **That the frames were clean:** the D3D12 debug layer, read back off the
//!   log. `crcbl-dx12`'s `ValidationReport::assert_clean` is test-only inside
//!   that crate and `Device::take_error` is never answered on this backend, so
//!   [`assert_validation_clean`] reads what `crcbl_dx12::debug` logs instead:
//!   the line saying the layer is on, the line its teardown always ends on, and
//!   any message the layer stored. `crcbl_core::log::capture` sees those lines
//!   because the device is created and dropped on the test's own thread.
//!
//! # Gated twice, like every other e2e suite here
//!
//! Behind the `dx12-windowed-e2e` feature *and* `#[ignore]`, so a plain
//! `cargo nextest run --workspace --all-features` on a Windows desktop opens no
//! window. `tests/run-dx12-windowed-e2e.sh` is the only thing that turns them
//! on, and it fails when the suite reports zero tests run.

#![cfg(all(target_os = "windows", feature = "dx12-windowed-e2e"))]

use core::time::Duration;
use std::time::Instant;

use crcbl::backend::GpuBackend;
use crcbl::core::log::Capture;
use crcbl::hal::{
    AcquiredFrame, BackendKind, Barriers, ClearValue, ColorAttachment, CommandEncoderDesc,
    ImageBarrier, ImageSubresourceRange, LoadOp, Rect2d, RenderPassDesc, ResourceState, StoreOp,
};
use crcbl::shell::{
    LogicalSize, PanelAnchor, PanelDesc, PhysicalSize, Shell, ShellBackend, ShellCaps, ShellEvent,
    SurfaceTarget, WindowDesc, WindowId, WindowRole,
};
use crcbl_app::present::{FrameOutcome, Presenter, PresenterDesc};

/// `SetWindowPos` from outside the shell, shared with `tests/windowed_e2e.rs`.
#[path = "win32/outside_resize.rs"]
mod outside_resize;

/// How long any single wait here may take before the test fails.
///
/// `tests/windowed_e2e.rs`'s figure and its argument: far longer than anything
/// needs on an idle machine, and bounded all the same, because a wait that
/// expires here fails naming what never happened.
const WAIT: Duration = Duration::from_secs(20);

/// How many frames are presented on each side of a resize.
///
/// More than any flip-model swapchain's buffer count, so every back buffer has
/// been drawn into, and presented, before the resize asks DXGI to let go of
/// them all.
const FRAMES: u32 = 8;

/// The ordinary window [`a_window_resized_from_outside_resizes_the_presenters_buffers`]
/// opens, in logical units.
const WINDOW: LogicalSize = LogicalSize::new(640.0, 480.0);

/// What that test resizes the window to, from outside.
///
/// Different in both dimensions from [`WINDOW`], so a swapchain that carried
/// one dimension over fails on the other.
const RESIZED: PhysicalSize = PhysicalSize::new(800, 600);

/// The bar [`a_bar_resized_by_set_panel_resizes_the_presenters_buffers`] opens:
/// its height, in logical units. Its width is the monitor's, because it is
/// anchored to both side edges.
const BAR: f64 = 30.0;

/// And the height `set_panel` gives it, at the opposite edge — the reload lyr
/// made, from a top bar to a taller bottom one.
const TALLER: f64 = 50.0;

/// A distinctive clear colour, for a human watching the runner's desktop.
/// Nothing reads it back.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// What `crcbl_dx12::debug` logs once a device's info queue is reachable.
const VALIDATION_ENABLED: &str = "crcbl-dx12: validation enabled (";

/// What `crcbl_dx12::debug`'s teardown log always ends on, with the counts.
const TEARDOWN_READ: &str = "crcbl-dx12: validation messages read at device teardown";

/// What every debug-layer message that teardown log writes starts with. It
/// writes only failures, so any one of these fails the test.
const VALIDATION_MESSAGE: &str = "dx12 validation: ";

/// A window on the Win32 desktop and a presenter on it.
///
/// Fields drop in declaration order, so a test that panics part-way drops the
/// presenter — swapchain, surface, device — before the shell that owns the
/// window, which is the teardown order the seam requires.
struct Windowed {
    presenter: Presenter,
    shell: Box<dyn Shell>,
    window: WindowId,
    /// The last size a [`ShellEvent::Resized`] for [`Self::window`] carried.
    resized: Option<PhysicalSize>,
}

/// Opens the Win32 shell, a window from `desc`, and a D3D12 presenter on it.
///
/// The shell is named rather than taken from the registry, and so is the
/// backend: a fallback to the headless shell would hand over an offscreen
/// target, and a fallback to Vulkan would present through a different
/// swapchain, and either would leave every assertion here about something
/// else. The adapter is `crcbl_app::adapter::pin`'s, which is how
/// `run-dx12-windowed-e2e.sh` puts this suite on WARP.
fn open(desc: &WindowDesc<'_>) -> Windowed {
    let mut shell = crcbl::shell::open_backend(ShellBackend::Win32)
        .expect("a Windows session has a desktop for the Win32 shell");
    let window = shell.create_window(desc).expect("create_window");
    let extent = crcbl_app::window::wait_for_configure(&mut *shell, window, &mut |_| {})
        .expect("the window configures");

    let target = shell.surface_target(window).expect("surface target");
    assert!(
        matches!(target, SurfaceTarget::Win32 { .. }),
        "the Win32 shell must hand over its own HWND, not {target:?}: an offscreen ring never \
         calls ResizeBuffers, which is the whole subject here"
    );

    let pin = crcbl::adapter::pin();
    let presenter = Presenter::open(
        &*shell,
        window,
        extent,
        &PresenterDesc {
            label: "crcbl dx12 windowed e2e",
            backend: Some(GpuBackend::Dx12),
            adapter_pin: pin.as_deref(),
            ..PresenterDesc::default()
        },
    )
    .expect("a D3D12 presenter opens on a Win32 window");
    assert_eq!(
        presenter.device().backend(),
        BackendKind::Dx12,
        "the presenter was asked for D3D12 by name"
    );
    let adapter = presenter
        .adapter()
        .expect("the adapter list does not change under an open device");
    // Load-bearing outside this file: `run-dx12-windowed-e2e.sh` greps for it,
    // and checks the type against the pin it exported.
    eprintln!(
        "crcbl dx12 windowed e2e: presenting on adapter {name:?} type={kind:?} \
         (CRCBL_ADAPTER={pin})",
        name = adapter.name,
        kind = adapter.device_type,
        pin = pin.as_deref().unwrap_or("<unset>"),
    );
    assert_eq!(
        presenter.extent(),
        extent,
        "a swapchain asked for the window's size is configured at it"
    );

    Windowed {
        presenter,
        shell,
        window,
        resized: None,
    }
}

impl Windowed {
    /// The window's size as the shell reports it.
    fn window_size(&self) -> (u32, u32) {
        let size = self
            .shell
            .window_state(self.window)
            .expect("state")
            .size()
            .expect("configured");
        (size.width, size.height)
    }

    /// One turn of a frame loop's event half: every event to the presenter, as
    /// `crcbl_app::present`'s module docs ask, and the window's resizes kept.
    fn pump(&mut self) {
        let Self {
            presenter,
            shell,
            window,
            resized,
        } = self;
        shell.pump(&mut |event| {
            presenter.observe(&event);
            if let ShellEvent::Resized {
                window: resized_window,
                size,
                ..
            } = event
                && resized_window == *window
            {
                *resized = Some(size);
            }
        });
    }

    /// Pumps until the shell has reported the window at a size other than
    /// `from`, and returns it.
    fn pump_until_resized_from(&mut self, from: (u32, u32)) -> (u32, u32) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if let Some(size) = self.resized
                && (size.width, size.height) != from
            {
                return (size.width, size.height);
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {WAIT:?} waiting for a Resized away from {from:?}; the shell \
                 reports the window at {:?}",
                self.shell
                    .window_state(self.window)
                    .ok()
                    .and_then(|state| state.size()),
            );
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    /// Acquires the next back buffer.
    ///
    /// `None` would be a finding: D3D12 has no out-of-date swapchain, so the
    /// presenter never reconfigures from an acquire here, and no test asks for
    /// a frame callback that could hold one back.
    fn acquire(&mut self) -> AcquiredFrame {
        self.presenter
            .acquire()
            .unwrap_or_else(|error| panic!("acquire refused: {error}"))
            .expect("D3D12 never reports a swapchain out of date, so a frame is always there")
    }

    /// Clears `acquired` and presents it through the presenter, which keeps
    /// the command buffer in flight rather than waiting for it.
    fn draw_and_present(&mut self, acquired: &AcquiredFrame) {
        let commands = {
            let device = self.presenter.device();
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("crcbl dx12 windowed e2e frame"),
                queue: self.presenter.queue(),
            });
            let range = ImageSubresourceRange::all(self.presenter.format());
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    acquired.image,
                    range,
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                )],
                ..Barriers::default()
            });
            encoder.begin_render_pass(&RenderPassDesc {
                label: Some("clear"),
                color_attachments: &[ColorAttachment {
                    view: acquired.view,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(CLEAR),
                }],
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
                timestamp_writes: None,
            });
            encoder.end_render_pass();
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    acquired.image,
                    range,
                    ResourceState::ColorAttachment,
                    ResourceState::Present,
                )],
                ..Barriers::default()
            });
            encoder.finish().expect("recording succeeded")
        };
        let outcome = self
            .presenter
            .submit_and_present(acquired, commands)
            .unwrap_or_else(|error| panic!("submit_and_present refused: {error}"));
        assert_eq!(
            outcome,
            FrameOutcome::Presented,
            "DXGI reports nothing that makes a present reconfigure"
        );
    }

    /// Presents [`FRAMES`] frames at `extent`, pumping between them as a frame
    /// loop does, and returns every image they were drawn into.
    fn present_frames(&mut self, extent: (u32, u32)) -> Vec<u64> {
        let mut images = Vec::new();
        for _ in 0..FRAMES {
            let acquired = self.acquire();
            assert_eq!(
                acquired.extent, extent,
                "every frame is drawn at the size the swapchain is configured at"
            );
            images.push(acquired.image.to_bits());
            self.draw_and_present(&acquired);
            self.pump();
        }
        images
    }

    /// Resizes the presenter to `after` — `Presenter::resize`, the call lyr's
    /// bar made — and checks the swapchain came out of it at that size, on back
    /// buffers it had never handed out, and presents at it.
    ///
    /// `before_images` is every image the frames before the resize drew into.
    fn resize_and_present(&mut self, before: (u32, u32), after: (u32, u32), before_images: &[u64]) {
        assert_ne!(
            before, after,
            "the premise: Presenter::resize does nothing for the size it already has"
        );
        self.presenter.resize(after).unwrap_or_else(|error| {
            panic!(
                "Presenter::resize({after:?}) after presenting refused: {error}. \
                 DXGI_ERROR_INVALID_CALL from ResizeBuffers here is lyr's defect: a command \
                 buffer still in flight holds a back buffer, which the presenter must drain \
                 before it reconfigures"
            )
        });

        let acquired = self.acquire();
        assert_eq!(
            acquired.extent, after,
            "the window is {after:?} and the presenter was resized to it, so the back buffer \
             must be {after:?}. {before:?} would mean ResizeBuffers never ran: crcbl-dx12 \
             records a windowed swapchain's new extent only once it has returned success"
        );
        assert_eq!(
            self.presenter.extent(),
            after,
            "and the presenter reports the size it is now configured at"
        );
        assert!(
            !before_images.contains(&acquired.image.to_bits()),
            "the back buffer after the resize is an image handed out before it. ResizeBuffers \
             invalidates every buffer the swapchain had, and crcbl-dx12 fetches them again \
             only after it succeeds, so an old handle here means the resize never reached it"
        );
        self.draw_and_present(&acquired);
        self.present_frames(after);
    }

    /// Tears the presenter down — which drops the device on this thread, where
    /// `crcbl_dx12::debug` writes what the layer stored — and then the window.
    fn finish(self, logs: &Capture) {
        let Self {
            presenter,
            mut shell,
            window,
            ..
        } = self;
        presenter
            .destroy()
            .unwrap_or_else(|error| panic!("the presenter's teardown refused: {error}"));
        shell.destroy_window(window).expect("the window goes away");
        assert_validation_clean(logs);
    }
}

/// Asserts the D3D12 debug layer was on for this test's device and stored no
/// failure against it.
///
/// `ValidationReport::assert_clean`'s semantics, read off the log: **a layer
/// that was never on fails before the messages are looked at**, because a run
/// that passed for want of a layer proves nothing. `CRCBL_DX12_VALIDATION=1` is
/// what the harness exports, and Windows' *Graphics Tools* optional feature is
/// what the layer needs.
fn assert_validation_clean(logs: &Capture) {
    let records = logs.records();
    let said = |prefix: &str| {
        records
            .iter()
            .filter(|record| record.message.starts_with(prefix))
            .map(|record| record.message.clone())
            .collect::<Vec<_>>()
    };
    assert!(
        !said(VALIDATION_ENABLED).is_empty(),
        "crcbl-dx12 never said the D3D12 debug layer was on for this device, so a clean run \
         here would prove nothing. Run with CRCBL_DX12_VALIDATION=1, on a machine with the \
         Graphics Tools optional feature"
    );
    let teardown = said(TEARDOWN_READ);
    assert!(
        !teardown.is_empty(),
        "the device's info queue was never read back at teardown, so the layer's messages \
         were never looked at — the device was not dropped on this thread"
    );
    let complaints = said(VALIDATION_MESSAGE);
    assert!(
        complaints.is_empty(),
        "the D3D12 debug layer reported failures against this test's device:\n{}\n{}",
        complaints.join("\n"),
        teardown.join("\n")
    );
    eprintln!("crcbl dx12 windowed e2e: {}", teardown.join("; "));
}

/// **A bar resized in place by `set_panel` after presenting gets back buffers
/// at its new size.** lyr's path, call for call.
///
/// The bar opens anchored to the top edge and both side edges, presents, and
/// is then given a taller height at the bottom edge by `set_panel`, which keeps
/// the same `HWND` and therefore the same swapchain. The `Resized` that
/// answers it is what hands the new size to [`Presenter::resize`] — exactly
/// where lyr's bar hit `DXGI_ERROR_INVALID_CALL`.
///
/// The bar reserves no edge. Reserving makes it an appbar, which moves every
/// other window on the runner's desktop, and the swapchain cannot tell the
/// difference: `set_panel` resizes the window by the same `SetWindowPos` either
/// way.
#[test]
#[ignore = "opens a window on the desktop; run tests/run-dx12-windowed-e2e.sh"]
fn a_bar_resized_by_set_panel_resizes_the_presenters_buffers() {
    let logs = crcbl::core::log::capture();
    let bar = PanelDesc {
        anchor: PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT,
        ..PanelDesc::default()
    };
    let mut fixture = open(&WindowDesc {
        title: "crcbl dx12 windowed e2e bar",
        app_id: "sh.kryptic.crcbl.dx12-windowed-e2e",
        size: LogicalSize::new(0.0, BAR),
        role: WindowRole::Panel(bar),
        ..WindowDesc::default()
    });
    assert!(
        fixture.shell.caps().contains(ShellCaps::PANEL),
        "the Win32 shell builds panels"
    );
    let before = fixture.window_size();
    let before_images = fixture.present_frames(before);

    let moved = PanelDesc {
        anchor: PanelAnchor::BOTTOM | PanelAnchor::LEFT | PanelAnchor::RIGHT,
        ..bar
    };
    let window = fixture.window;
    fixture
        .shell
        .set_panel(window, LogicalSize::new(0.0, TALLER), &moved)
        .expect("set_panel on a live panel");
    let after = fixture.pump_until_resized_from(before);
    assert_eq!(
        after,
        fixture.window_size(),
        "the Resized event and the window's state agree"
    );

    fixture.resize_and_present(before, after, &before_images);
    fixture.finish(&logs);
}

/// **A window resized from outside after presenting gets back buffers at its
/// new size.**
///
/// The other way a resize arrives: `SetWindowPos` on the `HWND` from outside
/// the shell, the call a user's drag ends in. DXGI never reports a resized
/// window — a flip-model swapchain simply stretches — so the shell's `Resized`
/// is the only signal, and [`Presenter::resize`] the only answer, exactly as on
/// the bar.
#[test]
#[ignore = "opens a window on the desktop; run tests/run-dx12-windowed-e2e.sh"]
fn a_window_resized_from_outside_resizes_the_presenters_buffers() {
    let logs = crcbl::core::log::capture();
    let mut fixture = open(&WindowDesc {
        title: "crcbl dx12 windowed e2e",
        app_id: "sh.kryptic.crcbl.dx12-windowed-e2e",
        size: WINDOW,
        ..WindowDesc::default()
    });
    let before = fixture.window_size();
    let before_images = fixture.present_frames(before);

    let target = fixture
        .shell
        .surface_target(fixture.window)
        .expect("surface target");
    let SurfaceTarget::Win32 { hwnd, .. } = target else {
        unreachable!("`open` asserted a Win32 target")
    };
    outside_resize::resize_client_area(hwnd.as_ptr(), RESIZED);
    let after = fixture.pump_until_resized_from(before);
    assert_eq!(
        after,
        (RESIZED.width, RESIZED.height),
        "SetWindowPos is synchronous on a window of this process, and there is no window \
         manager to alter the size"
    );

    fixture.resize_and_present(before, after, &before_images);
    fixture.finish(&logs);
}
