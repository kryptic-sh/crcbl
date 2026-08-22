//! The **windowed** swapchain, on a real X server and a real Vulkan WSI.
//!
//! # The gap this closes
//!
//! Every GPU suite in this workspace creates a
//! [`SurfaceTarget::Offscreen`](crcbl::core::SurfaceTarget::Offscreen) surface,
//! and on `crcbl-vk` that produces a null `VkSurfaceKHR` — which is the
//! discriminator the whole backend branches on. `build_swapchain` hands off to
//! the offscreen image ring, `acquire_next_frame` returns a frame with no
//! semaphores and never calls `vkAcquireNextImageKHR`, and `present` advances a
//! cursor and never calls `vkQueuePresentKHR`. So the acquire semaphores, the
//! per-slot acquire fence, `VkPresentIdKHR`, the suboptimal flag, the
//! `oldSwapchain` handoff and `resolve_swapchain_extent` against **real**
//! `VkSurfaceCapabilitiesKHR` were reached by no test at all —
//! `tests/hal_seam_e2e.rs`'s
//! `a_surface_offers_an_srgb_format_and_preferred_format_picks_it` says so in as
//! many words, and this file is the answer.
//!
//! # Why here, and why it names Vulkan
//!
//! `crcbl` is the only crate that depends on **both** halves: `crcbl-shell`
//! makes the window and `crcbl-vk` presents to it, and neither depends on the
//! other on purpose. So the join can only be tested from above them, and this is
//! the crate that is above them.
//!
//! Unlike `tests/hal_seam_e2e.rs`, this file **names `crcbl_vk::VkInstance`**
//! rather than going through `crcbl::backend::open`. Two reasons, both about
//! evidence:
//!
//! * The instrument for "the frames presented cleanly" is
//!   [`VkInstance::validation_report`], which is a `crcbl-vk` type. There is no
//!   cross-backend equivalent — the seam's `Device::take_error` answers `None`
//!   on Vulkan by construction, because this backend reports through its return
//!   values — so a version of this file that could not name the backend could
//!   not make the assertion the whole windowed path most needs.
//! * Naming the type *is* the backend pin, and a stronger one than an
//!   environment variable: there is no configuration under which this binary
//!   silently exercises something else.
//!
//! The X11 backend is likewise named, for the reason
//! `crates/crcbl-shell/tests/run-x11-e2e.sh` names it: the registry tries
//! Wayland first, and a silent fallback would report success about a window
//! system this harness never started.
//!
//! # Why X11 is the interesting server
//!
//! `crates/crcbl-vk/src/swapchain.rs`'s module docs say it: on X11 the server
//! reports `minImageExtent == maxImageExtent == currentExtent`, so the legal
//! range for `imageExtent` is a **single point** and clamping is forced rather
//! than chosen. That was measured on this harness's own display rather than
//! taken from the docs — `vulkaninfo` under Xvfb reports all three equal on
//! `llvmpipe` — and it is what makes
//! [`the_extent_is_clamped_to_what_the_server_actually_permits`] an assertion
//! the backends' fabricated-`SurfaceCaps` unit tests cannot make.
//!
//! # Gated twice, like every other e2e suite here
//!
//! Behind the `windowed-e2e` feature *and* `#[ignore]`, so a plain
//! `cargo nextest run --workspace --all-features` on a machine with no display
//! stays green. `tests/run-windowed-e2e.sh` is the only thing that turns them
//! on, and it fails when the suite reports zero tests run —
//! `docs/plan/12-testing.md` calls a silently-skipped e2e a known trap.

#![cfg(all(target_os = "linux", feature = "windowed-e2e"))]

use core::time::Duration;
use std::time::Instant;

use crcbl::hal::{
    AcquiredFrame, Barriers, ClearValue, ColorAttachment, CommandBufferHandle, CommandEncoderDesc,
    CompositeAlpha, Device, DeviceDesc, Features, Format, ImageBarrier, ImageSubresourceRange,
    Instance, LoadOp, PresentInfo, PresentMode, QueueHandle, QueueKind, Rect2d, RenderPassDesc,
    ResourceState, SemaphoreSignal, SemaphoreWait, StoreOp, SubmitInfo, SurfaceCaps, SurfaceError,
    SurfaceHandle, SwapchainDesc, SwapchainHandle,
};
use crcbl::shell::x11_test_support::Peer;
use crcbl::shell::{
    LogicalSize, PhysicalSize, Shell, ShellBackend, ShellCaps, SurfaceTarget, WindowDesc, WindowId,
};
use crcbl_vk::VkInstance;

