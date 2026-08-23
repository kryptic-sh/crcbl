//! The fixture the backend-agnostic GPU suites open with — not a test module,
//! and not a test target either: `tests/gpu_scene/` holds no `main.rs`, so Cargo
//! compiles nothing here on its own.
//!
//! **Three suites pull this in with `#[path]`** — `tests/draw_gen_e2e/`,
//! `tests/forward_e2e/` and `tests/sprite_e2e/` — because they open the same
//! device and read back frames from the same ring, and a second copy of that is
//! a second place a fix has to land. Each names itself in a `SUITE` constant at
//! its own crate root, which is what every line this file prints and every debug
//! label it sets is built from; there is nothing else suite-specific in here.
//!
//! The mesh scene the first two draw is **not** here: it is
//! `tests/gpu_scene/mesh_scene.rs`, which those two pull in as a second
//! `#[path]` module. The sprite suite renders no mesh at all — it draws through
//! `crcbl::render::SpriteRenderer` at a **pinned** format, which is what
//! [`Headless::open_at_format`] exists for — so a fixture and a scene in one
//! file made every scene symbol dead code in that binary.
//!
//! [`Headless`] is an offscreen surface, a device and a swapchain-shaped image
//! ring, opened on whatever [`crcbl::backend::open`] selects. It is the
//! backend-agnostic twin of `crcbl-vk`'s `vk_e2e/harness.rs` fixture, and the
//! two differ in exactly two places:
//!
//! * **The adapter is chosen through [`crcbl::adapter`]** rather than by taking
//!   the first one enumerated, so `CRCBL_ADAPTER` pins a device class here the
//!   way it does everywhere else in `crates/crcbl/tests/`.
//! * **[`Headless::finish`] asserts on [`Device::take_error`]**, not on a
//!   validation report. There is no cross-backend equivalent of a Vulkan
//!   validation layer; the seam's own out-of-band error channel is what every
//!   backend has. `tests/hal_seam_e2e.rs` records the same substitution and the
//!   same caveat: on WebGPU this is where a failure the return value did not
//!   carry actually arrives; on Vulkan it answers only when
//!   `CRCBL_VK_VALIDATION_FATAL` is set, and on Metal and D3D12 the trait's
//!   default answers `None`.
//!
//! [`select_adapter`] prints a `<SUITE>: device on adapter …` line, and that
//! line is load-bearing outside this file: each suite's runner greps the first
//! one to report which device really ran and to check that a `CRCBL_ADAPTER` it
//! exported actually arrived. Rewording it turns a green suite into a failed
//! harness run.

use core::ops::Deref;
use std::time::{Duration, Instant};

use crcbl::adapter::ADAPTER_ENV_VAR;
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::core::SurfaceTarget;
use crcbl::hal::{
    CompositeAlpha, Device, DeviceDesc, Features, Format, HalError, Instance, PresentMode,
    ReadbackDesc, ReadbackState, SwapchainDesc,
};

/// The byte every readback destination is filled with before it is polled.
///
/// Deliberately not `0`. A frame that legitimately rendered black reads back as
/// zeroes, and so does a destination no copy ever reached — an ambiguity that
/// makes the failing assertion unable to say whether the frame was empty or the
/// readback never landed. This value is neither a plausible pixel nor a
/// plausible counter, so a byte of it surviving into an assertion is evidence
/// that nothing was copied over it.
pub(crate) const POISON: u8 = 0xA5;

/// A readback destination of `len` bytes, filled with [`POISON`].
pub(crate) fn poisoned(len: usize) -> Vec<u8> {
    vec![POISON; len]
}

/// How long [`Headless::readback`] polls before it declares the copy lost.
const READBACK_DEADLINE: Duration = Duration::from_secs(30);

/// Opens whatever backend `CRCBL_GPU` names.
///
/// Also where the process gets a logger: `crcbl-core` already owns a `log::Log`
/// and its `CRCBL_LOG` filtering, so this is the engine's own sink rather than a
/// second one written for the tests. Idempotent, and it loses gracefully to a
/// logger already installed.
fn instance() -> Box<dyn Instance> {
    crcbl::core::log::init_logging();
    let instance = crcbl::backend::open().expect("a backend opens");
    assert_backend_matches_the_pin(instance.as_ref());
    instance
}