/// How long any single wait here may take before the test fails.
///
/// `crates/crcbl-shell/tests/x11_e2e.rs`'s figure and its argument: far larger
/// than anything needs on an idle machine, because a CI runner sharing its cores
/// can be an order of magnitude slower and still be perfectly healthy — and
/// bounded all the same, because a wait that expires here fails naming what
/// never happened, while one left to nextest is a SIGKILL with no context.
const WAIT: Duration = Duration::from_secs(20);

/// The window every test opens, in logical units.
///
/// Not the screen size, and not a multiple of anything: with no window manager
/// X11 honours the request exactly, so this number is what the *server* will
/// report as `currentExtent` and therefore what the extent assertions are
/// against.
const WINDOW: LogicalSize = LogicalSize::new(640.0, 480.0);

/// What [`a_resize_from_outside_forces_a_reconfigure_at_the_new_extent`] resizes
/// to.
///
/// Different in both dimensions from [`WINDOW`], so a backend that carried one
/// dimension over fails rather than passing on the half that happened to match.
const RESIZED: PhysicalSize = PhysicalSize::new(800, 600);

/// A distinctive clear colour, so a frame that reached the compositor is not
/// black.
///
/// Nothing reads it back — there is no readback path from a presented image —
/// so it is chosen for the log and for a human watching an Xvfb screenshot, not
/// for an assertion.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// How many frames [`a_run_of_frames_presents_with_no_validation_error`]
/// presents.
///
/// Comfortably more than any swapchain's image count, which is the point: the
/// acquire-semaphore ring and its per-slot fences only get reused — the case
/// `crcbl-vk`'s `FrameSync::acquire_armed` exists for, and the classic
/// hand-rolled-swapchain validation error — once the run is longer than the
/// ring.
const FRAMES: u32 = 32;

/// How far behind the newest present [`Windowed::draw_and_present`] waits.
///
/// `Device::wait_until_presented`'s own instruction: ask for a frame or more
/// back, never the one just submitted, because waiting on your own present
/// drains the pipeline to a single frame.
const PRESENT_LAG: u64 = 2;

/// A window on a real X server, a Vulkan device that can present to it, and a
/// swapchain configured on it.
struct Windowed {
    shell: Box<dyn Shell>,
    peer: Peer,
    window: WindowId,
    /// Captured at construction because [`Windowed::teardown`] needs it after
    /// the window is gone.
    xid: u32,
    instance: VkInstance,
    /// Emptied by [`Windowed::teardown`], which is what makes that function
    /// idempotent and lets [`Drop`] run it after a panicking test.
    device: Option<Box<dyn Device>>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// What the surface said when the swapchain was created. Kept so a
    /// reconfigure asks for the same format, image count and present mode and
    /// changes only the extent.
    caps: SurfaceCaps,
    image_count: u32,
    present_mode: PresentMode,
    /// Every command buffer submitted, destroyed together after the final
    /// `wait_idle`.
    ///
    /// A command buffer may not be destroyed until its submission has
    /// completed, and the alternative — `wait_idle` after every present —
    /// would collapse the frame overlap this suite exists to exercise.
    recorded: Vec<CommandBufferHandle>,
    /// The present counter, which is also the `present_id`. Strictly
    /// increasing, as `PresentInfo::present_id` requires.
    presented: u64,
}

impl Windowed {
    /// Opens a shell, a window and a swapchain **at the window's own size**.
    fn open() -> Self {
        Self::open_requesting(|size| size)
    }

    /// The same, with the swapchain asked for at whatever `request` makes of
    /// the window's size.
    ///
    /// The hook exists for
    /// [`the_extent_is_clamped_to_what_the_server_actually_permits`], which is
    /// the one test that deliberately asks for a size the server cannot give.
    fn open_requesting(request: impl FnOnce((u32, u32)) -> (u32, u32)) -> Self {
        crcbl::core::log::init_logging();

        let expect_wm = std::env::var("CRCBL_E2E_EXPECT_WM").is_ok();
        let deadline = Instant::now() + WAIT;
        let shell = loop {
            let shell = crcbl::shell::open_backend(ShellBackend::X11)
                .expect("the harness exported DISPLAY for a live Xvfb");
            if !expect_wm || shell.caps().contains(ShellCaps::SERVER_DECORATIONS) {
                break shell;
            }
            // A window manager takes `_NET_SUPPORTING_WM_CHECK` some time after
            // it starts and capabilities are latched per connection, so the
            // only way to wait for one is to open again. `x11_e2e.rs`'s loop,
            // for the same reason: a deadline and a poll, never a sleep.
            assert!(
                Instant::now() < deadline,
                "CRCBL_E2E_X11_WM was set but no window manager claimed the display"
            );
            drop(shell);
            std::thread::sleep(Duration::from_millis(50));
        };

        let peer = Peer::new().expect("libxcb-xtest and a second connection");
        let mut fixture = PartialWindowed { shell, peer };
        let window = fixture.create_window();
        let xid = fixture.xid(window);
        let PartialWindowed { shell, peer } = fixture;

        let size = shell
            .window_state(window)
            .expect("state")
            .size()
            .expect("the window was pumped until it was configured");
        let extent = (size.width, size.height);

        let instance = VkInstance::open().expect("the harness checked for a Vulkan loader");
        let target = shell.surface_target(window).expect("surface target");
        assert!(
            matches!(target, SurfaceTarget::Xcb { .. }),
            "the X11 shell must hand over an Xcb target, not {target:?} — every citation \
             this suite is written against branches on the surface being a real one"
        );
        // SAFETY: `target` names the live window created above, which this
        // fixture owns and which `teardown` destroys only *after* the swapchain
        // and the surface. That is the whole of `Instance::create_surface`'s
        // contract.
        let surface = unsafe { instance.create_surface(&target) }
            .expect("the Vulkan WSI accepts the shell's Xcb target");

        // **Adapter selection is surface-aware, and has to be.** Xvfb
        // advertises no DRI3, so a discrete radv GPU that enumerates first
        // cannot present to this display at all while the software rasteriser
        // behind it can. An `Err` here means "not this one"; only running out
        // of adapters is fatal. `crcbl::engine`'s `GpuContext::start_device` is
        // the shipped copy of this loop.
        let adapters = instance.adapters();
        let mut chosen = None;
        for adapter in &adapters {
            match instance.surface_caps(surface, adapter.id) {
                Ok(caps) if caps.preferred_format().is_some() => {
                    chosen = Some((adapter.clone(), caps));
                    break;
                }
                Ok(_) => eprintln!(
                    "crcbl windowed e2e: adapter {:?} offers no usable surface format",
                    adapter.name
                ),
                Err(error) => eprintln!(
                    "crcbl windowed e2e: adapter {:?} cannot present here ({error})",
                    adapter.name
                ),
            }
        }
        let (adapter, caps) = chosen.unwrap_or_else(|| {
            panic!(
                "no adapter can present to this window; {} were enumerated",
                adapters.len()
            )
        });
        // Load-bearing outside this file: `run-windowed-e2e.sh` greps for it and
        // fails when it is absent, so a run that never reached a presenting
        // device cannot report success about one.
        eprintln!(
            "crcbl windowed e2e: presenting on adapter {id} {name:?} type={kind:?}",
            id = adapter.id.0,
            name = adapter.name,
            kind = adapter.device_type,
        );

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl windowed e2e"),
                adapter: adapter.id,
                // Nothing required, so the same fixture opens on a discrete GPU
                // and on a software rasteriser. `PRESENT_FEEDBACK` is asked for
                // rather than demanded: it is what puts a `VkPresentIdKHR` on
                // every present, and a device without it still presents.
                required_features: Features::empty(),
                optional_features: Features::PRESENT_FEEDBACK,
                compatible_surface: Some(surface),
            })
            .expect("a device opens on an adapter that can present here");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("a graphics queue always exists");
        eprintln!(
            "crcbl windowed e2e: present feedback {}",
            if device.caps().supports(Features::PRESENT_FEEDBACK) {
                "granted, so every present carries a VkPresentIdKHR"
            } else {
                "unavailable, so presents go unnumbered"
            }
        );

        let format = caps.preferred_format().expect("just checked");
        let present_mode = caps.choose_present_mode(&[PresentMode::Fifo]);
        let image_count = caps
            .min_image_count
            .saturating_add(1)
            .min(caps.max_image_count);
        let requested = request(extent);
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("crcbl windowed e2e swapchain"),
                surface,
                format,
                extent: requested,
                image_count,
                present_mode,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("a swapchain is created on a live window");

        Self {
            shell,
            peer,
            window,
            xid,
            instance,
            device: Some(device),
            surface,
            swapchain,
            queue,
            format,
            caps,
            image_count,
            present_mode,
            recorded: Vec::new(),
            presented: 0,
        }
    }

    fn device(&self) -> &dyn Device {
        self.device
            .as_deref()
            .expect("`finish` has already destroyed this fixture's device")
    }

    /// The window's size as the shell reports it, which is the authority the
    /// seam's obligation 1 names.
    fn window_size(&self) -> (u32, u32) {
        let size = self
            .shell
            .window_state(self.window)
            .expect("state")
            .size()
            .expect("configured");
        (size.width, size.height)
    }

    /// One turn of the loop: the peer, then the shell.
    fn pump(&mut self) {
        self.peer.service();
        self.shell.pump(&mut |_| {});
    }

    /// Pumps until `ready`, or fails naming what never happened.
    fn pump_until(&mut self, what: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if ready(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {WAIT:?} waiting for {what}; the shell reports the window \
                 at {:?}",
                self.shell
                    .window_state(self.window)
                    .ok()
                    .and_then(|state| state.size()),
            );
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    /// Acquires the next image, or fails saying which `SurfaceError` came back.
    ///
    /// A refusal is never swallowed: `OutOfDate` is expected traffic in a frame
    /// loop and would be a finding here, because every acquire in this file is
    /// made at a moment the fixture has already established the window and the
    /// swapchain agree.
    fn acquire(&self) -> AcquiredFrame {
        self.device()
            .acquire_next_frame(self.swapchain)
            .unwrap_or_else(|error| panic!("acquire_next_frame refused: {error}"))
    }

    /// Clears `acquired` and presents it, waiting on and signalling the
    /// swapchain's own semaphores.
    ///
    /// The submission shape is `crcbl::engine`'s `submit_and_present`: wait on
    /// [`AcquiredFrame::acquire_semaphore`] before the first write, signal
    /// [`AcquiredFrame::present_semaphore`] from the last, and hand that same
    /// semaphore to the present. On a windowed Vulkan swapchain both are
    /// `Some`, and a submission that skipped either is a validation error the
    /// report at [`Windowed::finish`] catches.
    fn draw_and_present(&mut self, acquired: &AcquiredFrame) {
        let commands = {
            let device = self.device();
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("crcbl windowed e2e frame"),
                queue: self.queue,
            });
            let range = ImageSubresourceRange::all(self.format);
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    acquired.image,
                    range,
                    // A swapchain image's contents on acquire are undefined, so
                    // `Undefined` is the only correct source; loading one is
                    // never right.
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
                // The extent the swapchain was *configured* at, never the one it
                // was asked for — obligation 3, and the reason the field exists.
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
        self.recorded.push(commands);

        let waits: Vec<SemaphoreWait> = acquired
            .acquire_semaphore
            .map(|semaphore| SemaphoreWait {
                semaphore,
                value: 0,
            })
            .into_iter()
            .collect();
        let signals: Vec<SemaphoreSignal> = acquired
            .present_semaphore
            .map(|semaphore| SemaphoreSignal {
                semaphore,
                value: 0,
            })
            .into_iter()
            .collect();
        self.device()
            .submit(
                self.queue,
                &SubmitInfo {
                    command_buffers: &[commands],
                    waits: &waits,
                    signals: &signals,
                },
            )
            .expect("submit");

        self.presented += 1;
        let present_id = self.presented;
        self.device()
            .present(
                self.queue,
                &PresentInfo {
                    swapchain: self.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: Some(present_id),
                },
            )
            .unwrap_or_else(|error| panic!("present refused: {error}"));

        // Pace on the display rather than on a clock, which is the only thing
        // that drives `VkPresentWaitKHR` — and therefore the only thing that
        // makes the `VkPresentIdKHR` on the present above mean anything. A
        // device without `PRESENT_FEEDBACK` returns immediately by contract, and
        // a timeout is expected traffic rather than a failure.
        if present_id > PRESENT_LAG {
            match self
                .device()
                .wait_until_presented(self.swapchain, present_id - PRESENT_LAG, WAIT)
            {
                Ok(()) | Err(SurfaceError::Timeout) => {}
                Err(error) => panic!("wait_until_presented refused: {error}"),
            }
        }
    }

    /// Reconfigures the swapchain at `extent`, changing nothing else.
    fn reconfigure(&mut self, extent: (u32, u32)) {
        self.device()
            .reconfigure_swapchain(
                self.swapchain,
                &SwapchainDesc {
                    label: Some("crcbl windowed e2e swapchain"),
                    surface: self.surface,
                    format: self.format,
                    extent,
                    image_count: self.image_count,
                    present_mode: self.present_mode,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .unwrap_or_else(|error| panic!("reconfigure_swapchain refused: {error}"));
        // The numbering restarts with the new swapchain, which
        // `PresentInfo::present_id` states as a rule: a backend is free to key
        // state off the number, so carrying the old counter over would be a
        // caller bug this fixture would be committing on every resize.
        self.presented = 0;
    }

    /// Tears everything down in the order the seam's obligation 2 requires, and
    /// leaves the fixture safe to drop.
    ///
    /// Idempotent: [`Windowed::finish`] calls it, and so does [`Drop`] after a
    /// panicking test that never reached `finish`.
    fn teardown(&mut self) {
        let Some(device) = self.device.take() else {
            return;
        };
        let _ = device.wait_idle();
        for commands in self.recorded.drain(..) {
            device.destroy_command_buffer(commands);
        }
        device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(device);

        // **A process that exits with a window still mapped is not what a game
        // does**, and `crates/crcbl-shell/tests/x11_e2e.rs` documents what
        // `openbox` does with a client that simply disappears: it is left with
        // `_NET_ACTIVE_WINDOW` naming an XID no longer in `_NET_CLIENT_LIST`,
        // and focuses nothing new from then on. `set_visible(false)` is the
        // ICCCM withdrawal and `destroy_window` is the rest of it.
        let _ = self.shell.set_visible(self.window, false);
        let _ = self.shell.destroy_window(self.window);
        if !self.shell.caps().contains(ShellCaps::SERVER_DECORATIONS) {
            return;
        }
        // Wait for the manager to have *acted* on the withdrawal rather than
        // merely to have been told. Bounded, and it gives up quietly: this runs
        // on the failure path too, where a second panic would destroy the
        // output the failing test is trying to produce.
        let deadline = Instant::now() + WAIT;
        let root = self.peer.root();
        while Instant::now() < deadline {
            self.shell.pump(&mut |_| {});
            let listed = self
                .peer
                .window_property(root, "_NET_CLIENT_LIST")
                .unwrap_or_default();
            let still_there = listed
                .chunks_exact(4)
                .any(|word| u32::from_ne_bytes(word.try_into().expect("four bytes")) == self.xid);
            if !still_there {
                return;
            }
        }
    }

    /// Tears down, then asserts the validation layer saw nothing.
    ///
    /// This is the assertion the whole windowed path most needs and the one no
    /// offscreen suite can make: acquire semaphores, their per-slot fences, the
    /// image layouts a present requires and the `oldSwapchain` handoff are all
    /// things the driver accepts silently and the layer refuses. `assert_clean`
    /// also fails when the layer was never **loaded**, so a machine without it
    /// cannot report a clean run about a check that never happened.
    fn finish(mut self) {
        self.teardown();
        self.instance.validation_report().assert_clean();
    }
}

impl Drop for Windowed {
    fn drop(&mut self) {
        self.teardown();
        if !std::thread::panicking() {
            return;
        }
        // What `finish` would have said, on the runs that never reach it.
        // Nothing here may panic: the thread is already unwinding, and a second
        // panic aborts the process and destroys this output.
        let report = self.instance.validation_report();
        if report.enabled && !report.is_clean() {
            eprintln!(
                "crcbl windowed e2e: the validation layer reported {} error(s) and {} \
                 warning(s) before the failure above:\n{}",
                report.errors,
                report.warnings,
                report.summary()
            );
        }
    }
}

/// The shell and the peer, before there is a window to put in [`Windowed`].
///
/// Exists so window creation can be written once against `&mut` borrows of both
/// halves; folding it into [`Windowed::open_requesting`] would mean building the
/// fixture around a window that does not exist yet.
struct PartialWindowed {
    shell: Box<dyn Shell>,
    peer: Peer,
}

impl PartialWindowed {
    /// Creates the window and waits until the window system has given it a size.
    ///
    /// `Shell::create_window` documents this as a hard ordering constraint: a
    /// window has no size until the window system says so, and a swapchain needs
    /// an extent.
    fn create_window(&mut self) -> WindowId {
        let managed = self.shell.caps().contains(ShellCaps::SERVER_DECORATIONS);
        let window = self
            .shell
            .create_window(&WindowDesc {
                title: "crcbl windowed e2e",
                app_id: "sh.kryptic.crcbl.windowed-e2e",
                size: WINDOW,
                ..WindowDesc::default()
            })
            .expect("create_window");

        let deadline = Instant::now() + WAIT;
        loop {
            self.peer.service();
            self.shell.pump(&mut |_| {});
            let configured = self
                .shell
                .window_state(window)
                .is_ok_and(|state| state.is_configured() && (!managed || state.visible));
            if configured {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {WAIT:?} waiting for the first configuration"
            );
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }

        // **With a window manager, a configured window is not yet a settled
        // one**: `openbox` reparents, frames, focuses and writes properties on
        // its own schedule, and the last of that lands after the map. The
        // surface's `currentExtent` follows the window, so a swapchain created
        // mid-reparent would be sized against a geometry about to change. There
        // is no event that says "the manager has finished", so quiet is the
        // only available definition.
        if managed {
            self.settle();
        }
        window
    }

    /// Pumps until the display has stopped producing events, or gives up at the
    /// deadline.
    fn settle(&mut self) {
        /// Consecutive silent pumps that count as quiet. Each is followed by a
        /// 10 ms wait, so this is a tenth of a second of nothing.
        const QUIET_TURNS: u32 = 10;
        let deadline = Instant::now() + WAIT;
        let mut quiet = 0;
        while quiet < QUIET_TURNS && Instant::now() < deadline {
            let mut seen = 0_u32;
            self.peer.service();
            self.shell.pump(&mut |_| seen += 1);
            if seen == 0 {
                quiet += 1;
            } else {
                quiet = 0;
            }
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    /// The window's XID, which is what the peer has to name it by.
    fn xid(&self, window: WindowId) -> u32 {
        match self.shell.surface_target(window).expect("surface target") {
            SurfaceTarget::Xcb { window, .. } => window,
            other => panic!("the X11 shell handed over {other:?}"),
        }
    }
}

/// **The swapchain under test is a real WSI swapchain, not the offscreen ring.**
///
/// The observable, twice over, and each half is a value that reads differently
/// on the two paths:
///
/// * [`SurfaceCaps::current_extent`] is `Some(the window's size)`.
///   `crcbl-vk`'s `offscreen_surface_caps` answers `None` unconditionally —
///   "nothing here has an opinion about the size" — so a run that had somehow
///   ended up on the ring cannot produce a `Some` here at all, let alone the
///   right one. This is also the only place in the workspace where
///   `VkSurfaceCapabilitiesKHR` came from a window system rather than from a
///   fabricated struct in a unit test.
/// * [`AcquiredFrame::acquire_semaphore`] and
///   [`AcquiredFrame::present_semaphore`] are both `Some`. The offscreen arm of
///   `acquire_next_frame` returns `entry.frame(index, None, false)` and exits
///   before `vkAcquireNextImageKHR` — no semaphores, no acquire fence, no
///   acquire. So a `Some` here is the WSI acquire having run.
///
/// The second half is a claim about *this* backend rather than about the seam:
/// `AcquiredFrame`'s docs allow `None` on a backend with an implicit acquire,
/// and `crcbl-webgpu` is one. This binary names `crcbl_vk::VkInstance`, which is
/// what makes the stronger assertion sound here — see the module docs.
#[test]
#[ignore = "needs an X server and a Vulkan loader; run tests/run-windowed-e2e.sh"]
fn a_windowed_swapchain_is_not_the_offscreen_ring() {
    let fixture = Windowed::open();
    let size = fixture.window_size();

    assert_eq!(
        fixture.caps.current_extent,
        Some(size),
        "the surface must report the window's own size. An offscreen ring answers `None` \
         here, always, so a `None` means this suite is exercising the very path it exists \
         to get away from."
    );

    let acquired = fixture.acquire();
    assert!(
        acquired.acquire_semaphore.is_some(),
        "a windowed Vulkan acquire hands back the semaphore `vkAcquireNextImageKHR` \
         signalled. `None` is the offscreen ring's answer — it returns before the acquire \
         is ever made — so this is the assertion that says the WSI path ran."
    );
    assert!(
        acquired.present_semaphore.is_some(),
        "and the one `vkQueuePresentKHR` waits on, which the ring likewise has no use for"
    );
    assert_eq!(
        acquired.extent, size,
        "a swapchain asked for the window's size is configured at the window's size"
    );

    fixture.finish();
}

/// **The extent comes back clamped to what the server permits, not echoed.**
///
/// The observable: [`AcquiredFrame::extent`] is the *window's* size after a
/// swapchain was deliberately asked for a different one. On X11 the server
/// reports `minImageExtent == maxImageExtent == currentExtent`, so the legal
/// range for `imageExtent` is a single point and there is no swapchain at the
/// requested size to be had — `crcbl-vk`'s `resolve_swapchain_extent` clamps,
/// and `AcquiredFrame::extent` is where it reports what it did.
///
/// **This is the assertion the fabricated-`SurfaceCaps` unit tests cannot
/// make.** They can and do check that the clamp arithmetic is right for a range
/// the test wrote down; only a real server can say what the range *is*, and
/// only here does a wrong answer mean a real window rendered at the wrong size.
///
/// The offset is odd in both dimensions so a backend that rounded, aligned or
/// halved something would not land on the answer by luck, and it is added rather
/// than subtracted because a smaller request is legal on window systems whose
/// range is wide (Wayland's) and this test wants the case that is illegal
/// everywhere.
#[test]
#[ignore = "needs an X server and a Vulkan loader; run tests/run-windowed-e2e.sh"]
fn the_extent_is_clamped_to_what_the_server_actually_permits() {
    /// What is added to each of the window's dimensions.
    const OVERSHOOT: (u32, u32) = (137, 91);

    let mut requested = None;
    let mut fixture = Windowed::open_requesting(|size| {
        let asked = (size.0 + OVERSHOOT.0, size.1 + OVERSHOOT.1);
        requested = Some(asked);
        asked
    });
    let requested = requested.expect("the hook ran");
    let size = fixture.window_size();
    assert_ne!(
        requested, size,
        "the premise: this test is only about anything if the two differ"
    );

    let acquired = fixture.acquire();
    assert_eq!(
        acquired.extent, size,
        "the swapchain was asked for {requested:?} on a {size:?} window. On X11 \
         minImageExtent == maxImageExtent == currentExtent, so {requested:?} is not a size \
         a VkSwapchainKHR legally exists at — the backend must clamp and must report the \
         size it configured on AcquiredFrame::extent. Getting {requested:?} back means the \
         request was echoed, which is a render area, viewport and scissor that do not match \
         the image."
    );

    // And then actually draw with it. This is the only acquire in the tree
    // whose extent is not the one that was asked for, so it is the only place
    // the render area can be checked against a genuinely clamped image rather
    // than against one where both numbers happen to agree. `draw_and_present`
    // sets `render_area` from `acquired.extent`; the validation layer is what
    // reads the two against each other, and `finish` is what fails if it
    // complained.
    fixture.draw_and_present(&acquired);

    fixture.finish();
}

/// **A run of frames reaches the display, and the validation layer sees nothing
/// wrong with how.**
///
/// The observables are two:
///
/// * The ring **rotated**: over [`FRAMES`] acquires, more than one distinct
///   [`AcquiredFrame::index`] came back. A swapchain that handed out one image
///   forever would present the frame currently on screen, and every other
///   assertion in this test would still pass.
/// * The validation report is **clean and enabled**, checked by
///   [`Windowed::finish`]. That is what covers everything a return value cannot:
///   an acquire semaphore reused while its acquire is still pending (the classic
///   hand-rolled-swapchain error, which `crcbl-vk`'s per-slot acquire fence
///   exists to prevent and which only shows up once the run is longer than the
///   ring), an image presented from the wrong layout, a present without a wait.
///
/// [`FRAMES`] is what makes the first of those reachable: the fence-guarded slot
/// reuse in `acquire_next_frame` is dead code on any run shorter than the
/// swapchain's image count.
#[test]
#[ignore = "needs an X server and a Vulkan loader; run tests/run-windowed-e2e.sh"]
fn a_run_of_frames_presents_with_no_validation_error() {
    let mut fixture = Windowed::open();
    let mut indices = Vec::new();

    for _ in 0..FRAMES {
        let acquired = fixture.acquire();
        indices.push(acquired.index);
        fixture.draw_and_present(&acquired);
        // The window system is still talking while frames go out — a real frame
        // loop pumps, and a client that never reads its socket is one whose X
        // connection eventually backs up.
        fixture.pump();
    }

    let mut distinct = indices.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() > 1,
        "{FRAMES} acquires all returned image index {:?}. A swapchain that never rotates \
         presents the image the display is already showing, and nothing else here would \
         notice.",
        distinct
    );
    eprintln!(
        "crcbl windowed e2e: {FRAMES} frames presented over {} swapchain images",
        distinct.len()
    );

    fixture.finish();
}

/// **A resize from outside forces a reconfigure, and the new swapchain reports
/// the new size.**
///
/// The observable: [`AcquiredFrame::extent`] after the reconfigure is
/// [`RESIZED`], where before it was the window's original size. Both are read
/// from the same field, so a backend that reconfigured nothing — or that
/// rebuilt at the old geometry — fails on the value rather than on a return
/// code.
///
/// This is the `oldSwapchain` path: `reconfigure_swapchain` hands the live
/// `VkSwapchainKHR` to `vkCreateSwapchainKHR` as `oldSwapchain` so the driver
/// can reuse its images and keep presenting through the handoff, then retires it
/// once the device is idle. Nothing offscreen reaches it — the ring's
/// "reconfigure" allocates plain images — and it is where the surface
/// reference-counting bug found on the first resize under sway lived.
///
/// The resize is driven by [`Peer::resize`] because on X11 that is who does it:
/// a window manager or a user drag, both of them other programs. And frames are
/// presented on **both** sides of it, so the assertion is about a swapchain that
/// was working before and is working after, not about one that was merely
/// created twice.
#[test]
#[ignore = "needs an X server and a Vulkan loader; run tests/run-windowed-e2e.sh"]
fn a_resize_from_outside_forces_a_reconfigure_at_the_new_extent() {
    let mut fixture = Windowed::open();
    let before = fixture.window_size();
    assert_ne!(
        before,
        (RESIZED.width, RESIZED.height),
        "the premise: the window has to actually change size"
    );

    let acquired = fixture.acquire();
    assert_eq!(acquired.extent, before);
    fixture.draw_and_present(&acquired);

    let xid = fixture.xid;
    // **Asked again on every turn, because a `ConfigureRequest` is a request.**
    // A window manager owns the geometry of what it manages, and ICCCM lets it
    // grant, alter or ignore any single request; nothing acknowledges one that
    // went nowhere, so a client that asks once and then waits cannot tell "not
    // yet" from "never". `openbox` does honour this size — the sandbox suite's
    // `a_resize_from_outside_is_reported_exactly_once` asserts the same 800x600
    // arrives, exactly once — but a run under CI load reached this deadline
    // with the window still at its original extent, which is the shape of a
    // request that was dropped rather than one that was answered differently.
    // Re-asking costs nothing and cannot confuse the result: a request for the
    // size the window already has is one the manager grants by changing
    // nothing.
    fixture.pump_until("the shell to report the resize", |fixture| {
        if fixture
            .shell
            .window_state(fixture.window)
            .is_ok_and(|state| state.size() == Some(RESIZED))
        {
            return true;
        }
        fixture.peer.resize(xid, RESIZED.width, RESIZED.height);
        false
    });

    // The seam's obligation 1: the shell's size is what a swapchain is
    // configured at. Read back from the shell rather than reused from `RESIZED`,
    // so this is the number the window system actually settled on.
    let after = fixture.window_size();
    fixture.reconfigure(after);

    let acquired = fixture.acquire();
    assert_eq!(
        acquired.extent, after,
        "the window is {after:?} and the swapchain was reconfigured at {after:?}, so the \
         acquired frame must be {after:?}. {before:?} would mean the reconfigure did not \
         reach the surface — the handle stays valid across one, which is exactly what makes \
         a no-op invisible to every return code."
    );
    assert!(
        acquired.acquire_semaphore.is_some(),
        "and the swapchain that came out of the handoff is still a windowed one"
    );
    fixture.draw_and_present(&acquired);

    fixture.finish();
}