/// **The backend that opened is the backend that was asked for.**
///
/// The one failure this suite cannot survive silently: every test here is meant
/// to pass on every backend, so a fallback produces a green run that is evidence
/// about a backend nobody named. Both names go through [`GpuBackend::from_name`]
/// rather than a second table.
fn assert_backend_matches_the_pin(instance: &dyn Instance) {
    let Ok(requested) = std::env::var(BACKEND_ENV_VAR) else {
        return;
    };
    let opened = GpuBackend::from_name(&instance.backend().to_string())
        .expect("every backend the registry can open has a GpuBackend spelling");
    assert_eq!(
        Some(opened),
        GpuBackend::from_name(&requested),
        "{BACKEND_ENV_VAR}={requested} was asked for and {} answered",
        instance.backend()
    );
}

/// The adapter [`ADAPTER_ENV_VAR`] names, announced on stderr.
///
/// See this file's header for why the line matters outside it.
fn select_adapter(instance: &dyn Instance) -> crcbl::hal::AdapterInfo {
    let adapters = instance.adapters();
    let pin = crcbl::adapter::pin();
    let adapter = crcbl::adapter::select(pin.as_deref(), &adapters)
        .unwrap_or_else(|miss| panic!("{miss}"))
        .clone();
    eprintln!(
        "{suite}: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        suite = crate::SUITE,
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = pin.as_deref().unwrap_or("<unset>"),
    );
    adapter
}

/// The fixture's device, in a slot [`Headless::finish`] can empty.
///
/// `Headless` has a [`Drop`] impl — the failure path's diagnostics — and a type
/// with one cannot have a field moved out of it. The [`Deref`] is what keeps
/// every `headless.device.…` reading as it did in the suite this came from.
pub(crate) struct DeviceSlot(Option<Box<dyn Device>>);

impl DeviceSlot {
    fn new(device: Box<dyn Device>) -> Self {
        Self(Some(device))
    }

    /// Destroys the device. Emptying the slot is the point: a second call does
    /// nothing, and every later deref panics with the message below rather than
    /// reaching a dangling handle.
    fn destroy(&mut self) {
        drop(self.0.take());
    }
}

impl Deref for DeviceSlot {
    type Target = Box<dyn Device>;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("`Headless::finish` has already destroyed this fixture's device")
    }
}

/// An offscreen surface, a device, and a swapchain-shaped image ring.
pub(crate) struct Headless {
    pub(crate) device: DeviceSlot,
    /// The offscreen surface the ring hangs off. `pub(crate)` because a test
    /// that *reconfigures* the swapchain has to name it again —
    /// `tests/mesh_e2e/resize.rs` is the one that does.
    pub(crate) surface: crcbl::hal::SurfaceHandle,
    pub(crate) swapchain: crcbl::hal::SwapchainHandle,
    pub(crate) queue: crcbl::hal::QueueHandle,
    pub(crate) format: Format,
    /// Destroyed last, and therefore declared last: a field is dropped in
    /// declaration order, and the surface handle above it names an object this
    /// owns.
    instance: Box<dyn Instance>,
}

impl Headless {
    /// A ring of `extent`, with `optional_features` asked for and none required,
    /// at `pinned_format` or at whatever the surface prefers.
    ///
    /// `None` takes [`SurfaceCaps::preferred_format`](crcbl::hal::SurfaceCaps),
    /// which is what the mesh scenes want: no image they draw is committed and
    /// every pair of frames they compare goes through one choice.
    ///
    /// `Some` is for a suite that *does* commit an image and therefore cannot
    /// let the ring pick — `tests/sprite_e2e/` pins `Rgba8UnormSrgb`, because
    /// its sheets are uploaded as sRGB and its alpha blend is only in linear
    /// light if the target decodes too. The pin is asserted against the caps
    /// rather than passed through: a backend that stopped offering it would
    /// otherwise surface as a swapchain-creation failure with no name on it.
    pub(crate) fn open_at_format(
        extent: (u32, u32),
        pinned_format: Option<Format>,
        optional_features: Features,
    ) -> Self {
        let instance = instance();
        let adapter = select_adapter(instance.as_ref());

        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. `finish` tears the swapchain down
        // before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("the offscreen ring reports its own caps");
        let format = match pinned_format {
            Some(pinned) => {
                assert!(
                    caps.formats.contains(&pinned),
                    "the offscreen ring on {backend} offers {offered:?} and this suite pinned \
                     {pinned:?}. Every backend's offscreen caps list the same five formats on \
                     purpose, so one that dropped this is a divergence rather than a test that \
                     asked for too much.",
                    backend = instance.backend(),
                    offered = caps.formats,
                );
                pinned
            }
            // The ring's preferred format, which is what the mesh scenes want:
            // no image they draw is committed, and every pair of frames they
            // compare is drawn through the same choice.
            None => caps.preferred_format().expect("some format is offered"),
        };

        let device = instance
            .create_device(&DeviceDesc {
                label: Some(crate::SUITE),
                adapter: adapter.id,
                // Nothing is required, so the same fixture opens on a discrete
                // GPU and on a software rasteriser and the tests branch on what
                // actually came back.
                required_features: Features::empty(),
                optional_features,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl::hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");

        let ring = format!("{} ring", crate::SUITE);
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some(&ring),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");

        Self {
            device: DeviceSlot::new(device),
            surface,
            swapchain,
            queue,
            format,
            instance,
        }
    }

    /// Reads `size` bytes of `staging` back into `out`, polling with a deadline
    /// rather than sleeping — `docs/plan/12-testing.md`.
    pub(crate) fn readback(&self, staging: crcbl::hal::BufferHandle, size: u64, out: &mut [u8]) {
        let device = self.device.as_ref();
        let label = format!("{} readback", crate::SUITE);
        let readback = device
            .request_readback(&ReadbackDesc {
                label: Some(&label),
                buffer: staging,
                offset: 0,
                size,
                after: None,
            })
            .expect("a readback request");
        let started = Instant::now();
        let deadline = started + READBACK_DEADLINE;
        loop {
            match device
                .poll_readback(readback, out)
                .expect("the readback did not fail")
            {
                ReadbackState::Ready => break,
                ReadbackState::Pending => assert!(
                    Instant::now() < deadline,
                    "the {size}-byte readback was still Pending after {:?}, past the \
                     {READBACK_DEADLINE:?} this polls for. Nothing was copied into the \
                     destination, so it still holds {POISON:#04x} and every byte a caller \
                     reads out of it is this harness's fill and not a frame.",
                    started.elapsed(),
                ),
            }
            std::thread::yield_now();
        }
        device.destroy_readback(readback);
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires, then asserts
    /// the device reported nothing out of band.
    ///
    /// **Callers must end with this rather than dropping the fixture.** It is
    /// what the Vulkan original's `validation_report().assert_clean()` stands in
    /// for here; a test that drops the fixture instead never asks the device
    /// what it saw.
    pub(crate) fn finish(mut self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        if let Some(error) = self.device.take_error() {
            panic!(
                "the device reported out of band: {error}. Every call in this test returned \
                 success, so this is a failure the return values did not carry."
            );
        }
        self.device.destroy();
    }
}

impl Drop for Headless {
    /// The failure path's copy of what [`Headless::finish`] reports.
    ///
    /// `finish` is the last line of a test, so a test that panics never reaches
    /// it and the device's verdict is discarded on exactly the runs that go red.
    /// This prints it instead, and prints nothing at all otherwise.
    ///
    /// **Nothing here may panic.** It runs while the thread is already
    /// unwinding, and a second panic aborts the process, destroying the output
    /// this exists to produce.
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let suite = crate::SUITE;
        eprintln!(
            "{suite}: the fixture was dropped by a panicking test, so `finish` never ran. \
             What it can still see:"
        );
        match self.device.0.as_ref() {
            // The distinction a flake needs: a lost device is a driver-side
            // failure that makes every other symptom downstream noise, and it is
            // otherwise invisible because nothing in a failing test asks.
            Some(device) => {
                match device.wait_idle() {
                    Ok(()) => eprintln!(
                        "{suite}:   wait_idle: Ok — the device is alive and idle, so the \
                         submission completed and the failure is in what it produced"
                    ),
                    Err(HalError::DeviceLost(detail)) => eprintln!(
                        "{suite}:   wait_idle: device lost ({detail}) — nothing this test \
                         read back means anything"
                    ),
                    Err(error) => eprintln!("{suite}:   wait_idle: {error}"),
                }
                match device.take_error() {
                    Some(error) => eprintln!("{suite}:   out-of-band error: {error}"),
                    None => eprintln!("{suite}:   out-of-band error: none"),
                }
            }
            None => {
                eprintln!("{suite}:   wait_idle: not asked, `finish` already destroyed the device")
            }
        }
    }
}
